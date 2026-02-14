// Box content alignment lint rule.
//
// Detects text content inside boxes that is misaligned or overflows
// box boundaries: content touching edges (no padding), excessive
// vertical padding, and mixed alignment within a single box.

use crate::detect_arrows::detect_arrows;
use crate::detect_boxes::detect_boxes;
use crate::grid::{is_line_drawing, DiagramIR, Node};
use crate::{Diagnostic, Level, LintRule};

pub struct BoxContentAlignmentLint;

const RULE: &str = "box-content-alignment";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Alignment {
    Left,
    Right,
    Centered,
}

/// Returns true if the line contains non-whitespace text content
/// (not just whitespace and not structural box-drawing characters).
fn is_text_content(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty() && !trimmed.chars().any(is_line_drawing)
}

/// Classify a non-empty content line's horizontal alignment.
fn classify_alignment(line: &str) -> Alignment {
    let leading = line.len() - line.trim_start().len();
    let trailing = line.len() - line.trim_end().len();

    // Centered: roughly equal padding on both sides, both substantial
    if leading >= 4 && trailing >= 4 && (leading as isize - trailing as isize).unsigned_abs() <= 1 {
        return Alignment::Centered;
    }

    // Right-aligned: text near right edge with significantly more left padding
    if trailing <= 1 && leading > trailing + 2 {
        return Alignment::Right;
    }

    // Everything else is left-aligned (including indented text)
    Alignment::Left
}

impl LintRule for BoxContentAlignmentLint {
    fn name(&self) -> &str {
        RULE
    }

    fn check(&self, input: &str) -> Vec<Diagnostic> {
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);
        detect_arrows(&mut ir);

        let mut diagnostics = Vec::new();

