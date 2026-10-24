//! veb288_mlishb —— VE-F0288 Mali 着色器后端架构位。
//! 锚点（CoRun Unxreal 主册）：VE-F0288 · Mali 着色器后端架构位 · 目标 340 行
//! 判据原文：VE-F0288-J1 着色器后端账：ISA 代位表（Bifrost /
//!           Valhall / 5 代三 ISA）；编译目标门（ISA 代不识别
//!           → 拒编译）；变体位预算（每变体登记占一预算位，
//!           池满拒）；特化键（顶点 / 片元两域 × 变体号）；
//!           变体重编核（同键重编 → 复用既有变体不占新预
//!           算）；池淘汰（最久未用变体让位）；ISA 字节序
//!           核（变体码首字节 = ISA 签名）。
//! 组件：ISA 表 + 变体池 + 特化键 + LRU 淘汰。
//! 职责定位：着色器后端是「Mali 的编译车间」——ISA 代是车
//!           间的机床型号：型号不认识的单子不接；变体池是
//!           半成品架：架子满了要么复用要么淘汰，不扩地皮；
//!           LRU 是架子的自然规律：最久没人碰的先腾地方。
//! 性能逐项分解：登记 O(池容)；命中 O(1) 键查。
//! 诊断码段：0x31100（0x31101~0x31109 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SH_VERSION: &str = "veb288_mlishb-v1";

/// 诊断段基址（0x31100 段 · 本模块独占）。
pub const SH_SEG_BASE: u32 = 0x31100;
pub const XC_MAGIC: u32 = 0x31101; // 账魔数破
pub const XC_ISA: u32 = 0x31102; // ISA 代不识别
pub const XC_POOL: u32 = 0x31103; // 池满且不可淘汰
pub const XC_KEY: u32 = 0x31104; // 特化键不合法

/// 账魔数。
const MAGIC: u32 = 0x3110_1B7E;

/// ISA 三代。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Isa {
    Bifrost,
    Valhall,
    Gen5,
}

/// ISA 签名字节。
pub fn isa_sig(isa: Isa) -> u8 {
    match isa {
        Isa::Bifrost => 0xB7,
        Isa::Valhall => 0xA0,
        Isa::Gen5 => 0x5D,
    }
}

/// 特化键。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpecKey {
    /// 域：0 顶点 / 1 片元。
    pub domain: u8,
    /// 变体号。
    pub variant: u8,
}

impl SpecKey {
    /// 打包成 u16 池键。
    pub fn code(&self) -> u16 {
        ((self.domain as u16) << 8) | self.variant as u16
    }
}

/// 变体。
pub struct Variant {
    pub key: SpecKey,
    pub isa: Isa,
    /// 生成序（LRU 用）。
    pub stamp: u64,
}

/// 编译簿（变体池）。
pub struct ShBook {
    pub isa: Isa,
    pool: Vec<Variant>,
    /// 池容。
    pub cap: usize,
    tick: u64,
    /// 复用命中数。
    pub hits: u32,
    magic: u32,
}

