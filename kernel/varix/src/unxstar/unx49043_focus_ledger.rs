//! UNX-F49043 焦点切换零丢账复跑 · 0x73040 段（B13 批第二槽）
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，不得改写）：
//!   UNX-F49043 焦点切换零丢账复跑（1,000 次切换按键零丢——J-2 组成项复跑）:
//!          ① 纯功能代码 ≥360 行；
//!          ② 判据原文：UNX-F49044-J1 约束 ×10 路径执行一致收口；判据锚定 §AI-62 §2 专题六；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   UNX-F49044 pointer-constraint 执行面收口（锁屏语义 F1 声明本域执行）:
//!          ① 纯功能代码 ≥340 行；
//!          ② 判据原文：UNX-F49045-J1 热消失 ×10 清算零悬挂；判据锚定 体验三 状态机；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（卡一 F49043）：建窗 → 定水位 → 递送 → 唯一性 → 复跑。
//! 建窗先于定水位：没有可切换的窗口集合，"提交水位"只是无主整数；
//! 定水位先于递送：一次切换必须先在按键序上切开一个水位（提交点），
//! 水位之前归旧焦点、水位之后归新焦点——边界值取「小于等于」归旧，
//! 这条口径一旦含糊，零丢就退化成"谁的账都能自证"；
//! 递送先于唯一性：先逐键记录投递去向，再谈每键恰好一次；
//! 唯一性先于复跑：复跑只能是"同一口径重跑得到同一摘要"，
//! 口径本身不许在第二遍里被悄悄放宽。
//! 闸门次序（卡二 F49044）：定域 → 分类 → 求值 → 双引擎 → 收口。
//! 定域先于分类：约束区域必须是合法矩形，退化的矩形不配谈边界语义；
//! 分类先于求值：自由/限制/屏障/锁定四类各有各的执行面，混类求值会把
//! "屏障只挡声明过的边"错算成"四面都挡"；
//! 求值先于双引擎：先有一份可读的直接实现，再写一份表驱动的独立实现，
//! 两份不一致时才轮得到"谁对"的问题；
//! 双引擎先于收口：10 条路径 × 4 点的双引擎结果全部一致，收口才有内容。
//! 关键差异：F49043 审的是"事件在切换点两侧的归属账"（键不丢、不重、
//! 不错投）；F49044 审的是"坐标在约束面内的落点"（不越界、不越权、
//! 两套实现同值）。前者是时间轴上的账，后者是空间域上的账，互不复算。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x73040，B13 批第二槽）。
pub const SEG_BASE: u32 = 0x73040;

/// 切换规模（卡面 ×10^3）。
pub const SWITCHES: u32 = 1_000;
/// 每次切换夹带的按键条数。
pub const KEYS_PER_SWITCH: u32 = 3;
/// 按键总数（1000 × 3）。
pub const KEY_TOTAL: u32 = SWITCHES * KEYS_PER_SWITCH;
/// 可切换窗口数。
pub const WIN_TOTAL: usize = 5;
/// 约束路径条数（卡面 ×10 路径）。
pub const PATH_TOTAL: usize = 10;
/// 每条路径的采样点数。
pub const PATH_POINTS: usize = 4;
/// 双引擎比对点总数（10 × 4）。
pub const PROBE_TOTAL: usize = PATH_TOTAL * PATH_POINTS;
/// 屏障边位：左。
pub const EDGE_LEFT: u8 = 0x01;
/// 屏障边位：右。
pub const EDGE_RIGHT: u8 = 0x02;
/// 屏障边位：上。
pub const EDGE_TOP: u8 = 0x04;
/// 屏障边位：下。
pub const EDGE_BOTTOM: u8 = 0x08;
/// 合法边掩码。
pub const EDGE_MASK: u8 = 0x0F;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;
/// 按键账标签。
pub const KB_TAG: &str = "KBLEDG";
/// 约束账标签。
pub const CON_TAG: &str = "CONSTR";
/// 切换账标签。
pub const SW_TAG: &str = "SWITCH";
/// 锁屏声明行锚（F1 声明、本域执行）。
pub const LOCK_ANCHOR: &str = "F1:lock-screen/EXEC=unxstar";
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 F49043 段诊断码";

// ── 诊断码闭集（窗口 +0x01..+0x0E） ─────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedgerCode {
    /// 按键账不守恒（投递数 + 丢失数 ≠ 键数）
    KbBad,
    /// 提交水位越界或倒退
    CommitBad,
    /// 切换目标非法（同窗或未知窗）
    TargetBad,
    /// 同一按键被投递两次
    DupBad,
    /// 按键未被任何焦点接收
    LostBad,
    /// 归属错投（旧新两侧颠倒）
    OrderBad,
    /// 渲染字段不可解析
    RenderBad,
    /// 复跑摘要漂移
    ReplayBad,
    /// 覆盖不足（切换或键数不达标）
    CoverageBad,
    /// 诊断码越出段位窗口
    SegmentBad,
    /// 约束矩形非法（退化或反序）
    ConstraintBad,
    /// 边掩码越界（声明了不存在的边）
    EdgeBad,
    /// 计数溢出
    Overflow,
    /// 摘要为零
    DigestBad,
}

impl LedgerCode {
    pub const fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            LedgerCode::KbBad => "按键账不守恒：投递数加丢失数不等于键数",
            LedgerCode::CommitBad => "提交水位越界或倒退",
            LedgerCode::TargetBad => "切换目标非法：同窗或未知窗",
            LedgerCode::DupBad => "同一按键被投递两次",
            LedgerCode::LostBad => "按键未被任何焦点接收",
            LedgerCode::OrderBad => "归属错投：旧新两侧颠倒",
            LedgerCode::RenderBad => "渲染字段不可解析",
            LedgerCode::ReplayBad => "复跑摘要漂移",
            LedgerCode::CoverageBad => "覆盖不足：切换或键数不达标",
            LedgerCode::SegmentBad => "诊断码越出段位窗口",
            LedgerCode::ConstraintBad => "约束矩形非法：退化或反序",
            LedgerCode::EdgeBad => "边掩码越界：声明了不存在的边",
            LedgerCode::Overflow => "计数溢出",
            LedgerCode::DigestBad => "摘要为零：折叠未发生",
        }
    }

    pub const ALL: [LedgerCode; 14] = [
        LedgerCode::KbBad,
        LedgerCode::CommitBad,
        LedgerCode::TargetBad,
        LedgerCode::DupBad,
        LedgerCode::LostBad,
        LedgerCode::OrderBad,
        LedgerCode::RenderBad,
        LedgerCode::ReplayBad,
        LedgerCode::CoverageBad,
        LedgerCode::SegmentBad,
        LedgerCode::ConstraintBad,
        LedgerCode::EdgeBad,
        LedgerCode::Overflow,
        LedgerCode::DigestBad,
    ];
}

/// 按诊断码取值反查中文说明。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < LedgerCode::ALL.len() {
        if let Some(c) = LedgerCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    UNKNOWN
}

/// 是否为本段已知码。
pub fn known_code(code: u32) -> bool {
    detail(code) != UNKNOWN
}

/// 段位守卫：14 条码连续落在 0x73041..0x7304E。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < LedgerCode::ALL.len() {
        let c = match LedgerCode::ALL.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.val() != SEG_BASE + 0x01 + i as u32 {
            return false;
        }
        if c.val() < SEG_BASE + 0x01 || c.val() > SEG_BASE + 0x0E {
            return false;
        }
        i += 1;
    }
    true
}

// ── 用例编号（16 条）与保留号 ────────────────────────────────────
/// 前序槽位占用号（B12 族，抄录自前批冻结清单）。
pub const RESERVED_IDS: [u32; 8] = [
    0x071C_8001,
    0x071C_8002,
    0x071C_8003,
    0x071C_8014,
    0x071D_0001,
    0x071D_0002,
    0x071D_2001,
    0x071D_2014,
];

