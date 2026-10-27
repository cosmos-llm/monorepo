//! Session persistence (200).

use std::io::Write;

use cosmos_llm::{ContentBlock, Message, Role};
use cosmos_llm_agent_core::session::{aux_dir, parse_records};
use cosmos_llm_agent_core::{DisplayItem, Record, SessionLog, SessionStore};
use cosmos_llm_tool::ToolSpec;
use serde_json::json;

fn call(id: &str) -> Message {
    Message::assistant_blocks(vec![ContentBlock::tool_use(cosmos_llm::ToolCall {
        id: id.into(),
        name: "read".into(),
        input: json!({"file_path": "a"}),
    })])
}

fn result(id: &str) -> Message {
    Message::user_blocks(vec![ContentBlock::ToolResult {
        tool_use_id: id.into(),
        content: vec![ContentBlock::text("ok")],
        is_error: false,
    }])
}

fn texts(h: &[Message]) -> Vec<String> {
    h.iter()
        .map(|m| {
            m.content
                .iter()
                .map(|b| match b {
                    ContentBlock::Text { text } => text.clone(),
                    ContentBlock::ToolUse { .. } => "<use>".into(),
                    ContentBlock::ToolResult { .. } => "<result>".into(),
                    _ => "?".into(),
                })
                .collect::<String>()
        })
        .collect()
}

#[tokio::test]
async fn tools_compaction_and_truncation_replay() {
    let root = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let store = SessionStore::new(root.path());
    let w = store.create(cwd.path(), "m1").unwrap();
    let path = w.path().to_owned();

    // 1 user, 2 call, 3 result, 4 answer, 5 user, 6 answer
    let msgs = [
        Message::user("first"),
        call("t1"),
        result("t1"),
        Message::assistant("done one"),
        Message::user("second"),
        Message::assistant("done two"),
    ];
    for (i, m) in msgs.iter().enumerate() {
        w.append_message(i as u64 + 1, m, None).unwrap();
    }
    w.append(&Record::ToolOutcome {
        call_id: "t1".into(),
        display: Some(json!({"lines": 3})),
    })
    .unwrap();

    // Compact 1..=4 into a summary (id 7).
    w.append_message(7, &Message::user("summary of one"), None)
        .unwrap();
    w.append(&Record::Compaction {
        replaces: 1..5,
        summary: 7,
        except: vec![],
    })
    .unwrap();
    // Then a new exchange (8, 9) and an edit-and-resend dropping from 8.
    w.append_message(8, &Message::user("third"), None).unwrap();
    w.append_message(9, &Message::assistant("done three"), None)
        .unwrap();
    w.append(&Record::Truncated { after: 6 }).unwrap();
    w.append_message(10, &Message::user("third, reworded"), None)
        .unwrap();
    w.append(&Record::ModelChanged { model: "m2".into() })
        .unwrap();
    w.sync().await.unwrap();
    w.close().await.unwrap();

    let s = SessionLog::load(&path).unwrap();
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    assert_eq!(s.model, "m2");
    assert_eq!(s.msg_ids, vec![7, 5, 6, 10]);
    assert_eq!(
        texts(s.thread.history()),
        ["summary of one", "second", "done two", "third, reworded"]
    );
    assert_eq!(s.next_id, 11);

    // The display view keeps everything and flags what the model lost.
    let flags: Vec<(u64, bool)> = s
        .display
        .iter()
        .filter_map(|d| match d {
            DisplayItem::Message { id, in_context, .. } => Some((*id, *in_context)),
            _ => None,
        })
        .collect();
    assert_eq!(
        flags,
        vec![
            (1, false),
            (2, false),
            (3, false),
            (4, false),
            (5, true),
            (6, true),
            (7, true),
            (8, false),
            (9, false),
            (10, true)
        ]
    );
    assert!(s
        .display
        .iter()
        .any(|d| matches!(d, DisplayItem::ToolDisplay { call_id, .. } if call_id == "t1")));
    assert!(s
        .display
        .iter()
        .any(|d| matches!(d, DisplayItem::Compaction { .. })));
    assert!(s
        .display
        .iter()
        .any(|d| matches!(d, DisplayItem::Truncated { after: 6 })));
}

