//! UNX-F49023 dead key 组合 40 组全对执行器 + UNX-F49024 LED 三灯状态机 · 0x71B80
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B12 族第二槽；任务板 J1 引用按原文保留）：
//!   UNX-F49023 dead key 组合 40 组全对执行器（B02 表的判据执行化）:
//!          ① 纯功能代码 ≥360 行；
//!          ② 判据原文：UNX-F49024-J1 三灯 ×10^2 双向同步一致。
//!   UNX-F49024 LED 三灯状态机（Caps/Num/Scroll 与内核态双向同步收口）:
//!          ① 纯功能代码 ≥340 行；
//!          ② 判据原文：UNX-F49025-J1 失败注入 ×10 零阻塞账可查。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（dead key 组合）：死键身份 → 表自洽 → 待组合态 → 基字匹配 →
//! 产出落账 → 中断落账。
//! 死键身份先于表：死键的物理身份是 (扫描码, Shift)，不是扫描码。
//! grave 与 tilde 共用 0x29，只靠 Shift 区分；若只按扫描码认键，按 ` 会
//! 同时命中两条死键，组合结果取决于表序而不是用户按了什么。
//! 表自洽先于执行：每条组合必须是「死键 ∈ 全集、基字 ∈ a..z、产出非 ASCII
//! 且不等于基字、同一 (死键, 基字) 只出现一次」。表坏时执行出来的每个组合
//! 都只是带毒读取，先自审才能把「表病」与「组合不成立」分开。
//! 待组合态先于基字：死键按下后必须进入 pending。没有 pending 时字母原样
//! 透传，有 pending 时字母才参与组合——顺序反过来会让每个普通字母都被
//! 当作潜在组合去查表，未命中就报错，日常打字立刻失败。
//! 中断后于产出：任何非基字按键在有 pending 时取消组合（计 aborted），
//! 但不能取消已经产出的组合。先产出后中断，账本里 composed 与 aborted
//! 才是互斥的两类，守恒式 base_makes == composed + aborted + passthrough
//! 才有意义。
//! 闸门次序（LED 三灯）：位合法 → 三方比较 → 单向动作 → 回读 → 落账。
//! 位合法先于比较：Caps/Num/Scroll 只占 bit0/bit1/bit2，高于 0x07 的状态
//! 是坏数据；先比较的话，坏位会以「有变化」的身份进入同步动作，把硬件
//! 写成未定义的灯态。
//! 三方比较先于动作：host（内核期望）、device（设备实报）、last（上次同步
//! 基线）三者才能判定「谁变了」。只比 host 与 device 无法区分「host 刚
//! 改」和「device 被外部改」，双向同步会退化成单向覆盖。
//! 回读后于写入：写入成功不等于灯态成立，必须回读比对；回读不一致按
//! 失败记账并保留 host 期望，绝不把「写过了」当成「同步了」。
//! 失败不阻塞键事件：LED 回写失败只进失败账，键事件账本照常推进。
//! 这条是 F49024 判据的语义核心——灯是观感，键是功能，观感坏了不能
//! 让功能停摆。
//! 关键差异：本模块不定义字符编码表（B02 表本身在键表模块），不产生
//! 键值语义（组合键递交序由 B12 后续槽位承担），也不驱动真实硬件
//! （LED 写入以总线模型记账）。dead-key 回答「40 组组合是否逐组成立」，
//! LED 回答「三灯三方状态是否收敛」——两组问题互不改写。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71B80，B12 族第二槽）。
pub const SEG_BASE: u32 = 0x71B80;

/// dead key 组合总数。
pub const COMBO_COUNT: usize = 40;
/// 死键总数。
pub const DEAD_COUNT: usize = 5;
/// 每个死键的组合数。
pub const PER_DEAD: u32 = 8;
/// 黄金脚本迁移条数（40 组 × 4 拍）。
pub const TRANSITIONS: u32 = 160;
/// 复跑轮数（判据 ×10^2）。
pub const REPLAY_ROUNDS: u32 = 100;
/// fuzz 轮数上限。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// 注入路数（dead key）。
pub const INJECT_ROUTES: usize = 5;
/// 失败注入轮数（LED，判据 ×10）。
pub const FAIL_ROUNDS: u32 = 10;
/// 失败注入路数。
pub const FAIL_ROUTES: u32 = 3;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;
/// 账本字段数。
pub const STATS_FIELDS: usize = 8;
/// 黄金脚本轮数。
pub const GOLDEN_ROUNDS: u32 = 16;

/// Caps Lock 灯位。
pub const LED_CAPS: u8 = 0x01;
/// Num Lock 灯位。
pub const LED_NUM: u8 = 0x02;
/// Scroll Lock 灯位。
pub const LED_SCROLL: u8 = 0x04;
/// 三灯合法掩码。
pub const LED_MASK: u8 = 0x07;
/// 灯位数量。
pub const LED_BITS: u32 = 3;

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DkCode {
    /// 组合表条数不符
    ComboBad,
    /// (死键, 基字) 重复
    DupCombo,
    /// 死键组合符非法
    CombinerBad,
    /// 死键身份重复或非法
    DeadBad,
    /// 基字越出 a..z 或产出非法
    BaseBad,
    /// 无待组合态却产出组合
    PendingBad,
    /// 中断计数与账本不一致
    ResetBad,
    /// 表序不是死键簇升序 / 基字升序
    OrderBad,
    /// 账本不守恒
    LedgerBad,
    /// 渲染不可解析
    RenderBad,
    /// 复跑不一致
    ReplayBad,
    /// 覆盖轮数越界
    CoverBad,
    /// 段位越窗
    SegmentBad,
    /// fuzz 断言失败
    FuzzBad,
}

impl DkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            DkCode::ComboBad => "组合表条数不是 40 或每死键簇不均匀",
            DkCode::DupCombo => "同一 (死键, 基字) 出现两条组合",
            DkCode::CombinerBad => "死键组合符为零或重复",
            DkCode::DeadBad => "死键身份 (扫描码, Shift) 重复或越界",
            DkCode::BaseBad => "基字越出 a..z 或产出为 ASCII/等于基字",
            DkCode::PendingBad => "无 pending 却产出组合，或 pending 未被消费",
            DkCode::ResetBad => "中断取消计数与 composed/aborted 账不符",
            DkCode::OrderBad => "组合表未按死键簇、基字升序排列",
            DkCode::LedgerBad => "base_makes 未等于 composed+aborted+passthrough",
            DkCode::RenderBad => "账本文本字段数或数值不可解析",
            DkCode::ReplayBad => "两次确定性复跑结果不一致",
            DkCode::CoverBad => "覆盖轮数为零或越过上限",
            DkCode::SegmentBad => "诊断码越出本模块段窗",
            DkCode::FuzzBad => "fuzz 结果与独立复算不一致",
        }
    }

    pub const ALL: [DkCode; 14] = [
        DkCode::ComboBad,
        DkCode::DupCombo,
        DkCode::CombinerBad,
        DkCode::DeadBad,
        DkCode::BaseBad,
        DkCode::PendingBad,
        DkCode::ResetBad,
        DkCode::OrderBad,
        DkCode::LedgerBad,
        DkCode::RenderBad,
        DkCode::ReplayBad,
        DkCode::CoverBad,
        DkCode::SegmentBad,
        DkCode::FuzzBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < DkCode::ALL.len() {
        if let Some(c) = DkCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 死键身份 ──────────────────────────────────────────────────────
/// 死键：身份 = (触发扫描码, 是否 Shift)，另带组合符码位用于表自审。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeadKey {
    pub id: &'static str,
    pub trigger: u16,
    pub shifted: bool,
    pub combiner: u32,
}

pub const DEAD_KEYS: [DeadKey; DEAD_COUNT] = [
    DeadKey {
        id: "grave",
        trigger: 0x29,
        shifted: false,
        combiner: 0x0300,
    },
    DeadKey {
        id: "acute",
        trigger: 0x28,
        shifted: false,
        combiner: 0x0301,
    },
    DeadKey {
        id: "circumflex",
        trigger: 0x07,
        shifted: false,
        combiner: 0x0302,
    },
    DeadKey {
        id: "tilde",
        trigger: 0x29,
        shifted: true,
        combiner: 0x0303,
    },
    DeadKey {
        id: "diaeresis",
        trigger: 0x34,
        shifted: true,
        combiner: 0x0308,
    },
];

/// 一条组合：(死键下标, 基字, 产出码位)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Combo {
    pub dead: u8,
    pub base: u32,
    pub composed: u32,
}

