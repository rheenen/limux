//! The plan rail: the /dip plan as a native limux column. It shows the
//! plan tree, a hide-done filter and a plan picker. A click on a node with
//! a run, or on a run, selects that run's tile. The rail can collapse to
//! a narrow strip and can close; the pane header's "Show plan" button
//! opens it again.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk4 as gtk;

use super::config::{RailState, MAX_RAIL_WIDTH, MIN_RAIL_WIDTH};
use super::dip_state::{DipState, NodeStatus, PlanNode, Run, RunStatus};
use super::view::status_css;

pub const COLLAPSED_WIDTH: i32 = 44;
/// The drag handle on the rail's left edge; part of the open width.
const HANDLE_WIDTH: i32 = 6;

type RunCallback = dyn Fn(&str);
type ModeCallback = dyn Fn(RailState);
/// The new open width, and whether the drag ended (then it is saved).
type WidthCallback = dyn Fn(i32, bool);

/// The open width after a drag of `dx` pixels from `start`. The rail sits
/// at the right, so a drag to the left (negative dx) widens it.
pub fn dragged_width(start: i32, dx: f64) -> i32 {
    clamp_width(start - dx.round() as i32)
}

pub fn clamp_width(width: i32) -> i32 {
    width.clamp(MIN_RAIL_WIDTH as i32, MAX_RAIL_WIDTH as i32)
}

/// The run counts that the collapsed strip shows, one per status that
/// has runs, in a fixed order: running first.
pub fn strip_stats(runs: &[&Run]) -> Vec<(RunStatus, usize)> {
    const ORDER: [RunStatus; 7] = [
        RunStatus::Running,
        RunStatus::Failed,
        RunStatus::Limit,
        RunStatus::Stopped,
        RunStatus::TakenOver,
        RunStatus::Done,
        RunStatus::Unknown,
    ];
    ORDER
        .into_iter()
        .map(|status| (status, runs.iter().filter(|r| r.status == status).count()))
        .filter(|(_, count)| *count > 0)
        .collect()
}

fn status_word(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Running => "running",
        RunStatus::Done => "done",
        RunStatus::Failed => "failed",
        RunStatus::Limit => "at their limit",
        RunStatus::Stopped => "stopped",
        RunStatus::TakenOver => "taken over",
        RunStatus::Unknown => "in an unknown state",
    }
}

pub struct PlanRail {
    pub root: gtk::Box,
    handle: gtk::Box,
    open_panel: gtk::Box,
    open_width: Cell<i32>,
    strip: gtk::Box,
    title: gtk::Label,
    picker: gtk::DropDown,
    picker_model: gtk::StringList,
    hide_done: gtk::CheckButton,
    list: gtk::ListBox,
    strip_stats: gtk::Box,
    mode: Cell<RailState>,
    has_content: Cell<bool>,
    state: RefCell<Option<DipState>>,
    plan_ids: RefCell<Vec<String>>,
    row_runs: RefCell<Vec<Option<String>>>,
    filling_picker: Cell<bool>,
    on_select_run: RefCell<Option<Box<RunCallback>>>,
    on_pick_plan: RefCell<Option<Box<RunCallback>>>,
    on_mode_changed: RefCell<Option<Box<ModeCallback>>>,
    on_width_changed: RefCell<Option<Box<WidthCallback>>>,
}

fn node_glyph(status: NodeStatus) -> &'static str {
    match status {
        NodeStatus::Todo => "○",
        NodeStatus::Active => "◐",
        NodeStatus::Done => "✓",
        NodeStatus::Failed => "✗",
        NodeStatus::Blocked => "⊘",
        NodeStatus::Unknown => "·",
    }
}

fn flat_button(icon: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .has_frame(false)
        .valign(gtk::Align::Center)
        .build();
    button.add_css_class("limux-pane-action");
    button
}

/// The deepest active node, for the collapsed strip.
fn active_label(nodes: &[PlanNode]) -> Option<String> {
    nodes.iter().find_map(|node| {
        active_label(&node.children)
            .or_else(|| (node.status == NodeStatus::Active).then(|| node.label.clone()))
    })
}

