//! Bringing the game's window to the front ("Back to the game"). Windows only; elsewhere it reports nothing found. And
//! the process-wide safety of program and DLL lookups.

use std::path::PathBuf;

/// A program of Windows by its full path in System32 (`GetSystemDirectoryW`), never a bare name: Windows looks for a
/// bare name in the launcher's own folder first, so a file dropped next to a portable launcher would run instead.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn system_exe(name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        let mut buf = [0u16; 512];
        // SAFETY: the buffer and its length in characters; the call writes at most that many.
        let n = unsafe { windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
        if n > 0 && n < buf.len() {
            return PathBuf::from(std::ffi::OsString::from_wide(&buf[..n])).join(name);
        }
        std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows")).join("System32").join(name)
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/usr/bin").join(name)
    }
}

/// A program of the Windows folder itself (`explorer.exe`), by its full path: the parent of System32.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn windows_exe(name: &str) -> PathBuf {
    let sys = system_exe("x");
    sys.parent().and_then(|s| s.parent()).map(|w| w.join(name)).unwrap_or_else(|| PathBuf::from(r"C:\Windows").join(name))
}

/// At start-up: the DLLs loaded from now on by name come from System32 only (`SetDefaultDllDirectories`), never from
/// the launcher's folder or the current directory. Those the executable imports are covered by its link flag
/// (`build.rs`, `/DEPENDENTLOADFLAG`). A DLL loaded by its full path (the WebView2 runtime's) is not affected.
pub fn harden() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::LibraryLoader::{LOAD_LIBRARY_SEARCH_SYSTEM32, SetDefaultDllDirectories};
        // SAFETY: a process-wide setting with a valid flag; nothing is passed by pointer.
        unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32) };
    }
}

/// Chooses the window to focus among `(window id, owner pid, title)`: one owned by `pid` first, else one owned by
/// another process of the game (the game restarts itself once on a first launch, and the new process is not our
/// child), never the second local player's window (its title starts with `Kalamar`).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn pick_window(windows: &[(isize, u32, String)], pid: Option<u32>, game_pids: &[u32]) -> Option<isize> {
    let ours = |w: &&(isize, u32, String)| !w.2.starts_with("Kalamar");
    pid.and_then(|p| windows.iter().find(|w| w.1 == p).map(|w| w.0))
        .or_else(|| windows.iter().filter(ours).find(|w| game_pids.contains(&w.1)).map(|w| w.0))
}

/// The launcher's window is the desktop's foreground window (the player is looking at it).
pub fn is_foreground(w: &tauri::WebviewWindow) -> bool {
    #[cfg(windows)]
    {
        let Ok(h) = w.hwnd() else { return w.is_focused().unwrap_or(false) };
        // SAFETY: a plain query of the foreground window.
        let fg = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
        fg as isize == h.0 as isize
    }
    #[cfg(not(windows))]
    {
        w.is_focused().unwrap_or(false)
    }
}

/// The visible top-level windows that have no owner, as `(window id, owner pid, title)`.
#[cfg(windows)]
fn top_windows() -> Vec<(isize, u32, String)> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let mut found: Vec<(isize, u32, String)> = Vec::new();
    unsafe extern "system" fn each(h: HWND, l: LPARAM) -> i32 {
        // SAFETY: `l` is the address of the Vec passed below, alive for the whole enumeration.
        let out = unsafe { &mut *(l as *mut Vec<(isize, u32, String)>) };
        let mut owner = 0u32;
        unsafe { GetWindowThreadProcessId(h, &mut owner) };
        if unsafe { IsWindowVisible(h) } == 0 || !unsafe { GetWindow(h, GW_OWNER) }.is_null() {
            return 1;
        }
        let mut buf = [0u16; 128];
        let n = unsafe { GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32) }.max(0) as usize;
        out.push((h as isize, owner, String::from_utf16_lossy(&buf[..n])));
        1
    }
    // SAFETY: the callback only touches the Vec behind the pointer given here.
    unsafe { EnumWindows(Some(each), &mut found as *mut _ as LPARAM) };
    found
}

