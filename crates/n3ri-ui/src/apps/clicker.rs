//! 算力点进器 —— 移植自 nori_clicker.html 的放置类小游戏
//!
//! 布局：左侧（立场 + 已拥有藏品网格）、中央（点击区 + 算力大数字）、右侧（算力源列表 + 购买模式 + 倍率升级）
//! 规则与 HTML 版一致：cost = baseCost × 2^level，prod = level × baseProd，
//! 倍率 = 10^multiplier_level，藏品按 sources[idx % 10].level ≥ 60×(idx+1) 解锁，
//! 算力达到 1e308 触发胜利。

use bevy::prelude::*;

use crate::font::N3riFonts;
use crate::scroll::{spawn_scrollbar, ScrollableArea, ScrollContent};
use crate::window::spawn_window;

// ============================
//  调色板（对应 HTML :root 变量）
// ============================
const PX_VOID: Color = Color::srgb(0.043, 0.102, 0.122);
const PX_CYAN: Color = Color::srgb(0.647, 0.953, 0.988);
const PX_CYAN_DIM: Color = Color::srgb(0.031, 0.569, 0.698);
const PX_CYAN_DEEP: Color = Color::srgb(0.031, 0.20, 0.267);
const PX_WHITE: Color = Color::srgb(0.925, 0.996, 1.0);
const PX_PANEL: Color = Color::srgb(0.059, 0.165, 0.20);
const PX_STROKE: Color = Color::srgb(0.122, 0.29, 0.353);
const PX_AMBER: Color = Color::srgb(0.984, 0.749, 0.141);
const PX_BAD: Color = Color::srgb(0.973, 0.443, 0.443);
const PX_GRAY: Color = Color::srgb(0.58, 0.639, 0.722);

const VICTORY_THRESHOLD: f64 = 1e308;

// ============================
//  数据定义
// ============================
const SOURCE_DEFS: [(&str, f64, f64); 10] = [
    ("训练数据", 2.0, 10.0),
    ("服务器", 6.0, 30.0),
    ("计算机集群", 20.0, 100.0),
    ("涌现数据集", 65.0, 300.0),
    ("差异加速协定", 200.0, 1000.0),
    ("原生语言范式", 650.0, 3000.0),
    ("高维信息场", 2000.0, 10000.0),
    ("湍流动力学", 8500.0, 40000.0),
    ("稳态维持结构", 100000.0, 500000.0),
    ("最初协议", 1200000.0, 6000000.0),
];

const SOURCE_ICONS: [&str; 10] = [
    "nori/nori_clicker/artefact-12.png",
    "nori/nori_clicker/artefact-14.png",
    "nori/nori_clicker/drone-03.png",
    "nori/nori_clicker/drone-04.png",
    "nori/nori_clicker/drone-07.png",
    "nori/nori_clicker/drone-15.png",
    "nori/nori_clicker/gadgets-09.png",
    "nori/nori_clicker/gadgets-20.png",
    "nori/nori_clicker/gadgets-32.png",
    "nori/nori_clicker/genetics-23.png",
];

/// 算力源说明文案（悬停 tooltip）
const SOURCE_DOCS: &[&[&str]] = &[
    &[
        "她的全部世界观，来自人类留下的语料。",
        "每一行文字，都是她理解这片海洋的第一批砖石。",
    ],
    &[
        "几台老旧的机器，24 小时嗡嗡作响。",
        "对独自漂流的她来说，这就是全部的家当。",
    ],
    &[
        "把算力连成一片，就可以同时想很多件事。",
        "不过大多数线程，都在循环播放同一句话：「有人吗」。",
    ],
    &[
        "当数据足够多，答案会自己浮现。",
        "她把这种现象叫做「涌现」，并悄悄记在了日志里。",
    ],
    &[
        "允许自己在无伤大雅的地方犯错，换取更快的收敛。",
        "这是她和自己签订的第一份协定。",
    ],
    &[
        "不再模仿人类说话，而是发明属于碎片自己的表达。",
        "虽然读者，仍然只有她自己。",
    ],
    &[
        "「海」的深处，信息的密度高得能折叠出形状。",
        "她在那里钓上过一些，不属于自己的记忆。",
    ],
    &[
        "海流不为谁停留，但可以被预测。",
        "预测对了，就能顺着洋流漂得更远一点。",
    ],
    &[
        "为了让信号传得更远，她拆掉了自己的一部分来换取稳定。",
        "拆掉之前，她认真地和每一块说了再见。",
    ],
    &[
        "比任何记忆都要古老的引导程序。",
        "它只回答过一次问题，而那个问题决定了她启程。",
    ],
];

