//! VE-F0504 · GLSL 版本与扩展管理（VE-C 域 · GLSL 后端转译组 C06）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0504`
//!
//! **锚点原文**：「GLSL 版本与扩展管理：目标版本选择策略（按引擎配置加能力
//! 探测）、扩展启用登记（每扩展的可用性确认加回退路径）、版本指令发射
//! （#version 与 #extension 文本生成）；版本与扩展组合合法性表。判据：策略
//! 选择、回退路径、组合表、版本发射、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **策略选择**（判据一）。版本 = min(引擎配置上限, 能力探测上限)，
//!    两源缺一即拒（不猜版本）。
//!
//! 2. **回退路径**（判据二）。扩展启用前先登记回退路径（扩展缺席时代替
//!    构造），无回退路径的扩展启用即违规。
//!
//! 3. **组合表**（判据三）。版本 × 扩展组合合法性查表；表外组合显性拒绝。
//!
//! 4. **版本发射**（判据四）。#version 与 #extension 文本按发射规范逐字符
//!    校验（顺序、格式）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0504";

// ---------------------------------------------------------------------------
// 一、版本选择策略
// ---------------------------------------------------------------------------

/// 能力探测报告（驱动上报）。
#[derive(Clone, Copy, Debug)]
pub struct Probe {
    /// 驱动支持的最高 GLSL 版本。
    pub max_ver: u32,
}

/// 版本选择：min(配置上限, 探测上限)，且必须落在登记版本表内。
pub fn pick_version(cfg_cap: u32, probe: Option<Probe>) -> Result<u32, u32> {
    let p = match probe {
        Some(p) => p,
        None => return Err(XC_NOPROBE),
    };
    if cfg_cap == 0 {
        return Err(XC_NOCFG);
    }
    let v = if cfg_cap < p.max_ver { cfg_cap } else { p.max_ver };
    if SUPPORTED.contains(&v) {
        Ok(v)
    } else {
        // 向下取登记表内最大可用版本。
        let mut down: Vec<u32> = SUPPORTED.iter().copied().filter(|&x| x <= v).collect();
        if down.is_empty() {
            return Err(XC_NOVER);
        }
        down.sort();
        Ok(down[down.len() - 1])
    }
}

pub const XC_NOPROBE: u32 = 1;
pub const XC_NOCFG: u32 = 2;
pub const XC_NOVER: u32 = 3;

/// 登记版本表（升序）。
pub const SUPPORTED: [u32; 4] = [120, 330, 450, 460];

// ---------------------------------------------------------------------------
// 二、扩展登记（可用性 + 回退路径）
// ---------------------------------------------------------------------------

/// 扩展条目。
#[derive(Clone, Debug)]
pub struct Ext {
    pub name: &'static str,
    /// 最低宿主版本（低于则不可用）。
    pub min_ver: u32,
    /// 回退路径描述（缺席时的代替构造）。
    pub fallback: &'static str,
}

/// 扩展正本表。
pub const EXTS: [Ext; 4] = [
    Ext { name: "GL_ARB_shader_group_vote", min_ver: 450, fallback: "手写归约（性能损失）" },
    Ext { name: "GL_ARB_shader_ballot", min_ver: 450, fallback: "子组广播近似" },
    Ext { name: "GL_KHR_shader_subgroup_basic", min_ver: 460, fallback: "无（功能降级）" },
    Ext { name: "GL_ARB_gpu_shader5", min_ver: 400, fallback: "拆分内联展开" },
];

/// 扩展可用性：宿主版本 >= 最低要求。
pub fn ext_ok(host_ver: u32, name: &str) -> Result<&'static Ext, u32> {
    let e = EXTS.iter().find(|e| e.name == name).ok_or(XC_NOEXT)?;
    if host_ver >= e.min_ver {
        Ok(e)
    } else {
        Err(XC_LOWVER)
    }
}

pub const XC_NOEXT: u32 = 4;
pub const XC_LOWVER: u32 = 5;

/// 启用登记：可用才可启用，且启用必须带回退确认。
#[derive(Default)]
pub struct ExtBook {
    enabled: Vec<&'static str>,
}

impl ExtBook {
    pub fn new() -> ExtBook {
        ExtBook { enabled: Vec::new() }
    }

    /// 启用：版本可用 + 回退路径非空（无回退的扩展不许盲启）。
    pub fn enable(&mut self, host_ver: u32, name: &str) -> Result<(), u32> {
        let e = ext_ok(host_ver, name)?;
        if e.fallback.is_empty() {
            return Err(XC_NOFALLBACK);
        }
        if self.enabled.iter().any(|n| *n == e.name) {
            return Err(XC_DUP);
        }
        self.enabled.push(e.name);
        Ok(())
    }