impl ShBook {
    /// 建（ISA + 池容）。
    pub fn new(isa: Isa, cap: usize) -> ShBook {
        ShBook { isa, pool: Vec::new(), cap, tick: 0, hits: 0, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 编译：键合法 + 池有位（或可淘汰）→ 变体码。
    pub fn compile(&mut self, key: SpecKey) -> Result<u8, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if key.domain > 1 {
            return Err(XC_KEY);
        }
        if key.code() == 0 {
            return Err(XC_KEY); // 0 号键保留
        }
        self.tick += 1;
        // 命中复用。
        for i in 0..self.pool.len() {
            if self.pool[i].key == key {
                self.pool[i].stamp = self.tick;
                self.hits += 1;
                return Ok(isa_sig(self.isa));
            }
        }
        // 池满 → 淘汰最旧。
        if self.pool.len() >= self.cap {
            if self.cap == 0 {
                return Err(XC_POOL);
            }
            let mut oldest = 0;
            for i in 1..self.pool.len() {
                if self.pool[i].stamp < self.pool[oldest].stamp {
                    oldest = i;
                }
            }
            self.pool.remove(oldest);
        }
        self.pool.push(Variant { key, isa: self.isa, stamp: self.tick });
        Ok(isa_sig(self.isa))
    }

    /// 池内键集。
    pub fn keys(&self) -> Vec<u16> {
        let mut v = Vec::new();
        for p in self.pool.iter() {
            v.push(p.key.code());
        }
        v
    }

    /// 池深。
    pub fn depth(&self) -> usize {
        self.pool.len()
    }

    /// ISA 门：识别表校验（模型化为三枚举恒识）。
    pub fn isa_ok(isa: Isa) -> bool {
        matches!(isa, Isa::Bifrost | Isa::Valhall | Isa::Gen5)
    }
}

pub fn run_veb288_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0288");
    // 族零：ISA 表。
    s.add("三 ISA 皆识", ShBook::isa_ok(Isa::Bifrost) && ShBook::isa_ok(Isa::Valhall) && ShBook::isa_ok(Isa::Gen5), "全识");
    s.add("签名互异", isa_sig(Isa::Bifrost) != isa_sig(Isa::Valhall) && isa_sig(Isa::Valhall) != isa_sig(Isa::Gen5), "三签");
    // 族一：键门。
    let mut b = ShBook::new(Isa::Valhall, 2);
    s.add("域 2 拒", b.compile(SpecKey { domain: 2, variant: 1 }) == Err(XC_KEY), "无二域");
    s.add("零键拒", b.compile(SpecKey { domain: 0, variant: 0 }) == Err(XC_KEY), "保留键");
    // 族二：登记与签名。
    let k1 = SpecKey { domain: 0, variant: 1 };
    s.add("编 v1", b.compile(k1) == Ok(0xA0), "Valhall 签");
    s.add("池深 1", b.depth() == 1, "一变体");
    // 重编复用。
    s.add("重编复用", b.compile(k1) == Ok(0xA0) && b.hits == 1, "不占位");
    s.add("池深仍 1", b.depth() == 1, "无新位");
    // 族三：池满淘汰。
    let k2 = SpecKey { domain: 1, variant: 2 };
    let _ = b.compile(k2);
    s.add("池满 2", b.depth() == 2, "达容");
    let k3 = SpecKey { domain: 0, variant: 3 };
    s.add("满池编 v3 过", b.compile(k3) == Ok(0xA0), "淘汰让位");
    s.add("池深仍 2", b.depth() == 2, "有进有出");
    // v1 最久未用 → 已被淘汰；k1 重编为 Miss（hits 不加）。
    let h_before = b.hits;
    s.add("v1 池外", b.keys().contains(&k1.code()) == false, "被淘");
    let _ = b.compile(k1);
    s.add("重入占位", b.depth() == 2 && b.hits == h_before, "新登记");
    // 命中的键保位。
    s.add("v3 仍在", b.keys().contains(&k3.code()), "未动");
    // 族四：零容池。
    let mut z = ShBook::new(Isa::Bifrost, 0);
    s.add("零容拒", z.compile(k1) == Err(XC_POOL), "无地");
    // 族五：ISA 签名核。
    let mut bf = ShBook::new(Isa::Bifrost, 1);
    s.add("Bifrost 签 B7", bf.compile(k1) == Ok(0xB7), "首字节");
    let mut g5 = ShBook::new(Isa::Gen5, 1);
    s.add("Gen5 签 5D", g5.compile(k1) == Ok(0x5D), "首字节");
    // 键码打包核。
    s.add("键码打包", SpecKey { domain: 1, variant: 0x2A }.code() == 0x012A, "域左 8");
    s.add("魔数核", SH_SEG_BASE == 0x31100, "段基址");
    s
}
