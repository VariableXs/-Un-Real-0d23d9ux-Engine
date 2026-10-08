//! F184 热插拔体验 · 批次六深化（secstar · G-G-14）。
//!
//! 批次六功能面（与 b3/b4/b5 互补，本批管「键盘等价与风暴过滤」）：
//! - [`VolumeHotkey`]：卷快捷键——Ctrl+E 弹出当前聚焦卷（键盘用户和
//!   鼠标用户能力对等——第 4 章的卷面落地）；
//! - [`PlugStormFilter`]：插拔风暴过滤——1s 内 ≥5 次插拔 = 接触不良
//!   → 告警 + 停止弹 toast（风暴中刷屏是缺陷——通知面自我保护）；
//! - [`format_capacity`]：容量格式化——B/KB/MB/GB/TB 人类可读
//!   （插入 toast 的容量字段的格式化面）。
//!
//! 零堆纪律：状态位 + 定长缓冲，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 卷快捷键
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolumeHotkey {
    pub ctrl: bool,
    pub e: bool,
}

/// 快捷键命中：Ctrl+E 恰好组合（仅 E 无 Ctrl = 输入字符不误触发）。
pub fn hotkey_matches(h: VolumeHotkey) -> bool {
    h.ctrl && h.e
}

/// 聚焦卷弹出（返回目标盘符——无聚焦卷 = None 不误弹）。
pub fn eject_focused(focused_drive: Option<u8>) -> Option<u8> {
    focused_drive
}

// ---------------------------------------------------------------------------
// 插拔风暴过滤
// ---------------------------------------------------------------------------

/// 风暴线（1 秒内次数）。
pub const STORM_THRESHOLD: u32 = 5;

#[derive(Clone, Copy, Debug, Default)]
pub struct PlugStormFilter {
    pub events_this_second: u32,
    pub storms: u32,
    pub toast_suppressed: bool,
}

impl PlugStormFilter {
    pub fn on_plug_event(&mut self, same_second: bool) {
        if same_second {
            self.events_this_second += 1;
        } else {
            self.events_this_second = 1;
            self.toast_suppressed = false; // 新秒解除压制
        }
        if self.events_this_second >= STORM_THRESHOLD {
            if !self.toast_suppressed {
                self.storms += 1;
            }
            self.toast_suppressed = true; // 风暴中停止刷 toast
        }
    }

    /// 是否允许弹 toast（风暴中被压制）。
    pub fn toast_allowed(&self) -> bool {
        !self.toast_suppressed
    }
}

// ---------------------------------------------------------------------------
// 容量格式化
// ---------------------------------------------------------------------------

/// 容量 → 人类可读字节面（`1.5 GB` 风格；<1KB 显示 B）。
/// 返回写入长度。一位小数、单位大写。
pub fn format_capacity(bytes: u64, out: &mut [u8]) -> usize {
    let mut n = 0;
    let putc = |b: u8, out: &mut [u8], n: &mut usize| {
        if *n < out.len() {
            out[*n] = b;
            *n += 1;
        }
    };
    let putn = |v: u64, out: &mut [u8], n: &mut usize| {
        let mut d = [0u8; 20];
        let mut w = 0;
        if v == 0 {
            d[0] = b'0';
            w = 1;
        } else {
            let mut x = v;
            while x > 0 {
                d[w] = b'0' + (x % 10) as u8;
                w += 1;
                x /= 10;
            }
        }
        for i in (0..w).rev() {
            if *n < out.len() {
                out[*n] = d[i];
                *n += 1;
            }
        }
    };
    const KB: u64 = 1_024;
    const MB: u64 = KB * 1_024;
    const GB: u64 = MB * 1_024;
    const TB: u64 = GB * 1_024;
    let (unit, div) = if bytes >= TB {
        (b'T', TB)
    } else if bytes >= GB {
        (b'G', GB)
    } else if bytes >= MB {
        (b'M', MB)
    } else if bytes >= KB {
        (b'K', KB)
    } else {
        putn(bytes, out, &mut n);
        putc(b' ', out, &mut n);
        putc(b'B', out, &mut n);
        return n;
    };
    // 整数部分。
    let whole = bytes / div;
    // 一位小数（十分位）。
    let frac = (bytes % div) * 10 / div;
    putn(whole, out, &mut n);
    if frac > 0 {
        putc(b'.', out, &mut n);
        putc(b'0' + frac as u8, out, &mut n);
    }
    putc(b' ', out, &mut n);
    putc(unit, out, &mut n);
    putc(b'B', out, &mut n);
    n
}

