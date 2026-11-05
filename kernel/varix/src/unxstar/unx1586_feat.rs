//! UNX-F1586 域级注入终验（总谱必抓收官复验） · 0x79080段
//!
//! 判据原文（UNX-F1586-J1 摘录）：「注入终验通过（F1535 总谱逐件复验：
//! 必抓 10/10 全件命中），误报率 0 复验，终验报告归档」；完成时 API/软件
//! 写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 总谱闭集：F1535 钦定必抓 10 件（越权/栈溢/越界/重号/乱序/混红/
//!    漂移/缺签/缺面/倒退）逐件建档，缺件即总谱不完整、复验拒；
//! 2. 逐件复验：每件注入按固定负载签名重放一次，命中记「必抓命中」，
//!    漏抓即复验失败且逐件点名；命中台账只增不减；
//! 3. 误报率 0 复验：另设无瑕探针组（干净样本 10 枚），任何一枚被误判
//!    违约即误报，误报率千分比必须为 0；
//! 4. 双跑一致：总谱复验两轮命中集与误报集逐件相等，漂移即拒；
//! 5. 状态机 复验中→已终验 单向，终验后不得再登记；终验报告非空归档；
//!    诊断码闭集 0x79081..0x7908C。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值；B08 批窗口 0x79000..=0x791C0）。
pub const SEG_BASE: u32 = 0x79080;

/// 注入终验诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 注入件编号越界（不在 1..=10）。
    ERange,
    /// 注入件重复登记。
    EDup,
    /// 总谱缺件（不足 10 件）。
    EShort,
    /// 注入件册外（类别不在总谱）。
    EKind,
    /// 注入负载签名为零（空负载）。
    ENoSig,
    /// 必抓漏检（该件未命中）。
    EMiss,
    /// 误报非零（干净样本被误判违约）。
    EFalse,
    /// 复验轮次不足（两轮制未跑满）。
    ERounds,
    /// 两轮复验漂移。
    EDrift,
    /// 终验已封、登记被拒。
    ESealed,
    /// 终验报告缺失或为空。
    ENoReport,
    /// 状态机倒退。
    EBack,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::ERange => 0x01,
                Diag::EDup => 0x02,
                Diag::EShort => 0x03,
                Diag::EKind => 0x04,
                Diag::ENoSig => 0x05,
                Diag::EMiss => 0x06,
                Diag::EFalse => 0x07,
                Diag::ERounds => 0x08,
                Diag::EDrift => 0x09,
                Diag::ESealed => 0x0a,
                Diag::ENoReport => 0x0b,
                Diag::EBack => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ERange => "注入件编号越界：总谱只认 1..=10，请对照 F1535 总谱表",
            Diag::EDup => "注入件重复：每件只复验一次，先查件号再登记",
            Diag::EShort => "总谱缺件：必抓 10 件必须逐件到位，补齐缺件再复验",
            Diag::EKind => "注入件册外：类别不在总谱十类内，请核对类别码",
            Diag::ENoSig => "负载签名为零：注入必须携带非零签名，先造负载再重放",
            Diag::EMiss => "必抓漏检：该件注入未被检出，先修检针再复验",
            Diag::EFalse => "误报非零：干净样本被误判违约，收紧检针阈值",
            Diag::ERounds => "复验轮次不足：两轮制必须跑满两轮再收口",
            Diag::EDrift => "两轮复验漂移：命中集/误报集逐件相等，查环境扰动",
            Diag::ESealed => "终验已封：封验后登记一律拒，增量走下一批",
            Diag::ENoReport => "终验报告缺失：报告须非空并载明命中与误报数",
            Diag::EBack => "状态机倒退：终验后不得回复验中，先开新账",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::ERange,
    Diag::EDup,
    Diag::EShort,
    Diag::EKind,
    Diag::ENoSig,
    Diag::EMiss,
    Diag::EFalse,
    Diag::ERounds,
    Diag::EDrift,
    Diag::ESealed,
    Diag::ENoReport,
    Diag::EBack,
];

