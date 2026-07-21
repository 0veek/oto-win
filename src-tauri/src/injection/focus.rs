//! Save/restore the foreground window so injection hits the dictation target.

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow,
    SetForegroundWindow, ShowWindow, SW_RESTORE,
};

/// Snapshot of the application that should receive injected text.
#[derive(Debug, Clone, Default)]
pub struct FocusTarget {
    pub pid: Option<i32>,
    pub name: Option<String>,
    pub class: Option<String>,
    /// HWND stored as isize so FocusTarget stays Send + Sync.
    pub hwnd: Option<isize>,
}

fn hwnd_from_isize(value: isize) -> HWND {
    HWND(value as *mut _)
}

fn window_title(hwnd: HWND) -> Option<String> {
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if len <= 0 {
        return None;
    }
    let title = String::from_utf16_lossy(&buf[..len as usize]);
    if title.trim().is_empty() {
        None
    } else {
        Some(title)
    }
}

fn process_name(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);
        if ok.is_err() || size == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
    }
}

fn is_oto_window(pid: u32, title: Option<&str>, proc_name: Option<&str>) -> bool {
    let title_l = title.unwrap_or("").to_ascii_lowercase();
    let proc_l = proc_name.unwrap_or("").to_ascii_lowercase();
    if title_l.contains("oto") || proc_l == "oto" || proc_l.starts_with("oto") {
        return true;
    }
    // Own process — never treat Oto as the injection target.
    pid == std::process::id()
}

fn target_from_hwnd(hwnd: HWND) -> FocusTarget {
    if hwnd.0.is_null() || !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return FocusTarget::default();
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let name = process_name(pid).or_else(|| window_title(hwnd));
    let title = window_title(hwnd);
    FocusTarget {
        pid: if pid == 0 { None } else { Some(pid as i32) },
        name: name.clone(),
        class: title,
        hwnd: Some(hwnd.0 as isize),
    }
}

/// Capture the foreground non-Oto window.
pub fn capture_focus_target() -> FocusTarget {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return FocusTarget::default();
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let title = window_title(hwnd);
    let proc = process_name(pid);
    if is_oto_window(pid, title.as_deref(), proc.as_deref()) {
        // Settings/overlay may be focused after tray clicks — leave empty so
        // injection targets whatever the user focuses next, or falls back to
        // current foreground at inject time.
        return FocusTarget::default();
    }
    target_from_hwnd(hwnd)
}

/// Restore focus to a previously captured target. Returns true if activation ran.
pub fn restore_focus_target(target: &FocusTarget) -> bool {
    let Some(raw) = target.hwnd else {
        return false;
    };
    let hwnd = hwnd_from_isize(raw);
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return false;
    }

    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }

        // Attach input queues so SetForegroundWindow is allowed more often.
        let target_thread = GetWindowThreadProcessId(hwnd, None);
        let current_thread = GetCurrentThreadId();
        let attached = if target_thread != 0 && target_thread != current_thread {
            AttachThreadInput(current_thread, target_thread, true).as_bool()
        } else {
            false
        };

        let ok = SetForegroundWindow(hwnd).as_bool();

        if attached {
            let _ = AttachThreadInput(current_thread, target_thread, false);
        }
        ok
    }
}

/// Log-friendly summary of the currently foreground window.
pub fn active_focus_summary() -> String {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return "unknown".into();
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let title = window_title(hwnd).unwrap_or_else(|| "?".into());
    let proc = process_name(pid).unwrap_or_else(|| "?".into());
    format!("{title} | {proc} | pid={pid} hwnd={:?}", hwnd.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_focus_summary_nonempty() {
        let s = active_focus_summary();
        assert!(!s.is_empty());
    }
}
