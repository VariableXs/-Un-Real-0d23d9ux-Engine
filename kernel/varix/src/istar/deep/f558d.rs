//! 深化层 · F558 键盘测试工具（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F558 节）：
//! ①「按下即亮松开即灭」的**时序账**——每键按下/松开的 ms 对，按住
//!   时长统计（点亮时序是可查的账，不是一闪而过的视觉）；
//! ②「连击/幽灵键检测（三键无冲测试区）」的**明细深化**——幽灵事件
//!   不再只是计数：记下第几键被判定幽灵（送修时能说清是哪枚键）；
//! ③「测试结果可导出（送修/换机前的凭证）」的**凭证结构**——键位
//!   计数摘要 / 无冲结论 / 幽灵明细三段式，逐行确定文本（换机凭证
//!   要能打印能归档）。

use alloc::string::String;
use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::keybtest::{KeybTest, KeyId, KEYS, ROLLOVER_KEYS};

// ---------------------------------------------------------------------------
// 时序账
// ---------------------------------------------------------------------------

/// 一条按键时序（按下→松开）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HoldSpan {
    pub key: KeyId,
    pub down_ms: u64,
    pub up_ms: u64,
}

impl HoldSpan {
    pub fn hold_ms(&self) -> u64 {
        self.up_ms.saturating_sub(self.down_ms)
    }
}

/// 时序账（环，容量 64）。
pub struct TimingLedger {
    buf: [Option<HoldSpan>; 64],
    head: usize,
    len: usize,
}

impl TimingLedger {
    pub fn new() -> TimingLedger {
        TimingLedger { buf: [None; 64], head: 0, len: 0 }
    }

    pub fn record(&mut self, key: KeyId, down_ms: u64, up_ms: u64) {
        self.buf[self.head] = Some(HoldSpan { key, down_ms, up_ms });
        self.head = (self.head + 1) % 64;
        if self.len < 64 {
            self.len += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// 某键的平均按住时长（点亮节奏取证）。
    pub fn avg_hold_ms(&self, key: KeyId) -> Option<u64> {
        let mut sum = 0u64;
        let mut n = 0u64;
        for i in 0..self.len {
            if let Some(s) = self.buf[i] {
                if s.key == key {
                    sum += s.hold_ms();
                    n += 1;
                }
            }
        }
        if n == 0 {
            None
        } else {
            Some(sum / n)
        }
    }

    /// 时序一致性：所有记录 up >= down（倒挂 = 记账错，立红）。
    pub fn monotonic(&self) -> bool {
        (0..self.len).all(|i| self.buf[i].map(|s| s.up_ms >= s.down_ms).unwrap_or(true))
    }
}

impl Default for TimingLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 幽灵键明细
// ---------------------------------------------------------------------------

/// 一条幽灵判定（三键同按后第 N 键的判定记录）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GhostDetail {
    /// 被判幽灵的键。
    pub key: KeyId,
    /// 判定时的同按规模。
    pub combo_size: usize,
}

/// 幽灵明细环（容量 16）。
pub struct GhostLog {
    buf: [Option<GhostDetail>; 16],
    head: usize,
    len: usize,
}

impl GhostLog {
    pub fn new() -> GhostLog {
        GhostLog { buf: [None; 16], head: 0, len: 0 }
    }

