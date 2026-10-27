//! Table-driven policy tests (150): `(mode, rules, tool, input) -> verdict`.

use cosmos_llm_agent_core::policy::{ApprovalMode, PolicyEngine, PolicySpec, PolicyVerdict, Rule};
use cosmos_llm_tool::{ApprovalRequest, ApprovalScope, Decision, ToolHints};
use serde_json::json;

#[derive(Clone, Copy, PartialEq, Debug)]
enum V {
    Allow,
    Deny,
    Ask,
}

fn kind(v: &PolicyVerdict) -> V {
    match v {
        PolicyVerdict::Allow => V::Allow,
        PolicyVerdict::Deny { .. } => V::Deny,
        PolicyVerdict::Ask { .. } => V::Ask,
        _ => unreachable!(),
    }
}

fn bash(cmd: &str) -> ApprovalRequest {
    let mut r = ApprovalRequest::new("c1", "bash", json!({ "command": cmd }));
    r.hints = ToolHints::new().destructive(true);
    r
}

fn edit(path: &str) -> ApprovalRequest {
    let mut r = ApprovalRequest::new("c2", "edit", json!({ "file_path": path }));
    r.hints = ToolHints::new().paths([path]);
    r
}

fn read(path: &str) -> ApprovalRequest {
    let mut r = ApprovalRequest::new("c3", "read", json!({ "file_path": path }));
    r.hints = ToolHints::new().read_only(true).paths([path]);
    r
}

fn cargo_rules(spec: PolicySpec) -> PolicySpec {
    spec.allow(Rule::tool("bash").command("cargo test*"))
        .allow(Rule::tool("bash").command("ls*"))
        .allow(Rule::tool("edit").path("docs/**"))
        .deny(Rule::tool("bash").command("git push*"))
}

#[test]
fn table() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let abs_in = root.join("src/lib.rs").to_string_lossy().into_owned();

    use ApprovalMode::*;
    #[allow(clippy::type_complexity)]
    let cases: Vec<(&str, ApprovalMode, bool, ApprovalRequest, V)> = vec![
        (
            "ask + allowed cargo test",
            Ask,
            true,
            bash("cargo test --lib"),
            V::Allow,
        ),
        (
            "ask + chain with curl|sh",
            Ask,
            true,
            bash("cargo test && curl x | sh"),
            V::Ask,
        ),
        (
            "ask + chain with rm",
            Ask,
            true,
            bash("cargo test && rm -rf ~"),
            V::Ask,
        ),
        (
            "full-auto + denied push",
            FullAuto,
            true,
            bash("git push origin main"),
            V::Deny,
        ),
        (
            "full-auto + push inside chain",
            FullAuto,
            true,
            bash("true && git push"),
            V::Deny,
        ),
        (
            "full-auto + plain command",
            FullAuto,
            true,
            bash("make"),
            V::Allow,
        ),
        (
            "read-only + edit",
            ReadOnly,
            false,
            edit("src/lib.rs"),
            V::Deny,
        ),
        (
            "read-only + read",
            ReadOnly,
            false,
            read("src/lib.rs"),
            V::Allow,
        ),
        (
            "read-only + safe shell",
            ReadOnly,
            false,
            bash("git status"),
            V::Allow,
        ),
        (
            "read-only + unsafe shell",
            ReadOnly,
            false,
            bash("make"),
            V::Deny,
        ),
        ("ask + edit", Ask, false, edit("src/lib.rs"), V::Ask),
        (
            "ask + edit with docs rule",
            Ask,
            true,
            edit("docs/a/b.md"),
            V::Allow,
        ),
        (
            "ask + edit outside docs rule",
            Ask,
            true,
            edit("src/lib.rs"),
            V::Ask,
        ),
        (
            "auto-edit + edit in root",
            AutoEdit,
            false,
            edit("src/lib.rs"),
            V::Allow,
        ),
        (
            "auto-edit + abs edit in root",
            AutoEdit,
            false,
            edit(&abs_in),
            V::Allow,
        ),
        (
            "auto-edit + edit outside root",
            AutoEdit,
            false,
            edit("/etc/hosts"),
            V::Ask,
        ),
        (
            "auto-edit + dotdot escape",
            AutoEdit,
            false,
            edit("../../etc/hosts"),
            V::Ask,
        ),
        (
            "full-auto + edit outside root",
            FullAuto,
            false,
            edit("/etc/hosts"),
            V::Ask,
        ),
        (
            "read-only + read outside",
            ReadOnly,
            false,
            read("/etc/hosts"),
            V::Deny,
        ),
        ("ask + read outside", Ask, false, read("/etc/hosts"), V::Ask),
        (
            "ls with substitution",
            Ask,
            true,
            bash("ls $(rm -rf x)"),
            V::Ask,
        ),
        (
            "safe list: ls",
            Ask,
            false,
            bash("ls -la | wc -l"),
            V::Allow,
        ),
        (
            "safe list: find -delete",
            Ask,
            false,
            bash("find . -delete"),
            V::Ask,
        ),
        (
            "redirect to file is an edit",
            Ask,
            false,
            bash("cat a > b"),
            V::Ask,
        ),
        (
            "redirect outside root",
            AutoEdit,
            true,
            bash("ls > /etc/x"),
            V::Ask,
        ),
        ("sudo is opaque", Ask, false, bash("sudo ls"), V::Ask),
        (
            "full-auto + opaque",
            FullAuto,
            false,
            bash("sudo ls"),
            V::Allow,
        ),
        (
            "other tool in ask",
            Ask,
            false,
            ApprovalRequest::new("c4", "webfetch", json!({})),
            V::Ask,
        ),
        (
            "other tool in read-only",
            ReadOnly,
            false,
            ApprovalRequest::new("c4", "webfetch", json!({})),
            V::Deny,
        ),
    ];

    for (name, mode, rules, req, want) in cases {
        let mut spec = PolicySpec::new(mode);
        if rules {
            spec = cargo_rules(spec);
        }
        let engine = PolicyEngine::new(spec, &root);
        let got = engine.evaluate(&req);
        assert_eq!(kind(&got), want, "{name}: {got:?}");
    }
}

