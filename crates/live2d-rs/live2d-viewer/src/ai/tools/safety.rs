use std::path::PathBuf;

/// Safety classification for a tool call.
pub enum SafetyLevel {
    /// Read-only, no side effects, auto-execute.
    Safe,
    /// May have side effects; requires user approval.
    Dangerous,
}

/// Runtime safety configuration for tool execution.
pub struct SafetyConfig {
    /// Shell commands allowed without user approval (empty = all need approval).
    pub allowed_commands: Vec<String>,
    /// Readable path prefixes (empty = no path restrictions enforced).
    pub allowed_read_paths: Vec<String>,
    /// Maximum tool call rounds per conversation turn.
    pub max_tool_rounds: u32,
    /// Whether the current tool call was explicitly approved by the user.
    /// When true, the command whitelist check is bypassed.
    pub user_approved: bool,
    /// Working directory for resolving relative paths in file tools.
    pub working_dir: Option<std::path::PathBuf>,
}

/// Check if the first token of `cmd` is in the allowed commands list.
///
/// An empty `allowed` list means no restrictions.
pub fn is_command_allowed(cmd: &str, allowed: &[String]) -> bool {
    if allowed.is_empty() {
        return false;
    }
    let first = cmd.split_whitespace().next().unwrap_or("");
    allowed.iter().any(|a| a == first)
}

/// Sanitize a path string:
///
/// - Expands `~` to the user's home directory.
/// - Resolves relative paths against `working_dir` when provided.
/// - Rejects paths containing `..` components.
/// - When `allowed_roots` is non-empty, rejects paths that don't start with
///   one of the allowed prefixes (after canonicalization).
pub fn sanitize_path(
    raw: &str,
    allowed_roots: &[String],
    working_dir: Option<&std::path::Path>,
) -> Result<PathBuf, String> {
    let expanded = if let Some(rest) = raw.strip_prefix('~') {
        let home = dirs::home_dir().ok_or_else(|| "cannot determine home directory".to_string())?;
        if rest.is_empty() || rest.starts_with('/') {
            PathBuf::from(format!("{}{}", home.display(), rest))
        } else {
            return Err("invalid path after ~".to_string());
        }
    } else {
        PathBuf::from(raw)
    };

    // Resolve relative paths against working_dir
    let resolved = if !expanded.is_absolute() {
        if let Some(wd) = working_dir {
            wd.join(&expanded)
        } else {
            expanded
        }
    } else {
        expanded
    };

    if resolved.components().any(|c| c.as_os_str() == "..") {
        return Err("path contains '..' which is not allowed".to_string());
    }

    if !allowed_roots.is_empty() {
        let canonical = resolved
            .canonicalize()
            .map_err(|e| format!("cannot resolve path: {e}"))?;
        let allowed = allowed_roots.iter().any(|root| canonical.starts_with(root));
        if !allowed {
            return Err(format!(
                "path '{}' is not in allowed read directories",
                canonical.display()
            ));
        }
    }

    Ok(resolved)
}

/// Truncate a string to at most `max_chars` characters, appending a
/// truncation notice when the input was longer.
pub fn truncate_result(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        format!("{}...(truncated, {} chars total)", &s[..max_chars], s.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_command_allowed_matches_first_token() {
        let allowed = vec!["ps".to_string(), "ls".to_string()];
        assert!(is_command_allowed("ps aux", &allowed));
        assert!(is_command_allowed("ls -la /tmp", &allowed));
        assert!(!is_command_allowed("rm -rf /", &allowed));
    }

    #[test]
    fn is_command_allowed_empty_list_denies_all() {
        let allowed: Vec<String> = vec![];
        assert!(!is_command_allowed("ps aux", &allowed));
    }

    #[test]
    fn sanitize_path_rejects_double_dot() {
        assert!(sanitize_path("/home/user/../../etc/passwd", &[], None).is_err());
    }

    #[test]
    fn sanitize_path_expands_tilde() {
        let result = sanitize_path("~/test", &[], None).unwrap();
        assert!(result.starts_with(&dirs::home_dir().unwrap()));
    }

    #[test]
    fn sanitize_path_rejects_outside_allowed_roots() {
        let allowed = vec!["/home/user".to_string()];
        let bad = sanitize_path("/tmp/foo", &allowed, None);
        assert!(bad.is_err());
    }

    #[test]
    fn truncate_result_short_string_stays() {
        assert_eq!(truncate_result("hello", 10), "hello");
    }

    #[test]
    fn truncate_result_long_string_gets_cut() {
        let long = "a".repeat(100);
        let truncated = truncate_result(&long, 10);
        assert!(truncated.starts_with("aaaaaaaaaa"));
        assert!(truncated.contains("truncated"));
    }
}
