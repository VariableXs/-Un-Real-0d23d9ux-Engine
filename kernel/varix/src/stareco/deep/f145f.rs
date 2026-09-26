//! 深化层三 · F145 教育/作品集友好（2026-09-26 深化批次三）。
//!
//! 补深发布工程面（主册 G-D-20）：脱敏字节扫描器强化（路径/账号/
//! 密钥形态的偏移定位——不只判有无还给位置）、文章元数据校验
//! （主题/分级/ADR/引用四件）、可读性审计（长句/长段落检测）、
//! 配额台账（四主题×5 篇的逐题计数）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 脱敏字节扫描器强化：形态匹配 + 偏移定位
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Finding {
    /// 形态类别：0=Windows 路径盘符 / 1=Unix 用户目录 / 2=密钥形态。
    pub kind: u8,
    pub offset: usize,
}

/// 扫描正文：返回全部命中（偏移为字节位）。
/// 形态：`C:\`、`/Users/`、`sk-`（密钥前缀样本）。
pub fn sanitize_scan(text: &str) -> alloc::vec::Vec<Finding> {
    let b = text.as_bytes();
    let mut out: alloc::vec::Vec<Finding> = alloc::vec::Vec::new();
    for i in 0..b.len() {
        if b[i] == b'C' && i + 2 < b.len() && b[i + 1] == b':' && b[i + 2] == b'\\' {
            out.push(Finding { kind: 0, offset: i });
        }
        if i + 7 <= b.len() && &b[i..i + 7] == b"/Users/" {
            out.push(Finding { kind: 1, offset: i });
        }
        if i + 3 <= b.len() && &b[i..i + 3] == b"sk-" {
            out.push(Finding { kind: 2, offset: i });
        }
    }
    out
}

/// 出版裁决：零命中才可出版（整篇扣住的机器面判据）。
pub fn publishable(text: &str) -> Result<(), &'static str> {
    if sanitize_scan(text).is_empty() {
        Ok(())
    } else {
        Err("脱敏命中：正文含真实路径/账号/密钥形态，扣住不出版")
    }
}

// ---------------------------------------------------------------------------
// 文章元数据校验：主题(0-3) / 分级(0-2) / ADR 注解 / 引用非空
// ---------------------------------------------------------------------------

