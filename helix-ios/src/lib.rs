use std::ffi::{c_char, c_int, CStr, CString};
use std::os::fd::FromRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Once, OnceLock};
use std::thread::JoinHandle;

use tokio::sync::mpsc;

mod event_stream;
mod static_grammars;

use event_stream::PipeEventStream;

static HELIX_RUNTIME_PATH: OnceLock<String> = OnceLock::new();
static IOS_GRAMMAR_LOADER_INIT: Once = Once::new();
static LAST_ERROR_CODE: AtomicI32 = AtomicI32::new(HELIX_ERROR_NONE);
static VERSION_CSTRING: OnceLock<CString> = OnceLock::new();

const HELIX_ERROR_NONE: c_int = 0;
const HELIX_ERROR_NULL_RUNTIME_PATH: c_int = 1;
const HELIX_ERROR_INVALID_RUNTIME_PATH_UTF8: c_int = 2;
const HELIX_ERROR_RUNTIME_PATH_MISMATCH: c_int = 3;
const HELIX_ERROR_INVALID_FILE_PATH_UTF8: c_int = 4;
const HELIX_ERROR_THREAD_SPAWN_FAILED: c_int = 5;
const HELIX_ERROR_INVALID_ARG_UTF8: c_int = 6;
const HELIX_ERROR_ARG_PARSE_FAILED: c_int = 7;

/// Opaque handle to a running Helix editor instance.
///
/// Each instance owns a dedicated thread with its own tokio runtime.
/// Instances in the same process share global runtime/grammar configuration.
pub struct HelixHandle {
    thread: Option<JoinHandle<()>>,
    /// Wrapped in Mutex<Option<>> so helix_shutdown() can drop it.
    /// Dropping the sender causes the resize forwarder task to exit,
    /// which in turn closes the event channel and unblocks app.run().
    resize_tx: Mutex<Option<mpsc::UnboundedSender<(u16, u16)>>>,
    cols: Arc<AtomicU32>,
    rows: Arc<AtomicU32>,
    shutdown: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}

/// Create a new Helix editor instance on a background thread.
///
/// # Arguments
/// * `input_fd` - Read end of pipe (Helix reads terminal input from here). Ownership transfers to Helix.
/// * `output_fd` - Write end of pipe (Helix writes ANSI output here). Ownership transfers to Helix.
/// * `cols` - Initial terminal width in columns.
/// * `rows` - Initial terminal height in rows.
/// * `runtime_path` - Path to the bundled Helix runtime directory (themes, queries).
/// * `file_path` - Optional file to open (NULL for empty buffer).
///
/// # Returns
/// A pointer to a HelixHandle, or NULL on failure.
///
/// # Safety
/// * `runtime_path` must be a valid null-terminated C string.
/// * `file_path` must be a valid null-terminated C string or NULL.
/// * `input_fd` and `output_fd` must be valid, open file descriptors.
///
/// All editor instances in a process share global runtime settings.
/// `runtime_path` must match the first successful call.
#[no_mangle]
pub unsafe extern "C" fn helix_create(
    input_fd: c_int,
    output_fd: c_int,
    cols: u16,
    rows: u16,
    runtime_path: *const c_char,
    file_path: *const c_char,
) -> *mut HelixHandle {
    set_last_error(HELIX_ERROR_NONE);

    if runtime_path.is_null() {
        set_last_error(HELIX_ERROR_NULL_RUNTIME_PATH);
        close_fds(input_fd, output_fd);
        return std::ptr::null_mut();
    }

    let runtime_path_str = match CStr::from_ptr(runtime_path).to_str() {
        Ok(s) => s.to_owned(),
        Err(_) => {
            set_last_error(HELIX_ERROR_INVALID_RUNTIME_PATH_UTF8);
            close_fds(input_fd, output_fd);
            return std::ptr::null_mut();
        }
    };
    if !initialize_runtime_path(&runtime_path_str) {
        set_last_error(HELIX_ERROR_RUNTIME_PATH_MISMATCH);
        close_fds(input_fd, output_fd);
        return std::ptr::null_mut();
    }

    // Build a minimal argv: ["hx"] or ["hx", "<file>"]
    let mut raw_args: Vec<String> = vec!["hx".into()];
    if !file_path.is_null() {
        match CStr::from_ptr(file_path).to_str() {
            Ok(s) => raw_args.push(s.to_owned()),
            Err(_) => {
                set_last_error(HELIX_ERROR_INVALID_FILE_PATH_UTF8);
                close_fds(input_fd, output_fd);
                return std::ptr::null_mut();
            }
        }
    }

    spawn_helix_thread(input_fd, output_fd, cols, rows, raw_args)
}

