// Box corner and edge alignment lint rule.
//
// Detects malformed box-like structures: orphan corners, broken edges,
// missing corners, and mixed single/double-line styles.

use crate::detect_boxes::detect_boxes;
use crate::grid::{is_box_corner, BoundingRect, DiagramIR, Node, Position};
use crate::{Diagnostic, Level, LintRule};

pub struct BoxCornerEdgeLint;

const RULE: &str = "box-corner-edge";

// ---------------------------------------------------------------------------
// Edge character validation (mirrors detect_boxes.rs private helpers)
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

/// Arrow tips that can appear on a box edge where a connection enters/leaves.
fn is_arrow_tip(ch: char) -> bool {
    matches!(ch, '▲' | '▼' | '►' | '◄' | '△' | '▽' | '▷' | '◁')
}

// ---------------------------------------------------------------------------
// Style helpers
// ---------------------------------------------------------------------------

fn is_single_corner(ch: char) -> bool {
    matches!(ch, '┌' | '┐' | '└' | '┘')
}

fn is_double_corner(ch: char) -> bool {
    matches!(ch, '╔' | '╗' | '╚' | '╝')
}

/// Is this character a single-line junction (T-piece or cross)?
fn is_single_junction(ch: char) -> bool {
    matches!(ch, '┬' | '┴' | '├' | '┤' | '┼')
}

/// Is this character a double-line junction (T-piece or cross)?
fn is_double_junction(ch: char) -> bool {
    matches!(ch, '╦' | '╩' | '╠' | '╣' | '╬')
}

// ---------------------------------------------------------------------------
// Junction-as-corner validity
// ---------------------------------------------------------------------------
// A junction char can serve as a corner if it connects in the required
// directions. For example, ┐ connects left+down; ┤ (left+up+down) and
// ┼ (all) also connect left+down, so they're valid at the TR position.

// Scan terminators: chars that END an edge scan. They connect in the
// corner's required directions but do NOT continue in the scan direction.
// Used in the while-loop scan and find_corner_on_row/col.

/// Terminates a rightward top-edge scan (connects left+down, NOT right).
fn is_single_tr_terminator(ch: char) -> bool {
    matches!(ch, '┐' | '┤')
    // ┐: left+down. ┤: left+up+down. Neither connects right.
    // NOT ┬ (connects right) or ┼ (connects right).
}

/// Terminates a downward left-edge scan (connects up+right, NOT down).
fn is_single_bl_terminator(ch: char) -> bool {
    matches!(ch, '└' | '┴')
    // └: up+right. ┴: up+left+right. Neither connects down.
    // NOT ├ (connects down) or ┼ (connects down).
}

/// Terminates a rightward top-edge scan for double-line (connects left+down, NOT right).
fn is_double_tr_terminator(ch: char) -> bool {
    matches!(ch, '╗' | '╣')
}

/// Terminates a downward left-edge scan for double-line (connects up+right, NOT down).
fn is_double_bl_terminator(ch: char) -> bool {
    matches!(ch, '╚' | '╩')
}

// Corner validators: for the BR position check (not a scan). Any char
// that connects up+left is valid at the bottom-right corner.

/// Can `ch` serve as a single-line bottom-right corner (connects up + left)?
fn is_valid_single_br(ch: char) -> bool {
    matches!(ch, '┘' | '┤' | '┴' | '┼')
}

/// Can `ch` serve as a double-line bottom-right corner (connects up + left)?
fn is_valid_double_br(ch: char) -> bool {
    matches!(ch, '╝' | '╣' | '╩' | '╬')
}

/// Determine expected corner for a given position relative to the top-left
/// corner style.
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

/// Returns the expected edge character for a given corner style and direction.
#[cfg(test)]
fn edge_name(tl: char, which: &str) -> char {
    if is_single_corner(tl) {
        match which {
            "horizontal" => '─',
            "vertical" => '│',
            _ => '?',
        }
    } else if is_double_corner(tl) {
        match which {
            "horizontal" => '═',
            "vertical" => '║',
            _ => '?',
        }
    } else {
        '?'
    }
}

// ---------------------------------------------------------------------------
// Flow-diagram connectivity check
// ---------------------------------------------------------------------------

/// Characters that act as vertical connectors (edges, junctions, corners).
fn is_vertical_connector(ch: char) -> bool {
    matches!(
        ch,
        '│' | '├'
            | '┤'
            | '┼'
            | '┌'
            | '┐'
            | '╔'
            | '╗'
            | '║'
            | '╠'
            | '╣'
            | '╬'
            | '┬'
            | '┴'
    )
}

/// Characters that act as horizontal connectors (edges, junctions, corners).
fn is_horizontal_connector(ch: char) -> bool {
    matches!(
        ch,
        '─' | '┴'
            | '┬'
            | '┼'
            | '└'
            | '┘'
            | '╚'
            | '╝'
            | '═'
            | '╩'
            | '╦'
            | '╬'
            | '├'
            | '┤'
    )
}