impl PlanRail {
    pub fn new(initial: RailState, open_width: u32) -> Rc<Self> {
        let open_width = clamp_width(open_width as i32);
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("limux-plan-rail");
        root.set_vexpand(true);
        // The title label expands inside the header; the rail itself keeps
        // its width (the handle changes it) and never takes space from the
        // terminals.
        root.set_hexpand(false);

        // Drag handle: the open rail's left edge.
        let handle = gtk::Box::new(gtk::Orientation::Vertical, 0);
        handle.add_css_class("limux-plan-rail-handle");
        handle.set_size_request(HANDLE_WIDTH, -1);
        handle.set_vexpand(true);
        handle.set_cursor_from_name(Some("col-resize"));
        handle.set_tooltip_text(Some("Drag to resize the plan"));

        // Open panel: header, picker, filter, list.
        let open_panel = gtk::Box::new(gtk::Orientation::Vertical, 4);
        open_panel.set_size_request(open_width - HANDLE_WIDTH, -1);
        open_panel.set_vexpand(true);
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        header.set_margin_start(8);
        header.set_margin_end(2);
        header.set_margin_top(4);
        let title = gtk::Label::new(Some("Plan"));
        title.set_hexpand(true);
        title.set_xalign(0.0);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.add_css_class("heading");
        let collapse = flat_button("go-next-symbolic", "Collapse the plan");
        let close = flat_button("window-close-symbolic", "Close the plan");
        header.append(&title);
        header.append(&collapse);
        header.append(&close);

        let picker_model = gtk::StringList::new(&[]);
        let picker = gtk::DropDown::new(Some(picker_model.clone()), gtk::Expression::NONE);
        picker.set_margin_start(8);
        picker.set_margin_end(8);
        picker.set_tooltip_text(Some("Show another plan"));
        picker.set_visible(false);

        let hide_done = gtk::CheckButton::with_label("Hide done");
        hide_done.set_margin_start(8);

        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.set_activate_on_single_click(true);
        list.add_css_class("navigation-sidebar");
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();

        open_panel.append(&header);
        open_panel.append(&picker);
        open_panel.append(&hide_done);
        open_panel.append(&scroller);

        // Collapsed strip: expand, close, then one count per run status
        // (glyph above number). The active task is in the tooltip.
        let strip = gtk::Box::new(gtk::Orientation::Vertical, 6);
        strip.add_css_class("limux-plan-strip");
        strip.set_size_request(COLLAPSED_WIDTH, -1);
        strip.set_vexpand(true);
        let expand = flat_button("go-previous-symbolic", "Expand the plan");
        let strip_close = flat_button("window-close-symbolic", "Close the plan");
        expand.set_halign(gtk::Align::Center);
        strip_close.set_halign(gtk::Align::Center);
        let strip_separator = gtk::Separator::new(gtk::Orientation::Horizontal);
        strip_separator.set_margin_start(8);
        strip_separator.set_margin_end(8);
        let strip_stats = gtk::Box::new(gtk::Orientation::Vertical, 10);
        strip_stats.set_margin_top(4);
        strip_stats.set_halign(gtk::Align::Center);
        strip.append(&expand);
        strip.append(&strip_close);
        strip.append(&strip_separator);
        strip.append(&strip_stats);
        strip.set_cursor_from_name(Some("pointer"));

        root.append(&handle);
        root.append(&open_panel);
        root.append(&strip);

        let rail = Rc::new(Self {
            root,
            handle,
            open_panel,
            open_width: Cell::new(open_width),
            strip,
            title,
            picker,
            picker_model,
            hide_done,
            list,
            strip_stats,
            mode: Cell::new(initial),
            has_content: Cell::new(false),
            state: RefCell::new(None),
            plan_ids: RefCell::default(),
            row_runs: RefCell::default(),
            filling_picker: Cell::new(false),
            on_select_run: RefCell::new(None),
            on_pick_plan: RefCell::new(None),
            on_mode_changed: RefCell::new(None),
            on_width_changed: RefCell::new(None),
        });

        let set_mode = |button: &gtk::Button, mode: RailState| {
            let weak = Rc::downgrade(&rail);
            button.connect_clicked(move |_| {
                if let Some(rail) = weak.upgrade() {
                    rail.set_mode(mode);
                }
            });
        };
        set_mode(&collapse, RailState::Collapsed);
        set_mode(&close, RailState::Closed);
        set_mode(&expand, RailState::Open);
        set_mode(&strip_close, RailState::Closed);
        {
            // A click anywhere on the strip expands the rail; the close
            // button claims its own click first.
            let click = gtk::GestureClick::new();
            let weak = Rc::downgrade(&rail);
            click.connect_released(move |_, _, _, _| {
                if let Some(rail) = weak.upgrade() {
                    if rail.mode() == RailState::Collapsed {
                        rail.set_mode(RailState::Open);
                    }
                }
            });
            rail.strip.add_controller(click);
        }

        {
            let weak = Rc::downgrade(&rail);
            rail.hide_done.connect_toggled(move |_| {
                if let Some(rail) = weak.upgrade() {
                    rail.rebuild_list();
                }
            });
        }
        {
            let weak = Rc::downgrade(&rail);
            rail.list.connect_row_activated(move |_, row| {
                let Some(rail) = weak.upgrade() else { return };
                let run = usize::try_from(row.index())
                    .ok()
                    .and_then(|i| rail.row_runs.borrow().get(i).cloned().flatten());
                let callback = rail.on_select_run.borrow();
                if let (Some(run), Some(f)) = (run, callback.as_ref()) {
                    f(&run);
                }
            });
        }
        {
            let weak = Rc::downgrade(&rail);
            rail.picker.connect_selected_notify(move |picker| {
                let Some(rail) = weak.upgrade() else { return };
                if rail.filling_picker.get() {
                    return;
                }
                let id = rail
                    .plan_ids
                    .borrow()
                    .get(picker.selected() as usize)
                    .cloned();
                let callback = rail.on_pick_plan.borrow();
                if let (Some(id), Some(f)) = (id, callback.as_ref()) {
                    f(&id);
                }
            });
        }
        {
            let drag = gtk::GestureDrag::new();
            let start = Rc::new(Cell::new(open_width));
            let weak = Rc::downgrade(&rail);
            let begin = start.clone();
            drag.connect_drag_begin(move |_, _, _| {
                if let Some(rail) = weak.upgrade() {
                    begin.set(rail.open_width.get());
                }
            });
            let weak = Rc::downgrade(&rail);
            let update = start.clone();
            drag.connect_drag_update(move |_, dx, _| {
                if let Some(rail) = weak.upgrade() {
                    rail.resize(dragged_width(update.get(), dx), false);
                }
            });
            let weak = Rc::downgrade(&rail);
            drag.connect_drag_end(move |_, dx, _| {
                if let Some(rail) = weak.upgrade() {
                    rail.resize(dragged_width(start.get(), dx), true);
                }
            });
            rail.handle.add_controller(drag);
        }
        rail.sync_visibility();
        rail
    }

