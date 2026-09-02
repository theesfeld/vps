//! Native window. Lists grok PTYs, then a real terminal attaches over ssh.

mod config;
mod fonts;
mod settings;
mod terminal;

use std::process::Command;

use clap::{Parser, Subcommand};
use iced::keyboard::key::Named;
use iced::keyboard::Key;
use iced::widget::{column, container, text, Space};
use iced::{event, keyboard, window, Color, Element, Length, Size, Subscription, Task};
use vps_protocol::{decode_line, Message, SessionInfo};

use config::{AttachSpec, Config};
use terminal::Detected;

struct App {
    cfg: Config,
    title: String,
    mode: Mode,
}

enum Mode {
    Loading,
    ChooseTerm {
        found: Vec<Detected>,
        cursor: usize,
        err: Option<String>,
        /// First run (empty program). After pick, list/spawn instead of returning.
        first: bool,
        back: Option<(Vec<SessionInfo>, usize)>,
    },
    Pick {
        sessions: Vec<SessionInfo>,
        cursor: usize,
        err: Option<String>,
    },
    Settings {
        form: Box<settings::Form>,
        back: Back,
    },
}

enum Back {
    Quit,
    Pick {
        sessions: Vec<SessionInfo>,
        cursor: usize,
    },
    Choose {
        found: Vec<Detected>,
        cursor: usize,
        first: bool,
        pick: Option<(Vec<SessionInfo>, usize)>,
    },
}

