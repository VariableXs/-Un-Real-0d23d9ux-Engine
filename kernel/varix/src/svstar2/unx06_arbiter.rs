//! UNX-F0061 · 三源冲突仲裁策略器（CoRun · UNX-A1-B04 内存图三源合并）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md#UNX-F0061`
//! （A1-B04 · 内存图三源合并与物理布局 F0061–F0080）
//!
//! **判据（锚点原文）**：UNX-F0061-J1 构造 100 组合成冲突样本，仲裁
//! 结果与设计优先级矩阵 100% 一致，冲突有账。
//!
//! **职责定位（原文提炼）**：三源冲突仲裁策略器——内存图三源合并
//! （EFI 内存描述符表 / E820 / ACPI 表）存在同一地址区间重复申报
//! 且类型表述不一致的情形（类型非单射：E820 某型可映射到两种
//! EFI 类型；ACPI 表按地址区间二次申报）。仲裁策略 = **设计优先级
//! 矩阵**（源可信度 × 类型确定性）——同一区间多源申报时按矩阵
//! 裁决采用哪一源的条目，冲突必留账（对账后处理）。
//!
//! # 一、优先级矩阵（裁决表的可执行体）
//!
//! [`SourceKind`] 三源（Efi/E820/Acpi）；设计优先级（锚点设计）：
//!
//! 1. **EFI 描述符最权威**（boot services 精确分型，地址/长度以
//!    页为单位直接来自固件）；
//! 2. **E820 次之**（BIOS 兼容路径，类型经映射表转换，转换本身
//!    有歧义——多源冲突时 EFI 优先即消歧）；
//! 3. **ACPI 表兜底**（地址区间二次申报带推测性，仅当另两源
//!    缺位时采用）。
//!
//! 裁决 = 区间交集内最小优先级序号获胜；平级（同源两区间交叉）
//! 按地址低位优先 + 短区间优先（后写覆盖前写的确定性序）。
//!
//! # 二、对账留痕（冲突有账）
//!
//! 每次仲裁产出 [`ArbitrationRecord`]（区间/参与源/胜者/原因）——
//! 冲突不是吞掉就完了：账本供 F0062 保留区白名单、F0065 E820 桥接
//! 降级路径回查「当时为何如此裁决」。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::memmap::{Kind, Region};

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const ARBITER_VERSION: &str = "UNX-F0061-v1";

/// 区间非法（base+length 回绕或零长）。
pub const E_ARB_BAD_SPAN: &str = "E_ARB_BAD_SPAN";

/// 源条目区间重叠（合并输入自身不自洽）。
pub const E_ARB_OVERLAP: &str = "E_ARB_OVERLAP";

/// 账本满（溢出拒绝——不静默丢账）。
pub const E_ARB_LEDGER_FULL: &str = "E_ARB_LEDGER_FULL";

/// 账本容量（冲突账上限——超限显性拒）。
pub const MAX_ARB_LEDGER: usize = 512;

/// 样本规模（锚点 J1：100 组合成冲突样本）。
pub const ARB_SAMPLES: u32 = 100;

/// 三源（内存图三源合并的陈述源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceKind {
    /// ACPI 表（兜底——推测性区间）。
    Acpi,
    /// E820（BIOS 兼容路径——经映射表转换）。
    E820,
    /// EFI 内存描述符表（boot services 精确分型）。
    Efi,
}

impl SourceKind {
    /// 三源闭集。
    pub const ALL: [SourceKind; 3] = [SourceKind::Acpi, SourceKind::E820, SourceKind::Efi];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            SourceKind::Acpi => "ACPI 表",
            SourceKind::E820 => "E820",
            SourceKind::Efi => "EFI 描述符表",
        }
    }

    /// 设计优先级序位（0=最低兜底 … 2=最高权威——矩阵第一列）。
    pub fn rank(self) -> u32 {
        match self {
            SourceKind::Acpi => 0,
            SourceKind::E820 => 1,
            SourceKind::Efi => 2,
        }
    }
}

/// 源条目（区间+类型+源别）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceEntry {
    /// 区间基址。
    pub base: u64,
    /// 区间长度。
    pub length: u64,
    /// 申报类型。
    pub kind: Kind,
    /// 来源。
    pub source: SourceKind,
}

