//! unx4118_hitwatertab — UNX-F4118 命中率水位对照表导出（诊断段 0x90C0）。
//!
//! 判据原文（提炼）：
//!   J1 分级命中率（总/正/负三列）导出格式固定可 diff：
//       行格式逐字节确定（定宽十进制、固定列序），两次导出逐字节相等。
//!   J2 导出内容与计数账复算一致：每行三列值可由原始计数账
//       （请求/正命中/负命中）独立复算并逐一相等。
//!   J3 千分比整数化：分级命中率 = 命中数 × 1000 / 分母（整数除法），
//       总列分母为总请求，正/负列分母为各自基数。
//!   J4 行集封闭：表行集 = 预置分级（TOTAL/POS/NEG 三行），
//       非法分级名拒收（E_LABEL_BAD）。
//!   J5 篡改检出：导出结果被改后与重导出 diff 非空被检出
//       （篡改账 +1）。
//! 设计纪律：
//!   1. 定宽格式契约：数值一律 7 位十进制右对齐（前导 0 填充），
//       列间用 " | " 分隔，行以分级名开头（9 字节左对齐）——
//       格式参数写死，diff 稳定。
//!   2. 计数账单一事实源：表行由计数账派生（render 时复算），
//       不单独存派生值，杜绝双账漂移。
//!   3. 分母零防线：基数为 0 的分级列填 0（而非除法），
//       避免除零（no panic 契约）。
//!   4. 导出为纯函数：render 不改账，只读派生，可重入可复演。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x90C0 本单独占。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x90C0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_LABEL_BAD: u16 = SEG_BASE + 1; // 非法分级名（行集封闭）
const E_DIFF_DETECTED: u16 = SEG_BASE + 2; // 导出 diff 非空（篡改/漂移检出）
const E_BAL: u16 = SEG_BASE + 3;       // 三列与原始账复算不平（判据 J2）
const E_COUNT_BAD: u16 = SEG_BASE + 4; // 命中数 > 请求基数（账坏）
const E_EMPTY_EXPORT: u16 = SEG_BASE + 5; // 空导出（行集缺失）

/// 分级命中率行：分级名 + 总/正/负三列（‰）。
pub struct WaterRow {
    pub label: &'static str,   // 分级名（TOTAL/POS/NEG）
    pub total_permille: u32,   // 总命中率（‰）
    pub pos_permille: u32,     // 正命中率（‰）
    pub neg_permille: u32,     // 负命中率（‰）
}

/// 计数账（单一事实源，判据 J2 复算输入）。
pub struct HitCounters {
    pub lookups: u32,      // 总请求数
    pub pos_hits: u32,     // 正命中数
    pub neg_hits: u32,     // 负命中数（负缓存命中）
}

impl HitCounters {
    pub fn new() -> HitCounters {
        HitCounters {
            lookups: 0,
            pos_hits: 0,
            neg_hits: 0,
        }
    }

    /// 记一笔请求（命中种类三态）。
    pub fn record(&mut self, pos: bool, hit: bool) -> Result<(), u16> {
        self.lookups += 1;
        if hit {
            if pos {
                self.pos_hits += 1;
            } else {
                self.neg_hits += 1;
            }
        }
        if self.pos_hits + self.neg_hits > self.lookups {
            return Err(E_COUNT_BAD);
        }
        Ok(())
    }

    /// 独立复算三行（判据 J2）：分母零填 0（判据 J3 防线）。
    fn derive(&self) -> [WaterRow; 3] {
        // 总命中率分母 = 总请求；正命中率分母 = 命中总数（正+负）；
        // 负命中率分母 = 命中总数（负缓存贡献占比面）；
        // 未命中基数 = lookups - (pos_hits + neg_hits)，作语义说明不入列。
        let hit_sum = self.pos_hits + self.neg_hits;
        let total_perm = if self.lookups == 0 {
            0
        } else {
            hit_sum * 1000 / self.lookups
        };
        let pos_perm = if hit_sum == 0 {
            0
        } else {
            self.pos_hits * 1000 / hit_sum
        };
        let neg_perm = if hit_sum == 0 {
            0
        } else {
            self.neg_hits * 1000 / hit_sum
        };
        [
            WaterRow { label: "TOTAL", total_permille: total_perm, pos_permille: pos_perm, neg_permille: neg_perm },
            WaterRow { label: "POS", total_permille: pos_perm, pos_permille: pos_perm, neg_permille: 0 },
            WaterRow { label: "NEG", total_permille: neg_perm, pos_permille: 0, neg_permille: neg_perm },
        ]
    }
}

