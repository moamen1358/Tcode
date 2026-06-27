//! The Conductor: pure logic for coordinating the coding agents Tcode launches
//! across panes — giving each a stable id, and generating the per-session config
//! (awareness hooks + the Codex delegation tool) that lets them see what the others
//! are doing and hand work to each other.
//!
//! No I/O and no GTK here: the `tcode` crate writes these strings to a session "bus"
//! directory and points each agent at it via launch flags + env vars, so nothing is
//! ever written into the user's real `~/.claude`/`~/.codex` or their repo.

use crate::agents::Agent;
use serde::Deserialize;

/// Stable, human-readable ids for the agents in a session, numbered per kind in pane
/// order — e.g. `[Claude, Claude, Codex, Hermes]` → `claude-1, claude-2, codex-1, hermes-1`.
/// These are what the Mission Control board and the awareness ledger display.
pub fn agent_ids(layout: &[Agent]) -> Vec<String> {
    let (mut c, mut x, mut h) = (0u32, 0u32, 0u32);
    layout
        .iter()
        .map(|a| match a {
            Agent::Claude => {
                c += 1;
                format!("claude-{c}")
            }
            Agent::Codex => {
                x += 1;
                format!("codex-{x}")
            }
            Agent::Hermes => {
                h += 1;
                format!("hermes-{h}")
            }
        })
        .collect()
}

/// How to wire one agent into the session bus: extra environment (its identity + the
/// bus location) and extra CLI flags appended to its launch command.
pub struct Wiring {
    /// Extra environment for the spawned agent.
    pub env: Vec<(String, String)>,
    /// Extra CLI flags to append to the launch command (already shell-quoted). Empty
    /// for agents wired purely via env (Codex/Hermes in Phase 1).
    pub extra_args: String,
}

/// Compute the wiring for `agent` (identified as `agent_id`) against the bus rooted at
/// the absolute path `bus_dir` (which the `tcode` crate has created and populated).
///
/// Every agent gets identity env (`TCODE_AGENT_ID`, `TCODE_BUS_DIR`). Claude also gets
/// `--settings` (the awareness hooks) and `--mcp-config` (the `codex` delegation tool);
/// Codex/Hermes awareness is wired separately (Phase 1b) since they don't take those flags.
pub fn wiring_for(agent: Agent, agent_id: &str, bus_dir: &str) -> Wiring {
    let env = vec![
        ("TCODE_AGENT_ID".to_string(), agent_id.to_string()),
        ("TCODE_BUS_DIR".to_string(), bus_dir.to_string()),
    ];
    let extra_args = match agent {
        Agent::Claude => format!(
            " --settings {} --mcp-config {}",
            sh_quote(&format!("{bus_dir}/claude-settings.json")),
            sh_quote(&format!("{bus_dir}/codex-mcp.json")),
        ),
        Agent::Codex | Agent::Hermes => String::new(),
    };
    Wiring { env, extra_args }
}

/// PostToolUse hook (Claude/Codex share the contract): append this agent's file edits
/// to the session ledger as one JSON line each. Reads identity + bus from the env.
pub fn record_hook_script() -> &'static str {
    r#"#!/usr/bin/env bash
# Tcode Conductor — PostToolUse hook: record this agent's file edits to the session bus.
set -euo pipefail
input="$(cat)"
[ -n "${TCODE_BUS_DIR:-}" ] || exit 0
file="$(printf '%s' "$input" | jq -r '.tool_input.file_path // .tool_input.path // empty')"
[ -n "$file" ] || exit 0
jq -nc --arg ts "$(date -Iseconds)" --arg agent "${TCODE_AGENT_ID:-unknown}" --arg file "$file" \
  '{ts:$ts, agent:$agent, event:"edit", file:$file, base:($file|gsub("^.*/";""))}' \
  >> "$TCODE_BUS_DIR/events.jsonl"
"#
}

/// UserPromptSubmit hook: inject the OTHER agents' recent activity into this agent's
/// context every turn, so awareness is automatic (no reliance on the model looking).
pub fn inject_hook_script() -> &'static str {
    r#"#!/usr/bin/env bash
# Tcode Conductor — UserPromptSubmit hook: inject other agents' recent activity.
set -euo pipefail
input="$(cat)"
[ -n "${TCODE_BUS_DIR:-}" ] || exit 0
log="$TCODE_BUS_DIR/events.jsonl"
[ -f "$log" ] || exit 0
me="${TCODE_AGENT_ID:-unknown}"
recent="$(jq -rc --arg me "$me" \
  'select(.agent != $me) | "\(.ts[11:19])  [\(.agent)] \(.event) \(.base)"' \
  "$log" 2>/dev/null | tail -8)"
[ -n "$recent" ] || exit 0
ctx="⚡ Crew activity — other agents in this workspace:
$recent
Coordinate: avoid editing a file another agent just touched without checking."
jq -nc --arg c "$ctx" \
  '{hookSpecificOutput:{hookEventName:"UserPromptSubmit",additionalContext:$c}}'
"#
}