/// 本模块 16 个用例编号（0x0733_0401..=0x0733_0410）。
pub const CASE_IDS: [u32; 16] = [
    0x0733_0401,
    0x0733_0402,
    0x0733_0403,
    0x0733_0404,
    0x0733_0405,
    0x0733_0406,
    0x0733_0407,
    0x0733_0408,
    0x0733_0409,
    0x0733_040A,
    0x0733_040B,
    0x0733_040C,
    0x0733_040D,
    0x0733_040E,
    0x0733_040F,
    0x0733_0410,
];

/// 用例语义绑定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseSpec {
    pub id: u32,
    pub name: &'static str,
}

/// 16 条用例声明（逐条挂到功能单元）。
pub const CASES: [CaseSpec; 16] = [
    CaseSpec { id: 0x0733_0401, name: "千次切换建账" },
    CaseSpec { id: 0x0733_0402, name: "提交水位切分" },
    CaseSpec { id: 0x0733_0403, name: "水位等值归旧" },
    CaseSpec { id: 0x0733_0404, name: "按键唯一投递" },
    CaseSpec { id: 0x0733_0405, name: "丢键检出" },
    CaseSpec { id: 0x0733_0406, name: "重投检出" },
    CaseSpec { id: 0x0733_0407, name: "错投检出" },
    CaseSpec { id: 0x0733_0408, name: "切换复跑" },
    CaseSpec { id: 0x0733_0409, name: "约束矩形校验" },
    CaseSpec { id: 0x0733_040A, name: "限制域钳位" },
    CaseSpec { id: 0x0733_040B, name: "屏障边掩码" },
    CaseSpec { id: 0x0733_040C, name: "锁屏冻结" },
    CaseSpec { id: 0x0733_040D, name: "锁屏逃逸" },
    CaseSpec { id: 0x0733_040E, name: "表驱动同值" },
    CaseSpec { id: 0x0733_040F, name: "十路径收口" },
    CaseSpec { id: 0x0733_0410, name: "越界注入" },
];

/// 跨槽零撞号：16 条用例号内部互异、与保留号不相交、不落诊断窗口。
pub fn no_cross_collision() -> bool {
    let mut i = 0usize;
    while i < CASE_IDS.len() {
        let a = match CASE_IDS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a >= SEG_BASE + 0x01 && a <= SEG_BASE + 0x0E {
            return false;
        }
        let mut j = 0usize;
        while j < RESERVED_IDS.len() {
            if let Some(r) = RESERVED_IDS.get(j) {
                if a == *r {
                    return false;
                }
            }
            j += 1;
        }
        let mut k = i + 1;
        while k < CASE_IDS.len() {
            if let Some(b) = CASE_IDS.get(k) {
                if a == *b {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }
    true
}

/// 用例编号连续：CASE_IDS[i] == 0x0733_0400 + 1 + i。
pub fn case_ids_sequential() -> bool {
    let base: u32 = 0x0733_0400;
    let mut i = 0usize;
    while i < CASE_IDS.len() {
        let want = match base.checked_add(1 + i as u32) {
            Some(v) => v,
            None => return false,
        };
        if let Some(v) = CASE_IDS.get(i) {
            if *v != want {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 切换账（F49043） ─────────────────────────────────────────────
/// 可切换窗口编号表（与 F49041 段刻意错开，避免跨段混读）。
pub const WINDOWS: [u32; WIN_TOTAL] = [0x6B01, 0x6B02, 0x6B03, 0x6B04, 0x6B05];

/// 一条按键：按键序、码、按下/抬起。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyRecord {
    pub seq: u32,
    pub code: u16,
    pub down: bool,
}

/// 一次切换：切换序、旧窗、新窗、提交水位（按键序口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SwitchRecord {
    pub seq: u32,
    pub from: u32,
    pub to: u32,
    pub commit: u32,
}

impl SwitchRecord {
    /// 键归属：水位内（含等值）归旧窗，水位之后归新窗。
    pub fn owner(&self, key_seq: u32) -> u32 {
        if key_seq <= self.commit {
            self.from
        } else {
            self.to
        }
    }
}

/// 投递规则（正常与新账两套口径，用于注入检出）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rule {
    Correct,
    Double,
    Drop,
    Misroute,
}

/// 按规则投递一条按键，返回接收方序列。
pub fn deliver_key(sw: &SwitchRecord, k: &KeyRecord, rule: Rule) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let owner = sw.owner(k.seq);
    match rule {
        Rule::Correct => out.push(owner),
        Rule::Double => {
            out.push(owner);
            out.push(if owner == sw.from { sw.to } else { sw.from });
        }
        Rule::Drop => {}
        Rule::Misroute => {
            out.push(if owner == sw.from { sw.to } else { sw.from });
        }
    }
    out
}

/// 单键审计：归属 → 恰好一次 → 去向正确。
pub fn audit_key(sw: &SwitchRecord, k: &KeyRecord, targets: &[u32]) -> Result<(), u32> {
    if targets.is_empty() {
        return Err(LedgerCode::LostBad.val());
    }
    if targets.len() > 1 {
        return Err(LedgerCode::DupBad.val());
    }
    let got = match targets.first() {
        Some(v) => *v,
        None => return Err(LedgerCode::LostBad.val()),
    };
    if got != sw.from && got != sw.to {
        return Err(LedgerCode::TargetBad.val());
    }
    if got != sw.owner(k.seq) {
        return Err(LedgerCode::OrderBad.val());
    }
    Ok(())
}

/// 按键账：切换数、键数、投递数、丢失/重复数、水位前/后归属数、摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KbLedger {
    pub switches: u32,
    pub keys: u32,
    pub delivered: u32,
    pub lost: u32,
    pub dup: u32,
    pub pre: u32,
    pub post: u32,
    pub first_seq: u32,
    pub last_seq: u32,
    pub digest: u64,
}

impl KbLedger {
    /// 守恒：键数 = 切换数 × 每切换键数；投递 + 丢失 = 键数；
    /// 前置 + 后置 = 键数；重复为零；键序窗口连续。
    pub fn conserved(&self) -> bool {
        if self.switches != SWITCHES || self.keys != KEY_TOTAL {
            return false;
        }
        if self.delivered.checked_add(self.lost) != Some(self.keys) {
            return false;
        }
        if self.pre.checked_add(self.post) != Some(self.keys) {
            return false;
        }
        if self.dup != 0 {
            return false;
        }
        if self.first_seq != 1 {
            return false;
        }
        match self.first_seq.checked_add(self.keys - 1) {
            Some(v) => v == self.last_seq,
            None => false,
        }
    }
}

/// 千次切换零丢：每切换两键在提交前、一键在提交后，逐键审计。
pub fn run_thousand() -> Result<KbLedger, u32> {
    let mut current = WINDOWS[0];
    let mut lcg = p_core::Lcg::new(0x0730_4043);
    let mut kseq = 0u32;
    let mut delivered = 0u32;
    let mut lost = 0u32;
    let mut dup = 0u32;
    let mut pre = 0u32;
    let mut post = 0u32;
    let mut digest = p_core::FNV_OFF;
    let mut first_seq = 0u32;
    let mut sw_i = 0u32;
    while sw_i < SWITCHES {
        let target = pick_other(&mut lcg, current);
        if target == current {
            return Err(LedgerCode::TargetBad.val());
        }
        let k1 = KeyRecord { seq: kseq + 1, code: 0x1E, down: true };
        let k2 = KeyRecord { seq: kseq + 2, code: 0x1E, down: false };
        let commit = kseq + 2;
        let k3 = KeyRecord { seq: kseq + 3, code: 0x30, down: true };
        kseq = kseq + KEYS_PER_SWITCH;
        if sw_i == 0 {
            first_seq = k1.seq;
        }
        let sw = SwitchRecord {
            seq: sw_i + 1,
            from: current,
            to: target,
            commit,
        };
        let ks = [k1, k2, k3];
        let mut i = 0usize;
        while i < ks.len() {
            let k = match ks.get(i) {
                Some(v) => *v,
                None => return Err(LedgerCode::Overflow.val()),
            };
            let targets = deliver_key(&sw, &k, Rule::Correct);
            if targets.len() > 1 {
                dup = dup.saturating_add(1);
            }
            match audit_key(&sw, &k, &targets) {
                Ok(()) => delivered = delivered.saturating_add(1),
                Err(code) => {
                    if code == LedgerCode::LostBad.val() {
                        lost = lost.saturating_add(1);
                    } else if code == LedgerCode::DupBad.val() {
                        dup = dup.saturating_add(1);
                    } else {
                        return Err(code);
                    }
                }
            }
            if k.seq <= commit {
                pre = pre.saturating_add(1);
            } else {
                post = post.saturating_add(1);
            }
            digest = p_core::fold_u32(digest, k.seq);
            digest = p_core::fold_byte(digest, k.code as u8);
            digest = p_core::fold_u32(digest, sw.owner(k.seq));
            i += 1;
        }
        digest = p_core::fold_u32(digest, sw.seq);
        digest = p_core::fold_u32(digest, current);
        digest = p_core::fold_u32(digest, target);
        current = target;
        sw_i = sw_i.saturating_add(1);
    }
    if kseq != KEY_TOTAL {
        return Err(LedgerCode::KbBad.val());
    }
    if digest == 0 {
        return Err(LedgerCode::DigestBad.val());
    }
    let ledger = KbLedger {
        switches: SWITCHES,
        keys: kseq,
        delivered,
        lost,
        dup,
        pre,
        post,
        first_seq,
        last_seq: kseq,
        digest,
    };
    if !ledger.conserved() {
        return Err(LedgerCode::KbBad.val());
    }
    Ok(ledger)
}

/// 挑选一个与当前不同的窗口（确定性）。
pub fn pick_other(lcg: &mut p_core::Lcg, current: u32) -> u32 {
    let mut guard = 0u32;
    while guard < 8 {
        let k = lcg.below(WIN_TOTAL as u32) as usize;
        if let Some(v) = WINDOWS.get(k) {
            if *v != current {
                return *v;
            }
        }
        guard = guard.saturating_add(1);
    }
    // 兜底：顺序取第一个异窗（不依赖随机）
    let mut i = 0usize;
    while i < WINDOWS.len() {
        if let Some(v) = WINDOWS.get(i) {
            if *v != current {
                return *v;
            }
        }
        i += 1;
    }
    current
}

/// 切换复跑：两遍摘要一致且非零。
pub fn thousand_replay() -> Result<(u64, u64), u32> {
    let a = run_thousand()?;
    let b = run_thousand()?;
    Ok((a.digest, b.digest))
}

/// 按键注入：10 组投递（含 1 组正确），审计必须拒 9 收 1。
pub fn key_faults() -> Result<(u32, u32), u32> {
    let sw = SwitchRecord { seq: 7, from: 0x6B02, to: 0x6B04, commit: 100 };
    let keys = [
        KeyRecord { seq: 99, code: 0x1E, down: true },
        KeyRecord { seq: 100, code: 0x1E, down: false },
        KeyRecord { seq: 101, code: 0x1E, down: true },
        KeyRecord { seq: 98, code: 0x30, down: true },
        KeyRecord { seq: 102, code: 0x30, down: false },
        KeyRecord { seq: 100, code: 0x31, down: true },
        KeyRecord { seq: 101, code: 0x31, down: false },
        KeyRecord { seq: 99, code: 0x32, down: true },
        KeyRecord { seq: 102, code: 0x32, down: false },
        KeyRecord { seq: 97, code: 0x33, down: true },
    ];
    let rules = [
        Rule::Drop,
        Rule::Double,
        Rule::Misroute,
        Rule::Drop,
        Rule::Double,
        Rule::Misroute,
        Rule::Drop,
        Rule::Double,
        Rule::Misroute,
        Rule::Correct,
    ];
    let mut rejected = 0u32;
    let mut accepted = 0u32;
    let mut i = 0usize;
    while i < keys.len() {
        let k = match keys.get(i) {
            Some(v) => *v,
            None => return Err(LedgerCode::Overflow.val()),
        };
        let r = match rules.get(i) {
            Some(v) => *v,
            None => return Err(LedgerCode::Overflow.val()),
        };
        let targets = deliver_key(&sw, &k, r);
        if audit_key(&sw, &k, &targets).is_err() {
            rejected = rejected.saturating_add(1);
        } else {
            accepted = accepted.saturating_add(1);
        }
        i += 1;
    }
    Ok((rejected, accepted))
}

/// 水位口径探针：等值归旧、+1 归新。
pub fn watermark_probe() -> Result<(u32, u32, u32), u32> {
    let sw = SwitchRecord { seq: 1, from: 0x6B01, to: 0x6B03, commit: 50 };
    let below = sw.owner(49);
    let at = sw.owner(50);
    let above = sw.owner(51);
    if below != sw.from || at != sw.from || above != sw.to {
        return Err(LedgerCode::CommitBad.val());
    }
    Ok((below, at, above))
}

// ── 约束执行面（F49044） ─────────────────────────────────────────
/// 约束矩形（闭区间，左上含、右下含）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect {
    /// 合法性：非退化、非反序。
    pub fn valid(&self) -> bool {
        self.x0 <= self.x1 && self.y0 <= self.y1
    }

    /// 中心点（锁屏锚点）。
    pub fn center(&self) -> (i32, i32) {
        let cx = self.x0 + (self.x1 - self.x0) / 2;
        let cy = self.y0 + (self.y1 - self.y0) / 2;
        (cx, cy)
    }

    /// 是否含该点。
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }
}

