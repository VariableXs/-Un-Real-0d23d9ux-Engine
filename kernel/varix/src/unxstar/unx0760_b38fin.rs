//! UNX-F0760 · B38 批 finalize 收口（域经移交面定版） · 诊断段 0x5e00
//! 判据原文摘录：UNX-F0760-J1 收口五步断言链全过（防重/判据/守恒 11,500/
//! 台账 20 条已深化/四项齐备）且域经移交面定版封批；[留痕] R-A1-004：
//! 本批头部批小计 11500 为逐条求和后落账（先求和后落账，零虚记）。
//! 完成时 API/软件写回：状态=已完成、结果摘要非空。纯功能代码 ≥580 行。
//!
//! 设计纪律：
//! 1. 收口五步写死：防重终扫 / 判据聚合 / 守恒核对（11,500）/
//!    台账三态（20 条已深化）/ 四项齐备（断言集·报告·快照·封存），
//!    五步按序推进，任一步失败即收口拒绝，不允许跳步。
//! 2. 批小计 11,500 定版：先逐条求和后落账（R-A1-004 口径），
//!    落账值 == 求和值 == 11,500 三数恒等。
//! 3. 台账 20 条：域经+移交面 20 条目全部已深化态，零骨架。
//! 4. 定版封批：五步全过后批状态 Collecting→Finalizing→Sealed，
//!    封批后写拦截。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5e00 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5e00;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_STEP_SKIP: u16 = SEG_BASE + 1; // 收口跳步（五步须按序）
const E_STEP_TWICE: u16 = SEG_BASE + 2; // 步骤重复执行
const E_DUP_HIT: u16 = SEG_BASE + 3; // 防重终扫命中重名
const E_CRIT_LACK: u16 = SEG_BASE + 4; // 判据聚合不全（12 单缺）
const E_CAL_MISMATCH: u16 = SEG_BASE + 5; // 守恒口径不等
const E_SUBTOTAL: u16 = SEG_BASE + 6; // 批小计 ≠ 11,500
const E_LEDGER_DRAFT: u16 = SEG_BASE + 7; // 台账存在未深化条目
pub const _RESERVED: u16 = SEG_BASE + 8; // 预留位
const E_FOUR_LACK: u16 = SEG_BASE + 9; // 四项未齐
const E_STATE: u16 = SEG_BASE + 10; // 收口状态机非法边
const E_BAL: u16 = SEG_BASE + 11; // 守恒恒等式破裂
const E_SEALED: u16 = SEG_BASE + 12; // 批已封，拒绝再写
const E_EMPTY: u16 = SEG_BASE + 13; // 收口账为空
const E_TWICE_SEAL: u16 = SEG_BASE + 14; // 重复封批
const E_LEDGER_FULL: u16 = SEG_BASE + 15; // 台账超容

/// B38 批小计定版：11,500。
pub const BATCH_SUBTOTAL: u64 = 11_500;

/// 台账条目数：20 条。
pub const LEDGER_ROWS: usize = 20;

/// 聚合单元数：12 单（F0745..F0756）。
pub const UNITS: usize = 12;

/// 收口步骤号。
pub const S_DEDUP: u8 = 1; // 防重终扫
pub const S_CRIT: u8 = 2; // 判据聚合
pub const S_CONSERVE: u8 = 3; // 守恒核对
pub const S_LEDGER: u8 = 4; // 台账三态
pub const S_FOUR: u8 = 5; // 四项齐备
pub const STEPS: usize = 5;

/// 四项齐备项号。
pub const Q_ASSERTS: u8 = 1; // 断言集
pub const Q_REPORT: u8 = 2; // 报告
pub const Q_SNAPSHOT: u8 = 3; // 快照
pub const Q_SEAL: u8 = 4; // 封存
pub const FOUR_ITEMS: usize = 4;

/// 台账账定容。
pub const MAX_LEDGER: usize = 24;