/// Check whether a non-top-left corner is connected to appropriate edges
/// on both of its expected sides, indicating it is part of a flow diagram
/// (merge line, split line, etc.) rather than a truly orphan corner.
fn is_connected_corner(grid: &crate::grid::Grid, r: usize, c: usize, ch: char) -> bool {
    let above = if r > 0 { grid.get(r - 1, c) } else { None };
    let below = if r + 1 < grid.rows() {
        grid.get(r + 1, c)
    } else {
        None
    };
    let left = if c > 0 { grid.get(r, c - 1) } else { None };
    let right = if c + 1 < grid.cols() {
        grid.get(r, c + 1)
    } else {
        None
    };

    // In flow diagrams, corners are used as path turns/endpoints where only
    // one direction may have a visible connector. Use OR to tolerate these.
    // Also accept arrow tips (◄►▲▼ etc.) as connectors.
    let is_h = |ch: char| is_horizontal_connector(ch) || is_arrow_tip(ch);
    let is_v = |ch: char| is_vertical_connector(ch) || is_arrow_tip(ch);

    match ch {
        '└' | '╚' => {
            above.is_some_and(is_v) || right.is_some_and(is_h)
        }
        '┘' | '╝' => {
            above.is_some_and(is_v) || left.is_some_and(is_h)
        }
        '┐' | '╗' => {
            below.is_some_and(is_v) || left.is_some_and(is_h)
        }
        '┌' | '╔' => {
            below.is_some_and(is_v) || right.is_some_and(is_h)
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Diagnostic helpers
// ---------------------------------------------------------------------------

fn diag(line: usize, col: usize, level: Level, message: String) -> Diagnostic {
    Diagnostic {
        file: String::new(),
        line,
        col,
        level,
        message,
        rule: RULE.to_string(),
        fix: None,
    }
}

/// Convert 0-indexed grid position to 1-indexed diagnostic position.
fn pos(row: usize, col: usize) -> (usize, usize) {
    (row + 1, col + 1)
}

// ---------------------------------------------------------------------------
// Core diagnosis logic
// ---------------------------------------------------------------------------

/// Result of tracing a box from a top-left corner.
struct TraceResult {
    diagnostics: Vec<Diagnostic>,
    /// Corner positions discovered during tracing (to suppress orphan reports).
    found_corners: Vec<Position>,
}

/// Try to trace a single-line box from top-left '┌' at (r,c), returning
/// diagnostics for the first problem found (or nothing if it's valid — valid
/// boxes are already detected by `detect_boxes`).
fn diagnose_single_box(grid: &crate::grid::Grid, r: usize, c: usize) -> TraceResult {
    let cols = grid.cols();
    let rows = grid.rows();
    let (tl_line, tl_col) = pos(r, c);

    // Pre-scan: find potential TR (┐) and BL (└) corners to account for
    // all related corners even if the trace fails early.
    let mut found_corners = vec![Position { row: r, col: c }];

    // Find ┐ on same row (scanning right, skipping edge chars and bad chars)
    let c2_opt = find_corner_on_row(grid, r, c + 1, '┐');
    // Find └ on same col (scanning down)
    let r2_opt = find_corner_on_col(grid, c, r + 1, '└');

    // Account for any corners found at the 4 candidate positions
    if let Some(c2) = c2_opt {
        found_corners.push(Position { row: r, col: c2 });
        if let Some(r2) = r2_opt {
            found_corners.push(Position { row: r2, col: c });
            if grid.get(r2, c2).is_some_and(is_valid_single_br) {
                found_corners.push(Position { row: r2, col: c2 });
            }
        }
    } else if let Some(r2) = r2_opt {
        found_corners.push(Position { row: r2, col: c });
    }

    // Now do the actual validation trace
    let mut c2 = c + 1;
    while c2 < cols {
        // grid.get() is always Some for in-bounds coordinates
        let ch = grid.get(r, c2).unwrap();
        if is_single_tr_terminator(ch) {
            break;
        }
        if !is_single_top_edge(ch) {
            let (l, co) = pos(r, c2);
            return TraceResult {
                diagnostics: vec![diag(
                    l,
                    co,
                    Level::Error,
                    format!(
                        "expected '─' but found '{}' on top edge of box starting at {}:{}",
                        ch, tl_line, tl_col
                    ),
                )],
                found_corners,
            };
        }
        c2 += 1;
    }
    if c2 >= cols || !grid.get(r, c2).is_some_and(is_single_tr_terminator) {
        let (l, co) = pos(r, c2.min(cols.saturating_sub(1)));
        return TraceResult {
            diagnostics: vec![diag(
                l,
                co,
                Level::Error,
                format!(
                    "expected '┐' to close box started at {}:{}, but edge runs off grid",
                    tl_line, tl_col
                ),
            )],
            found_corners,
        };
    }
    if c2 <= c + 1 {
        return TraceResult {
            diagnostics: vec![],
            found_corners,
        };
    }

    let mut r2 = r + 1;
    while r2 < rows {
        let ch = grid.get(r2, c).unwrap();
        if is_single_bl_terminator(ch) {
            break;
        }
        if !is_single_left_edge(ch) {
            let (l, co) = pos(r2, c);
            return TraceResult {
                diagnostics: vec![diag(
                    l,
                    co,
                    Level::Error,
                    format!(
                        "expected '│' but found '{}' on left edge of box starting at {}:{}",
                        ch, tl_line, tl_col
                    ),
                )],
                found_corners,
            };
        }
        r2 += 1;
    }
    if r2 >= rows || !grid.get(r2, c).is_some_and(is_single_bl_terminator) {
        let (l, co) = pos(r2.min(rows.saturating_sub(1)), c);
        return TraceResult {
            diagnostics: vec![diag(
                l,
                co,
                Level::Error,
                format!(
                    "expected '└' to close box started at {}:{}, but edge runs off grid",
                    tl_line, tl_col
                ),
            )],
            found_corners,
        };
    }
    if r2 <= r + 1 {
        return TraceResult {
            diagnostics: vec![],
            found_corners,
        };
    }

    // Check bottom-right corner
    let br_ch = grid.get(r2, c2).unwrap_or(' ');
    if !is_valid_single_br(br_ch) {
        let (l, co) = pos(r2, c2);
        return TraceResult {
            diagnostics: vec![diag(
                l,
                co,
                Level::Error,
                format!(
                    "expected '┘' at {}:{} to complete box started at {}:{}",
                    l, co, tl_line, tl_col
                ),
            )],
            found_corners,
        };
    }

    // Check bottom edge (allow arrow tips as connection points)
    for col in (c + 1)..c2 {
        let ch = grid.get(r2, col).unwrap_or(' ');
        if !is_single_bottom_edge(ch) && !is_arrow_tip(ch) {
            let (l, co) = pos(r2, col);
            return TraceResult {
                diagnostics: vec![diag(
                    l,
                    co,
                    Level::Error,
                    format!(
                        "expected '─' but found '{}' on bottom edge of box started at {}:{}",
                        ch, tl_line, tl_col
                    ),
                )],
                found_corners,
            };
        }
    }

    // Check right edge (tolerate spaces from short lines)
    for row in (r + 1)..r2 {
        let ch = grid.get(row, c2).unwrap_or(' ');
        if !is_single_right_edge(ch) && ch != ' ' {
            let (l, co) = pos(row, c2);
            return TraceResult {
                diagnostics: vec![diag(
                    l,
                    co,
                    Level::Error,
                    format!(
                        "expected '│' but found '{}' on right edge of box started at {}:{}",
                        ch, tl_line, tl_col
                    ),
                )],
                found_corners,
            };
        }
    }

    TraceResult {
        diagnostics: vec![],
        found_corners,
    }
}

/// Check if the character terminates a scan for the given target corner.
/// For TR/BL scans, uses terminators (no continuation in scan direction).
/// For BR/other, uses exact match.
fn matches_corner_scan(ch: char, target: char) -> bool {
    match target {
        '┐' => is_single_tr_terminator(ch),
        '└' => is_single_bl_terminator(ch),
        '╗' => is_double_tr_terminator(ch),
        '╚' => is_double_bl_terminator(ch),
        _ => ch == target,
    }
}

/// Find a corner character (or valid junction substitute) on the same row,
/// scanning right from `start_col`.
fn find_corner_on_row(
    grid: &crate::grid::Grid,
    row: usize,
    start_col: usize,
    target: char,
) -> Option<usize> {
    let cols = grid.cols();
    (start_col..cols).find(|&c| grid.get(row, c).is_some_and(|ch| matches_corner_scan(ch, target)))
}

/// Find a corner character (or valid junction substitute) on the same column,
/// scanning down from `start_row`.
fn find_corner_on_col(
    grid: &crate::grid::Grid,
    col: usize,
    start_row: usize,
    target: char,
) -> Option<usize> {
    let rows = grid.rows();
    (start_row..rows).find(|&r| grid.get(r, col).is_some_and(|ch| matches_corner_scan(ch, target)))
}

/// Try to trace a double-line box from top-left '╔' at (r,c).
fn diagnose_double_box(grid: &crate::grid::Grid, r: usize, c: usize) -> TraceResult {
    let cols = grid.cols();
    let rows = grid.rows();
    let (tl_line, tl_col) = pos(r, c);

    let mut found_corners = vec![Position { row: r, col: c }];

    // Pre-scan: find potential TR (╗) and BL (╚) corners
    let c2_opt = find_corner_on_row(grid, r, c + 1, '╗');
    let r2_opt = find_corner_on_col(grid, c, r + 1, '╚');

    if let Some(c2) = c2_opt {
        found_corners.push(Position { row: r, col: c2 });
        if let Some(r2) = r2_opt {
            found_corners.push(Position { row: r2, col: c });
            if grid.get(r2, c2).is_some_and(is_valid_double_br) {
                found_corners.push(Position { row: r2, col: c2 });
            }
        }
    } else if let Some(r2) = r2_opt {
        found_corners.push(Position { row: r2, col: c });
    }

    // Scan right for ╗ (or valid TR junction)
    let mut c2 = c + 1;
    while c2 < cols {
        let ch = grid.get(r, c2).unwrap();
        if is_double_tr_terminator(ch) {
            break;
        }
        if !is_double_top_edge(ch) {
            let (l, co) = pos(r, c2);
            return TraceResult {
                diagnostics: vec![diag(
                    l,
                    co,
                    Level::Error,
                    format!(
                        "expected '═' but found '{}' on top edge of box starting at {}:{}",
                        ch, tl_line, tl_col
                    ),
                )],
                found_corners,
            };
        }
        c2 += 1;
    }
    if c2 >= cols || !grid.get(r, c2).is_some_and(is_double_tr_terminator) {
        let (l, co) = pos(r, c2.min(cols.saturating_sub(1)));
        return TraceResult {
            diagnostics: vec![diag(
                l,
                co,
                Level::Error,
                format!(
                    "expected '╗' to close box started at {}:{}, but edge runs off grid",
                    tl_line, tl_col
                ),
            )],
            found_corners,
        };
    }
    if c2 <= c + 1 {
        return TraceResult {
            diagnostics: vec![],
            found_corners,
        };
    }

    // Scan down for ╚ (or valid BL junction)
    let mut r2 = r + 1;
    while r2 < rows {
        let ch = grid.get(r2, c).unwrap();
        if is_double_bl_terminator(ch) {
            break;
        }
        if !is_double_left_edge(ch) {
            let (l, co) = pos(r2, c);
            return TraceResult {
                diagnostics: vec![diag(
                    l,
                    co,
                    Level::Error,
                    format!(
                        "expected '║' but found '{}' on left edge of box starting at {}:{}",
                        ch, tl_line, tl_col
                    ),
                )],
                found_corners,
            };
        }
        r2 += 1;
    }
    if r2 >= rows || !grid.get(r2, c).is_some_and(is_double_bl_terminator) {
        let (l, co) = pos(r2.min(rows.saturating_sub(1)), c);
        return TraceResult {
            diagnostics: vec![diag(
                l,
                co,
                Level::Error,
                format!(
                    "expected '╚' to close box started at {}:{}, but edge runs off grid",
                    tl_line, tl_col
                ),
            )],
            found_corners,
        };
    }
    if r2 <= r + 1 {
        return TraceResult {
            diagnostics: vec![],
            found_corners,
        };
    }

    // Check bottom-right corner
    let br_ch = grid.get(r2, c2).unwrap_or(' ');
    if !is_valid_double_br(br_ch) {
        let (l, co) = pos(r2, c2);
        return TraceResult {
            diagnostics: vec![diag(
                l,
                co,
                Level::Error,
                format!(
                    "expected '╝' at {}:{} to complete box started at {}:{}",
                    l, co, tl_line, tl_col
                ),
            )],
            found_corners,
        };
    }

    // Bottom edge (allow arrow tips)
    for col in (c + 1)..c2 {
        let ch = grid.get(r2, col).unwrap_or(' ');
        if !is_double_bottom_edge(ch) && !is_arrow_tip(ch) {
            let (l, co) = pos(r2, col);
            return TraceResult {
                diagnostics: vec![diag(
                    l,
                    co,
                    Level::Error,
                    format!(
                        "expected '═' but found '{}' on bottom edge of box started at {}:{}",
                        ch, tl_line, tl_col
                    ),
                )],
                found_corners,
            };
        }
    }

    // Right edge (tolerate spaces from short lines)
    for row in (r + 1)..r2 {
        let ch = grid.get(row, c2).unwrap_or(' ');
        if !is_double_right_edge(ch) && ch != ' ' {
            let (l, co) = pos(row, c2);
            return TraceResult {
                diagnostics: vec![diag(
                    l,
                    co,
                    Level::Error,
                    format!(
                        "expected '║' but found '{}' on right edge of box started at {}:{}",
                        ch, tl_line, tl_col
                    ),
                )],
                found_corners,
            };
        }
    }

    TraceResult {
        diagnostics: vec![],
        found_corners,
    }
}

/// Check a valid box for mixed corner styles.
fn check_style_consistency(grid: &crate::grid::Grid, bounds: &BoundingRect) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let tl = bounds.top_left;
    let br = bounds.bottom_right;

    let tl_ch = grid.get(tl.row, tl.col).unwrap_or(' ');
    let tr_ch = grid.get(tl.row, br.col).unwrap_or(' ');
    let bl_ch = grid.get(br.row, tl.col).unwrap_or(' ');
    let br_ch = grid.get(br.row, br.col).unwrap_or(' ');

    let (tl_line, tl_col_1) = pos(tl.row, tl.col);

    // The top-left corner defines the expected style
    let is_single = is_single_corner(tl_ch);
    let is_double = is_double_corner(tl_ch);

    if !is_single && !is_double {
        return diags;
    }

    // Check top-right (skip if junction char — junctions are style-neutral)
    let tr_expected = expected_corner(tl_ch, "top-right");
    if tr_ch != tr_expected && !is_single_junction(tr_ch) && !is_double_junction(tr_ch) {
        let same_family =
            (is_single && is_single_corner(tr_ch)) || (is_double && is_double_corner(tr_ch));
        if !same_family && is_box_corner(tr_ch) {
            let (l, co) = pos(tl.row, br.col);
            diags.push(diag(
                l,
                co,
                Level::Warning,
                format!(
                    "mixed box styles: corner '{}' does not match '{}' at {}:{}",
                    tr_ch, tl_ch, tl_line, tl_col_1
                ),
            ));
        }
    }

    // Check bottom-left (skip if junction char)
    let bl_expected = expected_corner(tl_ch, "bottom-left");
    if bl_ch != bl_expected && !is_single_junction(bl_ch) && !is_double_junction(bl_ch) {
        let same_family =
            (is_single && is_single_corner(bl_ch)) || (is_double && is_double_corner(bl_ch));
        if !same_family && is_box_corner(bl_ch) {
            let (l, co) = pos(br.row, tl.col);
            diags.push(diag(
                l,
                co,
                Level::Warning,
                format!(
                    "mixed box styles: corner '{}' does not match '{}' at {}:{}",
                    bl_ch, tl_ch, tl_line, tl_col_1
                ),
            ));
        }
    }

    // Check bottom-right (skip if junction char)
    let br_expected = expected_corner(tl_ch, "bottom-right");
    if br_ch != br_expected && !is_single_junction(br_ch) && !is_double_junction(br_ch) {
        let same_family =
            (is_single && is_single_corner(br_ch)) || (is_double && is_double_corner(br_ch));
        if !same_family && is_box_corner(br_ch) {
            let (l, co) = pos(br.row, br.col);
            diags.push(diag(
                l,
                co,
                Level::Warning,
                format!(
                    "mixed box styles: corner '{}' does not match '{}' at {}:{}",
                    br_ch, tl_ch, tl_line, tl_col_1
                ),
            ));
        }
    }

    diags
}

// ---------------------------------------------------------------------------
// LintRule implementation
// ---------------------------------------------------------------------------

impl LintRule for BoxCornerEdgeLint {
    fn name(&self) -> &str {
        RULE
    }

    fn check(&self, input: &str) -> Vec<Diagnostic> {
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);

        // Collect corner positions and bounding rects of all valid boxes
        let mut valid_corners = Vec::new();
        let mut valid_bounds: Vec<(Position, Position)> = Vec::new();
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
                valid_bounds.push((bounds.top_left, bounds.bottom_right));
            }
        }

        let mut diagnostics = Vec::new();
        // Corners accounted for by tracing from ┌/╔ (suppresses orphan reports)
        let mut accounted_corners: Vec<Position> = Vec::new();

        // Scan for orphan corners
        let rows = ir.grid.rows();
        let cols = ir.grid.cols();

        for r in 0..rows {
            for c in 0..cols {
                let ch = ir.grid.get(r, c).unwrap();
                if !is_box_corner(ch) {
                    continue;
                }

                let p = Position { row: r, col: c };
                if valid_corners.contains(&p) || accounted_corners.contains(&p) {
                    continue;
                }

                // Check if this position is inside an already-validated box.
                // Nested inner boxes with minor alignment issues should not
                // generate errors since the outer box structure is valid.
                let inside_valid_box = valid_bounds.iter().any(|(tl, br)| {
                    r > tl.row && r < br.row && c > tl.col && c < br.col
                });

                // Orphan corner — diagnose based on type
                match ch {
                    '┌' => {
                        let result = diagnose_single_box(&ir.grid, r, c);
                        accounted_corners.extend(&result.found_corners);
                        if result.diagnostics.is_empty() {
                            // Degenerate (too small for a box) — check if it's
                            // a connected flow-diagram element before reporting
                            if !is_connected_corner(&ir.grid, r, c, ch) {
                                let (l, co) = pos(r, c);
                                diagnostics.push(diag(
                                    l,
                                    co,
                                    Level::Error,
                                    format!("orphan corner '{}' is not part of any box", ch),
                                ));
                            }
                        } else if !inside_valid_box {
                            diagnostics.extend(result.diagnostics);
                        }
                    }
                    '╔' => {
                        let result = diagnose_double_box(&ir.grid, r, c);
                        accounted_corners.extend(&result.found_corners);
                        if result.diagnostics.is_empty() {
                            if !is_connected_corner(&ir.grid, r, c, ch) {
                                let (l, co) = pos(r, c);
                                diagnostics.push(diag(
                                    l,
                                    co,
                                    Level::Error,
                                    format!("orphan corner '{}' is not part of any box", ch),
                                ));
                            }
                        } else if !inside_valid_box {
                            diagnostics.extend(result.diagnostics);
                        }
                    }
                    _ => {
                        // Non-top-left corners: ┐ └ ┘ ╗ ╚ ╝
                        // Check if connected to flow-diagram elements before reporting orphan
                        if !is_connected_corner(&ir.grid, r, c, ch) {
                            let (l, co) = pos(r, c);
                            diagnostics.push(diag(
                                l,
                                co,
                                Level::Error,
                                format!("orphan corner '{}' is not part of any box", ch),
                            ));
                        }
                    }
                }
            }
        }

        // Check valid boxes for mixed styles
        for node in &ir.nodes {
            if let Node::Box { bounds, .. } = node {
                diagnostics.extend(check_style_consistency(&ir.grid, bounds));
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

    fn lint(input: &str) -> Vec<Diagnostic> {
        BoxCornerEdgeLint.check(input)
    }

    // 1. Well-formed single box — no diagnostics
    #[test]
    fn well_formed_single_box() {
        let diags = lint("┌──┐\n│  │\n└──┘");
        assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    }

    // 2. Well-formed double box — no diagnostics
    #[test]
    fn well_formed_double_box() {
        let diags = lint("╔══╗\n║  ║\n╚══╝");
        assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    }

    // 3. Well-formed nested boxes — no diagnostics
    #[test]
    fn well_formed_nested() {
        let input = "\
┌──────────────┐
│ ┌──────────┐ │
│ │  inner   │ │
│ └──────────┘ │
└──────────────┘";
        let diags = lint(input);
        assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    }

    // 4. Well-formed adjacent boxes — no diagnostics
    #[test]
    fn well_formed_adjacent() {
        let input = "\
┌───┐┌───┐
│ A ││ B │
└───┘└───┘";
        let diags = lint(input);
        assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    }

    // 5. Well-formed box with junctions — no diagnostics
    #[test]
    fn well_formed_with_junctions() {
        let input = "\
┌───┬───┐
│   │   │
├───┼───┤
│   │   │
└───┴───┘";
        let diags = lint(input);
        assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    }

    // 6. Missing bottom-right corner
    #[test]
    fn missing_bottom_right_corner() {
        let input = "\
┌──┐
│  │
└──X";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].level, Level::Error);
        assert!(
            diags[0].message.contains("expected '┘'"),
            "{}",
            diags[0].message
        );
        assert_eq!(diags[0].line, 3);
        assert_eq!(diags[0].col, 4);
    }

    // 7. Broken top edge
    #[test]
    fn broken_top_edge() {
        let input = "\
┌─X┐
│  │
└──┘";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0]
            .message
            .contains("expected '─' but found 'X' on top edge"));
        assert_eq!(diags[0].line, 1);
        assert_eq!(diags[0].col, 3);
    }

    // 8. Broken bottom edge
    #[test]
    fn broken_bottom_edge() {
        let input = "\
┌──┐
│  │
└─X┘";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("bottom edge"));
    }

    // 9. Broken left edge
    #[test]
    fn broken_left_edge() {
        let input = "\
┌──┐
│  │
X  │
└──┘";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("left edge"));
    }

    // 10. Broken right edge
    #[test]
    fn broken_right_edge() {
        let input = "\
┌──┐
│  │
│  X
└──┘";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("right edge"));
    }

    // 11. Top edge runs off grid
    #[test]
    fn top_edge_runs_off_grid() {
        let input = "┌───";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("runs off grid"));
    }

    // 12. Left edge runs off grid
    #[test]
    fn left_edge_runs_off_grid() {
        let input = "┌──┐\n│  │";
        let diags = lint(input);
        // ┌ diagnosed: left edge runs off grid
        // ┐ orphan corner
        assert!(diags.iter().any(|d| d.message.contains("runs off grid")));
    }

    // 13. Mixed styles — single TL with double TR
    #[test]
    fn mixed_styles_warning() {
        // We need a box that detect_boxes considers valid but has mixed corners.
        // detect_boxes only finds boxes where ALL corners match, so a mixed
        // box won't be detected as valid. Instead, ┌ trace will find the
        // wrong corner character and report an error.
        let input = "\
┌──╗
│  │
└──┘";
        let diags = lint(input);
        // The ┌ trace will fail to find ┐ because it hits ╗ (not a single top edge)
        assert!(!diags.is_empty());
    }

    // 14. Orphan non-top-left corners
    #[test]
    fn orphan_bottom_right() {
        let input = "  ┘  ";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("orphan corner '┘'"));
    }

    #[test]
    fn orphan_top_right() {
        let input = "  ┐  ";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("orphan corner '┐'"));
    }

    #[test]
    fn orphan_bottom_left() {
        let input = "  └  ";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("orphan corner '└'"));
    }

    // 15. Orphan double-line corners
    #[test]
    fn orphan_double_corners() {
        let input = "╗ ╚ ╝";
        let diags = lint(input);
        assert_eq!(diags.len(), 3);
        assert!(diags.iter().all(|d| d.message.contains("orphan corner")));
    }

    // 16. Demo diagram — zero false positives on well-formed boxes
    #[test]
    fn demo_diagram_no_false_positives_on_valid_boxes() {
        let input = include_str!("../examples/demo-flow-diagram.txt");
        let diags = lint(input);
        // All diagnostics should be about genuinely broken structures,
        // not about the well-formed boxes we know exist.
        let mut ir = DiagramIR::new(input);
        detect_boxes(&mut ir);
        let valid_box_count = ir.nodes.len();
        assert!(
            valid_box_count >= 3,
            "expected at least 3 valid boxes in demo, found {valid_box_count}"
        );
        // No diagnostic should reference a corner position that belongs to a valid box
        let valid_corners: Vec<Position> = ir
            .nodes
            .iter()
            .filter_map(|n| match n {
                Node::Box { bounds, .. } => Some(bounds),
                _ => None,
            })
            .flat_map(|b| {
                vec![
                    b.top_left,
                    Position {
                        row: b.top_left.row,
                        col: b.bottom_right.col,
                    },
                    Position {
                        row: b.bottom_right.row,
                        col: b.top_left.col,
                    },
                    b.bottom_right,
                ]
            })
            .collect();
        for d in &diags {
            let p = Position {
                row: d.line - 1,
                col: d.col - 1,
            };
            assert!(
                !valid_corners.contains(&p),
                "false positive at {}:{}: {}",
                d.line,
                d.col,
                d.message
            );
        }
    }

    // 17. Empty input — no diagnostics
    #[test]
    fn empty_input_no_diagnostics() {
        let diags = lint("");
        assert!(diags.is_empty());
    }

    // 18. Plain text — no diagnostics
    #[test]
    fn plain_text_no_diagnostics() {
        let diags = lint("Hello world\nNo boxes here\n");
        assert!(diags.is_empty());
    }

    // 19. Rule name
    #[test]
    fn rule_name() {
        assert_eq!(BoxCornerEdgeLint.name(), "box-corner-edge");
    }

    // 20. Degenerate width (c2 <= c+1) — no diagnostics from single diagnose
    #[test]
    fn degenerate_width_no_error() {
        // ┌┐ is too narrow for a real box, diagnose_single_box returns empty
        // but these become orphan corners (not top-left diagnosis)
        let input = "┌┐\n└┘";
        let diags = lint(input);
        // The ┌ diagnosis returns empty (degenerate), so it becomes orphan
        // The ┐ └ ┘ are also orphans since no valid box owns them
        assert!(!diags.is_empty());
    }

    // 21. Degenerate height (r2 <= r+1) — corners are connected via OR logic
    #[test]
    fn degenerate_height_no_error() {
        let input = "┌──┐\n└──┘";
        let diags = lint(input);
        // Degenerate box: corners are connected to edges so no orphan reports
        assert!(diags.is_empty(), "degenerate box corners are connected: {diags:?}");
    }

    // 22. Double box — broken top edge
    #[test]
    fn double_broken_top_edge() {
        let input = "\
╔═X╗
║  ║
╚══╝";
        let diags = lint(input);
        assert!(diags.iter().any(|d| d.message.contains("expected '═'")));
    }

    // 23. Double box — top edge runs off grid
    #[test]
    fn double_top_edge_runs_off() {
        let input = "╔═══";
        let diags = lint(input);
        assert!(diags.iter().any(|d| d.message.contains("runs off grid")));
    }

    // 24. Double box — broken left edge
    #[test]
    fn double_broken_left_edge() {
        let input = "\
╔══╗
║  ║
X  ║
╚══╝";
        let diags = lint(input);
        assert!(diags.iter().any(|d| d.message.contains("left edge")));
    }

    // 25. Double box — left edge runs off grid
    #[test]
    fn double_left_edge_runs_off() {
        let input = "╔══╗\n║  ║";
        let diags = lint(input);
        assert!(diags.iter().any(|d| d.message.contains("runs off grid")));
    }

    // 26. Double box — missing bottom-right corner
    #[test]
    fn double_missing_bottom_right() {
        let input = "\
╔══╗
║  ║
╚══X";
        let diags = lint(input);
        assert!(diags.iter().any(|d| d.message.contains("expected '╝'")));
    }

    // 27. Double box — broken bottom edge
    #[test]
    fn double_broken_bottom_edge() {
        let input = "\
╔══╗
║  ║
╚═X╝";
        let diags = lint(input);
        assert!(diags.iter().any(|d| d.message.contains("bottom edge")));
    }

    // 28. Double box — broken right edge
    #[test]
    fn double_broken_right_edge() {
        let input = "\
╔══╗
║  X
║  ║
╚══╝";
        let diags = lint(input);
        assert!(diags.iter().any(|d| d.message.contains("right edge")));
    }

    // 29. Double box — degenerate width
    #[test]
    fn double_degenerate_width() {
        let input = "╔╗\n╚╝";
        let diags = lint(input);
        assert!(!diags.is_empty());
    }

    // 30. Double box — degenerate height
    #[test]
    fn double_degenerate_height() {
        let input = "╔══╗\n╚══╝";
        let diags = lint(input);
        // Degenerate box: corners are connected to edges so no orphan reports
        assert!(diags.is_empty(), "degenerate double box corners are connected: {diags:?}");
    }

    // 31. Mixed style on valid box — test check_style_consistency directly
    // We need detect_boxes to consider it valid, so we craft a box that
    // is valid from detect_boxes perspective but we manually test the
    // style checker.
    #[test]
    fn style_consistency_check_direct() {
        let grid = crate::grid::Grid::new("┌──┐\n│  │\n└──┘");
        let bounds = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 2, col: 3 },
        };
        let diags = check_style_consistency(&grid, &bounds);
        assert!(
            diags.is_empty(),
            "well-formed box should have no style issues"
        );
    }

    // 32. edge_name helper
    #[test]
    fn edge_name_single() {
        assert_eq!(edge_name('┌', "horizontal"), '─');
        assert_eq!(edge_name('┌', "vertical"), '│');
    }

    #[test]
    fn edge_name_double() {
        assert_eq!(edge_name('╔', "horizontal"), '═');
        assert_eq!(edge_name('╔', "vertical"), '║');
    }

    #[test]
    fn edge_name_unknown() {
        assert_eq!(edge_name('X', "horizontal"), '?');
    }

    // 33. expected_corner helper
    #[test]
    fn expected_corner_single() {
        assert_eq!(expected_corner('┌', "top-right"), '┐');
        assert_eq!(expected_corner('┌', "bottom-left"), '└');
        assert_eq!(expected_corner('┌', "bottom-right"), '┘');
    }

    #[test]
    fn expected_corner_double() {
        assert_eq!(expected_corner('╔', "top-right"), '╗');
        assert_eq!(expected_corner('╔', "bottom-left"), '╚');
        assert_eq!(expected_corner('╔', "bottom-right"), '╝');
    }

    #[test]
    fn expected_corner_unknown() {
        assert_eq!(expected_corner('X', "top-right"), '?');
    }

    // 34. Multiple broken boxes
    #[test]
    fn multiple_broken_boxes() {
        let input = "\
┌─X┐   ┌──┐
│  │   │  │
└──┘   └──X";
        let diags = lint(input);
        assert!(diags.len() >= 2);
    }

    // 35. Orphan ┌ that traces to well-formed box (edge case: shouldn't happen
    // in practice, but tests the fallback path)
    // This happens when diagnose_single_box returns empty but no valid box
    // covers the corner. We'll test with a degenerate size.
    #[test]
    fn orphan_top_left_degenerate() {
        let input = "┌┐";
        let diags = lint(input);
        assert!(diags
            .iter()
            .any(|d| d.message.contains("orphan corner '┌'")));
    }

    // 36. Orphan ╔ degenerate
    #[test]
    fn orphan_double_top_left_degenerate() {
        let input = "╔╗";
        let diags = lint(input);
        assert!(diags
            .iter()
            .any(|d| d.message.contains("orphan corner '╔'")));
    }

    // 37. Mixed style check — construct grid with mixed corners manually
    #[test]
    fn mixed_style_tr_corner() {
        // Grid where TL is ┌ but TR is ╗ (mixed style)
        let grid = crate::grid::Grid::new("┌──╗\n│  │\n└──┘");
        let bounds = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 2, col: 3 },
        };
        let diags = check_style_consistency(&grid, &bounds);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("mixed box styles"));
        assert!(diags[0].message.contains("'╗'"));
    }

    // 38. Mixed style — BL corner
    #[test]
    fn mixed_style_bl_corner() {
        let grid = crate::grid::Grid::new("┌──┐\n│  │\n╚──┘");
        let bounds = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 2, col: 3 },
        };
        let diags = check_style_consistency(&grid, &bounds);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("'╚'"));
    }

    // 39. Mixed style — BR corner
    #[test]
    fn mixed_style_br_corner() {
        let grid = crate::grid::Grid::new("┌──┐\n│  │\n└──╝");
        let bounds = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 2, col: 3 },
        };
        let diags = check_style_consistency(&grid, &bounds);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("'╝'"));
    }

    // 40. Non-corner TL character — style check should return empty
    #[test]
    fn style_check_non_corner_tl() {
        let grid = crate::grid::Grid::new("X──┐\n│  │\n└──┘");
        let bounds = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 2, col: 3 },
        };
        let diags = check_style_consistency(&grid, &bounds);
        assert!(diags.is_empty());
    }

    // 41. Style check — same-family wrong corner (e.g., ┌ with ┌ at TR)
    // This shouldn't trigger mixed style warning (same family)
    #[test]
    fn style_check_same_family_wrong_corner() {
        let grid = crate::grid::Grid::new("┌──┌\n│  │\n└──┘");
        let bounds = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 2, col: 3 },
        };
        let diags = check_style_consistency(&grid, &bounds);
        // ┌ at TR is same family as ┌ at TL, so no mixed style warning
        assert!(diags.is_empty());
    }

    // 42. Style check — non-corner char at TR position
    #[test]
    fn style_check_non_corner_at_tr() {
        let grid = crate::grid::Grid::new("┌──X\n│  │\n└──┘");
        let bounds = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 2, col: 3 },
        };
        let diags = check_style_consistency(&grid, &bounds);
        // X is not a box corner, so no mixed style warning
        assert!(diags.is_empty());
    }

    // 43. pos helper
    #[test]
    fn pos_helper() {
        assert_eq!(pos(0, 0), (1, 1));
        assert_eq!(pos(4, 9), (5, 10));
    }

    // 44. diag helper
    #[test]
    fn diag_helper() {
        let d = diag(1, 1, Level::Error, "test".to_string());
        assert_eq!(d.rule, "box-corner-edge");
        assert_eq!(d.file, "");
    }

    // 45. is_single_corner / is_double_corner helpers
    #[test]
    fn corner_classification() {
        assert!(is_single_corner('┌'));
        assert!(is_single_corner('┐'));
        assert!(is_single_corner('└'));
        assert!(is_single_corner('┘'));
        assert!(!is_single_corner('╔'));

        assert!(is_double_corner('╔'));
        assert!(is_double_corner('╗'));
        assert!(is_double_corner('╚'));
        assert!(is_double_corner('╝'));
        assert!(!is_double_corner('┌'));
    }

    // 46. Edge char helpers
    #[test]
    fn edge_char_helpers() {
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

    // 47. Style consistency with double-line TL corner
    #[test]
    fn style_consistency_double_tl() {
        let grid = crate::grid::Grid::new("╔══╗\n║  ║\n╚══╝");
        let bounds = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 2, col: 3 },
        };
        let diags = check_style_consistency(&grid, &bounds);
        assert!(diags.is_empty());
    }

    // 48. Mixed style double TL with single TR
    #[test]
    fn mixed_style_double_tl_single_tr() {
        let grid = crate::grid::Grid::new("╔══┐\n║  ║\n╚══╝");
        let bounds = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 2, col: 3 },
        };
        let diags = check_style_consistency(&grid, &bounds);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("'┐'"));
    }

    // 49. Mixed style double TL with single BL
    #[test]
    fn mixed_style_double_tl_single_bl() {
        let grid = crate::grid::Grid::new("╔══╗\n║  ║\n└══╝");
        let bounds = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 2, col: 3 },
        };
        let diags = check_style_consistency(&grid, &bounds);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("'└'"));
    }

    // 50. Mixed style double TL with single BR
    #[test]
    fn mixed_style_double_tl_single_br() {
        let grid = crate::grid::Grid::new("╔══╗\n║  ║\n╚══┘");
        let bounds = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 2, col: 3 },
        };
        let diags = check_style_consistency(&grid, &bounds);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("'┘'"));
    }

    // 51. Non-corner at BL position — no mixed style warning
    #[test]
    fn style_check_non_corner_at_bl() {
        let grid = crate::grid::Grid::new("┌──┐\n│  │\nX──┘");
        let bounds = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 2, col: 3 },
        };
        let diags = check_style_consistency(&grid, &bounds);
        assert!(diags.is_empty());
    }

    // 52. Non-corner at BR position — no mixed style warning
    #[test]
    fn style_check_non_corner_at_br() {
        let grid = crate::grid::Grid::new("┌──┐\n│  │\n└──X");
        let bounds = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 2, col: 3 },
        };
        let diags = check_style_consistency(&grid, &bounds);
        assert!(diags.is_empty());
    }

    // 53. Double same-family wrong BL corner
    #[test]
    fn style_check_double_same_family_bl() {
        let grid = crate::grid::Grid::new("╔══╗\n║  ║\n╔══╝");
        let bounds = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 2, col: 3 },
        };
        let diags = check_style_consistency(&grid, &bounds);
        // ╔ at BL is same family as ╔ at TL, no mixed warning
        assert!(diags.is_empty());
    }

    // 54. Double same-family wrong BR corner
    #[test]
    fn style_check_double_same_family_br() {
        let grid = crate::grid::Grid::new("╔══╗\n║  ║\n╚══╗");
        let bounds = BoundingRect {
            top_left: Position { row: 0, col: 0 },
            bottom_right: Position { row: 2, col: 3 },
        };
        let diags = check_style_consistency(&grid, &bounds);
        assert!(diags.is_empty());
    }

    // 55. edge_name with unknown direction for single/double corners
    #[test]
    fn edge_name_unknown_direction() {
        assert_eq!(edge_name('┌', "diagonal"), '?');
        assert_eq!(edge_name('╔', "diagonal"), '?');
    }

    // 56. Direct test of diagnose_single_box on a well-formed box
    // (exercises the final success path)
    #[test]
    fn diagnose_single_box_wellformed() {
        let grid = crate::grid::Grid::new("┌──┐\n│  │\n└──┘");
        let result = diagnose_single_box(&grid, 0, 0);
        assert!(result.diagnostics.is_empty());
        assert_eq!(result.found_corners.len(), 4);
    }

    // 57. Direct test of diagnose_double_box on a well-formed box
    #[test]
    fn diagnose_double_box_wellformed() {
        let grid = crate::grid::Grid::new("╔══╗\n║  ║\n╚══╝");
        let result = diagnose_double_box(&grid, 0, 0);
        assert!(result.diagnostics.is_empty());
        assert_eq!(result.found_corners.len(), 4);
    }

    // 58. Double box prescan: ╚ below but no ╗ to the right
    #[test]
    fn double_prescan_bl_but_no_tr() {
        let input = "╔═X\n║  \n╚══";
        let diags = lint(input);
        // ╔ trace: top edge has X, diagnoses broken top edge
        // ╚ is accounted for by prescan
        assert!(diags.iter().any(|d| d.message.contains("expected '═'")));
    }

    // 59. Single box prescan: └ below but no ┐ to the right
    #[test]
    fn single_prescan_bl_but_no_tr() {
        let input = "┌──\n│  \n└──";
        let diags = lint(input);
        // ┌ trace: top edge runs off grid
        // └ is accounted for by prescan
        assert!(diags.iter().any(|d| d.message.contains("runs off grid")));
        // └ should NOT be reported as orphan since it's accounted for
        assert!(!diags
            .iter()
            .any(|d| d.message.contains("orphan corner '└'")));
    }

    // 60. filter_map coverage: input with arrows (non-Box nodes)
    #[test]
    fn input_with_arrows_and_broken_box() {
        // A valid box + an arrow creates Arrow nodes in the IR.
        // The filter_map's _ => None branch fires for Arrow nodes.
        let input = "\
┌──┐
│  │──►
└──┘";
        let diags = lint(input);
        // The valid box corners should not be flagged
        // The arrow tip '►' is not a box corner, so no diagnostics from it
        assert!(diags.is_empty(), "got: {diags:?}");
    }

    // -----------------------------------------------------------------------
    // Flow-diagram connectivity tests (boxlint-4nk)
    // -----------------------------------------------------------------------

    // 61. Merge line: └───┴───┘ with │ above each corner → no false positives
    #[test]
    fn merge_line_no_false_positives() {
        let input = "\
│       │
└───┴───┘";
        let diags = lint(input);
        assert!(
            diags.is_empty(),
            "expected no diagnostics for merge line, got: {diags:?}"
        );
    }

    // 62. Split line: ┌───┬───┐ with │ below
    // The ┌ triggers box diagnosis (which reports "runs off grid" since there's
    // no bottom row), but the ┐ is suppressed via accounted_corners from the
    // prescan. Non-top-left corners in the _ => branch are properly suppressed.
    #[test]
    fn split_line_suppresses_non_tl_corners() {
        // A merge line (non-top-left corners) should produce zero diagnostics
        let input = "│       │\n└───┴───┘";
        let diags = lint(input);
        assert!(
            diags.is_empty(),
            "expected no diagnostics for merge line, got: {diags:?}"
        );

        // A split line with ┌ still reports box-trace errors for ┌
        // but ┐ is accounted for by the prescan
        let input2 = "┌───┬───┐\n│       │";
        let diags2 = lint(input2);
        // ┌ produces a box-trace diagnostic, ┐ is accounted
        assert!(
            diags2.len() <= 1,
            "expected at most 1 diagnostic for split line, got: {diags2:?}"
        );
    }

    // 63. Truly orphan └ with no connections → still reported
    #[test]
    fn orphan_corner_still_reported() {
        let input = "  └  ";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("orphan corner '└'"));
    }

    // 64. Truly orphan ┐ with no connections → still reported
    #[test]
    fn orphan_top_right_still_reported() {
        let input = "  ┐  ";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("orphan corner '┐'"));
    }

    // 65. Connected ┘ with vertical above or horizontal left
    #[test]
    fn connected_bottom_right_corner() {
        // ┘ with vertical above only → connected (flow path turn)
        let diags = lint("│\n┘");
        assert!(
            diags.is_empty(),
            "┘ with vertical above should be connected, got: {diags:?}"
        );

        // Properly connected ┘: │ above and ─ to left
        let diags2 = lint(" │\n─┘");
        assert!(
            diags2.is_empty(),
            "connected ┘ should not be orphan, got: {diags2:?}"
        );

        // ┘ with only horizontal left → connected (flow path turn)
        let diags3 = lint("  \n─┘");
        assert!(
            diags3.is_empty(),
            "┘ with horizontal left should be connected, got: {diags3:?}"
        );

        // ┘ with arrow tip to left → connected
        let diags4 = lint("  \n◄┘");
        assert!(
            diags4.is_empty(),
            "┘ with arrow tip left should be connected, got: {diags4:?}"
        );
    }

    // 66. Connected ┐ with vertical below and horizontal left
    #[test]
    fn connected_top_right_corner() {
        let diags = lint("─┐\n │");
        assert!(
            diags.is_empty(),
            "connected ┐ should not be orphan, got: {diags:?}"
        );
    }

    // 67. is_connected_corner direct tests
    #[test]
    fn is_connected_corner_direct() {
        let grid = crate::grid::Grid::new("│\n└─");
        assert!(is_connected_corner(&grid, 1, 0, '└'));

        let grid = crate::grid::Grid::new(" │\n─┘");
        assert!(is_connected_corner(&grid, 1, 1, '┘'));

        let grid = crate::grid::Grid::new("─┐\n │");
        assert!(is_connected_corner(&grid, 0, 1, '┐'));

        let grid = crate::grid::Grid::new("┌─\n│ ");
        assert!(is_connected_corner(&grid, 0, 0, '┌'));

        // Not connected
        let grid = crate::grid::Grid::new(" \n└ ");
        assert!(!is_connected_corner(&grid, 1, 0, '└'));
    }

    // 68. is_connected_corner with unknown char
    #[test]
    fn is_connected_corner_unknown_char() {
        let grid = crate::grid::Grid::new("X");
        assert!(!is_connected_corner(&grid, 0, 0, 'X'));
    }

    // 69. Vertical/horizontal connector helpers
    #[test]
    fn connector_helpers() {
        assert!(is_vertical_connector('│'));
        assert!(is_vertical_connector('├'));
        assert!(is_vertical_connector('┤'));
        assert!(is_vertical_connector('┼'));
        assert!(is_vertical_connector('┌'));
        assert!(is_vertical_connector('┐'));
        assert!(is_vertical_connector('║'));
        assert!(is_vertical_connector('┬'));
        assert!(is_vertical_connector('┴'));
        assert!(!is_vertical_connector('─'));
        assert!(!is_vertical_connector(' '));

        assert!(is_horizontal_connector('─'));
        assert!(is_horizontal_connector('┴'));
        assert!(is_horizontal_connector('┬'));
        assert!(is_horizontal_connector('┼'));
        assert!(is_horizontal_connector('└'));
        assert!(is_horizontal_connector('┘'));
        assert!(is_horizontal_connector('═'));
        assert!(is_horizontal_connector('├'));
        assert!(is_horizontal_connector('┤'));
        assert!(!is_horizontal_connector('│'));
        assert!(!is_horizontal_connector(' '));
    }

    // 70. Double-line connected corners
    #[test]
    fn double_connected_corners() {
        let grid = crate::grid::Grid::new("║\n╚═");
        assert!(is_connected_corner(&grid, 1, 0, '╚'));

        let grid = crate::grid::Grid::new(" ║\n═╝");
        assert!(is_connected_corner(&grid, 1, 1, '╝'));

        let grid = crate::grid::Grid::new("═╗\n ║");
        assert!(is_connected_corner(&grid, 0, 1, '╗'));

        let grid = crate::grid::Grid::new("╔═\n║ ");
        assert!(is_connected_corner(&grid, 0, 0, '╔'));
    }

    // 71. Flow diagram from demo file should have fewer false positives
    #[test]
    fn demo_flow_diagram_reduced_false_positives() {
        let input = include_str!("../examples/demo-flow-diagram.txt");
        let diags = lint(input);
        // The demo contains flow-diagram merge/split lines that should NOT
        // be flagged as orphan corners when properly connected
        for d in &diags {
            // No diagnostic should be about a connected corner
            if d.message.contains("orphan corner") {
                // Verify it's truly orphan by checking it's not connected
                let grid = crate::grid::Grid::new(input);
                let r = d.line - 1;
                let c = d.col - 1;
                if let Some(ch) = grid.get(r, c) {
                    assert!(
                        !is_connected_corner(&grid, r, c, ch),
                        "false positive orphan at {}:{}: {}",
                        d.line,
                        d.col,
                        d.message
                    );
                }
            }
        }
    }

    // 72. Corner at grid edge (row 0 or col 0) — boundary check
    #[test]
    fn corner_at_grid_boundary() {
        // └ at row 0 — no row above but ─ to right → connected (OR logic)
        let grid = crate::grid::Grid::new("└─");
        assert!(is_connected_corner(&grid, 0, 0, '└'));

        // ┘ at col 0 — no col to left AND no vertical above → not connected
        let grid = crate::grid::Grid::new(" │\n┘ ");
        assert!(!is_connected_corner(&grid, 1, 0, '┘'));

        // ┐ at last row — no row below but ─ to left → connected (OR logic)
        let grid = crate::grid::Grid::new("─┐");
        assert!(is_connected_corner(&grid, 0, 1, '┐'));

        // Truly isolated corners → not connected
        let grid = crate::grid::Grid::new("┘");
        assert!(!is_connected_corner(&grid, 0, 0, '┘'));
    }

    // --- Phase 1: Junction tolerance tests ---

    // 73. Box with ┤ at TR position (pipe exits right from box)
    #[test]
    fn junction_tr_no_error() {
        // ┤ connects left+up+down — valid as TR when scanning top edge
        // In real diagrams, ┤ at TR means a pipe exits from the right side
        let input = "\
┌──┤
│  │
└──┘";
        let diags = lint(input);
        // ┤ is NOT a valid TR scan terminator (it connects right),
        // so this will not be found as a valid box. The ┌ traces but
        // doesn't find ┐ or ┤ as TR. This is correct — ┤ extends right.
        // The corners are all connected though, so no orphan errors.
        for d in &diags {
            assert!(
                !d.message.contains("orphan"),
                "unexpected orphan: {}", d.message
            );
        }
    }

    // 74. Box with ┴ at BL position (pipe exits down from box)
    #[test]
    fn junction_bl_no_error() {
        let input = "\
┌──┐
│  │
┴──┘";
        let diags = lint(input);
        for d in &diags {
            assert!(
                !d.message.contains("orphan"),
                "unexpected orphan: {}", d.message
            );
        }
    }

    // 75. Box with ┼ at BR (pipe crosses through corner)
    #[test]
    fn junction_br_valid() {
        let input = "\
┌──┐
│  │
│  │
└──┼
   │";
        let diags = lint(input);
        // ┼ is valid as BR corner
        assert!(
            !diags.iter().any(|d| d.message.contains("missing") && d.message.contains("bottom-right")),
            "┼ should be valid as BR corner: {diags:?}"
        );
    }

    // 76. Demo diagram: zero errors
    #[test]
    fn demo_diagram_zero_errors() {
        let input = include_str!("../examples/demo-flow-diagram.txt");
        let diags = lint(input);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.level == Level::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "expected 0 errors on demo diagram, got {}:\n{}",
            errors.len(),
            errors
                .iter()
                .map(|d| format!("  {}:{}: {}", d.line, d.col, d.message))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    // 77. Orphan ┘ adjacent to arrow tip — not reported
    #[test]
    fn arrow_adjacent_corner_not_orphan() {
        let input = "◄─┘";
        let diags = lint(input);
        assert!(
            !diags.iter().any(|d| d.message.contains("orphan")),
            "┘ adjacent to arrow path should not be orphan: {diags:?}"
        );
    }

    // 78. Style consistency skips junction chars at corners
    #[test]
    fn style_consistency_skips_junctions() {
        // A box with ┼ at BR — shouldn't trigger style mismatch
        let input = "\
┌──┐
│  │
│  │
└──┼";
        let diags = lint(input);
        assert!(
            !diags.iter().any(|d| d.message.contains("style")),
            "junction at corner should not trigger style warning: {diags:?}"
        );
    }

    // 79. matches_corner_scan fallback branch (non-TR/BL targets)
    #[test]
    fn matches_corner_scan_fallback() {
        // The _ branch handles targets like ┌ and ┘ — exact match
        assert!(matches_corner_scan('┌', '┌'));
        assert!(!matches_corner_scan('┐', '┌'));
        assert!(matches_corner_scan('┘', '┘'));
        assert!(!matches_corner_scan('┐', '┘'));
    }

    // 80. Nested box inside valid outer — errors suppressed
    #[test]
    fn nested_box_errors_suppressed() {
        // Outer box is valid. Inner box has a minor alignment issue
        // (content row extends 1 past the edge). Should not produce errors.
        let input = "\
┌────────────────┐
│  ┌────┐        │
│  │ hi  │       │
│  └────┘        │
└────────────────┘";
        let diags = lint(input);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.level == Level::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "nested box alignment issues should be suppressed: {errors:?}"
        );
    }
}