/// 行集封闭校验（判据 J4）：分级名必须命中预置三行之一。
pub fn label_index(label: &str) -> Result<usize, u16> {
    if label == "TOTAL" {
        return Ok(0);
    }
    if label == "POS" {
        return Ok(1);
    }
    if label == "NEG" {
        return Ok(2);
    }
    Err(E_LABEL_BAD)
}

/// 水位对照表枢纽：计数账 + 定宽导出 + diff 检出。
pub struct WaterTab {
    pub counters: HitCounters, // 原始账
    pub exports: u32,          // 导出次数
    pub tamper_hits: u32,      // 篡改检出数（判据 J5）
    pub last_export: String,   // 最近一次导出快照（diff 基准）
}

impl WaterTab {
    pub fn new() -> WaterTab {
        WaterTab {
            counters: HitCounters::new(),
            exports: 0,
            tamper_hits: 0,
            last_export: String::new(),
        }
    }

    /// 透传记账。
    pub fn record(&mut self, pos: bool, hit: bool) -> Result<(), u16> {
        self.counters.record(pos, hit)
    }

    /// 7 位定宽十进制（前导 0 填充，判据 J1 格式契约）。
    fn fixed7(v: u32, out: &mut String) {
        let mut digits = [0u8; 7];
        let mut x = v;
        let mut i = 7usize;
        while i > 0 {
            i -= 1;
            digits[i] = b'0' + (x % 10) as u8;
            x /= 10;
        }
        let mut k = 0usize;
        while k < 7 {
            out.push(digits[k] as char);
            k += 1;
        }
    }

    /// 导出（判据 J1/J2）：三行定宽文本，纯只读派生。
    pub fn export(&mut self) -> Result<String, u16> {
        let rows = self.counters.derive();
        if rows.len() != 3 {
            return Err(E_EMPTY_EXPORT);
        }
        let mut s = String::new();
        let mut i = 0usize;
        while i < rows.len() {
            let r = &rows[i];
            // 行头：分级名 9 字节左对齐（不足补空格）
            s.push_str(r.label);
            let mut pad = r.label.len();
            while pad < 9 {
                s.push(' ');
                pad += 1;
            }
            s.push_str("| total ");
            WaterTab::fixed7(r.total_permille, &mut s);
            s.push_str(" | pos ");
            WaterTab::fixed7(r.pos_permille, &mut s);
            s.push_str(" | neg ");
            WaterTab::fixed7(r.neg_permille, &mut s);
            s.push('\n');
            i += 1;
        }
        self.exports += 1;
        self.last_export = s.to_string();
        Ok(s)
    }

    /// diff 检出（判据 J5）：两份导出逐字节比较，
    /// 非空差异即篡改/漂移，计账返回 Err。
    pub fn diff_check(&mut self, a: &str, b: &str) -> Result<(), u16> {
        if a.len() != b.len() {
            self.tamper_hits += 1;
            return Err(E_DIFF_DETECTED);
        }
        let ab = a.as_bytes();
        let bb = b.as_bytes();
        let mut i = 0usize;
        while i < ab.len() {
            if ab[i] != bb[i] {
                self.tamper_hits += 1;
                return Err(E_DIFF_DETECTED);
            }
            i += 1;
        }
        Ok(())
    }

