//! OSC 7501 program status reports, so rootshell can show what the editor
//! is doing. Spec: https://www.superlogical.com/rex/docs/build/program-status

use std::cell::RefCell;
use std::io::Write;

use helix_view::Editor;

const APP: &str = "helix";
const MAX_TITLE_BYTES: usize = 192;
const MAX_MSG_BYTES: usize = 2048;

thread_local! {
    // Typed commands can't reach `Application`; each helix-ios editor runs on
    // its own thread, so a thread-local can't leak between instances.
    static QUIT_REFUSED: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// A quit was refused over unsaved buffers.
pub fn note_quit_refused<S: AsRef<str>>(names: &[S]) {
    let list: Vec<&str> = names.iter().map(AsRef::as_ref).collect();
    let msg = format!(
        "{} unsaved buffer{}: {}",
        list.len(),
        if list.len() == 1 { "" } else { "s" },
        list.join(", ")
    );
    QUIT_REFUSED.with(|q| *q.borrow_mut() = Some(msg));
}

/// Any key answers a refused quit.
pub fn note_input() {
    QUIT_REFUSED.with(|q| q.borrow_mut().take());
}

/// One root record for the editor, re-sent only when it changes.
#[derive(Default)]
pub struct ProgramStatus {
    last_body: Option<String>,
    write_error: Option<String>,
}

impl ProgramStatus {
    pub fn note_write(&mut self, error: Option<String>) {
        self.write_error = error;
    }

    /// `out` is this editor's own pipe: process stdout is shared by every
    /// helix-ios instance, so reports sent there could land in another pane.
    pub fn sync(&mut self, editor: &Editor, out: &mut impl Write) {
        let any_modified = editor.documents().any(|doc| doc.is_modified());
        let refused = QUIT_REFUSED
            .with(|q| q.borrow().clone())
            .filter(|_| any_modified);
        let (_, doc) = current_ref!(editor);
        let mut title = doc.display_name().into_owned();
        if doc.is_modified() {
            title.push_str(" [+]");
        }

        let body = compose(refused.as_deref(), self.write_error.as_deref(), &title);
        if self.last_body.as_deref() != Some(body.as_str()) {
            emit(out, &body);
            self.last_body = Some(body);
        }
    }

    /// An empty-id clear removes every record, including a lingering error.
    pub fn clear(&mut self, out: &mut impl Write) {
        if self.last_body.take().is_some() {
            emit(out, "state=clear");
        }
    }
}

fn compose(refused: Option<&str>, write_error: Option<&str>, title: &str) -> String {
    let mut body = match (refused, write_error) {
        (Some(_), _) => format!("state=blocked:kind=question:app={APP}"),
        (None, Some(_)) => format!("state=error:app={APP}"),
        (None, None) => format!("state=idle:app={APP}"),
    };
    let title = encode_text(title, MAX_TITLE_BYTES);
    if !title.is_empty() {
        body.push_str(":title=");
        body.push_str(&title);
    }
    if let Some(msg) = refused.or(write_error) {
        let msg = encode_text(msg, MAX_MSG_BYTES);
        if !msg.is_empty() {
            body.push_str(":msg=");
            body.push_str(&msg);
        }
    }
    body
}

fn emit(out: &mut impl Write, body: &str) {
    let _ = write!(out, "\x1b]7501;{body}\x1b\\").and_then(|_| out.flush());
}

/// Controls become spaces (the terminal rejects them), then the text is cut
/// on a char boundary to `max` bytes and base64 encoded.
fn encode_text(text: &str, max: usize) -> String {
    let spaced: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut clean = String::new();
    for c in spaced.trim().chars() {
        if clean.len() + c.len_utf8() > max {
            break;
        }
        clean.push(c);
    }
    base64(clean.as_bytes())
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        let vectors = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (input, expected) in vectors {
            assert_eq!(base64(input.as_bytes()), expected);
        }
    }

    #[test]
    fn idle_reports_the_focused_file() {
        assert_eq!(
            compose(None, None, "main.rs [+]"),
            format!("state=idle:app=helix:title={}", base64(b"main.rs [+]"))
        );
    }

    #[test]
    fn refused_quit_outranks_a_write_error() {
        let body = compose(Some("1 unsaved buffer: a.rs"), Some("denied"), "a.rs");
        assert_eq!(
            body,
            format!(
                "state=blocked:kind=question:app=helix:title={}:msg={}",
                base64(b"a.rs"),
                base64(b"1 unsaved buffer: a.rs")
            )
        );
    }

    #[test]
    fn write_error_carries_its_message() {
        assert_eq!(
            compose(None, Some("Permission denied"), "a.rs"),
            format!(
                "state=error:app=helix:title={}:msg={}",
                base64(b"a.rs"),
                base64(b"Permission denied")
            )
        );
    }

    #[test]
    fn text_drops_controls_and_cuts_on_char_boundaries() {
        assert_eq!(encode_text("a\nb\x1b", 16), base64(b"a b"));
        // "é" is two bytes; a 3-byte limit fits "aé" but not the next "é".
        assert_eq!(encode_text("aéé", 3), base64("aé".as_bytes()));
        assert_eq!(encode_text("   ", 16), "");
    }

    #[test]
    fn clear_only_follows_a_report() {
        let mut status = ProgramStatus::default();
        let mut out = Vec::new();
        status.clear(&mut out);
        assert!(out.is_empty());
        status.last_body = Some("state=idle:app=helix".into());
        status.clear(&mut out);
        assert_eq!(out, b"\x1b]7501;state=clear\x1b\\");
    }

    #[test]
    fn note_quit_refused_pluralizes() {
        note_quit_refused(&["a.rs", "b.rs"]);
        let msg = QUIT_REFUSED.with(|q| q.borrow_mut().take());
        assert_eq!(msg.as_deref(), Some("2 unsaved buffers: a.rs, b.rs"));
    }
}