pub fn meta_check(theme: u8, level: u8, has_adr: bool, refs: &[&'static str]) -> Result<(), &'static str> {
    if theme > 3 {
        return Err("主题越界：四主题之外");
    }
    if level > 2 {
        return Err("分级越界：入门/进阶/深水区三级");
    }
    if !has_adr {
        return Err("缺 ADR 决策注解：技术选型必须可溯");
    }
    if refs.is_empty() {
        return Err("引用清单为空：出处必须可查");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 可读性审计：句长均值（≤120 字节）与长段落（>800 字节）检测
// ---------------------------------------------------------------------------

pub struct Readability {
    pub avg_sentence_bytes: u32,
    pub long_paragraphs: usize,
}

/// 句界：'.' '?' '!' 与中文句号'。'；段界：'\n'。
/// 无句界尾巴在文末/段末如实按单句计入（不丢字）。
pub fn readability(text: &str) -> Readability {
    let mut sentences = 0u32;
    let mut sentence_bytes_sum: u64 = 0;
    let mut cur_sentence: u64 = 0;
    let mut long_paragraphs = 0usize;
    let mut cur_para: u64 = 0;

    let flush_sentence = |sentences: &mut u32, sum: &mut u64, cur: &mut u64| {
        if *cur > 0 {
            *sentences += 1;
            *sum += *cur;
            *cur = 0;
        }
    };

    for ch in text.chars() {
        let n = ch.len_utf8() as u64;
        cur_para += n;
        match ch {
            '.' | '?' | '!' | '。' => {
                cur_sentence += n;
                sentences += 1;
                sentence_bytes_sum += cur_sentence;
                cur_sentence = 0;
            }
            '\n' => {
                flush_sentence(&mut sentences, &mut sentence_bytes_sum, &mut cur_sentence);
                if cur_para > 800 {
                    long_paragraphs += 1;
                }
                cur_para = 0;
            }
            _ => cur_sentence += n,
        }
    }
    flush_sentence(&mut sentences, &mut sentence_bytes_sum, &mut cur_sentence);
    if cur_para > 800 {
        long_paragraphs += 1;
    }
    let avg = if sentences == 0 {
        0
    } else {
        (sentence_bytes_sum / sentences as u64) as u32
    };
    Readability { avg_sentence_bytes: avg, long_paragraphs }
}

// ---------------------------------------------------------------------------
// 配额台账：四主题 × 5 篇 → 缺额清单
// ---------------------------------------------------------------------------

/// counts: 每主题已发篇数（序号=主题）。
pub fn quota_gaps(counts: &[u32; 4]) -> alloc::vec::Vec<u8> {
    let mut gaps: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    for (t, c) in counts.iter().enumerate() {
        if *c < 5 {
            gaps.push(t as u8);
        }
    }
    gaps
}

pub fn quota_total(counts: &[u32; 4]) -> u32 {
    counts.iter().sum()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F145F_TAG: &str = "stareco-F145-deep3";

pub fn run_f145_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new(F145F_TAG);

    // 脱敏扫描
    let f = sanitize_scan("see C:\\Users\\x and sk-abc123 on /Users/y");
    set.add("f145f scan drive", f.contains(&Finding { kind: 0, offset: 4 }), "盘符形态偏移 4");
    set.add("f145f scan key", f.iter().any(|x| x.kind == 2 && x.offset == 19), "密钥形态偏移 19");
    set.add("f145f scan users", f.iter().filter(|x| x.kind == 1).count() == 1, "Unix 目录一处（Windows 形态不算）");
    set.add("f145f scan clean", sanitize_scan("clean text only").is_empty(), "干净正文零命中");
    set.add("f145f publish block", publishable("path C:\\temp").is_err(), "命中扣住不出版");
    set.add("f145f publish ok", publishable("clean").is_ok(), "干净放行");

    // 元数据
    set.add(
        "f145f meta ok",
        meta_check(1, 0, true, &["ADR-0091"]).is_ok(),
        "四件齐通过",
    );
    set.add("f145f meta theme", meta_check(4, 0, true, &["r"]).is_err(), "主题越界拒");
    set.add("f145f meta level", meta_check(0, 3, true, &["r"]).is_err(), "分级越界拒");
    set.add("f145f meta adr", meta_check(0, 0, false, &["r"]).is_err(), "缺 ADR 拒");
    set.add("f145f meta refs", meta_check(0, 0, true, &[]).is_err(), "缺引用拒");

    // 可读性
    let good = readability("First sentence is short. Second one too! Ok?");
    set.add("f145f readable avg", good.avg_sentence_bytes <= 30, "短句均值");
    set.add("f145f readable para", good.long_paragraphs == 0, "零长段");
    let mut long_para = alloc::string::String::new();
    for _ in 0..200 {
        long_para.push_str("word ");
    }
    let rp = readability(&long_para);
    set.add("f145f long para", rp.long_paragraphs == 1, "超 800 字节段落检出");
    set.add("f145f no sentence", readability("nosentencehere").avg_sentence_bytes == 14, "无句界按单句计");

    // 配额
    let q = [5u32, 3, 5, 0];
    set.add("f145f quota gaps", quota_gaps(&q) == alloc::vec![1, 3], "缺额主题清单");
    set.add("f145f quota full", quota_gaps(&[5, 5, 5, 5]).is_empty(), "满额零缺");
    set.add("f145f quota total", quota_total(&q) == 13, "合计");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn scan_offsets_precise() {
        let t = "ask-sk-x";
        // "sk-" 首现于 offset 1（a 后的 sk-），第二处于 4。
        let f = sanitize_scan(t);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].offset, 1);
        assert_eq!(f[1].offset, 4);
        // 重叠不重复：同一位置只报一次。
        let t2 = "sk-sk-";
        let f2 = sanitize_scan(t2);
        assert_eq!(f2.len(), 2);
        assert_eq!(f2[0].offset, 0);
        assert_eq!(f2[1].offset, 3);
    }

    #[test]
    fn cn_sentence_end() {
        let t = "第一句。第二句。";
        let r = readability(t);
        assert_eq!(r.avg_sentence_bytes, 12); // 每句 9 字节正文+3 字节句号
        assert!(r.long_paragraphs == 0);
    }
}
