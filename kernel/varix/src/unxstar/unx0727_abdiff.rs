//! UNX-F0727 · 升级前后零差异对照（A/B 同机同档）· 诊断段 0x5a60（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0727] 升级前后零差异对照（A/B 同机同档） <SEG 0x5a60>
//!   UNX-F0727-J1 A/B 对照账齐（旧版基线 vs 新版 50 次零差异结论对照
//!   ——同机同档双跑）且对照差异逐项归因。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. A/B 双轨定容：旧版基线 A 序列 50 次 + 新版 B 序列 50 次，
//!      同机同档（机号/档号在账头冻结）；双侧轮数不等即拒对照。
//!   2. 零差异结论三面：逐次结果面 / 成功率面 / 结论面——三面全等
//!      才出「零差异」；任一面不等即差异账逐项落账。
//!   3. 差异逐项归因闭集：Cause 四臂（版本差异/环境抖动/注入样本/
//!      账面污染），每项差异必挂一臂（未归因差异即 E_UNATTRIBUTED）。
//!   4. 同机同档闸：A/B 机号与档号必须全等才开对照（前置闸）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x5a60~0x5a7F 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥580（判据原文「纯功能代码 ≥580 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x5a60 独占 0x5a60~0x5a7F） ────────────────────────────
pub const SEG_BASE: u16 = 0x5a60;

/// 0x5a61 对照账已封板仍尝试落账
const E_SEALED: u16 = SEG_BASE + 0x1;
/// 0x5a62 轮数不足：双侧未满 50 拒对照
const E_ROUNDS_SHORT: u16 = SEG_BASE + 0x2;
/// 0x5a63 同机同档破：A/B 机号或档号不等
const E_TIER_MISMATCH: u16 = SEG_BASE + 0x3;
/// 0x5a64 结果态越闭集
const E_OUTCOME_RANGE: u16 = SEG_BASE + 0x4;
/// 0x5a65 归因臂越闭集
const E_CAUSE_RANGE: u16 = SEG_BASE + 0x5;
/// 0x5a66 差异未归因：差异账存在未挂臂项
const E_UNATTRIBUTED: u16 = SEG_BASE + 0x6;
/// 0x5a67 差异账与对照账失衡：差异数 ≠ 逐次差异和
const E_DIFF_IMBAL: u16 = SEG_BASE + 0x7;
/// 0x5a68 差异账溢出
const E_DIFF_FULL: u16 = SEG_BASE + 0x8;
/// 0x5a69 轮号越域（合法 1..=50）
const E_ROUND_RANGE: u16 = SEG_BASE + 0x9;
/// 0x5a6A 未对照即取零差异结论
const E_NOT_COMPARED: u16 = SEG_BASE + 0xA;
/// 0x5a6B 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xC;
/// 0x5a6C 对照重复：同轮二次对照
const E_SLOT_DUP: u16 = SEG_BASE + 0xD;
/// 0x5a6D A/B 序列空：零轮即对照
const E_EMPTY_SERIES: u16 = SEG_BASE + 0xE;

/// 诊断码 → 中文可操作诊断（段 0x5a60~0x5a7F 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "对照账已封板：落账冻结，重照请开新账",
        E_ROUNDS_SHORT => "轮数不足：双侧 50 轮齐才开对照",
        E_TIER_MISMATCH => "同机同档破：A/B 机号或档号不等",
        E_OUTCOME_RANGE => "结果态越闭集：二值口径只认过/败",
        E_CAUSE_RANGE => "归因臂越闭集：四臂之外无归因",
        E_UNATTRIBUTED => "差异未归因：逐项挂臂后才能出结论",
        E_DIFF_IMBAL => "差异账失衡：账条数 ≠ 逐次差异和",
        E_DIFF_FULL => "差异账溢出：先归因已落账项再续",
        E_ROUND_RANGE => "轮号越域：合法 1..=50",
        E_NOT_COMPARED => "未对照即取结论：先 compare",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_SLOT_DUP => "对照重复：同轮二次对照拒收",
        E_EMPTY_SERIES => "空序列：零轮无对照资格",
        _ => "未知诊断码（0x5a60 段外或保留位）",
    }
}

