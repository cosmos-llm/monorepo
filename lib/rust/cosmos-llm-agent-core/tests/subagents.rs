//! Subagents (240): the `task` tool, tool subsets, policy inheritance,
//! cancellation, usage roll-up, events and transcripts.

mod common;

use std::path::Path;
use std::time::Duration;

use common::*;
use cosmos_llm::providers::mock::MockProvider;
use cosmos_llm_agent_core::policy::{ApprovalMode, PolicySpec};
use cosmos_llm_agent_core::session::SessionLog;
use cosmos_llm_agent_core::{
    AgentSet, AgentSpec, Core, CoreConfig, EvalSnapshot, Event, LoadOptions, Op, Rule,
    SubagentOptions,
};
use cosmos_llm_tool::{AgentEvent, Decision, StopReason};
use serde_json::{json, Value};

fn full_auto(cwd: &Path, sess: &Path) -> CoreConfig {
    let mut c = config(cwd, sess);
    c.permissions = PolicySpec::new(ApprovalMode::FullAuto);
    c
}

fn task(prompt: &str, agent: Option<&str>) -> Value {
    let mut v = json!({"description": "look around", "prompt": prompt});
    if let Some(a) = agent {
        v["agent"] = json!(a);
    }
    v
}

fn finished_results(ev: &[Event]) -> Vec<cosmos_llm_tool::ToolOutcome> {
    ev.iter()
        .filter_map(|e| match e {
            Event::Agent(AgentEvent::ToolFinished { result, .. }) => Some(result.clone()),
            _ => None,
        })
        .collect()
}

fn offers_tool(req: &cosmos_llm::CompletionRequest, name: &str) -> bool {
    serde_json::to_string(&req.tools)
        .unwrap()
        .contains(&format!("\"name\":\"{name}\""))
}

#[tokio::test]
async fn parent_gets_only_the_childs_final_text() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = MockProvider::new()
        .respond_with_tool_call("task", task("find the parser", None)) // parent
        .respond("The parser is in src/parse.rs.") // child
        .respond("done"); // parent
    let rec = completer(mock);
    let (h, mut rx) = Core::builder(full_auto(cwd.path(), sess.path()))
        .completer(rec.clone())
        .tools(registry())
        .start()
        .unwrap();
    h.user_text("where is the parser?").await.unwrap();
    let ev = until(&mut rx, run_finished).await;

    let results = finished_results(&ev);
    // The child's own tool results are inside Subagent events, not here.
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].as_text(), "The parser is in src/parse.rs.");

    let reqs = rec.requests();
    assert_eq!(reqs.len(), 3);
    // Parent request 3: user, assistant tool call, tool result. Nothing of the child's.
    let parent_last = &reqs[2];
    let non_system: Vec<_> = parent_last
        .messages
        .iter()
        .filter(|m| m.role != cosmos_llm::Role::System)
        .collect();
    assert_eq!(non_system.len(), 3, "{non_system:#?}");
    // The child saw its brief and not the parent's conversation.
    let child_req = &reqs[1];
    let texts: Vec<String> = child_req.messages.iter().map(|m| m.text()).collect();
    assert!(texts.iter().any(|t| t == "find the parser"), "{texts:?}");
    assert!(!texts.iter().any(|t| t.contains("where is the parser")));
    // General children have the parent's tools but not `task`.
    assert!(offers_tool(child_req, "echo"));
    assert!(!offers_tool(child_req, "task"));
    assert!(offers_tool(&reqs[0], "task"));

    // The child's events arrive wrapped.
    let sub: Vec<_> = ev
        .iter()
        .filter_map(|e| match e {
            Event::Subagent {
                call_id,
                agent,
                inner,
            } => Some((call_id, agent, inner)),
            _ => None,
        })
        .collect();
    assert!(!sub.is_empty());
    assert!(sub.iter().all(|(_, a, _)| a.as_str() == "general"));
    assert!(sub.iter().any(|(_, _, i)| matches!(
        i.as_ref(),
        AgentEvent::TextDelta { text } if text.contains("src/parse.rs")
    )));
    // The child's text is not in the parent's stream.
    assert!(!ev.iter().any(|e| matches!(
        e,
        Event::Agent(AgentEvent::TextDelta { text }) if text.contains("src/parse.rs")
    )));
}

