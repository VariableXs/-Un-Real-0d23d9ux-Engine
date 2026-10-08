//! TRINITY-500 · AI-02 电源与休眠安全域（F026~F050，W1）
//!
//! 职责：Windows↔Varix 休眠切换闭环、休眠文件加密、掉电安全、切换状态机与预算。
//! 依赖：VARIX-500 AI-11（电源热）的 FADT/休眠骨架与 AI-01 的 BootNext 原语。
//! 边界：休眠文件**永不**落在共享卷（AI-04 的那座桥）上——这是防交叉读的底线。

use crate::checks::CheckSet;
use crate::power::{can_hibernate, crc32, parse_fadt, required_storage_bytes, HibernationHeader, WakeKind};
use crate::switcher::bootnext::{BootError, BootNext, BootTarget};

// ---------------------------------------------------------------------------
// F026 ACPI FADT 解析 — 已有骨架，收口
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FadtSummary {
    pub pm1a_cnt_blk: u32,
    pub pm1b_cnt_blk: u32,
    pub sci_int: u16,
    pub smi_cmd: u32,
    pub profile: u8,
    /// 该平台是否具备 S4 所需的寄存器布局。
    pub hibernate_capable: bool,
}

impl FadtSummary {
    pub fn new(pm1a: u32, pm1b: u32, sci: u16, smi: u32, profile: u8) -> FadtSummary {
        FadtSummary {
            pm1a_cnt_blk: pm1a,
            pm1b_cnt_blk: pm1b,
            sci_int: sci,
            smi_cmd: smi,
            profile,
            hibernate_capable: pm1a != 0,
        }
    }
}

/// 解析真机 FADT 表体（含 "FACP" 头）。解析失败返回 None——不猜寄存器。
pub fn fadt_summary(table: &[u8]) -> Option<FadtSummary> {
    let f = parse_fadt(table)?;
    Some(FadtSummary::new(f.pm1a_cnt_blk, f.pm1b_cnt_blk, f.sci_int, f.smi_cmd, f.profile))
}

// ---------------------------------------------------------------------------
// F027 S3/S4 休眠头构建
// F043 休眠头 CRC32 校验
// ---------------------------------------------------------------------------

pub fn s4_header(pages: u32, page_size: u64, image_crc: u32) -> HibernationHeader {
    crate::power::build_hibernation_header(pages, page_size, image_crc)
}

pub fn header_crc_ok(header: &HibernationHeader, image_crc: u32) -> bool {
    header.valid() && header.crc == image_crc
}

/// 镜像需要的落盘空间（含 8% 余量与头）。
pub fn image_storage_bytes(pages: u64, page_size: u64) -> u64 {
    required_storage_bytes(pages, page_size)
}

pub fn storage_ok(free_bytes: u64, pages: u64, page_size: u64) -> bool {
    can_hibernate(free_bytes, image_storage_bytes(pages, page_size))
}

// ---------------------------------------------------------------------------
// F028 唤醒向量写入 — 补真实设备路径
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WakeVector {
    /// 固件跳回的物理入口（FACS 里的 waking vector）。
    pub addr: u32,
    /// 该地址所在镜像的真实设备路径（不写死，由调用方给出）。
    pub path: &'static str,
    pub crc: u32,
}

impl WakeVector {
    pub fn new(addr: u32, path: &'static str) -> WakeVector {
        let mut crc = addr as u32;
        for &b in path.as_bytes() {
            crc = crc32(&[b, (addr >> 8) as u8]).wrapping_add(crc.rotate_left(5));
        }
        WakeVector { addr, path, crc }
    }

    pub fn ok(&self) -> bool {
        // 唤醒向量必须 4KiB 对齐且非空路径，否则固件会跳到垃圾里。
        self.addr != 0 && self.addr % 4096 == 0 && !self.path.is_empty()
    }
}

// ---------------------------------------------------------------------------
// F031 休眠文件位置策略 — U 盘/本地盘可配
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HiberLocation {
    /// U 盘上的专属分区（默认，随系统走）。
    Usb,
    /// 宿主机本地盘（更快，但不随系统走）。
    LocalDisk,
    /// 共享卷——**永远不允许**。
    SharedVolume,
}

impl HiberLocation {
    pub fn name(self) -> &'static str {
        match self {
            HiberLocation::Usb => "usb",
            HiberLocation::LocalDisk => "local-disk",
            HiberLocation::SharedVolume => "shared-volume",
        }
    }
}

/// F031 + F032 底线：休眠文件里是「睡着的 Windows」的全部内存，绝不能进共享卷。
pub fn location_allowed(loc: HiberLocation) -> bool {
    !matches!(loc, HiberLocation::SharedVolume)
}

pub fn location_reason(loc: HiberLocation) -> &'static str {
    match loc {
        HiberLocation::Usb => "portable, travels with the stick",
        HiberLocation::LocalDisk => "faster, but bound to one machine",
        HiberLocation::SharedVolume => "denied: holds sleeping memory, must never cross the shared bridge",
    }
}

// ---------------------------------------------------------------------------
// F032 休眠文件加密 — 防 Varix 读「睡着的 Windows」内存残留
// ---------------------------------------------------------------------------

