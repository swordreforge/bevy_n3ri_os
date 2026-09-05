//! Tool 注册与执行（M4）：`ToolRegistry` + 内置 tool handlers。
//!
//! 约束（dev §4.6/plan §6 M4）：
//! - Bevy-free：本模块只做纯计算 + 生成 `PendingEffect`；写 ECS（开窗/通知）
//!   由调用方在主线程落 `MessageWriter`（chat_capsule / dock 侧 reader）。
//! - 执行永不抛异常：未知 tool / 参数非法 / 白名单外 → `ToolEnvelope::err`，
//!   模型可自我纠正一次；失败只进一条错误气泡（调用方 turn.rs/chat 侧保证）。
//! - v1 全是安全动作：`open_app` 只开伪桌面内置窗口；`niri_window` 只 focus；
//!   `spawn-sh` 整段禁用；`close` 不暴露。

pub mod niri;

use crate::config::AgentConfig;
use crate::memory::{handle_recall_memory, MemoryStore};
use crate::tools::niri::{check_spawn_allow, spawn_allow_from, NiriCtl};
use crate::world::{OutsideView, OutsideWindow};

/// 需要主线程落 ECS 的副作用（线程内只生成，不执行）。
#[derive(Debug, Clone, PartialEq)]
pub enum PendingEffect {
    OpenApp { app_id: String },
    Notify { title: String, message: String },
}

/// 一次 tool 执行的完整结果：回模型的 envelope + 带回主线程的 effects。
#[derive(Debug, Clone)]
pub struct ToolOutcome {
    pub envelope: n3ri_llm::ToolEnvelope,
    pub effects: Vec<PendingEffect>,
}

/// `open_app` 白名单：伪桌面内置应用 id（与 dock `spawn_dock` entries 对齐）。
pub const OPEN_APP_ALLOW: &[&str] = &[
    "credits",
    "browser",
    "mail",
    "files",
    "signal",
    "pictionary",
    "idle",
    "chess",
    "cakeduel",
    "codenames",
    "terminal",
    "settings",
];

pub const NOTIFY_TITLE_MAX_CHARS: usize = 64;
pub const NOTIFY_MESSAGE_MAX_CHARS: usize = 500;

/// tool 注册表（emoclass 风格：无状态 struct + 关联函数，Bevy-free 可单测）。
pub struct ToolRegistry;

impl ToolRegistry {
    /// 按当前配置返回应暴露给模型的 tool defs。
    /// - `recall_memory`：memory_enabled 开才给。
    /// - niri 三件套：`cfg.niri_tools` 开才给（非 niri 会话运行时再静默降级）。
    pub fn defs_for(cfg: &AgentConfig) -> Vec<n3ri_llm::ToolDef> {
        let all = n3ri_llm::builtin_tool_defs();
        all.into_iter()
            .filter(|d| match d.name {
                "recall_memory" => cfg.memory_enabled,
                "niri_windows" | "niri_spawn" | "niri_window" => cfg.niri_tools,
                _ => true,
            })
            .collect()
    }

    /// 执行单个 tool call（线程内可调：纯计算 + 副作用只生成不执行）。
    pub fn execute(
        name: &str,
        args: &serde_json::Value,
        store: &MemoryStore,
        cfg: &AgentConfig,
        niri: &NiriCtl,
    ) -> ToolOutcome {
        match name {
            "recall_memory" => ToolOutcome::pure(handle_recall_memory(store, args)),
            "open_app" => Self::open_app(args),
            "notify" => Self::notify(args),
            "niri_windows" => Self::niri_windows(cfg, niri),
            "niri_spawn" => Self::niri_spawn(args, cfg, niri),
            "niri_window" => Self::niri_window(args, cfg, niri),
            _ => ToolOutcome::pure(n3ri_llm::ToolEnvelope::err(format!("未知工具 {name}"))),
        }
    }

    fn open_app(args: &serde_json::Value) -> ToolOutcome {
        let app_id = args.get("app_id").and_then(|v| v.as_str()).unwrap_or("").trim();
        if app_id.is_empty() {
            return ToolOutcome::pure(n3ri_llm::ToolEnvelope::err(
                "缺少 app_id（可选：terminal/files/browser/settings/mail/signal/pictionary/idle/chess/cakeduel/codenames/credits）",
            ));
        }
        if !OPEN_APP_ALLOW.contains(&app_id) {
            return ToolOutcome::pure(n3ri_llm::ToolEnvelope::err(format!(
                "未知应用 {app_id}（可选：{}）",
                OPEN_APP_ALLOW.join("/")
            )));
        }
        ToolOutcome::with_effect(
            n3ri_llm::ToolEnvelope::ok(serde_json::json!({ "ok": true, "app_id": app_id })),
            PendingEffect::OpenApp {
                app_id: app_id.to_string(),
            },
        )
    }

