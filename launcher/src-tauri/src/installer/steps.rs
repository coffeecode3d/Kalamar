//! The steps, one per stage of `Install-Splat3Recomp.ps1` (A checks, B prerequisites, B2 sources and Python, C hactool,
//! D ExeFS + version check + merged RomFS, E lift, F compile, G shader translator, H save keys, I layout, J self-checks),
//! with the same commands and checks; plus the plan shown before anything is installed, and the uninstall.

use super::*;

fn s(p: &Path) -> String {
    p.display().to_string()
}

/// A failure of the portable tools' download or unpacking, with the step's log.
#[cfg(windows)]
fn portable_fail(e: portable::Error, log: &Path) -> Fail {
    let f = match e {
        portable::Error::Cancelled => return Fail::cancelled(),
        portable::Error::Network { url, message } => Fail::new("download_failed").with("url", url).with("message", message),
        portable::Error::Mismatch { name, .. } => Fail::new("download_corrupt").with("file", name),
        portable::Error::Io { what, message } => Fail::new("io").with("what", what).with("message", message),
        portable::Error::Archive { name, message } => Fail::new("archive_bad").with("file", name).with("message", message),
    };
    Fail { log: Some(log.to_path_buf()), ..f }
}

impl Run {
    // ---- A: checks --------------------------------------------------------------------------------------------
    pub(super) fn step_checks(&mut self) -> StepResult {
        #[cfg(windows)]
        {
            let arch = |v: &str| std::env::var(v).is_ok_and(|a| a.eq_ignore_ascii_case("ARM64"));
            if arch("PROCESSOR_ARCHITECTURE") || arch("PROCESSOR_ARCHITEW6432") {
                return Err(Fail::new("arm64"));
            }
        }
        if !sys::cpu_has_v3() {
            return Err(Fail::new("cpu_v3"));
        }
        let mut warnings: Vec<String> = Vec::new();
        if !needs_player_files(&self.lay) {
            // An existing install (the wizard's or the pre-release installer's) already holds the extracted ExeFS and the merged RomFS:
            // nothing needs the player's dumps or keys again.
            self.log("    the extracted game code and data are already here: your files are not needed");
            return self.checks_machine(warnings);
        }
        let files = [("base", &self.req.base_nsp), ("update", &self.req.update_nsp), ("keys", &self.req.prod_keys)];
        for (which, p) in files {
            if p.trim().is_empty() || !Path::new(p).is_file() {
                return Err(Fail::new("file_missing").with("which", which).with("path", p));
            }
        }
        let base = nsp::check_base(Path::new(&self.req.base_nsp));
        if base.status == "bad" {
            return Err(Fail::new("base_bad").with("check", base.code));
        }
        let upd = nsp::check_update(Path::new(&self.req.update_nsp));
        if upd.status == "bad" {
            let mut f = Fail::new("update_bad").with("check", upd.code);
            f.params.extend(upd.params.clone());
            return Err(f);
        }
        let keys = nsp::check_keys(Path::new(&self.req.prod_keys));
        if keys.status == "bad" {
            return Err(Fail::new("keys_bad").with("check", keys.code));
        }
        self.nsp_sizes = (base.program_size.unwrap_or(0), upd.program_size.unwrap_or(0));
        self.update_known = upd.status == "ok";
        self.log("    your three files are present (they are only read, never copied into the install)");
        if base.status == "warn" {
            warnings.push("base_unverified".to_string());
        }
        if upd.status == "warn" {
            warnings.push("update_unverified".to_string());
        }
        self.checks_machine(warnings)
    }

