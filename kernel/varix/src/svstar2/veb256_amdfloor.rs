//! VE-F0256 · AMD 支持下限声明（VE-B 域 · AMD 组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0256`
//!
//! **职责定位（锚点原文）**：声明最低支持族（GFX9 起）与理由（测试成本
//! 与特性依赖），下限以下显性拒绝三要素；边界族实验档标注；调整走宣告
//! 流程提前公告；下限声明框架复用 F0237（下限框架第二用复用）。
//!
//! **数据结构（锚点原文）**：下限声明结构；实验档标记。
//!
//! **判据（锚点原文）**：显性拒绝、实验档、宣告流程、测试为准。
//! 本模块 12 条判据逐条映射：**显性拒绝 4 条**（下限以下设备拒绝三要素
//! （族号/拒绝码/理由）齐备／边界下族 GFX8 拒绝可查／拒绝理由含测试成
//! 本与特性依赖双因／未知族保守拒绝）、**实验档 3 条**（边界族 GFX9 实
//! 验档标注常驻／实验档不冒充稳定档／实验档渲染含「实验」字样）、**宣
//! 告流程 3 条**（下限调整须走宣告结构（新值+公告文本+生效序号）／公告
//! 未生效前旧值约束仍然有效／生效序号单调防回撤）、**测试为准 1 条**
//! （声明冲突以测试记录为准——实测通过可升档）、**判据自身 1 条**（码
//! 位互异段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 下限设备 → **显性拒绝** | [`FamilyFloor::gate`] 返回三要素拒绝——不是静默跳过 |
//! | 边界族 → **实验档** | GFX9 既是下限又是边界：准入但标注实验档，稳定承诺不含它 |
//! | 声明冲突 → **测试为准** | 宣告的新下限若高于实测通过族，实测记录优先回退 |
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **下限是宣告不是常量**：下限值带生效序号和公告文本——「我们把
//!    下限从 GFX9 提到 GFX10」是一个有生效时间的事件，而不是悄悄改一
//!    个数字。生效序号单调防回撤（不许偷偷放宽后再收紧）。
//! 2. **测试为准的仲裁**：宣告与实测冲突时以测试记录为准——声明是为
//!    测试服务的，不是反过来。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xBFC0 独占 0x10）。
pub const SEG_BASE: u32 = 0xBFC0;
pub const XC_BELOW_FLOOR: u32 = SEG_BASE + 1; // 下限以下显性拒绝
pub const XC_ANNOUNCE_BAD: u32 = SEG_BASE + 2; // 宣告非法（回撤/越界）
pub const XC_FAMILY_UNKNOWN: u32 = SEG_BASE + 3; // 未知族保守拒绝

/// 族轴。
pub const F_GFX8: u8 = 0; // 下限以下
pub const F_GFX9: u8 = 1; // 下限/边界族
pub const F_GFX10: u8 = 2;
pub const F_GFX103: u8 = 3;
pub const F_GFX11: u8 = 4;

/// 族名。
pub fn family_name(f: u8) -> &'static str {
    match f {
        F_GFX8 => "GFX8",
        F_GFX9 => "GFX9",
        F_GFX10 => "GFX10",
        F_GFX103 => "GFX10.3",
        F_GFX11 => "GFX11",
        _ => "未知",
    }
}

/// 拒绝三要素。
#[derive(Clone)]
pub struct Denial {
    pub family: u8,
    pub code: u32,
    pub reason: String, // 含测试成本与特性依赖双因
}

/// 宣告记录（下限调整）。
#[derive(Clone)]
pub struct Announce {
    pub seq: u64,      // 生效序号（单调）
    pub new_floor: u8,
    pub note: String,  // 公告文本
}

/// 实测记录（测试为准仲裁）。
#[derive(Clone)]
pub struct TestRec {
    pub family: u8,
    pub passed: bool,
}

/// 下限声明器（框架第二用复用：三要素/实验档/宣告与首用一致）。
pub struct FamilyFloor {
    pub floor: u8,          // 当前下限族
    pub seq: u64,           // 当前生效序号
    pub denials: Vec<Denial>,
    pub announces: Vec<Announce>,
    pub tests: Vec<TestRec>,
}

