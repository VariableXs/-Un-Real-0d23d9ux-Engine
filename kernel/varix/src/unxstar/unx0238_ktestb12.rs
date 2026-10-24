//! unx0238_ktestb12 —— UNX-F0238 ktest 收官面（B12 断言集）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0238 · ktest 收官面 · 目标 280 行
//! 判据原文：UNX-F0238-J1 B12 批断言集收口：A1 域收官所需的跨批
//!           不变量逐条可判定（账平/签稳/数齐/序闭四族），任一
//!           红灯收官不通过，断言清单可序列化复跑。
//! 组件：断言用例（四族枚举：账平/签稳/数齐/序闭，各携带数据）+
//!       判定引擎（逐例求值出红绿）+ 断言账（红项立案 + 收官闸）+
//!       清单序列化（词位流 + FNV 校验，复跑零漂移）。
//! 职责定位：收官面是「跨批不变量的总对账」——每条断言携带自己的
//!   数据（自包含），复跑不依赖外部台账（清单即世界）。四族覆盖
//!   域收官最常翻车的四类：账没平、签漂了、数不齐、序没闭。
//! 性能逐项分解：逐例判定 O(1)；收官闸 O(例数)；序列化 O(例数)。
//! 诊断码段：0x20100（0x20101~0x20109 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const KT_VERSION: &str = "unx0238_ktestb12-v1";

/// 诊断段基址（0x20100 段 · 本模块独占）。
pub const KT_SEG_BASE: u32 = 0x20100;
pub const XC_EMPTY_SUITE: u32 = 0x20101; // 空断言集拒
pub const XC_DUP_NAME: u32 = 0x20102; // 断言重名
pub const XC_RED_ASSERT: u32 = 0x20103; // 红灯存在
pub const XC_CKSUM: u32 = 0x20104; // 清单校验和不过
pub const XC_KIND_OOR: u32 = 0x20105; // 族越域
pub const XC_ROUND_DRIFT: u32 = 0x20106; // 复跑结果漂移
pub const XC_NAME_OOR: u32 = 0x20107; // 断言号越域
pub const XC_BAL_NEG: u32 = 0x20108; // 账目负值
pub const XC_MAGIC: u32 = 0x20109; // 清单魔数破

/// 断言四族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum B12Case {
    /// 账平族：借方 = 贷方（页帧/堆/计时三类账的通用形）。
    LedgerBalance { debit: u64, credit: u64 },
    /// 签稳族：两次内容签必须相等（快照冻结不变量）。
    SigStable { sig_a: u64, sig_b: u64 },
    /// 数齐族：实计数 = 应计数（批数/断言数/立案数）。
    CountMatch { actual: u32, expect: u32 },
    /// 序闭族：序列严格升序且首尾闭于 [lo, hi]（批号闭集）。
    OrderClosed { seq: [u32; 4], lo: u32, hi: u32 },
}

impl B12Case {
    /// 族标签（1~4）。
    pub fn kind(&self) -> u8 {
        match self {
            B12Case::LedgerBalance { .. } => 1,
            B12Case::SigStable { .. } => 2,
            B12Case::CountMatch { .. } => 3,
            B12Case::OrderClosed { .. } => 4,
        }
    }

    /// 判定：绿 = 不变量成立。
    pub fn eval(&self) -> bool {
        match self {
            B12Case::LedgerBalance { debit, credit } => debit == credit,
            B12Case::SigStable { sig_a, sig_b } => sig_a == sig_b && *sig_a != 0,
            B12Case::CountMatch { actual, expect } => actual == expect,
            B12Case::OrderClosed { seq, lo, hi } => {
                let mut prev = lo.saturating_sub(1);
                for &v in seq.iter() {
                    if v <= prev || v < *lo || v > *hi {
                        return false;
                    }
                    prev = v;
                }
                true
            }
        }
    }

