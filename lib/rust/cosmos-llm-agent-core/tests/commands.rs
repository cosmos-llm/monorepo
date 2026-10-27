//! Custom commands (230): loading, layering, expansion, `Op::RunCommand`.

mod common;

use std::path::Path;
use std::sync::Arc;

use common::*;
use cosmos_llm::providers::mock::MockProvider;
use cosmos_llm_agent_core::policy::{ApprovalMode, PolicyEngine, PolicySpec};
use cosmos_llm_agent_core::{
    CommandSet, CommandSpec, Core, EvalSnapshot, Event, Expansion, LoadOptions, Op,
};
use cosmos_llm_tool::AgentEvent;

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn opts(cwd: &Path, user: &Path, project: &Path) -> LoadOptions {
    LoadOptions::new(cwd)
        .user_dir(user)
        .project_dir(project)
        .snapshot(EvalSnapshot::empty())
}

#[test]
fn both_front_ends_lower_into_one_spec_and_project_wins() {
    let (cwd, user, project) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    write(
        user.path(),
        "commands/review.trongo",
        r#"{:description "User review" :prompt "user prompt"}"#,
    );
    write(
        user.path(),
        "commands/only-user.md",
        "---\ndescription: From markdown\nargument-hint: \"[x]\"\nallowed-tools: Read, Grep\nmodel: m1\n---\nBody $ARGUMENTS\n",
    );
    write(
        project.path(),
        "commands/review.trongo",
        r#"{:description "Project review"
           :args "[focus]"
           :allowed-tools [:read :grep]
           :prompt (read-file "review.prompt")}"#,
    );
    write(
        project.path(),
        "commands/review.prompt",
        "Review focusing on $ARGUMENTS",
    );
    write(
        project.path(),
        "commands/db/migrate.md",
        "Run the migration\n",
    );

    let (set, warnings) = CommandSet::load(&opts(cwd.path(), user.path(), project.path()));
    assert!(warnings.is_empty(), "{warnings:?}");
    let names: Vec<String> = set.list().into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["db:migrate", "only-user", "review"]);

    let review = set.get("/review").unwrap();
    assert_eq!(review.description, "Project review");
    assert_eq!(review.args.as_deref(), Some("[focus]"));
    assert_eq!(review.allowed_tools, ["read", "grep"]);
    assert_eq!(review.prompt, "Review focusing on $ARGUMENTS");

    let md = set.get("only-user").unwrap();
    assert_eq!(md.args.as_deref(), Some("[x]"));
    assert_eq!(md.allowed_tools, ["Read", "Grep"]);
    assert_eq!(md.model.as_deref(), Some("m1"));
    assert_eq!(md.prompt, "Body $ARGUMENTS\n");
}

#[test]
fn a_broken_file_is_a_warning_not_a_failure() {
    let (cwd, user, project) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    write(project.path(), "commands/bad.trongo", "{:prompt (");
    write(
        project.path(),
        "commands/empty.md",
        "---\ndescription: x\n---\n",
    );
    write(project.path(), "commands/good.md", "ok");
    let (set, warnings) = CommandSet::load(&opts(cwd.path(), user.path(), project.path()));
    assert_eq!(set.len(), 1);
    assert_eq!(warnings.len(), 2, "{warnings:?}");
}

#[tokio::test]
async fn expansion_substitutes_arguments_and_inlines_files() {
    let cwd = tempfile::tempdir().unwrap();
    write(cwd.path(), "src/lib.rs", "fn main() {}\n");
    write(cwd.path(), "notes.txt", "n");
    let spec = CommandSpec::new(
        "explain",
        "Explain @src/lib.rs, focus: $ARGUMENTS ($1 and $2). Ping @nobody and a@b.\n",
    );
    let out = spec
        .expand("speed \"error paths\"", &Expansion::new(cwd.path()))
        .await;
    assert!(out
        .text
        .contains("<file path=\"src/lib.rs\">\nfn main() {}\n\n</file>"));
    assert!(out
        .text
        .contains("focus: speed \"error paths\" (speed and error paths)."));
    assert!(out.text.contains("Ping @nobody and a@b."));
    assert!(out.warnings.is_empty());

    // A file outside the workspace is not inlined.
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "secret", "s3cret");
    let link = cwd.path().join("link");
    std::os::unix::fs::symlink(outside.path().join("secret"), &link).unwrap();
    let out = CommandSpec::new("x", "look @link")
        .expand("", &Expansion::new(cwd.path()))
        .await;
    assert!(!out.text.contains("s3cret"));
    assert_eq!(out.warnings.len(), 1);
}

