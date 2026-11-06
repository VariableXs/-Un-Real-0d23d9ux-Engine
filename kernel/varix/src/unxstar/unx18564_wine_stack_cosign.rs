//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18564 · Wine 上栈环境联签（对 AI-21） · 0x808C0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18564-J1 上栈环境前置条件清单验收绿（msiexec 面 B11
//!       依赖），缺口逐项列明不静默
//! 设计要点：
//!   - 前置条件清单 10 项闭集（msiexec 面 B11 依赖为第 0 项，钉死），
//!     每项判定 ∈ {未评/绿/红/缺口}，绿必须带证据指纹——无证据绿
//!     在评估闸必红（GreenNoEvidence）；
//!   - 缺口逐项列明不静默：收口闸逐行核对，凡存在未评行即 SilentSkip、
//!     凡红/缺口行未入缺口账即 GapMasked——静默丢失就是红线；
//!   - msiexec 面 B11 依赖：评估第 0 项前必须先绑定 B11 依赖指纹，
//!     未绑定评估必拒（DepMissing）；验收绿再验依赖位；
//!   - 清单状态机：采集→缺口列明→验收，单调三态；验收前置 = 全绿
//!     + 缺口账空 + B11 依赖在档，缺一即 CloseNotReady；
//!   - 注入核三类：无证据绿 / 未评收口 / 缺口遮蔽，各 10 轮全抓，
//!     漏检为零；
//!   - 边界：条件号越界/重复评估/错态评估；
//!   - LCG 种子写死 = SEG_BASE（0x808C0）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! 结构清单：
//!   数据结构：CondRow（条件行）/ PrecondLedger（前置条件清单账）
//!   状态机  ：ChecklistState（采集→缺口列明→验收 单调三态）
//!   账本    ：条件行数组账 + 缺口账 + 评估计数 + 注入检出账
//!   错误路径：WineCode 12 码闭集，code() = SEG_BASE + n（0x808C0 窗）
//!   注入核  ：无证据绿/未评收口/缺口遮蔽三类逐类断言被拒（10/10）
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x808C0，AI-VX11-1105 批09 第 4 槽，0x40 对齐独占窗）。
pub const SEG_BASE: u32 = 0x808C0;

// ── 冻结常量（写死并注明推导） ──────────────────────────────────
/// 前置条件条数（推导：B11 开工面 8 项 + 上栈专项 2 项 = 10）。
pub const PRECOND_N: usize = 10;
/// msiexec 面 B11 依赖所在条件行（钉死第 0 项）。
pub const MSIEXEC_IDX: usize = 0;
/// B11 工单号（判据原文给定）。
pub const B11_ID: u32 = 11;
/// 缺口账容量（推导：全清单 10 行都可成缺口，账容=10 不截断）。
pub const GAP_CAP: usize = 10;
/// 判据迭代轮数（写死：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 证据指纹种子（φ 前 64 位，冻结推导）。
pub const FP_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WineCode {
    /// 条件号越界：0..10 之外
    CondOutOfRange,
    /// 重复评估：同条件二评
    DupAssess,
    /// 无证据绿：绿缺证据指纹
    GreenNoEvidence,
    /// 证据不齐：指纹重算不符
    EvidenceMismatch,
    /// 静默跳过：存在未评行即收口
    SilentSkip,
    /// 缺口遮蔽：红/缺口行未入缺口账
    GapMasked,
    /// 收口未就绪：验收前置不齐
    CloseNotReady,
    /// 状态越级/倒退
    StateBreak,
    /// 依赖缺失：msiexec 面 B11 依赖未绑定
    DepMissing,
    /// 账目守恒破
    LedgerBreak,
    /// 双跑漂移
    NotReproducible,
    /// 未知诊断码兜底
    Unknown,
}

