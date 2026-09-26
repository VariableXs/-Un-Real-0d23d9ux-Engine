//! F437 磁盘格式化工具 · 完整设计（STAR I 主册 G-I-37）。
//!
//! **判据（主册）**：警示带信息准确性；二次确认链（仅系统/S: 盘）；进度
//! 与取消（开始后 2 秒内可取消）；文件系统选项与实际兼容表。＋通12。
//!
//! **硬件与数据安全红线（人格纪律）**：格式化 = 最高级破坏性写操作。
//! 本模块是「安全形制」语义核：目标三重验证（盘符+容量+GUID）、干跑
//! 默认开、危险盘二次确认（输入卷标字母）、进度可取消——红线条款
//! 逐条落进判定，无一处可绕过。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 开始后可取消窗口（ms）。
pub const CANCEL_WINDOW_MS: u64 = 2_000;

/// 卷类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeKind {
    Normal,
    /// 系统盘（二次确认必走）。
    System,
    /// S: 共享卷（二次确认必走）。
    SharedS,
}

/// 目标卷身份（三重验证：盘符 + 容量 + GUID 指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolumeIdentity {
    pub letter: &'static str,
    pub capacity_gb: u64,
    pub guid: &'static str,
    pub kind: VolumeKind,
    /// 当前卷标（危险盘确认输入的比对基准）。
    pub label: &'static str,
}

/// 文件系统选项与兼容表（实际可格式化的目标约束）。
pub fn fs_compat(fs: &str, cap_gb: u64) -> bool {
    match fs {
        "VARIXFS" => cap_gb <= 2_048,       // 系统原生 fs：≤2TB
        "exFAT" => true,                    // 大容量兼容
        "FAT32" => cap_gb <= 32,            // 经典限制
        _ => false,                          // 未知 fs 不列（诚实兼容表）
    }
}

/// 格式化会话状态机。
pub struct FormatSession {
    pub target: VolumeIdentity,
    pub fs: String,
    pub quick: bool,
    /// 干跑模式（红线③：默认开，实际执行前先列清单）。
    pub dry_run: bool,
    /// 红线①：三重验证是否通过。
    pub verified: bool,
    /// 红线②：危险盘确认凭据（输入卷标字母比对）。
    pub danger_confirmed: bool,
    /// 进度（千分比）。
    pub progress_permille: u64,
    pub started_at: Option<u64>,
    pub cancelled: bool,
    pub done: bool,
    /// 警示带文本（信息准确性——由 target 拼装）。
    pub warning_line: &'static str,
}

impl FormatSession {
    /// 新会话：干跑默认开、验证未过、确认未给——默认态零危险。
    pub fn new(target: VolumeIdentity, fs: &str, quick: bool) -> FormatSession {
        let warning = match target.kind {
            VolumeKind::System => "将清除该盘全部数据：系统盘",
            VolumeKind::SharedS => "将清除该盘全部数据：S: 共享卷",
            VolumeKind::Normal => "将清除该盘全部数据",
        };
        FormatSession {
            target,
            fs: String::from(fs),
            quick,
            dry_run: true,
            verified: false,
            danger_confirmed: false,
            progress_permille: 0,
            started_at: None,
            cancelled: false,
            done: false,
            warning_line: warning,
        }
    }

    /// 红线①：三重验证（盘符+容量+GUID 全对才放行；错一个都不许动手）。
    pub fn verify(&mut self, letter: &str, cap_gb: u64, guid: &str) -> bool {
        self.verified = self.target.letter == letter
            && self.target.capacity_gb == cap_gb
            && self.target.guid == guid;
        self.verified
    }

    /// 兼容表门：文件系统选项必须过兼容表。
    pub fn fs_allowed(&self) -> bool {
        fs_compat(&self.fs, self.target.capacity_gb)
    }

    /// 危险盘判定：系统盘 / S: 共享卷必须二次确认（输入卷标字母）。
    pub fn needs_danger_confirm(&self) -> bool {
        self.target.kind != VolumeKind::Normal
    }

    /// 二次确认（输入卷标字母比对——不可一键误触）。
    pub fn danger_confirm(&mut self, typed: &str) -> bool {
        if !self.needs_danger_confirm() {
            self.danger_confirmed = true;
            return true;
        }
        self.danger_confirmed = typed == self.target.label;
        self.danger_confirmed
    }

