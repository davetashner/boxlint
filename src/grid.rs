/// 2D character grid and diagram intermediate representation.
///
/// The [`Grid`] struct parses raw UTF-8 text into a 2D array of characters,
/// preserving original line:col positions for diagnostics. Character
/// classification helpers identify box-drawing elements. The [`DiagramIR`]
/// struct holds the grid plus detected nodes (boxes, arrows, text) that
/// downstream passes will populate.

// ---------------------------------------------------------------------------
// Grid
// ---------------------------------------------------------------------------

/// A 2D character grid built from raw UTF-8 text.
#[derive(Debug, Clone)]
pub struct Grid {
    cells: Vec<Vec<char>>,
    num_rows: usize,
    num_cols: usize,
}

impl Grid {
    /// Parse raw text into a 2D grid. Ragged lines are padded with spaces so
    /// every row has the same width.
    pub fn new(input: &str) -> Self {
        let lines: Vec<Vec<char>> = input.lines().map(|l| l.chars().collect()).collect();
        let num_rows = lines.len();
        let num_cols = lines.iter().map(|l| l.len()).max().unwrap_or(0);

        let cells: Vec<Vec<char>> = lines
            .into_iter()
            .map(|mut row| {
                row.resize(num_cols, ' ');
                row
            })
            .collect();

        Self {
            cells,
            num_rows,
            num_cols,
        }
    }

    /// Character at the given (row, col) position, or `None` if out of bounds.
    pub fn get(&self, row: usize, col: usize) -> Option<char> {
        self.cells.get(row).and_then(|r| r.get(col)).copied()
    }

    /// Number of rows in the grid.
    pub fn rows(&self) -> usize {
        self.num_rows
    }

    /// Number of columns in the grid (width of the widest line).
    pub fn cols(&self) -> usize {
        self.num_cols
    }
}

// ---------------------------------------------------------------------------
// Character classification helpers
// ---------------------------------------------------------------------------

/// Returns `true` for box-corner characters: `┌┐└┘╔╗╚╝`
pub fn is_box_corner(ch: char) -> bool {
    matches!(ch, '┌' | '┐' | '└' | '┘' | '╔' | '╗' | '╚' | '╝')
}

/// Returns `true` for horizontal edge characters: `─═`
pub fn is_horizontal_edge(ch: char) -> bool {
    matches!(ch, '─' | '═')
}

/// Returns `true` for vertical edge characters: `│║`
pub fn is_vertical_edge(ch: char) -> bool {
    matches!(ch, '│' | '║')
}

/// Returns `true` for arrow-tip characters: `►▼◄▲><v^`
pub fn is_arrow_tip(ch: char) -> bool {
    matches!(ch, '►' | '▼' | '◄' | '▲' | '>' | '<' | 'v' | '^')
}

/// Returns `true` for junction characters: `├┤┬┴┼╠╣╦╩╬`
pub fn is_junction(ch: char) -> bool {
    matches!(ch, '├' | '┤' | '┬' | '┴' | '┼' | '╠' | '╣' | '╦' | '╩' | '╬')
}

/// Returns `true` for any box-drawing or line character (corners, edges,
/// junctions).
pub fn is_line_drawing(ch: char) -> bool {
    is_box_corner(ch) || is_horizontal_edge(ch) || is_vertical_edge(ch) || is_junction(ch)
}

// ---------------------------------------------------------------------------
// Diagram IR types
// ---------------------------------------------------------------------------

/// A position in the grid (0-indexed row and column).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub row: usize,
    pub col: usize,
}

/// An axis-aligned bounding rectangle defined by two corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundingRect {
    pub top_left: Position,
    pub bottom_right: Position,
}

/// Cardinal direction for a segment of a line or arrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// A straight segment between two grid positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub start: Position,
    pub end: Position,
    pub direction: Direction,
}

/// A detected node in the diagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// A rectangular box with content lines.
    Box {
        bounds: BoundingRect,
        content: Vec<String>,
    },
    /// A multi-segment arrow with an optional label.
    Arrow {
        segments: Vec<Segment>,
        label: Option<String>,
    },
    /// Free-standing text.
    Text {
        position: Position,
        content: String,
    },
}

/// The intermediate representation of a diagram. Holds the parsed [`Grid`]
/// and a list of detected [`Node`]s. Detection passes (box, arrow, text)
/// populate the `nodes` vector; the grid is always available for raw access.
#[derive(Debug, Clone)]
pub struct DiagramIR {
    pub grid: Grid,
    pub nodes: Vec<Node>,
}

