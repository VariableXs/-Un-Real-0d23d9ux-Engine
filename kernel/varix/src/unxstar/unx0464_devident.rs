//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0464 · 设备身份账（vendor/device/revision）· 0x360C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥220 行；
//!   ② 有效设备身份三元组（vendor/device/revision）账齐（100% 覆盖
//!      有效清单），值域核过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 身份三元组：PCI 配置空间语义建模——VendorID（16 位，0xFFFF =
//!     无设备）、DeviceID（16 位）、RevisionID（8 位）；三元组落账
//!     键 = (slot, vendor, device, revision)；
//!   - 值域核：vendor ∈ 0x0001..0xFFFE（0x0000 非法占用、0xFFFF 无
//!     设备）、revision 全 8 位合法但全 0xFF 记「无修订报告」显性态
//!     （不算失败，算诚实缺席）；vendor==0xFFFF 的槽位不得落身份账
//!     （虚设设备）；
//!   - 100% 覆盖判据：有效清单（声明的 slot 集合）里的每个 slot 都
//!     有账行，且每行值域过；缺行/值域炸都计入欠账；
//!   - 厂商族账：按 vendor 聚合设备数（同 vendor 多 device），族账
//!     与明细账两向守恒（Σ族 = Σ明细）；
//!   - 一次性登记：同 slot 重复登记显性拒（防账面重写），换值登记
//!     视为冲突事件入冲突账（不静默覆盖）；
//!   - 注入面：虚设设备（vendor=0xFFFF）、非法 vendor（0x0000）、
//!     重复登记、slot 越界、清单缺行 五族，LCG 种子写死 = SEG_BASE
//!     （0x360C0），禁 rand/浮点/IO/墙钟；no_std + alloc；零
//!     unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x360C0，全仓独占）。
pub const SEG_BASE: u32 = 0x360C0;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DevIdCode {
    /// vendor = 0xFFFF（无设备却报身份）
    PhantomDevice,
    /// vendor = 0x0000（非法占用）
    VendorZero,
    /// 重复登记（同 slot 二次落账）
    DuplicateSlot,
    /// slot 越界
    SlotOutOfRange,
    /// 有效清单缺行（覆盖不齐）
    CoverageMissing,
    /// 值域炸（revision 区外非法态——预留）
    ValueOutOfRange,
    /// 冲突登记（同 slot 换值）
    ConflictEntry,
    /// 覆盖账不平（Σ族 ≠ Σ明细）
    FamilyUnbalanced,
}

impl DevIdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DevIdCode::PhantomDevice => "虚设设备：vendor=0xFFFF（无设备语义）不得落身份账",
            DevIdCode::VendorZero => "vendor=0x0000：非法占用值域",
            DevIdCode::DuplicateSlot => "重复登记：同 slot 已有账行，防重写",
            DevIdCode::SlotOutOfRange => "slot 越界：超出登记槽位数",
            DevIdCode::CoverageMissing => "覆盖缺行：有效清单 slot 无账行",
            DevIdCode::ValueOutOfRange => "值域炸：字段超出声明值域",
            DevIdCode::ConflictEntry => "冲突登记：同 slot 换值，入冲突账不静默覆盖",
            DevIdCode::FamilyUnbalanced => "族账不平：厂商族聚合与明细账不守恒",
        }
    }
    pub const ALL: [DevIdCode; 8] = [
        DevIdCode::PhantomDevice,
        DevIdCode::VendorZero,
        DevIdCode::DuplicateSlot,
        DevIdCode::SlotOutOfRange,
        DevIdCode::CoverageMissing,
        DevIdCode::ValueOutOfRange,
        DevIdCode::ConflictEntry,
        DevIdCode::FamilyUnbalanced,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in DevIdCode::ALL {
        if c.val() == code {
            return c.detail();
        }
    }
    "未知诊断码"
}

