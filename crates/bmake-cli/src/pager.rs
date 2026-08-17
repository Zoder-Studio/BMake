use crate::syntax_docs::{all_entries, SyntaxEntry};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{self, ClearType};
use crossterm::{cursor, execute, queue};
use std::io::{stdout, IsTerminal, Write};

enum View {
    Index,
    Detail(usize),
}

pub fn run(requested_version: Option<&str>) -> Result<()> {
    let entries: Vec<SyntaxEntry> = all_entries()
        .into_iter()
        .filter(|e| requested_version.map(|v| e.since <= v).unwrap_or(true))
        .collect();

    if entries.is_empty() {
        println!(" No syntax reference available for version {:?}", requested_version);
        return Ok(());
    }

    if !stdout().is_terminal() {
        print_plain(&entries);
        return Ok(());
    }

    terminal::enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, terminal::EnterAlternateScreen, cursor::Hide)?;

    let result = interactive_loop(&mut out, &entries);

    execute!(out, cursor::Show, terminal::LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;
    result
}

fn print_plain(entries: &[SyntaxEntry]) {
    for e in entries {
        println!(
            "{}\n{}\n\nPurpose:\n  {}\n\nScope:\n  {}\n\nExample:\n{}\n",
            e.name,
            e.form,
            e.purpose,
            e.scope,
            indent(e.example)
        );
        if !e.related.is_empty() {
            println!("Related: {}\n", e.related.join(", "));
        }
        println!("---");
    }
}

fn indent(s: &str) -> String {
    s.lines().map(|l| format!("  {}", l)).collect::<Vec<_>>().join("\n")
}

fn interactive_loop(out: &mut impl Write, entries: &[SyntaxEntry]) -> Result<()> {
    let mut view = View::Index;
    let mut scroll: u16 = 0;
    let mut search_query = String::new();
    let mut searching = false;
    let mut selected = 0usize;

    loop {
        let (_cols, rows) = terminal::size()?;
        let lines = render_lines(&view, entries, selected);
        draw(out, &lines, scroll, rows, &search_query, searching)?;

        let Event::Key(key) = event::read()? else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        if searching {
            match key.code {
                KeyCode::Enter | KeyCode::Esc => searching = false,
                KeyCode::Backspace => {
                    search_query.pop();
                }
                KeyCode::Char(c) => search_query.push(c),
                _ => {}
            }
            if let Some(pos) = find_match(&lines, &search_query) {
                scroll = pos;
            }
            continue;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => match view {
                View::Detail(_) => {
                    view = View::Index;
                    scroll = 0;
                }
                View::Index => break,
            },
            KeyCode::Up | KeyCode::Char('k') => match view {
                View::Index => selected = selected.saturating_sub(1),
                View::Detail(_) => scroll = scroll.saturating_sub(1),
            },
            KeyCode::Down | KeyCode::Char('j') => match view {
                View::Index => selected = (selected + 1).min(entries.len().saturating_sub(1)),
                View::Detail(_) => scroll = scroll.saturating_add(1),
            },
            KeyCode::PageUp => scroll = scroll.saturating_sub(rows.saturating_sub(2)),
            KeyCode::PageDown => scroll = scroll.saturating_add(rows.saturating_sub(2)),
            KeyCode::Char('g') => scroll = 0,
            KeyCode::Char('G') => scroll = (lines.len() as u16).saturating_sub(rows.saturating_sub(2)),
            KeyCode::Char('/') => {
                searching = true;
                search_query.clear();
            }
            KeyCode::Char('n') => {
                if let Some(pos) = find_match(&lines, &search_query) {
                    scroll = pos;
                }
            }
            KeyCode::Enter => {
                if let View::Index = view {
                    view = View::Detail(selected);
                    scroll = 0;
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn render_lines(view: &View, entries: &[SyntaxEntry], selected: usize) -> Vec<String> {
    match view {
        View::Index => {
            let mut lines = vec![
                "BMake Syntax Reference".to_string(),
                String::new(),
                "up/k down/j move   Enter open   / search   n next   q quit".to_string(),
                String::new(),
            ];
            for (i, e) in entries.iter().enumerate() {
                let marker = if i == selected { "> " } else { "  " };
                lines.push(format!("{}{}", marker, e.name));
            }
            lines
        }
        View::Detail(idx) => {
            let e = &entries[*idx];
            let mut lines = vec![e.name.to_string(), String::new(), "Form:".to_string()];
            for l in e.form.lines() {
                lines.push(format!("  {}", l));
            }
            lines.push(String::new());
            lines.push("Purpose:".to_string());
            lines.push(format!("  {}", e.purpose));
            lines.push(String::new());
            lines.push("Scope:".to_string());
            lines.push(format!("  {}", e.scope));
            lines.push(String::new());
            lines.push("Example:".to_string());
            for l in e.example.lines() {
                lines.push(format!("  {}", l));
            }
            if !e.related.is_empty() {
                lines.push(String::new());
                lines.push(format!("Related: {}", e.related.join(", ")));
            }
            lines.push(String::new());
            lines.push(format!("Since BMake Engine: {}", e.since));
            if let Some(dep) = e.deprecated {
                lines.push(format!("Deprecated: {}", dep));
            }
            lines.push(String::new());
            lines.push("Esc/q back   up/k down/j scroll   / search   n next".to_string());
            lines
        }
    }
}

fn draw(out: &mut impl Write, lines: &[String], scroll: u16, rows: u16, query: &str, searching: bool) -> Result<()> {
    queue!(out, cursor::MoveTo(0, 0), terminal::Clear(ClearType::All))?;
    let visible_rows = rows.saturating_sub(1) as usize;
    for (i, line) in lines.iter().skip(scroll as usize).take(visible_rows).enumerate() {
        queue!(out, cursor::MoveTo(0, i as u16))?;
        write!(out, "{}", truncate(line, 200))?;
    }
    queue!(out, cursor::MoveTo(0, rows.saturating_sub(1)))?;
    if searching {
        write!(out, "/{}", query)?;
    } else {
        write!(out, "-- BMake Syntax --")?;
    }
    out.flush()?;
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        s.chars().take(max).collect()
    } else {
        s.to_string()
    }
}

fn find_match(lines: &[String], query: &str) -> Option<u16> {
    if query.is_empty() {
        return None;
    }
    let q = query.to_lowercase();
    lines.iter().position(|l| l.to_lowercase().contains(&q)).map(|p| p as u16)
}