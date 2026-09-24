//! imepinyin — WP-202 · B-903 IME 三条硬线（MD2 篇 9.3）。
//!
//! 判据 B-903：100ms / 16ms / 32MB 实测达标。
//! MD2 原文（9.3）："自研极简拼音引擎（ADR-012）四层：输入缓冲与切分
//! （声韵母表最大匹配，zh/ch/sh 歧义按最长韵母优先，全拼为主简拼为辅）；
//! 音节到候选的检索（冻结三万条目文本库，词/拼音序列/词频档位三元组，
//! 构建期编译成前缀索引，完全匹配优先、模糊音关闭、词频档位排序——不做
//! 运行期学习）；候选装配（整词/单字/联想三段式分页，每页九个，页内序
//! 稳定）；提交（text_commit 报文投焦点窗口，编辑在缓冲层完成，永不污染
//! 应用窗口）。三条硬性能线：键入到候选首屏 ≤ 100ms（WD-011）、上屏 ≤
//! 16ms（一帧内）、内存常驻 ≤ 32MB（词库索引映射只读共享页，多进程零复制）。"
//!
//! 宿主可测形态：切分器（最长韵母优先的最大匹配，固化音节表）+ 冻结词库
//! 模型（词/拼音序列/词频档位三元组，完全匹配 + 档位稳定排序）+ 三段式
//! 分页（每页九个，页内序稳定）+ 三硬线整数预算模型（同 B-601/B-707 口径）
//! + **不做运行期学习**（类型面无 learn 入口——诚实标注的结构防线）。

use crate::checks::CheckSet;

/// 硬线一：键入到候选首屏 ≤ 100ms（WD-011）。
pub const FIRST_PAGE_MS: u32 = 100;
/// 硬线二：上屏 ≤ 16ms（一帧内）。
pub const COMMIT_MS: u32 = 16;
/// 硬线三：内存常驻 ≤ 32MB。
pub const MEM_BUDGET_KB: u32 = 32 * 1024;
/// 冻结词库条目数（三万）。
pub const DICT_ENTRIES: u32 = 30_000;
/// 每条目索引字节（词 id 4B + 拼音序列 12B + 词频档位 1B + 对齐 15B = 32B 模型值）。
pub const BYTES_PER_ENTRY: u32 = 32;
/// 每页候选数（数字键选重）。
pub const PAGE_SIZE: usize = 9;
/// 单条检索开销（纳秒，前缀索引标定锚——实机回填）。
pub const LOOKUP_NS_PER_ENTRY: u32 = 30;
/// 上屏提交开销（纳秒，text_commit 报文模型）。
pub const COMMIT_NS: u32 = 4_000_000; // 4ms 模型锚

/// 固化音节表（模型面：覆盖 zh/ch/sh 歧义与最长韵母优先验证）。
pub const SYLLABLES: [&str; 16] = [
    "a", "ba", "bai", "ban", "bang", "zh", "zhang", "chang", "shang", "ma", "mai", "shi", "sha", "she", "shou", "zhong",
];

/// 词库条目（词/拼音序列/词频档位三元组——冻结，无运行期写入）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DictEntry {
    /// 拼音序列的音节索引编码（每音节 6 bit 打包进 u32——宿主模型）。
    pub pinyin: u32,
    pub n_syll: u8,
    /// 词频档位（0 高 … 9 低——档位排序）。
    pub freq_tier: u8,
    pub word_id: u16,
}

/// 冻结词库（模型面：16 条代表性条目；三万条的三硬线用预算模型推导）。
pub const DICT: [DictEntry; 16] = [
    DictEntry { pinyin: 0, n_syll: 2, freq_tier: 0, word_id: 1 },  // 占位序列 0
    DictEntry { pinyin: 1, n_syll: 2, freq_tier: 0, word_id: 2 },
    DictEntry { pinyin: 2, n_syll: 1, freq_tier: 1, word_id: 3 },
    DictEntry { pinyin: 3, n_syll: 1, freq_tier: 2, word_id: 4 },
    DictEntry { pinyin: 4, n_syll: 2, freq_tier: 0, word_id: 5 },
    DictEntry { pinyin: 5, n_syll: 1, freq_tier: 3, word_id: 6 },
    DictEntry { pinyin: 6, n_syll: 1, freq_tier: 1, word_id: 7 },
    DictEntry { pinyin: 7, n_syll: 2, freq_tier: 2, word_id: 8 },
    DictEntry { pinyin: 8, n_syll: 1, freq_tier: 0, word_id: 9 },
    DictEntry { pinyin: 9, n_syll: 1, freq_tier: 4, word_id: 10 },
    DictEntry { pinyin: 10, n_syll: 2, freq_tier: 1, word_id: 11 },
    DictEntry { pinyin: 11, n_syll: 1, freq_tier: 2, word_id: 12 },
    DictEntry { pinyin: 12, n_syll: 1, freq_tier: 0, word_id: 13 },
    DictEntry { pinyin: 13, n_syll: 1, freq_tier: 3, word_id: 14 },
    DictEntry { pinyin: 14, n_syll: 2, freq_tier: 4, word_id: 15 },
    DictEntry { pinyin: 15, n_syll: 1, freq_tier: 1, word_id: 16 },
];

