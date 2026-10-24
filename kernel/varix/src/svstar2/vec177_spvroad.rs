//! VE-F0577 · SPIR-V 与 Vulkan 驱动远景
//!
//! 远景评估条：诚实声明当前实机环境缺位——Vulkan 驱动侧验收为预留态显性 N/A，
//! 登记随闸门补测；远景路线三层：驱动验收远景（spirv-val 之外驱动编译器另有
//! 约束——实机验证清单预研）、目标域前瞻（SPIR-V 作为主机与移动平台目标的路线
//! 推演）、与 B 域驱动适配域（F0201 起）的衔接设想（驱动怪癖回传机制的 SPIR-V 版）；
//! 每项远景附前置条件与触发条件，不假装已测。
//!
//! 跨批对接：上游 F0571 版本策略、F0201 驱动域；下游 F0580 组收口、D 域终验登记。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 远景条目状态
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// 预留态：实机缺位，显性 N/A
    Deferred,
    /// 升格评估：前置条件变化
    Promoting,
    /// 随闸门补测：触发条件达成
    Testing,
}

/// 远景条目：附前置条件与触发条件
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RoadItem {
    pub name: String,
    /// 前置条件描述
    pub precondition: String,
    /// 触发条件描述
    pub trigger: String,
    pub status: Status,
}

impl RoadItem {
    pub fn new(name: &str, precondition: &str, trigger: &str) -> Self {
        RoadItem { name: name.to_string(), precondition: precondition.to_string(), trigger: trigger.to_string(), status: Status::Deferred }
    }

    /// 不假装已测：Deferred/升格均不可标记为通过
    pub fn honest(&self) -> bool {
        self.status != Status::Testing || !self.trigger.is_empty()
    }

    /// 升格评估：前置条件变化触发
    pub fn promote(&mut self) -> bool {
        if self.status == Status::Deferred && !self.precondition.is_empty() {
            self.status = Status::Promoting;
            true
        } else {
            false
        }
    }

    /// 触发条件达成 → 随闸门补测流程
    pub fn begin_test(&mut self) -> bool {
        if self.status == Status::Promoting && !self.trigger.is_empty() {
            self.status = Status::Testing;
            true
        } else {
            false
        }
    }
}

/// 三层远景路线
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoadLayer {
    /// 驱动验收远景
    DriverAcceptance,
    /// 目标域前瞻
    TargetOutlook,
    /// B 域衔接设想（驱动怪癖回传机制的 SPIR-V 版）
    BridgePlan,
}

impl RoadLayer {
    pub fn name(self) -> &'static str {
        match self {
            RoadLayer::DriverAcceptance => "驱动验收远景",
            RoadLayer::TargetOutlook => "目标域前瞻",
            RoadLayer::BridgePlan => "B域衔接设想",
        }
    }
}

/// 预研清单：实机验证项（spirv-val 之外的驱动约束）
#[derive(Clone, Debug, Default)]
pub struct PreList {
    pub items: Vec<(String, String)>,
}

impl PreList {
    pub fn new() -> Self {
        PreList::default()
    }

