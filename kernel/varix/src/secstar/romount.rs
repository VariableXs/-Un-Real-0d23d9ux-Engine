//! F185 只读卷保护提示（secstar · G-G-15）——被拒绝的人需要知道为什么，以及下一步。
//!
//! 主册判据（验收标准第一句）：
//! **三类只读源徽标+原因分型正确；拖放受阻提示全链录屏；帮助链通。**
//!
//! 功能定义（G-G-15）：NTFS 只读挂载（B-705）等只读源在资源管理器显式标注：
//! 卷行「只读」徽标+原因 tooltip（「NTFS 只读挂载：保护 Windows 数据安全」）；
//! 写尝试 → 三要素提示（不是坏了，是设计如此）——限制有说明，不给「坏了」
//! 的错觉。
//!
//! 【交互设计】卷侧栏行徽标 12px 锁形+「只读」字；tooltip 原因模板（按卷
//! 类型三款）；拖放受阻 → 目标区禁止光标+顶部提示条（黄底三要素）；提示条
//! 含「了解 NTFS 只读策略」帮助链。
//! 【数据与存储】徽标态随挂载信息；无持久化。
//! 【状态与异常】只读原因多样（NTFS 策略/卷错误/写保护物理锁）→ 原因分型
//! 三模板（不笼统说只读）；卷错误型 → 引导「检查此卷」按钮（F120 修复项
//! 联动）。
//! 【设计细节】徽标走卷行右侧 4px 间距（乙-4 表语义）；三模板：策略型
//! （设计如此）/错误型（卷有故障+修复钮）/物理型（写保护开关——检测到则
//! 提示查硬件）；拖放提示条 4s 自动收（可点停）；「拷到 VARIX 区」为可执行
//! 按钮（一键复制到下载目录——被拒之后给台阶）。
//!
//! 零堆纪律：定长模板词条 + 定长提示条状态机，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 徽标 12px 锁形。
pub const BADGE_SIZE_PX: u32 = 12;
/// 徽标走卷行右侧 4px 间距（乙-4 表语义）。
pub const BADGE_OFFSET_PX: u32 = 4;
/// 拖放提示条 4s 自动收（可点停）。
pub const BANNER_AUTO_DISMISS_MS: u64 = 4_000;
/// 帮助链目标（「了解 NTFS 只读策略」——帮助链通的对账锚）。
pub const HELP_TARGET: &[u8] = b"help:F119/ntfs-readonly";
/// 「拷到 VARIX 区」动作的落点（下载目录语义锚）。
pub const COPY_FALLBACK_TARGET: &[u8] = b"downloads";

// ---------------------------------------------------------------------------
// 原因分型（三模板——不笼统说只读）
// ---------------------------------------------------------------------------

/// 只读原因分型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadOnlyCause {
    /// 策略型：NTFS 只读挂载（保护 Windows 数据安全——设计如此）。
    PolicyNtfs,
    /// 错误型：卷有故障（挂载降级只读——引导修复）。
    VolumeError,
    /// 物理型：写保护开关（卡/座物理锁——提示查硬件）。
    PhysicalLock,
}

