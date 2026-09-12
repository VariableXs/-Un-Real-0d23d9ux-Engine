//! m700dev — VARIX-M700 AI-12 设备模型域 (F276~F300)
//!
//! 设备树大典/设备生命周期谱/热插拔事件总线/设备自描述清单/驱动匹配裁判/
//! 电源管理钩子/设备依赖图/设备错误舱/DMA 地址官/设备权限门/设备热移除律/
//! 设备档案导出/资源分配官/中断路由谱/设备健康分/幽灵设备纠察/设备回放舱/
//! 设备性能仪/设备能耗账/设备命名宪法/设备组策略/设备自检入口/
//! 设备拓扑可视化数据源/设备回归走廊/设备域年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F276 — 设备树大典：父/子索引树
// ===========================================================================

pub const MAX_TREE_NODES: usize = 16;
/// 0 号保留为根。
pub const TREE_ROOT: usize = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevNode {
    pub parent: usize, // MAX_TREE_NODES 表示无父（仅根）
    pub depth: u8,
}

pub const NO_PARENT: usize = MAX_TREE_NODES;

/// 挂接子节点：父必须有效且树深不超过 8 层。
pub fn tree_attach(nodes: &mut [DevNode], used: usize, parent: usize) -> Option<usize> {
    if used >= nodes.len() || parent >= used {
        return None;
    }
    let depth = nodes[parent].depth + 1;
    if depth > 8 {
        return None;
    }
    nodes[used] = DevNode { parent, depth };
    Some(used)
}

/// 求祖先链（不含自身），写入 out，返回写入数。
pub fn tree_ancestors(nodes: &[DevNode], mut node: usize, out: &mut [usize]) -> usize {
    let mut n = 0usize;
    while node < nodes.len() && nodes[node].parent != NO_PARENT && n < out.len() {
        node = nodes[node].parent;
        out[n] = node;
        n += 1;
    }
    n
}

// ===========================================================================
// F277 — 设备生命周期谱：状态机迁移律
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevLifecycle {
    Unprobed,
    Bound,
    Suspended,
    Removed,
}

/// 合法迁移：Unprobed→Bound→Suspended→Bound / Removed。
pub fn lifecycle_transition_ok(from: DevLifecycle, to: DevLifecycle) -> bool {
    use DevLifecycle::*;
    matches!(
        (from, to),
        (Unprobed, Bound) | (Bound, Suspended) | (Suspended, Bound) | (Bound, Removed) | (Suspended, Removed)
    )
}

// ===========================================================================
// F278 — 热插拔事件总线：定容事件环 + 订阅掩码
// ===========================================================================

pub const HOTPLUG_RING: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotplugKind {
    Plug,
    Unplug,
}

#[derive(Clone, Copy, Debug)]
pub struct HotplugBus {
    kinds: [HotplugKind; HOTPLUG_RING],
    ids: [u32; HOTPLUG_RING],
    head: usize,
    pub dropped: u32,
    pub subscribers: u32, // 位掩码
}

impl HotplugBus {
    pub const fn new() -> HotplugBus {
        HotplugBus {
            kinds: [HotplugKind::Plug; HOTPLUG_RING],
            ids: [0; HOTPLUG_RING],
            head: 0,
            dropped: 0,
            subscribers: 0,
        }
    }

    pub fn subscribe(&mut self, listener_bit: u8) {
        self.subscribers |= 1u32 << listener_bit;
    }

    pub fn publish(&mut self, id: u32, kind: HotplugKind) {
        self.kinds[self.head] = kind;
        self.ids[self.head] = id;
        self.head = (self.head + 1) % HOTPLUG_RING;
    }

    pub fn kind_at(&self, i: usize) -> HotplugKind {
        self.kinds[i % HOTPLUG_RING]
    }

    pub fn id_at(&self, i: usize) -> u32 {
        self.ids[i % HOTPLUG_RING]
    }
}

// ===========================================================================
// F279 — 设备自描述清单：厂商/型号/类/修订
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceDescriptor {
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: u8,
    pub revision: u8,
}

impl DeviceDescriptor {
    pub fn self_describing(&self) -> bool {
        self.vendor_id != 0 && self.device_id != 0 && self.revision > 0
    }

    /// PCI 风格 32 位身份字。
    pub fn identity_word(&self) -> u32 {
        (self.vendor_id as u32) | ((self.device_id as u32) << 16)
    }
}

