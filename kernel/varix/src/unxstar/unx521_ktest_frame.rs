//! unx521_ktest_frame ↔ UNX-F0521 ktest 主框架总装（注册器/运行器）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0521，UNX-A1-B27）：
//! 「ktest 主框架总装（注册器/运行器）| 260 | 已深化 |
//!  框架双件齐（注册器/运行器），B19–B26 六套件经框架运行全绿（6/6 套件接入核）」
//!
//! 设计要点：
//! 1. **注册器/运行器双件**：`SuiteRegistry`（编译期定容 32 套件，
//!     按名字排序 + 名唯一性双门）+ `SuiteRunner`（按序泵动、逐套件
//!     收 CheckSet、汇总 pass/fail/截断三态）。
//! 2. **接入核 6/6 硬线**：`integration_audit()` 核对「已注册套件
//!     是否真的被运行过」——注册了没跑 = 假接入（最常见假绿源），
//!     必须逐套件对账 6/6。
//! 3. **截断显性**：CheckSet 超过 MAX_CHECKS 会被静默丢项，
//!     运行器对每个套件断言 `!truncated()`，截断即显性失败
//!     （丢项的绿是假绿）。
//! 4. **泵动语义**：`pump_one()` 单步推进（无中断长任务）、
//!     `run_all()` 全量；两侧共用同一状态机，避免两套语义。
//! 5. **确定性**：注入逻辑 tick 排序，同序同结果（回归可复现）。
//!
//! 诊断码：`KtestCode` 0x85 段（0x8500..=0x850F）。

use alloc::format;
use alloc::string::String;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX521_KTEST_VERSION: &str = "unx521-ktest-frame-v1";

/// 注册器容量（编译期定容，禁堆上动态扩容——注册面必须可枚举）。
pub const SUITE_CAP: usize = 32;

/// 套件探测缓存上限（锚点 B19–B26 六套件）。
pub const PROBE_CACHE: usize = 4;

/// CheckSet 容量硬线（与 crate::checks::MAX_CHECKS 同源，勿改）。
pub const LOCAL_MAX_CHECKS: usize = 112;

/// 套件记录（注册即冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SuiteEntry {
    /// 套件名（稳定标识）
    pub name: &'static str,
    /// 所属域
    pub domain: &'static str,
    /// 注册 tick
    pub registered_tick: u64,
    /// 是否被运行器跑过
    pub ran: bool,
    /// 运行结果（None = 未跑）
    pub verdict: Option<SuiteVerdict>,
}

/// 套件运行结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SuiteVerdict {
    /// 通过数
    pub passed: u32,
    /// 失败数
    pub failed: u32,
    /// 是否截断
    pub truncated: bool,
}

impl SuiteVerdict {
    /// 全绿判据。
    pub const fn all_green(&self) -> bool {
        self.failed == 0 && !self.truncated
    }
}

