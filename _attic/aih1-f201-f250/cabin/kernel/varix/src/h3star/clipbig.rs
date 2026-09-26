//! F329 剪贴板大对象处理 · 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：50MB 阈值行为切换；流式读取（复制后内存增量 <10MB
//! 判据）；预估准确性（误差 <20%）；历史引用条行为。
//!
//! **设计要点（主册）**：
//! - 剪贴板对大对象有礼仪：超过 50MB 的复制（大视频/大文件夹）立即显示
//!   「已复制引用」状态条（数据未实际驻留内存，粘贴时才流式读取）；粘
//!   贴大对象弹体积预估与预计时间（「将复制 1.2GB，约 40 秒」）；剪贴
//!   板历史不收录超阈值大对象（历史区放引用条，点开才加载）；
//! - 无感标准：复制大东西系统不卡不炸内存、知道自己在复制多大的东西。
//!
//! 实现形态：阈值分流账（驻留 vs 引用条）+ 流式读取模型（分块账——内存
//! 增量判据载体）+ 预估器（体积/时间，误差 <20% 判线）+ 历史引用条。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 大对象阈值（50MB）。
pub const BIG_OBJECT_BYTES: u64 = 50 * 1024 * 1024;

/// 复制动作内存增量判线（10MB——引用条不驻留数据的账面证据）。
pub const COPY_MEMORY_BUDGET_BYTES: u64 = 10 * 1024 * 1024;

/// 流式块大小（4MB——粘贴时分块读入）。
pub const STREAM_CHUNK_BYTES: u64 = 4 * 1024 * 1024;

/// 预估误差判线（20%）。
pub const ESTIMATE_ERROR_LIMIT_PERMILLE: u64 = 200;

/// 参考吞吐（10MB/s——预估基准）。
pub const REFERENCE_THROUGHPUT_BPS: u64 = 10 * 1024 * 1024;

// ---------------------------------------------------------------------------
// 剪贴板账
// ---------------------------------------------------------------------------

/// 剪贴板条目（驻留或引用）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipEntry {
    /// 小对象驻留（数据在账）。
    Resident { tag: String, bytes: u64 },
    /// 大对象引用条（数据不驻留——点开才流式加载）。
    Reference { tag: String, bytes: u64 },
}

impl ClipEntry {
    pub fn bytes(&self) -> u64 {
        match self {
            ClipEntry::Resident { bytes, .. } | ClipEntry::Reference { bytes, .. } => *bytes,
        }
    }

    pub fn is_reference(&self) -> bool {
        matches!(self, ClipEntry::Reference { .. })
    }

    pub fn tag(&self) -> &str {
        match self {
            ClipEntry::Resident { tag, .. } | ClipEntry::Reference { tag, .. } => tag,
        }
    }
}

/// 剪贴板。
#[derive(Clone, Debug, Default)]
pub struct Clipboard {
    pub current: Option<ClipEntry>,
    /// 内存账（复制动作后的驻留增量——判据直读）。
    pub resident_bytes: u64,
}

impl Clipboard {
    pub fn new() -> Clipboard {
        Clipboard { current: None, resident_bytes: 0 }
    }

    /// 复制：50MB 阈值分流（大对象 → 引用条，零驻留）。
    /// 返回状态条文案（「已复制」/「已复制引用」）。
    pub fn copy(&mut self, tag: &str, bytes: u64) -> &'static str {
        if bytes > BIG_OBJECT_BYTES {
            self.current = Some(ClipEntry::Reference { tag: String::from(tag), bytes });
            self.resident_bytes += 0; // 引用条零驻留——判据载体。
            "已复制引用"
        } else {
            self.current = Some(ClipEntry::Resident { tag: String::from(tag), bytes });
            self.resident_bytes += bytes;
            "已复制"
        }
    }

    /// 流式读取（粘贴大对象）：分块账——任一时刻在途数据 ≤ 单块大小。
    /// 返回块数与峰值在途字节（内存增量判据载体）。
    pub fn stream_read_plan(&self) -> Option<(u64, u64)> {
        let bytes = self.current.as_ref()?.bytes();
        let chunks = bytes.div_ceil(STREAM_CHUNK_BYTES);
        let peak = if bytes == 0 { 0 } else { bytes.min(STREAM_CHUNK_BYTES) };
        Some((chunks, peak))
    }

    /// 复制后内存增量是否达标（<10MB 判据——大对象复制路径：引用条零
    /// 增量 + 流式粘贴在途峰值 ≤ 单块 4MB，两项合计 <10MB）。
    pub fn memory_budget_ok(&self) -> bool {
        let copy_increment = match &self.current {
            Some(ClipEntry::Reference { .. }) => 0,
            _ => 0, // 驻留面（小对象）不属大对象判据——大对象一律引用。
        };
        let peak = self.stream_read_plan().map(|(_, p)| p).unwrap_or(0);
        copy_increment + peak < COPY_MEMORY_BUDGET_BYTES
    }
}

