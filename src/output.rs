//! Output formatting: readable tables on a terminal, tab-separated text in
//! pipes, JSON on request. Colors are only used on a TTY and never when
//! `NO_COLOR` is set.

use anyhow::Result;
use clap::Args;
use serde::Serialize;
use std::io::{IsTerminal, Write};

/// Shared `--json` flag for commands that print data.
#[derive(Args, Debug, Clone, Copy, Default)]
pub struct OutputArgs {
    /// Print JSON instead of a table
    #[arg(long)]
    pub json: bool,
}

impl OutputArgs {
    /// Print `value` as JSON when `--json` was given, else run `table`.
    pub fn print<T: Serialize>(&self, value: &T, table: impl FnOnce() -> Table) -> Result<()> {
        if self.json {
            print_json(value)
        } else {
            table().print();
            Ok(())
        }
    }
}

/// Whether stdout is an interactive terminal.
pub fn stdout_is_tty() -> bool {
    std::io::stdout().is_terminal()
}

/// Whether to emit ANSI styling: a TTY and `NO_COLOR` unset.
pub fn use_color() -> bool {
    stdout_is_tty() && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
}

/// Print `value` as JSON: pretty on a terminal, compact in a pipe.
pub fn print_json<T: Serialize>(value: &T) -> Result<()> {
    let mut out = std::io::stdout().lock();
    if stdout_is_tty() {
        serde_json::to_writer_pretty(&mut out, value)?;
    } else {
        serde_json::to_writer(&mut out, value)?;
    }
    out.write_all(b"\n")?;
    Ok(())
}

/// A simple column table. On a TTY, columns are padded and headers bold;
/// in a pipe, fields are tab-separated and the header row is kept so
/// `cut`/`awk` scripts stay stable.
#[derive(Debug, Default)]
pub struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new<S: Into<String>>(headers: impl IntoIterator<Item = S>) -> Self {
        Self {
            headers: headers.into_iter().map(Into::into).collect(),
            rows: Vec::new(),
        }
    }

    pub fn row<S: Into<String>>(&mut self, cells: impl IntoIterator<Item = S>) -> &mut Self {
        self.rows.push(cells.into_iter().map(Into::into).collect());
        self
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn print(&self) {
        let text = if stdout_is_tty() {
            self.render_aligned(use_color())
        } else {
            self.render_tsv()
        };
        let mut out = std::io::stdout().lock();
        // A closed pipe (`| head`) is not an error worth reporting.
        let _ = out.write_all(text.as_bytes());
    }

    fn render_tsv(&self) -> String {
        let mut s = String::new();
        for line in std::iter::once(&self.headers).chain(&self.rows) {
            s.push_str(&line.join("\t"));
            s.push('\n');
        }
        s
    }

    fn render_aligned(&self, color: bool) -> String {
        let cols = self.headers.len();
        let mut widths: Vec<usize> = self.headers.iter().map(|h| h.chars().count()).collect();
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate().take(cols) {
                widths[i] = widths[i].max(cell.chars().count());
            }
        }
        let mut s = String::new();
        let header = pad_line(&self.headers, &widths);
        if color {
            s.push_str("\x1b[1m");
            s.push_str(&header);
            s.push_str("\x1b[0m\n");
        } else {
            s.push_str(&header);
            s.push('\n');
        }
        for row in &self.rows {
            s.push_str(&pad_line(row, &widths));
            s.push('\n');
        }
        s
    }
}

fn pad_line(cells: &[String], widths: &[usize]) -> String {
    let last = widths.len().saturating_sub(1);
    let mut line = String::new();
    for (i, width) in widths.iter().enumerate() {
        let cell = cells.get(i).map_or("", String::as_str);
        if i == last {
            line.push_str(cell);
        } else {
            let pad = width.saturating_sub(cell.chars().count());
            line.push_str(cell);
            line.extend(std::iter::repeat_n(' ', pad + 2));
        }
    }
    line.trim_end().to_string()
}

/// Shorten an ISO-8601 timestamp to `YYYY-MM-DD HH:MM` for tables; other
/// strings pass through.
pub fn short_time(iso: &str) -> String {
    if iso.len() >= 16 && iso.as_bytes()[10] == b'T' {
        format!("{} {}", &iso[..10], &iso[11..16])
    } else {
        iso.to_string()
    }
}

/// A dash for missing values in tables.
pub fn or_dash(v: Option<&str>) -> String {
    match v {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => "-".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Table {
        let mut t = Table::new(["ID", "NAME", "STATE"]);
        t.row(["1", "alpha", "ok"]).row(["22", "b", "failed"]);
        t
    }

    #[test]
    fn tsv_keeps_header_and_tabs() {
        assert_eq!(
            sample().render_tsv(),
            "ID\tNAME\tSTATE\n1\talpha\tok\n22\tb\tfailed\n"
        );
    }

    #[test]
    fn aligned_pads_all_but_last_column() {
        let s = sample().render_aligned(false);
        assert_eq!(s, "ID  NAME   STATE\n1   alpha  ok\n22  b      failed\n");
    }

    #[test]
    fn aligned_bolds_header_with_color() {
        let s = sample().render_aligned(true);
        assert!(s.starts_with("\x1b[1mID  NAME   STATE\x1b[0m\n"));
    }

    #[test]
    fn helpers_format_times_and_blanks() {
        assert_eq!(short_time("2026-10-11T22:59:01+00:00"), "2026-10-11 22:59");
        assert_eq!(short_time("n/a"), "n/a");
        assert_eq!(or_dash(None), "-");
        assert_eq!(or_dash(Some("")), "-");
        assert_eq!(or_dash(Some("x")), "x");
    }
}