    fn checks_machine(&mut self, mut warnings: Vec<String>) -> StepResult {
        std::fs::create_dir_all(&self.lay.root).map_err(|e| Fail::io("install_dir", e))?;
        // Everything this run will add, at its peak (each step checks its own share again before it starts).
        self.check_space(StepId::Prereqs)?;
        let ram = sys::total_ram().unwrap_or(0);
        self.log(&format!("    memory: {:.1} GB, {} compile jobs", ram as f64 / (1u64 << 30) as f64, self.jobs));
        if ram > 0 && ram < (8u64 << 30) - (256 << 20) {
            return Err(Fail::new("ram").with("ram", format!("{:.0}", ram as f64 / (1u64 << 30) as f64)));
        }
        if ram > 0 && ram < (16u64 << 30) - (512 << 20) {
            warnings.push("ram_low".into());
        }
        #[cfg(windows)]
        {
            let sysroot = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
            if !sysroot.join("System32").join("vulkan-1.dll").is_file() {
                warnings.push("vulkan_missing".into());
            }
        }
        #[cfg(not(windows))]
        {
            // The loader is what the game dlopens; without it nothing renders (engine/docs/building.md, "Linux").
            let known = ["/usr/lib/libvulkan.so.1", "/usr/lib/x86_64-linux-gnu/libvulkan.so.1", "/usr/lib64/libvulkan.so.1"].iter().any(|p| Path::new(p).exists());
            let ldconfig = std::process::Command::new("ldconfig").arg("-p").output().is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("libvulkan.so.1"));
            if !known && !ldconfig {
                warnings.push("vulkan_missing".into());
            }
        }
        // Outside the profile, a new folder takes its parent's permissions: on most drives every account of the PC may
        // change the programs the launcher later runs from it. Said, not refused.
        if !inside_profile(&self.lay.root, crate::config::user_profile().as_deref()) {
            warnings.push("outside_profile".into());
        }
        self.update(true, |s| s.warnings = warnings);
        Ok(Flow::Done)
    }

    // ---- B: the build tools ---------------------------------------------------------------------------------------
    /// The compiler, Ninja and the libraries. Portable (every first install): downloaded into `<I>\tools` beside the
    /// next steps, joined before the compile. MSYS2 (an install whose objects it compiled): nothing to do.
    pub(super) fn step_prereqs(&mut self) -> StepResult {
        if self.toolchain == Toolchain::Msys2 {
            self.log("    MSYS2 UCRT64 stays the compiler of this install (its objects were compiled with it)");
            return Ok(Flow::Skipped);
        }
        #[cfg(not(windows))]
        {
            // Linux builds with the system's clang, lld, Ninja, Python and libraries: nothing is downloaded into the
            // install folder, and nothing is installed without the player's own package manager (the plan lists what
            // is missing, engine/docs/building.md "Linux").
            let missing = prereq::system_missing();
            if missing.is_empty() {
                self.log("    the system's clang, lld, Ninja and libraries are there (nothing to download)");
                return Ok(Flow::Skipped);
            }
            return Err(Fail::new("linux_tools_missing").with("list", prereq::missing_packages().join(", ")));
        }
        #[cfg(windows)]
        {
            if portable::toolchain_ready(&self.lay.root) {
                return Ok(Flow::Skipped);
            }
            self.check_space(StepId::Prereqs)?;
            let root = self.lay.root.clone();
            let log = self.lay.logs().join("outils.log");
            let total = portable::bytes(&portable::TOOLCHAIN);
            self.log(&format!("    downloading llvm-mingw {}, Ninja and the libraries ({:.0} MB) into {}, beside the next steps", portable::LLVM_VERSION, total as f64 / 1e6, self.lay.tools().display()));
            self.spawn_background_work(
                StepId::Prereqs,
                Box::new(move |halt, prog| {
                    let mut lf = std::fs::OpenOptions::new().create(true).append(true).open(&log).ok();
                    let mut note = |line: &str| {
                        if let Some(f) = lf.as_mut() {
                            let _ = writeln!(f, "[{}] {line}", now_stamp());
                        }
                    };
                    for p in portable::TOOLCHAIN {
                        note(&format!("pinned {} ({} bytes, sha256 {}) from {}", p.name, p.size, p.sha256, p.url));
                    }
                    let t = Instant::now();
                    let mut last_stage = String::new();
                    let r = portable::ensure_toolchain(&root, halt, &mut |stage, f, bytes| {
                        let mut p = prog.lock().unwrap_or_else(|e| e.into_inner());
                        p.fraction = Some(f);
                        p.stage = Some(stage.to_string());
                        p.detail = bytes.map(|(d, t)| format!("{:.0}/{:.0}", d as f64 / 1e6, t as f64 / 1e6));
                        if stage != last_stage {
                            last_stage = stage.to_string();
                        }
                    });
                    match &r {
                        Ok(_) => note(&format!("toolchain ready in {:.1} s: {}", t.elapsed().as_secs_f64(), portable::toolchain_id())),
                        Err(e) => note(&format!("failed after {:.1} s: {e}", t.elapsed().as_secs_f64())),
                    }
                    r.map(|did| if did { Flow::Done } else { Flow::Skipped }).map_err(|e| portable_fail(e, &log))
                }),
            )
        }
    }

    // ---- B2: sources ------------------------------------------------------------------------------------------
    pub(super) fn step_sources(&mut self) -> StepResult {
        let commit = self.commit();
        if self.is_done(StepId::Sources, &commit) {
            return Ok(Flow::Skipped);
        }
        let (Some(dir), Some(man)) = (self.bundle_dir.clone(), self.manifest.clone()) else { return Err(Fail::new("bundle_missing")) };
        self.progress(StepId::Sources, None, Some("extract".into()), None);
        let gz = std::fs::read(dir.join(bundle::ARCHIVE_NAME)).map_err(|e| Fail::io("bundle", e))?;
        if bundle::sha256_hex(&gz) != man.sha256 {
            return Err(Fail::new("bundle_corrupt"));
        }
        let src = self.lay.src();
        let fresh = self.lay.root.join("src.new");
        sys::remove_all(&fresh).map_err(|e| Fail::io("src.new", e))?;
        let n = bundle::extract(&gz, &fresh).map_err(|e| Fail::io("extract", e))?;
        // A file the new source did not change keeps its old timestamp, so the next compile is incremental: Ninja only
        // rebuilds what changed (extraction would otherwise make every header look new and every object stale).
        let kept = keep_unchanged_mtimes(&src, &fresh);
        self.log(&format!("    {kept} unchanged files keep their timestamps"));
        // Keep the hactool already built (it does not depend on the runtime source), and the save keys read from the
        // game (their own input digest says whether the keys step reads them again).
        let tc = src.join("toolchain");
        if tc.is_dir() {
            let _ = std::fs::rename(&tc, fresh.join("toolchain"));
        }
        let keys = self.lay.keys_json();
        if keys.is_file() {
            let to = fresh.join(incremental::KEYS_JSON);
            if to.parent().is_some_and(Path::is_dir) {
                let _ = std::fs::rename(&keys, &to);
            }
        }
        sys::remove_all(&src).map_err(|e| Fail::io("src", e))?;
        std::fs::rename(&fresh, &src).map_err(|e| Fail::io("src", e))?;
        bundle::write_marker(&src, &man).map_err(|e| Fail::io("marker", e))?;
        self.log(&format!("    runtime source {} extracted: {n} files", man.commit));
        Ok(Flow::Done)
    }

    // ---- B2: Python environment -------------------------------------------------------------------------------
    /// Python 3.14.9 (python.org's embeddable build) with the five packages the steps import, in `<I>\tools\python`;
    /// nothing is installed on the system. An older install's virtual environment (`work\venv`) is kept while it works.
    pub(super) fn step_python(&mut self) -> StepResult {
        let ready = |p: &Path| p.is_file() && prereq::run_capture(p, &["-I", "-c", "import cryptography, numpy, lz4.block; from compression import zstd"], Duration::from_secs(60)).is_some_and(|(c, _)| c == 0);
        let root = self.lay.root.clone();
        #[cfg(not(windows))]
        {
            // The system's python3 builds a virtual environment in the install folder: nothing is installed outside it.
            let venv = self.lay.venv();
            let vpy = self.lay.venv_py();
            if ready(&vpy) {
                return Ok(Flow::Skipped);
            }
            let Some(base) = prereq::find_python_system() else { return Err(Fail::new("linux_python_missing")) };
            self.check_space(StepId::Python)?;
            if !vpy.is_file() {
                self.progress(StepId::Python, Some(0.1), Some("venv".into()), None);
                let args: Vec<String> = vec!["-m".into(), "venv".into(), s(&venv), "--clear".into()];
                self.exec_ok(StepId::Python, &base, &args, &root, "python", &mut |_, _| {})?;
            }
            let req = self.lay.src().join("tools").join("requirements-installer.txt");
            if !req.is_file() {
                return Err(Fail::io("requirements-installer.txt", std::io::Error::from(std::io::ErrorKind::NotFound)));
            }
            self.progress(StepId::Python, Some(0.4), Some("pip".into()), None);
            let args = vec![
                "-m".into(),
                "pip".into(),
                "install".into(),
                "--no-compile".into(),
                "--disable-pip-version-check".into(),
                "--only-binary".into(),
                ":all:".into(),
                "-r".into(),
                s(&req),
            ];
            match self.exec(StepId::Python, &vpy, &args, &root, "python", false, &mut |_, _| {}, &mut |_| {}) {
                Ok(0) => {}
                _ => return Err(self.fail_log("python_env", "python")),
            }
            if !ready(&vpy) {
                return Err(self.fail_log("python_env", "python"));
            }
            self.log("    Python environment ready (system python3, packages installed in work/venv)");
            return Ok(Flow::Done);
        }
        #[cfg(windows)]
        {
            let portable_py = portable::python_exe(&root);
            if portable::python_ready(&root) && ready(&portable_py) {
                return Ok(Flow::Skipped);
            }
            if !portable::python_ready(&root) && ready(&self.lay.venv_py()) {
                self.log("    the Python environment of this install (work\\venv) is kept");
                return Ok(Flow::Skipped);
            }
            self.check_space(StepId::Python)?;
            let log = self.lay.logs().join("python.log");
            let mut lf = std::fs::OpenOptions::new().create(true).append(true).open(&log).ok();
            for p in portable::python_pins() {
                if let Some(f) = lf.as_mut() {
                    let _ = writeln!(f, "[{}] pinned {} ({} bytes, sha256 {}) from {}", now_stamp(), p.name, p.size, p.sha256, p.url);
                }
            }
            let cancel = self.inst.cancel.clone();
            let r = portable::ensure_python(&root, &cancel, &mut |stage, f, bytes| {
                self.progress(StepId::Python, Some(f), Some(stage.to_string()), bytes.map(|(d, t)| format!("{:.0}/{:.0}", d as f64 / 1e6, t as f64 / 1e6)));
            });
            if let Some(f) = lf.as_mut() {
                let _ = writeln!(f, "[{}] {}", now_stamp(), match &r { Ok(_) => format!("ready: {}", portable::python_id()), Err(e) => format!("failed: {e}") });
            }
            r.map_err(|e| portable_fail(e, &log))?;
            if !ready(&portable_py) {
                return Err(self.fail_log("python_env", "python"));
            }
            Ok(Flow::Done)
        }
    }

    // ---- D: ExeFS and the version check -----------------------------------------------------------------------
    pub(super) fn step_exefs(&mut self) -> StepResult {
        let targets = std::fs::read_to_string(self.lay.src().join("recomp").join("targets.py")).map_err(|e| Fail::io("targets.py", e))?;
        let Some(expected) = expected_main_sha(&targets, TARGET) else { return Err(Fail::new("target_missing")) };
        let exefs = self.lay.exefs();
        let main = exefs.join("main");
        let existed = main.is_file();
        if !existed {
            sys::remove_all(&exefs).map_err(|e| Fail::io("exefs", e))?;
            let args = vec![
                "-I".into(),
                s(&self.lay.kit_tools().join("s3_extract.py")),
                self.req.update_nsp.clone(),
                self.req.prod_keys.clone(),
                s(&exefs),
                s(&self.lay.vendor()),
            ];
            let (vpy, root) = (self.venv_py(), self.lay.root.clone());
            self.progress(StepId::Exefs, None, Some("extract".into()), None);
            let code = self.exec(StepId::Exefs, &vpy, &args, &root, "exefs-extract", false, &mut |_, _| {}, &mut |_| {})?;
            if code != 0 || !main.is_file() {
                let _ = sys::remove_all(&exefs);
                let mut f = self.fail_log(if code == 2 { "keys_cannot_decrypt" } else { "extract_failed" }, "exefs-extract");
                f.exit_code = Some(code);
                return Err(f);
            }
        }
        self.progress(StepId::Exefs, Some(0.9), Some("verify".into()), None);
        let actual = file_sha256(&main).map_err(|e| Fail::io("main", e))?;
        self.log(&format!("    SHA-256 of main: {actual}"));
        if actual != expected {
            let _ = sys::remove_all(&exefs);
            let _ = std::fs::remove_file(self.lay.game().join("extraction-manifest.json"));
            return Err(Fail::new("version_mismatch").with("actual", &actual[..16]).with("expected", &expected[..16]));
        }
        self.log("    update verified: Splatoon 3 11.2.0");
        Ok(if existed { Flow::Skipped } else { Flow::Done })
    }

    // ---- C: hactool -------------------------------------------------------------------------------------------
    /// hactool as the launcher ships it (built from the engine's sources, with `range.patch`), copied to `tools\bin`
    /// with its license texts; without a shipped one, built the old way (MSYS2 gcc, Git).
    pub(super) fn step_hactool(&mut self) -> StepResult {
        if self.is_done(StepId::Hactool, "") {
            return Ok(Flow::Skipped);
        }
        if let Some(t) = self.shipped.as_ref().filter(|t| t.has(prebuilt::HACTOOL)) {
            t.install(prebuilt::HACTOOL, &self.lay.hactool()).map_err(|e| Fail::io("hactool", e))?;
            let n = t.install_licences(&self.lay.tools().join("licenses")).map_err(|e| Fail::io("licenses", e))?;
            self.log(&format!("    hactool shipped with the launcher ({}), {n} licence texts in tools\\licenses", t.manifest.hactool));
            return Ok(Flow::Done);
        }
        let (vpy, src) = (self.venv_py(), self.lay.src());
        self.progress(StepId::Hactool, None, Some("build".into()), None);
        self.exec_ok(StepId::Hactool, &vpy, &[s(&src.join("tools").join("build_hactool.py"))], &src, "hactool", &mut |_, _| {})?;
        if !self.lay.hactool_built().is_file() {
            return Err(self.fail_log("hactool_missing", "hactool"));
        }
        let checkout = src.join("toolchain").join("src");
        self.remove_intermediate(&checkout);
        Ok(Flow::Done)
    }

    // ---- D: merged RomFS --------------------------------------------------------------------------------------
    pub(super) fn step_romfs(&mut self) -> StepResult {
        if self.lay.romfs_done() {
            return Ok(Flow::Skipped);
        }
        self.remove_merge_temps();
        let romfs = self.lay.romfs();
        sys::remove_all(&romfs).map_err(|e| Fail::io("romfs", e))?;
        std::fs::create_dir_all(self.lay.game()).map_err(|e| Fail::io("game", e))?;
        self.check_space(StepId::Romfs)?;
        let in_place = self.merge_in_place();
        let hactool = if self.shipped.is_some() { self.lay.hactool() } else { self.lay.hactool_built() };
        if !hactool.is_file() {
            return Err(Fail::new("hactool_missing"));
        }
        let kt = self.lay.kit_tools();
        let mut args = vec![
            "-I".into(),
            s(&kt.join("s3_merge.py")),
            self.req.base_nsp.clone(),
            self.req.update_nsp.clone(),
            self.req.prod_keys.clone(),
            s(&hactool),
            s(&romfs),
            s(&self.lay.vendor()),
        ];
        if in_place {
            // hactool reads the two Program NCAs inside the NSP files: no copy is written.
            args.push("--in-place".into());
        }
        let (base, upd) = self.nsp_sizes;
        let copies = if in_place { 0 } else { (base + upd).max(1) };
        let image = if self.update_known { nsp::ROMFS_SIZE_1120 } else { upd + upd / 3 };
        let total = (copies + image) as f64;
        let lay = self.lay.clone();
        let (vpy, root) = (self.venv_py(), self.lay.root.clone());
        let tick = move |p: &mut Prog| {
            let temps = lay.merge_temps();
            if temps.is_empty() {
                if p.fraction.is_some() {
                    p.stage = Some("index".into());
                    p.fraction = Some(0.97);
                }
                return;
            }
            // The image is written there by hactool, still open: its size from the file itself.
            let written: u64 = temps.iter().map(|t| sys::dir_size_live(t)).sum();
            p.stage = Some(if written < copies { "copy".into() } else { "hactool".into() });
            p.fraction = Some((written as f64 / total * 0.93).min(0.93));
            p.detail = Some(format!("{:.1}/{:.1}", written as f64 / 1e9, total / 1e9));
        };
        let lay = self.lay.clone();
        let after = move |code: i32, log: PathBuf| -> StepResult {
            // The temporary folder holds a key file derived from prod.keys: it goes whatever happened.
            for t in lay.merge_temps() {
                let _ = sys::remove_all(&t);
            }
            if code != 0 || !lay.romfs_done() {
                let _ = sys::remove_all(&lay.romfs());
                return Err(Fail { log: Some(log), exit_code: Some(code), ..Fail::new("merge_failed") });
            }
            Ok(Flow::Done)
        };
        // Beside the lift, which reads only the ExeFS; the compile waits for it.
        self.spawn_background(StepId::Romfs, &vpy, &args, &root, "romfs-merge", Box::new(tick), Box::new(after))
    }

    // ---- E: lift ----------------------------------------------------------------------------------------------
    pub(super) fn step_lift(&mut self) -> StepResult {
        let commit = self.commit();
        if self.is_done(StepId::Lift, &commit) {
            return Ok(Flow::Skipped);
        }
        // The generator reads the ExeFS and writes `lift\` only: the game of this install may run meanwhile (an update
        // played while it builds). It then runs below normal priority, so the game keeps the CPU it needs.
        if self.game_running() {
            self.low_priority_now = true;
            self.log("    the game of this install is open: the generator runs below normal priority");
        }
        let lift = self.lay.lift();
        // The generated C already there stays: the generator rewrites only the chunks whose text changes (atomically),
        // so the others keep their dates and Ninja does not recompile them. Until the lift ends its digest is unknown.
        self.state.lift_inputs.clear();
        self.save_state();
        let (vpy, src) = (self.venv_py(), self.lay.src());
        let args = vec![
            s(&src.join("recomp").join("gen.py")),
            "--exefs".into(),
            s(&self.lay.exefs()),
            "--out".into(),
            s(&lift),
            "--target".into(),
            TARGET.into(),
            "--jobs".into(),
            lift_jobs(sys::cpus(), sys::available_ram()).to_string(),
        ];
        self.progress(StepId::Lift, Some(0.0), Some("discover".into()), None);
        self.exec_ok(StepId::Lift, &vpy, &args, &src, "lift", &mut |l, p| {
            if let Some((n, t)) = progress::lift_fraction(l) {
                p.fraction = Some(0.06 + 0.94 * n as f64 / t as f64);
                p.stage = Some("emit".into());
                p.detail = Some(format!("{n}/{t}"));
            } else if l.starts_with("emitting with") {
                p.fraction = Some(0.06);
                p.stage = Some("emit".into());
            }
        })?;
        if !self.lay.lift_outputs() {
            return Err(self.fail_log("lift_incomplete", "lift"));
        }
        let (written, chunks) = lift_rewritten(&lift);
        self.log(&format!("    {written} of {chunks} generated files rewritten (the others keep their dates: not recompiled)"));
        self.state.lift_inputs = self.lift_hash.clone();
        Ok(Flow::Done)
    }

    // ---- F: compile and link ----------------------------------------------------------------------------------
    pub(super) fn step_compile(&mut self) -> StepResult {
        let commit = self.commit();
        if self.is_done(StepId::Compile, &commit) {
            return Ok(Flow::Skipped);
        }
        if !self.lay.lift_outputs() {
            return Err(Fail::new("lift_incomplete"));
        }
        let (vpy, src, g) = (self.venv_py(), self.lay.src(), self.lay.game());
        if self.portable() && !portable::toolchain_ready(&self.lay.root) {
            return Err(Fail::new("toolchain_missing"));
        }
        let mut args: Vec<String> = vec![
            s(&self.lay.src().join("tools").join("build_game.py")),
            "--version".into(),
            TARGET.into(),
            "--gen".into(),
            s(&self.lay.lift()),
            "--out".into(),
            s(&self.lay.build()),
            "--exefs".into(),
            s(&self.lay.exefs()),
            "--romfs".into(),
            s(&self.lay.romfs()),
            "--profile".into(),
            s(&g.join("userdata").join("profile")),
            "--jobs".into(),
            self.jobs.to_string(),
        ];
        if self.portable() {
            args.extend(portable::build_args(&self.lay.root));
        }
        // More jobs than cores: the second thread of each core compiles too, below normal priority so the PC stays
        // usable (it costs nothing while the PC is otherwise idle). The same while the game of this install is played.
        let playing = self.game_running();
        self.low_priority_now = self.jobs as usize > sys::physical_cores() || playing;
        if self.jobs as usize > sys::physical_cores() {
            self.log(&format!("    {} jobs on {} cores: the compile runs below normal priority", self.jobs, sys::physical_cores()));
        } else if playing {
            self.log("    the game of this install is open: the compile runs below normal priority");
        }
        // Until the compile ends, the executable is not known to match anything.
        self.state.compile_inputs.clear();
        self.state.toolchain = self.toolchain.id();
        self.save_state();
        if self.lay.game_exe().is_file() {
            self.prepare_previous()?;
        }
        self.progress(StepId::Compile, Some(0.0), Some("prepare".into()), None);
        // The objects first, then the link, when something has to be waited for between them:
        // * the RomFS merge may still run: no object needs the RomFS, only its index (one runtime file) and the link do.
        //   So every object is compiled beside the merge first (--compile-only, with a placeholder index), then the merge
        //   is waited for, then the normal run below recompiles the one file the index changes and links;
        // * an update (a game executable in place): the game may be played meanwhile, opened now or at any moment of the
        //   compile. Every object compiles beside it; only the link, which replaces its executable, waits for it to be
        //   closed. (The link alone would not stop for it: it renames the new executable over the open one, measured,
        //   and the DLLs it copies after are the ones a running game holds.) It costs one more short run of the build
        //   tool, a few seconds, and launches wait only during the link run itself.
        let merging = self.bg.iter().any(|b| b.id == StepId::Romfs);
        let update = self.lay.game_exe().is_file();
        let two_runs = merging || playing || update;
        let (mut compiled, mut linked) = (0u64, false);
        if two_runs {
            self.log(if merging {
                "    the RomFS merge still runs: compiling the objects beside it"
            } else if playing {
                "    the game of this install is open: compiling the objects while it runs, the link waits for it to be closed"
            } else {
                "    an update: the objects first, the link once no game of this install is open"
            });
            let mut first = args.clone();
            first.push("--compile-only".into());
            let r = self.exec_ok(StepId::Compile, &vpy, &first, &src, "compile", &mut |l, p| {
                if let Some(f) = progress::compile_progress(l, p.fraction) {
                    let (n, t) = progress::ninja_fraction(l).unwrap_or((0, 0));
                    compiled = compiled.max(n);
                    p.fraction = Some(f);
                    p.stage = Some("compile".into());
                    p.detail = Some(format!("{n}/{t}"));
                }
            });
            if let Err(e) = r {
                let _ = settle_previous(&self.lay);
                return Err(e);
            }
            if let Some(stop) = self.join_background_where(&|b: &Background| b.id == StepId::Romfs) {
                // The merge failed: its failure is the one reported (Run::execute takes it from pending_stop).
                let _ = settle_previous(&self.lay);
                self.pending_stop = Some(stop);
                return Err(Fail::new("bg_failed"));
            }
        }
        // The link replaces the executable: it waits for the game of this install to be closed. A game started from
        // outside the launcher during the link run itself (the launcher refuses launches then) makes it fail when it
        // copies the DLLs that game holds: it waits for that one and runs again (Ninja relinks only what it must). An
        // update never fails because the game is open.
        let mut attempt = 0;
        let r = loop {
            if let Err(e) = self.wait_for_no_game(StepId::Compile) {
                let _ = settle_previous(&self.lay);
                return Err(e);
            }
            if two_runs || attempt > 0 {
                self.progress(StepId::Compile, Some(0.97), Some("link".into()), None);
            }
            let r = self.exec_ok(StepId::Compile, &vpy, &args, &src, "compile", &mut |l, p| {
                if let Some(f) = progress::compile_progress(l, p.fraction) {
                    p.fraction = Some(f);
                    let (n, t) = progress::ninja_fraction(l).unwrap_or((0, 0));
                    if l.contains("LINK") {
                        linked = true;
                        p.stage = Some("link".into());
                        p.detail = None;
                    } else {
                        compiled = compiled.max(n);
                        p.stage = Some("compile".into());
                        p.detail = Some(format!("{n}/{t}"));
                    }
                }
            });
            match r {
                Err(f) if !f.cancelled && f.code == "command_failed" && attempt < 3 && self.game_running() => {
                    attempt += 1;
                    self.log("    the link run stopped while the game of this install was open (started meanwhile): waiting for it to be closed, then running it again");
                }
                other => break other,
            }
        };
        self.low_priority_now = false;
        // The executable was relinked (the kept one becomes the rollback) or not (nothing to keep); on a failure the
        // link did not replace it.
        match settle_previous(&self.lay) {
            Some(true) => {
                self.swapped = true;
                self.state.previous_commit = previous_mark(&self.lay).0;
                self.log(&format!("    previous game kept in {}", previous_dir(&self.lay).display()));
            }
            Some(false) => self.log("    the game executable did not change"),
            None => {}
        }
        r?;
        if !self.lay.game_exe().is_file() {
            return Err(self.fail_log("exe_missing", "compile"));
        }
        self.log(&format!("    {compiled} objects compiled, {}", if linked { "executable linked" } else { "nothing to link" }));
        self.state.compile_inputs = self.compile_hash();
        if !self.keep_build {
            let (lift, obj) = (self.lay.lift(), self.lay.build().join("obj"));
            self.remove_intermediate(&lift);
            self.remove_intermediate(&obj);
            self.state.lift_inputs.clear();
        }
        Ok(Flow::Done)
    }

    // ---- G: shader translator ---------------------------------------------------------------------------------
    pub(super) fn step_xlate(&mut self) -> StepResult {
        let commit = self.commit();
        if self.is_done(StepId::Xlate, &commit) {
            return Ok(Flow::Skipped);
        }
        // The translator the launcher ships, built from the very sources of the runtime source it installs.
        if let Some(t) = self.shipped.as_ref().filter(|t| t.translator_for(&self.xlate_hash)).cloned() {
            let xl = self.lay.xlate();
            std::fs::create_dir_all(&xl).map_err(|e| Fail::io("xlate", e))?;
            // A game started while its translator changes would pick up the old one or none: launches wait the instant
            // this takes.
            self.update(true, |s| s.replacing = true);
            if self.lay.xlate_exe().is_file() {
                self.stash_previous_xlate()?;
            }
            t.install(prebuilt::XLATE_EXE, &self.lay.xlate_exe()).map_err(|e| Fail::io("xlate", e))?;
            t.install(prebuilt::XLATE_ID, &xl.join("totk_xlate.id")).map_err(|e| Fail::io("xlate", e))?;
            self.update(true, |s| s.replacing = false);
            self.log(&format!("    shader translator shipped with the launcher (identity {}, engine {})", &t.manifest.xlate_id[..t.manifest.xlate_id.len().min(16)], t.manifest.engine_commit));
            self.state.xlate_inputs = self.xlate_hash.clone();
            return Ok(Flow::Done);
        }
        // Otherwise built here, the old way: needs Git, the .NET 10 SDK and the Visual Studio C++ build tools.
        let Some(git) = self.tool_path("git").or_else(prereq::find_git) else { return Err(Fail::new("xlate_tools_missing")) };
        if prereq::find_dotnet().is_none() {
            return Err(Fail::new("xlate_tools_missing"));
        }
        #[cfg(windows)]
        if prereq::find_vs_cpp().is_none() {
            return Err(Fail::new("xlate_tools_missing"));
        }
        let mng = self.lay.mng();
        let root = self.lay.root.clone();
        if !mng.join("vendor").join("ryujinx").is_dir() {
            self.progress(StepId::Xlate, Some(0.02), Some("fetch".into()), None);
            sys::remove_all(&mng).map_err(|e| Fail::io("MacNativeGames", e))?;
            std::fs::create_dir_all(&mng).map_err(|e| Fail::io("MacNativeGames", e))?;
            let m = s(&mng);
            for (log, args) in [
                ("mng-init", vec!["init".to_string(), "-q".into(), m.clone()]),
                ("mng-remote", vec!["-C".into(), m.clone(), "remote".into(), "add".into(), "origin".into(), MNG_URL.into()]),
                ("mng-fetch", vec!["-C".into(), m.clone(), "fetch".into(), "-q".into(), "--depth".into(), "1".into(), "origin".into(), MNG_PIN.into()]),
                ("mng-checkout", vec!["-C".into(), m.clone(), "checkout".into(), "-q".into(), "FETCH_HEAD".into()]),
            ] {
                self.exec_ok(StepId::Xlate, &git, &args, &root, log, &mut |_, _| {})?;
            }
        }
        let xl = self.lay.xlate();
        std::fs::create_dir_all(&xl).map_err(|e| Fail::io("xlate", e))?;
        if self.lay.xlate_exe().is_file() {
            self.stash_previous_xlate()?;
        }
        std::fs::create_dir_all(self.lay.nuget().join("packages")).map_err(|e| Fail::io("nuget", e))?;
        std::fs::write(xl.join("NuGet.Config"), NUGET_CONFIG).map_err(|e| Fail::io("NuGet.Config", e))?;
        let (vpy, src) = (self.venv_py(), self.lay.src());
        self.progress(StepId::Xlate, Some(0.3), Some("build".into()), None);
        self.exec_ok(StepId::Xlate, &vpy, &[s(&src.join("tools").join("build_xlate.py")), "--out".into(), s(&xl)], &src, "shader-translator", &mut |l, p| {
            let l = l.trim();
            if l.starts_with("applied ") {
                p.fraction = Some(p.fraction.unwrap_or(0.3).max(0.35));
            } else if l.contains("Generating native code") {
                p.fraction = Some(0.75);
                p.stage = Some("native".into());
            } else if (l.contains(".dll") || l.contains(".so")) && l.contains("->") {
                p.fraction = Some(p.fraction.unwrap_or(0.4).max(0.6));
            }
        })?;
        // Stop any .NET build server still running (they keep files of the NuGet cache open).
        if let Some(dotnet) = self.tool_path("dotnet") {
            let _ = self.exec(StepId::Xlate, &dotnet, &["build-server".into(), "shutdown".into()], &src, "dotnet-build-server", false, &mut |_, _| {}, &mut |_| {});
        }
        if !self.lay.xlate_exe().is_file() {
            return Err(self.fail_log("xlate_missing", "shader-translator"));
        }
        for p in [self.lay.mng(), self.lay.nuget(), xl.join("ryujinx"), xl.join("xlate"), xl.join("publish")] {
            self.remove_intermediate(&p);
        }
        self.state.xlate_inputs = self.xlate_hash.clone();
        Ok(Flow::Done)
    }

    // ---- H: save keys -----------------------------------------------------------------------------------------
    pub(super) fn step_keys(&mut self) -> StepResult {
        let commit = self.commit();
        if self.is_done(StepId::Keys, &commit) {
            return Ok(Flow::Skipped);
        }
        let (vpy, root) = (self.venv_py(), self.lay.root.clone());
        let args = vec![
            s(&self.lay.kit_tools().join("profile").join("s3save.py")),
            "keys".into(),
            "--exefs".into(),
            s(&self.lay.exefs()),
            "--romfs".into(),
            s(&self.lay.romfs()),
        ];
        self.state.keys_inputs.clear();
        self.exec_ok(StepId::Keys, &vpy, &args, &root, "save-keys", &mut |_, _| {})?;
        if !self.lay.keys_json().is_file() {
            return Err(self.fail_log("keys_json_missing", "save-keys"));
        }
        self.state.keys_inputs = self.keys_hash.clone();
        Ok(Flow::Done)
    }

    // ---- I: layout --------------------------------------------------------------------------------------------
    pub(super) fn step_layout(&mut self) -> StepResult {
        for d in [self.lay.userdata().join("logs"), self.lay.userdata().join("profile")] {
            std::fs::create_dir_all(&d).map_err(|e| Fail::io("userdata", e))?;
        }
        let needs = [self.lay.game_exe(), self.lay.romfs().join("romfs.bin"), self.lay.romfs().join("index.json"), self.lay.xlate_exe()];
        if let Some(m) = needs.iter().find(|p| !p.is_file()) {
            return Err(Fail::new("layout_missing").with("path", s(m)));
        }
        (self.on_root)(&self.lay.game());
        self.log(&format!("    the launcher now uses {}", self.lay.game().display()));
        Ok(Flow::Done)
    }

    // ---- J: self-checks ---------------------------------------------------------------------------------------
    pub(super) fn step_selftest(&mut self) -> StepResult {
        let probe = self.lay.userdata().join("selfcheck");
        std::fs::create_dir_all(&probe).map_err(|e| Fail::io("selfcheck", e))?;
        let exe = self.lay.game_exe();
        for (i, flag) in ["--selftest", "--probe-profile"].iter().enumerate() {
            let log = format!("verify{}", flag.replace("--", "-"));
            self.progress(StepId::Selftest, Some(0.1 + 0.45 * i as f64), Some(flag.trim_start_matches('-').replace('-', "_")), None);
            let start = std::fs::metadata(self.lay.logs().join(format!("{log}.log"))).map(|m| m.len()).unwrap_or(0);
            let code = self.exec(StepId::Selftest, &exe, &[flag.to_string()], &probe, &log, false, &mut |_, _| {}, &mut |_| {})?;
            let text = std::fs::read(self.lay.logs().join(format!("{log}.log"))).map(|b| String::from_utf8_lossy(&b[(start as usize).min(b.len())..]).into_owned()).unwrap_or_default();
            if code != 0 || selfcheck_failed(&text) {
                let mut f = self.fail_log("selftest_failed", &log).with("check", flag);
                f.exit_code = Some(code);
                return Err(f);
            }
            self.log(&format!("    {flag}: OK"));
        }
        let _ = sys::remove_all(&probe);
        Ok(Flow::Done)
    }

    // ---- intermediates ----------------------------------------------------------------------------------------
    pub(super) fn step_cleanup(&mut self) -> StepResult {
        self.remove_merge_temps();
        // The generated C and the objects stay for the next update unless the player asked to free the space.
        let keep = [self.lay.lift(), self.lay.build().join("obj")];
        for p in self.lay.intermediates() {
            if self.keep_build && keep.contains(&p) {
                continue;
            }
            if p == self.lay.lift() && p.exists() {
                self.state.lift_inputs.clear();
            }
            self.remove_intermediate(&p);
        }
        // A finished update keeps the previous game for "Revert to the previous version" (build-previous).
        self.swapped = false;
        self.xlate_swapped = false;
        Ok(Flow::Done)
    }
}

