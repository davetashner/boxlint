// Adjacent box horizontal alignment lint rule.
//
// Detects when horizontally adjacent boxes have mismatched top or bottom
// rows — a common visual alignment issue in box-drawing diagrams.

use crate::detect_arrows::detect_arrows;
use crate::detect_boxes::detect_boxes;
use crate::grid::{BoundingRect, DiagramIR, Node};
use crate::{Diagnostic, Level, LintRule};

pub struct AdjacentBoxAlignmentLint;

const RULE: &str = "adjacent-box-alignment";
const MAX_GAP: usize = 10;

fn diag(line: usize, col: usize, message: String) -> Diagnostic {
    Diagnostic {
        file: String::new(),
        line,
        col,
        level: Level::Warning,
        message,
        rule: RULE.to_string(),
    }
}

/// Check if two boxes' vertical (row) extents overlap.
fn vertical_overlap(a: &BoundingRect, b: &BoundingRect) -> bool {
    a.top_left.row <= b.bottom_right.row && b.top_left.row <= a.bottom_right.row
}

/// Check that boxes don't horizontally overlap (they're side-by-side).
fn no_horizontal_overlap(a: &BoundingRect, b: &BoundingRect) -> bool {
    a.bottom_right.col < b.top_left.col || b.bottom_right.col < a.top_left.col
}

/// Horizontal gap between two non-overlapping boxes.
fn horizontal_gap(a: &BoundingRect, b: &BoundingRect) -> usize {
    if a.bottom_right.col < b.top_left.col {
        b.top_left.col - a.bottom_right.col
    } else {
        a.top_left.col - b.bottom_right.col
    }
}

/// Check if any box from `boxes` sits between a and b horizontally.
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

impl LintRule for AdjacentBoxAlignmentLint {
    fn name(&self) -> &str {
        RULE
    }

    fn check(&self, input: &str) -> Vec<Diagnostic> {
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);
        detect_arrows(&mut ir);

        let mut boxes: Vec<BoundingRect> = ir
            .nodes
            .iter()
            .filter_map(|n| match n {
                Node::Box { bounds, .. } => Some(*bounds),
                _ => None,
            })
            .collect();

        boxes.sort_by_key(|b| (b.top_left.col, b.top_left.row));

        let mut diagnostics = Vec::new();

