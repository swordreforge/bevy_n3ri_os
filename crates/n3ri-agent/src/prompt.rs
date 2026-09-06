//! system 组装与 prompt 预算（M1：context 块；memory 段 M3）。
//!
//! 预算按字符数（Rust 无 tiktoken；CJK 为主时偏保守，可接受）。

use crate::world::{Activity, AgentWorldView, ContextSnapshot, Presence};

pub const CONTEXT_BLOCK_MAX_CHARS: usize = 800;

/// 时段词（纯函数）。
pub fn daypart(hour: u32) -> &'static str {
    match hour {
        0..=5 => "深夜",
        6..=7 => "凌晨",
        8..=11 => "上午",
        12..=13 => "中午",
        14..=17 => "下午",
        18..=19 => "傍晚",
        _ => "夜里",
    }
}

fn app_label(app_id: &str) -> &str {
    match app_id {
        "credits" => "致谢",
        "browser" => "浏览器",
        "mail" => "邮件",
        "files" => "文件",
        "signal" => "通讯",
        "pictionary" => "你画我猜",
        "idle" => "算力",
        "chess" => "国际象棋",
        "cakeduel" => "蛋糕对决",
        "codenames" => "森林寻宝",
        "terminal" => "终端",
        "settings" => "设置",
        _ => "桌面",
    }
}

fn dwell_text(dwell_secs: f32) -> String {
    if dwell_secs < 60.0 {
        "刚打开".to_string()
    } else if dwell_secs < 3600.0 {
        format!("已聚焦 {} 分钟", (dwell_secs / 60.0) as u32)
    } else {
        format!("已聚焦 {} 小时", (dwell_secs / 3600.0) as u32)
    }
}

/// 组装 `<context>` 块。砍预算顺序：visible_windows 尾部 → outside.windows 尾部。
/// 无记忆时 `<memory>` 整段省略（M3 接）。
pub fn build_context_block(view: &AgentWorldView, snap: &ContextSnapshot) -> String {
    let mut lines: Vec<String> = Vec::new();

    lines.push(format!("时间: {}", view.now_local));

    let (focus_id, dwell) = &snap.focus_dwell;
    let focus_line = if focus_id.is_empty() {
        "前台: 桌面（无聚焦窗口）".to_string()
    } else {
        format!(
            "前台: {} ({}, {})",
            app_label(focus_id),
            focus_id,
            dwell_text(*dwell)
        )
    };
    lines.push(focus_line);

    let mut names: Vec<&str> = view
        .visible_windows
        .iter()
        .map(|w| app_label(&w.app_id))
        .collect();
    names.dedup();
    let mut visible = format!("可见窗口: {}", names.join("、"));
    while visible.chars().count() > CONTEXT_BLOCK_MAX_CHARS / 2 && !names.is_empty() {
        names.pop();
        visible = format!("可见窗口: {}", names.join("、"));
    }
    lines.push(visible);

    if let Some(out) = view.outside.as_ref().filter(|o| o.available) {
        let mut counts: Vec<(&str, usize)> = Vec::new();
        for w in &out.windows {
            let label = outside_label(&w.app_id);
            match counts.iter_mut().find(|(l, _)| *l == label) {
                Some((_, n)) => *n += 1,
                None => counts.push((label, 1)),
            }
        }
        let mut parts: Vec<String> = counts
            .iter()
            .map(|(l, n)| {
                if *n > 1 {
                    format!("{l} x{n}")
                } else {
                    l.to_string()
                }
            })
            .collect();
        let focused = out
            .windows
            .iter()
            .find(|w| w.focused)
            .map(|w| outside_label(&w.app_id))
            .unwrap_or("未知");
        let mut outside = format!(
            "外面: niri 工作区 {}, {}（{}在前台）",
            out.workspace,
            parts.join("、"),
            focused
        );
        while outside.chars().count() > CONTEXT_BLOCK_MAX_CHARS / 3 && !parts.is_empty() {
            parts.pop();
            outside = format!(
                "外面: niri 工作区 {}, {}（{}在前台）",
                out.workspace,
                parts.join("、"),
                focused
            );
        }
        lines.push(outside);
    }

    let mut state: Vec<String> = Vec::new();
    match snap.activity {
        Activity::FocusedWork => state.push("专注中".to_string()),
        Activity::Immersive => state.push("沉浸中（勿扰）".to_string()),
        Activity::Free => state.push("空闲".to_string()),
    }
    if view.typing {
        state.push("正在输入".to_string());
    }
    match snap.presence {
        Presence::Active => {}
        Presence::Idle => state.push(format!("无操作 {} 分钟", (snap.idle_secs / 60.0) as u32)),
        Presence::Away => state.push(format!("离开 {} 分钟", (snap.idle_secs / 60.0) as u32)),
    }
    if view.music_playing {
        match &view.music_title {
            Some(t) if !t.is_empty() => state.push(format!("音乐播放中《{t}》")),
            _ => state.push("音乐播放中".to_string()),
        }
    }
    lines.push(format!("状态: {}", state.join("、")));

    lines.push(format!(
        "模式: {}",
        if view.wallpaper_mode {
            "壁纸模式"
        } else {
            "窗口模式"
        }
    ));

    let block = lines.join("\n");
    let mut out = format!("<context>\n{block}\n</context>");
    while out.chars().count() > CONTEXT_BLOCK_MAX_CHARS + 22 {
        if let Some(pos) = out.rfind("、") {
            out.truncate(pos);
            out.push_str("\n</context>");
        } else {
            break;
        }
    }
    out
}

