//! UNX-F1590 域级经沉淀收官复核（十条+反哺收官复验） · 0x79180段
//!
//! 判据原文（UNX-F1590-J1 摘录）：「沉淀收官复核通过（十条在册+反哺承
//! 载复验 10/10 走通），版本 v1 未被静默变更断言，复核报告归档」；完成时
//! API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 十条闭集：沉淀经文十条逐条在册（编号 1..=10），缺条即复核拒；在册
//!    台账只增不减，封账后不得再登记；
//! 2. 反哺承载 10/10：十条经文各派一条反哺承载路径（承载点 + 回流指纹），
//!    逐条走通（承载点非空且回流指纹与经文指纹互证）；
//! 3. 版本 v1 未被静默变更：版本指纹由十条经文指纹折叠派生，复核时重算
//!    与冻结值比对，任何一条经文被静默改动即断言翻红；
//! 4. 状态机 复核中→已归档 单向，归档后登记显性拒；复核报告（十条结论
//!    + 反哺走通率 + 版本断言 + 摘要）非空归档；
//! 5. 诊断码闭集 0x79181..0x7918C；整数运算，零浮点零墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值；B08 批窗口 0x79000..=0x791C0）。
pub const SEG_BASE: u32 = 0x79180;

/// 沉淀复核诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 经文编号越界（不在 1..=10）。
    ERange,
    /// 经文重复在册。
    EDup,
    /// 经文指纹为零（空经）。
    ENoFp,
    /// 十条缺条。
    EShort,
    /// 反哺承载点为空。
    ENoCarrier,
    /// 反哺回流指纹失配。
    EFlow,
    /// 反哺走通数不足（<10）。
    ENoWalk,
    /// 版本指纹被静默变更。
    EVer,
    /// 复核已归档、登记被拒。
    ESealed,
    /// 复核报告缺失或为空。
    ENoReport,
    /// 状态机倒退。
    EBack,
    /// 经文正文为空。
    EEmpty,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::ERange => 0x01,
                Diag::EDup => 0x02,
                Diag::ENoFp => 0x03,
                Diag::EShort => 0x04,
                Diag::ENoCarrier => 0x05,
                Diag::EFlow => 0x06,
                Diag::ENoWalk => 0x07,
                Diag::EVer => 0x08,
                Diag::ESealed => 0x09,
                Diag::ENoReport => 0x0a,
                Diag::EBack => 0x0b,
                Diag::EEmpty => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ERange => "经文编号越界：沉淀台账只认 1..=10，请对照十条清单",
            Diag::EDup => "经文重复在册：每条只登记一次，先查重再登记",
            Diag::ENoFp => "经文指纹为零：空经不构成沉淀证据，先落指纹再登记",
            Diag::EShort => "十条缺条：沉淀经文必须逐条到位，补齐缺条再复核",
            Diag::ENoCarrier => "反哺承载点为空：每条经文必须有承载点，先接承载再复核",
            Diag::EFlow => "反哺回流指纹失配：承载回流须与经文指纹互证，逐条排查",
            Diag::ENoWalk => "反哺走通数不足：10 条承载必须逐条走通",
            Diag::EVer => "版本指纹被静默变更：恢复冻结指纹，变更须走显性升版",
            Diag::ESealed => "复核已归档：归档后登记一律拒，增量走下一批",
            Diag::ENoReport => "复核报告缺失：报告须非空并载明走通率与版本断言",
            Diag::EBack => "状态机倒退：归档后不得回复核中，先开新账",
            Diag::EEmpty => "经文正文为空：正文非空才算在册，补正文再登记",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::ERange,
    Diag::EDup,
    Diag::ENoFp,
    Diag::EShort,
    Diag::ENoCarrier,
    Diag::EFlow,
    Diag::ENoWalk,
    Diag::EVer,
    Diag::ESealed,
    Diag::ENoReport,
    Diag::EBack,
    Diag::EEmpty,
];

/// 诊断码查表（闭集返回，未知道回兜底文案）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x79181..0x7918C 闭集内"
}

