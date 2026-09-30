//! F185 只读卷保护提示 · 批次六深化（secstar · G-G-15）。
//!
//! 批次六功能面（与 b3/b4/b5 互补，本批管「限速与优先级」）：
//! - [`CopyThrottle`]：拷贝限速——前台交互优先（拷贝吃满 IO 会让
//!   界面卡顿——限速是交互保障不是功能阉割）；
//! - [`QueuePriority`]：队列优先级——用户手动任务插队自动任务
//!   （用户动作永远比后台动作急——优先级语义在册）；
//! - [`drop_to_ro_hint`]：只读卷拖放联动提示——拖到只读卷的文件名
//!   → 提示语含该文件名（提示具体的文件，不说笼统的卷）。
//!
//! 零堆纪律：状态字段 + 定长缓冲，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 拷贝限速
// ---------------------------------------------------------------------------

/// 限速档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Throttle {
    /// 全速（无交互负载时）。
    Full,
    /// 限速 50%（有前台窗口活跃）。
    Half,
    /// 限速 20%（前台正在打字/拖拽——最高优先让路）。
    Gentle,
}

/// 档位决策：交互负载 → 限速档（交互优先的机械判定）。
pub fn throttle_for(interacting: bool, typing: bool) -> Throttle {
    if typing {
        Throttle::Gentle
    } else if interacting {
        Throttle::Half
    } else {
        Throttle::Full
    }
}

/// 限速后的速率 ‰（全速 1000 / 半速 500 / 温和 200）。
pub fn throttle_permille(t: Throttle) -> u32 {
    match t {
        Throttle::Full => 1_000,
        Throttle::Half => 500,
        Throttle::Gentle => 200,
    }
}

// ---------------------------------------------------------------------------
// 队列优先级
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// 自动任务（系统建议的批量拷贝）。
    Auto = 0,
    /// 用户手动任务（点了「拷到 VARIX 区」）。
    Manual = 1,
}

/// 插队判定：Manual 永远排在 Auto 前（同优先级 FIFO）。
/// 返回插入位置。
pub fn insert_position(queue_kinds: &[Priority], incoming: Priority) -> usize {
    match incoming {
        Priority::Manual => {
            // Manual 插到最后一个 Manual 之后（同优先级 FIFO 不越队）。
            let mut pos = 0;
            for (i, k) in queue_kinds.iter().enumerate() {
                if *k == Priority::Manual {
                    pos = i + 1;
                }
            }
            pos
        }
        Priority::Auto => queue_kinds.len(), // Auto 排队尾
    }
}

// ---------------------------------------------------------------------------
// 只读卷拖放联动提示
// ---------------------------------------------------------------------------

/// 提示语构造：含具体文件名（定长缓冲——名字被截断则加省略号语义）。
pub fn drop_to_ro_hint(file_name: &[u8], out: &mut [u8]) -> usize {
    let prefix = b"\xe6\x97\xa0\xe6\xb3\x95\xe5\x86\x99\xe5\x85\xa5\xef\xbc\x9a"; // “无法写入：”
    let suffix = b" \xef\xbc\x88\xe5\x8d\xb7\xe4\xb8\xba\xe5\x8f\xaa\xe8\xaf\xbb\xef\xbc\x89"; // " （卷为只读）"
    let mut n = 0;
    let put = |bytes: &[u8], out: &mut [u8], n: &mut usize| {
        for b in bytes {
            if *n < out.len() {
                out[*n] = *b;
                *n += 1;
            }
        }
    };
    put(prefix, out, &mut n);
    // 名字超预算 → 留出后缀空间截断。
    let budget = out.len().saturating_sub(n + suffix.len());
    let take = file_name.len().min(budget);
    put(&file_name[..take], out, &mut n);
    put(suffix, out, &mut n);
    n
}

// ---------------------------------------------------------------------------
// 批次六自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_romount_b6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F185-b6");

    // 1) 限速三档：闲全速 / 交互半速 / 打字温和（交互优先逐级让路）。
    cs.add(
        "throttle_three",
        throttle_for(false, false) == Throttle::Full
            && throttle_for(true, false) == Throttle::Half
            && throttle_for(true, true) == Throttle::Gentle,
        "",
    );

    // 2) 限速率：1000/500/200（速率表在册）。
    cs.add(
        "throttle_rates",
        throttle_permille(Throttle::Full) == 1_000
            && throttle_permille(Throttle::Half) == 500
            && throttle_permille(Throttle::Gentle) == 200,
        "",
    );

    // 3) Manual 插队：Auto 队中插 Manual → 排到 Auto 前（用户优先）。
    let q = [Priority::Auto, Priority::Auto];
    cs.add("priority_manual_first", insert_position(&q, Priority::Manual) == 0, "");

    // 4) 同优先级 FIFO：已有 Manual 再来 Manual → 排其后（不越队）。
    let q2 = [Priority::Manual, Priority::Auto];
    cs.add("priority_fifo_within_class", insert_position(&q2, Priority::Manual) == 1, "");

    // 5) Auto 排队尾：Auto 永远让 Manual 先走。
    let q3 = [Priority::Manual];
    cs.add("priority_auto_tail", insert_position(&q3, Priority::Auto) == 1, "");

    // 6) 联动提示含文件名：report.docx → 提示语内嵌该名（具体不说笼统）。
    let mut buf = [0u8; 96];
    let n = drop_to_ro_hint(b"report.docx", &mut buf);
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    cs.add("hint_has_filename", text.contains("report.docx") && text.starts_with("无法写入："), "");

    // 7) 长文件名截断：提示语总量不越 96B 缓冲（截断但完整后缀）。
    let long_name = [b'x'; 200];
    let n2 = drop_to_ro_hint(&long_name, &mut buf);
    let text2 = core::str::from_utf8(&buf[..n2]).unwrap_or("");
    cs.add(
        "hint_truncated_safe",
        n2 <= 96 && text2.starts_with("无法写入：") && text2.ends_with("（卷为只读）"),
        "",
    );

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次六）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b6 {
    use super::*;

    #[test]
    fn priority_multi_manual_queue() {
        // 多 Manual 排队：后来 Manual 排在已有 Manual 之后（FIFO 不越）。
        let q = [Priority::Manual, Priority::Auto, Priority::Manual];
        assert_eq!(insert_position(&q, Priority::Manual), 3);
        assert_eq!(insert_position(&q, Priority::Auto), 3);
    }

    #[test]
    fn hint_short_name_full() {
        // 短名完整保留（不截断不丢字）。
        let mut buf = [0u8; 96];
        let n = drop_to_ro_hint(b"a.txt", &mut buf);
        let t = core::str::from_utf8(&buf[..n]).unwrap();
        assert!(t.contains("a.txt"));
    }
}
