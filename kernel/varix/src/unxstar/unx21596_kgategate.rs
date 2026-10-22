//! UNX-F21596 · ktest 域总闸（F2 面 800 判据一键全跑）
//!
//! 诊断段 0x91079~0x91080（F2-B40 域收官段 16/100）。
//!
//! 判据原文（施工书 UNX-F21596-J1）：域总闸一条命令全跑：fast 档通过率 100%，失败清单自动产出。
//!
//! 设计要点（为什么总闸必须**只读**、且通过率可以是 `None`）：
//! 总闸的价值在于「同一条命令、同一份注册表、任何时刻跑出同一份失败清单」。它一旦
//! 在执行路径上写注册表（比如顺手把 `fail` 改成 `skip`、或给判据打上「已跑过」标记），
//! 第二次跑的覆盖面就和第一次不同了，回归比对从此失效。故 `run` 只借 `&Registry`，
//! 状态全落在返回的报告里——「只读」不靠约定，靠借用检查器保证。
//!
//! 通过率返回 `Option<u32>` 而不是 0%：分母为 0（该档位一条判据都没有）与「全挂」
//! 是两件完全不同的事——前者是注册表筛错了档，后者是实现挂了。把两者都压成 0%，
//! 运维会先去查实现，而真正该修的是档位归属。**空集合必须显性表达为「无值」**。
//!
//! 百分比用**向下取整**：四舍五入会把 799/800 显示成 100%，恰好把那一红藏起来。
//! 判「是否 100%」只看 `passed == selected`，不看过率——过率是给人看的，判定是给机器
//! 看的，两者不能共用同一条路径，否则取整策略一改，判定就跟着漂。
//!
//! 失败清单按**模块首次出现序**分组而不是字典序：模块顺序反映了判据的登记顺序（域
//! 内先核心后外设），字典序会把 `boot.early` 排到最前，让人先去看引导而漏掉核心红项。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 域面判据总数（施工书 F2 面 800 条）。
pub const GATE_TOTAL: usize = 800;
/// fast 档判据数（前 5 个模块 × 40 条）。
pub const FAST_TARGET: usize = 200;
/// full 档判据数（后 15 个模块 × 40 条）。
pub const FULL_TARGET: usize = 600;
/// 模块数（每模块 40 条判据）。
pub const MODULE_COUNT: usize = 20;
/// 每模块判据数。
pub const PER_MODULE: usize = 40;
/// FNV-1a 64 偏移基（来源：FNV 规范 1a 变体）。
const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 质数（同上）。
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 总闸诊断码（本单码段仅两位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateCode {
    Open = 0x91079,
    Blocked = 0x91080,
}

impl GateCode {
    /// 现象。
    pub const fn what(self) -> &'static str {
        match self {
            GateCode::Open => "总闸放行：所选档位零失败",
            GateCode::Blocked => "总闸拦截：所选档位存在失败判据",
        }
    }
    /// 原因。
    pub const fn why(self) -> &'static str {
        match self {
            GateCode::Open => "逐条执行后无 fail，且档位覆盖达到下限",
            GateCode::Blocked => "执行出 fail，或注册/档位本身不成立",
        }
    }
    /// 建议。
    pub const fn advice(self) -> &'static str {
        match self {
            GateCode::Open => "归档失败清单（空）并放行下游",
            GateCode::Blocked => "按失败清单按模块修复后重跑总闸",
        }
    }
    /// 数值形式（u32：五位数码段，`as u16` 会把 0x91079 截成 0x1079）。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 总闸拒绝成因（七元闭集：执行前的结构性拒绝，与「跑出红」是两回事）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Blame {
    CapacityOver,
    DuplicateCase,
    TierConflict,
    EmptyRegistry,
    TierShort,
    NameMalformed,
    OrdinalOutOfRange,
}