/// 被动轮 system 追加：世界观 + 情绪指令（调用方传入）+ context 块 + memory 段。
pub fn append_context(
    system: &str,
    view: &AgentWorldView,
    snap: &ContextSnapshot,
    memory: Option<&str>,
) -> String {
    match memory {
        Some(m) if !m.is_empty() => format!("{system}\n{}\n{m}", build_context_block(view, snap)),
        _ => format!("{system}\n{}", build_context_block(view, snap)),
    }
}

fn outside_label(app_id: &str) -> &str {
    match app_id {
        "firefox" => "火狐",
        "kitty" | "Alacritty" => "终端",
        "org.gnome.Nautilus" => "文件",
        "code" => "编辑器",
        _ => "应用",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{ContextSnapshot, WindowInfo};

    fn view() -> AgentWorldView {
        AgentWorldView {
            focused_app_id: Some("terminal".into()),
            visible_windows: vec![
                WindowInfo {
                    app_id: "terminal".into(),
                    title: "终端".into(),
                    z: 2,
                },
                WindowInfo {
                    app_id: "browser".into(),
                    title: "浏览器".into(),
                    z: 1,
                },
            ],
            now_local: "2026-09-05 周五 14:03".into(),
            ..Default::default()
        }
    }

    #[test]
    fn daypart_table() {
        assert_eq!(daypart(0), "深夜");
        assert_eq!(daypart(5), "深夜");
        assert_eq!(daypart(6), "凌晨");
        assert_eq!(daypart(11), "上午");
        assert_eq!(daypart(12), "中午");
        assert_eq!(daypart(17), "下午");
        assert_eq!(daypart(19), "傍晚");
        assert_eq!(daypart(23), "夜里");
    }

    #[test]
    fn block_contains_sections() {
        let v = view();
        let snap = ContextSnapshot {
            focus_dwell: ("terminal".into(), 720.0),
            presence: Presence::Active,
            activity: Activity::FocusedWork,
            ..Default::default()
        };
        let b = build_context_block(&v, &snap);
        assert!(b.starts_with("<context>"));
        assert!(b.ends_with("</context>"));
        assert!(b.contains("终端 (terminal, 已聚焦 12 分钟)"));
        assert!(b.contains("可见窗口: 终端、浏览器"));
        assert!(b.contains("专注中"));
        assert!(b.contains("窗口模式"));
    }

    #[test]
    fn block_budget_and_empty_focus() {
        let mut v = view();
        v.focused_app_id = None;
        v.visible_windows = (0..50)
            .map(|i| WindowInfo {
                app_id: format!("app{i}"),
                title: format!("t{i}"),
                z: i,
            })
            .collect();
        let snap = ContextSnapshot::default();
        let b = build_context_block(&v, &snap);
        assert!(b.contains("无聚焦窗口"));
        assert!(b.chars().count() <= CONTEXT_BLOCK_MAX_CHARS + 22);
    }

    #[test]
    fn append_context_with_and_without_memory() {
        let v = view();
        let snap = ContextSnapshot::default();
        let a = append_context("sys", &v, &snap, None);
        assert!(a.contains("<context>"));
        assert!(!a.contains("<memory>"));
        let b = append_context("sys", &v, &snap, Some("<memory>\nx\n</memory>"));
        assert!(b.contains("<memory>"));
    }

    #[test]
    fn outside_renders_once_each() {
        let mut v = view();
        v.outside = Some(crate::world::OutsideView {
            workspace: "1".into(),
            windows: vec![
                crate::world::OutsideWindow {
                    id: 1,
                    title: "a".into(),
                    app_id: "firefox".into(),
                    focused: true,
                },
                crate::world::OutsideWindow {
                    id: 2,
                    title: "b".into(),
                    app_id: "firefox".into(),
                    focused: false,
                },
            ],
            available: true,
        });
        let snap = ContextSnapshot::default();
        let b = build_context_block(&v, &snap);
        assert!(b.contains("火狐 x2"));
        assert!(b.contains("火狐在前台"));
    }
}
