//! In-app editor for `~/.config/vps/config.toml`.

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Alignment, Element, Length};

use crate::config::{config_path, Colors, Config};
use crate::{dim, fg, pane_style, parse_hex};

#[derive(Debug, Clone)]
pub struct Form {
    pub host: String,
    pub args: String,
    pub remote: String,
    pub list_args: String,
    pub list: String,
    pub terminal_program: String,
    pub terminal_args: String,
    pub picker_mode: String,
    pub width: String,
    pub height: String,
    pub app_id: String,
    pub family: String,
    pub size: String,
    pub scale: String,
    pub extras: String,
    pub term: String,
    pub colorterm: String,
    pub colors: Colors,
    pub status: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Host(String),
    Args(String),
    Remote(String),
    ListArgs(String),
    List(String),
    TerminalProgram(String),
    TerminalArgs(String),
    Picker(String),
    Width(String),
    Height(String),
    AppId(String),
    Family(String),
    Size(String),
    Scale(String),
    Extras(String),
    Term(String),
    Colorterm(String),
    Color(&'static str, String),
    Save,
    Cancel,
}

const PICKER_MODES: [&str; 3] = ["when_sessions", "always", "never"];

impl Form {
    pub fn from_cfg(cfg: &Config) -> Self {
        Self {
            host: cfg.ssh.host.clone(),
            args: cfg.ssh.args.join(" "),
            remote: cfg.ssh.remote.clone(),
            list_args: cfg.ssh.list_args.join(" "),
            list: cfg.ssh.list.clone(),
            terminal_program: cfg.terminal.program.clone(),
            terminal_args: cfg.terminal.args.join(" "),
            picker_mode: cfg.picker.mode.clone(),
            width: fmt_num(cfg.window.width),
            height: fmt_num(cfg.window.height),
            app_id: cfg.window.app_id.clone(),
            family: cfg.font.family.clone(),
            size: fmt_num(cfg.font.size),
            scale: fmt_num(cfg.font.scale),
            extras: cfg.font.extras.join(", "),
            term: cfg.term.term.clone(),
            colorterm: cfg.term.colorterm.clone(),
            colors: cfg.colors.clone(),
            status: Some(format!("file: {}", config_path().display())),
        }
    }

    pub fn apply(&self) -> Result<Config, String> {
        let mut cfg = Config::default();
        cfg.ssh.host = self.host.trim().to_string();
        if cfg.ssh.host.is_empty() {
            return Err("ssh.host is required".into());
        }
        cfg.ssh.args = split_ws(&self.args);
        cfg.ssh.remote = self.remote.trim().to_string();
        cfg.ssh.list_args = split_ws(&self.list_args);
        cfg.ssh.list = self.list.trim().to_string();
        cfg.terminal.program = self.terminal_program.trim().to_string();
        cfg.terminal.args = split_ws(&self.terminal_args);
        cfg.picker.mode = self.picker_mode.clone();
        if !PICKER_MODES.contains(&cfg.picker.mode.as_str()) {
            return Err("picker.mode must be when_sessions, always, or never".into());
        }
        cfg.window.width = parse_f32("window.width", &self.width)?;
        cfg.window.height = parse_f32("window.height", &self.height)?;
        cfg.window.app_id = self.app_id.trim().to_string();
        cfg.font.family = self.family.clone();
        cfg.font.size = parse_f32("font.size", &self.size)?;
        cfg.font.scale = parse_f32("font.scale", &self.scale)?;
        cfg.font.extras = self
            .extras
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        cfg.term.term = self.term.trim().to_string();
        cfg.term.colorterm = self.colorterm.trim().to_string();
        cfg.colors = self.colors.clone();
        Ok(cfg)
    }

    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Host(s) => self.host = s,
            Msg::Args(s) => self.args = s,
            Msg::Remote(s) => self.remote = s,
            Msg::ListArgs(s) => self.list_args = s,
            Msg::List(s) => self.list = s,
            Msg::TerminalProgram(s) => self.terminal_program = s,
            Msg::TerminalArgs(s) => self.terminal_args = s,
            Msg::Picker(s) => self.picker_mode = s,
            Msg::Width(s) => self.width = s,
            Msg::Height(s) => self.height = s,
            Msg::AppId(s) => self.app_id = s,
            Msg::Family(s) => self.family = s,
            Msg::Size(s) => self.size = s,
            Msg::Scale(s) => self.scale = s,
            Msg::Extras(s) => self.extras = s,
            Msg::Term(s) => self.term = s,
            Msg::Colorterm(s) => self.colorterm = s,
            Msg::Color(name, s) => set_color(&mut self.colors, name, s),
            Msg::Save | Msg::Cancel => {}
        }
    }
}

