use std::ffi::{c_char, c_int, CStr};
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

const HELIX_ERROR_NONE: c_int = 0;
const HELIX_ERROR_NULL_RUNTIME_PATH: c_int = 1;
const HELIX_ERROR_INVALID_RUNTIME_PATH_UTF8: c_int = 2;
const HELIX_ERROR_RUNTIME_PATH_MISMATCH: c_int = 3;
const HELIX_ERROR_INVALID_FILE_PATH_UTF8: c_int = 4;
const HELIX_ERROR_THREAD_SPAWN_FAILED: c_int = 5;

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

    let file_path_str = if file_path.is_null() {
        None
    } else {
        match CStr::from_ptr(file_path).to_str() {
            Ok(s) => Some(s.to_owned()),
            Err(_) => {
                set_last_error(HELIX_ERROR_INVALID_FILE_PATH_UTF8);
                close_fds(input_fd, output_fd);
                return std::ptr::null_mut();
            }
        }
    };

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
                file_path_str,
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

fn run_helix(
    input_fd: c_int,
    output_fd: c_int,
    _cols: u16,
    _rows: u16,
    file_path: Option<String>,
    shared_cols: Arc<AtomicU32>,
    shared_rows: Arc<AtomicU32>,
    shutdown: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    resize_rx: mpsc::UnboundedReceiver<(u16, u16)>,
) {
    // Initialize config/log files (safe to call multiple times; only first wins).
    helix_loader::initialize_config_file(None);
    helix_loader::initialize_log_file(None);

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

        // Build Helix application with default config.
        let config = helix_term::config::Config::default();
        let lang_loader = helix_core::config::default_lang_loader();

        let mut args = helix_term::args::Args::default();
        if let Some(p) = file_path {
            args.files
                .insert(PathBuf::from(p), vec![helix_core::Position::default()]);
        }

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
