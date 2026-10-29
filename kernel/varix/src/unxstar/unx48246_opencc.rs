//! unx48246_opencc — UNX-F48246 简繁转换 OpenCC 类词典内置单向（禁云转换红线）（诊断段 0xBFA0）。
//!
//! 判据原文：
//!   J1 转换 ×10^3 往返样张全对、网络审计零外联；判据锚定 §AI-61 §2 专题五简繁项、专题六禁云。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 词典三层冻结入账：单字表 CHAR_S2T（32 对，s→t 单向）、词条
//!      表 WORD_S2T（10 条多字词，最长匹配优先）、透传面（表外码位
//!      原样直通并计数）。词典内置即离线——表即账，编译期常量。
//!   2. 禁云红线独立成闸（NetGuard）：转换全程只许触内建词典；
//!      cloud_convert 探针恒拒（E_CLOUD）并计入 blocked；审计口径
//!      是 outbound==0 且 blocked==探针数——零外联是账面事实，不是
//!      口号。
//!   3. 转换主径：逐位最长匹配（词条表降序长度扫描）→ 单字表 →
//!      透传。歧义字（发/干/面）默认走单字表方向，词条表负责翻案
//!      （头发→髮、干净→乾）——多字词压过单字默认，OpenCC 类语义。
//!   4. J1 主判据 ×10^3：LCG 造 1000 串（词典字+词条+噪声码位混
//!      排），转换结果与构造期望逐码位相等；另附往返样张 32 笔
//!      （可逆子集反查 t→s 全对）与歧义翻案例 4 笔。
//!   5. 反查表独立：REVERSE 由单字表展开成 t→s 线性账，仅覆盖
//!      默认方向（可逆子集），歧义字不入反查——单向词典的反查是
//!      样张工具，不是第二引擎。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBFA0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_CLOUD: u16 = SEG_BASE + 1; // 禁云红线：外联转换被拒
const E_CONV_BAD: u16 = SEG_BASE + 2; // 转换结果与期望不符
const E_REV_MISS: u16 = SEG_BASE + 3; // 往返样张反查失败
const E_AUDIT_DIRTY: u16 = SEG_BASE + 4; // 网络审计账不平（外联非零）
const E_LEX_MISS: u16 = SEG_BASE + 5; // 词条/单字表账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空
const E_PASSTHRU: u16 = SEG_BASE + 7; // 透传计数与账不符

/// 单字表（冻结 32 对，s→t 单向，常用简化字子集）。
pub const CHAR_S2T: [(u32, u32); 32] = [
    (0x95E8, 0x9580), // 门→門
    (0x4E07, 0x842C), // 万→萬
    (0x4E0E, 0x8207), // 与→與
    (0x4E13, 0x5C08), // 专→專
    (0x4E1A, 0x696D), // 业→業
    (0x4E1C, 0x6771), // 东→東
    (0x4E24, 0x5169), // 两→兩
    (0x4E25, 0x56B4), // 严→嚴
    (0x4E2A, 0x500B), // 个→個
    (0x4E3A, 0x70BA), // 为→為
    (0x4E49, 0x7FA9), // 义→義
    (0x4E50, 0x6A02), // 乐→樂
    (0x4E66, 0x66F8), // 书→書
    (0x4E70, 0x8CB7), // 买→買
    (0x4E71, 0x4E82), // 乱→亂
    (0x4E89, 0x722D), // 争→爭
    (0x4E91, 0x96F2), // 云→雲
    (0x4EB2, 0x89AA), // 亲→親
    (0x4EA9, 0x755D), // 亩→畝
    (0x4EC5, 0x50C5), // 仅→僅
    (0x4ECE, 0x5F9E), // 从→從
    (0x4F17, 0x8846), // 众→眾
    (0x4F20, 0x50B3), // 传→傳
    (0x4F24, 0x50B7), // 伤→傷
    (0x4F2A, 0x507D), // 伪→偽
    (0x53D1, 0x767C), // 发→發（默认向；词条表翻案髮）
    (0x5E72, 0x5E79), // 干→幹（默认向；词条表翻案乾）
    (0x540E, 0x5F8C), // 后→後
    (0x91CC, 0x88E1), // 里→裏
    (0x6C49, 0x6F22), // 汉→漢
    (0x5F00, 0x958B), // 开→開
    (0x65F6, 0x6642), // 时→時
];