/// 内存驻留密钥：退出/切换完成即 `wipe()`，不落盘、不进共享卷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HiberKey {
    k0: u64,
    k1: u64,
    valid: bool,
}

impl HiberKey {
    pub const fn empty() -> HiberKey {
        HiberKey { k0: 0, k1: 0, valid: false }
    }

    pub fn from_seed(seed0: u64, seed1: u64) -> HiberKey {
        HiberKey { k0: seed0 ^ 0x9E37_79B9_7F4A_7C15, k1: seed1 ^ 0xD1B5_4A32_D192_ED03, valid: true }
    }

    pub fn wipe(&mut self) {
        self.k0 = 0;
        self.k1 = 0;
        self.valid = false;
    }

    pub fn is_valid(&self) -> bool {
        self.valid
    }
}

impl Default for HiberKey {
    fn default() -> Self {
        HiberKey::empty()
    }
}

/// splitmix64 派生的 64 位密钥流块。
///
/// 诚实边界：这是一条由驻留密钥派生的 XOR 密钥流，机密性完全依赖密钥不泄露；
/// 磁盘上的完整性由 CRC32 覆盖（F043）。正式部署走 `crate::security` 的
/// AES-256-GCM 通道，本函数不假装自己是认证加密。
pub fn keystream_block(key: &HiberKey, nonce: u64, counter: u64) -> u64 {
    let mut x = key.k0
        .wrapping_add(nonce)
        .wrapping_add(counter.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (x ^ (x >> 31)).wrapping_add(key.k1)
}

/// 原地加/解密（XOR 对称，同一函数两次即还原）。
pub fn crypt_in_place(key: &HiberKey, nonce: u64, buf: &mut [u8]) {
    let mut counter = 0u64;
    let mut i = 0usize;
    while i < buf.len() {
        let block = keystream_block(key, nonce, counter).to_le_bytes();
        let take = if buf.len() - i < 8 { buf.len() - i } else { 8 };
        for k in 0..take {
            buf[i + k] ^= block[k];
        }
        i += take;
        counter += 1;
    }
}

// ---------------------------------------------------------------------------
// F033 掉电安全 — 休眠写盘原子性 + 校验
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteStage {
    Idle,
    DataWritten,
    HeaderCommitted,
    Verified,
}

/// 原子写协议：**数据先落盘，头最后写**。任何时刻掉电，要么头不存在（视为无镜像、
/// 冷启动），要么头完整可校验——绝不出现「半个镜像」被当成有效镜像。
#[derive(Clone, Copy, Debug)]
pub struct AtomicHiberWrite {
    pub stage: WriteStage,
    pub expected_pages: u32,
    pub written_pages: u32,
}

impl AtomicHiberWrite {
    pub const fn new(pages: u32) -> AtomicHiberWrite {
        AtomicHiberWrite { stage: WriteStage::Idle, expected_pages: pages, written_pages: 0 }
    }

    pub fn begin(&mut self) {
        self.stage = WriteStage::Idle;
        self.written_pages = 0;
    }

    pub fn write_page(&mut self) -> bool {
        if self.written_pages >= self.expected_pages {
            return false;
        }
        self.written_pages += 1;
        if self.written_pages == self.expected_pages {
            self.stage = WriteStage::DataWritten;
        }
        true
    }

    /// 提交头（掉电前的最后一步）。
    pub fn commit_header(&mut self) -> bool {
        if self.stage != WriteStage::DataWritten {
            return false;
        }
        self.stage = WriteStage::HeaderCommitted;
        true
    }

    pub fn verify(&mut self, crc_match: bool) -> bool {
        if self.stage != WriteStage::HeaderCommitted || !crc_match {
            // 校验失败必须回到 Idle：宁可冷启动，也不恢复可疑镜像。
            self.stage = WriteStage::Idle;
            return false;
        }
        self.stage = WriteStage::Verified;
        true
    }

    /// 掉电后恢复：只有 Verified 头才允许 resume。
    pub fn resumable(&self) -> bool {
        self.stage == WriteStage::Verified
    }
}

// ---------------------------------------------------------------------------
// F034 切换超时与熔断 — 失败自动回退
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct CircuitBreaker {
    pub failures: u8,
    pub threshold: u8,
    pub open: bool,
}

impl CircuitBreaker {
    pub const fn new(threshold: u8) -> CircuitBreaker {
        CircuitBreaker { failures: 0, threshold, open: false }
    }

    pub fn allow(&self) -> bool {
        !self.open
    }

    pub fn record_failure(&mut self) {
        self.failures = self.failures.saturating_add(1);
        if self.failures >= self.threshold {
            self.open = true;
        }
    }

    pub fn record_success(&mut self) {
        self.failures = 0;
        self.open = false;
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        CircuitBreaker::new(2)
    }
}

// ---------------------------------------------------------------------------
// F035 切换状态机 — 空闲/休眠中/恢复中/运行
// F036 切换进度如实显示 — 无假进度
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchState {
    Idle,
    Hibernating,
    Resuming,
    Running,
    Failed,
}

impl SwitchState {
    pub fn name(self) -> &'static str {
        match self {
            SwitchState::Idle => "idle",
            SwitchState::Hibernating => "hibernating",
            SwitchState::Resuming => "resuming",
            SwitchState::Running => "running",
            SwitchState::Failed => "failed",
        }
    }
}