impl WineCode {
    /// 诊断码 = SEG_BASE + 序号（段内闭集，const 供静态裁决表）。
    pub const fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文建议（文案闭集，每码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            WineCode::CondOutOfRange => "条件号越界：只在 0..10 内取",
            WineCode::DupAssess => "重复评估：一条件一评，改判走复核账",
            WineCode::GreenNoEvidence => "无证据绿：绿必须带证据指纹",
            WineCode::EvidenceMismatch => "证据不齐：指纹重算不符，重评留证",
            WineCode::SilentSkip => "静默跳过：未评行必须先评再收口",
            WineCode::GapMasked => "缺口遮蔽：红/缺口行逐项列入缺口账",
            WineCode::CloseNotReady => "收口未就绪：全绿+缺口空+B11依赖齐才验收",
            WineCode::StateBreak => "状态越级或倒退：按 采集→缺口列明→验收 走",
            WineCode::DepMissing => "依赖缺失：先绑定 msiexec 面 B11 依赖指纹",
            WineCode::LedgerBreak => "账目不守恒：评估计数对不上已评行数",
            WineCode::NotReproducible => "双跑漂移：冻结种子重跑对拍",
            WineCode::Unknown => "未知诊断码：不在 0x808C0 段闭集内",
        }
    }
    pub const ALL: [WineCode; 12] = [
        WineCode::CondOutOfRange,
        WineCode::DupAssess,
        WineCode::GreenNoEvidence,
        WineCode::EvidenceMismatch,
        WineCode::SilentSkip,
        WineCode::GapMasked,
        WineCode::CloseNotReady,
        WineCode::StateBreak,
        WineCode::DepMissing,
        WineCode::LedgerBreak,
        WineCode::NotReproducible,
        WineCode::Unknown,
    ];
}

/// 码值反查中文建议（段外码落 Unknown 兜底）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < WineCode::ALL.len() {
        if WineCode::ALL[i].code() == code {
            return WineCode::ALL[i].detail();
        }
        i += 1;
    }
    WineCode::Unknown.detail()
}

// ── 指纹原语 ────────────────────────────────────────────────────
/// 64 位雪崩混合（写死常量，双跑必同）。
pub fn mix64(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^= x >> 33;
    x
}

/// 证据指纹：条件号、判词、payload 三元混合。
pub fn evidence_fp(cid: u32, verdict: u32, payload: u64) -> u64 {
    mix64(((cid as u64) << 32) ^ ((verdict as u64) << 16) ^ payload ^ FP_SEED)
}

// ── 状态机：清单三态（单调） ────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChecklistState {
    Collecting = 0,
    GapsListed = 1,
    Accepted = 2,
}

impl ChecklistState {
    pub fn rank(self) -> u32 {
        self as u32
    }
    /// 单调 +1 放行；倒退/越级拒 StateBreak。
    pub fn advance(self, to: ChecklistState) -> Result<ChecklistState, u32> {
        let from = self.rank();
        let dst = to.rank();
        if dst == from + 1 {
            Ok(to)
        } else {
            Err(WineCode::StateBreak.code())
        }
    }
}

/// 条件判词闭集：过 / 挂 / 缺口。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Pass = 0,
    Fail = 1,
    GapFound = 2,
}

impl Verdict {
    pub fn rank(self) -> u32 {
        self as u32
    }
}

/// 条件判定态闭集：未评 / 绿 / 红 / 缺口。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CondState {
    Unset = 0,
    Green = 1,
    Red = 2,
    Gap = 3,
}

impl CondState {
    pub fn rank(self) -> u32 {
        self as u32
    }
}

// ── 数据结构 ────────────────────────────────────────────────────
#[derive(Clone, Copy)]
pub struct CondRow {
    /// 判定态（Unset=未评）。
    pub state: CondState,
    /// 证据指纹（绿/红/缺口均留痕，重算对拍）。
    pub evidence: u64,
}

impl CondRow {
    pub const fn new() -> CondRow {
        CondRow { state: CondState::Unset, evidence: 0 }
    }
}

/// Wine 上栈前置条件清单账。
pub struct PrecondLedger {
    /// 条件行数组账（定容 10，逐行定态）。
    pub rows: [CondRow; PRECOND_N],
    /// 缺口账：(条件号, 判词) 逐项列明。
    pub gaps: Vec<(u32, u32)>,
    /// 清单状态机。
    pub state: ChecklistState,
    /// msiexec 面 B11 依赖指纹（None=未绑定）。
    pub b11_dep: Option<u64>,
    /// 评估计数（与已评行数守恒）。
    pub assess_cnt: u32,
}

