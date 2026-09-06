use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
use bevy::input::mouse::{MouseButton, MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::text::{LineHeight, PositionedGlyph, TextLayoutInfo};
use bevy::window::Ime;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use crate::cursor::CursorPosition;
use crate::dock::IsDragging;
use crate::font::{FontContext, N3riFonts};
use crate::input_focus::{TextInputFocus, TextInputOwner};
use crate::scroll::UiWheelConsumed;
use crate::topbar::FocusedTitle;

pub struct TerminalPlugin;

impl Plugin for TerminalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TerminalState>().add_systems(
            Update,
            (
                terminal_input.after(terminal_selection),
                terminal_ime.after(terminal_selection),
                terminal_sync_output.after(crate::scroll::wheel_dispatch),
                terminal_selection
                    .after(terminal_sync_output)
                    .after(crate::window::WindowFocusSet),
                terminal_render_lines.after(terminal_selection),
            ),
        );
    }
}

const TERMINAL_BG: Color = Color::srgba(0.05, 0.08, 0.12, 0.95);
const TERMINAL_TEXT: Color = Color::srgb(0.86, 0.93, 0.93);
const SELECTION_BG: Color = Color::srgba(0.3, 0.55, 0.8, 0.55);
const FONT_SIZE: f32 = 14.0;
const LINE_HEIGHT: f32 = 18.0;
const MAX_LINES: usize = 64;
const DOUBLE_CLICK_SECS: f32 = 0.4;
const DOUBLE_CLICK_DIST: f32 = 6.0;

#[derive(Component)]
pub struct TerminalOutput;

#[derive(Component)]
struct TerminalLine {
    row: usize,
    li: usize,
}

const NO_LINE: usize = usize::MAX;

#[derive(Component)]
struct SelectionSpan;

#[derive(Component)]
struct PostSpan;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct BufferPos {
    line: usize,
    ch: usize,
}

#[derive(Clone, Copy, Debug)]
struct Selection {
    start: BufferPos,
    end: BufferPos,
}

impl Selection {
    fn new(a: BufferPos, b: BufferPos) -> Self {
        if a.line < b.line || (a.line == b.line && a.ch <= b.ch) {
            Self { start: a, end: b }
        } else {
            Self { start: b, end: a }
        }
    }
}

static CLIPBOARD: OnceLock<Option<Mutex<arboard::Clipboard>>> = OnceLock::new();

fn clipboard() -> Option<&'static Mutex<arboard::Clipboard>> {
    CLIPBOARD
        .get_or_init(|| arboard::Clipboard::new().ok().map(Mutex::new))
        .as_ref()
}

pub(crate) fn copy_text(text: &str) {
    if let Some(mutex) = clipboard() {
        if let Ok(mut cb) = mutex.lock() {
            let _ = cb.set_text(text);
        }
    }
}

pub fn paste_text() -> Option<String> {
    let mutex = clipboard()?;
    let mut cb = mutex.lock().ok()?;
    cb.get_text().ok()
}

struct PtyHandle {
    writer: Option<Box<dyn Write + Send>>,
    _child: Option<Box<dyn portable_pty::Child + Send>>,
}

unsafe impl Send for PtyHandle {}
unsafe impl Sync for PtyHandle {}

#[derive(Clone, Default)]
enum VtMode {
    #[default]
    Ground,
    Esc,
    Csi(Vec<char>),
    Osc,
}

#[derive(Resource)]
pub struct TerminalState {
    lines: Vec<String>,
    input_buf: String,
    pty: Option<Arc<Mutex<PtyHandle>>>,
    pending: Arc<Mutex<String>>,
    scroll_offset: usize,
    stick_to_bottom: bool,
    /// Pixel 滚轮折算行数时的亚行余量（触摸板连续微增量逐帧累积）。
    wheel_accum: f32,
    initialized: bool,
    echo_col: usize,
    vt_mode: VtMode,
    selection: Option<Selection>,
    drag_anchor: Option<BufferPos>,
    click_count: u8,
    last_click_time: f32,
    last_click_pos: Vec2,
    composing: bool,
    preedit_text: String,
    preedit_cursor: Option<(usize, usize)>,
    initial_cmd: Option<String>,
}