    /// 词位编码（序列化用，每例 4 词：族 + 3 数据词）。
    pub fn words(&self) -> [u32; 4] {
        match self {
            B12Case::LedgerBalance { debit, credit } => {
                [1, *debit as u32, (*debit >> 32) as u32, *credit as u32]
            }
            B12Case::SigStable { sig_a, sig_b } => [2, *sig_a as u32, (*sig_a >> 32) as u32, *sig_b as u32],
            B12Case::CountMatch { actual, expect } => [3, *actual, *expect, 0],
            B12Case::OrderClosed { seq, lo, hi } => [4, seq[0] | (seq[1] << 16), seq[2] | (seq[3] << 16), lo | (hi << 16)],
        }
    }
}

/// 具名断言行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B12Assert {
    /// 断言号（1~256 闭集）。
    pub id: u32,
    pub case: B12Case,
}

/// B12 断言集。
pub struct B12Suite {
    cases: Vec<B12Assert>,
}

impl B12Suite {
    /// 构造：空集/重号/号越域当场拒。
    pub fn new(cases: &[B12Assert]) -> Result<B12Suite, u32> {
        if cases.is_empty() {
            return Err(XC_EMPTY_SUITE);
        }
        for c in cases.iter() {
            if c.id == 0 || c.id > 256 {
                return Err(XC_NAME_OOR);
            }
            if cases.iter().filter(|x| x.id == c.id).count() > 1 {
                return Err(XC_DUP_NAME);
            }
        }
        Ok(B12Suite { cases: cases.to_vec() })
    }

    /// 逐例判定：返回红灯号清单（空 = 全绿）。
    pub fn run(&self) -> Vec<u32> {
        let mut red = Vec::new();
        for c in self.cases.iter() {
            if !c.case.eval() {
                red.push(c.id);
            }
        }
        red
    }

    /// 收官闸：红灯非空即拦（收官不允许任何已知红）。
    pub fn close_gate(&self) -> Result<(), u32> {
        if !self.run().is_empty() {
            return Err(XC_RED_ASSERT);
        }
        Ok(())
    }

    /// 四族覆盖核：每族至少一例（收官面不许单族偏科）。
    pub fn coverage_ok(&self) -> bool {
        for k in 1..=4u8 {
            if !self.cases.iter().any(|c| c.case.kind() == k) {
                return false;
            }
        }
        true
    }

    /// 清单序列化：头词魔数 + 例数 + 每例 4 词 + 尾 FNV 校验。
    pub fn serialize(&self) -> Vec<u32> {
        let mut words = vec![0x42313243u32, self.cases.len() as u32]; // "B12C"
        let mut bytes = Vec::new();
        for c in self.cases.iter() {
            let w = c.case.words();
            words.extend_from_slice(&w);
            words.push(c.id);
            bytes.extend_from_slice(&c.id.to_le_bytes());
            for x in w.iter() {
                bytes.extend_from_slice(&x.to_le_bytes());
            }
        }
        let sig = fnv64(&bytes) as u32;
        words.push(sig);
        words
    }
}

/// FNV-1a 64 位。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 清单反序列化 + 校验和复算（复跑入口）。
pub fn deserialize(words: &[u32]) -> Result<B12Suite, u32> {
    if words.len() < 3 || words[0] != 0x42313243 {
        return Err(XC_MAGIC);
    }
    let n = words[1] as usize;
    if words.len() != 2 + n * 5 + 1 {
        return Err(XC_CKSUM);
    }
    let mut cases = Vec::new();
    let mut bytes = Vec::new();
    for i in 0..n {
        let base = 2 + i * 5;
        let id = words[base + 4];
        let w = [words[base], words[base + 1], words[base + 2], words[base + 3]];
        let case = match w[0] {
            1 => B12Case::LedgerBalance {
                debit: (w[1] as u64) | ((w[2] as u64) << 32),
                credit: w[3] as u64,
            },
            2 => B12Case::SigStable {
                sig_a: (w[1] as u64) | ((w[2] as u64) << 32),
                sig_b: w[3] as u64,
            },
            3 => B12Case::CountMatch { actual: w[1], expect: w[2] },
            4 => B12Case::OrderClosed {
                seq: [w[1] & 0xFFFF, w[1] >> 16, w[2] & 0xFFFF, w[2] >> 16],
                lo: w[3] & 0xFFFF,
                hi: w[3] >> 16,
            },
            _ => return Err(XC_KIND_OOR),
        };
        bytes.extend_from_slice(&id.to_le_bytes());
        for x in w.iter() {
            bytes.extend_from_slice(&x.to_le_bytes());
        }
        cases.push(B12Assert { id, case });
    }
    if words[words.len() - 1] != fnv64(&bytes) as u32 {
        return Err(XC_CKSUM);
    }
    B12Suite::new(&cases)
}

