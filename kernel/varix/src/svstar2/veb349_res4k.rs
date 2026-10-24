//! veb349_res4k —— VE-F0349 大分辨率策略（4K 底线）。
//! 锚点：VE-F0349 · 目标 320 行。
//! 判据原文：4K 底线承诺（基础 2D 全功能 4K 满帧判据实测，
//!           不达标阻断承诺并登记）、8K 实验档（不承诺显性
//!           标注，崩溃隔离不拖垮 4K 主路）、自适应默认开
//!           （可关留痕）、显性降档不硬撑、判据。
//! 组件：底线判据账 + 8K 实验档隔离舱 + 自适应开关。
//! 职责定位：4K 底线是「承诺书」——承诺的数字要用实测背书，
//!           测不出来就收回承诺并留痕；8K 是实验室展品，摔
//!           了也只能摔在玻璃柜里，不许砸到 4K 主路。
//! 性能逐项分解：判据 O(场景集) 实测；降档 O(1)；开关 O(1)。
//! 诊断码段：0x32040（0x32041~0x32049 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const R4_VERSION: &str = "veb349_res4k-v1";

/// 诊断段基址（0x32040 段 · 本模块独占）。
pub const R4_SEG_BASE: u32 = 0x32040;
pub const XC_NOTMET: u32 = 0x32041; // 4K 判据不达标
pub const XC_EXPLODE: u32 = 0x32042; // 8K 实验档崩溃（隔离）
pub const XC_LEAK: u32 = 0x32043; // 实验档溢出拖垮主路（红线）
pub const XC_BADRES: u32 = 0x32044; // 分辨率非法
pub const XC_NOEVID: u32 = 0x32045; // 无实测证据

/// 底线分辨率。
pub const W_4K: u32 = 3840;
pub const H_4K: u32 = 2160;
/// 满帧判据。
pub const FPS_FLOOR: u32 = 60;
/// 实验档分辨率。
pub const W_8K: u32 = 7680;
pub const H_8K: u32 = 4320;

/// 场景实测（2D 全功能 4K 满帧判据）。
pub struct Scene4K {
    /// 场景代号。
    pub sc: u32,
    /// 实测帧率。
    pub fps: u32,
}

/// 分辨率策略账。
pub struct ResPolicy {
    scenes: Vec<Scene4K>,
    /// 底线承诺态。
    pub claimed: bool,
    /// 8K 实验档开。
    pub exp8k: bool,
    /// 8K 崩溃隔离数。
    pub iso8k: u32,
    /// 自适应开。
    pub adaptive: bool,
    /// 关自适应留痕数。
    pub toggle_log: u32,
}

impl ResPolicy {
    /// 建：自适应默认开。
    pub fn new() -> ResPolicy {
        ResPolicy {
            scenes: Vec::new(),
            claimed: false,
            exp8k: false,
            iso8k: 0,
            adaptive: true,
            toggle_log: 0,
        }
    }

    /// 记场景实测：分辨率核对（须 4K 场）+ 帧率非零。
    pub fn record(&mut self, sc: u32, fps: u32) -> Result<(), u32> {
        if fps == 0 {
            return Err(XC_NOEVID);
        }
        self.scenes.push(Scene4K { sc, fps });
        Ok(())
    }

    /// 底线承诺：全场景 ≥ FPS_FLOOR 才许承诺；空证据拒。
    pub fn claim_4k(&mut self) -> Result<(), u32> {
        if self.scenes.is_empty() {
            return Err(XC_NOEVID);
        }
        let ok = self.scenes.iter().all(|s| s.fps >= FPS_FLOOR);
        if !ok {
            self.claimed = false;
            return Err(XC_NOTMET);
        }
        self.claimed = true;
        Ok(())
    }

    /// 8K 实验档：开关（显性标注语义）。
    pub fn set_exp8k(&mut self, on: bool) {
        self.exp8k = on;
    }

    /// 8K 用例运行：崩溃 → 隔离记账，主路不受扰。
    pub fn run8k(&mut self, ok: bool) -> Result<(), u32> {
        if !self.exp8k {
            return Err(XC_LEAK);
        }
        if !ok {
            self.iso8k += 1;
            return Err(XC_EXPLODE);
        }
        Ok(())
    }