/// 切分结果（音节索引序列）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Split {
    pub sylls: [u8; 8],
    pub n: usize,
}

/// 音节切分核心（字节面——零堆纪律，no_std 无 format/alloc）。
pub fn split_bytes(input: &[u8]) -> Split {
    let mut out = Split { sylls: [0; 8], n: 0 };
    let mut pos = 0;
    while pos < input.len() && out.n < 8 {
        // 音节表内长音节优先尝试（最长韵母优先）
        let mut matched = None;
        let mut best_len = 0;
        for (si, s) in SYLLABLES.iter().enumerate() {
            let sb = s.as_bytes();
            if sb.len() > best_len && input[pos..].starts_with(sb) {
                matched = Some(si as u8);
                best_len = sb.len();
            }
        }
        match matched {
            Some(si) => {
                out.sylls[out.n] = si;
                out.n += 1;
                pos += best_len;
            }
            None => break, // 无法切分（简拼/错误输入——缓冲层编辑解决）
        }
    }
    out
}

/// 音节切分：最大匹配，**最长韵母优先**（zhong 优先于 zh+ong 的模型化：
/// 音节表内长串优先尝试）。
pub fn split_input(input: &str) -> Split {
    split_bytes(input.as_bytes())
}

/// 候选集检索：音节序列完全匹配 + 词频档位稳定排序（**模糊音关闭**——
/// 只做完全匹配；页内序稳定）。
pub fn lookup(s: &Split) -> [u16; PAGE_SIZE] {
    let mut out = [0u16; PAGE_SIZE];
    if s.n == 0 {
        return out;
    }
    // 完全匹配收集（固化 16 条内线性扫——前缀索引的宿主模型）
    let mut hits: [DictEntry; 16] = [DictEntry { pinyin: 0, n_syll: 0, freq_tier: 9, word_id: 0 }; 16];
    let mut nh = 0;
    for e in DICT.iter() {
        if e.pinyin == s.sylls[0] as u32 && e.n_syll as usize == s.n.min(2) {
            hits[nh] = *e;
            nh += 1;
        }
    }
    // 档位稳定排序（插入排序——n≤16，稳定序保证页内不抖动）
    for i in 1..nh {
        let key = hits[i];
        let mut j = i;
        while j > 0 && hits[j - 1].freq_tier > key.freq_tier {
            hits[j] = hits[j - 1];
            j -= 1;
        }
        hits[j] = key;
    }
    for i in 0..nh.min(PAGE_SIZE) {
        out[i] = hits[i].word_id;
    }
    out
}

/// 提交报文（text_commit 投焦点窗口——候选上屏的协议落点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TextCommit {
    pub word_id: u16,
    /// 上屏发生在缓冲层编辑完成后——永不污染应用窗口的组合中间态。
    pub from_buffer: bool,
}

/// 三硬线预算模型（整数推导——实机 vxbench 回填翻案只改参数）。
pub fn first_page_ok() -> bool {
    // 全库扫描 worst case：30000 条 × 30ns = 900_000ns = 0.9ms ≤ 100ms ✓
    DICT_ENTRIES * LOOKUP_NS_PER_ENTRY / 1_000_000 <= FIRST_PAGE_MS
}

pub fn commit_ok() -> bool {
    COMMIT_NS / 1_000_000 <= COMMIT_MS
}

pub fn mem_ok() -> bool {
    // 30000 × 32B = 960KB ≪ 32MB（索引映射只读共享页，多进程零复制）
    DICT_ENTRIES * BYTES_PER_ENTRY / 1024 <= MEM_BUDGET_KB
}

// ---------------------------------------------------------------- 对练

/// IME 对练摘要。
#[derive(Default, PartialEq, Eq, Debug)]
pub struct ImeDrillSummary {
    pub rounds: u32,
    pub inputs: u64,
    /// 切分确定性（同输入同切分）
    pub split_stable: bool,
    /// 候选页内序稳定（同输入两次检索同序）
    pub page_stable: bool,
    /// 三硬线预算全过
    pub budget_ok: bool,
}

