//! The zoom view of one workspace: Ctrl+wheel down on a terminal folds
//! the other splits away and shows the session's subagents as live tiles
//! beside it; the plan rail is on every level. Ctrl+wheel up steps back
//! to Z0 and focuses the main terminal; a double-click on a tile opens
//! that run as a new tab in the same pane, at the same zoom level.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;

use super::config::{RailState, ZoomLayout, ZoomViewConfig};
use super::dip_state::{self, DipState, Run, RunFilter, RunStatus};
use super::geometry::{self, Frame, ZoomLevel};
use super::rail::PlanRail;
use crate::app_config::AppConfig;
use crate::pane;
use crate::split_tree::SplitTreeContainer;
use crate::terminal::{self, TerminalCallbacks, TerminalOptions, TerminalWidget};

const POLL_INTERVAL: Duration = Duration::from_millis(500);
/// The drag handle between the main terminal and the rail or tiles.
const MAIN_HANDLE_WIDTH: i32 = 6;
const TILE_NAME_PREFIX: &str = "limux-zoom-tile:";
const TILE_TITLE_HEIGHT: i32 = 24;
/// Used until the main terminal reports its size.
const FALLBACK_CELL: (f64, f64) = (8.0, 17.0);

struct Tile {
    run_id: String,
    frame: gtk::Box,
    title: CardTitle,
    term: TerminalWidget,
    follow: Vec<String>,
}

/// The title bar of a terminal card: status icon, name, model, then the
/// elapsed time (or the size) at the right. It has no buttons.
struct CardTitle {
    root: gtk::Box,
    glyph: gtk::Label,
    name: gtk::Label,
    detail: gtk::Label,
    end: gtk::Label,
}

const STATUS_CSS: [&str; 7] = [
    "limux-zoom-st-running",
    "limux-zoom-st-done",
    "limux-zoom-st-failed",
    "limux-zoom-st-limit",
    "limux-zoom-st-stopped",
    "limux-zoom-st-taken-over",
    "limux-zoom-st-unknown",
];

pub(super) fn status_css(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Running => STATUS_CSS[0],
        RunStatus::Done => STATUS_CSS[1],
        RunStatus::Failed => STATUS_CSS[2],
        RunStatus::Limit => STATUS_CSS[3],
        RunStatus::Stopped => STATUS_CSS[4],
        RunStatus::TakenOver => STATUS_CSS[5],
        RunStatus::Unknown => STATUS_CSS[6],
    }
}

impl CardTitle {
    fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        root.add_css_class("limux-zoom-card-title");
        root.set_size_request(-1, TILE_TITLE_HEIGHT);
        let label = |css: &str| {
            let label = gtk::Label::new(None);
            label.add_css_class(css);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label
        };
        let glyph = label("limux-zoom-card-glyph");
        glyph.set_ellipsize(gtk::pango::EllipsizeMode::None);
        let name = label("limux-zoom-card-name");
        let detail = label("limux-zoom-card-detail");
        detail.add_css_class("dim-label");
        let end = label("limux-zoom-card-end");
        end.add_css_class("dim-label");
        end.set_ellipsize(gtk::pango::EllipsizeMode::None);
        end.set_hexpand(true);
        end.set_xalign(1.0);
        root.append(&glyph);
        root.append(&name);
        root.append(&detail);
        root.append(&end);
        Self {
            root,
            glyph,
            name,
            detail,
            end,
        }
    }

    fn set(&self, status: RunStatus, name: &str, detail: Option<&str>, end: Option<&str>) {
        self.glyph.set_label(status.glyph());
        for css in STATUS_CSS {
            self.glyph.remove_css_class(css);
        }
        self.glyph.add_css_class(status_css(status));
        self.name.set_label(name);
        self.detail.set_label(detail.unwrap_or_default());
        self.detail.set_visible(detail.is_some());
        self.end.set_label(end.unwrap_or_default());
    }
}

/// The session whose subagents the view shows: a terminal surface and
/// the pane it lives in.
#[derive(Clone)]
struct Session {
    surface_id: String,
    pane: gtk::Widget,
}

struct LoadedState {
    path: PathBuf,
    modified: Option<SystemTime>,
    state: Option<DipState>,
}

pub struct ZoomView {
    container: Weak<SplitTreeContainer>,
    config: Rc<RefCell<AppConfig>>,
    frame: gtk::Grid,
    main_slot: gtk::Box,
    main_title: CardTitle,
    show_plan: gtk::Button,
    rail_holder: gtk::Box,
    /// The drag handle on the main card's right edge (focus and grid
    /// layouts): it sets the main terminal's share of the width.
    main_handle: gtk::Box,
    tiles_box: gtk::Box,
    tiles_header: gtk::Label,
    scroller: gtk::ScrolledWindow,
    grid: gtk::Grid,
    empty: gtk::Label,
    indicator: gtk::Label,
    rail: Rc<PlanRail>,
    level: Cell<ZoomLevel>,
    /// The session shown while zoomed (fixed) or at Z0 (follows focus).
    session: RefCell<Option<Session>>,
    loaded: RefCell<Option<LoadedState>>,
    tiles: RefCell<Vec<Tile>>,
    selected: RefCell<Option<String>>,
    last_size: Cell<(i32, i32)>,
    tile_height: Cell<f64>,
    grid_columns: Cell<u32>,
    timer: RefCell<Option<glib::SourceId>>,
    /// Called when `status()` changes (the top bar follows it).
    on_change: RefCell<Option<Rc<dyn Fn()>>>,
    last_status: Cell<Option<ZoomStatus>>,
    /// The saved rail width this view last applied; another workspace's
    /// drag changes the setting, and the poll then follows it.
    applied_rail_width: Cell<u32>,
    /// Promoted tab surface id -> the surface id of its parent session.
    promoted: RefCell<HashMap<String, String>>,
    /// The tile font size and the font generation it was applied for.
    applied_tile_font: Cell<(f32, u64)>,
    filter: Cell<RunFilter>,
    /// The share while the main handle is dragged; saved when it ends.
    drag_share: Cell<Option<f64>>,
}

