//! unx36001_sidcore — UNX-F36001 SID 结构全语义（修订号/子权限计数/子权限数组/RID）。
//!
//! 判据原文：
//!   J1 结构逐字段与版本锚（ADR-UNX-008）对照 20 例全对；非法子权限计数
//!      （>15）注入 10 例构造期全部拒绝。
//! 设计纪律：
//!   1. 版本锚 ADR-UNX-008：修订号恒为 1；标识符颁发机构（IA）为 48 位
//!      无符号整数，按 6 字节大端语义存放；子权限为 32 位无符号数组，
//!      计数 ∈ [1,15]；RID 恒为最后一个子权限。
//!   2. 构造期全量校验：修订号非 1 拒、计数越界拒、零子权限拒——非法
//!      SID 永不进入账本（拒绝账留痕，不静默丢弃）。
//!   3. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

pub const SEG_BASE: u16 = 0x7270;
use crate::checks::CheckSet;

// ── 诊断码闭集（0x7270~0x728F，独占）─────────────────────────
const E_REV: u16 = SEG_BASE + 1; // 修订号非 1
const E_CNT_HI: u16 = SEG_BASE + 2; // 子权限计数 > 15
const E_CNT_LO: u16 = SEG_BASE + 3; // 子权限计数 == 0（至少要有 RID）
const E_ANCHOR: u16 = SEG_BASE + 4; // 版本锚逐字段对照失配
const E_LEDGER: u16 = SEG_BASE + 5; // 账本守恒破（构造账+拒绝账≠总请求账）
const E_SLOT: u16 = SEG_BASE + 6; // 账本槽位耗尽

/// 子权限容量上限（版本锚：15）。
pub const MAX_SUBS: usize = 15;
/// 版本锚修订号（恒 1）。
pub const ANCHOR_REV: u8 = 1;
/// 账本容量（构造账 + 拒绝账共用）。
pub const LEDGER_CAP: usize = 96;

/// SID 核心结构（版本锚 ADR-UNX-008 全字段）。
///
/// 字段序即布局语义锚：修订号 → 计数 → IA → 子权数组（有效前缀）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sid {
    pub revision: u8,             // 修订号，恒 1
    pub sub_count: u8,            // 子权限计数，∈ [1,15]
    pub id_authority: [u8; 6],    // IA，6 字节大端语义（[0] 为最高字节）
    pub sub_authorities: [u32; MAX_SUBS], // 有效前缀 sub_count 项，末项为 RID
}

impl Sid {
    /// 构造期全量校验（判据 J1 注入面：计数 >15 十例全拒）。
    pub fn new(revision: u8, ia: [u8; 6], subs: &[u32]) -> Result<Sid, u16> {
        if revision != ANCHOR_REV {
            return Err(E_REV);
        }
        if subs.len() > MAX_SUBS {
            return Err(E_CNT_HI);
        }
        if subs.is_empty() {
            return Err(E_CNT_LO);
        }
        let mut arr = [0u32; MAX_SUBS];
        let mut i = 0;
        while i < subs.len() {
            arr[i] = subs[i];
            i += 1;
        }
        Ok(Sid {
            revision,
            sub_count: subs.len() as u8,
            id_authority: ia,
            sub_authorities: arr,
        })
    }

    /// RID：恒为最后一个子权限（计数 ≥1 由构造期保证）。
    pub fn rid(&self) -> u32 {
        self.sub_authorities[(self.sub_count as usize) - 1]
    }

    /// 有效子权限切片。
    pub fn subs(&self) -> &[u32] {
        &self.sub_authorities[..self.sub_count as usize]
    }

    /// IA 的 48 位数值（大端组合）。
    pub fn ia_value(&self) -> u64 {
        let mut v: u64 = 0;
        let mut i = 0;
        while i < 6 {
            v = (v << 8) | self.id_authority[i] as u64;
            i += 1;
        }
        v
    }

