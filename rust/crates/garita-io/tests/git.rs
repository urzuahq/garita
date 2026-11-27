#![cfg(unix)]

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use common::{TempDir, commit_all, git, init, write};
use garita_core::ObjectId;
use garita_core::risk::Touched;
use garita_io::git::{self as g, GitError};

const ODD: [&str; 4] = ["sp ace.txt", "quo\"te.txt", "new\nline.txt", "ñandú.txt"];

fn set(paths: Vec<PathBuf>) -> Vec<String> {
    let mut v: Vec<String> = paths
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

fn sorted(items: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = items.iter().map(|s| s.to_string()).collect();
    v.sort();
    v
}

/// A repository with every kind of change: staged, partly staged, unstaged, renamed,
/// deleted (staged and not), untracked and ignored, under names that need quoting.
fn messy() -> TempDir {
    let tmp = TempDir::new();
    let r = tmp.path();
    init(r);
    write(r, ".gitignore", "*.log\n");
    for name in [
        "a.txt",
        "partly.txt",
        "from.txt",
        "gone.txt",
        "dir/deep.txt",
    ]
    .into_iter()
    .chain(ODD)
    {
        write(r, name, &format!("{name} v1\n"));
    }
    commit_all(r, "base");
    write(r, "a.txt", "a v2\n");
    write(r, "quo\"te.txt", "quote v2\n");
    write(r, "new\nline.txt", "newline v2\n");
    write(r, "partly.txt", "partly v2\n");
    write(r, "added.txt", "added\n");
    write(r, "dir/deep.txt", "deep v2\n");
    git(
        r,
        &[
            "add",
            "a.txt",
            "quo\"te.txt",
            "new\nline.txt",
            "partly.txt",
            "added.txt",
            "dir/deep.txt",
        ],
    );
    git(r, &["mv", "from.txt", "to.txt"]);
    git(r, &["rm", "-q", "gone.txt"]);
    write(r, "partly.txt", "partly v3\n");
    write(r, "sp ace.txt", "space v2\n");
    fs::remove_file(r.join("ñandú.txt")).unwrap();
    write(r, "untracked.txt", "new\n");
    write(r, "debug.log", "ignored\n");
    tmp
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.insert(path.clone(), fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, &mut out);
    out
}

fn tree_names(root: &Path, tree: &ObjectId) -> Vec<String> {
    let out = git(root, &["ls-tree", "-r", "-z", "--name-only", tree.as_str()]);
    let mut v: Vec<String> = out
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    v.sort();
    v
}

#[test]
fn finds_the_root_from_a_subdirectory() {
    let tmp = messy();
    assert_eq!(g::root(&tmp.path().join("dir")).unwrap(), tmp.path());
    let outside = TempDir::new();
    assert!(matches!(
        g::root(outside.path()),
        Err(GitError::NotARepository)
    ));
}

#[test]
fn lists_staged_and_partly_staged_files() {
    let tmp = messy();
    let staged = set(g::staged_files(tmp.path()).unwrap());
    assert_eq!(
        staged,
        sorted(&[
            "a.txt",
            "added.txt",
            "dir/deep.txt",
            "new\nline.txt",
            "partly.txt",
            "quo\"te.txt",
            "to.txt"
        ])
    );
    assert_eq!(
        set(g::partly_staged(tmp.path()).unwrap()),
        sorted(&["partly.txt"])
    );
}

#[test]
fn lists_tracked_files_under_odd_names() {
    let tmp = messy();
    let all = set(g::all_files(tmp.path()).unwrap());
    for name in ODD.iter().chain(&["a.txt", "to.txt", ".gitignore"]) {
        assert!(all.contains(&name.to_string()), "{name:?} in {all:?}");
    }
    assert!(!all.contains(&"untracked.txt".into()) && !all.contains(&"gone.txt".into()));
}

#[test]
fn worktree_tree_hashes_the_working_files_without_touching_index_or_files() {
    let tmp = messy();
    let r = tmp.path();
    let index_before = fs::read(r.join(".git/index")).unwrap();
    let files_before = snapshot(r);
    let status_before = git(
        r,
        &[
            "--no-optional-locks",
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
        ],
    );

    let tree = g::worktree_tree(r).unwrap();
    let index_tree = g::tree_of_index(r).unwrap();

    assert_eq!(fs::read(r.join(".git/index")).unwrap(), index_before);
    let mut files_after = snapshot(r);
    let leftover: Vec<_> = files_after
        .keys()
        .filter(|p| p.to_string_lossy().contains("garita-index"))
        .cloned()
        .collect();
    assert!(leftover.is_empty(), "{leftover:?}");
    for (path, bytes) in &files_before {
        if !path.starts_with(r.join(".git/objects")) {
            assert_eq!(
                files_after.remove(path).as_ref(),
                Some(bytes),
                "{path:?} changed"
            );
        }
    }
    files_after.retain(|p, _| !p.starts_with(r.join(".git/objects")));
    assert!(
        files_after.is_empty(),
        "files appeared: {:?}",
        files_after.keys()
    );
    assert_eq!(
        git(
            r,
            &[
                "--no-optional-locks",
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all"
            ]
        ),
        status_before
    );

    let names = tree_names(r, &tree);
    for want in [
        "untracked.txt",
        "to.txt",
        "added.txt",
        "sp ace.txt",
        "new\nline.txt",
    ] {
        assert!(names.contains(&want.to_string()), "{want:?} in {names:?}");
    }
    for absent in ["debug.log", "gone.txt", "from.txt", "ñandú.txt"] {
        assert!(
            !names.contains(&absent.to_string()),
            "{absent:?} in {names:?}"
        );
    }
    let space = git(r, &["cat-file", "-p", &format!("{tree}:sp ace.txt")]);
    assert_eq!(space, "space v2\n");
    let partly = git(r, &["cat-file", "-p", &format!("{tree}:partly.txt")]);
    assert_eq!(partly, "partly v3\n");
    let staged_partly = git(r, &["cat-file", "-p", &format!("{index_tree}:partly.txt")]);
    assert_eq!(staged_partly, "partly v2\n");

    assert_eq!(index_tree.as_str(), git(r, &["write-tree"]).trim());
}

#[test]
fn dirty_compares_the_working_files_with_a_tree() {
    let tmp = messy();
    let r = tmp.path();
    let head = g::tree_of_commit(r, "HEAD").unwrap();
    assert!(g::dirty(r, &head).unwrap());
    commit_all(r, "everything");
    let head = g::tree_of_commit(r, "HEAD").unwrap();
    assert!(!g::dirty(r, &head).unwrap());
    assert_eq!(g::worktree_tree(r).unwrap(), head);
    assert_eq!(g::tree_of_index(r).unwrap(), head);
}

#[test]
fn head_commit_and_tree_of_commit() {
    let tmp = messy();
    let r = tmp.path();
    let head = g::head_commit(r).unwrap().unwrap();
    assert_eq!(head.as_str(), git(r, &["rev-parse", "HEAD"]).trim());
    let tree = g::tree_of_commit(r, head.as_str()).unwrap();
    assert_eq!(tree.as_str(), git(r, &["rev-parse", "HEAD^{tree}"]).trim());
    assert!(matches!(
        g::tree_of_commit(r, "no-such-ref"),
        Err(GitError::RefNotFound(_))
    ));
}

#[test]
fn an_empty_repository_has_no_commit() {
    let tmp = TempDir::new();
    let r = tmp.path();
    init(r);
    assert_eq!(g::head_commit(r).unwrap(), None);
    assert!(g::staged_files(r).unwrap().is_empty());
    let empty = g::worktree_tree(r).unwrap();
    assert_eq!(empty.as_str(), "4b825dc642cb6eb9a060e54bf8d69288fbee4904");
    write(r, "first.txt", "x\n");
    git(r, &["add", "first.txt"]);
    write(r, "loose.txt", "y\n");
    assert_eq!(set(g::staged_files(r).unwrap()), sorted(&["first.txt"]));
    let index = fs::read(r.join(".git/index")).unwrap();
    let tree = g::worktree_tree(r).unwrap();
    assert_eq!(fs::read(r.join(".git/index")).unwrap(), index);
    assert_eq!(tree_names(r, &tree), sorted(&["first.txt", "loose.txt"]));
    assert!(!g::is_shallow(r).unwrap());
}

fn branched() -> TempDir {
    let tmp = TempDir::new();
    let r = tmp.path();
    init(r);
    write(r, "base.txt", "1\n");
    write(r, "old.txt", "old\n");
    commit_all(r, "a");
    git(r, &["switch", "-q", "-c", "feature"]);
    write(r, "feature file.txt", "f\n");
    fs::remove_file(r.join("old.txt")).unwrap();
    commit_all(r, "c");
    write(r, "base.txt", "2\n");
    commit_all(r, "d");
    git(r, &["switch", "-q", "main"]);
    write(r, "main-only.txt", "m\n");
    commit_all(r, "b");
    git(r, &["switch", "-q", "feature"]);
    tmp
}

#[test]
fn changed_files_since_the_merge_base() {
    let tmp = branched();
    let r = tmp.path();
    write(r, "uncommitted.txt", "u\n");
    assert_eq!(
        set(g::changed_files(r, "main").unwrap()),
        sorted(&["base.txt", "feature file.txt"])
    );
    assert!(matches!(g::changed_files(r, "nope"), Err(GitError::RefNotFound(s)) if s == "nope"));
    assert!(matches!(
        g::changed_files(r, "--output=x"),
        Err(GitError::RefNotFound(_))
    ));
}

#[test]
fn touched_paths_name_deletes_and_both_sides_of_a_rename() {
    let tmp = branched();
    let r = tmp.path();
    let merge_base = g::merge_base(r, "main").unwrap();
    let mut committed: Vec<Touched> = g::touched_since(r, &merge_base, false).unwrap();
    committed.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(
        committed,
        [
            Touched::path("base.txt"),
            Touched::path("feature file.txt"),
            Touched::path("old.txt"),
        ]
    );
    write(
        r,
        "moved.txt",
        "a long enough line to be found as the same file\n",
    );
    commit_all(r, "e");
    git(r, &["mv", "moved.txt", "renamed.txt"]);
    write(r, "unstaged.txt", "u\n");
    let head = g::head_commit(r).unwrap().unwrap();
    assert_eq!(
        g::touched_since(r, &head, true).unwrap(),
        [Touched::renamed("moved.txt", "renamed.txt")]
    );
    assert!(g::touched_since(r, &head, false).unwrap().is_empty());
}

#[test]
fn show_file_reads_a_file_at_a_commit_or_says_there_is_none() {
    let tmp = branched();
    let r = tmp.path();
    assert_eq!(
        g::show_file(r, "main", "old.txt").unwrap().as_deref(),
        Some("old\n")
    );
    assert_eq!(g::show_file(r, "feature", "old.txt").unwrap(), None);
    assert_eq!(g::show_file(r, "main", "nope/x").unwrap(), None);
}

#[test]
fn a_shallow_clone_cannot_find_the_merge_base() {
    let src = branched();
    let dst = TempDir::new();
    let url = format!("file://{}", src.path().display());
    let clone = dst.path().join("c");
    git(
        dst.path(),
        &[
            "clone",
            "-q",
            "--depth",
            "1",
            "--no-single-branch",
            "-b",
            "feature",
            &url,
            clone.to_str().unwrap(),
        ],
    );
    assert!(g::is_shallow(&clone).unwrap());
    assert!(matches!(
        g::changed_files(&clone, "origin/main"),
        Err(GitError::Shallow(_))
    ));
    assert!(matches!(
        g::changed_files(&clone, "main"),
        Err(GitError::RefNotFound(_))
    ));
}

#[test]
fn unrelated_history_has_no_merge_base() {
    let tmp = branched();
    let r = tmp.path();
    git(r, &["switch", "-q", "--orphan", "other"]);
    write(r, "o.txt", "o\n");
    commit_all(r, "orphan");
    assert!(matches!(
        g::changed_files(r, "main"),
        Err(GitError::NoMergeBase(_))
    ));
}

#[test]
fn hooks_dir_follows_core_hooks_path() {
    let tmp = messy();
    let r = tmp.path();
    assert_eq!(g::hooks_dir(r).unwrap(), r.join(".git/hooks"));
    git(r, &["config", "core.hooksPath", ".githooks"]);
    assert_eq!(g::hooks_dir(r).unwrap(), r.join(".githooks"));
    assert_eq!(
        g::hooks_dir(&r.join("dir")).unwrap(),
        r.join("dir/.githooks")
    );
    git(r, &["config", "core.hooksPath", "/opt/hooks"]);
    assert_eq!(g::hooks_dir(r).unwrap(), PathBuf::from("/opt/hooks"));
    git(r, &["config", "core.hooksPath", "~/my-hooks"]);
    let home = PathBuf::from(std::env::var("HOME").unwrap());
    assert_eq!(g::hooks_dir(r).unwrap(), home.join("my-hooks"));
}

#[test]
fn a_linked_worktree_shares_hooks_and_keeps_its_own_index() {
    let tmp = messy();
    let r = tmp.path();
    let wt = r
        .parent()
        .unwrap()
        .join(format!("{}-wt", r.file_name().unwrap().to_string_lossy()));
    let _cleanup = Cleanup(wt.clone());
    git(
        r,
        &["worktree", "add", "-q", "-b", "wt", wt.to_str().unwrap()],
    );
    assert_eq!(g::root(&wt).unwrap(), wt);
    assert_eq!(g::hooks_dir(&wt).unwrap(), r.join(".git/hooks"));
    write(&wt, "wt-only.txt", "w\n");
    let wt_index = r
        .join(".git/worktrees/")
        .join(wt.file_name().unwrap())
        .join("index");
    let before = fs::read(&wt_index).unwrap();
    let main_before = fs::read(r.join(".git/index")).unwrap();
    let tree = g::worktree_tree(&wt).unwrap();
    assert_eq!(fs::read(&wt_index).unwrap(), before);
    assert_eq!(fs::read(r.join(".git/index")).unwrap(), main_before);
    assert!(tree_names(r, &tree).contains(&"wt-only.txt".to_string()));
    git(r, &["config", "core.hooksPath", ".githooks"]);
    assert_eq!(g::hooks_dir(&wt).unwrap(), wt.join(".githooks"));
}

struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn hash_object_matches_git() {
    let tmp = messy();
    let r = tmp.path();
    let file = r.join("sp ace.txt");
    let want = git(r, &["hash-object", "--", "sp ace.txt"]);
    assert_eq!(g::hash_object(r, &file).unwrap().as_str(), want.trim());
}

#[test]
fn worktree_tree_never_includes_receipts_whether_ignored_or_not() {
    for ignored in [false, true] {
        let tmp = TempDir::new();
        let r = tmp.path();
        init(r);
        write(r, "a.ts", "a\n");
        if ignored {
            write(r, ".gitignore", ".garita/receipts/\n");
        }
        commit_all(r, "one");
        let index_before = fs::read(r.join(".git/index")).unwrap();
        let clean = g::worktree_tree(r).unwrap();
        write(r, ".garita/receipts/x.json", "{}\n");
        write(r, ".garita/receipts/run/lint.log", "out\n");
        assert_eq!(g::worktree_tree(r).unwrap(), clean, "ignored: {ignored}");
        let head = g::tree_of_commit(r, "HEAD").unwrap();
        assert!(!g::dirty(r, &head).unwrap(), "ignored: {ignored}");
        assert_eq!(fs::read(r.join(".git/index")).unwrap(), index_before);
        write(r, ".garita/config.yaml", "schema_version: 1\n");
        assert_ne!(
            g::worktree_tree(r).unwrap(),
            clean,
            "other .garita files still count"
        );
    }
}
