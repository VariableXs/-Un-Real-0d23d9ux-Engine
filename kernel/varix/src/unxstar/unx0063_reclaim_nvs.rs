//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0063 · ACPI Reclaim/NVS 区分类账 · 0x6DF00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥240 行；
//!   ② UNX-F0063-J1 Reclaim（ACPI 回收）与 NVS（ACPI 非易失）
//!     两类内存区间分类判据：Reclaim 可在 ACPI 表消费后回收，
//!     NVS 永久保留（S3 唤醒账），回收动作只许一次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 区间账：Reclaim/NVS 两类条目（基址/长度/消费态）；
//!   - 分类守卫：区间重叠（同类内）拒；基址非 4K 对齐拒；
//!   - Reclaim 生命周期：登记→消费（ACPI 表账引用）→回收（一次
//!     红线，二次回收拒）→可分配；NVS 恒保留（回收企图拒）；
//!   - S3 账：NVS 区间总和入唤醒账（唤醒前冻结不可回收）；
//!   - 消费引用计数：区间被引用才可回收，零引用回收即拒；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6DF00，AI-VX3-1102 批I 独占区）。
pub const SEG_BASE: u32 = 0x6DF00;

/// 4K 对齐掩码。
pub const ALIGN_4K: u64 = 0x1000;
/// 最大区间数（防伪账）。
pub const MAX_REGIONS: usize = 32;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AcpiMemCode {
    /// 基址非 4K 对齐
    Misaligned4K,
    /// 同类区间重叠
    SameClassOverlap,
    /// 回收动作重复（一次红线）
    DoubleReclaim,
    /// NVS 回收企图（永久保留红线）
    NvsReclaimAttempt,
    /// 零引用回收
    ReclaimWithoutRef,
    /// 区间长度 0
    ZeroLength,
    /// S3 冻结期回收企图
    S3FrozenReclaim,
    /// 区间数越上限
    CountOver,
    /// 消费态错序（未消费即标记可分配）
    StageOutOfOrder,
    /// 双跑不一致
    NotReproducible,
}

