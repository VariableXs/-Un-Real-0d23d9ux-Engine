//! UNX-F0730 · 升级回滚预案（失败回滚路径）· 诊断段 0x5ac0（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0730] 升级回滚预案（失败回滚路径） <SEG 0x5ac0>
//!   UNX-F0730-J1 回滚预案三步留痕齐（触发/回退/验证）且回滚演练过
//!   （注入失败 → 回退 → 基线复现）。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 回滚状态机闭集：Normal→Triggered→RolledBack→BaselineVerified，
//!      合法边仅此三边；跳步/回步/终态出边一律拦截。
//!   2. 三步留痕闭集：触发留痕（失败样本指纹）/回退留痕（版本回指）/
//!      验证留痕（基线复现指纹），三步全齐才算演练过。
//!   3. 基线复现：回退后基线指纹必须与升级前基线指纹全等（复现判据），
//!      不等即 E_BASELINE_DIFFER。
//!   4. 单向演练：一次预案演练只走一遍闭环，重复演练必拦
//!      （E_ALREADY_DRILLED）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x5ac0~0x5adF 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥570（判据原文「纯功能代码 ≥570 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x5ac0 独占 0x5ac0~0x5adF） ────────────────────────────
pub const SEG_BASE: u16 = 0x5ac0;

/// 0x5ac1 预案已封板仍尝试演练
const E_SEALED: u16 = SEG_BASE + 0x1;
/// 0x5ac2 状态机非法迁移（合法边闭集外）
const E_BAD_EDGE: u16 = SEG_BASE + 0x2;
/// 0x5ac3 失败样本缺失：无失败指纹即触发回滚
const E_NO_FAILURE: u16 = SEG_BASE + 0x3;
/// 0x5ac4 基线未封存：升级前基线指纹缺失
const E_NO_BASELINE: u16 = SEG_BASE + 0x4;
/// 0x5ac5 基线复现失败：回退后指纹与基线不等
const E_BASELINE_DIFFER: u16 = SEG_BASE + 0x6;
/// 0x5ac6 三步留痕缺：触发/回退/验证缺一
const E_STEP_MISSING: u16 = SEG_BASE + 0x7;
/// 0x5ac7 重复演练：预案已闭环
const E_ALREADY_DRILLED: u16 = SEG_BASE + 0x8;
/// 0x5ac8 版本串非法：空串或超长
const E_VER_BAD: u16 = SEG_BASE + 0x9;
/// 0x5ac9 留痕序错乱：三步序不符（回退先于触发等）
const E_STEP_ORDER: u16 = SEG_BASE + 0xA;
/// 0x5aaA 指纹零值
const E_FP_ZERO: u16 = SEG_BASE + 0xB;
/// 0x5aab 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xC;
/// 0x5aac 未演练即取演练结论
const E_NOT_DRILLED: u16 = SEG_BASE + 0xD;
/// 0x5aad 留痕账溢出
const E_TRACE_FULL: u16 = SEG_BASE + 0xE;
/// 0x5aae 留痕悬空：解引越界
const E_REF_DANGLE: u16 = SEG_BASE + 0xF;

/// 诊断码 → 中文可操作诊断（段 0x5ac0~0x5adF 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "预案已封板：演练冻结，重演请开新预案",
        E_BAD_EDGE => "状态机非法迁移：合法边仅三步闭环",
        E_NO_FAILURE => "失败样本缺失：先注入失败指纹再触发",
        E_NO_BASELINE => "基线未封存：先封存升级前基线指纹",
        E_BASELINE_DIFFER => "基线复现失败：回退后指纹与基线不等",
        E_STEP_MISSING => "三步留痕缺：触发/回退/验证缺一",
        E_ALREADY_DRILLED => "重复演练：预案已闭环，重演请开新预案",
        E_VER_BAD => "版本串非法：空串或超 32 字节",
        E_STEP_ORDER => "留痕序错乱：三步序不符",
        E_FP_ZERO => "指纹零值：非法",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_NOT_DRILLED => "未演练即取结论",
        E_TRACE_FULL => "留痕账溢出：超定容上限",
        E_REF_DANGLE => "留痕悬空：解引越界",
        _ => "未知诊断码（0x5ac0 段外或保留位）",
    }
}