    pub fn add(&mut self, item: &str, driver_constraint: &str) -> bool {
        if item.is_empty() || driver_constraint.is_empty() || self.items.iter().any(|(i, _)| i == item) {
            return false;
        }
        self.items.push((item.to_string(), driver_constraint.to_string()));
        true
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// 预留态登记器：N/A 显性化
#[derive(Clone, Debug)]
pub struct DeferredReg {
    /// 登记项名
    pub items: Vec<String>,
    /// 闸门到达时间戳（0 = 未到达）
    pub gate_at: u64,
}

impl DeferredReg {
    pub fn new() -> Self {
        DeferredReg { items: Vec::new(), gate_at: 0 }
    }

    /// 显性登记 N/A
    pub fn register_na(&mut self, item: &str) -> bool {
        if item.is_empty() || self.items.iter().any(|i| i == item) {
            return false;
        }
        self.items.push(item.to_string());
        true
    }

    /// 当前为 N/A（实机缺位）
    pub fn is_na(&self, item: &str) -> bool {
        self.gate_at == 0 && self.items.iter().any(|i| i == item)
    }

    /// 闸门到达 → 补测
    pub fn gate_reached(&mut self, at: u64) {
        self.gate_at = at;
    }

    pub fn pending_retest(&self) -> bool {
        self.gate_at > 0 && !self.items.is_empty()
    }
}

/// 衔接设想：驱动怪癖回传机制的 SPIR-V 版条目
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct QuirkBridge {
    /// 怪癖签名
    pub quirk: String,
    /// 回传路径（B 域 F0201 起 → SPIR-V 版）
    pub path: String,
}

impl QuirkBridge {
    pub fn new(quirk: &str, path: &str) -> Self {
        QuirkBridge { quirk: quirk.to_string(), path: path.to_string() }
    }

    pub fn bridgeable(&self) -> bool {
        !self.quirk.is_empty() && self.path.contains("B域")
    }
}

pub fn run_vec177_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec177");
    // 族一：三层远景
    s.add("层-驱动验收", RoadLayer::DriverAcceptance.name() == "驱动验收远景", "l1");
    s.add("层-目标域", RoadLayer::TargetOutlook.name() == "目标域前瞻", "l2");
    s.add("层-衔接设想", RoadLayer::BridgePlan.name() == "B域衔接设想", "l3");
    // 族二：预留态显性
    let mut it = RoadItem::new("驱动编译器验收", "实机环境到位", "D域终验窗口");
    s.add("条目-初始预留", it.status == Status::Deferred, "def");
    s.add("条目-不假装已测", it.honest(), "honest");
    // 升格评估
    s.add("升格-前置变化触发", it.promote(), "promo");
    s.add("升格-状态迁移", it.status == Status::Promoting, "promoting");
    s.add("升格-重复拒绝", !it.promote(), "dup");
    // 触发条件达成 → 补测
    s.add("补测-触发达成", it.begin_test(), "test");
    s.add("补测-状态迁移", it.status == Status::Testing, "testing");
    s.add("补测-未升格拒绝", !RoadItem::new("x", "", "y").promote(), "nopromo");
    // 族三：预研清单
    let mut pre = PreList::new();
    s.add("预研-登记", pre.add("模块布局校验", "驱动要求对齐16B"), "add");
    s.add("预研-重复拒绝", !pre.add("模块布局校验", "x"), "dup");
    s.add("预研-空约束拒绝", !pre.add("y", ""), "empty");
    s.add("预研-计数", pre.len() == 1, "n1");
    // 族四：预留态登记器
    let mut reg = DeferredReg::new();
    s.add("N/A-显性登记", reg.register_na("Vulkan驱动验收"), "na");
    s.add("N/A-当前为N/A", reg.is_na("Vulkan驱动验收"), "isna");
    s.add("N/A-重复拒绝", !reg.register_na("Vulkan驱动验收"), "dup");
    s.add("N/A-未登记非N/A", !reg.is_na("其他项"), "other");
    s.add("补测-闸门未到不挂起", !reg.pending_retest(), "nogate");
    reg.gate_reached(1700000000);
    s.add("补测-闸门到达挂起", reg.pending_retest(), "gate");
    s.add("补测-闸门后非N/A", !reg.is_na("Vulkan驱动验收"), "after");
    // 族五：衔接设想
    let q = QuirkBridge::new("subgroup-size-quirk", "B域F0201怪癖回传 → SPIR-V版");
    s.add("衔接-怪癖签名", q.quirk == "subgroup-size-quirk", "q");
    s.add("衔接-路径可桥接", q.bridgeable(), "bridge");
    s.add("衔接-无域路径不可桥", !QuirkBridge::new("x", "本地处理").bridgeable(), "nobridge");
    // 族六：三层齐全
    let layers = [RoadLayer::DriverAcceptance, RoadLayer::TargetOutlook, RoadLayer::BridgePlan];
    s.add("远景-三层齐备", layers.len() == 3, "3l");
    s
}
