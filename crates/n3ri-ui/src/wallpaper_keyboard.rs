//! 壁纸模式键盘注入桥接。
//!
//! 壁纸模式没有 winit 主窗，bevy 原生 `KeyboardInput` 消息通道（winit 键盘事件）不工作。
//! 本模块把 vendor 层 `wl_keyboard.key` 事件（[`WallpaperKeyboardState`]，PostUpdate 灌入）
//! 按 XKB keycode 静态映射为 bevy `KeyboardInput` 消息注入 `MessageWriter<KeyboardInput>`，
//! 既有 terminal/chat/settings 等 `KeyboardInput` 消费方无需改动即可工作。
//!
//! 实现要点：
//! - **XKB keycode → bevy `KeyCode`**：Linux evdev 扫描码（vendor 捕获的原始值），
//!   静态 US 布局映射表（无 keymap/keysym 解析，中文经 IME 路径不经过本模块）。
//! - **`logical_key`**：与 `bevy_winit` 的 `convert_logical_key` 语义对齐——可打印键产
//!   `Key::Character`（shift 敏感：按住 Shift 出大写/上档符号，与 winit 一致）；
//!   功能键产 `Key::Enter/Space/Tab/Backspace/Escape/Arrow*/Home/End/PageUp/PageDown/
//!   Delete/Insert`；修饰键产 `Key::Control/Shift/Alt/Super`。
//! - **修饰键跨帧保持**：`ButtonInput::clear()` 只清 `just_pressed/just_released`，
//!   `pressed` 集合由 Pressed/Released 消息增减并跨帧持久，因此无需逐帧重注入。
//! - **调度**：PreUpdate 中 `.before(InputSystems)`，保证 `keyboard_input_system`
//!   同帧读取到注入消息并重建 `ButtonInput<KeyCode>`/`ButtonInput<Key>`。
//! - CapsLock/键位布局未处理（v1 静态 US 布局），与壁纸模式 v1 无键盘的既有范围一致。
//!
//! 仅在壁纸模式注册（见 examples/minimal）；窗口模式完全不加载本插件。

use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
use bevy::input::{ButtonState, InputSystems};
use bevy::prelude::*;
use bevy_live_wallpaper::WallpaperKeyboardState;
use smol_str::SmolStr;

pub struct WallpaperKeyboardPlugin;

impl Plugin for WallpaperKeyboardPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreUpdate, wallpaper_keyboard_inject.before(InputSystems));
    }
}

/// 当前按住的修饰键（跨帧状态，用于生成 shift 敏感的 `Key::Character`）。
/// 只跟踪 Shift；Ctrl/Alt/Super 只注入事件，不改字符映射（与 winit 语义一致）。
#[derive(Default)]
struct ModifierState {
    shift_left: bool,
    shift_right: bool,
}

impl ModifierState {
    fn shift(&self) -> bool {
        self.shift_left || self.shift_right
    }
}

