use std::path::Path;
use std::sync::Arc;

use cosmos_llm_coding_tools::search::{
    grep_tool, rg_available, search_builtin, search_rg, FileIndex, Query,
};
use cosmos_llm_coding_tools::tools::{plan_edit, plan_write};
use cosmos_llm_coding_tools::{CodingTools, DiskFs, EditSpec, GrepBackend};
use cosmos_llm_tool::fs::{ToolFs, VfsFs};
use cosmos_llm_tool::preset::{
    glob_tool_fs, grep_tool_fs, list_tool_fs, read_tool_fs, write_tool_fs,
};
use cosmos_llm_tool::Registry;
use serde_json::{json, Value};

struct Env {
    dir: tempfile::TempDir,
    tools: CodingTools,
    registry: Registry,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let tools = CodingTools::new(Arc::new(DiskFs::new(dir.path()).unwrap()));
    let mut registry = Registry::new();
    tools.register_all(&mut registry);
    Env {
        dir,
        tools,
        registry,
    }
}

impl Env {
    fn call(&self, tool: &str, params: Value) -> Value {
        self.registry.get(tool).unwrap().call(&params).unwrap()
    }
    fn put(&self, rel: &str, content: &str) {
        let p = self.dir.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }
    fn get(&self, rel: &str) -> String {
        std::fs::read_to_string(self.dir.path().join(rel)).unwrap()
    }
    fn read(&self, rel: &str) {
        let r = self.call("read", json!({ "file_path": rel }));
        assert_eq!(r["success"], true, "{r}");
    }
    fn edit(&self, rel: &str, old: &str, new: &str) -> Value {
        self.call(
            "edit",
            json!({ "file_path": rel, "old_string": old, "new_string": new }),
        )
    }
}

fn err(v: &Value) -> String {
    assert_eq!(v["success"], false, "expected failure: {v}");
    v["error"].as_str().unwrap().to_string()
}

// --- 100: the same preset suite on both backends ---

fn preset_suite(fs: Arc<dyn ToolFs>) {
    let w = write_tool_fs(fs.clone());
    let r = w
        .call(&json!({ "file_path": "src/lib.rs", "content": "// TODO: a\nfn main(){}\n" }))
        .unwrap();
    assert_eq!(r["created"], true);
    w.call(&json!({ "file_path": "notes.txt", "content": "one\ntwo\nthree\n" }))
        .unwrap();
    let r = w
        .call(&json!({ "file_path": "notes.txt", "content": "one\ntwo\nthree\nfour\n" }))
        .unwrap();
    assert_eq!(r["updated"], true);

    let rd = read_tool_fs(fs.clone());
    let r = rd
        .call(&json!({ "file_path": "notes.txt", "offset": 1, "limit": 2 }))
        .unwrap();
    assert_eq!(r["start_line"], 2);
    assert_eq!(r["read_lines"], 2);
    assert_eq!(r["total_lines"], 4);
    assert_eq!(
        rd.call(&json!({ "file_path": "nope" })).unwrap()["success"],
        false
    );

    let g = glob_tool_fs(fs.clone())
        .call(&json!({ "pattern": "**/*.rs" }))
        .unwrap();
    assert_eq!(g["paths"], json!(["src/lib.rs"]));
    let l = list_tool_fs(fs.clone()).call(&json!({})).unwrap();
    assert_eq!(l["count"], 2);
    let m = grep_tool_fs(fs)
        .call(&json!({ "pattern": "TODO" }))
        .unwrap();
    assert_eq!(m["match_count"], 1);
    assert_eq!(m["matches"][0]["file"], "src/lib.rs");
}

#[test]
fn presets_on_vfs() {
    preset_suite(Arc::new(VfsFs::empty()));
}

#[test]
fn presets_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    preset_suite(Arc::new(DiskFs::new(dir.path()).unwrap()));
}

// --- 110: edit ---

#[test]
fn edit_unique_multiple_none_replace_all() {
    let e = env();
    e.put("a.txt", "alpha\nbeta\nalpha\n");
    e.read("a.txt");
    assert!(err(&e.edit("a.txt", "alpha", "x")).contains("found 2 matches"));
    assert!(err(&e.edit("a.txt", "zzz", "x")).contains("not found"));
    let r = e.edit("a.txt", "beta", "gamma");
    assert_eq!(r["success"], true, "{r}");
    assert_eq!(r["message"], "Edited a.txt (+1 \u{2212}1)");
    assert_eq!(e.get("a.txt"), "alpha\ngamma\nalpha\n");
    let r = e.call(
        "edit",
        json!({"file_path": "a.txt", "old_string": "alpha", "new_string": "A", "replace_all": true}),
    );
    assert_eq!(r["replacements"], 2);
    assert_eq!(e.get("a.txt"), "A\ngamma\nA\n");
    // Back-to-back edits: the write refreshed the stamp.
    assert_eq!(e.edit("a.txt", "gamma", "g")["success"], true);
}

