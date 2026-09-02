//! 壁纸模式 IME 桥接。
//!
//! 壁纸模式没有 winit 主窗，bevy 原生 `Ime` 消息通道（winit IME）不工作。本模块把
//! vendor 层 `zwp_text_input_v3` 事件（[`WallpaperTextInputState`]）按 text-input-v3
//! 双缓冲语义映射为 bevy `Ime` 消息注入 `MessageWriter<Ime>`，既有 chat/terminal/
//! settings 等 `Ime` 消费方无需改动即可工作；反向由 [`TextInputOwner`] 驱动
//! [`WallpaperTextInputControl`] 发送 enable/disable。仅在壁纸模式注册（见
//! examples/minimal）。

use bevy::prelude::*;
use bevy::window::Ime;
use bevy_live_wallpaper::{TextInputEvent, WallpaperTextInputControl, WallpaperTextInputState};

use crate::input_focus::{TextInputFocus, TextInputOwner};

pub struct WallpaperImePlugin;

impl Plugin for WallpaperImePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ImeBatch>()
            .add_systems(PreUpdate, wallpaper_ime_inject)
            .add_systems(Update, wallpaper_ime_control);
    }
}

/// text-input-v3 的 preedit/commit 是双缓冲的：compositor 可先发多条
/// `preedit_string` / `commit_string`，直到 `done` 才要求客户端应用该批状态。
/// 事件与 `done` 可能跨帧到达，故缓冲放资源而非 Local。
#[derive(Resource, Default)]
struct ImeBatch {
    preedit: Option<(String, i32, i32)>,
    commit: Option<String>,
}

fn wallpaper_ime_inject(
    mut text_input: ResMut<WallpaperTextInputState>,
    mut batch: ResMut<ImeBatch>,
    mut messages: MessageWriter<Ime>,
) {
    for event in text_input.events.drain(..) {
        match event {
            TextInputEvent::Enter => {
                flush_batch(&mut batch, &mut messages);
                messages.write(Ime::Enabled {
                    window: Entity::PLACEHOLDER,
                });
            }
            TextInputEvent::Leave => {
                flush_batch(&mut batch, &mut messages);
                messages.write(Ime::Disabled {
                    window: Entity::PLACEHOLDER,
                });
            }
            TextInputEvent::Preedit {
                text,
                cursor_begin,
                cursor_end,
            } => {
                batch.preedit = Some((text, cursor_begin, cursor_end));
            }
            TextInputEvent::Commit { text } => {
                batch.commit = Some(text);
            }
            TextInputEvent::Done { .. } => {
                flush_batch(&mut batch, &mut messages);
            }
        }
    }
}

fn flush_batch(batch: &mut ImeBatch, messages: &mut MessageWriter<Ime>) {
    // 协议应用顺序：先插入 commit 文本，再设置新的 preedit（cursor 为字节偏移，
    // 两者均为 -1 表示隐藏光标）。
    if let Some(commit) = batch.commit.take() {
        messages.write(Ime::Commit {
            window: Entity::PLACEHOLDER,
            value: commit,
        });
    }
    if let Some((text, begin, end)) = batch.preedit.take() {
        let cursor = if begin < 0 || end < 0 {
            None
        } else {
            Some((begin as usize, end as usize))
        };
        messages.write(Ime::Preedit {
            window: Entity::PLACEHOLDER,
            value: text,
            cursor,
        });
    }
}

fn wallpaper_ime_control(
    owner: Res<TextInputOwner>,
    mut control: ResMut<WallpaperTextInputControl>,
) {
    control.enabled = owner.0 != TextInputFocus::None;
}
