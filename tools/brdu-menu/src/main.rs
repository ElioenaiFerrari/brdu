//! `cargo menu`: development menu with arrow keys. Dev only: runs `brdu-dev` as a subprocess.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use brdu_menu::{CATALOG, Item, command_line, defaults};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

const GREEN: Color = Color::Rgb(0, 155, 58);
const GOLD: Color = Color::Rgb(254, 223, 0);

enum Row {
    Group(&'static str),
    Item(&'static Item),
}

fn rows() -> Vec<Row> {
    let (mut out, mut last) = (Vec::new(), "");
    for i in CATALOG {
        if i.group != last {
            out.push(Row::Group(i.group));
            last = i.group;
        }
        out.push(Row::Item(i));
    }
    out
}

enum Screen {
    Menu,
    Form {
        item: &'static Item,
        values: Vec<String>,
        focus: usize,
    },
    Running {
        title: String,
        lines: Vec<String>,
        rx: Receiver<Option<String>>,
        done: bool,
        back: usize,
    },
}

struct App {
    rows: Vec<Row>,
    sel: usize,
    list: ListState,
    screen: Screen,
    dir: String,
}

fn brdu_dev() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let bin = exe.with_file_name("brdu-dev");
    if !bin.exists() {
        let ok = Command::new("cargo")
            .args(["build", "-q", "-p", "brdu-dev"])
            .status()
            .map_err(|e| e.to_string())?
            .success();
        if !ok || !bin.exists() {
            return Err("could not build brdu-dev".into());
        }
    }
    Ok(bin)
}

fn spawn(args: Vec<String>) -> Screen {
    let title = format!("cargo dev {}", args.join(" "));
    let (tx, rx) = channel();
    let bin = brdu_dev();
    std::thread::spawn(move || {
        let bin = match bin {
            Ok(b) => b,
            Err(e) => {
                let _ = tx.send(Some(e));
                let _ = tx.send(None);
                return;
            }
        };
        let child = Command::new(bin)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        match child {
            Ok(mut c) => {
                let (out, err) = (c.stdout.take(), c.stderr.take());
                let t2 = tx.clone();
                let h = std::thread::spawn(move || {
                    if let Some(e) = err {
                        for l in BufReader::new(e).lines().map_while(Result::ok) {
                            let _ = t2.send(Some(l));
                        }
                    }
                });
                if let Some(o) = out {
                    for l in BufReader::new(o).lines().map_while(Result::ok) {
                        let _ = tx.send(Some(l));
                    }
                }
                let _ = h.join();
                let code = c.wait().ok().and_then(|s| s.code());
                let _ = tx.send(Some(format!(
                    "— finished (exit code {})",
                    code.map_or("?".into(), |c| c.to_string())
                )));
            }
            Err(e) => {
                let _ = tx.send(Some(e.to_string()));
            }
        }
        let _ = tx.send(None);
    });
    Screen::Running {
        title,
        lines: Vec::new(),
        rx,
        done: false,
        back: 0,
    }
}

fn step(app: &mut App, dir_toggle: &mut bool, delta: isize) {
    let _ = dir_toggle;
    let n = app.rows.len() as isize;
    let mut i = app.sel as isize;
    loop {
        i = (i + delta).rem_euclid(n);
        if matches!(app.rows[i as usize], Row::Item(_)) {
            break;
        }
    }
    app.sel = i as usize;
    app.list.select(Some(app.sel));
}

fn draw(f: &mut ratatui::Frame, app: &mut App) {
    let area = f.area();
    let [head, body, foot] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(2),
    ])
    .areas(area);
    let title = Line::from(vec![
        Span::styled(
            " BRDU ",
            Style::new()
                .fg(Color::Black)
                .bg(GREEN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  development menu  "),
        Span::styled(format!("[--dir {}]", app.dir), Style::new().fg(GOLD)),
        Span::raw("   dev only: test keys"),
    ]);
    f.render_widget(
        Paragraph::new(title).block(Block::default().borders(Borders::BOTTOM)),
        head,
    );
    match &app.screen {
        Screen::Menu => {
            let items: Vec<ListItem> = app
                .rows
                .iter()
                .map(|r| match r {
                    Row::Group(g) => ListItem::new(Line::styled(
                        format!("── {g} ──"),
                        Style::new().fg(GOLD).add_modifier(Modifier::BOLD),
                    )),
                    Row::Item(i) => ListItem::new(format!("   {}", i.label)),
                })
                .collect();
            let list = List::new(items)
                .highlight_style(Style::new().fg(Color::Black).bg(GREEN))
                .highlight_symbol("▶");
            f.render_stateful_widget(list, body, &mut app.list);
            f.render_widget(
                Paragraph::new(
                    "↑/↓ choose · Enter open · d switch folder (net ⇄ compose) · q quit",
                ),
                foot,
            );
        }
        Screen::Form {
            item,
            values,
            focus,
        } => {
            let mut lines = vec![
                Line::styled(
                    item.label,
                    Style::new().fg(GOLD).add_modifier(Modifier::BOLD),
                ),
                Line::raw(""),
            ];
            for (i, p) in item.params.iter().enumerate() {
                let mark = if i == *focus { "▶ " } else { "  " };
                let style = if i == *focus {
                    Style::new().fg(GREEN).add_modifier(Modifier::BOLD)
                } else {
                    Style::new()
                };
                lines.push(Line::styled(
                    format!("{mark}{:<18} {}", p.name, values[i]),
                    style,
                ));
            }
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                format!(
                    "cargo dev {}",
                    command_line(item, values, &app.dir).join(" ")
                ),
                Style::new().fg(Color::DarkGray),
            ));
            f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
            f.render_widget(
                Paragraph::new("type to edit · Tab/↑/↓ switch field · Enter run · Esc back"),
                foot,
            );
        }
        Screen::Running {
            title,
            lines,
            back,
            done,
            ..
        } => {
            let h = body.height.saturating_sub(2) as usize;
            let end = lines.len().saturating_sub(*back);
            let start = end.saturating_sub(h);
            let text: Vec<Line> = lines[start..end]
                .iter()
                .map(|l| Line::raw(l.as_str()))
                .collect();
            f.render_widget(
                Paragraph::new(text)
                    .block(Block::default().borders(Borders::ALL).title(title.as_str())),
                body,
            );
            f.render_widget(
                Paragraph::new(if *done {
                    "finished · ↑/↓ scroll · Esc back"
                } else {
                    "running… · ↑/↓ scroll · Esc back (does not interrupt)"
                }),
                foot,
            );
        }
    }
}