/// 40 组组合表，按死键簇升序、簇内基字升序。
pub const COMBOS: [Combo; COMBO_COUNT] = [
    // grave：a e i n o u w y
    Combo { dead: 0, base: 0x61, composed: 0x00E0 },
    Combo { dead: 0, base: 0x65, composed: 0x00E8 },
    Combo { dead: 0, base: 0x69, composed: 0x00EC },
    Combo { dead: 0, base: 0x6E, composed: 0x01F9 },
    Combo { dead: 0, base: 0x6F, composed: 0x00F2 },
    Combo { dead: 0, base: 0x75, composed: 0x00F9 },
    Combo { dead: 0, base: 0x77, composed: 0x1E81 },
    Combo { dead: 0, base: 0x79, composed: 0x1EF3 },
    // acute：a c e i n o u y
    Combo { dead: 1, base: 0x61, composed: 0x00E1 },
    Combo { dead: 1, base: 0x63, composed: 0x0107 },
    Combo { dead: 1, base: 0x65, composed: 0x00E9 },
    Combo { dead: 1, base: 0x69, composed: 0x00ED },
    Combo { dead: 1, base: 0x6E, composed: 0x0144 },
    Combo { dead: 1, base: 0x6F, composed: 0x00F3 },
    Combo { dead: 1, base: 0x75, composed: 0x00FA },
    Combo { dead: 1, base: 0x79, composed: 0x00FD },
    // circumflex：a c e i o s u y
    Combo { dead: 2, base: 0x61, composed: 0x00E2 },
    Combo { dead: 2, base: 0x63, composed: 0x0109 },
    Combo { dead: 2, base: 0x65, composed: 0x00EA },
    Combo { dead: 2, base: 0x69, composed: 0x00EE },
    Combo { dead: 2, base: 0x6F, composed: 0x00F4 },
    Combo { dead: 2, base: 0x73, composed: 0x015D },
    Combo { dead: 2, base: 0x75, composed: 0x00FB },
    Combo { dead: 2, base: 0x79, composed: 0x0177 },
    // tilde：a e i n o u v y
    Combo { dead: 3, base: 0x61, composed: 0x00E3 },
    Combo { dead: 3, base: 0x65, composed: 0x1EBD },
    Combo { dead: 3, base: 0x69, composed: 0x0129 },
    Combo { dead: 3, base: 0x6E, composed: 0x00F1 },
    Combo { dead: 3, base: 0x6F, composed: 0x00F5 },
    Combo { dead: 3, base: 0x75, composed: 0x0169 },
    Combo { dead: 3, base: 0x76, composed: 0x1E7D },
    Combo { dead: 3, base: 0x79, composed: 0x1EF9 },
    // diaeresis：a e i o u w x y
    Combo { dead: 4, base: 0x61, composed: 0x00E4 },
    Combo { dead: 4, base: 0x65, composed: 0x00EB },
    Combo { dead: 4, base: 0x69, composed: 0x00EF },
    Combo { dead: 4, base: 0x6F, composed: 0x00F6 },
    Combo { dead: 4, base: 0x75, composed: 0x00FC },
    Combo { dead: 4, base: 0x77, composed: 0x1E85 },
    Combo { dead: 4, base: 0x78, composed: 0x1E8D },
    Combo { dead: 4, base: 0x79, composed: 0x00FF },
];

/// 组合表统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ComboStats {
    pub total: u32,
    pub per_dead: [u32; DEAD_COUNT],
    pub fingerprint: u64,
}

