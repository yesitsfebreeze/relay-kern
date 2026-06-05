use serde::Deserialize;

const DEFAULT_THEME_TOML: &str = include_str!("default_theme.toml");

/// Hard fallback foreground RGB when the active theme leaves `foreground` unset
/// or unparseable. Light grey, deliberately not pure white so it reads as
/// "default text" rather than "highlighted".
pub const FALLBACK_FG_RGB: (u8, u8, u8) = (235, 235, 235);

/// Hard fallback background RGB when the active theme leaves `background` unset
/// or unparseable. Pure black — the safest bet on any terminal emulator.
pub const FALLBACK_BG_RGB: (u8, u8, u8) = (0, 0, 0);

/// Hard fallback cursor RGB. Pure white — used by [`ThemeConfig::cursor_fade`]
/// when neither `cursor` nor a per-endpoint override resolves.
pub const FALLBACK_CURSOR_RGB: (u8, u8, u8) = (255, 255, 255);

/// Semantic palette. Two brand colours (primary + alert), each with dim/bright
/// variants; plus surfaces, text, and indicators. All fields optional — a user
/// file can override only the slots it cares about and inherit the rest.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ThemeConfig {
    #[serde(default)] pub name:             Option<String>,

    // Surfaces
    #[serde(default)] pub background: Option<String>,
    #[serde(default)] pub surface:    Option<String>,

    // Text
    #[serde(default)] pub foreground: Option<String>,
    #[serde(default)] pub muted:      Option<String>,

    // Indicators
    #[serde(default)] pub cursor:    Option<String>,
    #[serde(default)] pub selection: Option<String>,
    #[serde(default)] pub emphasis:  Option<String>,

    // Cursor fade endpoints. Each field optional; missing fields fall back to
    // an "inverted" pair of `cursor` and `background`:
    //   A defaults: (cursor_fg=cursor, cursor_bg=background)
    //   B defaults: (cursor_fg=background, cursor_bg=cursor)
    #[serde(default)] pub cursor_fg_a: Option<String>,
    #[serde(default)] pub cursor_bg_a: Option<String>,
    #[serde(default)] pub cursor_fg_b: Option<String>,
    #[serde(default)] pub cursor_bg_b: Option<String>,

    // Primary brand colour
    #[serde(default)] pub primary:        Option<String>,
    #[serde(default)] pub primary_dim:    Option<String>,
    #[serde(default)] pub primary_bright: Option<String>,

    // Alert / danger colour
    #[serde(default)] pub alert:        Option<String>,
    #[serde(default)] pub alert_dim:    Option<String>,
    #[serde(default)] pub alert_bright: Option<String>,

    // Derived semantic colours
    #[serde(default)] pub warning: Option<String>,
    #[serde(default)] pub info:    Option<String>,
}

