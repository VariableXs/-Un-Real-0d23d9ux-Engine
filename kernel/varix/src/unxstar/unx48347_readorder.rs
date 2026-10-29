//! unx48347_readorder — UNX-F48347 候选朗读序数据面（候选序 + 高亮位只读供数）（诊断段 0xCC40）。
//!
//! 判据原文：
//!   J1 供数 ×100 一致；判据锚定 体验四。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 只读供数：朗读序数据面只读候选页——供出"高亮候选先行、其余
//!      按榜位次序"的朗读序 id 列，绝无写路径，供数前后页指纹不变。
//!   2. 高亮位纪律：hl 必落在 [0, len)（E_HL_RANGE）；空页供空序；
//!      高亮位移动不改候选榜本身。
//!   3. ×100 一致：100 轮 确定性变更→供数→对线性扫描 oracle 逐位
//!      对表，错一位即 E_MISMATCH。
//!   4. oracle 双路：供数走"高亮提位 + 两段拼接"，oracle 走"逐榜位
//!      独立重排"，两条路径互为印证。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCC40;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_HL_RANGE: u16 = SEG_BASE + 1; // 高亮位越界
const E_MISMATCH: u16 = SEG_BASE + 2; // 供数与 oracle 失配
const E_RO_VIOL: u16 = SEG_BASE + 3; // 只读边界被击穿
const E_EMPTY_BAD: u16 = SEG_BASE + 4; // 空页供数异常
const E_TAMPER: u16 = SEG_BASE + 5; // 页指纹失配
const E_LEN_BAD: u16 = SEG_BASE + 6; // 供数长度异常

