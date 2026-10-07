use serde_json::{Map, Value};

use crate::event::{AgentEvent, AgentEventKind, EventAdapter};
use crate::tmux::LAIN_AGENT;
use crate::tool_name::CanonicalTool;

use super::{HookRegistration, json_str, json_value_or_null, optional_str};

pub struct LainAdapter;

impl LainAdapter {
    /// lain has no hook config file; the Lua bridge in
    /// `.lain/tmux_agent_sidebar.lua` forwards its autocmd events. Triggers
    /// name the lain autocmd that produces each kind, and this table is
    /// documentation plus the drift guard; setup does not consume it.
    /// No SessionEnd: lain fires no exit event, so the poller's
    /// shell-fallback sweep owns teardown.
    pub const HOOK_REGISTRATIONS: &'static [HookRegistration] = &[
        HookRegistration {
            trigger: "SessionFocusChanged",
            matcher: None,
            kind: AgentEventKind::SessionStart,
        },
        HookRegistration {
            trigger: "TurnStart",
            matcher: None,
            kind: AgentEventKind::UserPromptSubmit,
        },
        HookRegistration {
            trigger: "SessionStatusChanged",
            matcher: Some("needs_input"),
            kind: AgentEventKind::Notification,
        },
        HookRegistration {
            trigger: "TurnEnd",
            matcher: None,
            kind: AgentEventKind::Stop,
        },
        HookRegistration {
            trigger: "TurnError",
            matcher: None,
            kind: AgentEventKind::StopFailure,
        },
        HookRegistration {
            trigger: "ToolDone",
            matcher: None,
            kind: AgentEventKind::ActivityLog,
        },
    ];
}

/// Map lain's lowercase tool ids onto the Claude-style vocabulary the label
/// extractor and color table key off.
fn normalize_tool_name(raw: &str) -> String {
    let canonical = match raw {
        "bash" => CanonicalTool::Bash,
        // `index` is a structural read of one file, closest to Read.
        "read" | "index" => CanonicalTool::Read,
        "write" => CanonicalTool::Write,
        "edit" | "multiedit" => CanonicalTool::Edit,
        "glob" => CanonicalTool::Glob,
        "grep" => CanonicalTool::Grep,
        "webfetch" => CanonicalTool::WebFetch,
        "websearch" => CanonicalTool::WebSearch,
        "task" => CanonicalTool::Agent,
        "skill" => CanonicalTool::Skill,
        "question" => CanonicalTool::AskUserQuestion,
        other => return other.to_string(),
    };
    canonical.as_str().to_string()
}

/// lain names its file argument `path` and its skill argument `name`; copy
/// them to the keys the label extractor reads, keeping the originals.
fn normalize_tool_input(tool_name: &str, input: Value) -> Value {
    let Value::Object(mut map) = input else {
        return input;
    };
    let rewrites: &[(&str, &str)] = match tool_name {
        "Read" | "Write" | "Edit" => &[("path", "file_path")],
        "Skill" => &[("name", "skill")],
        _ => &[],
    };
    copy_keys(&mut map, rewrites);
    Value::Object(map)
}

fn copy_keys(map: &mut Map<String, Value>, pairs: &[(&str, &str)]) {
    for (src, dst) in pairs {
        if map.contains_key(*dst) {
            continue;
        }
        if let Some(value) = map.get(*src).cloned() {
            map.insert((*dst).to_string(), value);
        }
    }
}

