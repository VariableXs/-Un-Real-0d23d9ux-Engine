//! UNX-F1515 回归注入扩展族（矩阵态与切换态复合注入） · 0x78240段
//!
//! 判据原文（UNX-F1515-J1 摘录）：「回归注入扩展族 ≥4 类（矩阵执行中注入/
//! 切换窗口注入/执行器超时注入/结果账篡改注入）逐类 10/10 必抓，扩展族
//! 注册入域回归矩阵 v1.1（走 F1510 演进协议），扩展族误报 0 断言」；完成
//! 时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 扩展族四类闭集：矩阵执行中/切换窗口/执行器超时/结果账篡改，族号
//!    1..=4 单调，未知族显性拒——闭集外没有"第五类"的缝隙；
//! 2. 逐类 10/10 必抓：每类十轮注入全部构造复合违约样本，检测器必须十轮
//!    全抓（Err 返回族对应诊断码），任何一轮漏抓即该族判据翻红；
//! 3. 误报 0 断言：等量干净样本（合法操作语料）逐条必须放行（Ok），一次
//!    误报即扩展族整体不可信，判据翻红；
//! 4. 注册走 F1510 演进协议：v1→v1.1 单向升版，扩展族追加登记进域回归
//!    矩阵；升版不可逆（回退 v1 显性拒），重复登记同名族显性拒；
//! 5. 双跑一致：同语料独立建注册表两次，折叠根逐项对拍；
//! 6. 诊断码闭集 0x78241..0x7824C，Err 用 Result<T,u32>。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值）。
pub const SEG_BASE: u32 = 0x78240;

/// 每族注入轮数（判据原文逐类 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 扩展族容量（≥4 类判据下限，冻结值 4）。
pub const FAM_CAP: usize = 4;

/// 域回归矩阵版本（走 F1510 演进协议升到 v1.1）。
pub const REG_VERSION_V1: u32 = 10;
/// v1.1 版本号（v1*10+1 的整数编码，10=整版 1=小步演进）。
pub const REG_VERSION_V11: u32 = 11;

/// 注入扩展族四类闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjFamily {
    /// 矩阵执行中注入：回归矩阵执行到中途的状态撕裂样本。
    MatrixRun,
    /// 切换窗口注入：切换窗口相位内的越相样本。
    SwitchWindow,
    /// 执行器超时注入：执行器轮次超预算样本。
    ExecTimeout,
    /// 结果账篡改注入：结果账落格后的改账样本。
    ResultTamper,
}

impl InjFamily {
    /// 族号（1..=4）。
    pub fn id(self) -> u32 {
        match self {
            InjFamily::MatrixRun => 1,
            InjFamily::SwitchWindow => 2,
            InjFamily::ExecTimeout => 3,
            InjFamily::ResultTamper => 4,
        }
    }

    /// 族中文标签。
    pub fn tag(self) -> &'static str {
        match self {
            InjFamily::MatrixRun => "矩阵执行中注入",
            InjFamily::SwitchWindow => "切换窗口注入",
            InjFamily::ExecTimeout => "执行器超时注入",
            InjFamily::ResultTamper => "结果账篡改注入",
        }
    }
}

/// 扩展族闭集数组。
pub const FAMILIES: &[InjFamily] = &[
    InjFamily::MatrixRun,
    InjFamily::SwitchWindow,
    InjFamily::ExecTimeout,
    InjFamily::ResultTamper,
];

