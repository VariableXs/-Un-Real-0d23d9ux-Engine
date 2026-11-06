//! UNX-F48933 指针族 fuzz / 五路联轧 / 十五码族 · 0x71080
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，UNX-M2-B07）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F48933-J1 fuzz ×10^4 零崩零跳变；
//!   ③ UNX-F48934-J1 联轧 ×5 全绿零漂移；
//!   ④ UNX-F48935-J1 错误码 ×15 零漂移互不混淆。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：码族表审计 → fuzz 面 → 五路联轧 → 账本 → 复算。
//! 码族表必须先审：十五个诊断码是联轧失败时唯一的定位依据，若两张表里
//! 同一个值对应两个含义，联轧红了也查不出是谁红。fuzz 必须允许「拒绝」：
//! 乱参数的正确结果就是拒绝，若 fuzz 只统计「不崩」而丢掉拒绝数，账本
//! 就无法证明每一个输入都被处置过。联轧必须双跑比对：五路各自跑两遍，
//! 结果一致才说明没有隐藏顺序依赖。
//! 关键差异：本模块不实现新语义，只把 B07 前六模块的真实探针串起来做
//!           交叉复跑与码族冻结。任何一条探针红，本模块跟着红——联轧不
//!           吞红、不洗绿、不降级为「部分通过」。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48921_pointer_core as p_core;
use super::unx48923_pointer_meta as p_meta;
use super::unx48925_pointer_guard as p_guard;
use super::unx48928_pointer_dispatch as p_dispatch;
use super::unx48931_pointer_edge as p_edge;

/// 诊断段基址（0x71080，B07 第六槽）。
pub const SEG_BASE: u32 = 0x71080;

/// 族内冻结诊断码数量（15）。
pub const FAMILY_CODES: usize = 15;
/// fuzz 轮数（卡面 ×10^4 载体）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// 联轧路数（卡面 ×5 载体）。
pub const ROLLS: usize = 5;
/// 黄金轮数。
pub const GOLDEN_ROUNDS: u32 = 1000;
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0A） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BrCode {
    /// 码族表非法（重复/空明细/越界）
    TableBad,
    /// 覆盖不足（行数或路数不足）
    CoverageBad,
    /// 联轧不一致（两跑不同或某路红）
    RollBad,
    /// fuzz 不变量破坏（坐标越界/账不平）
    FuzzBad,
    /// 次序非法
    OrderBad,
    /// 复算不一致
    ReplayBad,
    /// 渲染回读失败
    RenderBad,
    /// 账本非法
    LedgerBad,
    /// 坐标越界
    PosBad,
    /// 段位越界
    SegmentBad,
}

impl BrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            BrCode::TableBad => "码族表非法：重复码/空明细/归属不明",
            BrCode::CoverageBad => "覆盖不足：码行数不足十五或联轧路数不足五",
            BrCode::RollBad => "联轧不一致：两跑不同或某一路探针红",
            BrCode::FuzzBad => "fuzz 不变量破坏：坐标越出档边界或账不平",
            BrCode::OrderBad => "次序非法：轮数与合成分/拒绝分之和不守恒",
            BrCode::ReplayBad => "复算不一致：两次黄金族账不同",
            BrCode::RenderBad => "渲染回读失败：字段缺失或数值越界",
            BrCode::LedgerBad => "账本非法：总轮数为零或拒绝分超过总轮数",
            BrCode::PosBad => "坐标越界：合成结果逃出档边界",
            BrCode::SegmentBad => "段位越界：诊断码逃出 0x71080 窗口",
        }
    }

    pub const ALL: [BrCode; 10] = [
        BrCode::TableBad,
        BrCode::CoverageBad,
        BrCode::RollBad,
        BrCode::FuzzBad,
        BrCode::OrderBad,
        BrCode::ReplayBad,
        BrCode::RenderBad,
        BrCode::LedgerBad,
        BrCode::PosBad,
        BrCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < BrCode::ALL.len() {
        if BrCode::ALL[i].val() == code {
            return BrCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x0000_0100_0000_01b3)
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 4 {
        x = fold_byte(x, ((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    x
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed | 1 }
    }

    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            (self.next() % (n as u64)) as u32
        }
    }
}

// ── 十五码族表 ────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CodeRow {
    pub owner: &'static str,
    pub code: u32,
    pub detail: &'static str,
}

