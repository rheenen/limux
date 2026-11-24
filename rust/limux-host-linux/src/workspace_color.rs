//! Palette for coloured sidebar workspace rows.
//!
//! Every colour is dark enough to carry white text. The palette, its
//! persisted names and the CSS that paints it all live here.

use serde::Deserialize;

/// CSS class on a sidebar row box that carries any palette colour.
pub const COLORED_ROW_CSS_CLASS: &str = "limux-ws-colored";
/// CSS class on the buttons that hold a swatch in the workspace context menu.
pub const SWATCH_BUTTON_CSS_CLASS: &str = "limux-ws-color-swatch-btn";
/// CSS class on the swatch label inside each of those buttons.
pub const SWATCH_CSS_CLASS: &str = "limux-ws-color-swatch";
/// CSS class on the swatch that clears the colour.
pub const NO_COLOR_SWATCH_CSS_CLASS: &str = "limux-ws-color-none";

#[derive(serde::Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceColor {
    Red,
    Orange,
    Amber,
    Green,
    Teal,
    Blue,
    Purple,
    Pink,
}

impl WorkspaceColor {
    pub const ALL: [WorkspaceColor; 8] = [
        Self::Red,
        Self::Orange,
        Self::Amber,
        Self::Green,
        Self::Teal,
        Self::Blue,
        Self::Purple,
        Self::Pink,
    ];

    /// Name used in the session file and on the control socket.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Orange => "orange",
            Self::Amber => "amber",
            Self::Green => "green",
            Self::Teal => "teal",
            Self::Blue => "blue",
            Self::Purple => "purple",
            Self::Pink => "pink",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.trim();
        Self::ALL
            .into_iter()
            .find(|color| color.as_str().eq_ignore_ascii_case(name))
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Red => "Red",
            Self::Orange => "Orange",
            Self::Amber => "Amber",
            Self::Green => "Green",
            Self::Teal => "Teal",
            Self::Blue => "Blue",
            Self::Purple => "Purple",
            Self::Pink => "Pink",
        }
    }

    fn rgb(self) -> (u8, u8, u8) {
        match self {
            Self::Red => (0xB9, 0x1C, 0x1C),
            Self::Orange => (0xAE, 0x3A, 0x0B),
            Self::Amber => (0x8A, 0x54, 0x06),
            Self::Green => (0x16, 0x6F, 0x37),
            Self::Teal => (0x0E, 0x6B, 0x64),
            Self::Blue => (0x1D, 0x4E, 0xD8),
            Self::Purple => (0x7E, 0x22, 0xCE),
            Self::Pink => (0xBE, 0x18, 0x5D),
        }
    }

    pub fn css_class(self) -> &'static str {
        match self {
            Self::Red => "limux-ws-color-red",
            Self::Orange => "limux-ws-color-orange",
            Self::Amber => "limux-ws-color-amber",
            Self::Green => "limux-ws-color-green",
            Self::Teal => "limux-ws-color-teal",
            Self::Blue => "limux-ws-color-blue",
            Self::Purple => "limux-ws-color-purple",
            Self::Pink => "limux-ws-color-pink",
        }
    }

    /// The colour as a CSS hex value, for styles generated elsewhere.
    pub fn css_hex(self) -> String {
        hex(self.rgb())
    }
}

/// Comma-separated palette names, for error messages.
pub fn color_names() -> String {
    WorkspaceColor::ALL.map(WorkspaceColor::as_str).join(", ")
}

/// Deserialize an optional colour, mapping an unknown name to `None` so a
/// session written by a newer build still loads.
pub fn deserialize_option<'de, D>(deserializer: D) -> Result<Option<WorkspaceColor>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let name = Option::<String>::deserialize(deserializer)?;
    Ok(name.as_deref().and_then(WorkspaceColor::from_name))
}

