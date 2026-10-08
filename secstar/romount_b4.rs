//! F185 只读卷保护提示 · 批次四深化（secstar · G-G-15）。
//!
//! 批次四功能面（与批次三互补：批次三管「单任务状态机」，本批管
//! 「队列治理与路径映射」）：
//! - [`PathMapper`]：源→目标路径映射——盘符剥离 + 目标锚拼接 +
//!   重名后缀（同名文件拷两次不互相覆盖——映射面的确定性规则）；
//! - [`ProgressAgg`]：队列聚合进度——多任务 ‰ 加权汇总（总进度条
//!   的算术面：大文件权重高，不是简单平均）；
//! - [`cancel_all`]：全取消语义——队列清空 + 在途任务取消请求
//!   （「取消全部」按钮是真实出口，不是装饰）；
//! - [`SpaceWatch`]：拷贝中空间监控——剩余跌破阈值 → 暂停+提示
//!   （写满盘是缺陷：空间预检不能只查一次）。
//!
//! 零堆纪律：定长映射缓冲 + 定长聚合账，无 alloc。

use super::romount::{CopyFallbackJob, CopyQueue, COPY_QUEUE_CAP};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 路径映射
// ---------------------------------------------------------------------------

/// 路径缓冲长度（与 CopyFallbackJob::SRC_CAP 同尺）。
pub const PATH_CAP: usize = 64;

/// 映射结果错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathError {
    NoDrivePrefix,
    EmptyName,
    TooLong,
}

/// 源路径 `E:\doc\a.txt` → 目标 `downloads/a.txt`（盘符前缀剥离 +
/// 目标锚拼接；无盘符前缀诚实拒）。
pub fn map_path(src: &[u8], out: &mut [u8; PATH_CAP]) -> Result<usize, PathError> {
    if src.len() < 3 || src[1] != b':' || src[2] != b'\\' {
        return Err(PathError::NoDrivePrefix);
    }
    let name = &src[3..];
    if name.is_empty() {
        return Err(PathError::EmptyName);
    }
    let anchor = b"downloads/";
    if anchor.len() + name.len() > PATH_CAP {
        return Err(PathError::TooLong);
    }
    out[..anchor.len()].copy_from_slice(anchor);
    // 卷上路径用反斜杠、VARIX 区用正斜杠——映射时规范化（目标侧的
    // 路径分隔符语义跟目标走）。
    for (i, b) in name.iter().enumerate() {
        out[anchor.len() + i] = if *b == b'\\' { b'/' } else { *b };
    }
    let n = anchor.len() + name.len();
    out[n..].fill(0);
    Ok(n)
}

/// 重名后缀：`a.txt` 已存在 → `a (2).txt`（拷两次不互相覆盖——
/// 扩展名前插后缀，Windows 同款语义）。
pub fn dedupe_name(name: &[u8], existing: &mut [u8; PATH_CAP], existing_len: &mut usize) -> bool {
    // 冲突判定：existing 与 name 相同 → 改写为 stem (2).ext。
    if existing[..*existing_len] != name[..] {
        return true; // 无冲突原样放行
    }
    let dot = name.iter().rposition(|b| *b == b'.');
    let (stem, ext) = match dot {
        Some(d) if d > 0 => (&name[..d], &name[d..]),
        _ => (name, &b""[..]),
    };
    let mut candidate = [0u8; PATH_CAP];
    let mut n = 0;
    let suffix = b" (2)";
    if stem.len() + suffix.len() + ext.len() > PATH_CAP {
        return false;
    }
    candidate[..stem.len()].copy_from_slice(stem);
    n += stem.len();
    candidate[n..n + suffix.len()].copy_from_slice(suffix);
    n += suffix.len();
    candidate[n..n + ext.len()].copy_from_slice(ext);
    n += ext.len();
    existing[..n].copy_from_slice(&candidate[..n]);
    *existing_len = n;
    true
}

// ---------------------------------------------------------------------------
// 队列聚合进度（字节加权）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct ProgressAgg {
    /// 各任务 (done, total) 字节账——最多 8 任务。
    done: [u64; COPY_QUEUE_CAP],
    total: [u64; COPY_QUEUE_CAP],
    pub n: usize,
}