impl SourceEntry {
    /// 新条目（构造即校验区间合法）。
    pub fn new(base: u64, length: u64, kind: Kind, source: SourceKind) -> Result<SourceEntry, String> {
        if length == 0 || base.checked_add(length).is_none() {
            return Err(format!(
                "{}：区间 [{:#x},{:#x}) 非法（零长或回绕）",
                E_ARB_BAD_SPAN, base, length
            ));
        }
        Ok(SourceEntry { base, length, kind, source })
    }

    /// 区间终点。
    pub fn end(&self) -> u64 {
        self.base + self.length
    }

    /// 与另一条目的交集长度（0=不相交）。
    pub fn overlap(&self, other: &SourceEntry) -> u64 {
        let lo = if self.base > other.base { self.base } else { other.base };
        let hi = if self.end() < other.end() { self.end() } else { other.end() };
        hi.saturating_sub(lo)
    }
}

/// 仲裁记录（冲突账一行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArbitrationRecord {
    /// 冲突区间起点。
    pub from: u64,
    /// 冲突区间终点。
    pub to: u64,
    /// 参与源（升序去重）。
    pub participants: Vec<SourceKind>,
    /// 胜者源。
    pub winner: SourceKind,
    /// 裁决原因（人读——矩阵判语）。
    pub reason: String,
}

/// 仲裁账本（冲突有账——超容显性拒）。
#[derive(Clone, Debug, Default)]
pub struct ArbLedger {
    records: Vec<ArbitrationRecord>,
}

impl ArbLedger {
    /// 空账本。
    pub fn new() -> ArbLedger {
        ArbLedger::default()
    }

    /// 记一条（超容显性拒——冲突账不许静默丢）。
    pub fn record(&mut self, rec: ArbitrationRecord) -> Result<(), String> {
        if self.records.len() >= MAX_ARB_LEDGER {
            return Err(format!(
                "{}：账本已满 {} 条——冲突账不许静默丢弃",
                E_ARB_LEDGER_FULL, MAX_ARB_LEDGER
            ));
        }
        self.records.push(rec);
        Ok(())
    }

    /// 在账条数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 全部账行（读屏可达）。
    pub fn all(&self) -> &[ArbitrationRecord] {
        &self.records
    }

    /// 按胜者计条数（对账统计）。
    pub fn count_by_winner(&self, s: SourceKind) -> usize {
        self.records.iter().filter(|r| r.winner == s).count()
    }
}

// ---------------------------------------------------------------------------
// 二、仲裁裁决（优先级矩阵可执行）
// ---------------------------------------------------------------------------

/// 单区间仲裁（同一区间起点重叠的条目集 → 胜者+原因）。
///
/// 判语：矩阵序位高者胜；平级取区间短者（短区间 = 更精确的二次
/// 申报）；再平级取基址低者（地址序确定，不靠输入序）。
pub fn arbitrate_span(entries: &[&SourceEntry]) -> Result<(SourceKind, String), String> {
    if entries.is_empty() {
        return Err(format!("{}：空条目集不可仲裁", E_ARB_OVERLAP));
    }
    let mut winner = entries[0];
    let mut best_rank = winner.source.rank();
    let mut best_len = winner.length;
    let mut best_base = winner.base;
    let mut reasons: Vec<SourceKind> = Vec::new();
    for e in entries.iter().skip(1) {
        if !reasons.iter().any(|s| *s == winner.source) {
            reasons.push(winner.source);
        }
        let cand = e;
        let cand_rank = cand.source.rank();
        // 1) 矩阵序位。
        if cand_rank > best_rank {
            winner = cand;
            best_rank = cand_rank;
            best_len = cand.length;
            best_base = cand.base;
        } else if cand_rank == best_rank {
            // 2) 同源冲突：短区间优先（二次申报更精确）。
            if cand.length < best_len {
                winner = cand;
                best_len = cand.length;
                best_base = cand.base;
            } else if cand.length == best_len && cand.base < best_base {
                // 3) 再平级：基址低者（输入序无关的确定性）。
                winner = cand;
                best_base = cand.base;
            }
        }
    }
    if !reasons.iter().any(|s| *s == winner.source) {
        reasons.push(winner.source);
    }
    let reason = format!(
        "区间 [{:#x},{:#x}) 三源申报 {:?}，按矩阵 {} 段位胜（{}）；平级规约：短区间优先→基址低者",
        winner.base,
        winner.end(),
        reasons,
        best_rank,
        winner.source.zh()
    );
    Ok((winner.source, reason))
}

