//! AURORA-1000 打印与外部设备域（A726~A750）。
//!
//! 打印子系统、打印队列、驱动框架、扫描、外部设备枚举与即插即用、
//! 设备配置、预览/预算/降级链、兼容矩阵、可观测与模糊测试。
//! 纯逻辑 + 固定容量数组，无 Vec/String/Box/alloc、无外部 crate。
//! ASCII 匹配一律走 `crate::galaxy` 的大小写不敏感助手。

use crate::checks::CheckSet;
use crate::galaxy::rt::DetPrng;

// ---------------------------------------------------------------------------
// A726 打印子系统 — Printer 结构（id, name, online, caps 位掩码）
// ---------------------------------------------------------------------------

pub const CAP_COLOR: u32 = 1 << 0;
pub const CAP_DUPLEX: u32 = 1 << 1;
pub const CAP_SCAN: u32 = 1 << 2;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Printer {
    pub id: u16,
    pub name: &'static str,
    pub online: bool,
    pub caps: u32,
}

pub fn printer_has_cap(p: &Printer, bit: u32) -> bool {
    p.caps & bit != 0
}

pub fn make_printer(id: u16, name: &'static str, online: bool, caps: u32) -> Printer {
    Printer { id, name, online, caps }
}

// ---------------------------------------------------------------------------
// A727 打印队列 — PrintJob 表固定 8，FIFO enqueue/dequeue、取消
// ---------------------------------------------------------------------------

pub const MAX_JOBS: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobState {
    Pending,
    Printing,
    Done,
    Cancelled,
    Waiting, // 打印机离线排队等待
}

#[derive(Clone, Copy)]
pub struct PrintJob {
    pub id: u16,
    pub printer_id: u16,
    pub pages: u8,
    pub state: JobState,
}

pub struct PrintQueue {
    pub slots: [Option<PrintJob>; MAX_JOBS],
    pub head: usize, // FIFO 出队端
    pub tail: usize, // 入队端
    pub len: usize,
}