impl Default for TerminalState {
    fn default() -> Self {
        Self {
            lines: vec!["n3ri_os 终端 v0.1.0".to_string(), String::new()],
            input_buf: String::new(),
            pty: None,
            pending: Arc::new(Mutex::new(String::new())),
            scroll_offset: 0,
            stick_to_bottom: true,
            wheel_accum: 0.0,
            initialized: false,
            echo_col: 0,
            vt_mode: VtMode::Ground,
            selection: None,
            drag_anchor: None,
            click_count: 0,
            last_click_time: 0.0,
            last_click_pos: Vec2::ZERO,
            composing: false,
            preedit_text: String::new(),
            preedit_cursor: None,
            initial_cmd: None,
        }
    }
}

impl TerminalState {
    pub(crate) fn input_caret(&self) -> (usize, usize) {
        (self.echo_col, self.lines.len().saturating_sub(1))
    }

    fn ensure_pty(&mut self) {
        if self.initialized {
            return;
        }
        self.initialized = true;

        let pty_system = NativePtySystem::default();
        let pair = match pty_system.openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        }) {
            Ok(p) => p,
            Err(e) => {
                self.lines.push(format!("PTY error: {}", e));
                return;
            }
        };

        let mut cmd = CommandBuilder::new("sh");
        if let Ok(cwd) = std::env::current_dir() {
            cmd.cwd(cwd);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        if let Some(initial) = self.initial_cmd.take() {
            cmd.args(["-c", &initial]);
        }

        let child = match pair.slave.spawn_command(cmd) {
            Ok(c) => c,
            Err(e) => {
                self.lines.push(format!("Shell error: {}", e));
                return;
            }
        };

        let mut reader = match pair.master.try_clone_reader() {
            Ok(r) => r,
            Err(e) => {
                self.lines.push(format!("Reader error: {}", e));
                return;
            }
        };

        let writer = match pair.master.take_writer() {
            Ok(w) => w,
            Err(e) => {
                self.lines.push(format!("Writer error: {}", e));
                return;
            }
        };

        let pending = self.pending.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&buf[..n]).to_string();
                        pending.lock().unwrap().push_str(&text);
                    }
                    Err(_) => break,
                }
            }
        });

        self.pty = Some(Arc::new(Mutex::new(PtyHandle {
            writer: Some(writer),
            _child: Some(child),
        })));
    }

    pub fn request_run(&mut self, command: &str) {
        if self.initialized {
            self.send(&format!("{}\n", command));
        } else {
            self.initial_cmd = Some(format!("{}; exec sh", command));
        }
    }

    pub fn reset(&mut self) {
        if let Some(pty) = self.pty.take() {
            if let Ok(mut handle) = pty.lock() {
                if let Some(child) = handle._child.as_mut() {
                    let _ = child.kill();
                }
                handle.writer = None;
            }
        }
        if let Ok(mut lock) = self.pending.lock() {
            lock.clear();
        }
        self.lines = vec![String::new()];
        self.input_buf.clear();
        self.initialized = false;
        self.scroll_offset = 0;
        self.wheel_accum = 0.0;
        self.stick_to_bottom = true;
        self.echo_col = 0;
        self.vt_mode = VtMode::Ground;
        self.selection = None;
        self.drag_anchor = None;
    }

    fn send(&self, data: &str) {
        if let Some(ref pty) = self.pty {
            let mut lock = pty.lock().unwrap();
            if let Some(ref mut writer) = lock.writer {
                let _ = writer.write_all(data.as_bytes());
                let _ = writer.flush();
            }
        }
    }

    fn visible_start(&self, viewport_lines: usize) -> usize {
        self.lines
            .len()
            .saturating_sub(self.scroll_offset)
            .saturating_sub(viewport_lines)
    }

    fn consume_wheel_rows(&mut self, wheel: &MouseWheel) -> isize {
        let rows = match wheel.unit {
            MouseScrollUnit::Line => wheel.y * 3.0,
            MouseScrollUnit::Pixel => wheel.y / LINE_HEIGHT,
        };
        self.wheel_accum += rows;
        let whole = self.wheel_accum.trunc() as isize;
        self.wheel_accum -= whole as f32;
        whole
    }

    fn vt_feed(&mut self, ch: char) {
        match std::mem::take(&mut self.vt_mode) {
            VtMode::Ground => self.vt_ground(ch),
            VtMode::Esc => match ch {
                '[' => self.vt_mode = VtMode::Csi(Vec::new()),
                ']' => self.vt_mode = VtMode::Osc,
                _ => {}
            },
            VtMode::Csi(mut params) => {
                if ch.is_ascii_alphabetic() || ch == '@' || ch == '~' {
                    self.vt_csi_exec(&params, ch);
                } else {
                    params.push(ch);
                    self.vt_mode = VtMode::Csi(params);
                }
            }
            VtMode::Osc => {
                if ch == '\u{7}' || ch == '\u{1b}' {
                    self.vt_mode = VtMode::Ground;
                }
            }
        }
    }

    fn vt_ground(&mut self, ch: char) {
        match ch {
            '\u{1b}' => self.vt_mode = VtMode::Esc,
            '\n' => {
                self.lines.push(String::new());
                self.echo_col = 0;
            }
            '\r' => self.echo_col = 0,
            '\u{8}' | '\u{7f}' => self.echo_col = self.echo_col.saturating_sub(1),
            '\u{7}' | '\0' => {}
            '\t' => {
                let target = (self.echo_col / 8 + 1) * 8;
                while self.line_len() < target {
                    self.append_char(' ');
                }
                self.echo_col = target;
            }
            c if !c.is_control() => {
                self.put_char_at(c, self.echo_col);
                self.echo_col += 1;
            }
            _ => {}
        }
    }

    fn vt_csi_exec(&mut self, params: &[char], final_char: char) {
        let n_str: String = params.iter().take_while(|c| c.is_ascii_digit()).collect();
        let n: usize = n_str.parse().unwrap_or(0);
        match final_char {
            'K' => match n {
                0 => {
                    if let Some(last) = self.lines.last_mut() {
                        let keep = char_byte(last, self.echo_col.min(last.chars().count()));
                        last.truncate(keep);
                    }
                }
                1 => {
                    let col = self.echo_col;
                    self.ensure_line_cols(col);
                    if let Some(last) = self.lines.last_mut() {
                        let end_b = char_byte(last, col);
                        let space_count = last[..end_b].chars().count();
                        last.replace_range(..end_b, &" ".repeat(space_count));
                    }
                }
                _ => {}
            },
            'D' => self.echo_col = self.echo_col.saturating_sub(n.max(1)),
            'C' => self.echo_col += n.max(1),
            'G'
                if n > 0 => {
                    self.echo_col = n - 1;
                }
            _ => {}
        }
    }

    fn line_len(&self) -> usize {
        self.lines.last().map(|l| l.chars().count()).unwrap_or(0)
    }

    fn append_char(&mut self, c: char) {
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        if let Some(last) = self.lines.last_mut() {
            last.push(c);
        }
    }

    fn ensure_line_cols(&mut self, col: usize) {
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        let li = self.lines.len() - 1;
        while self.lines[li].chars().count() < col {
            self.lines[li].push(' ');
        }
    }

    fn put_char_at(&mut self, c: char, col: usize) {
        self.ensure_line_cols(col);
        let li = self.lines.len() - 1;
        let byte_idx = char_byte(&self.lines[li], col);
        self.lines[li].replace_range(byte_idx..byte_idx, &c.to_string());
    }

    fn selection_text(&self, sel: Selection) -> String {
        if self.lines.is_empty() {
            return String::new();
        }
        let mut out = String::new();
        let last_line = sel.end.line.min(self.lines.len() - 1);
        for li in sel.start.line..=last_line {
            if let Some(line) = self.lines.get(li) {
                let line_len = line.chars().count();
                let start = if li == sel.start.line { sel.start.ch } else { 0 };
                let end = if li == sel.end.line { sel.end.ch } else { line_len };
                let start = start.min(line_len);
                let end = end.min(line_len).max(start);
                if li > sel.start.line {
                    out.push('\n');
                }
                out.push_str(
                    &line
                        .chars()
                        .skip(start)
                        .take(end - start)
                        .collect::<String>(),
                );
            }
        }
        out
    }

    fn shift_after_trim(&mut self, removed: usize) {
        self.selection = self.selection.and_then(|sel| {
            if sel.end.line < removed {
                None
            } else {
                Some(Selection {
                    start: BufferPos {
                        line: sel.start.line.saturating_sub(removed),
                        ch: if sel.start.line >= removed { sel.start.ch } else { 0 },
                    },
                    end: BufferPos {
                        line: sel.end.line.saturating_sub(removed),
                        ch: sel.end.ch,
                    },
                })
            }
        });
        self.drag_anchor = self.drag_anchor.and_then(|p| {
            if p.line < removed {
                None
            } else {
                Some(BufferPos {
                    line: p.line.saturating_sub(removed),
                    ch: p.ch,
                })
            }
        });
    }
}

