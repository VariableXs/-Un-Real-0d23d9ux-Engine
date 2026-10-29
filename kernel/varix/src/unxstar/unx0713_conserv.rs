//! UNX-F0713 · 断言计数守恒（注册=执行=通过+失败）· 诊断段 0x5920（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0713] 断言计数守恒（注册=执行=通过+失败） <SEG 0x5920>
//!   UNX-F0713-J1 三级守恒（注册数=执行数、执行数=通过+失败+SKIP、
//!   分项和=总数）全平误差 0，吞断言注入检出。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 三级守恒闭集：一级（注册数 == 执行数）/ 二级（执行数 == 通过+
//!      失败+SKIP）/ 三级（Σ分项计数 == 总数），级外无守恒式；全平
//!      误差 0（整数计数，无灰区）。
//!   2. 吞断言注入检出：登记后未执行（吞）→ 一级破；执行后未计数
//!      （吞结果）→ 二级破；分项漏计 → 三级破——三种吞法各设独立
//!      检出通道，注入必中。
//!   3. 分项账闭集：分项按批号聚合（批 1..=MAX_BATCHES），分项和守恒
//!      以批为粒度核对。
//!   4. 计数单调：计数器只增不减，负向操作（清账）走显性重置通道并
//!      留痕（重置事件账）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x5920~0x593F 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥570（判据原文「纯功能代码 ≥570 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x5920 独占 0x5920~0x593F） ────────────────────────────
pub const SEG_BASE: u16 = 0x5920;

/// 0x5921 账已封板仍尝试登记
const E_SEALED: u16 = SEG_BASE + 0x1;
/// 0x5922 一级守恒破：注册数 ≠ 执行数（吞断言）
const E_L1_SWALLOW: u16 = SEG_BASE + 0x2;
/// 0x5923 二级守恒破：执行数 ≠ 通过+失败+SKIP（吞结果）
const E_L2_SWALLOW: u16 = SEG_BASE + 0x3;
/// 0x5924 三级守恒破：Σ分项计数 ≠ 总数（分项漏计）
const E_L3_SWALLOW: u16 = SEG_BASE + 0x4;
/// 0x5925 批号越域（合法 1..=MAX_BATCHES）
const E_BATCH_RANGE: u16 = SEG_BASE + 0x5;
/// 0x5926 结果态越闭集
const E_OUTCOME_RANGE: u16 = SEG_BASE + 0x6;
/// 0x5927 未执行即守恒判定
const E_NOT_RUN: u16 = SEG_BASE + 0x7;
/// 0x5928 断言重号：同批同名二次登记
const E_ASSERT_DUP: u16 = SEG_BASE + 0x8;
/// 0x5929 断言名非法：空名或超长
const E_NAME_BAD: u16 = SEG_BASE + 0x9;
/// 0x592A 执行越位：执行未登记断言
const E_EXEC_GHOST: u16 = SEG_BASE + 0xA;
/// 0x592B 账溢出：登记数超定容
const E_LEDGER_FULL: u16 = SEG_BASE + 0xB;
/// 0x592C 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xC;
/// 0x592D 重置未留痕：清账无重置事件
const E_RESET_NOLOG: u16 = SEG_BASE + 0xD;
/// 0x592E 条目悬空：解引越界
const E_REF_DANGLE: u16 = SEG_BASE + 0xE;

/// 诊断码 → 中文可操作诊断（段 0x5920~0x593F 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "账已封板：计数冻结，重跑请开新账",
        E_L1_SWALLOW => "一级守恒破：注册数 ≠ 执行数，存在吞断言",
        E_L2_SWALLOW => "二级守恒破：执行数 ≠ 通过+失败+SKIP，结果被吞",
        E_L3_SWALLOW => "三级守恒破：Σ分项计数 ≠ 总数，分项漏计",
        E_BATCH_RANGE => "批号越域：合法 1..=MAX_BATCHES",
        E_OUTCOME_RANGE => "结果态越闭集：合法 Pass/Fail/Skip",
        E_NOT_RUN => "未执行即守恒判定：先执行全量再判",
        E_ASSERT_DUP => "断言重号：同批同名二次登记拒收",
        E_NAME_BAD => "断言名非法：空名或超 64 字节",
        E_EXEC_GHOST => "执行越位：执行了未登记断言",
        E_LEDGER_FULL => "账溢出：登记数超定容上限",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_RESET_NOLOG => "重置未留痕：清账必须写重置事件",
        E_REF_DANGLE => "条目悬空：解引越界",
        _ => "未知诊断码（0x5920 段外或保留位）",
    }
}