    fn notify(args: &serde_json::Value) -> ToolOutcome {
        let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("").trim();
        if message.is_empty() {
            return ToolOutcome::pure(n3ri_llm::ToolEnvelope::err("缺少 message（通知正文）"));
        }
        let message: String = message.chars().take(NOTIFY_MESSAGE_MAX_CHARS).collect();
        let title: String = args
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Nori")
            .trim()
            .chars()
            .take(NOTIFY_TITLE_MAX_CHARS)
            .collect();
        let title = if title.is_empty() { "Nori".to_string() } else { title };
        ToolOutcome::with_effect(
            n3ri_llm::ToolEnvelope::ok(serde_json::json!({ "ok": true })),
            PendingEffect::Notify {
                title: title.clone(),
                message: message.clone(),
            },
        )
    }

    fn niri_windows(cfg: &AgentConfig, niri: &NiriCtl) -> ToolOutcome {
        if !cfg.niri_tools {
            return ToolOutcome::pure(n3ri_llm::ToolEnvelope::ok(unavailable()));
        }
        match niri.windows() {
            Ok(ws) => ToolOutcome::pure(n3ri_llm::ToolEnvelope::ok(serde_json::json!({
                "available": true,
                "windows": ws.iter().map(|w| serde_json::json!({
                    "id": w.id, "title": w.title, "app_id": w.app_id, "focused": w.focused,
                })).collect::<Vec<_>>(),
            }))),
            Err(_) => ToolOutcome::pure(n3ri_llm::ToolEnvelope::ok(unavailable())),
        }
    }

    fn niri_spawn(args: &serde_json::Value, cfg: &AgentConfig, niri: &NiriCtl) -> ToolOutcome {
        if !cfg.niri_tools {
            return ToolOutcome::pure(n3ri_llm::ToolEnvelope::ok(unavailable()));
        }
        let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("").trim();
        if command.is_empty() {
            return ToolOutcome::pure(n3ri_llm::ToolEnvelope::err("缺少 command（白名单内的程序名）"));
        }
        let allow = spawn_allow_from(cfg);
        if let Err(e) = check_spawn_allow(command, &allow) {
            return ToolOutcome::pure(n3ri_llm::ToolEnvelope::err(e));
        }
        let mut argv = vec![command.to_string()];
        if let Some(extra) = args.get("args").and_then(|v| v.as_array()) {
            for a in extra.iter().filter_map(|v| v.as_str()).take(8) {
                if !a.is_empty() {
                    argv.push(a.to_string());
                }
            }
        }
        match niri.spawn(&argv, &allow) {
            Ok(msg) => ToolOutcome::pure(n3ri_llm::ToolEnvelope::ok(serde_json::json!({ "ok": true, "note": msg }))),
            Err(e) => ToolOutcome::pure(n3ri_llm::ToolEnvelope::err(e)),
        }
    }

    fn niri_window(args: &serde_json::Value, cfg: &AgentConfig, niri: &NiriCtl) -> ToolOutcome {
        if !cfg.niri_tools {
            return ToolOutcome::pure(n3ri_llm::ToolEnvelope::ok(unavailable()));
        }
        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("").trim();
        if action != "focus" {
            return ToolOutcome::pure(n3ri_llm::ToolEnvelope::err(
                "action 只支持 focus（close 暂不暴露）",
            ));
        }
        let Some(id) = args.get("window_id").and_then(|v| v.as_u64()) else {
            return ToolOutcome::pure(n3ri_llm::ToolEnvelope::err(
                "缺少 window_id（整数，先调 niri_windows 查）",
            ));
        };
        if id == 0 {
            return ToolOutcome::pure(n3ri_llm::ToolEnvelope::err("window_id 非法"));
        }
        match niri.focus_window(id) {
            Ok(()) => ToolOutcome::pure(n3ri_llm::ToolEnvelope::ok(serde_json::json!({ "ok": true, "window_id": id }))),
            Err(_) => ToolOutcome::pure(n3ri_llm::ToolEnvelope::ok(unavailable())),
        }
    }
}

