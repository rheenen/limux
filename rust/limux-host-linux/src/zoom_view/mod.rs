//! The zoom view: Ctrl+wheel on a terminal shows the subagents of its
//! session as live tiles, with the /dip plan as a native rail.
//!
//! Limux reads ONLY the /dip mod's state file (see `dip_state`); it does
//! not detect tmux sessions or child processes itself.

pub mod bar;
pub mod config;
pub mod dip_state;
pub mod geometry;
pub mod rail;
pub mod view;

/// The priority of the zoom view's CSS. A GTK theme can install a full
/// `~/.config/gtk-4.0/gtk.css` (MacTahoe does), which GTK loads at user
/// priority, above the app CSS; its button backgrounds then hide the
/// selected zoom level and filter. One above user priority, as the
/// workspace colour swatches do.
pub(crate) const CSS_PRIORITY: u32 = gtk4::STYLE_PROVIDER_PRIORITY_USER + 1;
