//! 统一光标与界面区域资源。
//!
//! 交互系统一律读 [`CursorPosition`] / [`UiArea`]，不直接查询主窗：
//! 窗口模式由 [`sync_cursor_from_window`] 同步（默认注册）；
//! 壁纸模式由 `wallpaper_bridge` 的合并系统接管写入（无主窗，本系统空转）。

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

#[derive(Resource, Debug, Clone, Copy)]
pub struct CursorPosition {
    /// 逻辑像素（UI 坐标，左上原点）
    pub logical: Vec2,
    /// UI 渲染空间像素（窗口模式 = 物理像素 = 逻辑×scale；壁纸模式 = Image 像素 = 逻辑，
    /// bevy 对 Image 目标取 scale_factor=1.0，命中检测一律用本字段）
    pub physical: Vec2,
    /// 合成器缩放（壁纸模式来自卫星 X 屏尺寸/逻辑尺寸；仅供 pet RTT 等非 UI 用途）
    pub scale: f32,
    /// 光标是否位于界面区域内（窗口模式 = 在主窗内；离开时交互系统按无光标处理）
    pub active: bool,
}

impl Default for CursorPosition {
    fn default() -> Self {
        Self {
            logical: Vec2::ZERO,
            physical: Vec2::ZERO,
            scale: 1.0,
            active: false,
        }
    }
}

/// 界面区域逻辑尺寸（窗口模式 = 主窗大小；壁纸模式 = 壁纸 surface 大小）。
#[derive(Resource, Debug, Clone, Copy, Default, Deref, DerefMut)]
pub struct UiArea(pub Vec2);

pub fn sync_cursor_from_window(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cursor: ResMut<CursorPosition>,
    mut area: ResMut<UiArea>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    area.0 = window.size();
    match window.cursor_position() {
        Some(pos) => {
            cursor.logical = pos;
            cursor.scale = window.scale_factor();
            cursor.physical = window
                .physical_cursor_position()
                .unwrap_or_else(|| pos * cursor.scale);
            cursor.active = true;
        }
        None => cursor.active = false,
    }
}

pub struct CursorPlugin;

impl Plugin for CursorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CursorPosition>()
            .init_resource::<UiArea>()
            .add_systems(First, sync_cursor_from_window);
    }
}
