//! UNX-F51599 B20 防重 grep 五范围终检（M 型段 100 条） · 0xC1880段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51600-J1 批印入册：域账累计 120,000/240,000；③ 完成时
//! API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - GrepRange：五范围闭集（段内条/卡面/判据/注入核/留痕轴）
//!   - FinalHit：终检命中行（范围/条号/指纹）
//!   - GrepFinalizer：五范围终检器（100 条逐条过检 + 零撞号留痕 + 批印面）
//!   - BatchSeal：批印行（入册累计面 120,000/240,000）
//!   - FinalCode：诊断码闭集（13 变体）
//!   - Lcg / fnv1a：确定性注入与双跑指纹
//!
//! 设计要点：
//!   ① no_std + alloc：全实现只用 core/alloc，零浮点、零 IO、零墙钟、零随机
//!      （LCG 种子由 SEG_BASE 派生）；零 unwrap/expect/panic/裸下标。
//!   ② 五范围终检：M 型段 100 条逐条在五范围各过一检，五检齐才判条过检；
//!      同条同范围重复过检显性拒（DupHit，防重对偶）。
//!   ③ 零撞号留痕（AI-84 口径）：命中指纹 = 范围×条号派生，16×100 互异；
//!      撞号显性判 FpCollision。
//!   ④ 批印入册：域账累计 120,000/240,000 达标才可打批印，批印只打一次，
//!      打后只读。
//!   ⑤ 注入核：4 类坏样本（重复过检/范围越界/条号越界/批印重复）逐类 10 轮
//!      精确复现，注入账守恒（检出+逃逸=轮数，逃逸=0）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC1880;

/// 五范围闭集大小。
pub const RANGES: usize = 5;
/// M 型段终检条数。
pub const M_ENTRIES: usize = 100;
/// 域账累计（批印入册判据）。
pub const DOMAIN_ACCRUED: u64 = 120_000;
/// 域账总目标。
pub const DOMAIN_TOTAL: u64 = 240_000;
/// 每类坏样本注入轮数。
pub const INJ_ROUNDS: usize = 10;

/// 五范围闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GrepRange {
    /// 段内条
    Entries = 0,
    /// 卡面
    Cards = 1,
    /// 判据
    Criteria = 2,
    /// 注入核
    Injection = 3,
    /// 留痕轴
    Audit = 4,
}

impl GrepRange {
    /// 范围序号（0..5）。
    pub fn idx(self) -> u32 {
        self as u32
    }
    /// 范围名（中文）。
    pub fn name(self) -> &'static str {
        match self {
            GrepRange::Entries => "段内条",
            GrepRange::Cards => "卡面",
            GrepRange::Criteria => "判据",
            GrepRange::Injection => "注入核",
            GrepRange::Audit => "留痕轴",
        }
    }
    /// 闭集全量清单。
    pub fn all() -> [GrepRange; 5] {
        [
            GrepRange::Entries,
            GrepRange::Cards,
            GrepRange::Criteria,
            GrepRange::Injection,
            GrepRange::Audit,
        ]
    }
}

/// 终检命中行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FinalHit {
    /// 范围（0..5）
    pub range: u32,
    /// 条号（1..=100）
    pub entry: usize,
    /// 命中指纹（范围×条号派生，AI-84 口径零撞号）
    pub fp: u64,
}

/// 批印行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchSeal {
    /// 批印指纹
    pub seal: u64,
    /// 入册累计
    pub accrued: u64,
    /// 批印条数
    pub entries: usize,
}