pub fn run_unx0238_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0238_ktestb12");
    // 族一：四族全绿样例。
    let cases = vec![
        B12Assert { id: 1, case: B12Case::LedgerBalance { debit: 4096, credit: 4096 } },
        B12Assert { id: 2, case: B12Case::SigStable { sig_a: 0xA5A5, sig_b: 0xA5A5 } },
        B12Assert { id: 3, case: B12Case::CountMatch { actual: 40, expect: 40 } },
        B12Assert { id: 4, case: B12Case::OrderClosed { seq: [1, 5, 9, 40], lo: 1, hi: 40 } },
    ];
    let suite = match B12Suite::new(&cases) {
        Ok(s) => s,
        Err(_) => {
            cs.add("断言集构造", false, "被拒");
            return cs;
        }
    };
    cs.add("全绿", suite.run().is_empty(), "四族零红");
    cs.add("收官闸开", suite.close_gate().is_ok(), "零红放行");
    cs.add("四族覆盖", suite.coverage_ok(), "每族≥1");
    // 族二：红灯检出。
    let bad = vec![
        B12Assert { id: 1, case: B12Case::LedgerBalance { debit: 4096, credit: 4095 } },
        B12Assert { id: 2, case: B12Case::SigStable { sig_a: 0, sig_b: 0 } },
        B12Assert { id: 3, case: B12Case::CountMatch { actual: 39, expect: 40 } },
        B12Assert { id: 4, case: B12Case::OrderClosed { seq: [1, 9, 5, 40], lo: 1, hi: 40 } },
    ];
    let s2 = B12Suite::new(&bad).unwrap_or(B12Suite { cases: alloc::vec::Vec::new() });
    cs.add("账不平红", s2.run().contains(&1), "借≠贷");
    cs.add("零签红", s2.run().contains(&2), "签恒零=没算");
    cs.add("数不齐红", s2.run().contains(&3), "39≠40");
    cs.add("序乱红", s2.run().contains(&4), "9>5 乱序");
    cs.add("收官闸拦", s2.close_gate() == Err(XC_RED_ASSERT), "有红不放");
    // 族三：构造拒。
    cs.add("空集拒", B12Suite::new(&[]).is_err(), "EMPTY");
    cs.add("重号拒", B12Suite::new(&[
        B12Assert { id: 1, case: B12Case::CountMatch { actual: 1, expect: 1 } },
        B12Assert { id: 1, case: B12Case::CountMatch { actual: 1, expect: 1 } },
    ]).is_err(), "DUP");
    cs.add("号越域拒", B12Suite::new(&[
        B12Assert { id: 257, case: B12Case::CountMatch { actual: 1, expect: 1 } },
    ]).is_err(), "NAME_OOR");
    // 族四：序列化复跑零漂移。
    let words = suite.serialize();
    cs.add("词位长", words.len() == 2 + 5 + 1, "头2+例5+校验1");
    let back = deserialize(&words);
    cs.add("复跑零漂", match back {
        Ok(s2) => s2.run() == suite.run() && s2.coverage_ok(),
        Err(_) => false,
    }, "红绿清单一致");
    // 族五：篡改检出。
    let mut t1 = words.clone();
    t1[3] ^= 1; // 改第一例数据词
    cs.add("数据篡改拒", deserialize(&t1).is_err(), "CKSUM");
    let mut t2 = words.clone();
    t2[0] = 0x41414141;
    cs.add("魔数拒", deserialize(&t2).is_err(), "MAGIC");
    let mut t3 = words.clone();
    let last = t3.len() - 1;
    t3[last] = t3[last].wrapping_add(1);
    cs.add("校验词拒", deserialize(&t3).is_err(), "CKSUM");
    cs
}
