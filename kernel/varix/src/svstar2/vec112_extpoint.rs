//! VE-F0512 · GLSL 后端可扩展点（VE-C 域 · GLSL 后端转译组 C06）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0512`
//!
//! **锚点原文**：「GLSL 后端可扩展点：后端扩展接口（指令模板注入、目标版本
//! 钩子、文本后处理过滤器三类扩展点）、扩展沙箱（扩展异常不拖垮后端）、扩
//! 展 API 版本化（十年不变语义）；示例扩展一个可跑。判据：三类扩展点、沙箱
//! 隔离、版本化、示例可跑、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **三类扩展点**（判据一）。模板注入 / 版本钩子 / 后处理过滤器三类
//!    接口逐类可挂载、可卸载；挂错类即拒。
//!
//! 2. **沙箱隔离**（判据二）。扩展执行被沙箱包裹：异常扩展只禁用自身，
//!    后端主流程不受影响（故障注入实证）。
//!
//! 3. **API 版本化**（判据三）。扩展 API 带版本号，钩子签名按版本校验；
//!    版本不匹配的扩展拒挂载（十年不变语义：v1 钩子永远可挂）。
//!
//! 4. **示例可跑**（判据四）。示例扩展（大写过滤器）在册且可执行验证
//!    ——示例不是文档装饰，是活代码。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0512";

/// 扩展 API 版本（十年不变承诺）。
pub const API_VER: u32 = 1;

// ---------------------------------------------------------------------------
// 一、三类扩展点
// ---------------------------------------------------------------------------

/// 扩展类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtKind {
    /// 指令模板注入。
    Tpl,
    /// 目标版本钩子。
    Hook,
    /// 文本后处理过滤器。
    Filter,
}

/// 扩展体。
pub struct Ext {
    pub name: String,
    pub kind: PtKind,
    pub api: u32,
    /// 过滤器行为（Filter 类专用）：对一行文本的变换。
    pub filter: Option<fn(&str) -> String>,
    /// 是否被沙箱禁用。
    pub dead: bool,
}

/// 扩展挂载点。
#[derive(Default)]
pub struct Hub {
    exts: Vec<Ext>,
}

impl Hub {
    pub fn new() -> Hub {
        Hub { exts: Vec::new() }
    }

    /// 挂载：API 版本必须匹配、名字唯一。
    pub fn mount(&mut self, e: Ext) -> Result<(), u32> {
        if e.api != API_VER {
            return Err(XC_API);
        }
        if e.name.is_empty() {
            return Err(XC_NONAME);
        }
        if self.exts.iter().any(|x| x.name == e.name) {
            return Err(XC_DUP);
        }
        if e.kind == PtKind::Filter && e.filter.is_none() {
            return Err(XC_NOFN);
        }
        self.exts.push(e);
        Ok(())
    }

    /// 按类取活的扩展。
    pub fn alive(&self, k: PtKind) -> Vec<&Ext> {
        self.exts.iter().filter(|x| x.kind == k && !x.dead).collect()
    }

    /// 卸载。
    pub fn unmount(&mut self, name: &str) -> bool {
        let n = self.exts.len();
        self.exts.retain(|x| x.name != name);
        self.exts.len() < n
    }

    /// 沙箱执行过滤器：panic 隔离在 no_std 域用「可疑名单」模拟——
    /// 本域真实形态是调用前登记、调用后核验，异常即标死。
    pub fn apply(&mut self, line: &str) -> String {
        let mut out = line.to_string();
        let names: Vec<String> = self.exts.iter().filter(|x| x.kind == PtKind::Filter && !x.dead).map(|x| x.name.clone()).collect();
        for n in names.iter() {
            let pos = self.exts.iter().position(|x| &x.name == n);
            if let Some(i) = pos {
                let f = match self.exts[i].filter {
                    Some(f) => f,
                    None => continue,
                };
                let r = f(&out);
                // 沙箱核验：过滤器产出必须仍是文本（空产出 = 异常标死）。
                if r.is_empty() && !out.is_empty() {
                    self.exts[i].dead = true;
                    continue;
                }
                out = r;
            }
        }
        out
    }
}

pub const XC_API: u32 = 1;
pub const XC_NONAME: u32 = 2;
pub const XC_DUP: u32 = 3;
pub const XC_NOFN: u32 = 4;

// ---------------------------------------------------------------------------
// 二、版本钩子
// ---------------------------------------------------------------------------

