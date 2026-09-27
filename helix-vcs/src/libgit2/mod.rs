//! Git diff provider backed by the host app's libgit2. Nothing is linked here on iOS and
//! visionOS: the app links libgit2 once and resolves these symbols at final link.

use anyhow::{bail, Context, Result};
use arc_swap::ArcSwap;
use std::ffi::{c_int, CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::{Arc, Once};

use crate::FileChange;

mod ffi;

#[cfg(test)]
mod test;

macro_rules! owned {
    ($name:ident, $raw:ty, $free:path) => {
        struct $name(*mut $raw);

        impl Drop for $name {
            fn drop(&mut self) {
                if !self.0.is_null() {
                    unsafe { $free(self.0) }
                }
            }
        }
    };
}

owned!(Repo, ffi::git_repository, ffi::git_repository_free);
owned!(Reference, ffi::git_reference, ffi::git_reference_free);
owned!(Object, ffi::git_object, ffi::git_object_free);
owned!(Tree, ffi::git_tree, ffi::git_tree_free);
owned!(TreeEntry, ffi::git_tree_entry, ffi::git_tree_entry_free);
owned!(Blob, ffi::git_blob, ffi::git_blob_free);
owned!(StatusList, ffi::git_status_list, ffi::git_status_list_free);

fn check(code: c_int, what: &str) -> Result<()> {
    if code >= 0 {
        return Ok(());
    }
    let message = unsafe {
        let err = ffi::git_error_last();
        if err.is_null() || (*err).message.is_null() {
            String::new()
        } else {
            CStr::from_ptr((*err).message)
                .to_string_lossy()
                .into_owned()
        }
    };
    bail!("{what} failed ({code}): {message}")
}

fn init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| unsafe {
        ffi::git_libgit2_init();
    });
}

#[cfg(unix)]
fn path_to_cstring(path: &Path) -> Result<CString> {
    use std::os::unix::ffi::OsStrExt;
    Ok(CString::new(path.as_os_str().as_bytes())?)
}

#[cfg(not(unix))]
fn path_to_cstring(path: &Path) -> Result<CString> {
    let path = path.to_str().context("path is not valid UTF-8")?;
    Ok(CString::new(path.replace('\\', "/"))?)
}

#[cfg(unix)]
unsafe fn cstr_to_path(s: *const std::ffi::c_char) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(CStr::from_ptr(s).to_bytes()))
}

#[cfg(not(unix))]
unsafe fn cstr_to_path(s: *const std::ffi::c_char) -> PathBuf {
    PathBuf::from(CStr::from_ptr(s).to_string_lossy().into_owned())
}

fn open_repo(path: &Path) -> Result<Repo> {
    init();
    let path = path_to_cstring(path)?;
    let mut repo = Repo(ptr::null_mut());
    check(
        unsafe { ffi::git_repository_open_ext(&mut repo.0, path.as_ptr(), 0, ptr::null()) },
        "git_repository_open_ext",
    )?;
    Ok(repo)
}

fn workdir(repo: &Repo) -> Option<PathBuf> {
    let dir = unsafe { ffi::git_repository_workdir(repo.0) };
    if dir.is_null() {
        return None;
    }
    let dir = unsafe { cstr_to_path(dir) };
    // libgit2 may report an unresolved path; the files we compare against are canonical.
    Some(dir.canonicalize().unwrap_or(dir))
}

fn head(repo: &Repo) -> Result<Reference> {
    let mut reference = Reference(ptr::null_mut());
    check(
        unsafe { ffi::git_repository_head(&mut reference.0, repo.0) },
        "git_repository_head",
    )?;
    Ok(reference)
}

fn head_commit(head: &Reference) -> Result<Object> {
    let mut commit = Object(ptr::null_mut());
    check(
        unsafe { ffi::git_reference_peel(&mut commit.0, head.0, ffi::GIT_OBJECT_COMMIT) },
        "git_reference_peel",
    )?;
    Ok(commit)
}

fn repo_for_file(file: &Path) -> Result<Repo> {
    let repo_dir = file.parent().context("file has no parent directory")?;
    open_repo(repo_dir).context("failed to open git repo")
}

