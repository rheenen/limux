//! The `zoom_view` object in settings.json: how Ctrl+wheel shows the
//! subagents of a terminal. Pure data; the GTK side reads it.

use serde_json::{json, Value};

pub const DEFAULT_MAIN_MIN_COLUMNS: u32 = 120;
pub const MIN_MAIN_MIN_COLUMNS: u32 = 80;
pub const MAX_MAIN_MIN_COLUMNS: u32 = 300;

/// Where the zoom view puts the subagent tiles around the main terminal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ZoomLayout {
    /// Main terminal at the left, plan rail, tiles at the right (default).
    #[default]
    Focus,
    /// Option B: one even grid; the main terminal takes a 2 x 2 block.
    Grid,
    /// Option C: main on top, executors below it, runners under them.
    Tree,
    /// Option D: main at full width, tiles in a row below it.
    Strip,
}

impl ZoomLayout {
    pub const ALL: [Self; 4] = [Self::Focus, Self::Grid, Self::Tree, Self::Strip];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Focus => "focus",
            Self::Grid => "grid",
            Self::Tree => "tree",
            Self::Strip => "strip",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|layout| layout.as_str() == value)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Focus => "Focus + tiles",
            Self::Grid => "Grid",
            Self::Tree => "Tree",
            Self::Strip => "Strip",
        }
    }

    pub fn dropdown_index(self) -> u32 {
        Self::ALL.iter().position(|l| *l == self).unwrap_or(0) as u32
    }

    pub fn from_dropdown_index(index: u32) -> Self {
        Self::ALL.get(index as usize).copied().unwrap_or_default()
    }
}

/// The state of the plan rail when a workspace opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RailState {
    #[default]
    Open,
    Collapsed,
    Closed,
}

impl RailState {
    pub const ALL: [Self; 3] = [Self::Open, Self::Collapsed, Self::Closed];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Collapsed => "collapsed",
            Self::Closed => "closed",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.as_str() == value)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Collapsed => "Collapsed",
            Self::Closed => "Closed",
        }
    }

    pub fn dropdown_index(self) -> u32 {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0) as u32
    }

    pub fn from_dropdown_index(index: u32) -> Self {
        Self::ALL.get(index as usize).copied().unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZoomViewConfig {
    pub layout: ZoomLayout,
    /// A zoom step never makes the main terminal narrower than this.
    pub main_min_columns: u32,
    pub plan_rail: RailState,
    /// Off: Ctrl+wheel goes to the terminal, as before.
    pub ctrl_wheel: bool,
    /// Finished runs stay as dimmed tiles until the session ends.
    pub show_finished: bool,
}

impl Default for ZoomViewConfig {
    fn default() -> Self {
        Self {
            layout: ZoomLayout::default(),
            main_min_columns: DEFAULT_MAIN_MIN_COLUMNS,
            plan_rail: RailState::default(),
            ctrl_wheel: true,
            show_finished: true,
        }
    }
}

impl ZoomViewConfig {
    /// Reads the `zoom_view` object; a missing or bad key keeps its default.
    pub fn from_value(value: Option<&Value>) -> Self {
        let defaults = Self::default();
        let Some(obj) = value.and_then(Value::as_object) else {
            return defaults;
        };
        let flag =
            |key: &str, default: bool| obj.get(key).and_then(Value::as_bool).unwrap_or(default);
        Self {
            layout: obj
                .get("layout")
                .and_then(Value::as_str)
                .and_then(ZoomLayout::from_str)
                .unwrap_or(defaults.layout),
            main_min_columns: obj
                .get("main_min_columns")
                .and_then(Value::as_u64)
                .filter(|cols| {
                    (u64::from(MIN_MAIN_MIN_COLUMNS)..=u64::from(MAX_MAIN_MIN_COLUMNS))
                        .contains(cols)
                })
                .map(|cols| cols as u32)
                .unwrap_or(defaults.main_min_columns),
            plan_rail: obj
                .get("plan_rail")
                .and_then(Value::as_str)
                .and_then(RailState::from_str)
                .unwrap_or(defaults.plan_rail),
            ctrl_wheel: flag("ctrl_wheel", defaults.ctrl_wheel),
            show_finished: flag("show_finished", defaults.show_finished),
        }
    }

    pub fn to_value(self) -> Value {
        json!({
            "layout": self.layout.as_str(),
            "main_min_columns": self.main_min_columns,
            "plan_rail": self.plan_rail.as_str(),
            "ctrl_wheel": self.ctrl_wheel,
            "show_finished": self.show_finished,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_object_gives_the_decided_defaults() {
        let config = ZoomViewConfig::from_value(None);
        assert_eq!(config.layout, ZoomLayout::Focus);
        assert_eq!(config.main_min_columns, 120);
        assert_eq!(config.plan_rail, RailState::Open);
        assert!(config.ctrl_wheel);
        assert!(config.show_finished);
    }

    #[test]
    fn round_trips_every_field() {
        let config = ZoomViewConfig {
            layout: ZoomLayout::Strip,
            main_min_columns: 140,
            plan_rail: RailState::Collapsed,
            ctrl_wheel: false,
            show_finished: false,
        };
        assert_eq!(ZoomViewConfig::from_value(Some(&config.to_value())), config);
    }

    #[test]
    fn bad_values_fall_back_per_key() {
        let value = json!({
            "layout": "spiral",
            "main_min_columns": 20,
            "plan_rail": "closed",
            "ctrl_wheel": "yes",
        });
        let config = ZoomViewConfig::from_value(Some(&value));
        assert_eq!(config.layout, ZoomLayout::Focus);
        assert_eq!(config.main_min_columns, DEFAULT_MAIN_MIN_COLUMNS);
        assert_eq!(config.plan_rail, RailState::Closed);
        assert!(config.ctrl_wheel);
    }

    #[test]
    fn dropdown_indexes_round_trip() {
        for layout in ZoomLayout::ALL {
            assert_eq!(
                ZoomLayout::from_dropdown_index(layout.dropdown_index()),
                layout
            );
        }
        for state in RailState::ALL {
            assert_eq!(
                RailState::from_dropdown_index(state.dropdown_index()),
                state
            );
        }
    }
}
