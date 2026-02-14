pub mod detect_arrows;
pub mod detect_boxes;
pub mod extract;
pub mod grid;

use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process;

// ---------------------------------------------------------------------------
// CLI argument types
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(
    name = "boxlint",
    about = "Lint and auto-fix Unicode box-drawing diagrams"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Lint a file or stdin and print diagnostics
    Lint {
        /// File or directory to lint (reads stdin if omitted)
        path: Option<String>,

        /// Output format
        #[arg(long, default_value = "text")]
        format: OutputFormat,

        /// Suppress warnings, show only errors
        #[arg(short, long)]
        quiet: bool,
    },
    /// Auto-fix a file or stdin
    Fix {
        /// File or directory to fix (reads stdin if omitted)
        path: Option<String>,

        /// Write fixes back to the file instead of stdout
        #[arg(short = 'i', long = "in-place")]
        in_place: bool,

        /// Suppress warnings, show only errors
        #[arg(short, long)]
        quiet: bool,

        /// Prefix to strip from each line before processing
        #[arg(long)]
        strip_prefix: Option<String>,
    },
}

#[derive(ValueEnum, Clone, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
}

// ---------------------------------------------------------------------------
// Diagnostic model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub file: String,
    pub line: usize,
    pub col: usize,
    pub level: Level,
    pub message: String,
    pub rule: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Error,
    Warning,
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Level::Error => write!(f, "error"),
            Level::Warning => write!(f, "warning"),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}: {} [{}]",
            self.file, self.line, self.col, self.level, self.message, self.rule
        )
    }
}

// ---------------------------------------------------------------------------
// Lint / Fix traits and registry
// ---------------------------------------------------------------------------

pub trait LintRule {
    fn check(&self, input: &str) -> Vec<Diagnostic>;
    fn name(&self) -> &str;
}

pub trait Fixer {
    fn fix(&self, input: &str) -> String;
    fn name(&self) -> &str;
}

pub struct RuleRegistry {
    pub lint_rules: Vec<Box<dyn LintRule>>,
    pub fixers: Vec<Box<dyn Fixer>>,
}

impl Default for RuleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleRegistry {
    pub fn new() -> Self {
        Self {
            lint_rules: Vec::new(),
            fixers: Vec::new(),
        }
    }

    pub fn run_lint(&self, input: &str) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        for rule in &self.lint_rules {
            diags.extend(rule.check(input));
        }
        diags
    }

    pub fn run_fix(&self, input: &str) -> String {
        let mut result = input.to_string();
        for fixer in &self.fixers {
            result = fixer.fix(&result);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Input helpers
// ---------------------------------------------------------------------------

#[cfg(not(tarpaulin_include))]
fn read_stdin() -> io::Result<String> {
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf)?;
    Ok(buf)
}

fn collect_files(path: &str) -> io::Result<Vec<String>> {
    let p = Path::new(path);
    if p.is_dir() {
        let mut files = Vec::new();
        for entry in fs::read_dir(p)? {
            let entry = entry?;
            let entry_path = entry.path();
            if entry_path.is_file() {
                if let Some(ext) = entry_path.extension() {
                    if ext == "txt" {
                        if let Some(s) = entry_path.to_str() {
                            files.push(s.to_string());
                        }
                    }
                }
            }
        }
        files.sort();
        Ok(files)
    } else {
        Ok(vec![path.to_string()])
    }
}

// ---------------------------------------------------------------------------
// Subcommand implementations
// ---------------------------------------------------------------------------

/// Load file inputs from a path, or read from stdin if `path` is None.
#[cfg(not(tarpaulin_include))]
fn load_inputs(path: Option<&str>) -> Result<Vec<(String, String)>, i32> {
    match path {
        Some(p) => load_file_inputs(p),
        None => match read_stdin() {
            Ok(content) => Ok(vec![("<stdin>".to_string(), content)]),
            Err(e) => {
                eprintln!("boxlint: stdin: {e}");
                Err(2)
            }
        },
    }
}

fn load_file_inputs(p: &str) -> Result<Vec<(String, String)>, i32> {
    let files = match collect_files(p) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("boxlint: {e}");
            return Err(2);
        }
    };
    let mut inputs = Vec::new();
    for file in files {
        match fs::read_to_string(&file) {
            Ok(content) => inputs.push((file, content)),
            Err(e) => {
                eprintln!("boxlint: {file}: {e}");
                return Err(2);
            }
        }
    }
    Ok(inputs)
}

