// Box corner and edge auto-fixer.
//
// Repairs broken box-drawing structures by replacing incorrect edge and corner
// characters. Traces from orphan top-left corners (┌/╔) to locate the other
// three corners, then overwrites broken characters along the edges.

use crate::detect_boxes::detect_boxes;
use crate::grid::{is_box_corner, DiagramIR, Node, Position};
use crate::Fixer;

pub struct BoxCornerEdgeFixer;

// ---------------------------------------------------------------------------
// Edge character validation (mirrors lint_box_corners.rs private helpers)
// ---------------------------------------------------------------------------

fn is_single_top_edge(ch: char) -> bool {
    matches!(ch, '─' | '┬' | '┴' | '┼')
}

fn is_single_bottom_edge(ch: char) -> bool {
    matches!(ch, '─' | '┬' | '┴' | '┼')
}

fn is_single_left_edge(ch: char) -> bool {
    matches!(ch, '│' | '├' | '┤' | '┼')
}

fn is_single_right_edge(ch: char) -> bool {
    matches!(ch, '│' | '├' | '┤' | '┼')
}

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

// ---------------------------------------------------------------------------
// Mutable grid helpers
// ---------------------------------------------------------------------------

/// Build a mutable grid from input — NO padding (preserves ragged lines for
/// round-trip fidelity).
fn input_to_mut_grid(input: &str) -> Vec<Vec<char>> {
    input.lines().map(|l| l.chars().collect()).collect()
}