        for i in 0..boxes.len() {
            for j in (i + 1)..boxes.len() {
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

                if a.top_left.row != b.top_left.row {
                    let diff = a.top_left.row.abs_diff(b.top_left.row);
                    diagnostics.push(diag(
                        a.top_left.row.min(b.top_left.row) + 1,
                        a.top_left.col.min(b.top_left.col) + 1,
                        format!(
                            "adjacent boxes misaligned: top rows differ by {} \
                             (box at {}:{} has top at row {}, box at {}:{} has top at row {})",
                            diff,
                            a.top_left.row + 1,
                            a.top_left.col + 1,
                            a.top_left.row + 1,
                            b.top_left.row + 1,
                            b.top_left.col + 1,
                            b.top_left.row + 1,
                        ),
                    ));
                }

                if a.bottom_right.row != b.bottom_right.row {
                    let diff = a.bottom_right.row.abs_diff(b.bottom_right.row);
                    diagnostics.push(diag(
                        a.bottom_right.row.min(b.bottom_right.row) + 1,
                        a.top_left.col.min(b.top_left.col) + 1,
                        format!(
                            "adjacent boxes misaligned: bottom rows differ by {} \
                             (box at {}:{} has bottom at row {}, box at {}:{} has bottom at row {})",
                            diff,
                            a.top_left.row + 1,
                            a.top_left.col + 1,
                            a.bottom_right.row + 1,
                            b.top_left.row + 1,
                            b.top_left.col + 1,
                            b.bottom_right.row + 1,
                        ),
                    ));
                }
            }
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lint(input: &str) -> Vec<Diagnostic> {
        AdjacentBoxAlignmentLint.check(input)
    }

    // -- No false positives --

    #[test]
    fn empty_input() {
        assert!(lint("").is_empty());
    }

    #[test]
    fn plain_text() {
        assert!(lint("Hello world\nNo boxes here\n").is_empty());
    }

    #[test]
    fn single_box() {
        assert!(lint("┌──┐\n│  │\n└──┘").is_empty());
    }

    #[test]
    fn well_aligned_adjacent() {
        let input = "\
┌───┐ ┌───┐
│ A │ │ B │
└───┘ └───┘";
        assert!(lint(input).is_empty());
    }

    #[test]
    fn well_aligned_adjacent_touching() {
        let input = "\
┌───┐┌───┐
│ A ││ B │
└───┘└───┘";
        assert!(lint(input).is_empty());
    }

    #[test]
    fn boxes_far_apart_not_adjacent() {
        let input = "\
┌──┐                    ┌──┐
│A │                    │B │
│  │                    └──┘
└──┘";
        // Gap > 10 columns, so not considered adjacent
        assert!(lint(input).is_empty());
    }

    #[test]
    fn nested_boxes_not_flagged() {
        let input = "\
┌──────────────┐
│ ┌──────────┐ │
│ │  inner   │ │
│ └──────────┘ │
└──────────────┘";
        assert!(lint(input).is_empty());
    }

    #[test]
    fn vertically_stacked_not_adjacent() {
        let input = "\
┌───┐
│ A │
└───┘
┌───┐
│ B │
└───┘";
        // No vertical overlap
        assert!(lint(input).is_empty());
    }

    // -- Detects misalignment --

    #[test]
    fn top_rows_differ() {
        let input = "\
┌───┐
│ A │ ┌───┐
│   │ │ B │
└───┘ └───┘";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("top rows differ by 1"));
        assert_eq!(diags[0].level, Level::Warning);
    }

    #[test]
    fn bottom_rows_differ() {
        let input = "\
┌───┐ ┌───┐
│ A │ │ B │
│   │ └───┘
└───┘";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("bottom rows differ by 1"));
    }

    #[test]
    fn both_top_and_bottom_misaligned() {
        let input = "\
┌───┐
│ A │ ┌───┐
│   │ │ B │
│   │ └───┘
└───┘";
        let diags = lint(input);
        assert_eq!(diags.len(), 2);
        assert!(diags.iter().any(|d| d.message.contains("top rows differ")));
        assert!(diags
            .iter()
            .any(|d| d.message.contains("bottom rows differ")));
    }

    #[test]
    fn three_boxes_middle_misaligned() {
        let input = "\
┌──┐ ┌──┐ ┌──┐
│A │ │B │ │C │
│  │ └──┘ │  │
└──┘      └──┘";
        let diags = lint(input);
        // A-B: bottoms differ, B-C: bottoms differ
        // A-C: B intervenes, so not flagged
        assert_eq!(diags.len(), 2);
        assert!(diags
            .iter()
            .all(|d| d.message.contains("bottom rows differ")));
    }

    // -- Edge cases --

    #[test]
    fn larger_top_difference() {
        let input = "\
┌───┐
│ A │
│   │ ┌───┐
│   │ │ B │
└───┘ └───┘";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("top rows differ by 2"));
    }

    #[test]
    fn rule_name() {
        assert_eq!(AdjacentBoxAlignmentLint.name(), "adjacent-box-alignment");
    }

    #[test]
    fn diag_helper() {
        let d = diag(1, 1, "test".to_string());
        assert_eq!(d.rule, "adjacent-box-alignment");
        assert_eq!(d.level, Level::Warning);
        assert_eq!(d.file, "");
    }

    #[test]
    fn vertical_overlap_check() {
        use crate::grid::Position;
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
    fn no_horizontal_overlap_check() {
        use crate::grid::Position;
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
    fn horizontal_gap_check() {
        use crate::grid::Position;
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
    fn intervening_box_blocks_adjacency() {
        use crate::grid::Position;
        let a = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 3, col: 3 },
        };
        let b = BoundingRect {
            top_left: Position { row: 0, col: 5 },
            bottom_right: Position { row: 3, col: 6 },
        };
        let c = BoundingRect {
            top_left: Position { row: 0, col: 9 },
            bottom_right: Position { row: 3, col: 12 },
        };
        // b is between a and c
        assert!(has_intervening_box(&a, &c, &[a, b, c]));
        // nothing between a and b
        assert!(!has_intervening_box(&a, &b, &[a, b, c]));
    }

    #[test]
    fn intervening_box_reversed_order() {
        use crate::grid::Position;
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
        // a is right of b, so the else branch fires
        assert!(has_intervening_box(&a, &b, &[a, between, b]));
    }

    #[test]
    fn intervening_box_wrong_rows_no_block() {
        use crate::grid::Position;
        let a = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 3, col: 3 },
        };
        let c = BoundingRect {
            top_left: Position { row: 0, col: 9 },
            bottom_right: Position { row: 3, col: 12 },
        };
        // "between" box is in a completely different row range
        let between = BoundingRect {
            top_left: Position { row: 10, col: 5 },
            bottom_right: Position { row: 13, col: 6 },
        };
        assert!(!has_intervening_box(&a, &c, &[a, between, c]));
    }

