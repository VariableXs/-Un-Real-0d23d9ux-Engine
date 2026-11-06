//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18565 · vcredist 免装联签收口（对 AI-22） · 0x80900段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18565-J1 免装语义判据收口断言全过，锚 2 绑定登记
//!       （衔接 F18556）
//! 设计要点：
//!   - 免装语义闭式：宿主 DLL 覆盖表逐行核（arch 对齐 + (maj,min)
//!     字典序 >= 请求版本），全行覆盖才准「免装」——缺一行即 RowFail；
//!   - 版本边界写死：宿主最高 14.38；请求 14.38 过（等界放行）、
//!     14.39 拒（界外必拒）、13.x 拒（主版本倒退必拒）；
//!   - 锚 2 绑定登记（衔接 F18556）：每条判据行在锚 2 各留一条绑定
//!     指纹，锚号必须为 2（AnchorWrong），缺行绑定 seal 闸必拒
//!     （AnchorUnbound），重复绑定显式拒（DupBinding）；
//!   - 收口状态机：评审→绑定→收口，单调三态；收口前置 = 全行判过
//!     + 全行过 + 全行绑定，缺一即拒；
//!   - 注入核三类：判据未过硬收口 / 绑定缺席 / 锚号错挂，各 10 轮
//!     全抓，漏检为零；
//!   - 边界：行号越界/绑定账满/版本三界（等界/界外/主版本倒退）；
//!   - LCG 种子写死 = SEG_BASE（0x80900）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! 结构清单：
//!   数据结构：VerReq（版本请求）/ NoInstLedger（免装收口账+锚2绑定账）
//!   状态机  ：ClosureState（评审→绑定→收口 单调三态）
//!   账本    ：判据行数组账 + 绑定账 + 收口指纹 + 注入检出账
//!   错误路径：VcCode 12 码闭集，code() = SEG_BASE + n（0x80900 窗）
//!   注入核  ：RowFail/绑定缺席/锚号错挂三类逐类断言被拒（10/10）
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80900，AI-VX11-1105 批09 第 5 槽，0x40 对齐独占窗）。
pub const SEG_BASE: u32 = 0x80900;

// ── 冻结常量（写死并注明推导） ──────────────────────────────────
/// 锚 2 号（判据原文钉死：锚 2 绑定登记，衔接 F18556）。
pub const ANCHOR2: u32 = 2;
/// 判据行容量（推导：2 架构 × 3 宿主版本 = 6 行闭集）。
pub const REQ_CAP: usize = 6;
/// 绑定账容量（推导：每行一绑 = 6，满即拒不覆盖）。
pub const BIND_CAP: usize = 6;
/// 宿主最高版本（14.38 冻结：本仓宿主 DLL 覆盖面上限）。
pub const HOST_MAJ_MAX: u32 = 14;
pub const HOST_MIN_MAX: u32 = 38;
/// 判据迭代轮数（写死：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 绑定指纹种子（γ 前 64 位，冻结推导）。
pub const FP_SEED: u64 = 0x6A09_E667_F3BC_C908;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VcCode {
    /// 行号越界：0..6 之外
    RowOutOfRange,
    /// 重复绑定：同行二绑
    DupBinding,
    /// 绑定账满
    BindFull,
    /// 锚号错挂：锚号 ≠ 2
    AnchorWrong,
    /// 绑定缺席：行未绑锚 2
    AnchorUnbound,
    /// 判据未过：宿主覆盖不足
    RowFail,
    /// 状态越级/倒退
    StateBreak,
    /// 收口未就绪
    CloseNotReady,
    /// 版本域错：请求版本形态非法（maj=0 等）
    VerIllegal,
    /// 指纹不齐：绑定重算不符
    FpMismatch,
    /// 双跑漂移
    NotReproducible,
    /// 未知诊断码兜底
    Unknown,
}

