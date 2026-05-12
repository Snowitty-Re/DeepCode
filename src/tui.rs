use crate::cache::{cache_key, read_cached, write_cached};
use crate::cli::Workflow;
use crate::code_structure::fill_missing_structure;
use crate::config::Config;
use crate::deepseek::DeepSeekClient;
use crate::prompts::build_messages;
use crate::report::{parse_report, AnalysisReport};
use crate::scanner::{scan_path, ProjectSnapshot, ScanOptions};
use anyhow::Result;
use crossterm::cursor::Show;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

pub fn run_chat(path: PathBuf, config: &Config, no_cache: bool) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    let mut app = ChatApp::new(path, config.clone(), no_cache);
    app.start_scan(&tx);
    run_terminal(app, rx, tx)
}

struct ChatApp {
    target: PathBuf,
    config: Config,
    no_cache: bool,
    snapshot: Option<ProjectSnapshot>,
    messages: Vec<ChatMessage>,
    input: String,
    status: String,
    busy: bool,
    should_quit: bool,
    chat_scroll: u16,
    file_scroll: u16,
    selected_file: usize,
    focus: FocusPane,
}

struct ChatMessage {
    role: ChatRole,
    content: String,
}

#[derive(Clone, Copy)]
enum ChatRole {
    System,
    User,
    Assistant,
    Error,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FocusPane {
    Files,
    Chat,
}

enum WorkerEvent {
    Status(String),
    ScanComplete(Result<ProjectSnapshot, String>),
    AnswerComplete(Result<String, String>),
}

impl ChatApp {
    fn new(target: PathBuf, config: Config, no_cache: bool) -> Self {
        Self {
            target,
            config,
            no_cache,
            snapshot: None,
            messages: vec![ChatMessage {
                role: ChatRole::System,
                content: "已进入 DeepCode 中文对话模式。输入问题分析当前目标；可用 /rescan、/path <路径>、/clear、/quit。".to_string(),
            }],
            input: String::new(),
            status: "准备扫描".to_string(),
            busy: false,
            should_quit: false,
            chat_scroll: 0,
            file_scroll: 0,
            selected_file: 0,
            focus: FocusPane::Chat,
        }
    }

    fn start_scan(&mut self, tx: &Sender<WorkerEvent>) {
        self.busy = true;
        self.snapshot = None;
        self.selected_file = 0;
        self.file_scroll = 0;
        self.status = format!("正在扫描 {}", self.target.display());
        let target = self.target.clone();
        let options = self.scan_options();
        let tx = tx.clone();
        thread::spawn(move || {
            let _ = tx.send(WorkerEvent::Status(format!(
                "正在扫描 {}",
                target.display()
            )));
            let result = scan_path(&target, options).map_err(|error| error.to_string());
            let _ = tx.send(WorkerEvent::ScanComplete(result));
        });
    }

    fn ask(&mut self, question: String, tx: &Sender<WorkerEvent>) {
        let Some(snapshot) = self.snapshot.clone() else {
            self.messages.push(ChatMessage {
                role: ChatRole::Error,
                content: "当前目标还没有扫描完成，稍后再提问。".to_string(),
            });
            return;
        };
        let selected_file = self.selected_file_path();
        self.messages.push(ChatMessage {
            role: ChatRole::User,
            content: format_user_question(&question, selected_file.as_deref()),
        });
        self.busy = true;
        self.status = "正在请求 DeepSeek".to_string();
        let config = self.config.clone();
        let no_cache = self.no_cache;
        let tx = tx.clone();
        thread::spawn(move || {
            let result = answer_question(question, selected_file, snapshot, config, no_cache, &tx);
            let _ = tx.send(WorkerEvent::AnswerComplete(result));
        });
    }

    fn handle_command(&mut self, command: &str, tx: &Sender<WorkerEvent>) {
        let trimmed = command.trim();
        if trimmed == "/quit" || trimmed == "/exit" {
            self.should_quit = true;
        } else if trimmed == "/clear" {
            self.messages.clear();
            self.chat_scroll = 0;
            self.messages.push(ChatMessage {
                role: ChatRole::System,
                content: "对话已清空。".to_string(),
            });
        } else if trimmed == "/rescan" {
            self.messages.push(ChatMessage {
                role: ChatRole::System,
                content: "开始重新扫描当前目标。".to_string(),
            });
            self.start_scan(tx);
        } else if let Some(path) = trimmed.strip_prefix("/path ") {
            let path = PathBuf::from(path.trim());
            self.target = path;
            self.messages.push(ChatMessage {
                role: ChatRole::System,
                content: format!("已切换目标：{}", self.target.display()),
            });
            self.start_scan(tx);
        } else {
            self.messages.push(ChatMessage {
                role: ChatRole::Error,
                content: "未知命令。可用 /rescan、/path <路径>、/clear、/quit。".to_string(),
            });
        }
    }