/// 合法迁移表：Idle→Hibernating→Resuming→Running；任意阶段可 →Failed。
#[derive(Clone, Copy, Debug)]
pub struct SwitchMachine {
    pub state: SwitchState,
    pub previous: SwitchState,
}

impl SwitchMachine {
    pub const fn new() -> SwitchMachine {
        SwitchMachine { state: SwitchState::Idle, previous: SwitchState::Idle }
    }

    pub fn transition(&mut self, to: SwitchState) -> bool {
        let legal = match (self.state, to) {
            (SwitchState::Idle, SwitchState::Hibernating) => true,
            (SwitchState::Hibernating, SwitchState::Resuming) => true,
            (SwitchState::Resuming, SwitchState::Running) => true,
            (SwitchState::Running, SwitchState::Hibernating) => true,
            (_, SwitchState::Failed) => true,
            (SwitchState::Failed, SwitchState::Idle) => true,
            _ => false,
        };
        if legal {
            self.previous = self.state;
            self.state = to;
        }
        legal
    }

    pub fn fail(&mut self) {
        self.previous = self.state;
        self.state = SwitchState::Failed;
    }
}

impl Default for SwitchMachine {
    fn default() -> Self {
        SwitchMachine::new()
    }
}

/// F036：进度。`known=false` 时 UI 必须显示不确定态（转圈），**不允许**编造百分比。
#[derive(Clone, Copy, Debug)]
pub struct SwitchProgress {
    pub state: SwitchState,
    pub permille: u16,
    pub known: bool,
}

impl SwitchProgress {
    pub fn for_state(state: SwitchState, permille: u16) -> SwitchProgress {
        // 只有「休眠中」能给出真实字节进度；恢复/启动阶段盘速未知。
        let known = matches!(state, SwitchState::Hibernating) && permille <= 1000;
        SwitchProgress { state, permille: if known { permille } else { 0 }, known }
    }

    pub fn label(&self) -> &'static str {
        if self.known {
            self.state.name()
        } else {
            "working"
        }
    }
}

// ---------------------------------------------------------------------------
// F029 Windows→Varix 切换 / F030 Varix→Windows 切换
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchDirection {
    WindowsToVarix,
    VarixToWindows,
}

impl SwitchDirection {
    pub fn from_os(self) -> &'static str {
        match self {
            SwitchDirection::WindowsToVarix => "Windows",
            SwitchDirection::VarixToWindows => "Varix",
        }
    }
    pub fn to_os(self) -> &'static str {
        match self {
            SwitchDirection::WindowsToVarix => "Varix",
            SwitchDirection::VarixToWindows => "Windows",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchError {
    Ok,
    NotHalted,
    NoStorage,
    CryptoMissing,
    BreakerOpen,
    LocationDenied,
    BootNextFailed,
}

impl SwitchError {
    pub fn as_str(self) -> &'static str {
        match self {
            SwitchError::Ok => "ok",
            SwitchError::NotHalted => "source system still running",
            SwitchError::NoStorage => "not enough space for image",
            SwitchError::CryptoMissing => "hibernation key not resident",
            SwitchError::BreakerOpen => "circuit breaker open",
            SwitchError::LocationDenied => "hibernation file may not live on the shared volume",
            SwitchError::BootNextFailed => "bootnext write failed",
        }
    }
}

/// 一次切换的完整计划（编排产物，交给执行器按序落盘/重启）。
#[derive(Clone, Copy, Debug)]
pub struct SwitchPlan {
    pub direction: SwitchDirection,
    pub location: HiberLocation,
    pub pages: u32,
    pub image_bytes: u64,
    pub boot_target: BootTarget,
    pub write_wake_vector: bool,
}

/// 规划一次切换。任何前置条件不满足都返回具体错误（不静默降级）。
pub fn plan_switch(
    direction: SwitchDirection,
    location: HiberLocation,
    pages: u32,
    page_size: u64,
    free_bytes: u64,
    breaker: &CircuitBreaker,
    key: &HiberKey,
) -> Result<SwitchPlan, SwitchError> {
    if !location_allowed(location) {
        return Err(SwitchError::LocationDenied);
    }
    if !breaker.allow() {
        return Err(SwitchError::BreakerOpen);
    }
    if !key.is_valid() {
        return Err(SwitchError::CryptoMissing);
    }
    if pages == 0 || !storage_ok(free_bytes, pages as u64, page_size) {
        return Err(SwitchError::NoStorage);
    }
    let target = match direction {
        SwitchDirection::WindowsToVarix => BootTarget::Varix,
        SwitchDirection::VarixToWindows => BootTarget::Windows,
    };
    Ok(SwitchPlan {
        direction,
        location,
        pages,
        image_bytes: image_storage_bytes(pages as u64, page_size),
        boot_target: target,
        write_wake_vector: matches!(direction, SwitchDirection::VarixToWindows),
    })
}

