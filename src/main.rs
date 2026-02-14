pub mod detect_arrows;
pub mod detect_boxes;
pub mod detect_labels;
pub mod extract;
pub mod fix_adjacent_boxes;
pub mod fix_arrow_connect;
pub mod fix_box_content;
pub mod fix_box_corners;
pub mod grid;
pub mod lint_adjacent_boxes;
pub mod lint_arrow_connect;
pub mod lint_box_content;
pub mod lint_box_corners;
#[cfg(feature = "mcp")]
pub mod mcp;

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

        /// Filename to use in diagnostics when reading from stdin
        #[arg(long)]
        stdin_filename: Option<String>,

        /// Only run these rules (comma-separated or repeated)
        #[arg(long, value_delimiter = ',')]
        rule: Vec<String>,

        /// Skip these rules (comma-separated or repeated)
        #[arg(long, value_delimiter = ',')]
        ignore: Vec<String>,

        /// Also auto-fix issues after linting
        #[arg(long)]
        fix: bool,

        /// Write fixes back to the file instead of stdout (requires --fix)
        #[arg(short = 'i', long = "in-place")]
        in_place: bool,
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

        /// Output format
        #[arg(long, default_value = "text")]
        format: OutputFormat,

        /// Show unified diff instead of fixed output
        #[arg(long)]
        diff: bool,

        /// Filename to use in diagnostics when reading from stdin
        #[arg(long)]
        stdin_filename: Option<String>,

        /// Only run these rules (comma-separated or repeated)
        #[arg(long, value_delimiter = ',')]
        rule: Vec<String>,

        /// Skip these rules (comma-separated or repeated)
        #[arg(long, value_delimiter = ',')]
        ignore: Vec<String>,
    },
    /// Check a file or stdin silently (exit 0 = clean, exit 1 = issues)
    Check {
        /// File or directory to check (reads stdin if omitted)
        path: Option<String>,

        /// Filename to use in diagnostics when reading from stdin
        #[arg(long)]
        stdin_filename: Option<String>,

        /// Only run these rules (comma-separated or repeated)
        #[arg(long, value_delimiter = ',')]
        rule: Vec<String>,

        /// Skip these rules (comma-separated or repeated)
        #[arg(long, value_delimiter = ',')]
        ignore: Vec<String>,
    },
    /// Start MCP (Model Context Protocol) server over stdio
    #[cfg(feature = "mcp")]
    Mcp,
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
        let mut registry = Self {
            lint_rules: Vec::new(),
            fixers: Vec::new(),
        };
        registry
            .lint_rules
            .push(Box::new(crate::lint_box_corners::BoxCornerEdgeLint));
        registry.lint_rules.push(Box::new(
            crate::lint_adjacent_boxes::AdjacentBoxAlignmentLint,
        ));
        registry
            .lint_rules
            .push(Box::new(crate::lint_box_content::BoxContentAlignmentLint));
        registry
            .lint_rules
            .push(Box::new(crate::lint_arrow_connect::ArrowConnectLint));
        registry
            .fixers
            .push(Box::new(crate::fix_box_corners::BoxCornerEdgeFixer));
        registry.fixers.push(Box::new(
            crate::fix_adjacent_boxes::AdjacentBoxAlignmentFixer,
        ));
        registry
            .fixers
            .push(Box::new(crate::fix_box_content::BoxContentFixer));
        registry
            .fixers
            .push(Box::new(crate::fix_arrow_connect::ArrowConnectFixer));
        registry
    }

    /// Collect all known rule and fixer names.
    fn all_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.lint_rules.iter().map(|r| r.name()).collect();
        for f in &self.fixers {
            let n = f.name();
            if !names.contains(&n) {
                names.push(n);
            }
        }
        names
    }

    /// Filter rules and fixers by `--rule` (include-only) or `--ignore` (exclude).
    /// Returns exit code 2 on conflicting flags, or 0 on success.
    /// Prints a warning to stderr for any unknown rule names.
    pub fn filter(&mut self, rule: &[String], ignore: &[String]) -> i32 {
        if !rule.is_empty() && !ignore.is_empty() {
            eprintln!("boxlint: --rule and --ignore are mutually exclusive");
            return 2;
        }
        let known = self.all_names();
        // Validate names and warn about unknowns.
        for name in rule.iter().chain(ignore.iter()) {
            if !known.contains(&name.as_str()) {
                eprintln!("boxlint: unknown rule '{name}'");
            }
        }
        if !rule.is_empty() {
            self.lint_rules
                .retain(|r| rule.iter().any(|n| n == r.name()));
            self.fixers.retain(|f| rule.iter().any(|n| n == f.name()));
        }
        if !ignore.is_empty() {
            self.lint_rules
                .retain(|r| !ignore.iter().any(|n| n == r.name()));
            self.fixers
                .retain(|f| !ignore.iter().any(|n| n == f.name()));
        }
        0
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
fn load_inputs(
    path: Option<&str>,
    stdin_filename: Option<&str>,
) -> Result<Vec<(String, String)>, i32> {
    if path.is_some() && stdin_filename.is_some() {
        eprintln!("boxlint: --stdin-filename cannot be used with a file argument");
        return Err(2);
    }
    match path {
        Some(p) => load_file_inputs(p),
        None => {
            let name = stdin_filename.unwrap_or("<stdin>").to_string();
            match read_stdin() {
                Ok(content) => Ok(vec![(name, content)]),
                Err(e) => {
                    eprintln!("boxlint: stdin: {e}");
                    Err(2)
                }
            }
        }
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
    stdin_filename: Option<&str>,
    fix: bool,
    in_place: bool,
) -> i32 {
    if in_place && !fix {
        eprintln!("boxlint: --in-place requires --fix");
        return 2;
    }
    if in_place && path.is_none() {
        eprintln!("boxlint: --in-place requires a file argument");
        return 2;
    }

    let inputs = match load_inputs(path, stdin_filename) {
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

    if fix {
        let stdout = io::stdout();
        let mut stdout = stdout.lock();
        for (file, content) in &inputs {
            let regions = extract::extract_diagrams(content);
            let output = if regions.is_empty()
                || (regions.len() == 1 && regions[0].prefix.is_empty())
            {
                registry.run_fix(content)
            } else {
                let lines: Vec<&str> = content.lines().collect();
                let mut result_lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
                for region in &regions {
                    let fixed_content = registry.run_fix(&region.content);
                    let fixed_lines: Vec<&str> = fixed_content.lines().collect();
                    for (i, fixed_line) in fixed_lines.iter().enumerate() {
                        let doc_idx = region.start_line - 1 + i;
                        if doc_idx < result_lines.len() {
                            result_lines[doc_idx] = format!("{}{}", region.prefix, fixed_line);
                        }
                    }
                }
                result_lines.join("\n")
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
    }

    if has_errors {
        1
    } else {
        0
    }
}

fn run_check(path: Option<&str>, registry: &RuleRegistry, stdin_filename: Option<&str>) -> i32 {
    let inputs = match load_inputs(path, stdin_filename) {
        Ok(inputs) => inputs,
        Err(code) => return code,
    };

    for (_, content) in &inputs {
        let regions = extract::extract_diagrams(content);
        if regions.is_empty() {
            let diags = registry.run_lint(content);
            if diags.iter().any(|d| d.level == Level::Error) {
                return 1;
            }
        } else {
            for region in &regions {
                let diags = registry.run_lint(&region.content);
                if diags.iter().any(|d| d.level == Level::Error) {
                    return 1;
                }
            }
        }
    }

    0
}

#[allow(clippy::too_many_arguments)]
fn run_fix(
    path: Option<&str>,
    in_place: bool,
    _quiet: bool,
    registry: &RuleRegistry,
    prefix: Option<&str>,
    format: &OutputFormat,
    diff: bool,
    stdin_filename: Option<&str>,
) -> i32 {
    if in_place && path.is_none() {
        eprintln!("boxlint: --in-place requires a file argument");
        return 2;
    }
    if diff && in_place {
        eprintln!("boxlint: --diff and --in-place are mutually exclusive");
        return 2;
    }

    let inputs = match load_inputs(path, stdin_filename) {
        Ok(inputs) => inputs,
        Err(code) => return code,
    };

    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let mut any_changed = false;
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

        let changed = output != *content;
        if changed {
            any_changed = true;
        }

        if diff {
            if changed {
                let diff_text = unified_diff(content, &output, file);
                let _ = stdout.write_all(diff_text.as_bytes());
            }
        } else if *format == OutputFormat::Json {
            let json_obj = serde_json::json!({
                "file": file,
                "changed": changed,
                "fixed_text": output,
                "original_text": content,
            });
            let _ = writeln!(stdout, "{}", json_obj);
        } else if in_place {
            if let Err(e) = fs::write(file, &output) {
                eprintln!("boxlint: {file}: {e}");
                return 2;
            }
        } else {
            let _ = stdout.write_all(output.as_bytes());
        }
    }

    if diff && any_changed {
        1
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// Diff helper
// ---------------------------------------------------------------------------

fn unified_diff(original: &str, modified: &str, filename: &str) -> String {
    let orig_lines: Vec<&str> = original.lines().collect();
    let mod_lines: Vec<&str> = modified.lines().collect();
    let mut result = String::new();
    result.push_str(&format!("--- {filename}\n"));
    result.push_str(&format!("+++ {filename}\n"));

    // Simple line-by-line diff: find contiguous changed regions (hunks).
    let max_len = std::cmp::max(orig_lines.len(), mod_lines.len());
    let mut i = 0;
    while i < max_len {
        // Skip matching lines.
        if i < orig_lines.len() && i < mod_lines.len() && orig_lines[i] == mod_lines[i] {
            i += 1;
            continue;
        }

        // Found a difference — build a hunk.
        let hunk_start = i.saturating_sub(3);

        // Extend through differing lines. Walk both sides in lockstep,
        // collecting until 3 consecutive matching lines or end of input.
        let (orig_end, mod_end) = {
            let mut consecutive_match = 0;
            let mut oi = i;
            let mut mi = i;
            while oi < orig_lines.len() || mi < mod_lines.len() {
                if oi < orig_lines.len() && mi < mod_lines.len() && orig_lines[oi] == mod_lines[mi]
                {
                    consecutive_match += 1;
                    oi += 1;
                    mi += 1;
                    if consecutive_match >= 3 {
                        break;
                    }
                } else {
                    consecutive_match = 0;
                    if oi < orig_lines.len() {
                        oi += 1;
                    }
                    if mi < mod_lines.len() {
                        mi += 1;
                    }
                }
            }
            (oi, mi)
        };

        // Add context after the hunk (up to 3 lines).
        let orig_ctx_end = std::cmp::min(orig_end + 3, orig_lines.len());
        let mod_ctx_end = std::cmp::min(mod_end + 3, mod_lines.len());

        let orig_count = orig_ctx_end - hunk_start;
        let mod_count = mod_ctx_end - hunk_start;
        result.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            hunk_start + 1,
            orig_count,
            hunk_start + 1,
            mod_count
        ));

        // Context before.
        for line in &orig_lines[hunk_start..i] {
            result.push_str(&format!(" {line}\n"));
        }

        // Changed region: show removed then added.
        for line in &orig_lines[i..std::cmp::min(orig_end, orig_lines.len())] {
            result.push_str(&format!("-{line}\n"));
        }
        for line in &mod_lines[i..std::cmp::min(mod_end, mod_lines.len())] {
            result.push_str(&format!("+{line}\n"));
        }

        // Context after.
        let ctx_after_start = std::cmp::min(orig_end, orig_lines.len());
        for line in &orig_lines[ctx_after_start..orig_ctx_end] {
            result.push_str(&format!(" {line}\n"));
        }

        i = std::cmp::max(orig_ctx_end, mod_ctx_end);
    }

    result
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[cfg(not(tarpaulin_include))]
fn main() {
    let cli = Cli::parse();
    let mut registry = RuleRegistry::new();

    let code = match &cli.command {
        Command::Lint {
            path,
            format,
            quiet,
            stdin_filename,
            rule,
            ignore,
            fix,
            in_place,
        } => {
            let rc = registry.filter(rule, ignore);
            if rc != 0 {
                rc
            } else {
                run_lint(
                    path.as_deref(),
                    format,
                    *quiet,
                    &registry,
                    stdin_filename.as_deref(),
                    *fix,
                    *in_place,
                )
            }
        }
        Command::Fix {
            path,
            in_place,
            quiet,
            strip_prefix,
            format,
            diff,
            stdin_filename,
            rule,
            ignore,
        } => {
            let rc = registry.filter(rule, ignore);
            if rc != 0 {
                rc
            } else {
                run_fix(
                    path.as_deref(),
                    *in_place,
                    *quiet,
                    &registry,
                    strip_prefix.as_deref(),
                    format,
                    *diff,
                    stdin_filename.as_deref(),
                )
            }
        }
        Command::Check {
            path,
            stdin_filename,
            rule,
            ignore,
        } => {
            let rc = registry.filter(rule, ignore);
            if rc != 0 {
                rc
            } else {
                run_check(path.as_deref(), &registry, stdin_filename.as_deref())
            }
        }
        #[cfg(feature = "mcp")]
        Command::Mcp => {
            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
            if let Err(e) = rt.block_on(mcp::run_mcp_server()) {
                eprintln!("boxlint: MCP server error: {e}");
                2
            } else {
                0
            }
        }
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
                stdin_filename,
                rule,
                ignore,
                fix,
                in_place,
            } => {
                assert!(path.is_none());
                assert_eq!(format, OutputFormat::Text);
                assert!(!quiet);
                assert!(stdin_filename.is_none());
                assert!(rule.is_empty());
                assert!(ignore.is_empty());
                assert!(!fix);
                assert!(!in_place);
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
        assert!(!registry.lint_rules.is_empty());
        let code = run_lint(
            Some(file.to_str().unwrap()),
            &OutputFormat::Text,
            false,
            &registry,
            None,
            false,
            false,
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
            None,
            false,
            false,
        );
        assert_eq!(code, 2);
    }

    #[test]
    fn fix_in_place_without_file_returns_two() {
        let registry = RuleRegistry::new();
        let code = run_fix(
            None,
            true,
            false,
            &registry,
            None,
            &OutputFormat::Text,
            false,
            None,
        );
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
            &OutputFormat::Text,
            false,
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
            None,
            false,
            false,
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
        let code = run_fix(
            Some(file.to_str().unwrap()),
            false,
            false,
            &registry,
            None,
            &OutputFormat::Text,
            false,
            None,
        );
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
        let code = run_fix(
            Some(file.to_str().unwrap()),
            true,
            false,
            &registry,
            None,
            &OutputFormat::Text,
            false,
            None,
        );
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
        assert!(!reg.lint_rules.is_empty());
        assert!(!reg.fixers.is_empty());
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
            None,
            false,
            false,
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
            None,
            false,
            false,
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
            None,
            false,
            false,
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
            None,
            false,
            false,
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
            None,
            false,
            false,
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
            &OutputFormat::Text,
            false,
            None,
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
        let code = run_fix(
            Some(file.to_str().unwrap()),
            false,
            false,
            &registry,
            None,
            &OutputFormat::Text,
            false,
            None,
        );
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
        let code = run_fix(
            Some(file.to_str().unwrap()),
            true,
            false,
            &registry,
            None,
            &OutputFormat::Text,
            false,
            None,
        );
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
            None,
            false,
            false,
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
        let code = run_fix(
            Some(dir.to_str().unwrap()),
            false,
            false,
            &registry,
            None,
            &OutputFormat::Text,
            false,
            None,
        );
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
        let code = run_fix(
            Some(file.to_str().unwrap()),
            true,
            false,
            &registry,
            None,
            &OutputFormat::Text,
            false,
            None,
        );
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
        let code = run_fix(
            Some(file.to_str().unwrap()),
            true,
            false,
            &registry,
            None,
            &OutputFormat::Text,
            false,
            None,
        );
        assert_eq!(code, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    // --stdin-filename tests
    #[test]
    fn parse_lint_stdin_filename() {
        let cli = Cli::try_parse_from(["boxlint", "lint", "--stdin-filename", "foo.txt"]).unwrap();
        match cli.command {
            Command::Lint {
                stdin_filename,
                path,
                ..
            } => {
                assert_eq!(stdin_filename.as_deref(), Some("foo.txt"));
                assert!(path.is_none());
            }
            _ => panic!("expected Lint command"),
        }
    }

    #[test]
    fn parse_fix_stdin_filename() {
        let cli = Cli::try_parse_from(["boxlint", "fix", "--stdin-filename", "bar.txt"]).unwrap();
        match cli.command {
            Command::Fix {
                stdin_filename,
                path,
                ..
            } => {
                assert_eq!(stdin_filename.as_deref(), Some("bar.txt"));
                assert!(path.is_none());
            }
            _ => panic!("expected Fix command"),
        }
    }

    // --diff tests
    #[test]
    fn parse_fix_diff_flag() {
        let cli = Cli::try_parse_from(["boxlint", "fix", "--diff"]).unwrap();
        match cli.command {
            Command::Fix { diff, .. } => {
                assert!(diff);
            }
            _ => panic!("expected Fix command"),
        }
    }

    #[test]
    fn fix_diff_and_in_place_error() {
        let registry = RuleRegistry::new();
        let code = run_fix(
            Some("f.txt"),
            true,
            false,
            &registry,
            None,
            &OutputFormat::Text,
            true,
            None,
        );
        assert_eq!(code, 2);
    }

    #[test]
    fn fix_diff_no_changes() {
        let dir = std::env::temp_dir().join("boxlint_test_fix_diff_clean");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("clean.txt");
        fs::write(&file, "hello\n").unwrap();

        let registry = RuleRegistry::new();
        let code = run_fix(
            Some(file.to_str().unwrap()),
            false,
            false,
            &registry,
            None,
            &OutputFormat::Text,
            true,
            None,
        );
        assert_eq!(code, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fix_diff_with_changes() {
        let dir = std::env::temp_dir().join("boxlint_test_fix_diff_chg");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("broken.txt");
        fs::write(&file, "bad text\n").unwrap();

        let mut registry = RuleRegistry::new();
        registry.fixers.push(Box::new(TestFixer));
        let code = run_fix(
            Some(file.to_str().unwrap()),
            false,
            false,
            &registry,
            None,
            &OutputFormat::Text,
            true,
            None,
        );
        assert_eq!(code, 1);

        let _ = fs::remove_dir_all(&dir);
    }

    // --format json for fix tests
    #[test]
    fn parse_fix_format_json() {
        let cli = Cli::try_parse_from(["boxlint", "fix", "--format", "json"]).unwrap();
        match cli.command {
            Command::Fix { format, .. } => {
                assert_eq!(format, OutputFormat::Json);
            }
            _ => panic!("expected Fix command"),
        }
    }

    #[test]
    fn fix_json_no_changes() {
        let dir = std::env::temp_dir().join("boxlint_test_fix_json_clean");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("clean.txt");
        fs::write(&file, "hello\n").unwrap();

        let registry = RuleRegistry::new();
        let code = run_fix(
            Some(file.to_str().unwrap()),
            false,
            false,
            &registry,
            None,
            &OutputFormat::Json,
            false,
            None,
        );
        assert_eq!(code, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fix_json_with_changes() {
        let dir = std::env::temp_dir().join("boxlint_test_fix_json_chg");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("broken.txt");
        fs::write(&file, "bad text\n").unwrap();

        let mut registry = RuleRegistry::new();
        registry.fixers.push(Box::new(TestFixer));
        let code = run_fix(
            Some(file.to_str().unwrap()),
            false,
            false,
            &registry,
            None,
            &OutputFormat::Json,
            false,
            None,
        );
        assert_eq!(code, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    // unified_diff tests
    #[test]
    fn unified_diff_no_changes() {
        let result = unified_diff("hello\nworld\n", "hello\nworld\n", "test.txt");
        // Should only have headers, no hunks
        assert!(result.starts_with("--- test.txt\n+++ test.txt\n"));
        assert!(!result.contains("@@"));
    }

    #[test]
    fn unified_diff_with_changes() {
        let result = unified_diff(
            "line1\nline2\nline3\n",
            "line1\nchanged\nline3\n",
            "test.txt",
        );
        assert!(result.contains("--- test.txt"));
        assert!(result.contains("+++ test.txt"));
        assert!(result.contains("@@"));
        assert!(result.contains("-line2"));
        assert!(result.contains("+changed"));
    }

    #[test]
    fn unified_diff_addition() {
        let result = unified_diff("a\nb\n", "a\nb\nc\n", "f.txt");
        assert!(result.contains("+c"));
    }

    #[test]
    fn unified_diff_deletion() {
        let result = unified_diff("a\nb\nc\n", "a\nc\n", "f.txt");
        assert!(result.contains("-b"));
    }

    #[test]
    fn unified_diff_with_trailing_context() {
        // A change followed by 3+ matching lines exercises the consecutive_match
        // break and context-after paths in unified_diff.
        let original = "a\nb\nc\nd\ne\nf\ng\n";
        let modified = "a\nX\nc\nd\ne\nf\ng\n";
        let result = unified_diff(original, modified, "ctx.txt");
        assert!(result.contains("-b"));
        assert!(result.contains("+X"));
        // Context after the hunk should include matching lines
        assert!(result.contains(" c"));
    }

    // --rule / --ignore tests

    #[test]
    fn parse_lint_rule_flag() {
        let cli = Cli::try_parse_from(["boxlint", "lint", "--rule", "box-corner-edge"]).unwrap();
        match cli.command {
            Command::Lint { rule, ignore, .. } => {
                assert_eq!(rule, vec!["box-corner-edge"]);
                assert!(ignore.is_empty());
            }
            _ => panic!("expected Lint command"),
        }
    }

    #[test]
    fn parse_lint_rule_comma_separated() {
        let cli =
            Cli::try_parse_from(["boxlint", "lint", "--rule", "box-corner-edge,arrow-connect"])
                .unwrap();
        match cli.command {
            Command::Lint { rule, .. } => {
                assert_eq!(rule, vec!["box-corner-edge", "arrow-connect"]);
            }
            _ => panic!("expected Lint command"),
        }
    }

    #[test]
    fn parse_lint_rule_repeated() {
        let cli = Cli::try_parse_from([
            "boxlint",
            "lint",
            "--rule",
            "box-corner-edge",
            "--rule",
            "arrow-connect",
        ])
        .unwrap();
        match cli.command {
            Command::Lint { rule, .. } => {
                assert_eq!(rule, vec!["box-corner-edge", "arrow-connect"]);
            }
            _ => panic!("expected Lint command"),
        }
    }

    #[test]
    fn parse_lint_ignore_flag() {
        let cli = Cli::try_parse_from(["boxlint", "lint", "--ignore", "arrow-connect"]).unwrap();
        match cli.command {
            Command::Lint { rule, ignore, .. } => {
                assert!(rule.is_empty());
                assert_eq!(ignore, vec!["arrow-connect"]);
            }
            _ => panic!("expected Lint command"),
        }
    }

    #[test]
    fn parse_fix_rule_flag() {
        let cli = Cli::try_parse_from(["boxlint", "fix", "--rule", "box-content-sizing"]).unwrap();
        match cli.command {
            Command::Fix { rule, ignore, .. } => {
                assert_eq!(rule, vec!["box-content-sizing"]);
                assert!(ignore.is_empty());
            }
            _ => panic!("expected Fix command"),
        }
    }

    #[test]
    fn parse_fix_ignore_flag() {
        let cli = Cli::try_parse_from([
            "boxlint",
            "fix",
            "--ignore",
            "box-corner-edge,adjacent-box-alignment",
        ])
        .unwrap();
        match cli.command {
            Command::Fix { ignore, .. } => {
                assert_eq!(ignore, vec!["box-corner-edge", "adjacent-box-alignment"]);
            }
            _ => panic!("expected Fix command"),
        }
    }

    #[test]
    fn filter_rule_retains_only_matching() {
        let mut registry = RuleRegistry::new();
        let rc = registry.filter(&["box-corner-edge".to_string()], &[]);
        assert_eq!(rc, 0);
        assert_eq!(registry.lint_rules.len(), 1);
        assert_eq!(registry.lint_rules[0].name(), "box-corner-edge");
        assert_eq!(registry.fixers.len(), 1);
        assert_eq!(registry.fixers[0].name(), "box-corner-edge");
    }

    #[test]
    fn filter_ignore_removes_matching() {
        let mut registry = RuleRegistry::new();
        let original_lint_count = registry.lint_rules.len();
        let original_fixer_count = registry.fixers.len();
        let rc = registry.filter(&[], &["arrow-connect".to_string()]);
        assert_eq!(rc, 0);
        assert_eq!(registry.lint_rules.len(), original_lint_count - 1);
        assert_eq!(registry.fixers.len(), original_fixer_count - 1);
        assert!(registry
            .lint_rules
            .iter()
            .all(|r| r.name() != "arrow-connect"));
        assert!(registry.fixers.iter().all(|f| f.name() != "arrow-connect"));
    }

    #[test]
    fn filter_rule_and_ignore_conflict() {
        let mut registry = RuleRegistry::new();
        let rc = registry.filter(
            &["box-corner-edge".to_string()],
            &["arrow-connect".to_string()],
        );
        assert_eq!(rc, 2);
    }

    #[test]
    fn filter_empty_is_noop() {
        let mut registry = RuleRegistry::new();
        let lint_count = registry.lint_rules.len();
        let fixer_count = registry.fixers.len();
        let rc = registry.filter(&[], &[]);
        assert_eq!(rc, 0);
        assert_eq!(registry.lint_rules.len(), lint_count);
        assert_eq!(registry.fixers.len(), fixer_count);
    }

    #[test]
    fn filter_unknown_name_warns_but_succeeds() {
        let mut registry = RuleRegistry::new();
        let lint_count = registry.lint_rules.len();
        // Unknown name should warn on stderr but still return 0
        let rc = registry.filter(&["nonexistent-rule".to_string()], &[]);
        assert_eq!(rc, 0);
        // All rules filtered out since none match
        assert_eq!(registry.lint_rules.len(), 0);
        // Verify original had rules (sanity check)
        assert!(lint_count > 0);
    }

    #[test]
    fn filter_mixed_lint_and_fixer_names() {
        // box-content-alignment is a lint name, box-content-sizing is a fixer name
        let mut registry = RuleRegistry::new();
        let rc = registry.filter(
            &[
                "box-content-alignment".to_string(),
                "box-content-sizing".to_string(),
            ],
            &[],
        );
        assert_eq!(rc, 0);
        assert_eq!(registry.lint_rules.len(), 1);
        assert_eq!(registry.lint_rules[0].name(), "box-content-alignment");
        assert_eq!(registry.fixers.len(), 1);
        assert_eq!(registry.fixers[0].name(), "box-content-sizing");
    }

    #[test]
    fn all_names_includes_both_lint_and_fixer() {
        let registry = RuleRegistry::new();
        let names = registry.all_names();
        assert!(names.contains(&"box-corner-edge"));
        assert!(names.contains(&"adjacent-box-alignment"));
        assert!(names.contains(&"box-content-alignment"));
        assert!(names.contains(&"arrow-connect"));
        assert!(names.contains(&"box-content-sizing"));
    }

    // check subcommand tests

    #[test]
    fn parse_check_no_args() {
        let cli = Cli::try_parse_from(["boxlint", "check"]).unwrap();
        match cli.command {
            Command::Check {
                path,
                stdin_filename,
                rule,
                ignore,
            } => {
                assert!(path.is_none());
                assert!(stdin_filename.is_none());
                assert!(rule.is_empty());
                assert!(ignore.is_empty());
            }
            _ => panic!("expected Check command"),
        }
    }

    #[test]
    fn parse_check_with_file() {
        let cli = Cli::try_parse_from(["boxlint", "check", "foo.txt"]).unwrap();
        match cli.command {
            Command::Check { path, .. } => {
                assert_eq!(path.as_deref(), Some("foo.txt"));
            }
            _ => panic!("expected Check command"),
        }
    }

    #[test]
    fn parse_check_with_rule() {
        let cli = Cli::try_parse_from(["boxlint", "check", "--rule", "box-corner-edge"]).unwrap();
        match cli.command {
            Command::Check { rule, .. } => {
                assert_eq!(rule, vec!["box-corner-edge"]);
            }
            _ => panic!("expected Check command"),
        }
    }

    #[test]
    fn parse_check_with_stdin_filename() {
        let cli =
            Cli::try_parse_from(["boxlint", "check", "--stdin-filename", "test.txt"]).unwrap();
        match cli.command {
            Command::Check {
                stdin_filename,
                path,
                ..
            } => {
                assert_eq!(stdin_filename.as_deref(), Some("test.txt"));
                assert!(path.is_none());
            }
            _ => panic!("expected Check command"),
        }
    }

    #[test]
    fn check_clean_file_returns_zero() {
        let dir = std::env::temp_dir().join("boxlint_test_check_clean");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("clean.txt");
        fs::write(&file, "hello\n").unwrap();

        let registry = RuleRegistry::new();
        let code = run_check(Some(file.to_str().unwrap()), &registry, None);
        assert_eq!(code, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_error_file_returns_one() {
        let dir = std::env::temp_dir().join("boxlint_test_check_err");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("broken.txt");
        fs::write(&file, "content").unwrap();

        let mut registry = RuleRegistry::new();
        registry.lint_rules.push(Box::new(ErrorRule));
        let code = run_check(Some(file.to_str().unwrap()), &registry, None);
        assert_eq!(code, 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_file_not_found_returns_two() {
        let registry = RuleRegistry::new();
        let code = run_check(Some("/nonexistent/path/to/file.txt"), &registry, None);
        assert_eq!(code, 2);
    }

    #[test]
    fn check_directory() {
        let dir = std::env::temp_dir().join("boxlint_test_check_dir");
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("a.txt"), "hello").unwrap();
        fs::write(dir.join("b.txt"), "world").unwrap();

        let registry = RuleRegistry::new();
        let code = run_check(Some(dir.to_str().unwrap()), &registry, None);
        assert_eq!(code, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_with_diagram_regions() {
        let dir = std::env::temp_dir().join("boxlint_test_check_regions");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("test.txt");
        fs::write(&file, "// ┌──┐\n// │hi│\n// └──┘\n").unwrap();

        let mut registry = RuleRegistry::new();
        registry.lint_rules.push(Box::new(ErrorRule));
        let code = run_check(Some(file.to_str().unwrap()), &registry, None);
        assert_eq!(code, 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_clean_diagram_regions_returns_zero() {
        let dir = std::env::temp_dir().join("boxlint_test_check_regions_clean");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("test.txt");
        fs::write(&file, "// ┌──┐\n// │hi│\n// └──┘\n").unwrap();

        let registry = RuleRegistry::new();
        let code = run_check(Some(file.to_str().unwrap()), &registry, None);
        assert_eq!(code, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_warnings_only_returns_zero() {
        struct WarnOnlyRule;
        impl LintRule for WarnOnlyRule {
            fn check(&self, _input: &str) -> Vec<Diagnostic> {
                vec![Diagnostic {
                    file: String::new(),
                    line: 1,
                    col: 1,
                    level: Level::Warning,
                    message: "a warning".to_string(),
                    rule: "warn-only".to_string(),
                }]
            }
            fn name(&self) -> &str {
                "warn-only"
            }
        }

        let dir = std::env::temp_dir().join("boxlint_test_check_warn");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("test.txt");
        fs::write(&file, "content").unwrap();

        let mut registry = RuleRegistry::new();
        registry.lint_rules.push(Box::new(WarnOnlyRule));
        let code = run_check(Some(file.to_str().unwrap()), &registry, None);
        assert_eq!(code, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    // lint --fix tests

    #[test]
    fn parse_lint_fix_flag() {
        let cli = Cli::try_parse_from(["boxlint", "lint", "--fix"]).unwrap();
        match cli.command {
            Command::Lint { fix, in_place, .. } => {
                assert!(fix);
                assert!(!in_place);
            }
            _ => panic!("expected Lint command"),
        }
    }

    #[test]
    fn parse_lint_fix_in_place() {
        let cli = Cli::try_parse_from(["boxlint", "lint", "--fix", "-i", "foo.txt"]).unwrap();
        match cli.command {
            Command::Lint {
                fix,
                in_place,
                path,
                ..
            } => {
                assert!(fix);
                assert!(in_place);
                assert_eq!(path.as_deref(), Some("foo.txt"));
            }
            _ => panic!("expected Lint command"),
        }
    }

    #[test]
    fn lint_in_place_without_fix_returns_two() {
        let registry = RuleRegistry::new();
        let code = run_lint(
            Some("f.txt"),
            &OutputFormat::Text,
            false,
            &registry,
            None,
            false,
            true,
        );
        assert_eq!(code, 2);
    }

    #[test]
    fn lint_fix_in_place_without_file_returns_two() {
        let registry = RuleRegistry::new();
        let code = run_lint(
            None,
            &OutputFormat::Text,
            false,
            &registry,
            None,
            true,
            true,
        );
        assert_eq!(code, 2);
    }

    #[test]
    fn lint_fix_applies_fixes() {
        let dir = std::env::temp_dir().join("boxlint_test_lint_fix");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("test.txt");
        fs::write(&file, "bad text").unwrap();

        let mut registry = RuleRegistry::new();
        registry.fixers.push(Box::new(TestFixer));
        let code = run_lint(
            Some(file.to_str().unwrap()),
            &OutputFormat::Text,
            false,
            &registry,
            None,
            true,
            false,
        );
        assert_eq!(code, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn lint_fix_in_place_writes_back() {
        let dir = std::env::temp_dir().join("boxlint_test_lint_fix_ip");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("test.txt");
        fs::write(&file, "bad text").unwrap();

        let mut registry = RuleRegistry::new();
        registry.fixers.push(Box::new(TestFixer));
        let code = run_lint(
            Some(file.to_str().unwrap()),
            &OutputFormat::Text,
            false,
            &registry,
            None,
            true,
            true,
        );
        assert_eq!(code, 0);
        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "good text");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn lint_fix_with_embedded_regions() {
        let dir = std::env::temp_dir().join("boxlint_test_lint_fix_embed");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("test.txt");
        fs::write(&file, "code\n// ┌──┐\n// │hi│\n// └──┘\nmore\n").unwrap();

        let registry = RuleRegistry::new();
        let code = run_lint(
            Some(file.to_str().unwrap()),
            &OutputFormat::Text,
            false,
            &registry,
            None,
            true,
            false,
        );
        assert_eq!(code, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn lint_fix_in_place_write_error() {
        let dir = std::env::temp_dir().join("boxlint_test_lint_fix_write_err");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("readonly.txt");
        fs::write(&file, "hello").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&file, fs::Permissions::from_mode(0o444));
        }
        let registry = RuleRegistry::new();
        let code = run_lint(
            Some(file.to_str().unwrap()),
            &OutputFormat::Text,
            false,
            &registry,
            None,
            true,
            true,
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&file, fs::Permissions::from_mode(0o644));
        }
        let _ = fs::remove_dir_all(&dir);
        #[cfg(unix)]
        assert_eq!(code, 2);
    }
}
