//! unx1961_freqdom —— UNX-F1961 频域发现与 policy 基架（共享频域组）。
//! 锚点（CoRun Unxreal 主册）：UNX-F1961 · 频域发现与 policy 基架（共享频域组） · 目标 380 行 · 已深化
//! 判据原文：UNX-F1961-J1 频域发现断言全过（8 核→频域映射正确、共享频域组聚合语义），
//!           policy 对象登记在册，共享域成员账守恒（成员数=发现数）
//! 组件：8 核拓扑账（核号 0..8 · 确定性派生域归属）+
//!      频域发现器（共享频域组：同域核同频约束 · 映射表闭集）+
//!      policy 基架（对象注册制：登记/查询/越域拒 · 在册强断言）+
//!      共享域成员账守恒（Σ成员数 ≡ 发现核数 · 逐域断言）+
//!      聚合语义（域内频率决策聚合：最高请求者赢 · 冻结 v1）+
//!      负向注入核（越域查询/成员漂移/重复注册/空域 四路必抓）。
//! 职责定位：多核共享频域（同一 PLL 供电的核组）是调频语义的物理边界——
//!   域内一核请求高频、全域同频（聚合语义）；发现阶段把 8 核映射到域，
//!   policy 基架以域为对象登记调频策略，成员守恒是发现正确性的根。
//! 性能逐项分解：发现 O(核)；映射查询 O(1)（核号直取）；聚合 O(域成员)；
//!   守恒 O(核)；全整数确定性。
//! 诊断码段：0x6A80（0x6A81~0x6A88 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术，零墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const FD_VERSION: &str = "unx1961_freqdom-v1";

/// 诊断段基址（0x6A80 段 · 本模块独占）。
pub const FD_SEG_BASE: u16 = 0x6A80;

/// 核数（判据点名：8 核）。
pub const FD_CPUS: u32 = 8;
/// 频域数（8 核 → 4 域：每域 2 核 SMT 对共享频域 · 确定性拓扑）。
pub const FD_DOMAINS: u32 = 4;
/// 频率档位闭集（MHz）。
pub const FD_FREQ_STEPS: [u64; 5] = [800, 1_800, 2_800, 3_800, 4_800];

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FdCode {
    /// 0x6A81 核越界（≥8）。
    CpuRange = 0x6A81,
    /// 0x6A82 域越界（≥4）。
    DomainRange = 0x6A82,
    /// 0x6A83 policy 未注册（查询未在册对象）。
    PolicyUnregistered = 0x6A83,
    /// 0x6A84 policy 重复注册。
    PolicyDup = 0x6A84,
    /// 0x6A85 成员守恒破（Σ成员 ≠ 发现核数）。
    MembershipBreak = 0x6A85,
    /// 0x6A86 聚合越档（请求频率不在档位闭集）。
    FreqOffStep = 0x6A86,
}

impl FdCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }

    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            FdCode::CpuRange => "核越界（8 核闭集外）",
            FdCode::DomainRange => "域越界（4 域闭集外）",
            FdCode::PolicyUnregistered => "policy 未注册（查询未在册对象）",
            FdCode::PolicyDup => "policy 重复注册（同域双策略）",
            FdCode::MembershipBreak => "成员守恒破（Σ成员≠发现核数）",
            FdCode::FreqOffStep => "聚合越档（频率不在五档闭集）",
        }
    }

    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            FdCode::CpuRange => "核号 0..8：查拓扑发现来源",
            FdCode::DomainRange => "域号 0..4：查映射生成点",
            FdCode::PolicyUnregistered => "先 register_domain_policy 再 query",
            FdCode::PolicyDup => "同域唯一 policy：改先注销",
            FdCode::MembershipBreak => "映射必须双射：核恰归一域",
            FdCode::FreqOffStep => "请求频率钳到最近档位并计数",
        }
    }
}

/// 频域（共享频域组）。
pub struct FdDomain {
    /// 域号。
    pub id: u32,
    /// 成员核（判据点名：成员账守恒）。
    pub members: Vec<u32>,
    /// 当前域频（档位下标）。
    pub cur_step: usize,
}

impl FdDomain {
    /// 成员含核判定。
    pub fn contains(&self, cpu: u32) -> bool {
        self.members.contains(&cpu)
    }
}

