use std::io::Write as _;
use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;

use crate::config::{ConfigField, ConfigSettings};
use crate::serial::{self, SerialEvent, SerialSession};
use crate::ui;

pub use crate::serial::PortSide;

const FOCUS_ORDER: [Focus; 6] = [
    Focus::Config,
    Focus::PortLeftSelector,
    Focus::StartStop,
    Focus::PortRightSelector,
    Focus::LogCheckbox,
    Focus::LogPath,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Config,
    PortLeftSelector,
    StartStop,
    PortRightSelector,
    LogCheckbox,
    LogPath,
}

/// `focus` is "what regains input when an overlay closes" -- navigation
/// *inside* an open overlay (dropdown highlight, modal field) lives here
/// instead, since it doesn't survive the overlay closing.
#[derive(Debug, Clone, Copy)]
pub enum Overlay {
    None,
    PortDropdown { side: PortSide, highlighted: usize },
    Config { field: ConfigField },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Stopped,
    Running,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Outgoing,
    Incoming,
}

impl Direction {
    pub fn arrow(self) -> &'static str {
        match self {
            Direction::Outgoing => "->",
            Direction::Incoming => "<-",
        }
    }
}

pub struct LogLine {
    pub direction: Direction,
    pub text: String,
}

pub struct App {
    pub focus: Focus,
    pub overlay: Overlay,
    pub run_state: RunState,

    pub available_ports: Vec<String>,
    pub port_enum_error: Option<String>,
    pub port_left_selected: Option<usize>,
    pub port_right_selected: Option<usize>,

    pub config: ConfigSettings,

    pub log_lines: Vec<LogLine>,
    pub log_scroll: usize,

    pub log_enabled: bool,
    pub log_path: String,
    pub log_path_cursor: usize,
    log_file: Option<std::fs::File>,

    serial_session: Option<SerialSession>,
    pub status_message: Option<String>,

    pub should_quit: bool,
}

impl App {
    pub fn new(available_ports: Vec<String>, port_enum_error: Option<String>) -> Self {
        let log_path = String::from("C:/logs/comsniff.txt");
        let log_path_cursor = log_path.chars().count();
        Self {
            focus: Focus::Config,
            overlay: Overlay::None,
            run_state: RunState::Stopped,
            available_ports,
            port_enum_error,
            port_left_selected: None,
            port_right_selected: None,
            config: ConfigSettings::default(),
            log_lines: stub_log_lines(),
            log_scroll: 0,
            log_enabled: false,
            log_path,
            log_path_cursor,
            log_file: None,
            serial_session: None,
            status_message: None,
            should_quit: false,
        }
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
        loop {
            terminal.draw(|frame| ui::render(self, frame))?;

            if crossterm::event::poll(Duration::from_millis(100))?
                && let Event::Key(key) = crossterm::event::read()?
                && key.kind == KeyEventKind::Press
            {
                self.handle_key(key);
            }

            self.drain_serial();

            if self.should_quit {
                self.stop_session();
                return Ok(());
            }
        }
    }

    pub fn is_enabled(&self, focus: Focus) -> bool {
        match focus {
            Focus::LogPath => self.log_enabled,
            Focus::LogCheckbox => self.run_state == RunState::Stopped,
            _ => true,
        }
    }

    pub fn next_focus(&mut self) {
        let start = FOCUS_ORDER.iter().position(|f| *f == self.focus).unwrap();
        for step in 1..=FOCUS_ORDER.len() {
            let candidate = FOCUS_ORDER[(start + step) % FOCUS_ORDER.len()];
            if self.is_enabled(candidate) {
                self.focus = candidate;
                return;
            }
        }
    }

    pub fn prev_focus(&mut self) {
        let start = FOCUS_ORDER.iter().position(|f| *f == self.focus).unwrap();
        let len = FOCUS_ORDER.len();
        for step in 1..=len {
            let candidate = FOCUS_ORDER[(start + len - step) % len];
            if self.is_enabled(candidate) {
                self.focus = candidate;
                return;
            }
        }
    }

    /// Moves focus per the spatial arrow-key map. If the natural target is
    /// disabled, lands there anyway then walks forward/backward through the
    /// normal Tab order (reusing next_focus/prev_focus) until an enabled
    /// widget is found, so arrow keys never strand focus on a dead widget.
    fn move_focus_by_arrow(&mut self, key: KeyCode) {
        let Some(target) = arrow_target(self.focus, key) else { return };
        self.focus = target;
        if !self.is_enabled(target) {
            match key {
                KeyCode::Left | KeyCode::Up => self.prev_focus(),
                _ => self.next_focus(),
            }
        }
    }