/// Rebuild a string from the mutable grid. Does NOT append a trailing newline.
fn mut_grid_to_string(grid: &[Vec<char>]) -> String {
    grid.iter()
        .map(|row| row.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Safely get a char from the mutable grid.
fn grid_get(grid: &[Vec<char>], row: usize, col: usize) -> Option<char> {
    grid.get(row).and_then(|r| r.get(col)).copied()
}

// ---------------------------------------------------------------------------
// Corner scanning helpers
// ---------------------------------------------------------------------------

/// Scan right along a row from `start_col` for any of `targets`.
fn find_any_corner_on_row(
    grid: &[Vec<char>],
    row: usize,
    start_col: usize,
    targets: &[char],
) -> Option<usize> {
    let row_data = grid.get(row)?;
    (start_col..row_data.len()).find(|&c| targets.contains(&row_data[c]))
}

/// Scan down a column from `start_row` for any of `targets`.
fn find_any_corner_on_col(
    grid: &[Vec<char>],
    col: usize,
    start_row: usize,
    targets: &[char],
) -> Option<usize> {
    (start_row..grid.len()).find(|&r| {
        grid.get(r)
            .and_then(|row| row.get(col))
            .is_some_and(|&ch| targets.contains(&ch))
    })
}

// ---------------------------------------------------------------------------
// Style helpers
// ---------------------------------------------------------------------------

fn is_single_tl(ch: char) -> bool {
    ch == '┌'
}

fn is_double_tl(ch: char) -> bool {
    ch == '╔'
}

fn expected_corner(tl: char, position: &str) -> char {
    match (tl, position) {
        ('┌', "top-right") => '┐',
        ('┌', "bottom-left") => '└',
        ('┌', "bottom-right") => '┘',
        ('╔', "top-right") => '╗',
        ('╔', "bottom-left") => '╚',
        ('╔', "bottom-right") => '╝',
        _ => '?',
    }
}

// ---------------------------------------------------------------------------
// Edge content validation
// ---------------------------------------------------------------------------

/// Returns true if `ch` is a character that could plausibly appear on a box
/// edge: a box-drawing character (U+2500..U+257F) or a space (missing edge).
fn is_fixable_edge_char(ch: char) -> bool {
    ch == ' ' || ('\u{2500}'..='\u{257F}').contains(&ch)
}

/// Any vertical edge character (single or double).
fn is_any_vertical_edge(ch: char) -> bool {
    matches!(ch, '│' | '├' | '┤' | '┼' | '║' | '╠' | '╣' | '╬')
}

/// Check that ALL four edges of the candidate box contain only fixable chars,
/// each edge has at least one box-drawing character (not all spaces), and no
/// edge position looks like a misaligned edge (space with the expected edge
/// character in the adjacent outward cell).
fn edges_are_fixable(grid: &[Vec<char>], r: usize, c: usize, r2: usize, c2: usize) -> bool {
    // Top edge
    let mut has_bd = false;
    for col in (c + 1)..c2 {
        if let Some(ch) = grid_get(grid, r, col) {
            if !is_fixable_edge_char(ch) {
                return false;
            }
            if ch != ' ' {
                has_bd = true;
            }
        }
    }
    if !has_bd {
        return false;
    }

    // Bottom edge
    has_bd = false;
    for col in (c + 1)..c2 {
        if let Some(ch) = grid_get(grid, r2, col) {
            if !is_fixable_edge_char(ch) {
                return false;
            }
            if ch != ' ' {
                has_bd = true;
            }
        }
    }
    if !has_bd {
        return false;
    }

    // Left edge
    has_bd = false;
    for row in (r + 1)..r2 {
        if let Some(ch) = grid_get(grid, row, c) {
            if !is_fixable_edge_char(ch) {
                return false;
            }
            if ch == ' ' {
                // Misalignment check: if the cell just outside has an edge char,
                // the box corners are likely off-by-one — not a broken edge.
                if c > 0 {
                    if let Some(adj) = grid_get(grid, row, c - 1) {
                        if is_any_vertical_edge(adj) {
                            return false;
                        }
                    }
                }
            } else {
                has_bd = true;
            }
        }
    }
    if !has_bd {
        return false;
    }

    // Right edge
    has_bd = false;
    for row in (r + 1)..r2 {
        if let Some(ch) = grid_get(grid, row, c2) {
            if !is_fixable_edge_char(ch) {
                return false;
            }
            if ch == ' ' {
                // Misalignment check: edge char just outside → off-by-one
                if let Some(adj) = grid_get(grid, row, c2 + 1) {
                    if is_any_vertical_edge(adj) {
                        return false;
                    }
                }
            } else {
                has_bd = true;
            }
        }
    }
    if !has_bd {
        return false;
    }

    true
}

// ---------------------------------------------------------------------------
// Core fix logic
// ---------------------------------------------------------------------------

/// Attempt to fix a single-line box starting at orphan TL corner (r, c).
/// Returns true if the box was fixed (all 4 corners located, >= 3x3).
fn fix_single_box(grid: &mut [Vec<char>], r: usize, c: usize) -> bool {
    // Find TR corner: scan right for ┐ or any corner that could be TR
    let c2 = match find_any_corner_on_row(grid, r, c + 1, &['┐', '╗']) {
        Some(col) => col,
        None => return false,
    };

    // Find BL corner: scan down for └ or any corner that could be BL
    let r2 = match find_any_corner_on_col(grid, c, r + 1, &['└', '╚']) {
        Some(row) => row,
        None => return false,
    };

    // Check minimum size 3x3
    if c2 < c + 2 || r2 < r + 2 {
        return false;
    }

    // Check BR position has a box corner (prevents false-positive matching
    // where TR and BL come from different boxes)
    match grid_get(grid, r2, c2) {
        Some(ch) if is_box_corner(ch) => {}
        _ => return false,
    }

    // Validate edges don't cross text content (prevents false-positive fixes)
    if !edges_are_fixable(&*grid, r, c, r2, c2) {
        return false;
    }

    // All 4 corners located and edges validated — apply fixes
    let tl = '┌';

    // Fix corners
    grid[r][c] = tl;
    grid[r][c2] = expected_corner(tl, "top-right");
    grid[r2][c] = expected_corner(tl, "bottom-left");
    grid[r2][c2] = expected_corner(tl, "bottom-right");

    // Fix top edge
    for cell in &mut grid[r][(c + 1)..c2] {
        if !is_single_top_edge(*cell) {
            *cell = '─';
        }
    }

    // Fix bottom edge
    for cell in &mut grid[r2][(c + 1)..c2] {
        if !is_single_bottom_edge(*cell) {
            *cell = '─';
        }
    }

    // Fix left edge
    for row in (r + 1)..r2 {
        if let Some(ch) = grid_get(grid, row, c) {
            if !is_single_left_edge(ch) {
                grid[row][c] = '│';
            }
        }
    }

    // Fix right edge
    for row in (r + 1)..r2 {
        if let Some(ch) = grid_get(grid, row, c2) {
            if !is_single_right_edge(ch) {
                grid[row][c2] = '│';
            }
        }
    }

    true
}

/// Attempt to fix a double-line box starting at orphan TL corner (r, c).
fn fix_double_box(grid: &mut [Vec<char>], r: usize, c: usize) -> bool {
    let c2 = match find_any_corner_on_row(grid, r, c + 1, &['╗', '┐']) {
        Some(col) => col,
        None => return false,
    };

    let r2 = match find_any_corner_on_col(grid, c, r + 1, &['╚', '└']) {
        Some(row) => row,
        None => return false,
    };

    if c2 < c + 2 || r2 < r + 2 {
        return false;
    }

    // Check BR position has a box corner (prevents false-positive matching)
    match grid_get(grid, r2, c2) {
        Some(ch) if is_box_corner(ch) => {}
        _ => return false,
    }

    // Validate edges don't cross text content (prevents false-positive fixes)
    if !edges_are_fixable(&*grid, r, c, r2, c2) {
        return false;
    }

    let tl = '╔';

    // Fix corners
    grid[r][c] = tl;
    grid[r][c2] = expected_corner(tl, "top-right");
    grid[r2][c] = expected_corner(tl, "bottom-left");
    grid[r2][c2] = expected_corner(tl, "bottom-right");

    // Fix top edge
    for cell in &mut grid[r][(c + 1)..c2] {
        if !is_double_top_edge(*cell) {
            *cell = '═';
        }
    }

    // Fix bottom edge
    for cell in &mut grid[r2][(c + 1)..c2] {
        if !is_double_bottom_edge(*cell) {
            *cell = '═';
        }
    }

    // Fix left edge
    for row in (r + 1)..r2 {
        if let Some(ch) = grid_get(grid, row, c) {
            if !is_double_left_edge(ch) {
                grid[row][c] = '║';
            }
        }
    }

    // Fix right edge
    for row in (r + 1)..r2 {
        if let Some(ch) = grid_get(grid, row, c2) {
            if !is_double_right_edge(ch) {
                grid[row][c2] = '║';
            }
        }
    }

    true
}

// ---------------------------------------------------------------------------
// Fixer trait implementation
// ---------------------------------------------------------------------------

impl Fixer for BoxCornerEdgeFixer {
    fn name(&self) -> &str {
        "box-corner-edge"
    }

    fn fix(&self, input: &str) -> String {
        if input.is_empty() {
            return String::new();
        }

        let trailing_newline = input.ends_with('\n');

        // 1. Build DiagramIR and detect valid boxes
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);

        // 2. Collect valid box corner positions
        let mut valid_corners: Vec<Position> = Vec::new();
        for node in &ir.nodes {
            if let Node::Box { bounds, .. } = node {
                valid_corners.push(bounds.top_left);
                valid_corners.push(Position {
                    row: bounds.top_left.row,
                    col: bounds.bottom_right.col,
                });
                valid_corners.push(Position {
                    row: bounds.bottom_right.row,
                    col: bounds.top_left.col,
                });
                valid_corners.push(bounds.bottom_right);
            }
        }

        // 3. Build mutable grid (NO padding — preserves ragged lines)
        let mut cells = input_to_mut_grid(input);

        // 4. Scan for orphan ┌/╔ and attempt to fix
        let rows = ir.grid.rows();
        let cols = ir.grid.cols();

        for r in 0..rows {
            for c in 0..cols {
                // grid.get() is always Some for in-bounds coordinates
                let ch = ir.grid.get(r, c).unwrap();

                if !is_box_corner(ch) {
                    continue;
                }

                let p = Position { row: r, col: c };
                if valid_corners.contains(&p) {
                    continue;
                }

                // Orphan corner — only fix from TL corners
                if is_single_tl(ch) {
                    fix_single_box(&mut cells, r, c);
                } else if is_double_tl(ch) {
                    fix_double_box(&mut cells, r, c);
                }
            }
        }

        // 5. Rebuild string preserving trailing newline
        let mut result = mut_grid_to_string(&cells);
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
    use crate::{Fixer, LintRule};

    fn fix(input: &str) -> String {
        BoxCornerEdgeFixer.fix(input)
    }

    // == Round-trip fidelity ==================================================

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
    fn round_trip_single_box() {
        let input = "┌──┐\n│hi│\n└──┘";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn round_trip_single_box_trailing_newline() {
        let input = "┌──┐\n│hi│\n└──┘\n";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn round_trip_double_box() {
        let input = "╔══╗\n║hi║\n╚══╝";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn round_trip_nested_boxes() {
        let input = "\
┌──────────────┐
│ ┌──────────┐ │
│ │  inner   │ │
│ └──────────┘ │
└──────────────┘";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn round_trip_adjacent_boxes() {
        let input = "\
┌───┐┌───┐
│ A ││ B │
└───┘└───┘";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn round_trip_box_with_junctions() {
        let input = "\
┌───┬───┐
│   │   │
├───┼───┤
│   │   │
└───┴───┘";
        assert_eq!(fix(input), input);
    }

    // == Single-line fix tests ================================================

    #[test]
    fn fix_broken_top_edge() {
        let input = "┌─═┐\n│  │\n└──┘";
        let expected = "┌──┐\n│  │\n└──┘";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn fix_broken_bottom_edge() {
        let input = "┌──┐\n│  │\n└─═┘";
        let expected = "┌──┐\n│  │\n└──┘";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn fix_broken_left_edge() {
        let input = "┌──┐\n═  │\n└──┘";
        let expected = "┌──┐\n│  │\n└──┘";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn fix_broken_right_edge() {
        let input = "┌──┐\n│  ═\n└──┘";
        let expected = "┌──┐\n│  │\n└──┘";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn fix_wrong_style_bottom_right_corner() {
        // ╝ is a corner char (wrong style) — fixer replaces with ┘
        let input = "┌──┐\n│  │\n└──╝";
        let expected = "┌──┐\n│  │\n└──┘";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn fix_mixed_style_tr_corner() {
        // ┌ with ╗ → replace ╗ with ┐
        let input = "┌──╗\n│  │\n└──┘";
        let expected = "┌──┐\n│  │\n└──┘";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn fix_mixed_style_bl_corner() {
        let input = "┌──┐\n│  │\n╚──┘";
        let expected = "┌──┐\n│  │\n└──┘";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn fix_mixed_style_br_corner() {
        let input = "┌──┐\n│  │\n└──╝";
        let expected = "┌──┐\n│  │\n└──┘";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn fix_multiple_broken_top_edge_chars() {
        let input = "┌════┐\n│    │\n└────┘";
        let expected = "┌────┐\n│    │\n└────┘";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn fix_space_on_edge() {
        let input = "┌─ ┐\n│  │\n└──┘";
        let expected = "┌──┐\n│  │\n└──┘";
        assert_eq!(fix(input), expected);
    }

    // == Double-line fix tests ================================================

    #[test]
    fn fix_double_broken_top_edge() {
        let input = "╔═─╗\n║  ║\n╚══╝";
        let expected = "╔══╗\n║  ║\n╚══╝";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn fix_double_broken_bottom_edge() {
        let input = "╔══╗\n║  ║\n╚═─╝";
        let expected = "╔══╗\n║  ║\n╚══╝";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn fix_double_broken_left_edge() {
        let input = "╔══╗\n─  ║\n╚══╝";
        let expected = "╔══╗\n║  ║\n╚══╝";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn fix_double_broken_right_edge() {
        let input = "╔══╗\n║  ─\n╚══╝";
        let expected = "╔══╗\n║  ║\n╚══╝";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn fix_double_wrong_corners() {
        // ╔ with single corners → replace with double
        let input = "╔══┐\n║  ║\n└══╝";
        let expected = "╔══╗\n║  ║\n╚══╝";
        assert_eq!(fix(input), expected);
    }

    // == Unfixable cases (no-op) =============================================

    #[test]
    fn unfixable_orphan_non_tl_corner() {
        let input = "  ┘  ";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn unfixable_edge_runs_off_grid() {
        let input = "┌───";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn unfixable_degenerate_box() {
        let input = "┌┐\n└┘";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn unfixable_no_bl_corner() {
        let input = "┌──┐\n│  │\n│  │";
        assert_eq!(fix(input), input);
    }

    // == Preserves junction characters ========================================

    #[test]
    fn preserves_junction_on_top_edge() {
        // ┬ is valid on top edge
        let input = "┌─┬═┐\n│ │ │\n└─┴─┘";
        let expected = "┌─┬─┐\n│ │ │\n└─┴─┘";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn preserves_junction_on_left_edge() {
        // ├ is valid on left edge
        let input = "┌──┐\n├──┤\n═  │\n└──┘";
        let expected = "┌──┐\n├──┤\n│  │\n└──┘";
        assert_eq!(fix(input), expected);
    }

    // == Integration tests ====================================================

    #[test]
    fn fix_then_lint_zero_diagnostics() {
        let input = "┌─═┐\n│  │\n└──┘";
        let fixed = fix(input);
        let diags = crate::lint_box_corners::BoxCornerEdgeLint.check(&fixed);
        assert!(
            diags.is_empty(),
            "expected no diagnostics after fix, got: {diags:?}"
        );
    }

    #[test]
    fn fix_preserves_interior_content() {
        let input = "┌────═┐\n│hello│\n│world│\n└─────┘";
        let fixed = fix(input);
        assert!(fixed.contains("hello"));
        assert!(fixed.contains("world"));
    }

    #[test]
    fn fixer_name() {
        assert_eq!(BoxCornerEdgeFixer.name(), "box-corner-edge");
    }

    // == Demo diagram test ====================================================

    #[test]
    fn demo_diagram_round_trip() {
        let input = include_str!("../examples/demo-flow-diagram.txt");
        assert_eq!(fix(input), input, "demo diagram must be unchanged by fix");
    }

    // == Nested box round-trip ================================================

    #[test]
    fn round_trip_nested_boxes_with_text() {
        let input = "\
┌────────────────────┐
│  ┌──────────────┐  │
│  │  Workflow:   │  │
│  │  1. Render   │  │
│  └──────────────┘  │
└────────────────────┘";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn round_trip_box_with_text_labels_outside() {
        let input = "\
┌──────────────────┐
│ AWS (us-east-1)  │
│ ┌────────┐       │
│ │ S3     │       │
│ │ files  │       │
│ └────────┘       │
└──────────────────┘";
        assert_eq!(fix(input), input);
    }

    // == Text on edges rejected ===============================================

    #[test]
    fn skip_fix_when_edge_crosses_text() {
        // Orphan ┌ whose top edge crosses text — should not be fixed
        let input = "┌hello─┐\n│      │\n└──────┘";
        assert_eq!(fix(input), input);
    }

    // == Helper function tests ================================================

    #[test]
    fn test_input_to_mut_grid_ragged() {
        let grid = input_to_mut_grid("ab\ncde\nf");
        assert_eq!(grid.len(), 3);
        assert_eq!(grid[0].len(), 2);
        assert_eq!(grid[1].len(), 3);
        assert_eq!(grid[2].len(), 1);
    }

    #[test]
    fn test_mut_grid_to_string() {
        let grid = vec![vec!['a', 'b'], vec!['c']];
        assert_eq!(mut_grid_to_string(&grid), "ab\nc");
    }

    #[test]
    fn test_grid_get_out_of_bounds() {
        let grid = vec![vec!['a']];
        assert_eq!(grid_get(&grid, 0, 0), Some('a'));
        assert_eq!(grid_get(&grid, 0, 1), None);
        assert_eq!(grid_get(&grid, 1, 0), None);
    }

    #[test]
    fn test_find_any_corner_on_row() {
        let grid = input_to_mut_grid("┌──┐");
        assert_eq!(find_any_corner_on_row(&grid, 0, 1, &['┐']), Some(3));
        assert_eq!(find_any_corner_on_row(&grid, 0, 1, &['╗']), None);
    }

    #[test]
    fn test_find_any_corner_on_col() {
        let grid = input_to_mut_grid("┌\n│\n└");
        assert_eq!(find_any_corner_on_col(&grid, 0, 1, &['└']), Some(2));
        assert_eq!(find_any_corner_on_col(&grid, 0, 1, &['╚']), None);
    }

    #[test]
    fn test_find_any_corner_on_row_out_of_bounds() {
        let grid = input_to_mut_grid("ab");
        assert_eq!(find_any_corner_on_row(&grid, 5, 0, &['a']), None);
    }

    #[test]
    fn test_find_any_corner_on_col_short_row() {
        // Column 2 doesn't exist in row 0 (only 2 chars wide)
        let grid = input_to_mut_grid("ab\ncde\nfg");
        assert_eq!(find_any_corner_on_col(&grid, 2, 0, &['e']), Some(1));
        assert_eq!(find_any_corner_on_col(&grid, 5, 0, &['a']), None);
    }

    #[test]
    fn test_expected_corner_helpers() {
        assert_eq!(expected_corner('┌', "top-right"), '┐');
        assert_eq!(expected_corner('┌', "bottom-left"), '└');
        assert_eq!(expected_corner('┌', "bottom-right"), '┘');
        assert_eq!(expected_corner('╔', "top-right"), '╗');
        assert_eq!(expected_corner('╔', "bottom-left"), '╚');
        assert_eq!(expected_corner('╔', "bottom-right"), '╝');
        assert_eq!(expected_corner('X', "top-right"), '?');
    }

    #[test]
    fn test_is_single_tl() {
        assert!(is_single_tl('┌'));
        assert!(!is_single_tl('╔'));
        assert!(!is_single_tl('┐'));
    }

    #[test]
    fn test_is_double_tl() {
        assert!(is_double_tl('╔'));
        assert!(!is_double_tl('┌'));
        assert!(!is_double_tl('╗'));
    }

    // == Edge validation helpers ==============================================

    #[test]
    fn test_edge_helpers_single() {
        assert!(is_single_top_edge('─'));
        assert!(is_single_top_edge('┬'));
        assert!(!is_single_top_edge('═'));

        assert!(is_single_bottom_edge('─'));
        assert!(is_single_bottom_edge('┴'));

        assert!(is_single_left_edge('│'));
        assert!(is_single_left_edge('├'));
        assert!(!is_single_left_edge('║'));

        assert!(is_single_right_edge('│'));
        assert!(is_single_right_edge('┤'));
    }

    #[test]
    fn test_edge_helpers_double() {
        assert!(is_double_top_edge('═'));
        assert!(is_double_top_edge('╦'));
        assert!(!is_double_top_edge('─'));

        assert!(is_double_bottom_edge('═'));
        assert!(is_double_bottom_edge('╩'));

        assert!(is_double_left_edge('║'));
        assert!(is_double_left_edge('╠'));
        assert!(!is_double_left_edge('│'));

        assert!(is_double_right_edge('║'));
        assert!(is_double_right_edge('╣'));
    }

    // == Fix function direct tests ============================================

    #[test]
    fn fix_single_box_returns_false_no_tr() {
        let mut grid = input_to_mut_grid("┌──\n│  \n└──");
        assert!(!fix_single_box(&mut grid, 0, 0));
    }

    #[test]
    fn fix_single_box_returns_false_no_bl() {
        let mut grid = input_to_mut_grid("┌──┐\n│  │\n│  │");
        assert!(!fix_single_box(&mut grid, 0, 0));
    }

    #[test]
    fn fix_single_box_returns_false_too_small() {
        let mut grid = input_to_mut_grid("┌┐\n└┘");
        assert!(!fix_single_box(&mut grid, 0, 0));
    }

    #[test]
    fn fix_double_box_returns_false_no_tr() {
        let mut grid = input_to_mut_grid("╔══\n║  \n╚══");
        assert!(!fix_double_box(&mut grid, 0, 0));
    }

    #[test]
    fn fix_double_box_returns_false_no_bl() {
        let mut grid = input_to_mut_grid("╔══╗\n║  ║\n║  ║");
        assert!(!fix_double_box(&mut grid, 0, 0));
    }

    #[test]
    fn fix_double_box_returns_false_too_small() {
        let mut grid = input_to_mut_grid("╔╗\n╚╝");
        assert!(!fix_double_box(&mut grid, 0, 0));
    }

    #[test]
    fn fix_single_box_br_off_grid() {
        // TR found at col 3 but row 2 is too short to have col 3
        let mut grid = input_to_mut_grid("┌──┐\n│  │\n└─");
        assert!(!fix_single_box(&mut grid, 0, 0));
    }

    #[test]
    fn fix_double_box_br_off_grid() {
        let mut grid = input_to_mut_grid("╔══╗\n║  ║\n╚═");
        assert!(!fix_double_box(&mut grid, 0, 0));
    }

    // == Edge content validation tests ========================================

    #[test]
    fn test_is_fixable_edge_char() {
        assert!(is_fixable_edge_char(' '));
        assert!(is_fixable_edge_char('─'));
        assert!(is_fixable_edge_char('│'));
        assert!(is_fixable_edge_char('═'));
        assert!(is_fixable_edge_char('║'));
        assert!(is_fixable_edge_char('┼'));
        assert!(!is_fixable_edge_char('a'));
        assert!(!is_fixable_edge_char('1'));
        assert!(!is_fixable_edge_char('-'));
        assert!(!is_fixable_edge_char('>'));
    }

    #[test]
    fn test_edges_are_fixable_clean_box() {
        let grid = input_to_mut_grid("┌──┐\n│  │\n└──┘");
        assert!(edges_are_fixable(&grid, 0, 0, 2, 3));
    }

    #[test]
    fn test_edges_are_fixable_rejects_text() {
        let grid = input_to_mut_grid("┌text┐\n│    │\n└────┘");
        assert!(!edges_are_fixable(&grid, 0, 0, 2, 5));
    }

    #[test]
    fn fix_single_box_rejects_text_on_edge() {
        let mut grid = input_to_mut_grid("┌text┐\n│    │\n└────┘");
        assert!(!fix_single_box(&mut grid, 0, 0));
    }

    #[test]
    fn fix_double_box_rejects_text_on_edge() {
        let mut grid = input_to_mut_grid("╔text╗\n║    ║\n╚════╝");
        assert!(!fix_double_box(&mut grid, 0, 0));
    }

    // == edges_are_fixable branch coverage =====================================

    #[test]
    fn edges_are_fixable_rejects_all_space_top_edge() {
        // Top edge is all spaces (no box-drawing char) → has_bd stays false
        let grid = input_to_mut_grid("┌  ┐\n│  │\n└──┘");
        assert!(!edges_are_fixable(&grid, 0, 0, 2, 3));
    }

    #[test]
    fn edges_are_fixable_rejects_text_on_bottom_edge() {
        let grid = input_to_mut_grid("┌──┐\n│  │\n└ab┘");
        assert!(!edges_are_fixable(&grid, 0, 0, 2, 3));
    }

    #[test]
    fn edges_are_fixable_rejects_all_space_bottom_edge() {
        let grid = input_to_mut_grid("┌──┐\n│  │\n└  ┘");
        assert!(!edges_are_fixable(&grid, 0, 0, 2, 3));
    }

    #[test]
    fn edges_are_fixable_rejects_text_on_left_edge() {
        let grid = input_to_mut_grid("┌──┐\na  │\n└──┘");
        assert!(!edges_are_fixable(&grid, 0, 0, 2, 3));
    }

    #[test]
    fn edges_are_fixable_rejects_left_edge_misalignment() {
        // Space on left edge at col 1 with │ at col 0 → misalignment detected
        let grid = input_to_mut_grid("│┌──┐\n││  │\n│   │\n│└──┘");
        assert!(!edges_are_fixable(&grid, 0, 1, 3, 4));
    }

    #[test]
    fn edges_are_fixable_rejects_all_space_right_edge() {
        // Right edge column has only a space (row too short) → has_bd stays false
        let grid = input_to_mut_grid("┌──┐\n│   \n└──┘");
        assert!(!edges_are_fixable(&grid, 0, 0, 2, 3));
    }

    #[test]
    fn edges_are_fixable_rejects_right_edge_misalignment() {
        // Right edge has space but adjacent col has │ → off-by-one misalignment
        let grid = input_to_mut_grid("┌──┐─\n│  │─\n│   │\n└──┘─");
        // Box corners at (0,0)→(3,3). Row 2 has space at col 3, │ at col 4.
        assert!(!edges_are_fixable(&grid, 0, 0, 3, 3));
    }
}