/// What the top-bar zoom buttons show for a workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZoomStatus {
    pub level: ZoomLevel,
    /// True when a zoom-out has something to show (or the view is zoomed).
    pub can_zoom: bool,
    pub filter: RunFilter,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn place(
    grid: &gtk::Grid,
    child: &impl IsA<gtk::Widget>,
    column: i32,
    row: i32,
    width: i32,
    height: i32,
) {
    let Some(layout_child) = grid
        .layout_manager()
        .map(|manager| manager.layout_child(child))
        .and_then(|child| child.downcast::<gtk::GridLayoutChild>().ok())
    else {
        return;
    };
    layout_child.set_column(column);
    layout_child.set_row(row);
    layout_child.set_column_span(width);
    layout_child.set_row_span(height);
}

fn install_css() {
    thread_local! {
        static INSTALLED: Cell<bool> = const { Cell::new(false) };
    }
    if INSTALLED.with(|done| done.replace(true)) {
        return;
    }
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        ".limux-plan-rail { border-left: 1px solid alpha(currentColor, 0.12); }\n\
         .limux-plan-rail-handle { background: transparent; }\n\
         .limux-plan-rail-handle:hover { background: alpha(currentColor, 0.15); }\n\
         .limux-zoom-main-handle { background: transparent; }\n\
         .limux-zoom-main-handle:hover { background: alpha(currentColor, 0.15); }\n\
         .limux-plan-strip-count { font-weight: bold; font-size: 0.9em; }\n\
         .limux-zoom-tile, .limux-zoom-card { border: 1px solid alpha(currentColor, 0.15); border-radius: 10px; }\n\
         .limux-zoom-card { border-color: alpha(#6aa6ff, 0.55); margin: 4px 0 4px 4px; }\n\
         .limux-zoom-tile-done { opacity: 0.6; }\n\
         .limux-zoom-tile-selected { border: 2px solid #f0b45e; }\n\
         .limux-zoom-card-title { padding: 0 10px; font-size: 0.9em; background: alpha(currentColor, 0.05); border-bottom: 1px solid alpha(currentColor, 0.1); }\n\
         .limux-zoom-card-glyph { font-weight: bold; }\n\
         .limux-zoom-card-name { font-weight: 600; }\n\
         .limux-zoom-card-end { font-family: monospace; }\n\
         .limux-zoom-card-plan { padding: 0 8px; min-height: 20px; font-size: 0.9em; }\n\
         .limux-plan-rail-summary { font-size: 0.85em; }\n\
         .limux-zoom-st-running { color: #e5c07b; }\n\
         .limux-zoom-st-done { color: #6fcf8f; }\n\
         .limux-zoom-st-failed { color: #ff8a8a; }\n\
         .limux-zoom-st-limit { color: #d68ae6; }\n\
         .limux-zoom-st-stopped { color: alpha(currentColor, 0.55); }\n\
         .limux-zoom-st-taken-over { color: #56c8d8; }\n\
         .limux-zoom-indicator { padding: 4px; font-size: 0.9em; }\n",
    );
    gtk::style_context_add_provider_for_display(&display, &provider, super::CSS_PRIORITY);
}