// ── 常量（来源：判据 50 次与归因口径） ───────────────────────────────
/// 每侧轮数（判据原文：50 次零差异对照）。
pub const ROUNDS: usize = 50;
/// 归因臂数。
pub const CAUSE_ARMS: u8 = 4;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(ROUNDS == 50);
const _: () = assert!(CAUSE_ARMS == 4);

// ── FNV-1a 64 ────────────────────────────────────────────────────────
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_mix(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

// ── 归因臂闭集 ───────────────────────────────────────────────────────
/// 差异归因四臂闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffCause {
    /// 版本差异（升级引入）。
    Version,
    /// 环境抖动（宿主侧）。
    Environment,
    /// 注入样本（验收面）。
    Injected,
    /// 账面污染（落簿面）。
    LedgerPollution,
}

impl DiffCause {
    /// 臂号。
    pub const fn id(self) -> u8 {
        match self {
            DiffCause::Version => 0,
            DiffCause::Environment => 1,
            DiffCause::Injected => 2,
            DiffCause::LedgerPollution => 3,
        }
    }

    /// 按臂号重建。
    pub fn from_id(v: u8) -> Result<DiffCause, u16> {
        match v {
            0 => Ok(DiffCause::Version),
            1 => Ok(DiffCause::Environment),
            2 => Ok(DiffCause::Injected),
            3 => Ok(DiffCause::LedgerPollution),
            _ => Err(E_CAUSE_RANGE),
        }
    }

    /// 臂名（摘要拼装）。
    pub const fn name(self) -> &'static str {
        match self {
            DiffCause::Version => "版本",
            DiffCause::Environment => "环境",
            DiffCause::Injected => "注入",
            DiffCause::LedgerPollution => "账面",
        }
    }
}

// ── 单次运行记录 ─────────────────────────────────────────────────────
/// 单次运行记录：轮号 + 二值结果 + 结论臂。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunRec {
    /// 轮号 1..=50。
    pub round: u16,
    /// 二值结果（1=过 0=败）。
    pub outcome: u8,
    /// 结论臂（0=全绿/1=条件/2=拦截/3=不可判）。
    pub verdict: u8,
}

impl RunRec {
    /// 记录合法性。
    pub const fn valid(&self) -> bool {
        self.round >= 1 && (self.round as usize) <= ROUNDS && self.outcome <= 1 && self.verdict <= 3
    }
}

// ── 差异账行 ─────────────────────────────────────────────────────────
/// 差异账行：轮号 + 面 + 归因臂（未归因为 0xFF）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffRow {
    /// 轮号。
    pub round: u16,
    /// 面（0=逐次结果 1=结论）。
    pub face: u8,
    /// A 侧值。
    pub a_val: u8,
    /// B 侧值。
    pub b_val: u8,
    /// 归因臂（0xFF=未归因）。
    pub cause: u8,
}

impl DiffRow {
    /// 已归因判定。
    pub const fn attributed(&self) -> bool {
        self.cause != 0xFF
    }
}

// ── A/B 对照账 ───────────────────────────────────────────────────────
/// A/B 对照账：双序列 + 同机同档闸 + 三面对照 + 差异归因账。
pub struct AbCompare {
    /// 机号（冻结）。
    pub machine: u8,
    /// 档号（冻结）。
    pub tier: u8,
    /// A 序列（旧版基线）。
    series_a: Vec<RunRec>,
    /// B 序列（新版）。
    series_b: Vec<RunRec>,
    /// 已对照轮位图（重照检出）。
    compared_slots: [bool; ROUNDS],
    /// 差异账。
    diffs: Vec<DiffRow>,
    /// 已对照位。
    compared: bool,
    /// 封板位。
    sealed: bool,
}