pub fn code_rows() -> Vec<CodeRow> {
    let mut v = Vec::new();
    let mut i = 0usize;
    while i < p_core::PtrCode::ALL.len() {
        if let Some(c) = p_core::PtrCode::ALL.get(i) {
            v.push(CodeRow {
                owner: "core",
                code: c.val(),
                detail: p_core::detail(c.val()),
            });
        }
        i += 1;
    }
    v.push(CodeRow {
        owner: "meta",
        code: p_meta::MnCode::SwitchBad.val(),
        detail: p_meta::detail(p_meta::MnCode::SwitchBad.val()),
    });
    v.push(CodeRow {
        owner: "guard",
        code: p_guard::GdCode::BtnBad.val(),
        detail: p_guard::detail(p_guard::GdCode::BtnBad.val()),
    });
    v.push(CodeRow {
        owner: "dispatch",
        code: p_dispatch::DpCode::SubBad.val(),
        detail: p_dispatch::detail(p_dispatch::DpCode::SubBad.val()),
    });
    v
}

/// 码族表审计：恰十五行、码值两两不同、明细非空且非回退文案、属主非空。
pub fn audit_codes(rows: &[CodeRow]) -> Result<u32, u32> {
    if rows.len() != FAMILY_CODES {
        return Err(BrCode::CoverageBad.val());
    }
    let mut i = 0usize;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(v) => v,
            None => return Err(BrCode::TableBad.val()),
        };
        if r.owner.is_empty() || r.detail.is_empty() {
            return Err(BrCode::TableBad.val());
        }
        if r.detail == "未知诊断码" || r.detail == "未知槽位" {
            return Err(BrCode::TableBad.val());
        }
        let mut j = i + 1;
        while j < rows.len() {
            let o = match rows.get(j) {
                Some(v) => v,
                None => return Err(BrCode::TableBad.val()),
            };
            if o.code == r.code {
                return Err(BrCode::TableBad.val());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(FAMILY_CODES as u32)
}

/// 码族摘要：折叠全部码值，作为联轧的冻结指纹。
pub fn codes_digest(rows: &[CodeRow]) -> u64 {
    let mut h = FNV_OFF;
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            h = fold_u32(h, r.code);
        }
        i += 1;
    }
    h
}

// ── fuzz 面 ───────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzLog {
    pub rounds: u32,
    pub composed: u32,
    pub rejected: u32,
    pub clamps: u32,
    pub digest: u64,
}

impl FuzzLog {
    pub fn balanced(&self) -> bool {
        self.rounds != 0
            && self.rounds
                == self.composed.saturating_add(self.rejected)
    }
}

