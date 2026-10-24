//! veb356_hwfloor —— VE-F0356 软渲下限硬件声明。
//! 锚点：VE-F0356 · 目标 300 行。
//! 判据原文：SSE4 底线（最低 SSE4.2，缺指令集显性拒绝三要
//!           素）、档位标注（推荐 AVX2，性能档按指令集分档）、
//!           显性拒绝（下限以下设备拒绝不静默慢跑）、版本化
//!           （声明随软渲版本演进）、判据。
//! 组件：下限判定器 + 档位表 + 拒绝三要素。
//! 职责定位：下限声明是「最低配置要求」——老机器跑不动就明
//!           说跑不动（拒绝三要素），不装能跑让用户干等；
//!           有 AVX2 的机器给高一点的预期，如实分档。
//! 性能逐项分解：判定 O(1) 一次性；分档 O(1)。
//! 诊断码段：0x32200（0x32201~0x32209 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const HF_VERSION: &str = "veb356_hwfloor-v1";

/// 诊断段基址（0x32200 段 · 本模块独占）。
pub const HF_SEG_BASE: u32 = 0x32200;
pub const XC_BELOW: u32 = 0x32201; // 下限以下
pub const XC_BADISA: u32 = 0x32202; // 指令集代号非法
pub const XC_BADVER: u32 = 0x32203; // 版本号非法

/// 指令集代号（升序能力）。
pub const I_NONE: u32 = 0; // 无 SIMD
pub const I_SSE2: u32 = 1;
pub const I_SSE42: u32 = 2; // 下限
pub const I_AVX: u32 = 3;
pub const I_AVX2: u32 = 4; // 推荐
pub const I_AVX512: u32 = 5;
pub const I_RVV: u32 = 6; // RVV 路径（按 F0326 结论）

/// 下限。
pub const FLOOR: u32 = I_SSE42;

/// 性能档位（百分比预期）。
pub fn tier_of(isa: u32) -> Result<u32, u32> {
    match isa {
        I_SSE42 => Ok(40),
        I_AVX => Ok(60),
        I_AVX2 => Ok(85),
        I_AVX512 => Ok(100),
        I_RVV => Ok(75), // RVV 实测档
        _ => Err(XC_BELOW),
    }
}

/// 拒绝三要素（缺指令集时的显性拒绝文案结构）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rej3 {
    /// 现状（探测到的指令集）。
    pub found: u32,
    /// 下限要求。
    pub need: u32,
    /// 出路代号（0=升级硬件 1=切换环境）。
    pub exit_hint: u32,
}

/// 下限判定账。
pub struct Floor {
    /// 声明版本号。
    pub ver: u32,
    /// 判定已完成。
    pub judged: bool,
    /// 判定结果。
    pub isa: u32,
}

impl Floor {
    /// 建：版本号非零。
    pub fn new(ver: u32) -> Result<Floor, u32> {
        if ver == 0 {
            return Err(XC_BADVER);
        }
        Ok(Floor {
            ver,
            judged: false,
            isa: I_NONE,
        })
    }

    /// 初始化期一次性判定：探测值核界；下限以下 → 拒绝三要素。
    pub fn judge(&mut self, probed: u32) -> Result<u32, u32> {
        if probed > I_RVV {
            return Err(XC_BADISA);
        }
        self.judged = true;
        self.isa = probed;
        if probed < FLOOR {
            return Err(XC_BELOW);
        }
        Ok(tier_of(probed)?)
    }

    /// 拒绝三要素构造。
    pub fn reject3(probed: u32) -> Rej3 {
        Rej3 {
            found: probed,
            need: FLOOR,
            exit_hint: 0,
        }
    }

    /// 探测失败保守档：按最低达标档处理（SSE4.2）。
    pub fn fallback(&mut self) -> Result<u32, u32> {
        self.judge(FLOOR)
    }

    /// 版本演进：升版声明。
    pub fn bump(&mut self, ver: u32) -> Result<(), u32> {
        if ver == 0 {
            return Err(XC_BADVER);
        }
        self.ver = ver;
        Ok(())
    }
}

pub fn run_veb356_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0356");
    // 族零：版本门。
    s.add("零版拒", matches!(Floor::new(0), Err(XC_BADVER)), "无版本");
    let mut f = match Floor::new(1) {
        Ok(x) => x,
        Err(_) => {
            s.add("建败", false, "不应到");
            return s;
        }
    };
    s.add("未判", !f.judged, "初始化前");
    // 族一：档位表。
    s.add("SSE4 40", tier_of(I_SSE42) == Ok(40), "下限档");
    s.add("AVX 60", tier_of(I_AVX) == Ok(60), "中档");
    s.add("AVX2 85", tier_of(I_AVX2) == Ok(85), "推荐档");
    s.add("AVX512 100", tier_of(I_AVX512) == Ok(100), "顶配");
    s.add("RVV 75", tier_of(I_RVV) == Ok(75), "RVV 实测档");
    s.add("SSE2 拒", tier_of(I_SSE2) == Err(XC_BELOW), "下限以下");
    s.add("无 SIMD 拒", tier_of(I_NONE) == Err(XC_BELOW), "裸机");
    // 族二：一次性判定。
    s.add("SSE2 拒判", f.judge(I_SSE2) == Err(XC_BELOW), "显性拒绝");
    s.add("拒绝已记账", f.judged, "判定过");
    s.add("三要素", Floor::reject3(I_SSE2) == Rej3 { found: 1, need: 2, exit_hint: 0 }, "现状+要求+出路");
    // 达标判定。
    let mut f2 = match Floor::new(1) {
        Ok(x) => x,
        Err(_) => {
            s.add("建败 2", false, "不应到");
            return s;
        }
    };
    s.add("AVX2 过判", f2.judge(I_AVX2) == Ok(85), "推荐档");
    s.add("isa 在案", f2.isa == I_AVX2, "记录");
    s.add("判过", f2.judged, "完成");
    // 族三：探测失败保守档。
    let mut f3 = match Floor::new(2) {
        Ok(x) => x,
        Err(_) => {
            s.add("建败 3", false, "不应到");
            return s;
        }
    };
    s.add("保守档过", f3.fallback() == Ok(40), "按 SSE4.2");
    s.add("保守 isa", f3.isa == I_SSE42, "最低达标");
    // 族四：越界探测。
    s.add("越界拒判", f3.judge(7) == Err(XC_BADISA), ">RVV");
    // 越界后仍可正常判（防御不锁死）。
    s.add("正常判复过", f3.judge(I_AVX) == Ok(60), "恢复");
    // 族五：版本化。
    s.add("升版过", f3.bump(2).is_ok(), "v2");
    s.add("零升拒", f3.bump(0) == Err(XC_BADVER), "无版");
    s.add("版本二", f3.ver == 2, "演进");
    s.add("魔数核", HF_SEG_BASE == 0x32200, "段基址");
    s
}
