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

/// The window actions of the two filter buttons (no parameter).
pub const HIDE_DONE_ACTION: &str = "zoom-hide-done";
pub const HIDE_SHELLS_ACTION: &str = "zoom-hide-shells";
const FILTER_ON_CSS: &str = "limux-zoom-filter-on";

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
        lines.push(NO_SUBAGENTS.to_string());
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

/// The filter buttons only act on /dip runs, so they follow the same
/// rule as the deeper levels: usable only when there are subagents.
pub fn filters_enabled(status: ZoomStatus) -> bool {
    status.can_zoom
}

/// The tooltip of a filter button: its own hint, plus why it is off.
pub fn filter_hint(hint: &str, enabled: bool) -> String {
    if enabled {
        hint.to_string()
    } else {
        format!("{hint}\n{NO_SUBAGENTS}")
    }
}

const NO_SUBAGENTS: &str = "No /dip subagents in the focused terminal";

pub fn hide_done_hint(on: bool) -> &'static str {
    if on {
        "Finished subagents and shells are hidden. Click to show them."
    } else {
        "Hide finished subagents and shells"
    }
}

pub fn hide_shells_hint(on: bool) -> &'static str {
    if on {
        "Background shells are hidden; only subagents show. Click to show them."
    } else {
        "Hide background shells; show only subagents"
    }
}

pub struct ZoomBar {
    pub root: gtk::Box,
    buttons: Vec<(ZoomLevel, gtk::Button)>,
    hide_done: gtk::Button,
    hide_shells: gtk::Button,
}

fn filter_button(label: &str, icon: &str, action: &str) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.append(&gtk::Image::from_icon_name(icon));
    content.append(&gtk::Label::new(Some(label)));
    let button = gtk::Button::new();
    button.set_child(Some(&content));
    button.add_css_class("flat");
    button.add_css_class("limux-top-bar-btn");
    button.add_css_class("limux-zoom-filter");
    button.set_focus_on_click(false);
    button.set_valign(gtk::Align::Center);
    button.set_action_name(Some(&format!("win.{action}")));
    button
}

impl ZoomBar {
    /// Each button starts `ACTION` with the index of its level.
    pub fn new() -> Self {
        install_css();
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        root.add_css_class("limux-zoom-bar");
        let levels = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        levels.add_css_class("linked");
        root.append(&levels);
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
                levels.append(&button);
                (level, button)
            })
            .collect();
        let separator = gtk::Separator::new(gtk::Orientation::Vertical);
        separator.set_margin_top(8);
        separator.set_margin_bottom(8);
        root.append(&separator);
        let hide_done = filter_button("Hide done", "object-select-symbolic", HIDE_DONE_ACTION);
        let hide_shells = filter_button(
            "Hide shells",
            "utilities-terminal-symbolic",
            HIDE_SHELLS_ACTION,
        );
        root.append(&hide_done);
        root.append(&hide_shells);
        Self {
            root,
            buttons,
            hide_done,
            hide_shells,
        }
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
        for (button, on, hint) in [
            (
                &self.hide_done,
                status.filter.hide_done,
                hide_done_hint(status.filter.hide_done),
            ),
            (
                &self.hide_shells,
                status.filter.hide_shells,
                hide_shells_hint(status.filter.hide_shells),
            ),
        ] {
            let enabled = filters_enabled(status);
            button.set_sensitive(enabled);
            if on {
                button.add_css_class(FILTER_ON_CSS);
            } else {
                button.remove_css_class(FILTER_ON_CSS);
            }
            button.set_tooltip_text(Some(&filter_hint(hint, enabled)));
            button.update_state(&[gtk::accessible::State::Pressed(if on {
                gtk::AccessibleTristate::True
            } else {
                gtk::AccessibleTristate::False
            })]);
        }
    }
}

/// Two classes per selector: the app CSS sets `.limux-top-bar-btn`
/// (background none, dim text) at the same priority and loads again
/// after a settings change, so a one-class rule here would lose to it.
const ZOOM_BAR_CSS: &str = "\
.limux-top-bar-btn.limux-zoom-level { min-width: 28px; padding: 0 6px; font-size: 0.85em; }\n\
.limux-top-bar-btn.limux-zoom-level-active { background: alpha(#f0b45e, 0.35); color: @window_fg_color; font-weight: bold; }\n\
.limux-top-bar-btn.limux-zoom-filter { padding: 0 8px; font-size: 0.85em; }\n\
.limux-top-bar-btn.limux-zoom-filter-on { background: alpha(#6aa6ff, 0.3); color: @window_fg_color; }\n\
.limux-top-bar-btn.limux-zoom-level:disabled, .limux-top-bar-btn.limux-zoom-filter:disabled { opacity: 0.35; }\n";

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
    provider.load_from_data(ZOOM_BAR_CSS);
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
        ZoomStatus {
            level,
            can_zoom,
            filter: Default::default(),
        }
    }

    #[test]
    fn filters_are_usable_only_with_subagents() {
        assert!(!filters_enabled(status(ZoomLevel::Z0, false)));
        assert!(filters_enabled(status(ZoomLevel::Z0, true)));
        assert!(filters_enabled(status(ZoomLevel::Z2, true)));
        let off = filter_hint(hide_shells_hint(false), false);
        assert!(off.starts_with("Hide background shells"));
        assert!(off.ends_with("No /dip subagents in the focused terminal"));
        assert_eq!(filter_hint("x", true), "x");
    }

    #[test]
    fn filter_hints_say_what_a_click_does() {
        assert!(hide_done_hint(false).starts_with("Hide finished"));
        assert!(hide_done_hint(true).contains("Click to show"));
        assert!(hide_shells_hint(false).contains("only subagents"));
        assert!(hide_shells_hint(true).contains("Click to show"));
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

    #[test]
    fn every_zoom_bar_rule_outranks_the_top_bar_button_rule() {
        for rule in ZOOM_BAR_CSS.lines() {
            let selectors = rule.split('{').next().unwrap();
            for selector in selectors.split(',') {
                assert!(
                    selector
                        .trim()
                        .starts_with(".limux-top-bar-btn.limux-zoom-"),
                    "{selector}"
                );
            }
        }
    }
}