// ── 常量 ─────────────────────────────────────────────────────────
/// 登记槽位数（PCI 总线 0 设备 0..31 × 常规 8 slot 建模折算）。
pub const SLOT_COUNT: usize = 32;
/// PCI 无设备语义值。
pub const VENDOR_NONE: u16 = 0xFFFF;
/// PCI 非法占用值。
pub const VENDOR_ILLEGAL: u16 = 0x0000;
/// revision「无修订报告」语义值。
pub const REV_NONE_REPORTED: u8 = 0xFF;

// ── 身份三元组 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DevIdentity {
    pub vendor: u16,
    pub device: u16,
    pub revision: u8,
}

impl DevIdentity {
    /// 值域核：vendor ∈ 0x0001..0xFFFE；revision 任意 8 位合法。
    pub fn value_domain_ok(&self) -> bool {
        self.vendor != VENDOR_ILLEGAL && self.vendor != VENDOR_NONE
    }
    /// 是否「无修订报告」态（诚实缺席，不算失败）。
    pub fn rev_absent(&self) -> bool {
        self.revision == REV_NONE_REPORTED
    }
}

/// 一条账行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DevRecord {
    pub slot: usize,
    pub id: DevIdentity,
}

/// 冲突账行（同 slot 换值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConflictRecord {
    pub slot: usize,
    pub old_id: DevIdentity,
    pub new_id: DevIdentity,
}

/// 设备身份总账。
pub struct DevIdLedger {
    pub records: Vec<DevRecord>,
    pub conflicts: Vec<ConflictRecord>,
    pub rejected_total: usize,
    /// 覆盖欠账清单（缺行 slot）。
    pub coverage_gaps: Vec<usize>,
}

impl DevIdLedger {
    pub fn new() -> DevIdLedger {
        DevIdLedger {
            records: Vec::new(),
            conflicts: Vec::new(),
            rejected_total: 0,
            coverage_gaps: Vec::new(),
        }
    }

    /// 登记一条身份：值域核 + 一次性 + 冲突账。
    pub fn register(&mut self, slot: usize, id: DevIdentity) -> Result<(), DevIdCode> {
        if slot >= SLOT_COUNT {
            self.rejected_total += 1;
            return Err(DevIdCode::SlotOutOfRange);
        }
        if id.vendor == VENDOR_NONE {
            self.rejected_total += 1;
            return Err(DevIdCode::PhantomDevice);
        }
        if id.vendor == VENDOR_ILLEGAL {
            self.rejected_total += 1;
            return Err(DevIdCode::VendorZero);
        }
        for r in &self.records {
            if r.slot == slot {
                if r.id == id {
                    self.rejected_total += 1;
                    return Err(DevIdCode::DuplicateSlot);
                }
                self.conflicts.push(ConflictRecord {
                    slot,
                    old_id: r.id,
                    new_id: id,
                });
                self.rejected_total += 1;
                return Err(DevIdCode::ConflictEntry);
            }
        }
        self.records.push(DevRecord { slot, id });
        Ok(())
    }

    pub fn record_of(&self, slot: usize) -> Option<DevRecord> {
        for r in &self.records {
            if r.slot == slot {
                return Some(*r);
            }
        }
        None
    }

    /// 100% 覆盖核：有效清单每 slot 有行且值域过；缺行落欠账清单。
    pub fn coverage_check(&mut self, valid_slots: &[usize]) -> bool {
        self.coverage_gaps.clear();
        let mut all = true;
        for s in valid_slots {
            if *s >= SLOT_COUNT {
                all = false;
                continue;
            }
            match self.record_of(*s) {
                Some(r) => {
                    if !r.id.value_domain_ok() {
                        all = false;
                    }
                }
                None => {
                    self.coverage_gaps.push(*s);
                    all = false;
                }
            }
        }
        all
    }