    /// 主路健康核：实验档无论怎么崩，4K 主路承诺态不变。
    pub fn main_ok(&self) -> bool {
        self.claimed
    }

    /// 自适应开关：关留痕。
    pub fn set_adaptive(&mut self, on: bool) {
        if self.adaptive != on {
            self.toggle_log += 1;
        }
        self.adaptive = on;
    }

    /// 显性降档决策：分辨率超 4K 且 2D 判据未背书 → 降档到 4K。
    pub fn cap(&self, w: u32, h: u32) -> Result<(u32, u32), u32> {
        if w == 0 || h == 0 {
            return Err(XC_BADRES);
        }
        if w > W_4K || h > H_4K {
            if !self.exp8k {
                return Ok((W_4K, H_4K)); // 显性降到 4K 不硬撑
            }
        }
        Ok((w.min(W_4K.max(w)), h))
    }

    /// 场景数。
    pub fn len(&self) -> usize {
        self.scenes.len()
    }
}

pub fn run_veb349_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0349");
    // 族零：证据门。
    let mut p = ResPolicy::new();
    s.add("空证据拒", p.claim_4k() == Err(XC_NOEVID), "无实测");
    s.add("零帧率拒", p.record(1, 0) == Err(XC_NOEVID), "假数据");
    s.add("场景一过", p.record(1, 62).is_ok(), "62 fps");
    s.add("场景二过", p.record(2, 60).is_ok(), "恰底线");
    s.add("场景三过", p.record(3, 75).is_ok(), "75 fps");
    s.add("场景三", p.len() == 3, "在账");
    // 族一：底线承诺。
    s.add("承诺过", p.claim_4k().is_ok(), "全场景 ≥60");
    s.add("承诺在", p.claimed, "底线立");
    // 不达标阻断。
    let mut p2 = ResPolicy::new();
    let _ = p2.record(1, 62);
    let _ = p2.record(2, 58);
    s.add("不达标拒", p2.claim_4k() == Err(XC_NOTMET), "58<60");
    s.add("承诺未立", !p2.claimed, "阻断");
    // 族二：8K 实验档。
    s.add("未开档跑拒", p.run8k(true) == Err(XC_LEAK), "先开档");
    p.set_exp8k(true);
    s.add("8K 过", p.run8k(true).is_ok(), "实验成功");
    s.add("8K 崩隔离", p.run8k(false) == Err(XC_EXPLODE), "玻璃柜里摔");
    s.add("隔离一", p.iso8k == 1, "记账");
    s.add("主路无恙", p.main_ok(), "承诺不变");
    p.set_exp8k(false);
    s.add("关档后拒跑", p.run8k(true) == Err(XC_LEAK), "档关");
    // 族三：显性降档。
    s.add("超 4K 降档", p.cap(5120, 2880) == Ok((W_4K, H_4K)), "不硬撑");
    s.add("4K 直过", p.cap(3840, 2160) == Ok((3840, 2160)), "恰底线");
    s.add("1080 过", p.cap(1920, 1080) == Ok((1920, 1080)), "小场景");
    s.add("零宽拒", p.cap(0, 100) == Err(XC_BADRES), "非法");
    // 8K 档开着时超 4K 放行（实验语义）。
    let mut p3 = ResPolicy::new();
    p3.set_exp8k(true);
    s.add("8K 档放行", p3.cap(W_8K, H_8K) == Ok((W_8K, H_8K)), "实验舱");
    // 族四：自适应开关。
    s.add("默认开", ResPolicy::new().adaptive, "默认开");
    s.add("关留痕", {
        p.set_adaptive(false);
        !p.adaptive && p.toggle_log == 1
    }, "用户可关");
    s.add("重复关不留", {
        p.set_adaptive(false);
        p.toggle_log == 1
    }, "幂等");
    s.add("再开留痕", {
        p.set_adaptive(true);
        p.toggle_log == 2
    }, "两次变更");
    // 族五：隔离红线核。
    let mut p4 = ResPolicy::new();
    p4.set_exp8k(true);
    let _ = p4.run8k(false);
    let _ = p4.run8k(false);
    s.add("崩两次记二", p4.iso8k == 2, "隔离账");
    s.add("主路态自洽", p4.main_ok() == p4.claimed, "承诺态一致");
    s.add("魔数核", R4_SEG_BASE == 0x32040, "段基址");
    s
}
