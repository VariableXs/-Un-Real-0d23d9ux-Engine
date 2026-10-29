//! UNX-F0763 · ktest 断言全域终盘（≥300 计数守恒+全绿复核） · 诊断段 0x5e20
//! 判据原文摘录：UNX-F0763-J1 终盘过（全域断言计数 ≥300 守恒核 +
//! 最近一次全绿执行账复核 + 封版一致 100%）；完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。纯功能代码 ≥570 行。
//! 命名注：unx0763_finalsweep.rs 已被在先占用，本单按落点纪律使用 _v2 名。
//!
//! 设计纪律：
//! 1. 全域断言计数 ≥300：终盘账逐批汇总 40 批断言数，合计 ≥300 才算
//!    全域成立；计数来源与封版清单逐位一致。
//! 2. 守恒核：批计数之和 == 全域计数 == 封版清单条数（三数恒等），
//!    任一不等即终盘拒绝。
//! 3. 最近一次全绿执行账复核：执行账行（批次+绿数+时间序）末行必须
//!    全绿且与全域计数一致，断链即复核失败。
//! 4. 封版一致 100%：终盘批计数与 F0752 封版批映射逐批相等。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5e20 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5e20;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_BATCH_BAD: u16 = SEG_BASE + 1; // 批号越界（1..=40）
const E_BATCH_TWICE: u16 = SEG_BASE + 2; // 批计数重复落账
const E_BATCH_ZERO: u16 = SEG_BASE + 3; // 批断言数为 0（每批 ≥1）
const E_MIN300: u16 = SEG_BASE + 4; // 全域计数不足 300
const E_CONSERVE: u16 = SEG_BASE + 5; // 守恒核失败（批和≠全域）
const E_FREEZE_MISMATCH: u16 = SEG_BASE + 6; // 与封版批映射不一致
const E_EXEC_EMPTY: u16 = SEG_BASE + 7; // 执行账为空
const E_EXEC_RED: u16 = SEG_BASE + 8; // 最近一次执行存在红
const E_EXEC_STALE: u16 = SEG_BASE + 9; // 执行账断链（序号不连续）
const E_BAL: u16 = SEG_BASE + 10; // 守恒恒等式破裂
const E_SEALED: u16 = SEG_BASE + 11; // 终盘已封，拒绝再写
const E_EMPTY: u16 = SEG_BASE + 12; // 终盘账为空
const E_EXEC_TWICE: u16 = SEG_BASE + 13; // 执行账行重复序号
const E_BATCH_LACK: u16 = SEG_BASE + 14; // 40 批未齐即终盘被拒

/// 批数闭集：40。
pub const BATCHES: usize = 40;

/// 全域断言下限：300。
pub const MIN_ASSERTS: usize = 300;

/// 执行账定容。
pub const MAX_EXEC: usize = 48;

/// 封版批映射定容（40 批 × 每批 8 条的标准封版口径）。
pub const STD_PER_BATCH: u32 = 8;

// ── FNV-1a 64（终盘哈希口径）──────────────────────────────────
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
        E_BATCH_BAD => "批号越界：B01..B40",
        E_BATCH_TWICE => "批计数重复落账：一批一行",
        E_BATCH_ZERO => "批断言数为 0：每批至少 1 条",
        E_MIN300 => "全域计数不足 300：先补断言",
        E_CONSERVE => "守恒核失败：批计数之和必须等于全域计数",
        E_FREEZE_MISMATCH => "与封版批映射不一致：逐批核对",
        E_EXEC_EMPTY => "执行账为空：先执行一次全绿",
        E_EXEC_RED => "最近一次执行存在红：先修复再终盘",
        E_EXEC_STALE => "执行账断链：执行序号须连续",
        E_BAL => "守恒破裂：计数账不平",
        E_SEALED => "终盘已封：封账后只读",
        E_EMPTY => "终盘账为空",
        E_EXEC_TWICE => "执行账行重复序号",
        E_BATCH_LACK => "40 批未齐：先补齐批计数",
        _ => "未知诊断码",
    }
}

// ── 批计数行 ─────────────────────────────────────────────────
/// 一批的断言计数行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchCount {
    /// 批号（1..=40）。
    pub batch: u32,
    /// 断言数。
    pub asserts: u32,
}

// ── 执行账行 ─────────────────────────────────────────────────
/// 一次 ktest 执行的账行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecRow {
    /// 执行序号（1 起连续）。
    pub exec_no: u32,
    /// 绿数。
    pub green: u32,
    /// 红数。
    pub red: u32,
    /// 是否全绿。
    pub all_green: bool,
}

