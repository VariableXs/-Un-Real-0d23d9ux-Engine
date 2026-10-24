//! veb262_nvpath —— VE-F0262 驱动路径决策（专有 vs 开源）。
//! 锚点（CoRun Unxreal 主册）：VE-F0262 · 驱动路径决策 · 目标 300 行
//! 判据原文：VE-F0262-J1 路径决策三分：Proprietary（闭源通行）、
//!           Nouveau（开源通行）、None（拒载）；决策依据三查：
//!           固件签名可验、命令流代际支持、法律允许档；任一查
//!           不过降档；决策稳定核（同输入同决策）；降档单调
//!           （只降不升）。
//! 组件：三查表 + 决策器 + 降档链 + 决策账。
//! 职责定位：路径决策是「NVIDIA 通路的岔路口」——专有驱动要
//!           固件签名和 ABI 稳定承诺，开源 Nouveau 要命令流文
//!           档；代际太新两边都缺料就只能拒载；降档单调保证决
//!           策不反复横跳：今天能跑闭源明天不会变成开源。
//! 性能逐项分解：决策 O(查表)；账 O(n)。
//! 诊断码段：0x30A80（0x30A81~0x30A89 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const NP_VERSION: &str = "veb262_nvpath-v1";

/// 诊断段基址（0x30A80 段 · 本模块独占）。
pub const NP_SEG_BASE: u32 = 0x30A80;
pub const XC_NO_PATH: u32 = 0x30A81; // 拒载
pub const XC_MAGIC: u32 = 0x30A82; // 账魔数破
pub const XC_GEN_OOR: u32 = 0x30A83; // 代际越域
pub const XC_NOT_MONO: u32 = 0x30A84; // 降档不单调
pub const XC_EMPTY: u32 = 0x30A85; // 空决策请求
pub const XC_UNSTABLE: u32 = 0x30A86; // 决策不稳

/// 路径档。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Path {
    /// 拒载（最低档）。
    None = 0,
    /// 开源 Nouveau。
    Nouveau = 1,
    /// 闭源专有（最高档）。
    Proprietary = 2,
}

impl Path {
    pub fn down(self) -> Path {
        match self {
            Path::Proprietary => Path::Nouveau,
            _ => Path::None,
        }
    }
}

/// 三查输入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    /// 固件签名可验。
    pub fw_signed: bool,
    /// 命令流代际支持（档内支持 = true）。
    pub cmdstream_ok: bool,
    /// 法律允许档（0=全允许 1=仅开源 2=全禁）。
    pub legal: u32,
}

/// 决策器。
pub struct PathDecider {
    log: Vec<(u32, Path)>,
    magic: u32,
}

impl PathDecider {
    /// 建。
    pub fn new() -> PathDecider {
        PathDecider { log: Vec::new(), magic: 0x0A80_DE0C }
    }

    /// 决策：三查全过 → Proprietary；固件/法律降档；命令流也
    /// 不过 → None。
    pub fn decide(&mut self, devid: u32, gen: u32, p: Probe) -> Result<Path, u32> {
        if self.magic != 0x0A80_DE0C {
            return Err(XC_MAGIC);
        }
        if gen == 0 || gen > 6 {
            return Err(XC_GEN_OOR);
        }
        let mut path = Path::Proprietary;
        if !p.fw_signed {
            path = path.down();
        }
        match p.legal {
            0 => {}
            1 => {
                // 仅开源：专有降开源。
                if path == Path::Proprietary {
                    path = Path::Nouveau;
                }
            }
            _ => path = Path::None,
        }
        if !p.cmdstream_ok {
            // 命令流不支持：再降一档（专有→开源→拒）。
            path = path.down();
        }
        self.log.push((devid, path));
        if path == Path::None {
            return Err(XC_NO_PATH);
        }
        Ok(path)
    }

    /// 决策稳定核：同 devid 重查（同输入）同果。
    pub fn stability(&self, devid: u32, expect: Path) -> Result<bool, u32> {
        let mut seen = false;
        for (d, pth) in self.log.iter() {
            if *d == devid {
                if *pth != expect {
                    return Err(XC_UNSTABLE);
                }
                seen = true;
            }
        }
        Ok(seen)
    }

    /// 决策数。
    pub fn count(&self) -> usize {
        self.log.len()
    }

    /// 档统计。
    pub fn count_path(&self, p: Path) -> u32 {
        let mut n = 0u32;
        for (_, pth) in self.log.iter() {
            if *pth == p {
                n += 1;
            }
        }
        n
    }

    /// 降档链自检。
    pub fn chain_audit() -> Result<(), u32> {
        // Proprietary > Nouveau > None；down 单调。
        let a = Path::Proprietary.down();
        let b = a.down();
        if !(a == Path::Nouveau && b == Path::None) {
            return Err(XC_NOT_MONO);
        }
        if Path::None.down() != Path::None {
            return Err(XC_NOT_MONO);
        }
        Ok(())
    }
}

pub fn run_veb262_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0262");
    // 族零：降档链。
    s.add("链自检", PathDecider::chain_audit().is_ok(), "降档单调");
    s.add("档序", Path::Proprietary > Path::Nouveau && Path::Nouveau > Path::None, "全序");
    // 族一：全绿 → 专有。
    let mut d = PathDecider::new();
    let good = Probe { fw_signed: true, cmdstream_ok: true, legal: 0 };
    s.add("全绿专有", d.decide(0x2204, 5, good) == Ok(Path::Proprietary), "GA102");
    s.add("账 1", d.count() == 1, "一笔");
    // 族二：固件无签 → 降开源。
    let nofw = Probe { fw_signed: false, cmdstream_ok: true, legal: 0 };
    s.add("无签降开源", d.decide(0x1180, 1, nofw) == Ok(Path::Nouveau), "Kepler");
    // 族三：仅开源法律档。
    let legal_os = Probe { fw_signed: true, cmdstream_ok: true, legal: 1 };
    s.add("法律压开源", d.decide(0x1C02, 3, legal_os) == Ok(Path::Nouveau), "Pascal");
    // 双重降：无签 + 仅开源 = 开源（不叠加出负）。
    let both = Probe { fw_signed: false, cmdstream_ok: true, legal: 1 };
    s.add("双重降开源", d.decide(0x1340, 2, both) == Ok(Path::Nouveau), "降无可降");
    // 族四：命令流不支持 → 拒。
    let nocmd = Probe { fw_signed: true, cmdstream_ok: false, legal: 0 };
    s.add("无命令流拒", d.decide(0x2684, 6, nocmd) == Err(XC_NO_PATH), "拒载");
    let nocmd2 = Probe { fw_signed: false, cmdstream_ok: false, legal: 1 };
    s.add("全降拒", d.decide(0x2704, 6, nocmd2) == Err(XC_NO_PATH), "三查全败");
    // 族五：稳定核与统计。
    s.add("稳定核真", d.stability(0x2204, Path::Proprietary) == Ok(true), "重查同果");
    s.add("稳定核伪", d.stability(0x1180, Path::Proprietary) == Err(XC_UNSTABLE), "账面是开源");
    s.add("统计", d.count_path(Path::Proprietary) == 1 && d.count_path(Path::Nouveau) == 3 && d.count_path(Path::None) == 2, "1/3/2");
    // 族六：入参。
    s.add("代际零拒", d.decide(0x1, 0, good) == Err(XC_GEN_OOR), "Unknown 不决策");
    s.add("代际越域拒", d.decide(0x1, 7, good) == Err(XC_GEN_OOR), ">6");
    s.add("魔数核", NP_SEG_BASE == 0x30A80, "段基址");
    s
}
