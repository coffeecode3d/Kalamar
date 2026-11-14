//! Runs the install wizard's pipeline without the window, for testing the Linux port on a real machine (or any
//! machine without a display). Same code as the interface's Install button: `installer::Installer::start`.
//!
//!   cargo run --example install_headless -- --dir <install folder> --base <base.nsp> --update <11.2.0 update.nsp> \
//!       --keys <prod.keys> [--bundle <resources folder>] [--jobs N] [--plan] [--stop-before <step>] [--stop-after <step>]
//!
//! `--plan` prints what the wizard would do and exits. `--stop-before compile` sets the cancel flag when the named step
//! starts (everything before it stays). The keys file is only passed to the tools by path; this program never opens it.
//! Exit code: 0 done, 1 failed or cancelled, 2 bad arguments.

use kalamar_lib::installer::{self, InstallRequest, Installer};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

fn arg(name: &str) -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.iter().position(|x| x == name).and_then(|i| a.get(i + 1)).cloned()
}

fn flag(name: &str) -> bool {
    std::env::args().any(|x| x == name)
}

fn need(name: &str) -> String {
    match arg(name) {
        Some(v) => v,
        None => {
            eprintln!("install_headless: missing {name}");
            std::process::exit(2);
        }
    }
}

fn main() {
    let dir = PathBuf::from(need("--dir"));
    let req = InstallRequest {
        install_dir: dir.display().to_string(),
        base_nsp: need("--base"),
        update_nsp: need("--update"),
        prod_keys: need("--keys"),
        jobs: arg("--jobs").and_then(|j| j.parse().ok()).unwrap_or(0),
        low_priority: flag("--low-priority"),
        ..Default::default()
    };
    let bundle = arg("--bundle").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources"));
    let stop_before = arg("--stop-before");
    let stop_after = arg("--stop-after");

    // The inputs are named, never read: the tools the wizard runs read them.
    for (what, p) in [("install folder", dir.as_path()), ("base", Path::new(&req.base_nsp)), ("update", Path::new(&req.update_nsp)), ("keys", Path::new(&req.prod_keys))] {
        if what == "install folder" {
            continue;
        }
        if !p.is_file() {
            eprintln!("install_headless: {what} is not a file: {}", p.display());
            std::process::exit(2);
        }
    }

    if flag("--plan") {
        let p = installer::plan(&req, Some(&bundle));
        println!("install: {}", p.install_dir);
        println!("game root: {}", p.game_root);
        println!("toolchain: {:?}", p.toolchain);
        println!("disk: need {} GB, free {}", p.disk_needed / 1_000_000_000, p.disk_free.map(|b| b / 1_000_000_000).map(|n| n.to_string()).unwrap_or_else(|| "?".into()));
        println!("jobs: {}", p.jobs);
        if !p.missing_packages.is_empty() {
            println!("missing packages: {}", p.missing_packages.join(", "));
        }
        for s in &p.steps {
            println!("  {:<9} {}", if s.done { "done" } else { "todo" }, format!("{:?}", s.id).to_lowercase());
        }
        return;
    }

    let inst = Installer::default();
    let emit: installer::Emit = Arc::new(|s: &installer::Snapshot| {
        if let Some(e) = &s.error {
            eprintln!("[error] {:?}: {}", e.step, e.code);
        }
    });
    let on_root: installer::OnRoot = Arc::new(|root: &Path| println!("[root] {}", root.display()));
    if let Err(e) = inst.start(req, Some(bundle), emit, on_root) {
        eprintln!("install_headless: {e}");
        std::process::exit(1);
    }

    let mut last = String::new();
    loop {
        let s = inst.snapshot();
        let now = format!(
            "{} {:?} {}",
            s.current.map(|c| format!("{c:?}").to_lowercase()).unwrap_or_else(|| "-".into()),
            s.stage,
            s.detail.clone().unwrap_or_default()
        );
        if now != last {
            println!("[{:>5.1}%] {}", s.overall * 100.0, now.trim_end());
            last = now;
        }
        if let Some(step) = &stop_before
            && s.current.is_some_and(|c| format!("{c:?}").eq_ignore_ascii_case(step))
            && !s.cancel_requested
        {
            println!("[stop] cancelling before {step}");
            inst.cancel();
        }
        if let Some(step) = &stop_after
            && s.steps.iter().any(|v| format!("{:?}", v.id).eq_ignore_ascii_case(step) && v.status == "done")
            && !s.cancel_requested
        {
            println!("[stop] {step} is done, cancelling");
            inst.cancel();
        }
        if !s.active {
            println!("[outcome] {:?}", s.outcome);
            if let Some(e) = &s.error {
                eprintln!("[error] step {:?}, code {} {:?}", e.step, e.code, e.params);
                if let Some(log) = &e.log {
                    eprintln!("[log] {log}");
                }
                for line in &e.tail {
                    eprintln!("  {line}");
                }
            }
            std::process::exit(if s.outcome == Some("done") { 0 } else { 1 });
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}