/// Create a new Helix editor instance with full CLI argument support.
///
/// # Arguments
/// * `input_fd` - Read end of pipe (Helix reads terminal input from here). Ownership transfers to Helix.
/// * `output_fd` - Write end of pipe (Helix writes ANSI output here). Ownership transfers to Helix.
/// * `cols` - Initial terminal width in columns.
/// * `rows` - Initial terminal height in rows.
/// * `runtime_path` - Path to the bundled Helix runtime directory (themes, queries).
/// * `argc` - Number of arguments in argv.
/// * `argv` - Array of null-terminated C strings. argv[0] is the program name ("hx").
///
/// # Returns
/// A pointer to a HelixHandle, or NULL on failure.
///
/// # Safety
/// * `runtime_path` must be a valid null-terminated C string.
/// * `argv` must be a valid pointer to `argc` null-terminated C strings.
/// * `input_fd` and `output_fd` must be valid, open file descriptors.
#[no_mangle]
pub unsafe extern "C" fn helix_create_with_args(
    input_fd: c_int,
    output_fd: c_int,
    cols: u16,
    rows: u16,
    runtime_path: *const c_char,
    argc: c_int,
    argv: *const *const c_char,
) -> *mut HelixHandle {
    set_last_error(HELIX_ERROR_NONE);

    if runtime_path.is_null() {
        set_last_error(HELIX_ERROR_NULL_RUNTIME_PATH);
        close_fds(input_fd, output_fd);
        return std::ptr::null_mut();
    }

    let runtime_path_str = match CStr::from_ptr(runtime_path).to_str() {
        Ok(s) => s.to_owned(),
        Err(_) => {
            set_last_error(HELIX_ERROR_INVALID_RUNTIME_PATH_UTF8);
            close_fds(input_fd, output_fd);
            return std::ptr::null_mut();
        }
    };
    if !initialize_runtime_path(&runtime_path_str) {
        set_last_error(HELIX_ERROR_RUNTIME_PATH_MISMATCH);
        close_fds(input_fd, output_fd);
        return std::ptr::null_mut();
    }

    // Convert C argv to Vec<String>
    let mut raw_args: Vec<String> = Vec::with_capacity(argc as usize);
    for i in 0..argc {
        let arg_ptr = *argv.offset(i as isize);
        match CStr::from_ptr(arg_ptr).to_str() {
            Ok(s) => raw_args.push(s.to_owned()),
            Err(_) => {
                set_last_error(HELIX_ERROR_INVALID_ARG_UTF8);
                close_fds(input_fd, output_fd);
                return std::ptr::null_mut();
            }
        }
    }

    spawn_helix_thread(input_fd, output_fd, cols, rows, raw_args)
}

/// Return the Helix version string (e.g. "25.1 (abcdef12)").
///
/// The returned pointer is valid for the lifetime of the process and must NOT be freed.
#[no_mangle]
pub extern "C" fn helix_version() -> *const c_char {
    let cs = VERSION_CSTRING.get_or_init(|| {
        CString::new(helix_loader::VERSION_AND_GIT_HASH).unwrap_or_else(|_| CString::new("unknown").unwrap())
    });
    cs.as_ptr()
}

/// Common implementation: validate runtime path, spawn helix thread.
unsafe fn spawn_helix_thread(
    input_fd: c_int,
    output_fd: c_int,
    cols: u16,
    rows: u16,
    raw_args: Vec<String>,
) -> *mut HelixHandle {
    let shared_cols = Arc::new(AtomicU32::new(cols as u32));
    let shared_rows = Arc::new(AtomicU32::new(rows as u32));
    let shutdown = Arc::new(AtomicBool::new(false));
    let running = Arc::new(AtomicBool::new(true));
    let (resize_tx, resize_rx) = mpsc::unbounded_channel();

    let thread_cols = Arc::clone(&shared_cols);
    let thread_rows = Arc::clone(&shared_rows);
    let thread_shutdown = Arc::clone(&shutdown);
    let thread_running = Arc::clone(&running);

    let thread = std::thread::Builder::new()
        .name("helix-editor".into())
        .spawn(move || {
            run_helix(
                input_fd,
                output_fd,
                cols,
                rows,
                raw_args,
                thread_cols,
                thread_rows,
                thread_shutdown,
                thread_running,
                resize_rx,
            );
        });

    match thread {
        Ok(thread) => {
            let handle = Box::new(HelixHandle {
                thread: Some(thread),
                resize_tx: Mutex::new(Some(resize_tx)),
                cols: shared_cols,
                rows: shared_rows,
                shutdown,
                running,
            });
            Box::into_raw(handle)
        }
        Err(err) => {
            log::error!("Failed to spawn helix editor thread: {}", err);
            set_last_error(HELIX_ERROR_THREAD_SPAWN_FAILED);
            close_fds(input_fd, output_fd);
            std::ptr::null_mut()
        }
    }
}