// ---- existing installs: adoption, safe update, rollback -------------------------------------------------------

/// The player's dumps and keys are needed only to extract the ExeFS and merge the RomFS; an install that has both
/// (the wizard's or the pre-release installer's) is updated without them.
pub fn needs_player_files(lay: &Layout) -> bool {
    !(lay.exefs().join("main").is_file() && lay.romfs_done())
}

/// For a folder made by the pre-release installer (no state file of the wizard yet, its own `src\` still in place): records which
/// sources its generated C and its translator were made from, so the update reuses them when the bundled source has
/// the same ones.
pub(super) fn adopt_records(state: &mut StateFile, lay: &Layout) {
    let pristine = lay.src().join("recomp").is_dir() && bundle::load_marker(&lay.src()).is_none();
    if !pristine {
        return;
    }
    if state.lift_inputs.is_empty() && lay.lift_outputs() {
        state.lift_inputs = bundle::inputs_hash_dir(&lay.src(), bundle::LIFT_INPUTS);
    }
    if state.xlate_inputs.is_empty() && lay.xlate_exe().is_file() {
        state.xlate_inputs = bundle::inputs_hash_dir(&lay.src(), bundle::XLATE_INPUTS);
    }
    if state.keys_inputs.is_empty() && lay.keys_json().is_file() {
        state.keys_inputs = bundle::inputs_hash_dir(&lay.src(), bundle::KEYS_INPUTS);
    }
    if state.steps.is_empty() && (lay.root.join("Jouer.cmd").is_file() || lay.root.join("launcher-data").is_dir()) {
        state.adopted_from_kit = true;
    }
}

