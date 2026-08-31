//! 蛋糕对决 —— 诈唬卡牌对决（参照「三段式卡牌」原型与「蛋糕对决卡牌设计.txt」）。
//!
//! 规则:
//! - 20 张牌：士兵×5 / 弓箭手×4 / 法师×3（抢2）/ 盾卫×4（挡士兵弓箭手）/
//!   科学家×3（挡法师）/ 狼爵士×1（不可声明，混入打出被质疑必输）。
//! - 每局 7 蛋糕：进攻方 3 / 防守方 4；先赢 3 局获胜；除首局外输家先攻。
//! - 攻击：牌面朝下打 1~4 张 + 声明一个攻击牌名（可撒谎）。
//! - 防守：回击（打牌 + 声明克制名，可撒谎）/ 质疑 / 接受。
//! - 未质疑按声明属性结算；质疑翻牌按真实属性，且说谎者输局 / 全真质疑者输局。
//! - 每次结算后角色互换、整手弃掉重抽 4 张；双方连续过牌本局结束，蛋糕多者胜。

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::text::{FontSource, FontSize};

use crate::font::N3riFonts;
use crate::window::spawn_window;

const WIN_W: f32 = 980.0;
const WIN_H: f32 = 620.0;
const HAND_N: usize = 4;
const WIN_ROUNDS: u32 = 3;
const AI_DELAY: f32 = 0.9;
const RESOLVE_DELAY: f32 = 1.5;
const BATTLE_END_DELAY: f32 = 2.0;

const CK_BROWN: Color = Color::srgb(0.42, 0.28, 0.19);
const CK_AMBER: Color = Color::srgb(0.84, 0.55, 0.32);
const CK_CREAM: Color = Color::srgb(1.0, 0.97, 0.94);
const CK_DIM: Color = Color::srgba(0.42, 0.28, 0.19, 0.65);
const CK_PANEL: Color = Color::srgba(1.0, 1.0, 1.0, 0.45);
const CK_SEL: Color = Color::srgb(0.98, 0.78, 0.23);
const CK_GREEN: Color = Color::srgb(0.35, 0.62, 0.3);

// ==================== 牌与规则 ====================

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum CardType {
    Soldier,
    Archer,
    Wizard,
    Defender,
    Scientist,
    Wolf,
}

const ALL_TYPES: [CardType; 6] = [
    CardType::Soldier,
    CardType::Archer,
    CardType::Wizard,
    CardType::Defender,
    CardType::Scientist,
    CardType::Wolf,
];

impl CardType {
    fn name(self) -> &'static str {
        match self {
            CardType::Soldier => "士兵",
            CardType::Archer => "弓箭手",
            CardType::Wizard => "法师",
            CardType::Defender => "盾卫",
            CardType::Scientist => "科学家",
            CardType::Wolf => "狼爵士",
        }
    }
    fn count(self) -> u32 {
        match self {
            CardType::Soldier => 5,
            CardType::Archer => 4,
            CardType::Wizard => 3,
            CardType::Defender => 4,
            CardType::Scientist => 3,
            CardType::Wolf => 1,
        }
    }
    fn steal(self) -> u32 {
        match self {
            CardType::Wizard => 2,
            CardType::Soldier | CardType::Archer => 1,
            _ => 0,
        }
    }
    fn face(self) -> &'static str {
        match self {
            CardType::Soldier => "nori/cakeduel/cards/zh-CN/soldier.jpg",
            CardType::Archer => "nori/cakeduel/cards/zh-CN/archer.jpg",
            CardType::Wizard => "nori/cakeduel/cards/zh-CN/wizard.jpg",
            CardType::Defender => "nori/cakeduel/cards/zh-CN/defender.jpg",
            CardType::Scientist => "nori/cakeduel/cards/zh-CN/scientist.jpg",
            CardType::Wolf => "nori/cakeduel/cards/zh-CN/wolfy.jpg",
        }
    }
    fn counters(self, attack: CardType) -> bool {
        matches!(
            (self, attack),
            (CardType::Defender, CardType::Soldier)
                | (CardType::Defender, CardType::Archer)
                | (CardType::Scientist, CardType::Wizard)
        )
    }
    fn claimable_defense(self) -> bool {
        matches!(self, CardType::Defender | CardType::Scientist)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    Human,
    Ai,
}

impl Side {
    fn other(self) -> Side {
        match self {
            Side::Human => Side::Ai,
            Side::Ai => Side::Human,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Side::Human => "你",
            Side::Ai => "Nori",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Difficulty {
    Soldier,
    Wizard,
    Assassin,
}

impl Difficulty {
    fn label(self) -> &'static str {
        match self {
            Difficulty::Soldier => "士兵 · 简单",
            Difficulty::Wizard => "法师 · 普通",
            Difficulty::Assassin => "刺客 · 困难",
        }
    }
    /// (bluff, challenge, mistake)
    fn params(self) -> (f32, f32, f32) {
        match self {
            Difficulty::Soldier => (0.15, 0.12, 0.30),
            Difficulty::Wizard => (0.35, 0.30, 0.10),
            Difficulty::Assassin => (0.55, 0.45, 0.03),
        }
    }
}

#[derive(Clone, Debug)]
struct CkPlay {
    cards: Vec<CardType>,
    claim: CardType,
}

impl CkPlay {
    fn steal_by_claim(&self) -> u32 {
        self.cards.len() as u32 * self.claim.steal()
    }
    fn truthful(&self) -> bool {
        self.cards.iter().all(|c| *c == self.claim)
    }
}

// ==================== 游戏状态 ====================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CkPage {
    Start,
    Game,
    Result,
}

/// AI 进入自己回合时的角色（显式分派，避免按 attacker 猜测导致歧义）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AiRole {
    Attack,
    Defend,
    ChallengeBlock,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Start,
    PlayerAct,
    AiAct,
    Resolve,
    BattleEnd,
    MatchEnd,
}