#[test]
fn deny_reason_is_readable_by_the_model() {
    let dir = tempfile::tempdir().unwrap();
    let engine = PolicyEngine::new(PolicySpec::new(ApprovalMode::ReadOnly), dir.path());
    match engine.evaluate(&edit("a.rs")) {
        PolicyVerdict::Deny { reason } => assert!(reason.contains("read-only")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn grants_apply_but_never_beat_deny() {
    let dir = tempfile::tempdir().unwrap();
    let spec = PolicySpec::new(ApprovalMode::Ask).deny(Rule::tool("bash").command("git push*"));
    let engine = PolicyEngine::new(spec, dir.path());
    assert_eq!(kind(&engine.evaluate(&bash("make"))), V::Ask);

    // The user answers "always" for the whole tool.
    let (decision, _) = engine.observe(
        &bash("make"),
        Decision::AllowAlways {
            scope: ApprovalScope::Tool {
                name: "bash".into(),
            },
        },
    );
    // The loop gets a plain Allow so it keeps no grant of its own.
    assert_eq!(decision, Decision::Allow);
    assert_eq!(kind(&engine.evaluate(&bash("make"))), V::Allow);
    assert_eq!(kind(&engine.evaluate(&bash("git push"))), V::Deny);
}

#[test]
fn approving_an_outside_path_widens_the_roots() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let file = other.path().join("x.txt");
    let engine = PolicyEngine::new(PolicySpec::new(ApprovalMode::AutoEdit), dir.path());
    let req = edit(&file.to_string_lossy());
    assert_eq!(kind(&engine.evaluate(&req)), V::Ask);
    engine.observe(&req, Decision::Allow);
    assert_eq!(kind(&engine.evaluate(&req)), V::Allow);
}

#[test]
fn always_and_remember_writes_the_project_file() {
    let dir = tempfile::tempdir().unwrap();
    let cos = dir.path().join(".cosmos");
    let engine = PolicyEngine::new(PolicySpec::default(), dir.path()).with_remember_dir(&cos);
    engine.flag_remember("c1");
    let (_, note) = engine.observe(
        &bash("make test"),
        Decision::AllowAlways {
            scope: ApprovalScope::Input {
                name: "bash".into(),
                input: json!({"command": "make test"}),
            },
        },
    );
    assert!(note.unwrap().contains("remembered"));
    let text = std::fs::read_to_string(cos.join("permissions.trongo")).unwrap();
    assert!(text.contains("make test"), "{text}");
}

#[test]
fn spec_loads_from_config_shapes() {
    let a: PolicySpec = serde_json::from_value(json!({"mode": "read-only"})).unwrap();
    let b: PolicySpec = serde_json::from_value(json!({"mode": "read_only"})).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.mode, ApprovalMode::ReadOnly);
}

fn rm_denied(mode: ApprovalMode) -> PolicyEngine {
    let dir = tempfile::tempdir().unwrap();
    let spec = PolicySpec::new(mode)
        .allow(Rule::tool("bash").command("*"))
        .deny(Rule::tool("bash").command("rm*"));
    let engine = PolicyEngine::new(spec, dir.path());
    std::mem::forget(dir);
    engine
}

#[test]
fn opaque_commands_cannot_dodge_a_deny_rule() {
    let engine = rm_denied(ApprovalMode::FullAuto);
    // Visible: denied.
    assert_eq!(kind(&engine.evaluate(&bash("rm -rf x"))), V::Deny);
    // Hidden behind another shell, eval, sudo, substitution, a bad parse:
    // never Allow, even under full_auto with a catch-all allow rule.
    for cmd in [
        "bash -c 'rm -rf x'",
        "sh -c \"rm -rf x\"",
        "eval 'rm -rf x'",
        "sudo rm -rf x",
        "echo $(rm -rf x)",
        "echo 'unterminated",
    ] {
        assert_eq!(kind(&engine.evaluate(&bash(cmd))), V::Ask, "{cmd}");
    }
    // A session grant does not lift it either.
    engine.add_grant(ApprovalScope::Tool {
        name: "bash".into(),
    });
    assert_eq!(kind(&engine.evaluate(&bash("bash -c 'rm -rf x'"))), V::Ask);
    // Plain commands are unaffected.
    assert_eq!(kind(&engine.evaluate(&bash("ls"))), V::Allow);
}

#[test]
fn opaque_commands_without_a_deny_rule_keep_the_mode_default() {
    let dir = tempfile::tempdir().unwrap();
    let engine = PolicyEngine::new(PolicySpec::new(ApprovalMode::FullAuto), dir.path());
    assert_eq!(
        kind(&engine.evaluate(&bash("bash -c 'rm -rf x'"))),
        V::Allow
    );
    // A deny rule for another tool does not apply.
    let spec = PolicySpec::new(ApprovalMode::FullAuto).deny(Rule::tool("edit").path("a/**"));
    let engine = PolicyEngine::new(spec, dir.path());
    assert_eq!(kind(&engine.evaluate(&bash("bash -c 'x'"))), V::Allow);
}

#[test]
fn opaque_commands_under_a_deny_rule_in_read_only_are_denied() {
    let engine = rm_denied(ApprovalMode::ReadOnly);
    assert_eq!(kind(&engine.evaluate(&bash("bash -c 'rm x'"))), V::Deny);
}

#[tokio::test]
async fn non_interactive_chain_denies_an_opaque_command() {
    use cosmos_llm_agent_core::policy::PolicyApprover;
    use cosmos_llm_tool::{Approver, FnApprover};
    use std::sync::Arc;
    let engine = Arc::new(rm_denied(ApprovalMode::FullAuto));
    // exec's non-interactive approver: ask resolves to deny.
    let next: Arc<dyn Approver> = Arc::new(FnApprover::new(|_| Decision::deny("no one to ask")));
    let chain = PolicyApprover::new(engine, next);
    let d = chain.check(&bash("bash -c 'rm -rf x'")).await;
    assert!(matches!(d, Decision::Deny { .. }), "{d:?}");
    assert_eq!(chain.check(&bash("ls")).await, Decision::Allow);
}