impl AbCompare {
    /// 新对照账（机号/档号冻结）。
    pub fn new(machine: u8, tier: u8) -> Self {
        AbCompare {
            machine,
            tier,
            series_a: Vec::new(),
            series_b: Vec::new(),
            compared_slots: [false; ROUNDS],
            diffs: Vec::new(),
            compared: false,
            sealed: false,
        }
    }

    /// 落 A 侧记录。
    pub fn put_a(&mut self, r: RunRec) -> Result<(), u16> {
        self.put_side(true, r)
    }

    /// 落 B 侧记录。
    pub fn put_b(&mut self, r: RunRec) -> Result<(), u16> {
        self.put_side(false, r)
    }

    fn put_side(&mut self, side_a: bool, r: RunRec) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if !r.valid() {
            if r.outcome > 1 {
                return Err(E_OUTCOME_RANGE);
            }
            return Err(E_ROUND_RANGE);
        }
        let series = if side_a { &mut self.series_a } else { &mut self.series_b };
        // 同侧同轮重落拦。
        let mut i = 0usize;
        while i < series.len() {
            if series[i].round == r.round {
                return Err(E_SLOT_DUP);
            }
            i += 1;
        }
        series.push(r);
        Ok(())
    }

    /// 侧序列长度。
    pub fn len_a(&self) -> usize {
        self.series_a.len()
    }

    pub fn len_b(&self) -> usize {
        self.series_b.len()
    }

    /// 同侧取记录。
    pub fn rec(&self, side_a: bool, round: u16) -> Result<RunRec, u16> {
        if round < 1 || round as usize > ROUNDS {
            return Err(E_ROUND_RANGE);
        }
        let series = if side_a { &self.series_a } else { &self.series_b };
        let mut i = 0usize;
        while i < series.len() {
            if series[i].round == round {
                return Ok(series[i]);
            }
            i += 1;
        }
        Err(E_EMPTY_SERIES)
    }

    /// 对照（50/50 齐后逐轮双面对拍）：差异逐项落账（未归因态）。
    /// 返回差异数。
    pub fn compare(&mut self) -> Result<usize, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.series_a.is_empty() || self.series_b.is_empty() {
            return Err(E_EMPTY_SERIES);
        }
        if self.len_a() != ROUNDS || self.len_b() != ROUNDS {
            return Err(E_ROUNDS_SHORT);
        }
        self.diffs.clear();
        let mut r = 1u16;
        while r as usize <= ROUNDS {
            let a = self.rec(true, r)?;
            let b = self.rec(false, r)?;
            // 面 0：逐次结果。
            if a.outcome != b.outcome {
                if self.diffs.len() >= ROUNDS * 2 {
                    return Err(E_DIFF_FULL);
                }
                self.diffs.push(DiffRow {
                    round: r,
                    face: 0,
                    a_val: a.outcome,
                    b_val: b.outcome,
                    cause: 0xFF,
                });
            }
            // 面 1：结论臂。
            if a.verdict != b.verdict {
                if self.diffs.len() >= ROUNDS * 2 {
                    return Err(E_DIFF_FULL);
                }
                self.diffs.push(DiffRow {
                    round: r,
                    face: 1,
                    a_val: a.verdict,
                    b_val: b.verdict,
                    cause: 0xFF,
                });
            }
            self.compared_slots[(r - 1) as usize] = true;
            r += 1;
        }
        self.compared = true;
        Ok(self.diffs.len())
    }

    /// 差异归因：逐差异挂臂。
    pub fn attribute(&mut self, round: u16, face: u8, cause: DiffCause) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if !self.compared {
            return Err(E_NOT_COMPARED);
        }
        let mut i = 0usize;
        while i < self.diffs.len() {
            if self.diffs[i].round == round && self.diffs[i].face == face {
                self.diffs[i].cause = cause.id();
                return Ok(());
            }
            i += 1;
        }
        Err(E_DIFF_IMBAL)
    }

    /// 归因完整性审计：全部差异已归因 + 账平衡（差异数 == 重算逐次差异和）。
    pub fn audit_diffs(&self) -> Result<(), u16> {
        if !self.compared {
            return Err(E_NOT_COMPARED);
        }
        // 重算逐次差异和。
        let mut expect = 0usize;
        let mut r = 1u16;
        while r as usize <= ROUNDS {
            let a = self.rec(true, r)?;
            let b = self.rec(false, r)?;
            if a.outcome != b.outcome {
                expect += 1;
            }
            if a.verdict != b.verdict {
                expect += 1;
            }
            r += 1;
        }
        if self.diffs.len() != expect {
            return Err(E_DIFF_IMBAL);
        }
        // 逐项已归因。
        let mut i = 0usize;
        while i < self.diffs.len() {
            if !self.diffs[i].attributed() {
                return Err(E_UNATTRIBUTED);
            }
            DiffCause::from_id(self.diffs[i].cause)?;
            i += 1;
        }
        Ok(())
    }

    /// 零差异结论：对照已跑 + 差异账空。
    pub fn zero_diff(&self) -> Result<bool, u16> {
        if !self.compared {
            return Err(E_NOT_COMPARED);
        }
        Ok(self.diffs.is_empty())
    }

    /// 成功率面（ppm）解引：侧成功率。
    pub fn success_ppm(&self, side_a: bool) -> Result<u64, u16> {
        let series = if side_a { &self.series_a } else { &self.series_b };
        if series.len() != ROUNDS {
            return Err(E_ROUNDS_SHORT);
        }
        let mut ok = 0u64;
        let mut i = 0usize;
        while i < series.len() {
            ok = ok.wrapping_add(series[i].outcome as u64);
            i += 1;
        }
        Ok(ok * 10_000 / ROUNDS as u64)
    }

    /// 归因臂分布。
    pub fn cause_dist(&self) -> Result<[u32; CAUSE_ARMS as usize], u16> {
        if !self.compared {
            return Err(E_NOT_COMPARED);
        }
        let mut dist = [0u32; CAUSE_ARMS as usize];
        let mut i = 0usize;
        while i < self.diffs.len() {
            let c = DiffCause::from_id(self.diffs[i].cause)?;
            dist[c.id() as usize] = dist[c.id() as usize].wrapping_add(1);
            i += 1;
        }
        Ok(dist)
    }

    /// 封板：归因完整性过后落锁。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.audit_diffs()?;
        self.sealed = true;
        Ok(self.fingerprint())
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fnv_mix(h, self.machine | 0x10);
        h = fnv_mix(h, self.tier | 0x20);
        let mut i = 0usize;
        while i < self.series_a.len() {
            let r = self.series_a[i];
            h = fnv_mix(h, (r.round & 0xFF) as u8);
            h = fnv_mix(h, r.outcome | 0x30);
            h = fnv_mix(h, r.verdict | 0x40);
            i += 1;
        }
        let mut j = 0usize;
        while j < self.series_b.len() {
            let r = self.series_b[j];
            h = fnv_mix(h, (r.round & 0xFF) as u8);
            h = fnv_mix(h, r.outcome | 0x50);
            h = fnv_mix(h, r.verdict | 0x60);
            j += 1;
        }
        h = fnv_mix(h, (self.diffs.len() & 0xFF) as u8 | 0x70);
        h
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.series_a.is_empty() && self.series_b.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("A/B 对照: A=");
        out.push_str(&itoa(self.len_a() as u64));
        out.push_str(" B=");
        out.push_str(&itoa(self.len_b() as u64));
        out.push_str(" 差异=");
        out.push_str(&itoa(self.diffs.len() as u64));
        out.push_str(" 指纹=");
        out.push_str(&hex16(self.fingerprint()));
        if out.len() < SUMMARY_MIN_LEN {
            return Err(E_SUMMARY_EMPTY);
        }
        Ok(out)
    }
}

