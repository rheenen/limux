//! Pure layout math for the zoom view. The font never changes: a zoom
//! step only gives the main terminal fewer columns and the tiles fewer
//! rows. The GTK side applies the result once per step, so every PTY
//! resizes once, not once per animation frame.

use super::config::ZoomLayout;

/// The zoom levels. Z0 is the normal workspace; Z3 shows the most tiles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum ZoomLevel {
    #[default]
    Z0,
    Z1,
    Z2,
    Z3,
}

impl ZoomLevel {
    pub fn out(self) -> Self {
        match self {
            Self::Z0 => Self::Z1,
            Self::Z1 => Self::Z2,
            Self::Z2 | Self::Z3 => Self::Z3,
        }
    }

    pub fn r#in(self) -> Self {
        match self {
            Self::Z0 | Self::Z1 => Self::Z0,
            Self::Z2 => Self::Z1,
            Self::Z3 => Self::Z2,
        }
    }

    pub fn is_zoomed(self) -> bool {
        self != Self::Z0
    }

    /// The share of the window's columns that the main terminal targets.
    /// Picked so that a 1920 px window gives 150 / 120 / 120 columns.
    pub fn main_share(self) -> f64 {
        match self {
            Self::Z0 => 1.0,
            Self::Z1 => 0.565,
            Self::Z2 => 0.45,
            Self::Z3 => 0.37,
        }
    }

    /// Text rows per tile in the focus layout.
    fn tile_rows(self) -> u32 {
        match self {
            Self::Z0 => 0,
            Self::Z1 => 14,
            Self::Z2 => 9,
            Self::Z3 => 5,
        }
    }

    /// The grid columns of tiles that this level asks for.
    fn tile_grid_columns(self) -> u32 {
        match self {
            Self::Z0 => 0,
            Self::Z1 => 1,
            Self::Z2 | Self::Z3 => 2,
        }
    }
}

/// The input of one layout pass, all in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub width: f64,
    pub height: f64,
    pub cell_width: f64,
    pub cell_height: f64,
    /// The width of the plan rail as it is now (0 when closed or hidden).
    pub rail_width: f64,
    pub main_min_columns: u32,
    /// The share of the width that the user dragged the main terminal to
    /// on this level; None keeps the level's default share.
    pub main_share: Option<f64>,
}

/// The chrome around a tile's text: title bar, border and padding.
pub const TILE_CHROME_HEIGHT: f64 = 30.0;
pub const TILE_CHROME_WIDTH: f64 = 12.0;
pub const TILE_GAP: f64 = 6.0;
/// A tile narrower than this many columns is not readable; the grid then
/// uses fewer columns.
pub const MIN_TILE_COLUMNS: u32 = 40;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The width of the main terminal in pixels (focus layout), or its
    /// height (strip and tree layouts).
    pub main_px: f64,
    pub main_columns: u32,
    pub grid_columns: u32,
    pub tile_rows: u32,
    pub tile_columns: u32,
    /// How many tiles fit without a scroll.
    pub visible_tiles: u32,
}

fn cols_for(px: f64, cell: f64) -> u32 {
    if cell <= 0.0 {
        return 0;
    }
    (px / cell).floor().max(0.0) as u32
}

/// The focus layout (default): main at the left, full height; the rail;
/// tiles at the right. A zoom step never makes the main terminal
/// narrower than `main_min_columns`; at the floor a step takes space
/// from the tiles. A width dragged with the main handle may go lower.
pub fn focus(frame: Frame, level: ZoomLevel) -> Placement {
    let available = (frame.width - frame.rail_width).max(0.0);
    if !level.is_zoomed() {
        return Placement {
            main_px: available,
            main_columns: cols_for(available - TILE_CHROME_WIDTH, frame.cell_width),
            grid_columns: 0,
            tile_rows: 0,
            tile_columns: 0,
            visible_tiles: 0,
        };
    }
    // A dragged width is the user's choice: it may go below
    // `main_min_columns`, down to the width of a readable tile.
    let floor_columns = if frame.main_share.is_some() {
        MIN_TILE_COLUMNS
    } else {
        frame.main_min_columns
    };
    let floor_px = f64::from(floor_columns) * frame.cell_width + TILE_CHROME_WIDTH;
    let target = match frame.main_share {
        // A dragged width leaves room for one readable tile column.
        Some(share) => (frame.width * share).min(available - min_tile_area(frame.cell_width)),
        None => frame.width * level.main_share(),
    };
    let main_px = target.max(floor_px).min(available);
    let tile_area = (available - main_px).max(0.0);

    let mut grid_columns = level.tile_grid_columns();
    let tile_columns = loop {
        let width = (tile_area - TILE_GAP * f64::from(grid_columns.saturating_sub(1)))
            / f64::from(grid_columns);
        let cols = cols_for(width - TILE_CHROME_WIDTH, frame.cell_width);
        if cols >= MIN_TILE_COLUMNS || grid_columns == 1 {
            break cols;
        }
        grid_columns -= 1;
    };

    let tile_rows = level.tile_rows();
    let tile_height = f64::from(tile_rows) * frame.cell_height + TILE_CHROME_HEIGHT + TILE_GAP;
    let per_column = if tile_columns == 0 {
        0
    } else {
        (frame.height / tile_height).floor().max(0.0) as u32
    };

    Placement {
        main_px,
        main_columns: cols_for(main_px - TILE_CHROME_WIDTH, frame.cell_width),
        grid_columns,
        tile_rows,
        tile_columns,
        visible_tiles: per_column * grid_columns,
    }
}