    /// Called while the handle moves (`done` false) and once when the drag
    /// ends (`done` true).
    pub fn connect_width_changed(&self, f: impl Fn(i32, bool) + 'static) {
        *self.on_width_changed.borrow_mut() = Some(Box::new(f));
    }

    /// The width of the open rail, the handle included.
    pub fn open_width(&self) -> i32 {
        self.open_width.get()
    }

    /// Set the open width (clamped); for example from another workspace.
    pub fn set_open_width(&self, width: i32) {
        let width = clamp_width(width);
        self.open_width.set(width);
        self.open_panel.set_size_request(width - HANDLE_WIDTH, -1);
    }

    fn resize(&self, width: i32, done: bool) {
        let changed = clamp_width(width) != self.open_width.get();
        self.set_open_width(width);
        if changed || done {
            if let Some(f) = self.on_width_changed.borrow().as_ref() {
                f(self.open_width.get(), done);
            }
        }
    }

    pub fn connect_select_run(&self, f: impl Fn(&str) + 'static) {
        *self.on_select_run.borrow_mut() = Some(Box::new(f));
    }

    pub fn connect_pick_plan(&self, f: impl Fn(&str) + 'static) {
        *self.on_pick_plan.borrow_mut() = Some(Box::new(f));
    }

    pub fn connect_mode_changed(&self, f: impl Fn(RailState) + 'static) {
        *self.on_mode_changed.borrow_mut() = Some(Box::new(f));
    }

    pub fn mode(&self) -> RailState {
        self.mode.get()
    }

