//! Pure filtering for the Alt+V clipboard palette: which history rows match a
//! search query. Ordering (pinned-first) is owned by the GTK model; this only
//! filters, preserving the given order, so it's trivially unit-testable.

/// Indices of `texts` to show for `query`: those containing `query`
/// case-insensitively, in the original order. An empty or whitespace-only query
/// returns every index (no filtering).
pub fn matching_indices(texts: &[String], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    texts
        .iter()
        .enumerate()
        .filter(|(_, t)| q.is_empty() || t.to_lowercase().contains(&q))
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty_query_returns_all_in_order() {
        assert_eq!(matching_indices(&v(&["a", "b", "c"]), ""), vec![0, 1, 2]);
    }

    #[test]
    fn whitespace_query_returns_all() {
        assert_eq!(matching_indices(&v(&["a", "b"]), "   "), vec![0, 1]);
    }

    #[test]
    fn filters_case_insensitive_substring() {
        let items = v(&["git Rebase", "ssh deploy", "GIT push"]);
        assert_eq!(matching_indices(&items, "git"), vec![0, 2]);
    }

    #[test]
    fn no_match_is_empty() {
        assert!(matching_indices(&v(&["abc", "def"]), "zzz").is_empty());
    }

    #[test]
    fn preserves_order_of_matches() {
        let items = v(&["xen", "axe", "box", "axiom"]);
        assert_eq!(matching_indices(&items, "x"), vec![0, 1, 2, 3]);
    }
}
