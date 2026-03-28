//! gix CLI module — provides the `gix_main` entry point for ios_system.
//!
//! This was originally in the standalone `gitoxide_ios` cdylib crate.
//! It is now part of helix-ios so that the gix library is compiled once,
//! shared between helix's diff gutter and the gix CLI command.
//!
//! I/O is routed through ios_system's thread-local FILE* streams.

use std::ffi::{c_char, c_int, CStr, OsString};
use std::io::{BufWriter, Write};
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;
use std::str::FromStr;
#[allow(unused_imports)]
use std::sync::atomic::AtomicBool;
#[allow(unused_imports)]
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use gix::bstr::BString;
use gitoxide_core as core;

// ---------------------------------------------------------------------------
// FFI: ios_system thread-local stream accessors
// ---------------------------------------------------------------------------
extern "C" {
    #[allow(dead_code)]
    fn ios_stdin() -> *mut libc::FILE;
    fn ios_stdout() -> *mut libc::FILE;
    fn ios_stderr() -> *mut libc::FILE;
}

// ---------------------------------------------------------------------------
// I/O wrapper: writes to a borrowed file descriptor without closing it on drop
// ---------------------------------------------------------------------------
struct IosWriter(c_int);

impl Write for IosWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let ret = unsafe { libc::write(self.0, buf.as_ptr() as *const libc::c_void, buf.len()) };
        if ret < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(ret as usize)
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Entry point — called via re-export in lib.rs
// ---------------------------------------------------------------------------
pub fn gix_main(argc: c_int, argv: *const *const c_char) -> c_int {
    // Convert C argv to Rust OsStrings.
    let args: Vec<OsString> = (0..argc)
        .map(|i| unsafe {
            let ptr = *argv.offset(i as isize);
            let bytes = CStr::from_ptr(ptr).to_bytes().to_vec();
            OsString::from_vec(bytes)
        })
        .collect();

    // Obtain ios_system's thread-local I/O file descriptors.
    let stdout_fd = unsafe { libc::fileno(ios_stdout()) };
    let stderr_fd = unsafe { libc::fileno(ios_stderr()) };

    let mut out = BufWriter::new(IosWriter(stdout_fd));
    let mut err = IosWriter(stderr_fd);

    let result = run(args, &mut out, &mut err);
    let _ = out.flush();

    match result {
        Ok(()) => 0,
        Err(e) => {
            let _ = writeln!(err, "{:#}", e);
            1
        }
    }
}

// ---------------------------------------------------------------------------
// CLI argument definitions (simplified from gitoxide's plumbing/options)
// ---------------------------------------------------------------------------

/// Custom value parser: OsString → BString
#[derive(Clone)]
struct AsBString;

impl clap::builder::TypedValueParser for AsBString {
    type Value = BString;
    fn parse_ref(
        &self,
        _cmd: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        gix::env::os_str_to_bstring(value)
            .ok_or_else(|| clap::Error::new(clap::error::ErrorKind::InvalidUtf8))
    }
}

/// Custom value parser: String → core::OutputFormat
#[derive(Clone)]
struct AsOutputFormat;

impl clap::builder::TypedValueParser for AsOutputFormat {
    type Value = core::OutputFormat;
    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        clap::builder::StringValueParser::new()
            .try_map(|s| core::OutputFormat::from_str(&s))
            .parse_ref(cmd, arg, value)
    }
    fn possible_values(&self) -> Option<Box<dyn Iterator<Item = clap::builder::PossibleValue> + '_>> {
        Some(Box::new(
            core::OutputFormat::variants()
                .iter()
                .map(clap::builder::PossibleValue::new),
        ))
    }
}

/// Custom value parser: String → gix::hash::Kind
#[derive(Clone)]
struct AsHashKind;

impl clap::builder::TypedValueParser for AsHashKind {
    type Value = gix::hash::Kind;
    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        clap::builder::StringValueParser::new()
            .try_map(|s| gix::hash::Kind::from_str(&s))
            .parse_ref(cmd, arg, value)
    }
}

/// Custom value parser: OsString → gix::pathspec::Pattern (used for pathspec args)
#[derive(Clone)]
#[allow(dead_code)]
struct AsPathSpec;

