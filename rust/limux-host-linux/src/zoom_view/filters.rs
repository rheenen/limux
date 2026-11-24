//! The subagent filters "Hide done" and "Hide shells" at the top of the
//! subagent pane. Pure text and rules; the GTK part is in `view`.

use super::dip_state::DipState;

/// The session has something for the rail and the tiles: a plan or runs.
pub fn has_content(state: Option<&DipState>) -> bool {
    state.is_some_and(DipState::has_content)
}

pub const HIDE_DONE_LABEL: &str = "Hide done";
pub const HIDE_SHELLS_LABEL: &str = "Hide shells";

pub fn hide_done_hint(on: bool) -> &'static str {
    if on {
        "Finished subagents and shells are hidden. Click to show them."
    } else {
        "Hide finished subagents and shells"
    }
}

pub fn hide_shells_hint(on: bool) -> &'static str {
    if on {
        "Background shells are hidden; only subagents show. Click to show them."
    } else {
        "Hide background shells; show only subagents"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_without_runs_has_content() {
        let plan_only = super::super::dip_state::parse(
            r#"{ "version": 1, "updatedMs": 0,
                 "plan": { "id": "P", "title": "Plan", "nodes": [
                   { "id": "p0", "label": "Phase 0", "status": "todo" } ] } }"#,
        )
        .unwrap();
        assert!(plan_only.runs.is_empty());
        assert!(has_content(Some(&plan_only)));
        let empty = super::super::dip_state::parse(r#"{ "version": 1, "updatedMs": 0 }"#).unwrap();
        assert!(!has_content(Some(&empty)));
        assert!(!has_content(None));
    }

    #[test]
    fn filter_hints_say_what_a_click_does() {
        assert!(hide_done_hint(false).starts_with("Hide finished"));
        assert!(hide_done_hint(true).contains("Click to show"));
        assert!(hide_shells_hint(false).contains("only subagents"));
        assert!(hide_shells_hint(true).contains("Click to show"));
    }
}