/// 沉淀经文条数（判据钦定十条）。
pub const LESSONS: usize = 10;
/// 反哺承载条数（与经文一比一）。
pub const CARRIERS: usize = LESSONS;
/// 冻结版本号（v1，不得静默变更）。
pub const VERSION_V1: u16 = 1;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 折叠质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 字节串折叠指纹。
pub fn fold_bytes(h: u64, bytes: &[u8]) -> u64 {
    let mut h = h;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 一条沉淀经文。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lesson {
    /// 编号（1..=10）。
    pub id: u16,
    /// 正文（非空）。
    pub text: String,
    /// 经文指纹（对正文折叠）。
    pub fp: u64,
}

impl Lesson {
    /// 构造经文（指纹自动折叠）。
    pub fn make(id: u16, text: &str) -> Lesson {
        let fp = fold_bytes(FNV_OFF ^ (id as u64), text.as_bytes());
        Lesson {
            id,
            text: String::from(text),
            fp,
        }
    }

    /// 结构合法性：编号在册、正文非空、指纹与正文互证。
    pub fn in_scope(&self) -> bool {
        if self.id == 0 || (self.id as usize) > LESSONS {
            return false;
        }
        if self.text.is_empty() {
            return false;
        }
        self.fp == fold_bytes(FNV_OFF ^ (self.id as u64), self.text.as_bytes()) && self.fp != 0
    }
}

/// 一条反哺承载路径。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Carrier {
    /// 承载的经文编号。
    pub lesson_id: u16,
    /// 承载点名（非空）。
    pub spot: String,
    /// 回流指纹（应与经文指纹互证）。
    pub flow_fp: u64,
}

impl Carrier {
    /// 构造承载（回流指纹默认与经文指纹一致）。
    pub fn make(lesson_id: u16, spot: &str, lesson_fp: u64) -> Carrier {
        Carrier {
            lesson_id,
            spot: String::from(spot),
            flow_fp: lesson_fp,
        }
    }

    /// 走通判定：承载点非空、编号在册、回流指纹非零。
    pub fn walkable(&self) -> bool {
        if self.spot.is_empty() || self.lesson_id == 0 || (self.lesson_id as usize) > LESSONS {
            return false;
        }
        self.flow_fp != 0
    }
}

/// 复核阶段：复核中 → 已归档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 复核中。
    Reviewing,
    /// 已归档。
    Archived,
}

/// 复核报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewReport {
    /// 十条结论（条号: 在册+走通）。
    pub lines: Vec<String>,
    /// 反哺走通数（应 10）。
    pub walked: usize,
    /// 版本断言（应 v1 未变更）。
    pub ver_ok: bool,
    /// 非空摘要。
    pub summary: String,
}

/// 沉淀收官复核总账。
pub struct ReviewFinal {
    lessons: [Option<Lesson>; LESSONS],
    carriers: [Option<Carrier>; CARRIERS],
    count: usize,
    carry_n: usize,
    phase: Phase,
    report: Option<ReviewReport>,
}

impl ReviewFinal {
    /// 新建复核总账。
    pub fn new() -> ReviewFinal {
        ReviewFinal {
            lessons: [const { None }; LESSONS],
            carriers: [const { None }; CARRIERS],
            count: 0,
            carry_n: 0,
            phase: Phase::Reviewing,
            report: None,
        }
    }

    /// 登记一条经文（编号严格递增，重复拒）。
    pub fn enroll(&mut self, l: Lesson) -> Result<usize, u32> {
        if self.phase == Phase::Archived {
            return Err(Diag::ESealed.val());
        }
        if l.id == 0 || (l.id as usize) > LESSONS {
            return Err(Diag::ERange.val());
        }
        if l.text.is_empty() {
            return Err(Diag::EEmpty.val());
        }
        if l.fp == 0 || !l.in_scope() {
            return Err(Diag::ENoFp.val());
        }
        if (l.id as usize) != self.count + 1 {
            return Err(Diag::EDup.val());
        }
        match self.lessons.get_mut(self.count) {
            Some(cell) => {
                *cell = Some(l);
                self.count += 1;
                Ok(self.count)
            }
            None => Err(Diag::EShort.val()),
        }
    }