impl ZoomView {
    pub fn new(container: &Rc<SplitTreeContainer>, config: Rc<RefCell<AppConfig>>) -> Rc<Self> {
        install_css();
        let (initial_rail, rail_width) = {
            let config = config.borrow();
            (config.zoom_view.plan_rail, config.zoom_view.plan_rail_width)
        };
        let rail = PlanRail::new(initial_rail, rail_width);

        let frame = gtk::Grid::new();
        frame.set_hexpand(true);
        frame.set_vexpand(true);
        frame.set_column_spacing(0);
        frame.set_row_spacing(0);
        // The main terminal card: its title bar, then the pane (the split
        // tree appends it while zoomed).
        let main_slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
        main_slot.set_vexpand(true);
        main_slot.add_css_class("limux-zoom-card");
        main_slot.set_overflow(gtk::Overflow::Hidden);
        let main_title = CardTitle::new();
        let show_plan = gtk::Button::with_label("Show plan");
        show_plan.add_css_class("limux-zoom-card-plan");
        show_plan.set_action_name(Some("win.zoom-show-plan"));
        show_plan.set_focus_on_click(false);
        show_plan.set_valign(gtk::Align::Center);
        show_plan.set_visible(false);
        main_title.root.append(&show_plan);
        main_slot.append(&main_title.root);
        let rail_holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
        rail_holder.set_vexpand(true);
        rail_holder.set_hexpand(false);

        let tiles_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        tiles_box.set_hexpand(true);
        tiles_box.set_vexpand(true);
        tiles_box.set_margin_start(6);
        tiles_box.set_margin_end(6);
        tiles_box.set_margin_top(4);
        let tiles_header = gtk::Label::new(None);
        tiles_header.set_xalign(0.0);
        tiles_header.add_css_class("dim-label");
        let grid = gtk::Grid::new();
        grid.set_column_spacing(geometry::TILE_GAP as u32);
        grid.set_row_spacing(geometry::TILE_GAP as u32);
        grid.set_column_homogeneous(true);
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .hexpand(true)
            .child(&grid)
            .build();
        let empty = gtk::Label::new(Some("No subagents in this session"));
        empty.add_css_class("dim-label");
        empty.set_vexpand(true);
        let indicator = gtk::Label::new(None);
        indicator.add_css_class("limux-zoom-indicator");
        indicator.add_css_class("dim-label");
        indicator.set_visible(false);
        tiles_box.append(&tiles_header);
        tiles_box.append(&scroller);
        tiles_box.append(&empty);
        tiles_box.append(&indicator);

        let main_handle = gtk::Box::new(gtk::Orientation::Vertical, 0);
        main_handle.add_css_class("limux-zoom-main-handle");
        main_handle.set_size_request(MAIN_HANDLE_WIDTH, -1);
        main_handle.set_vexpand(true);
        main_handle.set_hexpand(false);
        main_handle.set_cursor_from_name(Some("col-resize"));
        main_handle.set_tooltip_text(Some(
            "Drag to change the width of the terminal and the subagents.\nDouble-click for the default width of this zoom level.",
        ));

        frame.attach(&main_slot, 0, 0, 1, 1);
        frame.attach(&main_handle, 1, 0, 1, 1);
        frame.attach(&rail_holder, 2, 0, 1, 1);
        frame.attach(&tiles_box, 3, 0, 1, 1);

        let view = Rc::new(Self {
            container: Rc::downgrade(container),
            config,
            frame,
            main_slot,
            main_title,
            show_plan,
            rail_holder,
            main_handle,
            tiles_box,
            tiles_header,
            scroller,
            grid,
            empty,
            indicator,
            rail,
            level: Cell::new(ZoomLevel::Z0),
            session: RefCell::new(None),
            loaded: RefCell::new(None),
            tiles: RefCell::default(),
            selected: RefCell::new(None),
            last_size: Cell::new((0, 0)),
            tile_height: Cell::new(0.0),
            grid_columns: Cell::new(1),
            timer: RefCell::new(None),
            on_change: RefCell::new(None),
            last_status: Cell::new(None),
            applied_rail_width: Cell::new(rail_width),
            promoted: RefCell::default(),
            applied_tile_font: Cell::new((0.0, 0)),
            filter: Cell::new(RunFilter::default()),
            drag_share: Cell::new(None),
        });

        container.rail_slot().append(&view.rail.root);
        {
            let weak = Rc::downgrade(&view);
            container.set_on_frame_dropped(move || {
                if let Some(view) = weak.upgrade() {
                    view.frame_dropped();
                }
            });
        }
        {
            let weak = Rc::downgrade(&view);
            view.rail.connect_select_run(move |run| {
                if let Some(view) = weak.upgrade() {
                    view.select(Some(run.to_string()));
                }
            });
        }
        {
            let weak = Rc::downgrade(&view);
            view.rail.connect_zoom(move || {
                if let Some(view) = weak.upgrade() {
                    view.show_subagents();
                }
            });
            let weak = Rc::downgrade(&view);
            view.rail.connect_pick_plan(move |plan| {
                if let Some(view) = weak.upgrade() {
                    view.write_plan_choice(plan);
                }
            });
        }
        {
            let weak = Rc::downgrade(&view);
            view.rail.connect_width_changed(move |width, done| {
                let Some(view) = weak.upgrade() else { return };
                if view.level.get().is_zoomed() {
                    view.relayout();
                }
                if done {
                    view.save_rail_width(width);
                }
            });
        }
        {
            let weak = Rc::downgrade(&view);
            view.rail.connect_mode_changed(move |_| {
                if let Some(view) = weak.upgrade() {
                    view.sync_show_buttons();
                    view.relayout();
                }
            });
        }
        view.connect_main_handle();
        {
            let weak = Rc::downgrade(&view);
            view.scroller.vadjustment().connect_value_changed(move |_| {
                if let Some(view) = weak.upgrade() {
                    view.update_indicator();
                }
            });
        }

        let weak = Rc::downgrade(&view);
        let source = glib::timeout_add_local(POLL_INTERVAL, move || {
            let Some(view) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            view.poll();
            glib::ControlFlow::Continue
        });
        *view.timer.borrow_mut() = Some(source);
        view
    }

    fn zoom_config(&self) -> ZoomViewConfig {
        self.config.borrow().zoom_view
    }

    pub fn level(&self) -> ZoomLevel {
        self.level.get()
    }

    pub fn rail_mode(&self) -> RailState {
        self.rail.mode()
    }

    pub fn rail_visible(&self) -> bool {
        self.rail.root.is_visible()
    }

    pub fn status(&self) -> ZoomStatus {
        let level = self.level.get();
        ZoomStatus {
            level,
            can_zoom: level.is_zoomed() || !self.tile_runs().is_empty(),
            filter: self.filter.get(),
        }
    }

    /// The top-bar filters changed: tiles and rail follow them.
    pub fn set_filter(self: &Rc<Self>, filter: RunFilter) {
        if self.filter.replace(filter) == filter {
            return;
        }
        self.state_changed();
        if !self.level.get().is_zoomed() {
            self.sync_tiles();
        }
        self.notify_status();
    }

    /// `callback` runs when `status()` changes. It runs from inside the
    /// view, so it must not borrow state that the caller holds; defer.
    pub fn connect_status_changed(&self, callback: impl Fn() + 'static) {
        *self.on_change.borrow_mut() = Some(Rc::new(callback));
    }

    fn notify_status(&self) {
        let status = self.status();
        self.rail
            .set_zoom_button_visible(!status.level.is_zoomed() && status.can_zoom);
        if self.last_status.replace(Some(status)) == Some(status) {
            return;
        }
        let callback = self.on_change.borrow().clone();
        if let Some(callback) = callback {
            callback();
        }
    }

    pub fn show_rail(&self) {
        self.rail.set_mode(RailState::Open);
    }

    /// Stop the poll and end every tile's surface (workspace closes).
    pub fn shutdown(&self) {
        if let Some(source) = self.timer.borrow_mut().take() {
            source.remove();
        }
        for tile in self.tiles.borrow_mut().drain(..) {
            tile.term.handle.shutdown();
        }
    }