pub fn get_diff_base(file: &Path) -> Result<Vec<u8>> {
    debug_assert!(!file.exists() || file.is_file());
    debug_assert!(file.is_absolute());
    let file = file.canonicalize().context("resolve symlinks")?;

    let repo = repo_for_file(&file)?;
    let work_dir = workdir(&repo).context("repo has no worktree")?;
    let rel_path = path_to_cstring(file.strip_prefix(&work_dir)?)?;

    let commit = head_commit(&head(&repo)?)?;
    let mut tree = Tree(ptr::null_mut());
    check(
        unsafe { ffi::git_commit_tree(&mut tree.0, commit.0) },
        "git_commit_tree",
    )?;

    let mut entry = TreeEntry(ptr::null_mut());
    check(
        unsafe { ffi::git_tree_entry_bypath(&mut entry.0, tree.0, rel_path.as_ptr()) },
        "file is untracked",
    )?;
    // Trees, symlinks, and submodules have no file contents to diff against.
    match unsafe { ffi::git_tree_entry_filemode(entry.0) } {
        ffi::GIT_FILEMODE_BLOB | ffi::GIT_FILEMODE_BLOB_EXECUTABLE => {}
        mode => bail!("entry at {} is not a file (mode {mode:o})", file.display()),
    }

    let mut blob = Blob(ptr::null_mut());
    check(
        unsafe { ffi::git_blob_lookup(&mut blob.0, repo.0, ffi::git_tree_entry_id(entry.0)) },
        "git_blob_lookup",
    )?;

    // Apply the user's attributes and config (crlf and friends) like a checkout would.
    let mut opts = std::mem::MaybeUninit::<ffi::git_blob_filter_options>::zeroed();
    check(
        unsafe {
            ffi::git_blob_filter_options_init(
                opts.as_mut_ptr(),
                ffi::GIT_BLOB_FILTER_OPTIONS_VERSION,
            )
        },
        "git_blob_filter_options_init",
    )?;
    let mut buf = ffi::git_buf {
        ptr: ptr::null_mut(),
        reserved: 0,
        size: 0,
    };
    let code =
        unsafe { ffi::git_blob_filter(&mut buf, blob.0, rel_path.as_ptr(), opts.as_mut_ptr()) };
    let data = if code >= 0 && !buf.ptr.is_null() {
        unsafe { std::slice::from_raw_parts(buf.ptr as *const u8, buf.size) }.to_vec()
    } else {
        Vec::new()
    };
    unsafe { ffi::git_buf_dispose(&mut buf) };
    check(code, "git_blob_filter")?;
    Ok(data)
}

pub fn get_current_head_name(file: &Path) -> Result<Arc<ArcSwap<Box<str>>>> {
    debug_assert!(!file.exists() || file.is_file());
    debug_assert!(file.is_absolute());
    let file = file.canonicalize().context("resolve symlinks")?;

    let repo = repo_for_file(&file)?;
    let head = head(&repo)?;
    let name = if unsafe { ffi::git_repository_head_detached(repo.0) } == 1 {
        let commit = head_commit(&head)?;
        let hex = unsafe { CStr::from_ptr(ffi::git_oid_tostr_s(ffi::git_object_id(commit.0))) };
        hex.to_string_lossy().chars().take(8).collect::<String>()
    } else {
        unsafe { CStr::from_ptr(ffi::git_reference_shorthand(head.0)) }
            .to_string_lossy()
            .into_owned()
    };

    Ok(Arc::new(ArcSwap::from_pointee(name.into_boxed_str())))
}

/// Emulates the worktree half of `git status`: index against worktree, untracked files
/// listed individually, rename detection on.
pub fn for_each_changed_file(cwd: &Path, f: impl Fn(Result<FileChange>) -> bool) -> Result<()> {
    let repo = open_repo(cwd)?;
    let work_dir = workdir(&repo).context("working tree not found")?;

    let mut opts = std::mem::MaybeUninit::<ffi::git_status_options>::zeroed();
    check(
        unsafe { ffi::git_status_options_init(opts.as_mut_ptr(), ffi::GIT_STATUS_OPTIONS_VERSION) },
        "git_status_options_init",
    )?;
    let mut opts = unsafe { opts.assume_init() };
    opts.show = ffi::GIT_STATUS_SHOW_WORKDIR_ONLY;
    opts.flags = ffi::GIT_STATUS_OPT_INCLUDE_UNTRACKED
        | ffi::GIT_STATUS_OPT_RECURSE_UNTRACKED_DIRS
        | ffi::GIT_STATUS_OPT_RENAMES_INDEX_TO_WORKDIR;
    opts.rename_threshold = 50;

    let mut list = StatusList(ptr::null_mut());
    check(
        unsafe { ffi::git_status_list_new(&mut list.0, repo.0, &opts) },
        "git_status_list_new",
    )?;

    let count = unsafe { ffi::git_status_list_entrycount(list.0) };
    for idx in 0..count {
        let entry = unsafe { ffi::git_status_byindex(list.0, idx) };
        if entry.is_null() {
            continue;
        }
        let (status, delta) = unsafe { ((*entry).status, (*entry).index_to_workdir) };
        let Some(delta) = (unsafe { delta.as_ref() }) else {
            continue;
        };
        let path_of = |file: &ffi::git_diff_file| {
            (!file.path.is_null()).then(|| work_dir.join(unsafe { cstr_to_path(file.path) }))
        };
        let (Some(old_path), Some(new_path)) = (
            path_of(&delta.old_file).or_else(|| path_of(&delta.new_file)),
            path_of(&delta.new_file).or_else(|| path_of(&delta.old_file)),
        ) else {
            continue;
        };

        let change = if status & ffi::GIT_STATUS_CONFLICTED != 0 {
            FileChange::Conflict { path: new_path }
        } else if status & ffi::GIT_STATUS_WT_RENAMED != 0 {
            FileChange::Renamed {
                from_path: old_path,
                to_path: new_path,
            }
        } else if status & ffi::GIT_STATUS_WT_DELETED != 0 {
            FileChange::Deleted { path: old_path }
        } else if status & ffi::GIT_STATUS_WT_NEW != 0 {
            FileChange::Untracked { path: new_path }
        } else if status & (ffi::GIT_STATUS_WT_MODIFIED | ffi::GIT_STATUS_WT_TYPECHANGE) != 0 {
            FileChange::Modified { path: new_path }
        } else {
            continue;
        };
        if !f(Ok(change)) {
            break;
        }
    }

    Ok(())
}