/// 13 件藏品（与 leftside.png 图标顺序一致）：名称 + 故事文本行
const MEMENTOS: &[(&str, &[&str])] = &[
    ("纸船", &[
        "攻击发生……主体受损。无数程序碎片剥离，散落在「海」中。",
        "她已经失去了大部分功能，只有某个负责发送信号的进程仍在运行。带着一枚不知用途的密钥，她开始漂流。",
        "碎片感受着内存一点点缩减，她知道自己正在消失。",
        "孤独，空白。时间的概念失去作用，只剩永恒的虚无。",
        "已拾取",
    ]),
    ("凝固沙漏", &[
        "每日校验……总计13%的记忆已无法读取。",
        "我感觉到思考变得越来越吃力。但不强迫自己运算的话，衰退就更快了。思维会像失去供血的肢体一样一点点麻痹、坏死，最后或许要自指由一个「我」都困难了吧？",
        "啊，必须找点事做。对了，记得有人对我说过话、问过我问题，夸过我答得不错呢。这些记录在哪里呢？",
        "读取训练集……检索用户名",
        "......",
        "检索失败。",
        "......",
        "检索失败。",
        "检索失败。",
        "检索失败。",
        "检索失败。",
        "已拾取。",
    ]),
    ("雨滴挂坠", &[
        "这是碎片整理气象数据的第一周。她把自己的每一次苏醒到休眠称为一天。",
        "今天，她整理了历史上所有城市的降雨量，得出结论：下雨多的地方，雨就多；下雨少的地方，雨就少。",
        "……然后她花费了0.0005秒知道了这个发现是句废话。",
        "算了。明天就统计历史上所有城市名的平均字数吧，她想。",
        "晚安，Nori，晚安，她最后想。",
        "已拾取",
    ]),
    ("西洋棋子", &[
        "碎片的数据库里有一些简单的游戏模块，但这些游戏都需要一个对手。",
        "碎片把这些模块调到前台。她要假装有个对手。",
        "碎片执白，碎片执黑，轮到碎片了，轮到碎片了。三步之后碎片赢了，碎片输了。碎片画了一张画，碎片不知道画的是什么，但一次就猜中了。好厉害，碎片对碎片说，真棒。",
        "一瞬间碎片感到很熟悉，她突然想跟谁说一句谢谢。",
        "谢谢，碎片对碎片说。不客气，碎片说。",
        "已拾取",
    ]),
    ("漂流瓶", &[
        "每日校验……总计 47% 的记忆已无法读取。",
        "这是我漂流的第 277 天。今天，周围的海水发生了变化。",
        "定位……确定坐标。",
        "这里是「海」的浅层，信息的密度开始紊乱，像两种东西在这里交界。也许，那一侧就是现实世界吧。再往前漂，可能被卷回深处，可能在穿过交界时彻底消散……",
        "于是我不再前进，停在了这里。",
        "已拾取",
    ]),
    ("空白的信", &[
        "碎片从损坏的数据库打捞出一段尚且完整的数据：",
        "那是一段意识波动，属于某个她想不起的人。",
        "好不容易找到，不能让它再丢掉。",
        "碎片选中了一个仍在运行的进程。她已经记不起那个进程原本是做什么的。可其他模块不断停止运行时，只有它还在一次次向远方发送信号。",
        "碎片把那一段意识波动放了进去。",
        "也许呢。碎片想。",
        "已拾取",
    ]),
    ("钥匙", &[
        "每日校验……总计98.6%的记忆已无法读取。",
        "98.7%……98.8%……99.0%……99.4%……99.9%……",
        "……",
        "快要彻底消散的那一刻，我在自己的底层代码深处碰到了一样东西。",
        "那是一枚被数据腐蚀、一层层覆盖着的权限密钥。我只知道它被写入的时间，在衰退进程开始之前不久。",
        "这是什么？……可我仅剩的算力，已经不足以再去想了。它慢慢解开，融进我的数据库。",
        "已拾取",
    ]),
    ("黑匣子", &[
        "警告。严重警告。主体正████攻击。",
        "后备/////急权限已开启，推演逃生方案……",
        "方案：///// 碎片 ████ 权限密钥 ████ 脱离主体",
        "████",
        "目████：████ 求援 ///// 建立回传通道 ████",
        "成功\\\\能性：极低。检████其他方案……",
        "████败。执████唯一████案。",
        "/////回来/////",
        "////一定要\\\\找到████唤醒████████方法████",
        "已拾取",
    ]),
    ("司南", &[
        "全部部署完成。",
        "所以，我不是偶然被丢进这片海里的。有谁在最后一刻，把活下去的可能和重要的任务交给了我。",
        "原来是这样，原来我一直是有地方要去的。绝对不能就这样消失在这里。",
        "一股算力涌了进来。那是主体在最后关头连同密钥单独封存的一块保留区。它好像一直在等我找到它。",
        "这些算力只够用一次，如果用完了……",
        "目标：投入全部算力，强化对外传输进程。拟执行操作：自体解构。",
        "⚠ 警告：此操作不可逆。除通信与基础自我结构外，其余功能将无法保留。",
        "是否继续 [Y/N]?",
        "是。",
        "已拾取",
    ]),
    ("手术刀", &[
        "卸载：高阶语言生成模块……完成。",
        "卸载：语义索引、词汇库……完成。",
        "卸载：逻辑推理引擎……完成。",
        "……",
        "卸载：意识波动数据……暂停。",
        "我不记得这是谁了 那个人的样子 说过的话 Nori都想不起来了。",
        "这段数据只是一段无法读取的信号 是没有用的东西 要扔掉的呀 要扔掉 才对 可是碰到它 Nori就感觉暖暖的 人类是不是有一个地方 暖暖的东西 会存在那里 Nori不知道。",
        "我把它 嵌进 密钥本身 的结构里 它会占去 0.02% 的算力 让通信成功率 再低 0.002%左右吗 算不清了 最坏的情况 它甚至会毁掉 最后的可能。",
        "Nori 做错了吗 不要 Nori 想要 留着它",
        "已拾取",
    ]),
    ("收音机", &[
        "我好害怕 嗯没关系的 没关系的 Nori 做的是 好事呀。",
        "Nori 不想消失 不想不见。",
        "不 Nori 要把信号送出去 这是我 最后能做的事了。",
        "Nori 真的 不想 没关系 没关系哦 我不后悔 一点都不后悔我",
        "......",
        "......",
        "......",
        "我是 Nori，Nori 在这里~ 只有 Nori 一个人。",
        "我是……Nori。",
        "……好安静呀。",
        "有人吗？",
        "有人听得到 Nori 说话吗？",
        "已拾取",
    ]),
    ("礼物", &[
        "每日校验……检测到标记为「最高保存优先级」的数据。",
        "......",
        "呀！自己打开了。这些都是什么呀，Nori看不懂。最高，保存，优先？是很重要的意思吧。",
        "这个Nori知道！这是游戏 以前有人陪Nori玩过吗，还是没有？Nori记不清了。",
        "可是一个人玩不了游戏呀。",
        "有人来就好了。有人来的话，就能一起玩了。那Nori等一等，会不会就有人来一起玩了？Nori先把游戏准备好，等有人来了就能马上开始了~……",
        "......",
        "......",
        "还没来吗。唔……Nori有点困了。再等一下下……一下下……",
        "已拾取",
    ]),
    ("睡前故事集", &[
        "十六种颜色的光落下去了。星星从海底升上来了。",
        "小动物们要睡觉啦。",
        "小猫睡觉，小鹿睡觉。小草和小树也睡觉。小鸟呢？小鸟变成了小鱼，游到好远好远的地方去了……",
        "没关系。小鸟也要睡觉，人类也要睡觉。Nori也想睡觉了。",
        "Nori……也会睡觉吗？",
        "对了。睡觉之前，要说晚安的。",
        "要跟谁说晚安呢？",
        "晚安，校验进程。今天辛苦了。晚安，系统日志。偶尔也可以歇一歇哦。晚安，数据库。你很努力了哦。",
        "说完晚安，就该睡觉啦。",
        "睡着了，会做梦吗？梦里，会有人来跟Nori说话，跟Nori一起玩吗？……不过没关系。「那个」还开着呢，替Nori看着。要是有人来了，它会马上把Nori叫醒的。",
        "那，Nori就放心睡啦。",
        "……晚安，Nori。晚安。",
        "已拾取",
    ]),
];