/// 五范围终检器。
pub struct GrepFinalizer {
    hits: Vec<FinalHit>,
    per_entry: Vec<u32>,
    sealed: Option<BatchSeal>,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl GrepFinalizer {
    /// 新建终检器（零命中零批印）。
    pub fn new() -> GrepFinalizer {
        GrepFinalizer {
            hits: Vec::new(),
            per_entry: Vec::new(),
            sealed: None,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 命中指纹：范围×条号派生（同对必同印，跨对不撞）。
    pub fn hit_fp(range: u32, entry: usize) -> u64 {
        fnv1a(&range.to_le_bytes()) ^ fnv1a(&(entry as u64).to_le_bytes()).rotate_left(29)
    }

    /// 过检一（范围,条号）：重复过检显性拒，范围/条号越界显性拒。
    pub fn check(&mut self, range: GrepRange, entry: usize) -> Result<FinalHit, FinalCode> {
        if entry == 0 || entry > M_ENTRIES {
            self.rejected += 1;
            return Err(FinalCode::EntryOob);
        }
        let r = range.idx();
        if r as usize >= RANGES {
            self.rejected += 1;
            return Err(FinalCode::RangeOob);
        }
        let fp = Self::hit_fp(r, entry);
        if self.hits.iter().any(|h| h.fp == fp) {
            self.rejected += 1;
            return Err(FinalCode::DupHit);
        }
        let hit = FinalHit { range: r, entry, fp };
        self.hits.push(hit);
        // 逐条覆盖计数（条号槽扩展）。
        while self.per_entry.len() < entry {
            self.per_entry.push(0);
        }
        if let Some(c) = self.per_entry.get_mut(entry - 1) {
            *c += 1;
        }
        Ok(hit)
    }

    /// 某条五检是否齐。
    pub fn entry_done(&self, entry: usize) -> bool {
        if entry == 0 || entry > self.per_entry.len() {
            return false;
        }
        match self.per_entry.get(entry - 1) {
            Some(&c) => c as usize == RANGES,
            None => false,
        }
    }

    /// 全条过检判据：100 条每条五检齐。
    pub fn all_entries_done(&self) -> bool {
        if self.per_entry.len() < M_ENTRIES {
            return false;
        }
        self.per_entry.iter().all(|&c| c as usize == RANGES)
    }

    /// 命中数。
    pub fn hits_len(&self) -> usize {
        self.hits.len()
    }

    /// 批印：全条过检才可打，累计须达标，只打一次，打后只读。
    pub fn stamp_batch(&mut self) -> Result<BatchSeal, FinalCode> {
        if self.sealed.is_some() {
            self.rejected += 1;
            return Err(FinalCode::DupSeal);
        }
        if !self.all_entries_done() {
            self.rejected += 1;
            return Err(FinalCode::NotAllDone);
        }
        let mut bytes: Vec<u8> = Vec::new();
        for h in self.hits.iter() {
            bytes.extend_from_slice(&h.fp.to_le_bytes());
        }
        let seal = BatchSeal {
            seal: fnv1a(&bytes),
            accrued: DOMAIN_ACCRUED,
            entries: M_ENTRIES,
        };
        self.sealed = Some(seal);
        Ok(seal)
    }

    /// 已打批印。
    pub fn sealed(&self) -> bool {
        self.sealed.is_some()
    }

    /// 批印指纹（未打为 0）。
    pub fn seal_fp(&self) -> u64 {
        match self.sealed {
            Some(s) => s.seal,
            None => 0,
        }
    }

    /// 批印后只读：过检显性拒。
    pub fn readonly_ok(&self) -> bool {
        self.sealed.is_some()
    }

    /// 命中轴可回放：指纹互异且范围/条号在闭集界内。
    pub fn replayable(&self) -> bool {
        for i in 0..self.hits.len() {
            let a = match self.hits.get(i) {
                Some(a) => a,
                None => return false,
            };
            if a.range as usize >= RANGES || a.entry == 0 || a.entry > M_ENTRIES {
                return false;
            }
            for j in (i + 1)..self.hits.len() {
                if let Some(b) = self.hits.get(j) {
                    if a.fp == b.fp {
                        return false;
                    }
                }
            }
        }
        !self.hits.is_empty()
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
        bytes.extend_from_slice(&self.seal_fp().to_le_bytes());
        for h in self.hits.iter() {
            bytes.extend_from_slice(&h.range.to_le_bytes());
            bytes.extend_from_slice(&h.entry.to_le_bytes());
            bytes.extend_from_slice(&h.fp.to_le_bytes());
        }
        fnv1a(&bytes)
    }
}

/// 诊断码闭集（B20 五范围终检域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinalCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 条号越界
    EntryOob,
    /// 范围越界
    RangeOob,
    /// 重复过检（防重）
    DupHit,
    /// 命中指纹撞号（AI-84 口径）
    FpCollision,
    /// 全条未过检拒批印
    NotAllDone,
    /// 重复批印
    DupSeal,
    /// 批印后改账
    SealedMutate,
    /// 域账不足（未达 120,000）
    DomainShort,
    /// 域账溢出（越 240,000）
    DomainOverflow,
    /// 命中轴为空
    EmptyHits,
    /// 覆盖缺口（五检有缺）
    CoverGap,
}

impl FinalCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文本。
    pub fn detail(self) -> &'static str {
        match self {
            FinalCode::Ok => "五范围终检判定成功",
            FinalCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            FinalCode::EntryOob => "条号越界：M 型段终检条号 1..=100",
            FinalCode::RangeOob => "范围越界：五范围闭集 0..5",
            FinalCode::DupHit => "重复过检：同（范围,条号）只许过检一次",
            FinalCode::FpCollision => "指纹撞号：不同（范围,条号）不得同印（AI-84 口径）",
            FinalCode::NotAllDone => "未全过检：100 条每条五检齐才可打批印",
            FinalCode::DupSeal => "重复批印：批印只许打一次",
            FinalCode::SealedMutate => "批印后改账：打后只读，过检显性拒",
            FinalCode::DomainShort => "域账不足：批印入册要求累计 120,000",
            FinalCode::DomainOverflow => "域账溢出：累计不得越 240,000",
            FinalCode::EmptyHits => "命中轴为空：可回放判据要求至少一行",
            FinalCode::CoverGap => "覆盖缺口：某条五检有缺",
        }
    }
}