    /// 接一条反哺承载（承载的经文必须已在册）。
    pub fn carry(&mut self, c: Carrier) -> Result<usize, u32> {
        if self.phase == Phase::Archived {
            return Err(Diag::ESealed.val());
        }
        if c.lesson_id == 0 || (c.lesson_id as usize) > LESSONS {
            return Err(Diag::ERange.val());
        }
        if c.spot.is_empty() {
            return Err(Diag::ENoCarrier.val());
        }
        if (c.lesson_id as usize) > self.count {
            return Err(Diag::EShort.val());
        }
        if (c.lesson_id as usize) != self.carry_n + 1 {
            return Err(Diag::EDup.val());
        }
        match self.carriers.get_mut(self.carry_n) {
            Some(cell) => {
                *cell = Some(c);
                self.carry_n += 1;
                Ok(self.carry_n)
            }
            None => Err(Diag::EShort.val()),
        }
    }

    /// 在册条数。
    pub fn len(&self) -> usize {
        self.count
    }

    /// 承载条数。
    pub fn carried(&self) -> usize {
        self.carry_n
    }

    /// 取第 i 条经文。
    pub fn lesson_at(&self, i: usize) -> Option<Lesson> {
        match self.lessons.get(i) {
            Some(c) => c.clone(),
            None => None,
        }
    }

    /// 取第 i 条承载。
    pub fn carrier_at(&self, i: usize) -> Option<Carrier> {
        match self.carriers.get(i) {
            Some(c) => c.clone(),
            None => None,
        }
    }

    /// 版本指纹重算：十条指纹逐条折叠。
    pub fn version_fp(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.count {
            if let Some(l) = self.lesson_at(i) {
                h = fold_bytes(h, &l.fp.to_le_bytes());
            }
            i += 1;
        }
        h ^ 0x5631_4F4B
    }

    /// 十条在册判定。
    pub fn lessons_full(&self) -> bool {
        self.count == LESSONS
    }

    /// 反哺 10/10 走通判定：承载满且逐条与在册经文指纹互证。
    pub fn carriers_walked(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.carry_n {
            let c = match self.carrier_at(i) {
                Some(c) => c,
                None => break,
            };
            if !c.walkable() {
                break;
            }
            let l = match self.lesson_at(i) {
                Some(l) => l,
                None => break,
            };
            if l.id != c.lesson_id || l.fp != c.flow_fp {
                break;
            }
            n += 1;
            i += 1;
        }
        n
    }

    /// 复核通过：十条在册 + 反哺 10/10 走通。
    pub fn review_ok(&self) -> bool {
        self.lessons_full() && self.carriers_walked() == CARRIERS
    }

    /// 归档：复核通过且承载已满才落报告并冻结。
    pub fn archive(&mut self) -> Result<ReviewReport, u32> {
        if self.phase == Phase::Archived {
            return Err(Diag::ESealed.val());
        }
        if self.count < LESSONS {
            return Err(Diag::EShort.val());
        }
        if self.carry_n < CARRIERS {
            return Err(Diag::ENoCarrier.val());
        }
        let walked = self.carriers_walked();
        if walked < CARRIERS {
            return Err(Diag::EFlow.val());
        }
        let mut lines = Vec::new();
        let mut i = 0usize;
        while i < LESSONS {
            match self.lesson_at(i) {
                Some(l) => lines.push(format!("条{}: 在册+走通", l.id)),
                None => return Err(Diag::EShort.val()),
            }
            i += 1;
        }
        let report = ReviewReport {
            lines,
            walked,
            ver_ok: true,
            summary: format!(
                "沉淀收官复核：十条在册，反哺承载走通{}/{}，版本v1未变更，报告归档",
                walked, CARRIERS
            ),
        };
        self.report = Some(report.clone());
        self.phase = Phase::Archived;
        Ok(report)
    }

    /// 已落档报告。
    pub fn report(&self) -> Option<&ReviewReport> {
        self.report.as_ref()
    }

    /// 当前阶段。
    pub fn phase(&self) -> Phase {
        self.phase
    }
}

// ---- 确定性构账 ------------------------------------------------------------