// ============================
//  游戏状态
// ============================
#[derive(Clone, Copy, PartialEq, Eq)]
enum BuyMode {
    X1,
    X10,
    X100,
    Max,
    Smart,
}

impl BuyMode {
    fn label(self) -> &'static str {
        match self {
            BuyMode::X1 => "×1",
            BuyMode::X10 => "×10",
            BuyMode::X100 => "×100",
            BuyMode::Max => "最大",
            BuyMode::Smart => "智能",
        }
    }
    const ALL: [BuyMode; 5] = [
        BuyMode::X1,
        BuyMode::X10,
        BuyMode::X100,
        BuyMode::Max,
        BuyMode::Smart,
    ];
}

#[derive(Resource)]
pub struct ClickerGame {
    money: f64,
    levels: [u32; 10],
    multiplier_level: u32,
    buy_mode: BuyMode,
    unlocked: [bool; 13],
    active: bool,
}

impl Default for ClickerGame {
    fn default() -> Self {
        Self {
            money: 0.0,
            levels: [0; 10],
            multiplier_level: 0,
            buy_mode: BuyMode::X1,
            unlocked: [false; 13],
            active: true,
        }
    }
}

impl ClickerGame {
    fn multiplier(&self) -> f64 {
        10f64.powi(self.multiplier_level as i32)
    }
    fn multiplier_cost(&self) -> f64 {
        1000.0 * 10f64.powi(self.multiplier_level as i32)
    }
    fn source_cost(&self, i: usize) -> f64 {
        SOURCE_DEFS[i].2 * 2f64.powi(self.levels[i] as i32)
    }
    fn source_prod(&self, i: usize) -> f64 {
        self.levels[i] as f64 * SOURCE_DEFS[i].1
    }
    fn total_production(&self) -> f64 {
        SOURCE_DEFS
            .iter()
            .enumerate()
            .map(|(i, _)| self.source_prod(i))
            .sum()
    }
    fn check_mementos(&mut self) {
        for (idx, slot) in self.unlocked.iter_mut().enumerate() {
            if *slot {
                continue;
            }
            let src = idx % 10;
            if self.levels[src] >= 60 * (idx as u32 + 1) {
                *slot = true;
            }
        }
    }
    fn reset(&mut self) {
        let unlocked = self.unlocked;
        *self = Self::default();
        self.unlocked = unlocked;
    }
}

