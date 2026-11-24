# /dip plan rail and subagent view

Branch: `feat/dip-and-zoom-session`. Linux first.

The splits always stay. Beside them is a limux-native plan rail when the
focused terminal's /dip session has a plan or runs. The rail's
"Subagents" button shows the subagents that the /dip Claude Code mod
started from that terminal as live tiles to the right of the rail. The
font never changes: the tiles give the terminals fewer columns.

On 2026-11-24 the zoom levels (Z0–Z3), their top-bar buttons and
Ctrl+mouse wheel were removed: users found the levels confusing.

## Decisions

| Topic | Decision |
|---|---|
| Default layout | Focus + tiles: the splits at the left, the rail, the tiles at the right. Grid, Tree and Strip are options in Settings → Zoom view. |
| Terminals | Every pane is a rounded card; the pane with the keyboard focus has a blue edge. The splits stay visible while the tiles show. The tiles never make the splits narrower than `main_min_columns` (default 120). A drag handle on the splits' right edge (focus and grid layouts) sets their share of the width, saved as `main_share`; a dragged width may go down to 40 columns and always leaves one readable tile column. A double-click on the handle goes back to the default (0.565 of the width). |
| Plan rail | Shows while the session has a plan or runs. Open (240 px by default; drag its edge between 180 and 640 px, saved as `plan_rail_width`), collapsed (44 px strip with one glyph and count per run status; the plan title and active task are in its tooltip; a click anywhere on the strip opens the rail) or closed. The rail lists the same runs as the /dip pane: every running run, and per group (subagents, shells) the eight latest finished runs that ended less than one minute ago (30 minutes for a run at its limit). A closed rail leaves a "Show plan" button in the header of the session pane. |
| Subagents button | "Subagents" shows the tiles; the button then reads "Hide subagents" and hides them again. On the collapsed strip it is an icon. While the tiles show, the rail and the tiles stay on that session; while they are hidden, the rail follows the keyboard focus. |
| Tile size | Dynamic: one tile gets the full height; more tiles share it, down to 6 rows each. When one column gives a tile fewer than 12 rows, a second (or third) column comes, as long as each tile keeps 40 columns. |
| Dividers | The drag handles show a light gray line, also between the rail and the tiles. |
| Subagent pane header | The count of subagents and running runs, then "Hide done", "Hide shells" and a close button at the right. The close button hides the whole subagent pane, like "Hide subagents". Tiles have no close button of their own. |
| Double-click on a tile | Opens the run as a new tab in the session's pane; the tiles stay. The tab runs the run's `chat` argv (or `follow` when `chat` is null). |
| Takeover | Read-only until Enter on a typed line; Esc returns to read-only. This is `exec-tmux.sh chat` behaviour in the mod, not limux. |
| Overflow | The tile area scrolls and shows "N more subagents below · scroll". |
| Data source | Limux reads only the mod's state file. It never parses plans. |
| Scope | Only the runs of the focused terminal's session. |
| Tiles | The same runs as the rail and the /dip pane: running runs, and finished runs for one minute (30 minutes at a usage limit) as dimmed tiles while `show_finished` is on. The tile terminals use a font 2 points smaller than the terminal font (6 points at least). |
| No plan and no runs | The rail hides itself, and the tiles hide. |
| Rail v1 | Tree, "Hide done", plan picker. A click on a run selects its tile. Rows use the colours of the /dip pane (running and active yellow, done green, failed red, limit and blocked magenta, stopped grey, taken over cyan; to do dim), and each run has the pane's summary line (`summary` in the state file) below it. |
| Filters | "Hide done" hides finished subagents and shells; "Hide shells" hides background shells, so only subagents show. They are in the subagent pane header, not in the top bar. Limux remembers them in settings.json (`hide_done`, `hide_shells`) for every workspace and the next start; they apply to the tiles and the rail. A hidden tile stops its terminal; it starts again when the filter turns off. The CSS loads one above user priority, so a theme's `~/.config/gtk-4.0/gtk.css` cannot hide the "on" state. |
| Shortcuts | Ctrl+Alt+Z shows the subagents of the focused terminal, Ctrl+Alt+Shift+Z hides them. Keys `zoom_view_out` and `zoom_view_in` in `shortcuts.json`. |

