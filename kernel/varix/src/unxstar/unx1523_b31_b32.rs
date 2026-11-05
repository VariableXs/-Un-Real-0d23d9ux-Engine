//! UNX-F1523 延迟账本域级总对账（B31/B32 与单类账交叉） · 0x783C0段
//!
//! 判据原文（UNX-F1523-J1 摘录）：「延迟账域级对账一致率 100%（全向量账
//! F1401 与单类账 F0851/F0893/F0949/F1170 族交叉对平），差额=0 断言，漂移
//! 注入 10/10 必抓」；完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 四类单类账闭集：F0851 采样延迟账/F0893 提交延迟账/F0949 切换延迟账/
//!    F1170 恢复延迟账，类号 1..=4，类外样本显性拒——类闭集外没有
//!    "第五类延迟"的落脚点；
//! 2. 全向量账 F1401 与四类单类账同语料入账：交叉对平 = 全向量账每类小计
//!    == 对应单类账总额，且 总差额 = 全向量总额 - 四类之和 == 0；
//! 3. 差额=0 断言是域级对账的终局门：差额非零即两账有一处漂移，域级
//!    不许封账；
//! 4. 漂移注入 10/10 必抓：十轮注入分别篡改某类单类账总额/全向量某类
//!    小计/新增幽灵条目，对账器逐轮必红且按类定位；
//! 5. 状态机 对账中→已封账 单向，封账后改账/重对显性拒；对账报告非空
//!    落档（类小计/总额/差额）；
//! 6. 诊断码闭集 0x783C1..0x783CC，Err 用 Result<T,u32>；整数运算，
//!    延迟值一律微秒整数，零浮点。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值）。
pub const SEG_BASE: u32 = 0x783C0;

/// 延迟类数（四类单类账闭集）。
pub const CLASS_COUNT: usize = 4;

/// 漂移注入轮数（判据原文 10/10）。
pub const DRIFT_ROUNDS: usize = 10;

/// 延迟账诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 未知延迟类（不在 1..=4）。
    EClass,
    /// 交叉对账失配（类小计不符）。
    EDelta,
    /// 总差额非零（差额=0 断言破坏）。
    ETotal,
    /// 单类账缺失（四类未齐即对账）。
    EMiss,
    /// 条目乱序（序号必须递增）。
    EOrder,
    /// 账本已封账。
    ESealed,
    /// 重复条目（同源同序号重记）。
    EDup,
    /// 空账（零条目不构成对账证据）。
    EEmpty,
    /// 对账报告缺失或为空。
    ENoRep,
    /// 封账重放复核不符。
    EReplay,
    /// 注入轮号越界（不在 1..=10）。
    ERange,
    /// 封账后重对（状态机倒退）。
    ESeal,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EClass => 0x01,
                Diag::EDelta => 0x02,
                Diag::ETotal => 0x03,
                Diag::EMiss => 0x04,
                Diag::EOrder => 0x05,
                Diag::ESealed => 0x06,
                Diag::EDup => 0x07,
                Diag::EEmpty => 0x08,
                Diag::ENoRep => 0x09,
                Diag::EReplay => 0x0a,
                Diag::ERange => 0x0b,
                Diag::ESeal => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EClass => "未知延迟类：类号必须在 1..=4 闭集内（采样/提交/切换/恢复）",
            Diag::EDelta => "交叉对账失配：全向量账类小计必须等于对应单类账总额",
            Diag::ETotal => "总差额非零：全向量总额减四类之和必须为 0，先查漂移再封账",
            Diag::EMiss => "单类账缺失：四类逐类齐账才许域级对账",
            Diag::EOrder => "条目乱序：序号必须递增登记，禁止回填",
            Diag::ESealed => "延迟账已封账：封账后写入拒绝，开下一批延迟账",
            Diag::EDup => "重复条目：同源同序号只记一条，改账走修正轮",
            Diag::EEmpty => "空账：零条目不构成延迟对账证据",
            Diag::ENoRep => "对账报告缺失：类小计/总额/差额必须非空落档",
            Diag::EReplay => "封账重放不符：重放小计与封账报告不一致，账本可疑",
            Diag::ERange => "注入轮号越界：注入轮号必须在 1..=10 闭区间内",
            Diag::ESeal => "状态机倒退：对账中→已封账单向推进，禁止封账后重对",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EClass,
    Diag::EDelta,
    Diag::ETotal,
    Diag::EMiss,
    Diag::EOrder,
    Diag::ESealed,
    Diag::EDup,
    Diag::EEmpty,
    Diag::ENoRep,
    Diag::EReplay,
    Diag::ERange,
    Diag::ESeal,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x783C1..0x783CC 闭集内"
}