// ── 常量（来源：守恒口径与定容） ─────────────────────────────────────
/// 批数上限（分项聚合粒度）。
pub const MAX_BATCHES: usize = 16;
/// 登记定容。
pub const MAX_ASSERTS: usize = 256;
/// 名长度上限。
pub const NAME_MAX: usize = 64;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(MAX_BATCHES == 16);

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

// ── 结果态闭集 ───────────────────────────────────────────────────────
/// 断言结果三态闭集（二级守恒三项）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// 通过。
    Pass,
    /// 失败。
    Fail,
    /// 跳过（显性声明）。
    Skip,
}

impl Outcome {
    /// 态号。
    pub const fn id(self) -> u8 {
        match self {
            Outcome::Pass => 0,
            Outcome::Fail => 1,
            Outcome::Skip => 2,
        }
    }
}

// ── 断言条目 ─────────────────────────────────────────────────────────
/// 断言条目：名哈希 + 批号 + 执行态（None=未执行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsRow {
    /// 名 FNV 哈希。
    pub name_fp: u64,
    /// 批号 1..=MAX_BATCHES。
    pub batch: u16,
    /// 执行态（None=未执行）。
    pub outcome: Option<Outcome>,
}

// ── 重置事件 ─────────────────────────────────────────────────────────
/// 重置事件（清账留痕）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResetEvent {
    /// 重置序号。
    pub seq: u32,
    /// 重置时快照注册数。
    pub registered_at: u32,
}

// ── 守恒账 ───────────────────────────────────────────────────────────
/// 断言计数守恒账：注册/执行/三态计数 + 三级守恒审计 + 吞检出。
pub struct ConservationLedger {
    /// 断言账。
    rows: Vec<ConsRow>,
    /// 分项计数（按批：通过/失败/跳过三列）。
    per_batch: [[u32; 3]; MAX_BATCHES],
    /// 总注册数（== rows.len()）。
    registered: u32,
    /// 总执行数。
    executed: u32,
    /// 总三态计数。
    pass_total: u32,
    fail_total: u32,
    skip_total: u32,
    /// 封板位。
    sealed: bool,
    /// 重置事件账。
    resets: Vec<ResetEvent>,
}

impl ConservationLedger {
    /// 新账。
    pub fn new() -> Self {
        ConservationLedger {
            rows: Vec::new(),
            per_batch: [[0; 3]; MAX_BATCHES],
            registered: 0,
            executed: 0,
            pass_total: 0,
            fail_total: 0,
            skip_total: 0,
            sealed: false,
            resets: Vec::new(),
        }
    }