// ===========================================================================
// F280 — 驱动匹配裁判：得分制，分高者胜
// ===========================================================================

/// 匹配得分：精确 ID 命中 800，类匹配 400，通配 100；不认识 0。
pub fn match_score(desc: DeviceDescriptor, drv_vendor: u16, drv_device: u16, drv_class: Option<u8>) -> u32 {
    if desc.vendor_id == drv_vendor && desc.device_id == drv_device {
        800
    } else if let Some(c) = drv_class {
        if c == desc.class {
            400
        } else {
            0
        }
    } else {
        100
    }
}

/// 在候选驱动表中挑出得分最高的（全 0 返回 None）。
pub fn best_match(desc: DeviceDescriptor, candidates: &[(u16, u16, Option<u8>)]) -> Option<usize> {
    let mut best: Option<(usize, u32)> = None;
    for (i, c) in candidates.iter().enumerate() {
        let s = match_score(desc, c.0, c.1, c.2);
        if s > 0 {
            match best {
                Some((_, bs)) if bs >= s => {}
                _ => best = Some((i, s)),
            }
        }
    }
    best.map(|(i, _)| i)
}

// ===========================================================================
// F281 — 电源管理钩子：D 状态迁移
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevicePower {
    D0, // 全速
    D1,
    D2,
    D3, // 断电
}

/// D 状态只能逐级上下（D0↔D1↔D2↔D3）。
pub fn power_transition_ok(from: DevicePower, to: DevicePower) -> bool {
    let a = from as u8;
    let b = to as u8;
    if a > b {
        a - b == 1
    } else {
        b - a == 1
    }
}

/// D3 恢复到 D0 的预算（ms）：逐级唤醒每级 10ms。
pub fn wake_latency_ms(from: DevicePower) -> u32 {
    (from as u8) as u32 * 10
}

// ===========================================================================
// F282 — 设备依赖图：无环校验
// ===========================================================================

pub const MAX_DEP_NODES: usize = 6;

/// 邻接矩阵 + 染色法查环：返回是否存在环。
pub fn dep_graph_has_cycle(adj: &[[bool; MAX_DEP_NODES]; MAX_DEP_NODES], n: usize) -> bool {
    // 0=未访问 1=在栈 2=完成
    let mut color = [0u8; MAX_DEP_NODES];
    let mut has_cycle = false;
    // 迭代式 DFS（显式栈：节点 + 邻居游标）
    let mut stack = [0usize; MAX_DEP_NODES];
    let mut cursor = [0usize; MAX_DEP_NODES];
    for start in 0..n {
        if color[start] != 0 {
            continue;
        }
        let mut sp = 0usize;
        stack[0] = start;
        cursor[0] = 0;
        color[start] = 1;
        while sp < MAX_DEP_NODES {
            let u = stack[sp];
            let mut advanced = false;
            while cursor[sp] < n {
                let v = cursor[sp];
                cursor[sp] += 1;
                if adj[u][v] {
                    if color[v] == 1 {
                        has_cycle = true;
                    } else if color[v] == 0 {
                        color[v] = 1;
                        sp += 1;
                        stack[sp] = v;
                        cursor[sp] = 0;
                        advanced = true;
                        break;
                    }
                }
            }
            if !advanced {
                color[u] = 2;
                if sp == 0 {
                    break;
                }
                sp -= 1;
            }
        }
    }
    has_cycle
}

// ===========================================================================
// F283 — 设备错误舱：故障只波及依赖方
// ===========================================================================

/// 设备 fault 后：直接依赖它的设备被隔离，其余不受影响。
pub fn blast_radius(faulted: usize, adj: &[[bool; MAX_DEP_NODES]; MAX_DEP_NODES], n: usize) -> usize {
    let mut quarantined = 0usize;
    for v in 0..n {
        if v != faulted && adj[v][faulted] {
            quarantined += 1;
        }
    }
    quarantined
}

// ===========================================================================
// F284 — DMA 地址官：对齐分配与翻译
// ===========================================================================

pub const DMA_ALIGN_BYTES: u32 = 64;
pub const DMA_POOL_SIZE: u32 = 4096;