fn hex((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// Mix a colour toward black by `percent`, for rows that are not selected.
fn darken((r, g, b): (u8, u8, u8), percent: u16) -> (u8, u8, u8) {
    let mix = |channel: u8| (u16::from(channel) * (100 - percent) / 100) as u8;
    (mix(r), mix(g), mix(b))
}

/// How far an unselected row's colour is dimmed, at rest and under the pointer.
const DIM_PERCENT: u16 = 45;
const DIM_HOVER_PERCENT: u16 = 28;

/// Mix a colour toward white by `percent`, for the selected row's hover state.
fn lighten((r, g, b): (u8, u8, u8), percent: u16) -> (u8, u8, u8) {
    let mix = |channel: u8| {
        let channel = u16::from(channel);
        (channel + (255 - channel) * percent / 100) as u8
    };
    (mix(r), mix(g), mix(b))
}

/// How far an unselected row's colour is mixed toward white in light mode,
/// at rest and under the pointer. The row is then a tint that carries the
/// window's dark text; only the selected row shows the full colour.
const TINT_PERCENT: u16 = 80;
const TINT_HOVER_PERCENT: u16 = 70;

/// The darkest text Adwaita's light window colour gives on a tint:
/// `rgb(0 0 6 / 80%)` over the tint. The tests check the contrast with it.
#[cfg(test)]
const LIGHT_TEXT_ALPHA: f64 = 0.8;

/// Text and icon colours on the rows that are NOT selected. In dark mode the
/// dimmed rows are dark and carry white; in light mode the tinted rows are
/// light and carry the window's own text colour.
fn unselected_row_text_css(dark: bool) -> String {
    let (primary, secondary, icon) = if dark {
        ("alpha(white, 0.88)", "white", "alpha(white, 0.8)")
    } else {
        (
            "alpha(@window_fg_color, 0.88)",
            "alpha(@window_fg_color, 0.8)",
            "alpha(@window_fg_color, 0.6)",
        )
    };
    let strong = if dark { "white" } else { "@window_fg_color" };
    format!(
        r#"
/* ---------- Coloured workspace rows ---------- */
.limux-sidebar-list row .limux-ws-colored .limux-ws-name {{
    color: {primary};
}}
.limux-sidebar-list row .limux-ws-colored .limux-notify-msg-unread,
.limux-sidebar-list row .limux-ws-colored .limux-notify-dot {{
    color: {strong};
}}
.limux-sidebar-list row .limux-ws-colored .limux-ws-path,
.limux-sidebar-list row .limux-ws-colored .limux-notify-msg {{
    color: {secondary};
}}
.limux-sidebar-list row .limux-ws-colored .limux-ws-star-btn,
.limux-sidebar-list row .limux-ws-colored .limux-ws-close-btn {{
    color: {icon};
}}
.limux-sidebar-list row .limux-ws-colored .limux-ws-star-btn:hover,
.limux-sidebar-list row .limux-ws-colored .limux-ws-star-btn-active,
.limux-sidebar-list row .limux-ws-colored .limux-ws-close-btn:hover {{
    color: {strong};
}}
.limux-sidebar-row-unread.limux-ws-colored {{
    border-left-color: {strong};
}}
"#
    )
}

/// The selected row shows the full colour in both modes, so it carries white.
const SELECTED_ROW_CSS: &str = r#"
.limux-sidebar-list row:selected .limux-sidebar-row-box.limux-ws-colored {
    box-shadow: inset 0 0 0 2px alpha(white, 0.9);
}
.limux-sidebar-list row:selected .limux-ws-colored .limux-ws-name {
    color: white;
}
.limux-sidebar-list row:selected .limux-ws-colored .limux-notify-msg-unread,
.limux-sidebar-list row:selected .limux-ws-colored .limux-notify-dot,
.limux-sidebar-list row:selected .limux-ws-colored .limux-ws-path,
.limux-sidebar-list row:selected .limux-ws-colored .limux-notify-msg {
    color: white;
}
.limux-sidebar-list row:selected .limux-ws-colored .limux-ws-star-btn,
.limux-sidebar-list row:selected .limux-ws-colored .limux-ws-close-btn {
    color: alpha(white, 0.8);
}
.limux-sidebar-list row:selected .limux-ws-colored .limux-ws-star-btn:hover,
.limux-sidebar-list row:selected .limux-ws-colored .limux-ws-star-btn-active,
.limux-sidebar-list row:selected .limux-ws-colored .limux-ws-close-btn:hover {
    color: white;
}
.limux-sidebar-list row:selected .limux-sidebar-row-unread.limux-ws-colored {
    border-left-color: white;
}
"#;

const SWATCH_CSS: &str = r#"
/* The colour sits on a label inside the button: a theme installed as user
   CSS (~/.config/gtk-4.0/gtk.css) out-ranks application CSS for buttons. */
.limux-ws-color-swatch {
    min-width: 22px;
    min-height: 22px;
    padding: 0;
    border-radius: 999px;
    color: white;
    font-size: 11px;
}
.limux-ws-color-swatch.limux-ws-color-none {
    background: transparent;
    color: @window_fg_color;
    box-shadow: inset 0 0 0 1px alpha(@window_fg_color, 0.45);
}
button:hover > .limux-ws-color-swatch.limux-ws-color-none {
    background: alpha(@window_fg_color, 0.1);
}
"#;

/// Sizing of the swatch buttons. Loaded above user priority, because a theme
/// installed as user CSS otherwise pads every button and spreads the swatches.
pub const SWATCH_BUTTON_CSS: &str = r#"
button.limux-ws-color-swatch-btn {
    min-width: 0;
    min-height: 0;
    padding: 2px;
    margin: 0;
    border-radius: 999px;
}
"#;

/// The backgrounds of an unselected row, at rest and under the pointer:
/// dimmed toward black in dark mode, tinted toward white in light mode.
fn unselected_backgrounds(color: WorkspaceColor, dark: bool) -> [(u8, u8, u8); 2] {
    if dark {
        [
            darken(color.rgb(), DIM_PERCENT),
            darken(color.rgb(), DIM_HOVER_PERCENT),
        ]
    } else {
        [
            lighten(color.rgb(), TINT_PERCENT),
            lighten(color.rgb(), TINT_HOVER_PERCENT),
        ]
    }
}

/// CSS for coloured rows and the menu swatches, generated from the palette
/// for the dark or the light colour scheme.
pub fn workspace_color_css(dark: bool) -> String {
    let mut css = unselected_row_text_css(dark);
    css.push_str(SELECTED_ROW_CSS);
    css.push_str(SWATCH_CSS);
    for color in WorkspaceColor::ALL {
        let class = color.css_class();
        let base = hex(color.rgb());
        let hover = hex(lighten(color.rgb(), 12));
        let [rest, rest_hover] = unselected_backgrounds(color, dark).map(hex);
        // The extra class out-ranks the generic hover/selected row backgrounds.
        // Only the selected row shows the full colour; the others are dimmed
        // (dark mode) or tinted (light mode).
        css.push_str(&format!(
            ".limux-sidebar-list row .limux-sidebar-row-box.{class} {{\n    background: {rest};\n}}\n\
             .limux-sidebar-list row:hover .limux-sidebar-row-box.{class} {{\n    background: {rest_hover};\n}}\n\
             .limux-sidebar-list row:selected .limux-sidebar-row-box.{class},\n\
             .{SWATCH_CSS_CLASS}.{class} {{\n    background: {base};\n}}\n\
             .limux-sidebar-list row:selected:hover .limux-sidebar-row-box.{class},\n\
             button:hover > .{SWATCH_CSS_CLASS}.{class} {{\n    background: {hover};\n}}\n"
        ));
    }
    css
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relative_luminance((r, g, b): (u8, u8, u8)) -> f64 {
        let channel = |value: u8| {
            let value = f64::from(value) / 255.0;
            if value <= 0.03928 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
    }

    fn contrast_with_white(rgb: (u8, u8, u8)) -> f64 {
        1.05 / (relative_luminance(rgb) + 0.05)
    }

    #[test]
    fn palette_has_eight_distinct_colors() {
        let mut names: Vec<&str> = WorkspaceColor::ALL.map(WorkspaceColor::as_str).to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 8);
    }

    #[test]
    fn names_round_trip() {
        for color in WorkspaceColor::ALL {
            assert_eq!(WorkspaceColor::from_name(color.as_str()), Some(color));
            assert_eq!(
                serde_json::to_value(color).unwrap(),
                serde_json::json!(color.as_str())
            );
        }
        assert_eq!(
            WorkspaceColor::from_name(" Blue "),
            Some(WorkspaceColor::Blue)
        );
        assert_eq!(WorkspaceColor::from_name("chartreuse"), None);
        assert_eq!(WorkspaceColor::from_name(""), None);
    }

    #[test]
    fn every_color_is_readable_under_white_text() {
        for color in WorkspaceColor::ALL {
            // The hover state is the lightest the row background gets.
            for rgb in [color.rgb(), lighten(color.rgb(), 12)] {
                let contrast = contrast_with_white(rgb);
                assert!(
                    contrast >= 4.5,
                    "{} ({}) has contrast {contrast:.2} against white",
                    color.as_str(),
                    hex(rgb),
                );
            }
        }
    }

    fn contrast((fr, fg, fb): (u8, u8, u8), background: (u8, u8, u8)) -> f64 {
        let (light, dark) = {
            let a = relative_luminance((fr, fg, fb));
            let b = relative_luminance(background);
            (a.max(b), a.min(b))
        };
        (light + 0.05) / (dark + 0.05)
    }

    /// Text drawn at `alpha` over `background`.
    fn composite(text: (u8, u8, u8), alpha: f64, background: (u8, u8, u8)) -> (u8, u8, u8) {
        let mix =
            |fg: u8, bg: u8| (f64::from(fg) * alpha + f64::from(bg) * (1.0 - alpha)).round() as u8;
        (
            mix(text.0, background.0),
            mix(text.1, background.1),
            mix(text.2, background.2),
        )
    }

    #[test]
    fn secondary_text_is_readable_in_every_dark_row_state() {
        for color in WorkspaceColor::ALL {
            let [rest, rest_hover] = unselected_backgrounds(color, true);
            for background in [rest, rest_hover, color.rgb(), lighten(color.rgb(), 12)] {
                let contrast = contrast((255, 255, 255), background);
                assert!(
                    contrast >= 4.5,
                    "{} on {} has secondary-text contrast {contrast:.2}",
                    color.as_str(),
                    hex(background),
                );
            }
        }
    }

    #[test]
    fn light_tints_carry_the_window_text() {
        // Adwaita's light window text is rgb(0 0 6 / 80%); the secondary
        // text draws it at 80% of that again.
        for color in WorkspaceColor::ALL {
            for background in unselected_backgrounds(color, false) {
                for alpha in [LIGHT_TEXT_ALPHA, LIGHT_TEXT_ALPHA * 0.8] {
                    let text = composite((0, 0, 6), alpha, background);
                    let contrast = contrast(text, background);
                    assert!(
                        contrast >= 4.5,
                        "{} tint {} has text contrast {contrast:.2}",
                        color.as_str(),
                        hex(background),
                    );
                }
                // A tint stays lighter than the selected row, so the full
                // colour still marks the selection.
                assert!(relative_luminance(background) > relative_luminance(color.rgb()));
            }
        }
    }

    #[test]
    fn css_paints_every_color_for_rows_and_swatches() {
        for dark in [true, false] {
            let css = workspace_color_css(dark);
            for color in WorkspaceColor::ALL {
                let class = color.css_class();
                assert!(css.contains(&format!(".limux-sidebar-row-box.{class}")));
                assert!(css.contains(&format!("\n.{SWATCH_CSS_CLASS}.{class} {{")));
                // Themes loaded as user CSS restyle buttons, so no swatch colour
                // may depend on a button background.
                assert!(!css.contains(&format!("button.{SWATCH_CSS_CLASS}.{class}")));
                assert!(css.contains(&hex(color.rgb())));
                for background in unselected_backgrounds(color, dark) {
                    assert!(css.contains(&hex(background)));
                }
            }
            assert!(css.contains(COLORED_ROW_CSS_CLASS));
            assert!(css.contains(&format!(
                "\n.{SWATCH_CSS_CLASS}.{NO_COLOR_SWATCH_CSS_CLASS} {{"
            )));
        }
        // Unselected rows are dimmed in dark mode, so they stay darker than
        // the selected one.
        for color in WorkspaceColor::ALL {
            let [rest, _] = unselected_backgrounds(color, true);
            assert!(relative_luminance(rest) < relative_luminance(color.rgb()));
        }
        assert!(SWATCH_BUTTON_CSS.contains(&format!("button.{SWATCH_BUTTON_CSS_CLASS} {{")));
    }

    #[test]
    fn unselected_text_follows_the_color_scheme() {
        let name_rule = ".limux-sidebar-list row .limux-ws-colored .limux-ws-name {\n    color: ";
        assert!(workspace_color_css(true).contains(&format!("{name_rule}alpha(white")));
        assert!(workspace_color_css(false).contains(&format!("{name_rule}alpha(@window_fg_color")));
    }
}
