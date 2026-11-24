//! Pure layout math for the subagent view. The font never changes: the
//! tiles only give the terminals fewer columns and themselves fewer
//! rows. The GTK side applies the result once per change, so every PTY
//! resizes once, not once per animation frame.

use super::config::ZoomLayout;

/// The share of the width that the terminals (the splits) get while the
/// subagents show. Picked so that a 1920 px window gives about 150
/// columns.
pub const DEFAULT_MAIN_SHARE: f64 = 0.565;
/// A tile is never lower than this many text rows; below it the tile
/// list scrolls.
pub const MIN_TILE_ROWS: u32 = 6;
/// A tile is never higher than this many text rows.
pub const MAX_TILE_ROWS: u32 = 40;
/// Another grid column comes only when one column would give the tiles
/// fewer rows than this.
pub const COMFORT_TILE_ROWS: u32 = 12;
/// The focus layout never uses more tile columns than this.
pub const MAX_GRID_COLUMNS: u32 = 3;

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
    /// The share of the width that the user dragged the terminals to;
    /// None keeps `DEFAULT_MAIN_SHARE`.
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

/// The text rows of each tile when `per_column` tiles share `height`.
fn tile_rows_for(height: f64, cell_height: f64, per_column: u32) -> u32 {
    if per_column == 0 || cell_height <= 0.0 {
        return MAX_TILE_ROWS;
    }
    let count = f64::from(per_column);
    let each = (height - TILE_GAP * count) / count - TILE_CHROME_HEIGHT;
    ((each / cell_height).floor().max(0.0) as u32).clamp(MIN_TILE_ROWS, MAX_TILE_ROWS)
}

/// The text columns of each tile when `grid_columns` tiles share
/// `tile_area` side by side.
fn tile_columns_for(tile_area: f64, cell_width: f64, grid_columns: u32) -> u32 {
    let count = grid_columns.max(1);
    let width = (tile_area - TILE_GAP * f64::from(count - 1)) / f64::from(count);
    cols_for(width - TILE_CHROME_WIDTH, cell_width)
}

/// The focus layout (default): the terminals at the left, full height;
/// the rail; `tiles` subagent tiles at the right. The terminals never get
/// narrower than `main_min_columns`; at the floor the tiles get less
/// space. A width dragged with the main handle may go lower.
///
/// The tiles fill the height: one tile gets it all, more tiles share it,
/// down to `MIN_TILE_ROWS` each. When one column would give a tile fewer
/// than `COMFORT_TILE_ROWS` rows, a second (and third) column comes, as
/// long as each tile keeps `MIN_TILE_COLUMNS`.
pub fn focus(frame: Frame, tiles: u32) -> Placement {
    let available = (frame.width - frame.rail_width).max(0.0);
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
        None => frame.width * DEFAULT_MAIN_SHARE,
    };
    let main_px = target.max(floor_px).min(available);
    let tile_area = (available - main_px).max(0.0);

    let widest = (1..=MAX_GRID_COLUMNS)
        .rev()
        .find(|columns| tile_columns_for(tile_area, frame.cell_width, *columns) >= MIN_TILE_COLUMNS)
        .unwrap_or(1);
    let count = tiles.max(1);
    let mut grid_columns = 1;
    while grid_columns < widest.min(count) {
        let per_column = count.div_ceil(grid_columns);
        if tile_rows_for(frame.height, frame.cell_height, per_column) >= COMFORT_TILE_ROWS {
            break;
        }
        grid_columns += 1;
    }
    let per_column = count.div_ceil(grid_columns);
    let tile_rows = tile_rows_for(frame.height, frame.cell_height, per_column);
    let tile_columns = tile_columns_for(tile_area, frame.cell_width, grid_columns);
    let tile_height = f64::from(tile_rows) * frame.cell_height + TILE_CHROME_HEIGHT + TILE_GAP;
    let fit = (frame.height / tile_height).floor().max(0.0) as u32;

    Placement {
        main_px,
        main_columns: cols_for(main_px - TILE_CHROME_WIDTH, frame.cell_width),
        grid_columns,
        tile_rows,
        tile_columns,
        visible_tiles: fit * grid_columns,
    }
}

