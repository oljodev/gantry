//! The Changes pane's data, against a real store and real files (docs/plan/16 §5).

use std::sync::Arc;

use gantry_core::{ChatId, EditOp, Mode, ModelRef, ReasoningEffort, Surface, now_ms};
use gantry_store::{BlobStore, Store, repos::chats};
use gantry_workspace::{Change, Workspace};

struct Fixture {
    _dir: tempfile::TempDir,
    work: tempfile::TempDir,
    workspace: Arc<Workspace>,
    chat: ChatId,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path().join("t.db")).unwrap());
    let blobs = Arc::new(BlobStore::open(dir.path().join("blobs")).unwrap());
    let now = now_ms();
    let chat = ChatId::new();
    let record = chats::ChatRecord {
        surface: Surface::Code,
        id: chat,
        project_id: None,
        title: "changes".into(),
        title_source: "auto".into(),
        pinned: false,
        mode: Mode::AutoEdit,
        guard: true,
        model: ModelRef::default_model(),
        effort: ReasoningEffort::Medium,
        web_search: false,
        instructions: String::new(),
        system_snapshot: "sys".into(),
        system_snapshot_version: 1,
        created_at: now,
        updated_at: now,
        last_message_at: now,
        archived_at: None,
        incognito: false,
    };
    let root = work.path().display().to_string();
    store
        .write_blocking(move |conn| {
            chats::insert(conn, &record)?;
            chats::add_root(conn, chat, &root)
        })
        .unwrap();
    // No trash: a test that reverts a created file must see it gone, not moved.
    let workspace =
        Arc::new(Workspace::new(store, blobs, dir.path().join("app-data")).without_trash());
    Fixture {
        _dir: dir,
        work,
        workspace,
        chat,
    }
}

impl Fixture {
    fn path(&self, name: &str) -> String {
        self.work.path().join(name).display().to_string()
    }

    /// An edit as the code editor would make it: the file is read first, then changed.
    async fn edit(&self, name: &str, old: &str, new: &str) {
        let roots = self.workspace.roots(self.chat).unwrap();
        let path = self.path(name);
        let scoped = roots.resolve(&path).unwrap();
        self.workspace.read(self.chat, &scoped).await.unwrap();
        self.workspace
            .apply(
                &roots,
                self.chat,
                "call-1",
                &path,
                &Change::Replace {
                    old: old.to_owned(),
                    new: new.to_owned(),
                    count: 1,
                },
            )
            .await
            .unwrap();
    }

    async fn write(&self, name: &str, text: &str) {
        let roots = self.workspace.roots(self.chat).unwrap();
        self.workspace
            .write(
                &roots,
                self.chat,
                "call-1",
                &self.path(name),
                text.as_bytes(),
            )
            .await
            .unwrap();
    }

    async fn revert(&self, name: &str) -> Result<gantry_workspace::Reverted, String> {
        let roots = self.workspace.roots(self.chat).unwrap();
        self.workspace
            .revert_file(&roots, self.chat, &self.path(name))
            .await
            .map_err(|e| e.to_string())
    }
}

const FILE: &str = "one\ntwo\nthree\n";

#[tokio::test]
async fn a_file_edited_twice_is_one_row_with_the_net_diff() {
    let f = fixture();
    std::fs::write(f.work.path().join("a.txt"), FILE).unwrap();
    f.edit("a.txt", "one", "ONE").await;
    f.edit("a.txt", "three", "THREE").await;

    let changes = f.workspace.changes(f.chat).unwrap();
    assert_eq!(changes.len(), 1, "one file, not one row per edit");
    assert_eq!(changes[0].edits, 2, "and it says how many edits made it");
    assert_eq!(changes[0].op, EditOp::Modify);
    assert_eq!((changes[0].added, changes[0].removed), (2, 2));

    // The diff is the whole session's, first version to last, not the last edit's.
    let diff = f.workspace.file_change(f.chat, &f.path("a.txt")).unwrap();
    let text: String = diff.diff.hunks.iter().map(|h| h.text.clone()).collect();
    assert!(text.contains("-one"), "{text}");
    assert!(text.contains("+THREE"), "{text}");
}

#[tokio::test]
async fn revert_puts_the_file_back_and_closes_its_rows() {
    let f = fixture();
    std::fs::write(f.work.path().join("a.txt"), FILE).unwrap();
    f.edit("a.txt", "one", "ONE").await;
    f.edit("a.txt", "two", "TWO").await;

    let reverted = f.revert("a.txt").await.unwrap();
    assert_eq!(reverted.edits, 2, "both edits closed by one revert");
    assert_eq!(
        std::fs::read_to_string(f.work.path().join("a.txt")).unwrap(),
        FILE,
        "byte for byte"
    );
    assert!(
        f.workspace.changes(f.chat).unwrap().is_empty(),
        "a file that is back is not a change"
    );
    // The revert is itself in the history: a journal row is never deleted (code-editor.md §4).
    assert_eq!(f.workspace.journal().for_chat(f.chat).unwrap().len(), 3);
}

#[tokio::test]
async fn reverting_a_file_the_session_created_removes_it() {
    let f = fixture();
    f.write("new.txt", "fresh\n").await;
    let changes = f.workspace.changes(f.chat).unwrap();
    assert_eq!(changes[0].op, EditOp::Create);

    let reverted = f.revert("new.txt").await.unwrap();
    assert_eq!(reverted.op, EditOp::Delete);
    assert!(!f.work.path().join("new.txt").exists(), "it is gone again");
    assert!(f.workspace.changes(f.chat).unwrap().is_empty());
}

#[tokio::test]
async fn a_file_someone_else_wrote_since_is_refused_rather_than_overwritten() {
    let f = fixture();
    std::fs::write(f.work.path().join("a.txt"), FILE).unwrap();
    f.edit("a.txt", "one", "ONE").await;
    // Somebody's editor saves over it.
    std::fs::write(f.work.path().join("a.txt"), "their own work\n").unwrap();

    let err = f.revert("a.txt").await.unwrap_err();
    assert!(err.contains("changed since"), "{err}");
    assert_eq!(
        std::fs::read_to_string(f.work.path().join("a.txt")).unwrap(),
        "their own work\n",
        "their work is still there"
    );
    assert_eq!(
        f.workspace.changes(f.chat).unwrap().len(),
        1,
        "and the change is still listed, so it can be looked at"
    );
}

#[tokio::test]
async fn a_path_the_session_never_touched_has_nothing_to_put_back() {
    let f = fixture();
    std::fs::write(f.work.path().join("a.txt"), FILE).unwrap();
    let err = f.revert("a.txt").await.unwrap_err();
    assert!(err.contains("nothing to put back"), "{err}");
}
