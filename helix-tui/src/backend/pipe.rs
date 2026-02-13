use std::io::{self, BufWriter, Write as _};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use helix_view::{
    graphics::{CursorKind, Rect, UnderlineStyle},
    theme::{self, Color, Modifier},
};
use termina::{
    escape::csi::{self, Csi, SgrAttributes, SgrModifiers},
    style::CursorStyle,
    OneBased,
};

use crate::{buffer::Cell, terminal::Config};

use super::Backend;

macro_rules! decset {
    ($mode:ident) => {
        Csi::Mode(csi::Mode::SetDecPrivateMode(csi::DecPrivateMode::Code(
            csi::DecPrivateModeCode::$mode,
        )))
    };
}
macro_rules! decreset {
    ($mode:ident) => {
        Csi::Mode(csi::Mode::ResetDecPrivateMode(csi::DecPrivateMode::Code(
            csi::DecPrivateModeCode::$mode,
        )))
    };
}

/// A terminal backend that writes ANSI escape sequences to a pipe file descriptor.
///
/// This is used on iOS where Helix runs inside a host terminal emulator (Ghostty)
/// and communicates via pipes rather than owning a real terminal.
pub struct PipeBackend {
    output: BufWriter<Box<dyn io::Write + Send>>,
    cols: Arc<AtomicU32>,
    rows: Arc<AtomicU32>,
    config: Config,
    is_synchronized_output_set: bool,
}

impl PipeBackend {
    pub fn new(
        output: Box<dyn io::Write + Send>,
        cols: Arc<AtomicU32>,
        rows: Arc<AtomicU32>,
        config: Config,
    ) -> Self {
        Self {
            output: BufWriter::with_capacity(16384, output),
            cols,
            rows,
            config,
            is_synchronized_output_set: false,
        }
    }

    fn enable_mouse_capture(&mut self) -> io::Result<()> {
        if self.config.enable_mouse_capture {
            write!(
                self.output,
                "{}{}{}{}{}",
                decset!(MouseTracking),
                decset!(ButtonEventMouse),
                decset!(AnyEventMouse),
                decset!(RXVTMouse),
                decset!(SGRMouse),
            )?;
        }
        Ok(())
    }

    fn disable_mouse_capture(&mut self) -> io::Result<()> {
        if self.config.enable_mouse_capture {
            write!(
                self.output,
                "{}{}{}{}{}",
                decreset!(MouseTracking),
                decreset!(ButtonEventMouse),
                decreset!(AnyEventMouse),
                decreset!(RXVTMouse),
                decreset!(SGRMouse),
            )?;
        }
        Ok(())
    }

    fn start_synchronized_render(&mut self) -> io::Result<()> {
        if !self.is_synchronized_output_set {
            write!(self.output, "{}", decset!(SynchronizedOutput))?;
            self.is_synchronized_output_set = true;
        }
        Ok(())
    }

    fn end_synchronized_render(&mut self) -> io::Result<()> {
        if self.is_synchronized_output_set {
            write!(self.output, "{}", decreset!(SynchronizedOutput))?;
            self.is_synchronized_output_set = false;
        }
        Ok(())
    }
}

impl Backend for PipeBackend {
    fn claim(&mut self) -> io::Result<()> {
        write!(
            self.output,
            "{}{}{}{}",
            decset!(ClearAndEnableAlternateScreen),
            decset!(BracketedPaste),
            decset!(FocusTracking),
            Csi::Edit(csi::Edit::EraseInDisplay(csi::EraseInDisplay::EraseDisplay)),
        )?;
        self.enable_mouse_capture()?;
        self.flush()
    }

    fn reconfigure(&mut self, mut config: Config) -> io::Result<()> {
        std::mem::swap(&mut self.config, &mut config);
        if self.config.enable_mouse_capture != config.enable_mouse_capture {
            if self.config.enable_mouse_capture {
                self.enable_mouse_capture()?;
            } else {
                self.disable_mouse_capture()?;
            }
        }
        Ok(())
    }

    fn restore(&mut self) -> io::Result<()> {
        self.disable_mouse_capture()?;
        write!(
            self.output,
            "{}{}{}{}",
            Csi::Cursor(csi::Cursor::CursorStyle(CursorStyle::Default)),
            decreset!(BracketedPaste),
            decreset!(FocusTracking),
            decreset!(ClearAndEnableAlternateScreen),
        )?;
        self.flush()
    }

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        self.start_synchronized_render()?;

        let mut fg = Color::Reset;
        let mut bg = Color::Reset;
        let mut underline_color = Color::Reset;
        let mut underline_style = UnderlineStyle::Reset;
        let mut modifier = Modifier::empty();
        let mut last_pos: Option<(u16, u16)> = None;
        for (x, y, cell) in content {
            // Move the cursor if the previous location was not (x - 1, y)
            if !matches!(last_pos, Some(p) if x == p.0 + 1 && y == p.1) {
                write!(
                    self.output,
                    "{}",
                    Csi::Cursor(csi::Cursor::Position {
                        col: OneBased::from_zero_based(x),
                        line: OneBased::from_zero_based(y),
                    })
                )?;
            }
            last_pos = Some((x, y));

            let mut attributes = SgrAttributes::default();
            if cell.fg != fg {
                attributes.foreground = Some(cell.fg.into());
                fg = cell.fg;
            }
            if cell.bg != bg {
                attributes.background = Some(cell.bg.into());
                bg = cell.bg;
            }
            if cell.modifier != modifier {
                attributes.modifiers = diff_modifiers(modifier, cell.modifier);
                modifier = cell.modifier;
            }

            // Set underline style and color separately from SgrAttributes.
            let new_underline_style = cell.underline_style;
            if cell.underline_color != underline_color {
                write!(
                    self.output,
                    "{}",
                    Csi::Sgr(csi::Sgr::UnderlineColor(cell.underline_color.into()))
                )?;
                underline_color = cell.underline_color;
            }
            if new_underline_style != underline_style {
                write!(
                    self.output,
                    "{}",
                    Csi::Sgr(csi::Sgr::Underline(new_underline_style.into()))
                )?;
                underline_style = new_underline_style;
            }

            if !attributes.is_empty() {
                write!(
                    self.output,
                    "{}",
                    Csi::Sgr(csi::Sgr::Attributes(attributes))
                )?;
            }

            write!(self.output, "{}", &cell.symbol)?;
        }

