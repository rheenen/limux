//! The subagent view of one workspace. The splits always stay. Beside
//! them is the /dip plan rail when the focused terminal's session has a
//! plan. The rail's "Subagents" button shows that session's subagents as
//! live tiles to the right of the rail; "Hide subagents" hides them. The
//! top of the subagent pane has the "Hide done" and "Hide shells" filters
//! (kept in settings.json) and a close button that hides the pane. A
//! double-click on a tile opens its run as a new tab in the session's
//! pane.

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
use super::geometry::{self, Frame};
use super::rail::{HandleSide, PlanRail};
use crate::app_config::AppConfig;
use crate::pane;
use crate::split_tree::SplitTreeContainer;
use crate::terminal::{self, TerminalCallbacks, TerminalOptions, TerminalWidget};

const POLL_INTERVAL: Duration = Duration::from_millis(500);
/// The drag handle between the terminals and the rail while the tiles
/// show.
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

/// The title bar of a tile: status icon, name, model, then the elapsed
/// time at the right. It has no buttons.
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
    /// Holds the split tree.
    main_slot: gtk::Box,
    rail_holder: gtk::Box,
    /// The drag handle on the terminals' right edge while the tiles show
    /// (focus and grid layouts): it sets the terminals' share of the
    /// width.
    main_handle: gtk::Box,
    tiles_box: gtk::Box,
    tiles_header: gtk::Label,
    hide_done: gtk::ToggleButton,
    hide_shells: gtk::ToggleButton,
    scroller: gtk::ScrolledWindow,
    grid: gtk::Grid,
    empty: gtk::Label,
    indicator: gtk::Label,
    rail: Rc<PlanRail>,
    /// The subagent tiles show.
    shown: Cell<bool>,
    /// The session of the rail and the tiles. It follows the keyboard
    /// focus while the tiles are hidden, and stays while they show.
    session: RefCell<Option<Session>>,
    loaded: RefCell<Option<LoadedState>>,
    tiles: RefCell<Vec<Tile>>,
    selected: RefCell<Option<String>>,
    last_size: Cell<(i32, i32)>,
    tile_height: Cell<f64>,
    grid_columns: Cell<u32>,
    timer: RefCell<Option<glib::SourceId>>,
    /// The saved rail width this view last applied; another workspace's
    /// drag changes the setting, and the poll then follows it.
    applied_rail_width: Cell<u32>,
    /// Promoted tab surface id -> the surface id of its parent session.
    promoted: RefCell<HashMap<String, String>>,
    /// The tile font size and the font generation it was applied for.
    applied_tile_font: Cell<(f32, u64)>,
    /// The filters this view applies; the poll follows the saved ones,
    /// which another workspace may change.
    filter: Cell<RunFilter>,
    /// The share while the main handle is dragged; saved when it ends.
    drag_share: Cell<Option<f64>>,
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

/// A filter toggle at the top of the subagent pane: icon and label.
fn filter_button(label: &str, icon: &str, active: bool) -> gtk::ToggleButton {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.append(&gtk::Image::from_icon_name(icon));
    content.append(&gtk::Label::new(Some(label)));
    let button = gtk::ToggleButton::new();
    button.set_child(Some(&content));
    button.set_active(active);
    button.add_css_class("flat");
    button.add_css_class("limux-zoom-filter");
    button.set_focus_on_click(false);
    button.set_valign(gtk::Align::Center);
    button
}

thread_local! {
    static PROVIDER: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
}

fn install_css() {
    if PROVIDER.with(|provider| provider.borrow().is_some()) {
        return;
    }
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_data(&view_css(libadwaita::StyleManager::default().is_dark()));
    gtk::style_context_add_provider_for_display(&display, &provider, super::CSS_PRIORITY);
    PROVIDER.with(|slot| *slot.borrow_mut() = Some(provider));
}

/// Load the view's CSS for the dark or the light colour scheme. The window
/// calls this when the scheme changes; before the first view exists it does
/// nothing, and `install_css` reads the scheme itself.
pub(crate) fn sync_color_scheme(dark: bool) {
    PROVIDER.with(|provider| {
        if let Some(provider) = provider.borrow().as_ref() {
            provider.load_from_data(&view_css(dark));
        }
    });
}