    /// 登记断言：名去重 + 批号域。
    pub fn enroll(&mut self, name: &str, batch: u16) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.rows.len() >= MAX_ASSERTS {
            return Err(E_LEDGER_FULL);
        }
        if name.is_empty() || name.len() > NAME_MAX {
            return Err(E_NAME_BAD);
        }
        if batch < 1 || batch as usize > MAX_BATCHES {
            return Err(E_BATCH_RANGE);
        }
        let fp = fnv_str(name);
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].name_fp == fp {
                return Err(E_ASSERT_DUP);
            }
            i += 1;
        }
        self.rows.push(ConsRow {
            name_fp: fp,
            batch,
            outcome: None,
        });
        self.registered = self.registered.wrapping_add(1);
        Ok(self.registered)
    }

    /// 执行断言：按登记序逐条给结果（越位执行必拦）。
    pub fn execute(&mut self, idx: usize, oc: Outcome) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if idx >= self.rows.len() {
            return Err(E_EXEC_GHOST);
        }
        if self.rows[idx].outcome.is_some() {
            return Err(E_OUTCOME_RANGE);
        }
        self.rows[idx].outcome = Some(oc);
        self.executed = self.executed.wrapping_add(1);
        let b = (self.rows[idx].batch - 1) as usize;
        match oc {
            Outcome::Pass => {
                self.pass_total = self.pass_total.wrapping_add(1);
                self.per_batch[b][0] = self.per_batch[b][0].wrapping_add(1);
            }
            Outcome::Fail => {
                self.fail_total = self.fail_total.wrapping_add(1);
                self.per_batch[b][1] = self.per_batch[b][1].wrapping_add(1);
            }
            Outcome::Skip => {
                self.skip_total = self.skip_total.wrapping_add(1);
                self.per_batch[b][2] = self.per_batch[b][2].wrapping_add(1);
            }
        }
        Ok(())
    }

    /// 注册数。
    pub const fn registered(&self) -> u32 {
        self.registered
    }

    /// 执行数。
    pub const fn executed(&self) -> u32 {
        self.executed
    }

    /// 三态计数快照。
    pub const fn totals(&self) -> (u32, u32, u32) {
        (self.pass_total, self.fail_total, self.skip_total)
    }

    /// 一级守恒：注册数 == 执行数。
    pub fn audit_l1(&self) -> Result<(), u16> {
        if self.registered != self.executed {
            return Err(E_L1_SWALLOW);
        }
        Ok(())
    }

    /// 二级守恒：执行数 == 通过+失败+SKIP。
    pub fn audit_l2(&self) -> Result<(), u16> {
        let sum = self
            .pass_total
            .wrapping_add(self.fail_total)
            .wrapping_add(self.skip_total);
        if sum != self.executed {
            return Err(E_L2_SWALLOW);
        }
        Ok(())
    }

    /// 三级守恒：Σ分项计数 == 总数（批粒度）。
    pub fn audit_l3(&self) -> Result<(), u16> {
        let mut sum: u32 = 0;
        let mut b = 0usize;
        while b < MAX_BATCHES {
            sum = sum
                .wrapping_add(self.per_batch[b][0])
                .wrapping_add(self.per_batch[b][1])
                .wrapping_add(self.per_batch[b][2]);
            b += 1;
        }
        if sum != self.executed {
            return Err(E_L3_SWALLOW);
        }
        Ok(())
    }

    /// 全量执行：登记断言全数执行（确定性结果派发：全 Pass）。
    /// 守恒判定前置：三级全审。吞断言（漏执行）注入由显性通道模拟。
    pub fn run_all_pass(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].outcome.is_none() {
                self.execute(i, Outcome::Pass)?;
            }
            i += 1;
        }
        self.audit_l1()?;
        self.audit_l2()?;
        self.audit_l3()?;
        Ok(())
    }

    /// 三级全审入口（未执行即拒）。
    pub fn audit_all(&self) -> Result<(), u16> {
        if self.executed == 0 {
            return Err(E_NOT_RUN);
        }
        self.audit_l1()?;
        self.audit_l2()?;
        self.audit_l3()?;
        Ok(())
    }

    /// 封板：三级全审过后落锁。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.audit_all()?;
        self.sealed = true;
        Ok(self.fingerprint())
    }

    /// 显性重置（清账留痕）：写重置事件后清零计数与账。
    pub fn reset(&mut self) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        let seq = self.resets.len() as u32 + 1;
        self.resets.push(ResetEvent {
            seq,
            registered_at: self.registered,
        });
        self.rows.clear();
        self.registered = 0;
        self.executed = 0;
        self.pass_total = 0;
        self.fail_total = 0;
        self.skip_total = 0;
        let mut b = 0usize;
        while b < MAX_BATCHES {
            self.per_batch[b] = [0; 3];
            b += 1;
        }
        // 重置后零账三级守恒天然平（零 == 零），显性核对。
        self.audit_l2()?;
        Ok(seq)
    }

    /// 重置事件数。
    pub fn reset_count(&self) -> usize {
        self.resets.len()
    }

    /// 分项计数解引。
    pub fn batch_counts(&self, batch: u16) -> Result<[u32; 3], u16> {
        if batch < 1 || batch as usize > MAX_BATCHES {
            return Err(E_BATCH_RANGE);
        }
        Ok(self.per_batch[(batch - 1) as usize])
    }

    /// 条目解引。
    pub fn row(&self, idx: usize) -> Result<ConsRow, u16> {
        if idx >= self.rows.len() {
            return Err(E_REF_DANGLE);
        }
        Ok(self.rows[idx])
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fnv_mix(h, (self.registered & 0xFF) as u8);
        h = fnv_mix(h, (self.executed & 0xFF) as u8);
        h = fnv_mix(h, (self.pass_total & 0xFF) as u8);
        h = fnv_mix(h, (self.fail_total & 0xFF) as u8);
        h = fnv_mix(h, (self.skip_total & 0xFF) as u8);
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = self.rows[i];
            let mut shift = 56i32;
            while shift >= 0 {
                h = fnv_mix(h, ((r.name_fp >> shift) & 0xFF) as u8);
                shift -= 8;
            }
            h = fnv_mix(h, (r.batch & 0xFF) as u8 | 0x10);
            match r.outcome {
                Some(oc) => h = fnv_mix(h, oc.id() | 0x20),
                None => h = fnv_mix(h, 0x7F),
            }
            i += 1;
        }
        h
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.rows.is_empty() && self.resets.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("计数守恒账: 注册=");
        out.push_str(&itoa(self.registered as u64));
        out.push_str(" 执行=");
        out.push_str(&itoa(self.executed as u64));
        out.push_str(" 过=");
        out.push_str(&itoa(self.pass_total as u64));
        out.push_str(" 败=");
        out.push_str(&itoa(self.fail_total as u64));
        out.push_str(" 跳=");
        out.push_str(&itoa(self.skip_total as u64));
        out.push_str(" 重置=");
        out.push_str(&itoa(self.resets.len() as u64));
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

