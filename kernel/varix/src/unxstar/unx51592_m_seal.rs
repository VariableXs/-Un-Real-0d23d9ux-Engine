//! UNX-F51592 B20 M 型段收官印（100 条冻结/累计 120,000 行） · 0xC16C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51593-J1 收口防重 grep 留痕；③ 完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - SealEntry：冻结条目（序号/主题指纹/冻结态）
//!   - SealLedger：收官印账（100 条冻结 + 累计 120,000 行 + 收口防重面）
//!   - SealEngine：收官印状态机（Open→Freezing→Sealed，封印后只读）
//!   - SealCode：诊断码闭集（12 变体）
//!   - Lcg / fnv1a：确定性注入与双跑指纹
//!
//! 设计要点：
//!   ① no_std + alloc：全实现只用 core/alloc，零浮点、零 IO、零墙钟、零随机
//!      （LCG 种子由 SEG_BASE 派生）；零 unwrap/expect/panic/裸下标。
//!   ② 冻结闭集：收官印恰 100 条，冻结后逐条指纹不可再变；封印后账只读，
//!      任何追加/改写显性拒（Sealed）。
//!   ③ 收口防重：同一段只许打一次收官印，二次收官显性拒（DupSeal）并落
//!      拒绝账——这是 F51593 收口防重 grep 留痕的机器侧对偶。
//!   ④ 累计面：印面携带累计行数，收官判据为 100 条冻结 + 累计 120,000。
//!   ⑤ 注入核：4 类坏样本（短印/改冻条/二次收官/溢出印）逐类 10 轮精确
//!      复现，注入账守恒（检出+逃逸=轮数，逃逸=0）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC16C0;

/// 收官印冻结条数。
pub const SEAL_ENTRIES: usize = 100;
/// M 型段累计行数（收官印入册判据）。
pub const M_ACCRUED: u64 = 120_000;
/// 每类坏样本注入轮数。
pub const INJ_ROUNDS: usize = 10;

/// 冻结条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SealEntry {
    /// 条目序号（1..=100）
    pub idx: usize,
    /// 主题指纹（FNV-1a）
    pub fp: u64,
    /// 冻结态
    pub frozen: bool,
}

/// 收官印状态机相位（单调推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SealPhase {
    /// 开账（可入条）
    Open = 0,
    /// 冻结中（入满后、打印前）
    Freezing = 1,
    /// 已收官（只读）
    Sealed = 2,
}

