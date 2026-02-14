// Free-standing text label detection for box-drawing diagrams.
//
// Scans a `DiagramIR` grid for horizontal runs of non-structural characters
// (not whitespace, not line-drawing, not arrow tips) that are outside any
// detected box or arrow segment. Each run becomes a `Node::Text` entry.

use std::collections::HashSet;

use crate::grid::{is_arrow_tip, is_line_drawing, DiagramIR, Node, Position};

// ---------------------------------------------------------------------------
// Occupied-cell computation
// ---------------------------------------------------------------------------

/// Build a set of grid positions occupied by existing box and arrow nodes.
///
/// Box nodes occupy their entire bounding rectangle (corners + edges +
/// interior). Arrow nodes occupy every cell along each segment.
fn build_occupied(ir: &DiagramIR) -> HashSet<(usize, usize)> {
    let mut occupied = HashSet::new();

    for node in &ir.nodes {
        match node {
            Node::Box { bounds, .. } => {
                for r in bounds.top_left.row..=bounds.bottom_right.row {
                    for c in bounds.top_left.col..=bounds.bottom_right.col {
                        occupied.insert((r, c));
                    }
                }
            }
            Node::Arrow { segments, label: _ } => {
                for seg in segments {
                    add_segment_cells(&mut occupied, seg);
                }
            }
            Node::Text { .. } => {}
        }
    }

    occupied
}

/// Insert every cell along a segment into the occupied set.
fn add_segment_cells(occupied: &mut HashSet<(usize, usize)>, seg: &crate::grid::Segment) {
    let r1 = seg.start.row.min(seg.end.row);
    let r2 = seg.start.row.max(seg.end.row);
    let c1 = seg.start.col.min(seg.end.col);
    let c2 = seg.start.col.max(seg.end.col);

    for r in r1..=r2 {
        for c in c1..=c2 {
            occupied.insert((r, c));
        }
    }
}

// ---------------------------------------------------------------------------
// Label scanning
// ---------------------------------------------------------------------------

/// Detect free-standing text labels and append `Node::Text` entries to
/// `ir.nodes`. Must be called after `detect_boxes` and `detect_arrows`.
pub fn detect_labels(ir: &mut DiagramIR) {
    let rows = ir.grid.rows();
    let cols = ir.grid.cols();
    if rows == 0 || cols == 0 {
        return;
    }

    let occupied = build_occupied(ir);
    let mut labels: Vec<Node> = Vec::new();

    for r in 0..rows {
        let mut c = 0;
        while c < cols {
            let ch = ir.grid.get(r, c).unwrap_or(' ');

            // Skip whitespace, line-drawing, arrow tips, and occupied cells.
            if ch.is_whitespace()
                || is_line_drawing(ch)
                || is_arrow_tip(ch)
                || occupied.contains(&(r, c))
            {
                c += 1;
                continue;
            }

            // Start of a text run.
            let start_col = c;
            let mut text = String::new();
            while c < cols {
                let ch2 = ir.grid.get(r, c).unwrap_or(' ');
                if is_line_drawing(ch2) || is_arrow_tip(ch2) || occupied.contains(&(r, c)) {
                    break;
                }
                if ch2.is_whitespace() {
                    // Peek ahead: if there are more text chars after whitespace
                    // (before a line-drawing/arrow/occupied/end-of-row), include
                    // the space as part of this label.
                    if has_more_text(ir, &occupied, r, c + 1, cols) {
                        text.push(ch2);
                        c += 1;
                        continue;
                    }
                    break;
                }
                text.push(ch2);
                c += 1;
            }

            let trimmed = text.trim();
            if !trimmed.is_empty() {
                // Adjust start_col for leading whitespace we may have consumed.
                let leading = text.len() - text.trim_start().len();
                labels.push(Node::Text {
                    position: Position {
                        row: r,
                        col: start_col + leading,
                    },
                    content: trimmed.to_string(),
                });
            }

            // c is already past the run; continue outer loop.
        }
    }

    ir.nodes.extend(labels);
}