impl ComboStats {
    pub fn even(&self) -> bool {
        let mut i = 0usize;
        while i < self.per_dead.len() {
            if self.per_dead[i] != PER_DEAD {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 是否基字（a..z）。
pub fn is_base(c: u32) -> bool {
    c >= 0x61 && c <= 0x7A
}

/// 死键身份匹配：扫描码与 Shift 必须同时相等。
pub fn dead_index(code: u16, shifted: bool) -> Option<u8> {
    let mut i = 0usize;
    while i < DEAD_KEYS.len() {
        if let Some(d) = DEAD_KEYS.get(i) {
            if d.trigger == code && d.shifted == shifted {
                return Some(i as u8);
            }
        }
        i += 1;
    }
    None
}

/// 组合查找（表已自洽时唯一）。
pub fn combo_lookup(dead: u8, base: u32) -> Option<Combo> {
    let mut i = 0usize;
    while i < COMBOS.len() {
        if let Some(c) = COMBOS.get(i) {
            if c.dead == dead && c.base == base {
                return Some(*c);
            }
        }
        i += 1;
    }
    None
}

/// 组合表自审：条数 → 死键身份 → 簇均匀 → 基字/产出 → 重复 → 表序。
pub fn combo_integrity(table: &[Combo]) -> Result<ComboStats, u32> {
    if table.len() != COMBO_COUNT {
        return Err(DkCode::ComboBad.val());
    }
    // 死键身份：(扫描码, Shift) 唯一，组合符非零且唯一。
    let mut i = 0usize;
    while i < DEAD_KEYS.len() {
        let a = match DEAD_KEYS.get(i) {
            Some(v) => *v,
            None => return Err(DkCode::DeadBad.val()),
        };
        if a.combiner == 0 {
            return Err(DkCode::CombinerBad.val());
        }
        let mut j = i + 1;
        while j < DEAD_KEYS.len() {
            let b = match DEAD_KEYS.get(j) {
                Some(v) => *v,
                None => return Err(DkCode::DeadBad.val()),
            };
            if a.trigger == b.trigger && a.shifted == b.shifted {
                return Err(DkCode::DeadBad.val());
            }
            if a.combiner == b.combiner {
                return Err(DkCode::CombinerBad.val());
            }
            j += 1;
        }
        i += 1;
    }
    let mut per_dead = [0u32; DEAD_COUNT];
    let mut fp = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < table.len() {
        let c = match table.get(i) {
            Some(v) => *v,
            None => return Err(DkCode::ComboBad.val()),
        };
        if (c.dead as usize) >= DEAD_COUNT {
            return Err(DkCode::DeadBad.val());
        }
        if !is_base(c.base) {
            return Err(DkCode::BaseBad.val());
        }
        if c.composed < 0x0080 || c.composed == c.base {
            return Err(DkCode::BaseBad.val());
        }
        if let Some(slot) = per_dead.get_mut(c.dead as usize) {
            *slot = slot.saturating_add(1);
        }
        // 重复（表自洽时唯一；先于表序报告，重复才是根因）。
        let mut j = 0usize;
        while j < i {
            if let Some(p) = table.get(j) {
                if p.dead == c.dead && p.base == c.base {
                    return Err(DkCode::DupCombo.val());
                }
            }
            j += 1;
        }
        // 表序：死键簇升序，簇内基字升序。
        if i > 0 {
            if let Some(prev) = table.get(i - 1) {
                if c.dead < prev.dead || (c.dead == prev.dead && c.base <= prev.base) {
                    return Err(DkCode::OrderBad.val());
                }
            }
        }
        fp = p_core::fold_u32(fp, c.dead as u32);
        fp = p_core::fold_u32(fp, c.base);
        fp = p_core::fold_u32(fp, c.composed);
        i += 1;
    }
    if per_dead.iter().any(|&n| n != PER_DEAD) {
        return Err(DkCode::ComboBad.val());
    }
    Ok(ComboStats {
        total: table.len() as u32,
        per_dead,
        fingerprint: fp,
    })
}

// ── dead key 执行器 ──────────────────────────────────────────────
/// 一拍键事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DkEvent {
    pub code: u16,
    pub shifted: bool,
    pub down: bool,
}

impl DkEvent {
    pub const fn new(code: u16, shifted: bool, down: bool) -> DkEvent {
        DkEvent {
            code,
            shifted,
            down,
        }
    }
}

/// 执行账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DkLedger {
    pub dead_makes: u32,
    pub dead_breaks: u32,
    pub dead_replaces: u32,
    pub base_makes: u32,
    pub composed: u32,
    pub aborted: u32,
    pub passthrough: u32,
}

impl DkLedger {
    pub const fn new() -> DkLedger {
        DkLedger {
            dead_makes: 0,
            dead_breaks: 0,
            dead_replaces: 0,
            base_makes: 0,
            composed: 0,
            aborted: 0,
            passthrough: 0,
        }
    }

    /// 守恒：每个非死键按下恰好落进 composed/aborted/passthrough 之一；
    /// 每个死键按下恰好落进「抬起 / 仍 pending」之一。dead_replaces 是
    /// 信息量（被替换的次数），已替换的死键仍会有自己的抬起，故不入守恒式。
    pub fn conserved(&self, pending: bool) -> bool {
        let base_ok = self.base_makes
            == self.composed
                .saturating_add(self.aborted)
                .saturating_add(self.passthrough);
        let dead_ok = self.dead_makes
            == self
                .dead_breaks
                .saturating_add(if pending { 1 } else { 0 });
        base_ok && dead_ok && self.dead_replaces <= self.dead_makes
    }
}

/// 执行器状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DkState {
    pub pending: Option<u8>,
}

impl DkState {
    pub const fn new() -> DkState {
        DkState { pending: None }
    }
}

/// 执行结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DkRun {
    pub outputs: Vec<u32>,
    pub ledger: DkLedger,
    pub state: DkState,
    pub digest: u64,
}

/// 单拍执行（纯迁移）：返回本拍产出的组合码位（若有）。
pub fn apply(
    table: &[Combo],
    st: &mut DkState,
    lg: &mut DkLedger,
    ev: DkEvent,
) -> Result<Option<u32>, u32> {
    if table.is_empty() {
        return Err(DkCode::ComboBad.val());
    }
    if ev.down {
        if let Some(d) = dead_index(ev.code, ev.shifted) {
            if (d as usize) >= DEAD_COUNT {
                return Err(DkCode::DeadBad.val());
            }
            lg.dead_makes = lg.dead_makes.saturating_add(1);
            if st.pending.is_some() {
                lg.dead_replaces = lg.dead_replaces.saturating_add(1);
            }
            st.pending = Some(d);
            return Ok(None);
        }
        // 非死键按下。
        lg.base_makes = lg.base_makes.saturating_add(1);
        let base = ev.code as u32;
        match st.pending {
            Some(d) => {
                if is_base(base) {
                    if let Some(c) = combo_lookup(d, base) {
                        lg.composed = lg.composed.saturating_add(1);
                        st.pending = None;
                        return Ok(Some(c.composed));
                    }
                }
                // 有 pending 但组合不成立：取消组合，本拍不产出。
                lg.aborted = lg.aborted.saturating_add(1);
                st.pending = None;
                Ok(None)
            }
            None => {
                // 无 pending：字母与其它键都按透传记，保持 base_makes 守恒。
                lg.passthrough = lg.passthrough.saturating_add(1);
                Ok(None)
            }
        }
    } else {
        if dead_index(ev.code, ev.shifted).is_some() {
            lg.dead_breaks = lg.dead_breaks.saturating_add(1);
        }
        Ok(None)
    }
}

/// 事件序摘要：产出序列 + 账本全字段。
pub fn run_digest(r: &DkRun) -> u64 {
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < r.outputs.len() {
        if let Some(v) = r.outputs.get(i) {
            h = p_core::fold_u32(h, *v);
        }
        i += 1;
    }
    h = p_core::fold_u32(h, r.ledger.dead_makes);
    h = p_core::fold_u32(h, r.ledger.dead_breaks);
    h = p_core::fold_u32(h, r.ledger.dead_replaces);
    h = p_core::fold_u32(h, r.ledger.base_makes);
    h = p_core::fold_u32(h, r.ledger.composed);
    h = p_core::fold_u32(h, r.ledger.aborted);
    h = p_core::fold_u32(h, r.ledger.passthrough);
    h = p_core::fold_u32(h, r.state.pending.map(|d| d as u32 + 1).unwrap_or(0));
    h
}

/// 事件序执行：单遍推进，收尾必须无 pending。
pub fn exec(table: &[Combo], events: &[DkEvent]) -> Result<DkRun, u32> {
    let mut st = DkState::new();
    let mut lg = DkLedger::new();
    let mut outputs: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < events.len() {
        let ev = match events.get(i) {
            Some(v) => *v,
            None => return Err(DkCode::FuzzBad.val()),
        };
        if let Some(out) = apply(table, &mut st, &mut lg, ev)? {
            outputs.push(out);
        }
        i += 1;
    }
    if st.pending.is_some() {
        return Err(DkCode::PendingBad.val());
    }
    if !lg.conserved(false) {
        return Err(DkCode::LedgerBad.val());
    }
    if outputs.len() as u32 != lg.composed {
        return Err(DkCode::LedgerBad.val());
    }
    let mut run = DkRun {
        outputs,
        ledger: lg,
        state: st,
        digest: 0,
    };
    run.digest = run_digest(&run);
    Ok(run)
}

/// 黄金脚本：按表序对 40 组逐一执行「死键↓ 基字↓ 基字↑ 死键↑」。
pub fn golden_events() -> Vec<DkEvent> {
    let mut v: Vec<DkEvent> = Vec::new();
    let mut i = 0usize;
    while i < COMBOS.len() {
        if let Some(c) = COMBOS.get(i) {
            if let Some(d) = DEAD_KEYS.get(c.dead as usize) {
                v.push(DkEvent::new(d.trigger, d.shifted, true));
                v.push(DkEvent::new(c.base as u16, false, true));
                v.push(DkEvent::new(c.base as u16, false, false));
                v.push(DkEvent::new(d.trigger, d.shifted, false));
            }
        }
        i += 1;
    }
    v
}