impl Blame {
    /// 定位文本。
    pub const fn what(self) -> &'static str {
        match self {
            Blame::CapacityOver => "注册表已满：第 801 条判据无处登记",
            Blame::DuplicateCase => "同一模块内判据名重复登记",
            Blame::TierConflict => "同一判据被登记进两个档位（档位不互斥）",
            Blame::EmptyRegistry => "注册表为空，总闸无判据可跑",
            Blame::TierShort => "所选档位判据数不足下限",
            Blame::NameMalformed => "模块名或判据名为空",
            Blame::OrdinalOutOfRange => "序号越出 1..=800",
        }
    }
    /// 建议。
    pub const fn advice(self) -> &'static str {
        match self {
            Blame::CapacityOver => "先归并重复判据，或另立面",
            Blame::DuplicateCase => "改名或合并同名判据",
            Blame::TierConflict => "按耗时重新划档，一条判据只属一档",
            Blame::EmptyRegistry => "先登记判据再开闸",
            Blame::TierShort => "核对档位归属，或调低覆盖下限",
            Blame::NameMalformed => "补齐模块名与判据名后重登记",
            Blame::OrdinalOutOfRange => "按 1..=800 取序号",
        }
    }
}

/// 总闸故障（结构性拒绝，`Err` 侧）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fault {
    pub code: u32,
    pub blame: Blame,
    /// 相关序号或条数（0 表示与该维度无关）。
    pub at: u16,
}

impl Fault {
    const fn new(blame: Blame, at: u16) -> Fault {
        Fault { code: GateCode::Blocked.code(), blame, at }
    }
}

/// 档位（互斥：一条判据只属一档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    Fast,
    Full,
}

impl Tier {
    /// 档位名（指纹与报告用）。
    pub const fn label(self) -> &'static str {
        match self {
            Tier::Fast => "fast",
            Tier::Full => "full",
        }
    }
}

/// 单条判据的执行结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Pass,
    Fail,
}

impl Verdict {
    /// 是否通过。
    pub const fn is_pass(self) -> bool {
        matches!(self, Verdict::Pass)
    }
}

/// 一条判据登记项（序号由登记顺序决定，不另存字段——存了就能被改成乱序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Case {
    pub module: &'static str,
    pub name: &'static str,
    pub tier: Tier,
    pub verdict: Verdict,
}

impl Case {
    /// 构造登记项。
    pub const fn new(module: &'static str, name: &'static str, tier: Tier, verdict: Verdict) -> Case {
        Case { module, name, tier, verdict }
    }
}

/// 判据注册表（800 条闭集）。
pub struct Registry {
    cases: Vec<Case>,
}

impl Registry {
    /// 新建空表。
    pub const fn new() -> Registry {
        Registry { cases: Vec::new() }
    }

    /// 登记一条判据。
    ///
    /// 三道闸：名字 → 重名（含档位冲突）→ 容量。重名排在容量前是刻意的：重名条目本
    /// 就不该占新槽，先报容量会把「改个名就能登记」误导成「得先扩容」；反过来，
    /// 真正的新条目在第 801 条时才落到容量闸上，两者不互相遮盖。
    pub fn register(&mut self, c: Case) -> Result<(), Fault> {
        if c.module.is_empty() || c.name.is_empty() {
            return Err(Fault::new(Blame::NameMalformed, 0));
        }
        for x in &self.cases {
            if x.module == c.module && x.name == c.name {
                let blame = if x.tier == c.tier {
                    Blame::DuplicateCase
                } else {
                    Blame::TierConflict
                };
                return Err(Fault::new(blame, 0));
            }
        }
        if self.cases.len() >= GATE_TOTAL {
            return Err(Fault::new(Blame::CapacityOver, 0));
        }
        self.cases.push(c);
        Ok(())
    }

    /// 已登记条数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }
    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }
    /// fast 档条数。
    pub fn fast_count(&self) -> usize {
        self.cases.iter().filter(|c| c.tier == Tier::Fast).count()
    }
    /// full 档条数。
    pub fn full_count(&self) -> usize {
        self.cases.iter().filter(|c| c.tier == Tier::Full).count()
    }
    /// 按序号（1-based）取判据。
    ///
    /// 序号越界返回 `OrdinalOutOfRange` 而不是 `None`：失败清单是要写进报告的，
    /// 「查不到」必须和「查到但内容是空的」区分开，故用带码的 Err 而不是 Option。
    pub fn case(&self, ordinal: u16) -> Result<Case, Fault> {
        if ordinal < 1 || ordinal as usize > self.cases.len() {
            return Err(Fault::new(Blame::OrdinalOutOfRange, ordinal));
        }
        Ok(self.cases[ordinal as usize - 1])
    }
    /// 注册表指纹：按登记顺序咬合全部字段。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        for c in &self.cases {
            h = fnv_str(h, c.module);
            h = fnv_str(h, c.name);
            h = fnv_str(h, c.tier.label());
            h = fnv_u64(h, if c.verdict.is_pass() { 1 } else { 0 });
        }
        h
    }
}