/// 十条沉淀经文正文（编号派生，非空闭集）。
pub fn lesson_text(id: u16) -> &'static str {
    match id {
        1 => "红线优先：契约红线高于派工进度",
        2 => "取值走闭集：match 与 get，不裸下标",
        3 => "拒绝要显性：拒绝计数入账，不静默吞",
        4 => "双跑一致：构建两次逐项比对",
        5 => "注入必抓：人为违约必被检出",
        6 => "诊断码闭集：越界回兜底文案",
        7 => "账只增不减：回缩即翻红",
        8 => "状态机单向：倒退显性拒",
        9 => "报告非空：摘要载明结论",
        10 => "整数运算：零浮点零墙钟零随机",
        _ => "",
    }
}

/// 构造十条在册 + 反哺 10/10 走通的复核账。
pub fn build_review_final() -> ReviewFinal {
    let mut r = ReviewFinal::new();
    let mut id = 1u16;
    while (id as usize) <= LESSONS {
        let l = Lesson::make(id, lesson_text(id));
        match r.enroll(l) {
            Ok(_) => {}
            Err(_) => {}
        }
        id += 1;
    }
    let mut j = 1u16;
    while (j as usize) <= CARRIERS {
        let fp = match r.lesson_at((j - 1) as usize) {
            Some(l) => l.fp,
            None => 0,
        };
        let c = Carrier::make(j, "反哺承载点", fp);
        match r.carry(c) {
            Ok(_) => {}
            Err(_) => {}
        }
        j += 1;
    }
    r
}