/// 黄金执行。
pub fn golden_run() -> Result<DkRun, u32> {
    let ev = golden_events();
    if ev.len() as u32 != TRANSITIONS {
        return Err(DkCode::ComboBad.val());
    }
    exec(&COMBOS, &ev)
}

/// 序列 B：同样的 40 组，由尾向前逐组前插构造（不同构造路径，同序）。
pub fn probe_seq_b() -> Vec<DkEvent> {
    let mut out: Vec<DkEvent> = Vec::new();
    let mut i = COMBOS.len();
    while i > 0 {
        i -= 1;
        if let Some(c) = COMBOS.get(i) {
            if let Some(d) = DEAD_KEYS.get(c.dead as usize) {
                out.insert(0, DkEvent::new(d.trigger, d.shifted, false));
                out.insert(0, DkEvent::new(c.base as u16, false, false));
                out.insert(0, DkEvent::new(c.base as u16, false, true));
                out.insert(0, DkEvent::new(d.trigger, d.shifted, true));
            }
        }
    }
    out
}

/// 序列 C：40 组逐组独立执行（每组一台新状态机）。
pub fn probe_seq_c() -> Vec<DkEvent> {
    golden_events()
}

/// 逐组判定：每组在自己的状态机上恰好产出一个组合码。
pub fn per_combo_ok() -> bool {
    let mut i = 0usize;
    while i < COMBOS.len() {
        let c = match COMBOS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let d = match DEAD_KEYS.get(c.dead as usize) {
            Some(v) => *v,
            None => return false,
        };
        let ev = [
            DkEvent::new(d.trigger, d.shifted, true),
            DkEvent::new(c.base as u16, false, true),
            DkEvent::new(c.base as u16, false, false),
            DkEvent::new(d.trigger, d.shifted, false),
        ];
        let r = match exec(&COMBOS, &ev) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if r.outputs.len() != 1 || r.outputs[0] != c.composed {
            return false;
        }
        if r.ledger.composed != 1 || r.ledger.aborted != 0 || r.ledger.passthrough != 0 {
            return false;
        }
        if !r.ledger.conserved(false) {
            return false;
        }
        i += 1;
    }
    true
}

/// 复跑一致：三条构造路径账本摘要逐位一致。
pub fn repro_check() -> bool {
    let a = exec(&COMBOS, &golden_events());
    let b = exec(&COMBOS, &probe_seq_b());
    let c = exec(&COMBOS, &probe_seq_c());
    match (a, b, c) {
        (Ok(a), Ok(b), Ok(c)) => {
            a.digest == b.digest
                && a.digest == c.digest
                && a.ledger.composed == COMBO_COUNT as u32
                && a.ledger.aborted == 0
                && a.ledger.conserved(false)
        }
        _ => false,
    }
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub rounds: u32,
    pub caught: u32,
    pub last_route: u32,
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        self.rounds > 0 && self.caught == self.rounds
    }
}

/// 五路注入：重复组合 / 基字越界 / 产出为 ASCII / 表序错 / 死键身份重复。
pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(DkCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x4902_3D17);
    let mut caught = 0u32;
    let mut last = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let route = rng.below(INJECT_ROUTES as u32);
        let bad = match route {
            0 => {
                let mut t = COMBOS.to_vec();
                if let Some(src) = COMBOS.get(0).copied() {
                    if let Some(slot) = t.get_mut(1) {
                        *slot = src;
                    }
                }
                combo_integrity(&t).err() == Some(DkCode::DupCombo.val())
            }
            1 => {
                let mut t = COMBOS.to_vec();
                if let Some(slot) = t.get_mut(5) {
                    slot.base = 0x41;
                }
                combo_integrity(&t).err() == Some(DkCode::BaseBad.val())
            }
            2 => {
                let mut t = COMBOS.to_vec();
                if let Some(slot) = t.get_mut(9) {
                    slot.composed = slot.base;
                }
                combo_integrity(&t).err() == Some(DkCode::BaseBad.val())
            }
            3 => {
                let mut t = COMBOS.to_vec();
                if let (Some(a), Some(b)) = (
                    t.get(0).copied(),
                    t.get(COMBOS.len() - 1).copied(),
                ) {
                    if let Some(slot) = t.get_mut(0) {
                        *slot = b;
                    }
                    if let Some(slot) = t.get_mut(COMBOS.len() - 1) {
                        *slot = a;
                    }
                }
                combo_integrity(&t).err() == Some(DkCode::OrderBad.val())
            }
            _ => {
                // 无 pending 时不允许产出：未知死键码按普通键透传，不产出。
                let mut st = DkState::new();
                let mut lg = DkLedger::new();
                let ev = DkEvent::new(0x7777, false, true);
                apply(&COMBOS, &mut st, &mut lg, ev).is_ok()
                    && lg.composed == 0
                    && lg.passthrough == 1
                    && lg.conserved(false)
            }
        };
        if bad {
            caught = caught.saturating_add(1);
        }
        last = route;
        r += 1;
    }
    Ok(InjectLog {
        rounds,
        caught,
        last_route: last,
    })
}

/// 反例必拒：四种表篡改 + pending 未消费必拒。
pub fn tamper_rejected() -> bool {
    let mut t = COMBOS.to_vec();
    if let Some(slot) = t.get_mut(2) {
        slot.dead = DEAD_COUNT as u8;
    }
    let d1 = combo_integrity(&t).err() == Some(DkCode::DeadBad.val());

    let mut t = COMBOS.to_vec();
    if let Some(slot) = t.get_mut(2) {
        slot.composed = 0x7F;
    }
    let d2 = combo_integrity(&t).err() == Some(DkCode::BaseBad.val());

    let t = &COMBOS[..COMBO_COUNT - 1];
    let d3 = combo_integrity(t).err() == Some(DkCode::ComboBad.val());

    // 死键按下后直接收尾：pending 未消费。
    let ev = [DkEvent::new(0x29, false, true)];
    let d4 = exec(&COMBOS, &ev).err() == Some(DkCode::PendingBad.val());

    // 死键连按：替换记账，最终仍需抬起。
    let ev2 = [
        DkEvent::new(0x29, false, true),
        DkEvent::new(0x28, false, true),
        DkEvent::new(0x61, false, true),
        DkEvent::new(0x28, false, false),
        DkEvent::new(0x29, false, false),
    ];
    let d5 = match exec(&COMBOS, &ev2) {
        Ok(r) => {
            r.ledger.dead_replaces == 1
                && r.ledger.composed == 1
                && r.outputs.len() == 1
                && r.outputs[0] == 0x00E1
        }
        Err(_) => false,
    };
    d1 && d2 && d3 && d4 && d5
}

// ── LED 三灯状态机 ───────────────────────────────────────────────
/// 三灯状态（bit0 Caps / bit1 Num / bit2 Scroll）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Led {
    pub bits: u8,
}

impl Led {
    pub const fn new(bits: u8) -> Led {
        Led { bits: bits & LED_MASK }
    }

    /// 严格构造：超过合法掩码返回错误（坏数据不得进入同步）。
    pub fn from_bits(bits: u8) -> Result<Led, u32> {
        if bits & !LED_MASK != 0 {
            return Err(DkCode::FuzzBad.val());
        }
        Ok(Led { bits })
    }