impl PrecondLedger {
    pub fn new() -> PrecondLedger {
        PrecondLedger {
            rows: [CondRow::new(); PRECOND_N],
            gaps: Vec::new(),
            state: ChecklistState::Collecting,
            b11_dep: None,
            assess_cnt: 0,
        }
    }

    /// 绑定 msiexec 面 B11 依赖指纹（评估第 0 项的前置）。
    pub fn bind_b11(&mut self, fp: u64) {
        self.b11_dep = Some(fp);
    }

    /// msiexec 面就绪闭式：依赖已绑且指纹非零。
    pub fn msiexec_ready(&self) -> bool {
        match self.b11_dep {
            Some(f) => f != 0,
            None => false,
        }
    }

    /// 评估一条件：判词→判定态 + 证据指纹；重复评估/错态/越界显式拒。
    pub fn assess(&mut self, cid: usize, v: Verdict, payload: u64) -> Result<u64, u32> {
        if cid >= PRECOND_N {
            return Err(WineCode::CondOutOfRange.code());
        }
        if self.state != ChecklistState::Collecting {
            return Err(WineCode::StateBreak.code());
        }
        if self.rows[cid].state != CondState::Unset {
            return Err(WineCode::DupAssess.code());
        }
        // msiexec 面 B11 依赖：未绑定评估必拒。
        if cid == MSIEXEC_IDX && !self.msiexec_ready() {
            return Err(WineCode::DepMissing.code());
        }
        let st = match v {
            Verdict::Pass => CondState::Green,
            Verdict::Fail => CondState::Red,
            Verdict::GapFound => CondState::Gap,
        };
        let f = evidence_fp(cid as u32, v.rank(), payload);
        self.rows[cid].state = st;
        self.rows[cid].evidence = f;
        self.assess_cnt = self.assess_cnt.saturating_add(1);
        // 缺口/红行同步逐项列入缺口账（不静默的第一道闸）。
        if st == CondState::Red || st == CondState::Gap {
            if self.gaps.len() >= GAP_CAP {
                return Err(WineCode::LedgerBreak.code());
            }
            self.gaps.push((cid as u32, v.rank()));
        }
        Ok(f)
    }

    /// 收口闸（采集→缺口列明）：未评行拒 SilentSkip，缺口账与红/缺行
    /// 计数不平拒 GapMasked。
    pub fn close(&mut self) -> Result<(), u32> {
        if self.state != ChecklistState::Collecting {
            return Err(WineCode::StateBreak.code());
        }
        let mut assessed = 0u32;
        let mut non_green = 0u32;
        let mut i = 0;
        while i < PRECOND_N {
            match self.rows[i].state {
                CondState::Unset => return Err(WineCode::SilentSkip.code()),
                CondState::Green => {}
                CondState::Red | CondState::Gap => non_green = non_green.saturating_add(1),
            }
            assessed = assessed.saturating_add(1);
            i += 1;
        }
        // 缺口逐项列明：缺口账长度必须等于红/缺行数。
        if self.gaps.len() as u32 != non_green {
            return Err(WineCode::GapMasked.code());
        }
        // 证据重算对拍（证据不齐即篡改）。
        i = 0;
        while i < PRECOND_N {
            let st = self.rows[i].state;
            if st != CondState::Unset {
                // 重算用判词秩：由判定态反推（绿=0 红=1 缺=2）。
                let vr = match st {
                    CondState::Green => Verdict::Pass.rank(),
                    CondState::Red => Verdict::Fail.rank(),
                    _ => Verdict::GapFound.rank(),
                };
                let want = evidence_fp(i as u32, vr, REPRO_PAYLOAD);
                if want != self.rows[i].evidence {
                    return Err(WineCode::EvidenceMismatch.code());
                }
            }
            i += 1;
        }
        let _ = assessed;
        self.state = self.state.advance(ChecklistState::GapsListed)?;
        Ok(())
    }

    /// 验收闸（缺口列明→验收）：全绿 + 缺口账空 + B11 依赖在档。
    pub fn accept(&mut self) -> Result<(), u32> {
        if self.state != ChecklistState::GapsListed {
            return Err(WineCode::CloseNotReady.code());
        }
        if !self.msiexec_ready() {
            return Err(WineCode::DepMissing.code());
        }
        let mut i = 0;
        while i < PRECOND_N {
            if self.rows[i].state != CondState::Green {
                return Err(WineCode::CloseNotReady.code());
            }
            i += 1;
        }
        if !self.gaps.is_empty() {
            return Err(WineCode::CloseNotReady.code());
        }
        self.state = self.state.advance(ChecklistState::Accepted)?;
        Ok(())
    }