/// DMA 缓冲申请：长度必须对齐，范围不得越过池边界。
pub fn dma_alloc(base: u32, len: u32) -> Option<(u32, u32)> {
    if len == 0 || base % DMA_ALIGN_BYTES != 0 || len % DMA_ALIGN_BYTES != 0 {
        return None;
    }
    if base.checked_add(len)? > DMA_POOL_SIZE {
        return None;
    }
    Some((base, len))
}

/// 设备视角地址 → 主存物理地址（窗口偏移翻译）。
pub fn dma_translate(device_addr: u32, window_base: u32) -> Option<u32> {
    device_addr.checked_add(window_base)
}

// ===========================================================================
// F285 — 设备权限门：类别决定可用操作位
// ===========================================================================

pub const PERM_READ: u32 = 1;
pub const PERM_WRITE: u32 = 2;
pub const PERM_RESET: u32 = 4;
pub const PERM_FLASH: u32 = 8;

/// 类别 0=存储 1=网络 2=输入 3=显示。
pub fn device_perms(class: u8) -> u32 {
    match class {
        0 => PERM_READ | PERM_WRITE | PERM_RESET,
        1 => PERM_READ | PERM_WRITE,
        2 => PERM_READ,
        _ => PERM_READ | PERM_RESET,
    }
}

pub fn perm_granted(perms: u32, want: u32) -> bool {
    perms & want == want
}

// ===========================================================================
// F286 — 设备热移除律：先静默依赖方再拔自己
// ===========================================================================

/// 热移除次序：返回需要先静默的依赖方数量；有在途 IO 的依赖方阻塞移除。
pub fn removable(
    target: usize,
    adj: &[[bool; MAX_DEP_NODES]; MAX_DEP_NODES],
    inflight_io: &[bool; MAX_DEP_NODES],
    n: usize,
) -> bool {
    for v in 0..n {
        if v != target && adj[v][target] && inflight_io[v] {
            return false;
        }
    }
    true
}

// ===========================================================================
// F287 — 设备档案导出：16 字节定长档案
// ===========================================================================

/// 档案布局：[0..2]=vendor [2..4]=device [4]=class [5]=revision [6..10]=irq [10..16]=预留 0。
pub fn export_profile(desc: DeviceDescriptor, irq: u32, out: &mut [u8; 16]) {
    out[0] = (desc.vendor_id & 0xFF) as u8;
    out[1] = (desc.vendor_id >> 8) as u8;
    out[2] = (desc.device_id & 0xFF) as u8;
    out[3] = (desc.device_id >> 8) as u8;
    out[4] = desc.class;
    out[5] = desc.revision;
    out[6] = (irq & 0xFF) as u8;
    out[7] = ((irq >> 8) & 0xFF) as u8;
    out[8] = ((irq >> 16) & 0xFF) as u8;
    out[9] = ((irq >> 24) & 0xFF) as u8;
    let mut i = 10;
    while i < 16 {
        out[i] = 0;
        i += 1;
    }
}

pub fn import_profile(buf: &[u8; 16]) -> (DeviceDescriptor, u32) {
    let desc = DeviceDescriptor {
        vendor_id: buf[0] as u16 | ((buf[1] as u16) << 8),
        device_id: buf[2] as u16 | ((buf[3] as u16) << 8),
        class: buf[4],
        revision: buf[5],
    };
    let irq = buf[6] as u32
        | ((buf[7] as u32) << 8)
        | ((buf[8] as u32) << 16)
        | ((buf[9] as u32) << 24);
    (desc, irq)
}

// ===========================================================================
// F288 — 资源分配官：MMIO 区间不重叠
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MmioRange {
    pub base: u32,
    pub len: u32,
}

impl MmioRange {
    pub fn end(&self) -> u32 {
        self.base + self.len
    }

    pub fn overlaps(&self, other: &MmioRange) -> bool {
        self.base < other.end() && other.base < self.end()
    }

    pub fn sane(&self) -> bool {
        self.len > 0 && self.end() > self.base
    }
}

/// 分配官：新区间必须不与任何已分配区间重叠且自身合法。
pub fn mmio_allocate(allocated: &[MmioRange], want: MmioRange) -> bool {
    want.sane() && allocated.iter().all(|r| !r.overlaps(&want))
}

// ===========================================================================
// F289 — 中断路由谱：独占线 vs 共享线
// ===========================================================================

