//! UNX-F48953 滚轮 fuzz · 0x71300
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，UNX-M2-B08）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F48953-J1 fuzz ×10^4 零崩；
//!   ③ UNX-F48954-J1 联轧 ×5 全绿零漂移；
//!   ④ UNX-F48955-J1 错误码 ×15 零漂移互不混淆。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：四路 fuzz 轮转 → 正反样本守恒 → 五路联轧双跑 →
//!           错误码闭集与互异校验。
//! 先守恒再联轧：fuzz 的每次尝试只有「接受/拒绝」两种去向，若记账
//! 不等，后面的联轧绿不绿都没有意义——那只是把漏账藏进了绿里。
//! 五路联轧必须双跑：单跑全绿可能只是确定性伪随机恰好落在好路上；
//! 双跑逐项相同才证明本族行为可重放。
//! 关键差异：本模块只回答「乱输入不崩、五路可复跑、错误码互不相同」，
//!           不做质量面的 20 例用例集（B08-QUALITY）、不做反重 grep
//!           （B08-SEAL）。fuzz 的随机源固定种子，跨机器可复现。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;
use super::unx48943_wheel_meta as p_meta;
use super::unx48945_wheel_guard as p_guard;
use super::unx48948_wheel_dispatch as p_dispatch;
use super::unx48951_wheel_edge as p_edge;

/// 诊断段基址（0x71300，B08 族第六槽）。
pub const SEG_BASE: u32 = 0x71300;

/// fuzz 总轮数（卡面判据 ×10^4 的载体）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// fuzz 路线数。
pub const FUZZ_ROUTES: usize = 4;
/// 联轧路线数。
pub const ROLLS: usize = 5;
/// 错误码族条数（卡面判据 ×15）。
pub const CODE_FAMILY: usize = 15;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0F，共 15 条） ───────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HbCode {
    /// fuzz 路线非法
    RouteBad,
    /// fuzz 记账不平
    FuzzBad,
    /// 联轧清单长度非法
    CoverageBad,
    /// 联轧双跑不一致
    RollBad,
    /// 联轧出现红路
    GreenBad,
    /// 错误码族长度非法
    FamilyBad,
    /// 错误码重复
    DupBad,
    /// 错误码逃出窗口
    SegmentBad,
    /// 账本渲染非法
    RenderBad,
    /// 复算不一致
    ReplayBad,
    /// 探针不足
    ProbeBad,
    /// 轮数非法
    RoundsBad,
    /// 随机源漂移
    SeedBad,
    /// 聚合非法
    MergeBad,
    /// 顺序非法
    OrderBad,
}

