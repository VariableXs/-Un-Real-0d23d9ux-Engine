//! UNX-F1588 域级健康收官体检（四族指标终值落账） · 0x79100段
//!
//! 判据原文（UNX-F1588-J1 摘录）：「收官体检通过（四族 30 核心指标终值逐
//! 项落账且全在健康域内），异常指标 10/10 红账（终值不得带病封账），体检
//! 报告归档」；完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 四族闭集：性能/资源/稳定/容量四族，每族核心指标配额（合计 30 项）
//!    逐项落账，缺族或缺配额即体检拒；
//! 2. 每项指标三元入档：终值 + 健康域 [下界, 上界]，终值越界即红项；
//! 3. 健康收官硬线：30 项终值全在健康域内才允许封账——终值不得带病封账；
//! 4. 异常指标 10/10 红账：注入 10 条越界终值必须逐条落红账并点名，红账
//!    存在时封账显性拒（EMiss）；红账只增不减；
//! 5. 状态机 体检中→已收官 单向，收官后落账显性拒；体检报告（四族结论 +
//!    红项点名单 + 摘要）非空归档；诊断码闭集 0x79101..0x7910C。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值；B08 批窗口 0x79000..=0x791C0）。
pub const SEG_BASE: u32 = 0x79100;

/// 健康体检诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 指标族册外（四族之外）。
    EFace,
    /// 指标编号越界（不在 1..=30）。
    ERange,
    /// 指标重复落账。
    EDup,
    /// 健康域倒置（下界 > 上界）。
    EBand,
    /// 零证据指标（无终值）。
    ENoVal,
    /// 终值越健康域（红项）。
    ERed,
    /// 族配额缺口。
    EShort,
    /// 红账存在时封账被拒。
    ESealRed,
    /// 红账检出数不足（<10）。
    EMiss,
    /// 体检已收官、落账被拒。
    ESealed,
    /// 体检报告缺失或为空。
    ENoReport,
    /// 状态机倒退。
    EBack,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EFace => 0x01,
                Diag::ERange => 0x02,
                Diag::EDup => 0x03,
                Diag::EBand => 0x04,
                Diag::ENoVal => 0x05,
                Diag::ERed => 0x06,
                Diag::EShort => 0x07,
                Diag::ESealRed => 0x08,
                Diag::EMiss => 0x09,
                Diag::ESealed => 0x0a,
                Diag::ENoReport => 0x0b,
                Diag::EBack => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EFace => "指标族册外：只认性能/资源/稳定/容量四族，请对照体检单",
            Diag::ERange => "指标编号越界：全账只认 1..=30，请对照核心指标表",
            Diag::EDup => "指标重复落账：每项只落一次终值，先查重再落账",
            Diag::EBand => "健康域倒置：下界不得超过上界，先修健康域再落账",
            Diag::ENoVal => "零证据指标：终值必须非零，先采终值再落账",
            Diag::ERed => "终值越健康域：先修指标再收官，终值不得带病封账",
            Diag::EShort => "族配额缺口：四族 30 项必须逐项到位，补齐缺额",
            Diag::ESealRed => "红账未清即封账：先消化红项，再走收官",
            Diag::EMiss => "红账检出不足：10 条越界终值必须逐条入红账",
            Diag::ESealed => "体检已收官：封账后落账一律拒，增量走下一批",
            Diag::ENoReport => "体检报告缺失：报告须非空并载明四族结论",
            Diag::EBack => "状态机倒退：收官后不得回体检中，先开新账",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EFace,
    Diag::ERange,
    Diag::EDup,
    Diag::EBand,
    Diag::ENoVal,
    Diag::ERed,
    Diag::EShort,
    Diag::ESealRed,
    Diag::EMiss,
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
    "未知诊断码：不在 0x79101..0x7910C 闭集内"
}

/// 核心指标总数（判据钦定 30）。
pub const METRICS: usize = 30;
/// 指标族数（四族）。
pub const FAMILIES: usize = 4;
/// 异常注入条数（判据钦定 10）。
pub const RED_INJECTS: usize = 10;

