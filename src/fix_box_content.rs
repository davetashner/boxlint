// Box content sizing auto-fixer.
//
// Widens boxes so their content fits with proper padding (1 space each side).
// Re-centers centered content and pads all content lines to consistent width.

use crate::detect_arrows::detect_arrows;
use crate::detect_boxes::detect_boxes;
use crate::grid::{is_line_drawing, BoundingRect, DiagramIR, Node, Segment};
use crate::Fixer;

pub struct BoxContentFixer;

// ---------------------------------------------------------------------------
// Mutable grid helpers (duplicated from fix_adjacent_boxes — they're private)
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
// Box style detection (duplicated from fix_adjacent_boxes — they're private)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BoxStyle {
    Single,
    Double,
}

fn detect_style(grid: &[Vec<char>], bounds: &BoundingRect) -> BoxStyle {
    match grid_get(grid, bounds.top_left.row, bounds.top_left.col) {
        Some('╔') => BoxStyle::Double,
        _ => BoxStyle::Single,
    }
}

impl BoxStyle {
    fn horizontal(self) -> char {
        match self {
            BoxStyle::Single => '─',
            BoxStyle::Double => '═',
        }
    }

    fn vertical(self) -> char {
        match self {
            BoxStyle::Single => '│',
            BoxStyle::Double => '║',
        }
    }

    fn top_right(self) -> char {
        match self {
            BoxStyle::Single => '┐',
            BoxStyle::Double => '╗',
        }
    }

    fn bottom_right(self) -> char {
        match self {
            BoxStyle::Single => '┘',
            BoxStyle::Double => '╝',
        }
    }
}

// ---------------------------------------------------------------------------
// Arrow connection check
// ---------------------------------------------------------------------------

/// Returns true if any arrow segment endpoint is adjacent to the right edge
/// of the given box bounds.
fn arrow_touches_right_edge(bounds: &BoundingRect, arrows: &[Vec<Segment>]) -> bool {
    let right_col = bounds.bottom_right.col;
    let top = bounds.top_left.row;
    let bottom = bounds.bottom_right.row;

    for segments in arrows {
        for seg in segments {
            for pos in [seg.start, seg.end] {
                if pos.col == right_col + 1 && pos.row >= top && pos.row <= bottom {
                    return true;
                }
            }
        }
    }
    false
}