impl ToolOutcome {
    fn pure(envelope: n3ri_llm::ToolEnvelope) -> Self {
        Self { envelope, effects: Vec::new() }
    }

    fn with_effect(envelope: n3ri_llm::ToolEnvelope, effect: PendingEffect) -> Self {
        Self { envelope, effects: vec![effect] }
    }
}

fn unavailable() -> serde_json::Value {
    serde_json::json!({ "available": false, "note": "当前不在 niri 会话里，看不到外部窗口" })
}

/// 供 agent_bridge 5s 缓存调用：成功返回 `Some(view)`（含 `available:false` 的
/// 降级视图也算 Some，调用方照常比较/写入）；`niri_tools` 关时返回 None。
pub fn poll_outside_view(cfg: &AgentConfig, niri: &NiriCtl) -> Option<OutsideView> {
    if !cfg.niri_tools {
        return None;
    }
    Some(niri.snapshot())
}

/// 解析 `niri_windows` tool 结果回 `OutsideView`（context 渲染/单测用）。
pub fn outside_from_tool_output(output: &serde_json::Value, workspace: &str) -> OutsideView {
    let available = output.get("available").and_then(|v| v.as_bool()).unwrap_or(false);
    if !available {
        return OutsideView {
            workspace: "?".to_string(),
            windows: Vec::new(),
            available: false,
        };
    }
    let windows: Vec<OutsideWindow> = output
        .get("windows")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|w| {
                    Some(OutsideWindow {
                        id: w.get("id")?.as_u64()?,
                        title: w.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        app_id: w.get("app_id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        focused: w.get("focused").and_then(|v| v.as_bool()).unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    OutsideView {
        workspace: workspace.to_string(),
        windows,
        available: true,
    }
}

/// 真正的消息发送：`notify-send -a norios -i <icon> <title> <message>`。
/// 子进程 spawn-and-forget（不管结果，失败静默——通知不值得弹错）。
/// icon 缺失时退化为无 `-i` 调用。
pub fn send_desktop_notification(title: &str, message: &str) {
    const APP_ID: &str = "norios";
    const ICON: &str =
        "/home/swordreforge/project/业余项目/bevy_n3ri_os/assets/nori/app-icons/credits/icon-a.png";
    let mut cmd = std::process::Command::new("notify-send");
    cmd.arg("-a").arg(APP_ID);
    if std::path::Path::new(ICON).is_file() {
        cmd.arg("-i").arg(ICON);
    }
    cmd.arg(title).arg(message);
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null());
    let _ = cmd.spawn();
}

/// 供单测：不 spawn 子进程，只检查参数投影（与上面真实调用保持一致）。
pub fn notify_send_argv(title: &str, message: &str, icon_exists: bool) -> Vec<String> {
    let mut argv = vec!["-a".to_string(), "norios".to_string()];
    if icon_exists {
        argv.push("-i".to_string());
        argv.push(
            "/home/swordreforge/project/业余项目/bevy_n3ri_os/assets/nori/app-icons/credits/icon-a.png"
                .to_string(),
        );
    }
    argv.push(title.to_string());
    argv.push(message.to_string());
    argv
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::tools::niri::parse_windows_json;

    fn cfg() -> AgentConfig {
        AgentConfig::default()
    }

    fn store() -> MemoryStore {
        MemoryStore::default()
    }

    fn niri() -> NiriCtl {
        NiriCtl::new("niri-test-nonexistent-bin")
    }

    #[test]
    fn defs_respect_switches() {
        let full = ToolRegistry::defs_for(&cfg());
        assert_eq!(full.len(), 6);
        let mut c = cfg();
        c.memory_enabled = false;
        c.niri_tools = false;
        let names: Vec<&str> = ToolRegistry::defs_for(&c).iter().map(|d| d.name).collect();
        assert_eq!(names, vec!["open_app", "notify"]);
    }

    #[test]
    fn unknown_tool_is_error_envelope() {
        let out = ToolRegistry::execute("nope", &serde_json::json!({}), &store(), &cfg(), &niri());
        assert!(out.envelope.is_error);
        assert!(out.effects.is_empty());
    }

    #[test]
    fn open_app_validates_and_defers_effect() {
        let ok = ToolRegistry::execute(
            "open_app",
            &serde_json::json!({ "app_id": "terminal" }),
            &store(),
            &cfg(),
            &niri(),
        );
        assert!(!ok.envelope.is_error);
        assert!(matches!(
            ok.effects.as_slice(),
            [PendingEffect::OpenApp { app_id }] if app_id == "terminal"
        ));
        let bad = ToolRegistry::execute(
            "open_app",
            &serde_json::json!({ "app_id": "rm -rf" }),
            &store(),
            &cfg(),
            &niri(),
        );
        assert!(bad.envelope.is_error);
        assert!(bad.effects.is_empty());
        let missing = ToolRegistry::execute("open_app", &serde_json::json!({}), &store(), &cfg(), &niri());
        assert!(missing.envelope.is_error);
    }

    #[test]
    fn notify_truncates_and_defaults_title() {
        let out = ToolRegistry::execute(
            "notify",
            &serde_json::json!({ "message": "喝水" }),
            &store(),
            &cfg(),
            &niri(),
        );
        assert!(!out.envelope.is_error);
        assert!(matches!(
            out.effects.as_slice(),
            [PendingEffect::Notify { title, message }]
            if title == "Nori" && message == "喝水"
        ));
        let long = "x".repeat(600);
        let out2 = ToolRegistry::execute(
            "notify",
            &serde_json::json!({ "message": long }),
            &store(),
            &cfg(),
            &niri(),
        );
        match out2.effects.as_slice() {
            [PendingEffect::Notify { message, .. }] => {
                assert_eq!(message.chars().count(), NOTIFY_MESSAGE_MAX_CHARS)
            }
            _ => panic!("应有 notify effect"),
        }
        let empty = ToolRegistry::execute("notify", &serde_json::json!({}), &store(), &cfg(), &niri());
        assert!(empty.envelope.is_error);
    }

    #[test]
    fn niri_gated_off_returns_unavailable_without_subprocess() {
        let mut c = cfg();
        c.niri_tools = false;
        for (name, args) in [
            ("niri_windows", serde_json::json!({})),
            ("niri_spawn", serde_json::json!({ "command": "kitty" })),
            ("niri_window", serde_json::json!({ "action": "focus", "window_id": 1 })),
        ] {
            let out = ToolRegistry::execute(name, &args, &store(), &c, &niri());
            assert!(!out.envelope.is_error, "{name} 降级不应是 error");
            assert!(out.effects.is_empty());
        }
        assert!(poll_outside_view(&c, &niri()).is_none());
    }

    #[test]
    fn niri_window_rejects_non_focus_and_bad_id() {
        let out = ToolRegistry::execute(
            "niri_window",
            &serde_json::json!({ "action": "close", "window_id": 1 }),
            &store(),
            &cfg(),
            &niri(),
        );
        assert!(out.envelope.is_error);
        let out2 = ToolRegistry::execute(
            "niri_window",
            &serde_json::json!({ "action": "focus" }),
            &store(),
            &cfg(),
            &niri(),
        );
        assert!(out2.envelope.is_error);
    }

    #[test]
    fn niri_spawn_rejects_off_allowlist() {
        let out = ToolRegistry::execute(
            "niri_spawn",
            &serde_json::json!({ "command": "rm" }),
            &store(),
            &cfg(),
            &niri(),
        );
        assert!(out.envelope.is_error);
    }

    #[test]
    fn notify_argv_shape() {
        let argv = notify_send_argv("t", "m", true);
        assert_eq!(argv[0..2], vec!["-a".to_string(), "norios".to_string()]);
        assert!(argv.contains(&"-i".to_string()));
        assert_eq!(argv[argv.len() - 2..], vec!["t".to_string(), "m".to_string()]);
        let argv2 = notify_send_argv("t", "m", false);
        assert!(!argv2.contains(&"-i".to_string()));
    }

    #[test]
    fn outside_from_tool_output_roundtrip() {
        let raw = r#"[{"id":7,"title":"t","app_id":"kitty","is_focused":true,"is_floating":false}]"#;
        let ws = parse_windows_json(raw).expect("解析成功");
        let output = serde_json::json!({
            "available": true,
            "windows": ws.iter().map(|w| serde_json::json!({
                "id": w.id, "title": w.title, "app_id": w.app_id, "focused": w.focused,
            })).collect::<Vec<_>>(),
        });
        let view = outside_from_tool_output(&output, "1");
        assert!(view.available);
        assert_eq!(view.windows.len(), 1);
        assert_eq!(view.windows[0].app_id, "kitty");
        let off = outside_from_tool_output(&serde_json::json!({ "available": false }), "1");
        assert!(!off.available);
    }
}