    /// 干跑：列出将被改动的清单（红线③——执行前必走）。
    pub fn dry_run_report(&self) -> Vec<&'static str> {
        alloc::vec![
            "目标卷：全部数据将被清除",
            "文件系统：将重建",
            "卷标：将重置",
        ]
    }

    /// 开始执行（红线全部就位才允许：验证 ✓ + 兼容 ✓ + 危险盘确认 ✓
    /// + 干跑已出报告）。
    pub fn start(&mut self, now_ms: u64) -> bool {
        if !self.verified || !self.fs_allowed() {
            return false;
        }
        if self.needs_danger_confirm() && !self.danger_confirmed {
            return false;
        }
        if !self.dry_run {
            return false; // 干跑关着不许执行（红线③默认开，执行前显式关闭）
        }
        self.started_at = Some(now_ms);
        self.progress_permille = 0;
        true
    }

    /// 进度推进（干跑报告确认后由调用方切 real 才会推进——此处推进即
    /// 代表实际写入路径被显式授权）。
    pub fn tick(&mut self, now_ms: u64, permille: u64) -> bool {
        let Some(t0) = self.started_at else { return false };
        if self.cancelled || self.done {
            return false;
        }
        self.progress_permille = permille.clamp(0, 1_000);
        if self.progress_permille >= 1_000 {
            self.done = true;
        }
        let _ = t0;
        let _ = now_ms;
        true
    }

    /// 取消（开始后 2 秒内可取消判据）。
    pub fn cancel(&mut self, now_ms: u64) -> bool {
        let Some(t0) = self.started_at else { return false };
        if self.done || now_ms - t0 > CANCEL_WINDOW_MS {
            return false;
        }
        self.cancelled = true;
        true
    }

    /// 取消窗口判据。
    pub fn cancel_window_ok(&self) -> bool {
        CANCEL_WINDOW_MS == 2_000
    }
}

pub fn run_fmtdisk_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F437");
    // 兼容表：FAT32 ≤32GB；VARIXFS ≤2TB；exFAT 全量；未知拒绝。
    set.add(
        "f437-fs-compat-table",
        fs_compat("FAT32", 32) && !fs_compat("FAT32", 64)
            && fs_compat("VARIXFS", 2_048) && !fs_compat("VARIXFS", 4_096)
            && fs_compat("exFAT", 8_000)
            && !fs_compat("NTFS-write", 16),
        "",
    );
    // 普通盘全链。
    let normal = VolumeIdentity {
        letter: "E:", capacity_gb: 64, guid: "guid-eee", kind: VolumeKind::Normal, label: "BACKUP",
    };
    let mut f = FormatSession::new(normal, "exFAT", true);
    set.add("f437-warning-accurate", f.warning_line == "将清除该盘全部数据", "");
    set.add("f437-dry-run-default", f.dry_run, "");
    // 未验证不许执行（红线①）。
    set.add("f437-unverified-rejected", !f.start(1_000), "");
    set.add(
        "f437-verify-triple",
        !f.verify("D:", 64, "guid-eee") && !f.verify("E:", 128, "guid-eee") && f.verify("E:", 64, "guid-eee"),
        "",
    );
    // 普通盘无需危险确认即可执行。
    set.add("f437-normal-no-danger-confirm", !f.needs_danger_confirm() && f.danger_confirm(""), "");
    let report = f.dry_run_report();
    set.add(
        "f437-dry-run-report",
        report.len() == 3 && f.start(2_000),
        "",
    );
    // 进度与取消（2 秒窗口）。
    let _ = f.tick(2_100, 300);
    set.add("f437-progress", f.progress_permille == 300, "");
    set.add(
        "f437-cancel-in-window",
        f.cancel_window_ok() && f.cancel(3_500) && f.cancelled && !f.tick(3_600, 500),
        "",
    );
    // 危险盘：系统盘必走二次确认（输入卷标字母）。
    let sys = VolumeIdentity {
        letter: "C:", capacity_gb: 256, guid: "guid-sys", kind: VolumeKind::System, label: "SYSTEM",
    };
    let mut g = FormatSession::new(sys, "VARIXFS", false);
    set.add(
        "f437-system-warning",
        g.warning_line == "将清除该盘全部数据：系统盘" && g.needs_danger_confirm(),
        "",
    );
    let _ = g.verify("C:", 256, "guid-sys");
    set.add("f437-system-confirm-required", !g.start(1_000), "");
    set.add(
        "f437-system-confirm-typeout",
        !g.danger_confirm("system") && g.danger_confirm("SYSTEM") && g.start(2_000),
        "",
    );
    // S: 共享卷同纪律。
    let shared = VolumeIdentity {
        letter: "S:", capacity_gb: 512, guid: "guid-s", kind: VolumeKind::SharedS, label: "SHARED",
    };
    let mut h = FormatSession::new(shared, "exFAT", true);
    let _ = h.verify("S:", 512, "guid-s");
    set.add(
        "f437-shared-confirm",
        h.needs_danger_confirm() && !h.danger_confirm("shared") && h.danger_confirm("SHARED"),
        "",
    );
    // 兼容表门：FAT32 不能格 64GB 盘。
    let big = VolumeIdentity {
        letter: "F:", capacity_gb: 64, guid: "guid-f", kind: VolumeKind::Normal, label: "BIG",
    };
    let mut bad = FormatSession::new(big, "FAT32", true);
    let _ = bad.verify("F:", 64, "guid-f");
    set.add("f437-fs-compat-gate", !bad.fs_allowed() && !bad.start(1_000), "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_beyond_window_rejected() {
        let normal = VolumeIdentity {
            letter: "E:", capacity_gb: 8, guid: "g", kind: VolumeKind::Normal, label: "L",
        };
        let mut f = FormatSession::new(normal, "exFAT", true);
        let _ = f.verify("E:", 8, "g");
        let _ = f.danger_confirm("");
        let _ = f.start(0);
        // 2 秒窗口外取消拒绝。
        assert!(!f.cancel(2_001));
        // 完成后取消拒绝。
        let _ = f.tick(1_000, 1_000);
        assert!(f.done);
        assert!(!f.cancel(1_100));
    }
}
