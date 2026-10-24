//! VE-F0539 · HLSL 与 Windows 域协作协议（VE-C 域 · HLSL 后端组 C07）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0539`
//!
//! **锚点原文**：「HLSL 后端与 Windows 图形域的协作协议：绑定模型对齐协议
//! （F0524 分配映射表到运行时绑定的一致性契约）、编译产物交换格式约定、实机
//! 验证协作流程（虚机判据外的真机验证登记随闸门语义复用）；协议版本化双签。
//! 判据：三节协议、双签版本、真机随闸门、双侧登记、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **三节协议**（判据一）。对齐（分配表 ↔ 运行时绑定）/ 交换（产物格式
//!    约定）/ 验证（真机流程）三节逐节有内容。
//!
//! 2. **双签版本**（判据二）。协议版本唯一递增且变更需双侧签署（本域签 +
//!    Windows 域签），单签协议无效。
//!
//! 3. **真机随闸门**（判据三）。真机验证未登记不冒充通过（随闸门显性）。
//!
//! 4. **双侧登记**（判据四）。协议违反双侧登记整改（不单方销账）；交换格式
//!    不匹配拒绝并提示版本。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0539";

// ---------------------------------------------------------------------------
// 一、三节协议
// ---------------------------------------------------------------------------

/// 协议节。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sec {
    Align,
    Exchange,
    Verify,
}

/// 协议条目。
#[derive(Clone, Debug)]
pub struct Item {
    pub sec: Sec,
    pub text: String,
}

/// 协议账。
#[derive(Default)]
pub struct Proto {
    pub items: Vec<Item>,
}

impl Proto {
    pub fn new() -> Proto {
        Proto { items: Vec::new() }
    }

    pub fn add(&mut self, sec: Sec, text: &str) -> Result<(), u32> {
        if text.is_empty() {
            return Err(XC_EMPTY);
        }
        self.items.push(Item { sec, text: text.to_string() });
        Ok(())
    }

    pub fn complete(&self) -> bool {
        let has = |s: Sec| self.items.iter().any(|i| i.sec == s);
        has(Sec::Align) && has(Sec::Exchange) && has(Sec::Verify)
    }
}

pub const XC_EMPTY: u32 = 1;

// ---------------------------------------------------------------------------
// 二、绑定对齐契约（F0524 映射表 ↔ 运行时绑定）
// ---------------------------------------------------------------------------

/// 绑定记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bind {
    pub name: String,
    pub dom: char,
    pub slot: u32,
}

/// 对齐核对：分配表与运行时绑定逐项一致。
pub fn align_check(alloc: &[Bind], runtime: &[Bind]) -> Result<(), u32> {
    if alloc.len() != runtime.len() {
        return Err(XC_COUNT);
    }
    for a in alloc.iter() {
        match runtime.iter().find(|r| r.name == a.name) {
            None => return Err(XC_MISSING),
            Some(r) => {
                if r.dom != a.dom || r.slot != a.slot {
                    return Err(XC_MISMATCH);
                }
            }
        }
    }
    Ok(())
}

pub const XC_COUNT: u32 = 2;
pub const XC_MISSING: u32 = 3;
pub const XC_MISMATCH: u32 = 4;

// ---------------------------------------------------------------------------
// 三、交换格式约定
// ---------------------------------------------------------------------------

/// 交换产物头（魔数 + 协议版本）。
pub const MAGIC: u32 = 0x56455850; // "VEXP"

/// 交换头校验。
pub fn exchange_ok(magic: u32, proto_ver: u32, expect_ver: u32) -> Result<(), u32> {
    if magic != MAGIC {
        return Err(XC_MAGIC);
    }
    if proto_ver != expect_ver {
        return Err(XC_VER);
    }
    Ok(())
}

pub const XC_MAGIC: u32 = 5;
pub const XC_VER: u32 = 6;

// ---------------------------------------------------------------------------
// 四、真机随闸门与双签版本
// ---------------------------------------------------------------------------

/// 闸门账。
#[derive(Default)]
pub struct GateBook {
    pub regs: Vec<(String, bool)>,
}

