use std::thread;
use std::time::Duration;

/// Inject text into the currently focused window via clipboard + Ctrl+V.
pub fn inject_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }

    set_clipboard(text)?;
    thread::sleep(Duration::from_millis(50));
    send_ctrl_v()?;
    thread::sleep(Duration::from_millis(100));

    Ok(())
}

/// Set clipboard text via arboard (cross-platform, Unicode-safe).
fn set_clipboard(text: &str) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| format!("failed to open clipboard: {e}"))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| format!("failed to set clipboard text: {e}"))?;
    Ok(())
}

/// Simulate Ctrl+V keystroke using Win32 SendInput.
fn send_ctrl_v() -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYEVENTF_KEYUP,
        MapVirtualKeyW, VK_CONTROL, VK_V, MAPVK_VK_TO_VSC,
    };

    unsafe {
        let mut inputs: [INPUT; 4] = std::mem::zeroed();

        // Ctrl down
        inputs[0].r#type = INPUT_KEYBOARD;
        inputs[0].Anonymous.ki.wVk = VK_CONTROL;

        // V down
        inputs[1].r#type = INPUT_KEYBOARD;
        inputs[1].Anonymous.ki.wVk = VK_V;
        inputs[1].Anonymous.ki.wScan =
            MapVirtualKeyW(VK_V.0 as u32, MAPVK_VK_TO_VSC) as u16;

        // V up
        inputs[2].r#type = INPUT_KEYBOARD;
        inputs[2].Anonymous.ki.wVk = VK_V;
        inputs[2].Anonymous.ki.wScan =
            MapVirtualKeyW(VK_V.0 as u32, MAPVK_VK_TO_VSC) as u16;
        inputs[2].Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;

        // Ctrl up
        inputs[3].r#type = INPUT_KEYBOARD;
        inputs[3].Anonymous.ki.wVk = VK_CONTROL;
        inputs[3].Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;

        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        if sent != 4 {
            return Err(format!("SendInput sent {sent}/4 inputs"));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_empty_is_noop() {
        assert!(inject_text("").is_ok());
    }
}