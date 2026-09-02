//! Native window. Lists grok PTYs, then `ssh -tt` attaches to one.

mod config;

use std::process::Command;

use iced::keyboard::key::Named;
use iced::keyboard::Key;
use iced::widget::{column, container, text, Space};
use iced::{event, keyboard, window, Color, Element, Length, Size, Subscription, Task};
use iced_term::Terminal;
use vps_protocol::{decode_line, Message, SessionInfo};

use config::{AttachSpec, Config};

struct App {
    cfg: Config,
    title: String,
    mode: Mode,
}

enum Mode {
    Loading,
    Pick {
        sessions: Vec<SessionInfo>,
        cursor: usize,
        err: Option<String>,
    },
    Term {
        term: Box<Terminal>,
    },
}

#[derive(Debug, Clone)]
enum Event {
    Listed(Result<Vec<SessionInfo>, String>),
    Key(Key),
    Terminal(iced_term::Event),
}

impl App {
    fn new() -> (Self, Task<Event>) {
        let cfg = Config::load();
        let host = cfg.ssh.host.clone();
        (
            Self {
                title: format!("vps · {host}"),
                cfg: cfg.clone(),
                mode: Mode::Loading,
            },
            Task::perform(async move { list_sessions(&cfg) }, Event::Listed),
        )
    }

    fn title(&self) -> String {
        self.title.clone()
    }

    fn update(&mut self, event: Event) -> Task<Event> {
        match event {
            Event::Listed(Ok(sessions)) => {
                if self.cfg.want_picker(sessions.len()) {
                    self.mode = Mode::Pick {
                        sessions,
                        cursor: 0,
                        err: None,
                    };
                    Task::none()
                } else {
                    self.enter_term(AttachSpec::New)
                }
            }
            Event::Listed(Err(err)) => {
                self.mode = Mode::Pick {
                    sessions: Vec::new(),
                    cursor: 0,
                    err: Some(err),
                };
                Task::none()
            }
            Event::Key(key) => self.handle_pick_key(key),
            Event::Terminal(iced_term::Event::BackendCall(_, cmd)) => {
                if let Mode::Term { term } = &mut self.mode {
                    match term.handle(iced_term::Command::ProxyToBackend(cmd)) {
                        iced_term::actions::Action::Shutdown => {
                            return window::latest().and_then(window::close);
                        }
                        iced_term::actions::Action::ChangeTitle(title) => {
                            self.title = title;
                        }
                        _ => {}
                    }
                }
                Task::none()
            }
        }
    }

    fn handle_pick_key(&mut self, key: Key) -> Task<Event> {
        let Mode::Pick {
            sessions,
            cursor,
            err: _,
        } = &mut self.mode
        else {
            return Task::none();
        };
        let n = sessions.len() + 1; // last row is New
        match &key {
            Key::Named(Named::Escape) => window::latest().and_then(window::close),
            Key::Named(Named::ArrowUp) => {
                *cursor = cursor.checked_sub(1).unwrap_or(n.saturating_sub(1));
                Task::none()
            }
            Key::Named(Named::ArrowDown) => {
                *cursor = (*cursor + 1) % n.max(1);
                Task::none()
            }
            Key::Character(c) if c.as_str() == "k" => {
                *cursor = cursor.checked_sub(1).unwrap_or(n.saturating_sub(1));
                Task::none()
            }
            Key::Character(c) if c.as_str() == "j" => {
                *cursor = (*cursor + 1) % n.max(1);
                Task::none()
            }
            Key::Character(c) if c.as_str() == "n" => self.enter_term(AttachSpec::New),
            Key::Named(Named::Enter) => {
                if *cursor >= sessions.len() {
                    self.enter_term(AttachSpec::New)
                } else {
                    let s = sessions[*cursor].clone();
                    if s.attached {
                        if let Mode::Pick { err, .. } = &mut self.mode {
                            *err = Some(format!("session {} is already in another window", s.id));
                        }
                        Task::none()
                    } else {
                        self.enter_term(AttachSpec::Id(s.id))
                    }
                }
            }
            _ => Task::none(),
        }
    }

    fn enter_term(&mut self, spec: AttachSpec) -> Task<Event> {
        let settings = iced_term::settings::Settings {
            font: iced_term::settings::FontSettings {
                size: self.cfg.font.size,
                scale_factor: self.cfg.font.scale,
                font_type: self.cfg.iced_font(),
            },
            theme: iced_term::settings::ThemeSettings::new(Box::new(self.cfg.palette())),
            backend: iced_term::settings::BackendSettings {
                program: "ssh".into(),
                args: self.cfg.ssh_attach_argv(spec),
                env: self.cfg.term_env(),
                working_directory: None,
            },
        };
        let term = match Terminal::new(0, settings) {
            Ok(t) => t,
            Err(e) => {
                self.mode = Mode::Pick {
                    sessions: Vec::new(),
                    cursor: 0,
                    err: Some(e.to_string()),
                };
                return Task::none();
            }
        };
        let focus = iced_term::TerminalView::focus(term.widget_id().clone());
        self.title = match spec {
            AttachSpec::New => format!("vps · {}", self.cfg.ssh.host),
            AttachSpec::Id(id) => format!("vps · {} · #{id}", self.cfg.ssh.host),
        };
        self.mode = Mode::Term {
            term: Box::new(term),
        };
        Task::batch([focus, window::latest().and_then(window::gain_focus)])
    }

