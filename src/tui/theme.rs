//! Shared visual language for the terminal interface.
//!
//! Color reinforces semantics, while modifiers and visible markers keep focus,
//! selection, and status understandable in monochrome or reduced-color terms.

use ratatui::style::{Color, Modifier, Style};

use super::status::StatusKind;

/// Palette variants used by the renderer and style-sensitive tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Palette {
    /// Dark- and light-compatible terminal colors with no forced background.
    Standard,
    /// Modifier-first styles for terminals where color is unavailable.
    #[cfg(test)]
    Reduced,
}

/// Reusable semantic styles for the TUI.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    palette: Palette,
}

impl Theme {
    pub const STANDARD: Self = Self {
        palette: Palette::Standard,
    };
    #[cfg(test)]
    pub const REDUCED: Self = Self {
        palette: Palette::Reduced,
    };

    pub fn border(self, focused: bool) -> Style {
        if focused {
            self.color(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            self.color(Color::DarkGray)
        }
    }

    pub fn heading(self) -> Style {
        self.color(Color::Cyan).add_modifier(Modifier::BOLD)
    }

    pub fn label(self) -> Style {
        self.color(Color::Blue).add_modifier(Modifier::BOLD)
    }

    pub fn muted(self) -> Style {
        self.color(Color::DarkGray).add_modifier(Modifier::DIM)
    }

    pub fn focused(self) -> Style {
        self.color(Color::Cyan)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    }

    pub fn selected(self) -> Style {
        self.color(Color::Cyan)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
    }

    pub fn success(self) -> Style {
        self.color(Color::Green).add_modifier(Modifier::BOLD)
    }

    pub fn warning(self) -> Style {
        self.color(Color::Yellow).add_modifier(Modifier::BOLD)
    }

    pub fn error(self) -> Style {
        self.color(Color::Red).add_modifier(Modifier::BOLD)
    }

    pub fn progress(self) -> Style {
        self.color(Color::Cyan).add_modifier(Modifier::BOLD)
    }

    pub fn disabled(self) -> Style {
        self.color(Color::DarkGray)
            .add_modifier(Modifier::DIM | Modifier::CROSSED_OUT)
    }

    pub fn key(self) -> Style {
        self.focused()
    }

    /// Dim the inactive UI while a modal owns input.
    pub fn backdrop(self) -> Style {
        Style::default().add_modifier(Modifier::DIM)
    }

    /// Keep dialog borders distinct even when color is unavailable.
    pub fn dialog(self) -> Style {
        self.focused().add_modifier(Modifier::REVERSED)
    }

    pub fn status(self, kind: StatusKind) -> Style {
        match kind {
            StatusKind::Success => self.success(),
            StatusKind::Running => self.progress(),
            StatusKind::Warning => self.warning(),
            StatusKind::Error => self.error(),
        }
    }

    fn color(self, color: Color) -> Style {
        match self.palette {
            Palette::Standard => Style::default().fg(color),
            #[cfg(test)]
            Palette::Reduced => Style::default(),
        }
    }
}

pub const THEME: Theme = Theme::STANDARD;

#[cfg(test)]
#[path = "../../tests/unit/tui/theme.rs"]
mod tests;