/// Resolve 展示结束后走哪条路
#[derive(Clone, Copy, Debug)]
enum AfterResolve {
    Swap,
    BattleEnd(Side, &'static str),
}

#[derive(Resource)]
struct CakeduelGame {
    page: CkPage,
    phase: Phase,
    sel_difficulty: Difficulty,
    difficulty: Difficulty,
    wins_human: u32,
    wins_ai: u32,
    attacker: Side,
    cakes_human: u32,
    cakes_ai: u32,
    hand_human: Vec<CardType>,
    hand_ai: Vec<CardType>,
    sel: Vec<usize>,
    defense_selecting: bool,
    deck: Vec<CardType>,
    discard: Vec<CardType>,
    attack_play: Option<CkPlay>,
    defense_play: Option<CkPlay>,
    ai_role: AiRole,
    last_pass: bool,
    reveal: bool,
    timer: f32,
    after_resolve: Option<AfterResolve>,
    message: String,
    result_title: String,
    result_sub: String,
    last_winner: Side,
    dirty: bool,
    rng: u64,
}

impl Default for CakeduelGame {
    fn default() -> Self {
        Self::new()
    }
}

impl CakeduelGame {
    fn new() -> Self {
        Self {
            page: CkPage::Start,
            phase: Phase::Start,
            sel_difficulty: Difficulty::Soldier,
            difficulty: Difficulty::Soldier,
            wins_human: 0,
            wins_ai: 0,
            attacker: Side::Human,
            cakes_human: 3,
            cakes_ai: 4,
            hand_human: Vec::new(),
            hand_ai: Vec::new(),
            sel: Vec::new(),
            defense_selecting: false,
            deck: Vec::new(),
            discard: Vec::new(),
            attack_play: None,
            defense_play: None,
            ai_role: AiRole::Defend,
            last_pass: false,
            reveal: false,
            timer: 0.0,
            after_resolve: None,
            message: String::new(),
            result_title: String::new(),
            result_sub: String::new(),
            last_winner: Side::Human,
            dirty: true,
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

    fn rand(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn rand_f32(&mut self) -> f32 {
        (self.rand_index(10_000) as f32) / 10_000.0
    }

    fn rand_index(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.rand() % n as u64) as usize
        }
    }

    // ---------- 开局 ----------

    fn start_match(&mut self, diff: Difficulty) {
        self.difficulty = diff;
        self.wins_human = 0;
        self.wins_ai = 0;
        self.page = CkPage::Game;
        self.start_battle(Side::Human);
    }

    fn start_battle(&mut self, attacker: Side) {
        self.attacker = attacker;
        let (a, d) = (3, 4);
        (self.cakes_human, self.cakes_ai) = match attacker {
            Side::Human => (a, d),
            Side::Ai => (d, a),
        };
        self.discard.clear();
        self.deck = self.build_deck();
        self.hand_human.clear();
        self.hand_ai.clear();
        self.draw_up(Side::Human);
        self.draw_up(Side::Ai);
        self.attack_play = None;
        self.defense_play = None;
        self.sel.clear();
        self.defense_selecting = false;
        self.ai_role = match attacker {
            Side::Human => AiRole::Defend,
            Side::Ai => AiRole::Attack,
        };
        self.last_pass = false;
        self.reveal = false;
        self.after_resolve = None;
        self.dirty = true;
        self.message = format!(
            "第 {} 局开始：{} 先攻（攻方 3 蛋糕，守方 4 蛋糕）。",
            self.wins_human + self.wins_ai + 1,
            attacker.name()
        );
        self.phase = match attacker {
            Side::Human => Phase::PlayerAct,
            Side::Ai => {
                self.timer = AI_DELAY;
                Phase::AiAct
            }
        };
    }

    fn build_deck(&mut self) -> Vec<CardType> {
        let mut deck: Vec<CardType> = ALL_TYPES
            .iter()
            .flat_map(|t| std::iter::repeat_n(*t, t.count() as usize))
            .collect();
        for i in (1..deck.len()).rev() {
            let j = self.rand_index(i + 1);
            deck.swap(i, j);
        }
        deck
    }

    fn draw_up(&mut self, side: Side) {
        let hand_len = match side {
            Side::Human => self.hand_human.len(),
            Side::Ai => self.hand_ai.len(),
        };
        for _ in hand_len..HAND_N {
            if self.deck.is_empty() {
                if self.discard.is_empty() {
                    break;
                }
                let mut refilled = std::mem::take(&mut self.discard);
                for i in (1..refilled.len()).rev() {
                    let j = self.rand_index(i + 1);
                    refilled.swap(i, j);
                }
                self.deck = refilled;
            }
            let Some(card) = self.deck.pop() else { break };
            match side {
                Side::Human => self.hand_human.push(card),
                Side::Ai => self.hand_ai.push(card),
            }
        }
        self.dirty = true;
    }

    // ---------- 结算 ----------

    /// 攻方行动被接受（无防守牌）：按声明属性扣守方蛋糕
    fn settle_accept(&mut self) {
        let play = self.attack_play.clone().unwrap_or(CkPlay {
            cards: vec![],
            claim: CardType::Soldier,
        });
        let stolen = play.steal_by_claim();
        self.deduct_defender(stolen);
        self.last_pass = false;
        self.message = format!(
            "{} 接受进攻：{} 张「{}」未被挡住，抢走 {} 个蛋糕。",
            self.defender().name(),
            play.cards.len(),
            play.claim.name(),
            stolen
        );
        self.after_resolve = Some(self.battle_over_or_swap());
        self.phase = Phase::Resolve;
        self.timer = RESOLVE_DELAY;
    }

    /// 防守回击未被质疑：按双方声明判挡
    fn settle_block(&mut self) {
        let attack = self.attack_play.clone().expect("block needs attack");
        let block = self.defense_play.clone().expect("settle_block needs block");
        let blocked = if block.claim.counters(attack.claim) {
            block.cards.len().min(attack.cards.len()) as u32
        } else {
            0
        };
        let unblocked = attack.cards.len() as u32 - blocked;
        let stolen = unblocked * attack.claim.steal();
        self.deduct_defender(stolen);
        self.last_pass = false;
        self.message = if blocked > 0 {
            format!(
                "{} 声明「{}」防守：挡住 {} 张，「{}」×{} 未被挡住，抢走 {} 个蛋糕。",
                self.defender().name(),
                block.claim.name(),
                blocked,
                attack.claim.name(),
                unblocked,
                stolen
            )
        } else {
            format!(
                "{} 声明「{}」防守：克不住「{}」，全部 {} 张未被挡住，抢走 {} 个蛋糕。",
                self.defender().name(),
                block.claim.name(),
                attack.claim.name(),
                attack.cards.len(),
                stolen
            )
        };
        self.after_resolve = Some(self.battle_over_or_swap());
        self.phase = Phase::Resolve;
        self.timer = RESOLVE_DELAY;
    }

    fn defender(&self) -> Side {
        self.attacker.other()
    }

    fn deduct_defender(&mut self, n: u32) {
        match self.defender() {
            Side::Human => self.cakes_human = self.cakes_human.saturating_sub(n),
            Side::Ai => self.cakes_ai = self.cakes_ai.saturating_sub(n),
        }
        self.dirty = true;
    }

    /// 结算后：蛋糕见底则本局结束，否则换位刷新
    fn battle_over_or_swap(&self) -> AfterResolve {
        if self.cakes_human == 0 {
            AfterResolve::BattleEnd(Side::Ai, "蛋糕被抢光")
        } else if self.cakes_ai == 0 {
            AfterResolve::BattleEnd(Side::Human, "蛋糕被抢光")
        } else {
            AfterResolve::Swap
        }
    }

    /// 质疑结算：翻牌，按真实属性判罚，直接分出本局胜负
    fn resolve_challenge(&mut self, target: &CkPlay, target_side: Side, by: Side) {
        self.reveal = true;
        self.dirty = true;
        let cards_desc: Vec<&str> = target.cards.iter().map(|c| c.name()).collect();
        // 全部相符 → 质疑者输；有假 → 说谎者输
        let winner = if target.truthful() { target_side } else { by };
        self.message = format!(
            "{} 质疑！翻牌：[{}]，声明「{}」——{}",
            by.name(),
            cards_desc.join("、"),
            target.claim.name(),
            if target.truthful() {
                format!("全部相符，质疑者{}输掉本局！", by.name())
            } else {
                format!("与声明不符，说谎者{}输掉本局！", target_side.name())
            }
        );
        self.begin_battle_end(winner, "质疑判罚");
    }

    fn begin_battle_end(&mut self, winner: Side, reason: &str) {
        match winner {
            Side::Human => self.wins_human += 1,
            Side::Ai => self.wins_ai += 1,
        }
        self.last_winner = winner;
        self.phase = Phase::BattleEnd;
        self.timer = BATTLE_END_DELAY;
        self.after_resolve = None;
        self.dirty = true;
        let over = self.wins_human >= WIN_ROUNDS || self.wins_ai >= WIN_ROUNDS;
        self.message = format!(
            "{}赢下本局！（{}）比分 {} : {}{}",
            winner.name(),
            reason,
            self.wins_human,
            self.wins_ai,
            if over { "，比赛结束！" } else { "" }
        );
    }

    /// 本局结束后的展示到期：进入下一局或终局
    fn after_battle_end(&mut self) {
        if self.wins_human >= WIN_ROUNDS || self.wins_ai >= WIN_ROUNDS {
            let (title, sub) = if self.wins_human >= WIN_ROUNDS {
                ("胜利！", format!("以 {} : {} 击败 Nori，蛋糕守住！", self.wins_human, self.wins_ai))
            } else {
                ("败北…", format!("Nori 以 {} : {} 拿下比赛，下次再战。", self.wins_ai, self.wins_human))
            };
            self.result_title = title.to_string();
            self.result_sub = sub;
            self.page = CkPage::Result;
            self.phase = Phase::MatchEnd;
            self.dirty = true;
        } else {
            // 输家先攻
            self.start_battle(self.last_winner.other());
        }
    }

    /// 换位 + 整手弃掉重抽
    fn swap_and_refresh(&mut self) {
        if let Some(p) = &self.attack_play {
            self.discard.extend(p.cards.iter().copied());
        }
        if let Some(p) = &self.defense_play {
            self.discard.extend(p.cards.iter().copied());
        }
        self.attack_play = None;
        self.defense_play = None;
        self.discard.extend(self.hand_human.drain(..));
        self.discard.extend(self.hand_ai.drain(..));
        self.attacker = self.attacker.other();
        self.ai_role = match self.attacker {
            Side::Human => AiRole::Defend,
            Side::Ai => AiRole::Attack,
        };
        self.draw_up(Side::Human);
        self.draw_up(Side::Ai);
        self.sel.clear();
        self.defense_selecting = false;
        self.reveal = false;
        self.dirty = true;
        self.phase = match self.attacker {
            Side::Human => Phase::PlayerAct,
            Side::Ai => {
                self.timer = AI_DELAY;
                Phase::AiAct
            }
        };
    }

    fn pass_turn(&mut self, side: Side) {
        if self.last_pass {
            let winner = if self.cakes_human > self.cakes_ai {
                Side::Human
            } else {
                Side::Ai
            };
            self.message = "双方连续过牌，本局结束——蛋糕更多的一方获胜。".to_string();
            self.begin_battle_end(winner, "连续过牌");
            return;
        }
        self.last_pass = true;
        self.message = format!("{}选择过牌，放弃本次进攻。", side.name());
        self.swap_and_refresh();
    }
}

// ==================== AI ====================

enum BlockOrChallenge {
    Block(CkPlay),
    Challenge,
    Accept,
}

fn ai_take_turn(game: &mut CakeduelGame) {
    let (bluff, challenge, mistake) = game.difficulty.params();
    match game.ai_role {
        AiRole::Attack => {
            let play = ai_choose_attack(game, bluff, mistake);
            match play {
                Some(p) => {
                    let claim = p.claim;
                    let n = p.cards.len();
                    game.attack_play = Some(p);
                    game.message =
                        format!("Nori 打出 {n} 张牌，声明「{}」。", claim.name());
                    game.last_pass = false;
                    game.dirty = true;
                    game.phase = Phase::PlayerAct;
                }
                None => game.pass_turn(Side::Ai),
            }
        }
        AiRole::ChallengeBlock => {
            let do_it = game.rand_f32() < challenge * 0.6;
            if do_it {
                let block = game.defense_play.clone().expect("block present");
                game.resolve_challenge(&block, Side::Human, Side::Ai);
            } else {
                game.message = "Nori 没有质疑你的防守。".to_string();
                game.settle_block();
            }
        }
        AiRole::Defend => {
            let attack = game.attack_play.clone().expect("defend needs attack");
            let decision = ai_choose_defense(game, &attack, bluff, challenge, mistake);
            match decision {
                BlockOrChallenge::Block(play) => {
                    let claim = play.claim;
                    let n = play.cards.len();
                    game.defense_play = Some(play);
                    game.message = format!("Nori 回击：打出 {n} 张牌，声明「{}」。", claim.name());
                    game.phase = Phase::PlayerAct;
                    game.dirty = true;
                }
                BlockOrChallenge::Challenge => {
                    game.resolve_challenge(&attack, Side::Human, Side::Ai);
                }
                BlockOrChallenge::Accept => {
                    game.message = "Nori 接受进攻。".to_string();
                    game.settle_accept();
                }
            }
        }
    }
}

fn ai_choose_attack(game: &mut CakeduelGame, bluff: f32, mistake: f32) -> Option<CkPlay> {
    let hand = game.hand_ai.clone();
    let mut by_type: HashMap<CardType, Vec<usize>> = HashMap::new();
    for (i, c) in hand.iter().enumerate() {
        by_type.entry(*c).or_default().push(i);
    }
    let mut plans: Vec<(CkPlay, f32)> = Vec::new();
    let claimable = [CardType::Wizard, CardType::Soldier, CardType::Archer];
    for t in claimable {
        if let Some(idx) = by_type.get(&t) {
            if !idx.is_empty() {
                let cards: Vec<CardType> = idx.iter().map(|i| hand[*i]).collect();
                let score = 0.5 + 0.15 * cards.len() as f32 + t.steal() as f32 * 0.2;
                plans.push((CkPlay { cards, claim: t }, score));
            }
        }
    }
    if game.rand_f32() < bluff {
        let junk: Vec<CardType> = hand
            .iter()
            .copied()
            .filter(|c| !matches!(c, CardType::Wizard))
            .take(2)
            .collect();
        if !junk.is_empty() {
            let claim = if game.rand_f32() < 0.6 { CardType::Wizard } else { CardType::Soldier };
            plans.push((CkPlay { cards: junk, claim }, 0.55));
        }
    }
    if plans.is_empty() || game.rand_f32() < 0.08 {
        return None;
    }
    if game.rand_f32() < mistake {
        let i = game.rand_index(plans.len());
        return plans.into_iter().nth(i).map(|(p, _)| p);
    }
    plans.sort_by(|a, b| b.1.total_cmp(&a.1));
    Some(plans.remove(0).0)
}

fn ai_choose_defense(
    game: &mut CakeduelGame,
    attack: &CkPlay,
    bluff: f32,
    challenge: f32,
    mistake: f32,
) -> BlockOrChallenge {
    let hand = game.hand_ai.clone();
    let counters: Vec<CardType> = hand
        .iter()
        .copied()
        .filter(|c| c.counters(attack.claim))
        .collect();
    let suspicion = match attack.claim {
        CardType::Wizard => 0.6,
        CardType::Soldier => 0.25,
        _ => 0.35,
    } + 0.07 * attack.cards.len() as f32;
    let steal_risk = attack.steal_by_claim();

    let mut plans: Vec<(BlockOrChallenge, f32)> = Vec::new();
    if !counters.is_empty() {
        let take = counters.len().min(attack.cards.len());
        plans.push((
            BlockOrChallenge::Block(CkPlay {
                cards: counters[..take].to_vec(),
                claim: counters[0],
            }),
            0.75 + 0.05 * take as f32,
        ));
    }
    plans.push((BlockOrChallenge::Challenge, challenge * suspicion * 2.0));
    if game.rand_f32() < bluff {
        let junk: Vec<CardType> = hand
            .iter()
            .copied()
            .filter(|c| !c.counters(attack.claim))
            .take(attack.cards.len())
            .collect();
        if !junk.is_empty() {
            let claim = if attack.claim == CardType::Wizard {
                CardType::Scientist
            } else {
                CardType::Defender
            };
            plans.push((BlockOrChallenge::Block(CkPlay { cards: junk, claim }), 0.35));
        }
    }
    plans.push((BlockOrChallenge::Accept, if steal_risk >= 3 { 0.15 } else { 0.55 }));

    if game.rand_f32() < mistake {
        let i = game.rand_index(plans.len());
        return plans.remove(i).0;
    }
    plans.sort_by(|a, b| b.1.total_cmp(&a.1));
    plans.remove(0).0
}

// ==================== 组件标记 ====================

#[derive(Component)]
struct CkPageRoot(CkPage);

#[derive(Component)]
struct CkDiff(Difficulty);

#[derive(Component)]
struct CkStartBtn;

#[derive(Component)]
struct CkAgainBtn;

#[derive(Component)]
struct CkHomeBtn;

#[derive(Component)]
struct CkText(CkSlot);

#[derive(Component)]
struct CkActionPanel;

#[derive(Component)]
struct CkHandCard(usize);

#[derive(Component)]
struct CkClaimBtn(CardType);

#[derive(Component)]
struct CkDefenseAct(u8); // 0=质疑 1=接受 2=回击 3=放行

#[derive(Component)]
struct CkPassBtn;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CkSlot {
    Phase,
    Score,
    Message,
    ResultTitle,
    ResultSub,
    DeckCount,
}

#[derive(Component)]
struct CkHandRow;

#[derive(Component)]
struct CkAiHandRow;

#[derive(Component)]
struct CkPileRow;

#[derive(Component)]
struct CkDeckBox;

#[derive(Component)]
struct CkCakeRail;

#[derive(Component)]
struct CkBg {
    veiled: bool,
}

// ==================== 贴图资源 ====================

#[derive(Resource, Default)]
struct CkAssets {
    loaded: Option<CkTex>,
}

struct CkTex {
    playmat: Handle<Image>,
    cake: Handle<Image>,
    back: Handle<Image>,
    faces: HashMap<CardType, Handle<Image>>,
}

fn load_ck_assets(server: &AssetServer) -> CkTex {
    let faces = ALL_TYPES
        .iter()
        .map(|t| (*t, server.load(t.face())))
        .collect();
    CkTex {
        playmat: server.load("nori/cakeduel/playmat.jpg"),
        cake: server.load("nori/cakeduel/cake.png"),
        back: server.load("nori/cakeduel/card-back.jpg"),
        faces,
    }
}

// ==================== UI 构建 ====================

pub fn spawn_cakeduel(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    // 每次打开都重置对局：关窗后资源残留的战局/页面/脏标记不应带入新窗口
    parent.commands().insert_resource(CakeduelGame::new());
    let window_e = spawn_window(parent, "蛋糕对决", "cakeduel", WIN_W, WIN_H, fonts);
    parent.commands().entity(window_e).with_children(|win| {
        spawn_start_page(win, fonts);
        spawn_game_page(win, fonts);
        spawn_result_page(win, fonts);
    });
}

fn ck_font(fonts: &N3riFonts) -> FontSource {
    FontSource::Handle(fonts.default.clone())
}

fn ck_text(parent: &mut ChildSpawnerCommands, s: &str, size: f32, color: Color, fonts: &N3riFonts) {
    parent.spawn((
        Text::new(s),
        TextFont { font: ck_font(fonts), font_size: FontSize::Px(size), ..default() },
        TextColor(color),
    ));
}

fn page_node() -> Node {
    Node {
        width: Val::Percent(100.0),
        flex_grow: 1.0,
        display: Display::None,
        ..default()
    }
}

fn spawn_start_page(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    parent
        .spawn((
            CkPageRoot(CkPage::Start),
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(12.0),
                ..page_node()
            },
            BackgroundColor(CK_PANEL),
        ))
        .with_children(|page| {
            page.spawn((CkBg { veiled: true }, Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(0.0),
                ..default()
            },));
            ck_text(page, "蛋糕对决", 44.0, CK_BROWN, fonts);
            ck_text(page, "诈唬 · 克制 · 质疑 —— 先赢 3 局者获胜", 14.0, CK_DIM, fonts);
            page.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(10.0),
                    ..default()
                },
            ))
            .with_children(|row| {
                for d in [Difficulty::Soldier, Difficulty::Wizard, Difficulty::Assassin] {
                    row.spawn((
                        Button,
                        CkDiff(d),
                        Node {
                            width: Val::Px(150.0),
                            height: Val::Px(48.0),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border: UiRect::all(Val::Px(2.0)),
                            border_radius: BorderRadius::all(Val::Px(10.0)),
                            ..default()
                        },
                        BackgroundColor(CK_PANEL),
                        BorderColor::all(CK_AMBER),
                    ))
                    .with_children(|b| ck_text(b, d.label(), 14.0, CK_BROWN, fonts));
                }
            });
            page.spawn((
                Button,
                CkStartBtn,
                Node {
                    width: Val::Px(220.0),
                    height: Val::Px(52.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border_radius: BorderRadius::all(Val::Px(26.0)),
                    ..default()
                },
                BackgroundColor(CK_AMBER),
            ))
            .with_children(|b| ck_text(b, "开始决斗", 20.0, CK_CREAM, fonts));
            ck_text(
                page,
                "攻方 3 蛋糕 · 守方 4 蛋糕 · 回击可撒谎 · 质疑翻牌定胜负",
                12.0,
                CK_DIM,
                fonts,
            );
        });
}