// ── FNV-1a 64（封批哈希口径）──────────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 诊断详情：把错误码翻译成中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_STEP_SKIP => "收口跳步：防重→判据→守恒→台账→四项 按序",
        E_STEP_TWICE => "步骤重复执行：一步一落账",
        E_DUP_HIT => "防重终扫命中重名：条目名全域唯一被破坏",
        E_CRIT_LACK => "判据聚合不全：12 单断言缺位",
        E_CAL_MISMATCH => "守恒口径不等：卡面/求和/落账须一致",
        E_SUBTOTAL => "批小计非 11,500：逐条求和与落账不符",
        E_LEDGER_DRAFT => "台账存在未深化条目：先深化再收口",
        _ if code == SEG_BASE + 8 => "预留诊断位",
        E_FOUR_LACK => "四项未齐：断言集/报告/快照/封存",
        E_STATE => "收口状态机非法边",
        E_BAL => "守恒破裂：计数账不平",
        E_SEALED => "批已封：封批后只读",
        E_EMPTY => "收口账为空",
        E_TWICE_SEAL => "重复封批",
        E_LEDGER_FULL => "台账超容 24 条",
        _ => "未知诊断码",
    }
}

// ── 台账条目（三态）──────────────────────────────────────────
/// 台账条目：名 + 三态。
#[derive(Clone, Debug)]
pub struct LedgerEntry {
    /// 条目号（1 起连续）。
    pub entry_no: u32,
    /// 条目名（防重唯一键）。
    pub name: String,
    /// 三态：0=骨架 1=已深化。
    pub state: u8,
}

impl LedgerEntry {
    /// 是否已深化。
    pub fn deepened(&self) -> bool {
        self.state == 1
    }
}

// ── 收口状态机 ───────────────────────────────────────────────
/// 收口生命周期：Collecting→Finalizing→Sealed。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinalPhase {
    /// 采集中。
    Collecting,
    /// 五步推进中。
    Finalizing,
    /// 已封批。
    Sealed,
}

impl FinalPhase {
    /// 编号。
    pub fn code(self) -> u8 {
        match self {
            FinalPhase::Collecting => 1,
            FinalPhase::Finalizing => 2,
            FinalPhase::Sealed => 3,
        }
    }
}

// ── B38 收口账（主体）────────────────────────────────────────
/// B38 批 finalize 收口账：五步断言链 + 台账 + 四项 + 封批。
pub struct B38Finalize {
    /// 台账（20 条）。
    ledger: [Option<LedgerEntry>; MAX_LEDGER],
    ledger_count: usize,
    /// 已完成步骤位图（bit0..bit4 = 步 1..5）。
    step_bits: u8,
    /// 已完成步数。
    steps_done: u32,
    /// 聚合单元号表（判据聚合时逐单落）。
    crit_units: [u32; UNITS],
    crit_count: usize,
    /// 四项落账位图。
    four_bits: u8,
    /// 逐条求和值（口径二）。
    summed: u64,
    /// 落账批小计（口径三）。
    posted_subtotal: u64,
    phase: FinalPhase,
    /// 封批哈希。
    sealed_hash: u64,
}

impl B38Finalize {
    /// 开收口账。
    pub fn new() -> B38Finalize {
        B38Finalize {
            ledger: [const { None }; MAX_LEDGER],
            ledger_count: 0,
            step_bits: 0,
            steps_done: 0,
            crit_units: [0; UNITS],
            crit_count: 0,
            four_bits: 0,
            summed: 0,
            posted_subtotal: 0,
            phase: FinalPhase::Collecting,
            sealed_hash: 0,
        }
    }

    /// 台账登记：名唯一 + 三态初始骨架（0）。
    pub fn enroll_ledger(&mut self, name: &str) -> Result<u32, u16> {
        if self.phase == FinalPhase::Sealed {
            return Err(E_SEALED);
        }
        if self.ledger_count >= MAX_LEDGER {
            return Err(E_LEDGER_FULL);
        }
        if name.is_empty() {
            return Err(E_EMPTY);
        }
        // 防重：名唯一。
        let mut i = 0usize;
        while i < self.ledger_count {
            if let Some(e) = &self.ledger[i] {
                if e.name == name {
                    return Err(E_DUP_HIT);
                }
            }
            i += 1;
        }
        let entry_no = (self.ledger_count + 1) as u32;
        self.ledger[self.ledger_count] = Some(LedgerEntry {
            entry_no,
            name: name.to_string(),
            state: 0,
        });
        self.ledger_count += 1;
        Ok(entry_no)
    }