    /// 厂商族聚合（vendor → 设备数）。
    pub fn family_counts(&self) -> Vec<(u16, usize)> {
        let mut fam: Vec<(u16, usize)> = Vec::new();
        for r in &self.records {
            let mut found = false;
            for f in &mut fam {
                if f.0 == r.id.vendor {
                    f.1 += 1;
                    found = true;
                    break;
                }
            }
            if !found {
                fam.push((r.id.vendor, 1));
            }
        }
        fam
    }

    /// 族账守恒：Σ族 = Σ明细。
    pub fn family_balanced(&self) -> bool {
        let fam = self.family_counts();
        let mut sum = 0;
        for f in &fam {
            sum += f.1;
        }
        sum == self.records.len()
    }

    /// 汇总行（结果摘要，非空判据）。
    pub fn summary_line(&self) -> String {
        format!(
            "设备身份账: 行数={} 冲突={} 拒收={} 覆盖欠账={} 族守恒={}",
            self.records.len(),
            self.conflicts.len(),
            self.rejected_total,
            self.coverage_gaps.len(),
            self.family_balanced()
        )
    }
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 注入面（五族） ───────────────────────────────────────────────
fn drill_phantom_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    for i in 0..10u32 {
        let mut led = DevIdLedger::new();
        let slot = (lcg.next_below(SLOT_COUNT as u64)) as usize;
        let id = DevIdentity {
            vendor: VENDOR_NONE,
            device: 0x1234 + i as u16,
            revision: 0x01,
        };
        if led.register(slot, id) == Err(DevIdCode::PhantomDevice) && led.record_of(slot).is_none() {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("虚设设备注入 {}/10 必拒", caught))
}

fn drill_vendor_zero_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 1);
    let mut caught = 0;
    for i in 0..10u32 {
        let mut led = DevIdLedger::new();
        let slot = (lcg.next_below(SLOT_COUNT as u64)) as usize;
        let id = DevIdentity {
            vendor: VENDOR_ILLEGAL,
            device: 0x5678 + i as u16,
            revision: 0x02,
        };
        if led.register(slot, id) == Err(DevIdCode::VendorZero) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("非法 vendor 注入 {}/10 必拒", caught))
}

fn drill_duplicate_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 2);
    let mut caught = 0;
    for i in 0..10u32 {
        let mut led = DevIdLedger::new();
        let slot = (lcg.next_below(SLOT_COUNT as u64)) as usize;
        let id = DevIdentity {
            vendor: 0x8086,
            device: 0x1000 + i as u16,
            revision: 0x03,
        };
        let r1 = led.register(slot, id);
        let r2 = led.register(slot, id);
        if r1.is_ok() && r2 == Err(DevIdCode::DuplicateSlot) && led.records.len() == 1 {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("重复登记注入 {}/10 必拒", caught))
}

fn drill_conflict_entry_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 3);
    let mut caught = 0;
    for i in 0..10u32 {
        let mut led = DevIdLedger::new();
        let slot = (lcg.next_below(SLOT_COUNT as u64)) as usize;
        let a = DevIdentity {
            vendor: 0x1022,
            device: 0x2000 + i as u16,
            revision: 0x04,
        };
        let b = DevIdentity {
            vendor: 0x1022,
            device: 0x2000 + i as u16 + 1, // 换值
            revision: 0x04,
        };
        let r1 = led.register(slot, a);
        let r2 = led.register(slot, b);
        let conflict_logged = led.conflicts.len() == 1 && led.conflicts[0].old_id == a && led.conflicts[0].new_id == b;
        // 原账行不被静默覆盖。
        let kept = led.record_of(slot).map(|r| r.id) == Some(a);
        if r1.is_ok() && r2 == Err(DevIdCode::ConflictEntry) && conflict_logged && kept {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("换值冲突 {}/10 入冲突账不覆盖", caught))
}

