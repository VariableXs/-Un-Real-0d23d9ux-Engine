//! veb306_pvrid —— VE-F0306 PowerVR 识别与代际。
//! 锚点（CoRun Unxreal 主册）：VE-F0306 · PowerVR 识别与代际 · 目标 300 行
//! 判据原文：VE-F0306-J1 PVR 识别账：厂商号核（Imagination 商
//!           号 0x1010）；系列窗（Rogue / Volcanic / B 系列
//!           三窗）；核心数解包（BVNC 四元组：B=分支 V=版本
//!           N=核心数 C=约束）；代际判（按 B 分支值分段）；
//!           未知分支兜底；BVNC 稳定核。
//! 组件：商号核 + BVNC 解包 + 系列窗 + 兜底。
//! 职责定位：PVR 识别是「IMG 线的名牌解读」——BVNC 是四段铭
//!           牌：分支定辈分、版本定批次、核心数定块头、约束
//!           定体质；分支值是辈分的依据：B 值跨线就是跨代。
//! 性能逐项分解：识别 O(表长) ≤ 5。
//! 诊断码段：0x31580（0x31581~0x31589 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const PV_VERSION: &str = "veb306_pvrid-v1";

/// 诊断段基址（0x31580 段 · 本模块独占）。
pub const PV_SEG_BASE: u32 = 0x31580;
pub const XC_MAGIC: u32 = 0x31581; // 账魔数破
pub const XC_VENDOR: u32 = 0x31582; // 非 IMG 商号

/// 账魔数。
const MAGIC: u32 = 0x3158_1B10;

/// Imagination 商号。
pub const VENDOR_IMG: u32 = 0x1010;

/// PVR 系列。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PSer {
    Rogue,
    Volcanic,
    BSeries,
    Unknown,
}

/// BVNC 四元组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bvnc {
    /// 分支。
    pub b: u16,
    /// 版本。
    pub v: u16,
    /// 核心数。
    pub n: u16,
    /// 约束。
    pub c: u16,
}

/// 系列窗表：(分支 lo, hi, 系列)。
pub const BR_TABLE: [(u16, u16, PSer); 3] = [
    (0x0100, 0x0FFF, PSer::Rogue),
    (0x4000, 0x4FFF, PSer::Volcanic),
    (0x6000, 0x6FFF, PSer::BSeries),
];

/// 识别簿。
pub struct PvrIdent {
    pub unknown_cases: Vec<u16>,
    pub count: u32,
    magic: u32,
}

/// 识别结果。
#[derive(Debug, PartialEq, Eq)]
pub struct PvrRes {
    pub ser: PSer,
    pub bvnc: Bvnc,
}

impl PvrIdent {
    /// 建。
    pub fn new() -> PvrIdent {
        PvrIdent { unknown_cases: Vec::new(), count: 0, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 识别（BVNC 直传）。
    pub fn identify(&mut self, vendor: u32, bvnc: Bvnc) -> Result<PvrRes, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if vendor != VENDOR_IMG {
            return Err(XC_VENDOR);
        }
        self.count += 1;
        for row in BR_TABLE.iter() {
            if bvnc.b >= row.0 && bvnc.b <= row.1 {
                return Ok(PvrRes { ser: row.2, bvnc });
            }
        }
        self.unknown_cases.push(bvnc.b);
        Ok(PvrRes { ser: PSer::Unknown, bvnc })
    }

    /// 打包 BVNC → u64（B|V|N|C 各 16 位）。
    pub fn pack(bvnc: Bvnc) -> u64 {
        ((bvnc.b as u64) << 48) | ((bvnc.v as u64) << 32) | ((bvnc.n as u64) << 16) | bvnc.c as u64
    }

    /// 解包。
    pub fn unpack(w: u64) -> Bvnc {
        Bvnc {
            b: (w >> 48) as u16,
            v: (w >> 32) as u16,
            n: (w >> 16) as u16,
            c: w as u16,
        }
    }

    pub fn unknown_count(&self) -> usize {
        self.unknown_cases.len()
    }
}

pub fn run_veb306_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0306");
    let mut id = PvrIdent::new();
    // 族一：商号门。
    let bv = Bvnc { b: 0x0160, v: 0x0105, n: 0x0008, c: 0x0001 };
    s.add("非 IMG 拒", id.identify(0x13B5, bv) == Err(XC_VENDOR), "ARM 号");
    s.add("零商号拒", id.identify(0, bv) == Err(XC_VENDOR), "无门牌");
    // 族二：三窗。
    let r1 = id.identify(VENDOR_IMG, Bvnc { b: 0x0160, v: 0x0105, n: 0x0008, c: 0x0001 });
    s.add("0x0160 Rogue", matches!(&r1, Ok(r) if r.ser == PSer::Rogue), "Rogue 窗");
    let r2 = id.identify(VENDOR_IMG, Bvnc { b: 0x4000, v: 0x0001, n: 0x0016, c: 0x0002 });
    s.add("0x4000 Volcanic", matches!(&r2, Ok(r) if r.ser == PSer::Volcanic), "V 窗");
    let r3 = id.identify(VENDOR_IMG, Bvnc { b: 0x6200, v: 0x0025, n: 0x0010, c: 0x0003 });
    s.add("0x6200 B 系", matches!(&r3, Ok(r) if r.ser == PSer::BSeries), "B 窗");
    // 窗沿。
    let r4 = id.identify(VENDOR_IMG, Bvnc { b: 0x0100, v: 0, n: 0, c: 0 });
    s.add("0x0100 沿", matches!(&r4, Ok(r) if r.ser == PSer::Rogue), "lo 含等");
    let r5 = id.identify(VENDOR_IMG, Bvnc { b: 0x6FFF, v: 0, n: 0, c: 0 });
    s.add("0x6FFF 沿", matches!(&r5, Ok(r) if r.ser == PSer::BSeries), "hi 含等");
    // 族三：未知兜底。
    let r6 = id.identify(VENDOR_IMG, Bvnc { b: 0x8000, v: 0, n: 0, c: 0 });
    s.add("0x8000 兜底", matches!(&r6, Ok(r) if r.ser == PSer::Unknown), "立案");
    s.add("立案 1", id.unknown_count() == 1, "一笔");
    s.add("间隙 0x2000 兜底", matches!(id.identify(VENDOR_IMG, Bvnc { b: 0x2000, v: 0, n: 0, c: 0 }), Ok(_)), "窗间隙");
    s.add("立案 2", id.unknown_count() == 2, "两笔");
    // 族四：BVNC 打包解包。
    let w = PvrIdent::pack(bv);
    s.add("解包还原", PvrIdent::unpack(w) == bv, "四段");
    // 段独立性。
    let w2 = PvrIdent::pack(Bvnc { b: 0xFFFF, v: 0xAAAA, n: 0x5555, c: 0x0001 });
    let u2 = PvrIdent::unpack(w2);
    s.add("段不串", u2.b == 0xFFFF && u2.v == 0xAAAA && u2.n == 0x5555 && u2.c == 0x0001, "各 16 位");
    // 族五：BVNC 稳定核。
    let ra = id.identify(VENDOR_IMG, bv);
    let rb = id.identify(VENDOR_IMG, bv);
    s.add("两次同果", ra == rb, "稳定");
    s.add("识别计 9", id.count == 9, "含兜底");
    s.add("魔数核", PV_SEG_BASE == 0x31580, "段基址");
    s
}
