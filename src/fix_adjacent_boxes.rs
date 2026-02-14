// Adjacent box horizontal alignment auto-fixer.
//
// Extends shorter boxes to align with their horizontally adjacent neighbors.
// Misaligned tops are fixed by extending upward; misaligned bottoms by extending
// downward. Never shrinks or moves boxes — only grows them.

use crate::detect_boxes::detect_boxes;
use crate::grid::{BoundingRect, DiagramIR, Node};
use crate::Fixer;

pub struct AdjacentBoxAlignmentFixer;

const MAX_GAP: usize = 10;
const MAX_SHIFT: usize = 3;

// ---------------------------------------------------------------------------
// Adjacency helpers (duplicated from lint_adjacent_boxes — they're private)
// ---------------------------------------------------------------------------

fn vertical_overlap(a: &BoundingRect, b: &BoundingRect) -> bool {
    a.top_left.row <= b.bottom_right.row && b.top_left.row <= a.bottom_right.row
}

fn no_horizontal_overlap(a: &BoundingRect, b: &BoundingRect) -> bool {
    a.bottom_right.col < b.top_left.col || b.bottom_right.col < a.top_left.col
}

fn horizontal_gap(a: &BoundingRect, b: &BoundingRect) -> usize {
    if a.bottom_right.col < b.top_left.col {
        b.top_left.col - a.bottom_right.col
    } else {
        a.top_left.col - b.bottom_right.col
    }
}

fn has_intervening_box(a: &BoundingRect, b: &BoundingRect, boxes: &[BoundingRect]) -> bool {
    let (left, right) = if a.bottom_right.col < b.top_left.col {
        (a, b)
    } else {
        (b, a)
    };

    let gap_left = left.bottom_right.col;
    let gap_right = right.top_left.col;

    boxes.iter().any(|c| {
        c.top_left.col > gap_left
            && c.bottom_right.col < gap_right
            && (vertical_overlap(c, a) || vertical_overlap(c, b))
    })
}

// ---------------------------------------------------------------------------
// Mutable grid helpers (duplicated from fix_box_corners — they're private)
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

// ---------------------------------------------------------------------------
// Box extraction helper
// ---------------------------------------------------------------------------

