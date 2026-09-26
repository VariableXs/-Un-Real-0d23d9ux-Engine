//! F173 panic 画面设计 · 批次六深化（secstar · G-G-03）。
//!
//! 批次六功能面（与 b3/b4/b5 互补，本批管「画面自证与词条」）：
//! - [`PanicScreenshot`]：panic 画面自身入诊断（画面字节哈希进快照
//!   ——所见即所报：用户看到的画面就是诊断里的画面）；
//! - [`code_seq_alloc`]：错误码序号分配——同码 1s 内不重复发号
//!   （序号是事件身份：风暴中每秒至多一号，风暴闸联动）；
//! - [`advice_bilingual`]：建议双语词条——中/英两套（F140 本地化
//!   开放的语言面：词条化不是装饰是承诺）。
//!
//! 零堆纪律：定长哈希 + 定长词条，无 alloc。

use super::panicscreen::REBOOT_COUNTDOWN_MS;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// panic 画面快照
// ---------------------------------------------------------------------------

/// 画面哈希（FNV-1a 64——画面字节的指纹）。
pub fn screen_hash(pixels: &[u32]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for px in pixels {
        for b in px.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// 画面自证：同帧同指纹、异帧异指纹（诊断里的画面=用户看到的画面）。
pub fn screenshot_attests(frame_a: &[u32], frame_b: &[u32]) -> (bool, bool) {
    let ha = screen_hash(frame_a);
    let hb = screen_hash(frame_b);
    (ha == hb, ha != hb)
}

// ---------------------------------------------------------------------------
// 错误码序号分配（同码 1s 限一号）
// ---------------------------------------------------------------------------

pub const SEQ_RATE_WINDOW_MS: u64 = 1_000;

#[derive(Clone, Copy, Debug, Default)]
pub struct SeqAllocator {
    last_ms: u64,
    next_seq: u16,
}

impl SeqAllocator {
    pub const fn new() -> SeqAllocator {
        SeqAllocator { last_ms: 0, next_seq: 0 }
    }

    /// 发号：同码 1s 内不重复（返回 None——风暴中每秒至多一号，
    /// 与安全模式风暴闸联动：风暴本来就不该靠序号记录而是靠计数）。
    pub fn alloc(&mut self, now_ms: u64) -> Option<u16> {
        if self.next_seq > 0 && now_ms.saturating_sub(self.last_ms) < SEQ_RATE_WINDOW_MS {
            return None;
        }
        if self.next_seq >= 999 {
            return None; // 定宽 3 位上限（与 b4 编码器一致）
        }
        self.next_seq += 1;
        self.last_ms = now_ms;
        Some(self.next_seq)
    }

    pub fn seq(&self) -> u16 {
        self.next_seq
    }
}

// ---------------------------------------------------------------------------
// 建议双语词条
// ---------------------------------------------------------------------------

/// 语言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

/// 建议词条（中/英两套——同一建议两种话）。
pub fn advice_text(lang: Lang, module: &str) -> &'static str {
    match (lang, module) {
        (Lang::Zh, "GFX") => "已回退基础显示，进入系统后检查显示设置",
        (Lang::En, "GFX") => "Fallback display active; check display settings after boot",
        (Lang::Zh, "MEM") => "已转安全模式，建议运行内存诊断",
        (Lang::En, "MEM") => "Safe mode active; run memory diagnostics",
        (Lang::Zh, "STORE") => "文件系统已回滚，建议检查此卷",
        (Lang::En, "STORE") => "Filesystem rolled back; verify this volume",
        (Lang::Zh, _) => "已保存诊断快照，重启后如再现请反馈",
        (Lang::En, _) => "Snapshot saved; report if it recurs after reboot",
    }
}

/// 词条双语齐备：四键位两种语言全非空（缺译即红——B-1104 语言面）。
pub fn advice_bilingual_complete() -> bool {
    for m in ["GFX", "MEM", "STORE", "OTHER"] {
        if advice_text(Lang::Zh, m).is_empty() || advice_text(Lang::En, m).is_empty() {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 批次六自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_panicscreen_b6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F173-b6");

    // 1) 画面自证：同帧同指纹、异帧异指纹（两面）。
    let a = [0x11223344u32; 64];
    let b = [0x11223344u32; 64];
    let c = [0x11223345u32; 64];
    let (same, diff) = screenshot_attests(&a, &b);
    let (_, diff2) = screenshot_attests(&a, &c);
    cs.add("screen_hash_two_ways", same && !diff && diff2, "");

    // 2) 空画面指纹稳定：零帧指纹确定（不炸不随机）。
    let z1 = screen_hash(&[]);
    let z2 = screen_hash(&[]);
    cs.add("screen_hash_empty", z1 == z2, "");

    // 3) 发号：首号 1、1s 内再请求拒（风暴限速）。
    let mut s = SeqAllocator::new();
    let first = s.alloc(1_000);
    let quick = s.alloc(1_500);
    cs.add("seq_first_and_rate", first == Some(1) && quick.is_none() && s.seq() == 1, "");

    // 4) 窗满发号：1s 后再请求 → 2 号（窗口外正常发号）。
    let second = s.alloc(2_100);
    cs.add("seq_after_window", second == Some(2), "");

    // 5) 序号上限：999 后拒（定宽 3 位与 b4 编码器一致）。
    let mut s2 = SeqAllocator { last_ms: 0, next_seq: 999 };
    cs.add("seq_cap", s2.alloc(10_000).is_none(), "");

    // 6) 双语词条：四键位两语言全非空（缺译即红）。
    cs.add("advice_bilingual", advice_bilingual_complete(), "");

    // 7) 词条语义：中英 GFX 词条都含显示语义（不是空占位）。
    let zh = advice_text(Lang::Zh, "GFX");
    let en = advice_text(Lang::En, "GFX");
    cs.add("advice_semantic", zh.contains("显示") && en.contains("display"), "");

    // 8) 未知模块兜底词条：双语都齐（兜底不缺译）。
    cs.add(
        "advice_fallback_both",
        !advice_text(Lang::Zh, "WHATEVER").is_empty() && !advice_text(Lang::En, "WHATEVER").is_empty(),
        "",
    );

    // 9) 常量贯通：30s 倒计时一处一事实。
    cs.add("consts_aligned", REBOOT_COUNTDOWN_MS == 30_000, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次六）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b6 {
    use super::*;

    #[test]
    fn seq_rate_multiple_windows() {
        // 多窗口节奏：每窗口一号、序号单调递增（身份不重复）。
        let mut s = SeqAllocator::new();
        let mut seqs = Vec::new();
        for w in 0..5u64 {
            if let Some(n) = s.alloc(w * 1_500) {
                seqs.push(n);
            }
        }
        assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn screen_hash_sensitive_to_position() {
        // 位置敏感：同色不同位 → 异指纹（画面指纹不是多重集）。
        let a = [1u32, 2, 3];
        let b = [3u32, 2, 1];
        let (_, diff) = screenshot_attests(&a, &b);
        assert!(diff);
    }
}