impl ReadOnlyCause {
    /// 徽标 tooltip 原因模板（三款——逐型给真话）。
    pub fn tooltip(self) -> &'static str {
        match self {
            ReadOnlyCause::PolicyNtfs => "NTFS 只读挂载：保护 Windows 数据安全",
            ReadOnlyCause::VolumeError => "卷检测到故障，已降级只读挂载",
            ReadOnlyCause::PhysicalLock => "介质写保护开关已打开",
        }
    }

    /// 分诊建议（三要素的「下一步」槽位——每型各给真出路）。
    pub fn next_step(self) -> &'static str {
        match self {
            ReadOnlyCause::PolicyNtfs => "把文件先拷到 VARIX 区即可",
            ReadOnlyCause::VolumeError => "运行「检查此卷」修复后再写入",
            ReadOnlyCause::PhysicalLock => "关闭介质上的写保护开关",
        }
    }

    /// 错误型专属：「检查此卷」按钮（F120 修复项联动——只此型有）。
    pub fn repair_button(self) -> bool {
        self == ReadOnlyCause::VolumeError
    }

    /// 从挂载信息分型（B-705 注入口：ntfs_ro / fs_error / hw_writable=false 三通道）。
    pub fn classify(ntfs_readonly: bool, fs_error: bool, hw_write_protected: bool) -> Option<ReadOnlyCause> {
        // 分型优先序：物理锁 > 卷故障 > 策略（物理原因是硬约束——最诚实优先）。
        if hw_write_protected {
            Some(ReadOnlyCause::PhysicalLock)
        } else if fs_error {
            Some(ReadOnlyCause::VolumeError)
        } else if ntfs_readonly {
            Some(ReadOnlyCause::PolicyNtfs)
        } else {
            None // 可写卷——无徽标
        }
    }

    /// 元组适配（MountSource::mount_flags 的返回面直连——深化层接缝）。
    pub fn classify_tuple(flags: (bool, bool, bool)) -> Option<ReadOnlyCause> {
        Self::classify(flags.0, flags.1, flags.2)
    }
}

// ---------------------------------------------------------------------------
// 卷行徽标（12px 锁形 + 「只读」字 · 右侧 4px 间距）
// ---------------------------------------------------------------------------

/// 卷行只读徽标。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReadonlyBadge {
    pub cause: ReadOnlyCause,
}

impl ReadonlyBadge {
    /// 锁形徽标（12px）——形状即信号（色弱冗余 F114 联动语义）。
    pub const SHAPE: &str = "lock-12px";
    /// 「只读」字标。
    pub const LABEL: &str = "只读";
    /// 徽标右侧间距 4px。
    pub const OFFSET_PX: u32 = BADGE_OFFSET_PX;
}

// ---------------------------------------------------------------------------
// 写尝试拦截与提示条（黄底三要素 · 4s 自动收 · 可点停）
// ---------------------------------------------------------------------------

/// 提示条状态机：Hidden → Showing（4s 计时）→ Hidden；点击停驻=计时冻结。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DropBanner {
    pub visible: bool,
    pub pinned: bool,
    shown_at_ms: u64,
    cause: Option<ReadOnlyCause>,
}

impl DropBanner {
    pub const fn new() -> DropBanner {
        DropBanner { visible: false, pinned: false, shown_at_ms: 0, cause: None }
    }

    /// 写尝试被拒 → 黄条三要素弹出（三要素齐——发生了什么/为什么/下一步）。
    pub fn on_write_denied(&mut self, now_ms: u64, cause: ReadOnlyCause) {
        self.visible = true;
        self.pinned = false;
        self.shown_at_ms = now_ms;
        self.cause = Some(cause);
    }

    /// 点击停驻（可点停——用户在读时不收）。
    pub fn pin(&mut self) {
        if self.visible {
            self.pinned = true;
        }
    }

    /// 时间一拍：4s 自动收（停驻时不收）。
    pub fn tick(&mut self, now_ms: u64) {
        if self.visible && !self.pinned && now_ms.saturating_sub(self.shown_at_ms) >= BANNER_AUTO_DISMISS_MS {
            self.visible = false;
        }
    }

    /// 手动关闭。
    pub fn dismiss(&mut self) {
        self.visible = false;
        self.pinned = false;
    }