/// The narrowest tile area: one tile of `MIN_TILE_COLUMNS`, with the
/// tile box margins.
pub fn min_tile_area(cell_width: f64) -> f64 {
    f64::from(MIN_TILE_COLUMNS) * cell_width + TILE_CHROME_WIDTH + 2.0 * TILE_GAP
}

/// The widest the rail may get beside terminals `main_px` wide in a
/// frame `width` wide: the tiles keep the room of one readable tile. A
/// drag of the rail's handle then moves only the edge between the rail
/// and the tiles, and the terminals keep their width.
pub fn max_rail_width(width: f64, main_px: f64, cell_width: f64) -> f64 {
    (width - main_px - min_tile_area(cell_width)).max(0.0)
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

/// The share of the height that the strip of tiles takes at most.
const STRIP_SHARE: f64 = 0.35;
/// The strip never shows more tiles side by side than this.
const MAX_STRIP_TILES: u32 = 4;

/// The strip layout (option D): the terminals at full width on top, one
/// row of tiles below them. `main_px` is the height of the terminals.
pub fn strip(frame: Frame, tiles: u32) -> Placement {
    let width = (frame.width - frame.rail_width).max(0.0);
    let main_columns = cols_for(width - TILE_CHROME_WIDTH, frame.cell_width);
    let tile_rows = tile_rows_for(frame.height * STRIP_SHARE, frame.cell_height, 1);
    let strip_px = f64::from(tile_rows) * frame.cell_height + TILE_CHROME_HEIGHT + TILE_GAP;
    let min_tile_px = f64::from(MIN_TILE_COLUMNS) * frame.cell_width + TILE_CHROME_WIDTH;
    let fit = ((width + TILE_GAP) / (min_tile_px + TILE_GAP))
        .floor()
        .max(1.0) as u32;
    let grid_columns = tiles.clamp(1, MAX_STRIP_TILES).min(fit);
    Placement {
        main_px: (frame.height - strip_px).max(0.0),
        main_columns,
        grid_columns,
        tile_rows,
        tile_columns: tile_columns_for(width, frame.cell_width, grid_columns),
        visible_tiles: grid_columns,
    }
}

/// The placement for any layout. Grid (B) and tree (C) use the focus
/// math for the terminals and differ only in how the GTK side orders
/// and spans the tiles.
pub fn place(layout: ZoomLayout, frame: Frame, tiles: u32) -> Placement {
    match layout {
        ZoomLayout::Strip => strip(frame, tiles),
        ZoomLayout::Focus | ZoomLayout::Grid | ZoomLayout::Tree => focus(frame, tiles),
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
        let default = focus(frame(1920.0), 1);
        let wider = focus(
            Frame {
                main_share: Some(0.7),
                ..frame(1920.0)
            },
            1,
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
            1,
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
            1,
        );
        assert_eq!(narrow.main_columns, MIN_TILE_COLUMNS);
        assert!(narrow.tile_columns > default.tile_columns);
    }

    #[test]
    fn a_rail_within_its_limit_keeps_the_terminals_width() {
        // The terminals at the tile room cap, as a drag far right leaves
        // them; the share then pins that width.
        let capped = focus(
            Frame {
                main_share: Some(0.9),
                ..frame(1920.0)
            },
            1,
        );
        let pinned = Frame {
            main_share: Some(capped.main_px / 1920.0),
            ..frame(1920.0)
        };
        let limit = max_rail_width(1920.0, capped.main_px, 7.22);
        assert!((limit - 240.0).abs() < 0.01, "{limit}");
        for rail_width in [180.0, 200.0, limit] {
            let p = focus(
                Frame {
                    rail_width,
                    ..pinned
                },
                1,
            );
            assert!((p.main_px - capped.main_px).abs() < 0.01, "{rail_width}");
        }
        // With room to spare, the rail may grow into the tiles.
        let default = focus(frame(1920.0), 1);
        assert!(max_rail_width(1920.0, default.main_px, 7.22) > 240.0);
    }

    #[test]
    fn a_drag_gives_a_clamped_share_of_the_width() {
        assert!((dragged_share(1000.0, 100.0, 2000.0) - 0.55).abs() < 1e-9);
        assert_eq!(dragged_share(1000.0, -2000.0, 2000.0), MIN_MAIN_SHARE);
        assert_eq!(dragged_share(1000.0, 5000.0, 2000.0), MAX_MAIN_SHARE);
        assert_eq!(dragged_share(10.0, 0.0, 0.0), MIN_MAIN_SHARE);
    }

    #[test]
    fn the_terminals_never_go_below_the_floor() {
        for width in [1440.0, 1920.0, 2560.0, 3840.0] {
            for tiles in [1, 4, 12] {
                let p = focus(frame(width), tiles);
                assert!(p.main_columns >= 120, "{width} {tiles}: {}", p.main_columns);
            }
        }
    }

    #[test]
    fn a_1920_window_gives_the_terminals_about_150_columns() {
        let cols = focus(frame(1920.0), 1).main_columns;
        assert!((148..=152).contains(&cols), "{cols}");
    }

    #[test]
    fn one_tile_fills_the_height_and_more_tiles_share_it() {
        let one = focus(frame(1920.0), 1);
        assert_eq!(one.grid_columns, 1);
        // 1040 px high, 17 px rows, 36 px of chrome and gap: 59 rows,
        // capped.
        assert_eq!(one.tile_rows, MAX_TILE_ROWS);
        let three = focus(frame(1920.0), 3);
        assert_eq!(three.grid_columns, 1);
        assert!(three.tile_rows < one.tile_rows);
        assert!(three.tile_rows >= COMFORT_TILE_ROWS);
        assert!(three.visible_tiles >= 3);
    }

    #[test]
    fn many_tiles_get_a_second_column_when_it_stays_readable() {
        let wide = focus(frame(2560.0), 8);
        assert_eq!(wide.grid_columns, 2);
        assert!(wide.tile_columns >= MIN_TILE_COLUMNS);
        // At 1920 px a second column would be too narrow: the tiles get
        // fewer rows instead, and the list scrolls.
        let narrow = focus(frame(1920.0), 8);
        assert_eq!(narrow.grid_columns, 1);
        assert!(narrow.tile_rows >= MIN_TILE_ROWS);
    }

    #[test]
    fn many_tiles_never_go_below_the_minimum_rows() {
        let p = focus(frame(1920.0), 40);
        assert_eq!(p.tile_rows, MIN_TILE_ROWS);
        assert!(p.visible_tiles < 40);
    }

    #[test]
    fn strip_keeps_full_width_and_takes_height() {
        let p = strip(frame(1920.0), 3);
        assert_eq!(
            p.main_columns,
            cols_for(1920.0 - 240.0 - TILE_CHROME_WIDTH, 7.22)
        );
        assert!(p.main_px < 1040.0);
        assert!(p.main_px > 1040.0 * (1.0 - STRIP_SHARE) - 1.0);
        assert_eq!(p.grid_columns, 3);
        assert_eq!(strip(frame(1920.0), 9).grid_columns, MAX_STRIP_TILES);
    }

    #[test]
    fn tile_font_is_two_points_smaller_with_a_floor() {
        assert_eq!(tile_font_size(13.0), 11.0);
        assert_eq!(tile_font_size(10.5), 8.5);
        assert_eq!(tile_font_size(7.0), MIN_TILE_FONT_SIZE);
    }
}
