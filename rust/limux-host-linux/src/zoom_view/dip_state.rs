//! The state file that the /dip Claude Code mod writes for one limux
//! terminal surface. Limux renders it; it never parses plans itself.
//!
//! Path: `$XDG_RUNTIME_DIR/limux/dip/<LIMUX_SURFACE_ID>.json`, where the
//! surface id is `<pane_id>:<tab_id>`. The mod writes a temp file and
//! renames it, so a reader never sees half a file. While the session
//! lives, the mod rewrites the file at least once per `heartbeatMs`.
//!
//! Schema version 1:
//!
//! ```json
//! {
//!   "version": 1,
//!   "updatedMs": 1763830000000,
//!   "heartbeatMs": 5000,
//!   "plans": [ { "id": "LMX-42", "title": "Zoom view", "active": true } ],
//!   "plan": { "id": "LMX-42", "title": "Zoom view", "nodes": [
//!     { "id": "p2", "label": "Phase 2", "kind": "phase", "status": "active",
//!       "run": null, "children": [ ... ] } ] },
//!   "runs": [ { "id": "p2a", "name": "codex · p2a", "model": "gpt-5",
//!     "kind": "codex", "parent": null, "status": "running",
//!     "startedMs": 1763829990000, "endedMs": null,
//!     "follow": ["exec-tmux.sh", "follow", "/…/p2a-stream.jsonl"],
//!     "chat": ["exec-tmux.sh", "chat", "/…/p2a-stream.jsonl", "--launch", "17"] } ]
//! }
//! ```
//!
//! `chat` is null when the run cannot be taken over (a native subagent,
//! or a host without memcap scopes); its promoted tab then only follows.
//!
//! The plan picker writes its choice back to
//! `<LIMUX_SURFACE_ID>.select.json` (`{"plan": "<id>"}`) next to the state
//! file; the mod reads it on its next poll.

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const SCHEMA_VERSION: u32 = 1;
const DEFAULT_HEARTBEAT_MS: u64 = 5_000;
/// A state file older than this many heartbeats belongs to a dead session.
const STALE_HEARTBEATS: u64 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunStatus {
    Running,
    Done,
    Failed,
    Limit,
    Stopped,
    TakenOver,
    #[serde(other)]
    Unknown,
}

impl RunStatus {
    pub fn is_running(self) -> bool {
        self == Self::Running
    }