    fn handle_worker_event(&mut self, event: WorkerEvent) {
        match event {
            WorkerEvent::Status(status) => self.status = status,
            WorkerEvent::ScanComplete(Ok(snapshot)) => {
                self.status = format!(
                    "扫描完成：{} 个文件，跳过 {} 个，{} 字节，{} 行代码",
                    snapshot.summary.files_read,
                    snapshot.summary.files_skipped,
                    snapshot.summary.bytes_read,
                    snapshot.summary.total_code_lines
                );
                self.messages.push(ChatMessage {
                    role: ChatRole::System,
                    content: self.status.clone(),
                });
                self.snapshot = Some(snapshot);
                self.selected_file = 0;
                self.file_scroll = 0;
                self.busy = false;
            }
            WorkerEvent::ScanComplete(Err(error)) => {
                self.status = "扫描失败".to_string();
                self.messages.push(ChatMessage {
                    role: ChatRole::Error,
                    content: format!("扫描失败：{error}"),
                });
                self.busy = false;
            }
            WorkerEvent::AnswerComplete(Ok(answer)) => {
                self.status = "回答完成".to_string();
                self.messages.push(ChatMessage {
                    role: ChatRole::Assistant,
                    content: answer,
                });
                self.busy = false;
            }
            WorkerEvent::AnswerComplete(Err(error)) => {
                self.status = "请求失败".to_string();
                self.messages.push(ChatMessage {
                    role: ChatRole::Error,
                    content: format!("请求失败：{error}"),
                });
                self.busy = false;
            }
        }
    }

    fn scan_options(&self) -> ScanOptions {
        ScanOptions {
            max_file_bytes: self.config.max_file_bytes,
            max_files: self.config.max_files,
            max_total_bytes: self.config.max_total_bytes,
            max_concurrency: self.config.max_concurrency,
        }
    }

    fn selected_file_path(&self) -> Option<String> {
        self.snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.files.get(self.selected_file))
            .map(|file| file.path.display().to_string())
    }

    fn file_count(&self) -> usize {
        self.snapshot
            .as_ref()
            .map(|snapshot| snapshot.files.len())
            .unwrap_or(0)
    }

    fn move_file_selection(&mut self, delta: isize) {
        let count = self.file_count();
        if count == 0 {
            self.selected_file = 0;
            self.file_scroll = 0;
            return;
        }
        let current = self.selected_file as isize;
        let next = (current + delta).clamp(0, count.saturating_sub(1) as isize) as usize;
        self.selected_file = next;
        self.keep_selected_file_visible();
    }

    fn keep_selected_file_visible(&mut self) {
        let selected = self.selected_file as u16;
        if selected < self.file_scroll {
            self.file_scroll = selected;
        } else if selected >= self.file_scroll.saturating_add(12) {
            self.file_scroll = selected.saturating_sub(11);
        }
    }
}

fn run_terminal(
    mut app: ChatApp,
    rx: Receiver<WorkerEvent>,
    tx: Sender<WorkerEvent>,
) -> Result<()> {
    enable_raw_mode()?;
    let _guard = TerminalGuard;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    while !app.should_quit {
        while let Ok(event) = rx.try_recv() {
            app.handle_worker_event(event);
        }
        terminal.draw(|frame| draw(frame, &app))?;
        if event::poll(Duration::from_millis(80))? {
            if let Event::Key(key) = event::read()? {
                handle_key(key, &mut app, &tx);
            }
        }
    }

    Ok(())
}

fn handle_key(key: KeyEvent, app: &mut ChatApp, tx: &Sender<WorkerEvent>) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return;
    }
    match key.code {
        KeyCode::Esc => app.should_quit = true,
        KeyCode::Left => app.focus = FocusPane::Files,
        KeyCode::Right => app.focus = FocusPane::Chat,
        KeyCode::Enter => {
            let input = app.input.trim().to_string();
            app.input.clear();
            if input.is_empty() {
                return;
            }
            if input.starts_with('/') {
                app.handle_command(&input, tx);
            } else if app.busy {
                app.messages.push(ChatMessage {
                    role: ChatRole::Error,
                    content: "当前仍在处理上一个任务，请稍后再输入。".to_string(),
                });
            } else {
                app.ask(input, tx);
            }
        }
        KeyCode::Char(character) => app.input.push(character),
        KeyCode::Backspace => {
            app.input.pop();
        }
        KeyCode::Up => {
            if app.focus == FocusPane::Files && app.input.is_empty() {
                app.move_file_selection(-1);
            } else {
                app.chat_scroll = app.chat_scroll.saturating_add(1);
            }
        }
        KeyCode::Down => {
            if app.focus == FocusPane::Files && app.input.is_empty() {
                app.move_file_selection(1);
            } else {
                app.chat_scroll = app.chat_scroll.saturating_sub(1);
            }
        }
        KeyCode::PageUp => {
            if app.focus == FocusPane::Files && app.input.is_empty() {
                app.move_file_selection(-8);
            } else {
                app.chat_scroll = app.chat_scroll.saturating_add(8);
            }
        }
        KeyCode::PageDown => {
            if app.focus == FocusPane::Files && app.input.is_empty() {
                app.move_file_selection(8);
            } else {
                app.chat_scroll = app.chat_scroll.saturating_sub(8);
            }
        }
        _ => {}
    }
}