/// Memory of one emit worker of the generator on Splatoon 3 11.2.0, with a margin: 16 workers peaked at 8.0 GiB for all
/// the Python processes together (the parent included), 0.96 GiB for the largest. The generator's own default assumes
/// the upstream target's 1.5 GiB and so ran 7 workers on the 32 GB reference PC: 202 s in a first install, 133 s with
/// 16 (128 s alone).
pub const LIFT_WORKER_MEM: u64 = 768 << 20;

/// Emit workers of the lift: every logical CPU, within 80 % of the memory available now (the generated C does not depend
/// on their number).
pub fn lift_jobs(cpus: usize, available: Option<u64>) -> u32 {
    let by_mem = available.map_or(cpus as u64, |a| a * 4 / 5 / LIFT_WORKER_MEM);
    (cpus as u64).min(by_mem).clamp(1, 64) as u32
}

/// For every file of `fresh` whose content equals the same file in `old`, gives it the old file's modification time.
/// Returns how many files kept their time.
pub fn keep_unchanged_mtimes(old: &Path, fresh: &Path) -> usize {
    let mut n = 0;
    let mut stack = vec![fresh.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let Ok(rel) = p.strip_prefix(fresh) else { continue };
            let o = old.join(rel);
            let (Ok(mo), Ok(mn)) = (std::fs::metadata(&o), std::fs::metadata(&p)) else { continue };
            if mo.len() != mn.len() {
                continue;
            }
            let (Ok(a), Ok(b)) = (std::fs::read(&o), std::fs::read(&p)) else { continue };
            if a == b
                && let Ok(t) = mo.modified()
                && std::fs::File::options().write(true).open(&p).and_then(|f| f.set_modified(t)).is_ok()
            {
                n += 1;
            }
        }
    }
    n
}
/// How many generated files the last lift rewrote, of how many (`stats.chunks_written` of each module's manifest).
fn lift_rewritten(lift: &Path) -> (u64, u64) {
    let mut out = (0, 0);
    let Ok(rd) = std::fs::read_dir(lift) else { return out };
    for e in rd.flatten() {
        let Some(m) = std::fs::read_to_string(e.path().join("manifest.json")).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else { continue };
        out.0 += m.pointer("/stats/chunks_written").and_then(|v| v.as_u64()).unwrap_or(0);
        out.1 += m.get("chunks").and_then(|v| v.as_array()).map_or(0, |a| a.len() as u64 + 1);
    }
    out
}

pub const PREVIOUS_DIR: &str = "build-previous";
/// The second name the executable in place gets while a compile runs ([`Run::prepare_previous`]).
pub const PREVIOUS_NEW: &str = "build-previous.new";
const PREVIOUS_MARK: &str = "KALAMAR-PREVIOUS.json";

fn previous_dir(lay: &Layout) -> PathBuf {
    lay.game().join(PREVIOUS_DIR)
}

fn previous_new(lay: &Layout) -> PathBuf {
    lay.game().join(PREVIOUS_NEW)
}

/// Size and modification time of a file, read from the file itself: two names of one file (a hard link) give the same,
/// a relinked executable gives another.
fn identity(p: &Path) -> Option<(u64, u128)> {
    let m = std::fs::metadata(p).ok()?;
    Some((m.len(), m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos()))
}

fn read_mark(dir: &Path) -> serde_json::Value {
    std::fs::read_to_string(dir.join(PREVIOUS_MARK)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Runtime source commit and translator digest recorded with the executable kept in `build-previous`.
fn previous_mark(lay: &Layout) -> (String, String) {
    let m = read_mark(&previous_dir(lay));
    let get = |k: &str| m.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let commit = get("commit");
    (if commit.is_empty() { "kit".into() } else { commit }, get("xlate_inputs"))
}

/// After a compile, or at the next start when one was interrupted: settles the second name [`Run::prepare_previous`]
/// gave the executable. When the link replaced the executable, that name holds the previous game and becomes
/// `build-previous` (the one-step rollback); when nothing was relinked both names are the same file, and it is dropped
/// (`build-previous` stays the version before). `Some(true)` / `Some(false)` for those, `None` when there was nothing.
pub fn settle_previous(lay: &Layout) -> Option<bool> {
    let new = previous_new(lay);
    let kept = new.join(exe_name(GAME_STEM));
    if !kept.is_file() {
        let _ = sys::remove_all(&new);
        return None;
    }
    let mark = read_mark(&new);
    let recorded = mark.get("size").and_then(|v| v.as_u64()).zip(mark.get("mtime_ns").and_then(|v| v.as_str()).and_then(|s| s.parse::<u128>().ok()));
    let kept_id = identity(&kept);
    let replaced = kept_id.is_some() && kept_id == recorded && identity(&lay.game_exe()) != kept_id;
    if replaced && lay.game_exe().is_file() {
        let _ = sys::remove_all(&previous_dir(lay));
        if std::fs::rename(&new, previous_dir(lay)).is_ok() {
            return Some(true);
        }
    }
    // Not relinked (the same file), or the kept name no longer holds what it was given: nothing to keep.
    let _ = sys::remove_all(&new);
    Some(false)
}

fn dlls(dir: &Path) -> Vec<PathBuf> {
    // What the game loads beside its executable: the DLLs on Windows, the shared objects on Linux (`libSDL3.so.0`).
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    let name = p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                    name.ends_with(".dll") || (name.starts_with("lib") && name.contains(".so"))
                })
                .collect()
        })
        .unwrap_or_default()
}

impl Run {
    /// Before a compile: gives the executable in place a second name in `build-previous.new\` (a hard link: nothing is
    /// copied or moved; a copy only on a drive without hard links), with copies of its DLLs and a note of which build it
    /// is. The link step writes the new executable under a temporary name and renames it over the old one, so the
    /// second name keeps the previous game; when Ninja relinks nothing, both names stay one file and nothing is kept
    /// ([`settle_previous`]). The game stays playable during the whole update.
    pub(super) fn prepare_previous(&mut self) -> Result<(), Fail> {
        settle_previous(&self.lay);
        let new = previous_new(&self.lay);
        sys::remove_all(&new).map_err(|e| Fail::io("build-previous", e))?;
        std::fs::create_dir_all(&new).map_err(|e| Fail::io("build-previous", e))?;
        let (exe, kept) = (self.lay.game_exe(), new.join(exe_name(GAME_STEM)));
        if std::fs::hard_link(&exe, &kept).is_err() {
            std::fs::copy(&exe, &kept).map_err(|e| Fail::io("build-previous", e))?;
        }
        for d in dlls(&self.lay.build()) {
            if let Some(n) = d.file_name() {
                let _ = std::fs::copy(&d, new.join(n));
            }
        }
        let old = self.state.steps.get(StepId::Compile.name()).filter(|r| r.status == "done").map(|r| r.commit.clone()).unwrap_or_else(|| "kit".into());
        let (size, mtime) = identity(&kept).unwrap_or_default();
        let mark = serde_json::json!({ "commit": old, "xlate_inputs": self.state.xlate_inputs, "at": crate::util::now_secs(), "size": size, "mtime_ns": mtime.to_string() });
        std::fs::write(new.join(PREVIOUS_MARK), mark.to_string()).map_err(|e| Fail::io("build-previous", e))?;
        Ok(())
    }

    pub(super) fn stash_previous_xlate(&mut self) -> Result<(), Fail> {
        let prev = self.lay.xlate().join("previous");
        sys::remove_all(&prev).map_err(|e| Fail::io("xlate previous", e))?;
        std::fs::create_dir_all(&prev).map_err(|e| Fail::io("xlate previous", e))?;
        for n in &[exe_name(XLATE_STEM), "totk_xlate.id".to_string()] {
            let p = self.lay.xlate().join(n);
            if p.is_file() {
                std::fs::rename(&p, prev.join(n)).map_err(|e| Fail::io("xlate previous", e))?;
            }
        }
        self.xlate_swapped = true;
        Ok(())
    }

    /// Puts back the previous executable (kept in `build-previous` once this run relinked) and what
    /// [`Run::stash_previous_xlate`] moved aside.
    pub(super) fn restore_previous(&mut self) {
        if self.swapped {
            let prev = previous_dir(&self.lay);
            if prev.join(exe_name(GAME_STEM)).is_file() {
                let _ = std::fs::remove_file(self.lay.game_exe());
                if std::fs::rename(prev.join(exe_name(GAME_STEM)), self.lay.game_exe()).is_ok() {
                    for d in dlls(&prev) {
                        if let Some(n) = d.file_name() {
                            let _ = std::fs::copy(&d, self.lay.build().join(n));
                        }
                    }
                    let _ = sys::remove_all(&prev);
                    self.log("    the update did not finish: the previous game is back in place");
                }
            }
            self.swapped = false;
        }
        if self.xlate_swapped {
            let prev = self.lay.xlate().join("previous");
            for n in &[exe_name(XLATE_STEM), "totk_xlate.id".to_string()] {
                if prev.join(n).is_file() {
                    let _ = std::fs::remove_file(self.lay.xlate().join(n));
                    let _ = std::fs::rename(prev.join(n), self.lay.xlate().join(n));
                }
            }
            let _ = sys::remove_all(&prev);
            self.xlate_swapped = false;
        }
    }
}

/// After an installation that stopped without finishing its step (launcher closed or killed, crash, reboot): removes
/// the merge tool's temporary copy (it holds a key file derived from prod.keys), settles the second name a compile gave
/// the executable, and puts back the previous game executable if an older launcher had moved it aside, so the game
/// launches while the install waits to be resumed. Returns what it did, for the log. Never runs while an installation
/// is active.
pub fn recover_interrupted(root: &Path) -> Vec<String> {
    let lay = Layout::new(root);
    let mut done = Vec::new();
    for t in lay.merge_temps() {
        if sys::remove_all(&t).is_ok() {
            done.push(format!("removed {}", t.display()));
        }
    }
    if settle_previous(&lay) == Some(true) {
        done.push("previous game executable kept in build-previous".into());
    }
    let prev = previous_dir(&lay).join(exe_name(GAME_STEM));
    if prev.is_file() && !lay.game_exe().is_file() && std::fs::rename(&prev, lay.game_exe()).is_ok() {
        for d in dlls(&previous_dir(&lay)) {
            if let Some(n) = d.file_name() {
                let _ = std::fs::copy(&d, lay.build().join(n));
            }
        }
        let _ = sys::remove_all(&previous_dir(&lay));
        done.push("previous game executable restored".into());
    }
    done
}
#[derive(Debug, Clone, Serialize)]
pub struct RollbackReport {
    /// Runtime source of the game now in place (`kit` for a pre-release installer's build).
    pub now: String,
    pub previous: String,
}

