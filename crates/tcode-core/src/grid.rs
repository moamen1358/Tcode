//! Grid geometry: how N panes are arranged, and how focus moves between them.

/// Number of panes in each row of a balanced `n`-pane grid.
///
/// `rows = round(sqrt(n))`; the first `n % rows` rows get one extra pane, so
/// every pane fills its row and there are no empty cells.
pub fn layout(n: usize) -> Vec<usize> {
    if n == 0 {
        return Vec::new();
    }
    let rows = ((n as f64).sqrt().round() as usize).max(1);
    let base = n / rows;
    let extra = n % rows;
    (0..rows)
        .map(|r| if r < extra { base + 1 } else { base })
        .collect()
}

/// A focus-movement direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// Move focus within a jagged grid described by `widths` (panes per row).
///
/// Movement clamps at edges; up/down clamp the column into the target row so a
/// short last row never leaves focus pointing at a missing pane.
pub fn neighbor(widths: &[usize], row: usize, col: usize, dir: Dir) -> (usize, usize) {
    if widths.is_empty() {
        return (0, 0);
    }
    let row = row.min(widths.len() - 1);
    let col = col.min(widths[row].saturating_sub(1));
    match dir {
        Dir::Left => (row, col.saturating_sub(1)),
        Dir::Right => (row, (col + 1).min(widths[row].saturating_sub(1))),
        Dir::Up => {
            let nr = row.saturating_sub(1);
            (nr, col.min(widths[nr].saturating_sub(1)))
        }
        Dir::Down => {
            let nr = (row + 1).min(widths.len() - 1);
            (nr, col.min(widths[nr].saturating_sub(1)))
        }
    }
}

/// Flat (row-major) index of the pane at `(row, col)`.
///
/// # Panics
/// In debug builds, asserts `row < widths.len()` — `row == len` is one past the
/// last row, which would sum every width and return an out-of-range flat index.
/// Callers derive `(row, col)` from `layout`/`neighbor`, which keep them in range.
pub fn flat_index(widths: &[usize], row: usize, col: usize) -> usize {
    debug_assert!(
        row < widths.len(),
        "row {row} out of range for {} rows",
        widths.len()
    );
    widths[..row].iter().sum::<usize>() + col
}

/// Inverse of [`flat_index`]: the `(row, col)` of the pane at flat index `idx`,
/// given row `widths`. Clamps to the last cell if `idx` is out of range.
pub fn coords(widths: &[usize], idx: usize) -> (usize, usize) {
    let mut remaining = idx;
    for (r, &w) in widths.iter().enumerate() {
        if remaining < w {
            return (r, remaining);
        }
        remaining -= w;
    }
    let r = widths.len().saturating_sub(1);
    let c = widths.get(r).copied().unwrap_or(0).saturating_sub(1);
    (r, c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_counts_match_total() {
        for n in 1..=16 {
            assert_eq!(layout(n).iter().sum::<usize>(), n, "n={n}");
        }
    }

    #[test]
    fn layout_known_shapes() {
        assert_eq!(layout(1), vec![1]);
        assert_eq!(layout(2), vec![2]);
        assert_eq!(layout(3), vec![2, 1]);
        assert_eq!(layout(4), vec![2, 2]);
        assert_eq!(layout(5), vec![3, 2]);
        assert_eq!(layout(6), vec![3, 3]);
        assert_eq!(layout(9), vec![3, 3, 3]);
    }

    #[test]
    fn layout_zero_is_empty() {
        assert!(layout(0).is_empty());
    }

    #[test]
    fn neighbor_clamps_at_edges() {
        let w = [3, 3, 3];
        assert_eq!(neighbor(&w, 0, 0, Dir::Left), (0, 0));
        assert_eq!(neighbor(&w, 0, 0, Dir::Up), (0, 0));
        assert_eq!(neighbor(&w, 2, 2, Dir::Right), (2, 2));
        assert_eq!(neighbor(&w, 2, 2, Dir::Down), (2, 2));
    }

    #[test]
    fn neighbor_moves_within_grid() {
        let w = [3, 3, 3];
        assert_eq!(neighbor(&w, 1, 1, Dir::Left), (1, 0));
        assert_eq!(neighbor(&w, 1, 1, Dir::Right), (1, 2));
        assert_eq!(neighbor(&w, 1, 1, Dir::Up), (0, 1));
        assert_eq!(neighbor(&w, 1, 1, Dir::Down), (2, 1));
    }

    #[test]
    fn neighbor_clamps_column_into_shorter_row() {
        let w = [2, 1]; // second row has 1 pane
        assert_eq!(neighbor(&w, 0, 1, Dir::Down), (1, 0));
    }

    #[test]
    fn flat_index_maps_correctly() {
        let w = [2, 1];
        assert_eq!(flat_index(&w, 0, 0), 0);
        assert_eq!(flat_index(&w, 0, 1), 1);
        assert_eq!(flat_index(&w, 1, 0), 2);
    }

    #[test]
    fn coords_inverts_flat_index() {
        for w in [vec![1], vec![2], vec![3, 2], vec![3, 3, 3], vec![3, 3, 2]] {
            let total: usize = w.iter().sum();
            for idx in 0..total {
                let (r, c) = coords(&w, idx);
                assert_eq!(flat_index(&w, r, c), idx, "widths={w:?} idx={idx}");
            }
        }
    }

    #[test]
    fn neighbor_is_total_for_zero_width_rows() {
        // Degenerate zero-width rows must not panic (public-API totality).
        let _ = neighbor(&[0], 0, 0, Dir::Right);
        let _ = neighbor(&[0, 0], 1, 0, Dir::Up);
        let _ = neighbor(&[0, 0], 0, 0, Dir::Down);
    }
}
