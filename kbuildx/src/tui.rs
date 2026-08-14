use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read},
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
    Resources,
    BuildOptions,
    Help,
}

struct BuildProcess {
    child: Child,
    output: Receiver<Vec<u8>>,
    readers: Vec<thread::JoinHandle<()>>,
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
    cpus: u32,
    memory: u32,
    resource_field: usize,
    resource_cpus: String,
    resource_memory: String,
    build_args: Vec<String>,
    build_options_input: String,
    build: Option<BuildProcess>,
    build_status: String,
    logs: Vec<String>,
    partial_log: String,
    pending_carriage_return: bool,
    terminal_escape_state: u8,
    log_top: usize,
    follow_logs: bool,
    fullscreen_logs: bool,
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
        let host_cpus = std::thread::available_parallelism()
            .map(|count| count.get() as u32)
            .unwrap_or(2);

        Self {
            modal: Modal::None,
            query: String::new(),
            selected: 0,
            versions: Vec::new(),
            versions_rx: rx,
            selected_version: "7.1.8".to_string(),
            configs,
            overrides: BTreeMap::new(),
            cpus: host_cpus,
            memory: 2048,
            resource_field: 0,
            resource_cpus: host_cpus.to_string(),
            resource_memory: "2048".to_string(),
            build_args: Vec::new(),
            build_options_input: String::new(),
            build: None,
            build_status: "Ready".to_string(),
            logs: vec!["Loading kernel versions…".to_string()],
            partial_log: String::new(),
            pending_carriage_return: false,
            terminal_escape_state: 0,
            log_top: 0,
            follow_logs: true,
            fullscreen_logs: false,
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
        let mut chunks = Vec::new();
        if let Some(build) = &mut self.build {
            while let Ok(chunk) = build.output.try_recv() {
                chunks.push(chunk);
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
        for chunk in chunks {
            self.consume_output(&chunk);
        }
        if self.logs.len() > LOG_LIMIT {
            self.logs.drain(..self.logs.len() - LOG_LIMIT);
        }
        if let Some(code) = finished {
            if let Some(build) = self.build.take() {
                for reader in build.readers {
                    let _ = reader.join();
                }
                while let Ok(chunk) = build.output.try_recv() {
                    self.consume_output(&chunk);
                }
            }
            self.finish_partial_log();
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
        command.arg("build");
        if self.build_args.is_empty() {
            command
                .arg(&self.selected_version)
                .arg("--cpus")
                .arg(self.cpus.to_string())
                .arg("--memory")
                .arg(self.memory.to_string());
        } else {
            command.args(&self.build_args);
        }
        let (output, child_stdout, child_stderr) = output_pty()?;
        command.stdout(child_stdout).stderr(child_stderr);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        for (name, value) in &self.overrides {
            command.arg("--set-config").arg(format!("{name}={value}"));
        }

        let child = command.spawn().context("starting kernel build")?;
        drop(command);
        let (tx, rx) = mpsc::channel();
        let mut readers = Vec::new();
        for reader in [Box::new(output) as Box<dyn Read + Send>] {
            let tx = tx.clone();
            readers.push(thread::spawn(move || {
                let mut reader = reader;
                let mut buffer = [0_u8; 4096];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(length) => {
                            if tx.send(buffer[..length].to_vec()).is_err() {
                                break;
                            }
                        }
                    }
                }
            }));
        }
        drop(tx);

        self.logs.clear();
        self.partial_log.clear();
        self.pending_carriage_return = false;
        self.terminal_escape_state = 0;
        self.logs.push(if self.build_args.is_empty() {
            format!(
                "Starting Linux {} with {} config override(s), {} vCPU, {} MiB",
                self.selected_version,
                self.overrides.len(),
                self.cpus,
                self.memory
            )
        } else {
            format!("Starting custom build: {}", shell_join(&self.build_args))
        });
        self.follow_logs = true;
        self.build_status = "Building".to_string();
        self.build = Some(BuildProcess {
            child,
            output: rx,
            readers,
        });
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
        if modal == Modal::Resources {
            self.resource_cpus = self.cpus.to_string();
            self.resource_memory = self.memory.to_string();
            self.resource_field = 0;
        } else if modal == Modal::BuildOptions {
            self.build_options_input = if self.build_args.is_empty() {
                format!(
                    "{} --cpus {} --memory {}",
                    self.selected_version, self.cpus, self.memory
                )
            } else {
                shell_join(&self.build_args)
            };
        }
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
        if self.fullscreen_logs && key.code == KeyCode::Esc {
            self.fullscreen_logs = false;
            return Ok(());
        }

        match self.modal {
            Modal::Help => match key.code {
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => self.modal = Modal::None,
                _ => {}
            },
            Modal::Versions | Modal::Configs => self.handle_search_key(key),
            Modal::Resources => self.handle_resource_key(key),
            Modal::BuildOptions => self.handle_build_options_key(key),
            Modal::None => match key.code {
                KeyCode::Char('q') => {
                    self.stop_build();
                    self.should_quit = true;
                }
                KeyCode::Char('?') => self.open_modal(Modal::Help),
                KeyCode::Char('/') => self.open_modal(Modal::Versions),
                KeyCode::Char('c') => self.open_modal(Modal::Configs),
                KeyCode::Char('r') => self.open_modal(Modal::Resources),
                KeyCode::Char('o') => self.open_modal(Modal::BuildOptions),
                KeyCode::Char('l') => self.fullscreen_logs = !self.fullscreen_logs,
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
            KeyCode::Enter => match self.modal {
                Modal::Versions => {
                    if let Some(index) = self.filtered_versions().get(self.selected) {
                        self.selected_version.clone_from(&self.versions[*index]);
                    }
                    self.modal = Modal::None;
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
            },
            KeyCode::Char(character) => {
                self.query.push(character);
                self.selected = 0;
            }
            _ => {}
        }
    }

    fn handle_resource_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.modal = Modal::None,
            KeyCode::Tab | KeyCode::Up | KeyCode::Down => {
                self.resource_field = 1 - self.resource_field;
            }
            KeyCode::Backspace => {
                self.active_resource_input().pop();
            }
            KeyCode::Char(character) if character.is_ascii_digit() => {
                self.active_resource_input().push(character);
            }
            KeyCode::Enter => {
                let cpus = self
                    .resource_cpus
                    .parse::<u32>()
                    .ok()
                    .filter(|value| *value > 0);
                let memory = self
                    .resource_memory
                    .parse::<u32>()
                    .ok()
                    .filter(|value| *value > 0);
                match (cpus, memory) {
                    (Some(cpus), Some(memory)) => {
                        self.cpus = cpus;
                        self.memory = memory;
                        self.logs
                            .push(format!("Resources set to {cpus} vCPU, {memory} MiB"));
                        self.modal = Modal::None;
                    }
                    _ => self
                        .logs
                        .push("CPU and memory values must be greater than zero".to_string()),
                }
            }
            _ => {}
        }
    }

    fn active_resource_input(&mut self) -> &mut String {
        if self.resource_field == 0 {
            &mut self.resource_cpus
        } else {
            &mut self.resource_memory
        }
    }

    fn consume_output(&mut self, chunk: &[u8]) {
        let mut visible = Vec::with_capacity(chunk.len());
        for &byte in chunk {
            match self.terminal_escape_state {
                0 if byte == 0x1b => self.terminal_escape_state = 1,
                0 => visible.push(byte),
                1 => {
                    self.terminal_escape_state = match byte {
                        b'[' => 2,
                        b']' => 3,
                        _ => 0,
                    };
                }
                2 => {
                    if (0x40..=0x7e).contains(&byte) {
                        self.terminal_escape_state = 0;
                    }
                }
                3 => match byte {
                    0x07 => self.terminal_escape_state = 0,
                    0x1b => self.terminal_escape_state = 4,
                    _ => {}
                },
                4 => {
                    self.terminal_escape_state = if byte == b'\\' { 0 } else { 3 };
                }
                _ => self.terminal_escape_state = 0,
            }
        }
        for character in String::from_utf8_lossy(&visible).chars() {
            if self.pending_carriage_return {
                self.pending_carriage_return = false;
                if character == '\n' {
                    self.finish_partial_log();
                    continue;
                }
                self.partial_log.clear();
            }
            match character {
                '\r' => self.pending_carriage_return = true,
                '\n' => self.finish_partial_log(),
                '\u{8}' => {
                    self.partial_log.pop();
                }
                '\t' => self.partial_log.push_str("    "),
                character if character.is_control() => {}
                _ => self.partial_log.push(character),
            }
        }
    }

    fn finish_partial_log(&mut self) {
        self.pending_carriage_return = false;
        if !self.partial_log.is_empty() {
            self.logs.push(std::mem::take(&mut self.partial_log));
        }
    }

    fn handle_build_options_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.modal = Modal::None,
            KeyCode::Backspace => {
                self.build_options_input.pop();
            }
            KeyCode::Enter => match shell_split(&self.build_options_input) {
                Ok(args) if !args.is_empty() => {
                    self.build_args = args;
                    self.logs.push(format!(
                        "Build options overridden: {}",
                        shell_join(&self.build_args)
                    ));
                    self.modal = Modal::None;
                }
                Ok(_) => self
                    .logs
                    .push("Build options must include a kernel version".to_string()),
                Err(error) => self.logs.push(format!("Invalid build options: {error}")),
            },
            KeyCode::Char(character) => self.build_options_input.push(character),
            _ => {}
        }
    }
}