#[test]
fn crlf_is_preserved() {
    let e = env();
    e.put("w.txt", "one\r\ntwo\r\nthree\r\n");
    e.read("w.txt");
    assert_eq!(e.edit("w.txt", "two", "2\n2b")["success"], true);
    assert_eq!(e.get("w.txt"), "one\r\n2\r\n2b\r\nthree\r\n");
}

#[test]
fn whitespace_fallback_reindents_and_says_so() {
    let e = env();
    e.put("t.rs", "fn a() {\n\tif x {\n\t\tlet a = 1;\n\t}\n}\n");
    e.read("t.rs");
    let r = e.edit(
        "t.rs",
        "    if x {\n        let a = 1;\n    }",
        "    if x {\n        let a = 2;\n    }",
    );
    assert_eq!(r["success"], true, "{r}");
    assert!(r["note"].as_str().unwrap().contains("whitespace"));
    assert_eq!(
        e.get("t.rs"),
        "fn a() {\n\tif x {\n\t\tlet a = 2;\n\t}\n}\n"
    );
}

#[test]
fn guard_requires_a_fresh_read() {
    let e = env();
    e.put("g.txt", "one\n");
    assert!(err(&e.edit("g.txt", "one", "two")).contains("not been read"));
    e.read("g.txt");
    // Make sure the external write is visible in length/mtime.
    std::thread::sleep(std::time::Duration::from_millis(20));
    e.put("g.txt", "one!\n");
    let msg = err(&e.edit("g.txt", "one", "two"));
    assert!(msg.contains("modified since it was last read"), "{msg}");
    assert_eq!(e.get("g.txt"), "one!\n");
    e.read("g.txt");
    assert_eq!(e.edit("g.txt", "one", "two")["success"], true);
}

#[test]
fn rewrite_with_same_content_is_still_fresh() {
    let e = env();
    e.put("s.txt", "same\n");
    e.read("s.txt");
    std::thread::sleep(std::time::Duration::from_millis(20));
    e.put("s.txt", "same\n");
    assert_eq!(e.edit("s.txt", "same", "other")["success"], true);
}

#[test]
fn empty_old_string_creates_a_file() {
    let e = env();
    let r = e.edit("new/dir/f.txt", "", "hello\n");
    assert_eq!(r["success"], true, "{r}");
    assert_eq!(e.get("new/dir/f.txt"), "hello\n");
    // Missing file with a non-empty old_string is an error.
    assert!(err(&e.edit("nope.txt", "a", "b")).contains("does not exist"));
}

#[test]
fn multi_edit_is_all_or_nothing() {
    let e = env();
    e.put("m.txt", "a\nb\nc\n");
    e.read("m.txt");
    let bad = e.call(
        "multi_edit",
        json!({"file_path": "m.txt", "edits": [
            {"old_string": "a", "new_string": "A"},
            {"old_string": "b", "new_string": "B"},
            {"old_string": "zzz", "new_string": "Z"},
        ]}),
    );
    let msg = err(&bad);
    assert!(msg.contains("edit 2 failed"), "{msg}");
    assert_eq!(e.get("m.txt"), "a\nb\nc\n");
    let ok = e.call(
        "multi_edit",
        json!({"file_path": "m.txt", "edits": [
            {"old_string": "a", "new_string": "b"},
            {"old_string": "b", "new_string": "c", "replace_all": true},
        ]}),
    );
    assert_eq!(ok["success"], true, "{ok}");
    assert_eq!(e.get("m.txt"), "c\nc\nc\n");
}

#[test]
fn write_guard_and_create() {
    let e = env();
    let r = e.call("write", json!({"file_path": "d/n.txt", "content": "x"}));
    assert_eq!(r["success"], true);
    assert_eq!(r["message"], "Created d/n.txt (+1 \u{2212}0)");
    e.put("old.txt", "keep\n");
    let r = e.call("write", json!({"file_path": "old.txt", "content": "new\n"}));
    assert!(err(&r).contains("not been read"));
    e.read("old.txt");
    let r = e.call("write", json!({"file_path": "old.txt", "content": "new\n"}));
    assert_eq!(r["success"], true);
    assert_eq!(e.get("old.txt"), "new\n");
}

#[test]
fn preview_diff_equals_the_real_edit() {
    let e = env();
    e.put("p.txt", "1\n2\n3\n4\n5\n");
    e.read("p.txt");
    let fs = e.tools.fs();
    let tracker = e.tools.tracker();
    let specs = [EditSpec::new("3", "three")];
    let preview = plan_edit(&*fs, &tracker.lock().unwrap(), "p.txt", &specs).unwrap();
    let real = e.call(
        "edit",
        json!({"file_path": "p.txt", "old_string": "3", "new_string": "three"}),
    );
    assert_eq!(real["diff"], preview.diff.unified());
    // A write plan previews without touching the disk.
    let wp = plan_write(&*fs, &tracker.lock().unwrap(), "p.txt", "z\n").unwrap();
    assert_eq!(wp.diff.removed, 5);
    assert_eq!(e.get("p.txt"), "1\n2\nthree\n4\n5\n");
}