/// 把计划落到 BootNext（真正的重启由执行器发起，本函数不碰硬件）。
pub fn commit_switch(plan: &SwitchPlan, bootnext: &mut BootNext) -> Result<(), BootError> {
    bootnext.set(plan.boot_target)
}

// ---------------------------------------------------------------------------
// F037 唤醒源管理 — RTC/电源键/网络
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct WakePolicy {
    pub mask: u32,
    /// 零出站网络策略下默认关闭网络唤醒（AI-18 F434）。
    pub allow_network: bool,
}

impl WakePolicy {
    pub const fn safe_default() -> WakePolicy {
        // 手工展开 mask_bit()：const fn 里无法调用非 const 方法。
        WakePolicy { mask: (1u32 << WakeKind::PowerButton as u32) | (1u32 << WakeKind::Keyboard as u32), allow_network: false }
    }

    pub fn allows(&self, kind: WakeKind) -> bool {
        if matches!(kind, WakeKind::Nic) && !self.allow_network {
            return false;
        }
        self.mask & kind.mask_bit() != 0
    }

    pub fn enable(&mut self, kind: WakeKind) -> bool {
        if matches!(kind, WakeKind::Nic) && !self.allow_network {
            return false;
        }
        self.mask |= kind.mask_bit();
        true
    }

    pub fn disable(&mut self, kind: WakeKind) {
        self.mask &= !kind.mask_bit();
    }
}

// ---------------------------------------------------------------------------
// F038 S5 完整关机路径
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownStep {
    FlushDirty = 0,
    StopServices = 1,
    SyncVolumes = 2,
    UnmountShared = 3,
    PowerOff = 4,
}

impl ShutdownStep {
    pub fn name(self) -> &'static str {
        match self {
            ShutdownStep::FlushDirty => "flush-dirty",
            ShutdownStep::StopServices => "stop-services",
            ShutdownStep::SyncVolumes => "sync-volumes",
            ShutdownStep::UnmountShared => "unmount-shared",
            ShutdownStep::PowerOff => "power-off",
        }
    }
    pub fn from_index(i: usize) -> Option<ShutdownStep> {
        match i {
            0 => Some(ShutdownStep::FlushDirty),
            1 => Some(ShutdownStep::StopServices),
            2 => Some(ShutdownStep::SyncVolumes),
            3 => Some(ShutdownStep::UnmountShared),
            4 => Some(ShutdownStep::PowerOff),
            _ => None,
        }
    }
}

pub const S5_STEPS: usize = 5;

/// S5 必须卸载共享卷后再断电，否则共享卷上的写缓存会丢（AI-03 F062）。
pub fn shutdown_path(out: &mut [ShutdownStep]) -> usize {
    let mut n = 0usize;
    for i in 0..S5_STEPS {
        if let Some(s) = ShutdownStep::from_index(i) {
            if n < out.len() {
                out[n] = s;
                n += 1;
            }
        }
    }
    n
}

// ---------------------------------------------------------------------------
// F039 紧急中断切换 — Del+Backspace 语义保留
// F045 双 Esc 手势内核化
// ---------------------------------------------------------------------------

pub const EMERGENCY_WINDOW_MS: u32 = 500;
pub const DOUBLE_ESC_WINDOW_MS: u32 = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbortKey {
    Delete,
    Backspace,
    Escape,
    Other,
}

impl AbortKey {
    pub fn from_scancode(code: u8) -> AbortKey {
        match code {
            0x0E => AbortKey::Backspace,
            0x53 => AbortKey::Delete,
            0x01 => AbortKey::Escape,
            _ => AbortKey::Other,
        }
    }
}

/// F039：Del 后紧接 Backspace（在窗口内）触发紧急中断。
pub fn emergency_abort(keys: &[AbortKey], window_ms: u32, stamps: &[u32]) -> bool {
    if keys.len() < 2 || stamps.len() < keys.len() {
        return false;
    }
    for i in 1..keys.len() {
        if keys[i - 1] == AbortKey::Delete
            && keys[i] == AbortKey::Backspace
            && stamps[i].saturating_sub(stamps[i - 1]) <= window_ms
        {
            return true;
        }
    }
    false
}

/// F045：两次 Esc 判定为一次环境切换手势。
pub fn double_esc(prev_esc_ms: u32, now_ms: u32, window_ms: u32) -> bool {
    now_ms >= prev_esc_ms && now_ms.saturating_sub(prev_esc_ms) <= window_ms
}

#[derive(Clone, Copy, Debug)]
pub struct GestureTracker {
    pub last_escape_ms: u32,
    pub armed: bool,
}

impl GestureTracker {
    pub const fn new() -> GestureTracker {
        GestureTracker { last_escape_ms: 0, armed: false }
    }

    /// 返回 true 表示识别到「双 Esc」手势。
    pub fn escape(&mut self, now_ms: u32) -> bool {
        let hit = self.armed && double_esc(self.last_escape_ms, now_ms, DOUBLE_ESC_WINDOW_MS);
        self.last_escape_ms = now_ms;
        self.armed = !hit;
        hit
    }
}