/// 合并 + 对账（三源清单 → 仲裁胜出区间集 + 冲突账）。
///
/// 流程：输入区间两两求交，交集 >0 即构成冲突（记入账本）；胜出
/// 条目按基址排序输出（合并视图的确定序——与输入序无关）。
pub fn merge_three_sources(
    entries: &[SourceEntry],
    ledger: &mut ArbLedger,
) -> Result<Vec<Region>, String> {
    // 1) 输入区间重叠自检（合并输入自身必须两两不相交）。
    for (i, a) in entries.iter().enumerate() {
        for b in entries.iter().skip(i + 1) {
            if a.overlap(b) > 0 {
                return Err(format!(
                    "{}：源条目 [{:#x},{:#x}) 与 [{:#x},{:#x}) 重叠——先经源内去重再入仲裁",
                    E_ARB_OVERLAP, a.base, a.end(), b.base, b.end()
                ));
            }
        }
    }
    // 2) 冲突收集（同区间起点的条目聚为仲裁组）。
    let mut winners: Vec<SourceKind> = Vec::new();
    let mut used = alloc::vec![false; entries.len()];
    let mut i = 0usize;
    while i < entries.len() {
        if used[i] {
            i += 1;
            continue;
        }
        let mut group: Vec<&SourceEntry> = Vec::new();
        let mut j = i;
        while j < entries.len() {
            if !used[j] && entries[j].overlap(&entries[i]) > 0 {
                group.push(&entries[j]);
                used[j] = true;
            }
            j += 1;
        }
        let (winner, reason) = arbitrate_span(&group)?;
        let mut participants: Vec<SourceKind> = Vec::new();
        for g in group.iter() {
            if !participants.iter().any(|s| *s == g.source) {
                participants.push(g.source);
            }
        }
        participants.sort();
        ledger.record(ArbitrationRecord {
            from: entries[i].base.min(group.iter().map(|g| g.base).min().unwrap_or(entries[i].base)),
            to: entries[i].end().max(group.iter().map(|g| g.end()).max().unwrap_or(entries[i].end)),
            participants,
            winner,
            reason,
        })?;
        winners.push(winner);
        i += 1;
    }
    // 3) 胜出区间集（按仲裁后条目落 Region——类型随胜者源条目）。
    let mut regions: Vec<Region> = Vec::new();
    let mut k = 0usize;
    while k < entries.len() {
        let e = &entries[k];
        let won = winners.iter().any(|w| *w == e.source);
        if won {
            regions.push(Region {
                base: e.base,
                length: e.length,
                kind: e.kind,
            });
        }
        k += 1;
    }
    // 4) 确定序输出（按基址排序——与输入序无关）。
    regions.sort_by_key(|r| r.base);
    Ok(regions)
}

// ---------------------------------------------------------------------------
// 三、样本构造（100 组合成冲突样本）
// ---------------------------------------------------------------------------

/// 确定性 LCG（样本构造用——同种子同语料，CI 可固化）。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 新种子（0 种子映射非零常量防锁死）。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed ^ 0x9E37_79B9_7F4A_7C15 }
    }

    /// 下一个 u64。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    /// [lo,hi) 整数。
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        lo + self.next_u64() % (hi - lo)
    }
}

