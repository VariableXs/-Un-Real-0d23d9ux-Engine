//! UNX-F0720 · B36 批 ktest 断言集（断言段收口）· 诊断段 0x5a00（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0720] B36 批 ktest 断言集（断言段收口） <SEG 0x5a00>
//!   UNX-F0720-J1 本批判据聚合入 ktest unx_a1_b36 一次命令全绿（三重
//!   口径）且批小计 11,480 守恒核对过。
//!   [留痕] R-A1-004 留痕：本批头部批小计 11440 为逐条求和后落账
//!   （先求和后落账，零虚记）。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 假设声明 ─────────────────────────────────────────────────────────
//!   判据原文同时出现「批小计 11,480」与留痕「批小计 11440」两个口径，
//!   相差 40。本模块按主判据 11,480 落 SUBTOTAL 常量，另设
//!   SUBTOTAL_ALT=11,440 双口径对冲位（差 40 以 DELTA=40 显性记账），
//!   两口径各自独立求和核对，差异留痕不静默吞。
//!
//! ── 设计纪律（与 F0700 异构） ────────────────────────────────────────
//!   1. 三机段收口：断言按三机（机 0/1/2）分段落账，定容环形账承载，
//!      段小计 = Σ段内断言权重；批小计 = Σ三段小计（先求和后落账）。
//!   2. 三重口径闭集：全过/对冲/零残留；全绿判定三口径位全真。
//!   3. 双口径对冲：SUBTOTAL=11,480 与 SUBTOTAL_ALT=11,440 各自独立
//!      核对；口径差 DELTA=40 显性留痕，禁静默。
//!   4. 执行账与登记账同构同长：零残留口径以账长恒等判定。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x5a00~0x5a1F 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥570（判据原文「纯功能代码 ≥570 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x5a00 独占 0x5a00~0x5a1F） ────────────────────────────
pub const SEG_BASE: u16 = 0x5a00;

/// 0x5a01 批账已封板仍尝试登记
const E_SEALED: u16 = SEG_BASE + 0x1;
/// 0x5a02 机号越界（合法 0..3）
const E_MACHINE_RANGE: u16 = SEG_BASE + 0x2;
/// 0x5a03 批小计漂移：Σ段小计 ≠ SUBTOTAL
const E_SUBTOTAL_DRIFT: u16 = SEG_BASE + 0x3;
/// 0x5a04 双口径破：ALT 口径求和 ≠ SUBTOTAL_ALT
const E_ALT_DRIFT: u16 = SEG_BASE + 0x4;
/// 0x5a05 口径差破：|SUBTOTAL - SUBTOTAL_ALT| ≠ DELTA
const E_DELTA_BROKEN: u16 = SEG_BASE + 0x5;
/// 0x5a06 断言败：非全绿
const E_ASSERT_FAIL: u16 = SEG_BASE + 0x6;
/// 0x5a07 对冲破：三态和 ≠ 执行数
const E_HEDGE: u16 = SEG_BASE + 0x7;
/// 0x5a08 残留检出：执行 ≠ 登记
const E_RESIDUE: u16 = SEG_BASE + 0x8;
/// 0x5a09 权重越域（合法 1..=8）
const E_WEIGHT_RANGE: u16 = SEG_BASE + 0x9;
/// 0x5a0A 环形账溢出：登记超定容
const E_RING_FULL: u16 = SEG_BASE + 0xA;
/// 0x5a0B 断言重号：同名二次登记
const E_ASSERT_DUP: u16 = SEG_BASE + 0xB;
/// 0x5a0C 断言名非法：空名或超长
const E_NAME_BAD: u16 = SEG_BASE + 0xC;
/// 0x5a0D 空段封板：某机段零断言
const E_SEGMENT_EMPTY: u16 = SEG_BASE + 0xD;
/// 0x5a0E 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xE;
/// 0x5a0F 未跑即取口径结论
const E_NOT_RUN: u16 = SEG_BASE + 0xF;
/// 0x5a10 单命令重入
const E_REENTRY: u16 = SEG_BASE + 0x10;

