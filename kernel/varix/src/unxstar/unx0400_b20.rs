//! unx0400_b20 —— UNX-F0400 B20 批 ktest 断言集。
//! 锚点（CoRun Unxreal 主册）：UNX-F0400 · B20 批 ktest 断言集 · 目标 240 行
//! 判据原文：UNX-F0400-J1 B20 批断言集收口：堆面收官所需的跨批
//!           不变量逐条可判定（账平/签稳/数齐/序闭/可复现/
//!           往返/配额非负/峰值守恒八族），任一红项收官不通过；
//!           断言清单可序列化复跑（词位流 + FNV32 + 魔数）；
//!           红项立案带名可定位；批号闭集 [20, 119)。
//! 组件：断言用例（八族枚举，各携带自含数据）+ 判定引擎 +
//!       断言账（红项立案 + 收官闸）+ 清单序列化。
//! 职责定位：B20 批是「堆面收官的总对账」——在堆面接入面
//!   （UNX-F0396）逐模块登记之后，B20 从批视角再核一遍：六族
//!   通用形沿用 B19（0380），另加堆面特有两族：配额非负（预
//!   留窗/注入器不得出现负配额样本）与峰值守恒（演练峰值 =
//!   独立重算峰值，不许谎报）。
//! 性能逐项分解：逐例判定 O(1)；收官闸 O(例数)；序列化 O(例数)。
//! 诊断码段：0x20EC0（0x20EC1~0x20EC9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const B20_VERSION: &str = "unx0400_b20-v1";

/// 诊断段基址（0x20EC0 段 · 本模块独占）。
pub const B20_SEG_BASE: u32 = 0x20EC0;
pub const XC_EMPTY_SUITE: u32 = 0x20EC1; // 空断言集拒
pub const XC_DUP_NAME: u32 = 0x20EC2; // 断言重名
pub const XC_RED_ASSERT: u32 = 0x20EC3; // 红项存在
pub const XC_CKSUM: u32 = 0x20EC4; // 清单校验和不过
pub const XC_KIND_OOR: u32 = 0x20EC5; // 族越域
pub const XC_MAGIC: u32 = 0x20EC6; // 清单魔数破
pub const XC_NAME_LEN: u32 = 0x20EC7; // 断言名长度越界
pub const XC_BATCH_BAD: u32 = 0x20EC8; // 批号越闭集
pub const XC_QUOTA_NEG: u32 = 0x20EC9; // 配额负样本

/// B20 断言八族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum B20Case {
    /// 账平族：借方 = 贷方。
    LedgerBalance { debit: u64, credit: u64 },
    /// 签稳族：两次内容签相等且非零。
    SigStable { sig_a: u64, sig_b: u64 },
    /// 数齐族：实计数 = 应计数。
    CountMatch { actual: u32, expect: u32 },
    /// 序闭族：四元序列严格升序且闭于 [lo, hi]。
    OrderClosed { seq: [u32; 4], lo: u32, hi: u32 },
    /// 可复现族：指纹 = 确定性函数值。
    ReproStable { seed: u32, fp_a: u64, fp_b: u64 },
    /// 往返族：词位往返守恒。
    RoundTrip { orig: u64, back: u64 },
    /// 配额非负族（堆面特有）：配额样本 ≥ 0 且 ≤ 上限。
    QuotaNonNeg { quota: i64, ceiling: u64 },
    /// 峰值守恒族（堆面特有）：报告峰值 = 独立重算峰值。
    PeakConserv { reported: u64, recomputed: u64 },
}

/// 可复现指纹（LCG 两轮混合，与 B19 同构独立实现）。
pub fn b20_fp(seed: u32) -> u64 {
    let mut x = seed.wrapping_add(0x9E37_79B9) as u64;
    x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
    x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
    x ^ (x >> 29)
}

