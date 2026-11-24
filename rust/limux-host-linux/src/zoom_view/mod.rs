//! The subagent view: the /dip plan as a native rail beside the splits,
//! and the session's subagents as live tiles to the right of the rail.
//!
//! Limux reads ONLY the /dip mod's state file (see `dip_state`); it does
//! not detect tmux sessions or child processes itself.

pub mod config;
pub mod dip_state;
pub mod filters;
pub mod geometry;
pub mod rail;
pub mod view;

/// The priority of the zoom view's CSS. A GTK theme can install a full
/// `~/.config/gtk-4.0/gtk.css` (MacTahoe does), which GTK loads at user
/// priority, above the app CSS; its button backgrounds then hide the
/// selected filter. One above user priority, as the
/// workspace colour swatches do.
pub(crate) const CSS_PRIORITY: u32 = gtk4::STYLE_PROVIDER_PRIORITY_USER + 1;

/// The pointer's x in the window during `drag`. A drag handle moves with
/// the edge it drags, and GTK gives the drag offset in the handle's own
/// coordinates, so the offset alone lags behind the pointer. The window
/// does not move: take the distance there.
pub(crate) fn drag_x_in_window(drag: &gtk4::GestureDrag) -> Option<f64> {
    use gtk4::prelude::*;
    let widget = drag.widget()?;
    let root = widget.root()?;
    let (start_x, start_y) = drag.start_point()?;
    let (dx, dy) = drag.offset()?;
    let point = gtk4::graphene::Point::new((start_x + dx) as f32, (start_y + dy) as f32);
    widget
        .compute_point(&root, &point)
        .map(|point| f64::from(point.x()))
}
