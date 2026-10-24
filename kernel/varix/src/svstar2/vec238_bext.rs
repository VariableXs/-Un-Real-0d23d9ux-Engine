//! VE-F0638 自定义混合模式扩展点（VE-D 域 · 目标 320 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0638`
//!
//! **判据（锚点原文逐条）**：
//! - **双形态注册**：受限公式 DSL（纯像素函数）或宿主函数句柄；
//! - **命名空间隔离**：自定义模式与内置 18 语义并存不冲突；
//! - **成本告警**：每像素回调成本超阈值 → 建议改走内置近似；
//! - **责任声明**：自定义模式不进金标准对拍（无 1 LSB 保障），声明写在注册回执；
//! - **判据**：非法公式编译期拒绝三要素；沙箱逃逸拒绝并登记。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::ved21_blendreg::ModeKey;

/// 受限公式 DSL 节点（纯像素函数：无副作用无外部访问）
#[derive(Clone, Debug, PartialEq)]
pub enum DslNode {
    /// 常数
    Const(f32),
    /// 源色通道（0..3）
    Src(usize),
    /// 背景色通道（0..3）
    Bg(usize),
    /// 加 / 减 / 乘
    Add(alloc::boxed::Box<DslNode>, alloc::boxed::Box<DslNode>),
    Sub(alloc::boxed::Box<DslNode>, alloc::boxed::Box<DslNode>),
    Mul(alloc::boxed::Box<DslNode>, alloc::boxed::Box<DslNode>),
    /// 钳到 [0,1]
    Clamp01(alloc::boxed::Box<DslNode>),
}

pub const DSL_MAX_NODES: usize = 32;
pub const CHANNEL_MAX: usize = 4;

impl DslNode {
    /// 编译期校验三要素：类型合法 / 通道越界 / 节点数超限
    pub fn validate(&self, budget: &mut usize) -> Result<(), &'static str> {
        *budget = budget.saturating_add(1);
        if *budget > DSL_MAX_NODES {
            return Err("dsl too large");
        }
        match self {
            DslNode::Const(_) => Ok(()),
            DslNode::Src(c) | DslNode::Bg(c) => {
                if *c >= CHANNEL_MAX {
                    Err("channel out of range")
                } else {
                    Ok(())
                }
            }
            DslNode::Add(a, b) | DslNode::Sub(a, b) | DslNode::Mul(a, b) => {
                a.validate(budget)?;
                b.validate(budget)
            }
            DslNode::Clamp01(a) => a.validate(budget),
        }
    }

    /// 解释求值（O(节点数) 每像素最坏）
    pub fn eval(&self, src: &[f32; 4], bg: &[f32; 4]) -> f32 {
        match self {
            DslNode::Const(v) => *v,
            DslNode::Src(c) => src[*c],
            DslNode::Bg(c) => bg[*c],
            DslNode::Add(a, b) => a.eval(src, bg) + b.eval(src, bg),
            DslNode::Sub(a, b) => a.eval(src, bg) - b.eval(src, bg),
            DslNode::Mul(a, b) => a.eval(src, bg) * b.eval(src, bg),
            DslNode::Clamp01(a) => a.eval(src, bg).clamp(0.0, 1.0),
        }
    }

    /// 节点计数（成本模型输入）
    pub fn nodes(&self) -> usize {
        match self {
            DslNode::Const(_) | DslNode::Src(_) | DslNode::Bg(_) => 1,
            DslNode::Add(a, b) | DslNode::Sub(a, b) | DslNode::Mul(a, b) => 1 + a.nodes() + b.nodes(),
            DslNode::Clamp01(a) => 1 + a.nodes(),
        }
    }
}

/// 宿主函数句柄（沙箱接口：无副作用纯函数指针）
pub type HostFn = fn(&[f32; 4], &[f32; 4]) -> f32;

/// 自定义模式定义（双形态之一）
#[derive(Clone, Debug)]
pub enum CustomDef {
    Dsl(DslNode),
    Host(HostFn),
}

/// 注册回执（责任声明在此）
#[derive(Clone, Debug, PartialEq)]
pub struct CustomReceipt {
    pub name: &'static str,
    /// 每像素成本档（节点数或句柄标记）
    pub cost: u32,
    /// 责任声明：不进金标准对拍，质量责任在注册方
    pub disclaimer: &'static str,
}

pub const DISCLAIMER: &str = "no gold-standard audit; quality owned by registrar";

/// 每像素成本告警阈值（节点数）
pub const COST_WARN_NODES: u32 = 16;

/// 自定义注册表（命名空间隔离：自定义名一律带 "user:" 前缀）
#[derive(Clone, Debug, Default)]
pub struct CustomRegistry {
    pub defs: Vec<(&'static str, CustomDef)>,
    pub receipts: Vec<CustomReceipt>,
    /// 沙箱逃逸尝试登记
    pub escapes: Vec<&'static str>,
}

impl CustomRegistry {
    pub fn new() -> CustomRegistry {
        CustomRegistry::default()
    }

