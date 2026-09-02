//! Native window. Lists grok PTYs, then `ssh -tt` attaches to one.

mod config;
mod fonts;
mod settings;

use std::process::Command;

use clap::{Parser, Subcommand};
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
    Settings {
        form: Box<settings::Form>,
        /// `None` means this process was `vps settings` — cancel closes.
        back: Option<(Vec<SessionInfo>, usize)>,
    },
}

#[derive(Debug, Clone)]
enum Event {
    Listed(Result<Vec<SessionInfo>, String>),
    Key(Key),
    Terminal(iced_term::Event),
    Settings(settings::Msg),
}

#[derive(Parser, Debug)]
#[command(name = "vps", about = "Native window onto a grok PTY")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Edit ~/.config/vps/config.toml
    Settings,
}

impl App {
    fn boot(open_settings: bool) -> (Self, Task<Event>) {
        let cfg = Config::load();
        let host = cfg.ssh.host.clone();
        if open_settings {
            return (
                Self {
                    title: format!("vps · {host} · settings"),
                    cfg: cfg.clone(),
                    mode: Mode::Settings {
                        form: Box::new(settings::Form::from_cfg(&cfg)),
                        back: None,
                    },
                },
                Task::none(),
            );
        }
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
            Event::Settings(msg) => self.handle_settings(msg),
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
        let n = sessions.len() + 2; // new, then settings
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
            Key::Character(c) if c.as_str() == "s" => self.enter_settings(),
            Key::Named(Named::Enter) => {
                if *cursor == sessions.len() {
                    self.enter_term(AttachSpec::New)
                } else if *cursor > sessions.len() {
                    self.enter_settings()
                } else {
                    let s = sessions[*cursor].clone();
                    self.enter_term(AttachSpec::Id(s.id))
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

    fn enter_settings(&mut self) -> Task<Event> {
        let back = match &self.mode {
            Mode::Pick {
                sessions, cursor, ..
            } => Some((sessions.clone(), *cursor)),
            _ => None,
        };
        self.title = format!("vps · {} · settings", self.cfg.ssh.host);
        self.mode = Mode::Settings {
            form: Box::new(settings::Form::from_cfg(&self.cfg)),
            back,
        };
        Task::none()
    }

    fn handle_settings(&mut self, msg: settings::Msg) -> Task<Event> {
        match msg {
            settings::Msg::Save => {
                let Mode::Settings { form, .. } = &self.mode else {
                    return Task::none();
                };
                match form.apply() {
                    Ok(cfg) => match cfg.save() {
                        Ok(path) => {
                            self.cfg = cfg;
                            if let Mode::Settings { form, .. } = &mut self.mode {
                                form.status = Some(format!("saved {path}", path = path.display()));
                            }
                        }
                        Err(e) => {
                            if let Mode::Settings { form, .. } = &mut self.mode {
                                form.status = Some(e);
                            }
                        }
                    },
                    Err(e) => {
                        if let Mode::Settings { form, .. } = &mut self.mode {
                            form.status = Some(e);
                        }
                    }
                }
                Task::none()
            }
            settings::Msg::Cancel => match &self.mode {
                Mode::Settings {
                    back: Some((sessions, cursor)),
                    ..
                } => {
                    self.title = format!("vps · {}", self.cfg.ssh.host);
                    self.mode = Mode::Pick {
                        sessions: sessions.clone(),
                        cursor: *cursor,
                        err: None,
                    };
                    Task::none()
                }
                _ => window::latest().and_then(window::close),
            },
            other => {
                if let Mode::Settings { form, .. } = &mut self.mode {
                    form.update(other);
                }
                Task::none()
            }
        }
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
            Mode::Settings { form, .. } => settings::view(&self.cfg, form).map(Event::Settings),
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
            Mode::Settings { .. } => event::listen_with(|ev, _status, _id| {
                if let iced::Event::Keyboard(keyboard::Event::KeyPressed {
                    key: Key::Named(Named::Escape),
                    ..
                }) = ev
                {
                    Some(Event::Settings(settings::Msg::Cancel))
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
    rows = rows.push(session_row_settings(cfg, cursor == sessions.len() + 1));
    if let Some(err) = err {
        rows = rows.push(Space::new().height(8));
        rows = rows.push(text(err).size(14).color(parse_hex(&cfg.colors.red)));
    }
    rows = rows.push(Space::new().height(16));
    rows = rows.push(
        text("↑↓ / j k    enter    n new    s settings    esc")
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

fn session_row_settings(cfg: &Config, selected: bool) -> Element<'static, Event> {
    let fg = if selected {
        parse_hex(&cfg.colors.foreground)
    } else {
        dim(cfg)
    };
    let sel_bg = parse_hex(&cfg.colors.bright_black);
    let t = text("     settings").size(16).color(fg);
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

pub(crate) fn pane_style(cfg: &Config) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(parse_hex(&cfg.colors.background))),
        text_color: Some(parse_hex(&cfg.colors.foreground)),
        ..Default::default()
    }
}

pub(crate) fn fg(cfg: &Config) -> Color {
    parse_hex(&cfg.colors.foreground)
}

pub(crate) fn dim(cfg: &Config) -> Color {
    parse_hex(&cfg.colors.dim_foreground)
}

pub(crate) fn parse_hex(s: &str) -> Color {
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
    let cli = Cli::parse();
    let open_settings = matches!(cli.cmd, Some(Cmd::Settings));
    let cfg = Config::load();
    let window = window::Settings {
        size: Size::new(cfg.window.width, cfg.window.height),
        platform_specific: window::settings::PlatformSpecific {
            application_id: cfg.window.app_id.clone(),
            ..Default::default()
        },
        ..Default::default()
    };

    let loaded = fonts::load(&cfg.font);
    let mut app = iced::application(move || App::boot(open_settings), App::update, App::view)
        .title(App::title)
        .window(window)
        .default_font(iced::Font::with_name(loaded.family))
        .subscription(App::subscription);
    for face in &loaded.faces {
        app = app.font(*face);
    }
    app.run()
}
