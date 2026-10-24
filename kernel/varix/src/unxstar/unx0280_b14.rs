//! unx0280_b14 —— UNX-F0280 ktest 引导面 B14 批断言集。
//! 锚点（CoRun Unxreal 主册）：UNX-F0280 · B14 断言集 · 目标 340 行
//! 判据原文：UNX-F0280-J1 B14 批（引导面收官）断言集：介质序/
//!           默认项/超时/重试/回退五族断言各携自包含数据，逐例
//!           可判定；红灯收官不通过；断言清单序列化复跑零漂移；
//!           五族覆盖齐（缺族即断言集不完整）。
//! 组件：五族断言枚举（介质序闭集/默认项存活/超时窗/重试上限/
//!       回退链闭合）+ 判定引擎 + 断言账 + 清单序列化（词位 +
//!       FNV）+ 复跑对账。
//! 职责定位：B14 是「引导面的收官对账」——与 F0238（B12）同构但
//!   族集全部引导化：介质序是引导扫描的骨架次序、默认项存活是
//!   可用性底线、超时窗/重试上限是卡死防线、回退链闭合是「总有
//!   一条路能走通」的存在性证明。
//! 性能逐项分解：逐例判定 O(1)；收官闸 O(例数)；序列化 O(例数)。
//! 诊断码段：0x20540（0x20541~0x20549 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const B14_VERSION: &str = "unx0280_b14-v1";

/// 诊断段基址（0x20540 段 · 本模块独占）。
pub const B14_SEG_BASE: u32 = 0x20540;
pub const XC_EMPTY_SUITE: u32 = 0x20541; // 空断言集
pub const XC_DUP_ID: u32 = 0x20542; // 重号
pub const XC_RED_ASSERT: u32 = 0x20543; // 红灯存在
pub const XC_KIND_OOR: u32 = 0x20544; // 族越域
pub const XC_CKSUM: u32 = 0x20545; // 清单校验和
pub const XC_MAGIC: u32 = 0x20546; // 魔数破
pub const XC_COVER_MISS: u32 = 0x20547; // 族覆盖缺
pub const XC_ROUND_DRIFT: u32 = 0x20548; // 复跑漂移
pub const XC_LEN_BAD: u32 = 0x20549; // 清单长不合

/// 五族标签。
pub const K_MEDIA_ORDER: u8 = 1;
pub const K_DEFAULT_ALIVE: u8 = 2;
pub const K_TIMEOUT_WIN: u8 = 3;
pub const K_RETRY_CAP: u8 = 4;
pub const K_FALLBACK: u8 = 5;

/// B14 断言五族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum B14Case {
    /// 介质序：扫描序严格升序且闭于 [1,8]。
    MediaOrder { seq: [u32; 4], n: u32 },
    /// 默认项存活：默认 tag 命中且存活位真。
    DefaultAlive { tag: u32, alive_tag: u32 },
    /// 超时窗：实际耗时 ∈ [lo, hi]（毫秒）。
    TimeoutWin { actual_ms: u32, lo_ms: u32, hi_ms: u32 },
    /// 重试上限：实际次数 ≤ 上限且 ≥1（零次=没试）。
    RetryCap { actual: u32, cap: u32 },
    /// 回退链闭合：回退链最后一跳是死路之外的活路。
    FallbackClosed { chain: [u32; 3], alive_last: bool },
}

impl B14Case {
    pub fn kind(&self) -> u8 {
        match self {
            B14Case::MediaOrder { .. } => K_MEDIA_ORDER,
            B14Case::DefaultAlive { .. } => K_DEFAULT_ALIVE,
            B14Case::TimeoutWin { .. } => K_TIMEOUT_WIN,
            B14Case::RetryCap { .. } => K_RETRY_CAP,
            B14Case::FallbackClosed { .. } => K_FALLBACK,
        }
    }

