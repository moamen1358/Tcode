//! Which AI coding agent each terminal pane auto-launches, decided purely by the
//! pane count. This is the assignment logic only (no GTK, no process spawning), so
//! it's unit-testable; the actual command strings live in `Config::agents` and the
//! spawning lives in the `tcode` crate's pane/grid.

/// A coding agent Tcode can auto-launch in a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Claude,
    Codex,
    Hermes,
}

/// Assign an agent to each of `n` panes, in pane order (index 0 = first/top-left):
/// - `1` → `[Claude]`
/// - `2` → `[Claude, Codex]`
/// - `n ≥ 3` → `[Claude × (n-2), Codex, Hermes]` — Codex second-to-last, Hermes last.
///
/// So Claude always fills the majority, with exactly one Codex and one Hermes once
/// there are three or more panes. `0` yields an empty assignment.
pub fn layout(n: usize) -> Vec<Agent> {
    match n {
        0 => Vec::new(),
        1 => vec![Agent::Claude],
        2 => vec![Agent::Claude, Agent::Codex],
        n => {
            let mut v = vec![Agent::Claude; n - 2];
            v.push(Agent::Codex);
            v.push(Agent::Hermes);
            v
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_empty() {
        assert!(layout(0).is_empty());
    }

    #[test]
    fn one_pane_is_claude_only() {
        assert_eq!(layout(1), vec![Agent::Claude]);
    }

    #[test]
    fn two_panes_are_claude_then_codex() {
        assert_eq!(layout(2), vec![Agent::Claude, Agent::Codex]);
    }

    #[test]
    fn three_panes_are_one_of_each_in_order() {
        assert_eq!(
            layout(3),
            vec![Agent::Claude, Agent::Codex, Agent::Hermes]
        );
    }

    #[test]
    fn many_panes_fill_with_claude_and_keep_one_codex_one_hermes() {
        let l = layout(6);
        assert_eq!(l.len(), 6);
        assert_eq!(l.iter().filter(|a| **a == Agent::Claude).count(), 4);
        assert_eq!(l.iter().filter(|a| **a == Agent::Codex).count(), 1);
        assert_eq!(l.iter().filter(|a| **a == Agent::Hermes).count(), 1);
        // Codex is always second-to-last, Hermes always last.
        assert_eq!(l[l.len() - 2], Agent::Codex);
        assert_eq!(l[l.len() - 1], Agent::Hermes);
    }

    #[test]
    fn at_the_pane_cap_the_mix_still_holds() {
        let l = layout(16);
        assert_eq!(l.len(), 16);
        assert_eq!(l.iter().filter(|a| **a == Agent::Claude).count(), 14);
        assert_eq!(l[14], Agent::Codex);
        assert_eq!(l[15], Agent::Hermes);
    }
}
