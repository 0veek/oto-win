//! Hybrid text injection for Windows:
//! clipboard + Ctrl+V → direct Unicode typing → clipboard-only.

mod clipboard;
mod focus;
mod paste;
pub(crate) mod uia;

pub use clipboard::{get_clipboard_text, set_clipboard_text};
pub use focus::{
    active_focus_summary, capture_focus_target, capture_focus_target_async, restore_focus_target,
    FocusTarget,
};
pub use paste::{simulate_backspace, simulate_copy_to, simulate_paste_to, simulate_type_to};

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

/// How long the transcript stays on the clipboard before the previous contents
/// are put back. Long enough for any application to service the synthetic
/// Ctrl+V, short enough that the user's own clipboard is not gone for long.
const CLIPBOARD_RESTORE_DELAY: std::time::Duration = std::time::Duration::from_millis(900);

fn paste_via_clipboard(text: &str, target_pid: Option<i32>) -> OtoResult<()> {
    set_clipboard_text(text)?;
    // Give clipboard consumers a beat before Ctrl+V.
    std::thread::sleep(std::time::Duration::from_millis(120));
    simulate_paste_to(target_pid)?;
    std::thread::sleep(std::time::Duration::from_millis(40));
    Ok(())
}

/// Put `previous` back on the clipboard once the paste has landed.
///
/// Dictating should not cost the user whatever they had copied. Detached so the
/// pipeline is not held up, and it re-reads first so a clipboard the user
/// changed in the meantime is left alone.
fn restore_clipboard_later(previous: Option<String>, injected: String) {
    let Some(previous) = previous else {
        return;
    };
    if previous == injected {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(CLIPBOARD_RESTORE_DELAY);
        match get_clipboard_text() {
            // Only restore if our transcript is still what is on the clipboard.
            Ok(current) if current == injected => {
                if let Err(error) = set_clipboard_text(&previous) {
                    append_inject_log(&format!("clipboard restore failed: {error}"));
                } else {
                    append_inject_log("clipboard restored");
                }
            }
            _ => append_inject_log("clipboard changed since paste — left as is"),
        }
    });
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
    inject_text_with_options(text, mode, focus, true).await
}

/// As [`inject_text_to`], with control over whether the clipboard is restored.
pub async fn inject_text_with_options(
    text: &str,
    mode: &InjectionMode,
    focus: Option<&FocusTarget>,
    restore_clipboard: bool,
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
            target.class, target.pid
        ));
        let wait_ms = if restored { 200 } else { 80 };
        tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
        if restored {
            let _ = restore_focus_target(target);
            tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        }
    }
    append_inject_log(&format!("focus_at_type={}", active_focus_summary()));

    // Snapshot before anything writes to the clipboard. Clipboard-only mode is
    // excluded on purpose: there the clipboard *is* the delivery mechanism, so
    // putting the old value back would throw the transcript away.
    let previous_clipboard = if restore_clipboard && *mode != InjectionMode::ClipboardOnly {
        get_clipboard_text().ok()
    } else {
        None
    };

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
        Ok(kind) => {
            append_inject_log(&format!("result={kind:?}"));
            // A fallback to clipboard-only means the user still has to paste it.
            if *kind != InjectResult::ClipboardOnly {
                restore_clipboard_later(previous_clipboard, text.to_string());
            }
        }
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
    "platform=windows; input=SendInput; context=UIAutomation".into()
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
