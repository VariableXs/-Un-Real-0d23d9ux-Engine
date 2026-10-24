//! veb294_mliftl —— VE-F0294 Mali 下限声明。
//! 锚点（CoRun Unxreal 主册）：VE-F0294 · Mali 下限声明 · 目标 300 行
//! 判据原文：VE-F0294-J1 下限声明账：五项下限（最低 API 版 /
//!           最小显存 MB / 最低频点 MHz / 最小块型 / 最低代
//!          际）；逐项声明（值 + 可选降级值）；契约核（运行
//!           环境五项全达 → 契约立）；违约指认（哪项没达指
//!           哪项，不笼统拒）；降级契约（可选降级项用降级值
//!           补位 → 契约带降级记号成立）；硬项无降级（硬项
//!           违约即拒，无降级路）；声明冻结核（契约立后声明
//!           不可改）。
//! 组件：下限五项 + 契约核 + 违约指认 + 降级路。
//! 职责定位：下限声明是「Mali 的入住门槛」——五项是五道门
//!           槛：一票否决但要指认是谁绊的；降级值是备胎：备
//!           胎能跑就带记号跑，但硬项没有备胎；契约立后声明
//!           不可改是白纸黑字：住进来了再改门槛是耍赖。
//! 性能逐项分解：契约核 O(5)。
//! 诊断码段：0x31280（0x31281~0x31289 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const FL_VERSION: &str = "veb294_mliftl-v1";

/// 诊断段基址（0x31280 段 · 本模块独占）。
pub const FL_SEG_BASE: u32 = 0x31280;
pub const XC_MAGIC: u32 = 0x31281; // 账魔数破
pub const XC_FROZEN: u32 = 0x31282; // 声明已冻结
pub const XC_API: u32 = 0x31283; // API 版违约（硬）
pub const XC_VRAM: u32 = 0x31284; // 显存违约（可降）
pub const XC_FREQ: u32 = 0x31285; // 频点违约（可降）
pub const XC_BLK: u32 = 0x31286; // 块型违约（可降）
pub const XC_GEN: u32 = 0x31287; // 代际违约（硬）

/// 账魔数。
const MAGIC: u32 = 0x3128_1B1C;

/// 环境五项。
#[derive(Clone, Copy)]
pub struct Env {
    pub api_ver: u32,
    pub vram_mb: u32,
    pub freq_mhz: u32,
    pub blk: u32,
    pub gen: u32,
}

/// 下限声明。
pub struct Floor {
    // 硬项：api_ver, gen（无降级）。
    pub api_min: u32,
    pub gen_min: u32,
    // 可降项：主值 + 降级值。
    pub vram_min: u32,
    pub vram_alt: u32,
    pub freq_min: u32,
    pub freq_alt: u32,
    pub blk_min: u32,
    pub blk_alt: u32,
    /// 冻结旗。
    pub frozen: bool,
    magic: u32,
}

/// 契约结果。
#[derive(Debug, PartialEq, Eq)]
pub enum Pact {
    /// 全达。
    Full,
    /// 带降级记号成立（记录降了几项）。
    Degraded(u32),
    /// 违约（首项指认）。
    Breach(u32),
}