impl VcCode {
    /// 诊断码 = SEG_BASE + 序号（段内闭集，const 供静态裁决表）。
    pub const fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文建议（文案闭集，每码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            VcCode::RowOutOfRange => "行号越界：判据行只在 0..6 内取",
            VcCode::DupBinding => "重复绑定：一行一绑，改绑先撤旧账",
            VcCode::BindFull => "绑定账满：6 槽用尽，先补齐缺行再绑",
            VcCode::AnchorWrong => "锚号错挂：锚 2 绑定只认锚号 2",
            VcCode::AnchorUnbound => "绑定缺席：每行都要在锚 2 留绑定指纹",
            VcCode::RowFail => "判据未过：宿主 DLL 覆盖不足，补装或降请求",
            VcCode::StateBreak => "状态越级或倒退：按 评审→绑定→收口 走",
            VcCode::CloseNotReady => "收口未就绪：全行判过+全行绑+全行过才收口",
            VcCode::VerIllegal => "版本域错：主版本须 >= 1",
            VcCode::FpMismatch => "指纹不齐：绑定重算不符，重绑留证",
            VcCode::NotReproducible => "双跑漂移：冻结种子重跑对拍",
            VcCode::Unknown => "未知诊断码：不在 0x80900 段闭集内",
        }
    }
    pub const ALL: [VcCode; 12] = [
        VcCode::RowOutOfRange,
        VcCode::DupBinding,
        VcCode::BindFull,
        VcCode::AnchorWrong,
        VcCode::AnchorUnbound,
        VcCode::RowFail,
        VcCode::StateBreak,
        VcCode::CloseNotReady,
        VcCode::VerIllegal,
        VcCode::FpMismatch,
        VcCode::NotReproducible,
        VcCode::Unknown,
    ];
}

/// 码值反查中文建议（段外码落 Unknown 兜底）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < VcCode::ALL.len() {
        if VcCode::ALL[i].code() == code {
            return VcCode::ALL[i].detail();
        }
        i += 1;
    }
    VcCode::Unknown.detail()
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

/// 绑定指纹：行号、锚号、请求版本三元混合。
pub fn bind_fp(row: u32, anchor: u32, maj: u32, min: u32) -> u64 {
    let a = ((row as u64) << 40) ^ ((anchor as u64) << 24) ^ ((maj as u64) << 8) ^ (min as u64);
    mix64(a ^ FP_SEED)
}

// ── 免装语义闭式（宿主覆盖表 + 版本判定） ──────────────────────
/// 宿主 DLL 覆盖表（冻结：(arch, maj, min)；arch 0=x86 1=amd64）。
pub const HOST_COVER: [(u32, u32, u32); 6] = [
    (0, 14, 16),
    (0, 14, 29),
    (0, 14, 38),
    (1, 14, 16),
    (1, 14, 29),
    (1, 14, 38),
];

/// 免装语义判定：宿主存在同 arch 且 (maj,min) 字典序 >= 请求的覆盖。
pub fn covers(arch: u32, maj: u32, min: u32) -> bool {
    let mut i = 0;
    while i < HOST_COVER.len() {
        let (ha, hmaj, hmin) = HOST_COVER[i];
        if ha == arch && (hmaj > maj || (hmaj == maj && hmin >= min)) {
            return true;
        }
        i += 1;
    }
    false
}

/// 版本请求（arch + maj + min 三元）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VerReq {
    pub arch: u32,
    pub maj: u32,
    pub min: u32,
}

impl VerReq {
    pub const fn new(arch: u32, maj: u32, min: u32) -> VerReq {
        VerReq { arch, maj, min }
    }
}

/// 冻结判据行（6 行闭集：全部宿主可覆盖，14.38 等界行收边界）。
pub const REQ_TABLE: [VerReq; REQ_CAP] = [
    VerReq::new(0, 14, 16),
    VerReq::new(0, 14, 29),
    VerReq::new(0, 14, 38),
    VerReq::new(1, 14, 16),
    VerReq::new(1, 14, 29),
    VerReq::new(1, 14, 38),
];

// ── 收口状态机（单调三态） ──────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClosureState {
    Judging = 0,
    Bound = 1,
    Closed = 2,
}

impl ClosureState {
    pub fn rank(self) -> u32 {
        self as u32
    }
    /// 单调 +1 放行；倒退/越级拒 StateBreak。
    pub fn advance(self, to: ClosureState) -> Result<ClosureState, u32> {
        let from = self.rank();
        let dst = to.rank();
        if dst == from + 1 {
            Ok(to)
        } else {
            Err(VcCode::StateBreak.code())
        }
    }
}

// ── 数据结构：免装收口账 ────────────────────────────────────────
pub struct NoInstLedger {
    /// 判据行（≤6 行，逐行判定）。
    pub reqs: Vec<VerReq>,
    /// 判据行数组账（行过/未过，None=未判）。
    pub row_pass: [Option<bool>; REQ_CAP],
    /// 绑定账：(行号, 指纹) 逐行锚 2 登记。
    pub bindings: Vec<(u32, u64)>,
    /// 收口状态机。
    pub state: ClosureState,
    /// 收口指纹（Closed 后置位）。
    pub closure: Option<u64>,
}