fn cname(i: usize) -> String {
    let mut s = String::new();
    s.push_str("cons_");
    s.push_str(&itoa(i as u64));
    s
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 主路径：3 批 × 8 条登记 → 全执行（混三态） → 三级全审 → 封板 ——
    let mut led = ConservationLedger::new();
    let mut i = 0usize;
    while i < 24 {
        led.enroll(&cname(i), ((i / 8) as u16) + 1)?;
        i += 1;
    }
    // 混三态执行：8 过 / 8 败 / 8 跳（按序轮转）。
    let mut k = 0usize;
    while k < 24 {
        let oc = match k % 3 {
            0 => Outcome::Pass,
            1 => Outcome::Fail,
            _ => Outcome::Skip,
        };
        led.execute(k, oc)?;
        k += 1;
    }
    led.audit_all()?;
    let fp = led.seal()?;
    if fp == 0 {
        return Err(E_NOT_RUN);
    }
    // 封板后操作必拦。
    if led.enroll("late", 1) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    if led.execute(0, Outcome::Pass) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    let (p, f, s) = led.totals();
    if p != 8 || f != 8 || s != 8 {
        return Err(E_L2_SWALLOW);
    }
    // 分项解引：批 1 = [3,3,2]（前 8 条中按序轮转）。
    let b1 = led.batch_counts(1)?;
    if b1 != [3, 3, 2] {
        return Err(E_L3_SWALLOW);
    }
    let sm = led.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }

    // —— run_all_pass 主路径：全过执行 + 自动三级审计 ——
    let mut led2 = ConservationLedger::new();
    let mut j = 0usize;
    while j < 16 {
        led2.enroll(&cname(100 + j), ((j / 8) as u16) + 1)?;
        j += 1;
    }
    led2.run_all_pass()?;
    let (p2, f2, s2) = led2.totals();
    if p2 != 16 || f2 != 0 || s2 != 0 {
        return Err(E_L2_SWALLOW);
    }

    // —— 吞断言注入（一级破）：登记 4 条只执行 3 条 ——
    let mut sw1 = ConservationLedger::new();
    let mut m = 0usize;
    while m < 4 {
        sw1.enroll(&cname(200 + m), 1)?;
        m += 1;
    }
    let mut n = 0usize;
    while n < 3 {
        sw1.execute(n, Outcome::Pass)?;
        n += 1;
    }
    if sw1.audit_l1() != Err(E_L1_SWALLOW) {
        return Err(E_L1_SWALLOW);
    }
    if sw1.audit_all() != Err(E_L1_SWALLOW) {
        return Err(E_L1_SWALLOW);
    }

    // —— 吞结果注入（二级破）：计数账拉爆 ——
    let mut sw2 = ConservationLedger::new();
    sw2.enroll("solo", 1)?;
    sw2.execute(0, Outcome::Pass)?;
    sw2.executed = sw2.executed.wrapping_add(1);
    if sw2.audit_l2() != Err(E_L2_SWALLOW) {
        return Err(E_L2_SWALLOW);
    }

    // —— 分项漏计注入（三级破）：分项账削一笔 ——
    let mut sw3 = ConservationLedger::new();
    let mut q = 0usize;
    while q < 4 {
        sw3.enroll(&cname(300 + q), 2)?;
        q += 1;
    }
    let mut r = 0usize;
    while r < 4 {
        sw3.execute(r, Outcome::Pass)?;
        r += 1;
    }
    sw3.per_batch[1][0] = sw3.per_batch[1][0].wrapping_sub(1);
    if sw3.audit_l3() != Err(E_L3_SWALLOW) {
        return Err(E_L3_SWALLOW);
    }

    // —— 显性重置：留痕 + 清零 → 重置后三级平 ——
    let mut rz = ConservationLedger::new();
    let mut t = 0usize;
    while t < 8 {
        rz.enroll(&cname(400 + t), 3)?;
        t += 1;
    }
    rz.run_all_pass()?;
    let seq = rz.reset()?;
    if seq != 1 {
        return Err(E_RESET_NOLOG);
    }
    if rz.reset_count() != 1 {
        return Err(E_RESET_NOLOG);
    }
    if rz.registered() != 0 || rz.executed() != 0 {
        return Err(E_RESET_NOLOG);
    }
    rz.audit_l2()?;

    // —— 错误路径样本 ——
    if led3_dummy() != Err(E_BATCH_RANGE) {
        return Err(E_BATCH_RANGE);
    }
    let mut err = ConservationLedger::new();
    if err.enroll("", 1) != Err(E_NAME_BAD) {
        return Err(E_NAME_BAD);
    }
    if err.enroll("x", 0) != Err(E_BATCH_RANGE) {
        return Err(E_BATCH_RANGE);
    }
    if err.enroll("x", MAX_BATCHES as u16 + 1) != Err(E_BATCH_RANGE) {
        return Err(E_BATCH_RANGE);
    }
    err.enroll("dup", 1)?;
    if err.enroll("dup", 2) != Err(E_ASSERT_DUP) {
        return Err(E_ASSERT_DUP);
    }
    // 未执行即守恒判定。
    if err.audit_all() != Err(E_NOT_RUN) {
        return Err(E_NOT_RUN);
    }
    if err.seal() != Err(E_NOT_RUN) {
        return Err(E_NOT_RUN);
    }
    // 幽灵执行。
    if err.execute(0, Outcome::Pass) != Err(E_OUTCOME_RANGE) {
        // 0 号已注册未执行——本应成功；误拦说明逻辑破。
        return Err(E_EXEC_GHOST);
    }
    if err.execute(0, Outcome::Pass) != Err(E_OUTCOME_RANGE) {
        // 同条二执必拦。
        return Err(E_OUTCOME_RANGE);
    }
    if err.execute(9, Outcome::Pass) != Err(E_EXEC_GHOST) {
        return Err(E_EXEC_GHOST);
    }
    // 分项/条目悬空解引。
    if err.batch_counts(0) != Err(E_BATCH_RANGE) {
        return Err(E_BATCH_RANGE);
    }
    if err.batch_counts(MAX_BATCHES as u16 + 1) != Err(E_BATCH_RANGE) {
        return Err(E_BATCH_RANGE);
    }
    if err.row(99) != Err(E_REF_DANGLE) {
        return Err(E_REF_DANGLE);
    }
    // 摘要空账拦。
    let empty = ConservationLedger::new();
    if empty.summary() != Err(E_SUMMARY_EMPTY) {
        return Err(E_SUMMARY_EMPTY);
    }

    // 诊断表抽查。
    if diag_detail(E_L1_SWALLOW) == "未知诊断码（0x5920 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x5920 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}

// 占位校验函数（批号越域路径样本）。
fn led3_dummy() -> Result<(), u16> {
    let led = ConservationLedger::new();
    led.batch_counts(99).map(|_| ())
}
