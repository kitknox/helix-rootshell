use std::io::{self, Read};
use std::os::fd::RawFd;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use futures_util::Stream;
use termina::{Event, Parser, WindowSize};
use tokio::sync::mpsc;

/// An async event stream that reads terminal input from a pipe and parses it into
/// `termina::Event` values. Resize events are injected from an external channel.
pub struct PipeEventStream {
    event_rx: mpsc::UnboundedReceiver<io::Result<Event>>,
}

impl PipeEventStream {
    /// Create a new PipeEventStream.
    ///
    /// `input_fd` is the read end of a pipe. This function takes ownership and will
    /// close it when the reader thread exits. The caller must not close this FD.
    ///
    /// `resize_rx` receives (cols, rows) resize notifications from the FFI layer.
    ///
    /// `shutdown` is checked periodically to stop the reader thread.
    pub fn new(
        input_fd: RawFd,
        resize_rx: mpsc::UnboundedReceiver<(u16, u16)>,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        // Spawn a blocking reader thread for the input pipe.
        // Uses poll() with a timeout so we can check the shutdown flag periodically.
        let tx = event_tx.clone();
        std::thread::Builder::new()
            .name("helix-ios-input".into())
            .spawn(move || {
                // SAFETY: input_fd is a valid file descriptor owned by us.
                // We wrap it in a File to get automatic close-on-drop.
                let mut file = unsafe { std::fs::File::from_raw_fd(input_fd) };
                let mut parser = Parser::default();
                let mut buf = [0u8; 4096];

                loop {
                    if shutdown.load(Ordering::Relaxed) {
                        break;
                    }

                    // Poll with 100ms timeout to allow shutdown checks.
                    let mut pfd = libc::pollfd {
                        fd: input_fd,
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    let ret = unsafe { libc::poll(&mut pfd, 1, 100) };

                    if ret < 0 {
                        let err = io::Error::last_os_error();
                        if err.kind() == io::ErrorKind::Interrupted {
                            continue;
                        }
                        let _ = tx.send(Err(err));
                        break;
                    }
                    if ret == 0 {
                        continue; // Timeout, check shutdown flag
                    }

                    if pfd.revents & (libc::POLLHUP | libc::POLLERR) != 0 {
                        break; // Pipe closed or error
                    }

                    if pfd.revents & libc::POLLIN != 0 {
                        match file.read(&mut buf) {
                            Ok(0) => break, // EOF
                            Ok(n) => {
                                // Check if more data is immediately available.
                                // This helps the parser handle split escape sequences.
                                let maybe_more = has_more_data(input_fd);
                                parser.parse(&buf[..n], maybe_more);
                                while let Some(event) = parser.pop() {
                                    if tx.send(Ok(event)).is_err() {
                                        return;
                                    }
                                }
                            }
                            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                            Err(e) => {
                                let _ = tx.send(Err(e));
                                break;
                            }
                        }
                    }
                }
            })
            .expect("failed to spawn input reader thread");

        // Spawn a tokio task to forward resize events into the event channel.
        let tx = event_tx;
        tokio::spawn(async move {
            let mut resize_rx = resize_rx;
            while let Some((cols, rows)) = resize_rx.recv().await {
                let event = Event::WindowResized(WindowSize {
                    cols,
                    rows,
                    pixel_width: None,
                    pixel_height: None,
                });
                if tx.send(Ok(event)).is_err() {
                    break;
                }
            }
        });

        Self { event_rx }
    }
}

impl Stream for PipeEventStream {
    type Item = io::Result<Event>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.event_rx.poll_recv(cx)
    }
}

/// Check if more data is immediately available on the file descriptor.
fn has_more_data(fd: RawFd) -> bool {
    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let ret = unsafe { libc::poll(&mut pfd, 1, 0) };
    ret > 0 && (pfd.revents & libc::POLLIN != 0)
}

use std::os::fd::FromRawFd;