impl clap::builder::TypedValueParser for AsPathSpec {
    type Value = gix::pathspec::Pattern;
    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        clap::builder::OsStringValueParser::new()
            .try_map(|arg| {
                let p: &std::path::Path = arg.as_os_str().as_ref();
                let defaults = gix::pathspec::Defaults::from_environment(&mut |n| std::env::var_os(n))
                    .unwrap_or_default();
                gix::pathspec::parse(gix::path::into_bstr(p).as_ref(), defaults)
            })
            .parse_ref(cmd, arg, value)
    }
}

/// Custom value parser: validate pathspec then return as BString
#[derive(Clone)]
struct CheckPathSpec;

impl clap::builder::TypedValueParser for CheckPathSpec {
    type Value = BString;
    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        clap::builder::OsStringValueParser::new()
            .try_map(|arg| -> Result<_, gix::pathspec::parse::Error> {
                let bstr = gix::path::into_bstr(std::path::PathBuf::from(arg));
                gix::pathspec::parse(bstr.as_ref(), Default::default())?;
                Ok(bstr.into_owned())
            })
            .parse_ref(cmd, arg, value)
    }
}

/// Custom value parser: rename fraction (e.g. "50%" or "5" → 0.5)
#[derive(Clone)]
struct ParseRenameFraction;

impl clap::builder::TypedValueParser for ParseRenameFraction {
    type Value = f32;
    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        clap::builder::StringValueParser::new()
            .try_map(|s: String| -> Result<_, Box<dyn std::error::Error + Send + Sync>> {
                if s.ends_with('%') {
                    let val = u32::from_str(&s[..s.len() - 1])?;
                    Ok(val as f32 / 100.0)
                } else {
                    let val = u32::from_str(&s)?;
                    let num = format!("0.{val}");
                    Ok(f32::from_str(&num)?)
                }
            })
            .parse_ref(cmd, arg, value)
    }
}

/// Custom value parser: String → gix::date::Time
#[derive(Clone)]
struct AsTime;

impl clap::builder::TypedValueParser for AsTime {
    type Value = gix::date::Time;
    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        clap::builder::StringValueParser::new()
            .try_map(|s| {
                gix::date::parse(&s, Some(std::time::SystemTime::now()))
                    .map_err(gix::Exn::into_inner)
            })
            .parse_ref(cmd, arg, value)
    }
}

/// Custom value parser: BString → gix::refs::PartialName
#[derive(Clone)]
struct AsPartialRefName;

impl clap::builder::TypedValueParser for AsPartialRefName {
    type Value = gix::refs::PartialName;
    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        AsBString
            .try_map(gix::refs::PartialName::try_from)
            .parse_ref(cmd, arg, value)
    }
}

/// Custom value parser: "start,end" → RangeInclusive<u32>
#[derive(Clone)]
struct AsRange;

impl clap::builder::TypedValueParser for AsRange {
    type Value = std::ops::RangeInclusive<u32>;
    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        clap::builder::StringValueParser::new()
            .try_map(|s| -> Result<_, Box<dyn std::error::Error + Send + Sync>> {
                let (start, end) = s
                    .split_once(',')
                    .ok_or("expected format: start,end")?;
                let start = u32::from_str(start)?;
                let end = u32::from_str(end)?;
                if start <= end {
                    Ok(start..=end)
                } else {
                    Err("start must be <= end".into())
                }
            })
            .parse_ref(cmd, arg, value)
    }
}

// ---------------------------------------------------------------------------
// Top-level Args
// ---------------------------------------------------------------------------
#[derive(Debug, Parser)]
#[command(name = "gix", about = "The git underworld", version = env!("CARGO_PKG_VERSION"))]
#[command(subcommand_required = true, arg_required_else_help = true)]
struct Args {
    /// The repository to access.
    #[arg(short = 'r', long, default_value = ".")]
    repository: PathBuf,

    /// Add configuration values (key=value).
    #[arg(long, short = 'c', value_parser = AsBString)]
    config: Vec<BString>,

    /// The amount of threads to use (0 = no limit).
    #[arg(long, short = 't')]
    threads: Option<usize>,