        write!(self.output, "{}", Csi::Sgr(csi::Sgr::Reset))?;

        self.end_synchronized_render()?;

        Ok(())
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        write!(self.output, "{}", decreset!(ShowCursor))?;
        self.flush()
    }

    fn show_cursor(&mut self, kind: CursorKind) -> io::Result<()> {
        let style = match kind {
            CursorKind::Block => CursorStyle::SteadyBlock,
            CursorKind::Bar => CursorStyle::SteadyBar,
            CursorKind::Underline => CursorStyle::SteadyUnderline,
            CursorKind::Hidden => unreachable!(),
        };
        write!(
            self.output,
            "{}{}",
            decset!(ShowCursor),
            Csi::Cursor(csi::Cursor::CursorStyle(style)),
        )?;
        self.flush()
    }

    fn set_cursor(&mut self, x: u16, y: u16) -> io::Result<()> {
        let col = OneBased::from_zero_based(x);
        let line = OneBased::from_zero_based(y);
        write!(
            self.output,
            "{}",
            Csi::Cursor(csi::Cursor::Position { line, col })
        )?;
        self.flush()
    }

    fn clear(&mut self) -> io::Result<()> {
        self.start_synchronized_render()?;
        write!(
            self.output,
            "{}",
            Csi::Edit(csi::Edit::EraseInDisplay(csi::EraseInDisplay::EraseDisplay))
        )?;
        self.flush()
    }

    fn size(&self) -> io::Result<Rect> {
        let cols = self.cols.load(Ordering::Relaxed) as u16;
        let rows = self.rows.load(Ordering::Relaxed) as u16;
        Ok(Rect::new(0, 0, cols, rows))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }

    fn supports_true_color(&self) -> bool {
        true
    }

    fn get_theme_mode(&self) -> Option<theme::Mode> {
        None
    }
}

impl Drop for PipeBackend {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = self.disable_mouse_capture();
            let _ = write!(
                self.output,
                "{}{}{}{}",
                Csi::Cursor(csi::Cursor::CursorStyle(CursorStyle::Default)),
                decreset!(BracketedPaste),
                decreset!(FocusTracking),
                decreset!(ClearAndEnableAlternateScreen),
            );
            let _ = self.output.flush();
        }
    }
}

fn diff_modifiers(from: Modifier, to: Modifier) -> SgrModifiers {
    let mut modifiers = SgrModifiers::default();

    let removed = from - to;
    if removed.contains(Modifier::REVERSED) {
        modifiers |= SgrModifiers::NO_REVERSE;
    }
    if removed.contains(Modifier::BOLD) {
        modifiers |= SgrModifiers::INTENSITY_NORMAL;
        if to.contains(Modifier::DIM) {
            modifiers |= SgrModifiers::INTENSITY_DIM
        }
    }
    if removed.contains(Modifier::DIM) {
        modifiers |= SgrModifiers::INTENSITY_NORMAL;
    }
    if removed.contains(Modifier::ITALIC) {
        modifiers |= SgrModifiers::NO_ITALIC;
    }
    if removed.contains(Modifier::CROSSED_OUT) {
        modifiers |= SgrModifiers::NO_STRIKE_THROUGH;
    }
    if removed.contains(Modifier::HIDDEN) {
        modifiers |= SgrModifiers::NO_INVISIBLE;
    }
    if removed.contains(Modifier::SLOW_BLINK) || removed.contains(Modifier::RAPID_BLINK) {
        modifiers |= SgrModifiers::BLINK_NONE;
    }

    let added = to - from;
    if added.contains(Modifier::REVERSED) {
        modifiers |= SgrModifiers::REVERSE;
    }
    if added.contains(Modifier::BOLD) {
        modifiers |= SgrModifiers::INTENSITY_BOLD;
    }
    if added.contains(Modifier::DIM) {
        modifiers |= SgrModifiers::INTENSITY_DIM;
    }
    if added.contains(Modifier::ITALIC) {
        modifiers |= SgrModifiers::ITALIC;
    }
    if added.contains(Modifier::CROSSED_OUT) {
        modifiers |= SgrModifiers::STRIKE_THROUGH;
    }
    if added.contains(Modifier::HIDDEN) {
        modifiers |= SgrModifiers::INVISIBLE;
    }
    if added.contains(Modifier::SLOW_BLINK) {
        modifiers |= SgrModifiers::BLINK_SLOW;
    }
    if added.contains(Modifier::RAPID_BLINK) {
        modifiers |= SgrModifiers::BLINK_RAPID;
    }

    modifiers
}