fn swap_files(a: &Path, b: &Path) -> std::io::Result<()> {
    let tmp = a.with_extension("swap");
    std::fs::rename(a, &tmp)?;
    if let Err(e) = std::fs::rename(b, a) {
        let _ = std::fs::rename(&tmp, a);
        return Err(e);
    }
    std::fs::rename(&tmp, b)
}

/// "Revert to the previous version": swaps the game executable (and the translator, when the update replaced it)
/// with the one kept by the last update. Doing it again goes back to the newer one.
pub fn rollback(root: &Path) -> Result<RollbackReport, String> {
    let lay = Layout::new(root);
    let prev = previous_dir(&lay);
    if !prev.join(exe_name(GAME_STEM)).is_file() {
        return Err("no_previous".into());
    }
    let mut state = StateFile::load(&lay.state()).unwrap_or_default();
    let mark: serde_json::Value = std::fs::read_to_string(prev.join(PREVIOUS_MARK)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let prev_commit = mark.get("commit").and_then(|v| v.as_str()).unwrap_or("kit").to_string();
    let prev_xlate = mark.get("xlate_inputs").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if lay.game_exe().is_file() {
        swap_files(&lay.game_exe(), &prev.join(exe_name(GAME_STEM))).map_err(|e| e.to_string())?;
    } else {
        std::fs::rename(prev.join(exe_name(GAME_STEM)), lay.game_exe()).map_err(|e| e.to_string())?;
    }
    for d in dlls(&prev) {
        if let Some(n) = d.file_name() {
            let cur = lay.build().join(n);
            if cur.is_file() {
                let _ = swap_files(&cur, &d);
            }
        }
    }
    let xprev = lay.xlate().join("previous");
    let mut xlate_now = state.xlate_inputs.clone();
    if xprev.join(exe_name(XLATE_STEM)).is_file() {
        for n in &[exe_name(XLATE_STEM), "totk_xlate.id".to_string()] {
            let (cur, old) = (lay.xlate().join(n), xprev.join(n));
            if cur.is_file() && old.is_file() {
                let _ = swap_files(&cur, &old);
            }
        }
        xlate_now = prev_xlate.clone();
    }
    let cur_commit = state.steps.get(StepId::Compile.name()).map(|r| r.commit.clone()).unwrap_or_else(|| "kit".into());
    let mark = serde_json::json!({ "commit": cur_commit, "xlate_inputs": state.xlate_inputs, "at": crate::util::now_secs() });
    let _ = std::fs::write(prev.join(PREVIOUS_MARK), mark.to_string());
    if let Some(r) = state.steps.get_mut(StepId::Compile.name()) {
        r.commit = prev_commit.clone();
    }
    state.xlate_inputs = xlate_now;
    state.previous_commit = cur_commit.clone();
    // The executable in place is no longer what the objects were built from: a later update runs the compile again,
    // which relinks the newer objects (the generated C and the objects themselves stay valid).
    state.compile_inputs.clear();
    state.save(&lay.state()).map_err(|e| e.to_string())?;
    Ok(RollbackReport { now: prev_commit, previous: cur_commit })
}

/// A game install found on this PC.
#[derive(Debug, Clone, Serialize)]
pub struct Existing {
    pub dir: String,
    /// `wizard` (has the wizard's state file) or `kit` (made by the pre-release install script).
    pub kind: &'static str,
    pub game_root: String,
    pub exe: bool,
    pub exefs: bool,
    pub romfs: bool,
    pub lift: bool,
    pub objects: bool,
    pub xlate: bool,
    pub venv: bool,
    /// Runtime source in place: a commit (wizard) or `kit <VERSION>`.
    pub source: Option<String>,
    /// The old launcher's data folder (`launcher-data`, set by the pre-release installer's start script).
    pub launcher_data: Option<String>,
    pub needs_files: bool,
    /// A previous executable is kept (`build-previous`).
    pub previous: bool,
    pub saves: bool,
}

/// Looks at the given folders (install folders, or game roots `<install>\work\splatoon3-11.2.0`) for installs.
pub fn find_existing(cands: &[PathBuf]) -> Vec<Existing> {
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut out = Vec::new();
    for c in cands {
        let root = if c.ends_with(Path::new("work").join(GAME_DIR)) { c.parent().and_then(Path::parent).map(Path::to_path_buf) } else { Some(c.clone()) };
        let Some(root) = root else { continue };
        let key = PathBuf::from(root.display().to_string().to_ascii_lowercase());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        let lay = Layout::new(&root);
        let (kind, _) = folder_kind(&root);
        let kind = match kind {
            "wizard" => "wizard",
            "kit" => "kit",
            _ => continue,
        };
        if !lay.game().is_dir() {
            continue;
        }
        let source = bundle::load_marker(&lay.src()).or_else(|| {
            std::fs::read_to_string(lay.src().join("VERSION")).ok().map(|v| format!("kit {}", v.trim()))
        });
        out.push(Existing {
            dir: root.display().to_string(),
            kind,
            game_root: lay.game().display().to_string(),
            exe: lay.game_exe().is_file(),
            exefs: lay.exefs().join("main").is_file(),
            romfs: lay.romfs_done(),
            lift: lay.lift_outputs(),
            objects: lay.build().join("obj").is_dir(),
            xlate: lay.xlate_exe().is_file(),
            venv: lay.venv_py().is_file(),
            source,
            launcher_data: Some(root.join("launcher-data")).filter(|p| p.is_dir()).map(|p| p.display().to_string()),
            needs_files: needs_player_files(&lay),
            previous: previous_dir(&lay).join(exe_name(GAME_STEM)).is_file(),
            saves: crate::install::save_dir(&lay.game()).is_dir(),
        });
    }
    out
}

/// `dir` lies inside the user's profile (resolved: a junction or link inside the profile that points elsewhere is
/// elsewhere). Unknown (no profile, a folder that cannot be resolved): inside, nothing to say.
pub fn inside_profile(dir: &Path, profile: Option<&Path>) -> bool {
    let Some(home) = profile else { return true };
    match (std::fs::canonicalize(dir), std::fs::canonicalize(home)) {
        (Ok(d), Ok(h)) => crate::config::under(&d, &h),
        _ => true,
    }
}

/// Where installs are looked for: the pre-release installer's default folder, the configured game root, and the folders the
/// launcher was started from (up to 6 levels above them, inside the user's profile only: `config::ancestors_in_profile`).
pub fn default_candidates(config_root: Option<&Path>) -> Vec<PathBuf> {
    let mut c = Vec::new();
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        c.push(PathBuf::from(l).join("Splat3Recomp").join("Jeu"));
    }
    if let Some(r) = config_root {
        c.push(r.to_path_buf());
    }
    if let Some(v) = crate::config::game_root_env() {
        c.push(v);
    }
    let profile = crate::config::user_profile();
    for start in [std::env::current_exe().ok(), std::env::current_dir().ok()].into_iter().flatten() {
        for dir in crate::config::ancestors_in_profile(&start, profile.as_deref()).into_iter().take(6) {
            if dir.join("work").join(GAME_DIR).is_dir() {
                c.push(dir);
            }
        }
    }
    c
}
// ---- the "what will be installed and downloaded" screen -------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct Download {
    /// `python`, `pypi`, `llvm`, `ninja`, `sdl3`, `msys2` (portable tools); `shipped` (in the launcher, nothing
    /// downloaded).
    pub id: &'static str,
    pub source: String,
    /// Files fetched from it.
    pub items: Vec<String>,
    /// Whether this run needs it (otherwise it is already there).
    pub needed: bool,
    /// Bytes downloaded.
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanStep {
    pub id: StepId,
    pub done: bool,
    pub reference: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub install_dir: String,
    pub game_root: String,
    /// System tools the install uses (empty with the portable tools: nothing is installed on the PC).
    pub tools: Vec<prereq::Tool>,
    pub missing_packages: Vec<String>,
    pub winget: Option<String>,
    /// Some tool must be installed system-wide: Windows will ask for an administrator's approval (never now).
    pub needs_admin: bool,
    /// Something must be installed on the system first (never now: the agreement box of that case is gone).
    pub needs_prereqs: bool,
    /// The tools go into `<I>\tools` (`toolchain`: `portable`), or the install keeps MSYS2 (`msys2`).
    pub toolchain: Toolchain,
    pub tools_dir: String,
    pub downloads: Vec<Download>,
    /// Bytes this run downloads.
    pub download_bytes: u64,
    pub steps: Vec<PlanStep>,
    pub disk_free: Option<u64>,
    /// Free space needed from the start (the most the install needs at once, from what it will do).
    pub disk_needed: u64,
    /// The step at which the disk is fullest.
    pub disk_peak_step: Option<StepId>,
    /// What the install folder holds at the end.
    pub footprint: u64,
    pub ram: Option<u64>,
    pub cpus: usize,
    pub jobs: u32,
    pub source_commit: Option<String>,
    pub eta: f64,
    /// The estimate when a new generated C changes every chunk (equal to `eta` unless the code generator changed).
    pub eta_max: f64,
    /// For an existing install: what the update redoes to the generated C and the objects, said before it starts.
    pub update: Option<incremental::UpdatePreview>,
    /// The commit of the runtime source installed (`kit <version>` for a pre-release install).
    pub installed_source: Option<String>,
    /// An earlier install of the wizard is in this folder.
    pub existing: bool,
    pub existing_finished: bool,
    /// The player's dumps and keys are needed (the extracted data is not in the folder yet).
    pub needs_files: bool,
    /// Generated C and objects are kept: the compile recompiles only what changed.
    pub incremental: bool,
    /// What deleting the generated C and the objects after the build would save.
    pub build_files: u64,
    pub adopted_from_kit: bool,
    /// The folder holds other things than an install (the wizard then refuses it).
    pub foreign: bool,
}

/// Whether a folder can receive an install: missing, empty, a wizard install, or a pre-release install (adopted).
pub fn folder_kind(root: &Path) -> (&'static str, bool) {
    if !root.exists() {
        return ("new", false);
    }
    if root.join(STATE_FILE).is_file() {
        return ("wizard", false);
    }
    // The pre-release installer's layout: its own copy of the runtime source next to the game folder (never a developer's workspace,
    // which has no `src\recomp` there).
    if root.join("src").join("recomp").join("gen.py").is_file() && (root.join("work").join(GAME_DIR).is_dir() || root.join("Jouer.cmd").is_file()) {
        return ("kit", false);
    }
    let empty = std::fs::read_dir(root).map(|mut r| r.next().is_none()).unwrap_or(false);
    if empty { ("new", false) } else { ("other", true) }
}

/// The steps a run would do (with their reference durations) and its estimate, likely and at most, from a [`Run`] whose
/// preview is computed.
fn planned_steps(run: &Run, commit: &str) -> (Vec<PlanStep>, f64, f64) {
    let steps: Vec<PlanStep> = StepId::ALL
        .iter()
        .map(|&id| PlanStep { id, done: !id.always_runs() && run.is_done(id, commit), reference: run.reference(id, commit) })
        .collect();
    // The steps that run beside the others count once (the merge during the lift, the download from the start).
    let mut fg = Vec::new();
    let mut order = Vec::new();
    let mut beside = Vec::new();
    for p in steps.iter().filter(|p| !p.done) {
        match run.bg_before(p.id) {
            Some(before) => beside.push((p.reference, before)),
            None => {
                fg.push(p.reference);
                order.push(p.id);
            }
        }
    }
    let lanes: Vec<(f64, usize)> = beside.into_iter().map(|(r, b)| (r, order.iter().position(|k| *k == b).unwrap_or(order.len()))).collect();
    let eta = progress::lanes_eta(&fg, &lanes);
    let extra = run.preview.as_ref().filter(|p| p.compile).map_or(0.0, |p| p.compile_secs_max - p.compile_secs);
    (steps, eta, eta + extra)
}

/// What a first install needs on this PC, for the welcome screen: free space from the start, space at the end, the
/// estimated duration with `jobs` compile jobs, and the bytes downloaded.
pub fn fresh_estimate(jobs: u32) -> (u64, u64, f64, u64) {
    let uses: Vec<(StepId, space::Use)> = StepId::ALL.iter().filter(|id| !id.always_runs()).map(|&id| (id, space::step_use(id, true, true, 0))).collect();
    let need = space::needs(&uses).first().map_or(space::MARGIN, |n| n.1);
    let order: Vec<StepId> = StepId::ALL.iter().copied().filter(|id| !matches!(id, StepId::Prereqs | StepId::Romfs)).collect();
    let fg: Vec<f64> = order.iter().map(|&id| progress::reference_secs_portable(id, jobs)).collect();
    let at = |s: StepId| order.iter().position(|k| *k == s).unwrap_or(order.len());
    let lanes = [(progress::reference_secs_portable(StepId::Prereqs, jobs), at(StepId::Compile)), (progress::reference_secs_portable(StepId::Romfs, jobs), at(StepId::Compile))];
    (need, space::added(&uses), progress::lanes_eta(&fg, &lanes), portable::bytes(&portable::all_pins()))
}