    pub fn eval(&self) -> bool {
        match self {
            B14Case::MediaOrder { seq, n } => {
                if *n == 0 || *n as usize > seq.len() {
                    return false;
                }
                let mut prev = 0;
                for i in 0..*n as usize {
                    let v = seq[i];
                    if v <= prev || v < 1 || v > 8 {
                        return false;
                    }
                    prev = v;
                }
                true
            }
            B14Case::DefaultAlive { tag, alive_tag } => *tag != 0 && tag == alive_tag,
            B14Case::TimeoutWin { actual_ms, lo_ms, hi_ms } => {
                actual_ms >= lo_ms && actual_ms <= hi_ms && lo_ms <= hi_ms
            }
            B14Case::RetryCap { actual, cap } => *actual >= 1 && actual <= cap && *cap >= 1,
            B14Case::FallbackClosed { chain, alive_last } => {
                // 链内无重复回退环 + 末跳存活。
                let [a, b, c] = *chain;
                a != b && b != c && a != c && c != 0 && *alive_last
            }
        }
    }

    /// 词位编码（族 + 4 数据词 → 5 词）。
    pub fn words(&self) -> [u32; 5] {
        match self {
            B14Case::MediaOrder { seq, n } => {
                [K_MEDIA_ORDER as u32, seq[0] | (seq[1] << 8) | (seq[2] << 16) | (seq[3] << 24), *n, 0, 0]
            }
            B14Case::DefaultAlive { tag, alive_tag } => {
                [K_DEFAULT_ALIVE as u32, *tag, *alive_tag, 0, 0]
            }
            B14Case::TimeoutWin { actual_ms, lo_ms, hi_ms } => {
                [K_TIMEOUT_WIN as u32, *actual_ms, *lo_ms, *hi_ms, 0]
            }
            B14Case::RetryCap { actual, cap } => [K_RETRY_CAP as u32, *actual, *cap, 0, 0],
            B14Case::FallbackClosed { chain, alive_last } => {
                [K_FALLBACK as u32, chain[0] | (chain[1] << 16), chain[2], *alive_last as u32, 0]
            }
        }
    }
}

/// 具名断言。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B14Assert {
    pub id: u32,
    pub case: B14Case,
}

/// B14 断言集。
pub struct B14Suite {
    cases: Vec<B14Assert>,
}