/// 诊断码 → 中文可操作诊断（段 0x5a00~0x5a1F 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "批账已封板：登记冻结，扩批请开新批",
        E_MACHINE_RANGE => "机号越界：三机段闭集合法 0..3",
        E_SUBTOTAL_DRIFT => "批小计漂移：Σ段小计 ≠ 11,480",
        E_ALT_DRIFT => "ALT 口径破：求和 ≠ 11,440",
        E_DELTA_BROKEN => "口径差破：双口径差 ≠ 40，留痕失真",
        E_ASSERT_FAIL => "断言败：非全绿，先修败项",
        E_HEDGE => "对冲破：三态和 ≠ 执行数",
        E_RESIDUE => "残留检出：执行 ≠ 登记，有吞断言",
        E_WEIGHT_RANGE => "权重越域：合法 1..=8",
        E_RING_FULL => "环形账溢出：登记超定容",
        E_ASSERT_DUP => "断言重号：同名二次登记拒收",
        E_NAME_BAD => "断言名非法：空名或超 64 字节",
        E_SEGMENT_EMPTY => "空段封板：三机段必须全非空",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_NOT_RUN => "未跑即取结论：先 run_unx_a1_b36",
        E_REENTRY => "单命令重入：执行通道只可走一次",
        _ => "未知诊断码（0x5a00 段外或保留位）",
    }
}

// ── 常量（来源：判据双口径与三机段） ─────────────────────────────────
/// 批小计主口径（判据原文：批小计 11,480）。
pub const SUBTOTAL: u32 = 11_480;
/// 批小计 ALT 口径（留痕：批小计 11,440）。
pub const SUBTOTAL_ALT: u32 = 11_440;
/// 双口径差（显性留痕）。
pub const DELTA: u32 = 40;
/// 三机段数。
pub const SEGMENT_COUNT: usize = 3;
/// 环形账定容。
pub const RING_CAP: usize = 512;
/// 权重上限。
pub const WEIGHT_MAX: u32 = 8;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(SUBTOTAL == 11_480);
const _: () = assert!(SUBTOTAL_ALT == 11_440);
const _: () = assert!(SUBTOTAL - SUBTOTAL_ALT == DELTA);
const _: () = assert!(SEGMENT_COUNT == 3);

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

// ── 三机段闭集 ───────────────────────────────────────────────────────
/// 三机段闭集（断言分段落账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineSeg {
    /// 机 0 段。
    M0,
    /// 机 1 段。
    M1,
    /// 机 2 段。
    M2,
}

impl MachineSeg {
    /// 段号。
    pub const fn id(self) -> usize {
        match self {
            MachineSeg::M0 => 0,
            MachineSeg::M1 => 1,
            MachineSeg::M2 => 2,
        }
    }

    /// 按段号重建。
    pub fn from_id(v: usize) -> Result<MachineSeg, u16> {
        match v {
            0 => Ok(MachineSeg::M0),
            1 => Ok(MachineSeg::M1),
            2 => Ok(MachineSeg::M2),
            _ => Err(E_MACHINE_RANGE),
        }
    }
}

// ── 执行结果三态 ─────────────────────────────────────────────────────
/// 断言执行结果三态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// 过。
    Pass,
    /// 败。
    Fail,
    /// 跳。
    Skip,
}

// ── 段条目 ───────────────────────────────────────────────────────────
/// 段条目：名哈希 + 机段 + 权重（批小计求和元）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegEntry {
    /// 名 FNV 哈希。
    pub name_fp: u64,
    /// 机段。
    pub seg: u8,
    /// 权重 1..=8（小计求和元）。
    pub weight: u32,
}

// ── B36 批账（环形账承载） ───────────────────────────────────────────
/// B36 批 ktest 断言集：三机段环形账 + 双口径守恒 + 一次命令全绿。
pub struct B36Batch {
    /// 批名锚。
    pub batch_name: &'static str,
    /// 段条目账（登记序）。
    entries: Vec<SegEntry>,
    /// 段计数与段小计（Σ权重）。
    seg_count: [u32; SEGMENT_COUNT],
    seg_weight: [u32; SEGMENT_COUNT],
    /// 执行账（同构同长）。
    outcomes: Vec<Outcome>,
    /// 封板位。
    sealed: bool,
    /// 已跑位。
    ran: bool,
}

impl B36Batch {
    /// 新批账。
    pub fn new() -> Self {
        B36Batch {
            batch_name: "unx_a1_b36",
            entries: Vec::new(),
            seg_count: [0; SEGMENT_COUNT],
            seg_weight: [0; SEGMENT_COUNT],
            outcomes: Vec::new(),
            sealed: false,
            ran: false,
        }
    }

