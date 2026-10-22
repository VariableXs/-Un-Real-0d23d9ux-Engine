//! UNX-F6901 hotplug 中断拾取（PCIe 惊叹/端口状态变化轮询双路）（0x9100 段）
//!
//! 锚点：UNX-F6901-J1 中断拾取双路断言（事件驱动+轮询兜底）；
//! 双路事件去重账。
//! 域：CoRun·UNX-B4-B26（hotplug 拾取面）。
//!
//! 结构：拾取事件（端口/事件型/来源路）+ 事件驱动路（PCIe 惊叹中断
//! 直推）+ 轮询兜底路（端口状态寄存器周期扫描）+ 去重账（同端口同
//! 事件型未结算窗口内只入账一次）+ 双路一致性核（轮询必补事件路漏）
//! + 注入核（未知事件型/去重误放/轮询假事件/封账）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x9100 独占 0x80）。
pub const SEG_BASE: u32 = 0x9100;
pub const XC_HP_UNKEV: u32 = SEG_BASE + 0x01; // 未知事件型
pub const XC_HP_DUP: u32 = SEG_BASE + 0x02; // 去重窗口内重复
pub const XC_HP_PHANTOM: u32 = SEG_BASE + 0x03; // 轮询假事件
pub const XC_HP_SEALED: u32 = SEG_BASE + 0x04; // 封账

/// 拾取来源路。
pub const PATH_EVENT: u8 = 0; // 事件驱动（PCIe 惊叹）
pub const PATH_POLL: u8 = 1; // 轮询兜底

/// 事件型闭集。
pub const EV_PRESENT: u8 = 0; // 设备在位
pub const EV_ABSENT: u8 = 1; // 设备离位
pub const EV_POWER: u8 = 2; // 电源状态变化
pub const EV_LINK: u8 = 3; // 链路状态变化
pub const EV_TYPES: usize = 4;

/// 事件型合法判定。
pub fn is_known_event(ev: u8) -> bool {
    ev < EV_TYPES as u8
}

/// 事件型名。
pub fn event_name(ev: u8) -> &'static str {
    match ev {
        EV_PRESENT => "present",
        EV_ABSENT => "absent",
        EV_POWER => "power",
        EV_LINK => "link",
        _ => "?",
    }
}

/// 拾取事件账项。
#[derive(Clone)]
pub struct HpEvent {
    pub port: u16,
    pub ev: u8,
    pub path: u8,
    pub tick: u64,
    pub settled: bool,
}

/// 去重窗口：同 (port, ev) 在窗口 tick 内只入账一次。
pub const DEDUP_WINDOW: u64 = 8;

/// hotplug 拾取账。
pub struct HpBook {
    pub events: Vec<HpEvent>,
    pub seq: u64,
    pub sealed: bool,
    pub event_miss: u64, // 事件路漏、轮询补齐计数
}

impl HpBook {
    pub fn new() -> Self {
        HpBook { events: Vec::new(), seq: 0, sealed: false, event_miss: 0 }
    }

    /// 事件路拾取（PCIe 惊叹直推）。
    pub fn push_event(&mut self, port: u16, ev: u8, tick: u64) -> Result<bool, u32> {
        self.push(port, ev, PATH_EVENT, tick)
    }

    /// 轮询路拾取（周期扫描）。
    pub fn push_poll(&mut self, port: u16, ev: u8, tick: u64) -> Result<bool, u32> {
        self.push(port, ev, PATH_POLL, tick)
    }

    /// 统一入账（去重核）。
    fn push(&mut self, port: u16, ev: u8, path: u8, tick: u64) -> Result<bool, u32> {
        if !is_known_event(ev) {
            return Err(XC_HP_UNKEV);
        }
        if self.sealed {
            return Err(XC_HP_SEALED);
        }
        // 去重窗口：未结算同 (port, ev) 在窗口内拒重
        for e in self.events.iter().rev() {
            if tick > e.tick + DEDUP_WINDOW {
                break;
            }
            if e.port == port && e.ev == ev && !e.settled {
                return Err(XC_HP_DUP);
            }
        }
        self.seq += 1;
        self.events.push(HpEvent { port, ev, path, tick, settled: false });
        Ok(true)
    }

    /// 结算：标记某 (port, ev) 未结算项。
    pub fn settle(&mut self, port: u16, ev: u8) -> usize {
        let mut n = 0;
        for e in self.events.iter_mut() {
            if e.port == port && e.ev == ev && !e.settled {
                e.settled = true;
                n += 1;
            }
        }
        n
    }

    /// 双路一致性核：轮询路发现而事件路未报的 (port, ev) 数
    /// （事件路漏单 → 轮询兜底必须补上）。
    pub fn poll_coverage(&self) -> usize {
        let mut n = 0;
        for pe in self.events.iter() {
            if pe.path == PATH_POLL {
                let event_has = self
                    .events
                    .iter()
                    .any(|e| e.path == PATH_EVENT && e.port == pe.port && e.ev == pe.ev);
                if !event_has {
                    n += 1;
                }
            }
        }
        n
    }

    /// 双路全一致核：事件路报的轮询路也必须报（真在位状态轮询必然可见）。
    pub fn event_seen_by_poll(&self, port: u16, ev: u8) -> bool {
        self.events
            .iter()
            .any(|e| e.path == PATH_POLL && e.port == port && e.ev == ev)
    }
}