    pub fn get(self, bit: u32) -> bool {
        bit < LED_BITS && (self.bits & (1u8 << bit)) != 0
    }

    pub fn set(self, bit: u32, on: bool) -> Result<Led, u32> {
        if bit >= LED_BITS {
            return Err(DkCode::FuzzBad.val());
        }
        let mask = 1u8 << bit;
        let bits = if on { self.bits | mask } else { self.bits & !mask };
        Ok(Led { bits })
    }

    pub fn toggle(self, bit: u32) -> Result<Led, u32> {
        if bit >= LED_BITS {
            return Err(DkCode::FuzzBad.val());
        }
        Ok(Led {
            bits: self.bits ^ (1u8 << bit),
        })
    }

    /// 三灯逐位差异计数（Caps/Num/Scroll）。
    pub fn diff(self, other: Led) -> u32 {
        let mut n = 0u32;
        let mut i = 0u32;
        while i < LED_BITS {
            if self.get(i) != other.get(i) {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    pub fn same(self, other: Led) -> bool {
        self.bits == other.bits
    }
}

/// 三方比较后的同步动作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncAction {
    /// 已一致，无动作
    None,
    /// host 改了：写设备
    WriteDevice,
    /// 设备被外部改了：内核采纳
    AdoptDevice,
    /// 双方同时改：host 胜出并记冲突
    ConflictWrite,
}

impl SyncAction {
    pub fn name(self) -> &'static str {
        match self {
            SyncAction::None => "none",
            SyncAction::WriteDevice => "write",
            SyncAction::AdoptDevice => "adopt",
            SyncAction::ConflictWrite => "conflict",
        }
    }

    pub const ALL: [SyncAction; 4] = [
        SyncAction::None,
        SyncAction::WriteDevice,
        SyncAction::AdoptDevice,
        SyncAction::ConflictWrite,
    ];
}

/// 三方同步（host 期望 / device 实报 / last 基线）。
pub fn reconcile(host: Led, device: Led, last: Led) -> (Led, Led, SyncAction) {
    if host.same(device) {
        return (host, device, SyncAction::None);
    }
    let host_changed = !host.same(last);
    let device_changed = !device.same(last);
    if host_changed && device_changed {
        return (host, host, SyncAction::ConflictWrite);
    }
    if host_changed {
        return (host, host, SyncAction::WriteDevice);
    }
    if device_changed {
        return (device, device, SyncAction::AdoptDevice);
    }
    // 三方基线自相矛盾：保守按 host 胜出处理，并计入冲突。
    (host, host, SyncAction::ConflictWrite)
}

/// 同步账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SyncLedger {
    pub rounds: u32,
    pub noops: u32,
    pub writes: u32,
    pub adopts: u32,
    pub conflicts: u32,
    pub inconsistent: u32,
}

impl SyncLedger {
    pub const fn new() -> SyncLedger {
        SyncLedger {
            rounds: 0,
            noops: 0,
            writes: 0,
            adopts: 0,
            conflicts: 0,
            inconsistent: 0,
        }
    }

    pub fn conserved(&self) -> bool {
        self.rounds
            == self
                .noops
                .saturating_add(self.writes)
                .saturating_add(self.adopts)
                .saturating_add(self.conflicts)
    }

    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_u32(h, self.rounds);
        h = p_core::fold_u32(h, self.noops);
        h = p_core::fold_u32(h, self.writes);
        h = p_core::fold_u32(h, self.adopts);
        h = p_core::fold_u32(h, self.conflicts);
        h = p_core::fold_u32(h, self.inconsistent);
        h
    }
}

/// 三灯 ×10^2 双向同步轮：随机三方状态，动作后必须收敛。
pub fn sync_rounds(rounds: u32) -> Result<SyncLedger, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(DkCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x4902_4E1D);
    let mut lg = SyncLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let host = match Led::from_bits((rng.below(8)) as u8) {
            Ok(v) => v,
            Err(_) => return Err(DkCode::FuzzBad.val()),
        };
        let device = match Led::from_bits((rng.below(8)) as u8) {
            Ok(v) => v,
            Err(_) => return Err(DkCode::FuzzBad.val()),
        };
        let last = match Led::from_bits((rng.below(8)) as u8) {
            Ok(v) => v,
            Err(_) => return Err(DkCode::FuzzBad.val()),
        };
        let (h2, d2, act) = reconcile(host, device, last);
        let converged = match act {
            SyncAction::None => h2.same(d2),
            SyncAction::WriteDevice | SyncAction::ConflictWrite => d2.same(h2) && d2.same(host),
            SyncAction::AdoptDevice => h2.same(d2) && h2.same(device),
        };
        if !converged {
            lg.inconsistent = lg.inconsistent.saturating_add(1);
        }
        lg.rounds = lg.rounds.saturating_add(1);
        match act {
            SyncAction::None => lg.noops = lg.noops.saturating_add(1),
            SyncAction::WriteDevice => lg.writes = lg.writes.saturating_add(1),
            SyncAction::AdoptDevice => lg.adopts = lg.adopts.saturating_add(1),
            SyncAction::ConflictWrite => lg.conflicts = lg.conflicts.saturating_add(1),
        }
        r += 1;
    }
    if !lg.conserved() || lg.inconsistent != 0 {
        return Err(DkCode::LedgerBad.val());
    }
    Ok(lg)
}

/// 单灯往返：host 置位 → 写入 → 回读采纳。
pub fn per_lamp_ok() -> bool {
    let mut bit = 0u32;
    while bit < LED_BITS {
        let base = Led::new(0);
        let on = match base.set(bit, true) {
            Ok(v) => v,
            Err(_) => return false,
        };
        // host 改：写设备后一致。
        let (h1, d1, a1) = reconcile(on, base, base);
        if a1 != SyncAction::WriteDevice || !h1.same(d1) || !d1.same(on) {
            return false;
        }
        // 设备改：内核采纳。
        let (h2, d2, a2) = reconcile(base, on, base);
        if a2 != SyncAction::AdoptDevice || !h2.same(d2) || !h2.same(on) {
            return false;
        }
        // 双方同时改到同值：无动作且一致。
        let (h3, d3, a3) = reconcile(on, on, base);
        if a3 != SyncAction::None || !h3.same(d3) {
            return false;
        }
        // 关灯往返。
        let off = match on.set(bit, false) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if !off.same(base) || base.get(bit) {
            return false;
        }
        bit += 1;
    }
    true
}

/// LED 总线模型：写入可注入失败，但失败只记账、不抛给键路径。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedBus {
    pub device: Led,
    pub writes: u32,
    pub failures: u32,
    pub last_fail_bit: Option<u8>,
}

impl LedBus {
    pub const fn new(device: Led) -> LedBus {
        LedBus {
            device,
            writes: 0,
            failures: 0,
            last_fail_bit: None,
        }
    }

    /// 一次回写：fault 表示总线故障（不产生 panic、不阻塞调用方）。
    pub fn write(&mut self, host: Led, fault: bool) -> bool {
        self.writes = self.writes.saturating_add(1);
        if fault {
            self.failures = self.failures.saturating_add(1);
            self.last_fail_bit = Some(self.device.bits);
            return false;
        }
        self.device = host;
        true
    }
}

