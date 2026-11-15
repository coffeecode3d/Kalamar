//! Windows facilities the installer needs: child processes without a console window inside a kill-on-close job object
//! (cancelling kills the whole tree: Python, Ninja, every clang, git, dotnet), free disk space, memory, and the list of
//! running game instances.

use std::path::Path;
use std::process::{Child, Command};

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Optional (`InstallRequest::low_priority`, off by default): build tools below normal priority.
pub const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

/// Sets `CREATE_NO_WINDOW` on a command (no console window for it or its console children).
pub fn no_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// `CREATE_NO_WINDOW`, plus below-normal priority when asked (inherited by every process the command starts).
/// On Linux the command gets its own process group, so cancelling can kill the whole tree (Python, Ninja, clang, git,
/// dotnet) with one signal, and `nice` lowers its priority.
pub fn build_command(cmd: &mut Command, low_priority: bool) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW | if low_priority { BELOW_NORMAL_PRIORITY_CLASS } else { 0 });
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
        if low_priority {
            // SAFETY: `nice` only changes the child's own scheduling priority.
            unsafe {
                cmd.pre_exec(|| {
                    libc::nice(10);
                    Ok(())
                });
            }
        }
    }
    #[cfg(not(any(windows, unix)))]
    let _ = low_priority;
    cmd
}

/// A job object whose processes all die when it is terminated or when its last handle closes (the launcher exiting).
/// On Linux it is the child's process group (set by [`build_command`]), terminated with `SIGKILL`.
pub struct Job {
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
    #[cfg(unix)]
    pgid: std::sync::Mutex<Option<i32>>,
}

// SAFETY: the job handle is a kernel handle usable from any thread.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    /// `low_priority`: every process of the tree is also held below normal priority.
    #[cfg(windows)]
    pub fn new(low_priority: bool) -> std::io::Result<Job> {
        use windows_sys::Win32::System::JobObjects::*;
        // SAFETY: plain Win32 calls with owned, zero-initialised structures.
        unsafe {
            let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if h.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if low_priority {
                info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_PRIORITY_CLASS;
                info.BasicLimitInformation.PriorityClass = BELOW_NORMAL_PRIORITY_CLASS;
            }
            let ok = SetInformationJobObject(
                h,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                let e = std::io::Error::last_os_error();
                windows_sys::Win32::Foundation::CloseHandle(h);
                return Err(e);
            }
            Ok(Job { handle: h })
        }
    }
    #[cfg(not(windows))]
    pub fn new(_low_priority: bool) -> std::io::Result<Job> {
        Ok(Job {
            #[cfg(unix)]
            pgid: std::sync::Mutex::new(None),
        })
    }

    #[cfg(windows)]
    pub fn assign(&self, child: &Child) -> std::io::Result<()> {
        use std::os::windows::io::AsRawHandle;
        // SAFETY: both handles are valid for the duration of the call.
        let ok = unsafe { windows_sys::Win32::System::JobObjects::AssignProcessToJobObject(self.handle, child.as_raw_handle() as _) };
        if ok == 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
    }
    #[cfg(unix)]
    pub fn assign(&self, child: &Child) -> std::io::Result<()> {
        // build_command put the child in its own process group whose id is the child's pid.
        *self.pgid.lock().unwrap_or_else(|e| e.into_inner()) = Some(child.id() as i32);
        Ok(())
    }
    #[cfg(not(any(windows, unix)))]
    pub fn assign(&self, _child: &Child) -> std::io::Result<()> {
        Ok(())
    }

    /// Kills every process of the job.
    #[cfg(windows)]
    pub fn terminate(&self) {
        // SAFETY: valid job handle.
        unsafe { windows_sys::Win32::System::JobObjects::TerminateJobObject(self.handle, 1) };
    }
    #[cfg(unix)]
    pub fn terminate(&self) {
        if let Some(pgid) = *self.pgid.lock().unwrap_or_else(|e| e.into_inner()) {
            // SAFETY: a negative pid asks the kernel to signal the process group; ESRCH (already gone) is ignored.
            unsafe {
                libc::kill(-pgid, libc::SIGKILL);
            }
        }
    }
    #[cfg(not(any(windows, unix)))]
    pub fn terminate(&self) {}
}

impl Drop for Job {
    fn drop(&mut self) {
        #[cfg(windows)]
        // SAFETY: the handle is owned by this value.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
        #[cfg(unix)]
        self.terminate();
    }
}

