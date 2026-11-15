//! End to end against the local release server (`testkit.rs`, loopback only), with a test key the tests generate:
//! check, download (cut and resumed), verification, the swap of a program file, the restart and the witness, the
//! roll-back of a new version that crashes or hangs, and the refusals (another key, an altered file, a wrong size, an
//! older version, a redirect off the list, a rate limit). The "programs" are copies of this test executable in a
//! temporary folder; the copy started on trial runs [`child_entry`] with `KALAMAR_DATA` set to the test's own data
//! folder, or `KALAMAR_APPDATA` to a test folder standing for the profile (the update from a launcher of the earlier
//! identifier). Nothing outside the test folders is touched.

use super::testkit::{FakeServer, Route, TestKey, manifest_json, sha256_hex};
use super::*;
use std::process::Command;

const REPO: &str = "owner/kalamar";
const NEW: &str = "9.0.0";
const CUR: &str = "1.0.0";
const ENV_CHILD: &str = "KALAMAR_E2E_CHILD";
const ENV_VERSION: &str = "KALAMAR_E2E_VERSION";
const ENV_TRIAL: &str = "KALAMAR_E2E_TRIAL";

/// Runs inside the copied executable that the swap starts (never on its own: without the variable it returns). It does
/// what the launcher does: the start-up check of an update on trial, then, once "the page is up", the witness; or it
/// crashes, or hangs, as a broken new version would.
#[test]
#[ignore = "run inside a copied executable by the end-to-end tests"]
fn child_entry() {
    let Ok(what) = std::env::var(ENV_CHILD) else { return };
    // the data folder as the launcher decides it: the one named, else the one `datadir` finds in the test profile
    let data = std::env::var_os(crate::datadir::ENV_DATA).map(PathBuf::from).unwrap_or_else(|| crate::datadir::current().dir.clone()).join("update");
    let exe = std::env::current_exe().expect("exe");
    let version = std::env::var(ENV_VERSION).expect("version");
    let trial = std::env::var_os(ENV_TRIAL).is_some();
    if apply::early_start(&data, &exe, &version, trial, &|_| Ok(())) == apply::Early::Exit {
        return;
    }
    match what.as_str() {
        "crash" => std::process::exit(3),
        "hang" => loop {
            std::thread::sleep(Duration::from_secs(1));
        },
        _ => {}
    }
    assert!(apply::confirm(&data, &exe, &version, std::process::id()).is_some(), "on trial");
    apply::finish_confirmed(&data, &exe, &version, Duration::from_secs(20));
}

/// Developer aid for a trial run of `scripts/make-release.ps1` and `release_sign` without the maintainers' key (ignored;
/// `KALAMAR_TRIAL_DIR=<folder outside any repository>`): step `keys` writes a throwaway key pair there (`kalamar.key`
/// encrypted with the password `trial`, `kalamar.pub`, the public key line in `pub.txt`, and an unencrypted
/// `unencrypted.key` that the release tool refuses); step `sign` signs `<folder>\out\kalamar-release.json` with it.
#[test]
#[ignore = "developer aid for a trial run of the release tools"]
fn release_trial_helper() {
    let Ok(dir) = std::env::var("KALAMAR_TRIAL_DIR") else { return };
    let dir = PathBuf::from(dir);
    match std::env::var("KALAMAR_TRIAL_STEP").as_deref() {
        Ok("keys") => {
            let kp = minisign::KeyPair::generate_encrypted_keypair(Some("trial".into())).expect("key");
            std::fs::write(dir.join("kalamar.key"), kp.sk.to_box(Some("throwaway trial key")).expect("box").to_bytes()).expect("write");
            std::fs::write(dir.join("kalamar.pub"), kp.pk.to_box().expect("box").to_bytes()).expect("write");
            std::fs::write(dir.join("pub.txt"), kp.pk.to_base64()).expect("write");
            let un = minisign::KeyPair::generate_unencrypted_keypair().expect("key");
            std::fs::write(dir.join("unencrypted.key"), un.sk.to_box(None).expect("box").to_bytes()).expect("write");
        }
        Ok("sign") => {
            let out = dir.join("out");
            let sk = minisign::SecretKey::from_file(dir.join("kalamar.key"), Some("trial".into())).expect("encrypted key");
            let pk = minisign::PublicKey::from_secret_key(&sk).expect("public key");
            let data = std::fs::read(out.join(manifest::MANIFEST_NAME)).expect("manifest");
            let sig = minisign::sign(Some(&pk), &sk, std::io::Cursor::new(&data), None, None).expect("sign");
            std::fs::write(out.join(manifest::SIGNATURE_NAME), sig.into_string()).expect("write");
        }
        _ => {}
    }
}

