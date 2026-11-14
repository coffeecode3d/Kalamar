//! The launcher's icon in the notification area and the Windows notifications it carries (`Shell_NotifyIconW`, the
//! system's own API: no plugin, no registration, nothing written anywhere).
//!
//! Two uses:
//! * a game update chosen for "when the launcher closes": the window goes, the icon stays for the time of the update
//!   (its tooltip says how far it is), a notification says when it ends; clicking the icon or the notification brings
//!   the window back, its menu offers "Open" and "Quit" (Quit asks first, like closing the window during an install);
//! * the end of an installation while the window is in the background: one notification, from an icon that goes away
//!   as soon as the window is used again.
//!
//! The icon lives on its own thread with a hidden window that receives its clicks. Windows shows a notification of an
//! icon as a toast (Focus assist respected) with the program's name; it stays in the notification centre while the icon
//! exists.

use serde::Deserialize;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::AppHandle;

/// The words of the icon, in the interface's language (given by the page).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct TrayTexts {
    /// Tooltip; `{pct}` is replaced by the progress.
    pub tip: String,
    pub open: String,
    pub quit: String,
}

#[derive(Default)]
struct State {
    texts: TrayTexts,
    /// The icon stays until released (an update at close runs), instead of only carrying a notification.
    kept: bool,
    /// The icon should be in the notification area.
    shown: bool,
    tip: String,
    /// A notification to show (title, text).
    balloon: Option<(String, String)>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
/// The launcher, for what a click does (apart from the icon's state: the icon's own code does not depend on it).
static APP: Mutex<Option<AppHandle>> = Mutex::new(None);
/// What a click on the icon, its notification or its menu does: set with the launcher by [`keep`] and [`notify`].
static ON_EVENT: std::sync::OnceLock<fn(Event)> = std::sync::OnceLock::new();

#[derive(Clone, Copy)]
#[cfg_attr(not(windows), allow(dead_code))]
enum Event {
    Open,
    Quit,
}

#[cfg_attr(not(windows), allow(dead_code))]
fn dispatch(e: Event) {
    if let Some(f) = ON_EVENT.get() {
        f(e);
    }
}

fn handle(e: Event) {
    match e {
        Event::Open => open(),
        Event::Quit => quit(),
    }
}

fn set_app(app: &AppHandle) {
    *APP.lock().unwrap_or_else(|p| p.into_inner()) = Some(app.clone());
    let _ = ON_EVENT.set(handle);
}

fn app() -> Option<AppHandle> {
    APP.lock().unwrap_or_else(|p| p.into_inner()).clone()
}
/// The window was brought back from the icon or its notification.
static REOPENED: AtomicBool = AtomicBool::new(false);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    f(g.get_or_insert_with(State::default))
}

/// Keeps the icon in the notification area (an update at close runs), with these words.
pub fn keep(app: &AppHandle, texts: TrayTexts, progress: f64) {
    REOPENED.store(false, Ordering::SeqCst);
    set_app(app);
    with(|s| {
        s.tip = tip_text(&texts.tip, progress);
        s.texts = texts;
        s.kept = true;
        s.shown = true;
    });
    imp::sync();
}

/// The tooltip of a kept icon, with the progress (0 to 1).
pub fn progress(value: f64) {
    let changed = with(|s| {
        let t = tip_text(&s.texts.tip, value);
        let changed = s.kept && s.tip != t;
        s.tip = t;
        changed
    });
    if changed {
        imp::sync();
    }
}

fn tip_text(template: &str, value: f64) -> String {
    let pct = (value.clamp(0.0, 1.0) * 100.0).floor() as u32;
    let t = if template.is_empty() { "Kalamar".to_string() } else { template.replace("{pct}", &pct.to_string()) };
    t.chars().take(120).collect()
}