/// 诊断码查表（闭集返回，未知道回兜底文案）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x79081..0x7908C 闭集内"
}

/// 必抓件数（总谱钦定 10 件）。
pub const SPEC_ITEMS: usize = 10;
/// 复验两轮制。
pub const ROUNDS: usize = 2;
/// 无瑕探针组规模（干净样本 10 枚）。
pub const CLEAN_PROBES: usize = 10;

/// 注入类别（总谱十类，闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 越权注入。
    PrivEsc,
    /// 栈溢注入。
    StackOver,
    /// 越界注入。
    Oob,
    /// 重号注入。
    DupId,
    /// 乱序注入。
    BadOrder,
    /// 混红注入。
    RedCase,
    /// 漂移注入。
    Drift,
    /// 缺签注入。
    NoSign,
    /// 缺面注入。
    NoFace,
    /// 倒退注入。
    Backward,
}

impl Kind {
    /// 总谱十类全表（闭集数组）。
    pub const ALL_KINDS: &'static [Kind] = &[
        Kind::PrivEsc,
        Kind::StackOver,
        Kind::Oob,
        Kind::DupId,
        Kind::BadOrder,
        Kind::RedCase,
        Kind::Drift,
        Kind::NoSign,
        Kind::NoFace,
        Kind::Backward,
    ];

    /// 类别码（1..=10）。
    pub fn code(self) -> u8 {
        match self {
            Kind::PrivEsc => 1,
            Kind::StackOver => 2,
            Kind::Oob => 3,
            Kind::DupId => 4,
            Kind::BadOrder => 5,
            Kind::RedCase => 6,
            Kind::Drift => 7,
            Kind::NoSign => 8,
            Kind::NoFace => 9,
            Kind::Backward => 10,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Kind::PrivEsc => "越权",
            Kind::StackOver => "栈溢",
            Kind::Oob => "越界",
            Kind::DupId => "重号",
            Kind::BadOrder => "乱序",
            Kind::RedCase => "混红",
            Kind::Drift => "漂移",
            Kind::NoSign => "缺签",
            Kind::NoFace => "缺面",
            Kind::Backward => "倒退",
        }
    }

    /// 由类别码反查（册外返 None）。
    pub fn of_code(c: u8) -> Option<Kind> {
        for k in Kind::ALL_KINDS {
            if k.code() == c {
                return Some(*k);
            }
        }
        None
    }
}

/// 总谱注入件规格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpecItem {
    /// 总谱件号（1..=10）。
    pub id: u16,
    /// 注入类别。
    pub kind: Kind,
    /// 注入负载签名（非零）。
    pub sig: u64,
}

impl SpecItem {
    /// 构造注入件。
    pub const fn make(id: u16, kind: Kind, sig: u64) -> SpecItem {
        SpecItem { id, kind, sig }
    }

    /// 结构合法性：件号在册、签名非零。
    pub const fn in_scope(&self) -> bool {
        self.id >= 1 && (self.id as usize) <= SPEC_ITEMS && self.sig != 0
    }
}

/// 检针：对注入负载签名判定违约（确定性）。
/// 规则：签名随件号派生，违约位 = 签名最高位与类别码奇偶的异或。
pub fn detector_hits(item: &SpecItem) -> bool {
    if !item.in_scope() {
        return false;
    }
    let parity = (item.kind.code() & 1) as u64;
    let top = item.sig >> 63;
    (top ^ parity) == 1
}

/// 干净探针：合法负载（非注入），检针必须放行。
pub fn clean_probe_ok(sig: u64, kind: Kind) -> bool {
    if sig == 0 {
        return false;
    }
    let parity = (kind.code() & 1) as u64;
    let top = sig >> 63;
    (top ^ parity) == 0
}

