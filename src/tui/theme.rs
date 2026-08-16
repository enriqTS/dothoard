//! Shared visual language for the terminal interface.
//!
//! The active look is one of a set of palettes (`ThemeId`), selectable at
//! runtime through the theme picker (Ctrl+T) and persisted to `theme.toml` in
//! the configuration directory. The default System palette uses the terminal's
//! default and ANSI colors, so it follows terminal colorscheme changes without
//! requiring desktop-environment-specific integration. Optional built-in
//! palettes use fixed RGB colors. Every render call reads the active palette
//! through [`Theme::current`], so switching themes recolors the whole interface
//! immediately, not just borders and labels.

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};

use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};

use super::status::StatusKind;

/// A complete set of role-based colors for one built-in theme.
///
/// Every field is a distinct visual role rather than a raw hue, so each
/// theme can assign whichever of its own colors best fills that role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Full-frame canvas background.
    pub background: Color,
    /// Tab bar and footer background, distinct from the canvas.
    pub chrome: Color,
    /// Dialog and panel background.
    pub surface: Color,
    /// Selected-row background.
    pub selection_bg: Color,
    /// Body text.
    pub foreground: Color,
    /// Secondary/dim text.
    pub muted: Color,
    /// Inactive border color.
    pub border: Color,
    /// Primary accent: focus, headings, keybinding hints, dialog frames.
    pub accent: Color,
    /// Secondary accent: field labels, directories, informational highlights.
    pub secondary: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    /// Tertiary flourish: symlinks, special files, secondary badges.
    pub special: Color,
}

/// Built-in themes, selectable through the theme picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeId {
    /// Inherit the terminal's default foreground/background and ANSI palette.
    #[default]
    System,
    CatppuccinMocha,
    CatppuccinLatte,
    Dracula,
    Nord,
    GruvboxDark,
    TokyoNight,
    SolarizedDark,
    RosePine,
    Everforest,
    Kanagawa,
}