impl DiagramIR {
    /// Create a new `DiagramIR` from raw text. The nodes list starts empty.
    pub fn new(input: &str) -> Self {
        Self {
            grid: Grid::new(input),
            nodes: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Grid basics -------------------------------------------------------

    #[test]
    fn empty_input() {
        let g = Grid::new("");
        assert_eq!(g.rows(), 0);
        assert_eq!(g.cols(), 0);
        assert_eq!(g.get(0, 0), None);
    }

    #[test]
    fn single_line() {
        let g = Grid::new("hello");
        assert_eq!(g.rows(), 1);
        assert_eq!(g.cols(), 5);
        assert_eq!(g.get(0, 0), Some('h'));
        assert_eq!(g.get(0, 4), Some('o'));
        assert_eq!(g.get(0, 5), None);
        assert_eq!(g.get(1, 0), None);
    }

    #[test]
    fn ragged_lines_padded() {
        let g = Grid::new("ab\ncde\nf");
        assert_eq!(g.rows(), 3);
        assert_eq!(g.cols(), 3);
        // first row padded
        assert_eq!(g.get(0, 2), Some(' '));
        // third row padded
        assert_eq!(g.get(2, 1), Some(' '));
        assert_eq!(g.get(2, 2), Some(' '));
    }

    #[test]
    fn unicode_box_chars_preserved() {
        let input = "┌─┐\n│x│\n└─┘";
        let g = Grid::new(input);
        assert_eq!(g.rows(), 3);
        assert_eq!(g.cols(), 3);
        assert_eq!(g.get(0, 0), Some('┌'));
        assert_eq!(g.get(0, 1), Some('─'));
        assert_eq!(g.get(0, 2), Some('┐'));
        assert_eq!(g.get(1, 0), Some('│'));
        assert_eq!(g.get(1, 1), Some('x'));
        assert_eq!(g.get(1, 2), Some('│'));
        assert_eq!(g.get(2, 0), Some('└'));
        assert_eq!(g.get(2, 1), Some('─'));
        assert_eq!(g.get(2, 2), Some('┘'));
    }

    #[test]
    fn round_trip_preserves_chars() {
        let input = "┌──┐\n│hi│\n└──┘";
        let g = Grid::new(input);
        let mut rebuilt = String::new();
        for row in 0..g.rows() {
            if row > 0 {
                rebuilt.push('\n');
            }
            for col in 0..g.cols() {
                rebuilt.push(g.get(row, col).unwrap());
            }
        }
        assert_eq!(rebuilt, input);
    }

    // -- Character classification ------------------------------------------

    #[test]
    fn classify_corners() {
        for ch in ['┌', '┐', '└', '┘', '╔', '╗', '╚', '╝'] {
            assert!(is_box_corner(ch), "expected corner: {ch}");
            assert!(is_line_drawing(ch), "expected line drawing: {ch}");
        }
        assert!(!is_box_corner('─'));
        assert!(!is_box_corner('x'));
    }

    #[test]
    fn classify_edges() {
        assert!(is_horizontal_edge('─'));
        assert!(is_horizontal_edge('═'));
        assert!(!is_horizontal_edge('│'));
        assert!(is_vertical_edge('│'));
        assert!(is_vertical_edge('║'));
        assert!(!is_vertical_edge('─'));
    }

    #[test]
    fn classify_arrows() {
        for ch in ['►', '▼', '◄', '▲', '>', '<', 'v', '^'] {
            assert!(is_arrow_tip(ch), "expected arrow tip: {ch}");
        }
        assert!(!is_arrow_tip('─'));
        assert!(!is_arrow_tip('x'));
    }

    #[test]
    fn classify_junctions() {
        for ch in ['├', '┤', '┬', '┴', '┼', '╠', '╣', '╦', '╩', '╬'] {
            assert!(is_junction(ch), "expected junction: {ch}");
            assert!(is_line_drawing(ch), "expected line drawing: {ch}");
        }
        assert!(!is_junction('┌'));
    }

    #[test]
    fn classify_line_drawing() {
        // line drawing = corners + edges + junctions
        assert!(is_line_drawing('┌'));
        assert!(is_line_drawing('─'));
        assert!(is_line_drawing('│'));
        assert!(is_line_drawing('┼'));
        assert!(!is_line_drawing(' '));
        assert!(!is_line_drawing('A'));
        assert!(!is_line_drawing('►'));
    }

    // -- Demo diagram tests ------------------------------------------------

    #[test]
    fn demo_diagram_dimensions() {
        let input = include_str!("../examples/demo-flow-diagram.txt");
        let g = Grid::new(input);
        // The file has 135 lines (including a leading blank line)
        assert!(g.rows() > 100, "expected >100 rows, got {}", g.rows());
        assert!(g.cols() > 80, "expected >80 cols, got {}", g.cols());
    }

    #[test]
    fn demo_diagram_known_positions() {
        let input = include_str!("../examples/demo-flow-diagram.txt");
        let g = Grid::new(input);

        // Line 2 (index 1) starts with ┌ (the title box top-left corner)
        assert_eq!(g.get(1, 0), Some('┌'));
        // Line 4 (index 3) starts with └
        assert_eq!(g.get(3, 0), Some('└'));
    }

    // -- DiagramIR ---------------------------------------------------------

    #[test]
    fn diagram_ir_starts_empty() {
        let ir = DiagramIR::new("┌─┐\n│x│\n└─┘");
        assert!(ir.nodes.is_empty());
        assert_eq!(ir.grid.rows(), 3);
    }
}
