//! Jujutsu diff parser for unified diff format.
//!
//! Parses the output of `jj diff --git` which produces standard unified diff format.

use std::path::PathBuf;
use std::sync::LazyLock;

use crate::error::{Result, TuicrError};
use crate::model::{DiffFile, DiffHunk, DiffLine, FileStatus, LineOrigin};
use crate::syntax::SyntaxHighlighter;

static HIGHLIGHTER: LazyLock<SyntaxHighlighter> = LazyLock::new(SyntaxHighlighter::new);

/// Parse unified diff output from `jj diff --git` into DiffFile structures
pub fn parse_unified_diff(diff_text: &str) -> Result<Vec<DiffFile>> {
    let mut files: Vec<DiffFile> = Vec::new();
    let mut lines = diff_text.lines().peekable();

    while let Some(line) = lines.next() {
        // Look for "diff --git" style headers
        if line.starts_with("diff --git ") {
            let (old_path, new_path, status) = parse_file_header(&mut lines);

            // Check if binary
            if lines.peek().is_some_and(|l| l.contains("Binary")) {
                lines.next(); // consume binary message
                files.push(DiffFile {
                    old_path,
                    new_path,
                    status,
                    hunks: Vec::new(),
                    is_binary: true,
                });
                continue;
            }

            let file_path = new_path.as_ref().or(old_path.as_ref());
            let mut hunks = Vec::new();

            // Parse hunks until next file or end
            while lines.peek().is_some() {
                if let Some(peek_line) = lines.peek() {
                    if peek_line.starts_with("diff ") {
                        break;
                    } else if peek_line.starts_with("@@") {
                        if let Some(hunk) = parse_hunk(&mut lines, file_path) {
                            hunks.push(hunk);
                        }
                    } else {
                        lines.next(); // skip non-hunk, non-diff lines
                    }
                }
            }

            files.push(DiffFile {
                old_path,
                new_path,
                status,
                hunks,
                is_binary: false,
            });
        }
    }

    if files.is_empty() {
        return Err(TuicrError::NoChanges);
    }

    Ok(files)
}

fn parse_file_header<'a, I>(
    lines: &mut std::iter::Peekable<I>,
) -> (Option<PathBuf>, Option<PathBuf>, FileStatus)
where
    I: Iterator<Item = &'a str>,
{
    let mut old_path: Option<PathBuf> = None;
    let mut new_path: Option<PathBuf> = None;
    let mut status = FileStatus::Modified;

    // Parse --- and +++ lines and metadata
    while let Some(line) = lines.peek() {
        if line.starts_with("---") {
            let path_str = line.trim_start_matches("--- ").trim_start_matches("a/");
            if path_str != "/dev/null" {
                old_path = Some(PathBuf::from(path_str));
            }
            lines.next();
        } else if line.starts_with("+++") {
            let path_str = line.trim_start_matches("+++ ").trim_start_matches("b/");
            if path_str != "/dev/null" {
                new_path = Some(PathBuf::from(path_str));
            }
            lines.next();
            break; // Done with file header
        } else if line.starts_with("new file") {
            status = FileStatus::Added;
            lines.next();
        } else if line.starts_with("deleted file") {
            status = FileStatus::Deleted;
            lines.next();
        } else if line.starts_with("rename from") {
            status = FileStatus::Renamed;
            lines.next();
        } else if line.starts_with("copy from") {
            status = FileStatus::Copied;
            lines.next();
        } else if line.starts_with("@@") || line.starts_with("diff ") || line.contains("Binary") {
            break;
        } else {
            lines.next(); // Skip other metadata lines
        }
    }

    // Determine status from paths if not already set by metadata
    if status == FileStatus::Modified {
        if old_path.is_none() && new_path.is_some() {
            status = FileStatus::Added;
        } else if old_path.is_some() && new_path.is_none() {
            status = FileStatus::Deleted;
        }
    }

    (old_path, new_path, status)
}