// ── 常量（来源：回滚口径与定容） ─────────────────────────────────────
/// 版本串长度上限。
pub const VER_MAX: usize = 32;
/// 留痕账定容。
pub const MAX_TRACES: usize = 8;
/// 三步留痕数（判据原文：三步留痕齐）。
pub const STEP_COUNT: usize = 3;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(STEP_COUNT == 3);

// ── FNV-1a 64 ────────────────────────────────────────────────────────
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_mix(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

fn fnv_str(s: &str) -> u64 {
    let mut h = FNV_OFF;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        h = fnv_mix(h, b[i]);
        i += 1;
    }
    h
}

// ── 回滚状态机闭集 ───────────────────────────────────────────────────
/// 回滚状态机四态闭集。合法边：
///   Normal→Triggered（注入失败触发）/ Triggered→RolledBack（版本回退）/
///   RolledBack→BaselineVerified（基线复现）。终态 BaselineVerified 零出边。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RollbackState {
    /// 正常态（升级已上线）。
    Normal,
    /// 已触发回滚。
    Triggered,
    /// 已回退。
    RolledBack,
    /// 基线复现已验证（终态）。
    BaselineVerified,
}

impl RollbackState {
    /// 合法边闭集判定。
    pub const fn edge_legal(from: RollbackState, to: RollbackState) -> bool {
        matches!(
            (from, to),
            (RollbackState::Normal, RollbackState::Triggered)
                | (RollbackState::Triggered, RollbackState::RolledBack)
                | (RollbackState::RolledBack, RollbackState::BaselineVerified)
        )
    }
}

// ── 留痕步闭集 ───────────────────────────────────────────────────────
/// 三步留痕闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceStep {
    /// 步 0：触发（失败样本指纹）。
    Trigger,
    /// 步 1：回退（版本回指哈希）。
    Rollback,
    /// 步 2：验证（基线复现指纹）。
    Verify,
}

impl TraceStep {
    /// 步号。
    pub const fn id(self) -> usize {
        match self {
            TraceStep::Trigger => 0,
            TraceStep::Rollback => 1,
            TraceStep::Verify => 2,
        }
    }

    /// 按步号重建。
    pub fn from_id(v: u8) -> Result<TraceStep, u16> {
        match v {
            0 => Ok(TraceStep::Trigger),
            1 => Ok(TraceStep::Rollback),
            2 => Ok(TraceStep::Verify),
            _ => Err(E_STEP_MISSING),
        }
    }
}

// ── 留痕行 ───────────────────────────────────────────────────────────
/// 留痕行：步号 + 指纹 + 版本哈希（回退步有义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceRow {
    /// 步号。
    pub step: u8,
    /// 指纹。
    pub fp: u64,
    /// 版本哈希（回退步有义；其余为 0）。
    pub ver_fp: u64,
}

// ── 回滚预案 ─────────────────────────────────────────────────────────
/// 升级回滚预案：基线封存 + 状态机 + 三步留痕 + 演练闭环。
pub struct RollbackPlan {
    /// 预案名。
    pub plan_name: &'static str,
    /// 升级前基线指纹（封存后只读）。
    baseline_fp: Option<u64>,
    /// 状态机当前态。
    state: RollbackState,
    /// 留痕账。
    traces: Vec<TraceRow>,
    /// 失败样本指纹（触发前置）。
    failure_fp: Option<u64>,
    /// 演练闭环位。
    drilled: bool,
    /// 封板位。
    sealed: bool,
}

impl RollbackPlan {
    /// 新预案。
    pub fn new(name: &'static str) -> Self {
        RollbackPlan {
            plan_name: name,
            baseline_fp: None,
            state: RollbackState::Normal,
            traces: Vec::new(),
            failure_fp: None,
            drilled: false,
            sealed: false,
        }
    }

    /// 当前态。
    pub const fn state(&self) -> RollbackState {
        self.state
    }

    /// 封存升级前基线。
    pub fn seal_baseline(&mut self, fp: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if fp == 0 {
            return Err(E_FP_ZERO);
        }
        if self.baseline_fp.is_some() {
            return Err(E_ALREADY_DRILLED);
        }
        self.baseline_fp = Some(fp);
        Ok(())
    }