/// Free bytes available to the user on the volume holding `path` (the nearest existing ancestor).
pub fn free_bytes(path: &Path) -> Option<u64> {
    let mut p = path.to_path_buf();
    while !p.exists() {
        p = p.parent()?.to_path_buf();
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
        let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
        // SAFETY: valid NUL-terminated path and out pointers.
        let ok = unsafe { windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, &mut total, &mut free) };
        if ok == 0 { None } else { Some(avail) }
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(p.as_os_str().as_bytes()).ok()?;
        // SAFETY: a valid NUL-terminated path and an owned, zero-initialised statvfs.
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        let ok = unsafe { libc::statvfs(c.as_ptr(), &mut st) };
        if ok == 0 { Some(st.f_bavail as u64 * st.f_frsize as u64) } else { None }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = p;
        None
    }
}

/// One field of `/proc/meminfo` in bytes (`MemTotal`, `MemAvailable`).
#[cfg(target_os = "linux")]
fn meminfo(field: &str) -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = text.lines().find(|l| l.starts_with(field))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

/// Total physical memory in bytes.
pub fn total_ram() -> Option<u64> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        // SAFETY: dwLength is set as the API requires.
        unsafe {
            let mut m: MEMORYSTATUSEX = std::mem::zeroed();
            m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
            if GlobalMemoryStatusEx(&mut m) == 0 { None } else { Some(m.ullTotalPhys) }
        }
    }
    #[cfg(target_os = "linux")]
    {
        meminfo("MemTotal:")
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    None
}

/// Physical memory available now (free and standby), in bytes.
pub fn available_ram() -> Option<u64> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        // SAFETY: dwLength is set as the API requires.
        unsafe {
            let mut m: MEMORYSTATUSEX = std::mem::zeroed();
            m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
            if GlobalMemoryStatusEx(&mut m) == 0 { None } else { Some(m.ullAvailPhys) }
        }
    }
    #[cfg(target_os = "linux")]
    {
        meminfo("MemAvailable:")
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    None
}

/// Every numeric `/proc` directory: one running process.
#[cfg(target_os = "linux")]
fn proc_pids() -> Vec<u32> {
    std::fs::read_dir("/proc")
        .map(|rd| rd.flatten().filter_map(|e| e.file_name().to_str().and_then(|n| n.parse().ok())).collect())
        .unwrap_or_default()
}

/// Number of running processes with this executable file name (case-insensitive), e.g. `splat3.exe`.
pub fn count_processes(exe_name: &str) -> usize {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
        let want = exe_name.to_ascii_lowercase();
        let mut n = 0;
        // SAFETY: snapshot handle checked and closed; entry sized as required.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap.is_null() || snap == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
                return 0;
            }
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut ok = Process32FirstW(snap, &mut e);
            while ok != 0 {
                let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                if String::from_utf16_lossy(&e.szExeFile[..len]).to_ascii_lowercase() == want {
                    n += 1;
                }
                ok = Process32NextW(snap, &mut e);
            }
            windows_sys::Win32::Foundation::CloseHandle(snap);
        }
        n
    }
    #[cfg(target_os = "linux")]
    {
        proc_pids()
            .into_iter()
            .filter(|pid| std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.trim().eq_ignore_ascii_case(exe_name)))
            .count()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = exe_name;
        0
    }
}

/// The CPU features the game build targets (`build_game.py --cpu v3`: x86-64-v3, AVX2 + FMA + BMI2).
pub fn cpu_has_v3() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma") && std::arch::is_x86_feature_detected!("bmi2")
    }
    #[cfg(not(target_arch = "x86_64"))]
    false
}

/// Logical processors.
pub fn cpus() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
}

/// Physical cores (`GetLogicalProcessorInformation`, one `RelationProcessorCore` entry per core); logical processors
/// when unknown.
pub fn physical_cores() -> usize {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{GetLogicalProcessorInformation, RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION};
        let item = std::mem::size_of::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION>();
        let mut len: u32 = 0;
        // SAFETY: the first call only reports the size; the second fills a buffer of that size.
        unsafe {
            GetLogicalProcessorInformation(std::ptr::null_mut(), &mut len);
            if len == 0 {
                return cpus();
            }
            let mut buf: Vec<SYSTEM_LOGICAL_PROCESSOR_INFORMATION> = vec![std::mem::zeroed(); (len as usize).div_ceil(item)];
            if GetLogicalProcessorInformation(buf.as_mut_ptr(), &mut len) == 0 {
                return cpus();
            }
            let n = (len as usize / item).min(buf.len());
            let cores = buf[..n].iter().filter(|i| i.Relationship == RelationProcessorCore).count();
            if cores == 0 { cpus() } else { cores }
        }
    }
    #[cfg(target_os = "linux")]
    {
        // One (physical id, core id) pair per core; "cpu cores" of the first processor when the ids are absent.
        let text = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        let mut cores: std::collections::BTreeSet<(String, String)> = std::collections::BTreeSet::new();
        let (mut phys, mut core) = (String::new(), String::new());
        let mut per_socket = 0usize;
        for line in text.lines() {
            match line.split_once(':').map(|(k, v)| (k.trim(), v.trim())) {
                Some(("physical id", v)) => phys = v.to_string(),
                Some(("core id", v)) => core = v.to_string(),
                Some(("cpu cores", v)) if per_socket == 0 => per_socket = v.parse().unwrap_or(0),
                Some(("processor", _)) => {
                    if !core.is_empty() {
                        cores.insert((phys.clone(), core.clone()));
                        core.clear();
                    }
                }
                _ => {}
            }
        }
        if !core.is_empty() {
            cores.insert((phys, core));
        }
        let sockets = text.lines().filter(|l| l.starts_with("physical id")).count().max(1);
        match (cores.len(), per_socket) {
            (0, n) if n > 0 => n * sockets,
            (0, _) => cpus(),
            (n, _) => n,
        }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    cpus()
}

