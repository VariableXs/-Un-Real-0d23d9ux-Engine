//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0463 桥展开多总线扫描（只读递归） · 0x754C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，WBV1104·AI-VX7-1104）：
//!   多总线递归扫描（B0~B255 闭集），桥设备展开子总线号账，只读红
//!   线零配置写。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   ① PCI 总线树递归扫描模型：总线 0 起步，遇到 header type 01 的
//!     桥设备即取其 secondary bus number（偏移 0x19，PCI Local Bus
//!     Spec r3.0 §6.1）递归展开；扫描账逐总线逐槽逐功能三重循环账齐。
//!   ② 总线号闭集 0~255：`u8` 承载 + 256 位 visited 位账双闸，环回
//!     总线（子总线指向已访问总线）显性记 BridgeLoop 拒，不无限递归。
//!   ③ 只读红线：解析器只做读路径——`ConfigShim` 的写路径恒拒且
//!     计数入账，全链走完后 `write_ops == 0` 是硬判据；任何配置写
//!     尝试必被记账并显性回 Err。
//!   ④ 递归深度上限 16（冻结）：深链拓扑注入必抓 DepthOver，防栈
//!     溢出；单诊码窗口 0x40 内闭集查表。
//!   ⑤ 单调相位状态机：Idle→Scanning→Bridging→Settled 倒退显性拒，
//!     扫描账可复算（双跑一致红线）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x754C0，AI-VX7-1104 批F 独占区）。
pub const SEG_BASE: u32 = 0x754C0;

/// 总线号闭集上沿（PCI Local Bus Spec r3.0：总线号 8 位，0~255）。
pub const BUS_CLOSED_MAX: u16 = 255;
/// visited 位账容量（闭集 0..=256 总线）。
pub const BUS_CLOSED: usize = 256;
/// 每总线槽数（Spec §3.2.2：device number 5 位，0~31）。
pub const SLOTS_PER_BUS: usize = 32;
/// 每设备功能数（Spec §3.2.2：function number 3 位，0~7）。
pub const FUNCS_PER_DEV: usize = 8;
/// 每总线功能读次数（守恒分母：32 槽 × 8 功能）。
pub const FUNCS_PER_BUS: usize = SLOTS_PER_BUS * FUNCS_PER_DEV;
/// 递归深度上限（冻结 16）。
pub const MAX_DEPTH: usize = 16;
/// header type 01 = PCI-to-PCI bridge（Spec §6.1）。
pub const HDR_TYPE_BRIDGE: u8 = 0x01;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BridgeCode {
    /// 总线号越出 0~255 闭集
    BusOver,
    /// 桥环：子总线号已访问
    BridgeLoop,
    /// 配置读扑空（账内无此总线）
    ReadMiss,
    /// 配置写尝试被拒（只读红线）
    WriteRejected,
    /// 递归深度越限
    DepthOver,
    /// 扫描相位倒退（状态机红线）
    WalkRetrograde,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl BridgeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BridgeCode::BusOver => "总线号越界：闭集 0~255，请核对 secondary/subordinate 账",
            BridgeCode::BridgeLoop => "桥环检出：子总线号已访问，请核拓扑连线或桥账",
            BridgeCode::ReadMiss => "配置读扑空：总线未入账，请先 add_bus 再扫描",
            BridgeCode::WriteRejected => "配置写被拒：只读红线，解析器只做读路径",
            BridgeCode::DepthOver => "递归深度越限：>16 层，拓扑过深或桥账错链",
            BridgeCode::WalkRetrograde => "扫描相位倒退：状态机不许回退，请按序推进",
            BridgeCode::NotReproducible => "双跑不一致：复现红线，扫描须确定性",
        }
    }
    pub const ALL: [BridgeCode; 7] = [
        BridgeCode::BusOver,
        BridgeCode::BridgeLoop,
        BridgeCode::ReadMiss,
        BridgeCode::WriteRejected,
        BridgeCode::DepthOver,
        BridgeCode::WalkRetrograde,
        BridgeCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < BridgeCode::ALL.len() {
        if let Some(c) = BridgeCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x754C0 段闭集内"
}

// ── 域模型：设备/总线/配置垫片 ──────────────────────────────────
/// 设备头类型三态闭集 + 空槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderKind {
    /// 空槽（无设备）
    Empty,
    /// header type 00：通用端点设备
    Endpoint,
    /// header type 01：PCI-to-PCI 桥
    Bridge,
    /// header type 02：CardBus 桥
    CardBus,
    /// 未知类型（低 7 位 >2）
    Unknown,
}