    /// Not reachable through today's controls (Start/Stop requires focus to
    /// already be on itself, not the checkbox), but cheap insurance against
    /// future features that could flip `run_state` some other way.
    fn nudge_off_log_checkbox_if_running(&mut self) {
        if self.run_state == RunState::Running && self.focus == Focus::LogCheckbox {
            self.next_focus();
        }
    }

    pub fn toggle_run_state(&mut self) {
        match self.run_state {
            RunState::Stopped => self.start_session(),
            RunState::Running => self.stop_session(),
        }
    }

    fn start_session(&mut self) {
        let (Some(left_idx), Some(right_idx)) = (self.port_left_selected, self.port_right_selected) else {
            self.status_message = Some("Select both ports first".to_string());
            return;
        };
        let left_name = self.available_ports[left_idx].clone();
        let right_name = self.available_ports[right_idx].clone();

        let log_file = if self.log_enabled {
            match open_log_file(&self.log_path, &left_name, &right_name, &self.config) {
                Ok(file) => Some(file),
                Err(e) => {
                    self.status_message = Some(format!("could not open log file: {e}"));
                    return;
                }
            }
        } else {
            None
        };

        match SerialSession::start(&left_name, &right_name, &self.config) {
            Ok(session) => {
                self.serial_session = Some(session);
                self.log_file = log_file;
                self.run_state = RunState::Running;
                self.status_message = None;
                self.nudge_off_log_checkbox_if_running();
            }
            Err(e) => {
                self.status_message = Some(format!("could not open ports: {e}"));
            }
        }
    }

    fn stop_session(&mut self) {
        if let Some(session) = self.serial_session.take() {
            session.stop();
        }
        self.log_file = None;
        self.run_state = RunState::Stopped;
    }

    fn handle_serial_error(&mut self, side: PortSide, message: String) {
        self.stop_session();
        let which = match side {
            PortSide::Left => "left",
            PortSide::Right => "right",
        };
        self.status_message = Some(format!("{which} port error: {message} (stopped)"));
    }

    /// Drains everything the forwarder threads have sent since the last
    /// tick. Collects into a Vec first so the receiver isn't borrowed across
    /// the &mut self calls that follow.
    fn drain_serial(&mut self) {
        let Some(session) = &self.serial_session else { return };
        let events: Vec<SerialEvent> = session.rx.try_iter().collect();
        for event in events {
            match event {
                SerialEvent::Chunk { side, bytes } => {
                    let text = serial::decode_chunk(self.config.encoding, &bytes);
                    let direction = match side {
                        PortSide::Left => Direction::Outgoing,
                        PortSide::Right => Direction::Incoming,
                    };
                    if let Some(file) = &mut self.log_file {
                        let _ = writeln!(file, "{} {}", direction.arrow(), text);
                    }
                    self.log_lines.push(LogLine { direction, text });
                }
                SerialEvent::Error { side, message } => {
                    self.handle_serial_error(side, message);
                    break;
                }
            }
        }
    }

    pub fn toggle_log_enabled(&mut self) {
        if !self.is_enabled(Focus::LogCheckbox) {
            return;
        }
        self.log_enabled = !self.log_enabled;
        if !self.log_enabled && self.focus == Focus::LogPath {
            self.prev_focus();
        }
    }

    fn other_side_selected(&self, side: PortSide) -> Option<usize> {
        match side {
            PortSide::Left => self.port_right_selected,
            PortSide::Right => self.port_left_selected,
        }
    }

    /// Opens the dropdown for `side`, nudging the initial highlight off the
    /// other side's current selection (that port isn't choosable here).
    pub fn open_dropdown(&mut self, side: PortSide) {
        let current = match side {
            PortSide::Left => self.port_left_selected,
            PortSide::Right => self.port_right_selected,
        };
        let excluded = self.other_side_selected(side);
        let len = self.available_ports.len();
        let mut highlighted = current.unwrap_or(0);
        if len > 1 {
            while Some(highlighted) == excluded {
                highlighted = (highlighted + 1) % len;
            }
        }
        self.overlay = Overlay::PortDropdown { side, highlighted };
    }

    pub fn open_config(&mut self) {
        self.overlay = Overlay::Config { field: ConfigField::Encoding };
    }

    pub fn scroll_log_up(&mut self) {
        let max = self.log_lines.len().saturating_sub(1);
        self.log_scroll = (self.log_scroll + 1).min(max);
    }