    /// 三要素文案（发生了什么/为什么/下一步——逐型真话）。
    pub fn copy(&self) -> (&'static str, &'static str, &'static str) {
        let c = self.cause.unwrap_or(ReadOnlyCause::PolicyNtfs);
        match c {
            ReadOnlyCause::PolicyNtfs => (
                "文件没有拷进去",
                "这是保护机制（NTFS 只读挂载）",
                ReadOnlyCause::PolicyNtfs.next_step(),
            ),
            ReadOnlyCause::VolumeError => (
                "写入失败",
                "卷有故障，系统已降级只读保护数据",
                ReadOnlyCause::VolumeError.next_step(),
            ),
            ReadOnlyCause::PhysicalLock => (
                "写入被拒绝",
                "介质处于物理写保护状态",
                ReadOnlyCause::PhysicalLock.next_step(),
            ),
        }
    }

    /// 帮助链（「了解 NTFS 只读策略」——策略型提示条必含）。
    pub fn help_link(&self) -> &'static [u8] {
        HELP_TARGET
    }

    /// 「拷到 VARIX 区」一键动作（被拒之后给台阶——落点=下载目录）。
    pub fn copy_to_varix_target(&self) -> &'static [u8] {
        COPY_FALLBACK_TARGET
    }
}

// ---------------------------------------------------------------------------
// 拖放落点裁决（目标区禁止光标的判定面）
// ---------------------------------------------------------------------------