#[test]
fn tools_cannot_leave_the_workspace() {
    let e = env();
    let r = e.call(
        "write",
        json!({"file_path": "../escape.txt", "content": "x"}),
    );
    assert!(err(&r).contains("outside"));
    let r = e.call("read", json!({"file_path": "/etc/passwd"}));
    assert_eq!(r["success"], false);
}

// --- 130 through the tools: turn diff and undo ---

#[test]
fn turn_diff_and_revert_through_tools() {
    let e = env();
    e.put("a.txt", "one\n");
    let tracker = e.tools.tracker();
    tracker.lock().unwrap().begin_turn();
    e.read("a.txt");
    e.edit("a.txt", "one", "two");
    e.call("write", json!({"file_path": "b.txt", "content": "new\n"}));
    let fs = e.tools.fs();
    let diffs = tracker.lock().unwrap().turn_diff(&*fs, 1);
    assert_eq!(diffs.len(), 2);
    assert_eq!(diffs[0].path, "a.txt");
    assert_eq!((diffs[0].added, diffs[0].removed), (1, 1));
    assert_eq!(diffs[1].kind, cosmos_llm_coding_tools::ChangeKind::Created);

    let report = tracker.lock().unwrap().revert_turn(&*fs, 1, false).unwrap();
    assert_eq!(report.reverted.len(), 2);
    assert_eq!(e.get("a.txt"), "one\n");
    assert!(!e.dir.path().join("b.txt").exists());
}

// --- 140: search ---

#[test]
fn glob_ignores_gitignored_and_sorts_newest_first() {
    let e = env();
    e.put(".gitignore", "build/\n");
    e.put("build/gen.rs", "");
    e.put("node_modules/x/i.rs", "");
    e.put("src/old.rs", "");
    std::thread::sleep(std::time::Duration::from_millis(30));
    e.put("src/new.rs", "");
    let r = e.call("glob", json!({"pattern": "**/*.rs"}));
    assert_eq!(r["paths"], json!(["src/new.rs", "src/old.rs"]));
    let r = e.call("glob", json!({"pattern": "*.rs", "path": "src"}));
    assert_eq!(r["count"], 2);
}

#[test]
fn glob_caps_results() {
    let e = env();
    for i in 0..130 {
        e.put(&format!("f/{i:03}.txt"), "");
    }
    let r = e.call("glob", json!({"pattern": "*.txt"}));
    assert_eq!(r["count"], 100);
    assert_eq!(r["total"], 130);
    assert!(r["note"].as_str().unwrap().contains("30 more"));
}

fn tree(e: &Env) {
    e.put(".gitignore", "build/\n*.log\n");
    e.put("build/out.rs", "needle in build\n");
    e.put("debug.log", "needle in log\n");
    e.put(
        "src/a.rs",
        "fn needle() {}\nlet x = 1;\nlet y = 2;\nNeedle again\n",
    );
    e.put("src/b.py", "needle = 1\n");
    e.put("docs/c.md", "no match here\n");
    e.put("bin.dat", "needle\0binary");
    e.put(".hidden/h.txt", "needle hidden\n");
    e.put("node_modules/m/x.js", "needle nm\n");
}

#[test]
fn grep_skips_ignored_and_supports_modes() {
    let e = env();
    tree(&e);
    let r = e.call("grep", json!({"pattern": "needle"}));
    assert_eq!(r["output"], ".hidden/h.txt\nsrc/a.rs\nsrc/b.py", "{r}");
    // The in-process backend gives the same answer through the tool.
    let builtin = grep_tool(e.tools.fs(), GrepBackend::Builtin);
    let r = builtin.call(&json!({"pattern": "needle"})).unwrap();
    assert_eq!(r["backend"], "builtin");
    assert_eq!(r["output"], ".hidden/h.txt\nsrc/a.rs\nsrc/b.py", "{r}");
    let r = e.call("grep", json!({"pattern": "needle", "output_mode": "count"}));
    assert_eq!(r["output"], ".hidden/h.txt:1\nsrc/a.rs:1\nsrc/b.py:1");
    let r = e.call(
        "grep",
        json!({"pattern": "needle", "output_mode": "content", "glob": "*.rs", "case_insensitive": true}),
    );
    assert_eq!(
        r["output"],
        "src/a.rs:1:fn needle() {}\nsrc/a.rs:4:Needle again"
    );
    let r = e.call("grep", json!({"pattern": "needle", "type": "py"}));
    assert_eq!(r["output"], "src/b.py");
    let r = e.call(
        "grep",
        json!({"pattern": "let x", "output_mode": "content", "context": 1}),
    );
    assert_eq!(
        r["output"],
        "src/a.rs-1-fn needle() {}\nsrc/a.rs:2:let x = 1;\nsrc/a.rs-3-let y = 2;"
    );
    let r = e.call("grep", json!({"pattern": "(", "path": "src"}));
    assert!(err(&r).contains("invalid regex"));
}