    pub fn scroll_log_down(&mut self) {
        self.log_scroll = self.log_scroll.saturating_sub(1);
    }

    fn handle_dropdown_key(&mut self, key: KeyEvent) {
        let Overlay::PortDropdown { side, highlighted } = self.overlay else { return };
        let len = self.available_ports.len();
        let excluded = self.other_side_selected(side);
        match key.code {
            KeyCode::Up if len > 0 => {
                self.overlay = Overlay::PortDropdown { side, highlighted: step_over_excluded(highlighted, len, excluded, false) };
            }
            KeyCode::Down if len > 0 => {
                self.overlay = Overlay::PortDropdown { side, highlighted: step_over_excluded(highlighted, len, excluded, true) };
            }
            KeyCode::Enter => {
                if len > 0 && Some(highlighted) != excluded {
                    match side {
                        PortSide::Left => self.port_left_selected = Some(highlighted),
                        PortSide::Right => self.port_right_selected = Some(highlighted),
                    }
                    self.auto_select_remaining_port(side, highlighted);
                }
                self.overlay = Overlay::None;
            }
            KeyCode::Esc => self.overlay = Overlay::None,
            _ => {}
        }
    }

    /// If exactly two ports exist total and the other side is still
    /// unselected, there's only one port it could ever be -- pick it
    /// automatically rather than making the user open a second dropdown.
    fn auto_select_remaining_port(&mut self, side: PortSide, chosen: usize) {
        if self.available_ports.len() != 2 {
            return;
        }
        let remaining = (chosen + 1) % 2;
        match side {
            PortSide::Left if self.port_right_selected.is_none() => self.port_right_selected = Some(remaining),
            PortSide::Right if self.port_left_selected.is_none() => self.port_left_selected = Some(remaining),
            _ => {}
        }
    }

    /// Esc and Enter both just close the modal -- edits apply live to
    /// `self.config` as the user cycles values, there's no staging/cancel
    /// copy. That's a deliberate simplicity choice, not an oversight.
    fn handle_config_key(&mut self, key: KeyEvent) {
        let Overlay::Config { field } = self.overlay else { return };
        match key.code {
            KeyCode::Tab | KeyCode::Down => self.overlay = Overlay::Config { field: field.next() },
            KeyCode::BackTab | KeyCode::Up => self.overlay = Overlay::Config { field: field.prev() },
            KeyCode::Left => self.config.cycle_field(field, false),
            KeyCode::Right => self.config.cycle_field(field, true),
            KeyCode::Enter | KeyCode::Esc => self.overlay = Overlay::None,
            _ => {}
        }
    }

    fn handle_log_path_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left => self.log_path_cursor = self.log_path_cursor.saturating_sub(1),
            KeyCode::Right => {
                self.log_path_cursor = (self.log_path_cursor + 1).min(self.log_path.chars().count());
            }
            KeyCode::Backspace if self.log_path_cursor > 0 => {
                self.log_path_cursor -= 1;
                remove_char_at(&mut self.log_path, self.log_path_cursor);
            }
            KeyCode::Delete => remove_char_at(&mut self.log_path, self.log_path_cursor),
            KeyCode::Char(c) => {
                insert_char_at(&mut self.log_path, self.log_path_cursor, c);
                self.log_path_cursor += 1;
            }
            _ => {}
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Global quit -- deliberately Ctrl+C rather than 'q' or Esc, since
        // both of those are needed for text entry / closing overlays.
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return;
        }

        // The log view isn't in the Tab cycle -- it scrolls unconditionally.
        match key.code {
            KeyCode::PageUp => return self.scroll_log_up(),
            KeyCode::PageDown => return self.scroll_log_down(),
            _ => {}
        }

        // If there's an overlay open, send the key to the overlay handler.
        match self.overlay {
            Overlay::None => {}
            Overlay::PortDropdown { .. } => {
                self.handle_dropdown_key(key);
                return;
            }
            Overlay::Config { .. } => {
                self.handle_config_key(key);
                return;
            }
        }

        // Advance or backtrack through focus with Tab and Shift+Tab
        match key.code {
            KeyCode::Tab => self.next_focus(),
            KeyCode::BackTab => self.prev_focus(),
            _ => {}
        }

        // Arrow-key focus movement (spatial map). LogPath owns Left/Right
        // itself (cursor movement, below), so those are excluded here when
        // it has focus.
        match key.code {
            KeyCode::Up | KeyCode::Down => {
                self.move_focus_by_arrow(key.code);
                return;
            }
            KeyCode::Left | KeyCode::Right if self.focus != Focus::LogPath => {
                self.move_focus_by_arrow(key.code);
                return;
            }
            _ => {}
        }

        // Enter and Space take action on whatever's currently focused
        if key.code == KeyCode::Enter || key.code == KeyCode::Char(' ') {
            match self.focus {
                Focus::Config => self.open_config(),
                Focus::PortLeftSelector => self.open_dropdown(PortSide::Left),
                Focus::PortRightSelector => self.open_dropdown(PortSide::Right),
                Focus::StartStop => self.toggle_run_state(),
                Focus::LogCheckbox => self.toggle_log_enabled(),
                _ => {}
            }
        }

        // All other key codes handled by this point - only handle any other key if focused on LogPath
        if self.focus == Focus::LogPath {
            self.handle_log_path_key(key)
        }
    }
}