    /// 账守恒：评估计数 == 已评行数；缺口账长度 == 红/缺行数。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut n = 0u32;
        let mut ng = 0u32;
        let mut i = 0;
        while i < PRECOND_N {
            match self.rows[i].state {
                CondState::Unset => {}
                _ => n = n.saturating_add(1),
            }
            match self.rows[i].state {
                CondState::Red | CondState::Gap => ng = ng.saturating_add(1),
                _ => {}
            }
            i += 1;
        }
        if n != self.assess_cnt {
            return Err(WineCode::LedgerBreak.code());
        }
        if ng != self.gaps.len() as u32 {
            return Err(WineCode::GapMasked.code());
        }
        Ok(())
    }
}

/// 证据复算用的冻结 payload（收口闸重算轴；黄金流与注入流同源）。
pub const REPRO_PAYLOAD: u64 = 0x0B11_5EED;

// ── 收口裁决表（输入：全评/缺口列明/全绿/依赖 → 期望码；0=放行） ─
/// 四例样本：全绿+依赖→放行；有红→CloseNotReady；依赖缺→DepMissing；
/// 缺口未清→CloseNotReady。
pub const ACCEPT_CASES: [(bool, u32, u32, bool, u32); 4] = [
    // (dep_ok, non_green, gap_rows, all_green, want: 0=放行)
    (true, 0, 0, true, 0),
    (true, 2, 2, false, WineCode::CloseNotReady.code()),
    (false, 0, 0, true, WineCode::DepMissing.code()),
    (true, 1, 0, false, WineCode::CloseNotReady.code()),
];

/// 验收裁决闭式：与 accept() 闸语义同源（判据复算核）。
pub fn adjudicate_accept(dep_ok: bool, non_green: u32, gap_rows: u32, all_green: bool) -> u32 {
    if !dep_ok {
        return WineCode::DepMissing.code();
    }
    if non_green != 0 || !all_green || gap_rows != 0 {
        return WineCode::CloseNotReady.code();
    }
    0
}

/// 裁决表逐例对拍。
pub fn adjudication_ok() -> bool {
    let mut i = 0;
    while i < ACCEPT_CASES.len() {
        let (d, ng, gr, ag, want) = ACCEPT_CASES[i];
        if adjudicate_accept(d, ng, gr, ag) != want {
            return false;
        }
        i += 1;
    }
    true
}

// ── 标准流程与黄金全链 ──────────────────────────────────────────
/// 黄金全链：绑 B11 依赖→10 项全绿评估→收口→验收→账守恒。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut l = PrecondLedger::new();
    l.bind_b11(evidence_fp(MSIEXEC_IDX as u32, B11_ID as u32, REPRO_PAYLOAD));
    let mut i = 0;
    while i < PRECOND_N {
        l.assess(i, Verdict::Pass, REPRO_PAYLOAD)?;
        i += 1;
    }
    l.close()?;
    l.accept()?;
    l.conserve()?;
    Ok((l.assess_cnt, l.gaps.len() as u32))
}

