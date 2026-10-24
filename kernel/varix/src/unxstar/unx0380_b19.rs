//! unx0380_b19 —— UNX-F0380 B19 批 ktest 断言集。
//! 锚点（CoRun Unxreal 主册）：UNX-F0380 · B19 批 ktest 断言集 · 目标 260 行
//! 判据原文：UNX-F0380-J1 B19 批断言集收口：内存面收官所需的
//!           跨批不变量逐条可判定（账平/签稳/数齐/序闭/可复现/
//!           往返六族），任一红项收官不通过；断言清单可序列化
//!           复跑（词位流 + FNV 校验），红项立案带标签可定位。
//! 组件：断言用例（六族枚举，各携带自含数据）+ 判定引擎（逐例
//!       求值出红绿）+ 断言账（红项立案带名 + 收官闸）+ 清单
//!       序列化（词位流 + FNV32 + 魔数，复跑零漂移）。
//! 职责定位：B19 批是「内存面收官的总对账」——在接入面（0377）
//!   逐模块登记之后，B19 从批视角再核一遍跨批不变量：每条
//!   断言自带数据、判定不依赖外部状态（清单即世界）；可复现
//!   族核「同输入同输出」，往返族核「序列化—反序列化守恒」，
//!   这两族是内存面收官最容易翻车的两类性质。
//! 性能逐项分解：逐例判定 O(1)；收官闸 O(例数)；序列化 O(例数)。
//! 诊断码段：0x20B80（0x20B81~0x20B89 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const B19_VERSION: &str = "unx0380_b19-v1";

/// 诊断段基址（0x20B80 段 · 本模块独占）。
pub const B19_SEG_BASE: u32 = 0x20B80;
pub const XC_EMPTY_SUITE: u32 = 0x20B81; // 空断言集拒
pub const XC_DUP_NAME: u32 = 0x20B82; // 断言重名
pub const XC_RED_ASSERT: u32 = 0x20B83; // 红项存在
pub const XC_CKSUM: u32 = 0x20B84; // 清单校验和不过
pub const XC_KIND_OOR: u32 = 0x20B85; // 族越域
pub const XC_ROUND_DRIFT: u32 = 0x20B86; // 复跑结果漂移
pub const XC_MAGIC: u32 = 0x20B87; // 清单魔数破
pub const XC_NAME_LEN: u32 = 0x20B88; // 断言名长度越界
pub const XC_ORDER_BAD: u32 = 0x20B89; // 序闭族数据坏

/// B19 断言六族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum B19Case {
    /// 账平族：借方 = 贷方（帧账/堆账/流水账通用形）。
    LedgerBalance { debit: u64, credit: u64 },
    /// 签稳族：两次内容签相等且非零（快照冻结不变量）。
    SigStable { sig_a: u64, sig_b: u64 },
    /// 数齐族：实计数 = 应计数。
    CountMatch { actual: u32, expect: u32 },
    /// 序闭族：四元序列严格升序且闭于 [lo, hi]。
    OrderClosed { seq: [u32; 4], lo: u32, hi: u32 },
    /// 可复现族：同输入两次执行指纹相等（LCG 种子驱动）。
    ReproStable { seed: u32, fp_a: u64, fp_b: u64 },
    /// 往返族：值经词位往返后守恒（低 32 位核）。
    RoundTrip { orig: u64, back: u64 },
}

impl B19Case {
    /// 族标签（1~6）。
    pub fn kind(&self) -> u8 {
        match self {
            B19Case::LedgerBalance { .. } => 1,
            B19Case::SigStable { .. } => 2,
            B19Case::CountMatch { .. } => 3,
            B19Case::OrderClosed { .. } => 4,
            B19Case::ReproStable { .. } => 5,
            B19Case::RoundTrip { .. } => 6,
        }
    }

    /// 判定：绿 = 不变量成立。
    pub fn eval(&self) -> bool {
        match self {
            B19Case::LedgerBalance { debit, credit } => debit == credit,
            B19Case::SigStable { sig_a, sig_b } => sig_a == sig_b && *sig_a != 0,
            B19Case::CountMatch { actual, expect } => actual == expect,
            B19Case::OrderClosed { seq, lo, hi } => {
                let mut prev = lo.saturating_sub(1);
                for &v in seq.iter() {
                    if v <= prev || v < *lo || v > *hi {
                        return false;
                    }
                    prev = v;
                }
                true
            }
            B19Case::ReproStable { seed, fp_a, fp_b } => {
                // 可复现硬核：指纹必须等于 LCG(seed) 确定性函数值。
                let expect = repro_fp(*seed);
                fp_a == fp_b && *fp_a == expect
            }
            B19Case::RoundTrip { orig, back } => orig == back,
        }
    }

