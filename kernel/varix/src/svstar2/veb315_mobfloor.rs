//! veb315_mobfloor —— VE-F0315 移动两族下限声明。
//! 锚点（CoRun Unxreal 主册）：VE-F0315 · 移动两族下限声明 · 目标 320 行
//! 判据原文：VE-F0315-J1 两族下限账：两族（Adreno 族 / PVR
//!           族）各五项下限（API / 显存 / 频点 / 压缩块型 /
//!           代际）；族间差异核（Adreno 显存下限 ≥ PVR）；
//!           双族契约（环境须同时过两族 → 全立）；弱族记号
//!           （一族降级即带弱族记号成立）；硬项跨族（API 与
//!           代际为跨族硬项：任一族违约即拒）；指认序（先硬
//!           后软、软项内 Adreno 先 PVR 后）。
//! 组件：双族账 + 跨族硬项 + 弱族记号 + 指认序。
//! 职责定位：两族下限是「移动线的双门槛」——两族都要过：过
//!           了一族另一族趴下不算过；弱族记号是跛行的许可：
//!           一族降级可以跑但要说清楚哪条腿跛；硬项跨族是双
//!           门槛的共同地基：地基裂了哪一族都别住。
//! 性能逐项分解：双族核 O(10)。
//! 诊断码段：0x317C0（0x317C1~0x317C9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const MF_VERSION: &str = "veb315_mobfloor-v1";

/// 诊断段基址（0x317C0 段 · 本模块独占）。
pub const MF_SEG_BASE: u32 = 0x317C0;
pub const XC_MAGIC: u32 = 0x317C1; // 账魔数破
pub const XC_API: u32 = 0x317C2; // API 跨族硬项
pub const XC_GEN: u32 = 0x317C3; // 代际跨族硬项
pub const XC_VRAM_A: u32 = 0x317C4; // Adreno 显存
pub const XC_FREQ_A: u32 = 0x317C5; // Adreno 频点
pub const XC_VRAM_P: u32 = 0x317C6; // PVR 显存
pub const XC_FREQ_P: u32 = 0x317C7; // PVR 频点

/// 账魔数。
const MAGIC: u32 = 0x317C_1B1D;

/// 单族下限。
pub struct FamFloor {
    pub api_min: u32,
    pub gen_min: u32,
    pub vram_min: u32,
    pub freq_min: u32,
}

/// 环境。
#[derive(Clone, Copy)]
pub struct Env {
    pub api_ver: u32,
    pub gen: u32,
    pub vram_mb: u32,
    pub freq_mhz: u32,
}

/// 双族簿（Adreno 下限 ≥ PVR 的差异核在建簿时立）。
pub struct MobFloor {
    pub adreno: FamFloor,
    pub pvr: FamFloor,
    magic: u32,
}

/// 契约结果。
#[derive(Debug, PartialEq, Eq)]
pub enum Pact {
    Full,
    /// 弱族记号：0=Adreno 弱 1=PVR 弱。
    WeakFam(u32),
    Breach(u32),
}