fn extract_box_bounds(nodes: &[Node]) -> Vec<BoundingRect> {
    let mut result = Vec::new();
    for n in nodes {
        if let Node::Box { bounds, .. } = n {
            result.push(*bounds);
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Box style detection
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

    fn top_left(self) -> char {
        match self {
            BoxStyle::Single => '┌',
            BoxStyle::Double => '╔',
        }
    }

    fn top_right(self) -> char {
        match self {
            BoxStyle::Single => '┐',
            BoxStyle::Double => '╗',
        }
    }

    fn bottom_left(self) -> char {
        match self {
            BoxStyle::Single => '└',
            BoxStyle::Double => '╚',
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
// Union-find for grouping adjacent boxes
// ---------------------------------------------------------------------------

fn find(parent: &mut [usize], i: usize) -> usize {
    let mut root = i;
    while parent[root] != root {
        root = parent[root];
    }
    // Path compression
    let mut cur = i;
    while parent[cur] != root {
        let next = parent[cur];
        parent[cur] = root;
        cur = next;
    }
    root
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let ra = find(parent, a);
    let rb = find(parent, b);
    if ra != rb {
        parent[rb] = ra;
    }
}

// ---------------------------------------------------------------------------
// Feasibility checks
// ---------------------------------------------------------------------------

/// Check that the rectangular region contains only spaces in the mutable grid.
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
// Grid mutation: extend a box
// ---------------------------------------------------------------------------

fn ensure_grid_rows(grid: &mut Vec<Vec<char>>, needed_rows: usize, min_cols: usize) {
    while grid.len() <= needed_rows {
        grid.push(vec![' '; min_cols]);
    }
}

fn ensure_row_width(grid: &mut [Vec<char>], row: usize, needed_col: usize) {
    if row < grid.len() && grid[row].len() <= needed_col {
        grid[row].resize(needed_col + 1, ' ');
    }
}

fn extend_box_top(grid: &mut [Vec<char>], bounds: &BoundingRect, new_top: usize, style: BoxStyle) {
    let old_top = bounds.top_left.row;
    let left = bounds.top_left.col;
    let right = bounds.bottom_right.col;

    // Old top edge becomes interior: corners → verticals, edge → space
    ensure_row_width(grid, old_top, right);
    grid[old_top][left] = style.vertical();
    grid[old_top][right] = style.vertical();
    for c in (left + 1)..right {
        if c < grid[old_top].len() {
            grid[old_top][c] = ' ';
        }
    }

    // Intermediate rows
    for r in (new_top + 1)..old_top {
        ensure_row_width(grid, r, right);
        grid[r][left] = style.vertical();
        grid[r][right] = style.vertical();
    }

    // New top edge
    ensure_row_width(grid, new_top, right);
    grid[new_top][left] = style.top_left();
    grid[new_top][right] = style.top_right();
    for c in (left + 1)..right {
        if c < grid[new_top].len() {
            grid[new_top][c] = style.horizontal();
        }
    }
}

fn extend_box_bottom(
    grid: &mut Vec<Vec<char>>,
    bounds: &BoundingRect,
    new_bottom: usize,
    style: BoxStyle,
) {
    let old_bottom = bounds.bottom_right.row;
    let left = bounds.top_left.col;
    let right = bounds.bottom_right.col;

    // Old bottom edge becomes interior: corners → verticals, edge → space
    ensure_row_width(grid, old_bottom, right);
    grid[old_bottom][left] = style.vertical();
    grid[old_bottom][right] = style.vertical();
    for c in (left + 1)..right {
        if c < grid[old_bottom].len() {
            grid[old_bottom][c] = ' ';
        }
    }

    // Ensure grid has enough rows
    ensure_grid_rows(grid, new_bottom, right + 1);

    // Intermediate rows
    for r in (old_bottom + 1)..new_bottom {
        ensure_row_width(grid, r, right);
        grid[r][left] = style.vertical();
        grid[r][right] = style.vertical();
    }

    // New bottom edge
    ensure_row_width(grid, new_bottom, right);
    grid[new_bottom][left] = style.bottom_left();
    grid[new_bottom][right] = style.bottom_right();
    for c in (left + 1)..right {
        if c < grid[new_bottom].len() {
            grid[new_bottom][c] = style.horizontal();
        }
    }
}

// ---------------------------------------------------------------------------
// Fixer trait implementation
// ---------------------------------------------------------------------------

impl Fixer for AdjacentBoxAlignmentFixer {
    fn name(&self) -> &str {
        "adjacent-box-alignment"
    }

    fn fix(&self, input: &str) -> String {
        if input.is_empty() {
            return String::new();
        }

        let trailing_newline = input.ends_with('\n');

        // 1. Detect boxes
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);

        let mut boxes: Vec<BoundingRect> = extract_box_bounds(&ir.nodes);

        if boxes.len() < 2 {
            return input.to_string();
        }

        boxes.sort_by_key(|b| (b.top_left.col, b.top_left.row));

        // 2. Find adjacent pairs and build union-find groups
        let n = boxes.len();
        let mut parent: Vec<usize> = (0..n).collect();

        for i in 0..n {
            for j in (i + 1)..n {
                let a = &boxes[i];
                let b = &boxes[j];

                if !vertical_overlap(a, b) {
                    continue;
                }
                if !no_horizontal_overlap(a, b) {
                    continue;
                }
                if horizontal_gap(a, b) >= MAX_GAP {
                    continue;
                }
                if has_intervening_box(a, b, &boxes) {
                    continue;
                }

                // Adjacent pair — merge groups
                union(&mut parent, i, j);
            }
        }

        // 3. Collect groups
        let mut groups: std::collections::HashMap<usize, Vec<usize>> =
            std::collections::HashMap::new();
        for i in 0..n {
            let root = find(&mut parent, i);
            groups.entry(root).or_default().push(i);
        }

        // 4. Build mutable grid
        let mut grid = input_to_mut_grid(input);

        // Detect styles before mutating
        let styles: Vec<BoxStyle> = boxes.iter().map(|b| detect_style(&grid, b)).collect();

        // 5. For each group, compute target bounds and apply extensions
        for members in groups.values() {
            if members.len() < 2 {
                continue;
            }

            let target_top = members
                .iter()
                .map(|&i| boxes[i].top_left.row)
                .min()
                .unwrap();
            let target_bottom = members
                .iter()
                .map(|&i| boxes[i].bottom_right.row)
                .max()
                .unwrap();

            // Check each box in the group
            let mut extensions: Vec<(usize, Option<usize>, Option<usize>)> = Vec::new();

            for &idx in members {
                let b = &boxes[idx];
                let need_top = if b.top_left.row > target_top {
                    Some(target_top)
                } else {
                    None
                };
                let need_bottom = if b.bottom_right.row < target_bottom {
                    Some(target_bottom)
                } else {
                    None
                };

                if need_top.is_none() && need_bottom.is_none() {
                    continue;
                }

                // Check shift limits
                let top_shift = need_top.map(|t| b.top_left.row - t).unwrap_or(0);
                let bottom_shift = need_bottom.map(|bot| bot - b.bottom_right.row).unwrap_or(0);

                if top_shift > MAX_SHIFT || bottom_shift > MAX_SHIFT {
                    continue;
                }

                // Check feasibility: expansion region must be all spaces
                let left = b.top_left.col;
                let right = b.bottom_right.col;

                if let Some(new_top) = need_top {
                    if !region_is_clear(
                        &grid,
                        new_top,
                        b.top_left.row.saturating_sub(1),
                        left,
                        right,
                    ) {
                        continue;
                    }
                }

                if let Some(new_bottom) = need_bottom {
                    // For bottom extension, rows beyond grid are fine (will be appended)
                    let check_end = new_bottom.min(grid.len().saturating_sub(1));
                    if b.bottom_right.row < check_end
                        && !region_is_clear(&grid, b.bottom_right.row + 1, check_end, left, right)
                    {
                        continue;
                    }
                }

                extensions.push((idx, need_top, need_bottom));
            }

            // Apply extensions
            for (idx, need_top, need_bottom) in &extensions {
                let style = styles[*idx];

                if let Some(new_top) = need_top {
                    extend_box_top(&mut grid, &boxes[*idx], *new_top, style);
                }
                if let Some(new_bottom) = need_bottom {
                    extend_box_bottom(&mut grid, &boxes[*idx], *new_bottom, style);
                }
            }
        }

        // 6. Rebuild string
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
    use crate::grid::Position;
    use crate::{Fixer, LintRule};

    fn fix(input: &str) -> String {
        AdjacentBoxAlignmentFixer.fix(input)
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
    fn round_trip_single_box() {
        let input = "┌──┐\n│hi│\n└──┘";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn round_trip_already_aligned() {
        let input = "\
┌───┐ ┌───┐
│ A │ │ B │
└───┘ └───┘";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn round_trip_already_aligned_touching() {
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

    #[test]
    fn round_trip_trailing_newline() {
        let input = "┌──┐\n│hi│\n└──┘\n";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn fix_with_trailing_newline() {
        let input = "┌───┐\n│ A │ ┌───┐\n│   │ │ B │\n└───┘ └───┘\n";
        let expected = "┌───┐ ┌───┐\n│ A │ │   │\n│   │ │ B │\n└───┘ └───┘\n";
        assert_eq!(fix(input), expected);
    }

    // == Fixes misalignment ===================================================

    #[test]
    fn fix_top_misalignment() {
        let input = "\
┌───┐
│ A │ ┌───┐
│   │ │ B │
└───┘ └───┘";
        // B is extended upward; content stays at original row
        let expected = "\
┌───┐ ┌───┐
│ A │ │   │
│   │ │ B │
└───┘ └───┘";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn fix_bottom_misalignment() {
        let input = "\
┌───┐ ┌───┐
│ A │ │ B │
│   │ └───┘
└───┘";
        let expected = "\
┌───┐ ┌───┐
│ A │ │ B │
│   │ │   │
└───┘ └───┘";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn fix_both_top_and_bottom() {
        let input = "\
┌───┐
│ A │ ┌───┐
│   │ │ B │
│   │ └───┘
└───┘";
        // B extended up and down; content stays at original row
        let expected = "\
┌───┐ ┌───┐
│ A │ │   │
│   │ │ B │
│   │ │   │
└───┘ └───┘";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn fix_three_boxes_middle_shorter() {
        let input = "\
┌──┐ ┌──┐ ┌──┐
│A │ │B │ │C │
│  │ └──┘ │  │
└──┘      └──┘";
        let expected = "\
┌──┐ ┌──┐ ┌──┐
│A │ │B │ │C │
│  │ │  │ │  │
└──┘ └──┘ └──┘";
        assert_eq!(fix(input), expected);
    }

    // == Constraint enforcement ================================================

    #[test]
    fn skip_shift_too_large() {
        // Box B is 4 rows below A's top — exceeds MAX_SHIFT of 3
        let input = "\
┌───┐
│ A │
│   │
│   │
│   │ ┌───┐
│   │ │ B │
└───┘ └───┘";
        // Should not be fixed (shift would be 4)
        assert_eq!(fix(input), input);
    }

    #[test]
    fn skip_content_in_expansion_area() {
        // There's text above box B where it would need to extend
        let input = "\
┌───┐ text
│ A │ ┌───┐
│   │ │ B │
└───┘ └───┘";
        // Can't extend B upward because "text" is in the way
        assert_eq!(fix(input), input);
    }

    #[test]
    fn extend_grid_downward() {
        // Box B is shorter and needs to extend past the current grid
        let input = "\
┌──┐ ┌──┐
│A │ │B │
│  │ └──┘
└──┘";
        let expected = "\
┌──┐ ┌──┐
│A │ │B │
│  │ │  │
└──┘ └──┘";
        assert_eq!(fix(input), expected);
    }

    // == Integration tests ====================================================

    #[test]
    fn fix_then_lint_zero_diagnostics() {
        let input = "\
┌───┐
│ A │ ┌───┐
│   │ │ B │
└───┘ └───┘";
        let fixed = fix(input);
        let diags = crate::lint_adjacent_boxes::AdjacentBoxAlignmentLint.check(&fixed);
        assert!(
            diags.is_empty(),
            "expected no diagnostics after fix, got: {diags:?}"
        );
    }

    #[test]
    fn fix_preserves_interior_content() {
        let input = "\
┌──────┐ ┌──────┐
│hello │ │ test │
│world │ └──────┘
└──────┘";
        let fixed = fix(input);
        assert!(fixed.contains("hello"));
        assert!(fixed.contains("world"));
        assert!(fixed.contains("test"));
    }

    #[test]
    fn fixer_name() {
        assert_eq!(AdjacentBoxAlignmentFixer.name(), "adjacent-box-alignment");
    }

    // == Double-line box tests ================================================

    #[test]
    fn fix_double_boxes_top_misaligned() {
        let input = "\
╔═══╗
║ A ║ ╔═══╗
║   ║ ║ B ║
╚═══╝ ╚═══╝";
        // B extended upward; content stays at original row
        let expected = "\
╔═══╗ ╔═══╗
║ A ║ ║   ║
║   ║ ║ B ║
╚═══╝ ╚═══╝";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn fix_double_boxes_bottom_misaligned() {
        let input = "\
╔═══╗ ╔═══╗
║ A ║ ║ B ║
║   ║ ╚═══╝
╚═══╝";
        let expected = "\
╔═══╗ ╔═══╗
║ A ║ ║ B ║
║   ║ ║   ║
╚═══╝ ╚═══╝";
        assert_eq!(fix(input), expected);
    }

    // == Demo diagram test ====================================================

    #[test]
    fn demo_diagram_round_trip() {
        let input = include_str!("../examples/demo-flow-diagram.txt");
        let fixed = fix(input);
        // Should not panic; valid boxes preserved
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);
        let valid_count = ir
            .nodes
            .iter()
            .filter(|n| matches!(n, Node::Box { .. }))
            .count();
        assert!(
            valid_count >= 3,
            "expected >= 3 valid boxes, got {valid_count}"
        );

        let mut ir2 = DiagramIR::new(&fixed);
        detect_boxes(&mut ir2);
        let fixed_count = ir2
            .nodes
            .iter()
            .filter(|n| matches!(n, Node::Box { .. }))
            .count();
        assert!(fixed_count >= valid_count);
    }

    // == Helper function tests ================================================

    #[test]
    fn test_vertical_overlap() {
        let a = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 3, col: 5 },
        };
        let b = BoundingRect {
            top_left: Position { row: 2, col: 10 },
            bottom_right: Position { row: 5, col: 15 },
        };
        assert!(vertical_overlap(&a, &b));

        let c = BoundingRect {
            top_left: Position { row: 5, col: 10 },
            bottom_right: Position { row: 8, col: 15 },
        };
        assert!(!vertical_overlap(&a, &c));
    }

    #[test]
    fn test_no_horizontal_overlap() {
        let a = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 3, col: 5 },
        };
        let b = BoundingRect {
            top_left: Position { row: 0, col: 7 },
            bottom_right: Position { row: 3, col: 12 },
        };
        assert!(no_horizontal_overlap(&a, &b));
        assert!(no_horizontal_overlap(&b, &a));

        let c = BoundingRect {
            top_left: Position { row: 0, col: 3 },
            bottom_right: Position { row: 3, col: 8 },
        };
        assert!(!no_horizontal_overlap(&a, &c));
    }

    #[test]
    fn test_horizontal_gap() {
        let a = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 3, col: 5 },
        };
        let b = BoundingRect {
            top_left: Position { row: 0, col: 8 },
            bottom_right: Position { row: 3, col: 12 },
        };
        assert_eq!(horizontal_gap(&a, &b), 3);
        assert_eq!(horizontal_gap(&b, &a), 3);
    }

    #[test]
    fn test_has_intervening_box() {
        let a = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 3, col: 3 },
        };
        let between = BoundingRect {
            top_left: Position { row: 0, col: 5 },
            bottom_right: Position { row: 3, col: 6 },
        };
        let c = BoundingRect {
            top_left: Position { row: 0, col: 9 },
            bottom_right: Position { row: 3, col: 12 },
        };
        assert!(has_intervening_box(&a, &c, &[a, between, c]));
        assert!(!has_intervening_box(&a, &between, &[a, between, c]));
    }

    #[test]
    fn test_has_intervening_box_reversed() {
        let a = BoundingRect {
            top_left: Position { row: 0, col: 9 },
            bottom_right: Position { row: 3, col: 12 },
        };
        let b = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 3, col: 3 },
        };
        let between = BoundingRect {
            top_left: Position { row: 0, col: 5 },
            bottom_right: Position { row: 3, col: 6 },
        };
        assert!(has_intervening_box(&a, &b, &[a, between, b]));
    }

    #[test]
    fn test_detect_style() {
        let grid = input_to_mut_grid("┌──┐\n│  │\n└──┘");
        let bounds = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 2, col: 3 },
        };
        assert_eq!(detect_style(&grid, &bounds), BoxStyle::Single);

        let grid2 = input_to_mut_grid("╔══╗\n║  ║\n╚══╝");
        assert_eq!(detect_style(&grid2, &bounds), BoxStyle::Double);
    }

    #[test]
    fn test_box_style_chars() {
        assert_eq!(BoxStyle::Single.horizontal(), '─');
        assert_eq!(BoxStyle::Single.vertical(), '│');
        assert_eq!(BoxStyle::Single.top_left(), '┌');
        assert_eq!(BoxStyle::Single.top_right(), '┐');
        assert_eq!(BoxStyle::Single.bottom_left(), '└');
        assert_eq!(BoxStyle::Single.bottom_right(), '┘');

        assert_eq!(BoxStyle::Double.horizontal(), '═');
        assert_eq!(BoxStyle::Double.vertical(), '║');
        assert_eq!(BoxStyle::Double.top_left(), '╔');
        assert_eq!(BoxStyle::Double.top_right(), '╗');
        assert_eq!(BoxStyle::Double.bottom_left(), '╚');
        assert_eq!(BoxStyle::Double.bottom_right(), '╝');
    }

    #[test]
    fn test_union_find() {
        let mut parent: Vec<usize> = (0..5).collect();
        union(&mut parent, 0, 1);
        union(&mut parent, 2, 3);
        union(&mut parent, 1, 3);
        assert_eq!(find(&mut parent, 0), find(&mut parent, 3));
        assert_ne!(find(&mut parent, 0), find(&mut parent, 4));
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
        // Rows beyond grid are treated as clear (None → ok)
        assert!(region_is_clear(&grid, 5, 7, 0, 1));
    }

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
    fn test_ensure_grid_rows() {
        let mut grid: Vec<Vec<char>> = vec![vec!['a', 'b']];
        ensure_grid_rows(&mut grid, 3, 2);
        assert_eq!(grid.len(), 4);
        assert_eq!(grid[3], vec![' ', ' ']);
    }

    #[test]
    fn test_ensure_row_width() {
        let mut grid = vec![vec!['a', 'b']];
        ensure_row_width(&mut grid, 0, 4);
        assert_eq!(grid[0].len(), 5);
        assert_eq!(grid[0][4], ' ');
    }

    #[test]
    fn single_box_no_change() {
        let input = "┌──┐\n│  │\n└──┘";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn skip_content_blocking_bottom_extension() {
        // Content below box B blocks downward extension
        let input = "\
┌───┐ ┌───┐
│ A │ │ B │
│   │ └───┘
└───┘ XXXX";
        // B can't extend down because XXXX is in the way
        assert_eq!(fix(input), input);
    }

    #[test]
    fn fix_top_by_two_rows() {
        // Box B starts 2 rows below A — exercises intermediate rows in extend_box_top
        let input = "\
┌───┐
│ A │
│   │ ┌───┐
│   │ │ B │
└───┘ └───┘";
        let expected = "\
┌───┐ ┌───┐
│ A │ │   │
│   │ │   │
│   │ │ B │
└───┘ └───┘";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn fix_bottom_by_two_rows() {
        // Box B is 2 rows shorter at the bottom — exercises intermediate rows in extend_box_bottom
        let input = "\
┌───┐ ┌───┐
│ A │ │ B │
│   │ └───┘
│   │
└───┘";
        let expected = "\
┌───┐ ┌───┐
│ A │ │ B │
│   │ │   │
│   │ │   │
└───┘ └───┘";
        assert_eq!(fix(input), expected);
    }

    #[test]
    fn input_with_arrows_and_boxes() {
        // Exercises the filter_map `_ => None` branch for Arrow nodes
        let input = "\
┌──┐
│  │──►┌──┐
│  │   │  │
└──┘   └──┘";
        // The arrow is between the boxes; boxes have misaligned tops
        let _result = fix(input);
        // Just ensure no panic; arrows produce Node::Arrow filtered out by filter_map
    }

    #[test]
    fn nested_boxes_no_overlap_fix() {
        // Nested boxes overlap horizontally — exercises the no_horizontal_overlap continue
        let input = "\
┌──────────────┐
│ ┌──────────┐ │
│ │  inner   │ │
│ └──────────┘ │
└──────────────┘";
        assert_eq!(fix(input), input);
    }

    #[test]
    fn fix_then_lint_both_misaligned() {
        let input = "\
┌───┐
│ A │ ┌───┐
│   │ │ B │
│   │ └───┘
└───┘";
        let fixed = fix(input);
        let diags = crate::lint_adjacent_boxes::AdjacentBoxAlignmentLint.check(&fixed);
        assert!(
            diags.is_empty(),
            "expected no diagnostics after fix, got: {diags:?}"
        );
    }
}
