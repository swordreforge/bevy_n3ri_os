//! 森林寻宝 —— 人机合作 Codenames 变体。
//!
//! 规则:
//! - 5x5 词语棋盘(25 词取自词库),双方各自独立布局:每方 9 宝箱 / 3 怪物 / 13 树莓。
//! - 一方出提示(词 + 数量)、一方猜测算一回合;回合数由难度决定(13/11/9)。
//! - 猜测方翻开格子:宝箱扣减对应方剩余(可双方同时扣)并可继续寻找;
//!   踩到自己的树莓立即停止回合并互换角色;踩到任意怪物无条件结算(失败)。
//! - 回合耗尽后若仍有宝箱,进入"最后冲刺":双方禁止提示、只能猜测,
//!   此时踩到树莓也直接结算。
//! - 人类玩家可以看到对方的道具布局图;LLM 看到的是序列化格式。
//!   提示方利用对方布局引导猜测方寻找猜测方的宝箱。

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Mutex;
use std::thread;

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::text::{FontSource, FontSize};
use bevy::window::Ime;

use n3ri_llm::{LlmClient, Message};

use crate::font::N3riFonts;
use crate::input_focus::{TextInputFocus, TextInputOwner};
use crate::window::{spawn_window, AppWindow};

const WIN_W: f32 = 980.0;
const WIN_H: f32 = 620.0;
const BOARD: usize = 25;
const GRID_SIDE: usize = 5;
const TREASURE_PER_SIDE: usize = 9;
const MONSTER_PER_SIDE: usize = 3;
const MAX_CLUE_NUM: u32 = 5;
const AI_THINK_DELAY: f32 = 0.9;
const AI_NEXT_GUESS_DELAY: f32 = 1.1;
const AI_RETRY_DELAY: f32 = 0.6;
const AI_CLICK_DELAY_MIN: f32 = 0.6;
const AI_CLICK_JITTER: f32 = 1.2;
const SNIPPET_LEN: usize = 80;
const GAME_MIN_MAX_TOKENS: u32 = 8192;
const FINAL_NOTE: &str = "回合已耗尽，最后冲刺：禁止提示、只能猜测，踩到树莓或怪物即失败！";

const CARD_W: f32 = 122.0;
const CARD_H: f32 = 72.0;
const CARD_GAP: f32 = 6.0;
const MAP_CELL_W: f32 = 30.0;
const MAP_CELL_H: f32 = 21.0;
const MAP_GAP: f32 = 3.0;

// 配色(对齐 seek_treasure.html 的森林主题)
const GOLD: Color = Color::srgb(0.831, 0.659, 0.325);
const GOLD_LIGHT: Color = Color::srgb(0.941, 0.843, 0.549);
const GREEN: Color = Color::srgb(0.478, 0.710, 0.361);
const RED: Color = Color::srgb(0.910, 0.365, 0.459);
const CREAM: Color = Color::srgb(0.961, 0.941, 0.902);
const CREAM_DIM: Color = Color::srgba(0.961, 0.941, 0.902, 0.55);
const TEXT_DARK: Color = Color::srgb(0.102, 0.141, 0.094);
const SURFACE: Color = Color::srgba(0.0, 0.0, 0.0, 0.28);
const CARD_BG: Color = Color::srgba(0.102, 0.141, 0.094, 0.92);
const CARD_REVEALED_BG: Color = Color::srgba(0.0, 0.0, 0.0, 0.62);
const PANEL_BORDER: Color = Color::srgba(0.29, 0.353, 0.251, 0.5);
const BTN_BG: Color = Color::srgba(0.29, 0.353, 0.251, 0.45);

const WORDS_JSON: &str = include_str!("../../../../assets/nori/app-icons/codenames/words.json");
const FALLBACK_CLUES: [&str; 5] = ["宝物", "森林", "收获", "冒险", "神秘"];

// ==================== 游戏类型 ====================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Prop {
    Treasure,
    Raspberry,
    Monster,
}