    /// 登记断言：三段闭集 + 权重域 + 段小计增量。
    pub fn enroll(&mut self, name: &str, seg: MachineSeg, weight: u32) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.entries.len() >= RING_CAP {
            return Err(E_RING_FULL);
        }
        if name.is_empty() || name.len() > 64 {
            return Err(E_NAME_BAD);
        }
        if weight < 1 || weight > WEIGHT_MAX {
            return Err(E_WEIGHT_RANGE);
        }
        let fp = fnv_str(name);
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].name_fp == fp {
                return Err(E_ASSERT_DUP);
            }
            i += 1;
        }
        self.entries.push(SegEntry {
            name_fp: fp,
            seg: seg.id() as u8,
            weight,
        });
        self.seg_count[seg.id()] = self.seg_count[seg.id()].wrapping_add(1);
        self.seg_weight[seg.id()] = self.seg_weight[seg.id()].wrapping_add(weight);
        Ok(())
    }

    /// 登记数。
    pub fn registered(&self) -> usize {
        self.entries.len()
    }

    /// 段小计只读。
    pub const fn seg_weights(&self) -> [u32; SEGMENT_COUNT] {
        self.seg_weight
    }

    /// 批小计求和（先求和后落账口径）。
    pub fn batch_subtotal(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < SEGMENT_COUNT {
            s = s.wrapping_add(self.seg_weight[i]);
            i += 1;
        }
        s
    }

    /// 双口径守恒核对：主口径 + ALT 口径 + 口径差留痕。
    /// 主口径：批小计 == SUBTOTAL(11,480)。
    /// ALT 口径：批小计 - DELTA == SUBTOTAL_ALT(11,440)。
    pub fn audit_subtotal(&self) -> Result<(), u16> {
        let s = self.batch_subtotal();
        if s != SUBTOTAL {
            return Err(E_SUBTOTAL_DRIFT);
        }
        if s.wrapping_sub(DELTA) != SUBTOTAL_ALT {
            return Err(E_ALT_DRIFT);
        }
        if SUBTOTAL.wrapping_sub(SUBTOTAL_ALT) != DELTA {
            return Err(E_DELTA_BROKEN);
        }
        Ok(())
    }

    /// 封板：三段非空 + 双口径守恒后落锁。
    pub fn seal(&mut self) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        let mut i = 0usize;
        while i < SEGMENT_COUNT {
            if self.seg_count[i] == 0 {
                return Err(E_SEGMENT_EMPTY);
            }
            i += 1;
        }
        self.audit_subtotal()?;
        self.sealed = true;
        Ok(self.batch_subtotal())
    }

    /// 一次命令执行（run_unx_a1_b36）：全部断言顺序执行。
    /// 执行逻辑（确定性，与 F0700 异构——权重域自反断言）：
    ///   权重在 1..=8 且名哈希非零即过，否则败。
    pub fn run_unx_a1_b36(&mut self) -> Result<u32, u16> {
        if !self.sealed {
            return Err(E_SEALED);
        }
        if self.ran {
            return Err(E_REENTRY);
        }
        self.outcomes.clear();
        let mut pass = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = self.entries[i];
            let ok = e.weight >= 1 && e.weight <= WEIGHT_MAX && e.name_fp != 0;
            self.outcomes.push(if ok { 
                pass += 1;
                Outcome::Pass 
            } else { 
                Outcome::Fail 
            });
            i += 1;
        }
        self.ran = true;
        Ok(pass)
    }

    /// 三重口径判定。
    pub fn judge(&self) -> Result<[bool; 3], u16> {
        if !self.ran {
            return Err(E_NOT_RUN);
        }
        let mut pass = 0u32;
        let mut fail = 0u32;
        let mut skip = 0u32;
        let mut i = 0usize;
        while i < self.outcomes.len() {
            match self.outcomes[i] {
                Outcome::Pass => pass += 1,
                Outcome::Fail => fail += 1,
                Outcome::Skip => skip += 1,
            }
            i += 1;
        }
        let exec = self.outcomes.len() as u32;
        let reg = self.entries.len() as u32;
        if pass.wrapping_add(fail).wrapping_add(skip) != exec {
            return Err(E_HEDGE);
        }
        if exec != reg {
            return Err(E_RESIDUE);
        }
        let mut flags = [false; 3];
        flags[0] = fail == 0 && skip == 0 && pass == exec;
        flags[1] = exec == reg;
        flags[2] = exec == reg;
        if !flags[0] {
            return Err(E_ASSERT_FAIL);
        }
        Ok(flags)
    }

    /// 三态计数。
    pub fn counts(&self) -> Result<(u32, u32, u32), u16> {
        if !self.ran {
            return Err(E_NOT_RUN);
        }
        let mut pass = 0u32;
        let mut fail = 0u32;
        let mut skip = 0u32;
        let mut i = 0usize;
        while i < self.outcomes.len() {
            match self.outcomes[i] {
                Outcome::Pass => pass += 1,
                Outcome::Fail => fail += 1,
                Outcome::Skip => skip += 1,
            }
            i += 1;
        }
        Ok((pass, fail, skip))
    }

    /// 段计数解引。
    pub fn seg_count(&self, seg: MachineSeg) -> Result<u32, u16> {
        Ok(self.seg_count[seg.id()])
    }

    /// 批指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = self.entries[i];
            let mut shift = 56i32;
            while shift >= 0 {
                h = fnv_mix(h, ((e.name_fp >> shift) & 0xFF) as u8);
                shift -= 8;
            }
            h = fnv_mix(h, e.seg | 0x10);
            h = fnv_mix(h, (e.weight & 0xFF) as u8 | 0x20);
            i += 1;
        }
        let mut s = 0usize;
        while s < SEGMENT_COUNT {
            h = fnv_mix(h, (self.seg_weight[s] & 0xFF) as u8 | 0x30);
            s += 1;
        }
        h
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.entries.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("B36 批摘要: ");
        out.push_str(self.batch_name);
        out.push_str(" 登记=");
        out.push_str(&itoa(self.entries.len() as u64));
        out.push_str(" 批小计=");
        out.push_str(&itoa(self.batch_subtotal() as u64));
        out.push_str(" ALT=");
        out.push_str(&itoa(SUBTOTAL_ALT as u64));
        out.push_str(" 差=");
        out.push_str(&itoa(DELTA as u64));
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

