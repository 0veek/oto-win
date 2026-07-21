//! Keyboard simulation for paste/copy/type on Windows via SendInput.

use std::{thread, time::Duration};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_CONTROL, VK_C, VK_V,
};

use crate::error::{OtoError, OtoResult};

fn send_inputs(inputs: &[INPUT]) -> OtoResult<()> {
    if inputs.is_empty() {
        return Ok(());
    }
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(OtoError::Message(format!(
            "SendInput sent {sent}/{} events",
            inputs.len()
        )));
    }
    Ok(())
}

fn key_input(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn unicode_input(ch: u16, key_up: bool) -> INPUT {
    let mut flags = KEYEVENTF_UNICODE;
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: ch,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Simulate Ctrl+V (paste). `target_pid` is accepted for API parity but Windows
/// posts synthetic input to the focused window, not a specific PID.
pub fn simulate_paste_to(_target_pid: Option<i32>) -> OtoResult<()> {
    let inputs = [
        key_input(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
        key_input(VK_V, KEYBD_EVENT_FLAGS(0)),
        key_input(VK_V, KEYEVENTF_KEYUP),
        key_input(VK_CONTROL, KEYEVENTF_KEYUP),
    ];
    send_inputs(&inputs)?;
    thread::sleep(Duration::from_millis(30));
    Ok(())
}

/// Simulate Ctrl+C (copy) for selected-text capture fallback.
pub fn simulate_copy_to(_target_pid: Option<i32>) -> OtoResult<()> {
    let inputs = [
        key_input(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
        key_input(VK_C, KEYBD_EVENT_FLAGS(0)),
        key_input(VK_C, KEYEVENTF_KEYUP),
        key_input(VK_CONTROL, KEYEVENTF_KEYUP),
    ];
    send_inputs(&inputs)?;
    thread::sleep(Duration::from_millis(30));
    Ok(())
}

/// Type `text` with Unicode keyboard events (works for most character sets).
pub fn simulate_type_to(text: &str, _target_pid: Option<i32>) -> OtoResult<()> {
    // Batch characters to avoid huge single SendInput calls on long transcripts.
    const BATCH: usize = 64;
    let units: Vec<u16> = text.encode_utf16().collect();
    for chunk in units.chunks(BATCH) {
        let mut inputs = Vec::with_capacity(chunk.len() * 2);
        for &unit in chunk {
            // Surrogate pairs are emitted as sequential UTF-16 units with KEYEVENTF_UNICODE.
            inputs.push(unicode_input(unit, false));
            inputs.push(unicode_input(unit, true));
        }
        send_inputs(&inputs)?;
        thread::sleep(Duration::from_millis(4));
    }
    Ok(())
}

#[allow(dead_code)]
pub fn simulate_paste() -> OtoResult<()> {
    simulate_paste_to(None)
}

#[allow(dead_code)]
pub fn simulate_copy() -> OtoResult<()> {
    simulate_copy_to(None)
}

#[allow(dead_code)]
pub fn simulate_type(text: &str) -> OtoResult<()> {
    simulate_type_to(text, None)
}