pub fn spawn_terminal(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    let window_entity = crate::window::spawn_window(parent, "终端", "terminal", 700.0, 500.0, fonts);
    let terminal_font = fonts.get(FontContext::Terminal);

    parent
        .commands()
        .entity(window_entity)
        .with_children(|window| {
            window
                .spawn((
                    TerminalOutput,
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(12.0)),
                        overflow: Overflow::hidden(),
                        ..default()
                    },
                    BackgroundColor(TERMINAL_BG),
                ))
                .with_children(|content| {
                    for row in 0..MAX_LINES {
                        content
                            .spawn((
                                TerminalLine { row, li: NO_LINE },
                                Text::new(""),
                                TextFont {
                                    font: FontSource::Handle(terminal_font.clone()),
                                    font_size: FontSize::Px(FONT_SIZE),
                                    ..default()
                                },
                                TextColor(TERMINAL_TEXT),
                                TextLayout::no_wrap(),
                                LineHeight::Px(LINE_HEIGHT),
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(LINE_HEIGHT),
                                    ..default()
                                },
                            ))
                            .with_children(|line| {
                                line.spawn((
                                    SelectionSpan,
                                    TextSpan::new(""),
                                    TextFont {
                                        font: FontSource::Handle(terminal_font.clone()),
                                        font_size: FontSize::Px(FONT_SIZE),
                                        ..default()
                                    },
                                    TextColor(TERMINAL_TEXT),
                                    TextBackgroundColor(SELECTION_BG),
                                ));
                                line.spawn((
                                    PostSpan,
                                    TextSpan::new(""),
                                    TextFont {
                                        font: FontSource::Handle(terminal_font.clone()),
                                        font_size: FontSize::Px(FONT_SIZE),
                                        ..default()
                                    },
                                    TextColor(TERMINAL_TEXT),
                                ));
                            });
                    }
                });
        });
}