    /// 注册 DSL 形态：编译期拒绝非法公式
    pub fn register_dsl(&mut self, name: &'static str, n: DslNode) -> Result<CustomReceipt, &'static str> {
        if !name.starts_with("user:") {
            return Err("custom must be namespaced user:");
        }
        if self.defs.iter().any(|(k, _)| *k == name) {
            return Err("duplicate custom name");
        }
        let mut budget = 0usize;
        n.validate(&mut budget)?;
        let cost = n.nodes() as u32;
        let r = CustomReceipt { name, cost, disclaimer: DISCLAIMER };
        self.defs.push((name, CustomDef::Dsl(n)));
        self.receipts.push(r.clone());
        Ok(r)
    }

    /// 注册句柄形态：O(1) 调用
    pub fn register_host(&mut self, name: &'static str, f: HostFn) -> Result<CustomReceipt, &'static str> {
        if !name.starts_with("user:") {
            return Err("custom must be namespaced user:");
        }
        if self.defs.iter().any(|(k, _)| *k == name) {
            return Err("duplicate custom name");
        }
        let r = CustomReceipt { name, cost: 1, disclaimer: DISCLAIMER };
        self.defs.push((name, CustomDef::Host(f)));
        self.receipts.push(r.clone());
        Ok(r)
    }

    /// 求值（注册的才可用）
    pub fn eval(&self, name: &str, src: &[f32; 4], bg: &[f32; 4]) -> Option<f32> {
        self.defs.iter().find(|(k, _)| *k == name).map(|(_, d)| match d {
            CustomDef::Dsl(n) => n.eval(src, bg).clamp(0.0, 1.0),
            CustomDef::Host(f) => f(src, bg).clamp(0.0, 1.0),
        })
    }

    /// 成本告警：超阈值建议改走内置近似
    pub fn cost_warn(&self, name: &str) -> bool {
        self.receipts
            .iter()
            .find(|r| r.name == name)
            .map(|r| r.cost >= COST_WARN_NODES)
            .unwrap_or(false)
    }

    /// 与内置命名空间不冲突（内置 18 键不带前缀）
    pub fn no_builtin_conflict(&self) -> bool {
        use ModeKey::*;
        let builtins = [
            "normal", "multiply", "screen", "overlay", "darken", "lighten",
            "color-dodge", "color-burn", "hard-light", "soft-light",
            "difference", "exclusion", "hue", "saturation", "color",
            "luminosity", "plus-darker", "plus-lighter",
        ];
        self.defs.iter().all(|(k, _)| {
            !builtins.iter().any(|b| k == b) && k.starts_with("user:")
        })
    }
}

pub fn run_vec238_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec238");
    // 族一：DSL 编译期校验三要素
    let ok = DslNode::Clamp01(alloc::boxed::Box::new(DslNode::Add(
        alloc::boxed::Box::new(DslNode::Src(0)),
        alloc::boxed::Box::new(DslNode::Bg(0)),
    )));
    let mut b = 0usize;
    s.add("扩展-合法公式", ok.validate(&mut b).is_ok(), "valid dsl");
    let bad_ch = DslNode::Src(7);
    let mut b2 = 0usize;
    s.add("扩展-越界拒绝", bad_ch.validate(&mut b2) == Err("channel out of range"), "chan oob");
    let big = DslNode::Add(
        alloc::boxed::Box::new(DslNode::Mul(
            alloc::boxed::Box::new(DslNode::Src(0)),
            alloc::boxed::Box::new(DslNode::Bg(1)),
        )),
        alloc::boxed::Box::new(DslNode::Const(0.5)),
    );
    s.add("扩展-节点计数", big.nodes() == 5, "5 nodes");
    let mut b3 = DSL_MAX_NODES;
    s.add("扩展-超限拒绝", big.validate(&mut b3) == Err("dsl too large"), "too large");
    // 族二：注册（命名空间）
    let mut reg = CustomRegistry::new();
    let rc = reg.register_dsl("user:glowish", ok.clone()).expect("reg");
    s.add("扩展-DSL注册", rc.name == "user:glowish", "dsl reg");
    s.add("扩展-责任声明", rc.disclaimer == DISCLAIMER, "disclaimer in receipt");
    s.add("扩展-重名拒", reg.register_dsl("user:glowish", ok.clone()) == Err("duplicate custom name"), "dup");
    s.add("扩展-无前缀拒", reg.register_dsl("multiply", ok.clone()) == Err("custom must be namespaced user:"), "no ns");
    // 族三：句柄形态
    fn h(src: &[f32; 4], bg: &[f32; 4]) -> f32 {
        (src[0] + bg[0]) * 0.5
    }
    let rh = reg.register_host("user:avg", h).expect("host");
    s.add("扩展-句柄注册", rh.cost == 1, "host cost 1");
    // 族四：求值
    let src = [0.8f32, 0.0, 0.0, 1.0];
    let bg = [0.2f32, 0.0, 0.0, 1.0];
    s.add("扩展-DSL求值", reg.eval("user:glowish", &src, &bg).expect("e") == 1.0, "dsl eval clamp");
    s.add("扩展-句柄求值", reg.eval("user:avg", &src, &bg).expect("e") == 0.5, "host eval");
    s.add("扩展-未注册", reg.eval("user:ghost", &src, &bg).is_none(), "missing");
    // 族五：成本告警
    let deep = DslNode::Mul(
        alloc::boxed::Box::new(big.clone()),
        alloc::boxed::Box::new(big),
    );
    let _ = reg.register_dsl("user:heavy", deep).expect("heavy");
    s.add("扩展-成本告警", reg.cost_warn("user:heavy"), "cost warn");
    s.add("扩展-轻不告", !reg.cost_warn("user:avg"), "light ok");
    // 族六：命名空间隔离
    s.add("扩展-无内置冲突", reg.no_builtin_conflict(), "ns clean");
    s
}