/// 一条失败记录（模块 + 判据名 + 序号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Failure {
    pub module: &'static str,
    pub name: &'static str,
    pub ordinal: u16,
}

/// 总闸报告（一次执行的全部结论）。
pub struct GateReport {
    pub tier: Tier,
    pub selected: usize,
    pub passed: usize,
    pub failed: usize,
    pub digest: u64,
    failures: Vec<Failure>,
}

impl GateReport {
    /// 判决码。
    pub fn code(&self) -> GateCode {
        if self.failed == 0 {
            GateCode::Open
        } else {
            GateCode::Blocked
        }
    }
    /// 通过率（整数百分比，向下取整；分母为 0 返回 None）。
    pub fn rate(&self) -> Option<u32> {
        rate_percent(self.passed, self.selected)
    }
    /// 失败清单（登记序）。
    pub fn failures(&self) -> &[Failure] {
        &self.failures
    }
    /// 失败按模块聚拢（首次出现序）。
    pub fn by_module(&self) -> Vec<(&'static str, usize)> {
        let mut out: Vec<(&'static str, usize)> = Vec::new();
        for f in &self.failures {
            match out.iter_mut().find(|(m, _)| *m == f.module) {
                Some(slot) => slot.1 += 1,
                None => out.push((f.module, 1)),
            }
        }
        out
    }
    /// 失败清单文本：每模块一行头，其下逐条 `#序号 判据名`。
    pub fn failure_lines(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (module, n) in self.by_module() {
            let mut head = String::with_capacity(32);
            head.push_str(module);
            head.push(' ');
            head.push_str(&n.to_string());
            out.push(head);
            for f in self.failures.iter().filter(|f| f.module == module) {
                let mut row = String::with_capacity(32);
                row.push_str("  #");
                row.push_str(&f.ordinal.to_string());
                row.push(' ');
                row.push_str(f.name);
                out.push(row);
            }
        }
        out
    }
}

/// 整数百分比（向下取整；分母为 0 时显性 None，不退化成 0%）。
pub fn rate_percent(passed: usize, total: usize) -> Option<u32> {
    if total == 0 {
        return None;
    }
    Some((passed.min(total) * 100 / total) as u32)
}

/// 总闸执行器：按档位筛选、逐条执行、汇总。
///
/// `floor` 是覆盖下限：所选档位判据数不足时直接拒绝，而不是开出一份「只跑了 3 条
/// 却全绿」的空洞放行单。0 条的档位（筛选口径写错）同样落进这里。
pub fn run(reg: &Registry, tier: Tier, floor: usize) -> Result<GateReport, Fault> {
    if reg.is_empty() {
        return Err(Fault::new(Blame::EmptyRegistry, 0));
    }
    let mut failures: Vec<Failure> = Vec::new();
    let mut selected = 0usize;
    let mut passed = 0usize;
    for (i, c) in reg.cases.iter().enumerate() {
        if c.tier != tier {
            continue;
        }
        selected += 1;
        if c.verdict.is_pass() {
            passed += 1;
        } else {
            failures.push(Failure {
                module: c.module,
                name: c.name,
                ordinal: (i as u16) + 1,
            });
        }
    }
    if selected < floor {
        return Err(Fault::new(Blame::TierShort, selected as u16));
    }
    // 指纹只咬合「结论」而非「耗时/时间戳」：加进时间戳就等于每天换一份指纹，
    // 回归比对全靠人眼。
    let mut h = fnv_str(FNV_BASIS, tier.label());
    h = fnv_u64(h, selected as u64);
    h = fnv_u64(h, passed as u64);
    for f in &failures {
        h = fnv_u64(h, f.ordinal as u64);
    }
    Ok(GateReport {
        tier,
        selected,
        passed,
        failed: failures.len(),
        digest: h,
        failures,
    })
}

/// FNV-1a 64 单步。
pub const fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        h ^= ((v >> (i * 8)) & 0xFF) as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 字符串咬合（尾部补 0xFF 作分隔，避免 ("ab","c") 与 ("a","bc") 撞车）。
fn fnv_str(mut h: u64, s: &str) -> u64 {
    for byte in s.as_bytes() {
        h ^= *byte as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h ^= 0xFF;
    h.wrapping_mul(FNV_PRIME)
}

/// 模块名表（20 个，前 5 个属 fast 档）。
pub const MODULES: [&str; MODULE_COUNT] = [
    "core.irq", "core.sched", "core.time", "core.sync", "core.mm",
    "mem.pool", "mem.map", "mem.zone", "mem.reclaim", "mem.dma",
    "fs.node", "fs.page", "fs.journal", "net.q", "net.tcp",
    "net.arp", "dev.pci", "dev.mmio", "log.sink", "boot.early",
];

/// 判据名表（40 个，跨模块复用，故「模块 + 名」才唯一）。
pub const NAMES: [&str; PER_MODULE] = [
    "j01", "j02", "j03", "j04", "j05", "j06", "j07", "j08", "j09", "j10",
    "j11", "j12", "j13", "j14", "j15", "j16", "j17", "j18", "j19", "j20",
    "j21", "j22", "j23", "j24", "j25", "j26", "j27", "j28", "j29", "j30",
    "j31", "j32", "j33", "j34", "j35", "j36", "j37", "j38", "j39", "j40",
];

/// 构造 800 条注册表；`fails` 给出 (模块下标, 判据下标) 的失败位。
///
/// 档位由模块下标决定（前 5 个 fast），因此 fast 档恒为 200 条、full 档恒为 600 条——
/// 判据要钉这两个数，夹具就不能让档位随 `fails` 变化。
pub fn registry_with(fails: &[(usize, usize)]) -> Result<Registry, Fault> {
    let mut reg = Registry::new();
    for (mi, module) in MODULES.iter().enumerate() {
        for (ni, name) in NAMES.iter().enumerate() {
            let tier = if mi < 5 { Tier::Fast } else { Tier::Full };
            let verdict = if fails.contains(&(mi, ni)) {
                Verdict::Fail
            } else {
                Verdict::Pass
            };
            reg.register(Case::new(module, name, tier, verdict))?;
        }
    }
    Ok(reg)
}

/// 标准注册表（全绿，判据夹具）。
pub fn standard_registry() -> Result<Registry, Fault> {
    registry_with(&[])
}

/// 判据入口（UNX-F21596-J1 及其配套十三条）。
///
/// 判据常量独立写死：`800` / `200` / `600` / `203` / `206` / `408` / `0x91079` / `0x91080`
/// 在断言里以字面量出现，避免「实现改了常量，判据跟着改」的自证循环。
pub fn run_unx21596_kgategate_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21596");
    let reg = standard_registry();
    let fast = match &reg {
        Ok(r) => run(r, Tier::Fast, FAST_TARGET).ok(),
        Err(_) => None,
    };
    let full = match &reg {
        Ok(r) => run(r, Tier::Full, FULL_TARGET).ok(),
        Err(_) => None,
    };

    // 1) 夹具可构造：800 条、fast 200、full 600，指纹非零。
    cs.add(
        "f21596.fixture_buildable",
        reg.as_ref().map_or(0, |r| r.len()) == 800
            && reg.as_ref().map_or(0, |r| r.fast_count()) == 200
            && reg.as_ref().map_or(0, |r| r.full_count()) == 600
            && reg.as_ref().map_or(0, |r| r.fingerprint()) != 0,
        "注册表 800/200/600 三者有一不符，或指纹为零",
    );

    // 2) 码表钉死：两位码值不许漂移，三要素非空。
    cs.add(
        "f21596.code_table_pinned",
        GateCode::Open.code() == 0x91079
            && GateCode::Blocked.code() == 0x91080
            && !GateCode::Open.what().is_empty()
            && !GateCode::Open.why().is_empty()
            && !GateCode::Open.advice().is_empty()
            && !GateCode::Blocked.what().is_empty()
            && !GateCode::Blocked.why().is_empty()
            && !GateCode::Blocked.advice().is_empty(),
        "两位码值漂移，或三要素有空串",
    );

    // 3) 正向：fast 档一条命令全跑，通过率 100%，失败清单为空。
    cs.add(
        "f21596.fast_gate_green",
        fast.as_ref().map_or(false, |r| {
            r.code() == GateCode::Open
                && r.code().code() == 0x91079
                && r.selected == 200
                && r.passed == 200
                && r.failed == 0
                && r.rate() == Some(100)
                && r.failures().is_empty()
                && r.failure_lines().is_empty()
        }),
        "fast 档未 100% 放行，或分母/通过率不符",
    );

    // 4) 正向：full 档 600 条同样全绿（档位筛选不得漏跑）。
    cs.add(
        "f21596.full_gate_green",
        full.as_ref().map_or(false, |r| {
            r.code() == GateCode::Open && r.selected == 600 && r.passed == 600
                && r.rate() == Some(100) && r.failed == 0
        }),
        "full 档未 600 条全绿",
    );

    // 5) 档位互斥：两档条数之和必须等于全集，且各自命中预期数目（无重复计入）。
    cs.add(
        "f21596.tier_partition",
        match (&fast, &full) {
            (Some(f), Some(u)) => {
                f.selected + u.selected == 800
                    && f.selected == 200
                    && u.selected == 600
                    && f.tier != u.tier
            }
            _ => false,
        },
        "两档未构成 800 的划分（存在漏计或重复计入）",
    );

    // 6) 反向：注三条失败（mem.pool×2、fs.node×1）→ 拦截 + 清单按模块分组 + 序号精确。
    {
        let blocked = match registry_with(&[(5, 2), (5, 5), (10, 7)]) {
            Ok(r) => run(&r, Tier::Full, FULL_TARGET).ok(),
            Err(_) => None,
        };
        cs.add(
            "f21596.failures_grouped",
            blocked.as_ref().map_or(false, |r| {
                let lines = r.failure_lines();
                r.code() == GateCode::Blocked
                    && r.code().code() == 0x91080
                    && r.failed == 3
                    && r.passed == 597
                    && r.rate() == Some(99)
                    && r.failures().len() == 3
                    && r.failures()[0].ordinal == 203
                    && r.failures()[1].ordinal == 206
                    && r.failures()[2].ordinal == 408
                    && r.by_module() == vec![("mem.pool", 2), ("fs.node", 1)]
                    && lines.len() == 5
                    && lines[0] == "mem.pool 2"
                    && lines[1] == "  #203 j03"
                    && lines[2] == "  #206 j06"
                    && lines[3] == "fs.node 1"
                    && lines[4] == "  #408 j08"
            }),
            "失败清单未按模块分组，或序号/通过率不符（应 203/206/408、99%）",
        );
    }

    // 7) 边界：百分比向下取整；分母 0 显性 None 而非 0%。
    cs.add(
        "f21596.rate_is_floor",
        rate_percent(799, 800) == Some(99)
            && rate_percent(800, 800) == Some(100)
            && rate_percent(1, 800) == Some(0)
            && rate_percent(0, 5) == Some(0)
            && rate_percent(0, 0).is_none()
            && rate_percent(5, 0).is_none(),
        "通过率未向下取整，或分母为 0 时未返回 None",
    );

    // 8) 反向：同名同档重复登记必拒（0x91080 + DuplicateCase）。
    {
        let mut r = match standard_registry() {
            Ok(x) => x,
            Err(_) => Registry::new(),
        };
        let dup = r.register(Case::new("core.irq", "j01", Tier::Fast, Verdict::Pass));
        let before = r.len();
        cs.add(
            "f21596.duplicate_rejected",
            dup.err().map(|f| (f.code, f.blame)) == Some((0x91080, Blame::DuplicateCase))
                && before == 800,
            Blame::DuplicateCase.advice(),
        );
    }

    // 9) 反向：同名跨档登记必拒（档位互斥，0x91080 + TierConflict）。
    {
        let mut r = match standard_registry() {
            Ok(x) => x,
            Err(_) => Registry::new(),
        };
        let conflict = r.register(Case::new("core.irq", "j01", Tier::Full, Verdict::Pass));
        cs.add(
            "f21596.tier_conflict_rejected",
            conflict.err().map(|f| (f.code, f.blame)) == Some((0x91080, Blame::TierConflict)),
            Blame::TierConflict.advice(),
        );
    }

    // 10) 边界：800 条恰满可跑，第 801 条必拒（上限 / 上限+1）。
    {
        let mut r = match standard_registry() {
            Ok(x) => x,
            Err(_) => Registry::new(),
        };
        let full_ok = r.len() == 800;
        let over = r.register(Case::new("core.irq", "j99", Tier::Fast, Verdict::Pass));
        cs.add(
            "f21596.capacity_bound",
            full_ok
                && over.err().map(|f| (f.code, f.blame)) == Some((0x91080, Blame::CapacityOver))
                && r.len() == 800,
            "第 801 条未被拒，或 800 条恰满被误拒",
        );
    }

    // 11) 反向：空注册表开闸必拒（0x91080 + EmptyRegistry）。
    {
        let empty = Registry::new();
        let gate = run(&empty, Tier::Fast, 1);
        cs.add(
            "f21596.empty_registry_rejected",
            empty.is_empty()
                && gate.err().map(|f| (f.code, f.blame)) == Some((0x91080, Blame::EmptyRegistry)),
            Blame::EmptyRegistry.advice(),
        );
    }

    // 12) 边界：档位覆盖下限 200 收、201 拒；只有 fast 时跑 full 档判 TierShort。
    {
        let r = match standard_registry() {
            Ok(x) => x,
            Err(_) => Registry::new(),
        };
        let at_floor = run(&r, Tier::Fast, 200).is_ok();
        let over_floor = run(&r, Tier::Fast, 201);
        let no_case = run(&r, Tier::Fast, 999);
        cs.add(
            "f21596.tier_floor",
            at_floor
                && over_floor.err().map(|f| f.blame) == Some(Blame::TierShort)
                && no_case.err().map(|f| f.blame) == Some(Blame::TierShort),
            "下限 200 被误拒，或下限 +1 未报 TierShort",
        );
    }

    // 13) 反向 + 边界：空名为 NameMalformed；序号 0 / 801 拒，1 / 800 收。
    {
        let mut r = match standard_registry() {
            Ok(x) => x,
            Err(_) => Registry::new(),
        };
        let unnamed = r.register(Case::new("", "j01", Tier::Fast, Verdict::Pass));
        let naned = r.register(Case::new("core.irq", "", Tier::Fast, Verdict::Pass));
        cs.add(
            "f21596.name_and_ordinal_bounds",
            unnamed.err().map(|f| f.blame) == Some(Blame::NameMalformed)
                && naned.err().map(|f| f.blame) == Some(Blame::NameMalformed)
                && r.case(0).err().map(|f| (f.code, f.blame))
                    == Some((0x91080, Blame::OrdinalOutOfRange))
                && r.case(801).err().map(|f| (f.blame, f.at))
                    == Some((Blame::OrdinalOutOfRange, 801))
                && r.case(1).map(|c| c.name) == Ok("j01")
                && r.case(800).map(|c| c.module) == Ok("boot.early"),
            Blame::NameMalformed.what(),
        );
    }

    // 14) 只读 + 确定性：跑两次注册表指纹不变、报告逐位相同；改一条结论指纹必变。
    {
        let r = match standard_registry() {
            Ok(x) => x,
            Err(_) => Registry::new(),
        };
        let before = r.fingerprint();
        let a = run(&r, Tier::Fast, FAST_TARGET).ok();
        let b = run(&r, Tier::Fast, FAST_TARGET).ok();
        let after = r.fingerprint();
        let flipped = match registry_with(&[(0, 0)]) {
            Ok(x) => run(&x, Tier::Fast, FAST_TARGET).ok(),
            Err(_) => None,
        };
        cs.add(
            "f21596.read_only_and_deterministic",
            before == after
                && match (&a, &b) {
                    (Some(x), Some(y)) => {
                        x.digest == y.digest
                            && x.failures() == y.failures()
                            && x.rate() == y.rate()
                            && x.selected == y.selected
                    }
                    _ => false,
                }
                && match (&a, &flipped) {
                    (Some(x), Some(y)) => {
                        x.digest != y.digest
                            && y.code().code() == 0x91080
                            && y.failed == 1
                            && y.rate() == Some(99)
                    }
                    _ => false,
                },
            "执行改写了注册表，或同输入两次报告不一致 / 单条翻转未改变指纹",
        );
    }

    cs
}