/// 设备配置影子账（只读模型，不做真实 MMIO）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevCfg {
    pub vendor: u16,
    pub device: u16,
    /// header type 低 7 位（位 7 = multi-function，此账只按类型分派）
    pub hdr_type: u8,
    /// 桥设备 secondary bus number（0x19 偏移），非桥为 None
    pub secondary: Option<u8>,
    /// 桥设备 subordinate bus number（0x1A 偏移），非桥为 None
    pub subordinate: Option<u8>,
}

impl DevCfg {
    /// 头类型分派（只看低 7 位；位 7 是 multi-function 位，不改类型归属）。
    pub fn kind(&self) -> HeaderKind {
        if self.vendor == 0xFFFF {
            return HeaderKind::Empty;
        }
        match self.hdr_type & 0x7F {
            0x00 => HeaderKind::Endpoint,
            HDR_TYPE_BRIDGE => HeaderKind::Bridge,
            0x02 => HeaderKind::CardBus,
            _ => HeaderKind::Unknown,
        }
    }
}

/// 单总线设备账。
#[derive(Clone, Debug, Default)]
pub struct BusCfg {
    pub bus: u8,
    /// (slot, func, DevCfg) 平账；读路径按三键查
    pub funcs: Vec<(u8, u8, DevCfg)>,
}

/// 配置空间垫片：只读红线本体——读路径入账，写路径恒拒。
#[derive(Clone, Debug, Default)]
pub struct ConfigShim {
    pub buses: Vec<BusCfg>,
    /// 读操作计数（扫描账复核用）
    pub read_ops: u64,
    /// 写操作计数（只读红线：全链后必须 == 0）
    pub write_ops: u64,
}

impl ConfigShim {
    pub fn new() -> ConfigShim {
        ConfigShim::default()
    }

    /// 登记一条设备账（拓扑装载期，不算读也不算写）。
    pub fn add_dev(&mut self, bus: u8, slot: u8, func: u8, dev: DevCfg) -> Result<usize, u32> {
        if slot as usize >= SLOTS_PER_BUS || func as usize >= FUNCS_PER_DEV {
            return Err(BridgeCode::BusOver.val());
        }
        // 找到或创建该总线的账条目。
        let mut found: Option<usize> = None;
        let mut i = 0;
        while i < self.buses.len() {
            if let Some(b) = self.buses.get(i) {
                if b.bus == bus {
                    found = Some(i);
                }
            }
            i += 1;
        }
        let idx = match found {
            Some(k) => k,
            None => {
                self.buses.push(BusCfg { bus, funcs: Vec::new() });
                // 新条目在尾部（push 后 len-1，闭集内安全）。
                self.buses.len().checked_sub(1).ok_or(BridgeCode::ReadMiss.val())?
            }
        };
        // 同 (slot,func) 重复登记拒。
        let mut j = 0;
        while j < self.buses.len() {
            if let Some(b) = self.buses.get(j) {
                if b.bus == bus {
                    let mut k = 0;
                    while k < b.funcs.len() {
                        if let Some((s, f, _)) = b.funcs.get(k) {
                            if *s == slot && *f == func {
                                return Err(BridgeCode::ReadMiss.val());
                            }
                        }
                        k += 1;
                    }
                }
            }
            j += 1;
        }
        if let Some(b) = self.buses.get_mut(idx) {
            b.funcs.push((slot, func, dev));
        }
        Ok(idx)
    }

