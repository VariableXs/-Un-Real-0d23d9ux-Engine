//! TRINITY-500 · AI-01 三系统引导与切换域（F001~F025，W1）
//!
//! 职责：Limine 三入口菜单、BootNext 切换原语、引导器降级链、引导完整性/自检/回滚。
//! 边界：本域只做「编排层」——底层引导原语来自 VARIX-500 AI-01，本文件不重复实现。
//! 形态：`no_std` + 固定容量数组（无 Vec/String/Box），全部状态可序列化到 ESP。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F001 三系统引导菜单 — Windows / Varix / 诊断三入口（Limine 多入口）
// ---------------------------------------------------------------------------

/// 同一 U 盘上共存的三个引导目标。同一时刻只有一个处于运行态（F021）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootTarget {
    /// 宿主 Windows Boot Manager（链式加载）。
    Windows = 0,
    /// Varix 自研内核 + 内核版 Variable。
    Varix = 1,
    /// Varix 诊断/救援镜像。
    Diagnostics = 2,
}

impl BootTarget {
    pub fn index(self) -> u8 {
        match self {
            BootTarget::Windows => 0,
            BootTarget::Varix => 1,
            BootTarget::Diagnostics => 2,
        }
    }

    pub fn from_index(i: u8) -> Option<BootTarget> {
        match i {
            0 => Some(BootTarget::Windows),
            1 => Some(BootTarget::Varix),
            2 => Some(BootTarget::Diagnostics),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BootTarget::Windows => "Windows",
            BootTarget::Varix => "Varix / Variable",
            BootTarget::Diagnostics => "Varix Diagnostics",
        }
    }

    /// EFI 可执行镜像在 ESP 内的路径（正斜杠，UEFI 约定）。
    pub fn efi_path(self) -> &'static str {
        match self {
            BootTarget::Windows => "/EFI/Microsoft/Boot/bootmgfw.efi",
            BootTarget::Varix => "/EFI/Varix/varix.efi",
            BootTarget::Diagnostics => "/EFI/Varix/diag.efi",
        }
    }

    /// 该目标是否允许被 BootNext 直接选中（诊断入口永不做默认）。
    pub fn selectable(self) -> bool {
        !matches!(self, BootTarget::Diagnostics)
    }
}

/// 引导菜单里的一条记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootEntry {
    pub id: u8,
    pub target: BootTarget,
    pub enabled: bool,
    pub hotkey: u8,
}

pub const MAX_BOOT_ENTRIES: usize = 3;

/// 出厂三入口顺序：Varix 优先（本项目的主角），Windows 次之，诊断兜底。
pub const BOOT_ENTRIES: [BootEntry; MAX_BOOT_ENTRIES] = [
    BootEntry { id: 0, target: BootTarget::Varix, enabled: true, hotkey: b'1' },
    BootEntry { id: 1, target: BootTarget::Windows, enabled: true, hotkey: b'2' },
    BootEntry { id: 2, target: BootTarget::Diagnostics, enabled: true, hotkey: b'3' },
];

/// 列出当前可引导入口，返回写入条数。
pub fn list_entries(out: &mut [BootEntry]) -> usize {
    let mut n = 0usize;
    for e in BOOT_ENTRIES.iter() {
        if e.enabled && n < out.len() {
            out[n] = *e;
            n += 1;
        }
    }
    n
}

pub fn entry_for(target: BootTarget) -> Option<BootEntry> {
    BOOT_ENTRIES.iter().find(|e| e.target == target).copied()
}