fn terminal_input(
    mut keyboard_inputs: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<TerminalState>,
    focused: Res<FocusedTitle>,
    owner: Res<TextInputOwner>,
    terminal_open: Query<(), With<TerminalOutput>>,
) {
    // 窗口存在且 PTY 未初始化时才触碰 state：无条件 ensure_pty 会在每帧
    // deref ResMut → TerminalState 每帧 marked changed → 下游渲染无法做
    // is_changed 门控；同时避免关窗后残留一个无人使用的后台 sh。
    if !terminal_open.is_empty() && !state.initialized {
        state.ensure_pty();
    }

    if focused.title != "终端" || !owner.is(TextInputFocus::Terminal) || state.composing {
        keyboard_inputs.clear();
        return;
    }

    for event in keyboard_inputs.read() {
        if !event.state.is_pressed() {
            continue;
        }

        let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);

        if ctrl && shift {
            match event.key_code {
                KeyCode::KeyC => {
                    if let Some(sel) = state.selection {
                        copy_text(&state.selection_text(sel));
                    }
                    continue;
                }
                KeyCode::KeyV => {
                    if let Some(text) = paste_text() {
                        state.send(&text);
                    }
                    continue;
                }
                _ => {}
            }
        }

        match &event.logical_key {
            Key::Character(ch) => {
                let raw = ch.chars().next().unwrap();
                let c = if ctrl && raw.is_ascii_alphabetic() {
                    (raw.to_ascii_uppercase() as u8 & 0x1f) as char
                } else {
                    raw
                };
                if c == '\u{3}' {
                    state.send("\x03");
                } else if c == '\u{4}' {
                    state.send("\x04");
                } else if c == '\u{c}' {
                    let keep = state.lines.last().cloned().unwrap_or_default();
                    state.lines.clear();
                    state.lines.push(keep);
                    state.scroll_offset = 0;
                    state.stick_to_bottom = true;
                } else if c.is_control() {
                    state.send(&c.to_string());
                } else {
                    state.input_buf.push(c);
                    state.send(&c.to_string());
                }
            }
            Key::Escape => state.send("\x1b"),
            Key::Space => {
                state.input_buf.push(' ');
                state.send(" ");
            }
            Key::Enter => {
                let cmd = state.input_buf.trim().to_string();
                state.input_buf.clear();
                if cmd == "clear" || cmd == "cls" {
                    clear_terminal_screen(&mut state);
                }
                state.send("\r");
            }
            Key::Backspace => {
                state.input_buf.pop();
                state.send("\x7f");
            }
            Key::Tab => state.send("\t"),
            Key::ArrowUp => state.send("\x1b[A"),
            Key::ArrowDown => state.send("\x1b[B"),
            Key::ArrowLeft => state.send("\x1b[D"),
            Key::ArrowRight => state.send("\x1b[C"),
            Key::Home => state.send("\x1b[H"),
            Key::End => state.send("\x1b[F"),
            Key::Delete => state.send("\x1b[3~"),
            Key::PageUp => {
                state.send("\x1b[5~");
                state.scroll_offset = (state.scroll_offset + 24).min(state.lines.len());
                state.stick_to_bottom = false;
            }
            Key::PageDown => {
                state.send("\x1b[6~");
                state.scroll_offset = state.scroll_offset.saturating_sub(24);
                if state.scroll_offset == 0 {
                    state.stick_to_bottom = true;
                }
            }
            _ => {}
        }
    }
}

