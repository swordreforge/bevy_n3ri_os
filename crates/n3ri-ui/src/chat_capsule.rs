use std::collections::VecDeque;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;
use std::thread;

use bevy::prelude::*;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::window::Ime;

use n3ri_llm::{LlmClient, Message};

use crate::font::{FontContext, N3riFonts};
use crate::input_focus::{TextInputFocus, TextInputOwner};

pub struct ChatCapsulePlugin;

impl Plugin for ChatCapsulePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChatCapsuleState>()
            .init_resource::<ChatRise>()
            .init_resource::<ChatLlmState>()
            .init_resource::<ChatHistory>()
            .init_resource::<AgentBubbleInbox>()
            .init_resource::<ChatBubbleState>()
            .add_systems(
                Update,
                chat_capsule_interact.after(crate::window::WindowFocusSet),
            )
            .add_systems(Update, chat_capsule_animate)
            .add_systems(
                Update,
                chat_capsule_input
                    .after(chat_capsule_interact)
                    .after(chat_capsule_ime),
            )
            .add_systems(Update, chat_capsule_ime.after(chat_capsule_interact))
            .add_systems(Update, chat_capsule_cursor_blink)
            .add_systems(Update, chat_llm_dispatch.after(chat_capsule_input))
            .add_systems(Update, chat_llm_poll)
            .add_systems(Update, agent_outbox_bridge.after(chat_llm_poll))
            .add_systems(Update, agent_inbox_drain.after(agent_outbox_bridge))
            .add_systems(Update, chat_sentence_reveal)
            .add_systems(Update, chat_bubble_sync);
        app.add_message::<ChatEmotionEvent>();
    }
}

#[derive(Resource)]
pub struct ChatCapsuleState {
    pub active: bool,
    pub input_text: String,
    pub cursor_pos: usize,
    pub cursor_visible: bool,
    pub composing: bool,
    pub preedit_text: String,
    pub preedit_cursor: Option<(usize, usize)>,
    pub submitted: Option<String>,
}

impl Default for ChatCapsuleState {
    fn default() -> Self {
        Self {
            active: false,
            input_text: String::new(),
            cursor_pos: 0,
            cursor_visible: true,
            composing: false,
            preedit_text: String::new(),
            preedit_cursor: None,
            submitted: None,
        }
    }
}

/// Live2D 头部出现时气泡上移的距离(px),由 examples/minimal 依据头部可见性写入
#[derive(Resource, Default)]
pub struct ChatRise(pub f32);

#[derive(Resource)]
#[derive(Default)]
pub(crate) struct ChatLlmState {
    pub pending: bool,
    #[allow(clippy::type_complexity)]
    rx: Option<Mutex<Receiver<(Result<String, String>, String, Vec<n3ri_agent::PendingEffect>)>>>,
    system_prompt: Option<String>,
}


/// LLM 对话上下文(系统提示词单独存放,不占历史条目)
#[derive(Resource, Default)]
pub(crate) struct ChatHistory {
    messages: Vec<Message>,
}

/// Agent 主动轮投递口：M2 起 `n3ri-agent` 的 `proactive_poll` 经此队列进气泡。
#[derive(Resource, Default)]
pub struct AgentBubbleInbox {
    pub items: VecDeque<String>,
}

#[derive(Resource, Default)]
struct ChatBubbleState {
    /// 场内时钟
    now: f32,
    /// 当前显示的 AI 气泡(最多 3 条,旧→新)
    shown: VecDeque<BubbleEntry>,
    /// 待逐句显示的句子队列
    queue: VecDeque<String>,
    /// 距离下一句显示的倒计时
    timer: f32,
}

#[derive(Clone)]
struct BubbleEntry {
    text: String,
    born: f32,
    /// 淡出进度 0→1,None = 未开始
    dying: Option<f32>,
}

#[derive(Component)]
struct ChatCapsuleRoot;

#[derive(Component)]
struct ChatCapsuleDisplay;

#[derive(Component)]
struct ChatCapsuleShortcut;

#[derive(Component)]
struct ChatCapsuleScanIcon;

#[derive(Component)]
struct ChatCapsuleArrow;

#[derive(Component)]
struct ChatCapsuleShortcutBadge;

#[derive(Default, PartialEq)]
enum AnimState {
    #[default]
    Inactive,
    Activating { t: f32 },
    Active,
    Deactivating { t: f32 },
}