    /// 从 48 位数值装 IA（大端拆分）。
    pub fn ia_from(v: u64) -> [u8; 6] {
        [
            ((v >> 40) & 0xFF) as u8,
            ((v >> 32) & 0xFF) as u8,
            ((v >> 24) & 0xFF) as u8,
            ((v >> 16) & 0xFF) as u8,
            ((v >> 8) & 0xFF) as u8,
            (v & 0xFF) as u8,
        ]
    }

    /// 结构指纹（FNV-1a 64，逐字段序：rev/count/IA/有效子权）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        h = (h ^ self.revision as u64).wrapping_mul(0x0100_0193);
        h = (h ^ self.sub_count as u64).wrapping_mul(0x0100_0193);
        let mut i = 0;
        while i < 6 {
            h = (h ^ self.id_authority[i] as u64).wrapping_mul(0x0100_0193);
            i += 1;
        }
        let mut j = 0;
        while j < self.sub_count as usize {
            h = (h ^ self.sub_authorities[j] as u64).wrapping_mul(0x0100_0193);
            j += 1;
        }
        h
    }
}

/// 账本行：构造账（成功）或拒绝账（失败），不灭失。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerRow {
    pub ok: bool,      // true=构造账，false=拒绝账
    pub rev: u8,       // 请求修订号
    pub cnt: usize,    // 请求子权限计数
    pub code: u16,     // 成功为 0，失败为诊断码
    pub fp: u64,       // 成功时为 SID 指纹，失败为 0
}

/// 构造账本：总请求账 = 构造账 + 拒绝账（守恒，判据面）。
pub struct SidLedger {
    rows: [Option<LedgerRow>; LEDGER_CAP],
    pub built: u32,     // 构造账
    pub rejected: u32,  // 拒绝账
    pub req_total: u32, // 总请求账
    pub dropped: u32,   // 槽位溢出显性计数（不静默）
}

impl SidLedger {
    pub fn new() -> SidLedger {
        SidLedger {
            rows: [None; LEDGER_CAP],
            built: 0,
            rejected: 0,
            req_total: 0,
            dropped: 0,
        }
    }

    /// 带账构造：任何请求必落一账（判据 J1 注入面账目化）。
    pub fn build(&mut self, revision: u8, ia: [u8; 6], subs: &[u32]) -> Result<Sid, u16> {
        self.req_total += 1;
        let slot_free = ((self.built + self.rejected) as usize) < LEDGER_CAP;
        match Sid::new(revision, ia, subs) {
            Ok(s) => {
                self.built += 1;
                if slot_free {
                    self.record(LedgerRow {
                        ok: true,
                        rev: revision,
                        cnt: subs.len(),
                        code: 0,
                        fp: s.fingerprint(),
                    });
                } else {
                    self.dropped += 1;
                }
                Ok(s)
            }
            Err(c) => {
                self.rejected += 1;
                if slot_free {
                    self.record(LedgerRow {
                        ok: false,
                        rev: revision,
                        cnt: subs.len(),
                        code: c,
                        fp: 0,
                    });
                } else {
                    self.dropped += 1;
                }
                Err(c)
            }
        }
    }

    fn record(&mut self, row: LedgerRow) {
        let mut i = 0;
        while i < LEDGER_CAP {
            if self.rows[i].is_none() {
                self.rows[i] = Some(row);
                return;
            }
            i += 1;
        }
    }

    /// 守恒核对：构造账 + 拒绝账 == 总请求账（判据面）。
    pub fn conserve(&self) -> bool {
        self.built + self.rejected == self.req_total
    }

