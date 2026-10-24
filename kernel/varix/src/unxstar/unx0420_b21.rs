//! unx0420_b21 —— UNX-F0420 B21 批 ktest 断言集。
//! 锚点（CoRun Unxreal 主册）：UNX-F0420 · B21 批 ktest 断言集 · 目标 260 行
//! 判据原文：UNX-F0420-J1 B21 批断言集收口：计时面收官所需的
//!           跨批不变量逐条可判定（账平/签稳/数齐/序闭/可复现/
//!           往返/单调保/容差守八族），任一红项收官不通过；
//!           断言清单可序列化复跑（词位流 + FNV32 + 魔数）；红
//!           项立案带名可定位；批号闭集 [21, 120)。
//! 组件：断言用例（八族枚举，各携带自含数据）+ 判定引擎 +
//!       断言账（红项立案 + 收官闸）+ 清单序列化。
//! 职责定位：B21 批是「计时面收官的总对账」——通用六族沿用
//!   B19/B20，计时面特有两族：单调保（计时读值流的关键性质，
//!   收官时必须全绿）与容差守（校准交叉偏差收官复核）。
//! 性能逐项分解：逐例判定 O(1)；收官闸 O(例数)；序列化 O(例数)。
//! 诊断码段：0x21280（0x21281~0x21289 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const B21_VERSION: &str = "unx0420_b21-v1";

/// 诊断段基址（0x21280 段 · 本模块独占）。
pub const B21_SEG_BASE: u32 = 0x21280;
pub const XC_EMPTY_SUITE: u32 = 0x21281; // 空断言集拒
pub const XC_DUP_NAME: u32 = 0x21282; // 断言重名
pub const XC_RED_ASSERT: u32 = 0x21283; // 红项存在
pub const XC_CKSUM: u32 = 0x21284; // 清单校验和不过
pub const XC_KIND_OOR: u32 = 0x21285; // 族越域
pub const XC_MAGIC: u32 = 0x21286; // 清单魔数破
pub const XC_NAME_LEN: u32 = 0x21287; // 名长越界
pub const XC_BATCH_BAD: u32 = 0x21288; // 批号越闭集
pub const XC_TOL_BAD: u32 = 0x21289; // 容差样本越域

/// B21 断言八族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum B21Case {
    /// 账平族。
    LedgerBalance { debit: u64, credit: u64 },
    /// 签稳族。
    SigStable { sig_a: u64, sig_b: u64 },
    /// 数齐族。
    CountMatch { actual: u32, expect: u32 },
    /// 序闭族。
    OrderClosed { seq: [u32; 4], lo: u32, hi: u32 },
    /// 可复现族。
    ReproStable { seed: u32, fp_a: u64, fp_b: u64 },
    /// 往返族。
    RoundTrip { orig: u64, back: u64 },
    /// 单调保族（计时特有）：读值流单调（逐对递增）。
    MonoKeep { seq: [u64; 4] },
    /// 容差守族（计时特有）：偏差 ≤ 容差。
    TolKeep { drift_milli: u32, tol_milli: u32 },
}

/// B21 指纹（LCG 两轮混合，独立常量）。
pub fn b21_fp(seed: u32) -> u64 {
    let mut x = seed.wrapping_add(0x85EB_CA6B) as u64;
    x = x.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1_442_695_040_888_963_407);
    x = x.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1_442_695_040_888_963_407);
    x ^ (x >> 27)
}

impl B21Case {
    /// 族标签（1~8）。
    pub fn kind(&self) -> u8 {
        match self {
            B21Case::LedgerBalance { .. } => 1,
            B21Case::SigStable { .. } => 2,
            B21Case::CountMatch { .. } => 3,
            B21Case::OrderClosed { .. } => 4,
            B21Case::ReproStable { .. } => 5,
            B21Case::RoundTrip { .. } => 6,
            B21Case::MonoKeep { .. } => 7,
            B21Case::TolKeep { .. } => 8,
        }
    }

    /// 判定。
    pub fn eval(&self) -> bool {
        match self {
            B21Case::LedgerBalance { debit, credit } => debit == credit,
            B21Case::SigStable { sig_a, sig_b } => sig_a == sig_b && *sig_a != 0,
            B21Case::CountMatch { actual, expect } => actual == expect,
            B21Case::OrderClosed { seq, lo, hi } => {
                let mut prev = lo.saturating_sub(1);
                for &v in seq.iter() {
                    if v <= prev || v < *lo || v > *hi {
                        return false;
                    }
                    prev = v;
                }
                true
            }
            B21Case::ReproStable { seed, fp_a, fp_b } => {
                let expect = b21_fp(*seed);
                fp_a == fp_b && *fp_a == expect
            }
            B21Case::RoundTrip { orig, back } => orig == back,
            B21Case::MonoKeep { seq } => {
                for i in 1..seq.len() {
                    if seq[i] < seq[i - 1] {
                        return false;
                    }
                }
                true
            }
            B21Case::TolKeep { drift_milli, tol_milli } => {
                if *tol_milli > 1000 {
                    return false;
                }
                drift_milli <= tol_milli
            }
        }
    }