/// A Windows notification. Without a kept icon, a transient one carries it and goes when the window is used again.
/// Returns false where the system has no notification area (not Windows).
pub fn notify(app: &AppHandle, title: &str, body: &str) -> bool {
    if !cfg!(windows) {
        return false;
    }
    set_app(app);
    with(|s| {
        if !s.kept {
            s.tip = "Kalamar".into();
        }
        s.shown = true;
        s.balloon = Some((title.chars().take(60).collect(), body.chars().take(250).collect()));
    });
    imp::sync();
    true
}

/// The icon goes (the update at close is over and the window is back).
pub fn release() {
    with(|s| {
        s.kept = false;
        s.shown = false;
        s.balloon = None;
    });
    imp::sync();
}

/// The icon goes before the launcher quits, waiting for it to be gone (no ghost icon left in the notification area).
pub fn shutdown() {
    let had = with(|s| {
        let had = s.shown;
        s.kept = false;
        s.shown = false;
        s.balloon = None;
        had
    });
    if had {
        imp::sync_now();
    }
}

/// An icon is kept (an update at close runs).
pub fn kept() -> bool {
    with(|s| s.kept)
}

/// The window is hidden again during an update at close: it is the icon's again, as when it was first kept.
pub fn rekeep() {
    REOPENED.store(false, Ordering::SeqCst);
}

/// The window is in use again: a transient icon (one that only carried a notification) goes.
pub fn window_used() {
    let drop = with(|s| {
        let d = s.shown && !s.kept;
        if d {
            s.shown = false;
            s.balloon = None;
        }
        d
    });
    if drop {
        imp::sync();
    }
}

/// The window was brought back from the icon since the icon was kept.
pub fn reopened() -> bool {
    REOPENED.load(Ordering::SeqCst)
}

/// A click on the icon or its notification: the window comes back (a transient icon then goes).
fn open() {
    REOPENED.store(true, Ordering::SeqCst);
    if let Some(app) = app() {
        crate::shell::bring_back(&app);
    }
    window_used();
}