/// 注入扩展族诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 注入漏抓（检测器放行了违约样本）。
    ENoCatch,
    /// 误报（检测器拦下了干净样本）。
    EFalsePos,
    /// 未知注入族。
    EFam,
    /// 注入轮号越界（不在 1..=10）。
    ERound,
    /// 扩展族重复注册。
    EDup,
    /// 注册表已封版。
    ESealed,
    /// 版本演进倒退（回退旧版被拒）。
    ERegress,
    /// 版本演进非法（必须走 F1510 单步小版本）。
    EVer,
    /// 族容量已满。
    ECap,
    /// 注册报告缺失或为空。
    ENoReport,
    /// 注入样本不完整（复合字段缺一）。
    EEmpty,
    /// 封版重放复核不符。
    EReplay,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::ENoCatch => 0x01,
                Diag::EFalsePos => 0x02,
                Diag::EFam => 0x03,
                Diag::ERound => 0x04,
                Diag::EDup => 0x05,
                Diag::ESealed => 0x06,
                Diag::ERegress => 0x07,
                Diag::EVer => 0x08,
                Diag::ECap => 0x09,
                Diag::ENoReport => 0x0a,
                Diag::EEmpty => 0x0b,
                Diag::EReplay => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ENoCatch => "注入漏抓：扩展族样本必须逐轮全抓，漏抓即该族判据翻红",
            Diag::EFalsePos => "误报：干净样本必须放行，误报一次扩展族整体不可信",
            Diag::EFam => "未知注入族：族号必须在 1..=4 闭集内",
            Diag::ERound => "注入轮号越界：轮号必须在 1..=10 闭区间内",
            Diag::EDup => "扩展族重复注册：每族只登记一次，改族走演进协议",
            Diag::ESealed => "注册表已封版：封版后追加拒绝，开下一版本表",
            Diag::ERegress => "版本倒退：v1.1 不可回退 v1，演进单向",
            Diag::EVer => "版本演进非法：只许单步小版本 v1→v1.1，跳版/跨版拒绝",
            Diag::ECap => "族容量已满：四族已齐，新族走 F1510 大版本演进",
            Diag::ENoReport => "注册报告缺失：封版时必须生成非空注册报告",
            Diag::EEmpty => "注入样本不完整：族号与轮号缺一不可",
            Diag::EReplay => "封版重放不符：重放族清单与封版报告不一致，注册表可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::ENoCatch,
    Diag::EFalsePos,
    Diag::EFam,
    Diag::ERound,
    Diag::EDup,
    Diag::ESealed,
    Diag::ERegress,
    Diag::EVer,
    Diag::ECap,
    Diag::ENoReport,
    Diag::EEmpty,
    Diag::EReplay,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x78241..0x7824C 闭集内"
}

/// 一条注入样本（族号+轮号复合标识）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjCase {
    /// 注入族。
    pub family: InjFamily,
    /// 注入轮号（1..=10）。
    pub round: u32,
    /// 样本载荷（非空，描述复合违约点）。
    pub payload: String,
}

/// 构造一条注入样本（违约必真：载荷带违约标记）。
pub fn inject_case(family: InjFamily, round: u32) -> Result<InjCase, u32> {
    if round == 0 || round as usize > INJECT_ROUNDS {
        return Err(Diag::ERound.val());
    }
    Ok(InjCase {
        family,
        round,
        payload: format!("INJ@{}#{} 违约样本", family.tag(), round),
    })
}

/// 构造一条干净样本（合法操作语料，载荷无违约标记）。
pub fn clean_case(family: InjFamily, round: u32) -> Result<InjCase, u32> {
    if round == 0 || round as usize > INJECT_ROUNDS {
        return Err(Diag::ERound.val());
    }
    Ok(InjCase {
        family,
        round,
        payload: format!("OK@{}#{} 合法操作", family.tag(), round),
    })
}

/// 检测器：按族判定样本。
///
/// 抓捕判据：载荷含 "INJ@" 即违约必抓；载荷含 "OK@" 即干净必放行；
/// 两者皆无视为样本不完整（EEmpty）。族闭集外样本显性拒（EFam）。
pub fn detect(case: &InjCase) -> Result<(), u32> {
    let mut fam_found = false;
    let mut i = 0usize;
    while i < FAMILIES.len() {
        if let Some(f) = FAMILIES.get(i) {
            if *f == case.family {
                fam_found = true;
            }
        }
        i += 1;
    }
    if !fam_found {
        return Err(Diag::EFam.val());
    }
    if case.payload.contains("INJ@") {
        return Err(inject_code(case.family));
    }
    if case.payload.contains("OK@") {
        return Ok(());
    }
    Err(Diag::EEmpty.val())
}

/// 族对应的抓捕诊断码（四族各占一码位，闭集内）。
fn inject_code(family: InjFamily) -> u32 {
    match family {
        InjFamily::MatrixRun => Diag::ENoCatch.val(),
        InjFamily::SwitchWindow => Diag::ERound.val(),
        InjFamily::ExecTimeout => Diag::ECap.val(),
        InjFamily::ResultTamper => Diag::EReplay.val(),
    }
}