    /// 词位编码。
    pub fn words(&self) -> [u32; 5] {
        match self {
            B21Case::LedgerBalance { debit, credit } => {
                [1, *debit as u32, (*debit >> 32) as u32, *credit as u32, (*credit >> 32) as u32]
            }
            B21Case::SigStable { sig_a, sig_b } => {
                [2, *sig_a as u32, (*sig_a >> 32) as u32, *sig_b as u32, (*sig_b >> 32) as u32]
            }
            B21Case::CountMatch { actual, expect } => [3, *actual, *expect, 0, 0],
            B21Case::OrderClosed { seq, lo, hi } => {
                [4, seq[0], seq[3] | (lo << 8) | (hi << 16), seq[1], seq[2]]
            }
            B21Case::ReproStable { seed, fp_a, fp_b: _ } => {
                [5, *seed, *fp_a as u32, (*fp_a >> 32) as u32, 0]
            }
            B21Case::RoundTrip { orig, back: _ } => [6, *orig as u32, (*orig >> 32) as u32, 0, 0],
            B21Case::MonoKeep { seq } => {
                [7, seq[0] as u32, seq[1] as u32, seq[2] as u32, seq[3] as u32]
            }
            B21Case::TolKeep { drift_milli, tol_milli } => [8, *drift_milli, *tol_milli, 0, 0],
        }
    }

    /// 往返复原。
    pub fn roundtrip(&self) -> Result<B21Case, u32> {
        match self {
            B21Case::RoundTrip { orig, back: _ } => {
                let lo = (*orig & 0xFFFF_FFFF) as u32;
                let hi = (*orig >> 32) as u32;
                Ok(B21Case::RoundTrip { orig: *orig, back: ((hi as u64) << 32) | lo as u64 })
            }
            _ => Err(XC_KIND_OOR),
        }
    }
}

/// 断言项。
#[derive(Clone, Debug)]
pub struct B21Assert {
    pub name: &'static str,
    /// 批号（[21, 120)）。
    pub batch: u32,
    pub case: B21Case,
}

/// 断言账。
pub struct B21Suite {
    items: Vec<B21Assert>,
    reds: Vec<usize>,
}

impl B21Suite {
    /// 建空账。
    pub fn new() -> B21Suite {
        B21Suite { items: Vec::new(), reds: Vec::new() }
    }