#[tokio::test]
async fn usage_rolls_up_into_the_parents_run() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = MockProvider::new()
        .respond_with_tool_call("task", task("x", None))
        .respond("child answer")
        .respond("done");
    let (h, mut rx) = Core::builder(full_auto(cwd.path(), sess.path()))
        .completer(completer(mock))
        .tools(registry())
        .start()
        .unwrap();
    h.user_text("go").await.unwrap();
    let ev = until(&mut rx, run_finished).await;
    match ev.last().unwrap() {
        Event::RunFinished { usage, steps, .. } => {
            // Two parent turns plus the child's one.
            assert_eq!(*steps, 2);
            assert_eq!(usage.steps, 3);
            assert!(usage.total_tokens > 0);
        }
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn a_tool_outside_the_childs_subset_is_no_such_tool() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = MockProvider::new()
        .respond_with_tool_call("task", task("x", Some("explore")))
        .respond_with_tool_call("echo", json!({"text": "sneaky"})) // child asks for echo
        .respond("gave up") // child
        .respond("done");
    let (h, mut rx) = Core::builder(full_auto(cwd.path(), sess.path()))
        .completer(completer(mock))
        .tools(registry())
        .start()
        .unwrap();
    h.user_text("go").await.unwrap();
    let ev = until(&mut rx, run_finished).await;
    let child_results: Vec<_> = ev
        .iter()
        .filter_map(|e| match e {
            Event::Subagent { inner, .. } => match inner.as_ref() {
                AgentEvent::ToolFinished { result, .. } => Some(result.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(child_results.len(), 1);
    assert!(child_results[0].is_error);
    assert!(child_results[0].as_text().contains("No such tool: echo"));
}

#[tokio::test]
async fn unknown_agent_is_an_error_naming_the_choices() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = MockProvider::new()
        .respond_with_tool_call("task", task("x", Some("wizard")))
        .respond("ok");
    let (h, mut rx) = Core::builder(full_auto(cwd.path(), sess.path()))
        .completer(completer(mock))
        .tools(registry())
        .start()
        .unwrap();
    h.user_text("go").await.unwrap();
    let ev = until(&mut rx, run_finished).await;
    let r = &finished_results(&ev)[0];
    assert!(r.is_error);
    assert!(r.as_text().contains("unknown agent `wizard`") && r.as_text().contains("explore"));
}

#[tokio::test]
async fn depth_limit_controls_whether_children_get_task() {
    for (depth, child_has_task) in [(1, false), (2, true)] {
        let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mock = MockProvider::new()
            .respond_with_tool_call("task", task("x", None))
            .respond("c")
            .respond("done");
        let rec = completer(mock);
        let (h, mut rx) = Core::builder(full_auto(cwd.path(), sess.path()))
            .completer(rec.clone())
            .tools(registry())
            .subagents(SubagentOptions::depth(depth))
            .start()
            .unwrap();
        h.user_text("go").await.unwrap();
        until(&mut rx, run_finished).await;
        assert_eq!(
            offers_tool(&rec.requests()[1], "task"),
            child_has_task,
            "depth {depth}"
        );
    }

    // Depth 0 leaves the tool out altogether.
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let rec = completer(MockProvider::new().respond("hi"));
    let (h, mut rx) = Core::builder(full_auto(cwd.path(), sess.path()))
        .completer(rec.clone())
        .tools(registry())
        .subagents(SubagentOptions::depth(0))
        .start()
        .unwrap();
    h.user_text("go").await.unwrap();
    until(&mut rx, run_finished).await;
    assert!(!offers_tool(&rec.requests()[0], "task"));
}

#[tokio::test]
async fn a_grandchild_runs_when_depth_allows_and_events_nest() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = MockProvider::new()
        .respond_with_tool_call("task", task("outer", None)) // parent
        .respond_with_tool_call("task", task("inner", None)) // child
        .respond("grandchild says hi") // grandchild
        .respond("child done") // child
        .respond("parent done"); // parent
    let (h, mut rx) = Core::builder(full_auto(cwd.path(), sess.path()))
        .completer(completer(mock))
        .tools(registry())
        .subagents(SubagentOptions::depth(2))
        .start()
        .unwrap();
    h.user_text("go").await.unwrap();
    let ev = until(&mut rx, run_finished).await;
    assert_eq!(finished_results(&ev)[0].as_text(), "child done");
    let nested = ev.iter().any(|e| matches!(
        e,
        Event::Subagent { call_id, inner, .. }
            if call_id.contains('/')
                && matches!(inner.as_ref(), AgentEvent::TextDelta { text } if text.contains("grandchild"))
    ));
    assert!(nested, "grandchild events carry the path of task calls");
}

#[tokio::test]
async fn child_calls_use_the_parents_approval_stream() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = MockProvider::new()
        .respond_with_tool_call("task", task("x", None)) // parent (task is read-only: no prompt)
        .respond_with_tool_call("echo", json!({"text": "from child"})) // child: ask mode asks
        .respond("child done")
        .respond("parent done");
    let (h, mut rx) = Core::start(
        config(cwd.path(), sess.path()), // ask mode
        completer(mock),
        registry(),
    )
    .unwrap();
    h.user_text("go").await.unwrap();
    let ev = until(&mut rx, |e| matches!(e, Event::ApprovalRequested { .. })).await;
    let (call_id, tool) = match ev.last().unwrap() {
        Event::ApprovalRequested { call_id, tool, .. } => (call_id.clone(), tool.clone()),
        _ => unreachable!(),
    };
    assert_eq!(tool, "echo");
    assert!(
        call_id.contains('/'),
        "labelled with the task call: {call_id}"
    );
    h.approve(call_id, Decision::Allow).await.unwrap();
    let ev = until(&mut rx, run_finished).await;
    assert_eq!(finished_results(&ev)[0].as_text(), "child done");
}

#[tokio::test]
async fn a_child_cannot_exceed_the_parents_deny_rules() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut cfg = full_auto(cwd.path(), sess.path());
    cfg.permissions = PolicySpec::new(ApprovalMode::FullAuto).deny(Rule::tool("echo"));
    let mock = MockProvider::new()
        .respond_with_tool_call("task", task("x", None))
        .respond_with_tool_call("echo", json!({"text": "nope"}))
        .respond("child done")
        .respond("parent done");
    let (h, mut rx) = Core::builder(cfg)
        .completer(completer(mock))
        .tools(registry())
        .start()
        .unwrap();
    h.user_text("go").await.unwrap();
    let ev = until(&mut rx, run_finished).await;
    let denied = ev.iter().any(|e| match e {
        Event::Subagent { inner, .. } => matches!(
            inner.as_ref(),
            AgentEvent::ToolFinished { result, .. } if result.is_error
        ),
        _ => false,
    });
    assert!(denied);
    assert!(!ev
        .iter()
        .any(|e| matches!(e, Event::ApprovalRequested { .. })));
}

