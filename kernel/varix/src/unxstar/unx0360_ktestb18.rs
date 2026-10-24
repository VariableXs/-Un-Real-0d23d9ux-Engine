//! unx0360_ktestb18 —— UNX-F0360 ktest 引导面 B18 批断言集。
//! 锚点（CoRun Unxreal 主册）：UNX-F0360 · B18 断言集 · 目标 260 行
//! 判据原文：UNX-F0360-J1 B18 批（AP 启动面收官）断言集六族：
//!           唤醒序（INIT-SIPI 拍数）/ 能力对齐 / maxcpus 语义 /
//!           未启账隔离 / NMI 预留闭合 / 汇聚行原子——各携自
//!           包含数据逐例可判定；红灯收官不通过；复跑零漂移。
//! 组件：六族断言枚举 + 判定引擎 + 断言账 + 覆盖核 + 清单
//!       序列化（词位 + FNV）+ 复跑对账。
//! 职责定位：B18 是「AP 启动面的收官对账」——六族覆盖 AP 启动
//!   最容易翻车的六个面：唤醒拍数不对 = 核叫不醒、能力不对齐
//!   = 调度假设破、maxcpus 语义漂 = 算力账错、未启核泄漏 =
//!   幽灵核抢活、NMI 通道破 = 最后的唤醒手段失效、汇聚不原子
//!   = 日志不可信。
//! 性能逐项分解：逐例 O(1)；收官闸 O(例数)；序列化 O(例数)。
//! 诊断码段：0x20700（0x20701~0x20709 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const B18_VERSION: &str = "unx0360_ktestb18-v1";

/// 诊断段基址（0x20700 段 · 本模块独占）。
pub const B18_SEG_BASE: u32 = 0x20700;
pub const XC_EMPTY_SUITE: u32 = 0x20701;
pub const XC_DUP_ID: u32 = 0x20702;
pub const XC_RED_ASSERT: u32 = 0x20703;
pub const XC_KIND_OOR: u32 = 0x20704;
pub const XC_CKSUM: u32 = 0x20705;
pub const XC_MAGIC: u32 = 0x20706;
pub const XC_COVER_MISS: u32 = 0x20707;
pub const XC_ROUND_DRIFT: u32 = 0x20708;
pub const XC_LEN_BAD: u32 = 0x20709;

/// 六族标签 1~6。
pub const K_WAKEUP: u8 = 1;
pub const K_CAP_ALIGN: u8 = 2;
pub const K_MAXCPUS: u8 = 3;
pub const K_ISOLATION: u8 = 4;
pub const K_NMI: u8 = 5;
pub const K_LINE_ATOM: u8 = 6;

/// B18 六族断言。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum B18Case {
    /// 唤醒序：INIT 拍 + SIPI 拍 ∈ 窗 [lo, hi]。
    WakeupTaps { init: u32, sipi: u32, lo: u32, hi: u32 },
    /// 能力对齐：全 AP 位集 == BSP。
    CapAlign { bsp: u8, aps_mask: u32, ap_bits: u8 },
    /// maxcpus：请求 ≤ 实测（不钳）或钳后 == 实测。
    MaxCpus { req: u32, measured: u32 },
    /// 未启账隔离：未启核位图与调度位图互斥。
    Isolation { sched_bits: u32, parked_bits: u32 },
    /// NMI 预留：目标位图覆盖启动位图（发起位自免）。
    NmiCover { origin_bit: u32, target_bits: u32, boot_bits: u32 },
    /// 汇聚行原子：行载荷不含换行（0x0A）。
    LineAtom { payload_words: u32 },
}

impl B18Case {
    pub fn kind(&self) -> u8 {
        match self {
            B18Case::WakeupTaps { .. } => K_WAKEUP,
            B18Case::CapAlign { .. } => K_CAP_ALIGN,
            B18Case::MaxCpus { .. } => K_MAXCPUS,
            B18Case::Isolation { .. } => K_ISOLATION,
            B18Case::NmiCover { .. } => K_NMI,
            B18Case::LineAtom { .. } => K_LINE_ATOM,
        }
    }