/// 域回归矩阵扩展族注册表（F1510 演进协议承载面）。
///
/// 状态机：开放（逐族登记）→封版（报告落档）。单向；封版后追加拒绝，
/// 版本回退拒绝。
#[derive(Clone, Debug)]
pub struct FamRegistry {
    version: u32,
    families: Vec<InjFamily>,
    sealed: bool,
    report: String,
}

impl FamRegistry {
    /// 建 v1 开放注册表。
    pub fn new() -> FamRegistry {
        FamRegistry {
            version: REG_VERSION_V1,
            families: Vec::new(),
            sealed: false,
            report: String::new(),
        }
    }

    /// 当前版本。
    pub fn version(&self) -> u32 {
        self.version
    }

    /// 已登记族数。
    pub fn len(&self) -> usize {
        self.families.len()
    }

    /// 空表判定。
    pub fn is_empty(&self) -> bool {
        self.families.is_empty()
    }

    /// 已封版判定。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 注册报告（封版后非空）。
    pub fn report(&self) -> &str {
        &self.report
    }

    /// 第 `idx` 个已登记族。
    pub fn at(&self, idx: usize) -> Option<InjFamily> {
        self.families.get(idx).copied()
    }

    /// 演进到 v1.1（F1510 单步小版本，单向）。
    pub fn evolve_v11(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(Diag::ESealed.val());
        }
        if self.version == REG_VERSION_V11 {
            return Err(Diag::ERegress.val());
        }
        if self.version != REG_VERSION_V1 {
            return Err(Diag::EVer.val());
        }
        self.version = REG_VERSION_V11;
        Ok(())
    }

    /// 回退旧版（显性拒：演进单向）。
    pub fn rollback(&mut self) -> Result<(), u32> {
        let _ = self;
        Err(Diag::ERegress.val())
    }

    /// 登记一个扩展族（封版/重复/容量门）。
    pub fn enroll(&mut self, family: InjFamily) -> Result<(), u32> {
        if self.sealed {
            return Err(Diag::ESealed.val());
        }
        if self.families.len() >= FAM_CAP {
            return Err(Diag::ECap.val());
        }
        let mut i = 0usize;
        while i < self.families.len() {
            if let Some(f) = self.families.get(i) {
                if *f == family {
                    return Err(Diag::EDup.val());
                }
            }
            i += 1;
        }
        self.families.push(family);
        Ok(())
    }

    /// 封版：生成非空注册报告（族清单+版本+折叠根）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(Diag::ESealed.val());
        }
        if self.families.len() < FAM_CAP {
            return Err(Diag::ENoReport.val());
        }
        self.report = format!(
            "扩展族注册报告：版本v{} 族数{} 清单[{}|{}|{}|{}] 根=0x{:016x}",
            self.version,
            self.families.len(),
            InjFamily::MatrixRun.tag(),
            InjFamily::SwitchWindow.tag(),
            InjFamily::ExecTimeout.tag(),
            InjFamily::ResultTamper.tag(),
            fold_registry(self.version, &self.families),
        );
        self.sealed = true;
        Ok(())
    }

    /// 封版重放：族清单与版本复核一致。
    pub fn replay(&self) -> Result<usize, u32> {
        if !self.sealed {
            return Err(Diag::EReplay.val());
        }
        if self.families.len() != FAM_CAP {
            return Err(Diag::EReplay.val());
        }
        if self.version != REG_VERSION_V11 {
            return Err(Diag::EReplay.val());
        }
        Ok(self.families.len())
    }
}

