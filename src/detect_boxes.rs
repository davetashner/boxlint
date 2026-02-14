// Box detection pass: scans the grid for rectangular box-drawing structures
// (both single-line ┌─┐│└┘ and double-line ╔═╗║╚╝) and pushes Node::Box into
// the DiagramIR nodes list.

use crate::grid::{BoundingRect, DiagramIR, Node, Position};

/// Scans the grid for box-drawing rectangles and appends detected boxes as
/// `Node::Box` entries in `ir.nodes`.
///
/// Handles single-line boxes (┌┐└┘ with ─│ edges), double-line boxes
/// (╔╗╚╝ with ═║ edges), nested boxes, adjacent boxes sharing edges,
/// and boxes with junction characters on their edges.
pub fn detect_boxes(ir: &mut DiagramIR) {
    let rows = ir.grid.rows();
    let cols = ir.grid.cols();

    for r in 0..rows {
        for c in 0..cols {
            if let Some(ch) = ir.grid.get(r, c) {
                match ch {
                    '┌' => {
                        if let Some(node) = try_detect_single_box(&ir.grid, r, c) {
                            ir.nodes.push(node);
                        }
                    }
                    '╔' => {
                        if let Some(node) = try_detect_double_box(&ir.grid, r, c) {
                            ir.nodes.push(node);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Returns true if `ch` is valid on the top edge of a single-line box
/// (horizontal edge or junction where an arrow connects from above/below).
fn is_top_edge_char(ch: char) -> bool {
    matches!(ch, '─' | '┬' | '┴' | '┼')
}

/// Returns true if `ch` is valid on the bottom edge of a single-line box.
fn is_bottom_edge_char(ch: char) -> bool {
    matches!(ch, '─' | '┬' | '┴' | '┼')
}

/// Returns true if `ch` is valid on the left edge of a single-line box.
fn is_left_edge_char(ch: char) -> bool {
    matches!(ch, '│' | '├' | '┤' | '┼')
}

/// Returns true if `ch` is valid on the right edge of a single-line box.
fn is_right_edge_char(ch: char) -> bool {
    matches!(ch, '│' | '├' | '┤' | '┼')
}

/// Try to detect a single-line box starting at top-left corner (r, c) which
/// must contain '┌'.
fn try_detect_single_box(grid: &crate::grid::Grid, r: usize, c: usize) -> Option<Node> {
    let cols = grid.cols();
    let rows = grid.rows();

    // Scan right along top edge to find ┐
    let mut c2 = c + 1;
    while c2 < cols {
        let ch = grid.get(r, c2)?;
        if ch == '┐' {
            break;
        }
        if !is_top_edge_char(ch) {
            return None;
        }
        c2 += 1;
    }
    if c2 >= cols || grid.get(r, c2)? != '┐' {
        return None;
    }
    // Must be at least width 3 (corners + 1 edge char) to avoid degenerate
    if c2 <= c + 1 {
        return None;
    }

    // Scan down from top-left to find └
    let mut r2 = r + 1;
    while r2 < rows {
        let ch = grid.get(r2, c)?;
        if ch == '└' {
            break;
        }
        if !is_left_edge_char(ch) {
            return None;
        }
        r2 += 1;
    }
    if r2 >= rows || grid.get(r2, c)? != '└' {
        return None;
    }
    // Must be at least height 3
    if r2 <= r + 1 {
        return None;
    }

    // Verify bottom-right corner is ┘
    if grid.get(r2, c2)? != '┘' {
        return None;
    }

    // Verify bottom edge
    for col in (c + 1)..c2 {
        let ch = grid.get(r2, col)?;
        if !is_bottom_edge_char(ch) {
            return None;
        }
    }

    // Verify right edge
    for row in (r + 1)..r2 {
        let ch = grid.get(row, c2)?;
        if !is_right_edge_char(ch) {
            return None;
        }
    }

    // Extract content
    let content = extract_content(grid, r, c, r2, c2);

    Some(Node::Box {
        bounds: BoundingRect {
            top_left: Position { row: r, col: c },
            bottom_right: Position { row: r2, col: c2 },
        },
        content,
    })
}

// ---------------------------------------------------------------------------
// Double-line box detection (╔═╗║╚╝)
// ---------------------------------------------------------------------------

fn is_double_top_edge(ch: char) -> bool {
    matches!(ch, '═' | '╦' | '╩' | '╬')
}

fn is_double_bottom_edge(ch: char) -> bool {
    matches!(ch, '═' | '╦' | '╩' | '╬')
}

fn is_double_left_edge(ch: char) -> bool {
    matches!(ch, '║' | '╠' | '╣' | '╬')
}

fn is_double_right_edge(ch: char) -> bool {
    matches!(ch, '║' | '╠' | '╣' | '╬')
}

fn try_detect_double_box(grid: &crate::grid::Grid, r: usize, c: usize) -> Option<Node> {
    let cols = grid.cols();
    let rows = grid.rows();

    // Scan right for ╗
    let mut c2 = c + 1;
    while c2 < cols {
        let ch = grid.get(r, c2)?;
        if ch == '╗' {
            break;
        }
        if !is_double_top_edge(ch) {
            return None;
        }
        c2 += 1;
    }
    if c2 >= cols || grid.get(r, c2)? != '╗' {
        return None;
    }
    if c2 <= c + 1 {
        return None;
    }

    // Scan down for ╚
    let mut r2 = r + 1;
    while r2 < rows {
        let ch = grid.get(r2, c)?;
        if ch == '╚' {
            break;
        }
        if !is_double_left_edge(ch) {
            return None;
        }
        r2 += 1;
    }
    if r2 >= rows || grid.get(r2, c)? != '╚' {
        return None;
    }
    if r2 <= r + 1 {
        return None;
    }

    // Verify ╝
    if grid.get(r2, c2)? != '╝' {
        return None;
    }

    // Bottom edge
    for col in (c + 1)..c2 {
        let ch = grid.get(r2, col)?;
        if !is_double_bottom_edge(ch) {
            return None;
        }
    }

    // Right edge
    for row in (r + 1)..r2 {
        let ch = grid.get(row, c2)?;
        if !is_double_right_edge(ch) {
            return None;
        }
    }

    let content = extract_content(grid, r, c, r2, c2);

    Some(Node::Box {
        bounds: BoundingRect {
            top_left: Position { row: r, col: c },
            bottom_right: Position { row: r2, col: c2 },
        },
        content,
    })
}

// ---------------------------------------------------------------------------
// Content extraction
// ---------------------------------------------------------------------------

/// Extract the interior text of a box (everything between the edges, exclusive
/// of the border characters themselves). Each interior row becomes one String.
fn extract_content(
    grid: &crate::grid::Grid,
    r: usize,
    c: usize,
    r2: usize,
    c2: usize,
) -> Vec<String> {
    let mut content = Vec::new();
    for row in (r + 1)..r2 {
        let mut line = String::new();
        for col in (c + 1)..c2 {
            if let Some(ch) = grid.get(row, col) {
                line.push(ch);
            }
        }
        content.push(line);
    }
    content
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::DiagramIR;

    // 1. Single simple box
    #[test]
    fn simple_box() {
        let input = "┌─┐\n│ │\n└─┘";
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);
        assert_eq!(ir.nodes.len(), 1);
        match &ir.nodes[0] {
            Node::Box { bounds, content } => {
                assert_eq!(bounds.top_left, Position { row: 0, col: 0 });
                assert_eq!(bounds.bottom_right, Position { row: 2, col: 2 });
                assert_eq!(content, &vec![" ".to_string()]);
            }
            _ => panic!("expected Node::Box"),
        }
    }

    // 2. Box with content text
    #[test]
    fn box_with_content() {
        let input = "\
┌───────┐
│ Hello │
│ World │
└───────┘";
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);
        assert_eq!(ir.nodes.len(), 1);
        match &ir.nodes[0] {
            Node::Box { content, .. } => {
                assert_eq!(content.len(), 2);
                assert!(content[0].contains("Hello"));
                assert!(content[1].contains("World"));
            }
            _ => panic!("expected Node::Box"),
        }
    }

    // 3. Nested boxes
    #[test]
    fn nested_boxes() {
        let input = "\
┌──────────────┐
│ ┌──────────┐ │
│ │  inner   │ │
│ └──────────┘ │
└──────────────┘";
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);
        assert_eq!(ir.nodes.len(), 2, "expected outer + inner box");
        // The outer box is detected first (top-left scan order)
        let outer = &ir.nodes[0];
        let inner = &ir.nodes[1];
        match (outer, inner) {
            (Node::Box { bounds: ob, .. }, Node::Box { bounds: ib, .. }) => {
                assert_eq!(ob.top_left, Position { row: 0, col: 0 });
                assert_eq!(ob.bottom_right, Position { row: 4, col: 15 });
                assert_eq!(ib.top_left, Position { row: 1, col: 2 });
                assert_eq!(ib.bottom_right, Position { row: 3, col: 13 });
            }
            _ => panic!("expected two Node::Box"),
        }
    }

    // 4. Adjacent boxes (side by side)
    #[test]
    fn adjacent_boxes() {
        let input = "\
┌───┐┌───┐
│ A ││ B │
└───┘└───┘";
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);
        assert_eq!(ir.nodes.len(), 2, "expected two adjacent boxes");
    }

    // 5. Double-line box
    #[test]
    fn double_line_box() {
        let input = "╔═╗\n║ ║\n╚═╝";
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);
        assert_eq!(ir.nodes.len(), 1);
        match &ir.nodes[0] {
            Node::Box { bounds, content } => {
                assert_eq!(bounds.top_left, Position { row: 0, col: 0 });
                assert_eq!(bounds.bottom_right, Position { row: 2, col: 2 });
                assert_eq!(content, &vec![" ".to_string()]);
            }
            _ => panic!("expected Node::Box"),
        }
    }

    // 6. Box with junction chars on edges (where arrows connect)
    #[test]
    fn box_with_junctions() {
        let input = "\
┌───┬───┐
│   │   │
├───┼───┤
│   │   │
└───┴───┘";
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);
        // The outer rectangle should be detected as a single box
        assert!(
            !ir.nodes.is_empty(),
            "expected at least the outer box with junction edges"
        );
        // The full outer box (0,0)→(4,8) should be present
        let has_outer = ir.nodes.iter().any(|n| match n {
            Node::Box { bounds, .. } => {
                bounds.top_left == Position { row: 0, col: 0 }
                    && bounds.bottom_right == Position { row: 4, col: 8 }
            }
            _ => false,
        });
        assert!(has_outer, "outer box with junctions should be detected");
    }

    // 7. Demo diagram: verify box detection on real diagram.
    // The demo diagram has intentional alignment bugs (the purpose of boxlint
    // is to detect these), so only properly-formed boxes are found.
    #[test]
    fn demo_diagram_box_count() {
        let input = include_str!("../examples/demo-flow-diagram.txt");
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);
        let count = ir.nodes.len();
        assert!(
            count >= 3,
            "expected at least 3 well-formed boxes in demo diagram, found {count}"
        );
        // Verify the title box is detected
        let has_title_box = ir.nodes.iter().any(|n| match n {
            Node::Box { bounds, .. } => bounds.top_left == Position { row: 1, col: 0 },
            _ => false,
        });
        assert!(has_title_box, "title box should be detected");
    }

    // 8. Empty grid — no boxes
    #[test]
    fn empty_grid_no_boxes() {
        let mut ir = DiagramIR::new("");
        detect_boxes(&mut ir);
        assert!(ir.nodes.is_empty());
    }

    // 9. Incomplete box — missing corner
    #[test]
    fn incomplete_box_not_detected() {
        let input = "\
┌───┐
│   │
└───x";
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);
        assert!(ir.nodes.is_empty(), "incomplete box should not be detected");
    }

    // Extra: wider box (like the title box)
    #[test]
    fn wide_box() {
        let input = "\
┌──────────────────────────────────────────────────────────────────────────────────────┐
│                      Serverless Message Wall — End-to-End Flow                       │
└──────────────────────────────────────────────────────────────────────────────────────┘";
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);
        assert_eq!(ir.nodes.len(), 1);
        match &ir.nodes[0] {
            Node::Box { content, .. } => {
                assert!(content[0].contains("Serverless"));
            }
            _ => panic!("expected Node::Box"),
        }
    }

    // Extra: plain text with no box chars
    #[test]
    fn plain_text_no_boxes() {
        let input = "Hello world\nThis is just text\n";
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);
        assert!(ir.nodes.is_empty());
    }

    // Extra: degenerate box (height 2, no interior) should not be detected
    #[test]
    fn degenerate_height_box_skipped() {
        let input = "┌──┐\n└──┘";
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);
        // Height is only 2 rows (r=0, r2=1) which means r2 <= r+1, so skipped
        assert!(ir.nodes.is_empty(), "degenerate box should be skipped");
    }
}