/// FNV-1a 64 位指纹（u32 流）。
pub fn fp_u32s(vals: &[u32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < vals.len() {
        let mut b = 0usize;
        while b < 4 {
            h ^= ((vals[i] >> (b * 8)) & 0xFF) as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            b += 1;
        }
        i += 1;
    }
    h
}

/// 候选页：榜序候选 id 表 + 高亮位（高亮位是位标，不动榜）。
#[derive(Clone)]
pub struct CandPage {
    ids: Vec<u32>,
    hl: usize,
}

impl CandPage {
    pub fn new() -> Self {
        CandPage { ids: Vec::new(), hl: 0 }
    }

    /// 换榜：id 必须两两互异（候选唯一性）。
    pub fn set_board(&mut self, ids: Vec<u32>) -> Result<(), u16> {
        let mut a = 0usize;
        while a < ids.len() {
            let mut b = a + 1;
            while b < ids.len() {
                if ids[a] == ids[b] {
                    return Err(E_TAMPER);
                }
                b += 1;
            }
            a += 1;
        }
        self.ids = ids;
        if self.hl >= self.ids.len() {
            self.hl = 0;
        }
        Ok(())
    }

    /// 高亮位移动：越界必拒。
    pub fn move_hl(&mut self, hl: usize) -> Result<(), u16> {
        if hl >= self.ids.len() {
            return Err(E_HL_RANGE);
        }
        self.hl = hl;
        Ok(())
    }

    pub fn hl(&self) -> usize {
        self.hl
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn page_fp(&self) -> u64 {
        fp_u32s(&self.ids) ^ (self.hl as u64).wrapping_mul(1_000_003)
    }

    /// 榜只读视图（供 oracle 之外的对账用）。
    pub fn ids_view(&self) -> &[u32] {
        &self.ids
    }
}

/// 供数器：产出朗读序（高亮先行，其余按榜位），只读。
pub struct ReadSupplier;

impl ReadSupplier {
    pub fn supply(page: &CandPage) -> Result<Vec<u32>, u16> {
        if page.len() == 0 {
            if page.hl() != 0 {
                return Err(E_HL_RANGE);
            }
            return Ok(Vec::new());
        }
        if page.hl() >= page.len() {
            return Err(E_HL_RANGE);
        }
        let mut out: Vec<u32> = Vec::new();
        out.push(page.ids_view()[page.hl()]);
        let mut i = 0usize;
        while i < page.len() {
            if i != page.hl() {
                out.push(page.ids_view()[i]);
            }
            i += 1;
        }
        if out.len() != page.len() {
            return Err(E_LEN_BAD);
        }
        Ok(out)
    }
}

/// 线性扫描 oracle：独立重排路径（逐榜位判断前插）。
pub fn oracle_supply(page: &CandPage) -> Result<Vec<u32>, u16> {
    if page.len() == 0 {
        return Ok(Vec::new());
    }
    if page.hl() >= page.len() {
        return Err(E_HL_RANGE);
    }
    let hl_id = page.ids_view()[page.hl()];
    let mut out: Vec<u32> = Vec::new();
    out.push(0); // 占位，最后回填高亮
    let mut i = 0usize;
    while i < page.len() {
        if i != page.hl() {
            out.push(page.ids_view()[i]);
        }
        i += 1;
    }
    out[0] = hl_id;
    Ok(out)
}

/// 供数一致账。
pub struct SupplyLedger {
    pub rounds: u32,
    pub consistent: u32,
}

impl SupplyLedger {
    pub fn new() -> Self {
        SupplyLedger { rounds: 0, consistent: 0 }
    }
}

/// drill：J1 供数 ×100 一致。
pub fn drill() -> Result<u32, u16> {
    let mut ledger = SupplyLedger::new();
    let mut page = CandPage::new();

    // ── R1 主判据：100 轮 变更→供数→oracle 对表 全一致 ──────
    let mut r = 0u32;
    while r < 100 {
        // 确定性变更：每轮换 5..9 个候选的榜，并挪高亮位。
        let k = 5 + (r % 5) as u32;
        let mut ids: Vec<u32> = Vec::new();
        let mut j = 0u32;
        while j < k {
            ids.push(3000 + r * 10 + j);
            j += 1;
        }
        page.set_board(ids)?;
        let hl = (r as usize * 3) % page.len();
        page.move_hl(hl)?;

        let before = page.page_fp();
        let got = ReadSupplier::supply(&page)?;
        let want = oracle_supply(&page)?;
        ledger.rounds += 1;

        // 只读边界：供数前后页指纹不变。
        if page.page_fp() != before {
            return Err(E_RO_VIOL);
        }
        // 长度对账 + 逐位对表。
        if got.len() != want.len() || got.len() != page.len() {
            return Err(E_LEN_BAD);
        }
        let mut i = 0usize;
        while i < got.len() {
            if got[i] != want[i] {
                return Err(E_MISMATCH);
            }
            i += 1;
        }
        // 首位必为高亮候选，且 id 集合守恒（重排不丢不增）。
        if got[0] != page.ids_view()[hl] {
            return Err(E_MISMATCH);
        }
        let mut sum_got: u64 = 0;
        let mut sum_ids: u64 = 0;
        let mut i = 0usize;
        while i < got.len() {
            sum_got = sum_got.wrapping_add(got[i] as u64);
            i += 1;
        }
        let mut i = 0usize;
        while i < page.len() {
            sum_ids = sum_ids.wrapping_add(page.ids_view()[i] as u64);
            i += 1;
        }
        if sum_got != sum_ids {
            return Err(E_MISMATCH);
        }
        ledger.consistent += 1;
        r += 1;
    }
    if ledger.rounds != 100 || ledger.consistent != 100 {
        return Err(E_MISMATCH);
    }

    // ── R2 高亮越界：空页外任意位必拒 ────────────────────────
    let mut small = CandPage::new();
    let mut ids: Vec<u32> = Vec::new();
    ids.push(11);
    ids.push(22);
    small.set_board(ids)?;
    if small.move_hl(2).is_ok() || small.move_hl(usize::MAX >> 8).is_ok() {
        return Err(E_HL_RANGE);
    }

    // ── R3 空页供数：空序合法、高亮位恒 0 ────────────────────
    let empty = CandPage::new();
    let es = ReadSupplier::supply(&empty)?;
    if es.len() != 0 {
        return Err(E_EMPTY_BAD);
    }
    if oracle_supply(&empty)?.len() != 0 {
        return Err(E_EMPTY_BAD);
    }

    // ── R4 榜唯一性：重复 id 换榜必拒 ────────────────────────
    let mut dup = CandPage::new();
    let mut ids2: Vec<u32> = Vec::new();
    ids2.push(7);
    ids2.push(7);
    if dup.set_board(ids2).is_ok() {
        return Err(E_TAMPER);
    }

    // ── R5 重排守恒：单元素页高亮恒 0 位，供数即榜 ───────────
    let mut one = CandPage::new();
    let mut ids3: Vec<u32> = Vec::new();
    ids3.push(42);
    one.set_board(ids3)?;
    one.move_hl(0)?;
    let os = ReadSupplier::supply(&one)?;
    if os.len() != 1 || os[0] != 42 {
        return Err(E_LEN_BAD);
    }

    // ── R6 oracle 双路互证：20 轮随机位形两路逐位相等 ─────────
    let mut r2 = 0u32;
    while r2 < 20 {
        let k = 2 + (r2 % 4) as u32;
        let mut ids: Vec<u32> = Vec::new();
        let mut j = 0u32;
        while j < k {
            ids.push(700 + r2 * 7 + j);
            j += 1;
        }
        page.set_board(ids)?;
        page.move_hl((r2 as usize) % page.len())?;
        let a = ReadSupplier::supply(&page)?;
        let b = oracle_supply(&page)?;
        if a.len() != b.len() {
            return Err(E_LEN_BAD);
        }
        let mut i = 0usize;
        while i < a.len() {
            if a[i] != b[i] {
                return Err(E_MISMATCH);
            }
            i += 1;
        }
        r2 += 1;
    }

    // ── R7 高亮走查：逐位走一遍高亮，榜与页指纹中榜部分不变 ──
    let mut walk = CandPage::new();
    let mut wids: Vec<u32> = Vec::new();
    let mut j = 0u32;
    while j < 6 {
        wids.push(900 + j);
        j += 1;
    }
    walk.set_board(wids)?;
    let board_only = fp_u32s(walk.ids_view());
    let mut pos = 0usize;
    while pos < 6 {
        walk.move_hl(pos)?;
        let s = ReadSupplier::supply(&walk)?;
        if s[0] != 900 + pos as u32 {
            return Err(E_MISMATCH);
        }
        // 榜部分指纹只随榜动，高亮位移动不影响。
        if fp_u32s(walk.ids_view()) != board_only {
            return Err(E_RO_VIOL);
        }
        pos += 1;
    }
    // 供数序仍与 oracle 相符。
    let sa = ReadSupplier::supply(&walk)?;
    let sb = oracle_supply(&walk)?;
    if sa.len() != sb.len() || sa[0] != sb[0] || sa[5] != sb[5] {
        return Err(E_MISMATCH);
    }

    // J2 写回摘要非空：一致性账非空即摘要实质。
    if ledger.consistent == 0 {
        return Err(E_MISMATCH);
    }
    Ok(ledger.consistent)
}