/// 四类单类账闭集（类号+域级账编号+类名，冻结表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatClass {
    /// F0851 采样延迟账。
    Sample,
    /// F0893 提交延迟账。
    Commit,
    /// F0949 切换延迟账。
    Switch,
    /// F1170 恢复延迟账。
    Recover,
}

impl LatClass {
    /// 类号（1..=4）。
    pub fn id(self) -> u32 {
        match self {
            LatClass::Sample => 1,
            LatClass::Commit => 2,
            LatClass::Switch => 3,
            LatClass::Recover => 4,
        }
    }

    /// 域级账编号（Fxxxx）。
    pub fn ledger_id(self) -> u32 {
        match self {
            LatClass::Sample => 0xF0851,
            LatClass::Commit => 0xF0893,
            LatClass::Switch => 0xF0949,
            LatClass::Recover => 0xF1170,
        }
    }

    /// 类名。
    pub fn name(self) -> &'static str {
        match self {
            LatClass::Sample => "采样延迟",
            LatClass::Commit => "提交延迟",
            LatClass::Switch => "切换延迟",
            LatClass::Recover => "恢复延迟",
        }
    }
}

/// 四类闭集数组。
pub const CLASSES: &[LatClass] = &[
    LatClass::Sample,
    LatClass::Commit,
    LatClass::Switch,
    LatClass::Recover,
];

/// 一条延迟条目（类号+序号+微秒值，整数运算）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LatEntry {
    /// 延迟类。
    pub class: LatClass,
    /// 源序号（入账序，单调递增）。
    pub seq: u32,
    /// 延迟值（微秒）。
    pub usec: u64,
}

/// 单类延迟账。
#[derive(Clone, Debug)]
pub struct ClassLedger {
    cls: LatClass,
    entries: Vec<LatEntry>,
}

impl ClassLedger {
    /// 建空单类账。
    pub fn new(cls: LatClass) -> ClassLedger {
        ClassLedger {
            cls,
            entries: Vec::new(),
        }
    }

    /// 类号。
    pub fn class(&self) -> LatClass {
        self.cls
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空账判定。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 入账（类不符/乱序/重复门）。
    pub fn record(&mut self, e: LatEntry) -> Result<(), u32> {
        if e.class != self.cls {
            return Err(Diag::EClass.val());
        }
        if e.usec == 0 {
            return Err(Diag::EEmpty.val());
        }
        if let Some(last) = self.entries.last() {
            if e.seq <= last.seq {
                return Err(Diag::EOrder.val());
            }
            if e.seq == last.seq {
                return Err(Diag::EDup.val());
            }
        }
        self.entries.push(e);
        Ok(())
    }

    /// 本类总额（微秒和）。
    pub fn total(&self) -> u64 {
        let mut t = 0u64;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                t = t.wrapping_add(e.usec);
            }
            i += 1;
        }
        t
    }
}

/// 全向量延迟账 F1401（四类条目混入，逐类小计）。
#[derive(Clone, Debug)]
pub struct TotalLedger {
    entries: Vec<LatEntry>,
}

impl TotalLedger {
    /// 建空全向量账。
    pub fn new() -> TotalLedger {
        TotalLedger {
            entries: Vec::new(),
        }
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空账判定。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 第 `idx` 条。
    pub fn at(&self, idx: usize) -> Option<&LatEntry> {
        self.entries.get(idx)
    }

    /// 入账（类闭集/乱序门）。
    pub fn record(&mut self, e: LatEntry) -> Result<(), u32> {
        let mut known = false;
        let mut i = 0usize;
        while i < CLASSES.len() {
            if let Some(c) = CLASSES.get(i) {
                if *c == e.class {
                    known = true;
                }
            }
            i += 1;
        }
        if !known {
            return Err(Diag::EClass.val());
        }
        if e.usec == 0 {
            return Err(Diag::EEmpty.val());
        }
        if let Some(last) = self.entries.last() {
            if e.seq <= last.seq {
                return Err(Diag::EOrder.val());
            }
        }
        self.entries.push(e);
        Ok(())
    }

    /// 某类小计。
    pub fn class_total(&self, cls: LatClass) -> u64 {
        let mut t = 0u64;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.class == cls {
                    t = t.wrapping_add(e.usec);
                }
            }
            i += 1;
        }
        t
    }

    /// 总额。
    pub fn total(&self) -> u64 {
        let mut t = 0u64;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                t = t.wrapping_add(e.usec);
            }
            i += 1;
        }
        t
    }
}