impl EventAdapter for LainAdapter {
    fn parse(&self, event_name: &str, input: &Value) -> Option<AgentEvent> {
        match event_name {
            "session-start" => Some(AgentEvent::SessionStart {
                agent: LAIN_AGENT.into(),
                cwd: json_str(input, "cwd").into(),
                permission_mode: String::new(),
                source: json_str(input, "source").into(),
                worktree: None,
                agent_id: None,
                session_id: optional_str(input, "session_id"),
            }),
            "user-prompt-submit" => Some(AgentEvent::UserPromptSubmit {
                agent: LAIN_AGENT.into(),
                cwd: json_str(input, "cwd").into(),
                permission_mode: String::new(),
                prompt: json_str(input, "prompt").into(),
                worktree: None,
                agent_id: None,
                session_id: optional_str(input, "session_id"),
            }),
            "notification" => Some(AgentEvent::Notification {
                agent: LAIN_AGENT.into(),
                cwd: json_str(input, "cwd").into(),
                permission_mode: String::new(),
                wait_reason: json_str(input, "wait_reason").into(),
                meta_only: false,
                worktree: None,
                agent_id: None,
                session_id: optional_str(input, "session_id"),
            }),
            "stop" => Some(AgentEvent::Stop {
                agent: LAIN_AGENT.into(),
                cwd: json_str(input, "cwd").into(),
                permission_mode: String::new(),
                last_message: json_str(input, "last_message").into(),
                response: None,
                worktree: None,
                agent_id: None,
                session_id: optional_str(input, "session_id"),
            }),
            "stop-failure" => Some(AgentEvent::StopFailure {
                agent: LAIN_AGENT.into(),
                cwd: json_str(input, "cwd").into(),
                permission_mode: String::new(),
                error: json_str(input, "error").into(),
                worktree: None,
                agent_id: None,
                session_id: optional_str(input, "session_id"),
            }),
            "activity-log" => {
                let raw_name = json_str(input, "tool_name");
                if raw_name.is_empty() {
                    return None;
                }
                let tool_name = normalize_tool_name(raw_name);
                let tool_input =
                    normalize_tool_input(&tool_name, json_value_or_null(input, "tool_input"));
                Some(AgentEvent::ActivityLog {
                    tool_name,
                    tool_input,
                    tool_response: json_value_or_null(input, "tool_response"),
                })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn activity(input: Value) -> (String, Value) {
        match LainAdapter.parse("activity-log", &input).unwrap() {
            AgentEvent::ActivityLog {
                tool_name,
                tool_input,
                ..
            } => (tool_name, tool_input),
            other => panic!("expected ActivityLog, got {:?}", other),
        }
    }

    #[test]
    fn hook_registrations_match_parse_arms() {
        super::super::assert_table_drift_free("lain", LainAdapter::HOOK_REGISTRATIONS);
    }

    #[test]
    fn session_start() {
        let event = LainAdapter
            .parse(
                "session-start",
                &json!({"cwd": "/tmp", "session_id": "ses-1", "source": "startup"}),
            )
            .unwrap();
        assert_eq!(
            event,
            AgentEvent::SessionStart {
                agent: LAIN_AGENT.into(),
                cwd: "/tmp".into(),
                permission_mode: "".into(),
                source: "startup".into(),
                worktree: None,
                agent_id: None,
                session_id: Some("ses-1".into()),
            }
        );
    }

    #[test]
    fn session_end_not_supported() {
        assert!(LainAdapter.parse("session-end", &json!({})).is_none());
    }

    #[test]
    fn user_prompt_submit() {
        let event = LainAdapter
            .parse(
                "user-prompt-submit",
                &json!({"cwd": "/tmp", "prompt": "hello"}),
            )
            .unwrap();
        assert_eq!(
            event,
            AgentEvent::UserPromptSubmit {
                agent: LAIN_AGENT.into(),
                cwd: "/tmp".into(),
                permission_mode: "".into(),
                prompt: "hello".into(),
                worktree: None,
                agent_id: None,
                session_id: None,
            }
        );
    }

    #[test]
    fn notification_carries_wait_reason() {
        let event = LainAdapter
            .parse(
                "notification",
                &json!({"cwd": "/tmp", "wait_reason": "permission"}),
            )
            .unwrap();
        match event {
            AgentEvent::Notification {
                agent,
                wait_reason,
                meta_only,
                ..
            } => {
                assert_eq!(agent, LAIN_AGENT);
                assert_eq!(wait_reason, "permission");
                assert!(!meta_only);
            }
            other => panic!("expected Notification, got {:?}", other),
        }
    }

    #[test]
    fn stop_carries_last_message() {
        let event = LainAdapter
            .parse("stop", &json!({"cwd": "/tmp", "last_message": "done"}))
            .unwrap();
        match event {
            AgentEvent::Stop {
                agent,
                last_message,
                response,
                ..
            } => {
                assert_eq!(agent, LAIN_AGENT);
                assert_eq!(last_message, "done");
                assert!(response.is_none());
            }
            other => panic!("expected Stop, got {:?}", other),
        }
    }

    #[test]
    fn stop_failure() {
        let event = LainAdapter
            .parse("stop-failure", &json!({"cwd": "/tmp", "error": "boom"}))
            .unwrap();
        match event {
            AgentEvent::StopFailure { agent, error, .. } => {
                assert_eq!(agent, LAIN_AGENT);
                assert_eq!(error, "boom");
            }
            other => panic!("expected StopFailure, got {:?}", other),
        }
    }

    #[test]
    fn activity_log_requires_tool_name() {
        assert!(LainAdapter.parse("activity-log", &json!({})).is_none());
    }

    #[test]
    fn activity_log_maps_lowercase_tools_to_canonical() {
        for (raw, canonical) in [
            ("bash", "Bash"),
            ("read", "Read"),
            ("index", "Read"),
            ("write", "Write"),
            ("edit", "Edit"),
            ("multiedit", "Edit"),
            ("glob", "Glob"),
            ("grep", "Grep"),
            ("webfetch", "WebFetch"),
            ("websearch", "WebSearch"),
            ("task", "Agent"),
            ("skill", "Skill"),
            ("question", "AskUserQuestion"),
        ] {
            let (name, _) = activity(json!({"tool_name": raw}));
            assert_eq!(name, canonical, "{raw}");
        }
    }

    #[test]
    fn activity_log_unknown_tool_passes_through() {
        let (name, _) = activity(json!({"tool_name": "code_execution"}));
        assert_eq!(name, "code_execution");
    }

    #[test]
    fn activity_log_copies_path_to_file_path_for_file_tools() {
        for raw in ["read", "edit", "multiedit", "write", "index"] {
            let (_, input) = activity(json!({
                "tool_name": raw,
                "tool_input": {"path": "/repo/src/main.rs"}
            }));
            assert_eq!(input["file_path"], "/repo/src/main.rs", "{raw}");
            assert_eq!(input["path"], "/repo/src/main.rs", "{raw}");
        }
    }

    #[test]
    fn activity_log_copies_skill_name_to_skill() {
        let (_, input) = activity(json!({
            "tool_name": "skill",
            "tool_input": {"name": "commit"}
        }));
        assert_eq!(input["skill"], "commit");
    }

    #[test]
    fn activity_log_does_not_overwrite_existing_file_path() {
        let (_, input) = activity(json!({
            "tool_name": "read",
            "tool_input": {"path": "/a", "file_path": "/b"}
        }));
        assert_eq!(input["file_path"], "/b");
    }

    #[test]
    fn activity_log_keeps_response() {
        match LainAdapter
            .parse(
                "activity-log",
                &json!({"tool_name": "bash", "tool_response": {"output": "ok"}}),
            )
            .unwrap()
        {
            AgentEvent::ActivityLog { tool_response, .. } => {
                assert_eq!(tool_response["output"], "ok");
            }
            other => panic!("expected ActivityLog, got {:?}", other),
        }
    }
}
