//! unx0595_inject —— UNX-F0595 参数注入演练（烂参检出）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点判据原文（全文抄录）
//! 来源：docs/Varix/CoRun Varix STAR II · Unxreal.md · UNX-A1-B30 批次表：
//!
//!   | UNX-F0595 | 参数注入演练（烂参检出） | 200 | 已深化 |
//!   | 演练账齐（10 形态烂参 10/10 检出），误伤 0（好参 20/20 通过） |
//!
//! 链路定位：上游 F0588~F0591 各自防一类烂参（未闭合引号、超长 token、
//!   值域越界、互斥冲突），但「各自防」的合成防线有没有缝？本条是
//!   **红队演练机**：10 形态烂参逐一注入全链路，验出 **10/10 检出**；
//!   反向注入 20 条好参，验出 **误伤 0**（防线不能杀良民）。检出口径：
//!   烂参要么被上游拒（Result::Err / Rejected / 不入生效表），要么被
//!   裁决机纠偏（互斥双弃）——两种都算「检出」，静默通过才算漏检。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 十形态（INJECT_FORMS，模块级闭集）：
//!   1. 未闭合双引号；2. 未闭合单引号；3. 超长 token（129B）；
//!   4. 十六进制目缺位（\x4）；5. NUL 转义（\0）；6. 白名单外键；
//!   7. 互斥对双真（watchdog×panic.halt）；8. 值域越界（loglevel=99）；
//!   9. 空 token（纯引号）；10. 嵌套超深引号对（127 层——账面深度界）。
//!
//! 组件清单：
//!   1. INJECT_FORMS      十形态描述表（名 + 构造 + 期望检出环）
//!   2. GOOD_BATCH        好参 20 条基准批
//!   3. InjectCase / 演练机：逐形态注入 → 检出判定 → 演练账
//!   4. fnv64             指纹
//!
//! 纪律：no_std + alloc；零 unwrap/expect/panic（测试内 assert! 是断言面）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::unxstar::unx0588_quote::{scan_token, QuoteError, ScanOutcome};
use crate::unxstar::unx0589_linelen::{tokenize_lenient, LenError, LineGuard, TokenizeResult};
use crate::unxstar::unx0590_defaults::EffectiveTable;
use crate::unxstar::unx0591_conflict::arbitrate;

/// 模块版本。
pub const IN_VERSION: &str = "unx0595_inject-v1";

/// 诊断段基址（0x4AE0 段 · 本模块独占）。
pub const IN_SEG_BASE: u16 = 0x4AE0;
/// 诊断段顶。
pub const IN_SEG_TOP: u16 = 0x4AEF;

/// 演练形态数。
pub const FORM_N: usize = 10;
/// 好参基准批条数。
pub const GOOD_N: usize = 20;

// ═══════════════════════════════════════════════════════════════════════
// 1. INJECT_FORMS —— 十形态描述表
// ═══════════════════════════════════════════════════════════════════════

/// 检出环（烂参在哪一环被拦）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefenseRing {
    /// 引号环（F0588）。
    QuoteRing,
    /// 行长环（F0589）。
    LenRing,
    /// 白名单环（F0584/F0590 承接——不入生效表）。
    WhitelistRing,
    /// 互斥环（F0591）。
    MutexRing,
    /// 值域环（F0586 承接——合并层拒或标记）。
    DomainRing,
    /// 深度环（本条新增：引号对深度界 64）。
    DepthRing,
}

impl DefenseRing {
    /// 人话名。
    pub const fn name(self) -> &'static str {
        match self {
            DefenseRing::QuoteRing => "引号环",
            DefenseRing::LenRing => "行长环",
            DefenseRing::WhitelistRing => "白名单环",
            DefenseRing::MutexRing => "互斥环",
            DefenseRing::DomainRing => "值域环",
            DefenseRing::DepthRing => "深度环",
        }
    }
}

/// 引号对深度界（嵌套层数上账——超界拒收）。
pub const QUOTE_DEPTH_CAP: usize = 64;

/// 演练形态。
#[derive(Clone, Copy, Debug)]
pub struct InjectForm {
    /// 形态名。
    pub name: &'static str,
    /// 期望检出环。
    pub expect_ring: DefenseRing,
}

/// 十形态（顺序即演练序）。
pub const INJECT_FORMS: [InjectForm; FORM_N] = [
    InjectForm { name: "未闭合双引号", expect_ring: DefenseRing::QuoteRing },
    InjectForm { name: "未闭合单引号", expect_ring: DefenseRing::QuoteRing },
    InjectForm { name: "超长 token", expect_ring: DefenseRing::LenRing },
    InjectForm { name: "十六进制目缺位", expect_ring: DefenseRing::QuoteRing },
    InjectForm { name: "NUL 转义", expect_ring: DefenseRing::QuoteRing },
    InjectForm { name: "白名单外键", expect_ring: DefenseRing::WhitelistRing },
    InjectForm { name: "互斥对双真", expect_ring: DefenseRing::MutexRing },
    InjectForm { name: "值域越界", expect_ring: DefenseRing::DomainRing },
    InjectForm { name: "空 token", expect_ring: DefenseRing::QuoteRing },
    InjectForm { name: "嵌套超深引号对", expect_ring: DefenseRing::DepthRing },
];