    pub fn record(&mut self, key: KeyId, combo_size: usize) {
        self.buf[self.head] = Some(GhostDetail { key, combo_size });
        self.head = (self.head + 1) % 16;
        if self.len < 16 {
            self.len += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn get(&self, i: usize) -> Option<GhostDetail> {
        if i < self.len {
            self.buf[i]
        } else {
            None
        }
    }

    /// 某键是否反复被判幽灵（同一键 ≥2 次 = 该键有问题的证据）。
    pub fn repeat_offender(&self, key: KeyId) -> bool {
        (0..self.len).filter(|&i| self.get(i).map(|g| g.key) == Some(key)).count() >= 2
    }
}

impl Default for GhostLog {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 送修凭证（三段式结构化导出）
// ---------------------------------------------------------------------------

/// 凭证（三段：计数摘要 / 无冲结论 / 幽灵明细）。
pub struct Voucher {
    pub lines: alloc::vec::Vec<String>,
}

impl Voucher {
    /// 从测试台 + 深化账组装凭证（逐行确定文本——打印与归档同源）。
    pub fn build(t: &KeybTest, timing: &TimingLedger, ghosts: &GhostLog) -> Voucher {
        let mut lines = alloc::vec::Vec::new();
        lines.push(String::from("== 键盘测试凭证 =="));
        let pressed: u32 = (0..KEYS as u16).map(|k| t.count(k)).sum();
        lines.push(alloc::format!("按键总数: {}", pressed));
        let g = t.ghost_count();
        lines.push(alloc::format!("无冲结论: {}", if g == 0 { "通过（无幽灵键）" } else { "存在幽灵键" }));
        if g > 0 {
            for i in 0..ghosts.len() {
                if let Some(d) = ghosts.get(i) {
                    lines.push(alloc::format!("幽灵: 键{} 同按{}", d.key, d.combo_size));
                }
            }
        }
        lines.push(alloc::format!("时序账: {} 条, 倒挂 {}", timing.len(), if timing.monotonic() { 0 } else { 1 }));
        Voucher { lines }
    }

    /// 凭证合同：三段齐（标题/无冲结论/时序账）才算一份完整凭证。
    pub fn complete(&self) -> bool {
        self.lines.len() >= 3
            && self.lines[0].starts_with("==")
            && self.lines.iter().any(|l| l.contains("无冲结论"))
            && self.lines.iter().any(|l| l.contains("时序账"))
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f558_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 时序账：按下/松开成对，按住时长可统计，无倒挂。
    let mut tl = TimingLedger::new();
    tl.record(17, 1_000, 1_090);
    tl.record(17, 2_000, 2_050);
    tl.record(30, 2_100, 2_260);
    cs.add(
        "timing ledger stats",
        tl.len() == 3 && tl.avg_hold_ms(17) == Some(70) && tl.avg_hold_ms(30) == Some(160)
            && tl.monotonic(),
        "",
    );

    // 2) 幽灵明细：记键号与同按规模；同键反复被判 =  offender 证据。
    let mut gl = GhostLog::new();
    gl.record(41, 4);
    gl.record(41, 4);
    gl.record(55, 5);
    cs.add(
        "ghost detail with repeat offender",
        gl.len() == 3 && gl.repeat_offender(41) && !gl.repeat_offender(55),
        "",
    );

    // 3) 与基础台联动：三键无冲区第 4 键同按 → 基础 ghost_events 增量
    //    与深化明细一致（两本账同源对拍）。
    let mut t = KeybTest::new();
    let mut gl2 = GhostLog::new();
    for k in [10u16, 11, 12, 13] {
        let before = t.ghost_count();
        let _ = t.press(k);
        if t.ghost_count() > before {
            gl2.record(k, ROLLOVER_KEYS + 1); // 同按规模 = 4
        }
    }
    cs.add(
        "ghost ledger mirrors base counter",
        t.ghost_count() == 1 && gl2.len() == 1 && gl2.get(0).unwrap().key == 13,
        "",
    );

    // 4) 松开复位：第 4 键释放后无冲区恢复（检测不吞输入的回归面）。
    let _ = t.release(13);
    cs.add("release restores rollover", t.lit(13) == false, "");

    // 5) 凭证三段齐：干净台出「通过」结论，幽灵台带明细行。
    let clean = Voucher::build(&KeybTest::new(), &TimingLedger::new(), &GhostLog::new());
    let dirty = Voucher::build(&t, &tl, &gl2);
    cs.add(
        "voucher three sections",
        clean.complete() && dirty.complete() && dirty.lines.iter().any(|l| l.contains("幽灵")),
        "",
    );

    // 6) 即开即用合同复核（深化层不破坏基础判据）。
    let mut t2 = KeybTest::new();
    t2.note_open_ms(900);
    cs.add("open budget kept", t2.opens_in_budget(), "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_ring_rolls() {
        let mut tl = TimingLedger::new();
        for i in 0..70u64 {
            tl.record((i % 104) as KeyId, i * 10, i * 10 + 5);
        }
        assert_eq!(tl.len(), 64);
        assert!(tl.monotonic());
    }

    #[test]
    fn avg_hold_none_for_unpressed() {
        let tl = TimingLedger::new();
        assert_eq!(tl.avg_hold_ms(3), None);
    }
}
