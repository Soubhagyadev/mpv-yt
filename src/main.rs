use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear as WidgetClear, List, ListItem, ListState, Paragraph, Wrap},
};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{self, Write};
use std::process::Command;

const RESULTS_PER_PAGE: usize = 10;

#[derive(Debug, Clone, Deserialize)]
struct Video {
    title: Option<String>,
    channel: Option<String>,
    duration: Option<f64>,
    view_count: Option<u64>,
    url: Option<String>,
    id: Option<String>,
    #[serde(rename = "webpage_url")]
    webpage_url: Option<String>,
}

impl Video {
    fn display_title(&self) -> &str {
        self.title.as_deref().unwrap_or("Unknown Title")
    }

    fn display_channel(&self) -> String {
        self.channel.clone().unwrap_or_else(|| "Unknown".into())
    }

    fn display_duration(&self) -> String {
        match self.duration {
            Some(secs) => {
                let s = secs as u64;
                let h = s / 3600;
                let m = (s % 3600) / 60;
                let sec = s % 60;
                if h > 0 {
                    format!("{h}:{m:02}:{sec:02}")
                } else {
                    format!("{m}:{sec:02}")
                }
            }
            None => "??:??".into(),
        }
    }

    fn display_views(&self) -> String {
        match self.view_count {
            Some(v) if v >= 1_000_000_000 => format!("{:.1}B views", v as f64 / 1e9),
            Some(v) if v >= 1_000_000 => format!("{:.1}M views", v as f64 / 1e6),
            Some(v) if v >= 1_000 => format!("{:.1}K views", v as f64 / 1e3),
            Some(v) => format!("{v} views"),
            None => "? views".into(),
        }
    }