/// Returns true if there is at least one non-whitespace text character
/// between `from_col` and the end of the scannable region (before a
/// line-drawing char, arrow tip, occupied cell, or end of row).
fn has_more_text(
    ir: &DiagramIR,
    occupied: &HashSet<(usize, usize)>,
    row: usize,
    from_col: usize,
    cols: usize,
) -> bool {
    let mut c = from_col;
    while c < cols {
        let ch = ir.grid.get(row, c).unwrap_or(' ');
        if is_line_drawing(ch) || is_arrow_tip(ch) || occupied.contains(&(row, c)) {
            return false;
        }
        if !ch.is_whitespace() {
            return true;
        }
        c += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect_arrows::detect_arrows;
    use crate::detect_boxes::detect_boxes;

    /// Helper: run full detection pipeline and return the IR.
    fn detect(input: &str) -> DiagramIR {
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);
        detect_arrows(&mut ir);
        detect_labels(&mut ir);
        ir
    }

    /// Helper: return only Text nodes.
    fn texts(ir: &DiagramIR) -> Vec<(&Position, &str)> {
        ir.nodes
            .iter()
            .filter_map(|n| match n {
                Node::Text { position, content } => Some((position, content.as_str())),
                _ => None,
            })
            .collect()
    }

    // -- Basic cases -------------------------------------------------------

    #[test]
    fn empty_input_no_labels() {
        let ir = detect("");
        assert!(texts(&ir).is_empty());
    }

    #[test]
    fn plain_text_detected() {
        let ir = detect("Hello World");
        let t = texts(&ir);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].1, "Hello World");
        assert_eq!(t[0].0, &Position { row: 0, col: 0 });
    }

    #[test]
    fn multiple_lines_of_text() {
        let ir = detect("ALPHA\nBRAVO");
        let t = texts(&ir);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].1, "ALPHA");
        assert_eq!(t[1].1, "BRAVO");
    }

    // -- Exclusion cases ---------------------------------------------------

    #[test]
    fn text_inside_box_excluded() {
        let input = "┌─────┐\n│Hello│\n└─────┘";
        let ir = detect(input);
        let t = texts(&ir);
        assert!(t.is_empty(), "text inside box should not be a label");
    }

    #[test]
    fn section_header_near_box_detected() {
        let input = "HEADER\n┌──┐\n│  │\n└──┘";
        let ir = detect(input);
        let t = texts(&ir);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].1, "HEADER");
    }

    #[test]
    fn arrow_label_not_double_detected() {
        let input = "──POST──►";
        let ir = detect(input);
        let t = texts(&ir);
        // POST is consumed by the arrow detector as a label, so it should
        // not appear as a separate text node.
        assert!(
            t.is_empty(),
            "arrow inline label should not be a text node, got: {t:?}"
        );
    }

    #[test]
    fn line_drawing_underlines_excluded() {
        let input = "HEADER\n══════";
        let ir = detect(input);
        let t = texts(&ir);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].1, "HEADER");
    }

    #[test]
    fn arrow_tips_excluded() {
        let ir = detect("►");
        let t = texts(&ir);
        assert!(t.is_empty(), "arrow tip should not be a text node");
    }

    #[test]
    fn isolated_single_character() {
        let ir = detect("X");
        let t = texts(&ir);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].1, "X");
    }

    // -- Occupied cell building --------------------------------------------

    #[test]
    fn build_occupied_includes_box_bounds() {
        let mut ir = DiagramIR::new("┌──┐\n│  │\n└──┘");
        detect_boxes(&mut ir);
        let occ = build_occupied(&ir);
        // All cells in the 3x4 box should be occupied.
        for r in 0..3 {
            for c in 0..4 {
                assert!(occ.contains(&(r, c)), "({r},{c}) should be occupied");
            }
        }
    }

    #[test]
    fn build_occupied_includes_arrow_segments() {
        let mut ir = DiagramIR::new("──────►");
        detect_arrows(&mut ir);
        let occ = build_occupied(&ir);
        // Every cell in the arrow should be occupied.
        for c in 0..7 {
            assert!(occ.contains(&(0, c)), "(0,{c}) should be occupied");
        }
    }

    #[test]
    fn build_occupied_empty_ir() {
        let ir = DiagramIR::new("");
        let occ = build_occupied(&ir);
        assert!(occ.is_empty());
    }

    #[test]
    fn build_occupied_text_nodes_ignored() {
        let mut ir = DiagramIR::new("Hello");
        ir.nodes.push(Node::Text {
            position: Position { row: 0, col: 0 },
            content: "Hello".to_string(),
        });
        let occ = build_occupied(&ir);
        // Text nodes should not occupy cells.
        assert!(occ.is_empty());
    }

    // -- Demo diagram ------------------------------------------------------

    #[test]
    fn demo_diagram_text_labels() {
        let input = include_str!("../examples/demo-flow-diagram.txt");
        let ir = detect(input);
        let t = texts(&ir);

        // The demo has many text labels: "INTENT LAYER", "AUTHORITY LAYER",
        // "CLOUD LAYER", "cub unit update", "CMP plugin fetches", etc.
        assert!(
            t.len() >= 5,
            "expected at least 5 text labels in demo diagram, found {}",
            t.len()
        );

        // Check for some specific labels.
        let contents: Vec<&str> = t.iter().map(|x| x.1).collect();
        assert!(
            contents.iter().any(|s| s.contains("INTENT LAYER")),
            "expected INTENT LAYER label, got: {contents:?}"
        );
        assert!(
            contents.iter().any(|s| s.contains("AUTHORITY LAYER")),
            "expected AUTHORITY LAYER label"
        );
        assert!(
            contents.iter().any(|s| s.contains("cub unit update")),
            "expected 'cub unit update' label"
        );
    }

    // -- Edge cases --------------------------------------------------------

    #[test]
    fn whitespace_only_input() {
        let ir = detect("   \n   ");
        assert!(texts(&ir).is_empty());
    }

    #[test]
    fn text_adjacent_to_line_drawing() {
        let ir = detect("Hello──World");
        let t = texts(&ir);
        // "Hello" and "World" should be separate labels split by ──
        assert_eq!(t.len(), 2, "got: {t:?}");
        assert_eq!(t[0].1, "Hello");
        assert_eq!(t[1].1, "World");
    }

    #[test]
    fn text_with_leading_spaces() {
        let ir = detect("   INDENTED");
        let t = texts(&ir);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].1, "INDENTED");
        assert_eq!(t[0].0.col, 3);
    }

    #[test]
    fn has_more_text_helper_empty() {
        let ir = DiagramIR::new("   ");
        let occ = HashSet::new();
        assert!(!has_more_text(&ir, &occ, 0, 0, 3));
    }

    #[test]
    fn has_more_text_helper_found() {
        let ir = DiagramIR::new("  X");
        let occ = HashSet::new();
        assert!(has_more_text(&ir, &occ, 0, 0, 3));
    }

    #[test]
    fn has_more_text_blocked_by_line_drawing() {
        let ir = DiagramIR::new(" ─X");
        let occ = HashSet::new();
        // The ─ at col 1 blocks scanning before reaching X
        assert!(!has_more_text(&ir, &occ, 0, 0, 3));
    }

    #[test]
    fn has_more_text_blocked_by_occupied() {
        let ir = DiagramIR::new("  X");
        let mut occ = HashSet::new();
        occ.insert((0, 1));
        // Occupied cell at col 1 blocks scanning before reaching X at col 2
        assert!(!has_more_text(&ir, &occ, 0, 0, 3));
    }

    #[test]
    fn add_segment_cells_covers_range() {
        use crate::grid::{Direction, Segment};
        let mut occ = HashSet::new();
        let seg = Segment {
            start: Position { row: 2, col: 5 },
            end: Position { row: 2, col: 8 },
            direction: Direction::Right,
        };
        add_segment_cells(&mut occ, &seg);
        for c in 5..=8 {
            assert!(occ.contains(&(2, c)));
        }
        assert!(!occ.contains(&(2, 4)));
        assert!(!occ.contains(&(2, 9)));
    }

    #[test]
    fn add_segment_cells_reversed_endpoints() {
        use crate::grid::{Direction, Segment};
        let mut occ = HashSet::new();
        // Segment with start > end (e.g., leftward arrow)
        let seg = Segment {
            start: Position { row: 0, col: 5 },
            end: Position { row: 0, col: 2 },
            direction: Direction::Left,
        };
        add_segment_cells(&mut occ, &seg);
        for c in 2..=5 {
            assert!(occ.contains(&(0, c)));
        }
    }

    #[test]
    fn detect_labels_empty_grid() {
        let mut ir = DiagramIR::new("");
        detect_labels(&mut ir);
        assert!(ir.nodes.is_empty());
    }
}