/// Running processes whose executable is exactly `exe` (full path, case-insensitive).
pub fn count_image(exe: &Path) -> usize {
    pids_of_image(exe).len()
}

/// The ids of the running processes whose executable is exactly `exe` (full path, case-insensitive): never a program of
/// the same name elsewhere.
pub fn pids_of_image(exe: &Path) -> Vec<u32> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
        let want = exe.display().to_string().to_ascii_lowercase();
        let name = exe.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let mut found = Vec::new();
        // SAFETY: handles checked and closed; buffers sized as the APIs require.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap.is_null() || snap == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
                return found;
            }
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut ok = Process32FirstW(snap, &mut e);
            while ok != 0 {
                let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                if String::from_utf16_lossy(&e.szExeFile[..len]).to_ascii_lowercase() == name {
                    let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, e.th32ProcessID);
                    if !h.is_null() {
                        let mut buf = [0u16; 1024];
                        let mut size = buf.len() as u32;
                        if QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut size) != 0
                            && String::from_utf16_lossy(&buf[..size as usize]).to_ascii_lowercase() == want
                        {
                            found.push(e.th32ProcessID);
                        }
                        windows_sys::Win32::Foundation::CloseHandle(h);
                    }
                }
                ok = Process32NextW(snap, &mut e);
            }
            windows_sys::Win32::Foundation::CloseHandle(snap);
        }
        found
    }
    #[cfg(target_os = "linux")]
    {
        let want = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
        proc_pids()
            .into_iter()
            .filter(|pid| std::fs::read_link(format!("/proc/{pid}/exe")).is_ok_and(|p| p == want))
            .collect()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = exe;
        Vec::new()
    }
}

/// Whether the process `pid` still runs an executable whose file name is `name` (case-insensitive; a reused pid of
/// another program says no). `None` where this cannot be told.
pub fn process_alive(pid: u32, name: &str) -> Option<bool> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
        const STILL_ACTIVE: u32 = 259;
        // SAFETY: the handle is checked and closed; the buffer is sized as the API requires.
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return Some(false);
            }
            let mut code = 0u32;
            let running = GetExitCodeProcess(h, &mut code) != 0 && code == STILL_ACTIVE;
            let mut buf = [0u16; 1024];
            let mut size = buf.len() as u32;
            let same = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut size) != 0
                && Path::new(&String::from_utf16_lossy(&buf[..size as usize]))
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name));
            CloseHandle(h);
            Some(running && same)
        }
    }
    #[cfg(target_os = "linux")]
    {
        let stem = Path::new(name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        Some(std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.trim() == stem))
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (pid, name);
        None
    }
}

/// Total size in bytes of the files below `path` (0 when missing). Does not follow links.
pub fn dir_size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    let mut total = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                stack.push(e.path());
            } else {
                total += m.len();
            }
        }
    }
    total
}

/// The same for a folder whose files are still being written by another process (the merge tool's): each file's size
/// is read from the file itself, not from its folder's listing, which Windows updates only when the writer closes it
/// (with the listing, a merge writing a 10 GB image stays at 0 % until its end).
pub fn dir_size_live(path: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            match std::fs::metadata(&p) {
                Ok(m) if m.is_dir() => stack.push(p),
                Ok(m) => total += m.len(),
                Err(_) => {}
            }
        }
    }
    total
}