fn terminal_ime(
    mut ime_events: MessageReader<Ime>,
    focused: Res<FocusedTitle>,
    mut state: ResMut<TerminalState>,
    owner: Res<TextInputOwner>,
) {
    if focused.title != "终端" || !owner.is(TextInputFocus::Terminal) {
        ime_events.clear();
        return;
    }

    for event in ime_events.read() {
        match event {
            Ime::Preedit { value, cursor, .. } => {
                if value.is_empty() {
                    state.composing = false;
                    state.preedit_text.clear();
                    state.preedit_cursor = None;
                } else {
                    state.composing = true;
                    state.preedit_text = value.clone();
                    state.preedit_cursor = *cursor;
                }
            }
            Ime::Commit { value, .. } => {
                state.composing = false;
                state.preedit_text.clear();
                state.preedit_cursor = None;
                state.send(value);
            }
            Ime::Enabled { .. } => {
                state.composing = false;
                state.preedit_text.clear();
            }
            Ime::Disabled { .. } => {
                state.composing = false;
                state.preedit_text.clear();
            }
        }
    }
}

fn terminal_sync_output(
    mut state: ResMut<TerminalState>,
    mut mouse_wheel: MessageReader<MouseWheel>,
    ui_consumed: Res<UiWheelConsumed>,
    focused: Res<FocusedTitle>,
    cursor: Res<CursorPosition>,
    output: Query<(&ComputedNode, &UiGlobalTransform), With<TerminalOutput>>,
) {
    let pending: String = {
        let mut lock = state.pending.lock().unwrap();
        let s = lock.clone();
        lock.clear();
        s
    };

    if !pending.is_empty() {
        for ch in pending.chars() {
            state.vt_feed(ch);
        }

        if state.lines.len() > 2000 {
            let excess = state.lines.len() - 2000;
            state.lines.drain(..excess);
            state.shift_after_trim(excess);
        }

        if state.stick_to_bottom {
            state.scroll_offset = 0;
        }
    }

    // 无滚轮消息时连光标悬停检测都不必做（该检测每帧都要 inverse transform）。
    if mouse_wheel.is_empty() {
        return;
    }

    let cursor_over_terminal = focused.title == "终端"
        && cursor.active
        && output.iter().any(|(node, transform)| {
            transform
                .try_inverse()
                .map(|inverse| inverse.transform_point2(cursor.physical))
                .is_some_and(|local| {
                    let half = node.size() * 0.5;
                    local.x.abs() <= half.x && local.y.abs() <= half.y
                })
        });

    for wheel in mouse_wheel.read().filter(|_| cursor_over_terminal && !ui_consumed.0) {
        let delta = state.consume_wheel_rows(wheel);
        if delta > 0 {
            state.scroll_offset = (state.scroll_offset + delta as usize)
                .min(state.lines.len().saturating_sub(1));
            state.stick_to_bottom = false;
        } else if delta < 0 {
            state.scroll_offset = state.scroll_offset.saturating_sub((-delta) as usize);
            if state.scroll_offset == 0 {
                state.stick_to_bottom = true;
            }
        }
    }
}

