//! 你画我猜 —— 移植自 draw_and_guess.html
//!
//! 双向绘画游戏：
//! - 偶数回合：玩家画 → Nori 猜（2.0s 首猜，之后 2.8s 一次，12% 概率猜对）
//! - 奇数回合：Nori 画（drawings.json 笔画重放，150ms/笔）→ 玩家猜（文本输入）
//! 页面流程：主菜单 → 时长选择 → 游戏 → 结算

use bevy::asset::RenderAssetUsages;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::{FontSource, FontSize};
use bevy::window::Ime;
use std::collections::HashMap;

use crate::cursor::CursorPosition;
use crate::font::N3riFonts;
use crate::input_focus::{TextInputFocus, TextInputOwner};
use crate::scroll::{ScrollableArea, ScrollContent};
use crate::window::{spawn_window, AppWindow};

// ==================== 常量 ====================

const WIN_W: f32 = 1000.0;
const WIN_H: f32 = 640.0;

/// 画布纹理尺寸（正方形像素数）
const TEX_SIZE: u32 = 512;
const TEX_LEN: usize = (TEX_SIZE * TEX_SIZE * 4) as usize;

/// 纸张底色 / 墨色
const PAPER: [u8; 4] = [0xFF, 0xF8, 0xE7, 0xFF];
const INK: [u8; 4] = [0x36, 0x36, 0x36, 0xFF];

/// 调色板（与参考 HTML 一致）
const PALETTE: [[u8; 4]; 6] = [
    [0x36, 0x36, 0x36, 0xFF],
    [0x4A, 0x55, 0x68, 0xFF],
    [0x9B, 0x6B, 0x5B, 0xFF],
    [0xB8, 0x95, 0x6B, 0xFF],
    [0x5B, 0x7B, 0x6B, 0xFF],
    [0x7B, 0x4B, 0x5B, 0xFF],
];

/// 玩家笔宽 / 橡皮宽 / AI 笔宽（纹理像素）
const PEN_W: f32 = 5.0;
const ERASER_W: f32 = 22.0;
const AI_PEN_W: f32 = 4.0;

/// Nori 猜词节奏（秒）
const NORI_FIRST: f32 = 2.0;
const NORI_INTERVAL: f32 = 2.8;
const NORI_CHANCE_PCT: u64 = 12;

/// AI 绘画：起笔延迟 / 每笔间隔（秒）
const AI_START_DELAY: f32 = 0.8;
const AI_STROKE_SECS: f32 = 0.15;

/// 猜对 / 跳过后进入下一回合的延迟（秒）
const NEXT_ROUND_DELAY: f32 = 1.6;
const SKIP_DELAY: f32 = 0.7;

/// 聊天最多保留条数
const CHAT_MAX: usize = 60;

/// 聊天面板最多显示条数（超出的旧消息不渲染，防止撑走输入行）
const CHAT_VISIBLE: usize = 15;

// 配色
const SURFACE: Color = Color::srgb(0.13, 0.17, 0.26);
const SURFACE_2: Color = Color::srgb(0.16, 0.21, 0.32);
const ACCENT: Color = Color::srgb(0.86, 0.70, 0.40);
const TEXT_MAIN: Color = Color::srgb(0.95, 0.93, 0.88);
const TEXT_DIM: Color = Color::srgb(0.62, 0.66, 0.74);
const TEXT_DARK: Color = Color::srgb(0.16, 0.13, 0.08);

/// 128 个中英词对（英文 key 全部命中 assets/nori/pictionary/drawings.json）
const WORD_PAIRS: [(&str, &str); 128] = [
    ("airplane", "飞机"), ("alarm clock", "闹钟"), ("ambulance", "救护车"), ("angel", "天使"),
    ("ant", "蚂蚁"), ("apple", "苹果"), ("arm", "手臂"), ("asparagus", "芦笋"),
    ("axe", "斧头"), ("backpack", "背包"), ("banana", "香蕉"), ("bandage", "绷带"),
    ("barn", "谷仓"), ("baseball", "棒球"), ("basket", "篮子"), ("basketball", "篮球"),
    ("bat", "蝙蝠"), ("bathtub", "浴缸"), ("beach", "沙滩"), ("bear", "熊"),
    ("beard", "胡子"), ("bed", "床"), ("bee", "蜜蜂"), ("belt", "腰带"),
    ("bench", "长椅"), ("bicycle", "自行车"), ("binoculars", "望远镜"), ("bird", "鸟"),
    ("birthday cake", "生日蛋糕"), ("blackberry", "黑莓"), ("blueberry", "蓝莓"), ("book", "书"),
    ("boomerang", "回力镖"), ("bottlecap", "瓶盖"), ("bowtie", "蝴蝶结"), ("bracelet", "手链"),
    ("brain", "大脑"), ("bread", "面包"), ("bridge", "桥"), ("broccoli", "西兰花"),
    ("broom", "扫帚"), ("bucket", "水桶"), ("bus", "公交车"), ("butterfly", "蝴蝶"),
    ("cactus", "仙人掌"), ("cake", "蛋糕"), ("calculator", "计算器"), ("calendar", "日历"),
    ("camel", "骆驼"), ("camera", "相机"), ("candle", "蜡烛"), ("cannon", "大炮"),
    ("car", "汽车"), ("cat", "猫"), ("ceiling fan", "吊扇"), ("cell phone", "手机"),
    ("chair", "椅子"), ("chandelier", "吊灯"), ("church", "教堂"), ("circle", "圆形"),
    ("clock", "时钟"), ("cloud", "云"), ("coffee cup", "咖啡杯"), ("compass", "指南针"),
    ("computer", "电脑"), ("cookie", "饼干"), ("couch", "沙发"), ("cow", "奶牛"),
    ("crab", "螃蟹"), ("crayon", "蜡笔"), ("crocodile", "鳄鱼"), ("crown", "皇冠"),
    ("cup", "杯子"), ("diamond", "钻石"), ("dog", "狗"), ("dolphin", "海豚"),
    ("donut", "甜甜圈"), ("door", "门"), ("dragon", "龙"), ("drill", "电钻"),
    ("drums", "架子鼓"), ("duck", "鸭子"), ("dumbbell", "哑铃"), ("ear", "耳朵"),
    ("elephant", "大象"), ("envelope", "信封"), ("eraser", "橡皮擦"), ("eye", "眼睛"),
    ("eyeglasses", "眼镜"), ("fence", "栅栏"), ("finger", "手指"), ("fire hydrant", "消防栓"),
    ("fireplace", "壁炉"), ("fish", "鱼"), ("flashlight", "手电筒"), ("floor lamp", "落地灯"),
    ("flower", "花"), ("flying saucer", "飞碟"), ("foot", "脚"), ("fork", "叉子"),
    ("frog", "青蛙"), ("garden", "花园"), ("giraffe", "长颈鹿"), ("golf club", "高尔夫球杆"),
    ("grapes", "葡萄"), ("grass", "草"), ("guitar", "吉他"), ("hamburger", "汉堡"),
    ("hammer", "锤子"), ("hand", "手"), ("hat", "帽子"), ("headphones", "耳机"),
    ("helicopter", "直升机"), ("helmet", "头盔"), ("hexagon", "六边形"), ("hockey stick", "曲棍球棒"),
    ("horse", "马"), ("hospital", "医院"), ("hot air balloon", "热气球"), ("hot dog", "热狗"),
    ("hourglass", "沙漏"), ("house", "房子"), ("ice cream", "冰淇淋"), ("jacket", "夹克"),
    ("jail", "监狱"), ("kangaroo", "袋鼠"), ("key", "钥匙"), ("keyboard", "键盘"),
];