fn run_lint(
    path: Option<&str>,
    format: &OutputFormat,
    quiet: bool,
    registry: &RuleRegistry,
) -> i32 {
    let inputs = match load_inputs(path) {
        Ok(inputs) => inputs,
        Err(code) => return code,
    };

    let mut all_diags = Vec::new();
    for (file, content) in &inputs {
        let regions = extract::extract_diagrams(content);
        if regions.is_empty() {
            // No diagrams detected; lint the whole input.
            let mut diags = registry.run_lint(content);
            for d in &mut diags {
                if d.file.is_empty() {
                    d.file = file.clone();
                }
            }
            all_diags.extend(diags);
        } else {
            for region in &regions {
                let mut diags = registry.run_lint(&region.content);
                for d in &mut diags {
                    if d.file.is_empty() {
                        d.file = file.clone();
                    }
                    // Adjust line numbers to account for the region's position
                    // in the original document.
                    d.line += region.start_line - 1;
                }
                all_diags.extend(diags);
            }
        }
    }

    if quiet {
        all_diags.retain(|d| d.level == Level::Error);
    }

    let stderr = io::stderr();
    let mut stderr = stderr.lock();
    for diag in &all_diags {
        match format {
            OutputFormat::Text => {
                let _ = writeln!(stderr, "{diag}");
            }
            OutputFormat::Json => {
                if let Ok(json) = serde_json::to_string(diag) {
                    let _ = writeln!(stderr, "{json}");
                }
            }
        }
    }

    let has_errors = all_diags.iter().any(|d| d.level == Level::Error);
    if has_errors {
        1
    } else {
        0
    }
}