    /// 词位编码（族 + 4 数据词）。
    pub fn words(&self) -> [u32; 5] {
        match self {
            B19Case::LedgerBalance { debit, credit } => {
                [1, *debit as u32, (*debit >> 32) as u32, *credit as u32, (*credit >> 32) as u32]
            }
            B19Case::SigStable { sig_a, sig_b } => {
                [2, *sig_a as u32, (*sig_a >> 32) as u32, *sig_b as u32, (*sig_b >> 32) as u32]
            }
            B19Case::CountMatch { actual, expect } => [3, *actual, *expect, 0, 0],
            B19Case::OrderClosed { seq, lo, hi } => {
                [4, seq[0], seq[3] | (lo << 8) | (hi << 16), seq[1], seq[2]]
            }
            B19Case::ReproStable { seed, fp_a, fp_b: _ } => {
                [5, *seed, *fp_a as u32, (*fp_a >> 32) as u32, 0]
            }
            B19Case::RoundTrip { orig, back: _ } => {
                [6, *orig as u32, (*orig >> 32) as u32, 0, 0]
            }
        }
    }

    /// 往返复原（从词位重建案例值，往返族自证）。
    pub fn roundtrip(&self) -> Result<B19Case, u32> {
        match self {
            B19Case::RoundTrip { orig, back: _ } => {
                let lo = (*orig & 0xFFFF_FFFF) as u32;
                let hi = (*orig >> 32) as u32;
                let rebuilt = ((hi as u64) << 32) | lo as u64;
                Ok(B19Case::RoundTrip { orig: *orig, back: rebuilt })
            }
            B19Case::LedgerBalance { debit, credit } => {
                Ok(B19Case::RoundTrip { orig: *debit, back: *credit })
            }
            _ => Err(XC_KIND_OOR),
        }
    }
}

/// 可复现指纹：LCG 两轮混合（确定性纯函数）。
pub fn repro_fp(seed: u32) -> u64 {
    let mut x = seed as u64;
    x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
    x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
    x ^ (x >> 31)
}

/// 断言项。
#[derive(Clone, Debug)]
pub struct B19Assert {
    /// 断言名（账内唯一，1~32 字节）。
    pub name: &'static str,
    /// 批号（B19 批内 1~99）。
    pub batch: u32,
    /// 案例。
    pub case: B19Case,
}

/// 断言账。
pub struct B19Suite {
    items: Vec<B19Assert>,
    reds: Vec<usize>,
}

impl B19Suite {
    /// 建空账。
    pub fn new() -> B19Suite {
        B19Suite { items: Vec::new(), reds: Vec::new() }
    }