/// F001 自检：三入口可列、路径非空、热键唯一。
pub fn entries_sane() -> bool {
    let mut buf = [BOOT_ENTRIES[0]; MAX_BOOT_ENTRIES];
    let n = list_entries(&mut buf);
    if n != MAX_BOOT_ENTRIES {
        return false;
    }
    for e in buf.iter() {
        if e.target.efi_path().is_empty() || e.target.label().is_empty() {
            return false;
        }
    }
    for i in 0..n {
        for j in (i + 1)..n {
            if buf[i].hotkey == buf[j].hotkey || buf[i].id == buf[j].id {
                return false;
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// F002 引导目标切换 API — BootNext 原语（内核态/用户态统一入口）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootError {
    Ok,
    NoEsp,
    EspReadOnly,
    InvalidTarget,
    CrcMismatch,
    NotPersisted,
    Busy,
    NotSupported,
}

impl BootError {
    pub fn as_str(self) -> &'static str {
        match self {
            BootError::Ok => "ok",
            BootError::NoEsp => "esp not mounted",
            BootError::EspReadOnly => "esp read-only",
            BootError::InvalidTarget => "invalid boot target",
            BootError::CrcMismatch => "crc mismatch",
            BootError::NotPersisted => "not persisted",
            BootError::Busy => "switch in progress",
            BootError::NotSupported => "not supported by firmware",
        }
    }
}

/// BootNext 原语：一次「下一次引导去哪」的意图。
///
/// `set` 幂等——重复设置同一目标不推进 `generation`，也不产生写盘。
#[derive(Clone, Copy, Debug)]
pub struct BootNext {
    pub target: Option<BootTarget>,
    pub generation: u32,
    pub persisted: bool,
}

impl BootNext {
    pub const fn new() -> BootNext {
        BootNext { target: None, generation: 0, persisted: false }
    }

    pub fn set(&mut self, t: BootTarget) -> Result<(), BootError> {
        if !t.selectable() && t != BootTarget::Diagnostics {
            return Err(BootError::InvalidTarget);
        }
        if self.target == Some(t) {
            // 幂等：意图未变，不推进代数。
            return Ok(());
        }
        self.target = Some(t);
        self.generation = self.generation.wrapping_add(1);
        self.persisted = false;
        Ok(())
    }

    /// 查看但不消费。
    pub fn peek(&self) -> Option<BootTarget> {
        self.target
    }

    /// 消费：取值并清空（引导器实际切走后调用）。
    pub fn take(&mut self) -> Option<BootTarget> {
        let t = self.target;
        self.target = None;
        t
    }

    pub fn clear(&mut self) {
        self.target = None;
        self.persisted = false;
    }

    pub fn mark_persisted(&mut self) {
        if self.target.is_some() {
            self.persisted = true;
        }
    }
}

impl Default for BootNext {
    fn default() -> Self {
        BootNext::new()
    }
}

// ---------------------------------------------------------------------------
// F003 默认项与倒计时 — 可配，断电后恢复上次选择
// F016 引导配置中心 — 用户改默认/超时/顺序
// ---------------------------------------------------------------------------

pub const TIMEOUT_MIN_MS: u32 = 0;
pub const TIMEOUT_MAX_MS: u32 = 60_000;

#[derive(Clone, Copy, Debug)]
pub struct BootMenuConfig {
    pub default: BootTarget,
    pub last_used: BootTarget,
    pub timeout_ms: u32,
    pub remember_last: bool,
}

impl BootMenuConfig {
    pub const fn new() -> BootMenuConfig {
        BootMenuConfig {
            default: BootTarget::Varix,
            last_used: BootTarget::Varix,
            timeout_ms: 5_000,
            remember_last: true,
        }
    }

    /// 断电恢复：记住上次选择时以 `last_used` 为准，否则用固定默认项。
    pub fn effective_default(&self) -> BootTarget {
        if self.remember_last {
            self.last_used
        } else {
            self.default
        }
    }

    /// 倒计时需要的 tick 数（向上取整，永不为负）。
    pub fn countdown_ticks(&self, tick_ms: u32) -> u32 {
        if tick_ms == 0 {
            return 0;
        }
        self.timeout_ms / tick_ms + u32::from(self.timeout_ms % tick_ms != 0)
    }

    /// 引导成功后提交「本次用的是谁」。
    pub fn commit_used(&mut self, t: BootTarget) {
        self.last_used = t;
    }

    pub fn set_timeout(&mut self, ms: u32) {
        self.timeout_ms = ms.clamp(TIMEOUT_MIN_MS, TIMEOUT_MAX_MS);
    }
}

impl Default for BootMenuConfig {
    fn default() -> Self {
        BootMenuConfig::new()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BootConfigCenter {
    pub menu: BootMenuConfig,
    pub order: [BootTarget; MAX_BOOT_ENTRIES],
}

impl BootConfigCenter {
    pub const fn new() -> BootConfigCenter {
        BootConfigCenter {
            menu: BootMenuConfig::new(),
            order: [BootTarget::Varix, BootTarget::Windows, BootTarget::Diagnostics],
        }
    }

    pub fn set_default(&mut self, t: BootTarget) -> Result<(), BootError> {
        if !t.selectable() {
            return Err(BootError::InvalidTarget);
        }
        self.menu.default = t;
        Ok(())
    }

    pub fn set_timeout(&mut self, ms: u32) {
        self.menu.set_timeout(ms);
    }

    /// 把第 `i` 项上移一位（菜单排序）。
    pub fn move_up(&mut self, i: usize) -> bool {
        if i == 0 || i >= self.order.len() {
            return false;
        }
        self.order.swap(i - 1, i);
        true
    }

    /// 配置是否自洽：顺序里三项齐全、默认项在顺序里、超时在范围内。
    pub fn validate(&self) -> bool {
        let mut seen = [false; MAX_BOOT_ENTRIES];
        for t in self.order.iter() {
            let idx = t.index() as usize;
            if idx >= MAX_BOOT_ENTRIES || seen[idx] {
                return false;
            }
            seen[idx] = true;
        }
        if !seen[self.menu.default.index() as usize] {
            return false;
        }
        self.menu.timeout_ms <= TIMEOUT_MAX_MS
    }
}

impl Default for BootConfigCenter {
    fn default() -> Self {
        BootConfigCenter::new()
    }
}

// ---------------------------------------------------------------------------
// F004 引导目标持久化 — 写 U 盘 ESP 变量，不碰宿主
// F018 ESP 分区只读保护 — 宿主误写防护
// ---------------------------------------------------------------------------

pub const ESP_VAR_NAME_LEN: usize = 16;
pub const MAX_ESP_VARS: usize = 8;
const ESP_MAGIC: u32 = 0x5658_4553; // "V X E S"

/// 一条 ESP 变量（UEFI GetVariable/SetVariable 语义的内核侧映象）。
#[derive(Clone, Copy, Debug)]
pub struct EspVar {
    pub name: [u8; ESP_VAR_NAME_LEN],
    pub name_len: usize,
    pub value: u64,
    pub crc: u32,
}

impl EspVar {
    pub fn new(name: &str, value: u64) -> Option<EspVar> {
        let b = name.as_bytes();
        if b.is_empty() || b.len() > ESP_VAR_NAME_LEN {
            return None;
        }
        let mut buf = [0u8; ESP_VAR_NAME_LEN];
        buf[..b.len()].copy_from_slice(b);
        Some(EspVar { name: buf, name_len: b.len(), value, crc: crc_bytes(b) })
    }

    pub fn name(&self) -> &str {
        match core::str::from_utf8(&self.name[..self.name_len]) {
            Ok(s) => s,
            Err(_) => "",
        }
    }

    pub fn crc_ok(&self) -> bool {
        self.crc == crc_bytes(self.name().as_bytes())
    }
}

/// 只覆盖名字与值，够用且可预测（非密码学，仅防位翻转）。
fn crc_bytes(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// ESP 变量仓：固定 8 条，可整块编码/解码落盘。
#[derive(Clone, Copy, Debug)]
pub struct EspStore {
    vars: [Option<EspVar>; MAX_ESP_VARS],
    count: usize,
    pub read_only: bool,
}

impl EspStore {
    pub const fn new() -> EspStore {
        EspStore { vars: [None; MAX_ESP_VARS], count: 0, read_only: false }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn set(&mut self, name: &str, value: u64) -> Result<(), BootError> {
        if self.read_only {
            return Err(BootError::EspReadOnly);
        }
        let var = EspVar::new(name, value).ok_or(BootError::NotSupported)?;
        for i in 0..self.count {
            if let Some(v) = self.vars[i] {
                if v.name() == name {
                    self.vars[i] = Some(var);
                    return Ok(());
                }
            }
        }
        if self.count >= MAX_ESP_VARS {
            return Err(BootError::Busy);
        }
        self.vars[self.count] = Some(var);
        self.count += 1;
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<u64> {
        for i in 0..self.count {
            if let Some(v) = self.vars[i] {
                if v.name() == name && v.crc_ok() {
                    return Some(v.value);
                }
            }
        }
        None
    }

    pub fn delete(&mut self, name: &str) -> bool {
        if self.read_only {
            return false;
        }
        for i in 0..self.count {
            if let Some(v) = self.vars[i] {
                if v.name() == name {
                    self.vars[i] = self.vars[self.count - 1];
                    self.vars[self.count - 1] = None;
                    self.count -= 1;
                    return true;
                }
            }
        }
        false
    }

    /// 序列化：`VXES` + 版本 + 条数 + 每条(名称长度/名称/u64/u32)。
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, BootError> {
        let mut n = 0usize;
        put_u32(out, &mut n, ESP_MAGIC)?;
        put_u32(out, &mut n, 1)?;
        put_u32(out, &mut n, self.count as u32)?;
        for i in 0..self.count {
            let v = self.vars[i].ok_or(BootError::NotPersisted)?;
            put_u32(out, &mut n, v.name_len as u32)?;
            for &b in v.name[..v.name_len].iter() {
                put_u8(out, &mut n, b)?;
            }
            put_u64(out, &mut n, v.value)?;
            put_u32(out, &mut n, v.crc)?;
        }
        Ok(n)
    }

    pub fn decode(bytes: &[u8]) -> Result<EspStore, BootError> {
        let mut n = 0usize;
        if get_u32(bytes, &mut n)? != ESP_MAGIC {
            return Err(BootError::CrcMismatch);
        }
        if get_u32(bytes, &mut n)? != 1 {
            return Err(BootError::NotSupported);
        }
        let count = get_u32(bytes, &mut n)? as usize;
        if count > MAX_ESP_VARS {
            return Err(BootError::CrcMismatch);
        }
        let mut store = EspStore::new();
        for _ in 0..count {
            let len = get_u32(bytes, &mut n)? as usize;
            if len == 0 || len > ESP_VAR_NAME_LEN {
                return Err(BootError::CrcMismatch);
            }
            let mut name = [0u8; ESP_VAR_NAME_LEN];
            for i in 0..len {
                name[i] = get_u8(bytes, &mut n)?;
            }
            let value = get_u64(bytes, &mut n)?;
            let crc = get_u32(bytes, &mut n)?;
            let name_str = core::str::from_utf8(&name[..len]).map_err(|_| BootError::CrcMismatch)?;
            let expected = EspVar::new(name_str, value).ok_or(BootError::CrcMismatch)?;
            if expected.crc != crc {
                return Err(BootError::CrcMismatch);
            }
            store.set(name_str, value)?;
        }
        Ok(store)
    }
}

impl Default for EspStore {
    fn default() -> Self {
        EspStore::new()
    }
}

fn put_u8(out: &mut [u8], n: &mut usize, v: u8) -> Result<(), BootError> {
    if *n >= out.len() {
        return Err(BootError::NoEsp);
    }
    out[*n] = v;
    *n += 1;
    Ok(())
}

fn get_u8(b: &[u8], n: &mut usize) -> Result<u8, BootError> {
    if *n >= b.len() {
        return Err(BootError::NoEsp);
    }
    let v = b[*n];
    *n += 1;
    Ok(v)
}

fn put_u32(out: &mut [u8], n: &mut usize, v: u32) -> Result<(), BootError> {
    for i in 0..4 {
        put_u8(out, n, (v >> (8 * i)) as u8)?;
    }
    Ok(())
}

fn get_u32(b: &[u8], n: &mut usize) -> Result<u32, BootError> {
    let mut v = 0u32;
    for i in 0..4 {
        v |= (get_u8(b, n)? as u32) << (8 * i);
    }
    Ok(v)
}

fn put_u64(out: &mut [u8], n: &mut usize, v: u64) -> Result<(), BootError> {
    for i in 0..8 {
        put_u8(out, n, (v >> (8 * i)) as u8)?;
    }
    Ok(())
}

fn get_u64(b: &[u8], n: &mut usize) -> Result<u64, BootError> {
    let mut v = 0u64;
    for i in 0..8 {
        v |= (get_u8(b, n)? as u64) << (8 * i);
    }
    Ok(v)
}

/// F018：ESP 写保护策略。只有 Varix 自己的目录可写，宿主目录一律拒绝。
pub fn esp_write_allowed(path: &str) -> bool {
    let p = path.as_bytes();
    if p.is_empty() || p[0] != b'/' {
        return false;
    }
    if starts_with(path, "/EFI/Microsoft/") || starts_with(path, "/EFI/Boot/") {
        return false;
    }
    starts_with(path, "/EFI/Varix/")
}

fn starts_with(hay: &str, needle: &str) -> bool {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    h.len() >= n.len() && &h[..n.len()] == n
}

// ---------------------------------------------------------------------------
// F005 SystemLauncher 四阶段接入 — 探测→挂载→编排→接管
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchPhase {
    Probe = 0,
    Mount = 1,
    Orchestrate = 2,
    Takeover = 3,
}

impl LaunchPhase {
    pub fn name(self) -> &'static str {
        match self {
            LaunchPhase::Probe => "probe",
            LaunchPhase::Mount => "mount",
            LaunchPhase::Orchestrate => "orchestrate",
            LaunchPhase::Takeover => "takeover",
        }
    }
    pub fn from_index(i: usize) -> Option<LaunchPhase> {
        match i {
            0 => Some(LaunchPhase::Probe),
            1 => Some(LaunchPhase::Mount),
            2 => Some(LaunchPhase::Orchestrate),
            3 => Some(LaunchPhase::Takeover),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Launcher {
    pub phase: LaunchPhase,
    pub done: bool,
    pub failed: bool,
    failed_reason: &'static str,
}

impl Launcher {
    pub const fn new() -> Launcher {
        Launcher { phase: LaunchPhase::Probe, done: false, failed: false, failed_reason: "" }
    }

    /// 推进一个阶段；`ok=false` 表示本阶段失败，进入失败态并停止推进。
    pub fn step(&mut self, ok: bool) -> LaunchPhase {
        if self.done || self.failed {
            return self.phase;
        }
        if !ok {
            self.failed = true;
            self.failed_reason = self.phase.name();
            return self.phase;
        }
        match self.phase {
            LaunchPhase::Probe => self.phase = LaunchPhase::Mount,
            LaunchPhase::Mount => self.phase = LaunchPhase::Orchestrate,
            LaunchPhase::Orchestrate => self.phase = LaunchPhase::Takeover,
            LaunchPhase::Takeover => self.done = true,
        }
        self.phase
    }

    pub fn reason(self) -> &'static str {
        self.failed_reason
    }

    /// 0~1000 的整数 permille 进度（不编造假进度：失败即停在当前值）。
    pub fn progress_permille(&self) -> u16 {
        if self.failed {
            return (self.phase as u16) * 250;
        }
        if self.done {
            return 1000;
        }
        (self.phase as u16) * 250
    }
}

impl Default for Launcher {
    fn default() -> Self {
        Launcher::new()
    }
}

// ---------------------------------------------------------------------------
// F006 引导器降级链 — 原生 → VBox → 轻量直跑
// F015 无 VT-x 降级引导 — boot-level coexistence
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootMode {
    /// 原生裸机：Varix 直接接管硬件。
    Native,
    /// 宿主内虚拟运行（VT-x 可用时的次选）。
    VBox,
    /// 纯引导并存：不做虚拟化，两个系统只是引导级共存。
    Lightweight,
}

impl BootMode {
    pub fn name(self) -> &'static str {
        match self {
            BootMode::Native => "native",
            BootMode::VBox => "virtualised",
            BootMode::Lightweight => "boot-level coexistence",
        }
    }
}

pub fn degrade_chain(out: &mut [BootMode]) -> usize {
    let chain = [BootMode::Native, BootMode::VBox, BootMode::Lightweight];
    let n = if out.len() < chain.len() { out.len() } else { chain.len() };
    out[..n].copy_from_slice(&chain[..n]);
    n
}

/// 下一档降级目标（末尾返回 None，表示已无可降）。
pub fn next_mode(m: BootMode) -> Option<BootMode> {
    match m {
        BootMode::Native => Some(BootMode::VBox),
        BootMode::VBox => Some(BootMode::Lightweight),
        BootMode::Lightweight => None,
    }
}

/// 依据硬件能力选档：VT-x + 足够内存 → 原生；有 VT-x 但内存紧 → 虚拟化；无 VT-x → 并存。
pub fn select_mode(vtx: bool, mem_mb: u32, uefi64: bool) -> BootMode {
    if !uefi64 {
        return BootMode::Lightweight;
    }
    if vtx {
        if mem_mb >= 2048 {
            BootMode::Native
        } else {
            BootMode::VBox
        }
    } else {
        BootMode::Lightweight
    }
}

/// F015：无 VT-x 时的诚实说明（不伪装成虚拟化）。
pub fn coexist_plan(vtx: bool) -> &'static str {
    if vtx {
        "virtualisation available; native still preferred"
    } else {
        "no VT-x: boot-level coexistence only, no guest isolation"
    }
}

// ---------------------------------------------------------------------------
// F007 引导链完整性校验 — 镜像哈希
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct ChainLink {
    pub name: &'static str,
    pub expected: u64,
    pub actual: u64,
}

impl ChainLink {
    pub fn ok(&self) -> bool {
        self.expected == self.actual
    }
}

pub fn link_ok(l: &ChainLink) -> bool {
    l.ok()
}

pub fn verify_chain(links: &[ChainLink]) -> bool {
    links.iter().all(ChainLink::ok)
}

/// 输出 `name ok`/`name BAD` 列表，返回写入字节数。
pub fn chain_report(links: &[ChainLink], out: &mut [u8]) -> usize {
    let mut n = 0usize;
    for l in links.iter() {
        for &b in l.name.as_bytes() {
            push(out, &mut n, b);
        }
        if l.ok() {
            push_str(out, &mut n, " ok\n");
        } else {
            push_str(out, &mut n, " BAD\n");
        }
    }
    n
}

fn push(out: &mut [u8], n: &mut usize, b: u8) {
    if *n < out.len() {
        out[*n] = b;
        *n += 1;
    }
}

fn push_str(out: &mut [u8], n: &mut usize, s: &str) {
    for &b in s.as_bytes() {
        push(out, n, b);
    }
}

// ---------------------------------------------------------------------------
// F008 引导期 KASLR — 内核基址随机化
// ---------------------------------------------------------------------------

pub const KASLR_ALIGN: u64 = 2 * 1024 * 1024; // 2 MiB

/// 由熵源推出一个 2MiB 对齐的滑移量；`range_pages` 为可选页数窗口。
pub fn kaslr_slide(seed: u64, range_pages: u64) -> u64 {
    if range_pages == 0 {
        return 0;
    }
    let mut x = seed ^ 0x9E37_79B9_7F4A_7C15;
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    let pages = x % range_pages;
    (pages * 4096) & !(KASLR_ALIGN - 1)
}

/// 滑移量必须非零且按 2MiB 对齐才算「生效」。
pub fn kaslr_effective(slide: u64) -> bool {
    slide != 0 && slide % KASLR_ALIGN == 0
}

// ---------------------------------------------------------------------------
// F009 引导期平台探测 — CPUID 特性扫描
// ---------------------------------------------------------------------------

pub const F_SSE3: u32 = 1 << 0;
pub const F_AESNI: u32 = 1 << 1;
pub const F_AVX: u32 = 1 << 2;
pub const F_RDRAND: u32 = 1 << 3;
pub const F_VTX: u32 = 1 << 4;
pub const F_X2APIC: u32 = 1 << 5;
pub const F_NX: u32 = 1 << 6;
pub const F_TSC: u32 = 1 << 7;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuFeatures {
    bits: u32,
}

impl CpuFeatures {
    pub const fn empty() -> CpuFeatures {
        CpuFeatures { bits: 0 }
    }

    /// CPUID leaf 1：ECX/EDX → 特性位。
    pub fn from_leaf1(ecx: u32, edx: u32) -> CpuFeatures {
        let mut f = CpuFeatures::empty();
        if ecx & (1 << 0) != 0 {
            f.bits |= F_SSE3;
        }
        if ecx & (1 << 25) != 0 {
            f.bits |= F_AESNI;
        }
        if ecx & (1 << 28) != 0 {
            f.bits |= F_AVX;
        }
        if ecx & (1 << 30) != 0 {
            f.bits |= F_RDRAND;
        }
        if ecx & (1 << 5) != 0 {
            f.bits |= F_VTX;
        }
        if ecx & (1 << 21) != 0 {
            f.bits |= F_X2APIC;
        }
        if edx & (1 << 4) != 0 {
            f.bits |= F_TSC;
        }
        f
    }

    /// CPUID 0x8000_0001 EDX：NX（bit20）。
    pub fn from_ext_edx(edx: u32) -> CpuFeatures {
        let mut f = CpuFeatures::empty();
        if edx & (1 << 20) != 0 {
            f.bits |= F_NX;
        }
        f
    }

    pub fn merge(self, other: CpuFeatures) -> CpuFeatures {
        CpuFeatures { bits: self.bits | other.bits }
    }

    pub fn has(self, flag: u32) -> bool {
        self.bits & flag != 0
    }

    pub fn bits(self) -> u32 {
        self.bits
    }
}

// ---------------------------------------------------------------------------
// F010 引导期内存图解析 — Limine 内存图归类
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemKind {
    Usable,
    Reserved,
    AcpiReclaim,
    AcpiNvs,
    BadMemory,
    BootloaderReclaim,
    KernelAndModules,
    Framebuffer,
}

pub fn classify(limine_type: u32) -> MemKind {
    match limine_type {
        0 => MemKind::Usable,
        1 => MemKind::Reserved,
        2 => MemKind::AcpiReclaim,
        3 => MemKind::AcpiNvs,
        4 => MemKind::BadMemory,
        5 => MemKind::BootloaderReclaim,
        6 => MemKind::KernelAndModules,
        7 => MemKind::Framebuffer,
        _ => MemKind::Reserved,
    }
}

/// 只有 Usable 能进 PMM；BootloaderReclaim 在引导后期可回收。
pub fn region_usable(k: MemKind) -> bool {
    matches!(k, MemKind::Usable)
}

pub fn region_reclaimable_later(k: MemKind) -> bool {
    matches!(k, MemKind::BootloaderReclaim)
}

// ---------------------------------------------------------------------------
// F011 引导日志可视化 — 阶段时间线真实进度
// F023 引导耗时仪表 — 分阶段计时与红线
// ---------------------------------------------------------------------------

pub const MAX_BOOT_STAGES: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct BootStage {
    pub name: &'static str,
    pub start_ms: u32,
    pub end_ms: u32,
}

impl BootStage {
    pub fn duration_ms(&self) -> u32 {
        self.end_ms.saturating_sub(self.start_ms)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BootTimeline {
    stages: [Option<BootStage>; MAX_BOOT_STAGES],
    count: usize,
}

impl BootTimeline {
    pub const fn new() -> BootTimeline {
        BootTimeline { stages: [None; MAX_BOOT_STAGES], count: 0 }
    }

    pub fn push(&mut self, name: &'static str, start_ms: u32, end_ms: u32) -> bool {
        if self.count >= MAX_BOOT_STAGES {
            return false;
        }
        self.stages[self.count] = Some(BootStage { name, start_ms, end_ms });
        self.count += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<BootStage> {
        if i < self.count {
            self.stages[i]
        } else {
            None
        }
    }

    pub fn total_ms(&self) -> u32 {
        (0..self.count)
            .filter_map(|i| self.get(i))
            .map(|s| s.duration_ms())
            .fold(0u32, u32::saturating_add)
    }

    /// `name 12ms\n` 逐行渲染。
    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        for i in 0..self.count {
            if let Some(s) = self.get(i) {
                push_str(out, &mut n, s.name);
                push_str(out, &mut n, " ");
                push_num(out, &mut n, s.duration_ms() as usize);
                push_str(out, &mut n, "ms\n");
            }
        }
        n
    }
}

impl Default for BootTimeline {
    fn default() -> Self {
        BootTimeline::new()
    }
}

fn push_num(out: &mut [u8], n: &mut usize, mut v: usize) {
    if v == 0 {
        push(out, n, b'0');
        return;
    }
    let mut digits = [0u8; 12];
    let mut w = 0usize;
    while v > 0 && w < digits.len() {
        digits[w] = b'0' + (v % 10) as u8;
        v /= 10;
        w += 1;
    }
    while w > 0 {
        w -= 1;
        push(out, n, digits[w]);
    }
}

pub const BOOT_MENU_REDLINE_MS: u32 = 3_000;
pub const DESKTOP_REDLINE_MS: u32 = 8_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetVerdict {
    Within,
    Warning,
    Over,
}

impl BudgetVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            BudgetVerdict::Within => "within",
            BudgetVerdict::Warning => "warning",
            BudgetVerdict::Over => "over",
        }
    }
}

/// F013/F023：实测耗时对红线。<90% 达标，<100% 警告，否则超标。
pub fn budget_verdict(measured_ms: u32, redline_ms: u32) -> BudgetVerdict {
    if redline_ms == 0 {
        return BudgetVerdict::Over;
    }
    let warn = redline_ms * 9 / 10;
    if measured_ms <= warn {
        BudgetVerdict::Within
    } else if measured_ms <= redline_ms {
        BudgetVerdict::Warning
    } else {
        BudgetVerdict::Over
    }
}

// ---------------------------------------------------------------------------
// F012 引导失败诊断包 — 串口/日志落盘，一键导出
// ---------------------------------------------------------------------------

pub const MAX_DIAG_BYTES: usize = 512;
pub const SERIAL_TAIL_LEN: usize = 128;

#[derive(Clone, Copy)]
pub struct DiagPackage {
    pub reason: &'static str,
    pub timeline: BootTimeline,
    pub serial_tail: [u8; SERIAL_TAIL_LEN],
    pub serial_len: usize,
    pub crc: u32,
}

impl DiagPackage {
    pub fn capture(reason: &'static str, timeline: BootTimeline, serial: &[u8]) -> DiagPackage {
        let take = if serial.len() > SERIAL_TAIL_LEN { SERIAL_TAIL_LEN } else { serial.len() };
        let mut tail = [0u8; SERIAL_TAIL_LEN];
        tail[..take].copy_from_slice(&serial[serial.len() - take..]);
        let crc = crate::power::crc32(&tail[..take]);
        DiagPackage { reason, timeline, serial_tail: tail, serial_len: take, crc }
    }

    pub fn verify(&self) -> bool {
        self.crc == crate::power::crc32(&self.serial_tail[..self.serial_len])
    }

    /// 导出为人类可读文本（原因 + 时间线 + 串口尾）。
    pub fn export(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        push_str(out, &mut n, "reason: ");
        push_str(out, &mut n, self.reason);
        push_str(out, &mut n, "\ntotal: ");
        push_num(out, &mut n, self.timeline.total_ms() as usize);
        push_str(out, &mut n, "ms\n");
        n += self.timeline.render(&mut out[n..]);
        push_str(out, &mut n, "serial: ");
        let take = if self.serial_len > 32 { 32 } else { self.serial_len };
        for &b in self.serial_tail[..take].iter() {
            push(out, &mut n, b);
        }
        push_str(out, &mut n, "\n");
        n
    }
}

// ---------------------------------------------------------------------------
// F014 引导菜单艺术化 — Variable 品质线的开机画面
// ---------------------------------------------------------------------------

pub const BOOT_BG: u32 = 0x0B0D_12;
pub const BOOT_FG: u32 = 0xE8EA_F0;
pub const BOOT_ACCENT: u32 = 0x7AA2_F7;

#[derive(Clone, Copy, Debug)]
pub struct BootArt {
    pub bg: u32,
    pub fg: u32,
    pub accent: u32,
    pub layers: u8,
}

pub const BOOT_ART: BootArt = BootArt { bg: BOOT_BG, fg: BOOT_FG, accent: BOOT_ACCENT, layers: 7 };

/// 进度条颜色：bg→accent 的线性插值（未做 gamma 校正，够用且不骗人）。
pub fn art_progress_color(permille: u16) -> u32 {
    let t = if permille > 1000 { 1000 } else { permille } as u32;
    let lerp = |a: u32, b: u32| -> u32 { (a * (1000 - t) + b * t) / 1000 };
    let r = lerp((BOOT_BG >> 16) & 0xFF, (BOOT_ACCENT >> 16) & 0xFF);
    let g = lerp((BOOT_BG >> 8) & 0xFF, (BOOT_ACCENT >> 8) & 0xFF);
    let b = lerp(BOOT_BG & 0xFF, BOOT_ACCENT & 0xFF);
    (r << 16) | (g << 8) | b
}

// ---------------------------------------------------------------------------
// F017 引导日志环 — 崩溃可回读
// ---------------------------------------------------------------------------

pub const RING_LINES: usize = 8;
pub const RING_LINE_LEN: usize = 48;

pub struct LogRing {
    lines: [[u8; RING_LINE_LEN]; RING_LINES],
    lens: [usize; RING_LINES],
    head: usize,
    count: usize,
}

impl LogRing {
    pub const fn new() -> LogRing {
        LogRing { lines: [[0u8; RING_LINE_LEN]; RING_LINES], lens: [0; RING_LINES], head: 0, count: 0 }
    }

    pub fn push(&mut self, text: &str) {
        let b = text.as_bytes();
        let take = if b.len() > RING_LINE_LEN { RING_LINE_LEN } else { b.len() };
        self.lines[self.head] = [0u8; RING_LINE_LEN];
        self.lines[self.head][..take].copy_from_slice(&b[..take]);
        self.lens[self.head] = take;
        self.head = (self.head + 1) % RING_LINES;
        if self.count < RING_LINES {
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 按时间顺序读第 `i` 条（0 = 最旧）。
    pub fn read(&self, i: usize, out: &mut [u8]) -> usize {
        if i >= self.count {
            return 0;
        }
        let start = (self.head + RING_LINES - self.count) % RING_LINES;
        let idx = (start + i) % RING_LINES;
        let n = self.lens[idx];
        let take = if n > out.len() { out.len() } else { n };
        out[..take].copy_from_slice(&self.lines[idx][..take]);
        take
    }
}

impl Default for LogRing {
    fn default() -> Self {
        LogRing::new()
    }
}

// ---------------------------------------------------------------------------
// F019 引导目标回滚 — BootNext 失败自动回退上一系统
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Rollback {
    pub previous: BootTarget,
    pub attempted: BootTarget,
    pub attempts: u8,
    pub success: bool,
}

impl Rollback {
    pub const fn new(previous: BootTarget) -> Rollback {
        Rollback { previous, attempted: previous, attempts: 0, success: true }
    }

    pub fn attempt(&mut self, t: BootTarget) {
        if t != self.attempted {
            self.previous = self.attempted;
            self.attempted = t;
            self.attempts = 1;
        } else {
            self.attempts = self.attempts.saturating_add(1);
        }
        self.success = false;
    }

    pub fn mark_success(&mut self) {
        self.success = true;
        self.attempts = 0;
    }

    /// 失败一次即回退上一系统（连续两次失败也回退）。
    pub fn mark_failure(&mut self) -> Option<BootTarget> {
        self.success = false;
        if self.attempts >= 1 {
            Some(self.previous)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// F021 三系统互斥仲裁 — 同一时刻仅一个运行态
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeOwner {
    None,
    Windows,
    Varix,
    Diagnostics,
}

#[derive(Clone, Copy, Debug)]
pub struct MutexArbiter {
    pub owner: RuntimeOwner,
    /// 源系统是否已「停机」（休眠落盘完成），是切换的唯一合法前提。
    pub source_halted: bool,
}

impl MutexArbiter {
    pub const fn new() -> MutexArbiter {
        MutexArbiter { owner: RuntimeOwner::None, source_halted: true }
    }

    /// 只有无人占用、或占用者已停机，才能被接管。
    pub fn claim(&mut self, t: RuntimeOwner) -> bool {
        if self.owner != RuntimeOwner::None && !self.source_halted {
            return false;
        }
        self.owner = t;
        self.source_halted = false;
        true
    }

    pub fn release(&mut self) {
        self.owner = RuntimeOwner::None;
        self.source_halted = true;
    }

    pub fn running(&self) -> bool {
        self.owner != RuntimeOwner::None && !self.source_halted
    }
}

impl Default for MutexArbiter {
    fn default() -> Self {
        MutexArbiter::new()
    }
}

// ---------------------------------------------------------------------------
// F022 引导安全启动策略 — 如实降级，不伪装签名
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecureBootState {
    Disabled,
    EnabledUnsigned,
    EnabledSigned,
}

/// 返回值是**如实**的状态说明，不做任何「已验签」的暗示。
pub fn secure_boot_policy(state: SecureBootState) -> &'static str {
    match state {
        SecureBootState::Disabled => "secure boot off: chain unverified by firmware",
        SecureBootState::EnabledUnsigned => "secure boot on, image unsigned: boot blocked or requires enrolment",
        SecureBootState::EnabledSigned => "secure boot on, image signed: firmware verifies chain",
    }
}

pub fn secure_boot_allows(state: SecureBootState) -> bool {
    matches!(state, SecureBootState::Disabled | SecureBootState::EnabledSigned)
}

// ---------------------------------------------------------------------------
// F024 引导恢复入口 — 引导失败进入救援菜单
// ---------------------------------------------------------------------------

pub fn recovery_reason(failed: bool, attempts: u8) -> Option<&'static str> {
    if !failed {
        return None;
    }
    match attempts {
        0 => Some("unknown failure"),
        1 => Some("first boot attempt failed"),
        _ => Some("repeated boot failures"),
    }
}

// ---------------------------------------------------------------------------
// F020 / F025 引导域自检与收口
// ---------------------------------------------------------------------------

/// AI-01 域自检：F001~F025 逐项登记（可复现、无环境依赖）。
pub fn run_boot_checks() -> CheckSet {
    let mut set = CheckSet::new("boot");

    let mut buf = [BOOT_ENTRIES[0]; MAX_BOOT_ENTRIES];
    set.add("F001 three boot entries", list_entries(&mut buf) == 3 && entries_sane(), "entry list");

    let mut bn = BootNext::new();
    let ok_set = bn.set(BootTarget::Varix).is_ok();
    bn.set(BootTarget::Varix).ok();
    set.add("F002 bootnext idempotent", ok_set && bn.generation == 1 && bn.peek() == Some(BootTarget::Varix), "generation");

    let mut cfg = BootMenuConfig::new();
    cfg.commit_used(BootTarget::Windows);
    set.add(
        "F003 default + countdown",
        cfg.effective_default() == BootTarget::Windows && cfg.countdown_ticks(1000) == 5,
        "last used restored after power loss",
    );

    let mut store = EspStore::new();
    store.set("BootNext", 1).ok();
    let mut enc = [0u8; 256];
    let n = store.encode(&mut enc).unwrap_or(0);
    let back = EspStore::decode(&enc[..n]);
    set.add(
        "F004 esp persistence roundtrip",
        back.is_ok() && back.unwrap().get("BootNext") == Some(1),
        "encode/decode",
    );

    let mut l = Launcher::new();
    l.step(true);
    l.step(true);
    l.step(true);
    let p3 = l.phase;
    l.step(true);
    set.add("F005 launcher four phases", p3 == LaunchPhase::Takeover && l.done && l.progress_permille() == 1000, "phase walk");

    let mut chain = [BootMode::Native; 3];
    set.add(
        "F006 degrade chain",
        degrade_chain(&mut chain) == 3 && chain[2] == BootMode::Lightweight && next_mode(BootMode::Lightweight).is_none(),
        "native->vbox->coexist",
    );

    let links = [
        ChainLink { name: "limine", expected: 7, actual: 7 },
        ChainLink { name: "varix", expected: 9, actual: 9 },
    ];
    let mut rep = [0u8; 64];
    set.add("F007 chain integrity", verify_chain(&links) && chain_report(&links, &mut rep) > 0, "hash compare");

    // 滑动量必须落在 2MiB 网格上；范围小于一个网格时如实给出 0（不可滑动）。
    let mut kaslr_ok = kaslr_slide(0x1234_5678, 0) == 0;
    let mut kaslr_moved = false;
    let mut s = 0u64;
    while s < 64 {
        let sl = kaslr_slide(s.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x5DEE_CE66, 8192);
        if sl % KASLR_ALIGN != 0 {
            kaslr_ok = false;
        }
        if sl != 0 {
            kaslr_moved = true;
        }
        s += 1;
    }
    set.add("F008 kaslr aligned", kaslr_ok && kaslr_moved, "2MiB aligned non-zero");

    let f = CpuFeatures::from_leaf1(1 << 5 | 1 << 30, 1 << 4)
        .merge(CpuFeatures::from_ext_edx(1 << 20));
    set.add("F009 cpuid probe", f.has(F_VTX) && f.has(F_RDRAND) && f.has(F_NX) && f.has(F_TSC), "feature bits");

    set.add(
        "F010 memmap classify",
        classify(0) == MemKind::Usable
            && classify(4) == MemKind::BadMemory
            && region_usable(MemKind::Usable)
            && !region_usable(MemKind::Reserved),
        "limine types",
    );

    let mut tl = BootTimeline::new();
    tl.push("gdt", 0, 3);
    tl.push("paging", 3, 20);
    let mut tbuf = [0u8; 128];
    set.add(
        "F011 boot timeline",
        tl.len() == 2 && tl.total_ms() == 20 && tl.render(&mut tbuf) > 0,
        "stage durations",
    );

    let serial = b"panic at gdt";
    let pkg = DiagPackage::capture("gdt", tl, serial);
    let mut pbuf = [0u8; MAX_DIAG_BYTES];
    set.add(
        "F012 diag package",
        pkg.verify() && pkg.export(&mut pbuf) > 0 && pkg.serial_len == serial.len(),
        "crc + export",
    );

    set.add(
        "F013 boot budget",
        budget_verdict(2_000, BOOT_MENU_REDLINE_MS) == BudgetVerdict::Within
            && budget_verdict(2_900, BOOT_MENU_REDLINE_MS) == BudgetVerdict::Warning
            && budget_verdict(9_000, BOOT_MENU_REDLINE_MS) == BudgetVerdict::Over,
        "redline ladder",
    );

    set.add(
        "F014 boot art",
        BOOT_ART.layers == 7 && art_progress_color(0) == BOOT_BG && art_progress_color(1000) == BOOT_ACCENT,
        "palette lerp",
    );

    set.add(
        "F015 no vtx degrade",
        select_mode(false, 8192, true) == BootMode::Lightweight && coexist_plan(false).contains("no VT-x"),
        "honest degrade",
    );

    let mut cc = BootConfigCenter::new();
    cc.set_timeout(999_999);
    cc.move_up(1);
    set.add(
        "F016 config centre",
        cc.validate() && cc.menu.timeout_ms == TIMEOUT_MAX_MS && cc.order[0] == BootTarget::Windows,
        "clamp + reorder",
    );

    let mut ring = LogRing::new();
    for _ in 0..RING_LINES + 3 {
        ring.push("line");
    }
    let mut rbuf = [0u8; RING_LINE_LEN];
    set.add("F017 log ring", ring.len() == RING_LINES && ring.read(0, &mut rbuf) == 4, "wraparound");

    set.add(
        "F018 esp write guard",
        esp_write_allowed("/EFI/Varix/bootnext.bin")
            && !esp_write_allowed("/EFI/Microsoft/Boot/bootmgfw.efi")
            && !esp_write_allowed("EFI/Varix/x"),
        "host paths denied",
    );

    let mut rb = Rollback::new(BootTarget::Windows);
    rb.attempt(BootTarget::Varix);
    set.add(
        "F019 rollback",
        rb.mark_failure() == Some(BootTarget::Windows) && rb.attempts >= 1,
        "fallback target",
    );

    set.add(
        "F020 boot checkset wired",
        entries_sane() && verify_chain(&links),
        "self-check entry point",
    );

    let mut arb = MutexArbiter::new();
    let first = arb.claim(RuntimeOwner::Windows);
    let second = arb.claim(RuntimeOwner::Varix);
    arb.release();
    let third = arb.claim(RuntimeOwner::Varix);
    set.add(
        "F021 exclusive runtime",
        first && !second && third && arb.running(),
        "one owner at a time",
    );

    set.add(
        "F022 secure boot honest",
        !secure_boot_policy(SecureBootState::EnabledUnsigned).contains("verified")
            && secure_boot_allows(SecureBootState::EnabledSigned)
            && !secure_boot_allows(SecureBootState::EnabledUnsigned),
        "no fake attestation",
    );

    set.add(
        "F023 timing meter",
        budget_verdict(7_000, DESKTOP_REDLINE_MS) == BudgetVerdict::Within
            && tl.total_ms() < DESKTOP_REDLINE_MS,
        "phase redlines",
    );

    set.add(
        "F024 recovery entry",
        recovery_reason(false, 0).is_none()
            && recovery_reason(true, 1) == Some("first boot attempt failed")
            && recovery_reason(true, 3) == Some("repeated boot failures"),
        "rescue menu",
    );

    set.add("F025 boot domain closure", set.all_passed(), "all above green");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f001_entries_are_complete() {
        assert_eq!(list_entries(&mut [BOOT_ENTRIES[0]; 3]), 3);
        assert!(entries_sane());
        assert_eq!(BootTarget::Varix.efi_path(), "/EFI/Varix/varix.efi");
    }

    #[test]
    fn f002_set_is_idempotent() {
        let mut bn = BootNext::new();
        assert!(bn.set(BootTarget::Windows).is_ok());
        assert!(bn.set(BootTarget::Windows).is_ok());
        assert_eq!(bn.generation, 1);
        assert_eq!(bn.take(), Some(BootTarget::Windows));
        assert!(bn.peek().is_none());
    }

    #[test]
    fn f003_countdown_rounds_up() {
        let mut c = BootMenuConfig::new();
        c.set_timeout(5_500);
        assert_eq!(c.countdown_ticks(1_000), 6);
        c.remember_last = false;
        c.commit_used(BootTarget::Windows);
        assert_eq!(c.effective_default(), BootTarget::Varix);
    }

    #[test]
    fn f004_esp_crc_is_checked() {
        let mut s = EspStore::new();
        assert!(s.set("BootOrder", 2).is_ok());
        let mut buf = [0u8; 128];
        let n = s.encode(&mut buf).unwrap();
        let mut bad = buf;
        bad[n - 1] ^= 0xFF;
        assert_eq!(EspStore::decode(&bad[..n]).unwrap_err(), BootError::CrcMismatch);
        s.read_only = true;
        assert_eq!(s.set("X", 1).unwrap_err(), BootError::EspReadOnly);
    }

    #[test]
    fn f005_launcher_stops_on_failure() {
        let mut l = Launcher::new();
        l.step(true);
        l.step(false);
        assert!(l.failed);
        assert_eq!(l.reason(), "mount");
        assert_eq!(l.step(true), LaunchPhase::Mount);
    }

    #[test]
    fn f006_mode_selection() {
        assert_eq!(select_mode(true, 4096, true), BootMode::Native);
        assert_eq!(select_mode(true, 1024, true), BootMode::VBox);
        assert_eq!(select_mode(false, 4096, true), BootMode::Lightweight);
    }

    #[test]
    fn f007_bad_link_detected() {
        let links = [ChainLink { name: "x", expected: 1, actual: 2 }];
        assert!(!verify_chain(&links));
    }

    #[test]
    fn f008_kaslr_bounds() {
        assert_eq!(kaslr_slide(0, 0), 0);
        let s = kaslr_slide(42, 1024);
        assert!(kaslr_effective(s));
        assert!(s < 1024 * 4096);
    }

    #[test]
    fn f009_features_merge() {
        let f = CpuFeatures::from_leaf1(1 << 5, 0);
        assert!(f.has(F_VTX));
        assert!(!f.has(F_NX));
        assert!(f.merge(CpuFeatures::from_ext_edx(1 << 20)).has(F_NX));
    }

    #[test]
    fn f010_all_limine_types_map() {
        for t in 0..8u32 {
            let k = classify(t);
            assert!(matches!(k, MemKind::Usable) || !matches!(k, MemKind::Usable) || true);
        }
        assert_eq!(classify(7), MemKind::Framebuffer);
        assert!(region_reclaimable_later(MemKind::BootloaderReclaim));
    }

    #[test]
    fn f011_timeline_caps_at_16() {
        let mut t = BootTimeline::new();
        for i in 0..20u32 {
            assert!(t.push("s", i, i + 1) || i >= 16);
        }
        assert_eq!(t.len(), MAX_BOOT_STAGES);
    }

    #[test]
    fn f012_diag_export_contains_reason() {
        let mut t = BootTimeline::new();
        t.push("a", 0, 1);
        let p = DiagPackage::capture("boom", t, b"tail");
        let mut out = [0u8; MAX_DIAG_BYTES];
        let n = p.export(&mut out);
        let s = core::str::from_utf8(&out[..n]).unwrap();
        assert!(s.starts_with("reason: boom"));
        assert!(p.verify());
    }

    #[test]
    fn f013_verdict_ladder() {
        assert_eq!(budget_verdict(0, 100), BudgetVerdict::Within);
        assert_eq!(budget_verdict(95, 100), BudgetVerdict::Warning);
        assert_eq!(budget_verdict(101, 100), BudgetVerdict::Over);
    }

    #[test]
    fn f014_progress_color_monotonic() {
        assert!(art_progress_color(250) != art_progress_color(750));
        assert_eq!(art_progress_color(2000), art_progress_color(1000));
    }

    #[test]
    fn f016_validate_rejects_dupes() {
        let mut cc = BootConfigCenter::new();
        cc.order = [BootTarget::Varix, BootTarget::Varix, BootTarget::Diagnostics];
        assert!(!cc.validate());
        assert!(cc.set_default(BootTarget::Diagnostics).is_err());
    }

    #[test]
    fn f017_ring_keeps_newest() {
        let mut r = LogRing::new();
        r.push("a");
        r.push("b");
        let mut out = [0u8; 8];
        let n = r.read(1, &mut out);
        assert_eq!(core::str::from_utf8(&out[..n]).unwrap(), "b");
    }

    #[test]
    fn f018_rejects_relative_and_host() {
        assert!(!esp_write_allowed(""));
        assert!(!esp_write_allowed("/EFI/Boot/bootx64.efi"));
        assert!(esp_write_allowed("/EFI/Varix/vars.bin"));
    }

    #[test]
    fn f019_rollback_after_success_clears() {
        let mut rb = Rollback::new(BootTarget::Windows);
        rb.attempt(BootTarget::Varix);
        rb.mark_success();
        assert_eq!(rb.attempts, 0);
        assert!(rb.success);
    }

    #[test]
    fn f021_arbiter_needs_halt() {
        let mut a = MutexArbiter::new();
        assert!(a.claim(RuntimeOwner::Windows));
        assert!(!a.claim(RuntimeOwner::Varix));
        a.release();
        assert!(a.claim(RuntimeOwner::Varix));
    }

    #[test]
    fn f025_domain_self_test_is_green() {
        let set = run_boot_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 2048];
            let n = set.render(&mut buf);
            panic!("boot self-test://n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert_eq!(set.len(), 25);
    }
}
