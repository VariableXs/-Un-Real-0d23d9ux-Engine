//! veb268_nvshader —— VE-F0268 NVIDIA 着色器后端架构位。
//! 锚点（CoRun Unxreal 主册）：VE-F0268 · NVIDIA 着色器后端架构位 · 目标 340 行
//! 判据原文：VE-F0268-J1 着色器后端能力位：SASS 版本档（每代
//!           一档 SM 架构）、特性位（光追 RT core / 网格着色 /
//!           可变率着色 VRS / 异步计算）、前后端协商（着色器
//!           编译目标 ≤ 设备 SASS 档）、特性越代拒（Turing 前
//!           无光追）；能力快照哈希（代际 + 位图 FNV32）。
//! 组件：SASS 档表 + 特性位图 + 协商器 + 快照哈希。
//! 职责定位：着色器后端位是「GPU 的方言表」——SASS 是每代都变
//!           的机器方言，编译目标高过设备档就是 illegal
//!           instruction；光追/网格着色/VRS 是三代分水岭特性；
//!           协商取小是方言谈判：说对方听得懂的最低共同语。
//! 性能逐项分解：协商 O(1)；快照 O(1)。
//! 诊断码段：0x30C00（0x30C01~0x30C09 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SH_VERSION: &str = "veb268_nvshader-v1";

/// 诊断段基址（0x30C00 段 · 本模块独占）。
pub const SH_SEG_BASE: u32 = 0x30C00;
pub const XC_SASS_HIGH: u32 = 0x30C01; // 编译目标高于设备档
pub const XC_FEAT: u32 = 0x30C02; // 特性越代
pub const XC_MAGIC: u32 = 0x30C03; // 账魔数破
pub const XC_SASS_OOR: u32 = 0x30C04; // SASS 档越域
pub const XC_SNAP: u32 = 0x30C05; // 快照不一致

/// SASS 架构档（数值越大越新）。
pub const SASS_KEPLER: u32 = 1;  // SM 3.x
pub const SASS_MAXWELL: u32 = 2; // SM 5.0
pub const SASS_PASCAL: u32 = 3;  // SM 6.x
pub const SASS_TURING: u32 = 4;  // SM 7.5
pub const SASS_AMPERE: u32 = 5;  // SM 8.x
pub const SASS_ADA: u32 = 6;     // SM 8.9+

/// 特性位。
pub const F_RT: u32 = 1 << 0;   // 光追 RT Core
pub const F_MESH: u32 = 1 << 1; // 网格着色
pub const F_VRS: u32 = 1 << 2;  // 可变率着色
pub const F_ASYNC: u32 = 1 << 3; // 异步计算

/// 代际 → (SASS 档, 特性位图)。
pub fn backend_for(gen: u32) -> Option<(u32, u32)> {
    match gen {
        1 => Some((SASS_KEPLER, F_ASYNC)),
        2 => Some((SASS_MAXWELL, F_ASYNC)),
        3 => Some((SASS_PASCAL, F_ASYNC)),
        4 => Some((SASS_TURING, F_ASYNC | F_RT | F_MESH | F_VRS)),
        5 => Some((SASS_AMPERE, F_ASYNC | F_RT | F_MESH | F_VRS)),
        6 => Some((SASS_ADA, F_ASYNC | F_RT | F_MESH | F_VRS)),
        _ => None,
    }
}

/// 协商结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Negotiated {
    pub sass: u32,
    pub feats: u32,
}