fn parse_hunk<'a, I>(
    lines: &mut std::iter::Peekable<I>,
    file_path: Option<&PathBuf>,
) -> Option<DiffHunk>
where
    I: Iterator<Item = &'a str>,
{
    let header_line = lines.next()?;

    // Parse @@ -old_start,old_count +new_start,new_count @@
    let (old_start, old_count, new_start, new_count) = parse_hunk_header(header_line)?;

    let mut line_contents: Vec<String> = Vec::new();
    let mut line_origins: Vec<LineOrigin> = Vec::new();
    let mut line_numbers: Vec<(Option<u32>, Option<u32>)> = Vec::new();

    let mut old_lineno = old_start;
    let mut new_lineno = new_start;

    // Collect lines until next hunk or file
    while let Some(line) = lines.peek() {
        if line.starts_with("@@") || line.starts_with("diff ") {
            break;
        }

        let line = lines.next().unwrap();

        if line.starts_with('\\') {
            // "\ No newline at end of file" - skip
            continue;
        }

        let (origin, content, old_ln, new_ln) = if let Some(stripped) = line.strip_prefix('+') {
            if line.starts_with("+++") {
                continue;
            }
            let ln = new_lineno;
            new_lineno += 1;
            (LineOrigin::Addition, stripped, None, Some(ln))
        } else if let Some(stripped) = line.strip_prefix('-') {
            if line.starts_with("---") {
                continue;
            }
            let ln = old_lineno;
            old_lineno += 1;
            (LineOrigin::Deletion, stripped, Some(ln), None)
        } else if let Some(stripped) = line.strip_prefix(' ') {
            let old_ln = old_lineno;
            let new_ln = new_lineno;
            old_lineno += 1;
            new_lineno += 1;
            (LineOrigin::Context, stripped, Some(old_ln), Some(new_ln))
        } else if line.is_empty() {
            // Empty line in diff (context line with no content after space)
            let old_ln = old_lineno;
            let new_ln = new_lineno;
            old_lineno += 1;
            new_lineno += 1;
            (LineOrigin::Context, "", Some(old_ln), Some(new_ln))
        } else {
            // Unknown format, skip
            continue;
        };

        line_contents.push(content.to_string());
        line_origins.push(origin);
        line_numbers.push((old_ln, new_ln));
    }

    // Apply syntax highlighting if we have a file path
    let highlighted_lines =
        file_path.and_then(|path| HIGHLIGHTER.highlight_file_lines(path, &line_contents));

    // Build DiffLines
    let diff_lines: Vec<DiffLine> = line_contents
        .into_iter()
        .enumerate()
        .map(|(idx, content)| {
            let origin = line_origins[idx];
            let (old_lineno, new_lineno) = line_numbers[idx];

            let highlighted_spans = highlighted_lines.as_ref().and_then(|all| {
                all.get(idx)
                    .map(|spans| SyntaxHighlighter::apply_diff_background(spans.clone(), origin))
            });

            DiffLine {
                origin,
                content,
                old_lineno,
                new_lineno,
                highlighted_spans,
            }
        })
        .collect();

    Some(DiffHunk {
        header: header_line.to_string(),
        lines: diff_lines,
        old_start,
        old_count,
        new_start,
        new_count,
    })
}

fn parse_hunk_header(line: &str) -> Option<(u32, u32, u32, u32)> {
    // Format: @@ -old_start,old_count +new_start,new_count @@
    // or: @@ -old_start +new_start @@ (count defaults to 1)

    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 || parts[0] != "@@" {
        return None;
    }

    let old_part = parts[1].trim_start_matches('-');
    let new_part = parts[2].trim_start_matches('+');

    let (old_start, old_count) = parse_range(old_part);
    let (new_start, new_count) = parse_range(new_part);

    Some((old_start, old_count, new_start, new_count))
}