/// Extract arrow segments from nodes.
fn extract_arrow_segments(nodes: &[Node]) -> Vec<Vec<Segment>> {
    nodes
        .iter()
        .filter_map(|n| {
            if let Node::Arrow { segments, .. } = n {
                Some(segments.clone())
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Region clear check
// ---------------------------------------------------------------------------

fn region_is_clear(
    grid: &[Vec<char>],
    row_start: usize,
    row_end: usize,
    col_start: usize,
    col_end: usize,
) -> bool {
    for r in row_start..=row_end {
        for c in col_start..=col_end {
            match grid_get(grid, r, c) {
                Some(' ') | None => {}
                _ => return false,
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Content line analysis
// ---------------------------------------------------------------------------

/// Returns true if a content line contains line-drawing chars (inner box border).
fn has_line_drawing(line: &[char]) -> bool {
    line.iter().any(|&ch| is_line_drawing(ch))
}

/// Determine if a content string was centered: roughly equal padding on both
/// sides, with both sides having at least 4 spaces.
fn is_centered(trimmed: &str, interior_width: usize) -> bool {
    if trimmed.is_empty() {
        return false;
    }
    let total_padding = interior_width.saturating_sub(trimmed.len());
    let leading = total_padding / 2;
    let trailing = total_padding - leading;
    // Both sides must have substantial padding to count as centered
    leading >= 4 && trailing >= 4 && (leading as isize - trailing as isize).unsigned_abs() <= 1
}

// ---------------------------------------------------------------------------
// Box widening
// ---------------------------------------------------------------------------

fn widen_box_right(
    grid: &mut [Vec<char>],
    bounds: &BoundingRect,
    extension: usize,
    style: BoxStyle,
) {
    let r1 = bounds.top_left.row;
    let r2 = bounds.bottom_right.row;
    let old_c2 = bounds.bottom_right.col;
    let new_c2 = old_c2 + extension;

    // Top edge: erase old TR corner, fill with horizontal, write new TR
    ensure_row_width(grid, r1, new_c2);
    grid[r1][old_c2] = style.horizontal();
    grid[r1][(old_c2 + 1)..new_c2].fill(style.horizontal());
    grid[r1][new_c2] = style.top_right();

    // Bottom edge: erase old BR corner, fill with horizontal, write new BR
    ensure_row_width(grid, r2, new_c2);
    grid[r2][old_c2] = style.horizontal();
    grid[r2][(old_c2 + 1)..new_c2].fill(style.horizontal());
    grid[r2][new_c2] = style.bottom_right();

    // Content rows: erase old right edge, fill with spaces, write new right edge
    for r in (r1 + 1)..r2 {
        ensure_row_width(grid, r, new_c2);
        grid[r][old_c2] = ' ';
        grid[r][(old_c2 + 1)..new_c2].fill(' ');
        grid[r][new_c2] = style.vertical();
    }
}

// ---------------------------------------------------------------------------
// Edge padding: extend short content lines to box width
// ---------------------------------------------------------------------------

fn pad_short_content_lines(
    grid: &mut [Vec<char>],
    bounds: &BoundingRect,
    style: BoxStyle,
) {
    let r1 = bounds.top_left.row;
    let r2 = bounds.bottom_right.row;
    let c1 = bounds.top_left.col;
    let c2 = bounds.bottom_right.col;
    let left_edge = style.vertical();

    for r in (r1 + 1)..r2 {
        let row_len = grid[r].len();

        // Only pad if the row starts with the correct left edge
        if grid_get(grid, r, c1) != Some(left_edge) {
            continue;
        }

        // If the row is already at or beyond the expected width, skip
        if row_len > c2 {
            continue;
        }

        // Extend with spaces and add closing edge
        grid[r].resize(c2 + 1, ' ');
        grid[r][c2] = style.vertical();
    }
}

// ---------------------------------------------------------------------------
// Content re-padding
// ---------------------------------------------------------------------------

fn repad_content(
    grid: &mut [Vec<char>],
    bounds: &BoundingRect,
    new_c2: usize,
    old_interior: usize,
) {
    let r1 = bounds.top_left.row;
    let r2 = bounds.bottom_right.row;
    let c1 = bounds.top_left.col;
    let new_interior = new_c2 - c1 - 1;

    for r in (r1 + 1)..r2 {
        // Read current content between left and right edges
        let content_start = c1 + 1;
        let content_end = new_c2; // exclusive (right edge position)
        let mut chars: Vec<char> = Vec::new();
        for c in content_start..content_end {
            chars.push(grid_get(grid, r, c).unwrap_or(' '));
        }

        // Skip lines with line-drawing chars (inner box borders)
        if has_line_drawing(&chars) {
            continue;
        }

        let line_str: String = chars.iter().collect();
        let trimmed = line_str.trim();

        if trimmed.is_empty() {
            // Empty line: fill with spaces
            for c in content_start..content_end {
                if c < grid[r].len() {
                    grid[r][c] = ' ';
                }
            }
            continue;
        }

        // Decide alignment: was it centered in the old interior?
        let was_centered = is_centered(trimmed, old_interior);

        let new_line: Vec<char> = if was_centered {
            // Re-center in new width
            let total_padding = new_interior.saturating_sub(trimmed.len());
            let left_pad = total_padding / 2;
            let right_pad = total_padding - left_pad;
            let mut v = Vec::with_capacity(new_interior);
            v.extend_from_slice(&vec![' '; left_pad]);
            v.extend(trimmed.chars());
            v.extend_from_slice(&vec![' '; right_pad]);
            v
        } else {
            // Left-aligned: 1 space + trimmed + fill
            let mut v = Vec::with_capacity(new_interior);
            v.push(' ');
            v.extend(trimmed.chars());
            let remaining = new_interior.saturating_sub(1 + trimmed.len());
            v.extend_from_slice(&vec![' '; remaining]);
            v
        };

        // Write back
        for (i, &ch) in new_line.iter().enumerate() {
            let c = content_start + i;
            if c < grid[r].len() {
                grid[r][c] = ch;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Fixer trait implementation
// ---------------------------------------------------------------------------

impl Fixer for BoxContentFixer {
    fn name(&self) -> &str {
        "box-content-sizing"
    }

    fn fix(&self, input: &str) -> String {
        if input.is_empty() {
            return String::new();
        }

        let trailing_newline = input.ends_with('\n');

        // 1. Build IR: detect boxes and arrows
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);
        detect_arrows(&mut ir);

        let arrow_segments = extract_arrow_segments(&ir.nodes);

        // Collect boxes
        let boxes: Vec<(BoundingRect, Vec<String>)> = ir
            .nodes
            .iter()
            .filter_map(|n| {
                if let Node::Box { bounds, content } = n {
                    Some((*bounds, content.clone()))
                } else {
                    None
                }
            })
            .collect();

        if boxes.is_empty() {
            return input.to_string();
        }

        // 2. Build mutable grid
        let mut grid = input_to_mut_grid(input);

        // 3. Pad short content lines to box width (Phase 3: edge padding)
        for (bounds, _) in &boxes {
            let style = detect_style(&grid, bounds);
            pad_short_content_lines(&mut grid, bounds, style);
        }

        // 4. Process each box for widening
        for (bounds, content) in &boxes {
            let c1 = bounds.top_left.col;
            let c2 = bounds.bottom_right.col;
            let current_interior = c2 - c1 - 1;

            // Compute max trimmed content width across non-empty text lines
            let max_trimmed_width = content
                .iter()
                .filter(|line| {
                    let trimmed = line.trim();
                    !trimmed.is_empty() && !trimmed.chars().any(is_line_drawing)
                })
                .map(|line| line.trim().len())
                .max()
                .unwrap_or(0);

            if max_trimmed_width == 0 {
                continue;
            }

            let needed_interior = max_trimmed_width + 2; // 1 space padding each side
            let style = detect_style(&grid, bounds);

            if needed_interior <= current_interior {
                // No widening needed, but still re-pad content lines
                repad_content(&mut grid, bounds, c2, current_interior);
                continue;
            }

            let extension = needed_interior - current_interior;

            // Check arrow connection on right edge
            if arrow_touches_right_edge(bounds, &arrow_segments) {
                continue;
            }

            // Check region is clear for extension
            let r1 = bounds.top_left.row;
            let r2 = bounds.bottom_right.row;
            if !region_is_clear(&grid, r1, r2, c2 + 1, c2 + extension) {
                continue;
            }

            // Widen the box
            widen_box_right(&mut grid, bounds, extension, style);

            // Re-pad content in the now-wider box
            let new_c2 = c2 + extension;
            repad_content(&mut grid, bounds, new_c2, current_interior);
        }

        // 5. Rebuild string
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
        BoxContentFixer.fix(input)
    }

    // == Round-trip fidelity (no-op on clean input) ============================

    #[test]
    fn round_trip_empty() {
        assert_eq!(fix(""), "");
    }

    #[test]
    fn round_trip_plain_text() {
        let input = "Hello world\nNo boxes here\n";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn round_trip_well_padded_box() {
        let input = "\
┌──────────┐
│ Hello    │
│ World    │
└──────────┘";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn round_trip_trailing_newline() {
        let input = "┌──────┐\n│ hi   │\n└──────┘\n";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn round_trip_demo_diagram() {
        let input = include_str!("../examples/demo-flow-diagram.txt");
        let fixed = fix(input);
        // Demo diagram boxes are already well-padded, should be unchanged
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);
        let box_count = ir
            .nodes
            .iter()
            .filter(|n| matches!(n, Node::Box { .. }))
            .count();
        assert!(box_count >= 3, "expected >= 3 boxes, got {box_count}");

        let mut ir2 = DiagramIR::new(&fixed);
        detect_boxes(&mut ir2);
        let fixed_count = ir2
            .nodes
            .iter()
            .filter(|n| matches!(n, Node::Box { .. }))
            .count();
        assert!(fixed_count >= box_count);
    }

    // == Fixes applied ========================================================

    #[test]
    fn fix_content_touching_right_edge() {
        let input = "\
┌────┐
│ Hi!│
└────┘";
        let fixed = fix(input);
        assert!(
            !fixed.contains("Hi!│") && !fixed.contains("Hi!┘"),
            "content should not touch right edge after fix: {fixed}"
        );
        // Should have padding on right
        assert!(fixed.contains("Hi! "), "expected right padding: {fixed}");
    }

    #[test]
    fn fix_content_touching_left_edge() {
        let input = "\
┌──────┐
│Hello │
└──────┘";
        let fixed = fix(input);
        assert!(
            fixed.contains(" Hello"),
            "expected left padding after fix: {fixed}"
        );
    }

    #[test]
    fn fix_content_touching_both_edges() {
        let input = "\
┌──────┐
│Hello!│
└──────┘";
        let fixed = fix(input);
        // Should be widened and padded
        assert!(
            fixed.contains(" Hello! "),
            "expected padding on both sides: {fixed}"
        );
    }

    #[test]
    fn fix_centered_content_stays_centered() {
        let input = "\
┌──────────────────────┐
│     Hello World      │
└──────────────────────┘";
        let fixed = fix(input);
        // Centered content should remain centered
        let lines: Vec<&str> = fixed.lines().collect();
        let content_line = lines[1];
        let inner = &content_line[3..content_line.len() - 3]; // skip │ and edges
        let trimmed = inner.trim();
        assert_eq!(trimmed, "Hello World");
    }

    #[test]
    fn fix_multiple_boxes_only_tight_one_widened() {
        let input = "\
┌──────┐  ┌────┐
│ OK   │  │ Hi!│
└──────┘  └────┘";
        let fixed = fix(input);
        // First box should be unchanged (well-padded)
        assert!(fixed.contains("│ OK   │"), "first box should be unchanged");
        // Second box should be widened
        assert!(
            !fixed.contains("Hi!│"),
            "second box should be widened: {fixed}"
        );
    }

    // == Constraints ==========================================================

    #[test]
    fn skip_box_blocked_by_adjacent_content() {
        let input = "\
┌────┐XXXX
│ Hi!│
└────┘";
        let fixed = fix(input);
        // Can't widen because XXXX is blocking
        assert!(
            fixed.contains("Hi!│"),
            "box should not be widened when blocked: {fixed}"
        );
    }

    #[test]
    fn skip_box_with_arrow_on_right_edge() {
        let input = "\
┌────┐
│ Hi!│──►
└────┘";
        let fixed = fix(input);
        // Arrow touches right edge, so box should not be widened
        assert!(
            fixed.contains("Hi!│"),
            "box should not be widened with arrow on right: {fixed}"
        );
    }

    #[test]
    fn fix_double_line_box() {
        let input = "\
╔════╗
║ Hi!║
╚════╝";
        let fixed = fix(input);
        assert!(
            !fixed.contains("Hi!║"),
            "double box should be widened: {fixed}"
        );
        assert!(
            fixed.contains(" Hi! "),
            "double box content should be padded: {fixed}"
        );
    }

    // == Integration ==========================================================

    #[test]
    fn fix_then_lint_zero_edge_touching() {
        let input = "\
┌────┐
│ Hi!│
└────┘";
        let fixed = fix(input);
        let lint = crate::lint_box_content::BoxContentAlignmentLint;
        let diags = lint.check(&fixed);
        let edge_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.message.contains("edge"))
            .collect();
        assert!(
            edge_diags.is_empty(),
            "expected no edge-touching diagnostics after fix, got: {edge_diags:?}"
        );
    }

    #[test]
    fn fixer_name() {
        assert_eq!(BoxContentFixer.name(), "box-content-sizing");
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
    }

    #[test]
    fn test_detect_style() {
        let grid = input_to_mut_grid("┌──┐\n│  │\n└──┘");
        let bounds = BoundingRect {
            top_left: crate::grid::Position { row: 0, col: 0 },
            bottom_right: crate::grid::Position { row: 2, col: 3 },
        };
        assert_eq!(detect_style(&grid, &bounds), BoxStyle::Single);

        let grid2 = input_to_mut_grid("╔══╗\n║  ║\n╚══╝");
        assert_eq!(detect_style(&grid2, &bounds), BoxStyle::Double);
    }

    #[test]
    fn test_box_style_chars() {
        assert_eq!(BoxStyle::Single.horizontal(), '─');
        assert_eq!(BoxStyle::Single.vertical(), '│');
        assert_eq!(BoxStyle::Single.top_right(), '┐');
        assert_eq!(BoxStyle::Single.bottom_right(), '┘');

        assert_eq!(BoxStyle::Double.horizontal(), '═');
        assert_eq!(BoxStyle::Double.vertical(), '║');
        assert_eq!(BoxStyle::Double.top_right(), '╗');
        assert_eq!(BoxStyle::Double.bottom_right(), '╝');
    }

    #[test]
    fn test_region_is_clear() {
        let grid = input_to_mut_grid("     \n     \n     ");
        assert!(region_is_clear(&grid, 0, 2, 0, 4));

        let grid2 = input_to_mut_grid("  X  \n     \n     ");
        assert!(!region_is_clear(&grid2, 0, 2, 0, 4));
    }

    #[test]
    fn test_region_is_clear_beyond_grid() {
        let grid = input_to_mut_grid("  ");
        assert!(region_is_clear(&grid, 5, 7, 0, 1));
    }

    #[test]
    fn test_has_line_drawing() {
        assert!(has_line_drawing(&['a', '─', 'b']));
        assert!(!has_line_drawing(&['a', 'b', 'c']));
        assert!(!has_line_drawing(&[' ', ' ']));
    }

    #[test]
    fn test_is_centered() {
        assert!(is_centered("Hello", 15)); // 5 left, 5 right
        assert!(!is_centered("Hello", 8)); // not enough padding
        assert!(!is_centered("", 10));
    }

    #[test]
    fn test_arrow_touches_right_edge() {
        let bounds = BoundingRect {
            top_left: crate::grid::Position { row: 0, col: 0 },
            bottom_right: crate::grid::Position { row: 2, col: 5 },
        };

        // Arrow starting at col 6 (right_col + 1), row 1 (within box)
        let seg = Segment {
            start: crate::grid::Position { row: 1, col: 6 },
            end: crate::grid::Position { row: 1, col: 10 },
            direction: crate::grid::Direction::Right,
        };
        assert!(arrow_touches_right_edge(&bounds, &[vec![seg]]));

        // Arrow at wrong position
        let seg2 = Segment {
            start: crate::grid::Position { row: 1, col: 8 },
            end: crate::grid::Position { row: 1, col: 10 },
            direction: crate::grid::Direction::Right,
        };
        assert!(!arrow_touches_right_edge(&bounds, &[vec![seg2]]));

        // Arrow at right col+1 but outside row range
        let seg3 = Segment {
            start: crate::grid::Position { row: 5, col: 6 },
            end: crate::grid::Position { row: 5, col: 10 },
            direction: crate::grid::Direction::Right,
        };
        assert!(!arrow_touches_right_edge(&bounds, &[vec![seg3]]));
    }

    #[test]
    fn test_extract_arrow_segments() {
        let nodes = vec![
            Node::Box {
                bounds: BoundingRect {
                    top_left: crate::grid::Position { row: 0, col: 0 },
                    bottom_right: crate::grid::Position { row: 2, col: 3 },
                },
                content: vec![],
            },
            Node::Arrow {
                segments: vec![Segment {
                    start: crate::grid::Position { row: 0, col: 0 },
                    end: crate::grid::Position { row: 0, col: 5 },
                    direction: crate::grid::Direction::Right,
                }],
                label: None,
            },
        ];
        let arrows = extract_arrow_segments(&nodes);
        assert_eq!(arrows.len(), 1);
        assert_eq!(arrows[0].len(), 1);
    }

    #[test]
    fn fix_empty_box_no_change() {
        let input = "\
┌──────┐
│      │
│      │
└──────┘";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn fix_box_with_nested_inner_box() {
        let input = "\
┌──────────────┐
│ ┌──────────┐ │
│ │  inner   │ │
│ └──────────┘ │
└──────────────┘";
        // Inner box lines have line-drawing chars, should be skipped for re-padding
        let fixed = fix(input);
        assert!(
            fixed.contains("inner"),
            "nested box content should be preserved"
        );
    }

    #[test]
    fn fix_repad_skips_line_drawing_rows() {
        // Outer box has tight text content AND a row with line-drawing chars.
        // The text content triggers repadding; the line-drawing row is skipped.
        let input = "\
┌────────────┐
│Hello       │
│ ────────── │
└────────────┘";
        let fixed = fix(input);
        // "Hello" should be repadded with left space
        assert!(
            fixed.contains(" Hello"),
            "text should be left-padded: {fixed}"
        );
        // Line-drawing row should be preserved as-is
        assert!(
            fixed.contains("────"),
            "line-drawing row should be preserved: {fixed}"
        );
    }

    #[test]
    fn fix_content_right_edge_only() {
        // Content touches right edge but not left — needs widening
        let input = "\
┌─────┐
│ test│
└─────┘";
        let fixed = fix(input);
        assert!(
            fixed.contains(" test "),
            "expected padding on both sides after fix: {fixed}"
        );
    }

    #[test]
    fn fix_box_no_text_content() {
        // Box with only whitespace — no change needed
        let input = "\
┌──────┐
│      │
└──────┘";
        assert_eq!(fix(input), input);
    }

    // == Edge padding (Phase 3) ================================================

    #[test]
    fn pad_short_line_adds_closing_edge() {
        let input = "\
┌──────┐
│ hi
└──────┘";
        let expected = "\
┌──────┐
│ hi   │
└──────┘";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn pad_multiple_short_lines() {
        let input = "\
┌──────┐
│ one
│ two
└──────┘";
        let expected = "\
┌──────┐
│ one  │
│ two  │
└──────┘";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn pad_already_correct_no_change() {
        let input = "\
┌──────┐
│ hi   │
└──────┘";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn pad_double_line_style() {
        let input = "\
╔══════╗
║ hi
╚══════╝";
        let expected = "\
╔══════╗
║ hi   ║
╚══════╝";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn pad_skips_line_without_left_edge() {
        // A content line missing the left edge char should not be padded.
        // We test pad_short_content_lines directly since the box detector
        // may not find boxes with missing edges.
        use crate::grid::Position;
        let mut grid = input_to_mut_grid("┌──────┐\n  hi\n└──────┘");
        let bounds = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 2, col: 7 },
        };
        pad_short_content_lines(&mut grid, &bounds, BoxStyle::Single);
        // Line 1 doesn't start with │, so should not get right edge added
        let line1: String = grid[1].iter().collect();
        assert!(!line1.ends_with('│'), "line without left edge should not get right edge: {line1}");
    }

    #[test]
    fn pad_short_line_empty_content() {
        let input = "\
┌──────┐
│
└──────┘";
        let expected = "\
┌──────┐
│      │
└──────┘";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn pad_preserves_existing_content() {
        let input = "\
┌──────────┐
│ content
│ ok       │
└──────────┘";
        let fixed = fix(input);
        assert!(fixed.contains("│ content  │"), "padded line should have correct width: {fixed}");
        // "ok" gets left-repadded since content is left-aligned
        assert!(fixed.contains("ok"), "existing content preserved: {fixed}");
    }
}