fn spawn_game_page(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    parent
        .spawn((
            CkPageRoot(CkPage::Game),
            Node {
                flex_direction: FlexDirection::Column,
                ..page_node()
            },
        ))
        .with_children(|page| {
            // 背景牌垫（渲染同步时填充贴图）
            page.spawn((CkBg { veiled: false }, Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(0.0),
                ..default()
            },));
            // 顶栏
            page.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    height: Val::Px(42.0),
                    padding: UiRect::horizontal(Val::Px(14.0)),
                    ..default()
                },
                BackgroundColor(CK_PANEL),
            ))
            .with_children(|bar| {
                bar.spawn((
                    Text::new(""),
                    TextFont { font: ck_font(fonts), font_size: FontSize::Px(15.0), ..default() },
                    TextColor(CK_BROWN),
                    CkText(CkSlot::Phase),
                ));
                bar.spawn((
                    Text::new(""),
                    TextFont { font: ck_font(fonts), font_size: FontSize::Px(15.0), ..default() },
                    TextColor(CK_BROWN),
                    CkText(CkSlot::Score),
                ));
            });
            // 主体：左蛋糕栏 + 中央 + 右牌堆
            page.spawn((Node { flex_grow: 1.0, ..default() },))
                .with_children(|body| {
                    body.spawn((CkCakeRail, Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(8.0),
                        top: Val::Px(8.0),
                        bottom: Val::Px(8.0),
                        width: Val::Px(52.0),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::SpaceBetween,
                        padding: UiRect::new(
                            Val::Auto, Val::Auto,
                            Val::Px(8.0), Val::Px(8.0),
                        ),
                        ..default()
                    },));
                    // 中央列
                    body.spawn((Node {
                        flex_direction: FlexDirection::Column,
                        position_type: PositionType::Absolute,
                        left: Val::Px(70.0),
                        right: Val::Px(80.0),
                        top: Val::Px(0.0),
                        bottom: Val::Px(0.0),
                        ..default()
                    },))
                    .with_children(|mid| {
                        // AI 手牌
                        mid.spawn((CkAiHandRow, Node {
                            flex_direction: FlexDirection::Row,
                            justify_content: JustifyContent::Center,
                            height: Val::Px(84.0),
                            column_gap: Val::Px(-20.0),
                            ..default()
                        },));
                        // 中央牌堆区
                        mid.spawn((CkPileRow, Node {
                            flex_direction: FlexDirection::Row,
                            justify_content: JustifyContent::Center,
                            column_gap: Val::Px(40.0),
                            flex_grow: 1.0,
                            align_items: AlignItems::Center,
                            ..default()
                        },));
                        // 消息
                        mid.spawn((Node {
                            justify_content: JustifyContent::Center,
                            padding: UiRect::vertical(Val::Px(4.0)),
                            ..default()
                        },))
                        .with_children(|m| {
                            m.spawn((
                                Text::new(""),
                                TextFont { font: ck_font(fonts), font_size: FontSize::Px(14.0), ..default() },
                                TextColor(CK_BROWN),
                                CkText(CkSlot::Message),
                            ));
                        });
                        // 操作面板
                        mid.spawn((CkActionPanel, Node {
                            flex_direction: FlexDirection::Row,
                            justify_content: JustifyContent::Center,
                            column_gap: Val::Px(8.0),
                            height: Val::Px(64.0),
                            align_items: AlignItems::Center,
                            ..default()
                        },));
                        // 玩家手牌
                        mid.spawn((CkHandRow, Node {
                            flex_direction: FlexDirection::Row,
                            justify_content: JustifyContent::Center,
                            height: Val::Px(150.0),
                            column_gap: Val::Px(-34.0),
                            ..default()
                        },));
                    });
                    // 右侧牌堆
                    body.spawn((CkDeckBox, Node {
                        position_type: PositionType::Absolute,
                        right: Val::Px(8.0),
                        top: Val::Px(80.0),
                        width: Val::Px(64.0),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: Val::Px(4.0),
                        ..default()
                    },));
                });
        });
}

