// Junction character auto-fixer.
//
// Analyzes 4-neighbor connectivity of box-drawing characters and upgrades them
// to the correct junction character when neighbors expect connections the
// current character doesn't provide. Only adds connections, never removes them.

use crate::Fixer;

pub struct JunctionCharacterFixer;

// ---------------------------------------------------------------------------
// Mutable grid helpers (duplicated per project pattern)
// ---------------------------------------------------------------------------

fn input_to_mut_grid(input: &str) -> Vec<Vec<char>> {
    input.lines().map(|l| l.chars().collect()).collect()
}

fn mut_grid_to_string(grid: &[Vec<char>], had_trailing_newline: bool) -> String {
    let mut s = grid
        .iter()
        .map(|row| row.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    if had_trailing_newline {
        s.push('\n');
    }
    s
}

fn grid_get(grid: &[Vec<char>], row: usize, col: usize) -> Option<char> {
    grid.get(row).and_then(|r| r.get(col)).copied()
}

// ---------------------------------------------------------------------------
// Direction connectivity
// ---------------------------------------------------------------------------

/// Which directions a box-drawing character connects in.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Connections {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
}

/// Return the directional connections for a single-line box-drawing char.
fn single_connections(ch: char) -> Option<Connections> {
    Some(match ch {
        '─' => Connections { left: true, right: true, ..Default::default() },
        '│' => Connections { up: true, down: true, ..Default::default() },
        '┌' => Connections { down: true, right: true, ..Default::default() },
        '┐' => Connections { down: true, left: true, ..Default::default() },
        '└' => Connections { up: true, right: true, ..Default::default() },
        '┘' => Connections { up: true, left: true, ..Default::default() },
        '┬' => Connections { down: true, left: true, right: true, ..Default::default() },
        '┴' => Connections { up: true, left: true, right: true, ..Default::default() },
        '├' => Connections { up: true, down: true, right: true, ..Default::default() },
        '┤' => Connections { up: true, down: true, left: true, ..Default::default() },
        '┼' => Connections { up: true, down: true, left: true, right: true },
        _ => return None,
    })
}

/// Return the directional connections for a double-line box-drawing char.
fn double_connections(ch: char) -> Option<Connections> {
    Some(match ch {
        '═' => Connections { left: true, right: true, ..Default::default() },
        '║' => Connections { up: true, down: true, ..Default::default() },
        '╔' => Connections { down: true, right: true, ..Default::default() },
        '╗' => Connections { down: true, left: true, ..Default::default() },
        '╚' => Connections { up: true, right: true, ..Default::default() },
        '╝' => Connections { up: true, left: true, ..Default::default() },
        '╦' => Connections { down: true, left: true, right: true, ..Default::default() },
        '╩' => Connections { up: true, left: true, right: true, ..Default::default() },
        '╠' => Connections { up: true, down: true, right: true, ..Default::default() },
        '╣' => Connections { up: true, down: true, left: true, ..Default::default() },
        '╬' => Connections { up: true, down: true, left: true, right: true },
        _ => return None,
    })
}

/// Get connections for any box-drawing character (single or double).
fn char_connections(ch: char) -> Option<Connections> {
    single_connections(ch).or_else(|| double_connections(ch))
}

/// Whether a character is a single-line box-drawing char.
fn is_single_box_char(ch: char) -> bool {
    single_connections(ch).is_some()
}

/// Whether a character is a double-line box-drawing char.
fn is_double_box_char(ch: char) -> bool {
    double_connections(ch).is_some()
}

/// Whether a character is an arrow tip (should not be modified).
fn is_arrow_tip(ch: char) -> bool {
    matches!(ch, '▲' | '▼' | '►' | '◄' | '△' | '▽' | '▷' | '◁')
}

// ---------------------------------------------------------------------------
// Character lookup tables
// ---------------------------------------------------------------------------

/// Look up the single-line character for a given set of connections.
fn single_char_for(c: Connections) -> Option<char> {
    Some(match (c.up, c.down, c.left, c.right) {
        (false, false, true, true) => '─',
        (true, true, false, false) => '│',
        (false, true, false, true) => '┌',
        (false, true, true, false) => '┐',
        (true, false, false, true) => '└',
        (true, false, true, false) => '┘',
        (false, true, true, true) => '┬',
        (true, false, true, true) => '┴',
        (true, true, false, true) => '├',
        (true, true, true, false) => '┤',
        (true, true, true, true) => '┼',
        _ => return None,
    })
}