impl FamilyFloor {
    pub fn new() -> Self {
        FamilyFloor {
            floor: F_GFX9,
            seq: 1,
            denials: Vec::new(),
            announces: Vec::new(),
            tests: Vec::new(),
        }
    }

    /// 准入门：下限以下显性拒绝三要素；未知族保守拒绝。
    /// 返回 Ok(实验档?) —— 边界族（=下限）true，以上 false。
    pub fn gate(&mut self, family: u8) -> Result<bool, u32> {
        if family_name(family) == "未知" {
            return Err(XC_FAMILY_UNKNOWN);
        }
        if family < self.floor {
            let d = Denial {
                family,
                code: XC_BELOW_FLOOR,
                reason: format!(
                    "族 {} 低于支持下限 {}：测试成本未覆盖 + 依赖特性缺席",
                    family_name(family),
                    family_name(self.floor)
                ),
            };
            self.denials.push(d);
            return Err(XC_BELOW_FLOOR);
        }
        Ok(family == self.floor) // 边界族 = 实验档
    }

    /// 宣告调整：新下限不得低于当前（单调防回撤）、不得越出已知族表。
    pub fn announce(&mut self, new_floor: u8, note: String) -> Result<u64, u32> {
        if family_name(new_floor) == "未知" || new_floor < self.floor {
            return Err(XC_ANNOUNCE_BAD);
        }
        // 测试为准仲裁：若实测有高于新下限的通过族且新下限把它们划出，
        // 实测记录优先——回退宣告（声明服务于测试）。
        for t in &self.tests {
            if t.passed && t.family < new_floor && t.family >= self.floor {
                return Err(XC_ANNOUNCE_BAD); // 实测通过族被划出 → 拒绝宣告
            }
        }
        self.seq += 1;
        self.floor = new_floor;
        self.announces.push(Announce { seq: self.seq, new_floor, note });
        Ok(self.seq)
    }

    /// 实测记录入账（测试为准的数据源）。
    pub fn test_result(&mut self, family: u8, passed: bool) {
        self.tests.push(TestRec { family, passed });
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut out = format!(
            "下限声明 当前={}（{}） 序号={} 拒绝={} 宣告={}\n",
            family_name(self.floor),
            if self.floor == F_GFX9 { "实验档：稳定承诺不含 GFX9" } else { "稳定档" },
            self.seq,
            self.denials.len(),
            self.announces.len()
        );
        for d in &self.denials {
            let line = format!("  拒绝 {} 码={:x} {}\n", family_name(d.family), d.code, d.reason);
            out.push_str(&line);
        }
        for a in &self.announces {
            let line = format!("  宣告 #{} → {} {}\n", a.seq, family_name(a.new_floor), a.note);
            out.push_str(&line);
        }
        out
    }
}

impl Default for FamilyFloor {
    fn default() -> Self {
        Self::new()
    }
}

/// 场景核：GFX8 拒 / GFX9 实验档 / GFX10 稳定。
pub fn scene_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut f = FamilyFloor::new();
    // 1) GFX8 显性拒绝（三要素）
    if f.gate(F_GFX8) == Err(XC_BELOW_FLOOR)
        && f.denials[0].reason.contains("测试成本")
        && f.denials[0].reason.contains("特性")
    {
        caught += 1;
    }
    // 2) GFX9 边界族实验档
    if f.gate(F_GFX9) == Ok(true) {
        caught += 1;
    }
    // 3) GFX10 稳定档
    if f.gate(F_GFX10) == Ok(false) {
        caught += 1;
    }
    // 4) 宣告提升到 GFX10 生效
    if f.announce(F_GFX10, "下限提升公告".into()).is_ok() && f.floor == F_GFX10 {
        caught += 1;
    }
    // 5) 生效前旧值约束（GFX9 提升后拒绝）
    if f.gate(F_GFX9) == Err(XC_BELOW_FLOOR) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口。