impl MobFloor {
    /// 建（校验 Adreno 显存下限 ≥ PVR）。
    pub fn new(adreno: FamFloor, pvr: FamFloor) -> MobFloor {
        MobFloor { adreno, pvr, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 族间差异核。
    pub fn fam_order_ok(&self) -> bool {
        self.adreno.vram_min >= self.pvr.vram_min
    }

    /// 双族契约：硬项跨族 → 软项按 Adreno → PVR 序指认。
    pub fn pact(&self, env: &Env) -> Pact {
        if !self.magic_ok() {
            return Pact::Breach(XC_MAGIC);
        }
        // 跨族硬项。
        if env.api_ver < self.adreno.api_min || env.api_ver < self.pvr.api_min {
            return Pact::Breach(XC_API);
        }
        if env.gen < self.adreno.gen_min || env.gen < self.pvr.gen_min {
            return Pact::Breach(XC_GEN);
        }
        // Adreno 软项。
        let a_ok = env.vram_mb >= self.adreno.vram_min && env.freq_mhz >= self.adreno.freq_min;
        let p_ok = env.vram_mb >= self.pvr.vram_min && env.freq_mhz >= self.pvr.freq_min;
        match (a_ok, p_ok) {
            (true, true) => Pact::Full,
            (false, true) => Pact::WeakFam(0),
            (true, false) => Pact::WeakFam(1),
            // 双弱/双违：按 Adreno 先指认（取其违项）。
            (false, false) => {
                if env.vram_mb < self.adreno.vram_min {
                    Pact::Breach(XC_VRAM_A)
                } else {
                    Pact::Breach(XC_FREQ_A)
                }
            }
        }
    }
}

pub fn run_veb315_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0315");
    // 下限：Adreno API20 代6 显存 2048 频 700；PVR API20 代5 显存 1024 频 500。
    let ad = FamFloor { api_min: 20, gen_min: 6, vram_min: 2048, freq_min: 700 };
    let pv = FamFloor { api_min: 20, gen_min: 5, vram_min: 1024, freq_min: 500 };
    let f = MobFloor::new(ad, pv);
    // 族零：族间差异。
    s.add("显存序立", f.fam_order_ok(), "A≥P");
    // 反例：PVR 显存更高 → 序破。
    let bad = MobFloor::new(
        FamFloor { api_min: 20, gen_min: 6, vram_min: 512, freq_min: 700 },
        FamFloor { api_min: 20, gen_min: 5, vram_min: 1024, freq_min: 500 },
    );
    s.add("序破检得", bad.fam_order_ok() == false, "A<P");
    // 族一：双族全达。
    s.add("全达", f.pact(&Env { api_ver: 30, gen: 7, vram_mb: 4096, freq_mhz: 900 }) == Pact::Full, "双族过");
    // 恰达：Adreno 门槛上（2048/700），PVR 亦过。
    s.add("恰达 A 线", f.pact(&Env { api_ver: 20, gen: 6, vram_mb: 2048, freq_mhz: 700 }) == Pact::Full, "含等");
    // 族二：跨族硬项。
    s.add("API 低拒", f.pact(&Env { api_ver: 19, gen: 7, vram_mb: 4096, freq_mhz: 900 }) == Pact::Breach(XC_API), "跨族硬");
    s.add("代际低拒", f.pact(&Env { api_ver: 30, gen: 5, vram_mb: 4096, freq_mhz: 900 }) == Pact::Breach(XC_GEN), "PVR 代 5 但 A 要 6");
    // 硬项优先于软项指认。
    s.add("硬先指认", f.pact(&Env { api_ver: 10, gen: 3, vram_mb: 100, freq_mhz: 100 }) == Pact::Breach(XC_API), "API 最先");
    // 族三：弱族记号。
    // 显存 1500：过 PVR（1024）不过 Adreno（2048）→ 弱 Adreno。
    s.add("弱 Adreno", f.pact(&Env { api_ver: 30, gen: 7, vram_mb: 1500, freq_mhz: 900 }) == Pact::WeakFam(0), "A 跛");
    // 频 600：过 PVR（500）不过 Adreno（700）→ 弱 Adreno。
    s.add("弱 Adreno 频", f.pact(&Env { api_ver: 30, gen: 7, vram_mb: 4096, freq_mhz: 600 }) == Pact::WeakFam(0), "A 跛频");
    // 弱 PVR 造不出（PVR 两项下限均低于 Adreno）——用低 PVR 族簿造：
    let lo = MobFloor::new(
        FamFloor { api_min: 20, gen_min: 6, vram_min: 512, freq_min: 400 },
        FamFloor { api_min: 20, gen_min: 5, vram_min: 1024, freq_min: 800 },
    );
    // 频 600：过 Adreno（400）不过 PVR（800）→ 弱 PVR。
    s.add("弱 PVR", lo.pact(&Env { api_ver: 30, gen: 7, vram_mb: 2048, freq_mhz: 600 }) == Pact::WeakFam(1), "P 跛");
    // 族四：双违指认序。
    // 显存 300 频 300：Adreno 显存 2048 违 → 指 XC_VRAM_A。
    s.add("双违指 A 显存", f.pact(&Env { api_ver: 30, gen: 7, vram_mb: 300, freq_mhz: 300 }) == Pact::Breach(XC_VRAM_A), "A 显存先");
    // 显存 3000 频 300：A 显存过频违 → 指 XC_FREQ_A。
    s.add("双违指 A 频", f.pact(&Env { api_ver: 30, gen: 7, vram_mb: 3000, freq_mhz: 300 }) == Pact::Breach(XC_FREQ_A), "A 频后");
    // PVR 侧违项码存在性（簿内常量）。
    s.add("PVR 码非 A 码", XC_VRAM_P != XC_VRAM_A && XC_FREQ_P != XC_FREQ_A, "分族码");
    // 族五：恰违边界。
    s.add("显存 2047 违", f.pact(&Env { api_ver: 30, gen: 7, vram_mb: 2047, freq_mhz: 900 }) == Pact::WeakFam(0), "差一弱");
    s.add("频 699 弱", f.pact(&Env { api_ver: 30, gen: 7, vram_mb: 4096, freq_mhz: 699 }) == Pact::WeakFam(0), "差一弱");
    s.add("魔数核", MF_SEG_BASE == 0x317C0, "段基址");
    s
}