    fn playback_url(&self) -> String {
        if let Some(url) = &self.webpage_url {
            url.clone()
        } else if let Some(id) = &self.id {
            format!("https://www.youtube.com/watch?v={id}")
        } else if let Some(url) = &self.url {
            if url.starts_with("http") {
                url.clone()
            } else {
                format!("https://www.youtube.com/watch?v={url}")
            }
        } else {
            String::new()
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum SortOrder {
    Default,
    ViewsDesc,
    ViewsAsc,
    DurationDesc,
    DurationAsc,
}

impl SortOrder {
    fn label(&self) -> &str {
        match self {
            SortOrder::Default => "default",
            SortOrder::ViewsDesc => "most views",
            SortOrder::ViewsAsc => "least views",
            SortOrder::DurationDesc => "longest",
            SortOrder::DurationAsc => "shortest",
        }
    }
}

fn parse_query_and_sort(input: &str) -> (String, SortOrder) {
    let input = input.trim();

    let filters: &[(&str, SortOrder)] = &[
        ("--sort-by-views-desc", SortOrder::ViewsDesc),
        ("--sort-by-views-asc", SortOrder::ViewsAsc),
        ("--sort-by-views", SortOrder::ViewsDesc),
        ("--sort-by-duration-desc", SortOrder::DurationDesc),
        ("--sort-by-duration-asc", SortOrder::DurationAsc),
        ("--sort-by-duration", SortOrder::DurationDesc),
        ("--most-viewed", SortOrder::ViewsDesc),
        ("--least-viewed", SortOrder::ViewsAsc),
        ("--longest", SortOrder::DurationDesc),
        ("--shortest", SortOrder::DurationAsc),
    ];

    for (flag, order) in filters {
        if let Some(pos) = input.find(flag) {
            let mut query = String::new();
            query.push_str(input[..pos].trim());
            let after = input[pos + flag.len()..].trim();
            if !after.is_empty() {
                if !query.is_empty() {
                    query.push(' ');
                }
                query.push_str(after);
            }
            return (query, order.clone());
        }
    }

    (input.to_string(), SortOrder::Default)
}

fn sort_videos(videos: &mut [Video], order: &SortOrder) {
    match order {
        SortOrder::Default => {}
        SortOrder::ViewsDesc => videos.sort_by(|a, b| b.view_count.cmp(&a.view_count)),
        SortOrder::ViewsAsc => videos.sort_by(|a, b| a.view_count.cmp(&b.view_count)),
        SortOrder::DurationDesc => {
            videos.sort_by(|a, b| {
                b.duration
                    .unwrap_or(0.0)
                    .partial_cmp(&a.duration.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        SortOrder::DurationAsc => {
            videos.sort_by(|a, b| {
                a.duration
                    .unwrap_or(f64::MAX)
                    .partial_cmp(&b.duration.unwrap_or(f64::MAX))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
    }
}

fn search_youtube(query: &str, page: usize) -> Result<Vec<Video>> {
    let count = RESULTS_PER_PAGE;
    let offset = page * RESULTS_PER_PAGE;
    let search_count = offset + count;
    let search_term = format!("ytsearch{search_count}:{query}");

    let output = Command::new("yt-dlp")
        .args([
            "--dump-json",
            "--flat-playlist",
            "--no-warnings",
            "--skip-download",
            &search_term,
        ])
        .output()
        .context("Failed to run yt-dlp. Is it installed and in PATH?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("yt-dlp error: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let videos: Vec<Video> = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<Video>(line).ok())
        .collect();

    Ok(videos.into_iter().skip(offset).take(count).collect())
}

enum InputMode {
    Normal,
    Search,
    Help,
}

struct App {
    query: String,
    input_buf: String,
    videos: Vec<Video>,
    list_state: ListState,
    page: usize,
    input_mode: InputMode,
    message: String,
    should_quit: bool,
    sort_order: SortOrder,
    page_cache: HashMap<usize, Vec<Video>>,
}

impl App {
    fn new() -> Self {
        Self {
            query: String::new(),
            input_buf: String::new(),
            videos: Vec::new(),
            list_state: ListState::default(),
            page: 0,
            input_mode: InputMode::Search,
            message: String::from("Type a search query and press Enter (? for help)"),
            should_quit: false,
            sort_order: SortOrder::Default,
            page_cache: HashMap::new(),
        }
    }

    fn do_search(&mut self) {
        self.message = format!("Searching \"{}\" (page {})...", self.query, self.page + 1);
    }

    fn load_page(&mut self, page: usize) -> Result<bool> {
        if let Some(cached) = self.page_cache.get(&page) {
            self.videos = cached.clone();
            self.page = page;
            self.list_state.select(if self.videos.is_empty() {
                None
            } else {
                Some(0)
            });
            self.message = format!("Page {} -- {} results (cached)", page + 1, self.videos.len());
            return Ok(!self.videos.is_empty());
        }

        let mut videos = search_youtube(&self.query, page)?;
        if videos.is_empty() {
            return Ok(false);
        }

        sort_videos(&mut videos, &self.sort_order);
        self.page_cache.insert(page, videos.clone());
        self.videos = videos;
        self.page = page;
        self.list_state.select(Some(0));
        self.message = format!("Page {} -- {} results", page + 1, self.videos.len());
        Ok(true)
    }

    fn clear_cache(&mut self) {
        self.page_cache.clear();
    }

    fn select_next(&mut self) {
        if self.videos.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => (i + 1).min(self.videos.len() - 1),
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn select_prev(&mut self) {
        if self.videos.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn selected_video(&self) -> Option<&Video> {
        self.list_state.selected().and_then(|i| self.videos.get(i))
    }
}

fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(f.area());

    let sort_label = if app.sort_order != SortOrder::Default {
        format!(" [sorted: {}]", app.sort_order.label())
    } else {
        String::new()
    };

    let header_text = match app.input_mode {
        InputMode::Search | InputMode::Help => {
            format!(" Search: {}|", app.input_buf)
        }
        InputMode::Normal => {
            format!(
                " \"{}\"  --  Page {}{}",
                app.query,
                app.page + 1,
                sort_label
            )
        }
    };
    let header = Paragraph::new(header_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta))
            .title(" mpv-yt ")
            .title_style(Style::default().fg(Color::Cyan).bold()),
    );
    f.render_widget(header, chunks[0]);

    let items: Vec<ListItem> = app
        .videos
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let idx = i + 1 + (app.page * RESULTS_PER_PAGE);
            let line = Line::from(vec![
                Span::styled(format!(" {idx:>2}. "), Style::default().fg(Color::DarkGray)),
                Span::styled(
                    truncate_str(v.display_title(), 55),
                    Style::default().fg(Color::White).bold(),
                ),
                Span::raw("  "),
                Span::styled(v.display_channel(), Style::default().fg(Color::Yellow)),
                Span::raw("  "),
                Span::styled(v.display_duration(), Style::default().fg(Color::Green)),
                Span::raw("  "),
                Span::styled(v.display_views(), Style::default().fg(Color::Cyan)),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Videos ")
                .title_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(40, 40, 60))
                .fg(Color::White)
                .bold(),
        )
        .highlight_symbol("> ");

    f.render_stateful_widget(list, chunks[1], &mut app.list_state);

    let footer_text = match app.input_mode {
        InputMode::Search => " Enter: search | Esc: cancel | ?: filter help ".to_string(),
        InputMode::Help => " Press any key to close help ".to_string(),
        InputMode::Normal => {
            format!(
                " {} | j/k: navigate  Enter: play  n/p: page  /: search  ?: help  q: quit",
                app.message
            )
        }
    };
    let footer = Paragraph::new(footer_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(footer, chunks[2]);

    if matches!(app.input_mode, InputMode::Help) {
        render_help_popup(f);
    }
}

fn render_help_popup(f: &mut Frame) {
    let area = f.area();
    let popup_width = 60u16.min(area.width.saturating_sub(4));
    let popup_height = 15u16.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    f.render_widget(WidgetClear, popup_area);

    let help_text = vec![
        Line::from(Span::styled(
            " Search Filters",
            Style::default().fg(Color::Cyan).bold(),
        )),
        Line::from(""),
        Line::from(" Add a flag after your search query:"),
        Line::from(""),
        Line::from(vec![
            Span::styled("   --sort-by-views      ", Style::default().fg(Color::Green)),
            Span::raw("most viewed first"),
        ]),
        Line::from(vec![
            Span::styled("   --sort-by-views-asc  ", Style::default().fg(Color::Green)),
            Span::raw("least viewed first"),
        ]),
        Line::from(vec![
            Span::styled("   --sort-by-duration   ", Style::default().fg(Color::Green)),
            Span::raw("longest first"),
        ]),
        Line::from(vec![
            Span::styled("   --sort-by-duration-asc", Style::default().fg(Color::Green)),
            Span::raw(" shortest first"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " Shortcuts",
            Style::default().fg(Color::Cyan).bold(),
        )),
        Line::from(vec![
            Span::styled("   --most-viewed  ", Style::default().fg(Color::Yellow)),
            Span::styled("  --longest", Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("   --least-viewed ", Style::default().fg(Color::Yellow)),
            Span::styled("  --shortest", Style::default().fg(Color::Yellow)),
        ]),
        Line::from(""),
        Line::from(" Example: rust tutorial --sort-by-views"),
        Line::from(""),
        Line::from(Span::styled(
            " Press any key to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let help = Paragraph::new(help_text).wrap(Wrap { trim: false }).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta))
            .title(" Filter Help ")
            .title_style(Style::default().fg(Color::Cyan).bold()),
    );
    f.render_widget(help, popup_area);
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() > max_len {
        let truncated: String = s.chars().take(max_len - 1).collect();
        format!("{truncated}...")
    } else {
        s.to_string()
    }
}

fn play_video(url: &str) -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;

    let status = Command::new("mpv")
        .arg(url)
        .status()
        .context("Failed to launch mpv. Is it installed and in PATH?")?;

    if !status.success() {
        eprintln!("mpv exited with status: {status}");
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, Clear(ClearType::All))?;
    stdout.flush()?;
    Ok(())
}

fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match app.input_mode {
                InputMode::Help => {
                    app.input_mode = if app.query.is_empty() {
                        InputMode::Search
                    } else {
                        InputMode::Normal
                    };
                }
                InputMode::Search => match key.code {
                    KeyCode::Enter => {
                        if !app.input_buf.trim().is_empty() {
                            let raw_input = app
                                .input_buf
                                .drain(..)
                                .collect::<String>();
                            let (query, sort) = parse_query_and_sort(&raw_input);
                            app.query = query;
                            app.sort_order = sort;
                            app.page = 0;
                            app.clear_cache();
                            app.input_mode = InputMode::Normal;
                            app.do_search();

                            terminal.draw(|f| ui(f, &mut app))?;

                            match app.load_page(0) {
                                Ok(true) => {}
                                Ok(false) => {
                                    app.message = "No results found.".into();
                                    app.videos.clear();
                                    app.list_state.select(None);
                                }
                                Err(e) => {
                                    app.message = format!("Error: {e}");
                                    app.videos.clear();
                                }
                            }
                        }
                    }
                    KeyCode::Esc => {
                        if app.query.is_empty() {
                            app.should_quit = true;
                        } else {
                            app.input_buf.clear();
                            app.input_mode = InputMode::Normal;
                        }
                    }
                    KeyCode::Char('?') => {
                        app.input_mode = InputMode::Help;
                    }
                    KeyCode::Char(c) => {
                        app.input_buf.push(c);
                    }
                    KeyCode::Backspace => {
                        app.input_buf.pop();
                    }
                    _ => {}
                },
                InputMode::Normal => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        app.should_quit = true;
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        app.select_next();
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        app.select_prev();
                    }
                    KeyCode::Char('/') => {
                        app.input_mode = InputMode::Search;
                        app.input_buf.clear();
                    }
                    KeyCode::Char('?') => {
                        app.input_mode = InputMode::Help;
                    }
                    KeyCode::Char('n') => {
                        let next = app.page + 1;
                        app.do_search();
                        terminal.draw(|f| ui(f, &mut app))?;

                        match app.load_page(next) {
                            Ok(true) => {}
                            Ok(false) => {
                                app.message = "No more results.".into();
                            }
                            Err(e) => {
                                app.message = format!("Error: {e}");
                            }
                        }
                    }
                    KeyCode::Char('p') => {
                        if app.page > 0 {
                            let prev = app.page - 1;
                            match app.load_page(prev) {
                                Ok(_) => {}
                                Err(e) => {
                                    app.message = format!("Error: {e}");
                                }
                            }
                        } else {
                            app.message = "Already on first page.".into();
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(video) = app.selected_video() {
                            let url = video.playback_url();
                            let title = video.display_title().to_string();
                            if !url.is_empty() {
                                app.message = format!("Playing: {title}");
                                terminal.draw(|f| ui(f, &mut app))?;
                                if let Err(e) = play_video(&url) {
                                    app.message = format!("Playback error: {e}");
                                } else {
                                    terminal.clear()?;
                                    app.message = "Returned from mpv.".into();
                                }
                            } else {
                                app.message = "No URL available for this video.".into();
                            }
                        }
                    }
                    _ => {}
                },
            }
        }

        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}