        for node in &ir.nodes {
            let (bounds, content) = match node {
                Node::Box { bounds, content } => (bounds, content),
                _ => continue,
            };

            let box_row = bounds.top_left.row;
            let box_col = bounds.top_left.col;

            // Check edge padding: non-empty text lines touching left/right edge
            for (i, line) in content.iter().enumerate() {
                if !is_text_content(line) {
                    continue;
                }
                let leading = line.len() - line.trim_start().len();
                let trailing = line.len() - line.trim_end().len();

                if leading == 0 {
                    diagnostics.push(diag(
                        box_row + i + 2, // +1 for content offset, +1 for 1-indexed
                        box_col + 2,
                        "content touches left edge of box (no padding space)".to_string(),
                    ));
                }
                if trailing == 0 {
                    diagnostics.push(diag(
                        box_row + i + 2,
                        box_col + 2,
                        "content touches right edge of box (no padding space)".to_string(),
                    ));
                }
            }

            // Check excessive vertical padding (>1 consecutive empty rows at top/bottom)
            let leading_empty = content.iter().take_while(|l| l.trim().is_empty()).count();
            let trailing_empty = content
                .iter()
                .rev()
                .take_while(|l| l.trim().is_empty())
                .count();

            let has_text = content.iter().any(|l| !l.trim().is_empty());

            if leading_empty > 1 && has_text {
                diagnostics.push(diag(
                    box_row + 2,
                    box_col + 1,
                    format!(
                        "excessive vertical padding: {} empty rows at top of box \
                         (expected at most 1)",
                        leading_empty
                    ),
                ));
            }
            if trailing_empty > 1 && has_text {
                diagnostics.push(diag(
                    box_row + content.len() - trailing_empty + 2,
                    box_col + 1,
                    format!(
                        "excessive vertical padding: {} empty rows at bottom of box \
                         (expected at most 1)",
                        trailing_empty
                    ),
                ));
            }

            // Check mixed alignment among text content lines
            let alignments: Vec<Alignment> = content
                .iter()
                .filter(|l| is_text_content(l))
                .map(|l| classify_alignment(l))
                .collect();

            if alignments.len() >= 2 {
                let has_left = alignments.contains(&Alignment::Left);
                let has_right = alignments.contains(&Alignment::Right);
                let has_centered = alignments.contains(&Alignment::Centered);

                let mixed =
                    (has_left && (has_right || has_centered)) || (has_right && has_centered);

                if mixed {
                    diagnostics.push(diag(
                        box_row + 2,
                        box_col + 1,
                        "mixed content alignment within box".to_string(),
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
        BoxContentAlignmentLint.check(input)
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
    fn single_well_padded_box() {
        let input = "\
┌──────────┐
│ Hello    │
│ World    │
└──────────┘";
        assert!(lint(input).is_empty());
    }

    #[test]
    fn left_aligned_with_indented_sub_items() {
        let input = "\
┌──────────────┐
│ Components:  │
│   - Alpha    │
│   - Beta     │
│   - Gamma    │
└──────────────┘";
        let diags = lint(input);
        assert!(
            diags.is_empty(),
            "indented sub-items under left-aligned label should not warn: {diags:?}"
        );
    }

    #[test]
    fn centered_title() {
        let input = "\
┌──────────────────────┐
│     Hello World      │
└──────────────────────┘";
        assert!(lint(input).is_empty());
    }

    #[test]
    fn box_with_one_empty_row_padding() {
        let input = "\
┌──────────┐
│          │
│ Content  │
│          │
└──────────┘";
        assert!(lint(input).is_empty());
    }

    #[test]
    fn box_with_nested_inner_box() {
        let input = "\
┌──────────────┐
│ ┌──────────┐ │
│ │  inner   │ │
│ └──────────┘ │
└──────────────┘";
        // Lines with box-drawing chars should be skipped for text checks
        let diags = lint(input);
        assert!(
            diags.is_empty(),
            "nested box content should not produce warnings: {diags:?}"
        );
    }

    #[test]
    fn single_non_empty_line_no_mixed_check() {
        let input = "\
┌──────────┐
│          │
│ Content  │
│          │
└──────────┘";
        assert!(lint(input).is_empty());
    }

    #[test]
    fn empty_box() {
        let input = "\
┌──────┐
│      │
│      │
└──────┘";
        assert!(lint(input).is_empty());
    }

    #[test]
    fn only_whitespace_lines() {
        let input = "\
┌──────┐
│      │
│      │
│      │
└──────┘";
        // All empty lines with no text content → no excessive padding warning
        assert!(lint(input).is_empty());
    }

    // -- Detects issues --

    #[test]
    fn content_touches_left_edge() {
        let input = "\
┌──────────┐
│Hello     │
└──────────┘";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("left edge"));
        assert_eq!(diags[0].level, Level::Warning);
    }

    #[test]
    fn content_touches_right_edge() {
        let input = "\
┌──────────┐
│     Hello│
└──────────┘";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("right edge"));
    }

    #[test]
    fn content_touches_both_edges() {
        let input = "\
┌──────┐
│Hello!│
└──────┘";
        let diags = lint(input);
        assert_eq!(diags.len(), 2);
        assert!(diags.iter().any(|d| d.message.contains("left edge")));
        assert!(diags.iter().any(|d| d.message.contains("right edge")));
    }

    #[test]
    fn excessive_top_padding() {
        let input = "\
┌──────────┐
│          │
│          │
│ Content  │
└──────────┘";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("top of box"));
        assert!(diags[0].message.contains("2 empty rows"));
    }

    #[test]
    fn excessive_bottom_padding() {
        let input = "\
┌──────────┐
│ Content  │
│          │
│          │
└──────────┘";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("bottom of box"));
        assert!(diags[0].message.contains("2 empty rows"));
    }

    #[test]
    fn excessive_both_padding() {
        let input = "\
┌──────────┐
│          │
│          │
│ Content  │
│          │
│          │
└──────────┘";
        let diags = lint(input);
        assert_eq!(diags.len(), 2);
        assert!(diags.iter().any(|d| d.message.contains("top of box")));
        assert!(diags.iter().any(|d| d.message.contains("bottom of box")));
    }

    #[test]
    fn mixed_left_and_centered() {
        let input = "\
┌──────────────────────┐
│ Left text            │
│     Centered text    │
└──────────────────────┘";
        let diags = lint(input);
        assert!(
            diags.iter().any(|d| d.message.contains("mixed")),
            "should detect mixed left + centered: {diags:?}"
        );
    }

    #[test]
    fn mixed_left_and_right() {
        let input = "\
┌──────────────────────┐
│ Left text            │
│           Right text │
└──────────────────────┘";
        let diags = lint(input);
        assert!(
            diags.iter().any(|d| d.message.contains("mixed")),
            "should detect mixed left + right: {diags:?}"
        );
    }

    #[test]
    fn mixed_right_and_centered() {
        let input = "\
┌──────────────────────┐
│     Centered text    │
│           Right text │
└──────────────────────┘";
        let diags = lint(input);
        assert!(
            diags.iter().any(|d| d.message.contains("mixed")),
            "should detect mixed right + centered: {diags:?}"
        );
    }

    // -- Double-line boxes --

    #[test]
    fn double_box_content_touching_edge() {
        let input = "\
╔══════════╗
║Hello     ║
╚══════════╝";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("left edge"));
    }

    #[test]
    fn double_box_well_padded() {
        let input = "\
╔══════════╗
║ Hello    ║
╚══════════╝";
        assert!(lint(input).is_empty());
    }

    // -- Edge cases --

    #[test]
    fn long_text_near_right_edge_not_right_aligned() {
        // Text that is left-aligned but happens to be long should not be
        // classified as right-aligned.
        let input = "\
┌──────────────┐
│ Short        │
│ LongerTextXX │
└──────────────┘";
        let diags = lint(input);
        assert!(
            diags.is_empty(),
            "long left-aligned text near right edge should not be flagged: {diags:?}"
        );
    }

    // -- Integration --

    #[test]
    fn rule_name() {
        assert_eq!(BoxContentAlignmentLint.name(), "box-content-alignment");
    }

    #[test]
    fn diag_helper() {
        let d = diag(1, 1, "test".to_string());
        assert_eq!(d.rule, "box-content-alignment");
        assert_eq!(d.level, Level::Warning);
        assert_eq!(d.file, "");
    }

    #[test]
    fn classify_left_aligned() {
        assert_eq!(classify_alignment(" Hello      "), Alignment::Left);
        assert_eq!(classify_alignment("Hello       "), Alignment::Left);
    }

    #[test]
    fn classify_right_aligned() {
        assert_eq!(classify_alignment("          Hello "), Alignment::Right);
        assert_eq!(classify_alignment("      Hello"), Alignment::Right);
    }

    #[test]
    fn classify_centered() {
        assert_eq!(classify_alignment("     Hello      "), Alignment::Centered);
        assert_eq!(classify_alignment("    Hello     "), Alignment::Centered);
    }

    #[test]
    fn classify_indented_as_left() {
        // Indented text (leading > 1 but not enough trailing to be centered)
        assert_eq!(classify_alignment("   Hello         "), Alignment::Left);
    }

    #[test]
    fn is_text_content_checks() {
        assert!(!is_text_content("      "));
        assert!(!is_text_content(""));
        assert!(is_text_content(" Hello "));
        assert!(!is_text_content(" ┌──┐ "));
        assert!(!is_text_content(" │ "));
    }

    #[test]
    fn input_with_arrows_skips_non_box_nodes() {
        let input = "\
┌──────────┐
│ Hello    │──►
└──────────┘";
        // Arrow produces a non-Box node that should be skipped
        assert!(lint(input).is_empty());
    }

    #[test]
    fn demo_diagram_no_crash() {
        let input = include_str!("../examples/demo-flow-diagram.txt");
        let diags = lint(input);
        for d in &diags {
            assert_eq!(d.level, Level::Warning);
            assert_eq!(d.rule, "box-content-alignment");
        }
    }

    #[test]
    fn diagnostic_positions_are_correct() {
        let input = "\
┌──────────┐
│Hello     │
└──────────┘";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        // Box top-left is (0,0). Content starts at row 1. 1-indexed = row 2.
        assert_eq!(diags[0].line, 2);
        // Content starts at col 1. 1-indexed = col 2.
        assert_eq!(diags[0].col, 2);
    }

    #[test]
    fn offset_box_positions() {
        // Box not at origin — use explicit string to preserve leading spaces
        let input = "    ┌──────────┐\n    │Hello     │\n    └──────────┘";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].line, 2); // row 0 + 0 + 2
        assert_eq!(diags[0].col, 6); // col 4 + 2
    }
}