## Settings (`settings.json`)

```jsonc
"zoom_view": {
  "layout": "focus",          // focus | grid | tree | strip
  "main_min_columns": 120,    // 80..=300
  "plan_rail": "open",        // open | collapsed | closed
  "plan_rail_width": 240,     // 180-640 px; a drag of the rail edge saves it
  "show_finished": true,
  "hide_done": false,         // the subagent pane's filters, remembered
  "hide_shells": false,
  "main_share": 0.6           // optional; a drag of the splits' edge saves it
}
```

An older `main_share` object (`{"z1": …, "z2": …, "z3": …}`) gives its
`z1` value. An old `ctrl_wheel` key is ignored.

## State file contract (mod → limux)

Path: `$XDG_RUNTIME_DIR/limux/dip/<LIMUX_SURFACE_ID>.json`, where the
surface id is `<pane_id>:<tab_id>`. The full schema (version 1) is the
doc comment at the top of
`rust/limux-host-linux/src/zoom_view/dip_state.rs`; that comment is the
source of truth. In short:

- Write a temp file, then rename it.
- Rewrite at least once per `heartbeatMs`. Limux treats a file older than
  3 heartbeats as a dead session.
- `runs[].follow` and `runs[].chat` are argv arrays. Limux runs them
  through the shell as `exec <quoted argv>`.
- `runs[].status`: `running`, `done`, `failed`, `limit`, `stopped`,
  `taken-over`. Unknown values do not break the parse.
- The plan picker writes `<LIMUX_SURFACE_ID>.select.json`
  (`{"plan": "<id>"}`) next to the state file.

`limux zoom-state [--surface ID]` (socket method `surface.zoom_state`)
returns `{workspace_id, surface_id, subagents_shown, rail, rail_active,
rail_visible}`. The mod can use it to know when the user looks at the
tiles.

## Code map

| Part | Where |
|---|---|
| Settings type | `zoom_view/config.rs` |
| State file, schema, quoting | `zoom_view/dip_state.rs` |
| Layout math (pure) | `zoom_view/geometry.rs` |
| Plan rail widget | `zoom_view/rail.rs` |
| Subagent view per workspace | `zoom_view/view.rs` |
| Split tree in the view frame | `split_tree.rs` (`mount_in_frame`) |
| Pane card style | `pane.rs` (`limux-pane-card`); CSS in `zoom_view/view.rs` |
| "Show plan" and shortcut actions | `window.rs` (`install_zoom_controls`, `zoom_shortcut`) |
| Filter text and rules | `zoom_view/filters.rs`; the buttons are in `zoom_view/view.rs` |
| Tab for a double-clicked tile | `pane.rs` (`add_command_tab_to_pane`) |
| Settings page | `settings_editor.rs` (`build_zoom_view_page`) |
| CLI | `limux-cli` `zoom-state`; method spec `surface.zoom_state` in `limux-protocol/src/methods.rs` |

## Status

The first version (zoom levels) was checked in a private Xvfb smoke run
(fake state file, six colour runs). The level-free version of 2026-11-24
passes the unit tests; it is not yet checked in a live session.

Open in limux:

- Tiles that scroll out of view keep their terminals mapped (no
  occlusion yet).
- A session restore brings back a tab of a double-clicked tile as a
  plain terminal.
- At 1920 px with the 120-column floor, Grid has room for one tile
  column only, so it looks like Focus. It differs on wider windows.

Open in the mod (`~/projects`, needs separate approval):

- Write the state file and its heartbeat.
- Read `<surface>.select.json`.
- Esc in `exec-tmux.sh chat` to go back to read-only (today Esc is
  ignored; an empty Enter or Ctrl-C returns to watching).