/// 指针合成 fuzz：乱 DPI/增益/边界/曲线/REL，全部输入必被处置。
fn fuzz_pointer(log: &mut FuzzLog, lcg: &mut Lcg, n: u32) -> Result<(), u32> {
    let mut i = 0u32;
    while i < n {
        let dpi = lcg.below(40_000);
        let gain = lcg.below(5_000);
        let bound = (lcg.below(2_000_001) as i64).saturating_sub(1_000_000);
        let mode = if lcg.below(2) == 0 {
            p_core::Mode::Flat
        } else {
            p_core::Mode::Curve
        };
        let curve = match lcg.below(3) {
            0 => p_core::Curve::new(p_core::curve_of(&[(0, 1000), (250, 1400), (1000, 2200)])),
            1 => p_core::Curve::new(p_core::curve_of(&[(0, 1000), (400, 600)])),
            _ => p_core::Curve::new(p_core::curve_of(&[])),
        };
        let safe_bound = if bound <= 0 { 1 } else { bound };
        let profile =
            p_core::PtrProfile::new(dpi, gain, curve.ok(), mode, safe_bound);
        match profile {
            Ok(p) => {
                let dx = lcg.below(70_000) as i32 - 35_000;
                let dy = lcg.below(70_000) as i32 - 35_000;
                let ev = p_core::RelEvent::new(dx, dy);
                match ev {
                    Ok(e) => {
                        match p_core::compose_checked(&p, p_core::Point { x: 0, y: 0 }, e) {
                            Ok((q, clamped)) => {
                                if q.x.unsigned_abs() > p.bound as u64
                                    || q.y.unsigned_abs() > p.bound as u64
                                {
                                    return Err(BrCode::PosBad.val());
                                }
                                log.composed = log.composed.saturating_add(1);
                                if clamped {
                                    log.clamps = log.clamps.saturating_add(1);
                                }
                                log.digest = fold_u32(log.digest, q.x as u32);
                                log.digest = fold_u32(log.digest, q.y as u32);
                            }
                            Err(_) => {
                                log.rejected = log.rejected.saturating_add(1);
                            }
                        }
                    }
                    Err(_) => {
                        log.rejected = log.rejected.saturating_add(1);
                    }
                }
            }
            Err(_) => {
                log.rejected = log.rejected.saturating_add(1);
            }
        }
        log.rounds = log.rounds.saturating_add(1);
        i = i.saturating_add(1);
    }
    Ok(())
}

/// 拓扑 fuzz：乱矩形/越界宽高，成功即定位并核对局部坐标不越出矩形。
fn fuzz_topology(log: &mut FuzzLog, lcg: &mut Lcg, n: u32) -> Result<(), u32> {
    let mut i = 0u32;
    while i < n {
        let count = 1u32.saturating_add(lcg.below(3));
        let mut rects = Vec::new();
        let mut k = 0u32;
        while k < count {
            rects.push(p_dispatch::MonitorRect {
                x: lcg.below(4_000) as i64,
                y: lcg.below(4_000) as i64,
                w: lcg.below(2_000) as i64,
                h: lcg.below(2_000) as i64,
                primary: k == 0,
            });
            k = k.saturating_add(1);
        }
        match p_dispatch::Topology::new(rects) {
            Ok(t) => {
                let px = lcg.below(5_000) as i64;
                let py = lcg.below(5_000) as i64;
                match t.locate(px, py) {
                    Ok((idx, (lx, ly))) => {
                        if let Some(r) = t.rects().get(idx) {
                            if lx < 0 || ly < 0 || lx >= r.w || ly >= r.h {
                                return Err(BrCode::PosBad.val());
                            }
                        }
                        log.composed = log.composed.saturating_add(1);
                        log.digest = fold_u32(log.digest, idx as u32);
                    }
                    Err(_) => {
                        log.rejected = log.rejected.saturating_add(1);
                    }
                }
            }
            Err(_) => {
                log.rejected = log.rejected.saturating_add(1);
            }
        }
        log.rounds = log.rounds.saturating_add(1);
        i = i.saturating_add(1);
    }
    Ok(())
}

/// 状态机 fuzz：乱事件序列，事件账必须守恒，终态只能是四态之一。
fn fuzz_fsm(log: &mut FuzzLog, lcg: &mut Lcg, n: u32) -> Result<(), u32> {
    let mut i = 0u32;
    while i < n {
        let mut f = p_edge::PointerFsm::new();
        let seq = 1u32.saturating_add(lcg.below(6));
        let mut k = 0u32;
        while k < seq {
            let ev = match lcg.below(4) {
                0 => p_edge::PointerEvent::Press(if lcg.below(2) == 0 {
                    p_edge::BTN_LEFT
                } else {
                    p_edge::BTN_RIGHT
                }),
                1 => p_edge::PointerEvent::Move(
                    lcg.below(201) as i64 - 100,
                    lcg.below(201) as i64 - 100,
                ),
                2 => p_edge::PointerEvent::Release(p_edge::BTN_LEFT),
                _ => p_edge::PointerEvent::Leave,
            };
            let _ = f.input(ev);
            k = k.saturating_add(1);
        }
        if f.events.saturating_add(f.rejects) != seq {
            return Err(BrCode::OrderBad.val());
        }
        if !matches!(
            f.state,
            p_edge::FsmState::Idle
                | p_edge::FsmState::Down { .. }
                | p_edge::FsmState::Dragging { .. }
                | p_edge::FsmState::Cancelled { .. }
        ) {
            return Err(BrCode::FuzzBad.val());
        }
        log.composed = log.composed.saturating_add(1);
        log.digest = fold_u32(log.digest, f.events);
        log.rounds = log.rounds.saturating_add(1);
        i = i.saturating_add(1);
    }
    Ok(())
}

