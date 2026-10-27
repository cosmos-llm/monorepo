//! Golden-file round trips for the IRs, through JSON and TrongoScript.

use cosmos_llm_tool::{OutputLimit, Truncate};

fn expected() -> OutputLimit {
    OutputLimit::new()
        .max_bytes(12_000)
        .max_lines(500)
        .strategy(Truncate::HeadTail { head_ratio: 0.25 })
}

#[test]
fn output_limit_json_golden() {
    let src = include_str!("golden/output_limit.json");
    let ir: OutputLimit = serde_json::from_str(src).unwrap();
    assert_eq!(ir, expected());
    let back: OutputLimit = serde_json::from_str(&serde_json::to_string(&ir).unwrap()).unwrap();
    assert_eq!(back, ir);
}

#[cfg(feature = "trongo")]
#[test]
fn output_limit_trongo_golden() {
    use cosmos_llm_tool::trongo::{from_trongo, to_trongo};

    let src = include_str!("golden/output_limit.trongo");
    let ir: OutputLimit = from_trongo(src).unwrap();
    assert_eq!(ir, expected());
    // Dump is stable and re-evaluates to the same value.
    let dumped = to_trongo(&ir).unwrap();
    assert_eq!(from_trongo::<OutputLimit>(&dumped).unwrap(), ir);
    // And JSON and trongo goldens agree.
    let j: OutputLimit = serde_json::from_str(include_str!("golden/output_limit.json")).unwrap();
    assert_eq!(j, ir);
}

#[cfg(feature = "agent")]
mod loop_irs {
    use cosmos_llm::ContentBlock;
    use cosmos_llm_tool::parameter::ParameterDef;
    use cosmos_llm_tool::{
        AgentEvent, ApprovalRequest, ApprovalScope, Decision, OutputLimit, ParameterType,
        ThreadStatus, TokenUsage, ToolDefinition, ToolHints, ToolOutcome, ToolSpec, TurnStop,
        UserInput,
    };
    use serde::{de::DeserializeOwned, Serialize};
    use serde_json::json;
    use std::time::Duration;

    pub fn tool_spec() -> ToolSpec {
        ToolDefinition::new("read")
            .description("Read a file")
            .param("path", ParameterType::String, true, "File path")
            .push_param(
                ParameterDef::new("limit", ParameterType::Integer, false, "Max lines")
                    .with_default(json!(200)),
            )
            .read_only(true)
            .timeout(Duration::from_secs(5))
            .output_limit(OutputLimit::new().max_lines(500))
            .spec()
    }

    pub fn tool_outcome() -> ToolOutcome {
        ToolOutcome::text("edited src/lib.rs (+2 -1)")
            .with_display(json!({"path": "src/lib.rs", "added": 2, "removed": 1}))
    }

    pub fn approval_request() -> ApprovalRequest {
        let mut req = ApprovalRequest::new("call_1", "edit", json!({"file_path": "src/lib.rs"}));
        req.preview = Some(json!({"diff": "-old\n+new\n"}));
        req.hints = ToolHints::new().destructive(true).paths(["src/lib.rs"]);
        req
    }

    pub fn decisions() -> Vec<Decision> {
        vec![
            Decision::Allow,
            Decision::AllowAlways {
                scope: ApprovalScope::Tool {
                    name: "shell".into(),
                },
            },
            Decision::AllowAlways {
                scope: ApprovalScope::Path {
                    name: "edit".into(),
                    prefix: "src".into(),
                },
            },
            Decision::deny("not now"),
            Decision::Deny { reason: None },
            Decision::Abort,
        ]
    }