impl ProgressAgg {
    pub const fn new() -> ProgressAgg {
        ProgressAgg { done: [0; COPY_QUEUE_CAP], total: [0; COPY_QUEUE_CAP], n: 0 }
    }

    pub fn add_task(&mut self, total_bytes: u64) -> bool {
        if self.n >= COPY_QUEUE_CAP {
            return false;
        }
        self.total[self.n] = total_bytes;
        self.n += 1;
        true
    }

    pub fn on_progress(&mut self, task_idx: usize, bytes: u64) -> bool {
        if task_idx >= self.n {
            return false;
        }
        self.done[task_idx] = self.done[task_idx].saturating_add(bytes).min(self.total[task_idx]);
        true
    }

    /// 聚合进度 ‰：字节加权（Σdone / Σtotal）——大文件权重高。
    pub fn aggregate_permille(&self) -> u32 {
        let total: u64 = self.total[..self.n].iter().sum();
        if total == 0 {
            return 0;
        }
        let done: u64 = self.done[..self.n].iter().sum();
        (done * 1_000 / total) as u32
    }
}

// ---------------------------------------------------------------------------
// 全取消语义
// ---------------------------------------------------------------------------

/// 全取消结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CancelReport {
    /// 队列中待做的任务数（直接清空）。
    pub queued_dropped: usize,
    /// 在途任务取消请求（下一拍生效——异步取消语义与批次三一致）。
    pub in_flight_cancelled: bool,
}

/// 全取消：队列排干 + 在途置取消旗。
pub fn cancel_all(queue: &mut CopyQueue, in_flight: &mut bool) -> CancelReport {
    let dropped = queue.n;
    for slot in queue.jobs.iter_mut() {
        *slot = None;
    }
    queue.n = 0;
    *in_flight = true;
    CancelReport { queued_dropped: dropped, in_flight_cancelled: true }
}

// ---------------------------------------------------------------------------
// 拷贝中空间监控
// ---------------------------------------------------------------------------

/// 剩余空间警戒线（KiB——跌破即暂停）。
pub const SPACE_FLOOR_KIB: u64 = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpaceVerdict {
    Ok,
    /// 跌破警戒线 → 暂停 + 提示清理。
    PauseAndAsk,
}

/// 每任务前空间复核（不只查一次——每个任务开工前再看一眼）。
pub fn space_watch(free_kib: u64, need_kib: u64) -> SpaceVerdict {
    if free_kib >= SPACE_FLOOR_KIB + need_kib {
        SpaceVerdict::Ok
    } else {
        SpaceVerdict::PauseAndAsk
    }
}