    fn view(&self) -> Element<'_, Event> {
        match &self.mode {
            Mode::Loading => container(text("listing sessions…").color(fg(&self.cfg)))
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(24)
                .style(|_| pane_style(&self.cfg))
                .into(),
            Mode::Pick {
                sessions,
                cursor,
                err,
            } => pick_view(&self.cfg, sessions, *cursor, err.as_deref()),
            Mode::Term { term } => {
                container(iced_term::TerminalView::show(term).map(Event::Terminal))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            }
        }
    }

    fn subscription(&self) -> Subscription<Event> {
        match &self.mode {
            Mode::Term { term } => term.subscription().map(Event::Terminal),
            Mode::Pick { .. } => event::listen_with(|ev, _status, _id| {
                if let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) = ev {
                    Some(Event::Key(key))
                } else {
                    None
                }
            }),
            Mode::Loading => Subscription::none(),
        }
    }
}

fn pick_view<'a>(
    cfg: &'a Config,
    sessions: &'a [SessionInfo],
    cursor: usize,
    err: Option<&'a str>,
) -> Element<'a, Event> {
    let fg = fg(cfg);
    let mut rows = column![
        text(format!("vps · {}", cfg.ssh.host)).size(22).color(fg),
        text("reconnect to a PTY, or start a new one")
            .size(14)
            .color(dim(cfg)),
        Space::new().height(12),
    ]
    .spacing(4);

    for (i, s) in sessions.iter().enumerate() {
        rows = rows.push(session_row(cfg, s, i == cursor));
    }
    rows = rows.push(session_row_new(cfg, cursor == sessions.len()));
    if let Some(err) = err {
        rows = rows.push(Space::new().height(8));
        rows = rows.push(text(err).size(14).color(parse_hex(&cfg.colors.red)));
    }
    rows = rows.push(Space::new().height(16));
    rows = rows.push(
        text("↑↓ / j k    enter    n new    esc")
            .size(13)
            .color(dim(cfg)),
    );

    container(rows)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(24)
        .style(move |_| pane_style(cfg))
        .into()
}

fn session_row(cfg: &Config, s: &SessionInfo, selected: bool) -> Element<'static, Event> {
    let state = if s.attached { "live" } else { "idle" };
    let cmd = if s.command.is_empty() {
        "bash".into()
    } else {
        truncate(&s.command, 48)
    };
    let cwd = if s.cwd.is_empty() {
        String::new()
    } else {
        truncate(&s.cwd, 40)
    };
    let line = format!("#{:<3} {state:<5}  {cwd}  {cmd}", s.id);
    let fg = if selected {
        parse_hex(&cfg.colors.foreground)
    } else {
        dim(cfg)
    };
    let sel_bg = parse_hex(&cfg.colors.bright_black);
    let t = text(line).size(16).color(fg);
    container(t)
        .width(Length::Fill)
        .padding(6)
        .style(move |_| {
            if selected {
                iced::widget::container::Style {
                    background: Some(iced::Background::Color(sel_bg)),
                    ..Default::default()
                }
            } else {
                iced::widget::container::Style::default()
            }
        })
        .into()
}

fn session_row_new(cfg: &Config, selected: bool) -> Element<'static, Event> {
    let fg = if selected {
        parse_hex(&cfg.colors.foreground)
    } else {
        dim(cfg)
    };
    let sel_bg = parse_hex(&cfg.colors.bright_black);
    let t = text("+    new session").size(16).color(fg);
    container(t)
        .width(Length::Fill)
        .padding(6)
        .style(move |_| {
            if selected {
                iced::widget::container::Style {
                    background: Some(iced::Background::Color(sel_bg)),
                    ..Default::default()
                }
            } else {
                iced::widget::container::Style::default()
            }
        })
        .into()
}

fn pane_style(cfg: &Config) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(parse_hex(&cfg.colors.background))),
        text_color: Some(parse_hex(&cfg.colors.foreground)),
        ..Default::default()
    }
}

fn fg(cfg: &Config) -> Color {
    parse_hex(&cfg.colors.foreground)
}

fn dim(cfg: &Config) -> Color {
    parse_hex(&cfg.colors.dim_foreground)
}

fn parse_hex(s: &str) -> Color {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return Color::WHITE;
    }
    let Ok(n) = u32::from_str_radix(s, 16) else {
        return Color::WHITE;
    };
    Color::from_rgb8(
        ((n >> 16) & 0xff) as u8,
        ((n >> 8) & 0xff) as u8,
        (n & 0xff) as u8,
    )
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{t}…")
    }
}

fn list_sessions(cfg: &Config) -> Result<Vec<SessionInfo>, String> {
    let argv = cfg.ssh_list_argv();
    let out = Command::new("ssh")
        .args(&argv)
        .output()
        .map_err(|e| format!("ssh list: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("ssh list failed ({:?}): {err}", out.status.code()));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout
        .lines()
        .find(|l| l.contains("\"t\""))
        .unwrap_or(stdout.trim());
    match decode_line(line) {
        Ok(Message::Sessions { sessions }) => Ok(sessions),
        Ok(other) => Err(format!("unexpected list reply: {other:?}")),
        Err(e) => Err(format!("list json: {e}: {line}")),
    }
}

fn main() -> iced::Result {
    let cfg = Config::load();
    let window = window::Settings {
        size: Size::new(cfg.window.width, cfg.window.height),
        platform_specific: window::settings::PlatformSpecific {
            application_id: cfg.window.app_id.clone(),
            ..Default::default()
        },
        ..Default::default()
    };

    iced::application(App::new, App::update, App::view)
        .title(App::title)
        .window(window)
        .subscription(App::subscription)
        .run()
}