impl NoInstLedger {
    pub fn new() -> NoInstLedger {
        NoInstLedger {
            reqs: Vec::new(),
            row_pass: [None; REQ_CAP],
            bindings: Vec::new(),
            state: ClosureState::Judging,
            closure: None,
        }
    }

    /// 挂判据行：版本域守卫（maj>=1）+ 账满拒；挂后立即判定入账。
    pub fn add_req(&mut self, r: VerReq) -> Result<usize, u32> {
        if self.reqs.len() >= REQ_CAP {
            return Err(VcCode::BindFull.code());
        }
        if r.maj == 0 {
            return Err(VcCode::VerIllegal.code());
        }
        let idx = self.reqs.len();
        self.reqs.push(r);
        self.row_pass[idx] = Some(covers(r.arch, r.maj, r.min));
        Ok(idx)
    }

    /// 行已过？（未挂行/未判行一律 false 语义由调用方先核）
    pub fn row_ok(&self, row: usize) -> Result<bool, u32> {
        if row >= REQ_CAP || row >= self.reqs.len() {
            return Err(VcCode::RowOutOfRange.code());
        }
        match self.row_pass[row] {
            Some(p) => Ok(p),
            None => Err(VcCode::RowOutOfRange.code()),
        }
    }

    /// 锚 2 绑定：锚号必须为 2；行号域守卫；查重；账满拒。
    pub fn bind(&mut self, row: usize, anchor: u32) -> Result<u64, u32> {
        if anchor != ANCHOR2 {
            return Err(VcCode::AnchorWrong.code());
        }
        if row >= self.reqs.len() {
            return Err(VcCode::RowOutOfRange.code());
        }
        let mut i = 0;
        while i < self.bindings.len() {
            match self.bindings.get(i) {
                Some((r, _)) => {
                    if *r == row as u32 {
                        return Err(VcCode::DupBinding.code());
                    }
                }
                None => return Err(VcCode::RowOutOfRange.code()),
            }
            i += 1;
        }
        if self.bindings.len() >= BIND_CAP {
            return Err(VcCode::BindFull.code());
        }
        let r = match self.reqs.get(row) {
            Some(r) => *r,
            None => return Err(VcCode::RowOutOfRange.code()),
        };
        let f = bind_fp(row as u32, anchor, r.maj, r.min);
        self.bindings.push((row as u32, f));
        Ok(f)
    }

    /// 绑定重算对拍（收口闸用）：逐绑重算指纹。
    fn bindings_ok(&self) -> Result<(), u32> {
        if self.bindings.len() != self.reqs.len() {
            return Err(VcCode::AnchorUnbound.code());
        }
        let mut i = 0;
        while i < self.bindings.len() {
            let (row, f) = match self.bindings.get(i) {
                Some(v) => *v,
                None => return Err(VcCode::AnchorUnbound.code()),
            };
            let r = match self.reqs.get(row as usize) {
                Some(r) => *r,
                None => return Err(VcCode::RowOutOfRange.code()),
            };
            let want = bind_fp(row, ANCHOR2, r.maj, r.min);
            if want != f {
                return Err(VcCode::FpMismatch.code());
            }
            i += 1;
        }
        Ok(())
    }

    /// 绑定闸（评审→绑定）：全行判过 + 绑定账逐行齐 + 重算对拍。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state != ClosureState::Judging {
            return Err(VcCode::StateBreak.code());
        }
        // 全行判过。
        let mut i = 0;
        while i < self.reqs.len() {
            if self.row_pass[i].is_none() {
                return Err(VcCode::CloseNotReady.code());
            }
            i += 1;
        }
        // 绑定齐 + 对拍（缺行即 AnchorUnbound）。
        self.bindings_ok()?;
        self.state = self.state.advance(ClosureState::Bound)?;
        Ok(())
    }

    /// 收口闸（绑定→收口）：全行过；任一行未过即 RowFail。
    pub fn close(&mut self) -> Result<u64, u32> {
        if self.state != ClosureState::Bound {
            return Err(VcCode::CloseNotReady.code());
        }
        let mut acc: u64 = FP_SEED;
        let mut i = 0;
        while i < self.reqs.len() {
            match self.row_pass[i] {
                Some(true) => {}
                Some(false) => return Err(VcCode::RowFail.code()),
                None => return Err(VcCode::CloseNotReady.code()),
            }
            let r = match self.reqs.get(i) {
                Some(r) => *r,
                None => return Err(VcCode::RowOutOfRange.code()),
            };
            acc = mix64(acc ^ bind_fp(i as u32, ANCHOR2, r.maj, r.min));
            i += 1;
        }
        self.state = self.state.advance(ClosureState::Closed)?;
        self.closure = Some(acc);
        Ok(acc)
    }

    /// 账守恒：绑定账长度 == 判据行数（收口后恒等）。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.state.rank() >= ClosureState::Bound.rank() && self.bindings.len() != self.reqs.len()
        {
            return Err(VcCode::AnchorUnbound.code());
        }
        if self.bindings.len() > BIND_CAP {
            return Err(VcCode::BindFull.code());
        }
        Ok(())
    }
}