#[tokio::test]
async fn shell_inlining_obeys_the_policy() {
    let cwd = tempfile::tempdir().unwrap();
    let engine = |mode| Arc::new(PolicyEngine::new(PolicySpec::new(mode), cwd.path()));
    std::fs::write(cwd.path().join("note"), "hello").unwrap();
    let spec = CommandSpec::new("diff", "out: !`cat $ARGUMENTS`\n");

    // Known-safe, so allowed even in ask mode.
    let env = Expansion::new(cwd.path()).engine(engine(ApprovalMode::Ask));
    let out = spec.expand("note", &env).await;
    assert_eq!(out.text, "out: hello\n");

    // Not known-safe: asking is not possible mid-expansion, so it is skipped.
    let spec = CommandSpec::new("x", "!`touch made`");
    let out = spec.expand("", &env).await;
    assert!(out.text.contains("command not run"), "{}", out.text);
    assert!(!cwd.path().join("made").exists());
    assert_eq!(out.warnings.len(), 1);

    // Full-auto runs it; with no engine at all nothing runs.
    let full = Expansion::new(cwd.path()).engine(engine(ApprovalMode::FullAuto));
    spec.expand("", &full).await;
    assert!(cwd.path().join("made").exists());
    std::fs::remove_file(cwd.path().join("made")).unwrap();
    spec.expand("", &Expansion::new(cwd.path())).await;
    assert!(!cwd.path().join("made").exists());
}

fn last_user_text(mock_req: &cosmos_llm::CompletionRequest) -> String {
    mock_req
        .messages
        .iter()
        .rev()
        .find(|m| m.role == cosmos_llm::Role::User)
        .map(|m| m.text())
        .unwrap_or_default()
}

#[tokio::test]
async fn run_command_sends_the_expanded_prompt() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    write(cwd.path(), "a.txt", "alpha");
    let mut set = CommandSet::new();
    set.insert(CommandSpec::new("look", "Check @a.txt for $1"));
    let rec = completer(MockProvider::new().respond("looked"));
    let (h, mut rx) = Core::builder(config(cwd.path(), sess.path()))
        .completer(rec.clone())
        .tools(registry())
        .commands(set)
        .start()
        .unwrap();
    assert_eq!(h.list_commands()[0].name, "look");

    h.send(Op::RunCommand {
        name: "/look".into(),
        args: "typos".into(),
    })
    .await
    .unwrap();
    let ev = until(&mut rx, run_finished).await;
    assert!(ev
        .iter()
        .any(|e| matches!(e, Event::Agent(AgentEvent::TextDelta { text }) if text == "looked")));
    let sent = last_user_text(&rec.last_request().unwrap());
    assert!(sent.contains("Check "), "{sent}");
    assert!(
        sent.contains("<file path=\"a.txt\">\nalpha\n</file>"),
        "{sent}"
    );
    assert!(sent.contains("for typos"), "{sent}");
}

#[tokio::test]
async fn unknown_command_is_an_error_event() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (h, mut rx) = Core::start(
        config(cwd.path(), sess.path()),
        completer(MockProvider::new().respond("x")),
        registry(),
    )
    .unwrap();
    h.send(Op::RunCommand {
        name: "nope".into(),
        args: String::new(),
    })
    .await
    .unwrap();
    let ev = until(&mut rx, |e| matches!(e, Event::Error { .. })).await;
    assert!(
        matches!(ev.last(), Some(Event::Error { message, fatal: false }) if message.contains("nope"))
    );
}

#[tokio::test]
async fn a_command_can_name_its_model_for_one_turn() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut set = CommandSet::new();
    let mut spec = CommandSpec::new("cheap", "go");
    spec.model = Some("small-model".into());
    set.insert(spec);
    let rec = completer(MockProvider::new().respond("a").respond("b"));
    let (h, mut rx) = Core::builder(config(cwd.path(), sess.path()))
        .completer(rec.clone())
        .tools(registry())
        .commands(set)
        .start()
        .unwrap();
    h.send(Op::RunCommand {
        name: "cheap".into(),
        args: String::new(),
    })
    .await
    .unwrap();
    until(&mut rx, run_finished).await;
    h.user_text("plain").await.unwrap();
    until(&mut rx, run_finished).await;
    let models: Vec<String> = rec.requests().into_iter().map(|r| r.model).collect();
    assert_eq!(models, ["small-model", "mock-model"]);
}