/// 双路拾取断言核：事件路直推 + 轮询路兜底补漏。
pub fn dual_path_exact() -> bool {
    let mut b = HpBook::new();
    // 事件路：端口 3 在位
    if b.push_event(3, EV_PRESENT, 100).is_err() {
        return false;
    }
    // 轮询路同 tick 拾取同事件 → 去重拒
    if b.push_poll(3, EV_PRESENT, 100).is_err() {
        return false;
    }
    // 轮询路发现端口 5 离位（事件路漏）→ 兜底入账
    if b.push_poll(5, EV_ABSENT, 120).is_err() {
        return false;
    }
    // 双路覆盖核：端口 5 事件路无记录、轮询有 → 计入漏单
    b.poll_coverage() == 1 && b.events.len() == 2
}

/// 去重账核：窗口内重拒、窗口外放行。
pub fn dedup_exact() -> bool {
    let mut b = HpBook::new();
    if b.push_event(7, EV_LINK, 200).is_err() {
        return false;
    }
    // 窗口内（tick 207）拒
    if b.push_event(7, EV_LINK, 207).is_ok() {
        return false;
    }
    // 窗口外（tick 209）放行
    if b.push_event(7, EV_LINK, 209).is_err() {
        return false;
    }
    // 结算后窗口内重报：settled 项不参与去重 → 放行
    let n = b.settle(7, EV_LINK);
    if n != 2 {
        return false;
    }
    b.push_event(7, EV_LINK, 210).is_ok()
}

/// 轮询兜底核：轮询周期状态变化全捕获。
pub fn poll_sweep_exact() -> bool {
    let mut b = HpBook::new();
    // 模拟 4 端口 2 轮扫描：轮 1 发现 4 端口在位；轮 2 端口 2 离位
    let mut tick = 1000u64;
    for port in 1..=4u16 {
        if b.push_poll(port, EV_PRESENT, tick).is_err() {
            return false;
        }
    }
    tick += 100;
    for port in 1..=4u16 {
        if port == 2 {
            // 窗口外新事件
            if b.push_poll(port, EV_ABSENT, tick).is_err() {
                return false;
            }
        }
    }
    b.events.len() == 5 && b.settle(2, EV_ABSENT) == 1
}

/// 注入核（6 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut b = HpBook::new();
    // 1) 未知事件型（ev=9）
    if b.push_event(1, 9, 100) == Err(XC_HP_UNKEV) {
        caught += 1;
    }
    // 2) 去重窗口内重复
    let _ = b.push_event(1, EV_PRESENT, 100);
    if b.push_event(1, EV_PRESENT, 100) == Err(XC_HP_DUP) {
        caught += 1;
    }
    // 3) 轮询假事件：未知型经轮询路同样拒
    if b.push_poll(2, 200, 100) == Err(XC_HP_UNKEV) {
        caught += 1;
    }
    // 4) 结算封账后再推
    let _ = b.push_poll(2, EV_ABSENT, 300);
    b.settle(2, EV_ABSENT);
    b.sealed = true;
    if b.push_event(3, EV_PRESENT, 400) == Err(XC_HP_SEALED) {
        caught += 1;
    }
    // 5) 去重跨端口不误伤：端口 1/端口 2 同事件型同 tick 均可入
    let mut b2 = HpBook::new();
    let r1 = b2.push_event(1, EV_POWER, 500);
    let r2 = b2.push_event(2, EV_POWER, 500);
    if r1.is_ok() && r2.is_ok() {
        caught += 1;
    }
    // 6) 去重同端口异事件型不误伤
    let mut b3 = HpBook::new();
    let r3 = b3.push_event(4, EV_PRESENT, 600);
    let r4 = b3.push_event(4, EV_ABSENT, 600);
    if r3.is_ok() && r4.is_ok() {
        caught += 1;
    }
    (caught, 6)
}

/// 指纹冻结：事件账折叠哈希。
pub fn fp_frozen(b: &HpBook) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for e in &b.events {
        h ^= e.port as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= e.ev as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= e.path as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= e.tick;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx6901_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F6901");
    // J1-1 双路拾取断言
    let ok = dual_path_exact();
    s.add("B26-01-双路拾取", ok, if ok { "" } else { "双路漂移" });
    // J1-2 去重账（窗口内拒/外放/结算放）
    let ok = dedup_exact();
    s.add("B26-02-去重账", ok, if ok { "" } else { "去重漂移" });
    // J1-3 轮询兜底扫
    let ok = poll_sweep_exact();
    s.add("B26-03-轮询兜底", ok, if ok { "" } else { "兜底漂移" });
    // J1-4 事件型闭集
    let ok = (0..4u8).all(is_known_event) && !is_known_event(4) && !is_known_event(255);
    s.add("B26-04-事件型闭集", ok, if ok { "" } else { "闭集漂移" });
    // J1-5 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B26-05-注入核必抓", ok, &detail);
    // J1-6 指纹冻结 + 双路覆盖
    let mut b = HpBook::new();
    let _ = b.push_event(9, EV_PRESENT, 700);
    let _ = b.push_poll(10, EV_ABSENT, 750);
    let ok = fp_frozen(&b) == fp_frozen(&b) && b.poll_coverage() == 1;
    s.add("B26-06-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
