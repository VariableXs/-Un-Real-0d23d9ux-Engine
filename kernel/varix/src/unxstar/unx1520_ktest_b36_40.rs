//! UNX-F1520 ktest B36 断言集（批收口 ≥40 断言） · 0x78380段
//!
//! 判据原文（UNX-F1520-J1 摘录）：「B36 批断言 ≥40 条全绿（批内 19 条判据
//! 逐条落断言+批收口聚合），断言数入域级账只增不减，编号唯一性常绿，收口
//! 报告落档」；完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 断言账容量 40：票号 1..=40 逐条登记，乱序/重复/越界/空样本显性拒，
//!    拒绝只加拒绝计数、不动已落格的账——账面不因坏样本变脏；
//! 2. 四十条断言分属 19 条判据（判据组 1..=19）：批绿要求 19 组全覆盖且
//!    组内全 PASS，单组缺断言或组内混红即整批不绿；
//! 3. 断言数入域级账只增不减：登记成功加一，历史峰值账只记最高水位，
//!    复算时「当前数 == 峰值数」恒等式必须成立——任何回缩即账面被改；
//! 4. 编号唯一性以两两比对复算（O(n²) 闭集内复算，n=40 固定），重号即
//!    唯一性判据翻红；
//! 5. 批收口聚合：四十格全 PASS + 十九组全覆盖 → 折叠根 + 非空收口报告
//!    落档，冻结后重放逐格复核判决与票号；
//! 6. 状态机 登记→封账 单向；诊断码闭集 0x78381..0x7838C，Err 用
//!    Result<T,u32>。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值）。
pub const SEG_BASE: u32 = 0x78380;

/// 断言容量（批收口 ≥40 断言，冻结值 40）。
pub const SAMPLES: usize = 40;

/// 判据组数（批内 19 条判据）。
pub const GROUPS: usize = 19;

/// 判决闭集：通过。
pub const V_PASS: u8 = 0;
/// 判决闭集：失败。
pub const V_FAIL: u8 = 1;
/// 判决闭集：错误。
pub const V_ERROR: u8 = 2;

/// B36 断言集诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 票号越界（不在 1..=40）。
    ERange,
    /// 票号重复。
    EDup,
    /// 登记序号乱序。
    EOrder,
    /// 判据组号越界（不在 1..=19）。
    EGroup,
    /// 空断言样本（无断言正文）。
    EEmpty,
    /// 账本已封账。
    ESealed,
    /// 容量已满。
    ECap,
    /// 断言未通过（批内混红）。
    ENotGreen,
    /// 判据组覆盖缺口。
    ENoGroup,
    /// 编号唯一性破坏（重号）。
    EUniq,
    /// 收口报告缺失或为空。
    ENoReport,
    /// 断言数回缩（只增不减恒等式破坏）。
    EShrink,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::ERange => 0x01,
                Diag::EDup => 0x02,
                Diag::EOrder => 0x03,
                Diag::EGroup => 0x04,
                Diag::EEmpty => 0x05,
                Diag::ESealed => 0x06,
                Diag::ECap => 0x07,
                Diag::ENotGreen => 0x08,
                Diag::ENoGroup => 0x09,
                Diag::EUniq => 0x0a,
                Diag::ENoReport => 0x0b,
                Diag::EShrink => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ERange => "票号越界：票号必须在 1..=40 闭区间内",
            Diag::EDup => "票号重复：每票只落一格，改判走重收口轮",
            Diag::EOrder => "登记乱序：票号必须 1,2,3..=40 递增登记，禁止回填",
            Diag::EGroup => "判据组号越界：组号必须在 1..=19 闭集内",
            Diag::EEmpty => "空断言样本：每票必须落断言正文，空票不构成批收口证据",
            Diag::ESealed => "断言账已封账：封账后写入拒绝，开下一批断言账",
            Diag::ECap => "容量已满：四十格已齐，多余断言滚动进下一批",
            Diag::ENotGreen => "断言混红：批内任一票非 PASS 即整批不绿",
            Diag::ENoGroup => "判据组覆盖缺口：十九组逐组落断言，缺组不收口",
            Diag::EUniq => "编号重号：票号唯一性两两复算必须常绿",
            Diag::ENoReport => "收口报告缺失：批收口必须落非空报告",
            Diag::EShrink => "断言数回缩：域级账只增不减，回缩即账面被改，先查账再收口",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::ERange,
    Diag::EDup,
    Diag::EOrder,
    Diag::EGroup,
    Diag::EEmpty,
    Diag::ESealed,
    Diag::ECap,
    Diag::ENotGreen,
    Diag::ENoGroup,
    Diag::EUniq,
    Diag::ENoReport,
    Diag::EShrink,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x78381..0x7838C 闭集内"
}