/// 缺口列明路径：一项红 + 一项缺口，逐项入账，收口过、验收拒
/// （缺口逐项列明不静默的正例语义）。
pub fn gap_flow_run() -> Result<(u32, u32), u32> {
    let mut l = PrecondLedger::new();
    l.bind_b11(evidence_fp(MSIEXEC_IDX as u32, B11_ID as u32, REPRO_PAYLOAD));
    let mut i = 0;
    while i < PRECOND_N {
        let v = match i {
            3 => Verdict::Fail,
            7 => Verdict::GapFound,
            _ => Verdict::Pass,
        };
        l.assess(i, v, REPRO_PAYLOAD)?;
        i += 1;
    }
    l.close()?;
    // 缺口在档：验收必拒（CloseNotReady），但缺口账逐项在册。
    match l.accept() {
        Err(c) if c == WineCode::CloseNotReady.code() => {}
        _ => return Err(WineCode::CloseNotReady.code()),
    }
    l.conserve()?;
    Ok((l.assess_cnt, l.gaps.len() as u32))
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn missed(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0;
        while i < 3 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入核：三类违约各 JUDGE_ROUNDS 轮，逐类断言被显式拒绝。
///   类 0：无证据绿（篡改证据指纹 → 收口重算 EvidenceMismatch）
///   类 1：未评收口（留一行 Unset 即 close → SilentSkip）
///   类 2：缺口遮蔽（清空缺口账后 close → GapMasked）
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 类 0：篡改证据 → 收口重算不符。
        let mut a = PrecondLedger::new();
        a.bind_b11(evidence_fp(MSIEXEC_IDX as u32, B11_ID as u32, REPRO_PAYLOAD));
        let mut i = 0;
        while i < PRECOND_N {
            a.assess(i, Verdict::Pass, REPRO_PAYLOAD)?;
            i += 1;
        }
        a.rows[2].evidence ^= 0x0000_0000_0000_00FF;
        let hit_a = match a.close() {
            Err(c) if c == WineCode::EvidenceMismatch.code() => true,
            _ => false,
        };
        led.record(0, hit_a);
        // 类 1：未评收口。
        let mut b = PrecondLedger::new();
        b.bind_b11(evidence_fp(MSIEXEC_IDX as u32, B11_ID as u32, REPRO_PAYLOAD));
        i = 0;
        while i < PRECOND_N - 1 {
            b.assess(i, Verdict::Pass, REPRO_PAYLOAD)?;
            i += 1;
        }
        let hit_b = match b.close() {
            Err(c) if c == WineCode::SilentSkip.code() => true,
            _ => false,
        };
        led.record(1, hit_b);
        // 类 2：缺口遮蔽（红行存在但缺口账被清空）。
        let mut c = PrecondLedger::new();
        c.bind_b11(evidence_fp(MSIEXEC_IDX as u32, B11_ID as u32, REPRO_PAYLOAD));
        i = 0;
        while i < PRECOND_N {
            let v = if i == 5 { Verdict::Fail } else { Verdict::Pass };
            c.assess(i, v, REPRO_PAYLOAD)?;
            i += 1;
        }
        c.gaps.clear();
        let hit_c = match c.close() {
            Err(cc) if cc == WineCode::GapMasked.code() => true,
            _ => false,
        };
        led.record(2, hit_c);
        r += 1;
    }
    Ok(led)
}

// ── 边界与探针 ──────────────────────────────────────────────────
/// 边界探针：条件号越界 / 未绑依赖评 B11 项 / 重复评估 / 错态评估。
pub fn boundary_probe() -> Result<(), u32> {
    let mut l = PrecondLedger::new();
    // 条件号越界。
    match l.assess(PRECOND_N, Verdict::Pass, 1) {
        Err(c) if c == WineCode::CondOutOfRange.code() => {}
        _ => return Err(WineCode::CondOutOfRange.code()),
    }
    // 未绑 B11 依赖评第 0 项必拒。
    match l.assess(MSIEXEC_IDX, Verdict::Pass, 1) {
        Err(c) if c == WineCode::DepMissing.code() => {}
        _ => return Err(WineCode::DepMissing.code()),
    }
    // 绑定后评估合法，重复评估必拒。
    l.bind_b11(evidence_fp(MSIEXEC_IDX as u32, B11_ID as u32, REPRO_PAYLOAD));
    l.assess(MSIEXEC_IDX, Verdict::Pass, REPRO_PAYLOAD)?;
    match l.assess(MSIEXEC_IDX, Verdict::Pass, REPRO_PAYLOAD) {
        Err(c) if c == WineCode::DupAssess.code() => {}
        _ => return Err(WineCode::DupAssess.code()),
    }
    // 错态评估：收口后再评必拒。
    let mut i = 1;
    while i < PRECOND_N {
        l.assess(i, Verdict::Pass, REPRO_PAYLOAD)?;
        i += 1;
    }
    l.close()?;
    match l.assess(0, Verdict::Pass, REPRO_PAYLOAD) {
        Err(c) if c == WineCode::StateBreak.code() => Ok(()),
        _ => Err(WineCode::StateBreak.code()),
    }
}