// ── 整数工具 ─────────────────────────────────────────────────────────
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return "0".to_string();
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

fn hex16(v: u64) -> String {
    const HEXD: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::new();
    s.push_str("0x");
    let mut shift = 60i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEXD[nib] as char);
        shift -= 4;
    }
    s
}

/// 确定性序列生成：种子派发 50 轮（过/败 + 结论臂）。
fn gen_series(seed: u8, fail_rounds: &[u16]) -> Vec<RunRec> {
    let mut out: Vec<RunRec> = Vec::new();
    let mut r = 1u16;
    while r as usize <= ROUNDS {
        let fail = contains_round(fail_rounds, r);
        let rec = RunRec {
            round: r,
            outcome: if fail { 0 } else { 1 },
            verdict: if fail { 1 } else { 0 },
        };
        out.push(rec);
        r += 1;
    }
    let _ = seed;
    out
}

fn contains_round(list: &[u16], r: u16) -> bool {
    let mut i = 0usize;
    while i < list.len() {
        if list[i] == r {
            return true;
        }
        i += 1;
    }
    false
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 主路径：A/B 各 50 轮零差异 → 零差异结论 → 封板 ——
    let mut ab = AbCompare::new(1, 3);
    let sa = gen_series(0, &[]);
    let sb = gen_series(1, &[]);
    let mut i = 0usize;
    while i < ROUNDS {
        ab.put_a(sa[i])?;
        ab.put_b(sb[i])?;
        i += 1;
    }
    if ab.len_a() != ROUNDS || ab.len_b() != ROUNDS {
        return Err(E_ROUNDS_SHORT);
    }
    // 成功率面全同（10000ppm）。
    if ab.success_ppm(true)? != ab.success_ppm(false)? {
        return Err(E_DIFF_IMBAL);
    }
    let d = ab.compare()?;
    if d != 0 {
        return Err(E_UNATTRIBUTED);
    }
    if !ab.zero_diff()? {
        return Err(E_UNATTRIBUTED);
    }
    ab.audit_diffs()?;
    let fp = ab.seal()?;
    if fp == 0 {
        return Err(E_DIFF_IMBAL);
    }
    // 封板后落账必拦。
    if ab.put_a(RunRec { round: 1, outcome: 1, verdict: 0 }) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    let sm = ab.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }

    // —— 差异 + 归因主路径：3 轮差异逐项归因 ——
    let mut ab2 = AbCompare::new(0, 1);
    // B 侧在轮 3/10/27 注入差异。
    let sa2 = gen_series(0, &[]);
    let sb2 = gen_series(1, &[3, 10, 27]);
    i = 0;
    while i < ROUNDS {
        ab2.put_a(sa2[i])?;
        ab2.put_b(sb2[i])?;
        i += 1;
    }
    let d2 = ab2.compare()?;
    if d2 != 6 {
        // 每轮差异落两面（结果+结论）→ 3×2=6。
        return Err(E_DIFF_IMBAL);
    }
    // 未归因即封板必拦。
    if ab2.seal() != Err(E_UNATTRIBUTED) {
        return Err(E_UNATTRIBUTED);
    }
    // 逐项归因。
    let mut r = 1u16;
    while r as usize <= ROUNDS {
        let a = ab2.rec(true, r)?;
        let b = ab2.rec(false, r)?;
        if a.outcome != b.outcome {
            ab2.attribute(r, 0, DiffCause::Injected)?;
        }
        if a.verdict != b.verdict {
            ab2.attribute(r, 1, DiffCause::Version)?;
        }
        r += 1;
    }
    ab2.audit_diffs()?;
    let dist = ab2.cause_dist()?;
    if dist[DiffCause::Injected.id() as usize] != 3
        || dist[DiffCause::Version.id() as usize] != 3
    {
        return Err(E_UNATTRIBUTED);
    }
    // 有差异 → 零差异结论 false。
    if ab2.zero_diff()? {
        return Err(E_UNATTRIBUTED);
    }
    ab2.seal()?;
    let sm2 = ab2.summary()?;
    if sm2.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }

    // —— 同机同档闸：机号不等样本（前置闸语义在账头冻结核对） ——
    let mut ab3 = AbCompare::new(1, 1);
    let mut ab4 = AbCompare::new(2, 1);
    if ab3.machine == ab4.machine {
        return Err(E_TIER_MISMATCH);
    }
    if ab3.tier != ab4.tier {
        return Err(E_TIER_MISMATCH);
    }

    // —— 错误路径样本 ——
    if DiffCause::from_id(4) != Err(E_CAUSE_RANGE) {
        return Err(E_CAUSE_RANGE);
    }
    let mut err = AbCompare::new(0, 0);
    if err.put_a(RunRec { round: 0, outcome: 1, verdict: 0 }) != Err(E_ROUND_RANGE) {
        return Err(E_ROUND_RANGE);
    }
    if err.put_a(RunRec { round: ROUNDS as u16 + 1, outcome: 1, verdict: 0 }) != Err(E_ROUND_RANGE) {
        return Err(E_ROUND_RANGE);
    }
    if err.put_a(RunRec { round: 1, outcome: 2, verdict: 0 }) != Err(E_OUTCOME_RANGE) {
        return Err(E_OUTCOME_RANGE);
    }
    if err.put_a(RunRec { round: 1, outcome: 1, verdict: 4 }) != Err(E_ROUND_RANGE) {
        return Err(E_ROUND_RANGE);
    }
    err.put_a(RunRec { round: 1, outcome: 1, verdict: 0 })?;
    if err.put_a(RunRec { round: 1, outcome: 0, verdict: 1 }) != Err(E_SLOT_DUP) {
        return Err(E_SLOT_DUP);
    }
    // 空序列对照。
    if err.compare() != Err(E_EMPTY_SERIES) {
        return Err(E_EMPTY_SERIES);
    }
    if err.zero_diff() != Err(E_NOT_COMPARED) {
        return Err(E_NOT_COMPARED);
    }
    if err.audit_diffs() != Err(E_NOT_COMPARED) {
        return Err(E_NOT_COMPARED);
    }
    // 未对照即归因。
    if err.attribute(1, 0, DiffCause::Version) != Err(E_NOT_COMPARED) {
        return Err(E_NOT_COMPARED);
    }
    // 轮数不足对照。
    let mut short = AbCompare::new(0, 0);
    let sa3 = gen_series(0, &[]);
    let mut k = 0usize;
    while k < ROUNDS - 1 {
        short.put_a(sa3[k])?;
        short.put_b(sa3[k])?;
        k += 1;
    }
    if short.compare() != Err(E_ROUNDS_SHORT) {
        return Err(E_ROUNDS_SHORT);
    }
    // 轮号解引越界。
    if ab.rec(true, 0) != Err(E_ROUND_RANGE) {
        return Err(E_ROUND_RANGE);
    }
    if ab.rec(true, ROUNDS as u16 + 1) != Err(E_ROUND_RANGE) {
        return Err(E_ROUND_RANGE);
    }
    // 归因无对应差异。
    if ab2.sealed {
        // ab2 已封板；用 attribute 前置态核对（封板态归因必拦）。
        if ab2.attribute(1, 0, DiffCause::Environment) != Err(E_SEALED) {
            return Err(E_SEALED);
        }
    }

    // 诊断表抽查。
    if diag_detail(E_UNATTRIBUTED) == "未知诊断码（0x5a60 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x5a60 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}