impl ThemeId {
    /// All themes, in the order the picker lists them.
    pub const ALL: &'static [ThemeId] = &[
        ThemeId::System,
        ThemeId::CatppuccinMocha,
        ThemeId::CatppuccinLatte,
        ThemeId::Dracula,
        ThemeId::Nord,
        ThemeId::GruvboxDark,
        ThemeId::TokyoNight,
        ThemeId::SolarizedDark,
        ThemeId::RosePine,
        ThemeId::Everforest,
        ThemeId::Kanagawa,
    ];

    /// Display name shown in the theme picker.
    pub fn label(self) -> &'static str {
        match self {
            ThemeId::System => "System (Terminal)",
            ThemeId::CatppuccinMocha => "Catppuccin Mocha",
            ThemeId::CatppuccinLatte => "Catppuccin Latte",
            ThemeId::Dracula => "Dracula",
            ThemeId::Nord => "Nord",
            ThemeId::GruvboxDark => "Gruvbox Dark",
            ThemeId::TokyoNight => "Tokyo Night",
            ThemeId::SolarizedDark => "Solarized Dark",
            ThemeId::RosePine => "Rose Pine",
            ThemeId::Everforest => "Everforest",
            ThemeId::Kanagawa => "Kanagawa",
        }
    }

    /// Stable identifier used for persistence (`theme.toml`).
    pub fn slug(self) -> &'static str {
        match self {
            ThemeId::System => "system",
            ThemeId::CatppuccinMocha => "catppuccin-mocha",
            ThemeId::CatppuccinLatte => "catppuccin-latte",
            ThemeId::Dracula => "dracula",
            ThemeId::Nord => "nord",
            ThemeId::GruvboxDark => "gruvbox-dark",
            ThemeId::TokyoNight => "tokyo-night",
            ThemeId::SolarizedDark => "solarized-dark",
            ThemeId::RosePine => "rose-pine",
            ThemeId::Everforest => "everforest",
            ThemeId::Kanagawa => "kanagawa",
        }
    }

    /// Resolve a persisted slug back to a theme. Unknown slugs fall back to
    /// the default so a hand-edited or stale `theme.toml` never blocks
    /// startup.
    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|id| id.slug() == slug)
    }

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|&id| id == self)
            .unwrap_or_default()
    }

    /// The next theme in picker order, wrapping around.
    pub fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    /// The previous theme in picker order, wrapping around.
    pub fn prev(self) -> Self {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    /// The resolved color roles for this theme.
    pub fn palette(self) -> Palette {
        fn rgb(hex: u32) -> Color {
            Color::Rgb(
                ((hex >> 16) & 0xFF) as u8,
                ((hex >> 8) & 0xFF) as u8,
                (hex & 0xFF) as u8,
            )
        }

        match self {
            ThemeId::System => Palette {
                // Named ANSI colors are resolved by the terminal, while Reset
                // preserves its configured foreground and background. This
                // makes live terminal palette updates visible on the next draw.
                background: Color::Reset,
                chrome: Color::Reset,
                surface: Color::Reset,
                selection_bg: Color::DarkGray,
                foreground: Color::Reset,
                muted: Color::DarkGray,
                border: Color::DarkGray,
                accent: Color::Cyan,
                secondary: Color::Blue,
                success: Color::Green,
                warning: Color::Yellow,
                error: Color::Red,
                special: Color::Magenta,
            },
            ThemeId::CatppuccinMocha => Palette {
                background: rgb(0x1e1e2e),
                chrome: rgb(0x181825),
                surface: rgb(0x313244),
                selection_bg: rgb(0x45475a),
                foreground: rgb(0xcdd6f4),
                muted: rgb(0xa6adc8),
                border: rgb(0x6c7086),
                accent: rgb(0xcba6f7),
                secondary: rgb(0x89b4fa),
                success: rgb(0xa6e3a1),
                warning: rgb(0xf9e2af),
                error: rgb(0xf38ba8),
                special: rgb(0xf5c2e7),
            },
            ThemeId::CatppuccinLatte => Palette {
                background: rgb(0xeff1f5),
                chrome: rgb(0xe6e9ef),
                surface: rgb(0xccd0da),
                selection_bg: rgb(0xbcc0cc),
                foreground: rgb(0x4c4f69),
                muted: rgb(0x6c6f85),
                border: rgb(0x9ca0b0),
                accent: rgb(0x8839ef),
                secondary: rgb(0x1e66f5),
                success: rgb(0x40a02b),
                warning: rgb(0xdf8e1d),
                error: rgb(0xd20f39),
                special: rgb(0xea76cb),
            },
            ThemeId::Dracula => Palette {
                background: rgb(0x282a36),
                chrome: rgb(0x21222c),
                surface: rgb(0x343746),
                selection_bg: rgb(0x44475a),
                foreground: rgb(0xf8f8f2),
                muted: rgb(0x6272a4),
                border: rgb(0x565971),
                accent: rgb(0xbd93f9),
                secondary: rgb(0x8be9fd),
                success: rgb(0x50fa7b),
                warning: rgb(0xf1fa8c),
                error: rgb(0xff5555),
                special: rgb(0xff79c6),
            },
            ThemeId::Nord => Palette {
                background: rgb(0x2e3440),
                chrome: rgb(0x272c36),
                surface: rgb(0x3b4252),
                selection_bg: rgb(0x434c5e),
                foreground: rgb(0xe5e9f0),
                muted: rgb(0x8792a8),
                border: rgb(0x4c566a),
                accent: rgb(0x88c0d0),
                secondary: rgb(0x81a1c1),
                success: rgb(0xa3be8c),
                warning: rgb(0xebcb8b),
                error: rgb(0xbf616a),
                special: rgb(0xb48ead),
            },
            ThemeId::GruvboxDark => Palette {
                background: rgb(0x282828),
                chrome: rgb(0x1d2021),
                surface: rgb(0x3c3836),
                selection_bg: rgb(0x504945),
                foreground: rgb(0xebdbb2),
                muted: rgb(0xa89984),
                border: rgb(0x7c6f64),
                accent: rgb(0xfe8019),
                secondary: rgb(0x83a598),
                success: rgb(0xb8bb26),
                warning: rgb(0xfabd2f),
                error: rgb(0xfb4934),
                special: rgb(0xd3869b),
            },
            ThemeId::TokyoNight => Palette {
                background: rgb(0x24283b),
                chrome: rgb(0x1f2335),
                surface: rgb(0x292e42),
                selection_bg: rgb(0x364a82),
                foreground: rgb(0xc0caf5),
                muted: rgb(0x737aa2),
                border: rgb(0x414868),
                accent: rgb(0x7aa2f7),
                secondary: rgb(0x7dcfff),
                success: rgb(0x9ece6a),
                warning: rgb(0xe0af68),
                error: rgb(0xf7768e),
                special: rgb(0xbb9af7),
            },
            ThemeId::SolarizedDark => Palette {
                background: rgb(0x002b36),
                chrome: rgb(0x00212b),
                surface: rgb(0x073642),
                selection_bg: rgb(0x0a4552),
                foreground: rgb(0x93a1a1),
                muted: rgb(0x586e75),
                border: rgb(0x586e75),
                accent: rgb(0x268bd2),
                secondary: rgb(0x2aa198),
                success: rgb(0x859900),
                warning: rgb(0xb58900),
                error: rgb(0xdc322f),
                special: rgb(0xd33682),
            },
            ThemeId::RosePine => Palette {
                background: rgb(0x191724),
                chrome: rgb(0x1f1d2e),
                surface: rgb(0x26233a),
                selection_bg: rgb(0x403d52),
                foreground: rgb(0xe0def4),
                muted: rgb(0x908caa),
                border: rgb(0x6e6a86),
                accent: rgb(0xc4a7e7),
                secondary: rgb(0x9ccfd8),
                success: rgb(0x31748f),
                warning: rgb(0xf6c177),
                error: rgb(0xeb6f92),
                special: rgb(0xebbcba),
            },
            ThemeId::Everforest => Palette {
                background: rgb(0x2d353b),
                chrome: rgb(0x272e33),
                surface: rgb(0x3d484d),
                selection_bg: rgb(0x475258),
                foreground: rgb(0xd3c6aa),
                muted: rgb(0x9da9a0),
                border: rgb(0x7a8478),
                accent: rgb(0xa7c080),
                secondary: rgb(0x7fbbb3),
                success: rgb(0xa7c080),
                warning: rgb(0xdbbc7f),
                error: rgb(0xe67e80),
                special: rgb(0xd699b6),
            },
            ThemeId::Kanagawa => Palette {
                background: rgb(0x1f1f28),
                chrome: rgb(0x16161d),
                surface: rgb(0x2a2a37),
                selection_bg: rgb(0x363646),
                foreground: rgb(0xdcd7ba),
                muted: rgb(0x9a9aad),
                border: rgb(0x727169),
                accent: rgb(0x957fb8),
                secondary: rgb(0x7fb4ca),
                success: rgb(0x98bb6c),
                warning: rgb(0xe6c384),
                error: rgb(0xc34043),
                special: rgb(0xd27e99),
            },
        }
    }
}