    /// 配置读（只读路径唯一入口）：空槽回 Ok(None)，账缺总线回 Err。
    pub fn read_dev(&mut self, bus: u8, slot: u8, func: u8) -> Result<Option<DevCfg>, u32> {
        if slot as usize >= SLOTS_PER_BUS || func as usize >= FUNCS_PER_DEV {
            return Err(BridgeCode::BusOver.val());
        }
        self.read_ops = self.read_ops.saturating_add(1);
        let mut i = 0;
        while i < self.buses.len() {
            if let Some(b) = self.buses.get(i) {
                if b.bus == bus {
                    let mut k = 0;
                    while k < b.funcs.len() {
                        if let Some((s, f, d)) = b.funcs.get(k) {
                            if *s == slot && *f == func {
                                return Ok(Some(d.clone()));
                            }
                        }
                        k += 1;
                    }
                    return Ok(None);
                }
            }
            i += 1;
        }
        Err(BridgeCode::ReadMiss.val())
    }

    /// 配置写（只读红线：恒拒且显性记账）。
    pub fn cfg_write(&mut self, _bus: u8, _slot: u8, _func: u8, _off: u8, _val: u32) -> Result<(), u32> {
        self.write_ops = self.write_ops.saturating_add(1);
        Err(BridgeCode::WriteRejected.val())
    }
}

// ── 扫描账（逐总线逐槽逐功能三重循环产出） ──────────────────────
#[derive(Clone, Debug, Default)]
pub struct WalkLedger {
    /// visited 位账：256 总线闭集
    pub visited: Vec<bool>,
    /// 访问序（总线号入账序）
    pub order: Vec<u8>,
    /// 桥展开账：(bus, slot, func, secondary)
    pub bridges: Vec<(u8, u8, u8, u8)>,
    /// 已扫描总线数
    pub bus_scans: u32,
    /// 槽扫描计数（bus_scans × 32）
    pub slot_scans: u32,
    /// 功能读计数（守恒分母：bus_scans × 256）
    pub func_reads: u32,
    /// 桥环检出次数
    pub loops: u32,
}

impl WalkLedger {
    pub fn new() -> WalkLedger {
        WalkLedger { visited: alloc::vec![false; BUS_CLOSED], ..Default::default() }
    }

    pub fn visited_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < self.visited.len() {
            if let Some(true) = self.visited.get(i) {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }
}

// ── 单调相位状态机（倒退显性拒） ────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WalkPhase {
    /// 未起步
    #[default]
    Idle,
    /// 总线扫描中
    Scanning,
    /// 桥递归展开中
    Bridging,
    /// 全树收束
    Settled,
}

impl WalkPhase {
    pub fn rank(self) -> u8 {
        match self {
            WalkPhase::Idle => 0,
            WalkPhase::Scanning => 1,
            WalkPhase::Bridging => 2,
            WalkPhase::Settled => 3,
        }
    }
}

/// 桥展开扫描器（只读递归本体）。
pub struct BridgeWalker {
    pub phase: WalkPhase,
}

impl BridgeWalker {
    pub fn new() -> BridgeWalker {
        BridgeWalker { phase: WalkPhase::Idle }
    }

    /// 相位推进（同相允许，倒退显性拒）。
    pub fn advance(&mut self, next: WalkPhase) -> Result<(), u32> {
        if next.rank() < self.phase.rank() {
            return Err(BridgeCode::WalkRetrograde.val());
        }
        self.phase = next;
        Ok(())
    }