impl Prop {
    fn label(self) -> &'static str {
        match self {
            Prop::Treasure => "宝",
            Prop::Raspberry => "莓",
            Prop::Monster => "怪",
        }
    }
    fn full(self) -> &'static str {
        match self {
            Prop::Treasure => "宝箱",
            Prop::Raspberry => "树莓",
            Prop::Monster => "怪物",
        }
    }
    fn color(self) -> Color {
        match self {
            Prop::Treasure => GOLD,
            Prop::Raspberry => GREEN,
            Prop::Monster => RED,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    Human,
    Ai,
}

impl Side {
    fn badge(self) -> &'static str {
        match self {
            Side::Human => "你",
            Side::Ai => "AI",
        }
    }
    fn other(self) -> Side {
        match self {
            Side::Human => Side::Ai,
            Side::Ai => Side::Human,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StPage {
    Start,
    Game,
    Result,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StPhase {
    /// 人类出提示(AI 待猜测)
    HumanClue,
    /// AI 猜测中(LLM 驱动)
    AiSeeking,
    /// AI 出提示中(LLM 驱动)
    AiClue,
    /// 人类点击卡片猜测
    HumanGuessing,
    /// 已结算
    Settled,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LlmTask {
    Clue,
    Guess,
}

struct StCell {
    word: String,
    human: Prop,
    ai: Prop,
    revealed: bool,
}

#[derive(Resource)]
struct SeekGame {
    page: StPage,
    phase: StPhase,
    grid: Vec<StCell>,
    difficulty: u32,
    sel_difficulty: u32,
    turns_left: u32,
    total_turns: u32,
    human_left: u32,
    ai_left: u32,
    clue_giver: Side,
    seeker: Side,
    final_stage: bool,
    win: bool,
    round_count: u32,
    clue_word: String,
    clue_num: u32,
    guesses_used: u32,
    input_buf: String,
    llm_task: Option<LlmTask>,
    llm_rx: Option<Mutex<Receiver<Result<String, String>>>>,
    llm_retries: u32,
    invalid_reply: Option<String>,
    ai_timer: f32,
    ai_guess_queue: VecDeque<usize>,
    board_dirty: bool,
    map_dirty: bool,
    message: String,
    result_title: String,
    result_sub: String,
    rng: u64,
}

impl Default for SeekGame {
    fn default() -> Self {
        Self::new()
    }
}

impl SeekGame {
    fn new() -> Self {
        Self {
            page: StPage::Start,
            phase: StPhase::HumanClue,
            grid: Vec::new(),
            difficulty: 11,
            sel_difficulty: 11,
            turns_left: 0,
            total_turns: 0,
            human_left: 0,
            ai_left: 0,
            clue_giver: Side::Human,
            seeker: Side::Ai,
            final_stage: false,
            win: false,
            round_count: 0,
            clue_word: String::new(),
            clue_num: 1,
            guesses_used: 0,
            input_buf: String::new(),
            llm_task: None,
            llm_rx: None,
            llm_retries: 0,
            invalid_reply: None,
            ai_timer: 0.0,
            ai_guess_queue: VecDeque::new(),
            board_dirty: false,
            map_dirty: false,
            message: String::new(),
            result_title: String::new(),
            result_sub: String::new(),
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

    fn set_message(&mut self, msg: impl Into<String>) {
        self.message = msg.into();
    }

    fn treasure_left(&self, side: Side) -> u32 {
        match side {
            Side::Human => self.human_left,
            Side::Ai => self.ai_left,
        }
    }

    fn settle(&mut self, win: bool, title: &str, sub: &str, msg: &str) {
        self.win = win;
        self.ai_guess_queue.clear();
        self.phase = StPhase::Settled;
        self.page = StPage::Result;
        self.result_title = title.to_string();
        self.result_sub = sub.to_string();
        self.set_message(msg);
    }

    // ---------- 开局 ----------

    fn start_game(&mut self) {
        let words = self.pick_words();
        let (human_props, ai_props) = gen_props(&mut self.rng);
        self.grid = words
            .into_iter()
            .zip(human_props)
            .zip(ai_props)
            .map(|((word, human), ai)| StCell {
                word,
                human,
                ai,
                revealed: false,
            })
            .collect();
        self.human_left = TREASURE_PER_SIDE as u32;
        self.ai_left = TREASURE_PER_SIDE as u32;
        self.total_turns = self.difficulty;
        self.turns_left = self.difficulty;
        self.clue_giver = Side::Human;
        self.seeker = Side::Ai;
        self.final_stage = false;
        self.win = false;
        self.round_count = 0;
        self.clue_word.clear();
        self.clue_num = 1;
        self.guesses_used = 0;
        self.input_buf.clear();
        self.llm_task = None;
        self.llm_rx = None;
        self.llm_retries = 0;
        self.invalid_reply = None;
        self.ai_timer = 0.0;
        self.ai_guess_queue.clear();
        self.page = StPage::Game;
        self.phase = StPhase::HumanClue;
        self.board_dirty = true;
        self.map_dirty = true;
        self.set_message("游戏开始！请给出提示，引导 AI 寻找 AI 方的宝箱。");
    }

    fn pick_words(&mut self) -> Vec<String> {
        let all = load_word_bank();
        let mut indices: Vec<usize> = (0..all.len()).collect();
        let n = indices.len();
        for i in (1..n).rev() {
            let j = self.rand_index(i + 1);
            indices.swap(i, j);
        }
        indices
            .into_iter()
            .take(BOARD)
            .map(|i| all[i].clone())
            .collect()
    }

    // ---------- 回合流转 ----------

    /// 人类提交提示
    fn submit_human_clue(&mut self) -> bool {
        if self.page != StPage::Game || self.phase != StPhase::HumanClue || self.final_stage {
            return false;
        }
        let word = self.input_buf.trim().to_string();
        if word.is_empty() {
            self.set_message("请先输入提示词。");
            return false;
        }
        if self.grid.iter().any(|c| c.word == word) {
            self.set_message("提示词不能与棋盘上的词语相同。");
            return false;
        }
        self.clue_word = word;
        self.clue_num = self.clue_num.clamp(1, MAX_CLUE_NUM);
        self.input_buf.clear();
        self.guesses_used = 0;
        self.turns_left = self.turns_left.saturating_sub(1);
        self.phase = StPhase::AiSeeking;
        self.llm_task = Some(LlmTask::Guess);
        self.llm_retries = 0;
        self.invalid_reply = None;
        self.ai_timer = AI_THINK_DELAY;
        self.set_message(format!(
            "你提示「{}」×{}，AI 正在思考…",
            self.clue_word, self.clue_num
        ));
        true
    }

    /// 执行一次猜测并返回结果描述(供消息栏拼接)
    fn resolve_guess(&mut self, idx: usize) -> String {
        if idx >= BOARD || self.grid[idx].revealed {
            return String::from("无效猜测。");
        }
        let seeker = self.seeker;
        self.grid[idx].revealed = true;
        self.guesses_used += 1;
        self.round_count += 1;
        self.board_dirty = true;
        self.map_dirty = true;
        let (human_p, ai_p) = {
            let c = &self.grid[idx];
            (c.human, c.ai)
        };

        // 1) 怪物:任意一方是怪物即无条件结算
        if human_p == Prop::Monster || ai_p == Prop::Monster {
            self.settle(false, "探险失败…", "踩到了怪物，游戏立即结算。", "踩到怪物！游戏失败…");
            return Prop::Monster.full().to_string() + "！";
        }

        // 2) 宝箱双方独立扣减
        let mut found = false;
        if human_p == Prop::Treasure {
            self.human_left -= 1;
            found = true;
        }
        if ai_p == Prop::Treasure {
            self.ai_left -= 1;
            found = true;
        }
        let found_note = if found {
            format!("找到宝箱！我方剩 {}，AI 方剩 {}。", self.human_left, self.ai_left)
        } else {
            String::new()
        };

        // 3) 胜利:双方宝箱全部找到
        if self.human_left == 0 && self.ai_left == 0 {
            self.settle(
                true,
                "探险成功！",
                "所有宝箱都找到了，太厉害了！",
                &format!("{found_note} 所有宝箱已找到，你们赢了！"),
            );
            return found_note;
        }

        // 4) 树莓:猜测者自己的地图
        let own_raspberry = match seeker {
            Side::Human => human_p == Prop::Raspberry,
            Side::Ai => ai_p == Prop::Raspberry,
        };
        if own_raspberry {
            if self.final_stage {
                self.settle(
                    false,
                    "探险失败…",
                    "最后冲刺阶段踩到树莓，游戏直接结算。",
                    "最后冲刺踩到树莓！游戏失败…",
                );
                return found_note + "踩到树莓！";
            }
            let mut detail = if found {
                format!("{found_note}踩到树莓！回合结束，角色互换。")
            } else {
                String::from("踩到树莓！回合结束，角色互换。")
            };
            self.end_streak_swap(seeker);
            if self.enter_final_if_due() {
                detail.push_str(FINAL_NOTE);
            }
            return detail;
        }

        // 5) 猜测者自己的宝箱是否已全部找到(提前移交)
        if self.treasure_left(seeker) == 0 {
            let mut detail = format!("{found_note}{}方宝箱已全部找到。", seeker.badge());
            if self.final_stage {
                self.guesses_used = 0;
                self.seeker = seeker.other();
                self.clue_giver = self.seeker.other();
                self.llm_task = None;
                self.llm_rx = None;
                self.ai_guess_queue.clear();
                self.phase = match self.seeker {
                    Side::Human => StPhase::HumanGuessing,
                    Side::Ai => {
                        self.llm_task = Some(LlmTask::Guess);
                        self.ai_timer = AI_THINK_DELAY;
                        StPhase::AiSeeking
                    }
                };
            } else {
                self.end_streak_swap(seeker);
            }
            if self.enter_final_if_due() {
                detail.push_str(FINAL_NOTE);
            }
            return detail;
        }

        // 6) 继续寻找或次数用完互换
        let may_continue = self.final_stage || self.guesses_used < self.clue_num;
        if may_continue {
            return match seeker {
                Side::Ai => {
                    if self.ai_guess_queue.is_empty() {
                        self.llm_task = Some(LlmTask::Guess);
                        self.ai_timer = AI_NEXT_GUESS_DELAY;
                    }
                    if self.final_stage {
                        format!("{found_note}AI 继续寻找…")
                    } else {
                        format!(
                            "{found_note}AI 继续寻找（本线索还可猜 {} 次）…",
                            self.clue_num - self.guesses_used
                        )
                    }
                }
                Side::Human => {
                    self.phase = StPhase::HumanGuessing;
                    if self.final_stage {
                        format!("{found_note}可继续点击卡片。")
                    } else {
                        format!(
                            "{found_note}可继续点击卡片（本线索还可猜 {} 次）。",
                            self.clue_num - self.guesses_used
                        )
                    }
                }
            };
        }

        // 次数用完 → 互换
        let mut detail = format!("{found_note}寻找次数用完，角色互换。");
        self.end_streak_swap(seeker);
        if self.enter_final_if_due() {
            detail.push_str(FINAL_NOTE);
        }
        detail
    }

    /// 结束当前线索并互换角色(原猜测方成为新的提示方)
    fn end_streak_swap(&mut self, old_seeker: Side) {
        self.guesses_used = 0;
        self.clue_giver = old_seeker;
        self.seeker = old_seeker.other();
        self.clue_word.clear();
        self.clue_num = 1;
        self.llm_retries = 0;
        self.invalid_reply = None;
        self.ai_guess_queue.clear();
        match self.clue_giver {
            Side::Human => self.phase = StPhase::HumanClue,
            Side::Ai => {
                self.phase = StPhase::AiClue;
                self.llm_task = Some(LlmTask::Clue);
                self.ai_timer = AI_THINK_DELAY;
            }
        }
    }

    /// 回合耗尽且仍有宝箱 → 进入最后冲刺(只能猜测)。
    /// 返回是否进入。
    fn enter_final_if_due(&mut self) -> bool {
        if self.final_stage || self.page == StPage::Result {
            return false;
        }
        if self.turns_left > 0 || (self.human_left == 0 && self.ai_left == 0) {
            return false;
        }
        self.final_stage = true;
        self.guesses_used = 0;
        self.llm_task = None;
        self.llm_rx = None;
        self.ai_guess_queue.clear();
        // 寻找方:优先当前寻找方(若其仍有宝箱),否则交给对方
        self.seeker = if self.treasure_left(self.seeker) > 0 {
            self.seeker
        } else {
            self.seeker.other()
        };
        self.clue_giver = self.seeker.other();
        self.phase = match self.seeker {
            Side::Human => StPhase::HumanGuessing,
            Side::Ai => {
                self.llm_task = Some(LlmTask::Guess);
                self.ai_timer = AI_THINK_DELAY;
                StPhase::AiSeeking
            }
        };
        true
    }
}

// ==================== 布局与词库 ====================

/// 每方独立布局:9 宝箱 + 3 怪物(同方不重叠),其余 13 格树莓;跨方完全独立。
fn gen_props(rng: &mut u64) -> ([Prop; BOARD], [Prop; BOARD]) {
    let mut human = [Prop::Raspberry; BOARD];
    let mut ai = [Prop::Raspberry; BOARD];
    assign_side_props(&mut human, rng);
    assign_side_props(&mut ai, rng);
    (human, ai)
}

fn assign_side_props(props: &mut [Prop; BOARD], rng: &mut u64) {
    let mut indices: Vec<usize> = (0..BOARD).collect();
    let n = indices.len();
    for i in (1..n).rev() {
        let j = (rand_u64(rng) % (i + 1) as u64) as usize;
        indices.swap(i, j);
    }
    for &i in &indices[..TREASURE_PER_SIDE] {
        props[i] = Prop::Treasure;
    }
    for &i in &indices[TREASURE_PER_SIDE..TREASURE_PER_SIDE + MONSTER_PER_SIDE] {
        props[i] = Prop::Monster;
    }
}

fn rand_u64(rng: &mut u64) -> u64 {
    let mut x = *rng;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *rng = x;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

fn load_word_bank() -> Vec<String> {
    let bank: HashMap<String, Vec<String>> = serde_json::from_str(WORDS_JSON).unwrap_or_default();
    bank.into_values().flatten().collect()
}

// ==================== LLM 集成 ====================

/// AI 视角的棋盘序列化:显示【人类】的道具布局(AI 的"对方布局图")
fn serialize_board(game: &SeekGame) -> String {
    let mut out = String::new();
    for i in 0..BOARD {
        if i % GRID_SIDE == 0 {
            out.push('\n');
        }
        let cell = &game.grid[i];
        let rev = if cell.revealed { "(已翻开)" } else { "" };
        out.push_str(&format!(
            "{}:{}={}{}",
            i,
            cell.word,
            cell.human.full(),
            rev
        ));
        if i % GRID_SIDE != GRID_SIDE - 1 {
            out.push_str("  ");
        }
    }
    out
}

fn build_llm_messages(game: &SeekGame) -> Vec<Message> {
    let task = game.llm_task.unwrap_or(LlmTask::Guess);
    let board = serialize_board(game);
    let (system, user) = match task {
        LlmTask::Clue => (
            format!(
                "你是「森林寻宝」合作游戏中的 AI 玩家。5x5 棋盘，25 格（索引 0~24，按行排列）。\
给出一个线索，引导人类找到【人类的宝箱】。\
人类道具布局：\n{board}\n\
规则：只把未翻开的「宝箱」格作为目标；提示词是简短中文词，不能是棋盘上的词；绝不引导人类踩「怪物」。\
不要展开思考过程，不要解释。整个回复只有一行 JSON：{{\"word\": \"提示词\", \"number\": <1~5 的整数>}}，\
示例：{{\"word\": \"森林\", \"number\": 3}}"
            ),
            format!(
                "当前局面：剩余回合 {}，你(AI)的宝箱剩 {}，人类的宝箱剩 {}。请给出线索。",
                game.turns_left, game.ai_left, game.human_left
            ),
        ),
        LlmTask::Guess => {
            let remain = game.clue_num.saturating_sub(game.guesses_used);
            let cap = if game.final_stage {
                (0..BOARD).filter(|&i| !game.grid[i].revealed).count() as u32
            } else {
                remain
            };
            let goal = if game.final_stage {
                "最后冲刺阶段：没有线索，避开怪物，把有把握的格子尽可能多地一次性列出。"
            } else {
                "根据线索，把本线索下想翻开的格子一次性全部给出。"
            };
            let situation = if game.final_stage {
                "最后冲刺：无线索限制，尽量多翻安全格。".to_string()
            } else {
                format!(
                    "当前线索：「{}」，指向 {} 张卡片；本线索你还可以猜 {} 次。",
                    game.clue_word, game.clue_num, remain
                )
            };
            (
                format!(
                    "你是「森林寻宝」合作游戏中的 AI 玩家。5x5 棋盘，25 格（索引 0~24，按行排列）。\
目标是用线索找到【AI 的宝箱】（自己的宝箱位置未知，靠线索推理）。\
人类道具布局（对方布局，用于避开危险）：\n{board}\n\
规则：只选未翻开的格子；避开人类布局中的「怪物」格（踩怪物即输）；人类「宝箱」格可优先。\
{goal}\
不要展开思考过程，不要解释。整个回复只有一行 JSON：\
{{\"indexes\": [<0~24 的整数>, ...]}}，按把握从大到小排列，1~{cap} 个。\
示例：{{\"indexes\": [7, 12, 3]}}"
                ),
                format!(
                    "{situation}\
剩余回合 {}，你(AI)的宝箱剩 {}，人类的宝箱剩 {}。请一次性给出全部猜测。",
                    game.turns_left,
                    game.ai_left,
                    game.human_left
                ),
            )
        }
    };
    let mut messages = vec![Message::system(system)];
    if let Some(bad) = &game.invalid_reply {
        messages.push(Message::user(format!(
            "你上一次的回复（{bad}）无法解析。请严格只输出一行 JSON，不要包含解释文字、代码块标记或占位符。"
        )));
    }
    messages.push(Message::user(user));
    messages
}

/// 全角数字/标点归一化为半角,提高小模型输出的可解析性
fn ascii_normalize(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '０'..='９' => {
                char::from_u32('0' as u32 + (ch as u32 - '０' as u32)).unwrap_or(ch)
            }
            '：' => ':',
            '，' => ',',
            '；' => ';',
            '“' | '”' => '"',
            '‘' | '’' => '\'',
            '｛' => '{',
            '｝' => '}',
            '【' | '〖' => '[',
            '】' | '〗' => ']',
            _ => ch,
        })
        .collect()
}

fn extract_json(text: &str) -> Option<serde_json::Value> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end < start {
        return None;
    }
    serde_json::from_str(&text[start..=end]).ok()
}

fn json_u64(v: &serde_json::Value, key: &str) -> Option<u64> {
    let val = v.get(key)?;
    val.as_u64()
        .or_else(|| val.as_str().and_then(|s| s.trim().parse().ok()))
}

/// 在文本中寻找关键词后跟的第一个数字(如 "index: 7"、"索引 5")
fn keyword_u64(text: &str, keys: &[&str]) -> Option<u64> {
    let lower = text.to_lowercase();
    for key in keys {
        let mut search_from = 0;
        while let Some(rel) = lower[search_from..].find(key) {
            let after = search_from + rel + key.len();
            let digits: String = lower[after..]
                .chars()
                .skip_while(|c| !c.is_ascii_digit())
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(n) = digits.parse::<u64>() {
                return Some(n);
            }
            search_from = after;
        }
    }
    None
}

fn first_u64(text: &str) -> Option<u64> {
    let digits: String = text
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

fn clean_clue_word(word: &str) -> String {
    word.trim()
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '「' | '」' | '*' | '`'))
        .trim()
        .to_string()
}

fn clue_word_ok(word: &str, game: &SeekGame) -> bool {
    !word.is_empty() && word.chars().count() <= 12 && !game.grid.iter().any(|c| c.word == word)
}

/// 从「词」×N 形式的非 JSON 回复中提取线索
fn quoted_clue(text: &str) -> Option<(String, u32)> {
    let open = text.find('「')? + '「'.len_utf8();
    let close = text[open..].find('」')? + open;
    let word = clean_clue_word(&text[open..close]);
    if word.is_empty() {
        return None;
    }
    let tail = &text[close + '」'.len_utf8()..];
    let num = first_u64(tail).unwrap_or(1).clamp(1, MAX_CLUE_NUM as u64) as u32;
    Some((word, num))
}

/// 解析 AI 的一次性批量猜测:{"indexes": [...]};兼容 {"index": N} 与纯文本 "索引 N"
fn parse_ai_guesses(text: &str, game: &SeekGame) -> Option<Vec<usize>> {
    let norm = ascii_normalize(text);
    let mut raw: Vec<u64> = Vec::new();
    let mut has_key = false;
    if let Some(v) = extract_json(&norm) {
        let entry = v
            .get("indexes")
            .or_else(|| v.get("index"))
            .or_else(|| v.get("guesses"));
        if let Some(entry) = entry {
            has_key = true;
            match entry {
                serde_json::Value::Array(items) => {
                    for item in items {
                        if let Some(n) = item
                            .as_u64()
                            .or_else(|| item.as_str().and_then(|s| s.trim().parse().ok()))
                        {
                            raw.push(n);
                        }
                    }
                }
                _ => {
                    if let Some(n) = json_u64(&v, "indexes").or_else(|| json_u64(&v, "index")) {
                        raw.push(n);
                    }
                }
            }
        }
    }
    if !has_key {
        if let Some(n) = keyword_u64(&norm, &["index", "索引"]) {
            raw.push(n);
        }
    }
    let mut out: Vec<usize> = Vec::new();
    for n in raw {
        let idx = n as usize;
        if idx < BOARD && !game.grid[idx].revealed && !out.contains(&idx) {
            out.push(idx);
        }
    }
    (!out.is_empty()).then_some(out)
}

fn parse_ai_clue(text: &str, game: &SeekGame) -> Option<(String, u32)> {
    let norm = ascii_normalize(text);
    if let Some(v) = extract_json(&norm) {
        if let Some(word) = v.get("word").and_then(|w| w.as_str()) {
            let cleaned = clean_clue_word(word);
            if clue_word_ok(&cleaned, game) {
                let num = json_u64(&v, "number")
                    .unwrap_or(1)
                    .clamp(1, MAX_CLUE_NUM as u64) as u32;
                return Some((cleaned, num));
            }
        }
    }
    let (word, num) = quoted_clue(&norm)?;
    clue_word_ok(&word, game).then_some((word, num))
}

fn snippet(text: &str) -> String {
    let flat: String = text.chars().map(|c| if c.is_whitespace() { ' ' } else { c }).collect();
    let flat = flat.trim();
    if flat.chars().count() > SNIPPET_LEN {
        format!("{}…", flat.chars().take(SNIPPET_LEN).collect::<String>())
    } else {
        flat.to_string()
    }
}

/// LLM 失败时的兜底猜测:随机未翻开的、避开人类怪物格的格子
fn fallback_ai_guess(game: &mut SeekGame, err: &str) {
    let candidates: Vec<usize> = (0..BOARD)
        .filter(|&i| !game.grid[i].revealed && game.grid[i].human != Prop::Monster)
        .collect();
    let idx = if candidates.is_empty() {
        (0..BOARD).find(|&i| !game.grid[i].revealed)
    } else {
        candidates.get(game.rand_index(candidates.len())).copied()
    };
    let Some(idx) = idx else { return };
    let word = game.grid[idx].word.clone();
    let detail = game.resolve_guess(idx);
    game.set_message(format!("（AI 连接失败：{err}）AI 兜底翻开「{word}」：{detail}"));
}

/// LLM 失败时的兜底提示:通用词 + 数量 1,目标随机未翻开的人类宝箱格
fn fallback_ai_clue(game: &mut SeekGame, err: &str) {
    let targets: Vec<usize> = (0..BOARD)
        .filter(|&i| !game.grid[i].revealed && game.grid[i].human == Prop::Treasure)
        .collect();
    if targets.is_empty() {
        game.phase = StPhase::HumanGuessing;
        return;
    }
    let word = FALLBACK_CLUES[game.rand_index(FALLBACK_CLUES.len())].to_string();
    game.clue_word = word;
    game.clue_num = 1;
    game.guesses_used = 0;
    game.phase = StPhase::HumanGuessing;
    game.set_message(format!(
        "（AI 连接失败：{err}）AI 给出保守提示「{}」×1，请点击卡片寻找你的宝箱。",
        game.clue_word
    ));
}

/// 模拟人类翻牌节奏:[AI_CLICK_DELAY_MIN, MIN+JITTER) 的随机间隔
fn human_click_delay(game: &mut SeekGame) -> f32 {
    AI_CLICK_DELAY_MIN + (game.rand_index(1000) as f32 / 1000.0) * AI_CLICK_JITTER
}

fn apply_ai_guess(game: &mut SeekGame, text: &str) {
    match parse_ai_guesses(text, game) {
        Some(idxs) => {
            let n = idxs.len();
            game.ai_guess_queue.extend(idxs);
            game.ai_timer = human_click_delay(game);
            game.set_message(format!("AI 已想好，将按把握依次翻开 {n} 张卡片…"));
        }
        None => fallback_ai_guess(game, "解析异常"),
    }
}

fn apply_ai_clue(game: &mut SeekGame, text: &str) {
    match parse_ai_clue(text, game) {
        Some((word, num)) => {
            game.clue_word = word;
            game.clue_num = num;
            game.guesses_used = 0;
            game.phase = StPhase::HumanGuessing;
            game.set_message(format!(
                "AI 提示：「{}」×{}，请点击卡片寻找你的宝箱（还可猜 {} 次）。",
                game.clue_word, game.clue_num, game.clue_num
            ));
        }
        None => fallback_ai_clue(game, "解析异常"),
    }
}

// ==================== 组件标记 ====================

#[derive(Component)]
struct StPageRoot(StPage);

#[derive(Component)]
struct StCard(usize);

#[derive(Component)]
struct StCardWord;

#[derive(Component)]
struct StCardResult;

#[derive(Component)]
struct StMapCell(usize);

#[derive(Component)]
struct StMapText(usize);

#[derive(Component)]
struct StDiffBtn(u32);

#[derive(Component)]
struct StStartBtn;

#[derive(Component)]
struct StAgainBtn;

#[derive(Component)]
struct StHomeBtn;

#[derive(Component)]
struct StSubmitBtn;

#[derive(Component)]
struct StNumBtn(u32);

#[derive(Component)]
pub struct StClueInput;

#[derive(Component)]
struct StActionRow;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum StText {
    Turns,
    HumanT,
    AiT,
    Clue,
    Giver,
    Seeker,
    Message,
    Input,
    ResultBadge,
    ResultTitle,
    ResultSub,
    ResultStats,
}

// ==================== UI 构建 ====================

pub fn spawn_seek_treasure(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    let window_e = spawn_window(parent, "森林寻宝", "codenames", WIN_W, WIN_H, fonts);
    parent.commands().entity(window_e).with_children(|win| {
        spawn_start_page(win, fonts);
        spawn_game_page(win, fonts);
        spawn_result_page(win, fonts);
    });
}

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

fn page_node() -> Node {
    Node {
        width: Val::Percent(100.0),
        flex_grow: 1.0,
        display: Display::None, // page_sync 按当前页面切换为 Flex
        ..default()
    }
}

// ---------- 开始页 ----------

fn spawn_start_page(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    parent
        .spawn((
            StPageRoot(StPage::Start),
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(14.0),
                ..page_node()
            },
        ))
        .with_children(|page| {
            spawn_text(page, "森林寻宝", 44.0, GOLD, fonts);
            spawn_text(page, "与 AI 合作 · 找出全部 18 个宝箱", 15.0, CREAM_DIM, fonts);
            page.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(10.0),
                    margin: UiRect::top(Val::Px(6.0)),
                    ..default()
                },
            ))
            .with_children(|row| {
                for (d, label) in [(13u32, "简单 13回合"), (11, "中等 11回合"), (9, "困难 9回合")] {
                    row.spawn((
                        Button,
                        StDiffBtn(d),
                        Node {
                            width: Val::Px(130.0),
                            height: Val::Px(44.0),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border_radius: BorderRadius::all(Val::Px(10.0)),
                            ..default()
                        },
                        BackgroundColor(BTN_BG),
                    ))
                    .with_children(|b| {
                        spawn_text(b, label, 15.0, CREAM, fonts);
                    });
                }
            });
            spawn_text(
                page,
                "提示方引导猜测方寻找猜测方的宝箱 · 树莓停止回合并互换角色 · 怪物即死",
                12.0,
                CREAM_DIM,
                fonts,
            );
            page.spawn((
                Button,
                StStartBtn,
                Node {
                    width: Val::Px(220.0),
                    height: Val::Px(52.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border_radius: BorderRadius::all(Val::Px(26.0)),
                    ..default()
                },
                BackgroundColor(GREEN),
            ))
            .with_children(|b| {
                spawn_text(b, "进入森林", 20.0, TEXT_DARK, fonts);
            });
        });
}

// ---------- 游戏页 ----------

fn spawn_game_page(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    parent
        .spawn((
            StPageRoot(StPage::Game),
            Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(12.0)),
                row_gap: Val::Px(8.0),
                ..page_node()
            },
        ))
        .with_children(|page| {
            // 状态栏
            page.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(16.0),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(14.0)),
                    height: Val::Px(34.0),
                    border_radius: BorderRadius::all(Val::Px(10.0)),
                    ..default()
                },
                BackgroundColor(SURFACE),
            ))
            .with_children(|bar| {
                spawn_text(bar, "回合", 13.0, GOLD, fonts);
                bar.spawn((Text::new("0/0"), TextFont {
                    font: ui_font(fonts),
                    font_size: FontSize::Px(13.0),
                    ..default()
                }, TextColor(CREAM), StText::Turns));
                spawn_text(bar, "我方宝箱", 13.0, GOLD, fonts);
                bar.spawn((Text::new("0"), TextFont {
                    font: ui_font(fonts),
                    font_size: FontSize::Px(13.0),
                    ..default()
                }, TextColor(CREAM), StText::HumanT));
                spawn_text(bar, "AI宝箱", 13.0, GOLD, fonts);
                bar.spawn((Text::new("0"), TextFont {
                    font: ui_font(fonts),
                    font_size: FontSize::Px(13.0),
                    ..default()
                }, TextColor(CREAM), StText::AiT));
                spawn_text(bar, "线索", 13.0, GOLD, fonts);
                bar.spawn((Text::new(""), TextFont {
                    font: ui_font(fonts),
                    font_size: FontSize::Px(13.0),
                    ..default()
                }, TextColor(GOLD_LIGHT), StText::Clue));
                spawn_text(bar, "提示", 13.0, GOLD, fonts);
                bar.spawn((Text::new("你"), TextFont {
                    font: ui_font(fonts),
                    font_size: FontSize::Px(13.0),
                    ..default()
                }, TextColor(CREAM), StText::Giver));
                spawn_text(bar, "猜测", 13.0, GOLD, fonts);
                bar.spawn((Text::new("AI"), TextFont {
                    font: ui_font(fonts),
                    font_size: FontSize::Px(13.0),
                    ..default()
                }, TextColor(CREAM), StText::Seeker));
            });

            // 主区域:左卡片网格 + 右对方布局图
            page.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(14.0),
                    flex_grow: 1.0,
                    ..default()
                },
            ))
            .with_children(|main| {
                // 左侧:卡片 + 消息栏
                main.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(8.0),
                        ..default()
                    },
                ))
                .with_children(|left| {
                    // 5x5 卡片
                    left.spawn((
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(CARD_GAP),
                            ..default()
                        },
                    ))
                    .with_children(|grid| {
                        for row in 0..GRID_SIDE {
                            grid.spawn((
                                Node {
                                    flex_direction: FlexDirection::Row,
                                    column_gap: Val::Px(CARD_GAP),
                                    ..default()
                                },
                            ))
                            .with_children(|row_node| {
                                for col in 0..GRID_SIDE {
                                    let idx = row * GRID_SIDE + col;
                                    spawn_card(row_node, idx, fonts);
                                }
                            });
                        }
                    });
                    // 消息栏
                    left.spawn((
                        Node {
                            min_height: Val::Px(40.0),
                            align_items: AlignItems::Center,
                            padding: UiRect {
                                left: Val::Px(12.0),
                                right: Val::Px(12.0),
                                top: Val::Px(6.0),
                                bottom: Val::Px(6.0),
                            },
                            border: UiRect::left(Val::Px(4.0)),
                            border_radius: BorderRadius::all(Val::Px(8.0)),
                            ..default()
                        },
                        BackgroundColor(SURFACE),
                        BorderColor::all(GOLD),
                    ))
                    .with_children(|msg| {
                        msg.spawn((Text::new(""), TextFont {
                            font: ui_font(fonts),
                            font_size: FontSize::Px(13.0),
                            ..default()
                        }, TextColor(CREAM), StText::Message));
                    });
                });

                // 右侧:对方布局图(AI 的道具布局)
                main.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        width: Val::Px(198.0),
                        padding: UiRect::all(Val::Px(10.0)),
                        row_gap: Val::Px(8.0),
                        border_radius: BorderRadius::all(Val::Px(12.0)),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(SURFACE),
                    BorderColor::all(PANEL_BORDER),
                ))
                .with_children(|panel| {
                    spawn_text(panel, "对方布局 (AI)", 13.0, GOLD, fonts);
                    panel.spawn((
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(MAP_GAP),
                            ..default()
                        },
                    ))
                    .with_children(|grid| {
                        for row in 0..GRID_SIDE {
                            grid.spawn((
                                Node {
                                    flex_direction: FlexDirection::Row,
                                    column_gap: Val::Px(MAP_GAP),
                                    ..default()
                                },
                            ))
                            .with_children(|row_node| {
                                for col in 0..GRID_SIDE {
                                    let idx = row * GRID_SIDE + col;
                                    row_node
                                        .spawn((
                                            StMapCell(idx),
                                            Node {
                                                width: Val::Px(MAP_CELL_W),
                                                height: Val::Px(MAP_CELL_H),
                                                align_items: AlignItems::Center,
                                                justify_content: JustifyContent::Center,
                                                border_radius: BorderRadius::all(Val::Px(5.0)),
                                                ..default()
                                            },
                                            BackgroundColor(SURFACE),
                                        ))
                                        .with_children(|cell| {
                                            cell.spawn((Text::new(""), TextFont {
                                                font: ui_font(fonts),
                                                font_size: FontSize::Px(11.0),
                                                ..default()
                                            }, TextColor(CREAM), StMapText(idx)));
                                        });
                                }
                            });
                        }
                    });
                    spawn_text(panel, "宝=宝箱 莓=树莓 怪=怪物", 10.0, CREAM_DIM, fonts);
                    spawn_text(panel, "提示对方找宝箱时参考此图", 10.0, CREAM_DIM, fonts);
                });
            });

            // 操作区:提示词输入 + 数量 + 提交(仅人类提示回合显示)
            page.spawn((
                StActionRow,
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(8.0),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(12.0)),
                    height: Val::Px(46.0),
                    border_radius: BorderRadius::all(Val::Px(12.0)),
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(SURFACE),
            ))
            .with_children(|row| {
                row.spawn((
                    Button,
                    StClueInput,
                    Node {
                        width: Val::Px(220.0),
                        height: Val::Px(32.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::left(Val::Px(12.0)),
                        border_radius: BorderRadius::all(Val::Px(16.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.4)),
                    BorderColor::all(GOLD),
                ))
                .with_children(|input| {
                    input.spawn((Text::new("点击输入提示词"), TextFont {
                        font: ui_font(fonts),
                        font_size: FontSize::Px(13.0),
                        ..default()
                    }, TextColor(CREAM_DIM), StText::Input));
                });
                for n in 1..=MAX_CLUE_NUM {
                    row.spawn((
                        Button,
                        StNumBtn(n),
                        Node {
                            width: Val::Px(28.0),
                            height: Val::Px(28.0),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border_radius: BorderRadius::all(Val::Px(14.0)),
                            ..default()
                        },
                        BackgroundColor(BTN_BG),
                    ))
                    .with_children(|b| {
                        spawn_text(b, &n.to_string(), 13.0, CREAM, fonts);
                    });
                }
                row.spawn((
                    Button,
                    StSubmitBtn,
                    Node {
                        width: Val::Px(110.0),
                        height: Val::Px(32.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(Val::Px(16.0)),
                        ..default()
                    },
                    BackgroundColor(GREEN),
                ))
                .with_children(|b| {
                    spawn_text(b, "发送提示", 14.0, TEXT_DARK, fonts);
                });
            });
        });
}