fn spawn_result_page(parent: &mut ChildSpawnerCommands, fonts: &N3riFonts) {
    parent
        .spawn((
            CkPageRoot(CkPage::Result),
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(14.0),
                ..page_node()
            },
            BackgroundColor(CK_PANEL),
        ))
        .with_children(|page| {
            page.spawn((CkBg { veiled: true }, Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(0.0),
                ..default()
            },));
            page.spawn((
                Text::new(""),
                TextFont { font: ck_font(fonts), font_size: FontSize::Px(46.0), ..default() },
                TextColor(CK_BROWN),
                CkText(CkSlot::ResultTitle),
            ));
            page.spawn((
                Text::new(""),
                TextFont { font: ck_font(fonts), font_size: FontSize::Px(16.0), ..default() },
                TextColor(CK_DIM),
                CkText(CkSlot::ResultSub),
            ));
            page.spawn((Node { column_gap: Val::Px(10.0), ..default() },))
                .with_children(|row| {
                    row.spawn((
                        Button,
                        CkAgainBtn,
                        Node {
                            width: Val::Px(150.0),
                            height: Val::Px(46.0),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border_radius: BorderRadius::all(Val::Px(23.0)),
                            ..default()
                        },
                        BackgroundColor(CK_AMBER),
                    ))
                    .with_children(|b| ck_text(b, "再来一局", 16.0, CK_CREAM, fonts));
                    row.spawn((
                        Button,
                        CkHomeBtn,
                        Node {
                            width: Val::Px(150.0),
                            height: Val::Px(46.0),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border_radius: BorderRadius::all(Val::Px(23.0)),
                            ..default()
                        },
                        BackgroundColor(CK_PANEL),
                    ))
                    .with_children(|b| ck_text(b, "返回起点", 16.0, CK_BROWN, fonts));
                });
        });
}