// ── 收口裁决表（输入：全判/全过/全绑/锚对 → 期望码；0=放行） ───
/// 四例样本：全齐→放行；行未过→RowFail；绑缺→AnchorUnbound；
/// 锚号错→AnchorWrong。
pub const CLOSE_CASES: [(bool, bool, bool, bool, u32); 4] = [
    // (all_judged, all_pass, all_bound, anchor_ok, want: 0=放行)
    (true, true, true, true, 0),
    (true, false, true, true, VcCode::RowFail.code()),
    (true, true, false, true, VcCode::AnchorUnbound.code()),
    (true, true, true, false, VcCode::AnchorWrong.code()),
];

/// 收口裁决闭式：与 seal/close 闸语义同源（判据复算核）。
pub fn adjudicate_close(all_judged: bool, all_pass: bool, all_bound: bool, anchor_ok: bool) -> u32 {
    if !anchor_ok {
        return VcCode::AnchorWrong.code();
    }
    if !all_judged {
        return VcCode::CloseNotReady.code();
    }
    if !all_bound {
        return VcCode::AnchorUnbound.code();
    }
    if !all_pass {
        return VcCode::RowFail.code();
    }
    0
}

/// 裁决表逐例对拍。
pub fn adjudication_ok() -> bool {
    let mut i = 0;
    while i < CLOSE_CASES.len() {
        let (j, p, b, a, want) = CLOSE_CASES[i];
        if adjudicate_close(j, p, b, a) != want {
            return false;
        }
        i += 1;
    }
    true
}

// ── 标准流程与黄金全链 ──────────────────────────────────────────
/// 黄金全链：6 行判据全挂全过→6 绑锚2→绑定闸→收口闸→账守恒。
pub fn golden_run() -> Result<(u32, u64), u32> {
    let mut l = NoInstLedger::new();
    let mut i = 0;
    while i < REQ_CAP {
        let r = REQ_TABLE[i];
        l.add_req(r)?;
        i += 1;
    }
    // 绑定前先核全行过（免装语义正例）。
    i = 0;
    while i < REQ_CAP {
        if !l.row_ok(i)? {
            return Err(VcCode::RowFail.code());
        }
        i += 1;
    }
    i = 0;
    while i < REQ_CAP {
        l.bind(i, ANCHOR2)?;
        i += 1;
    }
    l.seal()?;
    let f = l.close()?;
    l.conserve()?;
    Ok((l.reqs.len() as u32, f))
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
///   类 0：判据未过硬收口（请求 14.39 行挂进账 → close 拒 RowFail）
///   类 1：绑定缺席（只绑一半 → seal 拒 AnchorUnbound）
///   类 2：锚号错挂（锚 1 绑定 → AnchorWrong）
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 类 0：宿主不可覆盖的请求行。
        let mut a = NoInstLedger::new();
        let mut i = 0;
        while i < REQ_CAP - 1 {
            a.add_req(REQ_TABLE[i])?;
            i += 1;
        }
        a.add_req(VerReq::new(1, 14, 39))?; // 界外行：必挂
        i = 0;
        while i < REQ_CAP {
            a.bind(i, ANCHOR2)?;
            i += 1;
        }
        a.seal()?;
        let hit_a = match a.close() {
            Err(c) if c == VcCode::RowFail.code() => true,
            _ => false,
        };
        led.record(0, hit_a);
        // 类 1：绑定缺席（5/6 绑）。
        let mut b = fresh_full()?;
        i = 0;
        while i < REQ_CAP - 1 {
            b.bind(i, ANCHOR2)?;
            i += 1;
        }
        let hit_b = match b.seal() {
            Err(c) if c == VcCode::AnchorUnbound.code() => true,
            _ => false,
        };
        led.record(1, hit_b);
        // 类 2：锚号错挂。
        let mut c = fresh_full()?;
        let hit_c = match c.bind(0, 1) {
            Err(cc) if cc == VcCode::AnchorWrong.code() => true,
            _ => false,
        };
        led.record(2, hit_c);
        r += 1;
    }
    Ok(led)
}