#[test]
fn builtin_matches_ripgrep() {
    if !rg_available() {
        eprintln!("rg not installed; skipping");
        return;
    }
    let e = env();
    tree(&e);
    let fs = e.tools.fs();
    let queries = [
        Query::new("needle"),
        Query::new("needle").case_insensitive(true).glob("*.rs"),
        Query::new("let x").context(1),
        Query::new("needle").file_type("py"),
        Query::new("^fn \\w+"),
    ];
    for q in &queries {
        let a = search_rg(&fs, Path::new(""), q).unwrap();
        let b = search_builtin(&fs, Path::new(""), q).unwrap();
        assert_eq!(a, b, "query {q:?}");
    }
    let a = search_rg(&fs, Path::new("src"), &Query::new("needle")).unwrap();
    let b = search_builtin(&fs, Path::new("src"), &Query::new("needle")).unwrap();
    assert_eq!(a, b);
}

#[test]
fn grep_caps_output_and_cuts_long_lines() {
    let e = env();
    let many: String = (0..1000).map(|i| format!("match {i}\n")).collect();
    e.put("big.txt", &many);
    e.put("min.js", &format!("match {}\n", "x".repeat(2000)));
    let r = e.call(
        "grep",
        json!({"pattern": "match", "output_mode": "content", "path": ""}),
    );
    assert_eq!(r["truncated"], true);
    assert_eq!(r["match_count"], 1001);
    let out = r["output"].as_str().unwrap();
    assert!(out.contains("(truncated at 250; 751 more; narrow the search)"));
    assert!(out.lines().all(|l| l.chars().count() < 560));
    let r = e.call(
        "grep",
        json!({"pattern": "match", "output_mode": "content", "glob": "*.js"}),
    );
    let out = r["output"].as_str().unwrap();
    assert!(out.contains("... [line truncated]"));
    assert!(out.chars().count() < 560);
    let r = e.call(
        "grep",
        json!({"pattern": "match", "output_mode": "content", "head_limit": 5}),
    );
    assert_eq!(r["output"].as_str().unwrap().lines().count(), 6);
}

#[test]
fn file_index_ranks_path_matches() {
    let e = env();
    e.put("src/lib.rs", "");
    e.put("src/library_notes.md", "");
    e.put("docs/readme.md", "");
    e.put("target/debug/lib.rs", "");
    let fs = e.tools.fs();
    let mut idx = FileIndex::build(&*fs);
    assert_eq!(idx.len(), 3);
    let m = idx.query("srclib", 5);
    assert_eq!(m[0].path, "src/lib.rs");
    assert_eq!(m[0].indices[0], 0);
    assert!(idx.query("zzzqq", 5).is_empty());
    e.put("src/zzzqq.rs", "");
    idx.refresh(&*fs);
    assert_eq!(idx.query("zzzqq", 5)[0].path, "src/zzzqq.rs");
    assert_eq!(idx.query("", 2).len(), 2);
}

#[test]
fn edit_tools_offer_a_diff_preview_and_path_hints() {
    use cosmos_llm_coding_tools::{CodingTools, DiskFs};
    use cosmos_llm_tool::Registry;
    use serde_json::json;
    use std::sync::Arc;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
    let tools = CodingTools::new(Arc::new(DiskFs::new(dir.path()).unwrap()));
    let mut registry = Registry::new();
    tools.register_all(&mut registry);

    registry
        .get("read")
        .unwrap()
        .call(&json!({"file_path": "a.txt"}))
        .unwrap();

    let edit = registry.get("edit").unwrap();
    let input = json!({"file_path": "a.txt", "old_string": "two", "new_string": "2"});
    let hints = edit.call_hints(&input);
    assert!(hints.destructive && !hints.read_only);
    assert_eq!(hints.paths[0].to_str(), Some("a.txt"));
    let preview = edit.call_preview(&input).expect("preview");
    assert_eq!(preview["diff"]["added"], 1);
    // Previewing writes nothing.
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "one\ntwo\n"
    );

    // An edit that cannot apply has no preview; the tool explains after approval.
    let bad = json!({"file_path": "a.txt", "old_string": "zzz", "new_string": "y"});
    assert!(edit.call_preview(&bad).is_none());
    assert!(registry.get("read").unwrap().base_hints.read_only);
}
