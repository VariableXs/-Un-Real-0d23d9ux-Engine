//! VE-F0569 · SPIR-V 对拍验证
//!
//! 产物正确性双重对拍——结构对拍（spirv-dis 反汇编文本与预期快照比对：指令
//! 序列、装饰、类型布局三段快照）；语义对拍（诚实声明：Vulkan 实机不可用，
//! 语义等价以 IR 层对拍 F0490 加反汇编结构全量比对替代，实机执行档登记随闸门
//! 补测）；与 F0506/F0529/F0548 家族差异：SPIR-V 无文本源，反汇编即对拍介质；
//! 白名单纪律：装饰序漂移可白名单，指令序失配一律阻断。
//!
//! 跨批对接：上游 F0565/F0566 生成、F0490 IR 优化对拍；下游 F0578 总对拍。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 三段快照
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Snapshot {
    /// 指令序列段
    pub insts: Vec<String>,
    /// 装饰段
    pub decors: Vec<String>,
    /// 类型布局段
    pub layout: Vec<String>,
}

impl Snapshot {
    pub fn new(insts: &[&str], decors: &[&str], layout: &[&str]) -> Self {
        Snapshot {
            insts: insts.iter().map(|s| s.to_string()).collect(),
            decors: decors.iter().map(|s| s.to_string()).collect(),
            layout: layout.iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// 对拍结果分级
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapVerdict {
    /// 三段全同
    Identical,
    /// 装饰序漂移（可白名单）
    DecorDrift,
    /// 指令序失配（一律阻断）
    InstMismatch,
    /// 布局失配（阻断）
    LayoutMismatch,
}

/// 三段快照比对：白名单纪律内置
pub fn compare_snap(expected: &Snapshot, actual: &Snapshot) -> SnapVerdict {
    if expected.insts != actual.insts {
        return SnapVerdict::InstMismatch;
    }
    if expected.layout != actual.layout {
        return SnapVerdict::LayoutMismatch;
    }
    if expected.decors != actual.decors {
        return SnapVerdict::DecorDrift;
    }
    SnapVerdict::Identical
}

/// 白名单：装饰序漂移登记后放行
#[derive(Clone, Debug, Default)]
pub struct DriftWhitelist {
    entries: Vec<String>,
}

impl DriftWhitelist {
    pub fn new() -> Self {
        DriftWhitelist::default()
    }

    pub fn register(&mut self, variant: &str) -> bool {
        if variant.is_empty() || self.entries.iter().any(|e| e == variant) {
            return false;
        }
        self.entries.push(variant.to_string());
        true
    }

    pub fn allows(&self, variant: &str) -> bool {
        self.entries.iter().any(|e| e == variant)
    }
}

/// 对拍结论
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dispo {
    Pass,
    Whitelisted,
    Blocked,
}

/// 处置：快照判定 + 白名单 → 结论
pub fn dispose(v: SnapVerdict, variant: &str, wl: &DriftWhitelist) -> Dispo {
    match v {
        SnapVerdict::Identical => Dispo::Pass,
        SnapVerdict::DecorDrift => {
            if wl.allows(variant) {
                Dispo::Whitelisted
            } else {
                Dispo::Blocked
            }
        }
        _ => Dispo::Blocked,
    }
}

/// 反汇编降级：工具缺席 → 二进制哈希比对
pub fn hash_compare(a: &[u32], b: &[u32]) -> bool {
    a == b
}

/// 语义对拍诚实声明
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SemanticPlan {
    /// 替代路径：IR 层对拍 + 反汇编结构全量比对
    IrPlusStruct,
    /// 实机执行档：随闸门登记后补测
    OnDeviceDeferred,
}

/// 实机闸门
#[derive(Clone, Debug)]
pub struct DeviceGate {
    pub device_ready: bool,
    pub registered: bool,
}

impl DeviceGate {
    pub fn plan(&self) -> SemanticPlan {
        if self.device_ready && self.registered {
            SemanticPlan::OnDeviceDeferred
        } else {
            SemanticPlan::IrPlusStruct
        }
    }

    pub fn deferred_pending(&self) -> bool {
        !self.device_ready && !self.registered
    }
}

/// 对拍记录册
#[derive(Clone, Debug, Default)]
pub struct DiffBook {
    records: Vec<(String, Dispo)>,
}

impl DiffBook {
    pub fn new() -> Self {
        DiffBook::default()
    }

    pub fn record(&mut self, variant: &str, d: Dispo) {
        self.records.push((variant.to_string(), d));
    }

    pub fn blocked(&self) -> Vec<String> {
        self.records.iter().filter(|(_, d)| *d == Dispo::Blocked).map(|(v, _)| v.clone()).collect()
    }

    pub fn all_passed(&self) -> bool {
        self.records.iter().all(|(_, d)| *d != Dispo::Blocked)
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

pub fn run_vec169_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec169");
    // 族一：三段快照比对
    let a = Snapshot::new(&["OpCapability", "OpMemoryModel"], &["OpDecorate 1"], &["OpTypeFloat"]);
    s.add("快照-全同", compare_snap(&a, &a) == SnapVerdict::Identical, "same");
    let b = Snapshot::new(&["OpCapability", "OpMemoryModel"], &["OpDecorate 2"], &["OpTypeFloat"]);
    s.add("快照-装饰漂移", compare_snap(&a, &b) == SnapVerdict::DecorDrift, "drift");
    let c = Snapshot::new(&["OpCapability", "OpReturn"], &["OpDecorate 1"], &["OpTypeFloat"]);
    s.add("快照-指令序失配优先", compare_snap(&a, &c) == SnapVerdict::InstMismatch, "inst");
    let d = Snapshot::new(&["OpCapability", "OpMemoryModel"], &["OpDecorate 1"], &["OpTypeInt"]);
    s.add("快照-布局失配", compare_snap(&a, &d) == SnapVerdict::LayoutMismatch, "layout");
    // 族二：白名单纪律
    let mut wl = DriftWhitelist::new();
    s.add("白名单-登记", wl.register("v1-decor-order"), "reg");
    s.add("白名单-重复拒绝", !wl.register("v1-decor-order"), "dup");
    s.add("白名单-查询", wl.allows("v1-decor-order") && !wl.allows("other"), "q");
    // 族三：处置分级
    s.add("处置-全同通过", dispose(SnapVerdict::Identical, "v1", &wl) == Dispo::Pass, "pass");
    s.add("处置-漂移白名单放行", dispose(SnapVerdict::DecorDrift, "v1", &wl) == Dispo::Whitelisted, "wl");
    s.add("处置-漂移未登记阻断", dispose(SnapVerdict::DecorDrift, "zz", &wl) == Dispo::Blocked, "blk");
    s.add("处置-指令序失配一律阻断", dispose(SnapVerdict::InstMismatch, "v1", &wl) == Dispo::Blocked, "instblk");
    s.add("处置-布局失配阻断", dispose(SnapVerdict::LayoutMismatch, "v1", &wl) == Dispo::Blocked, "layblk");
    // 族四：反汇编降级哈希比对
    s.add("降级-哈希一致", hash_compare(&[1, 2, 3], &[1, 2, 3]), "h-ok");
    s.add("降级-哈希失配", !hash_compare(&[1, 2, 3], &[1, 2, 4]), "h-bad");
    // 族五：语义对拍诚实声明
    let g_off = DeviceGate { device_ready: false, registered: false };
    s.add("语义-无实机走替代路径", g_off.plan() == SemanticPlan::IrPlusStruct, "alt");
    s.add("语义-替代路径显性登记", g_off.deferred_pending(), "pend");
    let g_on = DeviceGate { device_ready: true, registered: true };
    s.add("语义-实机档随闸门", g_on.plan() == SemanticPlan::OnDeviceDeferred, "dev");
    let g_half = DeviceGate { device_ready: true, registered: false };
    s.add("语义-未登记仍替代", g_half.plan() == SemanticPlan::IrPlusStruct, "half");
    // 族六：对拍记录册
    let mut book = DiffBook::new();
    book.record("v1", Dispo::Pass);
    book.record("v2", Dispo::Whitelisted);
    s.add("记录-通过判定", book.all_passed() && book.len() == 2, "ok");
    book.record("v3", Dispo::Blocked);
    s.add("记录-阻断检出", !book.all_passed() && book.blocked() == vec!["v3".to_string()], "blk");
    s
}
