// Arrow and connector lint rules.
//
// Two rules:
// - `disconnected-arrow`: an arrow tip is not adjacent to any box edge
// - `arrow-direction`: an arrow tip faces away from the connected box

use crate::detect_arrows::detect_arrows;
use crate::detect_boxes::detect_boxes;
use crate::grid::{is_arrow_tip, is_line_drawing, BoundingRect, DiagramIR, Node, Position};
use crate::{Diagnostic, Level, LintRule};

pub struct ArrowConnectLint;

// ---------------------------------------------------------------------------
// Diagnostic helpers
// ---------------------------------------------------------------------------

fn diag(line: usize, col: usize, level: Level, message: String, rule: &str) -> Diagnostic {
    Diagnostic {
        file: String::new(),
        line,
        col,
        level,
        message,
        rule: rule.to_string(),
        fix: None,
    }
}

/// Convert 0-indexed grid position to 1-indexed diagnostic position.
fn pos(row: usize, col: usize) -> (usize, usize) {
    (row + 1, col + 1)
}

// ---------------------------------------------------------------------------
// Box edge adjacency
// ---------------------------------------------------------------------------

/// Which edge of a box an endpoint is adjacent to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

/// Check whether `p` is adjacent to (or on the edge of) a box's bounding rect.
/// Returns which edge it touches, or None.
fn endpoint_touches_box(p: Position, bounds: &BoundingRect) -> Option<Edge> {
    let tl = bounds.top_left;
    let br = bounds.bottom_right;

    // ON the top edge (junction/through-connection)
    if p.row == tl.row && p.col >= tl.col && p.col <= br.col {
        return Some(Edge::Top);
    }
    // ON the bottom edge
    if p.row == br.row && p.col >= tl.col && p.col <= br.col {
        return Some(Edge::Bottom);
    }
    // ON the left edge
    if p.col == tl.col && p.row >= tl.row && p.row <= br.row {
        return Some(Edge::Left);
    }
    // ON the right edge
    if p.col == br.col && p.row >= tl.row && p.row <= br.row {
        return Some(Edge::Right);
    }

    // ADJACENT: 1 cell outside the edge, within the box's span

    // 1 cell above top edge, within column span
    if p.row + 1 == tl.row && p.col >= tl.col && p.col <= br.col {
        return Some(Edge::Top);
    }
    // 1 cell below bottom edge, within column span
    if p.row == br.row + 1 && p.col >= tl.col && p.col <= br.col {
        return Some(Edge::Bottom);
    }
    // 1 cell left of left edge, within row span
    if p.col + 1 == tl.col && p.row >= tl.row && p.row <= br.row {
        return Some(Edge::Left);
    }
    // 1 cell right of right edge, within row span
    if p.col == br.col + 1 && p.row >= tl.row && p.row <= br.row {
        return Some(Edge::Right);
    }

    None
}

// ---------------------------------------------------------------------------
// Direction consistency
// ---------------------------------------------------------------------------