// ==================== 交互系统 ====================

fn ck_focused(focused: &crate::topbar::FocusedTitle) -> bool {
    focused.title == "蛋糕对决"
}

fn st_ck_nav(
    mouse: Res<ButtonInput<MouseButton>>,
    focused: Res<crate::topbar::FocusedTitle>,
    mut game: ResMut<CakeduelGame>,
    q_diff: Query<(&CkDiff, &Interaction)>,
    q_start: Query<&Interaction, With<CkStartBtn>>,
    q_again: Query<&Interaction, With<CkAgainBtn>>,
    q_home: Query<&Interaction, With<CkHomeBtn>>,
) {
    if !mouse.just_pressed(MouseButton::Left) || !ck_focused(&focused) {
        return;
    }
    if game.page == CkPage::Start {
        for (btn, i) in q_diff.iter() {
            if *i == Interaction::Pressed {
                game.sel_difficulty = btn.0;
                game.dirty = true;
            }
        }
        for i in q_start.iter() {
            if *i == Interaction::Pressed {
                let d = game.sel_difficulty;
                game.start_match(d);
            }
        }
    } else if game.page == CkPage::Result {
        for i in q_again.iter() {
            if *i == Interaction::Pressed {
                let d = game.sel_difficulty;
                game.start_match(d);
            }
        }
        for i in q_home.iter() {
            if *i == Interaction::Pressed {
                *game = CakeduelGame::new();
            }
        }
    }
}