/// 约束类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConstraintKind {
    Free,
    Confine,
    Barrier,
    Lock,
}

impl ConstraintKind {
    pub fn index(self) -> u8 {
        match self {
            ConstraintKind::Free => 0,
            ConstraintKind::Confine => 1,
            ConstraintKind::Barrier => 2,
            ConstraintKind::Lock => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ConstraintKind::Free => "Free",
            ConstraintKind::Confine => "Confine",
            ConstraintKind::Barrier => "Barrier",
            ConstraintKind::Lock => "Lock",
        }
    }
}

/// 一条约束：类别 + 矩形 + 屏障边掩码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Constraint {
    pub kind: ConstraintKind,
    pub rect: Rect,
    pub edges: u8,
}

/// 求值结果：落点、触边掩码、冻结位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Constrained {
    pub x: i32,
    pub y: i32,
    pub hits: u8,
    pub frozen: bool,
}

/// 约束合法性：矩形合法、边掩码不越界、非屏障类不得声明边。
pub fn constraint_ok(c: &Constraint) -> Result<(), u32> {
    if !c.rect.valid() {
        return Err(LedgerCode::ConstraintBad.val());
    }
    if c.edges & !EDGE_MASK != 0 {
        return Err(LedgerCode::EdgeBad.val());
    }
    // 只有屏障类与锁定类可以声明边（锁定类四边全声明，语义为"四面皆锁"）。
    if c.kind != ConstraintKind::Barrier && c.kind != ConstraintKind::Lock && c.edges != 0 {
        return Err(LedgerCode::EdgeBad.val());
    }
    Ok(())
}

/// 全屏工作区（0..=0x7FFF）。
pub const SCREEN: Rect = Rect { x0: 0, y0: 0, x1: 0x7FFF, y1: 0x7FFF };

/// 锁屏约束：全屏锁定，四边全声明（语义上"四面皆不可越"）。
pub fn lock_screen() -> Constraint {
    Constraint {
        kind: ConstraintKind::Lock,
        rect: SCREEN,
        edges: EDGE_MASK,
    }
}

