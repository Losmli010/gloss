//! 按键注入：模拟复制快捷键的合成键序。
//!
//! 注入经系统输入队列与真实按键同源；对提权进程的注入被系统静默丢弃
//! （UIPI），本层不感知、不区分——写入是否发生由调用方以剪贴板序列号
//! 判定（见剪贴板兜底模块）。键序一次投递，部分失败时按已注入的按下键
//! 逆序补放释放，不留卡住的修饰键。

use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_CONTROL,
};

const VK_C: u16 = 0x43;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InjectionFailure {
    pub(crate) injected: usize,
}

pub(crate) fn copy_sequence() -> [(u16, bool); 4] {
    [
        (VK_CONTROL.0, false),
        (VK_C, false),
        (VK_C, true),
        (VK_CONTROL.0, true),
    ]
}

pub(crate) fn releases_before(sequence: &[(u16, bool)], index: usize) -> Vec<u16> {
    sequence[..index]
        .iter()
        .rev()
        .filter(|(_, key_down)| !*key_down)
        .map(|(key, _)| *key)
        .collect()
}

pub(crate) fn inject_copy() -> Result<(), InjectionFailure> {
    let sequence = copy_sequence();
    let inputs: Vec<INPUT> = sequence
        .iter()
        .map(|&(key, up)| keyboard_input(key, up))
        .collect();
    // SAFETY: `inputs` is a valid array of keyboard events with the element
    // size reported truthfully; the call synthesizes them into the system
    // input queue and returns how many were accepted.
    let injected = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) } as usize;
    if injected == sequence.len() {
        return Ok(());
    }
    release_each(releases_before(&sequence, injected));
    Err(InjectionFailure { injected })
}

fn release_each(keys: Vec<u16>) {
    if keys.is_empty() {
        return;
    }
    let inputs: Vec<INPUT> = keys.iter().map(|&key| keyboard_input(key, true)).collect();
    // SAFETY: same contract as `inject_copy`; a failed release is left to the
    // next injection round, and key-up events are idempotent.
    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
}

fn keyboard_input(key: u16, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(key),
                wScan: 0,
                dwFlags: if key_up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_sequence_presses_modifier_first_and_releases_it_last() {
        let sequence = copy_sequence();
        assert_eq!(sequence[0], (VK_CONTROL.0, false));
        assert_eq!(sequence[1], (VK_C, false));
        assert_eq!(sequence[2], (VK_C, true));
        assert_eq!(sequence[3], (VK_CONTROL.0, true));
    }

    #[test]
    fn injection_failure_releases_pressed_keys_in_reverse() {
        let sequence = copy_sequence();
        assert_eq!(
            releases_before(&sequence, 0),
            Vec::<u16>::new(),
            "第一个事件失败时还没有键按下"
        );
        assert_eq!(
            releases_before(&sequence, 1),
            vec![VK_CONTROL.0],
            "键按下失败：只需补放修饰键"
        );
        assert_eq!(
            releases_before(&sequence, 2),
            vec![VK_C, VK_CONTROL.0],
            "键释放失败：先补键再补修饰键（只放修饰键会留下键按下态）"
        );
        assert_eq!(
            releases_before(&sequence, 3),
            vec![VK_C, VK_CONTROL.0],
            "修饰键释放失败：键已释放，重复 key-up 幂等"
        );
    }
}