fn run(term: &mut DefaultTerminal) -> std::io::Result<()> {
    let rows = rows();
    let first = rows
        .iter()
        .position(|r| matches!(r, Row::Item(_)))
        .unwrap_or(0);
    let mut app = App {
        rows,
        sel: first,
        list: ListState::default(),
        screen: Screen::Menu,
        dir: {
            let args: Vec<String> = std::env::args().skip(1).collect();
            brdu_menu::initial_dir(&args, std::env::var("BRDU_DIR").ok().as_deref(), &|p| {
                std::path::Path::new(p).exists()
            })
        },
    };
    app.list.select(Some(first));
    let mut toggle = false;
    loop {
        if let Screen::Running {
            lines, rx, done, ..
        } = &mut app.screen
        {
            while let Ok(m) = rx.try_recv() {
                match m {
                    Some(l) => lines.push(l),
                    None => *done = true,
                }
            }
        }
        term.draw(|f| draw(f, &mut app))?;
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        let Event::Key(k) = event::read()? else {
            continue;
        };
        if k.kind != KeyEventKind::Press {
            continue;
        }
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            return Ok(());
        }
        match &mut app.screen {
            Screen::Menu => match k.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Up => step(&mut app, &mut toggle, -1),
                KeyCode::Down => step(&mut app, &mut toggle, 1),
                KeyCode::Char('d') => {
                    app.dir = if app.dir == "compose" {
                        "net".into()
                    } else {
                        "compose".into()
                    }
                }
                KeyCode::Enter => {
                    if let Row::Item(item) = app.rows[app.sel] {
                        app.screen = if item.params.is_empty() {
                            spawn(command_line(item, &[], &app.dir))
                        } else {
                            Screen::Form {
                                item,
                                values: defaults(item),
                                focus: 0,
                            }
                        };
                    }
                }
                _ => {}
            },
            Screen::Form {
                item,
                values,
                focus,
            } => match k.code {
                KeyCode::Esc => app.screen = Screen::Menu,
                KeyCode::Tab | KeyCode::Down => *focus = (*focus + 1) % values.len(),
                KeyCode::Up => *focus = (*focus + values.len() - 1) % values.len(),
                KeyCode::Backspace => {
                    values[*focus].pop();
                }
                KeyCode::Char(c) => values[*focus].push(c),
                KeyCode::Enter => {
                    let args = command_line(item, values, &app.dir);
                    app.screen = spawn(args);
                }
                _ => {}
            },
            Screen::Running { back, lines, .. } => match k.code {
                KeyCode::Esc | KeyCode::Char('q') => app.screen = Screen::Menu,
                KeyCode::Up => *back = (*back + 1).min(lines.len()),
                KeyCode::Down => *back = back.saturating_sub(1),
                _ => {}
            },
        }
    }
}

fn main() {
    let mut term = ratatui::init();
    let r = run(&mut term);
    ratatui::restore();
    if let Err(e) = r {
        eprintln!("brdu-menu: {e}");
        std::process::exit(1);
    }
}