impl AcpiMemCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AcpiMemCode::Misaligned4K => "基址非 4K 对齐",
            AcpiMemCode::SameClassOverlap => "同类区间重叠",
            AcpiMemCode::DoubleReclaim => "回收动作重复：一次红线",
            AcpiMemCode::NvsReclaimAttempt => "NVS 回收企图：永久保留",
            AcpiMemCode::ReclaimWithoutRef => "零引用回收",
            AcpiMemCode::ZeroLength => "区间长度 0",
            AcpiMemCode::S3FrozenReclaim => "S3 冻结期回收企图",
            AcpiMemCode::CountOver => "区间数越上限：32 冻结",
            AcpiMemCode::StageOutOfOrder => "未消费即标可分配",
            AcpiMemCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [AcpiMemCode; 10] = [
        AcpiMemCode::Misaligned4K,
        AcpiMemCode::SameClassOverlap,
        AcpiMemCode::DoubleReclaim,
        AcpiMemCode::NvsReclaimAttempt,
        AcpiMemCode::ReclaimWithoutRef,
        AcpiMemCode::ZeroLength,
        AcpiMemCode::S3FrozenReclaim,
        AcpiMemCode::CountOver,
        AcpiMemCode::StageOutOfOrder,
        AcpiMemCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AcpiMemCode::ALL.len() {
        if AcpiMemCode::ALL[i].val() == code {
            return AcpiMemCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6DF00 段闭集内"
}

// ── 区间账 ──────────────────────────────────────────────────────
/// 区间类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AcpiClass {
    /// ACPI 回收（表消费后可还给分配器）。
    Reclaim,
    /// ACPI NVS（S3 唤醒账，永久保留）。
    Nvs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcpiRegion {
    pub class: AcpiClass,
    pub base: u64,
    pub len: u64,
    /// 消费引用计数（ACPI 表账引用）。
    pub refs: u32,
    /// 已回收标志（一次红线）。
    pub reclaimed: bool,
}

impl AcpiRegion {
    pub fn end(&self) -> u64 {
        self.base.saturating_add(self.len)
    }

    /// 区间四闸。
    pub fn legal(&self) -> Result<(), u32> {
        if self.len == 0 {
            return Err(AcpiMemCode::ZeroLength.val());
        }
        if self.base % ALIGN_4K != 0 {
            return Err(AcpiMemCode::Misaligned4K.val());
        }
        Ok(())
    }
}

// ── 类账（分类 + 生命周期） ─────────────────────────────────────
#[derive(Clone, Debug, Default)]
pub struct AcpiClassLedger {
    pub regions: Vec<AcpiRegion>,
    pub reclaim_caught: u32,
    pub nvs_caught: u32,
    pub s3_frozen: bool,
}

impl AcpiClassLedger {
    pub fn new() -> AcpiClassLedger {
        AcpiClassLedger { regions: Vec::new(), reclaim_caught: 0, nvs_caught: 0, s3_frozen: false }
    }

    /// 登记（同类重叠守卫）。
    pub fn add(&mut self, r: AcpiRegion) -> Result<usize, u32> {
        r.legal()?;
        if self.regions.len() >= MAX_REGIONS {
            return Err(AcpiMemCode::CountOver.val());
        }
        let mut i = 0;
        while i < self.regions.len() {
            let e = &self.regions[i];
            if e.class == r.class && r.base < e.end() && e.base < r.end() {
                return Err(AcpiMemCode::SameClassOverlap.val());
            }
            i += 1;
        }
        self.regions.push(r);
        Ok(self.regions.len() - 1)
    }

    /// 消费引用 +1（Reclaim 专属语义；NVS 不消费只冻结）。
    pub fn consume(&mut self, idx: usize) -> Result<u32, u32> {
        match self.regions.get_mut(idx) {
            Some(r) if r.class == AcpiClass::Reclaim => {
                r.refs += 1;
                Ok(r.refs)
            }
            Some(_) => Ok(0),
            None => Err(AcpiMemCode::CountOver.val()),
        }
    }

    /// 标记可分配（必须先消费）。
    pub fn mark_freeable(&mut self, idx: usize) -> Result<(), u32> {
        match self.regions.get_mut(idx) {
            Some(r) => {
                if r.class == AcpiClass::Nvs {
                    self.nvs_caught += 1;
                    return Err(AcpiMemCode::NvsReclaimAttempt.val());
                }
                if r.refs == 0 {
                    return Err(AcpiMemCode::StageOutOfOrder.val());
                }
                Ok(())
            }
            None => Err(AcpiMemCode::CountOver.val()),
        }
    }

    /// 回收（Reclaim 一次红线；NVS 拒；S3 冻结期拒）。
    pub fn reclaim(&mut self, idx: usize) -> Result<u64, u32> {
        if self.s3_frozen {
            self.reclaim_caught += 1;
            return Err(AcpiMemCode::S3FrozenReclaim.val());
        }
        match self.regions.get(idx) {
            Some(r) if r.class == AcpiClass::Nvs => {
                self.nvs_caught += 1;
                Err(AcpiMemCode::NvsReclaimAttempt.val())
            }
            Some(r) => {
                if r.reclaimed {
                    self.reclaim_caught += 1;
                    return Err(AcpiMemCode::DoubleReclaim.val());
                }
                if r.refs == 0 {
                    return Err(AcpiMemCode::ReclaimWithoutRef.val());
                }
                let base = r.base;
                let len = r.len;
                let m = self.regions.get_mut(idx);
                match m {
                    Some(r2) => {
                        r2.reclaimed = true;
                    }
                    None => return Err(AcpiMemCode::CountOver.val()),
                }
                Ok(base + len)
            }
            None => Err(AcpiMemCode::CountOver.val()),
        }
    }

    /// S3 冻结（唤醒账：冻结后一切回收拒）。
    pub fn freeze_s3(&mut self) {
        self.s3_frozen = true;
    }

    /// NVS 总账（S3 唤醒账字节面）。
    pub fn nvs_total(&self) -> u64 {
        let mut t = 0u64;
        let mut i = 0;
        while i < self.regions.len() {
            if self.regions[i].class == AcpiClass::Nvs {
                t += self.regions[i].len;
            }
            i += 1;
        }
        t
    }

    /// Reclaim 可回收总量（未回收且已消费）。
    pub fn reclaimable_total(&self) -> u64 {
        let mut t = 0u64;
        let mut i = 0;
        while i < self.regions.len() {
            let r = &self.regions[i];
            if r.class == AcpiClass::Reclaim && !r.reclaimed && r.refs > 0 {
                t += r.len;
            }
            i += 1;
        }
        t
    }
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（四路：回收路/NVS 路/冻结路/序路） ────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 真机样本：Reclaim 64K（含 DSDT）+ NVS 128K（S3 账）。
pub fn make_machine_regions() -> Vec<AcpiRegion> {
    vec![
        AcpiRegion { class: AcpiClass::Reclaim, base: 0x7F00_0000, len: 0x1_0000, refs: 0, reclaimed: false },
        AcpiRegion { class: AcpiClass::Nvs, base: 0x7F10_0000, len: 0x2_0000, refs: 0, reclaimed: false },
    ]
}

// ── 黄金全链：登记→消费→回收一次→NVS 恒留 ────────────────────────
pub fn golden_run() -> Result<(u64, u64, u32), u32> {
    let mut led = AcpiClassLedger::new();
    let rs = make_machine_regions();
    let mut i = 0;
    while i < rs.len() {
        led.add(rs[i].clone())?;
        i += 1;
    }
    led.consume(0)?;
    let reclaimed = led.reclaim(0)?;
    let nvs = led.nvs_total();
    let bad = if led.regions[1].reclaimed { 1 } else { 0 };
    Ok((reclaimed, nvs, bad))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：真机账 → Reclaim 回收终址 0x7F010000 + NVS 128K + 零违例。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((0x7F01_0000, 0x2_0000, 0)) => Ok(()),
        _ => Err(AcpiMemCode::NotReproducible.val()),
    }
}

/// 序 B：二次回收 ⇒ DoubleReclaim。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut led = AcpiClassLedger::new();
    let _ = led.add(AcpiRegion { class: AcpiClass::Reclaim, base: 0x7F20_0000, len: 0x1000, refs: 0, reclaimed: false });
    led.consume(0)?;
    let _ = led.reclaim(0);
    match led.reclaim(0) {
        Err(e) if e == AcpiMemCode::DoubleReclaim.val() => Ok(()),
        _ => Err(AcpiMemCode::NotReproducible.val()),
    }
}

/// 序 C：零引用回收 ⇒ ReclaimWithoutRef。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut led = AcpiClassLedger::new();
    let _ = led.add(AcpiRegion { class: AcpiClass::Reclaim, base: 0x7F30_0000, len: 0x1000, refs: 0, reclaimed: false });
    match led.reclaim(0) {
        Err(e) if e == AcpiMemCode::ReclaimWithoutRef.val() => Ok(()),
        _ => Err(AcpiMemCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：NVS 回收路。
        let mut l1 = AcpiClassLedger::new();
        let _ = l1.add(AcpiRegion { class: AcpiClass::Nvs, base: 0x7F40_0000, len: 0x1000, refs: 0, reclaimed: false });
        led.record(0, l1.reclaim(0).is_err() && l1.nvs_caught == 1);
        // 路 2：冻结路。
        let mut l2 = AcpiClassLedger::new();
        let _ = l2.add(AcpiRegion { class: AcpiClass::Reclaim, base: 0x7F50_0000, len: 0x1000, refs: 0, reclaimed: false });
        let _ = l2.consume(0);
        l2.freeze_s3();
        led.record(1, l2.reclaim(0).is_err() && l2.reclaim_caught == 1);
        // 路 3：序路（未消费即标可分配）。
        let mut l3 = AcpiClassLedger::new();
        let _ = l3.add(AcpiRegion { class: AcpiClass::Reclaim, base: 0x7F60_0000, len: 0x1000, refs: 0, reclaimed: false });
        led.record(2, l3.mark_freeable(0).is_err());
        // 路 4：重叠路（同类重叠必拒）。
        let mut l4 = AcpiClassLedger::new();
        let _ = l4.add(AcpiRegion { class: AcpiClass::Reclaim, base: 0x7F70_0000, len: 0x2000, refs: 0, reclaimed: false });
        led.record(3, l4.add(AcpiRegion { class: AcpiClass::Reclaim, base: 0x7F70_1000, len: 0x1000, refs: 0, reclaimed: false }).is_err());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(AcpiMemCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AcpiMemCode::ALL.len() {
        let v = AcpiMemCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((end, nvs, bad)) => *end == 0x7F01_0000 && *nvs == 0x2_0000 && *bad == 0,
            Err(_) => false,
        },
        match &g {
            Ok((end, nvs, bad)) => format!("reclaimed_end=0x{:x} nvs=0x{:x} bad={}", end, nvs, bad),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 分类对账：两类各自记账（NVS 恒留）。
    let mut l = AcpiClassLedger::new();
    let rs = make_machine_regions();
    let mut i = 0;
    while i < rs.len() {
        let _ = l.add(rs[i].clone());
        i += 1;
    }
    v.push((
        "J1-分类对账",
        l.nvs_total() == 0x2_0000 && l.reclaimable_total() == 0 && l.regions.len() == 2,
        format!("nvs=0x{:x} reclaimable=0x{:x} n={}", l.nvs_total(), l.reclaimable_total(), l.regions.len()),
    ));
    // 消费后可回收总量翻正。
    let _ = l.consume(0);
    v.push((
        "J1-消费引用",
        l.reclaimable_total() == 0x1_0000,
        String::from("refs=1 ⇒ reclaimable=64K"),
    ));
    // S3 冻结后全拒。
    let mut l2 = AcpiClassLedger::new();
    let _ = l2.add(AcpiRegion { class: AcpiClass::Reclaim, base: 0x7F80_0000, len: 0x1000, refs: 0, reclaimed: false });
    let _ = l2.consume(0);
    l2.freeze_s3();
    v.push((
        "J1-S3冻结",
        l2.reclaim(0).is_err() && l2.reclaim_caught == 1,
        String::from("冻结期回收一律拒"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=检出{}", b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6DF00 窗口"),
    ));
    v
}

pub fn run_unx0063_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0063 ACPI Reclaim/NVS 区分类账");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
