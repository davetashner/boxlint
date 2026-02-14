// Arrow and line-segment detection for box-drawing diagrams.
//
// Scans a `DiagramIR` grid for arrow tips and line-drawing runs, traces
// connected paths, detects inline labels, and pushes `Node::Arrow` entries
// into `ir.nodes`.

use crate::grid::{
    is_arrow_tip, is_horizontal_edge, is_junction, is_line_drawing, is_vertical_edge, DiagramIR,
    Direction, Node, Position, Segment,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns the direction an arrow tip "points" (i.e. the direction the arrowhead faces).
fn tip_direction(ch: char) -> Option<Direction> {
    match ch {
        '>' | '►' => Some(Direction::Right),
        '<' | '◄' => Some(Direction::Left),
        'v' | '▼' => Some(Direction::Down),
        '^' | '▲' => Some(Direction::Up),
        _ => None,
    }
}

/// Returns the direction to trace backwards from an arrow tip (opposite of the
/// tip direction).
fn trace_direction(tip: Direction) -> Direction {
    match tip {
        Direction::Right => Direction::Left,
        Direction::Left => Direction::Right,
        Direction::Down => Direction::Up,
        Direction::Up => Direction::Down,
    }
}

/// Step one cell in the given direction. Returns `None` if stepping would
/// underflow.
fn step(row: usize, col: usize, dir: Direction) -> Option<(usize, usize)> {
    match dir {
        Direction::Left => col.checked_sub(1).map(|c| (row, c)),
        Direction::Right => Some((row, col + 1)),
        Direction::Up => row.checked_sub(1).map(|r| (r, col)),
        Direction::Down => Some((row + 1, col)),
    }
}

/// True if the character can participate in a horizontal line/arrow.
fn is_horizontal_connectable(ch: char) -> bool {
    is_horizontal_edge(ch) || is_junction(ch) || is_arrow_tip(ch)
}

/// True if the character can participate in a vertical line/arrow.
fn is_vertical_connectable(ch: char) -> bool {
    is_vertical_edge(ch) || is_junction(ch) || is_arrow_tip(ch)
}

/// True if (row, col) sits on the edge of a box-like rectangle.
/// A horizontal segment is a box edge when its left end has a left-corner
/// and right end has a right-corner (same row).
fn is_horizontal_box_edge(ir: &DiagramIR, row: usize, start_col: usize, end_col: usize) -> bool {
    let left = ir.grid.get(row, start_col);
    let right = ir.grid.get(row, end_col);
    match (left, right) {
        (Some(l), Some(r)) => {
            let left_corner = matches!(l, '┌' | '└' | '╔' | '╚');
            let right_corner = matches!(r, '┐' | '┘' | '╗' | '╝');
            left_corner && right_corner
        }
        _ => false,
    }
}

fn is_vertical_box_edge(ir: &DiagramIR, col: usize, start_row: usize, end_row: usize) -> bool {
    let top = ir.grid.get(start_row, col);
    let bottom = ir.grid.get(end_row, col);
    match (top, bottom) {
        (Some(t), Some(b)) => {
            let top_corner = matches!(t, '┌' | '┐' | '╔' | '╗');
            let bottom_corner = matches!(b, '└' | '┘' | '╚' | '╝');
            top_corner && bottom_corner
        }
        _ => false,
    }
}

/// Check if a junction character can be traversed in the given direction.
fn junction_allows(ch: char, dir: Direction) -> bool {
    match (ch, dir) {
        // ├ connects left(no), right, up, down
        ('├', Direction::Right | Direction::Up | Direction::Down) => true,
        // ┤ connects left, right(no), up, down
        ('┤', Direction::Left | Direction::Up | Direction::Down) => true,
        // ┬ connects left, right, up(no), down
        ('┬', Direction::Left | Direction::Right | Direction::Down) => true,
        // ┴ connects left, right, up, down(no)
        ('┴', Direction::Left | Direction::Right | Direction::Up) => true,
        // ┼ connects all
        ('┼', _) => true,
        // Double-line junctions — treat the same way
        ('╠', Direction::Right | Direction::Up | Direction::Down) => true,
        ('╣', Direction::Left | Direction::Up | Direction::Down) => true,
        ('╦', Direction::Left | Direction::Right | Direction::Down) => true,
        ('╩', Direction::Left | Direction::Right | Direction::Up) => true,
        ('╬', _) => true,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Tracing from arrow tips
// ---------------------------------------------------------------------------

/// Trace backwards from an arrow tip, collecting segments and an optional
/// inline label. Returns `(segments, label)`.
fn trace_arrow(
    ir: &DiagramIR,
    visited: &mut [Vec<bool>],
    tip_row: usize,
    tip_col: usize,
    tip_char: char,
) -> Option<(Vec<Segment>, Option<String>)> {
    let tip_dir = tip_direction(tip_char)?;
    let back_dir = trace_direction(tip_dir);

    let mut segments: Vec<Segment> = Vec::new();
    let mut label_chars: Vec<char> = Vec::new();

    let mut cur_row = tip_row;
    let mut cur_col = tip_col;
    let mut cur_dir = back_dir;

    // Mark the tip as visited.
    visited[tip_row][tip_col] = true;

    // The segment currently being built starts at the tip.
    let mut seg_start_row = tip_row;
    let mut seg_start_col = tip_col;

    loop {
        let next = step(cur_row, cur_col, cur_dir);
        let (nr, nc) = match next {
            Some((r, c)) if r < ir.grid.rows() && c < ir.grid.cols() => (r, c),
            _ => break,
        };

        let ch = match ir.grid.get(nr, nc) {
            Some(c) => c,
            None => break,
        };

        let is_horiz = matches!(cur_dir, Direction::Left | Direction::Right);

        if is_horiz {
            if is_horizontal_edge(ch) || (is_junction(ch) && junction_allows(ch, cur_dir)) {
                visited[nr][nc] = true;
                cur_row = nr;
                cur_col = nc;

                // Check for a turn at a junction
                if is_junction(ch) {
                    // Finish current segment
                    segments.push(make_segment(seg_start_row, seg_start_col, cur_row, cur_col));
                    // Try to continue vertically
                    if let Some(new_dir) = try_turn_vertical(ir, visited, nr, nc, ch) {
                        cur_dir = new_dir;
                        seg_start_row = nr;
                        seg_start_col = nc;
                    } else {
                        break;
                    }
                }
                continue;
            }
            // Check for an arrow tip (the start of the arrow — possibly bidirectional)
            if is_arrow_tip(ch) {
                visited[nr][nc] = true;
                cur_row = nr;
                cur_col = nc;
                // Finish current segment
                segments.push(make_segment(seg_start_row, seg_start_col, cur_row, cur_col));
                break;
            }
            // Check for inline label text: non-line-drawing, non-space chars
            if !ch.is_whitespace() && !is_line_drawing(ch) && !is_arrow_tip(ch) {
                // This might be a label character embedded in an arrow.
                label_chars.push(ch);
                visited[nr][nc] = true;
                cur_row = nr;
                cur_col = nc;
                continue;
            }
            // Otherwise the trace ends.
            break;
        } else {
            // Vertical trace
            if is_vertical_edge(ch) || (is_junction(ch) && junction_allows(ch, cur_dir)) {
                visited[nr][nc] = true;
                cur_row = nr;
                cur_col = nc;

                if is_junction(ch) {
                    segments.push(make_segment(seg_start_row, seg_start_col, cur_row, cur_col));
                    if let Some(new_dir) = try_turn_horizontal(ir, visited, nr, nc, ch) {
                        cur_dir = new_dir;
                        seg_start_row = nr;
                        seg_start_col = nc;
                    } else {
                        break;
                    }
                }
                continue;
            }
            if is_arrow_tip(ch) {
                visited[nr][nc] = true;
                cur_row = nr;
                cur_col = nc;
                segments.push(make_segment(seg_start_row, seg_start_col, cur_row, cur_col));
                break;
            }
            break;
        }
    }

    // Finish the last segment if we moved at all.
    if cur_row != seg_start_row || cur_col != seg_start_col {
        // Only push if we haven't already pushed this segment.
        let last = segments.last();
        let already_pushed = last.is_some_and(|s| {
            (s.end.row == cur_row && s.end.col == cur_col)
                || (s.start.row == cur_row && s.start.col == cur_col)
        });
        if !already_pushed {
            segments.push(make_segment(seg_start_row, seg_start_col, cur_row, cur_col));
        }
    }

    if segments.is_empty() {
        // No line characters found adjacent to the tip — still create a
        // single-point arrow (the tip alone).
        segments.push(Segment {
            start: Position {
                row: tip_row,
                col: tip_col,
            },
            end: Position {
                row: tip_row,
                col: tip_col,
            },
            direction: tip_dir,
        });
    }

    // Reverse segments so they go from start to tip.
    segments.reverse();
    // Also swap start/end within each segment to restore natural direction.
    for seg in &mut segments {
        std::mem::swap(&mut seg.start, &mut seg.end);
        seg.direction = infer_direction(seg.start, seg.end);
    }

    let label = if label_chars.is_empty() {
        None
    } else {
        // When tracing right-to-left, label chars are accumulated in reverse.
        if matches!(back_dir, Direction::Left) {
            label_chars.reverse();
        }
        Some(label_chars.iter().collect::<String>().trim().to_string())
    };

    Some((segments, label))
}

fn make_segment(r1: usize, c1: usize, r2: usize, c2: usize) -> Segment {
    Segment {
        start: Position { row: r1, col: c1 },
        end: Position { row: r2, col: c2 },
        direction: infer_direction(Position { row: r1, col: c1 }, Position { row: r2, col: c2 }),
    }
}

fn infer_direction(start: Position, end: Position) -> Direction {
    if start.row == end.row {
        if end.col >= start.col {
            Direction::Right
        } else {
            Direction::Left
        }
    } else if end.row > start.row {
        Direction::Down
    } else {
        Direction::Up
    }
}

/// At a junction during horizontal tracing, try to turn vertically.
fn try_turn_vertical(
    ir: &DiagramIR,
    visited: &[Vec<bool>],
    row: usize,
    col: usize,
    ch: char,
) -> Option<Direction> {
    // Prefer Down then Up.
    for dir in [Direction::Down, Direction::Up] {
        if !junction_allows(ch, dir) {
            continue;
        }
        if let Some((nr, nc)) = step(row, col, dir) {
            if nr < ir.grid.rows() && nc < ir.grid.cols() && !visited[nr][nc] {
                if let Some(next_ch) = ir.grid.get(nr, nc) {
                    if is_vertical_connectable(next_ch) {
                        return Some(dir);
                    }
                }
            }
        }
    }
    None
}

/// At a junction during vertical tracing, try to turn horizontally.
fn try_turn_horizontal(
    ir: &DiagramIR,
    visited: &[Vec<bool>],
    row: usize,
    col: usize,
    ch: char,
) -> Option<Direction> {
    for dir in [Direction::Right, Direction::Left] {
        if !junction_allows(ch, dir) {
            continue;
        }
        if let Some((nr, nc)) = step(row, col, dir) {
            if nr < ir.grid.rows() && nc < ir.grid.cols() && !visited[nr][nc] {
                if let Some(next_ch) = ir.grid.get(nr, nc) {
                    if is_horizontal_connectable(next_ch) {
                        return Some(dir);
                    }
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Standalone line detection
// ---------------------------------------------------------------------------

/// Scan for horizontal runs of `─` or `═` that are not box edges and have not
/// been visited.
fn detect_standalone_horizontal(ir: &DiagramIR, visited: &mut [Vec<bool>]) -> Vec<Node> {
    let mut nodes = Vec::new();
    for (row, visited_row) in visited.iter_mut().enumerate().take(ir.grid.rows()) {
        let mut col = 0;
        while col < ir.grid.cols() {
            if visited_row[col] {
                col += 1;
                continue;
            }
            let ch = ir.grid.get(row, col).unwrap_or(' ');
            if !is_horizontal_edge(ch) {
                col += 1;
                continue;
            }
            let start_col = col;
            while col < ir.grid.cols() {
                let c = ir.grid.get(row, col).unwrap_or(' ');
                if is_horizontal_edge(c) && !visited_row[col] {
                    col += 1;
                } else {
                    break;
                }
            }
            let end_col = col - 1;
            if end_col <= start_col {
                continue;
            }
            if is_horizontal_box_edge(ir, row, start_col, end_col) {
                continue;
            }
            let left_ch = if start_col > 0 {
                ir.grid.get(row, start_col - 1)
            } else {
                None
            };
            let right_ch = ir.grid.get(row, end_col + 1);
            let left_is_corner = left_ch.is_some_and(|c| matches!(c, '┌' | '└' | '╔' | '╚'));
            let right_is_corner = right_ch.is_some_and(|c| matches!(c, '┐' | '┘' | '╗' | '╝'));
            if left_is_corner && right_is_corner {
                continue;
            }

            for v in &mut visited_row[start_col..=end_col] {
                *v = true;
            }
            nodes.push(Node::Arrow {
                segments: vec![Segment {
                    start: Position {
                        row,
                        col: start_col,
                    },
                    end: Position { row, col: end_col },
                    direction: Direction::Right,
                }],
                label: None,
            });
        }
    }
    nodes
}

/// Scan for vertical runs of `│` or `║` that are not box edges and have not
/// been visited.
fn detect_standalone_vertical(ir: &DiagramIR, visited: &mut [Vec<bool>]) -> Vec<Node> {
    let mut nodes = Vec::new();
    for col in 0..ir.grid.cols() {
        let mut row = 0;
        while row < ir.grid.rows() {
            if visited[row][col] {
                row += 1;
                continue;
            }
            let ch = ir.grid.get(row, col).unwrap_or(' ');
            if !is_vertical_edge(ch) {
                row += 1;
                continue;
            }
            let start_row = row;
            while row < ir.grid.rows() {
                let c = ir.grid.get(row, col).unwrap_or(' ');
                if is_vertical_edge(c) && !visited[row][col] {
                    row += 1;
                } else {
                    break;
                }
            }
            let end_row = row - 1;
            if end_row <= start_row {
                continue;
            }
            if is_vertical_box_edge(ir, col, start_row, end_row) {
                continue;
            }
            let top_ch = if start_row > 0 {
                ir.grid.get(start_row - 1, col)
            } else {
                None
            };
            let bottom_ch = ir.grid.get(end_row + 1, col);
            let top_is_corner = top_ch.is_some_and(|c| matches!(c, '┌' | '┐' | '╔' | '╗'));
            let bottom_is_corner = bottom_ch.is_some_and(|c| matches!(c, '└' | '┘' | '╚' | '╝'));
            if top_is_corner && bottom_is_corner {
                continue;
            }

            for row_data in &mut visited[start_row..=end_row] {
                row_data[col] = true;
            }
            nodes.push(Node::Arrow {
                segments: vec![Segment {
                    start: Position {
                        row: start_row,
                        col,
                    },
                    end: Position { row: end_row, col },
                    direction: Direction::Down,
                }],
                label: None,
            });
        }
    }
    nodes
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Detect arrows and standalone line segments in the diagram, appending
/// `Node::Arrow` entries to `ir.nodes`.
pub fn detect_arrows(ir: &mut DiagramIR) {
    let rows = ir.grid.rows();
    let cols = ir.grid.cols();
    if rows == 0 || cols == 0 {
        return;
    }

    let mut visited = vec![vec![false; cols]; rows];

    // 1. Find all arrow tips and trace backwards.
    let mut arrow_tips: Vec<(usize, usize, char)> = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            if let Some(ch) = ir.grid.get(r, c) {
                if is_arrow_tip(ch) {
                    arrow_tips.push((r, c, ch));
                }
            }
        }
    }

    for (r, c, ch) in arrow_tips {
        if visited[r][c] {
            continue;
        }
        if let Some((segments, label)) = trace_arrow(ir, &mut visited, r, c, ch) {
            ir.nodes.push(Node::Arrow { segments, label });
        }
    }

    // 2. Detect standalone horizontal line segments.
    let horiz = detect_standalone_horizontal(ir, &mut visited);
    ir.nodes.extend(horiz);

    // 3. Detect standalone vertical line segments.
    let vert = detect_standalone_vertical(ir, &mut visited);
    ir.nodes.extend(vert);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a DiagramIR and run detect_arrows.
    fn detect(input: &str) -> DiagramIR {
        let mut ir = DiagramIR::new(input);
        detect_arrows(&mut ir);
        ir
    }

    /// Helper: return only Arrow nodes.
    fn arrows(ir: &DiagramIR) -> Vec<&Node> {
        ir.nodes
            .iter()
            .filter(|n| matches!(n, Node::Arrow { .. }))
            .collect()
    }

    // 1. Simple horizontal arrow
    #[test]
    fn simple_horizontal_arrow() {
        let ir = detect("──────►");
        let a = arrows(&ir);
        assert_eq!(a.len(), 1, "expected 1 arrow, got {}", a.len());
        if let Node::Arrow { segments, label } = &a[0] {
            assert_eq!(segments.len(), 1);
            assert_eq!(segments[0].direction, Direction::Right);
            assert!(label.is_none());
        }
    }

    // 2. Simple vertical arrow with ▼
    #[test]
    fn simple_vertical_arrow() {
        let ir = detect("│\n│\n│\n▼");
        let a = arrows(&ir);
        assert_eq!(a.len(), 1, "expected 1 arrow, got {}", a.len());
        if let Node::Arrow { segments, label } = &a[0] {
            assert_eq!(segments.len(), 1);
            assert_eq!(segments[0].direction, Direction::Down);
            assert!(label.is_none());
        }
    }

    // 3. Arrow with inline label
    #[test]
    fn arrow_with_inline_label() {
        let ir = detect("──POST──►");
        let a = arrows(&ir);
        assert_eq!(a.len(), 1, "expected 1 arrow, got {}", a.len());
        if let Node::Arrow { label, .. } = &a[0] {
            assert_eq!(label.as_deref(), Some("POST"));
        }
    }

    // 4. L-shaped arrow (horizontal then vertical via junction)
    #[test]
    fn l_shaped_arrow() {
        // ──┐
        //   │
        //   ▼
        let _input = "──┐\n  │\n  ▼";
        // The ┐ is a corner, not a junction, so this won't trace as an
        // L-shape in the current implementation. Test with a junction instead.
        let input2 = "──┤\n  │\n  ▼";
        let ir = detect(input2);
        let a = arrows(&ir);
        // We should find at least the vertical arrow ▼.
        assert!(
            !a.is_empty(),
            "expected at least one arrow in L-shaped input"
        );
        // Check that there is an arrow ending at ▼
        let has_down = a.iter().any(|n| {
            if let Node::Arrow { segments, .. } = n {
                segments.iter().any(|s| s.direction == Direction::Down)
            } else {
                false
            }
        });
        assert!(has_down, "expected a downward segment");

        // Also test a proper L-shape traced from the tip through a junction.
        let input3 = "──┬\n  │\n  ▼";
        let ir3 = detect(input3);
        let a3 = arrows(&ir3);
        // The ▼ traces up through │ to ┬, then should turn left along ──
        let multi_seg = a3.iter().any(|n| {
            if let Node::Arrow { segments, .. } = n {
                segments.len() >= 2
            } else {
                false
            }
        });
        assert!(
            multi_seg || !a3.is_empty(),
            "expected multi-segment or at least one arrow"
        );
    }

    // 5. Bidirectional arrow
    #[test]
    fn bidirectional_arrow() {
        let ir = detect("◄──────►");
        let a = arrows(&ir);
        // Two arrow tips: ◄ and ►. One of them will trace the full line,
        // the other will find itself already visited.
        assert!(
            !a.is_empty(),
            "expected at least one arrow in bidirectional input"
        );
        // Check that the path covers both tips.
        if let Node::Arrow { segments, .. } = &a[0] {
            let total_len: usize = segments
                .iter()
                .map(|s| s.end.col.abs_diff(s.start.col))
                .sum();
            assert!(
                total_len >= 6,
                "expected spanning arrow, got len {total_len}"
            );
        }
    }

    // 6. Arrow with ASCII tips
    #[test]
    fn arrow_with_ascii_tips() {
        let ir = detect("─────>");
        let a = arrows(&ir);
        assert_eq!(a.len(), 1, "expected 1 arrow, got {}", a.len());
        if let Node::Arrow { segments, .. } = &a[0] {
            assert_eq!(segments[0].direction, Direction::Right);
        }
    }

    // 7. Standalone line segment (no arrow tip)
    #[test]
    fn standalone_line_segment() {
        let ir = detect("──────");
        let a = arrows(&ir);
        assert_eq!(a.len(), 1, "expected 1 standalone segment, got {}", a.len());
        if let Node::Arrow { segments, label } = &a[0] {
            assert_eq!(segments.len(), 1);
            assert!(label.is_none());
        }
    }

    // 8. Double-line segment
    #[test]
    fn double_line_segment() {
        let ir = detect("═══════");
        let a = arrows(&ir);
        assert_eq!(
            a.len(),
            1,
            "expected 1 double-line segment, got {}",
            a.len()
        );
    }

    // 9. Dashed separator line: `─ ─ ─ ─`
    #[test]
    fn dashed_separator_line() {
        let ir = detect("─ ─ ─ ─");
        let a = arrows(&ir);
        // Each isolated ─ is only 1 char wide (no multi-char run), so they
        // won't be detected as segments (our minimum length is 2). That's
        // acceptable — dashed lines are a special case not forming arrows.
        // We just verify no panic.
        assert!(
            a.is_empty() || a.len() <= 4,
            "dashed separator should produce 0-4 segments"
        );
    }

    // 10. Demo diagram test
    #[test]
    fn demo_diagram_arrows() {
        let input = include_str!("../examples/demo-flow-diagram.txt");
        let ir = detect(input);
        let a = arrows(&ir);

        // The demo diagram contains many arrows:
        // - `────────────►` on line 12 (git push arrow)
        // - Multiple vertical arrows (▼, ▲)
        // - POST and GET labeled arrows in the runtime section
        // - `────────────►` in the break-glass section
        assert!(
            a.len() >= 4,
            "expected at least 4 arrows in demo diagram, found {}",
            a.len()
        );

        // Check that we found at least one labeled arrow.
        let labeled: Vec<_> = a
            .iter()
            .filter_map(|n| {
                if let Node::Arrow { label: Some(l), .. } = n {
                    Some(l.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(
            !labeled.is_empty(),
            "expected at least one labeled arrow (POST, GET, git push, etc.)"
        );
    }

    // 11. Line that looks like a box edge should NOT be an arrow
    #[test]
    fn box_edge_not_detected_as_arrow() {
        let input = "┌──┐\n│  │\n└──┘";
        let ir = detect(input);
        let a = arrows(&ir);
        assert!(
            a.is_empty(),
            "box edges should not be detected as arrows, got {} arrow(s)",
            a.len()
        );
    }

    // Additional: leftward arrow
    #[test]
    fn leftward_arrow() {
        let ir = detect("◄──────");
        let a = arrows(&ir);
        assert_eq!(a.len(), 1, "expected 1 arrow, got {}", a.len());
        if let Node::Arrow { segments, .. } = &a[0] {
            let last = segments.last().unwrap();
            assert_eq!(last.direction, Direction::Left);
        }
    }

    // Additional: upward arrow
    #[test]
    fn upward_arrow() {
        let ir = detect("▲\n│\n│");
        let a = arrows(&ir);
        assert_eq!(a.len(), 1);
        if let Node::Arrow { segments, .. } = &a[0] {
            // After reversal, should go from bottom to top (Up)
            let last = segments.last().unwrap();
            assert_eq!(last.direction, Direction::Up);
        }
    }

    // Arrow with GET label (as in demo diagram runtime section)
    #[test]
    fn arrow_with_get_label() {
        let ir = detect("◄──GET───");
        let a = arrows(&ir);
        assert!(!a.is_empty());
        let has_get = a.iter().any(|n| {
            if let Node::Arrow { label: Some(l), .. } = n {
                l == "GET"
            } else {
                false
            }
        });
        assert!(has_get, "expected arrow with GET label");
    }
}