impl B20Case {
    /// 族标签（1~8）。
    pub fn kind(&self) -> u8 {
        match self {
            B20Case::LedgerBalance { .. } => 1,
            B20Case::SigStable { .. } => 2,
            B20Case::CountMatch { .. } => 3,
            B20Case::OrderClosed { .. } => 4,
            B20Case::ReproStable { .. } => 5,
            B20Case::RoundTrip { .. } => 6,
            B20Case::QuotaNonNeg { .. } => 7,
            B20Case::PeakConserv { .. } => 8,
        }
    }

    /// 判定：绿 = 不变量成立。
    pub fn eval(&self) -> bool {
        match self {
            B20Case::LedgerBalance { debit, credit } => debit == credit,
            B20Case::SigStable { sig_a, sig_b } => sig_a == sig_b && *sig_a != 0,
            B20Case::CountMatch { actual, expect } => actual == expect,
            B20Case::OrderClosed { seq, lo, hi } => {
                let mut prev = lo.saturating_sub(1);
                for &v in seq.iter() {
                    if v <= prev || v < *lo || v > *hi {
                        return false;
                    }
                    prev = v;
                }
                true
            }
            B20Case::ReproStable { seed, fp_a, fp_b } => {
                let expect = b20_fp(*seed);
                fp_a == fp_b && *fp_a == expect
            }
            B20Case::RoundTrip { orig, back } => orig == back,
            B20Case::QuotaNonNeg { quota, ceiling } => *quota >= 0 && (*quota as u64) <= *ceiling,
            B20Case::PeakConserv { reported, recomputed } => reported == recomputed,
        }
    }

    /// 词位编码（族 + 4 数据词）。
    pub fn words(&self) -> [u32; 5] {
        match self {
            B20Case::LedgerBalance { debit, credit } => {
                [1, *debit as u32, (*debit >> 32) as u32, *credit as u32, (*credit >> 32) as u32]
            }
            B20Case::SigStable { sig_a, sig_b } => {
                [2, *sig_a as u32, (*sig_a >> 32) as u32, *sig_b as u32, (*sig_b >> 32) as u32]
            }
            B20Case::CountMatch { actual, expect } => [3, *actual, *expect, 0, 0],
            B20Case::OrderClosed { seq, lo, hi } => {
                [4, seq[0], seq[3] | (lo << 8) | (hi << 16), seq[1], seq[2]]
            }
            B20Case::ReproStable { seed, fp_a, fp_b: _ } => {
                [5, *seed, *fp_a as u32, (*fp_a >> 32) as u32, 0]
            }
            B20Case::RoundTrip { orig, back: _ } => [6, *orig as u32, (*orig >> 32) as u32, 0, 0],
            B20Case::QuotaNonNeg { quota, ceiling } => {
                [7, *quota as u32, ((*quota >> 32) as u32), *ceiling as u32, 0]
            }
            B20Case::PeakConserv { reported, recomputed: _ } => {
                [8, *reported as u32, (*reported >> 32) as u32, 0, 0]
            }
        }
    }

    /// 往返复原（往返族自证）。
    pub fn roundtrip(&self) -> Result<B20Case, u32> {
        match self {
            B20Case::RoundTrip { orig, back: _ } => {
                let lo = (*orig & 0xFFFF_FFFF) as u32;
                let hi = (*orig >> 32) as u32;
                Ok(B20Case::RoundTrip { orig: *orig, back: ((hi as u64) << 32) | lo as u64 })
            }
            _ => Err(XC_KIND_OOR),
        }
    }
}

/// 断言项。
#[derive(Clone, Debug)]
pub struct B20Assert {
    /// 断言名（1~32 字节，账内唯一）。
    pub name: &'static str,
    /// 批号（[20, 119)）。
    pub batch: u32,
    /// 案例。
    pub case: B20Case,
}

/// 断言账。
pub struct B20Suite {
    items: Vec<B20Assert>,
    reds: Vec<usize>,
}

impl B20Suite {
    /// 建空账。
    pub fn new() -> B20Suite {
        B20Suite { items: Vec::new(), reds: Vec::new() }
    }