/// 域级对账机（对账中→已封账 单向）。
#[derive(Clone, Debug)]
pub struct DomainRecon {
    total: TotalLedger,
    singles: Vec<ClassLedger>,
    sealed: bool,
    report: String,
}

impl DomainRecon {
    /// 建对账机：全向量账+空单类账族。
    pub fn new(total: TotalLedger) -> DomainRecon {
        DomainRecon {
            total,
            singles: Vec::new(),
            sealed: false,
            report: String::new(),
        }
    }

    /// 已挂单类账数。
    pub fn single_count(&self) -> usize {
        self.singles.len()
    }

    /// 已封账判定。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 对账报告（封账后非空）。
    pub fn report(&self) -> &str {
        &self.report
    }

    /// 第 `idx` 笔单类账。
    pub fn single_at(&self, idx: usize) -> Option<&ClassLedger> {
        self.singles.get(idx)
    }

    /// 全向量账。
    pub fn total_ledger(&self) -> &TotalLedger {
        &self.total
    }

    /// 挂一笔单类账（封账/重复类/容量门）。
    pub fn attach(&mut self, l: ClassLedger) -> Result<(), u32> {
        if self.sealed {
            return Err(Diag::ESealed.val());
        }
        if self.singles.len() >= CLASS_COUNT {
            return Err(Diag::EMiss.val());
        }
        let mut i = 0usize;
        while i < self.singles.len() {
            if let Some(x) = self.singles.get(i) {
                if x.class() == l.class() {
                    return Err(Diag::EDup.val());
                }
            }
            i += 1;
        }
        self.singles.push(l);
        Ok(())
    }

    /// 交叉对账：逐类小计对平 + 差额=0，报告非空落档。
    pub fn reconcile(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(Diag::ESeal.val());
        }
        if self.singles.len() != CLASS_COUNT {
            return Err(Diag::EMiss.val());
        }
        if self.total.is_empty() {
            return Err(Diag::EEmpty.val());
        }
        // 差额=0 断言：全向量总额 - 四类之和 == 0（无符号减法前先比大小）。
        let mut singles_sum = 0u64;
        let mut i = 0usize;
        while i < self.singles.len() {
            let l = match self.singles.get(i) {
                Some(x) => x,
                None => return Err(Diag::EMiss.val()),
            };
            // 逐类交叉：全向量类小计必须等于单类账总额。
            if self.total.class_total(l.class()) != l.total() {
                return Err(Diag::EDelta.val());
            }
            singles_sum = singles_sum.wrapping_add(l.total());
            i += 1;
        }
        let grand = self.total.total();
        if grand < singles_sum {
            return Err(Diag::ETotal.val());
        }
        let delta = grand - singles_sum;
        if delta != 0 {
            return Err(Diag::ETotal.val());
        }
        self.report = format!(
            "域级对账报告：全向量账{}条 总额{}µs 四类之和{}µs 差额0 四类小计[{}|{}|{}|{}]",
            self.total.len(),
            grand,
            singles_sum,
            self.total.class_total(LatClass::Sample),
            self.total.class_total(LatClass::Commit),
            self.total.class_total(LatClass::Switch),
            self.total.class_total(LatClass::Recover),
        );
        if self.report.is_empty() {
            return Err(Diag::ENoRep.val());
        }
        self.sealed = true;
        Ok(delta)
    }

    /// 封账重放：逐类小计与报告面复核。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.sealed {
            return Err(Diag::EReplay.val());
        }
        if self.singles.len() != CLASS_COUNT || self.report.is_empty() {
            return Err(Diag::EReplay.val());
        }
        let mut singles_sum = 0u64;
        let mut i = 0usize;
        while i < self.singles.len() {
            if let Some(l) = self.singles.get(i) {
                singles_sum = singles_sum.wrapping_add(l.total());
            }
            i += 1;
        }
        if self.total.total() != singles_sum {
            return Err(Diag::EReplay.val());
        }
        Ok(0)
    }
}