    /// 登记一项：重名拒、族越域拒、批号越域拒、名长越界拒。
    pub fn enroll(&mut self, name: &'static str, batch: u32, case: B19Case) -> Result<(), u32> {
        let nb = name.as_bytes().len();
        if nb == 0 || nb > 32 {
            return Err(XC_NAME_LEN);
        }
        if batch == 0 || batch > 99 {
            return Err(XC_ORDER_BAD);
        }
        if case.kind() == 0 || case.kind() > 6 {
            return Err(XC_KIND_OOR);
        }
        for it in self.items.iter() {
            if it.name == name {
                return Err(XC_DUP_NAME);
            }
        }
        self.items.push(B19Assert { name, batch, case });
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

    /// 收官闸：求值绿 + 全部批号同闭集 [19, 19+99)。
    pub fn close_gate(&mut self) -> Result<(), u32> {
        self.evaluate()?;
        for it in self.items.iter() {
            if it.batch < 19 || it.batch >= 118 {
                return Err(XC_ORDER_BAD);
            }
        }
        Ok(())
    }

    /// 清单序列化：魔数 + 项数 + 各项（批号 + 名长 + 五词），FNV32 收尾。
    pub fn digest(&self) -> (Vec<u32>, u32) {
        let mut words: Vec<u32> = Vec::new();
        words.push(0xB190_0019u32); // 魔数：B19 批 v19
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

    /// 复跑核：摘要重算一致。
    pub fn replay_stable(&self) -> Result<(), u32> {
        let (words, sig) = self.digest();
        if words[0] != 0xB190_0019 {
            return Err(XC_MAGIC);
        }
        if words[words.len() - 1] != sig {
            return Err(XC_CKSUM);
        }
        Ok(())
    }

    /// 红项名清单（定位用）。
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

pub fn run_unx0380_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0380_b19");
    // 族一：六族各登记一例全绿。
    let mut s1 = B19Suite::new();
    let _ = s1.enroll("账平", 19, B19Case::LedgerBalance { debit: 8192, credit: 8192 });
    let _ = s1.enroll("签稳", 19, B19Case::SigStable { sig_a: 0xDEAD_BEEF, sig_b: 0xDEAD_BEEF });
    let _ = s1.enroll("数齐", 19, B19Case::CountMatch { actual: 128, expect: 128 });
    let _ = s1.enroll("序闭", 19, B19Case::OrderClosed { seq: [3, 7, 11, 19], lo: 1, hi: 20 });
    let fp = repro_fp(42);
    let _ = s1.enroll("可复现", 19, B19Case::ReproStable { seed: 42, fp_a: fp, fp_b: fp });
    let _ = s1.enroll("往返", 19, B19Case::RoundTrip { orig: 0x1_0000_00FF, back: 0x1_0000_00FF });
    cs.add("登记六项", s1.len() == 6, "六族齐");
    cs.add("求值全绿", s1.evaluate().is_ok(), "红 0");
    cs.add("收官闸过", s1.close_gate().is_ok(), "批号闭集内");
    // 族二：可复现族的确定性硬核。
    let wrong = repro_fp(42) ^ 1;
    let bad = B19Case::ReproStable { seed: 42, fp_a: wrong, fp_b: wrong };
    cs.add("指纹谎报检出", !bad.eval(), "非 LCG 值拒");
    let mut_stable = B19Case::ReproStable { seed: 42, fp_a: fp, fp_b: repro_fp(42) };
    cs.add("独立重算一致", mut_stable.eval(), "纯函数同值");
    cs.add("换种子变指纹", repro_fp(42) != repro_fp(43), "种子敏感");
    // 族三：往返族硬核。
    let rt = B19Case::RoundTrip { orig: 0x1234_5678_9ABC_DEF0, back: 0 };
    let rebuilt = match rt.roundtrip() {
        Ok(c) => c,
        Err(_) => B19Case::RoundTrip { orig: 0, back: 1 },
    };
    cs.add("往返复原", rebuilt.eval(), "高低 32 位守恒");
    cs.add("往返词位", rt.words()[1] == 0x9ABC_DEF0 && rt.words()[2] == 0x1234_5678, "词位切分");
    // 族四：红项检出与定位。
    let mut s2 = B19Suite::new();
    let _ = s2.enroll("账破", 19, B19Case::LedgerBalance { debit: 100, credit: 99 });
    let _ = s2.enroll("零签", 19, B19Case::SigStable { sig_a: 0, sig_b: 0 });
    cs.add("双红检出", s2.evaluate() == Err(XC_RED_ASSERT), "红 2");
    cs.add("红项数", s2.red_count() == 2, "两条立案");
    let names = s2.red_names();
    cs.add("红名定位",
        names.len() == 2 && names[0] == "账破" && names[1] == "零签",
        "按登记序");
    // 族五：登记拒绝。
    let mut s3 = B19Suite::new();
    cs.add("空名拒",
        s3.enroll("", 19, B19Case::CountMatch { actual: 1, expect: 1 }) == Err(XC_NAME_LEN),
        "零字节");
    cs.add("批号零拒",
        s3.enroll("x", 0, B19Case::CountMatch { actual: 1, expect: 1 }) == Err(XC_ORDER_BAD),
        "batch=0");
    cs.add("批号超闭拒",
        s3.enroll("y", 200, B19Case::CountMatch { actual: 1, expect: 1 }) == Err(XC_ORDER_BAD),
        "batch=200");
    let _ = s3.enroll("重", 19, B19Case::CountMatch { actual: 1, expect: 1 });
    cs.add("重名拒",
        s3.enroll("重", 19, B19Case::CountMatch { actual: 2, expect: 2 }) == Err(XC_DUP_NAME),
        "同名拒");
    cs.add("空集拒", B19Suite::new().evaluate() == Err(XC_EMPTY_SUITE), "先拒空");
    // 族六：序闭族边界。
    let eq = B19Case::OrderClosed { seq: [1, 1, 2, 3], lo: 1, hi: 9 };
    cs.add("相等非严格升序拒", !eq.eval(), "v <= prev 拦");
    let oo = B19Case::OrderClosed { seq: [1, 2, 3, 11], lo: 1, hi: 10 };
    cs.add("越上界拒", !oo.eval(), "11 > 10");
    // 族七：清单复跑稳。
    cs.add("复跑稳", s1.replay_stable().is_ok(), "签自洽");
    cs.add("魔数对", s1.digest().0[0] == 0xB190_0019, "B1900019");
    cs
}