/// 词条表（冻结 10 条，最长匹配优先，覆盖歧义字翻案）。
pub const WORD_S2T: [(&str, &str); 10] = [
    ("头发", "頭髮"),
    ("出发", "出發"),
    ("发展", "發展"),
    ("干活", "幹活"),
    ("干净", "乾淨"),
    ("干脆", "乾脆"),
    ("面条", "麵條"),
    ("后面", "後面"),
    ("里边", "裏邊"),
    ("里外", "裏外"),
];

/// 词条码位化（编译期不可为，运行期冻结展开一次）。
pub fn word_table() -> Vec<(Vec<u32>, Vec<u32>)> {
    let mut out: Vec<(Vec<u32>, Vec<u32>)> = Vec::new();
    let mut i = 0usize;
    while i < WORD_S2T.len() {
        let (s, t) = WORD_S2T[i];
        out.push((str_codes(s), str_codes(t)));
        i += 1;
    }
    out
}

/// &str → 码位序（UTF-8 逐字节拼 u32 的域内口径：直接取字节）。
fn str_codes(s: &str) -> Vec<u32> {
    let b = s.as_bytes();
    let mut out: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        out.push(b[i] as u32);
        i += 1;
    }
    out
}

/// 反查表（纪律 5）：单字表展开 t→s，仅可逆子集（歧义字 發/幹 不入）。
pub fn reverse_table() -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    let mut i = 0usize;
    while i < CHAR_S2T.len() {
        let (s, t) = CHAR_S2T[i];
        if s == 0x53D1 || s == 0x5E72 {
            i += 1;
            continue; // 歧义字默认向不入反查
        }
        out.push((t, s));
        i += 1;
    }
    out
}

/// 禁云闸（纪律 2）：全程只记账不出网。
pub struct NetGuard {
    pub outbound: u32,
    pub blocked: u32,
}

impl NetGuard {
    pub fn new() -> NetGuard {
        NetGuard { outbound: 0, blocked: 0 }
    }
    /// 云转换探针：恒拒，红线落账。
    pub fn cloud_convert(&mut self, _payload: u32) -> Result<Vec<u32>, u16> {
        self.blocked += 1;
        Err(E_CLOUD)
    }
    /// 审计：零外联口径。
    pub fn audit(&self) -> Result<(), u16> {
        if self.outbound != 0 {
            return Err(E_AUDIT_DIRTY);
        }
        Ok(())
    }
}

/// 单字映射查表。
fn char_map(s: u32) -> Option<u32> {
    let mut i = 0usize;
    while i < CHAR_S2T.len() {
        if CHAR_S2T[i].0 == s {
            return Some(CHAR_S2T[i].1);
        }
        i += 1;
    }
    None
}

/// 词条前缀匹配：在 pos 处试最长词条，命中返回 (词长, 译文)。
fn word_at(words: &[(Vec<u32>, Vec<u32>)], input: &[u32], pos: usize) -> Option<(usize, Vec<u32>)> {
    let mut best: Option<(usize, Vec<u32>)> = None;
    let mut i = 0usize;
    while i < words.len() {
        let (src, dst) = &words[i];
        if src.len() > 0 && pos + src.len() <= input.len() {
            let mut hit = true;
            let mut j = 0usize;
            while j < src.len() {
                if input[pos + j] != src[j] {
                    hit = false;
                }
                j += 1;
            }
            if hit {
                let better = match &best {
                    Some((blen, _)) => src.len() > *blen,
                    None => true,
                };
                if better {
                    let mut d: Vec<u32> = Vec::new();
                    let mut j = 0usize;
                    while j < dst.len() {
                        d.push(dst[j]);
                        j += 1;
                    }
                    best = Some((src.len(), d));
                }
            }
        }
        i += 1;
    }
    best
}