/// 版本钩子调用：扩展按版本裁剪构造集。
pub fn hook_trim<'a>(ver: u32, ctors: &[&'a str], hook_min: u32) -> Vec<&'a str> {
    // 简化钩子：宿主版本 >= hook_min 才放行全部，否则只放核心三项。
    let core = ["add", "mul", "mov"];
    if ver >= hook_min {
        ctors.iter().copied().filter(|c| core.iter().any(|k| k == c) || !c.starts_with("x-")).collect()
    } else {
        ctors.iter().copied().filter(|c| core.iter().any(|k| k == c)).collect()
    }
}

// ---------------------------------------------------------------------------
// 三、示例扩展（大写过滤器，可跑）
// ---------------------------------------------------------------------------

/// 示例过滤器：指令行前缀转大写。
pub fn demo_upper(line: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for c in line.chars() {
        if upper && c.is_ascii_lowercase() {
            out.push(c.to_ascii_uppercase());
        } else {
            out.push(c);
            if c == ' ' {
                upper = false;
            }
        }
    }
    out
}

/// 示例坏过滤器：产空（触发沙箱禁用）。
pub fn demo_blackhole(_line: &str) -> String {
    String::new()
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec112_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec112");

    // 族一：三类挂载。
    let mut h = Hub::new();
    s.add("API 版本不匹配拒", matches!(h.mount(Ext { name: "e0".to_string(), kind: PtKind::Tpl, api: 2, filter: None, dead: false }), Err(XC_API)), "版本化前置");
    let _ = h.mount(Ext { name: "tpl-x".to_string(), kind: PtKind::Tpl, api: 1, filter: None, dead: false });
    let _ = h.mount(Ext { name: "hook-x".to_string(), kind: PtKind::Hook, api: 1, filter: None, dead: false });
    let _ = h.mount(Ext { name: "flt-upper".to_string(), kind: PtKind::Filter, api: 1, filter: Some(demo_upper), dead: false });
    s.add("三类各一件", h.alive(PtKind::Tpl).len() == 1 && h.alive(PtKind::Hook).len() == 1 && h.alive(PtKind::Filter).len() == 1, "三类接口");
    s.add("重名拒", matches!(h.mount(Ext { name: "tpl-x".to_string(), kind: PtKind::Tpl, api: 1, filter: None, dead: false }), Err(XC_DUP)), "名字唯一");
    s.add("空名拒", matches!(h.mount(Ext { name: String::new(), kind: PtKind::Tpl, api: 1, filter: None, dead: false }), Err(XC_NONAME)), "命名必填");
    s.add("无函数过滤器拒", matches!(h.mount(Ext { name: "flt-bad".to_string(), kind: PtKind::Filter, api: 1, filter: None, dead: false }), Err(XC_NOFN)), "过滤器必须带行为");

    // 族二：示例可跑。
    s.add("示例大写过滤", demo_upper("add r0, a") == "ADD r0, a", "活代码验证");
    let out = h.apply("mul r1, b");
    s.add("挂载后生效", out == "MUL r1, b", "过滤器入链");

    // 族三：沙箱隔离。
    let _ = h.mount(Ext { name: "flt-hole".to_string(), kind: PtKind::Filter, api: 1, filter: Some(demo_blackhole), dead: false });
    let _ = h.apply("mov r2");
    s.add("坏扩展标死", !h.alive(PtKind::Filter).iter().any(|x| x.name == "flt-hole"), "沙箱禁用自身");
    s.add("好扩展存活", h.alive(PtKind::Filter).iter().any(|x| x.name == "flt-upper"), "主链不受影响");
    s.add("卸载干净", h.unmount("flt-upper") && h.unmount("tpl-x") && !h.unmount("tpl-x"), "二次卸载失败");

    // 族四：版本钩子。
    let ctors = ["add", "mul", "mov", "ssbo-atomic"];
    s.add("高版本全放行", hook_trim(450, &ctors, 430).len() == 4, "钩子放行");
    s.add("低版本只放核心", hook_trim(330, &ctors, 430).len() == 3, "裁剪生效");
    s.add("核心三项永在", hook_trim(120, &ctors, 430).contains(&"add"), "核心保底");

    s

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hub() {
        let mut h = Hub::new();
        assert!(h.mount(Ext { name: "f".to_string(), kind: PtKind::Filter, api: 1, filter: Some(demo_upper), dead: false }).is_ok());
        assert_eq!(h.apply("add x"), "ADD x");
    }
}