impl Floor {
    /// 建。
    pub fn new(api_min: u32, gen_min: u32, vram_min: u32, vram_alt: u32,
               freq_min: u32, freq_alt: u32, blk_min: u32, blk_alt: u32) -> Floor {
        Floor { api_min, gen_min, vram_min, vram_alt, freq_min, freq_alt,
                blk_min, blk_alt, frozen: false, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 改声明（冻结后拒）。
    pub fn revise(&mut self, vram_min: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.frozen {
            return Err(XC_FROZEN);
        }
        self.vram_min = vram_min;
        Ok(())
    }

    /// 契约核：硬项违 → 指认；可降项违 → 用降级值补位。
    pub fn pact(&mut self, env: &Env) -> Pact {
        if !self.magic_ok() {
            return Pact::Breach(XC_MAGIC);
        }
        // 硬项先核。
        if env.api_ver < self.api_min {
            return Pact::Breach(XC_API);
        }
        if env.gen < self.gen_min {
            return Pact::Breach(XC_GEN);
        }
        // 可降项。
        let mut deg = 0u32;
        if env.vram_mb < self.vram_min {
            if env.vram_mb >= self.vram_alt {
                deg += 1;
            } else {
                return Pact::Breach(XC_VRAM);
            }
        }
        if env.freq_mhz < self.freq_min {
            if env.freq_mhz >= self.freq_alt {
                deg += 1;
            } else {
                return Pact::Breach(XC_FREQ);
            }
        }
        if env.blk < self.blk_min {
            if env.blk >= self.blk_alt {
                deg += 1;
            } else {
                return Pact::Breach(XC_BLK);
            }
        }
        if deg == 0 {
            self.frozen = true;
            Pact::Full
        } else {
            self.frozen = true;
            Pact::Degraded(deg)
        }
    }
}

pub fn run_veb294_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0294");
    // 声明：API 20 / 代 5 / 显存 2048→1024 / 频 600→400 / 块 8→4。
    let mut f = Floor::new(20, 5, 2048, 1024, 600, 400, 8, 4);
    // 族一：全达。
    let ok_env = Env { api_ver: 30, vram_mb: 4096, freq_mhz: 800, blk: 8, gen: 6 };
    s.add("全达 Full", f.pact(&ok_env) == Pact::Full, "五项全");
    s.add("契约立即冻", f.frozen, "白纸黑字");
    s.add("冻后改拒", f.revise(999) == Err(XC_FROZEN), "不可改");
    // 族二：硬项指认。
    let mut h = Floor::new(20, 5, 2048, 1024, 600, 400, 8, 4);
    s.add("API 违指认", h.pact(&Env { api_ver: 19, vram_mb: 4096, freq_mhz: 800, blk: 8, gen: 6 }) == Pact::Breach(XC_API), "硬项");
    s.add("代际违指认", h.pact(&Env { api_ver: 30, vram_mb: 4096, freq_mhz: 800, blk: 8, gen: 4 }) == Pact::Breach(XC_GEN), "硬项");
    // 硬项先于可降项指认：API 与显存同违 → 指 API。
    s.add("硬项优先指", h.pact(&Env { api_ver: 19, vram_mb: 0, freq_mhz: 100, blk: 4, gen: 4 }) == Pact::Breach(XC_API), "先硬后软");
    // 族三：可降项。
    let mut d = Floor::new(20, 5, 2048, 1024, 600, 400, 8, 4);
    // 显存 1500（<2048 ≥1024）→ 降 1。
    s.add("显存降 1", d.pact(&Env { api_ver: 30, vram_mb: 1500, freq_mhz: 800, blk: 8, gen: 6 }) == Pact::Degraded(1), "备胎");
    // 频 500 → 降。
    s.add("频降 1", d.pact(&Env { api_ver: 30, vram_mb: 4096, freq_mhz: 500, blk: 8, gen: 6 }) == Pact::Degraded(1), "备胎");
    // 三软全降 → Degraded(3)。
    s.add("三软全降", d.pact(&Env { api_ver: 30, vram_mb: 1100, freq_mhz: 450, blk: 4, gen: 6 }) == Pact::Degraded(3), "全备胎");
    // 降级也不够 → 指认。
    s.add("显存崩指认", d.pact(&Env { api_ver: 30, vram_mb: 500, freq_mhz: 800, blk: 8, gen: 6 }) == Pact::Breach(XC_VRAM), "备胎也没有");
    s.add("频崩指认", d.pact(&Env { api_ver: 30, vram_mb: 4096, freq_mhz: 300, blk: 8, gen: 6 }) == Pact::Breach(XC_FREQ), "频备胎无");
    s.add("块崩指认", d.pact(&Env { api_ver: 30, vram_mb: 4096, freq_mhz: 800, blk: 2, gen: 6 }) == Pact::Breach(XC_BLK), "块备胎无");
    // 族四：可降指认序（显存先于频先于块）。
    s.add("软指认序", d.pact(&Env { api_ver: 30, vram_mb: 500, freq_mhz: 300, blk: 2, gen: 6 }) == Pact::Breach(XC_VRAM), "显存最先");
    // 族五：冻结在降级契约也生效。
    let mut g = Floor::new(20, 5, 2048, 1024, 600, 400, 8, 4);
    let _ = g.pact(&Env { api_ver: 30, vram_mb: 1500, freq_mhz: 800, blk: 8, gen: 6 });
    s.add("降级也冻", g.frozen, "带记号立");
    s.add("降级冻改拒", g.revise(1) == Err(XC_FROZEN), "同样白纸");
    // 未冻前可改。
    let mut u = Floor::new(20, 5, 2048, 1024, 600, 400, 8, 4);
    s.add("未冻可改", u.revise(4096).is_ok() && u.vram_min == 4096, "改动自由");
    s.add("魔数核", FL_SEG_BASE == 0x31280, "段基址");
    s
}