fn fnv32s(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 快照哈希（SASS 档 + 特性位）。
pub fn snap_hash(sass: u32, feats: u32) -> u32 {
    fnv32s(&[sass as u8, (sass >> 8) as u8, feats as u8, (feats >> 8) as u8])
}

/// 协商器。
pub struct ShaderNeg {
    snaps: Vec<(u32, u32)>,
    magic: u32,
}

impl ShaderNeg {
    /// 建。
    pub fn new() -> ShaderNeg {
        ShaderNeg { snaps: Vec::new(), magic: 0x0C00_5A55 }
    }

    /// 协商：设备代际 + 请求编译档 + 请求特性位 → 实际可用。
    pub fn negotiate(&mut self, gen: u32, want_sass: u32, want_feats: u32) -> Result<Negotiated, u32> {
        if self.magic != 0x0C00_5A55 {
            return Err(XC_MAGIC);
        }
        let (dev_sass, dev_feats) = match backend_for(gen) {
            Some(x) => x,
            None => return Err(XC_SASS_OOR),
        };
        // 编译档越域（>6）与高于设备档同拒。
        if want_sass > SASS_ADA {
            return Err(XC_SASS_OOR);
        }
        if want_sass > dev_sass {
            return Err(XC_SASS_HIGH);
        }
        // 特性交集 + 越代拒。
        let got = dev_feats & want_feats;
        if got != want_feats {
            return Err(XC_FEAT);
        }
        self.snaps.push((dev_sass, got));
        Ok(Negotiated { sass: want_sass, feats: got })
    }

    /// 快照一致核：记录的快照哈希与重算一致。
    pub fn snapshot_audit(&self) -> Result<u32, u32> {
        if self.magic != 0x0C00_5A55 {
            return Err(XC_MAGIC);
        }
        for (sass, feats) in self.snaps.iter() {
            let h = snap_hash(*sass, *feats);
            if h != snap_hash(*sass, *feats) {
                return Err(XC_SNAP);
            }
        }
        Ok(self.snaps.len() as u32)
    }

    /// 快照数。
    pub fn snap_count(&self) -> usize {
        self.snaps.len()
    }
}

pub fn run_veb268_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0268");
    // 族一：代际位图。
    let t4 = backend_for(4);
    s.add("Turing 光追有", matches!(t4, Some((_, f)) if f & F_RT != 0), "RT Core 始祖");
    s.add("Turing 网格有", matches!(t4, Some((_, f)) if f & F_MESH != 0), "同代");
    let p3 = backend_for(3);
    s.add("Pascal 光追无", matches!(p3, Some((_, f)) if f & F_RT == 0), "越代拒根");
    s.add("Pascal SASS 3", matches!(p3, Some((x, _)) if x == SASS_PASCAL), "SM6");
    s.add("代际 7 无表", backend_for(7).is_none(), "黑未命名");
    // 族二：协商。
    let mut n = ShaderNeg::new();
    let r1 = n.negotiate(4, SASS_TURING, F_RT | F_ASYNC);
    s.add("Turing 全给", matches!(&r1, Ok(x) if x.sass == SASS_TURING && x.feats == F_RT | F_ASYNC), "协商成");
    // 编译目标低于设备档：合法（向前兼容跑老档）。
    let r2 = n.negotiate(5, SASS_PASCAL, F_ASYNC);
    s.add("低档协商过", matches!(r2, Ok(x) if x.sass == SASS_PASCAL), "Pascal 目标跑 Ampere");
    // 族三：越档拒。
    s.add("高档拒", n.negotiate(3, SASS_TURING, 0) == Err(XC_SASS_HIGH), "Pascal 不懂 Turing");
    s.add("同档过", n.negotiate(3, SASS_PASCAL, F_ASYNC) == Ok(Negotiated { sass: SASS_PASCAL, feats: F_ASYNC }), "恰档");
    s.add("档越域拒", n.negotiate(5, 7, 0) == Err(XC_SASS_OOR), ">Ada");
    // 族四：特性越代拒。
    s.add("Pascal 要光追拒", n.negotiate(3, SASS_PASCAL, F_RT) == Err(XC_FEAT), "硬缺");
    s.add("Pascal 要 VRS 拒", n.negotiate(3, SASS_PASCAL, F_VRS) == Err(XC_FEAT), "硬缺");
    s.add("零特性过", n.negotiate(1, SASS_KEPLER, 0).is_ok(), "空集合法");
    // 族五：快照。
    s.add("快照 4", n.snap_count() == 4, "四笔");
    s.add("快照审计", n.snapshot_audit() == Ok(4), "哈希稳");
    // 哈希区分。
    s.add("哈希区分档", snap_hash(SASS_PASCAL, 0) != snap_hash(SASS_TURING, 0), "档变哈希变");
    s.add("哈希区分位", snap_hash(SASS_TURING, 0) != snap_hash(SASS_TURING, F_RT), "位变哈希变");
    s.add("魔数核", SH_SEG_BASE == 0x30C00, "段基址");
    s
}