// ==================== 数据模型 ====================

/// drawings.json：词 → 样本(10) → 笔画 → [xs, ys]
type DrawingsDb = HashMap<String, Vec<Vec<Vec<Vec<u8>>>>>;

/// 绘画数据库（开始游戏时懒加载）
#[derive(Resource, Default)]
struct DrawingsState(Option<DrawingsDb>);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PicPage {
    Menu,
    Duration,
    Game,
    Result,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PicPhase {
    /// 玩家画，Nori 猜
    PlayerDraws,
    /// Nori 绘画动画中
    NoriDraws,
    /// Nori 画完，玩家猜词
    PlayerGuesses,
}

struct WordRecord {
    word: String,
    who: &'static str,
    time: f32,
    correct: bool,
}

#[derive(Clone, Copy)]
enum ChatKind {
    System,
    Guess,
    Correct,
    Player,
}

struct ChatMsg {
    kind: ChatKind,
    text: String,
}

/// 画布纹理句柄（启动后创建一次，窗口重开复用）
#[derive(Resource, Default)]
struct PicCanvasTex(Handle<Image>);

#[derive(Resource)]
struct PictionaryGame {
    page: PicPage,
    phase: PicPhase,
    game_active: bool,
    round_active: bool,
    duration: u32,
    time_left: f32,
    /// 当前回合已耗时
    round_elapsed: f32,
    score: u32,
    total_rounds: u32,
    correct_rounds: u32,
    round_times: Vec<f32>,
    word_history: Vec<WordRecord>,
    used_words: Vec<String>,
    word_zh: String,
    word_en: String,
    /// 画笔状态
    color_idx: usize,
    is_eraser: bool,
    is_drawing: bool,
    last_pos: Option<Vec2>,
    /// 画布清空请求
    clear_canvas: bool,
    /// Nori 猜词计时
    nori_timer: f32,
    /// AI 绘画动画
    ai_strokes: Vec<Vec<Vec2>>,
    ai_stroke_idx: usize,
    ai_timer: f32,
    ai_started: bool,
    /// 猜词输入
    input_buf: String,
    hint_flash: f32,
    /// 聊天
    chat: Vec<ChatMsg>,
    chat_dirty: bool,
    /// 回合过渡倒计时
    transition: Option<f32>,
    /// 结算列表需要重建
    result_dirty: bool,
    rng: u64,
}

impl Default for PictionaryGame {
    fn default() -> Self {
        Self::new()
    }
}

impl PictionaryGame {
    fn new() -> Self {
        Self {
            page: PicPage::Menu,
            phase: PicPhase::PlayerDraws,
            game_active: false,
            round_active: false,
            duration: 180,
            time_left: 180.0,
            round_elapsed: 0.0,
            score: 0,
            total_rounds: 0,
            correct_rounds: 0,
            round_times: Vec::new(),
            word_history: Vec::new(),
            used_words: Vec::new(),
            word_zh: String::new(),
            word_en: String::new(),
            color_idx: 0,
            is_eraser: false,
            is_drawing: false,
            last_pos: None,
            clear_canvas: false,
            nori_timer: 0.0,
            ai_strokes: Vec::new(),
            ai_stroke_idx: 0,
            ai_timer: 0.0,
            ai_started: false,
            input_buf: String::new(),
            hint_flash: 0.0,
            chat: Vec::new(),
            chat_dirty: false,
            transition: None,
            result_dirty: false,
            rng: Self::seed(),
        }
    }

    fn seed() -> u64 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        nanos | 1
    }

    /// xorshift64*
    fn rand(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn rand_index(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.rand() % n as u64) as usize
        }
    }

    fn push_chat(&mut self, kind: ChatKind, text: impl Into<String>) {
        self.chat.push(ChatMsg { kind, text: text.into() });
        if self.chat.len() > CHAT_MAX {
            let overflow = self.chat.len() - CHAT_MAX;
            self.chat.drain(0..overflow);
        }
        self.chat_dirty = true;
    }
}

// ==================== 组件标记 ====================

/// 需要实时刷新的文本节点
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum PicText {
    Timer,
    Turn,
    Word,
    Score,
    Input,
}

/// 页面根容器
#[derive(Component, Clone, Copy)]
struct PicPageRoot(PicPage);

/// 画布节点（ImageNode，鼠标绘制目标）
#[derive(Component)]
struct PicCanvas;

#[derive(Component)]
struct PicColorBtn(usize);

#[derive(Component)]
struct PicEraserBtn;

#[derive(Component)]
struct PicClearBtn;

#[derive(Component)]
struct PicSkipBtn;

#[derive(Component)]
struct PicSendBtn;

/// 猜词输入框（pub：input_focus 定位 IME 用）
#[derive(Component)]
pub struct PicGuessInput;

/// 聊天列表容器
#[derive(Component)]
struct PicChatList;

#[derive(Component)]
struct PicMenuStartBtn;

#[derive(Component)]
struct PicDurBtn(u32);

#[derive(Component)]
struct PicStartGameBtn;

#[derive(Component)]
struct PicBackMenuBtn;

#[derive(Component)]
struct PicReplayBtn;

#[derive(Component)]
struct PicExitGameBtn;

/// 结算页统计文本
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum PicResultText {
    Score,
    Total,
    Duration,
    Accuracy,
    Fastest,
    Average,
}

/// 结算词汇列表容器
#[derive(Component)]
struct PicResultList;

// ==================== 插件 ====================

pub struct PictionaryPlugin;

impl Plugin for PictionaryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PicCanvasTex>()
            .init_resource::<DrawingsState>()
            .init_resource::<PictionaryGame>()
            .add_systems(
                Update,
                (
                    pic_actions_nav,
                    pic_actions_tools,
                    pic_draw,
                    pic_guess_input,
                    pic_nori_guess,
                    pic_ai_draw,
                    pic_tick,
                    pic_ui_update,
                    pic_chat_sync,
                    pic_result_sync,
                    pic_page_sync,
                    pic_cleanup,
                    pic_init_canvas,
                ),
            );
    }
}

// ==================== 入口（dock 启动） ====================

pub fn spawn_pictionary(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    let window_e = spawn_window(parent, "你画我猜", "pictionary", WIN_W, WIN_H, fonts);
    parent.commands().entity(window_e).with_children(|win| {
        spawn_menu_page(win, fonts);
        spawn_duration_page(win, fonts);
        spawn_game_page(win, fonts);
        spawn_result_page(win, fonts);
    });
}

// ==================== 游戏流程 ====================

const DRAWINGS_JSON: &str = include_str!("../../../../assets/nori/pictionary/drawings.json");

/// 加载 drawings.json（兼容两种运行目录;嵌入发布时兜底编译期内置）
fn load_drawings() -> Option<DrawingsDb> {
    for path in [
        "../../assets/nori/pictionary/drawings.json",
        "assets/nori/pictionary/drawings.json",
    ] {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(db) = serde_json::from_str::<DrawingsDb>(&text) {
                return Some(db);
            }
        }
    }
    serde_json::from_str::<DrawingsDb>(DRAWINGS_JSON).ok()
}

fn start_game(game: &mut PictionaryGame, db: &mut DrawingsState) {
    if db.0.is_none() {
        db.0 = load_drawings();
    }
    game.time_left = game.duration as f32;
    game.score = 0;
    game.total_rounds = 0;
    game.correct_rounds = 0;
    game.round_times.clear();
    game.word_history.clear();
    game.used_words.clear();
    game.transition = None;
    game.result_dirty = false;
    game.chat.clear();
    game.chat_dirty = true;
    game.push_chat(
        ChatKind::System,
        match db.0 {
            Some(_) => "🎨 绘画数据已加载，游戏开始！",
            None => "⚠️ 绘画数据未加载，Nori 回合将无法作画",
        },
    );
    game.game_active = true;
    game.page = PicPage::Game;
    start_new_round(game, db);
}