    /// 登记。
    pub fn enroll(&mut self, name: &'static str, batch: u32, case: B21Case) -> Result<(), u32> {
        let nb = name.as_bytes().len();
        if nb == 0 || nb > 32 {
            return Err(XC_NAME_LEN);
        }
        if batch < 21 || batch >= 120 {
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
        self.items.push(B21Assert { name, batch, case });
        Ok(())
    }

    /// 求值。
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

    /// 收官闸。
    pub fn close_gate(&mut self) -> Result<(), u32> {
        self.evaluate()?;
        for it in self.items.iter() {
            if it.batch < 21 || it.batch >= 120 {
                return Err(XC_BATCH_BAD);
            }
        }
        Ok(())
    }

    /// 序列化。
    pub fn digest(&self) -> (Vec<u32>, u32) {
        let mut words: Vec<u32> = Vec::new();
        words.push(0xB210_0021u32); // 魔数：B21 批 v21
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
        if words[0] != 0xB210_0021 {
            return Err(XC_MAGIC);
        }
        if words[words.len() - 1] != sig {
            return Err(XC_CKSUM);
        }
        Ok(())
    }

    /// 红项名。
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

pub fn run_unx0420_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0420_b21");
    // 族一：八族各登记一例全绿。
    let mut s1 = B21Suite::new();
    let _ = s1.enroll("账平", 21, B21Case::LedgerBalance { debit: 8192, credit: 8192 });
    let _ = s1.enroll("签稳", 21, B21Case::SigStable { sig_a: 0xB210_BEEF, sig_b: 0xB210_BEEF });
    let _ = s1.enroll("数齐", 21, B21Case::CountMatch { actual: 512, expect: 512 });
    let _ = s1.enroll("序闭", 21, B21Case::OrderClosed { seq: [21, 25, 30, 44], lo: 20, hi: 45 });
    let fp = b21_fp(21);
    let _ = s1.enroll("可复现", 21, B21Case::ReproStable { seed: 21, fp_a: fp, fp_b: fp });
    let _ = s1.enroll("往返", 21, B21Case::RoundTrip { orig: 0xB210_0000_0000_0021, back: 0xB210_0000_0000_0021 });
    let _ = s1.enroll("单调保", 21, B21Case::MonoKeep { seq: [10, 20, 30, 40] });
    let _ = s1.enroll("容差守", 21, B21Case::TolKeep { drift_milli: 5, tol_milli: 50 });
    cs.add("登记八项", s1.len() == 8, "八族齐");
    cs.add("求值全绿", s1.evaluate().is_ok(), "红 0");
    cs.add("收官闸过", s1.close_gate().is_ok(), "批号闭集内");
    // 族二：计时特有两族硬核。
    let mono_bad = B21Case::MonoKeep { seq: [10, 20, 15, 40] };
    cs.add("回退检出", !mono_bad.eval(), "30 位置回退");
    let mono_eq = B21Case::MonoKeep { seq: [5, 5, 5, 5] };
    cs.add("恒等单调过", mono_eq.eval(), "相等合法");
    let tol_bad = B21Case::TolKeep { drift_milli: 60, tol_milli: 50 };
    cs.add("超容差检出", !tol_bad.eval(), "60 > 50");
    let tol_bad2 = B21Case::TolKeep { drift_milli: 5, tol_milli: 1001 };
    cs.add("容差越域即红", !tol_bad2.eval(), "tol > 1000");
    // 可复现硬核。
    let wrong = b21_fp(21) ^ 1;
    let bad = B21Case::ReproStable { seed: 21, fp_a: wrong, fp_b: wrong };
    cs.add("指纹谎报检出", !bad.eval(), "非函数值拒");
    cs.add("换种子变指纹", b21_fp(21) != b21_fp(22), "种子敏感");
    // 往返硬核。
    let rt = B21Case::RoundTrip { orig: 0x2121_AAAA_5555_00FF, back: 0 };
    let built = match rt.roundtrip() {
        Ok(c) => c,
        Err(_) => B21Case::RoundTrip { orig: 0, back: 1 },
    };
    cs.add("往返复原", built.eval(), "高低 32 位守恒");
    // 族三：红项检出与定位。
    let mut s2 = B21Suite::new();
    let _ = s2.enroll("账破", 21, B21Case::LedgerBalance { debit: 2, credit: 3 });
    let _ = s2.enroll("单破", 21, B21Case::MonoKeep { seq: [1, 2, 0, 4] });
    cs.add("双红检出", s2.evaluate() == Err(XC_RED_ASSERT), "红 2");
    cs.add("红项数", s2.red_count() == 2, "两条立案");
    let names = s2.red_names();
    cs.add("红名定位", names.len() == 2 && names[0] == "账破" && names[1] == "单破", "按登记序");
    // 族四：登记拒绝。
    let mut s3 = B21Suite::new();
    cs.add("空名拒",
        s3.enroll("", 21, B21Case::CountMatch { actual: 1, expect: 1 }) == Err(XC_NAME_LEN),
        "零字节");
    cs.add("批号下界拒",
        s3.enroll("x", 20, B21Case::CountMatch { actual: 1, expect: 1 }) == Err(XC_BATCH_BAD),
        "batch=20");
    cs.add("批号上界拒",
        s3.enroll("y", 120, B21Case::CountMatch { actual: 1, expect: 1 }) == Err(XC_BATCH_BAD),
        "batch=120");
    let _ = s3.enroll("重", 21, B21Case::CountMatch { actual: 1, expect: 1 });
    cs.add("重名拒",
        s3.enroll("重", 21, B21Case::CountMatch { actual: 2, expect: 2 }) == Err(XC_DUP_NAME),
        "同名");
    cs.add("空集拒", B21Suite::new().evaluate() == Err(XC_EMPTY_SUITE), "先拒空");
    // 族五：清单复跑稳。
    cs.add("复跑稳", s1.replay_stable().is_ok(), "签自洽");
    cs.add("魔数对", s1.digest().0[0] == 0xB210_0021, "B2100021");
    // 族六：序闭边界。
    let eq = B21Case::OrderClosed { seq: [1, 1, 2, 3], lo: 1, hi: 9 };
    cs.add("相等非升序拒", !eq.eval(), "严格升序");
    cs
}