fn spawn_card(parent: &mut ChildSpawnerCommands, idx: usize, fonts: &N3riFonts) {
    parent
        .spawn((
            Button,
            StCard(idx),
            Node {
                width: Val::Px(CARD_W),
                height: Val::Px(CARD_H),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(10.0)),
                ..default()
            },
            BackgroundColor(CARD_BG),
        ))
        .with_children(|card| {
            card.spawn((Text::new(""), TextFont {
                font: ui_font(fonts),
                font_size: FontSize::Px(16.0),
                ..default()
            }, TextColor(CREAM), StCardWord));
            card.spawn((Text::new(""), TextFont {
                font: ui_font(fonts),
                font_size: FontSize::Px(18.0),
                ..default()
            }, TextColor(CREAM), StCardResult));
        });
}

// ---------- 结果页 ----------

fn spawn_result_page(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    parent
        .spawn((
            StPageRoot(StPage::Result),
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(14.0),
                ..page_node()
            },
        ))
        .with_children(|page| {
            page.spawn((Text::new("胜"), TextFont {
                font: ui_font(fonts),
                font_size: FontSize::Px(52.0),
                ..default()
            }, TextColor(GOLD), StText::ResultBadge));
            page.spawn((Text::new(""), TextFont {
                font: ui_font(fonts),
                font_size: FontSize::Px(28.0),
                ..default()
            }, TextColor(GOLD), StText::ResultTitle));
            page.spawn((Text::new(""), TextFont {
                font: ui_font(fonts),
                font_size: FontSize::Px(15.0),
                ..default()
            }, TextColor(CREAM_DIM), StText::ResultSub));
            page.spawn((Text::new(""), TextFont {
                font: ui_font(fonts),
                font_size: FontSize::Px(15.0),
                ..default()
            }, TextColor(CREAM), StText::ResultStats));
            page.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(12.0),
                    margin: UiRect::top(Val::Px(8.0)),
                    ..default()
                },
            ))
            .with_children(|row| {
                row.spawn((
                    Button,
                    StAgainBtn,
                    Node {
                        width: Val::Px(150.0),
                        height: Val::Px(44.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(Val::Px(22.0)),
                        ..default()
                    },
                    BackgroundColor(GREEN),
                ))
                .with_children(|b| {
                    spawn_text(b, "再来一局", 16.0, TEXT_DARK, fonts);
                });
                row.spawn((
                    Button,
                    StHomeBtn,
                    Node {
                        width: Val::Px(150.0),
                        height: Val::Px(44.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(Val::Px(22.0)),
                        ..default()
                    },
                    BackgroundColor(BTN_BG),
                ))
                .with_children(|b| {
                    spawn_text(b, "返回起点", 16.0, CREAM, fonts);
                });
            });
        });
}

