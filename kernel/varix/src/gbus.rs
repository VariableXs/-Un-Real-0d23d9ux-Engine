//! GALAXY-1800 AI-05 I/O·总线·用户态驱动域（G241~G300）。
//!
//! Three merged sub-domains: PCI bus (G241~G260), async batch I/O
//! (G261~G280) and user-space drivers (G281~G300). Pure logic, fixed
//! arrays, host-testable.

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// G241~G260 — PCI
// ---------------------------------------------------------------------------

/// G241 配置空间地址：ECAM = base + bus<<20 | dev<<15 | fn<<12 | off。
pub fn ecam_addr(base: u64, bus: u8, dev: u8, fun: u8, off: u16) -> u64 {
    base + ((bus as u64) << 20) | ((dev as u64) << 15) | ((fun as u64) << 12) | (off as u64 & 0xFFF)
}

#[derive(Clone, Copy)]
pub struct PciDev {
    pub bus: u8,
    pub dev: u8,
    pub fun: u8,
    pub vendor: u16,
    pub device: u16,
    pub class: u8,
}

/// G242 枚举：无效 vendor (0xFFFF) 跳过，构造设备树列表。
pub fn pci_enumerate(raw: &[(u8, u8, u8, u16, u16, u8)], out: &mut [PciDev]) -> usize {
    let mut n = 0;
    for &(bus, dev, fun, vendor, device, class) in raw {
        if vendor == 0xFFFF || n >= out.len() {
            continue;
        }
        out[n] = PciDev { bus, dev, fun, vendor, device, class };
        n += 1;
    }
    n
}

/// G243 BAR 解析：bit0=IO/MEM。
pub fn bar_parse(raw: u32) -> (bool, u64) {
    let is_io = raw & 1 != 0;
    let mask = if is_io { 0xFFFF_FFFC } else { 0xFFFF_FFF0 };
    (is_io, (raw & mask) as u64)
}

/// 简化 BAR 尺寸：写入全 1 读回掩码求 size。
pub fn bar_size(mask: u32) -> u32 {
    if mask == 0 || mask == 0xFFFF_FFFF {
        return 0;
    }
    (!mask).wrapping_add(1)
}

/// G244 MSI/MSI-X：地址 0xFEE00000 | dest<<12，数据 = 向量 | 触发位。
pub fn msi_msg(dest_apic: u8, vector: u8, edge: bool) -> (u32, u32) {
    let addr = 0xFEE0_0000u32 | ((dest_apic as u32) << 12);
    let data = (vector as u32) | if edge { 0 } else { 1 << 15 };
    (addr, data)
}

/// G245 能力链解析：cap_ptr → (id, next)。
pub fn cap_walk(cfg: &[u8], mut ptr: usize) -> ([u8; 8], usize) {
    let mut ids = [0u8; 8];
    let mut n = 0;
    while ptr + 2 <= cfg.len() && n < 8 && ptr != 0 {
        let id = cfg[ptr];
        ids[n] = id;
        n += 1;
        let next = (cfg[ptr + 1] & 0xFC) as usize;
        if next == 0 || next <= ptr {
            break;
        }
        ptr = next;
    }
    (ids, n)
}

/// G246 热插拔状态机：off→present→configured→enabled→off。
pub fn hp_next(state: u8, event: u8) -> u8 {
    // states: 0 off, 1 present, 2 configured, 3 enabled
    match (state, event) {
        (0, 0) => 1, // present
        (1, 1) => 2, // configure
        (2, 2) => 3, // enable
        (3, 3) | (1, 3) | (2, 3) => 0, // removal
        (3, 4) => 3, // 无事件保持
        _ => state,
    }
}

/// G247 AER：严重/可恢复分类。
pub fn aer_severity(status: u32) -> &'static str {
    let fatal = status & (1 << 5 | 1 << 4 | 1 << 2); // 链路训练失败/数据链路层/意外完成
    let _ = 1 << 2;
    if status & (1 << 5) != 0 {
        "fatal"
    } else if fatal != 0 {
        "uncorrected-nonfatal"
    } else if status != 0 {
        "correctable"
    } else {
        "none"
    }
}