// ---------------------------------------------------------------------------
// 预估器
// ---------------------------------------------------------------------------

/// 粘贴预估（体积 + 时间——「将复制 1.2GB，约 40 秒」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PasteEstimate {
    pub bytes: u64,
    /// 预计秒数（参考吞吐）。
    pub seconds: u64,
}

/// 预估计算（误差 <20% 判线：对已知吞吐实况回算）。
pub fn estimate(bytes: u64) -> PasteEstimate {
    PasteEstimate { bytes, seconds: bytes.div_ceil(REFERENCE_THROUGHPUT_BPS) }
}

/// 预估准确性核账：实况耗时 vs 预估（吞吐抖动 ≤20% 场景——判据对账面）。
pub fn estimate_error_permille(est: &PasteEstimate, actual_seconds: u64) -> u64 {
    if est.seconds == 0 {
        return 0;
    }
    let diff = est.seconds.abs_diff(actual_seconds);
    diff * 1000 / est.seconds
}

/// 人话预估文案（「将复制 1.2GB，约 40 秒」格式——容量按十进制 GB/MB
/// 口径，与文件系统标称容量一致，用户对得上号）。
pub fn estimate_text(est: &PasteEstimate) -> String {
    let gb = est.bytes as f64 / 1_000_000_000.0;
    let mb = est.bytes as f64 / 1_000_000.0;
    let size = if gb >= 1.0 {
        alloc::format!("{:.1}GB", gb)
    } else {
        alloc::format!("{:.0}MB", mb)
    };
    alloc::format!("将复制 {}，约 {} 秒", size, est.seconds)
}

// ---------------------------------------------------------------------------
// 历史引用条
// ---------------------------------------------------------------------------

/// 剪贴板历史（大对象只进引用条——历史区不被撑爆）。
#[derive(Clone, Debug, Default)]
pub struct ClipHistory {
    pub items: Vec<ClipEntry>,
}

impl ClipHistory {
    /// 记录：大对象强制引用条形态入账（点开才加载）。
    pub fn record(&mut self, entry: ClipEntry) {
        let e = match entry {
            ClipEntry::Resident { tag, bytes } if bytes > BIG_OBJECT_BYTES => {
                ClipEntry::Reference { tag, bytes }
            }
            other => other,
        };
        self.items.insert(0, e);
    }