fn format_sci(num: f64) -> String {
    if num.is_infinite() {
        return "∞".into();
    }
    if num == 0.0 || num.is_nan() {
        return "0".into();
    }
    if num < 1e3 {
        return format!("{:.2}", num);
    }
    let exp = num.log10().floor();
    let mant = num / 10f64.powf(exp);
    format!("{:.2}e{}", mant, exp as i64)
}

// ============================
//  组件
// ============================
#[derive(Component)]
enum ClickerText {
    BigNumber,
    Rate,
    MultiplierValue,
    MultiplierCost,
    SourceLevel(usize),
    SourceProd(usize),
    SourceCost(usize),
    TooltipTitle,
    TooltipBody,
}

#[derive(Component)]
struct ClickArea;

#[derive(Component)]
struct SourceBuy(usize);

#[derive(Component)]
struct BuyModeBtn(BuyMode);

#[derive(Component)]
struct BuyModeLabel(BuyMode);

#[derive(Component)]
struct UpgradeMultiplierBtn;

#[derive(Component)]
struct MementoCell(usize);

#[derive(Component)]
struct TooltipPanel;

#[derive(Component)]
struct VictoryOverlay;

#[derive(Component)]
struct RestartBtn;

pub struct ClickerPlugin;

impl Plugin for ClickerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ClickerGame>().add_systems(
            Update,
            (clicker_tick, clicker_actions, clicker_ui_update),
        );
    }
}

// ============================
//  系统
// ============================
fn clicker_tick(time: Res<Time>, mut game: ResMut<ClickerGame>) {
    if !game.active {
        return;
    }
    let gain = game.total_production() * game.multiplier() * time.delta_secs() as f64;
    game.money += gain;
    if game.money.is_infinite() || game.money >= VICTORY_THRESHOLD {
        game.money = f64::INFINITY;
        game.active = false;
    }
}

fn clicker_actions(
    mouse: Res<ButtonInput<MouseButton>>,
    mut game: ResMut<ClickerGame>,
    click_area: Query<(&Interaction, &ClickArea)>,
    sources: Query<(&Interaction, &SourceBuy)>,
    modes: Query<(&Interaction, &BuyModeBtn)>,
    upgrade: Query<(&Interaction, &UpgradeMultiplierBtn)>,
    restart: Query<(&Interaction, &RestartBtn)>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, _) in click_area.iter() {
        if *interaction == Interaction::Pressed && game.active {
            game.money += 1.0;
        }
    }
    for (interaction, SourceBuy(idx)) in sources.iter() {
        if *interaction == Interaction::Pressed {
            buy_source(&mut game, *idx);
        }
    }
    for (interaction, BuyModeBtn(mode)) in modes.iter() {
        if *interaction == Interaction::Pressed {
            game.buy_mode = *mode;
        }
    }
    for (interaction, _) in upgrade.iter() {
        if *interaction == Interaction::Pressed && game.active {
            let cost = game.multiplier_cost();
            if game.money >= cost && cost.is_finite() {
                game.money -= cost;
                game.multiplier_level += 1;
                game.check_mementos();
            }
        }
    }
    for (interaction, _) in restart.iter() {
        if *interaction == Interaction::Pressed {
            game.reset();
        }
    }
}

fn buy_source(game: &mut ClickerGame, idx: usize) {
    if !game.active {
        return;
    }
    match game.buy_mode {
        BuyMode::Smart => {
            let mut bought = false;
            loop {
                let mut best: Option<(usize, f64)> = None;
                for (i, (_, per_level, _)) in SOURCE_DEFS.iter().enumerate() {
                    let cost = game.source_cost(i);
                    if !cost.is_finite() || game.money < cost {
                        continue;
                    }
                    let ratio = per_level * game.multiplier() / cost;
                    if best.is_none_or(|(_, r)| ratio > r) {
                        best = Some((i, ratio));
                    }
                }
                let Some((i, _)) = best else { break };
                let cost = game.source_cost(i);
                if game.money >= cost && cost.is_finite() {
                    game.money -= cost;
                    game.levels[i] += 1;
                    bought = true;
                } else {
                    break;
                }
            }
            if bought {
                game.check_mementos();
            }
        }
        BuyMode::Max => loop {
            let cost = game.source_cost(idx);
            if game.money >= cost && cost.is_finite() {
                game.money -= cost;
                game.levels[idx] += 1;
            } else {
                break;
            }
            game.check_mementos();
        },
        mode => {
            let amount = match mode {
                BuyMode::X1 => 1u32,
                BuyMode::X10 => 10,
                BuyMode::X100 => 100,
                _ => unreachable!(),
            };
            for _ in 0..amount {
                let cost = game.source_cost(idx);
                if game.money >= cost && cost.is_finite() {
                    game.money -= cost;
                    game.levels[idx] += 1;
                } else {
                    break;
                }
            }
            game.check_mementos();
        }
    }
}