struct Setup {
    root: PathBuf,
    data: PathBuf,
    exe: PathBuf,
    old: Vec<u8>,
    new: Vec<u8>,
    key: TestKey,
    web: FakeServer,
    assets: FakeServer,
}

impl Drop for Setup {
    fn drop(&mut self) {
        // the trial process may still be closing: a few tries
        for _ in 0..20 {
            if std::fs::remove_dir_all(&self.root).is_ok() || !self.root.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime")
}

/// A folder with `app\Kalamar.exe` (a copy of this test executable, the "old" version), the data folder, a test key,
/// and the two servers: `web` plays api.github.com and github.com, `assets` the release-asset host it redirects to.
fn setup(tag: &str) -> Setup {
    let root = std::env::temp_dir().join(format!("s3l-e2e-{tag}-{}", crate::util::random_hex(6)));
    let (app, data) = (root.join("app"), root.join("data"));
    std::fs::create_dir_all(&app).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    let me = std::env::current_exe().unwrap();
    let old = std::fs::read(&me).unwrap();
    let exe = app.join("Kalamar.exe");
    std::fs::write(&exe, &old).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // the "new" version: the same program with bytes appended (Windows ignores data after the sections), so its hash
    // differs and it still runs
    let mut new = old.clone();
    new.extend_from_slice(b"\0KALAMAR-E2E-NEW-VERSION\0");
    Setup { root, data, exe, old, new, key: TestKey::generate(), web: FakeServer::start(), assets: FakeServer::start() }
}

impl Setup {
    fn source(&self) -> Source {
        Source::local(&self.web.base(), REPO, vec![self.key.public()])
    }

    fn updater(&self, version: &str, temp: Vec<PathBuf>) -> Updater {
        Updater::new(Some(self.source()), &self.data, version, Some(self.exe.clone()), temp, Channel::Stable)
    }

    /// A release `tag` whose files are served by the asset host through redirects.
    fn publish(&self, tag: &str, manifest: &[u8], sig: &[u8], files: &[(&str, Route)]) {
        self.web.route(&format!("/api/repos/{REPO}/releases/latest"), Route { etag: Some("\"etag-1\"".into()), ..Route::json(serde_json::json!({"tag_name": tag, "prerelease": false, "draft": false, "body": "<img src=x onerror=alert(1)>"})) });
        let mut all: Vec<(String, Route)> = vec![(manifest::MANIFEST_NAME.into(), Route::ok(manifest.to_vec())), (manifest::SIGNATURE_NAME.into(), Route::ok(sig.to_vec()))];
        all.extend(files.iter().map(|(n, r)| (n.to_string(), r.clone())));
        for (name, route) in all {
            let a = format!("/a/{tag}/{name}");
            self.web.route(&format!("/web/{REPO}/releases/download/{tag}/{name}"), Route::redirect(&format!("{}{a}", self.assets.base())));
            self.assets.route(&a, route);
        }
    }

    fn release(&self, version: &str, files: &[(&str, &str, &[u8])]) {
        let list: Vec<(&str, &str, u64, String)> = files.iter().map(|(n, k, b)| (*n, *k, b.len() as u64, sha256_hex(b))).collect();
        let refs: Vec<(&str, &str, u64, &str)> = list.iter().map(|(n, k, s, h)| (*n, *k, *s, h.as_str())).collect();
        let m = manifest_json(REPO, version, "stable", &refs);
        let routes: Vec<(&str, Route)> = files.iter().map(|(n, _, b)| (*n, Route { ranges: true, ..Route::ok(b.to_vec()) })).collect();
        self.publish(&format!("v{version}"), &m, &self.key.sign(&m), &routes);
    }

    /// Starts the copied program at `exe` as the swap would, running [`child_entry`] in `what` mode.
    fn spawner(&self, what: &'static str) -> impl Fn(&Path) -> std::io::Result<std::process::Child> + use<> {
        let data = self.data.clone();
        move |exe: &Path| {
            Command::new(exe)
                .args(["--exact", "update::e2e_tests::child_entry", "--ignored", "--nocapture", "--test-threads=1"])
                .env(ENV_CHILD, what)
                .env(ENV_VERSION, NEW)
                .env(ENV_TRIAL, "1")
                .env(crate::datadir::ENV_DATA, &data)
                .current_dir(exe.parent().unwrap())
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
        }
    }
}

fn prefs() -> UpdatePrefs {
    UpdatePrefs::default()
}

fn download(rt: &tokio::runtime::Runtime, u: &Updater) {
    let job = u.begin_download().expect("a download starts");
    rt.block_on(u.run_download(job, &|| {}));
}

#[test]
fn end_to_end_download_verify_swap_restart_witness() {
    let s = setup("ok");
    s.release(NEW, &[("Kalamar.exe", "portable", &s.new)]);
    // the connection is cut a third of the way: the download resumes with a range
    let a = format!("/a/v{NEW}/Kalamar.exe");
    s.assets.route(&a, Route { ranges: true, cut_after: Some(s.new.len() / 3), ..Route::ok(s.new.clone()) });
    let rt = rt();
    let u = s.updater(CUR, vec![]);
    assert_eq!(u.view(&prefs(), None).available, None);
    rt.block_on(u.check(Channel::Stable, true, &|| {}));
    let v = u.view(&prefs(), None);
    let av = v.available.clone().expect("an update is offered");
    assert_eq!((av.version.as_str(), av.mode, av.size), (NEW, Mode::Swap, s.new.len() as u64));
    assert!(av.notes.contains("test notes") && !av.notes.contains("onerror"), "notes come from the signed manifest, never the API body");
    assert_eq!(v.phase, "idle", "nothing is downloaded before the player asks");
    assert!(s.assets.requests(&a).is_empty());

    // the next check is answered by the ETag (304), and the verified manifest is not fetched again
    rt.block_on(u.check(Channel::Stable, false, &|| {}));
    let latest = s.web.requests(&format!("/api/repos/{REPO}/releases/latest"));
    assert_eq!(latest.len(), 2);
    assert_eq!(latest[1].header("If-None-Match"), Some("\"etag-1\""));
    assert_eq!(s.assets.requests(&format!("/a/v{NEW}/{}", manifest::MANIFEST_NAME)).len(), 1);
    // minimal requests: no token, no cookie, the fixed agent
    for r in s.web.seen.lock().unwrap().iter().chain(s.assets.seen.lock().unwrap().iter()) {
        assert!(r.header("Authorization").is_none() && r.header("Cookie").is_none() && r.header("Referer").is_none(), "{r:?}");
        assert_eq!(r.header("User-Agent"), Some(net::USER_AGENT));
    }

    download(&rt, &u);
    assert_eq!(u.view(&prefs(), None).phase, "ready");
    let reqs = s.assets.requests(&a);
    assert_eq!(reqs.len(), 2, "cut, then resumed");
    assert_eq!(reqs[1].header("Range"), Some(format!("bytes={}-", s.new.len() / 3).as_str()));

    // "Restart to finish": the swap, the new program started on trial, its witness
    let (mut child, paths, to) = u.apply_swap(&s.spawner("ok")).expect("swap");
    assert_eq!(to, NEW);
    assert_eq!(u.view(&prefs(), None).phase, "restarting");
    assert!(u.apply_swap(&s.spawner("ok")).is_err(), "a second click starts nothing");
    assert!(std::fs::read(&s.exe).unwrap() == s.new, "the new program is in place");
    let outcome = apply::watch(&mut child, u.dir(), &to, Duration::from_secs(90), Duration::from_millis(100));
    assert_eq!(outcome, apply::TrialOutcome::Started);
    let _ = child.wait();
    // the new program removed the old file once confirmed
    assert!(!paths.old.exists() && !paths.new.exists() && !paths.failed.exists());
    assert!(matches!(apply::load_journal(u.dir()), Some(apply::Journal::Done { .. })));

    // the next start, as the new version: no update left, the stored manifest and the download are gone
    let next = s.updater(NEW, vec![]);
    assert_eq!(apply::early_start(next.dir(), &s.exe, NEW, false, &|_| panic!("no relaunch")), apply::Early::Continue);
    assert_eq!(next.view(&prefs(), None).available, None);
    assert!(!next.dir().join("available").exists() && !next.dir().join("download").exists());
}

/// The update that changes the identifier: the old version keeps its data in the folder of the earlier identifier and
/// waits there for the witness; the new version, started on trial with no data folder named, finds that folder (the
/// update's journal keeps the folder in place for that session), confirms in it, and the folder moves at the next start
/// with the update's finished journal in it.
#[test]
fn end_to_end_an_update_from_the_earlier_identifier_confirms_then_moves_the_folder() {
    let s = setup("ident");
    let appdata = s.root.join("appdata");
    let roots = crate::datadir::Roots { roaming: appdata.join("Roaming"), local: appdata.join("Local") };
    let legacy = roots.legacy();
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(legacy.join("config.json"), r#"{"language":"fr"}"#).unwrap();
    s.release(NEW, &[("Kalamar.exe", "portable", &s.new)]);
    let rt = rt();
    let u = Updater::new(Some(s.source()), &legacy, CUR, Some(s.exe.clone()), vec![], Channel::Stable);
    rt.block_on(u.check(Channel::Stable, true, &|| {}));
    download(&rt, &u);
    let spawn = |exe: &Path| {
        Command::new(exe)
            .args(["--exact", "update::e2e_tests::child_entry", "--ignored", "--nocapture", "--test-threads=1"])
            .env(ENV_CHILD, "ok")
            .env(ENV_VERSION, NEW)
            .env(ENV_TRIAL, "1")
            .env_remove(crate::datadir::ENV_DATA)
            .env_remove(crate::datadir::LEGACY_ENV_DATA)
            .env(crate::datadir::ENV_APPDATA, &appdata)
            .current_dir(exe.parent().unwrap())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    };
    let (mut child, paths, to) = u.apply_swap(&spawn).expect("swap");
    let outcome = apply::watch(&mut child, u.dir(), &to, Duration::from_secs(90), Duration::from_millis(100));
    assert_eq!(outcome, apply::TrialOutcome::Started, "the new version confirmed in the earlier folder");
    let _ = child.wait();
    assert!(!paths.old.exists());
    assert!(matches!(apply::load_journal(u.dir()), Some(apply::Journal::Done { .. })));
    assert!(!roots.data().exists(), "nothing moved during the trial");
    assert!(std::fs::read_to_string(legacy.join(crate::datadir::LOG)).unwrap().contains("UpdateTrial"), "the decision is logged");
    drop(u);
    // the next start of the new version: the folder moves, the finished update with it
    let probes = crate::datadir::Probes { old_launcher_open: &|| false, rename: &|a, b| std::fs::rename(a, b), stop_at: None };
    let (dir, out) = crate::datadir::resolve(&roots, &probes, true, crate::util::now_secs()).unwrap();
    assert_eq!(dir, roots.data());
    assert!(matches!(out, crate::datadir::Outcome::Moved { .. }), "{out:?}");
    assert!(matches!(apply::load_journal(&dir.join("update")), Some(apply::Journal::Done { .. })));
    assert!(!legacy.exists());
    let next = Updater::new(Some(s.source()), &dir, NEW, Some(s.exe.clone()), vec![], Channel::Stable);
    assert_eq!(apply::early_start(next.dir(), &s.exe, NEW, false, &|_| panic!("no relaunch")), apply::Early::Continue);
}

#[test]
fn end_to_end_a_new_version_that_crashes_or_hangs_is_rolled_back() {
    for (what, timeout) in [("crash", Duration::from_secs(90)), ("hang", Duration::from_secs(4))] {
        let s = setup(what);
        s.release(NEW, &[("Kalamar.exe", "portable", &s.new)]);
        let rt = rt();
        let u = s.updater(CUR, vec![]);
        rt.block_on(u.check(Channel::Stable, true, &|| {}));
        download(&rt, &u);
        let (mut child, paths, to) = u.apply_swap(&s.spawner(what)).expect("swap");
        if what == "hang" {
            // the program on trial runs from Kalamar.exe: it is "another Kalamar on this file" for a second launcher
            let seen = (0..50).any(|_| {
                std::thread::sleep(Duration::from_millis(100));
                apply::other_instances(&s.exe).contains(&child.id())
            });
            assert!(seen, "the running copy is found by its path");
        }
        let outcome = apply::watch(&mut child, u.dir(), &to, timeout, Duration::from_millis(100));
        match what {
            "crash" => assert_eq!(outcome, apply::TrialOutcome::Exited(Some(3))),
            _ => assert_eq!(outcome, apply::TrialOutcome::TimedOut),
        }
        // what the old process does then
        apply::roll_back(&paths, u.dir(), CUR, &to, &outcome.reason()).expect("roll back");
        u.after_rollback(&to, true);
        assert!(std::fs::read(&s.exe).unwrap() == s.old, "{what}: the old program is back");
        assert!(!paths.old.exists());
        let v = u.view(&prefs(), None);
        assert_eq!(v.notice.as_ref().map(|n| n.kind), Some("rolled_back"));
        assert_eq!(v.phase, "ready", "it can be tried again");
        assert!(matches!(apply::load_journal(u.dir()), Some(apply::Journal::RolledBack { .. })));
        // the old version, started again, is not on trial and leaves nothing behind
        assert_eq!(apply::early_start(u.dir(), &s.exe, CUR, false, &|_| panic!("no relaunch")), apply::Early::Continue);
        apply::cleanup_leftovers(u.dir(), &s.exe);
        assert!(!paths.failed.exists());
    }
}

#[test]
fn a_manifest_signed_with_another_key_offers_nothing() {
    let s = setup("key");
    let m = manifest_json(REPO, NEW, "stable", &[("Kalamar.exe", "portable", s.new.len() as u64, &sha256_hex(&s.new))]);
    let stranger = TestKey::generate();
    s.publish(&format!("v{NEW}"), &m, &stranger.sign(&m), &[("Kalamar.exe", Route::ok(s.new.clone()))]);
    let rt = rt();
    let u = s.updater(CUR, vec![]);
    rt.block_on(u.check(Channel::Stable, true, &|| {}));
    let v = u.view(&prefs(), None);
    assert_eq!((v.available, v.check_error.as_deref()), (None, Some("untrusted")));
    assert!(s.assets.requests(&format!("/a/v{NEW}/Kalamar.exe")).is_empty(), "the program is never fetched");
    // no manifest at all: nothing either
    let s2 = setup("nomanifest");
    s2.web.route(&format!("/api/repos/{REPO}/releases/latest"), Route::json(serde_json::json!({"tag_name": "v9.0.0"})));
    let u2 = s2.updater(CUR, vec![]);
    rt.block_on(u2.check(Channel::Stable, true, &|| {}));
    assert_eq!(u2.view(&prefs(), None).available, None);
}

#[test]
fn an_altered_or_longer_file_is_refused_and_never_kept() {
    for (tag, served) in [("altered", 0usize), ("longer", 1)] {
        let s = setup(tag);
        s.release(NEW, &[("Kalamar.exe", "portable", &s.new)]);
        let a = format!("/a/v{NEW}/Kalamar.exe");
        let mut bad = s.new.clone();
        if served == 0 {
            let i = bad.len() / 2;
            bad[i] ^= 0xff; // same size, one byte changed
        } else {
            bad.extend_from_slice(b"more"); // larger than signed
        }
        s.assets.route(&a, Route::ok(bad));
        let rt = rt();
        let u = s.updater(CUR, vec![]);
        rt.block_on(u.check(Channel::Stable, true, &|| {}));
        download(&rt, &u);
        let v = u.view(&prefs(), None);
        assert_eq!((v.phase, v.error.as_deref()), ("idle", Some("integrity")), "{tag}");
        let dl = u.dir().join("download").join(format!("v{NEW}"));
        assert!(std::fs::read_dir(&dl).map(|d| d.count()).unwrap_or(0) == 0, "{tag}: nothing kept");
        assert!(std::fs::read(&s.exe).unwrap() == s.old);
        assert!(u.apply_swap(&s.spawner("ok")).is_err(), "nothing to apply");
    }
}

#[test]
fn an_older_or_mislabelled_version_is_never_offered() {
    let s = setup("old");
    // the API's latest release is older than the running launcher (a roll-back attempt)
    s.release("0.9.0", &[("Kalamar.exe", "portable", &s.new)]);
    let rt = rt();
    let u = s.updater(CUR, vec![]);
    rt.block_on(u.check(Channel::Stable, true, &|| {}));
    let v = u.view(&prefs(), None);
    assert_eq!((v.available, v.check_error), (None, None), "up to date");
    assert!(s.assets.requests(&format!("/a/v0.9.0/{}", manifest::MANIFEST_NAME)).is_empty());
    // a signed 0.9.0 manifest published under the tag v9.0.0
    let s = setup("mislabel");
    let m = manifest_json(REPO, "0.9.0", "stable", &[("Kalamar.exe", "portable", s.new.len() as u64, &sha256_hex(&s.new))]);
    s.publish("v9.0.0", &m, &s.key.sign(&m), &[("Kalamar.exe", Route::ok(s.new.clone()))]);
    let u = s.updater(CUR, vec![]);
    rt.block_on(u.check(Channel::Stable, true, &|| {}));
    let v = u.view(&prefs(), None);
    assert_eq!((v.available, v.check_error.as_deref()), (None, Some("untrusted")));
}

#[test]
fn a_redirect_off_the_list_is_not_followed() {
    let s = setup("redirect");
    s.release(NEW, &[("Kalamar.exe", "portable", &s.new)]);
    // the manifest's download now redirects to another host name (localhost is not on the list, even on this machine)
    s.web.route(&format!("/web/{REPO}/releases/download/v{NEW}/{}", manifest::MANIFEST_NAME), Route::redirect(&format!("http://localhost:{}/a/v{NEW}/{}", s.assets.port, manifest::MANIFEST_NAME)));
    let rt = rt();
    let u = s.updater(CUR, vec![]);
    rt.block_on(u.check(Channel::Stable, true, &|| {}));
    let v = u.view(&prefs(), None);
    assert_eq!(v.available, None);
    assert_eq!(v.check_error.as_deref(), Some("untrusted"));
    assert!(s.assets.requests(&format!("/a/v{NEW}/{}", manifest::MANIFEST_NAME)).is_empty(), "never reached");
    // the program itself redirected off the list after a good check: the download fails, nothing is kept
    let s = setup("redirect2");
    s.release(NEW, &[("Kalamar.exe", "portable", &s.new)]);
    let u = s.updater(CUR, vec![]);
    rt.block_on(u.check(Channel::Stable, true, &|| {}));
    s.web.route(&format!("/web/{REPO}/releases/download/v{NEW}/Kalamar.exe"), Route::redirect("https://evil.example/Kalamar.exe"));
    download(&rt, &u);
    let v = u.view(&prefs(), None);
    assert_eq!((v.phase, v.error.as_deref()), ("idle", Some("untrusted")));
}

#[test]
fn rate_limits_and_missing_releases_are_quiet() {
    let s = setup("rate");
    let reset = crate::util::now_secs() + 1800;
    s.web.route(&format!("/api/repos/{REPO}/releases/latest"), Route { status: 403, headers: vec![("x-ratelimit-remaining".into(), "0".into()), ("x-ratelimit-reset".into(), reset.to_string())], ..Default::default() });
    let rt = rt();
    let u = s.updater(CUR, vec![]);
    rt.block_on(u.check(Channel::Stable, false, &|| {}));
    assert_eq!(u.view(&prefs(), None).check_error.as_deref(), Some("rate_limited"));
    // until the reset, no request at all (even "Check now" after the 30 s gap)
    u.lock().last_attempt = 0;
    rt.block_on(u.check(Channel::Stable, true, &|| {}));
    assert_eq!(s.web.requests(&format!("/api/repos/{REPO}/releases/latest")).len(), 1);
    // a repository without any stable release yet: up to date
    let s = setup("none");
    s.web.route(&format!("/api/repos/{REPO}/releases/latest"), Route::status(404));
    let u = s.updater(CUR, vec![]);
    rt.block_on(u.check(Channel::Stable, true, &|| {}));
    let v = u.view(&prefs(), None);
    assert_eq!((v.available, v.check_error, v.last_check.is_some()), (None, None, true));
}

#[test]
fn the_check_cadence_is_every_start_then_every_six_hours() {
    assert_eq!(auto_wait(true), Duration::from_secs(20));
    assert_eq!(auto_wait(false), Duration::from_secs(6 * 3600));
}

#[test]
fn a_check_at_every_start_costs_a_conditional_request() {
    let s = setup("everystart");
    s.release(NEW, &[("Kalamar.exe", "portable", &s.new)]);
    let rt = rt();
    let u = s.updater(CUR, vec![]);
    rt.block_on(u.check(Channel::Stable, false, &|| {}));
    drop(u);
    // the launcher is started again right away (a new Updater reads what the first kept on disk): it checks again,
    // with the ETag, and the verified manifest is not fetched again
    let u = s.updater(CUR, vec![]);
    assert!(u.view(&prefs(), None).available.is_some(), "the stored offer is back at once");
    rt.block_on(u.check(Channel::Stable, false, &|| {}));
    let latest = s.web.requests(&format!("/api/repos/{REPO}/releases/latest"));
    assert_eq!(latest.len(), 2);
    assert_eq!(latest[1].header("If-None-Match"), Some("\"etag-1\""));
    assert_eq!(s.assets.requests(&format!("/a/v{NEW}/{}", manifest::MANIFEST_NAME)).len(), 1);
}

#[test]
fn later_holds_for_this_run_only() {
    let s = setup("later");
    s.release(NEW, &[("Kalamar.exe", "portable", &s.new)]);
    let rt = rt();
    let u = s.updater(CUR, vec![]);
    rt.block_on(u.check(Channel::Stable, false, &|| {}));
    assert!(!u.view(&prefs(), None).later);
    u.later();
    assert!(u.view(&prefs(), None).later);
    drop(u);
    let u = s.updater(CUR, vec![]);
    let v = u.view(&prefs(), None);
    assert!(v.available.is_some() && !v.later, "a new start shows it again");
}

#[test]
fn the_beta_channel_sees_pre_releases_and_leaving_it_drops_them() {
    let s = setup("beta");
    let v = "9.1.0-beta.1";
    let m = manifest_json(REPO, v, "beta", &[("Kalamar.exe", "portable", s.new.len() as u64, &sha256_hex(&s.new))]);
    let tag = format!("v{v}");
    s.publish(&tag, &m, &s.key.sign(&m), &[("Kalamar.exe", Route::ok(s.new.clone()))]);
    // the stable answer (`latest`) knows only an older stable release; the list has the pre-release
    s.web.route(&format!("/api/repos/{REPO}/releases/latest"), Route::json(serde_json::json!({"tag_name": "v0.9.0"})));
    s.web.route(&format!("/api/repos/{REPO}/releases?per_page=10"), Route::json(serde_json::json!([{"tag_name": tag, "prerelease": true}, {"tag_name": "v0.9.0"}])));
    let rt = rt();
    let u = s.updater(CUR, vec![]);
    rt.block_on(u.check(Channel::Stable, true, &|| {}));
    assert_eq!(u.view(&prefs(), None).available, None, "stable: nothing newer");
    u.lock().last_attempt = 0;
    rt.block_on(u.check(Channel::Beta, true, &|| {}));
    let av = u.view(&prefs(), None).available.expect("beta: the pre-release");
    assert_eq!((av.version.as_str(), av.channel), (v, Channel::Beta));
    // back to stable: dropped at once, and not offered again at the next start
    u.channel_changed(Channel::Stable);
    assert_eq!(u.view(&prefs(), None).available, None);
    assert_eq!(s.updater(CUR, vec![]).view(&prefs(), None).available, None);
}

#[test]
fn a_build_without_a_repository_does_nothing() {
    let d = std::env::temp_dir().join(format!("s3l-e2e-off-{}", crate::util::random_hex(6)));
    let u = Updater::new(None, &d, CUR, None, vec![], Channel::Stable);
    let rt = rt();
    rt.block_on(u.check(Channel::Stable, true, &|| panic!("no check")));
    assert!(u.begin_download().is_none());
    let v = u.view(&prefs(), None);
    assert!(!v.enabled && v.available.is_none());
    assert!(!d.exists(), "not even a folder");
}

#[test]
fn a_copy_run_from_a_temporary_folder_saves_the_new_version_elsewhere() {
    let s = setup("temp");
    s.release(NEW, &[("Kalamar.exe", "portable", &s.new)]);
    let rt = rt();
    // the program's folder counts as temporary (an archive opened in Explorer)
    let u = s.updater(CUR, vec![s.root.join("app")]);
    rt.block_on(u.check(Channel::Stable, true, &|| {}));
    let av = u.view(&prefs(), None).available.unwrap();
    assert_eq!((av.mode, av.why), (Mode::Elsewhere, Some("temp")));
    download(&rt, &u);
    assert!(u.apply_swap(&s.spawner("ok")).is_err(), "never swapped in place");
    let out = s.root.join("Programs");
    std::fs::create_dir_all(&out).unwrap();
    let saved = u.save_to(&out).expect("saved");
    assert!(std::fs::read(&saved).unwrap() == s.new);
    assert_eq!(u.view(&prefs(), None).saved_to, Some(saved.display().to_string()));
    assert!(std::fs::read(&s.exe).unwrap() == s.old, "the running copy is left alone");
    // a temporary folder is refused as a destination
    assert!(u.save_to(&s.root.join("app")).is_err());
}

#[test]
fn an_installed_copy_gets_the_installer_checked_again_before_it_starts() {
    let s = setup("installer");
    std::fs::write(s.root.join("app").join(apply::NSIS_UNINSTALLER), b"nsis").unwrap();
    let installer = b"pretend installer".to_vec();
    s.release(NEW, &[("Kalamar.exe", "portable", &s.new), ("Kalamar_9.0.0_x64-setup.exe", "installer", &installer)]);
    let rt = rt();
    let u = s.updater(CUR, vec![]);
    rt.block_on(u.check(Channel::Stable, true, &|| {}));
    let av = u.view(&prefs(), None).available.unwrap();
    assert_eq!((av.mode, av.size), (Mode::Installer, installer.len() as u64));
    download(&rt, &u);
    let (file, entry, _, mode) = u.ready().unwrap();
    assert_eq!((mode, entry.kind), (Mode::Installer, FileKind::Installer));
    // altered on disk after the download: refused, removed, nothing started
    std::fs::write(&file, b"pretend installeR").unwrap();
    assert_eq!(u.apply_installer(), Err(apply::ApplyError::Integrity));
    assert!(!file.exists());
    assert_eq!(apply::load_journal(u.dir()), None);
    assert_eq!(u.view(&prefs(), None).error.as_deref(), Some("integrity"));
}