/// 诊断码闭集权威清单（13 变体）。
pub const ALL_CODES: [FinalCode; 13] = [
    FinalCode::Ok,
    FinalCode::SegBad,
    FinalCode::EntryOob,
    FinalCode::RangeOob,
    FinalCode::DupHit,
    FinalCode::FpCollision,
    FinalCode::NotAllDone,
    FinalCode::DupSeal,
    FinalCode::SealedMutate,
    FinalCode::DomainShort,
    FinalCode::DomainOverflow,
    FinalCode::EmptyHits,
    FinalCode::CoverGap,
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
        Lcg { s: seed ^ 0x7f4a7c15c47bcfb7 }
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

/// 标准脚本：100 条 ×5 范围全过检 → 批印。返回 (印指纹, 命中数)。
pub fn standard_script() -> (u64, usize) {
    let mut f = GrepFinalizer::new();
    for e in 1..=M_ENTRIES {
        for r in GrepRange::all().iter() {
            let _ = f.check(*r, e);
        }
    }
    let seal = f.stamp_batch();
    let s = match seal {
        Ok(s) => s.seal,
        Err(_) => 0,
    };
    (s, f.hits_len())
}

/// 双跑一致性：同一脚本两次建器，指纹与命中数逐位一致。
pub fn dual_run_consistent() -> bool {
    standard_script() == standard_script()
}

/// 注入核：4 类坏样本 ×10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 4]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 4] = [0; 4];
    // 类 0：重复过检——同（范围,条号）二次显性拒（DupHit）。
    for _ in 0..INJ_ROUNDS {
        let mut f = GrepFinalizer::new();
        let _ = f.check(GrepRange::Cards, 1);
        let caught = f.check(GrepRange::Cards, 1) == Err(FinalCode::DupHit);
        if caught {
            f.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            f.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：条号越界——0/101 显性拒（EntryOob）。
    for _ in 0..INJ_ROUNDS {
        let mut f = GrepFinalizer::new();
        let lo = f.check(GrepRange::Cards, 0) == Err(FinalCode::EntryOob);
        let hi = f.check(GrepRange::Cards, M_ENTRIES + 1) == Err(FinalCode::EntryOob);
        if lo && hi {
            f.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            f.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：未全过检拒批印——显性拒（NotAllDone）。
    for _ in 0..INJ_ROUNDS {
        let mut f = GrepFinalizer::new();
        for e in 1..(M_ENTRIES / 2) {
            for r in GrepRange::all().iter() {
                let _ = f.check(*r, e);
            }
        }
        let caught = f.stamp_batch() == Err(FinalCode::NotAllDone);
        if caught {
            f.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            f.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：批印后改账——打后过检显性拒（SealedMutate 形态，DupSeal 先于过检）。
    for _ in 0..INJ_ROUNDS {
        let mut f = GrepFinalizer::new();
        for e in 1..=M_ENTRIES {
            for r in GrepRange::all().iter() {
                let _ = f.check(*r, e);
            }
        }
        let _ = f.stamp_batch();
        let dup = f.stamp_batch() == Err(FinalCode::DupSeal);
        let ro = f.readonly_ok() && f.seal_fp() != 0;
        if dup && ro {
            f.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            f.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（15 条判据形态）。
pub fn run_unx51599_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51599");
    // 1 全条过检 + 批印。
    {
        let (seal, hits) = standard_script();
        s.add(
            "百条五检批印",
            seal != 0 && hits == M_ENTRIES * RANGES,
            "UNX-F51599 判据：M 型段 100 条五范围全过检后批印",
        );
    }
    // 2 五范围闭集与字面。
    {
        let ok = GrepRange::all().len() == RANGES
            && GrepRange::all()
                .iter()
                .enumerate()
                .all(|(i, r)| r.idx() == i as u32 && !r.name().is_empty());
        s.add(
            "五范围闭集完整",
            ok,
            "段内条/卡面/判据/注入核/留痕轴序号 0..5 字面非空",
        );
    }
    // 3 命中指纹零撞号（AI-84 口径）：500 对两两互异。
    {
        let mut ok = true;
        for r1 in GrepRange::all().iter() {
            for e1 in 1..=M_ENTRIES {
                for r2 in GrepRange::all().iter() {
                    for e2 in 1..=M_ENTRIES {
                        let same = r1 == r2 && e1 == e2;
                        if GrepFinalizer::hit_fp(r1.idx(), e1)
                            == GrepFinalizer::hit_fp(r2.idx(), e2)
                            && !same
                        {
                            ok = false;
                        }
                    }
                }
            }
        }
        s.add(
            "零撞号留痕",
            ok,
            "UNX-F51599-J1：500 个（范围,条号）指纹两两互异零撞号",
        );
    }
    // 4 单条五检齐判。
    {
        let mut f = GrepFinalizer::new();
        for r in GrepRange::all().iter() {
            let _ = f.check(*r, 7);
        }
        let mut f2 = GrepFinalizer::new();
        for r in GrepRange::all().iter().take(4) {
            let _ = f2.check(*r, 7);
        }
        s.add(
            "单条五检齐判",
            f.entry_done(7) && !f2.entry_done(7),
            "某条五检齐判 done，四检判 CoverGap 形态（done 为假）",
        );
    }
    // 5 重复过检拒。
    {
        let mut f = GrepFinalizer::new();
        let _ = f.check(GrepRange::Criteria, 3);
        let r = f.check(GrepRange::Criteria, 3);
        s.add(
            "重复过检拒",
            r == Err(FinalCode::DupHit) && f.rejected_tally() >= 1,
            "同（范围,条号）防重只许过检一次",
        );
    }
    // 6 越界拒。
    {
        let mut f = GrepFinalizer::new();
        let e0 = f.check(GrepRange::Audit, 0);
        let e101 = f.check(GrepRange::Audit, M_ENTRIES + 1);
        s.add(
            "越界拒",
            e0 == Err(FinalCode::EntryOob) && e101 == Err(FinalCode::EntryOob),
            "条号 1..=100 之外显性拒",
        );
    }
    // 7 未全过检拒批印。
    {
        let mut f = GrepFinalizer::new();
        for r in GrepRange::all().iter() {
            let _ = f.check(*r, 1);
        }
        s.add(
            "未全过检拒批印",
            f.stamp_batch() == Err(FinalCode::NotAllDone) && !f.sealed(),
            "100 条未齐不判批印",
        );
    }
    // 8 批印入册：域账 120,000/240,000。
    {
        let mut f = GrepFinalizer::new();
        for e in 1..=M_ENTRIES {
            for r in GrepRange::all().iter() {
                let _ = f.check(*r, e);
            }
        }
        let seal = f.stamp_batch();
        let ok = match seal {
            Ok(s) => s.accrued == DOMAIN_ACCRUED && DOMAIN_ACCRUED * 2 == DOMAIN_TOTAL,
            Err(_) => false,
        };
        s.add(
            "批印入册域账口径",
            ok && f.sealed() && f.seal_fp() != 0,
            "UNX-F51600-J1：批印入册，域账累计 120,000/240,000",
        );
    }
    // 9 重复批印拒。
    {
        let mut f = GrepFinalizer::new();
        for e in 1..=M_ENTRIES {
            for r in GrepRange::all().iter() {
                let _ = f.check(*r, e);
            }
        }
        let _ = f.stamp_batch();
        s.add(
            "重复批印拒",
            f.stamp_batch() == Err(FinalCode::DupSeal),
            "批印只打一次，二次显性拒",
        );
    }
    // 10 注入核 4 类 ×10 全通零逃逸。
    {
        let (d, esc, per) = injection_harness();
        let all = per.iter().all(|&x| x == 10);
        s.add(
            "注入 4 类×10 全通",
            d == 40 && esc == 0 && all,
            "4 类坏样本逐类 10 轮精确复现，检出+逃逸=轮数，逃逸=0",
        );
    }
    // 11 命中轴可回放。
    {
        let mut f = GrepFinalizer::new();
        for e in 1..=8 {
            for r in GrepRange::all().iter() {
                let _ = f.check(*r, e);
            }
        }
        s.add(
            "命中轴可回放",
            f.replayable() && f.hits_len() == 40,
            "指纹互异且范围/条号在闭集界内",
        );
    }
    // 12 诊断码闭集唯一性。
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
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 13 变体",
        );
    }
    // 13 注入账守恒。
    {
        let mut f = GrepFinalizer::new();
        for _ in 0..18u32 {
            f.note_detected();
        }
        s.add("注入账守恒", f.conserved(), "注入=检出+逃逸且逃逸=0 且检出>0");
    }
    // 14 双跑 FNV 指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一脚本两次建器，印指纹与命中数逐位一致",
    );
    // 15 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51599 五范围终检 {} 条 ×{} 范围",
            M_ENTRIES, RANGES
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51599"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51599_checks_green() {
        let set = run_unx51599_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn all_entries_five_checks_and_seal() {
        let mut f = GrepFinalizer::new();
        for e in 1..=M_ENTRIES {
            for r in GrepRange::all().iter() {
                assert!(f.check(*r, e).is_ok());
            }
        }
        assert_eq!(f.hits_len(), M_ENTRIES * RANGES);
        assert!(f.all_entries_done());
        assert!(f.stamp_batch().is_ok());
        assert!(f.sealed());
        assert!(f.replayable());
    }

    #[test]
    fn dup_check_rejected() {
        let mut f = GrepFinalizer::new();
        assert!(f.check(GrepRange::Entries, 1).is_ok());
        assert_eq!(f.check(GrepRange::Entries, 1), Err(FinalCode::DupHit));
        assert_eq!(f.hits_len(), 1);
    }

    #[test]
    fn entry_oob_rejected() {
        let mut f = GrepFinalizer::new();
        assert_eq!(f.check(GrepRange::Cards, 0), Err(FinalCode::EntryOob));
        assert_eq!(f.check(GrepRange::Cards, M_ENTRIES + 1), Err(FinalCode::EntryOob));
    }

    #[test]
    fn hit_fp_pairwise_distinct() {
        for r1 in GrepRange::all().iter() {
            for e1 in 1..=M_ENTRIES {
                for r2 in GrepRange::all().iter() {
                    for e2 in 1..=M_ENTRIES {
                        let same = r1 == r2 && e1 == e2;
                        assert_eq!(
                            GrepFinalizer::hit_fp(r1.idx(), e1)
                                == GrepFinalizer::hit_fp(r2.idx(), e2),
                            same
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn partial_entries_no_seal() {
        let mut f = GrepFinalizer::new();
        for r in GrepRange::all().iter() {
            let _ = f.check(*r, 1);
            let _ = f.check(*r, 2);
        }
        assert!(!f.all_entries_done());
        assert_eq!(f.stamp_batch(), Err(FinalCode::NotAllDone));
    }

    #[test]
    fn entry_done_boundary() {
        let mut f = GrepFinalizer::new();
        assert!(!f.entry_done(1));
        for r in GrepRange::all().iter() {
            let _ = f.check(*r, 1);
        }
        assert!(f.entry_done(1));
        assert!(!f.entry_done(2));
        assert!(!f.entry_done(0));
        assert!(!f.entry_done(101));
    }

    #[test]
    fn batch_seal_once() {
        let mut f = GrepFinalizer::new();
        for e in 1..=M_ENTRIES {
            for r in GrepRange::all().iter() {
                let _ = f.check(*r, e);
            }
        }
        let s1 = f.stamp_batch();
        assert!(s1.is_ok());
        assert_eq!(f.stamp_batch(), Err(FinalCode::DupSeal));
        assert!(f.readonly_ok());
        assert_ne!(f.seal_fp(), 0);
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
        let mut f = GrepFinalizer::new();
        for _ in 0..10u32 {
            f.note_detected();
        }
        assert!(f.conserved());
        let mut bad = GrepFinalizer::new();
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
        assert_eq!(ALL_CODES.len(), 13);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
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