fn drill_slot_range_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 4);
    let mut caught = 0;
    for i in 0..10u32 {
        let mut led = DevIdLedger::new();
        let bad = SLOT_COUNT + (lcg.next_below(16) as usize) + i as usize;
        let id = DevIdentity {
            vendor: 0x10DE,
            device: 0x3000,
            revision: 0x05,
        };
        if led.register(bad, id) == Err(DevIdCode::SlotOutOfRange) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("slot 越界注入 {}/10 必拒", caught))
}

fn drill_coverage_missing() -> (bool, String) {
    // 有效清单缺行 → 覆盖不齐且欠账清单落账。
    let mut led = DevIdLedger::new();
    let _ = led.register(0, DevIdentity { vendor: 0x8086, device: 0x1234, revision: 0x01 });
    // 清单要 0/1/2，账只有 0。
    let full = led.coverage_check(&[0, 1, 2]);
    let gaps_logged = led.coverage_gaps == vec![1, 2];
    (full == false && gaps_logged, format!("覆盖缺行: 欠账={:?} 判定={}", led.coverage_gaps, full))
}

fn drill_coverage_full_100() -> (bool, String) {
    // 100% 覆盖正例：清单齐账。
    let mut led = DevIdLedger::new();
    for slot in 0..8 {
        let _ = led.register(slot, DevIdentity {
            vendor: 0x8086 + slot as u16,
            device: 0xB000 + slot as u16,
            revision: 0x10 + slot as u8,
        });
    }
    let full = led.coverage_check(&[0, 1, 2, 3, 4, 5, 6, 7]);
    let balanced = led.family_balanced();
    (full && balanced && led.coverage_gaps.is_empty(), format!("100% 覆盖: full={} gaps={} 族平={}", full, led.coverage_gaps.len(), balanced))
}

fn drill_family_conservation() -> (bool, String) {
    // 族账两向守恒：多 device 同 vendor 聚合正确。
    let mut led = DevIdLedger::new();
    let _ = led.register(0, DevIdentity { vendor: 0x8086, device: 0x1000, revision: 0x01 });
    let _ = led.register(1, DevIdentity { vendor: 0x8086, device: 0x1001, revision: 0x01 });
    let _ = led.register(2, DevIdentity { vendor: 0x8086, device: 0x1002, revision: 0x02 });
    let _ = led.register(3, DevIdentity { vendor: 0x1022, device: 0x2000, revision: 0x01 });
    let _ = led.register(4, DevIdentity { vendor: 0x10DE, device: 0x3000, revision: 0x03 });
    let fam = led.family_counts();
    // 期望 [(0x8086,3), (0x1022,1), (0x10DE,1)]（按首现序）。
    let expect: Vec<(u16, usize)> = vec![(0x8086, 3), (0x1022, 1), (0x10DE, 1)];
    let mut ok = fam.len() == 3;
    for (i, f) in fam.iter().enumerate() {
        ok = ok && i < expect.len() && f.0 == expect[i].0 && f.1 == expect[i].1;
    }
    ok = ok && led.family_balanced();
    (ok, format!("族聚合: {:?} 守恒={}", fam, led.family_balanced()))
}

fn drill_rev_absent_semantics() -> (bool, String) {
    // revision=0xFF 是诚实缺席：值域仍过（可登记），可识别缺席态。
    let mut led = DevIdLedger::new();
    let id = DevIdentity {
        vendor: 0x1AF4, // virtio 语义示例
        device: 0x1000,
        revision: REV_NONE_REPORTED,
    };
    let r = led.register(0, id);
    let rec = led.record_of(0);
    let ok = r.is_ok()
        && rec.map(|x| x.id.rev_absent()) == Some(true)
        && rec.map(|x| x.id.value_domain_ok()) == Some(true);
    (ok, format!("无修订报告态: 登记={} 缺席识别={}", r.is_ok(), rec.map(|x| x.id.rev_absent()) == Some(true)))
}

