//! veb265_nvdisplay —— VE-F0265 NVIDIA 显示输出。
//! 锚点（CoRun Unxreal 主册）：VE-F0265 · NVIDIA 显示输出 · 目标 400 行
//! 判据原文：VE-F0265-J1 显示头（head）管理：头表（0~3 四头）；
//!           模式集（分辨率 × 刷新率合法组合表）；头绑定屏
//!           （一屏一头，一头一屏，重复绑拒）；模式切换核
//!           （切换时旧模式必须先退）；输出账（活跃头数 + 绑
//!           定屏数一致）。
//! 组件：头表 + 模式表 + 绑定账 + 切换状态机。
//! 职责定位：显示输出是「NVIDIA 通路的脸面」——四个显示头是四
//!           张嘴，一张嘴只许对一块屏；模式切换必须先退后进，
//!           硬切模式等于在扫描输出中途换胶片（花屏之源）；模
//!           式表是白名单：表外组合不猜。
//! 性能逐项分解：绑定 O(头数)；切换 O(1)；账 O(4)。
//! 诊断码段：0x30B40（0x30B41~0x30B4A 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const DP_VERSION: &str = "veb265_nvdisplay-v1";

/// 诊断段基址（0x30B40 段 · 本模块独占）。
pub const DP_SEG_BASE: u32 = 0x30B40;
pub const XC_HEAD_OOR: u32 = 0x30B41; // 头号越域
pub const XC_MODE_BAD: u32 = 0x30B42; // 模式不在表
pub const XC_BIND_DUP: u32 = 0x30B43; // 重复绑定
pub const XC_NOT_BOUND: u32 = 0x30B44; // 未绑定先设模式
pub const XC_MAGIC: u32 = 0x30B45; // 账魔数破
pub const XC_LEDGER: u32 = 0x30B46; // 输出账不平
pub const XC_MODE_LIVE: u32 = 0x30B47; // 模式活跃未退
pub const XC_UNBIND: u32 = 0x30B48; // 未绑先解
pub const XC_SCREEN: u32 = 0x30B49; // 一屏多头
pub const XC_HEADS: u32 = 0x30B4A; // 头数越域

/// 头数。
pub const HEADS: u32 = 4;

/// 显示模式（合法组合表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode {
    pub w: u32,
    pub h: u32,
    pub hz: u32,
}

/// 白名单模式表。
pub const MODE_TABLE: &[Mode] = &[
    Mode { w: 1920, h: 1080, hz: 60 },
    Mode { w: 1920, h: 1080, hz: 144 },
    Mode { w: 2560, h: 1440, hz: 144 },
    Mode { w: 3840, h: 2160, hz: 60 },
    Mode { w: 2560, h: 1440, hz: 60 },
];

/// 头状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Head {
    /// 绑定的屏号（None 未绑）。
    pub screen: Option<u32>,
    /// 当前模式。
    pub mode: Option<Mode>,
    /// 模式活跃（切换中须先退）。
    pub live: bool,
}

/// 显示输出管理器。
pub struct DisplayMgr {
    heads: [Head; 4],
    magic: u32,
}

impl DisplayMgr {
    /// 建。
    pub fn new() -> DisplayMgr {
        DisplayMgr {
            heads: [
                Head { screen: None, mode: None, live: false },
                Head { screen: None, mode: None, live: false },
                Head { screen: None, mode: None, live: false },
                Head { screen: None, mode: None, live: false },
            ],
            magic: 0x0B40_015C,
        }
    }

    /// 模式表核。
    pub fn mode_ok(m: Mode) -> bool {
        for t in MODE_TABLE.iter() {
            if *t == m {
                return true;
            }
        }
        false
    }

    /// 绑定屏。
    pub fn bind(&mut self, head: u32, screen: u32) -> Result<(), u32> {
        if self.magic != 0x0B40_015C {
            return Err(XC_MAGIC);
        }
        if head >= HEADS {
            return Err(XC_HEAD_OOR);
        }
        // 一屏一头。
        for (i, h) in self.heads.iter().enumerate() {
            if h.screen == Some(screen) && i as u32 != head {
                return Err(XC_SCREEN);
            }
        }
        if self.heads[head as usize].screen.is_some() {
            return Err(XC_BIND_DUP);
        }
        self.heads[head as usize].screen = Some(screen);
        Ok(())
    }

    /// 解绑（须先退模式）。
    pub fn unbind(&mut self, head: u32) -> Result<(), u32> {
        if head >= HEADS {
            return Err(XC_HEAD_OOR);
        }
        let h = &mut self.heads[head as usize];
        if h.screen.is_none() {
            return Err(XC_UNBIND);
        }
        if h.live {
            return Err(XC_MODE_LIVE);
        }
        h.screen = None;
        h.mode = None;
        Ok(())
    }