    /// 基线封存位。
    pub const fn has_baseline(&self) -> bool {
        self.baseline_fp.is_some()
    }

    /// 注入失败样本（触发前置）。
    pub fn inject_failure(&mut self, fp: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if fp == 0 {
            return Err(E_FP_ZERO);
        }
        if self.failure_fp.is_some() {
            return Err(E_ALREADY_DRILLED);
        }
        self.failure_fp = Some(fp);
        Ok(())
    }

    /// 触发回滚：Normal→Triggered + 触发留痕。
    pub fn trigger(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.drilled {
            return Err(E_ALREADY_DRILLED);
        }
        let fail_fp = match self.failure_fp {
            Some(f) => f,
            None => return Err(E_NO_FAILURE),
        };
        if !RollbackState::edge_legal(self.state, RollbackState::Triggered) {
            return Err(E_BAD_EDGE);
        }
        if self.traces.len() >= MAX_TRACES {
            return Err(E_TRACE_FULL);
        }
        // 留痕序：触发必为第一笔。
        if !self.traces.is_empty() {
            return Err(E_STEP_ORDER);
        }
        self.state = RollbackState::Triggered;
        self.traces.push(TraceRow {
            step: TraceStep::Trigger.id() as u8,
            fp: fail_fp,
            ver_fp: 0,
        });
        Ok(())
    }