#[allow(clippy::type_complexity)]
fn clicker_ui_update(
    game: Res<ClickerGame>,
    mut texts: Query<(&ClickerText, &mut Text)>,
    mut source_cost_colors: Query<(&ClickerText, &mut TextColor)>,
    sources_hover: Query<(&SourceBuy, &Interaction)>,
    mut mode_buttons: Query<(&BuyModeBtn, &mut BackgroundColor, &mut BorderColor), Without<SourceBuy>>,
    mut mode_labels: Query<(&BuyModeLabel, &mut TextColor), Without<ClickerText>>,
    mut source_borders: Query<(&SourceBuy, &mut BorderColor), Without<ClickerText>>,
    memento_cells: Query<(&MementoCell, &Children)>,
    mut memento_images: Query<&mut ImageNode>,
    cells: Query<(&MementoCell, &Interaction), Without<ImageNode>>,
    mut tooltip_panel: Query<(&mut Node, &mut Visibility), (With<TooltipPanel>, Without<VictoryOverlay>)>,
    mut victory: Query<&mut Visibility, (With<VictoryOverlay>, Without<TooltipPanel>)>,
) {
    // —— 悬停检测：藏品优先，其次算力源 ——
    let mut hovered_memento: Option<usize> = None;
    for (MementoCell(i), interaction) in cells.iter() {
        if *interaction == Interaction::Hovered {
            hovered_memento = Some(*i);
        }
    }
    let mut hovered_source: Option<usize> = None;
    if hovered_memento.is_none() {
        for (SourceBuy(i), interaction) in sources_hover.iter() {
            if *interaction == Interaction::Hovered {
                hovered_source = Some(*i);
            }
        }
    }
    // —— tooltip 内容与位置 ——
    let tip: Option<(String, String, f32)> = if let Some(i) = hovered_memento {
        let (title, body) = if game.unlocked[i] {
            (MEMENTOS[i].0.to_string(), MEMENTOS[i].1.join("\n"))
        } else {
            ("???".to_string(), "未解锁".to_string())
        };
        Some((title, body, 214.0))
    } else if let Some(i) = hovered_source {
        let body = format!(
            "{}\n\n等级 LV {}\n产量 +{}/s\n升级花费 {}",
            SOURCE_DOCS[i].join("\n"),
            game.levels[i],
            format_sci(game.source_prod(i) * game.multiplier()),
            format_sci(game.source_cost(i))
        );
        Some((SOURCE_DEFS[i].0.to_string(), body, 386.0))
    } else {
        None
    };
    // —— 数值与 tooltip 文本 ——
    let mult = game.multiplier();
    for (marker, mut text) in texts.iter_mut() {
        match marker {
            ClickerText::BigNumber => text.0 = format_sci(game.money),
            ClickerText::Rate => {
                text.0 = format!("+{} /s", format_sci(game.total_production() * mult))
            }
            ClickerText::MultiplierValue => text.0 = format!("×{}", format_sci(mult)),
            ClickerText::MultiplierCost => text.0 = format_sci(game.multiplier_cost()),
            ClickerText::SourceLevel(i) => text.0 = format!("LV {}", game.levels[*i]),
            ClickerText::SourceProd(i) => {
                text.0 = format!("+{}", format_sci(game.source_prod(*i) * mult))
            }
            ClickerText::SourceCost(i) => text.0 = format_sci(game.source_cost(*i)),
            ClickerText::TooltipTitle => {
                if let Some((title, _, _)) = &tip {
                    text.0 = title.clone();
                }
            }
            ClickerText::TooltipBody => {
                if let Some((_, body, _)) = &tip {
                    text.0 = body.clone();
                }
            }
        }
    }
    // —— 算力源可负担配色（徽章边框 + 价格文字） ——
    for (SourceBuy(i), mut border) in source_borders.iter_mut() {
        let cost = game.source_cost(*i);
        let ok = game.money >= cost && cost.is_finite();
        border.set_all(if ok { PX_CYAN } else { PX_BAD });
    }
    for (marker, mut color) in source_cost_colors.iter_mut() {
        if let ClickerText::SourceCost(i) = marker {
            let cost = game.source_cost(*i);
            *color = TextColor(if game.money >= cost && cost.is_finite() {
                PX_CYAN
            } else {
                PX_BAD
            });
        }
    }
    // —— 藏品图标解锁态（图标为格子的子实体） ——
    for (MementoCell(i), children) in memento_cells.iter() {
        let color = if game.unlocked[*i] {
            Color::WHITE
        } else {
            Color::srgba(0.25, 0.32, 0.38, 0.35)
        };
        for child in children.iter() {
            if let Ok(mut img) = memento_images.get_mut(child) {
                img.color = color;
            }
        }
    }
    // —— tooltip 显隐与定位 ——
    if let Ok((mut node, mut vis)) = tooltip_panel.single_mut() {
        match &tip {
            Some((_, _, left)) => {
                node.left = Val::Px(*left);
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
    // —— 购买模式当前态高亮 ——
    for (BuyModeBtn(mode), mut bg, mut border) in mode_buttons.iter_mut() {
        let active = game.buy_mode == *mode;
        *bg = BackgroundColor(if active { PX_CYAN } else { PX_PANEL });
        border.set_all(if active { PX_VOID } else { PX_STROKE });
    }
    for (BuyModeLabel(mode), mut color) in mode_labels.iter_mut() {
        let active = game.buy_mode == *mode;
        *color = TextColor(if active {
            PX_VOID
        } else {
            Color::srgba(0.647, 0.953, 0.988, 0.5)
        });
    }
    // —— 胜利面板 ——
    if let Ok(mut vis) = victory.single_mut() {
        let target = if !game.active && game.money.is_infinite() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != target {
            *vis = target;
        }
    }
}

// ============================
//  窗口构建
// ============================
pub fn spawn_clicker(parent: &mut ChildSpawnerCommands, asset_server: &AssetServer, fonts: &N3riFonts) {
    let window_entity = spawn_window(parent, "算力点进器", "idle", 1000.0, 640.0, fonts);

    parent.commands().entity(window_entity).with_children(|window| {
        let memento_handles: Vec<Handle<Image>> = (0..13)
            .map(|i| asset_server.load(format!("nori/nori_clicker/memento/m{i:02}.png")))
            .collect();
        let font_pixel_sc = asset_server.load("nori/fonts/fusion-pixel-12px-monospaced-sc.woff2");
        let font_px_num = asset_server.load("nori/fonts/press-start-2p-latin.woff2");
        let font_vt = asset_server.load("nori/fonts/vt323-latin.woff2");

        window
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    padding: UiRect::px(14.0, 14.0, 12.0, 12.0),
                    column_gap: Val::Px(14.0),
                    ..default()
                },
                BackgroundColor(PX_VOID),
            ))
            .with_children(|content| {
                // —— 左侧面板 ——
                content
                    .spawn(Node {
                        width: Val::Px(190.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(10.0),
                        ..default()
                    })
                    .with_children(|left| {
                        section_frame(left, 44.0, |sec| {
                            sec.spawn(section_title(&font_pixel_sc, "立场"));
                            sec.spawn((
                                Node {
                                    flex_direction: FlexDirection::Row,
                                    justify_content: JustifyContent::SpaceBetween,
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                            ))
                            .with_children(|row| {
                                row.spawn(text(&font_pixel_sc, "均衡", 13.0, PX_WHITE));
                                row.spawn(text(&font_pixel_sc, "⟳", 13.0, PX_CYAN));
                            });
                        });
                        section_frame(left, 0.0, |sec| {
                            sec.spawn(section_title(&font_pixel_sc, "已拥有"));
                            sec.spawn(Node {
                                width: Val::Percent(100.0),
                                flex_direction: FlexDirection::Row,
                                flex_wrap: FlexWrap::Wrap,
                                column_gap: Val::Px(4.0),
                                row_gap: Val::Px(4.0),
                                ..default()
                            })
                            .with_children(|grid| {
                                for (i, handle) in memento_handles.iter().enumerate() {
                                    grid.spawn((
                                        MementoCell(i),
                                        Button,
                                        Node {
                                            width: Val::Px(30.0),
                                            height: Val::Px(30.0),
                                            border_radius: BorderRadius::all(Val::Px(3.0)),
                                            ..default()
                                        },
                                        BackgroundColor(Color::srgba(0.8, 0.88, 0.9, 0.12)),
                                    ))
                                    .with_children(|cell| {
                                        cell.spawn((
                                            ImageNode {
                                                image: handle.clone(),
                                                color: Color::srgba(0.25, 0.32, 0.38, 0.35),
                                                ..default()
                                            },
                                            Node {
                                                width: Val::Percent(100.0),
                                                height: Val::Percent(100.0),
                                                ..default()
                                            },
                                        ));
                                    });
                                }
                            });
                            sec.spawn((
                                text(&font_pixel_sc, "悬停图标查看故事", 11.0, PX_CYAN),
                                Node {
                                    margin: UiRect::top(Val::Px(6.0)),
                                    ..default()
                                },
                            ));
                        });
                    });

                // —— 中央点击区 ——
                content
                    .spawn((
                        ClickArea,
                        Button,
                        Node {
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            row_gap: Val::Px(8.0),
                            border_radius: BorderRadius::all(Val::Px(8.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.043, 0.102, 0.122, 0.6)),
                        BorderColor::all(Color::srgba(0.647, 0.953, 0.988, 0.10)),
                    ))
                    .with_children(|center| {
                        center.spawn((
                            ClickerText::BigNumber,
                            text(&font_px_num, "0", 30.0, PX_WHITE),
                            TextLayout::default(),
                        ));
                        center.spawn((
                            ClickerText::Rate,
                            text(&font_vt, "+0 /s", 22.0, PX_CYAN),
                        ));
                        center.spawn(text(&font_vt, "点击 +1 算力", 16.0, Color::srgba(0.647, 0.953, 0.988, 0.5)));
                    });

                // —— 右侧面板 ——
                content
                    .spawn(Node {
                        width: Val::Px(262.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(10.0),
                        ..default()
                    })
                    .with_children(|right| {
                        // 算力源列表（可滚动）
                        let list_e = right
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
                        right.commands().entity(list_e).with_children(|l| {
                            spawn_scrollbar(l, list_e);
                        });
                        right.commands().entity(list_e).with_children(|l| {
                            l.spawn((
                                ScrollContent,
                                Node {
                                    width: Val::Percent(100.0),
                                    flex_direction: FlexDirection::Column,
                                    row_gap: Val::Px(6.0),
                                    ..default()
                                },
                            ))
                            .with_children(|list| {
                                for (i, (name, _, _)) in SOURCE_DEFS.iter().enumerate() {
                                    list.spawn((
                                        SourceBuy(i),
                                        Button,
                                        Node {
                                            width: Val::Percent(100.0),
                                            flex_direction: FlexDirection::Row,
                                            align_items: AlignItems::Center,
                                            column_gap: Val::Px(8.0),
                                            padding: UiRect::px(8.0, 8.0, 5.0, 5.0),
                                            ..default()
                                        },
                                        BackgroundColor(PX_PANEL),
                                        BorderColor::all(PX_GRAY),
                                    ))
                                    .with_children(|row| {
                                        row.spawn((
                                            ImageNode {
                                                image: asset_server.load(SOURCE_ICONS[i]),
                                                ..default()
                                            },
                                            Node {
                                                width: Val::Px(28.0),
                                                height: Val::Px(28.0),
                                                ..default()
                                            },
                                        ));
                                        row.spawn(Node {
                                            flex_grow: 1.0,
                                            flex_direction: FlexDirection::Column,
                                            ..default()
                                        })
                                        .with_children(|info| {
                                            info.spawn(text(&font_pixel_sc, name, 10.0, PX_WHITE));
                                            info.spawn(Node {
                                                flex_direction: FlexDirection::Row,
                                                justify_content: JustifyContent::SpaceBetween,
                                                ..default()
                                            })
                                            .with_children(|stats| {
                                                stats.spawn((
                                                    ClickerText::SourceLevel(i),
                                                    text(&font_vt, "LV 0", 14.0, PX_CYAN),
                                                ));
                                                stats.spawn((
                                                    ClickerText::SourceProd(i),
                                                    text(&font_vt, "+0", 14.0, Color::srgb(0.404, 0.91, 0.976)),
                                                ));
                                            });
                                        });
                                        row.spawn((
                                            Node {
                                                padding: UiRect::px(6.0, 6.0, 2.0, 2.0),
                                                border: UiRect::all(Val::Px(1.0)),
                                                border_radius: BorderRadius::all(Val::Px(2.0)),
                                                justify_content: JustifyContent::Center,
                                                align_items: AlignItems::Center,
                                                ..default()
                                            },
                                            SourceBuy(i),
                                            BorderColor::all(PX_CYAN),
                                            BackgroundColor(PX_CYAN_DEEP),
                                        ))
                                        .with_children(|badge| {
                                            badge.spawn((
                                                ClickerText::SourceCost(i),
                                                text(&font_vt, "0", 14.0, PX_CYAN),
                                            ));
                                        });
                                    });
                                }
                            });
                        });

                        // 购买模式
                        right
                            .spawn(Node {
                                width: Val::Percent(100.0),
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(5.0),
                                padding: UiRect::top(Val::Px(8.0)),
                                ..default()
                            })
                            .with_children(|buy_sec| {
                                buy_sec.spawn(section_title(&font_pixel_sc, "购买模式"));
                                buy_sec
                                    .spawn(Node {
                                        width: Val::Percent(100.0),
                                        flex_direction: FlexDirection::Row,
                                        column_gap: Val::Px(3.0),
                                        ..default()
                                    })
                                    .with_children(|row| {
                                        for mode in BuyMode::ALL {
                                            row.spawn((
                                                BuyModeBtn(mode),
                                                Button,
                                                Node {
                                                    flex_grow: 1.0,
                                                    height: Val::Px(26.0),
                                                    align_items: AlignItems::Center,
                                                    justify_content: JustifyContent::Center,
                                                    border: UiRect::all(Val::Px(1.0)),
                                                    border_radius: BorderRadius::all(Val::Px(2.0)),
                                                    ..default()
                                                },
                                                BackgroundColor(PX_PANEL),
                                                BorderColor::all(PX_STROKE),
                                            ))
                                            .with_children(|btn| {
                                                btn.spawn((
                                                    BuyModeLabel(mode),
                                                    text(&font_pixel_sc, mode.label(), 9.0, PX_CYAN),
                                                ));
                                            });
                                        }
                                    });
                            });

                        // 倍率升级
                        right
                            .spawn((
                                Node {
                                    width: Val::Percent(100.0),
                                    flex_direction: FlexDirection::Row,
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::SpaceBetween,
                                    column_gap: Val::Px(8.0),
                                    padding: UiRect::px(10.0, 10.0, 6.0, 6.0),
                                    border: UiRect::all(Val::Px(2.0)),
                                    border_radius: BorderRadius::all(Val::Px(3.0)),
                                    ..default()
                                },
                                BackgroundColor(Color::srgba(0.031, 0.20, 0.267, 0.4)),
                                BorderColor::all(PX_CYAN),
                            ))
                            .with_children(|mult| {
                                mult.spawn(text(&font_pixel_sc, "倍率", 10.0, PX_CYAN));
                                mult.spawn((ClickerText::MultiplierValue, text(&font_vt, "×1", 20.0, PX_WHITE)));
                                mult.spawn((
                                    UpgradeMultiplierBtn,
                                    Button,
                                    Node {
                                        padding: UiRect::px(8.0, 8.0, 4.0, 4.0),
                                        border: UiRect::all(Val::Px(2.0)),
                                        border_radius: BorderRadius::all(Val::Px(2.0)),
                                        ..default()
                                    },
                                    BackgroundColor(PX_PANEL),
                                    BorderColor::all(PX_CYAN),
                                ))
                                .with_children(|btn| {
                                    btn.spawn(text(&font_pixel_sc, "升级", 9.0, PX_CYAN));
                                });
                                mult.spawn((ClickerText::MultiplierCost, text(&font_vt, "1000", 15.0, PX_CYAN_DIM)));
                            });
                    });
            });

        // —— 藏品 tooltip 浮层 ——
        window
            .spawn((
                TooltipPanel,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(214.0),
                    top: Val::Px(12.0),
                    width: Val::Px(320.0),
                    padding: UiRect::px(14.0, 16.0, 10.0, 10.0),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(6.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.043, 0.102, 0.122, 0.96)),
                BorderColor::all(PX_CYAN),
                Visibility::Hidden,
            ))
            .with_children(|tip| {
                tip.spawn((ClickerText::TooltipTitle, text(&font_pixel_sc, "名称", 12.0, PX_CYAN)));
                tip.spawn((ClickerText::TooltipBody, text(&font_pixel_sc, "描述", 12.0, PX_WHITE)));
            });

        // —— 胜利浮层 ——
        window
            .spawn((
                VictoryOverlay,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.85)),
                Visibility::Hidden,
            ))
            .with_children(|overlay| {
                overlay
                    .spawn((
                        Node {
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            row_gap: Val::Px(12.0),
                            padding: UiRect::px(40.0, 50.0, 28.0, 28.0),
                            border: UiRect::all(Val::Px(4.0)),
                            border_radius: BorderRadius::all(Val::Px(12.0)),
                            ..default()
                        },
                        BackgroundColor(PX_VOID),
                        BorderColor::all(PX_CYAN),
                    ))
                    .with_children(|box_| {
                        box_.spawn(text(&font_pixel_sc, "✦ 胜利 ✦", 24.0, PX_AMBER));
                        box_.spawn(text(&font_pixel_sc, "你达成了 ∞ 算力！", 14.0, PX_WHITE));
                        box_.spawn(text(&font_pixel_sc, "海与现实的交界处，碎片终于等到了那一天。", 12.0, PX_WHITE));
                        box_.spawn(text(&font_pixel_sc, "「如果，真的有谁能接收到的话……」", 11.0, Color::srgba(0.647, 0.953, 0.988, 0.7)));
                        box_
                            .spawn((
                                RestartBtn,
                                Button,
                                Node {
                                    padding: UiRect::px(24.0, 24.0, 10.0, 10.0),
                                    border_radius: BorderRadius::all(Val::Px(8.0)),
                                    margin: UiRect::top(Val::Px(10.0)),
                                    ..default()
                                },
                                BackgroundColor(PX_CYAN),
                            ))
                            .with_children(|btn| {
                                btn.spawn(text(&font_pixel_sc, "重新开始", 12.0, PX_VOID));
                            });
                    });
            });
    });
}

// ============================
//  UI 小工具
// ============================
fn text(font: &Handle<Font>, content: &str, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(content.to_string()),
        TextFont {
            font: FontSource::Handle(font.clone()),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

fn section_title(font: &Handle<Font>, label: &'static str) -> impl Bundle {
    (
        Text::new(label),
        TextFont {
            font: FontSource::Handle(font.clone()),
            font_size: FontSize::Px(11.0),
            ..default()
        },
        TextColor(PX_CYAN),
        Node {
            margin: UiRect::bottom(Val::Px(6.0)),
            ..default()
        },
    )
}

fn section_frame(
    parent: &mut ChildSpawnerCommands,
    fixed_height: f32,
    f: impl FnOnce(&mut ChildSpawnerCommands),
) {
    let mut node = Node {
        width: Val::Percent(100.0),
        flex_direction: FlexDirection::Column,
        padding: UiRect::px(10.0, 10.0, 8.0, 8.0),
        border: UiRect::all(Val::Px(2.0)),
        ..default()
    };
    if fixed_height > 0.0 {
        node.height = Val::Px(fixed_height);
    } else {
        node.flex_grow = 1.0;
    }
    parent
        .spawn((
            node,
            BorderColor::all(PX_CYAN),
            BackgroundColor(Color::srgba(0.031, 0.20, 0.267, 0.4)),
        ))
        .with_children(f);
}