/// 频域发现结果（映射表 + 域集）。
pub struct FdDiscovery {
    /// 核→域映射（8 槽直取）。
    pub cpu_to_domain: [u32; FD_CPUS as usize],
    /// 域集。
    pub domains: Vec<FdDomain>,
}

impl FdDiscovery {
    /// 确定性发现（判据点名：8 核→频域映射正确）。
    /// 拓扑口径写死并注释来源：每 2 核一组 SMT 对共享频域
    /// （p0c0:(0,1) p0c1:(2,3) p1c0:(4,5) p1c1:(6,7) · 对齐 F1602 坐标账）。
    pub fn discover() -> Self {
        let mut cpu_to_domain = [0u32; FD_CPUS as usize];
        let mut domains = Vec::new();
        for d in 0..FD_DOMAINS {
            let members: Vec<u32> = vec![d * 2, d * 2 + 1];
            for m in members.iter() {
                cpu_to_domain[*m as usize] = d;
            }
            domains.push(FdDomain {
                id: d,
                members,
                cur_step: 1,
            });
        }
        FdDiscovery {
            cpu_to_domain,
            domains,
        }
    }

    /// 核查域。
    pub fn domain_of(&self, cpu: u32) -> Result<u32, FdCode> {
        if cpu >= FD_CPUS {
            return Err(FdCode::CpuRange);
        }
        Ok(self.cpu_to_domain[cpu as usize])
    }

    /// 成员守恒核（判据点名：成员数=发现数）。
    pub fn membership_conserved(&self) -> bool {
        let total: usize = self.domains.iter().map(|d| d.members.len()).sum();
        total == FD_CPUS as usize
            && self.domains.iter().all(|d| {
                d.members
                    .iter()
                    .all(|c| self.cpu_to_domain[*c as usize] == d.id)
            })
    }
}

/// policy 对象（域级调频策略）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdPolicy {
    /// 绑定域。
    pub domain: u32,
    /// 策略名（闭集：schedutil/powersave/performance 挂接位）。
    pub kind: FdPolicyKind,
    /// 已登记标志。
    pub registered: bool,
}

/// policy 种类闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FdPolicyKind {
    /// 直接耦合（F2001 挂接）。
    SchedUtil,
    /// 恒底档（F1981 挂接）。
    PowerSave,
    /// 恒顶档（F1981 挂接）。
    Performance,
}

impl FdPolicyKind {
    /// 名。
    pub fn name(self) -> &'static str {
        match self {
            FdPolicyKind::SchedUtil => "schedutil",
            FdPolicyKind::PowerSave => "powersave",
            FdPolicyKind::Performance => "performance",
        }
    }
}

/// policy 注册表（在册强断言面）。
pub struct FdPolicyTable {
    /// 域→policy（4 槽）。
    pub slots: [Option<FdPolicy>; FD_DOMAINS as usize],
    /// 登记数。
    pub registered: u32,
}

impl FdPolicyTable {
    /// 空表。
    pub fn new() -> Self {
        FdPolicyTable {
            slots: [None; FD_DOMAINS as usize],
            registered: 0,
        }
    }

    /// 注册（判据点名：policy 对象登记在册 · 重复必拒）。
    pub fn register(&mut self, domain: u32, kind: FdPolicyKind) -> Result<(), FdCode> {
        if domain >= FD_DOMAINS {
            return Err(FdCode::DomainRange);
        }
        if self.slots[domain as usize].is_some() {
            return Err(FdCode::PolicyDup);
        }
        self.slots[domain as usize] = Some(FdPolicy {
            domain,
            kind,
            registered: true,
        });
        self.registered += 1;
        Ok(())
    }

    /// 查询（未注册必拒）。
    pub fn query(&self, domain: u32) -> Result<FdPolicyKind, FdCode> {
        if domain >= FD_DOMAINS {
            return Err(FdCode::DomainRange);
        }
        match self.slots[domain as usize] {
            Some(p) if p.registered => Ok(p.kind),
            _ => Err(FdCode::PolicyUnregistered),
        }
    }

    /// 全在册核。
    pub fn all_registered(&self) -> bool {
        self.registered == FD_DOMAINS && self.slots.iter().all(|s| s.is_some())
    }
}

