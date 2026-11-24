//! The `zoom_view` object in settings.json: how the subagent view shows
//! the subagents of a terminal. Pure data; the GTK side reads it.

use serde_json::{json, Value};

use super::dip_state::RunFilter;
use super::geometry::{MAX_MAIN_SHARE, MIN_MAIN_SHARE};

pub const DEFAULT_MAIN_MIN_COLUMNS: u32 = 120;
pub const MIN_MAIN_MIN_COLUMNS: u32 = 80;
pub const MAX_MAIN_MIN_COLUMNS: u32 = 300;
/// The open plan rail's width in pixels; the user drags its left edge.
pub const DEFAULT_RAIL_WIDTH: u32 = 240;
pub const MIN_RAIL_WIDTH: u32 = 180;
pub const MAX_RAIL_WIDTH: u32 = 640;

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
    /// The subagents never make the terminals narrower than this.
    pub main_min_columns: u32,
    pub plan_rail: RailState,
    /// The width of the open plan rail; dragging its edge saves it here.
    pub plan_rail_width: u32,
    /// Recently finished runs (the /dip pane's rule) stay as dimmed tiles.
    pub show_finished: bool,
    /// The terminals' dragged share of the width while the subagents
    /// show, in thousandths; 0 keeps the default.
    pub main_share_permille: u16,
    /// The "Hide done" filter of the subagent pane.
    pub hide_done: bool,
    /// The "Hide shells" filter of the subagent pane.
    pub hide_shells: bool,
}

impl ZoomViewConfig {
    /// The dragged share, or None for the default.
    pub fn main_share(&self) -> Option<f64> {
        let permille = self.main_share_permille;
        (permille > 0).then(|| f64::from(permille) / 1000.0)
    }

    /// The subagent filters that the user chose last.
    pub fn filter(&self) -> RunFilter {
        RunFilter {
            hide_done: self.hide_done,
            hide_shells: self.hide_shells,
        }
    }

    pub fn set_filter(&mut self, filter: RunFilter) {
        self.hide_done = filter.hide_done;
        self.hide_shells = filter.hide_shells;
    }

    /// Keep a dragged share; None goes back to the default.
    pub fn set_main_share(&mut self, share: Option<f64>) {
        self.main_share_permille = share.map(share_permille).unwrap_or(0);
    }
}

/// A share from settings.json. Older builds kept one share per zoom
/// level (`{"z1": 0.6, ...}`); the Z1 share was the one next to the
/// subagents, so it stays.
fn read_share(value: Option<&Value>) -> u16 {
    let value = match value {
        Some(Value::Object(levels)) => levels.get("z1"),
        other => other,
    };
    value
        .and_then(Value::as_f64)
        .filter(|share| share.is_finite() && *share > 0.0)
        .map(share_permille)
        .unwrap_or(0)
}

fn share_permille(share: f64) -> u16 {
    (share.clamp(MIN_MAIN_SHARE, MAX_MAIN_SHARE) * 1000.0).round() as u16
}

impl Default for ZoomViewConfig {
    fn default() -> Self {
        Self {
            layout: ZoomLayout::default(),
            main_min_columns: DEFAULT_MAIN_MIN_COLUMNS,
            plan_rail: RailState::default(),
            plan_rail_width: DEFAULT_RAIL_WIDTH,
            show_finished: true,
            main_share_permille: 0,
            hide_done: false,
            hide_shells: false,
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
            // An out-of-range width clamps: a wide screen saved a wide rail.
            plan_rail_width: obj
                .get("plan_rail_width")
                .and_then(Value::as_u64)
                .map(|w| w.clamp(u64::from(MIN_RAIL_WIDTH), u64::from(MAX_RAIL_WIDTH)) as u32)
                .unwrap_or(defaults.plan_rail_width),
            show_finished: flag("show_finished", defaults.show_finished),
            main_share_permille: read_share(obj.get("main_share")),
            hide_done: flag("hide_done", defaults.hide_done),
            hide_shells: flag("hide_shells", defaults.hide_shells),
        }
    }

    pub fn to_value(self) -> Value {
        let mut value = json!({
            "layout": self.layout.as_str(),
            "main_min_columns": self.main_min_columns,
            "plan_rail": self.plan_rail.as_str(),
            "plan_rail_width": self.plan_rail_width,
            "show_finished": self.show_finished,
            "hide_done": self.hide_done,
            "hide_shells": self.hide_shells,
        });
        if let Some(share) = self.main_share() {
            value["main_share"] = json!(share);
        }
        value
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
        assert_eq!(config.plan_rail_width, 240);
        assert!(config.show_finished);
        assert_eq!(config.main_share(), None);
        assert_eq!(config.filter(), RunFilter::default());
    }

    #[test]
    fn the_filters_are_remembered() {
        let mut config = ZoomViewConfig::default();
        config.set_filter(RunFilter {
            hide_done: true,
            hide_shells: false,
        });
        let value = config.to_value();
        assert_eq!(value["hide_done"], json!(true));
        assert_eq!(value["hide_shells"], json!(false));
        assert_eq!(
            ZoomViewConfig::from_value(Some(&value)).filter(),
            config.filter()
        );
    }

    #[test]
    fn rail_width_clamps_and_ignores_a_bad_type() {
        let width = |value: Value| {
            ZoomViewConfig::from_value(Some(&json!({ "plan_rail_width": value }))).plan_rail_width
        };
        assert_eq!(width(json!(320)), 320);
        assert_eq!(width(json!(20)), MIN_RAIL_WIDTH);
        assert_eq!(width(json!(5000)), MAX_RAIL_WIDTH);
        assert_eq!(width(json!("wide")), DEFAULT_RAIL_WIDTH);
    }

    #[test]
    fn round_trips_every_field() {
        let config = ZoomViewConfig {
            layout: ZoomLayout::Strip,
            main_min_columns: 140,
            plan_rail: RailState::Collapsed,
            plan_rail_width: 360,
            show_finished: false,
            main_share_permille: 620,
            hide_done: true,
            hide_shells: true,
        };
        assert_eq!(ZoomViewConfig::from_value(Some(&config.to_value())), config);
    }

    #[test]
    fn bad_values_fall_back_per_key() {
        let value = json!({
            "layout": "spiral",
            "main_min_columns": 20,
            "plan_rail": "closed",
            "show_finished": "yes",
        });
        let config = ZoomViewConfig::from_value(Some(&value));
        assert_eq!(config.layout, ZoomLayout::Focus);
        assert_eq!(config.main_min_columns, DEFAULT_MAIN_MIN_COLUMNS);
        assert_eq!(config.plan_rail, RailState::Closed);
        assert!(config.show_finished);
    }

    #[test]
    fn the_main_share_is_clamped_optional_and_reads_the_old_z1_share() {
        let share = |value: Value| {
            ZoomViewConfig::from_value(Some(&json!({ "main_share": value }))).main_share()
        };
        assert_eq!(share(json!(0.62)), Some(0.62));
        assert_eq!(share(json!(2.0)), Some(MAX_MAIN_SHARE));
        assert_eq!(share(json!("wide")), None);
        assert_eq!(share(json!({ "z1": 0.6, "z2": 0.4 })), Some(0.6));
        assert_eq!(share(json!({ "z2": 0.4 })), None);
        let mut config = ZoomViewConfig::from_value(Some(&json!({ "main_share": 0.5 })));
        config.set_main_share(None);
        assert_eq!(config.main_share(), None);
        assert!(config.to_value().get("main_share").is_none());
        config.set_main_share(Some(0.05));
        assert_eq!(config.to_value()["main_share"], json!(MIN_MAIN_SHARE));
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