/// Free space a folder needs for the install still to do (from what is already there), for the folder check of the
/// files screen: the portable tools, an in-place merge.
pub fn first_need(lay: &Layout) -> u64 {
    let done = |id: StepId| match id {
        StepId::Prereqs => portable::toolchain_ready(&lay.root),
        StepId::Python => portable::python_ready(&lay.root) || lay.venv_py().is_file(),
        StepId::Exefs => lay.exefs().join("main").is_file(),
        StepId::Romfs => lay.romfs_done(),
        StepId::Lift => lay.lift_outputs(),
        StepId::Compile => lay.game_exe().is_file(),
        StepId::Xlate => lay.xlate_exe().is_file(),
        _ => false,
    };
    let uses: Vec<(StepId, space::Use)> = StepId::ALL.iter().filter(|id| !id.always_runs() && !done(**id)).map(|&id| (id, space::step_use(id, true, true, 0))).collect();
    space::needs(&uses).first().map_or(space::MARGIN, |n| n.1)
}

/// A wizard run of `req` that only looks (nothing is started), with what the update would change.
fn dry_run(req: &InstallRequest, bundle_dir: Option<&Path>) -> Run {
    let mut run = Run::new(Installer::default(), req.clone(), bundle_dir.map(Path::to_path_buf), Arc::new(|_| {}), Arc::new(|_| {}));
    run.jobs = if req.jobs == 0 { default_jobs() } else { req.jobs };
    adopt_records(&mut run.state, &run.lay);
    run.preview = run.assess();
    run
}

/// The game installed in `install_dir` against the source this launcher carries, for the Play page.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateCheck {
    pub install_dir: String,
    /// The installed source is not the bundled one, or a step that depends on it would run.
    pub available: bool,
    pub installed_source: Option<String>,
    pub bundled_commit: String,
    /// Steps that would do work (the quick checks that always run left out).
    pub redo: Vec<StepId>,
    pub update: Option<incremental::UpdatePreview>,
    pub eta: f64,
    pub eta_max: f64,
    /// The player chose to run it when the launcher closes (set by the launcher, not by the check).
    pub at_close: bool,
}

/// Whether the game installed in `install_dir` (a finished install of the wizard or of the pre-release installer, with its executable)
/// can be updated to the bundled source, and what that would redo. `None` for anything else.
pub fn update_check(install_dir: &Path, bundle_dir: &Path) -> Option<UpdateCheck> {
    let lay = Layout::new(install_dir);
    let (kind, _) = folder_kind(install_dir);
    if !matches!(kind, "wizard" | "kit") || !lay.game_exe().is_file() || needs_player_files(&lay) {
        return None;
    }
    let state = StateFile::load(&lay.state());
    if state.as_ref().is_some_and(|s| !s.finished) {
        return None; // an unfinished install: the wizard offers to resume it
    }
    let manifest = bundle::load_manifest(bundle_dir)?;
    let req = InstallRequest { install_dir: install_dir.display().to_string(), ..Default::default() };
    let run = dry_run(&req, Some(bundle_dir));
    let (steps, eta, eta_max) = planned_steps(&run, &manifest.commit);
    let redo: Vec<StepId> = steps.iter().filter(|p| !p.done && !p.id.always_runs()).map(|p| p.id).collect();
    let installed_source = installed_source(&lay);
    Some(UpdateCheck {
        install_dir: install_dir.display().to_string(),
        available: !redo.is_empty(),
        installed_source,
        bundled_commit: manifest.commit,
        redo,
        update: run.preview,
        eta,
        eta_max,
        at_close: false,
    })
}

/// The runtime source installed: the commit the wizard extracted, or `kit <version>`.
fn installed_source(lay: &Layout) -> Option<String> {
    bundle::load_marker(&lay.src()).or_else(|| std::fs::read_to_string(lay.src().join("VERSION")).ok().map(|v| format!("kit {}", v.trim())))
}

pub fn plan(req: &InstallRequest, bundle_dir: Option<&Path>) -> Plan {
    let lay = Layout::new(&req.install_dir);
    let manifest = bundle_dir.and_then(bundle::load_manifest);
    let commit = manifest.as_ref().map(|m| m.commit.clone()).unwrap_or_default();
    let state = StateFile::load(&lay.state()).unwrap_or_default();
    let run = dry_run(req, bundle_dir);
    let jobs = run.jobs;
    let (steps, eta, eta_max) = planned_steps(&run, &commit);
    let done = |id: StepId| steps.iter().any(|p| p.id == id && p.done);
    // What is downloaded, from where: the portable tools (official sources, pinned SHA-256) on Windows; the system's
    // package manager on Linux, where the launcher installs nothing by itself. hactool and the shader translator come
    // with the launcher when it carries them.
    let python_needed = !done(StepId::Python);
    #[cfg(windows)]
    let names = |pins: &[portable::Pin]| pins.iter().map(|p| p.name.to_string()).collect::<Vec<_>>();
    #[cfg(windows)]
    let tools_needed = run.portable() && !done(StepId::Prereqs);
    let shipped = run.shipped.as_ref();
    #[cfg(windows)]
    let mut downloads: Vec<Download> = portable::download_groups()
        .into_iter()
        .map(|(id, source, bytes)| {
            let (items, needed) = match id {
                "python" => (vec![portable::PYTHON_EMBED.name.to_string()], python_needed),
                "pypi" => (names(&portable::WHEELS), python_needed),
                "llvm" => (vec![portable::LLVM_MINGW.name.to_string()], tools_needed),
                "ninja" => (vec![portable::NINJA.name.to_string()], tools_needed),
                "sdl3" => (names(&[portable::SDL3_DLL, portable::SDL3_DEVEL]), tools_needed),
                _ => (names(&[portable::OPUS, portable::LIBVPX, portable::VULKAN_HEADERS, portable::VULKAN_LOADER, portable::VMA, portable::ZSTD]), tools_needed),
            };
            Download { id, source, items, needed, bytes }
        })
        .collect();
    #[cfg(not(windows))]
    let mut downloads: Vec<Download> = vec![
        Download {
            id: "pypi",
            source: "https://pypi.org (PyPI, the versions of tools/requirements-installer.txt)".into(),
            items: vec!["numpy".into(), "lz4".into(), "cryptography".into(), "cffi".into(), "pycparser".into(), "fonttools".into()],
            needed: python_needed,
            bytes: 0,
        },
        Download {
            id: "system",
            source: "the system's package manager (nothing is installed by the launcher)".into(),
            items: prereq::system_missing().iter().map(|id| id.to_string()).collect(),
            needed: !prereq::system_ready(),
            bytes: 0,
        },
    ];
    let _ = python_needed;
    downloads.push(Download {
        id: "shipped",
        source: shipped.map(|t| format!("Kalamar (engine {})", &t.manifest.engine_commit[..t.manifest.engine_commit.len().min(7)])).unwrap_or_default(),
        items: vec![exe_name(HACTOOL_STEM), exe_name(XLATE_STEM)],
        needed: shipped.is_some() && (!done(StepId::Hactool) || !done(StepId::Xlate)),
        bytes: 0,
    });
    let download_bytes = downloads.iter().filter(|d| d.needed).map(|d| d.bytes).sum();
    let (kind, foreign) = folder_kind(&lay.root);
    let existing = kind == "wizard" || kind == "kit";
    let uses = run.space_steps(StepId::Prereqs);
    let disk_needed = space::needs(&uses).first().map_or(space::MARGIN, |n| n.1);
    let disk_peak_step = space::peak_step(&uses);
    #[cfg(windows)]
    let tools = if run.portable() { Vec::new() } else { vec![prereq::Tool { id: "msys2", winget: prereq::WINGET_MSYS2, path: Some(prereq::msys_bash().display().to_string()) }] };
    #[cfg(not(windows))]
    let tools = prereq::detect().tools;
    Plan {
        install_dir: lay.root.display().to_string(),
        game_root: lay.game().display().to_string(),
        needs_admin: false,
        needs_prereqs: false,
        missing_packages: Vec::new(),
        winget: None,
        tools,
        toolchain: run.toolchain,
        tools_dir: lay.tools().display().to_string(),
        downloads,
        download_bytes,
        disk_free: sys::free_bytes(&lay.root),
        disk_needed,
        disk_peak_step,
        footprint: sys::dir_size(&lay.root) + space::added(&uses),
        ram: sys::total_ram(),
        cpus: sys::cpus(),
        jobs,
        source_commit: manifest.as_ref().map(|m| m.commit.clone()),
        eta,
        eta_max,
        existing,
        existing_finished: state.finished,
        needs_files: needs_player_files(&lay),
        incremental: run.preview.as_ref().is_some_and(|p| p.kind != "full"),
        build_files: BUILD_FILES_ESTIMATE,
        update: run.preview.clone().filter(|_| existing && lay.game_exe().is_file()),
        installed_source: installed_source(&lay),
        adopted_from_kit: run.state.adopted_from_kit,
        foreign,
        steps,
    }
}

// ---- uninstall ----------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct UninstallReport {
    pub removed: Vec<String>,
    pub kept: Vec<String>,
    pub freed: u64,
}