    /// 账内指纹查找：构造账中指纹命中返回 true。
    pub fn has_fingerprint(&self, fp: u64) -> bool {
        let mut i = 0;
        while i < LEDGER_CAP {
            if let Some(r) = self.rows[i] {
                if r.ok && r.fp == fp {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 拒绝码统计：返回某诊断码的拒绝次数。
    pub fn rej_count_of(&self, code: u16) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < LEDGER_CAP {
            if let Some(r) = self.rows[i] {
                if !r.ok && r.code == code {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

/// 版本锚逐字段对照：给定 SID 与锚参数（20 例对照用）。
/// 修订号、计数、IA 值、RID 四字段全对才返回 true。
pub fn anchor_match(s: &Sid, rev: u8, ia_val: u64, cnt: usize, rid: u32) -> bool {
    s.revision == rev && s.ia_value() == ia_val && s.sub_count as usize == cnt && s.rid() == rid
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_REV => "修订号非 1（版本锚拒）",
        E_CNT_HI => "子权限计数 > 15（容量越界拒）",
        E_CNT_LO => "子权限计数为 0（无 RID 拒）",
        E_ANCHOR => "版本锚逐字段对照失配",
        E_LEDGER => "账本守恒破（构造+拒绝≠总请求）",
        E_SLOT => "账本槽位耗尽（显性丢弃计数）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：20 例锚对照 + 10 例非法计数注入 + 账目守恒（判据 J1）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut led = SidLedger::new();

    // R1：20 例结构逐字段锚对照（覆盖 1~15 全计数档 + 特征 IA/RID 组合）。
    let mut k = 0u32;
    while k < 20 {
        let cnt = (k as usize % MAX_SUBS) + 1;
        let ia_val = 0x0000_0005_0000_0021u64 + k as u64; // S-1-5-21 族特征
        let ia = Sid::ia_from(ia_val);
        let mut subs = [0u32; MAX_SUBS];
        let mut i = 0;
        while i < cnt {
            subs[i] = 100 + i as u32;
            i += 1;
        }
        let s = led.build(1, ia, &subs[..cnt])?;
        if !anchor_match(&s, 1, ia_val, cnt, subs[cnt - 1]) {
            return Err(E_ANCHOR);
        }
        if s.fingerprint() == 0 {
            return Err(E_ANCHOR);
        }
        k += 1;
    }
    hits += 1;

    // R2：非法子权限计数注入 10 例（16~25），构造期全拒且入拒绝账。
    // 背衬数组取 26 长（>MAX_SUBS），切片越界本身即注入样本。
    let mut bad = 16usize;
    while bad <= 25 {
        let mut big = [0u32; 26];
        let mut i = 0;
        while i < bad {
            big[i] = i as u32 + 1;
            i += 1;
        }
        if led.build(1, Sid::ia_from(5), &big[..bad]) != Err(E_CNT_HI) {
            return Err(E_CNT_HI);
        }
        bad += 1;
    }
    if led.rej_count_of(E_CNT_HI) != 10 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3：修订号注入（0/2/255）与零子权限注入各拒。
    if led.build(0, Sid::ia_from(5), &[1]) != Err(E_REV) {
        return Err(E_REV);
    }
    if led.build(2, Sid::ia_from(5), &[1]) != Err(E_REV) {
        return Err(E_REV);
    }
    if led.build(255, Sid::ia_from(5), &[1]) != Err(E_REV) {
        return Err(E_REV);
    }
    if led.build(1, Sid::ia_from(5), &[]) != Err(E_CNT_LO) {
        return Err(E_CNT_LO);
    }
    hits += 1;

    // R4：RID 语义——末子权恒为 RID；IA 48 位大端往返。
    let s = led.build(1, Sid::ia_from(0x0000_0005_0000_0021), &[21, 100, 1000])?;
    if s.rid() != 1000 || s.ia_value() != 0x0000_0005_0000_0021 {
        return Err(E_ANCHOR);
    }
    if !led.has_fingerprint(s.fingerprint()) {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R5：账目守恒——构造 21（20+1）+ 拒绝 14（10+3+1）== 总请求 35。
    if !led.conserve() {
        return Err(E_LEDGER);
    }
    if led.built != 21 || led.rejected != 14 || led.req_total != 35 {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}

// ── 判据聚合（CheckSet 面 · 进 unxstar_blocks 检查表）────────
/// 判据 J1 聚合入口：SEG 锚 + 确定性演练全绿（红时回填诊断码）。
pub fn run_unx36001_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36001_sidcore");
    cs.add("SEG 段位独占", SEG_BASE == 0x7270, "0x7270~0x728F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }
}
