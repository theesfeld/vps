//! Native window. Child is `ssh -tt <host> <remote>` so the TTY lives on
//! grok and the only path in is SSH.

mod config;

use iced::widget::container;
use iced::{window, Element, Length, Size, Subscription, Task};
use iced_term::Terminal;

use config::Config;

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
        let cfg = Config::load();
        let settings = iced_term::settings::Settings {
            font: iced_term::settings::FontSettings {
                size: cfg.font.size,
                scale_factor: cfg.font.scale,
                font_type: cfg.iced_font(),
            },
            theme: iced_term::settings::ThemeSettings::new(Box::new(cfg.palette())),
            backend: iced_term::settings::BackendSettings {
                program: "ssh".into(),
                args: cfg.ssh_argv(),
                env: cfg.term_env(),
                working_directory: None,
            },
        };

        let term = Terminal::new(0, settings).expect("terminal");
        (
            Self {
                title: format!("vps · {}", cfg.ssh.host),
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
