//! AI-18 引导管理与部署域（F426~F450）。
//!
//! The three-entry boot menu, the countdown that lands on Varix, Windows
//! discovery, non-destructive BCD coexistence, boot-time condition variables,
//! the 1 TB five-partition USB deployer, the live image, the install wizard
//! with a read-only dry run, snapshots/repair media/one-key rollback, the
//! deploy log, the artistic menu, bilingual strings, accessibility and the
//! install/deploy self-tests.
//!
//! 红线 (F436): no plan may touch a disk until the operator confirms it, and
//! every destructive step has a dry run in front of it.

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F426/F427 — 三项引导菜单与倒计时
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootEntryKind {
    Varix,
    WtgGuest,
    HostWindows,
    Recovery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootEntry {
    pub kind: BootEntryKind,
    pub label: &'static str,
    pub available: bool,
}

pub const BOOT_MENU_ORDER: [BootEntryKind; 3] =
    [BootEntryKind::Varix, BootEntryKind::WtgGuest, BootEntryKind::HostWindows];

/// The default entry is always Varix (F427 野心): the countdown never drops
/// the user into another system unless they ask.
pub fn default_entry(entries: &[BootEntry]) -> BootEntryKind {
    for kind in BOOT_MENU_ORDER.iter() {
        if entries
            .iter()
            .any(|e| e.kind == *kind && e.available)
        {
            return *kind;
        }
    }
    entries
        .iter()
        .find(|e| e.available)
        .map(|e| e.kind)
        .unwrap_or(BootEntryKind::Recovery)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootCountdown {
    pub total_ms: u32,
    pub elapsed_ms: u32,
    pub cancelled: bool,
    /// Set once the user picks an entry manually.
    pub chosen: Option<BootEntryKind>,
}

impl BootCountdown {
    pub const fn new(total_ms: u32) -> BootCountdown {
        BootCountdown { total_ms, elapsed_ms: 0, cancelled: false, chosen: None }
    }

    pub fn tick(&mut self, dt_ms: u32) {
        if self.cancelled || self.chosen.is_some() {
            return;
        }
        self.elapsed_ms = (self.elapsed_ms + dt_ms).min(self.total_ms);
    }

    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    pub fn choose(&mut self, kind: BootEntryKind) {
        self.chosen = Some(kind);
        self.cancelled = true;
    }

    pub fn remaining_ms(&self) -> u32 {
        self.total_ms.saturating_sub(self.elapsed_ms)
    }

    pub fn expired(&self) -> bool {
        self.elapsed_ms >= self.total_ms
    }

    /// Final decision: an explicit choice wins, otherwise the default on
    /// timeout. A cancelled countdown with no choice waits forever.
    pub fn resolve(&self, default: BootEntryKind) -> Option<BootEntryKind> {
        if let Some(kind) = self.chosen {
            return Some(kind);
        }
        if !self.cancelled && self.expired() {
            return Some(default);
        }
        None
    }
}

// ---------------------------------------------------------------------------
// F428 — Windows 检测项
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowsInstall {
    pub partition_index: u8,
    pub esp_index: u8,
    /// `\EFI\Microsoft\Boot\bootmgfw.efi` present.
    pub bootmgr_present: bool,
    /// `\Windows\System32\ntoskrnl.exe` present.
    pub kernel_present: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowsDiscovery {
    Bootable(WindowsInstall),
    /// Partitions exist but the boot files are gone — repair medium (F439).
    NeedsRepair(WindowsInstall),
    NotFound,
}

/// Pick the first fully bootable install, else the first repairable one.
pub fn discover_windows(installs: &[WindowsInstall]) -> WindowsDiscovery {
    for i in installs {
        if i.bootmgr_present && i.kernel_present {
            return WindowsDiscovery::Bootable(*i);
        }
    }
    for i in installs {
        if i.kernel_present != i.bootmgr_present {
            return WindowsDiscovery::NeedsRepair(*i);
        }
    }
    WindowsDiscovery::NotFound
}

// ---------------------------------------------------------------------------
// F429 — BCD 共存安装
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BcdEntry {
    pub id: &'static str,
    pub description: &'static str,
    pub device: &'static str,
    /// Windows boot entries are never rewritten by the installer.
    pub system: bool,
}

pub const MAX_BCD: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BcdPlan {
    pub entries_before: usize,
    pub entries_after: usize,
    pub preserved_system: usize,
    pub added: usize,
}

/// Non-destructive BCD update: add our entry, keep every system entry.
pub fn plan_bcd(existing: &[BcdEntry], added: &[BcdEntry]) -> BcdPlan {
    let system = existing.iter().filter(|e| e.system).count();
    BcdPlan {
        entries_before: existing.len(),
        entries_after: existing.len() + added.len(),
        preserved_system: system,
        added: added.len(),
    }
}

/// The plan is only valid when nothing system-owned was dropped.
pub fn bcd_plan_safe(before: &[BcdEntry], plan: BcdPlan) -> bool {
    plan.preserved_system == before.iter().filter(|e| e.system).count()
        && plan.entries_after >= plan.entries_before
}

// ---------------------------------------------------------------------------
// F430/F431 — 引导期条件变量与重启意图
// ---------------------------------------------------------------------------

pub const BOOTVAR_BYTES: usize = 64;
pub const BOOTVAR_ENTRIES: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootVarKey {
    /// Next boot should be Windows (one-shot).
    NextBootWindows,
    /// Next boot should be the WTG guest.
    NextBootWtg,
    /// Installer completed successfully.
    InstallComplete,
    /// Last boot reached userspace.
    LastBootHealthy,
}

impl BootVarKey {
    pub fn slot(self) -> usize {
        match self {
            BootVarKey::NextBootWindows => 0,
            BootVarKey::NextBootWtg => 1,
            BootVarKey::InstallComplete => 2,
            BootVarKey::LastBootHealthy => 3,
        }
    }

    pub fn from_slot(slot: usize) -> Option<BootVarKey> {
        match slot {
            0 => Some(BootVarKey::NextBootWindows),
            1 => Some(BootVarKey::NextBootWtg),
            2 => Some(BootVarKey::InstallComplete),
            3 => Some(BootVarKey::LastBootHealthy),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootVars {
    values: [u32; BOOTVAR_ENTRIES],
    set: [bool; BOOTVAR_ENTRIES],
}

impl BootVars {
    pub const fn new() -> BootVars {
        BootVars { values: [0; BOOTVAR_ENTRIES], set: [false; BOOTVAR_ENTRIES] }
    }

    pub fn set(&mut self, key: BootVarKey, value: u32) {
        let slot = key.slot();
        self.values[slot] = value;
        self.set[slot] = true;
    }

    pub fn get(&self, key: BootVarKey) -> Option<u32> {
        let slot = key.slot();
        if self.set[slot] {
            Some(self.values[slot])
        } else {
            None
        }
    }

    pub fn take(&mut self, key: BootVarKey) -> Option<u32> {
        let slot = key.slot();
        if !self.set[slot] {
            return None;
        }
        self.set[slot] = false;
        Some(self.values[slot])
    }

    pub fn encode(self) -> [u8; BOOTVAR_BYTES] {
        let mut out = [0u8; BOOTVAR_BYTES];
        out[0..4].copy_from_slice(b"VBRT");
        for i in 0..BOOTVAR_ENTRIES {
            out[4 + i * 8..8 + i * 8].copy_from_slice(&self.values[i].to_le_bytes());
            out[8 + i * 8] = self.set[i] as u8;
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<BootVars> {
        if bytes.len() < BOOTVAR_BYTES || &bytes[0..4] != b"VBRT" {
            return None;
        }
        let mut vars = BootVars::new();
        for i in 0..BOOTVAR_ENTRIES {
            let mut v = [0u8; 4];
            v.copy_from_slice(&bytes[4 + i * 8..8 + i * 8]);
            vars.values[i] = u32::from_le_bytes(v);
            vars.set[i] = bytes[8 + i * 8] != 0;
        }
        Some(vars)
    }
}

/// F431: consume the one-shot reboot intent — it must not survive twice.
pub fn consume_reboot_intent(vars: &mut BootVars) -> Option<BootEntryKind> {
    if vars.take(BootVarKey::NextBootWindows).is_some() {
        return Some(BootEntryKind::HostWindows);
    }
    if vars.take(BootVarKey::NextBootWtg).is_some() {
        return Some(BootEntryKind::WtgGuest);
    }
    None
}

// ---------------------------------------------------------------------------
// F432 — 条件切换引擎
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchCondition {
    /// Boot into Windows on weekdays before this hour.
    WeekdayBeforeHour(u8),
    BatteryBelow(u8),
    Docked(bool),
    Always,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchContext {
    pub hour: u8,
    pub weekday: bool,
    pub battery_percent: u8,
    pub docked: bool,
}

/// Returns the entry to boot next, or `None` to keep the default.
pub fn evaluate_conditions(
    rules: &[SwitchCondition],
    ctx: SwitchContext,
    target: BootEntryKind,
) -> Option<BootEntryKind> {
    for rule in rules {
        let hit = match *rule {
            SwitchCondition::WeekdayBeforeHour(h) => ctx.weekday && ctx.hour < h,
            SwitchCondition::BatteryBelow(p) => ctx.battery_percent < p,
            SwitchCondition::Docked(want) => ctx.docked == want,
            SwitchCondition::Always => true,
        };
        if hit {
            return Some(target);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// F433/F436 — U 盘部署器与磁盘分区工具
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartFs {
    Fat32,
    Exfat,
    Ntfs,
    VarixLog,
    Raw,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartitionPlan {
    pub name: &'static str,
    pub size_bytes: u64,
    pub fs: PartFs,
    /// Shares one filesystem across all three systems (data area).
    pub shared: bool,
}

pub const ALIGNMENT_BYTES: u64 = 1 << 20; // 1 MiB

/// The 1 TB five-partition layout: ESP, WTG (Windows), Varix, shared data,
/// recovery. Sizes are GiB-exact so the plan is reproducible.
pub fn usb_layout(total_bytes: u64, gib: u64) -> [PartitionPlan; 5] {
    let fixed = [
        PartitionPlan { name: "ESP", size_bytes: 1 * gib, fs: PartFs::Fat32, shared: true },
        PartitionPlan { name: "WTG", size_bytes: 256 * gib, fs: PartFs::Ntfs, shared: false },
        PartitionPlan { name: "VARIX", size_bytes: 128 * gib, fs: PartFs::VarixLog, shared: false },
        PartitionPlan { name: "RECOVERY", size_bytes: 4 * gib, fs: PartFs::Fat32, shared: true },
    ];
    let used: u64 = fixed.iter().map(|p| p.size_bytes).sum();
    let data = total_bytes.saturating_sub(used).saturating_sub(ALIGNMENT_BYTES * 5);
    [
        fixed[0],
        fixed[1],
        fixed[2],
        PartitionPlan { name: "DATA", size_bytes: data, fs: PartFs::Exfat, shared: true },
        fixed[3],
    ]
}

/// Every partition must be 1 MiB-aligned and non-empty.
pub fn layout_ok(plans: &[PartitionPlan], total_bytes: u64) -> bool {
    let mut sum = 0u64;
    for (i, p) in plans.iter().enumerate() {
        if p.size_bytes == 0 || p.size_bytes % ALIGNMENT_BYTES != 0 {
            return false;
        }
        let _ = i;
        sum = sum.saturating_add(p.size_bytes);
    }
    sum <= total_bytes
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditAction {
    Create,
    Resize,
    Format,
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartitionEdit {
    pub disk: u8,
    pub index: u8,
    pub action: EditAction,
    pub size_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditVerdict {
    Ok,
    /// The host disk (0) is off-limits unless the operator explicitly opts in.
    HostDiskRefused,
    Misaligned,
    EmptySize,
}

pub fn validate_edit(edit: PartitionEdit, allow_host_disk: bool) -> EditVerdict {
    if edit.disk == 0 && !allow_host_disk {
        return EditVerdict::HostDiskRefused;
    }
    if edit.size_bytes == 0 {
        return EditVerdict::EmptySize;
    }
    if edit.size_bytes % ALIGNMENT_BYTES != 0 {
        return EditVerdict::Misaligned;
    }
    EditVerdict::Ok
}

// ---------------------------------------------------------------------------
// F434/F435 — Live 镜像与安装向导
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveImage {
    pub kernel_bytes: u64,
    pub rootfs_bytes: u64,
    pub checksum: u32,
    pub efi_bootable: bool,
}

pub const LIVE_MAX_BYTES: u64 = 8 << 30;

impl LiveImage {
    pub fn total_bytes(&self) -> u64 {
        self.kernel_bytes + self.rootfs_bytes
    }

    pub fn valid(&self) -> bool {
        self.efi_bootable && self.kernel_bytes > 0 && self.rootfs_bytes > 0
            && self.total_bytes() <= LIVE_MAX_BYTES
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WizardStep {
    Language,
    Disk,
    DryRun,
    Confirm,
    Install,
    Finish,
}

pub const WIZARD_STEPS: [WizardStep; 6] = [
    WizardStep::Language,
    WizardStep::Disk,
    WizardStep::DryRun,
    WizardStep::Confirm,
    WizardStep::Install,
    WizardStep::Finish,
];

/// The wizard refuses to advance past DryRun without a completed dry run.
pub fn wizard_can_advance(current: WizardStep, dry_run_done: bool) -> bool {
    if current == WizardStep::DryRun && !dry_run_done {
        return false;
    }
    current != WizardStep::Finish
}

pub fn wizard_index(step: WizardStep) -> usize {
    WIZARD_STEPS.iter().position(|s| *s == step).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// F437/F438/F439 — 预演、快照脚本、修复介质
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DryRunStep {
    pub disk: u8,
    pub action: EditAction,
    pub bytes: u64,
    /// A dry run never writes.
    pub read_only: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DryRunReport {
    pub steps: u8,
    pub total_bytes: u64,
    pub writes_performed: u8,
}

impl DryRunReport {
    pub fn safe(&self) -> bool {
        self.writes_performed == 0
    }
}

pub fn dry_run(steps: &[DryRunStep]) -> DryRunReport {
    let mut total = 0u64;
    for s in steps {
        total = total.saturating_add(s.bytes);
    }
    DryRunReport {
        steps: steps.len().min(u8::MAX as usize) as u8,
        total_bytes: total,
        writes_performed: steps.iter().filter(|s| !s.read_only).count().min(u8::MAX as usize) as u8,
    }
}

/// Emit the pre-deploy snapshot script (device-mapper style, one command per
/// line) into `out`. Returns the byte count.
pub fn snapshot_script(disk: u8, index: u32, out: &mut [u8]) -> usize {
    let mut n = 0usize;
    let write = |bytes: &[u8], out: &mut [u8], n: &mut usize| {
        for &b in bytes {
            if *n < out.len() {
                out[*n] = b;
                *n += 1;
            }
        }
    };
    write(b"varix-snapshot --disk ", out, &mut n);
    write(&[b'0' + (disk % 10)], out, &mut n);
    write(b" --index ", out, &mut n);
    let mut digits = [0u8; 10];
    let mut w = 0usize;
    let mut v = index;
    if v == 0 {
        digits[0] = b'0';
        w = 1;
    } else {
        while v > 0 && w < digits.len() {
            digits[w] = b'0' + (v % 10) as u8;
            v /= 10;
            w += 1;
        }
    }
    while w > 0 {
        w -= 1;
        write(&[digits[w]], out, &mut n);
    }
    write(b" --verify\n", out, &mut n);
    n
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepairMedia {
    pub has_bootloader: bool,
    pub has_varix_image: bool,
    pub has_windows_boot_files: bool,
    pub bytes: u64,
}

pub const REPAIR_MIN_BYTES: u64 = 512 << 20;

impl RepairMedia {
    pub fn usable(&self) -> bool {
        self.has_bootloader
            && self.has_varix_image
            && self.has_windows_boot_files
            && self.bytes >= REPAIR_MIN_BYTES
    }
}

// ---------------------------------------------------------------------------
// F440/F441 — 一键回滚与部署日志
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreMode {
    /// Put back the whole disk as it was.
    Full,
    /// Restore only the Varix partitions.
    VarixOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestorePlan {
    pub snapshot_index: u32,
    pub disk: u8,
    pub mode: RestoreMode,
    pub requires_repair_media: bool,
}

/// A rollback is always staged from a snapshot; without one it is refused —
/// silently "undoing" an install is how data gets lost.
pub fn restore_plan(snapshot_index: Option<u32>, disk: u8, mode: RestoreMode) -> Option<RestorePlan> {
    Some(RestorePlan {
        snapshot_index: snapshot_index?,
        disk,
        mode,
        requires_repair_media: true,
    })
}

const DEPLOY_LOG_CAP: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeployPhase {
    Preflight,
    Snapshot,
    Partition,
    Extract,
    Bootloader,
    Verify,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug)]
pub struct DeployLogEntry {
    pub phase: DeployPhase,
    pub stamp_ms: u64,
    pub detail: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct DeployLog {
    entries: [Option<DeployLogEntry>; DEPLOY_LOG_CAP],
    count: usize,
}

impl DeployLog {
    pub const fn new() -> DeployLog {
        DeployLog { entries: [None; DEPLOY_LOG_CAP], count: 0 }
    }

    pub fn push(&mut self, entry: DeployLogEntry) -> bool {
        if self.count >= DEPLOY_LOG_CAP {
            return false;
        }
        self.entries[self.count] = Some(entry);
        self.count += 1;
        true
    }

    pub fn get(&self, index: usize) -> Option<DeployLogEntry> {
        if index < self.count {
            self.entries[index]
        } else {
            None
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn last(&self) -> Option<DeployLogEntry> {
        if self.count == 0 {
            None
        } else {
            self.entries[self.count - 1]
        }
    }

    pub fn failed(&self) -> bool {
        (0..self.count).any(|i| {
            self.entries[i]
                .map(|e| e.phase == DeployPhase::Failed)
                .unwrap_or(false)
        })
    }
}

// ---------------------------------------------------------------------------
// F442/F443/F444/F445 — 菜单艺术化、动画、双语文案、可访问性
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuTheme {
    pub background: u32,
    pub accent: u32,
    pub blur_permille: u16,
    pub show_logo: bool,
    /// The seven-layer light show from F409 is reused in the boot menu.
    pub layered_animation: bool,
}

impl MenuTheme {
    pub const fn varix() -> MenuTheme {
        MenuTheme {
            background: 0x0B0F14,
            accent: 0x3B82F6,
            blur_permille: 800,
            show_logo: true,
            layered_animation: true,
        }
    }
}

/// Boot-menu animation reuses the desktop boot animation contract.
pub fn menu_animation(theme: MenuTheme, total_ms: u32) -> crate::vsem::BootAnimation {
    crate::vsem::BootAnimation {
        total_ms: if theme.layered_animation { total_ms } else { 200 },
        reduce_motion: !theme.layered_animation,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootLang {
    Zh,
    En,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootString {
    pub key: &'static str,
    pub zh: &'static str,
    pub en: &'static str,
}

pub const BOOT_STRINGS: [BootString; 5] = [
    BootString { key: "title", zh: "选择启动系统", en: "Select a system" },
    BootString { key: "varix", zh: "Varix 系统", en: "Varix" },
    BootString { key: "wtg", zh: "Windows 便携系统", en: "Windows (USB)" },
    BootString { key: "windows", zh: "本机 Windows", en: "Windows (internal)" },
    BootString { key: "timeout", zh: "秒后自动进入", en: "s to auto-start" },
];

pub fn boot_string(lang: BootLang, key: &str) -> Option<&'static str> {
    BOOT_STRINGS.iter().find(|s| s.key == key).map(|s| match lang {
        BootLang::Zh => s.zh,
        BootLang::En => s.en,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootA11y {
    /// Every menu item reachable with the keyboard.
    pub keyboard_nav: bool,
    pub narration_slots: u8,
    pub contrast_ok: bool,
}

impl BootA11y {
    pub fn usable(&self) -> bool {
        self.keyboard_nav && self.narration_slots >= 4 && self.contrast_ok
    }
}

/// WCAG-ish contrast ratio (Q8 fixed point) between two colours.
pub fn contrast_ratio(a: u32, b: u32) -> u32 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    // (hi + 0.05) / (lo + 0.05), scaled by 256 to stay in integers.
    ((hi + 13) * 256) / (lo + 13).max(1)
}

/// Luminance in 0..255 (gamma-blind approximation, good enough for gating).
pub fn relative_luminance(color: u32) -> u32 {
    let r = (color >> 16) & 0xFF;
    let g = (color >> 8) & 0xFF;
    let b = color & 0xFF;
    (r * 299 + g * 587 + b * 114) / 1000
}

/// 4.5:1 in Q8 = 1152.
pub const CONTRAST_AA_Q8: u32 = 1152;

// ---------------------------------------------------------------------------
// F446/F447/F448 — 离线包、驱动注入、升级保留
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfflineBundle {
    pub packages: u16,
    pub bytes: u64,
    /// All dependencies closed inside the bundle.
    pub closure_complete: bool,
    pub signature_verified: bool,
}

pub const OFFLINE_MAX_BYTES: u64 = 32 << 30;

impl OfflineBundle {
    pub fn installable(&self) -> bool {
        self.packages > 0
            && self.bytes <= OFFLINE_MAX_BYTES
            && self.closure_complete
            && self.signature_verified
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverInjection {
    pub vendor: u16,
    pub device: u16,
    pub driver: &'static str,
    /// Injection without a signature is refused (F309 link).
    pub signed: bool,
}

pub fn injection_allowed(injection: DriverInjection) -> bool {
    injection.signed && !injection.driver.is_empty()
}

/// F448: upgrade must never touch user data or the Windows partition.
pub const PRESERVE_ALWAYS: [&'static str; 4] =
    ["/home", "/data", "/media/host", "/etc/varix/local"];

pub fn should_preserve(path: &str) -> bool {
    for p in PRESERVE_ALWAYS.iter() {
        if path == *p {
            return true;
        }
        if path.len() > p.len() && path.starts_with(p) && path.as_bytes()[p.len()] == b'/' {
            return true; // a sub-path of a preserved root
        }
    }
    false
}

/// F448: an upgrade plan may only write the system area.
pub fn upgrade_writes_only_system(targets: &[&str]) -> bool {
    targets.iter().all(|t| !should_preserve(t))
}

// ---------------------------------------------------------------------------
// F449/F450 — 安装与部署自检
// ---------------------------------------------------------------------------

pub fn run_install_checks() -> CheckSet {
    let gib: u64 = 1 << 30;
    let mut set = CheckSet::new("install");

    let plans = usb_layout(1000 * gib, gib);
    set.add(
        "F433 deployer layout",
        plans.len() == 5
            && plans[0].name == "ESP"
            && plans[1].fs == PartFs::Ntfs
            && plans[2].fs == PartFs::VarixLog
            && plans[3].fs == PartFs::Exfat
            && plans[3].shared
            && layout_ok(&plans, 1000 * gib)
            && plans[3].size_bytes > 0,
        "1tb five partitions",
    );
    set.add(
        "F433 layout rejects",
        !layout_ok(&[PartitionPlan { name: "x", size_bytes: 3, fs: PartFs::Raw, shared: false }], gib),
        "alignment",
    );

    set.add(
        "F436 partition tool",
        validate_edit(
            PartitionEdit { disk: 1, index: 0, action: EditAction::Create, size_bytes: ALIGNMENT_BYTES },
            false,
        ) == EditVerdict::Ok
            && validate_edit(
                PartitionEdit { disk: 0, index: 0, action: EditAction::Format, size_bytes: ALIGNMENT_BYTES },
                false,
            ) == EditVerdict::HostDiskRefused
            && validate_edit(
                PartitionEdit { disk: 0, index: 0, action: EditAction::Format, size_bytes: ALIGNMENT_BYTES },
                true,
            ) == EditVerdict::Ok
            && validate_edit(
                PartitionEdit { disk: 1, index: 0, action: EditAction::Resize, size_bytes: 0 },
                false,
            ) == EditVerdict::EmptySize
            && validate_edit(
                PartitionEdit { disk: 1, index: 0, action: EditAction::Resize, size_bytes: 1000 },
                false,
            ) == EditVerdict::Misaligned,
        "host disk guard",
    );

    let live = LiveImage {
        kernel_bytes: 8 << 20,
        rootfs_bytes: 3 << 30,
        checksum: 0xABCD,
        efi_bootable: true,
    };
    set.add(
        "F434 live image",
        live.valid()
            && live.total_bytes() == (8 << 20) + (3 << 30)
            && !LiveImage { efi_bootable: false, ..live }.valid()
            && !LiveImage { rootfs_bytes: 0, ..live }.valid(),
        "live image",
    );

    set.add(
        "F435 install wizard",
        wizard_index(WizardStep::DryRun) == 2
            && !wizard_can_advance(WizardStep::DryRun, false)
            && wizard_can_advance(WizardStep::DryRun, true)
            && wizard_can_advance(WizardStep::Confirm, true)
            && !wizard_can_advance(WizardStep::Finish, true),
        "wizard gate",
    );

    let steps = [
        DryRunStep { disk: 1, action: EditAction::Create, bytes: ALIGNMENT_BYTES, read_only: true },
        DryRunStep { disk: 1, action: EditAction::Format, bytes: 0, read_only: true },
    ];
    let report = dry_run(&steps);
    set.add(
        "F437 dry run",
        report.steps == 2 && report.safe() && report.total_bytes == ALIGNMENT_BYTES,
        "read-only",
    );
    set.add(
        "F437 dry run detects writes",
        !dry_run(&[DryRunStep {
            disk: 1,
            action: EditAction::Format,
            bytes: 1,
            read_only: false,
        }])
        .safe(),
        "write detector",
    );

    let mut script = [0u8; 96];
    let n = snapshot_script(1, 42, &mut script);
    let text = core::str::from_utf8(&script[..n]).unwrap_or("");
    set.add(
        "F438 snapshot script",
        n > 0
            && text.starts_with("varix-snapshot")
            && text.contains("--index 42")
            && text.ends_with('\n'),
        "script generation",
    );
    set.add(
        "F438 script truncation",
        snapshot_script(9, 9, &mut [0u8; 8]) == 8,
        "bounded buffer",
    );

    let media = RepairMedia {
        has_bootloader: true,
        has_varix_image: true,
        has_windows_boot_files: true,
        bytes: REPAIR_MIN_BYTES,
    };
    set.add(
        "F439 repair media",
        media.usable() && !RepairMedia { bytes: 0, ..media }.usable(),
        "repair medium",
    );

    let plan = restore_plan(Some(7), 1, RestoreMode::Full).expect("plan");
    set.add(
        "F440 one-key rollback",
        plan.snapshot_index == 7
            && plan.requires_repair_media
            && restore_plan(None, 1, RestoreMode::Full).is_none(),
        "snapshot required",
    );

    let mut log = DeployLog::new();
    log.push(DeployLogEntry { phase: DeployPhase::Preflight, stamp_ms: 1, detail: 0 });
    log.push(DeployLogEntry { phase: DeployPhase::Done, stamp_ms: 2, detail: 0 });
    set.add(
        "F441 deploy log",
        log.len() == 2
            && log.last().map(|e| e.phase) == Some(DeployPhase::Done)
            && !log.failed(),
        "log",
    );
    log.push(DeployLogEntry { phase: DeployPhase::Failed, stamp_ms: 3, detail: -1 });
    set.add("F441 failure visible", log.failed(), "failure flag");

    set.add(
        "F446 offline bundle",
        OfflineBundle { packages: 10, bytes: 1 << 30, closure_complete: true, signature_verified: true }
            .installable()
            && !OfflineBundle { closure_complete: false, ..OfflineBundle {
                packages: 10,
                bytes: 1 << 30,
                closure_complete: true,
                signature_verified: true,
            } }
            .installable(),
        "closure + signature",
    );

    set.add(
        "F447 driver injection",
        injection_allowed(DriverInjection { vendor: 0x8086, device: 0x2922, driver: "ahci", signed: true })
            && !injection_allowed(DriverInjection { vendor: 1, device: 2, driver: "x", signed: false })
            && !injection_allowed(DriverInjection { vendor: 1, device: 2, driver: "", signed: true }),
        "signed only",
    );

    set.add(
        "F448 preserve user data",
        should_preserve("/home")
            && should_preserve("/home")
            && upgrade_writes_only_system(&["/system/varix"])
            && !upgrade_writes_only_system(&["/home"]),
        "preserve rules",
    );

    set
}

pub fn run_deploy_checks() -> CheckSet {
    let mut set = CheckSet::new("deploy");

    let entries = [
        BootEntry { kind: BootEntryKind::Varix, label: "Varix", available: true },
        BootEntry { kind: BootEntryKind::HostWindows, label: "Windows", available: true },
    ];
    set.add(
        "F426 three-entry menu",
        default_entry(&entries) == BootEntryKind::Varix
            && BOOT_MENU_ORDER.len() == 3
            && default_entry(&[BootEntry { kind: BootEntryKind::HostWindows, label: "w", available: true }])
                == BootEntryKind::HostWindows,
        "menu order",
    );

    let mut countdown = BootCountdown::new(3000);
    countdown.tick(1000);
    let remaining = countdown.remaining_ms();
    let resolved_mid = countdown.resolve(BootEntryKind::Varix);
    countdown.tick(5000);
    let resolved_end = countdown.resolve(BootEntryKind::Varix);
    set.add(
        "F427 countdown default",
        remaining == 2000
            && resolved_mid.is_none()
            && resolved_end == Some(BootEntryKind::Varix)
            && countdown.expired(),
        "auto start",
    );
    let mut cancelled = BootCountdown::new(3000);
    cancelled.cancel();
    cancelled.tick(9999);
    set.add(
        "F427 cancel holds",
        cancelled.resolve(BootEntryKind::Varix).is_none(),
        "wait for input",
    );
    let mut chosen = BootCountdown::new(3000);
    chosen.choose(BootEntryKind::HostWindows);
    set.add(
        "F427 explicit choice",
        chosen.resolve(BootEntryKind::Varix) == Some(BootEntryKind::HostWindows),
        "choice wins",
    );

    let installs = [
        WindowsInstall { partition_index: 2, esp_index: 1, bootmgr_present: true, kernel_present: true },
        WindowsInstall { partition_index: 3, esp_index: 1, bootmgr_present: false, kernel_present: true },
    ];
    set.add(
        "F428 windows discovery",
        matches!(discover_windows(&installs), WindowsDiscovery::Bootable(i) if i.partition_index == 2)
            && matches!(
                discover_windows(&[installs[1]]),
                WindowsDiscovery::NeedsRepair(_)
            )
            && discover_windows(&[]) == WindowsDiscovery::NotFound,
        "discovery",
    );

    let existing = [
        BcdEntry { id: "{a}", description: "Windows", device: "C:", system: true },
        BcdEntry { id: "{b}", description: "Recovery", device: "D:", system: true },
    ];
    let added = [BcdEntry { id: "{v}", description: "Varix", device: "E:", system: false }];
    let plan = plan_bcd(&existing, &added);
    set.add(
        "F429 bcd coexistence",
        plan.entries_after == 3
            && plan.preserved_system == 2
            && bcd_plan_safe(&existing, plan)
            && !bcd_plan_safe(&existing, BcdPlan { preserved_system: 1, ..plan }),
        "non destructive",
    );

    let mut vars = BootVars::new();
    vars.set(BootVarKey::InstallComplete, 1);
    let encoded = vars.encode();
    let mut decoded = BootVars::decode(&encoded).expect("bootvars");
    set.add(
        "F430 boot variables",
        decoded.get(BootVarKey::InstallComplete) == Some(1)
            && decoded.get(BootVarKey::NextBootWindows).is_none()
            && BootVars::decode(b"XXXX").is_none()
            && vars.get(BootVarKey::InstallComplete) == Some(1),
        "persist",
    );
    decoded.set(BootVarKey::NextBootWindows, 1);
    let first = consume_reboot_intent(&mut decoded);
    let second = consume_reboot_intent(&mut decoded);
    set.add(
        "F431 reboot to windows",
        first == Some(BootEntryKind::HostWindows) && second.is_none(),
        "one shot",
    );

    let ctx = SwitchContext { hour: 7, weekday: true, battery_percent: 20, docked: false };
    set.add(
        "F432 condition switch",
        evaluate_conditions(&[SwitchCondition::WeekdayBeforeHour(9)], ctx, BootEntryKind::HostWindows)
            == Some(BootEntryKind::HostWindows)
            && evaluate_conditions(
                &[SwitchCondition::WeekdayBeforeHour(6)],
                ctx,
                BootEntryKind::HostWindows,
            )
            .is_none()
            && evaluate_conditions(&[SwitchCondition::BatteryBelow(30)], ctx, BootEntryKind::WtgGuest)
                == Some(BootEntryKind::WtgGuest)
            && evaluate_conditions(&[SwitchCondition::Always], ctx, BootEntryKind::Recovery)
                == Some(BootEntryKind::Recovery),
        "rules",
    );

    let theme = MenuTheme::varix();
    let anim = menu_animation(theme, 1400);
    let plain = menu_animation(MenuTheme { layered_animation: false, ..theme }, 1400);
    set.add(
        "F442/F443 menu art",
        theme.show_logo
            && theme.blur_permille > 0
            && anim.layer_count() == 7
            && plain.layer_count() == 1
            && anim.progress_permille(700) == 500,
        "menu theme",
    );

    set.add(
        "F444 bilingual strings",
        boot_string(BootLang::Zh, "title") == Some("选择启动系统")
            && boot_string(BootLang::En, "title") == Some("Select a system")
            && boot_string(BootLang::En, "missing").is_none()
            && BOOT_STRINGS.len() >= 5,
        "zh + en",
    );

    let a11y = BootA11y { keyboard_nav: true, narration_slots: 4, contrast_ok: true };
    set.add(
        "F445 boot accessibility",
        a11y.usable()
            && !BootA11y { contrast_ok: false, ..a11y }.usable()
            && !BootA11y { narration_slots: 1, ..a11y }.usable()
            && contrast_ratio(0xFFFFFF, 0x000000) >= CONTRAST_AA_Q8
            && contrast_ratio(0x777777, 0x777777) < CONTRAST_AA_Q8,
        "a11y gate",
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
    fn f426_default_prefers_varix() {
        let entries = [
            BootEntry { kind: BootEntryKind::HostWindows, label: "w", available: true },
            BootEntry { kind: BootEntryKind::Varix, label: "v", available: false },
        ];
        assert_eq!(default_entry(&entries), BootEntryKind::HostWindows);
        assert_eq!(
            default_entry(&[BootEntry { kind: BootEntryKind::WtgGuest, label: "t", available: true }]),
            BootEntryKind::WtgGuest
        );
        assert_eq!(default_entry(&[]), BootEntryKind::Recovery);
    }

    #[test]
    fn f427_countdown_saturates() {
        let mut c = BootCountdown::new(1000);
        c.tick(u32::MAX);
        assert_eq!(c.remaining_ms(), 0);
        assert!(c.expired());
        assert_eq!(c.elapsed_ms, 1000);
    }

    #[test]
    fn f428_repair_detection() {
        let needs = [WindowsInstall {
            partition_index: 1,
            esp_index: 0,
            bootmgr_present: true,
            kernel_present: false,
        }];
        assert!(matches!(discover_windows(&needs), WindowsDiscovery::NeedsRepair(_)));
        let dead = [WindowsInstall {
            partition_index: 1,
            esp_index: 0,
            bootmgr_present: false,
            kernel_present: false,
        }];
        assert_eq!(discover_windows(&dead), WindowsDiscovery::NotFound);
    }

    #[test]
    fn f429_bcd_never_shrinks() {
        let existing = [BcdEntry { id: "a", description: "a", device: "C:", system: true }];
        let plan = plan_bcd(&existing, &[]);
        assert!(bcd_plan_safe(&existing, plan));
        assert_eq!(plan.entries_after, plan.entries_before);
        assert!(!bcd_plan_safe(&existing, BcdPlan { entries_after: 0, ..plan }));
    }

    #[test]
    fn f430_bootvars_round_trip_all_keys() {
        let mut v = BootVars::new();
        for slot in 0..BOOTVAR_ENTRIES {
            let key = BootVarKey::from_slot(slot).expect("key");
            v.set(key, slot as u32 + 1);
        }
        let decoded = BootVars::decode(&v.encode()).expect("decode");
        for slot in 0..BOOTVAR_ENTRIES {
            let key = BootVarKey::from_slot(slot).unwrap();
            assert_eq!(decoded.get(key), Some(slot as u32 + 1));
        }
        assert!(BootVarKey::from_slot(99).is_none());
        assert!(BootVars::decode(&[0u8; 4]).is_none());
    }

    #[test]
    fn f432_conditions_are_ordered() {
        let ctx = SwitchContext { hour: 12, weekday: false, battery_percent: 90, docked: true };
        assert!(evaluate_conditions(&[], ctx, BootEntryKind::Varix).is_none());
        assert_eq!(
            evaluate_conditions(&[SwitchCondition::Docked(true)], ctx, BootEntryKind::Varix),
            Some(BootEntryKind::Varix)
        );
        assert!(evaluate_conditions(&[SwitchCondition::Docked(false)], ctx, BootEntryKind::Varix).is_none());
    }

    #[test]
    fn f433_layout_math() {
        let gib: u64 = 1 << 30;
        let plans = usb_layout(1000 * gib, gib);
        let total: u64 = plans.iter().map(|p| p.size_bytes).sum();
        assert!(total <= 1000 * gib);
        assert!(plans[3].size_bytes > 500 * gib);
        assert!(layout_ok(&plans, 1000 * gib));
        // A smaller stick still produces a valid (smaller) DATA partition.
        let small = usb_layout(400 * gib, gib);
        assert!(small[3].size_bytes < plans[3].size_bytes);
    }

    #[test]
    fn f436_alignment_is_mandatory() {
        let ok = PartitionEdit { disk: 2, index: 1, action: EditAction::Create, size_bytes: 2 * ALIGNMENT_BYTES };
        assert_eq!(validate_edit(ok, false), EditVerdict::Ok);
        assert_eq!(validate_edit(PartitionEdit { size_bytes: ALIGNMENT_BYTES + 1, ..ok }, false), EditVerdict::Misaligned);
    }

    #[test]
    fn f437_dry_run_is_read_only() {
        let report = dry_run(&[]);
        assert_eq!(report.steps, 0);
        assert!(report.safe());
        assert_eq!(report.total_bytes, 0);
    }

    #[test]
    fn f442_contrast_gate() {
        assert!(contrast_ratio(0x0B0F14, 0xFFFFFF) >= CONTRAST_AA_Q8);
        let mid = contrast_ratio(0x3B82F6, 0x3B82F6);
        assert!(mid < CONTRAST_AA_Q8);
    }

    #[test]
    fn f446_bundle_limits() {
        let big = OfflineBundle {
            packages: 1,
            bytes: OFFLINE_MAX_BYTES + 1,
            closure_complete: true,
            signature_verified: true,
        };
        assert!(!big.installable());
        let empty = OfflineBundle { packages: 0, ..big };
        assert!(!empty.installable());
        assert_eq!(REPAIR_MIN_BYTES, 512 << 20);
    }

    #[test]
    fn f448_preserve_prefixes() {
        assert!(should_preserve("/home"));
        assert!(!should_preserve("/system"));
        assert!(upgrade_writes_only_system(&["/system", "/boot"]));
        assert!(!upgrade_writes_only_system(&["/system", "/data"]));
    }

    #[test]
    fn f449_install_checks_pass() {
        let set = run_install_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("install self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 12);
    }

    #[test]
    fn f450_deploy_checks_pass() {
        let set = run_deploy_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("deploy self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 12);
    }
}
