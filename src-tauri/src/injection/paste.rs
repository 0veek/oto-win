//! Keyboard simulation for paste/copy/type/undo on Windows via SendInput.

use std::{thread, time::Duration};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_C, VK_LCONTROL,
    VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
    VK_V,
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

/// Modifiers a push-to-talk chord can leave physically down, paired with whether
/// the key is an *extended* one. The right-hand Ctrl/Alt and both Windows keys
/// share a virtual-key code with their left-hand twin and are distinguished only
/// by that flag, so a key-up without it does not clear them.
const MODIFIER_KEYS: &[(VIRTUAL_KEY, bool)] = &[
    (VK_CONTROL, false),
    (VK_LCONTROL, false),
    (VK_RCONTROL, true),
    (VK_SHIFT, false),
    (VK_LSHIFT, false),
    (VK_RSHIFT, false),
    (VK_MENU, false),
    (VK_LMENU, false),
    (VK_RMENU, true),
    (VK_LWIN, true),
    (VK_RWIN, true),
];

/// Tell the system every modifier is up before synthesizing a chord.
///
/// The global-shortcut release fires on the first key of the chord to come up,
/// so Ctrl and Shift are often still physically held — which would turn the
/// Ctrl+V below into Ctrl+Shift+V, a different command in most editors and
/// browsers. A key-up for a key the user is still holding is harmless: their
/// eventual physical release is just a second key-up.
///
/// The Windows keys are included because a chord containing Win would otherwise
/// make the paste Win+Ctrl+V. That cannot leave the Start menu open: Windows
/// only opens it for a Win press and release with no other key in between, and
/// here the chord's own key was pressed during the hold.
fn release_modifiers() {
    let inputs: Vec<INPUT> = MODIFIER_KEYS
        .iter()
        .map(|&(vk, extended)| {
            let mut flags = KEYEVENTF_KEYUP;
            if extended {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            key_input(vk, flags)
        })
        .collect();
    let _ = send_inputs(&inputs);
}

/// Simulate Ctrl+V (paste). `target_pid` is accepted for API parity but Windows
/// posts synthetic input to the focused window, not a specific PID.
pub fn simulate_paste_to(_target_pid: Option<i32>) -> OtoResult<()> {
    release_modifiers();
    thread::sleep(Duration::from_millis(30));
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
    release_modifiers();
    thread::sleep(Duration::from_millis(30));
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
    release_modifiers();
    thread::sleep(Duration::from_millis(30));
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

/// Number of BackSpace presses sent in one `SendInput` call.
///
/// Applications process synthetic input from their own message loop; a single
/// call with two thousand events can outrun a slow editor and lose keystrokes.
const BACKSPACE_BATCH: usize = 32;

/// Delete the previous `count` characters by sending BackSpace.
///
/// Genuinely destructive: it removes whatever is left of the caret, which is
/// only the dictated text if nothing has moved since. The caller is responsible
/// for the freshness and same-window checks that make that assumption safe.
///
/// `count` is a count of *characters*, and a character outside the basic
/// multilingual plane occupies two UTF-16 units — but editors delete by
/// grapheme, not by code unit, so one BackSpace per character is correct.
pub fn simulate_backspace(count: usize) -> OtoResult<()> {
    if count == 0 {
        return Ok(());
    }
    release_modifiers();
    thread::sleep(Duration::from_millis(40));

    let mut remaining = count;
    while remaining > 0 {
        let batch = remaining.min(BACKSPACE_BATCH);
        let mut inputs = Vec::with_capacity(batch * 2);
        for _ in 0..batch {
            inputs.push(key_input(VK_BACK, KEYBD_EVENT_FLAGS(0)));
            inputs.push(key_input(VK_BACK, KEYEVENTF_KEYUP));
        }
        send_inputs(&inputs)?;
        remaining -= batch;
        thread::sleep(Duration::from_millis(8));
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