/// The narrowest tile area: one tile of `MIN_TILE_COLUMNS`, with the
/// tile box margins.
fn min_tile_area(cell_width: f64) -> f64 {
    f64::from(MIN_TILE_COLUMNS) * cell_width + TILE_CHROME_WIDTH + 2.0 * TILE_GAP
}

/// The shares of the width that a drag can give the main terminal.
pub const MIN_MAIN_SHARE: f64 = 0.15;
pub const MAX_MAIN_SHARE: f64 = 0.9;

/// The main terminal's share of `width` after a drag of `dx` pixels on
/// its right edge, which started at `start_px`. `focus` applies the
/// floor and the tile room after this.
pub fn dragged_share(start_px: f64, dx: f64, width: f64) -> f64 {
    if width <= 0.0 {
        return MIN_MAIN_SHARE;
    }
    ((start_px + dx) / width).clamp(MIN_MAIN_SHARE, MAX_MAIN_SHARE)
}

/// The strip layout (option D): the main terminal at full width on top,
/// one row of tiles below it. `main_px` is the height of the main.
pub fn strip(frame: Frame, level: ZoomLevel) -> Placement {
    let width = (frame.width - frame.rail_width).max(0.0);
    let main_columns = cols_for(width - TILE_CHROME_WIDTH, frame.cell_width);
    if !level.is_zoomed() {
        return Placement {
            main_px: frame.height,
            main_columns,
            grid_columns: 0,
            tile_rows: 0,
            tile_columns: 0,
            visible_tiles: 0,
        };
    }
    let tile_rows = level.tile_rows();
    let strip_px = f64::from(tile_rows) * frame.cell_height + TILE_CHROME_HEIGHT + TILE_GAP;
    let min_tile_px = f64::from(MIN_TILE_COLUMNS) * frame.cell_width + TILE_CHROME_WIDTH;
    let count = match level {
        ZoomLevel::Z1 => 2,
        ZoomLevel::Z2 => 3,
        _ => 4,
    };
    let fit = ((width + TILE_GAP) / (min_tile_px + TILE_GAP))
        .floor()
        .max(1.0) as u32;
    let grid_columns = count.min(fit);
    let tile_width = (width - TILE_GAP * f64::from(grid_columns - 1)) / f64::from(grid_columns);
    Placement {
        main_px: (frame.height - strip_px).max(0.0),
        main_columns,
        grid_columns,
        tile_rows,
        tile_columns: cols_for(tile_width - TILE_CHROME_WIDTH, frame.cell_width),
        visible_tiles: grid_columns,
    }
}

/// The placement for any layout. Grid (B) and tree (C) use the focus
/// math for the main terminal and differ only in how the GTK side orders
/// and spans the tiles.
pub fn place(layout: ZoomLayout, frame: Frame, level: ZoomLevel) -> Placement {
    match layout {
        ZoomLayout::Strip => strip(frame, level),
        ZoomLayout::Focus | ZoomLayout::Grid | ZoomLayout::Tree => focus(frame, level),
    }
}

/// The tile terminals use a font this many points smaller than the
/// terminal font, so a tile shows more of a run.
pub const TILE_FONT_STEP: f32 = 2.0;
/// The tile font never gets smaller than this.
pub const MIN_TILE_FONT_SIZE: f32 = 6.0;

