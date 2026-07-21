//! Hybrid text injection for Windows:
//! clipboard + Ctrl+V → direct Unicode typing → clipboard-only.

mod clipboard;
mod focus;
mod paste;

pub use clipboard::{get_clipboard_text, set_clipboard_text};
pub use focus::{
    active_focus_summary, capture_focus_target, restore_focus_target, FocusTarget,
};
pub use paste::{simulate_copy_to, simulate_paste_to, simulate_type_to};

use crate::config::InjectionMode;
use crate::error::{OtoError, OtoResult};

/// How text was delivered to the target application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum InjectResult {
    /// Kept for API parity with the macOS build (UI Automation not used yet).
    Accessibility,
    DirectTyped,
    Pasted,
    ClipboardOnly,
}

#[allow(dead_code)]
pub async fn inject_text(text: &str, mode: &InjectionMode) -> OtoResult<InjectResult> {
    inject_text_to(text, mode, None).await
}

fn append_inject_log(message: &str) {
    use std::io::Write;
    let path = std::env::temp_dir().join("oto-inject.log");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{message}");
    }
    eprintln!("oto injection: {message}");
}

fn paste_via_clipboard(text: &str, target_pid: Option<i32>) -> OtoResult<()> {
    set_clipboard_text(text)?;
    // Give clipboard consumers a beat before Ctrl+V.
    std::thread::sleep(std::time::Duration::from_millis(120));
    simulate_paste_to(target_pid)?;
    std::thread::sleep(std::time::Duration::from_millis(40));
    Ok(())
}

fn automatic_injection_failed(
    text: &str,
    paste_error: &OtoError,
    type_error: &OtoError,
) -> OtoResult<InjectResult> {
    // Preserve the transcript even when every automatic delivery path fails.
    set_clipboard_text(text)?;
    Err(OtoError::Message(format!(
        "Could not insert into the focused app; the transcript was copied. Paste: {paste_error}; direct typing: {type_error}. Press Ctrl+V to paste."
    )))
}

/// Inject `text`, optionally restoring a previously captured focus target first.
pub async fn inject_text_to(
    text: &str,
    mode: &InjectionMode,
    focus: Option<&FocusTarget>,
) -> OtoResult<InjectResult> {
    let target_pid = focus.and_then(|f| f.pid);
    append_inject_log(&format!(
        "inject_text mode={mode:?} chars={} focus_before={} target_pid={:?}",
        text.chars().count(),
        active_focus_summary(),
        target_pid
    ));

    if let Some(target) = focus {
        let restored = restore_focus_target(target);
        append_inject_log(&format!(
            "restore_focus ok={restored} target={:?} pid={:?}",
            target.name, target.pid
        ));
        let wait_ms = if restored { 200 } else { 80 };
        tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
        if restored {
            let _ = restore_focus_target(target);
            tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        }
    }
    append_inject_log(&format!("focus_at_type={}", active_focus_summary()));

    let result = match mode {
        InjectionMode::ClipboardOnly => {
            set_clipboard_text(text)?;
            Ok(InjectResult::ClipboardOnly)
        }
        InjectionMode::DirectType => {
            let _ = set_clipboard_text(text);
            simulate_type_to(text, target_pid)?;
            Ok(InjectResult::DirectTyped)
        }
        InjectionMode::ClipboardPaste => {
            paste_via_clipboard(text, target_pid)?;
            Ok(InjectResult::Pasted)
        }
        InjectionMode::Auto => match paste_via_clipboard(text, target_pid) {
            Ok(()) => {
                append_inject_log("auto: clipboard+paste ok");
                Ok(InjectResult::Pasted)
            }
            Err(paste_error) => {
                append_inject_log(&format!("clipboard+paste failed: {paste_error}"));
                match simulate_type_to(text, target_pid) {
                    Ok(()) => {
                        append_inject_log("auto: direct type ok");
                        Ok(InjectResult::DirectTyped)
                    }
                    Err(type_error) => {
                        append_inject_log(&format!("direct typing failed: {type_error}"));
                        automatic_injection_failed(text, &paste_error, &type_error)
                    }
                }
            }
        },
    };
    match &result {
        Ok(kind) => append_inject_log(&format!("result={kind:?}")),
        Err(error) => append_inject_log(&format!("error={error}")),
    }
    result
}

pub async fn capture_selected_text() -> OtoResult<String> {
    let previous = get_clipboard_text().ok();
    let sentinel = format!(
        "__oto_selection_{}__",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    set_clipboard_text(&sentinel)?;
    let pid = capture_focus_target().pid;
    if let Err(error) = simulate_copy_to(pid) {
        if let Some(previous) = previous {
            let _ = set_clipboard_text(&previous);
        }
        return Err(error);
    }
    tokio::time::sleep(std::time::Duration::from_millis(160)).await;
    let selected = get_clipboard_text()?;
    if selected == sentinel || selected.trim().is_empty() {
        if let Some(previous) = previous {
            let _ = set_clipboard_text(&previous);
        }
        return Err(OtoError::Message(
            "No selected text found — select text in the target app first".into(),
        ));
    }
    if let Some(previous) = previous {
        let _ = set_clipboard_text(&previous);
    }
    Ok(selected)
}

pub fn paste_tooling_summary() -> String {
    "platform=windows; input=SendInput".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn clipboard_only_mode() {
        let result = inject_text("oto unit", &InjectionMode::ClipboardOnly).await;
        match result {
            Ok(r) => assert_eq!(r, InjectResult::ClipboardOnly),
            Err(e) => {
                let msg = e.to_string().to_lowercase();
                assert!(
                    msg.contains("clipboard") || msg.contains("not available"),
                    "unexpected error: {e}"
                );
            }
        }
    }

    #[test]
    fn paste_tooling_summary_nonempty() {
        let s = paste_tooling_summary();
        assert!(s.contains("platform=windows"));
    }
}