    /// Display verbose messages and progress information.
    #[arg(long, short = 'v')]
    verbose: bool,

    /// Turn off default verbose display.
    #[arg(long, conflicts_with = "verbose")]
    no_verbose: bool,

    /// Strict configuration mode.
    #[arg(long, short = 's')]
    strict: bool,

    /// Output format for statistics (human or json).
    #[arg(long, short = 'f', default_value = "human", value_parser = AsOutputFormat)]
    format: core::OutputFormat,

    /// Object hash algorithm.
    #[arg(long, default_value_t = gix::hash::Kind::default(), value_parser = AsHashKind)]
    object_hash: gix::hash::Kind,

    #[command(subcommand)]
    cmd: Subcommands,
}

// ---------------------------------------------------------------------------
// Subcommands
// ---------------------------------------------------------------------------
#[derive(Debug, clap::Subcommand)]
enum Subcommands {
    /// Print paths relevant to the Git installation.
    Env,
    /// Compute repository status similar to `git status`.
    Status {
        /// Show ignored files.
        #[arg(long)]
        ignored: Option<Option<StatusIgnored>>,
        /// Status display format.
        #[arg(long, short = 'f')]
        format: Option<StatusFormat>,
        /// Print statistics.
        #[arg(long, short = 's')]
        statistics: bool,
        /// Submodule handling.
        #[arg(long)]
        submodules: Option<StatusSubmodules>,
        /// Don't write back changed index.
        #[arg(long)]
        no_write: bool,
        /// Enable rename tracking.
        #[arg(long, value_parser = ParseRenameFraction)]
        index_worktree_renames: Option<Option<f32>>,
        /// Pathspec patterns.
        #[arg(value_parser = CheckPathSpec)]
        pathspec: Vec<BString>,
    },
    /// Show commit log.
    Log {
        /// Path to show log for.
        #[arg(value_parser = AsBString)]
        pathspec: Option<BString>,
    },
    /// Show diffs.
    #[command(subcommand)]
    Diff(DiffSubcommands),
    /// Clone a repository.
    Clone {
        /// Output handshake info.
        #[arg(long, short = 'H')]
        handshake_info: bool,
        /// Create a bare clone.
        #[arg(long)]
        bare: bool,
        /// Don't clone tags.
        #[arg(long)]
        no_tags: bool,
        /// Fetch depth.
        #[arg(long)]
        depth: Option<std::num::NonZeroU32>,
        /// The remote URL.
        remote: OsString,
        /// Reference to check out.
        #[arg(long = "ref", value_parser = AsPartialRefName, value_name = "REF_NAME")]
        ref_name: Option<gix::refs::PartialName>,
        /// Target directory.
        directory: Option<PathBuf>,
    },
    /// Fetch from a remote.
    Fetch {
        /// Dry run.
        #[arg(long, short = 'n')]
        dry_run: bool,
        /// Output handshake info.
        #[arg(long, short = 'H')]
        handshake_info: bool,
        /// Print negotiation statistics.
        #[arg(long, short = 's')]
        negotiation_info: bool,
        /// Fetch depth.
        #[arg(long)]
        depth: Option<std::num::NonZeroU32>,
        /// Deepen by N commits.
        #[arg(long)]
        deepen: Option<u32>,
        /// Remove shallow boundary.
        #[arg(long)]
        unshallow: bool,
        /// Remote name or URL.
        #[arg(long, short = 'r')]
        remote: Option<String>,
        /// Ref-specs to fetch.
        #[arg(value_parser = AsBString)]
        ref_spec: Vec<BString>,
    },
    /// List or query configuration.
    Config {
        /// Filter by section/subsection glob.
        #[arg(value_parser = AsBString)]
        filter: Vec<BString>,
    },
    /// Interact with branches.
    #[command(subcommand)]
    Branch(BranchSubcommands),
    /// Interact with tags.
    #[command(subcommand)]
    Tag(TagSubcommands),
    /// Blame lines in a file.
    Blame {
        /// Print statistics.
        #[arg(long, short = 's')]
        statistics: bool,
        /// The file to blame.
        file: OsString,
        /// Blame line range (1-based, inclusive): start,end.
        #[arg(short = 'L', value_parser = AsRange, action = clap::ArgAction::Append)]
        ranges: Vec<std::ops::RangeInclusive<u32>>,
        /// Don't consider commits before this date.
        #[arg(long, value_parser = AsTime, value_name = "DATE")]
        since: Option<gix::date::Time>,
    },
    /// Show an object (like `git cat-file -p`).
    Cat {
        /// The object to print.
        revspec: String,
    },
    /// Interact with commits.
    #[command(subcommand)]
    Commit(CommitSubcommands),
    /// Interact with tree objects.
    #[command(subcommand)]
    Tree(TreeSubcommands),
    /// Check if the repository is clean (exit 0 if clean, 1 if dirty).
    IsClean,
    /// Check if the repository has changes (exit 0 if dirty, 1 if clean).
    IsChanged,
    /// Interact with the remote.
    Remote {
        /// Remote name or URL.
        #[arg(long, short = 'n')]
        name: Option<String>,
        /// Output handshake info.
        #[arg(long, short = 'H')]
        handshake_info: bool,
        #[command(subcommand)]
        cmd: RemoteSubcommands,
    },
    /// Interact with the index.
    #[command(subcommand)]
    Index(IndexSubcommands),
    /// Interact with the mailmap.
    #[command(subcommand)]
    Mailmap(MailmapSubcommands),
    /// Interact with the object database.
    #[command(subcommand)]
    Odb(OdbSubcommands),
    /// Interact with worktrees.
    #[command(subcommand)]
    Worktree(WorktreeSubcommands),
    /// Interact with submodules.
    #[command(subcommand)]
    Submodule(SubmoduleSubcommands),
    /// Compute merge-base.
    MergeBase {
        /// First revision.
        first: String,
        /// Other revisions.
        others: Vec<String>,
    },
    /// Check for missing objects.
    Fsck {
        /// Revspec to start from.
        spec: Option<String>,
    },
}