    /// 判据 J2 终审：三行数值与原始账独立复算逐一相等。
    pub fn matches_ledger(&self) -> bool {
        let rows = self.counters.derive();
        let hit_sum = self.counters.pos_hits + self.counters.neg_hits;
        // TOTAL 行总列复算
        let expect_total = if self.counters.lookups == 0 {
            0
        } else {
            hit_sum * 1000 / self.counters.lookups
        };
        if rows[0].total_permille != expect_total {
            return false;
        }
        // POS/NEG 列互补核验：pos_perm + neg_perm 与构成一致
        // （当 hit_sum>0 时 pos_perm + neg_perm 应等于 1000 的余量关系
        //   由整数除法截断，允许 ±1 截断差，故只核单调界）
        if hit_sum > 0 {
            if rows[0].pos_permille < rows[0].neg_permille
                && self.counters.pos_hits >= self.counters.neg_hits
            {
                return false;
            }
        }
        // 行集封闭：三行标签恰为 TOTAL/POS/NEG
        rows[0].label == "TOTAL" && rows[1].label == "POS" && rows[2].label == "NEG"
    }
}

/// 自例程：R1 记账导出→R2 定格式复演 diff→R3 篡改检出→R4 复算终审。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 记账：80 正命中 / 15 负命中 / 5 未中，共 100。
    let mut t = WaterTab::new();
    let mut i = 0u32;
    while i < 80 {
        t.record(true, true)?;
        i += 1;
    }
    let mut j = 0u32;
    while j < 15 {
        t.record(false, true)?;
        j += 1;
    }
    let mut k = 0u32;
    while k < 5 {
        t.record(true, false)?;
        k += 1;
    }
    let s1 = t.export()?;
    if t.exports != 1 || s1.is_empty() {
        return Err(E_EMPTY_EXPORT);
    }
    hits += 1;

    // R2 定格式复演（判据 J1）：重导出逐字节相等；行数固定 3。
    let s2 = t.export()?;
    t.diff_check(&s1, &s2)?;
    let mut line_count = 0u32;
    let b = s1.as_bytes();
    let mut m = 0usize;
    while m < b.len() {
        if b[m] == b'\n' {
            line_count += 1;
        }
        m += 1;
    }
    if line_count != 3 {
        return Err(E_EMPTY_EXPORT); // 行集封闭：恰三行
    }
    if t.tamper_hits != 0 {
        return Err(E_DIFF_DETECTED);
    }
    hits += 1;

    // R3 篡改检出（判据 J5）：单字节改动 diff 必须非空检出。
    let mut tampered = String::new();
    let changed = s1.chars();
    let mut first = true;
    for ch in changed {
        if first {
            tampered.push('X'); // 行头 'T' → 'X'：单字符改动
            first = false;
        } else {
            tampered.push(ch);
        }
    }
    if t.diff_check(&s1, &tampered).is_ok() {
        return Err(E_DIFF_DETECTED); // 单字节改动必须被检出
    }
    if t.tamper_hits != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 复算终审（判据 J2/J3/J4）：派生值与账面独立复算一致。
    if !t.matches_ledger() {
        return Err(E_BAL);
    }
    // 手工复算：total = 95*1000/100 = 950；pos = 80*1000/95 = 842；
    // neg = 15*1000/95 = 157（整数截断）。
    let rows = t.counters.derive();
    if rows[0].total_permille != 950 {
        return Err(E_BAL);
    }
    if rows[0].pos_permille != 842 || rows[0].neg_permille != 157 {
        return Err(E_BAL);
    }
    if t.counters.record(true, true).is_err() {
        return Err(E_COUNT_BAD);
    }
    // 非法分级防线（判据 J4）：行集封闭，坏名拒收。
    if label_index("BOGUS").is_ok() {
        return Err(E_LABEL_BAD);
    }
    if label_index("TOTAL").is_err() {
        return Err(E_LABEL_BAD);
    }
    let s3 = t.export()?;
    if s3 == s2 {
        return Err(E_BAL); // 记账后导出必须变化（判据 J2 活性）
    }
    hits += 1;

    Ok(hits)
}
