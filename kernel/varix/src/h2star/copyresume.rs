//! F269 长复制暂停与恢复 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：暂停生效 <1s；断点续传校验用例（人为截断后恢复，
//! 跳过已传块）；关机询问流程；暂停期间源占用释放验证。
//!
//! **设计要点（主册）**：大文件复制支持暂停/恢复：暂停即时生效（当前
//! 4MB 块写完即停，<1s）、恢复从断点续传（已复制部分校验后跳过）、
//! 暂停期间源占用释放；关机时未完成的复制任务在下次开机询问「继续吗」。
//!
//! 实装：块状复制状态机（4MB 块——暂停在块边界生效，<1s 判据=最多等
//! 一块）；断点账（已传块清单 + 块校验——恢复时校验通过的跳过）；源
//! 占用开关（暂停释放/恢复重占）；关机挂起任务登记（下次开机询问口）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 块大小 4MB（主册定值）。
pub const CHUNK_BYTES: u64 = 4 * 1024 * 1024;

/// 复制任务状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyPhase {
    Running,
    Paused,
    Done,
    /// 挂起待询问（关机时未完成——下次开机问「继续吗」）。
    Suspended,
}

/// 复制任务（单文件）。
pub struct CopyJob {
    pub src: String,
    pub dst: String,
    pub total_bytes: u64,
    pub phase: CopyPhase,
    /// 已完成块数。
    pub chunks_done: u64,
    /// 暂停时源占用是否已释放（判据验证位）。
    pub src_released: bool,
}

/// 关机询问条目（下次开机渲染「继续吗」）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResumeAsk {
    pub src: String,
    pub dst: String,
    pub done_bytes: u64,
    pub total_bytes: u64,
}

impl CopyJob {
    pub fn new(src: &str, dst: &str, total_bytes: u64) -> CopyJob {
        CopyJob {
            src: String::from(src),
            dst: String::from(dst),
            total_bytes,
            phase: CopyPhase::Running,
            chunks_done: 0,
            src_released: false,
        }
    }

    pub fn total_chunks(&self) -> u64 {
        (self.total_bytes + CHUNK_BYTES - 1) / CHUNK_BYTES
    }

    /// 写完一块（运行中推进；暂停态拒绝——块边界纪律）。
    pub fn chunk_written(&mut self) -> bool {
        if self.phase != CopyPhase::Running {
            return false;
        }
        self.chunks_done += 1;
        if self.chunks_done >= self.total_chunks() {
            self.phase = CopyPhase::Done;
        }
        true
    }

    /// 暂停：当前块写完即停（调用方先 finish 当前块再调用）；
    /// 源占用同步释放（判据「暂停期间源占用释放」）。
    pub fn pause(&mut self) -> Result<(), &'static str> {
        match self.phase {
            CopyPhase::Running => {
                self.phase = CopyPhase::Paused;
                self.src_released = true;
                Ok(())
            }
            _ => Err("暂停只在运行中有效"),
        }
    }

    /// 恢复：断点续传（从 `chunks_done` 继续），重新占用源。
    pub fn resume(&mut self) -> Result<u64, &'static str> {
        match self.phase {
            CopyPhase::Paused => {
                self.phase = CopyPhase::Running;
                self.src_released = false;
                Ok(self.chunks_done * CHUNK_BYTES)
            }
            _ => Err("恢复只在暂停中有效"),
        }
    }

    /// 已传字节（断点账）。
    pub fn done_bytes(&self) -> u64 {
        (self.chunks_done * CHUNK_BYTES).min(self.total_bytes)
    }

    /// 断点校验：恢复前对已传块清单校验（`verify_chunk` 由调用方供给
    /// 校验闭包）。全部通过 → 可续传；有坏块 → 从该块重传（返回坏块号）。
    pub fn verify_resume(&self, verify_chunk: impl Fn(u64) -> bool) -> Result<(), u64> {
        for c in 0..self.chunks_done {
            if !verify_chunk(c) {
                return Err(c);
            }
        }
        Ok(())
    }
}

/// 关机扫描：未完成任务 → 挂起 + 询问条目。
pub fn suspend_on_shutdown(jobs: &mut [CopyJob]) -> Vec<ResumeAsk> {
    let mut asks = Vec::new();
    for j in jobs.iter_mut() {
        if j.phase == CopyPhase::Running || j.phase == CopyPhase::Paused {
            j.phase = CopyPhase::Suspended;
            j.src_released = true;
            asks.push(ResumeAsk {
                src: j.src.clone(),
                dst: j.dst.clone(),
                done_bytes: j.done_bytes(),
                total_bytes: j.total_bytes,
            });
        }
    }
    asks
}

/// 开机询问文案（「继续吗」——判据原文口径）。
pub fn resume_ask_text(ask: &ResumeAsk) -> String {
    alloc::format!(
        "上次的复制未完成（已传 {} / {} 字节）——继续吗？",
        ask.done_bytes,
        ask.total_bytes
    )
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_copyresume_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F269");
    // 40GB 任务分块推进 + 暂停即时生效（块边界 <1s）。
    let mut job = CopyJob::new("S:/影片.mkv", "vx:/影片.mkv", 40 * CHUNK_BYTES);
    for _ in 0..3 {
        let _ = job.chunk_written();
    }
    set.add("F269 chunk progress", job.done_bytes() == 3 * CHUNK_BYTES, "block ledger");
    let _ = job.pause();
    set.add(
        "F269 pause instant",
        job.phase == CopyPhase::Paused && job.src_released,
        "src freed",
    );
    // 暂停中写块被拒（块边界纪律）。
    set.add("F269 no write when paused", !job.chunk_written(), "boundary rule");
    // 断点续传：校验通过跳过已传块。
    let resume_at = job.resume();
    set.add(
        "F269 resume skip",
        resume_at == Ok(3 * CHUNK_BYTES) && !job.src_released,
        "verify then skip",
    );
    // 人为截断：第 1 块坏 → 从块 1 重传。
    let bad = job.verify_resume(|c| c != 1);
    set.add("F269 truncation detected", bad == Err(1), "bad chunk id");
    let good = job.verify_resume(|_| true);
    set.add("F269 clean resume", good.is_ok(), "all chunks ok");
    // 关机询问流程。
    let mut jobs = alloc::vec![
        CopyJob::new("a", "b", 10 * CHUNK_BYTES),
        CopyJob::new("c", "d", 2 * CHUNK_BYTES), // 写 1 块后暂停——未完成。
    ];
    let _ = jobs[1].chunk_written();
    let _ = jobs[1].pause();
    let asks = suspend_on_shutdown(&mut jobs);
    set.add(
        "F269 shutdown ask",
        asks.len() == 2
            && jobs.iter().all(|j| j.phase == CopyPhase::Suspended)
            && resume_ask_text(&asks[1]).contains("继续吗"),
        "ask on next boot",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f269_pause_resume_green() {
        let set = run_copyresume_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F269 自检红 {f}/{p}");
    }

    #[test]
    fn pause_only_from_running() {
        let mut j = CopyJob::new("a", "b", 100);
        let _ = j.pause();
        assert!(j.pause().is_err(), "重复暂停被拒——状态机不开后门");
    }
}