fn st_ck_game(
    mouse: Res<ButtonInput<MouseButton>>,
    focused: Res<crate::topbar::FocusedTitle>,
    mut game: ResMut<CakeduelGame>,
    q_cards: Query<(&CkHandCard, &Interaction)>,
    q_claims: Query<(&CkClaimBtn, &Interaction)>,
    q_defense: Query<(&CkDefenseAct, &Interaction)>,
    q_pass: Query<&Interaction, With<CkPassBtn>>,
) {
    if !mouse.just_pressed(MouseButton::Left) || !ck_focused(&focused) {
        return;
    }
    if game.page != CkPage::Game || game.phase != Phase::PlayerAct {
        return;
    }

    // 手牌点选（攻击选牌 或 回击选牌）
    for (card, i) in q_cards.iter() {
        if *i != Interaction::Pressed {
            continue;
        }
        let idx = card.0;
        if game.defense_selecting {
            if let Some(pos) = game.sel.iter().position(|s| *s == idx) {
                game.sel.remove(pos);
            } else if game.sel.len() < 4 {
                game.sel.push(idx);
            }
        } else if game.attacker == Side::Human {
            if let Some(pos) = game.sel.iter().position(|s| *s == idx) {
                game.sel.remove(pos);
            } else if game.sel.len() < 4 {
                game.sel.push(idx);
            }
        }
        game.dirty = true;
    }

    let acting_as_attacker = game.attacker == Side::Human && game.attack_play.is_none();
    let acting_as_defender = game.attacker == Side::Ai && game.attack_play.is_some();

    // 声明按钮
    for (claim, i) in q_claims.iter() {
        if *i != Interaction::Pressed {
            continue;
        }
        let t = claim.0;
        if acting_as_attacker && !game.sel.is_empty() {
            let cards: Vec<CardType> = game.sel.iter().map(|i| game.hand_human[*i]).collect();
            game.attack_play = Some(CkPlay { cards, claim: t });
            game.sel.clear();
            game.defense_selecting = false;
            game.last_pass = false;
            game.dirty = true;
            game.message = format!("你打出 {} 张牌，声明「{}」。", {
                game.attack_play.as_ref().unwrap().cards.len()
            }, t.name());
            game.ai_role = AiRole::Defend;
            game.phase = Phase::AiAct;
            game.timer = AI_DELAY;
        } else if game.defense_selecting && !game.sel.is_empty() && t.claimable_defense() {
            let cards: Vec<CardType> = game.sel.iter().map(|i| game.hand_human[*i]).collect();
            game.defense_play = Some(CkPlay { cards, claim: t });
            game.sel.clear();
            game.defense_selecting = false;
            game.dirty = true;
            game.message = format!("你回击：声明「{}」。等待 Nori 判定…", t.name());
            game.ai_role = AiRole::ChallengeBlock;
            game.phase = Phase::AiAct;
            game.timer = AI_DELAY;
        }
    }

    // 防守动作按钮
    for (act, i) in q_defense.iter() {
        if *i != Interaction::Pressed {
            continue;
        }
        match act.0 {
            0 => {
                // 质疑：AI 阻挡了人类攻击 → 质疑防守牌；AI 攻击 → 质疑攻击牌
                if game.attacker == Side::Human && game.defense_play.is_some() {
                    let block = game.defense_play.clone().unwrap();
                    game.resolve_challenge(&block, Side::Ai, Side::Human);
                } else if acting_as_defender {
                    let attack = game.attack_play.clone().unwrap();
                    game.resolve_challenge(&attack, Side::Ai, Side::Human);
                }
            }
            1 => {
                // 接受
                if acting_as_defender {
                    game.message = "你接受进攻。".to_string();
                    game.settle_accept();
                }
            }
            2 => {
                // 开始回击选牌
                if acting_as_defender && game.defense_play.is_none() {
                    game.defense_selecting = true;
                    game.sel.clear();
                    game.dirty = true;
                }
            }
            3 => {
                // 放行 AI 的防守
                if game.attacker == Side::Human && game.defense_play.is_some() {
                    game.message = "你不质疑 Nori 的防守。".to_string();
                    game.settle_block();
                }
            }
            4 => {
                // 取消回击选牌
                if game.defense_selecting {
                    game.defense_selecting = false;
                    game.sel.clear();
                    game.dirty = true;
                }
            }
            _ => {}
        }
    }

    // 过牌（仅攻方）
    for i in q_pass.iter() {
        if *i == Interaction::Pressed && game.attacker == Side::Human && game.attack_play.is_none()
        {
            game.pass_turn(Side::Human);
        }
    }
}