// ---------------------------------------------------------------------------
// 批次四自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_romount_b4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F185-b4");

    // 1) 路径映射：E:\doc\a.txt → downloads/doc/a.txt（前缀剥离+锚拼）。
    let mut buf = [0u8; PATH_CAP];
    let n = map_path(b"E:\\doc\\a.txt", &mut buf).unwrap();
    cs.add("path_map_basic", &buf[..n] == b"downloads/doc/a.txt", "");

    // 2) 路径映射三拒：无盘符 / 空名 / 超长（映射面诚实）。
    let e1 = map_path(b"doc/a.txt", &mut buf);
    let e2 = map_path(b"E:\\", &mut buf);
    // 超长用例：E:\ + 64 个 x（定长数组——零堆纪律下不用 Vec）。
    let mut long_src = [0u8; PATH_CAP + 3];
    long_src[0] = b'E';
    long_src[1] = b':';
    long_src[2] = b'\\';
    long_src[3..].fill(b'x');
    let e3_long = map_path(&long_src, &mut buf);
    cs.add(
        "path_map_rejects",
        e1 == Err(PathError::NoDrivePrefix) && e2 == Err(PathError::EmptyName) && e3_long == Err(PathError::TooLong),
        "",
    );

    // 3) 重名后缀：无冲突原样、有冲突 a.txt → a (2).txt（扩展名前插）。
    let mut existing = [0u8; PATH_CAP];
    let src = b"report.docx";
    existing[..src.len()].copy_from_slice(src);
    let mut elen = src.len();
    let dedup = dedupe_name(src, &mut existing, &mut elen);
    cs.add(
        "dedupe_conflict",
        dedup && &existing[..elen] == b"report (2).docx",
        "",
    );

    // 4) 重名无冲突：不同名原样放行（不乱加后缀）。
    let mut ex2 = [0u8; PATH_CAP];
    let other = b"other.txt";
    ex2[..other.len()].copy_from_slice(other);
    let mut el2 = other.len();
    let no_conflict = dedupe_name(b"report.docx", &mut ex2, &mut el2);
    cs.add("dedupe_no_conflict", no_conflict && &ex2[..el2] == b"other.txt", "");

    // 5) 聚合进度：字节加权（大任务权重高——非简单平均）。
    let mut agg = ProgressAgg::new();
    agg.add_task(9_000); // 大任务 9KB
    agg.add_task(1_000); // 小任务 1KB
    agg.on_progress(0, 9_000); // 大任务完成
    cs.add("agg_weighted", agg.aggregate_permille() == 900, "");

    // 6) 聚合进度空队：0 总量 → 0‰（不编造）。
    cs.add("agg_empty_zero", ProgressAgg::new().aggregate_permille() == 0, "");

    // 7) 聚合进度钳制：单任务超量上报不越界（饱和语义）。
    let mut agg2 = ProgressAgg::new();
    agg2.add_task(1_000);
    agg2.on_progress(0, u64::MAX);
    cs.add("agg_clamped", agg2.aggregate_permille() == 1_000, "");

    // 8) 全取消：队列排干 + 在途置旗（两出口各有名）。
    let mut q = CopyQueue::new();
    q.enqueue(CopyFallbackJob::new(b'E', b"f0.txt"));
    q.enqueue(CopyFallbackJob::new(b'E', b"f1.txt"));
    q.enqueue(CopyFallbackJob::new(b'E', b"f2.txt"));
    let mut in_flight = false;
    let rep = cancel_all(&mut q, &mut in_flight);
    cs.add(
        "cancel_all",
        rep.queued_dropped == 3 && rep.in_flight_cancelled && q.n == 0 && in_flight,
        "",
    );

    // 9) 空间监控：满量充足 Ok、跌破警戒线 PauseAndAsk（两态逐点）。
    cs.add(
        "space_watch",
        space_watch(SPACE_FLOOR_KIB + 1_000, 500) == SpaceVerdict::Ok
            && space_watch(SPACE_FLOOR_KIB, 1) == SpaceVerdict::PauseAndAsk
            && space_watch(0, 0) == SpaceVerdict::PauseAndAsk,
        "",
    );

    // 10) 主册常量贯通：队列 8 上限一处一事实。
    cs.add("consts_aligned", COPY_QUEUE_CAP == 8, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次四）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b4 {
    use super::*;

    #[test]
    fn path_map_roots() {
        // 根文件：E:\a.txt → downloads/a.txt（无子目录场景）。
        let mut buf = [0u8; PATH_CAP];
        let n = map_path(b"E:\\a.txt", &mut buf).unwrap();
        assert_eq!(&buf[..n], b"downloads/a.txt");
    }

    #[test]
    fn dedupe_chain() {
        // 连续两次同名拷贝：第一次无冲突、第二次加 (2)（不覆盖第一次）。
        let mut existing = [0u8; PATH_CAP];
        let mut elen = 0usize;
        let name = b"doc.txt";
        // 第一次：existing 空 → 无冲突。
        assert!(dedupe_name(name, &mut existing, &mut elen));
        assert_eq!(elen, 0, "existing 未动——调用方自行落盘后同步");
        // existing 写入 name 后再拷同名 → 加后缀。
        existing[..name.len()].copy_from_slice(name);
        elen = name.len();
        assert!(dedupe_name(name, &mut existing, &mut elen));
        assert_eq!(&existing[..elen], b"doc (2).txt");
    }

    #[test]
    fn agg_multi_task_exact() {
        // 三任务精确加权：完成 2/3 大任务 + 全部小任务 → 精确 ‰。
        let mut agg = ProgressAgg::new();
        agg.add_task(3_000);
        agg.add_task(3_000);
        agg.add_task(3_000);
        agg.on_progress(0, 3_000);
        agg.on_progress(1, 2_000);
        agg.on_progress(2, 0);
        assert_eq!(agg.aggregate_permille(), 555); // done 5000 / total 9000 → 555‰
    }
}
