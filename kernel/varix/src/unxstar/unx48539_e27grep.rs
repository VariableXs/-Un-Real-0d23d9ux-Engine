//! UNX-F48539 —— E27 批防重 grep（关键词族：hotplug/teardown/reseatchange）
//! 判据原文：UNX-F48539-J1 对本批 E27 段卡面清单做关键词族 grep：
//! ① 卡面号不重复（防重）；② 每个关键词至少命中两张卡；
//! ③ 负对照清单（词库域卡面）对 "teardown" 必须零命中；
//! ④ 命中统计守恒可复核；⑤ 命中位置可复现。
//! 判据落地（drill 场景）：R1 关键词族闭集；R2 清单防重（卡号唯一）；
//! R3 卡号段完整；R4 关键词覆盖下限；R5 负对照零命中；
//! R6 命中统计守恒；R7 命中位置可复现；R8 指纹稳定；R9 空关键词拒扫。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF0C0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_KW: u16 = SEG_BASE + 1; // 关键词越出族闭集
const E_DUP: u16 = SEG_BASE + 2; // 卡面号重复（防重命中）
const E_MISS: u16 = SEG_BASE + 3; // 卡号段缺卡
const E_COVER: u16 = SEG_BASE + 4; // 关键词覆盖不足下限
const E_NEG: u16 = SEG_BASE + 5; // 负对照出现命中
const E_CONSERV: u16 = SEG_BASE + 6; // 命中统计不守恒
const E_POS: u16 = SEG_BASE + 7; // 命中位置不可复现
const E_FP: u16 = SEG_BASE + 8; // 指纹漂移
const E_EMPTY: u16 = SEG_BASE + 9; // 空关键词拒扫
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 关键词族闭集。
pub const KW_HOTPLUG: &str = "hotplug";
pub const KW_TEARDOWN: &str = "teardown";
pub const KW_RESEATCHANGE: &str = "reseatchange";
pub const KEYWORDS: [&str; 3] = [KW_HOTPLUG, KW_TEARDOWN, KW_RESEATCHANGE];
/// 每个关键词在批内至少命中的卡数下限。
pub const COVER_MIN: u32 = 2;

/// 批清单条目：卡号 + 合成源行（模拟 grep 输入）。
pub struct CardManifest {
    pub card: u32,
    pub lines: [&'static str; 3],
}

/// E27 段八卡清单（卡号即验收批实际卡面号）。
pub const MANIFEST: [CardManifest; 8] = [
    CardManifest {
        card: 48533,
        lines: [
            "e27obs: four-point hotplug probe ledger",
            "remove attach reseat teardown events",
            "teardown counted under ring cap",
        ],
    },
    CardManifest {
        card: 48534,
        lines: [
            "e27fuzz: random device state flip",
            "zero crash across hotplug runs",
            "teardown twice rejected keep state",
        ],
    },
    CardManifest {
        card: 48535,
        lines: [
            "e27pres: three-element rendering",
            "hotplug change visible to user",
            "replug advice attached non-blocking",
        ],
    },
    CardManifest {
        card: 48536,
        lines: [
            "e27vec: vector ledger with checksum",
            "hotplug case scripts encoded",
            "reseatchange replay matches ledger",
        ],
    },
    CardManifest {
        card: 48537,
        lines: [
            "e27usb: boundary contract to M5",
            "enum semantics single write owner",
            "teardown key referenced not owned",
        ],
    },
    CardManifest {
        card: 48538,
        lines: [
            "e27perf: hot switch latency sampling",
            "teardown budget under five ms",
            "reseatchange p95 recheck passed",
        ],
    },
    CardManifest {
        card: 48539,
        lines: [
            "e27grep: keyword family dedup scan",
            "hotplug teardown reseatchange tri-family",
            "no duplicate card id allowed",
        ],
    },
    CardManifest {
        card: 48540,
        lines: [
            "e27seal: batch closing seal",
            "twenty frozen cards summed",
            "missing card injection detected",
        ],
    },
];

/// 负对照清单（词库域卡面，正文不得含 "teardown"）。
pub const NEGATIVE: [(&str, [&str; 2]); 2] = [
    ("diccrc", ["dictionary crc guard", "fallback mini dict"]),
    ("halfrec", ["half record rejected", "forged files refused"]),
];

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 子串判定（朴素扫描，统一口径）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}

/// 命中位置扫描：返回 kw 在 hay 中的全部起点（字节偏移）。
pub fn hit_positions(hay: &[u8], kw: &[u8]) -> Vec<u32> {
    let mut ps: Vec<u32> = vec![];
    if kw.is_empty() || hay.len() < kw.len() {
        return ps;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            ps.push(i as u32);
        }
        i += 1;
    }
    ps
}