fn st_ck_ai(time: Res<Time>, mut game: ResMut<CakeduelGame>) {
    if game.page != CkPage::Game || game.phase != Phase::AiAct {
        return;
    }
    if game.timer > 0.0 {
        game.timer -= time.delta_secs();
        return;
    }
    ai_take_turn(&mut game);
}

fn st_ck_resolve(time: Res<Time>, mut game: ResMut<CakeduelGame>) {
    if game.page != CkPage::Game {
        return;
    }
    match game.phase {
        Phase::Resolve => {
            game.timer -= time.delta_secs();
            if game.timer <= 0.0 {
                match game.after_resolve.take() {
                    Some(AfterResolve::Swap) => game.swap_and_refresh(),
                    Some(AfterResolve::BattleEnd(winner, reason)) => game.begin_battle_end(winner, reason),
                    None => game.swap_and_refresh(),
                }
            }
        }
        Phase::BattleEnd => {
            game.timer -= time.delta_secs();
            if game.timer <= 0.0 {
                game.after_battle_end();
            }
        }
        _ => {}
    }
}

// ==================== 渲染同步 ====================

fn st_ck_load(server: Res<AssetServer>, mut assets: ResMut<CkAssets>) {
    if assets.loaded.is_none() {
        assets.loaded = Some(load_ck_assets(&server));
    }
}

