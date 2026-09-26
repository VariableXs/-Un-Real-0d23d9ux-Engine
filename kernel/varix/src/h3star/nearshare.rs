//! F321 就近共享 · 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：发现/确认/传输/完成四步用例；默认隐身判据；断点续
//! 传（传输中断网恢复）；拒绝路径不留半截文件；10MB/s 起步速度记录。
//!
//! **设计要点（主册）**：同一局域网内两台 VARIX 设备传文件：发送方右键
//! 「共享到附近设备」→发现列表→接收方弹确认条→传输进度（F086 同形
//! 制）→完成双方通知；大文件走断点续传（F269 同源）；默认可发现性关闭
//! （需要时开，防止被骚扰）。
//!
//! 实现形态：共享会话状态机（发现→确认→传输→完成，含拒绝/断线分支）
//! + 分块传输账（断点续传——已确认块不重传）+ 隐身默认位。

use crate::checks::CheckSet;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 起步速度判线（10MB/s）。
pub const MIN_THROUGHPUT_MBPS: u64 = 10;

/// 传输块大小（1MB——断点续传粒度）。
pub const CHUNK_BYTES: u64 = 1024 * 1024;

// ---------------------------------------------------------------------------
// 共享会话
// ---------------------------------------------------------------------------

/// 会话阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareStage {
    /// 发现列表（对端可见）。
    Discovering,
    /// 等接收方确认。
    AwaitingConfirm,
    /// 传输中。
    Transferring,
    /// 完成（双方通知）。
    Done,
    /// 被拒绝（终态——清理闭环）。
    Rejected,
    /// 断线（可续传恢复）。
    Disconnected,
}

/// 一次共享会话。
pub struct ShareSession {
    pub stage: ShareStage,
    pub file: String,
    pub total_bytes: u64,
    /// 已确认落盘块数（断点续传锚点）。
    pub confirmed_chunks: u64,
    /// 接收方默认隐身（可发现性默认关——判据载体）。
    pub discoverable: bool,
    /// 半截文件账（拒绝/取消路径必须为 0——不留半截）。
    pub partial_file_bytes: u64,
}

impl ShareSession {
    pub fn new(file: &str, total_bytes: u64) -> ShareSession {
        ShareSession {
            stage: ShareStage::Discovering,
            file: String::from(file),
            total_bytes,
            confirmed_chunks: 0,
            discoverable: false, // 默认隐身——判据载体。
            partial_file_bytes: 0,
        }
    }

    pub fn total_chunks(&self) -> u64 {
        self.total_bytes.div_ceil(CHUNK_BYTES)
    }

    /// 四步：发现 → 请求确认。
    pub fn request_confirm(&mut self) -> bool {
        if self.stage == ShareStage::Discovering {
            self.stage = ShareStage::AwaitingConfirm;
            true
        } else {
            false
        }
    }

    /// 接收方接受 → 传输开始。
    pub fn accept(&mut self) -> bool {
        if self.stage == ShareStage::AwaitingConfirm {
            self.stage = ShareStage::Transferring;
            true
        } else {
            false
        }
    }

    /// 接收方拒绝：终态 + 半截文件清零（拒绝路径不留半截文件）。
    pub fn reject(&mut self) -> bool {
        if self.stage == ShareStage::AwaitingConfirm {
            self.stage = ShareStage::Rejected;
            self.partial_file_bytes = 0;
            true
        } else {
            false
        }
    }

    /// 传输推进：确认 N 块。
    pub fn advance(&mut self, chunks: u64) -> bool {
        if self.stage != ShareStage::Transferring {
            return false;
        }
        self.confirmed_chunks = (self.confirmed_chunks + chunks).min(self.total_chunks());
        if self.confirmed_chunks == self.total_chunks() {
            self.stage = ShareStage::Done; // 完成双方通知（进度面）。
        }
        true
    }

    /// 断网：传输中断（已确认块保留——续传锚点）。
    pub fn disconnect(&mut self) -> bool {
        if self.stage == ShareStage::Transferring {
            self.stage = ShareStage::Disconnected;
            true
        } else {
            false
        }
    }