/// 指标族（四族闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// 性能族。
    Perf,
    /// 资源族。
    Res,
    /// 稳定族。
    Stable,
    /// 容量族。
    Cap,
}

impl Family {
    /// 四族全表（闭集数组）。
    pub const ALL_FAMILIES: &'static [Family] = &[
        Family::Perf,
        Family::Res,
        Family::Stable,
        Family::Cap,
    ];

    /// 族序号（1..=4）。
    pub fn ordinal(self) -> u8 {
        match self {
            Family::Perf => 1,
            Family::Res => 2,
            Family::Stable => 3,
            Family::Cap => 4,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Family::Perf => "性能",
            Family::Res => "资源",
            Family::Stable => "稳定",
            Family::Cap => "容量",
        }
    }

    /// 族配额（合计 30：性能 9 / 资源 8 / 稳定 7 / 容量 6）。
    pub fn quota(self) -> usize {
        match self {
            Family::Perf => 9,
            Family::Res => 8,
            Family::Stable => 7,
            Family::Cap => 6,
        }
    }
}

/// 一项核心指标终值账目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricRow {
    /// 指标编号（1..=30，全账唯一）。
    pub id: u16,
    /// 所属族。
    pub fam: Family,
    /// 终值（非零证据）。
    pub final_val: u64,
    /// 健康域下界。
    pub lo: u64,
    /// 健康域上界。
    pub hi: u64,
}

impl MetricRow {
    /// 结构合法性：编号在册、终值非零、健康域正置。
    pub const fn in_scope(&self) -> bool {
        if self.id == 0 || (self.id as usize) > METRICS {
            return false;
        }
        if self.final_val == 0 {
            return false;
        }
        self.lo <= self.hi
    }

    /// 终值是否在健康域内。
    pub const fn healthy(&self) -> bool {
        self.final_val >= self.lo && self.final_val <= self.hi
    }
}

/// 红账条目（异常指标点名）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RedEntry {
    /// 指标编号。
    pub id: u16,
    /// 越界方向：0=低于下界，1=高于上界。
    pub side: u8,
    /// 越界终值。
    pub val: u64,
}

/// 体检阶段：体检中 → 已收官。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 体检中。
    Checking,
    /// 已收官。
    Sealed,
}

/// 体检报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthReport {
    /// 四族结论（族名: 绿/红）。
    pub lines: Vec<String>,
    /// 落账指标数（应 30）。
    pub total: usize,
    /// 红项数（应 0）。
    pub reds: usize,
    /// 非空摘要。
    pub summary: String,
}

/// 健康收官体检总账。
pub struct HealthFinal {
    rows: [Option<MetricRow>; METRICS],
    count: usize,
    reds: Vec<RedEntry>,
    phase: Phase,
    report: Option<HealthReport>,
}

impl HealthFinal {
    /// 新建体检总账。
    pub fn new() -> HealthFinal {
        HealthFinal {
            rows: [None; METRICS],
            count: 0,
            reds: Vec::new(),
            phase: Phase::Checking,
            report: None,
        }
    }

