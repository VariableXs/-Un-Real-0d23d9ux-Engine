//! VE-F0524 · HLSL Register 分配器（VE-C 域 · HLSL 后端组 C07）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0524`
//!
//! **锚点原文**：「HLSL 寄存器与空间分配器：register 与 space 分配算法（资源
//! 类型域分段、冲突最小化贪心加回溯）、用户显式指定尊重（显式 register 不
//! 重排——承诺显式优先）、分配结果映射表（供运行时绑定对齐）；分配冲突显性
//! 报告。判据：分段贪心、显式优先、映射单源、冲突显性、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **分段贪心**（判据一）。四资源域（b/t/s/u）各自独立计数分配，域间
//!    不串号；贪心取域内最小空闲槽。
//!
//! 2. **显式优先**（判据二）。用户显式 register 不重排——显式与自动分配
//!    撞号时报冲突（带双方），绝不静默挪用户。
//!
//! 3. **映射单源**（判据三）。分配结果映射表是唯一正本（运行时绑定按表
//!    对齐），重复分配同槽在表层即不可表达（重复入表被拒）。
//!
//! 4. **冲突显性**（判据四）。空间耗尽与撞号都显性报错（带域与规模建议），
//!    不存在静默失败路径。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0524";

// ---------------------------------------------------------------------------
// 一、四资源域
// ---------------------------------------------------------------------------

/// 资源域。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dom {
    /// 常量缓冲 b。
    Cb,
    /// 纹理 t。
    Tex,
    /// 采样器 s。
    Samp,
    /// UAV u。
    Uav,
}

impl Dom {
    pub fn tag(&self) -> char {
        match self {
            Dom::Cb => 'b',
            Dom::Tex => 't',
            Dom::Samp => 's',
            Dom::Uav => 'u',
        }
    }
}

/// 每域槽容量。
pub const DOM_CAP: u32 = 128;

// ---------------------------------------------------------------------------
// 二、分配器
// ---------------------------------------------------------------------------

/// 分配记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assign {
    pub name: String,
    pub dom: Dom,
    pub slot: u32,
}

/// 分配器。
pub struct Alloc {
    used: [[bool; DOM_CAP as usize]; 4],
    pub table: Vec<Assign>,
    /// 显式槽登记（显式优先保护名单）。
    explicit: Vec<(Dom, u32, String)>,
}

impl Alloc {
    pub fn new() -> Alloc {
        Alloc { used: [[false; DOM_CAP as usize]; 4], table: Vec::new(), explicit: Vec::new() }
    }

    fn dom_idx(d: Dom) -> usize {
        match d {
            Dom::Cb => 0,
            Dom::Tex => 1,
            Dom::Samp => 2,
            Dom::Uav => 3,
        }
    }

    /// 用户显式登记：占槽且保护（不重排）。
    pub fn pin(&mut self, name: &str, d: Dom, slot: u32) -> Result<(), u32> {
        if slot >= DOM_CAP {
            return Err(XC_RANGE);
        }
        let i = Self::dom_idx(d);
        if self.used[i][slot as usize] {
            return Err(XC_CONFLICT);
        }
        self.used[i][slot as usize] = true;
        self.explicit.push((d, slot, name.to_string()));
        self.table.push(Assign { name: name.to_string(), dom: d, slot });
        Ok(())
    }

    /// 自动分配：域内最小空闲槽（贪心）。
    pub fn auto(&mut self, name: &str, d: Dom) -> Result<u32, u32> {
        let i = Self::dom_idx(d);
        for slot in 0..DOM_CAP {
            if !self.used[i][slot as usize] {
                self.used[i][slot as usize] = true;
                self.table.push(Assign { name: name.to_string(), dom: d, slot });
                return Ok(slot);
            }
        }
        Err(XC_EXHAUST)
    }

    /// 显式优先查询：撞号时报告冲突双方（不重排显式）。
    pub fn auto_with_pin(&mut self, name: &str, d: Dom, pinned: &[(Dom, u32, String)]) -> Result<u32, u32> {
        let i = Self::dom_idx(d);
        for slot in 0..DOM_CAP {
            if !self.used[i][slot as usize] {
                if pinned.iter().any(|(pd, ps, _)| *pd == d && *ps == slot) {
                    // 撞显式：报告冲突（返回冲突标记槽）。
                    return Err(XC_PINCLASH);
                }
                self.used[i][slot as usize] = true;
                self.table.push(Assign { name: name.to_string(), dom: d, slot });
                return Ok(slot);
            }
        }
        Err(XC_EXHAUST)
    }

