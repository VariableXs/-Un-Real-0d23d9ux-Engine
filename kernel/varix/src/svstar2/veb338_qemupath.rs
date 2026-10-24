//! veb338_qemupath —— VE-F0338 QEMU RISC-V 模拟路径。
//! 锚点：VE-F0338 · 目标 340 行。
//! 判据原文：模拟态标注（QEMU 环境结果标注模拟态，不冒充
//!           实机结论）、三组用例（启动 / 软渲渲染 / 调试通
//!           道连通）、不外推（模拟与实机差异如实登记不外推）、
//!           批队列兼容（与 full 档批队列兼容）、判据。
//! 组件：模拟环境定义 + 用例集 + 模拟态标注器。
//! 职责定位：QEMU 模拟是「模拟考」——成绩单上必须盖「模拟」
//!           章；拿模拟考分数报实机高考是造假，一律不外推。
//! 性能逐项分解：搭建一次性；用例 O(组数)；标注 O(1)。
//! 诊断码段：0x31D80（0x31D81~0x31D89 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const QE_VERSION: &str = "veb338_qemupath-v1";

/// 诊断段基址（0x31D80 段 · 本模块独占）。
pub const QE_SEG_BASE: u32 = 0x31D80;
pub const XC_NOSETUP: u32 = 0x31D81; // 未搭建先跑用例
pub const XC_BADGRP: u32 = 0x31D82; // 用例组非法
pub const XC_EXTRAP: u32 = 0x31D83; // 外推实机（红线）
pub const XC_BADVER: u32 = 0x31D84; // QEMU 版本不合法
pub const XC_NOCASE: u32 = 0x31D85; // 用例不存在

/// 支持的 QEMU 主版本下限。
pub const QEMU_MIN_MAJOR: u32 = 7;

/// 用例组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grp {
    /// 启动。
    Boot,
    /// 软渲渲染。
    Soft,
    /// 调试通道连通。
    Dbg,
}

/// 组序号。
pub fn grp_no(g: Grp) -> u32 {
    match g {
        Grp::Boot => 0,
        Grp::Soft => 1,
        Grp::Dbg => 2,
    }
}

/// 运行态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// 模拟态（QEMU 结果必须标注）。
    Sim,
    /// 实机态（仅真机批队列）。
    Real,
}

/// 用例结果。
pub struct CaseRes {
    pub grp: Grp,
    pub passed: bool,
    pub mark: Mark,
    /// 运行周期。
    pub epoch: u32,
}

/// 模拟环境。
pub struct QemuEnv {
    /// 是否已搭建。
    setup: bool,
    /// QEMU 主版本。
    pub major: u32,
    /// 版本差异登记数。
    pub variant_logged: u32,
    /// 模拟与实机差异登记（不外推的诚实账）。
    pub diffs: Vec<(u32, u32, u32)>, // (组, 模拟值, 实机值)
    res: Vec<CaseRes>,
}

impl QemuEnv {
    /// 建：版本低于下限即登记差异（兼容登记）但仍可用。
    pub fn new(major: u32) -> QemuEnv {
        QemuEnv {
            setup: false,
            major,
            variant_logged: if major < QEMU_MIN_MAJOR { 1 } else { 0 },
            diffs: Vec::new(),
            res: Vec::new(),
        }
    }

    /// 搭建（一次性；重复搭建幂等）。
    pub fn setup(&mut self) -> bool {
        let was = self.setup;
        self.setup = true;
        !was
    }

    /// 已搭建核。
    pub fn ready(&self) -> bool {
        self.setup
    }

    /// 跑用例：未搭建拒；组恒合法（枚举收口）；结果强制标 Sim。
    pub fn run(&mut self, g: Grp, passed: bool, epoch: u32) -> Result<Mark, u32> {
        if !self.setup {
            return Err(XC_NOSETUP);
        }
        let _ = grp_no(g);
        self.res.push(CaseRes {
            grp: g,
            passed,
            mark: Mark::Sim,
            epoch,
        });
        Ok(Mark::Sim)
    }

    /// 三组覆盖核：启动/软渲/调试都跑过才算贯通。
    pub fn covered(&self) -> bool {
        let mut m = 0u32;
        for r in self.res.iter() {
            m |= 1 << grp_no(r.grp);
        }
        m == 0b111
    }

