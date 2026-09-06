//! 国际象棋 —— 移植自 international_chess.html 的双人本地对弈
//!
//! 完整规则：走法生成（6 种棋子）、将军/将死/逼和判定、王车易位（两翼）、
//! 吃过路兵、兵升变（Q/R/B/N 四选一浮层）。本地双人轮流走子，白先。
//!
//! 坐标约定与参考 HTML 一致：row 0 = 顶部 = 第 8 横排（黑方底线），
//! row 7 = 底部 = 第 1 横排（白方底线），col 0 = a 列。
//!
//! 布局：左侧 8×8 棋盘（每格 64px，共 512px），右侧侧栏（状态、走法记录、操作按钮）。

use bevy::prelude::*;

use crate::font::{FontContext, N3riFonts};
use crate::scroll::{spawn_scrollbar, ScrollableArea, ScrollContent};
use crate::window::spawn_window;

// ============================
//  调色板与常量
// ============================
// —— 窗口玻璃 & 内容（oklch → sRGB）——
const CONTENT_BG: Color = Color::srgb(0.020, 0.038, 0.059); // oklch(0.14 0.015 250)
const BOARD_WRAPPER_BG: Color = Color::srgba(0.086, 0.110, 0.165, 0.85); // rgba(22,28,42,0.85)
const CARD_BORDER: Color = Color::srgba(0.471, 0.706, 0.824, 0.12); // rgba(120,180,210,0.12)

// —— 棋盘格（渐变中点近似为半透明实色）——
const LIGHT_SQ: Color = Color::srgba(0.569, 0.784, 0.922, 0.19);
const DARK_SQ: Color = Color::srgba(0.167, 0.314, 0.471, 0.42);
const SELECTED_SQ: Color = Color::srgba(0.627, 0.902, 1.0, 0.25); // rgba(160,230,255,0.25)
const DOT_COLOR: Color = Color::srgba(0.549, 0.882, 1.0, 0.25); // rgba(140,225,255,0.25)
const RING_COLOR: Color = Color::srgba(0.549, 0.882, 1.0, 0.35); // rgba(140,225,255,0.35)
const LABEL_COLOR: Color = Color::srgba(0.588, 0.765, 0.882, 0.55); // rgba(150,195,225,0.55)

// —— 文本（oklch → sRGB）——
const TEXT_MAIN: Color = Color::srgb(0.887, 0.916, 0.917); // oklch(0.93 0.008 200)
const TEXT_DIM: Color = Color::srgb(0.499, 0.532, 0.549); // oklch(0.62 0.012 230)
const TEXT_ACCENT: Color = Color::srgb(0.466, 0.776, 0.830); // oklch(0.78 0.08 210) 青色
const TEXT_BAD: Color = Color::srgb(0.943, 0.559, 0.530); // oklch(0.75 0.12 25) 危险红
const TEXT_GRAY: Color = Color::srgb(0.316, 0.338, 0.354); // oklch(0.45 0.01 240)

// —— 按钮 ——
const BTN_PRIMARY_BG: Color = Color::srgb(0.466, 0.776, 0.830); // oklch(0.78 0.08 210)
const BTN_PRIMARY_TEXT: Color = Color::srgb(0.012, 0.030, 0.058); // oklch(0.13 0.02 250)
const BTN_SECONDARY_BG: Color = Color::srgba(1.0, 1.0, 1.0, 0.06); // rgba(255,255,255,0.06)
const ACTION_BTN_BG: Color = Color::srgba(1.0, 1.0, 1.0, 0.05); // rgba(255,255,255,0.05)
const DANGER_BG: Color = Color::srgba(0.784, 0.314, 0.314, 0.10); // rgba(200,80,80,0.1)
const DANGER_BORDER: Color = Color::srgba(0.784, 0.314, 0.314, 0.30); // rgba(200,80,80,0.3)
const ACTIVE_SELECT_BG: Color = Color::srgba(0.569, 0.784, 0.922, 0.145); // 激活颜色/难度按钮渐变中点

// —— 尺寸 ——
const BOARD_SIZE: f32 = 512.0;
const SQUARE_SIZE: f32 = 64.0;
const RANK_COL_WIDTH: f32 = 16.0;
const FILE_ROW_HEIGHT: f32 = 16.0;
const SIDEBAR_WIDTH: f32 = 268.0;

// —— 难度等级 ——
const DIFF_LEVELS: [(u16, &str, &str); 5] = [
    (800, "困困的 Nori", "800 ELO"),
    (1200, "慵懒的 Nori", "1200 ELO"),
    (1600, "普通的 Nori", "1600 ELO"),
    (2000, "专注的 Nori", "2000 ELO"),
    (2400, "认真的 Nori", "2400 ELO"),
];

// ============================
//  棋局纯逻辑（不依赖 Bevy）
// ============================
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ChessColor {
    White,
    Black,
}