    pub fn set_mode(&self, mode: RailState) {
        if self.mode.replace(mode) == mode {
            return;
        }
        self.sync_visibility();
        if let Some(f) = self.on_mode_changed.borrow().as_ref() {
            f(mode);
        }
    }

    /// True when the rail is closed but has a plan to show: the pane
    /// headers then show their "Show plan" button.
    pub fn wants_show_button(&self) -> bool {
        self.mode.get() == RailState::Closed && self.has_content.get()
    }

    /// The width the rail takes now, for the zoom geometry.
    pub fn width(&self) -> f64 {
        if !self.root.is_visible() {
            return 0.0;
        }
        match self.mode.get() {
            RailState::Open => f64::from(self.open_width.get()),
            RailState::Collapsed => f64::from(COLLAPSED_WIDTH),
            RailState::Closed => 0.0,
        }
    }

    fn sync_visibility(&self) {
        let mode = self.mode.get();
        self.root
            .set_visible(self.has_content.get() && mode != RailState::Closed);
        self.open_panel.set_visible(mode == RailState::Open);
        self.handle.set_visible(mode == RailState::Open);
        self.strip.set_visible(mode == RailState::Collapsed);
    }

    /// Show a new state, or None when the session has no state file. With
    /// no plan and no runs the rail hides itself (and comes back later).
    pub fn update(&self, state: Option<&DipState>) {
        let changed = self.state.borrow().as_ref() != state;
        if !changed {
            return;
        }
        *self.state.borrow_mut() = state.cloned();
        self.has_content
            .set(state.is_some_and(DipState::has_content));
        self.fill_picker();
        self.rebuild_list();
        self.sync_visibility();
    }

    fn fill_picker(&self) {
        let state = self.state.borrow();
        let plans = state.as_ref().map(|s| s.plans.as_slice()).unwrap_or(&[]);
        let ids: Vec<String> = plans.iter().map(|p| p.id.clone()).collect();
        if *self.plan_ids.borrow() != ids {
            self.filling_picker.set(true);
            let titles: Vec<&str> = plans.iter().map(|p| p.title.as_str()).collect();
            self.picker_model
                .splice(0, self.picker_model.n_items(), &titles);
            *self.plan_ids.borrow_mut() = ids;
            self.filling_picker.set(false);
        }
        if let Some(active) = plans.iter().position(|p| p.active) {
            self.filling_picker.set(true);
            self.picker.set_selected(active as u32);
            self.filling_picker.set(false);
        }
        self.picker.set_visible(plans.len() > 1);
    }