impl ThemeConfig {
    pub fn from_toml(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    pub fn baked_default() -> Self {
        Self::from_toml(DEFAULT_THEME_TOML).expect("default theme parses")
    }

    /// Merge `other` into `self`. Fields set in `other` win; unset fields leave `self` alone.
    pub fn merge(&mut self, other: ThemeConfig) {
        macro_rules! pull {
            ($f:ident) => { if other.$f.is_some() { self.$f = other.$f; } };
        }
        pull!(name);
        pull!(background); pull!(surface);
        pull!(foreground); pull!(muted);
        pull!(cursor); pull!(selection); pull!(emphasis);
        pull!(cursor_fg_a); pull!(cursor_bg_a);
        pull!(cursor_fg_b); pull!(cursor_bg_b);
        pull!(primary); pull!(primary_dim); pull!(primary_bright);
        pull!(alert); pull!(alert_dim); pull!(alert_bright);
        pull!(warning); pull!(info);
    }

    /// Project semantic colours onto the 16 ANSI palette slots.
    ///
    /// | ANSI | semantic           |
    /// |------|--------------------|
    /// |  0   | background         |
    /// |  1   | alert (alert)  |
    /// |  2   | primary (ok)       |
    /// |  3   | warning            |
    /// |  4   | info               |
    /// |  5   | primary_dim        |
    /// |  6   | info               |
    /// |  7   | foreground         |
    /// |  8   | muted              |
    /// |  9   | alert_bright   |
    /// | 10   | primary_bright     |
    /// | 11   | warning            |
    /// | 12   | info               |
    /// | 13   | primary_bright     |
    /// | 14   | info               |
    /// | 15   | emphasis           |
    fn ansi_slots(&self) -> [(u8, Option<&str>); 16] {
        [
            (0,  self.background.as_deref()),
            (1,  self.alert.as_deref()),
            (2,  self.primary.as_deref()),
            (3,  self.warning.as_deref()),
            (4,  self.info.as_deref()),
            (5,  self.primary_dim.as_deref()),
            (6,  self.info.as_deref()),
            (7,  self.foreground.as_deref()),
            (8,  self.muted.as_deref()),
            (9,  self.alert_bright.as_deref()),
            (10, self.primary_bright.as_deref()),
            (11, self.warning.as_deref()),
            (12, self.info.as_deref()),
            (13, self.primary_bright.as_deref()),
            (14, self.info.as_deref()),
            (15, self.emphasis.as_deref()),
        ]
    }

    /// Build the OSC sequence that installs this theme into the terminal.
    /// Emit once after entering raw mode + alt-screen.
    /// OSC 4 = palette slots 0-15, OSC 10/11/12 = fg/bg/cursor (the 3 additional).
    pub fn install_osc(&self) -> String {
        let mut s = String::with_capacity(640);
        for (idx, hex) in self.ansi_slots() {
            if let Some(rgb) = hex.and_then(parse_hex) {
                s.push_str(&format!(
                    "\x1b]4;{idx};rgb:{:02x}/{:02x}/{:02x}\x1b\\",
                    rgb.0, rgb.1, rgb.2
                ));
            }
        }
        if let Some(rgb) = self.foreground.as_deref().and_then(parse_hex) {
            s.push_str(&format!("\x1b]10;rgb:{:02x}/{:02x}/{:02x}\x1b\\", rgb.0, rgb.1, rgb.2));
        }
        if let Some(rgb) = self.background.as_deref().and_then(parse_hex) {
            s.push_str(&format!("\x1b]11;rgb:{:02x}/{:02x}/{:02x}\x1b\\", rgb.0, rgb.1, rgb.2));
        }
        if let Some(rgb) = self.cursor.as_deref().and_then(parse_hex) {
            s.push_str(&format!("\x1b]12;rgb:{:02x}/{:02x}/{:02x}\x1b\\", rgb.0, rgb.1, rgb.2));
        }
        s
    }

    /// Reset all OSC overrides installed by `install_osc`. Emit on Drop and panic hook.
    /// Explicit 0..15 slot list for OSC 104 — Windows Terminal ignores the bare form.
    pub fn reset_osc() -> &'static str {
        "\x1b]104;0;1;2;3;4;5;6;7;8;9;10;11;12;13;14;15\x1b\\\x1b]110\x1b\\\x1b]111\x1b\\\x1b]112\x1b\\"
    }

    /// Resolve cursor fade endpoints. Returns `((fg_a, bg_a), (fg_b, bg_b))`.
    /// Per-field default: A = (cursor, background), B = (background, cursor).
    /// Falls back to muted greys if neither cursor nor background is set.
    #[allow(clippy::type_complexity)] // explicit RGB fade-endpoint tuple reads clearer inline here
    pub fn cursor_fade(&self) -> (((u8, u8, u8), (u8, u8, u8)), ((u8, u8, u8), (u8, u8, u8))) {
        let cursor = self.cursor.as_deref().and_then(parse_hex).unwrap_or(FALLBACK_CURSOR_RGB);
        let bg     = self.background.as_deref().and_then(parse_hex).unwrap_or(FALLBACK_BG_RGB);
        let pick = |s: &Option<String>, fallback: (u8, u8, u8)| -> (u8, u8, u8) {
            s.as_deref().and_then(parse_hex).unwrap_or(fallback)
        };
        (
            (pick(&self.cursor_fg_a, cursor), pick(&self.cursor_bg_a, bg)),
            (pick(&self.cursor_fg_b, bg),     pick(&self.cursor_bg_b, cursor)),
        )
    }