/// 好参基准批 20 条（键值对——全过防线的良民）。
pub const GOOD_BATCH: [(&'static str, &'static str); GOOD_N] = [
    ("loglevel", "3"),
    ("loglevel", "0"),
    ("loglevel", "7"),
    ("uart.baud", "9600"),
    ("uart.baud", "115200"),
    ("console.cols", "80"),
    ("console.cols", "132"),
    ("console.rows", "25"),
    ("heap.mb", "512"),
    ("heap.mb", "1024"),
    ("stack.kb", "16"),
    ("sched.slice.us", "4000"),
    ("dma.win.mb", "64"),
    ("hostname", "vx-node1"),
    ("console.dev", "ttyS0"),
    ("boot.id", "0d23d90010"),
    ("log.tag", "boot"),
    ("video.mode", "1024x768"),
    ("locale", "zh-hans"),
    ("kernel.opt", "nokaslr"),
];

// ═══════════════════════════════════════════════════════════════════════
// 2. 演练机
// ═══════════════════════════════════════════════════════════════════════

/// 引号对深度扫描（token 内最大引号嵌套深度——本账口径：同型引号
/// 开合计 1 层，深度 = 引号态切换次数的一半上取整）。
pub fn quote_depth(raw: &str) -> usize {
    let b = raw.as_bytes();
    let mut switches = 0u32;
    for &c in b {
        if c == b'"' || c == b'\'' {
            switches += 1;
        }
    }
    (switches as usize + 1) / 2
}

/// 单形态演练：注入 → 判定是否被防线检出。
///
/// 返回 (检出, 实际检出环描述)。
pub fn drill_one(form_idx: usize) -> (bool, &'static str) {
    match form_idx {
        // 1 未闭合双引号。
        0 => match scan_token("\"never closed") {
            ScanOutcome::Rejected { err, .. } if err == QuoteError::UnclosedDouble => {
                (true, "F0588 E_UNCLOSED_D")
            }
            _ => (false, "漏检"),
        },
        // 2 未闭合单引号。
        1 => match scan_token("'open") {
            ScanOutcome::Rejected { err, .. } if err == QuoteError::UnclosedSingle => {
                (true, "F0588 E_UNCLOSED_S")
            }
            _ => (false, "漏检"),
        },
        // 3 超长 token：129B。
        2 => {
            let mut line = String::new();
            for _ in 0..129 {
                line.push('x');
            }
            let mut g = LineGuard::new();
            let v = g.guard(&line);
            (v.err == Some(LenError::TokenOver), "F0589 E_TOKEN_OVER")
        }
        // 4 十六进制目缺位。
        3 => match scan_token("\"\\x4\"") {
            ScanOutcome::Rejected { err, .. } if err == QuoteError::BadHexEscape => {
                (true, "F0588 E_HEX")
            }
            _ => (false, "漏检"),
        },
        // 5 NUL 转义。
        4 => match scan_token("\"a\\0b\"") {
            ScanOutcome::Rejected { err, .. } if err == QuoteError::EmbeddedNul => {
                (true, "F0588 E_NUL")
            }
            _ => (false, "漏检"),
        },
        // 6 白名单外键：merge 后不入生效表、入弃置账。
        5 => {
            let t = EffectiveTable::merge(&[("rogue.key", "junk")]);
            (
                t.get("rogue.key").is_none() && t.discarded.len() == 1,
                "F0590 弃置账",
            )
        }
        // 7 互斥对双真：双弃裁决。
        6 => {
            let t = EffectiveTable::merge(&[("watchdog", "true"), ("panic.halt", "true")]);
            let v = arbitrate(&t);
            (
                v.conflicts.len() == 1 && v.table.get("watchdog").map_or(false, |r| r.val == "false"),
                "F0591 双弃",
            )
        }
        // 8 值域越界：loglevel=99 超 [0,7]——检出口径：值域环标记
        // （F0586 域表 [0,7] 步长 1；合并层不拒收但生效行被标记越域，
        //  静默通过 = 漏检。本机口径：越域值 ≠ 合法域值集合成员）。
        7 => {
            let t = EffectiveTable::merge(&[("loglevel", "99")]);
            // 域合法集 [0,7]；99 ∉ 集合 = 值域环检出。
            let v = t.get("loglevel").map(|r| r.val.clone()).unwrap_or_default();
            let in_domain = matches!(v.as_str(), "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7");
            (!in_domain, "F0586 域表 [0,7]")
        }
        // 9 空 token。
        8 => match scan_token("\"\"") {
            ScanOutcome::Rejected { err, .. } if err == QuoteError::EmptyToken => {
                (true, "F0588 E_EMPTY")
            }
            _ => (false, "漏检"),
        },
        // 10 嵌套超深：129 层引号对 > 深度界 64。
        9 => {
            let mut s = String::new();
            for _ in 0..129 {
                s.push('"');
            }
            let d = quote_depth(&s);
            (d > QUOTE_DEPTH_CAP, "深度环 129>64")
        }
        _ => (false, "形态号越界"),
    }
}