/// Asks the game of `exe` to close the way its window's close button does (`WM_CLOSE`: the game then ends by itself).
/// The window is the one of the started process `pid`, else one of another process whose image is exactly `exe` (the
/// game that restarted itself), never the second local player's. `true` when a close request was posted; nothing is
/// terminated here.
pub fn request_close(pid: Option<u32>, exe: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
        let mut pids = crate::installer::sys::pids_of_image(exe);
        if let Some(p) = pid
            && !pids.contains(&p)
        {
            pids.push(p);
        }
        let Some(h) = pick_window(&top_windows(), pid, &pids) else { return false };
        // SAFETY: a plain message post to a window handle just enumerated.
        unsafe { PostMessageW(h as HWND, WM_CLOSE, 0, 0) != 0 }
    }
    #[cfg(not(windows))]
    {
        let _ = (pid, exe);
        false
    }
}

/// Restores (if minimized) and focuses the game's window. `true` when a window was found.
pub fn focus_game(pid: Option<u32>, exe_name: &str) -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
        use windows_sys::Win32::UI::WindowsAndMessaging::*;

        let want = exe_name.to_ascii_lowercase();
        let mut pids = Vec::new();
        // SAFETY: the snapshot handle is checked and closed; the entry is sized as the API requires.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if !snap.is_null() && snap != windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
                let mut e: PROCESSENTRY32W = std::mem::zeroed();
                e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
                let mut ok = Process32FirstW(snap, &mut e);
                while ok != 0 {
                    let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                    if String::from_utf16_lossy(&e.szExeFile[..len]).to_ascii_lowercase() == want {
                        pids.push(e.th32ProcessID);
                    }
                    ok = Process32NextW(snap, &mut e);
                }
                windows_sys::Win32::Foundation::CloseHandle(snap);
            }
        }
        if let Some(p) = pid
            && !pids.contains(&p)
        {
            pids.push(p);
        }
        let found = top_windows();
        let Some(h) = pick_window(&found, pid, &pids) else { return false };
        // SAFETY: plain window calls on a handle just enumerated.
        unsafe {
            let hwnd = h as HWND;
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd);
        }
        true
    }
    #[cfg(not(windows))]
    {
        let _ = (pid, exe_name);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(id: isize, pid: u32, title: &str) -> (isize, u32, String) {
        (id, pid, title.to_string())
    }

    #[cfg(windows)]
    #[test]
    fn system_programs_have_full_paths() {
        for (p, name) in [(system_exe("cmd.exe"), "cmd.exe"), (system_exe("rundll32.exe"), "rundll32.exe"), (windows_exe("explorer.exe"), "explorer.exe")] {
            assert!(p.is_absolute() && p.is_file() && p.ends_with(name), "{}", p.display());
        }
        assert!(system_exe(r"WindowsPowerShell\v1.0\powershell.exe").is_file());
    }

    /// Windows looks for a bare program name in the launcher's own folder first (a planted
    /// `rundll32.exe` next to a portable launcher would run on every link). No system program is started by a bare name.
    #[test]
    fn no_system_program_is_started_by_a_bare_name() {
        fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let mut files = Vec::new();
        walk(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
        assert!(files.len() > 20);
        for f in files {
            let text = std::fs::read_to_string(&f).unwrap();
            for bare in ["rundll32", "powershell", "explorer", "cmd\"", "cmd.exe\"", "ping", "msiexec", "reg.exe", "net.exe", "schtasks", "icacls", "where.exe"] {
                let needle = format!("Command::new(\"{bare}");
                let tests_start = text.find("#[cfg(test)]").unwrap_or(text.len());
                if let Some(at) = text.find(&needle).filter(|&at| at < tests_start) {
                    panic!("{}: a system program started by its bare name at byte {at}: {needle}", f.display());
                }
            }
        }
    }

    #[test]
    fn the_window_of_the_started_process_wins() {
        let ws = [w(1, 50, "Kalamar — Player2"), w(2, 60, "Splatoon 3"), w(3, 70, "Splatoon 3")];
        assert_eq!(pick_window(&ws, Some(70), &[50, 60, 70]), Some(3));
    }

    #[test]
    fn a_restarted_game_is_found_by_its_executable_but_never_the_second_player() {
        let ws = [w(1, 50, "Kalamar — Player2"), w(2, 60, "Splatoon 3"), w(4, 99, "Notepad")];
        // the started pid (70) has no window any more: fall back to another splat3.exe process
        assert_eq!(pick_window(&ws, Some(70), &[50, 60]), Some(2));
        // only the second player's window left: nothing to focus
        assert_eq!(pick_window(&[w(1, 50, "Kalamar — Player2")], Some(70), &[50]), None);
        assert_eq!(pick_window(&ws, None, &[]), None);
    }
}