/// Look up the double-line character for a given set of connections.
fn double_char_for(c: Connections) -> Option<char> {
    Some(match (c.up, c.down, c.left, c.right) {
        (false, false, true, true) => '═',
        (true, true, false, false) => '║',
        (false, true, false, true) => '╔',
        (false, true, true, false) => '╗',
        (true, false, false, true) => '╚',
        (true, false, true, false) => '╝',
        (false, true, true, true) => '╦',
        (true, false, true, true) => '╩',
        (true, true, false, true) => '╠',
        (true, true, true, false) => '╣',
        (true, true, true, true) => '╬',
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Neighbor analysis
// ---------------------------------------------------------------------------

/// Determine what connections neighbors expect at position (r, c).
fn expected_connections(grid: &[Vec<char>], r: usize, c: usize) -> Connections {
    let mut expected = Connections::default();

    // Check neighbor above: if it connects down, we need up
    if r > 0 {
        if let Some(ch) = grid_get(grid, r - 1, c) {
            if let Some(conn) = char_connections(ch) {
                if conn.down {
                    expected.up = true;
                }
            }
        }
    }

    // Check neighbor below: if it connects up, we need down
    if let Some(ch) = grid_get(grid, r + 1, c) {
        if let Some(conn) = char_connections(ch) {
            if conn.up {
                expected.down = true;
            }
        }
    }

    // Check neighbor left: if it connects right, we need left
    if c > 0 {
        if let Some(ch) = grid_get(grid, r, c - 1) {
            if let Some(conn) = char_connections(ch) {
                if conn.right {
                    expected.left = true;
                }
            }
        }
    }

    // Check neighbor right: if it connects left, we need right
    if let Some(ch) = grid_get(grid, r, c + 1) {
        if let Some(conn) = char_connections(ch) {
            if conn.left {
                expected.right = true;
            }
        }
    }

    expected
}

/// Determine if the position should use double-line style.
/// Uses the current char's style, or majority of neighbors if ambiguous.
fn should_use_double(grid: &[Vec<char>], r: usize, c: usize) -> bool {
    let ch = match grid_get(grid, r, c) {
        Some(ch) => ch,
        None => return false,
    };

    // If current char is already a known style, keep it
    if is_double_box_char(ch) {
        return true;
    }
    if is_single_box_char(ch) {
        return false;
    }

    // For non-box chars being upgraded, check neighbor majority
    let neighbors = [
        if r > 0 { grid_get(grid, r - 1, c) } else { None },
        grid_get(grid, r + 1, c),
        if c > 0 { grid_get(grid, r, c - 1) } else { None },
        grid_get(grid, r, c + 1),
    ];

    let double_count = neighbors.iter().filter(|n| n.is_some_and(is_double_box_char)).count();
    let single_count = neighbors.iter().filter(|n| n.is_some_and(is_single_box_char)).count();

    double_count > single_count
}

// ---------------------------------------------------------------------------
// Fixer implementation
// ---------------------------------------------------------------------------

impl Fixer for JunctionCharacterFixer {
    fn name(&self) -> &str {
        "junction-character"
    }

    fn fix(&self, input: &str) -> String {
        let had_trailing_newline = input.ends_with('\n');
        let mut grid = input_to_mut_grid(input);
        let rows = grid.len();
        let mut changed = true;

        // Iterate until stable (neighbor upgrades may cascade)
        while changed {
            changed = false;
            for r in 0..rows {
                let cols = grid[r].len();
                for c in 0..cols {
                    let ch = grid[r][c];

                    // Skip non-box-drawing characters and arrow tips
                    if char_connections(ch).is_none() || is_arrow_tip(ch) {
                        continue;
                    }

                    // Only upgrade corners and junctions, not straight edges.
                    // Straight lines (│, ─, ║, ═) are often standalone elements
                    // adjacent to unrelated box structures, so upgrading them
                    // would create spurious junctions.
                    if matches!(ch, '│' | '─' | '║' | '═') {
                        continue;
                    }

                    let current = char_connections(ch).unwrap();
                    let expected = expected_connections(&grid, r, c);

                    // Merge: add expected connections to current (never remove)
                    let merged = Connections {
                        up: current.up || expected.up,
                        down: current.down || expected.down,
                        left: current.left || expected.left,
                        right: current.right || expected.right,
                    };

                    if merged == current {
                        continue; // No change needed
                    }

                    // Look up the correct character
                    let use_double = should_use_double(&grid, r, c);
                    let new_ch = if use_double {
                        double_char_for(merged)
                    } else {
                        single_char_for(merged)
                    };

                    if let Some(new_ch) = new_ch {
                        if new_ch != ch {
                            grid[r][c] = new_ch;
                            changed = true;
                        }
                    }
                }
            }
        }

        mut_grid_to_string(&grid, had_trailing_newline)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn fix(input: &str) -> String {
        JunctionCharacterFixer.fix(input)
    }

    // 1. Straight edges are NOT upgraded (│ stays │ even next to ─)
    #[test]
    fn straight_edges_not_upgraded() {
        assert_eq!(fix("─│─"), "─│─");
        assert_eq!(fix("│\n─\n│"), "│\n─\n│");
        assert_eq!(fix("│─"), "│─");
        assert_eq!(fix("─│"), "─│");
        assert_eq!(fix("─\n│"), "─\n│");
        assert_eq!(fix("│\n─"), "│\n─");
    }

    // 2. Corners get upgraded when neighbors expect more connections
    #[test]
    fn corner_upgraded_to_tee() {
        // ┘ connects up+left. ─ to the right expects left, so ┘ gets right → ┴
        let input = "┘─";
        assert_eq!(fix(input), "┴─");
    }

    // 3. T-piece upgraded to cross when neighbors expect it
    #[test]
    fn tee_upgraded_to_cross() {
        // ┤ connects up+down+left. ─ to right expects left → ┤ gets right → ┼
        let input = "─┤─";
        assert_eq!(fix(input), "─┼─");
    }

    // 7. Already correct → no change
    #[test]
    fn already_correct_unchanged() {
        let input = "┌──┐\n│  │\n└──┘";
        assert_eq!(fix(input), input);
    }

    // 8. Arrow tips not modified
    #[test]
    fn arrow_tips_preserved() {
        let input = "──►\n──◄\n▼\n▲";
        assert_eq!(fix(input), input);
    }

    // 9. Text content not modified
    #[test]
    fn text_content_preserved() {
        let input = "hello world";
        assert_eq!(fix(input), input);
    }

    // 10. Empty input
    #[test]
    fn empty_input() {
        assert_eq!(fix(""), "");
    }

    // 10b. Input with trailing newline preserved
    #[test]
    fn trailing_newline_preserved() {
        let input = "hello\n";
        let result = fix(input);
        assert_eq!(result, input, "trailing newline should be preserved");
    }

    // 11. Double-line straight edges also NOT upgraded
    #[test]
    fn double_straight_edges_not_upgraded() {
        assert_eq!(fix("═║═"), "═║═");
        assert_eq!(fix("═\n║"), "═\n║");
        assert_eq!(fix("║\n═"), "║\n═");
        assert_eq!(fix("║═"), "║═");
        assert_eq!(fix("═║"), "═║");
    }

    // 12. Double-line corners get upgraded
    #[test]
    fn double_corner_upgraded() {
        // ╝ connects up+left. ═ to right expects left → ╝ gets right → ╩
        let input = "╝═";
        assert_eq!(fix(input), "╩═");
    }

    // 13. Double-line T-piece upgraded to cross
    #[test]
    fn double_tee_upgraded_to_cross() {
        let input = "═╣═";
        assert_eq!(fix(input), "═╬═");
    }

    // 16. Already correct double-line box
    #[test]
    fn double_box_unchanged() {
        let input = "╔══╗\n║  ║\n╚══╝";
        assert_eq!(fix(input), input);
    }

    // 17. ┌ with │ above and ─ left → ┼ (adding up+left connections)
    #[test]
    fn corner_upgrade_to_cross() {
        let input = " │\n─┌─\n │";
        assert_eq!(fix(input), " │\n─┼─\n │");
    }

    // 18. ┐ with ─ from right → ┐ gets right → ┬
    #[test]
    fn corner_gets_right() {
        let input = "┐─";
        assert_eq!(fix(input), "┬─");
    }

    // 19. Cascading fix: corner upgraded triggers further upgrades
    #[test]
    fn cascading_fix() {
        // ┘ with ─ to right → ┴ (first pass). Then stable.
        let input = "─┘─";
        assert_eq!(fix(input), "─┴─");
    }

    // 20. Boundary: single character
    #[test]
    fn single_char_boundary() {
        assert_eq!(fix("│"), "│");
        assert_eq!(fix("─"), "─");
    }

    // 21. Ragged lines — straight edges stay unchanged
    #[test]
    fn ragged_lines() {
        let input = "─│\n│";
        let result = fix(input);
        // Straight edges not upgraded, so no change
        assert_eq!(result, "─│\n│");
    }

    // 22. Demo diagram round-trip: no changes on already correct input
    #[test]
    fn demo_diagram_round_trip() {
        let input = include_str!("../examples/demo-flow-diagram.txt");
        let result = fix(input);
        // Find and report differences
        for (i, (orig, fixed)) in input.lines().zip(result.lines()).enumerate() {
            if orig != fixed {
                for (j, (oc, fc)) in orig.chars().zip(fixed.chars()).enumerate() {
                    if oc != fc {
                        eprintln!("Line {}, col {}: '{}' → '{}'", i + 1, j + 1, oc, fc);
                    }
                }
            }
        }
        assert_eq!(result, input, "junction fixer should not modify correct demo diagram");
    }

    // 23. ┘ with ─ below → should NOT add down (only adds expected connections)
    #[test]
    fn no_spurious_below_connection() {
        // ┘ connects up+left. Below is ─ which connects left+right, NOT up.
        // So ─ doesn't expect ┘ to connect down. No change.
        let input = "┘\n─";
        assert_eq!(fix(input), "┘\n─");
    }

    // 24. Single ┼ is stable
    #[test]
    fn cross_is_stable() {
        let input = " │ \n─┼─\n │ ";
        assert_eq!(fix(input), input);
    }

    // 25. Double ╬ is stable
    #[test]
    fn double_cross_is_stable() {
        let input = " ║ \n═╬═\n ║ ";
        assert_eq!(fix(input), input);
    }

    // 26. should_use_double: single char returns false immediately
    #[test]
    fn style_detection_single_char() {
        // ─ is a single-line char → returns false immediately
        assert!(!should_use_double(
            &vec![vec!['═', '─', '═']],
            0, 1
        ));
    }

    // 26b. should_use_double with non-box char uses neighbor majority
    #[test]
    fn style_detection_nonbox_majority() {
        // 'x' at (0,1) is not a box char. Neighbors: ═ left (double), ═ right (double)
        let grid = vec![vec!['═', 'x', '═']];
        assert!(should_use_double(&grid, 0, 1));
    }

    // 27. should_use_double for single char
    #[test]
    fn style_detection_single() {
        let grid = vec![vec!['│']];
        assert!(!should_use_double(&grid, 0, 0));
    }

    // 28. should_use_double for double char
    #[test]
    fn style_detection_double() {
        let grid = vec![vec!['║']];
        assert!(should_use_double(&grid, 0, 0));
    }

    // 29. expected_connections with no neighbors
    #[test]
    fn expected_connections_none() {
        let grid = vec![vec!['│']];
        let exp = expected_connections(&grid, 0, 0);
        assert!(!exp.up && !exp.down && !exp.left && !exp.right);
    }

    // 30. char_connections for non-box char
    #[test]
    fn char_connections_non_box() {
        assert!(char_connections('a').is_none());
        assert!(char_connections(' ').is_none());
    }

    // 31. single_char_for with no connections returns None
    #[test]
    fn single_char_for_no_connections() {
        assert!(single_char_for(Connections::default()).is_none());
    }

    // 32. double_char_for with no connections returns None
    #[test]
    fn double_char_for_no_connections() {
        assert!(double_char_for(Connections::default()).is_none());
    }

    // 33. should_use_double for non-box char with no neighbors
    #[test]
    fn should_use_double_out_of_bounds() {
        let grid: Vec<Vec<char>> = vec![];
        assert!(!should_use_double(&grid, 0, 0));
    }

    // 34. grid_get out of bounds
    #[test]
    fn grid_get_bounds() {
        let grid = vec![vec!['a']];
        assert_eq!(grid_get(&grid, 0, 0), Some('a'));
        assert_eq!(grid_get(&grid, 1, 0), None);
        assert_eq!(grid_get(&grid, 0, 1), None);
    }

    // 35. Connections with only one direction (single_char_for partial)
    #[test]
    fn single_char_for_partial() {
        // Only up → no valid single-line char
        assert!(single_char_for(Connections { up: true, ..Default::default() }).is_none());
        // Only down → no valid char
        assert!(single_char_for(Connections { down: true, ..Default::default() }).is_none());
    }

    // 36. Exercise all single_connections branches
    #[test]
    fn single_connections_all_chars() {
        let cases = [
            ('─', (false, false, true, true)),
            ('│', (true, true, false, false)),
            ('┌', (false, true, false, true)),
            ('┐', (false, true, true, false)),
            ('└', (true, false, false, true)),
            ('┘', (true, false, true, false)),
            ('┬', (false, true, true, true)),
            ('┴', (true, false, true, true)),
            ('├', (true, true, false, true)),
            ('┤', (true, true, true, false)),
            ('┼', (true, true, true, true)),
        ];
        for (ch, (u, d, l, r)) in cases {
            let c = single_connections(ch).unwrap();
            assert_eq!((c.up, c.down, c.left, c.right), (u, d, l, r), "single_connections('{ch}')");
        }
        assert!(single_connections('x').is_none());
    }

    // 37. Exercise all double_connections branches
    #[test]
    fn double_connections_all_chars() {
        let cases = [
            ('═', (false, false, true, true)),
            ('║', (true, true, false, false)),
            ('╔', (false, true, false, true)),
            ('╗', (false, true, true, false)),
            ('╚', (true, false, false, true)),
            ('╝', (true, false, true, false)),
            ('╦', (false, true, true, true)),
            ('╩', (true, false, true, true)),
            ('╠', (true, true, false, true)),
            ('╣', (true, true, true, false)),
            ('╬', (true, true, true, true)),
        ];
        for (ch, (u, d, l, r)) in cases {
            let c = double_connections(ch).unwrap();
            assert_eq!((c.up, c.down, c.left, c.right), (u, d, l, r), "double_connections('{ch}')");
        }
        assert!(double_connections('x').is_none());
    }

    // 38. Exercise all single_char_for branches
    #[test]
    fn single_char_for_all() {
        let cases = [
            ((false, false, true, true), '─'),
            ((true, true, false, false), '│'),
            ((false, true, false, true), '┌'),
            ((false, true, true, false), '┐'),
            ((true, false, false, true), '└'),
            ((true, false, true, false), '┘'),
            ((false, true, true, true), '┬'),
            ((true, false, true, true), '┴'),
            ((true, true, false, true), '├'),
            ((true, true, true, false), '┤'),
            ((true, true, true, true), '┼'),
        ];
        for ((u, d, l, r), expected) in cases {
            let c = Connections { up: u, down: d, left: l, right: r };
            assert_eq!(single_char_for(c), Some(expected), "single_char_for({u},{d},{l},{r})");
        }
    }

    // 39. Exercise all double_char_for branches
    #[test]
    fn double_char_for_all() {
        let cases = [
            ((false, false, true, true), '═'),
            ((true, true, false, false), '║'),
            ((false, true, false, true), '╔'),
            ((false, true, true, false), '╗'),
            ((true, false, false, true), '╚'),
            ((true, false, true, false), '╝'),
            ((false, true, true, true), '╦'),
            ((true, false, true, true), '╩'),
            ((true, true, false, true), '╠'),
            ((true, true, true, false), '╣'),
            ((true, true, true, true), '╬'),
        ];
        for ((u, d, l, r), expected) in cases {
            let c = Connections { up: u, down: d, left: l, right: r };
            assert_eq!(double_char_for(c), Some(expected), "double_char_for({u},{d},{l},{r})");
        }
    }

    // 40. ├ as neighbor is recognized (exercises single_connections for ├ via expected_connections)
    #[test]
    fn left_tee_neighbor_recognized() {
        // ├ connects up+down+right. A ┘ to its right: ├ connects right, so
        // expected_connections at ┘'s position should see left=true from ├
        let input = "├┘";
        // ┘ connects up+left. ├ to left connects right → expected left=true (already has it).
        // No upgrade needed.
        assert_eq!(fix(input), "├┘");
    }

    // 41. ╦ as neighbor (exercises double_connections for ╦)
    #[test]
    fn double_top_tee_neighbor() {
        // ╦ connects down+left+right. ╗ below: ╦ connects down, ╗ connects up → expected up=true
        let input = "╦\n╗";
        // ╗ connects down+left. ╦ above connects down → expected up=true for ╗ → ╗ gets up → ╣
        assert_eq!(fix(input), "╦\n╣");
    }

    // 42. ╠ as neighbor (exercises double_connections for ╠)
    #[test]
    fn double_left_tee_neighbor() {
        let input = "╠╗";
        // ╠ connects up+down+right. ╗ connects down+left.
        // ╠ to left of ╗: ╠ connects right → expected left=true for ╗ (already has it).
        // No upgrade for ╗. No upgrade for ╠ since ╗ connects left.
        assert_eq!(fix(input), "╠╗");
    }

    // 43. is_arrow_tip coverage
    #[test]
    fn all_arrow_tips_detected() {
        for ch in ['▲', '▼', '►', '◄', '△', '▽', '▷', '◁'] {
            assert!(is_arrow_tip(ch), "is_arrow_tip('{ch}') should be true");
        }
        assert!(!is_arrow_tip('─'));
        assert!(!is_arrow_tip('x'));
    }
}
