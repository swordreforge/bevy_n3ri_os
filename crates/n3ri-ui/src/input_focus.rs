use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::apps::terminal::{TerminalOutput, TerminalState};
use crate::chat_capsule::ChatCapsuleState;
use crate::topbar::FocusedTitle;

const TERMINAL_FONT_SIZE: f32 = 14.0;
const TERMINAL_LINE_HEIGHT: f32 = 18.0;
const TERMINAL_PADDING: f32 = 12.0;
const CHAT_WIDTH: f32 = 320.0;
const CHAT_HEIGHT: f32 = 42.0;
const CHAT_BOTTOM: f32 = 14.0;
const CHAT_RIGHT: f32 = 24.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextInputFocus {
    None,
    Chat,
    Terminal,
    Settings(usize),
    Pictionary,
    SeekTreasure,
    Browser,
}

#[derive(Resource, Default, Debug)]
pub struct TextInputOwner(pub TextInputFocus);

impl Default for TextInputFocus {
    fn default() -> Self {
        Self::None
    }
}

impl TextInputOwner {
    pub fn is(&self, focus: TextInputFocus) -> bool {
        self.0 == focus
    }
}

pub fn sync_ime_window(
    owner: Res<TextInputOwner>,
    state: Res<TerminalState>,
    chat: Res<ChatCapsuleState>,
    focused: Res<FocusedTitle>,
    browser_anchor: Res<crate::apps::browser::BrowserImeAnchor>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    output: Query<(&ComputedNode, &UiGlobalTransform), With<TerminalOutput>>,
    guess_input: Query<(&ComputedNode, &UiGlobalTransform), With<crate::apps::pictionary::PicGuessInput>>,
    seek_input: Query<(&ComputedNode, &UiGlobalTransform), With<crate::apps::seek_treasure::StClueInput>>,
) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };

    match owner.0 {
        TextInputFocus::Browser => {
            if focused.title == "浏览器" {
                window.ime_enabled = browser_anchor.enabled;
                if browser_anchor.enabled {
                    window.ime_position = browser_anchor.pos;
                }
            } else {
                window.ime_enabled = false;
            }
        }
        TextInputFocus::Chat if chat.active => {
            let x = window.resolution.width() - CHAT_RIGHT - CHAT_WIDTH + 16.0;
            let y = window.resolution.height() - CHAT_BOTTOM - CHAT_HEIGHT * 0.5;
            window.ime_enabled = true;
            window.ime_position = Vec2::new(x, y);
        }
        TextInputFocus::Terminal if focused.title == "终端" => {
            if let Ok((node, transform)) = output.single() {
                let (echo_col, line) = state.input_caret();
                let x = -node.size().x * 0.5
                    + TERMINAL_PADDING
                    + echo_col as f32 * TERMINAL_FONT_SIZE;
                let y = -node.size().y * 0.5
                    + TERMINAL_PADDING
                    + line as f32 * TERMINAL_LINE_HEIGHT;
                window.ime_enabled = true;
                window.ime_position = transform.transform_point2(Vec2::new(x, y));
            }
        }
        TextInputFocus::Settings(_) => {
            window.ime_enabled = true;
        }
        TextInputFocus::Pictionary => {
            window.ime_enabled = true;
            if let Ok((node, transform)) = guess_input.single() {
                let x = -node.size().x * 0.5 + 12.0;
                window.ime_position = transform.transform_point2(Vec2::new(x, 0.0));
            }
        }
        TextInputFocus::SeekTreasure => {
            window.ime_enabled = true;
            if let Ok((node, transform)) = seek_input.single() {
                let x = -node.size().x * 0.5 + 12.0;
                window.ime_position = transform.transform_point2(Vec2::new(x, 0.0));
            }
        }
        TextInputFocus::None => {
            window.ime_enabled = false;
        }
        TextInputFocus::Chat | TextInputFocus::Terminal => {
            window.ime_enabled = false;
        }
    }
}