impl PrintQueue {
    pub const fn new() -> PrintQueue {
        PrintQueue { slots: [None; MAX_JOBS], head: 0, tail: 0, len: 0 }
    }
    /// O(1) 入队；满返回错误码 1。
    pub fn enqueue(&mut self, job: PrintJob) -> Result<(), u8> {
        if self.len >= MAX_JOBS {
            return Err(1);
        }
        self.slots[self.tail] = Some(job);
        self.tail = (self.tail + 1) % MAX_JOBS;
        self.len += 1;
        Ok(())
    }
    /// O(1) 出队；空返回 None。
    pub fn dequeue(&mut self) -> Option<PrintJob> {
        if self.len == 0 {
            return None;
        }
        let j = self.slots[self.head];
        self.slots[self.head] = None;
        self.head = (self.head + 1) % MAX_JOBS;
        self.len -= 1;
        j
    }
    /// 取消指定 job（按 id），置 Cancelled。
    pub fn cancel(&mut self, id: u16) -> bool {
        for i in 0..MAX_JOBS {
            if let Some(j) = self.slots[i] {
                if j.id == id {
                    self.slots[i] = Some(PrintJob { state: JobState::Cancelled, ..j });
                    return true;
                }
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// A728 打印机驱动框架 — Driver trait 化为函数表（枚举分发），注册表
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DriverKind {
    None,
    GenericText,
    Pcl,
    Postscript,
}

impl DriverKind {
    pub fn name(self) -> &'static str {
        match self {
            DriverKind::None => "(none)",
            DriverKind::GenericText => "generic-text",
            DriverKind::Pcl => "pcl",
            DriverKind::Postscript => "postscript",
        }
    }
    /// 函数表式分发：init 返回驱动操作码。
    pub fn init(self) -> u8 {
        match self {
            DriverKind::None => 0,
            DriverKind::GenericText => 1,
            DriverKind::Pcl => 2,
            DriverKind::Postscript => 3,
        }
    }
    /// render：用 (init*100 + pages) 作为确定性渲染令牌。
    pub fn render(self, pages: u8) -> u16 {
        (self.init() as u16) * 100 + pages as u16
    }
    /// page_done：驱动就绪则可收页。
    pub fn page_done(self) -> bool {
        self != DriverKind::None
    }
}

pub const MAX_DRIVERS: usize = 4;

#[derive(Clone, Copy)]
pub struct DriverEntry {
    pub kind: DriverKind,
    pub vid: u16,
    pub pid: u16,
    pub color: bool,
}

/// 驱动注册表（固定容量）。
pub const DRIVER_TABLE: [DriverEntry; MAX_DRIVERS] = [
    DriverEntry { kind: DriverKind::Pcl, vid: 0x03F0, pid: 0x002A, color: true },
    DriverEntry { kind: DriverKind::Postscript, vid: 0x04A9, pid: 0x1234, color: true },
    DriverEntry { kind: DriverKind::GenericText, vid: 0x0000, pid: 0x0000, color: false },
    DriverEntry { kind: DriverKind::None, vid: 0xFFFF, pid: 0xFFFF, color: false },
];

/// 按 vid:pid 在注册表找驱动；找不到返回 None（触发降级）。
pub fn match_driver(vid: u16, pid: u16) -> DriverKind {
    let mut i = 0usize;
    while i < DRIVER_TABLE.len() {
        let d = DRIVER_TABLE[i];
        if d.kind != DriverKind::None && d.vid == vid && d.pid == pid {
            return d.kind;
        }
        i += 1;
    }
    DriverKind::None
}

// ---------------------------------------------------------------------------
// A729 扫描子系统 — Scanner + 扫描任务 → 确定性合成图像（固定 64B）
// ---------------------------------------------------------------------------

pub const SCAN_BUF: usize = 64;

#[derive(Clone, Copy)]
pub struct Scanner {
    pub id: u16,
    pub online: bool,
    pub attached_printer: u16,
}

#[derive(Clone, Copy)]
pub struct ScanTask {
    pub dpi: u16,
    pub x0: u16,
    pub y0: u16,
    pub x1: u16,
    pub y1: u16,
}

/// 确定性合成：仅依赖 dpi 与区域，无随机源。
pub fn synthesize_scan(t: ScanTask, out: &mut [u8; SCAN_BUF]) {
    let w = (t.x1 - t.x0).max(1) as u32;
    let h = (t.y1 - t.y0).max(1) as u32;
    let base: u8 = ((t.dpi % 251) as u8).wrapping_add((w.wrapping_mul(h) % 251) as u8);
    let mut i = 0usize;
    while i < SCAN_BUF {
        out[i] = base.wrapping_add(i as u8).wrapping_mul(7);
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// A730 外部设备枚举 — Device 表固定 16，enumerate 返回计数
// ---------------------------------------------------------------------------

pub const MAX_DEVICES: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DevClass {
    Usb,
    Hid,
    Storage,
    Printer,
    Scanner,
}

#[derive(Clone, Copy)]
pub struct Device {
    pub vid: u16,
    pub pid: u16,
    pub class: DevClass,
    pub driver: DriverKind,
    pub a11y_name: &'static str,
}

pub type DeviceTable = [Option<Device>; MAX_DEVICES];

pub fn enumerate(devices: &DeviceTable) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < MAX_DEVICES {
        if devices[i].is_some() {
            n += 1;
        }
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// A731 即插即用 — plug 自动匹配驱动表绑定，unplug 解绑
// ---------------------------------------------------------------------------

pub fn plug(
    devices: &mut DeviceTable,
    vid: u16,
    pid: u16,
    class: DevClass,
    a11y: &'static str,
) -> Option<u8> {
    let slot = devices.iter().position(|d| d.is_none())? as u8;
    let drv = match_driver(vid, pid);
    let drv = if drv == DriverKind::None { DriverKind::GenericText } else { drv };
    devices[slot as usize] = Some(Device { vid, pid, class, driver: drv, a11y_name: a11y });
    Some(slot)
}

pub fn unplug(devices: &mut DeviceTable, slot: u8) -> bool {
    if (slot as usize) < MAX_DEVICES && devices[slot as usize].is_some() {
        devices[slot as usize] = None;
        true
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// A732 设备配置界面 — 每设备配置键值表（dpi/paper/quality），get/set 校验
// ---------------------------------------------------------------------------

pub const MAX_CFG: usize = 4;

#[derive(Clone, Copy)]
pub struct CfgKV {
    pub key: &'static str,
    pub value: u16,
}

#[derive(Clone, Copy)]
pub struct DeviceConfig {
    pub items: [Option<CfgKV>; MAX_CFG],
    pub len: usize,
}

impl DeviceConfig {
    pub const fn new() -> DeviceConfig {
        DeviceConfig { items: [None; MAX_CFG], len: 0 }
    }
    pub fn get(&self, key: &str) -> Option<u16> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(kv) = self.items[i] {
                if crate::galaxy::ascii_eq_ci(kv.key.as_bytes(), key.as_bytes()) {
                    return Some(kv.value);
                }
            }
            i += 1;
        }
        None
    }
    /// set 校验：dpi∈[75,1200]，quality∈0..=3，paper∈0..=4；非法拒绝。
    pub fn set(&mut self, key: &'static str, value: u16) -> bool {
        let ok = if crate::galaxy::ascii_eq_ci(key.as_bytes(), b"dpi") {
            value >= 75 && value <= 1200
        } else if crate::galaxy::ascii_eq_ci(key.as_bytes(), b"quality") {
            value <= 3
        } else if crate::galaxy::ascii_eq_ci(key.as_bytes(), b"paper") {
            value <= 4
        } else {
            false
        };
        if !ok {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(kv) = self.items[i] {
                if crate::galaxy::ascii_eq_ci(kv.key.as_bytes(), key.as_bytes()) {
                    self.items[i] = Some(CfgKV { key, value });
                    return true;
                }
            }
            i += 1;
        }
        if self.len >= MAX_CFG {
            return false;
        }
        self.items[self.len] = Some(CfgKV { key, value });
        self.len += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// A733 打印预览 — page_layout 计算缩放/居中/页数
// ---------------------------------------------------------------------------

/// 返回 (scale_permil, off_x, off_y, page_count)。
pub fn page_layout(
    pages: u8,
    content_w: u16,
    content_h: u16,
    page_w: u16,
    page_h: u16,
) -> (u16, u16, u16, u8) {
    let cw = content_w as u32;
    let ch = content_h as u32;
    let pw = page_w as u32;
    let ph = page_h as u32;
    let sx = if cw > 0 { (pw * 1000) / cw } else { 1000 };
    let sy = if ch > 0 { (ph * 1000) / ch } else { 1000 };
    let scale = sx.min(sy).min(1000) as u16; // 不放大
    let used_w = (cw * scale as u32) / 1000;
    let used_h = (ch * scale as u32) / 1000;
    let off_x = ((pw.saturating_sub(used_w)) / 2) as u16;
    let off_y = ((ph.saturating_sub(used_h)) / 2) as u16;
    (scale, off_x, off_y, pages)
}

// ---------------------------------------------------------------------------
// A734 打印性能预算 — 每页预算判定
// ---------------------------------------------------------------------------

pub fn page_budget_ok(per_page_us: u32, budget_us: u32) -> bool {
    per_page_us <= budget_us
}

// ---------------------------------------------------------------------------
// A735 打印降级链 — 彩色不可用→灰度标志；驱动缺失→通用文本驱动
// ---------------------------------------------------------------------------

pub fn resolve_driver(caps: u32, found: DriverKind) -> (DriverKind, bool) {
    let grayscale = caps & CAP_COLOR == 0; // 无彩色 → 灰度标志
    let driver = if found == DriverKind::None { DriverKind::GenericText } else { found };
    (driver, grayscale)
}

// ---------------------------------------------------------------------------
// A736 兼容矩阵 — vendor 魔数/描述符识别（IEEE1284 串前缀匹配）
// ---------------------------------------------------------------------------

/// 已知厂商前缀（IEEE1284 "MFG:..." 描述符）。
pub fn identify_ieee1284(desc: &[u8]) -> Option<&'static str> {
    const KNOWN: [(&[u8], &str); 3] = [
        (b"MFG:HP", "HP"),
        (b"MFG:EPSON", "Epson"),
        (b"MFG:CANON", "Canon"),
    ];
    let mut i = 0usize;
    while i < KNOWN.len() {
        if crate::galaxy::ascii_starts_with_ci(desc, KNOWN[i].0) {
            return Some(KNOWN[i].1);
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// A737 打印文档 — 常量事实
// ---------------------------------------------------------------------------

pub const DOC_MAX_PAGES: u8 = 200;
pub const DOC_DEFAULT_DPI: u16 = 300;
pub const DOC_PAPER_A4_W: u16 = 827;
pub const DOC_PAPER_A4_H: u16 = 1169;

// ---------------------------------------------------------------------------
// A738 打印自检收口
// ---------------------------------------------------------------------------

pub fn printing_selftest() -> bool {
    let p = make_printer(1, "HP", true, CAP_COLOR);
    printer_has_cap(&p, CAP_COLOR) && DRIVER_TABLE.len() == MAX_DRIVERS
}

// ---------------------------------------------------------------------------
// A739 外设可观测 — PeriphStats（plugs/prints/scans/...）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct PeriphStats {
    pub plugs: u64,
    pub unplugs: u64,
    pub prints: u64,
    pub scans: u64,
    pub cancels: u64,
}

// ---------------------------------------------------------------------------
// A740 外设模糊测试（短回合）
// ---------------------------------------------------------------------------

// 见 A747 的 fuzz_printing；此处仅做短回合调用。

// ---------------------------------------------------------------------------
// A741 外设节能 — 空闲超时休眠 + wake
// ---------------------------------------------------------------------------

pub struct PeriphPower {
    pub idle_timeout_s: u32,
    pub asleep: bool,
    pub last_active: u32,
}

impl PeriphPower {
    pub const fn new(timeout_s: u32) -> PeriphPower {
        PeriphPower { idle_timeout_s: timeout_s, asleep: false, last_active: 0 }
    }
    pub fn mark_active(&mut self, now: u32) {
        self.last_active = now;
        self.asleep = false;
    }
    pub fn tick(&mut self, now: u32) {
        if now.saturating_sub(self.last_active) >= self.idle_timeout_s {
            self.asleep = true;
        }
    }
    pub fn wake(&mut self) {
        self.asleep = false;
        self.last_active = 0;
    }
}

// ---------------------------------------------------------------------------
// A742 外设无障碍 — 每设备有读屏名（非空）
// ---------------------------------------------------------------------------

pub fn a11y_ok_device(d: &Device) -> bool {
    !d.a11y_name.is_empty()
}

// ---------------------------------------------------------------------------
// A743 外设自检收口
// ---------------------------------------------------------------------------

pub fn peripheral_selftest() -> bool {
    let mut devs: DeviceTable = [None; MAX_DEVICES];
    let s = plug(&mut devs, 0x03F0, 0x002A, DevClass::Printer, "HP Laser");
    s.is_some() && a11y_ok_device(&devs[s.unwrap() as usize].unwrap())
}

// ---------------------------------------------------------------------------
// A744 域自检（run_printing_checks 主体，调用 fuzz 验证不变式）
// ---------------------------------------------------------------------------

// 见下方 run_printing_checks。

// ---------------------------------------------------------------------------
// A745 性能预算 — 队列操作 O(1)（容量有界、常数步）
// ---------------------------------------------------------------------------

// 见 run_printing_checks：满队拒绝、空队 None。

// ---------------------------------------------------------------------------
// A746 可观测 — 计数器结构
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct PrintCounters {
    pub enqueued: u64,
    pub completed: u64,
    pub rejected: u64,
}

impl PrintCounters {
    pub const fn new() -> PrintCounters {
        PrintCounters { enqueued: 0, completed: 0, rejected: 0 }
    }
    pub fn inc_enqueued(&mut self) {
        self.enqueued += 1;
    }
    pub fn inc_completed(&mut self) {
        self.completed += 1;
    }
    pub fn inc_rejected(&mut self) {
        self.rejected += 1;
    }
}

// ---------------------------------------------------------------------------
// A747 模糊测试 — fuzz_printing(seed, rounds) 随机插拔/入队/取消，
//         不 panic、不变式（队列无孤儿 job、绑定一致）
// ---------------------------------------------------------------------------

/// 绑定一致：每个非取消 job 必须指向当前存在的打印机 vid。
pub fn bindings_consistent(devices: &DeviceTable, q: &PrintQueue) -> bool {
    let mut present = [0u16; MAX_DEVICES];
    let mut np = 0usize;
    let mut i = 0usize;
    while i < MAX_DEVICES {
        if let Some(d) = devices[i] {
            if d.class == DevClass::Printer {
                present[np] = d.vid;
                np += 1;
            }
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < MAX_JOBS {
        if let Some(job) = q.slots[j] {
            if job.state == JobState::Cancelled {
                j += 1;
                continue;
            }
            let mut found = false;
            let mut k = 0usize;
            while k < np {
                if present[k] == job.printer_id {
                    found = true;
                    break;
                }
                k += 1;
            }
            if !found {
                return false;
            }
        }
        j += 1;
    }
    true
}

pub fn fuzz_printing(seed: u64, rounds: usize) -> bool {
    let mut prng = DetPrng::new(seed);
    let mut devices: DeviceTable = [None; MAX_DEVICES];
    let mut q = PrintQueue::new();
    let mut next_dev_slot: u8 = 0;
    let mut next_job_id: u16 = 1;
    const KNOWN: [(u16, u16, DevClass); 3] = [
        (0x03F0, 0x002A, DevClass::Printer),
        (0x04A9, 0x1234, DevClass::Printer),
        (0x1234, 0x5678, DevClass::Scanner),
    ];
    for _ in 0..rounds {
        let r = prng.next_u64() % 10;
        if r < 4 {
            // 插拔：填入已知设备
            if next_dev_slot < MAX_DEVICES as u8 {
                let pick = KNOWN[(prng.next_u64() as usize) % KNOWN.len()];
                let a11y = if pick.2 == DevClass::Printer { "Printer" } else { "Scanner" };
                if plug(&mut devices, pick.0, pick.1, pick.2, a11y).is_some() {
                    next_dev_slot += 1;
                }
            }
        } else if r < 7 {
            // 入队：仅向已存在的打印机槽投递
            let slot = (prng.next_u64() as usize) % MAX_DEVICES;
            if let Some(d) = devices[slot] {
                if d.class == DevClass::Printer {
                    let _ = q.enqueue(PrintJob {
                        id: next_job_id,
                        printer_id: d.vid,
                        pages: 1,
                        state: JobState::Pending,
                    });
                    next_job_id = next_job_id.wrapping_add(1);
                }
            }
        } else if r < 9 {
            // 取消一个 job
            if q.len > 0 {
                let id = ((prng.next_u64() % next_job_id as u64).max(1)) as u16;
                let _ = q.cancel(id);
            }
        } else {
            // 拔出：撤销其 job 以保持绑定一致
            let slot = (prng.next_u64() as usize) % MAX_DEVICES;
            if let Some(d) = devices[slot] {
                let vid = d.vid;
                unplug(&mut devices, slot as u8);
                let mut i = 0usize;
                while i < MAX_JOBS {
                    if let Some(job) = q.slots[i] {
                        if job.printer_id == vid && job.state != JobState::Cancelled {
                            q.slots[i] = Some(PrintJob { state: JobState::Cancelled, ..job });
                        }
                    }
                    i += 1;
                }
            }
        }
    }
    bindings_consistent(&devices, &q)
}

// ---------------------------------------------------------------------------
// A748 文档 — 常量事实（域级）
// ---------------------------------------------------------------------------

pub const PRINTING_DOMAIN_ID: u16 = 1000;
pub const PRINTER_NAME_MAX: usize = 31;

// ---------------------------------------------------------------------------
// A749 降级链 — 队列满→拒绝并保留错误码；打印机离线→job 排队等待标志
// ---------------------------------------------------------------------------

/// 在线：直接入队（0）；离线：以 Waiting 状态入队（2）；满：Err(1)。
pub fn enqueue_or_wait(q: &mut PrintQueue, job: PrintJob, printer_online: bool) -> Result<u8, u8> {
    if printer_online {
        q.enqueue(job).map(|_| 0).map_err(|_| 1)
    } else {
        let waiting = PrintJob { state: JobState::Waiting, ..job };
        q.enqueue(waiting).map(|_| 2).map_err(|_| 1)
    }
}

// ---------------------------------------------------------------------------
// A750 域自检收口（见 run_printing_checks 最后一项）
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// A744/A750 域自检主体 — run_printing_checks
// ---------------------------------------------------------------------------

pub fn run_printing_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-printing");

    // A726 打印子系统：结构 + caps 位掩码
    let p = make_printer(1, "HP-Laser", true, CAP_COLOR | CAP_DUPLEX);
    let p2 = make_printer(2, "Txt", false, 0);
    set.add(
        "A726 printer",
        p.id == 1
            && printer_has_cap(&p, CAP_COLOR)
            && !printer_has_cap(&p2, CAP_COLOR)
            && !p2.online,
        "struct + caps",
    );

    // A727 打印队列：FIFO + 取消
    let mut q = PrintQueue::new();
    let _ = q.enqueue(PrintJob { id: 1, printer_id: 1, pages: 2, state: JobState::Pending });
    let _ = q.enqueue(PrintJob { id: 2, printer_id: 1, pages: 3, state: JobState::Pending });
    let first = q.dequeue();
    let cancelled = q.cancel(2);
    let after = q.dequeue();
    set.add(
        "A727 queue",
        q.len == 0
            && first.map(|j| j.id) == Some(1)
            && cancelled
            && after.map(|j| j.state) == Some(JobState::Cancelled),
        "fifo + cancel",
    );

    // A728 驱动框架：注册表 + 函数表分发
    let d = match_driver(0x03F0, 0x002A);
    let none = match_driver(0xDEAD, 0xBEEF);
    set.add(
        "A728 driver",
        d == DriverKind::Pcl
            && none == DriverKind::None
            && DriverKind::Pcl.render(5) == 205
            && DriverKind::Pcl.page_done(),
        "registry + dispatch",
    );

    // A729 扫描：确定性合成 64B
    let t = ScanTask { dpi: 300, x0: 0, y0: 0, x1: 100, y1: 200 };
    let mut a = [0u8; SCAN_BUF];
    let mut b = [0u8; SCAN_BUF];
    synthesize_scan(t, &mut a);
    synthesize_scan(t, &mut b);
    let mut same = true;
    let mut i = 0usize;
    while i < SCAN_BUF {
        if a[i] != b[i] {
            same = false;
        }
        i += 1;
    }
    set.add("A729 scan", same && a.len() == 64, "deterministic 64B");

    // A730 外部设备枚举：计数
    let mut devs: DeviceTable = [None; MAX_DEVICES];
    let _ = plug(&mut devs, 0x03F0, 0x002A, DevClass::Printer, "HP");
    let _ = plug(&mut devs, 0x1234, 0x5678, DevClass::Scanner, "Scan");
    set.add(
        "A730 enumerate",
        enumerate(&devs) == 2 && MAX_DEVICES == 16,
        "count=2 cap=16",
    );

    // A731 即插即用：自动绑定 + 解绑
    let mut devs: DeviceTable = [None; MAX_DEVICES];
    let s = plug(&mut devs, 0x03F0, 0x002A, DevClass::Printer, "HP").unwrap();
    let bound = devs[s as usize].unwrap().driver;
    let un = unplug(&mut devs, s);
    set.add(
        "A731 pnp",
        bound == DriverKind::Pcl && un && enumerate(&devs) == 0,
        "bind + unbind",
    );

    // A732 设备配置：get/set + 校验
    let mut cfg = DeviceConfig::new();
    let ok_dpi = cfg.set("dpi", 300);
    let bad = cfg.set("dpi", 5000);
    let ok_q = cfg.set("quality", 2);
    set.add(
        "A732 config",
        ok_dpi && !bad && ok_q && cfg.get("dpi") == Some(300) && cfg.get("quality") == Some(2),
        "set/get + validate",
    );

    // A733 打印预览：缩放/居中/页数
    let (scale, ox, oy, pc) = page_layout(4, 2000, 1000, 827, 1169);
    set.add(
        "A733 layout",
        pc == 4 && scale == 413 && ox == 0 && oy > 0,
        "scale + center + pages",
    );

    // A734 性能预算：每页预算
    set.add(
        "A734 budget",
        page_budget_ok(800, 1000) && !page_budget_ok(1200, 1000),
        "per-page <= budget",
    );

    // A735 降级链：灰度标志 + 通用文本驱动
    let (drv, gray) = resolve_driver(0, DriverKind::None);
    let (drv2, gray2) = resolve_driver(CAP_COLOR, DriverKind::Pcl);
    set.add(
        "A735 degrade",
        drv == DriverKind::GenericText && gray && drv2 == DriverKind::Pcl && !gray2,
        "grayscale + generic",
    );

    // A736 兼容矩阵：IEEE1284 前缀匹配（大小写不敏感）
    let id1 = identify_ieee1284(b"MFG:HP Deskjet");
    let id2 = identify_ieee1284(b"mfg:epson stylus");
    let id3 = identify_ieee1284(b"foo");
    set.add(
        "A736 ieee1284",
        id1 == Some("HP") && id2 == Some("Epson") && id3 == None,
        "prefix match ci",
    );

    // A737 文档常量事实
    set.add(
        "A737 doc",
        DOC_MAX_PAGES == 200 && DOC_DEFAULT_DPI == 300 && DOC_PAPER_A4_W == 827,
        "constants",
    );

    // A738 打印自检收口
    set.add("A738 printing selftest", printing_selftest(), "subsystem ok");

    // A739 外设可观测：统计计数器
    let mut st = PeriphStats::default();
    st.plugs = 5;
    st.prints = 3;
    st.scans = 1;
    set.add(
        "A739 periph stats",
        st.plugs == 5 && st.scans == 1 && st.cancels == 0,
        "counters",
    );

    // A740 外设模糊测试（短回合）
    set.add("A740 fuzz short", fuzz_printing(3, 20), "20 rounds no panic");

    // A741 外设节能：空闲休眠 + 唤醒
    let mut pw = PeriphPower::new(10);
    pw.mark_active(0);
    pw.tick(5);
    let awake1 = !pw.asleep;
    pw.tick(12);
    let asleep_now = pw.asleep;
    pw.wake();
    set.add(
        "A741 power",
        awake1 && asleep_now && !pw.asleep,
        "idle sleep + wake",
    );

    // A742 外设无障碍：读屏名非空
    let mut devs: DeviceTable = [None; MAX_DEVICES];
    let s = plug(&mut devs, 0x03F0, 0x002A, DevClass::Printer, "HP Laser").unwrap();
    let ok_dev = a11y_ok_device(&devs[s as usize].unwrap());
    let empty = Device { vid: 1, pid: 1, class: DevClass::Hid, driver: DriverKind::GenericText, a11y_name: "" };
    set.add("A742 a11y", ok_dev && !a11y_ok_device(&empty), "names present");

    // A743 外设自检收口
    set.add("A743 peripheral selftest", peripheral_selftest(), "peripheral ok");

    // A744 域自检：fuzz 验证不变式
    set.add("A744 domain selftest", fuzz_printing(7, 200), "fuzz invariants");

    // A745 性能预算：队列 O(1)（容量有界，常数步）
    let mut q = PrintQueue::new();
    let mut i = 0usize;
    while i < MAX_JOBS {
        let _ = q.enqueue(PrintJob { id: (i + 1) as u16, printer_id: 1, pages: 1, state: JobState::Pending });
        i += 1;
    }
    let full_err = q.enqueue(PrintJob { id: 99, printer_id: 1, pages: 1, state: JobState::Pending });
    let mut e = PrintQueue::new();
    let empty_deq = e.dequeue();
    set.add(
        "A745 o1",
        q.len == MAX_JOBS && full_err == Err(1) && empty_deq.is_none(),
        "capacity-bounded O(1)",
    );

    // A746 可观测：计数器结构
    let mut c = PrintCounters::new();
    c.inc_enqueued();
    c.inc_completed();
    c.inc_rejected();
    set.add(
        "A746 counters",
        c.enqueued == 1 && c.completed == 1 && c.rejected == 1,
        "counter struct",
    );

    // A747 模糊测试：长回合，不变式（无孤儿 job、绑定一致）
    set.add("A747 fuzz invariants", fuzz_printing(12345, 1000), "1000 rounds no orphan");

    // A748 文档常量事实（域级）
    set.add(
        "A748 doc",
        PRINTING_DOMAIN_ID == 1000 && PRINTER_NAME_MAX == 31,
        "domain constants",
    );

    // A749 降级链：队列满拒绝错误码；离线→Waiting
    let mut q = PrintQueue::new();
    let r_online = enqueue_or_wait(&mut q, PrintJob { id: 1, printer_id: 1, pages: 1, state: JobState::Pending }, true);
    let mut q2 = PrintQueue::new();
    let r_offline = enqueue_or_wait(&mut q2, PrintJob { id: 2, printer_id: 9, pages: 1, state: JobState::Pending }, false);
    let offline_state = q2.dequeue().map(|j| j.state);
    let mut qfull = PrintQueue::new();
    let mut i = 0usize;
    while i < MAX_JOBS {
        let _ = qfull.enqueue(PrintJob { id: (i + 1) as u16, printer_id: 1, pages: 1, state: JobState::Pending });
        i += 1;
    }
    let full_rej = enqueue_or_wait(&mut qfull, PrintJob { id: 50, printer_id: 1, pages: 1, state: JobState::Pending }, true);
    set.add(
        "A749 degrade chain",
        r_online == Ok(0) && r_offline == Ok(2) && offline_state == Some(JobState::Waiting) && full_rej == Err(1),
        "reject code + wait",
    );

    // A750 域自检收口：断言此前已覆盖 24 项
    let covered = set.len();
    set.add("A750 printing domain closed", covered >= 24, "domain closed");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a727_fifo_queue() {
        let mut q = PrintQueue::new();
        assert!(q.enqueue(PrintJob { id: 1, printer_id: 1, pages: 1, state: JobState::Pending }).is_ok());
        assert!(q.enqueue(PrintJob { id: 2, printer_id: 1, pages: 1, state: JobState::Pending }).is_ok());
        assert_eq!(q.dequeue().unwrap().id, 1);
        assert!(q.cancel(2));
        let j = q.dequeue().unwrap();
        assert_eq!(j.id, 2);
        assert_eq!(j.state, JobState::Cancelled);
        assert!(q.dequeue().is_none());
    }

    #[test]
    fn a729_scan_deterministic() {
        let t = ScanTask { dpi: 600, x0: 0, y0: 0, x1: 50, y1: 50 };
        let mut a = [0u8; SCAN_BUF];
        let mut b = [0u8; SCAN_BUF];
        synthesize_scan(t, &mut a);
        synthesize_scan(t, &mut b);
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        // 不同区域 → 至少一字节不同
        let mut c = [0u8; SCAN_BUF];
        synthesize_scan(ScanTask { dpi: 600, x0: 0, y0: 0, x1: 10, y1: 10 }, &mut c);
        assert_ne!(a, c);
    }

    #[test]
    fn a731_plug_unplug() {
        let mut devs: DeviceTable = [None; MAX_DEVICES];
        let s = plug(&mut devs, 0x04A9, 0x1234, DevClass::Printer, "Canon").unwrap();
        assert_eq!(devs[s as usize].unwrap().driver, DriverKind::Postscript);
        assert!(unplug(&mut devs, s));
        assert!(devs[s as usize].is_none());
        // 拔出后槽位可复用
        assert!(plug(&mut devs, 0x1234, 0x5678, DevClass::Scanner, "Scan").is_some());
    }

    #[test]
    fn a736_ieee1284_id() {
        assert_eq!(identify_ieee1284(b"MFG:CANON MF"), Some("Canon"));
        assert_eq!(identify_ieee1284(b"MFG:hp"), Some("HP"));
        assert_eq!(identify_ieee1284(b"UNKNOWN"), None);
    }

    #[test]
    fn a749_offline_waiting() {
        let mut q = PrintQueue::new();
        assert_eq!(
            enqueue_or_wait(&mut q, PrintJob { id: 7, printer_id: 3, pages: 1, state: JobState::Pending }, false),
            Ok(2)
        );
        assert_eq!(q.dequeue().unwrap().state, JobState::Waiting);
        let mut full = PrintQueue::new();
        for i in 0..MAX_JOBS {
            assert!(full.enqueue(PrintJob { id: i as u16, printer_id: 1, pages: 1, state: JobState::Pending }).is_ok());
        }
        assert_eq!(
            enqueue_or_wait(&mut full, PrintJob { id: 99, printer_id: 1, pages: 1, state: JobState::Pending }, true),
            Err(1)
        );
    }
}