const BG_INACTIVE: Color = Color::srgba(0.10, 0.12, 0.16, 0.55);
const BG_ACTIVE: Color = Color::srgba(0.92, 0.94, 0.96, 0.92);
const BORDER_INACTIVE: Color = Color::srgba(0.30, 0.35, 0.40, 0.30);
const BORDER_ACTIVE: Color = Color::srgba(0.60, 0.65, 0.70, 0.40);
const TEXT_INACTIVE: Color = Color::srgb(0.35, 0.72, 0.82);
const TEXT_ACTIVE: Color = Color::srgba(0.20, 0.25, 0.28, 0.80);
const SHORTCUT_TEXT_INACTIVE: Color = Color::srgba(0.50, 0.55, 0.60, 0.60);
const SHORTCUT_TEXT_ACTIVE: Color = Color::srgba(0.35, 0.38, 0.42, 0.50);
const ICON_INACTIVE: Color = Color::srgba(0.50, 0.55, 0.60, 0.70);
const ICON_ACTIVE: Color = Color::srgba(0.35, 0.40, 0.45, 0.60);

const CAPSULE_WIDTH: f32 = 320.0;
const CAPSULE_HEIGHT: f32 = 42.0;
const CAPSULE_RADIUS: f32 = 21.0;
const CAPSULE_BOTTOM: f32 = 14.0;
const CAPSULE_RIGHT: f32 = 24.0;
const ANIM_DURATION: f32 = 0.15;
const BLINK_RATE: f32 = 0.5;
const PREEDIT_COLOR: Color = Color::srgba(0.20, 0.25, 0.28, 0.60);
const PLACEHOLDER: &str = "和 Nori 聊天...";

pub fn spawn_chat_capsule(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    parent
        .spawn((
            ChatCapsuleRoot,
            Button,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(CAPSULE_BOTTOM),
                right: Val::Px(CAPSULE_RIGHT),
                width: Val::Px(CAPSULE_WIDTH),
                height: Val::Px(CAPSULE_HEIGHT),
                border_radius: BorderRadius::all(Val::Px(CAPSULE_RADIUS)),
                padding: UiRect {
                    left: Val::Px(16.0),
                    right: Val::Px(14.0),
                    ..default()
                },
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                column_gap: Val::Px(0.0),
                ..default()
            },
            BackgroundColor(BG_INACTIVE),
            BorderColor::all(BORDER_INACTIVE),
            GlobalZIndex(100),
        ))
        .with_children(|root| {
            root.spawn((
                ChatCapsuleDisplay,
                Text::new(PLACEHOLDER),
                TextFont {
                    font: FontSource::Handle(fonts.get(FontContext::Ui)),
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(TEXT_INACTIVE),
            ));

            root.spawn((
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    ..default()
                },
            ))
            .with_children(|right| {
                right.spawn((
                    ChatCapsuleShortcutBadge,
                    Node {
                        padding: UiRect {
                            left: Val::Px(5.0),
                            right: Val::Px(5.0),
                            top: Val::Px(1.0),
                            bottom: Val::Px(1.0),
                        },
                        border_radius: BorderRadius::all(Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.25, 0.28, 0.32, 0.35)),
                ))
                .with_children(|badge| {
                    badge.spawn((
                        ChatCapsuleShortcut,
                        Text::new("Ctrl + K"),
                        TextFont {
                            font: FontSource::Handle(fonts.get(FontContext::Ui)),
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(SHORTCUT_TEXT_INACTIVE),
                    ));
                });

                right.spawn((
                    ChatCapsuleScanIcon,
                    Text::new("⊞"),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(ICON_INACTIVE),
                ));

                right.spawn((
                    ChatCapsuleArrow,
                    Text::new("↑"),
                    TextFont {
                        font_size: FontSize::Px(16.0),
                        ..default()
                    },
                    TextColor(ICON_INACTIVE),
                ));
            });
        });

    // —— AI 回复气泡堆叠(最多 3 条,新在下) ——
    parent
        .spawn((
            ChatBubbleRoot,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(CAPSULE_RIGHT),
                bottom: Val::Px(64.0),
                width: Val::Px(460.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                row_gap: Val::Px(8.0),
                ..default()
            },
            GlobalZIndex(100),
        ))
        .with_children(|stack| {
            for i in 0..3usize {
                stack
                    .spawn((
                        ChatBubble(i),
                        Node {
                            max_width: Val::Px(460.0),
                            padding: UiRect::px(14.0, 14.0, 9.0, 9.0),
                            border_radius: BorderRadius::all(Val::Px(14.0)),
                            display: Display::Flex,
                            ..default()
                        },
                        UiTransform::IDENTITY,
                        BackgroundColor(BUBBLE_BG),
                        Visibility::Hidden,
                    ))
                    .with_children(|bubble| {
                        bubble.spawn((
                            ChatBubbleText(i),
                            Text::new(""),
                            TextFont {
                                font: FontSource::Handle(fonts.get(FontContext::Ui)),
                                font_size: FontSize::Px(14.0),
                                ..default()
                            },
                            TextColor(BUBBLE_TEXT),
                        ));
                    });
            }
        });
}