/// 造一台 6 行判据全挂账（注入前置；不绑不闸）。
fn fresh_full() -> Result<NoInstLedger, u32> {
    let mut l = NoInstLedger::new();
    let mut i = 0;
    while i < REQ_CAP {
        l.add_req(REQ_TABLE[i])?;
        i += 1;
    }
    Ok(l)
}

// ── 边界与探针 ──────────────────────────────────────────────────
/// 版本三界探针：等界放行 / 界外拒 / 主版本倒退拒 / 零主版本拒。
pub fn version_boundary_probe() -> Result<(), u32> {
    // 等界：14.38 == 宿主最高 → 过。
    if !covers(0, 14, 38) {
        return Err(VcCode::RowFail.code());
    }
    // 界外：14.39 → 拒。
    if covers(0, 14, 39) {
        return Err(VcCode::RowFail.code());
    }
    // 主版本倒退：13.x → 拒（宿主最低 14.16）。
    if covers(0, 13, 99) {
        return Err(VcCode::RowFail.code());
    }
    // 架构错位：amd64=2 不在闭集 → 拒。
    if covers(2, 14, 16) {
        return Err(VcCode::RowFail.code());
    }
    // 版本域守卫：maj=0 挂账必拒。
    let mut l = NoInstLedger::new();
    match l.add_req(VerReq::new(0, 0, 1)) {
        Err(c) if c == VcCode::VerIllegal.code() => {}
        _ => return Err(VcCode::VerIllegal.code()),
    }
    Ok(())
}

/// 边界探针：行号越界 / 重复绑定 / 账满。
pub fn boundary_probe() -> Result<(), u32> {
    let mut l = fresh_full()?;
    // 行号越界绑定必拒。
    match l.bind(REQ_CAP, ANCHOR2) {
        Err(c) if c == VcCode::RowOutOfRange.code() => {}
        _ => return Err(VcCode::RowOutOfRange.code()),
    }
    // 重复绑定必拒。
    l.bind(0, ANCHOR2)?;
    match l.bind(0, ANCHOR2) {
        Err(c) if c == VcCode::DupBinding.code() => {}
        _ => return Err(VcCode::DupBinding.code()),
    }
    // 绑满 6 行后无余行可绑：再造一台挂第 7 行必拒账满。
    let mut m = fresh_full()?;
    let mut i = 0;
    while i < REQ_CAP {
        m.bind(i, ANCHOR2)?;
        i += 1;
    }
    match m.add_req(VerReq::new(0, 14, 16)) {
        Err(c) if c == VcCode::BindFull.code() => {}
        _ => return Err(VcCode::BindFull.code()),
    }
    Ok(())
}

/// 探针 A：状态机越级（评审直取收口）必拒。
pub fn probe_state_skip() -> Result<(), u32> {
    let l = NoInstLedger::new();
    let skip = match ClosureState::Judging.advance(ClosureState::Closed) {
        Err(c) if c == VcCode::StateBreak.code() => true,
        _ => false,
    };
    if !skip {
        return Err(VcCode::StateBreak.code());
    }
    let _ = l;
    Ok(())
}

/// 探针 B：未 seal 即 close 必拒。
pub fn probe_close_not_ready() -> Result<(), u32> {
    let mut l = fresh_full()?;
    match l.close() {
        Err(c) if c == VcCode::CloseNotReady.code() => Ok(()),
        _ => Err(VcCode::CloseNotReady.code()),
    }
}

