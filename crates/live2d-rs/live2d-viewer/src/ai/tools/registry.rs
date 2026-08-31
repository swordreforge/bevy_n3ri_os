use std::collections::HashMap;

use serde_json::Value;

use super::executors::{exec_cmd, get_env, list_dir, read_file, read_proc};
use super::safety::{SafetyConfig, SafetyLevel};
use crate::ai::types::{ToolDefinition, ToolFunctionSpec};

/// A registered tool with its definition, executor, and safety classification.
struct ToolEntry {
    definition: ToolDefinition,
    executor: fn(&Value, &SafetyConfig) -> Result<String, String>,
    safety: SafetyLevel,
}

/// Registry of all available tools.
///
/// Tools are defined here (name, description, parameter schema, executor
/// function, safety level) and exposed to the API via `definitions()`.
pub struct ToolRegistry {
    tools: HashMap<&'static str, ToolEntry>,
}

impl ToolRegistry {
    /// Create the registry with all built-in tools.
    pub fn builtin() -> Self {
        let mut reg = Self {
            tools: HashMap::new(),
        };
        reg.register(
            "exec_cmd",
            "Execute a shell command and return its output. Only allows read-only system queries. Examples: ps aux, df -h, free -m, ls -la, pgrep -f name, cat /proc/cpuinfo",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "cmd": {"type": "string", "description": "Shell command to execute"},
                    "timeout_secs": {"type": "number", "description": "Timeout in seconds (default 5, max 30)"}
                },
                "required": ["cmd"]
            }),
            exec_cmd as fn(&Value, &SafetyConfig) -> Result<String, String>,
            SafetyLevel::Dangerous,
        );
        reg.register(
            "read_file",
            "Read a text file from the filesystem. Only readable files within allowed paths.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Absolute path to the file"},
                    "max_lines": {"type": "number", "description": "Maximum lines to read (optional)"}
                },
                "required": ["path"]
            }),
            read_file as fn(&Value, &SafetyConfig) -> Result<String, String>,
            SafetyLevel::Dangerous,
        );
        reg.register(
            "list_dir",
            "List entries in a directory.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Directory path"},
                    "max_entries": {"type": "number", "description": "Maximum entries to list (default 50)"}
                },
                "required": ["path"]
            }),
            list_dir as fn(&Value, &SafetyConfig) -> Result<String, String>,
            SafetyLevel::Dangerous,
        );
        reg.register(
            "read_proc",
            "Read /proc filesystem information (status, cmdline, maps) for a process. Safer than exec_cmd for process inspection.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pid": {"type": "number", "description": "Process ID (default: self)"},
                    "stat": {"type": "string", "description": "File to read: status, cmdline, maps, etc. (default: status)"}
                }
            }),
            read_proc as fn(&Value, &SafetyConfig) -> Result<String, String>,
            SafetyLevel::Safe,
        );
        reg.register(
            "get_env",
            "Get the value of an environment variable.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Environment variable name"}
                },
                "required": ["name"]
            }),
            get_env as fn(&Value, &SafetyConfig) -> Result<String, String>,
            SafetyLevel::Safe,
        );
        reg.register(
            "look_at_screen",
            "Capture a screenshot and analyze it. Use the 'prompt' parameter to ask a specific question about the screen (e.g. 'What app is open?', 'Is there a code editor visible?').",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "prompt": {"type": "string", "description": "What to ask about the screen. Default: describe what you see."}
                },
                "required": []
            }),
            noop_executor as fn(&Value, &SafetyConfig) -> Result<String, String>,
            SafetyLevel::Safe,
        );
        reg.register(
            "list_expressions",
            "List the Live2D facial expressions available on the current model.",
            serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
            noop_executor as fn(&Value, &SafetyConfig) -> Result<String, String>,
            SafetyLevel::Safe,
        );
        reg.register(
            "list_motions",
            "List the Live2D motion groups and motions available on the current model.",
            serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
            noop_executor as fn(&Value, &SafetyConfig) -> Result<String, String>,
            SafetyLevel::Safe,
        );
        reg.register(
            "play_expression",
            "Play a named facial expression on the model. Use list_expressions to discover names.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Exact expression name"}
                },
                "required": ["name"]
            }),
            noop_executor as fn(&Value, &SafetyConfig) -> Result<String, String>,
            SafetyLevel::Safe,
        );
        reg.register(
            "set_face_params",
            "Inject raw face parameter values directly (e.g. ParamMouthForm=1.0 for a big smile). Overrides any expression while active.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "params": {
                        "type": "array",
                        "description": "Parameter values to inject",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": {"type": "string", "description": "Parameter ID, e.g. ParamMouthForm"},
                                "value": {"type": "number", "description": "Target value within the parameter's range"}
                            },
                            "required": ["id", "value"]
                        }
                    }
                },
                "required": ["params"]
            }),
            noop_executor as fn(&Value, &SafetyConfig) -> Result<String, String>,
            SafetyLevel::Safe,
        );
        reg.register(
            "play_motion",
            "Trigger a motion by group and name. Use list_motions to discover groups/motions.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "group": {"type": "string", "description": "Motion group, e.g. Idle / Reactions / Effects"},
                    "motion": {"type": "string", "description": "Motion file name (substring match ok). Omit for the first motion in the group."}
                },
                "required": ["group"]
            }),
            noop_executor as fn(&Value, &SafetyConfig) -> Result<String, String>,
            SafetyLevel::Safe,
        );
        reg
    }

    fn register(
        &mut self,
        name: &'static str,
        description: &'static str,
        parameters: Value,
        executor: fn(&Value, &SafetyConfig) -> Result<String, String>,
        safety: SafetyLevel,
    ) {
        self.tools.insert(
            name,
            ToolEntry {
                definition: ToolDefinition {
                    type_: "function".to_string(),
                    function: ToolFunctionSpec {
                        name: name.to_string(),
                        description: description.to_string(),
                        parameters,
                    },
                },
                executor,
                safety,
            },
        );
    }

    /// Return the list of ToolDefinitions to send to the API.
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|e| e.definition.clone()).collect()
    }

    /// Look up and execute a tool by name.
    pub fn execute(
        &self,
        name: &str,
        args: &Value,
        safety: &SafetyConfig,
    ) -> Result<String, String> {
        let entry = self
            .tools
            .get(name)
            .ok_or_else(|| format!("unknown tool: '{name}'"))?;
        (entry.executor)(args, safety)
    }

    /// Get the safety level for a tool.
    pub fn safety_level(&self, name: &str) -> Option<SafetyLevel> {
        self.tools.get(name).map(|e| match e.safety {
            SafetyLevel::Safe => SafetyLevel::Safe,
            SafetyLevel::Dangerous => SafetyLevel::Dangerous,
        })
    }
}

fn noop_executor(_args: &Value, _safety: &SafetyConfig) -> Result<String, String> {
    Ok("look_at_screen handled by application".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_has_all_tools() {
        let reg = ToolRegistry::builtin();
        let defs = reg.definitions();
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert!(names.contains(&"exec_cmd"));
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"list_dir"));
        assert!(names.contains(&"read_proc"));
        assert!(names.contains(&"get_env"));
    }

    #[test]
    fn execute_unknown_tool_returns_error() {
        let reg = ToolRegistry::builtin();
        let safety = SafetyConfig {
            allowed_commands: vec![],
            allowed_read_paths: vec![],
            max_tool_rounds: 10,
            user_approved: false,
            working_dir: None,
        };
        let result = reg.execute("nonexistent", &serde_json::json!({}), &safety);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown tool"));
    }

    #[test]
    fn safety_level_returns_for_known_tool() {
        let reg = ToolRegistry::builtin();
        assert!(reg.safety_level("exec_cmd").is_some());
        assert!(reg.safety_level("read_proc").is_some());
    }

    #[test]
    fn safety_level_none_for_unknown() {
        let reg = ToolRegistry::builtin();
        assert!(reg.safety_level("nonexistent").is_none());
    }
}