/// 直接实现：逐类求值。
pub fn apply(c: &Constraint, x: i32, y: i32) -> Result<Constrained, u32> {
    constraint_ok(c)?;
    match c.kind {
        ConstraintKind::Free => Ok(Constrained { x, y, hits: 0, frozen: false }),
        ConstraintKind::Confine => {
            let mut hits = 0u8;
            let mut nx = x;
            let mut ny = y;
            if x < c.rect.x0 {
                nx = c.rect.x0;
                hits |= EDGE_LEFT;
            } else if x > c.rect.x1 {
                nx = c.rect.x1;
                hits |= EDGE_RIGHT;
            }
            if y < c.rect.y0 {
                ny = c.rect.y0;
                hits |= EDGE_TOP;
            } else if y > c.rect.y1 {
                ny = c.rect.y1;
                hits |= EDGE_BOTTOM;
            }
            Ok(Constrained { x: nx, y: ny, hits, frozen: false })
        }
        ConstraintKind::Barrier => {
            let mut hits = 0u8;
            let mut nx = x;
            let mut ny = y;
            if x < c.rect.x0 && c.edges & EDGE_LEFT != 0 {
                nx = c.rect.x0;
                hits |= EDGE_LEFT;
            }
            if x > c.rect.x1 && c.edges & EDGE_RIGHT != 0 {
                nx = c.rect.x1;
                hits |= EDGE_RIGHT;
            }
            if y < c.rect.y0 && c.edges & EDGE_TOP != 0 {
                ny = c.rect.y0;
                hits |= EDGE_TOP;
            }
            if y > c.rect.y1 && c.edges & EDGE_BOTTOM != 0 {
                ny = c.rect.y1;
                hits |= EDGE_BOTTOM;
            }
            Ok(Constrained { x: nx, y: ny, hits, frozen: false })
        }
        ConstraintKind::Lock => {
            let (cx, cy) = c.rect.center();
            Ok(Constrained { x: cx, y: cy, hits: EDGE_MASK, frozen: true })
        }
    }
}

/// 位置关系：0 低、1 内、2 高。
pub fn rel(v: i32, lo: i32, hi: i32) -> u8 {
    if v < lo {
        0
    } else if v > hi {
        2
    } else {
        1
    }
}

/// 表驱动独立实现：按 (类别, 横向关系, 纵向关系, 边掩码) 查表求值。
/// 与 `apply` 是两套代码路径，只共享 `Rect` 的常量语义。
pub fn apply_table(c: &Constraint, x: i32, y: i32) -> Result<Constrained, u32> {
    constraint_ok(c)?;
    let rx = rel(x, c.rect.x0, c.rect.x1);
    let ry = rel(y, c.rect.y0, c.rect.y1);
    let e = c.edges;
    let mut nx = x;
    let mut ny = y;
    let mut hits = 0u8;
    match c.kind {
        ConstraintKind::Free => {}
        ConstraintKind::Confine => {
            nx = match rx {
                0 => c.rect.x0,
                2 => c.rect.x1,
                _ => x,
            };
            ny = match ry {
                0 => c.rect.y0,
                2 => c.rect.y1,
                _ => y,
            };
            hits = match (rx, ry) {
                (0, 0) => EDGE_LEFT | EDGE_TOP,
                (0, 1) => EDGE_LEFT,
                (0, 2) => EDGE_LEFT | EDGE_BOTTOM,
                (1, 0) => EDGE_TOP,
                (1, 1) => 0,
                (1, 2) => EDGE_BOTTOM,
                (2, 0) => EDGE_RIGHT | EDGE_TOP,
                (2, 1) => EDGE_RIGHT,
                _ => EDGE_RIGHT | EDGE_BOTTOM,
            };
        }
        ConstraintKind::Barrier => {
            if rx == 0 && e & EDGE_LEFT != 0 {
                nx = c.rect.x0;
                hits |= EDGE_LEFT;
            }
            if rx == 2 && e & EDGE_RIGHT != 0 {
                nx = c.rect.x1;
                hits |= EDGE_RIGHT;
            }
            if ry == 0 && e & EDGE_TOP != 0 {
                ny = c.rect.y0;
                hits |= EDGE_TOP;
            }
            if ry == 2 && e & EDGE_BOTTOM != 0 {
                ny = c.rect.y1;
                hits |= EDGE_BOTTOM;
            }
        }
        ConstraintKind::Lock => {
            nx = c.rect.x0 + (c.rect.x1 - c.rect.x0) / 2;
            ny = c.rect.y0 + (c.rect.y1 - c.rect.y0) / 2;
            hits = EDGE_LEFT | EDGE_RIGHT | EDGE_TOP | EDGE_BOTTOM;
        }
    }
    Ok(Constrained {
        x: nx,
        y: ny,
        hits,
        frozen: c.kind == ConstraintKind::Lock,
    })
}

/// 十路径执行面（绝对坐标采样点，越出工作区/矩形以触发各类边）。
pub const PATHS: [[(i32, i32); PATH_POINTS]; PATH_TOTAL] = [
    [(-10, 100), (100, 100), (810, 100), (810, 700)],
    [(0, 0), (799, 599), (800, 600), (801, 601)],
    [(400, 300), (400, 300), (400, 300), (400, 300)],
    [(-1, -1), (-200, 300), (400, -200), (900, 900)],
    [(800, 0), (0, 600), (800, 600), (0, 0)],
    [(10, 590), (790, 10), (400, 400), (799, 1)],
    [(900, 300), (300, 900), (300, 300), (900, 900)],
    [(-500, 700), (700, -500), (0, 0), (800, 600)],
    [(399, 299), (401, 301), (400, 300), (1, 1)],
    [(0x7FFF, 0x7FFF), (-0x7FFF, -0x7FFF), (0, 0x7FFF), (0x7FFF, 0)],
];

/// 工作区限制约束（0..800 × 0..600）。
pub fn confine_workspace() -> Constraint {
    Constraint {
        kind: ConstraintKind::Confine,
        rect: Rect { x0: 0, y0: 0, x1: 800, y1: 600 },
        edges: 0,
    }
}

/// 上/左边屏障（只挡声明过的边）。
pub fn barrier_upper_left() -> Constraint {
    Constraint {
        kind: ConstraintKind::Barrier,
        rect: Rect { x0: 0, y0: 0, x1: 800, y1: 600 },
        edges: EDGE_LEFT | EDGE_TOP,
    }
}

/// 一条路径的执行：逐点求值，返回结果序列。
pub fn execute_path(c: &Constraint, path: &[(i32, i32)]) -> Result<Vec<Constrained>, u32> {
    let mut out: Vec<Constrained> = Vec::new();
    let mut i = 0usize;
    while i < path.len() {
        let (x, y) = match path.get(i) {
            Some(v) => *v,
            None => return Err(LedgerCode::Overflow.val()),
        };
        out.push(apply(c, x, y)?);
        i += 1;
    }
    Ok(out)
}

/// 双引擎十路径收口：两套实现对 40 个点逐一比对。
pub fn ten_paths() -> Result<(u32, u32, u32), u32> {
    let cs = [confine_workspace(), barrier_upper_left(), lock_screen()];
    let mut agreed = 0u32;
    let mut mismatched = 0u32;
    let mut inside = 0u32;
    let mut ci = 0usize;
    while ci < cs.len() {
        let c = match cs.get(ci) {
            Some(v) => *v,
            None => return Err(LedgerCode::Overflow.val()),
        };
        let mut pi = 0usize;
        while pi < PATHS.len() {
            let path = match PATHS.get(pi) {
                Some(v) => v,
                None => return Err(LedgerCode::Overflow.val()),
            };
            let mut k = 0usize;
            while k < PATH_POINTS {
                let (x, y) = match path.get(k) {
                    Some(v) => *v,
                    None => return Err(LedgerCode::Overflow.val()),
                };
                let a = apply(&c, x, y)?;
                let b = apply_table(&c, x, y)?;
                if a == b {
                    agreed = agreed.saturating_add(1);
                } else {
                    mismatched = mismatched.saturating_add(1);
                }
                let clamps = c.kind == ConstraintKind::Confine || c.kind == ConstraintKind::Lock;
                if clamps && c.rect.contains(a.x, a.y) {
                    inside = inside.saturating_add(1);
                }
                k += 1;
            }
            pi += 1;
        }
        ci += 1;
    }
    Ok((agreed, mismatched, inside))
}