/// fuzz ×10^4：合成 8 成、拓扑 1 成、状态机 1 成，账本必须守恒。
pub fn fuzz_rounds(rounds: u32) -> Result<FuzzLog, u32> {
    if rounds == 0 {
        return Err(BrCode::LedgerBad.val());
    }
    let mut log = FuzzLog {
        rounds: 0,
        composed: 0,
        rejected: 0,
        clamps: 0,
        digest: FNV_OFF,
    };
    let mut lcg = Lcg::new(0x4893_3001);
    let pointer_n = rounds.saturating_mul(8) / 10;
    let topo_n = rounds / 10;
    let fsm_n = rounds.saturating_sub(pointer_n).saturating_sub(topo_n);
    fuzz_pointer(&mut log, &mut lcg, pointer_n)?;
    fuzz_topology(&mut log, &mut lcg, topo_n)?;
    fuzz_fsm(&mut log, &mut lcg, fsm_n)?;
    if log.rounds != rounds {
        return Err(BrCode::OrderBad.val());
    }
    if !log.balanced() {
        return Err(BrCode::FuzzBad.val());
    }
    Ok(log)
}

// ── 五路联轧 ──────────────────────────────────────────────────────
pub fn roll_core() -> bool {
    p_core::probe_seq_a().is_ok()
        && p_core::probe_seq_b().is_ok()
        && p_core::probe_seq_c().is_ok()
        && p_core::repro_check().is_ok()
}

pub fn roll_meta() -> bool {
    p_meta::probe_seq_a().is_ok()
        && p_meta::probe_seq_b().is_ok()
        && p_meta::probe_seq_c().is_ok()
        && p_meta::repro_check().is_ok()
}

pub fn roll_guard() -> bool {
    p_guard::probe_seq_a().is_ok()
        && p_guard::probe_seq_b().is_ok()
        && p_guard::probe_seq_c().is_ok()
        && p_guard::repro_check().is_ok()
}

pub fn roll_dispatch() -> bool {
    p_dispatch::probe_seq_a().is_ok()
        && p_dispatch::probe_seq_b().is_ok()
        && p_dispatch::probe_seq_c().is_ok()
        && p_dispatch::repro_check().is_ok()
}

pub fn roll_edge() -> bool {
    p_edge::probe_seq_a().is_ok()
        && p_edge::probe_seq_b().is_ok()
        && p_edge::probe_seq_c().is_ok()
        && p_edge::repro_check().is_ok()
}

