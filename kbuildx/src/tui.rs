use std::{
    collections::BTreeMap,
    io::{self, BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

use crate::{
    config::{ConfigLine, ConfigValue, KernelConfig},
    consts::KERNEL_REPO,
};

const VIOLET: Color = Color::Rgb(125, 86, 244);
const CYAN: Color = Color::Rgb(0, 215, 215);
const GREEN: Color = Color::Rgb(0, 215, 135);
const YELLOW: Color = Color::Rgb(255, 215, 95);
const MUTED: Color = Color::Rgb(108, 112, 134);
const LOG_LIMIT: usize = 10_000;

const LOGO: &str = r#" _    ____        _ _     _
| | _| __ ) _   _(_) | __| |_  __
| |/ /  _ \| | | | |/ _` \ \/ /
|   <| |_) | |_| | | | (_| |>  <
|_|\_\____/ \__,_|_|_|\__,_/_/\_\"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modal {
    None,
    Versions,
    Configs,
    Help,
}

struct BuildProcess {
    child: Child,
    output: Receiver<String>,
}

struct App {
    modal: Modal,
    query: String,
    selected: usize,
    versions: Vec<String>,
    versions_rx: Receiver<Result<Vec<String>, String>>,
    selected_version: String,
    configs: Vec<(String, char)>,
    overrides: BTreeMap<String, char>,
    build: Option<BuildProcess>,
    build_status: String,
    logs: Vec<String>,
    log_top: usize,
    follow_logs: bool,
    should_quit: bool,
}

impl App {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = fetch_versions().map_err(|error| error.to_string());
            let _ = tx.send(result);
        });

        let configs = KernelConfig::default()
            .lines
            .into_iter()
            .filter_map(|line| match line {
                ConfigLine::Option { name, value } => match value {
                    ConfigValue::Yes => Some((name, 'y')),
                    ConfigValue::Module => Some((name, 'm')),
                    ConfigValue::No => Some((name, 'n')),
                    _ => None,
                },
                _ => None,
            })
            .collect();

        Self {
            modal: Modal::None,
            query: String::new(),
            selected: 0,
            versions: Vec::new(),
            versions_rx: rx,
            selected_version: "7.1.8".to_string(),
            configs,
            overrides: BTreeMap::new(),
            build: None,
            build_status: "Ready".to_string(),
            logs: vec!["Loading kernel versions…".to_string()],
            log_top: 0,
            follow_logs: true,
            should_quit: false,
        }
    }

    fn tick(&mut self) {
        if let Ok(result) = self.versions_rx.try_recv() {
            match result {
                Ok(versions) => {
                    self.logs
                        .push(format!("Loaded {} kernel versions", versions.len()));
                    if !versions
                        .iter()
                        .any(|version| version == &self.selected_version)
                        && let Some(version) = versions.first()
                    {
                        self.selected_version.clone_from(version);
                    }
                    self.versions = versions;
                }
                Err(error) => self.logs.push(format!("Version lookup failed: {error}")),
            }
        }

        let mut finished = None;
        if let Some(build) = &mut self.build {
            while let Ok(line) = build.output.try_recv() {
                self.logs.push(line);
            }
            if self.logs.len() > LOG_LIMIT {
                self.logs.drain(..self.logs.len() - LOG_LIMIT);
            }
            match build.child.try_wait() {
                Ok(Some(status)) => finished = Some(status.code().unwrap_or(-1)),
                Ok(None) => {}
                Err(error) => {
                    self.logs.push(format!("Unable to inspect build: {error}"));
                    finished = Some(-1);
                }
            }
        }
        if let Some(code) = finished {
            self.build = None;
            self.build_status = if code == 0 {
                "Build completed".to_string()
            } else {
                format!("Build failed (exit {code})")
            };
            self.logs.push(self.build_status.clone());
        }
    }

    fn start_build(&mut self) -> Result<()> {
        if self.build.is_some() {
            self.logs.push("A build is already running".to_string());
            return Ok(());
        }

        let executable = std::env::current_exe().context("locating kbuildx executable")?;
        let mut command = Command::new(executable);
        command
            .arg("build")
            .arg(&self.selected_version)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        for (name, value) in &self.overrides {
            command.arg("--set-config").arg(format!("{name}={value}"));
        }

        let mut child = command.spawn().context("starting kernel build")?;
        let stdout = child.stdout.take().context("capturing build stdout")?;
        let stderr = child.stderr.take().context("capturing build stderr")?;
        let (tx, rx) = mpsc::channel();
        for reader in [
            Box::new(BufReader::new(stdout)) as Box<dyn BufRead + Send>,
            Box::new(BufReader::new(stderr)) as Box<dyn BufRead + Send>,
        ] {
            let tx = tx.clone();
            thread::spawn(move || {
                for line in reader.lines().map_while(Result::ok) {
                    let _ = tx.send(line);
                }
            });
        }
        drop(tx);

        self.logs.clear();
        self.logs.push(format!(
            "Starting Linux {} with {} config override(s)",
            self.selected_version,
            self.overrides.len()
        ));
        self.follow_logs = true;
        self.build_status = "Building".to_string();
        self.build = Some(BuildProcess { child, output: rx });
        Ok(())
    }

    fn stop_build(&mut self) {
        if let Some(mut build) = self.build.take() {
            #[cfg(unix)]
            let stopped = unsafe { libc::kill(-(build.child.id() as i32), libc::SIGTERM) == 0 };
            #[cfg(not(unix))]
            let stopped = build.child.kill().is_ok();

            if stopped {
                let _ = build.child.wait();
                self.build_status = "Build stopped".to_string();
                self.logs.push("Build stopped by user".to_string());
            } else {
                self.logs
                    .push("Unable to stop the build process group".to_string());
                self.build = Some(build);
            }
        }
    }

    fn open_modal(&mut self, modal: Modal) {
        self.modal = modal;
        self.query.clear();
        self.selected = 0;
    }

    fn filtered_versions(&self) -> Vec<usize> {
        fuzzy_indices(self.versions.iter().map(String::as_str), &self.query)
    }

    fn filtered_configs(&self) -> Vec<usize> {
        fuzzy_indices(
            self.configs.iter().map(|(name, _)| name.as_str()),
            &self.query,
        )
    }

    fn modal_len(&self) -> usize {
        match self.modal {
            Modal::Versions => self.filtered_versions().len(),
            Modal::Configs => self.filtered_configs().len(),
            _ => 0,
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.stop_build();
            self.should_quit = true;
            return Ok(());
        }

        match self.modal {
            Modal::Help => match key.code {
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => self.modal = Modal::None,
                _ => {}
            },
            Modal::Versions | Modal::Configs => self.handle_search_key(key),
            Modal::None => match key.code {
                KeyCode::Char('q') => {
                    self.stop_build();
                    self.should_quit = true;
                }
                KeyCode::Char('?') => self.open_modal(Modal::Help),
                KeyCode::Char('/') => self.open_modal(Modal::Versions),
                KeyCode::Char('c') => self.open_modal(Modal::Configs),
                KeyCode::Char('b') => self.start_build()?,
                KeyCode::Char('x') => self.stop_build(),
                KeyCode::Up => {
                    self.follow_logs = false;
                    self.log_top = self.log_top.saturating_sub(1);
                }
                KeyCode::Down => {
                    self.follow_logs = false;
                    self.log_top = self.log_top.saturating_add(1);
                }
                KeyCode::PageUp => {
                    self.follow_logs = false;
                    self.log_top = self.log_top.saturating_sub(10);
                }
                KeyCode::PageDown => {
                    self.follow_logs = false;
                    self.log_top = self.log_top.saturating_add(10);
                }
                KeyCode::End => self.follow_logs = true,
                _ => {}
            },
        }
        Ok(())
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.modal = Modal::None,
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(self.modal_len().saturating_sub(1));
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.selected = 0;
            }
            KeyCode::Enter => {
                match self.modal {
                    Modal::Versions => {
                        if let Some(index) = self.filtered_versions().get(self.selected) {
                            self.selected_version.clone_from(&self.versions[*index]);
                        }
                    }
                    Modal::Configs => {
                        if let Some(index) = self.filtered_configs().get(self.selected) {
                            let (name, default) = &self.configs[*index];
                            let current = self.overrides.get(name).copied().unwrap_or(*default);
                            let next = match current {
                                'n' => 'y',
                                'y' => 'm',
                                _ => 'n',
                            };
                            self.overrides.insert(name.clone(), next);
                        }
                    }
                    _ => {}
                }
                self.modal = Modal::None;
            }
            KeyCode::Char(character) => {
                self.query.push(character);
                self.selected = 0;
            }
            _ => {}
        }
    }
}

pub fn run_tui() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_loop(&mut terminal);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    let mut app = App::new();
    while !app.should_quit {
        app.tick();
        terminal.draw(|frame| draw(frame, &mut app))?;
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.kind == crossterm::event::KeyEventKind::Press
        {
            app.handle_key(key)?;
        }
    }
    Ok(())
}

fn draw(frame: &mut Frame, app: &mut App) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let logo = Paragraph::new(LOGO)
        .alignment(Alignment::Center)
        .style(Style::default().fg(VIOLET).add_modifier(Modifier::BOLD));
    frame.render_widget(logo, areas[0]);

    let state = Line::from(vec![
        Span::styled(" Linux ", Style::default().fg(MUTED)),
        Span::styled(&app.selected_version, Style::default().fg(CYAN)),
        Span::styled("  Config overrides ", Style::default().fg(MUTED)),
        Span::styled(app.overrides.len().to_string(), Style::default().fg(YELLOW)),
        Span::styled("  Status ", Style::default().fg(MUTED)),
        Span::styled(&app.build_status, status_style(&app.build_status)),
    ]);
    frame.render_widget(
        Paragraph::new(state)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Kernel build workspace "),
            )
            .alignment(Alignment::Center),
        areas[1],
    );

    let height = areas[2].height.saturating_sub(2) as usize;
    let max_top = app.logs.len().saturating_sub(height);
    if app.follow_logs {
        app.log_top = max_top;
    } else {
        app.log_top = app.log_top.min(max_top);
    }
    let logs = app.logs.join("\n");
    frame.render_widget(
        Paragraph::new(logs)
            .scroll((app.log_top.min(u16::MAX as usize) as u16, 0))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(if app.follow_logs {
                        " Live build logs • following "
                    } else {
                        " Build logs • scroll paused (End to follow) "
                    }),
            ),
        areas[2],
    );

    let shortcuts = " b build  x stop  / versions  c config  ↑↓ logs  End follow  ? help  q quit ";
    frame.render_widget(
        Paragraph::new(shortcuts)
            .alignment(Alignment::Center)
            .style(
                Style::default()
                    .bg(VIOLET)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        areas[3],
    );

    match app.modal {
        Modal::Versions => draw_search_modal(frame, app, true),
        Modal::Configs => draw_search_modal(frame, app, false),
        Modal::Help => draw_help(frame),
        Modal::None => {}
    }
}

fn draw_search_modal(frame: &mut Frame, app: &App, versions: bool) {
    let area = centered_rect(76, 72, frame.area());
    frame.render_widget(Clear, area);
    let query = if app.query.is_empty() {
        "type to fuzzy-search…"
    } else {
        &app.query
    };
    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(2),
            Constraint::Length(1),
        ])
        .split(area);
    frame.render_widget(
        Paragraph::new(format!("> {query}"))
            .style(Style::default().fg(CYAN))
            .block(Block::default().borders(Borders::ALL).title(if versions {
                " / Kernel versions "
            } else {
                " / Kernel config "
            })),
        inner[0],
    );

    let indices = if versions {
        app.filtered_versions()
    } else {
        app.filtered_configs()
    };
    let items = indices
        .iter()
        .take(inner[1].height as usize)
        .enumerate()
        .map(|(row, index)| {
            let marker = if row == app.selected { "❯ " } else { "  " };
            let text = if versions {
                app.versions[*index].clone()
            } else {
                let (name, default) = &app.configs[*index];
                let value = app.overrides.get(name).copied().unwrap_or(*default);
                format!("CONFIG_{name}={value}")
            };
            ListItem::new(format!("{marker}{text}")).style(if row == app.selected {
                Style::default().fg(VIOLET).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            })
        });
    frame.render_widget(List::new(items), inner[1]);
    frame.render_widget(
        Paragraph::new(if versions {
            " Enter select • Esc cancel "
        } else {
            " Enter cycle n → y → m and set • Esc cancel "
        })
        .alignment(Alignment::Center)
        .style(Style::default().fg(MUTED)),
        inner[2],
    );
}

fn draw_help(frame: &mut Frame) {
    let area = centered_rect(64, 62, frame.area());
    frame.render_widget(Clear, area);
    let help = Text::from(vec![
        Line::styled(
            "Keyboard shortcuts",
            Style::default().fg(VIOLET).add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        Line::from("b       Start the selected kernel build"),
        Line::from("x       Stop the running build"),
        Line::from("/       Fuzzy-search and select a kernel version"),
        Line::from("c       Fuzzy-search and set a kernel config option"),
        Line::from("↑/↓     Scroll build logs and pause auto-follow"),
        Line::from("PgUp/Dn Scroll logs by ten lines"),
        Line::from("End     Resume real-time log following"),
        Line::from("?       Toggle this help"),
        Line::from("q       Quit (stops an active build)"),
        Line::from(""),
        Line::styled("Press Esc or ? to close", Style::default().fg(CYAN)),
    ]);
    frame.render_widget(
        Paragraph::new(help).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" kbuildx help "),
        ),
        area,
    );
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn fuzzy_indices<'a>(values: impl Iterator<Item = &'a str>, query: &str) -> Vec<usize> {
    if query.is_empty() {
        return values.enumerate().map(|(index, _)| index).collect();
    }
    let matcher = SkimMatcherV2::default();
    let mut scored: Vec<_> = values
        .enumerate()
        .filter_map(|(index, value)| {
            matcher
                .fuzzy_match(value, query)
                .map(|score| (index, score))
        })
        .collect();
    scored.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
    scored.into_iter().map(|(index, _)| index).collect()
}

fn fetch_versions() -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["ls-remote", "--tags", KERNEL_REPO])
        .output()
        .context("fetching kernel tags")?;
    if !output.status.success() {
        anyhow::bail!("git ls-remote exited with {}", output.status);
    }
    let mut versions: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.ends_with("^{}") && !line.contains("-rc"))
        .filter_map(|line| line.split("refs/tags/v").nth(1))
        .map(ToOwned::to_owned)
        .collect();
    versions.sort_by(|left, right| version_key(right).cmp(&version_key(left)));
    versions.dedup();
    Ok(versions)
}

fn version_key(version: &str) -> Vec<u64> {
    version
        .split('.')
        .map(|part| part.parse().unwrap_or_default())
        .collect()
}

fn status_style(status: &str) -> Style {
    if status == "Building" {
        Style::default().fg(YELLOW)
    } else if status.contains("completed") {
        Style::default().fg(GREEN)
    } else if status.contains("failed") || status.contains("stopped") {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(MUTED)
    }
}

#[cfg(test)]
mod tests {
    use super::{fuzzy_indices, version_key};

    #[test]
    fn fuzzy_search_prioritizes_close_matches() {
        let values = ["7.1.8", "6.6.1", "7.0.1"];
        let matches = fuzzy_indices(values.into_iter(), "718");
        assert_eq!(matches.first(), Some(&0));
    }

    #[test]
    fn version_keys_sort_numerically() {
        assert!(version_key("7.1.10") > version_key("7.1.9"));
    }
}