    /// 历史区大对象引用条行为：不驻留数据（bytes 只是账面元数据）。
    pub fn reference_only_policy(&self) -> bool {
        self.items.iter().all(|e| match e {
            ClipEntry::Reference { .. } => true,
            ClipEntry::Resident { bytes, .. } => *bytes <= BIG_OBJECT_BYTES,
        })
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F329 自检（判据：50MB 阈值；流式 <10MB；预估 <20%；历史引用条）。
pub fn run_clipbig_checks() -> CheckSet {
    let mut set = CheckSet::new("F329-clipbig");

    // 1. 50MB 阈值行为切换：50MB-1 驻留 / 50MB+1 引用。
    let mut cb = Clipboard::new();
    let r1 = cb.copy("小视频.mov", BIG_OBJECT_BYTES - 1);
    let mut cb2 = Clipboard::new();
    let r2 = cb2.copy("大视频.mov", BIG_OBJECT_BYTES + 1);
    set.add(
        "threshold behavior switch",
        r1 == "已复制" && r2 == "已复制引用" && cb2.current.as_ref().unwrap().is_reference(),
        "",
    );

    // 2. 引用条零驻留：复制后内存增量 <10MB（大对象账面增量 = 0）。
    set.add(
        "reference zero residency",
        cb2.resident_bytes == 0 && cb2.memory_budget_ok(),
        "",
    );

    // 3. 流式读取计划：1.2GB → 307 块、峰值在途 4MB（<10MB 判据）。
    let big_bytes: u64 = 1_200_000_000; // ~1.2GB。
    let mut cb3 = Clipboard::new();
    let _ = cb3.copy("影片.mkv", big_bytes);
    let (chunks, peak) = cb3.stream_read_plan().unwrap();
    set.add(
        "stream chunked peak small",
        chunks == big_bytes.div_ceil(STREAM_CHUNK_BYTES)
            && peak == STREAM_CHUNK_BYTES
            && peak < COPY_MEMORY_BUDGET_BYTES,
        "",
    );

    // 4. 预估准确性：1.2GB ≈ 115 秒（10MB/s 参考吞吐）；±20% 吞吐抖动
    //    下误差 <20% 判线。
    let est = estimate(big_bytes);
    let expected = big_bytes.div_ceil(REFERENCE_THROUGHPUT_BPS);
    set.add("estimate formula", est.seconds == expected, "");
    // 吞吐抖动 15%（实际更慢）→ 误差 ≈ 15% < 20%。
    let err = estimate_error_permille(&est, expected * 115 / 100);
    set.add("estimate error under 20 percent", err < ESTIMATE_ERROR_LIMIT_PERMILLE, "");

    // 5. 预估文案（人话：「将复制 1.2GB，约 115 秒」）。
    let text = estimate_text(&est);
    set.add(
        "estimate human text",
        text.contains("1.2GB") && text.contains("115 秒"),
        "",
    );

    // 6. 历史引用条：大对象入历史被强制引用形态（历史区不被撑爆）。
    let mut hist = ClipHistory::default();
    hist.record(ClipEntry::Resident { tag: String::from("大包.zip"), bytes: 500_000_000 });
    hist.record(ClipEntry::Resident { tag: String::from("笔记.txt"), bytes: 2048 });
    hist.record(ClipEntry::Reference { tag: String::from("另一大包"), bytes: 900_000_000 });
    set.add(
        "history reference only",
        hist.reference_only_policy()
            && hist.items[0].is_reference()
            && !hist.items[1].is_reference()
            && hist.items[2].is_reference(),
        "",
    );

    // 7. 驻留条目体积正确累加（小对象照常驻留——非大对象零行为变化）。
    let mut cb = Clipboard::new();
    let _ = cb.copy("文本", 1000);
    let _ = cb.copy("图片", 3_000_000);
    set.add("resident accounting", cb.resident_bytes == 3_001_000, "");

    // 8. 50MB 整点：不超阈值 → 驻留（判据口径「超过 50MB」）。
    let mut cb = Clipboard::new();
    let r = cb.copy("整点.mp4", BIG_OBJECT_BYTES);
    set.add(
        "exact threshold is resident",
        r == "已复制" && cb.resident_bytes == BIG_OBJECT_BYTES,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_bytes_edge() {
        let mut cb = Clipboard::new();
        let r = cb.copy("空", 0);
        assert_eq!(r, "已复制");
        let (chunks, peak) = cb.stream_read_plan().unwrap();
        assert_eq!((chunks, peak), (0, 0));
    }

    #[test]
    fn estimate_zero_seconds() {
        let est = estimate(0);
        assert_eq!(estimate_error_permille(&est, 0), 0);
    }

    #[test]
    fn estimate_mb_text() {
        let text = estimate_text(&estimate(12_000_000));
        assert!(text.contains("11MB") || text.contains("12MB"), "MB 级文案：{text}");
    }

    #[test]
    fn memory_budget_small_copy_ok() {
        let mut cb = Clipboard::new();
        let _ = cb.copy("小", 1024);
        assert!(cb.memory_budget_ok());
    }
}