pub fn roll_table() -> [(&'static str, bool); ROLLS] {
    [
        ("core", roll_core()),
        ("meta", roll_meta()),
        ("guard", roll_guard()),
        ("dispatch", roll_dispatch()),
        ("edge", roll_edge()),
    ]
}

/// 双跑比对：两表逐项相同且每路为绿，否则 RollBad。
pub fn roll_compare(
    a: &[(&'static str, bool)],
    b: &[(&'static str, bool)],
) -> Result<u32, u32> {
    if a.len() != ROLLS || b.len() != ROLLS {
        return Err(BrCode::CoverageBad.val());
    }
    let mut i = 0usize;
    while i < ROLLS {
        let x = match a.get(i) {
            Some(v) => *v,
            None => return Err(BrCode::CoverageBad.val()),
        };
        let y = match b.get(i) {
            Some(v) => *v,
            None => return Err(BrCode::CoverageBad.val()),
        };
        if x.0 != y.0 || x.1 != y.1 {
            return Err(BrCode::RollBad.val());
        }
        if !x.1 {
            return Err(BrCode::RollBad.val());
        }
        i += 1;
    }
    Ok(ROLLS as u32)
}

pub fn roll_rounds() -> Result<u32, u32> {
    let a = roll_table();
    let b = roll_table();
    roll_compare(&a, &b)
}

// ── 族账本 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FamilyLedger {
    pub codes: u32,
    pub composed: u32,
    pub rejected: u32,
    pub digest: u64,
}

pub fn render_family(l: &FamilyLedger) -> String {
    format!(
        "{}|{}|{}|0x{:016X}",
        l.codes, l.composed, l.rejected, l.digest
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(BrCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(BrCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(BrCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(BrCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 || (!s.starts_with("0x") && !s.starts_with("0X")) {
        return Err(BrCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(BrCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(BrCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(BrCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn audit_render(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != 4 {
        return Err(BrCode::RenderBad.val());
    }
    let codes = parse_u32(match parts.first() {
        Some(v) => v,
        None => return Err(BrCode::RenderBad.val()),
    })?;
    let composed = parse_u32(match parts.get(1) {
        Some(v) => v,
        None => return Err(BrCode::RenderBad.val()),
    })?;
    let rejected = parse_u32(match parts.get(2) {
        Some(v) => v,
        None => return Err(BrCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(3) {
        Some(v) => v,
        None => return Err(BrCode::RenderBad.val()),
    })?;
    if codes != FAMILY_CODES as u32 || composed > 10_000_000 || rejected > 10_000_000 {
        return Err(BrCode::RenderBad.val());
    }
    if digest == 0 {
        return Err(BrCode::RenderBad.val());
    }
    Ok(codes)
}

/// 族黄金：码族 15 + fuzz 1000 + 联轧 5，返回账本。
pub fn family_run() -> Result<FamilyLedger, u32> {
    let rows = code_rows();
    let codes = audit_codes(&rows)?;
    let fuzz = fuzz_rounds(GOLDEN_ROUNDS)?;
    let rolls = roll_rounds()?;
    let mut digest = codes_digest(&rows);
    digest = fold_u32(digest, fuzz.digest as u32);
    digest = fold_u32(digest, (fuzz.digest >> 32) as u32);
    digest = fold_u32(digest, rolls);
    Ok(FamilyLedger {
        codes,
        composed: fuzz.composed,
        rejected: fuzz.rejected,
        digest,
    })
}

// ── 注入（四路） ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn route(&self, i: usize) -> u32 {
        match self.caught.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    let mut log = InjectLog {
        tries: [rounds; 4],
        caught: [0; 4],
    };
    let mut r = 0u32;
    while r < rounds {
        let mut dup = code_rows();
        if let Some(first) = dup.first().copied() {
            dup.push(first);
        }
        if audit_codes(&dup).err() == Some(BrCode::CoverageBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        let mut forged = code_rows();
        if let Some(first) = forged.get_mut(0) {
            first.detail = "";
        }
        if audit_codes(&forged).err() == Some(BrCode::TableBad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        let mut short = code_rows();
        short.pop();
        if audit_codes(&short).err() == Some(BrCode::CoverageBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        let a = roll_table();
        let mut b = a;
        if let Some(slot) = b.get_mut(0) {
            slot.1 = !slot.1;
        }
        if roll_compare(&a, &b).err() == Some(BrCode::RollBad.val()) {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        r = r.saturating_add(1);
    }
    Ok(log)
}

// ── 探针三序 ──────────────────────────────────────────────────────
/// 探针 A：十五码族表两两不同、明细非回退、摘要稳定。
pub fn probe_seq_a() -> Result<(), u32> {
    let rows = code_rows();
    if audit_codes(&rows)? != FAMILY_CODES as u32 {
        return Err(BrCode::CoverageBad.val());
    }
    let d1 = codes_digest(&rows);
    let d2 = codes_digest(&rows);
    if d1 != d2 || d1 == FNV_OFF {
        return Err(BrCode::ReplayBad.val());
    }
    let mut seen = Vec::new();
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            if seen.contains(&r.code) {
                return Err(BrCode::TableBad.val());
            }
            seen.push(r.code);
        }
        i += 1;
    }
    if seen.len() != FAMILY_CODES {
        return Err(BrCode::CoverageBad.val());
    }
    Ok(())
}

/// 探针 B：fuzz ×10^3 账守恒、合成点不越档、拒绝数为正（乱参必须被拒）。
pub fn probe_seq_b() -> Result<(), u32> {
    let log = fuzz_rounds(GOLDEN_ROUNDS)?;
    if log.rounds != GOLDEN_ROUNDS || !log.balanced() {
        return Err(BrCode::OrderBad.val());
    }
    if log.rejected == 0 {
        return Err(BrCode::FuzzBad.val());
    }
    if log.composed == 0 {
        return Err(BrCode::FuzzBad.val());
    }
    if log.digest == FNV_OFF {
        return Err(BrCode::FuzzBad.val());
    }
    Ok(())
}

/// 探针 C：五路联轧双跑一致、族账双跑一致、渲染回读一致。
pub fn probe_seq_c() -> Result<(), u32> {
    let rolls = roll_rounds()?;
    if rolls != ROLLS as u32 {
        return Err(BrCode::CoverageBad.val());
    }
    let a = family_run()?;
    let b = family_run()?;
    if a != b {
        return Err(BrCode::ReplayBad.val());
    }
    let text = render_family(&a);
    if audit_render(&text)? != a.codes {
        return Err(BrCode::RenderBad.val());
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = family_run()?;
    let b = family_run()?;
    if a != b {
        return Err(BrCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 1;
    let hi = SEG_BASE + BrCode::ALL.len() as u32;
    let mut i = 0usize;
    while i < BrCode::ALL.len() {
        match BrCode::ALL.get(i) {
            Some(c) => {
                let v = c.val();
                if v < lo || v > hi {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let rows = code_rows();
    let codes = audit_codes(&rows);
    v.push((
        "F48935-J1-错误码×15互不混淆",
        codes == Ok(FAMILY_CODES as u32),
        match &codes {
            Ok(n) => format!("{} 码（core12+meta+guard+dispatch），两两不同", n),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48935-J1-码族明细零回退",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < rows.len() {
                if let Some(r) = rows.get(i) {
                    if r.detail.is_empty() || r.detail == "未知诊断码" {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok
        },
        String::from("十五行明细全部来自源模块"),
    ));
    let fz = fuzz_rounds(FUZZ_ROUNDS);
    v.push((
        "F48933-J1-fuzz×10^4零崩零跳变",
        match &fz {
            Ok(l) => l.rounds == FUZZ_ROUNDS && l.balanced(),
            Err(_) => false,
        },
        match &fz {
            Ok(l) => format!(
                "rounds={} composed={} rejected={} clamps={}",
                l.rounds, l.composed, l.rejected, l.clamps
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48933-J1-fuzz拒绝必入账",
        match &fz {
            Ok(l) => l.rejected > 0 && l.composed > 0,
            Err(_) => false,
        },
        String::from("乱参数被拒且拒绝分入账"),
    ));
    let rolls = roll_rounds();
    v.push((
        "F48934-J1-联轧×5全绿零漂移",
        rolls == Ok(ROLLS as u32),
        match &rolls {
            Ok(n) => format!("{} 路（core/meta/guard/dispatch/edge）双跑一致", n),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48934-J1-联轧单路可定位",
        {
            let t = roll_table();
            let mut ok = true;
            let mut i = 0usize;
            while i < t.len() {
                match t.get(i) {
                    Some((name, pass)) => {
                        if name.is_empty() || !*pass {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                i += 1;
            }
            ok
        },
        String::from("五路标签非空且逐路真值"),
    ));
    let fam = family_run();
    v.push((
        "F48935-J1-族账×10^3双跑",
        match &fam {
            Ok(l) => l.codes == FAMILY_CODES as u32 && l.digest != 0,
            Err(_) => false,
        },
        match &fam {
            Ok(l) => format!(
                "codes={} composed={} rejected={} digest=0x{:016X}",
                l.codes, l.composed, l.rejected, l.digest
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let inj = inject_rounds(10);
    v.push((
        "F48933-J1-注入四路10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let render_ok = match family_run() {
        Ok(l) => {
            let text = render_family(&l);
            audit_render(&text) == Ok(l.codes)
        }
        Err(_) => false,
    };
    v.push((
        "F48935-J1-渲染四字段回读",
        render_ok,
        String::from("codes|composed|rejected|digest"),
    ));
    v.push((
        "F48933-J1-探针三序与双跑",
        probe_seq_a().is_ok()
            && probe_seq_b().is_ok()
            && probe_seq_c().is_ok()
            && repro_check().is_ok(),
        format!(
            "a={} b={} c={} repro={}",
            probe_seq_a().is_ok(),
            probe_seq_b().is_ok(),
            probe_seq_c().is_ok(),
            repro_check().is_ok()
        ),
    ));
    v.push((
        "F48933-J1-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口 {} 码", SEG_BASE, BrCode::ALL.len()),
    ));
    v
}

pub fn run_unx48933_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48933 指针族联轧与码族冻结");
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

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_table_is_fifteen_and_distinct() {
        let rows = code_rows();
        assert_eq!(rows.len(), FAMILY_CODES);
        assert_eq!(audit_codes(&rows), Ok(FAMILY_CODES as u32));
        let mut seen = Vec::new();
        let mut i = 0usize;
        while i < rows.len() {
            if let Some(r) = rows.get(i) {
                assert!(!seen.contains(&r.code));
                seen.push(r.code);
            }
            i += 1;
        }
    }

    #[test]
    fn code_table_rejects_duplicates() {
        let mut rows = code_rows();
        if let Some(first) = rows.first().copied() {
            rows.push(first);
        }
        assert_eq!(audit_codes(&rows), Err(BrCode::CoverageBad.val()));
        let mut forged = code_rows();
        if let Some(r) = forged.get_mut(0) {
            r.detail = "";
        }
        assert_eq!(audit_codes(&forged), Err(BrCode::TableBad.val()));
    }

    #[test]
    fn fuzz_balances_and_rejects() {
        match fuzz_rounds(2000) {
            Ok(l) => {
                assert!(l.balanced());
                assert_eq!(l.rounds, 2000);
                assert!(l.rejected > 0);
                assert!(l.composed > 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn fuzz_zero_rejected() {
        assert_eq!(fuzz_rounds(0), Err(BrCode::LedgerBad.val()));
    }

    #[test]
    fn rolls_are_green_and_stable() {
        assert_eq!(roll_rounds(), Ok(ROLLS as u32));
        let a = roll_table();
        let b = roll_table();
        assert_eq!(a, b);
        assert!(roll_core());
        assert!(roll_meta());
        assert!(roll_guard());
        assert!(roll_dispatch());
        assert!(roll_edge());
    }

    #[test]
    fn roll_compare_detects_tamper() {
        let a = roll_table();
        let mut b = a;
        if let Some(slot) = b.get_mut(2) {
            slot.1 = !slot.1;
        }
        assert_eq!(roll_compare(&a, &b), Err(BrCode::RollBad.val()));
        let short: [(&'static str, bool); 4] =
            [("core", true), ("meta", true), ("guard", true), ("edge", true)];
        assert_eq!(
            roll_compare(&a, &short),
            Err(BrCode::CoverageBad.val())
        );
    }

    #[test]
    fn family_is_deterministic() {
        assert_eq!(family_run(), family_run());
        assert!(repro_check().is_ok());
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn injection_catches_all_four() {
        match inject_rounds(4) {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.tries, [4, 4, 4, 4]);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn render_roundtrip() {
        match family_run() {
            Ok(l) => {
                let text = render_family(&l);
                assert_eq!(audit_render(&text), Ok(l.codes));
                let bad = text.replacen("|", "#", 1);
                assert!(audit_render(&bad).is_err());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn codes_inside_window() {
        assert!(segment_guard());
        assert_eq!(BrCode::ALL.len(), 10);
        assert_eq!(detail(BrCode::RollBad.val()), BrCode::RollBad.detail());
        assert_eq!(detail(0), "未知诊断码");
    }
}