/// 开始新的一轮（轮流：偶数回合玩家画 / 奇数回合 Nori 画）
fn start_new_round(game: &mut PictionaryGame, db: &DrawingsState) {
    if !game.game_active {
        return;
    }
    let pair = pick_word(game);
    game.word_zh = pair.1.to_string();
    game.word_en = pair.0.to_string();
    game.round_active = true;
    game.round_elapsed = 0.0;
    game.input_buf.clear();
    game.hint_flash = 0.0;
    game.is_drawing = false;
    game.last_pos = None;
    game.clear_canvas = true;
    if game.total_rounds.is_multiple_of(2) {
        game.phase = PicPhase::PlayerDraws;
        game.nori_timer = NORI_FIRST;
        let msg = format!("🎯 画「{}」", game.word_zh);
        game.push_chat(ChatKind::System, msg);
    } else {
        game.phase = PicPhase::NoriDraws;
        game.ai_started = false;
        game.ai_timer = AI_START_DELAY;
        game.ai_stroke_idx = 0;
        let strokes = build_ai_strokes(game, db);
        game.ai_strokes = strokes;
        game.push_chat(ChatKind::System, "🎨 Nori 正在画画…");
    }
}

/// 抽一个未用过的词对
fn pick_word(game: &mut PictionaryGame) -> (&'static str, &'static str) {
    let mut candidates: Vec<usize> = (0..WORD_PAIRS.len())
        .filter(|i| !game.used_words.iter().any(|w| w == WORD_PAIRS[*i].0))
        .collect();
    if candidates.is_empty() {
        game.used_words.clear();
        candidates = (0..WORD_PAIRS.len()).collect();
    }
    let idx = candidates[game.rand_index(candidates.len())];
    game.used_words.push(WORD_PAIRS[idx].0.to_string());
    WORD_PAIRS[idx]
}

/// 从 drawings.json 取随机样本，缩放到画布纹理坐标（同参考 HTML 公式）
fn build_ai_strokes(game: &mut PictionaryGame, db: &DrawingsState) -> Vec<Vec<Vec2>> {
    let mut out = Vec::new();
    if let Some(db) = &db.0 {
        if let Some(samples) = db.get(&game.word_en) {
            if !samples.is_empty() {
                let sample = &samples[game.rand_index(samples.len())];
                let scale = (TEX_SIZE as f32 / 300.0) * 0.8;
                let off = (TEX_SIZE as f32 - 255.0 * scale) * 0.5;
                for stroke in sample {
                    let (Some(xs), Some(ys)) = (stroke.first(), stroke.get(1)) else {
                        continue;
                    };
                    let pts: Vec<Vec2> = xs
                        .iter()
                        .zip(ys.iter())
                        .map(|(x, y)| Vec2::new(off + *x as f32 * scale, off + *y as f32 * scale))
                        .collect();
                    if pts.len() >= 2 {
                        out.push(pts);
                    }
                }
            }
        }
    }
    out
}

/// 猜对：计分 + 记录 + 过渡到下一回合
fn record_correct(game: &mut PictionaryGame, who: &'static str) {
    game.score += 1;
    game.correct_rounds += 1;
    game.round_times.push(game.round_elapsed);
    game.word_history.push(WordRecord {
        word: game.word_zh.clone(),
        who,
        time: game.round_elapsed,
        correct: true,
    });
    game.total_rounds += 1;
    game.round_active = false;
    game.transition = Some(NEXT_ROUND_DELAY);
    game.input_buf.clear();
}

/// 失败记录（跳过 / 时间到）
fn record_fail(game: &mut PictionaryGame, who: &'static str) {
    game.word_history.push(WordRecord {
        word: game.word_zh.clone(),
        who,
        time: game.round_elapsed,
        correct: false,
    });
    game.total_rounds += 1;
    game.round_active = false;
    game.input_buf.clear();
}

/// 时间到 → 结算
fn end_game(game: &mut PictionaryGame) {
    if !game.game_active {
        return;
    }
    // 只记录时间到时仍在进行的回合（猜对后的过渡期不重复记失败）
    let round_was_active = game.round_active;
    game.game_active = false;
    game.round_active = false;
    game.transition = None;
    if round_was_active && !game.word_zh.is_empty() {
        let who = if game.phase == PicPhase::PlayerDraws { "Nori" } else { "你" };
        record_fail(game, who);
    }
    game.page = PicPage::Result;
    game.result_dirty = true;
    game.push_chat(ChatKind::System, "🏁 时间到！");
    let msg = format!("最终得分：{}", game.score);
    game.push_chat(ChatKind::System, msg);
}

/// 提交猜词（玩家）
fn submit_guess(game: &mut PictionaryGame) {
    if !game.round_active || game.phase != PicPhase::PlayerGuesses {
        return;
    }
    let trimmed = game.input_buf.trim().to_string();
    if trimmed.is_empty() {
        return;
    }
    game.push_chat(ChatKind::Player, trimmed.clone());
    game.input_buf.clear();
    if trimmed == game.word_zh {
        let msg = format!("🎉 你猜对了！就是「{}」！", game.word_zh);
        game.push_chat(ChatKind::Correct, msg);
        record_correct(game, "你");
    } else {
        game.hint_flash = 1.5;
    }
}

// ==================== UI 构建 ====================

fn ui_font(fonts: &N3riFonts) -> FontSource {
    FontSource::Handle(fonts.default.clone())
}