impl HbCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            HbCode::RouteBad => "fuzz 路线非法：编号超出四路闭集",
            HbCode::FuzzBad => "fuzz 记账不平：接受+拒绝≠尝试",
            HbCode::CoverageBad => "联轧清单长度非法：不是五路",
            HbCode::RollBad => "联轧双跑不一致：两表逐项不同",
            HbCode::GreenBad => "联轧出现红路：至少一路未过",
            HbCode::FamilyBad => "错误码族长度非法：不是 15 条",
            HbCode::DupBad => "错误码重复：族内出现同值",
            HbCode::SegmentBad => "错误码逃出窗口：段位越界",
            HbCode::RenderBad => "账本渲染非法：字段缺失或回读失败",
            HbCode::ReplayBad => "复算不一致：同一输入重放不同",
            HbCode::ProbeBad => "探针不足：路线或样本数不足",
            HbCode::RoundsBad => "轮数非法：为零或超界",
            HbCode::SeedBad => "随机源漂移：同种子结果不同",
            HbCode::MergeBad => "聚合非法：合并后的计数不守恒",
            HbCode::OrderBad => "顺序非法：账本字段次序被破坏",
        }
    }

    pub const ALL: [HbCode; CODE_FAMILY] = [
        HbCode::RouteBad,
        HbCode::FuzzBad,
        HbCode::CoverageBad,
        HbCode::RollBad,
        HbCode::GreenBad,
        HbCode::FamilyBad,
        HbCode::DupBad,
        HbCode::SegmentBad,
        HbCode::RenderBad,
        HbCode::ReplayBad,
        HbCode::ProbeBad,
        HbCode::RoundsBad,
        HbCode::SeedBad,
        HbCode::MergeBad,
        HbCode::OrderBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < HbCode::ALL.len() {
        if HbCode::ALL[i].val() == code {
            return HbCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < HbCode::ALL.len() {
        let v = HbCode::ALL[i].val();
        if v <= SEG_BASE || v > SEG_BASE + 0x3F {
            return false;
        }
        i += 1;
    }
    true
}

pub fn family_is_distinct() -> bool {
    let mut i = 0usize;
    while i < HbCode::ALL.len() {
        let mut j = i + 1;
        while j < HbCode::ALL.len() {
            if HbCode::ALL[i].val() == HbCode::ALL[j].val() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 四路 fuzz ─────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzStats {
    pub attempts: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub faults: u32,
    pub digest: u64,
    pub per_route: [[u32; 3]; FUZZ_ROUTES],
}

impl FuzzStats {
    pub fn new() -> FuzzStats {
        FuzzStats {
            attempts: 0,
            accepted: 0,
            rejected: 0,
            faults: 0,
            digest: p_core::FNV_OFF,
            per_route: [[0; 3]; FUZZ_ROUTES],
        }
    }

    /// 记账：`expected` 是「系统对本次样本的处置是否符合预期」（不符即记 fault），
    /// `inject_bad` 区分样本类别（合法 / 非法）。首版把 `expected` 同时当
    /// 样本类别用，于是每路「拒绝样本」计数恒为 0，`route_cover` 永远不成立。
    fn tally(&mut self, route: usize, expected: bool, inject_bad: bool) {
        if route >= FUZZ_ROUTES {
            self.faults = self.faults.saturating_add(1);
            return;
        }
        if let Some(row) = self.per_route.get_mut(route) {
            if let Some(a) = row.get_mut(0) {
                *a = a.saturating_add(1);
            }
            let idx = if inject_bad { 2 } else { 1 };
            if let Some(c) = row.get_mut(idx) {
                *c = c.saturating_add(1);
            }
        }
        self.attempts = self.attempts.saturating_add(1);
        if inject_bad {
            self.rejected = self.rejected.saturating_add(1);
        } else {
            self.accepted = self.accepted.saturating_add(1);
        }
        if !expected {
            self.faults = self.faults.saturating_add(1);
        }
        self.digest = p_core::fold_u32(self.digest, route as u32);
        self.digest = p_core::fold_u32(self.digest, expected as u32);
        self.digest = p_core::fold_u32(self.digest, inject_bad as u32);
    }

    pub fn balanced(&self) -> bool {
        self.attempts == self.accepted.saturating_add(self.rejected) && self.faults == 0
    }

    pub fn route_cover(&self) -> bool {
        let mut i = 0usize;
        while i < FUZZ_ROUTES {
            match self.per_route.get(i) {
                Some(row) => {
                    if row[0] == 0 || row[1] == 0 || row[2] == 0 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.attempts == 0 {
            return Err(HbCode::ProbeBad.val());
        }
        if !self.balanced() {
            return Err(HbCode::FuzzBad.val());
        }
        if !self.route_cover() {
            return Err(HbCode::CoverageBad.val());
        }
        let mut i = 0usize;
        let mut sum = 0u32;
        while i < FUZZ_ROUTES {
            if let Some(row) = self.per_route.get(i) {
                sum = sum.saturating_add(row[0]);
            }
            i += 1;
        }
        if sum != self.attempts {
            return Err(HbCode::FuzzBad.val());
        }
        Ok(())
    }
}

/// 单路 fuzz：每 7 次尝试注入一次非法样本，其余合法。
pub fn fuzz_route(route: usize, rounds: u32) -> Result<(u32, u32, u64), u32> {
    if route >= FUZZ_ROUTES {
        return Err(HbCode::RouteBad.val());
    }
    if rounds == 0 {
        return Err(HbCode::RoundsBad.val());
    }
    let mut rng = p_core::Lcg::new(0x8A11_0053_u64 + route as u64);
    let mut ok_n = 0u32;
    let mut bad_n = 0u32;
    let mut h = p_core::FNV_OFF;
    let mut i = 0u32;
    while i < rounds {
        let inject_bad = i % 7 == 0;
        let ok = match route {
            0 => {
                let d = if inject_bad {
                    0
                } else {
                    let span = rng.below(p_core::DELTA_MAX as u32 - 1);
                    rng.signed(1 + span)
                };
                let mut a = p_core::WheelAccumulator::vertical();
                let res = a.push(d);
                if !inject_bad {
                    res.is_ok() && a.audit().is_ok()
                } else {
                    res.is_err()
                }
            }
            1 => {
                let d = if inject_bad {
                    0
                } else {
                    let span = rng.below(p_meta::FINE_MAX as u32 - 1);
                    rng.signed(1 + span)
                };
                let mut s = p_meta::HiResStream::vertical();
                let res = s.push_fine(d);
                if !inject_bad {
                    res.is_ok() && s.audit().is_ok()
                } else {
                    res.is_err()
                }
            }
            2 => {
                let mut r = p_guard::WheelRouter::new();
                if inject_bad {
                    r.set_hover(Some(0)).is_err()
                } else {
                    let hv = 1 + rng.below(999_999);
                    r.set_hover(Some(hv)).is_ok() && r.deliver().is_ok()
                }
            }
            _ => {
                let mut a = p_dispatch::AxisLock::new();
                let _ = a.begin();
                let res = if inject_bad {
                    a.feed(0, 0)
                } else {
                    a.feed(rng.signed(40), rng.signed(40))
                };
                if inject_bad {
                    res.is_err()
                } else {
                    res.is_ok()
                }
            }
        };
        // `ok` 是「处置符合预期」而非「样本被接受」：合法样本应被接住，
        // 非法样本应被拒绝。先验处置，再按**样本类别**分类计数；首版把
        // 处置当类别，导致 bad 恒为 0，探针「正反两侧都有」永远不成立。
        if !ok {
            return Err(HbCode::FuzzBad.val());
        }
        if inject_bad {
            bad_n = bad_n.saturating_add(1);
        } else {
            ok_n = ok_n.saturating_add(1);
        }
        h = p_core::fold_u32(h, i);
        h = p_core::fold_u32(h, ok as u32);
        i = i.saturating_add(1);
    }
    Ok((ok_n, bad_n, h))
}

/// 四路轮转 fuzz ×10^4。
pub fn fuzz_face(rounds: u32) -> Result<FuzzStats, u32> {
    if rounds == 0 {
        return Err(HbCode::RoundsBad.val());
    }
    let mut stats = FuzzStats::new();
    let mut rng = p_core::Lcg::new(0x8A11_0054);
    let mut i = 0u32;
    while i < rounds {
        // 路线取**高位**：LCG 低位周期短（低 k 位周期 2^k），用 `below(4)`
        // 会与轮号 i 的模数同步锁死在同一条路上（实测 10^4 轮只有 route 0），
        // 四路覆盖判据永远不成立。
        let route = ((rng.next() >> 33) % FUZZ_ROUTES as u64) as usize;
        let inject_bad = i % 11 == 0;
        let ok = match route {
            0 => {
                let d = if inject_bad {
                    0
                } else {
                    let span = rng.below(10_000);
                    rng.signed(1 + span)
                };
                let mut a = p_core::WheelAccumulator::vertical();
                let res = a.push(d);
                if inject_bad { res.is_err() } else { res.is_ok() }
            }
            1 => {
                let d = if inject_bad {
                    p_meta::FINE_MAX + 1
                } else {
                    let span = rng.below(1_000);
                    rng.signed(1 + span)
                };
                let mut s = p_meta::HiResStream::horizontal();
                let res = s.push_fine(d);
                if inject_bad { res.is_err() } else { res.is_ok() }
            }
            2 => {
                let mut r = p_guard::WheelRouter::new();
                if inject_bad {
                    r.set_focus(Some(0)).is_err()
                } else {
                    r.set_focus(Some(1 + rng.below(999_999))).is_ok()
                }
            }
            _ => {
                let mut a = p_dispatch::AxisLock::new();
                let _ = a.begin();
                let res = if inject_bad {
                    a.feed(0, 0)
                } else {
                    a.feed(rng.signed(50), rng.signed(50))
                };
                if inject_bad { res.is_err() } else { res.is_ok() }
            }
        };
        stats.tally(route, ok, inject_bad);
        i = i.saturating_add(1);
    }
    stats.audit()?;
    Ok(stats)
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
        return Err(HbCode::CoverageBad.val());
    }
    let mut i = 0usize;
    while i < ROLLS {
        let x = match a.get(i) {
            Some(v) => *v,
            None => return Err(HbCode::CoverageBad.val()),
        };
        let y = match b.get(i) {
            Some(v) => *v,
            None => return Err(HbCode::CoverageBad.val()),
        };
        if x.0 != y.0 || x.1 != y.1 {
            return Err(HbCode::RollBad.val());
        }
        if !x.1 {
            return Err(HbCode::GreenBad.val());
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

// ── fuzz 账本渲染 ─────────────────────────────────────────────────
pub fn render_fuzz(s: &FuzzStats) -> String {
    format!(
        "{}|{}|{}|{}|0x{:016X}",
        s.attempts, s.accepted, s.rejected, s.faults, s.digest
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(HbCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(HbCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(HbCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(HbCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 || (!s.starts_with("0x") && !s.starts_with("0X")) {
        return Err(HbCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(HbCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(HbCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(HbCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn audit_render(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != 5 {
        return Err(HbCode::RenderBad.val());
    }
    let attempts = parse_u32(match parts.first() {
        Some(v) => v,
        None => return Err(HbCode::RenderBad.val()),
    })?;
    let accepted = parse_u32(match parts.get(1) {
        Some(v) => v,
        None => return Err(HbCode::RenderBad.val()),
    })?;
    let rejected = parse_u32(match parts.get(2) {
        Some(v) => v,
        None => return Err(HbCode::RenderBad.val()),
    })?;
    let faults = parse_u32(match parts.get(3) {
        Some(v) => v,
        None => return Err(HbCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(4) {
        Some(v) => v,
        None => return Err(HbCode::RenderBad.val()),
    })?;
    if attempts == 0 || attempts > 10_000_000 {
        return Err(HbCode::RenderBad.val());
    }
    if accepted.saturating_add(rejected) != attempts {
        return Err(HbCode::FuzzBad.val());
    }
    if faults != 0 {
        return Err(HbCode::FuzzBad.val());
    }
    if digest == 0 {
        return Err(HbCode::RenderBad.val());
    }
    Ok(attempts)
}

pub fn golden_run() -> Result<(u32, u32, u64), u32> {
    let s = fuzz_face(FUZZ_ROUNDS)?;
    let text = render_fuzz(&s);
    let attempts = audit_render(&text)?;
    if attempts != s.attempts || s.accepted == 0 || s.rejected == 0 {
        return Err(HbCode::ReplayBad.val());
    }
    Ok((attempts, s.rejected, s.digest))
}

// ── 注入对账（四路缺陷必须被捕获） ────────────────────────────────
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
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 {
        return Err(HbCode::RoundsBad.val());
    }
    let mut log = InjectLog {
        tries: [0; 4],
        caught: [0; 4],
    };
    let mut i = 0u32;
    while i < rounds {
        // ① 路线越界必须被拒。
        if fuzz_route(FUZZ_ROUTES, 8).err() == Some(HbCode::RouteBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        log.tries[0] = log.tries[0].saturating_add(1);
        // ② 记账被破坏必须炸 FuzzBad。
        let mut s = FuzzStats::new();
        s.tally(0, true, false);
        s.accepted = s.accepted.saturating_add(1);
        if s.audit().err() == Some(HbCode::FuzzBad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        log.tries[1] = log.tries[1].saturating_add(1);
        // ③ 红路必须炸 GreenBad。
        let table = [("a", true), ("b", false), ("c", true), ("d", true), ("e", true)];
        let same = roll_compare(&table, &table);
        if same.err() == Some(HbCode::GreenBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        log.tries[2] = log.tries[2].saturating_add(1);
        // ④ 渲染篡改必须被拒（基底账先造平，确保拒绝来自篡改本身）。
        let mut f = FuzzStats::new();
        f.tally(0, true, false);
        f.tally(0, true, true);
        let text = render_fuzz(&f).replacen("0x", "0y", 1);
        if audit_render(&text).is_err() {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        log.tries[3] = log.tries[3].saturating_add(1);
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(HbCode::CoverageBad.val());
    }
    Ok(log)
}

// ── 探针 ──────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let s = fuzz_face(2_000)?;
    if !s.balanced() || s.accepted == 0 || s.rejected == 0 {
        return Err(HbCode::FuzzBad.val());
    }
    if !s.route_cover() {
        return Err(HbCode::CoverageBad.val());
    }
    let mut r = 0usize;
    while r < FUZZ_ROUTES {
        let (ok_n, bad_n, _) = fuzz_route(r, 50)?;
        if ok_n == 0 || bad_n == 0 || ok_n + bad_n != 50 {
            return Err(HbCode::FuzzBad.val());
        }
        r += 1;
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    if roll_rounds()? != ROLLS as u32 {
        return Err(HbCode::RollBad.val());
    }
    let t = roll_table();
    let mut i = 0usize;
    while i < ROLLS {
        match t.get(i) {
            Some((_, true)) => {}
            _ => return Err(HbCode::GreenBad.val()),
        }
        i += 1;
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    if HbCode::ALL.len() != CODE_FAMILY {
        return Err(HbCode::FamilyBad.val());
    }
    if !family_is_distinct() {
        return Err(HbCode::DupBad.val());
    }
    if !segment_guard() {
        return Err(HbCode::SegmentBad.val());
    }
    let a = fuzz_face(500)?;
    let b = fuzz_face(500)?;
    if a.digest != b.digest || a.attempts != b.attempts {
        return Err(HbCode::ReplayBad.val());
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = fuzz_face(800)?;
    let b = fuzz_face(800)?;
    if a.digest != b.digest || a.per_route != b.per_route {
        return Err(HbCode::ReplayBad.val());
    }
    if roll_rounds()? != ROLLS as u32 {
        return Err(HbCode::RollBad.val());
    }
    Ok(())
}

// ── 域检查项 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    let fuzz = fuzz_face(FUZZ_ROUNDS);
    out.push((
        "F48953 fuzz×10^4零崩",
        matches!(&fuzz, Ok(s) if s.audit().is_ok()),
        match &fuzz {
            Ok(s) => format!(
                "attempts={} ok={} bad={} faults={}",
                s.attempts, s.accepted, s.rejected, s.faults
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out.push((
        "F48953 四路正反样本",
        matches!(&fuzz, Ok(s) if s.route_cover()),
        match &fuzz {
            Ok(s) => {
                let mut d = String::new();
                let mut i = 0usize;
                while i < FUZZ_ROUTES {
                    if let Some(row) = s.per_route.get(i) {
                        d.push_str(&format!("r{}=({},{}) ", i, row[1], row[2]));
                    }
                    i += 1;
                }
                d
            }
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let roll = roll_rounds();
    out.push((
        "F48954 联轧×5双跑",
        roll == Ok(ROLLS as u32),
        match &roll {
            Ok(n) => format!("rolls={}", n),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let table = roll_table();
    let mut names = String::new();
    let mut i = 0usize;
    let mut all_green = true;
    while i < ROLLS {
        match table.get(i) {
            Some((n, ok)) => {
                if i > 0 {
                    names.push(',');
                }
                names.push_str(n);
                if !*ok {
                    all_green = false;
                }
            }
            None => all_green = false,
        }
        i += 1;
    }
    out.push((
        "F48954 五路全绿",
        all_green,
        names,
    ));

    out.push((
        "F48955 错误码×15",
        HbCode::ALL.len() == CODE_FAMILY,
        format!("family={}", HbCode::ALL.len()),
    ));

    out.push((
        "F48955 错误码互不混淆",
        family_is_distinct(),
        String::from("逐对比较 val 唯一"),
    ));

    out.push((
        "F48955 错误码段内闭集",
        segment_guard(),
        format!("seg=0x{:04X} last=0x{:04X}", SEG_BASE, HbCode::ALL[CODE_FAMILY - 1].val()),
    ));

    let render = golden_run();
    out.push((
        "F48953 fuzz账可解析",
        render.is_ok(),
        match &render {
            Ok((a, r, d)) => format!("attempts={} rejected={} d=0x{:X}", a, r, d),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out
}

pub fn run_unx48953_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48953 滚轮 fuzz 与段内联轧");
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
    fn fuzz_face_balances() {
        match fuzz_face(1_000) {
            Ok(s) => {
                assert_eq!(s.attempts, s.accepted + s.rejected);
                assert_eq!(s.faults, 0);
                assert!(s.route_cover());
                assert!(s.audit().is_ok());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn fuzz_face_is_deterministic() {
        let a = fuzz_face(500);
        let b = fuzz_face(500);
        match (a, b) {
            (Ok(x), Ok(y)) => {
                assert_eq!(x.digest, y.digest);
                assert_eq!(x.per_route, y.per_route);
            }
            _ => assert!(false),
        }
    }

    #[test]
    fn fuzz_rejects_zero_rounds() {
        assert_eq!(fuzz_face(0), Err(HbCode::RoundsBad.val()));
    }

    #[test]
    fn fuzz_route_has_both_sides() {
        let mut r = 0usize;
        while r < FUZZ_ROUTES {
            match fuzz_route(r, 70) {
                Ok((ok, bad, _)) => {
                    assert!(ok > 0, "route {}", r);
                    assert!(bad > 0, "route {}", r);
                    assert_eq!(ok + bad, 70);
                }
                Err(e) => panic!("route {} err=0x{:X}", r, e),
            }
            r += 1;
        }
    }

    #[test]
    fn fuzz_route_rejects_bad_index() {
        assert_eq!(fuzz_route(FUZZ_ROUTES, 10), Err(HbCode::RouteBad.val()));
    }

    #[test]
    fn roll_is_green() {
        assert_eq!(roll_rounds(), Ok(ROLLS as u32));
        assert!(roll_core());
        assert!(roll_meta());
        assert!(roll_guard());
        assert!(roll_dispatch());
        assert!(roll_edge());
    }

    #[test]
    fn roll_compare_catches_red() {
        let bad = [("a", true), ("b", true), ("c", false), ("d", true), ("e", true)];
        assert_eq!(roll_compare(&bad, &bad), Err(HbCode::GreenBad.val()));
    }

    #[test]
    fn roll_compare_catches_length() {
        let short = [("a", true)];
        assert_eq!(roll_compare(&short, &short), Err(HbCode::CoverageBad.val()));
    }

    #[test]
    fn family_has_fifteen_distinct() {
        assert_eq!(HbCode::ALL.len(), 15);
        assert!(family_is_distinct());
        assert!(segment_guard());
    }

    #[test]
    fn family_codes_fit_window() {
        let mut i = 0usize;
        while i < HbCode::ALL.len() {
            let c = HbCode::ALL[i];
            assert!(c.val() > SEG_BASE);
            assert!(c.val() <= SEG_BASE + 0x0F);
            assert!(!c.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn tally_counts_routes() {
        let mut s = FuzzStats::new();
        s.tally(0, true, false);
        s.tally(0, true, true);
        s.tally(1, true, false);
        assert_eq!(s.attempts, 3);
        assert_eq!(s.accepted, 2);
        assert_eq!(s.rejected, 1);
        assert!(s.balanced());
        assert!(!s.route_cover());
    }

    #[test]
    fn render_roundtrip_and_tamper() {
        let mut s = FuzzStats::new();
        s.tally(0, true, false);
        s.tally(0, true, true);
        let text = render_fuzz(&s);
        assert_eq!(audit_render(&text), Ok(2));
        assert!(audit_render(&text.replacen("0x", "0y", 1)).is_err());
    }

    #[test]
    fn render_rejects_unbalanced() {
        let mut s = FuzzStats::new();
        s.tally(0, true, false);
        s.accepted = s.accepted.saturating_add(1);
        assert_eq!(audit_render(&render_fuzz(&s)), Err(HbCode::FuzzBad.val()));
    }

    #[test]
    fn injection_catches_four() {
        match inject_rounds(5) {
            Ok(l) => assert!(l.all_caught()),
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_are_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn repro_is_deterministic() {
        assert!(repro_check().is_ok());
        assert_eq!(golden_run(), golden_run());
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48953_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
        assert!(!s.truncated());
    }
}