// -- Sub-enums for nested subcommands --

#[derive(Debug, clap::Subcommand)]
enum DiffSubcommands {
    /// Diff two trees.
    Tree {
        #[arg(value_parser = AsBString)]
        old_treeish: BString,
        #[arg(value_parser = AsBString)]
        new_treeish: BString,
    },
    /// Diff two file versions.
    File {
        #[arg(value_parser = AsBString)]
        old_revspec: BString,
        #[arg(value_parser = AsBString)]
        new_revspec: BString,
    },
}

#[derive(Debug, clap::Subcommand)]
enum BranchSubcommands {
    /// List branches.
    List {
        /// Include remote-tracking branches.
        #[arg(long, short = 'a')]
        all: bool,
    },
}

#[derive(Debug, clap::Subcommand)]
enum TagSubcommands {
    /// List all tags.
    List,
}

#[derive(Debug, clap::Subcommand)]
enum CommitSubcommands {
    /// Describe a commit using the closest tag.
    Describe {
        #[arg(long, short = 't', conflicts_with = "all_refs")]
        annotated_tags: bool,
        #[arg(long, short = 'a', conflicts_with = "annotated_tags")]
        all_refs: bool,
        #[arg(long, short = 'f')]
        first_parent: bool,
        #[arg(long, short = 'l')]
        long: bool,
        #[arg(long, short = 's')]
        statistics: bool,
        #[arg(long, short = 'c', default_value = "10")]
        max_candidates: usize,
        #[arg(long)]
        always: bool,
        #[arg(short = 'd', long)]
        dirty_suffix: Option<Option<String>>,
        rev_spec: Option<String>,
    },
    /// Verify a commit signature.
    Verify {
        rev_spec: Option<String>,
    },
}

#[derive(Debug, clap::Subcommand)]
enum TreeSubcommands {
    /// Print entries in a tree.
    Entries {
        #[arg(long, short = 'r')]
        recursive: bool,
        #[arg(long, short = 'e')]
        extended: bool,
        treeish: Option<String>,
    },
    /// Show tree information.
    Info {
        #[arg(long, short = 'e')]
        extended: bool,
        treeish: Option<String>,
    },
}