/// Claude `--settings` JSON wiring the two hooks at their paths under `bus_dir`.
pub fn claude_settings_json(bus_dir: &str) -> String {
    let rec = json_escape(&format!("{bus_dir}/record.sh"));
    let inj = json_escape(&format!("{bus_dir}/inject.sh"));
    format!(
        r#"{{
  "hooks": {{
    "PostToolUse": [{{ "matcher": "Edit|Write", "hooks": [{{ "type": "command", "command": "{rec}" }}] }}],
    "UserPromptSubmit": [{{ "hooks": [{{ "type": "command", "command": "{inj}" }}] }}]
  }}
}}
"#
    )
}

/// Claude `--mcp-config` JSON exposing Codex as the `codex` delegation tool, so a
/// Claude pane can hand a task to Codex (`codex mcp-server` over stdio).
pub fn codex_mcp_json() -> String {
    "{ \"mcpServers\": { \"codex\": { \"command\": \"codex\", \"args\": [\"mcp-server\"] } } }\n"
        .to_string()
}

/// POSIX single-quote a string for safe interpolation into a shell command line
/// (the launch command is fed into the pane's shell). Embedded `'` → `'\''`.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Minimal JSON string-body escape (quotes + backslashes) for embedding a filesystem
/// path into the settings JSON without pulling in a JSON dependency.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out
}

/// One recorded activity line from the session ledger (`events.jsonl`), matching the
/// JSON written by `record_hook_script`. Every field defaults so a partial or
/// future-extended record still parses instead of dropping the whole line.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Event {
    /// ISO-8601 timestamp the edit was recorded.
    #[serde(default)]
    pub ts: String,
    /// The acting agent's id (e.g. `claude-1`).
    #[serde(default)]
    pub agent: String,
    /// The activity kind — `"edit"` for now (room for `"read"`, `"run"`, …).
    #[serde(default)]
    pub event: String,
    /// Absolute path the agent touched.
    #[serde(default)]
    pub file: String,
    /// The path's basename, as recorded by the hook (we re-derive it if absent).
    #[serde(default)]
    pub base: String,
}

/// What one agent is up to, aggregated from its events: the file it last touched,
/// how many edits it has made, and when it was last active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentStatus {
    pub agent: String,
    pub last_base: String,
    pub last_file: String,
    pub edits: usize,
    pub last_ts: String,
}

/// A file touched by more than one agent — a coordination hot spot Mission Control
/// flags so two agents don't unknowingly clobber each other's work. `agents` is in
/// first-seen order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub base: String,
    pub agents: Vec<String>,
}

/// The Mission Control board: per-agent status (sorted by id) plus any multi-agent
/// file conflicts, derived purely from the event log.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Board {
    pub agents: Vec<AgentStatus>,
    pub conflicts: Vec<Conflict>,
}

/// Parse the session ledger (one JSON object per line) into a board summary. Blank
/// and malformed lines are skipped — the log is append-only and may be read while a
/// hook is mid-write. Agents come out sorted by id; conflicts list every file more
/// than one agent has touched, with the agents in first-seen order.
pub fn parse_board(jsonl: &str) -> Board {
    let events: Vec<Event> = jsonl
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Event>(l).ok())
        .filter(|e| !e.agent.is_empty())
        .collect();

    // Per-agent status: edit count + the latest file/ts (events are in append order,
    // so the last line seen for an agent is its most recent). A BTreeMap keys the
    // output by agent id for a stable, sorted board.
    let mut by_agent: std::collections::BTreeMap<String, AgentStatus> =
        std::collections::BTreeMap::new();
    for e in &events {
        let status = by_agent.entry(e.agent.clone()).or_insert_with(|| AgentStatus {
            agent: e.agent.clone(),
            last_base: String::new(),
            last_file: String::new(),
            edits: 0,
            last_ts: String::new(),
        });
        status.edits += 1;
        status.last_base = display_base(e);
        status.last_file = e.file.clone();
        status.last_ts = e.ts.clone();
    }
    let agents: Vec<AgentStatus> = by_agent.into_values().collect();

    // Conflicts: group edits by file basename, preserving file first-seen order and
    // each file's distinct agents in first-seen order, then keep the multi-agent ones.
    let mut files: Vec<(String, Vec<String>)> = Vec::new();
    for e in &events {
        let base = display_base(e);
        if base.is_empty() {
            continue;
        }
        match files.iter_mut().find(|(b, _)| *b == base) {
            Some((_, agents)) if !agents.contains(&e.agent) => agents.push(e.agent.clone()),
            Some(_) => {}
            None => files.push((base, vec![e.agent.clone()])),
        }
    }
    let conflicts: Vec<Conflict> = files
        .into_iter()
        .filter(|(_, agents)| agents.len() > 1)
        .map(|(base, agents)| Conflict { base, agents })
        .collect();

    Board { agents, conflicts }
}