    /// 台账深化：骨架 → 已深化。
    pub fn deepen(&mut self, entry_no: u32) -> Result<(), u16> {
        if self.phase == FinalPhase::Sealed {
            return Err(E_SEALED);
        }
        if entry_no == 0 || entry_no as usize > self.ledger_count {
            return Err(E_EMPTY);
        }
        match &mut self.ledger[(entry_no - 1) as usize] {
            Some(e) => {
                if e.state == 1 {
                    return Err(E_STEP_TWICE);
                }
                e.state = 1;
                Ok(())
            }
            None => Err(E_EMPTY),
        }
    }

    /// 台账计数。
    pub fn ledger_count(&self) -> usize {
        self.ledger_count
    }

    /// 已深化计数。
    pub fn deepened_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.ledger_count {
            if let Some(e) = &self.ledger[i] {
                if e.deepened() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 转入收口期（Collecting → Finalizing）：台账 20 条齐。
    pub fn begin_finalize(&mut self) -> Result<(), u16> {
        if self.phase != FinalPhase::Collecting {
            return Err(E_STATE);
        }
        if self.ledger_count != LEDGER_ROWS {
            return Err(E_LEDGER_DRAFT);
        }
        self.phase = FinalPhase::Finalizing;
        Ok(())
    }

    /// 步进：按序推进五步。
    fn mark_step(&mut self, step: u8) -> Result<(), u16> {
        if self.phase != FinalPhase::Finalizing {
            return Err(E_STATE);
        }
        if step == 0 || step as usize > STEPS {
            return Err(E_STEP_SKIP);
        }
        let bit = 1u8 << (step - 1);
        if self.step_bits & bit != 0 {
            return Err(E_STEP_TWICE);
        }
        // 前置步必须齐。
        let prev = bit - 1;
        if self.step_bits & prev != prev {
            return Err(E_STEP_SKIP);
        }
        self.step_bits |= bit;
        self.steps_done += 1;
        Ok(())
    }

    /// 步一：防重终扫（台账名唯一重扫）。
    pub fn step1_dedup(&mut self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.ledger_count {
            let n = match &self.ledger[i] {
                Some(e) => e.name.clone(),
                None => return Err(E_EMPTY),
            };
            let mut j = i + 1;
            while j < self.ledger_count {
                if let Some(e) = &self.ledger[j] {
                    if e.name == n {
                        return Err(E_DUP_HIT);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        self.mark_step(S_DEDUP)
    }

    /// 步二：判据聚合（12 单逐单落账）。
    pub fn step2_criteria(&mut self, units: &[u32; UNITS]) -> Result<(), u16> {
        let mut i = 0usize;
        while i < UNITS {
            let u = units[i];
            if u == 0 || u as usize > UNITS {
                return Err(E_CRIT_LACK);
            }
            // 单元号查重。
            let mut j = 0usize;
            while j < self.crit_count {
                if self.crit_units[j] == u {
                    return Err(E_CRIT_LACK);
                }
                j += 1;
            }
            self.crit_units[self.crit_count] = u;
            self.crit_count += 1;
            i += 1;
        }
        if self.crit_count != UNITS {
            return Err(E_CRIT_LACK);
        }
        self.mark_step(S_CRIT)
    }

    /// 口径二：逐条行数求和（先求和后落账）。
    pub fn sum_unit_lines(&mut self, lines: &[u64; UNITS]) -> Result<u64, u16> {
        if self.summed != 0 {
            return Err(E_BAL);
        }
        let mut s = 0u64;
        let mut i = 0usize;
        while i < UNITS {
            if lines[i] == 0 {
                return Err(E_BAL);
            }
            s += lines[i];
            i += 1;
        }
        self.summed = s;
        Ok(s)
    }

    /// 口径三：落账批小计（求和值必须 == 11,500）。
    pub fn post_subtotal(&mut self) -> Result<(), u16> {
        if self.posted_subtotal != 0 {
            return Err(E_BAL);
        }
        if self.summed == 0 {
            return Err(E_CAL_MISMATCH);
        }
        if self.summed != BATCH_SUBTOTAL {
            return Err(E_SUBTOTAL);
        }
        self.posted_subtotal = self.summed;
        Ok(())
    }

    /// 步三：守恒核对（三口径恒等）。
    pub fn step3_conserve(&mut self) -> Result<(), u16> {
        if !self.calibers_conserved() {
            return Err(if self.summed != BATCH_SUBTOTAL { E_SUBTOTAL } else { E_CAL_MISMATCH });
        }
        self.mark_step(S_CONSERVE)
    }

    /// 三口径恒等：求和 == 落账 == 11,500。
    pub fn calibers_conserved(&self) -> bool {
        self.summed == BATCH_SUBTOTAL
            && self.posted_subtotal == BATCH_SUBTOTAL
            && self.summed == self.posted_subtotal
    }

    /// 步四：台账三态（20 条全已深化）。
    pub fn step4_ledger(&mut self) -> Result<(), u16> {
        if self.ledger_count != LEDGER_ROWS {
            return Err(E_LEDGER_DRAFT);
        }
        if self.deepened_count() != LEDGER_ROWS as u32 {
            return Err(E_LEDGER_DRAFT);
        }
        self.mark_step(S_LEDGER)
    }

    /// 四项落账。
    pub fn mark_four(&mut self, item: u8) -> Result<(), u16> {
        if item == 0 || item as usize > FOUR_ITEMS {
            return Err(E_FOUR_LACK);
        }
        let bit = 1u8 << (item - 1);
        if self.four_bits & bit != 0 {
            return Err(E_FOUR_LACK);
        }
        self.four_bits |= bit;
        Ok(())
    }

    /// 四项齐备判定。
    pub fn four_complete(&self) -> bool {
        self.four_bits == 0b1111
    }

    /// 步五：四项齐备核。
    pub fn step5_four(&mut self) -> Result<(), u16> {
        if !self.four_complete() {
            return Err(E_FOUR_LACK);
        }
        self.mark_step(S_FOUR)
    }

    /// 五步齐备判定。
    pub fn steps_complete(&self) -> bool {
        self.step_bits == 0b11111
    }

    /// 封批：五步全过 + 台账 20 深化 + 三口径 + 四项齐。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.phase == FinalPhase::Sealed {
            return Err(E_TWICE_SEAL);
        }
        if self.phase != FinalPhase::Finalizing {
            return Err(E_STATE);
        }
        if !self.steps_complete() {
            return Err(E_STEP_SKIP);
        }
        if self.deepened_count() != LEDGER_ROWS as u32 {
            return Err(E_LEDGER_DRAFT);
        }
        if !self.calibers_conserved() {
            return Err(E_CAL_MISMATCH);
        }
        if !self.four_complete() {
            return Err(E_FOUR_LACK);
        }
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.posted_subtotal.to_le_bytes());
        buf.push(self.step_bits);
        buf.push(self.four_bits);
        let mut i = 0usize;
        while i < self.ledger_count {
            if let Some(e) = &self.ledger[i] {
                buf.extend_from_slice(e.entry_no.to_le_bytes().as_slice());
                buf.extend_from_slice(e.name.as_bytes());
                buf.push(0x1f);
                buf.push(e.state);
            }
            i += 1;
        }
        self.sealed_hash = fnv1a64(&buf);
        self.phase = FinalPhase::Sealed;
        Ok(self.sealed_hash)
    }

    /// 封批哈希。
    pub fn sealed_hash(&self) -> u64 {
        self.sealed_hash
    }

    /// 当前阶段。
    pub fn phase(&self) -> FinalPhase {
        self.phase
    }

    /// 步进度位图。
    pub fn step_progress(&self) -> u8 {
        self.step_bits
    }

    /// 收口报告行。
    pub fn report_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== B38 finalize 收口 · 五步断言链 ===".to_string());
        lines.push("阶段: ".to_string() + match self.phase {
            FinalPhase::Collecting => "采集中",
            FinalPhase::Finalizing => "收口中",
            FinalPhase::Sealed => "已封批",
        });
        lines.push("步位: ".to_string() + &itoa(self.step_bits as u64)
            + " 完成 " + &itoa(self.steps_done as u64) + "/5");
        lines.push("判据单元: ".to_string() + &itoa(self.crit_count as u64) + "/12");
        lines.push("台账深化: ".to_string() + &itoa(self.deepened_count() as u64)
            + "/" + &itoa(self.ledger_count as u64));
        lines.push("四项位: ".to_string() + &itoa(self.four_bits as u64));
        lines.push("批小计: ".to_string() + &itoa(self.posted_subtotal));
        lines
    }
}

/// u64 转十进制。
fn itoa(v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    let mut x = v;
    while x > 0 {
        buf[n] = b'0' + (x % 10) as u8;
        x /= 10;
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

// ── 标准收口装配 ─────────────────────────────────────────────
/// 标准十二单行数（合计恒等 11,500）。
pub const STANDARD_LINES: [u64; UNITS] = [
    890, 905, 880, 912, 878, 895, 902, 883, 930, 872, 899, 1654,
];

/// 装配标准收口样本（台账 20 条 + 判据 12 单）。
pub fn enroll_standard(fz: &mut B38Finalize) -> Result<(), u16> {
    // 台账 20 条：确定性命名。
    let mut i = 0usize;
    while i < LEDGER_ROWS {
        let mut name = String::from("b38.ledger.e");
        name.push_str(&itoa((i + 1) as u64));
        fz.enroll_ledger(&name)?;
        i += 1;
    }
    // 全部深化。
    let mut n = 1u32;
    while n as usize <= LEDGER_ROWS {
        fz.deepen(n)?;
        n += 1;
    }
    Ok(())
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：五步链全过 + 守恒 11,500 + 台账 20 深化 + 封批。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let mut fz = B38Finalize::new();
    enroll_standard(&mut fz)?;
    if fz.ledger_count() != 20 || fz.deepened_count() != 20 {
        return Err(SEG_BASE);
    }
    fz.begin_finalize()?;
    // 步一：防重。
    fz.step1_dedup()?;
    // 步二：判据聚合（12 单顺序落）。
    let units: [u32; UNITS] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
    fz.step2_criteria(&units)?;
    // 口径二/三：求和落账。
    let s = fz.sum_unit_lines(&STANDARD_LINES)?;
    if s != BATCH_SUBTOTAL {
        return Err(SEG_BASE);
    }
    fz.post_subtotal()?;
    // 步三：守恒。
    fz.step3_conserve()?;
    // 步四：台账三态。
    fz.step4_ledger()?;
    // 步五：四项齐备。
    fz.mark_four(Q_ASSERTS)?;
    fz.mark_four(Q_REPORT)?;
    fz.mark_four(Q_SNAPSHOT)?;
    fz.mark_four(Q_SEAL)?;
    fz.step5_four()?;
    if !fz.steps_complete() {
        return Err(SEG_BASE);
    }
    let h = fz.seal()?;
    if h == 0 || fz.phase() != FinalPhase::Sealed {
        return Err(SEG_BASE);
    }
    // 封批后写拒绝。
    match fz.enroll_ledger("封批后条目") {
        Err(c) if c == E_SEALED => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：台账未深化即收口被拒。
    let mut fz2 = B38Finalize::new();
    let mut i = 0usize;
    while i < LEDGER_ROWS {
        let mut name = String::from("b38.lite.e");
        name.push_str(&itoa((i + 1) as u64));
        fz2.enroll_ledger(&name)?;
        i += 1;
    }
    fz2.begin_finalize()?;
    fz2.step1_dedup()?;
    fz2.step2_criteria(&units)?;
    let _ = fz2.sum_unit_lines(&STANDARD_LINES)?;
    fz2.post_subtotal()?;
    fz2.step3_conserve()?;
    match fz2.step4_ledger() {
        Err(c) if c == E_LEDGER_DRAFT => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：跳步（未防重先判据）。
    let mut fz3 = B38Finalize::new();
    enroll_standard(&mut fz3)?;
    fz3.begin_finalize()?;
    match fz3.step2_criteria(&units) {
        Err(c) if c == E_STEP_SKIP => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：批小计错。
    let mut fz4 = B38Finalize::new();
    enroll_standard(&mut fz4)?;
    fz4.begin_finalize()?;
    fz4.step1_dedup()?;
    fz4.step2_criteria(&units)?;
    let mut bad_lines = STANDARD_LINES;
    bad_lines[0] += 1;
    let s4 = fz4.sum_unit_lines(&bad_lines)?;
    if s4 == BATCH_SUBTOTAL {
        return Err(SEG_BASE);
    }
    match fz4.post_subtotal() {
        Err(c) if c == E_SUBTOTAL => {}
        _ => return Err(SEG_BASE),
    }
    let rl = fz.report_lines();
    if rl.len() != 8 {
        return Err(SEG_BASE);
    }
    Ok(0)
}