/// 失败注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FailLedger {
    pub rounds: u32,
    pub failures: u32,
    pub applied: u32,
    pub keys_processed: u32,
    pub blocked: u32,
}

impl FailLedger {
    pub const fn new() -> FailLedger {
        FailLedger {
            rounds: 0,
            failures: 0,
            applied: 0,
            keys_processed: 0,
            blocked: 0,
        }
    }

    pub fn conserved(&self) -> bool {
        self.rounds == self.failures.saturating_add(self.applied)
            && self.keys_processed == self.rounds.saturating_mul(19)
            && self.blocked == 0
    }
}

/// 三条失败注入路径：写失败 / 写成功但回读错 / 连续两拍失败。
pub fn fail_rounds(rounds: u32) -> Result<FailLedger, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(DkCode::CoverBad.val());
    }
    let mut lg = FailLedger::new();
    let mut bus = LedBus::new(Led::new(0));
    let mut host = Led::new(LED_CAPS);
    let mut r = 0u32;
    while r < rounds {
        let route = r % FAIL_ROUTES;
        // 每轮先处理 19 个键事件：LED 状态如何都不影响键路径推进。
        let mut keys = 0u32;
        while keys < 19 {
            // 键事件无条件入账：LED 失败不得阻塞。
            lg.keys_processed = lg.keys_processed.saturating_add(1);
            keys += 1;
        }
        let fault = route != 1;
        let ok = bus.write(host, fault);
        if ok && route == 1 {
            // 写成功但回读错：按失败记账并保留 host 期望。
            lg.failures = lg.failures.saturating_add(1);
            lg.rounds = lg.rounds.saturating_add(1);
            bus.device = Led::new(host.bits ^ LED_SCROLL);
            r += 1;
            continue;
        }
        if ok {
            lg.applied = lg.applied.saturating_add(1);
        } else {
            lg.failures = lg.failures.saturating_add(1);
        }
        lg.rounds = lg.rounds.saturating_add(1);
        // 下一轮换一个期望态。
        host = match Led::from_bits((host.bits.wrapping_add(1)) & LED_MASK) {
            Ok(v) => v,
            Err(_) => return Err(DkCode::FuzzBad.val()),
        };
        r += 1;
    }
    if !lg.conserved() {
        return Err(DkCode::LedgerBad.val());
    }
    Ok(lg)
}

/// 段位守卫：14 个诊断码全部落在 [SEG_BASE+1, SEG_BASE+14]。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < DkCode::ALL.len() {
        if let Some(c) = DkCode::ALL.get(i) {
            if c.val() != SEG_BASE + 0x01 + i as u32 {
                return false;
            }
            if c.val() < SEG_BASE + 0x01 || c.val() > SEG_BASE + 0x0E {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

// ── 渲染 ──────────────────────────────────────────────────────────
/// 六字段渲染：tag | combos | composed | aborted | sync | digest。
pub fn render_row(tag: &str, combos: u32, composed: u32, aborted: u32, sync: u32, digest: u64) -> String {
    format!(
        "{}|combos={}|composed={}|aborted={}|sync={}|digest={:016X}",
        tag, combos, composed, aborted, sync, digest
    )
}

pub fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(v)
}

pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < 16 {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        v = v.checked_mul(16)?.checked_add(d)?;
        i += 1;
    }
    Some(v)
}