/// The visual grid is: row 0 `Config` alone; row 1 `PortLeftSelector |
/// StartStop | PortRightSelector`; row 3 `LogCheckbox | LogPath` (row 2, the
/// log view, isn't focusable). Left/Right move within a row; Up/Down move
/// between rows. `LogPath`'s Left/Right are deliberately absent here -- the
/// caller routes those to cursor movement instead.
fn arrow_target(focus: Focus, key: KeyCode) -> Option<Focus> {
    use Focus::*;
    match (focus, key) {
        (PortLeftSelector, KeyCode::Right) => Some(StartStop),
        (StartStop, KeyCode::Left) => Some(PortLeftSelector),
        (StartStop, KeyCode::Right) => Some(PortRightSelector),
        (PortRightSelector, KeyCode::Left) => Some(StartStop),
        (LogCheckbox, KeyCode::Right) => Some(LogPath),

        (Config, KeyCode::Down) => Some(PortLeftSelector),
        (PortLeftSelector, KeyCode::Up) => Some(Config),
        (StartStop, KeyCode::Up) => Some(Config),
        (PortRightSelector, KeyCode::Up) => Some(Config),
        (PortLeftSelector, KeyCode::Down) => Some(LogCheckbox),
        (StartStop, KeyCode::Down) => Some(LogPath),
        (PortRightSelector, KeyCode::Down) => Some(LogPath),
        (LogCheckbox, KeyCode::Up) => Some(PortLeftSelector),
        (LogPath, KeyCode::Up) => Some(StartStop),

        _ => None,
    }
}

/// Moves `from` one step (forward if `forward`, else backward) around a ring
/// of `len` indices, skipping `excluded` -- used to keep dropdown navigation
/// off the port already chosen on the other side. Falls back to landing on
/// `excluded` anyway if every other index has been tried (all excluded, e.g.
/// `len == 1`), so it can't spin forever.
fn step_over_excluded(from: usize, len: usize, excluded: Option<usize>, forward: bool) -> usize {
    let mut idx = from;
    for _ in 0..len {
        idx = if forward { (idx + 1) % len } else { (idx + len - 1) % len };
        if Some(idx) != excluded {
            return idx;
        }
    }
    idx
}

fn open_log_file(
    path: &str,
    left_name: &str,
    right_name: &str,
    config: &ConfigSettings,
) -> std::io::Result<std::fs::File> {
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(
        file,
        "===== COMsniff session {} | L={left_name} R={right_name} | {} =====",
        chrono::Local::now().to_rfc3339(),
        config.line_summary(),
    )?;
    Ok(file)
}

fn insert_char_at(s: &mut String, idx: usize, c: char) {
    let byte_idx = s.char_indices().nth(idx).map(|(i, _)| i).unwrap_or(s.len());
    s.insert(byte_idx, c);
}

fn remove_char_at(s: &mut String, idx: usize) {
    if let Some((byte_idx, ch)) = s.char_indices().nth(idx) {
        s.replace_range(byte_idx..byte_idx + ch.len_utf8(), "");
    }
}

fn stub_log_lines() -> Vec<LogLine> {
    let raw = [
        (Direction::Outgoing, "DE IN 00"),
        (Direction::Incoming, "ACK 00"),
        (Direction::Outgoing, "DE CHG 500"),
        (Direction::Incoming, "ACK"),
        (Direction::Incoming, "TX APVD"),
        (Direction::Incoming, "ECD 4234xxxxxx991234"),
        (Direction::Incoming, "TXID 3f8da09d8cb0090"),
        (Direction::Outgoing, "ACK"),
    ];
    raw.into_iter()
        .map(|(direction, text)| LogLine { direction, text: text.to_string() })
        .collect()
}
