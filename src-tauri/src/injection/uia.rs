//! Reading the focused control's selection through UI Automation.
//!
//! This is the Windows counterpart to the Linux build's AT-SPI reader, and it
//! exists for exactly one feature: the `Selection` context tier, where the user
//! has opted in to sending nearby text to the polish model. It is used instead
//! of a simulated Ctrl+C so the clipboard is never touched — the user's
//! clipboard is theirs, and a dictation must not silently rewrite it.
//!
//! Everything here is best-effort. UI Automation is a cross-process COM call
//! into an application that may be busy, unresponsive, or simply not implement
//! `TextPattern` (most native Win32 edit controls do; many custom-drawn editors
//! do not). Every failure degrades the context to the next tier down rather
//! than disturbing the dictation.

use std::time::Duration;

use windows::core::Interface;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, UIA_TextPatternId,
};

use crate::error::OtoResult;

/// Longest a UI Automation round trip may take before it is abandoned.
///
/// A hung target application must never hold up a dictation, and this runs on
/// the polish path where the user is already waiting.
const UIA_TIMEOUT: Duration = Duration::from_millis(600);

/// Upper bound on how much text is pulled out of the target control.
///
/// `pipeline::context` clips to its own limit afterwards; this one only keeps a
/// selected-everything (Ctrl+A) case from marshalling a whole document across
/// the process boundary.
const MAX_SELECTION_CHARS: i32 = 4_096;

/// Read the current selection from the focused control.
///
/// Returns `Ok(None)` when there is no selection, when the control does not
/// support text patterns, or when UI Automation is unavailable — all of which
/// are ordinary, not errors.
pub async fn try_uia_selection() -> OtoResult<Option<String>> {
    let handle = tauri::async_runtime::spawn_blocking(read_selection_blocking);
    match tokio::time::timeout(UIA_TIMEOUT, handle).await {
        Ok(Ok(selection)) => Ok(selection),
        Ok(Err(error)) => {
            eprintln!("oto uia: selection task failed: {error}");
            Ok(None)
        }
        Err(_) => {
            // The task is detached rather than cancelled — a blocking COM call
            // cannot be interrupted — but nothing waits on it any more.
            eprintln!("oto uia: selection read timed out; continuing without it");
            Ok(None)
        }
    }
}

/// The COM work itself. Runs on a blocking thread with its own apartment.
fn read_selection_blocking() -> Option<String> {
    unsafe {
        // Multithreaded apartment: this thread does no message pumping, and
        // UI Automation marshals to the target's apartment on our behalf.
        let com = CoInitializeEx(None, COINIT_MULTITHREADED);
        let result = read_selection_inner();
        if com.is_ok() {
            CoUninitialize();
        }
        result
    }
}

unsafe fn read_selection_inner() -> Option<String> {
    let automation: IUIAutomation =
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
    let element = automation.GetFocusedElement().ok()?;

    // A control that does not implement TextPattern hands back a null interface,
    // which windows-rs surfaces as an error — so this is where a plain button or
    // a custom-drawn editor drops out, and the context quietly falls back to the
    // window title.
    let pattern = element.GetCurrentPattern(UIA_TextPatternId).ok()?;
    let text_pattern: IUIAutomationTextPattern = pattern.cast().ok()?;

    let ranges = text_pattern.GetSelection().ok()?;
    if ranges.Length().ok()? < 1 {
        return None;
    }
    let range = ranges.GetElement(0).ok()?;
    let text = range.GetText(MAX_SELECTION_CHARS).ok()?;

    let text = text.to_string();
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}
