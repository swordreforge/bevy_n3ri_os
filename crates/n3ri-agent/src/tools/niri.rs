//! niri 特化 tools 执行面（M4）：`niri msg` 子进程封装 + 白名单。
//!
//! 设计约束（dev §4.6）：
//! - 默认走 `niri msg` 子进程（实现最简，5s 缓存下开销可忽略），不用 `$NIRI_SOCKET` raw 协议。
//! - `spawn-sh` 整段禁用（shell 注入面太大）；`niri_window` 只暴露 focus，不暴露 close。
//! - 非 niri 会话（`niri msg` 不可用）→ 返回 `available: false`，静默降级不弹错。
//! - 全部纯函数 + 薄封装，方便单测；子进程调用本身不单测（走 M5 手动走查）。
//! - Bevy-free：`poll_outside()` 在 agent_bridge 侧做缓存后调这里的纯函数。

use std::process::Command;
use std::time::Duration;

use crate::config::AgentConfig;
use crate::world::{OutsideView, OutsideWindow};

pub const NIRI_TIMEOUT: Duration = Duration::from_secs(3);
pub const NIRI_ERR_SNIPPET_CHARS: usize = 300;

#[derive(Debug, Clone)]
pub struct NiriCtl {
    pub niri_bin: String,
}

impl Default for NiriCtl {
    fn default() -> Self {
        Self {
            niri_bin: "niri".to_string(),
        }
    }
}

impl NiriCtl {
    pub fn new(niri_bin: impl Into<String>) -> Self {
        Self {
            niri_bin: niri_bin.into(),
        }
    }

    fn run_msg(&self, args: &[&str]) -> Result<String, String> {
        use std::io::Read;
        let mut child = Command::new(&self.niri_bin)
            .arg("msg")
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("niri 不可用: {e}"))?;
        // 先把管道接走再等退出，避免输出超 64K 管道缓冲时死锁。
        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut o = String::new();
            let mut e = String::new();
            if let Some(mut s) = stdout.take() {
                let _ = s.read_to_string(&mut o);
            }
            if let Some(mut s) = stderr.take() {
                let _ = s.read_to_string(&mut e);
            }
            let status = child.wait();
            let _ = tx.send((status, o, e));
        });
        match rx.recv_timeout(NIRI_TIMEOUT) {
            // 超时只报 Err；waiter 线程仍持有 child，niri 退出时自行收尸，不留 zombie。
            Err(_) => Err("niri msg 超时".to_string()),
            Ok((Err(e), _, _)) => Err(format!("等待 niri 失败: {e}")),
            Ok((Ok(status), o, _e)) if status.success() => Ok(o),
            Ok((Ok(_), _, e)) => Err(snippet(&e)),
        }
    }

    /// `niri msg --json windows` → 投影 4 字段。失败回 Err（原文截 300ch）。
    pub fn windows(&self) -> Result<Vec<OutsideWindow>, String> {
        let raw = self.run_msg(&["--json", "windows"])?;
        parse_windows_json(&raw)
    }

    /// `niri msg --json focused-window` 所在工作区名（拿不到则 "?"）。
    pub fn active_workspace(&self) -> String {
        let Ok(raw) = self.run_msg(&["--json", "workspaces"]) else {
            return "?".to_string();
        };
        parse_active_workspace(&raw).unwrap_or_else(|| "?".to_string())
    }

    /// `niri msg action spawn -- <argv...>`。argv[0] 必须过白名单。
    pub fn spawn(&self, argv: &[String], allow: &[String]) -> Result<String, String> {
        let Some(cmd) = argv.first() else {
            return Err("command 为空".to_string());
        };
        check_spawn_allow(cmd, allow)?;
        let mut c = Command::new(&self.niri_bin);
        c.arg("msg").arg("action").arg("spawn").arg("--");
        for a in argv {
            c.arg(a);
        }
        c.stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("niri spawn 失败: {e}"))?;
        Ok(format!("已启动 {cmd}"))
    }

    /// `niri msg action focus-window --id <id>`。
    pub fn focus_window(&self, id: u64) -> Result<(), String> {
        self.run_msg(&["action", "focus-window", "--id", &id.to_string()])?;
        Ok(())
    }

    /// 外部投影快照（给 agent_bridge 5s 缓存用）：windows + workspace 一起拿，
    /// 失败整包回 `available: false` 空视图。
    pub fn snapshot(&self) -> OutsideView {
        match self.windows() {
            Ok(windows) => OutsideView {
                workspace: self.active_workspace(),
                windows,
                available: true,
            },
            Err(_) => OutsideView {
                workspace: "?".to_string(),
                windows: Vec::new(),
                available: false,
            },
        }
    }
}

/// 白名单检查（纯函数）：精确匹配程序名；返回 Err 时带可用列表提示。
pub fn check_spawn_allow(cmd: &str, allow: &[String]) -> Result<(), String> {
    if allow.iter().any(|a| a == cmd) {
        Ok(())
    } else {
        Err(format!("{cmd} 不在启动白名单里（可用：{}）", allow.join("、")))
    }
}