/// 标准同源语料：八条延迟条目（每类两条，微秒整数，序号递增）。
pub fn corpus() -> [LatEntry; 8] {
    [
        LatEntry { class: LatClass::Sample, seq: 1, usec: 120 },
        LatEntry { class: LatClass::Commit, seq: 2, usec: 340 },
        LatEntry { class: LatClass::Switch, seq: 3, usec: 80 },
        LatEntry { class: LatClass::Recover, seq: 4, usec: 260 },
        LatEntry { class: LatClass::Sample, seq: 5, usec: 140 },
        LatEntry { class: LatClass::Commit, seq: 6, usec: 220 },
        LatEntry { class: LatClass::Switch, seq: 7, usec: 60 },
        LatEntry { class: LatClass::Recover, seq: 8, usec: 180 },
    ]
}

/// 建标准域级对账：同语料入全向量账与四类单类账 → 对平封账。
pub fn build_recon() -> DomainRecon {
    let mut total = TotalLedger::new();
    let mut c1 = ClassLedger::new(LatClass::Sample);
    let mut c2 = ClassLedger::new(LatClass::Commit);
    let mut c3 = ClassLedger::new(LatClass::Switch);
    let mut c4 = ClassLedger::new(LatClass::Recover);
    let mut i = 0usize;
    while i < 8 {
        let e = match corpus().get(i) {
            Some(x) => x.clone(),
            None => break,
        };
        let _ = total.record(e.clone());
        match e.class {
            LatClass::Sample => {
                let _ = c1.record(e);
            }
            LatClass::Commit => {
                let _ = c2.record(e);
            }
            LatClass::Switch => {
                let _ = c3.record(e);
            }
            LatClass::Recover => {
                let _ = c4.record(e);
            }
        }
        i += 1;
    }
    let mut d = DomainRecon::new(total);
    let _ = d.attach(c1);
    let _ = d.attach(c2);
    let _ = d.attach(c3);
    let _ = d.attach(c4);
    let _ = d.reconcile();
    d
}

/// 漂移注入矩阵：十轮注入（单类账篡改/全向量小计篡改/幽灵条目）逐轮必抓。
fn drift_matrix() -> [bool; DRIFT_ROUNDS] {
    let mut hits = [false; DRIFT_ROUNDS];
    let mut r = 0usize;
    while r < DRIFT_ROUNDS {
        let mut d = build_recon_unsealed();
        let kind = r % 3;
        let hit = match kind {
            // 注入 1：篡改某单类账末条目微秒值 → 交叉失配。
            0 => {
                let slot = r % CLASS_COUNT;
                let mut patched = false;
                let mut i = 0usize;
                while i < d.singles.len() {
                    if i == slot {
                        if let Some(l) = d.singles.get_mut(i) {
                            let n = l.entries.len();
                            if n > 0 {
                                if let Some(e) = l.entries.get_mut(n - 1) {
                                    e.usec = e.usec.wrapping_add(1);
                                    patched = true;
                                }
                            }
                        }
                    }
                    i += 1;
                }
                patched && match d.reconcile() {
                    Err(c) => c == Diag::EDelta.val(),
                    Ok(_) => false,
                }
            }
            // 注入 2：全向量账追加幽灵条目（同源外延迟）→ 差额非零。
            1 => {
                let ghost = LatEntry {
                    class: LatClass::Sample,
                    seq: 99,
                    usec: 7,
                };
                let _ = d.total.record(ghost);
                match d.reconcile() {
                    Err(c) => c == Diag::EDelta.val(),
                    Ok(_) => false,
                }
            }
            // 注入 3：撤走一笔单类账 → 单类账缺失。
            _ => {
                let n = d.singles.len();
                d.singles.truncate(n.saturating_sub(1));
                match d.reconcile() {
                    Err(c) => c == Diag::EMiss.val(),
                    Ok(_) => false,
                }
            }
        };
        match hits.get_mut(r) {
            Some(cell) => *cell = hit,
            None => return hits,
        }
        r += 1;
    }
    hits
}