/// The colours of the view that do not come from the theme. The dark
/// values are light so they read on a dark window; the light values are
/// dark so they read on a light window.
struct ViewPalette {
    /// Behind the rail and the tiles. The window's content area is dark in
    /// both schemes (it matches the terminal), so the light scheme paints
    /// these panels with the window colour.
    panel: &'static str,
    handle: &'static str,
    focus: &'static str,
    selected: &'static str,
    running: &'static str,
    done: &'static str,
    failed: &'static str,
    limit: &'static str,
    taken_over: &'static str,
}

const DARK_PALETTE: ViewPalette = ViewPalette {
    panel: "transparent",
    handle: "#c8c8c8",
    focus: "#6aa6ff",
    selected: "#f0b45e",
    running: "#e5c07b",
    done: "#6fcf8f",
    failed: "#ff8a8a",
    limit: "#d68ae6",
    taken_over: "#56c8d8",
};

const LIGHT_PALETTE: ViewPalette = ViewPalette {
    panel: "@window_bg_color",
    handle: "#8c8c8c",
    focus: "#1f6feb",
    selected: "#c87f0a",
    running: "#8a5a00",
    done: "#1a7f37",
    failed: "#c62828",
    limit: "#8e3fb0",
    taken_over: "#0b6e7a",
};