#[cfg(unix)]
fn output_pty() -> Result<(File, Stdio, Stdio)> {
    use std::os::fd::FromRawFd;

    let mut master = -1;
    let mut slave = -1;
    // SAFETY: openpty initializes both descriptors on success. Each descriptor
    // is immediately wrapped in exactly one File, transferring ownership.
    if unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error()).context("opening build pseudo-terminal");
    }
    // SAFETY: openpty returned owned, valid file descriptors.
    let master = unsafe { File::from_raw_fd(master) };
    // SAFETY: openpty returned owned, valid file descriptors.
    let slave = unsafe { File::from_raw_fd(slave) };
    let stderr = slave.try_clone().context("cloning build pseudo-terminal")?;
    Ok((master, Stdio::from(slave), Stdio::from(stderr)))
}

#[cfg(not(unix))]
fn output_pty() -> Result<(File, Stdio, Stdio)> {
    anyhow::bail!("the TUI build log pseudo-terminal is unsupported on this platform")
}

fn shell_split(input: &str) -> std::result::Result<Vec<String>, &'static str> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut started = false;
    for character in input.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            started = true;
        } else if character == '\\' && quote != Some('\'') {
            escaped = true;
            started = true;
        } else if matches!(character, '\'' | '"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
                started = true;
            } else {
                current.push(character);
            }
        } else if character.is_whitespace() && quote.is_none() {
            if started {
                args.push(std::mem::take(&mut current));
                started = false;
            }
        } else {
            current.push(character);
            started = true;
        }
    }
    if escaped || quote.is_some() {
        return Err("unterminated quote or escape");
    }
    if started {
        args.push(current);
    }
    Ok(args)
}

fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-._/=:".contains(c))
            {
                arg.clone()
            } else {
                format!("'{}'", arg.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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
    if app.fullscreen_logs {
        draw_fullscreen_logs(frame, app);
        return;
    }

    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let mut logo_lines: Vec<Line> = aligned_logo_lines()
        .into_iter()
        .map(|line| {
            Line::styled(
                line,
                Style::default().fg(VIOLET).add_modifier(Modifier::BOLD),
            )
        })
        .collect();
    logo_lines.push(Line::from(""));
    logo_lines.push(Line::styled(
        "Interactive Linux kernel builder",
        Style::default().fg(CYAN),
    ));
    let logo = Paragraph::new(Text::from(logo_lines)).alignment(Alignment::Center);
    frame.render_widget(logo, areas[0]);

    let state = Line::from(vec![
        Span::styled(" Linux ", Style::default().fg(MUTED)),
        Span::styled(&app.selected_version, Style::default().fg(CYAN)),
        Span::styled("  Config overrides ", Style::default().fg(MUTED)),
        Span::styled(app.overrides.len().to_string(), Style::default().fg(YELLOW)),
        Span::styled("  Resources ", Style::default().fg(MUTED)),
        Span::styled(
            format!("{} CPU / {} MiB", app.cpus, app.memory),
            Style::default().fg(CYAN),
        ),
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

    draw_logs(frame, app, areas[2]);

    let shortcuts =
        " b build  x stop  / version  c config  r resources  o options  l logs  ? help  q quit ";
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
        Modal::Resources => draw_resource_modal(frame, app),
        Modal::BuildOptions => draw_build_options_modal(frame, app),
        Modal::Help => draw_help(frame),
        Modal::None => {}
    }
}

fn draw_fullscreen_logs(frame: &mut Frame, app: &mut App) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(frame.area());
    draw_logs(frame, app, areas[0]);
    frame.render_widget(
        Paragraph::new(" l/Esc normal view  ↑↓/PgUp/PgDn scroll  End follow  x stop  q quit ")
            .alignment(Alignment::Center)
            .style(
                Style::default()
                    .bg(VIOLET)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        areas[1],
    );
}

fn draw_logs(frame: &mut Frame, app: &mut App, area: Rect) {
    let height = area.height.saturating_sub(2) as usize;
    let visible_lines = app.logs.len() + usize::from(!app.partial_log.is_empty());
    let max_top = visible_lines.saturating_sub(height);
    if app.follow_logs {
        app.log_top = max_top;
    } else {
        app.log_top = app.log_top.min(max_top);
    }
    let mut logs = app.logs.join("\n");
    if !app.partial_log.is_empty() {
        if !logs.is_empty() {
            logs.push('\n');
        }
        logs.push_str(&app.partial_log);
    }
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
        area,
    );
}

fn draw_resource_modal(frame: &mut Frame, app: &App) {
    let area = centered_rect(54, 58, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(VIOLET))
        .title(" Build resources ");
    let content = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(content);

    frame.render_widget(
        Paragraph::new("Set sandbox CPU and memory limits")
            .alignment(Alignment::Center)
            .style(Style::default().fg(MUTED)),
        rows[0],
    );
    for (index, (title, value)) in [
        (" vCPUs ", app.resource_cpus.as_str()),
        (" Memory (MiB) ", app.resource_memory.as_str()),
    ]
    .into_iter()
    .enumerate()
    {
        let selected = app.resource_field == index;
        frame.render_widget(
            Paragraph::new(format!("> {value}"))
                .style(Style::default().fg(if selected { CYAN } else { Color::White }))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(if selected { VIOLET } else { MUTED }))
                        .title(title),
                ),
            rows[index + 1],
        );
    }
    frame.render_widget(
        Paragraph::new(" Tab/↑/↓ switch • Enter apply • Esc cancel ")
            .alignment(Alignment::Center)
            .style(Style::default().fg(MUTED)),
        rows[4],
    );
}