fn parse_range(s: &str) -> (u32, u32) {
    if let Some((start, count)) = s.split_once(',') {
        (start.parse().unwrap_or(1), count.parse().unwrap_or(1))
    } else {
        (s.parse().unwrap_or(1), 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_return_no_changes_for_empty_diff() {
        let result = parse_unified_diff("");
        assert!(matches!(result, Err(TuicrError::NoChanges)));
    }

    #[test]
    fn should_parse_simple_diff() {
        let diff = r#"diff --git a/file.txt b/file.txt
--- a/file.txt
+++ b/file.txt
@@ -1,3 +1,4 @@
 line1
+added
 line2
 line3
"#;
        let files = parse_unified_diff(diff).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].new_path, Some(PathBuf::from("file.txt")));
        assert_eq!(files[0].status, FileStatus::Modified);
        assert_eq!(files[0].hunks.len(), 1);
        assert_eq!(files[0].hunks[0].lines.len(), 4);
    }

    #[test]
    fn should_parse_new_file() {
        let diff = r#"diff --git a/new.txt b/new.txt
new file mode 100644
--- /dev/null
+++ b/new.txt
@@ -0,0 +1,2 @@
+line1
+line2
"#;
        let files = parse_unified_diff(diff).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, FileStatus::Added);
    }

    #[test]
    fn should_parse_deleted_file() {
        let diff = r#"diff --git a/old.txt b/old.txt
deleted file mode 100644
--- a/old.txt
+++ /dev/null
@@ -1,2 +0,0 @@
-line1
-line2
"#;
        let files = parse_unified_diff(diff).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, FileStatus::Deleted);
    }

    #[test]
    fn should_parse_renamed_file() {
        let diff = r#"diff --git a/old.txt b/new.txt
rename from old.txt
rename to new.txt
"#;
        let files = parse_unified_diff(diff).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, FileStatus::Renamed);
    }

    #[test]
    fn should_parse_multiple_files() {
        let diff = r#"diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1 +1 @@
-old
+new
diff --git a/b.txt b/b.txt
--- a/b.txt
+++ b/b.txt
@@ -1 +1 @@
-foo
+bar
"#;
        let files = parse_unified_diff(diff).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].new_path, Some(PathBuf::from("a.txt")));
        assert_eq!(files[1].new_path, Some(PathBuf::from("b.txt")));
    }

    #[test]
    fn should_parse_hunk_header() {
        let result = parse_hunk_header("@@ -1,3 +1,4 @@");
        assert_eq!(result, Some((1, 3, 1, 4)));

        let result = parse_hunk_header("@@ -10,5 +20,8 @@ context");
        assert_eq!(result, Some((10, 5, 20, 8)));
    }

    #[test]
    fn should_calculate_line_numbers() {
        let diff = r#"diff --git a/file.txt b/file.txt
--- a/file.txt
+++ b/file.txt
@@ -5,4 +5,5 @@
 context
-deleted
+added1
+added2
 more
"#;
        let files = parse_unified_diff(diff).unwrap();
        let hunk = &files[0].hunks[0];

        // First line is context at old:5, new:5
        assert_eq!(hunk.lines[0].old_lineno, Some(5));
        assert_eq!(hunk.lines[0].new_lineno, Some(5));

        // Second line is deletion at old:6
        assert_eq!(hunk.lines[1].old_lineno, Some(6));
        assert_eq!(hunk.lines[1].new_lineno, None);

        // Third line is addition at new:6
        assert_eq!(hunk.lines[2].old_lineno, None);
        assert_eq!(hunk.lines[2].new_lineno, Some(6));

        // Fourth line is addition at new:7
        assert_eq!(hunk.lines[3].old_lineno, None);
        assert_eq!(hunk.lines[3].new_lineno, Some(7));

        // Fifth line is context at old:7, new:8
        assert_eq!(hunk.lines[4].old_lineno, Some(7));
        assert_eq!(hunk.lines[4].new_lineno, Some(8));
    }
}
