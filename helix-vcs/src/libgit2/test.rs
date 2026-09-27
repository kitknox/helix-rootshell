use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};

use tempfile::TempDir;

use crate::libgit2 as git;
use crate::FileChange;

fn exec_git_cmd(args: &str, git_dir: &Path) {
    let res = Command::new("git")
        .arg("-C")
        .arg(git_dir) // execute the git command in this directory
        .args(args.split_whitespace())
        .env_remove("GIT_DIR")
        .env_remove("GIT_ASKPASS")
        .env_remove("SSH_ASKPASS")
        .env("GIT_TERMINAL_PROMPT", "false")
        .env("GIT_AUTHOR_DATE", "2000-01-01 00:00:00 +0000")
        .env("GIT_AUTHOR_EMAIL", "author@example.com")
        .env("GIT_AUTHOR_NAME", "author")
        .env("GIT_COMMITTER_DATE", "2000-01-02 00:00:00 +0000")
        .env("GIT_COMMITTER_EMAIL", "committer@example.com")
        .env("GIT_COMMITTER_NAME", "committer")
        .env("GIT_CONFIG_COUNT", "2")
        .env("GIT_CONFIG_KEY_0", "commit.gpgsign")
        .env("GIT_CONFIG_VALUE_0", "false")
        .env("GIT_CONFIG_KEY_1", "init.defaultBranch")
        .env("GIT_CONFIG_VALUE_1", "main")
        .output()
        .unwrap_or_else(|_| panic!("`git {args}` failed"));
    if !res.status.success() {
        println!("{}", String::from_utf8_lossy(&res.stdout));
        eprintln!("{}", String::from_utf8_lossy(&res.stderr));
        panic!("`git {args}` failed (see output above)")
    }
}

fn create_commit(repo: &Path, add_modified: bool) {
    if add_modified {
        exec_git_cmd("add -A", repo);
    }
    exec_git_cmd("commit -m message", repo);
}

fn empty_git_repo() -> TempDir {
    let tmp = tempfile::tempdir().expect("create temp dir for git testing");
    exec_git_cmd("init", tmp.path());
    exec_git_cmd("config user.email test@helix.org", tmp.path());
    exec_git_cmd("config user.name helix-test", tmp.path());
    tmp
}

#[test]
fn missing_file() {
    let temp_git = empty_git_repo();
    let file = temp_git.path().join("file.txt");
    File::create(&file).unwrap().write_all(b"foo").unwrap();

    assert!(git::get_diff_base(&file).is_err());
}

#[test]
fn unmodified_file() {
    let temp_git = empty_git_repo();
    let file = temp_git.path().join("file.txt");
    let contents = b"foo".as_slice();
    File::create(&file).unwrap().write_all(contents).unwrap();
    create_commit(temp_git.path(), true);
    assert_eq!(git::get_diff_base(&file).unwrap(), Vec::from(contents));
}

#[test]
fn modified_file() {
    let temp_git = empty_git_repo();
    let file = temp_git.path().join("file.txt");
    let contents = b"foo".as_slice();
    File::create(&file).unwrap().write_all(contents).unwrap();
    create_commit(temp_git.path(), true);
    File::create(&file).unwrap().write_all(b"bar").unwrap();

    assert_eq!(git::get_diff_base(&file).unwrap(), Vec::from(contents));
}

/// Test that `get_file_head` does not return content for a directory.
/// This is important to correctly cover cases where a directory is removed and replaced by a file.
/// If the contents of the directory object were returned a diff between a path and the directory children would be produced.
#[test]
fn directory() {
    let temp_git = empty_git_repo();
    let dir = temp_git.path().join("file.txt");
    std::fs::create_dir(&dir).expect("");
    let file = dir.join("file.txt");
    let contents = b"foo".as_slice();
    File::create(file).unwrap().write_all(contents).unwrap();

    create_commit(temp_git.path(), true);

    std::fs::remove_dir_all(&dir).unwrap();
    File::create(&dir).unwrap().write_all(b"bar").unwrap();
    assert!(git::get_diff_base(&dir).is_err());
}