    /// Return parsed RGB for a named semantic colour. Used for colours that cannot
    /// be expressed as one of the 16 ANSI slots (e.g. `surface`, `selection`).
    pub fn rgb(&self, name: ThemeColor) -> Option<(u8, u8, u8)> {
        let hex = match name {
            ThemeColor::Background      => self.background.as_deref(),
            ThemeColor::Surface         => self.surface.as_deref(),
            ThemeColor::Foreground      => self.foreground.as_deref(),
            ThemeColor::Muted           => self.muted.as_deref(),
            ThemeColor::Cursor          => self.cursor.as_deref(),
            ThemeColor::Selection       => self.selection.as_deref(),
            ThemeColor::Emphasis        => self.emphasis.as_deref(),
            ThemeColor::Primary         => self.primary.as_deref(),
            ThemeColor::PrimaryDim      => self.primary_dim.as_deref(),
            ThemeColor::PrimaryBright   => self.primary_bright.as_deref(),
            ThemeColor::Secondary       => self.alert.as_deref(),
            ThemeColor::SecondaryDim    => self.alert_dim.as_deref(),
            ThemeColor::SecondaryBright => self.alert_bright.as_deref(),
            ThemeColor::Warning         => self.warning.as_deref(),
            ThemeColor::Info            => self.info.as_deref(),
        };
        hex.and_then(parse_hex)
    }
}

/// Named colour selector for [`ThemeConfig::rgb`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ThemeColor {
    Background, Surface,
    Foreground, Muted,
    Cursor, Selection, Emphasis,
    Primary, PrimaryDim, PrimaryBright,
    Secondary, SecondaryDim, SecondaryBright,
    Warning, Info,
}

/// Parse `#rrggbb` or `rrggbb`. Returns `None` on malformed input.
pub fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let t = s.trim().trim_start_matches('#');
    if t.len() != 6 { return None; }
    let r = u8::from_str_radix(&t[0..2], 16).ok()?;
    let g = u8::from_str_radix(&t[2..4], 16).ok()?;
    let b = u8::from_str_radix(&t[4..6], 16).ok()?;
    Some((r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_parses() {
        let t = ThemeConfig::baked_default();
        assert_eq!(t.name.as_deref(), Some("relay"));
        assert!(t.primary.is_some());
        assert!(t.alert.is_some());
        assert!(t.background.is_some());
    }

    #[test]
    fn install_osc_emits_palette_plus_ui() {
        let t = ThemeConfig::baked_default();
        let osc = t.install_osc();
        // 16 palette slots + 3 ui (fg/bg/cursor) = 19 OSC sequences
        assert_eq!(osc.matches("\x1b]").count(), 19);
    }

    #[test]
    fn reset_osc_covers_palette_and_ui() {
        let r = ThemeConfig::reset_osc();
        assert!(r.contains("\x1b]104"));
        assert!(r.contains("\x1b]110"));
        assert!(r.contains("\x1b]111"));
        assert!(r.contains("\x1b]112"));
    }

    #[test]
    fn merge_overrides_only_set_fields() {
        let mut base = ThemeConfig::baked_default();
        let prior_alert = base.alert.clone();
        let over: ThemeConfig = toml::from_str(r##"primary = "#000000""##).unwrap();
        base.merge(over);
        assert_eq!(base.alert, prior_alert);
        assert_eq!(base.primary.as_deref(), Some("#000000"));
    }

    #[test]
    fn malformed_hex_skipped() {
        let mut t = ThemeConfig::baked_default();
        t.primary = Some("not-hex".into());
        // slot 2 + slot 5 + slot 10 + slot 13 drop out → 15 palette + 3 ui = 18
        // (exact count depends on which slots share primary values)
        let count = t.install_osc().matches("\x1b]").count();
        assert!(count < 19);
    }
}