// ── ktest 全域终盘账（主体）──────────────────────────────────
/// ktest 断言全域终盘：批计数汇总 + 守恒 + 执行账复核 + 封版一致核。
pub struct KtestFinal {
    /// 批计数账（batch → asserts）。
    counts: [u32; BATCHES + 1],
    batch_marked: [bool; BATCHES + 1],
    batch_total: u32,
    /// 执行账。
    execs: [Option<ExecRow>; MAX_EXEC],
    exec_count: usize,
    /// 是否封账。
    sealed: bool,
    /// 终盘哈希。
    sealed_hash: u64,
}

impl KtestFinal {
    /// 开终盘。
    pub fn new() -> KtestFinal {
        KtestFinal {
            counts: [0; BATCHES + 1],
            batch_marked: [false; BATCHES + 1],
            batch_total: 0,
            execs: [None; MAX_EXEC],
            exec_count: 0,
            sealed: false,
            sealed_hash: 0,
        }
    }

    /// 落批计数：批号闭集 + ≥1 + 一批一行。
    pub fn put_batch_count(&mut self, batch: u32, asserts: u32) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if batch == 0 || batch as usize > BATCHES {
            return Err(E_BATCH_BAD);
        }
        if asserts == 0 {
            return Err(E_BATCH_ZERO);
        }
        if self.batch_marked[batch as usize] {
            return Err(E_BATCH_TWICE);
        }
        self.counts[batch as usize] = asserts;
        self.batch_marked[batch as usize] = true;
        self.batch_total += asserts;
        Ok(())
    }

    /// 批计数只读。
    pub fn batch_count(&self, batch: u32) -> Result<u32, u16> {
        if batch == 0 || batch as usize > BATCHES {
            return Err(E_BATCH_BAD);
        }
        if !self.batch_marked[batch as usize] {
            return Err(E_BATCH_LACK);
        }
        Ok(self.counts[batch as usize])
    }

    /// 已落批数。
    pub fn batches_marked(&self) -> u32 {
        let mut n = 0u32;
        let mut b = 1usize;
        while b <= BATCHES {
            if self.batch_marked[b] {
                n += 1;
            }
            b += 1;
        }
        n
    }

    /// 全域计数（批计数之和，实时重算防漂移）。
    pub fn total_asserts(&self) -> u32 {
        let mut s = 0u32;
        let mut b = 1usize;
        while b <= BATCHES {
            s += self.counts[b];
            b += 1;
        }
        s
    }

    /// 落执行账行：序号连续 + 三数一致。
    pub fn put_exec_row(&mut self, green: u32, red: u32) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.exec_count >= MAX_EXEC {
            return Err(E_BAL);
        }
        let exec_no = (self.exec_count + 1) as u32;
        let all_green = red == 0 && green > 0;
        self.execs[self.exec_count] = Some(ExecRow {
            exec_no,
            green,
            red,
            all_green,
        });
        self.exec_count += 1;
        Ok(exec_no)
    }

    /// 执行账只读行数。
    pub fn exec_count(&self) -> usize {
        self.exec_count
    }

    /// 最近一次执行行。
    pub fn last_exec(&self) -> Result<ExecRow, u16> {
        if self.exec_count == 0 {
            return Err(E_EXEC_EMPTY);
        }
        match &self.execs[self.exec_count - 1] {
            Some(e) => Ok(*e),
            None => Err(E_EXEC_EMPTY),
        }
    }

    /// 执行账断链核：序号 1..N 连续。
    pub fn exec_chain_assert(&self) -> Result<(), u16> {
        if self.exec_count == 0 {
            return Err(E_EXEC_EMPTY);
        }
        let mut n = 1u32;
        while n as usize <= self.exec_count {
            match &self.execs[(n - 1) as usize] {
                Some(e) if e.exec_no == n => {}
                _ => return Err(E_EXEC_STALE),
            }
            n += 1;
        }
        Ok(())
    }

    /// 守恒核：批计数之和 == 落账累计 == 全域重算。
    pub fn conserved(&self) -> bool {
        self.batch_total == self.total_asserts()
    }

    /// 封版一致 100% 断言：批计数与封版批映射逐批相等。
    pub fn freeze_consistency_assert(&self, freeze_map: &[u32; BATCHES]) -> Result<(), u16> {
        if freeze_map.len() != BATCHES {
            return Err(E_FREEZE_MISMATCH);
        }
        let mut b = 0usize;
        while b < BATCHES {
            if !self.batch_marked[b + 1] {
                return Err(E_BATCH_LACK);
            }
            if self.counts[b + 1] != freeze_map[b] {
                return Err(E_FREEZE_MISMATCH);
            }
            b += 1;
        }
        Ok(())
    }

    /// 终盘断言：40 批齐 + ≥300 + 守恒 + 最近执行全绿 + 执行账断链核。
    pub fn final_assert(&self) -> Result<(), u16> {
        if self.batches_marked() != BATCHES as u32 {
            return Err(E_BATCH_LACK);
        }
        if self.total_asserts() < MIN_ASSERTS as u32 {
            return Err(E_MIN300);
        }
        if !self.conserved() {
            return Err(E_CONSERVE);
        }
        let last = self.last_exec()?;
        if !last.all_green {
            return Err(E_EXEC_RED);
        }
        if last.green != self.total_asserts() {
            return Err(E_EXEC_RED);
        }
        self.exec_chain_assert()?;
        Ok(())
    }

    /// 封账：终盘断言过 → 终盘哈希。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.final_assert()?;
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.total_asserts().to_le_bytes());
        let mut b = 1usize;
        while b <= BATCHES {
            buf.extend_from_slice(self.counts[b].to_le_bytes().as_slice());
            b += 1;
        }
        if let Some(e) = &self.execs[self.exec_count - 1] {
            buf.extend_from_slice(e.exec_no.to_le_bytes().as_slice());
            buf.push(e.all_green as u8);
        }
        self.sealed_hash = fnv1a64(&buf);
        self.sealed = true;
        Ok(self.sealed_hash)
    }

    /// 终盘哈希。
    pub fn sealed_hash(&self) -> u64 {
        self.sealed_hash
    }

    /// 是否封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 终盘报告行。
    pub fn report_lines(&self, sample: usize) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== ktest 断言全域终盘 ===".to_string());
        lines.push("全域计数: ".to_string() + &itoa(self.total_asserts() as u64)
            + "（下限 300）");
        lines.push("批覆盖: ".to_string() + &itoa(self.batches_marked() as u64) + "/40");
        let step = if sample == 0 { 1 } else { sample };
        let mut b = 1usize;
        while b <= BATCHES {
            if b % step == 0 {
                let mut line = String::from("B");
                line.push_str(&itoa(b as u64));
                line.push_str("|");
                line.push_str(&itoa(self.counts[b] as u64));
                line.push_str(" 条");
                lines.push(line);
            }
            b += 1;
        }
        lines
    }

    /// 批分布三元组：(已落批数, 最小批计数, 最大批计数)。
    pub fn batch_distribution(&self) -> (u32, u32, u32) {
        let mut mn = u32::MAX;
        let mut mx = 0u32;
        let mut used = 0u32;
        let mut b = 1usize;
        while b <= BATCHES {
            if self.batch_marked[b] {
                used += 1;
                if self.counts[b] < mn {
                    mn = self.counts[b];
                }
                if self.counts[b] > mx {
                    mx = self.counts[b];
                }
            }
            b += 1;
        }
        if used == 0 {
            return (0, 0, 0);
        }
        (used, mn, mx)
    }

    /// 绿率千分值：最近一次执行绿数 / 全域计数（千分整数化）。
    pub fn green_rate_permille(&self) -> Result<u32, u16> {
        let last = self.last_exec()?;
        let total = self.total_asserts();
        if total == 0 {
            return Err(E_EMPTY);
        }
        Ok(last.green * 1000 / total)
    }

    /// 尾部全绿连击：从最近一行往回数连续 all_green 行数。
    pub fn last_green_streak(&self) -> u32 {
        let mut n = 0u32;
        let mut i = self.exec_count;
        while i > 0 {
            match &self.execs[i - 1] {
                Some(e) if e.all_green => n += 1,
                _ => break,
            }
            i -= 1;
        }
        n
    }

    /// 低于下限的批次清单（批均衡排查口）：threshold = 批计数下限。
    pub fn underfloor_batches(&self, threshold: u32) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut b = 1usize;
        while b <= BATCHES {
            if self.batch_marked[b] && self.counts[b] < threshold {
                out.push(b as u32);
            }
            b += 1;
        }
        out
    }

    /// 执行账行渲染（全账）。
    pub fn exec_history_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 全绿执行账 ===".to_string());
        let mut i = 0usize;
        while i < self.exec_count {
            if let Some(e) = &self.execs[i] {
                let mut line = String::from("X");
                line.push_str(&itoa(e.exec_no as u64));
                line.push_str("|绿 ");
                line.push_str(&itoa(e.green as u64));
                line.push_str("|红 ");
                line.push_str(&itoa(e.red as u64));
                line.push_str("|");
                line.push_str(if e.all_green { "全绿" } else { "有红" });
                lines.push(line);
            }
            i += 1;
        }
        lines
    }

    /// 批计数直方图：计数值 → 批数（分布形态取数口，桶 0..=12）。
    pub fn batch_histogram(&self) -> [u32; 13] {
        let mut hist = [0u32; 13];
        let mut b = 1usize;
        while b <= BATCHES {
            if self.batch_marked[b] {
                let c = self.counts[b] as usize;
                if c < 13 {
                    hist[c] += 1;
                }
            }
            b += 1;
        }
        hist
    }

    /// 执行账累计绿数（历史全量口径）。
    pub fn exec_total_green(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.exec_count {
            if let Some(e) = &self.execs[i] {
                s += e.green;
            }
            i += 1;
        }
        s
    }
}

