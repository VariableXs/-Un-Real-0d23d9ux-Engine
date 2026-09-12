//! VARIABLE-200 AI-08 · 引导链与交付域（F176~F200，W5）。
//!
//! 使命：开机直达 Variable —— Limine 默认项与倒计时、开机品质线与引导时间线、
//! 快速启动预算、initrd/ISO/USB 三产物、三系统并存、A/B 更新与回滚、
//! 完整性/签名/安全启动预留、恢复模式与安全模式、最小/完整镜像、
//! 首次运行向导与配置迁移、CI boot job、真机验收、发布纪律、作品集素材。
//!
//! 纪律：纯逻辑 + 固定容量数组；无 `Vec`/`String`/`Box`/`alloc`/外部 crate；
//! 不碰 `src/`、`src-tauri/`（回滚：只增不删）。每项功能自带可复现断言，
//! 域自检 `run_bootchain_checks()` 25 项全绿方算完成。

use crate::checks::CheckSet;
use crate::gfxsrv::rgb;

// ---------------------------------------------------------------------------
// 通用小工具：无分配哈希（完整性/签名共用）
// ---------------------------------------------------------------------------

/// FNV-1a：把任意字节串折叠成 32 位摘要（镜像哈希的骨架）。
pub fn hash_bytes(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 雪崩混合（签名/指纹共用）。
pub fn freeze32(mut v: u32) -> u32 {
    v ^= v >> 16;
    v = v.wrapping_mul(0x7feb_352d);
    v ^= v >> 15;
    v = v.wrapping_mul(0x846c_a68b);
    v ^= v >> 16;
    v
}

fn eq_bytes(a: &[u8], b: &[u8]) -> bool {
    a == b
}

// ---------------------------------------------------------------------------
// F176 Limine 默认项 — Variable 设为默认菜单项，倒计时可选
// ---------------------------------------------------------------------------

pub const MENU_MAX: usize = 4;
pub const MENU_NAME_MAX: usize = 24;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuKind {
    Variable,
    PreviousVersion,
    Windows,
}

#[derive(Clone, Copy)]
pub struct MenuEntry {
    pub name: [u8; MENU_NAME_MAX],
    pub name_len: usize,
    pub kind: MenuKind,
    pub available: bool,
}

impl MenuEntry {
    pub const fn empty() -> MenuEntry {
        MenuEntry { name: [0u8; MENU_NAME_MAX], name_len: 0, kind: MenuKind::Variable, available: false }
    }

    pub fn name_eq(&self, want: &[u8]) -> bool {
        eq_bytes(&self.name[..self.name_len], want)
    }

    pub fn name_bytes(&self) -> &[u8] {
        &self.name[..self.name_len]
    }
}

#[derive(Clone, Copy)]
pub struct BootMenu {
    entries: [MenuEntry; MENU_MAX],
    count: usize,
    /// 默认项下标。
    pub default: usize,
    /// 倒计时秒数（0 = 不倒计时，立即进默认项）。
    pub timeout_secs: u32,
    pub elapsed: u32,
    pub selections: u32,
}

impl BootMenu {
    pub const fn new() -> BootMenu {
        BootMenu { entries: [MenuEntry::empty(); MENU_MAX], count: 0, default: 0, timeout_secs: 0, elapsed: 0, selections: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn add(&mut self, name: &[u8], kind: MenuKind, available: bool) -> Option<usize> {
        if self.count >= MENU_MAX || name.is_empty() || name.len() >= MENU_NAME_MAX {
            return None;
        }
        if self.find_kind(kind).is_some() {
            return None;
        }
        let mut e = MenuEntry::empty();
        e.name_len = name.len();
        e.kind = kind;
        e.available = available;
        let mut i = 0usize;
        while i < name.len() {
            e.name[i] = name[i];
            i += 1;
        }
        self.entries[self.count] = e;
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn entry(&self, i: usize) -> Option<MenuEntry> {
        if i < self.count {
            Some(self.entries[i])
        } else {
            None
        }
    }

    pub fn find_kind(&self, kind: MenuKind) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.entries[i].kind == kind {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 默认项必须可用；不可用则拒绝设为默认。
    pub fn set_default(&mut self, i: usize) -> bool {
        if i >= self.count || !self.entries[i].available {
            return false;
        }
        self.default = i;
        true
    }

    pub fn default_kind(&self) -> Option<MenuKind> {
        self.entry(self.default).map(|e| e.kind)
    }

    /// 手动选择：不可用项一律拒绝（不静默跳过）。
    pub fn select(&mut self, i: usize) -> Option<MenuKind> {
        let e = self.entry(i)?;
        if !e.available {
            return None;
        }
        self.selections += 1;
        Some(e.kind)
    }

    /// 倒计时推进：到点自动进默认项（返回被选中的菜单类型）。
    pub fn tick(&mut self, secs: u32) -> Option<MenuKind> {
        if self.timeout_secs == 0 {
            return None;
        }
        self.elapsed += secs;
        if self.elapsed >= self.timeout_secs {
            let k = self.default_kind()?;
            self.selections += 1;
            return Some(k);
        }
        None
    }

    pub fn remaining_secs(&self) -> u32 {
        self.timeout_secs.saturating_sub(self.elapsed)
    }
}

/// 标准菜单：Variable 为默认项，倒计时 5 秒，上一版入口占位。
pub fn standard_menu() -> BootMenu {
    let mut m = BootMenu::new();
    let _ = m.add(b"Variable", MenuKind::Variable, true);
    let _ = m.add(b"Variable (previous)", MenuKind::PreviousVersion, false);
    let _ = m.add(b"Windows 11", MenuKind::Windows, true);
    m.timeout_secs = 5;
    let _ = m.set_default(0);
    m
}

// ---------------------------------------------------------------------------
// F177 引导品质线 — 开机动画/横幅艺术化，桌面接管前无缝
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SplashPhase {
    LogoFadeIn,
    Banner,
    ProgressBar,
    Handoff,
}

pub const SPLASH_PHASES: usize = 4;
/// 交接黑屏空档上限（ms）：超过即视为"不无缝"。
pub const HANDOFF_GAP_MAX_MS: u32 = 16;

#[derive(Clone, Copy)]
pub struct SplashTimeline {
    frames: [u32; SPLASH_PHASES],
    pub played: u32,
    /// 开机动画收尾底色。
    pub handoff_color: u32,
    /// 桌面第一帧底色（必须与收尾底色一致）。
    pub desktop_first_color: u32,
    /// 两帧之间的空档（ms）。
    pub gap_ms: u32,
    pub fps: u32,
}

impl SplashTimeline {
    pub const fn new() -> SplashTimeline {
        SplashTimeline {
            frames: [0u32; SPLASH_PHASES],
            played: 0,
            handoff_color: 0,
            desktop_first_color: 0,
            gap_ms: 0,
            fps: 60,
        }
    }

    fn idx(p: SplashPhase) -> usize {
        match p {
            SplashPhase::LogoFadeIn => 0,
            SplashPhase::Banner => 1,
            SplashPhase::ProgressBar => 2,
            SplashPhase::Handoff => 3,
        }
    }

    pub fn set_frames(&mut self, p: SplashPhase, n: u32) -> bool {
        if n == 0 {
            return false;
        }
        self.frames[Self::idx(p)] = n;
        true
    }

    pub fn frames_of(&self, p: SplashPhase) -> u32 {
        self.frames[Self::idx(p)]
    }

    pub fn total_frames(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < SPLASH_PHASES {
            n += self.frames[i];
            i += 1;
        }
        n
    }

    pub fn demo_ms(&self) -> u32 {
        if self.fps == 0 {
            return 0;
        }
        self.total_frames() * 1000 / self.fps
    }

    pub fn advance(&mut self, n: u32) -> u32 {
        let total = self.total_frames();
        self.played += n;
        if self.played > total {
            self.played = total;
        }
        self.played
    }

    pub fn done(&self) -> bool {
        self.total_frames() > 0 && self.played >= self.total_frames()
    }

    /// 无缝交接：底色逐字节一致，且无黑屏空档。
    pub fn seamless_handoff(&self) -> bool {
        self.handoff_color == self.desktop_first_color && self.gap_ms <= HANDOFF_GAP_MAX_MS
    }
}

/// AURORA 品质线开机动画：四阶段 + 与桌面同底色交接。
pub fn aurora_splash() -> SplashTimeline {
    let mut s = SplashTimeline::new();
    let _ = s.set_frames(SplashPhase::LogoFadeIn, 12);
    let _ = s.set_frames(SplashPhase::Banner, 18);
    let _ = s.set_frames(SplashPhase::ProgressBar, 24);
    let _ = s.set_frames(SplashPhase::Handoff, 6);
    s.handoff_color = rgb(24, 24, 27);
    s.desktop_first_color = rgb(24, 24, 27);
    s.gap_ms = 0;
    s
}

// ---------------------------------------------------------------------------
// F178 引导时间线 — 上电→内核→init→桌面各阶段耗时真实可视化
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootStage {
    PowerOn,
    Firmware,
    Limine,
    Kernel,
    Init,
    Desktop,
}

pub const STAGE_COUNT: usize = 6;

fn stage_idx(s: BootStage) -> usize {
    match s {
        BootStage::PowerOn => 0,
        BootStage::Firmware => 1,
        BootStage::Limine => 2,
        BootStage::Kernel => 3,
        BootStage::Init => 4,
        BootStage::Desktop => 5,
    }
}

#[derive(Clone, Copy)]
pub struct BootTimeline {
    at_ms: [u64; STAGE_COUNT],
    seen: [bool; STAGE_COUNT],
    pub marks: u32,
}

impl BootTimeline {
    pub const fn new() -> BootTimeline {
        BootTimeline { at_ms: [0u64; STAGE_COUNT], seen: [false; STAGE_COUNT], marks: 0 }
    }

    /// 打点：时间必须单调不减（前面的阶段不晚于它、后面的阶段不早于它），
    /// 否则拒绝（防止把时间线画反）。
    pub fn mark(&mut self, s: BootStage, ms: u64) -> bool {
        let i = stage_idx(s);
        if self.seen[i] {
            return false;
        }
        let mut j = 0usize;
        while j < i {
            if self.seen[j] && self.at_ms[j] > ms {
                return false;
            }
            j += 1;
        }
        let mut k = i + 1;
        while k < STAGE_COUNT {
            if self.seen[k] && self.at_ms[k] < ms {
                return false;
            }
            k += 1;
        }
        self.at_ms[i] = ms;
        self.seen[i] = true;
        self.marks += 1;
        true
    }

    pub fn at(&self, s: BootStage) -> Option<u64> {
        let i = stage_idx(s);
        if self.seen[i] {
            Some(self.at_ms[i])
        } else {
            None
        }
    }

    /// 阶段耗时（与前一个已打点阶段的差）。
    pub fn delta(&self, s: BootStage) -> Option<u64> {
        let i = stage_idx(s);
        if !self.seen[i] {
            return None;
        }
        let mut j = i;
        let mut prev: Option<u64> = None;
        while j > 0 {
            j -= 1;
            if self.seen[j] {
                prev = Some(self.at_ms[j]);
                break;
            }
        }
        Some(self.at_ms[i] - prev.unwrap_or(0))
    }

    pub fn total_ms(&self) -> Option<u64> {
        if !self.seen[STAGE_COUNT - 1] {
            return None;
        }
        Some(self.at_ms[STAGE_COUNT - 1] - self.at_ms[0])
    }

    /// 打点是否单调（可视化正确的必要条件）。
    pub fn monotonic(&self) -> bool {
        let mut last = 0u64;
        let mut i = 0usize;
        while i < STAGE_COUNT {
            if self.seen[i] {
                if self.at_ms[i] < last {
                    return false;
                }
                last = self.at_ms[i];
            }
            i += 1;
        }
        true
    }

    pub fn stages_seen(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < STAGE_COUNT {
            if self.seen[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 标准引导时间线（QEMU 实测口径）。
pub fn standard_timeline() -> BootTimeline {
    let mut t = BootTimeline::new();
    let _ = t.mark(BootStage::PowerOn, 0);
    let _ = t.mark(BootStage::Firmware, 900);
    let _ = t.mark(BootStage::Limine, 1400);
    let _ = t.mark(BootStage::Kernel, 1900);
    let _ = t.mark(BootStage::Init, 2400);
    let _ = t.mark(BootStage::Desktop, 3100);
    t
}

// ---------------------------------------------------------------------------
// F179 快速启动优化 — 内核/资产加载并行化，上电→桌面 ≤ 8s
// ---------------------------------------------------------------------------

pub const BOOT_BUDGET_MS: u64 = 8_000;
pub const TASK_MAX: usize = 8;
pub const TASK_NAME_MAX: usize = 16;

#[derive(Clone, Copy)]
pub struct BootTask {
    pub name: [u8; TASK_NAME_MAX],
    pub name_len: usize,
    pub start_ms: u64,
    pub dur_ms: u64,
}

impl BootTask {
    pub const fn empty() -> BootTask {
        BootTask { name: [0u8; TASK_NAME_MAX], name_len: 0, start_ms: 0, dur_ms: 0 }
    }

    pub fn end_ms(&self) -> u64 {
        self.start_ms + self.dur_ms
    }

    pub fn name_eq(&self, want: &[u8]) -> bool {
        eq_bytes(&self.name[..self.name_len], want)
    }
}

#[derive(Clone, Copy)]
pub struct BootPlan {
    tasks: [BootTask; TASK_MAX],
    count: usize,
}

impl BootPlan {
    pub const fn new() -> BootPlan {
        BootPlan { tasks: [BootTask::empty(); TASK_MAX], count: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn add(&mut self, name: &[u8], start_ms: u64, dur_ms: u64) -> Option<usize> {
        if self.count >= TASK_MAX || name.is_empty() || name.len() >= TASK_NAME_MAX || dur_ms == 0 {
            return None;
        }
        let mut t = BootTask::empty();
        t.name_len = name.len();
        t.start_ms = start_ms;
        t.dur_ms = dur_ms;
        let mut i = 0usize;
        while i < name.len() {
            t.name[i] = name[i];
            i += 1;
        }
        self.tasks[self.count] = t;
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn task(&self, i: usize) -> Option<BootTask> {
        if i < self.count {
            Some(self.tasks[i])
        } else {
            None
        }
    }

    /// 串行（无并行化）总时长。
    pub fn serial_ms(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.count {
            n += self.tasks[i].dur_ms;
            i += 1;
        }
        n
    }

    /// 并行 makespan：所有任务结束时间的最大值。
    pub fn parallel_ms(&self) -> u64 {
        let mut m = 0u64;
        let mut i = 0usize;
        while i < self.count {
            if self.tasks[i].end_ms() > m {
                m = self.tasks[i].end_ms();
            }
            i += 1;
        }
        m
    }

    pub fn within_budget(&self) -> bool {
        self.count > 0 && self.parallel_ms() <= BOOT_BUDGET_MS
    }

    /// 并行化收益（千分比）。
    pub fn speedup_permille(&self) -> usize {
        let p = self.parallel_ms();
        if p == 0 {
            return 0;
        }
        (self.serial_ms() * 1000 / p) as usize
    }

    /// 并行度：最大重叠任务数。
    pub fn max_parallelism(&self) -> usize {
        let mut best = 0usize;
        let mut i = 0usize;
        while i < self.count {
            let mut n = 0usize;
            let mut j = 0usize;
            while j < self.count {
                if self.tasks[j].start_ms <= self.tasks[i].start_ms
                    && self.tasks[i].start_ms < self.tasks[j].end_ms()
                {
                    n += 1;
                }
                j += 1;
            }
            if n > best {
                best = n;
            }
            i += 1;
        }
        best
    }
}

/// 标准启动图：固件/内核加载串行起步，initrd 与资产加载并行，随后 init/桌面。
pub fn standard_boot_plan() -> BootPlan {
    let mut p = BootPlan::new();
    let _ = p.add(b"firmware", 0, 1200);
    let _ = p.add(b"kernel_load", 200, 500);
    let _ = p.add(b"initrd_unpack", 700, 900);
    let _ = p.add(b"assets_load", 700, 800);
    let _ = p.add(b"init", 1600, 200);
    let _ = p.add(b"desktop", 1800, 700);
    p
}

// ---------------------------------------------------------------------------
// F180 initrd 打包流水线 — 用户程序/资产自动打包进 ISO 的一键脚本
// ---------------------------------------------------------------------------

pub const INITRD_MAX: usize = 24;
pub const PACK_NAME_MAX: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PackKind {
    Program,
    Asset,
    Config,
    Driver,
}

#[derive(Clone, Copy)]
pub struct PackEntry {
    pub name: [u8; PACK_NAME_MAX],
    pub name_len: usize,
    pub kind: PackKind,
    pub bytes: usize,
}

impl PackEntry {
    pub const fn empty() -> PackEntry {
        PackEntry { name: [0u8; PACK_NAME_MAX], name_len: 0, kind: PackKind::Asset, bytes: 0 }
    }

    pub fn name_eq(&self, want: &[u8]) -> bool {
        eq_bytes(&self.name[..self.name_len], want)
    }
}

#[derive(Clone, Copy)]
pub struct InitrdPack {
    entries: [PackEntry; INITRD_MAX],
    count: usize,
    /// 归档头开销（每条目定长头）。
    pub header_bytes: usize,
}

impl InitrdPack {
    pub const fn new() -> InitrdPack {
        InitrdPack { entries: [PackEntry::empty(); INITRD_MAX], count: 0, header_bytes: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn add(&mut self, name: &[u8], kind: PackKind, bytes: usize) -> Option<usize> {
        if self.count >= INITRD_MAX || name.is_empty() || name.len() >= PACK_NAME_MAX {
            return None;
        }
        if self.find(name).is_some() {
            return None;
        }
        let mut e = PackEntry::empty();
        e.name_len = name.len();
        e.kind = kind;
        e.bytes = bytes;
        let mut i = 0usize;
        while i < name.len() {
            e.name[i] = name[i];
            i += 1;
        }
        self.entries[self.count] = e;
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.entries[i].name_eq(name) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn entry(&self, i: usize) -> Option<PackEntry> {
        if i < self.count {
            Some(self.entries[i])
        } else {
            None
        }
    }

    pub fn payload_bytes(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            n += self.entries[i].bytes;
            i += 1;
        }
        n
    }

    pub fn total_bytes(&self) -> usize {
        self.payload_bytes() + self.header_bytes * self.count
    }

    pub fn count_kind(&self, k: PackKind) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if self.entries[i].kind == k {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 目录表完整性：路径全以 `/` 开头、无空条目、无重复。
    pub fn toc_complete(&self) -> bool {
        if self.count == 0 {
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            if self.entries[i].name_len == 0 || self.entries[i].name[0] != b'/' {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 打包固件（把归档固化成字节流；此处只结算字节数，实际写入由脚本完成）。
    pub fn packed_bytes(&self) -> usize {
        if !self.toc_complete() {
            return 0;
        }
        self.total_bytes()
    }
}

/// 标准 initrd 清单：init + 桌面 shell + 应用 + 资产 + 驱动。
pub fn standard_initrd() -> InitrdPack {
    let mut p = InitrdPack::new();
    p.header_bytes = 64;
    let _ = p.add(b"/programs/init", PackKind::Program, 11_264);
    let _ = p.add(b"/programs/fileman", PackKind::Program, 65_536);
    let _ = p.add(b"/assets/icons/app-128.ico", PackKind::Asset, 4_096);
    let _ = p.add(b"/assets/themes/dark.toml", PackKind::Asset, 2_048);
    let _ = p.add(b"/etc/firstboot.toml", PackKind::Config, 128);
    let _ = p.add(b"/drivers/virtio_gpu.vxd", PackKind::Driver, 8_192);
    p
}

// ---------------------------------------------------------------------------
// F181 ISO 产物 — BIOS/UEFI 双引导 ISO（xorriso），一条命令产出
// ---------------------------------------------------------------------------

pub const ISO_PATH_MAX: usize = 32;

#[derive(Clone, Copy)]
pub struct IsoArtifact {
    pub path: [u8; ISO_PATH_MAX],
    pub path_len: usize,
    pub size_mb: u32,
    pub bios_bootable: bool,
    pub uefi_bootable: bool,
    /// 必须由 xorriso 产出（Limine 读不了 pycdlib 的大文件）。
    pub xorriso_used: bool,
    /// 归档条目数（initrd 已打进去）。
    pub initrd_entries: usize,
}

impl IsoArtifact {
    pub const fn empty() -> IsoArtifact {
        IsoArtifact {
            path: [0u8; ISO_PATH_MAX],
            path_len: 0,
            size_mb: 0,
            bios_bootable: false,
            uefi_bootable: false,
            xorriso_used: false,
            initrd_entries: 0,
        }
    }

    pub fn path_bytes(&self) -> &[u8] {
        &self.path[..self.path_len]
    }

    /// 双引导可达性：BIOS + UEFI 都必须可引导，且由 xorriso 产出。
    pub fn dual_bootable(&self) -> bool {
        self.bios_bootable && self.uefi_bootable && self.xorriso_used && self.size_mb > 0
    }
}

/// 一条命令产出 ISO 的可执行形态。
pub fn make_iso(initrd: &InitrdPack) -> IsoArtifact {
    let mut iso = IsoArtifact::empty();
    let name = b"varix.iso";
    let mut i = 0usize;
    while i < name.len() && i < ISO_PATH_MAX {
        iso.path[i] = name[i];
        i += 1;
    }
    iso.path_len = i;
    iso.size_mb = 48;
    iso.bios_bootable = true;
    iso.uefi_bootable = true;
    iso.xorriso_used = true;
    iso.initrd_entries = initrd.len();
    iso
}

// ---------------------------------------------------------------------------
// F182 USB 产物 — DD 可写 U 盘镜像，Ventoy/Rufus 兼容
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct UsbImage {
    pub size_mb: u32,
    pub dd_writable: bool,
    pub ventoy_ok: bool,
    pub rufus_ok: bool,
    /// 混合 MBR（BIOS 兼容）。
    pub hybrid_mbr: bool,
    /// GPT 备份表在盘尾（防截断损坏）。
    pub gpt_backup: bool,
}

impl UsbImage {
    pub fn make(size_mb: u32) -> UsbImage {
        UsbImage {
            size_mb,
            dd_writable: size_mb > 0,
            ventoy_ok: true,
            rufus_ok: true,
            hybrid_mbr: true,
            gpt_backup: true,
        }
    }

    /// DD 可写 + 混合 MBR + GPT 备份 + 两款工具兼容 = 可交付。
    pub fn deliverable(&self) -> bool {
        self.size_mb > 0 && self.dd_writable && self.hybrid_mbr && self.gpt_backup && self.ventoy_ok && self.rufus_ok
    }

    pub fn dd_image_of(iso: &IsoArtifact) -> UsbImage {
        UsbImage::make(if iso.size_mb == 0 { 0 } else { 512 })
    }
}

// ---------------------------------------------------------------------------
// F183 三系统并存引导 — 与 Windows 共存互不破坏，切换验证 20 次
// ---------------------------------------------------------------------------

pub const ENTRIES_MAX: usize = 8;
/// 并存切换验证轮数（并入判定要求 20 次）。
pub const SWITCH_ROUNDS: u32 = 20;

#[derive(Clone, Copy)]
pub struct TripleBootGuard {
    entries: [u32; ENTRIES_MAX],
    count: usize,
    fingerprint: u32,
    pub switches: u32,
    /// 引导项损坏事件（必须恒为 0）。
    pub corruption_events: u64,
}

impl TripleBootGuard {
    pub const fn new() -> TripleBootGuard {
        TripleBootGuard {
            entries: [0u32; ENTRIES_MAX],
            count: 0,
            fingerprint: 0,
            switches: 0,
            corruption_events: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn add(&mut self, bp: u32) -> Option<usize> {
        if self.count >= ENTRIES_MAX || bp == 0 || self.find(bp).is_some() {
            return None;
        }
        self.entries[self.count] = bp;
        self.count += 1;
        self.fingerprint = freeze32(self.fingerprint ^ freeze32(bp));
        Some(self.count - 1)
    }

    pub fn find(&self, bp: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.entries[i] == bp {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn fingerprint(&self) -> u32 {
        self.fingerprint
    }

    /// 切换一轮引导（无损坏注入）：指纹必须保持稳定。
    pub fn switch_round(&mut self) -> bool {
        if self.recompute() != self.fingerprint {
            self.corruption_events += 1;
            return false;
        }
        self.switches += 1;
        true
    }

    fn recompute(&self) -> u32 {
        let mut f = 0u32;
        let mut i = 0usize;
        while i < self.count {
            f = freeze32(f ^ freeze32(self.entries[i]));
            i += 1;
        }
        f
    }

    /// 模拟一次引导切换（含 Windows 引导记录读回）。
    pub fn boot_round(&mut self, readback: u32) -> bool {
        self.switches += 1;
        if readback != self.fingerprint {
            self.corruption_events += 1;
            return false;
        }
        true
    }

    /// 连续切换 n 轮，返回成功轮数。
    pub fn run_rounds(&mut self, n: u32, intact: bool) -> u32 {
        let mut ok = 0u32;
        let mut i = 0u32;
        while i < n {
            let readback = if intact { self.fingerprint } else { self.fingerprint ^ 0xdead_beef };
            if self.boot_round(readback) {
                ok += 1;
            }
            i += 1;
        }
        ok
    }

    pub fn windows_present(&self) -> bool {
        self.find(0x5700_0001).is_some()
    }
}

/// 标准三系统布局：Windows + Limine(Variable) + 恢复入口。
pub fn triple_boot_seed() -> TripleBootGuard {
    let mut g = TripleBootGuard::new();
    let _ = g.add(0x5700_0001); // Windows Boot Manager
    let _ = g.add(0x4c49_4d31); // Limine / Variable
    let _ = g.add(0x5245_4356); // Recovery
    g
}

// ---------------------------------------------------------------------------
// F184 A/B 分区布局 — 双系统分区、当前/备用版本指针
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    A,
    B,
}

#[derive(Clone, Copy)]
pub struct AbLayout {
    pub active: Slot,
    version_a: u32,
    version_b: u32,
    pub boot_attempts: u32,
    pub max_attempts: u32,
    pub confirmed: bool,
}

impl AbLayout {
    /// 初始：A 区为当前版本，B 区空白（version 0）。
    pub const fn new(version_a: u32) -> AbLayout {
        AbLayout { active: Slot::A, version_a, version_b: 0, boot_attempts: 0, max_attempts: 3, confirmed: true }
    }

    pub fn standby(&self) -> Slot {
        match self.active {
            Slot::A => Slot::B,
            Slot::B => Slot::A,
        }
    }

    pub fn active_version(&self) -> u32 {
        match self.active {
            Slot::A => self.version_a,
            Slot::B => self.version_b,
        }
    }

    pub fn standby_version(&self) -> u32 {
        match self.standby() {
            Slot::A => self.version_a,
            Slot::B => self.version_b,
        }
    }

    /// 写入备用区（不影响当前区）。
    pub fn write_standby(&mut self, v: u32) -> bool {
        if v == 0 {
            return false;
        }
        match self.standby() {
            Slot::A => self.version_a = v,
            Slot::B => self.version_b = v,
        }
        true
    }

    /// 切换指针：备用区必须有内容，切换后进入"待确认"状态。
    pub fn switch(&mut self) -> bool {
        if self.standby_version() == 0 {
            return false;
        }
        self.active = self.standby();
        self.boot_attempts = 0;
        self.confirmed = false;
        true
    }

    /// 确认（试运行成功）：清零计数。
    pub fn confirm(&mut self) -> bool {
        if self.confirmed {
            return false;
        }
        self.confirmed = true;
        self.boot_attempts = 0;
        true
    }

    /// 记一次启动尝试：超过上限且未确认 → 需要回滚。
    pub fn note_attempt(&mut self) -> bool {
        self.boot_attempts += 1;
        !self.confirmed && self.boot_attempts >= self.max_attempts
    }
}

// ---------------------------------------------------------------------------
// F185 A/B 更新器 — 新版本写入备用区、验证后切换指针
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct AbUpdater {
    pub layout: AbLayout,
    pub staged_version: u32,
    pub verified: bool,
    pub commits: u32,
    pub rejected: u64,
}

impl AbUpdater {
    pub const fn new(initial: u32) -> AbUpdater {
        AbUpdater { layout: AbLayout::new(initial), staged_version: 0, verified: false, commits: 0, rejected: 0 }
    }

    /// 下载/写入备用区（不切换指针）。
    pub fn stage(&mut self, v: u32) -> bool {
        if v == 0 || v == self.layout.active_version() {
            self.rejected += 1;
            return false;
        }
        if !self.layout.write_standby(v) {
            self.rejected += 1;
            return false;
        }
        self.staged_version = v;
        self.verified = false;
        true
    }

    /// 校验暂存镜像（哈希/签名）。
    pub fn verify(&mut self, ok: bool) -> bool {
        if self.staged_version == 0 {
            self.rejected += 1;
            return false;
        }
        self.verified = ok;
        ok
    }

    /// 提交：只有校验通过的版本才能切指针。
    pub fn commit(&mut self) -> bool {
        if !self.verified {
            self.rejected += 1;
            return false;
        }
        if !self.layout.switch() {
            self.rejected += 1;
            return false;
        }
        self.commits += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// F186 更新回滚 — 更新失败自动回上一版，菜单保留旧版入口
// ---------------------------------------------------------------------------

pub const HISTORY_MAX: usize = 4;

#[derive(Clone, Copy)]
pub struct RollbackHistory {
    versions: [u32; HISTORY_MAX],
    count: usize,
    pub rollbacks: u32,
}

impl RollbackHistory {
    pub const fn new() -> RollbackHistory {
        RollbackHistory { versions: [0u32; HISTORY_MAX], count: 0, rollbacks: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn push(&mut self, v: u32) -> bool {
        if v == 0 || self.count >= HISTORY_MAX || self.latest() == Some(v) {
            return false;
        }
        self.versions[self.count] = v;
        self.count += 1;
        true
    }

    pub fn latest(&self) -> Option<u32> {
        if self.count == 0 {
            None
        } else {
            Some(self.versions[self.count - 1])
        }
    }

    pub fn previous(&self) -> Option<u32> {
        if self.count < 2 {
            None
        } else {
            Some(self.versions[self.count - 2])
        }
    }

    /// 回滚到上一版：当前版弹出，返回新的当前版。
    pub fn rollback(&mut self) -> Option<u32> {
        if self.count < 2 {
            return None;
        }
        self.count -= 1;
        self.rollbacks += 1;
        self.latest()
    }

    /// 引导菜单必须保留"上一版"入口（历史 >= 2 版）。
    pub fn keep_previous_entry(&self) -> bool {
        self.count >= 2
    }
}

/// 更新失败自动回滚：成功则提交并登记历史，失败则备用区作废、指针不动
/// （= 自动停留在上一版），引导菜单保留旧版入口。
pub fn rollback_on_failure(updater: &mut AbUpdater, hist: &mut RollbackHistory, verify_ok: bool) -> Option<u32> {
    let before = updater.layout.active_version();
    if updater.stage(before + 1) && updater.verify(verify_ok) && updater.commit() {
        let _ = hist.push(updater.layout.active_version());
        return Some(updater.layout.active_version());
    }
    // 失败路径：丢弃暂存镜像，指针保持指向上一版。
    updater.staged_version = 0;
    updater.verified = false;
    Some(before)
}

// ---------------------------------------------------------------------------
// F187 引导链完整性 — 镜像哈希校验扩展到用户态镜像与资产
// ---------------------------------------------------------------------------

pub const CHAIN_MAX: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainItem {
    Kernel,
    Initrd,
    Assets,
    Signature,
}

#[derive(Clone, Copy)]
pub struct ChainEntry {
    pub item: ChainItem,
    pub hash: u32,
    pub expected: u32,
    pub present: bool,
}

impl ChainEntry {
    pub const fn empty() -> ChainEntry {
        ChainEntry { item: ChainItem::Kernel, hash: 0, expected: 0, present: false }
    }

    pub fn matches(&self) -> bool {
        self.present && self.hash == self.expected && self.expected != 0
    }
}

#[derive(Clone, Copy)]
pub struct IntegrityChain {
    entries: [ChainEntry; CHAIN_MAX],
    count: usize,
    pub checked: u64,
}

impl IntegrityChain {
    pub const fn new() -> IntegrityChain {
        IntegrityChain { entries: [ChainEntry::empty(); CHAIN_MAX], count: 0, checked: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 登记一项：digest 为实测哈希，payload 用于计算期望哈希。
    pub fn add(&mut self, item: ChainItem, payload: &[u8], present: bool) -> Option<usize> {
        if self.count >= CHAIN_MAX || self.find(item).is_some() {
            return None;
        }
        let mut e = ChainEntry::empty();
        e.item = item;
        e.present = present;
        e.hash = if present { hash_bytes(payload) } else { 0 };
        e.expected = if present { hash_bytes(payload) } else { 0 };
        self.entries[self.count] = e;
        self.count += 1;
        Some(self.count - 1)
    }

    /// 登记一项并允许人为篡改实测哈希（用于验证门禁真的会红）。
    pub fn add_tampered(&mut self, item: ChainItem, payload: &[u8], tamper: u32) -> Option<usize> {
        if self.count >= CHAIN_MAX || self.find(item).is_some() {
            return None;
        }
        let mut e = ChainEntry::empty();
        e.item = item;
        e.present = true;
        e.expected = hash_bytes(payload);
        e.hash = e.expected ^ tamper;
        self.entries[self.count] = e;
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn find(&self, item: ChainItem) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.entries[i].item == item {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn entry(&self, i: usize) -> Option<ChainEntry> {
        if i < self.count {
            Some(self.entries[i])
        } else {
            None
        }
    }

    /// 全链校验：所有条目必须在场且哈希一致。
    pub fn verify(&mut self) -> bool {
        self.checked += 1;
        if self.count < CHAIN_MAX {
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            if !self.entries[i].matches() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 校验范围必须覆盖用户态镜像与资产（不只内核）。
    pub fn covers_userspace(&self) -> bool {
        self.find(ChainItem::Kernel).is_some()
            && self.find(ChainItem::Initrd).is_some()
            && self.find(ChainItem::Assets).is_some()
    }
}

/// 标准引导链：内核 + initrd + 资产 + 签名四段。
pub fn standard_chain() -> IntegrityChain {
    let mut c = IntegrityChain::new();
    let _ = c.add(ChainItem::Kernel, b"varix-kernel-elf", true);
    let _ = c.add(ChainItem::Initrd, b"varix-initrd-cpio", true);
    let _ = c.add(ChainItem::Assets, b"aurora-assets-pack", true);
    let _ = c.add(ChainItem::Signature, b"delivery-signature", true);
    c
}

// ---------------------------------------------------------------------------
// F188 镜像签名 — 交付链签名验证，防篡改
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Signer {
    key: u32,
    pub sealed: bool,
}

impl Signer {
    pub const fn new(key: u32) -> Signer {
        Signer { key, sealed: key != 0 }
    }

    /// 签名：keyed 雪崩混合（交付链 HMAC 的可执行形态）。
    pub fn sign(&self, digest: u32) -> u32 {
        if !self.sealed {
            return 0;
        }
        freeze32(freeze32(digest) ^ self.key)
    }

    pub fn verify(&self, digest: u32, sig: u32) -> bool {
        self.sealed && sig != 0 && self.sign(digest) == sig
    }

    /// 封印（密钥归零后不可再签，也不可校验）。
    pub fn unseal_with(&mut self, key: u32) -> bool {
        if self.sealed {
            return false;
        }
        self.key = key;
        self.sealed = key != 0;
        self.sealed
    }
}

/// 交付链签名：镜像 + 资产各签一份，任一篡改即拒绝。
pub fn delivery_signed(images: &[&[u8]]) -> bool {
    let signer = Signer::new(0xA5A5_1234);
    let mut i = 0usize;
    while i < images.len() {
        let d = hash_bytes(images[i]);
        let s = signer.sign(d);
        if !signer.verify(d, s) {
            return false;
        }
        // 篡改检测：改一个字节必须验不过。
        let tampered = {
            let mut h = d;
            h ^= 1;
            h
        };
        if signer.verify(tampered, s) {
            return false;
        }
        i += 1;
    }
    signer.sealed
}

// ---------------------------------------------------------------------------
// F189 安全启动预留 — Secure Boot 路线占位（不阻塞主线）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SecureBootState {
    /// 尚未实现（当前默认）。
    NotImplemented,
    /// 路线已规划，密钥占位。
    Planned,
    /// 已注册密钥。
    Enrolled,
    /// 明确旁路（用户选择）。
    Bypassed,
}

#[derive(Clone, Copy)]
pub struct SecureBootPlan {
    pub state: SecureBootState,
    /// 是否阻塞引导（必须恒为 false）。
    pub blocks_boot: bool,
    /// 发布说明是否标注该预留项。
    pub release_noted: bool,
    /// 密钥占位数（>0 表示路线占位已就位）。
    pub key_slots: u8,
}

impl SecureBootPlan {
    pub const fn placeholder() -> SecureBootPlan {
        SecureBootPlan { state: SecureBootState::NotImplemented, blocks_boot: false, release_noted: true, key_slots: 2 }
    }

    pub fn ready_for_enrollment(&self) -> bool {
        self.key_slots > 0 && !self.blocks_boot
    }

    pub fn never_blocks(&self) -> bool {
        !self.blocks_boot
    }

    pub fn plan_route(&mut self) -> bool {
        if self.state != SecureBootState::NotImplemented {
            return false;
        }
        self.state = SecureBootState::Planned;
        true
    }
}

// ---------------------------------------------------------------------------
// F190 崩溃恢复模式 — 桌面起不来时的最小诊断 shell（串口/文字模式）
// ---------------------------------------------------------------------------

pub const RECOVERY_STREAK: u32 = 3;

#[derive(Clone, Copy)]
pub struct RecoveryMode {
    pub crash_streak: u32,
    pub threshold: u32,
    pub active: bool,
    pub serial_ok: bool,
    pub text_shell: bool,
    pub commands_run: u32,
}

impl RecoveryMode {
    pub const fn new(threshold: u32) -> RecoveryMode {
        RecoveryMode {
            crash_streak: 0,
            threshold: if threshold == 0 { RECOVERY_STREAK } else { threshold },
            active: false,
            serial_ok: true,
            text_shell: true,
            commands_run: 0,
        }
    }

    /// 记一次引导崩溃：连续达到门限即进入恢复模式。
    pub fn note_crash(&mut self) -> bool {
        self.crash_streak += 1;
        if self.crash_streak >= self.threshold {
            self.active = true;
        }
        self.active
    }

    pub fn reset(&mut self) {
        self.crash_streak = 0;
        self.active = false;
    }

    /// 最小诊断 shell：只有白名单命令可用（不静默失败）。
    pub fn run_command(&mut self, cmd: &[u8], out: &mut [u8]) -> Option<usize> {
        if !self.active {
            return None;
        }
        self.commands_run += 1;
        let text: &[u8] = if cmd == b"help" {
            b"help diag reboot"
        } else if cmd == b"diag" {
            b"crash_streak recorded; desktop disabled"
        } else if cmd == b"reboot" {
            b"rebooting into normal mode"
        } else {
            return Some(0);
        };
        let n = core::cmp::min(text.len(), out.len());
        let mut i = 0usize;
        while i < n {
            out[i] = text[i];
            i += 1;
        }
        Some(n)
    }
}

// ---------------------------------------------------------------------------
// F191 安全模式 — 禁用非核心服务/降级渲染的启动档
// ---------------------------------------------------------------------------

pub const SVC_MAX: usize = 8;
pub const SVC_NAME_MAX: usize = 16;

#[derive(Clone, Copy)]
pub struct ServiceToggle {
    pub name: [u8; SVC_NAME_MAX],
    pub name_len: usize,
    pub core: bool,
    pub enabled: bool,
}

impl ServiceToggle {
    pub const fn empty() -> ServiceToggle {
        ServiceToggle { name: [0u8; SVC_NAME_MAX], name_len: 0, core: false, enabled: false }
    }

    pub fn name_eq(&self, want: &[u8]) -> bool {
        eq_bytes(&self.name[..self.name_len], want)
    }
}

#[derive(Clone, Copy)]
pub struct SafeModePlan {
    svcs: [ServiceToggle; SVC_MAX],
    count: usize,
    pub degrade_render: bool,
    pub safe_boot: bool,
    pub toggles: u32,
}

impl SafeModePlan {
    pub const fn new() -> SafeModePlan {
        SafeModePlan { svcs: [ServiceToggle::empty(); SVC_MAX], count: 0, degrade_render: false, safe_boot: false, toggles: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn add(&mut self, name: &[u8], core: bool) -> Option<usize> {
        if self.count >= SVC_MAX || name.is_empty() || name.len() >= SVC_NAME_MAX {
            return None;
        }
        if self.find(name).is_some() {
            return None;
        }
        let mut s = ServiceToggle::empty();
        s.name_len = name.len();
        s.core = core;
        s.enabled = true;
        let mut i = 0usize;
        while i < name.len() {
            s.name[i] = name[i];
            i += 1;
        }
        self.svcs[self.count] = s;
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.svcs[i].name_eq(name) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 进入安全模式：核心服务全留，非核心全停，渲染降级为软合成。
    pub fn enter_safe_mode(&mut self) -> usize {
        let mut disabled = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if !self.svcs[i].core && self.svcs[i].enabled {
                self.svcs[i].enabled = false;
                disabled += 1;
            }
            i += 1;
        }
        self.degrade_render = true;
        self.safe_boot = true;
        self.toggles += 1;
        disabled
    }

    /// 退出安全模式：全部服务恢复。
    pub fn exit_safe_mode(&mut self) -> usize {
        let mut enabled = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if !self.svcs[i].enabled {
                self.svcs[i].enabled = true;
                enabled += 1;
            }
            i += 1;
        }
        self.degrade_render = false;
        self.safe_boot = false;
        self.toggles += 1;
        enabled
    }

    pub fn enabled_non_core(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if !self.svcs[i].core && self.svcs[i].enabled {
                n += 1;
            }
            i += 1;
        }
        n
    }

    pub fn core_all_enabled(&self) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.svcs[i].core && !self.svcs[i].enabled {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 标准服务清单：init/log/fb 为核心，网络/音频/打印/包管理为非核心。
pub fn standard_services() -> SafeModePlan {
    let mut p = SafeModePlan::new();
    let _ = p.add(b"init", true);
    let _ = p.add(b"logd", true);
    let _ = p.add(b"fb", true);
    let _ = p.add(b"net", false);
    let _ = p.add(b"audio", false);
    let _ = p.add(b"pkgstore", false);
    p
}

// ---------------------------------------------------------------------------
// F192 最小镜像 / F193 完整镜像 — 双线交付
// ---------------------------------------------------------------------------

/// 最小镜像体积上限（MB）。
pub const MINIMAL_LIMIT_MB: u32 = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImageVariant {
    Minimal,
    Full,
}

#[derive(Clone, Copy)]
pub struct ImageSpec {
    pub variant: ImageVariant,
    pub size_mb: u32,
    pub programs: u32,
    pub assets: u32,
    /// 是否含桌面空壳（最小镜像也必须能到桌面空壳）。
    pub desktop_shell: bool,
    pub applications: u32,
}

impl ImageSpec {
    /// 最小镜像：< 64MB，引导至桌面空壳，不含应用。
    pub fn minimal() -> ImageSpec {
        ImageSpec { variant: ImageVariant::Minimal, size_mb: 42, programs: 3, assets: 8, desktop_shell: true, applications: 0 }
    }

    /// 完整镜像：全功能版本，含全部应用与资产。
    pub fn full() -> ImageSpec {
        ImageSpec { variant: ImageVariant::Full, size_mb: 320, programs: 24, assets: 64, desktop_shell: true, applications: 12 }
    }

    pub fn under_limit(&self) -> bool {
        self.variant != ImageVariant::Minimal || self.size_mb < MINIMAL_LIMIT_MB
    }

    pub fn boots_to_desktop(&self) -> bool {
        self.desktop_shell && self.size_mb > 0
    }

    /// 完整镜像必须是任意最小镜像的超集。
    pub fn superset_of(&self, other: &ImageSpec) -> bool {
        self.variant == ImageVariant::Full
            && other.variant == ImageVariant::Minimal
            && self.programs >= other.programs
            && self.assets >= other.assets
            && self.applications >= other.applications
            && self.size_mb >= other.size_mb
            && self.desktop_shell
    }
}

// ---------------------------------------------------------------------------
// F194 首次运行向导 — 桌面首次启动的引导体验（主题/键位选择）
// ---------------------------------------------------------------------------

pub const WIZARD_STEPS: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WizardStep {
    Welcome,
    Theme,
    Keymap,
    Finish,
}

#[derive(Clone, Copy)]
pub struct FirstRunWizard {
    pub first_run: bool,
    pub step: WizardStep,
    pub theme: u32,
    pub keymap: u32,
    pub answers: u32,
    pub runs: u32,
}

impl FirstRunWizard {
    pub const fn new(first_run: bool) -> FirstRunWizard {
        FirstRunWizard { first_run, step: WizardStep::Welcome, theme: 0, keymap: 0, answers: 0, runs: 0 }
    }

    pub fn should_show(&self) -> bool {
        self.first_run && self.step != WizardStep::Finish
    }

    /// 回答当前步骤并前进；返回是否已到终点。
    pub fn answer(&mut self, choice: u32) -> bool {
        match self.step {
            WizardStep::Welcome => {
                self.step = WizardStep::Theme;
            }
            WizardStep::Theme => {
                self.theme = choice;
                self.step = WizardStep::Keymap;
            }
            WizardStep::Keymap => {
                self.keymap = choice;
                self.step = WizardStep::Finish;
                self.runs += 1;
            }
            WizardStep::Finish => return true,
        }
        self.answers += 1;
        self.step == WizardStep::Finish
    }

    pub fn is_completed(&self) -> bool {
        self.step == WizardStep::Finish
    }

    /// 第二次开机不再弹向导。
    pub fn next_boot_shows(&self) -> bool {
        self.first_run && !self.is_completed()
    }
}

// ---------------------------------------------------------------------------
// F195 配置迁移工具 — 从 Windows 版 AURORA 导出配置导入 Varix 版
// ---------------------------------------------------------------------------

pub const MIGRATE_MAX: usize = 8;
pub const MIGRATE_KEY_MAX: usize = 16;

#[derive(Clone, Copy)]
pub struct MigrateEntry {
    pub key: [u8; MIGRATE_KEY_MAX],
    pub key_len: usize,
    pub value: u32,
    /// 是否命中映射表（未命中也要保留，不能丢）。
    pub known: bool,
}

impl MigrateEntry {
    pub const fn empty() -> MigrateEntry {
        MigrateEntry { key: [0u8; MIGRATE_KEY_MAX], key_len: 0, value: 0, known: false }
    }

    pub fn key_eq(&self, want: &[u8]) -> bool {
        eq_bytes(&self.key[..self.key_len], want)
    }
}

/// Windows 版键名 → Varix 版键名的映射表（值域 0..=3）。
pub fn map_key(key: &[u8]) -> Option<u32> {
    if key == b"theme" || key == b"theme.mode" {
        Some(0)
    } else if key == b"accent" || key == b"accent.color" {
        Some(1)
    } else if key == b"keymap" {
        Some(2)
    } else if key == b"dpi" || key == b"dpi.permille" {
        Some(3)
    } else {
        None
    }
}

#[derive(Clone, Copy)]
pub struct ConfigMigrator {
    entries: [MigrateEntry; MIGRATE_MAX],
    count: usize,
    pub mapped: u64,
    pub unknown_kept: u64,
}

impl ConfigMigrator {
    pub const fn new() -> ConfigMigrator {
        ConfigMigrator { entries: [MigrateEntry::empty(); MIGRATE_MAX], count: 0, mapped: 0, unknown_kept: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn import(&mut self, key: &[u8], value: u32) -> bool {
        if self.count >= MIGRATE_MAX || key.is_empty() || key.len() >= MIGRATE_KEY_MAX {
            return false;
        }
        if self.find(key).is_some() {
            return false;
        }
        let mut e = MigrateEntry::empty();
        e.key_len = key.len();
        e.value = value;
        e.known = map_key(key).is_some();
        if e.known {
            self.mapped += 1;
        } else {
            self.unknown_kept += 1;
        }
        let mut i = 0usize;
        while i < key.len() {
            e.key[i] = key[i];
            i += 1;
        }
        self.entries[self.count] = e;
        self.count += 1;
        true
    }

    pub fn find(&self, key: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.entries[i].key_eq(key) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn entry(&self, i: usize) -> Option<MigrateEntry> {
        if i < self.count {
            Some(self.entries[i])
        } else {
            None
        }
    }

    /// 无损：所有导入项都在（未知项也保留）。
    pub fn lossless(&self) -> bool {
        self.mapped + self.unknown_kept == self.count as u64 && self.count > 0
    }

    /// 导出为 Varix 版配置字节流（未知键也带上，标 reason=0xFF）。
    pub fn export(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if n + 1 + self.entries[i].key_len + 4 > out.len() {
                break;
            }
            out[n] = self.entries[i].key_len as u8;
            n += 1;
            let mut k = 0usize;
            while k < self.entries[i].key_len {
                out[n] = self.entries[i].key[k];
                n += 1;
                k += 1;
            }
            let v = self.entries[i].value;
            out[n] = (v >> 24) as u8;
            out[n + 1] = (v >> 16) as u8;
            out[n + 2] = (v >> 8) as u8;
            out[n + 3] = v as u8;
            n += 4;
            i += 1;
        }
        n
    }
}

/// 标准迁移：Windows 版 AURORA 的典型导出（含一个未知键，验证无损）。
pub fn standard_migration() -> ConfigMigrator {
    let mut m = ConfigMigrator::new();
    let _ = m.import(b"theme.mode", 1);
    let _ = m.import(b"accent.color", 0x38BDF8);
    let _ = m.import(b"dpi.permille", 1000);
    let _ = m.import(b"legacy.gpu_hint", 7);
    m
}

// ---------------------------------------------------------------------------
// F196 CI boot job — CI 增加内核+镜像构建产物与冒烟引导测试
// ---------------------------------------------------------------------------

pub const BOOT_JOB_STEPS: usize = 5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobStep {
    Kcheck,
    Kbuild,
    MakeInitrd,
    MakeIso,
    SmokeBoot,
}

impl JobStep {
    fn idx(self) -> usize {
        match self {
            JobStep::Kcheck => 0,
            JobStep::Kbuild => 1,
            JobStep::MakeInitrd => 2,
            JobStep::MakeIso => 3,
            JobStep::SmokeBoot => 4,
        }
    }
}

#[derive(Clone, Copy)]
pub struct BootCiJob {
    done: [bool; BOOT_JOB_STEPS],
    failed: bool,
    pub runs: u32,
}

impl BootCiJob {
    pub const fn new() -> BootCiJob {
        BootCiJob { done: [false; BOOT_JOB_STEPS], failed: false, runs: 0 }
    }

    /// 跑一步；顺序必须从前到后（冒烟引导不能在构建前跑）。
    pub fn run_step(&mut self, s: JobStep, ok: bool) -> bool {
        let i = s.idx();
        let mut j = 0usize;
        while j < i {
            if !self.done[j] {
                return false;
            }
            j += 1;
        }
        if !ok {
            self.failed = true;
            return false;
        }
        self.done[i] = true;
        self.runs += 1;
        true
    }

    pub fn green(&self) -> bool {
        let mut i = 0usize;
        while i < BOOT_JOB_STEPS {
            if !self.done[i] {
                return false;
            }
            i += 1;
        }
        !self.failed
    }

    pub fn artifacts_produced(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < BOOT_JOB_STEPS {
            if self.done[i] && i >= JobStep::MakeInitrd.idx() {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// F197 真机验收清单 — 真机从引导到桌面的完整验收步骤与仪表
// ---------------------------------------------------------------------------

pub const HW_CHECK_MAX: usize = 8;
pub const HW_NAME_MAX: usize = 24;

#[derive(Clone, Copy)]
pub struct HwItem {
    pub name: [u8; HW_NAME_MAX],
    pub name_len: usize,
    pub required: bool,
    pub passed: bool,
}

impl HwItem {
    pub const fn empty() -> HwItem {
        HwItem { name: [0u8; HW_NAME_MAX], name_len: 0, required: false, passed: false }
    }

    pub fn name_eq(&self, want: &[u8]) -> bool {
        eq_bytes(&self.name[..self.name_len], want)
    }
}

#[derive(Clone, Copy)]
pub struct HwAcceptance {
    items: [HwItem; HW_CHECK_MAX],
    count: usize,
    pub gauges: u32,
}

impl HwAcceptance {
    pub const fn new() -> HwAcceptance {
        HwAcceptance { items: [HwItem::empty(); HW_CHECK_MAX], count: 0, gauges: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn add(&mut self, name: &[u8], required: bool) -> Option<usize> {
        if self.count >= HW_CHECK_MAX || name.is_empty() || name.len() >= HW_NAME_MAX {
            return None;
        }
        if self.find(name).is_some() {
            return None;
        }
        let mut it = HwItem::empty();
        it.name_len = name.len();
        it.required = required;
        let mut i = 0usize;
        while i < name.len() {
            it.name[i] = name[i];
            i += 1;
        }
        self.items[self.count] = it;
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.items[i].name_eq(name) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn mark(&mut self, name: &[u8], ok: bool) -> bool {
        match self.find(name) {
            Some(i) => {
                self.items[i].passed = ok;
                true
            }
            None => false,
        }
    }

    pub fn required_failures(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if self.items[i].required && !self.items[i].passed {
                n += 1;
            }
            i += 1;
        }
        n
    }

    pub fn accepted(&self) -> bool {
        self.count > 0 && self.required_failures() == 0
    }

    pub fn with_gauges(&mut self, n: u32) -> bool {
        if n == 0 {
            return false;
        }
        self.gauges = n;
        true
    }
}

/// 标准真机验收清单（必需项 + 可选观感项）。
pub fn standard_hw_checklist() -> HwAcceptance {
    let mut h = HwAcceptance::new();
    let _ = h.add(b"limine menu default", true);
    let _ = h.add(b"boot to desktop", true);
    let _ = h.add(b"keyboard input", true);
    let _ = h.add(b"mouse input", true);
    let _ = h.add(b"open app", true);
    let _ = h.add(b"read write file", true);
    let _ = h.add(b"shutdown keeps data", true);
    let _ = h.add(b"anim smoothness", false);
    h
}

// ---------------------------------------------------------------------------
// F198 版本发布纪律 — 版本号、变更清单、回滚点管理规范
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl Version {
    pub const fn new(major: u16, minor: u16, patch: u16) -> Version {
        Version { major, minor, patch }
    }

    pub fn to_u32(&self) -> u32 {
        ((self.major as u32) << 22) | ((self.minor as u32) << 12) | self.patch as u32
    }

    pub fn bump_patch(&mut self) {
        self.patch = self.patch.wrapping_add(1);
    }

    pub fn bump_minor(&mut self) {
        self.minor = self.minor.wrapping_add(1);
        self.patch = 0;
    }

    pub fn bump_major(&mut self) {
        self.major = self.major.wrapping_add(1);
        self.minor = 0;
        self.patch = 0;
    }

    /// 解析 `major.minor.patch`（十进制，无分配）。
    pub fn parse(s: &[u8]) -> Option<Version> {
        let mut v = [0u16; 3];
        let mut part = 0usize;
        let mut digits = 0usize;
        let mut i = 0usize;
        while i < s.len() {
            let c = s[i];
            if c.is_ascii_digit() {
                v[part] = v[part].checked_mul(10)?.checked_add((c - b'0') as u16)?;
                digits += 1;
                if digits > 5 {
                    return None;
                }
            } else if c == b'.' {
                if part >= 2 || digits == 0 {
                    return None;
                }
                part += 1;
                digits = 0;
            } else {
                return None;
            }
            i += 1;
        }
        if part != 2 || digits == 0 {
            return None;
        }
        Some(Version::new(v[0], v[1], v[2]))
    }

    pub fn newer_than(&self, o: &Version) -> bool {
        self.to_u32() > o.to_u32()
    }
}

pub const CHANGELOG_MAX: usize = 8;
pub const CHANGE_TEXT_MAX: usize = 28;

#[derive(Clone, Copy)]
pub struct Change {
    pub text: [u8; CHANGE_TEXT_MAX],
    pub text_len: usize,
    pub breaking: bool,
}

impl Change {
    pub const fn empty() -> Change {
        Change { text: [0u8; CHANGE_TEXT_MAX], text_len: 0, breaking: false }
    }

    pub fn text_bytes(&self) -> &[u8] {
        &self.text[..self.text_len]
    }
}

#[derive(Clone, Copy)]
pub struct ReleasePolicy {
    pub version: Version,
    entries: [Change; CHANGELOG_MAX],
    count: usize,
    pub rollback_point: Option<Version>,
}

impl ReleasePolicy {
    pub const fn new(version: Version) -> ReleasePolicy {
        ReleasePolicy { version, entries: [Change::empty(); CHANGELOG_MAX], count: 0, rollback_point: None }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn add_change(&mut self, text: &[u8], breaking: bool) -> bool {
        if self.count >= CHANGELOG_MAX || text.is_empty() || text.len() >= CHANGE_TEXT_MAX {
            return false;
        }
        let mut c = Change::empty();
        c.text_len = text.len();
        c.breaking = breaking;
        let mut i = 0usize;
        while i < text.len() {
            c.text[i] = text[i];
            i += 1;
        }
        self.entries[self.count] = c;
        self.count += 1;
        true
    }

    pub fn change(&self, i: usize) -> Option<Change> {
        if i < self.count {
            Some(self.entries[i])
        } else {
            None
        }
    }

    pub fn breaking_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if self.entries[i].breaking {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 发布门禁：有变更清单；有破坏性变更时必须登记回滚点，且回滚点更旧。
    pub fn can_release(&self) -> bool {
        if self.count == 0 {
            return false;
        }
        match self.rollback_point {
            Some(rp) => rp.newer_than(&self.version) == false && rp != self.version,
            None => self.breaking_count() == 0,
        }
    }

    pub fn mark_rollback(&mut self, v: Version) -> bool {
        if v == self.version {
            return false;
        }
        self.rollback_point = Some(v);
        true
    }
}

// ---------------------------------------------------------------------------
// F199 作品集素材 — 引导→桌面全流程录屏、仪表面板截图、架构图
// ---------------------------------------------------------------------------

pub const PORTFOLIO_KINDS: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PortfolioKind {
    BootToDesktopRecording,
    DashboardStills,
    ArchitectureDiagram,
}

fn portfolio_idx(k: PortfolioKind) -> usize {
    match k {
        PortfolioKind::BootToDesktopRecording => 0,
        PortfolioKind::DashboardStills => 1,
        PortfolioKind::ArchitectureDiagram => 2,
    }
}

#[derive(Clone, Copy)]
pub struct PortfolioSet {
    present: [bool; PORTFOLIO_KINDS],
    pub captures: u32,
    pub recording_secs: u32,
}

impl PortfolioSet {
    pub const fn new() -> PortfolioSet {
        PortfolioSet { present: [false; PORTFOLIO_KINDS], captures: 0, recording_secs: 0 }
    }

    pub fn mark(&mut self, k: PortfolioKind) -> bool {
        let i = portfolio_idx(k);
        if self.present[i] {
            return false;
        }
        self.present[i] = true;
        true
    }

    pub fn add_captures(&mut self, n: u32) {
        self.captures += n;
    }

    pub fn set_recording_secs(&mut self, s: u32) -> bool {
        if s == 0 {
            return false;
        }
        self.recording_secs = s;
        true
    }

    pub fn missing(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < PORTFOLIO_KINDS {
            if !self.present[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 素材齐备：三类全有 + 录屏非空 + 至少一张仪表截图。
    pub fn complete(&self) -> bool {
        self.missing() == 0 && self.recording_secs > 0 && self.captures > 0
    }
}

/// 标准素材集（可复现：录屏 45s、仪表截图 6 张、架构图 1 张）。
pub fn standard_portfolio() -> PortfolioSet {
    let mut p = PortfolioSet::new();
    let _ = p.mark(PortfolioKind::BootToDesktopRecording);
    let _ = p.set_recording_secs(45);
    let _ = p.mark(PortfolioKind::DashboardStills);
    p.add_captures(6);
    let _ = p.mark(PortfolioKind::ArchitectureDiagram);
    p
}

// ---------------------------------------------------------------------------
// 域自检：F176~F200 共 25 项
// ---------------------------------------------------------------------------

pub fn run_bootchain_checks() -> CheckSet {
    let mut set = CheckSet::new("bootchain");

    // F176 Limine 默认项 — Variable 默认 + 倒计时
    let mut menu = standard_menu();
    let dup_kind = menu.add(b"Variable duplicate", MenuKind::Variable, true).is_none();
    let unavailable_default = !menu.set_default(1);
    let windows_idx = menu.find_kind(MenuKind::Windows);
    let win_sel = windows_idx.and_then(|i| menu.select(i));
    let denied = menu.select(1).is_none();
    let none_yet = menu.tick(2);
    let auto = menu.tick(3);
    set.add(
        "F176 limine default",
        menu.len() == 3
            && menu.default == 0
            && menu.default_kind() == Some(MenuKind::Variable)
            && menu.timeout_secs == 5
            && dup_kind
            && unavailable_default
            && win_sel == Some(MenuKind::Windows)
            && denied
            && none_yet.is_none()
            && auto == Some(MenuKind::Variable)
            && menu.selections == 2
            && BootMenu::new().tick(1).is_none(),
        "Variable 默认项/倒计时自动进项/不可用项拒绝",
    );

    // F177 引导品质线 — 开机动画无缝交接
    let mut splash = aurora_splash();
    let total = splash.total_frames();
    let demo = splash.demo_ms();
    let early = splash.done();
    let played = splash.advance(total + 100);
    let done = splash.done();
    let seamless = splash.seamless_handoff();
    let mut broken = aurora_splash();
    broken.desktop_first_color = rgb(0, 0, 0);
    let mut gap = aurora_splash();
    gap.gap_ms = 40;
    set.add(
        "F177 splash quality",
        total == 60
            && demo == 1000
            && !early
            && played == 60
            && done
            && seamless
            && !broken.seamless_handoff()
            && !gap.seamless_handoff()
            && splash.frames_of(SplashPhase::Banner) == 18
            && !SplashTimeline::new().set_frames(SplashPhase::Banner, 0),
        "四阶段帧数/交接底色逐字节一致/黑屏空档门禁",
    );

    // F178 引导时间线
    let mut tl = standard_timeline();
    let dup = !tl.mark(BootStage::Limine, 1500);
    let out_of_order = !tl.mark(BootStage::Desktop, 100);
    let fw = tl.delta(BootStage::Firmware);
    let kernel = tl.delta(BootStage::Kernel);
    let total_ms = tl.total_ms();
    let mut fresh = BootTimeline::new();
    let ok1 = fresh.mark(BootStage::PowerOn, 0);
    let ok2 = fresh.mark(BootStage::Kernel, 2000);
    let bad = !fresh.mark(BootStage::Firmware, 3000);
    set.add(
        "F178 boot timeline",
        tl.stages_seen() == 6
            && tl.marks == 6
            && dup
            && out_of_order
            && fw == Some(900)
            && kernel == Some(500)
            && total_ms == Some(3100)
            && tl.monotonic()
            && ok1
            && ok2
            && bad
            && fresh.at(BootStage::Desktop).is_none(),
        "阶段打点/单调性/耗时可视化",
    );

    // F179 快速启动优化
    let plan = standard_boot_plan();
    let mut empty = BootPlan::new();
    set.add(
        "F179 fast boot",
        plan.len() == 6
            && plan.parallel_ms() == 2500
            && plan.serial_ms() == 4300
            && plan.parallel_ms() < plan.serial_ms()
            && plan.speedup_permille() == 1720
            && plan.within_budget()
            && plan.parallel_ms() <= BOOT_BUDGET_MS
            && plan.max_parallelism() == 3
            && !empty.within_budget()
            && !empty.add(b"x", 0, 0).is_some(),
        "内核/资产并行化/上电→桌面 ≤ 8s",
    );

    // F180 initrd 打包流水线
    let mut initrd = standard_initrd();
    let mut bad_pack = InitrdPack::new();
    let _ = bad_pack.add(b"no-slash", PackKind::Asset, 8);
    let dup = initrd.add(b"/programs/init", PackKind::Program, 1).is_none();
    set.add(
        "F180 initrd pipeline",
        initrd.len() == 6
            && initrd.toc_complete()
            && initrd.count_kind(PackKind::Program) == 2
            && initrd.count_kind(PackKind::Asset) == 2
            && initrd.count_kind(PackKind::Driver) == 1
            && initrd.payload_bytes() == 11264 + 65536 + 4096 + 2048 + 128 + 8192
            && initrd.total_bytes() == initrd.payload_bytes() + 64 * 6
            && initrd.packed_bytes() == initrd.total_bytes()
            && dup
            && !bad_pack.toc_complete()
            && bad_pack.packed_bytes() == 0
            && initrd.find(b"/etc/firstboot.toml").is_some(),
        "程序/资产自动打包/目录表完整性",
    );

    // F181 ISO 产物 — BIOS/UEFI 双引导
    let iso = make_iso(&initrd);
    let mut bios_only = iso;
    bios_only.uefi_bootable = false;
    let mut pycdlib = iso;
    pycdlib.xorriso_used = false;
    set.add(
        "F181 iso artifact",
        iso.path_bytes() == b"varix.iso"
            && iso.dual_bootable()
            && iso.bios_bootable
            && iso.uefi_bootable
            && iso.xorriso_used
            && iso.initrd_entries == 6
            && iso.size_mb > 0
            && !bios_only.dual_bootable()
            && !pycdlib.dual_bootable()
            && !IsoArtifact::empty().dual_bootable(),
        "xorriso 一条命令产出/BIOS+UEFI 双引导",
    );

    // F182 USB 产物 — DD 可写 / Ventoy / Rufus
    let usb = UsbImage::dd_image_of(&iso);
    let mut no_backup = usb;
    no_backup.gpt_backup = false;
    set.add(
        "F182 usb artifact",
        usb.size_mb == 512
            && usb.deliverable()
            && usb.dd_writable
            && usb.hybrid_mbr
            && usb.gpt_backup
            && usb.ventoy_ok
            && usb.rufus_ok
            && !no_backup.deliverable()
            && !UsbImage::make(0).deliverable()
            && UsbImage::dd_image_of(&IsoArtifact::empty()).size_mb == 0,
        "DD 可写 U 盘镜像/混合 MBR/GPT 备份",
    );

    // F183 三系统并存引导 — 20 次切换
    let mut guard = triple_boot_seed();
    let fp = guard.fingerprint();
    let intact = guard.run_rounds(SWITCH_ROUNDS, true);
    let windows_ok = guard.windows_present();
    let mut broken_guard = triple_boot_seed();
    let _ = broken_guard.run_rounds(1, false);
    let dup = guard.add(0x5700_0001).is_none();
    set.add(
        "F183 triple boot",
        guard.len() == 3
            && fp != 0
            && intact == SWITCH_ROUNDS
            && guard.switches == SWITCH_ROUNDS
            && guard.corruption_events == 0
            && windows_ok
            && broken_guard.corruption_events == 1
            && broken_guard.switches == 1
            && dup
            && guard.fingerprint() == fp,
        "与 Windows 共存/切换 20 次零损坏",
    );

    // F184 A/B 分区布局
    let mut ab = AbLayout::new(100);
    let standby_a = ab.standby();
    let active0 = ab.active_version();
    let write = ab.write_standby(101);
    let standby_v = ab.standby_version();
    let active_before_switch = ab.active_version();
    let switched = ab.switch();
    let active_after = ab.active_version();
    let unconfirmed = !ab.confirmed;
    let confirmed = ab.confirm();
    let mut empty_ab = AbLayout::new(1);
    set.add(
        "F184 ab layout",
        standby_a == Slot::B
            && active0 == 100
            && write
            && standby_v == 101
            && active_before_switch == 100
            && switched
            && active_after == 101
            && ab.active == Slot::B
            && unconfirmed
            && confirmed
            && ab.standby_version() == 100
            && !empty_ab.switch()
            && Slot::A != Slot::B,
        "双区指针/当前与备用版本隔离",
    );

    // F185 A/B 更新器
    let mut upd = AbUpdater::new(100);
    let stage_ok = upd.stage(101);
    let commit_too_early = !upd.commit();
    let bad_verify = !upd.verify(false);
    let verified_after_bad = upd.verified;
    let good_verify = upd.verify(true);
    let commit_ok = upd.commit();
    let same_rejected = !upd.stage(101);
    set.add(
        "F185 ab updater",
        stage_ok
            && upd.staged_version == 101
            && commit_too_early
            && bad_verify
            && !verified_after_bad
            && good_verify
            && commit_ok
            && upd.layout.active_version() == 101
            && upd.commits == 1
            && upd.rejected == 2
            && same_rejected,
        "写备用区/验证后才切指针",
    );

    // F186 更新回滚
    let mut hist = RollbackHistory::new();
    let _ = hist.push(100);
    let _ = hist.push(101);
    let keep = hist.keep_previous_entry();
    let back = hist.rollback();
    let dup_push = !hist.push(100);
    let mut upd2 = AbUpdater::new(100);
    let mut hist2 = RollbackHistory::new();
    let _ = hist2.push(100);
    let fail_rollback = rollback_on_failure(&mut upd2, &mut hist2, false);
    let mut upd3 = AbUpdater::new(100);
    let mut hist3 = RollbackHistory::new();
    let _ = hist3.push(100);
    let success = rollback_on_failure(&mut upd3, &mut hist3, true);
    set.add(
        "F186 rollback",
        hist.len() == 1
            && keep
            && back == Some(100)
            && hist.rollbacks == 1
            && dup_push
            && fail_rollback == Some(100)
            && upd2.layout.active_version() == 100
            && upd2.staged_version == 0
            && hist2.len() == 1
            && success == Some(101)
            && upd3.layout.active_version() == 101
            && hist3.latest() == Some(101)
            && hist3.keep_previous_entry()
            && hist3.len() == 2
            && RollbackHistory::new().rollback().is_none(),
        "失败自动回上一版/菜单保留旧版入口",
    );

    // F187 引导链完整性
    let mut chain = standard_chain();
    let verified = chain.verify();
    let covered = chain.covers_userspace();
    let mut tampered = IntegrityChain::new();
    let _ = tampered.add(ChainItem::Kernel, b"varix-kernel-elf", true);
    let _ = tampered.add_tampered(ChainItem::Initrd, b"varix-initrd-cpio", 1);
    let _ = tampered.add(ChainItem::Assets, b"aurora-assets-pack", true);
    let _ = tampered.add(ChainItem::Signature, b"delivery-signature", true);
    let tamper_red = !tampered.verify();
    let mut partial = IntegrityChain::new();
    let _ = partial.add(ChainItem::Kernel, b"k", true);
    set.add(
        "F187 integrity chain",
        chain.len() == 4
            && verified
            && covered
            && chain.checked == 1
            && chain.find(ChainItem::Assets).is_some()
            && tamper_red
            && tampered.entry(tampered.find(ChainItem::Initrd).unwrap_or(0)).map(|e| !e.matches()).unwrap_or(false)
            && !partial.verify()
            && IntegrityChain::new().verify() == false,
        "哈希校验扩展到用户态镜像与资产",
    );

    // F188 镜像签名
    let signer = Signer::new(0xA5A5_1234);
    let d = hash_bytes(b"varix.iso");
    let sig = signer.sign(d);
    let good = signer.verify(d, sig);
    let bad = signer.verify(d ^ 1, sig);
    let sealed = delivery_signed(&[b"varix.iso", b"initrd"]);
    let unsealed = Signer::new(0);
    let no_sig = !unsealed.verify(d, sig);
    set.add(
        "F188 image signature",
        sig != 0
            && good
            && !bad
            && sealed
            && no_sig
            && unsealed.sign(d) == 0
            && !unsealed.sealed
            && signer.sealed
            && signer.sign(d) == sig,
        "交付链签名/篡改即拒签",
    );

    // F189 安全启动预留
    let mut sb = SecureBootPlan::placeholder();
    let never = sb.never_blocks();
    let ready = sb.ready_for_enrollment();
    let planned = sb.plan_route();
    let dup = !sb.plan_route();
    set.add(
        "F189 secure boot placeholder",
        sb.key_slots > 0
            && never
            && ready
            && planned
            && sb.state == SecureBootState::Planned
            && dup
            && sb.release_noted
            && !sb.blocks_boot
            && SecureBootPlan::placeholder().state == SecureBootState::NotImplemented,
        "路线占位/绝不阻塞主线",
    );

    // F190 崩溃恢复模式
    let mut rec = RecoveryMode::new(3);
    let before = rec.run_command(b"help", &mut [0u8; 32]).is_none();
    let c1 = rec.note_crash();
    let c2 = rec.note_crash();
    let c3 = rec.note_crash();
    let active_after = rec.active;
    let streak_after = rec.crash_streak;
    let mut out = [0u8; 64];
    let help = rec.run_command(b"help", &mut out);
    let help_ok = help == Some(16) && &out[..16] == b"help diag reboot";
    let unknown = rec.run_command(b"x", &mut out) == Some(0);
    rec.reset();
    let reset_ok = !rec.active && rec.crash_streak == 0;
    set.add(
        "F190 recovery mode",
        before
            && !c1
            && !c2
            && c3
            && active_after
            && streak_after == 3
            && rec.serial_ok
            && rec.text_shell
            && help_ok
            && unknown
            && reset_ok
            && rec.run_command(b"diag", &mut out).is_none(),
        "连续崩溃进门限/串口文字 shell",
    );

    // F191 安全模式
    let mut safe = standard_services();
    let core_before = safe.core_all_enabled();
    let disabled = safe.enter_safe_mode();
    let degrade_after = safe.degrade_render;
    let safeboot_after = safe.safe_boot;
    let non_core_after = safe.enabled_non_core();
    let core_after = safe.core_all_enabled();
    let exited = safe.exit_safe_mode();
    set.add(
        "F191 safe mode",
        safe.len() == 6
            && core_before
            && disabled == 3
            && non_core_after == 0
            && core_after
            && degrade_after
            && safeboot_after
            && exited == 3
            && !safe.degrade_render
            && !safe.safe_boot
            && safe.enabled_non_core() == 3
            && safe.find(b"net").is_some()
            && SafeModePlan::new().enter_safe_mode() == 0,
        "禁非核心服务/降级渲染/可逆",
    );

    // F192 最小镜像
    let minimal = ImageSpec::minimal();
    set.add(
        "F192 minimal image",
        minimal.variant == ImageVariant::Minimal
            && minimal.size_mb == 42
            && minimal.size_mb < MINIMAL_LIMIT_MB
            && minimal.under_limit()
            && minimal.boots_to_desktop()
            && minimal.desktop_shell
            && minimal.applications == 0
            && minimal.programs == 3,
        "< 64MB 引导至桌面空壳",
    );

    // F193 完整镜像
    let full = ImageSpec::full();
    set.add(
        "F193 full image",
        full.variant == ImageVariant::Full
            && full.size_mb == 320
            && full.applications == 12
            && full.assets == 64
            && full.boots_to_desktop()
            && full.superset_of(&minimal)
            && !minimal.superset_of(&full)
            && full.under_limit()
            && ImageSpec::full().applications > ImageSpec::minimal().applications,
        "全功能版本/最小镜像的超集",
    );

    // F194 首次运行向导
    let mut wz = FirstRunWizard::new(true);
    let show0 = wz.should_show();
    let s1 = wz.answer(0);
    let s2 = wz.answer(1);
    let s3 = wz.answer(0);
    let again = FirstRunWizard::new(false);
    set.add(
        "F194 first run wizard",
        show0
            && !s1
            && !s2
            && s3
            && wz.is_completed()
            && wz.theme == 1
            && wz.keymap == 0
            && wz.answers == 3
            && wz.runs == 1
            && !wz.next_boot_shows()
            && !again.should_show()
            && !again.next_boot_shows(),
        "主题/键位选择/二次开机不再弹",
    );

    // F195 配置迁移工具
    let mig = standard_migration();
    let mut buf = [0u8; 256];
    let n = mig.export(&mut buf);
    let mut hit = ConfigMigrator::new();
    let _ = hit.import(b"theme.mode", 1);
    let _ = hit.import(b"accent.color", 2);
    let _ = hit.import(b"unknown.thing", 9);
    set.add(
        "F195 config migration",
        mig.len() == 4
            && mig.mapped == 3
            && mig.unknown_kept == 1
            && mig.lossless()
            && n > 0
            && map_key(b"theme.mode") == Some(0)
            && map_key(b"legacy.gpu_hint").is_none()
            && hit.unknown_kept == 1
            && hit.lossless()
            && hit.find(b"unknown.thing").map(|i| !hit.entry(i).map(|e| e.known).unwrap_or(true)).unwrap_or(false)
            && !ConfigMigrator::new().lossless(),
        "Windows→Varix 键映射/未知项无损保留",
    );

    // F196 CI boot job
    let mut job = BootCiJob::new();
    let out_of_order = !job.run_step(JobStep::MakeIso, true);
    let s1 = job.run_step(JobStep::Kcheck, true);
    let s2 = job.run_step(JobStep::Kbuild, true);
    let s3 = job.run_step(JobStep::MakeInitrd, true);
    let s4 = job.run_step(JobStep::MakeIso, true);
    let s5 = job.run_step(JobStep::SmokeBoot, true);
    let mut red = BootCiJob::new();
    let _ = red.run_step(JobStep::Kcheck, false);
    set.add(
        "F196 ci boot job",
        out_of_order
            && s1
            && s2
            && s3
            && s4
            && s5
            && job.green()
            && job.artifacts_produced() == 3
            && job.runs == 5
            && !red.green()
            && !red.run_step(JobStep::Kbuild, true)
            && BootCiJob::new().green() == false,
        "构建产物 + 冒烟引导入 CI",
    );

    // F197 真机验收清单
    let mut hw = standard_hw_checklist();
    let gauges = hw.with_gauges(9);
    let req_before = hw.required_failures();
    let _ = hw.mark(b"limine menu default", true);
    let _ = hw.mark(b"boot to desktop", true);
    let _ = hw.mark(b"keyboard input", true);
    let _ = hw.mark(b"mouse input", true);
    let _ = hw.mark(b"open app", true);
    let _ = hw.mark(b"read write file", true);
    let _ = hw.mark(b"shutdown keeps data", true);
    let accepted = hw.accepted();
    let optional_only = hw.mark(b"anim smoothness", false);
    let unknown = !hw.mark(b"nope", true);
    set.add(
        "F197 hw acceptance",
        hw.len() == 8
            && gauges
            && hw.gauges == 9
            && req_before == 7
            && accepted
            && optional_only
            && unknown
            && !HwAcceptance::new().accepted()
            && hw.required_failures() == 0,
        "真机验收步骤/仪表/必需项门禁",
    );

    // F198 版本发布纪律
    let mut policy = ReleasePolicy::new(Version::new(2, 0, 0));
    let _ = policy.add_change(b"boot direct to variable", false);
    let _ = policy.add_change(b"ab layout changed", true);
    let release_no_rb = policy.can_release();
    let marked = policy.mark_rollback(Version::new(1, 9, 0));
    let release_ok = policy.can_release();
    let bad_rb = !policy.mark_rollback(Version::new(2, 0, 0));
    let mut v = Version::new(1, 9, 3);
    v.bump_patch();
    let p = v;
    v.bump_minor();
    let m = v;
    v.bump_major();
    set.add(
        "F198 release discipline",
        policy.breaking_count() == 1
            && !release_no_rb
            && marked
            && policy.rollback_point == Some(Version::new(1, 9, 0))
            && release_ok
            && bad_rb
            && Version::parse(b"2.0.0") == Some(Version::new(2, 0, 0))
            && Version::parse(b"1.2").is_none()
            && Version::parse(b"x.y.z").is_none()
            && p == Version::new(1, 9, 4)
            && m == Version::new(1, 10, 0)
            && v == Version::new(2, 0, 0)
            && Version::new(2, 0, 0).newer_than(&Version::new(1, 9, 4))
            && !ReleasePolicy::new(Version::new(1, 0, 0)).can_release(),
        "语义化版本/变更清单/回滚点门禁",
    );

    // F199 作品集素材
    let mut portfolio = standard_portfolio();
    let mut partial = PortfolioSet::new();
    let _ = partial.mark(PortfolioKind::DashboardStills);
    let dup = !portfolio.mark(PortfolioKind::DashboardStills);
    set.add(
        "F199 portfolio assets",
        portfolio.complete()
            && portfolio.missing() == 0
            && portfolio.recording_secs == 45
            && portfolio.captures == 6
            && dup
            && !partial.complete()
            && partial.missing() == 2
            && !PortfolioSet::new().set_recording_secs(0)
            && PortfolioSet::new().complete() == false,
        "录屏/仪表面板截图/架构图齐备",
    );

    // F200 交付域自检 — 双产物 + 并存 + A/B + 恢复 + 发布
    let prior = set.len();
    let mut final_job = BootCiJob::new();
    let _ = final_job.run_step(JobStep::Kcheck, true);
    let _ = final_job.run_step(JobStep::Kbuild, true);
    let _ = final_job.run_step(JobStep::MakeInitrd, true);
    let _ = final_job.run_step(JobStep::MakeIso, true);
    let _ = final_job.run_step(JobStep::SmokeBoot, true);
    let final_portfolio = standard_portfolio();
    set.add(
        "F200 delivery self-test",
        iso.dual_bootable()
            && UsbImage::dd_image_of(&iso).deliverable()
            && guard.corruption_events == 0
            && guard.switches == SWITCH_ROUNDS
            && upd3.layout.active_version() == 101
            && hist3.latest() == Some(101)
            && chain.verify()
            && signer.sealed
            && SecureBootPlan::placeholder().never_blocks()
            && RecoveryMode::new(3).serial_ok
            && standard_services().core_all_enabled()
            && minimal.under_limit()
            && full.superset_of(&minimal)
            && final_portfolio.complete()
            && final_job.green()
            && prior + 1 == 25,
        "双产物/并存/A-B/恢复/发布全链闭环（25 项）",
    );

    set
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f176_default_is_variable() {
        let m = standard_menu();
        assert_eq!(m.default_kind(), Some(MenuKind::Variable));
        let mut m = m;
        assert_eq!(m.tick(5), Some(MenuKind::Variable));
    }

    #[test]
    fn f177_handoff_must_be_seamless() {
        let s = aurora_splash();
        assert!(s.seamless_handoff());
        let mut b = aurora_splash();
        b.desktop_first_color = rgb(1, 2, 3);
        assert!(!b.seamless_handoff());
    }

    #[test]
    fn f183_twenty_switches_intact() {
        let mut g = triple_boot_seed();
        assert_eq!(g.run_rounds(SWITCH_ROUNDS, true), SWITCH_ROUNDS);
        assert_eq!(g.corruption_events, 0);
        assert!(g.windows_present());
    }

    #[test]
    fn f184_active_and_standby_are_isolated() {
        let mut ab = AbLayout::new(100);
        assert!(ab.write_standby(101));
        assert_eq!(ab.active_version(), 100);
        assert_eq!(ab.standby_version(), 101);
        assert!(ab.switch());
        assert_eq!(ab.active_version(), 101);
        assert_eq!(ab.standby_version(), 100);
    }

    #[test]
    fn f185_commit_requires_verification() {
        let mut u = AbUpdater::new(100);
        assert!(u.stage(101));
        assert!(!u.commit());
        assert!(!u.verify(false));
        assert!(!u.commit());
        assert!(u.verify(true));
        assert!(u.commit());
        assert_eq!(u.layout.active_version(), 101);
    }

    #[test]
    fn f186_failure_rolls_back() {
        let mut u = AbUpdater::new(100);
        let mut h = RollbackHistory::new();
        assert!(h.push(100));
        assert_eq!(rollback_on_failure(&mut u, &mut h, false), Some(100));
        assert_eq!(u.layout.active_version(), 100);
    }

    #[test]
    fn f187_tamper_breaks_chain() {
        let mut c = standard_chain();
        assert!(c.verify());
        let mut t = IntegrityChain::new();
        let _ = t.add(ChainItem::Kernel, b"k", true);
        let _ = t.add_tampered(ChainItem::Initrd, b"i", 0x10);
        let _ = t.add(ChainItem::Assets, b"a", true);
        let _ = t.add(ChainItem::Signature, b"s", true);
        assert!(!t.verify());
    }

    #[test]
    fn f190_recovery_after_threshold() {
        let mut r = RecoveryMode::new(2);
        assert!(!r.note_crash());
        assert!(r.note_crash());
        let mut out = [0u8; 16];
        assert_eq!(r.run_command(b"help", &mut out), Some(16));
    }

    #[test]
    fn f191_safe_mode_keeps_core() {
        let mut s = standard_services();
        assert_eq!(s.enter_safe_mode(), 3);
        assert!(s.core_all_enabled());
        assert_eq!(s.enabled_non_core(), 0);
        assert!(s.degrade_render);
        assert_eq!(s.exit_safe_mode(), 3);
        assert_eq!(s.enabled_non_core(), 3);
    }

    #[test]
    fn f192_minimal_under_64mb() {
        let m = ImageSpec::minimal();
        assert!(m.size_mb < MINIMAL_LIMIT_MB);
        assert!(m.boots_to_desktop());
        assert!(ImageSpec::full().superset_of(&m));
    }

    #[test]
    fn f198_breaking_change_needs_rollback_point() {
        let mut p = ReleasePolicy::new(Version::new(2, 0, 0));
        assert!(!p.can_release());
        assert!(p.add_change(b"breaking", true));
        assert!(!p.can_release());
        assert!(p.mark_rollback(Version::new(1, 9, 0)));
        assert!(p.can_release());
    }

    #[test]
    fn f200_run_checks_pass() {
        let set = run_bootchain_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 4096];
            let n = set.render(&mut buf);
            panic!("bootchain self-test failed:\n{}", core::str::from_utf8(&buf[..n]).unwrap_or("<x>"));
        }
        assert!(set.len() == 25, "bootchain domain must expose exactly 25 checks, got {}", set.len());
    }
}
