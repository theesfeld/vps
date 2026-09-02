//! Native window. Child is `ssh -t <host> vpsd attach` so the TTY lives on
//! grok and the only path in is SSH.

use std::collections::HashMap;
use std::path::PathBuf;

use iced::widget::container;
use iced::{window, Element, Font, Length, Size, Subscription, Task};
use iced_term::{ColorPalette, Terminal};
use serde::Deserialize;

const APP_ID: &str = "vps";

#[derive(Debug, Deserialize)]
struct Config {
    #[serde(default = "default_host")]
    host: String,
    /// argv after `ssh -t <host>`. Default runs vpsd via a login shell so
    /// `~/.local/bin` is on PATH.
    #[serde(default = "default_remote")]
    remote: Vec<String>,
}

fn default_host() -> String {
    "grok".into()
}

fn default_remote() -> Vec<String> {
    // OpenSSH joins argv with spaces and runs `$SHELL -c "<joined>"`.
    vec!["/home/tj/.local/bin/vpsd attach".into()]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: default_host(),
            remote: default_remote(),
        }
    }
}

fn load_config() -> Config {
    let path = config_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Config::default();
    };
    toml::from_str(&text).unwrap_or_default()
}

fn config_path() -> PathBuf {
    let dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs_config());
    dir.join("vps").join("config.toml")
}

fn dirs_config() -> PathBuf {
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".config"))
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// Irongall, ground shifted toward red so a VPS window is obvious.
fn palette() -> ColorPalette {
    ColorPalette {
        foreground: "#ede6de".into(),
        background: "#241018".into(),
        black: "#241018".into(),
        red: "#e03818".into(),
        green: "#3d9650".into(),
        yellow: "#e0bc55".into(),
        blue: "#1e8ae8".into(),
        magenta: "#d47a82".into(),
        cyan: "#16a8b6".into(),
        white: "#ede6de".into(),
        bright_black: "#4a2830".into(),
        bright_red: "#e5563b".into(),
        bright_green: "#5aa66a".into(),
        bright_yellow: "#e5c66f".into(),
        bright_blue: "#409ceb".into(),
        bright_magenta: "#da8e95".into(),
        bright_cyan: "#39b5c1".into(),
        bright_white: "#b8c0c8".into(),
        bright_foreground: None,
        dim_foreground: "#8e7a76".into(),
        dim_black: "#14080c".into(),
        dim_red: "#8a2010".into(),
        dim_green: "#245830".into(),
        dim_yellow: "#8a7030".into(),
        dim_blue: "#145080".into(),
        dim_magenta: "#804850".into(),
        dim_cyan: "#0c6870".into(),
        dim_white: "#6e6460".into(),
    }
}

struct App {
    title: String,
    term: Terminal,
}

#[derive(Debug, Clone)]
enum Event {
    Terminal(iced_term::Event),
}

impl App {
    fn new() -> (Self, Task<Event>) {
        let cfg = load_config();
        let mut env = HashMap::new();
        env.insert("TERM".into(), "xterm-256color".into());
        env.insert("COLORTERM".into(), "truecolor".into());

        let settings = iced_term::settings::Settings {
            font: iced_term::settings::FontSettings {
                size: 22.0,
                scale_factor: 1.3,
                font_type: Font::with_name("Berkeley Mono"),
            },
            theme: iced_term::settings::ThemeSettings::new(Box::new(palette())),
            backend: iced_term::settings::BackendSettings {
                program: "ssh".into(),
                args: {
                    let mut a = vec!["-tt".into(), cfg.host.clone()];
                    a.extend(cfg.remote.clone());
                    a
                },
                env,
                working_directory: None,
            },
        };

        let term = Terminal::new(0, settings).expect("terminal");
        (
            Self {
                title: format!("vps · {}", cfg.host),
                term,
            },
            Task::none(),
        )
    }

    fn title(&self) -> String {
        self.title.clone()
    }

    fn update(&mut self, event: Event) -> Task<Event> {
        match event {
            Event::Terminal(iced_term::Event::BackendCall(_, cmd)) => {
                match self.term.handle(iced_term::Command::ProxyToBackend(cmd)) {
                    iced_term::actions::Action::Shutdown => {
                        return window::latest().and_then(window::close);
                    }
                    iced_term::actions::Action::ChangeTitle(title) => {
                        self.title = title;
                    }
                    _ => {}
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Event> {
        container(iced_term::TerminalView::show(&self.term).map(Event::Terminal))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn subscription(&self) -> Subscription<Event> {
        self.term.subscription().map(Event::Terminal)
    }
}

fn main() -> iced::Result {
    let window = window::Settings {
        platform_specific: window::settings::PlatformSpecific {
            application_id: APP_ID.into(),
            ..Default::default()
        },
        ..Default::default()
    };

    iced::application(App::new, App::update, App::view)
        .title(App::title)
        .window(window)
        .window_size(Size::new(1280.0, 800.0))
        .subscription(App::subscription)
        .run()
}