/// "Quit" in the icon's menu: during an installation the window comes back and asks first (stay, keep installing in
/// the background, or stop and quit); otherwise the launcher quits.
fn quit() {
    let Some(app) = app() else { return };
    use tauri::{Emitter, Manager};
    let installing = app.try_state::<crate::commands::AppState>().is_some_and(|s| s.installer.active());
    if installing {
        REOPENED.store(true, Ordering::SeqCst);
        crate::shell::bring_back(&app);
        let _ = app.emit("installer-close-requested", ());
    } else {
        shutdown();
        app.exit(0);
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn menu_words() -> (String, String) {
    with(|s| (s.texts.open.clone(), s.texts.quit.clone()))
}

#[cfg(windows)]
mod imp {
    use super::{STATE, State};
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Shell::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const WM_ICON: u32 = WM_APP + 1;
    const WM_SYNC: u32 = WM_APP + 2;
    const ICON_ID: u32 = 1;
    /// NIN_SELECT | NINF_KEY: the icon chosen with the keyboard (not in windows-sys).
    const NIN_KEY_SELECT: u32 = NIN_SELECT | 1;
    const ID_OPEN: usize = 1;
    const ID_QUIT: usize = 2;

    /// The hidden window of the icon's thread (as an integer: a window handle is not `Send`).
    static WINDOW: OnceLock<isize> = OnceLock::new();
    /// The icon is in the notification area now.
    static ADDED: AtomicBool = AtomicBool::new(false);

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn copy_wide(dst: &mut [u16], s: &str) {
        let w: Vec<u16> = s.encode_utf16().take(dst.len().saturating_sub(1)).collect();
        dst[..w.len()].copy_from_slice(&w);
        dst[w.len()] = 0;
    }

    /// The program's own icon (the resource the build embeds as 32512), at a size of the system's.
    fn app_icon(large: bool) -> HICON {
        // SAFETY: plain Win32 calls; a null module handle is the executable itself.
        unsafe {
            let inst = GetModuleHandleW(std::ptr::null());
            let (cx, cy) = if large { (GetSystemMetrics(SM_CXICON), GetSystemMetrics(SM_CYICON)) } else { (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON)) };
            let h = LoadImageW(inst, 32512_usize as *const u16, IMAGE_ICON, cx, cy, LR_DEFAULTCOLOR);
            if !h.is_null() { h as HICON } else { LoadIconW(std::ptr::null_mut(), IDI_APPLICATION) }
        }
    }

    fn data(hwnd: HWND) -> NOTIFYICONDATAW {
        // SAFETY: a zeroed NOTIFYICONDATAW is its documented empty value.
        let mut d: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        d.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        d.hWnd = hwnd;
        d.uID = ICON_ID;
        d
    }

    /// Asks the icon's thread to apply the state (started the first time).
    pub fn sync() {
        let Some(hwnd) = window() else { return };
        // SAFETY: posting a message to our own window.
        unsafe {
            PostMessageW(hwnd as HWND, WM_SYNC, 0, 0);
        }
    }

    /// The icon is in the notification area (tests).
    #[cfg(test)]
    pub fn added() -> bool {
        ADDED.load(Ordering::SeqCst)
    }

    /// The same, waiting until it is done (at most two seconds).
    pub fn sync_now() {
        let Some(&hwnd) = WINDOW.get() else { return };
        let mut out = 0usize;
        // SAFETY: a message to our own window; its thread answers it.
        unsafe {
            SendMessageTimeoutW(hwnd as HWND, WM_SYNC, 0, 0, SMTO_ABORTIFHUNG, 2000, &mut out);
        }
    }

    fn window() -> Option<isize> {
        if let Some(h) = WINDOW.get() {
            return Some(*h);
        }
        static STARTING: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _g = STARTING.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(h) = WINDOW.get() {
            return Some(*h);
        }
        let (tx, rx) = std::sync::mpsc::channel::<isize>();
        std::thread::Builder::new()
            .name("tray".into())
            .spawn(move || {
                // SAFETY: the window class and window are created and used on this thread only; the strings outlive the
                // calls that read them.
                unsafe {
                    let inst = GetModuleHandleW(std::ptr::null());
                    let class = wide("KalamarTrayWindow");
                    let mut wc: WNDCLASSEXW = std::mem::zeroed();
                    wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
                    wc.lpfnWndProc = Some(proc_);
                    wc.hInstance = inst;
                    wc.lpszClassName = class.as_ptr();
                    RegisterClassExW(&wc);
                    let title = wide("Kalamar");
                    let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW, class.as_ptr(), title.as_ptr(), WS_POPUP, 0, 0, 0, 0, std::ptr::null_mut(), std::ptr::null_mut(), inst, std::ptr::null());
                    let _ = tx.send(hwnd as isize);
                    if hwnd.is_null() {
                        return;
                    }
                    let mut msg: MSG = std::mem::zeroed();
                    while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            })
            .ok()?;
        let h = rx.recv_timeout(std::time::Duration::from_secs(5)).ok().filter(|h| *h != 0)?;
        let _ = WINDOW.set(h);
        Some(h)
    }

    /// Applies the wanted state to the icon (on the icon's thread).
    fn apply(hwnd: HWND) {
        let (shown, tip, balloon) = {
            let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
            let s: &mut State = g.get_or_insert_with(State::default);
            (s.shown, s.tip.clone(), s.balloon.take())
        };
        // SAFETY: the structure is filled as the API documents; the icons are shared system or module icons.
        unsafe {
            if !shown {
                if ADDED.swap(false, Ordering::SeqCst) {
                    let d = data(hwnd);
                    Shell_NotifyIconW(NIM_DELETE, &d);
                }
                return;
            }
            let mut d = data(hwnd);
            d.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
            d.uCallbackMessage = WM_ICON;
            d.hIcon = app_icon(false);
            copy_wide(&mut d.szTip, &tip);
            if !ADDED.load(Ordering::SeqCst) {
                if Shell_NotifyIconW(NIM_ADD, &d) != 0 {
                    ADDED.store(true, Ordering::SeqCst);
                    d.Anonymous.uVersion = NOTIFYICON_VERSION_4;
                    Shell_NotifyIconW(NIM_SETVERSION, &d);
                }
            } else {
                Shell_NotifyIconW(NIM_MODIFY, &d);
            }
            if let Some((title, body)) = balloon {
                let mut b = data(hwnd);
                b.uFlags = NIF_INFO;
                copy_wide(&mut b.szInfoTitle, &title);
                copy_wide(&mut b.szInfo, &body);
                b.dwInfoFlags = NIIF_USER | NIIF_LARGE_ICON | NIIF_RESPECT_QUIET_TIME;
                b.hBalloonIcon = app_icon(true);
                Shell_NotifyIconW(NIM_MODIFY, &b);
            }
        }
    }

    fn menu(hwnd: HWND) {
        let (open, quit) = super::menu_words();
        let open = wide(if open.is_empty() { "Kalamar" } else { &open });
        let quit = wide(if quit.is_empty() { "Quit" } else { &quit });
        // SAFETY: the menu is created, shown and destroyed here; the strings outlive the calls.
        let cmd = unsafe {
            let m = CreatePopupMenu();
            if m.is_null() {
                return;
            }
            AppendMenuW(m, MF_STRING, ID_OPEN, open.as_ptr());
            AppendMenuW(m, MF_STRING, ID_QUIT, quit.as_ptr());
            SetMenuDefaultItem(m, ID_OPEN as u32, 0);
            let mut p = POINT { x: 0, y: 0 };
            GetCursorPos(&mut p);
            // The documented dance: the owner in front while the menu is open, so a click elsewhere closes it.
            SetForegroundWindow(hwnd);
            let cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, p.x, p.y, 0, hwnd, std::ptr::null());
            PostMessageW(hwnd, WM_NULL, 0, 0);
            DestroyMenu(m);
            cmd as usize
        };
        match cmd {
            ID_OPEN => super::dispatch(super::Event::Open),
            ID_QUIT => super::dispatch(super::Event::Quit),
            _ => {}
        }
    }

    unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_SYNC => {
                apply(hwnd);
                0
            }
            WM_ICON => {
                // NOTIFYICON_VERSION_4: the event is the low word of lParam.
                match (lp as u32) & 0xFFFF {
                    NIN_SELECT | NIN_KEY_SELECT | NIN_BALLOONUSERCLICK => super::dispatch(super::Event::Open),
                    WM_CONTEXTMENU => menu(hwnd),
                    _ => {}
                }
                0
            }
            // SAFETY: the default handling of every other message.
            _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn sync() {}
    pub fn sync_now() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real notification area: the icon is added by its own thread and window, then removed before the end.
    #[cfg(windows)]
    #[test]
    #[ignore = "shows the launcher's icon in the notification area for a moment"]
    fn the_icon_is_added_then_removed_without_a_ghost() {
        with(|s| {
            s.shown = true;
            s.tip = tip_text("Kalamar (test) {pct} %", 0.5);
        });
        imp::sync();
        imp::sync_now();
        assert!(imp::added(), "Shell_NotifyIconW(NIM_ADD) failed");
        progress(0.7);
        shutdown();
        assert!(!imp::added(), "the icon is still there");
    }

    #[test]
    fn the_tooltip_says_the_progress_and_stays_short() {
        assert_eq!(tip_text("Kalamar — updating the game: {pct} %", 0.426), "Kalamar — updating the game: 42 %");
        assert_eq!(tip_text("", 0.5), "Kalamar");
        assert_eq!(tip_text("{pct}", 1.7), "100");
        assert!(tip_text(&"x".repeat(400), 0.0).chars().count() <= 120);
    }
}
