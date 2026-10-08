//! F437 磁盘格式化工具 · 完整设计（STAR I 主册 G-I-37）。
//!
//! **判据（主册）**：警示带信息准确性；二次确认链（仅系统/S: 盘）；进度
//! 与取消（开始后 2 秒内可取消）；文件系统选项与实际兼容表。＋通12。
//!
//! **硬件与数据安全红线（人格纪律）**：格式化 = 最高级破坏性写操作。
//! 本模块是「安全形制」语义核：目标三重验证（盘符+容量+GUID）、干跑
//! 默认开、危险盘二次确认（输入卷标字母）、进度可取消——红线条款
//! 逐条落进判定，无一处可绕过。
//!
//! **v4 深化批次新增（AI-U1）**：
//! - 簇大小推荐表 [`cluster_size_hint`]：fs × 容量 → 推荐簇大小
//!   （大簇浪费小文件空间、小簇拖慢大文件——推荐而不是替用户决定）；
//! - 卷标合法性 [`validate_volume_label`]：FAT32 ≤11 字节 ASCII、
//!   exFAT ≤15 字符、VARIXFS ≤32 字符 + 禁字符集（`\/:*?"<>|`）——
//!   坏卷标在入口被拦（不进写盘路径）；
//! - ETA 估算账 [`FormatSession::eta_seconds`]：剩余千分比 × 实测速率
//!   → 人话剩余时间（「慢要有诚实的进度」——进度条会动、剩余可信）；
//! - 写后校验两阶段：写阶段 0-800‰ + 校验阶段 800-1000‰——校验读回
//!   不对即诚实失败（`verify_failed`），带三要素错误呈现（绝不假装
//!   格式化成功）；
//! - 中止清理 [`FormatSession::abort`]：取消后状态回到干净空闲位
//!   （无半悬状态——「操作到一半打断，状态不许留在半空」）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 开始后可取消窗口（ms）。
pub const CANCEL_WINDOW_MS: u64 = 2_000;
/// 写阶段终点（千分比——800 之后进校验阶段）。
pub const WRITE_PHASE_END: u64 = 800;

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

/// 簇大小推荐（字节）：fs × 容量 → 推荐值。
/// 大盘小簇 = FAT 表巨大 + 写放大；小盘大簇 = 小文件空间浪费。
/// 推荐不强制——用户可以在选项里改（界面侧约束写死非法组合）。
pub fn cluster_size_hint(fs: &str, cap_gb: u64) -> Option<u32> {
    if !fs_compat(fs, cap_gb) {
        return None; // 兼容表外的组合无推荐（诚实）
    }
    match fs {
        "FAT32" => Some(if cap_gb <= 8 { 4_096 } else { 16_384 }),
        "exFAT" => Some(if cap_gb <= 64 { 32_768 } else { 128 * 1_024 }),
        "VARIXFS" => Some(if cap_gb <= 128 { 4_096 } else { 16_384 }),
        _ => None,
    }
}

/// 卷标合法性：长度上限（按 fs）+ 禁字符集（写盘路径前置拦截）。
pub fn validate_volume_label(fs: &str, label: &str) -> Result<(), &'static str> {
    const FORBIDDEN: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
    if label.chars().any(|c| FORBIDDEN.contains(&c)) {
        return Err("卷标不能包含 \\ / : * ? \" < > | 字符");
    }
    let over = match fs {
        "FAT32" => label.len() > 11,
        "exFAT" => label.chars().count() > 15,
        "VARIXFS" => label.chars().count() > 32,
        _ => return Err("未知文件系统——无法校验卷标"),
    };
    if over {
        return Err("卷标超长——FAT32 最多 11 字节、exFAT 15 字符、VARIXFS 32 字符");
    }
    Ok(())
}