/// Index into `ThemeId::ALL` of the currently active theme.
static ACTIVE_THEME: AtomicU8 = AtomicU8::new(0);

/// Serializes tests that touch the process-wide active theme.
///
/// Cargo runs unit tests in parallel within one process, and the active
/// theme is shared global state; any test that calls `set_active` (directly
/// or through `App::open_theme_picker`/key handling) must hold this guard
/// for its duration and restore the default theme before releasing it, so
/// unrelated tests never observe an unexpected palette.
#[cfg(test)]
pub(crate) static TEST_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Switch the active theme. Takes effect on the next render.
pub fn set_active(id: ThemeId) {
    ACTIVE_THEME.store(theme_index(id) as u8, Ordering::Relaxed);
}

/// The currently active theme's identifier.
pub fn active_id() -> ThemeId {
    let index = ACTIVE_THEME.load(Ordering::Relaxed) as usize;
    ThemeId::ALL.get(index).copied().unwrap_or_default()
}

fn theme_index(id: ThemeId) -> usize {
    ThemeId::ALL.iter().position(|&t| t == id).unwrap_or(0)
}

/// Shorthand for [`Theme::current`], so render code can write
/// `theme::current().heading()` without naming the `Theme` type.
pub fn current() -> Theme {
    Theme::current()
}

/// Reusable semantic styles for the TUI, resolved from the active palette.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    palette: Option<Palette>,
}

impl Theme {
    /// The theme in effect right now. Cheap to call per-widget; render code
    /// is expected to call this on every frame rather than cache it, so a
    /// theme change take effect immediately.
    pub fn current() -> Self {
        Self {
            palette: Some(active_id().palette()),
        }
    }

    /// Colors with all fields unset, kept only for terminals or tests that
    /// need to verify that focus and selection remain legible through
    /// modifiers alone, without relying on color.
    #[cfg(test)]
    pub const REDUCED: Self = Self { palette: None };

    /// The raw palette backing this theme, e.g. for swatch previews.
    pub fn palette(self) -> Palette {
        self.palette.unwrap_or(ThemeId::CatppuccinMocha.palette())
    }

    /// Full-frame canvas paint: background and default body text color.
    pub fn canvas(self) -> Style {
        self.painted(|p| p.background)
    }

    /// Tab bar and footer background.
    pub fn chrome(self) -> Style {
        self.painted(|p| p.chrome)
    }

    /// Dialog and panel background.
    pub fn surface(self) -> Style {
        self.painted(|p| p.surface)
    }

    pub fn border(self, focused: bool) -> Style {
        if focused {
            self.color(|p| p.accent).add_modifier(Modifier::BOLD)
        } else {
            self.color(|p| p.border)
        }
    }