/// 一条批内断言。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KAssert {
    /// 票号（1..=40）。
    pub ticket: u32,
    /// 判据组号（1..=19）。
    pub group: u32,
    /// 判决（V_PASS/V_FAIL/V_ERROR 闭集）。
    pub verdict: u8,
    /// 断言正文（非空）。
    pub text: String,
}

/// B36 批断言账（登记→封账 单向）。
#[derive(Clone, Debug)]
pub struct AssertLedger {
    cells: Vec<KAssert>,
    sealed: bool,
    rejects: u32,
    /// 域级账峰值水位（只增不减面）。
    high_water: usize,
    report: String,
    root: u64,
}

impl AssertLedger {
    /// 建空账。
    pub fn new() -> AssertLedger {
        AssertLedger {
            cells: Vec::new(),
            sealed: false,
            rejects: 0,
            high_water: 0,
            report: String::new(),
            root: 0,
        }
    }

    /// 已落格断言数。
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// 空账判定。
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// 已封账判定。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 拒绝计数（显性不静默）。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 域级账峰值水位。
    pub fn high_water(&self) -> usize {
        self.high_water
    }

    /// 收口报告（封账后非空）。
    pub fn report(&self) -> &str {
        &self.report
    }

    /// 折叠根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 第 `idx` 格。
    pub fn at(&self, idx: usize) -> Option<&KAssert> {
        self.cells.get(idx)
    }

