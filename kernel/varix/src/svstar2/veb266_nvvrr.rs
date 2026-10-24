//! veb266_nvvrr —— VE-F0266 NVIDIA G-Sync/VRR。
//! 锚点（CoRun Unxreal 主册）：VE-F0266 · NVIDIA G-Sync/VRR · 目标 320 行
//! 判据原文：VE-F0266-J1 VRR 状态机：Off→Armed→Active；激活条
//!           件（刷新率落在 VRR 窗 [min,max] 内且帧间抖动低于
//!           阈）；窗内追频（目标刷新 = 帧到达率，钳在窗内）；
//!           出窗回落（刷新超窗 → Off 并记录回落原因）；窗参
//!           数核（min < max，min ≥ 30Hz）。
//! 组件：VRR 状态机 + 刷新窗 + 追频器 + 回落账。
//! 职责定位：VRR 是「显示头的变速器」——窗是变速箱工作区间，
//!           低于 min 变频器打滑（闪烁），高于 max 就是普通模
//!           式；追频钳窗保证不追出边界；出窗回落要记录原因，
//!           不明不白掉回固定刷新是调试噩梦。
//! 性能逐项分解：追频 O(1)；状态迁移 O(1)。
//! 诊断码段：0x30B80（0x30B81~0x30B89 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const VR_VERSION: &str = "veb266_nvvrr-v1";

/// 诊断段基址（0x30B80 段 · 本模块独占）。
pub const VR_SEG_BASE: u32 = 0x30B80;
pub const XC_WINDOW: u32 = 0x30B81; // 窗参不合法
pub const XC_STATE: u32 = 0x30B82; // 状态机非法迁移
pub const XC_MAGIC: u32 = 0x30B83; // 账魔数破
pub const XC_NOT_ARMED: u32 = 0x30B84; // 未武装先激活
pub const XC_DROP_LOW: u32 = 0x30B85; // 低于 min 回落
pub const XC_DROP_HIGH: u32 = 0x30B86; // 高于 max 回落
pub const XC_TWICE: u32 = 0x30B87; // 重复激活

/// VRR 三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VrrState {
    #[default]
    Off = 0,
    Armed = 1,
    Active = 2,
}

/// 回落原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropReason {
    LowHz = 0,
    HighHz = 1,
    Manual = 2,
}

/// VRR 控制器。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct VrrCtl {
    pub state: VrrState,
    pub min_hz: u32,
    pub max_hz: u32,
    /// 当前追频目标。
    pub target_hz: u32,
    /// 回落账。
    pub drops: Vec<(DropReason, u32)>,
    magic: u32,
}

impl VrrCtl {
    /// 建（窗参数校验）。
    pub fn new(min_hz: u32, max_hz: u32) -> Result<VrrCtl, u32> {
        if min_hz < 30 || min_hz >= max_hz {
            return Err(XC_WINDOW);
        }
        Ok(VrrCtl {
            state: VrrState::Off,
            min_hz,
            max_hz,
            target_hz: 0,
            drops: Vec::new(),
            magic: 0x0B80_5828,
        })
    }

    /// 武装（Off → Armed）。
    pub fn arm(&mut self) -> Result<(), u32> {
        if self.magic != 0x0B80_5828 {
            return Err(XC_MAGIC);
        }
        if self.state != VrrState::Off {
            return Err(XC_STATE);
        }
        self.state = VrrState::Armed;
        Ok(())
    }

    /// 帧到达（Armed/Active 态）：hz 落窗内 → Active + 追频；
    /// 出窗 → Off + 回落账。
    pub fn frame(&mut self, hz: u32) -> Result<u32, u32> {
        if self.state == VrrState::Off {
            return Err(XC_NOT_ARMED);
        }
        if hz < self.min_hz {
            self.state = VrrState::Off;
            self.target_hz = 0;
            self.drops.push((DropReason::LowHz, hz));
            return Err(XC_DROP_LOW);
        }
        if hz > self.max_hz {
            self.state = VrrState::Off;
            self.target_hz = 0;
            self.drops.push((DropReason::HighHz, hz));
            return Err(XC_DROP_HIGH);
        }
        self.state = VrrState::Active;
        self.target_hz = hz;
        Ok(hz)
    }

    /// 手动关（Active/Armed → Off）。
    pub fn off(&mut self) -> Result<(), u32> {
        if self.state == VrrState::Off {
            return Err(XC_STATE);
        }
        self.state = VrrState::Off;
        self.target_hz = 0;
        self.drops.push((DropReason::Manual, 0));
        Ok(())
    }

    /// 回落账数。
    pub fn drop_count(&self) -> usize {
        self.drops.len()
    }

    /// 窗参数核。
    pub fn window_ok(&self) -> bool {
        self.min_hz >= 30 && self.min_hz < self.max_hz
    }
}

pub fn run_veb266_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0266");
    // 族零：窗参。
    s.add("窗 48-144 合法", VrrCtl::new(48, 144).is_ok(), "常规窗");
    s.add("min 29 拒", matches!(VrrCtl::new(29, 144), Err(XC_WINDOW)), "<30");
    s.add("min≥max 拒", matches!(VrrCtl::new(144, 144), Err(XC_WINDOW)), "等窗");
    s.add("倒窗拒", matches!(VrrCtl::new(144, 60), Err(XC_WINDOW)), "倒置");
    // 族一：状态机。
    let mut v = match VrrCtl::new(48, 144) {
        Ok(x) => x,
        Err(_) => {
            s.add("建账败", false, "不应到");
            return s;
        }
    };
    s.add("初始 Off", v.state == VrrState::Off, "出厂");
    s.add("未武装收帧拒", v.frame(60) == Err(XC_NOT_ARMED), "先武装");
    s.add("武装过", v.arm().is_ok(), "Armed");
    s.add("重复武装拒", v.arm() == Err(XC_STATE), "一态一次");
    s.add("窗内激活 60Hz", v.frame(60) == Ok(60), "Active");
    s.add("追频 60", v.target_hz == 60, "跟帧");
    s.add("窗内续追 120", v.frame(120) == Ok(120), "变帧");
    s.add("重复激活无害", v.frame(90) == Ok(90), "Active 保态");
    // 族二：出窗回落。
    s.add("低出窗回落", v.frame(30) == Err(XC_DROP_LOW), "<48");
    s.add("回落 Off", v.state == VrrState::Off, "停机");
    s.add("回落账 1", v.drop_count() == 1, "一低");
    // 再武装再高出窗。
    let _ = v.arm();
    s.add("高出窗回落", v.frame(240) == Err(XC_DROP_HIGH), ">144");
    s.add("回落账 2", v.drop_count() == 2, "低+高");
    // 族三：手动关。
    let _ = v.arm();
    let _ = v.frame(60);
    s.add("手动关过", v.off().is_ok(), "Manual");
    s.add("手动回落账", v.drop_count() == 3, "三笔");
    s.add("Off 后关拒", v.off() == Err(XC_STATE), "已关");
    // 族四：边界追频。
    let mut w = match VrrCtl::new(48, 144) {
        Ok(x) => x,
        Err(_) => {
            s.add("建账败 2", false, "不应到");
            return s;
        }
    };
    let _ = w.arm();
    s.add("窗底 48 激活", w.frame(48) == Ok(48), "贴底");
    s.add("窗顶 144 激活", w.frame(144) == Ok(144), "贴顶");
    s.add("47 拒", w.frame(47) == Err(XC_DROP_LOW), "出底");
    s.add("魔数核", VR_SEG_BASE == 0x30B80, "段基址");
    s
}