    pub fn heading(self) -> Style {
        self.color(|p| p.accent).add_modifier(Modifier::BOLD)
    }

    pub fn label(self) -> Style {
        self.color(|p| p.secondary).add_modifier(Modifier::BOLD)
    }

    pub fn muted(self) -> Style {
        self.color(|p| p.muted).add_modifier(Modifier::DIM)
    }

    pub fn focused(self) -> Style {
        self.color(|p| p.accent).add_modifier(Modifier::BOLD)
    }

    pub fn selected(self) -> Style {
        self.painted(|p| p.selection_bg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn success(self) -> Style {
        self.color(|p| p.success).add_modifier(Modifier::BOLD)
    }

    pub fn warning(self) -> Style {
        self.color(|p| p.warning).add_modifier(Modifier::BOLD)
    }

    pub fn error(self) -> Style {
        self.color(|p| p.error).add_modifier(Modifier::BOLD)
    }

    pub fn progress(self) -> Style {
        self.color(|p| p.accent).add_modifier(Modifier::BOLD)
    }

    pub fn disabled(self) -> Style {
        self.color(|p| p.muted)
            .add_modifier(Modifier::DIM | Modifier::CROSSED_OUT)
    }

    pub fn key(self) -> Style {
        self.focused()
    }

    /// Content highlight color for inline emphasis that is not a keybinding
    /// hint (a commit hash, a count, a selected path). Bold, no underline.
    pub fn accent(self) -> Style {
        self.color(|p| p.accent).add_modifier(Modifier::BOLD)
    }

    /// Bold emphasis with no explicit color, so it reads correctly against
    /// both dark and light palettes by inheriting the painted foreground.
    pub fn emphasis(self) -> Style {
        Style::default().add_modifier(Modifier::BOLD)
    }

    /// Directory entries in file pickers.
    pub fn directory(self) -> Style {
        self.color(|p| p.secondary).add_modifier(Modifier::BOLD)
    }

    /// Symlink entries in file pickers.
    pub fn symlink(self) -> Style {
        self.color(|p| p.special)
    }

    /// Dim the inactive UI while a modal owns input.
    pub fn backdrop(self) -> Style {
        Style::default().add_modifier(Modifier::DIM)
    }

    /// Frame color for dialog borders.
    pub fn dialog(self) -> Style {
        self.color(|p| p.accent).add_modifier(Modifier::BOLD)
    }

    pub fn status(self, kind: StatusKind) -> Style {
        match kind {
            StatusKind::Success => self.success(),
            StatusKind::Running => self.progress(),
            StatusKind::Warning => self.warning(),
            StatusKind::Error => self.error(),
        }
    }

    fn color(self, pick: impl Fn(&Palette) -> Color) -> Style {
        match self.palette {
            Some(palette) => Style::default().fg(pick(&palette)),
            None => Style::default(),
        }
    }

    /// A background fill paired with the theme's default foreground, used
    /// for the canvas, chrome, and surface roles. In the color-free
    /// `REDUCED` test palette this paints nothing, matching `color`.
    fn painted(self, pick: impl Fn(&Palette) -> Color) -> Style {
        match self.palette {
            Some(palette) => Style::default().fg(palette.foreground).bg(pick(&palette)),
            None => Style::default(),
        }
    }
}

/// The user's persisted theme choice, stored at `<config_dir>/theme.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ThemePreference {
    theme: String,
}

/// Load the persisted theme choice, if any. Missing or corrupt preference
/// files are treated as "no preference" so a stale or hand-edited file never
/// blocks startup; the caller falls back to the default theme.
pub fn load_preference(config_dir: &Path) -> Option<ThemeId> {
    let path = config_dir.join(crate::app::THEME_FILE_NAME);
    let text = std::fs::read_to_string(path).ok()?;
    let preference: ThemePreference = toml::from_str(&text).ok()?;
    ThemeId::from_slug(&preference.theme)
}

/// Persist the given theme choice atomically to `<config_dir>/theme.toml`.
pub fn save_preference(config_dir: &Path, id: ThemeId) -> std::io::Result<()> {
    if !config_dir.exists() {
        std::fs::create_dir_all(config_dir)?;
    }

    let preference = ThemePreference {
        theme: id.slug().to_string(),
    };
    let text = toml::to_string_pretty(&preference).map_err(std::io::Error::other)?;

    let mut tmp = tempfile::NamedTempFile::new_in(config_dir)?;
    tmp.write_all(text.as_bytes())?;
    tmp.flush()?;
    tmp.persist(config_dir.join(crate::app::THEME_FILE_NAME))
        .map_err(|e| e.error)?;

    Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/tui/theme.rs"]
mod tests;