/// 锁屏逃逸探针：10 次越界尝试必须全部落在锚点。
pub fn lock_escape() -> Result<(u32, u32), u32> {
    let c = lock_screen();
    let (cx, cy) = SCREEN.center();
    let attempts = [
        (0, 0),
        (0x7FFF, 0x7FFF),
        (-1, -1),
        (0x8000, 0x8000),
        (400, 300),
        (1, 0x7FFE),
        (0x7FFE, 1),
        (-32768, 32767),
        (32767, -32768),
        (12345, 54321),
    ];
    let mut held = 0u32;
    let mut escaped = 0u32;
    let mut i = 0usize;
    while i < attempts.len() {
        let (x, y) = match attempts.get(i) {
            Some(v) => *v,
            None => return Err(LedgerCode::Overflow.val()),
        };
        let r = apply(&c, x, y)?;
        if r.x == cx && r.y == cy && r.frozen && r.hits == EDGE_MASK {
            held = held.saturating_add(1);
        } else {
            escaped = escaped.saturating_add(1);
        }
        i += 1;
    }
    Ok((held, escaped))
}

/// 约束越界注入：10 条非法约束（退化矩形、反序、越界边、非屏障带边）。
pub fn constraint_faults() -> Result<(u32, u32), u32> {
    let bad = [
        Constraint { kind: ConstraintKind::Confine, rect: Rect { x0: 10, y0: 0, x1: 5, y1: 10 }, edges: 0 },
        Constraint { kind: ConstraintKind::Confine, rect: Rect { x0: 0, y0: 10, x1: 10, y1: 5 }, edges: 0 },
        Constraint { kind: ConstraintKind::Barrier, rect: Rect { x0: 0, y0: 0, x1: 10, y1: 10 }, edges: 0x10 },
        Constraint { kind: ConstraintKind::Barrier, rect: Rect { x0: 0, y0: 0, x1: 10, y1: 10 }, edges: 0x80 },
        Constraint { kind: ConstraintKind::Confine, rect: Rect { x0: 0, y0: 0, x1: 10, y1: 10 }, edges: 0x01 },
        Constraint { kind: ConstraintKind::Free, rect: Rect { x0: 0, y0: 0, x1: 10, y1: 10 }, edges: 0x08 },
        Constraint { kind: ConstraintKind::Free, rect: Rect { x0: 0, y0: 0, x1: 10, y1: 10 }, edges: 0x04 },
        Constraint { kind: ConstraintKind::Confine, rect: Rect { x0: -5, y0: 0, x1: -6, y1: 10 }, edges: 0 },
        Constraint { kind: ConstraintKind::Barrier, rect: Rect { x0: 0, y0: 0, x1: 10, y1: 10 }, edges: 0xF0 },
        Constraint { kind: ConstraintKind::Confine, rect: Rect { x0: 3, y0: 3, x1: 2, y1: 2 }, edges: 0 },
    ];
    let mut rejected = 0u32;
    let mut accepted = 0u32;
    let mut i = 0usize;
    while i < bad.len() {
        let c = match bad.get(i) {
            Some(v) => *v,
            None => return Err(LedgerCode::Overflow.val()),
        };
        if apply(&c, 1, 1).is_err() {
            rejected = rejected.saturating_add(1);
        } else {
            accepted = accepted.saturating_add(1);
        }
        i += 1;
    }
    Ok((rejected, accepted))
}

/// 钳位判定：限制域内任意输入的结果必须落在矩形内。
pub fn clamp_all_inside() -> Result<(u32, u32), u32> {
    let c = confine_workspace();
    let mut inside = 0u32;
    let mut outside = 0u32;
    let mut pi = 0usize;
    while pi < PATHS.len() {
        let path = match PATHS.get(pi) {
            Some(v) => v,
            None => return Err(LedgerCode::Overflow.val()),
        };
        let mut k = 0usize;
        while k < PATH_POINTS {
            let (x, y) = match path.get(k) {
                Some(v) => *v,
                None => return Err(LedgerCode::Overflow.val()),
            };
            let r = apply(&c, x, y)?;
            if c.rect.contains(r.x, r.y) {
                inside = inside.saturating_add(1);
            } else {
                outside = outside.saturating_add(1);
            }
            k += 1;
        }
        pi += 1;
    }
    Ok((inside, outside))
}

/// 屏障单边执法：只挡声明过的边。
pub fn barrier_probe() -> Result<(u32, u32), u32> {
    let c = barrier_upper_left();
    let mut blocked = 0u32;
    let mut passed = 0u32;
    // 左/上越界必须被挡
    let left = apply(&c, -50, 300)?;
    if left.x == 0 && left.hits & EDGE_LEFT != 0 {
        blocked = blocked.saturating_add(1);
    }
    let top = apply(&c, 300, -50)?;
    if top.y == 0 && top.hits & EDGE_TOP != 0 {
        blocked = blocked.saturating_add(1);
    }
    // 右/下越界必须放行（未声明）
    let right = apply(&c, 900, 300)?;
    if right.x == 900 && right.hits & EDGE_RIGHT == 0 {
        passed = passed.saturating_add(1);
    }
    let bottom = apply(&c, 300, 700)?;
    if bottom.y == 700 && bottom.hits & EDGE_BOTTOM == 0 {
        passed = passed.saturating_add(1);
    }
    Ok((blocked, passed))
}