fn snippet(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() {
        return "niri 返回错误（无详情）".to_string();
    }
    t.chars().take(NIRI_ERR_SNIPPET_CHARS).collect()
}

/// 解析 `niri msg --json windows` 数组 → 4 字段投影。app_id 缺失（null）→ ""。
/// 非数组/非法 JSON → Err（原文截 300ch）。
pub fn parse_windows_json(raw: &str) -> Result<Vec<OutsideWindow>, String> {
    let v: serde_json::Value =
        serde_json::from_str(raw).map_err(|_| format!("niri windows 解析失败: {}", snippet(raw)))?;
    let arr = v.as_array().ok_or_else(|| format!("niri windows 非数组: {}", snippet(raw)))?;
    let mut out = Vec::with_capacity(arr.len());
    for w in arr {
        let id = w.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        if id == 0 {
            continue;
        }
        out.push(OutsideWindow {
            id,
            title: w.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            app_id: w.get("app_id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            focused: w.get("is_focused").and_then(|v| v.as_bool()).unwrap_or(false),
        });
    }
    Ok(out)
}

/// 解析 `niri msg --json workspaces` → 当前聚焦工作区的 idx（无聚焦则 active 的）。
pub fn parse_active_workspace(raw: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let arr = v.as_array()?;
    arr.iter()
        .find(|w| w.get("is_focused").and_then(|v| v.as_bool()).unwrap_or(false))
        .or_else(|| {
            arr.iter()
                .find(|w| w.get("is_active").and_then(|v| v.as_bool()).unwrap_or(false))
        })
        .and_then(|w| {
            w.get("idx")
                .and_then(|v| v.as_u64())
                .map(|i| i.to_string())
                .or_else(|| w.get("name").and_then(|v| v.as_str()).map(str::to_string))
        })
}

/// 从 AgentConfig 派生 spawn 白名单（空配置时回退编译期默认）。
pub fn spawn_allow_from(cfg: &AgentConfig) -> Vec<String> {
    if cfg.niri_spawn_allow.is_empty() {
        AgentConfig::default().niri_spawn_allow
    } else {
        cfg.niri_spawn_allow.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[{"id":100,"title":"⌘ Agent Architecture","app_id":"Alacritty","pid":108492,"workspace_id":1,"is_focused":false,"is_floating":false,"layout":{},"focus_timestamp":{"secs":32183,"nanos":1}},{"id":24,"title":"release","app_id":"org.gnome.Nautilus","pid":29558,"workspace_id":1,"is_focused":false,"is_floating":false,"layout":{},"focus_timestamp":{"secs":32054,"nanos":1}},{"id":109,"title":"n3ri_os","app_id":null,"pid":112858,"workspace_id":1,"is_focused":true,"is_floating":false,"layout":{},"focus_timestamp":{"secs":32203,"nanos":1}}]"#;

    #[test]
    fn windows_snapshot_projects_four_fields() {
        let ws = parse_windows_json(SAMPLE).expect("解析成功");
        assert_eq!(ws.len(), 3);
        assert_eq!(ws[0].id, 100);
        assert_eq!(ws[0].app_id, "Alacritty");
        assert!(!ws[0].focused);
        assert_eq!(ws[1].app_id, "org.gnome.Nautilus");
        assert_eq!(ws[2].app_id, "");
        assert!(ws[2].focused);
    }

    #[test]
    fn windows_bad_json_errors_with_snippet() {
        let err = parse_windows_json("not json at all").expect_err("应失败");
        assert!(err.contains("解析失败"));
        let err2 = parse_windows_json(r#"{"a":1}"#).expect_err("非数组应失败");
        assert!(err2.contains("非数组"));
    }

    #[test]
    fn workspace_picks_focused_idx() {
        let raw = r#"[{"id":1,"idx":1,"name":null,"output":"eDP-1","is_active":true,"is_focused":true,"active_window_id":100},{"id":4,"idx":2,"name":null,"output":"eDP-1","is_active":false,"is_focused":false,"active_window_id":null}]"#;
        assert_eq!(parse_active_workspace(raw).as_deref(), Some("1"));
        assert_eq!(parse_active_workspace("[]"), None);
    }

    #[test]
    fn spawn_allow_exact_match_only() {
        let allow = vec!["firefox".to_string(), "kitty".to_string()];
        assert!(check_spawn_allow("kitty", &allow).is_ok());
        assert!(check_spawn_allow("kitty;rm", &allow).is_err());
        assert!(check_spawn_allow("Firefox", &allow).is_err());
        assert!(check_spawn_allow("", &allow).is_err());
    }

    #[test]
    fn spawn_allow_falls_back_to_default() {
        let mut cfg = AgentConfig::default();
        cfg.niri_spawn_allow.clear();
        assert!(!spawn_allow_from(&cfg).is_empty());
    }
}