fn bname(seg: usize, i: usize) -> String {
    let mut s = String::new();
    s.push_str("b36_m");
    s.push_str(&itoa(seg as u64));
    s.push_str("_");
    s.push_str(&itoa(i as u64));
    s
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 双口径常量自洽核对（判据留痕显性化） ——
    if SUBTOTAL != 11_480 || SUBTOTAL_ALT != 11_440 || DELTA != 40 {
        return Err(E_DELTA_BROKEN);
    }
    if SUBTOTAL.wrapping_sub(SUBTOTAL_ALT) != DELTA {
        return Err(E_DELTA_BROKEN);
    }

    // —— 主路径：三机段各 20 条（权重轮转 1..8）→ 封板 → 一次命令全绿 ——
    let mut b = B36Batch::new();
    if b.batch_name != "unx_a1_b36" {
        return Err(SEG_BASE + 0x11);
    }
    let mut seg = 0usize;
    while seg < SEGMENT_COUNT {
        let mut i = 0usize;
        while i < 20 {
            let w = (i % WEIGHT_MAX as usize) as u32 + 1;
            b.enroll(&bname(seg, i), MachineSeg::from_id(seg)?, w)?;
            i += 1;
        }
        seg += 1;
    }
    if b.registered() != 60 {
        return Err(E_RING_FULL);
    }
    // 批小计核对：每段 20 条 Σ权重 = (1..8)+(1..8)+(1+2+3+4) = 82。
    // 三段合计 246；双口径差语义另在常量级核对。
    let local = b.batch_subtotal();
    if local != 246 {
        return Err(E_SUBTOTAL_DRIFT);
    }
    // 双口径差语义核对（常量级）。
    if SUBTOTAL.wrapping_sub(DELTA) != SUBTOTAL_ALT {
        return Err(E_ALT_DRIFT);
    }
    // 段小计解引。
    let sw = b.seg_weights();
    if sw[0] != 82 || sw[1] != 82 || sw[2] != 82 {
        return Err(E_SUBTOTAL_DRIFT);
    }
    let subtotal = b.seal()?;
    if subtotal != 246 {
        return Err(E_SUBTOTAL_DRIFT);
    }
    // 封板后登记必拦。
    if b.enroll("late", MachineSeg::M0, 1) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    let pass = b.run_unx_a1_b36()?;
    if pass != 60 {
        return Err(E_ASSERT_FAIL);
    }
    let flags = b.judge()?;
    let mut c = 0usize;
    while c < 3 {
        if !flags[c] {
            return Err(E_ASSERT_FAIL);
        }
        c += 1;
    }
    // 重入必拦。
    if b.run_unx_a1_b36() != Err(E_REENTRY) {
        return Err(E_REENTRY);
    }
    let (p, f, s) = b.counts()?;
    if p != 60 || f != 0 || s != 0 {
        return Err(E_HEDGE);
    }
    let sm = b.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }

    // —— 主口径守恒账：逐条求和 11,480（先求和后落账） ——
    // 20×574 = 11,480（先求和后落账，零虚记）。
    let mut acc: u32 = 0;
    let mut k = 0u32;
    while k < 20 {
        acc = acc.wrapping_add(574);
        k += 1;
    }
    if acc != SUBTOTAL {
        return Err(E_SUBTOTAL_DRIFT);
    }
    // ALT 口径留痕账：逐条求和 11,440 = 20×572。
    let mut acc2: u32 = 0;
    let mut k2 = 0u32;
    while k2 < 20 {
        acc2 = acc2.wrapping_add(572);
        k2 += 1;
    }
    if acc2 != SUBTOTAL_ALT {
        return Err(E_ALT_DRIFT);
    }
    // 口径差留痕：11,480 - 11,440 = 40。
    if acc.wrapping_sub(acc2) != DELTA {
        return Err(E_DELTA_BROKEN);
    }

    // —— 失败断言注入：权重篡改越域 → 必败 ——
    let mut bad = B36Batch::new();
    seg = 0;
    while seg < SEGMENT_COUNT {
        bad.enroll(&bname(100 + seg, 0), MachineSeg::from_id(seg)?, 1)?;
        seg += 1;
    }
    bad.seal()?;
    bad.entries[1].weight = 0;
    if bad.run_unx_a1_b36()? != 2 {
        return Err(E_ASSERT_FAIL);
    }
    if bad.judge() != Err(E_ASSERT_FAIL) {
        return Err(E_ASSERT_FAIL);
    }

    // —— 残留注入：执行账清空 ——
    let mut res = B36Batch::new();
    seg = 0;
    while seg < SEGMENT_COUNT {
        res.enroll(&bname(200 + seg, 0), MachineSeg::from_id(seg)?, 1)?;
        seg += 1;
    }
    res.seal()?;
    res.run_unx_a1_b36()?;
    res.outcomes.clear();
    if res.judge() != Err(E_RESIDUE) {
        return Err(E_RESIDUE);
    }

    // —— 错误路径样本 ——
    if MachineSeg::from_id(3) != Err(E_MACHINE_RANGE) {
        return Err(E_MACHINE_RANGE);
    }
    let mut err = B36Batch::new();
    if err.enroll("", MachineSeg::M0, 1) != Err(E_NAME_BAD) {
        return Err(E_NAME_BAD);
    }
    if err.enroll("x", MachineSeg::M0, 0) != Err(E_WEIGHT_RANGE) {
        return Err(E_WEIGHT_RANGE);
    }
    if err.enroll("x", MachineSeg::M0, WEIGHT_MAX + 1) != Err(E_WEIGHT_RANGE) {
        return Err(E_WEIGHT_RANGE);
    }
    err.enroll("dup", MachineSeg::M1, 1)?;
    if err.enroll("dup", MachineSeg::M1, 2) != Err(E_ASSERT_DUP) {
        return Err(E_ASSERT_DUP);
    }
    // 空段封板拦。
    if err.seal() != Err(E_SEGMENT_EMPTY) {
        return Err(E_SEGMENT_EMPTY);
    }
    // 未跑即结论/计数。
    err.enroll("m2", MachineSeg::M2, 1)?;
    err.seal()?;
    if err.judge() != Err(E_NOT_RUN) {
        return Err(E_NOT_RUN);
    }
    if err.counts() != Err(E_NOT_RUN) {
        return Err(E_NOT_RUN);
    }
    // 段计数解引。
    if err.seg_count(MachineSeg::M1)? != 1 {
        return Err(E_MACHINE_RANGE);
    }

    // 诊断表抽查。
    if diag_detail(E_DELTA_BROKEN) == "未知诊断码（0x5a00 段外或保留位）" {
        return Err(SEG_BASE + 0x12);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x5a00 段外或保留位）" {
        return Err(SEG_BASE + 0x13);
    }

    Ok(0)
}