pub const MAX_IRQ_LINES: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct IrqMap {
    pub line_of: [u8; MAX_DEP_NODES], // 设备 → 中断线
    pub shared: [u32; MAX_IRQ_LINES], // 每条线上的设备数
}

impl IrqMap {
    pub const fn new() -> IrqMap {
        IrqMap { line_of: [0xFF; MAX_DEP_NODES], shared: [0; MAX_IRQ_LINES] }
    }

    pub fn route(&mut self, dev: usize, line: u8) -> bool {
        if dev >= MAX_DEP_NODES || line as usize >= MAX_IRQ_LINES {
            return false;
        }
        if self.line_of[dev] != 0xFF {
            let old = self.line_of[dev] as usize;
            self.shared[old] -= 1;
        }
        self.line_of[dev] = line;
        self.shared[line as usize] += 1;
        true
    }

    pub fn line_shared(&self, line: u8) -> bool {
        (line as usize) < MAX_IRQ_LINES && self.shared[line as usize] > 1
    }

    pub fn line_of_device(&self, dev: usize) -> Option<u8> {
        if dev < MAX_DEP_NODES && self.line_of[dev] != 0xFF {
            Some(self.line_of[dev])
        } else {
            None
        }
    }
}

// ===========================================================================
// F290 — 设备健康分：错误率与延迟合成
// ===========================================================================

/// 健康分 permille = 1000 - 错误率‰×5 - 超时率‰×3，下限 0。
pub fn health_score(ops: u64, errors: u64, timeouts: u64) -> u32 {
    if ops == 0 {
        return 1000;
    }
    let err_pm = (errors * 1000 / ops) as u32;
    let to_pm = (timeouts * 1000 / ops) as u32;
    1000u32.saturating_sub(err_pm.saturating_mul(5)).saturating_sub(to_pm.saturating_mul(3))
}

// ===========================================================================
// F291 — 幽灵设备纠察：树上挂着但总线扫不到
// ===========================================================================

/// 幽灵 = 在设备树中登记却不在总线扫描名单里。
pub fn ghost_devices(tree_ids: &[u32], bus_ids: &[u32]) -> usize {
    tree_ids
        .iter()
        .filter(|t| !bus_ids.iter().any(|&b| b == **t))
        .count()
}

// ===========================================================================
// F292 — 设备回放舱：操作日志确定性回放
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevOp {
    Probe(u32),
    Reset(u32),
    Io(u32),
}

/// 回放一致性：两条日志逐条相同且长度一致。
pub fn replay_matches(a: &[DevOp], b: &[DevOp]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x == y)
}

// ===========================================================================
// F293 — 设备性能仪：吞吐与延迟预算
// ===========================================================================

pub const DEV_LATENCY_BUDGET_US: u32 = 1000;

#[derive(Clone, Copy, Debug, Default)]
pub struct DevPerf {
    pub ops: u64,
    pub total_latency_us: u64,
}

impl DevPerf {
    pub fn avg_latency_us(&self) -> u64 {
        if self.ops == 0 {
            0
        } else {
            self.total_latency_us / self.ops
        }
    }

    pub fn within_budget(&self) -> bool {
        self.avg_latency_us() <= DEV_LATENCY_BUDGET_US as u64
    }

    /// 吞吐 ops/s（毫秒采样 ×1000）。
    pub fn throughput_per_s(&self, sample_ms: u64) -> u64 {
        if sample_ms == 0 {
            0
        } else {
            self.ops * 1000 / sample_ms
        }
    }
}

// ===========================================================================
// F294 — 设备能耗账：毫瓦 × 毫秒 = 微焦
// ===========================================================================

#[derive(Clone, Copy, Debug, Default)]
pub struct EnergyBook {
    pub microjoules: u64,
}

impl EnergyBook {
    /// 消耗 = 功率 mw × 时长 ms（µJ = mw·ms）。
    pub fn burn(&mut self, power_mw: u32, duration_ms: u32) {
        self.microjoules += power_mw as u64 * duration_ms as u64;
    }

    /// 换算 permille 预算占比。
    pub fn budget_permille(&self, budget_uj: u64) -> u32 {
        if budget_uj == 0 {
            return 0;
        }
        ((self.microjoules.min(u32::MAX as u64) * 1000) / budget_uj) as u32
    }
}

// ===========================================================================
// F295 — 设备命名宪法：bus:addr.fn 格式
// ===========================================================================