fn chat_capsule_interact(
    mouse: Res<ButtonInput<MouseButton>>,
    roots: Query<(&Interaction, Entity), With<ChatCapsuleRoot>>,
    mut state: ResMut<ChatCapsuleState>,
    mut owner: ResMut<TextInputOwner>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    let mut clicked_capsule = false;
    for (interaction, _entity) in roots.iter() {
        if *interaction == Interaction::Pressed {
            clicked_capsule = true;
            break;
        }
    }

    if clicked_capsule {
        state.active = !state.active;
        owner.0 = if state.active {
            TextInputFocus::Chat
        } else {
            TextInputFocus::None
        };
    } else if state.active {
        state.active = false;
        if owner.is(TextInputFocus::Chat) {
            owner.0 = TextInputFocus::None;
        }
    }
}

#[allow(clippy::type_complexity)]
fn chat_capsule_animate(
    time: Res<Time>,
    state: Res<ChatCapsuleState>,
    mut anim_state: Local<AnimState>,
    mut root_q: Query<(&mut BackgroundColor, &mut BorderColor), With<ChatCapsuleRoot>>,
    mut display_q: Query<(&mut Text, &mut TextColor), With<ChatCapsuleDisplay>>,
    mut shortcut_text_q: Query<
        &mut TextColor,
        (
            With<ChatCapsuleShortcut>,
            Without<ChatCapsuleDisplay>,
        ),
    >,
    mut shortcut_bg_q: Query<
        &mut BackgroundColor,
        (With<ChatCapsuleShortcutBadge>, Without<ChatCapsuleRoot>),
    >,
    mut scan_q: Query<
        &mut TextColor,
        (
            With<ChatCapsuleScanIcon>,
            Without<ChatCapsuleDisplay>,
            Without<ChatCapsuleShortcut>,
        ),
    >,
    mut arrow_q: Query<
        &mut TextColor,
        (
            With<ChatCapsuleArrow>,
            Without<ChatCapsuleDisplay>,
            Without<ChatCapsuleShortcut>,
            Without<ChatCapsuleScanIcon>,
        ),
    >,
) {
    let dt = time.delta_secs();
    let want_active = state.active;

    match &*anim_state {
        AnimState::Inactive if want_active => {
            *anim_state = AnimState::Activating { t: 0.0 };
        }
        AnimState::Active if !want_active => {
            *anim_state = AnimState::Deactivating { t: 0.0 };
        }
        _ => {}
    }

    let progress;
    let mut transitioning = false;

    match &mut *anim_state {
        AnimState::Activating { t } => {
            *t += dt / ANIM_DURATION;
            if *t >= 1.0 {
                *t = 1.0;
            }
            progress = *t;
            transitioning = true;
        }
        AnimState::Deactivating { t } => {
            *t += dt / ANIM_DURATION;
            if *t >= 1.0 {
                *t = 1.0;
            }
            progress = 1.0 - *t;
            transitioning = true;
        }
        AnimState::Active => {
            progress = 1.0;
        }
        AnimState::Inactive => {
            progress = 0.0;
        }
    }

    let should_transition_to_active = matches!(&*anim_state, AnimState::Activating { .. });
    let should_transition_to_inactive = matches!(&*anim_state, AnimState::Deactivating { .. });

    if should_transition_to_active && progress >= 1.0 {
        *anim_state = AnimState::Active;
    } else if should_transition_to_inactive && progress <= 0.0 {
        *anim_state = AnimState::Inactive;
    }

    if !transitioning && !want_active {
        return;
    }

    let active_t = progress;

    // 现值比较:Active 稳态下 active_t 恒定,插值目标每帧相同,相同则跳过组件写,
    // 避免触发无谓的 layout/重绘(参照 chat_bubble_sync 的 bg.0 != target 先例)。
    if let Ok((mut bg, mut border)) = root_q.single_mut() {
        let bg_target = lerp_color(BG_INACTIVE, BG_ACTIVE, active_t);
        if bg.0 != bg_target {
            *bg = BackgroundColor(bg_target);
        }
        let border_target = BorderColor::all(lerp_color(BORDER_INACTIVE, BORDER_ACTIVE, active_t));
        if *border != border_target {
            *border = border_target;
        }
    }

    if let Ok((mut text, mut color)) = display_q.single_mut() {
        let display_text = if want_active {
            if state.input_text.is_empty() && !state.composing {
                PLACEHOLDER.to_string()
            } else if state.composing {
                let pos = state.cursor_pos.min(state.input_text.chars().count());
                let before: String = state.input_text.chars().take(pos).collect();
                let after: String = state.input_text.chars().skip(pos).collect();
                format!("{}{}{}", before, state.preedit_text, after)
            } else {
                let cursor = if state.cursor_visible { "│" } else { "" };
                let pos = state.cursor_pos.min(state.input_text.chars().count());
                let before: String = state.input_text.chars().take(pos).collect();
                let after: String = state.input_text.chars().skip(pos).collect();
                format!("{before}{cursor}{after}")
            }
        } else {
            PLACEHOLDER.to_string()
        };
        if **text != display_text {
            **text = display_text;
        }

        let text_col = if state.composing {
            PREEDIT_COLOR
        } else {
            lerp_color(TEXT_INACTIVE, TEXT_ACTIVE, active_t)
        };
        if color.0 != text_col {
            *color = TextColor(text_col);
        }
    }

    let icon_target = lerp_color(ICON_INACTIVE, ICON_ACTIVE, active_t);
    for mut tc in scan_q.iter_mut() {
        if tc.0 != icon_target {
            *tc = TextColor(icon_target);
        }
    }
    for mut tc in arrow_q.iter_mut() {
        if tc.0 != icon_target {
            *tc = TextColor(icon_target);
        }
    }
    let shortcut_text_target =
        lerp_color(SHORTCUT_TEXT_INACTIVE, SHORTCUT_TEXT_ACTIVE, active_t);
    for mut tc in shortcut_text_q.iter_mut() {
        if tc.0 != shortcut_text_target {
            *tc = TextColor(shortcut_text_target);
        }
    }
    let badge_target = lerp_color(
        Color::srgba(0.25, 0.28, 0.32, 0.35),
        Color::srgba(0.70, 0.73, 0.76, 0.20),
        active_t,
    );
    for mut badge_bg in shortcut_bg_q.iter_mut() {
        if badge_bg.0 != badge_target {
            *badge_bg = BackgroundColor(badge_target);
        }
    }
}