    fn rebuild_list(&self) {
        while let Some(row) = self.list.row_at_index(0) {
            self.list.remove(&row);
        }
        let mut row_runs = Vec::new();
        let state = self.state.borrow();
        let Some(state) = state.as_ref() else {
            *self.row_runs.borrow_mut() = row_runs;
            return;
        };
        let hide_done = self.hide_done.is_active();

        let title = state
            .plan
            .as_ref()
            .map(|p| p.title.clone())
            .unwrap_or_else(|| "Subagents".to_string());
        self.title.set_label(&title);
        self.title.set_tooltip_text(Some(&title));

        let mut add_row = |text: String, depth: usize, run: Option<String>, dim: bool| {
            let label = gtk::Label::new(Some(&text));
            label.set_xalign(0.0);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_margin_start(6 + 12 * depth as i32);
            if dim {
                label.add_css_class("dim-label");
            }
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&label));
            row.set_activatable(run.is_some());
            row.set_tooltip_text(Some(&text));
            self.list.append(&row);
            row_runs.push(run);
        };

        fn walk(
            nodes: &[PlanNode],
            depth: usize,
            hide_done: bool,
            state: &DipState,
            add: &mut dyn FnMut(String, usize, Option<String>, bool),
        ) {
            for node in nodes {
                if hide_done && !node.has_open_work() {
                    continue;
                }
                let run_glyph = node
                    .run
                    .as_deref()
                    .and_then(|id| state.run(id))
                    .map(|run| format!("  {}", run.status.glyph()))
                    .unwrap_or_default();
                add(
                    format!("{} {}{}", node_glyph(node.status), node.label, run_glyph),
                    depth,
                    node.run.clone(),
                    node.status == NodeStatus::Done,
                );
                walk(&node.children, depth + 1, hide_done, state, add);
            }
        }
        if let Some(plan) = state.plan.as_ref() {
            walk(&plan.nodes, 0, hide_done, state, &mut add_row);
        }

        // The same runs as the /dip pane: running, and finished for a minute.
        let recent = state.recent_runs();
        let runs: Vec<_> = recent
            .iter()
            .filter(|run| !hide_done || run.status.is_running())
            .collect();
        if !runs.is_empty() {
            add_row("Subagents".to_string(), 0, None, true);
            for run in runs {
                add_row(
                    format!("{} {}", run.status.glyph(), run.name),
                    1 + state.depth(run),
                    Some(run.id.clone()),
                    !run.status.is_running(),
                );
            }
        }
        *self.row_runs.borrow_mut() = row_runs;

        self.fill_strip(state, &recent);
    }

    fn fill_strip(&self, state: &DipState, recent: &[&Run]) {
        while let Some(child) = self.strip_stats.first_child() {
            self.strip_stats.remove(&child);
        }
        let stats = strip_stats(recent);
        for (status, count) in &stats {
            let stat = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let glyph = gtk::Label::new(Some(status.glyph()));
            glyph.add_css_class(status_css(*status));
            let number = gtk::Label::new(Some(&count.to_string()));
            number.add_css_class("limux-plan-strip-count");
            stat.append(&glyph);
            stat.append(&number);
            stat.set_tooltip_text(Some(&format!("{count} {}", status_word(*status))));
            self.strip_stats.append(&stat);
        }
        if stats.is_empty() {
            let none = gtk::Label::new(Some("0"));
            none.add_css_class("dim-label");
            none.set_tooltip_text(Some("No running or recent subagents"));
            self.strip_stats.append(&none);
        }
        let task = state
            .plan
            .as_ref()
            .and_then(|plan| active_label(&plan.nodes));
        let title = state.plan.as_ref().map(|p| p.title.as_str());
        let tip = match (title, task) {
            (Some(title), Some(task)) => format!("{title}\nNow: {task}"),
            (Some(title), None) => title.to_string(),
            (None, Some(task)) => format!("Now: {task}"),
            (None, None) => "Subagents".to_string(),
        };
        self.strip.set_tooltip_text(Some(&tip));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(label: &str, status: NodeStatus, children: Vec<PlanNode>) -> PlanNode {
        PlanNode {
            id: label.to_string(),
            label: label.to_string(),
            kind: None,
            status,
            run: None,
            children,
        }
    }

    #[test]
    fn active_label_prefers_the_deepest_active_node() {
        let nodes = vec![
            node("p1", NodeStatus::Done, vec![]),
            node(
                "p2",
                NodeStatus::Active,
                vec![
                    node("p2a", NodeStatus::Done, vec![]),
                    node("p2b", NodeStatus::Active, vec![]),
                ],
            ),
        ];
        assert_eq!(active_label(&nodes).as_deref(), Some("p2b"));
        assert_eq!(active_label(&nodes[..1]), None);
    }

    fn run(id: &str, status: &str) -> Run {
        serde_json::from_value(serde_json::json!({
            "id": id, "name": id, "status": status, "follow": []
        }))
        .unwrap()
    }

    #[test]
    fn strip_counts_each_status_with_runs_running_first() {
        let runs = [
            run("a", "done"),
            run("b", "running"),
            run("c", "done"),
            run("d", "failed"),
        ];
        assert_eq!(
            strip_stats(&runs.iter().collect::<Vec<_>>()),
            vec![
                (RunStatus::Running, 1),
                (RunStatus::Failed, 1),
                (RunStatus::Done, 2)
            ]
        );
        assert!(strip_stats(&[]).is_empty());
    }

    #[test]
    fn a_drag_to_the_left_widens_the_rail_within_its_limits() {
        assert_eq!(dragged_width(240, -60.0), 300);
        assert_eq!(dragged_width(240, 20.4), 220);
        assert_eq!(dragged_width(240, 500.0), MIN_RAIL_WIDTH as i32);
        assert_eq!(dragged_width(240, -2000.0), MAX_RAIL_WIDTH as i32);
    }
}