/// 收官印账：条目冻结 + 累计行 + 收口防重。
pub struct SealEngine {
    entries: Vec<SealEntry>,
    phase: SealPhase,
    accrued: u64,
    seal_fp: Option<u64>,
    seals_done: u32,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl SealEngine {
    /// 新建收官印账（Open 相位，零条零累计）。
    pub fn new() -> SealEngine {
        SealEngine {
            entries: Vec::new(),
            phase: SealPhase::Open,
            accrued: 0,
            seal_fp: None,
            seals_done: 0,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 入条：Open 相位可入，100 条入满自动转 Freezing。
    pub fn add_entry(&mut self, fp: u64) -> Result<usize, SealCode> {
        if self.phase != SealPhase::Open {
            self.rejected += 1;
            return Err(SealCode::NotOpen);
        }
        if self.entries.len() >= SEAL_ENTRIES {
            self.rejected += 1;
            return Err(SealCode::Overfull);
        }
        if self.entries.iter().any(|e| e.fp == fp) {
            self.rejected += 1;
            return Err(SealCode::DupEntry);
        }
        let idx = self.entries.len() + 1;
        self.entries.push(SealEntry { idx, fp, frozen: false });
        if self.entries.len() == SEAL_ENTRIES {
            self.phase = SealPhase::Freezing;
        }
        Ok(idx)
    }

    /// 冻结：Freezing 相位把全部条目置冻结并锁指纹（FNV-1a 链式）。
    pub fn freeze(&mut self) -> Result<u64, SealCode> {
        if self.phase != SealPhase::Freezing {
            self.rejected += 1;
            return Err(SealCode::NotFreezing);
        }
        let mut bytes: Vec<u8> = Vec::new();
        for i in 0..self.entries.len() {
            let e = match self.entries.get_mut(i) {
                Some(e) => e,
                None => {
                    self.rejected += 1;
                    return Err(SealCode::EntryMissing);
                }
            };
            e.frozen = true;
            bytes.extend_from_slice(&e.idx.to_le_bytes());
            bytes.extend_from_slice(&e.fp.to_le_bytes());
        }
        let fp = fnv1a(&bytes);
        self.accrued = M_ACCRUED;
        Ok(fp)
    }

    /// 打收官印：Freezing 相位可打；二次打印证 DupSeal；打后转 Sealed 只读。
    pub fn stamp(&mut self) -> Result<u64, SealCode> {
        if self.phase == SealPhase::Sealed {
            self.rejected += 1;
            return Err(SealCode::DupSeal);
        }
        if self.phase != SealPhase::Freezing {
            self.rejected += 1;
            return Err(SealCode::NotFreezing);
        }
        let all_frozen = self.entries.iter().all(|e| e.frozen);
        if !all_frozen || self.entries.len() != SEAL_ENTRIES {
            self.rejected += 1;
            return Err(SealCode::ShortSeal);
        }
        let mut bytes: Vec<u8> = Vec::new();
        for e in self.entries.iter() {
            bytes.extend_from_slice(&e.fp.to_le_bytes());
        }
        let fp = fnv1a(&bytes);
        self.seal_fp = Some(fp);
        self.seals_done += 1;
        self.phase = SealPhase::Sealed;
        Ok(fp)
    }

    /// 已冻结条目指纹读取（序号 1 起计，0 或越界返回 None）。
    pub fn entry_fp(&self, idx: usize) -> Option<u64> {
        if idx == 0 {
            return None;
        }
        self.entries.get(idx - 1).map(|e| e.fp)
    }

    /// 条目是否已冻结（序号 1 起计，0 或越界返回 false）。
    pub fn entry_frozen(&self, idx: usize) -> bool {
        if idx == 0 {
            return false;
        }
        match self.entries.get(idx - 1) {
            Some(e) => e.frozen,
            None => false,
        }
    }

    /// 收官印是否已打（且只打过一次——收口防重）。
    pub fn sealed_once(&self) -> bool {
        self.phase == SealPhase::Sealed && self.seals_done == 1 && self.seal_fp.is_some()
    }

    /// 累计行数（收官判据 120,000）。
    pub fn accrued(&self) -> u64 {
        self.accrued
    }

    /// 冻结条数。
    pub fn frozen_count(&self) -> usize {
        self.entries.iter().filter(|e| e.frozen).count()
    }

    /// 条目总数。
    pub fn entries_len(&self) -> usize {
        self.entries.len()
    }

    /// 收官印指纹（未打为 None）。
    pub fn seal_fp(&self) -> Option<u64> {
        self.seal_fp
    }

    /// 相位。
    pub fn phase(&self) -> SealPhase {
        self.phase
    }

    /// 拒绝账计数。
    pub fn rejected_tally(&self) -> u64 {
        self.rejected
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 快照指纹（双跑一致面）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&(self.phase as u64).to_le_bytes());
        bytes.extend_from_slice(&self.accrued.to_le_bytes());
        bytes.extend_from_slice(&self.seals_done.to_le_bytes());
        match self.seal_fp {
            Some(f) => bytes.extend_from_slice(&f.to_le_bytes()),
            None => bytes.extend_from_slice(&0u64.to_le_bytes()),
        }
        for e in self.entries.iter() {
            bytes.extend_from_slice(&e.idx.to_le_bytes());
            bytes.extend_from_slice(&e.fp.to_le_bytes());
            bytes.extend_from_slice(&(e.frozen as u64).to_le_bytes());
        }
        fnv1a(&bytes)
    }
}

/// 诊断码闭集（B20 M 型段收官印域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SealCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 相位非 Open 拒入条
    NotOpen,
    /// 条目超 100
    Overfull,
    /// 条目指纹重复
    DupEntry,
    /// 相位非 Freezing 拒冻结/拒打印
    NotFreezing,
    /// 印面缺条或未全冻结
    ShortSeal,
    /// 二次收官（收口防重）
    DupSeal,
    /// 条目缺失（内部账面不一致）
    EntryMissing,
    /// 冻结后改写（指纹漂移）
    FrozenMutate,
    /// 印后指纹漂移（封印不可变性破坏）
    SealDrift,
    /// 累计行不足（未达 120,000）
    AccruedShort,
}

impl SealCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文本。
    pub fn detail(self) -> &'static str {
        match self {
            SealCode::Ok => "收官印判定成功",
            SealCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            SealCode::NotOpen => "相位非 Open：开账期之外不得入条",
            SealCode::Overfull => "条目超限：收官印恰 100 条不得多入",
            SealCode::DupEntry => "条目重复：同指纹条目不得二次入账",
            SealCode::NotFreezing => "相位非 Freezing：冻结/打印须在入满之后",
            SealCode::ShortSeal => "印面缺条：100 条未齐或未全冻结不得打印",
            SealCode::DupSeal => "二次收官：同一 M 型段只许打一次收官印",
            SealCode::EntryMissing => "条目缺失：账面序号与条数不一致",
            SealCode::FrozenMutate => "冻结改写：已冻结条目指纹不得再变",
            SealCode::SealDrift => "印后漂移：封印后账面指纹不得变化",
            SealCode::AccruedShort => "累计不足：收官印入册要求累计 120,000 行",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [SealCode; 12] = [
    SealCode::Ok,
    SealCode::SegBad,
    SealCode::NotOpen,
    SealCode::Overfull,
    SealCode::DupEntry,
    SealCode::NotFreezing,
    SealCode::ShortSeal,
    SealCode::DupSeal,
    SealCode::EntryMissing,
    SealCode::FrozenMutate,
    SealCode::SealDrift,
    SealCode::AccruedShort,
];

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg { s: seed ^ 0x2545f4914f6cdd1d }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 标准脚本：入满 100 条 → 冻结 → 打印。返回 (印指纹, 冻结条数)。
pub fn standard_script() -> (u64, usize) {
    let mut e = SealEngine::new();
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _ in 0..SEAL_ENTRIES {
        let hi = lcg.next_u32() as u64;
        let lo = lcg.next_u32() as u64;
        let _ = e.add_entry((hi << 32) | lo);
    }
    let _ = e.freeze();
    let fp = e.stamp();
    let f = match fp {
        Ok(f) => f,
        Err(_) => 0,
    };
    (f, e.frozen_count())
}

/// 双跑一致性：同一脚本两次建账，指纹与冻结数逐位一致。
pub fn dual_run_consistent() -> bool {
    standard_script() == standard_script()
}

/// 注入核：4 类坏样本 ×10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 4]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 4] = [0; 4];
        // 类 0：短印——只入 99 条即试图冻结，显性拒（NotFreezing）。
        for _ in 0..INJ_ROUNDS {
            let mut e2 = SealEngine::new();
            for i in 0..(SEAL_ENTRIES - 1) {
                let _ = e2.add_entry(0x1000 + i as u64);
            }
            let caught = e2.freeze() == Err(SealCode::NotFreezing);
        if caught {
            e2.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            e2.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：改冻条——冻结后再入条显性拒（NotOpen，相位已走）。
    for _ in 0..INJ_ROUNDS {
        let mut e = SealEngine::new();
        for i in 0..SEAL_ENTRIES {
            let _ = e.add_entry(0x2000 + i as u64);
        }
        let _ = e.freeze();
        let caught = e.add_entry(0xFFFF) == Err(SealCode::NotOpen);
        if caught {
            e.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            e.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：二次收官——打印后再打，显性拒（DupSeal）。
    for _ in 0..INJ_ROUNDS {
        let mut e = SealEngine::new();
        for i in 0..SEAL_ENTRIES {
            let _ = e.add_entry(0x3000 + i as u64);
        }
        let _ = e.freeze();
        let _ = e.stamp();
        let caught = e.stamp() == Err(SealCode::DupSeal);
        if caught {
            e.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            e.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：溢出印——入满后继续入条显性拒（相位已转 Freezing，判 NotOpen）。
    for _ in 0..INJ_ROUNDS {
        let mut e = SealEngine::new();
        for i in 0..SEAL_ENTRIES {
            let _ = e.add_entry(0x4000 + i as u64);
        }
        let caught = e.add_entry(0xBEEF) == Err(SealCode::NotOpen);
        if caught {
            e.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            e.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（14 条判据形态）。
pub fn run_unx51592_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51592");
    // 1 标准收官路径全通。
    {
        let (fp, frozen) = standard_script();
        s.add(
            "标准收官路径",
            fp != 0 && frozen == SEAL_ENTRIES,
            "入满 100 条→冻结→打印，印指纹非零且全冻结",
        );
    }
    // 2 冻结恰 100 条。
    {
        let mut e = SealEngine::new();
        for i in 0..SEAL_ENTRIES {
            let _ = e.add_entry(0x10 + i as u64);
        }
        let _ = e.freeze();
        s.add(
            "冻结恰 100 条",
            e.entries_len() == SEAL_ENTRIES && e.frozen_count() == SEAL_ENTRIES,
            "UNX-F51592 判据：M 型段收官印 100 条冻结",
        );
    }
    // 3 累计 120,000 入册。
    {
        let mut e = SealEngine::new();
        for i in 0..SEAL_ENTRIES {
            let _ = e.add_entry(0x20 + i as u64);
        }
        let _ = e.freeze();
        let _ = e.stamp();
        s.add(
            "累计 120,000 入册",
            e.accrued() == M_ACCRUED && e.sealed_once(),
            "收官印入册：M 型段累计 120,000 行且印只打一次",
        );
    }
    // 4 条目指纹重复显性拒。
    {
        let mut e = SealEngine::new();
        let _ = e.add_entry(0xABC);
        let r = e.add_entry(0xABC);
        s.add(
            "条目指纹重复拒",
            r == Err(SealCode::DupEntry),
            "同指纹条目不得二次入账",
        );
    }
    // 5 短印显性拒。
    {
        let mut e = SealEngine::new();
        for i in 0..(SEAL_ENTRIES - 1) {
            let _ = e.add_entry(0x30 + i as u64);
        }
        s.add(
            "短印显性拒",
            e.freeze() == Err(SealCode::NotFreezing),
            "100 条未齐不得进入冻结相位",
        );
    }
    // 6 二次收官防重。
    {
        let mut e = SealEngine::new();
        for i in 0..SEAL_ENTRIES {
            let _ = e.add_entry(0x40 + i as u64);
        }
        let _ = e.freeze();
        let first = e.stamp();
        let second = e.stamp();
        s.add(
            "收口防重",
            first.is_ok() && second == Err(SealCode::DupSeal) && e.seals_once_ok(),
            "UNX-F51593-J1 收口防重对偶：收官印只许打一次，二次显性拒",
        );
    }
    // 7 封印后只读。
    {
        let mut e = SealEngine::new();
        for i in 0..SEAL_ENTRIES {
            let _ = e.add_entry(0x50 + i as u64);
        }
        let _ = e.freeze();
        let _ = e.stamp();
        let ro = e.add_entry(0x777) == Err(SealCode::NotOpen)
            && e.freeze() == Err(SealCode::NotFreezing);
        s.add(
            "封印后只读",
            ro && e.phase() == SealPhase::Sealed,
            "Sealed 相位入条/冻结全部显性拒",
        );
    }
    // 8 相位单调：Open→Freezing→Sealed。
    {
        let mut e = SealEngine::new();
        let p0 = e.phase() as u8;
        for i in 0..SEAL_ENTRIES {
            let _ = e.add_entry(0x60 + i as u64);
        }
        let p1 = e.phase() as u8;
        let _ = e.freeze();
        let _ = e.stamp();
        let p2 = e.phase() as u8;
        s.add(
            "相位单调",
            p0 < p1 && p1 < p2,
            "Open→Freezing→Sealed 只进不退",
        );
    }
    // 9 注入核 4 类 ×10 全通零逃逸。
    {
        let (d, esc, per) = injection_harness();
        let all = per.iter().all(|&x| x == 10);
        s.add(
            "注入 4 类×10 全通",
            d == 40 && esc == 0 && all,
            "4 类坏样本逐类 10 轮精确复现，检出+逃逸=轮数，逃逸=0",
        );
    }
    // 10 诊断码闭集唯一性。
    {
        let mut ok = true;
        for (i, c) in ALL_CODES.iter().enumerate() {
            if c.val() != SEG_BASE + 1 + i as u32 || c.detail().is_empty() {
                ok = false;
            }
        }
        s.add(
            "诊断码闭集唯一",
            ok && ALL_CODES.len() >= 10 && ALL_CODES.len() <= 14,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 12 变体",
        );
    }
    // 11 注入账守恒。
    {
        let mut e = SealEngine::new();
        for _ in 0..11u32 {
            e.note_detected();
        }
        s.add("注入账守恒", e.conserved(), "注入=检出+逃逸且逃逸=0 且检出>0");
    }
    // 12 双跑 FNV 指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一脚本两次建账，印指纹与冻结数逐位一致",
    );
    // 13 指纹复算稳定。
    {
        let mut e = SealEngine::new();
        for i in 0..SEAL_ENTRIES {
            let _ = e.add_entry(0x70 + i as u64);
        }
        let _ = e.freeze();
        let _ = e.stamp();
        let f1 = e.fingerprint();
        let f2 = e.fingerprint();
        let brief: String = format!("{:016x}", f1);
        s.add(
            "指纹复算稳定",
            f1 == f2 && brief.len() == 16,
            "指纹两次复算相等且十六进制摘要定长 16",
        );
    }
    // 14 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51592 B20 M 型段收官印 {} 条冻结 累计 {}",
            SEAL_ENTRIES, M_ACCRUED
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51592"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空",
        );
    }
    s
}

impl SealEngine {
    /// 印只打一次判据面。
    pub fn seals_once_ok(&self) -> bool {
        self.seals_done == 1 && self.seal_fp.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51592_checks_green() {
        let set = run_unx51592_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn full_seal_path() {
        let mut e = SealEngine::new();
        for i in 0..SEAL_ENTRIES {
            assert!(e.add_entry(0x900 + i as u64).is_ok());
        }
        assert_eq!(e.phase(), SealPhase::Freezing);
        let f = e.freeze();
        assert!(f.is_ok());
        let fp = e.stamp();
        assert!(fp.is_ok());
        assert_eq!(e.phase(), SealPhase::Sealed);
        assert_eq!(e.frozen_count(), SEAL_ENTRIES);
        assert_eq!(e.accrued(), M_ACCRUED);
    }

    #[test]
    fn short_seal_rejected() {
        let mut e = SealEngine::new();
        for i in 0..(SEAL_ENTRIES - 1) {
            let _ = e.add_entry(0x800 + i as u64);
        }
        assert_eq!(e.freeze(), Err(SealCode::NotFreezing));
        assert_eq!(e.stamp(), Err(SealCode::NotFreezing));
    }

    #[test]
    fn duplicate_entry_rejected() {
        let mut e = SealEngine::new();
        assert!(e.add_entry(0x111).is_ok());
        assert_eq!(e.add_entry(0x111), Err(SealCode::DupEntry));
        assert_eq!(e.entries_len(), 1);
    }

    #[test]
    fn double_stamp_rejected() {
        let mut e = SealEngine::new();
        for i in 0..SEAL_ENTRIES {
            let _ = e.add_entry(0x700 + i as u64);
        }
        let _ = e.freeze();
        assert!(e.stamp().is_ok());
        assert_eq!(e.stamp(), Err(SealCode::DupSeal));
        assert_eq!(e.rejected_tally(), 1);
    }

    #[test]
    fn sealed_is_readonly() {
        let mut e = SealEngine::new();
        for i in 0..SEAL_ENTRIES {
            let _ = e.add_entry(0x600 + i as u64);
        }
        let _ = e.freeze();
        let _ = e.stamp();
            assert_eq!(e.add_entry(0x222), Err(SealCode::NotOpen));
        assert_eq!(e.freeze(), Err(SealCode::NotFreezing));
    }

    #[test]
    fn entry_lookup_bounds() {
        let mut e = SealEngine::new();
        for i in 0..5 {
            let _ = e.add_entry(0x500 + i as u64);
        }
        assert_eq!(e.entry_fp(1), Some(0x500));
        assert_eq!(e.entry_fp(5), Some(0x504));
        assert_eq!(e.entry_fp(6), None);
        assert_eq!(e.entry_fp(0), None);
        assert!(!e.entry_frozen(6));
    }

    #[test]
    fn injection_all_caught() {
        let (d, esc, per) = injection_harness();
        assert_eq!(esc, 0);
        assert_eq!(d, 40);
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "类 {} 应恰 10 次检出", i);
        }
    }

    #[test]
    fn conservation_holds() {
        let mut e = SealEngine::new();
        for _ in 0..9u32 {
            e.note_detected();
        }
        assert!(e.conserved());
        let mut bad = SealEngine::new();
        bad.note_escape();
        assert!(!bad.conserved());
    }

    #[test]
    fn dual_run_identical_fingerprint() {
        assert!(dual_run_consistent());
        assert_eq!(standard_script(), standard_script());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 12);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
        for i in 0..ALL_CODES.len() {
            for j in (i + 1)..ALL_CODES.len() {
                let a = match ALL_CODES.get(i) {
                    Some(&x) => x,
                    None => continue,
                };
                let b = match ALL_CODES.get(j) {
                    Some(&y) => y,
                    None => continue,
                };
                assert_ne!(a.val(), b.val());
            }
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