fn terminal_selection(
    mouse: Res<ButtonInput<MouseButton>>,
    is_dragging: Res<IsDragging>,
    cursor: Res<CursorPosition>,
    lines_query: Query<(&TerminalLine, &ComputedNode, &UiGlobalTransform, &TextLayoutInfo)>,
    container_query: Query<(&ComputedNode, &UiGlobalTransform), With<TerminalOutput>>,
    time: Res<Time>,
    mut state: ResMut<TerminalState>,
    focused: Res<FocusedTitle>,
    mut owner: ResMut<TextInputOwner>,
) {
    if !cursor.active {
        return;
    }
    let cursor = cursor.physical;

    // 无左键活动（按下/按住/松开）时整段选择逻辑无事可做：跳过 per-line
    // hit-test（原先每帧对全部 64 行做 inverse transform，鼠标在顶栏/dock
    // 上静止也照扫）。
    if !mouse.just_pressed(MouseButton::Left)
        && !mouse.pressed(MouseButton::Left)
        && !mouse.just_released(MouseButton::Left)
    {
        return;
    }

    let hit = lines_query.iter().find_map(|(line, node, transform, layout)| {
        if line.li == NO_LINE {
            return None;
        }
        hit_test_char(node, transform, layout, cursor).map(|ch| (line.li, ch))
    });

    if mouse.just_pressed(MouseButton::Left) {
        if focused.title == "终端" {
            if owner.0 != TextInputFocus::Terminal {
                info!(
                    "[ime] terminal focus activated | cursor.physical={:?}",
                    cursor
                );
            }
            owner.0 = TextInputFocus::Terminal;
        }
        if is_dragging.0 {
            return;
        }
        let in_content = container_query
            .iter()
            .any(|(node, transform)| node_in_bounds(node, transform, cursor));
        if !in_content {
            return;
        }

        let now = time.elapsed_secs();
        let click = if now - state.last_click_time < DOUBLE_CLICK_SECS
            && (cursor - state.last_click_pos).length() < DOUBLE_CLICK_DIST
        {
            (state.click_count + 1).min(3)
        } else {
            1
        };
        state.click_count = click;
        state.last_click_time = now;
        state.last_click_pos = cursor;

        if let Some((li, ch)) = hit {
            if li < state.lines.len() {
                let line_len = state.lines[li].chars().count();
                let pos = BufferPos {
                    line: li,
                    ch: ch.min(line_len),
                };
                match click {
                    2 => {
                        let sel = select_word(&state, pos);
                        state.selection = Some(sel);
                        state.drag_anchor = None;
                    }
                    3 => {
                        state.selection = Some(Selection::new(
                            BufferPos { line: li, ch: 0 },
                            BufferPos { line: li, ch: line_len },
                        ));
                        state.drag_anchor = None;
                    }
                    _ => {
                        state.selection = None;
                        state.drag_anchor = Some(pos);
                    }
                }
            }
        } else if click == 1 {
            state.selection = None;
            state.drag_anchor = None;
        }
    }

    if mouse.pressed(MouseButton::Left) {
        if let Some(anchor) = state.drag_anchor {
            if let Some((li, ch)) = hit {
                if li < state.lines.len() {
                    let line_len = state.lines[li].chars().count();
                    let pos = BufferPos {
                        line: li,
                        ch: ch.min(line_len),
                    };
                    state.selection = Some(Selection::new(anchor, pos));
                }
            }
        }
    }

    if mouse.just_released(MouseButton::Left) {
        if let Some(sel) = state.selection {
            let text = state.selection_text(sel);
            if !text.is_empty() {
                copy_text(&text);
            }
        }
        state.drag_anchor = None;
    }
}