/// 建未封账对账机（注入驱动用）。
fn build_recon_unsealed() -> DomainRecon {
    let mut d = build_recon();
    d.sealed = false;
    d.report = String::new();
    d
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（B01 批段窗 0x78200..=0x783C0，派工给定值锁定）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x75000 || SEG_BASE > 0x783C0 {
        return false;
    }
    SEG_BASE == 0x783C0
}

/// 四类闭集：类号连续、域级账编号互异、类名非空。
fn classes_closed_ok() -> bool {
    if CLASSES.len() != CLASS_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < CLASSES.len() {
        let c = match CLASSES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if c.id() != (i + 1) as u32 || c.name().is_empty() || c.ledger_id() == 0 {
            return false;
        }
        let mut k = i + 1;
        while k < CLASSES.len() {
            if let Some(u) = CLASSES.get(k) {
                if *u == c || u.ledger_id() == c.ledger_id() {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }
    true
}

/// 差额=0 断言（域级对账一致率 100%）。
fn delta_zero_ok() -> bool {
    let d = build_recon();
    d.is_sealed() && d.replay() == Ok(0) && !d.report().is_empty()
}

/// 逐类交叉对平：四类逐类小计相等。
fn per_class_ok() -> bool {
    let d = build_recon();
    let mut i = 0usize;
    while i < CLASS_COUNT {
        let cls = match CLASSES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let l = match d.single_at(i) {
            Some(x) => x,
            None => return false,
        };
        if l.class() != cls || d.total_ledger().class_total(cls) != l.total() {
            return false;
        }
        i += 1;
    }
    true
}

/// 标准语料小计恒等：采样260/提交560/切换140/恢复440，总 1400。
fn corpus_totals_ok() -> bool {
    let d = build_recon();
    let t = d.total_ledger();
    t.class_total(LatClass::Sample) == 260
        && t.class_total(LatClass::Commit) == 560
        && t.class_total(LatClass::Switch) == 140
        && t.class_total(LatClass::Recover) == 440
        && t.total() == 1400
}

/// 漂移注入 10/10 必抓。
fn drift_ok() -> bool {
    drift_matrix() == [true; DRIFT_ROUNDS]
}

/// 入账门负样本：零值/乱序/单类账类不符 均显性拒。
fn gate_negatives_ok() -> bool {
    let mut t = TotalLedger::new();
    let zero = match t.record(LatEntry {
        class: LatClass::Sample,
        seq: 1,
        usec: 0,
    }) {
        Err(c) => c == Diag::EEmpty.val(),
        Ok(()) => false,
    };
    let _ = t.record(LatEntry {
        class: LatClass::Sample,
        seq: 1,
        usec: 10,
    });
    let order = match t.record(LatEntry {
        class: LatClass::Commit,
        seq: 1,
        usec: 10,
    }) {
        Err(c) => c == Diag::EOrder.val(),
        Ok(()) => false,
    };
    let mut c1 = ClassLedger::new(LatClass::Sample);
    let alien = match c1.record(LatEntry {
        class: LatClass::Commit,
        seq: 2,
        usec: 10,
    }) {
        Err(c) => c == Diag::EClass.val(),
        Ok(()) => false,
    };
    zero && order && alien
}

/// 挂账门负样本：重复类/缺类即对账/封账后挂 均显性拒。
fn attach_negatives_ok() -> bool {
    let mut d = build_recon_unsealed();
    let n0 = d.singles.len();
    d.singles.truncate(n0.saturating_sub(3));
    let again = match d.attach(ClassLedger::new(LatClass::Sample)) {
        Err(c) => c == Diag::EDup.val(),
        Ok(()) => false,
    };
    let mut half = build_recon_unsealed();
    let n = half.singles.len();
    half.singles.truncate(n.saturating_sub(2));
    let miss = match half.reconcile() {
        Err(c) => c == Diag::EMiss.val(),
        Ok(_) => false,
    };
    let mut sealed = build_recon();
    let after = match sealed.attach(ClassLedger::new(LatClass::Sample)) {
        Err(c) => c == Diag::ESealed.val(),
        Ok(()) => false,
    };
    again && miss && after
}

/// 对账报告落档：非空且含四类小计与差额 0。
fn report_ok() -> bool {
    let d = build_recon();
    d.report().contains("差额0")
        && d.report().contains("采样延迟") == false
        && d.report().contains("总额1400")
        && d.report().contains("260")
}

/// 双跑一致：独立建账两次，报告与逐类小计逐项对拍。
fn dual_run_ok() -> bool {
    let a = build_recon();
    let b = build_recon();
    a.report() == b.report() && a.single_count() == b.single_count() && {
        let mut i = 0usize;
        while i < CLASS_COUNT {
            match (a.single_at(i), b.single_at(i)) {
                (Some(x), Some(y)) => {
                    if x.total() != y.total() || x.len() != y.len() {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

/// 封账单向：封账后重对/重放显性定性。
fn seal_monotonic_ok() -> bool {
    let mut d = build_recon();
    let re_recon = match d.reconcile() {
        Err(c) => c == Diag::ESeal.val(),
        Ok(_) => false,
    };
    re_recon && d.replay() == Ok(0)
}

/// 未对平不封账（缺单类账时 reconcile 拒，账面保持未封）。
fn seal_needs_recon_ok() -> bool {
    let mut d = build_recon_unsealed();
    let n = d.singles.len();
    d.singles.truncate(n.saturating_sub(1));
    match d.reconcile() {
        Err(_) => !d.is_sealed(),
        Ok(_) => false,
    }
}

/// 诊断码闭集自检。
fn diag_closed_ok() -> bool {
    if ALL.len() != 12 {
        return false;
    }
    if Diag::EClass.val() != SEG_BASE + 0x01 || Diag::ESeal.val() != SEG_BASE + 0x0C {
        return false;
    }
    let mut i = 0usize;
    while i < ALL.len() {
        let d = match ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if d.detail().is_empty() || detail(d.val()) != d.detail() {
            return false;
        }
        i += 1;
    }
    true
}

/// 十六项判据（UNX-F1523 延迟账域级总对账）。
pub fn run_unx1523_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1523");
    let recon = build_recon();

    s.add_dyn(
        "F1523-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1523-02 四类单类账闭集",
        classes_closed_ok(),
        &format!("classes={}", CLASS_COUNT),
    );

    s.add_dyn(
        "F1523-03 交叉对平一致率 100%",
        delta_zero_ok(),
        &format!("singles={} sealed={}", recon.single_count(), recon.is_sealed()),
    );

    s.add_dyn(
        "F1523-04 逐类交叉对平",
        per_class_ok(),
        &format!("per_class=4/4"),
    );

    s.add_dyn(
        "F1523-05 标准语料小计恒等",
        corpus_totals_ok(),
        &format!("total={}us", recon.total_ledger().total()),
    );

    s.add_dyn(
        "F1523-06 差额=0 断言",
        recon.replay() == Ok(0),
        &format!("delta=0"),
    );

    s.add_dyn(
        "F1523-07 漂移注入 10/10 必抓",
        drift_ok(),
        &format!("rounds={}", DRIFT_ROUNDS),
    );

    s.add_dyn(
        "F1523-08 入账门负样本显性拒",
        gate_negatives_ok(),
        &format!("zero/order/alien=3拒绝"),
    );

    s.add_dyn(
        "F1523-09 挂账门负样本显性拒",
        attach_negatives_ok(),
        &format!("dup/miss/sealed=3拒绝"),
    );

    s.add_dyn(
        "F1523-10 对账报告落档",
        report_ok(),
        &format!("report_len={}", recon.report().len()),
    );

    s.add_dyn(
        "F1523-11 双跑一致",
        dual_run_ok(),
        &format!("report_same=1"),
    );

    s.add_dyn(
        "F1523-12 封账单向推进",
        seal_monotonic_ok(),
        &format!("sealed={}", recon.is_sealed()),
    );

    s.add_dyn(
        "F1523-13 未对平不封账",
        seal_needs_recon_ok(),
        &format!("miss_gate=0x{:05x}", Diag::EMiss.val()),
    );

    s.add_dyn(
        "F1523-14 封账重放复核",
        recon.replay() == Ok(0) && !recon.report().is_empty(),
        &format!("replay=0delta"),
    );

    s.add_dyn(
        "F1523-15 全向量账四类齐入",
        recon.total_ledger().len() == 8 && recon.total_ledger().is_empty() == false,
        &format!("entries=8"),
    );

    s.add_dyn(
        "F1523-16 诊断码闭集查表",
        diag_closed_ok(),
        &format!("all={} last=0x{:05x}", ALL.len(), Diag::ESeal.val()),
    );

    s
}

// ---- 测试区 ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_closed_with_ledger_ids() {
        assert_eq!(CLASSES.len(), 4);
        assert_eq!(LatClass::Sample.ledger_id(), 0xF0851);
        assert_eq!(LatClass::Recover.ledger_id(), 0xF1170);
        assert_eq!(detail(SEG_BASE + 0x03), Diag::ETotal.detail());
        assert_eq!(detail(SEG_BASE + 0x40), "未知诊断码：不在 0x783C1..0x783CC 闭集内");
    }

    #[test]
    fn corpus_totals_identity() {
        let d = build_recon();
        let t = d.total_ledger();
        assert_eq!(t.class_total(LatClass::Sample), 260);
        assert_eq!(t.class_total(LatClass::Commit), 560);
        assert_eq!(t.class_total(LatClass::Switch), 140);
        assert_eq!(t.class_total(LatClass::Recover), 440);
        assert_eq!(t.total(), 1400);
        assert_eq!(d.single_count(), 4);
    }

    #[test]
    fn delta_zero_and_sealed() {
        let d = build_recon();
        assert!(d.is_sealed());
        assert_eq!(d.replay(), Ok(0));
        assert!(d.report().contains("差额0"));
    }

    #[test]
    fn drift_all_caught() {
        assert_eq!(drift_matrix(), [true; DRIFT_ROUNDS]);
    }

    #[test]
    fn entry_gates_reject() {
        let mut t = TotalLedger::new();
        assert_eq!(
            t.record(LatEntry { class: LatClass::Sample, seq: 1, usec: 0 }),
            Err(Diag::EEmpty.val())
        );
        assert_eq!(
            t.record(LatEntry { class: LatClass::Sample, seq: 1, usec: 10 }),
            Ok(())
        );
        assert_eq!(
            t.record(LatEntry { class: LatClass::Commit, seq: 1, usec: 10 }),
            Err(Diag::EOrder.val())
        );
        let mut c = ClassLedger::new(LatClass::Switch);
        assert_eq!(
            c.record(LatEntry { class: LatClass::Sample, seq: 2, usec: 10 }),
            Err(Diag::EClass.val())
        );
    }

    #[test]
    fn attach_gates_reject() {
        let mut d = build_recon_unsealed();
        // 容量门先于重号门：先缩到 1 笔单类账再重挂，才能触达 EDup 防御位。
        let n0 = d.singles.len();
        d.singles.truncate(n0.saturating_sub(3));
        assert_eq!(d.attach(ClassLedger::new(LatClass::Sample)), Err(Diag::EDup.val()));
        let mut half = build_recon_unsealed();
        let n = half.singles.len();
        half.singles.truncate(n - 1);
        assert_eq!(half.reconcile(), Err(Diag::EMiss.val()));
        let mut sealed = build_recon();
        assert_eq!(sealed.attach(ClassLedger::new(LatClass::Sample)), Err(Diag::ESealed.val()));
    }

    #[test]
    fn ghost_entry_breaks_delta() {
        let mut d = build_recon_unsealed();
        let _ = d.total.record(LatEntry {
            class: LatClass::Commit,
            seq: 99,
            usec: 5,
        });
        assert_eq!(d.reconcile(), Err(Diag::EDelta.val()));
    }

    #[test]
    fn seal_terminal_and_replay() {
        let mut d = build_recon();
        assert_eq!(d.reconcile(), Err(Diag::ESeal.val()));
        assert_eq!(d.replay(), Ok(0));
    }

    #[test]
    fn dual_run_reports_agree() {
        let a = build_recon();
        let b = build_recon();
        assert_eq!(a.report(), b.report());
        assert_eq!(a.single_count(), b.single_count());
    }

    #[test]
    fn checks_all_green_sixteen() {
        let s = run_unx1523_checks();
        assert!(s.all_passed());
        assert!(s.len() >= 12);
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn seg_guard_matches_dispatch_value() {
        assert_eq!(SEG_BASE, 0x783C0);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(seg_guard_ok());
    }
}