#[derive(Debug, Clone)]
enum Event {
    Listed(Result<Vec<SessionInfo>, String>),
    Key(Key),
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
    /// Exec the chosen terminal onto a grok PTY (used by the picker via niri spawn).
    Attach {
        /// Reconnect to this session id.
        #[arg(long)]
        id: Option<u64>,
        /// Always create a new PTY.
        #[arg(long)]
        new: bool,
    },
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
                        back: Back::Quit,
                    },
                },
                Task::none(),
            );
        }
        if terminal::needs_chooser(&cfg) {
            return (
                Self {
                    title: format!("vps · {host} · terminal"),
                    cfg: cfg.clone(),
                    mode: Mode::ChooseTerm {
                        found: terminal::detect(),
                        cursor: 0,
                        err: terminal::missing_terminal_message(&cfg),
                        first: true,
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
            Event::Key(key) => match &self.mode {
                Mode::ChooseTerm { .. } => self.handle_choose_key(key),
                Mode::Pick { .. } => self.handle_pick_key(key),
                _ => Task::none(),
            },
            Event::Settings(msg) => self.handle_settings(msg),
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
            Key::Character(c) if c.as_str() == "t" => self.enter_chooser(false),
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

    fn handle_choose_key(&mut self, key: Key) -> Task<Event> {
        let (n, first, back) = match &self.mode {
            Mode::ChooseTerm {
                found, first, back, ..
            } => (found.len() + 1, *first, back.clone()),
            _ => return Task::none(),
        };
        match &key {
            Key::Named(Named::Escape) => {
                if first {
                    window::latest().and_then(window::close)
                } else if let Some((sessions, cur)) = back {
                    self.title = format!("vps · {}", self.cfg.ssh.host);
                    self.mode = Mode::Pick {
                        sessions,
                        cursor: cur,
                        err: None,
                    };
                    Task::none()
                } else {
                    window::latest().and_then(window::close)
                }
            }
            Key::Named(Named::ArrowUp) => {
                if let Mode::ChooseTerm { cursor, .. } = &mut self.mode {
                    *cursor = cursor.checked_sub(1).unwrap_or(n.saturating_sub(1));
                }
                Task::none()
            }
            Key::Named(Named::ArrowDown) => {
                if let Mode::ChooseTerm { cursor, .. } = &mut self.mode {
                    *cursor = (*cursor + 1) % n.max(1);
                }
                Task::none()
            }
            Key::Character(c) if c.as_str() == "k" => {
                if let Mode::ChooseTerm { cursor, .. } = &mut self.mode {
                    *cursor = cursor.checked_sub(1).unwrap_or(n.saturating_sub(1));
                }
                Task::none()
            }
            Key::Character(c) if c.as_str() == "j" => {
                if let Mode::ChooseTerm { cursor, .. } = &mut self.mode {
                    *cursor = (*cursor + 1) % n.max(1);
                }
                Task::none()
            }
            Key::Character(c) if c.as_str() == "s" => self.enter_settings(),
            Key::Named(Named::Enter) => {
                let picked = match &self.mode {
                    Mode::ChooseTerm {
                        found,
                        cursor,
                        first,
                        back,
                        ..
                    } if *cursor < found.len() => Some((
                        found[*cursor].path.display().to_string(),
                        *first,
                        back.clone(),
                    )),
                    Mode::ChooseTerm { .. } => None,
                    _ => return Task::none(),
                };
                match picked {
                    Some((program, first, back)) => self.apply_terminal(program, first, back),
                    None => self.enter_settings(),
                }
            }
            _ => Task::none(),
        }
    }

    fn apply_terminal(
        &mut self,
        program: String,
        first: bool,
        back: Option<(Vec<SessionInfo>, usize)>,
    ) -> Task<Event> {
        self.cfg.terminal.program = program;
        match self.cfg.save() {
            Ok(path) => {
                if first {
                    self.title = format!("vps · {}", self.cfg.ssh.host);
                    self.mode = Mode::Loading;
                    let cfg = self.cfg.clone();
                    Task::perform(async move { list_sessions(&cfg) }, Event::Listed)
                } else {
                    self.title = format!("vps · {}", self.cfg.ssh.host);
                    self.mode = Mode::Pick {
                        sessions: back.as_ref().map(|b| b.0.clone()).unwrap_or_default(),
                        cursor: back.as_ref().map(|b| b.1).unwrap_or(0),
                        err: Some(format!("terminal saved ({})", path.display())),
                    };
                    Task::none()
                }
            }
            Err(e) => {
                if let Mode::ChooseTerm { err, .. } = &mut self.mode {
                    *err = Some(e);
                }
                Task::none()
            }
        }
    }

    fn enter_term(&mut self, spec: AttachSpec) -> Task<Event> {
        if terminal::needs_chooser(&self.cfg) {
            let first = !matches!(self.mode, Mode::Pick { .. });
            return self.enter_chooser(first);
        }
        match terminal::spawn_session(&self.cfg, spec) {
            Ok(()) => window::latest().and_then(window::close),
            Err(e) => {
                if terminal::needs_chooser(&self.cfg) {
                    let first = !matches!(self.mode, Mode::Pick { .. });
                    return self.enter_chooser(first);
                }
                match &mut self.mode {
                    Mode::Pick { err, .. } => *err = Some(e),
                    _ => {
                        self.mode = Mode::Pick {
                            sessions: Vec::new(),
                            cursor: 0,
                            err: Some(e),
                        };
                    }
                }
                Task::none()
            }
        }
    }

    fn enter_chooser(&mut self, first: bool) -> Task<Event> {
        let back = match &self.mode {
            Mode::Pick {
                sessions, cursor, ..
            } => Some((sessions.clone(), *cursor)),
            Mode::ChooseTerm { back, .. } => back.clone(),
            _ => None,
        };
        self.title = format!("vps · {} · terminal", self.cfg.ssh.host);
        self.mode = Mode::ChooseTerm {
            found: terminal::detect(),
            cursor: 0,
            err: terminal::missing_terminal_message(&self.cfg),
            first,
            back,
        };
        Task::none()
    }

    fn enter_settings(&mut self) -> Task<Event> {
        let back = match &self.mode {
            Mode::Pick {
                sessions, cursor, ..
            } => Back::Pick {
                sessions: sessions.clone(),
                cursor: *cursor,
            },
            Mode::ChooseTerm {
                found,
                cursor,
                first,
                back,
                ..
            } => Back::Choose {
                found: found.clone(),
                cursor: *cursor,
                first: *first,
                pick: back.clone(),
            },
            Mode::Settings { back, .. } => match back {
                Back::Quit => Back::Quit,
                Back::Pick { sessions, cursor } => Back::Pick {
                    sessions: sessions.clone(),
                    cursor: *cursor,
                },
                Back::Choose {
                    found,
                    cursor,
                    first,
                    pick,
                } => Back::Choose {
                    found: found.clone(),
                    cursor: *cursor,
                    first: *first,
                    pick: pick.clone(),
                },
            },
            Mode::Loading => Back::Quit,
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
                    back:
                        Back::Pick {
                            sessions, cursor, ..
                        },
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
                Mode::Settings {
                    back:
                        Back::Choose {
                            found,
                            cursor,
                            first,
                            pick,
                        },
                    ..
                } => {
                    self.title = format!("vps · {} · terminal", self.cfg.ssh.host);
                    self.mode = Mode::ChooseTerm {
                        found: found.clone(),
                        cursor: *cursor,
                        err: None,
                        first: *first,
                        back: pick.clone(),
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
            Mode::ChooseTerm {
                found, cursor, err, ..
            } => choose_view(&self.cfg, found, *cursor, err.as_deref()),
            Mode::Pick {
                sessions,
                cursor,
                err,
            } => pick_view(&self.cfg, sessions, *cursor, err.as_deref()),
            Mode::Settings { form, .. } => settings::view(&self.cfg, form).map(Event::Settings),
        }
    }

    fn subscription(&self) -> Subscription<Event> {
        match &self.mode {
            Mode::Pick { .. } | Mode::ChooseTerm { .. } => {
                event::listen_with(|ev, _status, _id| {
                    if let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) = ev {
                        Some(Event::Key(key))
                    } else {
                        None
                    }
                })
            }
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
        text("↑↓ / j k    enter    n new    t terminal    s settings    esc")
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

fn choose_view<'a>(
    cfg: &'a Config,
    found: &'a [Detected],
    cursor: usize,
    err: Option<&'a str>,
) -> Element<'a, Event> {
    let fg = fg(cfg);
    let mut rows = column![
        text(format!("vps · {}", cfg.ssh.host)).size(22).color(fg),
        text("choose a terminal — saved to ~/.config/vps/config.toml")
            .size(14)
            .color(dim(cfg)),
        Space::new().height(12),
    ]
    .spacing(4);

    if found.is_empty() {
        rows = rows.push(
            text("no known terminal on PATH (kitty, foot, alacritty, ghostty, wezterm)")
                .size(14)
                .color(parse_hex(&cfg.colors.red)),
        );
        rows = rows.push(Space::new().height(8));
    }
    for (i, d) in found.iter().enumerate() {
        rows = rows.push(choice_row(
            cfg,
            &format!("  {:<12} {}", d.name, d.path.display()),
            i == cursor,
        ));
    }
    rows = rows.push(choice_row(
        cfg,
        "     type a path in settings",
        cursor == found.len(),
    ));
    if let Some(err) = err {
        rows = rows.push(Space::new().height(8));
        rows = rows.push(text(err).size(14).color(parse_hex(&cfg.colors.red)));
    }
    rows = rows.push(Space::new().height(16));
    rows = rows.push(
        text("↑↓ / j k    enter    s settings    esc")
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
    let cmd = session_command_label(s);
    let cwd = if s.cwd.is_empty() {
        String::new()
    } else {
        truncate(&s.cwd, 40)
    };
    let line = format!("#{:<3} {state:<5}  {cwd}  {cmd}", s.id);
    choice_row(cfg, &line, selected)
}

fn session_command_label(s: &SessionInfo) -> String {
    if s.command.is_empty() {
        return "bash".into();
    }
    if !s.title.is_empty() {
        let base = s
            .command
            .split_whitespace()
            .next()
            .unwrap_or("grok")
            .rsplit('/')
            .next()
            .unwrap_or("grok");
        return truncate(&format!("{base} [{}]", s.title), 56);
    }
    truncate(&s.command, 48)
}

fn session_row_settings(cfg: &Config, selected: bool) -> Element<'static, Event> {
    choice_row(cfg, "     settings", selected)
}

fn session_row_new(cfg: &Config, selected: bool) -> Element<'static, Event> {
    choice_row(cfg, "+    new session", selected)
}

fn choice_row(cfg: &Config, line: &str, selected: bool) -> Element<'static, Event> {
    let fg = if selected {
        parse_hex(&cfg.colors.foreground)
    } else {
        dim(cfg)
    };
    let sel_bg = parse_hex(&cfg.colors.bright_black);
    let t = text(line.to_string()).size(16).color(fg);
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
    if let Some(Cmd::Attach { id, new }) = cli.cmd {
        attach_cli(id, new);
    }
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

fn attach_cli(id: Option<u64>, new: bool) -> ! {
    if new && id.is_some() {
        eprintln!("vps: --new and --id cannot both be set");
        std::process::exit(1);
    }
    let spec = match id {
        Some(id) => AttachSpec::Id(id),
        None => AttachSpec::New,
    };
    let cfg = Config::load();
    if terminal::needs_chooser(&cfg) {
        eprintln!("vps: no terminal chosen — run vps and pick one");
        std::process::exit(1);
    }
    terminal::exec_attach(&cfg, spec);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grok_title_in_row() {
        let s = SessionInfo {
            id: 1,
            pts: "/dev/pts/1".into(),
            pid: 1,
            attached: false,
            cwd: "/home/tj".into(),
            command: "grok".into(),
            title: "FOO".into(),
        };
        assert_eq!(session_command_label(&s), "grok [FOO]");
    }

    #[test]
    fn grok_path_title() {
        let s = SessionInfo {
            id: 1,
            pts: "/dev/pts/1".into(),
            pid: 1,
            attached: false,
            cwd: "/home/tj".into(),
            command: "/home/tj/.local/bin/grok --resume x".into(),
            title: "VPS".into(),
        };
        assert_eq!(session_command_label(&s), "grok [VPS]");
    }

    #[test]
    fn empty_command_is_bash() {
        let s = SessionInfo {
            id: 1,
            pts: "/dev/pts/1".into(),
            pid: 1,
            attached: false,
            cwd: String::new(),
            command: String::new(),
            title: String::new(),
        };
        assert_eq!(session_command_label(&s), "bash");
    }
}