fn spawn_text(
    parent: &mut ChildSpawnerCommands,
    s: &str,
    size: f32,
    color: Color,
    fonts: &N3riFonts,
) {
    parent.spawn((
        Text::new(s),
        TextFont {
            font: ui_font(fonts),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    ));
}

fn tool_btn(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    fonts: &N3riFonts,
) -> Entity {
    parent
        .spawn((
            Button,
            Node {
                height: Val::Px(30.0),
                padding: UiRect::horizontal(Val::Px(14.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(SURFACE_2),
        ))
        .with_children(|b| {
            spawn_text(b, label, 14.0, TEXT_MAIN, fonts);
        })
        .id()
}

fn page_node() -> Node {
    Node {
        width: Val::Percent(100.0),
        flex_grow: 1.0,
        display: Display::None, // page_sync 按当前页面切换为 Flex
        ..default()
    }
}

// ---------- 主菜单页 ----------

fn spawn_menu_page(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    parent
        .spawn((
            PicPageRoot(PicPage::Menu),
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(14.0),
                ..page_node()
            },
        ))
        .with_children(|page| {
            spawn_text(page, "你画我猜", 48.0, TEXT_MAIN, fonts);
            spawn_text(page, "与 Nori 一起", 16.0, TEXT_DIM, fonts);
            page.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(16.0),
                margin: UiRect::top(Val::Px(6.0)),
                ..default()
            })
            .with_children(|opts| {
                option_card(opts, "✏️", "画画", "你画，Nori 猜", fonts);
                option_card(opts, "👀", "猜词", "Nori 画，你猜", fonts);
            });
            page.spawn((
                Button,
                PicMenuStartBtn,
                Node {
                    width: Val::Px(220.0),
                    height: Val::Px(52.0),
                    margin: UiRect::top(Val::Px(10.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border_radius: BorderRadius::all(Val::Px(10.0)),
                    ..default()
                },
                BackgroundColor(ACCENT),
            ))
            .with_children(|b| {
                spawn_text(b, "→ 开始", 20.0, TEXT_DARK, fonts);
            });
        });
}

fn option_card(
    parent: &mut ChildSpawnerCommands,
    icon: &str,
    title: &str,
    desc: &str,
    fonts: &N3riFonts,
) {
    parent
        .spawn((
            Node {
                width: Val::Px(200.0),
                padding: UiRect::all(Val::Px(14.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(4.0),
                border_radius: BorderRadius::all(Val::Px(10.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(SURFACE),
            BorderColor::all(Color::srgba(0.55, 0.60, 0.70, 0.35)),
        ))
        .with_children(|card| {
            spawn_text(card, icon, 26.0, TEXT_MAIN, fonts);
            spawn_text(card, title, 17.0, TEXT_MAIN, fonts);
            spawn_text(card, desc, 12.0, TEXT_DIM, fonts);
        });
}

// ---------- 时长选择页 ----------

fn spawn_duration_page(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    parent
        .spawn((
            PicPageRoot(PicPage::Duration),
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(12.0),
                ..page_node()
            },
        ))
        .with_children(|page| {
            spawn_text(page, "选择游戏时长", 28.0, TEXT_MAIN, fonts);
            for (secs, label) in [(120u32, "2 分钟"), (180, "3 分钟"), (300, "5 分钟")] {
                page.spawn((
                    Button,
                    PicDurBtn(secs),
                    Node {
                        width: Val::Px(220.0),
                        height: Val::Px(46.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(Val::Px(10.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(SURFACE_2),
                    BorderColor::all(Color::srgba(0.55, 0.60, 0.70, 0.35)),
                ))
                .with_children(|b| {
                    spawn_text(b, label, 18.0, TEXT_MAIN, fonts);
                });
            }
            page.spawn((
                Button,
                PicStartGameBtn,
                Node {
                    width: Val::Px(220.0),
                    height: Val::Px(48.0),
                    margin: UiRect::top(Val::Px(6.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border_radius: BorderRadius::all(Val::Px(10.0)),
                    ..default()
                },
                BackgroundColor(ACCENT),
            ))
            .with_children(|b| {
                spawn_text(b, "开始游戏 →", 18.0, TEXT_DARK, fonts);
            });
            page.spawn((
                Button,
                PicBackMenuBtn,
                Node {
                    width: Val::Px(220.0),
                    height: Val::Px(36.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border_radius: BorderRadius::all(Val::Px(10.0)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ))
            .with_children(|b| {
                spawn_text(b, "← 返回", 15.0, TEXT_DIM, fonts);
            });
        });
}

// ---------- 游戏页 ----------

fn spawn_game_page(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    parent
        .spawn((
            PicPageRoot(PicPage::Game),
            Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(10.0)),
                row_gap: Val::Px(8.0),
                ..page_node()
            },
        ))
        .with_children(|root| {
            // 顶部信息栏（全宽，右侧留给面板自然让位）
            root.spawn(Node {
                height: Val::Px(56.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::horizontal(Val::Px(18.0)),
                border_radius: BorderRadius::all(Val::Px(10.0)),
                ..default()
            })
            .with_children(|bar| {
                bar.spawn((
                    PicText::Timer,
                    Text::new("03:00"),
                    TextFont {
                        font: ui_font(fonts),
                        font_size: FontSize::Px(24.0),
                        ..default()
                    },
                    TextColor(ACCENT),
                ));
                bar.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(2.0),
                    ..default()
                })
                .with_children(|center| {
                    center.spawn((
                        PicText::Turn,
                        Text::new("轮到你画画了"),
                        TextFont {
                            font: ui_font(fonts),
                            font_size: FontSize::Px(14.0),
                            ..default()
                        },
                        TextColor(TEXT_DIM),
                    ));
                    center.spawn((
                        PicText::Word,
                        Text::new(""),
                        TextFont {
                            font: ui_font(fonts),
                            font_size: FontSize::Px(22.0),
                            ..default()
                        },
                        TextColor(TEXT_MAIN),
                    ));
                });
                bar.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::FlexEnd,
                    row_gap: Val::Px(2.0),
                    ..default()
                })
                .with_children(|score_col| {
                    spawn_text(score_col, "得分", 12.0, TEXT_DIM, fonts);
                    score_col.spawn((
                        PicText::Score,
                        Text::new("0"),
                        TextFont {
                            font: ui_font(fonts),
                            font_size: FontSize::Px(24.0),
                            ..default()
                        },
                        TextColor(ACCENT),
                    ));
                });
            });

            // 工具栏（全宽）
            root.spawn(Node {
                height: Val::Px(38.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(10.0),
                ..default()
            })
            .with_children(|bar| {
                for i in 0..6 {
                    let [r, g, b, _] = PALETTE[i];
                    bar.spawn((
                        Button,
                        PicColorBtn(i),
                        Node {
                            width: Val::Px(26.0),
                            height: Val::Px(26.0),
                            border_radius: BorderRadius::all(Val::Px(13.0)),
                            border: UiRect::all(Val::Px(2.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb_u8(r, g, b)),
                        BorderColor::all(Color::srgba(0.85, 0.85, 0.90, 0.35)),
                    ));
                }
                bar.spawn((
                    Node {
                        width: Val::Px(1.0),
                        height: Val::Px(22.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.5, 0.55, 0.65, 0.3)),
                ));
                let eraser_e = tool_btn(bar, "橡皮", fonts);
                bar.commands().entity(eraser_e).insert(PicEraserBtn);
                let clear_e = tool_btn(bar, "清空", fonts);
                bar.commands().entity(clear_e).insert(PicClearBtn);
                let skip_e = tool_btn(bar, "跳过", fonts);
                bar.commands().entity(skip_e).insert(PicSkipBtn);
            });

            // 主内容行：左侧画布 + 右侧聊天面板
            root.spawn(Node {
                flex_grow: 1.0,
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(8.0),
                min_height: Val::Px(0.0),
                ..default()
            })
            .with_children(|row| {
                // 左侧画布区（flex_grow 撑满剩余宽度）
                row.spawn(Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    border_radius: BorderRadius::all(Val::Px(10.0)),
                    overflow: Overflow::hidden(),
                    ..default()
                })
                .with_children(|wrap| {
                    wrap.spawn((
                        PicCanvas,
                        ImageNode::default(),
                        Node {
                            width: Val::Percent(100.0),
                    height: Val::Px(480.0),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.05, 0.07, 0.12)),
                    ));
                });

                // 右侧聊天面板（固定 300px，flex_shrink:0 不被压缩）
                // 参照 signal app 布局：面板 height:100% + ScrollableArea + 固定输入行
                row.spawn(Node {
                    width: Val::Px(300.0),
                    height: Val::Percent(100.0),
                    flex_shrink: 0.0,
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(8.0)),
                    border_radius: BorderRadius::all(Val::Px(10.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                })
                .with_children(|panel| {
                    // 消息滚动区（参照 signal: ScrollableArea + flex_grow:1）
                    let area_e = panel
                        .spawn((
                            ScrollableArea,
                            Node {
                                width: Val::Percent(100.0),
                                flex_grow: 1.0,
                                min_height: Val::Px(0.0),
                                align_items: AlignItems::FlexStart,
                                overflow: Overflow::hidden(),
                                ..default()
                            },
                        ))
                        .id();

                    panel.commands().entity(area_e).with_children(|a| {
                        crate::scroll::spawn_scrollbar(a, area_e);
                    });

                    panel.commands().entity(area_e).with_children(|area| {
                        area.spawn((
                            PicChatList,
                            ScrollContent,
                            Node {
                                width: Val::Percent(100.0),
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(4.0),
                                padding: UiRect::all(Val::Px(4.0)),
                                ..default()
                            },
                        ));
                    });

                    // 猜词输入行（固定 46px，参照 signal input bar）
                    panel.spawn(Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(46.0),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(4.0)),
                        column_gap: Val::Px(8.0),
                        ..default()
                    })
                    .with_children(|input_row| {
                        input_row.spawn((
                            Button,
                            PicGuessInput,
                            Node {
                                flex_grow: 1.0,
                                height: Val::Px(34.0),
                                align_items: AlignItems::Center,
                                padding: UiRect::horizontal(Val::Px(12.0)),
                                border_radius: BorderRadius::all(Val::Px(17.0)),
                                border: UiRect::all(Val::Px(1.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.1, 0.15, 0.21, 0.8)),
                            BorderColor::all(Color::srgba(0.55, 0.60, 0.70, 0.35)),
                        ))
                        .with_children(|input| {
                            input.spawn((
                                PicText::Input,
                                Text::new("💡 输入你猜的词"),
                                TextFont {
                                    font: ui_font(fonts),
                                    font_size: FontSize::Px(14.0),
                                    ..default()
                                },
                                TextColor(TEXT_DIM),
                            ));
                        });
                        input_row.spawn((
                            Button,
                            PicSendBtn,
                            Node {
                                width: Val::Px(46.0),
                                height: Val::Px(34.0),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                border_radius: BorderRadius::all(Val::Px(17.0)),
                                ..default()
                            },
                            BackgroundColor(ACCENT),
                        ))
                        .with_children(|b| {
                            spawn_text(b, "发送", 14.0, TEXT_DARK, fonts);
                        });
                    });
                });
            });
        });
}

// ---------- 结算页 ----------

fn spawn_result_page(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    parent
        .spawn((
            PicPageRoot(PicPage::Result),
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(10.0),
                padding: UiRect::all(Val::Px(24.0)),
                ..page_node()
            },
        ))
        .with_children(|page| {
            spawn_text(page, "游戏结束", 32.0, TEXT_MAIN, fonts);
            page.spawn((
                PicResultText::Score,
                Text::new("得分：0"),
                TextFont {
                    font: ui_font(fonts),
                    font_size: FontSize::Px(40.0),
                    ..default()
                },
                TextColor(ACCENT),
            ));
            page.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(22.0),
                ..default()
            })
            .with_children(|stats| {
                for marker in [
                    PicResultText::Total,
                    PicResultText::Duration,
                    PicResultText::Accuracy,
                    PicResultText::Fastest,
                    PicResultText::Average,
                ] {
                    stats.spawn((
                        marker,
                        Text::new("—"),
                        TextFont {
                            font: ui_font(fonts),
                            font_size: FontSize::Px(15.0),
                            ..default()
                        },
                        TextColor(TEXT_DIM),
                    ));
                }
            });
            let area_e = page
                .spawn((
                    ScrollableArea,
                    Node {
                        width: Val::Px(480.0),
                        height: Val::Px(200.0),
                        align_items: AlignItems::FlexStart,
                        overflow: Overflow::hidden(),
                        border_radius: BorderRadius::all(Val::Px(10.0)),
                        ..default()
                    },
                ))
                .id();
            page.commands().entity(area_e).with_children(|a| {
                crate::scroll::spawn_scrollbar(a, area_e);
            });
            page.commands().entity(area_e).with_children(|area| {
                area.spawn((
                    PicResultList,
                    ScrollContent,
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(4.0),
                        padding: UiRect::all(Val::Px(10.0)),
                        ..default()
                    },
                    BackgroundColor(SURFACE),
                ));
            });
            page.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(16.0),
                margin: UiRect::top(Val::Px(6.0)),
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    Button,
                    PicReplayBtn,
                    Node {
                        width: Val::Px(160.0),
                        height: Val::Px(44.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(Val::Px(10.0)),
                        ..default()
                    },
                    BackgroundColor(ACCENT),
                ))
                .with_children(|b| {
                    spawn_text(b, "再玩一次", 16.0, TEXT_DARK, fonts);
                });
                row.spawn((
                    Button,
                    PicExitGameBtn,
                    Node {
                        width: Val::Px(160.0),
                        height: Val::Px(44.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(Val::Px(10.0)),
                        ..default()
                    },
                    BackgroundColor(SURFACE_2),
                ))
                .with_children(|b| {
                    spawn_text(b, "返回菜单", 16.0, TEXT_MAIN, fonts);
                });
            });
        });
}

// ==================== 画布初始化与像素工具 ====================

/// 创建画布纹理并绑定到 ImageNode（幂等）
fn pic_init_canvas(
    mut canvas: ResMut<PicCanvasTex>,
    mut images: ResMut<Assets<Image>>,
    mut nodes: Query<&mut ImageNode, With<PicCanvas>>,
) {
    if !canvas.0.is_strong() {
        let mut data = vec![0u8; TEX_LEN];
        fill_paper(&mut data);
        let image = Image::new(
            Extent3d {
                width: TEX_SIZE,
                height: TEX_SIZE,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        canvas.0 = images.add(image);
    }
    for mut node in nodes.iter_mut() {
        if node.image != canvas.0 {
            node.image = canvas.0.clone();
        }
    }
}

/// 整张刷成纸色
fn fill_paper(data: &mut [u8]) {
    for px in data.as_chunks_mut::<4>().0 {
        px.copy_from_slice(&PAPER);
    }
}

/// 画一个实心圆点（越界部分裁剪）
fn stamp_circle(data: &mut [u8], cx: f32, cy: f32, r: f32, color: [u8; 4]) {
    let r2 = r * r;
    let x0 = (cx - r).floor() as i32;
    let x1 = (cx + r).ceil() as i32;
    let y0 = (cy - r).floor() as i32;
    let y1 = (cy + r).ceil() as i32;
    for y in y0..=y1 {
        if y < 0 || y >= TEX_SIZE as i32 {
            continue;
        }
        for x in x0..=x1 {
            if x < 0 || x >= TEX_SIZE as i32 {
                continue;
            }
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            if dx * dx + dy * dy <= r2 {
                let idx = ((y as u32 * TEX_SIZE + x as u32) * 4) as usize;
                data[idx..idx + 4].copy_from_slice(&color);
            }
        }
    }
}

/// 画一条带圆头的线段
fn draw_line(data: &mut [u8], x0: f32, y0: f32, x1: f32, y1: f32, color: [u8; 4], width: f32) {
    let dist = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
    let steps = (dist / 1.5).ceil().max(1.0) as usize;
    let r = (width * 0.5).max(0.75);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        stamp_circle(data, x0 + (x1 - x0) * t, y0 + (y1 - y0) * t, r, color);
    }
}

fn chat_color(kind: ChatKind) -> Color {
    match kind {
        ChatKind::System => Color::srgb(0.60, 0.65, 0.74),
        ChatKind::Guess => Color::srgb(0.49, 0.64, 0.85),
        ChatKind::Correct => Color::srgb(0.44, 0.75, 0.56),
        ChatKind::Player => Color::srgb(0.91, 0.89, 0.83),
    }
}

fn format_time(secs: f32) -> String {
    let s = secs.max(0.0).ceil() as u32;
    format!("{:02}:{:02}", s / 60, s % 60)
}

// ==================== 系统 ====================

/// 按钮点击：页面导航（主菜单 / 时长 / 开始 / 返回 / 再玩 / 退出）
#[allow(clippy::too_many_arguments)]
fn pic_actions_nav(
    mouse: Res<ButtonInput<MouseButton>>,
    focused: Res<crate::topbar::FocusedTitle>,
    mut game: ResMut<PictionaryGame>,
    mut db: ResMut<DrawingsState>,
    q_menu: Query<&Interaction, With<PicMenuStartBtn>>,
    q_dur: Query<(&PicDurBtn, &Interaction)>,
    q_start: Query<&Interaction, With<PicStartGameBtn>>,
    q_back: Query<&Interaction, With<PicBackMenuBtn>>,
    q_replay: Query<&Interaction, With<PicReplayBtn>>,
    q_exit: Query<&Interaction, With<PicExitGameBtn>>,
) {
    if !mouse.just_pressed(MouseButton::Left) || focused.title != "你画我猜" {
        return;
    }
    for i in q_menu.iter() {
        if *i == Interaction::Pressed {
            game.page = PicPage::Duration;
        }
    }
    for (btn, i) in q_dur.iter() {
        if *i == Interaction::Pressed {
            game.duration = btn.0;
        }
    }
    for i in q_start.iter() {
        if *i == Interaction::Pressed && game.page == PicPage::Duration {
            start_game(&mut game, &mut db);
        }
    }
    for i in q_back.iter() {
        if *i == Interaction::Pressed {
            game.page = PicPage::Menu;
        }
    }
    for i in q_replay.iter() {
        if *i == Interaction::Pressed {
            game.page = PicPage::Duration;
        }
    }
    for i in q_exit.iter() {
        if *i == Interaction::Pressed {
            game.game_active = false;
            game.round_active = false;
            game.transition = None;
            game.page = PicPage::Menu;
        }
    }
}

/// 按钮点击：画笔 / 橡皮 / 清空 / 跳过 / 发送 / 输入框聚焦
#[allow(clippy::too_many_arguments)]
fn pic_actions_tools(
    mouse: Res<ButtonInput<MouseButton>>,
    focused: Res<crate::topbar::FocusedTitle>,
    mut game: ResMut<PictionaryGame>,
    mut owner: ResMut<TextInputOwner>,
    q_color: Query<(&PicColorBtn, &Interaction)>,
    q_eraser: Query<&Interaction, With<PicEraserBtn>>,
    q_clear: Query<&Interaction, With<PicClearBtn>>,
    q_skip: Query<&Interaction, With<PicSkipBtn>>,
    q_send: Query<&Interaction, With<PicSendBtn>>,
    q_input: Query<&Interaction, With<PicGuessInput>>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let mut clicked_input = false;
    if focused.title == "你画我猜" {
        for (idx, i) in q_color.iter() {
            if *i == Interaction::Pressed {
                game.color_idx = idx.0;
                game.is_eraser = false;
            }
        }
        for i in q_eraser.iter() {
            if *i == Interaction::Pressed {
                game.is_eraser = !game.is_eraser;
            }
        }
        for i in q_clear.iter() {
            if *i == Interaction::Pressed && game.page == PicPage::Game {
                game.clear_canvas = true;
            }
        }
        for i in q_skip.iter() {
            let skippable = game.page == PicPage::Game
                && game.round_active
                && game.phase != PicPhase::NoriDraws;
            if *i == Interaction::Pressed && skippable {
                let msg = format!("⏭ 跳过了「{}」", game.word_zh);
                game.push_chat(ChatKind::System, msg);
                let who = if game.phase == PicPhase::PlayerDraws { "Nori" } else { "你" };
                record_fail(&mut game, who);
                game.transition = Some(SKIP_DELAY);
            }
        }
        for i in q_send.iter() {
            if *i == Interaction::Pressed {
                submit_guess(&mut game);
            }
        }
        for i in q_input.iter() {
            let focusable = game.page == PicPage::Game
                && game.round_active
                && game.phase == PicPhase::PlayerGuesses;
            if *i == Interaction::Pressed && focusable {
                owner.0 = TextInputFocus::Pictionary;
                clicked_input = true;
            }
        }
    }
    if !clicked_input && owner.is(TextInputFocus::Pictionary) {
        owner.0 = TextInputFocus::None;
    }
}

/// 玩家绘画：鼠标在画布上拖动写纹理
fn pic_draw(
    mouse: Res<ButtonInput<MouseButton>>,
    is_dragging: Res<crate::dock::IsDragging>,
    focused: Res<crate::topbar::FocusedTitle>,
    cursor: Res<CursorPosition>,
    canvas_q: Query<(&ComputedNode, &UiGlobalTransform), With<PicCanvas>>,
    canvas: Res<PicCanvasTex>,
    mut game: ResMut<PictionaryGame>,
    mut images: ResMut<Assets<Image>>,
) {
    if game.page != PicPage::Game
        || !game.round_active
        || game.phase != PicPhase::PlayerDraws
        || is_dragging.0
        || focused.title != "你画我猜"
    {
        return;
    }
    let Ok((node, transform)) = canvas_q.single() else {
        return;
    };
    if !cursor.active {
        return;
    }
    let cursor = cursor.physical;
    let Some(inverse) = transform.try_inverse() else {
        return;
    };
    let local = inverse.transform_point2(cursor);
    let size = node.size();
    let half = size * 0.5;
    let in_canvas = local.x.abs() <= half.x && local.y.abs() <= half.y;
    // 画布节点局部坐标（中心原点，y 向下）→ 纹理坐标
    let tex_pos = Vec2::new(
        ((local.x + half.x) / size.x).clamp(0.0, 0.999) * TEX_SIZE as f32,
        ((local.y + half.y) / size.y).clamp(0.0, 0.999) * TEX_SIZE as f32,
    );

    if mouse.just_pressed(MouseButton::Left) {
        if in_canvas {
            game.is_drawing = true;
            game.last_pos = Some(tex_pos);
        } else {
            game.is_drawing = false;
        }
        return;
    }
    if !mouse.pressed(MouseButton::Left) {
        game.is_drawing = false;
        game.last_pos = None;
        return;
    }
    if !game.is_drawing || !in_canvas {
        return;
    }
    let Some(prev) = game.last_pos else {
        game.last_pos = Some(tex_pos);
        return;
    };
    let color = if game.is_eraser { PAPER } else { PALETTE[game.color_idx] };
    let width = if game.is_eraser { ERASER_W } else { PEN_W };
    if let Some(mut image) = images.get_mut(&canvas.0) {
        if let Some(data) = image.data.as_mut() {
            draw_line(data, prev.x, prev.y, tex_pos.x, tex_pos.y, color, width);
        }
    }
    game.last_pos = Some(tex_pos);
}

/// 猜词输入：键盘 + IME（复用 TextInputOwner 焦点）
fn pic_guess_input(
    mut keys: MessageReader<KeyboardInput>,
    mut ime: MessageReader<Ime>,
    focused: Res<crate::topbar::FocusedTitle>,
    owner: Res<TextInputOwner>,
    mut game: ResMut<PictionaryGame>,
) {
    if game.page != PicPage::Game
        || !owner.is(TextInputFocus::Pictionary)
        || focused.title != "你画我猜"
    {
        keys.clear();
        ime.clear();
        return;
    }
    let enabled = game.round_active && game.phase == PicPhase::PlayerGuesses;
    for ev in ime.read() {
        if enabled {
            if let Ime::Commit { value, .. } = ev {
                game.input_buf.push_str(value);
            }
        }
    }
    for ev in keys.read() {
        if !ev.state.is_pressed() || !enabled {
            continue;
        }
        match &ev.logical_key {
            Key::Character(ch) => {
                let raw = ch.chars().next().unwrap_or(' ');
                if !raw.is_control() {
                    game.input_buf.push(raw);
                }
            }
            Key::Space => game.input_buf.push(' '),
            Key::Backspace => {
                game.input_buf.pop();
            }
            Key::Enter => submit_guess(&mut game),
            _ => {}
        }
    }
}

/// Nori 猜词（玩家画阶段）：2.0s 首猜，之后 2.8s 一次，12% 概率猜对
fn pic_nori_guess(time: Res<Time>, mut game: ResMut<PictionaryGame>) {
    if !game.game_active || !game.round_active || game.phase != PicPhase::PlayerDraws {
        return;
    }
    game.nori_timer -= time.delta_secs();
    if game.nori_timer > 0.0 {
        return;
    }
    game.nori_timer = NORI_INTERVAL;
    let is_correct = game.rand() % 100 < NORI_CHANCE_PCT;
    if is_correct {
        let msg = format!("🎉 Nori 猜对了！是「{}」！", game.word_zh);
        game.push_chat(ChatKind::Correct, msg);
        record_correct(&mut game, "Nori");
    } else {
        let mut candidates: Vec<&str> = WORD_PAIRS
            .iter()
            .map(|p| p.1)
            .filter(|w| *w != game.word_zh)
            .collect();
        if candidates.is_empty() {
            candidates.push("未知");
        }
        let guess = candidates[game.rand_index(candidates.len())];
        game.push_chat(ChatKind::Guess, guess);
    }
}

/// AI 绘画动画：0.8s 起笔延迟，之后每 0.15s 画完一笔
fn pic_ai_draw(
    time: Res<Time>,
    mut game: ResMut<PictionaryGame>,
    mut images: ResMut<Assets<Image>>,
    canvas: Res<PicCanvasTex>,
) {
    if !game.game_active || !game.round_active || game.phase != PicPhase::NoriDraws {
        return;
    }
    game.ai_timer -= time.delta_secs();
    if !game.ai_started {
        if game.ai_timer > 0.0 {
            return;
        }
        game.ai_started = true;
        game.ai_timer = 0.0;
    }
    if game.ai_timer > 0.0 {
        return;
    }
    game.ai_timer = AI_STROKE_SECS;
    if game.ai_stroke_idx >= game.ai_strokes.len() {
        game.phase = PicPhase::PlayerGuesses;
        game.push_chat(ChatKind::System, "✏️ Nori 画完了，轮到你猜！");
        return;
    }
    let stroke = game.ai_strokes[game.ai_stroke_idx].clone();
    if let Some(mut image) = images.get_mut(&canvas.0) {
        if let Some(data) = image.data.as_mut() {
            for pair in stroke.windows(2) {
                draw_line(data, pair[0].x, pair[0].y, pair[1].x, pair[1].y, INK, AI_PEN_W);
            }
        }
    }
    game.ai_stroke_idx += 1;
}

/// 主计时：画布清空 / 倒计时 / 回合计时 / 回合过渡 / 提示闪烁
fn pic_tick(
    time: Res<Time>,
    db: Res<DrawingsState>,
    canvas: Res<PicCanvasTex>,
    mut game: ResMut<PictionaryGame>,
    mut images: ResMut<Assets<Image>>,
) {
    if game.clear_canvas {
        if let Some(mut image) = images.get_mut(&canvas.0) {
            if let Some(data) = image.data.as_mut() {
                fill_paper(data);
            }
        }
        game.clear_canvas = false;
    }
    if game.page != PicPage::Game || !game.game_active {
        return;
    }
    game.time_left -= time.delta_secs();
    if game.time_left <= 0.0 {
        game.time_left = 0.0;
        end_game(&mut game);
        return;
    }
    if game.round_active {
        game.round_elapsed += time.delta_secs();
    }
    if let Some(t) = game.transition.as_mut() {
        *t -= time.delta_secs();
        if *t <= 0.0 {
            game.transition = None;
            start_new_round(&mut game, &db);
        }
    }
    if game.hint_flash > 0.0 {
        game.hint_flash -= time.delta_secs();
    }
}

fn turn_label(game: &PictionaryGame) -> String {
    if game.game_active {
        if !game.round_active {
            return "下一题…".to_string();
        }
        match game.phase {
            PicPhase::PlayerDraws => "轮到你画画了",
            PicPhase::NoriDraws => "Nori 在画画…",
            PicPhase::PlayerGuesses => "轮到你猜了",
        }
        .to_string()
    } else if game.page == PicPage::Result {
        "游戏结束".to_string()
    } else {
        String::new()
    }
}

fn word_label(game: &PictionaryGame) -> String {
    if !game.game_active {
        return String::new();
    }
    if game.phase == PicPhase::PlayerDraws {
        if game.round_active {
            format!("「{}」", game.word_zh)
        } else {
            String::new()
        }
    } else {
        "？？？".to_string()
    }
}

fn input_label(game: &PictionaryGame) -> String {
    if game.hint_flash > 0.0 {
        return "❌ 不对哦，再想想".to_string();
    }
    if !game.input_buf.is_empty() {
        return format!("{}▏", game.input_buf);
    }
    if !game.game_active {
        return "💡 输入你猜的词".to_string();
    }
    match game.phase {
        PicPhase::PlayerDraws => "✏️ 画吧，Nori 会猜",
        PicPhase::NoriDraws => "⏳ 等待 Nori 画完…",
        PicPhase::PlayerGuesses => "💡 输入你猜的词",
    }
    .to_string()
}

/// 文本刷新：计时 / 回合 / 词 / 分数 / 输入框 + 结算统计 + 时长按钮高亮
fn pic_ui_update(
    game: Res<PictionaryGame>,
    mut texts: Query<(&PicText, &mut Text)>,
    mut results: Query<(&PicResultText, &mut Text), Without<PicText>>,
    mut dur_btns: Query<(&PicDurBtn, &mut BackgroundColor)>,
) {
    let timer = format_time(game.time_left);
    for (marker, mut text) in texts.iter_mut() {
        let target = match marker {
            PicText::Timer => timer.clone(),
            PicText::Turn => turn_label(&game),
            PicText::Word => word_label(&game),
            PicText::Score => game.score.to_string(),
            PicText::Input => input_label(&game),
        };
        if **text != target {
            **text = target;
        }
    }
    let total = game.total_rounds.max(1);
    let accuracy = (game.correct_rounds as f32 / total as f32 * 100.0).round() as u32;
    let dur_used = format_time(game.duration as f32 - game.time_left);
    let times: Vec<f32> = game.round_times.iter().copied().filter(|t| *t > 0.0).collect();
    let fastest = times.iter().cloned().fold(f32::INFINITY, f32::min);
    let avg = if times.is_empty() {
        0.0
    } else {
        times.iter().sum::<f32>() / times.len() as f32
    };
    for (marker, mut text) in results.iter_mut() {
        let target = match marker {
            PicResultText::Score => format!("得分：{}", game.score),
            PicResultText::Total => format!("共 {} 题", game.total_rounds),
            PicResultText::Duration => format!("用时 {}", dur_used),
            PicResultText::Accuracy => format!("正确率 {}%", accuracy),
            PicResultText::Fastest => {
                if fastest.is_finite() {
                    format!("最快 {:.1}s", fastest)
                } else {
                    "最快 —".to_string()
                }
            }
            PicResultText::Average => {
                if avg > 0.0 {
                    format!("平均 {:.1}s", avg)
                } else {
                    "平均 —".to_string()
                }
            }
        };
        if **text != target {
            **text = target;
        }
    }
    for (btn, mut bg) in dur_btns.iter_mut() {
        let target = if btn.0 == game.duration {
            ACCENT
        } else {
            SURFACE_2
        };
        if bg.0 != target {
            bg.0 = target;
        }
    }
}

/// 聊天列表重建（脏标记触发，只渲染最近 CHAT_VISIBLE 条）
fn pic_chat_sync(
    mut game: ResMut<PictionaryGame>,
    mut commands: Commands,
    fonts: Res<N3riFonts>,
    lists: Query<(Entity, Option<&Children>), With<PicChatList>>,
) {
    if !game.chat_dirty {
        return;
    }
    let Ok((list_e, children)) = lists.single() else {
        return;
    };
    game.chat_dirty = false;
    if let Some(children) = children {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }
    // 只渲染最后 N 条，旧消息直接不渲染
    let skip = game.chat.len().saturating_sub(CHAT_VISIBLE);
    for msg in game.chat.iter().skip(skip) {
        let color = chat_color(msg.kind);
        let text = msg.text.clone();
        commands.entity(list_e).with_children(|list| {
            list.spawn((Text::new(text), TextFont {
                font: FontSource::Handle(fonts.default.clone()),
                font_size: FontSize::Px(13.0),
                ..default()
            }, TextColor(color)));
        });
    }
}

/// 结算词汇列表重建（脏标记触发）
fn pic_result_sync(
    mut game: ResMut<PictionaryGame>,
    mut commands: Commands,
    fonts: Res<N3riFonts>,
    lists: Query<(Entity, Option<&Children>), With<PicResultList>>,
) {
    if !game.result_dirty {
        return;
    }
    let Ok((list_e, children)) = lists.single() else {
        return;
    };
    game.result_dirty = false;
    if let Some(children) = children {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }
    if game.word_history.is_empty() {
        commands.entity(list_e).with_children(|list| {
            list.spawn((Text::new("还没有词汇记录"), TextFont {
                font: FontSource::Handle(fonts.default.clone()),
                font_size: FontSize::Px(13.0),
                ..default()
            }, TextColor(TEXT_DIM)));
        });
        return;
    }
    for rec in &game.word_history {
        let icon = if rec.correct { "✓" } else { "✗" };
        let time_s = if rec.correct {
            format!("{:.1}s", rec.time)
        } else {
            "—".to_string()
        };
        let line = format!("{} {} · {} · {}", icon, rec.word, rec.who, time_s);
        let color = if rec.correct {
            Color::srgb(0.44, 0.75, 0.56)
        } else {
            Color::srgb(0.78, 0.44, 0.44)
        };
        commands.entity(list_e).with_children(|list| {
            list.spawn((Text::new(line), TextFont {
                font: FontSource::Handle(fonts.default.clone()),
                font_size: FontSize::Px(13.0),
                ..default()
            }, TextColor(color)));
        });
    }
}

/// 页面显隐切换
fn pic_page_sync(game: Res<PictionaryGame>, mut pages: Query<(&PicPageRoot, &mut Node)>) {
    for (root, mut node) in pages.iter_mut() {
        let target = if root.0 == game.page {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != target {
            node.display = target;
        }
    }
}

/// 窗口关闭后复位状态与输入焦点
fn pic_cleanup(
    windows: Query<&AppWindow>,
    mut game: ResMut<PictionaryGame>,
    mut owner: ResMut<TextInputOwner>,
) {
    if windows.iter().any(|w| w.app_id == "pictionary") {
        return;
    }
    if owner.is(TextInputFocus::Pictionary) {
        owner.0 = TextInputFocus::None;
    }
    if game.game_active || game.page != PicPage::Menu {
        *game = PictionaryGame::new();
    }
}

// ==================== 测试 ====================

#[cfg(test)]
mod tests {
    use super::*;

    /// 完整走一遍 spawn_pictionary 的 UI 构建路径。
    /// Bundle 重复组件（如页面曾出现的双 Node）在 queue.apply 注册 bundle 时会 panic，
    /// 该测试作为回归防线。
    #[test]
    fn spawn_ui_bundles_are_valid() {
        let mut world = World::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        {
            let mut commands = Commands::new(&mut queue, &world);
            let fonts = N3riFonts {
                default: Handle::default(),
                terminal: Handle::default(),
                ui: Handle::default(),
                dock: Handle::default(),
            };
            commands.spawn_empty().with_children(|root| {
                spawn_pictionary(root, &fonts);
            });
        }
        queue.apply(&mut world);
    }

    /// 聊天/结算列表在空列表（无 Children 组件）时也必须能渲染消息。
    /// 回归：曾因 Query 要求 &Children 且空列表无该组件，首帧重建失败后
    /// 脏标记被清掉，消息永远不显示。
    #[test]
    fn chat_and_result_lists_render_messages() {
        use bevy::ecs::system::SystemState;

        let mut world = World::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        {
            let mut commands = Commands::new(&mut queue, &world);
            let fonts = N3riFonts {
                default: Handle::default(),
                terminal: Handle::default(),
                ui: Handle::default(),
                dock: Handle::default(),
            };
            commands.insert_resource(N3riFonts {
                default: fonts.default.clone(),
                terminal: fonts.terminal.clone(),
                ui: fonts.ui.clone(),
                dock: fonts.dock.clone(),
            });
            commands.spawn_empty().with_children(|root| {
                spawn_pictionary(root, &fonts);
            });
        }
        queue.apply(&mut world);
        world.init_resource::<PictionaryGame>();
        world.init_resource::<DrawingsState>();

        // 开始一局：push 出加载 + 出题消息
        world.resource_scope(|world: &mut World, mut game: Mut<PictionaryGame>| {
            world.resource_scope(|_world: &mut World, mut db: Mut<DrawingsState>| {
                start_game(&mut game, &mut db);
            });
        });
        let expected_chat = world.resource::<PictionaryGame>().chat.len();
        assert!(expected_chat >= 2, "开始游戏应至少有加载与出题两条消息");

        // 运行聊天重建系统
        let mut state: SystemState<(
            ResMut<PictionaryGame>,
            Commands,
            Res<N3riFonts>,
            Query<(Entity, Option<&Children>), With<PicChatList>>,
        )> = SystemState::new(&mut world);
        let (game, commands, fonts, lists) = state.get_mut(&mut world).unwrap();
        pic_chat_sync(game, commands, fonts, lists);
        state.apply(&mut world);

        let mut q = world.query_filtered::<&Children, With<PicChatList>>();
        let children = q.single(&world).expect("聊天列表应存在且有 Children");
        let expected_rendered = expected_chat.min(CHAT_VISIBLE);
        assert_eq!(
            children.len(),
            expected_rendered,
            "聊天消息节点数量必须与渲染条数一致（最多 CHAT_VISIBLE）"
        );

        // 结束一局：结算列表同样要能渲染（空 Children 起点）
        {
            let mut game = world.resource_mut::<PictionaryGame>();
            end_game(&mut game);
            assert!(game.result_dirty);
        }
        let mut state: SystemState<(
            ResMut<PictionaryGame>,
            Commands,
            Res<N3riFonts>,
            Query<(Entity, Option<&Children>), With<PicResultList>>,
        )> = SystemState::new(&mut world);
        let (game, commands, fonts, lists) = state.get_mut(&mut world).unwrap();
        pic_result_sync(game, commands, fonts, lists);
        state.apply(&mut world);

        let expected_rows = world.resource::<PictionaryGame>().word_history.len();
        assert!(expected_rows >= 1, "时间到应记录未完成词");
        let mut q = world.query_filtered::<&Children, With<PicResultList>>();
        let children = q.single(&world).expect("结算列表应存在且有 Children");
        assert_eq!(children.len(), expected_rows, "结算词汇行数量必须与记录一致");
    }
}