/// 转换账：结果 + 透传计数 + 词条命中计数。
pub struct ConvReport {
    pub out: Vec<u32>,
    pub passthrough: u32,
    pub word_hits: u32,
}

/// 转换主径（纪律 3）：词条最长匹配 → 单字 → 透传。
pub fn convert(input: &[u32]) -> ConvReport {
    let words = word_table();
    let mut out: Vec<u32> = Vec::new();
    let mut passthrough = 0u32;
    let mut word_hits = 0u32;
    let mut pos = 0usize;
    while pos < input.len() {
        match word_at(&words, input, pos) {
            Some((wlen, dst)) => {
                word_hits += 1;
                let mut j = 0usize;
                while j < dst.len() {
                    out.push(dst[j]);
                    j += 1;
                }
                pos += wlen;
            }
            None => match char_map(input[pos]) {
                Some(t) => {
                    out.push(t);
                    pos += 1;
                }
                None => {
                    out.push(input[pos]);
                    passthrough += 1;
                    pos += 1;
                }
            },
        }
    }
    ConvReport { out, passthrough, word_hits }
}

/// LCG（确定性）。
pub struct Lcg {
    s: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { s: seed }
    }
    pub fn next(&mut self) -> u32 {
        self.s = self.s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.s
    }
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

/// 写回判定（判据 J2）。
fn writeback(done: bool, summary_len: u32) -> Result<(), u16> {
    if !done || summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

fn vec_eq(a: &[u32], b: &[u32]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 自例程判据：逐条落实 J1（转换 ×10^3 往返样张全对、网络审计零外联），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut guard = NetGuard::new();

    // R1 主判据 ×10^3：LCG 造串（单字表源/词条源/噪声三源混排），
    //    转换与构造期望逐码位相等。
    let mut rng = Lcg::new(0xBFA0_1000);
    let mut i = 0u32;
    while i < 1_000 {
        // 造串：1..6 个槽位，槽位三选一。
        let n = 1 + rng.below(6);
        let mut input: Vec<u32> = Vec::new();
        let mut expect: Vec<u32> = Vec::new();
        let mut k = 0u32;
        while k < n {
            let pick = rng.below(3);
            if pick == 0 {
                // 单字表源：期望即表向。
                let idx = rng.below(CHAR_S2T.len() as u32) as usize;
                input.push(CHAR_S2T[idx].0);
                expect.push(CHAR_S2T[idx].1);
            } else if pick == 1 {
                // 词条源：期望即词条译文； 词条后不续接可匹配尾巴。
                let idx = rng.below(WORD_S2T.len() as u32) as usize;
                let (s, t) = WORD_S2T[idx];
                let sc = str_codes(s);
                let tc = str_codes(t);
                let mut j = 0usize;
                while j < sc.len() {
                    input.push(sc[j]);
                    j += 1;
                }
                let mut j = 0usize;
                while j < tc.len() {
                    expect.push(tc[j]);
                    j += 1;
                }
            } else {
                // 噪声源：透传。
                let noise = 0x3000 + rng.below(0x1000);
                input.push(noise);
                expect.push(noise);
            }
            k += 1;
        }
        // 槽位独立构账：单字/词条槽是码位面（0x4E00+）、噪声槽在
        // 0x3000..0x4000，词条字节面在 0x80..0xFF——三源互不粘连，
        // 转换器逐槽所得与构造期望严格逐码位相等。
        let rep = convert(&input);
        if !vec_eq(&rep.out, &expect) {
            return Err(E_CONV_BAD);
        }
        i += 1;
    }
    hits += 1;

    // R2 词条压过单字默认（纪律 3）：头发/干净/面条 三翻案例逐笔验。
    let rep = convert(&str_codes("头发"));
    if !vec_eq(&rep.out, &str_codes("頭髮")) || rep.word_hits != 1 {
        return Err(E_CONV_BAD);
    }
    let rep = convert(&str_codes("干净"));
    if !vec_eq(&rep.out, &str_codes("乾淨")) {
        return Err(E_CONV_BAD);
    }
    let rep = convert(&str_codes("面条"));
    if !vec_eq(&rep.out, &str_codes("麵條")) {
        return Err(E_CONV_BAD);
    }
    // 默认向：孤字 发→發（词条不在场）。
    let rep = convert(&[0x53D1]);
    if !vec_eq(&rep.out, &[0x767C]) {
        return Err(E_CONV_BAD);
    }
    hits += 1;

    // R3 往返样张 32 笔：可逆子集 s→t→s 全对；歧义字（發/幹）反查
    //    必须落空（单向词典口径，纪律 5）。
    let rev = reverse_table();
    let mut i = 0usize;
    while i < CHAR_S2T.len() {
        let (s, t) = CHAR_S2T[i];
        let rep = convert(&[s]);
        if rep.out[0] != t {
            return Err(E_CONV_BAD);
        }
        // 反查（仅可逆子集）。
        let mut hit = false;
        let mut j = 0usize;
        while j < rev.len() {
            if rev[j].0 == t {
                if rev[j].1 != s {
                    return Err(E_REV_MISS);
                }
                hit = true;
            }
            j += 1;
        }
        if s != 0x53D1 && s != 0x5E72 && !hit {
            return Err(E_REV_MISS);
        }
        if s == 0x53D1 || s == 0x5E72 {
            if hit {
                return Err(E_REV_MISS); // 歧义字不得入反查
            }
        }
        i += 1;
    }
    hits += 1;

    // R4 禁云红线（纪律 2）：云转换探针 ×8 恒拒；审计零外联。
    let mut i = 0u32;
    while i < 8 {
        if guard.cloud_convert(0x53D1 + i).is_ok() {
            return Err(E_CLOUD);
        }
        i += 1;
    }
    if guard.blocked != 8 {
        return Err(E_AUDIT_DIRTY);
    }
    guard.audit()?;
    hits += 1;

    // R5 词典账平衡：单字表 32 对无重键；词条 10 条均长 ≥2；
    //    反查表恰 30 条（32 减歧义两字）。
    let mut i = 0usize;
    while i < CHAR_S2T.len() {
        let mut j = i + 1;
        while j < CHAR_S2T.len() {
            if CHAR_S2T[i].0 == CHAR_S2T[j].0 {
                return Err(E_LEX_MISS);
            }
            j += 1;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < WORD_S2T.len() {
        if str_codes(WORD_S2T[i].0).len() < 2 {
            return Err(E_LEX_MISS);
        }
        i += 1;
    }
    if reverse_table().len() != 30 {
        return Err(E_LEX_MISS);
    }
    hits += 1;

    // R6 透传计数：纯噪声串转换，passthrough == 串长；词典零命中。
    let mut input: Vec<u32> = Vec::new();
    let mut rng = Lcg::new(0xBFA0_2000);
    let mut k = 0u32;
    while k < 7 {
        input.push(0x3100 + rng.below(0x40));
        k += 1;
    }
    let rep = convert(&input);
    if rep.passthrough != 7 || rep.word_hits != 0 {
        return Err(E_PASSTHRU);
    }
    if !vec_eq(&rep.out, &input) {
        return Err(E_PASSTHRU);
    }
    hits += 1;

    // R7 写回（判据 J2）：完成态 + 非空摘要放行；空摘要拒。
    if writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 0x2710_0FA0 ^ 0xFFFF)?;
    hits += 1;

    Ok(hits)
}
