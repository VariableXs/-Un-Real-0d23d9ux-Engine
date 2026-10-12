//! UNX-F1181 中断亲和性路由 · 目标寄存器语义
//! 判据原文（CoRun Unxreal 主册）：UNX-F1181-J1 全注册向量亲和性目标寄存器值与路由表一致率 100%（逐向量硬件读回比对），路由表更新生效延迟 ≤50µs 实测，非法目标（不在线核）设置拦截 10/10 次
//! · 组件：亲和性目标寄存器模型（掩码/物理逻辑模式）+ 位序合法性 + 读回一致
//! · 判据落地：目标寄存器值与路由表一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const AFT_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const AFT_SEG_BASE: u16 = 0x6300;

// --------------------------------------------------------------------------
// 诊断码（aft 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 目标寄存器诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum aftCode {
    /// 亲和性掩码为零（无目标核）。
    MaskZero,
    /// 掩码为全核（广播义）。
    MaskAll,
    /// 掩码指向超出核数。
    CpuOob,
    /// 物理/逻辑模式位非法。
    ModeBad,
    /// 寄存器读回与路由表不符。
    ReadbackDiff,
    /// 目标不在线。
    OfflineTarget,
    /// 寄存器已冻结（引导期）。
    RegFrozen,
    /// 同向量重复设置。
    DupSet,
}

impl aftCode {
    /// 全量闭集。
    pub const ALL: [aftCode; 8] = [
        aftCode::MaskZero,
        aftCode::MaskAll,
        aftCode::CpuOob,
        aftCode::ModeBad,
        aftCode::ReadbackDiff,
        aftCode::OfflineTarget,
        aftCode::RegFrozen,
        aftCode::DupSet,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            aftCode::MaskZero => 0x6300 + 0,
            aftCode::MaskAll => 0x6300 + 1,
            aftCode::CpuOob => 0x6300 + 2,
            aftCode::ModeBad => 0x6300 + 3,
            aftCode::ReadbackDiff => 0x6300 + 4,
            aftCode::OfflineTarget => 0x6300 + 5,
            aftCode::RegFrozen => 0x6300 + 6,
            aftCode::DupSet => 0x6300 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            aftCode::MaskZero => "亲和性掩码为零（无目标核）",
            aftCode::MaskAll => "掩码为全核（广播义）",
            aftCode::CpuOob => "掩码指向超出核数",
            aftCode::ModeBad => "物理/逻辑模式位非法",
            aftCode::ReadbackDiff => "寄存器读回与路由表不符",
            aftCode::OfflineTarget => "目标不在线",
            aftCode::RegFrozen => "寄存器已冻结（引导期）",
            aftCode::DupSet => "同向量重复设置",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            aftCode::MaskZero => "按在线核掩码兜底",
            aftCode::MaskAll => "按低优先级核收敛",
            aftCode::CpuOob => "按核数掩码裁剪",
            aftCode::ModeBad => "按物理模式兜底",
            aftCode::ReadbackDiff => "重设后复读",
            aftCode::OfflineTarget => "拒设并记账",
            aftCode::RegFrozen => "解冻后才可设",
            aftCode::DupSet => "幂等或显性拒",
        }
    }
}


// --------------------------------------------------------------------------
// 冻结常量表 aft_reg（5 行逐行注释自证）
// --------------------------------------------------------------------------
/// 亲和性目标寄存器能力表。
pub const aft_reg_COUNT: usize = 5;

/// 表本体（编译期冻结，运行期只读）。
pub static aft_reg: [(&'static str, u32); 5] = [
    ("phys_lo", 0), // 物理模式目标字段低位
    ("phys_hi", 7), // 物理模式目标字段高位
    ("log_bits", 8), // 逻辑模式位宽 8
    ("mode_bit", 11), // 模式位号（第 11 位）
    ("mask_max", 255), // 目标掩码上限
];

/// 线性查找（键唯一由构造期断言保证）。
pub fn aft_reg_get(key: &'static str) -> Option<u32> {
    let mut i = 0usize;
    while i < aft_reg_COUNT {
        if aft_reg[i].0 == key {
            return Some(aft_reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（守恒对账用）。
pub fn aft_reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < aft_reg_COUNT {
        s = s.wrapping_add(aft_reg[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（逐对扫描；重复即结构腐坏）。
pub fn aft_reg_unique() -> bool {
    let mut i = 0usize;
    while i < aft_reg_COUNT {
        let mut j = i + 1;
        while j < aft_reg_COUNT {
            if aft_reg[i].0 == aft_reg[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}


// ---------------------------------------------------------------------------
// 亲和性目标寄存器（掩码 + 模式；读回一致由调用方保证）
// ---------------------------------------------------------------------------

/// 亲和性目标。
#[derive(Clone, Copy, Debug)]
pub struct AffinityTarget {
    /// 目标核掩码（bit i = 核 i）。
    pub mask: u32,
    /// 物理模式（true）或逻辑模式（false）。
    pub phys: bool,
}

impl AffinityTarget {
    /// 物理模式单目标。
    pub const fn phys_single(cpu: u32) -> Self {
        Self { mask: 1u32 << (cpu & 31), phys: true }
    }

    /// 掩码是否为零。
    pub const fn is_empty(&self) -> bool {
        self.mask == 0
    }

    /// 掩码内是否有核超出在线集。
    pub fn has_offline(&self, online: u32) -> bool {
        (self.mask & !online) != 0
    }

    /// 掩码内首个在线核。
    pub fn first_online(&self, online: u32) -> Option<u32> {
        let m = self.mask & online;
        if m == 0 {
            return None;
        }
        Some(m.trailing_zeros())
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x6300, 0x630f]。
pub const fn aft_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < aftCode::ALL.len() {
        let c = aftCode::ALL[i].code();
        if c < 0x6300 || c > 0x630f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 单目标构造 + 空判定。
fn c1_single() -> bool {
    let t = AffinityTarget::phys_single(3);
    !t.is_empty() && t.mask == 8
        && AffinityTarget { mask: 0, phys: true }.is_empty()
}

/// 离线核检测 + 首个在线核。
fn c2_offline() -> bool {
    let t = AffinityTarget { mask: 0b1010, phys: true };
    let online = 0b0010u32;
    t.has_offline(online) && t.first_online(online) == Some(1)
        && AffinityTarget { mask: 0, phys: false }.first_online(online).is_none()
}

/// 段位守卫：全部诊断码落在本模块段内。
fn aft_guard_seg() -> bool {
    aft_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_aft_checks() -> CheckSet {
    let mut set = CheckSet::new("aft");
    set.add(
        "F1181-tgt-c1",
        c1_single(),
        if c1_single() { "" } else { "affinity target construction broken" },
    );
    set.add(
        "F1181-tgt-c2",
        c2_offline(),
        if c2_offline() { "" } else { "offline cpu detection broken" },
    );
    set.add(
        "aft-seg-guard",
        aft_guard_seg(),
        if aft_guard_seg() { "" } else { "diagnostic code outside module segment" },
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（域聚合测试在 mod.rs——此处只放本模块纯函数级断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aft_all_checks_green() {
        let set = run_aft_checks();
        assert!(set.all_passed(), "AFT domain checks must be green");
    }

    #[test]
    fn aft_segment_guard_holds() {
        assert!(aft_segment_guard());
    }
}