    /// 差异登记：模拟值 ≠ 实机值时入账（不外推的凭证）。
    pub fn log_diff(&mut self, g: Grp, sim: u32, real: u32) -> Result<(), u32> {
        if sim == real {
            return Ok(()); // 无差异不登记
        }
        self.diffs.push((grp_no(g), sim, real));
        Ok(())
    }

    /// 外推检查：差异未清零前拒绝实机结论。
    pub fn extrap_ok(&self) -> Result<(), u32> {
        if !self.diffs.is_empty() {
            return Err(XC_EXTRAP);
        }
        Ok(())
    }

    /// 实机标注：仅真机批队列使用；模拟环境里一律拒绝制造 Real 标注。
    pub fn real_mark(&self) -> Result<Mark, u32> {
        Err(XC_EXTRAP)
    }

    /// 批队列兼容：模拟态结果可入 full 档批队列（带 Sim 标记排队）。
    pub fn enqueue(&self, out: &mut Vec<(u32, Mark)>) -> usize {
        let before = out.len();
        for r in self.res.iter() {
            out.push((grp_no(r.grp), r.mark));
        }
        out.len() - before
    }

    /// 组结果查询（最近一次）。
    pub fn last_of(&self, g: Grp) -> Option<bool> {
        self.res.iter().rev().find(|r| r.grp == g).map(|r| r.passed)
    }

    /// 结果数。
    pub fn len(&self) -> usize {
        self.res.len()
    }
}

pub fn run_veb338_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0338");
    // 族零：搭建门。
    let mut q = QemuEnv::new(8);
    s.add("未搭拒跑", q.run(Grp::Boot, true, 1) == Err(XC_NOSETUP), "先搭建");
    s.add("搭建过", q.setup(), "首次");
    s.add("重搭幂等", !q.setup(), "二次 false");
    s.add("就绪", q.ready(), "门开");
    // 族一：三组用例。
    s.add("启动过", q.run(Grp::Boot, true, 1) == Ok(Mark::Sim), "标 Sim");
    s.add("软渲过", q.run(Grp::Soft, true, 2) == Ok(Mark::Sim), "标 Sim");
    s.add("调试过", q.run(Grp::Dbg, false, 3) == Ok(Mark::Sim), "败也 Sim");
    s.add("三组齐", q.covered(), "0b111");
    s.add("结果三", q.len() == 3, "三笔");
    s.add("败如实", q.last_of(Grp::Dbg) == Some(false), "调试未通");
    s.add("过如实", q.last_of(Grp::Soft) == Some(true), "软渲通");
    // 族二：模拟态红线。
    s.add("造实机标注拒", q.real_mark() == Err(XC_EXTRAP), "模拟不出 Real");
    s.add("无差异直过", q.log_diff(Grp::Boot, 5, 5).is_ok(), "同值不记");
    s.add("差异入账", q.log_diff(Grp::Soft, 60, 90).is_ok(), "模拟 60 实机 90");
    s.add("差异一笔", q.diffs.len() == 1, "入账");
    s.add("外推拒", q.extrap_ok() == Err(XC_EXTRAP), "有差异不外推");
    // 差异清零后可外推（以实现为准修正登记）。
    q.diffs.clear();
    s.add("清零可判", q.extrap_ok().is_ok(), "无差异");
    // 族三：版本兼容登记。
    let q2 = QemuEnv::new(6);
    s.add("低版登记", q2.variant_logged == 1, "v6 差异");
    s.add("v8 免登", QemuEnv::new(8).variant_logged == 0, "达标");
    // 族四：批队列兼容。
    let mut queue: Vec<(u32, Mark)> = Vec::new();
    let n = q.enqueue(&mut queue);
    s.add("入队三", n == 3, "三笔排队");
    s.add("全标 Sim", queue.iter().all(|x| x.1 == Mark::Sim), "队列诚实");
    s.add("序 012", queue[0].0 == 0 && queue[1].0 == 1 && queue[2].0 == 2, "组序");
    // 族五：重跑覆盖。
    let _ = q.run(Grp::Dbg, true, 4);
    s.add("复跑转绿", q.last_of(Grp::Dbg) == Some(true), "最近一次");
    s.add("结果四", q.len() == 4, "追加不覆盖");
    s.add("魔数核", QE_SEG_BASE == 0x31D80, "段基址");
    s
}