    /// One glyph for the tile title and the rail.
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Running => "●",
            Self::Done => "✓",
            Self::Failed => "✗",
            Self::Limit => "⏸",
            Self::Stopped => "■",
            Self::TakenOver => "⇄",
            Self::Unknown => "?",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NodeStatus {
    Todo,
    Active,
    Done,
    Failed,
    Blocked,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    /// The id of the run that started this one; null for a top-level run.
    #[serde(default)]
    pub parent: Option<String>,
    pub status: RunStatus,
    #[serde(default)]
    pub started_ms: Option<u64>,
    #[serde(default)]
    pub ended_ms: Option<u64>,
    /// The argv of the read-only live view (the tile command).
    pub follow: Vec<String>,
    /// The argv of the view that can take the run over.
    #[serde(default)]
    pub chat: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PlanNode {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub kind: Option<String>,
    pub status: NodeStatus,
    /// The run that works on this node, if any.
    #[serde(default)]
    pub run: Option<String>,
    #[serde(default)]
    pub children: Vec<PlanNode>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Plan {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub nodes: Vec<PlanNode>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PlanRef {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DipState {
    pub version: u32,
    pub updated_ms: u64,
    #[serde(default)]
    pub heartbeat_ms: Option<u64>,
    #[serde(default)]
    pub plans: Vec<PlanRef>,
    #[serde(default)]
    pub plan: Option<Plan>,
    #[serde(default)]
    pub runs: Vec<Run>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    Json(String),
    Version(u32),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(err) => write!(f, "dip state is not valid: {err}"),
            Self::Version(v) => write!(f, "dip state version {v} is not supported"),
        }
    }
}

pub fn parse(raw: &str) -> Result<DipState, ParseError> {
    // Read the version first, so a newer schema gives a clear error.
    let probe: serde_json::Value =
        serde_json::from_str(raw).map_err(|err| ParseError::Json(err.to_string()))?;
    let version = probe.get("version").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    if version != SCHEMA_VERSION {
        return Err(ParseError::Version(version));
    }
    serde_json::from_value(probe).map_err(|err| ParseError::Json(err.to_string()))
}

impl DipState {
    /// True when the mod stopped its heartbeat: the session ended.
    pub fn is_stale(&self, now_ms: u64) -> bool {
        let heartbeat = self.heartbeat_ms.unwrap_or(DEFAULT_HEARTBEAT_MS).max(1);
        now_ms.saturating_sub(self.updated_ms) > heartbeat * STALE_HEARTBEATS
    }

    /// True when the rail has something to show (decision 7: no plan and
    /// no subagents hides the rail).
    pub fn has_content(&self) -> bool {
        self.plan.as_ref().is_some_and(|p| !p.nodes.is_empty()) || !self.runs.is_empty()
    }

    /// The runs that get a tile: running first, then the finished ones
    /// when `show_finished` is on. The mod's order is kept inside each group.
    pub fn tile_runs(&self, show_finished: bool) -> Vec<&Run> {
        let running = self.runs.iter().filter(|r| r.status.is_running());
        let finished = self
            .runs
            .iter()
            .filter(|r| !r.status.is_running())
            .filter(|_| show_finished);
        running.chain(finished).collect()
    }

    pub fn run(&self, id: &str) -> Option<&Run> {
        self.runs.iter().find(|r| r.id == id)
    }

    /// The parent chain depth of a run (0 for a top-level run). It stops
    /// at a cycle or a missing parent.
    pub fn depth(&self, run: &Run) -> usize {
        let mut depth = 0;
        let mut current = run.parent.as_deref();
        while let Some(parent) = current {
            if depth >= self.runs.len() {
                break;
            }
            let Some(next) = self.run(parent) else { break };
            depth += 1;
            current = next.parent.as_deref();
        }
        depth
    }
}

impl PlanNode {
    /// The rail's hide-done filter: false when this node and all its
    /// children are done.
    pub fn has_open_work(&self) -> bool {
        self.status != NodeStatus::Done || self.children.iter().any(Self::has_open_work)
    }
}

/// The folder for all state files: `$XDG_RUNTIME_DIR/limux/dip`.
pub fn state_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(|dir| state_dir_in(Path::new(&dir)))
}

pub fn state_dir_in(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("limux").join("dip")
}

/// The file name for one surface. The `:` in the surface id stays; it is
/// a valid file-name character on Linux.
pub fn state_file_name(surface_id: &str) -> String {
    format!("{}.json", sanitize(surface_id))
}

pub fn select_file_name(surface_id: &str) -> String {
    format!("{}.select.json", sanitize(surface_id))
}

/// A shell command line that runs `argv` in place of the shell (Ghostty
/// runs a surface command through the shell). None for an empty argv.
pub fn shell_command(argv: &[String]) -> Option<String> {
    if argv.first().is_none_or(|arg| arg.is_empty()) {
        return None;
    }
    let quoted: Vec<String> = argv.iter().map(|arg| shell_quote(arg)).collect();
    Some(format!("exec {}", quoted.join(" ")))
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&byte))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn sanitize(surface_id: &str) -> String {
    surface_id.replace(['/', '\0'], "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "version": 1,
      "updatedMs": 10000,
      "heartbeatMs": 1000,
      "plans": [ { "id": "LMX-42", "title": "Zoom view", "active": true } ],
      "plan": { "id": "LMX-42", "title": "Zoom view", "nodes": [
        { "id": "p1", "label": "Phase 1", "status": "done", "children": [
          { "id": "p1a", "label": "Settings", "status": "done", "run": "a" } ] },
        { "id": "p2", "label": "Phase 2", "status": "active", "children": [
          { "id": "p2a", "label": "Reader", "status": "done" },
          { "id": "p2b", "label": "Rail", "status": "todo", "run": "b" } ] } ] },
      "runs": [
        { "id": "a", "name": "codex · p1a", "status": "done",
          "follow": ["exec-tmux.sh", "follow", "/s/a.jsonl"] },
        { "id": "b", "name": "codex · p2b", "status": "running",
          "follow": ["exec-tmux.sh", "follow", "/s/b.jsonl"],
          "chat": ["exec-tmux.sh", "chat", "/s/b.jsonl", "--launch", "7"] },
        { "id": "c", "name": "runner · clippy", "parent": "b", "status": "taken-over",
          "follow": ["exec-tmux.sh", "follow", "/s/c.jsonl"] },
        { "id": "d", "name": "new", "status": "paused-by-future-mod",
          "follow": [] }
      ]
    }"#;

    #[test]
    fn parses_the_documented_schema() {
        let state = parse(SAMPLE).unwrap();
        assert_eq!(state.plans.len(), 1);
        assert_eq!(state.plan.as_ref().unwrap().nodes.len(), 2);
        assert_eq!(state.runs[1].chat.as_ref().unwrap()[4], "7");
        assert_eq!(state.runs[2].status, RunStatus::TakenOver);
        assert_eq!(state.runs[3].status, RunStatus::Unknown);
    }

    #[test]
    fn rejects_other_versions_and_bad_json() {
        assert_eq!(
            parse(r#"{"version": 2, "updatedMs": 0}"#),
            Err(ParseError::Version(2))
        );
        assert!(matches!(parse("{"), Err(ParseError::Json(_))));
    }

    #[test]
    fn running_tiles_come_first_and_finished_tiles_follow_the_setting() {
        let state = parse(SAMPLE).unwrap();
        let ids = |v: Vec<&Run>| v.iter().map(|r| r.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(state.tile_runs(true)), ["b", "a", "c", "d"]);
        assert_eq!(ids(state.tile_runs(false)), ["b"]);
    }

    #[test]
    fn staleness_uses_three_heartbeats() {
        let state = parse(SAMPLE).unwrap();
        assert!(!state.is_stale(13_000));
        assert!(state.is_stale(13_001));
        // A clock that runs behind the writer never counts as stale.
        assert!(!state.is_stale(0));
    }

    #[test]
    fn depth_follows_parents_and_survives_cycles() {
        let mut state = parse(SAMPLE).unwrap();
        assert_eq!(state.depth(&state.runs[2]), 1);
        assert_eq!(state.depth(&state.runs[0]), 0);
        state.runs[1].parent = Some("c".into());
        let run = state.runs[2].clone();
        assert!(state.depth(&run) <= state.runs.len());
    }

    #[test]
    fn hide_done_keeps_a_phase_with_open_children() {
        let state = parse(SAMPLE).unwrap();
        let nodes = &state.plan.as_ref().unwrap().nodes;
        assert!(!nodes[0].has_open_work());
        assert!(nodes[1].has_open_work());
    }

    #[test]
    fn rail_content_rule() {
        let empty = parse(r#"{"version": 1, "updatedMs": 0}"#).unwrap();
        assert!(!empty.has_content());
        assert!(parse(SAMPLE).unwrap().has_content());
    }

    #[test]
    fn shell_command_quotes_every_argument() {
        let argv: Vec<String> = ["/a b/exec-tmux.sh", "follow", "it's", ""]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            shell_command(&argv).as_deref(),
            Some("exec '/a b/exec-tmux.sh' follow 'it'\\''s' ''")
        );
        assert_eq!(shell_command(&[]), None);
    }

    #[test]
    fn file_names_follow_the_surface_id() {
        let name = state_file_name("3:5b0c-uuid");
        assert_eq!(name, "3:5b0c-uuid.json");
        assert_eq!(select_file_name("3:x"), "3:x.select.json");
        assert_eq!(
            state_dir_in(Path::new("/run/user/1000")),
            PathBuf::from("/run/user/1000/limux/dip")
        );
    }
}