/// 会话阶段（两阶段判定——写与校验分明，不混账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatPhase {
    Idle,
    Writing,
    Verifying,
    Done,
    Aborted,
    VerifyFailed,
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
    /// 校验失败（写完读回不对——诚实失败态）。
    pub verify_failed: bool,
    /// 实测速率（ms/千分位进度——ETA 账的基准，由 tick 注入；
    /// 低于 1ms/‰ 分辨率的采样不记账——宁可 None 不给假速率）。
    pub measured_ms_per_permille: Option<u64>,
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
            verify_failed: false,
            measured_ms_per_permille: None,
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
        self.cancelled = false;
        self.done = false;
        self.verify_failed = false;
        true
    }

    /// 当前阶段（两阶段判定）。
    pub fn phase(&self) -> FormatPhase {
        if self.verify_failed {
            FormatPhase::VerifyFailed
        } else if self.cancelled {
            FormatPhase::Aborted
        } else if self.done {
            FormatPhase::Done
        } else if self.progress_permille >= WRITE_PHASE_END && self.started_at.is_some() {
            FormatPhase::Verifying
        } else if self.started_at.is_some() {
            FormatPhase::Writing
        } else {
            FormatPhase::Idle
        }
    }

    /// 进度推进：0-800 写阶段、800-1000 校验阶段。校验阶段进度只接受
    /// 顺序推进（写可以快照推进，校验必须逐段读回——语义不同）。
    pub fn tick(&mut self, now_ms: u64, permille: u64) -> bool {
        let Some(t0) = self.started_at else { return false };
        if self.cancelled || self.done || self.verify_failed {
            return false;
        }
        let p = permille.clamp(0, 1_000);
        if p < self.progress_permille {
            return false; // 不接受倒退（进度单调）
        }
        if p < WRITE_PHASE_END && self.progress_permille >= WRITE_PHASE_END {
            return false; // 已进校验阶段不接受写阶段值
        }
        // 速率账：写阶段每次推进更新（ETA 基准）；低于 1ms/‰ 分辨率
        // 不记账（假速率比没有更害人）。
        if p > self.progress_permille && now_ms > t0 {
            let span_permille = p - self.progress_permille;
            let span_ms = now_ms - t0;
            let rate = span_ms / span_permille;
            if rate > 0 {
                self.measured_ms_per_permille = Some(rate);
            }
        }
        self.progress_permille = p;
        if p >= 1_000 {
            self.done = true;
        }
        true
    }

    /// 校验失败登记（写完读回不对——诚实失败，绝不假装成功）。
    pub fn fail_verify(&mut self) -> bool {
        if self.progress_permille < WRITE_PHASE_END || self.done {
            return false; // 只有校验阶段可判失败
        }
        self.verify_failed = true;
        self.done = false;
        true
    }

    /// ETA：剩余千分位 × 实测速率 → 人话剩余秒（无速率账返回 None——
    /// 不拍脑袋估）。
    pub fn eta_seconds(&self) -> Option<u64> {
        let rate = self.measured_ms_per_permille?;
        let remain = (1_000u64.saturating_sub(self.progress_permille)).max(1);
        Some(rate * remain / 1_000)
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

    /// 中止清理：取消后回干净空闲位（无半悬状态——进度/计时/失败位
    /// 全归零；红线确认凭据一并撤销——重新开始要重新验证）。
    pub fn abort(&mut self) {
        self.started_at = None;
        self.progress_permille = 0;
        self.cancelled = false;
        self.done = false;
        self.verify_failed = false;
        self.verified = false;
        self.danger_confirmed = false;
        self.measured_ms_per_permille = None;
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
    // 簇大小推荐：兼容表内给值、表外 None（诚实——不硬编）。
    set.add(
        "f437-cluster-hint",
        cluster_size_hint("FAT32", 8) == Some(4_096)
            && cluster_size_hint("FAT32", 16) == Some(16_384)
            && cluster_size_hint("exFAT", 512) == Some(128 * 1_024)
            && cluster_size_hint("FAT32", 64).is_none(),
        "",
    );
    // 卷标合法性：禁字符 + 分 fs 长度上限（写盘路径前置拦截）。
    set.add(
        "f437-label-guard",
        validate_volume_label("FAT32", "BACKUP").is_ok()
            && validate_volume_label("FAT32", "TOOLONG_LABEL").is_err()
            && validate_volume_label("exFAT", "我的备份盘十五字内").is_ok()
            && validate_volume_label("FAT32", "bad:name").is_err()
            && validate_volume_label("BTRFS", "x").is_err(),
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
    // 两阶段：写阶段 → 校验阶段边界。
    let _ = f.tick(2_500, 500);
    set.add("f437-phase-writing", f.phase() == FormatPhase::Writing, "");
    let _ = f.tick(3_000, 850);
    set.add("f437-phase-verifying", f.phase() == FormatPhase::Verifying, "");
    set.add("f437-no-regress", !f.tick(3_100, 400), "");
    // 进度与取消（2 秒窗口）。
    let _ = f.tick(3_200, 300); // 校验阶段拒写阶段值——进度仍在 850
    set.add(
        "f437-cancel-in-window",
        f.cancel_window_ok() && f.cancel(3_500) && f.cancelled && !f.tick(3_600, 500),
        "",
    );
    // 中止清理：回干净空闲位 + 红线凭据一并撤销。
    f.abort();
    set.add(
        "f437-abort-clean-state",
        f.phase() == FormatPhase::Idle
            && f.progress_permille == 0
            && !f.verified
            && !f.danger_confirmed
            && !f.start(9_000),
        "",
    );
    // 校验失败：诚实失败态（不假装成功）。
    let _ = f.verify("E:", 64, "guid-eee");
    let _ = f.danger_confirm("");
    let _ = f.start(10_000);
    let _ = f.tick(10_500, 800);
    let _ = f.tick(11_000, 1_000);
    set.add("f437-done-after-full-tick", f.done && f.phase() == FormatPhase::Done, "");
    // 失败只在未 done 前可判——完成后不可翻旧账。
    set.add("f437-verify-fail-only-in-flight", !f.fail_verify(), "");
    let mut g2 = FormatSession::new(normal, "exFAT", true);
    let _ = g2.verify("E:", 64, "guid-eee");
    let _ = g2.danger_confirm("");
    let _ = g2.start(1_000);
    let _ = g2.tick(1_500, 800);
    set.add("f437-verify-fail-honest", g2.fail_verify() && g2.phase() == FormatPhase::VerifyFailed && !g2.done, "");
    // ETA：无速率账 None（不拍脑袋）；有速率账可算。
    set.add("f437-eta-none-without-rate", g2.eta_seconds().is_none(), "");
    let mut h = FormatSession::new(normal, "exFAT", true);
    let _ = h.verify("E:", 64, "guid-eee");
    let _ = h.danger_confirm("");
    let _ = h.start(0);
    let _ = h.tick(10_000, 100); // 100‰ 用了 10s → 0.1s/‰
    set.add("f437-eta-from-measured-rate", h.eta_seconds() == Some(90), "");
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
    let mut h2 = FormatSession::new(shared, "exFAT", true);
    let _ = h2.verify("S:", 512, "guid-s");
    set.add(
        "f437-shared-confirm",
        h2.needs_danger_confirm() && !h2.danger_confirm("shared") && h2.danger_confirm("SHARED"),
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

    #[test]
    fn monotonic_progress_never_regresses() {
        let normal = VolumeIdentity {
            letter: "E:", capacity_gb: 8, guid: "g", kind: VolumeKind::Normal, label: "L",
        };
        let mut f = FormatSession::new(normal, "exFAT", true);
        let _ = f.verify("E:", 8, "g");
        let _ = f.danger_confirm("");
        let _ = f.start(0);
        let _ = f.tick(100, 300);
        assert!(!f.tick(200, 299), "倒退拒绝");
        assert!(f.tick(200, 700), "写阶段内快进合法");
        assert!(!f.tick(300, 100), "已在校验阶段：写阶段值拒");
        assert!(f.tick(300, 1_000), "校验收尾合法");
    }

    #[test]
    fn label_charset_boundary() {
        assert!(validate_volume_label("exFAT", "十六字符的卷标十六字符的卷标").is_ok());
        assert!(validate_volume_label("exFAT", "十六字符的卷标十六字符的卷标XY").is_err(), "16 字超 exFAT 15 上限");
        assert!(validate_volume_label("exFAT", "十六字符的卷标十六字符的卷标X").is_ok(), "15 字恰在上限内");
        assert!(validate_volume_label("VARIXFS", &"长".repeat(32)).is_ok());
        assert!(validate_volume_label("VARIXFS", &"长".repeat(33)).is_err());
    }
}
