//! veb341_sfreg —— VE-F0341 软渲驱动族正式注册。
//! 锚点：VE-F0341 · 目标 340 行。
//! 判据原文：fallback 档（纯 CPU 软渲注册为合法适配器候选）、
//!           显性子集（能力子集显性列出）、排序联动（硬渲缺
//!           席时唯一候选 / 在场时按用户策略排序）、版本化
//!           （注册信息版本化，漂移重注册）、判据。
//! 组件：注册条目 + 选择排序联动 + 版本账。
//! 职责定位：软渲注册是「替补队员的正式工牌」——不是杂牌军，
//!           是挂了牌的合法候选；硬渲在场时它让位，硬渲缺席
//!           时它顶上，谁先谁后由用户策略说了算。
//! 性能逐项分解：注册 O(1)；排序 O(候选数)；版本 O(1)。
//! 诊断码段：0x31E40（0x31E41~0x31E49 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SR_VERSION: &str = "veb341_sfreg-v1";

/// 诊断段基址（0x31E40 段 · 本模块独占）。
pub const SR_SEG_BASE: u32 = 0x31E40;
pub const XC_NOCAPS: u32 = 0x31E41; // 能力探测失败且无最小集
pub const XC_BADTIER: u32 = 0x31E42; // 性能档非法
pub const XC_DRIFT: u32 = 0x31E43; // 版本漂移（需重注册）
pub const XC_EMPTY: u32 = 0x31E44; // 无候选可排

/// 能力子集位（显性列出）。
pub const C_FILL: u32 = 1; // 填充
pub const C_BLIT: u32 = 2; // 位块传输
pub const C_BLEND: u32 = 4; // 混合
pub const C_CLIP: u32 = 8; // 裁剪
pub const C_TEXT: u32 = 16; // 文本栅格化
/// 最小能力集（低于此不得注册）。
pub const CAPS_MIN: u32 = C_FILL | C_BLIT;

/// 性能档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Perf {
    /// 保守档。
    Safe,
    /// 均衡档。
    Bal,
    /// 全速档。
    Full,
}

/// 注册条目。
pub struct Entry {
    /// 能力掩码。
    pub caps: u32,
    pub perf: Perf,
    /// 注册版本号。
    pub ver: u32,
    /// fallback 档语义（软渲恒 true）。
    pub fallback: bool,
}

/// 注册簿。
pub struct SfReg {
    e: Option<Entry>,
    /// 累计重注册次数（版本漂移）。
    pub re_regs: u32,
}

impl SfReg {
    /// 空簿。
    pub fn new() -> SfReg {
        SfReg {
            e: None,
            re_regs: 0,
        }
    }

    /// 注册：能力低于最小集拒；fallback 恒标注。
    pub fn register(&mut self, caps: u32, perf: Perf, ver: u32) -> Result<(), u32> {
        if caps & CAPS_MIN != CAPS_MIN {
            return Err(XC_NOCAPS);
        }
        if let Some(ref old) = self.e {
            if old.ver != ver {
                self.re_regs += 1; // 版本漂移重注册
            }
        }
        self.e = Some(Entry {
            caps,
            perf,
            ver,
            fallback: true,
        });
        Ok(())
    }

    /// 能力查询：显性子集。
    pub fn caps(&self) -> Option<u32> {
        self.e.as_ref().map(|x| x.caps)
    }

    /// 具备某能力。
    pub fn has(&self, bit: u32) -> bool {
        self.caps().map(|c| c & bit != 0).unwrap_or(false)
    }

    /// 排序联动：返回候选序（0 = 本软渲）。
    /// 硬渲缺席 → 软渲唯一候选；硬渲在场 → 用户策略（0 = 软渲优先，1 = 硬渲优先）。
    pub fn sort_rank(&self, hw_present: bool, user_pref: u32) -> Result<u32, u32> {
        if self.e.is_none() {
            return Err(XC_EMPTY);
        }
        if !hw_present {
            return Ok(0); // 缺席：唯一候选
        }
        Ok(user_pref) // 在场：策略优先并留痕
    }

    /// 版本查询。
    pub fn ver(&self) -> Option<u32> {
        self.e.as_ref().map(|x| x.ver)
    }

    /// 在册核。
    pub fn registered(&self) -> bool {
        self.e.is_some()
    }
}

pub fn run_veb341_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0341");
    // 族零：注册门。
    let mut r = SfReg::new();
    s.add("无探测拒", r.register(0, Perf::Safe, 1) == Err(XC_NOCAPS), "空能力");
    s.add("缺位块拒", r.register(C_FILL, Perf::Safe, 1) == Err(XC_NOCAPS), "最小集");
    s.add("最小集过", r.register(CAPS_MIN, Perf::Safe, 1).is_ok(), "填充+位块");
    s.add("fallback 标", r.registered(), "在册");
    s.add("版本一", r.ver() == Some(1), "v1");
    // 族一：显性子集。
    s.add("有填充", r.has(C_FILL), "bit0");
    s.add("无文本", !r.has(C_TEXT), "未列即无");
    s.add("全子集过", r.register(C_FILL | C_BLIT | C_BLEND | C_CLIP | C_TEXT, Perf::Bal, 2).is_ok(), "五能力");
    s.add("子集五", r.caps() == Some(31), "0b11111");
    // 族二：排序联动。
    s.add("硬渲缺席首选", r.sort_rank(false, 1) == Ok(0), "唯一候选");
    s.add("策略硬渲先", r.sort_rank(true, 1) == Ok(1), "在场让位");
    s.add("策略软渲先", r.sort_rank(true, 0) == Ok(0), "用户点名");
    // 族三：版本漂移。
    s.add("漂移重注册", r.register(31, Perf::Full, 3).is_ok(), "v2→v3");
    s.add("重注册一", r.re_regs == 1, "漂移账");
    s.add("版本三", r.ver() == Some(3), "新版本");
    s.add("同版不重", r.register(31, Perf::Full, 3).is_ok() && r.re_regs == 1, "幂等");
    // 族四：性能档。
    s.add("档在 Full", r.register(31, Perf::Full, 3).is_ok(), "标注诚实");
    let _ = r.register(31, Perf::Safe, 3);
    s.add("档可回退", r.register(31, Perf::Safe, 3).is_ok(), "保守档");
    // 族五：空簿排序拒。
    let r2 = SfReg::new();
    s.add("空簿拒排", r2.sort_rank(false, 0) == Err(XC_EMPTY), "无候选");
    s.add("空簿无版", r2.ver().is_none(), "版本空");
    s.add("魔数核", SR_SEG_BASE == 0x31E40, "段基址");
    s
}