    /// 递归展开一条总线：入口闸（闭集/环/深度）→ 三重循环扫描 → 桥递归。
    pub fn walk(
        &mut self,
        shim: &mut ConfigShim,
        led: &mut WalkLedger,
        bus: u8,
        depth: usize,
    ) -> Result<(), u32> {
        if depth > MAX_DEPTH {
            return Err(BridgeCode::DepthOver.val());
        }
        if (bus as u16) > BUS_CLOSED_MAX {
            return Err(BridgeCode::BusOver.val());
        }
        let bi = bus as usize;
        if let Some(true) = led.visited.get(bi) {
            // 环回：显性记账后拒，不无限递归。
            led.loops = led.loops.saturating_add(1);
            return Err(BridgeCode::BridgeLoop.val());
        }
        if let Some(v) = led.visited.get_mut(bi) {
            *v = true;
        }
        let _ = led.order.push(bus);
        led.bus_scans = led.bus_scans.saturating_add(1);
        if depth == 0 {
            self.advance(WalkPhase::Scanning)?;
        }
        // 三重循环：总线 × 槽 × 功能。
        let mut slot = 0usize;
        while slot < SLOTS_PER_BUS {
            led.slot_scans = led.slot_scans.saturating_add(1);
            let mut func = 0usize;
            while func < FUNCS_PER_DEV {
                led.func_reads = led.func_reads.saturating_add(1);
                let got = shim.read_dev(bus, slot as u8, func as u8)?;
                if let Some(dev) = got {
                    let kind = dev.kind();
                    if kind == HeaderKind::Bridge {
                        if let Some(sec) = dev.secondary {
                            self.advance(WalkPhase::Bridging)?;
                            let _ = led.bridges.push((bus, slot as u8, func as u8, sec));
                            self.walk(shim, led, sec, depth.saturating_add(1))?;
                        }
                    }
                }
                func += 1;
            }
            slot += 1;
        }
        Ok(())
    }

    /// 全链：总线 0 起步，走完收束到 Settled。
    pub fn full_walk(&mut self, shim: &mut ConfigShim, led: &mut WalkLedger) -> Result<(), u32> {
        self.walk(shim, led, 0, 0)?;
        self.advance(WalkPhase::Settled)?;
        Ok(())
    }
}

// ── 冻结拓扑（真机影子：4 总线 3 桥，来源：PCIe 拓扑样例账） ────
/// 真机拓扑账：bus0 根复合体 + 两条根口 + 下游挂桥。
pub fn machine_topology() -> Vec<(u8, u8, u8, DevCfg)> {
    vec![
        // bus 0：host bridge + 两个 PCIe 根口 + 南桥端点
        (0, 0, 0, DevCfg { vendor: 0x8086, device: 0x1237, hdr_type: 0x00, secondary: None, subordinate: None }),
        (0, 2, 0, DevCfg { vendor: 0x8086, device: 0x15D8, hdr_type: HDR_TYPE_BRIDGE, secondary: Some(2), subordinate: Some(5) }),
        (0, 3, 0, DevCfg { vendor: 0x8086, device: 0x15D9, hdr_type: HDR_TYPE_BRIDGE, secondary: Some(1), subordinate: Some(1) }),
        (0, 31, 0, DevCfg { vendor: 0x8086, device: 0x2922, hdr_type: 0x00, secondary: None, subordinate: None }),
        // bus 1：根口 3 下游网卡
        (1, 0, 0, DevCfg { vendor: 0x8086, device: 0x1539, hdr_type: 0x00, secondary: None, subordinate: None }),
        // bus 2：根口 2 下游 GPU + 一条子桥（展开到 bus 3）
        (2, 0, 0, DevCfg { vendor: 0x10DE, device: 0x1B80, hdr_type: 0x00, secondary: None, subordinate: None }),
        (2, 2, 0, DevCfg { vendor: 0x1B36, device: 0x000C, hdr_type: HDR_TYPE_BRIDGE, secondary: Some(3), subordinate: Some(3) }),
        // bus 3：子桥下游 NVMe 端点
        (3, 0, 0, DevCfg { vendor: 0x1AF4, device: 0x104D, hdr_type: 0x00, secondary: None, subordinate: None }),
    ]
}

/// 装配拓扑账（真机序）。
pub fn load_machine() -> ConfigShim {
    let mut shim = ConfigShim::new();
    let topo = machine_topology();
    let mut i = 0;
    while i < topo.len() {
        if let Some((b, s, f, d)) = topo.get(i) {
            let _ = shim.add_dev(*b, *s, *f, d.clone());
        }
        i += 1;
    }
    shim
}