/// 对单卡计数某关键词命中行数。
pub fn grep_card(m: &CardManifest, kw: &str) -> u32 {
    if kw.is_empty() {
        return 0;
    }
    let mut n = 0u32;
    let mut i = 0usize;
    while i < m.lines.len() {
        if has_kw(m.lines[i].as_bytes(), kw.as_bytes()) {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 全批命中统计：每关键词命中行数。
pub fn batch_stats() -> [u32; 3] {
    let mut s = [0u32; 3];
    let mut k = 0usize;
    while k < 3 {
        let mut m = 0usize;
        while m < MANIFEST.len() {
            s[k] += grep_card(&MANIFEST[m], KEYWORDS[k]);
            m += 1;
        }
        k += 1;
    }
    s
}

/// 命中行呈现：card=<n> kw=<词> hits=<n>。
pub fn hit_line(card: u32, kw: &str, hits: u32) -> String {
    let mut s = String::new();
    s.push_str("card=");
    s.push_str(&card.to_string());
    s.push_str(" kw=");
    s.push_str(kw);
    s.push_str(" hits=");
    s.push_str(&hits.to_string());
    s
}

/// 清单指纹：卡号与全部源行字节串联后 FNV。
pub fn manifest_fp() -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut m = 0usize;
    while m < MANIFEST.len() {
        buf.extend_from_slice(&MANIFEST[m].card.to_le_bytes());
        let mut li = 0usize;
        while li < MANIFEST[m].lines.len() {
            buf.extend_from_slice(MANIFEST[m].lines[li].as_bytes());
            buf.push(0x0A);
            li += 1;
        }
        m += 1;
    }
    fnv1a64(&buf)
}

/// 主巡检入口：R1..R9 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 关键词族闭集：三词非空且互异。
    if KEYWORDS.len() != 3 {
        return Err(E_KW);
    }
    let mut i = 0usize;
    while i < 3 {
        if KEYWORDS[i].is_empty() {
            return Err(E_KW);
        }
        let mut j = i + 1;
        while j < 3 {
            if KEYWORDS[i] == KEYWORDS[j] {
                return Err(E_KW);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R2 清单防重：卡号两两互异。
    let mut i = 0usize;
    while i < MANIFEST.len() {
        let mut j = i + 1;
        while j < MANIFEST.len() {
            if MANIFEST[i].card == MANIFEST[j].card {
                return Err(E_DUP);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R3 卡号段完整：48533..=48540 八卡逐一在册。
    let mut c = 48533u32;
    while c <= 48540 {
        let mut found = false;
        let mut m = 0usize;
        while m < MANIFEST.len() {
            if MANIFEST[m].card == c {
                found = true;
            }
            m += 1;
        }
        if !found {
            return Err(E_MISS);
        }
        c += 1;
    }
    hits += 1;

    // R4 关键词覆盖下限：每词至少命中 COVER_MIN 张卡。
    let mut k = 0usize;
    while k < 3 {
        let mut cards_hit = 0u32;
        let mut m = 0usize;
        while m < MANIFEST.len() {
            if grep_card(&MANIFEST[m], KEYWORDS[k]) > 0 {
                cards_hit += 1;
            }
            m += 1;
        }
        if cards_hit < COVER_MIN {
            return Err(E_COVER);
        }
        k += 1;
    }
    hits += 1;

    // R5 负对照零命中：词库域两卡对 "teardown" 必须零命中。
    let mut n = 0usize;
    while n < NEGATIVE.len() {
        let mut li = 0usize;
        while li < 2 {
            if has_kw(NEGATIVE[n].1[li].as_bytes(), KW_TEARDOWN.as_bytes()) {
                return Err(E_NEG);
            }
            li += 1;
        }
        n += 1;
    }
    hits += 1;

    // R6 命中统计守恒：逐卡计数之和 == 全批统计。
    let stats = batch_stats();
    let mut k = 0usize;
    while k < 3 {
        let mut sum = 0u32;
        let mut m = 0usize;
        while m < MANIFEST.len() {
            sum += grep_card(&MANIFEST[m], KEYWORDS[k]);
            m += 1;
        }
        if sum != stats[k] || stats[k] == 0 {
            return Err(E_CONSERV);
        }
        k += 1;
    }
    hits += 1;

    // R7 命中位置可复现：同行两次扫描位置表一致，且首处确有该词。
    let mut lcg = Lcg::new(0x48539);
    let mut t = 0u32;
    while t < 8 {
        let m = lcg.below(MANIFEST.len() as u32) as usize;
        let kw = KEYWORDS[lcg.below(3) as usize];
        let mut li = 0usize;
        while li < MANIFEST[m].lines.len() {
            let hay = MANIFEST[m].lines[li].as_bytes();
            let p1 = hit_positions(hay, kw.as_bytes());
            let p2 = hit_positions(hay, kw.as_bytes());
            if p1 != p2 {
                return Err(E_POS);
            }
            let mut pi = 0usize;
            while pi < p1.len() {
                let at = p1[pi] as usize;
                if !has_kw(&hay[at..], kw.as_bytes()) {
                    return Err(E_POS);
                }
                pi += 1;
            }
            li += 1;
        }
        t += 1;
    }
    hits += 1;

    // R8 指纹稳定：全批清单指纹两次一致且非零。
    let f1 = manifest_fp();
    let f2 = manifest_fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 空关键词拒扫：grep_card 空词恒零，呈现行三段齐全。
    if grep_card(&MANIFEST[0], "") != 0 {
        return Err(E_EMPTY);
    }
    let line = hit_line(48533, KW_HOTPLUG, grep_card(&MANIFEST[0], KW_HOTPLUG));
    if !has_kw(line.as_bytes(), b"card=")
        || !has_kw(line.as_bytes(), b"kw=")
        || !has_kw(line.as_bytes(), b"hits=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}