/// Removes a file or folder tree; missing is fine. Read-only files (git objects) are made writable first.
pub fn remove_all(path: &Path) -> std::io::Result<()> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if !meta.is_dir() {
        let mut perm = meta.permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perm.set_readonly(false);
        let _ = std::fs::set_permissions(path, perm);
        return std::fs::remove_file(path);
    }
    if std::fs::remove_dir_all(path).is_ok() {
        return Ok(());
    }
    // Second pass: clear read-only attributes (git packs) and retry.
    let mut stack = vec![path.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if let Ok(m) = std::fs::symlink_metadata(&p) {
                if m.is_dir() {
                    stack.push(p);
                } else {
                    let mut perm = m.permissions();
                    #[allow(clippy::permissions_set_readonly_false)]
                    perm.set_readonly(false);
                    let _ = std::fs::set_permissions(&p, perm);
                }
            }
        }
    }
    std::fs::remove_dir_all(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_still_being_written_is_measured_by_what_it_holds() {
        use std::io::Write;
        let d = std::env::temp_dir().join(format!("s3l-live-{}", crate::util::random_hex(4)));
        std::fs::create_dir_all(d.join("result")).unwrap();
        // the merge's image, open for writing (not closed) while the progress is read
        let mut f = std::fs::File::create(d.join("result").join("romfs.bin")).unwrap();
        f.write_all(&vec![7u8; 3 << 20]).unwrap();
        f.flush().unwrap();
        std::fs::write(d.join("title.keys"), b"k").unwrap();
        assert_eq!(dir_size_live(&d), (3 << 20) + 1);
        drop(f);
        assert_eq!(dir_size_live(&d), dir_size(&d));
        assert_eq!(dir_size_live(&d.join("missing")), 0);
        remove_all(&d).unwrap();
    }

    #[test]
    fn sizes_and_removal() {
        let d = std::env::temp_dir().join(format!("s3l-sys-{}", crate::util::random_hex(4)));
        std::fs::create_dir_all(d.join("a").join("b")).unwrap();
        std::fs::write(d.join("a").join("x"), b"12345").unwrap();
        std::fs::write(d.join("a").join("b").join("y"), b"123").unwrap();
        let ro = d.join("a").join("ro");
        std::fs::write(&ro, b"1").unwrap();
        let mut p = std::fs::metadata(&ro).unwrap().permissions();
        p.set_readonly(true);
        std::fs::set_permissions(&ro, p).unwrap();
        assert_eq!(dir_size(&d), 9);
        remove_all(&d).unwrap();
        assert!(!d.exists());
        remove_all(&d).unwrap();
    }

    #[test]
    fn machine_facts_are_readable() {
        assert!(total_ram().unwrap_or(1) > 0);
        assert!(free_bytes(&std::env::temp_dir().join("does-not-exist-yet")).is_some() || !cfg!(windows));
        assert!(cpus() >= 1);
        assert!(physical_cores() >= 1 && physical_cores() <= cpus());
        let _ = count_processes("splat3.exe");
        // this test's own executable is running, under its full path
        assert_eq!(count_image(&std::env::current_exe().unwrap()), 1);
        assert_eq!(count_image(Path::new(r"Z:\nowhere\none.exe")), 0);
    }

    #[cfg(windows)]
    #[test]
    fn build_commands_run_at_normal_priority_unless_asked() {
        use std::os::windows::io::AsRawHandle;
        let sys = std::env::var_os("SystemRoot").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
        // Without a priority flag a child gets the normal class, except that Windows hands down the parent's class
        // when that is idle (0x40) or below normal (0x4000): hosted CI runners run their jobs below normal.
        // SAFETY: the pseudo-handle of the current process is always valid.
        let own = unsafe {
            windows_sys::Win32::System::Threading::GetPriorityClass(windows_sys::Win32::System::Threading::GetCurrentProcess())
        };
        let inherited = if own == 0x40 || own == BELOW_NORMAL_PRIORITY_CLASS { own } else { 0x20u32 };
        for (low, want) in [(false, inherited), (true, BELOW_NORMAL_PRIORITY_CLASS)] {
            let job = Job::new(low).unwrap();
            let mut cmd = Command::new(sys.join("System32").join("ping.exe"));
            cmd.args(["-n", "3", "127.0.0.1"]).stdout(std::process::Stdio::null());
            build_command(&mut cmd, low);
            let mut child = cmd.spawn().unwrap();
            job.assign(&child).unwrap();
            // SAFETY: valid process handle owned by `child`.
            let class = unsafe { windows_sys::Win32::System::Threading::GetPriorityClass(child.as_raw_handle() as _) };
            job.terminate();
            let _ = child.wait();
            assert_eq!(class, want, "low_priority={low}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn job_kills_the_tree() {
        let job = Job::new(false).unwrap();
        let sys = std::env::var_os("SystemRoot").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
        // cmd starts a child ping; terminating the job must end both.
        let mut cmd = Command::new(sys.join("System32").join("cmd.exe"));
        cmd.args(["/C", "ping -n 30 127.0.0.1 >NUL"]);
        no_window(&mut cmd);
        let mut child = cmd.spawn().unwrap();
        job.assign(&child).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        job.terminate();
        let st = child.wait().unwrap();
        assert!(!st.success());
    }
}