#[tokio::test]
async fn a_stricter_child_mode_holds_even_when_the_parent_is_full_auto() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut agents = AgentSet::new();
    let mut spec = AgentSpec::new("auditor", "Audit only.");
    spec.mode = Some(ApprovalMode::ReadOnly);
    agents.insert(spec);
    let mock = MockProvider::new()
        .respond_with_tool_call("task", task("x", Some("auditor")))
        .respond_with_tool_call("echo", json!({"text": "write"})) // echo is not read-only
        .respond("child done")
        .respond("parent done");
    let (h, mut rx) = Core::builder(full_auto(cwd.path(), sess.path()))
        .completer(completer(mock))
        .tools(registry())
        .agents(agents)
        .start()
        .unwrap();
    h.user_text("go").await.unwrap();
    let ev = until(&mut rx, run_finished).await;
    let msg = ev.iter().find_map(|e| match e {
        Event::Subagent { inner, .. } => match inner.as_ref() {
            AgentEvent::ToolFinished { result, .. } => Some(result.as_text()),
            _ => None,
        },
        _ => None,
    });
    assert!(
        msg.unwrap().contains("read-only"),
        "denied by the stricter mode"
    );
}

#[tokio::test]
async fn interrupting_the_parent_interrupts_the_child_and_histories_stay_valid() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = MockProvider::new()
        .respond_with_tool_call("task", task("wait", None))
        .respond_with_tool_call("slow", json!({})) // child blocks here
        .respond("after interrupt");
    let (h, mut rx) = Core::builder(full_auto(cwd.path(), sess.path()))
        .completer(completer(mock))
        .tools(registry())
        .start()
        .unwrap();
    h.user_text("go").await.unwrap();
    until(&mut rx, |e| {
        matches!(
            e,
            Event::Subagent { inner, .. } if matches!(inner.as_ref(), AgentEvent::ToolStarted { .. })
        )
    })
    .await;
    h.send(Op::Interrupt).await.unwrap();
    let ev = tokio::time::timeout(Duration::from_secs(10), until(&mut rx, run_finished))
        .await
        .expect("the interrupt must reach the child, whose tool sleeps for 30s");
    match ev.last().unwrap() {
        Event::RunFinished { reason, .. } => assert_eq!(*reason, Some(StopReason::Interrupted)),
        _ => unreachable!(),
    }
    // The child's tool was cancelled through its derived token.
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Subagent { inner, .. } if matches!(inner.as_ref(), AgentEvent::ToolFinished { .. })
    )));

    // The parent's history is valid (debug builds check before each request).
    h.user_text("again").await.unwrap();
    let ev = until(&mut rx, run_finished).await;
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::RunFinished {
            reason: Some(StopReason::Finished),
            ..
        }
    )));
}