/// 极简无符号转十进制。
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

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：40 批 320 条 + 全绿执行账 + 封版一致 + 封账 + 错误路径。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let mut kf = KtestFinal::new();
    // 40 批 × 8 条 = 320（≥300 达标），与 F0752 封版同构。
    let mut freeze_map = [0u32; BATCHES];
    let mut b = 1u32;
    while b as usize <= BATCHES {
        kf.put_batch_count(b, STD_PER_BATCH)?;
        freeze_map[(b - 1) as usize] = STD_PER_BATCH;
        b += 1;
    }
    if kf.total_asserts() != 320 {
        return Err(SEG_BASE);
    }
    kf.freeze_consistency_assert(&freeze_map)?;
    // 执行账：先红后绿两行（最近一次必须全绿）。
    let _ = kf.put_exec_row(318, 2)?;
    let e2 = kf.put_exec_row(320, 0)?;
    if e2 != 2 {
        return Err(SEG_BASE);
    }
    kf.final_assert()?;
    kf.exec_chain_assert()?;
    if !kf.conserved() {
        return Err(SEG_BASE);
    }
    // 取数口：分布、绿率、连击、下限排查、执行账行。
    let (used, mn, mx) = kf.batch_distribution();
    if used != 40 || mn != STD_PER_BATCH || mx != STD_PER_BATCH {
        return Err(SEG_BASE);
    }
    if kf.green_rate_permille()? != 1000 {
        return Err(SEG_BASE);
    }
    if kf.last_green_streak() != 1 {
        return Err(SEG_BASE);
    }
    if !kf.underfloor_batches(STD_PER_BATCH).is_empty() {
        return Err(SEG_BASE);
    }
    let xl = kf.exec_history_lines();
    if xl.len() != 3 {
        return Err(SEG_BASE);
    }
    let rl = kf.report_lines(8);
    if rl.len() != 8 {
        return Err(SEG_BASE);
    }
    // 直方图与累计绿数。
    let hist = kf.batch_histogram();
    if hist[STD_PER_BATCH as usize] != 40 {
        return Err(SEG_BASE);
    }
    if kf.exec_total_green() != 638 {
        return Err(SEG_BASE);
    }
    let h = kf.seal()?;
    if h == 0 || !kf.is_sealed() {
        return Err(SEG_BASE);
    }
    // 封账后写拒绝。
    match kf.put_batch_count(1, 1) {
        Err(c) if c == E_SEALED => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：不足 300。
    let mut kf2 = KtestFinal::new();
    let mut b = 1u32;
    while b as usize <= BATCHES {
        kf2.put_batch_count(b, 7)?;
        b += 1;
    }
    let _ = kf2.put_exec_row(280, 0)?;
    match kf2.final_assert() {
        Err(c) if c == E_MIN300 => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：最近一次执行有红。
    let mut kf3 = KtestFinal::new();
    let mut b = 1u32;
    while b as usize <= BATCHES {
        kf3.put_batch_count(b, STD_PER_BATCH)?;
        b += 1;
    }
    let _ = kf3.put_exec_row(320, 0)?;
    let _ = kf3.put_exec_row(319, 1)?;
    match kf3.final_assert() {
        Err(c) if c == E_EXEC_RED => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：与封版映射不一致。
    let mut kf4 = KtestFinal::new();
    let mut b = 1u32;
    while b as usize <= BATCHES {
        kf4.put_batch_count(b, STD_PER_BATCH)?;
        b += 1;
    }
    let mut bad_map = freeze_map;
    bad_map[0] = 9;
    match kf4.freeze_consistency_assert(&bad_map) {
        Err(c) if c == E_FREEZE_MISMATCH => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}