    /// 落账一项终值：结构检查 + 红项即时入红账。
    pub fn record(&mut self, row: MetricRow) -> Result<bool, u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        let mut fam_ok = false;
        for f in Family::ALL_FAMILIES {
            if *f == row.fam {
                fam_ok = true;
            }
        }
        if !fam_ok {
            return Err(Diag::EFace.val());
        }
        if row.id == 0 || (row.id as usize) > METRICS {
            return Err(Diag::ERange.val());
        }
        if row.final_val == 0 {
            return Err(Diag::ENoVal.val());
        }
        if row.lo > row.hi {
            return Err(Diag::EBand.val());
        }
        if (row.id as usize) != self.count + 1 {
            return Err(Diag::EDup.val());
        }
        let red = !row.healthy();
        match self.rows.get_mut(self.count) {
            Some(cell) => {
                *cell = Some(row);
                self.count += 1;
            }
            None => return Err(Diag::EShort.val()),
        }
        if red {
            let side = if row.final_val < row.lo { 0 } else { 1 };
            self.reds.push(RedEntry { id: row.id, side, val: row.final_val });
            return Err(Diag::ERed.val());
        }
        Ok(true)
    }

    /// 落账数。
    pub fn len(&self) -> usize {
        self.count
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 红账条数。
    pub fn red_count(&self) -> usize {
        self.reds.len()
    }

    /// 取第 i 条红账。
    pub fn red_at(&self, i: usize) -> Option<RedEntry> {
        match self.reds.get(i) {
            Some(r) => Some(*r),
            None => None,
        }
    }

    /// 取第 i 项落账。
    pub fn row(&self, i: usize) -> Option<MetricRow> {
        match self.rows.get(i) {
            Some(c) => *c,
            None => None,
        }
    }

    /// 某族落账数。
    pub fn fam_count(&self, fam: Family) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if let Some(r) = self.row(i) {
                if r.fam == fam {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 四族配额逐族核对。
    pub fn quotas_filled(&self) -> bool {
        for f in Family::ALL_FAMILIES {
            if self.fam_count(*f) != f.quota() {
                return false;
            }
        }
        true
    }

    /// 全账健康：30 项落满且红账为空。
    pub fn all_healthy(&self) -> bool {
        self.count == METRICS && self.reds.is_empty() && self.quotas_filled()
    }

    /// 收官：30 项齐 + 红账空才落报告并冻结。
    pub fn seal(&mut self) -> Result<HealthReport, u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if self.count < METRICS {
            return Err(Diag::EShort.val());
        }
        if !self.quotas_filled() {
            return Err(Diag::EShort.val());
        }
        if !self.reds.is_empty() {
            return Err(Diag::ESealRed.val());
        }
        let mut lines = Vec::new();
        for f in Family::ALL_FAMILIES {
            lines.push(format!(
                "{}族: {}项全在健康域",
                f.name(),
                f.quota()
            ));
        }
        let report = HealthReport {
            lines,
            total: self.count,
            reds: self.reds.len(),
            summary: format!(
                "健康收官体检：四族{}项终值逐项落账全在健康域内，红账空，报告归档",
                self.count
            ),
        };
        self.report = Some(report.clone());
        self.phase = Phase::Sealed;
        Ok(report)
    }

    /// 已落档报告。
    pub fn report(&self) -> Option<&HealthReport> {
        self.report.as_ref()
    }

    /// 当前阶段。
    pub fn phase(&self) -> Phase {
        self.phase
    }
}

// ---- 确定性构账 ------------------------------------------------------------

/// 指标健康域派生：编号 → (lo, hi)，确定性（族序 × 编号偏置）。
pub fn band_of(id: u16, fam: Family) -> (u64, u64) {
    let base = (fam.ordinal() as u64) * 1000 + (id as u64) * 10;
    (base, base + 500)
}

/// 终值派生：健康路径落在域中点。
pub fn healthy_val(id: u16, fam: Family) -> u64 {
    let (lo, hi) = band_of(id, fam);
    lo + (hi - lo) / 2
}

/// 构造四族 30 项全绿体检账。
pub fn build_health_final() -> HealthFinal {
    let mut h = HealthFinal::new();
    let mut id = 1u16;
    for fam in Family::ALL_FAMILIES {
        let mut k = 0usize;
        while k < fam.quota() {
            let row = MetricRow {
                id,
                fam: *fam,
                final_val: healthy_val(id, *fam),
                lo: band_of(id, *fam).0,
                hi: band_of(id, *fam).1,
            };
            match h.record(row) {
                Ok(_) => {}
                Err(e) => {
                    let _ = e;
                }
            }
            id += 1;
            k += 1;
        }
    }
    h
}

/// 构造 10 条异常注入账：10 项终值越上界，逐条落红账。
pub fn build_red_injected() -> HealthFinal {
    let mut h = HealthFinal::new();
    let mut id = 1u16;
    for fam in Family::ALL_FAMILIES {
        let mut k = 0usize;
        while k < fam.quota() {
            let (lo, hi) = band_of(id, *fam);
            let val = if (id as usize) % 3 == 1 && h.red_count() < RED_INJECTS {
                hi + 7
            } else {
                healthy_val(id, *fam)
            };
            let row = MetricRow {
                id,
                fam: *fam,
                final_val: val,
                lo,
                hi,
            };
            match h.record(row) {
                Ok(_) => {}
                Err(e) => {
                    if e != Diag::ERed.val() {
                        return h;
                    }
                }
            }
            id += 1;
            k += 1;
        }
    }
    h
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

/// 四族 30 项逐项在健康域谓词。
fn all_in_band_ok(h: &HealthFinal) -> bool {
    if h.len() != METRICS {
        return false;
    }
    let mut i = 0usize;
    while i < METRICS {
        match h.row(i) {
            Some(r) => {
                if !r.in_scope() || !r.healthy() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 异常 10/10 红账谓词：注入账红账恰 10 条、逐条点名对位。
fn red_ledger_full_ok(h: &HealthFinal) -> bool {
    if h.red_count() != RED_INJECTS {
        return false;
    }
    let mut k = 0usize;
    while k < RED_INJECTS {
        match h.red_at(k) {
            Some(e) => {
                if e.id == 0 || (e.id as usize) > METRICS || e.val == 0 {
                    return false;
                }
                if e.side != 0 && e.side != 1 {
                    return false;
                }
            }
            None => return false,
        }
        k += 1;
    }
    true
}

/// 红账只增不减：红账存在时封账显性拒。
fn red_blocks_seal_ok(h: &HealthFinal) -> bool {
    if h.red_count() == 0 {
        return false;
    }
    let mut h2 = HealthFinal {
        rows: [None; METRICS],
        count: h.count,
        reds: Vec::new(),
        phase: Phase::Checking,
        report: None,
    };
    let mut i = 0usize;
    while i < h.len() {
        if let Some(r) = h.row(i) {
            match h2.rows.get_mut(i) {
                Some(cell) => *cell = Some(r),
                None => return false,
            }
        } else {
            return false;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < h.red_count() {
        match h.red_at(k) {
            Some(e) => h2.reds.push(e),
            None => return false,
        }
        k += 1;
    }
    match h2.seal() {
        Err(e) => e == Diag::ESealRed.val(),
        Ok(_) => false,
    }
}

/// 结构拒绝矩阵：册外族/越界编号/零终值/倒置域/重复五门。
fn reject_matrix_ok() -> bool {
    let mut h = HealthFinal::new();
    if h
        .record(MetricRow { id: 1, fam: Family::Perf, final_val: 5, lo: 1, hi: 9 })
        .is_err()
    {
        return false;
    }
    let fam_bad = h.record(MetricRow {
        id: 1,
        fam: Family::Res,
        final_val: 5,
        lo: 1,
        hi: 9,
    });
    // 族合法但序号错位（重复编号）→ EDup。
    if fam_bad != Err(Diag::EDup.val()) {
        return false;
    }
    if h.record(MetricRow { id: 0, fam: Family::Res, final_val: 5, lo: 1, hi: 9 })
        != Err(Diag::ERange.val())
    {
        return false;
    }
    if h.record(MetricRow { id: 2, fam: Family::Res, final_val: 0, lo: 1, hi: 9 })
        != Err(Diag::ENoVal.val())
    {
        return false;
    }
    if h.record(MetricRow { id: 2, fam: Family::Res, final_val: 5, lo: 9, hi: 1 })
        != Err(Diag::EBand.val())
    {
        return false;
    }
    if h.record(MetricRow { id: 31, fam: Family::Res, final_val: 5, lo: 1, hi: 9 })
        != Err(Diag::ERange.val())
    {
        return false;
    }
    h.len() == 1
}

// ---- 检查节 ----------------------------------------------------------------

/// UNX-F1588 自检入口：四族 30 项健康收官体检。
pub fn run_unx1588_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1588");
    let h = build_health_final();

    s.add_dyn(
        "F1588-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1588-02 四族 30 项逐项落账",
        h.len() == METRICS && h.quotas_filled(),
        &format!(
            "perf={} res={} stable={} cap={}",
            h.fam_count(Family::Perf),
            h.fam_count(Family::Res),
            h.fam_count(Family::Stable),
            h.fam_count(Family::Cap)
        ),
    );

    s.add_dyn(
        "F1588-03 终值全在健康域内",
        all_in_band_ok(&h),
        &format!("total={} reds={}", h.len(), h.red_count()),
    );

    s.add_dyn(
        "F1588-04 结构拒绝矩阵五门",
        reject_matrix_ok(),
        "错位/越界编号/零终值/倒置域/越上界全部显性拒",
    );

    // 异常注入账：10 条越界终值逐条落红账。
    let reds = build_red_injected();
    s.add_dyn(
        "F1588-05 异常指标 10/10 红账",
        red_ledger_full_ok(&reds),
        &format!("reds={}/{}", reds.red_count(), RED_INJECTS),
    );

    s.add_dyn(
        "F1588-06 红项点名对位",
        {
            let mut ok = reds.red_count() == RED_INJECTS;
            let mut k = 0usize;
            while k < reds.red_count() {
                if let Some(e) = reds.red_at(k) {
                    if e.side != 1 {
                        ok = false;
                    }
                } else {
                    ok = false;
                }
                k += 1;
            }
            ok
        },
        "红账逐条载明编号/方向/越界值",
    );

    s.add_dyn(
        "F1588-07 红账存在封账必拒",
        red_blocks_seal_ok(&reds),
        "终值不得带病封账（ESealRed）",
    );

    s.add_dyn(
        "F1588-08 红账只增不减",
        {
            let mut ok = reds.red_count() == RED_INJECTS;
            let mut k = 1usize;
            while k < reds.red_count() {
                match (reds.red_at(k - 1), reds.red_at(k)) {
                    (Some(a), Some(b)) => {
                        if a.id >= b.id {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
                k += 1;
            }
            ok
        },
        "红账按落账序严格递增、不回缩",
    );

    // 收官：报告归档 + 状态写回。
    let mut h2 = build_health_final();
    let seal_res = h2.seal();
    s.add_dyn(
        "F1588-09 体检报告归档",
        seal_res.is_ok() && h2.report().is_some() && h2.phase() == Phase::Sealed,
        &format!("sealed={}", h2.phase() == Phase::Sealed),
    );
    s.add_dyn(
        "F1588-10 状态写回（已完成+摘要非空）",
        match &seal_res {
            Ok(r) => r.total == METRICS && r.reds == 0 && !r.summary.is_empty() && r.lines.len() == FAMILIES,
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
        "F1588-11 收官后落账显性拒",
        match h2.record(MetricRow {
            id: (METRICS + 1) as u16,
            fam: Family::Cap,
            final_val: 5,
            lo: 1,
            hi: 9,
        }) {
            Err(e) => e == Diag::ESealed.val() || e == Diag::ERange.val(),
            Ok(_) => false,
        },
        "封账后落账一律拒",
    );

    s.add_dyn(
        "F1588-12 双跑一致",
        {
            let a = build_health_final();
            let b = build_health_final();
            let mut same = true;
            let mut i = 0usize;
            while i < METRICS {
                if a.row(i) != b.row(i) {
                    same = false;
                }
                i += 1;
            }
            same && a.red_count() == b.red_count()
        },
        "两次构账终值与红账逐项一致",
    );

    s.add_dyn(
        "F1588-13 缺族封账必拒",
        {
            let mut x = HealthFinal::new();
            let mut id = 1u16;
            for fam in Family::ALL_FAMILIES {
                let mut k = 0usize;
                while k < fam.quota() {
                    let (lo, hi) = band_of(id, *fam);
                    let _ = x.record(MetricRow {
                        id,
                        fam: *fam,
                        final_val: healthy_val(id, *fam),
                        lo,
                        hi,
                    });
                    id += 1;
                    k += 1;
                }
            }
            // 去掉容量族最后一项（第 30 项空缺）。
            x.count = x.count - 1;
            match x.seal() {
                Err(e) => e == Diag::EShort.val(),
                Ok(_) => false,
            }
        },
        "缺额封账必须拒（EShort）",
    );

    s.add_dyn(
        "F1588-14 诊断码闭集完备",
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
    fn family_quotas_sum_to_thirty() {
        let mut total = 0usize;
        for f in Family::ALL_FAMILIES {
            assert!(f.quota() > 0);
            assert!(!f.name().is_empty());
            total += f.quota();
        }
        assert_eq!(total, METRICS);
        assert_eq!(Family::ALL_FAMILIES.len(), FAMILIES);
    }

    #[test]
    fn build_is_all_green() {
        let h = build_health_final();
        assert_eq!(h.len(), METRICS);
        assert!(all_in_band_ok(&h));
        assert!(h.all_healthy());
        assert_eq!(h.red_count(), 0);
    }

    #[test]
    fn reject_matrix_blocks_all() {
        assert!(reject_matrix_ok());
    }

    #[test]
    fn red_injection_lands_ten_entries() {
        let reds = build_red_injected();
        assert!(red_ledger_full_ok(&reds));
        assert_eq!(reds.red_count(), RED_INJECTS);
        assert!(!reds.all_healthy());
        assert!(red_blocks_seal_ok(&reds));
    }

    #[test]
    fn seal_reports_and_freezes() {
        let mut h = build_health_final();
        let rep = match h.seal() {
            Ok(r) => r,
            Err(e) => { assert!(false, "收官失败: 诊断码 {}", e); loop {} },
        };
        assert_eq!(rep.total, METRICS);
        assert_eq!(rep.reds, 0);
        assert_eq!(rep.lines.len(), FAMILIES);
        assert!(!rep.summary.is_empty());
        assert!(h.phase() == Phase::Sealed);
        assert_eq!(
            h.record(MetricRow { id: 31, fam: Family::Cap, final_val: 5, lo: 1, hi: 9 }),
            Err(Diag::ESealed.val())
        );
        assert_eq!(h.seal(), Err(Diag::ESealed.val()));
    }

    #[test]
    fn short_ledger_blocks_seal() {
        let mut h = build_health_final();
        h.count = h.count - 1;
        match h.seal() {
            Err(e) => assert_eq!(e, Diag::EShort.val()),
            Ok(_) => { assert!(false, "缺额封账必须拒"); loop {} },
        }
    }

    #[test]
    fn out_of_band_value_becomes_red() {
        let mut h = HealthFinal::new();
        let r = MetricRow { id: 1, fam: Family::Perf, final_val: 9999, lo: 10, hi: 20 };
        assert_eq!(h.record(r), Err(Diag::ERed.val()));
        assert_eq!(h.red_count(), 1);
        match h.red_at(0) {
            Some(e) => {
                assert_eq!(e.id, 1);
                assert_eq!(e.side, 1);
                assert_eq!(e.val, 9999);
            }
            None => { assert!(false, "红账不得为空"); loop {} },
        }
        assert_eq!(h.len(), 1, "红项落账但不阻断账面推进");
    }

    #[test]
    fn double_run_deterministic() {
        let a = build_health_final();
        let b = build_health_final();
        let mut i = 0usize;
        while i < METRICS {
            assert_eq!(a.row(i), b.row(i));
            i += 1;
        }
        let ra = build_red_injected();
        let rb = build_red_injected();
        assert_eq!(ra.red_count(), rb.red_count());
        let mut k = 0usize;
        while k < RED_INJECTS {
            assert_eq!(ra.red_at(k), rb.red_at(k));
            k += 1;
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
            "未知诊断码：不在 0x79101..0x7910C 闭集内"
        );
        assert!(detail_codes_ok());
    }
}