    /// 设模式（须已绑定；切模式须先退旧）。
    pub fn set_mode(&mut self, head: u32, m: Mode) -> Result<(), u32> {
        if head >= HEADS {
            return Err(XC_HEAD_OOR);
        }
        if !Self::mode_ok(m) {
            return Err(XC_MODE_BAD);
        }
        let h = &mut self.heads[head as usize];
        if h.screen.is_none() {
            return Err(XC_NOT_BOUND);
        }
        if h.live {
            return Err(XC_MODE_LIVE);
        }
        h.mode = Some(m);
        h.live = true;
        Ok(())
    }

    /// 退模式。
    pub fn retire(&mut self, head: u32) -> Result<(), u32> {
        if head >= HEADS {
            return Err(XC_HEAD_OOR);
        }
        let h = &mut self.heads[head as usize];
        if !h.live {
            return Err(XC_MODE_LIVE);
        }
        h.live = false;
        Ok(())
    }

    /// 输出账：活跃头数 = 绑定且 live 的头数；绑定数核。
    pub fn audit(&self) -> Result<(u32, u32), u32> {
        if self.magic != 0x0B40_015C {
            return Err(XC_MAGIC);
        }
        let mut bound = 0u32;
        let mut live = 0u32;
        for h in self.heads.iter() {
            if h.screen.is_some() {
                bound += 1;
            }
            if h.live {
                live += 1;
            }
        }
        if live > bound {
            return Err(XC_LEDGER);
        }
        Ok((bound, live))
    }

    /// 活跃头数。
    pub fn live_count(&self) -> u32 {
        let mut n = 0u32;
        for h in self.heads.iter() {
            if h.live {
                n += 1;
            }
        }
        n
    }
}

pub fn run_veb265_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0265");
    // 族零：模式表。
    s.add("1080p60 在表", DisplayMgr::mode_ok(Mode { w: 1920, h: 1080, hz: 60 }), "白名单");
    s.add("1080p75 不在", !DisplayMgr::mode_ok(Mode { w: 1920, h: 1080, hz: 75 }), "表外拒");
    s.add("4K60 在表", DisplayMgr::mode_ok(Mode { w: 3840, h: 2160, hz: 60 }), "白名单");
    // 族一：绑定。
    let mut d = DisplayMgr::new();
    s.add("头 0 绑屏 0", d.bind(0, 0).is_ok(), "首绑");
    s.add("头 1 绑屏 1", d.bind(1, 1).is_ok(), "次绑");
    s.add("一屏多头拒", d.bind(2, 0) == Err(XC_SCREEN), "屏 0 已占");
    s.add("重绑拒", d.bind(0, 2) == Err(XC_BIND_DUP), "头 0 已绑");
    s.add("越头拒", d.bind(4, 4) == Err(XC_HEAD_OOR), "0~3");
    s.add("账 2+0", d.audit() == Ok((2, 0)), "绑二活零");
    // 族二：设模式。
    let m = Mode { w: 2560, h: 1440, hz: 144 };
    s.add("未绑设模式拒", d.set_mode(2, m) == Err(XC_NOT_BOUND), "先绑后设");
    s.add("设模式过", d.set_mode(0, m).is_ok(), "头 0 活");
    s.add("账 2+1", d.audit() == Ok((2, 1)), "一活");
    s.add("活模式切换拒", d.set_mode(0, Mode { w: 1920, h: 1080, hz: 60 }) == Err(XC_MODE_LIVE), "先退后切");
    s.add("退模式", d.retire(0).is_ok(), "退");
    s.add("退后再退拒", d.retire(0) == Err(XC_MODE_LIVE), "已退");
    s.add("切换过", d.set_mode(0, Mode { w: 1920, h: 1080, hz: 60 }).is_ok(), "新模式");
    // 族三：解绑。
    s.add("活时解绑拒", d.unbind(0) == Err(XC_MODE_LIVE), "先退");
    let _ = d.retire(0);
    s.add("退后解绑过", d.unbind(0).is_ok(), "回收");
    s.add("未绑解拒", d.unbind(0) == Err(XC_UNBIND), "已空");
    s.add("账 1+0", d.audit() == Ok((1, 0)), "复平");
    // 族四：四头全活。
    let mut f = DisplayMgr::new();
    for i in 0..4u32 {
        let _ = f.bind(i, i);
        let _ = f.set_mode(i, Mode { w: 1920, h: 1080, hz: 60 });
    }
    s.add("四头全活", f.live_count() == 4, "满载");
    s.add("账 4+4", f.audit() == Ok((4, 4)), "全屏");
    // 屏号可不同于头号。
    let mut g = DisplayMgr::new();
    let _ = g.bind(0, 3);
    s.add("头 0 绑屏 3 过", g.audit() == Ok((1, 0)), "屏号独立");
    s.add("屏 3 再绑拒", g.bind(1, 3) == Err(XC_SCREEN), "一屏一头");
    s.add("魔数核", DP_SEG_BASE == 0x30B40, "段基址");
    s
}