/// Return the error code for the most recent `helix_create` failure.
///
/// Returns 0 if no `helix_create` error has been recorded.
#[no_mangle]
pub extern "C" fn helix_last_error_code() -> c_int {
    LAST_ERROR_CODE.load(Ordering::Relaxed)
}

/// Resize the editor terminal.
///
/// # Safety
/// `handle` must be a valid pointer returned by `helix_create`.
#[no_mangle]
pub unsafe extern "C" fn helix_resize(handle: *mut HelixHandle, cols: u16, rows: u16) {
    if handle.is_null() {
        return;
    }
    let handle = &*handle;
    handle.cols.store(cols as u32, Ordering::Relaxed);
    handle.rows.store(rows as u32, Ordering::Relaxed);
    if let Ok(guard) = handle.resize_tx.lock() {
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send((cols, rows));
        }
    }
}

/// Request graceful shutdown of the editor.
///
/// # Safety
/// `handle` must be a valid pointer returned by `helix_create`.
#[no_mangle]
pub unsafe extern "C" fn helix_shutdown(handle: *mut HelixHandle) {
    if handle.is_null() {
        return;
    }
    let handle = &*handle;
    handle.shutdown.store(true, Ordering::Relaxed);
    // Drop resize_tx so the resize forwarder task exits.
    // This allows the event channel to close (once the input reader
    // thread also exits), unblocking app.run().
    if let Ok(mut guard) = handle.resize_tx.lock() {
        let _ = guard.take();
    }
}

/// Destroy the editor instance and free all resources.
/// Blocks until the editor thread exits.
///
/// # Safety
/// `handle` must be a valid pointer returned by `helix_create`.
/// After this call, `handle` is invalid and must not be used.
#[no_mangle]
pub unsafe extern "C" fn helix_destroy(handle: *mut HelixHandle) {
    if handle.is_null() {
        return;
    }
    let mut handle = Box::from_raw(handle);
    handle.shutdown.store(true, Ordering::Relaxed);
    // Drop resize_tx to unblock the event stream before joining.
    if let Ok(mut guard) = handle.resize_tx.lock() {
        let _ = guard.take();
    }
    if let Some(thread) = handle.thread.take() {
        let _ = thread.join();
    }
    // handle is dropped here, freeing all Arc references
}

/// Check if the editor is still running.
///
/// # Safety
/// `handle` must be a valid pointer returned by `helix_create`.
#[no_mangle]
pub unsafe extern "C" fn helix_is_running(handle: *const HelixHandle) -> bool {
    if handle.is_null() {
        return false;
    }
    let handle = &*handle;
    handle.running.load(Ordering::Relaxed)
}

/// RAII guard that restores the original stdout when dropped.
struct StdoutRedirect {
    saved_fd: c_int,
}

impl Drop for StdoutRedirect {
    fn drop(&mut self) {
        let _ = std::io::Write::flush(&mut std::io::stdout());
        unsafe {
            libc::dup2(self.saved_fd, libc::STDOUT_FILENO);
            libc::close(self.saved_fd);
        }
    }
}