    /// 版本回退：Triggered→RolledBack + 回退留痕。
    pub fn rollback_to(&mut self, version: &str) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if version.is_empty() || version.len() > VER_MAX {
            return Err(E_VER_BAD);
        }
        if !RollbackState::edge_legal(self.state, RollbackState::RolledBack) {
            return Err(E_BAD_EDGE);
        }
        if self.traces.len() >= MAX_TRACES {
            return Err(E_TRACE_FULL);
        }
        // 留痕序：回退必为第二笔（触发在前）。
        if self.traces.len() != 1 {
            return Err(E_STEP_ORDER);
        }
        self.state = RollbackState::RolledBack;
        self.traces.push(TraceRow {
            step: TraceStep::Rollback.id() as u8,
            fp: fnv_str(version),
            ver_fp: fnv_str(version),
        });
        Ok(())
    }

    /// 基线复现验证：RolledBack→BaselineVerified + 验证留痕。
    /// 复现判据：复现指纹必须与封存基线全等。
    pub fn verify_baseline(&mut self, reproduced_fp: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if reproduced_fp == 0 {
            return Err(E_FP_ZERO);
        }
        if !RollbackState::edge_legal(self.state, RollbackState::BaselineVerified) {
            return Err(E_BAD_EDGE);
        }
        let base = match self.baseline_fp {
            Some(b) => b,
            None => return Err(E_NO_BASELINE),
        };
        if reproduced_fp != base {
            return Err(E_BASELINE_DIFFER);
        }
        if self.traces.len() >= MAX_TRACES {
            return Err(E_TRACE_FULL);
        }
        // 留痕序：验证必为第三笔。
        if self.traces.len() != 2 {
            return Err(E_STEP_ORDER);
        }
        self.state = RollbackState::BaselineVerified;
        self.traces.push(TraceRow {
            step: TraceStep::Verify.id() as u8,
            fp: reproduced_fp,
            ver_fp: 0,
        });
        self.drilled = true;
        Ok(())
    }

    /// 三步留痕齐审计：步序 0/1/2 逐笔复列。
    pub fn audit_traces(&self) -> Result<(), u16> {
        if self.traces.len() != STEP_COUNT {
            return Err(E_STEP_MISSING);
        }
        let mut i = 0usize;
        while i < STEP_COUNT {
            if self.traces[i].step != i as u8 {
                return Err(E_STEP_ORDER);
            }
            if self.traces[i].fp == 0 {
                return Err(E_FP_ZERO);
            }
            i += 1;
        }
        // 回退步版本哈希必须非零。
        if self.traces[TraceStep::Rollback.id()].ver_fp == 0 {
            return Err(E_STEP_MISSING);
        }
        Ok(())
    }

    /// 演练结论：闭环 + 三步齐 + 基线复现全等。
    pub fn drill_verdict(&self) -> Result<bool, u16> {
        if !self.drilled {
            return Err(E_NOT_DRILLED);
        }
        self.audit_traces()?;
        if self.state != RollbackState::BaselineVerified {
            return Err(E_BAD_EDGE);
        }
        Ok(true)
    }

    /// 封板：演练闭环后落锁。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if !self.drilled {
            return Err(E_NOT_DRILLED);
        }
        self.audit_traces()?;
        self.sealed = true;
        Ok(self.fingerprint())
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        match self.baseline_fp {
            Some(b) => {
                let mut shift = 56i32;
                while shift >= 0 {
                    h = fnv_mix(h, ((b >> shift) & 0xFF) as u8);
                    shift -= 8;
                }
            }
            None => h = fnv_mix(h, 0x7F),
        }
        let mut i = 0usize;
        while i < self.traces.len() {
            let t = self.traces[i];
            h = fnv_mix(h, t.step | 0x10);
            let mut shift = 56i32;
            while shift >= 0 {
                h = fnv_mix(h, ((t.fp >> shift) & 0xFF) as u8);
                shift -= 8;
            }
            i += 1;
        }
        h = fnv_mix(h, self.drilled as u8 | 0x20);
        h
    }

    /// 留痕解引。
    pub fn trace_at(&self, idx: usize) -> Result<TraceRow, u16> {
        if idx >= self.traces.len() {
            return Err(E_REF_DANGLE);
        }
        Ok(self.traces[idx])
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.traces.is_empty() && self.baseline_fp.is_none() {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("回滚预案: ");
        out.push_str(self.plan_name);
        out.push_str(" 留痕=");
        out.push_str(&itoa(self.traces.len() as u64));
        out.push_str("/3 闭环=");
        out.push_str(&itoa(self.drilled as u64));
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

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 主路径：封基线 → 注失败 → 触发 → 回退 → 复现 → 闭环封板 ——
    let mut plan = RollbackPlan::new("rb_limine_upgrade");
    if plan.plan_name != "rb_limine_upgrade" {
        return Err(SEG_BASE + 0x10);
    }
    plan.seal_baseline(0xABCD_0001)?;
    if !plan.has_baseline() {
        return Err(E_NO_BASELINE);
    }
    // 重复封基线必拦。
    if plan.seal_baseline(0xABCD_0002) != Err(E_ALREADY_DRILLED) {
        return Err(E_ALREADY_DRILLED);
    }
    plan.inject_failure(0xFEED_0001)?;
    // 无失败即触发必拦（未注失败前）——已注，先验证状态机。
    plan.trigger()?;
    if plan.state() != RollbackState::Triggered {
        return Err(E_BAD_EDGE);
    }
    plan.rollback_to("v1.2.2")?;
    if plan.state() != RollbackState::RolledBack {
        return Err(E_BAD_EDGE);
    }
    // 基线复现：指纹必须与封存基线全等。
    if plan.verify_baseline(0xDEAD_0001) != Err(E_BASELINE_DIFFER) {
        return Err(E_BASELINE_DIFFER);
    }
    plan.verify_baseline(0xABCD_0001)?;
    if plan.state() != RollbackState::BaselineVerified {
        return Err(E_BAD_EDGE);
    }
    if !plan.drill_verdict()? {
        return Err(E_BASELINE_DIFFER);
    }
    let fp = plan.seal()?;
    if fp == 0 {
        return Err(E_STEP_MISSING);
    }
    // 封板后演练必拦。
    if plan.trigger() != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    let sm = plan.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }
    // 三步留痕解引。
    if plan.trace_at(0)?.step != 0 {
        return Err(E_STEP_ORDER);
    }
    if plan.trace_at(1)?.ver_fp == 0 {
        return Err(E_STEP_MISSING);
    }
    if plan.trace_at(2)?.fp != 0xABCD_0001 {
        return Err(E_BASELINE_DIFFER);
    }
    if plan.trace_at(3) != Err(E_REF_DANGLE) {
        return Err(E_REF_DANGLE);
    }

    // —— 幂等：同脚本重建预案，指纹全等 ——
    let mut plan2 = RollbackPlan::new("rb_limine_upgrade");
    plan2.seal_baseline(0xABCD_0001)?;
    plan2.inject_failure(0xFEED_0001)?;
    plan2.trigger()?;
    plan2.rollback_to("v1.2.2")?;
    plan2.verify_baseline(0xABCD_0001)?;
    if plan2.fingerprint() != fp {
        return Err(E_BASELINE_DIFFER);
    }

    // —— 跳步拦截：未触发即回退 / 未回退即验证 ——
    let mut skip = RollbackPlan::new("rb_skip");
    skip.seal_baseline(0x11)?;
    skip.inject_failure(0x22)?;
    if skip.rollback_to("v0") != Err(E_BAD_EDGE) {
        return Err(E_BAD_EDGE);
    }
    if skip.verify_baseline(0x11) != Err(E_BAD_EDGE) {
        return Err(E_BAD_EDGE);
    }
    // 无失败即触发必拦。
    let mut nofail = RollbackPlan::new("rb_nofail");
    nofail.seal_baseline(0x33)?;
    if nofail.trigger() != Err(E_NO_FAILURE) {
        return Err(E_NO_FAILURE);
    }
    // 无基线即验证必拦。
    let mut nobase = RollbackPlan::new("rb_nobase");
    nobase.inject_failure(0x44)?;
    nobase.trigger()?;
    nobase.rollback_to("v1")?;
    if nobase.verify_baseline(0x55) != Err(E_NO_BASELINE) {
        return Err(E_NO_BASELINE);
    }

    // —— 留痕序错乱拦截：双触发 ——
    let mut dup = RollbackPlan::new("rb_dup");
    dup.seal_baseline(0x66)?;
    dup.inject_failure(0x77)?;
    dup.trigger()?;
    dup.inject_failure(0x88)?;
    if dup.trigger() != Err(E_BAD_EDGE) {
        return Err(E_BAD_EDGE);
    }

    // —— 错误路径样本 ——
    if RollbackPlan::new("p").state() != RollbackState::Normal {
        return Err(E_BAD_EDGE);
    }
    let mut err = RollbackPlan::new("rb_err");
    if err.seal_baseline(0) != Err(E_FP_ZERO) {
        return Err(E_FP_ZERO);
    }
    if err.inject_failure(0) != Err(E_FP_ZERO) {
        return Err(E_FP_ZERO);
    }
    if err.rollback_to("") != Err(E_VER_BAD) {
        return Err(E_VER_BAD);
    }
    let long = [b'v'; VER_MAX + 1];
    let long_ver = core::str::from_utf8(&long);
    if let Ok(s) = long_ver {
        if err.rollback_to(s) != Err(E_VER_BAD) {
            return Err(E_VER_BAD);
        }
    }
    // 留痕序：回退前触发缺失（无失败直接触发已被拦，再验状态序）。
    err.seal_baseline(0x99)?;
    err.inject_failure(0xAA)?;
    err.trigger()?;
    // 未回退即验证。
    if err.verify_baseline(0x99) != Err(E_BAD_EDGE) {
        return Err(E_BAD_EDGE);
    }
    // 未演练即结论。
    if err.drill_verdict() != Err(E_NOT_DRILLED) {
        return Err(E_NOT_DRILLED);
    }
    if err.seal() != Err(E_NOT_DRILLED) {
        return Err(E_NOT_DRILLED);
    }
    // 三步缺：只有一步留痕。
    if err.audit_traces() != Err(E_STEP_MISSING) {
        return Err(E_STEP_MISSING);
    }
    // 留痕解引越界。
    if err.trace_at(9) != Err(E_REF_DANGLE) {
        return Err(E_REF_DANGLE);
    }
    // 摘要基线在但零留痕——允许（有 baseline）。
    let sm2 = err.summary()?;
    if sm2.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }

    // 诊断表抽查。
    if diag_detail(E_BASELINE_DIFFER) == "未知诊断码（0x5ac0 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x5ac0 段外或保留位）" {
        return Err(SEG_BASE + 0x12);
    }

    Ok(0)
}
