use std::io::Read;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use serde_json::Value;

use super::safety::{is_command_allowed, sanitize_path, truncate_result, SafetyConfig};

const MAX_RESULT_CHARS: usize = 4096;

/// Execute a shell command and return its stdout + stderr.
///
/// Spawns the process in its own process group so we can kill all children on timeout.
/// stdout/stderr are captured via pipe readers to avoid pipe buffer deadlocks.
///
/// Arguments (from JSON):
///   cmd: string — the command to run.
///   timeout_secs: number (optional, default 5) — timeout in seconds (max 30).
pub fn exec_cmd(args: &Value, safety: &SafetyConfig) -> Result<String, String> {
    let cmd = args["cmd"]
        .as_str()
        .ok_or_else(|| "missing 'cmd' argument".to_string())?;
    let timeout = args
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(5)
        .min(30);

    if !safety.user_approved && !is_command_allowed(cmd, &safety.allowed_commands) {
        return Err(format!(
            "command '{}' is not in the allowed commands list",
            cmd.split_whitespace().next().unwrap_or(cmd)
        ));
    }

    let mut cmd_builder = Command::new("sh");
    cmd_builder
        .arg("-c")
        .arg(cmd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Create a new process group so we can kill all children on timeout
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd_builder.process_group(0);
    }

    let mut child = cmd_builder
        .spawn()
        .map_err(|e| format!("failed to spawn command: {e}"))?;

    let pid = child.id();

    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();

    let stdout_handle = thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout.read_to_string(&mut buf);
        buf
    });
    let stderr_handle = thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr.read_to_string(&mut buf);
        buf
    });

    let timeout_dur = Duration::from_secs(timeout);
    let start = std::time::Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout_text = stdout_handle.join().unwrap_or_default();
                let stderr_text = stderr_handle.join().unwrap_or_default();
                return build_result(&stdout_text, &stderr_text, status.code());
            }
            Ok(None) => {
                if start.elapsed() >= timeout_dur {
                    kill_process_tree(pid);
                    // Give children a moment to die, then collect output
                    thread::sleep(Duration::from_millis(100));
                    let stdout_text = stdout_handle.join().unwrap_or_default();
                    let stderr_text = stderr_handle.join().unwrap_or_default();
                    let mut result = truncate_smart(&stdout_text, MAX_RESULT_CHARS);
                    if !stderr_text.is_empty() {
                        if !result.is_empty() {
                            result.push('\n');
                        }
                        result.push_str("stderr:\n");
                        result.push_str(&truncate_smart(&stderr_text, MAX_RESULT_CHARS / 2));
                    }
                    if !result.is_empty() {
                        result.push('\n');
                    }
                    result.push_str(&format!("⚠ 命令超时（{timeout}s），进程已终止"));
                    return Ok(result);
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                kill_process_tree(pid);
                return Err(format!("failed to wait for command: {e}"));
            }
        }
    }
}

/// Kill a process group by PGID (negative PID = kill group).
fn kill_process_tree(pid: u32) {
    let pgid = pid as i32; // process_group(0) means PGID == PID
    unsafe {
        libc::kill(-pgid, libc::SIGTERM);
    }
    // SIGKILL after short grace period
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        unsafe {
            libc::kill(-pgid, libc::SIGKILL);
        }
    });
}

/// Build the result string from stdout, stderr, and exit code.
fn build_result(stdout: &str, stderr: &str, exit_code: Option<i32>) -> Result<String, String> {
    let mut result = truncate_smart(stdout, MAX_RESULT_CHARS);

    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str("stderr:\n");
        result.push_str(&truncate_smart(stderr, MAX_RESULT_CHARS / 2));
    }

    match exit_code {
        Some(0) => Ok(result),
        Some(code) => {
            if result.is_empty() {
                Err(format!("command exited with code {code}"))
            } else {
                Ok(format!("{result}\n(exit code: {code})"))
            }
        }
        None => {
            if result.is_empty() {
                Err("command terminated by signal".to_string())
            } else {
                Ok(result)
            }
        }
    }
}

/// Smart truncation: keep first half + last half, show line count of removed middle.
fn truncate_smart(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        return s.to_string();
    }
    let half = max_chars / 2;
    let start = &s[..half];
    let end = &s[s.len() - half..];
    let removed = &s[half..s.len() - half];
    let removed_lines = removed.lines().count();
    format!("{start}\n\n... [{removed_lines} lines truncated] ...\n\n{end}")
}

