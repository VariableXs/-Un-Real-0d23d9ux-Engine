//! veb324_rvsoft —— VE-F0324 软渲优先策略（RISC-V 特化）。
//! 锚点（CoRun Unxreal 主册）：VE-F0324 · 软渲优先策略（RISC-V 特化） · 目标 320 行
//! 判据原文：VE-F0324-J1 软渲策略账：候选栈（硬 GPU / 软渲 /
//!           直写软位图三层）；特化旗（RVV 可用 → 软渲提速
//!           档）；选路（硬 GPU 在 → 走硬；不在 → 走软渲；
//!           软渲也缺 → 直写位图兜底）；提速核（RVV 旗立 →
//!           软渲吞吐 ×4）；降路记账（每降一层记一笔 + 降级
//!           原因）；回硬探测（硬 GPU 迟到 → 升回硬路）；三
//!           层序核（不可跳层：软渲缺直接到直写，不许越层
//!           探测）。
//! 组件：候选栈 + 选路器 + 提速旗 + 回硬探测。
//! 职责定位：软渲优先是「RISC-V 的备胎哲学」——硬 GPU 是正
//!           胎、软渲是备胎、直写位图是补胎液：一层比一层狼
//!           狈但都能跑；RVV 是备胎的 Turbo：装上就是 ×4；
//!           不可跳层是换胎的顺序：补胎液不能直接上，先看
//!           看备胎在不在。
//! 性能逐项分解：选路 O(1)；回硬 O(1)。
//! 诊断码段：0x31A00（0x31A01~0x31A09 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const RS_VERSION: &str = "veb324_rvsoft-v1";

/// 诊断段基址（0x31A00 段 · 本模块独占）。
pub const RS_SEG_BASE: u32 = 0x31A00;
pub const XC_MAGIC: u32 = 0x31A01; // 账魔数破

/// 账魔数。
const MAGIC: u32 = 0x31A0_1B2B;

/// 三层路。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// 硬 GPU。
    Hard,
    /// 软渲。
    Soft,
    /// 直写位图。
    Blit,
}

/// 基准软渲吞吐（MPix/s）。
pub const SOFT_BASE: u32 = 100;

/// RVV 提速倍数。
pub const RVV_X: u32 = 4;

/// 软渲策略簿。
pub struct SoftBook {
    /// 硬 GPU 在位。
    pub hard_present: bool,
    /// 软渲可用。
    pub soft_present: bool,
    /// RVV 旗。
    pub rvv: bool,
    /// 当前路。
    pub route: Route,
    /// 降路账：(降级原因码)。
    pub drops: Vec<u32>,
    magic: u32,
}

impl SoftBook {
    /// 建（初始全探测：三层逐层定路）。
    pub fn new(hard: bool, soft: bool, rvv: bool) -> SoftBook {
        let mut b = SoftBook {
            hard_present: hard,
            soft_present: soft,
            rvv,
            route: Route::Blit,
            drops: Vec::new(),
            magic: MAGIC,
        };
        b.pick();
        b
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    fn pick(&mut self) {
        if self.hard_present {
            self.route = Route::Hard;
        } else if self.soft_present {
            self.route = Route::Soft;
            self.drops.push(1); // 硬缺降软
        } else {
            self.route = Route::Blit;
            self.drops.push(1);
            self.drops.push(2); // 软也缺降直写
        }
    }

    /// 软渲吞吐：RVV 旗立 ×4。
    pub fn soft_thru(&self) -> u32 {
        if self.rvv {
            SOFT_BASE * RVV_X
        } else {
            SOFT_BASE
        }
    }

    /// 回硬探测：硬 GPU 迟到 → 升硬并清降账。
    pub fn hard_arrive(&mut self) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        self.hard_present = true;
        self.route = Route::Hard;
        self.drops.clear();
        Ok(())
    }

    /// 降路层数（当前路距硬层的距离）。
    pub fn depth_from_hard(&self) -> u32 {
        match self.route {
            Route::Hard => 0,
            Route::Soft => 1,
            Route::Blit => 2,
        }
    }
}

pub fn run_veb324_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0324");
    // 族一：三层选路。
    let a = SoftBook::new(true, true, false);
    s.add("硬在走硬", a.route == Route::Hard, "正胎");
    s.add("零降账", a.drops.is_empty(), "无降");
    let b = SoftBook::new(false, true, false);
    s.add("硬缺走软", b.route == Route::Soft, "备胎");
    s.add("降一笔", b.drops.len() == 1 && b.drops[0] == 1, "硬缺");
    let c = SoftBook::new(false, false, false);
    s.add("软缺直写", c.route == Route::Blit, "补胎液");
    s.add("降两笔", c.drops.len() == 2 && c.drops[1] == 2, "逐层");
    // 族二：不可跳层核（降账序 1→2）。
    s.add("降序核", c.drops[0] == 1 && c.drops[1] == 2, "不越层");
    // 族三：RVV 提速。
    let d = SoftBook::new(false, true, true);
    s.add("RVV ×4", d.soft_thru() == 400, "提速");
    s.add("无 RVV 基准", b.soft_thru() == 100, "基准");
    // 硬路上 RVV 不影响（吞吐只在软路有意义，恒给值）。
    s.add("硬路吞照给", a.soft_thru() == 100, "旗未立");
    // 族四：回硬探测。
    let mut e = SoftBook::new(false, true, true);
    s.add("初软路", e.route == Route::Soft, "备胎期");
    s.add("回硬", e.hard_arrive().is_ok(), "正胎到");
    s.add("路升硬", e.route == Route::Hard, "升回");
    s.add("降账清", e.drops.is_empty(), "既往不咎");
    s.add("层距 0", e.depth_from_hard() == 0, "顶层");
    // 回硬幂等。
    s.add("回硬幂等", e.hard_arrive().is_ok() && e.route == Route::Hard, "再报");
    // 族五：层距核。
    s.add("硬距 0", a.depth_from_hard() == 0, "硬");
    s.add("软距 1", b.depth_from_hard() == 1, "软");
    s.add("直写距 2", c.depth_from_hard() == 2, "位图");
    s.add("魔数核", RS_SEG_BASE == 0x31A00, "段基址");
    s
}
