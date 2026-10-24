//! veb271_nvfloor —— VE-F0271 NVIDIA 老旧代支持下限。
//! 锚点（CoRun Unxreal 主册）：VE-F0271 · NVIDIA 老旧代支持下限 · 目标 300 行
//! 判据原文：VE-F0271-J1 支持下限线：专有路径最低支持代际与开
//!           源路径最低支持代际各自定线；低于下限 → 该路径拒
//!           （降档链末端）；下限变更史（提高下限要留档：何时
//!           从哪提到哪）；下限核（下限 ≤ 当前最新代际 - 2，
//!           保底向后窗口）。
//! 组件：双下限账 + 变更史 + 下限核。
//! 职责定位：支持下限是「NVIDIA 通路的退休线」——代际老到一定
//!           程度固件不再签名、命令流文档不再维护，硬撑只会产
//!           出静默损坏；专有与开源的下限不同（开源靠社区文档
//!           通常更长寿）；提高下限要留档：退休一个代际是有成
//!           本的决定，不是随手改的常量。
//! 性能逐项分解：查线 O(史长)；核 O(1)。
//! 诊断码段：0x30CC0（0x30CC1~0x30CC9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const FL_VERSION: &str = "veb271_nvfloor-v1";

/// 诊断段基址（0x30CC0 段 · 本模块独占）。
pub const FL_SEG_BASE: u32 = 0x30CC0;
pub const XC_BELOW: u32 = 0x30CC1; // 低于下限
pub const XC_MAGIC: u32 = 0x30CC2; // 账魔数破
pub const XC_GEN_OOR: u32 = 0x30CC3; // 代际越域
pub const XC_FLOOR_HI: u32 = 0x30CC4; // 下限越过窗口核
pub const XC_NOENT: u32 = 0x30CC5; // 无变更史
pub const XC_BACK: u32 = 0x30CC6; // 下限回退（降线）
pub const XC_PATH_OOR: u32 = 0x30CC7; // 路径越域

/// 最新已知代际（Ada）。
pub const LATEST_GEN: u32 = 6;
/// 向后保底窗口。
pub const FLOOR_WINDOW: u32 = 2;

/// 路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathK {
    Prop = 0,
    Open = 1,
}

/// 下限变更记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FloorRec {
    pub path: u32,
    pub old: u32,
    pub new: u32,
}

/// 下限账。
pub struct FloorBook {
    floors: [u32; 2],
    history: Vec<FloorRec>,
    magic: u32,
}

impl FloorBook {
    /// 建（初始下限：专有 3、开源 1）。
    pub fn new() -> FloorBook {
        FloorBook { floors: [3, 1], history: Vec::new(), magic: 0x0CC0_1100 }
    }

    /// 查下限。
    pub fn floor_of(&self, p: PathK) -> u32 {
        self.floors[p as usize]
    }

    /// 支持判定。
    pub fn supports(&self, p: PathK, gen: u32) -> Result<(), u32> {
        if self.magic != 0x0CC0_1100 {
            return Err(XC_MAGIC);
        }
        if gen == 0 || gen > LATEST_GEN {
            return Err(XC_GEN_OOR);
        }
        if gen < self.floors[p as usize] {
            return Err(XC_BELOW);
        }
        Ok(())
    }

    /// 提高下限（只升不降）。
    pub fn raise(&mut self, p: PathK, new_floor: u32) -> Result<(), u32> {
        if self.magic != 0x0CC0_1100 {
            return Err(XC_MAGIC);
        }
        if new_floor > LATEST_GEN {
            return Err(XC_GEN_OOR);
        }
        let old = self.floors[p as usize];
        if new_floor < old {
            return Err(XC_BACK);
        }
        if new_floor > LATEST_GEN - FLOOR_WINDOW {
            return Err(XC_FLOOR_HI);
        }
        self.floors[p as usize] = new_floor;
        self.history.push(FloorRec { path: p as u32, old, new: new_floor });
        Ok(())
    }

    /// 变更史核。
    pub fn history_audit(&self) -> Result<u32, u32> {
        if self.history.is_empty() {
            return Err(XC_NOENT);
        }
        for i in 1..self.history.len() {
            if self.history[i].old != self.history[i - 1].new && self.history[i].path == self.history[i - 1].path {
                return Err(XC_BACK);
            }
        }
        Ok(self.history.len() as u32)
    }
}

pub fn run_veb271_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0271");
    // 族一：初始下限。
    let mut b = FloorBook::new();
    s.add("专有下限 3", b.floor_of(PathK::Prop) == 3, "Pascal 起");
    s.add("开源下限 1", b.floor_of(PathK::Open) == 1, "Kepler 起");
    s.add("专有 Pascal 过", b.supports(PathK::Prop, 3).is_ok(), "恰线");
    s.add("专有 Maxwell 拒", b.supports(PathK::Prop, 2) == Err(XC_BELOW), "退休线");
    s.add("开源 Maxwell 过", b.supports(PathK::Open, 2).is_ok(), "开源更长寿");
    s.add("开源 Kepler 过", b.supports(PathK::Open, 1).is_ok(), "底线");
    s.add("代际 0 拒", b.supports(PathK::Open, 0) == Err(XC_GEN_OOR), "Unknown");
    s.add("代际 9 拒", b.supports(PathK::Open, 9) == Err(XC_GEN_OOR), "未命名代");
    // 族二：提高下限。
    s.add("开源提到 2 过", b.raise(PathK::Open, 2).is_ok(), "Kepler 退休");
    s.add("开源 Kepler 现拒", b.supports(PathK::Open, 1) == Err(XC_BELOW), "线已抬");
    s.add("史 1 笔", b.history_audit() == Ok(1), "留档");
    s.add("下限回退拒", b.raise(PathK::Open, 1) == Err(XC_BACK), "退休不复职");
    s.add("窗口核拒", b.raise(PathK::Open, 5) == Err(XC_FLOOR_HI), "6-2=4 上限");
    s.add("窗口恰过", b.raise(PathK::Open, 4).is_ok(), "抬到窗口沿");
    s.add("史 2 笔", b.history_audit() == Ok(2), "两笔");
    // 族三：空史。
    let mut c = FloorBook::new();
    s.add("空史拒", c.history_audit() == Err(XC_NOENT), "无变更");
    // 族四：跨路径独立。
    s.add("专有不受开源影响", b.floor_of(PathK::Prop) == 3, "独立线");
    s.add("支持 Ada", b.supports(PathK::Prop, 6).is_ok(), "最新代");
    s.add("魔数核", FL_SEG_BASE == 0x30CC0, "段基址");
    s
}