fn split_ws(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

fn parse_f32(name: &str, s: &str) -> Result<f32, String> {
    s.trim()
        .parse()
        .map_err(|_| format!("{name} must be a number"))
}

fn fmt_num(n: f32) -> String {
    if (n - n.round()).abs() < 1e-6 {
        format!("{}", n as i32)
    } else {
        n.to_string()
    }
}

fn set_color(c: &mut Colors, name: &str, s: String) {
    let slot = match name {
        "foreground" => &mut c.foreground,
        "background" => &mut c.background,
        "black" => &mut c.black,
        "red" => &mut c.red,
        "green" => &mut c.green,
        "yellow" => &mut c.yellow,
        "blue" => &mut c.blue,
        "magenta" => &mut c.magenta,
        "cyan" => &mut c.cyan,
        "white" => &mut c.white,
        "bright_black" => &mut c.bright_black,
        "bright_red" => &mut c.bright_red,
        "bright_green" => &mut c.bright_green,
        "bright_yellow" => &mut c.bright_yellow,
        "bright_blue" => &mut c.bright_blue,
        "bright_magenta" => &mut c.bright_magenta,
        "bright_cyan" => &mut c.bright_cyan,
        "bright_white" => &mut c.bright_white,
        "dim_foreground" => &mut c.dim_foreground,
        "dim_black" => &mut c.dim_black,
        "dim_red" => &mut c.dim_red,
        "dim_green" => &mut c.dim_green,
        "dim_yellow" => &mut c.dim_yellow,
        "dim_blue" => &mut c.dim_blue,
        "dim_magenta" => &mut c.dim_magenta,
        "dim_cyan" => &mut c.dim_cyan,
        "dim_white" => &mut c.dim_white,
        _ => return,
    };
    *slot = s;
}

pub fn view<'a>(cfg: &'a Config, form: &'a Form) -> Element<'a, Msg> {
    let mut col =
        column![
        text("settings").size(22).color(fg(cfg)),
        text("writes ~/.config/vps/config.toml · font family needs a restart")
            .size(13)
            .color(dim(cfg)),
        Space::new().height(12),
        heading(cfg, "ssh"),
        field("host", &form.host, Msg::Host),
        field("args", &form.args, Msg::Args),
        field("remote", &form.remote, Msg::Remote),
        field("list_args", &form.list_args, Msg::ListArgs),
        field("list", &form.list, Msg::List),
        heading(cfg, "terminal"),
        field(
            "program (empty = chooser next start; kitty/foot/alacritty/ghostty/wezterm or a path)",
            &form.terminal_program,
            Msg::TerminalProgram,
        ),
        field("args (extra argv before ssh)", &form.terminal_args, Msg::TerminalArgs),
        heading(cfg, "picker"),
        field(
            "mode (when_sessions | always | never)",
            &form.picker_mode,
            Msg::Picker,
        ),
        heading(cfg, "window"),
        field("width", &form.width, Msg::Width),
        field("height", &form.height, Msg::Height),
        field("app_id", &form.app_id, Msg::AppId),
        heading(cfg, "font"),
        field(
            "family (empty = system monospace)",
            &form.family,
            Msg::Family
        ),
        field("size", &form.size, Msg::Size),
        field("scale", &form.scale, Msg::Scale),
        field("extras (comma)", &form.extras, Msg::Extras),
        heading(cfg, "term"),
        field("TERM", &form.term, Msg::Term),
        field("COLORTERM", &form.colorterm, Msg::Colorterm),
        heading(cfg, "colors"),
    ]
        .spacing(8);

    for (name, value) in color_fields(&form.colors) {
        col = col.push(field(name, value, move |s| Msg::Color(name, s)));
    }

    if let Some(status) = &form.status {
        col = col.push(Space::new().height(8));
        col = col.push(text(status).size(13).color(dim(cfg)));
    }

    col = col.push(Space::new().height(12));
    col = col.push(
        row![
            button(text("save")).on_press(Msg::Save),
            button(text("cancel")).on_press(Msg::Cancel),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    );

    container(scrollable(col).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(24)
        .style(|_| pane_style(cfg))
        .into()
}

fn heading<'a>(cfg: &'a Config, label: &'a str) -> Element<'a, Msg> {
    column![
        Space::new().height(10),
        text(label).size(16).color(parse_hex(&cfg.colors.red)),
    ]
    .into()
}

fn field<'a>(
    label: &'a str,
    value: &'a str,
    on_input: impl Fn(String) -> Msg + 'a,
) -> Element<'a, Msg> {
    column![
        text(label).size(13),
        text_input(label, value).on_input(on_input).padding(8),
    ]
    .spacing(4)
    .into()
}

fn color_fields(c: &Colors) -> Vec<(&'static str, &str)> {
    vec![
        ("foreground", c.foreground.as_str()),
        ("background", c.background.as_str()),
        ("black", c.black.as_str()),
        ("red", c.red.as_str()),
        ("green", c.green.as_str()),
        ("yellow", c.yellow.as_str()),
        ("blue", c.blue.as_str()),
        ("magenta", c.magenta.as_str()),
        ("cyan", c.cyan.as_str()),
        ("white", c.white.as_str()),
        ("bright_black", c.bright_black.as_str()),
        ("bright_red", c.bright_red.as_str()),
        ("bright_green", c.bright_green.as_str()),
        ("bright_yellow", c.bright_yellow.as_str()),
        ("bright_blue", c.bright_blue.as_str()),
        ("bright_magenta", c.bright_magenta.as_str()),
        ("bright_cyan", c.bright_cyan.as_str()),
        ("bright_white", c.bright_white.as_str()),
        ("dim_foreground", c.dim_foreground.as_str()),
        ("dim_black", c.dim_black.as_str()),
        ("dim_red", c.dim_red.as_str()),
        ("dim_green", c.dim_green.as_str()),
        ("dim_yellow", c.dim_yellow.as_str()),
        ("dim_blue", c.dim_blue.as_str()),
        ("dim_magenta", c.dim_magenta.as_str()),
        ("dim_cyan", c.dim_cyan.as_str()),
        ("dim_white", c.dim_white.as_str()),
    ]
}