impl B14Suite {
    pub fn new(cases: &[B14Assert]) -> Result<B14Suite, u32> {
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
        Ok(B14Suite { cases: cases.to_vec() })
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

    /// 五族覆盖核。
    pub fn coverage_ok(&self) -> bool {
        for k in 1..=5u8 {
            if !self.cases.iter().any(|c| c.case.kind() == k) {
                return false;
            }
        }
        true
    }

    /// 序列化：魔数 + 例数 + 每例 (id + 5 词) + FNV 尾。
    pub fn serialize(&self) -> Vec<u32> {
        let mut words = vec![0x42313443u32, self.cases.len() as u32]; // "B14C"
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

/// FNV-1a 64 位。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 反序列化 + 校验和复算。
pub fn deserialize(words: &[u32]) -> Result<B14Suite, u32> {
    if words.len() < 3 || words[0] != 0x42313443 {
        return Err(XC_MAGIC);
    }
    let n = words[1] as usize;
    if words.len() != 2 + n * 6 + 1 {
        return Err(XC_LEN_BAD);
    }
    let mut cases = Vec::new();
    let mut bytes = Vec::new();
    for i in 0..n {
        let base = 2 + i * 6;
        let id = words[base];
        let w = [words[base + 1], words[base + 2], words[base + 3], words[base + 4], words[base + 5]];
        let case = match w[0] {
            1 => B14Case::MediaOrder {
                seq: [w[1] & 0xFF, (w[1] >> 8) & 0xFF, (w[1] >> 16) & 0xFF, (w[1] >> 24) & 0xFF],
                n: w[2],
            },
            2 => B14Case::DefaultAlive { tag: w[1], alive_tag: w[2] },
            3 => B14Case::TimeoutWin { actual_ms: w[1], lo_ms: w[2], hi_ms: w[3] },
            4 => B14Case::RetryCap { actual: w[1], cap: w[2] },
            5 => B14Case::FallbackClosed {
                chain: [w[1] & 0xFFFF, w[1] >> 16, w[2]],
                alive_last: w[3] != 0,
            },
            _ => return Err(XC_KIND_OOR),
        };
        bytes.extend_from_slice(&id.to_le_bytes());
        for x in w.iter() {
            bytes.extend_from_slice(&x.to_le_bytes());
        }
        cases.push(B14Assert { id, case });
    }
    if words[words.len() - 1] != fnv64(&bytes) as u32 {
        return Err(XC_CKSUM);
    }
    B14Suite::new(&cases)
}

pub fn run_unx0280_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0280_b14");
    // 族一：五族全绿样例。
    let cases = vec![
        B14Assert { id: 1, case: B14Case::MediaOrder { seq: [2, 4, 6, 8], n: 4 } },
        B14Assert { id: 2, case: B14Case::DefaultAlive { tag: 100, alive_tag: 100 } },
        B14Assert { id: 3, case: B14Case::TimeoutWin { actual_ms: 500, lo_ms: 100, hi_ms: 1000 } },
        B14Assert { id: 4, case: B14Case::RetryCap { actual: 2, cap: 3 } },
        B14Assert { id: 5, case: B14Case::FallbackClosed { chain: [1, 2, 3], alive_last: true } },
    ];
    let suite = match B14Suite::new(&cases) {
        Ok(s) => s,
        Err(_) => {
            cs.add("断言集构造", false, "被拒");
            return cs;
        }
    };
    cs.add("全绿", suite.run().is_empty(), "五族零红");
    cs.add("收官闸开", suite.close_gate().is_ok(), "放行");
    cs.add("五族覆盖", suite.coverage_ok(), "缺族即不完整");
    // 族二：红灯逐族。
    let bad = vec![
        B14Assert { id: 1, case: B14Case::MediaOrder { seq: [2, 2, 6, 8], n: 4 } },
        B14Assert { id: 2, case: B14Case::DefaultAlive { tag: 100, alive_tag: 200 } },
        B14Assert { id: 3, case: B14Case::TimeoutWin { actual_ms: 1500, lo_ms: 100, hi_ms: 1000 } },
        B14Assert { id: 4, case: B14Case::RetryCap { actual: 0, cap: 3 } },
        B14Assert { id: 5, case: B14Case::FallbackClosed { chain: [1, 1, 2], alive_last: false } },
    ];
    let s2 = B14Suite::new(&bad).unwrap_or(B14Suite { cases: alloc::vec::Vec::new() });
    cs.add("介质序重红", s2.run().contains(&1), "2 重复");
    cs.add("默认项失配红", s2.run().contains(&2), "tag 漂");
    cs.add("超时出窗红", s2.run().contains(&3), "1500>1000");
    cs.add("零次重试红", s2.run().contains(&4), "没试");
    cs.add("回退环红", s2.run().contains(&5), "1→1 环+末跳死");
    cs.add("收官拦", s2.close_gate() == Err(XC_RED_ASSERT), "有红不放");
    // 族三：构造拒。
    cs.add("空集拒", B14Suite::new(&[]).is_err(), "EMPTY");
    cs.add("重号拒", B14Suite::new(&[
        B14Assert { id: 1, case: B14Case::RetryCap { actual: 1, cap: 1 } },
        B14Assert { id: 1, case: B14Case::RetryCap { actual: 1, cap: 1 } },
    ]).is_err(), "DUP");
    cs.add("号越域拒", B14Suite::new(&[
        B14Assert { id: 513, case: B14Case::RetryCap { actual: 1, cap: 1 } },
    ]).is_err(), "KIND_OOR");
    // 族四：序列化复跑。
    let words = suite.serialize();
    cs.add("词位长", words.len() == 2 + 5 * 6 + 1, "头2+5×6+校验1");
    let back = deserialize(&words);
    cs.add("复跑零漂", match back {
        Ok(s3) => s3.run() == suite.run() && s3.coverage_ok(),
        Err(_) => false,
    }, "红绿与覆盖双对");
    // 族五：篡改检出。
    let mut t = words.clone();
    t[4] ^= 1;
    cs.add("数据篡改拒", deserialize(&t).is_err(), "CKSUM");
    let mut t2 = words.clone();
    t2[0] = 0x41414141;
    cs.add("魔数拒", deserialize(&t2).is_err(), "MAGIC");
    let mut t3 = words.clone();
    let last = t3.len() - 1;
    t3[last] = t3[last].wrapping_add(1);
    cs.add("校验词拒", deserialize(&t3).is_err(), "CKSUM");
    cs
}