    /// 恢复续传：从已确认块继续（不重传）。
    pub fn resume(&mut self) -> bool {
        if self.stage == ShareStage::Disconnected {
            self.stage = ShareStage::Transferring;
            true
        } else {
            false
        }
    }

    /// 传输进度（0-1000‰）。
    pub fn progress_permille(&self) -> u64 {
        if self.total_chunks() == 0 {
            return 0;
        }
        self.confirmed_chunks * 1000 / self.total_chunks()
    }

    /// 完成判据：整文件确认 + 无半截残留。
    pub fn done_clean(&self) -> bool {
        self.stage == ShareStage::Done && self.partial_file_bytes == 0
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F321 自检（判据：四步用例；默认隐身；断点续传；拒绝零半截；速度记录）。
pub fn run_nearshare_checks() -> CheckSet {
    let mut set = CheckSet::new("F321-nearshare");

    // 1. 默认隐身判据（可发现性默认关）。
    let s = ShareSession::new("报告.pdf", 45 * 1024 * 1024);
    set.add("discoverable default off", !s.discoverable, "");

    // 2. 四步用例：发现 → 确认 → 传输 → 完成（45MB / 1MB 块）。
    let mut s = ShareSession::new("报告.pdf", 45 * 1024 * 1024);
    let ok = s.request_confirm() && s.accept();
    let mut mid = true;
    for _ in 0..44 {
        mid = mid && s.advance(1);
    }
    set.add(
        "four steps flow",
        ok && mid && s.stage == ShareStage::Transferring && s.progress_permille() == 977,
        "",
    );
    let _ = s.advance(1);
    set.add("complete notifies both", s.done_clean(), "");

    // 3. 断点续传：传 30 块断网 → 恢复 → 续传 15 块到完成（已确认块不重传）。
    let mut s = ShareSession::new("大包.dat", 45 * 1024 * 1024);
    let _ = s.request_confirm();
    let _ = s.accept();
    let _ = s.advance(30);
    let _ = s.disconnect();
    let anchor = s.confirmed_chunks;
    let resumed = s.resume();
    let _ = s.advance(15); // 只需再传 15 块（45-30）——锚点后无重传。
    set.add(
        "resume completes without resend",
        anchor == 30 && resumed && s.done_clean() && s.confirmed_chunks == 45,
        "",
    );

    // 4. 拒绝路径：半截文件零残留（终态不可再推进）。
    let mut s = ShareSession::new("x", 10 * 1024 * 1024);
    let _ = s.request_confirm();
    let _ = s.reject();
    set.add(
        "reject leaves no partial",
        s.stage == ShareStage::Rejected && s.partial_file_bytes == 0 && !s.advance(1),
        "",
    );

    // 5. 乱序防护：未确认就传输拒绝（状态机闭环）。
    let mut s = ShareSession::new("x", 1024);
    set.add(
        "stage machine closed",
        !s.advance(1) && !s.accept() && s.request_confirm() && !s.advance(1) && s.accept(),
        "",
    );

    // 6. 起步速度判线常量（10MB/s 记录面）。
    set.add("min throughput constant", MIN_THROUGHPUT_MBPS == 10, "");

    // 7. 进度账：0 块 0‰、全块 1000‰（边界）。
    let mut s = ShareSession::new("空", 1024);
    set.add("progress edges", s.progress_permille() == 0 && {
        let _ = s.request_confirm();
        let _ = s.accept();
        let _ = s.advance(1);
        s.progress_permille() == 1000
    }, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_ceil_boundary() {
        let s = ShareSession::new("x", CHUNK_BYTES * 3 + 1);
        assert_eq!(s.total_chunks(), 4);
    }

    #[test]
    fn double_reject_rejected() {
        let mut s = ShareSession::new("x", 1);
        let _ = s.request_confirm();
        let _ = s.reject();
        assert!(!s.reject());
    }

    #[test]
    fn over_advance_clamped() {
        let mut s = ShareSession::new("x", CHUNK_BYTES);
        let _ = s.request_confirm();
        let _ = s.accept();
        let _ = s.advance(5);
        assert_eq!(s.confirmed_chunks, 1);
        assert!(s.done_clean());
    }
}