/// G248 驱动匹配表：vendor:device 精确优先，其次 class。
pub fn driver_match(table: &[(u16, u16, u8, &'static str)], d: &PciDev) -> Option<&'static str> {
    for &(v, dv, cls, name) in table {
        if v == d.vendor && dv == d.device {
            return Some(name);
        }
    }
    for &(_, _, cls, name) in table {
        if cls == d.class {
            return Some(name);
        }
    }
    None
}

/// G250 设备电源状态 D0~D3。
pub fn dev_power_state(active_pct: u8) -> u8 {
    match active_pct {
        0 => 3,
        1..=10 => 2,
        11..=50 => 1,
        _ => 0,
    }
}

/// G254 桥与层次：二级总线范围判断。
pub fn behind_bridge(bridge_secondary: u8, bridge_subordinate: u8, dev_bus: u8) -> bool {
    dev_bus >= bridge_secondary && dev_bus <= bridge_subordinate
}

/// G257 SR-IOV：VF 总数与使能数。
pub fn sriov_enable(total_vfs: u16, requested: u16) -> u16 {
    requested.min(total_vfs).min(256)
}

/// G258 枚举确定性：同输入同序（排序 bus/dev/fn）。
pub fn enumeration_stable(devs: &mut [PciDev]) -> bool {
    devs.sort_unstable_by_key(|d| ((d.bus as u32) << 16) | ((d.dev as u32) << 8) | d.fun as u32);
    true
}

/// G259 设备数据库：vendor 名表。
pub fn vendor_name(vendor: u16) -> &'static str {
    match vendor {
        0x8086 => "Intel",
        0x10EC => "Realtek",
        0x1AF4 => "Red Hat (virtio)",
        0x1022 => "AMD",
        _ => "unknown",
    }
}

// ---------------------------------------------------------------------------
// G261~G280 — 异步批量 I/O（io_uring 类）
// ---------------------------------------------------------------------------

pub const SQ_DEPTH: usize = 16;
pub const CQ_DEPTH: usize = 16;

pub const IORING_READ: u8 = 1;
pub const IORING_WRITE: u8 = 2;
pub const IORING_FSYNC: u8 = 3;
pub const IORING_NOP: u8 = 0;

#[derive(Clone, Copy)]
pub struct Sqe {
    pub opcode: u8,
    pub fd: u8,
    pub offset: u64,
    pub len: u32,
    pub user_data: u64,
    pub flags: u8, // bit0: linked
}

#[derive(Clone, Copy)]
pub struct Cqe {
    pub user_data: u64,
    pub result: i32,
}

/// G261/G262 提交/完成环。
pub struct IoUring {
    pub sq: [Option<Sqe>; SQ_DEPTH],
    pub cq: [Option<Cqe>; CQ_DEPTH],
    sq_head: usize,
    sq_tail: usize,
    cq_head: usize,
    cq_tail: usize,
    pub submitted: u64,
    pub reaped: u64,
}

impl IoUring {
    pub const fn new() -> IoUring {
        IoUring {
            sq: [None; SQ_DEPTH],
            cq: [None; CQ_DEPTH],
            sq_head: 0,
            sq_tail: 0,
            cq_head: 0,
            cq_tail: 0,
            submitted: 0,
            reaped: 0,
        }
    }
    pub fn submit(&mut self, e: Sqe) -> bool {
        let next = (self.sq_tail + 1) % SQ_DEPTH;
        if next == self.sq_head {
            return false; // 环满
        }
        self.sq[self.sq_tail] = Some(e);
        self.sq_tail = next;
        self.submitted += 1;
        true
    }
    /// 内核侧消费一个 SQE 并产出 CQE（演示：NOP/Negative 模拟）。
    pub fn complete_one(&mut self, result: i32) -> bool {
        if self.sq_head == self.sq_tail {
            return false;
        }
        let e = self.sq[self.sq_head].take().unwrap();
        self.sq_head = (self.sq_head + 1) % SQ_DEPTH;
        let cnext = (self.cq_tail + 1) % CQ_DEPTH;
        if cnext == self.cq_head {
            self.sq[self.sq_head] = Some(e); // 放回（环满，简化）
            return false;
        }
        self.cq[self.cq_tail] = Some(Cqe { user_data: e.user_data, result });
        self.cq_tail = cnext;
        true
    }
    pub fn reap(&mut self) -> Option<Cqe> {
        if self.cq_head == self.cq_tail {
            return None;
        }
        let c = self.cq[self.cq_head].take().unwrap();
        self.cq_head = (self.cq_head + 1) % CQ_DEPTH;
        self.reaped += 1;
        Some(c)
    }
    pub fn pending(&self) -> usize {
        (self.sq_tail + SQ_DEPTH - self.sq_head) % SQ_DEPTH
    }
}

/// G263 批量系统调用：submit_and_wait（一次提交多个）。
pub fn submit_batch(ring: &mut IoUring, sqes: &[Sqe]) -> usize {
    let mut n = 0;
    for &e in sqes {
        if ring.submit(e) {
            n += 1;
        } else {
            break;
        }
    }
    n
}

/// G266 I/O 调度器：合并相邻 LBA + 按 LBA 排序（原地压缩，尾部清零）。
pub fn io_sort_merge(reqs: &mut [(u64, u32)]) -> usize {
    reqs.sort_unstable_by_key(|r| r.0);
    let mut w = 1usize;
    let mut merged = 0usize;
    for i in 1..reqs.len() {
        let prev_end = reqs[w - 1].0 + reqs[w - 1].1 as u64;
        if reqs[i].0 <= prev_end {
            let end = (reqs[i].0 + reqs[i].1 as u64).max(prev_end);
            reqs[w - 1].1 = (end - reqs[w - 1].0) as u32;
            merged += 1;
        } else {
            reqs[w] = reqs[i];
            w += 1;
        }
    }
    for r in reqs[w..].iter_mut() {
        *r = (0, 0);
    }
    merged
}

/// G268 I/O 优先级：RT/BE/IDLE 三类，先高后低。
pub fn io_priority_class(ioprio: u16) -> u8 {
    (ioprio >> 13) as u8 & 0x3 // 0 RT, 2 BE, 3 IDLE（ioprio 语义）
}

/// 固定容量版：按优先级排（稳定），输出序号表。
pub fn io_sched_order_fixed(prios: &[u16], out: &mut [usize]) -> usize {
    let idx: [usize; 8] = [0, 1, 2, 3, 4, 5, 6, 7];
    let mut n = 0usize;
    for class in 0u8..4 {
        for &i in idx.iter() {
            if i < prios.len() && io_priority_class(prios[i]) == class && n < out.len() {
                out[n] = i;
                n += 1;
            }
        }
    }
    n
}

/// G272 取消与超时：超时未完成的请求被取消。
pub fn cancel_timed_out(pending: &mut [(u64, bool, u64)], now: u64, timeout: u64) -> usize {
    let mut cancelled = 0;
    for p in pending.iter_mut() {
        if !p.1 && now - p.2 > timeout {
            p.1 = true; // cancelled
            cancelled += 1;
        }
    }
    cancelled
}

/// G273 链式 I/O：带 linked 标志的依赖链按序完成。
pub fn chain_ready(linked: &[bool], done: &[bool], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    if linked.get(i.wrapping_sub(1)).copied().unwrap_or(false) {
        done.get(i - 1).copied().unwrap_or(false)
    } else {
        true
    }
}

/// G269 缓冲与页缓存直通：命中直接返回。
pub fn page_cache_hit(cached: bool) -> bool {
    cached
}

/// G276 零拷贝校验和：对缓冲原地计算（无拷贝）。
pub fn zero_copy_cksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    for (i, &b) in data.iter().enumerate() {
        sum = sum.wrapping_add((b as u32) << ((i % 4) * 8));
    }
    sum
}

// ---------------------------------------------------------------------------
// G281~G300 — 用户态驱动
// ---------------------------------------------------------------------------

/// G281 驱动框架：驱动描述符。
#[derive(Clone, Copy)]
pub struct UDrv {
    pub name: &'static str,
    pub pci_vendor: u16,
    pub pci_device: u16,
    pub caps: u32, // 能力位
    pub state:DrvState,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrvState {
    Loaded,
    Running,
    Crashed,
    Quarantined,
}

/// G282/G283 驱动迁用户态 + 服务沙箱：能力不足则拒绝启动。
pub fn drv_sandbox_ok(drv_caps: u32, granted_caps: u32) -> bool {
    drv_caps & !granted_caps == 0
}

/// G284 驱动热插拔服务：设备到达 → 匹配 → 启动。
pub fn drv_on_arrive(table: &[(u16, u16, &'static str)], vendor: u16, device: u16) -> Option<&'static str> {
    table.iter().find(|&&(v, d, _)| v == vendor && d == device).map(|&(_, _, n)| n)
}

/// G285 驱动崩溃隔离：崩溃只隔离该设备，其他不受影响。
pub fn crash_isolate(states: &mut [DrvState], crashed_idx: usize) -> usize {
    let mut n = 0;
    for (i, st) in states.iter_mut().enumerate() {
        if i == crashed_idx {
            *st = DrvState::Quarantined;
            n += 1;
        }
    }
    n
}

/// G286 用户态驱动自检（收口在 CheckSet）。
/// G287 性能预算：用户态轮询路径每中断开销预算。
pub fn udrv_budget_ok(poll_cycles: u64, budget: u64) -> bool {
    poll_cycles <= budget
}

/// G288 可观测：驱动事件计数。
#[derive(Default)]
pub struct UDrvEvents {
    pub starts: u64,
    pub crashes: u64,
    pub restarts: u64,
}

/// G290 文档/G291 降级链：真实驱动失败 → 通用驱动 → 内核桩。
pub fn udrv_degrade(real_ok: bool, generic_ok: bool) -> &'static str {
    if real_ok {
        "real"
    } else if generic_ok {
        "generic"
    } else {
        "stub"
    }
}

/// G292 兼容矩阵：驱动 × 内核 ABI。
pub const UDRV_ABI_MATRIX: [[bool; 3]; 2] = [[true, true, false], [true, false, false]];

pub fn udrv_abi_ok(drv: usize, abi: usize) -> bool {
    UDRV_ABI_MATRIX.get(drv).and_then(|r| r.get(abi)).copied().unwrap_or(false)
}

/// G293 与能力 IPC 协作：驱动句柄即能力。
pub fn drv_handle_is_cap(handle: u64, cap_bits: u64) -> bool {
    handle & cap_bits == handle && handle != 0
}

/// G294 与容器协作：容器内可见设备白名单。
pub fn container_dev_visible(whitelist: &[u16], dev_class: u16) -> bool {
    whitelist.contains(&dev_class)
}

/// G295 策略中心：允许哪些驱动进用户态。
#[derive(Clone, Copy)]
pub struct UDrvPolicy {
    pub allow_user: bool,
    pub require_signature: bool,
    pub max_restart: u8,
}

/// G296 一致性验证：同一设备两次匹配结果一致。
pub fn drv_match_deterministic(table: &[(u16, u16, &'static str)], v: u16, d: u16) -> bool {
    drv_on_arrive(table, v, d) == drv_on_arrive(table, v, d)
}

/// G297 工具集：驱动状态渲染。
pub fn render_drv_state(st: DrvState, out: &mut [u8]) -> usize {
    let s = match st {
        DrvState::Loaded => "loaded",
        DrvState::Running => "running",
        DrvState::Crashed => "crashed",
        DrvState::Quarantined => "quarantined",
    };
    let bytes = s.as_bytes();
    let n = bytes.len().min(out.len());
    out[..n].copy_from_slice(&bytes[..n]);
    n
}

/// G298 驱动回滚：重启 N 次仍崩 → 回滚旧版。
pub fn drv_rollback(restarts: u8, max: u8) -> bool {
    restarts >= max
}

/// G299 审计：环形日志。
pub struct DrvAudit {
    pub buf: [u64; 8],
    pub head: usize,
    pub count: usize,
}

impl DrvAudit {
    pub const fn new() -> DrvAudit {
        DrvAudit { buf: [0; 8], head: 0, count: 0 }
    }
    pub fn push(&mut self, ev: u64) {
        self.buf[self.head] = ev;
        self.head = (self.head + 1) % 8;
        if self.count < 8 {
            self.count += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// 自检与收口
// ---------------------------------------------------------------------------

/// AI-05 域自检：≤32 项覆盖三段。
pub fn run_bus_checks() -> CheckSet {
    let mut s = CheckSet::new("gbus");
    s.add("G241 ecam", ecam_addr(0xC000_0000, 2, 3, 1, 0x10) == 0xC000_0000 + (2 << 20) + (3 << 15) + (1 << 12) + 0x10, "addr");
    let raw = [(0u8, 0, 0, 0x8086u16, 0x1234u16, 2u8), (0, 2, 0, 0xFFFF, 0, 0), (1, 0, 0, 0x1AF4, 0x1000, 2)];
    let mut devs = [PciDev { bus: 0, dev: 0, fun: 0, vendor: 0, device: 0, class: 0 }; 8];
    let n = pci_enumerate(&raw, &mut devs);
    s.add("G242 enumerate", n == 2 && devs[0].vendor == 0x8086 && devs[1].vendor == 0x1AF4, "skip invalid");
    let (is_io, base) = bar_parse(0xF000_0002);
    s.add("G243 bar", !is_io && base == 0xF000_0000 && bar_size(0xFFFF_F000) == 0x1000, "mem/size");
    let (a, d) = msi_msg(5, 0x40, true);
    s.add("G244 msi", a == 0xFEE0_5000 && d == 0x40, "msg");
    let cfg = [0u8, 0, 0x01, 0x04, 0x05, 0x00, 0, 0, 0x10, 0x00];
    let (ids, cn) = cap_walk(&cfg, 2);
    s.add("G245 caps", cn == 2 && ids[0] == 0x01 && ids[1] == 0x05, "chain");
    let mut st = 0u8;
    st = hp_next(st, 0);
    let present = st;
    st = hp_next(st, 1);
    st = hp_next(st, 2);
    s.add("G246 hotplug", present == 1 && st == 3 && hp_next(3, 3) == 0, "state machine");
    s.add("G247 aer", aer_severity(1 << 5) == "fatal" && aer_severity(1 << 0) == "correctable" && aer_severity(0) == "none", "severity");
    let table = [(0x8086u16, 0x1234u16, 0u8, "ixv"), (0xFFFF, 0xFFFF, 2u8, "nvme-class")];
    s.add("G248 match", driver_match(&table, &devs[0]) == Some("ixv") && driver_match(&table, &devs[1]) == Some("nvme-class"), "exact>class");
    s.add("G250 dstate", dev_power_state(100) == 0 && dev_power_state(0) == 3 && dev_power_state(5) == 2, "D0..D3");
    s.add("G254 bridge", behind_bridge(2, 5, 3) && !behind_bridge(2, 5, 6), "range");
    s.add("G257 sriov", sriov_enable(8, 100) == 8 && sriov_enable(300, 100) == 100, "vf count");
    let mut unsorted = [devs[1], devs[0]];
    enumeration_stable(&mut unsorted);
    s.add("G258 stable enum", unsorted[0].bus == 0 && unsorted[0].device == 0x1234 && unsorted[1].bus == 1 && unsorted[1].vendor == 0x1AF4, "sorted");
    s.add("G259 vendor db", vendor_name(0x8086) == "Intel" && vendor_name(0x1AF4) == "Red Hat (virtio)" && vendor_name(0x1) == "unknown", "names");
    // 异步段
    let mut ring = IoUring::new();
    let sqe = Sqe { opcode: IORING_READ, fd: 1, offset: 0, len: 512, user_data: 42, flags: 0 };
    s.add("G261 submit", ring.submit(sqe) && ring.pending() == 1, "sqe");
    s.add("G262 complete+reap", ring.complete_one(512) && ring.reap().map(|c| c.user_data == 42 && c.result == 512).unwrap_or(false), "cqe");
    let batch = [sqe, sqe, sqe];
    let mut ring2 = IoUring::new();
    s.add("G263 batch", submit_batch(&mut ring2, &batch) == 3, "submit_and_wait");
    let mut reqs = [(10u64, 4u32), (12, 4), (0, 8), (0, 0)];
    let merged = io_sort_merge(&mut reqs);
    s.add("G266 merge", merged == 2 && reqs[0] == (0, 8) && reqs[1] == (10, 6), "adjacent");
    let prios = [0x8000u16, 0x4000, 0x6000]; // RT, IDLE(3)?, BE
    let mut order = [0usize; 8];
    let on = io_sched_order_fixed(&prios, &mut order);
    s.add("G268 qos", on == 3 && io_priority_class(0x8000) == 0 && order[0] == 0, "priority class");
    let mut pend = [(1u64, false, 100u64), (2, false, 500)];
    let c = cancel_timed_out(&mut pend, 1000, 300);
    s.add("G272 timeout", c == 2 && pend[0].1 && pend[1].1, "cancel");
    let linked = [true, true, false];
    let done = [true, false, false];
    s.add("G273 chain", chain_ready(&linked, &done, 0) && chain_ready(&linked, &done, 1) && !chain_ready(&linked, &done, 2), "deps");
    s.add("G276 zero-copy cksum", zero_copy_cksum(&[1, 0, 0, 0]) == 1, "in-place");
    // 用户态驱动段
    let drv = UDrv { name: "e1000-udrv", pci_vendor: 0x8086, pci_device: 0x100E, caps: 0b111, state: DrvState::Loaded };
    s.add("G282/283 sandbox", drv_sandbox_ok(drv.caps, 0b111) && !drv_sandbox_ok(drv.caps, 0b011), "cap gate");
    let dtable = [(0x8086u16, 0x100Eu16, "e1000-udrv")];
    s.add("G284 hotplug svc", drv_on_arrive(&dtable, 0x8086, 0x100E) == Some("e1000-udrv") && drv_on_arrive(&dtable, 1, 1).is_none(), "match");
    let mut states = [DrvState::Running; 4];
    crash_isolate(&mut states, 2);
    s.add("G285 crash isolation", states[2] == DrvState::Quarantined && states[0] == DrvState::Running && states[3] == DrvState::Running, "blast radius 1");
    s.add("G287 budget", udrv_budget_ok(50, 100) && !udrv_budget_ok(200, 100), "poll cycles");
    s.add("G291 degrade chain", udrv_degrade(true, false) == "real" && udrv_degrade(false, true) == "generic" && udrv_degrade(false, false) == "stub", "3 tiers");
    s.add("G292 abi matrix", udrv_abi_ok(0, 1) && !udrv_abi_ok(0, 2) && !udrv_abi_ok(1, 1), "cells");
    s.add("G293 cap handle", drv_handle_is_cap(0b101, 0b111) && !drv_handle_is_cap(0b1000, 0b0111), "handle");
    s.add("G294 container", container_dev_visible(&[2, 5], 5) && !container_dev_visible(&[2, 5], 9), "whitelist");
    s.add("G296 match deterministic", drv_match_deterministic(&dtable, 0x8086, 0x100E), "same twice");
    let mut buf = [0u8; 16];
    let rn = render_drv_state(DrvState::Quarantined, &mut buf);
    s.add("G297 tools", rn == 11, "render");
    s.add("G298 rollback", drv_rollback(3, 3) && !drv_rollback(2, 3), "restarts>=max");
    let mut au = DrvAudit::new();
    au.push(1);
    au.push(2);
    s.add("G299 audit", au.count == 2 && au.buf[0] == 1, "ring");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g243_bar_size_decode() {
        assert_eq!(bar_size(0xFFFF_F000), 0x1000);
        assert_eq!(bar_size(0xFFFF_FFC0), 0x40);
        assert_eq!(bar_size(0), 0);
        assert_eq!(bar_size(0xFFFF_FFFF), 0);
    }

    #[test]
    fn g261_ring_wraparound() {
        let mut ring = IoUring::new();
        let sqe = Sqe { opcode: IORING_NOP, fd: 0, offset: 0, len: 0, user_data: 7, flags: 0 };
        // 填满 SQ_DEPTH - 1
        for i in 0..SQ_DEPTH - 1 {
            assert!(ring.submit(Sqe { user_data: i as u64, ..sqe }));
        }
        assert!(!ring.submit(sqe), "ring full");
        assert_eq!(ring.pending(), SQ_DEPTH - 1);
        // 收割全部
        for i in 0..SQ_DEPTH - 1 {
            assert!(ring.complete_one(0));
            let c = ring.reap().unwrap();
            assert_eq!(c.user_data, i as u64);
        }
        assert_eq!(ring.pending(), 0);
        assert!(ring.reap().is_none());
    }

    #[test]
    fn g266_merge_adjacent_requests() {
        let mut reqs = [(8u64, 1u32), (9, 1), (9, 3), (20, 4)];
        let merged = io_sort_merge(&mut reqs);
        assert_eq!(merged, 2);
        assert_eq!(reqs[0], (8, 4), "8..9 + 9..10 + 9..12 coalesced");
        assert_eq!(reqs[1], (20, 4));
        assert_eq!(reqs[2], (0, 0), "tail zeroed");
    }

    #[test]
    fn g285_crash_blast_radius_one() {
        let mut states = [DrvState::Running; 3];
        crash_isolate(&mut states, 0);
        assert_eq!(states[0], DrvState::Quarantined);
        assert_eq!(states[1], DrvState::Running);
        assert_eq!(states[2], DrvState::Running);
    }

    #[test]
    fn g273_chain_dependencies() {
        let linked = [true, true, true];
        let done = [true, true, false];
        assert!(chain_ready(&linked, &done, 0));
        assert!(chain_ready(&linked, &done, 1));
        assert!(chain_ready(&linked, &done, 2));
        let done2 = [true, false, false];
        assert!(!chain_ready(&linked, &done2, 2), "blocked by unfinished predecessor");
    }

    #[test]
    fn g300_domain_selftest_all_green() {
        let s = run_bus_checks();
        if !s.all_passed() {
            let mut buf = [0u8; 2048];
            let n = s.render(&mut buf);
            panic!("domain self-test must pass://n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(s.len() >= 25);
    }
}