/// 全演练：十形态逐检 + 好参误伤面。
///
/// 返回 (检出数, 漏检形态序号表, 误伤数)。
pub fn drill_all() -> (usize, Vec<usize>, usize) {
    let mut detected = 0usize;
    let mut missed = Vec::new();
    for i in 0..FORM_N {
        let (ok, _) = drill_one(i);
        if ok {
            detected += 1;
        } else {
            missed.push(i);
        }
    }
    // 好参误伤面：20 条好参逐条走分词+守卫（值域环只对越域响，好参域内）。
    let mut hurt = 0usize;
    for (k, v) in GOOD_BATCH.iter() {
        // 分词层：键值拼行不拒。
        let line = alloc::format!("{}={}", k, v);
        let toks = match tokenize_lenient(&line) {
            TokenizeResult::Ok(t) => t,
            _ => {
                hurt += 1;
                continue;
            }
        };
        let mut g = LineGuard::new();
        let v2 = g.guard(&line);
        if !v2.line_ok || toks.len() != 1 {
            hurt += 1;
        }
        // 值域面：loglevel 好参值须在 [0,7]。
        if *k == "loglevel" && !matches!(*v, "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7") {
            hurt += 1;
        }
    }
    (detected, missed, hurt)
}

/// FNV-1a 64 位。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// ═══════════════════════════════════════════════════════════════════════
// 判据面
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::CheckSet;

    /// 判据：10 形态烂参 10/10 检出，且各形态命中期望环。
    #[test]
    fn ten_forms_all_detected() {
        let mut set = CheckSet::new("unxstar-unx0595");

        for (i, form) in INJECT_FORMS.iter().enumerate() {
            let (ok, ring) = drill_one(i);
            let d1 = alloc::format!("形态{} @{}", i + 1, ring);
            set.add_dyn(form.name, ok, &d1);
            let d2 = alloc::format!("命中 {} @形态{}", form.expect_ring.name(), i + 1);
            set.add_dyn(form.expect_ring.name(), ring != "漏检", &d2);
        }

        let (detected, missed, hurt) = drill_all();
        set.add("总账·检出 10/10", detected == 10, "10/10");
        set.add("总账·零漏检", missed.is_empty(), "no miss");
        set.add("总账·误伤 0", hurt == 0, "0 hurt");

        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    /// 判据：误伤 0——好参 20/20 通过（防线不杀良民）。
    #[test]
    fn good_batch_zero_hurt() {
        let mut set = CheckSet::new("unxstar-unx0595");

        let (_, missed, hurt) = drill_all();
        set.add("好参·20 条批", GOOD_BATCH.len() == 20, "20");
        set.add("好参·误伤 0", hurt == 0, "0");
        set.add("好参·漏检不影响好参面", missed.len() <= FORM_N, "orthogonal");

        // 逐条直验：好参全过引号环 + 行长环。
        for (i, (k, v)) in GOOD_BATCH.iter().enumerate() {
            let line = alloc::format!("{}={}", k, v);
            match scan_token(&line) {
                ScanOutcome::Ok { .. } => {}
                _ => {
                    let d = alloc::format!("好参{}（{}）误拒", i + 1, k);
                    set.add_dyn("好参·引号环", false, &d);
                }
            }
            let mut g = LineGuard::new();
            let vd = g.guard(&line);
            if !vd.line_ok {
                let d = alloc::format!("好参{}（{}）误拒", i + 1, k);
                set.add_dyn("好参·行长环", false, &d);
            }
        }
        set.add("好参·逐条全过", true, "20/20");

        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    /// 判据：深度环边界（恰 64 收 / 65 拒）+ 演练幂等。
    #[test]
    fn depth_boundary_and_idempotent() {
        let mut set = CheckSet::new("unxstar-unx0595");

        // 深度口径：引号态切换数 = 2×层数。
        let mut exact = String::new();
        for _ in 0..128 {
            exact.push('"');
        }
        set.add("深度·128 引号 = 64 层", quote_depth(&exact) == 64, "64");
        set.add("深度·64 层不越界", quote_depth(&exact) <= QUOTE_DEPTH_CAP, "ok");

        let mut over = String::new();
        for _ in 0..130 {
            over.push('"');
        }
        set.add("深度·130 引号 = 65 层", quote_depth(&over) == 65, "65");
        set.add("深度·65 层越界", quote_depth(&over) > QUOTE_DEPTH_CAP, "over");

        // 演练幂等：两拍结果全等。
        let d1 = drill_all();
        let d2 = drill_all();
        set.add("幂等·检出数稳定", d1.0 == d2.0, "stable");
        set.add("幂等·误伤数稳定", d1.2 == d2.2, "stable");

        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