    /// The settings changed: apply the layout and the finished-tile filter.
    pub fn apply_config(self: &Rc<Self>) {
        self.sync_tiles();
        self.relayout();
    }

    // -- Sessions and the state file ------------------------------------

    fn session_for_pane(&self, pane: &gtk::Widget) -> Option<Session> {
        let surface = pane::active_surface_summary(pane)?;
        if surface.kind != "terminal" {
            return None;
        }
        // A promoted tab belongs to the session that started its run.
        let surface_id = self
            .promoted
            .borrow()
            .get(&surface.surface_id)
            .cloned()
            .unwrap_or(surface.surface_id);
        Some(Session {
            surface_id,
            pane: pane.clone(),
        })
    }

    /// The pane in this workspace that holds the keyboard focus.
    fn focused_pane(&self) -> Option<gtk::Widget> {
        let container = self.container.upgrade()?;
        let window = container.widget().root()?.downcast::<gtk::Window>().ok()?;
        let mut current = gtk::prelude::GtkWindowExt::focus(&window);
        while let Some(widget) = current {
            if pane::is_pane_widget(&widget) && container.contains(&widget) {
                return Some(widget);
            }
            current = widget.parent();
        }
        None
    }

    fn set_session(self: &Rc<Self>, session: Option<Session>) {
        let same = match (self.session.borrow().as_ref(), session.as_ref()) {
            (Some(a), Some(b)) => a.surface_id == b.surface_id,
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        *self.session.borrow_mut() = session;
        *self.loaded.borrow_mut() = None;
        *self.selected.borrow_mut() = None;
        self.reload();
    }

    /// Read the session's state file again when its mtime changed.
    /// Returns true when the shown state changed.
    fn reload(self: &Rc<Self>) -> bool {
        let Some(surface_id) = self.session.borrow().as_ref().map(|s| s.surface_id.clone()) else {
            let had = self
                .loaded
                .borrow_mut()
                .take()
                .is_some_and(|l| l.state.is_some());
            if had {
                self.state_changed();
            }
            return had;
        };
        let Some(dir) = dip_state::state_dir() else {
            return false;
        };
        let path = dir.join(dip_state::state_file_name(&surface_id));
        let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        let unchanged = self
            .loaded
            .borrow()
            .as_ref()
            .is_some_and(|l| l.path == path && l.modified == modified);
        let mut state = if unchanged {
            self.loaded.borrow().as_ref().and_then(|l| l.state.clone())
        } else {
            modified
                .and_then(|_| std::fs::read_to_string(&path).ok())
                .and_then(|raw| match dip_state::parse(&raw) {
                    Ok(state) => Some(state),
                    Err(err) => {
                        eprintln!("limux: zoom view: {}: {err}", path.display());
                        None
                    }
                })
        };
        // The session ended: its heartbeat stopped.
        if state.as_ref().is_some_and(|s| s.is_stale(now_ms())) {
            state = None;
        }
        let changed = self.loaded.borrow().as_ref().map(|l| &l.state) != Some(&state);
        *self.loaded.borrow_mut() = Some(LoadedState {
            path,
            modified,
            state,
        });
        if changed {
            self.state_changed();
        }
        changed
    }

    fn current_state(&self) -> Option<DipState> {
        self.loaded.borrow().as_ref().and_then(|l| l.state.clone())
    }

    fn state_changed(self: &Rc<Self>) {
        let filter = self.filter.get();
        let state = self.current_state().map(|state| state.filtered(filter));
        self.rail.update(state.as_ref());
        self.sync_show_buttons();
        if self.level.get().is_zoomed() {
            self.sync_tiles();
            self.relayout();
        } else {
            self.prune_tiles(&self.tile_runs());
        }
    }

    fn poll(self: &Rc<Self>) {
        self.follow_saved_rail_width();
        self.follow_tile_font();
        if !self.level.get().is_zoomed() {
            if let Some(pane) = self.focused_pane() {
                let session = self.session_for_pane(&pane);
                self.set_session(session);
            }
        }
        self.reload();
        if self.level.get().is_zoomed() {
            let size = self.size();
            if size != self.last_size.get() {
                self.relayout();
            }
            self.sync_titles();
        }
        self.notify_status();
    }

    /// The tile font size (two points below the terminal font) and the
    /// current font generation.
    fn tile_font(&self) -> (f32, u64) {
        let default_size = terminal::base_ghostty_font_defaults().size;
        let size = self.config.borrow().effective_font_size(default_size);
        (geometry::tile_font_size(size), terminal::font_generation())
    }

    /// A font change in Settings resets every terminal to the new font
    /// size; the tiles then get their smaller size again.
    fn follow_tile_font(&self) {
        let wanted = self.tile_font();
        let applied = self.applied_tile_font.replace(wanted);
        if applied == wanted || applied.1 == 0 && applied.0 == 0.0 {
            return;
        }
        let action = format!("set_font_size:{}", wanted.0);
        for tile in self.tiles.borrow().iter() {
            tile.term.handle.perform_binding_action(&action);
        }
    }

    fn follow_saved_rail_width(&self) {
        let saved = self.config.borrow().zoom_view.plan_rail_width;
        if self.applied_rail_width.replace(saved) != saved && self.rail.open_width() != saved as i32
        {
            self.rail.set_open_width(saved as i32);
            if self.level.get().is_zoomed() {
                self.relayout();
            }
        }
    }

    /// The main handle: a drag moves the main card's right edge (live),
    /// its end saves the share for this zoom level, and a double-click
    /// goes back to the level's default.
    fn connect_main_handle(self: &Rc<Self>) {
        let drag = gtk::GestureDrag::new();
        // The main card's width and the pointer's window x at the start.
        let start = Rc::new(Cell::new((0.0, 0.0)));
        {
            let weak = Rc::downgrade(self);
            let start = start.clone();
            drag.connect_drag_begin(move |drag, _, _| {
                if let Some(view) = weak.upgrade() {
                    let x = super::drag_x_in_window(drag).unwrap_or(0.0);
                    start.set((f64::from(view.main_slot.width()), x));
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            let start = start.clone();
            drag.connect_drag_update(move |drag, _, _| {
                let Some(view) = weak.upgrade() else { return };
                let (start_px, start_x) = start.get();
                let x = super::drag_x_in_window(drag).unwrap_or(start_x);
                let width = f64::from(view.size().0);
                let share = geometry::dragged_share(start_px, x - start_x, width);
                if view.drag_share.replace(Some(share)) != Some(share) {
                    view.relayout();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            drag.connect_drag_end(move |_, _, _| {
                let Some(view) = weak.upgrade() else { return };
                // A click without a move has no share: the width stays.
                if let Some(share) = view.drag_share.take() {
                    view.save_main_share(Some(share));
                }
                view.relayout();
            });
        }
        self.main_handle.add_controller(drag);

        let click = gtk::GestureClick::new();
        let weak = Rc::downgrade(self);
        click.connect_pressed(move |_, n_press, _, _| {
            if n_press != 2 {
                return;
            }
            if let Some(view) = weak.upgrade() {
                view.drag_share.set(None);
                view.save_main_share(None);
                view.relayout();
            }
        });
        self.main_handle.add_controller(click);
    }

    /// Keep the main terminal's share for the current zoom level (None:
    /// the default) for every workspace and the next start.
    fn save_main_share(&self, share: Option<f64>) {
        let level = self.level.get();
        let snapshot = {
            let mut config = self.config.borrow_mut();
            let before = config.zoom_view.main_share_permille;
            config.zoom_view.set_main_share(level, share);
            if config.zoom_view.main_share_permille == before {
                return;
            }
            config.clone()
        };
        if let Err(err) = crate::app_config::save(&snapshot) {
            eprintln!("limux: zoom view: cannot save the terminal width: {err}");
        }
    }

    /// Keep a dragged rail width for new workspaces and the next start.
    fn save_rail_width(&self, width: i32) {
        let width = width.max(0) as u32;
        let snapshot = {
            let mut config = self.config.borrow_mut();
            if config.zoom_view.plan_rail_width == width {
                return;
            }
            config.zoom_view.plan_rail_width = width;
            self.applied_rail_width.set(width);
            config.clone()
        };
        if let Err(err) = crate::app_config::save(&snapshot) {
            eprintln!("limux: zoom view: cannot save the plan rail width: {err}");
        }
    }

    fn write_plan_choice(&self, plan: &str) {
        let Some(surface_id) = self.session.borrow().as_ref().map(|s| s.surface_id.clone()) else {
            return;
        };
        let Some(dir) = dip_state::state_dir() else {
            return;
        };
        let path = dir.join(dip_state::select_file_name(&surface_id));
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::json!({ "plan": plan }).to_string();
        let result = std::fs::create_dir_all(&dir)
            .and_then(|_| std::fs::write(&tmp, body))
            .and_then(|_| std::fs::rename(&tmp, &path));
        if let Err(err) = result {
            eprintln!("limux: zoom view: cannot write {}: {err}", path.display());
        }
    }

    fn sync_show_buttons(&self) {
        let Some(container) = self.container.upgrade() else {
            return;
        };
        // Only the pane of the session gets the button: the plan belongs
        // to that terminal, not to its neighbours.
        let session_pane = self
            .session
            .borrow()
            .as_ref()
            .filter(|_| self.rail.wants_show_button())
            .map(|session| session.pane.clone());
        for pane in container.panes() {
            if let Some(button) = pane::pane_plan_button(&pane) {
                button.set_visible(session_pane.as_ref() == Some(&pane));
            }
        }
        // While zoomed the pane header hides; the card title has the button.
        self.show_plan.set_visible(session_pane.is_some());
    }

    // -- Tiles ----------------------------------------------------------

    fn tile_runs(&self) -> Vec<Run> {
        let show_finished = self.zoom_config().show_finished;
        self.current_state()
            .map(|state| {
                let filter = self.filter.get();
                state
                    .tile_runs(show_finished)
                    .into_iter()
                    .filter(|run| filter.keeps(run))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    fn make_tile(self: &Rc<Self>, run: &Run) -> Option<Tile> {
        let command = dip_state::shell_command(&run.follow)?;
        let config = self.config.borrow();
        let term = terminal::create_terminal(
            None,
            TerminalOptions {
                suppress_initial_focus: true,
                saved_font_size: Some(self.tile_font().0),
                startup_command: Some(command),
                ..TerminalOptions::default()
            },
            TerminalCallbacks::disconnected(),
        );
        drop(config);
        // Read-only: clicks select the tile and the wheel scrolls the list.
        term.root.set_can_target(false);
        term.root.set_vexpand(true);

        let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        frame.add_css_class("limux-zoom-tile");
        frame.set_widget_name(&format!("{TILE_NAME_PREFIX}{}", run.id));
        frame.set_overflow(gtk::Overflow::Hidden);
        let title = CardTitle::new();
        frame.append(&title.root);
        frame.append(&term.root);

        let click = gtk::GestureClick::new();
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        let run_id = run.id.clone();
        click.connect_pressed(move |gesture, n_press, _, _| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            if let Some(view) = weak.upgrade() {
                view.select(Some(run_id.clone()));
                if n_press == 2 {
                    view.open_in_tab(&run_id);
                }
            }
        });
        frame.add_controller(click);

        self.grid.attach(&frame, 0, 0, 1, 1);
        Some(Tile {
            run_id: run.id.clone(),
            frame,
            title,
            term,
            follow: run.follow.clone(),
        })
    }

    /// Make the tiles match the runs: new tiles for new runs, and the
    /// surfaces of removed runs end. A tile keeps its surface while its
    /// run stays, so its output is not lost on a state change.
    /// Stop the tiles whose run left the list (it finished more than a
    /// minute ago, or a filter hides it): their terminal and its follow
    /// process end, so a long session does not keep old terminals.
    fn prune_tiles(&self, runs: &[Run]) {
        self.tiles.borrow_mut().retain(|tile| {
            let keep = runs
                .iter()
                .any(|run| run.id == tile.run_id && run.follow == tile.follow);
            if !keep {
                tile.term.handle.shutdown();
                self.grid.remove(&tile.frame);
            }
            keep
        });
    }

    fn sync_tiles(self: &Rc<Self>) {
        let runs = self.tile_runs();
        self.prune_tiles(&runs);
        let mut tiles = self.tiles.borrow_mut();
        let mut ordered = Vec::with_capacity(runs.len());
        for run in &runs {
            let index = tiles.iter().position(|tile| tile.run_id == run.id);
            let tile = match index {
                Some(index) => tiles.remove(index),
                None => match self.make_tile(run) {
                    Some(tile) => tile,
                    None => continue,
                },
            };
            tile.title.set(
                run.status,
                &run.name,
                run.model.as_deref(),
                run.elapsed_label(now_ms()).as_deref(),
            );
            if run.status.is_running() {
                tile.frame.remove_css_class("limux-zoom-tile-done");
            } else {
                tile.frame.add_css_class("limux-zoom-tile-done");
            }
            ordered.push(tile);
        }
        *tiles = ordered;
        drop(tiles);
        self.sync_selection();
        self.sync_titles();
    }

    /// The elapsed times of the tiles and the main card's title. The poll
    /// calls it, so the times move on.
    fn sync_titles(&self) {
        let state = self.current_state();
        let now = now_ms();
        for tile in self.tiles.borrow().iter() {
            if let Some(run) = state.as_ref().and_then(|s| s.run(&tile.run_id)) {
                tile.title
                    .end
                    .set_label(&run.elapsed_label(now).unwrap_or_default());
            }
        }
        let Some(session) = self.session.borrow().clone() else {
            return;
        };
        Self::sync_pane_header(&session.pane);
        let summary = pane::active_surface_summary(&session.pane);
        let name = summary
            .as_ref()
            .map(|s| s.title.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "Terminal".to_string());
        let plan = state
            .as_ref()
            .and_then(|s| s.plan.as_ref())
            .map(|p| p.title.clone());
        let size = pane::terminal_handle_for_surface(&session.pane, None)
            .map(|(_, handle)| handle.health())
            .filter(|h| h.columns > 0)
            .map(|h| format!("{} × {}", h.columns, h.rows));
        self.main_title
            .set(RunStatus::Running, &name, plan.as_deref(), size.as_deref());
    }

    pub fn select(&self, run_id: Option<String>) {
        *self.selected.borrow_mut() = run_id;
        self.sync_selection();
        let selected = self.selected.borrow().clone();
        if let Some(tile) = selected.as_deref().and_then(|id| {
            self.tiles
                .borrow()
                .iter()
                .find(|t| t.run_id == id)
                .map(|t| t.frame.clone())
        }) {
            if let Some(bounds) = tile.compute_bounds(&self.grid) {
                let adj = self.scroller.vadjustment();
                let top = f64::from(bounds.y());
                let bottom = top + f64::from(bounds.height());
                if top < adj.value() || bottom > adj.value() + adj.page_size() {
                    adj.set_value(top);
                }
            }
        }
    }

    fn sync_selection(&self) {
        let selected = self.selected.borrow();
        for tile in self.tiles.borrow().iter() {
            if selected.as_deref() == Some(tile.run_id.as_str()) {
                tile.frame.add_css_class("limux-zoom-tile-selected");
            } else {
                tile.frame.remove_css_class("limux-zoom-tile-selected");
            }
        }
    }

    // -- Zoom steps -----------------------------------------------------

    /// Ctrl+wheel down over `pane` (a pane of this workspace). Returns
    /// false when there is nothing to zoom to, so the wheel goes to the
    /// terminal as before.
    pub fn zoom_out(self: &Rc<Self>, pane: Option<&gtk::Widget>) -> bool {
        let Some(container) = self.container.upgrade() else {
            return false;
        };
        let level = self.level.get();
        if level.is_zoomed() {
            self.set_level(level.out());
            return true;
        }
        let Some(pane) = pane else {
            return false;
        };
        let Some(session) = self.session_for_pane(pane) else {
            return false;
        };
        self.set_session(Some(session));
        self.reload();
        if self.tile_runs().is_empty() {
            return false;
        }
        // Move the rail into the frame, then show the frame.
        if let Some(parent) = self.rail.root.parent().and_downcast::<gtk::Box>() {
            parent.remove(&self.rail.root);
        }
        self.rail_holder.append(&self.rail.root);
        if !container.enter_zoom_frame(pane, self.frame.clone().upcast(), self.main_slot.clone()) {
            self.return_rail(&container);
            return false;
        }
        self.level.set(ZoomLevel::Z1);
        Self::sync_pane_header(pane);
        self.sync_tiles();
        self.relayout();
        self.notify_status();
        true
    }

    /// The zoom-out shortcut: zoom out of the focused pane (or one more
    /// step when the view is zoomed already).
    pub fn zoom_out_focused(self: &Rc<Self>) -> bool {
        let pane = self.focused_pane();
        self.zoom_out(pane.as_ref())
    }

    /// A top-bar level button: go straight to `target`. Z0 returns to the
    /// splits; a deeper level zooms out of the focused pane
    /// first. Returns false when there is nothing to zoom to.
    pub fn go_to(self: &Rc<Self>, target: ZoomLevel) -> bool {
        let level = self.level.get();
        if target == level {
            return true;
        }
        if !target.is_zoomed() {
            self.leave();
            return true;
        }
        if !level.is_zoomed() && !self.zoom_out_focused() {
            return false;
        }
        self.set_level(target);
        true
    }

    /// Ctrl+wheel up: one level in, from Z1 back to Z0. The selected tile
    /// does not matter; the main terminal gets the keyboard focus.
    pub fn zoom_in(self: &Rc<Self>) -> bool {
        let level = self.level.get();
        if !level.is_zoomed() {
            return false;
        }
        if level > ZoomLevel::Z1 {
            self.set_level(level.r#in());
        } else {
            self.leave();
        }
        self.focus_main();
        true
    }

    /// The rail's "Subagents" button: zoom out of the session's pane to
    /// Z1, with the keyboard focus on its terminal.
    fn show_subagents(self: &Rc<Self>) {
        let pane = self.session.borrow().as_ref().map(|s| s.pane.clone());
        if self.level.get().is_zoomed() || !self.zoom_out(pane.as_ref()) {
            return;
        }
        self.focus_main();
    }

    /// The keyboard focus to the session's terminal.
    fn focus_main(&self) {
        let Some(session) = self.session.borrow().clone() else {
            return;
        };
        if let Some((_, handle)) = pane::terminal_handle_for_surface(&session.pane, None) {
            handle.focus_surface();
        }
    }

    /// A double-click on a tile: the run opens as a new tab in the
    /// session's pane, and the zoom level stays.
    fn open_in_tab(self: &Rc<Self>, run_id: &str) {
        let Some(run) = self
            .current_state()
            .and_then(|state| state.run(run_id).cloned())
        else {
            return;
        };
        // The zoom level stays: the tab opens in the main terminal's pane,
        // and the pane header comes back so the tabs can be switched.
        let session = self.session.borrow().clone();
        if let Some(session) = session {
            self.promote(&session, &run);
            if self.level.get().is_zoomed() {
                Self::sync_pane_header(&session.pane);
            }
        }
    }

    /// While zoomed, the card title replaces the pane header. A pane with
    /// more than one tab keeps its header, so its tabs stay reachable.
    fn sync_pane_header(pane: &gtk::Widget) {
        pane::set_pane_header_visible(pane, pane::tab_count_in_pane(pane) > 1);
    }

    fn set_level(&self, level: ZoomLevel) {
        if self.level.replace(level) != level {
            self.relayout();
        }
        self.notify_status();
    }

    /// Every pane header back (the zoomed pane may have moved meanwhile).
    fn show_pane_headers(container: &SplitTreeContainer) {
        for pane in container.panes() {
            pane::set_pane_header_visible(&pane, true);
        }
    }

    fn return_rail(&self, container: &SplitTreeContainer) {
        if let Some(parent) = self.rail.root.parent().and_downcast::<gtk::Box>() {
            parent.remove(&self.rail.root);
        }
        container.rail_slot().append(&self.rail.root);
    }

    /// Back to Z0: the splits return. The tiles keep their surfaces; they
    /// are unmapped, so they stop drawing.
    fn leave(&self) {
        self.level.set(ZoomLevel::Z0);
        *self.selected.borrow_mut() = None;
        self.sync_selection();
        if let Some(container) = self.container.upgrade() {
            self.return_rail(&container);
            Self::show_pane_headers(&container);
            container.leave_zoom_frame();
        }
        self.notify_status();
    }

    /// The split container dropped the frame (a split, a close, or the
    /// pane-zoom toggle).
    fn frame_dropped(&self) {
        self.level.set(ZoomLevel::Z0);
        *self.selected.borrow_mut() = None;
        self.sync_selection();
        if let Some(container) = self.container.upgrade() {
            self.return_rail(&container);
            Self::show_pane_headers(&container);
        }
        self.notify_status();
    }

    /// The tab runs the run's `chat` argv, or `follow` when `chat` is null.
    fn promote(&self, session: &Session, run: &Run) {
        let argv = run.chat.as_ref().unwrap_or(&run.follow);
        let Some(command) = dip_state::shell_command(argv) else {
            return;
        };
        if let Some(surface) = pane::add_command_tab_to_pane(&session.pane, &run.name, command) {
            self.promoted
                .borrow_mut()
                .insert(surface, session.surface_id.clone());
        }
    }

    // -- Layout ---------------------------------------------------------

    fn size(&self) -> (i32, i32) {
        self.container
            .upgrade()
            .map(|c| (c.widget().width(), c.widget().height()))
            .unwrap_or_default()
    }

    fn cell_size(&self) -> (f64, f64) {
        let Some(session) = self.session.borrow().clone() else {
            return FALLBACK_CELL;
        };
        let Some((_, handle)) = pane::terminal_handle_for_surface(&session.pane, None) else {
            return FALLBACK_CELL;
        };
        let health = handle.health();
        if health.columns == 0 || health.rows == 0 {
            return FALLBACK_CELL;
        }
        let scale = f64::from(self.main_slot.scale_factor().max(1));
        (
            f64::from(health.width_px) / scale / f64::from(health.columns),
            f64::from(health.height_px) / scale / f64::from(health.rows),
        )
    }

    fn relayout(&self) {
        if !self.level.get().is_zoomed() {
            return;
        }
        let config = self.zoom_config();
        let (width, height) = self.size();
        self.last_size.set((width, height));
        let (cell_width, cell_height) = self.cell_size();
        let frame = Frame {
            width: f64::from(width),
            height: f64::from(height),
            cell_width,
            cell_height,
            rail_width: self.rail.width(),
            main_min_columns: config.main_min_columns,
            main_share: self
                .drag_share
                .get()
                .or_else(|| config.main_share(self.level.get())),
        };
        let level = self.level.get();
        let placement = geometry::place(config.layout, frame, level);

        let tiles = self.tiles.borrow();
        let tile_px_height = (f64::from(placement.tile_rows) * cell_height).round() as i32;
        let side_by_side = matches!(config.layout, ZoomLayout::Focus | ZoomLayout::Grid);
        self.main_handle.set_visible(side_by_side);
        if side_by_side {
            place(&self.frame, &self.main_slot, 0, 0, 1, 1);
            place(&self.frame, &self.main_handle, 1, 0, 1, 1);
            place(&self.frame, &self.rail_holder, 2, 0, 1, 1);
            place(&self.frame, &self.tiles_box, 3, 0, 1, 1);
            self.main_slot.set_hexpand(false);
            self.main_slot.set_vexpand(true);
            self.main_slot
                .set_size_request(placement.main_px.round() as i32, -1);
            self.tiles_box.set_vexpand(true);
            self.scroller.set_propagate_natural_height(false);
            self.scroller.set_max_content_height(-1);
        } else {
            // The handle is hidden; GTK skips a hidden grid child.
            place(&self.frame, &self.main_slot, 0, 0, 1, 1);
            place(&self.frame, &self.main_handle, 3, 0, 1, 1);
            place(&self.frame, &self.tiles_box, 0, 1, 1, 1);
            place(&self.frame, &self.rail_holder, 2, 0, 1, 2);
            // The tile row takes its natural height and the main terminal
            // the rest. A fixed main height would push the frame past the
            // workspace when the real tile chrome is taller than the
            // estimate, and `size()` would then read the larger height.
            self.main_slot.set_hexpand(true);
            self.main_slot.set_vexpand(true);
            self.main_slot.set_size_request(-1, -1);
            self.tiles_box.set_vexpand(false);
            // The tree scrolls through its rows; cap it at half the frame.
            let cap = if config.layout == ZoomLayout::Tree {
                height / 2
            } else {
                -1
            };
            self.scroller.set_max_content_height(cap);
            self.scroller.set_propagate_natural_height(true);
        }

        let mut columns = placement.grid_columns.max(1);
        if config.layout == ZoomLayout::Grid {
            // Option B: one more grid column while tiles stay readable.
            let tile_area = frame.width - frame.rail_width - placement.main_px;
            let wider = columns + 1;
            let each = (tile_area - geometry::TILE_GAP * f64::from(columns)) / f64::from(wider);
            if each / cell_width >= f64::from(geometry::MIN_TILE_COLUMNS) {
                columns = wider;
            }
        }
        let strip = config.layout == ZoomLayout::Strip;
        self.scroller.set_hscrollbar_policy(if strip {
            gtk::PolicyType::Automatic
        } else {
            gtk::PolicyType::Never
        });
        self.scroller.set_vscrollbar_policy(if strip {
            gtk::PolicyType::Never
        } else {
            gtk::PolicyType::Automatic
        });
        self.grid.set_column_homogeneous(!strip);

        let state = self.current_state();
        let mut per_depth: HashMap<usize, i32> = HashMap::new();
        for (index, tile) in tiles.iter().enumerate() {
            let index = index as i32;
            let (column, row) = match config.layout {
                ZoomLayout::Strip => (index, 0),
                ZoomLayout::Tree => {
                    let depth = state
                        .as_ref()
                        .and_then(|s| s.run(&tile.run_id).map(|run| s.depth(run)))
                        .unwrap_or(0);
                    let slot = per_depth.entry(depth).or_insert(0);
                    let column = *slot;
                    *slot += 1;
                    (column, depth as i32)
                }
                ZoomLayout::Focus | ZoomLayout::Grid => {
                    (index % columns as i32, index / columns as i32)
                }
            };
            place(&self.grid, &tile.frame, column, row, 1, 1);
            tile.term.root.set_size_request(-1, tile_px_height);
            let width = if strip || config.layout == ZoomLayout::Tree {
                (f64::from(placement.tile_columns) * cell_width + geometry::TILE_CHROME_WIDTH)
                    .round() as i32
            } else {
                -1
            };
            tile.frame.set_size_request(width, -1);
        }
        self.tile_height
            .set(f64::from(tile_px_height + TILE_TITLE_HEIGHT) + geometry::TILE_GAP);
        self.grid_columns.set(columns);

        let running = tiles
            .iter()
            .filter(|tile| {
                state
                    .as_ref()
                    .and_then(|s| s.run(&tile.run_id))
                    .is_some_and(|run| run.status.is_running())
            })
            .count();
        let level_name = format!("{level:?}");
        self.tiles_header.set_label(&format!(
            "{} subagents · {running} running · {level_name}",
            tiles.len()
        ));
        let has_tiles = !tiles.is_empty();
        drop(tiles);
        self.scroller.set_visible(has_tiles);
        self.empty.set_visible(!has_tiles);
        self.update_indicator();
    }

    /// "N more subagents below · scroll", hidden at the end of the list.
    fn update_indicator(&self) {
        let adj = self.scroller.vadjustment();
        let below = (adj.upper() - (adj.value() + adj.page_size())).max(0.0);
        let tile_height = self.tile_height.get();
        let total = self.tiles.borrow().len() as u32;
        let hidden = if tile_height <= 0.0 || below < 1.0 {
            0
        } else {
            let rows = (below / tile_height).ceil() as u32;
            (rows * self.grid_columns.get()).min(total)
        };
        let strip = self.zoom_config().layout == ZoomLayout::Strip;
        self.indicator.set_visible(hidden > 0 && !strip);
        let noun = if hidden == 1 { "subagent" } else { "subagents" };
        self.indicator
            .set_label(&format!("{hidden} more {noun} below · scroll"));
    }
}

impl Drop for ZoomView {
    fn drop(&mut self) {
        self.shutdown();
    }
}