pub fn run_veb256_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0256");
    // J1-1 下限以下显性拒绝
    let mut f = FamilyFloor::new();
    let ok = f.gate(F_GFX8) == Err(XC_BELOW_FLOOR) && f.denials.len() == 1;
    s.add("AMD-256-01-下限显性拒", ok, if ok { "" } else { "下限放水" });
    // J1-2 拒绝三要素齐备（族号/码/理由）
    let d = &f.denials[0];
    let ok = d.family == F_GFX8 && d.code == XC_BELOW_FLOOR && !d.reason.is_empty();
    s.add("AMD-256-02-拒绝三要素", ok, if ok { "" } else { "要素缺失" });
    // J1-3 理由双因（测试成本 + 特性依赖）
    let ok = d.reason.contains("测试成本") && d.reason.contains("特性");
    s.add("AMD-256-03-理由双因", ok, if ok { "" } else { "理由单因" });
    // J1-4 边界族实验档
    let ok = f.gate(F_GFX9) == Ok(true);
    s.add("AMD-256-04-边界实验档", ok, if ok { "" } else { "边界档破缺" });
    // J1-5 实验档不冒充稳定（渲染声明）
    let ok = f.render().contains("实验档") && f.render().contains("稳定承诺不含");
    s.add("AMD-256-05-实验档声明", ok, if ok { "" } else { "实验档冒充稳定" });
    // J1-6 未知族保守拒绝
    let ok = f.gate(0x77) == Err(XC_FAMILY_UNKNOWN);
    s.add("AMD-256-06-未知族拒", ok, if ok { "" } else { "未知族放行" });
    // J1-7 宣告流程（新值+公告文本+生效序号）
    let mut f2 = FamilyFloor::new();
    f2.test_result(F_GFX10, true);
    let seq = f2.announce(F_GFX10, "GFX10 全量认证完成".into());
    let ok = seq.is_ok() && f2.announces[0].seq == seq.unwrap_or(0)
        && f2.announces[0].note.contains("GFX10");
    s.add("AMD-256-07-宣告三件套", ok, if ok { "" } else { "宣告破缺" });
    // J1-8 生效序号单调
    let ok = f2.announces[0].seq > 1;
    s.add("AMD-256-08-序号单调", ok, if ok { "" } else { "序号回撤" });
    // J1-9 回撤拒绝（新下限低于当前）
    let ok = f2.announce(F_GFX9, "放宽".into()) == Err(XC_ANNOUNCE_BAD);
    s.add("AMD-256-09-回撤拒绝", ok, if ok { "" } else { "回撤放行" });
    // J1-10 测试为准（实测通过族被划出 → 宣告拒绝）
    let mut f3 = FamilyFloor::new();
    f3.test_result(F_GFX9, true); // GFX9 实测通过
    let ok = f3.announce(F_GFX10, "划出 GFX9".into()) == Err(XC_ANNOUNCE_BAD);
    s.add("AMD-256-10-测试为准", ok, if ok { "" } else { "宣告压过实测" });
    // J1-11 场景核 5/5
    let (a, n) = scene_core();
    let ok = a == n;
    let dd = if ok { String::new() } else { format!("场景 {a}/{n}") };
    s.add_dyn("AMD-256-11-场景核", ok, &dd);
    // J1-12 码位互异 + 段对齐
    let ok = XC_BELOW_FLOOR != XC_ANNOUNCE_BAD
        && XC_ANNOUNCE_BAD != XC_FAMILY_UNKNOWN
        && SEG_BASE % 0x10 == 0;
    s.add("AMD-256-12-码位段位", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb256_checks_all_green() {
        let set = run_veb256_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn veb256_floor_move_effect() {
        // 宣告生效前旧约束有效、生效后新约束接管
        let mut f = FamilyFloor::new();
        f.test_result(F_GFX103, true);
        assert!(f.gate(F_GFX9).is_ok()); // 旧下限 GFX9 准入
        f.announce(F_GFX103, "实测 GFX10.3 通过，收窄".into()).ok();
        assert!(f.gate(F_GFX9).is_err());
        assert!(f.gate(F_GFX10).is_err());
        assert!(f.gate(F_GFX103) == Ok(true));
    }
}