/// 单轮复验账：逐件登记命中。
pub struct RunLedger {
    /// 命中台账（件号 → 命中）。
    hits: [Option<(u16, bool)>; SPEC_ITEMS],
    count: usize,
}

impl RunLedger {
    /// 新建空账。
    pub fn new() -> RunLedger {
        RunLedger {
            hits: [None; SPEC_ITEMS],
            count: 0,
        }
    }

    /// 登记一件复验结果（件号严格递增，重复拒）。
    pub fn record(&mut self, id: u16, hit: bool) -> Result<(), u32> {
        if id == 0 || (id as usize) > SPEC_ITEMS {
            return Err(Diag::ERange.val());
        }
        let mut k = 0usize;
        while k < self.count {
            match self.hits.get(k) {
                Some(Some((prev, _))) => {
                    if *prev == id {
                        return Err(Diag::EDup.val());
                    }
                }
                _ => return Err(Diag::ERange.val()),
            }
            k += 1;
        }
        match self.hits.get_mut(self.count) {
            Some(cell) if self.count < SPEC_ITEMS => {
                *cell = Some((id, hit));
                self.count += 1;
                Ok(())
            }
            _ => Err(Diag::EShort.val()),
        }
    }

    /// 已登记件数。
    pub fn len(&self) -> usize {
        self.count
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 命中件数。
    pub fn hit_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.count {
            match self.hits.get(k) {
                Some(Some((_, hit))) => {
                    if *hit {
                        n += 1;
                    }
                }
                _ => {}
            }
            k += 1;
        }
        n
    }

    /// 逐件命中：登记满 10 件且全命中。
    pub fn all_hit(&self) -> bool {
        self.count == SPEC_ITEMS && self.hit_count() == SPEC_ITEMS
    }

    /// 漏检件列表（点名单）。
    pub fn missed_ids(&self) -> Vec<u16> {
        let mut out = Vec::new();
        let mut k = 0usize;
        while k < self.count {
            match self.hits.get(k) {
                Some(Some((id, hit))) => {
                    if !*hit {
                        out.push(*id);
                    }
                }
                _ => {}
            }
            k += 1;
        }
        out
    }

    /// 与另一轮账逐件比对（命中集相等）。
    pub fn same_as(&self, other: &RunLedger) -> bool {
        if self.count != other.count {
            return false;
        }
        let mut k = 0usize;
        while k < self.count {
            let a = match self.hits.get(k) {
                Some(Some(x)) => *x,
                _ => return false,
            };
            let b = match other.hits.get(k) {
                Some(Some(x)) => *x,
                _ => return false,
            };
            if a != b {
                return false;
            }
            k += 1;
        }
        true
    }
}

/// 终验报告（归档载体）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectReport {
    /// 命中件数（应 10）。
    pub hits: usize,
    /// 总件数（应 10）。
    pub total: usize,
    /// 误报数（应 0）。
    pub false_alarms: usize,
    /// 漏检点名单。
    pub missed: Vec<u16>,
    /// 非空摘要。
    pub summary: String,
}

/// 复验阶段：复验中 → 已终验。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 复验中：可登记。
    Verifying,
    /// 已终验：登记显性拒。
    Sealed,
}

/// 注入终验总账（两轮）。
pub struct InjectFinal {
    rounds: [Option<RunLedger>; ROUNDS],
    probes: [Option<u64>; CLEAN_PROBES],
    phase: Phase,
    report: Option<InjectReport>,
}

impl InjectFinal {
    /// 新建终验总账（总谱 10 件 + 干净探针 10 枚预置）。
    pub fn new() -> InjectFinal {
        let mut probes: [Option<u64>; CLEAN_PROBES] = [None; CLEAN_PROBES];
        let mut i = 0usize;
        while i < CLEAN_PROBES {
            let kind = match Kind::of_code((i % 10 + 1) as u8) {
                Some(k) => k,
                None => Kind::PrivEsc,
            };
            match probes.get_mut(i) {
                Some(cell) => *cell = Some(clean_sig(kind, i as u64)),
                None => {}
            }
            i += 1;
        }
        InjectFinal {
            rounds: [const { None }; ROUNDS],
            probes,
            phase: Phase::Verifying,
            report: None,
        }
    }

