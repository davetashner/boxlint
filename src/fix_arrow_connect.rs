// Arrow connection auto-fixer.
//
// Extends arrow segments so disconnected tips reach the nearest box edge.
// Only fixes gaps of 1-2 characters; larger gaps are left unfixed.

use crate::detect_arrows::detect_arrows;
use crate::detect_boxes::detect_boxes;
use crate::grid::{is_arrow_tip, is_line_drawing, BoundingRect, DiagramIR, Node, Position};
use crate::Fixer;

pub struct ArrowConnectFixer;

// ---------------------------------------------------------------------------
// Mutable grid helpers (duplicated — they're private in other modules)
// ---------------------------------------------------------------------------

fn input_to_mut_grid(input: &str) -> Vec<Vec<char>> {
    input.lines().map(|l| l.chars().collect()).collect()
}

fn mut_grid_to_string(grid: &[Vec<char>]) -> String {
    grid.iter()
        .map(|row| row.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn grid_get(grid: &[Vec<char>], row: usize, col: usize) -> Option<char> {
    grid.get(row).and_then(|r| r.get(col)).copied()
}

fn ensure_row_width(grid: &mut [Vec<char>], row: usize, needed_col: usize) {
    if row < grid.len() && grid[row].len() <= needed_col {
        grid[row].resize(needed_col + 1, ' ');
    }
}

// ---------------------------------------------------------------------------
// Edge / direction helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

/// Check whether `p` is adjacent to (or on the edge of) a box's bounding rect.
fn endpoint_touches_box(p: Position, bounds: &BoundingRect) -> Option<Edge> {
    let tl = bounds.top_left;
    let br = bounds.bottom_right;

    // ON the edge
    if p.row == tl.row && p.col >= tl.col && p.col <= br.col {
        return Some(Edge::Top);
    }
    if p.row == br.row && p.col >= tl.col && p.col <= br.col {
        return Some(Edge::Bottom);
    }
    if p.col == tl.col && p.row >= tl.row && p.row <= br.row {
        return Some(Edge::Left);
    }
    if p.col == br.col && p.row >= tl.row && p.row <= br.row {
        return Some(Edge::Right);
    }

    // ADJACENT: 1 cell outside
    if p.row + 1 == tl.row && p.col >= tl.col && p.col <= br.col {
        return Some(Edge::Top);
    }
    if p.row == br.row + 1 && p.col >= tl.col && p.col <= br.col {
        return Some(Edge::Bottom);
    }
    if p.col + 1 == tl.col && p.row >= tl.row && p.row <= br.row {
        return Some(Edge::Left);
    }
    if p.col == br.col + 1 && p.row >= tl.row && p.row <= br.row {
        return Some(Edge::Right);
    }

    None
}

/// Returns the grid delta (dr, dc) for the direction a tip character faces.
fn tip_delta(ch: char) -> Option<(isize, isize)> {
    match ch {
        '►' | '>' => Some((0, 1)),
        '◄' | '<' => Some((0, -1)),
        '▼' | 'v' => Some((1, 0)),
        '▲' | '^' => Some((-1, 0)),
        _ => None,
    }
}

/// Returns the line-drawing character for the given direction.
fn trail_char(_dr: isize, dc: isize) -> char {
    if dc != 0 {
        '─'
    } else {
        '│'
    }
}

/// Check if a tip's facing direction is consistent with the box edge.
fn tip_direction_ok(tip_char: char, edge: Edge) -> bool {
    match tip_char {
        '►' | '>' => edge == Edge::Left,
        '◄' | '<' => edge == Edge::Right,
        '▼' | 'v' => edge == Edge::Top,
        '▲' | '^' => edge == Edge::Bottom,
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// Text adjacency (noise reduction — same logic as lint)
// ---------------------------------------------------------------------------

fn adjacent_to_text(ir: &DiagramIR, p: Position) -> bool {
    let directions: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    let radius: isize = 3;
    for (dr, dc) in &directions {
        for dist in 1..=radius {
            let nr = p.row as isize + dr * dist;
            let nc = p.col as isize + dc * dist;
            if nr < 0 || nc < 0 {
                break;
            }
            match ir.grid.get(nr as usize, nc as usize) {
                Some(ch) if !ch.is_whitespace() && !is_line_drawing(ch) && !is_arrow_tip(ch) => {
                    return true;
                }
                Some(ch) if ch.is_whitespace() => continue,
                _ => break,
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Cell clearance check
// ---------------------------------------------------------------------------

/// Returns true if `ch` can be overwritten by extending an arrow trail.
/// Allows spaces, matching trail chars, and out-of-bounds (None).
fn cell_overwritable(ch: Option<char>, expected_trail: char) -> bool {
    match ch {
        Some(' ') | None => true,
        Some(c) => c == expected_trail,
    }
}

// ---------------------------------------------------------------------------
// Fixer implementation
// ---------------------------------------------------------------------------

impl Fixer for ArrowConnectFixer {
    fn name(&self) -> &str {
        "arrow-connect"
    }

    fn fix(&self, input: &str) -> String {
        if input.is_empty() {
            return String::new();
        }

        let trailing_newline = input.ends_with('\n');

        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);
        detect_arrows(&mut ir);

        let box_bounds: Vec<BoundingRect> = ir
            .nodes
            .iter()
            .filter_map(|n| {
                if let Node::Box { bounds, .. } = n {
                    Some(*bounds)
                } else {
                    None
                }
            })
            .collect();

        if box_bounds.is_empty() {
            return input.to_string();
        }

        let mut grid = input_to_mut_grid(input);

        for node in &ir.nodes {
            let segments = match node {
                Node::Arrow { segments, .. } if !segments.is_empty() => segments,
                _ => continue,
            };

            let first_pos = segments[0].start;
            let last_pos = segments.last().unwrap().end;
            let first_char = grid_get(&grid, first_pos.row, first_pos.col).unwrap_or(' ');
            let last_char = grid_get(&grid, last_pos.row, last_pos.col).unwrap_or(' ');

            if !is_arrow_tip(first_char) && !is_arrow_tip(last_char) {
                continue;
            }

            let endpoints = [(first_pos, first_char), (last_pos, last_char)];

            for (ep_pos, ep_char) in &endpoints {
                if !is_arrow_tip(*ep_char) {
                    continue;
                }

                // Already connected?
                let connected = box_bounds
                    .iter()
                    .any(|b| endpoint_touches_box(*ep_pos, b).is_some());
                if connected {
                    continue;
                }

                // Skip text-to-text arrows
                if adjacent_to_text(&ir, *ep_pos) {
                    continue;
                }

                // tip_delta always returns Some for is_arrow_tip chars
                let (dr, dc) = tip_delta(*ep_char).unwrap();

                let tchar = trail_char(dr, dc);

                // Extend forward 1-2 cells to reach a box
                'extend: for dist in 1..=2_isize {
                    let nr = ep_pos.row as isize + dr * dist;
                    let nc = ep_pos.col as isize + dc * dist;
                    if nr < 0 || nc < 0 {
                        break;
                    }
                    let nr = nr as usize;
                    let nc = nc as usize;

                    let new_pos = Position { row: nr, col: nc };

                    for bounds in &box_bounds {
                        if let Some(edge) = endpoint_touches_box(new_pos, bounds) {
                            if !tip_direction_ok(*ep_char, edge) {
                                continue;
                            }

                            // Check all cells between old and new position are overwritable
                            let mut clear = true;
                            for d in 1..=dist {
                                let cr = (ep_pos.row as isize + dr * d) as usize;
                                let cc = (ep_pos.col as isize + dc * d) as usize;
                                if !cell_overwritable(grid_get(&grid, cr, cc), tchar) {
                                    clear = false;
                                    break;
                                }
                            }

                            if clear {
                                // Replace old tip with trail char
                                grid[ep_pos.row][ep_pos.col] = tchar;
                                // Fill intermediate cells
                                for d in 1..dist {
                                    let cr = (ep_pos.row as isize + dr * d) as usize;
                                    let cc = (ep_pos.col as isize + dc * d) as usize;
                                    ensure_row_width(&mut grid, cr, cc);
                                    grid[cr][cc] = tchar;
                                }
                                // Place tip at new position
                                ensure_row_width(&mut grid, nr, nc);
                                grid[nr][nc] = *ep_char;
                                break 'extend;
                            }
                        }
                    }
                }
            }
        }

        let mut result = mut_grid_to_string(&grid);
        if trailing_newline {
            result.push('\n');
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LintRule;

    fn fix(input: &str) -> String {
        ArrowConnectFixer.fix(input)
    }

    // == Round-trip fidelity ===================================================

    #[test]
    fn round_trip_empty() {
        assert_eq!(fix(""), "");
    }

    #[test]
    fn round_trip_plain_text() {
        let input = "Hello world\nNo diagrams here\n";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn round_trip_well_connected_horizontal() {
        let input = "\
┌──┐
│  │──►┌──┐
└──┘   │  │
       └──┘";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn round_trip_well_connected_vertical() {
        let input = "\
┌──┐
│  │
└──┘
  │
  ▼
┌──┐
│  │
└──┘";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn round_trip_trailing_newline() {
        let input = "┌──┐\n│  │──►┌──┐\n└──┘   │  │\n       └──┘\n";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn round_trip_no_boxes() {
        let input = "──────►";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn round_trip_demo_diagram() {
        let input = include_str!("../examples/demo-flow-diagram.txt");
        let fixed = fix(input);
        // Demo has some disconnected arrows (due to malformed boxes) but no
        // arrows that are within 2 chars of a box edge they could connect to,
        // so it should be unchanged.
        assert_eq!(fixed, input);
    }

    #[test]
    fn round_trip_text_to_text_arrow() {
        let input = "Browser ──POST──► Lambda";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn round_trip_double_line_box() {
        let input = "\
╔══╗
║  ║──►╔══╗
╚══╝   ║  ║
       ╚══╝";
        assert_eq!(fix(input), input);
    }

    // == Extend forward fixes =================================================

    #[test]
    fn extend_right_1_cell() {
        let input = "\
┌──┐
│  │──► ┌──┐
└──┘    │  │
        └──┘";
        let fixed = fix(input);
        assert!(
            fixed.contains("───►┌"),
            "tip should be extended 1 cell right: {fixed}"
        );
        // Verify connection
        let diags = crate::lint_arrow_connect::ArrowConnectLint.check(&fixed);
        let arrow_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.rule == "disconnected-arrow")
            .collect();
        assert!(
            arrow_diags.is_empty(),
            "should have no disconnected arrows after fix: {arrow_diags:?}"
        );
    }

    #[test]
    fn extend_right_2_cells() {
        let input = "\
┌──┐
│  │──►  ┌──┐
└──┘     │  │
         └──┘";
        let fixed = fix(input);
        assert!(
            fixed.contains("────►┌"),
            "tip should be extended 2 cells right: {fixed}"
        );
    }

    #[test]
    fn extend_left_1_cell() {
        let input = "\
    ┌──┐
┌──┐ ◄──│  │
│  │    └──┘
└──┘";
        let fixed = fix(input);
        assert!(
            fixed.contains("◄───"),
            "tip should be extended 1 cell left: {fixed}"
        );
    }

    #[test]
    fn extend_down_1_cell() {
        let input = "\
┌──┐
│  │
└──┘
  │
  ▼

┌──┐
│  │
└──┘";
        let fixed = fix(input);
        let lines: Vec<&str> = fixed.lines().collect();
        // The ▼ should move down 1 cell to be adjacent to the bottom box
        assert!(
            lines.iter().any(|l| l.contains('▼')),
            "tip should still exist: {fixed}"
        );
        let diags = crate::lint_arrow_connect::ArrowConnectLint.check(&fixed);
        let disc: Vec<_> = diags
            .iter()
            .filter(|d| d.rule == "disconnected-arrow")
            .collect();
        assert!(
            disc.is_empty(),
            "should have no disconnected arrows after fix: {disc:?}"
        );
    }

    #[test]
    fn extend_up_1_cell() {
        let input = "\
┌──┐
│  │
└──┘

  ▲
  │
┌──┐
│  │
└──┘";
        let fixed = fix(input);
        let diags = crate::lint_arrow_connect::ArrowConnectLint.check(&fixed);
        let disc: Vec<_> = diags
            .iter()
            .filter(|d| d.rule == "disconnected-arrow")
            .collect();
        assert!(
            disc.is_empty(),
            "should have no disconnected arrows after fix: {disc:?}"
        );
    }

    #[test]
    fn extend_through_existing_trail() {
        // Tip has trail chars ahead (from malformed diagram), should still fix
        let input = "\
┌──┐
│  │──►─┌──┐
└──┘    │  │
        └──┘";
        let fixed = fix(input);
        assert!(
            fixed.contains("───►┌"),
            "tip should slide over trail char: {fixed}"
        );
    }

    // == Constraints ==========================================================

    #[test]
    fn skip_gap_too_large() {
        let input = "\
┌──┐
│  │──►   ┌──┐
└──┘      │  │
          └──┘";
        let fixed = fix(input);
        // 3-cell gap, unfixable
        assert_eq!(fixed, input, "should not fix gap > 2 cells");
    }

    #[test]
    fn skip_blocked_cell() {
        // ║ is line-drawing (not treated as text by adjacent_to_text) but is
        // not a valid horizontal trail char, so the extend path is blocked.
        let input = "\
┌──┐
│  │──►║┌──┐
└──┘   ║│  │
       ║└──┘";
        let fixed = fix(input);
        assert_eq!(fixed, input, "should not fix when path is blocked");
    }

    #[test]
    fn skip_text_arrow() {
        let input = "Hello ──► World";
        assert_eq!(fix(input), input, "should not fix text-to-text arrows");
    }

    #[test]
    fn direction_preserved() {
        let input = "\
┌──┐
│  │──► ┌──┐
└──┘    │  │
        └──┘";
        let fixed = fix(input);
        assert!(
            fixed.contains('►'),
            "arrow tip character should be preserved: {fixed}"
        );
    }

    // == Multiple arrows ======================================================

    #[test]
    fn fix_only_disconnected_arrows() {
        let input = "\
┌──┐
│  │──►┌──┐     ┌──┐
└──┘   │  │──► ─│  │
       └──┘     └──┘";
        let fixed = fix(input);
        // First arrow already connected — should be unchanged
        assert!(
            fixed.contains("──►┌──┐"),
            "connected arrow should be unchanged: {fixed}"
        );
    }

    // == Integration ==========================================================

    #[test]
    fn fix_then_lint_zero_disconnected() {
        let input = "\
┌──┐
│  │──► ┌──┐
└──┘    │  │
        └──┘";
        let fixed = fix(input);
        let diags = crate::lint_arrow_connect::ArrowConnectLint.check(&fixed);
        let disc: Vec<_> = diags
            .iter()
            .filter(|d| d.rule == "disconnected-arrow")
            .collect();
        assert!(
            disc.is_empty(),
            "fix then lint should produce zero disconnected-arrow: {disc:?}"
        );
    }

    // == Helper function coverage =============================================

    #[test]
    fn test_grid_helpers() {
        let grid = input_to_mut_grid("ab\ncde\nf");
        assert_eq!(grid.len(), 3);
        assert_eq!(grid_get(&grid, 0, 0), Some('a'));
        assert_eq!(grid_get(&grid, 1, 2), Some('e'));
        assert_eq!(grid_get(&grid, 2, 1), None);
        assert_eq!(grid_get(&grid, 5, 0), None);
        assert_eq!(mut_grid_to_string(&grid), "ab\ncde\nf");
    }

    #[test]
    fn test_ensure_row_width() {
        let mut grid = vec![vec!['a', 'b']];
        ensure_row_width(&mut grid, 0, 4);
        assert_eq!(grid[0].len(), 5);
        assert_eq!(grid[0][4], ' ');
        // Out of range row — no panic
        ensure_row_width(&mut grid, 5, 2);
    }

    #[test]
    fn test_endpoint_touches_box() {
        let bounds = BoundingRect {
            top_left: Position { row: 2, col: 3 },
            bottom_right: Position { row: 5, col: 8 },
        };
        assert_eq!(
            endpoint_touches_box(Position { row: 2, col: 5 }, &bounds),
            Some(Edge::Top)
        );
        assert_eq!(
            endpoint_touches_box(Position { row: 5, col: 5 }, &bounds),
            Some(Edge::Bottom)
        );
        assert_eq!(
            endpoint_touches_box(Position { row: 3, col: 3 }, &bounds),
            Some(Edge::Left)
        );
        assert_eq!(
            endpoint_touches_box(Position { row: 3, col: 8 }, &bounds),
            Some(Edge::Right)
        );
        // Adjacent
        assert_eq!(
            endpoint_touches_box(Position { row: 1, col: 5 }, &bounds),
            Some(Edge::Top)
        );
        assert_eq!(
            endpoint_touches_box(Position { row: 6, col: 5 }, &bounds),
            Some(Edge::Bottom)
        );
        assert_eq!(
            endpoint_touches_box(Position { row: 3, col: 2 }, &bounds),
            Some(Edge::Left)
        );
        assert_eq!(
            endpoint_touches_box(Position { row: 3, col: 9 }, &bounds),
            Some(Edge::Right)
        );
        // Out of range
        assert_eq!(
            endpoint_touches_box(Position { row: 0, col: 5 }, &bounds),
            None
        );
    }

    #[test]
    fn test_endpoint_touches_box_at_origin() {
        let bounds = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 2, col: 3 },
        };
        assert_eq!(
            endpoint_touches_box(Position { row: 0, col: 1 }, &bounds),
            Some(Edge::Top)
        );
        assert_eq!(
            endpoint_touches_box(Position { row: 1, col: 0 }, &bounds),
            Some(Edge::Left)
        );
    }

    #[test]
    fn test_tip_delta() {
        assert_eq!(tip_delta('►'), Some((0, 1)));
        assert_eq!(tip_delta('>'), Some((0, 1)));
        assert_eq!(tip_delta('◄'), Some((0, -1)));
        assert_eq!(tip_delta('<'), Some((0, -1)));
        assert_eq!(tip_delta('▼'), Some((1, 0)));
        assert_eq!(tip_delta('v'), Some((1, 0)));
        assert_eq!(tip_delta('▲'), Some((-1, 0)));
        assert_eq!(tip_delta('^'), Some((-1, 0)));
        assert_eq!(tip_delta('─'), None);
    }

    #[test]
    fn test_trail_char() {
        assert_eq!(trail_char(0, 1), '─');
        assert_eq!(trail_char(0, -1), '─');
        assert_eq!(trail_char(1, 0), '│');
        assert_eq!(trail_char(-1, 0), '│');
    }

    #[test]
    fn test_tip_direction_ok() {
        assert!(tip_direction_ok('►', Edge::Left));
        assert!(!tip_direction_ok('►', Edge::Right));
        assert!(tip_direction_ok('◄', Edge::Right));
        assert!(!tip_direction_ok('◄', Edge::Left));
        assert!(tip_direction_ok('▼', Edge::Top));
        assert!(!tip_direction_ok('▼', Edge::Bottom));
        assert!(tip_direction_ok('▲', Edge::Bottom));
        assert!(!tip_direction_ok('▲', Edge::Top));
        assert!(tip_direction_ok('>', Edge::Left));
        assert!(tip_direction_ok('<', Edge::Right));
        assert!(tip_direction_ok('v', Edge::Top));
        assert!(tip_direction_ok('^', Edge::Bottom));
        assert!(tip_direction_ok('─', Edge::Left));
    }

    #[test]
    fn test_adjacent_to_text() {
        let ir = DiagramIR::new("A►B");
        assert!(adjacent_to_text(&ir, Position { row: 0, col: 1 }));

        let ir2 = DiagramIR::new("► Text");
        assert!(adjacent_to_text(&ir2, Position { row: 0, col: 0 }));

        let ir3 = DiagramIR::new("►     Text");
        assert!(!adjacent_to_text(&ir3, Position { row: 0, col: 0 }));
    }

    #[test]
    fn test_adjacent_to_text_stops_at_line_drawing() {
        let ir = DiagramIR::new("►─Text");
        assert!(!adjacent_to_text(&ir, Position { row: 0, col: 0 }));
    }

    #[test]
    fn test_adjacent_to_text_at_origin() {
        let ir = DiagramIR::new("►    ");
        assert!(!adjacent_to_text(&ir, Position { row: 0, col: 0 }));
    }

    #[test]
    fn test_cell_overwritable() {
        assert!(cell_overwritable(Some(' '), '─'));
        assert!(cell_overwritable(None, '─'));
        assert!(cell_overwritable(Some('─'), '─'));
        assert!(!cell_overwritable(Some('─'), '│'));
        assert!(!cell_overwritable(Some('X'), '─'));
    }

    #[test]
    fn extend_at_grid_edge_no_panic() {
        // ◄ at col 0, far from any box — extending left would go negative
        let input = "\
◄

      ┌──┐
      │  │
      └──┘";
        let fixed = fix(input);
        assert_eq!(fixed, input);
    }

    #[test]
    fn fixer_name() {
        assert_eq!(ArrowConnectFixer.name(), "arrow-connect");
    }

    #[test]
    fn no_arrows_no_change() {
        let input = "\
┌──┐
│  │
└──┘";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn standalone_segment_no_tips() {
        let input = "\
┌──┐
│  │──────
└──┘";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn arrow_already_connected_no_change() {
        let input = "\
┌──┐
│  │──►┌──┐
└──┘   │  │──►┌──┐
       └──┘   │  │
              └──┘";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn extend_with_ascii_tip() {
        let input = "\
┌──┐
│  │--> ┌──┐
└──┘    │  │
        └──┘";
        let fixed = fix(input);
        // > should be extended
        let diags = crate::lint_arrow_connect::ArrowConnectLint.check(&fixed);
        let disc: Vec<_> = diags
            .iter()
            .filter(|d| d.rule == "disconnected-arrow")
            .collect();
        assert!(
            disc.is_empty(),
            "ASCII arrow should be fixed: {disc:?}\nfixed: {fixed}"
        );
    }

    #[test]
    fn direction_mismatch_already_connected_unchanged() {
        // ▼ points down but is adjacent to bottom edge — already "connected"
        // (lint fires arrow-direction, not disconnected-arrow). Fixer should
        // not alter it.
        let input = "\
┌──┐
│  │
└──┘
  ▼";
        let fixed = fix(input);
        assert_eq!(fixed, input, "direction-mismatch tip should be unchanged");
    }

    #[test]
    fn bidirectional_arrows() {
        let input = "\
┌──┐
│  │◄────►┌──┐
└──┘      │  │
          └──┘";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn extend_left_2_cells() {
        let input = "\
    ┌──┐
┌──┐  ◄──│  │
│  │     └──┘
└──┘";
        let fixed = fix(input);
        assert!(
            fixed.contains("◄────"),
            "tip should be extended 2 cells left: {fixed}"
        );
    }

    #[test]
    fn extend_down_2_cells() {
        let input = "\
┌──┐
│  │
└──┘
  │
  ▼


┌──┐
│  │
└──┘";
        let fixed = fix(input);
        let diags = crate::lint_arrow_connect::ArrowConnectLint.check(&fixed);
        let disc: Vec<_> = diags
            .iter()
            .filter(|d| d.rule == "disconnected-arrow")
            .collect();
        assert!(
            disc.is_empty(),
            "vertical arrow should be fixed: {disc:?}\nfixed: {fixed}"
        );
    }
}