/// The CSS of the view. Every pane is a card with rounded corners; the
/// pane with the keyboard focus has a blue edge. The drag handles show a
/// gray line, so the edge between the rail and the tiles is visible.
fn view_css(dark: bool) -> String {
    let ViewPalette {
        panel,
        handle,
        focus,
        selected,
        running,
        done,
        failed,
        limit,
        taken_over,
    } = if dark { DARK_PALETTE } else { LIGHT_PALETTE };
    format!(
        "\
         .limux-plan-rail, .limux-plan-strip, .limux-zoom-tiles {{ background-color: {panel}; }}\n\
         .limux-zoom-tiles {{ padding: 4px 6px 0 6px; }}\n\
         .limux-plan-strip {{ border-left: 1px solid alpha(currentColor, 0.12); border-right: 1px solid alpha(currentColor, 0.12); }}\n\
         .limux-plan-rail-handle, .limux-zoom-main-handle {{ background: linear-gradient(to right, transparent 2px, {handle} 2px, {handle} 4px, transparent 4px); }}\n\
         .limux-plan-rail-handle:hover, .limux-zoom-main-handle:hover {{ background: alpha({handle}, 0.6); }}\n\
         .limux-plan-strip-count {{ font-weight: bold; font-size: 0.9em; }}\n\
         .limux-zoom-tile, .limux-pane-card {{ border: 1px solid alpha(currentColor, 0.15); border-radius: 10px; }}\n\
         .limux-pane-card {{ margin: 3px; }}\n\
         .limux-pane-card:focus-within {{ border-color: alpha({focus}, 0.55); }}\n\
         .limux-zoom-pane-header {{ margin-bottom: 2px; }}\n\
         .limux-zoom-filter {{ padding: 0 8px; min-height: 24px; font-size: 0.85em; }}\n\
         .limux-zoom-filter:checked {{ background: alpha({focus}, 0.3); color: @window_fg_color; }}\n\
         .limux-zoom-pane-close {{ min-height: 0; min-width: 0; padding: 2px; }}\n\
         .limux-zoom-tile-done {{ opacity: 0.6; }}\n\
         .limux-zoom-tile-selected {{ border: 2px solid {selected}; }}\n\
         .limux-zoom-card-title {{ padding: 0 10px; font-size: 0.9em; background: alpha(currentColor, 0.05); border-bottom: 1px solid alpha(currentColor, 0.1); }}\n\
         .limux-zoom-card-glyph {{ font-weight: bold; }}\n\
         .limux-zoom-card-name {{ font-weight: 600; }}\n\
         .limux-zoom-card-end {{ font-family: monospace; }}\n\
         .limux-plan-rail-summary {{ font-size: 0.85em; }}\n\
         .limux-plan-rail-list > row {{ min-height: 0; padding-top: 1px; padding-bottom: 1px; margin-top: 0; margin-bottom: 0; }}\n\
         .limux-zoom-st-running {{ color: {running}; }}\n\
         .limux-zoom-st-done {{ color: {done}; }}\n\
         .limux-zoom-st-failed {{ color: {failed}; }}\n\
         .limux-zoom-st-limit {{ color: {limit}; }}\n\
         .limux-zoom-st-stopped {{ color: alpha(currentColor, 0.55); }}\n\
         .limux-zoom-st-taken-over {{ color: {taken_over}; }}\n\
         .limux-zoom-indicator {{ padding: 4px; font-size: 0.9em; }}\n"
    )
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
        // The split tree goes in here (`mount_in_frame`).
        let main_slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
        main_slot.set_hexpand(true);
        main_slot.set_vexpand(true);
        let rail_holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
        rail_holder.set_vexpand(true);
        rail_holder.set_hexpand(false);

        let tiles_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        tiles_box.set_hexpand(true);
        tiles_box.set_vexpand(true);
        // Padding, not margins: the light scheme paints the box (see `view_css`).
        tiles_box.add_css_class("limux-zoom-tiles");
        let tiles_header = gtk::Label::new(None);
        tiles_header.set_xalign(0.0);
        tiles_header.set_hexpand(true);
        tiles_header.set_ellipsize(gtk::pango::EllipsizeMode::End);
        tiles_header.add_css_class("dim-label");
        let filter = config.borrow().zoom_view.filter();
        let hide_done = filter_button(
            super::filters::HIDE_DONE_LABEL,
            "object-select-symbolic",
            filter.hide_done,
        );
        let hide_shells = filter_button(
            super::filters::HIDE_SHELLS_LABEL,
            "utilities-terminal-symbolic",
            filter.hide_shells,
        );
        let close_pane = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text("Hide the subagents")
            .has_frame(false)
            .valign(gtk::Align::Center)
            .focus_on_click(false)
            .build();
        close_pane.add_css_class("limux-zoom-pane-close");
        let pane_header = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        pane_header.add_css_class("limux-zoom-pane-header");
        pane_header.append(&tiles_header);
        pane_header.append(&hide_done);
        pane_header.append(&hide_shells);
        pane_header.append(&close_pane);
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
        tiles_box.append(&pane_header);
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
            "Drag to change the width of the terminals and the subagents.\nDouble-click for the default width.",
        ));
        main_handle.set_visible(false);
        tiles_box.set_visible(false);

        frame.attach(&main_slot, 0, 0, 1, 1);
        frame.attach(&main_handle, 1, 0, 1, 1);
        frame.attach(&rail_holder, 2, 0, 1, 1);
        frame.attach(&tiles_box, 3, 0, 1, 1);

        let view = Rc::new(Self {
            container: Rc::downgrade(container),
            config,
            frame,
            main_slot,
            rail_holder,
            main_handle,
            tiles_box,
            tiles_header,
            hide_done,
            hide_shells,
            scroller,
            grid,
            empty,
            indicator,
            rail,
            shown: Cell::new(false),
            session: RefCell::new(None),
            loaded: RefCell::new(None),
            tiles: RefCell::default(),
            selected: RefCell::new(None),
            last_size: Cell::new((0, 0)),
            tile_height: Cell::new(0.0),
            grid_columns: Cell::new(1),
            timer: RefCell::new(None),
            applied_rail_width: Cell::new(rail_width),
            promoted: RefCell::default(),
            applied_tile_font: Cell::new((0.0, 0)),
            filter: Cell::new(filter),
            drag_share: Cell::new(None),
        });

        view.rail_holder.append(&view.rail.root);
        container.mount_in_frame(view.frame.upcast_ref(), &view.main_slot);
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
                    view.toggle_subagents();
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
                view.relayout();
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
        for (button, shells) in [(&view.hide_done, false), (&view.hide_shells, true)] {
            let weak = Rc::downgrade(&view);
            button.connect_toggled(move |button| {
                let Some(view) = weak.upgrade() else { return };
                let mut filter = view.filter.get();
                if shells {
                    filter.hide_shells = button.is_active();
                } else {
                    filter.hide_done = button.is_active();
                }
                view.set_filter(filter);
            });
        }
        {
            let weak = Rc::downgrade(&view);
            close_pane.connect_clicked(move |_| {
                if let Some(view) = weak.upgrade() {
                    view.hide_subagents();
                }
            });
        }
        view.sync_filter_buttons();
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

    pub fn subagents_shown(&self) -> bool {
        self.shown.get()
    }

    pub fn rail_mode(&self) -> RailState {
        self.rail.mode()
    }

    pub fn rail_visible(&self) -> bool {
        self.rail.root.is_visible()
    }

    /// The session has a plan or runs: the rail shows them, and the
    /// subagents button shows, also before a subagent starts.
    fn has_plan(&self) -> bool {
        super::filters::has_content(self.current_state().as_ref())
    }

    /// A filter button changed: tiles and rail follow it, and the choice
    /// is saved for every workspace and the next start.
    fn set_filter(self: &Rc<Self>, filter: RunFilter) {
        if self.filter.replace(filter) == filter {
            return;
        }
        self.save_filter(filter);
        self.sync_filter_buttons();
        self.state_changed();
    }

    /// Another workspace changed the saved filters: follow them.
    fn follow_saved_filter(self: &Rc<Self>) {
        let saved = self.config.borrow().zoom_view.filter();
        if self.filter.replace(saved) != saved {
            self.sync_filter_buttons();
            self.state_changed();
        }
    }

    fn sync_filter_buttons(&self) {
        let filter = self.filter.get();
        for (button, on, hint) in [
            (
                &self.hide_done,
                filter.hide_done,
                super::filters::hide_done_hint(filter.hide_done),
            ),
            (
                &self.hide_shells,
                filter.hide_shells,
                super::filters::hide_shells_hint(filter.hide_shells),
            ),
        ] {
            // `set_active` to the same value emits nothing, so a toggle
            // does not come back here.
            button.set_active(on);
            button.set_tooltip_text(Some(hint));
        }
    }

    fn save_filter(&self, filter: RunFilter) {
        let snapshot = {
            let mut config = self.config.borrow_mut();
            if config.zoom_view.filter() == filter {
                return;
            }
            config.zoom_view.set_filter(filter);
            config.clone()
        };
        if let Err(err) = crate::app_config::save(&snapshot) {
            eprintln!("limux: zoom view: cannot save the subagent filters: {err}");
        }
    }

    /// The rail's subagents button follows the plan and the tiles.
    fn notify_status(&self) {
        self.rail
            .set_subagents_button(self.has_plan(), self.shown.get());
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
        if self.shown.get() {
            self.sync_tiles();
        }
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
        if !self.shown.get() {
            self.prune_tiles(&self.tile_runs());
            return;
        }
        // The plan and the runs are gone (the session ended): the tiles
        // hide, and the splits get the space back.
        if !self.has_plan() {
            self.set_shown(false);
            return;
        }
        self.sync_tiles();
        self.relayout();
    }

    fn poll(self: &Rc<Self>) {
        self.follow_saved_rail_width();
        self.follow_saved_filter();
        self.follow_tile_font();
        if self.shown.get() && !self.session_pane_alive() {
            // The session's pane closed: its tiles go with it.
            self.set_shown(false);
        }
        if !self.shown.get() {
            if let Some(pane) = self.focused_pane() {
                let session = self.session_for_pane(&pane);
                self.set_session(session);
            }
        }
        self.reload();
        if self.shown.get() {
            let size = self.size();
            if size != self.last_size.get() {
                self.relayout();
            }
            self.sync_titles();
        }
        self.notify_status();
    }

    /// The session's pane is still in this workspace.
    fn session_pane_alive(&self) -> bool {
        let Some(container) = self.container.upgrade() else {
            return false;
        };
        self.session
            .borrow()
            .as_ref()
            .is_some_and(|session| container.contains(&session.pane))
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
            self.relayout();
        }
    }

    /// The main handle: a drag moves the terminals' right edge (live), its
    /// end saves the share, and a double-click goes back to the default.
    fn connect_main_handle(self: &Rc<Self>) {
        let drag = gtk::GestureDrag::new();
        // The terminals' width and the pointer's window x at the start.
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

    /// Keep the terminals' share (None: the default) for every workspace
    /// and the next start.
    fn save_main_share(&self, share: Option<f64>) {
        let snapshot = {
            let mut config = self.config.borrow_mut();
            let before = config.zoom_view.main_share_permille;
            config.zoom_view.set_main_share(share);
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
        // Only the pane of the session gets the "Show plan" button: the
        // plan belongs to that terminal, not to its neighbours.
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

    /// The elapsed times of the tiles. The poll calls it, so the times
    /// move on.
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

    // -- Show and hide --------------------------------------------------

    /// The rail's subagents button: show the tiles, or hide them.
    fn toggle_subagents(self: &Rc<Self>) {
        if self.shown.get() {
            self.hide_subagents();
        } else {
            self.show_subagents();
        }
    }

    /// Show the subagents of the session as tiles to the right of the
    /// rail. Returns false when the session has no plan and no runs.
    pub fn show_subagents(self: &Rc<Self>) -> bool {
        if self.shown.get() {
            return true;
        }
        // The session of the focused pane, when it has one; else the
        // session that the rail shows now.
        if let Some(session) = self
            .focused_pane()
            .and_then(|pane| self.session_for_pane(&pane))
        {
            self.set_session(Some(session));
        }
        self.reload();
        if !self.has_plan() {
            return false;
        }
        self.set_shown(true);
        true
    }

    /// Hide the tiles: the splits get the space back. The tiles keep
    /// their surfaces; they are unmapped, so they stop drawing.
    pub fn hide_subagents(self: &Rc<Self>) -> bool {
        if !self.shown.get() {
            return false;
        }
        self.set_shown(false);
        self.focus_main();
        true
    }

    fn set_shown(self: &Rc<Self>, shown: bool) {
        self.shown.set(shown);
        if shown {
            self.sync_tiles();
        } else {
            *self.selected.borrow_mut() = None;
            self.sync_selection();
            self.drag_share.set(None);
        }
        self.relayout();
        self.notify_status();
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
    /// session's pane. The tiles stay.
    fn open_in_tab(self: &Rc<Self>, run_id: &str) {
        let Some(run) = self
            .current_state()
            .and_then(|state| state.run(run_id).cloned())
        else {
            return;
        };
        let session = self.session.borrow().clone();
        if let Some(session) = session {
            self.promote(&session, &run);
        }
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
        self.tiles_box.set_visible(self.shown.get());
        if !self.shown.get() {
            // The splits take all the width; the rail is the rightmost
            // column, with its handle on its left edge.
            self.main_handle.set_visible(false);
            self.rail.set_handle_side(HandleSide::Start);
            place(&self.frame, &self.main_slot, 0, 0, 1, 1);
            place(&self.frame, &self.main_handle, 1, 0, 1, 1);
            place(&self.frame, &self.rail_holder, 2, 0, 1, 1);
            place(&self.frame, &self.tiles_box, 3, 0, 1, 1);
            self.main_slot.set_hexpand(true);
            self.main_slot.set_size_request(-1, -1);
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
            main_share: self.drag_share.get().or_else(|| config.main_share()),
        };
        let tiles = self.tiles.borrow();
        let placement = geometry::place(config.layout, frame, tiles.len() as u32);

        let tile_px_height = (f64::from(placement.tile_rows) * cell_height).round() as i32;
        let side_by_side = matches!(config.layout, ZoomLayout::Focus | ZoomLayout::Grid);
        self.main_handle.set_visible(side_by_side);
        // Side by side, the tiles are right of the rail: its handle goes
        // between the rail and the tiles, not next to the main handle.
        // Otherwise the rail is the rightmost column.
        self.rail.set_handle_side(if side_by_side {
            HandleSide::End
        } else {
            HandleSide::Start
        });
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
        self.tiles_header
            .set_label(&format!("{} subagents · {running} running", tiles.len()));
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

#[cfg(test)]
mod css_tests {
    use super::*;

    fn luminance(hex: &str) -> f64 {
        let channel = |i: usize| {
            let value = f64::from(u8::from_str_radix(&hex[i..i + 2], 16).unwrap()) / 255.0;
            if value <= 0.03928 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5)
    }

    fn contrast(a: &str, b: &str) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    fn status_colors(palette: &ViewPalette) -> [&'static str; 5] {
        [
            palette.running,
            palette.done,
            palette.failed,
            palette.limit,
            palette.taken_over,
        ]
    }

    #[test]
    fn status_text_reads_on_its_window() {
        // Adwaita's window backgrounds: #222226 (dark) and #fafafb (light).
        for color in status_colors(&DARK_PALETTE) {
            assert!(contrast(color, "#222226") >= 4.5, "{color} on dark");
        }
        for color in status_colors(&LIGHT_PALETTE) {
            assert!(contrast(color, "#fafafb") >= 4.5, "{color} on light");
        }
    }

    #[test]
    fn both_schemes_style_every_status() {
        for dark in [true, false] {
            let css = view_css(dark);
            for status in [
                RunStatus::Running,
                RunStatus::Done,
                RunStatus::Failed,
                RunStatus::Limit,
                RunStatus::Stopped,
                RunStatus::TakenOver,
            ] {
                assert!(css.contains(&format!(".{} {{", status_css(status))));
            }
        }
        assert!(view_css(false).contains(LIGHT_PALETTE.running));
        assert!(!view_css(false).contains(DARK_PALETTE.running));
    }
}