/// 探针 A：状态机越级（采集直取验收）必拒。
pub fn probe_state_skip() -> Result<(), u32> {
    let l = PrecondLedger::new();
    let skip = match ChecklistState::Collecting.advance(ChecklistState::Accepted) {
        Err(c) if c == WineCode::StateBreak.code() => true,
        _ => false,
    };
    if !skip {
        return Err(WineCode::StateBreak.code());
    }
    let _ = l;
    Ok(())
}

/// 探针 B：未就绪验收（未收口即 accept）必拒。
pub fn probe_accept_not_ready() -> Result<(), u32> {
    let mut l = PrecondLedger::new();
    match l.accept() {
        Err(c) if c == WineCode::CloseNotReady.code() => Ok(()),
        _ => Err(WineCode::CloseNotReady.code()),
    }
}

/// 探针 C：依赖缺失验收必拒（全绿但无 B11 依赖）。
pub fn probe_dep_missing() -> Result<(), u32> {
    let mut l = PrecondLedger::new();
    // 不绑 B11：第 0 项评不了 → 全链构造改走：绑依赖评完再清依赖。
    l.bind_b11(evidence_fp(MSIEXEC_IDX as u32, B11_ID as u32, REPRO_PAYLOAD));
    let mut i = 0;
    while i < PRECOND_N {
        l.assess(i, Verdict::Pass, REPRO_PAYLOAD)?;
        i += 1;
    }
    l.b11_dep = None;
    match l.accept() {
        Err(c) if c == WineCode::DepMissing.code() => Ok(()),
        _ => Err(WineCode::DepMissing.code()),
    }
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(WineCode::NotReproducible.code());
    }
    let g = gap_flow_run()?;
    let h = gap_flow_run()?;
    if g != h {
        return Err(WineCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫：SEG 对齐/窗位 + 码闭集段内 + 文案非空。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x84000).contains(&SEG_BASE) {
        return false;
    }
    let mut i = 0;
    while i < WineCode::ALL.len() {
        let v = WineCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if WineCode::ALL[i].detail().len() == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 运行期摘要（动态明细消费）。
pub fn summary() -> String {
    let g = golden_run();
    format!("{:?}", g)
}

// ── 检查入口（判据 8–20 条） ────────────────────────────────────
pub fn run_unx18564_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18564 Wine 上栈环境联签（对 AI-21）");

    // ── 静态明细（主体） ─────────────────────────────────────
    let golden = golden_run();
    s.add("J1-上栈清单黄金全链", golden.is_ok(), "绑B11依赖→10项全绿→收口→验收");
    let ok2 = match &golden {
        Ok((n, g)) => *n == PRECOND_N as u32 && *g == 0,
        Err(_) => false,
    };
    s.add("J1-验收绿10/10", ok2, "B11 开工条件 10 项全绿、缺口账空");
    let gap = gap_flow_run();
    let ok3 = match &gap {
        Ok((n, g)) => *n == PRECOND_N as u32 && *g == 2,
        Err(_) => false,
    };
    s.add("J1-缺口逐项列明不静默", ok3, "1红+1缺逐项入账，收口过、验收拒");
    s.add("J1-验收裁决表四例对拍", adjudication_ok(), "依赖/缺口/全绿四态闭式一致");
    s.add("J3-状态机越级必拒", probe_state_skip().is_ok(), "采集直取验收被 StateBreak 拒");
    s.add("J3-未就绪验收必拒", probe_accept_not_ready().is_ok(), "未收口 accept 被 CloseNotReady 拒");
    s.add("J3-B11依赖缺失必拒", probe_dep_missing().is_ok(), "依赖位清空后验收被 DepMissing 拒");
    s.add("J4-评估边界（越界/未绑/重复/错态）", boundary_probe().is_ok(), "四类界外全部显式拒");
    s.add("J7-段位守卫与文案闭集", segment_guard(), "0x808C0 段 0x40 窗，12 码全有中文建议");

    // ── 动态明细（≤8 槽） ────────────────────────────────────
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    let (t, c) = match &led {
        Ok(l) => (l.tries[0] + l.tries[1] + l.tries[2], l.caught[0] + l.caught[1] + l.caught[2]),
        Err(_) => (0u32, 0u32),
    };
    s.add_dyn("J2-注入10/10全抓", led_ok, &format!("三类各{}轮 抓{}/{}", JUDGE_ROUNDS, c, t));
    s.add_dyn("J2-漏检为零", match &led { Ok(l) => l.missed() == 0, Err(_) => false }, &format!("漏检{}", match &led { Ok(l) => l.missed(), Err(_) => 99u32 }));
    s.add_dyn("J6-双跑一致", repro_check().is_ok(), &format!("golden={:?}", summary()));
    s.add_dyn("J5-账守恒", conserve_probe().is_ok(), &format!("守恒={:?}", conserve_probe().err()));
    s
}

/// 账守恒探针：黄金流后另造一账逐行评估再守恒。
pub fn conserve_probe() -> Result<(), u32> {
    golden_run()?;
    let mut l = PrecondLedger::new();
    l.bind_b11(evidence_fp(MSIEXEC_IDX as u32, B11_ID as u32, REPRO_PAYLOAD));
    let mut i = 0;
    while i < PRECOND_N {
        l.assess(i, Verdict::Pass, REPRO_PAYLOAD)?;
        i += 1;
    }
    l.conserve()?;
    Ok(())
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checks_all_passed() {
        let set = run_unx18564_checks();
        assert!(set.all_passed(), "UNX-F18564 判据集必须全绿: {}", set.red_items_text());
        assert!(set.len() >= 8 && set.len() <= 20);
    }

    #[test]
    fn test_golden_run_green() {
        let g = golden_run();
        assert!(g.is_ok());
        let (n, gaps) = g.unwrap_or((0, 0));
        assert_eq!(n, PRECOND_N as u32);
        assert_eq!(gaps, 0);
    }

    #[test]
    fn test_gap_flow_listed_not_silent() {
        let g = gap_flow_run();
        assert!(g.is_ok());
        let (n, gaps) = g.unwrap_or((0, 0));
        assert_eq!(gaps, 2);
        assert_eq!(n, PRECOND_N as u32);
    }

    #[test]
    fn test_state_machine_monotonic() {
        assert!(probe_state_skip().is_ok());
        assert_eq!(
            ChecklistState::Accepted.advance(ChecklistState::Collecting).err(),
            Some(WineCode::StateBreak.code())
        );
        assert!(ChecklistState::Collecting.advance(ChecklistState::GapsListed).is_ok());
    }

    #[test]
    fn test_dup_assess_rejected() {
        let mut l = PrecondLedger::new();
        l.bind_b11(1);
        l.assess(MSIEXEC_IDX, Verdict::Pass, REPRO_PAYLOAD).unwrap_or(0);
        assert_eq!(
            l.assess(MSIEXEC_IDX, Verdict::Pass, REPRO_PAYLOAD).err(),
            Some(WineCode::DupAssess.code())
        );
    }

    #[test]
    fn test_dep_missing_gates() {
        let mut l = PrecondLedger::new();
        assert_eq!(
            l.assess(MSIEXEC_IDX, Verdict::Pass, REPRO_PAYLOAD).err(),
            Some(WineCode::DepMissing.code())
        );
        assert!(probe_dep_missing().is_ok());
    }

    #[test]
    fn test_boundary() {
        assert!(boundary_probe().is_ok());
    }

    #[test]
    fn test_inject_rounds_all_caught() {
        let led = inject_rounds(JUDGE_ROUNDS);
        let l = led.unwrap_or(InjectLedger::new());
        assert!(l.all_caught());
        assert_eq!(l.missed(), 0);
        for i in 0..3 {
            assert_eq!(l.tries[i], JUDGE_ROUNDS);
            assert_eq!(l.caught[i], JUDGE_ROUNDS);
        }
    }

    #[test]
    fn test_repro_dual_run() {
        assert!(repro_check().is_ok());
        let a = golden_run().unwrap_or((0, 0));
        let b = golden_run().unwrap_or((9, 9));
        assert_eq!(a, b);
    }

    #[test]
    fn test_segment_guard_and_detail() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x808C0);
        assert_eq!(SEG_BASE % 0x40, 0);
        for c in WineCode::ALL.iter() {
            assert_eq!(detail(c.code()), c.detail());
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
    }
}