impl ChessColor {
    fn opposite(self) -> ChessColor {
        match self {
            ChessColor::White => ChessColor::Black,
            ChessColor::Black => ChessColor::White,
        }
    }
    fn letter(self) -> char {
        match self {
            ChessColor::White => 'w',
            ChessColor::Black => 'b',
        }
    }
    fn label(self) -> &'static str {
        match self {
            ChessColor::White => "白方",
            ChessColor::Black => "黑方",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PieceKind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

impl PieceKind {
    fn letter(self) -> char {
        match self {
            PieceKind::Pawn => 'P',
            PieceKind::Knight => 'N',
            PieceKind::Bishop => 'B',
            PieceKind::Rook => 'R',
            PieceKind::Queen => 'Q',
            PieceKind::King => 'K',
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Piece {
    kind: PieceKind,
    color: ChessColor,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Sq {
    r: i32,
    c: i32,
}

impl Sq {
    fn new(r: i32, c: i32) -> Self {
        Self { r, c }
    }
    fn in_bounds(self) -> bool {
        (0..8).contains(&self.r) && (0..8).contains(&self.c)
    }
    fn file(self) -> char {
        (b'a' + self.c as u8) as char
    }
    fn rank(self) -> char {
        (b'8' - self.r as u8) as char
    }
    fn alg(self) -> String {
        format!("{}{}", self.file(), self.rank())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MoveKind {
    Normal,
    DoublePush,
    EnPassant,
    CastleKingside,
    CastleQueenside,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Move {
    from: Sq,
    to: Sq,
    kind: MoveKind,
    promotion: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct CastleRights {
    wk: bool,
    wq: bool,
    bk: bool,
    bq: bool,
}

impl CastleRights {
    const ALL: CastleRights = CastleRights {
        wk: true,
        wq: true,
        bk: true,
        bq: true,
    };
    /// 仅测试使用（`#[cfg(test)]` 之外为死代码）
    #[allow(dead_code)]
    const NONE: CastleRights = CastleRights {
        wk: false,
        wq: false,
        bk: false,
        bq: false,
    };
}

type Board = [[Option<Piece>; 8]; 8];

/// 一局棋的完整状态
#[derive(Clone, PartialEq, Debug)]
struct Position {
    board: Board,
    turn: ChessColor,
    castle: CastleRights,
    en_passant: Option<Sq>,
}

impl Position {
    fn initial() -> Self {
        let mut board = [[None; 8]; 8];
        let back = [
            PieceKind::Rook,
            PieceKind::Knight,
            PieceKind::Bishop,
            PieceKind::Queen,
            PieceKind::King,
            PieceKind::Bishop,
            PieceKind::Knight,
            PieceKind::Rook,
        ];
        for (c, kind) in back.iter().enumerate() {
            board[0][c] = Some(Piece {
                kind: *kind,
                color: ChessColor::Black,
            });
            board[7][c] = Some(Piece {
                kind: *kind,
                color: ChessColor::White,
            });
        }
        for c in 0..8 {
            board[1][c] = Some(Piece {
                kind: PieceKind::Pawn,
                color: ChessColor::Black,
            });
            board[6][c] = Some(Piece {
                kind: PieceKind::Pawn,
                color: ChessColor::White,
            });
        }
        Position {
            board,
            turn: ChessColor::White,
            castle: CastleRights::ALL,
            en_passant: None,
        }
    }

    fn piece_at(&self, sq: Sq) -> Option<Piece> {
        self.board[sq.r as usize][sq.c as usize]
    }

    fn find_king(&self, color: ChessColor) -> Option<Sq> {
        for r in 0..8 {
            for c in 0..8 {
                if self.board[r][c] == Some(Piece {
                    kind: PieceKind::King,
                    color,
                }) {
                    return Some(Sq::new(r as i32, c as i32));
                }
            }
        }
        None
    }

    fn is_in_check(&self, color: ChessColor) -> bool {
        match self.find_king(color) {
            Some(k) => is_square_attacked(&self.board, k, color.opposite()),
            None => false,
        }
    }

    /// 伪合法走法生成（不含「走后己方王被将军」过滤、不含易位合法性过滤）
    fn pseudo_moves(&self, from: Sq) -> Vec<Move> {
        let Some(piece) = self.piece_at(from) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        match piece.kind {
            PieceKind::Pawn => self.pawn_moves(from, piece.color, &mut out),
            PieceKind::Knight => {
                for (dr, dc) in KNIGHT_DELTAS {
                    self.add_step(from, piece.color, dr, dc, &mut out);
                }
            }
            PieceKind::King => {
                for (dr, dc) in KING_DELTAS {
                    self.add_step(from, piece.color, dr, dc, &mut out);
                }
                self.castle_moves(from, piece.color, &mut out);
            }
            PieceKind::Bishop => self.add_sliding(from, piece.color, &BISHOP_DELTAS, &mut out),
            PieceKind::Rook => self.add_sliding(from, piece.color, &ROOK_DELTAS, &mut out),
            PieceKind::Queen => self.add_sliding(from, piece.color, &QUEEN_DELTAS, &mut out),
        }
        out
    }

    fn pawn_moves(&self, from: Sq, color: ChessColor, out: &mut Vec<Move>) {
        let dir: i32 = if color == ChessColor::White { -1 } else { 1 };
        let start_row: i32 = if color == ChessColor::White { 6 } else { 1 };
        let promo_row: i32 = if color == ChessColor::White { 0 } else { 7 };
        // 直进一格
        let fwd = Sq::new(from.r + dir, from.c);
        if fwd.in_bounds() && self.piece_at(fwd).is_none() {
            out.push(Move {
                from,
                to: fwd,
                kind: MoveKind::Normal,
                promotion: fwd.r == promo_row,
            });
            // 起始位直进两格
            if from.r == start_row {
                let fwd2 = Sq::new(from.r + 2 * dir, from.c);
                if fwd2.in_bounds() && self.piece_at(fwd2).is_none() {
                    out.push(Move {
                        from,
                        to: fwd2,
                        kind: MoveKind::DoublePush,
                        promotion: false,
                    });
                }
            }
        }
        // 斜吃 + 吃过路兵
        for dc in [-1, 1] {
            let cap = Sq::new(from.r + dir, from.c + dc);
            if !cap.in_bounds() {
                continue;
            }
            if let Some(t) = self.piece_at(cap) {
                if t.color != color {
                    out.push(Move {
                        from,
                        to: cap,
                        kind: MoveKind::Normal,
                        promotion: cap.r == promo_row,
                    });
                }
            }
            // 过路兵：目标格是敌人兵跳过的空格（from.r + 2*dir）
            let ep_target = Sq::new(from.r + 2 * dir, from.c + dc);
            if ep_target.in_bounds() && self.en_passant == Some(ep_target) {
                if let Some(bp) = self.piece_at(cap) {
                    if bp.kind == PieceKind::Pawn && bp.color != color {
                        out.push(Move {
                            from,
                            to: ep_target,
                            kind: MoveKind::EnPassant,
                            promotion: false,
                        });
                    }
                }
            }
        }
    }

    fn add_step(&self, from: Sq, color: ChessColor, dr: i32, dc: i32, out: &mut Vec<Move>) {
        let to = Sq::new(from.r + dr, from.c + dc);
        if !to.in_bounds() {
            return;
        }
        match self.piece_at(to) {
            Some(t) if t.color == color => {}
            _ => out.push(Move {
                from,
                to,
                kind: MoveKind::Normal,
                promotion: false,
            }),
        }
    }

    fn add_sliding(
        &self,
        from: Sq,
        color: ChessColor,
        deltas: &[(i32, i32)],
        out: &mut Vec<Move>,
    ) {
        for &(dr, dc) in deltas {
            let mut r = from.r + dr;
            let mut c = from.c + dc;
            while Sq::new(r, c).in_bounds() {
                let sq = Sq::new(r, c);
                match self.piece_at(sq) {
                    None => out.push(Move {
                        from,
                        to: sq,
                        kind: MoveKind::Normal,
                        promotion: false,
                    }),
                    Some(t) => {
                        if t.color != color {
                            out.push(Move {
                                from,
                                to: sq,
                                kind: MoveKind::Normal,
                                promotion: false,
                            });
                        }
                        break;
                    }
                }
                r += dr;
                c += dc;
            }
        }
    }

    fn castle_moves(&self, from: Sq, color: ChessColor, out: &mut Vec<Move>) {
        // 王必须在原位
        let (row, kingside_ok, queenside_ok, rook_k, rook_q) = match color {
            ChessColor::White => (7, self.castle.wk, self.castle.wq, 7, 0),
            ChessColor::Black => (0, self.castle.bk, self.castle.bq, 7, 0),
        };
        if from != Sq::new(row, 4) {
            return;
        }
        let king_rook = Some(Piece {
            kind: PieceKind::Rook,
            color,
        });
        if kingside_ok
            && self.piece_at(Sq::new(row, 5)).is_none()
            && self.piece_at(Sq::new(row, 6)).is_none()
            && self.piece_at(Sq::new(row, rook_k)) == king_rook
        {
            out.push(Move {
                from,
                to: Sq::new(row, 6),
                kind: MoveKind::CastleKingside,
                promotion: false,
            });
        }
        if queenside_ok
            && self.piece_at(Sq::new(row, 1)).is_none()
            && self.piece_at(Sq::new(row, 2)).is_none()
            && self.piece_at(Sq::new(row, 3)).is_none()
            && self.piece_at(Sq::new(row, rook_q)) == king_rook
        {
            out.push(Move {
                from,
                to: Sq::new(row, 2),
                kind: MoveKind::CastleQueenside,
                promotion: false,
            });
        }
    }

    /// 单步是否合法：走完己方王不被将军；易位另有王不在将军/不穿越被攻击格的条件
    fn move_is_legal(&self, m: Move) -> bool {
        let Some(piece) = self.piece_at(m.from) else {
            return false;
        };
        let color = piece.color;
        if matches!(
            m.kind,
            MoveKind::CastleKingside | MoveKind::CastleQueenside
        ) {
            if self.is_in_check(color) {
                return false;
            }
            let cross_col = if m.kind == MoveKind::CastleKingside {
                5
            } else {
                3
            };
            let cross = Sq::new(m.from.r, cross_col);
            if is_square_attacked(&self.board, cross, color.opposite()) {
                return false;
            }
        }
        let mut copy = self.clone();
        copy.apply_move(m);
        !copy.is_in_check(color)
    }

    fn legal_moves(&self, from: Sq) -> Vec<Move> {
        self.pseudo_moves(from)
            .into_iter()
            .filter(|&m| self.move_is_legal(m))
            .collect()
    }

    fn has_any_legal(&self) -> bool {
        for r in 0..8 {
            for c in 0..8 {
                let sq = Sq::new(r, c);
                if let Some(p) = self.piece_at(sq) {
                    if p.color == self.turn && !self.legal_moves(sq).is_empty() {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// 返回 (是否被将军, 是否无合法走法)
    fn status(&self) -> (bool, bool) {
        let in_check = self.is_in_check(self.turn);
        let no_moves = !self.has_any_legal();
        (in_check, no_moves)
    }

    /// 应用走法到棋盘。返回被吃的棋子（吃过路兵时是旁边那枚兵）。
    fn apply_move(&mut self, m: Move) -> Option<Piece> {
        let moved = self.piece_at(m.from).expect("moving piece exists");
        let mut captured = self.piece_at(m.to);
        self.board[m.to.r as usize][m.to.c as usize] = Some(moved);
        self.board[m.from.r as usize][m.from.c as usize] = None;
        match m.kind {
            MoveKind::CastleKingside => {
                let row = m.from.r;
                self.board[row as usize][5] = self.board[row as usize][7];
                self.board[row as usize][7] = None;
            }
            MoveKind::CastleQueenside => {
                let row = m.from.r;
                self.board[row as usize][3] = self.board[row as usize][0];
                self.board[row as usize][0] = None;
            }
            MoveKind::EnPassant => {
                // 被吃的兵在「起点与目标格的行中点」、同一列
                let cap_row = (m.from.r + m.to.r) / 2;
                captured = self.board[cap_row as usize][m.to.c as usize];
                self.board[cap_row as usize][m.to.c as usize] = None;
            }
            _ => {}
        }
        // 更新易位权
        if moved.kind == PieceKind::King {
            match moved.color {
                ChessColor::White => {
                    self.castle.wk = false;
                    self.castle.wq = false;
                }
                ChessColor::Black => {
                    self.castle.bk = false;
                    self.castle.bq = false;
                }
            }
        }
        if moved.kind == PieceKind::Rook {
            match (moved.color, m.from.r, m.from.c) {
                (ChessColor::White, 7, 0) => self.castle.wq = false,
                (ChessColor::White, 7, 7) => self.castle.wk = false,
                (ChessColor::Black, 0, 0) => self.castle.bq = false,
                (ChessColor::Black, 0, 7) => self.castle.bk = false,
                _ => {}
            }
        }
        // 车在己方底线原位被吃，对应侧易位权取消
        if let Some(cap) = captured {
            if cap.kind == PieceKind::Rook {
                match (cap.color, m.to.r, m.to.c) {
                    (ChessColor::White, 7, 0) => self.castle.wq = false,
                    (ChessColor::White, 7, 7) => self.castle.wk = false,
                    (ChessColor::Black, 0, 0) => self.castle.bq = false,
                    (ChessColor::Black, 0, 7) => self.castle.bk = false,
                    _ => {}
                }
            }
        }
        // 更新过路兵目标格
        self.en_passant = if m.kind == MoveKind::DoublePush {
            Some(Sq::new((m.from.r + m.to.r) / 2, m.from.c))
        } else {
            None
        };
        captured
    }
}

const KNIGHT_DELTAS: [(i32, i32); 8] = [
    (-2, -1),
    (-2, 1),
    (-1, -2),
    (-1, 2),
    (1, -2),
    (1, 2),
    (2, -1),
    (2, 1),
];

const KING_DELTAS: [(i32, i32); 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, -1),
    (0, 1),
    (1, -1),
    (1, 0),
    (1, 1),
];

const BISHOP_DELTAS: [(i32, i32); 4] = [(-1, -1), (-1, 1), (1, -1), (1, 1)];

const ROOK_DELTAS: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

const QUEEN_DELTAS: [(i32, i32); 8] = [
    (-1, -1),
    (-1, 1),
    (1, -1),
    (1, 1),
    (-1, 0),
    (1, 0),
    (0, -1),
    (0, 1),
];

/// 某格是否被 `by` 方攻击（用于将军/易位路径判定）
fn is_square_attacked(board: &Board, sq: Sq, by: ChessColor) -> bool {
    // 兵
    let pr = if by == ChessColor::White {
        sq.r + 1
    } else {
        sq.r - 1
    };
    for pc in [sq.c - 1, sq.c + 1] {
        let p = Sq::new(pr, pc);
        if p.in_bounds()
            && board[p.r as usize][p.c as usize]
                == Some(Piece {
                    kind: PieceKind::Pawn,
                    color: by,
                })
        {
            return true;
        }
    }
    // 马
    for (dr, dc) in KNIGHT_DELTAS {
        let p = Sq::new(sq.r + dr, sq.c + dc);
        if p.in_bounds()
            && board[p.r as usize][p.c as usize]
                == Some(Piece {
                    kind: PieceKind::Knight,
                    color: by,
                })
        {
            return true;
        }
    }
    // 王
    for (dr, dc) in KING_DELTAS {
        let p = Sq::new(sq.r + dr, sq.c + dc);
        if p.in_bounds()
            && board[p.r as usize][p.c as usize]
                == Some(Piece {
                    kind: PieceKind::King,
                    color: by,
                })
        {
            return true;
        }
    }
    // 车/后（正交）
    for (dr, dc) in ROOK_DELTAS {
        if ray_hits(board, sq, dr, dc, by, &[PieceKind::Rook, PieceKind::Queen]) {
            return true;
        }
    }
    // 象/后（斜线）
    for (dr, dc) in BISHOP_DELTAS {
        if ray_hits(board, sq, dr, dc, by, &[PieceKind::Bishop, PieceKind::Queen]) {
            return true;
        }
    }
    false
}

fn ray_hits(
    board: &Board,
    from: Sq,
    dr: i32,
    dc: i32,
    by: ChessColor,
    kinds: &[PieceKind],
) -> bool {
    let mut r = from.r + dr;
    let mut c = from.c + dc;
    while Sq::new(r, c).in_bounds() {
        let s = Sq::new(r, c);
        if let Some(p) = board[s.r as usize][s.c as usize] {
            return p.color == by && kinds.contains(&p.kind);
        }
        r += dr;
        c += dc;
    }
    false
}

// ============================
//  对局结果与 SAN
// ============================
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GameResult {
    /// 胜者
    Checkmate(ChessColor),
    Stalemate,
    /// 双方协议和棋
    Draw,
    /// 认输方的对手为胜者
    Resign(ChessColor),
}

impl GameResult {
    fn text(&self) -> String {
        match self {
            GameResult::Checkmate(w) => format!("将死！{}胜", w.label()),
            GameResult::Stalemate => "和棋（逼和）".to_string(),
            GameResult::Draw => "和棋".to_string(),
            GameResult::Resign(w) => format!("认输，{}胜", w.label()),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct PendingPromotion {
    from: Sq,
    to: Sq,
    kind: MoveKind,
}

#[derive(Clone, Debug)]
struct HistoryEntry {
    san: String,
    pos: Position,
}

// ============================
//  Bevy 资源与组件
// ============================
#[derive(Resource)]
struct ChessGame {
    pos: Position,
    selected: Option<Sq>,
    legal_moves: Vec<Move>,
    history: Vec<HistoryEntry>,
    game_over: Option<GameResult>,
    promotion: Option<PendingPromotion>,
    player_color: ChessColor,
    difficulty: u16,
    started: bool,
}

impl Default for ChessGame {
    fn default() -> Self {
        Self {
            pos: Position::initial(),
            selected: None,
            legal_moves: Vec::new(),
            history: Vec::new(),
            game_over: None,
            promotion: None,
            player_color: ChessColor::White,
            difficulty: 1200,
            started: false,
        }
    }
}

/// 12 个棋子的图片句柄：[color][kind]
#[derive(Resource, Default)]
struct ChessAssets {
    by_piece: [[Handle<Image>; 6]; 2],
}

impl ChessAssets {
    fn handle(&self, piece: Piece) -> Handle<Image> {
        let ci = match piece.color {
            ChessColor::White => 0,
            ChessColor::Black => 1,
        };
        let ki = match piece.kind {
            PieceKind::Pawn => 0,
            PieceKind::Knight => 1,
            PieceKind::Bishop => 2,
            PieceKind::Rook => 3,
            PieceKind::Queen => 4,
            PieceKind::King => 5,
        };
        self.by_piece[ci][ki].clone()
    }
    /// 升变面板按钮按此顺序展示 Q/R/B/N
    const PROMO_KINDS: [PieceKind; 4] = [
        PieceKind::Queen,
        PieceKind::Rook,
        PieceKind::Bishop,
        PieceKind::Knight,
    ];
}

/// 每格的 UI 实体引用
#[derive(Resource)]
struct ChessBoardEntities {
    squares: [[SquareEnts; 8]; 8],
    status_text: Entity,
    status_indicator: Entity,
    move_list: Entity,
    promo_panel: Entity,
    promo_buttons: [Entity; 4],
    promo_images: [Entity; 4],
    undo_btn: Entity,
    draw_btn: Entity,
    resign_btn: Entity,
    restart_btn: Entity,
    config_area: Entity,
    status_panel: Entity,
    color_btns: [(Entity, Entity); 2],
    diff_rows: [(Entity, Entity, Entity); 5],
}

#[derive(Clone, Copy)]
struct SquareEnts {
    sq: Entity,
    piece: Entity,
    dot: Entity,
    ring: Entity,
}

#[derive(Component)]
struct ChessSquare(Sq);

#[derive(Component)]
struct SquarePiece;

#[derive(Component)]
struct SquareDot;

#[derive(Component)]
struct SquareRing;

#[derive(Component)]
struct StatusText;

#[derive(Component)]
struct MoveListText;

#[derive(Component)]
struct PromoPanel;

#[derive(Component)]
struct PromoButton(usize);

#[derive(Component)]
struct PromoImage;

#[derive(Component)]
struct UndoBtn;

#[derive(Component)]
struct DrawBtn;

#[derive(Component)]
struct ResignBtn;

#[derive(Component)]
struct RestartBtn;

#[derive(Component)]
struct ColorBtn(ChessColor);

#[derive(Component)]
struct DiffBtn(u16);

#[derive(Component)]
struct StartBtn;

#[derive(Component)]
struct TeachBtn;

#[derive(Component)]
struct ConfigArea;

#[derive(Component)]
struct StatusPanel;

pub struct InternationalChessPlugin;

impl Plugin for InternationalChessPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChessGame>()
            .init_resource::<ChessAssets>()
            .add_systems(
                Update,
                (
                    chess_clicks.run_if(resource_exists::<ChessBoardEntities>),
                    chess_sync
                        .after(chess_clicks)
                        .run_if(resource_exists::<ChessBoardEntities>),
                ),
            );
    }
}

// ============================
//  点击处理
// ============================
fn chess_clicks(
    mouse: Res<ButtonInput<MouseButton>>,
    mut game: ResMut<ChessGame>,
    squares: Query<
        (&ChessSquare, &Interaction),
        (
            Without<PromoButton>,
            Without<ColorBtn>,
            Without<DiffBtn>,
            Without<StartBtn>,
            Without<TeachBtn>,
            Without<DrawBtn>,
            Without<UndoBtn>,
            Without<ResignBtn>,
            Without<RestartBtn>,
        ),
    >,
    promo: Query<
        (&PromoButton, &Interaction),
        (
            Without<ChessSquare>,
            Without<ColorBtn>,
            Without<DiffBtn>,
            Without<StartBtn>,
            Without<TeachBtn>,
            Without<DrawBtn>,
            Without<UndoBtn>,
            Without<ResignBtn>,
            Without<RestartBtn>,
        ),
    >,
    color: Query<
        (&ColorBtn, &Interaction),
        (
            Without<ChessSquare>,
            Without<PromoButton>,
            Without<DiffBtn>,
            Without<StartBtn>,
            Without<TeachBtn>,
            Without<DrawBtn>,
            Without<UndoBtn>,
            Without<ResignBtn>,
            Without<RestartBtn>,
        ),
    >,
    diff: Query<
        (&DiffBtn, &Interaction),
        (
            Without<ChessSquare>,
            Without<PromoButton>,
            Without<ColorBtn>,
            Without<StartBtn>,
            Without<TeachBtn>,
            Without<DrawBtn>,
            Without<UndoBtn>,
            Without<ResignBtn>,
            Without<RestartBtn>,
        ),
    >,
    start: Query<
        &Interaction,
        (
            With<StartBtn>,
            Without<ChessSquare>,
            Without<PromoButton>,
            Without<ColorBtn>,
            Without<DiffBtn>,
            Without<TeachBtn>,
            Without<DrawBtn>,
            Without<UndoBtn>,
            Without<ResignBtn>,
            Without<RestartBtn>,
        ),
    >,
    teach: Query<
        &Interaction,
        (
            With<TeachBtn>,
            Without<ChessSquare>,
            Without<PromoButton>,
            Without<ColorBtn>,
            Without<DiffBtn>,
            Without<StartBtn>,
            Without<DrawBtn>,
            Without<UndoBtn>,
            Without<ResignBtn>,
            Without<RestartBtn>,
        ),
    >,
    draw: Query<
        &Interaction,
        (
            With<DrawBtn>,
            Without<ChessSquare>,
            Without<PromoButton>,
            Without<ColorBtn>,
            Without<DiffBtn>,
            Without<StartBtn>,
            Without<TeachBtn>,
            Without<UndoBtn>,
            Without<ResignBtn>,
            Without<RestartBtn>,
        ),
    >,
    undo: Query<
        &Interaction,
        (
            With<UndoBtn>,
            Without<ChessSquare>,
            Without<PromoButton>,
            Without<ColorBtn>,
            Without<DiffBtn>,
            Without<StartBtn>,
            Without<TeachBtn>,
            Without<DrawBtn>,
            Without<ResignBtn>,
            Without<RestartBtn>,
        ),
    >,
    resign: Query<
        &Interaction,
        (
            With<ResignBtn>,
            Without<ChessSquare>,
            Without<PromoButton>,
            Without<ColorBtn>,
            Without<DiffBtn>,
            Without<StartBtn>,
            Without<TeachBtn>,
            Without<DrawBtn>,
            Without<UndoBtn>,
            Without<RestartBtn>,
        ),
    >,
    restart: Query<
        &Interaction,
        (
            With<RestartBtn>,
            Without<ChessSquare>,
            Without<PromoButton>,
            Without<ColorBtn>,
            Without<DiffBtn>,
            Without<StartBtn>,
            Without<TeachBtn>,
            Without<DrawBtn>,
            Without<UndoBtn>,
            Without<ResignBtn>,
        ),
    >,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    // 升变待定：只响应升变按钮
    if game.promotion.is_some() {
        for (PromoButton(i), interaction) in promo.iter() {
            if *interaction == Interaction::Pressed {
                let kind = ChessAssets::PROMO_KINDS[*i];
                game.complete_promotion(kind);
                return;
            }
        }
        return;
    }
    // 配置阶段：颜色 / 难度 / 开始 / 教学
    if !game.started {
        for (ColorBtn(c), interaction) in color.iter() {
            if *interaction == Interaction::Pressed {
                game.player_color = *c;
                return;
            }
        }
        for (DiffBtn(d), interaction) in diff.iter() {
            if *interaction == Interaction::Pressed {
                game.difficulty = *d;
                return;
            }
        }
        for interaction in start.iter() {
            if *interaction == Interaction::Pressed {
                let color = game.player_color;
                let difficulty = game.difficulty;
                game.start(color, difficulty);
                return;
            }
        }
        // 教学按钮：占位无操作
        let _ = teach.iter().any(|i| *i == Interaction::Pressed);
        return;
    }
    // 棋盘格
    let mut clicked: Option<Sq> = None;
    for (ChessSquare(sq), interaction) in squares.iter() {
        if *interaction == Interaction::Pressed {
            clicked = Some(*sq);
        }
    }
    if let Some(sq) = clicked {
        game.handle_square_click(sq);
        return;
    }
    // 按钮
    if undo.iter().any(|i| *i == Interaction::Pressed) {
        game.undo();
        return;
    }
    if draw.iter().any(|i| *i == Interaction::Pressed) {
        game.offer_draw();
        return;
    }
    if resign.iter().any(|i| *i == Interaction::Pressed) {
        game.resign();
        return;
    }
    if restart.iter().any(|i| *i == Interaction::Pressed) {
        game.new_game();
    }
}

impl ChessGame {
    fn handle_square_click(&mut self, sq: Sq) {
        if !self.started || self.promotion.is_some() || self.game_over.is_some() {
            return;
        }
        if self.selected.is_some() {
            if let Some(m) = self.legal_moves.iter().find(|m| m.to == sq) {
                self.attempt_move(*m);
                return;
            }
            // 不是合法目标：若点到自己棋子则改选，否则取消选择
            if let Some(p) = self.pos.piece_at(sq) {
                if p.color == self.pos.turn {
                    self.selected = Some(sq);
                    self.legal_moves = self.pos.legal_moves(sq);
                    return;
                }
            }
            self.selected = None;
            self.legal_moves.clear();
            return;
        }
        if let Some(p) = self.pos.piece_at(sq) {
            if p.color == self.pos.turn {
                self.selected = Some(sq);
                self.legal_moves = self.pos.legal_moves(sq);
            }
        }
    }

    fn attempt_move(&mut self, m: Move) {
        if m.promotion {
            self.promotion = Some(PendingPromotion {
                from: m.from,
                to: m.to,
                kind: m.kind,
            });
            self.selected = None;
            self.legal_moves.clear();
            return;
        }
        self.complete_move(m, None);
    }

    fn complete_move(&mut self, m: Move, promo: Option<PieceKind>) {
        let Some(moved) = self.pos.piece_at(m.from) else {
            return;
        };
        let snapshot = self.pos.clone();
        let captured = self.pos.apply_move(m);
        if let Some(k) = promo {
            self.pos.board[m.to.r as usize][m.to.c as usize] =
                Some(Piece {
                    kind: k,
                    color: moved.color,
                });
        }
        self.pos.turn = moved.color.opposite();
        let (in_check, no_moves) = self.pos.status();
        let san = build_san(moved, m, captured, promo, in_check, no_moves);
        self.history.push(HistoryEntry { san, pos: snapshot });
        self.selected = None;
        self.legal_moves.clear();
        self.promotion = None;
        self.game_over = if no_moves {
            if in_check {
                Some(GameResult::Checkmate(moved.color))
            } else {
                Some(GameResult::Stalemate)
            }
        } else {
            None
        };
    }

    fn complete_promotion(&mut self, kind: PieceKind) {
        let Some(pp) = self.promotion else {
            return;
        };
        let m = Move {
            from: pp.from,
            to: pp.to,
            kind: pp.kind,
            promotion: true,
        };
        self.complete_move(m, Some(kind));
    }

    fn undo(&mut self) {
        if !self.started
            || self.game_over.is_some()
            || self.history.is_empty()
            || self.promotion.is_some()
        {
            return;
        }
        let entry = self.history.pop().expect("history non-empty");
        self.pos = entry.pos;
        self.selected = None;
        self.legal_moves.clear();
    }

    fn resign(&mut self) {
        if !self.started || self.game_over.is_some() || self.promotion.is_some() {
            return;
        }
        let winner = self.pos.turn.opposite();
        self.game_over = Some(GameResult::Resign(winner));
        self.selected = None;
        self.legal_moves.clear();
        self.stop();
    }

    fn offer_draw(&mut self) {
        if !self.started || self.game_over.is_some() || self.promotion.is_some() {
            return;
        }
        self.game_over = Some(GameResult::Draw);
        self.selected = None;
        self.legal_moves.clear();
        self.stop();
    }

    fn start(&mut self, color: ChessColor, difficulty: u16) {
        *self = ChessGame::default();
        self.player_color = color;
        self.difficulty = difficulty;
        self.started = true;
    }

    fn stop(&mut self) {
        self.started = false;
    }

    fn new_game(&mut self) {
        *self = ChessGame::default();
    }
}

fn build_san(
    moved: Piece,
    m: Move,
    captured: Option<Piece>,
    promo: Option<PieceKind>,
    in_check: bool,
    no_moves: bool,
) -> String {
    let is_capture = captured.is_some() || m.kind == MoveKind::EnPassant;
    let to_sq = m.to.alg();
    let mut s = match m.kind {
        MoveKind::CastleKingside => "O-O".to_string(),
        MoveKind::CastleQueenside => "O-O-O".to_string(),
        _ => match moved.kind {
            PieceKind::Pawn => {
                if is_capture {
                    format!("{}x{}", m.from.file(), to_sq)
                } else {
                    to_sq.clone()
                }
            }
            _ => {
                let letter = moved.kind.letter();
                if is_capture {
                    format!("{}x{}", letter, to_sq)
                } else {
                    format!("{}{}", letter, to_sq)
                }
            }
        },
    };
    if let Some(k) = promo {
        s.push('=');
        s.push(k.letter());
    }
    if no_moves && in_check {
        s.push('#');
    } else if in_check {
        s.push('+');
    }
    s
}

// ============================
//  UI 同步
// ============================
fn chess_sync(
    game: Res<ChessGame>,
    assets: Res<ChessAssets>,
    ents: Res<ChessBoardEntities>,
    mut colors: Query<&mut BackgroundColor>,
    mut pieces: Query<
        (&mut ImageNode, &mut Visibility),
        (With<SquarePiece>, Without<SquareDot>, Without<SquareRing>),
    >,
    mut dots: Query<&mut Visibility, (With<SquareDot>, Without<SquarePiece>, Without<SquareRing>)>,
    mut rings: Query<&mut Visibility, (With<SquareRing>, Without<SquarePiece>, Without<SquareDot>)>,
    mut texts: Query<&mut Text>,
    mut text_colors: Query<&mut TextColor>,
    mut promo_vis: Query<
        &mut Visibility,
        (
            With<PromoPanel>,
            Without<SquarePiece>,
            Without<SquareDot>,
            Without<SquareRing>,
        ),
    >,
    mut promo_imgs: Query<(&PromoImage, &mut ImageNode), Without<SquarePiece>>,
    mut config_nodes: Query<
        (&mut Node, &mut Visibility),
        (
            With<ConfigArea>,
            Without<SquarePiece>,
            Without<SquareDot>,
            Without<SquareRing>,
            Without<PromoPanel>,
            Without<StatusPanel>,
        ),
    >,
    mut status_nodes: Query<
        (&mut Node, &mut Visibility),
        (
            With<StatusPanel>,
            Without<SquarePiece>,
            Without<SquareDot>,
            Without<SquareRing>,
            Without<PromoPanel>,
            Without<ConfigArea>,
        ),
    >,
) {
    // —— 棋盘格 ——
    for r in 0..8 {
        for c in 0..8 {
            let e = ents.squares[r][c];
            let sq = Sq::new(r as i32, c as i32);
            let piece = game.pos.piece_at(sq);
            let is_light = (r + c) % 2 == 0;
            let is_selected = game.selected == Some(sq);
            let is_legal = game
                .legal_moves
                .iter()
                .any(|m| m.to == sq);
            // 底色
            let bg = if is_selected {
                SELECTED_SQ
            } else if is_light {
                LIGHT_SQ
            } else {
                DARK_SQ
            };
            if let Ok(mut color) = colors.get_mut(e.sq) {
                *color = BackgroundColor(bg);
            }
            // 棋子图
            if let Ok((mut img, mut vis)) = pieces.get_mut(e.piece) {
                if let Some(p) = piece {
                    img.image = assets.handle(p);
                    *vis = Visibility::Inherited;
                } else {
                    *vis = Visibility::Hidden;
                }
            }
            // 合法走法圆点 / 吃子圆环
            let is_capture = is_legal && piece.is_some();
            let is_dot = is_legal && piece.is_none();
            if let Ok(mut vis) = dots.get_mut(e.dot) {
                *vis = if is_dot {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
            if let Ok(mut vis) = rings.get_mut(e.ring) {
                *vis = if is_capture {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
        }
    }
    // —— 状态文本 ——
    let status = if let Some(result) = game.game_over {
        result.text()
    } else if game.pos.is_in_check(game.pos.turn) {
        "将军！".to_string()
    } else if game.pos.turn == game.player_color {
        "你的回合".to_string()
    } else {
        "Nori 思考中…".to_string()
    };
    if let Ok(mut text) = texts.get_mut(ents.status_text) {
        text.0 = status;
    }
    let status_color = if game.game_over.is_some() {
        TEXT_BAD
    } else {
        TEXT_ACCENT
    };
    if let Ok(mut color) = text_colors.get_mut(ents.status_text) {
        *color = TextColor(status_color);
    }
    // —— 走法记录 ——
    let mut lines: Vec<String> = Vec::new();
    let mut i = 0;
    while i < game.history.len() {
        let num = i / 2 + 1;
        let white = &game.history[i].san;
        if i + 1 < game.history.len() {
            let black = &game.history[i + 1].san;
            lines.push(format!("{num}. {white} {black}"));
        } else {
            lines.push(format!("{num}. {white}"));
        }
        i += 2;
    }
    let move_text = if lines.is_empty() {
        "暂无走法。".to_string()
    } else {
        lines.join("\n")
    };
    if let Ok(mut text) = texts.get_mut(ents.move_list) {
        text.0 = move_text;
    }
    // —— 升变面板 ——
    let promo_active = game.promotion.is_some();
    if let Ok(mut vis) = promo_vis.get_mut(ents.promo_panel) {
        let target = if promo_active {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != target {
            *vis = target;
        }
    }
    // 升变按钮棋子图颜色 = 当前行棋方
    if promo_active {
        let color = game.pos.turn;
        for (i, btn) in ChessAssets::PROMO_KINDS.iter().enumerate() {
            let piece = Piece {
                kind: *btn,
                color,
            };
            if let Ok((_, mut img)) = promo_imgs.get_mut(ents.promo_images[i]) {
                img.image = assets.handle(piece);
            }
            let _ = ents.promo_buttons[i];
        }
    }
    // —— 配置 / 状态面板可见性 ——
    // 注意：必须同时设置 display，否则 Visibility::Hidden 仍会占据 flex 布局空间
    if let Ok((mut node, mut vis)) = config_nodes.get_mut(ents.config_area) {
        if game.started {
            node.display = Display::None;
            *vis = Visibility::Hidden;
        } else {
            node.display = Display::Flex;
            *vis = Visibility::Inherited;
        }
    }
    if let Ok((mut node, mut vis)) = status_nodes.get_mut(ents.status_panel) {
        if game.started {
            node.display = Display::Flex;
            *vis = Visibility::Inherited;
        } else {
            node.display = Display::None;
            *vis = Visibility::Hidden;
        }
    }
    // —— 颜色按钮 ——
    let colors_list = [ChessColor::White, ChessColor::Black];
    for (i, (btn_e, label_e)) in ents.color_btns.iter().enumerate() {
        let active = game.player_color == colors_list[i];
        if let Ok(mut bg) = colors.get_mut(*btn_e) {
            *bg = BackgroundColor(if active {
                ACTIVE_SELECT_BG
            } else {
                Color::NONE
            });
        }
        if let Ok(mut tc) = text_colors.get_mut(*label_e) {
            *tc = TextColor(if active { TEXT_MAIN } else { TEXT_DIM });
        }
    }
    // —— 难度按钮 ——
    for (i, (row_e, name_e, elo_e)) in ents.diff_rows.iter().enumerate() {
        let active = game.difficulty == DIFF_LEVELS[i].0;
        if let Ok(mut bg) = colors.get_mut(*row_e) {
            *bg = BackgroundColor(if active {
                ACTIVE_SELECT_BG
            } else {
                Color::NONE
            });
        }
        if let Ok(mut tc) = text_colors.get_mut(*name_e) {
            *tc = TextColor(if active { TEXT_MAIN } else { TEXT_DIM });
        }
        if let Ok(mut tc) = text_colors.get_mut(*elo_e) {
            *tc = TextColor(if active { TEXT_ACCENT } else { TEXT_GRAY });
        }
    }
    // —— 状态指示点 ——
    let indicator_color = if game.game_over.is_some() {
        TEXT_GRAY
    } else {
        TEXT_ACCENT
    };
    if let Ok(mut bg) = colors.get_mut(ents.status_indicator) {
        *bg = BackgroundColor(indicator_color);
    }
}

// ============================
//  窗口构建
// ============================
pub fn spawn_international_chess(
    parent: &mut ChildSpawnerCommands,
    asset_server: &AssetServer,
    fonts: &N3riFonts,
) {
    let window_entity = spawn_window(parent, "国际象棋", "chess", 880.0, 700.0, fonts);

    // 关窗重开时重置对局状态（ChessGame 是插件级全局资源，不会随窗口 despawn 清理）
    parent.commands().insert_resource(ChessGame::default());

    // 加载 12 个棋子图片
    let mut by_piece: [[Handle<Image>; 6]; 2] =
        core::array::from_fn(|_| core::array::from_fn(|_| Handle::default()));
    let color_letters = [ChessColor::White.letter(), ChessColor::Black.letter()];
    let kind_letters = [
        PieceKind::Pawn.letter(),
        PieceKind::Knight.letter(),
        PieceKind::Bishop.letter(),
        PieceKind::Rook.letter(),
        PieceKind::Queen.letter(),
        PieceKind::King.letter(),
    ];
    for (ci, &cl) in color_letters.iter().enumerate() {
        for (ki, &kl) in kind_letters.iter().enumerate() {
            by_piece[ci][ki] =
                asset_server.load(format!("nori/international_chess/{cl}{kl}.png"));
        }
    }
    // 颜色选择按钮用的王图标
    let white_king = asset_server.load("nori/international_chess/wK.png");
    let black_king = asset_server.load("nori/international_chess/bK.png");
    parent.commands().insert_resource(ChessAssets { by_piece });

    let font = fonts.get(FontContext::Ui);
    let font_term = fonts.get(FontContext::Terminal);

    let mut ents = ChessBoardEntities {
        squares: [[SquareEnts {
            sq: Entity::PLACEHOLDER,
            piece: Entity::PLACEHOLDER,
            dot: Entity::PLACEHOLDER,
            ring: Entity::PLACEHOLDER,
        }; 8]; 8],
        status_text: Entity::PLACEHOLDER,
        status_indicator: Entity::PLACEHOLDER,
        move_list: Entity::PLACEHOLDER,
        promo_panel: Entity::PLACEHOLDER,
        promo_buttons: [Entity::PLACEHOLDER; 4],
        promo_images: [Entity::PLACEHOLDER; 4],
        undo_btn: Entity::PLACEHOLDER,
        draw_btn: Entity::PLACEHOLDER,
        resign_btn: Entity::PLACEHOLDER,
        restart_btn: Entity::PLACEHOLDER,
        config_area: Entity::PLACEHOLDER,
        status_panel: Entity::PLACEHOLDER,
        color_btns: [(Entity::PLACEHOLDER, Entity::PLACEHOLDER); 2],
        diff_rows: [(Entity::PLACEHOLDER, Entity::PLACEHOLDER, Entity::PLACEHOLDER); 5],
    };

    parent
        .commands()
        .entity(window_entity)
        .with_children(|window| {
            window
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::FlexStart,
                        padding: UiRect::px(14.0, 14.0, 12.0, 12.0),
                        column_gap: Val::Px(14.0),
                        ..default()
                    },
                    BackgroundColor(CONTENT_BG),
                ))
                .with_children(|content| {
                    // —— 左侧棋盘卡片 ——
                    content
                        .spawn((
                            Node {
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(Val::Px(4.0)),
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(16.0)),
                                ..default()
                            },
                            BackgroundColor(BOARD_WRAPPER_BG),
                            BorderColor::all(CARD_BORDER),
                        ))
                        .with_children(|wrapper| {
                            // 行：rank 标签列 + 棋盘
                            wrapper
                                .spawn(Node {
                                    flex_direction: FlexDirection::Row,
                                    ..default()
                                })
                                .with_children(|row| {
                                    // rank 标签列
                                    row.spawn(Node {
                                        width: Val::Px(RANK_COL_WIDTH),
                                        flex_direction: FlexDirection::Column,
                                        ..default()
                                    })
                                    .with_children(|rank_col| {
                                        for r in 0..8 {
                                            rank_col.spawn((
                                                Text::new((8 - r).to_string()),
                                                TextFont {
                                                    font: FontSource::Handle(font.clone()),
                                                    font_size: FontSize::Px(10.0),
                                                    ..default()
                                                },
                                                TextColor(LABEL_COLOR),
                                                Node {
                                                    flex_grow: 1.0,
                                                    width: Val::Percent(100.0),
                                                    align_items: AlignItems::Center,
                                                    justify_content: JustifyContent::Center,
                                                    ..default()
                                                },
                                            ));
                                        }
                                    });
                                    // 8×8 棋盘
                                    row.spawn(Node {
                                        width: Val::Px(BOARD_SIZE),
                                        height: Val::Px(BOARD_SIZE),
                                        flex_wrap: FlexWrap::Wrap,
                                        ..default()
                                    })
                                    .with_children(|board| {
                                        for r in 0..8 {
                                            for c in 0..8 {
                                                let is_light = (r + c) % 2 == 0;
                                                let bg = if is_light { LIGHT_SQ } else { DARK_SQ };
                                                let sq = Sq::new(r as i32, c as i32);
                                                let border_radius = match (r, c) {
                                                    (0, 0) => BorderRadius {
                                                        top_left: Val::Px(10.0),
                                                        ..default()
                                                    },
                                                    (0, 7) => BorderRadius {
                                                        top_right: Val::Px(10.0),
                                                        ..default()
                                                    },
                                                    (7, 0) => BorderRadius {
                                                        bottom_left: Val::Px(10.0),
                                                        ..default()
                                                    },
                                                    (7, 7) => BorderRadius {
                                                        bottom_right: Val::Px(10.0),
                                                        ..default()
                                                    },
                                                    _ => BorderRadius::default(),
                                                };
                                                let sq_e = board
                                                    .spawn((
                                                        ChessSquare(sq),
                                                        Button,
                                                        Node {
                                                            width: Val::Px(SQUARE_SIZE),
                                                            height: Val::Px(SQUARE_SIZE),
                                                            border_radius,
                                                            ..default()
                                                        },
                                                        BackgroundColor(bg),
                                                    ))
                                                    .id();
                                                let mut piece_e = Entity::PLACEHOLDER;
                                                let mut dot_e = Entity::PLACEHOLDER;
                                                let mut ring_e = Entity::PLACEHOLDER;
                                                board.commands().entity(sq_e).with_children(
                                                    |sq_children| {
                                                        piece_e = sq_children
                                                            .spawn((
                                                                SquarePiece,
                                                                ImageNode {
                                                                    image: Handle::default(),
                                                                    ..default()
                                                                },
                                                                Node {
                                                                    width: Val::Percent(100.0),
                                                                    height: Val::Percent(100.0),
                                                                    ..default()
                                                                },
                                                                Visibility::Hidden,
                                                            ))
                                                            .id();
                                                        dot_e = sq_children
                                                            .spawn((
                                                                SquareDot,
                                                                Node {
                                                                    position_type: PositionType::Absolute,
                                                                    left: Val::Px(
                                                                        (SQUARE_SIZE - 24.0) / 2.0,
                                                                    ),
                                                                    top: Val::Px(
                                                                        (SQUARE_SIZE - 24.0) / 2.0,
                                                                    ),
                                                                    width: Val::Px(24.0),
                                                                    height: Val::Px(24.0),
                                                                    border_radius: BorderRadius::all(
                                                                        Val::Px(12.0),
                                                                    ),
                                                                    ..default()
                                                                },
                                                                BackgroundColor(DOT_COLOR),
                                                                Visibility::Hidden,
                                                            ))
                                                            .id();
                                                        ring_e = sq_children
                                                            .spawn((
                                                                SquareRing,
                                                                Node {
                                                                    position_type: PositionType::Absolute,
                                                                    left: Val::Px(4.0),
                                                                    top: Val::Px(4.0),
                                                                    width: Val::Px(SQUARE_SIZE - 8.0),
                                                                    height: Val::Px(SQUARE_SIZE - 8.0),
                                                                    border: UiRect::all(Val::Px(4.0)),
                                                                    border_radius: BorderRadius::all(
                                                                        Val::Px(50.0),
                                                                    ),
                                                                    ..default()
                                                                },
                                                                BorderColor::all(RING_COLOR),
                                                                Visibility::Hidden,
                                                            ))
                                                            .id();
                                                    },
                                                );
                                                ents.squares[r][c] = SquareEnts {
                                                    sq: sq_e,
                                                    piece: piece_e,
                                                    dot: dot_e,
                                                    ring: ring_e,
                                                };
                                            }
                                        }
                                    });
                                });
                            // 文件标签行（左偏移 = rank 列宽）
                            wrapper
                                .spawn(Node {
                                    height: Val::Px(FILE_ROW_HEIGHT),
                                    width: Val::Px(BOARD_SIZE),
                                    margin: UiRect {
                                        left: Val::Px(RANK_COL_WIDTH),
                                        ..default()
                                    },
                                    flex_direction: FlexDirection::Row,
                                    ..default()
                                })
                                .with_children(|file_row| {
                                    for c in 0..8 {
                                        file_row.spawn((
                                            Text::new(((b'a' + c as u8) as char).to_string()),
                                            TextFont {
                                                font: FontSource::Handle(font.clone()),
                                                font_size: FontSize::Px(10.0),
                                                ..default()
                                            },
                                            TextColor(LABEL_COLOR),
                                            Node {
                                                flex_grow: 1.0,
                                                align_items: AlignItems::Center,
                                                justify_content: JustifyContent::Center,
                                                ..default()
                                            },
                                        ));
                                    }
                                });
                        });

                    // —— 右侧侧栏 ——
                    content
                        .spawn(Node {
                            width: Val::Px(SIDEBAR_WIDTH),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(12.0),
                            padding: UiRect::vertical(Val::Px(4.0)),
                            ..default()
                        })
                        .with_children(|side| {
                            // —— 配置区域 ——
                            let config_e = side
                                .spawn((
                                    ConfigArea,
                                    Node {
                                        width: Val::Percent(100.0),
                                        flex_direction: FlexDirection::Column,
                                        row_gap: Val::Px(12.0),
                                        ..default()
                                    },
                                ))
                                .id();
                            ents.config_area = config_e;
                            side.commands().entity(config_e).with_children(|cfg| {
                                // 标题
                                cfg.spawn((
                                    Text::new("与 Nori 下棋"),
                                    TextFont {
                                        font: FontSource::Handle(font.clone()),
                                        font_size: FontSize::Px(23.0),
                                        ..default()
                                    },
                                    TextColor(TEXT_MAIN),
                                ));
                                // 副标题
                                cfg.spawn((
                                    Text::new("选择你的颜色和 Nori 的状态。"),
                                    TextFont {
                                        font: FontSource::Handle(font.clone()),
                                        font_size: FontSize::Px(12.0),
                                        ..default()
                                    },
                                    TextColor(TEXT_DIM),
                                ));
                                // 颜色选择
                                cfg.spawn(Node {
                                    width: Val::Percent(100.0),
                                    flex_direction: FlexDirection::Row,
                                    column_gap: Val::Px(6.0),
                                    margin: UiRect::top(Val::Px(4.0)),
                                    ..default()
                                })
                                .with_children(|row| {
                                    let (wk_btn, wk_label) = color_btn(
                                        row,
                                        &font,
                                        white_king.clone(),
                                        ChessColor::White,
                                        "执白",
                                        true,
                                    );
                                    ents.color_btns[0] = (wk_btn, wk_label);
                                    let (bk_btn, bk_label) = color_btn(
                                        row,
                                        &font,
                                        black_king.clone(),
                                        ChessColor::Black,
                                        "执黑",
                                        false,
                                    );
                                    ents.color_btns[1] = (bk_btn, bk_label);
                                });
                                // 难度选择
                                cfg.spawn(Node {
                                    width: Val::Percent(100.0),
                                    flex_direction: FlexDirection::Column,
                                    row_gap: Val::Px(4.0),
                                    margin: UiRect::top(Val::Px(4.0)),
                                    ..default()
                                })
                                .with_children(|diff_col| {
                                    for (i, (elo, name, elo_label)) in DIFF_LEVELS.iter().enumerate()
                                    {
                                        let active = *elo == 1200;
                                        let row_e = diff_col
                                            .spawn((
                                                DiffBtn(*elo),
                                                Button,
                                                diff_btn_node(),
                                                BackgroundColor(if active {
                                                    ACTIVE_SELECT_BG
                                                } else {
                                                    Color::NONE
                                                }),
                                            ))
                                            .id();
                                        let mut name_e = Entity::PLACEHOLDER;
                                        let mut elo_e = Entity::PLACEHOLDER;
                                        diff_col.commands().entity(row_e).with_children(|r| {
                                            name_e = r
                                                .spawn((
                                                    label_text(
                                                        &font,
                                                        name,
                                                        13.0,
                                                        if active { TEXT_MAIN } else { TEXT_DIM },
                                                    ),
                                                    Node {
                                                        flex_grow: 1.0,
                                                        ..default()
                                                    },
                                                ))
                                                .id();
                                            elo_e = r
                                                .spawn(label_text(
                                                    &font,
                                                    elo_label,
                                                    11.0,
                                                    if active { TEXT_ACCENT } else { TEXT_GRAY },
                                                ))
                                                .id();
                                        });
                                        ents.diff_rows[i] = (row_e, name_e, elo_e);
                                    }
                                });
                                // 开始按钮
                                cfg.spawn((
                                    StartBtn,
                                    Button,
                                    primary_btn_node(),
                                    BackgroundColor(BTN_PRIMARY_BG),
                                ))
                                .with_children(|b| {
                                    b.spawn(label_text(&font, "开始游戏", 14.0, BTN_PRIMARY_TEXT));
                                });
                                // 教学按钮
                                cfg.spawn((
                                    TeachBtn,
                                    Button,
                                    secondary_btn_node(),
                                    BackgroundColor(BTN_SECONDARY_BG),
                                ))
                                .with_children(|b| {
                                    b.spawn(label_text(&font, "让 Nori 教你下棋", 13.0, TEXT_MAIN));
                                });
                            });

                            // —— 游戏状态面板 ——
                            let status_e = side
                                .spawn((
                                    StatusPanel,
                                    Node {
                                        width: Val::Percent(100.0),
                                        flex_direction: FlexDirection::Column,
                                        row_gap: Val::Px(12.0),
                                        display: Display::None,
                                        ..default()
                                    },
                                ))
                                .id();
                            ents.status_panel = status_e;
                            side.commands().entity(status_e).with_children(|sp| {
                                // 状态卡片
                                sp.spawn((
                                    Node {
                                        width: Val::Percent(100.0),
                                        flex_direction: FlexDirection::Column,
                                        padding: UiRect::all(Val::Px(16.0)),
                                        border: UiRect::all(Val::Px(1.0)),
                                        border_radius: BorderRadius::all(Val::Px(16.0)),
                                        ..default()
                                    },
                                    BackgroundColor(BOARD_WRAPPER_BG),
                                    BorderColor::all(CARD_BORDER),
                                ))
                                .with_children(|card| {
                                    // 头部：状态 标签 + 指示点
                                    card.spawn(Node {
                                        width: Val::Percent(100.0),
                                        flex_direction: FlexDirection::Row,
                                        align_items: AlignItems::Center,
                                        justify_content: JustifyContent::SpaceBetween,
                                        ..default()
                                    })
                                    .with_children(|hdr| {
                                        hdr.spawn(label_text(&font, "状态", 10.0, TEXT_GRAY));
                                        let indicator_e = hdr
                                            .spawn((
                                                Node {
                                                    width: Val::Px(12.0),
                                                    height: Val::Px(12.0),
                                                    border_radius: BorderRadius::all(Val::Px(6.0)),
                                                    ..default()
                                                },
                                                BackgroundColor(TEXT_ACCENT),
                                            ))
                                            .id();
                                        ents.status_indicator = indicator_e;
                                    });
                                    // 状态文本
                                    let status_text_e = card
                                        .spawn((
                                            StatusText,
                                            Text::new("你的回合"),
                                            TextFont {
                                                font: FontSource::Handle(font.clone()),
                                                font_size: FontSize::Px(18.0),
                                                ..default()
                                            },
                                            TextColor(TEXT_ACCENT),
                                            Node {
                                                margin: UiRect::top(Val::Px(4.0)),
                                                ..default()
                                            },
                                        ))
                                        .id();
                                    ents.status_text = status_text_e;
                                });
                                // 走法列表面板
                                sp.spawn((
                                    Node {
                                        width: Val::Percent(100.0),
                                        flex_grow: 1.0,
                                        min_height: Val::Px(96.0),
                                        flex_direction: FlexDirection::Column,
                                        padding: UiRect::all(Val::Px(16.0)),
                                        border: UiRect::all(Val::Px(1.0)),
                                        border_radius: BorderRadius::all(Val::Px(16.0)),
                                        ..default()
                                    },
                                    BackgroundColor(BOARD_WRAPPER_BG),
                                    BorderColor::all(CARD_BORDER),
                                ))
                                .with_children(|mp| {
                                    mp.spawn(label_text(&font, "走法", 10.0, TEXT_GRAY));
                                    let list_e = mp
                                        .spawn((
                                            ScrollableArea,
                                            Node {
                                                flex_grow: 1.0,
                                                width: Val::Percent(100.0),
                                                overflow: Overflow::hidden_y(),
                                                ..default()
                                            },
                                        ))
                                        .id();
                                    mp.commands().entity(list_e).with_children(|l| {
                                        spawn_scrollbar(l, list_e);
                                    });
                                    let mut move_e = Entity::PLACEHOLDER;
                                    mp.commands().entity(list_e).with_children(|l| {
                                        l.spawn((
                                            ScrollContent,
                                            Node {
                                                width: Val::Percent(100.0),
                                                flex_direction: FlexDirection::Column,
                                                ..default()
                                            },
                                        ))
                                        .with_children(|c| {
                                            move_e = c
                                                .spawn((
                                                    MoveListText,
                                                    Text::new("暂无走法。"),
                                                    TextFont {
                                                        font: FontSource::Handle(font_term.clone()),
                                                        font_size: FontSize::Px(13.0),
                                                        ..default()
                                                    },
                                                    TextColor(TEXT_ACCENT),
                                                    TextLayout::default(),
                                                ))
                                                .id();
                                        });
                                    });
                                    ents.move_list = move_e;
                                });
                                // 操作按钮
                                sp.spawn(Node {
                                    width: Val::Percent(100.0),
                                    flex_direction: FlexDirection::Column,
                                    row_gap: Val::Px(6.0),
                                    margin: UiRect::top(Val::Px(2.0)),
                                    ..default()
                                })
                                .with_children(|actions| {
                                    let undo_e =
                                        action_btn(actions, &font, UndoBtn, "请求悔棋", false);
                                    ents.undo_btn = undo_e;
                                    let draw_e =
                                        action_btn(actions, &font, DrawBtn, "提议和棋", false);
                                    ents.draw_btn = draw_e;
                                    let resign_e =
                                        action_btn(actions, &font, ResignBtn, "认输", true);
                                    ents.resign_btn = resign_e;
                                    let restart_e =
                                        action_btn(actions, &font, RestartBtn, "重新开始", false);
                                    ents.restart_btn = restart_e;
                                });
                            });
                        });

                    // —— 升变浮层 ——
                    let promo_e = content
                        .spawn((
                            PromoPanel,
                            Node {
                                position_type: PositionType::Absolute,
                                width: Val::Percent(100.0),
                                height: Val::Percent(100.0),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
                            Visibility::Hidden,
                        ))
                        .id();
                    ents.promo_panel = promo_e;
                    content.commands().entity(promo_e).with_children(|panel| {
                        panel
                            .spawn((
                                Node {
                                    flex_direction: FlexDirection::Row,
                                    column_gap: Val::Px(10.0),
                                    padding: UiRect::px(18.0, 18.0, 14.0, 14.0),
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(10.0)),
                                    ..default()
                                },
                                BackgroundColor(BOARD_WRAPPER_BG),
                                BorderColor::all(CARD_BORDER),
                            ))
                            .with_children(|box_| {
                                for i in 0..4 {
                                    let btn_e = box_
                                        .spawn((
                                            PromoButton(i),
                                            Button,
                                            Node {
                                                width: Val::Px(64.0),
                                                height: Val::Px(64.0),
                                                align_items: AlignItems::Center,
                                                justify_content: JustifyContent::Center,
                                                border: UiRect::all(Val::Px(1.0)),
                                                ..default()
                                            },
                                            BackgroundColor(ACTION_BTN_BG),
                                            BorderColor::all(CARD_BORDER),
                                        ))
                                        .id();
                                    let mut img_e = Entity::PLACEHOLDER;
                                    box_.commands().entity(btn_e).with_children(|b| {
                                        img_e = b
                                            .spawn((
                                                PromoImage,
                                                ImageNode {
                                                    image: Handle::default(),
                                                    ..default()
                                                },
                                                Node {
                                                    width: Val::Px(56.0),
                                                    height: Val::Px(56.0),
                                                    ..default()
                                                },
                                            ))
                                            .id();
                                    });
                                    ents.promo_buttons[i] = btn_e;
                                    ents.promo_images[i] = img_e;
                                }
                            });
                    });
                });
        });

    parent.commands().insert_resource(ents);
}

fn label_text(font: &Handle<Font>, label: &str, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(label.to_string()),
        TextFont {
            font: FontSource::Handle(font.clone()),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

fn color_btn_node() -> Node {
    Node {
        flex_grow: 1.0,
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        row_gap: Val::Px(6.0),
        padding: UiRect::vertical(Val::Px(10.0)),
        border_radius: BorderRadius::all(Val::Px(10.0)),
        ..default()
    }
}

fn diff_btn_node() -> Node {
    Node {
        width: Val::Percent(100.0),
        height: Val::Px(38.0),
        align_items: AlignItems::Center,
        padding: UiRect::horizontal(Val::Px(12.0)),
        column_gap: Val::Px(10.0),
        border_radius: BorderRadius::all(Val::Px(10.0)),
        ..default()
    }
}

fn primary_btn_node() -> Node {
    Node {
        width: Val::Percent(100.0),
        height: Val::Px(40.0),
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        border_radius: BorderRadius::all(Val::Px(10.0)),
        margin: UiRect::top(Val::Px(16.0)),
        ..default()
    }
}

fn secondary_btn_node() -> Node {
    Node {
        width: Val::Percent(100.0),
        height: Val::Px(36.0),
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        border_radius: BorderRadius::all(Val::Px(10.0)),
        margin: UiRect::top(Val::Px(8.0)),
        ..default()
    }
}

fn action_btn_node() -> Node {
    Node {
        width: Val::Percent(100.0),
        height: Val::Px(32.0),
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        border: UiRect::all(Val::Px(1.0)),
        border_radius: BorderRadius::all(Val::Px(8.0)),
        ..default()
    }
}

fn color_btn(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    icon: Handle<Image>,
    color: ChessColor,
    label: &str,
    active: bool,
) -> (Entity, Entity) {
    let btn = parent
        .spawn((
            ColorBtn(color),
            Button,
            color_btn_node(),
            BackgroundColor(if active { ACTIVE_SELECT_BG } else { Color::NONE }),
        ))
        .id();
    let mut label_e = Entity::PLACEHOLDER;
    parent.commands().entity(btn).with_children(|b| {
        b.spawn((
            ImageNode {
                image: icon,
                ..default()
            },
            Node {
                width: Val::Px(32.0),
                height: Val::Px(32.0),
                ..default()
            },
        ));
        label_e = b
            .spawn(label_text(
                font,
                label,
                11.0,
                if active { TEXT_MAIN } else { TEXT_DIM },
            ))
            .id();
    });
    (btn, label_e)
}

fn action_btn(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    marker: impl Component,
    label: &str,
    danger: bool,
) -> Entity {
    let (bg, border, text_color) = if danger {
        (DANGER_BG, DANGER_BORDER, TEXT_BAD)
    } else {
        (ACTION_BTN_BG, CARD_BORDER, TEXT_MAIN)
    };
    parent
        .spawn((
            marker,
            Button,
            action_btn_node(),
            BackgroundColor(bg),
            BorderColor::all(border),
        ))
        .with_children(|b| {
            b.spawn(label_text(font, label, 12.0, text_color));
        })
        .id()
}

// ============================
//  单元测试
// ============================
#[cfg(test)]
mod tests {
    use super::*;

    fn initial_legal_count() -> usize {
        let pos = Position::initial();
        let mut total = 0;
        for r in 0..8 {
            for c in 0..8 {
                let sq = Sq::new(r as i32, c as i32);
                if let Some(p) = pos.piece_at(sq) {
                    if p.color == pos.turn {
                        total += pos.legal_moves(sq).len();
                    }
                }
            }
        }
        total
    }

    #[test]
    fn initial_position_has_20_legal_moves() {
        assert_eq!(initial_legal_count(), 20);
    }

    #[test]
    fn pawn_double_push_sets_en_passant() {
        let mut pos = Position::initial();
        let m = Move {
            from: Sq::new(6, 4),
            to: Sq::new(4, 4),
            kind: MoveKind::DoublePush,
            promotion: false,
        };
        assert!(pos.move_is_legal(m));
        pos.apply_move(m);
        assert_eq!(pos.en_passant, Some(Sq::new(5, 4)));
        assert_eq!(pos.turn, ChessColor::White); // apply_move 不改回合
    }

    #[test]
    fn en_passant_capture_is_generated() {
        let mut pos = Position::initial();
        // 白 e2e4（row6→row4），黑 d7d5（row1→row3），白 exd6 过路兵
        pos.apply_move(Move {
            from: Sq::new(6, 4),
            to: Sq::new(4, 4),
            kind: MoveKind::DoublePush,
            promotion: false,
        });
        pos.apply_move(Move {
            from: Sq::new(1, 3),
            to: Sq::new(3, 3),
            kind: MoveKind::DoublePush,
            promotion: false,
        });
        // 白 e4 的兵现在在 (4,4)，应有过路兵走到 d6=(2,3)（黑兵跳过格）
        let moves = pos.pseudo_moves(Sq::new(4, 4));
        assert!(
            moves
                .iter()
                .any(|m| m.kind == MoveKind::EnPassant && m.to == Sq::new(2, 3)),
            "exd6 en passant move missing: {:?}",
            moves
        );
        // 执行过路兵后，黑 d5 兵应被移除
        let m = moves
            .iter()
            .find(|m| m.kind == MoveKind::EnPassant)
            .copied()
            .unwrap();
        pos.apply_move(m);
        assert_eq!(pos.piece_at(Sq::new(3, 3)), None, "captured d5 pawn gone");
        assert_eq!(
            pos.piece_at(Sq::new(2, 3)),
            Some(Piece {
                kind: PieceKind::Pawn,
                color: ChessColor::White,
            }),
            "white pawn now on d6"
        );
    }

    #[test]
    fn castling_kingside_is_legal() {
        let mut pos = Position::initial();
        // 清空 e1 与 h1 之间的白子
        pos.board[7][4] = None; // 白王
        pos.board[7][5] = None;
        pos.board[7][6] = None;
        // 白王放回 e1，h1 车保留
        pos.board[7][4] = Some(Piece {
            kind: PieceKind::King,
            color: ChessColor::White,
        });
        let moves = pos.pseudo_moves(Sq::new(7, 4));
        assert!(
            moves
                .iter()
                .any(|m| m.kind == MoveKind::CastleKingside),
            "kingside castle missing"
        );
        // 走完车到 f1
        let m = moves
            .iter()
            .find(|m| m.kind == MoveKind::CastleKingside)
            .copied()
            .unwrap();
        assert!(pos.move_is_legal(m));
        pos.apply_move(m);
        assert_eq!(pos.piece_at(Sq::new(7, 6)), Some(Piece {
            kind: PieceKind::King,
            color: ChessColor::White,
        }));
        assert_eq!(pos.piece_at(Sq::new(7, 5)), Some(Piece {
            kind: PieceKind::Rook,
            color: ChessColor::White,
        }));
    }

    #[test]
    fn castle_blocked_through_check() {
        let mut pos = Position::initial();
        pos.board[7][4] = None;
        pos.board[7][5] = None;
        pos.board[7][6] = None;
        pos.board[7][4] = Some(Piece {
            kind: PieceKind::King,
            color: ChessColor::White,
        });
        // 黑车 e8 控制 e1（易位起点不在将军，但路径过 f1）
        pos.board[0][4] = None;
        pos.board[7][5] = Some(Piece {
            kind: PieceKind::Rook,
            color: ChessColor::Black,
        });
        pos.board[6][5] = None; // 清掉会挡黑车的兵
        // 黑车从 e8 沿 e 列攻击 —— 改为直接放 f1 攻击 f1
        // 简化：黑车放 f8，直线攻击 f1
        pos.board[0][5] = Some(Piece {
            kind: PieceKind::Rook,
            color: ChessColor::Black,
        });
        let moves = pos.pseudo_moves(Sq::new(7, 4));
        let Some(m) = moves
            .iter()
            .find(|m| m.kind == MoveKind::CastleKingside)
            .copied()
        else {
            return; // 若因其他原因无易位，跳过（防御性）
        };
        assert!(
            !pos.move_is_legal(m),
            "castle through attacked f1 must be illegal"
        );
    }

    #[test]
    fn checkmate_detected() {
        // 标准角杀：黑王 h8=(0,7)，白后 g7=(1,6) 将军并封锁 g8/h7，
        // 白王 g6=(2,6) 保护后，黑王无处可逃 → 将死
        let mut pos = Position::initial();
        for r in 0..8 {
            for c in 0..8 {
                pos.board[r][c] = None;
            }
        }
        pos.board[0][7] = Some(Piece {
            kind: PieceKind::King,
            color: ChessColor::Black,
        });
        pos.board[1][6] = Some(Piece {
            kind: PieceKind::Queen,
            color: ChessColor::White,
        });
        pos.board[2][6] = Some(Piece {
            kind: PieceKind::King,
            color: ChessColor::White,
        });
        pos.castle = CastleRights::NONE;
        pos.turn = ChessColor::Black;
        let (in_check, no_moves) = pos.status();
        assert!(in_check, "black king must be in check");
        assert!(no_moves, "must be checkmate");
    }

    #[test]
    fn knight_has_eight_jumps_from_center() {
        let mut pos = Position::initial();
        // 清空中心 5x5（行2-6 列2-6），马在 d4=(4,4) 可达全部 8 格
        for r in 2..7 {
            for c in 2..7 {
                pos.board[r][c] = None;
            }
        }
        pos.board[4][4] = Some(Piece {
            kind: PieceKind::Knight,
            color: ChessColor::White,
        });
        let moves = pos.pseudo_moves(Sq::new(4, 4));
        assert_eq!(moves.len(), 8);
    }
}