// ---------------------------------------------------------------------------
// 批次六自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_hotplug_b6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F184-b6");

    // 1) 快捷键命中：Ctrl+E 真、仅 E 假、仅 Ctrl 假（组合逐点）。
    cs.add(
        "hotkey_combo",
        hotkey_matches(VolumeHotkey { ctrl: true, e: true })
            && !hotkey_matches(VolumeHotkey { ctrl: false, e: true })
            && !hotkey_matches(VolumeHotkey { ctrl: true, e: false }),
        "",
    );

    // 2) 聚焦弹出：有聚焦返回盘符、无聚焦 None（不误弹）。
    cs.add(
        "eject_focused",
        eject_focused(Some(b'E')) == Some(b'E') && eject_focused(None).is_none(),
        "",
    );

    // 3) 风暴触发：同秒 5 事件 → 风暴计数 + toast 压制（自我保护面）。
    let mut f = PlugStormFilter::default();
    for _ in 0..5 {
        f.on_plug_event(true);
    }
    cs.add("storm_triggers", f.storms == 1 && f.toast_suppressed && !f.toast_allowed(), "");

    // 4) 新秒解除压制：换秒后 toast 恢复（压制是暂时的）。
    f.on_plug_event(false);
    cs.add("storm_released", !f.toast_suppressed && f.toast_allowed(), "");

    // 5) 正常插拔不误伤：4 次/秒 < 线 → 不压制（不误伤正常使用）。
    let mut f2 = PlugStormFilter::default();
    for _ in 0..4 {
        f2.on_plug_event(true);
    }
    cs.add("storm_threshold_not_hit", f2.toast_allowed() && f2.storms == 0, "");

    // 6) 容量格式化：B/KB/MB/GB/TB 五档（一位小数）。
    let mut buf = [0u8; 16];
    let cases: [(u64, &str); 6] = [
        (512, "512 B"),
        (1_536, "1.5 KB"),
        (5 * 1_048_576, "5 MB"),
        (1_610_612_736, "1.5 GB"),
        (1_099_511_627_776, "1 TB"),
        (2_199_023_255_552, "2 TB"),
    ];
    let mut all = true;
    for (b, expect) in cases {
        let n = format_capacity(b, &mut buf);
        all &= &buf[..n] == expect.as_bytes();
    }
    cs.add("capacity_format", all, "");

    // 7) 零容量：0 B（空盘不显示 0.0 KB）。
    let n = format_capacity(0, &mut buf);
    cs.add("capacity_zero", &buf[..n] == b"0 B", "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次六）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b6 {
    use super::*;

    #[test]
    fn storm_repeated_cycles() {
        // 反复风暴：每次风暴计数、换秒恢复（周期行为闭环）。
        let mut f = PlugStormFilter::default();
        for _ in 0..3 {
            for _ in 0..5 {
                f.on_plug_event(true);
            }
            assert!(f.toast_suppressed);
            f.on_plug_event(false);
            assert!(!f.toast_suppressed);
        }
        assert_eq!(f.storms, 3);
    }

    #[test]
    fn capacity_exact_powers() {
        // 整数幂：1024B → "1 KB"（无小数尾巴）。
        let mut buf = [0u8; 16];
        let n = format_capacity(1_024, &mut buf);
        assert_eq!(&buf[..n], b"1 KB");
        let n = format_capacity(1_048_576, &mut buf);
        assert_eq!(&buf[..n], b"1 MB");
    }
}