fn draw_build_options_modal(frame: &mut Frame, app: &App) {
    let area = centered_rect(86, 48, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(VIOLET))
        .title(" Override all build options ");
    let content = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .split(content);
    frame.render_widget(
        Paragraph::new("Enter every argument after `kbuildx build` (quotes are supported).")
            .alignment(Alignment::Center)
            .style(Style::default().fg(MUTED)),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(format!("> {}", app.build_options_input))
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(CYAN))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Build arguments "),
            ),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new("Example: 7.1.8 --host --modules --defconfig x86_64_defconfig\nEnter apply • Esc cancel")
            .alignment(Alignment::Center)
            .style(Style::default().fg(MUTED)),
        rows[2],
    );
}

fn aligned_logo_lines() -> Vec<String> {
    let width = LOGO.lines().map(str::len).max().unwrap_or_default();
    LOGO.lines().map(|line| format!("{line:<width$}")).collect()
}

fn draw_search_modal(frame: &mut Frame, app: &App, versions: bool) {
    let area = centered_rect(76, 72, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(VIOLET))
            .title(if versions {
                " Kernel versions "
            } else {
                " Kernel config "
            }),
        area,
    );
    let content = area.inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 1,
    });
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
        .split(content);
    frame.render_widget(
        Paragraph::new(format!("> {query}"))
            .style(Style::default().fg(CYAN))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Fuzzy search "),
            ),
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
            " Enter cycle n → y → m • Esc done "
        })
        .alignment(Alignment::Center)
        .style(Style::default().fg(MUTED)),
        inner[2],
    );
}

