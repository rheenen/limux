//! The zoom-level buttons in the top bar: one button per level (Z0–Z3).
//! The hint text is pure; the GTK part only shows it.

use gtk::prelude::*;
use gtk4 as gtk;

use super::geometry::ZoomLevel;
use super::view::ZoomStatus;

/// The window action that the level buttons start; its target is the
/// index of the level in `LEVELS`.
pub const ACTION: &str = "win.zoom-level";
pub const ACTION_NAME: &str = "zoom-level";

pub const LEVELS: [ZoomLevel; 4] = [ZoomLevel::Z0, ZoomLevel::Z1, ZoomLevel::Z2, ZoomLevel::Z3];
const ACTIVE_CSS: &str = "limux-zoom-level-active";

pub fn level_label(level: ZoomLevel) -> &'static str {
    match level {
        ZoomLevel::Z0 => "Z0",
        ZoomLevel::Z1 => "Z1",
        ZoomLevel::Z2 => "Z2",
        ZoomLevel::Z3 => "Z3",
    }
}

fn level_meaning(level: ZoomLevel) -> &'static str {
    match level {
        ZoomLevel::Z0 => "Terminal only, splits shown",
        ZoomLevel::Z1 => "Terminal and large subagent tiles",
        ZoomLevel::Z2 => "Terminal and more, smaller tiles",
        ZoomLevel::Z3 => "Terminal and the most tiles",
    }
}

/// The tooltip of one level button. `zoom_out` and `zoom_in` are the
/// display labels of the shortcuts (None when the user unbound one).
pub fn level_hint(
    level: ZoomLevel,
    can_zoom: bool,
    zoom_out: Option<&str>,
    zoom_in: Option<&str>,
) -> String {
    let mut lines = vec![format!(
        "Zoom level {}: {}",
        &level_label(level)[1..],
        level_meaning(level)
    )];
    if !can_zoom {
        lines.push("No /dip subagents in the focused terminal".to_string());
    }
    let keys: Vec<String> = [("zoom out", zoom_out), ("zoom in", zoom_in)]
        .into_iter()
        .filter_map(|(what, key)| key.map(|key| format!("{key} to {what}")))
        .collect();
    if !keys.is_empty() {
        lines.push(format!("{}.", keys.join(", ")));
    }
    lines.push("Ctrl+mouse wheel over a terminal also zooms.".to_string());
    lines.join("\n")
}

/// A level button is usable when it leads somewhere: Z0 only while
/// zoomed, a deeper level only when there are subagents to show.
pub fn level_enabled(level: ZoomLevel, status: ZoomStatus) -> bool {
    if level.is_zoomed() {
        status.can_zoom
    } else {
        status.level.is_zoomed()
    }
}

pub struct ZoomBar {
    pub root: gtk::Box,
    buttons: Vec<(ZoomLevel, gtk::Button)>,
}

impl ZoomBar {
    /// Each button starts `ACTION` with the index of its level.
    pub fn new() -> Self {
        install_css();
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("linked");
        root.add_css_class("limux-zoom-bar");
        root.set_valign(gtk::Align::Center);
        root.set_margin_start(6);
        root.set_margin_end(6);
        let buttons = LEVELS
            .into_iter()
            .enumerate()
            .map(|(index, level)| {
                let button = gtk::Button::with_label(level_label(level));
                button.add_css_class("limux-top-bar-btn");
                button.add_css_class("limux-zoom-level");
                button.set_focus_on_click(false);
                button.set_action_name(Some(ACTION));
                button.set_action_target_value(Some(&(index as u32).to_variant()));
                root.append(&button);
                (level, button)
            })
            .collect();
        Self { root, buttons }
    }

    pub fn sync(&self, status: ZoomStatus, zoom_out: Option<&str>, zoom_in: Option<&str>) {
        for (level, button) in &self.buttons {
            if *level == status.level {
                button.add_css_class(ACTIVE_CSS);
            } else {
                button.remove_css_class(ACTIVE_CSS);
            }
            button.set_sensitive(*level == status.level || level_enabled(*level, status));
            let hint = level_hint(*level, status.can_zoom, zoom_out, zoom_in);
            button.set_tooltip_text(Some(&hint));
        }
    }
}

fn install_css() {
    thread_local! {
        static INSTALLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if INSTALLED.with(|done| done.replace(true)) {
        return;
    }
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        ".limux-zoom-level { min-width: 28px; padding: 0 6px; font-size: 0.85em; }\n\
         .limux-zoom-level-active { background: alpha(#f0b45e, 0.35); font-weight: bold; }\n",
    );
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(level: ZoomLevel, can_zoom: bool) -> ZoomStatus {
        ZoomStatus { level, can_zoom }
    }

    #[test]
    fn hint_names_both_shortcuts_and_the_wheel() {
        let hint = level_hint(
            ZoomLevel::Z2,
            true,
            Some("Ctrl+Alt+Z"),
            Some("Ctrl+Alt+Shift+Z"),
        );
        assert!(hint.starts_with("Zoom level 2: "));
        assert!(hint.contains("Ctrl+Alt+Z to zoom out, Ctrl+Alt+Shift+Z to zoom in."));
        assert!(hint.ends_with("Ctrl+mouse wheel over a terminal also zooms."));
        assert!(!hint.contains("No /dip subagents"));
    }

    #[test]
    fn hint_skips_an_unbound_shortcut_and_says_why_a_level_is_off() {
        let hint = level_hint(ZoomLevel::Z1, false, None, Some("Ctrl+Alt+Shift+Z"));
        assert!(hint.contains("No /dip subagents in the focused terminal"));
        assert!(hint.contains("Ctrl+Alt+Shift+Z to zoom in."));
        assert!(!hint.contains("zoom out"));
        let none = level_hint(ZoomLevel::Z1, true, None, None);
        assert_eq!(none.lines().count(), 2);
    }

    #[test]
    fn z0_is_usable_only_while_zoomed_and_deeper_levels_need_subagents() {
        assert!(!level_enabled(ZoomLevel::Z0, status(ZoomLevel::Z0, true)));
        assert!(level_enabled(ZoomLevel::Z0, status(ZoomLevel::Z2, true)));
        assert!(level_enabled(ZoomLevel::Z3, status(ZoomLevel::Z0, true)));
        assert!(!level_enabled(ZoomLevel::Z1, status(ZoomLevel::Z0, false)));
    }
}