/// 探针 C：绑定指纹篡改 → seal 重算不符必拒。
pub fn probe_bind_tamper() -> Result<(), u32> {
    let mut l = fresh_full()?;
    let mut i = 0;
    while i < REQ_CAP {
        l.bind(i, ANCHOR2)?;
        i += 1;
    }
    // 篡改首绑指纹。
    match l.bindings.get_mut(0) {
        Some(b) => b.1 ^= 0x0000_0000_0000_00FF,
        None => return Err(VcCode::AnchorUnbound.code()),
    }
    match l.seal() {
        Err(c) if c == VcCode::FpMismatch.code() => Ok(()),
        _ => Err(VcCode::FpMismatch.code()),
    }
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(VcCode::NotReproducible.code());
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
    while i < VcCode::ALL.len() {
        let v = VcCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if VcCode::ALL[i].detail().len() == 0 {
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
pub fn run_unx18565_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18565 vcredist 免装联签收口（对 AI-22）");

    // ── 静态明细（主体） ─────────────────────────────────────
    let golden = golden_run();
    s.add("J1-免装收口黄金全链", golden.is_ok(), "6行判据全过→6绑锚2→绑定闸→收口闸");
    let ok2 = match &golden {
        Ok((n, _)) => *n == REQ_CAP as u32,
        Err(_) => false,
    };
    s.add("J1-锚2绑定登记在档", ok2, "每行一绑、锚号=2（衔接 F18556）");
    s.add("J1-收口裁决表四例对拍", adjudication_ok(), "判/过/绑/锚四态闭式一致");
    s.add("J4-版本三界（等界/界外/倒退）", version_boundary_probe().is_ok(), "14.38过 14.39拒 13.x拒 maj=0拒");
    s.add("J4-绑定边界（越界/重复/满）", boundary_probe().is_ok(), "三类界外全部显式拒");
    s.add("J3-状态机越级必拒", probe_state_skip().is_ok(), "评审直取收口被 StateBreak 拒");
    s.add("J3-未绑定即收口必拒", probe_close_not_ready().is_ok(), "未 seal close 被 CloseNotReady 拒");
    s.add("J5-绑定指纹篡改必抓", probe_bind_tamper().is_ok(), "seal 重算不符被 FpMismatch 拒");
    s.add("J7-段位守卫与文案闭集", segment_guard(), "0x80900 段 0x40 窗，12 码全有中文建议");

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
    s
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checks_all_passed() {
        let set = run_unx18565_checks();
        assert!(set.all_passed(), "UNX-F18565 判据集必须全绿: {}", set.red_items_text());
        assert!(set.len() >= 8 && set.len() <= 20);
    }

    #[test]
    fn test_golden_run_green() {
        let g = golden_run();
        assert!(g.is_ok());
        let (n, f) = g.unwrap_or((0, 0));
        assert_eq!(n, REQ_CAP as u32);
        assert_ne!(f, 0);
    }

    #[test]
    fn test_semantics_all_rows_pass() {
        let mut i = 0;
        while i < REQ_CAP {
            let r = REQ_TABLE[i];
            assert!(covers(r.arch, r.maj, r.min), "行{}应被宿主覆盖", i);
            i += 1;
        }
    }

    #[test]
    fn test_version_boundary() {
        assert!(version_boundary_probe().is_ok());
        assert!(covers(1, 14, 38));
        assert!(!covers(1, 14, 39));
        assert!(!covers(1, 15, 0));
    }

    #[test]
    fn test_row_fail_blocks_close() {
        let mut l = NoInstLedger::new();
        l.add_req(VerReq::new(1, 14, 39)).unwrap_or(0);
        l.bind(0, ANCHOR2).unwrap_or(0);
        l.seal().unwrap_or(());
        assert_eq!(l.close().err(), Some(VcCode::RowFail.code()));
    }

    #[test]
    fn test_binding_gates() {
        assert!(boundary_probe().is_ok());
        let mut l = fresh_full().unwrap_or(NoInstLedger::new());
        // 缺绑即 seal 拒。
        assert_eq!(l.seal().err(), Some(VcCode::AnchorUnbound.code()));
    }

    #[test]
    fn test_state_machine_monotonic() {
        assert!(probe_state_skip().is_ok());
        assert_eq!(
            ClosureState::Closed.advance(ClosureState::Bound).err(),
            Some(VcCode::StateBreak.code())
        );
        assert!(ClosureState::Judging.advance(ClosureState::Bound).is_ok());
    }

    #[test]
    fn test_bind_tamper_caught() {
        assert!(probe_bind_tamper().is_ok());
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
        let b = golden_run().unwrap_or((1, 1));
        assert_eq!(a, b);
    }

    #[test]
    fn test_segment_guard_and_detail() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80900);
        assert_eq!(SEG_BASE % 0x40, 0);
        for c in VcCode::ALL.iter() {
            assert_eq!(detail(c.code()), c.detail());
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
    }
}