#[tokio::test]
async fn compaction_after_compaction_uses_except() {
    let root = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let store = SessionStore::new(root.path());
    let w = store.create(cwd.path(), "m").unwrap();
    let path = w.path().to_owned();
    for i in 1..=4u64 {
        let m = if i % 2 == 1 {
            Message::user(format!("u{i}"))
        } else {
            Message::assistant(format!("a{i}"))
        };
        w.append_message(i, &m, None).unwrap();
    }
    // Summary S(5) replaces 1..=2, leaving [S5, 3, 4].
    w.append_message(5, &Message::user("S"), None).unwrap();
    w.append(&Record::Compaction {
        replaces: 1..3,
        summary: 5,
        except: vec![],
    })
    .unwrap();
    // Second compaction replaces [S5, 3] => ids {5, 3}; range 3..6 holds 4 too.
    w.append_message(6, &Message::user("S2"), None).unwrap();
    w.append(&Record::Compaction {
        replaces: 3..6,
        summary: 6,
        except: vec![4],
    })
    .unwrap();
    w.close().await.unwrap();
    let s = SessionLog::load(&path).unwrap();
    assert_eq!(s.msg_ids, vec![6, 4]);
}

#[tokio::test]
async fn torn_last_line_loads_everything_before_it() {
    let root = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let store = SessionStore::new(root.path());
    let w = store.create(cwd.path(), "m").unwrap();
    let path = w.path().to_owned();
    w.append_message(1, &Message::user("hi"), None).unwrap();
    w.append_message(2, &Message::assistant("hello"), None)
        .unwrap();
    w.close().await.unwrap();

    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    f.write_all(b"{\"type\":\"message\",\"id\":3,\"mess")
        .unwrap();
    drop(f);

    let s = SessionLog::load(&path).unwrap();
    assert_eq!(s.msg_ids, vec![1, 2]);
    assert!(s.warnings.iter().any(|w| w.contains("torn")));

    // Reopening appends on a fresh line.
    let w = store.reopen(&path).unwrap();
    w.append_message(3, &Message::user("again"), None).unwrap();
    w.close().await.unwrap();
    assert_eq!(SessionLog::load(&path).unwrap().msg_ids, vec![1, 2, 3]);
}

#[tokio::test]
async fn a_crash_mid_tool_call_is_repaired() {
    let root = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let store = SessionStore::new(root.path());
    let w = store.create(cwd.path(), "m").unwrap();
    let path = w.path().to_owned();
    w.append_message(1, &Message::user("go"), None).unwrap();
    w.append_message(2, &call("t1"), None).unwrap();
    w.close().await.unwrap();
    let s = SessionLog::load(&path).unwrap();
    assert_eq!(texts(s.thread.history()), ["go"]);
    assert!(s.warnings.iter().any(|w| w.contains("interrupted")));
}

#[test]
fn unknown_record_types_are_skipped_with_a_warning() {
    let text = concat!(
        r#"{"type":"session_start","v":1,"id":"67e55044-10b1-426f-9247-bb680e5fe0c8","cwd":"/x","model":"m","started_at":"2026-10-27T14:03:11Z"}"#,
        "\n",
        r#"{"type":"hologram","v":9,"x":1}"#,
        "\n",
        r#"{"type":"message","id":1,"message":{"role":"user","content":[{"type":"text","text":"hi"}]}}"#,
        "\n"
    );
    let (records, warnings) = parse_records(text);
    assert_eq!(records.len(), 2);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("hologram"));
}

#[tokio::test]
async fn fork_copies_up_to_the_cut_and_points_at_the_parent() {
    let root = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let store = SessionStore::new(root.path());
    let w = store.create(cwd.path(), "m").unwrap();
    let src = w.path().to_owned();
    let src_id = w.id();
    let msgs = [
        Message::user("one"),
        Message::assistant("two"),
        Message::user("three"),
        Message::assistant("four"),
        Message::user("five"),
    ];
    for (i, m) in msgs.iter().enumerate() {
        w.append_message(i as u64 + 1, m, None).unwrap();
    }
    w.close().await.unwrap();

    let forked = store.fork(&src, Some(3)).unwrap();
    store.delete(&src).unwrap(); // the fork must not depend on the source
    let s = SessionLog::load(&forked).unwrap();
    assert_eq!(texts(s.thread.history()), ["one", "two", "three"]);
    let parent = s.parent.unwrap();
    assert_eq!((parent.session, parent.at), (src_id, Some(3)));
    assert_ne!(s.id, src_id);
}