fn drill_value_domain_boundary() -> (bool, String) {
    // 值域边界：0x0001 与 0xFFFE 过，0x0000/0xFFFF 拒（双向）。
    let ok1 = DevIdentity { vendor: 0x0001, device: 0, revision: 0 }.value_domain_ok();
    let ok2 = DevIdentity { vendor: 0xFFFE, device: 0, revision: 0 }.value_domain_ok();
    let bad1 = !DevIdentity { vendor: 0x0000, device: 0, revision: 0 }.value_domain_ok();
    let bad2 = !DevIdentity { vendor: 0xFFFF, device: 0, revision: 0 }.value_domain_ok();
    (ok1 && ok2 && bad1 && bad2, format!("值域边界: 0x0001={} 0xFFFE={} 0x0000拒={} 0xFFFF拒={}", ok1, ok2, bad1, bad2))
}

fn drill_slot_boundary() -> (bool, String) {
    // slot 边界：0 与 SLOT_COUNT-1 可登记，SLOT_COUNT 拒。
    let mut led = DevIdLedger::new();
    let id = DevIdentity { vendor: 0x8086, device: 0x1, revision: 0x1 };
    let r1 = led.register(0, id);
    let r2 = led.register(SLOT_COUNT - 1, id);
    let r3 = led.register(SLOT_COUNT, id);
    (r1.is_ok() && r2.is_ok() && r3 == Err(DevIdCode::SlotOutOfRange),
        format!("slot 边界: 首槽={} 末槽={} 越界拒={}", r1.is_ok(), r2.is_ok(), r3.is_err()))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    DevIdCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

// ── 全流程对账（判据 J1 落地面） ─────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let mut led = DevIdLedger::new();
    // 8 槽有效清单全落账（含 1 条无修订报告态）。
    let mut expect: Vec<usize> = Vec::new();
    for slot in 0..8usize {
        let id = if slot == 7 {
            DevIdentity { vendor: 0x1B36, device: 0x0010, revision: REV_NONE_REPORTED }
        } else {
            DevIdentity { vendor: 0x8086, device: 0x1500 + slot as u16, revision: 0x01 + slot as u8 }
        };
        if led.register(slot, id).is_ok() {
            expect.push(slot);
        }
    }
    let cov = led.coverage_check(&expect);
    let balanced = led.family_balanced();
    // 一条坏样本拒收后账面不受污染。
    let bad = led.register(99, DevIdentity { vendor: 0x8086, device: 0, revision: 0 });
    let ok = cov
        && balanced
        && led.records.len() == 8
        && bad == Err(DevIdCode::SlotOutOfRange)
        && led.rejected_total == 1
        && led.summary_line().len() > 0;
    (
        ok,
        format!(
            "全流程: 覆盖={} 族平={} 行数={} [{}]",
            cov, balanced, led.records.len(), led.summary_line()
        ),
    )
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_phantom_10();
    v.push(("J2-虚设设备注入10/10", a, b));
    let (a, b) = drill_vendor_zero_10();
    v.push(("J2-非法vendor注入10/10", a, b));
    let (a, b) = drill_duplicate_10();
    v.push(("J2-重复登记注入10/10", a, b));
    let (a, b) = drill_conflict_entry_10();
    v.push(("J2-换值冲突10/10", a, b));
    let (a, b) = drill_slot_range_10();
    v.push(("J2-slot越界注入10/10", a, b));
    let (a, b) = drill_coverage_missing();
    v.push(("J2-覆盖缺行落账", a, b));
    let (a, b) = drill_coverage_full_100();
    v.push(("J1-100%覆盖正例", a, b));
    let (a, b) = drill_family_conservation();
    v.push(("J3-族账守恒", a, b));
    let (a, b) = drill_rev_absent_semantics();
    v.push(("J3-无修订报告态", a, b));
    let (a, b) = drill_value_domain_boundary();
    v.push(("J3-值域边界双向", a, b));
    let (a, b) = drill_slot_boundary();
    v.push(("J3-slot边界", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx0464_checks：本单判据聚合（诊断段 0x360C0）。
pub fn run_unx0464_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0464 设备身份账");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x360C0 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