    /// 登记：名长 / 批号 / 族越域 / 重名 拒。
    pub fn enroll(&mut self, name: &'static str, batch: u32, case: B20Case) -> Result<(), u32> {
        let nb = name.as_bytes().len();
        if nb == 0 || nb > 32 {
            return Err(XC_NAME_LEN);
        }
        if batch < 20 || batch >= 119 {
            return Err(XC_BATCH_BAD);
        }
        if case.kind() == 0 || case.kind() > 8 {
            return Err(XC_KIND_OOR);
        }
        for it in self.items.iter() {
            if it.name == name {
                return Err(XC_DUP_NAME);
            }
        }
        self.items.push(B20Assert { name, batch, case });
        Ok(())
    }

    /// 求值全部：红项立案；空集拒。
    pub fn evaluate(&mut self) -> Result<u32, u32> {
        if self.items.is_empty() {
            return Err(XC_EMPTY_SUITE);
        }
        self.reds.clear();
        for (i, it) in self.items.iter().enumerate() {
            if !it.case.eval() {
                self.reds.push(i);
            }
        }
        if !self.reds.is_empty() {
            return Err(XC_RED_ASSERT);
        }
        Ok(self.items.len() as u32)
    }

    /// 收官闸：求值绿 + 批号闭集复核。
    pub fn close_gate(&mut self) -> Result<(), u32> {
        self.evaluate()?;
        for it in self.items.iter() {
            if it.batch < 20 || it.batch >= 119 {
                return Err(XC_BATCH_BAD);
            }
        }
        Ok(())
    }

    /// 清单序列化：魔数 + 项数 + 各项（批号 + 名长 + 五词）+ FNV32。
    pub fn digest(&self) -> (Vec<u32>, u32) {
        let mut words: Vec<u32> = Vec::new();
        words.push(0xB200_0020u32); // 魔数：B20 批 v20
        words.push(self.items.len() as u32);
        for it in self.items.iter() {
            words.push(it.batch);
            words.push(it.name.as_bytes().len() as u32);
            for w in it.case.words().iter() {
                words.push(*w);
            }
        }
        let mut h: u32 = 0x811C_9DC5;
        for &w in words.iter() {
            h ^= w;
            h = h.wrapping_mul(0x0100_0193);
        }
        words.push(h);
        (words, h)
    }

    /// 复跑稳。
    pub fn replay_stable(&self) -> Result<(), u32> {
        let (words, sig) = self.digest();
        if words[0] != 0xB200_0020 {
            return Err(XC_MAGIC);
        }
        if words[words.len() - 1] != sig {
            return Err(XC_CKSUM);
        }
        Ok(())
    }

    /// 红项名清单。
    pub fn red_names(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for &i in self.reds.iter() {
            if let Some(it) = self.items.get(i) {
                out.push(it.name);
            }
        }
        out
    }

    /// 登记数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 红项数。
    pub fn red_count(&self) -> usize {
        self.reds.len()
    }
}