/// The font size of a tile terminal for the terminal font size `size`.
pub fn tile_font_size(size: f32) -> f32 {
    (size - TILE_FONT_STEP).max(MIN_TILE_FONT_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A JetBrains Mono-like cell at the default font size.
    fn frame(width: f64) -> Frame {
        Frame {
            width,
            height: 1040.0,
            cell_width: 7.22,
            cell_height: 17.0,
            rail_width: 240.0,
            main_min_columns: 120,
            main_share: None,
        }
    }

    #[test]
    fn a_dragged_share_moves_the_main_edge_within_the_floor_and_the_tile_room() {
        let default = focus(frame(1920.0), ZoomLevel::Z1);
        let wider = focus(
            Frame {
                main_share: Some(0.7),
                ..frame(1920.0)
            },
            ZoomLevel::Z1,
        );
        assert!((wider.main_px - 1344.0).abs() < 0.01);
        assert!(wider.main_px > default.main_px);
        assert!(wider.tile_columns >= MIN_TILE_COLUMNS);
        // A share past the tile room stops at one readable tile column.
        let widest = focus(
            Frame {
                main_share: Some(0.9),
                ..frame(1920.0)
            },
            ZoomLevel::Z1,
        );
        // The room holds the tile box margins too, so one column more fits.
        assert!((MIN_TILE_COLUMNS..=MIN_TILE_COLUMNS + 2).contains(&widest.tile_columns));
        assert_eq!(widest.grid_columns, 1);
        // A small share goes below main_min_columns, down to a tile's
        // minimum.
        let narrow = focus(
            Frame {
                main_share: Some(0.15),
                ..frame(1920.0)
            },
            ZoomLevel::Z1,
        );
        assert_eq!(narrow.main_columns, MIN_TILE_COLUMNS);
        assert!(narrow.tile_columns > default.tile_columns);
    }

    #[test]
    fn a_drag_gives_a_clamped_share_of_the_width() {
        assert!((dragged_share(1000.0, 100.0, 2000.0) - 0.55).abs() < 1e-9);
        assert_eq!(dragged_share(1000.0, -2000.0, 2000.0), MIN_MAIN_SHARE);
        assert_eq!(dragged_share(1000.0, 5000.0, 2000.0), MAX_MAIN_SHARE);
        assert_eq!(dragged_share(10.0, 0.0, 0.0), MIN_MAIN_SHARE);
    }

    #[test]
    fn levels_step_and_clamp() {
        assert_eq!(ZoomLevel::Z0.out(), ZoomLevel::Z1);
        assert_eq!(ZoomLevel::Z3.out(), ZoomLevel::Z3);
        assert_eq!(ZoomLevel::Z1.r#in(), ZoomLevel::Z0);
        assert_eq!(ZoomLevel::Z0.r#in(), ZoomLevel::Z0);
    }

    #[test]
    fn z0_gives_all_space_to_the_main_terminal() {
        let p = focus(frame(1920.0), ZoomLevel::Z0);
        assert_eq!(p.main_px, 1680.0);
        assert_eq!(p.visible_tiles, 0);
    }

    #[test]
    fn main_terminal_never_goes_below_the_floor() {
        for width in [1440.0, 1920.0, 2560.0, 3840.0] {
            for level in [ZoomLevel::Z1, ZoomLevel::Z2, ZoomLevel::Z3] {
                let p = focus(frame(width), level);
                assert!(
                    p.main_columns >= 120,
                    "{width} {level:?}: {}",
                    p.main_columns
                );
            }
        }
    }

    #[test]
    fn a_1920_window_matches_the_design_table() {
        let cols = |l| focus(frame(1920.0), l).main_columns;
        assert!(
            (148..=152).contains(&cols(ZoomLevel::Z1)),
            "{}",
            cols(ZoomLevel::Z1)
        );
        assert_eq!(cols(ZoomLevel::Z2), 120);
        assert_eq!(cols(ZoomLevel::Z3), 120);
    }

    #[test]
    fn zooming_out_never_shows_fewer_tiles() {
        for width in [1440.0, 1920.0, 2560.0] {
            let z1 = focus(frame(width), ZoomLevel::Z1).visible_tiles;
            let z2 = focus(frame(width), ZoomLevel::Z2).visible_tiles;
            let z3 = focus(frame(width), ZoomLevel::Z3).visible_tiles;
            assert!(z1 <= z2 && z2 <= z3, "{width}: {z1} {z2} {z3}");
        }
    }

    #[test]
    fn narrow_tile_area_falls_back_to_one_grid_column() {
        let p = focus(frame(1440.0), ZoomLevel::Z3);
        assert_eq!(p.grid_columns, 1);
        assert!(p.tile_columns >= MIN_TILE_COLUMNS);
    }

    #[test]
    fn strip_keeps_full_width_and_takes_height() {
        let p = strip(frame(1920.0), ZoomLevel::Z2);
        assert_eq!(
            p.main_columns,
            focus(frame(1920.0), ZoomLevel::Z0).main_columns
        );
        assert!(p.main_px < 1040.0);
        assert_eq!(p.grid_columns, 3);
    }

    #[test]
    fn tile_font_is_two_points_smaller_with_a_floor() {
        assert_eq!(tile_font_size(13.0), 11.0);
        assert_eq!(tile_font_size(10.5), 8.5);
        assert_eq!(tile_font_size(7.0), MIN_TILE_FONT_SIZE);
    }
}
