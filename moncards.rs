//! 监视器呈现面（WP-206 · B-2002）：四卡与状态机实时一致——"诚实界面"的样板件。
//!
//! MD2 篇 20.2：监视器首页四卡（存储、网络、音频、兼容）即诊断中心的健康
//! 总表同源，卡上状态色加一行人话（降级原因直接来自服务的降级声明）。
//! 进程页：结束进程是受控动作（二次确认，系统关键进程拒杀并解释——哪些
//! 算关键在启动表声明）。温度与风扇：阶段 3 接入，先占位明示"阶段 3"——
//! 诚实呈现做不到与呈现假数据之间，永远选前者。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 四卡与服务状态机
// ---------------------------------------------------------------------------

pub const CARD_COUNT: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardKind {
    Storage,
    Net,
    Audio,
    Compat,
}

pub const ALL_CARDS: [CardKind; CARD_COUNT] =
    [CardKind::Storage, CardKind::Net, CardKind::Audio, CardKind::Compat];

/// 服务状态三态（与诊断中心健康总表同源——MD1 第 36.2 节）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SvcState {
    Normal,
    Degraded,
    Down,
}

/// 卡面颜色（呈现面枚举，与状态一一映射——不许呈现面自造颜色规则）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardColor {
    Green,
    Yellow,
    Red,
}

/// 卡视图：状态色 + 一行人话。人话=服务降级声明原文（直通不转写）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CardView {
    pub color: CardColor,
    pub note: &'static str,
}

/// 服务声明：状态 + 人话原因（降级原因直接来自服务——呈现面无权编写）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ServiceDecl {
    pub state: SvcState,
    pub reason: &'static str,
}

impl ServiceDecl {
    pub const fn normal() -> Self {
        ServiceDecl { state: SvcState::Normal, reason: "运行正常" }
    }

    pub fn view(&self) -> CardView {
        let color = match self.state {
            SvcState::Normal => CardColor::Green,
            SvcState::Degraded => CardColor::Yellow,
            SvcState::Down => CardColor::Red,
        };
        CardView { color, note: self.reason }
    }
}

/// 四卡状态机：声明面（服务写）与呈现面（卡面读）分离，恒等式在此对账。
pub struct CardBoard {
    decls: [ServiceDecl; CARD_COUNT],
    /// 每卡一条历史环（HISTORY_CAP 槽），共 CARD_COUNT × HISTORY_CAP。
    history: [HistorySlot; CARD_COUNT * HISTORY_CAP],
}

const HISTORY_CAP: usize = 10; // 最近十次状态变化（诊断中心 23.3 展开面）

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HistorySlot {
    pub used: bool,
    pub from: SvcState,
    pub to: SvcState,
    pub seq: u64,
}

impl CardBoard {
    pub fn new() -> Self {
        CardBoard {
            decls: [ServiceDecl::normal(); CARD_COUNT],
            history: [HistorySlot { used: false, from: SvcState::Normal, to: SvcState::Normal, seq: 0 }; CARD_COUNT * HISTORY_CAP],
        }
    }

    /// 服务声明更新：呈现面立即跟随（实时一致——没有轮询延迟语义位）。
    /// 状态变化才记历史（同态刷新不记——记录面与变化粒度对齐）。
    pub fn declare(&mut self, kind: CardKind, decl: ServiceDecl, seq: u64) -> bool {
        let idx = match kind {
            CardKind::Storage => 0,
            CardKind::Net => 1,
            CardKind::Audio => 2,
            CardKind::Compat => 3,
        };
        let changed = self.decls[idx].state != decl.state;
        self.decls[idx] = decl;
        if changed {
            self.push_history(idx, decl.state, seq);
        }
        changed
    }

    fn push_history(&mut self, idx: usize, to: SvcState, seq: u64) {
        let from = if self.history[idx * HISTORY_CAP].used {
            self.history[idx * HISTORY_CAP].to
        } else {
            SvcState::Normal
        };
        // 环形：满 10 后覆盖最旧（数组整体后移一格，首位进新记录）。
        let mut i = HISTORY_CAP - 1;
        while i > 0 {
            self.history[idx * HISTORY_CAP + i] = self.history[idx * HISTORY_CAP + i - 1];
            i -= 1;
        }
        self.history[idx * HISTORY_CAP] = HistorySlot { used: true, from, to, seq };
    }