// ==================== 交互系统 ====================

fn st_actions_nav(
    mouse: Res<ButtonInput<MouseButton>>,
    focused: Res<crate::topbar::FocusedTitle>,
    mut game: ResMut<SeekGame>,
    q_diff: Query<(&StDiffBtn, &Interaction)>,
    q_start: Query<&Interaction, With<StStartBtn>>,
    q_again: Query<&Interaction, With<StAgainBtn>>,
    q_home: Query<&Interaction, With<StHomeBtn>>,
) {
    if !mouse.just_pressed(MouseButton::Left) || focused.title != "森林寻宝" {
        return;
    }
    if game.page == StPage::Start {
        for (btn, i) in q_diff.iter() {
            if *i == Interaction::Pressed {
                game.sel_difficulty = btn.0;
            }
        }
        for i in q_start.iter() {
            if *i == Interaction::Pressed {
                game.difficulty = game.sel_difficulty;
                game.start_game();
            }
        }
    } else if game.page == StPage::Result {
        for i in q_again.iter() {
            if *i == Interaction::Pressed {
                game.difficulty = game.sel_difficulty;
                game.start_game();
            }
        }
        for i in q_home.iter() {
            if *i == Interaction::Pressed {
                *game = SeekGame::new();
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn st_actions_game(
    mouse: Res<ButtonInput<MouseButton>>,
    focused: Res<crate::topbar::FocusedTitle>,
    mut game: ResMut<SeekGame>,
    mut owner: ResMut<TextInputOwner>,
    q_cards: Query<(&StCard, &Interaction)>,
    q_nums: Query<(&StNumBtn, &Interaction)>,
    q_submit: Query<&Interaction, With<StSubmitBtn>>,
    q_input: Query<&Interaction, With<StClueInput>>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let in_window = focused.title == "森林寻宝";
    let mut clicked_input = false;
    if in_window && game.page == StPage::Game {
        for i in q_input.iter() {
            if *i == Interaction::Pressed && game.phase == StPhase::HumanClue {
                owner.0 = TextInputFocus::SeekTreasure;
                clicked_input = true;
            }
        }
        for (btn, i) in q_nums.iter() {
            if *i == Interaction::Pressed && game.phase == StPhase::HumanClue {
                game.clue_num = btn.0.clamp(1, MAX_CLUE_NUM);
                clicked_input = true;
            }
        }
        for i in q_submit.iter() {
            if *i == Interaction::Pressed && game.submit_human_clue() {
                owner.0 = TextInputFocus::None;
                clicked_input = true;
            }
        }
        // 卡片点击 = 人类猜测
        let can_guess = game.phase == StPhase::HumanGuessing;
        for (card, i) in q_cards.iter() {
            if *i != Interaction::Pressed {
                continue;
            }
            if !can_guess {
                if !game.grid.is_empty() && !game.grid[card.0].revealed {
                    game.set_message("现在不是你的猜测回合。");
                }
                continue;
            }
            if game.grid[card.0].revealed {
                game.set_message("该卡片已翻开。");
                continue;
            }
            let idx = card.0;
            let word = game.grid[idx].word.clone();
            let detail = game.resolve_guess(idx);
            if game.page != StPage::Result {
                game.set_message(format!("你翻开「{word}」：{detail}"));
            }
        }
    }
    if !clicked_input && owner.is(TextInputFocus::SeekTreasure) {
        owner.0 = TextInputFocus::None;
    }
}

/// 提示词输入:键盘 + IME(中文输入)
fn st_clue_input(
    mut keys: MessageReader<KeyboardInput>,
    mut ime: MessageReader<Ime>,
    focused: Res<crate::topbar::FocusedTitle>,
    owner: Res<TextInputOwner>,
    mut game: ResMut<SeekGame>,
) {
    if game.page != StPage::Game
        || !owner.is(TextInputFocus::SeekTreasure)
        || focused.title != "森林寻宝"
    {
        keys.clear();
        ime.clear();
        return;
    }
    let enabled = game.phase == StPhase::HumanClue;
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
            Key::Enter => {
                game.submit_human_clue();
            }
            _ => {}
        }
    }
}

/// AI 行动调度:LLM 请求派发到后台线程;整批猜测按随机人类节奏逐张翻牌
fn st_ai_act(time: Res<Time>, mut game: ResMut<SeekGame>) {
    if game.page != StPage::Game {
        return;
    }
    if game.phase == StPhase::AiSeeking
        && game.llm_task.is_none()
        && game.llm_rx.is_none()
        && !game.ai_guess_queue.is_empty()
    {
        if game.ai_timer > 0.0 {
            game.ai_timer -= time.delta_secs();
            return;
        }
        let Some(idx) = game.ai_guess_queue.pop_front() else {
            return;
        };
        if idx >= game.grid.len() {
            return;
        }
        let word = game.grid[idx].word.clone();
        let detail = game.resolve_guess(idx);
        if game.page != StPage::Result {
            game.set_message(format!("AI 翻开第 {idx} 格「{word}」：{detail}"));
        }
        if game.phase == StPhase::AiSeeking && !game.ai_guess_queue.is_empty() {
            game.ai_timer = human_click_delay(&mut game);
        }
        return;
    }
    if game.llm_task.is_none() || game.llm_rx.is_some() {
        return;
    }
    if game.ai_timer > 0.0 {
        game.ai_timer -= time.delta_secs();
        return;
    }
    let messages = build_llm_messages(&game);
    let (tx, rx) = channel();
    let client = LlmClient::new();
    let mut cfg = n3ri_llm::load_config();
    if cfg.max_tokens < GAME_MIN_MAX_TOKENS {
        eprintln!(
            "[森林寻宝] max_tokens={} 过小,推理型模型会把预算耗在 reasoning 上导致 content 被截断,已提升到 {GAME_MIN_MAX_TOKENS}",
            cfg.max_tokens
        );
        cfg.max_tokens = GAME_MIN_MAX_TOKENS;
    }
    let model_lower = cfg.model.to_lowercase();
    if model_lower.contains("reasoner") || model_lower.contains("r1") {
        eprintln!(
            "[森林寻宝] 提示: 当前模型 {} 带思考模式,已随请求发送 thinking=disabled;\
若端点不支持该参数,请在设置中改用非思考模型",
            cfg.model
        );
    }
    eprintln!(
        "[森林寻宝] LLM 请求 task={:?} model={} base_url={} max_tokens={} thinking={}",
        game.llm_task,
        cfg.model,
        cfg.base_url,
        cfg.max_tokens,
        if cfg.disable_thinking { "disabled" } else { "default" }
    );
    for (i, m) in messages.iter().enumerate() {
        eprintln!("[森林寻宝]   消息[{i}] {:?}: {}", m.role, m.content);
    }
    thread::spawn(move || {
        let result = client.send(&messages, &cfg);
        eprintln!("[森林寻宝] LLM 后台线程完成: {:?}", result.as_ref().map(|s| snippet(s)));
        let _ = tx.send(result);
    });
    game.llm_rx = Some(Mutex::new(rx));
}

/// LLM 结果轮询;解析失败先重试一次(附纠错反馈),再走兜底
fn st_llm_poll(mut game: ResMut<SeekGame>) {
    if game.page != StPage::Game {
        return;
    }
    let Some(task) = game.llm_task else { return };
    let Some(rx) = game.llm_rx.as_ref() else { return };
    let recv = rx.lock().unwrap().try_recv();
    match recv {
        Ok(Ok(text)) => {
            eprintln!("[森林寻宝] LLM 原始回复({task:?}): {text}");
            let parsed = match task {
                LlmTask::Clue => parse_ai_clue(&text, &game).is_some(),
                LlmTask::Guess => parse_ai_guesses(&text, &game).is_some(),
            };
            eprintln!("[森林寻宝] 解析结果: {parsed} (重试次数={})", game.llm_retries);
            if parsed {
                game.llm_task = None;
                game.llm_rx = None;
                game.llm_retries = 0;
                game.invalid_reply = None;
                match task {
                    LlmTask::Clue => apply_ai_clue(&mut game, &text),
                    LlmTask::Guess => apply_ai_guess(&mut game, &text),
                }
            } else if game.llm_retries < 1 {
                eprintln!("[森林寻宝] 解析失败,要求重试");
                game.llm_retries += 1;
                game.invalid_reply = Some(snippet(&text));
                game.llm_rx = None;
                game.ai_timer = AI_RETRY_DELAY;
                game.set_message("AI 的回复格式无效，正在要求重试…");
            } else {
                eprintln!("[森林寻宝] 重试后仍解析失败,走兜底");
                game.llm_task = None;
                game.llm_rx = None;
                game.llm_retries = 0;
                let err = format!(
                    "返回格式无效：{}",
                    game.invalid_reply.take().unwrap_or_default()
                );
                match task {
                    LlmTask::Clue => fallback_ai_clue(&mut game, &err),
                    LlmTask::Guess => fallback_ai_guess(&mut game, &err),
                }
            }
        }
        Ok(Err(e)) => {
            eprintln!("[森林寻宝] LLM API 错误: {e}");
            game.llm_task = None;
            game.llm_rx = None;
            game.llm_retries = 0;
            game.invalid_reply = None;
            match task {
                LlmTask::Clue => fallback_ai_clue(&mut game, &e),
                LlmTask::Guess => fallback_ai_guess(&mut game, &e),
            }
        }
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => {
            eprintln!("[森林寻宝] LLM 通道断开");
            game.llm_task = None;
            game.llm_rx = None;
            game.llm_retries = 0;
            game.invalid_reply = None;
            match task {
                LlmTask::Clue => fallback_ai_clue(&mut game, "通道断开"),
                LlmTask::Guess => fallback_ai_guess(&mut game, "通道断开"),
            }
        }
    }
}

// ==================== 渲染同步系统 ====================

/// 文本刷新 + 操作区显隐 + 按钮/数字高亮
fn st_ui_update(
    game: Res<SeekGame>,
    mut texts: Query<(&StText, &mut Text)>,
    mut action_rows: Query<&mut Node, With<StActionRow>>,
    mut diff_btns: Query<(&StDiffBtn, &mut BackgroundColor), Without<StNumBtn>>,
    mut num_btns: Query<(&StNumBtn, &mut BackgroundColor), Without<StDiffBtn>>,
) {
    let has_grid = game.grid.len() == BOARD;
    let turns = format!("{}/{}", game.turns_left, game.total_turns);
    let giver = format!("{} →", game.clue_giver.badge());
    let seeker = format!("{} →", game.seeker.badge());
    let clue = if game.clue_word.is_empty() {
        String::new()
    } else {
        format!("「{}」×{}", game.clue_word, game.clue_num)
    };
    for (marker, mut text) in texts.iter_mut() {
        let target = match marker {
            StText::Turns => turns.clone(),
            StText::HumanT => game.human_left.to_string(),
            StText::AiT => game.ai_left.to_string(),
            StText::Clue => clue.clone(),
            StText::Giver => giver.clone(),
            StText::Seeker => seeker.clone(),
            StText::Input => {
                if game.input_buf.is_empty() {
                    String::from("点击输入提示词")
                } else {
                    format!("{}▏", game.input_buf)
                }
            }
            StText::Message => game.message.clone(),
            StText::ResultBadge => {
                if game.win { String::from("胜") } else { String::from("败") }
            }
            StText::ResultTitle => game.result_title.clone(),
            StText::ResultSub => game.result_sub.clone(),
            StText::ResultStats => {
                let found = 18u32.saturating_sub(game.human_left + game.ai_left);
                format!(
                    "宝箱 {found}/18 · 回合 {}/{} · 猜测 {} 次",
                    game.total_turns - game.turns_left,
                    game.total_turns,
                    game.round_count
                )
            }
        };
        if **text != target {
            **text = target;
        }
    }
    // 操作区仅在人类提示回合显示
    let show_actions = game.page == StPage::Game
        && game.phase == StPhase::HumanClue
        && !game.final_stage
        && has_grid;
    for mut node in action_rows.iter_mut() {
        let target = if show_actions { Display::Flex } else { Display::None };
        if node.display != target {
            node.display = target;
        }
    }
    for (btn, mut bg) in diff_btns.iter_mut() {
        let target = if btn.0 == game.sel_difficulty { GOLD } else { BTN_BG };
        if bg.0 != target {
            bg.0 = target;
        }
    }
    for (btn, mut bg) in num_btns.iter_mut() {
        let target = if btn.0 == game.clue_num { GOLD } else { BTN_BG };
        if bg.0 != target {
            bg.0 = target;
        }
    }
}

/// 卡片状态刷新:翻开 → 变暗 + 显示结果字符(按人类布局)
fn st_board_sync(
    mut game: ResMut<SeekGame>,
    mut cards: Query<(&StCard, &mut BackgroundColor, &Children)>,
    mut words: Query<
        (&StCardWord, &mut Text, &mut TextFont, &mut TextColor),
        Without<StCardResult>,
    >,
    mut results: Query<
        (&StCardResult, &mut Text, &mut TextColor),
        Without<StCardWord>,
    >,
) {
    if !game.board_dirty || game.grid.len() != BOARD {
        return;
    }
    game.board_dirty = false;
    for (card, mut bg, children) in cards.iter_mut() {
        let cell = &game.grid[card.0];
        if cell.revealed {
            if bg.0 != CARD_REVEALED_BG {
                bg.0 = CARD_REVEALED_BG;
            }
        } else if bg.0 != CARD_BG {
            bg.0 = CARD_BG;
        }
        for child in children.iter() {
            if let Ok((_, mut text, mut font, mut color)) = words.get_mut(child) {
                let target = cell.word.clone();
                if **text != target {
                    **text = target;
                }
                let size = if cell.revealed { 12.0 } else { 16.0 };
                if font.font_size != FontSize::Px(size) {
                    font.font_size = FontSize::Px(size);
                }
                let color_target = if cell.revealed { CREAM_DIM } else { CREAM };
                if color.0 != color_target {
                    color.0 = color_target;
                }
            }
            if let Ok((_, mut text, mut color)) = results.get_mut(child) {
                let target = if cell.revealed {
                    cell.human.label().to_string()
                } else {
                    String::new()
                };
                if **text != target {
                    **text = target;
                }
                let color_target = if cell.revealed {
                    cell.human.color()
                } else {
                    CREAM
                };
                if color.0 != color_target {
                    color.0 = color_target;
                }
            }
        }
    }
}

/// 对方布局图刷新:AI 的道具布局;已翻开的格子变暗
fn st_map_sync(
    mut game: ResMut<SeekGame>,
    mut cells: Query<(&StMapCell, &mut BackgroundColor)>,
    mut texts: Query<(&StMapText, &mut Text, &mut TextColor), Without<StMapCell>>,
) {
    if !game.map_dirty || game.grid.len() != BOARD {
        return;
    }
    game.map_dirty = false;
    for (cell, mut bg) in cells.iter_mut() {
        let data = &game.grid[cell.0];
        let base = data.ai.color();
        let target = if data.revealed {
            Color::srgba(base.to_srgba().red, base.to_srgba().green, base.to_srgba().blue, 0.12)
        } else {
            Color::srgba(base.to_srgba().red, base.to_srgba().green, base.to_srgba().blue, 0.32)
        };
        if bg.0 != target {
            bg.0 = target;
        }
    }
    for (marker, mut text, mut color) in texts.iter_mut() {
        let data = &game.grid[marker.0];
        let target = data.ai.label().to_string();
        if **text != target {
            **text = target;
        }
        let color_target = if data.revealed { CREAM_DIM } else { data.ai.color() };
        if color.0 != color_target {
            color.0 = color_target;
        }
    }
}

/// 页面显隐切换
fn st_page_sync(game: Res<SeekGame>, mut pages: Query<(&StPageRoot, &mut Node)>) {
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
fn st_cleanup(
    windows: Query<&AppWindow>,
    mut game: ResMut<SeekGame>,
    mut owner: ResMut<TextInputOwner>,
) {
    if windows.iter().any(|w| w.app_id == "codenames") {
        return;
    }
    if owner.is(TextInputFocus::SeekTreasure) {
        owner.0 = TextInputFocus::None;
    }
    if game.page != StPage::Start || game.llm_task.is_some() {
        *game = SeekGame::new();
    }
}

// ==================== 插件 ====================

pub struct SeekTreasurePlugin;

impl Plugin for SeekTreasurePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SeekGame>().add_systems(
            Update,
            (
                st_actions_nav,
                st_actions_game,
                st_clue_input,
                st_ai_act,
                st_llm_poll,
                st_ui_update,
                st_board_sync,
                st_map_sync,
                st_page_sync,
                st_cleanup,
            ),
        );
    }
}

// ==================== 测试 ====================

#[cfg(test)]
mod tests {
    use super::*;

    /// 完整走一遍 spawn_seek_treasure 的 UI 构建路径。
    /// Bundle 重复组件在 queue.apply 注册时会 panic,该测试作为回归防线。
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
                spawn_seek_treasure(root, &fonts);
            });
        }
        queue.apply(&mut world);
    }

    fn test_game(human: [Prop; BOARD], ai: [Prop; BOARD]) -> SeekGame {
        let mut game = SeekGame::new();
        game.page = StPage::Game;
        game.difficulty = 3;
        game.grid = (0..BOARD)
            .map(|i| StCell {
                word: format!("词{i}"),
                human: human[i],
                ai: ai[i],
                revealed: false,
            })
            .collect();
        game.human_left = TREASURE_PER_SIDE as u32;
        game.ai_left = TREASURE_PER_SIDE as u32;
        game.turns_left = game.difficulty;
        game.total_turns = game.difficulty;
        game.clue_giver = Side::Human;
        game.seeker = Side::Ai;
        game.phase = StPhase::HumanClue;
        game
    }

    fn side_layout(treasures: &[usize], monsters: &[usize]) -> [Prop; BOARD] {
        let mut props = [Prop::Raspberry; BOARD];
        for &i in treasures {
            props[i] = Prop::Treasure;
        }
        for &i in monsters {
            props[i] = Prop::Monster;
        }
        props
    }

    #[test]
    fn word_bank_has_enough_words() {
        let bank = load_word_bank();
        assert!(bank.len() >= BOARD, "词库至少要有 25 个词,实际 {}", bank.len());
    }

    #[test]
    fn prop_generation_counts() {
        for _ in 0..20 {
            let mut rng = SeekGame::seed();
            let (human, ai) = gen_props(&mut rng);
            for side in [&human, &ai] {
                assert_eq!(side.iter().filter(|p| **p == Prop::Treasure).count(), 9);
                assert_eq!(side.iter().filter(|p| **p == Prop::Monster).count(), 3);
                assert_eq!(side.iter().filter(|p| **p == Prop::Raspberry).count(), 13);
            }
        }
    }

    #[test]
    fn clue_submission_consumes_turn_and_schedules_ai() {
        let human = side_layout(&[0, 1, 2, 3, 4, 5, 6, 7, 8], &[9, 10, 11]);
        let ai = side_layout(&[12, 13, 14, 15, 16, 17, 18, 19, 20], &[21, 22, 23]);
        let mut game = test_game(human, ai);
        game.input_buf = "词0".into(); // 棋盘词,应被拒绝
        assert!(!game.submit_human_clue());
        game.input_buf = "探索".into();
        game.clue_num = 2;
        assert!(game.submit_human_clue());
        assert_eq!(game.phase, StPhase::AiSeeking);
        assert_eq!(game.turns_left, game.difficulty - 1);
        assert_eq!(game.clue_word, "探索");
        assert_eq!(game.llm_task, Some(LlmTask::Guess));
    }

    #[test]
    fn treasure_lets_seeker_continue_then_swaps() {
        let human = side_layout(&[9, 10, 11, 12, 13, 14, 15, 16, 17], &[20, 21, 22]);
        let ai = side_layout(&[0, 1, 3, 4, 5, 6, 7, 8, 18], &[19, 23, 24]);
        assert_eq!(ai[0], Prop::Treasure);
        assert_eq!(ai[1], Prop::Treasure);
        assert_ne!(human[0], Prop::Monster);
        let mut game = test_game(human, ai);
        game.input_buf = "探索".into();
        game.clue_num = 2;
        assert!(game.submit_human_clue());
        // when: 第一次猜测落在 AI 宝箱
        let detail = game.resolve_guess(0);
        assert_eq!(game.ai_left, 8);
        assert!(detail.contains("找到宝箱"));
        assert_eq!(game.phase, StPhase::AiSeeking);
        // then: 数量用完,互换角色,AI 成为提示方
        game.resolve_guess(1);
        assert_eq!(game.ai_left, 7);
        assert_eq!(game.clue_giver, Side::Ai);
        assert_eq!(game.seeker, Side::Human);
        assert_eq!(game.phase, StPhase::AiClue);
        assert_eq!(game.llm_task, Some(LlmTask::Clue));
    }

    #[test]
    fn shared_treasure_decrements_both() {
        let human = side_layout(&[0, 9, 10, 11, 12, 13, 14, 15, 16], &[1, 2, 3]);
        let ai = side_layout(&[0, 17, 18, 19, 20, 21, 22, 23, 24], &[4, 5, 6]);
        assert_eq!(human[0], Prop::Treasure);
        assert_eq!(ai[0], Prop::Treasure);
        assert_eq!(ai[7], Prop::Raspberry);
        let mut game = test_game(human, ai);
        game.input_buf = "探索".into();
        game.clue_num = 5;
        assert!(game.submit_human_clue());
        // when: 双方重叠的宝箱格被翻开
        let detail = game.resolve_guess(0);
        assert_eq!(game.human_left, 8);
        assert_eq!(game.ai_left, 8);
        assert!(detail.contains("找到宝箱"));
        assert_eq!(game.phase, StPhase::AiSeeking);
        // then: 猜测者自己的树莓格立即停止回合并互换
        game.resolve_guess(7);
        assert_eq!(game.clue_giver, Side::Ai);
        assert_eq!(game.phase, StPhase::AiClue);
    }

    #[test]
    fn raspberry_swaps_roles() {
        let human = side_layout(&[3, 4, 5, 6, 7, 8, 9, 10, 11], &[12, 13, 14]);
        let ai = side_layout(&[15, 16, 17, 18, 19, 20, 21, 22, 23], &[24, 1, 2]);
        assert_eq!(human[0], Prop::Raspberry);
        assert_eq!(ai[0], Prop::Raspberry);
        let mut game = test_game(human, ai);
        game.phase = StPhase::HumanGuessing;
        game.seeker = Side::Human;
        game.clue_giver = Side::Ai;
        game.clue_num = 3;
        // when/then: 踩树莓者成为新的提示方
        game.resolve_guess(0);
        assert_eq!(game.clue_giver, Side::Human);
        assert_eq!(game.seeker, Side::Ai);
        assert_eq!(game.phase, StPhase::HumanClue);
    }

    #[test]
    fn final_stage_raspberry_settles() {
        let human = side_layout(&[0, 9, 10, 11, 12, 13, 14, 15, 16], &[1, 2, 3]);
        let ai = side_layout(&[4, 17, 18, 19, 20, 21, 22, 23, 24], &[5, 6, 7]);
        let mut game = test_game(human, ai);
        game.turns_left = 0;
        game.final_stage = true;
        game.phase = StPhase::HumanGuessing;
        game.seeker = Side::Human;
        // 格子 8 对人类是树莓 → 最后冲刺踩树莓直接结算
        game.resolve_guess(8);
        assert_eq!(game.phase, StPhase::Settled);
        assert!(!game.win);
        assert_eq!(game.page, StPage::Result);
    }

    #[test]
    fn final_stage_entered_when_turns_exhausted() {
        let human = side_layout(&[0, 9, 10, 11, 12, 13, 14, 15, 16], &[1, 2, 3]);
        let ai = side_layout(&[4, 17, 18, 19, 20, 21, 22, 23, 24], &[5, 6, 7]);
        let mut game = test_game(human, ai);
        game.input_buf = "探索".into();
        game.clue_num = 1;
        game.turns_left = 1;
        assert!(game.submit_human_clue());
        assert_eq!(game.turns_left, 0);
        // AI 猜中 AI 宝箱(格 4),数量 1 用完 → 互换 → 进入最后冲刺
        game.resolve_guess(4);
        assert_eq!(game.ai_left, 8);
        assert!(game.final_stage);
        assert_eq!(game.phase, StPhase::HumanGuessing, "人类仍有宝箱,最后冲刺由人类继续猜");
        assert_eq!(game.seeker, Side::Human);
        assert!(game.llm_task.is_none(), "进入最后冲刺必须清除陈旧的 AI 提示任务");
    }

    #[test]
    fn parse_guess_and_clue_json() {
        let human = side_layout(&[0, 1, 2, 3, 4, 5, 6, 7, 8], &[9, 10, 11]);
        let ai = side_layout(&[12, 13, 14, 15, 16, 17, 18, 19, 20], &[21, 22, 23]);
        let game = test_game(human, ai);
        let idx = parse_ai_guesses("好的,我选择 {\"index\": 7} 这一格", &game);
        assert_eq!(idx, Some(vec![7]));
        assert_eq!(parse_ai_guesses("{\"index\": 99}", &game), None);
        let clue = parse_ai_clue("线索如下 {\"word\": \"森林\", \"number\": 3}", &game);
        assert_eq!(clue, Some(("森林".into(), 3)));
        // 线索不能是棋盘词
        assert_eq!(parse_ai_clue("{\"word\": \"词0\", \"number\": 1}", &game), None);
        // 数量截断到 1..=5
        let clue = parse_ai_clue("{\"word\": \"宝物\", \"number\": 9}", &game);
        assert_eq!(clue, Some(("宝物".into(), 5)));
    }

    #[test]
    fn parse_tolerates_string_numbers_fullwidth_and_prose() {
        let human = side_layout(&[0, 1, 2, 3, 4, 5, 6, 7, 8], &[9, 10, 11]);
        let ai = side_layout(&[12, 13, 14, 15, 16, 17, 18, 19, 20], &[21, 22, 23]);
        let game = test_game(human, ai);
        // 字符串数字
        assert_eq!(parse_ai_guesses("{\"index\": \"7\"}", &game), Some(vec![7]));
        // 全角数字/标点/花括号
        assert_eq!(
            parse_ai_guesses("｛\"index\"：１２｝，这是我的选择", &game),
            Some(vec![12])
        );
        // 无 JSON 的散文回复 → 关键词扫描
        assert_eq!(parse_ai_guesses("我选择 index：9 号格子", &game), Some(vec![9]));
        assert_eq!(parse_ai_guesses("我认为 索引 5 最可能是宝箱", &game), Some(vec![5]));
        // markdown 围栏包裹
        assert_eq!(
            parse_ai_guesses("```json\n{\"index\": 6}\n```", &game),
            Some(vec![6])
        );
        // 已翻开的格子无效
        let mut game_revealed = test_game(human, ai);
        game_revealed.grid[20].revealed = true;
        assert_eq!(parse_ai_guesses("{\"index\": 20}", &game_revealed), None);
        // 「词」×N 形式的线索
        assert_eq!(
            parse_ai_clue("我的提示词：「森林」×3，请猜这一类", &game),
            Some(("森林".into(), 3))
        );
        // JSON 里的 word 带引号包裹
        assert_eq!(
            parse_ai_clue("{\"word\": \"「森林」\", \"number\": 2}", &game),
            Some(("森林".into(), 2))
        );
        // 全角冒号数字
        assert_eq!(
            parse_ai_clue("{\"word\"：\"宝物\"，\"number\"：２}", &game),
            Some(("宝物".into(), 2))
        );
    }

    #[test]
    fn win_when_all_treasures_found() {
        let human = side_layout(&[0, 1, 2, 3, 4, 5, 6, 7, 8], &[9, 10, 11]);
        let ai = side_layout(&[0, 1, 2, 3, 4, 5, 6, 7, 8], &[12, 13, 14]);
        let mut game = test_game(human, ai);
        for i in 9..BOARD {
            game.grid[i].revealed = true;
        }
        game.human_left = 1;
        game.ai_left = 1;
        game.phase = StPhase::HumanGuessing;
        game.seeker = Side::Human;
        game.clue_num = 5;
        game.resolve_guess(0);
        assert!(game.win);
        assert_eq!(game.human_left, 0);
        assert_eq!(game.ai_left, 0);
        assert_eq!(game.page, StPage::Result);
    }
}