fn draw(frame: &mut Frame<'_>, app: &ChatApp) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(38), Constraint::Min(40)])
        .split(chunks[0]);
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Length(8),
            Constraint::Min(8),
        ])
        .split(main[0]);

    let messages = Paragraph::new(render_messages(app))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(focus_title("对话", app.focus == FocusPane::Chat)),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.chat_scroll, 0));
    frame.render_widget(render_target(app), left[0]);
    frame.render_widget(render_summary(app), left[1]);
    frame.render_widget(render_files(app), left[2]);
    frame.render_widget(messages, main[1]);

    let input = Paragraph::new(app.input.as_str())
        .block(Block::default().borders(Borders::ALL).title("输入"));
    frame.render_widget(input, chunks[1]);

    let status_style = if app.busy {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Green)
    };
    let focus = match app.focus {
        FocusPane::Files => "文件",
        FocusPane::Chat => "对话",
    };
    let selected = app
        .selected_file_path()
        .unwrap_or_else(|| "未选择文件".to_string());
    let status = Paragraph::new(Text::from(vec![
        Line::from(vec![
            Span::styled("状态：", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(app.status.as_str(), status_style),
        ]),
        Line::from(vec![
            Span::styled("焦点：", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(focus),
            Span::raw("  选中文件："),
            Span::styled(selected, Style::default().fg(Color::Cyan)),
            Span::raw("  ←/→切换  ↑/↓选择或滚动  /rescan /path /clear /quit"),
        ]),
    ]))
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(status, chunks[2]);
}

fn render_target(app: &ChatApp) -> Paragraph<'static> {
    Paragraph::new(Text::from(vec![
        Line::from(Span::styled(
            "DeepCode Chat",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(app.target.display().to_string()),
    ]))
    .block(Block::default().borders(Borders::ALL).title("目标"))
    .wrap(Wrap { trim: false })
}

fn render_summary(app: &ChatApp) -> Paragraph<'static> {
    let mut lines = Vec::new();
    if let Some(snapshot) = &app.snapshot {
        lines.push(Line::from(format!("文件：{}", snapshot.summary.files_read)));
        lines.push(Line::from(format!(
            "跳过：{}",
            snapshot.summary.files_skipped
        )));
        lines.push(Line::from(format!("字节：{}", snapshot.summary.bytes_read)));
        lines.push(Line::from(format!(
            "代码行：{}",
            snapshot.summary.total_code_lines
        )));
        let languages = snapshot
            .summary
            .languages
            .iter()
            .take(2)
            .map(|language| format!("{} {}", language.language, language.files))
            .collect::<Vec<_>>()
            .join(", ");
        if !languages.is_empty() {
            lines.push(Line::from(languages));
        }
    } else {
        lines.push(Line::from("等待扫描结果"));
    }
    Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::ALL).title("扫描摘要"))
}

fn render_files(app: &ChatApp) -> List<'static> {
    let mut items = Vec::new();
    if let Some(snapshot) = &app.snapshot {
        for (index, file) in snapshot
            .files
            .iter()
            .enumerate()
            .skip(app.file_scroll as usize)
        {
            let selected = index == app.selected_file;
            let marker = if selected { ">" } else { " " };
            let style = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            items.push(ListItem::new(Line::from(vec![
                Span::raw(marker),
                Span::raw(" "),
                Span::styled(file.path.display().to_string(), style),
            ])));
        }
    }
    if items.is_empty() {
        items.push(ListItem::new("暂无文件"));
    }
    List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(focus_title("文件", app.focus == FocusPane::Files)),
    )
}

fn focus_title(title: &str, focused: bool) -> String {
    if focused {
        format!("* {title}")
    } else {
        title.to_string()
    }
}

