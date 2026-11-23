# /dip plan rail and zoom-out subagent view

Branch: `feat/dip-and-zoom-session`. Linux first.

Ctrl+MouseWheel over a terminal zooms out to live tiles of the subagents
that the /dip Claude Code mod started from that terminal. The font never
changes: a zoom step gives the main terminal fewer columns and the tiles
fewer rows. A limux-native plan rail shows the plan tree next to them.

## Decisions

| Topic | Decision |
|---|---|
| Default layout | Focus + tiles. Grid, Tree and Strip are options in Settings → Zoom view. |
| Main terminal | Never narrower than `main_min_columns` (default 120). |
| Plan rail | On every zoom level. Open (240 px by default; drag its left edge between 180 and 640 px, saved as `plan_rail_width`), collapsed (44 px strip with one glyph and count per run status; the plan title and active task are in its tooltip; a click anywhere on the strip opens the rail) or closed. The rail lists the same runs as the /dip pane: every running run, and per group (subagents, shells) the eight latest finished runs that ended less than one minute ago (30 minutes for a run at its limit). A closed rail leaves a "Show plan" button in the header of the session pane. |
| Splits | Hidden on zoom-out; they come back at Z0. |
| Zoom-in | One level in; from Z1 back to Z0. The main terminal gets the keyboard focus; a selected tile does not matter. |
| Double-click on a tile | Opens the run as a new tab in the same pane; the zoom level stays. The tab runs the run's `chat` argv (or `follow` when `chat` is null). |
| Takeover | Read-only until Enter on a typed line; Esc returns to read-only. This is `exec-tmux.sh chat` behaviour in the mod, not limux. |
| Overflow | The tile area scrolls and shows "N more subagents below · scroll". |
| Data source | Limux reads only the mod's state file. It never parses plans. |
| Scope | Only the runs of the session under the pointer. |
| Tiles | The same runs as the rail and the /dip pane: running runs, and finished runs for one minute (30 minutes at a usage limit) as dimmed tiles while `show_finished` is on. The tile terminals use a font 2 points smaller than the terminal font (6 points at least). |
| No plan and no runs | The rail hides itself. |
| Rail v1 | Tree, "Hide done", plan picker. A click on a run selects its tile. Rows use the colours of the /dip pane (running and active yellow, done green, failed red, limit and blocked magenta, stopped grey, taken over cyan; to do dim), and each run has the pane's summary line (`summary` in the state file) below it. At Z0, with subagents, a "Subagents" button (a grid icon on the strip) zooms out to Z1. |
| Top bar | Buttons Z0–Z3 go straight to a level. The selected level has an amber background. The zoom CSS loads one above user priority, so a theme's `~/.config/gtk-4.0/gtk.css` cannot hide it. A deeper level zooms out of the focused pane. The tooltip names the shortcuts and Ctrl+wheel. |
| Top bar filters | "Hide done" hides finished subagents and shells; "Hide shells" hides background shells, so only subagents show. The filter is per workspace and applies to the tiles and the rail. A hidden tile stops its terminal; it starts again when the filter turns off. |
| Cards | The main terminal and each tile are rounded cards with a title: status glyph, name, detail (model, or tab · plan · cols × rows) and the elapsed time. Tiles have no other controls. The pane header hides while zoomed, unless the pane has more than one tab, and comes back at Z0. |
| Shortcuts | Ctrl+Alt+Z zooms out of the focused pane, Ctrl+Alt+Shift+Z zooms in (like Ctrl+wheel). Keys `zoom_view_out` and `zoom_view_in` in `shortcuts.json`. Ctrl+Alt+Shift+Up/Down are GNOME's move-to-workspace keys, so the shortcuts do not use them. |

## Settings (`settings.json`)

```jsonc
"zoom_view": {
  "layout": "focus",          // focus | grid | tree | strip
  "main_min_columns": 120,    // 80..=300
  "plan_rail": "open",        // open | collapsed | closed
  "plan_rail_width": 240,     // 180-640 px; a drag of the rail edge saves it
  "ctrl_wheel": true,         // false: Ctrl+wheel goes to the terminal
  "show_finished": true
}
```

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
returns `{workspace_id, surface_id, level, rail, rail_active,
rail_visible}`. The mod can use it to know when the user looks at the
tiles.

## Code map

| Part | Where |
|---|---|
| Settings type | `zoom_view/config.rs` |
| State file, schema, quoting | `zoom_view/dip_state.rs` |
| Layout math (pure) | `zoom_view/geometry.rs` |
| Plan rail widget | `zoom_view/rail.rs` |
| Zoom view per workspace | `zoom_view/view.rs` |
| Zoom frame in the split tree | `split_tree.rs` (`enter_zoom_frame`, `leave_zoom_frame`, `drop_zoom_frame`) |
| Ctrl+wheel, "Show plan" and level actions | `window.rs` (`install_zoom_controls`, `zoom_scroll`, `zoom_shortcut`) |
| Top-bar level buttons | `zoom_view/bar.rs`; `window.rs` (`sync_zoom_bar`) |
| Tab for a double-clicked tile | `pane.rs` (`add_command_tab_to_pane`) |
| Settings page | `settings_editor.rs` (`build_zoom_view_page`) |
| CLI | `limux-cli` `zoom-state`; method spec `surface.zoom_state` in `limux-protocol/src/methods.rs` |

## Status

Done and checked in a private Xvfb smoke run (fake state file, six
colour runs):

- Z0 → Z3 and back, split hidden and restored, the 120-column floor.
- Tile selection; a double-click opens the run as a tab.
- Rail collapse, close, and reopen through "Show plan".
- Focus, Grid, Tree and Strip layouts; the scroll indicator.
- The top-bar buttons (Z3, then Z0), both
  shortcuts and the tooltip hint.

Open in limux:

- Tiles that scroll out of view keep their terminals mapped (no
  occlusion yet).
- No "N splits hidden" chip.
- The level buttons live in the top bar only. With the top bar hidden,
  only the shortcuts and Ctrl+wheel zoom.
- A session restore brings back a tab of a double-clicked tile as a
  plain terminal.
- At 1920 px with the 120-column floor, Grid has room for one tile
  column only, so it looks like Focus. It differs on wider windows.

Open in the mod (`~/projects`, needs separate approval):

- Write the state file and its heartbeat.
- Read `<surface>.select.json`.
- Esc in `exec-tmux.sh chat` to go back to read-only (today Esc is
  ignored; an empty Enter or Ctrl-C returns to watching).