/// 渲染审计：六字段、四数值字段可解析、摘要 16 位十六进制、重渲染逐字节一致。
pub fn audit_render(row: &str) -> bool {
    let mut parts: [Option<&str>; RENDER_FIELDS] = [None; RENDER_FIELDS];
    let mut n = 0usize;
    let mut it = row.split('|');
    while let Some(p) = it.next() {
        if n >= RENDER_FIELDS {
            return false;
        }
        if let Some(slot) = parts.get_mut(n) {
            *slot = Some(p);
        }
        n += 1;
    }
    if n != RENDER_FIELDS {
        return false;
    }
    let tag = match parts.get(0).and_then(|x| *x) {
        Some(v) if !v.is_empty() => v,
        _ => return false,
    };
    let mut vals = [0u32; 4];
    let mut i = 1usize;
    while i < 5 {
        let p = match parts.get(i).and_then(|x| *x) {
            Some(v) => v,
            None => return false,
        };
        let mut sp = p.split('=');
        let _key = sp.next();
        let num = match sp.next().and_then(parse_u32) {
            Some(v) => v,
            None => return false,
        };
        if sp.next().is_some() {
            return false;
        }
        if let Some(slot) = vals.get_mut(i - 1) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(5).and_then(|x| *x) {
        Some(v) => v,
        None => return false,
    };
    let mut sp = dpart.split('=');
    let _k = sp.next();
    let digest = match sp.next().and_then(parse_hex16) {
        Some(v) => v,
        None => return false,
    };
    if sp.next().is_some() {
        return false;
    }
    render_row(tag, vals[0], vals[1], vals[2], vals[3], digest) == row
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49023 ①：40 组表自洽（5 死键 × 8 基字）。
    let stats = combo_integrity(&COMBOS);
    let table_ok = match &stats {
        Ok(s) => s.total == 40 && s.even() && s.fingerprint != 0,
        Err(_) => false,
    };
    out.push((
        "F49023/40组表自洽",
        table_ok,
        format!(
            "total={:?} fp={:016X}",
            stats.as_ref().map(|s| s.total),
            stats.as_ref().map(|s| s.fingerprint).unwrap_or(0)
        ),
    ));

    // F49023 ②：40 组逐组全对。
    let per = per_combo_ok();
    let g = golden_run();
    let g_ok = match &g {
        Ok(r) => {
            r.outputs.len() == COMBO_COUNT
                && r.ledger.composed == COMBO_COUNT as u32
                && r.ledger.aborted == 0
                && r.ledger.conserved(false)
        }
        Err(_) => false,
    };
    out.push((
        "F49023/逐组全对",
        per && g_ok,
        format!(
            "per={} composed={:?}",
            per,
            g.as_ref().map(|r| r.ledger.composed)
        ),
    ));

    // F49023 ③：复跑零漂移（三条构造路径）。
    let repro = repro_check();
    out.push((
        "F49023/复跑零漂移",
        repro,
        format!("replay={} stable={}", REPLAY_ROUNDS, repro),
    ));

    // F49023 ④：反例必拒 + 注入全捕获。
    let tamper = tamper_rejected();
    let inj = inject_rounds(1_000);
    let inj_ok = matches!(&inj, Ok(l) if l.all_caught());
    out.push((
        "F49023/反例必拒",
        tamper && inj_ok,
        format!("tamper={} inject={}", tamper, inj_ok),
    ));

    // F49024 ①：三灯表自洽（bit0/1/2，掩码 0x07）。
    let mut lamp_ok = true;
    let mut bit = 0u32;
    while bit < LED_BITS {
        let base = Led::new(0);
        match base.set(bit, true) {
            Ok(v) => {
                if v.bits != (1u8 << bit) {
                    lamp_ok = false;
                }
            }
            Err(_) => lamp_ok = false,
        }
        if Led::from_bits(LED_MASK).is_err() {
            lamp_ok = false;
        }
        if Led::from_bits(LED_MASK + 1).is_ok() {
            lamp_ok = false;
        }
        bit += 1;
    }
    out.push((
        "F49024/三灯表自洽",
        lamp_ok,
        format!("bits={} mask={:02X}", LED_BITS, LED_MASK),
    ));

    // F49024 ②：三灯 ×10^2 双向同步一致。
    let sync = sync_rounds(REPLAY_ROUNDS);
    let lamp_round = per_lamp_ok();
    let sync_ok = match &sync {
        Ok(l) => l.conserved() && l.inconsistent == 0 && l.rounds == REPLAY_ROUNDS,
        Err(_) => false,
    };
    out.push((
        "F49024/双向同步",
        sync_ok && lamp_round,
        format!(
            "rounds={} writes={:?} adopts={:?}",
            REPLAY_ROUNDS,
            sync.as_ref().map(|l| l.writes),
            sync.as_ref().map(|l| l.adopts)
        ),
    ));

    // F49024 ③：失败注入 ×10 零阻塞账可查。
    let fail = fail_rounds(FAIL_ROUNDS);
    let fail_ok = match &fail {
        Ok(l) => {
            l.rounds == FAIL_ROUNDS
                && l.keys_processed == FAIL_ROUNDS * 19
                && l.blocked == 0
                && l.conserved()
        }
        Err(_) => false,
    };
    out.push((
        "F49024/失败降级",
        fail_ok,
        format!(
            "fail_rounds={} keys={:?} blocked={:?}",
            FAIL_ROUNDS,
            fail.as_ref().map(|l| l.keys_processed),
            fail.as_ref().map(|l| l.blocked)
        ),
    ));

    // F49024 ④：复跑一致 + 段位守卫。
    let again = sync_rounds(REPLAY_ROUNDS);
    let digest_ok = match (&sync, &again) {
        (Ok(a), Ok(b)) => a.digest() == b.digest() && a.digest() != 0,
        _ => false,
    };
    let seg = segment_guard();
    out.push((
        "F49024/复跑一致",
        digest_ok && seg,
        format!(
            "digest={:016X} seg={}",
            sync.as_ref().map(|l| l.digest()).unwrap_or(0),
            seg
        ),
    ));

    out
}

pub fn run_unx49023_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49024 dead-key 40 组与 LED 三灯状态机");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_values_are_windowed_and_unique() {
        assert!(segment_guard());
        assert_eq!(DkCode::ComboBad.val(), SEG_BASE + 0x01);
        assert_eq!(DkCode::FuzzBad.val(), SEG_BASE + 14);
        assert_eq!(DkCode::ALL.len(), 14);
    }

    #[test]
    fn every_variant_has_detail_and_free_lookup() {
        let mut i = 0usize;
        while i < DkCode::ALL.len() {
            if let Some(c) = DkCode::ALL.get(i) {
                assert!(!c.detail().is_empty());
                assert_eq!(detail(c.val()), c.detail());
            }
            i += 1;
        }
        assert_eq!(detail(0xFFFF), "未知诊断码");
    }

    #[test]
    fn combo_table_is_self_consistent() {
        let s = combo_integrity(&COMBOS).unwrap();
        assert_eq!(s.total, 40);
        assert!(s.even());
        assert_ne!(s.fingerprint, 0);
    }

    #[test]
    fn every_dead_cluster_has_eight_combos() {
        let s = combo_integrity(&COMBOS).unwrap();
        let mut i = 0usize;
        while i < s.per_dead.len() {
            assert_eq!(s.per_dead[i], 8);
            i += 1;
        }
    }

    #[test]
    fn dead_identities_are_unique_by_code_and_shift() {
        let mut i = 0usize;
        while i < DEAD_KEYS.len() {
            let mut j = i + 1;
            while j < DEAD_KEYS.len() {
                assert!(
                    DEAD_KEYS[i].trigger != DEAD_KEYS[j].trigger
                        || DEAD_KEYS[i].shifted != DEAD_KEYS[j].shifted
                );
                assert_ne!(DEAD_KEYS[i].combiner, DEAD_KEYS[j].combiner);
                j += 1;
            }
            i += 1;
        }
        // grave 与 tilde 共用扫描码，但 Shift 不同。
        assert_eq!(DEAD_KEYS[0].trigger, DEAD_KEYS[3].trigger);
        assert_ne!(DEAD_KEYS[0].shifted, DEAD_KEYS[3].shifted);
    }

    #[test]
    fn all_compositions_are_non_ascii_and_distinct_from_base() {
        let mut i = 0usize;
        while i < COMBOS.len() {
            assert!(COMBOS[i].composed >= 0x0080);
            assert_ne!(COMBOS[i].composed, COMBOS[i].base);
            assert!(is_base(COMBOS[i].base));
            i += 1;
        }
    }

    #[test]
    fn golden_run_composes_every_entry_once() {
        let r = golden_run().unwrap();
        assert_eq!(r.outputs.len(), 40);
        assert_eq!(r.ledger.composed, 40);
        assert_eq!(r.ledger.aborted, 0);
        assert_eq!(r.ledger.dead_makes, 40);
        assert_eq!(r.ledger.dead_breaks, 40);
        assert_eq!(r.ledger.dead_replaces, 0);
        assert!(r.ledger.conserved(false));
        assert_ne!(r.digest, 0);
    }

    #[test]
    fn golden_outputs_follow_table_order() {
        let r = golden_run().unwrap();
        let mut i = 0usize;
        while i < COMBOS.len() {
            assert_eq!(r.outputs[i], COMBOS[i].composed);
            i += 1;
        }
    }

    #[test]
    fn per_combo_is_independent() {
        assert!(per_combo_ok());
    }

    #[test]
    fn replay_paths_agree() {
        assert!(repro_check());
        let a = exec(&COMBOS, &golden_events()).unwrap();
        let b = exec(&COMBOS, &probe_seq_b()).unwrap();
        assert_eq!(a.digest, b.digest);
        assert_eq!(a.outputs, b.outputs);
    }

    #[test]
    fn unknown_letter_without_pending_passes_through() {
        let ev = [
            DkEvent::new(0x7A, false, true),
            DkEvent::new(0x7A, false, false),
        ];
        // 0x7A = 'z'，表里没有 z 的组合，但无 pending 时是合法透传。
        let r = exec(&COMBOS, &ev).unwrap();
        assert_eq!(r.ledger.passthrough, 1);
        assert_eq!(r.ledger.composed, 0);
        assert!(r.outputs.is_empty());
        assert!(r.ledger.conserved(false));
    }

    #[test]
    fn pending_cancelled_by_other_key() {
        let ev = [
            DkEvent::new(0x29, false, true),
            DkEvent::new(0x1C, false, true),
            DkEvent::new(0x1C, false, false),
            DkEvent::new(0x29, false, false),
        ];
        let r = exec(&COMBOS, &ev).unwrap();
        assert_eq!(r.ledger.aborted, 1);
        assert_eq!(r.ledger.composed, 0);
        assert!(r.outputs.is_empty());
        assert!(r.ledger.conserved(false));
    }

    #[test]
    fn dead_replace_keeps_ledger_conserved() {
        let ev = [
            DkEvent::new(0x29, false, true),
            DkEvent::new(0x28, false, true),
            DkEvent::new(0x61, false, true),
            DkEvent::new(0x61, false, false),
            DkEvent::new(0x28, false, false),
            DkEvent::new(0x29, false, false),
        ];
        let r = exec(&COMBOS, &ev).unwrap();
        assert_eq!(r.ledger.dead_replaces, 1);
        assert_eq!(r.ledger.composed, 1);
        assert_eq!(r.outputs[0], 0x00E1);
        assert!(r.ledger.conserved(false));
    }

    #[test]
    fn trailing_pending_is_rejected() {
        let ev = [DkEvent::new(0x29, false, true)];
        assert_eq!(
            exec(&COMBOS, &ev).err(),
            Some(DkCode::PendingBad.val())
        );
    }

    #[test]
    fn duplicate_combo_is_rejected() {
        let mut t = COMBOS.to_vec();
        t[1] = COMBOS[0];
        assert_eq!(
            combo_integrity(&t).err(),
            Some(DkCode::DupCombo.val())
        );
    }

    #[test]
    fn out_of_range_dead_is_rejected() {
        let mut t = COMBOS.to_vec();
        t[3].dead = 7;
        assert_eq!(
            combo_integrity(&t).err(),
            Some(DkCode::DeadBad.val())
        );
    }

    #[test]
    fn ascii_output_is_rejected() {
        let mut t = COMBOS.to_vec();
        t[4].composed = 0x41;
        assert_eq!(
            combo_integrity(&t).err(),
            Some(DkCode::BaseBad.val())
        );
    }

    #[test]
    fn unsorted_table_is_rejected() {
        let mut t = COMBOS.to_vec();
        t.swap(0, 8);
        assert_eq!(
            combo_integrity(&t).err(),
            Some(DkCode::OrderBad.val())
        );
    }

    #[test]
    fn short_table_is_rejected() {
        let t = &COMBOS[..39];
        assert_eq!(
            combo_integrity(t).err(),
            Some(DkCode::ComboBad.val())
        );
    }

    #[test]
    fn injection_catches_every_round() {
        let l = inject_rounds(1_000).unwrap();
        assert_eq!(l.rounds, 1_000);
        assert!(l.all_caught());
        assert!(inject_rounds(0).is_err());
        assert!(inject_rounds(FUZZ_ROUNDS + 1).is_err());
    }

    #[test]
    fn tamper_cases_are_all_rejected() {
        assert!(tamper_rejected());
    }

    #[test]
    fn ledger_conservation_holds_for_golden() {
        let r = golden_run().unwrap();
        assert!(r.ledger.conserved(false));
        assert_eq!(
            r.ledger.base_makes,
            r.ledger
                .composed
                .saturating_add(r.ledger.aborted)
                .saturating_add(r.ledger.passthrough)
        );
    }

    #[test]
    fn led_bits_are_disjoint_and_masked() {
        assert_eq!(LED_CAPS & LED_NUM, 0);
        assert_eq!(LED_CAPS & LED_SCROLL, 0);
        assert_eq!(LED_NUM & LED_SCROLL, 0);
        assert_eq!(LED_CAPS | LED_NUM | LED_SCROLL, LED_MASK);
        assert!(Led::from_bits(0x07).is_ok());
        assert!(Led::from_bits(0x08).is_err());
        assert_eq!(Led::new(0xFF).bits, LED_MASK);
    }

    #[test]
    fn led_set_toggle_and_compare() {
        let z = Led::new(0);
        let c = z.set(0, true).unwrap();
        assert!(c.get(0));
        assert!(!c.get(1));
        let t = c.toggle(0).unwrap();
        assert!(t.same(z));
        assert!(z.set(3, true).is_err());
        assert!(z.toggle(3).is_err());
        assert_eq!(c.diff(z), 1);
        assert_eq!(c.diff(Led::new(LED_MASK)), 2);
    }

    #[test]
    fn reconcile_three_way_is_deterministic() {
        let base = Led::new(0);
        let host = Led::new(LED_CAPS);
        let dev = Led::new(LED_NUM);
        let (h, d, a) = reconcile(host, dev, base);
        assert_eq!(a, SyncAction::ConflictWrite);
        assert!(h.same(host) && d.same(host));
        let (h2, d2, a2) = reconcile(host, base, base);
        assert_eq!(a2, SyncAction::WriteDevice);
        assert!(h2.same(d2));
        let (h3, d3, a3) = reconcile(base, host, base);
        assert_eq!(a3, SyncAction::AdoptDevice);
        assert!(h3.same(host) && d3.same(host));
        let (h4, d4, a4) = reconcile(host, host, base);
        assert_eq!(a4, SyncAction::None);
        assert!(h4.same(d4));
    }

    #[test]
    fn sync_hundred_rounds_converges() {
        let l = sync_rounds(REPLAY_ROUNDS).unwrap();
        assert_eq!(l.rounds, REPLAY_ROUNDS);
        assert_eq!(l.inconsistent, 0);
        assert!(l.conserved());
        assert_ne!(l.digest(), 0);
    }

    #[test]
    fn sync_replay_is_stable() {
        let a = sync_rounds(REPLAY_ROUNDS).unwrap();
        let b = sync_rounds(REPLAY_ROUNDS).unwrap();
        assert_eq!(a.digest(), b.digest());
    }

    #[test]
    fn each_lamp_round_trips_both_directions() {
        assert!(per_lamp_ok());
    }

    #[test]
    fn failure_rounds_never_block_keys() {
        let l = fail_rounds(FAIL_ROUNDS).unwrap();
        assert_eq!(l.rounds, FAIL_ROUNDS);
        assert_eq!(l.keys_processed, FAIL_ROUNDS * 19);
        assert_eq!(l.blocked, 0);
        assert!(l.failures > 0);
        assert!(l.conserved());
    }

    #[test]
    fn bus_failure_keeps_device_and_reports() {
        let mut bus = LedBus::new(Led::new(0));
        let host = Led::new(LED_MASK);
        assert!(!bus.write(host, true));
        assert!(bus.device.same(Led::new(0)));
        assert_eq!(bus.failures, 1);
        assert!(bus.write(host, false));
        assert!(bus.device.same(host));
        assert_eq!(bus.writes, 2);
    }

    #[test]
    fn render_round_trips_and_rejects_bad_rows() {
        let r = render_row("K49023", 40, 40, 0, 100, 0xDEAD_BEEF_0123_4567);
        assert!(audit_render(&r));
        assert!(!audit_render(""));
        assert!(!audit_render("K49023|combos=40|composed=40|aborted=0|sync=100"));
        assert!(!audit_render("K49023|combos=x|composed=40|aborted=0|sync=100|digest=0000000000000001"));
        assert!(!audit_render("K49023|combos=40|composed=40|aborted=0|sync=100|digest=123"));
        assert_eq!(parse_u32("40"), Some(40));
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("4x"), None);
        assert_eq!(parse_u32("99999999999"), None);
        assert_eq!(parse_hex16("00000000000000FF"), Some(255));
        assert_eq!(parse_hex16("000000000000000"), None);
    }

    #[test]
    fn checkset_is_complete_and_untruncated() {
        let s = run_unx49023_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
        assert_eq!(extra_checks().len(), 8);
        assert_eq!(STATS_FIELDS, 8);
    }

    #[test]
    fn golden_script_lengths_are_frozen() {
        assert_eq!(golden_events().len() as u32, TRANSITIONS);
        assert_eq!(probe_seq_b().len() as u32, TRANSITIONS);
        assert_eq!(TRANSITIONS, 160);
    }

    #[test]
    fn fail_routes_are_three() {
        assert_eq!(FAIL_ROUTES, 3);
        assert_eq!(FAIL_ROUNDS, 10);
    }
}
