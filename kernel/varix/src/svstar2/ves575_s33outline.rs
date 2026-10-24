//! VE-F4341 · U08 开工与测试总纲（U08 测试与资产组）
//! 职责：U08 组开工——一致性测试工具总纲（工具双角色定位：把一致性自身
//! 当被测对象 + 提供测试工具箱双职能）+ 工具箱清单十五件（自动化/审查器/
//! 仿真器/语料库/回归/用户测试/看板/API/示例/文档/扩展/第三方/兼容/fuzz/
//! 走查——清单断言拦截缺件）+ 与 T08 姊妹结构呼应 + 双角色混淆职责声明卡。
//! 段位：0x3BE0（续表 idx575）。行数预算 340。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3BE0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TgCode {
    /// 工具缺件（清单断言拦截）。
    ToolMissing = SEG_BASE,
    /// 双角色混淆（职责声明卡拒绝）。
    RoleConfused = SEG_BASE + 0x01,
    /// 件号越界。
    SlotRange = SEG_BASE + 0x02,
}

/// 十五件清单位（0-14）。
pub const TOOL_SLOTS: usize = 15;

pub const TOOL_NAMES: [&str; TOOL_SLOTS] = [
    "自动化", "审查器", "仿真器", "语料库", "回归",
    "用户测试", "看板", "API", "示例", "文档",
    "扩展", "第三方", "兼容", "fuzz", "走查",
];

/// 总纲台。
#[derive(Default)]
pub struct TestOutline {
    /// 十五件在册位。
    pub tools: [bool; TOOL_SLOTS],
    /// 双角色职责声明：true = 被测对象角色 / false = 工具箱角色 分离在案。
    pub role_dut: bool,
    pub role_box: bool,
    pub blocks: u32,
}

impl TestOutline {
    pub fn new() -> Self {
        TestOutline { ..Default::default() }
    }

    /// 工具件登记（件号校验）。
    pub fn put(&mut self, slot: usize) -> Result<(), TgCode> {
        if slot >= TOOL_SLOTS {
            return Err(TgCode::SlotRange);
        }
        self.tools[slot] = true;
        Ok(())
    }

    /// 清单断言：缺件拦截（返回缺件清单，计数留痕）。
    pub fn assert_full(&mut self) -> Vec<usize> {
        let mut missing = Vec::new();
        for i in 0..TOOL_SLOTS {
            if !self.tools[i] {
                missing.push(i);
            }
        }
        if !missing.is_empty() {
            self.blocks += 1;
        }
        missing
    }

    /// 双角色声明：两角色都须显式声明且互斥使用（同一时刻单角色）。
    pub fn declare_roles(&mut self, dut: bool, box_role: bool) -> Result<(), TgCode> {
        // 双角色必须都声明在案（职责卡双列），但任一时刻只可单角色执行。
        if dut == box_role {
            return Err(TgCode::RoleConfused); // 同真同假 = 混淆
        }
        self.role_dut = dut;
        self.role_box = box_role;
        Ok(())
    }

    /// 当前角色查询。
    pub fn current_role(&self) -> Option<&'static str> {
        if self.role_dut {
            Some("被测对象")
        } else if self.role_box {
            Some("工具箱")
        } else {
            None
        }
    }

    /// 姊妹结构对齐（T08 参照——十五件同构断言）。
    pub fn sister_aligned(&self, names: &[&str; TOOL_SLOTS]) -> bool {
        names == &TOOL_NAMES
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves575_checks() -> CheckSet {
    let mut set = CheckSet::new("ves575");
    let mut to = TestOutline::new();

    // C575-01 清单断言：空台全缺（15 件缺）。
    let missing = to.assert_full();
    chk(&mut set, "C575-01", missing.len() == 15 && to.blocks == 1, "空台全缺拦截");

    // C575-02 逐件登记后断言清零。
    let mut ok = true;
    for i in 0..TOOL_SLOTS {
        if to.put(i).is_err() {
            ok = false;
        }
    }
    chk(&mut set, "C575-02", ok && to.assert_full().is_empty(), "十五件齐备");

    // C575-03 件号越界拒。
    chk(&mut set, "C575-03", to.put(15).is_err() && to.put(usize::MAX).is_err(), "件号越界拒");

    // C575-04 双角色声明（单角色时刻）。
    chk(&mut set, "C575-04", to.declare_roles(true, false).is_ok() && to.current_role() == Some("被测对象"), "被测对象角色");

    // C575-05 角色切换。
    chk(&mut set, "C575-05", to.declare_roles(false, true).is_ok() && to.current_role() == Some("工具箱"), "工具箱角色");

    // C575-06 双角色混淆拒绝（同真/同假）。
    chk(&mut set, "C575-06", to.declare_roles(true, true).is_err() && to.declare_roles(false, false).is_err(), "混淆拒绝");

    // C575-07 姊妹结构对齐（T08 同构）。
    chk(&mut set, "C575-07", to.sister_aligned(&TOOL_NAMES), "姊妹结构对齐");

    // C575-08 十五件名目单源（关键槽位抽查）。
    chk(&mut set, "C575-08", TOOL_NAMES[0] == "自动化" && TOOL_NAMES[13] == "fuzz" && TOOL_NAMES[14] == "走查", "件名单源");

    // C575-09 断言计数只增不误（重复断言无缺件不计数）。
    let b0 = to.blocks;
    let _ = to.assert_full();
    chk(&mut set, "C575-09", to.blocks == b0, "齐备断言不计数");

    // C575-10 缺件断言计数（拆一件再断言）。
    to.tools[6] = false;
    let m2 = to.assert_full();
    chk(&mut set, "C575-10", m2 == [6] && to.blocks == b0 + 1, "缺件计数精确");

    // C575-11 未声明角色查询恒 None（不臆测）。
    let to2 = TestOutline::new();
    chk(&mut set, "C575-11", to2.current_role().is_none(), "未声明不臆测");

    // C575-12 全件恢复后断言洁（回修重扫路径）。
    let _ = to.put(6);
    chk(&mut set, "C575-12", to.assert_full().is_empty(), "回修复扫洁");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assert_missing_slots() {
        let mut to = TestOutline::new();
        for i in [0, 2, 14] {
            let _ = to.put(i);
        }
        let m = to.assert_full();
        assert_eq!(m.len(), 12);
        assert!(!m.contains(&0));
        assert!(m.contains(&1));
    }

    #[test]
    fn role_exclusive() {
        let mut to = TestOutline::new();
        assert!(to.declare_roles(true, false).is_ok());
        assert!(to.declare_roles(false, true).is_ok());
        assert!(to.declare_roles(true, true).is_err());
        assert_eq!(to.current_role(), Some("工具箱"));
    }
}
