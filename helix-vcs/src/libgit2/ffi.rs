//! Minimal libgit2 bindings, transcribed from the headers at libgit2 885ad64e4 (the revision
//! libgit2-rootshell ships). That revision's `git_oid` carries a type byte and a 32-byte id,
//! so these layouts do not match libgit2 1.9 releases; re-check them when the pin moves.
#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_int, c_uint};

#[repr(C)]
pub struct git_repository {
    _private: [u8; 0],
}
#[repr(C)]
pub struct git_reference {
    _private: [u8; 0],
}
#[repr(C)]
pub struct git_object {
    _private: [u8; 0],
}
#[repr(C)]
pub struct git_tree {
    _private: [u8; 0],
}
#[repr(C)]
pub struct git_tree_entry {
    _private: [u8; 0],
}
#[repr(C)]
pub struct git_blob {
    _private: [u8; 0],
}
#[repr(C)]
pub struct git_status_list {
    _private: [u8; 0],
}

#[repr(C)]
pub struct git_oid {
    pub kind: u8,
    pub id: [u8; 32],
}

#[repr(C)]
pub struct git_buf {
    pub ptr: *mut c_char,
    pub reserved: usize,
    pub size: usize,
}

#[repr(C)]
pub struct git_error {
    pub message: *const c_char,
    pub klass: c_int,
}

#[repr(C)]
pub struct git_strarray {
    pub strings: *mut *mut c_char,
    pub count: usize,
}

#[repr(C)]
pub struct git_status_options {
    pub version: c_uint,
    pub show: c_uint,
    pub flags: c_uint,
    pub pathspec: git_strarray,
    pub baseline: *mut git_tree,
    pub rename_threshold: u16,
}

#[repr(C)]
pub struct git_diff_file {
    pub id: git_oid,
    pub path: *const c_char,
    pub size: u64,
    pub flags: u32,
    pub mode: u16,
    pub id_abbrev: u16,
}

#[repr(C)]
pub struct git_diff_delta {
    pub status: c_uint,
    pub flags: u32,
    pub similarity: u16,
    pub nfiles: u16,
    pub old_file: git_diff_file,
    pub new_file: git_diff_file,
}

#[repr(C)]
pub struct git_status_entry {
    pub status: c_uint,
    pub head_to_index: *const git_diff_delta,
    pub index_to_workdir: *const git_diff_delta,
}

#[repr(C)]
pub struct git_blob_filter_options {
    pub version: c_int,
    pub flags: u32,
    pub commit_id: *mut git_oid,
    pub attr_commit_id: git_oid,
}

pub const GIT_OBJECT_COMMIT: c_int = 1;

pub const GIT_FILEMODE_BLOB: c_uint = 0o100644;
pub const GIT_FILEMODE_BLOB_EXECUTABLE: c_uint = 0o100755;

pub const GIT_STATUS_WT_NEW: c_uint = 1 << 7;
pub const GIT_STATUS_WT_MODIFIED: c_uint = 1 << 8;
pub const GIT_STATUS_WT_DELETED: c_uint = 1 << 9;
pub const GIT_STATUS_WT_TYPECHANGE: c_uint = 1 << 10;
pub const GIT_STATUS_WT_RENAMED: c_uint = 1 << 11;
pub const GIT_STATUS_CONFLICTED: c_uint = 1 << 15;

pub const GIT_STATUS_OPTIONS_VERSION: c_uint = 1;
pub const GIT_STATUS_SHOW_WORKDIR_ONLY: c_uint = 2;
pub const GIT_STATUS_OPT_INCLUDE_UNTRACKED: c_uint = 1 << 0;
pub const GIT_STATUS_OPT_RECURSE_UNTRACKED_DIRS: c_uint = 1 << 4;
pub const GIT_STATUS_OPT_RENAMES_INDEX_TO_WORKDIR: c_uint = 1 << 8;

pub const GIT_BLOB_FILTER_OPTIONS_VERSION: c_uint = 1;

extern "C" {
    pub fn git_libgit2_init() -> c_int;
    pub fn git_error_last() -> *const git_error;

    pub fn git_repository_open_ext(
        out: *mut *mut git_repository,
        path: *const c_char,
        flags: c_uint,
        ceiling_dirs: *const c_char,
    ) -> c_int;
    pub fn git_repository_free(repo: *mut git_repository);
    pub fn git_repository_workdir(repo: *const git_repository) -> *const c_char;
    pub fn git_repository_head(out: *mut *mut git_reference, repo: *mut git_repository) -> c_int;
    pub fn git_repository_head_detached(repo: *mut git_repository) -> c_int;

    pub fn git_reference_free(reference: *mut git_reference);
    pub fn git_reference_shorthand(reference: *const git_reference) -> *const c_char;
    pub fn git_reference_peel(
        out: *mut *mut git_object,
        reference: *const git_reference,
        kind: c_int,
    ) -> c_int;

    pub fn git_object_free(object: *mut git_object);
    pub fn git_object_id(object: *const git_object) -> *const git_oid;

    // `commit` is a `git_commit`; a peeled `git_object` of kind commit is one.
    pub fn git_commit_tree(out: *mut *mut git_tree, commit: *const git_object) -> c_int;
    pub fn git_tree_free(tree: *mut git_tree);
    pub fn git_tree_entry_bypath(
        out: *mut *mut git_tree_entry,
        root: *const git_tree,
        path: *const c_char,
    ) -> c_int;
    pub fn git_tree_entry_free(entry: *mut git_tree_entry);
    pub fn git_tree_entry_id(entry: *const git_tree_entry) -> *const git_oid;
    pub fn git_tree_entry_filemode(entry: *const git_tree_entry) -> c_uint;

    pub fn git_blob_lookup(
        out: *mut *mut git_blob,
        repo: *mut git_repository,
        id: *const git_oid,
    ) -> c_int;
    pub fn git_blob_free(blob: *mut git_blob);
    pub fn git_blob_filter_options_init(
        opts: *mut git_blob_filter_options,
        version: c_uint,
    ) -> c_int;
    pub fn git_blob_filter(
        out: *mut git_buf,
        blob: *mut git_blob,
        as_path: *const c_char,
        opts: *mut git_blob_filter_options,
    ) -> c_int;
    pub fn git_buf_dispose(buffer: *mut git_buf);

    pub fn git_status_options_init(opts: *mut git_status_options, version: c_uint) -> c_int;
    pub fn git_status_list_new(
        out: *mut *mut git_status_list,
        repo: *mut git_repository,
        opts: *const git_status_options,
    ) -> c_int;
    pub fn git_status_list_entrycount(list: *mut git_status_list) -> usize;
    pub fn git_status_byindex(list: *mut git_status_list, idx: usize) -> *const git_status_entry;
    pub fn git_status_list_free(list: *mut git_status_list);

    pub fn git_oid_tostr_s(oid: *const git_oid) -> *const c_char;
}