/// The file label to show for an event: prefer the basename the hook recorded, but
/// derive it from the full path when absent (older or hand-written records).
fn display_base(e: &Event) -> String {
    if !e.base.is_empty() {
        e.base.clone()
    } else {
        e.file.rsplit('/').next().unwrap_or(&e.file).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::layout;

    #[test]
    fn ids_number_per_kind_in_pane_order() {
        assert_eq!(
            agent_ids(&layout(4)),
            vec!["claude-1", "claude-2", "codex-1", "hermes-1"]
        );
        assert_eq!(agent_ids(&layout(1)), vec!["claude-1"]);
        assert_eq!(agent_ids(&[]), Vec::<String>::new());
    }

    #[test]
    fn claude_wiring_has_identity_env_and_injection_flags() {
        let w = wiring_for(Agent::Claude, "claude-1", "/bus/x");
        assert!(w
            .env
            .contains(&("TCODE_AGENT_ID".to_string(), "claude-1".to_string())));
        assert!(w
            .env
            .contains(&("TCODE_BUS_DIR".to_string(), "/bus/x".to_string())));
        assert!(w.extra_args.contains("--settings"));
        assert!(w.extra_args.contains("--mcp-config"));
        assert!(w.extra_args.contains("/bus/x/claude-settings.json"));
        assert!(w.extra_args.contains("/bus/x/codex-mcp.json"));
    }

    #[test]
    fn codex_and_hermes_get_identity_but_no_flags() {
        for (a, id) in [(Agent::Codex, "codex-1"), (Agent::Hermes, "hermes-1")] {
            let w = wiring_for(a, id, "/bus/x");
            assert!(w.extra_args.is_empty());
            assert!(w
                .env
                .contains(&("TCODE_AGENT_ID".to_string(), id.to_string())));
        }
    }

    #[test]
    fn settings_json_points_at_both_hooks() {
        let s = claude_settings_json("/bus/x");
        assert!(s.contains("/bus/x/record.sh"));
        assert!(s.contains("/bus/x/inject.sh"));
        assert!(s.contains("PostToolUse"));
        assert!(s.contains("UserPromptSubmit"));
        assert!(s.contains("Edit|Write"));
    }

    #[test]
    fn mcp_json_exposes_codex_server() {
        let s = codex_mcp_json();
        assert!(s.contains("\"codex\""));
        assert!(s.contains("mcp-server"));
    }

    #[test]
    fn hook_scripts_use_the_bus_env() {
        assert!(record_hook_script().contains("TCODE_BUS_DIR"));
        assert!(record_hook_script().contains("events.jsonl"));
        assert!(inject_hook_script().contains("additionalContext"));
        assert!(inject_hook_script().contains("TCODE_AGENT_ID"));
    }

    #[test]
    fn sh_quote_wraps_and_escapes() {
        assert_eq!(sh_quote("/a b/c"), "'/a b/c'");
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn parse_board_empty_is_empty() {
        let b = parse_board("");
        assert!(b.agents.is_empty());
        assert!(b.conflicts.is_empty());
    }

    #[test]
    fn parse_board_aggregates_per_agent_and_flags_conflicts() {
        let log = concat!(
            r#"{"ts":"2026-06-27T10:00:01+00:00","agent":"claude-1","event":"edit","file":"/p/a.rs","base":"a.rs"}"#,
            "\n",
            r#"{"ts":"2026-06-27T10:00:05+00:00","agent":"claude-1","event":"edit","file":"/p/b.rs","base":"b.rs"}"#,
            "\n",
            r#"{"ts":"2026-06-27T10:00:09+00:00","agent":"codex-1","event":"edit","file":"/p/a.rs","base":"a.rs"}"#,
            "\n",
        );
        let b = parse_board(log);
        // Two agents, sorted by id; edit counts and latest-file tracked per agent.
        assert_eq!(b.agents.len(), 2);
        assert_eq!(b.agents[0].agent, "claude-1");
        assert_eq!(b.agents[0].edits, 2);
        assert_eq!(b.agents[0].last_base, "b.rs"); // the later line wins
        assert_eq!(b.agents[0].last_ts, "2026-06-27T10:00:05+00:00");
        assert_eq!(b.agents[1].agent, "codex-1");
        assert_eq!(b.agents[1].edits, 1);
        // a.rs was touched by both agents -> a conflict, agents in first-seen order.
        assert_eq!(b.conflicts.len(), 1);
        assert_eq!(b.conflicts[0].base, "a.rs");
        assert_eq!(b.conflicts[0].agents, vec!["claude-1", "codex-1"]);
    }

    #[test]
    fn parse_board_skips_blank_and_malformed_lines() {
        let log = concat!(
            "\n",
            "not json at all\n",
            r#"{"ts":"t","agent":"claude-1","event":"edit","file":"/p/x.rs","base":"x.rs"}"#,
            "\n",
            "{ broken json\n",
        );
        let b = parse_board(log);
        assert_eq!(b.agents.len(), 1);
        assert_eq!(b.agents[0].agent, "claude-1");
        assert_eq!(b.agents[0].edits, 1);
        assert!(b.conflicts.is_empty());
    }

    #[test]
    fn parse_board_derives_base_when_missing() {
        let log = r#"{"ts":"t","agent":"claude-1","event":"edit","file":"/deep/path/z.rs"}"#;
        let b = parse_board(log);
        assert_eq!(b.agents[0].last_base, "z.rs");
    }
}