/// Removes what the wizard created in `root`, and nothing else: never a system tool, never the player's files (they
/// were never copied), and the game's user data (saves, settings file) unless `remove_saves`.
pub fn uninstall(root: &Path, remove_saves: bool) -> Result<UninstallReport, String> {
    let lay = Layout::new(root);
    if !lay.state().is_file() {
        return Err("not_wizard_install".into());
    }
    let g = lay.game();
    let mut targets = vec![
        lay.src(),
        lay.root.join("src.new"),
        lay.tools(),
        lay.venv(),
        lay.nuget(),
        lay.xlate(),
        lay.exefs(),
        lay.romfs(),
        lay.lift(),
        previous_dir(&lay),
        previous_new(&lay),
    ];
    targets.extend(lay.merge_temps());
    targets.push(g.join("extraction-manifest.json"));
    targets.push(g.join("merge_romfs.log"));
    let mut kept = Vec::new();
    // build\: everything but the runtime's settings file (kept with the saves).
    if let Ok(rd) = std::fs::read_dir(lay.build()) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_ascii_lowercase();
            if !remove_saves && (n == "kalamar.ini" || n == "totk.ini") {
                kept.push(e.path().display().to_string());
            } else {
                targets.push(e.path());
            }
        }
    }
    if remove_saves {
        targets.push(lay.userdata());
    } else if lay.userdata().exists() {
        kept.push(lay.userdata().display().to_string());
    }
    targets.push(lay.logs());
    targets.push(lay.state());
    let mut removed = Vec::new();
    let mut freed = 0;
    for t in targets {
        if !t.exists() {
            continue;
        }
        let size = sys::dir_size(&t);
        sys::remove_all(&t).map_err(|e| format!("{}: {e}", t.display()))?;
        freed += size;
        removed.push(t.display().to_string());
    }
    // Folders left empty.
    for d in [lay.build(), g.clone(), lay.work(), lay.root.clone()] {
        if std::fs::read_dir(&d).map(|mut r| r.next().is_none()).unwrap_or(false) {
            let _ = std::fs::remove_dir(&d);
        }
    }
    Ok(UninstallReport { removed, kept, freed })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The runtime library a game build copies beside its executable.
    fn test_lib() -> &'static str {
        if cfg!(windows) { "SDL3.dll" } else { "libSDL3.so.0" }
    }

    /// An install folder outside the profile is said (its permissions come from its parent).
    #[test]
    fn an_install_folder_outside_the_profile_is_flagged() {
        let base = std::env::temp_dir().join(format!("s3l-prof-{}", crate::util::random_hex(4)));
        let (home, inside, outside) = (base.join("home"), base.join("home").join("Kalamar"), base.join("D").join("Kalamar"));
        for d in [&inside, &outside] {
            std::fs::create_dir_all(d).unwrap();
        }
        assert!(inside_profile(&inside, Some(&home)));
        assert!(!inside_profile(&outside, Some(&home)));
        assert!(inside_profile(&outside, None), "no profile known: nothing to say");
        std::fs::remove_dir_all(&base).unwrap();
    }

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    #[test]
    fn uninstall_removes_only_what_the_wizard_made() {
        let root = std::env::temp_dir().join(format!("s3l-uninst-{}", crate::util::random_hex(4)));
        let lay = Layout::new(&root);
        assert_eq!(uninstall(&root, false).unwrap_err(), "not_wizard_install");
        touch(&lay.state());
        touch(&lay.game_exe());
        touch(&lay.build().join("kalamar.ini"));
        touch(&lay.romfs().join("romfs.bin"));
        touch(&lay.venv_py());
        touch(&lay.xlate_exe());
        touch(&lay.src().join("recomp").join("gen.py"));
        touch(&lay.userdata().join("profile").join("save").join("u").join("save.dat"));
        touch(&lay.logs().join("lift.log"));
        touch(&lay.game().join(PREVIOUS_DIR).join(exe_name(GAME_STEM)));
        touch(&lay.lift().join("main").join("main_0000.c"));
        // something the wizard did not make, in the same folders
        touch(&root.join("Jouer.cmd"));
        touch(&lay.work().join("my-notes.txt"));
        let r = uninstall(&root, false).unwrap();
        assert!(!lay.game_exe().exists() && !lay.romfs().exists() && !lay.venv().exists() && !lay.src().exists() && !lay.state().exists());
        assert!(!lay.game().join(PREVIOUS_DIR).exists() && !lay.lift().exists(), "the kept previous game and the generated C go too");
        assert!(lay.build().join("kalamar.ini").is_file(), "settings kept");
        assert!(lay.userdata().join("profile").join("save").join("u").join("save.dat").is_file(), "saves kept");
        assert!(root.join("Jouer.cmd").is_file() && lay.work().join("my-notes.txt").is_file(), "foreign files kept");
        assert_eq!(r.kept.len(), 2);
        // second time, with the saves: refused (no state file any more) - the box is only offered on a wizard install
        assert!(uninstall(&root, true).is_err());
        touch(&lay.state());
        uninstall(&root, true).unwrap();
        assert!(!lay.userdata().exists() && !lay.build().exists());
        assert!(root.join("Jouer.cmd").is_file());
        sys::remove_all(&root).unwrap();
    }

    #[test]
    fn folder_kinds() {
        let root = std::env::temp_dir().join(format!("s3l-kind-{}", crate::util::random_hex(4)));
        assert_eq!(folder_kind(&root), ("new", false));
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(folder_kind(&root), ("new", false));
        touch(&root.join("photo.jpg"));
        assert_eq!(folder_kind(&root), ("other", true));
        std::fs::create_dir_all(root.join("work").join(GAME_DIR)).unwrap();
        // a developer's workspace (work\\splatoon3-11.2.0 without an install's src\\recomp) is not an install
        assert_eq!(folder_kind(&root).0, "other");
        touch(&root.join("src").join("recomp").join("gen.py"));
        assert_eq!(folder_kind(&root).0, "kit");
        touch(&root.join(STATE_FILE));
        assert_eq!(folder_kind(&root).0, "wizard");
        sys::remove_all(&root).unwrap();
    }

    #[test]
    fn an_old_kit_install_is_found_and_updated_without_the_players_files() {
        let root = std::env::temp_dir().join(format!("s3l-adopt-{}", crate::util::random_hex(4)));
        let lay = Layout::new(&root);
        touch(&lay.src().join("recomp").join("gen.py"));
        touch(&lay.src().join("tools").join("build_xlate.py"));
        std::fs::write(lay.src().join("VERSION"), "0.3.13\n").unwrap();
        touch(&lay.game_exe());
        touch(&lay.xlate_exe());
        touch(&root.join("Jouer.cmd"));
        std::fs::create_dir_all(root.join("launcher-data")).unwrap();
        // without the extracted data the files are needed
        assert!(needs_player_files(&lay));
        touch(&lay.exefs().join("main"));
        touch(&lay.romfs().join("romfs.bin"));
        touch(&lay.romfs().join("index.json"));
        assert!(!needs_player_files(&lay));
        let found = find_existing(&[lay.game(), root.clone()]);
        assert_eq!(found.len(), 1, "the same install named twice is listed once");
        let e = &found[0];
        assert_eq!(e.kind, "kit");
        assert_eq!(e.source.as_deref(), Some("kit 0.3.13"));
        assert!(e.exe && e.exefs && e.romfs && e.xlate && !e.lift && !e.needs_files);
        assert!(e.launcher_data.is_some());
        // a pre-release install's own source tells what its translator was built from
        let mut st = StateFile::default();
        adopt_records(&mut st, &lay);
        assert!(st.adopted_from_kit);
        assert_eq!(st.xlate_inputs, bundle::inputs_hash_dir(&lay.src(), bundle::XLATE_INPUTS));
        assert!(st.lift_inputs.is_empty(), "no generated C, nothing to reuse");
        // a source the wizard extracted is never taken for a pre-release install's
        bundle::write_marker(&lay.src(), &bundle::Manifest { commit: "abc".into(), ..Default::default() }).unwrap();
        let mut st2 = StateFile::default();
        adopt_records(&mut st2, &lay);
        assert!(st2.xlate_inputs.is_empty());
        sys::remove_all(&root).unwrap();
    }

    #[test]
    fn an_interrupted_install_is_put_right_at_the_next_start() {
        let root = std::env::temp_dir().join(format!("s3l-recover-{}", crate::util::random_hex(4)));
        let lay = Layout::new(&root);
        // killed during the merge: its temporary copy (with a derived key file) is left in the game folder
        touch(&lay.game().join("update-merge-abc").join("hactool.keys"));
        // killed during an update's compile: the previous executable is in build-previous, none in build
        touch(&lay.game().join(PREVIOUS_DIR).join(exe_name(GAME_STEM)));
        touch(&lay.game().join(PREVIOUS_DIR).join(test_lib()));
        std::fs::create_dir_all(lay.build()).unwrap();
        let done = recover_interrupted(&root);
        assert_eq!(done.len(), 2, "{done:?}");
        assert!(lay.merge_temps().is_empty());
        assert!(lay.game_exe().is_file() && lay.build().join(test_lib()).is_file());
        assert!(!lay.game().join(PREVIOUS_DIR).exists());
        // nothing to do the second time
        assert!(recover_interrupted(&root).is_empty());
        sys::remove_all(&root).unwrap();
    }
    #[test]
    fn unchanged_source_files_keep_their_timestamps() {
        let root = std::env::temp_dir().join(format!("s3l-mtime-{}", crate::util::random_hex(4)));
        let (old, fresh) = (root.join("old"), root.join("new"));
        for (d, same, changed) in [(&old, "x", "v1"), (&fresh, "x", "v2")] {
            std::fs::create_dir_all(d.join("runtime")).unwrap();
            std::fs::write(d.join("runtime").join("cpu.h"), same).unwrap();
            std::fs::write(d.join("runtime").join("vk.c"), changed).unwrap();
        }
        std::fs::write(fresh.join("runtime").join("new.c"), "n").unwrap();
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(86_400);
        std::fs::File::options().write(true).open(old.join("runtime").join("cpu.h")).unwrap().set_modified(past).unwrap();
        assert_eq!(keep_unchanged_mtimes(&old, &fresh), 1);
        let t = |p: &Path| std::fs::metadata(p).unwrap().modified().unwrap();
        assert_eq!(t(&fresh.join("runtime").join("cpu.h")), past, "same content: old time kept");
        assert!(t(&fresh.join("runtime").join("vk.c")) > past, "changed content: new time");
        sys::remove_all(&root).unwrap();
    }
    #[test]
    fn rollback_swaps_the_executable_both_ways() {
        let root = std::env::temp_dir().join(format!("s3l-rollback-{}", crate::util::random_hex(4)));
        let lay = Layout::new(&root);
        assert_eq!(rollback(&root).unwrap_err(), "no_previous");
        std::fs::create_dir_all(lay.build()).unwrap();
        std::fs::write(lay.game_exe(), b"new").unwrap();
        std::fs::write(lay.build().join(test_lib()), b"dll-new").unwrap();
        let prev = lay.game().join(PREVIOUS_DIR);
        std::fs::create_dir_all(&prev).unwrap();
        std::fs::write(prev.join(exe_name(GAME_STEM)), b"old").unwrap();
        std::fs::write(prev.join(test_lib()), b"dll-old").unwrap();
        std::fs::write(prev.join(PREVIOUS_MARK), r#"{"commit":"kit"}"#).unwrap();
        let mut st = StateFile::default();
        st.steps.insert("compile".into(), StepRecord { status: "done".into(), commit: "a105da5".into(), ..Default::default() });
        st.save(&lay.state()).unwrap();
        let r = rollback(&root).unwrap();
        assert_eq!((r.now.as_str(), r.previous.as_str()), ("kit", "a105da5"));
        assert_eq!(std::fs::read(lay.game_exe()).unwrap(), b"old");
        assert_eq!(std::fs::read(lay.build().join(test_lib())).unwrap(), b"dll-old");
        // the next update rebuilds: the compile record is the old one
        assert!(!StateFile::load(&lay.state()).unwrap().done_with(StepId::Compile, "a105da5"));
        let r2 = rollback(&root).unwrap();
        assert_eq!(r2.now, "a105da5");
        assert_eq!(std::fs::read(lay.game_exe()).unwrap(), b"new");
        sys::remove_all(&root).unwrap();
    }
    /// A bundle folder (archive + manifest) holding `files`, named `commit`.
    fn make_bundle(dir: &Path, files: &[(&str, &str)], commit: &str) -> PathBuf {
        let mut tar = tar::Builder::new(Vec::new());
        for (p, d) in files {
            let mut h = tar::Header::new_gnu();
            h.set_size(d.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            tar.append_data(&mut h, p, d.as_bytes()).unwrap();
        }
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut enc, &tar.into_inner().unwrap()).unwrap();
        let gz = enc.finish().unwrap();
        let man = bundle::Manifest { commit: commit.into(), files: files.len(), sha256: bundle::sha256_hex(&gz), ..Default::default() };
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(bundle::ARCHIVE_NAME), &gz).unwrap();
        std::fs::write(dir.join(bundle::MANIFEST_NAME), serde_json::to_vec(&man).unwrap()).unwrap();
        dir.to_path_buf()
    }

    const BASE: [(&str, &str); 8] = [
        ("recomp/gen.py", "gen 1"),
        ("tools/check_content.py", "marker"),
        ("tools/build_game.py", "x = 1\nGEN_FLAGS = ['-O1']\nRT_FLAGS = ['-O2']\n"),
        ("runtime/vk.c", "vk 1"),
        ("docs/building.md", "doc 1"),
        ("kit-tools/profile/s3save.py", "keys 1"),
        ("tools/build_xlate.py", "xlate 1"),
        ("VERSION", "1.0"),
    ];

    fn with(changes: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
        let mut v: Vec<(&str, &str)> = BASE.to_vec();
        for (p, d) in changes {
            match v.iter_mut().find(|(q, _)| q == p) {
                Some(e) => e.1 = d,
                None => v.push((p, d)),
            }
        }
        v
    }

    /// The portable toolchain unpacked (its marker and the files the readiness test looks at).
    fn portable_tools_ready(root: &Path) {
        for p in [portable::llvm_dir(root).join("bin").join("clang.exe"), portable::ninja_exe(root), portable::deps_dir(root).join("lib").join("libSDL3.dll.a")] {
            touch(&p);
        }
        std::fs::write(portable::tools(root).join("toolchain.json"), serde_json::json!({ "id": portable::toolchain_id() }).to_string()).unwrap();
        assert!(portable::toolchain_ready(root));
    }

    /// A finished install whose game, generated C, keys and translator were made from the `BASE` bundle.
    fn built_install(tag: &str) -> (PathBuf, Layout, InstallRequest) {
        let root = std::env::temp_dir().join(format!("s3l-upd-{tag}-{}", crate::util::random_hex(4)));
        let lay = Layout::new(&root);
        let b0 = make_bundle(&root.join("b0"), &BASE, "c0");
        bundle::extract(&std::fs::read(b0.join(bundle::ARCHIVE_NAME)).unwrap(), &lay.src()).unwrap();
        bundle::write_marker(&lay.src(), &bundle::load_manifest(&b0).unwrap()).unwrap();
        for p in [lay.game_exe(), lay.xlate_exe(), lay.keys_json(), lay.venv_py(), lay.exefs().join("main"), lay.romfs().join("romfs.bin"), lay.romfs().join("index.json")] {
            touch(&p);
        }
        portable_tools_ready(&root);
        touch(&lay.lift().join("layout.json"));
        std::fs::write(lay.lift().join("target.json"), r#"{"target":"splatoon3-v1120","modules":["main"]}"#).unwrap();
        touch(&lay.lift().join("main").join("manifest.json"));
        let req = InstallRequest { install_dir: root.display().to_string(), jobs: 4, ..Default::default() };
        let run = dry_run(&req, Some(&b0));
        let mut st = StateFile { finished: true, lift_inputs: run.lift_hash.clone(), compile_inputs: run.compile_hash(), keys_inputs: run.keys_hash.clone(), xlate_inputs: run.xlate_hash.clone(), ..Default::default() };
        st.steps.insert("compile".into(), StepRecord { status: "done".into(), commit: "c0".into(), ..Default::default() });
        st.save(&lay.state()).unwrap();
        (root, lay, req)
    }

    /// The steps that depend on the runtime source and would run with a bundle of `files`.
    fn redone(root: &Path, req: &InstallRequest, files: &[(&str, &str)], force: bool) -> (Vec<StepId>, Option<incremental::UpdatePreview>) {
        let b = make_bundle(&root.join(format!("b-{}", crate::util::random_hex(4))), files, &format!("c-{}", crate::util::random_hex(4)));
        let run = dry_run(&InstallRequest { force_rebuild: force, ..req.clone() }, Some(&b));
        let commit = run.commit();
        let steps = [StepId::Lift, StepId::Compile, StepId::Xlate, StepId::Keys].into_iter().filter(|&id| !run.is_done(id, &commit)).collect();
        (steps, run.preview)
    }

    #[test]
    fn an_update_redoes_only_what_its_inputs_changed() {
        let (root, lay, req) = built_install("decide");
        // A newer commit with the same files (or only documentation changed): nothing is rebuilt.
        let (steps, p) = redone(&root, &req, &BASE, false);
        assert!(steps.is_empty(), "{steps:?}");
        let p = p.unwrap();
        assert_eq!((p.kind, p.changed_files, p.compile, p.lift), ("none", 0, false, false));
        let (steps, p) = redone(&root, &req, &with(&[("docs/building.md", "doc 2"), ("docs/new.md", "n")]), false);
        assert!(steps.is_empty(), "{steps:?}");
        assert_eq!(p.unwrap().changed_files, 2);
        // The runtime: only the compile (the generated C is kept).
        let (steps, p) = redone(&root, &req, &with(&[("runtime/vk.c", "vk 2")]), false);
        assert_eq!(steps, [StepId::Compile]);
        let p = p.unwrap();
        assert_eq!((p.compile, p.lift, p.changed_runtime), (true, false, 1));
        // The build script or the version string: the compile runs (Ninja decides what to rebuild).
        assert_eq!(redone(&root, &req, &with(&[("VERSION", "1.1")]), false).0, [StepId::Compile]);
        // The code generator, or the marker it writes: the generated C is made again, then compiled.
        assert_eq!(redone(&root, &req, &with(&[("recomp/gen.py", "gen 2")]), false).0, [StepId::Lift, StepId::Compile]);
        assert_eq!(redone(&root, &req, &with(&[("tools/check_content.py", "marker 2")]), false).0, [StepId::Lift, StepId::Compile]);
        // The keys script, the translator: their own step only.
        assert_eq!(redone(&root, &req, &with(&[("kit-tools/profile/s3save.py", "keys 2")]), false).0, [StepId::Keys]);
        assert_eq!(redone(&root, &req, &with(&[("tools/build_xlate.py", "xlate 2")]), false).0, [StepId::Xlate]);
        // Rebuild from scratch on request.
        let (steps, p) = redone(&root, &req, &BASE, true);
        assert_eq!(steps, [StepId::Lift, StepId::Compile]);
        assert_eq!(p.unwrap().kind, "full");
        // A lift that stopped half-way (no manifest) is made again; a missing executable is relinked.
        std::fs::remove_file(lay.lift().join("main").join("manifest.json")).unwrap();
        std::fs::remove_file(lay.game_exe()).unwrap();
        assert_eq!(redone(&root, &req, &BASE, false).0, [StepId::Lift, StepId::Compile]);
        sys::remove_all(&root).unwrap();
    }

    #[test]
    fn an_install_from_before_the_digests_falls_back_on_the_commit() {
        let (root, lay, req) = built_install("legacy");
        let mut st = StateFile::load(&lay.state()).unwrap();
        st.compile_inputs.clear();
        st.keys_inputs.clear();
        st.steps.insert("keys".into(), StepRecord { status: "done".into(), commit: "c0".into(), ..Default::default() });
        st.save(&lay.state()).unwrap();
        // Another commit: compiled and keys read again once (their digests are then recorded).
        assert_eq!(redone(&root, &req, &BASE, false).0, [StepId::Compile, StepId::Keys]);
        sys::remove_all(&root).unwrap();
    }

    #[test]
    fn the_previous_game_is_kept_only_when_the_link_replaces_it() {
        let root = std::env::temp_dir().join(format!("s3l-prev-{}", crate::util::random_hex(4)));
        let lay = Layout::new(&root);
        std::fs::create_dir_all(lay.build()).unwrap();
        std::fs::write(lay.game_exe(), b"v1").unwrap();
        std::fs::write(lay.build().join(test_lib()), b"dll").unwrap();
        let req = InstallRequest { install_dir: root.display().to_string(), ..Default::default() };
        let mut run = dry_run(&req, None);
        // Nothing relinked: both names are one file, nothing is kept.
        run.prepare_previous().unwrap();
        assert!(previous_new(&lay).join(exe_name(GAME_STEM)).is_file() && previous_new(&lay).join(test_lib()).is_file());
        assert_eq!(settle_previous(&lay), Some(false));
        assert!(!previous_new(&lay).exists() && !previous_dir(&lay).exists());
        assert_eq!(std::fs::read(lay.game_exe()).unwrap(), b"v1");
        // Relinked the way the linker does it: a new file renamed over the old name.
        run.prepare_previous().unwrap();
        std::fs::write(lay.build().join(format!("{}.tmp", exe_name(GAME_STEM))), b"v2-new").unwrap();
        std::fs::rename(lay.build().join(format!("{}.tmp", exe_name(GAME_STEM))), lay.game_exe()).unwrap();
        assert_eq!(settle_previous(&lay), Some(true));
        assert_eq!(std::fs::read(previous_dir(&lay).join(exe_name(GAME_STEM))).unwrap(), b"v1");
        assert_eq!(std::fs::read(lay.game_exe()).unwrap(), b"v2-new");
        assert!(!previous_new(&lay).exists());
        // An update that found nothing to relink keeps the version before as the rollback.
        run.prepare_previous().unwrap();
        assert_eq!(settle_previous(&lay), Some(false));
        assert_eq!(std::fs::read(previous_dir(&lay).join(exe_name(GAME_STEM))).unwrap(), b"v1");
        // A compile interrupted after its link is settled at the next start.
        run.prepare_previous().unwrap();
        std::fs::write(lay.build().join(format!("{}.tmp", exe_name(GAME_STEM))), b"v3-newer").unwrap();
        std::fs::rename(lay.build().join(format!("{}.tmp", exe_name(GAME_STEM))), lay.game_exe()).unwrap();
        assert_eq!(recover_interrupted(&root), vec!["previous game executable kept in build-previous".to_string()]);
        assert_eq!(std::fs::read(previous_dir(&lay).join(exe_name(GAME_STEM))).unwrap(), b"v2-new");
        // The rollback swaps them.
        rollback(&root).unwrap();
        assert_eq!(std::fs::read(lay.game_exe()).unwrap(), b"v2-new");
        sys::remove_all(&root).unwrap();
    }

    #[test]
    fn the_update_check_of_the_play_page() {
        #[cfg(not(windows))]
        if !prereq::system_ready() {
            // This machine lacks the Linux build tools, so the wizard would also list Prereqs: not what this tests.
            eprintln!("skipping: the Linux build tools are not installed here");
            return;
        }
        let (root, lay, _) = built_install("check");
        let same = make_bundle(&root.join("same"), &BASE, "c0");
        let c = update_check(&root, &same).unwrap();
        assert!(!c.available && c.redo.is_empty(), "{c:?}");
        let newer = make_bundle(&root.join("newer"), &with(&[("runtime/vk.c", "vk 2")]), "c1");
        let c = update_check(&root, &newer).unwrap();
        assert!(c.available);
        assert_eq!(c.redo, [StepId::Sources, StepId::Compile]);
        assert_eq!(c.update.unwrap().changed_runtime, 1);
        // Only the commit changed: still offered (a short run that records the new source), nothing rebuilt.
        let renamed = make_bundle(&root.join("renamed"), &BASE, "c2");
        let c = update_check(&root, &renamed).unwrap();
        assert_eq!(c.redo, [StepId::Sources]);
        // An unfinished install is the wizard's to resume.
        let mut st = StateFile::load(&lay.state()).unwrap();
        st.finished = false;
        st.save(&lay.state()).unwrap();
        assert!(update_check(&root, &newer).is_none());
        sys::remove_all(&root).unwrap();
    }

    #[test]
    fn plan_lists_every_download_source() {
        let root = std::env::temp_dir().join(format!("s3l-plan-{}", crate::util::random_hex(4)));
        let p = plan(&InstallRequest { install_dir: root.display().to_string(), ..Default::default() }, None);
        let ids: Vec<&str> = p.downloads.iter().map(|d| d.id).collect();
        #[cfg(windows)]
        {
            assert_eq!(ids, ["python", "pypi", "llvm", "ninja", "sdl3", "msys2", "shipped"]);
            // nothing on the system: no administrator, no agreement for system tools, everything in the folder
            assert!(!p.needs_admin && !p.needs_prereqs && p.tools.is_empty() && p.toolchain == Toolchain::Portable);
            assert_eq!(p.download_bytes, portable::bytes(&portable::all_pins()));
        }
        #[cfg(not(windows))]
        {
            // Linux: the packages of the system's manager (listed, never installed by the launcher) and the Python
            // environment's wheels; nothing downloaded into the install folder.
            assert_eq!(ids, ["pypi", "system", "shipped"]);
            assert!(!p.needs_admin && !p.needs_prereqs && p.toolchain == Toolchain::System);
            assert!(!p.tools.is_empty());
            assert_eq!(p.download_bytes, 0);
        }
        assert!(p.downloads.iter().find(|d| d.id == "pypi").unwrap().needed);
        // 10 GB of RomFS and the rest, nowhere near the 65 GB asked before
        assert!((12 * space::GB..18 * space::GB).contains(&p.disk_needed), "{}", p.disk_needed);
        assert!(!p.existing && !p.foreign);
        assert_eq!(p.steps.len(), 14);
        // the merge and the download run beside the lift: the estimate is less than the plain sum
        let sum: f64 = p.steps.iter().filter(|s| !s.done).map(|s| s.reference).sum();
        assert!(p.eta < sum - 50.0, "{} vs {sum}", p.eta);
    }

    #[test]
    fn lift_workers_follow_threads_and_available_memory() {
        // 16 threads, 20 GB available: every thread (a worker needs about 0.75 GB)
        assert_eq!(lift_jobs(16, Some(20 << 30)), 16);
        // 6 GB available: what 80 % of it holds
        assert_eq!(lift_jobs(16, Some(6 << 30)), 6);
        assert_eq!(lift_jobs(4, None), 4);
        assert_eq!(lift_jobs(16, Some(100 << 20)), 1);
    }

    #[test]
    fn the_first_install_estimate_and_the_folder_check() {
        let (need, footprint, eta, bytes) = fresh_estimate(16);
        assert!(need > footprint && footprint > 12 * space::GB, "{need} {footprint}");
        assert!((600.0..3000.0).contains(&eta), "{eta}");
        assert_eq!(bytes, portable::bytes(&portable::all_pins()));
        let root = std::env::temp_dir().join(format!("s3l-need-{}", crate::util::random_hex(4)));
        let lay = Layout::new(&root);
        let all = first_need(&lay);
        assert_eq!(all, need);
        touch(&lay.romfs().join("romfs.bin"));
        touch(&lay.romfs().join("index.json"));
        assert!(first_need(&lay) < all - 10 * space::GB, "the merged RomFS is there");
        sys::remove_all(&root).unwrap();
    }

    #[test]
    fn a_source_folder_of_another_archive_with_the_same_commit_is_extracted_again() {
        let root = std::env::temp_dir().join(format!("s3l-marker-{}", crate::util::random_hex(4)));
        let lay = Layout::new(&root);
        let b1 = make_bundle(&root.join("b1"), &BASE, "c0");
        let mut other = BASE.to_vec();
        other.push(("kit-tools/s3_merge.py", "in place"));
        let b2 = make_bundle(&root.join("b2"), &other, "c0");
        bundle::extract(&std::fs::read(b1.join(bundle::ARCHIVE_NAME)).unwrap(), &lay.src()).unwrap();
        bundle::write_marker(&lay.src(), &bundle::load_manifest(&b1).unwrap()).unwrap();
        let req = InstallRequest { install_dir: root.display().to_string(), ..Default::default() };
        assert!(dry_run(&req, Some(&b1)).is_done(StepId::Sources, "c0"));
        assert!(!dry_run(&req, Some(&b2)).is_done(StepId::Sources, "c0"), "same commit, other kit-tools");
        sys::remove_all(&root).unwrap();
    }
}