fn render_messages(app: &ChatApp) -> Text<'static> {
    let mut lines = Vec::new();
    for message in &app.messages {
        let (label, style) = match message.role {
            ChatRole::System => ("系统", Style::default().fg(Color::Blue)),
            ChatRole::User => ("你", Style::default().fg(Color::Green)),
            ChatRole::Assistant => ("DeepCode", Style::default().fg(Color::Cyan)),
            ChatRole::Error => ("错误", Style::default().fg(Color::Red)),
        };
        lines.push(Line::from(vec![Span::styled(
            format!("{label}:"),
            style.add_modifier(Modifier::BOLD),
        )]));
        for line in message.content.lines() {
            lines.push(Line::from(line.to_string()));
        }
        lines.push(Line::from(""));
    }
    Text::from(lines)
}

fn answer_question(
    question: String,
    selected_file: Option<String>,
    snapshot: ProjectSnapshot,
    config: Config,
    no_cache: bool,
    tx: &Sender<WorkerEvent>,
) -> Result<String, String> {
    send_status(tx, "正在构建中文分析问题");
    let goal = build_chat_goal(&question, selected_file.as_deref());
    let cache_enabled = config.cache_enabled && !no_cache;
    let key = cache_key(Workflow::Chat, Some(&goal), &config, &snapshot)
        .map_err(|error| error.to_string())?;
    let raw = if cache_enabled {
        send_status(tx, "正在检查缓存");
        match read_cached(&config, &key).map_err(|error| error.to_string())? {
            Some(content) => {
                send_status(tx, "命中缓存，正在解析结果");
                content
            }
            None => request_and_cache(&goal, &snapshot, &config, &key, tx)?,
        }
    } else {
        request_model(&goal, &snapshot, &config, tx)?
    };

    send_status(tx, "正在解析 DeepSeek JSON 响应");
    let mut report = parse_report(&raw).map_err(|error| error.to_string())?;
    fill_missing_structure(&mut report, &snapshot);
    Ok(format_chat_answer(&report))
}

fn build_chat_goal(question: &str, selected_file: Option<&str>) -> String {
    match selected_file {
        Some(path) => format!(
            "请始终使用中文回答。请基于提供的代码上下文回答用户问题，必要时引用相对文件路径。当前选中文件：{path}。如果用户问题没有明确限定范围，请优先分析这个文件。用户问题：{question}"
        ),
        None => format!(
            "请始终使用中文回答。请基于提供的代码上下文回答用户问题，必要时引用相对文件路径。用户问题：{question}"
        ),
    }
}

fn format_user_question(question: &str, selected_file: Option<&str>) -> String {
    match selected_file {
        Some(path) => format!("{question}\n\n当前选中文件：{path}"),
        None => question.to_string(),
    }
}

fn request_and_cache(
    goal: &str,
    snapshot: &ProjectSnapshot,
    config: &Config,
    key: &str,
    tx: &Sender<WorkerEvent>,
) -> Result<String, String> {
    send_status(tx, "缓存未命中，正在请求 DeepSeek");
    let content = request_model(goal, snapshot, config, tx)?;
    send_status(tx, "正在写入缓存");
    write_cached(config, key, &content).map_err(|error| error.to_string())?;
    Ok(content)
}

fn request_model(
    goal: &str,
    snapshot: &ProjectSnapshot,
    config: &Config,
    tx: &Sender<WorkerEvent>,
) -> Result<String, String> {
    let client = DeepSeekClient::new(config).map_err(|error| error.to_string())?;
    let messages = build_messages(Workflow::Chat, snapshot, Some(goal));
    client
        .complete_with_progress(&messages, true, |message| send_status(tx, message))
        .map_err(|error| error.to_string())
}

fn format_chat_answer(report: &AnalysisReport) -> String {
    let mut output = String::new();
    output.push_str(report.summary.trim());
    if !report.risks.is_empty() {
        output.push_str("\n\n风险：\n");
        for risk in &report.risks {
            output.push_str(&format!("- {risk}\n"));
        }
    }
    if !report.improvements.is_empty() {
        output.push_str("\n建议：\n");
        for improvement in &report.improvements {
            output.push_str(&format!(
                "- {}：{} 风险：{}\n",
                improvement.title, improvement.rationale, improvement.risk
            ));
        }
    }
    if !report.tests.is_empty() {
        output.push_str("\n验证：\n");
        for test in &report.tests {
            output.push_str(&format!("- {test}\n"));
        }
    }
    output
}

fn send_status(tx: &Sender<WorkerEvent>, message: &str) {
    let _ = tx.send(WorkerEvent::Status(message.to_string()));
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, LeaveAlternateScreen, Show);
    }
}