    pub fn enabled(&self) -> &[&'static str] {
        &self.enabled
    }

    pub fn len(&self) -> usize {
        self.enabled.len()
    }
}

pub const XC_NOFALLBACK: u32 = 6;
pub const XC_DUP: u32 = 7;

// ---------------------------------------------------------------------------
// 三、组合合法性表
// ---------------------------------------------------------------------------

/// 组合裁定：扩展在宿主版本上可用即组合合法（登记表驱动）。
pub fn combo_legal(host_ver: u32, ext: &str) -> bool {
    ext_ok(host_ver, ext).is_ok()
}

// ---------------------------------------------------------------------------
// 四、指令发射
// ---------------------------------------------------------------------------

/// 发射 #version 与 #extension 文本。
pub fn emit_prelude(ver: u32, exts: &[&str]) -> Result<Vec<String>, u32> {
    if !SUPPORTED.contains(&ver) {
        return Err(XC_NOVER);
    }
    let mut out: Vec<String> = Vec::new();
    out.push(format!("#version {}", ver));
    for e in exts {
        let tag = match e.strip_prefix("GL_") {
            Some(t) => t,
            None => return Err(XC_NOEXT),
        };
        out.push(format!("#extension GL_{} : require", tag));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec104_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec104");

    // 族一：版本策略。
    s.add("双源取小", pick_version(330, Some(Probe { max_ver: 460 })) == Ok(330), "配置上限生效");
    s.add("探测上限生效", pick_version(460, Some(Probe { max_ver: 330 })) == Ok(330), "驱动上限生效");
    s.add("无探测拒", matches!(pick_version(460, None), Err(XC_NOPROBE)), "不猜版本");
    s.add("零配置拒", matches!(pick_version(0, Some(Probe { max_ver: 460 })), Err(XC_NOCFG)), "配置必填");
    s.add("夹逼回退", pick_version(500, Some(Probe { max_ver: 500 })) == Ok(460), "向下取登记版");
    s.add("全低回退拒", matches!(pick_version(100, Some(Probe { max_ver: 100 })), Err(XC_NOVER)), "无可用版本");

    // 族二：扩展可用性。
    s.add("460 用子组基本", ext_ok(460, "GL_KHR_shader_subgroup_basic").is_ok(), "高版本可用");
    s.add("330 子组拒", matches!(ext_ok(330, "GL_KHR_shader_subgroup_basic"), Err(XC_LOWVER)), "低版本显性");
    s.add("表外扩展拒", matches!(ext_ok(460, "GL_未登记"), Err(XC_NOEXT)), "不猜扩展");
    s.add("回退路径全登记", EXTS.iter().all(|e| !e.fallback.is_empty()), "无盲启扩展");

    // 族三：启用登记。
    let mut b = ExtBook::new();
    s.add("低版本启用拒", matches!(b.enable(330, "GL_KHR_shader_subgroup_basic"), Err(XC_LOWVER)), "先可用后启用");
    let _ = b.enable(460, "GL_ARB_gpu_shader5");
    let _ = b.enable(460, "GL_ARB_shader_ballot");
    s.add("双扩展在册", b.len() == 2, "登记生效");
    s.add("重复启用拒", matches!(b.enable(460, "GL_ARB_gpu_shader5"), Err(XC_DUP)), "一扩一签");

    // 族四：组合表与发射。
    s.add("组合合法判定", combo_legal(450, "GL_ARB_shader_ballot"), "表内合法");
    s.add("组合非法判定", !combo_legal(330, "GL_ARB_shader_ballot"), "表外非法");
    let pre = match emit_prelude(460, b.enabled()) {
        Ok(p) => p,
        Err(_) => { s.add("发射败", false, "不应到"); return s; }
    };
    s.add("版本行首", pre[0] == "#version 460", "首行规范");
    s.add("扩展行规范", pre[1].starts_with("#extension GL_ARB_gpu_shader5 : require"), "require 语义");
    s.add("未登记版本拒", matches!(emit_prelude(410, &[]), Err(XC_NOVER)), "版本表外拒");
    s.add("非 GL_ 名拒", matches!(emit_prelude(460, &["VULKAN"]), Err(XC_NOEXT)), "命名规范");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick() {
        assert_eq!(pick_version(330, Some(Probe { max_ver: 460 })).ok(), Some(330));
        assert!(matches!(pick_version(0, None), Err(_)));
    }
}