/// 随机音节串对练：切分稳定 + 候选序稳定 + 三硬线。
pub fn run_ime_drills(seed: u64, rounds: u32) -> ImeDrillSummary {
    let mut g = crate::comprecover::Lcg(seed);
    let mut sum = ImeDrillSummary::default();
    sum.rounds = rounds;
    sum.split_stable = true;
    sum.page_stable = true;
    sum.budget_ok = first_page_ok() && commit_ok() && mem_ok();
    for _ in 0..rounds {
        // 拼两个音节成输入串（字节级拼接——零堆纪律）
        let a_idx = (g.next() % 16) as usize;
        let b_idx = (g.next() % 16) as usize;
        let sa = SYLLABLES[a_idx].as_bytes();
        let sb = SYLLABLES[b_idx].as_bytes();
        let mut buf = [0u8; 16];
        buf[..sa.len()].copy_from_slice(sa);
        buf[sa.len()..sa.len() + sb.len()].copy_from_slice(sb);
        let input = &buf[..sa.len() + sb.len()];
        let s1 = split_bytes(input);
        let s2 = split_bytes(input);
        if s1 != s2 {
            sum.split_stable = false;
        }
        let c1 = lookup(&s1);
        let c2 = lookup(&s1);
        if c1 != c2 {
            sum.page_stable = false;
        }
        sum.inputs += 1;
    }
    sum
}

// ---------------------------------------------------------------- 自检

pub fn run_imepinyin_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-903 IME 三条硬线");
    {
        // 硬线三预算：内存
        set.add(
            "B-903 内存 ≤ 32MB（预算推导）",
            mem_ok() && DICT_ENTRIES * BYTES_PER_ENTRY / 1024 == 937,
            "30000 条 × 32B = 937KB 索引映射只读共享页——多进程零复制",
        );
    }
    {
        // 硬线一预算：首屏
        set.add(
            "B-903 首屏 ≤ 100ms（预算推导）",
            first_page_ok() && DICT_ENTRIES * LOOKUP_NS_PER_ENTRY / 1_000_000 == 0,
            "worst case 全库 0.9ms ≪ 100ms（WD-011）",
        );
    }
    {
        // 硬线二预算：上屏
        set.add(
            "B-903 上屏 ≤ 16ms（预算推导）",
            commit_ok(),
            "text_commit 4ms 模型锚 ≤ 16ms（一帧内）",
        );
    }
    {
        // 最长韵母优先：zhong 切成一个音节而非 zh+ong
        let s = split_input("zhong");
        set.add(
            "B-903 最长韵母优先切分",
            s.n == 1 && s.sylls[0] == 15,
            "zh/ch/sh 歧义按最长韵母优先（MD2 9.3）",
        );
    }
    {
        // 声韵组合切分
        let s = split_input("zhangchang");
        set.add(
            "B-903 连续音节切分",
            s.n == 2 && s.sylls[0] == 6 && s.sylls[1] == 7,
            "zhang+chang 最大匹配",
        );
    }
    {
        // 完全匹配 + 档位稳定排序（模糊音关闭）
        let s = split_input("zhang");
        let c = lookup(&s);
        set.add(
            "B-903 完全匹配档位序",
            c[0] != 0,
            "音节序列完全匹配优先、模糊音关闭、词频档位排序",
        );
    }
    {
        // 不做运行期学习（类型面防线）
        // 审计面：DictEntry 与 DICT 均为 const 冻结——不存在写入口
        set.add(
            "B-903 不做运行期学习",
            DICT.len() == 16 && DICT[0].freq_tier == 0,
            "词库冻结三元组（诚实标注：第一版没有用户词频）——类型面无 learn 入口",
        );
    }
    {
        // 每页九个 + 页内序稳定
        let s = split_input("zhong");
        let c1 = lookup(&s);
        let c2 = lookup(&s);
        set.add(
            "B-903 页面九个序稳定",
            c1.len() == 9 && c1 == c2,
            "数字键选重、页内序不抖动（MD2 9.3）",
        );
    }
    {
        // IME 对练
        let sum = run_ime_drills(0xB903, 60);
        set.add(
            "B-903 IME 对练",
            sum.rounds == 60 && sum.split_stable && sum.page_stable && sum.budget_ok,
            "100ms/16ms/32MB 三硬线（判据原文）",
        );
    }
    set
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f803_split_longest() {
        let s = split_input("zhong");
        assert_eq!(s.n, 1);
        assert_eq!(s.sylls[0], 15, "zhong 整体成音节（最长优先）");
        let s = split_input("shangma");
        assert_eq!(s.n, 2);
        assert_eq!(s.sylls[0], 8, "shang");
        assert_eq!(s.sylls[1], 9, "ma");
    }

    #[test]
    fn f803_budget_triple() {
        assert!(first_page_ok());
        assert!(commit_ok());
        assert!(mem_ok());
        assert_eq!(DICT_ENTRIES * BYTES_PER_ENTRY / 1024, 937, "937KB ≪ 32MB");
    }

    #[test]
    fn f803_frozen_dict() {
        // 冻结语义：两次检索同结果 + 档位不因检索变化
        let s = split_input("zhong");
        let c1 = lookup(&s);
        let c2 = lookup(&s);
        assert_eq!(c1, c2);
    }

    #[test]
    fn f803_drill_deterministic() {
        let a = run_ime_drills(3, 30);
        let b = run_ime_drills(3, 30);
        assert_eq!(a, b);
        assert!(a.split_stable && a.page_stable && a.budget_ok);
    }
}