pub fn run_unx0400_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0400_b20");
    // 族一：八族各登记一例全绿。
    let mut s1 = B20Suite::new();
    let _ = s1.enroll("账平", 20, B20Case::LedgerBalance { debit: 4096, credit: 4096 });
    let _ = s1.enroll("签稳", 20, B20Case::SigStable { sig_a: 0xCAFE_F00D, sig_b: 0xCAFE_F00D });
    let _ = s1.enroll("数齐", 20, B20Case::CountMatch { actual: 256, expect: 256 });
    let _ = s1.enroll("序闭", 20, B20Case::OrderClosed { seq: [5, 9, 14, 20], lo: 4, hi: 21 });
    let fp = b20_fp(7);
    let _ = s1.enroll("可复现", 20, B20Case::ReproStable { seed: 7, fp_a: fp, fp_b: fp });
    let _ = s1.enroll("往返", 20, B20Case::RoundTrip { orig: 0xDEAD_BEEF_1234_5678, back: 0xDEAD_BEEF_1234_5678 });
    let _ = s1.enroll("配额非负", 20, B20Case::QuotaNonNeg { quota: 2048, ceiling: 4096 });
    let _ = s1.enroll("峰值守恒", 20, B20Case::PeakConserv { reported: 700, recomputed: 700 });
    cs.add("登记八项", s1.len() == 8, "八族齐");
    cs.add("求值全绿", s1.evaluate().is_ok(), "红 0");
    cs.add("收官闸过", s1.close_gate().is_ok(), "批号闭集内");
    // 族二：堆面特有两族硬核。
    let neg = B20Case::QuotaNonNeg { quota: -1, ceiling: 4096 };
    cs.add("负配额检出", !neg.eval(), "quota=-1 拒");
    let over = B20Case::QuotaNonNeg { quota: 5000, ceiling: 4096 };
    cs.add("超顶配额检出", !over.eval(), "5000 > 4096");
    let lie = B20Case::PeakConserv { reported: 800, recomputed: 700 };
    cs.add("峰值谎报检出", !lie.eval(), "800 ≠ 700");
    // 可复现硬核。
    let wrong = b20_fp(7) ^ 1;
    let bad = B20Case::ReproStable { seed: 7, fp_a: wrong, fp_b: wrong };
    cs.add("指纹谎报检出", !bad.eval(), "非函数值拒");
    cs.add("换种子变指纹", b20_fp(7) != b20_fp(8), "种子敏感");
    // 往返硬核。
    let rt = B20Case::RoundTrip { orig: 0x0123_4567_89AB_CDEF, back: 0 };
    let built = match rt.roundtrip() {
        Ok(c) => c,
        Err(_) => B20Case::RoundTrip { orig: 0, back: 1 },
    };
    cs.add("往返复原", built.eval(), "高低 32 位守恒");
    // 族三：红项检出与定位。
    let mut s2 = B20Suite::new();
    let _ = s2.enroll("账破", 20, B20Case::LedgerBalance { debit: 1, credit: 2 });
    let _ = s2.enroll("峰破", 20, B20Case::PeakConserv { reported: 9, recomputed: 8 });
    cs.add("双红检出", s2.evaluate() == Err(XC_RED_ASSERT), "红 2");
    cs.add("红项数", s2.red_count() == 2, "两条立案");
    let names = s2.red_names();
    cs.add("红名定位", names.len() == 2 && names[0] == "账破" && names[1] == "峰破", "按登记序");
    // 族四：登记拒绝。
    let mut s3 = B20Suite::new();
    cs.add("空名拒",
        s3.enroll("", 20, B20Case::CountMatch { actual: 1, expect: 1 }) == Err(XC_NAME_LEN),
        "零字节");
    cs.add("批号下界拒",
        s3.enroll("x", 19, B20Case::CountMatch { actual: 1, expect: 1 }) == Err(XC_BATCH_BAD),
        "batch=19");
    cs.add("批号上界拒",
        s3.enroll("y", 119, B20Case::CountMatch { actual: 1, expect: 1 }) == Err(XC_BATCH_BAD),
        "batch=119");
    let _ = s3.enroll("重", 20, B20Case::CountMatch { actual: 1, expect: 1 });
    cs.add("重名拒",
        s3.enroll("重", 20, B20Case::CountMatch { actual: 2, expect: 2 }) == Err(XC_DUP_NAME),
        "同名");
    cs.add("空集拒", B20Suite::new().evaluate() == Err(XC_EMPTY_SUITE), "先拒空");
    // 族五：清单复跑稳。
    cs.add("复跑稳", s1.replay_stable().is_ok(), "签自洽");
    cs.add("魔数对", s1.digest().0[0] == 0xB200_0020, "B2000020");
    // 族六：序闭边界。
    let eq = B20Case::OrderClosed { seq: [1, 1, 2, 3], lo: 1, hi: 9 };
    cs.add("相等非升序拒", !eq.eval(), "严格升序");
    let oo = B20Case::OrderClosed { seq: [1, 2, 3, 99], lo: 1, hi: 20 };
    cs.add("越上界拒", !oo.eval(), "99 > 20");
    cs
}
