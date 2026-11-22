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