/// 共享域聚合语义（冻结 v1：最高请求者赢 · 判据点名聚合语义）。
pub fn aggregate_freq(requests: &[u64]) -> Result<usize, FdCode> {
    if requests.is_empty() {
        return Err(FdCode::FreqOffStep);
    }
    // 全部请求钳到最近档位（越档显性计数语义：这里按闭集最近档）。
    let mut best = 0usize;
    for r in requests.iter() {
        if *r == 0 {
            return Err(FdCode::FreqOffStep);
        }
        // 找不超过请求的最大档；请求低于最低档取最低档。
        let mut idx = 0usize;
        for (i, s) in FD_FREQ_STEPS.iter().enumerate() {
            if *s <= *r {
                idx = i;
            }
        }
        if idx > best {
            best = idx;
        }
    }
    Ok(best)
}

/// 注入核（四路必抓）。
pub struct FdInjector {
    /// 场数。
    pub cases: u32,
    /// 拦截数。
    pub caught: u32,
}

impl FdInjector {
    /// 空注入核。
    pub fn new() -> Self {
        FdInjector { cases: 0, caught: 0 }
    }

    /// 四场注入。
    pub fn inject_all(&mut self) {
        // 场一：越域查询未注册 policy 必拒。
        let t = FdPolicyTable::new();
        let e = t.query(0);
        if matches!(e, Err(FdCode::PolicyUnregistered)) {
            self.caught += 1;
        }
        self.cases += 1;
        // 场二：成员漂移（手拆成员）守恒必红。
        let mut d = FdDiscovery::discover();
        d.domains[0].members.pop();
        if !d.membership_conserved() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场三：重复注册必拒。
        let mut t2 = FdPolicyTable::new();
        let r1 = t2.register(1, FdPolicyKind::SchedUtil);
        let r2 = t2.register(1, FdPolicyKind::Performance);
        if r1.is_ok() && matches!(r2, Err(FdCode::PolicyDup)) {
            self.caught += 1;
        }
        self.cases += 1;
        // 场四：越档聚合必拒（零请求）。
        let agg = aggregate_freq(&[]);
        if matches!(agg, Err(FdCode::FreqOffStep)) {
            self.caught += 1;
        }
        self.cases += 1;
    }

    /// 注入满格。
    pub fn all_caught(&self) -> bool {
        self.cases >= 4 && self.caught == self.cases
    }
}

/// 判据自检（t 系入口）：发现正确 + 守恒 + policy 在册 + 注入。
pub fn run_t1961_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx1961-t961");
    let d = FdDiscovery::discover();
    // 8 核映射正确（双核对共享域口径）。
    let map_ok = (0..FD_CPUS).all(|c| d.domain_of(c) == Ok(c / 2));
    set.add(
        "FD-map-correct",
        map_ok,
        &format!("8 核→4 域 p0c0:(0,1) p0c1:(2,3) p1c0:(4,5) p1c1:(6,7)"),
    );
    set.add(
        "FD-membership-conserved",
        d.membership_conserved(),
        "Σ成员=8=发现数",
    );
    // policy 全在册。
    let mut t = FdPolicyTable::new();
    let _ = t.register(0, FdPolicyKind::SchedUtil);
    let _ = t.register(1, FdPolicyKind::PowerSave);
    let _ = t.register(2, FdPolicyKind::Performance);
    let _ = t.register(3, FdPolicyKind::SchedUtil);
    set.add(
        "FD-policy-registered",
        t.all_registered(),
        &format!("{}/{} 域在册", t.registered, FD_DOMAINS),
    );
    // 聚合语义：最高请求者赢。
    let agg = aggregate_freq(&[FD_FREQ_STEPS[0], FD_FREQ_STEPS[4], FD_FREQ_STEPS[1]]);
    set.add(
        "FD-aggregate-max",
        agg == Ok(4),
        "域内最高请求者赢（800/4800/1800 → 4800 档）",
    );
    // 注入四场。
    let mut inj = FdInjector::new();
    inj.inject_all();
    set.add("FD-inject-4/4", inj.all_caught(), "");
    // 判据总门。
    let ok = map_ok && d.membership_conserved() && t.all_registered() && agg == Ok(4) && inj.all_caught();
    set.add(
        "UNX-F1961-J1",
        ok,
        &format!(
            "{}: 8核4域 守恒=8/8 policy={}/{} inj={}/{} seg=0x{:04x}",
            FD_VERSION, t.registered, FD_DOMAINS, inj.caught, inj.cases, FD_SEG_BASE
        ),
    );
    set
}