/// Read a text file and return its contents.
///
/// Arguments (from JSON):
///   path: string — path to the file.
///   max_lines: number (optional) — maximum lines to read.
pub fn read_file(args: &Value, safety: &SafetyConfig) -> Result<String, String> {
    let raw_path = args["path"]
        .as_str()
        .ok_or_else(|| "missing 'path' argument".to_string())?;
    let path = sanitize_path(
        raw_path,
        &safety.allowed_read_paths,
        safety.working_dir.as_deref(),
    )?;
    let max_lines = args.get("max_lines").and_then(|v| v.as_u64());

    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read '{}': {e}", path.display()))?;

    let result = if let Some(max) = max_lines {
        let lines: Vec<&str> = content.lines().take(max as usize).collect();
        lines.join("\n")
    } else {
        content
    };

    Ok(truncate_result(&result, MAX_RESULT_CHARS))
}

/// List entries in a directory.
///
/// Arguments:
///   path: string — directory path.
///   max_entries: number (optional, default 50).
pub fn list_dir(args: &Value, safety: &SafetyConfig) -> Result<String, String> {
    let raw_path = args["path"]
        .as_str()
        .ok_or_else(|| "missing 'path' argument".to_string())?;
    let path = sanitize_path(
        raw_path,
        &safety.allowed_read_paths,
        safety.working_dir.as_deref(),
    )?;
    let max_entries = args
        .get("max_entries")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;

    let entries: Vec<String> = std::fs::read_dir(&path)
        .map_err(|e| format!("cannot read directory '{}': {e}", path.display()))?
        .filter_map(|e| e.ok())
        .take(max_entries)
        .map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let kind = if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                "📁"
            } else {
                "📄"
            };
            format!("{kind} {name}")
        })
        .collect();

    Ok(truncate_result(&entries.join("\n"), MAX_RESULT_CHARS))
}

/// Read /proc information.
///
/// Arguments:
///   pid: number (optional) — specific process ID.
///   stat: string (optional) — what to read ("status", "cmdline", "maps").
pub fn read_proc(args: &Value, _safety: &SafetyConfig) -> Result<String, String> {
    let pid = args
        .get("pid")
        .and_then(|v| v.as_u64())
        .map(|p| p.to_string())
        .unwrap_or_else(|| "self".to_string());
    let stat = args
        .get("stat")
        .and_then(|v| v.as_str())
        .unwrap_or("status");

    let path = format!("/proc/{pid}/{stat}");
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(truncate_result(&content, MAX_RESULT_CHARS)),
        Err(e) => Err(format!("cannot read {path}: {e}")),
    }
}

/// Get an environment variable value.
pub fn get_env(args: &Value, _safety: &SafetyConfig) -> Result<String, String> {
    let name = args["name"]
        .as_str()
        .ok_or_else(|| "missing 'name' argument".to_string())?;
    match std::env::var(name) {
        Ok(val) => Ok(val),
        Err(std::env::VarError::NotPresent) => Err(format!("env var '{name}' is not set")),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(format!("env var '{name}' contains non-unicode data"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::tools::safety::SafetyConfig;

    fn safe_cfg() -> SafetyConfig {
        SafetyConfig {
            allowed_commands: vec!["echo".to_string(), "sleep".to_string()],
            allowed_read_paths: vec![],
            max_tool_rounds: 10,
            user_approved: false,
            working_dir: None,
        }
    }

    #[test]
    fn exec_cmd_echo() {
        let args = serde_json::json!({"cmd": "echo hello world"});
        let result = exec_cmd(&args, &safe_cfg()).unwrap();
        assert!(result.contains("hello world"));
    }

    #[test]
    fn exec_cmd_timeout() {
        let args = serde_json::json!({"cmd": "sleep 10", "timeout_secs": 1});
        let result = exec_cmd(&args, &safe_cfg());
        // Timeout returns Ok with partial output + warning (process killed)
        let text = result.unwrap();
        assert!(text.contains("超时") || text.contains("timed out"));
    }

    #[test]
    fn read_file_etc_hostname() {
        let args = serde_json::json!({"path": "/etc/hostname"});
        assert!(read_file(&args, &safe_cfg()).is_ok());
    }

    #[test]
    fn read_file_nonexistent_returns_error() {
        let args = serde_json::json!({"path": "/nonexistent_file_xyz"});
        assert!(read_file(&args, &safe_cfg()).is_err());
    }

    #[test]
    fn get_env_path_exists() {
        let args = serde_json::json!({"name": "PATH"});
        let result = get_env(&args, &safe_cfg()).unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn get_env_nonexistent() {
        let args = serde_json::json!({"name": "THIS_VAR_SHOULD_NOT_EXIST_XYZ"});
        assert!(get_env(&args, &safe_cfg()).is_err());
    }
}