/// 环拓扑注入账：bus0 桥→1，bus1 桥→0（环回必抓）。
pub fn loop_topology() -> ConfigShim {
    let mut shim = ConfigShim::new();
    let _ = shim.add_dev(0, 0, 0, DevCfg { vendor: 0x8086, device: 0x1237, hdr_type: 0x00, secondary: None, subordinate: None });
    let _ = shim.add_dev(0, 1, 0, DevCfg { vendor: 0x1B36, device: 0x000C, hdr_type: HDR_TYPE_BRIDGE, secondary: Some(1), subordinate: Some(1) });
    let _ = shim.add_dev(1, 0, 0, DevCfg { vendor: 0x1B36, device: 0x000D, hdr_type: HDR_TYPE_BRIDGE, secondary: Some(0), subordinate: Some(0) });
    shim
}

/// 深链拓扑生成器：n 级桥逐级下挂（深度越限注入用）。
pub fn chain_topology(n: usize) -> ConfigShim {
    let mut shim = ConfigShim::new();
    let _ = shim.add_dev(0, 0, 0, DevCfg { vendor: 0x8086, device: 0x1237, hdr_type: 0x00, secondary: None, subordinate: None });
    let mut b = 0usize;
    while b < n {
        let cur = b as u8;
        let next = match b.checked_add(1) {
            Some(v) if v <= BUS_CLOSED_MAX as usize => v as u8,
            _ => break,
        };
        let _ = shim.add_dev(cur, 1, 0, DevCfg {
            vendor: 0x1B36,
            device: 0x000C,
            hdr_type: HDR_TYPE_BRIDGE,
            secondary: Some(next),
            subordinate: Some(next),
        });
        b += 1;
    }
    shim
}

/// 边界拓扑：直达总线 255（闭集上沿可达）。
pub fn far_topology() -> ConfigShim {
    let mut shim = ConfigShim::new();
    let _ = shim.add_dev(0, 1, 0, DevCfg { vendor: 0x1B36, device: 0x000C, hdr_type: HDR_TYPE_BRIDGE, secondary: Some(255), subordinate: Some(255) });
    let _ = shim.add_dev(255, 0, 0, DevCfg { vendor: 0x1AF4, device: 0x1000, hdr_type: 0x00, secondary: None, subordinate: None });
    shim
}

// ── LCG（种子固定，注入轮驱动用） ───────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
/// 主拓扑全走：4 总线 3 桥，访问序起点 0。
fn main_walk_ok() -> bool {
    let mut shim = load_machine();
    let mut led = WalkLedger::new();
    let mut w = BridgeWalker::new();
    match w.full_walk(&mut shim, &mut led) {
        Ok(()) => {}
        Err(_) => return false,
    }
    if led.bus_scans != 4 || led.bridges.len() != 3 {
        return false;
    }
    if led.order.first().copied() != Some(0) {
        return false;
    }
    if led.visited_count() != 4 {
        return false;
    }
    true
}