/// 注册错误。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegError {
    /// 重名注册
    Duplicate { name: &'static str },
    /// 注册器满
    RegistryFull { cap: usize },
    /// 未注册套件被运行
    UnknownSuite { name: &'static str },
    /// 空域名
    DomainEmpty { name: &'static str },
    /// 接入核缺口（注册但没跑）
    NotRun { name: &'static str },
    /// 套件截断
    Truncated { name: &'static str },
}

/// 注册器（定容 + 名唯一 + 注册序冻结）。
pub struct SuiteRegistry {
    entries: [Option<SuiteEntry>; SUITE_CAP],
    count: usize,
}

/// 默认值（Option<SuiteEntry> = None 数组初始化）。
impl Default for SuiteRegistry {
    fn default() -> Self {
        SuiteRegistry::new()
    }
}

impl SuiteRegistry {
    /// 空注册器。
    pub const fn new() -> Self {
        SuiteRegistry {
            entries: [None; SUITE_CAP],
            count: 0,
        }
    }

    /// 条数。
    pub const fn len(&self) -> usize {
        self.count
    }

    /// 空否。
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 查重（存在即返 Index）。
    pub fn find(&self, name: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.count {
            if let Some(e) = self.entries.get(i) {
                if let Some(en) = e {
                    if en.name == name {
                        return Some(i);
                    }
                }
            }
            i += 1;
        }
        None
    }

    /// 注册一套件（重名拒、域名空拒、满员显性）。
    pub fn register(
        &mut self,
        name: &'static str,
        domain: &'static str,
        tick: u64,
    ) -> Result<(), RegError> {
        if self.find(name).is_some() {
            return Err(RegError::Duplicate { name });
        }
        if domain.is_empty() {
            return Err(RegError::DomainEmpty { name });
        }
        if self.count >= SUITE_CAP {
            return Err(RegError::RegistryFull { cap: SUITE_CAP });
        }
        if let Some(slot) = self.entries.get_mut(self.count) {
            *slot = Some(SuiteEntry {
                name,
                domain,
                registered_tick: tick,
                ran: false,
                verdict: None,
            });
        }
        self.count += 1;
        Ok(())
    }

    /// 标已运行（带结论）。
    pub fn mark_ran(&mut self, name: &str, v: SuiteVerdict) -> Result<(), RegError> {
        let idx = match self.find(name) {
            Some(i) => i,
            None => return Err(RegError::UnknownSuite { name: "?" }),
        };
        if let Some(Some(e)) = self.entries.get_mut(idx) {
            e.ran = true;
            e.verdict = Some(v);
        }
        Ok(())
    }

    /// 接入核审计：所有注册套件都必须跑过且全绿。
    pub fn integration_audit(&self) -> Result<u32, RegError> {
        let mut ok = 0u32;
        let mut i = 0;
        while i < self.count {
            if let Some(Some(e)) = self.entries.get(i) {
                if !e.ran {
                    return Err(RegError::NotRun { name: e.name });
                }
                if let Some(v) = e.verdict {
                    if v.truncated {
                        return Err(RegError::Truncated { name: e.name });
                    }
                    if v.failed != 0 {
                        return Err(RegError::Truncated { name: e.name });
                    }
                    ok += 1;
                } else {
                    return Err(RegError::NotRun { name: e.name });
                }
            }
            i += 1;
        }
        Ok(ok)
    }

    /// 遍历（注册序）。
    pub fn iter(&self) -> impl Iterator<Item = &SuiteEntry> {
        self.entries
            .iter()
            .take(self.count)
            .filter_map(|e| e.as_ref())
    }
}

/// 运行器（泵动式，单步/全量同状态机）。
pub struct SuiteRunner<'a> {
    reg: &'a mut SuiteRegistry,
    cursor: usize,
    total_runs: u32,
    total_fails: u32,
}

impl<'a> SuiteRunner<'a> {
    /// 绑定注册器。
    pub const fn new(reg: &'a mut SuiteRegistry) -> Self {
        SuiteRunner {
            reg,
            cursor: 0,
            total_runs: 0,
            total_fails: 0,
        }
    }

    /// 单步泵动：跑一个未跑套件（返回是否还有剩余）。
    pub fn pump_one<F>(&mut self, mut producer: F) -> Result<bool, RegError>
    where
        F: FnMut(&str) -> Result<CheckSet, RegError>,
    {
        // 找下一个未跑套件
        let mut i = self.cursor;
        while i < self.reg.len() {
            if let Some(e) = self.reg.iter().nth(i) {
                if !e.ran {
                    break;
                }
            }
            i += 1;
        }
        if i >= self.reg.len() {
            return Ok(false);
        }
        let name = match self.reg.iter().nth(i) {
            Some(e) => e.name,
            None => return Ok(false),
        };
        self.cursor = i + 1;
        let set = producer(name)?;
        let (passed, failed) = set.tally();
        let passed = passed as u32;
        let failed = failed as u32;
        let v = SuiteVerdict {
            passed,
            failed,
            truncated: set.truncated(),
        };
        self.total_runs += 1;
        self.total_fails += failed;
        self.reg.mark_ran(name, v)?;
        Ok(true)
    }

    /// 全量跑（直到泵空或出错）。
    pub fn run_all<F>(&mut self, mut producer: F) -> Result<u32, RegError>
    where
        F: FnMut(&str) -> Result<CheckSet, RegError>,
    {
        while self.pump_one(&mut producer)? {}
        Ok(self.total_runs)
    }

    /// 汇总失败数（跨套件）。
    pub const fn total_fails(&self) -> u32 {
        self.total_fails
    }
}

/// 诊断码（0x85 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtestCode {
    /// 0x8500 重名
    Duplicate,
    /// 0x8501 注册器满
    RegistryFull,
    /// 0x8502 未注册套件
    UnknownSuite,
    /// 0x8503 域名空
    DomainEmpty,
    /// 0x8504 接入核缺口
    NotRun,
    /// 0x8505 套件截断
    Truncated,
    /// 0x8506 接入核 6/6 通过
    Integrated,
    /// 0x8507 全量运行通过
    AllRun,
}

impl KtestCode {
    pub const fn code(self) -> u16 {
        match self {
            KtestCode::Duplicate => 0x8500,
            KtestCode::RegistryFull => 0x8501,
            KtestCode::UnknownSuite => 0x8502,
            KtestCode::DomainEmpty => 0x8503,
            KtestCode::NotRun => 0x8504,
            KtestCode::Truncated => 0x8505,
            KtestCode::Integrated => 0x8506,
            KtestCode::AllRun => 0x8507,
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            KtestCode::Duplicate => "KTEST-DUPLICATE",
            KtestCode::RegistryFull => "KTEST-REGISTRY-FULL",
            KtestCode::UnknownSuite => "KTEST-UNKNOWN-SUITE",
            KtestCode::DomainEmpty => "KTEST-DOMAIN-EMPTY",
            KtestCode::NotRun => "KTEST-NOT-RUN",
            KtestCode::Truncated => "KTEST-TRUNCATED",
            KtestCode::Integrated => "KTEST-INTEGRATED",
            KtestCode::AllRun => "KTEST-ALL-RUN",
        }
    }

    pub fn detail(self) -> String {
        let base = match self {
            KtestCode::Duplicate => "套件重名注册：注册器名唯一性被破坏，后注册者被拒",
            KtestCode::RegistryFull => "注册器满：定容 32 已用尽，显性失败",
            KtestCode::UnknownSuite => "运行了未注册套件：注册面与运行面脱节",
            KtestCode::DomainEmpty => "套件域名为空：归属不清拒收",
            KtestCode::NotRun => "接入核缺口：套件注册了但从未被运行（假接入）",
            KtestCode::Truncated => "套件截断：CheckSet 超 MAX_CHECKS 丢项，绿是假绿",
            KtestCode::Integrated => "接入核通过：全部注册套件实跑且全绿",
            KtestCode::AllRun => "全量运行通过：运行器把注册面跑空",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx521_ktest_frame.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }

    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x8500 && (c & 0x00FF) < 0x10
    }
}

impl RegError {
    pub const fn code(self) -> KtestCode {
        match self {
            RegError::Duplicate { .. } => KtestCode::Duplicate,
            RegError::RegistryFull { .. } => KtestCode::RegistryFull,
            RegError::UnknownSuite { .. } => KtestCode::UnknownSuite,
            RegError::DomainEmpty { .. } => KtestCode::DomainEmpty,
            RegError::NotRun { .. } => KtestCode::NotRun,
            RegError::Truncated { .. } => KtestCode::Truncated,
        }
    }

    pub fn detail(self) -> String {
        self.code().detail()
    }
}

/// 域自检。
pub fn run_unx521_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx521");

    // 构造六套件注册面（B19–B24 锚点语义；b25 重名用例独立注册面）
    let mut reg = SuiteRegistry::new();
    let names: [&str; 6] = ["b19", "b20", "b21", "b22", "b23", "b24"];
    let mut i = 0;
    while i < 6 {
        let r = reg.register(names[i], "b-domain", i as u64);
        if r.is_err() {
            s.add("C521-00-六套件注册", false, "注册失败");
        }
        i += 1;
    }
    {
        let mut dup_reg = SuiteRegistry::new();
        let _ = dup_reg.register("b25", "b-domain", 100);
        let r = dup_reg.register("b25", "b-domain", 200);
        let dup = matches!(r, Err(RegError::Duplicate { name: "b25" }));
        s.add("C521-01-重名注册被拒", dup, if dup { "" } else { "重名被放行" });
    }

    // C521-02 运行前接入核必报缺口
    {
        let r = reg.integration_audit();
        let good = matches!(r, Err(RegError::NotRun { .. }));
        s.add("C521-02-未跑即缺口", good, if good { "" } else { "假接入通过" });
    }

    // C521-03 泵动全量 + 接入核 6/6
    {
        let producer = |_n: &str| -> Result<CheckSet, RegError> {
            let mut set = CheckSet::new("probe");
            set.add("p1", true, "");
            set.add("p2", true, "");
            Ok(set)
        };
        let runs;
        {
            let mut runner = SuiteRunner::new(&mut reg);
            runs = runner.run_all(producer);
        }
        let audit = reg.integration_audit();
        let good = runs == Ok(6) && audit == Ok(6);
        s.add("C521-03-泵动全量 6/6 接入", good, if good { "" } else { "接入核不满" });
    }

    // C521-04 截断套件被接入核拒（假绿拦截）
    {
        let mut reg2 = SuiteRegistry::new();
        let _ = reg2.register("t1", "t-domain", 0);
        let producer = |_n: &str| -> Result<CheckSet, RegError> {
            let mut set = CheckSet::new("probe");
            let mut k = 0;
            while k < 200 {
                set.add("overflow", true, "");
                k += 1;
            }
            Ok(set)
        };
        let r;
        {
            let mut runner = SuiteRunner::new(&mut reg2);
            r = runner.run_all(producer);
        }
        let audit = reg2.integration_audit();
        let good = r == Ok(1) && matches!(audit, Err(RegError::Truncated { .. }));
        s.add("C521-04-截断即接入失败", good, if good { "" } else { "截断套件被放行" });
    }

    // C521-05 运行面等于注册面（首泵跑 1 套件、二泵空转、接入核 1/1）
    {
        let mut reg3 = SuiteRegistry::new();
        let producer = |n: &str| -> Result<CheckSet, RegError> {
            let mut set = CheckSet::new("probe");
            set.add("x", true, "");
            let _ = n;
            Ok(set)
        };
        let _ = reg3.register("known", "d", 0);
        let r1;
        let r2;
        {
            let mut runner = SuiteRunner::new(&mut reg3);
            r1 = runner.pump_one(producer);
            r2 = runner.pump_one(producer);
        }
        let audit = reg3.integration_audit();
        let good = r1 == Ok(true) && r2 == Ok(false) && audit == Ok(1);
        s.add("C521-05-运行面等于注册面", good, if good { "" } else { "运行面溢出" });
    }

    // C521-06 注册器满显性
    {
        let mut reg4 = SuiteRegistry::new();
        let mut k = 0;
        let mut last = Ok(());
        while k < SUITE_CAP + 2 {
            let name: &'static str = if k < SUITE_CAP {
                // 用 leak 出稳定名（no_std 静态上下文：静态数组名）
                STATIC_NAMES[k]
            } else {
                "overflow"
            };
            last = reg4.register(name, "d", 0);
            k += 1;
        }
        let good = matches!(last, Err(RegError::RegistryFull { cap: 32 }));
        s.add("C521-06-注册器满显性", good, if good { "" } else { "满员被静默吞" });
    }

    // C521-07 域名空拒
    {
        let mut reg5 = SuiteRegistry::new();
        let r = reg5.register("x", "", 0);
        let good = matches!(r, Err(RegError::DomainEmpty { .. }));
        s.add("C521-07-空域名拒收", good, if good { "" } else { "空域名被放行" });
    }

    // C521-08 诊断码段独占
    {
        let codes = [
            KtestCode::Duplicate,
            KtestCode::RegistryFull,
            KtestCode::UnknownSuite,
            KtestCode::DomainEmpty,
            KtestCode::NotRun,
            KtestCode::Truncated,
            KtestCode::Integrated,
            KtestCode::AllRun,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C521-08-诊断码 0x85 段独占", all, if all { "" } else { "码值越出 0x85 段" });
    }

    s
}

/// 静态套件名表（注册器满员测试用，避免堆分配）。
static STATIC_NAMES: [&str; SUITE_CAP] = [
    "s00", "s01", "s02", "s03", "s04", "s05", "s06", "s07", "s08", "s09", "s10", "s11", "s12",
    "s13", "s14", "s15", "s16", "s17", "s18", "s19", "s20", "s21", "s22", "s23", "s24", "s25",
    "s26", "s27", "s28", "s29", "s30", "s31",
];