fn wallpaper_keyboard_inject(
    mut keyboard: ResMut<WallpaperKeyboardState>,
    mut modifiers: Local<ModifierState>,
    mut messages: MessageWriter<KeyboardInput>,
) {
    // backend 每帧「替换」events（非追加），取走即排空，避免重复注入。
    let raw: Vec<_> = std::mem::take(&mut keyboard.events);

    for event in raw {
        let state = if event.pressed {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        };

        // 先更新修饰键状态，再生成字符，保证同一批内 Shift 按下 → 后续键出大写。
        match event.keycode {
            KEYCODE_SHIFT_LEFT => modifiers.shift_left = event.pressed,
            KEYCODE_SHIFT_RIGHT => modifiers.shift_right = event.pressed,
            _ => {}
        }

        let Some(key_code) = xkb_keycode_to_bevy(event.keycode) else {
            continue;
        };

        messages.write(KeyboardInput {
            key_code,
            logical_key: logical_key(event.keycode, modifiers.shift()),
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
    }
}

/// 生成与 bevy_winit `convert_logical_key` 语义一致的 logical_key。
fn logical_key(keycode: u32, shift: bool) -> Key {
    match keycode {
        KEYCODE_ESCAPE => Key::Escape,
        KEYCODE_BACKSPACE => Key::Backspace,
        KEYCODE_TAB => Key::Tab,
        KEYCODE_ENTER => Key::Enter,
        KEYCODE_SPACE => Key::Space,
        KEYCODE_DELETE => Key::Delete,
        KEYCODE_INSERT => Key::Insert,
        KEYCODE_HOME => Key::Home,
        KEYCODE_END => Key::End,
        KEYCODE_PAGE_UP => Key::PageUp,
        KEYCODE_PAGE_DOWN => Key::PageDown,
        KEYCODE_ARROW_UP => Key::ArrowUp,
        KEYCODE_ARROW_DOWN => Key::ArrowDown,
        KEYCODE_ARROW_LEFT => Key::ArrowLeft,
        KEYCODE_ARROW_RIGHT => Key::ArrowRight,
        KEYCODE_CAPS_LOCK => Key::CapsLock,
        KEYCODE_NUM_LOCK => Key::NumLock,
        KEYCODE_SCROLL_LOCK => Key::ScrollLock,
        KEYCODE_PRINT_SCREEN => Key::PrintScreen,
        KEYCODE_CONTEXT_MENU => Key::ContextMenu,
        KEYCODE_CONTROL_LEFT | KEYCODE_CONTROL_RIGHT => Key::Control,
        KEYCODE_SHIFT_LEFT | KEYCODE_SHIFT_RIGHT => Key::Shift,
        KEYCODE_ALT_LEFT | KEYCODE_ALT_RIGHT => Key::Alt,
        KEYCODE_SUPER_LEFT | KEYCODE_SUPER_RIGHT => Key::Super,
        KEYCODE_F1 => Key::F1,
        KEYCODE_F2 => Key::F2,
        KEYCODE_F3 => Key::F3,
        KEYCODE_F4 => Key::F4,
        KEYCODE_F5 => Key::F5,
        KEYCODE_F6 => Key::F6,
        KEYCODE_F7 => Key::F7,
        KEYCODE_F8 => Key::F8,
        KEYCODE_F9 => Key::F9,
        KEYCODE_F10 => Key::F10,
        2..=13 | 16..=27 | 30..=41 | 43..=53 => {
            let (plain, shifted) = us_layout_char(keycode);
            let c = if shift { shifted } else { plain };
            Key::Character(SmolStr::new(c.to_string()))
        }
        _ => Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
    }
}

/// US 布局字符表：XKB keycode → (无 Shift, 带 Shift)。
/// 覆盖字母行、数字行、符号键与空格；数字行带 Shift 出上档符号（`1`→`!` 等）。
fn us_layout_char(keycode: u32) -> (char, char) {
    match keycode {
        2 => ('1', '!'),
        3 => ('2', '@'),
        4 => ('3', '#'),
        5 => ('4', '$'),
        6 => ('5', '%'),
        7 => ('6', '^'),
        8 => ('7', '&'),
        9 => ('8', '*'),
        10 => ('9', '('),
        11 => ('0', ')'),
        12 => ('-', '_'),
        13 => ('=', '+'),
        16 => ('q', 'Q'),
        17 => ('w', 'W'),
        18 => ('e', 'E'),
        19 => ('r', 'R'),
        20 => ('t', 'T'),
        21 => ('y', 'Y'),
        22 => ('u', 'U'),
        23 => ('i', 'I'),
        24 => ('o', 'O'),
        25 => ('p', 'P'),
        26 => ('[', '{'),
        27 => (']', '}'),
        30 => ('a', 'A'),
        31 => ('s', 'S'),
        32 => ('d', 'D'),
        33 => ('f', 'F'),
        34 => ('g', 'G'),
        35 => ('h', 'H'),
        36 => ('j', 'J'),
        37 => ('k', 'K'),
        38 => ('l', 'L'),
        39 => (';', ':'),
        40 => ('\'', '"'),
        41 => ('`', '~'),
        43 => ('\\', '|'),
        44 => ('z', 'Z'),
        45 => ('x', 'X'),
        46 => ('c', 'C'),
        47 => ('v', 'V'),
        48 => ('b', 'B'),
        49 => ('n', 'N'),
        50 => ('m', 'M'),
        51 => (',', '<'),
        52 => ('.', '>'),
        53 => ('/', '?'),
        _ => (' ', ' '), // 不可达 fallback（逻辑键均已在上层分支覆盖）
    }
}

/// XKB keycode → bevy `KeyCode`。仅映射 US 布局常用键；未知键返回 None（跳过）。
fn xkb_keycode_to_bevy(keycode: u32) -> Option<KeyCode> {
    use KeyCode::*;
    Some(match keycode {
        1 => Escape,
        2 => Digit1,
        3 => Digit2,
        4 => Digit3,
        5 => Digit4,
        6 => Digit5,
        7 => Digit6,
        8 => Digit7,
        9 => Digit8,
        10 => Digit9,
        11 => Digit0,
        12 => Minus,
        13 => Equal,
        14 => Backspace,
        15 => Tab,
        16 => KeyQ,
        17 => KeyW,
        18 => KeyE,
        19 => KeyR,
        20 => KeyT,
        21 => KeyY,
        22 => KeyU,
        23 => KeyI,
        24 => KeyO,
        25 => KeyP,
        26 => BracketLeft,
        27 => BracketRight,
        28 => Enter,
        29 => ControlLeft,
        30 => KeyA,
        31 => KeyS,
        32 => KeyD,
        33 => KeyF,
        34 => KeyG,
        35 => KeyH,
        36 => KeyJ,
        37 => KeyK,
        38 => KeyL,
        39 => Semicolon,
        40 => Quote,
        41 => Backquote,
        42 => ShiftLeft,
        43 => Backslash,
        44 => KeyZ,
        45 => KeyX,
        46 => KeyC,
        47 => KeyV,
        48 => KeyB,
        49 => KeyN,
        50 => KeyM,
        51 => Comma,
        52 => Period,
        53 => Slash,
        54 => ShiftRight,
        55 => NumpadMultiply,
        56 => AltLeft,
        57 => Space,
        58 => CapsLock,
        59 => F1,
        60 => F2,
        61 => F3,
        62 => F4,
        63 => F5,
        64 => F6,
        65 => F7,
        66 => F8,
        67 => F9,
        68 => F10,
        69 => NumLock,
        70 => ScrollLock,
        71 => Numpad7,
        72 => Numpad8,
        73 => Numpad9,
        74 => Numpad4,
        75 => Numpad5,
        76 => Numpad6,
        77 => Numpad1,
        78 => Numpad2,
        79 => Numpad3,
        80 => Numpad0,
        81 => NumpadDecimal,
        86 => IntlBackslash,
        97 => ControlRight,
        98 => NumpadDivide,
        99 => PrintScreen,
        100 => AltRight,
        102 => Home,
        103 => ArrowUp,
        104 => PageUp,
        105 => ArrowLeft,
        106 => ArrowRight,
        107 => End,
        108 => ArrowDown,
        109 => PageDown,
        110 => Insert,
        111 => Delete,
        119 => Pause,
        125 => SuperLeft,
        126 => SuperRight,
        127 => ContextMenu,
        _ => return None,
    })
}

// XKB keycode 常量（Linux evdev 扫描码）。
const KEYCODE_ESCAPE: u32 = 1;
const KEYCODE_BACKSPACE: u32 = 14;
const KEYCODE_TAB: u32 = 15;
const KEYCODE_ENTER: u32 = 28;
const KEYCODE_CONTROL_LEFT: u32 = 29;
const KEYCODE_SHIFT_LEFT: u32 = 42;
const KEYCODE_SHIFT_RIGHT: u32 = 54;
const KEYCODE_ALT_LEFT: u32 = 56;
const KEYCODE_SPACE: u32 = 57;
const KEYCODE_CAPS_LOCK: u32 = 58;
const KEYCODE_F1: u32 = 59;
const KEYCODE_F2: u32 = 60;
const KEYCODE_F3: u32 = 61;
const KEYCODE_F4: u32 = 62;
const KEYCODE_F5: u32 = 63;
const KEYCODE_F6: u32 = 64;
const KEYCODE_F7: u32 = 65;
const KEYCODE_F8: u32 = 66;
const KEYCODE_F9: u32 = 67;
const KEYCODE_F10: u32 = 68;
const KEYCODE_NUM_LOCK: u32 = 69;
const KEYCODE_SCROLL_LOCK: u32 = 70;
const KEYCODE_CONTROL_RIGHT: u32 = 97;
const KEYCODE_PRINT_SCREEN: u32 = 99;
const KEYCODE_ALT_RIGHT: u32 = 100;
const KEYCODE_HOME: u32 = 102;
const KEYCODE_ARROW_UP: u32 = 103;
const KEYCODE_PAGE_UP: u32 = 104;
const KEYCODE_ARROW_LEFT: u32 = 105;
const KEYCODE_ARROW_RIGHT: u32 = 106;
const KEYCODE_END: u32 = 107;
const KEYCODE_ARROW_DOWN: u32 = 108;
const KEYCODE_PAGE_DOWN: u32 = 109;
const KEYCODE_INSERT: u32 = 110;
const KEYCODE_DELETE: u32 = 111;
const KEYCODE_SUPER_LEFT: u32 = 125;
const KEYCODE_SUPER_RIGHT: u32 = 126;
const KEYCODE_CONTEXT_MENU: u32 = 127;