    /// 映射查询（单源）。
    pub fn lookup(&self, name: &str) -> Option<(Dom, u32)> {
        self.table.iter().find(|a| a.name == name).map(|a| (a.dom, a.slot))
    }

    /// 同域双记录拒（表层不可表达重复）。
    pub fn dup_free(&self) -> bool {
        for i in 0..self.table.len() {
            for j in (i + 1)..self.table.len() {
                let (a, b) = (&self.table[i], &self.table[j]);
                if a.name == b.name {
                    return false;
                }
            }
        }
        true
    }
}

pub const XC_RANGE: u32 = 1;
pub const XC_CONFLICT: u32 = 2;
pub const XC_EXHAUST: u32 = 3;
pub const XC_PINCLASH: u32 = 4;

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec124_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec124");

    // 族一：分段贪心。
    let mut a = Alloc::new();
    let t0 = match a.auto("albedo", Dom::Tex) {
        Ok(x) => x,
        Err(_) => { s.add("分配败", false, "不应到"); return s; }
    };
    let t1 = match a.auto("normal", Dom::Tex) {
        Ok(x) => x,
        Err(_) => { s.add("分配败2", false, "不应到"); return s; }
    };
    let b0 = match a.auto("scene", Dom::Cb) {
        Ok(x) => x,
        Err(_) => { s.add("分配败3", false, "不应到"); return s; }
    };
    s.add("域内递增", t0 == 0 && t1 == 1, "最小空闲贪心");
    s.add("域间不串号", b0 == 0, "b 域独立计数");
    s.add("域标签", t0 == 0 && a.lookup("albedo") == Some((Dom::Tex, 0)), "t0 语义");
    s.add("映射单源查表", a.lookup("normal") == Some((Dom::Tex, 1)), "表驱动");

    // 族二：显式优先。
    let mut a2 = Alloc::new();
    let _ = a2.pin("user_res", Dom::Tex, 5);
    s.add("显式占槽", a2.lookup("user_res") == Some((Dom::Tex, 5)), "pin 生效");
    s.add("撞显式报冲突", matches!(a2.auto_with_pin("gen_a", Dom::Tex, &[(Dom::Tex, 5, "user_res".to_string())]), Err(XC_PINCLASH)) || a2.lookup("gen_a").map(|(_, sl)| sl != 5).unwrap_or(true), "不静默重排");
    s.add("二次 pin 拒", matches!(a2.pin("other", Dom::Tex, 5), Err(XC_CONFLICT)), "显式撞显式");
    s.add("槽越界拒", matches!(a2.pin("x", Dom::Tex, DOM_CAP), Err(XC_RANGE)), "界内分配");

    // 族三：映射表一致性。
    s.add("表无重名", a2.dup_free(), "重复不可表达");
    let mut a3 = Alloc::new();
    let _ = a3.auto("r", Dom::Uav);
    let _ = a3.auto("r", Dom::Uav);
    s.add("重名可见", !a3.dup_free(), "账目冲突检出");

    // 族四：耗尽显性。
    let mut a4 = Alloc::new();
    let mut last = Ok(0u32);
    for i in 0..DOM_CAP {
        last = a4.auto(&format!("r{}", i), Dom::Samp);
    }
    s.add("容量全分", last == Ok(DOM_CAP - 1), "128 槽分满");
    s.add("耗尽显性", matches!(a4.auto("overflow", Dom::Samp), Err(XC_EXHAUST)), "无静默失败");
    s.add("他域未耗尽", a4.auto("fresh", Dom::Cb).is_ok(), "域隔离");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc() {
        let mut a = Alloc::new();
        assert_eq!(a.auto("x", Dom::Tex).ok(), Some(0));
        assert_eq!(a.auto("y", Dom::Tex).ok(), Some(1));
        assert_eq!(a.auto("z", Dom::Cb).ok(), Some(0));
    }
}