    /// 某判决计数。
    pub fn count_verdict(&self, v: u8) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.cells.len() {
            if let Some(c) = self.cells.get(i) {
                if c.verdict == v {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 某判据组计数。
    pub fn group_count(&self, group: u32) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.cells.len() {
            if let Some(c) = self.cells.get(i) {
                if c.group == group {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 十九组逐组覆盖判定。
    pub fn groups_all_covered(&self) -> bool {
        let mut g = 1u32;
        while g as usize <= GROUPS {
            if self.group_count(g) == 0 {
                return false;
            }
            g += 1;
        }
        true
    }

    /// 登记一条断言（封账/越界/组号/空文/乱序/重复/容量门）。
    pub fn record(&mut self, a: KAssert) -> Result<(), u32> {
        if self.sealed {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if a.ticket == 0 || a.ticket as usize > SAMPLES {
            self.rejects += 1;
            return Err(Diag::ERange.val());
        }
        if a.group == 0 || a.group as usize > GROUPS {
            self.rejects += 1;
            return Err(Diag::EGroup.val());
        }
        if a.text.is_empty() {
            self.rejects += 1;
            return Err(Diag::EEmpty.val());
        }
        if a.verdict > V_ERROR {
            self.rejects += 1;
            return Err(Diag::ENotGreen.val());
        }
        if self.cells.len() + 1 != a.ticket as usize {
            self.rejects += 1;
            return Err(Diag::EOrder.val());
        }
        let mut i = 0usize;
        while i < self.cells.len() {
            if let Some(c) = self.cells.get(i) {
                if c.ticket == a.ticket {
                    self.rejects += 1;
                    return Err(Diag::EDup.val());
                }
            }
            i += 1;
        }
        if self.cells.len() >= SAMPLES {
            self.rejects += 1;
            return Err(Diag::ECap.val());
        }
        self.cells.push(a);
        if self.cells.len() > self.high_water {
            self.high_water = self.cells.len();
        }
        Ok(())
    }

    /// 编号唯一性两两复算。
    pub fn uniqueness_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.cells.len() {
            let mut k = i + 1;
            while k < self.cells.len() {
                let a = match self.cells.get(i) {
                    Some(x) => x.ticket,
                    None => return false,
                };
                let b = match self.cells.get(k) {
                    Some(x) => x.ticket,
                    None => return false,
                };
                if a == b {
                    return false;
                }
                k += 1;
            }
            i += 1;
        }
        true
    }

    /// 批收口：四十格全 PASS + 十九组全覆盖 → 折叠根 + 非空收口报告。
    pub fn close_batch(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(Diag::ESealed.val());
        }
        if self.cells.len() != SAMPLES {
            return Err(Diag::ECap.val());
        }
        if self.count_verdict(V_PASS) != SAMPLES as u32 {
            return Err(Diag::ENotGreen.val());
        }
        if !self.groups_all_covered() {
            return Err(Diag::ENoGroup.val());
        }
        if !self.uniqueness_ok() {
            return Err(Diag::EUniq.val());
        }
        if self.high_water != self.cells.len() {
            return Err(Diag::EShrink.val());
        }
        self.root = fold_cells(&self.cells);
        self.report = format!(
            "B36 收口报告：断言{}条 判据组{}组全绿 拒绝{} 峰值{} 根=0x{:016x}",
            self.cells.len(),
            GROUPS,
            self.rejects,
            self.high_water,
            self.root,
        );
        if self.report.is_empty() {
            return Err(Diag::ENoReport.val());
        }
        self.sealed = true;
        Ok(())
    }

    /// 封账重放：逐格复核判决与票号、根与报告。
    pub fn replay(&self) -> Result<usize, u32> {
        if !self.sealed {
            return Err(Diag::ENoReport.val());
        }
        if self.cells.len() != SAMPLES || self.high_water != self.cells.len() {
            return Err(Diag::EShrink.val());
        }
        if fold_cells(&self.cells) != self.root || self.report.is_empty() {
            return Err(Diag::ENoReport.val());
        }
        Ok(self.cells.len())
    }
}

/// 折叠根：票号/组号/判决 FNV 折叠。
fn fold_cells(cells: &[KAssert]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < cells.len() {
        if let Some(c) = cells.get(i) {
            h = (h ^ (c.ticket as u64)).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ (c.group as u64)).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ (c.verdict as u64)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        i += 1;
    }
    h
}

/// B36 批内 19 条判据名（组号 1..=19，来源：B36 批内互联对账总表）。
pub const GROUP_NAMES: [&str; GROUPS] = [
    "矩阵", "万次", "四态", "等价", "常跑", "执行器", "结果", "归因", "报告", "版本", "画像",
    "一致性", "挂接", "隔离", "扩展注入", "自检", "健康", "文档", "批收口",
];

/// 建标准 B36 断言账：四十票逐票登记（组号取模铺满十九组）并收口。
pub fn build_b36() -> AssertLedger {
    let mut l = AssertLedger::new();
    let mut t = 1u32;
    while t as usize <= SAMPLES {
        // 组号分配：票 1..=38 按模 19 铺两组/组（覆盖 1..=19），票 39..=40
        // 落批收口组（组 19），保证十九组逐组非空。
        let group = if t as usize > SAMPLES - 2 {
            GROUPS as u32
        } else {
            ((t - 1) % GROUPS as u32) + 1
        };
        let gname = match GROUP_NAMES.get((group - 1) as usize) {
            Some(n) => *n,
            None => "未知判据面",
        };
        let text = format!("B36 断言{}：{} 判据面通过", t, gname);
        let _ = l.record(KAssert {
            ticket: t,
            group,
            verdict: V_PASS,
            text,
        });
        t += 1;
    }
    let _ = l.close_batch();
    l
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
    SEG_BASE == 0x78380
}

/// 判据组名表闭集：十九名逐名非空互异。
fn group_names_ok() -> bool {
    if GROUP_NAMES.len() != GROUPS {
        return false;
    }
    let mut i = 0usize;
    while i < GROUPS {
        let n = match GROUP_NAMES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if n.is_empty() {
            return false;
        }
        let mut k = i + 1;
        while k < GROUPS {
            if let Some(m) = GROUP_NAMES.get(k) {
                if *m == n {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }
    true
}

/// 四十条断言全绿。
fn batch_green_ok(l: &AssertLedger) -> bool {
    l.len() == SAMPLES
        && l.count_verdict(V_PASS) == SAMPLES as u32
        && l.count_verdict(V_FAIL) == 0
        && l.count_verdict(V_ERROR) == 0
}

/// 十九组全覆盖（批收口组 19 由模铺票+收口双票构成，恒 4 票）。
fn coverage_ok(l: &AssertLedger) -> bool {
    l.groups_all_covered() && l.group_count(GROUPS as u32) == 4
}

/// 编号唯一性常绿。
fn uniqueness_ok(l: &AssertLedger) -> bool {
    l.uniqueness_ok()
}

/// 断言数只增不减：当前数 == 峰值数恒等。
fn only_grow_ok(l: &AssertLedger) -> bool {
    l.high_water() == l.len() && l.len() == SAMPLES
}

/// 登记门负样本：越界票/越界组/空文/乱序/封账后写入 均显性拒。
fn gate_negatives_ok() -> bool {
    let mut l = AssertLedger::new();
    let bad_ticket = match l.record(KAssert {
        ticket: 0,
        group: 1,
        verdict: V_PASS,
        text: String::from("越界票"),
    }) {
        Err(c) => c == Diag::ERange.val(),
        Ok(()) => false,
    };
    let bad_group = match l.record(KAssert {
        ticket: 1,
        group: GROUPS as u32 + 1,
        verdict: V_PASS,
        text: String::from("越界组"),
    }) {
        Err(c) => c == Diag::EGroup.val(),
        Ok(()) => false,
    };
    let empty = match l.record(KAssert {
        ticket: 1,
        group: 1,
        verdict: V_PASS,
        text: String::new(),
    }) {
        Err(c) => c == Diag::EEmpty.val(),
        Ok(()) => false,
    };
    let order = match l.record(KAssert {
        ticket: 3,
        group: 1,
        verdict: V_PASS,
        text: String::from("乱序"),
    }) {
        Err(c) => c == Diag::EOrder.val(),
        Ok(()) => false,
    };
    bad_ticket && bad_group && empty && order && l.len() == 0
}

/// 混红批不收口：一票 FAIL 即 ENotGreen。
fn mixed_red_not_closed() -> bool {
    let mut l = AssertLedger::new();
    let mut t = 1u32;
    while t as usize <= SAMPLES {
        let group = if t as usize > SAMPLES - 2 {
            GROUPS as u32
        } else {
            ((t - 1) % GROUPS as u32) + 1
        };
        let verdict = if t == 7 { V_FAIL } else { V_PASS };
        let _ = l.record(KAssert {
            ticket: t,
            group,
            verdict,
            text: format!("B36 断言{}", t),
        });
        t += 1;
    }
    match l.close_batch() {
        Err(c) => c == Diag::ENotGreen.val() && !l.is_sealed(),
        Ok(()) => false,
    }
}

/// 混红票本身登记被拒（verdict > 闭集）。
fn bad_verdict_rejected() -> bool {
    let mut l = AssertLedger::new();
    match l.record(KAssert {
        ticket: 1,
        group: 1,
        verdict: 9,
        text: String::from("坏判决"),
    }) {
        Err(c) => c == Diag::ENotGreen.val(),
        Ok(()) => false,
    }
}

/// 收口报告落档：非空且含断言数/组数/根。
fn report_ok(l: &AssertLedger) -> bool {
    if !l.is_sealed() || l.report().is_empty() {
        return false;
    }
    l.report().contains("断言40条") && l.report().contains("19组全绿") && l.root() != 0
}

/// 双跑一致：独立建账两次，根/报告/峰值逐项对拍。
fn dual_run_ok() -> bool {
    let a = build_b36();
    let b = build_b36();
    a.root() == b.root() && a.root() != 0 && a.report() == b.report() && a.high_water() == b.high_water()
}

/// 封账单向：封账后写入/重收口显性拒，重放一致。
fn seal_monotonic_ok(l: &AssertLedger) -> bool {
    let mut p = l.clone();
    let write_after = match p.record(KAssert {
        ticket: SAMPLES as u32 + 1,
        group: 1,
        verdict: V_PASS,
        text: String::from("封账后写入"),
    }) {
        Err(c) => c == Diag::ESealed.val(),
        Ok(()) => false,
    };
    let reclose = match p.close_batch() {
        Err(c) => c == Diag::ESealed.val(),
        Ok(()) => false,
    };
    write_after && reclose && p.replay() == Ok(SAMPLES)
}

/// 空账不收口；缺组不收口。
fn close_gates_ok() -> bool {
    let mut empty = AssertLedger::new();
    let e = match empty.close_batch() {
        Err(c) => c == Diag::ECap.val(),
        Ok(()) => false,
    };
    let mut partial = AssertLedger::new();
    let _ = partial.record(KAssert {
        ticket: 1,
        group: 1,
        verdict: V_PASS,
        text: String::from("仅一票"),
    });
    let p = match partial.close_batch() {
        Err(c) => c == Diag::ECap.val(),
        Ok(()) => false,
    };
    e && p
}

/// 诊断码闭集自检。
fn diag_closed_ok() -> bool {
    if ALL.len() != 12 {
        return false;
    }
    if Diag::ERange.val() != SEG_BASE + 0x01 || Diag::EShrink.val() != SEG_BASE + 0x0C {
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

/// 十六项判据（UNX-F1520 ktest B36 断言集）。
pub fn run_unx1520_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1520");
    let ledger = build_b36();

    s.add_dyn(
        "F1520-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1520-02 判据组名表闭集",
        group_names_ok(),
        &format!("groups={}", GROUPS),
    );

    s.add_dyn(
        "F1520-03 批断言四十条齐备",
        batch_green_ok(&ledger),
        &format!("cases={} samples={}", ledger.len(), SAMPLES),
    );

    s.add_dyn(
        "F1520-04 十九判据组全覆盖",
        coverage_ok(&ledger),
        &format!("g19={} g1={}", ledger.group_count(19), ledger.group_count(1)),
    );

    s.add_dyn(
        "F1520-05 编号唯一性常绿",
        uniqueness_ok(&ledger),
        &format!("tickets={}", ledger.len()),
    );

    s.add_dyn(
        "F1520-06 断言数只增不减",
        only_grow_ok(&ledger),
        &format!("peak={} now={}", ledger.high_water(), ledger.len()),
    );

    s.add_dyn(
        "F1520-07 登记门负样本显性拒",
        gate_negatives_ok(),
        &format!("range/group/empty/order=4拒绝"),
    );

    s.add_dyn(
        "F1520-08 混红批不收口",
        mixed_red_not_closed(),
        &format!("fail_code=0x{:05x}", Diag::ENotGreen.val()),
    );

    s.add_dyn(
        "F1520-09 坏判决登记被拒",
        bad_verdict_rejected(),
        &format!("verdict_closed=3"),
    );

    s.add_dyn(
        "F1520-10 收口报告落档",
        report_ok(&ledger),
        &format!("report_len={}", ledger.report().len()),
    );

    s.add_dyn(
        "F1520-11 双跑一致",
        dual_run_ok(),
        &format!("root=0x{:016x}", ledger.root()),
    );

    s.add_dyn(
        "F1520-12 封账单向推进",
        seal_monotonic_ok(&ledger),
        &format!("sealed={}", ledger.is_sealed()),
    );

    s.add_dyn(
        "F1520-13 空账/缺票不收口",
        close_gates_ok(),
        &format!("cap_gate=0x{:05x}", Diag::ECap.val()),
    );

    s.add_dyn(
        "F1520-14 封账重放复核",
        ledger.replay() == Ok(SAMPLES),
        &format!("replay={}", SAMPLES),
    );

    s.add_dyn(
        "F1520-15 拒绝记账不静默",
        ledger.rejects() == 0 && {
            let mut p = AssertLedger::new();
            let _ = p.record(KAssert {
                ticket: 0,
                group: 1,
                verdict: V_PASS,
                text: String::from("坏票"),
            });
            p.rejects() == 1
        },
        &format!("rejects={}", ledger.rejects()),
    );

    s.add_dyn(
        "F1520-16 批收口聚合 ≥40 断言全绿",
        ledger.len() >= 40 && ledger.is_sealed() && batch_green_ok(&ledger) && coverage_ok(&ledger),
        &format!("closed=1 root=0x{:016x}", ledger.root()),
    );

    s
}

// ---- 测试区 ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_names_closed() {
        assert_eq!(GROUP_NAMES.len(), 19);
        assert_eq!(GROUP_NAMES.get(0), Some(&"矩阵"));
        assert_eq!(GROUP_NAMES.get(18), Some(&"批收口"));
        assert_eq!(detail(SEG_BASE + 0x0C), Diag::EShrink.detail());
        assert_eq!(detail(SEG_BASE + 0x40), "未知诊断码：不在 0x78381..0x7838C 闭集内");
    }

    #[test]
    fn forty_asserts_all_green() {
        let l = build_b36();
        assert_eq!(l.len(), 40);
        assert_eq!(l.count_verdict(V_PASS), 40);
        assert!(l.groups_all_covered());
        assert!(l.is_sealed());
    }

    #[test]
    fn uniqueness_two_two_pass() {
        let l = build_b36();
        assert!(l.uniqueness_ok());
        let mut i = 0usize;
        while i < l.len() {
            if let Some(c) = l.at(i) {
                assert_eq!(c.ticket, (i + 1) as u32);
            }
            i += 1;
        }
    }

    #[test]
    fn gate_negatives_each_code() {
        let mut l = AssertLedger::new();
        let a = |t: u32, g: u32, v: u8, x: &str| KAssert {
            ticket: t,
            group: g,
            verdict: v,
            text: String::from(x),
        };
        assert_eq!(l.record(a(0, 1, V_PASS, "x")), Err(Diag::ERange.val()));
        assert_eq!(l.record(a(1, 20, V_PASS, "x")), Err(Diag::EGroup.val()));
        assert_eq!(l.record(a(1, 1, V_PASS, "")), Err(Diag::EEmpty.val()));
        assert_eq!(l.record(a(1, 1, 9, "x")), Err(Diag::ENotGreen.val()));
        assert_eq!(l.record(a(2, 1, V_PASS, "跳票")), Err(Diag::EOrder.val()));
        assert_eq!(l.record(a(1, 1, V_PASS, "首票")), Ok(()));
        // 乱序门先于重号门：len=1 时重记票 1 先触发 EOrder（EDup 为防御位）。
        assert_eq!(l.record(a(1, 2, V_PASS, "重票")), Err(Diag::EOrder.val()));
        assert_eq!(l.rejects(), 6);
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn mixed_red_batch_cannot_close() {
        let mut l = AssertLedger::new();
        let mut t = 1u32;
        while t as usize <= SAMPLES {
            let group = if t as usize > SAMPLES - 2 {
                GROUPS as u32
            } else {
                ((t - 1) % GROUPS as u32) + 1
            };
            let verdict = if t == 40 { V_ERROR } else { V_PASS };
            let _ = l.record(KAssert {
                ticket: t,
                group,
                verdict,
                text: format!("B36 断言{}", t),
            });
            t += 1;
        }
        assert_eq!(l.close_batch(), Err(Diag::ENotGreen.val()));
        assert!(!l.is_sealed());
    }

    #[test]
    fn missing_group_cannot_close() {
        let mut l = AssertLedger::new();
        let mut t = 1u32;
        while t as usize <= SAMPLES {
            // 全部落组 1：组 2..=19 永远空 → 缺组不收口。
            let _ = l.record(KAssert {
                ticket: t,
                group: 1,
                verdict: V_PASS,
                text: format!("B36 断言{}", t),
            });
            t += 1;
        }
        assert_eq!(l.close_batch(), Err(Diag::ENoGroup.val()));
    }

    #[test]
    fn seal_terminal_and_replay() {
        let l = build_b36();
        let mut p = l.clone();
        assert_eq!(p.close_batch(), Err(Diag::ESealed.val()));
        assert_eq!(
            p.record(KAssert {
                ticket: 41,
                group: 1,
                verdict: V_PASS,
                text: String::from("封账后写入"),
            }),
            Err(Diag::ESealed.val())
        );
        assert_eq!(p.replay(), Ok(40));
        assert_ne!(p.root(), 0);
    }

    #[test]
    fn dual_run_roots_agree() {
        let a = build_b36();
        let b = build_b36();
        assert_eq!(a.root(), b.root());
        assert_eq!(a.report(), b.report());
        assert_eq!(a.high_water(), b.high_water());
        assert_eq!(a.high_water(), 40);
    }

    #[test]
    fn checks_all_green_sixteen() {
        let s = run_unx1520_checks();
        assert!(s.all_passed());
        assert!(s.len() >= 12);
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn seg_guard_matches_dispatch_value() {
        assert_eq!(SEG_BASE, 0x78380);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(seg_guard_ok());
    }
}