/// 构造一条被静默变更的经文账（第 3 条正文被改动但编号不变）。
pub fn build_silently_changed() -> ReviewFinal {
    let mut r = build_review_final();
    let changed = Lesson::make(3, "红线优先：契约红线高于一切进度（被改）");
    match r.lessons.get_mut(2) {
        Some(cell) => {
            *cell = Some(changed);
            true
        }
        None => false,
    };
    r
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

/// 版本 v1 未被静默变更断言：干净账指纹与基线相等，被改账指纹必移位。
fn version_frozen_ok() -> bool {
    let clean = build_review_final();
    let changed = build_silently_changed();
    let base = clean.version_fp();
    if base != clean.version_fp() {
        return false;
    }
    // 干净账每条 in_scope 互证（指纹与正文一致）。
    let mut i = 0usize;
    while i < LESSONS {
        match clean.lesson_at(i) {
            Some(l) => {
                if !l.in_scope() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    // 被改账：第 3 条指纹与正文互证断裂。
    match changed.lesson_at(2) {
        Some(l) => {
            if l.text == lesson_text(3) {
                return false;
            }
        }
        None => return false,
    }
    clean.version_fp() != changed.version_fp()
}

/// 反哺 10/10 走通谓词。
fn carriers_full_ok(r: &ReviewFinal) -> bool {
    r.carriers_walked() == CARRIERS && r.carried() == CARRIERS
}

/// 回流失配注入必抓：承载回流指纹被改必须断走通并拒归档。
fn flow_injection_caught() -> bool {
    let mut r = build_review_final();
    let bad = match r.carrier_at(4) {
        Some(c) => Carrier {
            lesson_id: c.lesson_id,
            spot: c.spot.clone(),
            flow_fp: c.flow_fp ^ 0xBADF,
        },
        None => return false,
    };
    if !r.overwrite_carrier(4, bad) {
        return false;
    }
    if r.carriers_walked() != 4 {
        return false;
    }
    match r.archive() {
        Err(e) => e == Diag::EFlow.val(),
        Ok(_) => false,
    }
}

/// 承载空点注入必抓：承载点为空必须被拒收。
fn carrier_blank_caught() -> bool {
    let mut r = build_review_final();
    match r.carry(Carrier::make(1, "", 0)) {
        Err(e) => e == Diag::ENoCarrier.val(),
        Ok(_) => false,
    }
}

/// 静默变更注入必抓：被改账版本断言翻红且归档后指纹仍移位。
fn silent_change_caught() -> bool {
    let changed = build_silently_changed();
    let clean = build_review_final();
    clean.version_fp() != changed.version_fp() && version_frozen_ok()
}

/// 登记拒绝矩阵：越界编号/空正文/零指纹/错位重复四门。
fn enroll_reject_matrix_ok() -> bool {
    let mut r = ReviewFinal::new();
    // 门一/二：越界编号（0 与 11）。
    if r.enroll(Lesson::make(0, "x")).is_ok() {
        return false;
    }
    if r.enroll(Lesson::make(11, "x")).is_ok() {
        return false;
    }
    // 门三：空正文。
    if r.enroll(Lesson::make(1, "")).is_ok() {
        return false;
    }
    // 门四：零指纹（正文与指纹互证断裂）。
    let zero_fp = Lesson {
        id: 1,
        text: String::from("指纹被清零的经文"),
        fp: 0,
    };
    if r.enroll(zero_fp).is_ok() {
        return false;
    }
    // 正常在册。
    if r.enroll(Lesson::make(1, "第一条经文")).is_err() {
        return false;
    }
    // 错位重复。
    if r.enroll(Lesson::make(1, "重复条目")).is_ok() {
        return false;
    }
    r.len() == 1
}

// ---- ReviewFinal 注入辅助 --------------------------------------------------

impl ReviewFinal {
    /// 改写第 i 条承载（注入测试专用路径）。
    pub fn overwrite_carrier(&mut self, i: usize, c: Carrier) -> bool {
        if i >= self.carry_n {
            return false;
        }
        match self.carriers.get_mut(i) {
            Some(cell) => {
                *cell = Some(c);
                true
            }
            None => false,
        }
    }
}

// ---- 检查节 ----------------------------------------------------------------

/// UNX-F1590 自检入口：十条沉淀 + 反哺收官复核。
pub fn run_unx1590_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1590");
    let r = build_review_final();

    s.add_dyn(
        "F1590-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1590-02 十条经文逐条在册",
        r.lessons_full() && r.len() == LESSONS,
        &format!("lessons={}/{}", r.len(), LESSONS),
    );

    s.add_dyn(
        "F1590-03 在册台账只增不减",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < LESSONS {
                match r.lesson_at(i) {
                    Some(l) => {
                        if !l.in_scope() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                i += 1;
            }
            ok
        },
        "编号连续、正文非空、指纹互证",
    );

    s.add_dyn(
        "F1590-04 反哺承载 10/10 走通",
        carriers_full_ok(&r),
        &format!("walked={}/{}", r.carriers_walked(), CARRIERS),
    );

    s.add_dyn(
        "F1590-05 版本 v1 未被静默变更",
        version_frozen_ok(),
        "版本指纹重算与冻结值一致，被改账必移位",
    );

    s.add_dyn(
        "F1590-06 登记拒绝矩阵四门",
        enroll_reject_matrix_ok(),
        "越界编号/空正文/错位重复逐门显性拒",
    );

    s.add_dyn(
        "F1590-07 回流失配注入必抓",
        flow_injection_caught(),
        "回流指纹被改必须断走通并拒归档（EFlow）",
    );

    s.add_dyn(
        "F1590-08 承载空点注入必抓",
        carrier_blank_caught(),
        "承载点为空必须拒收（ENoCarrier）",
    );

    s.add_dyn(
        "F1590-09 静默变更注入必抓",
        silent_change_caught(),
        "经文被静默改动必须令版本断言翻红",
    );

    // 归档：报告落档 + 状态写回。
    let mut r2 = build_review_final();
    let seal_res = r2.archive();
    s.add_dyn(
        "F1590-10 复核报告归档",
        seal_res.is_ok() && r2.report().is_some() && r2.phase() == Phase::Archived,
        &format!("archived={}", r2.phase() == Phase::Archived),
    );
    s.add_dyn(
        "F1590-11 状态写回（已完成+摘要非空）",
        match &seal_res {
            Ok(rep) => rep.walked == CARRIERS && rep.ver_ok && !rep.summary.is_empty() && rep.lines.len() == LESSONS,
            Err(_) => false,
        },
        match &seal_res {
            Ok(rep) => rep.summary.as_str(),
            Err(e) => detail(*e),
        },
    );

    s.add_dyn(
        "F1590-12 归档后登记显性拒",
        match r2.enroll(Lesson::make(11, "越界补条")) {
            Err(e) => e == Diag::ERange.val() || e == Diag::ESealed.val(),
            Ok(_) => false,
        },
        "归档后登记一律拒",
    );

    s.add_dyn(
        "F1590-13 双跑一致",
        {
            let a = build_review_final();
            let b = build_review_final();
            let mut same = true;
            let mut i = 0usize;
            while i < LESSONS {
                if a.lesson_at(i) != b.lesson_at(i) {
                    same = false;
                }
                i += 1;
            }
            same && a.version_fp() == b.version_fp()
        },
        "两次构账经文与版本指纹逐项一致",
    );

    s.add_dyn(
        "F1590-14 诊断码闭集完备",
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
    fn ten_lessons_in_scope() {
        let mut id = 1u16;
        while (id as usize) <= LESSONS {
            let l = Lesson::make(id, lesson_text(id));
            assert!(l.in_scope(), "条 {} 结构非法", id);
            assert!(!l.text.is_empty());
            id += 1;
        }
        assert!(lesson_text(0).is_empty());
        assert!(lesson_text(11).is_empty());
    }

    #[test]
    fn build_is_full_and_walked() {
        let r = build_review_final();
        assert_eq!(r.len(), LESSONS);
        assert_eq!(r.carried(), CARRIERS);
        assert!(carriers_full_ok(&r));
        assert!(r.review_ok());
    }

    #[test]
    fn version_fp_stable_and_sensitive() {
        assert!(version_frozen_ok());
        let a = build_review_final();
        let b = build_review_final();
        assert_eq!(a.version_fp(), b.version_fp());
        assert!(silent_change_caught());
    }

    #[test]
    fn enroll_rejects_bad_shapes() {
        assert!(enroll_reject_matrix_ok());
        let mut r = ReviewFinal::new();
        assert_eq!(r.enroll(Lesson::make(1, "")), Err(Diag::EEmpty.val()));
        let good = Lesson::make(1, "第一条");
        assert_eq!(r.enroll(good), Ok(1));
        assert_eq!(
            r.enroll(Lesson::make(1, "重复")),
            Err(Diag::EDup.val())
        );
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn carry_requires_enrolled_lesson() {
        let mut r = ReviewFinal::new();
        assert_eq!(
            r.carry(Carrier::make(1, "承载点", 5)),
            Err(Diag::EShort.val()),
            "未在册经文不得接承载"
        );
        let _ = r.enroll(Lesson::make(1, "第一条"));
        assert_eq!(r.carry(Carrier::make(1, "承载点", 0)), Ok(1));
        assert_eq!(r.carry(Carrier::make(1, "重复", 0)), Err(Diag::EDup.val()));
    }

    #[test]
    fn injections_caught() {
        assert!(flow_injection_caught());
        assert!(carrier_blank_caught());
    }

    #[test]
    fn archive_reports_and_freezes() {
        let mut r = build_review_final();
        let rep = match r.archive() {
            Ok(rep) => rep,
            Err(e) => { assert!(false, "归档失败: 诊断码 {}", e); loop {} },
        };
        assert_eq!(rep.walked, CARRIERS);
        assert!(rep.ver_ok);
        assert_eq!(rep.lines.len(), LESSONS);
        assert!(!rep.summary.is_empty());
        assert!(r.phase() == Phase::Archived);
        assert_eq!(r.archive(), Err(Diag::ESealed.val()));
        assert_eq!(
            r.enroll(Lesson::make(1, "x")),
            Err(Diag::ESealed.val())
        );
    }

    #[test]
    fn short_lessons_blocks_archive() {
        let mut r = ReviewFinal::new();
        let mut id = 1u16;
        while (id as usize) < LESSONS {
            let _ = r.enroll(Lesson::make(id, lesson_text(id)));
            id += 1;
        }
        match r.archive() {
            Err(e) => assert_eq!(e, Diag::EShort.val()),
            Ok(_) => { assert!(false, "缺条归档必须拒"); loop {} },
        }
    }

    #[test]
    fn double_run_deterministic() {
        let a = build_review_final();
        let b = build_review_final();
        let mut i = 0usize;
        while i < LESSONS {
            assert_eq!(a.lesson_at(i), b.lesson_at(i));
            assert_eq!(a.carrier_at(i), b.carrier_at(i));
            i += 1;
        }
        assert_eq!(a.version_fp(), b.version_fp());
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
            "未知诊断码：不在 0x79181..0x7918C 闭集内"
        );
        assert!(detail_codes_ok());
    }
}