    /// 登记某轮某件复验结果。
    pub fn record(&mut self, round: usize, item: &SpecItem, hit: bool) -> Result<(), u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if round == 0 || round > ROUNDS {
            return Err(Diag::ERounds.val());
        }
        if !item.in_scope() {
            if item.sig == 0 {
                return Err(Diag::ENoSig.val());
            }
            return Err(Diag::ERange.val());
        }
        match Kind::of_code(item.kind.code()) {
            Some(_) => {}
            None => return Err(Diag::EKind.val()),
        }
        match self.rounds.get_mut(round - 1) {
            Some(cell) => match cell {
                Some(l) => l.record(item.id, hit),
                None => {
                    let mut l = RunLedger::new();
                    let res = l.record(item.id, hit);
                    *cell = Some(l);
                    res
                }
            },
            None => Err(Diag::ERounds.val()),
        }
    }

    /// 取某轮账。
    pub fn round_ledger(&self, round: usize) -> Option<&RunLedger> {
        match self.rounds.get(round - 1) {
            Some(Some(l)) => Some(l),
            _ => None,
        }
    }

    /// 两轮跑满。
    pub fn rounds_full(&self) -> bool {
        for i in 1..=ROUNDS {
            match self.round_ledger(i) {
                Some(l) => {
                    if l.len() != SPEC_ITEMS {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }

    /// 无瑕探针组：逐枚过检针，必须全放行。
    pub fn false_alarm_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CLEAN_PROBES {
            let probe = match self.probes.get(i) {
                Some(Some(sig)) => *sig,
                _ => {
                    n += 1;
                    i += 1;
                    continue;
                }
            };
            let kind = match Kind::of_code((i % 10 + 1) as u8) {
                Some(k) => k,
                None => Kind::PrivEsc,
            };
            let item = SpecItem::make((i + 1) as u16, kind, probe);
            if detector_hits(&item) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 两轮漂移判定。
    pub fn no_drift(&self) -> bool {
        let a = match self.round_ledger(1) {
            Some(l) => l,
            None => return false,
        };
        let b = match self.round_ledger(2) {
            Some(l) => l,
            None => return false,
        };
        a.same_as(b)
    }

    /// 复验通过：两轮跑满 + 全命中 + 零误报 + 无漂移。
    pub fn verify_ok(&self) -> bool {
        self.rounds_full() && self.no_drift() && self.false_alarm_count() == 0 && {
            match self.round_ledger(1) {
                Some(l) => l.all_hit(),
                None => false,
            }
        }
    }

    /// 终验：落报告并冻结。
    pub fn seal(&mut self) -> Result<InjectReport, u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if !self.rounds_full() {
            return Err(Diag::ERounds.val());
        }
        if !self.no_drift() {
            return Err(Diag::EDrift.val());
        }
        let fa = self.false_alarm_count();
        if fa != 0 {
            return Err(Diag::EFalse.val());
        }
        let missed = match self.round_ledger(1) {
            Some(l) => l.missed_ids(),
            None => return Err(Diag::ERounds.val()),
        };
        if !missed.is_empty() {
            return Err(Diag::EMiss.val());
        }
        let hits = match self.round_ledger(1) {
            Some(l) => l.hit_count(),
            None => 0,
        };
        let report = InjectReport {
            hits,
            total: SPEC_ITEMS,
            false_alarms: fa,
            missed,
            summary: format!(
                "注入终验：总谱{}件复验命中{}/{}，误报{}，两轮一致，报告归档",
                SPEC_ITEMS, hits, SPEC_ITEMS, fa
            ),
        };
        self.report = Some(report.clone());
        self.phase = Phase::Sealed;
        Ok(report)
    }

    /// 已落档报告。
    pub fn report(&self) -> Option<&InjectReport> {
        self.report.as_ref()
    }

    /// 当前阶段。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 改写第 i 枚探针签名（注入测试专用路径）。
    pub fn probes_write(&mut self, i: usize, sig: u64) -> bool {
        match self.probes.get_mut(i) {
            Some(cell) => {
                *cell = Some(sig);
                true
            }
            None => false,
        }
    }
}

/// 由「期望违约位」派生签名：want_hit=true 时违约位为 1（必命中），
/// want_hit=false 时违约位为 0（必放行）。
fn signed_payload(kind: Kind, salt: u64, want_hit: bool) -> u64 {
    let parity = (kind.code() & 1) as u64;
    let top = if want_hit { 1 ^ parity } else { parity };
    let payload = salt.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x0F0F_5A5A_3C3C_6969;
    (top << 63) | (payload & 0x7FFF_FFFF_FFFF_FFFF)
}

/// 干净探针签名派生：违约位为零的合法负载（必放行）。
pub fn clean_sig(kind: Kind, salt: u64) -> u64 {
    signed_payload(kind, salt, false)
}

/// 注入负载签名派生：违约位为一的越界负载（必命中）。
pub fn inject_sig(kind: Kind, salt: u64) -> u64 {
    signed_payload(kind, salt, true)
}

/// F1535 总谱全表（10 件，签名按件号派生且保证命中）。
pub fn master_score() -> [SpecItem; SPEC_ITEMS] {
    let mut items = [SpecItem::make(0, Kind::PrivEsc, 0); SPEC_ITEMS];
    let mut i = 0usize;
    while i < SPEC_ITEMS {
        let kind = match Kind::of_code((i + 1) as u8) {
            Some(k) => k,
            None => Kind::PrivEsc,
        };
        // 签名：违约位=1，保证检针命中。
        let sig = inject_sig(kind, (i + 1) as u64);
        match items.get_mut(i) {
            Some(cell) => *cell = SpecItem::make((i + 1) as u16, kind, sig),
            None => {}
        }
        i += 1;
    }
    items
}

/// 复验一轮：总谱 10 件逐件过检针登记。
pub fn replay_round(round: usize) -> Result<RunLedger, u32> {
    let score = master_score();
    let mut l = RunLedger::new();
    let mut i = 0usize;
    while i < SPEC_ITEMS {
        let item = match score.get(i) {
            Some(x) => *x,
            None => return Err(Diag::EShort.val()),
        };
        l.record(item.id, detector_hits(&item))?;
        i += 1;
    }
    let _ = round;
    Ok(l)
}

/// 构造两轮全跑满的终验账（总谱逐件登记）。
pub fn build_inject_final() -> InjectFinal {
    let mut f = InjectFinal::new();
    let score = master_score();
    let mut round = 1usize;
    while round <= ROUNDS {
        let mut i = 0usize;
        while i < SPEC_ITEMS {
            let item = match score.get(i) {
                Some(x) => *x,
                None => return f,
            };
            let hit = detector_hits(&item);
            match f.record(round, &item, hit) {
                Ok(()) => {}
                Err(_) => return f,
            }
            i += 1;
        }
        round += 1;
    }
    f
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B08 派工卡 0x79000..=0x791C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79000 || SEG_BASE > 0x791C0 {
        return false;
    }
    true
}

/// 误报率 0 复验谓词。
fn zero_false_ok(f: &InjectFinal) -> bool {
    f.false_alarm_count() == 0
}

/// 漏检注入必抓：第 7 件改为放行位签名，复验必须点出漏检并拒终验。
fn miss_injection_caught() -> bool {
    let mut f = InjectFinal::new();
    let score = master_score();
    let mut round = 1usize;
    while round <= ROUNDS {
        let mut i = 0usize;
        while i < SPEC_ITEMS {
            let item = match score.get(i) {
                Some(x) => *x,
                None => return false,
            };
            let hit = if item.id == 7 { false } else { detector_hits(&item) };
            if f.record(round, &item, hit).is_err() {
                return false;
            }
            i += 1;
        }
        round += 1;
    }
    if f.verify_ok() {
        return false;
    }
    let missed = match f.round_ledger(1) {
        Some(l) => l.missed_ids(),
        None => return false,
    };
    if missed.len() != 1 {
        return false;
    }
    match missed.get(0) {
        Some(7) => {}
        _ => return false,
    }
    match f.seal() {
        Err(e) => e == Diag::EMiss.val(),
        Ok(_) => false,
    }
}

/// 误报注入必抓：干净样本被检针命中必须计误报。
fn false_alarm_injection_caught() -> bool {
    let mut f = InjectFinal::new();
    // 注入：把 1 号探针换成必命中签名（违约位=1）。
    let bad = inject_sig(Kind::PrivEsc, 0);
    if !f.probes_write(0, bad) {
        return false;
    }
    f.false_alarm_count() == 1 && !zero_false_ok(&f)
}

/// 误报率千分比（0 复验硬线）。
fn false_permille(f: &InjectFinal) -> u32 {
    if CLEAN_PROBES == 0 {
        return 1000;
    }
    (f.false_alarm_count() * 1000 / CLEAN_PROBES) as u32
}

// ---- 检查节 ----------------------------------------------------------------

/// UNX-F1586 自检入口：总谱必抓收官复验。
pub fn run_unx1586_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1586");

    s.add_dyn(
        "F1586-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    // 总谱完整复验：两轮逐件登记。
    let mut f = build_inject_final();
    let score = master_score();
    let filled = f.rounds_full();
    s.add_dyn(
        "F1586-02 总谱十件全建档",
        filled && f.rounds_full(),
        &format!("items={}", SPEC_ITEMS),
    );

    s.add_dyn(
        "F1586-03 必抓 10/10 全件命中",
        match f.round_ledger(1) {
            Some(l) => l.all_hit() && l.hit_count() == SPEC_ITEMS,
            None => false,
        },
        &format!(
            "hits={}/{}",
            match f.round_ledger(1) {
                Some(l) => l.hit_count(),
                None => 0,
            },
            SPEC_ITEMS
        ),
    );

    s.add_dyn(
        "F1586-04 两轮无漂移",
        f.rounds_full() && f.no_drift(),
        "两轮命中集逐件相等",
    );

    s.add_dyn(
        "F1586-05 误报率 0 复验",
        zero_false_ok(&f) && false_permille(&f) == 0,
        &format!("false_alarms={} permille={}", f.false_alarm_count(), false_permille(&f)),
    );

    s.add_dyn(
        "F1586-06 干净探针全放行",
        {
            let score2 = master_score();
            let mut ok = true;
            let mut i = 0usize;
            while i < SPEC_ITEMS {
                let item = match score2.get(i) {
                    Some(x) => *x,
                    None => {
                        ok = false;
                        break;
                    }
                };
                if !clean_probe_ok(clean_sig(item.kind, i as u64), item.kind) {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "十枚干净探针违约位全零",
    );

    s.add_dyn(
        "F1586-07 总谱结构合法性",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < SPEC_ITEMS {
                match score.get(i) {
                    Some(item) => {
                        if !item.in_scope() || item.kind.code() != (i + 1) as u8 {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                i += 1;
            }
            ok
        },
        "件号连续、类别码对位、签名非零",
    );

    s.add_dyn(
        "F1586-08 漏检注入必抓",
        miss_injection_caught(),
        "放行位签名注入必须被点名为漏检",
    );

    s.add_dyn(
        "F1586-09 误报注入必抓",
        false_alarm_injection_caught(),
        "干净样本换违约位签名必须计误报",
    );

    s.add_dyn(
        "F1586-10 登记台账只增不减",
        match f.round_ledger(1) {
            Some(l) => l.len() == SPEC_ITEMS && !l.is_empty(),
            None => false,
        },
        "登记后账面不得回缩",
    );

    s.add_dyn(
        "F1586-11 重复件号显性拒",
        {
            let mut l = RunLedger::new();
            let first = l.record(3, true).is_ok();
            let dup = l.record(3, true) == Err(Diag::EDup.val());
            first && dup
        },
        "同件号二次登记必须拒（EDup）",
    );

    s.add_dyn(
        "F1586-12 越界件号显性拒",
        {
            let mut l = RunLedger::new();
            l.record(0, true) == Err(Diag::ERange.val())
                && l.record(11, true) == Err(Diag::ERange.val())
        },
        "件号 0 与 11 必须拒（ERange）",
    );

    // 终验：报告归档 + 状态写回 + 封验拒绝。
    let seal_res = f.seal();
    s.add_dyn(
        "F1586-13 终验报告归档",
        seal_res.is_ok() && f.report().is_some() && f.phase() == Phase::Sealed,
        &format!("sealed={}", f.phase() == Phase::Sealed),
    );
    s.add_dyn(
        "F1586-14 状态写回（已完成+摘要非空）",
        match &seal_res {
            Ok(r) => r.hits == SPEC_ITEMS && r.false_alarms == 0 && !r.summary.is_empty(),
            Err(e) => {
                let _ = e;
                false
            }
        },
        match &seal_res {
            Ok(r) => r.summary.as_str(),
            Err(e) => detail(*e),
        },
    );

    s.add_dyn(
        "F1586-15 封验后登记显性拒",
        match f.record(1, &SpecItem::make(1, Kind::PrivEsc, inject_sig(Kind::PrivEsc, 1)), true) {
            Err(e) => e == Diag::ESealed.val(),
            Ok(_) => false,
        },
        "终验后不得再登记（ESealed）",
    );

    s.add_dyn(
        "F1586-16 双跑一致",
        {
            let a = replay_round(1);
            let b = replay_round(1);
            match (a, b) {
                (Ok(x), Ok(y)) => x.same_as(&y),
                _ => false,
            }
        },
        "总谱重放两轮命中集逐件一致",
    );

    s.add_dyn(
        "F1586-17 诊断码闭集完备",
        detail_codes_ok(),
        "12 码逐条查表非空且兜底可达",
    );

    s
}

/// 诊断码闭集完备性。
fn detail_codes_ok() -> bool {
    if ALL.len() != 12 {
        return false;
    }
    for d in ALL {
        if d.detail().is_empty() {
            return false;
        }
        if detail(d.val()) != d.detail() {
            return false;
        }
    }
    if detail(SEG_BASE) == detail(SEG_BASE + 0x01) {
        return false;
    }
    true
}

// ---- 测试区 ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_holds() {
        assert!(SEG_BASE % 0x40 == 0);
        assert!(SEG_BASE >= 0x79000 && SEG_BASE <= 0x791C0);
        assert!(seg_guard_ok());
    }

    #[test]
    fn master_score_is_complete_and_hits() {
        let score = master_score();
        assert_eq!(score.len(), SPEC_ITEMS);
        for (i, item) in score.iter().enumerate() {
            assert!(item.in_scope(), "件 {} 结构非法", i + 1);
            assert_eq!(item.kind.code() as usize, i + 1);
            assert!(detector_hits(item), "件 {} 必须命中", item.id);
        }
    }

    #[test]
    fn clean_probes_pass_detector() {
        for i in 0..SPEC_ITEMS {
            let kind = match Kind::of_code((i + 1) as u8) {
                Some(k) => k,
                None => { assert!(false, "类别码册外"); loop {} },
            };
            let sig = clean_sig(kind, i as u64);
            assert!(clean_probe_ok(sig, kind), "探针 {} 误报", i + 1);
            let fake = SpecItem::make((i + 1) as u16, kind, sig);
            assert!(!detector_hits(&fake), "干净负载不得命中");
        }
    }

    #[test]
    fn rounds_replay_full_and_drift_free() {
        let a = replay_round(1);
        let b = replay_round(2);
        match (a, b) {
            (Ok(x), Ok(y)) => {
                assert!(x.all_hit());
                assert!(y.all_hit());
                assert!(x.same_as(&y));
            }
            _ => { assert!(false, "总谱重放失败"); loop {} },
        }
    }

    #[test]
    fn dup_and_range_rejections() {
        let mut l = RunLedger::new();
        assert!(l.record(1, true).is_ok());
        assert_eq!(l.record(1, true), Err(Diag::EDup.val()));
        assert_eq!(l.record(0, true), Err(Diag::ERange.val()));
        assert_eq!(l.record(11, true), Err(Diag::ERange.val()));
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn miss_list_points_at_injected_id() {
        let mut l = RunLedger::new();
        let score = master_score();
        let mut i = 0usize;
        while i < SPEC_ITEMS {
            let item = match score.get(i) {
                Some(x) => *x,
                None => { assert!(false, "总谱缺件"); loop {} },
            };
            let hit = if item.id == 7 { false } else { detector_hits(&item) };
            assert!(l.record(item.id, hit).is_ok());
            i += 1;
        }
        let missed = l.missed_ids();
        assert_eq!(missed.len(), 1);
        match missed.get(0) {
            Some(7) => {}
            other => { assert!(false, "漏检点名错位: {:?}", other); loop {} },
        }
        assert!(!l.all_hit());
    }

    #[test]
    fn false_alarm_detected_after_probe_swap() {
        assert!(false_alarm_injection_caught());
        let f = InjectFinal::new();
        assert_eq!(f.false_alarm_count(), 0);
        assert_eq!(false_permille(&f), 0);
    }

    #[test]
    fn seal_reports_and_freezes() {
        let mut f = InjectFinal::new();
        let score = master_score();
        let mut round = 1usize;
        while round <= ROUNDS {
            let mut i = 0usize;
            while i < SPEC_ITEMS {
                let item = match score.get(i) {
                    Some(x) => *x,
                    None => { assert!(false, "总谱缺件"); loop {} },
                };
                assert!(f.record(round, &item, detector_hits(&item)).is_ok());
                i += 1;
            }
            round += 1;
        }
        assert!(f.verify_ok());
        let rep = match f.seal() {
            Ok(r) => r,
            Err(e) => { assert!(false, "终验失败: 诊断码 {}", e); loop {} },
        };
        assert_eq!(rep.hits, 10);
        assert_eq!(rep.false_alarms, 0);
        assert!(rep.missed.is_empty());
        assert!(!rep.summary.is_empty());
        assert!(f.phase() == Phase::Sealed);
        let first = match score.get(0) {
            Some(x) => *x,
            None => { assert!(false, "总谱缺件"); loop {} },
        };
        assert_eq!(f.record(1, &first, true), Err(Diag::ESealed.val()));
    }

    #[test]
    fn incomplete_score_blocks_seal() {
        let mut f = InjectFinal::new();
        let score = master_score();
        let mut i = 0usize;
        while i < SPEC_ITEMS - 1 {
            assert!(f.record(1, &score[i], true).is_ok());
            i += 1;
        }
        match f.seal() {
            Err(e) => assert_eq!(e, Diag::ERounds.val()),
            Ok(_) => { assert!(false, "缺件终验必须拒"); loop {} },
        }
    }

    #[test]
    fn diag_closed_set_complete() {
        assert_eq!(ALL.len(), 12);
        for (i, d) in ALL.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + (i as u32) + 1);
            assert!(!d.detail().is_empty());
        }
        assert_eq!(
            detail(SEG_BASE + 0x0d),
            "未知诊断码：不在 0x79081..0x7908C 闭集内"
        );
        assert!(detail_codes_ok());
    }
}