fn draw_help(frame: &mut Frame) {
    let area = centered_rect(64, 70, frame.area());
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
        Line::from("r       Edit sandbox CPU and memory options"),
        Line::from("o       Override every build command option"),
        Line::from("l       Toggle fullscreen build logs"),
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
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{
        App, Modal, aligned_logo_lines, fuzzy_indices, output_pty, shell_split, version_key,
    };

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

    #[test]
    fn logo_uses_a_fixed_width_canvas() {
        let lines = aligned_logo_lines();
        assert!(lines.windows(2).all(|pair| pair[0].len() == pair[1].len()));
    }

    #[test]
    fn log_shortcut_toggles_fullscreen_view() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE))
            .unwrap();
        assert!(app.fullscreen_logs);

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        assert!(!app.fullscreen_logs);
    }

    #[test]
    fn tui_defaults_to_available_host_cpus() {
        let app = App::new();
        let available = std::thread::available_parallelism()
            .map(|count| count.get() as u32)
            .unwrap_or(2);
        assert_eq!(app.cpus, available);
    }

    #[test]
    fn build_options_support_shell_style_quotes() {
        assert_eq!(
            shell_split("7.1.8 --uimage-name 'CI kernel'").unwrap(),
            ["7.1.8", "--uimage-name", "CI kernel"]
        );
        assert!(shell_split("7.1.8 --uimage-name 'unfinished").is_err());
    }

    #[test]
    fn output_chunks_stream_lines_and_carriage_return_progress() {
        let mut app = App::new();
        app.logs.clear();

        app.consume_output(b"compile 10%");
        assert_eq!(app.partial_log, "compile 10%");
        app.consume_output(b"\rcompile 20%\nnext");

        assert_eq!(app.logs, ["compile 20%"]);
        assert_eq!(app.partial_log, "next");
    }

    #[test]
    fn output_chunks_remove_terminal_control_sequences() {
        let mut app = App::new();
        app.logs.clear();

        app.consume_output(b"\x1b[38;2;125;");
        app.consume_output(b"86;244m[1/4 SANDBOX]\x1b[0m Configuring sandbox\n");
        app.consume_output(b"install 10%\x1b[2K\rinstall 20%");

        assert_eq!(app.logs, ["[1/4 SANDBOX] Configuring sandbox"]);
        assert_eq!(app.partial_log, "install 20%");
    }

    #[cfg(unix)]
    #[test]
    fn build_output_uses_a_real_terminal() {
        use std::io::Read;
        use std::process::Command;

        let (mut output, stdout, stderr) = output_pty().unwrap();
        let status = Command::new("sh")
            .args([
                "-c",
                "test -t 1 && test -t 2 && printf 'stdout\\nstderr\\n' >&2",
            ])
            .stdout(stdout)
            .stderr(stderr)
            .status()
            .unwrap();
        let mut captured = String::new();
        output.read_to_string(&mut captured).unwrap();

        assert!(status.success(), "child failed; captured: {captured:?}");
        assert!(captured.contains("stdout"), "captured: {captured:?}");
        assert!(captured.contains("stderr"), "captured: {captured:?}");
    }

    #[test]
    fn build_options_modal_applies_complete_argument_list() {
        let mut app = App::new();
        app.modal = Modal::BuildOptions;
        app.build_options_input = "7.1.8 --host --modules".to_string();
        app.handle_build_options_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.build_args, ["7.1.8", "--host", "--modules"]);
        assert_eq!(app.modal, Modal::None);
    }

    #[test]
    fn enter_cycles_config_without_closing_modal() {
        let mut app = App::new();
        app.modal = Modal::Configs;
        app.query = "BPF".to_string();
        let index = app.filtered_configs()[0];
        let (name, default) = app.configs[index].clone();
        app.handle_search_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.modal, Modal::Configs);
        assert_ne!(app.overrides.get(&name), Some(&default));
    }

    #[test]
    fn resource_modal_applies_cpu_and_memory_values() {
        let mut app = App::new();
        app.modal = Modal::Resources;
        app.resource_cpus = "8".to_string();
        app.resource_memory = "8192".to_string();
        app.handle_resource_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.cpus, 8);
        assert_eq!(app.memory, 8192);
        assert_eq!(app.modal, Modal::None);
    }
}