/// 桥展开账逐条核：三桥的 (bus,slot,func,secondary) 精确匹配。
fn bridge_ledger_ok() -> bool {
    let mut shim = load_machine();
    let mut led = WalkLedger::new();
    let mut w = BridgeWalker::new();
    if w.full_walk(&mut shim, &mut led).is_err() {
        return false;
    }
    // 深度优先序：slot2 桥先递归展开 bus2（内含 slot2 子桥），再到 slot3 桥。
    let want = [(0u8, 2u8, 0u8, 2u8), (2, 2, 0, 3), (0, 3, 0, 1)];
    if led.bridges.len() != want.len() {
        return false;
    }
    let mut i = 0;
    while i < want.len() {
        match led.bridges.get(i) {
            Some(t) if *t == want[i] => {}
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 总线闭集：所有访问序总线号 ≤255，账位容量恰 256。
fn bus_closed_ok() -> bool {
    let mut shim = load_machine();
    let mut led = WalkLedger::new();
    let mut w = BridgeWalker::new();
    if w.full_walk(&mut shim, &mut led).is_err() {
        return false;
    }
    if led.visited.len() != BUS_CLOSED {
        return false;
    }
    let mut i = 0;
    while i < led.order.len() {
        if let Some(b) = led.order.get(i) {
            if (*b as u16) > BUS_CLOSED_MAX {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 只读红线：全链后写计数 == 0 且读计数 > 0。
fn readonly_ok() -> bool {
    let mut shim = load_machine();
    let mut led = WalkLedger::new();
    let mut w = BridgeWalker::new();
    if w.full_walk(&mut shim, &mut led).is_err() {
        return false;
    }
    if shim.write_ops != 0 || shim.read_ops == 0 {
        return false;
    }
    true
}

/// 三重循环守恒：func_reads == bus_scans × 256，slot_scans == bus_scans × 32。
fn conservation_ok() -> bool {
    let mut shim = load_machine();
    let mut led = WalkLedger::new();
    let mut w = BridgeWalker::new();
    if w.full_walk(&mut shim, &mut led).is_err() {
        return false;
    }
    let bs = led.bus_scans as u64;
    if led.func_reads as u64 != bs.saturating_mul(FUNCS_PER_BUS as u64) {
        return false;
    }
    if led.slot_scans as u64 != bs.saturating_mul(SLOTS_PER_BUS as u64) {
        return false;
    }
    true
}

/// 相位状态机：倒退显性拒（Settled→Scanning 回 Err）。
fn phase_monotonic_ok() -> bool {
    let mut w = BridgeWalker::new();
    if w.advance(WalkPhase::Scanning).is_err() {
        return false;
    }
    if w.advance(WalkPhase::Bridging).is_err() {
        return false;
    }
    if w.advance(WalkPhase::Settled).is_err() {
        return false;
    }
    // 倒退必抓。
    if w.advance(WalkPhase::Scanning).is_ok() {
        return false;
    }
    true
}

/// 注入-桥环必抓：环拓扑走链回 Err(BridgeLoop) 且 loops 计数 > 0。
fn inject_loop_ok() -> bool {
    let mut shim = loop_topology();
    let mut led = WalkLedger::new();
    let mut w = BridgeWalker::new();
    match w.full_walk(&mut shim, &mut led) {
        Err(e) if e == BridgeCode::BridgeLoop.val() => {}
        _ => return false,
    }
    if led.loops == 0 {
        return false;
    }
    true
}

/// 注入-深度越限必抓：深链拓扑（20 级桥）回 Err(DepthOver)。
fn inject_depth_ok() -> bool {
    let mut shim = chain_topology(20);
    let mut led = WalkLedger::new();
    let mut w = BridgeWalker::new();
    match w.full_walk(&mut shim, &mut led) {
        Err(e) if e == BridgeCode::DepthOver.val() => {}
        _ => return false,
    }
    true
}

/// 注入-配置写必抓：cfg_write 回 Err(WriteRejected) 且 write_ops 计入。
fn inject_write_ok() -> bool {
    let mut shim = load_machine();
    match shim.cfg_write(0, 0, 0, 0x04, 0x7) {
        Err(e) if e == BridgeCode::WriteRejected.val() => {}
        _ => return false,
    }
    if shim.write_ops != 1 {
        return false;
    }
    true
}

/// 边界：总线 255 桥展开可达（闭集上沿）。
fn boundary_far_ok() -> bool {
    let mut shim = far_topology();
    let mut led = WalkLedger::new();
    let mut w = BridgeWalker::new();
    if w.full_walk(&mut shim, &mut led).is_err() {
        return false;
    }
    if led.bus_scans != 2 {
        return false;
    }
    if let Some(false) = led.visited.get(255) {
        return false;
    }
    true
}

/// 双跑一致（复现红线）：两轮全链账全等。
fn repro_ok() -> bool {
    let mut a_shim = load_machine();
    let mut a_led = WalkLedger::new();
    let mut a_w = BridgeWalker::new();
    let a = match a_w.full_walk(&mut a_shim, &mut a_led) {
        Ok(()) => (a_led.order.clone(), a_led.bridges.clone(), a_led.func_reads, a_shim.read_ops),
        Err(_) => return false,
    };
    let mut b_shim = load_machine();
    let mut b_led = WalkLedger::new();
    let mut b_w = BridgeWalker::new();
    let b = match b_w.full_walk(&mut b_shim, &mut b_led) {
        Ok(()) => (b_led.order.clone(), b_led.bridges.clone(), b_led.func_reads, b_shim.read_ops),
        Err(_) => return false,
    };
    a == b
}

/// 段位守卫：诊断码全部落在 0x754C0 窗口内。
fn segment_guard_ok() -> bool {
    let mut i = 0;
    while i < BridgeCode::ALL.len() {
        if let Some(c) = BridgeCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    if SEG_BASE % 0x40 != 0 || !(SEG_BASE >= 0x75000 && SEG_BASE <= 0x781C0) {
        return false;
    }
    true
}

/// 诊断码闭集查表：7 码全部 detail 可达且非"未知"。
fn code_table_ok() -> bool {
    let mut i = 0;
    while i < BridgeCode::ALL.len() {
        if let Some(c) = BridgeCode::ALL.get(i) {
            let d = detail(c.val());
            if d.starts_with("未知诊断码") {
                return false;
            }
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 检查节 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push(("F0463-J1-段位守卫", segment_guard_ok(),
        format!("SEG_BASE=0x{:X} 窗口 0x40", SEG_BASE)));
    v.push(("F0463-J1-主拓扑全走", main_walk_ok(),
        format!("期望 4 总线 3 桥，实际判定 {}", main_walk_ok())));
    v.push(("F0463-J1-桥展开账", bridge_ledger_ok(),
        format!("三桥三元组逐条核，判定 {}", bridge_ledger_ok())));
    v.push(("F0463-J1-总线闭集0~255", bus_closed_ok(),
        format!("visited 容量={} 闭集核 {}", BUS_CLOSED, bus_closed_ok())));
    v.push(("F0463-J1-只读红线", readonly_ok(),
        format!("全链后 write_ops==0 read_ops>0，判定 {}", readonly_ok())));
    v.push(("F0463-J1-注入配置写必抓", inject_write_ok(),
        format!("cfg_write 恒拒且计数，判定 {}", inject_write_ok())));
    v.push(("F0463-J1-注入桥环必抓", inject_loop_ok(),
        format!("环拓扑回 BridgeLoop，判定 {}", inject_loop_ok())));
    v.push(("F0463-J1-注入深度越限必抓", inject_depth_ok(),
        format!("20 级深链回 DepthOver（上限 {}），判定 {}", MAX_DEPTH, inject_depth_ok())));
    v.push(("F0463-J1-边界总线255", boundary_far_ok(),
        format!("总线 255 桥展开可达，判定 {}", boundary_far_ok())));
    v.push(("F0463-J1-三重循环守恒", conservation_ok(),
        format!("func_reads==bus_scans*256 slot_scans==bus_scans*32，判定 {}", conservation_ok())));
    v.push(("F0463-J1-相位状态机", phase_monotonic_ok(),
        format!("倒退显性拒，判定 {}", phase_monotonic_ok())));
    v.push(("F0463-J2-双跑一致", repro_ok(),
        format!("两轮账全等，判定 {}", repro_ok())));
    v.push(("F0463-J1-诊断码闭集", code_table_ok(),
        format!("7 码查表可达，判定 {}", code_table_ok())));
    v
}

pub fn run_unx0463_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0463 桥展开多总线扫描");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（允许 assert!/assert_eq!/unwrap） ────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_walk_basics() {
        let mut shim = load_machine();
        let mut led = WalkLedger::new();
        let mut w = BridgeWalker::new();
        w.full_walk(&mut shim, &mut led).unwrap();
        assert_eq!(led.bus_scans, 4);
        assert_eq!(led.bridges.len(), 3);
        assert_eq!(led.order.first().copied(), Some(0));
        assert_eq!(led.visited_count(), 4);
    }

    #[test]
    fn readonly_redline_holds_after_full_walk() {
        let mut shim = load_machine();
        let mut led = WalkLedger::new();
        let mut w = BridgeWalker::new();
        w.full_walk(&mut shim, &mut led).unwrap();
        assert_eq!(shim.write_ops, 0);
        assert!(shim.read_ops > 0);
    }

    #[test]
    fn bridge_ledger_exact_match() {
        let mut shim = load_machine();
        let mut led = WalkLedger::new();
        let mut w = BridgeWalker::new();
        w.full_walk(&mut shim, &mut led).unwrap();
        assert_eq!(led.bridges, vec![(0, 2, 0, 2), (2, 2, 0, 3), (0, 3, 0, 1)]);
    }

    #[test]
    fn loop_injection_caught() {
        let mut shim = loop_topology();
        let mut led = WalkLedger::new();
        let mut w = BridgeWalker::new();
        let r = w.full_walk(&mut shim, &mut led);
        assert_eq!(r.unwrap_err(), BridgeCode::BridgeLoop.val());
        assert!(led.loops >= 1);
    }

    #[test]
    fn depth_injection_caught() {
        let mut shim = chain_topology(20);
        let mut led = WalkLedger::new();
        let mut w = BridgeWalker::new();
        let r = w.full_walk(&mut shim, &mut led);
        assert_eq!(r.unwrap_err(), BridgeCode::DepthOver.val());
    }

    #[test]
    fn write_injection_caught() {
        let mut shim = ConfigShim::new();
        let r = shim.cfg_write(0, 0, 0, 0x04, 0x7);
        assert_eq!(r.unwrap_err(), BridgeCode::WriteRejected.val());
        assert_eq!(shim.write_ops, 1);
    }

    #[test]
    fn far_bus_255_reachable() {
        let mut shim = far_topology();
        let mut led = WalkLedger::new();
        let mut w = BridgeWalker::new();
        w.full_walk(&mut shim, &mut led).unwrap();
        assert_eq!(led.bus_scans, 2);
        assert_eq!(*led.visited.get(255).unwrap(), true);
    }

    #[test]
    fn conservation_after_walk() {
        let mut shim = load_machine();
        let mut led = WalkLedger::new();
        let mut w = BridgeWalker::new();
        w.full_walk(&mut shim, &mut led).unwrap();
        assert_eq!(led.func_reads, led.bus_scans * (FUNCS_PER_BUS as u32));
        assert_eq!(led.slot_scans, led.bus_scans * (SLOTS_PER_BUS as u32));
    }

    #[test]
    fn phase_monotonic_rejects_retrograde() {
        let mut w = BridgeWalker::new();
        w.advance(WalkPhase::Settled).unwrap();
        assert_eq!(w.advance(WalkPhase::Idle).unwrap_err(), BridgeCode::WalkRetrograde.val());
    }

    #[test]
    fn reproducible_double_run() {
        assert!(repro_ok());
        let s1 = run_unx0463_checks();
        let s2 = run_unx0463_checks();
        assert_eq!(s1.all_passed(), s2.all_passed());
        assert!(s1.all_passed());
    }

    #[test]
    fn segment_guard_and_code_table() {
        assert!(segment_guard_ok());
        assert!(code_table_ok());
        assert_eq!(detail(0x999_9999), "未知诊断码：不在 0x754C0 段闭集内");
    }

    #[test]
    fn header_kind_dispatch() {
        let ep = DevCfg { vendor: 0x8086, device: 1, hdr_type: 0x00, secondary: None, subordinate: None };
        let br = DevCfg { vendor: 0x8086, device: 2, hdr_type: 0x81, secondary: Some(1), subordinate: None };
        assert_eq!(ep.kind(), HeaderKind::Endpoint);
        assert_eq!(br.kind(), HeaderKind::Bridge);
        let empty = DevCfg { vendor: 0xFFFF, device: 0xFFFF, hdr_type: 0x00, secondary: None, subordinate: None };
        assert_eq!(empty.kind(), HeaderKind::Empty);
    }
}