fn st_ck_page_sync(game: Res<CakeduelGame>, mut pages: Query<(&CkPageRoot, &mut Node)>) {
    for (root, mut node) in pages.iter_mut() {
        let target = if root.0 == game.page { Display::Flex } else { Display::None };
        if node.display != target {
            node.display = target;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn st_ck_ui(
    mut game: ResMut<CakeduelGame>,
    assets: Res<CkAssets>,
    fonts: Res<N3riFonts>,
    mut texts: Query<(&CkText, &mut Text)>,
    hand_row: Query<(Entity, &CkHandRow)>,
    ai_row: Query<(Entity, &CkAiHandRow)>,
    pile_row: Query<(Entity, &CkPileRow)>,
    deck_box: Query<(Entity, &CkDeckBox)>,
    cake_rail: Query<(Entity, &CkCakeRail)>,
    mut diff_btns: Query<(&CkDiff, &mut BackgroundColor)>,
    action_panel: Query<(Entity, &CkActionPanel)>,
    bg_query: Query<(Entity, &CkBg)>,
    mut commands: Commands,
) {
    let Some(tex) = assets.loaded.as_ref() else { return };

    // 难度按钮高亮
    for (d, mut bg) in diff_btns.iter_mut() {
        let target = if d.0 == game.sel_difficulty { CK_SEL } else { CK_PANEL };
        if bg.0 != target {
            bg.0 = target;
        }
    }

    // 文本
    let phase_label = match game.phase {
        Phase::PlayerAct => {
            if game.attacker == Side::Human {
                if game.defense_selecting {
                    "你的回击：选牌并声明克制牌".to_string()
                } else {
                    "你的进攻回合".to_string()
                }
            } else if game.defense_play.is_some() {
                "Nori 回击了——质疑还是放行？".to_string()
            } else {
                "你的防守回合：回击 / 质疑 / 接受".to_string()
            }
        }
        Phase::AiAct => "Nori 思考中…".to_string(),
        Phase::Resolve | Phase::BattleEnd => "结算".to_string(),
        _ => String::new(),
    };
    for (slot, mut text) in texts.iter_mut() {
        let target = match slot.0 {
            CkSlot::Phase => phase_label.clone(),
            CkSlot::Score => format!("你 {} : {} Nori", game.wins_human, game.wins_ai),
            CkSlot::Message => game.message.clone(),
            CkSlot::ResultTitle => game.result_title.clone(),
            CkSlot::ResultSub => game.result_sub.clone(),
            CkSlot::DeckCount => game.deck.len().to_string(),
        };
        if **text != target {
            **text = target;
        }
    }

    if !game.dirty {
        return;
    }

    // 背景牌垫（三页通用）
    for (e, meta) in bg_query.iter() {
        commands.entity(e).despawn_children();
        commands.entity(e).with_children(|layer| {
            layer.spawn((ImageNode { image: tex.playmat.clone(), ..default() },));
            if meta.veiled {
                layer.spawn((Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    right: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    ..default()
                }, BackgroundColor(Color::srgba(1.0, 0.97, 0.94, 0.4))));
            }
        });
    }

    // 玩家手牌重建
    if let Ok((e, _)) = hand_row.single() {
        commands.entity(e).despawn_children();
        let selecting = game.defense_selecting
            || (game.attacker == Side::Human && game.phase == Phase::PlayerAct && game.attack_play.is_none());
        commands.entity(e).with_children(|row| {
            for (i, card) in game.hand_human.iter().enumerate() {
                let selected = game.sel.contains(&i);
                let face = tex.faces.get(card).expect("face handle").clone();
                row.spawn((
                    Button,
                    CkHandCard(i),
                    Node {
                        width: Val::Px(103.0),
                        height: Val::Px(141.0),
                        border: UiRect::all(Val::Px(if selected { 3.0 } else { 0.0 })),
                        border_radius: BorderRadius::all(Val::Px(11.0)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    ImageNode { image: face, ..default() },
                    BorderColor::all(CK_SEL),
                ));
            }
            if selecting && !game.defense_selecting {
                // 声明按钮挂在操作面板，不在这里
            }
        });
    }

    // AI 手牌
    if let Ok((e, _)) = ai_row.single() {
        commands.entity(e).despawn_children();
        commands.entity(e).with_children(|row| {
            for _ in 0..game.hand_ai.len() {
                row.spawn((
                    Node {
                        width: Val::Px(56.0),
                        height: Val::Px(76.0),
                        border_radius: BorderRadius::all(Val::Px(6.0)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    ImageNode { image: tex.back.clone(), ..default() },
                ));
            }
        });
    }

    // 中央牌堆
    if let Ok((e, _)) = pile_row.single() {
        commands.entity(e).despawn_children();
        commands.entity(e).with_children(|row| {
            spawn_pile(row, tex, "攻击牌堆", game.attack_play.as_ref(), game.reveal, &fonts);
            spawn_pile(row, tex, "防守牌堆", game.defense_play.as_ref(), game.reveal, &fonts);
        });
    }

    // 牌堆计数
    if let Ok((e, _)) = deck_box.single() {
        commands.entity(e).despawn_children();
        commands.entity(e).with_children(|box_| {
            box_.spawn((
                Node {
                    width: Val::Px(56.0),
                    height: Val::Px(76.0),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                ImageNode { image: tex.back.clone(), ..default() },
            ));
            box_.spawn((
                Text::new(""),
                TextFont { font: ck_font(&fonts), font_size: FontSize::Px(14.0), ..default() },
                TextColor(CK_BROWN),
                CkText(CkSlot::DeckCount),
            ));
        });
    }

    // 蛋糕栏
    if let Ok((e, _)) = cake_rail.single() {
        commands.entity(e).despawn_children();
        commands.entity(e).with_children(|rail| {
            rail.spawn((Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                align_items: AlignItems::Center,
                ..default()
            },))
            .with_children(|col| {
                for _ in 0..game.cakes_ai {
                    col.spawn((Node { width: Val::Px(30.0), height: Val::Px(30.0), ..default() },))
                        .with_children(|c| {
                            c.spawn((ImageNode { image: tex.cake.clone(), ..default() },));
                        });
                }
            });
            rail.spawn((Node { flex_grow: 1.0, ..default() },));
            rail.spawn((Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                align_items: AlignItems::Center,
                ..default()
            },))
            .with_children(|col| {
                for _ in 0..game.cakes_human {
                    col.spawn((Node { width: Val::Px(30.0), height: Val::Px(30.0), ..default() },))
                        .with_children(|c| {
                            c.spawn((ImageNode { image: tex.cake.clone(), ..default() },));
                        });
                }
            });
        });
    }

    // 操作面板重建（跟 dirty 走）
    if let Ok((panel, _)) = action_panel.single() {
        rebuild_action_panel(&game, &fonts, panel, &mut commands);
    }

    game.dirty = false;
}

#[allow(clippy::too_many_arguments)]
fn spawn_pile(
    row: &mut ChildSpawnerCommands,
    tex: &CkTex,
    label: &str,
    play: Option<&CkPlay>,
    reveal: bool,
    fonts: &N3riFonts,
) {
    row.spawn((Node {
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        row_gap: Val::Px(4.0),
        ..default()
    },))
    .with_children(|col| {
        ck_text(col, label, 12.0, CK_DIM, fonts);
        col.spawn((Node {
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(-18.0),
            min_height: Val::Px(76.0),
            align_items: AlignItems::Center,
            ..default()
        },))
        .with_children(|cards| {
            if let Some(p) = play {
                for card in &p.cards {
                    let image = if reveal {
                        tex.faces.get(card).expect("face").clone()
                    } else {
                        tex.back.clone()
                    };
                    cards.spawn((
                        Node {
                            width: Val::Px(56.0),
                            height: Val::Px(76.0),
                            border_radius: BorderRadius::all(Val::Px(6.0)),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        ImageNode { image, ..default() },
                    ));
                }
            }
        });
        if let Some(p) = play {
            ck_text(
                col,
                &format!("声明「{}」×{}", p.claim.name(), p.cards.len()),
                12.0,
                CK_AMBER,
                fonts,
            );
        } else {
            ck_text(col, "—", 12.0, CK_DIM, fonts);
        }
    });
}

fn ck_button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    marker: impl Component,
    bg: Color,
    fonts: &N3riFonts,
) {
    parent
        .spawn((
            Button,
            marker,
            Node {
                height: Val::Px(44.0),
                padding: UiRect::horizontal(Val::Px(16.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(12.0)),
                ..default()
            },
            BackgroundColor(bg),
        ))
        .with_children(|b| ck_text(b, label, 14.0, CK_BROWN, fonts));
}

fn rebuild_action_panel(game: &CakeduelGame, fonts: &N3riFonts, panel: Entity, commands: &mut Commands) {
    commands.entity(panel).despawn_children();
    commands.entity(panel).with_children(|p| {
        let human_attacking =
            game.attacker == Side::Human && game.phase == Phase::PlayerAct && game.attack_play.is_none();
        let human_defending =
            game.attacker == Side::Ai && game.phase == Phase::PlayerAct && game.attack_play.is_some();
        let ai_blocked =
            game.attacker == Side::Human && game.phase == Phase::PlayerAct && game.defense_play.is_some();

        if ai_blocked {
            ck_button(p, "质疑 Nori 的防守", CkDefenseAct(0), CK_SEL, fonts);
            ck_button(p, "放行", CkDefenseAct(3), CK_PANEL, fonts);
        } else if human_defending && game.defense_selecting {
            for t in [CardType::Defender, CardType::Scientist] {
                ck_button(p, &format!("声明「{}」", t.name()), CkClaimBtn(t), CK_SEL, fonts);
            }
            ck_button(p, "取消回击", CkDefenseAct(4), CK_PANEL, fonts);
        } else if human_defending {
            ck_button(p, "质疑", CkDefenseAct(0), CK_SEL, fonts);
            ck_button(p, "回击", CkDefenseAct(2), CK_GREEN, fonts);
            ck_button(p, "接受", CkDefenseAct(1), CK_PANEL, fonts);
        } else if human_attacking {
            if game.sel.is_empty() {
                ck_button(p, "过牌（放弃本次进攻）", CkPassBtn, CK_PANEL, fonts);
            } else {
                ck_text(p, "声明：", 13.0, CK_DIM, fonts);
                for t in [CardType::Soldier, CardType::Archer, CardType::Wizard] {
                    ck_button(p, t.name(), CkClaimBtn(t), CK_SEL, fonts);
                }
            }
        }
    });
}

// ==================== 插件 ====================

pub struct CakeduelPlugin;

impl Plugin for CakeduelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CakeduelGame>()
            .init_resource::<CkAssets>()
            .add_systems(
                Update,
                (
                    st_ck_load,
                    st_ck_nav,
                    st_ck_game,
                    st_ck_ai,
                    st_ck_resolve,
                    st_ck_page_sync,
                    st_ck_ui,
                ),
            );
    }
}
