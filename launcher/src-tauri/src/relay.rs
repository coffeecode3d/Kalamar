//! Optional local relay: starts and stops the `splat3-relay` program (a separate project, never copied into this
//! repository) on this PC for internet LAN play. The launcher never touches the firewall or the router: the user must
//! allow the program and open or forward UDP 47313 (or the chosen port) themselves.

use crate::error::{AppError, AppResult};
use crate::util;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

pub const EXE_NAME: &str = if cfg!(windows) { "splat3-relay.exe" } else { "splat3-relay" };

/// Where the relay binary is looked for when no path is configured: `relay\target\release\splat3-relay.exe` in a
/// folder above the game root or the launcher.
pub fn locate(configured: Option<&Path>, starts: &[PathBuf]) -> Option<PathBuf> {
    if let Some(p) = configured
        && p.is_file()
    {
        return Some(p.to_path_buf());
    }
    for start in starts {
        for dir in start.ancestors() {
            let cand = dir.join("relay").join("target").join("release").join(EXE_NAME);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

/// Command line of the relay: every interface, the chosen UDP port (`splat3-relay --bind 0.0.0.0 --port 47313`).
pub fn relay_args(port: u16) -> Vec<String> {
    vec!["--bind".into(), "0.0.0.0".into(), "--port".into(), port.to_string()]
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RelayStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub port: Option<u16>,
    pub exe: Option<String>,
    pub log_path: String,
    /// Last lines of the relay's own log.
    pub tail: Option<String>,
    /// The relay stopped by itself (exit code) since it was started.
    pub exited: Option<i32>,
}

struct Running {
    child: Child,
    port: u16,
}

#[derive(Clone, Default)]
pub struct RelayManager {
    inner: Arc<Mutex<(Option<Running>, Option<i32>)>>,
}

impl RelayManager {
    fn lock(&self) -> std::sync::MutexGuard<'_, (Option<Running>, Option<i32>)> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn status(&self, exe: Option<&Path>, log: &Path) -> RelayStatus {
        let mut g = self.lock();
        if let Some(r) = g.0.as_mut()
            && let Ok(Some(st)) = r.child.try_wait()
        {
            g.1 = Some(st.code().unwrap_or(-1));
            g.0 = None;
        }
        RelayStatus {
            running: g.0.is_some(),
            pid: g.0.as_ref().map(|r| r.child.id()),
            port: g.0.as_ref().map(|r| r.port),
            exe: exe.map(|p| p.display().to_string()),
            log_path: log.display().to_string(),
            tail: util::tail_lines(log, 12).ok().flatten(),
            exited: g.1,
        }
    }

    pub fn start(&self, exe: &Path, port: u16, log: &Path) -> AppResult<()> {
        if port == 0 {
            return Err(AppError::InvalidInput("relay port must be 1 to 65535".into()));
        }
        if !exe.is_file() {
            return Err(AppError::NotAvailable(format!("relay program not found: {}", exe.display())));
        }
        let mut g = self.lock();
        if let Some(r) = g.0.as_mut()
            && matches!(r.child.try_wait(), Ok(None))
        {
            return Err(AppError::AlreadyRunning);
        }
        if let Some(dir) = log.parent() {
            std::fs::create_dir_all(dir).map_err(|e| AppError::io(dir, &e))?;
        }
        let out = std::fs::File::create(log).map_err(|e| AppError::io(log, &e))?;
        let err = out.try_clone().map_err(|e| AppError::io(log, &e))?;
        let mut cmd = Command::new(exe);
        cmd.args(relay_args(port)).stdin(Stdio::null()).stdout(out).stderr(err);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let child = cmd.spawn().map_err(|e| AppError::io(exe, &e))?;
        *g = (Some(Running { child, port }), None);
        Ok(())
    }

    pub fn stop(&self) -> AppResult<()> {
        let mut g = self.lock();
        match g.0.take() {
            Some(mut r) => {
                let _ = r.child.kill();
                let _ = r.child.wait();
                g.1 = None;
                Ok(())
            }
            None => Err(AppError::NotRunning),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_bind_all_interfaces_on_the_chosen_port() {
        assert_eq!(relay_args(47313), vec!["--bind", "0.0.0.0", "--port", "47313"]);
        assert_eq!(relay_args(9000)[3], "9000");
    }

    #[test]
    fn locate_prefers_the_configured_path_then_walks_up() {
        let base = std::env::temp_dir().join(format!("s3l-relay-{}", util::random_hex(4)));
        let exe = base.join("relay").join("target").join("release").join(EXE_NAME);
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"x").unwrap();
        let deep = base.join("work").join("game");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(locate(None, std::slice::from_ref(&deep)), Some(exe.clone()));
        assert_eq!(locate(Some(&base.join("nope.exe")), std::slice::from_ref(&deep)), Some(exe.clone()));
        let own = base.join("mine.exe");
        std::fs::write(&own, b"x").unwrap();
        assert_eq!(locate(Some(&own), &[]), Some(own));
        assert_eq!(locate(None, &[std::env::temp_dir().join("s3l-nowhere-xyz")]), None);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn start_status_stop_with_a_stand_in_program() {
        let sys = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let exe = sys.join("System32").join("ping.exe"); // rejects the relay flags and exits by itself
        let log = std::env::temp_dir().join(format!("s3l-relaylog-{}", util::random_hex(4))).join("relay.log");
        let m = RelayManager::default();
        assert!(matches!(m.stop(), Err(AppError::NotRunning)));
        assert!(m.start(&exe, 0, &log).is_err());
        assert!(m.start(&log, 47313, &log).is_err()); // not an existing program
        m.start(&exe, 47313, &log).unwrap();
        for _ in 0..50 {
            if !m.status(Some(&exe), &log).running {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let st = m.status(Some(&exe), &log);
        assert!(!st.running && st.exited.is_some());
        std::fs::remove_dir_all(log.parent().unwrap()).unwrap();
    }
}