/// Test that `get_diff_base` resolves symlinks so that the same diff base is
/// used as the target file.
///
/// This is important to correctly cover cases where a symlink is removed and
/// replaced by a file. If the contents of the symlink object were returned
/// a diff between a literal file path and the actual file content would be
/// produced (bad ui).
#[cfg(any(unix, windows))]
#[test]
fn symlink() {
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    #[cfg(not(unix))]
    use std::os::windows::fs::symlink_file as symlink;

    let temp_git = empty_git_repo();
    let file = temp_git.path().join("file.txt");
    let contents = Vec::from(b"foo");
    File::create(&file).unwrap().write_all(&contents).unwrap();
    let file_link = temp_git.path().join("file_link.txt");

    symlink("file.txt", &file_link).unwrap();
    create_commit(temp_git.path(), true);

    assert_eq!(git::get_diff_base(&file_link).unwrap(), contents);
    assert_eq!(git::get_diff_base(&file).unwrap(), contents);
}

/// Test that `get_diff_base` returns content when the file is a symlink to
/// another file that is in a git repo, but the symlink itself is not.
#[cfg(any(unix, windows))]
#[test]
fn symlink_to_git_repo() {
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    #[cfg(not(unix))]
    use std::os::windows::fs::symlink_file as symlink;

    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let temp_git = empty_git_repo();

    let file = temp_git.path().join("file.txt");
    let contents = Vec::from(b"foo");
    File::create(&file).unwrap().write_all(&contents).unwrap();
    create_commit(temp_git.path(), true);

    let file_link = temp_dir.path().join("file_link.txt");
    symlink(&file, &file_link).unwrap();

    assert_eq!(git::get_diff_base(&file_link).unwrap(), contents);
    assert_eq!(git::get_diff_base(&file).unwrap(), contents);
}

#[test]
fn crlf_attributes() {
    let temp_git = empty_git_repo();
    File::create(temp_git.path().join(".gitattributes"))
        .unwrap()
        .write_all(b"*.txt text eol=crlf\n")
        .unwrap();
    let file = temp_git.path().join("file.txt");
    File::create(&file).unwrap().write_all(b"a\nb\n").unwrap();
    create_commit(temp_git.path(), true);

    assert_eq!(git::get_diff_base(&file).unwrap(), b"a\r\nb\r\n".to_vec());
}

#[test]
fn head_name() {
    let temp_git = empty_git_repo();
    let file = temp_git.path().join("file.txt");
    File::create(&file).unwrap().write_all(b"foo").unwrap();
    create_commit(temp_git.path(), true);
    let name = git::get_current_head_name(&file)
        .unwrap()
        .load()
        .to_string();
    assert_eq!(name, "main");

    exec_git_cmd("checkout --detach", temp_git.path());
    let name = git::get_current_head_name(&file)
        .unwrap()
        .load()
        .to_string();
    assert_eq!(name.len(), 8);
    assert!(name.chars().all(|c| c.is_ascii_hexdigit()));
}

fn changed_files(dir: &Path) -> Vec<(&'static str, PathBuf, Option<PathBuf>)> {
    let changes = Mutex::new(Vec::new());
    git::for_each_changed_file(dir, |change| {
        let entry = match change.unwrap() {
            FileChange::Untracked { path } => ("untracked", path, None),
            FileChange::Modified { path } => ("modified", path, None),
            FileChange::Conflict { path } => ("conflict", path, None),
            FileChange::Deleted { path } => ("deleted", path, None),
            FileChange::Renamed { from_path, to_path } => ("renamed", to_path, Some(from_path)),
        };
        changes.lock().unwrap().push(entry);
        true
    })
    .unwrap();
    let mut changes = changes.into_inner().unwrap();
    changes.sort();
    changes
}

#[test]
fn status() {
    let temp_git = empty_git_repo();
    let root = temp_git.path().canonicalize().unwrap();
    let write = |name: &str, contents: &[u8]| {
        File::create(root.join(name))
            .unwrap()
            .write_all(contents)
            .unwrap()
    };
    write("modified.txt", b"one");
    write("deleted.txt", b"two");
    write("renamed.txt", b"three\nthree\nthree\nthree\n");
    create_commit(&root, true);

    write("modified.txt", b"changed");
    std::fs::remove_file(root.join("deleted.txt")).unwrap();
    std::fs::rename(root.join("renamed.txt"), root.join("moved.txt")).unwrap();
    std::fs::create_dir(root.join("dir")).unwrap();
    write("dir/untracked.txt", b"new");

    assert_eq!(
        changed_files(&root),
        vec![
            ("deleted", root.join("deleted.txt"), None),
            ("modified", root.join("modified.txt"), None),
            (
                "renamed",
                root.join("moved.txt"),
                Some(root.join("renamed.txt"))
            ),
            ("untracked", root.join("dir/untracked.txt"), None),
        ]
    );
}