fn terminal_render_lines(
    state: Res<TerminalState>,
    container_query: Query<&ComputedNode, With<TerminalOutput>>,
    row_nodes: Query<&ComputedNode, With<TerminalLine>>,
    new_container: Query<(), (With<TerminalOutput>, Added<TerminalOutput>)>,
    mut lines_query: Query<(&mut TerminalLine, &Children, &mut Text, &mut Visibility)>,
    mut span_query: Query<(
        &mut TextSpan,
        Option<&mut TextBackgroundColor>,
        Option<&mut TextColor>,
    )>,
    mut last_viewport: Local<usize>,
) {
    let Some(container) = container_query.iter().next() else {
        return;
    };
    let viewport_lines = viewport_from(container, measured_row_pitch(&row_nodes));

    // 内容/几何都未变化、且终端容器不是本帧新建时跳过整轮文本重建——
    // 空闲终端每帧仍在做 per-line 字符串切片 + 分配，纯属浪费。
    // （用 TerminalOutput 的 Added 而非 TerminalLine：避免与 &mut TerminalLine
    // 参数构成 B0001 访问冲突。）
    if !state.is_changed()
        && new_container.is_empty()
        && *last_viewport == viewport_lines
    {
        return;
    }
    *last_viewport = viewport_lines;

    let start = state.visible_start(viewport_lines);

    for (mut line, children, mut text, mut vis) in lines_query.iter_mut() {
        let row = line.row;
        if row >= viewport_lines {
            *vis = Visibility::Hidden;
            line.li = NO_LINE;
            continue;
        }
        *vis = Visibility::Inherited;

        let li = start + row;
        line.li = li;
        let full = state.lines.get(li).map(|s| s.as_str()).unwrap_or("");

        let (sel_start, sel_end) = match state.selection {
            Some(sel) => selection_range(sel, li, full.chars().count()),
            None => (0, 0),
        };

        let sel_start_b = char_byte(full, sel_start);
        let sel_end_b = char_byte(full, sel_end);

        let char_count = full.chars().count();
        let is_cursor_row = !state.lines.is_empty() && li == state.lines.len() - 1;
        let has_selection_here = sel_end > sel_start;

        let (root, selected, post, invert) = if is_cursor_row && !has_selection_here {
            let col = state.echo_col.min(char_count);
            let col_b = char_byte(full, col);
            (
                &full[..col_b],
                full[col_b..]
                    .chars()
                    .next()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| " ".to_string()),
                full[col_b..].chars().skip(1).collect::<String>(),
                true,
            )
        } else {
            (
                &full[..sel_start_b],
                full[sel_start_b..sel_end_b].to_string(),
                full[sel_end_b..].to_string(),
                false,
            )
        };

        if **text != root {
            **text = root.to_string();
        }

        if let Some(&sel_e) = children.first() {
            if let Ok((mut span, maybe_bg, maybe_fg)) = span_query.get_mut(sel_e) {
                if **span != selected {
                    **span = selected;
                }
                if let Some(mut bg) = maybe_bg {
                    bg.0 = if invert { TERMINAL_TEXT } else { SELECTION_BG };
                }
                if let Some(mut fg) = maybe_fg {
                    fg.0 = if invert { TERMINAL_BG } else { TERMINAL_TEXT };
                }
            }
        }
        if let Some(&post_e) = children.get(1) {
            if let Ok((mut span, _, _)) = span_query.get_mut(post_e) {
                if **span != post {
                    **span = post;
                }
            }
        }
    }
}