    pub fn events() -> Vec<AgentEvent> {
        let usage = TokenUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            steps: 1,
            ..TokenUsage::default()
        };
        vec![
            AgentEvent::TurnStarted { turn: 1 },
            AgentEvent::ThinkingDelta { text: "hmm".into() },
            AgentEvent::TextDelta {
                text: "Let me look.".into(),
            },
            AgentEvent::ToolCallStarted {
                call_id: "call_1".into(),
                name: "edit".into(),
            },
            AgentEvent::ToolCallArgsDelta {
                call_id: "call_1".into(),
                delta: "{\"file_path\":".into(),
            },
            AgentEvent::ToolCallReady {
                call_id: "call_1".into(),
                name: "edit".into(),
                input: json!({"file_path": "src/lib.rs"}),
            },
            AgentEvent::ApprovalRequested {
                request: approval_request(),
            },
            AgentEvent::ApprovalResolved {
                call_id: "call_1".into(),
                decision: Decision::Allow,
            },
            AgentEvent::ToolStarted {
                call_id: "call_1".into(),
            },
            AgentEvent::ToolOutputDelta {
                call_id: "call_1".into(),
                chunk: "compiling".into(),
            },
            AgentEvent::ToolFinished {
                call_id: "call_1".into(),
                result: tool_outcome(),
            },
            AgentEvent::InputDelivered {
                text: "also the docs".into(),
            },
            AgentEvent::Usage {
                turn: usage,
                total: usage,
            },
            AgentEvent::TurnFinished {
                turn: 1,
                stop: TurnStop::ToolUse,
            },
            AgentEvent::Error {
                message: "rate limited".into(),
                retryable: true,
            },
        ]
    }

    pub fn user_input() -> UserInput {
        UserInput::from_blocks(vec![
            ContentBlock::text("fix the build"),
            ContentBlock::text("it fails on CI"),
        ])
    }

    pub fn statuses() -> Vec<ThreadStatus> {
        vec![
            ThreadStatus::Idle,
            ThreadStatus::Running,
            ThreadStatus::AwaitingApproval,
        ]
    }

    #[cfg(feature = "trongo")]
    pub fn json_dump<T: Serialize>(v: &T) -> String {
        serde_json::to_string_pretty(v).unwrap() + "\n"
    }

    pub fn check<T>(name: &str, json_src: &str, trongo_src: &str, expected: &T)
    where
        T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let from_json: T = serde_json::from_str(json_src).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(&from_json, expected, "{name}.json");
        let back: T = serde_json::from_str(&serde_json::to_string(&from_json).unwrap()).unwrap();
        assert_eq!(&back, expected, "{name} json round trip");

        #[cfg(feature = "trongo")]
        {
            use cosmos_llm_tool::trongo::{from_trongo, to_trongo};
            let from_t: T = from_trongo(trongo_src).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(&from_t, expected, "{name}.trongo");
            let dumped = to_trongo(&from_t).unwrap();
            assert_eq!(&from_trongo::<T>(&dumped).unwrap(), expected, "{name} dump");
        }
        let _ = trongo_src;
    }
}

#[cfg(feature = "agent")]
mod loop_golden {
    use super::loop_irs::*;

    macro_rules! golden {
        ($test:ident, $file:literal, $expected:expr) => {
            #[test]
            fn $test() {
                check(
                    $file,
                    include_str!(concat!("golden/", $file, ".json")),
                    include_str!(concat!("golden/", $file, ".trongo")),
                    &$expected,
                );
            }
        };
    }

    golden!(tool_spec_golden, "tool_spec", tool_spec());
    golden!(tool_outcome_golden, "tool_outcome", tool_outcome());
    golden!(
        approval_request_golden,
        "approval_request",
        approval_request()
    );
    golden!(decision_golden, "decision", decisions());
    golden!(agent_event_golden, "agent_event", events());
    golden!(user_input_golden, "user_input", user_input());
    golden!(thread_status_golden, "thread_status", statuses());

    /// Regenerates the golden files: `UPDATE_GOLDEN=1 cargo test --all-features
    /// --test golden_ir regenerate`. Review the diff before committing.
    #[cfg(feature = "trongo")]
    #[test]
    fn regenerate() {
        if std::env::var_os("UPDATE_GOLDEN").is_none() {
            return;
        }
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");
        fn write<T: serde::Serialize>(dir: &str, name: &str, v: &T) {
            std::fs::write(format!("{dir}/{name}.json"), json_dump(v)).unwrap();
            std::fs::write(
                format!("{dir}/{name}.trongo"),
                cosmos_llm_tool::trongo::to_trongo(v).unwrap() + "\n",
            )
            .unwrap();
        }
        write(dir, "tool_spec", &tool_spec());
        write(dir, "tool_outcome", &tool_outcome());
        write(dir, "approval_request", &approval_request());
        write(dir, "decision", &decisions());
        write(dir, "agent_event", &events());
        write(dir, "user_input", &user_input());
        write(dir, "thread_status", &statuses());
    }
}