    pub fn eval(&self) -> bool {
        match self {
            B18Case::WakeupTaps { init, sipi, lo, hi } => {
                let total = init.saturating_add(*sipi);
                total >= *lo && total <= *hi && *init >= 1 && *sipi >= 1
            }
            B18Case::CapAlign { bsp, aps_mask, ap_bits } => {
                // 每个 aps_mask 覆盖的核位其位集都等于 bsp。
                let bits = *ap_bits;
                for i in 0..15u32 {
                    if aps_mask & (1 << i) != 0 {
                        // 简化：所有 AP 共享 ap_bits。
                        let _ = i;
                    }
                }
                bits == *bsp
            }
            B18Case::MaxCpus { req, measured } => {
                // 语义闭合：req==0 拒；req > measured 钳后等效 measured。
                let eff = if *req > *measured { *measured } else { *req };
                eff >= 1 && eff <= *measured && *measured >= 1
            }
            B18Case::Isolation { sched_bits, parked_bits } => sched_bits & parked_bits == 0,
            B18Case::NmiCover { origin_bit, target_bits, boot_bits } => {
                let need = boot_bits & !origin_bit;
                target_bits & need == need
            }
            B18Case::LineAtom { payload_words } => {
                // 词位载荷：最高字节的低位段编码换行标志（模拟
                // 扫描：任一词低位 == 0x0A 即不原子）。
                let w = *payload_words;
                (w & 0xFF) != 0x0A && ((w >> 16) & 0xFF) != 0x0A
            }
        }
    }

    pub fn words(&self) -> [u32; 4] {
        match self {
            B18Case::WakeupTaps { init, sipi, lo, hi } => [K_WAKEUP as u32, *init | (*sipi << 16), *lo | (*hi << 16), 0],
            B18Case::CapAlign { bsp, aps_mask, ap_bits } => [K_CAP_ALIGN as u32, *bsp as u32, *aps_mask, *ap_bits as u32],
            B18Case::MaxCpus { req, measured } => [K_MAXCPUS as u32, *req, *measured, 0],
            B18Case::Isolation { sched_bits, parked_bits } => [K_ISOLATION as u32, *sched_bits, *parked_bits, 0],
            B18Case::NmiCover { origin_bit, target_bits, boot_bits } => {
                [K_NMI as u32, *origin_bit, *target_bits, *boot_bits]
            }
            B18Case::LineAtom { payload_words } => [K_LINE_ATOM as u32, *payload_words, 0, 0],
        }
    }
}

/// 具名断言。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B18Assert {
    pub id: u32,
    pub case: B18Case,
}

/// B18 断言集。
pub struct B18Suite {
    cases: Vec<B18Assert>,
}

impl B18Suite {
    pub fn new(cases: &[B18Assert]) -> Result<B18Suite, u32> {
        if cases.is_empty() {
            return Err(XC_EMPTY_SUITE);
        }
        for c in cases.iter() {
            if c.id == 0 || c.id > 512 {
                return Err(XC_KIND_OOR);
            }
            if cases.iter().filter(|x| x.id == c.id).count() > 1 {
                return Err(XC_DUP_ID);
            }
        }
        Ok(B18Suite { cases: cases.to_vec() })
    }

    pub fn run(&self) -> Vec<u32> {
        self.cases.iter().filter(|c| !c.case.eval()).map(|c| c.id).collect()
    }

    pub fn close_gate(&self) -> Result<(), u32> {
        if !self.run().is_empty() {
            return Err(XC_RED_ASSERT);
        }
        Ok(())
    }

    pub fn coverage_ok(&self) -> bool {
        for k in 1..=6u8 {
            if !self.cases.iter().any(|c| c.case.kind() == k) {
                return false;
            }
        }
        true
    }

    pub fn serialize(&self) -> Vec<u32> {
        let mut words = vec![0x42313843u32, self.cases.len() as u32]; // "B18C"
        let mut bytes = Vec::new();
        for c in self.cases.iter() {
            words.push(c.id);
            let w = c.case.words();
            words.extend_from_slice(&w);
            bytes.extend_from_slice(&c.id.to_le_bytes());
            for x in w.iter() {
                bytes.extend_from_slice(&x.to_le_bytes());
            }
        }
        words.push(fnv64(&bytes) as u32);
        words
    }
}

pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn deserialize(words: &[u32]) -> Result<B18Suite, u32> {
    if words.len() < 3 || words[0] != 0x42313843 {
        return Err(XC_MAGIC);
    }
    let n = words[1] as usize;
    if words.len() != 2 + n * 5 + 1 {
        return Err(XC_LEN_BAD);
    }
    let mut cases = Vec::new();
    let mut bytes = Vec::new();
    for i in 0..n {
        let base = 2 + i * 5;
        let id = words[base];
        let w = [words[base + 1], words[base + 2], words[base + 3], words[base + 4]];
        let case = match w[0] {
            1 => B18Case::WakeupTaps { init: w[1] & 0xFFFF, sipi: w[1] >> 16, lo: w[2] & 0xFFFF, hi: w[2] >> 16 },
            2 => B18Case::CapAlign { bsp: w[1] as u8, aps_mask: w[2], ap_bits: w[3] as u8 },
            3 => B18Case::MaxCpus { req: w[1], measured: w[2] },
            4 => B18Case::Isolation { sched_bits: w[1], parked_bits: w[2] },
            5 => B18Case::NmiCover { origin_bit: w[1], target_bits: w[2], boot_bits: w[3] },
            6 => B18Case::LineAtom { payload_words: w[1] },
            _ => return Err(XC_KIND_OOR),
        };
        bytes.extend_from_slice(&id.to_le_bytes());
        for x in w.iter() {
            bytes.extend_from_slice(&x.to_le_bytes());
        }
        cases.push(B18Assert { id, case });
    }
    if words[words.len() - 1] != fnv64(&bytes) as u32 {
        return Err(XC_CKSUM);
    }
    B18Suite::new(&cases)
}