fn run_helix(
    input_fd: c_int,
    output_fd: c_int,
    _cols: u16,
    _rows: u16,
    raw_args: Vec<String>,
    shared_cols: Arc<AtomicU32>,
    shared_rows: Arc<AtomicU32>,
    shutdown: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    resize_rx: mpsc::UnboundedReceiver<(u16, u16)>,
) {
    // Parse CLI arguments first so -c and --log take effect.
    let args = match helix_term::args::Args::parse_from_iter(raw_args) {
        Ok(args) => args,
        Err(e) => {
            // Write the error to the output pipe so the user sees it.
            // SAFETY: output_fd is a valid FD that we now own.
            let mut output = unsafe { std::fs::File::from_raw_fd(output_fd) };
            let _ = std::io::Write::write_all(
                &mut output,
                format!("hx: {}\r\n", e).as_bytes(),
            );
            drop(output);
            unsafe { libc::close(input_fd); }
            set_last_error(HELIX_ERROR_ARG_PARSE_FAILED);
            running.store(false, Ordering::Relaxed);
            return;
        }
    };

    // Initialize config/log files (safe to call multiple times; only first wins).
    // Use -c / --log from parsed args if provided.
    // Build the config path explicitly from $HOME (which ios_setenv sets to
    // ~/Documents). We can't rely on etcetera's XDG_CONFIG_HOME resolution
    // because Ghostty.swift sets that to Library/Application Support for its
    // own config.
    let config_file = args.config_file.clone().or_else(|| {
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(".config/helix/config.toml"))
    });
    helix_loader::initialize_config_file(config_file);
    helix_loader::initialize_log_file(args.log_file.clone());
    log::info!("Helix config path: {:?}", helix_loader::config_file());

    // Apply -w / --working-dir if specified.
    if let Some(ref dir) = args.working_directory {
        let _ = helix_stdx::env::set_current_working_dir(dir);
    }

    // Handle print-and-exit flags (--health, -g fetch, -g build).
    // These print diagnostic info to the output pipe and return without
    // launching the editor, mirroring helix-term/src/main.rs behavior.
    if args.health || args.fetch_grammars || args.build_grammars {
        // Register grammar loader so health checks find statically linked grammars.
        IOS_GRAMMAR_LOADER_INIT.call_once(|| {
            helix_loader::grammar::set_grammar_loader(Box::new(
                static_grammars::StaticGrammarLoader,
            ));
        });

        let output = if args.health {
            capture_stdout(|| {
                let _ = helix_term::health::print_health(args.health_arg);
            })
        } else {
            // -g fetch / -g build: grammars are statically compiled on iOS.
            let mut buf = Vec::new();
            use std::io::Write;
            let _ = writeln!(buf, "Tree-sitter grammars are statically compiled on iOS.");
            let _ = writeln!(buf, "The fetch and build commands are not needed.\n");
            let _ = writeln!(buf, "Available grammars ({}):", STATIC_GRAMMAR_NAMES.len());
            for name in STATIC_GRAMMAR_NAMES {
                let _ = writeln!(buf, "  {}", name);
            }
            buf
        };

        write_to_pipe_crlf(output_fd, &output);
        unsafe { libc::close(input_fd); }
        running.store(false, Ordering::Relaxed);
        return;
    }

    // Create a dedicated tokio runtime for this editor instance.
    // The integration_test feature makes helix-event statics per-runtime.
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            log::error!("Failed to create tokio runtime: {}", e);
            running.store(false, Ordering::Relaxed);
            return;
        }
    };

    // Redirect process stdout to the output pipe so that the Termcode
    // clipboard provider's OSC 52 sequences reach Ghostty's terminal parser,
    // which handles clipboard integration via UIPasteboard.
    let _stdout_guard = {
        let saved = unsafe { libc::dup(libc::STDOUT_FILENO) };
        if saved >= 0 && unsafe { libc::dup2(output_fd, libc::STDOUT_FILENO) } >= 0 {
            Some(StdoutRedirect { saved_fd: saved })
        } else {
            if saved >= 0 {
                unsafe { libc::close(saved); }
            }
            None
        }
    };

    rt.block_on(async {
        // Register helix-event hooks for this runtime.
        helix_term::events::register();

        // Create the output pipe writer.
        // SAFETY: output_fd is a valid FD that we now own.
        let output_file: Box<dyn std::io::Write + Send> =
            Box::new(unsafe { std::fs::File::from_raw_fd(output_fd) });

        // Create PipeBackend with sensible defaults for Ghostty.
        let backend_config = tui::terminal::Config {
            enable_mouse_capture: true,
            force_enable_extended_underlines: true,
            kitty_keyboard_protocol: helix_view::editor::KittyKeyboardProtocolConfig::Disabled,
        };
        let backend = tui::backend::PipeBackend::new(
            output_file,
            Arc::clone(&shared_cols),
            Arc::clone(&shared_rows),
            backend_config,
        );

        // Create the event stream (spawns input reader thread).
        let mut events = PipeEventStream::new(input_fd, resize_rx, Arc::clone(&shutdown));

        // Register statically linked tree-sitter grammars (iOS cannot use dlopen).
        IOS_GRAMMAR_LOADER_INIT.call_once(|| {
            helix_loader::grammar::set_grammar_loader(Box::new(
                static_grammars::StaticGrammarLoader,
            ));
        });

        // Load config from ~/.config/helix/config.toml, falling back to defaults.
        let config = match helix_term::config::Config::load_default() {
            Ok(config) => config,
            Err(helix_term::config::ConfigLoadError::Error(err))
                if err.kind() == std::io::ErrorKind::NotFound =>
            {
                helix_term::config::Config::default()
            }
            Err(err) => {
                log::warn!("Failed to load config: {err}, using defaults");
                helix_term::config::Config::default()
            }
        };
        let lang_loader = helix_core::config::default_lang_loader();

        let mut app = match helix_term::application::Application::new_with_backend(
            args,
            config,
            lang_loader,
            backend,
        ) {
            Ok(app) => app,
            Err(e) => {
                log::error!("Failed to create Helix application: {}", e);
                running.store(false, Ordering::Relaxed);
                return;
            }
        };

        // Run the editor event loop.
        let _ = app.run(&mut events).await;

        running.store(false, Ordering::Relaxed);
    });
}