    fn history_slot(&self, idx: usize, h: usize) -> &HistorySlot {
        &self.history[idx * HISTORY_CAP + h]
    }

    /// 卡面读数：呈现面无权自造，原样取声明视图。
    pub fn card_view(&self, kind: CardKind) -> CardView {
        let idx = match kind {
            CardKind::Storage => 0,
            CardKind::Net => 1,
            CardKind::Audio => 2,
            CardKind::Compat => 3,
        };
        self.decls[idx].view()
    }

    pub fn decl(&self, kind: CardKind) -> ServiceDecl {
        let idx = match kind {
            CardKind::Storage => 0,
            CardKind::Net => 1,
            CardKind::Audio => 2,
            CardKind::Compat => 3,
        };
        self.decls[idx]
    }

    /// 降级历史：最近十次状态变化（新→旧），供诊断中心每卡展开。
    pub fn history(&self, kind: CardKind) -> [HistorySlot; HISTORY_CAP] {
        let idx = match kind {
            CardKind::Storage => 0,
            CardKind::Net => 1,
            CardKind::Audio => 2,
            CardKind::Compat => 3,
        };
        let mut out = [HistorySlot { used: false, from: SvcState::Normal, to: SvcState::Normal, seq: 0 }; HISTORY_CAP];
        let mut h = 0;
        while h < HISTORY_CAP {
            out[h] = *self.history_slot(idx, h);
            h += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 进程页：结束进程受控动作（二次确认 + 关键进程拒杀并解释）
// ---------------------------------------------------------------------------

/// 系统关键进程清单（启动表声明——哪些算关键不是呈现面说了算）。
pub const KEY_PROCS: [&[u8]; 4] = [b"vxinit", b"vxcomp", b"vxstore", b"vxaudiod"];

pub const PID_CAP: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProcRow {
    pub pid: u32,
    pub name: [u8; 16],
    pub name_len: usize,
    pub cpu_permille: u64,
}

impl ProcRow {
    pub fn name_bytes(&self) -> &[u8] {
        &self.name[..self.name_len.min(16)]
    }
}

/// 结束进程结果：受控动作三出口（拒杀解释/缺确认拒/允许）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KillVerdict {
    /// 系统关键进程：拒杀 + 解释（解释非空是人话义务）。
    Protected(&'static str),
    /// 未带二次确认令牌：拒绝。
    NeedsConfirm,
    /// 允许结束（二次确认 + 非关键）。
    Allowed,
}

/// 结束进程裁决：ack=二次确认令牌（None 即未确认）。
pub fn verdict_kill(row: &ProcRow, ack: Option<()>) -> KillVerdict {
    let name = row.name_bytes();
    for k in KEY_PROCS.iter() {
        if name == *k {
            return KillVerdict::Protected(
                "系统关键进程，结束将导致对应子系统失能；如需处置请用服务页的降级恢复",
            );
        }
    }
    match ack {
        None => KillVerdict::NeedsConfirm,
        Some(()) => KillVerdict::Allowed,
    }
}

// ---------------------------------------------------------------------------
// 帧率页开发者门 + 温度卡占位
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UiMode {
    User,
    Developer,
}

/// 帧率页对普通用户隐藏在开发者模式后（开发向仪表）。
pub fn fps_panel_visible(mode: UiMode) -> bool {
    mode == UiMode::Developer
}

/// 温度风扇卡呈现：阶段 3 前永远占位明示——不呈现任何编造数字。
pub const THERMAL_CARD_NOTE: &str = "温度与风扇：阶段 3 接入（EC 通道）";

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ThermalCardView {
    pub placeholder: bool,
    pub note: &'static str,
}

/// 温度卡视图：恒占位（placeholder==true 且无数值字段——类型面不存在
/// 可被填假的数值槽）。
pub fn thermal_card_view() -> ThermalCardView {
    ThermalCardView { placeholder: true, note: THERMAL_CARD_NOTE }
}

// ---------------------------------------------------------------------------
// CheckSet（B-2002 · 9 项）
// ---------------------------------------------------------------------------

pub fn run_moncards_checks() -> CheckSet {
    let mut set = CheckSet::new("B-2002 降级原因呈现");
    // 1. 四卡枚举齐。
    set.add(
        "B-2002 四卡枚举齐",
        ALL_CARDS.len() == CARD_COUNT
            && ALL_CARDS[0] == CardKind::Storage
            && ALL_CARDS[3] == CardKind::Compat,
        "存储/网络/音频/兼容四卡齐备",
    );
    // 2. 状态机三态与卡面颜色映射（绿/黄/红一一对应）。
    let v_n = ServiceDecl::normal().view();
    let v_d = ServiceDecl { state: SvcState::Degraded, reason: "缓存写回延迟升高" }.view();
    let v_x = ServiceDecl { state: SvcState::Down, reason: "块通道失联" }.view();
    set.add(
        "B-2002 三态颜色映射",
        v_n.color == CardColor::Green && v_d.color == CardColor::Yellow && v_x.color == CardColor::Red,
        "正常绿/降级黄/不可用红，映射由声明面决定",
    );
    // 3. 卡面实时一致：声明一变，卡面立即跟随（恒等式）。
    let mut board = CardBoard::new();
    let v0 = board.card_view(CardKind::Net);
    board.declare(CardKind::Net, ServiceDecl { state: SvcState::Degraded, reason: "DNS 缓存污染，已切换备用解析" }, 1);
    let v1 = board.card_view(CardKind::Net);
    let d1 = board.decl(CardKind::Net);
    set.add(
        "B-2002 卡面实时一致",
        v0.color == CardColor::Green
            && v1.color == CardColor::Yellow
            && v1.note == d1.reason
            && d1.state == SvcState::Degraded,
        "声明面一变卡面即变，卡面文案==服务声明原文（直通不转写）",
    );
    // 4. 人话降级原因直通（非空且非呈现面编写）。
    set.add(
        "B-2002 降级原因直通",
        !v1.note.is_empty() && v1.note == "DNS 缓存污染，已切换备用解析",
        "一行人话来自服务降级声明，呈现面无权编写原因",
    );
    // 5. 降级历史最近十次：11 次变化后历史长度恰 10，最旧被挤出。
    let mut board5 = CardBoard::new();
    let seqs = [SvcState::Degraded, SvcState::Normal, SvcState::Degraded, SvcState::Normal, SvcState::Degraded,
        SvcState::Normal, SvcState::Degraded, SvcState::Normal, SvcState::Degraded, SvcState::Normal, SvcState::Down];
    let mut i = 0;
    while i < seqs.len() {
        board5.declare(CardKind::Audio, ServiceDecl { state: seqs[i], reason: "对练" }, (i + 1) as u64);
        i += 1;
    }
    let hist5 = board5.history(CardKind::Audio);
    let used_cnt = hist5.iter().filter(|h| h.used).count();
    let newest = hist5[0];
    set.add(
        "B-2002 降级历史最近十次",
        used_cnt == HISTORY_CAP
            && newest.used
            && newest.to == SvcState::Down
            && newest.seq == 11,
        "历史环容量 10，最新变化在首位（新→旧），第 11 次挤出最旧",
    );
    // 6. 历史只记变化：同态刷新不产生新历史。
    let mut board6 = CardBoard::new();
    board6.declare(CardKind::Storage, ServiceDecl { state: SvcState::Degraded, reason: "r1" }, 1);
    let n1 = board6.history(CardKind::Storage).iter().filter(|h| h.used).count();
    board6.declare(CardKind::Storage, ServiceDecl { state: SvcState::Degraded, reason: "r2" }, 2);
    let n2 = board6.history(CardKind::Storage).iter().filter(|h| h.used).count();
    set.add(
        "B-2002 历史只记变化",
        n1 == 1 && n2 == 1,
        "同态刷新（状态未变仅原因文案变）不产生新历史——记录面与变化粒度对齐",
    );
    // 7. 结束进程受控：无二次确认拒绝。
    let p7 = ProcRow { pid: 42, name: make_name(b"vxedit"), name_len: 6, cpu_permille: 5 };
    set.add(
        "B-2002 结束进程二次确认",
        verdict_kill(&p7, None) == KillVerdict::NeedsConfirm,
        "无确认令牌的结束请求一律拒绝",
    );
    // 8. 系统关键进程拒杀并解释（启动表声明清单，确认与否都拒）。
    let p8 = ProcRow { pid: 1, name: make_name(b"vxcomp"), name_len: 6, cpu_permille: 30 };
    match verdict_kill(&p8, Some(())) {
        KillVerdict::Protected(note) => {
            set.add(
                "B-2002 关键进程拒杀解释",
                !note.is_empty() && verdict_kill(&p8, None) != KillVerdict::Allowed,
                "关键进程无论是否确认都拒杀，且给出解释人话",
            );
        }
        _ => set.add("B-2002 关键进程拒杀解释", false, "关键进程必须走 Protected 出口"),
    }
    // 9. 温度风扇占位明示（诚实呈现）+ 帧率页开发者门。
    let tv = thermal_card_view();
    set.add(
        "B-2002 占位诚实与开发者门",
        tv.placeholder && tv.note == THERMAL_CARD_NOTE && !fps_panel_visible(UiMode::User) && fps_panel_visible(UiMode::Developer),
        "温度卡恒占位无数值槽；帧率页只在开发者模式可见",
    );
    set
}

fn make_name(src: &[u8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    let n = src.len().min(16);
    out[..n].copy_from_slice(&src[..n]);
    out
}

// ---------------------------------------------------------------------------
// 单测（fa02 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fa02_realtime_consistency() {
        // 声明面与呈现面实时一致：三卡各自独立跟随。
        let mut board = CardBoard::new();
        board.declare(CardKind::Storage, ServiceDecl { state: SvcState::Down, reason: "U 盘失联" }, 1);
        board.declare(CardKind::Audio, ServiceDecl { state: SvcState::Normal, reason: "运行正常" }, 2);
        board.declare(CardKind::Compat, ServiceDecl { state: SvcState::Degraded, reason: "Wine GL 降档" }, 3);
        assert_eq!(board.card_view(CardKind::Storage).color, CardColor::Red);
        assert_eq!(board.card_view(CardKind::Audio).color, CardColor::Green);
        assert_eq!(board.card_view(CardKind::Compat).color, CardColor::Yellow);
        assert_eq!(board.card_view(CardKind::Net).color, CardColor::Green, "未声明的卡保持初始正常态");
    }

    #[test]
    fn fa02_history_cap() {
        let mut board = CardBoard::new();
        let mut i = 0u64;
        let cycle = [SvcState::Degraded, SvcState::Normal];
        while i < 25 {
            board.declare(CardKind::Net, ServiceDecl { state: cycle[(i % 2) as usize], reason: "对练" }, i + 1);
            i += 1;
        }
        let hist = board.history(CardKind::Net);
        assert_eq!(hist.iter().filter(|h| h.used).count(), HISTORY_CAP);
        assert_eq!(hist[0].seq, 25, "最新在首位");
        assert_eq!(hist[9].seq, 16, "容量 10，seq16 之后被挤出");
    }

    #[test]
    fn fa02_kill_control() {
        let key = ProcRow { pid: 1, name: make_name(b"vxinit"), name_len: 6, cpu_permille: 1 };
        let user = ProcRow { pid: 99, name: make_name(b"vxedit"), name_len: 6, cpu_permille: 2 };
        assert!(matches!(verdict_kill(&key, Some(())), KillVerdict::Protected(_)));
        assert_eq!(verdict_kill(&user, None), KillVerdict::NeedsConfirm);
        assert_eq!(verdict_kill(&user, Some(())), KillVerdict::Allowed);
    }

    #[test]
    fn fa02_honest_placeholder() {
        let tv = thermal_card_view();
        assert!(tv.placeholder);
        assert_eq!(tv.note, THERMAL_CARD_NOTE);
        assert!(!fps_panel_visible(UiMode::User));
        assert!(fps_panel_visible(UiMode::Developer));
    }
}