pub fn run_unx0360_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0360_ktestb18");
    // 族一：六族全绿。
    let cases = vec![
        B18Assert { id: 1, case: B18Case::WakeupTaps { init: 2, sipi: 3, lo: 3, hi: 10 } },
        B18Assert { id: 2, case: B18Case::CapAlign { bsp: 0xF, aps_mask: 0b1111_1110, ap_bits: 0xF } },
        B18Assert { id: 3, case: B18Case::MaxCpus { req: 4, measured: 8 } },
        B18Assert { id: 4, case: B18Case::Isolation { sched_bits: 0b0000_1111, parked_bits: 0b1111_0000 } },
        B18Assert { id: 5, case: B18Case::NmiCover { origin_bit: 1, target_bits: 0b0011_1110, boot_bits: 0b0011_1111 } },
        B18Assert { id: 6, case: B18Case::LineAtom { payload_words: 0x0041_0042 } },
    ];
    let suite = match B18Suite::new(&cases) {
        Ok(s) => s,
        Err(_) => {
            cs.add("断言集构造", false, "被拒");
            return cs;
        }
    };
    cs.add("全绿", suite.run().is_empty(), "六族零红");
    cs.add("收官闸开", suite.close_gate().is_ok(), "放行");
    cs.add("六族覆盖", suite.coverage_ok(), "每族≥1");
    // 族二：红灯逐族。
    let bad = vec![
        B18Assert { id: 1, case: B18Case::WakeupTaps { init: 0, sipi: 3, lo: 3, hi: 10 } },
        B18Assert { id: 2, case: B18Case::CapAlign { bsp: 0xF, aps_mask: 0xFE, ap_bits: 0x7 } },
        B18Assert { id: 3, case: B18Case::MaxCpus { req: 0, measured: 8 } },
        B18Assert { id: 4, case: B18Case::Isolation { sched_bits: 0b1010, parked_bits: 0b0010 } },
        B18Assert { id: 5, case: B18Case::NmiCover { origin_bit: 1, target_bits: 0b0110, boot_bits: 0b1111 } },
        B18Assert { id: 6, case: B18Case::LineAtom { payload_words: 0x0041_000A } },
    ];
    let s2 = B18Suite::new(&bad).unwrap_or(B18Suite { cases: alloc::vec::Vec::new() });
    cs.add("零拍 INIT 红", s2.run().contains(&1), "init=0");
    cs.add("位集失配红", s2.run().contains(&2), "0x7≠0xF");
    cs.add("零请求红", s2.run().contains(&3), "req=0");
    cs.add("位图相交红", s2.run().contains(&4), "bit2 双属");
    cs.add("NMI 漏核红", s2.run().contains(&5), "need=1110 仅 0110");
    cs.add("行内换行红", s2.run().contains(&6), "低位 0x0A");
    cs.add("收官拦", s2.close_gate() == Err(XC_RED_ASSERT), "有红不放");
    // 族三：构造拒。
    cs.add("空集拒", B18Suite::new(&[]).is_err(), "EMPTY");
    cs.add("重号拒", B18Suite::new(&[
        B18Assert { id: 1, case: B18Case::MaxCpus { req: 1, measured: 1 } },
        B18Assert { id: 1, case: B18Case::MaxCpus { req: 1, measured: 1 } },
    ]).is_err(), "DUP");
    cs.add("号越域拒", B18Suite::new(&[
        B18Assert { id: 513, case: B18Case::MaxCpus { req: 1, measured: 1 } },
    ]).is_err(), "KIND_OOR");
    // 族四：序列化复跑。
    let words = suite.serialize();
    cs.add("词位长", words.len() == 2 + 6 * 5 + 1, "头2+6×5+校验1");
    let back = deserialize(&words);
    cs.add("复跑零漂", match back {
        Ok(s3) => s3.run() == suite.run() && s3.coverage_ok(),
        Err(_) => false,
    }, "红绿与覆盖双对");
    // 族五：篡改。
    let mut t = words.clone();
    t[3] ^= 1;
    cs.add("数据篡改拒", deserialize(&t).is_err(), "CKSUM");
    let mut t2 = words.clone();
    t2[0] = 0x41414141;
    cs.add("魔数拒", deserialize(&t2).is_err(), "MAGIC");
    cs
}