impl GateBook {
    pub fn new() -> GateBook {
        GateBook { regs: Vec::new() }
    }

    pub fn pend(&mut self, case: &str) {
        if !self.regs.iter().any(|(c, _)| c == case) {
            self.regs.push((case.to_string(), false));
        }
    }

    pub fn clear(&mut self, case: &str) -> bool {
        match self.regs.iter_mut().find(|(c, _)| c == case) {
            Some(r) => {
                r.1 = true;
                true
            }
            None => false,
        }
    }

    pub fn pending(&self) -> usize {
        self.regs.iter().filter(|(_, g)| !g).count()
    }

    /// 不冒充通过：未过闸门 = 待验证。
    pub fn claimed_ok(&self, case: &str) -> bool {
        self.regs.iter().any(|(c, g)| c == case && *g)
    }
}

/// 协议版本双签。
pub struct ProtoSig {
    pub ver: u32,
    /// 本域签 + Windows 域签。
    pub sig_local: bool,
    pub sig_remote: bool,
}

impl ProtoSig {
    pub fn valid(&self) -> bool {
        self.ver > 0 && self.sig_local && self.sig_remote
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec139_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec139");

    // 族一：三节协议。
    let mut p = Proto::new();
    s.add("空条目拒", matches!(p.add(Sec::Align, ""), Err(XC_EMPTY)), "内容必填");
    let _ = p.add(Sec::Align, "分配表逐项对齐运行时绑定");
    let _ = p.add(Sec::Exchange, "产物 VEXP 头 + 协议版本");
    let _ = p.add(Sec::Verify, "真机验证随闸门登记");
    s.add("三节齐", p.complete(), "对齐/交换/验证");

    // 族二：绑定对齐契约。
    let alloc = vec![
        Bind { name: "t".to_string(), dom: 't', slot: 0 },
        Bind { name: "b".to_string(), dom: 'b', slot: 1 },
    ];
    s.add("对齐通过", align_check(&alloc, &alloc).is_ok(), "逐项一致");
    let mut rt = alloc.clone();
    rt[1].slot = 2;
    s.add("槽位漂移拦", matches!(align_check(&alloc, &rt), Err(XC_MISMATCH)), "双侧登记整改");
    s.add("缺绑拦", matches!(align_check(&alloc, &alloc[..1]), Err(XC_COUNT)), "数量契约");
    s.add("未知资源拦", {
        let rt2 = vec![Bind { name: "t".to_string(), dom: 't', slot: 0 }, Bind { name: "z".to_string(), dom: 'b', slot: 1 }];
        matches!(align_check(&alloc, &rt2), Err(XC_MISSING))
    }, "双侧定位");

    // 族三：交换格式。
    s.add("魔数正确", exchange_ok(MAGIC, 1, 1).is_ok(), "VEXP 头");
    s.add("魔数错拒", matches!(exchange_ok(0x1234, 1, 1), Err(XC_MAGIC)), "拒绝交换");
    s.add("版本不符拒", matches!(exchange_ok(MAGIC, 2, 1), Err(XC_VER)), "提示版本");

    // 族四：真机随闸门与双签。
    let mut g = GateBook::new();
    g.pend("sm66-wave");
    s.add("未验不冒充", !g.claimed_ok("sm66-wave") && g.pending() == 1, "随闸门显性");
    s.add("过闸后可宣称", g.clear("sm66-wave") && g.claimed_ok("sm66-wave"), "实机补齐");
    s.add("未知清不动", !g.clear("zz"), "账目严谨");
    let sig_bad = ProtoSig { ver: 1, sig_local: true, sig_remote: false };
    let sig_ok = ProtoSig { ver: 2, sig_local: true, sig_remote: true };
    s.add("单签无效", !sig_bad.valid(), "双签前置");
    s.add("双签有效", sig_ok.valid(), "协议生效");
    s.add("零版本无效", !ProtoSig { ver: 0, sig_local: true, sig_remote: true }.valid(), "版本必填");

    s

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn align() {
        let a = vec![Bind { name: "x".to_string(), dom: 't', slot: 0 }];
        assert!(align_check(&a, &a).is_ok());
    }
}
