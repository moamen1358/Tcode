//! BridgeShot annotation model + pure geometry helpers (unit-tested, no GTK).

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Box,
    Arrow,
    Text,
    Pen,
    Highlight,
}

pub type Rgb = (f64, f64, f64);

/// Toolbar swatches.
pub const PALETTE: [Rgb; 6] = [
    (0.910, 0.302, 0.357), // red    #e84d5b
    (0.478, 0.635, 0.969), // blue   #7aa2f7  (existing accent)
    (0.878, 0.686, 0.408), // yellow #e0af68
    (0.549, 0.776, 0.451), // green  #8cc673
    (0.949, 0.949, 0.969), // white  #f2f2f7
    (0.102, 0.110, 0.149), // near-black #1a1c26
];

pub const DEFAULT_COLOR: Rgb = PALETTE[1];

pub enum Shape {
    Box {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
    Arrow {
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    },
    Text {
        x: f64,
        y: f64,
        content: String,
        size: f64,
    },
    Stroke {
        points: Vec<(f64, f64)>,
        highlight: bool,
    },
}

pub struct Annotation {
    pub shape: Shape,
    pub color: Rgb,
}

/// Order two drag corners into (x, y, w, h) with non-negative size.
pub fn norm(x0: f64, y0: f64, x1: f64, y1: f64) -> (f64, f64, f64, f64) {
    (x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs())
}

/// Triangle for an arrowhead at the (x1,y1) end: returns [tip, wing, wing].
pub fn arrow_head(
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    head_len: f64,
    head_w: f64,
) -> [(f64, f64); 3] {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-6 {
        return [(x1, y1), (x1, y1), (x1, y1)];
    }
    let (ux, uy) = (dx / len, dy / len);
    let (bx, by) = (x1 - ux * head_len, y1 - uy * head_len); // base center
    let (px, py) = (-uy, ux); // unit perpendicular
    let half = head_w / 2.0;
    [
        (x1, y1),
        (bx + px * half, by + py * half),
        (bx - px * half, by - py * half),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_orders_corners() {
        assert_eq!(norm(5.0, 5.0, 1.0, 1.0), (1.0, 1.0, 4.0, 4.0));
        assert_eq!(norm(1.0, 1.0, 5.0, 3.0), (1.0, 1.0, 4.0, 2.0));
    }

    #[test]
    fn arrow_head_points_at_tip() {
        // Horizontal arrow (0,0)->(10,0), head 2 long, 2 wide.
        let [tip, left, right] = arrow_head(0.0, 0.0, 10.0, 0.0, 2.0, 2.0);
        assert_eq!(tip, (10.0, 0.0));
        // base center is 2 back from tip at (8,0); wings ±1 perpendicular.
        assert!((left.0 - 8.0).abs() < 1e-9 && (left.1 - 1.0).abs() < 1e-9);
        assert!((right.0 - 8.0).abs() < 1e-9 && (right.1 + 1.0).abs() < 1e-9);
    }

    #[test]
    fn arrow_head_zero_length_is_degenerate() {
        let pts = arrow_head(3.0, 3.0, 3.0, 3.0, 2.0, 2.0);
        assert_eq!(pts, [(3.0, 3.0), (3.0, 3.0), (3.0, 3.0)]);
    }
}
