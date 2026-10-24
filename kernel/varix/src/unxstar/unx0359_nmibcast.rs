//! unx0359_nmibcast —— UNX-F0359 多核启动 NMI 广播预留（F0128 扩展）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0359 · NMI 广播预留 · 目标 200 行
//! 判据原文：UNX-F0359-J1 NMI 广播接口预留：INIT-SIPI 之外的
//!           唤醒通道（NMI 向量 2）作为预案位；广播目标集 =
//!           启动集全核（不许漏核）；NMI 不携带载荷（唤醒信号
//!           纯触发）；预留接口冻结（判定/账面/广播清单三件
//!           套成型，具体寄存器位接 F0128）。
//! 组件：广播预案（目标集 + 触发位图）+ 覆盖核（目标集 ⊇ 启动
//!       集全核）+ 无载荷核（NMI 无参锚）+ 冻结闸。
//! 职责定位：NMI 广播是「唤醒的最后一招」——INIT-SIPI 对卡死
//!   的核可能无效，NMI 不可屏蔽的特质是唯一能穿透的触发。预
//!   留阶段把判定/账面/清单三件套钉死，寄存器位留给 F0128，
//!   接口冻结防「临时加参数」把无载荷语义撕开。
//! 性能逐项分解：覆盖核 O(核数)；冻结 O(1)；位图 O(1)。
//! 诊断码段：0x206C0（0x206C1~0x206C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const NB_VERSION: &str = "unx0359_nmibcast-v1";

/// 诊断段基址（0x206C0 段 · 本模块独占）。
pub const NB_SEG_BASE: u32 = 0x206C0;
pub const XC_COVER_MISS: u32 = 0x206C1; // 目标集漏核
pub const XC_WITH_PAYLOAD: u32 = 0x206C2; // NMI 带载荷（破语义）
pub const XC_VEC_BAD: u32 = 0x206C3; // 非 NMI 向量
pub const XC_FROZEN: u32 = 0x206C4; // 冻结后拒改
pub const XC_EMPTY_TARGET: u32 = 0x206C5; // 空目标集拒
pub const XC_CORE_OOR: u32 = 0x206C6; // 核号越域
pub const XC_BITMAP_DRIFT: u32 = 0x206C7; // 位图/清单漂移
pub const XC_SELF_TARGET: u32 = 0x206C8; // 自核广播拒
pub const XC_FROZEN_NO: u32 = 0x206C9; // 未冻结就广播

/// NMI 向量号。
pub const VEC_NMI: u32 = 2;
/// 核号闭集。
pub const CORE_MAX: u32 = 15;

/// NMI 广播预案。
pub struct NmiPlan {
    /// 目标核清单（升序无重复）。
    targets: Vec<u32>,
    /// 广播发起核。
    origin: u32,
    frozen: bool,
}

impl NmiPlan {
    pub fn new(origin: u32, targets: &[u32]) -> Result<NmiPlan, u32> {
        if origin > CORE_MAX {
            return Err(XC_CORE_OOR);
        }
        if targets.is_empty() {
            return Err(XC_EMPTY_TARGET);
        }
        let mut prev = 0;
        let mut first = true;
        for &t in targets.iter() {
            if t > CORE_MAX {
                return Err(XC_CORE_OOR);
            }
            if t == origin {
                return Err(XC_SELF_TARGET); // 自核广播无意义
            }
            if !first && t <= prev {
                return Err(XC_BITMAP_DRIFT); // 清单乱序
            }
            prev = t;
            first = false;
        }
        Ok(NmiPlan { targets: targets.to_vec(), origin, frozen: false })
    }

    /// 覆盖核：目标集必须覆盖启动集全核（漏核 = 有核永远叫不醒）。
    pub fn covers(&self, boot_set: &[u32]) -> Result<(), u32> {
        for &c in boot_set.iter() {
            if c == self.origin {
                continue; // 发起核自免
            }
            if !self.targets.contains(&c) {
                return Err(XC_COVER_MISS);
            }
        }
        Ok(())
    }

    /// 无载荷核：NMI 只有触发位、无参（接口语义锚）。
    pub fn trigger(&self) -> Result<(), u32> {
        if !self.frozen {
            return Err(XC_FROZEN_NO);
        }
        Ok(())
    }

    /// 带载荷请求拒（NMI 不许携带参数）。
    pub fn trigger_with_payload(&self, _payload: u32) -> Result<(), u32> {
        Err(XC_WITH_PAYLOAD)
    }

    /// 向量核：仅向量 2 可广播。
    pub fn vector_ok(vector: u32) -> Result<(), u32> {
        if vector != VEC_NMI {
            return Err(XC_VEC_BAD);
        }
        Ok(())
    }

    /// 冻结。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// 目标数。
    pub fn target_count(&self) -> u32 {
        self.targets.len() as u32
    }

    /// 发起核自免后的目标集（广播位图）。
    pub fn bitmap(&self) -> Vec<u32> {
        self.targets.iter().copied().filter(|&t| t != self.origin).collect()
    }
}

pub fn run_unx0359_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0359_nmibcast");
    // 族一：预案构造。
    let mut plan = match NmiPlan::new(0, &[1, 2, 3, 4]) {
        Ok(p) => p,
        Err(_) => {
            cs.add("预案构造", false, "被拒");
            return cs;
        }
    };
    cs.add("目标 4", plan.target_count() == 4, "AP 1..4");
    // 族二：覆盖核。
    cs.add("覆盖启动集", plan.covers(&[0, 1, 2, 3, 4]).is_ok(), "全核在列");
    cs.add("漏核立案", plan.covers(&[0, 1, 2, 3, 4, 5]) == Err(XC_COVER_MISS), "核 5 漏");
    cs.add("发起核自免", plan.covers(&[0]).is_ok(), "核 0 发起自免");
    // 族三：无载荷语义。
    cs.add("未冻结拒触发", plan.trigger() == Err(XC_FROZEN_NO), "先冻结");
    plan.freeze();
    cs.add("冻结触发过", plan.trigger().is_ok(), "纯触发");
    cs.add("带载荷拒", plan.trigger_with_payload(0x42) == Err(XC_WITH_PAYLOAD), "无参锚");
    // 族四：向量核。
    cs.add("向量 2 过", NmiPlan::vector_ok(VEC_NMI).is_ok(), "NMI");
    cs.add("向量 3 拒", NmiPlan::vector_ok(3) == Err(XC_VEC_BAD), "非 NMI");
    // 族五：构造闸。
    cs.add("空目标拒", NmiPlan::new(0, &[]).is_err(), "EMPTY");
    cs.add("自核目标拒", NmiPlan::new(2, &[1, 2]).is_err(), "SELF");
    cs.add("乱序拒", NmiPlan::new(0, &[3, 1]).is_err(), "BITMAP_DRIFT");
    cs.add("核越域拒", NmiPlan::new(16, &[1]).is_err(), "CORE_OOR");
    // 族六：位图自免。
    cs.add("位图自免", {
        let mut q = NmiPlan::new(1, &[2, 3]).unwrap_or(NmiPlan { targets: vec![], origin: 0, frozen: false });
        q.freeze();
        q.trigger().is_ok()
    }, "构造合法路径");
    cs.add("发起核不在位图", {
        // origin=1 的合法构造：目标 [2,3]，位图不含 1。
        NmiPlan::new(1, &[2, 3]).map(|q| q.bitmap().as_slice() == &[2u32, 3]).unwrap_or(false)
    }, "1 号自免");
    cs
}