/// 合法命名：`bus:addr.fn`，bus 为字母数字，addr/fn 为 1~2 位十六进制。
pub fn device_name_ok(name: &str) -> bool {
    let b = name.as_bytes();
    let sep1 = b.iter().position(|&c| c == b':');
    let sep2 = b.iter().rposition(|&c| c == b'.');
    let (s1, s2) = match (sep1, sep2) {
        (Some(a), Some(c)) if a > 0 && c > a + 1 && c < b.len() - 1 => (a, c),
        _ => return false,
    };
    let bus = &b[..s1];
    let addr = &b[s1 + 1..s2];
    let func = &b[s2 + 1..];
    if bus.is_empty() || bus.len() > 8 {
        return false;
    }
    let alnum = |sl: &[u8]| sl.iter().all(|&c| c.is_ascii_alphanumeric());
    let hex12 = |sl: &[u8]| !sl.is_empty() && sl.len() <= 2 && sl.iter().all(|&c| c.is_ascii_hexdigit());
    alnum(bus) && hex12(addr) && hex12(func)
}

// ===========================================================================
// F296 — 设备组策略：同组设备共享策略位
// ===========================================================================

pub const GROUP_POLICY_MAX: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct DeviceGroup {
    pub policy: u32,
    pub members: [bool; GROUP_POLICY_MAX],
}

impl DeviceGroup {
    pub const fn new(policy: u32) -> DeviceGroup {
        DeviceGroup { policy, members: [false; GROUP_POLICY_MAX] }
    }

    pub fn join(&mut self, dev: usize) -> bool {
        if dev >= GROUP_POLICY_MAX || self.members[dev] {
            return false;
        }
        self.members[dev] = true;
        true
    }

    pub fn member_count(&self) -> u32 {
        self.members.iter().filter(|m| **m).count() as u32
    }

    /// 组策略生效要求：至少 2 名成员。
    pub fn policy_active(&self) -> bool {
        self.member_count() >= 2
    }
}

// ===========================================================================
// F297 — 设备自检入口：自检码约定
// ===========================================================================

pub const SELFTEST_PASS: u8 = 0;

/// 自检结果码：0 通过；非零按位编码失败子项。
pub fn selftest_passed(code: u8) -> bool {
    code == SELFTEST_PASS
}

/// 连续自检：所有设备码为 0 才算域通过。
pub fn selftest_all(codes: &[u8]) -> bool {
    codes.iter().all(|&c| c == SELFTEST_PASS)
}

// ===========================================================================
// F298 — 设备拓扑可视化数据源：边表导出
// ===========================================================================

/// 从树导出边表 (parent, child)，写入 out，返回边数。
pub fn topology_edges(nodes: &[DevNode], used: usize, out: &mut [(usize, usize)]) -> usize {
    let mut n = 0usize;
    for child in 0..used.min(nodes.len()) {
        let p = nodes[child].parent;
        if p != NO_PARENT && n < out.len() {
            out[n] = (p, child);
            n += 1;
        }
    }
    n
}

// ===========================================================================
// F299 — 设备回归走廊：场景清单全绿
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct DevScenario {
    pub name: &'static str,
    pub passed: bool,
}

pub fn regression_green(scenarios: &[DevScenario]) -> bool {
    !scenarios.is_empty() && scenarios.iter().all(|s| s.passed && !s.name.is_empty())
}

// ===========================================================================
// F300 — 设备域年报
// ===========================================================================

pub const DEV_REPORT_SECTIONS: [&str; 5] = ["tree", "hotplug", "dma", "power", "health"];