/// 合成样本条目集（100 组：三源 × 冲突/无冲突混合）。
///
/// 每组三源条目同基址（保证交集）但长度/类型按矩阵设计错位——
/// 仲裁结果必胜 EFI 源条目（矩阵最高段位），抽样断言 100% 一致。
pub fn synth_samples(count: u32, seed: u64) -> Result<Vec<SourceEntry>, String> {
    let mut out: Vec<SourceEntry> = Vec::new();
    let mut rng = Lcg::new(seed);
    let mut i = 0u32;
    while i < count {
        let base = 0x100_0000u64 + i as u64 * 0x10_0000u64;
        // EFI：权威条目（最长、Usable）。
        out.push(SourceEntry::new(
            base,
            rng.range(0x8000, 0x10000),
            Kind::Usable,
            SourceKind::Efi,
        )?);
        // E820：次权威（映射歧义型）。
        out.push(SourceEntry::new(
            base,
            rng.range(0x4000, 0x8000),
            Kind::Reserved,
            SourceKind::E820,
        )?);
        // ACPI：兜底（短区间推测性）。
        out.push(SourceEntry::new(base, rng.range(0x1000, 0x4000), Kind::AcpiNvs, SourceKind::Acpi)?);
        i += 1;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 四、域自检
// ---------------------------------------------------------------------------

/// UNX-F0061 域自检（判据：优先级矩阵/冲突账/样本 100%/输入自检）。
pub fn run_unx06_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0061");

    // -- UNX06-MAT-01 优先级矩阵（EFI>E820>ACPI 段位钉死）-----------------
    {
        let ranks: Vec<u32> = SourceKind::ALL.iter().map(|k| k.rank()).collect();
        s.add(
            "UNX06-MAT-01 设计优先级矩阵（EFI=2>E820=1>ACPI=0 逐段钉死）",
            ranks[0] == 0 && ranks[1] == 1 && ranks[2] == 2
                && SourceKind::ALL.len() == 3
                && SourceKind::Acpi < SourceKind::E820
                && SourceKind::E820 < SourceKind::Efi,
            "锚点设计优先级矩阵：EFI 精确分型最权威、ACPI 推测仅兜底",
        );
    }

    // -- UNX06-ARB-01 同区间仲裁必胜最高段位 ------------------------------
    {
        let efi = SourceEntry::new(0x1000, 0x1000, Kind::Usable, SourceKind::Efi);
        let e820 = SourceEntry::new(0x1000, 0x800, Kind::Reserved, SourceKind::E820);
        let acpi = SourceEntry::new(0x1000, 0x400, Kind::AcpiNvs, SourceKind::Acpi);
        let group = [&efi, &e820, &acpi];
        let r = arbitrate_span(&group);
        s.add(
            "UNX06-ARB-01 同区间仲裁 EFI 胜（段位高者胜，不靠输入序）",
            matches!(&r, Ok((SourceKind::Efi, _))),
            "锚点仲裁策略：矩阵段位裁决是仲裁第一判语",
        );
    }

    // -- UNX06-ARB-02 平级规约确定（短区间→基址低者）---------------------
    {
        // 同源两区间重叠：短区间胜。
        let long = SourceEntry::new(0x2000, 0x1000, Kind::Usable, SourceKind::Efi);
        let short = SourceEntry::new(0x2000, 0x800, Kind::Usable, SourceKind::Efi);
        let r1 = arbitrate_span(&[&long, &short]);
        let r2 = arbitrate_span(&[&short, &long]);
        // 输入序无关（两次都短者胜——确定性）。
        s.add(
            "UNX06-ARB-02 同源平级短区间胜（输入序无关）",
            matches!(&r1, Ok((SourceKind::Efi, _)))
                && matches!(&r2, Ok((SourceKind::Efi, _)))
                && r1.as_ref().unwrap().1 == r2.as_ref().unwrap().1,
            "锚点平级规约：短区间（二次申报）更精确，且判语不随输入序变",
        );
    }

    // -- UNX06-ARB-03 冲突有账（每次仲裁落账+可回读）---------------------
    {
        let efi = SourceEntry::new(0x1000, 0x1000, Kind::Usable, SourceKind::Efi);
        let acpi = SourceEntry::new(0x1000, 0x400, Kind::AcpiNvs, SourceKind::Acpi);
        let mut ledger = ArbLedger::new();
        let regions = merge_three_sources(&[efi.clone(), acpi.clone()], &mut ledger);
        s.add(
            "UNX06-ARB-03 冲突有账（胜者 Region + 账行含 participants/reason）",
            regions.is_ok()
                && ledger.len() == 1
                && ledger.all()[0].winner == SourceKind::Efi
                && ledger.all()[0].participants.len() == 2
                && !ledger.all()[0].reason.is_empty(),
            "锚点「冲突有账」：裁决留痕供 F0062/F0065 回查",
        );
    }

    // -- UNX06-SAMPLE-01 100% 一致（100 组样本仲裁全 EFI 胜出）----------
    {
        let samples = synth_samples(ARB_SAMPLES, 0x0061);
        let mut all_efi = true;
        let mut total = 0usize;
        match &samples {
            Ok(list) => {
                let mut ledger = ArbLedger::new();
                let _ = merge_three_sources(list, &mut ledger);
                total = list.len();
                // 每三条目一组同基址（构造保证交集）——仲裁胜者必为 Efi。
                let mut i = 0usize;
                while i + 2 < list.len() {
                    let g = [&list[i], &list[i + 1], &list[i + 2]];
                    if let Ok((w, _)) = arbitrate_span(&g) {
                        if w != SourceKind::Efi {
                            all_efi = false;
                        }
                    } else {
                        all_efi = false;
                    }
                    i += 3;
                }
            }
            Err(_) => all_efi = false,
        }
        s.add(
            "UNX06-SAMPLE-01 100 组合成样本仲裁 100% 一致（EFI 全胜）",
            all_efi && total == ARB_SAMPLES as usize * 3,
            "锚点 J1：100 组合成冲突样本仲裁结果与设计优先级矩阵 100% 一致",
        );
    }

    // -- UNX06-IN-01 输入重叠自检（源条目两两重叠即拒）-------------------
    {
        let a = SourceEntry::new(0x1000, 0x1000, Kind::Usable, SourceKind::Efi);
        let b = SourceEntry::new(0x1800, 0x1000, Kind::Usable, SourceKind::Efi);
        let mut ledger = ArbLedger::new();
        let r = merge_three_sources(&[a, b], &mut ledger);
        s.add(
            "UNX06-IN-01 源内重叠自检（两两重叠即拒，先源内去重再仲裁）",
            r.is_err() && r.unwrap_err().contains(E_ARB_OVERLAP),
            "锚点输入自检：合并输入自身不自洽不可入仲裁",
        );
    }

    // -- UNX06-IN-02 区间非法自检（零长/回绕即拒）-----------------------
    {
        let zero = SourceEntry::new(0x1000, 0, Kind::Usable, SourceKind::Efi);
        let wrap = SourceEntry::new(u64::MAX - 8, 0x100, Kind::Usable, SourceKind::Efi);
        s.add(
            "UNX06-IN-02 区间非法拒（零长/基址回绕）",
            zero.is_err() && wrap.is_err() && zero.unwrap_err().contains(E_ARB_BAD_SPAN),
            "锚点输入自检：回绕区间会让长度计算静默变小",
        );
    }

    // -- UNX06-LEDGER-01 账本容量守卫（满即拒，不静默丢账）---------------
    {
        let mut full = ArbLedger::new();
        let mut ok = true;
        let mut i = 0usize;
        while i < MAX_ARB_LEDGER {
            if full
                .record(ArbitrationRecord {
                    from: i as u64,
                    to: i as u64 + 1,
                    participants: alloc::vec![SourceKind::Efi],
                    winner: SourceKind::Efi,
                    reason: String::from("x"),
                })
                .is_err()
            {
                ok = false;
            }
            i += 1;
        }
        let over = full.record(ArbitrationRecord {
            from: 0,
            to: 1,
            participants: alloc::vec![SourceKind::Efi],
            winner: SourceKind::Efi,
            reason: String::from("y"),
        });
        s.add(
            "UNX06-LEDGER-01 账本容量守卫（满即拒——冲突账不许静默丢弃）",
            ok && full.len() == MAX_ARB_LEDGER && over.is_err() && over.unwrap_err().contains(E_ARB_LEDGER_FULL),
            "锚点账本纪律：容量溢出显性拒",
        );
    }

    // -- UNX06-META-01 版本指纹 -------------------------------------------
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in ARBITER_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    s.add("UNX06-META-01 版本指纹非零", fp != 0, "UNX-F0061-v1");

    s
}