fn measured_row_pitch(row_nodes: &Query<&ComputedNode, With<TerminalLine>>) -> f32 {
    row_nodes
        .iter()
        .map(|node| node.size().y)
        .find(|h| *h > 0.0)
        .unwrap_or(LINE_HEIGHT)
}

fn viewport_from(container: &ComputedNode, pitch: f32) -> usize {
    let content_h = container.size().y - container.padding.min_inset.y - container.padding.max_inset.y;
    if content_h <= 0.0 || pitch <= 0.0 {
        return 24;
    }
    ((content_h / pitch).floor() as usize).clamp(1, MAX_LINES)
}

fn node_in_bounds(node: &ComputedNode, transform: &UiGlobalTransform, point: Vec2) -> bool {
    let Some(local) = transform.try_inverse().map(|t| t.transform_point2(point)) else {
        return false;
    };
    let half = node.size() * 0.5;
    local.x >= -half.x && local.x <= half.x && local.y >= -half.y && local.y <= half.y
}

fn hit_test_char(
    node: &ComputedNode,
    transform: &UiGlobalTransform,
    layout: &TextLayoutInfo,
    point: Vec2,
) -> Option<usize> {
    let local = transform.try_inverse()?.transform_point2(point);
    let half = node.size() * 0.5;
    if local.y < -half.y || local.y > half.y {
        return None;
    }
    if local.x < -half.x || local.x > half.x {
        return None;
    }
    let x = local.x - node.content_box().min.x;
    Some(char_at_x(&layout.glyphs, x))
}

fn char_at_x(glyphs: &[PositionedGlyph], x: f32) -> usize {
    let mut idx = 0;
    for (i, g) in glyphs.iter().enumerate() {
        if x <= g.position.x {
            idx = i;
            break;
        }
        idx = i + 1;
    }
    idx
}

fn selection_range(sel: Selection, li: usize, line_len: usize) -> (usize, usize) {
    if li < sel.start.line || li > sel.end.line {
        return (0, 0);
    }
    let start = if li == sel.start.line { sel.start.ch } else { 0 };
    let end = if li == sel.end.line { sel.end.ch } else { line_len };
    (start.min(line_len), end.min(line_len).max(start))
}

fn char_byte(s: &str, idx: usize) -> usize {
    s.char_indices()
        .nth(idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

fn clear_terminal_screen(state: &mut TerminalState) {
    state.lines.clear();
    state.lines.push(String::new());
    state.scroll_offset = 0;
    state.wheel_accum = 0.0;
    state.stick_to_bottom = true;
}

fn select_word(state: &TerminalState, pos: BufferPos) -> Selection {
    let Some(line) = state.lines.get(pos.line) else {
        return Selection::new(pos, pos);
    };
    let chars: Vec<char> = line.chars().collect();
    let ci = pos.ch.min(chars.len());
    if ci >= chars.len() {
        return Selection::new(pos, pos);
    }
    let is_ws = chars[ci].is_whitespace();
    let mut start = ci;
    let mut end = ci;
    while start > 0 && chars[start - 1].is_whitespace() == is_ws {
        start -= 1;
    }
    while end < chars.len() && chars[end].is_whitespace() == is_ws {
        end += 1;
    }
    Selection::new(
        BufferPos {
            line: pos.line,
            ch: start,
        },
        BufferPos {
            line: pos.line,
            ch: end,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::schedule::Schedule;

    #[test]
    fn terminal_systems_init_without_b0001() {
        let mut world = World::new();
        let mut schedule = Schedule::default();
        schedule.add_systems((
            terminal_input,
            terminal_ime,
            terminal_sync_output,
            terminal_selection,
            terminal_render_lines,
        ));
        schedule.initialize(&mut world);
    }
}
