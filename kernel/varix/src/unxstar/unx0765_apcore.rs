//! UNX-F0765 · 八核真机 AP 到齐终核（50 次 8/8 计数复核） · 诊断段 0x5e60
//! 判据原文摘录：UNX-F0765-J1 终核过（8 核真机 50 次 AP 到齐账复核：
//! 8/8 计数逐次核 + 50 次守恒 + 判定再现）；完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。纯功能代码 ≥570 行。
//!
//! 设计纪律：
//! 1. 8/8 逐次核：每次真机启动的 AP 到齐账 8 核逐核核到位（位图 0xFF），
//!    缺任一核即该次启动判不过。
//! 2. 50 次守恒：50 次启动逐次到齐核数之和 == 50×8 == 400，
//!    次数守恒（启动账行数 == 50）。
//! 3. 判定再现：原始判定（50/50 全过）必须由账面独立重算再现，
//!    不读旧结论只算新数；重算结论与落账结论等值。
//! 4. 核位图闭集：8 核位（bit0..bit7），bit8 以上为非法核位。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5e60 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5e60;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_ROUND_DUP: u16 = SEG_BASE + 1; // 启动轮次重复登记
const E_ROUND_FULL: u16 = SEG_BASE + 2; // 启动账已满
const E_ROUND_BAD: u16 = SEG_BASE + 3; // 轮次号越界（1..=50）
const E_BITMAP_BAD: u16 = SEG_BASE + 4; // 核位图越界（bit8 以上）
const E_BITMAP_ZERO: u16 = SEG_BASE + 5; // 核位图为 0（未采集）
const E_NOT_88: u16 = SEG_BASE + 6; // 非满 8/8（该次启动判不过）
const E_ROUNDS_LACK: u16 = SEG_BASE + 7; // 50 次未齐
const E_CONSERVE: u16 = SEG_BASE + 8; // 50 次守恒失败（核数和≠400）
const E_JUDGE_DRIFT: u16 = SEG_BASE + 9; // 判定再现失败（重算≠落账）
const E_BAL: u16 = SEG_BASE + 10; // 守恒恒等式破裂
const E_SEALED: u16 = SEG_BASE + 11; // 终核已封，拒绝再写
const E_EMPTY: u16 = SEG_BASE + 12; // 启动账为空
const E_CORES_BAD: u16 = SEG_BASE + 13; // 核数常量非法（内部错误）
const E_TWICE: u16 = SEG_BASE + 14; // 终核重复执行

/// 核数：8。
pub const CORES: u32 = 8;

/// 启动轮次：50。
pub const ROUNDS: usize = 50;

/// 满员位图：8/8。
pub const FULL_BITMAP: u8 = 0xFF;

/// 守恒分子：50×8 = 400。
pub const CONSERVE_TOTAL: u32 = ROUNDS as u32 * CORES;

/// 启动账定容（预留）。
pub const MAX_ROUNDS: usize = 64;

// ── FNV-1a 64（终核哈希口径）──────────────────────────────────
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
        E_ROUND_DUP => "启动轮次重复：50 轮各一行",
        E_ROUND_FULL => "启动账已满 64 行",
        E_ROUND_BAD => "轮次号越界：1..=50",
        E_BITMAP_BAD => "核位图越界：8 核位 bit0..bit7",
        E_BITMAP_ZERO => "核位图为 0：AP 到齐账未采集",
        E_NOT_88 => "非满 8/8：该次启动 AP 未到齐",
        E_ROUNDS_LACK => "50 次未齐：先补齐启动账",
        E_CONSERVE => "守恒失败：到齐核数之和必须等于 400",
        E_JUDGE_DRIFT => "判定再现失败：重算结论与落账不等值",
        E_BAL => "守恒破裂：计数账不平",
        E_SEALED => "终核已封：封账后只读",
        E_EMPTY => "启动账为空",
        E_CORES_BAD => "核数常量非法：内部口径损坏",
        E_TWICE => "终核重复执行",
        _ => "未知诊断码",
    }
}

// ── 位图计数 ─────────────────────────────────────────────────
/// 8 位位图计 1 数（逐位法，无查表）。
pub fn popcount8(b: u8) -> u32 {
    let mut n = 0u32;
    let mut x = b;
    let mut i = 0;
    while i < 8 {
        n += (x & 1) as u32;
        x >>= 1;
        i += 1;
    }
    n
}

// ── 启动账行 ─────────────────────────────────────────────────
/// 一次启动的 AP 到齐账行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootRound {
    /// 轮次号（1..=50）。
    pub round: u32,
    /// AP 到齐核位图（bit_i = 核 i AP 已到齐）。
    pub ap_bitmap: u8,
    /// 到齐核数（= popcount8(ap_bitmap)）。
    pub arrived: u32,
    /// 该次是否 8/8。
    pub full: bool,
}