fn chat_capsule_input(
    mut keyboard_inputs: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    llm: Res<ChatLlmState>,
    mut state: ResMut<ChatCapsuleState>,
    owner: Res<TextInputOwner>,
) {
    if !state.active || !owner.is(TextInputFocus::Chat) {
        keyboard_inputs.clear();
        return;
    }

    if state.composing {
        keyboard_inputs.clear();
        return;
    }

    state.cursor_pos = state
        .cursor_pos
        .min(state.input_text.chars().count());

    for event in keyboard_inputs.read() {
        if !event.state.is_pressed() {
            continue;
        }

        let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        let char_count = state.input_text.chars().count();
        if state.cursor_pos > char_count {
            state.cursor_pos = char_count;
        }

        match &event.logical_key {
            Key::Character(ch) => {
                let Some(raw) = ch.chars().next() else {
                    continue;
                };
                let c = if ctrl && raw.is_ascii_alphabetic() {
                    (raw.to_ascii_uppercase() as u8 & 0x1f) as char
                } else {
                    raw
                };
                if !c.is_control() {
                    let byte_pos = state.input_text.char_indices()
                        .nth(state.cursor_pos)
                        .map(|(i, _)| i)
                        .unwrap_or(state.input_text.len());
                    state.input_text.insert(byte_pos, c);
                    state.cursor_pos = state.cursor_pos.saturating_add(1);
                }
            }
            Key::Space => {
                let byte_pos = state
                    .input_text
                    .char_indices()
                    .nth(state.cursor_pos)
                    .map(|(i, _)| i)
                    .unwrap_or(state.input_text.len());
                state.input_text.insert(byte_pos, ' ');
                state.cursor_pos = state.cursor_pos.saturating_add(1);
            }
            Key::Enter => {
                let input = state.input_text.clone();
                if input.is_empty() || llm.pending {
                    continue;
                }
                state.input_text.clear();
                state.cursor_pos = 0;
                state.submitted = Some(input);
            }
            Key::Backspace => {
                if state.cursor_pos > 0 {
                    state.cursor_pos = state.cursor_pos.saturating_sub(1);
                    if let Some((byte_pos, _)) =
                        state.input_text.char_indices().nth(state.cursor_pos)
                    {
                        state.input_text.remove(byte_pos);
                    }
                }
            }
            Key::Delete => {
                let byte_pos = state.input_text.char_indices()
                    .nth(state.cursor_pos)
                    .map(|(i, _)| i)
                    .unwrap_or(state.input_text.len());
                if byte_pos < state.input_text.len() {
                    state.input_text.remove(byte_pos);
                }
            }
            Key::ArrowLeft => {
                state.cursor_pos = state.cursor_pos.saturating_sub(1);
            }
            Key::ArrowRight => {
                let len = state.input_text.chars().count();
                if state.cursor_pos < len {
                    state.cursor_pos = state.cursor_pos.saturating_add(1);
                }
            }
            Key::Home => state.cursor_pos = 0,
            Key::End => state.cursor_pos = state.input_text.chars().count(),
            _ => {}
        }
    }
}

