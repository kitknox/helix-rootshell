#ifndef HELIX_IOS_H
#define HELIX_IOS_H

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

/// Opaque handle to a running Helix editor instance.
typedef struct HelixHandle HelixHandle;

/// Error codes returned by `helix_last_error_code`.
enum {
  HELIX_ERROR_NONE = 0,
  HELIX_ERROR_NULL_RUNTIME_PATH = 1,
  HELIX_ERROR_INVALID_RUNTIME_PATH_UTF8 = 2,
  HELIX_ERROR_RUNTIME_PATH_MISMATCH = 3,
  HELIX_ERROR_INVALID_FILE_PATH_UTF8 = 4,
  HELIX_ERROR_THREAD_SPAWN_FAILED = 5,
  HELIX_ERROR_INVALID_ARG_UTF8 = 6,
  HELIX_ERROR_ARG_PARSE_FAILED = 7,
};

/// Create a new Helix editor instance on a background thread.
///
/// @param input_fd   Read end of pipe (Helix reads terminal input). Ownership transfers.
/// @param output_fd  Write end of pipe (Helix writes ANSI output). Ownership transfers.
/// @param cols       Initial terminal width in columns.
/// @param rows       Initial terminal height in rows.
/// @param runtime_path  Path to bundled Helix runtime directory (themes, queries).
/// @param file_path  Optional file to open (NULL for empty buffer).
/// @note All instances in a process share global runtime configuration.
///       `runtime_path` must match the first successful call.
/// @return Handle pointer, or NULL on failure.
HelixHandle* helix_create(int input_fd, int output_fd,
                          uint16_t cols, uint16_t rows,
                          const char* runtime_path,
                          const char* file_path);

/// Create a new Helix editor instance with full CLI argument support.
///
/// @param input_fd   Read end of pipe (Helix reads terminal input). Ownership transfers.
/// @param output_fd  Write end of pipe (Helix writes ANSI output). Ownership transfers.
/// @param cols       Initial terminal width in columns.
/// @param rows       Initial terminal height in rows.
/// @param runtime_path  Path to bundled Helix runtime directory (themes, queries).
/// @param argc       Number of arguments in argv.
/// @param argv       Array of null-terminated C strings. argv[0] is the program name ("hx").
/// @note All instances in a process share global runtime configuration.
///       `runtime_path` must match the first successful call.
/// @return Handle pointer, or NULL on failure.
HelixHandle* helix_create_with_args(int input_fd, int output_fd,
                                    uint16_t cols, uint16_t rows,
                                    const char* runtime_path,
                                    int argc, const char* const* argv);

/// Resize the editor terminal.
void helix_resize(HelixHandle* handle, uint16_t cols, uint16_t rows);

/// Request graceful shutdown of the editor.
void helix_shutdown(HelixHandle* handle);

/// Destroy instance and free resources. Blocks until editor thread exits.
void helix_destroy(HelixHandle* handle);

/// Check if the editor is still running.
bool helix_is_running(const HelixHandle* handle);

/// Return the error code for the most recent `helix_create` failure.
///
/// Returns `HELIX_ERROR_NONE` if no create error has been recorded.
int helix_last_error_code(void);

/// Return the Helix version string (e.g. "25.1 (abcdef12)").
///
/// The returned pointer is valid for the lifetime of the process and must NOT be freed.
const char* helix_version(void);

/// Entry point for the gix CLI command (gitoxide).
/// Called by ios_system for the "gix" shell command.
int gix_main(int argc, char* argv[]);

#ifdef __cplusplus
}
#endif

#endif /* HELIX_IOS_H */