#[derive(Debug, clap::Subcommand)]
enum RemoteSubcommands {
    /// Print all references on the remote.
    Refs,
    /// Print references filtered through ref-specs.
    RefMap {
        #[arg(long, short = 'u')]
        show_unmapped_remote_refs: bool,
        #[arg(value_parser = AsBString)]
        ref_spec: Vec<BString>,
    },
}

#[derive(Debug, clap::Subcommand)]
enum IndexSubcommands {
    /// Print index entries.
    Entries {
        #[arg(long)]
        no_attributes: bool,
        #[arg(long, short = 'i', conflicts_with = "no_attributes")]
        attributes_from_index: bool,
        #[arg(long, short = 'r')]
        recurse_submodules: bool,
        #[arg(long, short = 's')]
        statistics: bool,
        #[arg(value_parser = CheckPathSpec)]
        pathspec: Vec<BString>,
    },
}

#[derive(Debug, clap::Subcommand)]
enum MailmapSubcommands {
    /// Print mailmap entries.
    Entries,
    /// Check contacts against the mailmap.
    Check {
        contacts: Vec<BString>,
    },
}

#[derive(Debug, clap::Subcommand)]
enum OdbSubcommands {
    /// Print all object names.
    Entries,
    /// Show object database info.
    Info,
    /// Show object database statistics.
    Stats {
        #[arg(long)]
        extra_header_lookup: bool,
    },
}

#[derive(Debug, clap::Subcommand)]
enum WorktreeSubcommands {
    /// List worktrees.
    List,
}

#[derive(Debug, clap::Subcommand)]
enum SubmoduleSubcommands {
    /// List submodules.
    List {
        #[arg(short = 'd', long)]
        dirty_suffix: Option<Option<String>>,
    },
}

// -- Value enums for status options --

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, clap::ValueEnum)]
enum StatusFormat {
    #[default]
    Simplified,
    PorcelainV2,
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, clap::ValueEnum)]
enum StatusIgnored {
    #[default]
    Collapsed,
    Matching,
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, clap::ValueEnum)]
enum StatusSubmodules {
    #[default]
    All,
    RefChange,
    Modifications,
    None,
}

// ---------------------------------------------------------------------------
// Repository opening — matches gitoxide's Mode enum pattern
// ---------------------------------------------------------------------------
enum RepoMode {
    Strict,
    StrictWithGitInstallConfig,
    Lenient,
    LenientWithGitInstallConfig,
}

fn open_repo(
    path: &std::path::Path,
    config: &[BString],
    strict: bool,
    mut mode: RepoMode,
) -> Result<gix::Repository> {
    let mut mapping: gix::sec::trust::Mapping<gix::open::Options> = Default::default();

    if !config.is_empty() {
        mode = match mode {
            RepoMode::Lenient => RepoMode::Strict,
            RepoMode::LenientWithGitInstallConfig => RepoMode::StrictWithGitInstallConfig,
            other => other,
        };
    }

    let strict_toggle =
        matches!(mode, RepoMode::Strict | RepoMode::StrictWithGitInstallConfig) || strict;
    mapping.full = mapping.full.strict_config(strict_toggle);
    mapping.reduced = mapping.reduced.strict_config(strict_toggle);

    let git_installation = matches!(
        mode,
        RepoMode::StrictWithGitInstallConfig | RepoMode::LenientWithGitInstallConfig
    );

    let config_clone = config.to_vec();
    let config_clone2 = config.to_vec();
    let to_match_settings = move |mut opts: gix::open::Options| {
        opts.permissions.config.git_binary = git_installation;
        opts.permissions.attributes.git_binary = git_installation;
        if config_clone.is_empty() {
            opts
        } else {
            opts.cli_overrides(config_clone.clone())
        }
    };
    mapping.full.modify(to_match_settings.clone());
    mapping.reduced.modify(to_match_settings);

    let mut repo = gix::ThreadSafeRepository::discover_with_environment_overrides_opts(
        path,
        Default::default(),
        mapping,
    )
    .map(gix::Repository::from)?;

    if !config_clone2.is_empty() {
        repo.config_snapshot_mut()
            .append_config(config_clone2.iter(), gix::config::Source::Cli)
            .context("Unable to parse command-line configuration")?;
    }

    // Enable precious file parsing by default.
    {
        let mut config_mut = repo.config_snapshot_mut();
        if config_mut
            .boolean(gix::config::tree::Gitoxide::PARSE_PRECIOUS)
            .is_none()
        {
            config_mut.set_raw_value(&gix::config::tree::Gitoxide::PARSE_PRECIOUS, "true")?;
        }
    }

    Ok(repo)
}