    #[test]
    fn demo_diagram_no_crash() {
        let input = include_str!("../examples/demo-flow-diagram.txt");
        let diags = lint(input);
        // Many inner boxes have junction chars on edges and aren't detected
        // by detect_boxes, so few adjacent pairs exist. Just verify no panics
        // and all diagnostics are warnings.
        for d in &diags {
            assert_eq!(d.level, Level::Warning);
        }
    }

    #[test]
    fn demo_diagram_aligned_lambda_boxes_no_warning() {
        // The Lambda boxes in the demo diagram are aligned and should not produce warnings.
        // Test with a simplified version of the Lambda boxes pattern.
        let input = "\
┌────────────────┐  ┌────────────────┐
│  Lambda:       │  │  Lambda:       │
│  api-handler   │  │  snapshot-     │
│  (POST)        │  │  writer        │
└────────────────┘  └────────────────┘";
        let diags = lint(input);
        assert!(diags.is_empty(), "aligned boxes should produce no warnings");
    }

    #[test]
    fn double_boxes_adjacent_misaligned() {
        let input = "\
╔═══╗ ╔═══╗
║ A ║ ║ B ║
║   ║ ╚═══╝
╚═══╝";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("bottom rows differ"));
    }

    #[test]
    fn gap_exactly_at_max_not_adjacent() {
        // Build boxes with exactly MAX_GAP (10) columns between them
        let input = "\
┌──┐          ┌──┐
│A │          │B │
└──┘          └──┘";
        let diags = lint(input);
        // gap = 10, which is >= MAX_GAP, so not adjacent
        assert!(diags.is_empty());
    }

    #[test]
    fn gap_just_under_max_is_adjacent() {
        // Build boxes with MAX_GAP - 1 (9) columns between them
        // Box A: cols 0-3, Box B: cols 12-15, gap = 12 - 3 = 9
        let input = "\
┌──┐        ┌──┐
│A │        │B │
│  │        └──┘
└──┘";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("bottom rows differ"));
    }

    #[test]
    fn input_with_arrows_filters_non_box_nodes() {
        // Arrows produce Node::Arrow entries that should be filtered out
        let input = "\
┌──┐
│  │──►
└──┘";
        let diags = lint(input);
        // Single box with arrow — no adjacent pair, just exercises filter_map
        assert!(diags.is_empty());
    }

    #[test]
    fn boxes_with_arrows_between() {
        let input = "\
┌───┐    ┌───┐
│ A │───►│ B │
│   │    └───┘
└───┘";
        let diags = lint(input);
        // Boxes are still adjacent (arrows are not boxes)
        assert!(diags
            .iter()
            .any(|d| d.message.contains("bottom rows differ")));
    }
}