fn close_fds(input_fd: c_int, output_fd: c_int) {
    unsafe {
        libc::close(input_fd);
        if output_fd != input_fd {
            libc::close(output_fd);
        }
    }
}

fn initialize_runtime_path(runtime_path: &str) -> bool {
    let configured_runtime = HELIX_RUNTIME_PATH.get_or_init(|| {
        std::env::set_var("HELIX_RUNTIME", runtime_path);
        runtime_path.to_owned()
    });

    if configured_runtime != runtime_path {
        log::error!(
            "helix_create runtime_path mismatch: existing='{}' requested='{}'",
            configured_runtime,
            runtime_path
        );
        return false;
    }

    true
}

fn set_last_error(code: c_int) {
    LAST_ERROR_CODE.store(code, Ordering::Relaxed);
}

/// Capture everything written to stdout by the given closure.
///
/// Redirects fd 1 to a pipe, runs `f`, then restores stdout and
/// returns the captured bytes.
fn capture_stdout<F: FnOnce()>(f: F) -> Vec<u8> {
    let mut pipe_fds = [0i32; 2];
    if unsafe { libc::pipe(pipe_fds.as_mut_ptr()) } != 0 {
        // Pipe creation failed — just run the closure and return nothing.
        f();
        return Vec::new();
    }
    let (cap_read, cap_write) = (pipe_fds[0], pipe_fds[1]);

    let saved_stdout = unsafe { libc::dup(libc::STDOUT_FILENO) };
    unsafe { libc::dup2(cap_write, libc::STDOUT_FILENO); }
    // Close our copy of the write end; stdout now owns it.
    unsafe { libc::close(cap_write); }

    f();

    // Flush Rust's buffered stdout so all output reaches the pipe.
    let _ = std::io::Write::flush(&mut std::io::stdout());

    // Restore stdout (closes the write end, signaling EOF to the reader).
    unsafe {
        libc::dup2(saved_stdout, libc::STDOUT_FILENO);
        libc::close(saved_stdout);
    }

    // Read all captured output.
    let mut captured = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = unsafe { libc::read(cap_read, buf.as_mut_ptr() as *mut _, buf.len()) };
        if n <= 0 {
            break;
        }
        captured.extend_from_slice(&buf[..n as usize]);
    }
    unsafe { libc::close(cap_read); }

    captured
}

/// Write bytes to a file descriptor, converting lone LF to CRLF.
///
/// Terminal emulators treat bare `\n` as cursor-down without carriage
/// return, producing a staircase effect. This function ensures proper
/// line breaks for pipe-based terminal output.
fn write_to_pipe_crlf(fd: c_int, data: &[u8]) {
    // SAFETY: fd is a valid, open file descriptor that we take ownership of.
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    use std::io::Write;
    let mut prev_was_cr = false;
    for &byte in data {
        if byte == b'\n' && !prev_was_cr {
            let _ = file.write_all(b"\r\n");
        } else {
            let _ = file.write_all(&[byte]);
        }
        prev_was_cr = byte == b'\r';
    }
    // File::drop closes the fd.
}

/// Names of all statically linked tree-sitter grammars (alphabetical).
const STATIC_GRAMMAR_NAMES: &[&str] = &[
    "bash",
    "c",
    "comment",
    "cpp",
    "css",
    "diff",
    "dockerfile",
    "git-rebase",
    "gitcommit",
    "go",
    "html",
    "java",
    "javascript",
    "json",
    "kotlin",
    "lua",
    "markdown",
    "markdown_inline",
    "python",
    "ruby",
    "rust",
    "sql",
    "swift",
    "toml",
    "tsx",
    "typescript",
    "xml",
    "yaml",
    "zig",
];