/// 拖放落点裁决：只读卷=禁止光标+黄条；可写卷=放行。
pub fn drop_target_verdict(writable: bool) -> (bool, &'static str) {
    if writable {
        (true, "")
    } else {
        (false, "no-drop")
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
#[inline(never)]
pub fn run_romount_checks() -> CheckSet {
    let mut cs = CheckSet::new("F185-romount");

    // 1) 原因分型三通道：NTFS 策略/卷故障/物理锁各归各型（不笼统说只读）。
    let a = ReadOnlyCause::classify(true, false, false);
    let b = ReadOnlyCause::classify(false, true, false);
    let c = ReadOnlyCause::classify(false, false, true);
    cs.add(
        "cause_three_way_classification",
        a == Some(ReadOnlyCause::PolicyNtfs)
            && b == Some(ReadOnlyCause::VolumeError)
            && c == Some(ReadOnlyCause::PhysicalLock),
        "",
    );

    // 2) 可写卷无徽标（classify=None——不冤枉好卷）。
    cs.add("writable_volume_no_badge", ReadOnlyCause::classify(false, false, false).is_none(), "");

    // 3) 分型优先序：物理锁 > 卷故障 > 策略（硬约束最诚实优先）。
    let mixed = ReadOnlyCause::classify(true, true, true);
    cs.add("cause_priority_physical_first", mixed == Some(ReadOnlyCause::PhysicalLock), "");

    // 4) 三模板 tooltip 逐型真话（策略型=主册原文）。
    cs.add(
        "tooltip_templates",
        ReadOnlyCause::PolicyNtfs.tooltip() == "NTFS 只读挂载：保护 Windows 数据安全"
            && ReadOnlyCause::VolumeError.tooltip().contains("降级只读")
            && ReadOnlyCause::PhysicalLock.tooltip().contains("写保护开关"),
        "",
    );

    // 5) 「检查此卷」按钮只有错误型有（F120 联动——不滥用）。
    cs.add(
        "repair_button_only_volume_error",
        ReadOnlyCause::VolumeError.repair_button()
            && !ReadOnlyCause::PolicyNtfs.repair_button()
            && !ReadOnlyCause::PhysicalLock.repair_button(),
        "",
    );

    // 6) 徽标几何（12px 锁形 + 「只读」字 + 右侧 4px 间距）。
    cs.add(
        "badge_geometry",
        BADGE_SIZE_PX == 12 && BADGE_OFFSET_PX == 4 && ReadonlyBadge::SHAPE == "lock-12px" && ReadonlyBadge::LABEL == "只读",
        "",
    );

    // 7) 拖放受阻 → 黄条三要素（发生了什么/为什么/下一步——策略型逐字）。
    let mut banner = DropBanner::new();
    banner.on_write_denied(0, ReadOnlyCause::PolicyNtfs);
    let (what, why, next) = banner.copy();
    cs.add(
        "banner_three_elements",
        banner.visible && what == "文件没有拷进去" && why.contains("保护机制") && next.contains("VARIX 区"),
        "",
    );

    // 8) 提示条 4s 自动收（可点停：停驻后 10s 仍在）。
    let mut b2 = DropBanner::new();
    b2.on_write_denied(0, ReadOnlyCause::PolicyNtfs);
    b2.tick(3_999);
    let stays = b2.visible;
    b2.tick(4_000);
    let auto_dismissed = !b2.visible;
    let mut b3 = DropBanner::new();
    b3.on_write_denied(0, ReadOnlyCause::PolicyNtfs);
    b3.pin();
    b3.tick(60_000);
    cs.add(
        "banner_4s_auto_dismiss_pin",
        stays && auto_dismissed && b3.visible && BANNER_AUTO_DISMISS_MS == 4_000,
        "",
    );

    // 9) 帮助链通（「了解 NTFS 只读策略」→ F119 篇锚在提示条上）。
    cs.add(
        "help_link_present",
        b2.help_link() == b"help:F119/ntfs-readonly" || banner.help_link() == b"help:F119/ntfs-readonly",
        "",
    );

    // 10) 「拷到 VARIX 区」一键动作（落点=下载目录——被拒之后给台阶）。
    cs.add("copy_to_varix_fallback", banner.copy_to_varix_target() == b"downloads", "");

    // 11) 拖放落点裁决：只读=禁止光标、可写=放行。
    let (ro_ok, ro_cursor) = drop_target_verdict(false);
    let (rw_ok, rw_cursor) = drop_target_verdict(true);
    cs.add(
        "drop_verdict_cursor",
        !ro_ok && ro_cursor == "no-drop" && rw_ok && rw_cursor == "",
        "",
    );

    // 12) 错误型三要素含修复出路（「检查此卷」——不是「再试一次」敷衍）。
    let mut b4 = DropBanner::new();
    b4.on_write_denied(0, ReadOnlyCause::VolumeError);
    let (_, _, next4) = b4.copy();
    cs.add("volume_error_next_step_repair", next4.contains("检查此卷"), "");

    // 13) 手动关闭路径（Esc/点停钮——「取消」永远是安全出路）。
    let mut b5 = DropBanner::new();
    b5.on_write_denied(0, ReadOnlyCause::PhysicalLock);
    b5.dismiss();
    cs.add("banner_manual_dismiss", !b5.visible, "");

    cs
}

// ---------------------------------------------------------------------------
// 深化层（批次二）：挂载信息源 trait · 「拷到 VARIX 区」动作队列 ·
// 提示条替换语义 —— 主册【设计细节】「挂载信息存储栈既有（B-705 面）/
// 一键复制到下载目录」落地。
// ---------------------------------------------------------------------------

/// 挂载信息源（B-705 注入口实型——卷管理栈实现本 trait，本层只消费）。
pub trait MountSource {
    /// 卷只读与否 + 原因三通道（ntfs_ro / fs_error / hw_write_protected）。
    fn mount_flags(&self, drive: u8) -> (bool, bool, bool);
    /// 卷是否可写（只读面快捷判定）。
    fn writable(&self, drive: u8) -> bool {
        let (ntfs, err, hw) = self.mount_flags(drive);
        !(ntfs || err || hw)
    }
}

/// 台架样本源（测试与真实卷栈同一 trait——一处一事实）。
pub struct FakeMounts {
    pub flags: [(u8, bool, bool, bool); 4],
    pub n: usize,
}

impl FakeMounts {
    pub const fn new() -> FakeMounts {
        FakeMounts { flags: [(0, false, false, false); 4], n: 0 }
    }
    pub fn add(&mut self, drive: u8, ntfs: bool, err: bool, hw: bool) {
        if self.n < 4 {
            self.flags[self.n] = (drive, ntfs, err, hw);
            self.n += 1;
        }
    }
}

impl MountSource for FakeMounts {
    fn mount_flags(&self, drive: u8) -> (bool, bool, bool) {
        for (d, ntfs, err, hw) in self.flags[..self.n].iter() {
            if *d == drive {
                return (*ntfs, *err, *hw);
            }
        }
        (false, false, false)
    }
}

/// 「拷到 VARIX 区」动作条目（被拒之后给台阶——一键动作的队列面：
/// 源路径字面+目标锚，执行在调用方文件栈）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CopyFallbackJob {
    pub drive: u8,
    /// 源路径字节面（定长——被拒文件的卷上路径）。
    pub src: [u8; 64],
    pub src_len: usize,
    /// 目标锚恒 = downloads（COPY_FALLBACK_TARGET）。
    pub dst_is_downloads: bool,
}

impl CopyFallbackJob {
    pub const SRC_CAP: usize = 64;

    pub fn new(drive: u8, src: &[u8]) -> CopyFallbackJob {
        let l = src.len().min(Self::SRC_CAP);
        let mut buf = [0u8; Self::SRC_CAP];
        buf[..l].copy_from_slice(&src[..l]);
        CopyFallbackJob { drive, src: buf, src_len: l, dst_is_downloads: true }
    }
}

/// 动作队列（定长——用户连点不堆积无限：8 上限后拒并提示）。
pub const COPY_QUEUE_CAP: usize = 8;

pub struct CopyQueue {
    pub jobs: [Option<CopyFallbackJob>; COPY_QUEUE_CAP],
    pub n: usize,
    /// 满拒计数（诚实账——不静默丢）。
    pub rejected_full: u32,
}

impl CopyQueue {
    pub const fn new() -> CopyQueue {
        CopyQueue { jobs: [const { None }; COPY_QUEUE_CAP], n: 0, rejected_full: 0 }
    }

    pub fn enqueue(&mut self, job: CopyFallbackJob) -> bool {
        if self.n >= COPY_QUEUE_CAP {
            self.rejected_full += 1;
            return false;
        }
        self.jobs[self.n] = Some(job);
        self.n += 1;
        true
    }

    pub fn dequeue(&mut self) -> Option<CopyFallbackJob> {
        if self.n == 0 {
            return None;
        }
        let job = self.jobs[0];
        for i in 1..COPY_QUEUE_CAP {
            self.jobs[i - 1] = self.jobs[i];
        }
        self.jobs[COPY_QUEUE_CAP - 1] = None;
        self.n -= 1;
        job
    }
}

/// 深化自检（检查项对账层——主册【设计细节】子句逐项实算）。
#[inline(never)]
pub fn run_romount_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F185-deep");

    // 1) 挂载源 trait：NTFS 只读卷 → 策略型原因（B-705 注入口贯通）。
    let mut mounts = FakeMounts::new();
    mounts.add(b'W', true, false, false);
    mounts.add(b'C', false, false, false);
    cs.add(
        "mount_source_policy",
        ReadOnlyCause::classify_tuple(mounts.mount_flags(b'W')) == Some(ReadOnlyCause::PolicyNtfs),
        "",
    );

    // 2) 可写卷经 trait 快捷判定（writable 语义位——不冤枉好卷贯通）。
    cs.add("mount_source_writable", mounts.writable(b'C') && !mounts.writable(b'W'), "");

    // 3) 未知卷 → 全假通道（trait 默认面——不编造原因贯通）。
    cs.add("mount_source_unknown_honest", mounts.mount_flags(b'Z') == (false, false, false) && mounts.writable(b'Z'), "");

    // 4) 「拷到 VARIX 区」动作条目：源路径+目标锚（被拒之后给台阶）。
    let job = CopyFallbackJob::new(b'W', b"/doc/report.docx");
    cs.add(
        "copy_job_fields",
        job.drive == b'W' && job.src_len == 16 && job.dst_is_downloads && core::str::from_utf8(&job.src[..job.src_len]).unwrap() == "/doc/report.docx",
        "",
    );

    // 5) 动作队列进出序（FIFO——用户连点按序执行不乱序）。
    let mut q = CopyQueue::new();
    let j1 = CopyFallbackJob::new(b'W', b"/a.txt");
    let j2 = CopyFallbackJob::new(b'W', b"/b.txt");
    q.enqueue(j1);
    q.enqueue(j2);
    let d1 = q.dequeue().unwrap();
    let d2 = q.dequeue().unwrap();
    cs.add(
        "copy_queue_fifo",
        d1.src_len == 6 && core::str::from_utf8(&d1.src[..6]).unwrap() == "/a.txt" && d2.src_len == 6 && q.n == 0,
        "",
    );

    // 6) 队列满诚实拒（8 上限+拒计数——不静默丢）。
    let mut q2 = CopyQueue::new();
    for _ in 0..10 {
        q2.enqueue(CopyFallbackJob::new(b'W', b"/x"));
    }
    cs.add("copy_queue_cap", q2.n == COPY_QUEUE_CAP && q2.rejected_full == 2 && COPY_QUEUE_CAP == 8, "");

    // 7) 空队列出队 None（不 panic 不编造）。
    cs.add("copy_queue_empty_none", CopyQueue::new().dequeue().is_none(), "");

    // 8) 源路径超长截断（64 字节封顶——定长纪律）。
    let long = CopyFallbackJob::new(b'W', &[b'a'; 80]);
    cs.add("copy_job_src_cap", long.src_len == CopyFallbackJob::SRC_CAP && long.src_len == 64, "");

    // 9) 提示条替换语义（新拒替旧条——不堆叠成瀑布）。
    let mut b = DropBanner::new();
    b.on_write_denied(0, ReadOnlyCause::PolicyNtfs);
    b.on_write_denied(1_000, ReadOnlyCause::PhysicalLock);
    let (_, why, _) = b.copy();
    cs.add("banner_replaces_not_stacks", b.visible && why.contains("物理"), "");

    // 10) 帮助链与台阶钮并存（被拒界面两出路齐：了解为什么+马上能做什么）。
    cs.add(
        "two_ways_out",
        b.help_link() == HELP_TARGET && b.copy_to_varix_target() == COPY_FALLBACK_TARGET,
        "",
    );

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_lifecycle_full() {
        // 弹出→停驻→解除停驻→到期收起 全生命周期。
        let mut b = DropBanner::new();
        b.on_write_denied(1_000, ReadOnlyCause::PolicyNtfs);
        assert!(b.visible);
        b.pin();
        b.tick(9_000);
        assert!(b.visible, "停驻期不自动收");
        b.dismiss();
        assert!(!b.visible);
        // 重新弹出后不再停驻——4s 正常收。
        b.on_write_denied(10_000, ReadOnlyCause::PolicyNtfs);
        b.tick(14_000);
        assert!(!b.visible);
    }

    #[test]
    fn physical_lock_copy() {
        // 物理型三要素：发生了什么=被拒绝、为什么=物理写保护、下一步=关开关。
        let mut b = DropBanner::new();
        b.on_write_denied(0, ReadOnlyCause::PhysicalLock);
        let (what, why, next) = b.copy();
        assert!(what.contains("拒绝"));
        assert!(why.contains("物理"));
        assert!(next.contains("写保护开关"));
    }

    #[test]
    fn badge_const_sync() {
        // 徽标常量同源（OFFSET_PX 引用 BADGE_OFFSET_PX——一处一事实）。
        assert_eq!(ReadonlyBadge::OFFSET_PX, 4);
        assert_eq!(BADGE_SIZE_PX, 12);
    }

    #[test]
    fn classify_never_fabricates() {
        // 全假通道 → None（不编造原因——诚实分型）。
        assert!(ReadOnlyCause::classify(false, false, false).is_none());
    }
}