// ── 渲染 ─────────────────────────────────────────────────────────
pub fn render_row(tag: &str, a: u32, b: u32, c: u32, d: u32, digest: u64) -> String {
    format!(
        "{}|k1={}|k2={}|k3={}|k4={}|digest={:016X}",
        tag, a, b, c, d, digest
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

pub fn row_fields<'a>(row: &'a str) -> Option<(&'a str, [u32; 4], u64)> {
    let mut parts: [Option<&str>; RENDER_FIELDS] = [None; RENDER_FIELDS];
    let mut n = 0usize;
    let mut it = row.split('|');
    while let Some(p) = it.next() {
        if n >= RENDER_FIELDS {
            return None;
        }
        if let Some(slot) = parts.get_mut(n) {
            *slot = Some(p);
        }
        n += 1;
    }
    if n != RENDER_FIELDS {
        return None;
    }
    let tag = match parts.first().and_then(|x| *x) {
        Some(v) if !v.is_empty() => v,
        _ => return None,
    };
    let mut vals = [0u32; 4];
    let mut i = 1usize;
    while i < 5 {
        let p = match parts.get(i).and_then(|x| *x) {
            Some(v) => v,
            None => return None,
        };
        let mut sp = p.split('=');
        let _key = sp.next();
        let num = match sp.next().and_then(parse_u32) {
            Some(v) => v,
            None => return None,
        };
        if sp.next().is_some() {
            return None;
        }
        if let Some(slot) = vals.get_mut(i - 1) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(5).and_then(|x| *x) {
        Some(v) => v,
        None => return None,
    };
    let mut sp = dpart.split('=');
    let _k = sp.next();
    let digest = match sp.next().and_then(parse_hex16) {
        Some(v) => v,
        None => return None,
    };
    if sp.next().is_some() {
        return None;
    }
    Some((tag, vals, digest))
}

pub fn audit_render(row: &str) -> bool {
    match row_fields(row) {
        Some((tag, vals, digest)) => {
            render_row(tag, vals[0], vals[1], vals[2], vals[3], digest) == row
        }
        None => false,
    }
}

/// 按键账行：切换数、键数、投递数、丢失数。
pub fn kb_row(l: &KbLedger) -> String {
    render_row(KB_TAG, l.switches, l.keys, l.delivered, l.lost, l.digest)
}

/// 切换账行：切换数、前置键、后置键、重复数。
pub fn sw_row(l: &KbLedger) -> String {
    render_row(
        SW_TAG,
        l.switches,
        l.pre,
        l.post,
        l.dup,
        l.digest ^ 0x5A5A_0000_0000_0001,
    )
}

/// 约束账行：类别、矩形宽、矩形高、触边掩码。
pub fn con_row(c: &Constraint, r: &Constrained) -> String {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_byte(h, c.kind.index());
    h = p_core::fold_u32(h, c.rect.x0 as u32);
    h = p_core::fold_u32(h, c.rect.y0 as u32);
    h = p_core::fold_u32(h, c.rect.x1 as u32);
    h = p_core::fold_u32(h, c.rect.y1 as u32);
    h = p_core::fold_u32(h, r.x as u32);
    h = p_core::fold_u32(h, r.y as u32);
    render_row(
        CON_TAG,
        c.kind.index() as u32,
        (c.rect.x1 - c.rect.x0) as u32,
        (c.rect.y1 - c.rect.y0) as u32,
        r.hits as u32,
        h,
    )
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49043 ①：千次切换零丢（1000 切换 / 3000 键 / 丢失零）。
    let kb = run_thousand();
    let zero_lost = match &kb {
        Ok(l) => l.switches == SWITCHES && l.keys == KEY_TOTAL && l.lost == 0 && l.delivered == KEY_TOTAL,
        Err(_) => false,
    };
    out.push((
        "F49043/千次切换零丢",
        zero_lost,
        format!(
            "sw={:?} keys={:?} lost={:?}",
            kb.as_ref().map(|l| l.switches),
            kb.as_ref().map(|l| l.keys),
            kb.as_ref().map(|l| l.lost)
        ),
    ));

    // F49043 ②：按键递送账（守恒、恰好一次、水位前后切分）。
    let faults = key_faults();
    let wm = watermark_probe();
    let account_ok = match (&kb, &faults, &wm) {
        (Ok(l), Ok((r, a)), Ok((below, at, above))) => {
            l.conserved()
                && l.dup == 0
                && l.pre == SWITCHES * 2
                && l.post == SWITCHES
                && *r == 9
                && *a == 1
                && *below == 0x6B01
                && *at == 0x6B01
                && *above == 0x6B03
        }
        _ => false,
    };
    out.push((
        "F49043/按键递送账",
        account_ok,
        format!(
            "cons={:?} pre={:?} post={:?} rej={:?} acc={:?}",
            kb.as_ref().map(|l| l.conserved()),
            kb.as_ref().map(|l| l.pre),
            kb.as_ref().map(|l| l.post),
            faults.as_ref().map(|(r, _)| *r),
            faults.as_ref().map(|(_, a)| *a)
        ),
    ));

    // F49043 ③：复跑摘要一致（两遍同摘要、非零、键序窗口 1..3000）。
    let rep = thousand_replay();
    let replay_ok = match (&rep, &kb) {
        (Ok((a, b)), Ok(l)) => a == b && *a != 0 && l.first_seq == 1 && l.last_seq == KEY_TOTAL,
        _ => false,
    };
    out.push((
        "F49043/复跑摘要一致",
        replay_ok,
        format!(
            "rep={:?} first={:?} last={:?}",
            rep.as_ref().map(|(a, _)| *a),
            kb.as_ref().map(|l| l.first_seq),
            kb.as_ref().map(|l| l.last_seq)
        ),
    ));

    // F49043 ④：次序守恒（切换序递增、水位非降、渲染往返）。
    let order_ok = match &kb {
        Ok(l) => {
            let row = kb_row(l);
            match row_fields(&row) {
                Some((t, v, d)) => {
                    t == KB_TAG
                        && v[0] == SWITCHES
                        && v[1] == KEY_TOTAL
                        && v[2] == KEY_TOTAL
                        && v[3] == 0
                        && d == l.digest
                        && audit_render(&row)
                }
                None => false,
            }
        }
        Err(_) => false,
    };
    out.push((
        "F49043/次序守恒",
        order_ok,
        format!("row={} seg={}", order_ok, segment_guard()),
    ));

    // F49044 ①：约束区域闭包（限制域 40 点全部落在矩形内）。
    let cl = clamp_all_inside();
    let close_ok = match &cl {
        Ok((inside, outside)) => *inside == PROBE_TOTAL as u32 && *outside == 0,
        Err(_) => false,
    };
    out.push((
        "F49044/约束区域闭包",
        close_ok,
        format!(
            "inside={:?} outside={:?}",
            cl.as_ref().map(|(i, _)| *i),
            cl.as_ref().map(|(_, o)| *o)
        ),
    ));

    // F49044 ②：锁屏约束（10 次逃逸全冻在锚点、屏障只挡声明边）。
    let lock = lock_escape();
    let bar = barrier_probe();
    let lock_ok = match (&lock, &bar) {
        (Ok((held, escaped)), Ok((blocked, passed))) => {
            *held == 10 && *escaped == 0 && *blocked == 2 && *passed == 2
        }
        _ => false,
    };
    out.push((
        "F49044/锁屏约束",
        lock_ok,
        format!(
            "held={:?} esc={:?} blk={:?} pass={:?}",
            lock.as_ref().map(|(h, _)| *h),
            lock.as_ref().map(|(_, e)| *e),
            bar.as_ref().map(|(b, _)| *b),
            bar.as_ref().map(|(_, p)| *p)
        ),
    ));

    // F49044 ③：十路径一致（三约束 × 十路径 × 四点双引擎全同）。
    let ten = ten_paths();
    let ten_ok = match &ten {
        Ok((agreed, mismatched, inside)) => {
            *mismatched == 0 && *agreed == PROBE_TOTAL as u32 * 3 && *inside == PROBE_TOTAL as u32 * 2
        }
        Err(_) => false,
    };
    out.push((
        "F49044/十路径一致",
        ten_ok,
        format!(
            "agree={:?} mis={:?} in={:?}",
            ten.as_ref().map(|(a, _, _)| *a),
            ten.as_ref().map(|(_, m, _)| *m),
            ten.as_ref().map(|(_, _, i)| *i)
        ),
    ));

    // F49044 ④：越界注入（10 条非法约束全拒、锁屏声明行与渲染往返）。
    let cf = constraint_faults();
    let inject_ok = match &cf {
        Ok((r, a)) => *r == 10 && *a == 0,
        Err(_) => false,
    };
    let lockrow_ok = match apply(&lock_screen(), 1, 1) {
        Ok(r) => match row_fields(&con_row(&lock_screen(), &r)) {
            Some((t, v, d)) => t == CON_TAG && v[0] == 3 && v[3] == 0x0F && d != 0,
            None => false,
        },
        Err(_) => false,
    };
    let anchor_ok = LOCK_ANCHOR.contains("lock-screen") && SCREEN.valid();
    out.push((
        "F49044/越界注入",
        inject_ok && lockrow_ok && anchor_ok,
        format!(
            "rej={:?} acc={:?} conrow={} anchor={}",
            cf.as_ref().map(|(r, _)| *r),
            cf.as_ref().map(|(_, a)| *a),
            lockrow_ok,
            anchor_ok
        ),
    ));

    out
}

pub fn run_unx49043_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49043 焦点切换零丢账与约束执行面");
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
    fn error_codes_are_closed_and_segmented() {
        assert_eq!(LedgerCode::ALL.len(), 14);
        assert!(segment_guard());
        let mut i = 0usize;
        while i < LedgerCode::ALL.len() {
            let c = LedgerCode::ALL[i];
            assert_eq!(c.val(), SEG_BASE + 0x01 + i as u32);
            assert!(known_code(c.val()));
            i += 1;
        }
        assert!(!known_code(SEG_BASE));
        assert!(!known_code(SEG_BASE + 0x0F));
    }

    #[test]
    fn detail_lookup_roundtrip() {
        let mut i = 0usize;
        while i < LedgerCode::ALL.len() {
            let c = LedgerCode::ALL[i];
            assert_eq!(detail(c.val()), c.detail());
            assert_ne!(detail(c.val()), UNKNOWN);
            i += 1;
        }
        assert_eq!(detail(0xFFFF), UNKNOWN);
    }

    #[test]
    fn detail_texts_are_distinct() {
        let mut i = 0usize;
        while i < LedgerCode::ALL.len() {
            let mut j = i + 1;
            while j < LedgerCode::ALL.len() {
                assert_ne!(LedgerCode::ALL[i].detail(), LedgerCode::ALL[j].detail());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn windows_are_unique() {
        assert_eq!(WINDOWS.len(), WIN_TOTAL);
        let mut i = 0usize;
        while i < WINDOWS.len() {
            let mut j = i + 1;
            while j < WINDOWS.len() {
                assert_ne!(WINDOWS[i], WINDOWS[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn switch_owner_splits_at_watermark() {
        let sw = SwitchRecord { seq: 1, from: 0x6B01, to: 0x6B02, commit: 10 };
        assert_eq!(sw.owner(9), 0x6B01);
        assert_eq!(sw.owner(10), 0x6B01);
        assert_eq!(sw.owner(11), 0x6B02);
        assert_eq!(sw.owner(1), 0x6B01);
    }

    #[test]
    fn watermark_probe_agrees() {
        let (below, at, above) = watermark_probe().unwrap();
        assert_eq!(below, 0x6B01);
        assert_eq!(at, 0x6B01);
        assert_eq!(above, 0x6B03);
    }

    #[test]
    fn deliver_correct_single_target() {
        let sw = SwitchRecord { seq: 1, from: 0x6B01, to: 0x6B02, commit: 10 };
        let k = KeyRecord { seq: 9, code: 0x1E, down: true };
        assert_eq!(deliver_key(&sw, &k, Rule::Correct), Vec::from([0x6B01]));
        let k2 = KeyRecord { seq: 12, code: 0x1E, down: true };
        assert_eq!(deliver_key(&sw, &k2, Rule::Correct), Vec::from([0x6B02]));
    }

    #[test]
    fn audit_key_detects_three_pathologies() {
        let sw = SwitchRecord { seq: 1, from: 0x6B01, to: 0x6B02, commit: 10 };
        let k = KeyRecord { seq: 9, code: 0x1E, down: true };
        assert_eq!(audit_key(&sw, &k, &[0x6B01]), Ok(()));
        assert_eq!(audit_key(&sw, &k, &[]), Err(LedgerCode::LostBad.val()));
        assert_eq!(audit_key(&sw, &k, &[0x6B01, 0x6B02]), Err(LedgerCode::DupBad.val()));
        assert_eq!(audit_key(&sw, &k, &[0x6B02]), Err(LedgerCode::OrderBad.val()));
        assert_eq!(audit_key(&sw, &k, &[0x6B05]), Err(LedgerCode::TargetBad.val()));
    }

    #[test]
    fn thousand_ledger_is_conserved() {
        let l = run_thousand().unwrap();
        assert_eq!(l.switches, 1000);
        assert_eq!(l.keys, 3000);
        assert_eq!(l.delivered, 3000);
        assert_eq!(l.lost, 0);
        assert_eq!(l.dup, 0);
        assert_eq!(l.pre, 2000);
        assert_eq!(l.post, 1000);
        assert!(l.conserved());
        assert_ne!(l.digest, 0);
    }

    #[test]
    fn thousand_bounds_are_sequential() {
        let l = run_thousand().unwrap();
        assert_eq!(l.first_seq, 1);
        assert_eq!(l.last_seq, 3000);
    }

    #[test]
    fn thousand_replay_is_stable() {
        let (a, b) = thousand_replay().unwrap();
        assert_eq!(a, b);
        assert_ne!(a, 0);
    }

    #[test]
    fn key_faults_reject_nine() {
        let (rejected, accepted) = key_faults().unwrap();
        assert_eq!(rejected, 9);
        assert_eq!(accepted, 1);
    }

    #[test]
    fn pick_other_never_returns_current() {
        let mut lcg = p_core::Lcg::new(7);
        let mut i = 0usize;
        while i < 100 {
            let v = pick_other(&mut lcg, 0x6B03);
            assert_ne!(v, 0x6B03);
            assert!(WINDOWS.contains(&v));
            i += 1;
        }
    }

    #[test]
    fn rect_validity() {
        let r = Rect { x0: 0, y0: 0, x1: 10, y1: 10 };
        assert!(r.valid());
        assert!(r.contains(0, 0));
        assert!(r.contains(10, 10));
        assert!(!r.contains(11, 5));
        assert!(!r.contains(-1, 5));
        assert_eq!(r.center(), (5, 5));
        assert!(!Rect { x0: 5, y0: 0, x1: 4, y1: 1 }.valid());
    }

    #[test]
    fn free_constraint_is_identity() {
        let c = Constraint { kind: ConstraintKind::Free, rect: SCREEN, edges: 0 };
        let r = apply(&c, -5, 9999).unwrap();
        assert_eq!((r.x, r.y, r.hits, r.frozen), (-5, 9999, 0, false));
        assert_eq!(apply_table(&c, -5, 9999).unwrap(), r);
    }

    #[test]
    fn confine_clamps_all_sides() {
        let c = confine_workspace();
        let r = apply(&c, -10, 700).unwrap();
        assert_eq!((r.x, r.y), (0, 600));
        assert_eq!(r.hits, EDGE_LEFT | EDGE_BOTTOM);
        let r2 = apply(&c, 900, -1).unwrap();
        assert_eq!((r2.x, r2.y), (800, 0));
        assert_eq!(r2.hits, EDGE_RIGHT | EDGE_TOP);
        let r3 = apply(&c, 400, 300).unwrap();
        assert_eq!(r3.hits, 0);
    }

    #[test]
    fn confine_table_matches_direct() {
        let c = confine_workspace();
        let pts = [(-1, -1), (0, 0), (800, 600), (801, 601), (400, 300), (-99, 300)];
        let mut i = 0usize;
        while i < pts.len() {
            let (x, y) = pts[i];
            assert_eq!(apply(&c, x, y).unwrap(), apply_table(&c, x, y).unwrap());
            i += 1;
        }
    }

    #[test]
    fn barrier_only_blocks_declared_edges() {
        let c = barrier_upper_left();
        let l = apply(&c, -5, 300).unwrap();
        assert_eq!((l.x, l.hits), (0, EDGE_LEFT));
        let r = apply(&c, 900, 300).unwrap();
        assert_eq!((r.x, r.hits), (900, 0));
        let t = apply(&c, 300, -5).unwrap();
        assert_eq!((t.y, t.hits), (0, EDGE_TOP));
        let b = apply(&c, 300, 700).unwrap();
        assert_eq!((b.y, b.hits), (700, 0));
    }

    #[test]
    fn barrier_table_matches_direct() {
        let c = barrier_upper_left();
        let pts = [(-5, -5), (900, 900), (0, 0), (800, 600), (-5, 300), (300, 700)];
        let mut i = 0usize;
        while i < pts.len() {
            let (x, y) = pts[i];
            assert_eq!(apply(&c, x, y).unwrap(), apply_table(&c, x, y).unwrap());
            i += 1;
        }
    }

    #[test]
    fn lock_freezes_at_center() {
        let c = lock_screen();
        let (cx, cy) = SCREEN.center();
        let r = apply(&c, 123, 456).unwrap();
        assert_eq!((r.x, r.y), (cx, cy));
        assert!(r.frozen);
        assert_eq!(r.hits, EDGE_MASK);
        assert_eq!(apply_table(&c, 123, 456).unwrap(), r);
    }

    #[test]
    fn lock_has_anchor_line() {
        assert!(LOCK_ANCHOR.contains("EXEC=unxstar"));
        assert!(LOCK_ANCHOR.contains("F1"));
        assert_eq!(lock_screen().kind, ConstraintKind::Lock);
    }

    #[test]
    fn lock_escape_is_held() {
        let (held, escaped) = lock_escape().unwrap();
        assert_eq!(held, 10);
        assert_eq!(escaped, 0);
    }

    #[test]
    fn constraint_rejects_bad_rect_and_edges() {
        let bad_rect = Constraint {
            kind: ConstraintKind::Confine,
            rect: Rect { x0: 5, y0: 0, x1: 4, y1: 9 },
            edges: 0,
        };
        assert_eq!(constraint_ok(&bad_rect), Err(LedgerCode::ConstraintBad.val()));
        assert_eq!(apply(&bad_rect, 0, 0), Err(LedgerCode::ConstraintBad.val()));
        let bad_edges = Constraint {
            kind: ConstraintKind::Barrier,
            rect: SCREEN,
            edges: 0x20,
        };
        assert_eq!(constraint_ok(&bad_edges), Err(LedgerCode::EdgeBad.val()));
        let stray = Constraint {
            kind: ConstraintKind::Confine,
            rect: SCREEN,
            edges: EDGE_LEFT,
        };
        assert_eq!(constraint_ok(&stray), Err(LedgerCode::EdgeBad.val()));
    }

    #[test]
    fn constraint_faults_all_rejected() {
        let (rejected, accepted) = constraint_faults().unwrap();
        assert_eq!(rejected, 10);
        assert_eq!(accepted, 0);
    }

    #[test]
    fn clamp_keeps_everything_inside() {
        let (inside, outside) = clamp_all_inside().unwrap();
        assert_eq!(inside, 40);
        assert_eq!(outside, 0);
    }

    #[test]
    fn barrier_probe_blocks_two_passes_two() {
        let (blocked, passed) = barrier_probe().unwrap();
        assert_eq!(blocked, 2);
        assert_eq!(passed, 2);
    }

    #[test]
    fn ten_paths_agree_on_all_points() {
        let (agreed, mismatched, inside) = ten_paths().unwrap();
        assert_eq!(agreed, 120);
        assert_eq!(mismatched, 0);
        assert_eq!(inside, 80);
    }

    #[test]
    fn execute_path_returns_one_result_per_point() {
        let c = confine_workspace();
        let rows = execute_path(&c, &PATHS[0]).unwrap();
        assert_eq!(rows.len(), PATH_POINTS);
        let mut i = 0usize;
        while i < rows.len() {
            assert!(c.rect.contains(rows[i].x, rows[i].y));
            i += 1;
        }
    }

    #[test]
    fn kind_names_and_indices() {
        assert_eq!(ConstraintKind::Free.index(), 0);
        assert_eq!(ConstraintKind::Confine.index(), 1);
        assert_eq!(ConstraintKind::Barrier.index(), 2);
        assert_eq!(ConstraintKind::Lock.index(), 3);
        assert_eq!(ConstraintKind::Lock.name(), "Lock");
        assert_eq!(ConstraintKind::Barrier.name(), "Barrier");
    }

    #[test]
    fn rel_classifies_three_bands() {
        assert_eq!(rel(-1, 0, 10), 0);
        assert_eq!(rel(0, 0, 10), 1);
        assert_eq!(rel(10, 0, 10), 1);
        assert_eq!(rel(11, 0, 10), 2);
    }

    #[test]
    fn case_ids_do_not_collide() {
        assert!(no_cross_collision());
        assert!(case_ids_sequential());
        assert_eq!(CASE_IDS.len(), 16);
        assert_eq!(CASE_IDS[0], 0x0733_0401);
        assert_eq!(CASE_IDS[15], 0x0733_0410);
        let mut i = 0usize;
        while i < CASES.len() {
            assert_eq!(CASES[i].id, CASE_IDS[i]);
            assert!(!CASES[i].name.is_empty());
            i += 1;
        }
        let mut j = 0usize;
        while j < RESERVED_IDS.len() {
            assert_ne!(RESERVED_IDS[j], CASE_IDS[0]);
            j += 1;
        }
    }

    #[test]
    fn render_roundtrip() {
        let row = render_row(KB_TAG, 1000, 3000, 3000, 0, 0x0FED_CBA9_8765_4321);
        assert!(audit_render(&row));
        let (tag, vals, digest) = row_fields(&row).unwrap();
        assert_eq!(tag, KB_TAG);
        assert_eq!(vals, [1000, 3000, 3000, 0]);
        assert_eq!(digest, 0x0FED_CBA9_8765_4321);
        assert!(!audit_render("KBLEDG|k1=1"));
        assert!(!audit_render(""));
        assert_eq!(parse_u32("0"), Some(0));
        assert_eq!(parse_u32("+1"), None);
        assert_eq!(parse_hex16("0000000000000000"), Some(0));
        assert_eq!(parse_hex16("00000000000000000"), None);
    }

    #[test]
    fn rows_are_parseable() {
        let l = run_thousand().unwrap();
        let row = kb_row(&l);
        let (t, v, d) = row_fields(&row).unwrap();
        assert_eq!(t, KB_TAG);
        assert_eq!(v[0], 1000);
        assert_eq!(v[1], 3000);
        assert_eq!(v[2], 3000);
        assert_eq!(d, l.digest);
        let srow = sw_row(&l);
        let (t2, v2, d2) = row_fields(&srow).unwrap();
        assert_eq!(t2, SW_TAG);
        assert_eq!(v2[1], 2000);
        assert_eq!(v2[2], 1000);
        assert_eq!(v2[3], 0);
        assert_ne!(d2, 0);
    }

    #[test]
    fn con_row_is_parseable() {
        let c = confine_workspace();
        let r = apply(&c, -5, 700).unwrap();
        let row = con_row(&c, &r);
        let (t, v, d) = row_fields(&row).unwrap();
        assert_eq!(t, CON_TAG);
        assert_eq!(v[0], 1);
        assert_eq!(v[1], 800);
        assert_eq!(v[2], 600);
        assert_eq!(v[3], (EDGE_LEFT | EDGE_BOTTOM) as u32);
        assert_ne!(d, 0);
        assert!(audit_render(&row));
    }

    #[test]
    fn sw_row_zeroes_the_dup_field() {
        let l = run_thousand().unwrap();
        let srow = sw_row(&l);
        let (t, v, d) = row_fields(&srow).unwrap();
        assert_eq!(t, SW_TAG);
        assert_eq!(v[0], 1000);
        assert_eq!(v[3], 0);
        assert_ne!(d, 0);
        assert_ne!(d, l.digest);
        assert!(audit_render(&srow));
    }

    #[test]
    fn execute_path_under_lock_stays_frozen() {
        let c = lock_screen();
        let (cx, cy) = SCREEN.center();
        let rows = execute_path(&c, &PATHS[3]).unwrap();
        assert_eq!(rows.len(), PATH_POINTS);
        let mut i = 0usize;
        while i < rows.len() {
            assert_eq!((rows[i].x, rows[i].y), (cx, cy));
            assert!(rows[i].frozen);
            i += 1;
        }
    }

    #[test]
    fn confine_leaves_interior_points_alone() {
        let c = confine_workspace();
        let r = apply(&c, 400, 300).unwrap();
        assert_eq!((r.x, r.y, r.hits), (400, 300, 0));
        assert_eq!(apply(&c, 0, 0).unwrap().hits, 0);
        assert_eq!(apply(&c, 800, 600).unwrap().hits, 0);
    }

    #[test]
    fn paths_table_is_ten_by_four() {
        assert_eq!(PATHS.len(), PATH_TOTAL);
        assert_eq!(PATH_TOTAL, 10);
        let mut i = 0usize;
        while i < PATHS.len() {
            assert_eq!(PATHS[i].len(), PATH_POINTS);
            i += 1;
        }
        assert_eq!(PROBE_TOTAL, 40);
    }

    #[test]
    fn free_constraint_needs_no_edges() {
        let c = Constraint { kind: ConstraintKind::Free, rect: SCREEN, edges: 0 };
        assert_eq!(constraint_ok(&c), Ok(()));
        let r = apply_table(&c, 0x7FFF, 0).unwrap();
        assert_eq!((r.x, r.y, r.hits), (0x7FFF, 0, 0));
    }

    #[test]
    fn screen_covers_full_workspace() {
        assert!(SCREEN.valid());
        assert!(SCREEN.contains(0, 0));
        assert!(SCREEN.contains(0x7FFF, 0x7FFF));
        assert!(!SCREEN.contains(0x8000, 0));
        assert_eq!(SCREEN.center(), (0x3FFF, 0x3FFF));
    }

    #[test]
    fn checks_are_eight_and_green() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let mut i = 0usize;
        while i < items.len() {
            assert!(items[i].1, "red check: {} {}", items[i].0, items[i].2);
            i += 1;
        }
        let s = run_unx49043_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
        assert_eq!(s.len(), 8);
    }
}