pub fn dev_report_complete(filled: u32) -> bool {
    filled >= DEV_REPORT_SECTIONS.len() as u32
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m700dev_checks() -> CheckSet {
    let mut set = CheckSet::new("m700dev");

    // F276 设备树大典
    let mut tree = [DevNode { parent: NO_PARENT, depth: 0 }; MAX_TREE_NODES];
    tree[0] = DevNode { parent: NO_PARENT, depth: 0 };
    let c1 = tree_attach(&mut tree, 1, TREE_ROOT);
    let c2 = tree_attach(&mut tree, 2, 1);
    set.add("F276 tree attach", c1 == Some(1) && c2 == Some(2) && tree[2].depth == 2, "depth grows");
    let mut anc = [0usize; 8];
    let na = tree_ancestors(&tree, 2, &mut anc);
    set.add("F276 ancestors", na == 2 && anc[0] == 1 && anc[1] == 0, "root chain");
    set.add("F276 attach rejects", tree_attach(&mut tree, 3, 9).is_none(), "unknown parent");

    // F277 设备生命周期谱
    set.add(
        "F277 lifecycle forward",
        lifecycle_transition_ok(DevLifecycle::Unprobed, DevLifecycle::Bound)
            && lifecycle_transition_ok(DevLifecycle::Bound, DevLifecycle::Suspended)
            && lifecycle_transition_ok(DevLifecycle::Bound, DevLifecycle::Removed),
        "legal path",
    );
    set.add(
        "F277 lifecycle illegal",
        !lifecycle_transition_ok(DevLifecycle::Unprobed, DevLifecycle::Suspended)
            && !lifecycle_transition_ok(DevLifecycle::Removed, DevLifecycle::Bound),
        "no resurrection",
    );

    // F278 热插拔事件总线
    let mut bus = HotplugBus::new();
    bus.subscribe(0);
    bus.subscribe(3);
    bus.publish(11, HotplugKind::Plug);
    bus.publish(12, HotplugKind::Unplug);
    set.add("F278 bus delivery", bus.id_at(0) == 11 && bus.kind_at(1) == HotplugKind::Unplug, "in order");
    set.add("F278 bus subscribers", bus.subscribers & 0b1001 == 0b1001, "two listeners");

    // F279 设备自描述清单
    let desc = DeviceDescriptor { vendor_id: 0x10DE, device_id: 0x2504, class: 3, revision: 2 };
    set.add(
        "F279 self describing",
        desc.self_describing()
            && !DeviceDescriptor { vendor_id: 0, device_id: 1, class: 1, revision: 1 }.self_describing(),
        "ids+rev required",
    );
    set.add("F279 identity word", desc.identity_word() == 0x2504_10DE, "packed word");

    // F280 驱动匹配裁判
    set.add(
        "F280 match scores",
        match_score(desc, 0x10DE, 0x2504, None) == 800
            && match_score(desc, 0xFFFF, 0xFFFF, Some(3)) == 400
            && match_score(desc, 0xFFFF, 0xFFFF, None) == 100,
        "exact > class > wildcard",
    );
    let candidates = [(0xFFFFu16, 0xFFFFu16, Some(3u8)), (0x10DEu16, 0x2504u16, None)];
    set.add("F280 best match wins", best_match(desc, &candidates) == Some(1), "highest score");

    // F281 电源管理钩子
    set.add(
        "F281 power ladder",
        power_transition_ok(DevicePower::D0, DevicePower::D1)
            && power_transition_ok(DevicePower::D2, DevicePower::D3)
            && !power_transition_ok(DevicePower::D0, DevicePower::D3),
        "one step at a time",
    );
    set.add("F281 wake latency", wake_latency_ms(DevicePower::D3) == 30 && wake_latency_ms(DevicePower::D0) == 0, "10ms/step");

    // F282 设备依赖图
    let mut adj = [[false; MAX_DEP_NODES]; MAX_DEP_NODES];
    adj[0][1] = true;
    adj[1][2] = true;
    set.add("F282 dag accepted", !dep_graph_has_cycle(&adj, 3), "chain is acyclic");
    adj[2][0] = true;
    set.add("F282 cycle caught", dep_graph_has_cycle(&adj, 3), "0→1→2→0");

    // F283 设备错误舱
    let mut blast = [[false; MAX_DEP_NODES]; MAX_DEP_NODES];
    blast[0][1] = true; // 0 依赖 1
    blast[2][1] = true; // 2 依赖 1
    blast[3][0] = true; // 3 依赖 0
    set.add("F283 blast radius", blast_radius(1, &blast, 4) == 2, "only dependents");
    set.add("F283 no collateral", blast_radius(3, &blast, 4) == 0, "leaf faults isolated");

    // F284 DMA 地址官
    set.add(
        "F284 dma alloc rules",
        dma_alloc(0, 128).is_some()
            && dma_alloc(32, 64).is_none() // 未对齐基址
            && dma_alloc(0, 33).is_none(), // 长度未对齐
        "alignment enforced",
    );
    set.add("F284 dma overflow", dma_alloc(4032, 128).is_none() && dma_alloc(4032, 64).is_some(), "pool bounds");
    set.add("F284 dma translate", dma_translate(100, 0x1000) == Some(0x1064), "window offset");

    // F285 设备权限门
    let storage = device_perms(0);
    let input = device_perms(2);
    set.add(
        "F285 perms by class",
        perm_granted(storage, PERM_WRITE | PERM_RESET) && !perm_granted(input, PERM_WRITE),
        "input is read-only",
    );
    set.add(
        "F285 flash gated",
        !perm_granted(device_perms(0), PERM_FLASH) && !perm_granted(input, PERM_FLASH),
        "flash needs grant",
    );

    // F286 设备热移除律
    let mut inflight = [false; MAX_DEP_NODES];
    inflight[0] = true;
    set.add("F286 removal blocked", !removable(1, &blast, &inflight, 4), "dependent busy");
    let idle = [false; MAX_DEP_NODES];
    set.add("F286 removal ok", removable(1, &blast, &idle, 4), "quiesced first");

    // F287 设备档案导出
    let mut buf = [0u8; 16];
    export_profile(desc, 0x1234, &mut buf);
    let (back, irq_back) = import_profile(&buf);
    set.add("F287 profile roundtrip", back == desc && irq_back == 0x1234, "encode/decode");
    set.add("F287 reserved zero", buf[10] == 0 && buf[15] == 0, "layout stable");

    // F288 资源分配官
    let allocated = [MmioRange { base: 0xF000, len: 0x100 }, MmioRange { base: 0xE000, len: 0x80 }];
    set.add(
        "F288 alloc ok",
        mmio_allocate(&allocated, MmioRange { base: 0xF200, len: 0x40 }),
        "free hole",
    );
    set.add(
        "F288 alloc overlap",
        !mmio_allocate(&allocated, MmioRange { base: 0xF080, len: 0x40 })
            && !mmio_allocate(&allocated, MmioRange { base: 0x10, len: 0 }),
        "overlap & insane",
    );

    // F289 中断路由谱
    let mut irq = IrqMap::new();
    irq.route(0, 3);
    irq.route(1, 3);
    irq.route(2, 5);
    set.add(
        "F289 irq routing",
        irq.line_of_device(0) == Some(3) && irq.line_of_device(2) == Some(5),
        "routed",
    );
    set.add(
        "F289 irq sharing",
        irq.line_shared(3) && !irq.line_shared(5),
        "shared detected",
    );
    let rerouted_ok = irq.route(0, 6);
    let line3_shared = irq.line_shared(3);
    set.add("F289 irq reroute", rerouted_ok && !line3_shared, "move frees line");

    // F290 设备健康分
    set.add("F290 healthy device", health_score(1000, 0, 0) == 1000, "flawless");
    set.add(
        "F290 degradation",
        health_score(1000, 10, 0) == 950 && health_score(1000, 10, 20) == 890,
        "weighted demerits",
    );

    // F291 幽灵设备纠察
    let tree_ids = [1, 2, 3, 4];
    let bus_ids = [1, 3, 4, 9];
    set.add("F291 ghost count", ghost_devices(&tree_ids, &bus_ids) == 1, "one phantom");
    set.add(
        "F291 no ghost",
        ghost_devices(&[5, 6], &[5, 6, 7]) == 0,
        "clean tree",
    );

    // F292 设备回放舱
    let run1 = [DevOp::Probe(1), DevOp::Reset(1), DevOp::Io(64)];
    let run2 = [DevOp::Probe(1), DevOp::Reset(1), DevOp::Io(64)];
    let run3 = [DevOp::Probe(1), DevOp::Io(64), DevOp::Reset(1)];
    set.add("F292 replay deterministic", replay_matches(&run1, &run2), "identical");
    set.add("F292 replay divergence", !replay_matches(&run1, &run3), "order matters");

    // F293 设备性能仪
    let perf = DevPerf { ops: 100, total_latency_us: 50_000 };
    let slow = DevPerf { ops: 10, total_latency_us: 20_000 };
    set.add("F293 perf budget", perf.within_budget() && !slow.within_budget(), "500us vs 2000us");
    set.add("F293 throughput", perf.throughput_per_s(500) == 200, "ops/s math");

    // F294 设备能耗账
    let mut e = EnergyBook::default();
    e.burn(500, 1000); // 500mW × 1s = 500_000 µJ
    let after_burn = e.microjoules;
    e.burn(100, 100);
    set.add("F294 energy burn", after_burn == 500_000 && e.microjoules == 510_000, "mw*ms=µJ");
    set.add("F294 energy budget", e.budget_permille(1_000_000) == 510, "51% of 1J");

    // F295 设备命名宪法
    set.add(
        "F295 name valid",
        device_name_ok("pci:00.1f") && device_name_ok("usb:a3.2"),
        "bus:addr.fn",
    );
    set.add(
        "F295 name invalid",
        !device_name_ok("pci001f") && !device_name_ok("pci:zz.1") && !device_name_ok(":00.1"),
        "format enforced",
    );

    // F296 设备组策略
    let mut g = DeviceGroup::new(0b110);
    let joined1 = g.join(0);
    let count1 = g.member_count();
    g.join(2);
    set.add("F296 group join", joined1 && count1 == 1 && g.member_count() == 2, "membership");
    set.add("F296 group active", g.policy_active() && !DeviceGroup::new(1).policy_active(), "two to tango");
    set.add("F296 no rejoin", !g.join(0), "idempotent");

    // F297 设备自检入口
    set.add(
        "F297 selftest pass",
        selftest_passed(SELFTEST_PASS) && !selftest_passed(0b101),
        "zero is pass",
    );
    set.add(
        "F297 selftest all",
        selftest_all(&[0, 0, 0]) && !selftest_all(&[0, 1, 0]),
        "fleet check",
    );

    // F298 设备拓扑可视化数据源
    let mut edges = [(0usize, 0usize); 8];
    let ne = topology_edges(&tree, 3, &mut edges);
    set.add("F298 edge export", ne == 2 && edges[0] == (0, 1) && edges[1] == (1, 2), "parent→child");

    // F299 设备回归走廊
    let green = [
        DevScenario { name: "plug-storm", passed: true },
        DevScenario { name: "irq-reroute", passed: true },
    ];
    let red = [DevScenario { name: "dma-panic", passed: false }];
    set.add("F299 regression green", regression_green(&green), "all pass");
    set.add(
        "F299 regression red",
        !regression_green(&red) && !regression_green(&[]),
        "fail & empty rejected",
    );

    // F300 设备域年报
    set.add(
        "F300 dev report",
        DEV_REPORT_SECTIONS.len() == 5 && dev_report_complete(5) && !dev_report_complete(3),
        "sections complete",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f276_tree_depths() {
        let mut tree = [DevNode { parent: NO_PARENT, depth: 0 }; MAX_TREE_NODES];
        assert_eq!(tree_attach(&mut tree, 1, TREE_ROOT), Some(1));
        assert_eq!(tree_attach(&mut tree, 2, 1), Some(2));
        assert_eq!(tree_attach(&mut tree, 3, 2), Some(3));
        assert_eq!(tree[3].depth, 3);
        assert_eq!(tree_attach(&mut tree, 4, 4), None); // 自引用非法
    }

    #[test]
    fn f282_cycle_detection() {
        let mut adj = [[false; MAX_DEP_NODES]; MAX_DEP_NODES];
        assert!(!dep_graph_has_cycle(&adj, 2));
        adj[1][0] = true;
        adj[0][1] = true;
        assert!(dep_graph_has_cycle(&adj, 2));
    }

    #[test]
    fn f284_dma_alignment() {
        assert!(dma_alloc(64, 64).is_some());
        assert!(dma_alloc(65, 64).is_none());
        assert!(dma_translate(u32::MAX, 1).is_none()); // 溢出拒绝
    }

    #[test]
    fn f287_profile_roundtrip() {
        let d = DeviceDescriptor { vendor_id: 0x8086, device_id: 0x1237, class: 6, revision: 3 };
        let mut buf = [0xFFu8; 16];
        export_profile(d, 11, &mut buf);
        let (back, irq) = import_profile(&buf);
        assert_eq!(back, d);
        assert_eq!(irq, 11);
    }

    #[test]
    fn f295_name_grammar() {
        assert!(device_name_ok("pci:0.1"));
        assert!(!device_name_ok("pci:001.1")); // addr 最多 2 位
        assert!(!device_name_ok("pci:00."));
    }

    #[test]
    fn f300_domain_selfcheck_all_pass() {
        let set = run_m700dev_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