// ---------------------------------------------------------------------------
// Main dispatch
// ---------------------------------------------------------------------------
fn run(args: Vec<OsString>, out: &mut dyn Write, err: &mut dyn Write) -> Result<()> {
    let cli = Args::try_parse_from(args)?;

    let format = cli.format;
    let thread_limit = cli.threads;
    let config = cli.config.clone();
    let repo_path = cli.repository.clone();
    let strict = cli.strict;
    let _auto_verbose = !cli.verbose && !cli.no_verbose;

    // No-op progress (iOS has no terminal TUI).
    let no_progress =
        || gix_features::progress::DoOrDiscard::from(None::<prodash::tree::Item>);

    let _should_interrupt = Arc::new(AtomicBool::new(false));

    // Helper closures for opening repositories with different trust levels.
    let repo = |mode: RepoMode| open_repo(&repo_path, &config, strict, mode);

    match cli.cmd {
        // -- env --
        Subcommands::Env => {
            core::env(out, format)?;
        }

        // -- status --
        Subcommands::Status {
            ignored,
            format: status_format,
            statistics,
            submodules,
            no_write,
            index_worktree_renames,
            pathspec,
        } => {
            core::repository::status::show(
                repo(RepoMode::Lenient)?,
                pathspec,
                out,
                err,
                no_progress(),
                core::repository::status::Options {
                    format: match status_format.unwrap_or_default() {
                        StatusFormat::Simplified => core::repository::status::Format::Simplified,
                        StatusFormat::PorcelainV2 => core::repository::status::Format::PorcelainV2,
                    },
                    ignored: ignored.map(|ig| match ig.unwrap_or_default() {
                        StatusIgnored::Matching => core::repository::status::Ignored::Matching,
                        StatusIgnored::Collapsed => core::repository::status::Ignored::Collapsed,
                    }),
                    output_format: format,
                    statistics,
                    thread_limit: thread_limit.or(Some(3)),
                    allow_write: !no_write,
                    index_worktree_renames: index_worktree_renames
                        .map(|pct| pct.unwrap_or(0.5)),
                    submodules: submodules.map(|s| match s {
                        StatusSubmodules::All => core::repository::status::Submodules::All,
                        StatusSubmodules::RefChange => {
                            core::repository::status::Submodules::RefChange
                        }
                        StatusSubmodules::Modifications => {
                            core::repository::status::Submodules::Modifications
                        }
                        StatusSubmodules::None => core::repository::status::Submodules::None,
                    }),
                },
            )?;
        }

        // -- log --
        Subcommands::Log { pathspec } => {
            core::repository::log::log(repo(RepoMode::Lenient)?, out, pathspec)?;
        }

        // -- diff --
        Subcommands::Diff(sub) => match sub {
            DiffSubcommands::Tree {
                old_treeish,
                new_treeish,
            } => {
                core::repository::diff::tree(
                    repo(RepoMode::Lenient)?,
                    out,
                    old_treeish,
                    new_treeish,
                )?;
            }
            DiffSubcommands::File {
                old_revspec,
                new_revspec,
            } => {
                core::repository::diff::file(
                    repo(RepoMode::Lenient)?,
                    out,
                    old_revspec,
                    new_revspec,
                )?;
            }
        },

        // -- clone --
        Subcommands::Clone {
            handshake_info,
            bare,
            no_tags,
            depth,
            remote,
            ref_name,
            directory,
        } => {
            let shallow = if let Some(d) = depth {
                gix::remote::fetch::Shallow::DepthAtRemote(d)
            } else {
                gix::remote::fetch::Shallow::default()
            };
            let opts = core::repository::clone::Options {
                format,
                bare,
                handshake_info,
                no_tags,
                ref_name,
                shallow,
            };
            core::repository::clone(
                remote,
                directory,
                config.clone(),
                no_progress(),
                out,
                err,
                opts,
            )?;
        }

        // -- fetch --
        Subcommands::Fetch {
            dry_run,
            handshake_info,
            negotiation_info,
            depth,
            deepen,
            unshallow,
            remote,
            ref_spec,
        } => {
            let shallow = if let Some(d) = depth {
                gix::remote::fetch::Shallow::DepthAtRemote(d)
            } else if let Some(d) = deepen {
                gix::remote::fetch::Shallow::Deepen(d)
            } else if unshallow {
                gix::remote::fetch::Shallow::undo()
            } else {
                gix::remote::fetch::Shallow::default()
            };
            let opts = core::repository::fetch::Options {
                format,
                dry_run,
                remote,
                handshake_info,
                negotiation_info,
                open_negotiation_graph: None,
                shallow,
                ref_specs: ref_spec,
            };
            core::repository::fetch(
                repo(RepoMode::LenientWithGitInstallConfig)?,
                no_progress(),
                out,
                err,
                opts,
            )?;
        }

        // -- config --
        Subcommands::Config { filter } => {
            core::repository::config::list(
                repo(RepoMode::LenientWithGitInstallConfig)?,
                filter,
                config.clone(),
                format,
                out,
            )?;
        }

        // -- branch --
        Subcommands::Branch(sub) => match sub {
            BranchSubcommands::List { all } => {
                let kind = if all {
                    core::repository::branch::list::Kind::All
                } else {
                    core::repository::branch::list::Kind::Local
                };
                core::repository::branch::list(
                    repo(RepoMode::Lenient)?,
                    out,
                    format,
                    core::repository::branch::list::Options { kind },
                )?;
            }
        },

        // -- tag --
        Subcommands::Tag(sub) => match sub {
            TagSubcommands::List => {
                core::repository::tag::list(repo(RepoMode::Lenient)?, out, format)?;
            }
        },

        // -- blame --
        Subcommands::Blame {
            statistics,
            file,
            ranges,
            since,
        } => {
            let r = repo(RepoMode::Lenient)?;
            let diff_algorithm = r.diff_algorithm()?;
            core::repository::blame::blame_file(
                r,
                &file,
                gix::blame::Options {
                    diff_algorithm,
                    ranges: gix::blame::BlameRanges::from_one_based_inclusive_ranges(ranges)?,
                    since,
                    rewrites: Some(gix::diff::Rewrites::default()),
                    debug_track_path: false,
                },
                out,
                if statistics { Some(err) } else { None },
            )?;
        }

        // -- cat --
        Subcommands::Cat { revspec } => {
            core::repository::cat(repo(RepoMode::Lenient)?, &revspec, out)?;
        }

        // -- commit --
        Subcommands::Commit(sub) => match sub {
            CommitSubcommands::Describe {
                annotated_tags,
                all_refs,
                first_parent,
                long,
                statistics,
                max_candidates,
                always,
                dirty_suffix,
                rev_spec,
            } => {
                core::repository::commit::describe(
                    repo(RepoMode::Strict)?,
                    rev_spec.as_deref(),
                    out,
                    err,
                    core::repository::commit::describe::Options {
                        all_tags: !annotated_tags,
                        all_refs,
                        long_format: long,
                        first_parent,
                        statistics,
                        max_candidates,
                        always,
                        dirty_suffix: dirty_suffix
                            .map(|s| s.unwrap_or_else(|| "dirty".to_string())),
                    },
                )?;
            }
            CommitSubcommands::Verify { rev_spec } => {
                core::repository::commit::verify(
                    repo(RepoMode::Lenient)?,
                    rev_spec.as_deref(),
                )?;
            }
        },

        // -- tree --
        Subcommands::Tree(sub) => match sub {
            TreeSubcommands::Entries {
                recursive,
                extended,
                treeish,
            } => {
                core::repository::tree::entries(
                    repo(RepoMode::Strict)?,
                    treeish.as_deref(),
                    recursive,
                    extended,
                    format,
                    out,
                )?;
            }
            TreeSubcommands::Info { extended, treeish } => {
                core::repository::tree::info(
                    repo(RepoMode::Strict)?,
                    treeish.as_deref(),
                    extended,
                    format,
                    out,
                    err,
                )?;
            }
        },

        // -- is-clean / is-changed --
        Subcommands::IsClean => {
            core::repository::dirty::check(
                repo(RepoMode::Lenient)?,
                core::repository::dirty::Mode::IsClean,
                out,
                format,
            )?;
        }
        Subcommands::IsChanged => {
            core::repository::dirty::check(
                repo(RepoMode::Lenient)?,
                core::repository::dirty::Mode::IsDirty,
                out,
                format,
            )?;
        }

        // -- remote --
        Subcommands::Remote {
            name,
            handshake_info,
            cmd,
        } => {
            let kind = match cmd {
                RemoteSubcommands::Refs => core::repository::remote::refs::Kind::Remote,
                RemoteSubcommands::RefMap {
                    ref_spec,
                    show_unmapped_remote_refs,
                } => core::repository::remote::refs::Kind::Tracking {
                    ref_specs: ref_spec,
                    show_unmapped_remote_refs,
                },
            };
            let context = core::repository::remote::refs::Options {
                name_or_url: name,
                format,
                handshake_info,
            };
            core::repository::remote::refs(
                repo(RepoMode::LenientWithGitInstallConfig)?,
                kind,
                no_progress(),
                out,
                err,
                context,
            )?;
        }

        // -- index --
        Subcommands::Index(sub) => match sub {
            IndexSubcommands::Entries {
                no_attributes,
                attributes_from_index,
                recurse_submodules,
                statistics,
                pathspec,
            } => {
                core::repository::index::entries(
                    repo(RepoMode::Lenient)?,
                    pathspec,
                    out,
                    err,
                    core::repository::index::entries::Options {
                        format,
                        simple: true,
                        attributes: if no_attributes {
                            None
                        } else {
                            Some(if attributes_from_index {
                                core::repository::index::entries::Attributes::Index
                            } else {
                                core::repository::index::entries::Attributes::WorktreeAndIndex
                            })
                        },
                        recurse_submodules,
                        statistics,
                    },
                )?;
            }
        },

        // -- mailmap --
        Subcommands::Mailmap(sub) => match sub {
            MailmapSubcommands::Entries => {
                core::repository::mailmap::entries(
                    repo(RepoMode::Lenient)?,
                    format,
                    out,
                    err,
                )?;
            }
            MailmapSubcommands::Check { contacts } => {
                core::repository::mailmap::check(
                    repo(RepoMode::Lenient)?,
                    format,
                    contacts,
                    out,
                    err,
                )?;
            }
        },

        // -- odb --
        Subcommands::Odb(sub) => match sub {
            OdbSubcommands::Entries => {
                core::repository::odb::entries(repo(RepoMode::Strict)?, format, out)?;
            }
            OdbSubcommands::Info => {
                core::repository::odb::info(repo(RepoMode::Strict)?, format, out, err)?;
            }
            OdbSubcommands::Stats {
                extra_header_lookup,
            } => {
                core::repository::odb::statistics(
                    repo(RepoMode::Strict)?,
                    no_progress(),
                    out,
                    err,
                    core::repository::odb::statistics::Options {
                        format,
                        thread_limit,
                        extra_header_lookup,
                    },
                )?;
            }
        },

        // -- worktree --
        Subcommands::Worktree(sub) => match sub {
            WorktreeSubcommands::List => {
                core::repository::worktree::list(repo(RepoMode::Lenient)?, out, format)?;
            }
        },

        // -- submodule --
        Subcommands::Submodule(sub) => match sub {
            SubmoduleSubcommands::List { dirty_suffix } => {
                core::repository::submodule::list(
                    repo(RepoMode::Lenient)?,
                    out,
                    format,
                    dirty_suffix.map(|s| s.unwrap_or_else(|| "dirty".to_string())),
                )?;
            }
        },

        // -- merge-base --
        Subcommands::MergeBase { first, others } => {
            core::repository::merge_base(
                repo(RepoMode::Lenient)?,
                first,
                others,
                out,
                format,
            )?;
        }

        // -- fsck --
        Subcommands::Fsck { spec } => {
            core::repository::fsck(repo(RepoMode::Strict)?, spec, out)?;
        }
    }

    Ok(())
}