#[tokio::test]
async fn fork_inside_a_tool_exchange_backs_up() {
    let root = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let store = SessionStore::new(root.path());
    let w = store.create(cwd.path(), "m").unwrap();
    let src = w.path().to_owned();
    w.append_message(1, &Message::user("go"), None).unwrap();
    w.append_message(2, &call("t"), None).unwrap();
    w.append_message(3, &result("t"), None).unwrap();
    w.append_message(4, &Message::assistant("done"), None)
        .unwrap();
    w.close().await.unwrap();
    let forked = store.fork(&src, Some(2)).unwrap();
    let s = SessionLog::load(&forked).unwrap();
    assert_eq!(texts(s.thread.history()), ["go"]);
    assert!(s.warnings.is_empty());
}

#[tokio::test]
async fn continue_picks_the_newest_and_resume_finds_by_prefix() {
    let root = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let store = SessionStore::new(root.path());
    let a = store.create(cwd.path(), "m").unwrap();
    a.append_message(1, &Message::user("old"), None).unwrap();
    a.close().await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let b = store.create(cwd.path(), "m").unwrap();
    b.append_message(1, &Message::user("new"), None).unwrap();
    b.append_message(2, &Message::assistant("r"), None).unwrap();
    let b_id = b.id();
    b.close().await.unwrap();
    let c = store.create(other.path(), "m").unwrap();
    c.close().await.unwrap();

    let latest = store.latest(cwd.path()).unwrap().unwrap();
    assert_eq!(latest.id, b_id);
    assert_eq!(latest.first_user_message.as_deref(), Some("new"));
    assert_eq!(latest.turns, 1);
    assert_eq!(store.list(cwd.path()).unwrap().len(), 2);
    assert_eq!(store.list(other.path()).unwrap().len(), 1);
    let found = store.find(cwd.path(), &b_id.to_string()[..8]).unwrap();
    assert_eq!(found.id, b_id);
    assert!(store.find(cwd.path(), "zzzz").is_err());
}

#[tokio::test]
async fn images_become_blob_refs_and_come_back() {
    let root = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let store = SessionStore::new(root.path());
    let w = store.create(cwd.path(), "m").unwrap();
    let path = w.path().to_owned();
    let msg = Message::user_blocks(vec![
        ContentBlock::text("look"),
        ContentBlock::image_base64("image/png", [137u8, 80, 78, 71, 13, 10, 26, 10].repeat(100)),
    ]);
    w.append_message(1, &msg, None).unwrap();
    w.close().await.unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(
        !raw.contains("iVBORw0KGgo"),
        "image data leaked into the log"
    );
    assert!(raw.contains("blob:"));
    assert_eq!(
        std::fs::read_dir(aux_dir(&path).join("blobs"))
            .unwrap()
            .count(),
        1
    );

    let s = SessionLog::load(&path).unwrap();
    assert_eq!(s.thread.history()[0], msg);
}

#[tokio::test]
async fn a_second_writer_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let store = SessionStore::new(root.path());
    let w = store.create(cwd.path(), "m").unwrap();
    let err = store.reopen(w.path());
    assert!(
        matches!(err, Err(cosmos_llm_agent_core::CoreError::SessionLocked(_))),
        "{err:?}"
    );
    w.close().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn files_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let store = SessionStore::new(root.path().join("sessions"));
    let w = store.create(cwd.path(), "m").unwrap();
    let path = w.path().to_owned();
    w.close().await.unwrap();
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(path.parent().unwrap()), 0o700);
}

#[tokio::test]
async fn retention_deletes_old_sessions() {
    let root = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let store = SessionStore::new(root.path());
    let w = store.create(cwd.path(), "m").unwrap();
    w.close().await.unwrap();
    assert_eq!(store.apply_retention(30).unwrap(), 0);
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    // Zero days: everything older than "now" goes.
    assert_eq!(store.apply_retention(0).unwrap(), 1);
    assert!(store.list(cwd.path()).unwrap().is_empty());
    let _ = ToolSpec::new("unused");
    let _ = Role::User;
}