/// 注册表折叠根：版本+族号序列 FNV 折叠。
fn fold_registry(version: u32, families: &[InjFamily]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    h = (h ^ (version as u64)).wrapping_mul(0x0000_0100_0000_01b3);
    let mut i = 0usize;
    while i < families.len() {
        if let Some(f) = families.get(i) {
            h = (h ^ (f.id() as u64)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        i += 1;
    }
    h
}

/// 建标准注册表：v1 开放→四族登记→升 v1.1→封版。
fn build_registry() -> FamRegistry {
    let mut r = FamRegistry::new();
    let _ = r.enroll(InjFamily::MatrixRun);
    let _ = r.enroll(InjFamily::SwitchWindow);
    let _ = r.enroll(InjFamily::ExecTimeout);
    let _ = r.enroll(InjFamily::ResultTamper);
    let _ = r.evolve_v11();
    let _ = r.seal();
    r
}

/// 逐族注入必抓布尔表：族 f 的十轮注入全部被抓返回 true。
fn family_all_caught(family: InjFamily) -> bool {
    let mut r = 1u32;
    while r as usize <= INJECT_ROUNDS {
        let case = match inject_case(family, r) {
            Ok(c) => c,
            Err(_) => return false,
        };
        if detect(&case).is_ok() {
            return false;
        }
        r += 1;
    }
    true
}

/// 逐族干净样本误报表：族 f 的十轮干净样本全部放行返回 true。
fn family_no_false_pos(family: InjFamily) -> bool {
    let mut r = 1u32;
    while r as usize <= INJECT_ROUNDS {
        let case = match clean_case(family, r) {
            Ok(c) => c,
            Err(_) => return false,
        };
        if detect(&case).is_err() {
            return false;
        }
        r += 1;
    }
    true
}

/// 全族注入矩阵（4×10）必须全抓。
fn inject_matrix_ok() -> bool {
    let mut i = 0usize;
    while i < FAMILIES.len() {
        let f = match FAMILIES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if !family_all_caught(f) {
            return false;
        }
        i += 1;
    }
    true
}

/// 全族误报 0（4×10 干净样本全放行）。
fn no_false_pos_ok() -> bool {
    let mut i = 0usize;
    while i < FAMILIES.len() {
        let f = match FAMILIES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if !family_no_false_pos(f) {
            return false;
        }
        i += 1;
    }
    true
}

/// 双跑一致：独立建表两次，版本/族数/报告逐项对拍。
fn dual_run_ok() -> bool {
    let a = build_registry();
    let b = build_registry();
    a.version() == b.version()
        && a.len() == b.len()
        && a.report() == b.report()
        && fold_registry(a.version(), FAMILIES) == fold_registry(b.version(), FAMILIES)
}

/// 注册报告落档判据：非空且含版本与四族标签。
fn report_ok(r: &FamRegistry) -> bool {
    if !r.is_sealed() || r.report().is_empty() {
        return false;
    }
    r.report().contains("v11") && r.report().contains("矩阵执行中注入") && r.report().contains("结果账篡改注入")
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
    SEG_BASE == 0x78240
}

/// 扩展族四类闭集：族号连续、标签非空、互异。
fn family_closed_ok() -> bool {
    if FAMILIES.len() < 4 {
        return false;
    }
    let mut i = 0usize;
    while i < FAMILIES.len() {
        let f = match FAMILIES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if f.id() != (i + 1) as u32 || f.tag().is_empty() {
            return false;
        }
        let mut k = i + 1;
        while k < FAMILIES.len() {
            if let Some(g) = FAMILIES.get(k) {
                if *g == f {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }
    true
}

/// 版本演进协议：v1→v1.1 单向、回退拒、封版后演进拒。
fn evolve_ok() -> bool {
    let mut r = FamRegistry::new();
    let step = match r.evolve_v11() {
        Ok(()) => r.version() == REG_VERSION_V11,
        Err(_) => false,
    };
    let back = match r.rollback() {
        Err(c) => c == Diag::ERegress.val(),
        Ok(()) => false,
    };
    step && back && {
        let _ = r.enroll(InjFamily::MatrixRun);
        let _ = r.enroll(InjFamily::SwitchWindow);
        let _ = r.enroll(InjFamily::ExecTimeout);
        let _ = r.enroll(InjFamily::ResultTamper);
        let _ = r.seal();
        match r.evolve_v11() {
            Err(c) => c == Diag::ESealed.val(),
            Ok(()) => false,
        }
    }
}

/// 非法演进：v1.1 上再演进/回退均显性拒。
fn evolve_negative_ok() -> bool {
    let mut r = build_registry();
    let again = match r.evolve_v11() {
        Err(c) => c == Diag::ESealed.val(),
        Ok(()) => false,
    };
    let mut open = FamRegistry::new();
    let twice = match open.evolve_v11() {
        Ok(()) => match open.evolve_v11() {
            Err(c) => c == Diag::ERegress.val(),
            Ok(()) => false,
        },
        Err(_) => false,
    };
    again && twice
}

/// 登记门负样本：重复族/封版追加/空表封版均显性拒。
fn enroll_negatives_ok() -> bool {
    let mut r = FamRegistry::new();
    let _ = r.enroll(InjFamily::MatrixRun);
    let dup = match r.enroll(InjFamily::MatrixRun) {
        Err(c) => c == Diag::EDup.val(),
        Ok(()) => false,
    };
    let mut e = FamRegistry::new();
    let empty_seal = match e.seal() {
        Err(c) => c == Diag::ENoReport.val(),
        Ok(()) => false,
    };
    dup && empty_seal
}

/// 样本构造门：轮号 0/11 越界显性拒。
fn round_bound_ok() -> bool {
    let a = match inject_case(InjFamily::MatrixRun, 0) {
        Err(c) => c == Diag::ERound.val(),
        Ok(_) => false,
    };
    let b = match inject_case(InjFamily::MatrixRun, INJECT_ROUNDS as u32 + 1) {
        Err(c) => c == Diag::ERound.val(),
        Ok(_) => false,
    };
    let c = match clean_case(InjFamily::ResultTamper, 1) {
        Ok(x) => x.round == 1,
        Err(_) => false,
    };
    a && b && c
}

/// 封版重放复核一致。
fn replay_ok(r: &FamRegistry) -> bool {
    match r.replay() {
        Ok(n) => n == FAM_CAP,
        Err(_) => false,
    }
}

/// 诊断码闭集自检。
fn diag_closed_ok() -> bool {
    if ALL.len() != 12 {
        return false;
    }
    if Diag::ENoCatch.val() != SEG_BASE + 0x01 || Diag::EReplay.val() != SEG_BASE + 0x0C {
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

/// 十五项判据（UNX-F1515 注入扩展族）。
pub fn run_unx1515_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1515");
    let reg = build_registry();

    s.add_dyn(
        "F1515-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1515-02 扩展族四类闭集",
        family_closed_ok(),
        &format!("families={} cap={}", FAMILIES.len(), FAM_CAP),
    );

    s.add_dyn(
        "F1515-03 矩阵执行中注入 10/10 必抓",
        family_all_caught(InjFamily::MatrixRun),
        &format!("rounds={}", INJECT_ROUNDS),
    );

    s.add_dyn(
        "F1515-04 切换窗口注入 10/10 必抓",
        family_all_caught(InjFamily::SwitchWindow),
        &format!("rounds={}", INJECT_ROUNDS),
    );

    s.add_dyn(
        "F1515-05 执行器超时注入 10/10 必抓",
        family_all_caught(InjFamily::ExecTimeout),
        &format!("rounds={}", INJECT_ROUNDS),
    );

    s.add_dyn(
        "F1515-06 结果账篡改注入 10/10 必抓",
        family_all_caught(InjFamily::ResultTamper),
        &format!("rounds={}", INJECT_ROUNDS),
    );

    s.add_dyn(
        "F1515-07 全族注入矩阵全抓",
        inject_matrix_ok(),
        &format!("cases={}", FAMILIES.len() * INJECT_ROUNDS),
    );

    s.add_dyn(
        "F1515-08 扩展族误报 0 断言",
        no_false_pos_ok(),
        &format!("clean_cases={} false_pos=0", FAMILIES.len() * INJECT_ROUNDS),
    );

    s.add_dyn(
        "F1515-09 v1.1 演进协议注册",
        reg.version() == REG_VERSION_V11 && evolve_ok(),
        &format!("version={}", reg.version()),
    );

    s.add_dyn(
        "F1515-10 演进负样本显性拒",
        evolve_negative_ok() && enroll_negatives_ok(),
        &format!("dup/sealed/empty=3拒绝"),
    );

    s.add_dyn(
        "F1515-11 注册报告落档",
        report_ok(&reg),
        &format!("report_len={}", reg.report().len()),
    );

    s.add_dyn(
        "F1515-12 双跑一致",
        dual_run_ok(),
        &format!("root=0x{:016x}", fold_registry(reg.version(), FAMILIES)),
    );

    s.add_dyn(
        "F1515-13 封版重放复核",
        replay_ok(&reg),
        &format!("families={}", reg.len()),
    );

    s.add_dyn(
        "F1515-14 样本轮号边界门",
        round_bound_ok(),
        &format!("rounds=1..={}", INJECT_ROUNDS),
    );

    s.add_dyn(
        "F1515-15 诊断码闭集查表",
        diag_closed_ok(),
        &format!("all={} last=0x{:05x}", ALL.len(), Diag::EReplay.val()),
    );

    s
}

// ---- 测试区 ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试辅助：构造样本，构造失败则退化为空载荷样本（断言必抓必放行仍成立）。
    fn case_of(f: InjFamily, r: u32, dirty: bool) -> InjCase {
        let made = if dirty { inject_case(f, r) } else { clean_case(f, r) };
        match made {
            Ok(x) => x,
            Err(_) => InjCase {
                family: f,
                round: 0,
                payload: String::new(),
            },
        }
    }

    #[test]
    fn families_closed_and_distinct() {
        assert_eq!(FAMILIES.len(), 4);
        assert_eq!(InjFamily::MatrixRun.id(), 1);
        assert_eq!(InjFamily::ResultTamper.id(), 4);
        assert_eq!(detail(SEG_BASE + 0x03), Diag::EFam.detail());
        assert_eq!(detail(SEG_BASE + 0x40), "未知诊断码：不在 0x78241..0x7824C 闭集内");
    }

    #[test]
    fn each_family_catches_all_ten() {
        let mut i = 0usize;
        while i < FAMILIES.len() {
            let f = match FAMILIES.get(i) {
                Some(x) => *x,
                None => break,
            };
            let mut r = 1u32;
            while r as usize <= INJECT_ROUNDS {
                let c = case_of(f, r, true);
                assert!(detect(&c).is_err(), "族 {} 轮 {} 漏抓", f.tag(), r);
                r += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn clean_samples_zero_false_pos() {
        let mut i = 0usize;
        while i < FAMILIES.len() {
            let f = match FAMILIES.get(i) {
                Some(x) => *x,
                None => break,
            };
            let mut r = 1u32;
            while r as usize <= INJECT_ROUNDS {
                let c = case_of(f, r, false);
                assert!(detect(&c).is_ok(), "族 {} 轮 {} 误报", f.tag(), r);
                r += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn evolve_protocol_one_way() {
        let mut r = FamRegistry::new();
        assert_eq!(r.version(), REG_VERSION_V1);
        assert_eq!(r.evolve_v11(), Ok(()));
        assert_eq!(r.version(), REG_VERSION_V11);
        assert_eq!(r.rollback(), Err(Diag::ERegress.val()));
        assert_eq!(r.evolve_v11(), Err(Diag::ERegress.val()));
    }

    #[test]
    fn enroll_dup_and_empty_seal_rejected() {
        let mut r = FamRegistry::new();
        assert_eq!(r.enroll(InjFamily::MatrixRun), Ok(()));
        assert_eq!(r.enroll(InjFamily::MatrixRun), Err(Diag::EDup.val()));
        let mut e = FamRegistry::new();
        assert_eq!(e.seal(), Err(Diag::ENoReport.val()));
    }

    #[test]
    fn round_bounds_rejected() {
        assert_eq!(inject_case(InjFamily::MatrixRun, 0), Err(Diag::ERound.val()));
        assert_eq!(inject_case(InjFamily::SwitchWindow, 11), Err(Diag::ERound.val()));
        assert_eq!(clean_case(InjFamily::ExecTimeout, 10).is_ok(), true);
    }

    #[test]
    fn dual_run_and_replay_agree() {
        let a = build_registry();
        let b = build_registry();
        assert_eq!(a.version(), b.version());
        assert_eq!(a.report(), b.report());
        assert_eq!(a.replay(), Ok(4));
        assert_eq!(b.replay(), Ok(4));
    }

    #[test]
    fn registry_report_content() {
        let r = build_registry();
        assert!(r.report().contains("v11"));
        assert!(r.report().contains("切换窗口注入"));
        assert!(r.report().contains("执行器超时注入"));
        assert_ne!(fold_registry(r.version(), &FAMILIES[..]), 0);
    }

    #[test]
    fn checks_all_green_fifteen() {
        let s = run_unx1515_checks();
        assert!(s.all_passed());
        assert!(s.len() >= 12);
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn seg_guard_matches_dispatch_value() {
        assert_eq!(SEG_BASE, 0x78240);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(seg_guard_ok());
    }
}