// ── AP 到齐终核账（主体）────────────────────────────────────
/// 八核真机 AP 到齐终核：50 轮启动账 → 逐次核 → 守恒 → 判定再现。
pub struct ApArrivalFinal {
    rounds: [Option<BootRound>; MAX_ROUNDS],
    round_count: usize,
    /// 落账判定（50/50 全过）。
    posted_verdict: Option<bool>,
    /// 是否封账。
    sealed: bool,
    /// 终核哈希。
    sealed_hash: u64,
}

impl ApArrivalFinal {
    /// 开终核账。
    pub fn new() -> ApArrivalFinal {
        ApArrivalFinal {
            rounds: [None; MAX_ROUNDS],
            round_count: 0,
            posted_verdict: None,
            sealed: false,
            sealed_hash: 0,
        }
    }

    /// 登记启动轮：轮次号闭集 + 位图 8 位闭集 + 一轮一行。
    pub fn put_round(&mut self, round: u32, ap_bitmap: u8) -> Result<BootRound, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if round == 0 || round as usize > ROUNDS {
            return Err(E_ROUND_BAD);
        }
        if ap_bitmap == 0 {
            return Err(E_BITMAP_ZERO);
        }
        // 核位图闭集：bit8 以上必须为 0（u8 天然 8 位，显式核）。
        if ap_bitmap & !FULL_BITMAP != 0 {
            return Err(E_BITMAP_BAD);
        }
        let idx = (round - 1) as usize;
        if self.rounds[idx].is_some() {
            return Err(E_ROUND_DUP);
        }
        let arrived = popcount8(ap_bitmap);
        if arrived != CORES {
            return Err(E_CORES_BAD);
        }
        let row = BootRound {
            round,
            ap_bitmap,
            arrived,
            full: ap_bitmap == FULL_BITMAP,
        };
        self.rounds[idx] = Some(row);
        self.round_count += 1;
        Ok(row)
    }

    /// 轮次只读。
    pub fn get_round(&self, round: u32) -> Result<BootRound, u16> {
        if round == 0 || round as usize > ROUNDS {
            return Err(E_ROUND_BAD);
        }
        match &self.rounds[(round - 1) as usize] {
            Some(r) => Ok(*r),
            None => Err(E_ROUND_BAD),
        }
    }

    /// 已登记轮数。
    pub fn round_count(&self) -> usize {
        self.round_count
    }

    /// 逐次核：50 轮全满 8/8（账面重算）。
    pub fn per_round_assert(&self) -> Result<(), u16> {
        if self.round_count == 0 {
            return Err(E_EMPTY);
        }
        let mut r = 1u32;
        while r as usize <= ROUNDS {
            let row = self.get_round(r)?;
            if !row.full || row.arrived != CORES {
                return Err(E_NOT_88);
            }
            r += 1;
        }
        Ok(())
    }

    /// 轮次齐备：50 行全在。
    pub fn rounds_complete(&self) -> bool {
        self.round_count == ROUNDS
    }

    /// 守恒核：50 轮到齐核数之和 == 400。
    pub fn conserved(&self) -> bool {
        let mut s = 0u32;
        let mut r = 1u32;
        while r as usize <= ROUNDS {
            match self.get_round(r) {
                Ok(row) => s += row.arrived,
                Err(_) => return false,
            }
            r += 1;
        }
        s == CONSERVE_TOTAL
    }

    /// 判定再现：账面重算（全满 + 守恒）与落账判定等值。
    pub fn reproduce_assert(&self) -> Result<bool, u16> {
        if self.round_count == 0 {
            return Err(E_EMPTY);
        }
        if !self.rounds_complete() {
            return Err(E_ROUNDS_LACK);
        }
        // 落账判定必须在位。
        let posted = self.posted_verdict.ok_or(E_JUDGE_DRIFT)?;
        // 独立重算：逐次 8/8 且守恒 → 全过。
        let recomputed = match (self.per_round_assert(), self.conserved()) {
            (Ok(()), true) => true,
            (Ok(()), false) => false,
            (Err(E_NOT_88), _) => false,
            (Err(c), _) => return Err(c),
        };
        if recomputed != posted {
            return Err(E_JUDGE_DRIFT);
        }
        Ok(recomputed)
    }

    /// 落账判定（原始结论）。
    pub fn post_verdict(&mut self, all_pass: bool) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if !self.rounds_complete() {
            return Err(E_ROUNDS_LACK);
        }
        if self.posted_verdict.is_some() {
            return Err(E_TWICE);
        }
        self.posted_verdict = Some(all_pass);
        Ok(())
    }

    /// 终核总断言：50 轮齐 + 守恒 + 再现。
    pub fn final_assert(&self) -> Result<(), u16> {
        if self.round_count == 0 {
            return Err(E_EMPTY);
        }
        if !self.rounds_complete() {
            return Err(E_ROUNDS_LACK);
        }
        if !self.conserved() {
            return Err(E_CONSERVE);
        }
        self.reproduce_assert()?;
        Ok(())
    }

    /// 封核。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.final_assert()?;
        let mut buf: Vec<u8> = Vec::new();
        let mut r = 1u32;
        while r as usize <= ROUNDS {
            let row = self.get_round(r)?;
            buf.push(row.ap_bitmap);
            r += 1;
        }
        self.sealed_hash = fnv1a64(&buf);
        self.sealed = true;
        Ok(self.sealed_hash)
    }

    /// 终核哈希。
    pub fn sealed_hash(&self) -> u64 {
        self.sealed_hash
    }

    /// 到齐率千分值：守恒分子 / 满员分子（应恒 1000）。
    pub fn arrival_permille(&self) -> Result<u32, u16> {
        if self.round_count == 0 {
            return Err(E_EMPTY);
        }
        let mut s = 0u32;
        let mut r = 1u32;
        while r as usize <= self.round_count {
            s += self.get_round(r)?.arrived;
            r += 1;
        }
        Ok(s * 1000 / CONSERVE_TOTAL)
    }

    /// 终核报告行。
    pub fn report_lines(&self, sample: usize) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 八核真机 AP 到齐终核 · 50 次 ===".to_string());
        lines.push("轮次: ".to_string() + &itoa(self.round_count as u64) + "/50");
        lines.push("守恒: ".to_string()
            + if self.conserved() { "400/400" } else { "未平" });
        let step = if sample == 0 { 1 } else { sample };
        let mut r = 1u32;
        while r as usize <= ROUNDS {
            if r % step as u32 == 0 {
                if let Ok(row) = self.get_round(r) {
                    let mut line = String::from("R");
                    line.push_str(&itoa(row.round as u64));
                    line.push_str("|8/8=");
                    line.push_str(&itoa(row.arrived as u64));
                    line.push_str(if row.full { "|满员" } else { "|缺核" });
                    lines.push(line);
                }
            }
            r += 1;
        }
        lines
    }

    /// 逐核到齐计数：核 i 在 50 轮中到齐的次数（满员口径下每核 = 50）。
    pub fn core_tally(&self) -> [u32; 8] {
        let mut tally = [0u32; 8];
        let mut r = 1u32;
        while r as usize <= self.round_count {
            if let Ok(row) = self.get_round(r) {
                let mut c = 0usize;
                while c < 8 {
                    if row.ap_bitmap & (1u8 << c) != 0 {
                        tally[c] += 1;
                    }
                    c += 1;
                }
            }
            r += 1;
        }
        tally
    }

    /// 逐核均衡断言：八核到齐计数两两相等（满员口径 = 每核 50 次）。
    pub fn core_balance_assert(&self) -> Result<(), u16> {
        if self.round_count == 0 {
            return Err(E_EMPTY);
        }
        let tally = self.core_tally();
        let first = tally[0];
        let mut c = 1usize;
        while c < 8 {
            if tally[c] != first {
                return Err(E_BAL);
            }
            c += 1;
        }
        if self.rounds_complete() && first != ROUNDS as u32 {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 缺核轮清单：非满 8/8 的轮次号（升序）。
    pub fn deficit_rounds(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut r = 1u32;
        while r as usize <= self.round_count {
            if let Ok(row) = self.get_round(r) {
                if !row.full {
                    out.push(row.round);
                }
            }
            r += 1;
        }
        out
    }

    /// 相异位图数：50 轮中出现过的不同核位图种数（形态盘点）。
    pub fn distinct_bitmaps(&self) -> u32 {
        let mut seen = [false; 256];
        let mut n = 0u32;
        let mut r = 1u32;
        while r as usize <= self.round_count {
            if let Ok(row) = self.get_round(r) {
                let idx = row.ap_bitmap as usize;
                if !seen[idx] {
                    seen[idx] = true;
                    n += 1;
                }
            }
            r += 1;
        }
        n
    }

    /// 到齐核数直方图：arrived 值（0..=8）→ 轮数。
    pub fn arrival_histogram(&self) -> [u32; 9] {
        let mut hist = [0u32; 9];
        let mut r = 1u32;
        while r as usize <= self.round_count {
            if let Ok(row) = self.get_round(r) {
                if (row.arrived as usize) < 9 {
                    hist[row.arrived as usize] += 1;
                }
            }
            r += 1;
        }
        hist
    }

    /// 落账判定读取。
    pub fn verdict(&self) -> Option<bool> {
        self.posted_verdict
    }

    /// 满员轮计数：8/8 轮数（应恒 50）。
    pub fn full_round_count(&self) -> u32 {
        let mut n = 0u32;
        let mut r = 1u32;
        while r as usize <= self.round_count {
            if let Ok(row) = self.get_round(r) {
                if row.full {
                    n += 1;
                }
            }
            r += 1;
        }
        n
    }

    /// 单核视角账行：核 c 的到齐轮号清单（升序，逐核审计口）。
    pub fn core_rounds(&self, core: u32) -> Result<Vec<u32>, u16> {
        if core >= CORES {
            return Err(E_BITMAP_BAD);
        }
        let bit = 1u8 << core;
        let mut out: Vec<u32> = Vec::new();
        let mut r = 1u32;
        while r as usize <= self.round_count {
            if let Ok(row) = self.get_round(r) {
                if row.ap_bitmap & bit != 0 {
                    out.push(row.round);
                }
            }
            r += 1;
        }
        Ok(out)
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
/// 演练：50 轮 8/8 + 守恒 400 + 判定再现 + 封核 + 错误路径。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let mut af = ApArrivalFinal::new();
    // 50 轮全满（位图按轮次确定性变化但恒满员）。
    let mut r = 1u32;
    while r as usize <= ROUNDS {
        // 满员位图，逐轮异或 0 校验恒等（全 FF）。
        af.put_round(r, FULL_BITMAP)?;
        r += 1;
    }
    if af.round_count() != 50 {
        return Err(SEG_BASE);
    }
    af.per_round_assert()?;
    if !af.conserved() {
        return Err(SEG_BASE);
    }
    af.post_verdict(true)?;
    let reproduced = af.reproduce_assert()?;
    if !reproduced {
        return Err(SEG_BASE);
    }
    af.final_assert()?;
    if af.arrival_permille()? != 1000 {
        return Err(SEG_BASE);
    }
    // 取数口：逐核均衡、缺核清单、位图形态、直方图。
    af.core_balance_assert()?;
    let ct = af.core_tally();
    if ct[0] != 50 || ct[7] != 50 {
        return Err(SEG_BASE);
    }
    if !af.deficit_rounds().is_empty() {
        return Err(SEG_BASE);
    }
    if af.distinct_bitmaps() != 1 {
        return Err(SEG_BASE);
    }
    let hist = af.arrival_histogram();
    if hist[8] != 50 {
        return Err(SEG_BASE);
    }
    if af.verdict() != Some(true) {
        return Err(SEG_BASE);
    }
    if af.full_round_count() != 50 {
        return Err(SEG_BASE);
    }
    let cr = af.core_rounds(3)?;
    if cr.len() != 50 || cr[0] != 1 || cr[49] != 50 {
        return Err(SEG_BASE);
    }
    let h = af.seal()?;
    if h == 0 {
        return Err(SEG_BASE);
    }
    // 封核后写拒绝。
    match af.put_round(1, FULL_BITMAP) {
        Err(c) if c == E_SEALED || c == E_ROUND_DUP => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：缺核轮（7/8）→ per_round_assert 必拦。
    let mut af2 = ApArrivalFinal::new();
    let mut r = 1u32;
    while r as usize <= ROUNDS {
        let bmp = if r == 25 { 0b0111_1111 } else { FULL_BITMAP };
        let _ = af2.put_round(r, bmp)?;
        r += 1;
    }
    match af2.per_round_assert() {
        Err(c) if c == E_NOT_88 => {}
        _ => return Err(SEG_BASE),
    }
    // 判定再现：落账"全过"但重算"不过" → 漂移拦截。
    af2.post_verdict(true)?;
    match af2.reproduce_assert() {
        Err(c) if c == E_JUDGE_DRIFT => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：50 轮未齐。
    let mut af3 = ApArrivalFinal::new();
    let mut r = 1u32;
    while r as usize <= 49 {
        af3.put_round(r, FULL_BITMAP)?;
        r += 1;
    }
    match af3.post_verdict(true) {
        Err(c) if c == E_ROUNDS_LACK => {}
        _ => return Err(SEG_BASE),
    }
    // 位图计数单元核：popcount8(0b1011_0110) = 5。
    if popcount8(0b1011_0110) != 5 {
        return Err(SEG_BASE);
    }
    let rl = af.report_lines(10);
    if rl.len() != 8 {
        return Err(SEG_BASE);
    }
    Ok(0)
}