fn chat_capsule_ime(
    mut ime_events: MessageReader<Ime>,
    mut state: ResMut<ChatCapsuleState>,
    owner: Res<TextInputOwner>,
) {
    if !state.active || !owner.is(TextInputFocus::Chat) {
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

                let pos = state.cursor_pos.min(state.input_text.chars().count());
                let byte_pos = state
                    .input_text
                    .char_indices()
                    .nth(pos)
                    .map(|(i, _)| i)
                    .unwrap_or(state.input_text.len());
                state.input_text.insert_str(byte_pos, value);
                state.cursor_pos = pos.saturating_add(value.chars().count());
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

fn chat_capsule_cursor_blink(time: Res<Time>, mut state: ResMut<ChatCapsuleState>) {
    let t = time.elapsed_secs();
    state.cursor_visible = (t / BLINK_RATE) as i32 % 2 == 0;
}

fn color_components(c: Color) -> (f32, f32, f32, f32) {
    let s = c.to_srgba();
    (s.red, s.green, s.blue, s.alpha)
}

fn lerp_color(from: Color, to: Color, t: f32) -> Color {
    let (fr, fg, fb, fa) = color_components(from);
    let (tr, tg, tb, ta) = color_components(to);
    let t = t.clamp(0.0, 1.0);
    Color::srgba(
        fr + (tr - fr) * t,
        fg + (tg - fg) * t,
        fb + (tb - fb) * t,
        fa + (ta - fa) * t,
    )
}

// ============================
//  AI 回复气泡
// ============================
const BUBBLE_BG: Color = Color::srgb(0.608, 0.784, 0.855);
const BUBBLE_TEXT: Color = Color::srgb(0.09, 0.19, 0.24);
const THINKING_TEXT: &str = "……";
const MAX_BUBBLES: usize = 3;
const BUBBLE_LIFE: f32 = 5.0;
const BUBBLE_FADE: f32 = 0.6;

#[derive(Component)]
struct ChatBubbleRoot;

#[derive(Component)]
struct ChatBubble(usize);

#[derive(Component)]
struct ChatBubbleText(usize);

/// 聊天回复解析出的情绪(供 Live2D 表情桥接)
#[derive(bevy::ecs::message::Message)]
pub struct ChatEmotionEvent(pub String);

const EMOTION_TAGS: &[(&str, &str)] = &[
    ("[开心]", "happy"),
    ("[难过]", "sad"),
    ("[生气]", "angry"),
    ("[惊讶]", "surprised"),
    ("[困惑]", "confused"),
    ("[得意]", "proud"),
    ("[害羞]", "shy"),
    ("[疲惫]", "tired"),
    ("[平静]", "neutral"),
];

const EMOTION_PROMPT: &str = "\n\n【输出格式附加要求】每次回复的最末尾,附加且仅附加一个情绪标签,只能从以下选择:[开心] [难过] [生气] [惊讶] [困惑] [得意] [害羞] [疲惫] [平静]。标签只出现一次,放在整条回复的最后。";

/// Agent 主动轮投递：outbox → inbox（chat 侧）+ emotion 转发 + tool 副作用落事件。
/// outbox 由 `n3ri-agent` 的 `proactive_poll` / 被动 `chat_llm_poll` 写入，
/// 本桥只搬运，不做决策。开窗走 `AppLaunchEvent`（dock reader 消费），
/// 通知走 `NotificationEvent`（agent_bridge 的 notification_sender 消费）。
fn agent_outbox_bridge(
    mut outbox: ResMut<n3ri_agent::AgentOutbox>,
    mut inbox: ResMut<AgentBubbleInbox>,
    mut emotion_events: MessageWriter<ChatEmotionEvent>,
    mut launch_events: MessageWriter<n3ri_core::events::AppLaunchEvent>,
    mut notify_events: MessageWriter<n3ri_core::events::NotificationEvent>,
) {
    if !outbox.dirty() {
        return;
    }
    let (sentences, emotion, effects) = outbox.take_out();
    for s in sentences {
        inbox.items.push_back(s);
    }
    if let Some(e) = emotion {
        emotion_events.write(ChatEmotionEvent(e));
    }
    for effect in effects {
        match effect {
            n3ri_agent::PendingEffect::OpenApp { app_id } => {
                launch_events.write(n3ri_core::events::AppLaunchEvent {
                    app_id,
                    window_title: None,
                });
            }
            n3ri_agent::PendingEffect::Notify { title, message } => {
                notify_events.write(n3ri_core::events::NotificationEvent {
                    title,
                    message,
                    icon: None,
                    duration: None,
                });
            }
        }
    }
}

/// Agent 主动轮投递：inbox → 气泡句子队列（复用逐句揭示/3 条上限/寿命逻辑）。
fn agent_inbox_drain(
    mut inbox: ResMut<AgentBubbleInbox>,
    mut bubbles: ResMut<ChatBubbleState>,
) {
    if inbox.items.is_empty() {
        return;
    }
    for s in inbox.items.drain(..) {
        bubbles.queue.push_back(s);
    }
}

fn extract_emotion(text: &str) -> (String, Option<String>) {
    let mut result = text.to_string();
    let mut emotion: Option<String> = None;
    for (tag, key) in EMOTION_TAGS {
        if result.contains(tag) {
            if emotion.is_none() {
                emotion = Some((*key).to_string());
            }
            result = result.replace(tag, "");
        }
    }
    (result, emotion)
}

#[derive(Default)]
struct BubbleSpring {
    t: Option<f32>,
}

const NORI_SYSTEM_PROMPT: &str = include_str!("../../../assets/prompt/Nori_system_prompt.txt");

fn load_system_prompt() -> Option<String> {
    // 磁盘优先（开发期可改即生效），嵌入发布时兜底编译期内置
    let from_disk = std::fs::read_to_string("assets/prompt/Nori_system_prompt.txt")
        .or_else(|_| std::fs::read_to_string("../../assets/prompt/Nori_system_prompt.txt"));
    Some(from_disk.unwrap_or_else(|_| NORI_SYSTEM_PROMPT.to_string()))
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        cur.push(ch);
        if matches!(ch, '。' | '！' | '？' | '!' | '?' | '…' | '~' | '～') {
            let t = cur.trim().to_string();
            if !t.is_empty() {
                out.push(t);
            }
            cur.clear();
        }
    }
    let t = cur.trim().to_string();
    if !t.is_empty() {
        out.push(t);
    }
    out
}

fn chat_llm_dispatch(
    time: Res<Time>,
    mut state: ResMut<ChatCapsuleState>,
    mut llm: ResMut<ChatLlmState>,
    mut history: ResMut<ChatHistory>,
    mut bubbles: ResMut<ChatBubbleState>,
    view: Res<n3ri_agent::AgentWorldView>,
    snap: Res<n3ri_agent::ContextSnapshot>,
    mut sched: ResMut<n3ri_agent::SchedulerState>,
    mut passive: ResMut<n3ri_agent::PassivePending>,
    hot: Res<n3ri_agent::HotMemory>,
    store: Res<n3ri_agent::MemoryStoreRes>,
    agent_cfg: Res<n3ri_agent::AgentConfig>,
) {
        let g = &mut *bubbles;
    let Some(input) = state.submitted.take() else {
        return;
    };
    if llm.pending {
        return;
    };
    n3ri_agent::turn::note_user_reply_now(&mut sched, time.elapsed_secs_f64());
    passive.0 = true;
    if llm.system_prompt.is_none() {
        llm.system_prompt = load_system_prompt();
    }
    let system_prompt = format!(
        "{}{}",
        llm.system_prompt
            .clone()
            .unwrap_or_else(|| "你是 Nori,一个被困在蓝色数字空间里的白发 AI 女孩。".to_string()),
        EMOTION_PROMPT
    );

    let user_text = input.clone();
    history.messages.push(Message::user(input));
    if history.messages.len() > 20 {
        history.messages.remove(0);
    }

    let mem_block = n3ri_agent::build_memory_block(&store.store, &hot);
    let mem_opt = if mem_block.is_empty() || !agent_cfg.memory_enabled {
        None
    } else {
        Some(mem_block.as_str())
    };
    let mut req = Vec::with_capacity(history.messages.len() + 1);
    req.push(Message::system(n3ri_agent::append_context(
        &system_prompt,
        &view,
        &snap,
        mem_opt,
    )));
    req.extend(history.messages.iter().cloned());

    let (tx, rx) = channel();
    let client = LlmClient::new();
    let cfg = n3ri_llm::load_config();
    // M4：被动轮全量走 tool-loop（recall 由 defs_for 按 memory_enabled 门控，
    // open_app/notify/niri 与记忆开关无关，始终可用）。
    let agent_cfg = agent_cfg.clone();
    thread::spawn(move || {
        let store = n3ri_agent::MemoryStore::load();
        let out = n3ri_agent::turn::run_tool_loop(&client, &req, &cfg, &store, &agent_cfg);
        let _ = tx.send((out.text, user_text, out.effects));
    });
    let born = time.elapsed_secs();
    g.now = born;
    llm.rx = Some(Mutex::new(rx));
    llm.pending = true;
    g.shown.push_back(BubbleEntry {
        text: THINKING_TEXT.to_string(),
        born,
        dying: None,
    });
    while g.shown.len() > MAX_BUBBLES {
        g.shown.pop_front();
    }
    g.timer = 0.5;
}

fn chat_llm_poll(
    mut llm: ResMut<ChatLlmState>,
    mut history: ResMut<ChatHistory>,
    mut bubbles: ResMut<ChatBubbleState>,
    mut emotion_events: MessageWriter<ChatEmotionEvent>,
    mut passive: ResMut<n3ri_agent::PassivePending>,
    mut hot: ResMut<n3ri_agent::HotMemory>,
    mut store: ResMut<n3ri_agent::MemoryStoreRes>,
    mut outbox: ResMut<n3ri_agent::AgentOutbox>,
    agent_cfg: Res<n3ri_agent::AgentConfig>,
) {
        let g = &mut *bubbles;
    if !llm.pending {
        return;
    }
    let Some(rx) = llm.rx.as_ref() else {
        llm.pending = false;
        passive.0 = false;
        return;
    };
    let recv = rx.lock().unwrap().try_recv();
    match recv {
        Ok((Ok(response), user_text, effects)) => {
            let (cleaned, emotion) = extract_emotion(&response);
            if let Some(e) = emotion {
                emotion_events.write(ChatEmotionEvent(e));
            }
            history.messages.push(Message::assistant(cleaned.clone()));
            n3ri_agent::record_turn(&mut hot, &mut store.store, &user_text, &cleaned, &agent_cfg);
            // tool 副作用（open_app/notify）经 outbox 交 agent_outbox_bridge 落事件。
            outbox.push_effects(effects);
            let sentences = split_sentences(&cleaned);
            if sentences.is_empty() {
                g.queue.push_back("……".to_string());
            } else {
                for s in sentences {
                    g.queue.push_back(s);
                }
            }
            llm.pending = false;
            llm.rx = None;
            passive.0 = false;
        }
        Ok((Err(e), _, _)) => {
            if g.shown.back().map(|l| l.text == THINKING_TEXT).unwrap_or(false) {
                g.shown.pop_back();
            }
            g.queue.push_back(format!("呜……信号断断续续的,Nori 连不上。({e})"));
            llm.pending = false;
            llm.rx = None;
            passive.0 = false;
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            llm.pending = false;
            llm.rx = None;
            passive.0 = false;
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
    }
}

fn chat_sentence_reveal(time: Res<Time>, mut bubbles: ResMut<ChatBubbleState>) {
        let g = &mut *bubbles;
    let dt = time.delta_secs();
    g.now = time.elapsed_secs();

    // 老化:超过寿命的气泡进入淡出;"……"思考泡豁免
    for e in g.shown.iter_mut() {
        if e.dying.is_none()
            && e.text != THINKING_TEXT
            && g.now - e.born > BUBBLE_LIFE
        {
            e.dying = Some(0.0);
        }
        if let Some(p) = e.dying.as_mut() {
            *p += dt / BUBBLE_FADE;
        }
    }
    // 淡出完成的最旧气泡移除
    while let Some(front) = g.shown.front() {
        let expired = match front.dying {
            Some(p) => p >= 1.0,
            None => false,
        };
        if expired {
            g.shown.pop_front();
        } else {
            break;
        }
    }

    if g.queue.is_empty() {
        return;
    }
    g.timer -= dt;
    if g.timer > 0.0 {
        return;
    }
    if let Some(s) = g.queue.pop_front() {
        if g.shown.back().map(|l| l.text == THINKING_TEXT).unwrap_or(false) {
            g.shown.pop_back();
        }
        g.shown.push_back(BubbleEntry {
            text: s,
            born: g.now,
            dying: None,
        });
        while g.shown.len() > MAX_BUBBLES {
            g.shown.pop_front();
        }
    }
    let r = ((time.elapsed_secs() * 7919.0).fract().abs() * 997.0) as u64;
    g.timer = 0.35 + (r % 550) as f32 / 1000.0;
}
fn chat_bubble_sync(
    time: Res<Time>,
    rise: Res<ChatRise>,
    mut bubbles: ResMut<ChatBubbleState>,
    mut root_q: Query<(&Children, &mut Node), With<ChatBubbleRoot>>,
    mut bubble_q: Query<(
        &ChatBubble,
        &mut Visibility,
        &mut UiTransform,
        &mut BackgroundColor,
    )>,
    mut text_q: Query<(&ChatBubbleText, &mut Text, &mut TextColor)>,
    mut springs: Local<[BubbleSpring; MAX_BUBBLES]>,
    mut last_texts: Local<[String; MAX_BUBBLES]>,
) {
        let g = &mut *bubbles;
    let dt = time.delta_secs();

    // 老化:超过寿命进入淡出;"……"思考泡豁免
    for e in g.shown.iter_mut() {
        if e.dying.is_none()
            && e.text != THINKING_TEXT
            && g.now - e.born > BUBBLE_LIFE
        {
            e.dying = Some(0.0);
        }
        if let Some(p) = e.dying.as_mut() {
            *p += dt / BUBBLE_FADE;
        }
    }
    // 淡出完成的最旧气泡移除
    while let Some(front) = g.shown.front() {
        let expired = matches!(front.dying, Some(p) if p >= 1.0);
        if expired {
            g.shown.pop_front();
        } else {
            break;
        }
    }

    let n = g.shown.len().min(MAX_BUBBLES);
    let offset = MAX_BUBBLES - n;
    if let Ok((children, mut node)) = root_q.single_mut() {
        let target = Val::Px(64.0 + rise.0);
        if node.bottom != target {
            node.bottom = target;
        }
        for child in children.iter() {
            if let Ok((bubble, mut vis, mut tr, mut bg)) =
                bubble_q.get_mut(child)
            {
                let slot = bubble.0;
                let visible = slot >= offset && slot - offset < n;
                let new_vis = if visible {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                // 仅状态翻转时写:可见↔隐藏必须落到组件,其余帧跳过
                if *vis != new_vis {
                    *vis = new_vis;
                }
                let mut alpha = 1.0;
                let mut ty = 0.0;
                if visible {
                    if let Some(e) = g.shown.get(slot - offset) {
                        if let Some(p) = e.dying {
                            ty = -60.0 * p;
                            alpha = 1.0 - p;
                        }
                        // 新内容触发弹簧
                        if last_texts[slot] != e.text && !e.text.is_empty() {
                            springs[slot].t = Some(0.0);
                        }
                    }
                }
                let s = match springs[slot].t.as_mut() {
                    Some(t) => {
                        *t += dt;
                        if *t >= 0.9 {
                            springs[slot].t = None;
                            1.0
                        } else {
                            1.0 - 0.42 * (-5.0 * *t).exp() * (12.0 * *t).cos()
                        }
                    }
                    None => 1.0,
                };
                let target_translation = Val2::px(0.0, ty);
                if tr.translation != target_translation {
                    tr.translation = target_translation;
                }
                let target_scale = Vec2::splat(s);
                if tr.scale != target_scale {
                    tr.scale = target_scale;
                }
                let target_bg = BUBBLE_BG.with_alpha(alpha);
                if bg.0 != target_bg {
                    *bg = BackgroundColor(target_bg);
                }
                last_texts[slot] = if visible {
                    g.shown
                        .get(slot - offset)
                        .map(|e| e.text.clone())
                        .unwrap_or_default()
                } else {
                    String::new()
                };
            }
        }
    }
    for (ChatBubbleText(idx), mut text, mut color) in text_q.iter_mut() {
        let (mut target, mut alpha) = (String::new(), 1.0_f32);
        if *idx >= offset {
            if let Some(e) = g.shown.get(*idx - offset) {
                target = e.text.clone();
                if let Some(p) = e.dying {
                    alpha = 1.0 - p;
                }
            }
        }
        if **text != target {
            **text = target;
        }
        let target_color = BUBBLE_TEXT.with_alpha(alpha);
        if color.0 != target_color {
            *color = TextColor(target_color);
        }
    }
}