fn run_fix(
    path: Option<&str>,
    in_place: bool,
    _quiet: bool,
    registry: &RuleRegistry,
    prefix: Option<&str>,
) -> i32 {
    if in_place && path.is_none() {
        eprintln!("boxlint: --in-place requires a file argument");
        return 2;
    }

    let inputs = match load_inputs(path) {
        Ok(inputs) => inputs,
        Err(code) => return code,
    };

    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    for (file, content) in &inputs {
        let output = if let Some(pfx) = prefix {
            // Explicit prefix: strip, fix, restore.
            let stripped = extract::strip_prefix(content, pfx);
            let fixed = registry.run_fix(&stripped);
            extract::restore_prefix(&fixed, pfx)
        } else {
            // Auto-detect embedded diagrams.
            let regions = extract::extract_diagrams(content);
            if regions.is_empty() || (regions.len() == 1 && regions[0].prefix.is_empty()) {
                // No embedded prefix detected; fix the whole input.
                registry.run_fix(content)
            } else {
                // Fix each region in place within the original document.
                let lines: Vec<&str> = content.lines().collect();
                let mut result_lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();

                for region in &regions {
                    let fixed_content = registry.run_fix(&region.content);
                    let fixed_lines: Vec<&str> = fixed_content.lines().collect();

                    // Replace lines in the result, restoring the prefix.
                    for (i, fixed_line) in fixed_lines.iter().enumerate() {
                        let doc_idx = region.start_line - 1 + i;
                        if doc_idx < result_lines.len() {
                            result_lines[doc_idx] = format!("{}{}", region.prefix, fixed_line);
                        }
                    }
                }

                result_lines.join("\n")
            }
        };
        if in_place {
            if let Err(e) = fs::write(file, &output) {
                eprintln!("boxlint: {file}: {e}");
                return 2;
            }
        } else {
            let _ = stdout.write_all(output.as_bytes());
        }
    }

    0
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[cfg(not(tarpaulin_include))]
fn main() {
    let cli = Cli::parse();
    let registry = RuleRegistry::new();

    let code = match &cli.command {
        Command::Lint {
            path,
            format,
            quiet,
        } => run_lint(path.as_deref(), format, *quiet, &registry),
        Command::Fix {
            path,
            in_place,
            quiet,
            strip_prefix,
        } => run_fix(
            path.as_deref(),
            *in_place,
            *quiet,
            &registry,
            strip_prefix.as_deref(),
        ),
    };

    process::exit(code);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lint_no_args() {
        let cli = Cli::try_parse_from(["boxlint", "lint"]).unwrap();
        match cli.command {
            Command::Lint {
                path,
                format,
                quiet,
            } => {
                assert!(path.is_none());
                assert_eq!(format, OutputFormat::Text);
                assert!(!quiet);
            }
            _ => panic!("expected Lint command"),
        }
    }

    #[test]
    fn parse_lint_with_file() {
        let cli = Cli::try_parse_from(["boxlint", "lint", "foo.txt"]).unwrap();
        match cli.command {
            Command::Lint { path, .. } => {
                assert_eq!(path.as_deref(), Some("foo.txt"));
            }
            _ => panic!("expected Lint command"),
        }
    }

    #[test]
    fn parse_lint_json_format() {
        let cli = Cli::try_parse_from(["boxlint", "lint", "--format", "json"]).unwrap();
        match cli.command {
            Command::Lint { format, .. } => {
                assert_eq!(format, OutputFormat::Json);
            }
            _ => panic!("expected Lint command"),
        }
    }

    #[test]
    fn parse_lint_quiet() {
        let cli = Cli::try_parse_from(["boxlint", "lint", "-q"]).unwrap();
        match cli.command {
            Command::Lint { quiet, .. } => {
                assert!(quiet);
            }
            _ => panic!("expected Lint command"),
        }
    }

    #[test]
    fn parse_fix_no_args() {
        let cli = Cli::try_parse_from(["boxlint", "fix"]).unwrap();
        match cli.command {
            Command::Fix {
                path,
                in_place,
                strip_prefix,
                ..
            } => {
                assert!(path.is_none());
                assert!(!in_place);
                assert!(strip_prefix.is_none());
            }
            _ => panic!("expected Fix command"),
        }
    }

    #[test]
    fn parse_fix_in_place() {
        let cli = Cli::try_parse_from(["boxlint", "fix", "foo.txt", "-i"]).unwrap();
        match cli.command {
            Command::Fix { path, in_place, .. } => {
                assert_eq!(path.as_deref(), Some("foo.txt"));
                assert!(in_place);
            }
            _ => panic!("expected Fix command"),
        }
    }

    #[test]
    fn parse_fix_in_place_long() {
        let cli = Cli::try_parse_from(["boxlint", "fix", "foo.txt", "--in-place"]).unwrap();
        match cli.command {
            Command::Fix { in_place, .. } => {
                assert!(in_place);
            }
            _ => panic!("expected Fix command"),
        }
    }

    #[test]
    fn parse_invalid_subcommand() {
        let result = Cli::try_parse_from(["boxlint", "unknown"]);
        assert!(result.is_err());
    }

    #[test]
    fn diagnostic_text_format() {
        let d = Diagnostic {
            file: "test.txt".to_string(),
            line: 10,
            col: 5,
            level: Level::Error,
            message: "broken corner".to_string(),
            rule: "corner-align".to_string(),
        };
        assert_eq!(
            d.to_string(),
            "test.txt:10:5: error: broken corner [corner-align]"
        );
    }

    #[test]
    fn diagnostic_warning_format() {
        let d = Diagnostic {
            file: "test.txt".to_string(),
            line: 3,
            col: 1,
            level: Level::Warning,
            message: "text overflow".to_string(),
            rule: "text-overflow".to_string(),
        };
        assert_eq!(
            d.to_string(),
            "test.txt:3:1: warning: text overflow [text-overflow]"
        );
    }

    #[test]
    fn diagnostic_json_format() {
        let d = Diagnostic {
            file: "test.txt".to_string(),
            line: 10,
            col: 5,
            level: Level::Error,
            message: "broken corner".to_string(),
            rule: "corner-align".to_string(),
        };
        let json = serde_json::to_string(&d).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["file"], "test.txt");
        assert_eq!(parsed["line"], 10);
        assert_eq!(parsed["col"], 5);
        assert_eq!(parsed["level"], "error");
        assert_eq!(parsed["message"], "broken corner");
        assert_eq!(parsed["rule"], "corner-align");
    }

    #[test]
    fn diagnostic_json_warning_level() {
        let d = Diagnostic {
            file: "a.txt".to_string(),
            line: 1,
            col: 1,
            level: Level::Warning,
            message: "msg".to_string(),
            rule: "r".to_string(),
        };
        let json = serde_json::to_string(&d).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["level"], "warning");
    }

    #[test]
    fn lint_no_rules_returns_zero() {
        let dir = std::env::temp_dir().join("boxlint_test_lint_zero");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("empty.txt");
        fs::write(&file, "").unwrap();

        let registry = RuleRegistry::new();
        assert!(registry.lint_rules.is_empty());
        let code = run_lint(
            Some(file.to_str().unwrap()),
            &OutputFormat::Text,
            false,
            &registry,
        );
        assert_eq!(code, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn lint_file_not_found_returns_two() {
        let registry = RuleRegistry::new();
        let code = run_lint(
            Some("/nonexistent/path/to/file.txt"),
            &OutputFormat::Text,
            false,
            &registry,
        );
        assert_eq!(code, 2);
    }

    #[test]
    fn fix_in_place_without_file_returns_two() {
        let registry = RuleRegistry::new();
        let code = run_fix(None, true, false, &registry, None);
        assert_eq!(code, 2);
    }

    #[test]
    fn fix_file_not_found_returns_two() {
        let registry = RuleRegistry::new();
        let code = run_fix(
            Some("/nonexistent/path/to/file.txt"),
            false,
            false,
            &registry,
            None,
        );
        assert_eq!(code, 2);
    }

    #[test]
    fn empty_registry_lint_returns_no_diagnostics() {
        let registry = RuleRegistry::new();
        let diags = registry.run_lint("some content");
        assert!(diags.is_empty());
    }

    #[test]
    fn empty_registry_fix_returns_input() {
        let registry = RuleRegistry::new();
        let result = registry.run_fix("hello world");
        assert_eq!(result, "hello world");
    }

    #[test]
    fn lint_real_file_no_rules() {
        let dir = std::env::temp_dir().join("boxlint_test_lint");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("sample.txt");
        fs::write(&file, "hello\n").unwrap();

        let registry = RuleRegistry::new();
        let code = run_lint(
            Some(file.to_str().unwrap()),
            &OutputFormat::Text,
            false,
            &registry,
        );
        assert_eq!(code, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fix_real_file_no_fixers() {
        let dir = std::env::temp_dir().join("boxlint_test_fix");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("sample.txt");
        fs::write(&file, "hello\n").unwrap();

        let registry = RuleRegistry::new();
        let code = run_fix(Some(file.to_str().unwrap()), false, false, &registry, None);
        assert_eq!(code, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fix_in_place_writes_back() {
        let dir = std::env::temp_dir().join("boxlint_test_fix_ip");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("sample.txt");
        fs::write(&file, "hello\n").unwrap();

        let registry = RuleRegistry::new();
        let code = run_fix(Some(file.to_str().unwrap()), true, false, &registry, None);
        assert_eq!(code, 0);
        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hello\n");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn collect_files_filters_txt() {
        let dir = std::env::temp_dir().join("boxlint_test_collect");
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("a.txt"), "").unwrap();
        fs::write(dir.join("b.rs"), "").unwrap();
        fs::write(dir.join("c.txt"), "").unwrap();

        let files = collect_files(dir.to_str().unwrap()).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f.ends_with(".txt")));

        let _ = fs::remove_dir_all(&dir);
    }

    struct TestRule;

    impl LintRule for TestRule {
        fn check(&self, _input: &str) -> Vec<Diagnostic> {
            vec![Diagnostic {
                file: String::new(),
                line: 1,
                col: 1,
                level: Level::Error,
                message: "test error".to_string(),
                rule: "test-rule".to_string(),
            }]
        }
        fn name(&self) -> &str {
            "test-rule"
        }
    }

    #[test]
    fn custom_rule_produces_diagnostics() {
        let mut registry = RuleRegistry::new();
        registry.lint_rules.push(Box::new(TestRule));
        let diags = registry.run_lint("anything");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "test-rule");
    }

    struct TestFixer;

    impl Fixer for TestFixer {
        fn fix(&self, input: &str) -> String {
            input.replace("bad", "good")
        }
        fn name(&self) -> &str {
            "test-fixer"
        }
    }

    #[test]
    fn custom_fixer_transforms_input() {
        let mut registry = RuleRegistry::new();
        registry.fixers.push(Box::new(TestFixer));
        let result = registry.run_fix("this is bad text");
        assert_eq!(result, "this is good text");
    }

    #[test]
    fn quiet_mode_filters_warnings() {
        struct WarnRule;
        impl LintRule for WarnRule {
            fn check(&self, _input: &str) -> Vec<Diagnostic> {
                vec![
                    Diagnostic {
                        file: String::new(),
                        line: 1,
                        col: 1,
                        level: Level::Warning,
                        message: "a warning".to_string(),
                        rule: "warn-rule".to_string(),
                    },
                    Diagnostic {
                        file: String::new(),
                        line: 2,
                        col: 1,
                        level: Level::Error,
                        message: "an error".to_string(),
                        rule: "err-rule".to_string(),
                    },
                ]
            }
            fn name(&self) -> &str {
                "warn-rule"
            }
        }

        let mut registry = RuleRegistry::new();
        registry.lint_rules.push(Box::new(WarnRule));

        let mut diags = registry.run_lint("content");
        diags.retain(|d| d.level == Level::Error);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].level, Level::Error);
    }

    // Coverage: RuleRegistry::default() (line 128-129)
    #[test]
    fn registry_default() {
        let reg = RuleRegistry::default();
        assert!(reg.lint_rules.is_empty());
        assert!(reg.fixers.is_empty());
    }

    // Helper rule that produces diagnostics for testing run_lint paths
    struct ErrorRule;
    impl LintRule for ErrorRule {
        fn check(&self, _input: &str) -> Vec<Diagnostic> {
            vec![Diagnostic {
                file: String::new(),
                line: 1,
                col: 1,
                level: Level::Error,
                message: "test error".to_string(),
                rule: "err".to_string(),
            }]
        }
        fn name(&self) -> &str {
            "err"
        }
    }

    struct WarnAndErrorRule;
    impl LintRule for WarnAndErrorRule {
        fn check(&self, _input: &str) -> Vec<Diagnostic> {
            vec![
                Diagnostic {
                    file: String::new(),
                    line: 1,
                    col: 1,
                    level: Level::Warning,
                    message: "warn".to_string(),
                    rule: "w".to_string(),
                },
                Diagnostic {
                    file: String::new(),
                    line: 2,
                    col: 1,
                    level: Level::Error,
                    message: "err".to_string(),
                    rule: "e".to_string(),
                },
            ]
        }
        fn name(&self) -> &str {
            "warn-and-err"
        }
    }

    // Coverage: run_lint with JSON format (lines 272-273)
    #[test]
    fn run_lint_json_output() {
        let dir = std::env::temp_dir().join("boxlint_test_lint_json");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("test.txt");
        fs::write(&file, "hello").unwrap();

        let mut registry = RuleRegistry::new();
        registry.lint_rules.push(Box::new(ErrorRule));
        let code = run_lint(
            Some(file.to_str().unwrap()),
            &OutputFormat::Json,
            false,
            &registry,
        );
        assert_eq!(code, 1);

        let _ = fs::remove_dir_all(&dir);
    }

    // Coverage: run_lint quiet mode filters warnings (lines 260-261)
    #[test]
    fn run_lint_quiet_filters_warnings() {
        let dir = std::env::temp_dir().join("boxlint_test_lint_quiet");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("test.txt");
        fs::write(&file, "hello").unwrap();

        let mut registry = RuleRegistry::new();
        registry.lint_rules.push(Box::new(WarnAndErrorRule));
        let code = run_lint(
            Some(file.to_str().unwrap()),
            &OutputFormat::Text,
            true,
            &registry,
        );
        assert_eq!(code, 1);

        let _ = fs::remove_dir_all(&dir);
    }

    // Coverage: run_lint returns 1 on error (line 281)
    #[test]
    fn run_lint_returns_one_on_error() {
        let dir = std::env::temp_dir().join("boxlint_test_lint_err");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("test.txt");
        fs::write(&file, "content").unwrap();

        let mut registry = RuleRegistry::new();
        registry.lint_rules.push(Box::new(ErrorRule));
        let code = run_lint(
            Some(file.to_str().unwrap()),
            &OutputFormat::Text,
            false,
            &registry,
        );
        assert_eq!(code, 1);

        let _ = fs::remove_dir_all(&dir);
    }

    // Coverage: run_lint with diagram regions (lines 244-256)
    #[test]
    fn run_lint_with_diagram_regions() {
        let dir = std::env::temp_dir().join("boxlint_test_lint_regions");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("test.txt");
        // File with comment-prefixed diagram triggers region extraction
        fs::write(&file, "// ┌──┐\n// │hi│\n// └──┘\n").unwrap();

        let mut registry = RuleRegistry::new();
        registry.lint_rules.push(Box::new(ErrorRule));
        let code = run_lint(
            Some(file.to_str().unwrap()),
            &OutputFormat::Text,
            false,
            &registry,
        );
        assert_eq!(code, 1);

        let _ = fs::remove_dir_all(&dir);
    }

    // Coverage: run_lint no regions → whole input (line 235-243)
    #[test]
    fn run_lint_no_regions_whole_input() {
        let dir = std::env::temp_dir().join("boxlint_test_lint_whole");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("test.txt");
        // Plain text with no box chars and no prefix → no regions, lints whole input
        fs::write(&file, "plain text\n").unwrap();

        let mut registry = RuleRegistry::new();
        registry.lint_rules.push(Box::new(ErrorRule));
        let code = run_lint(
            Some(file.to_str().unwrap()),
            &OutputFormat::Text,
            false,
            &registry,
        );
        assert_eq!(code, 1);

        let _ = fs::remove_dir_all(&dir);
    }

    // Coverage: run_fix with explicit prefix (lines 333-336)
    #[test]
    fn run_fix_with_explicit_prefix() {
        let dir = std::env::temp_dir().join("boxlint_test_fix_prefix");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("test.txt");
        fs::write(&file, "// ┌──┐\n// │hi│\n// └──┘").unwrap();

        let registry = RuleRegistry::new();
        let code = run_fix(
            Some(file.to_str().unwrap()),
            false,
            false,
            &registry,
            Some("// "),
        );
        assert_eq!(code, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    // Coverage: run_fix with embedded regions (lines 339-361)
    #[test]
    fn run_fix_with_embedded_regions() {
        let dir = std::env::temp_dir().join("boxlint_test_fix_embed");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("test.txt");
        // Comment-prefixed diagram with surrounding text → regions detected
        fs::write(&file, "code here\n// ┌──┐\n// │hi│\n// └──┘\nmore code\n").unwrap();

        let registry = RuleRegistry::new();
        let code = run_fix(Some(file.to_str().unwrap()), false, false, &registry, None);
        assert_eq!(code, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    // Coverage: run_fix in-place writes file with fixer (lines 364-367)
    #[test]
    fn run_fix_in_place_with_fixer() {
        let dir = std::env::temp_dir().join("boxlint_test_fix_ip_fixer");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("test.txt");
        fs::write(&file, "bad text").unwrap();

        let mut registry = RuleRegistry::new();
        registry.fixers.push(Box::new(TestFixer));
        let code = run_fix(Some(file.to_str().unwrap()), true, false, &registry, None);
        assert_eq!(code, 0);
        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "good text");

        let _ = fs::remove_dir_all(&dir);
    }

    // Coverage: collect_files with single file (line 188)
    #[test]
    fn collect_files_single_file() {
        let dir = std::env::temp_dir().join("boxlint_test_collect_single");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("single.txt");
        fs::write(&file, "").unwrap();

        let files = collect_files(file.to_str().unwrap()).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0], file.to_str().unwrap());

        let _ = fs::remove_dir_all(&dir);
    }

    // Coverage: run_lint directory (lines 204-220, collect_files directory path)
    #[test]
    fn run_lint_directory() {
        let dir = std::env::temp_dir().join("boxlint_test_lint_dir");
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("a.txt"), "hello").unwrap();
        fs::write(dir.join("b.txt"), "world").unwrap();

        let registry = RuleRegistry::new();
        let code = run_lint(
            Some(dir.to_str().unwrap()),
            &OutputFormat::Text,
            false,
            &registry,
        );
        assert_eq!(code, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    // Coverage: run_fix directory
    #[test]
    fn run_fix_directory() {
        let dir = std::env::temp_dir().join("boxlint_test_fix_dir");
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("a.txt"), "hello").unwrap();

        let registry = RuleRegistry::new();
        let code = run_fix(Some(dir.to_str().unwrap()), false, false, &registry, None);
        assert_eq!(code, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    // Coverage: load_file_inputs collect_files error
    #[test]
    fn load_file_inputs_collect_error() {
        let dir = std::env::temp_dir().join("boxlint_test_load_unreadable");
        let _ = fs::create_dir_all(&dir);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o000));
        }
        let result = load_file_inputs(dir.to_str().unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o755));
        }
        let _ = fs::remove_dir_all(&dir);
        #[cfg(unix)]
        assert_eq!(result, Err(2));
    }

    // Coverage: load_file_inputs read_to_string error
    #[test]
    fn load_file_inputs_read_error() {
        let result = load_file_inputs("/nonexistent/path/to/file.txt");
        assert_eq!(result, Err(2));
    }

    // Coverage: fs::write error in run_fix in-place (lines 367-368)
    #[test]
    fn run_fix_in_place_write_error() {
        let dir = std::env::temp_dir().join("boxlint_test_fix_write_err");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("readonly.txt");
        fs::write(&file, "hello").unwrap();
        // Make file read-only
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&file, fs::Permissions::from_mode(0o444));
        }
        let registry = RuleRegistry::new();
        let code = run_fix(Some(file.to_str().unwrap()), true, false, &registry, None);
        // Restore permissions for cleanup
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&file, fs::Permissions::from_mode(0o644));
        }
        let _ = fs::remove_dir_all(&dir);
        #[cfg(unix)]
        assert_eq!(code, 2);
    }

    // Coverage: run_fix in-place with embedded regions
    #[test]
    fn run_fix_in_place_with_embedded_regions() {
        let dir = std::env::temp_dir().join("boxlint_test_fix_ip_embed");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("test.txt");
        fs::write(&file, "code\n// ┌──┐\n// │hi│\n// └──┘\nmore\n").unwrap();

        let registry = RuleRegistry::new();
        let code = run_fix(Some(file.to_str().unwrap()), true, false, &registry, None);
        assert_eq!(code, 0);

        let _ = fs::remove_dir_all(&dir);
    }
}