/// Check that a tip character's facing direction is consistent with the box
/// edge it connects to. A tip always points TOWARD the box it connects to,
/// regardless of whether it's at the start or end of the traced arrow path.
fn tip_direction_ok(tip_char: char, edge: Edge) -> bool {
    match tip_char {
        '►' | '>' => edge == Edge::Left, // points right → enters box on its left side
        '◄' | '<' => edge == Edge::Right,
        '▼' | 'v' => edge == Edge::Top,
        '▲' | '^' => edge == Edge::Bottom,
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// Text adjacency check (noise reduction)
// ---------------------------------------------------------------------------

/// Returns true if any cell within `radius` cells of `p` (in cardinal
/// directions) contains non-whitespace, non-line-drawing, non-arrow-tip text.
/// Used to suppress false positives on text-to-text arrows (like the runtime
/// data flow section where tips have a space before the next word).
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
            let nr = nr as usize;
            let nc = nc as usize;
            match ir.grid.get(nr, nc) {
                Some(ch) if !ch.is_whitespace() && !is_line_drawing(ch) && !is_arrow_tip(ch) => {
                    return true;
                }
                Some(ch) if ch.is_whitespace() => {
                    // Keep scanning through whitespace
                    continue;
                }
                _ => break, // line-drawing or arrow-tip or out of bounds → stop
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// LintRule implementation
// ---------------------------------------------------------------------------

impl LintRule for ArrowConnectLint {
    fn name(&self) -> &str {
        "arrow-connect"
    }

    fn check(&self, input: &str) -> Vec<Diagnostic> {
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);

        // Collect box bounds (only Box nodes exist at this point)
        let mut box_bounds: Vec<BoundingRect> = Vec::new();
        for node in &ir.nodes {
            if let Node::Box { bounds, .. } = node {
                box_bounds.push(*bounds);
            }
        }

        detect_arrows(&mut ir);

        let mut diagnostics = Vec::new();

        for node in &ir.nodes {
            let segments = match node {
                Node::Arrow { segments, .. } if !segments.is_empty() => segments,
                _ => continue,
            };

            // Check if this arrow has any tip characters on the grid
            let first_pos = segments[0].start;
            let last_pos = segments.last().unwrap().end;

            let first_char = ir.grid.get(first_pos.row, first_pos.col).unwrap_or(' ');
            let last_char = ir.grid.get(last_pos.row, last_pos.col).unwrap_or(' ');

            let has_tip = is_arrow_tip(first_char) || is_arrow_tip(last_char);
            if !has_tip {
                // Standalone segment (no tips) — skip
                continue;
            }

            // Check each tipped endpoint
            let endpoints: [(Position, char); 2] = [(first_pos, first_char), (last_pos, last_char)];

            for (ep_pos, ep_char) in &endpoints {
                if !is_arrow_tip(*ep_char) {
                    continue;
                }

                // Find if adjacent to any box
                let mut touching: Option<Edge> = None;
                for bounds in &box_bounds {
                    if let Some(edge) = endpoint_touches_box(*ep_pos, bounds) {
                        touching = Some(edge);
                        break;
                    }
                }

                let (line, col) = pos(ep_pos.row, ep_pos.col);

                match touching {
                    None => {
                        // Not adjacent to any box — check if adjacent to text
                        if !adjacent_to_text(&ir, *ep_pos) {
                            diagnostics.push(diag(
                                line,
                                col,
                                Level::Warning,
                                format!("arrow tip '{}' is not connected to any box", ep_char),
                                "disconnected-arrow",
                            ));
                        }
                    }
                    Some(edge) => {
                        // Adjacent to a box — check direction consistency
                        let ok = tip_direction_ok(*ep_char, edge);
                        if !ok {
                            let edge_name = match edge {
                                Edge::Top => "top",
                                Edge::Bottom => "bottom",
                                Edge::Left => "left",
                                Edge::Right => "right",
                            };
                            diagnostics.push(diag(
                                line,
                                col,
                                Level::Warning,
                                format!(
                                    "arrow tip '{}' faces wrong direction for {} edge of box",
                                    ep_char, edge_name
                                ),
                                "arrow-direction",
                            ));
                        }
                    }
                }
            }
        }

        diagnostics
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Direction;

    fn lint(input: &str) -> Vec<Diagnostic> {
        ArrowConnectLint.check(input)
    }

    // --- No false positives ---

    #[test]
    fn empty_input() {
        assert!(lint("").is_empty());
    }

    #[test]
    fn plain_text() {
        assert!(lint("Hello world\nNo diagrams here\n").is_empty());
    }

    #[test]
    fn well_connected_arrow_right() {
        let input = "\
┌──┐
│  │──►┌──┐
└──┘   │  │
       └──┘";
        let diags = lint(input);
        assert!(diags.is_empty(), "got: {diags:?}");
    }

    #[test]
    fn well_connected_arrow_down() {
        let input = "\
┌──┐
│  │
└──┘
  │
  ▼
┌──┐
│  │
└──┘";
        let diags = lint(input);
        assert!(diags.is_empty(), "got: {diags:?}");
    }

    #[test]
    fn through_edge_junction_arrow() {
        // Arrow goes through a junction on the box edge
        let input = "\
┌──┬──┐
│  │  │
└──┴──┘
   │
   ▼
┌──┐
│  │
└──┘";
        let diags = lint(input);
        assert!(diags.is_empty(), "got: {diags:?}");
    }

    #[test]
    fn bidirectional_arrow_between_boxes() {
        let input = "\
┌──┐
│  │
└──┘
  │
  ▼
┌──┐
│  │◄────►┌──┐
└──┘      │  │
          └──┘";
        let diags = lint(input);
        assert!(diags.is_empty(), "got: {diags:?}");
    }

    #[test]
    fn standalone_segment_no_tips() {
        // No tips = no diagnostics
        let input = "──────────";
        let diags = lint(input);
        assert!(diags.is_empty(), "got: {diags:?}");
    }

    #[test]
    fn text_to_text_arrow_suppressed() {
        // Arrows between text labels should not fire (adjacent to text)
        let input = "Browser ──POST──► Lambda";
        let diags = lint(input);
        assert!(diags.is_empty(), "got: {diags:?}");
    }

    // --- Detects issues ---

    #[test]
    fn disconnected_arrow_floating() {
        let input = "\
          ──────►


┌──┐
│  │
└──┘";
        let diags = lint(input);
        assert!(
            diags.iter().any(|d| d.rule == "disconnected-arrow"),
            "expected disconnected-arrow, got: {diags:?}"
        );
    }

    #[test]
    fn disconnected_arrow_isolated_tip() {
        let input = "\
►

┌──┐
│  │
└──┘";
        let diags = lint(input);
        assert!(
            diags.iter().any(|d| d.rule == "disconnected-arrow"),
            "expected disconnected-arrow, got: {diags:?}"
        );
    }

    #[test]
    fn arrow_wrong_direction_at_box() {
        // Arrow tip ◄ pointing left, but it's next to the LEFT edge of the box
        // (should point right to enter from the left)
        let input = "\
◄┌──┐
 │  │
 └──┘";
        let diags = lint(input);
        assert!(
            diags.iter().any(|d| d.rule == "arrow-direction"),
            "expected arrow-direction, got: {diags:?}"
        );
    }

    #[test]
    fn arrow_wrong_direction_vertical() {
        // ▲ pointing up, adjacent to top edge (should point down to enter from top)
        let input = "\
  ▲
┌──┐
│  │
└──┘";
        let diags = lint(input);
        assert!(
            diags.iter().any(|d| d.rule == "arrow-direction"),
            "expected arrow-direction, got: {diags:?}"
        );
    }

    #[test]
    fn one_end_connected_other_floating() {
        let input = "\
──────►┌──┐
       │  │
       └──┘";
        let diags = lint(input);
        // The ► end is connected, but the start has no tip so no diagnostic there.
        // This should produce no diagnostics since only tipped endpoints are checked.
        assert!(diags.is_empty(), "got: {diags:?}");
    }

    // --- Edge cases ---

    #[test]
    fn arrow_with_label_still_checks_endpoints() {
        let input = "\
──POST──►┌──┐
         │  │
         └──┘";
        let diags = lint(input);
        // ► is adjacent to box's left edge, direction is correct
        assert!(diags.is_empty(), "got: {diags:?}");
    }

    #[test]
    fn double_line_box_arrow() {
        let input = "\
╔══╗
║  ║──►╔══╗
╚══╝   ║  ║
       ╚══╝";
        let diags = lint(input);
        assert!(diags.is_empty(), "got: {diags:?}");
    }

    #[test]
    fn demo_diagram_reasonable_diagnostics() {
        let input = include_str!("../examples/demo-flow-diagram.txt");
        let diags = lint(input);
        // The demo diagram has several boxes with slight right-edge misalignment
        // (e.g. GitHub box ┐ at col 61 but │ at col 62 on content rows), so
        // detect_boxes can't find those boxes and arrows connecting to them
        // correctly report as disconnected. These are true positives, not
        // false positives.
        //
        // All diagnostics should be warnings (not errors).
        assert!(
            diags.iter().all(|d| d.level == Level::Warning),
            "expected only warnings, got: {diags:?}"
        );
        // The count should be reasonable — not hundreds.
        assert!(
            diags.len() < 15,
            "too many diagnostics ({}): {:?}",
            diags.len(),
            diags
        );
        // No arrow-direction diagnostics on the demo (all tips face correctly).
        assert!(
            !diags.iter().any(|d| d.rule == "arrow-direction"),
            "unexpected arrow-direction diagnostic in demo: {:?}",
            diags
                .iter()
                .filter(|d| d.rule == "arrow-direction")
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn rule_name() {
        assert_eq!(ArrowConnectLint.name(), "arrow-connect");
    }

    // --- Helper unit tests ---

    #[test]
    fn endpoint_touches_box_on_edges() {
        let bounds = BoundingRect {
            top_left: Position { row: 2, col: 3 },
            bottom_right: Position { row: 5, col: 8 },
        };
        // On top edge
        assert_eq!(
            endpoint_touches_box(Position { row: 2, col: 5 }, &bounds),
            Some(Edge::Top)
        );
        // On bottom edge
        assert_eq!(
            endpoint_touches_box(Position { row: 5, col: 5 }, &bounds),
            Some(Edge::Bottom)
        );
        // On left edge
        assert_eq!(
            endpoint_touches_box(Position { row: 3, col: 3 }, &bounds),
            Some(Edge::Left)
        );
        // On right edge
        assert_eq!(
            endpoint_touches_box(Position { row: 3, col: 8 }, &bounds),
            Some(Edge::Right)
        );
    }

    #[test]
    fn endpoint_touches_box_adjacent() {
        let bounds = BoundingRect {
            top_left: Position { row: 2, col: 3 },
            bottom_right: Position { row: 5, col: 8 },
        };
        // 1 cell above top
        assert_eq!(
            endpoint_touches_box(Position { row: 1, col: 5 }, &bounds),
            Some(Edge::Top)
        );
        // 1 cell below bottom
        assert_eq!(
            endpoint_touches_box(Position { row: 6, col: 5 }, &bounds),
            Some(Edge::Bottom)
        );
        // 1 cell left of left edge
        assert_eq!(
            endpoint_touches_box(Position { row: 3, col: 2 }, &bounds),
            Some(Edge::Left)
        );
        // 1 cell right of right edge
        assert_eq!(
            endpoint_touches_box(Position { row: 3, col: 9 }, &bounds),
            Some(Edge::Right)
        );
    }

    #[test]
    fn endpoint_touches_box_out_of_range() {
        let bounds = BoundingRect {
            top_left: Position { row: 2, col: 3 },
            bottom_right: Position { row: 5, col: 8 },
        };
        // Too far away
        assert_eq!(
            endpoint_touches_box(Position { row: 0, col: 5 }, &bounds),
            None
        );
        // Outside column span
        assert_eq!(
            endpoint_touches_box(Position { row: 1, col: 1 }, &bounds),
            None
        );
    }

    #[test]
    fn endpoint_touches_box_at_origin() {
        // Test with box at (0,0) — exercises underflow prevention
        let bounds = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 2, col: 3 },
        };
        // On top edge at origin
        assert_eq!(
            endpoint_touches_box(Position { row: 0, col: 1 }, &bounds),
            Some(Edge::Top)
        );
        // On left edge at origin
        assert_eq!(
            endpoint_touches_box(Position { row: 1, col: 0 }, &bounds),
            Some(Edge::Left)
        );
    }

    #[test]
    fn tip_direction_ok_cases() {
        assert!(tip_direction_ok('►', Edge::Left));
        assert!(!tip_direction_ok('►', Edge::Right));
        assert!(tip_direction_ok('◄', Edge::Right));
        assert!(!tip_direction_ok('◄', Edge::Left));
        assert!(tip_direction_ok('▼', Edge::Top));
        assert!(!tip_direction_ok('▼', Edge::Bottom));
        assert!(tip_direction_ok('▲', Edge::Bottom));
        assert!(!tip_direction_ok('▲', Edge::Top));
        // ASCII variants
        assert!(tip_direction_ok('>', Edge::Left));
        assert!(tip_direction_ok('<', Edge::Right));
        assert!(tip_direction_ok('v', Edge::Top));
        assert!(tip_direction_ok('^', Edge::Bottom));
        // Non-tip char
        assert!(tip_direction_ok('─', Edge::Left));
    }

    #[test]
    fn adjacent_to_text_helper() {
        let ir = DiagramIR::new("A►B");
        assert!(adjacent_to_text(&ir, Position { row: 0, col: 1 }));

        // Text within radius of 3 (space then text)
        let ir2 = DiagramIR::new("► Text");
        assert!(adjacent_to_text(&ir2, Position { row: 0, col: 0 }));

        // Too far away (>3 spaces)
        let ir3 = DiagramIR::new("►     Text");
        assert!(!adjacent_to_text(&ir3, Position { row: 0, col: 0 }));
    }

    #[test]
    fn adjacent_to_text_at_origin() {
        // Edge case: position at (0,0), no text nearby
        let ir = DiagramIR::new("►    ");
        assert!(!adjacent_to_text(&ir, Position { row: 0, col: 0 }));
    }

    #[test]
    fn adjacent_to_text_stops_at_line_drawing() {
        // Line-drawing char between tip and text stops the scan
        let ir = DiagramIR::new("►─Text");
        assert!(!adjacent_to_text(&ir, Position { row: 0, col: 0 }));
    }

    #[test]
    fn pos_helper() {
        assert_eq!(pos(0, 0), (1, 1));
        assert_eq!(pos(4, 9), (5, 10));
    }

    #[test]
    fn diag_helper() {
        let d = diag(1, 1, Level::Warning, "test".to_string(), "test-rule");
        assert_eq!(d.rule, "test-rule");
        assert_eq!(d.file, "");
    }

    // --- Segment alignment invariant tests ---
    // Rules 3-4 from the story (horizontal segments share same row, vertical
    // share same column) are inherently satisfied by the arrow tracer which
    // only produces axis-aligned segments.

    #[test]
    fn segments_are_axis_aligned() {
        let input = include_str!("../examples/demo-flow-diagram.txt");
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);
        detect_arrows(&mut ir);
        for node in &ir.nodes {
            if let Node::Arrow { segments, .. } = node {
                for seg in segments {
                    let same_row = seg.start.row == seg.end.row;
                    let same_col = seg.start.col == seg.end.col;
                    assert!(
                        same_row || same_col,
                        "segment is not axis-aligned: {:?}",
                        seg
                    );
                    if same_row {
                        // Horizontal: direction must be Left or Right
                        assert!(
                            matches!(seg.direction, Direction::Left | Direction::Right),
                            "horizontal segment has vertical direction: {:?}",
                            seg
                        );
                    } else {
                        // Vertical: direction must be Up or Down
                        assert!(
                            matches!(seg.direction, Direction::Up | Direction::Down),
                            "vertical segment has horizontal direction: {:?}",
                            seg
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn arrow_tip_away_from_box_disconnected() {
        // ◄ pointing left, but it's far from the box (3 cells gap).
        // The tracer traces from ◄ right through ──, stops at ┌ (corner).
        // ◄ ends up as the last endpoint. It's not adjacent to any box → disconnected.
        let input = "\
◄──┌──┐
   │  │
   └──┘";
        let diags = lint(input);
        let has_disconnected = diags.iter().any(|d| d.rule == "disconnected-arrow");
        assert!(
            has_disconnected,
            "expected disconnected-arrow for ◄ far from box, got: {diags:?}"
        );
    }

    #[test]
    fn arrow_connects_to_box_left_edge_correctly() {
        // Arrow comes from the left and tip ► enters box on its left edge
        let input = "\
──────►┌──┐
       │  │
       └──┘";
        let diags = lint(input);
        assert!(diags.is_empty(), "got: {diags:?}");
    }

    #[test]
    fn upward_arrow_to_box_bottom() {
        let input = "\
┌──┐
│  │
└──┘
  │
  │
  ▲";
        let diags = lint(input);
        // ▲ is at the end of the arrow (tracer finds it as tip, traces down,
        // reverses). ▲ at end, adjacent to... nothing (it's below the box,
        // far away). Check distance.
        // Actually the ▲ is 3 rows below the box bottom (row 2). Not adjacent.
        // So it should fire disconnected-arrow.
        assert!(
            diags.iter().any(|d| d.rule == "disconnected-arrow"),
            "expected disconnected-arrow for ▲ far from box, got: {diags:?}"
        );
    }

    #[test]
    fn arrow_no_segments() {
        // Edge case: ensure no panic with empty arrow
        let diags = lint("   ");
        assert!(diags.is_empty());
    }

    // Coverage: arrow-direction on bottom edge
    #[test]
    fn arrow_wrong_direction_at_bottom_edge() {
        // ▼ pointing down, adjacent to bottom edge (should point up to enter from bottom)
        let input = "\
┌──┐
│  │
└──┘
  ▼";
        let diags = lint(input);
        assert!(
            diags
                .iter()
                .any(|d| d.rule == "arrow-direction" && d.message.contains("bottom")),
            "expected arrow-direction for bottom edge, got: {diags:?}"
        );
    }

    // Coverage: arrow-direction on right edge
    #[test]
    fn arrow_wrong_direction_at_right_edge() {
        // ► pointing right, adjacent to right edge (should point left to enter from right)
        let input = "\
┌──┐
│  │►
└──┘";
        let diags = lint(input);
        assert!(
            diags
                .iter()
                .any(|d| d.rule == "arrow-direction" && d.message.contains("right")),
            "expected arrow-direction for right edge, got: {diags:?}"
        );
    }
}