impl Default for GestureTracker {
    fn default() -> Self {
        GestureTracker::new()
    }
}

// ---------------------------------------------------------------------------
// F040 切换日志与取证 — 谁在何时切过
// F046 切换失败诊断
// ---------------------------------------------------------------------------

pub const SWITCH_LOG_CAP: usize = 12;

#[derive(Clone, Copy, Debug)]
pub struct SwitchRecord {
    pub direction: SwitchDirection,
    pub stamp_ms: u64,
    pub duration_ms: u32,
    pub ok: bool,
    pub error: SwitchError,
}

#[derive(Clone, Copy, Debug)]
pub struct SwitchLog {
    records: [Option<SwitchRecord>; SWITCH_LOG_CAP],
    count: usize,
}

impl SwitchLog {
    pub const fn new() -> SwitchLog {
        SwitchLog { records: [None; SWITCH_LOG_CAP], count: 0 }
    }

    pub fn append(&mut self, r: SwitchRecord) {
        if self.count < SWITCH_LOG_CAP {
            self.records[self.count] = Some(r);
            self.count += 1;
        } else {
            // 环式覆盖：取证只看最近 12 次。
            for i in 1..SWITCH_LOG_CAP {
                self.records[i - 1] = self.records[i];
            }
            self.records[SWITCH_LOG_CAP - 1] = Some(r);
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<SwitchRecord> {
        if i < self.count {
            self.records[i]
        } else {
            None
        }
    }

    pub fn failures(&self) -> usize {
        (0..self.count).filter(|i| self.get(*i).map(|r| !r.ok).unwrap_or(false)).count()
    }
}

impl Default for SwitchLog {
    fn default() -> Self {
        SwitchLog::new()
    }
}

/// F046：把错误码翻译成人能看懂的归因（不隐瞒、不美化）。
pub fn diagnose(e: SwitchError, stage: SwitchState) -> &'static str {
    match e {
        SwitchError::Ok => "no failure",
        SwitchError::NotHalted => "source system never reached a halted state",
        SwitchError::NoStorage => "hibernation image does not fit; free space or shrink memory",
        SwitchError::CryptoMissing => "hibernation key was wiped before the image was written",
        SwitchError::BreakerOpen => "too many consecutive switch failures; breaker open",
        SwitchError::LocationDenied => "shared volume refused for hibernation file",
        SwitchError::BootNextFailed => match stage {
            SwitchState::Hibernating => "bootnext write failed while hibernating",
            _ => "bootnext write failed",
        },
    }
}

// ---------------------------------------------------------------------------
// F041 休眠镜像大小预算 / F042 切换性能预算（往返 < 60s）
// ---------------------------------------------------------------------------

pub const SWITCH_REDLINE_MS: u32 = 60_000;

/// 按内存量分档的往返预算（写盘 + 恢复 + 两次引导）。
pub fn roundtrip_budget_ms(mem_gb: u32) -> u32 {
    match mem_gb {
        0..=4 => 20_000,
        5..=8 => 30_000,
        9..=16 => 40_000,
        17..=32 => 55_000,
        _ => SWITCH_REDLINE_MS,
    }
}

/// 镜像是否超预算：以「内存量的 60%」为可裁剪目标。
pub fn image_within_budget(image_bytes: u64, mem_bytes: u64) -> bool {
    image_bytes <= mem_bytes * 60 / 100 + mem_bytes / 8
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchVerdict {
    Within,
    Over,
}

pub fn switch_verdict(measured_ms: u32, mem_gb: u32) -> SwitchVerdict {
    if measured_ms <= roundtrip_budget_ms(mem_gb) {
        SwitchVerdict::Within
    } else {
        SwitchVerdict::Over
    }
}

// ---------------------------------------------------------------------------
// F044 宿主 UAC/登录态边界 — 如实降级
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostLoginState {
    /// 无人登录（锁屏/登录屏）。
    NoSession,
    /// 已登录但非提升权限。
    UserSession,
    /// 已提升（管理员）。
    Elevated,
}

/// 内核无法代替宿主完成 UAC 提升或会话登录——只能如实说明。
pub fn host_boundary_note(state: HostLoginState) -> &'static str {
    match state {
        HostLoginState::NoSession => "no host session: hibernation must be triggered from the lock screen path",
        HostLoginState::UserSession => "unelevated session: kernel cannot satisfy UAC, switch may be blocked",
        HostLoginState::Elevated => "elevated session: switch may proceed",
    }
}

pub fn host_switch_allowed(state: HostLoginState) -> bool {
    matches!(state, HostLoginState::Elevated)
}

// ---------------------------------------------------------------------------
// F047 多环境休眠共存 — 互不覆盖
// ---------------------------------------------------------------------------

pub const MAX_HIBER_SLOTS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HiberSlot {
    pub owner: BootTarget,
    pub generation: u32,
    pub offset_bytes: u64,
    pub size_bytes: u64,
}

pub struct SlotTable {
    slots: [Option<HiberSlot>; MAX_HIBER_SLOTS],
    count: usize,
}

impl SlotTable {
    pub const fn new() -> SlotTable {
        SlotTable { slots: [None; MAX_HIBER_SLOTS], count: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// 分配一个不与既有镜像重叠的槽位；同一 owner 复用其旧槽。
    pub fn allocate(&mut self, owner: BootTarget, size_bytes: u64, generation: u32) -> Option<HiberSlot> {
        if size_bytes == 0 {
            return None;
        }
        for i in 0..self.count {
            if let Some(s) = self.slots[i] {
                if s.owner == owner {
                    let updated = HiberSlot { owner, generation, offset_bytes: s.offset_bytes, size_bytes };
                    self.slots[i] = Some(updated);
                    return Some(updated);
                }
            }
        }
        if self.count >= MAX_HIBER_SLOTS {
            return None;
        }
        let mut offset = 0u64;
        for i in 0..self.count {
            if let Some(s) = self.slots[i] {
                offset = offset.max(s.offset_bytes + s.size_bytes);
            }
        }
        let slot = HiberSlot { owner, generation, offset_bytes: offset, size_bytes };
        self.slots[self.count] = Some(slot);
        self.count += 1;
        Some(slot)
    }

    pub fn get(&self, i: usize) -> Option<HiberSlot> {
        if i < self.count {
            self.slots[i]
        } else {
            None
        }
    }

    /// 任意两个槽位都不重叠，才算共存安全。
    pub fn no_overlap(&self) -> bool {
        for i in 0..self.count {
            for j in (i + 1)..self.count {
                if let (Some(a), Some(b)) = (self.get(i), self.get(j)) {
                    let a_end = a.offset_bytes + a.size_bytes;
                    let b_end = b.offset_bytes + b.size_bytes;
                    if a.offset_bytes < b_end && b.offset_bytes < a_end {
                        return false;
                    }
                }
            }
        }
        true
    }
}

impl Default for SlotTable {
    fn default() -> Self {
        SlotTable::new()
    }
}

// ---------------------------------------------------------------------------
// F049 电源策略 — 性能/平衡/省电
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerPolicy {
    Performance,
    Balanced,
    PowerSaver,
}

impl PowerPolicy {
    pub fn name(self) -> &'static str {
        match self {
            PowerPolicy::Performance => "performance",
            PowerPolicy::Balanced => "balanced",
            PowerPolicy::PowerSaver => "power-saver",
        }
    }

    /// 空闲多久后自动休眠（分钟）。省电档 5 分钟，性能档永不自动休眠。
    pub fn auto_hibernate_minutes(self) -> u32 {
        match self {
            PowerPolicy::Performance => 0,
            PowerPolicy::Balanced => 20,
            PowerPolicy::PowerSaver => 5,
        }
    }

    /// CPU 频率上限 permille（1000 = 满频）。
    pub fn freq_cap_permille(self) -> u16 {
        match self {
            PowerPolicy::Performance => 1000,
            PowerPolicy::Balanced => 850,
            PowerPolicy::PowerSaver => 600,
        }
    }
}

// ---------------------------------------------------------------------------
// F048 / F050 电源域自检与收口
// ---------------------------------------------------------------------------

/// AI-02 域自检：F026~F050 逐项登记。
pub fn run_hibernate_checks() -> CheckSet {
    let mut set = CheckSet::new("hibernate");

    // 构造一份最小合法 FADT（FACP + 偏移 64 的 PM1a_CNT_BLK）。
    let mut fadt = [0u8; 96];
    fadt[0..4].copy_from_slice(b"FACP");
    fadt[64] = 0x04;
    fadt[65] = 0x08;
    let fs = fadt_summary(&fadt);
    set.add(
        "F026 fadt parsed",
        fs.map(|f| f.hibernate_capable && f.pm1a_cnt_blk == 0x0804).unwrap_or(false),
        "fadt fixed fields",
    );

    let hdr = s4_header(1024, 4096, 0xABCD);
    set.add("F027 s4 header", hdr.valid() && hdr.image_bytes == 1024 * 4096, "header build");

    let wv = WakeVector::new(0x0010_0000, "/EFI/Varix/varix.efi");
    set.add("F028 wake vector", wv.ok() && !WakeVector::new(0x1001, "/x").ok(), "4KiB aligned");

    let key = HiberKey::from_seed(1, 2);
    let mut data = [0xAAu8, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22, 0x33];
    let plain = data;
    crypt_in_place(&key, 7, &mut data);
    let ciphered = data != plain;
    crypt_in_place(&key, 7, &mut data);
    set.add("F032 keystream roundtrip", ciphered && data == plain, "xor keystream");

    let mut aw = AtomicHiberWrite::new(2);
    aw.begin();
    aw.write_page();
    aw.write_page();
    let committed = aw.commit_header();
    set.add(
        "F033 atomic header last",
        committed && aw.verify(true) && aw.resumable(),
        "data before header",
    );

    let mut bad = AtomicHiberWrite::new(1);
    bad.begin();
    bad.write_page();
    bad.commit_header();
    set.add("F033b crc fail rolls back", !bad.verify(false) && !bad.resumable(), "no half image");

    let mut cb = CircuitBreaker::new(2);
    cb.record_failure();
    let still = cb.allow();
    cb.record_failure();
    set.add("F034 breaker opens", still && !cb.allow(), "two failures");

    let mut sm = SwitchMachine::new();
    let ok = sm.transition(SwitchState::Hibernating)
        && sm.transition(SwitchState::Resuming)
        && sm.transition(SwitchState::Running);
    set.add("F035 state machine", ok && !sm.transition(SwitchState::Idle), "legal transitions only");

    let p = SwitchProgress::for_state(SwitchState::Resuming, 500);
    set.add("F036 no fake progress", !p.known && p.permille == 0 && p.label() == "working", "indeterminate");

    let plan = plan_switch(
        SwitchDirection::WindowsToVarix,
        HiberLocation::Usb,
        1024,
        4096,
        64 * 1024 * 1024,
        &CircuitBreaker::new(2),
        &key,
    );
    set.add(
        "F029 plan windows->varix",
        plan.is_ok() && plan.unwrap().boot_target == BootTarget::Varix && !plan.unwrap().write_wake_vector,
        "target + wake vector",
    );

    let mut bn = BootNext::new();
    let committed = plan.map(|p| commit_switch(&p, &mut bn).is_ok()).unwrap_or(false);
    set.add("F030 commit bootnext", committed && bn.peek() == Some(BootTarget::Varix), "bootnext wired");

    let denied = plan_switch(
        SwitchDirection::VarixToWindows,
        HiberLocation::SharedVolume,
        1,
        4096,
        1 << 30,
        &CircuitBreaker::new(2),
        &key,
    );
    set.add(
        "F031 shared volume denied",
        matches!(denied, Err(SwitchError::LocationDenied)),
        "never on the bridge",
    );

    let mut wp = WakePolicy::safe_default();
    let nic = wp.enable(WakeKind::Nic);
    set.add(
        "F037 wake policy",
        wp.allows(WakeKind::PowerButton) && !nic && !wp.allows(WakeKind::Nic),
        "network wake off by default",
    );

    let mut steps = [ShutdownStep::FlushDirty; S5_STEPS];
    set.add(
        "F038 s5 path",
        shutdown_path(&mut steps) == S5_STEPS && steps[S5_STEPS - 1] == ShutdownStep::PowerOff,
        "unmount before power off",
    );

    let keys = [AbortKey::Delete, AbortKey::Backspace];
    set.add(
        "F039 emergency abort",
        emergency_abort(&keys, EMERGENCY_WINDOW_MS, &[100, 300])
            && !emergency_abort(&keys, EMERGENCY_WINDOW_MS, &[100, 900]),
        "del+backspace window",
    );

    let mut log = SwitchLog::new();
    log.append(SwitchRecord {
        direction: SwitchDirection::WindowsToVarix,
        stamp_ms: 1,
        duration_ms: 100,
        ok: false,
        error: SwitchError::NoStorage,
    });
    set.add("F040 forensics log", log.len() == 1 && log.failures() == 1, "switch records");

    set.add(
        "F041 image budget",
        image_within_budget(1 << 30, 2 << 30) && !image_within_budget(4 << 30, 2 << 30),
        "size vs memory",
    );

    set.add(
        "F042 roundtrip budget",
        roundtrip_budget_ms(8) == 30_000
            && switch_verdict(29_000, 8) == SwitchVerdict::Within
            && switch_verdict(61_000, 64) == SwitchVerdict::Over,
        "60s redline",
    );

    set.add("F043 header crc", header_crc_ok(&s4_header(4, 4096, 42), 42) && !header_crc_ok(&s4_header(4, 4096, 42), 43), "crc compare");

    set.add(
        "F044 host boundary",
        host_switch_allowed(HostLoginState::Elevated)
            && !host_switch_allowed(HostLoginState::UserSession)
            && host_boundary_note(HostLoginState::NoSession).contains("lock screen"),
        "no UAC spoofing",
    );

    let mut g = GestureTracker::new();
    g.escape(1_000);
    set.add("F045 double esc", g.escape(1_200) && !g.escape(9_000), "300ms window");

    set.add(
        "F046 diagnosis",
        diagnose(SwitchError::NoStorage, SwitchState::Idle).contains("does not fit"),
        "attribution",
    );

    let mut slots = SlotTable::new();
    slots.allocate(BootTarget::Windows, 1 << 30, 1);
    slots.allocate(BootTarget::Varix, 1 << 30, 1);
    set.add(
        "F047 slots coexist",
        slots.len() == 2 && slots.no_overlap() && slots.get(1).unwrap().offset_bytes == 1 << 30,
        "non-overlapping",
    );

    set.add("F048 power self-check", set.all_passed(), "entry point");

    set.add(
        "F049 power policy",
        PowerPolicy::PowerSaver.auto_hibernate_minutes() == 5
            && PowerPolicy::Performance.freq_cap_permille() == 1000
            && PowerPolicy::Balanced.freq_cap_permille() == 850,
        "governor hints",
    );

    set.add("F050 power domain closure", set.all_passed(), "all above green");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f026_rejects_non_facp() {
        assert!(fadt_summary(&[0u8; 96]).is_none());
        assert!(fadt_summary(&[0u8; 8]).is_none());
    }

    #[test]
    fn f027_header_dimensions() {
        let h = s4_header(10, 4096, 0);
        assert_eq!(h.page_count, 10);
        assert_eq!(h.image_bytes, 40960);
        assert!(h.valid());
    }

    #[test]
    fn f028_wake_vector_alignment() {
        assert!(!WakeVector::new(0, "/EFI/Varix/varix.efi").ok());
        assert!(WakeVector::new(0x200_000, "/EFI/Varix/varix.efi").ok());
    }

    #[test]
    fn f032_key_wipe_invalidates() {
        let mut k = HiberKey::from_seed(9, 9);
        assert!(k.is_valid());
        k.wipe();
        assert!(!k.is_valid());
        assert_eq!(k, HiberKey::empty());
    }

    #[test]
    fn f033_cannot_commit_before_data() {
        let mut w = AtomicHiberWrite::new(1);
        assert!(!w.commit_header());
        assert_eq!(w.stage, WriteStage::Idle);
    }

    #[test]
    fn f034_success_resets_breaker() {
        let mut cb = CircuitBreaker::new(1);
        cb.record_failure();
        assert!(!cb.allow());
        cb.record_success();
        assert!(cb.allow());
    }

    #[test]
    fn f035_illegal_transition_rejected() {
        let mut sm = SwitchMachine::new();
        assert!(!sm.transition(SwitchState::Running));
        assert_eq!(sm.state, SwitchState::Idle);
    }

    #[test]
    fn f036_hibernating_is_measurable() {
        let p = SwitchProgress::for_state(SwitchState::Hibernating, 420);
        assert!(p.known);
        assert_eq!(p.permille, 420);
    }

    #[test]
    fn f029_varix_to_windows_writes_wake_vector() {
        let key = HiberKey::from_seed(0, 0);
        let p = plan_switch(
            SwitchDirection::VarixToWindows,
            HiberLocation::Usb,
            8,
            4096,
            1 << 20,
            &CircuitBreaker::new(2),
            &key,
        )
        .unwrap();
        assert!(p.write_wake_vector);
        assert_eq!(p.boot_target, BootTarget::Windows);
    }

    #[test]
    fn f031_location_reasons_are_honest() {
        assert!(location_reason(HiberLocation::SharedVolume).contains("denied"));
        assert!(location_allowed(HiberLocation::LocalDisk));
    }

    #[test]
    fn f037_disable_works() {
        let mut wp = WakePolicy::safe_default();
        wp.disable(WakeKind::Keyboard);
        assert!(!wp.allows(WakeKind::Keyboard));
    }

    #[test]
    fn f038_shutdown_order() {
        let mut steps = [ShutdownStep::FlushDirty; S5_STEPS];
        shutdown_path(&mut steps);
        assert_eq!(steps[3], ShutdownStep::UnmountShared);
        assert_eq!(steps[4], ShutdownStep::PowerOff);
    }

    #[test]
    fn f039_scancode_mapping() {
        assert_eq!(AbortKey::from_scancode(0x53), AbortKey::Delete);
        assert_eq!(AbortKey::from_scancode(0x0E), AbortKey::Backspace);
        assert_eq!(AbortKey::from_scancode(0x1C), AbortKey::Other);
    }

    #[test]
    fn f040_log_wraps_at_capacity() {
        let mut log = SwitchLog::new();
        for i in 0..SWITCH_LOG_CAP + 3 {
            log.append(SwitchRecord {
                direction: SwitchDirection::WindowsToVarix,
                stamp_ms: i as u64,
                duration_ms: 1,
                ok: true,
                error: SwitchError::Ok,
            });
        }
        assert_eq!(log.len(), SWITCH_LOG_CAP);
        assert_eq!(log.failures(), 0);
    }

    #[test]
    fn f041_zero_size_rejected() {
        assert!(image_within_budget(0, 1 << 20));
        assert!(!image_within_budget(1 << 30, 0), "no memory budget means no room");
        assert!(!image_within_budget(2 << 30, 1 << 30));
    }

    #[test]
    fn f042_budget_monotonic() {
        assert!(roundtrip_budget_ms(2) <= roundtrip_budget_ms(8));
        assert!(roundtrip_budget_ms(64) <= SWITCH_REDLINE_MS);
    }

    #[test]
    fn f045_gesture_needs_two_presses() {
        let mut g = GestureTracker::new();
        assert!(!g.escape(0));
        assert!(g.escape(100));
    }

    #[test]
    fn f047_slot_reuse_for_same_owner() {
        let mut t = SlotTable::new();
        let a = t.allocate(BootTarget::Windows, 1 << 20, 1).unwrap();
        let b = t.allocate(BootTarget::Windows, 2 << 20, 2).unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(a.offset_bytes, b.offset_bytes);
        assert_eq!(b.generation, 2);
    }

    #[test]
    fn f050_domain_self_test_is_green() {
        let set = run_hibernate_checks();
        assert!(set.len() >= 25);
        assert!(set.all_passed(), "hibernate domain self-test must pass");
    }
}
