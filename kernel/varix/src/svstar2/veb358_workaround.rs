//! veb358_workaround —— VE-F0358 软渲规避表。
//! 锚点：VE-F0358 · 目标 300 行。
//! 判据原文：自动规避（问题条目触发条件×规避动作×影响面，
//!           运行时自动启用规避路径）、热更新（表随白名单
//!           热更新发布）、保守兜底（表加载失败保守加告警）、
//!           快照联动（启用入诊断快照 F0354）、判据。
//! 组件：规避表 + 匹配器 + 启用位。
//! 职责定位：规避表是「老司机的经验卡」——什么路况踩什么刹
//!           车（触发×动作）写在卡上，装车自动生效；卡丢了
//!           就慢点开（保守加告警），不裸奔。
//! 性能逐项分解：启动期匹配一次性；运行时 O(1) 启用位。
//! 诊断码段：0x32280（0x32281~0x32289 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const WA_VERSION: &str = "veb358_workaround-v1";

/// 诊断段基址（0x32280 段 · 本模块独占）。
pub const WA_SEG_BASE: u32 = 0x32280;
pub const XC_BADCOND: u32 = 0x32281; // 触发条件非法
pub const XC_LOADFAIL: u32 = 0x32282; // 表加载失败
pub const XC_CONFLICT: u32 = 0x32283; // 条目冲突（非精确匹配优先级不可分）

/// 规避条目。
pub struct Wa {
    /// 触发条件代号（环境指纹）。
    pub cond: u32,
    /// 规避动作代号。
    pub act: u32,
    /// 影响面代号。
    pub scope: u32,
    /// 启用位。
    pub on: bool,
}

/// 规避表。
pub struct WaBook {
    items: Vec<Wa>,
    /// 表健康态（加载失败 → 保守）。
    pub healthy: bool,
    /// 告警数。
    pub alerts: u32,
    /// 启用记录（进快照）。
    pub activations: u32,
}

impl WaBook {
    /// 空表（健康）。
    pub fn new() -> WaBook {
        WaBook {
            items: Vec::new(),
            healthy: true,
            alerts: 0,
            activations: 0,
        }
    }

    /// 模拟加载失败：转保守态加告警。
    pub fn load_fail(&mut self) -> u32 {
        self.healthy = false;
        self.alerts += 1;
        self.items.clear();
        self.alerts
    }

    /// 恢复加载（热更新成功）。
    pub fn reload(&mut self) {
        self.healthy = true;
    }

    /// 登记条目：触发与动作都非零。
    pub fn put(&mut self, cond: u32, act: u32, scope: u32) -> Result<(), u32> {
        if cond == 0 || act == 0 {
            return Err(XC_BADCOND);
        }
        // 精确匹配优先：同 cond 重复登记拒（避免歧义冲突）。
        if self.items.iter().any(|w| w.cond == cond) {
            return Err(XC_CONFLICT);
        }
        self.items.push(Wa {
            cond,
            act,
            scope,
            on: false,
        });
        Ok(())
    }

    /// 运行时匹配：环境指纹精确命中 → 启用位并记激活。
    pub fn match_run(&mut self, env: u32) -> Option<u32> {
        if !self.healthy {
            return None; // 保守：不启用任何规避
        }
        let w = self.items.iter_mut().find(|w| w.cond == env)?;
        if !w.on {
            w.on = true;
            self.activations += 1;
        }
        Some(w.act)
    }

    /// 启用位查询。
    pub fn is_on(&self, cond: u32) -> bool {
        self.items
            .iter()
            .find(|w| w.cond == cond)
            .map(|w| w.on)
            .unwrap_or(false)
    }

    /// 已启用数（快照供给）。
    pub fn on_count(&self) -> u32 {
        self.items.iter().filter(|w| w.on).count() as u32
    }

    /// 表长。
    pub fn len(&self) -> usize {
        self.items.len()
    }
}

pub fn run_veb358_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0358");
    // 族零：登记门。
    let mut b = WaBook::new();
    s.add("零触发拒", b.put(0, 1, 1) == Err(XC_BADCOND), "无条件");
    s.add("零动作拒", b.put(1, 0, 1) == Err(XC_BADCOND), "无动作");
    s.add("登记过", b.put(0xA1, 7, 2).is_ok(), "A1→动作 7");
    s.add("同触发冲突拒", b.put(0xA1, 8, 3) == Err(XC_CONFLICT), "精确匹配唯一");
    s.add("异触发过", b.put(0xB2, 9, 1).is_ok(), "第二条");
    s.add("表二", b.len() == 2, "两卡");
    // 族一：自动规避。
    s.add("命中启用", b.match_run(0xA1) == Some(7), "装车生效");
    s.add("启用位在", b.is_on(0xA1), "bit on");
    s.add("激活一", b.activations == 1, "记账");
    s.add("重复匹配幂等", b.match_run(0xA1) == Some(7) && b.activations == 1, "已启用不再计");
    s.add("未命中无启", b.match_run(0xFF).is_none(), "无卡不装");
    s.add("B2 未启", !b.is_on(0xB2), "没跑过");
    // 命中 B2。
    let _ = b.match_run(0xB2);
    s.add("B2 启用", b.is_on(0xB2), "bit on");
    s.add("启用二", b.on_count() == 2, "快照供给");
    // 族二：加载失败保守兜底。
    let mut b2 = WaBook::new();
    let _ = b2.put(0xC3, 5, 1);
    let _ = b2.match_run(0xC3);
    s.add("失败前健康", b2.healthy, "正常");
    s.add("失败告警一", b2.load_fail() == 1, "告警");
    s.add("转保守", !b2.healthy, "不裸奔");
    s.add("保守不匹配", b2.match_run(0xC3).is_none(), "表空不启用");
    s.add("热更恢复", {
        b2.reload();
        b2.healthy
    }, "重载成功");
    // 恢复后重登记可再用。
    let _ = b2.put(0xC3, 5, 1);
    s.add("恢复后匹配", b2.match_run(0xC3) == Some(5), "经验卡重装");
    // 族三：快照联动语义。
    s.add("启用数一致", b2.on_count() == 1 && b2.activations >= 2, "对账");
    // 族四：多表互不干扰。
    let mut b3 = WaBook::new();
    let _ = b3.put(0xA1, 1, 1);
    s.add("独立表 A1", b3.match_run(0xA1) == Some(1), "动作 1 非 7");
    s.add("原表 A1 仍 7", b.match_run(0xA1) == Some(7), "互不干扰");
    s.add("魔数核", WA_SEG_BASE == 0x32280, "段基址");
    s
}