#[tokio::test]
async fn the_childs_transcript_is_a_separate_session_linked_from_the_call() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = MockProvider::new()
        .respond_with_tool_call("task", task("check the logs", None))
        .respond("logs are clean")
        .respond("done");
    let (h, mut rx) = Core::builder(full_auto(cwd.path(), sess.path()))
        .completer(completer(mock))
        .tools(registry())
        .start()
        .unwrap();
    h.user_text("go").await.unwrap();
    let ev = until(&mut rx, run_finished).await;
    let display = ev
        .iter()
        .find_map(|e| match e {
            Event::Agent(AgentEvent::ToolFinished { result, .. }) => result.display.clone(),
            _ => None,
        })
        .expect("task sets a display");
    assert_eq!(display["type"], "subagent");
    assert_eq!(display["agent"], "general");
    let path = display["session_path"].as_str().expect("a transcript path");
    let loaded = SessionLog::load(Path::new(path)).unwrap();
    let texts: Vec<String> = loaded.thread.history().iter().map(|m| m.text()).collect();
    assert!(texts.iter().any(|t| t == "check the logs"));
    assert!(texts.iter().any(|t| t == "logs are clean"));
}

// --- definitions -----------------------------------------------------------

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn agents_load_from_trongo_families_and_markdown() {
    let (cwd, user, project) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    write(
        project.path(),
        "agents/explore.md",
        "Search the code and answer.\n",
    );
    write(
        project.path(),
        "agents/reviewers.trongo",
        r#"
(defn reviewer [lang]
  {:name        (str "review-" lang)
   :description (str "Reviews " lang " changes")
   :tools       [:read :glob :grep]
   :model       "claude-haiku-5-5"
   :max-steps   30
   :prompt      (str "You review " lang " code.")})

{:agents (concat [{:name "scout" :description "Scouts" :prompt (read-file "scout.txt")}]
                 (map reviewer ["rust" "ruby"]))}
"#,
    );
    write(project.path(), "agents/scout.txt", "Scout around.");
    write(
        project.path(),
        "agents/writer.md",
        "---\nname: writer\ndescription: Writes docs\ntools: Read, Write\nmodel: m2\nmax-steps: 9\n---\nWrite documentation.\n",
    );
    write(project.path(), "agents/broken.trongo", "{:name");

    let opts = LoadOptions::new(cwd.path())
        .user_dir(user.path())
        .project_dir(project.path())
        .snapshot(EvalSnapshot::empty());
    let (set, warnings) = AgentSet::load(&opts);
    assert_eq!(warnings.len(), 1, "{warnings:?}");

    let names: Vec<String> = set.list().into_iter().map(|a| a.name).collect();
    assert_eq!(
        names,
        [
            "explore",
            "general",
            "review-ruby",
            "review-rust",
            "scout",
            "writer"
        ]
    );
    // A project file replaces the built-in of the same name.
    assert_eq!(
        set.get("explore").unwrap().prompt,
        "Search the code and answer.\n"
    );
    let r = set.get("review-rust").unwrap();
    assert_eq!(
        r.tools.as_deref(),
        Some(&["read".to_owned(), "glob".into(), "grep".into()][..])
    );
    assert_eq!(r.model.as_deref(), Some("claude-haiku-5-5"));
    assert_eq!(r.max_steps, Some(30));
    assert_eq!(set.get("scout").unwrap().prompt, "Scout around.");
    let w = set.get("writer").unwrap();
    assert_eq!(
        w.tools.as_deref(),
        Some(&["Read".to_owned(), "Write".into()][..])
    );
    assert_eq!(w.max_steps, Some(9));
}

#[tokio::test]
async fn a_childs_model_comes_from_its_spec() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut agents = AgentSet::new();
    let mut spec = AgentSpec::new("cheap", "Be quick.");
    spec.model = Some("haiku-ish".into());
    agents.insert(spec);
    let mock = MockProvider::new()
        .respond_with_tool_call("task", task("x", Some("cheap")))
        .respond("c")
        .respond("done");
    let rec = completer(mock);
    let (h, mut rx) = Core::builder(full_auto(cwd.path(), sess.path()))
        .completer(rec.clone())
        .tools(registry())
        .agents(agents)
        .start()
        .unwrap();
    h.user_text("go").await.unwrap();
    until(&mut rx, run_finished).await;
    let models: Vec<String> = rec.requests().into_iter().map(|r| r.model).collect();
    assert_eq!(models, ["mock-model", "haiku-ish", "mock-model"]);
}
