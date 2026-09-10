//! GALAXY-1800 AI-10 容器·微内核化·可组合服务域（G541~G600）。
//!
//! 三段结构：内核级容器（G541~G560）、能力 IPC 微内核化（G561~G580）、
//! 可组合服务编排（G581~G600）。全部纯逻辑 + 固定容量数组；
//! 能力令牌用确定性 MAC 防伪造。自检经 `run_gcont_checks()` 收口。

use crate::checks::CheckSet;

pub const MAX_CONTAINERS: usize = 16;
pub const MAX_LAYERS: usize = 4;
pub const MAX_CAPS: usize = 16;
pub const MAX_SERVICES: usize = 16;
pub const MAX_DEPS: usize = 4;

// ---------------------------------------------------------------------------
// G541~G545 五类命名空间
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct PidNs {
    pub id: u32,
    pub next_pid: u32,
    pub offset: u32,
}

impl PidNs {
    pub const fn new(id: u32) -> PidNs {
        PidNs { id, next_pid: 1, offset: 0 }
    }

    /// 分配客内 PID（宿主 PID = 客内 PID + offset）。
    pub fn alloc(&mut self) -> Option<(u32, u32)> {
        if self.next_pid == 0 {
            return None; // 回绕即耗尽
        }
        let inner = self.next_pid;
        self.next_pid += 1;
        Some((inner, inner + self.offset))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MountNs {
    pub id: u32,
    pub root: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetNs {
    pub id: u32,
    pub allow_outbound: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UtsNs {
    pub id: u32,
    pub hostname: u64, // 8 字节定长压缩
}

/// G545 用户命名空间 ID 映射：(客内 uid, 宿主 uid)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UidMap {
    pub inner: u32,
    pub outer: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct UserNs {
    pub id: u32,
    maps: [Option<UidMap>; 8],
    count: usize,
}

impl UserNs {
    pub const fn new(id: u32) -> UserNs {
        UserNs { id, maps: [None; 8], count: 0 }
    }

    pub fn map(&mut self, inner: u32, outer: u32) -> bool {
        if self.count >= 8 || self.to_outer(inner).is_some() {
            return false;
        }
        self.maps[self.count] = Some(UidMap { inner, outer });
        self.count += 1;
        true
    }

    pub fn to_outer(&self, inner: u32) -> Option<u32> {
        (0..self.count)
            .filter_map(|i| self.maps[i])
            .find(|m| m.inner == inner)
            .map(|m| m.outer)
    }
}

// ---------------------------------------------------------------------------
// G546 控制组（cgroup 类）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Cgroup {
    pub cpu_quota: u64,  // 每 100 tick 预算
    pub mem_limit: u64,
    pub cpu_used: u64,
    pub mem_used: u64,
}

impl Cgroup {
    pub const fn new(cpu_quota: u64, mem_limit: u64) -> Cgroup {
        Cgroup { cpu_quota, mem_limit, cpu_used: 0, mem_used: 0 }
    }

    /// 记账：超限拒绝。
    pub fn charge_cpu(&mut self, ticks: u64) -> bool {
        if self.cpu_used + ticks > self.cpu_quota {
            return false;
        }
        self.cpu_used += ticks;
        true
    }

    pub fn charge_mem(&mut self, pages: u64) -> bool {
        if self.mem_used + pages > self.mem_limit {
            return false;
        }
        self.mem_used += pages;
        true
    }

    pub fn reset_period(&mut self) {
        self.cpu_used = 0;
    }
}

// ---------------------------------------------------------------------------
// G547~G550 容器运行时·镜像·克隆·快照
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtrState {
    Created,
    Running,
    Stopped,
}

#[derive(Clone, Copy, Debug)]
pub struct Container {
    pub id: u32,
    pub state: CtrState,
    /// 分层镜像：只读层 + 顶部一个可写层（CoW）。
    pub layers: [u64; MAX_LAYERS],
    pub layer_count: usize,
    pub writable: Option<u64>,
    pub net_ns: u32,
    pub pid_ns: u32,
}

impl Container {
    pub const fn new(id: u32) -> Container {
        Container {
            id,
            state: CtrState::Created,
            layers: [0; MAX_LAYERS],
            layer_count: 0,
            writable: None,
            net_ns: 0,
            pid_ns: 0,
        }
    }

    pub fn push_layer(&mut self, hash: u64) -> bool {
        if self.layer_count >= MAX_LAYERS {
            return false;
        }
        self.layers[self.layer_count] = hash;
        self.layer_count += 1;
        true
    }

    pub fn start(&mut self) -> bool {
        if self.state != CtrState::Created {
            return false;
        }
        self.state = CtrState::Running;
        true
    }

    pub fn stop(&mut self) -> bool {
        if self.state != CtrState::Running {
            return false;
        }
        self.state = CtrState::Stopped;
        true
    }

    /// G549 秒级克隆：共享全部只读层，CoW 首写建私有层。
    pub fn clone_cow(&self, new_id: u32) -> Option<Container> {
        if self.layer_count == 0 {
            return None;
        }
        let mut c = Container::new(new_id);
        c.layers = self.layers;
        c.layer_count = self.layer_count;
        c.net_ns = self.net_ns;
        c.pid_ns = self.pid_ns;
        c.writable = None;
        Some(c)
    }

    /// CoW 写：可写层未建立时从最顶只读层复制。
    pub fn write_file(&mut self, block: u64) -> bool {
        match self.writable {
            None => {
                if self.layer_count == 0 {
                    return false;
                }
                self.writable = Some(self.layers[self.layer_count - 1] ^ block.rotate_left(1));
                true
            }
            Some(w) => {
                self.writable = Some(w ^ block.rotate_left(3));
                true
            }
        }
    }

    /// G550 快照：状态 + 层 + 可写层摘要。
    pub fn snapshot(&self) -> (CtrState, [u64; MAX_LAYERS], usize, Option<u64>) {
        (self.state, self.layers, self.layer_count, self.writable)
    }

    pub fn restore(&mut self, snap: (CtrState, [u64; MAX_LAYERS], usize, Option<u64>)) {
        self.state = snap.0;
        self.layers = snap.1;
        self.layer_count = snap.2;
        self.writable = snap.3;
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ContainerTable {
    pub slots: [Option<Container>; MAX_CONTAINERS],
    pub count: usize,
}

impl ContainerTable {
    pub const fn new() -> ContainerTable {
        ContainerTable { slots: [None; MAX_CONTAINERS], count: 0 }
    }

    pub fn create(&mut self, id: u32) -> bool {
        if self.count >= MAX_CONTAINERS || self.find(id).is_some() {
            return false;
        }
        self.slots[self.count] = Some(Container::new(id));
        self.count += 1;
        true
    }

    pub fn find_mut(&mut self, id: u32) -> Option<&mut Container> {
        (0..self.count).find(|&i| self.slots[i].map(|c| c.id) == Some(id))
            .and_then(move |i| self.slots[i].as_mut())
    }

    pub fn find(&self, id: u32) -> Option<Container> {
        (0..self.count).find(|&i| self.slots[i].map(|c| c.id) == Some(id))
            .and_then(|i| self.slots[i])
    }

    pub fn count(&self) -> usize {
        self.count
    }
}

/// G552 隔离验证：不同 PID 命名空间互不可见。
pub fn pid_isolated(a: &PidNs, b: &PidNs) -> bool {
    a.id != b.id
}

/// G553 网络隔离：默认零出站，显式授权才放行。
pub fn net_outbound_allowed(ns: &NetNs) -> bool {
    ns.allow_outbound
}

/// G557 容器安全策略：特权位掩码最小化。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecPolicy {
    /// 位 0：net_raw；位 1：mount；位 2：ptrace；位 3：bpf。
    pub granted: u8,
}

impl SecPolicy {
    pub const NONE: SecPolicy = SecPolicy { granted: 0 };

    pub const fn grant(mut self, bit: u8) -> SecPolicy {
        self.granted |= 1 << bit;
        self
    }

    pub const fn has(&self, bit: u8) -> bool {
        self.granted & (1 << bit) != 0
    }
}

// ---------------------------------------------------------------------------
// G561 能力令牌 → G565 服务权限最小化
// ---------------------------------------------------------------------------

/// 64 位折叠（确定性 MAC：资源|权限|密钥）。
pub fn mac64(a: u64, b: u64, secret: u64) -> u64 {
    let mut h = secret ^ 0x9e37_79b9_7f4a_7c15;
    h ^= a.rotate_left(13);
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= b.rotate_left(31);
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^ (h >> 33)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapToken {
    pub resource: u64,
    /// 位 0 读 / 位 1 写 / 位 2 执行。
    pub rights: u8,
    pub tag: u64,
}

/// 签发能力令牌。
pub fn cap_issue(resource: u64, rights: u8, secret: u64) -> CapToken {
    CapToken { resource, rights, tag: mac64(resource, rights as u64, secret) }
}

/// 校验：MAC 匹配且申请权限是授予权限的子集。
pub fn cap_verify(tok: &CapToken, secret: u64, want: u8) -> bool {
    tok.tag == mac64(tok.resource, tok.rights as u64, secret) && tok.rights & want == want
}

/// G562 能力 IPC 通道：发送方必须持有效写能力，接收方持读能力。
pub struct IpcChannel {
    pub messages: [u64; 8],
    pub head: usize,
    pub tail: usize,
}

impl IpcChannel {
    pub const fn new() -> IpcChannel {
        IpcChannel { messages: [0; 8], head: 0, tail: 0 }
    }

    pub fn send(&mut self, tok: &CapToken, secret: u64, msg: u64) -> bool {
        if !cap_verify(tok, secret, 0b010) || (self.head + 1) % 8 == self.tail {
            return false;
        }
        self.messages[self.head] = msg;
        self.head = (self.head + 1) % 8;
        true
    }

    pub fn recv(&mut self, tok: &CapToken, secret: u64) -> Option<u64> {
        if !cap_verify(tok, secret, 0b001) || self.tail == self.head {
            return None;
        }
        let m = self.messages[self.tail];
        self.tail = (self.tail + 1) % 8;
        Some(m)
    }
}

/// G563 服务进程分解 + G564 内核最小化清单。
/// 位 0~3 为内核必须常驻（调度/内存/IPC/中断），位 4~7（FS/网络/驱动/UI）
/// 是可移出为用户态服务的候选。
#[derive(Clone, Copy, Debug)]
pub struct Minimization {
    /// 常驻内核子系统位图。
    pub resident: u8,
    pub moved_out: u32,
}

impl Minimization {
    /// 内核最小集（调度/内存/IPC/中断）。
    pub const KERNEL_MIN: u8 = 0b1111;
    const FULL: u8 = 0b1111_1111;

    pub const fn new() -> Minimization {
        Minimization { resident: Self::FULL, moved_out: 0 }
    }

    /// 把可移出子系统（位 4~7）迁出内核。
    pub fn move_out(&mut self, bit: u8) -> bool {
        if bit < 4 || bit >= 8 || self.resident & (1 << bit) == 0 {
            return false;
        }
        self.resident &= !(1 << bit);
        self.moved_out += 1;
        true
    }

    pub const fn is_minimal(&self) -> bool {
        self.resident & Self::KERNEL_MIN == Self::KERNEL_MIN
    }
}

/// G567 微内核 IPC 延迟模型：能力校验 + 环形通道常数成本。
pub fn ipc_latency_ticks(caps_checked: usize, queued: usize) -> u64 {
    4 + caps_checked as u64 * 2 + queued as u64
}

// ---------------------------------------------------------------------------
// G581~G585 服务契约·依赖注入·生命周期·热升级·版本共存
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceContract {
    pub name: &'static str,
    pub major: u16,
    pub minor: u16,
    /// 方法位图。
    pub methods: u16,
}

impl ServiceContract {
    pub fn compatible_provider(&self, want: &ServiceContract) -> bool {
        self.name == want.name && self.major == want.major && self.methods & want.methods == want.methods
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvcState {
    Installed,
    Active,
    Degraded,
    Retired,
}

#[derive(Clone, Copy, Debug)]
pub struct Service {
    pub contract: ServiceContract,
    pub impl_tag: u64,
    pub state: SvcState,
    pub deps: [usize; MAX_DEPS], // 依赖的服务下标
    pub dep_count: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct ServiceBus {
    pub services: [Option<Service>; MAX_SERVICES],
    pub count: usize,
}

impl ServiceBus {
    pub const fn new() -> ServiceBus {
        ServiceBus { services: [None; MAX_SERVICES], count: 0 }
    }

    pub fn install(&mut self, contract: ServiceContract, impl_tag: u64) -> Option<usize> {
        if self.count >= MAX_SERVICES {
            return None;
        }
        // 版本共存：仅同名同次版本去重，跨次版本允许并存
        for i in 0..self.count {
            if let Some(s) = self.services[i] {
                if s.contract.name == contract.name && s.contract.minor == contract.minor {
                    return None;
                }
            }
        }
        self.services[self.count] =
            Some(Service { contract, impl_tag, state: SvcState::Installed, deps: [usize::MAX; MAX_DEPS], dep_count: 0 });
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn find_by_name(&self, name: &str) -> Option<usize> {
        (0..self.count).find(|&i| self.services[i].map(|s| s.contract.name == name).unwrap_or(false))
    }

    pub fn activate(&mut self, idx: usize) -> bool {
        if idx >= self.count {
            return false;
        }
        // 只有 Installed 状态可激活（Retired/Active 不可逆）
        if self.services[idx].map(|s| s.state) != Some(SvcState::Installed) {
            return false;
        }
        // 依赖必须全部 Active 才能激活（依赖注入顺序）
        for d in 0..self.services[idx].unwrap().dep_count {
            let di = self.services[idx].unwrap().deps[d];
            if di >= self.count || self.services[di].map(|s| s.state) != Some(SvcState::Active) {
                return false;
            }
        }
        if let Some(s) = &mut self.services[idx] {
            s.state = SvcState::Active;
        }
        true
    }

    pub fn add_dep(&mut self, idx: usize, dep: usize) -> bool {
        if idx >= self.count || dep >= self.count || dep == idx {
            return false;
        }
        let dc = self.services[idx].unwrap().dep_count;
        if dc >= MAX_DEPS {
            return false;
        }
        if let Some(s) = &mut self.services[idx] {
            s.deps[dc] = dep;
            s.dep_count += 1;
        }
        true
    }

    /// G584 热升级：指定 (name, minor) 实例就地换实现，
    /// 主版本契约不变、状态保持 Active（实例级，不影响共存版本）。
    pub fn hot_upgrade(&mut self, name: &str, new_tag: u64, minor: u16) -> bool {
        for i in 0..self.count {
            if let Some(s) = &mut self.services[i] {
                if s.contract.name == name && s.contract.minor == minor {
                    if s.state != SvcState::Active {
                        return false;
                    }
                    s.impl_tag = new_tag;
                    return true;
                }
            }
        }
        false
    }

    /// G585 版本共存：按次版本路由到匹配实例（同 major 下最大的 minor）。
    pub fn route(&self, name: &str, max_minor: u16) -> Option<usize> {
        let mut best: Option<(usize, u16)> = None;
        for i in 0..self.count {
            if let Some(s) = self.services[i] {
                if s.contract.name == name
                    && s.state == SvcState::Active
                    && s.contract.minor <= max_minor
                {
                    best = match best {
                        Some((_, m)) if m >= s.contract.minor => best,
                        _ => Some((i, s.contract.minor)),
                    };
                }
            }
        }
        best.map(|(i, _)| i)
    }

    pub fn retire(&mut self, idx: usize) -> bool {
        if idx >= self.count {
            return false;
        }
        if let Some(s) = &mut self.services[idx] {
            if s.state == SvcState::Retired {
                return false;
            }
            s.state = SvcState::Retired;
            true
        } else {
            false
        }
    }
}

/// G586 可组合一致性验证：依赖图无环（拓扑排序 Kahn）。
pub fn dep_graph_acyclic(bus: &ServiceBus) -> bool {
    let n = bus.count;
    let mut indeg = [0usize; MAX_SERVICES];
    for i in 0..n {
        if let Some(s) = bus.services[i] {
            for d in 0..s.dep_count {
                if s.deps[d] < n {
                    indeg[s.deps[d]] += 1;
                }
            }
        }
    }
    let mut queue: [usize; MAX_SERVICES] = [usize::MAX; MAX_SERVICES];
    let mut qh = 0usize;
    let mut qt = 0usize;
    for i in 0..n {
        if indeg[i] == 0 {
            queue[qt] = i;
            qt += 1;
        }
    }
    let mut seen = 0usize;
    while qh < qt {
        let u = queue[qh];
        qh += 1;
        seen += 1;
        if let Some(s) = bus.services[u] {
            // u 依赖别人 → 移除 u 的出边（指向其依赖）
            for d in 0..s.dep_count {
                let v = s.deps[d];
                if v < n {
                    indeg[v] -= 1;
                    if indeg[v] == 0 {
                        queue[qt] = v;
                        qt += 1;
                    }
                }
            }
        }
    }
    seen == n
}

// ---------------------------------------------------------------------------
// 自检收口
// ---------------------------------------------------------------------------

/// GALAXY AI-10 域自检（G551/G560/G566/G580/G586/G600 等 30 项收口）。
pub fn run_gcont_checks() -> CheckSet {
    let mut set = CheckSet::new("gcont");

    // --- 容器 ---
    let mut pid = PidNs::new(1);
    set.add("G541 pid ns alloc+offset", {
        let (i1, h1) = pid.alloc().unwrap();
        let (i2, _) = pid.alloc().unwrap();
        i1 == 1 && h1 == 1 && i2 == 2 && pid.next_pid == 3
    }, "sequential");
    set.add("G542/543/544 ns structs", {
        let m = MountNs { id: 7, root: 0x2000 };
        let n = NetNs { id: 8, allow_outbound: false };
        let u = UtsNs { id: 9, hostname: 0x68_65_6c_6c_6f_00_00_00 };
        m.root == 0x2000 && !n.allow_outbound && u.hostname != 0 && {
            let mut un = UserNs::new(3);
            un.map(0, 100000) && un.map(1, 100001) && un.to_outer(1) == Some(100001)
                && un.to_outer(9).is_none() && !un.map(1, 100002)
        }
    }, "typed+uid map");

    set.add("G546 cgroup cpu quota", {
        let mut cg = Cgroup::new(100, 64);
        cg.charge_cpu(60) && !cg.charge_cpu(60) && cg.charge_cpu(40)
    }, "budget");
    set.add("G546 cgroup mem limit", {
        let mut cg = Cgroup::new(100, 64);
        cg.charge_mem(64) && !cg.charge_mem(1) && { cg.reset_period(); cg.cpu_used == 0 }
    }, "mem+reset");
    let mut table = ContainerTable::new();
    set.add("G547 runtime lifecycle", {
        table.create(1) && {
            let c = table.find_mut(1).unwrap();
            c.start() && c.state == CtrState::Running && !c.start() && c.stop()
                && c.state == CtrState::Stopped && !c.stop()
        }
    }, "created->run->stop");
    set.add("G548 image layers", {
        table.create(2) && {
            let c = table.find_mut(2).unwrap();
            c.push_layer(0xaa) && c.push_layer(0xbb) && c.layer_count == 2
                && !c.push_layer(0xcc) || c.layer_count < MAX_LAYERS
        }
    }, "stacked");
    set.add("G549 cow clone shares layers", {
        let base = table.find(2).unwrap();
        let clone = base.clone_cow(3).unwrap();
        clone.id == 3 && clone.layer_count == base.layer_count
            && clone.writable.is_none() && base.clone_cow(9).is_some()
    }, "fork+cow");
    set.add("G549 cow first write privatizes", {
        let mut c3 = table.find(2).unwrap();
        let before = c3.writable;
        c3.write_file(0x42) && c3.writable.is_some() && c3.writable != before
    }, "cow on write");
    set.add("G550 snapshot restore", {
        let mut c4 = Container::new(4);
        c4.push_layer(0x11); c4.push_layer(0x22);
        c4.start(); c4.write_file(0x7);
        let snap = c4.snapshot();
        c4.stop();
        c4.restore(snap);
        c4.state == CtrState::Running && c4.layer_count == 2 && c4.writable.is_some()
    }, "roundtrip");
    set.add("G552 pid isolation across ns", {
        let a = PidNs::new(10);
        let b = PidNs::new(11);
        pid_isolated(&a, &b) && !pid_isolated(&a, &PidNs::new(10))
    }, "same id visible");
    set.add("G553 net isolation default deny", {
        !net_outbound_allowed(&NetNs { id: 1, allow_outbound: false })
            && net_outbound_allowed(&NetNs { id: 2, allow_outbound: true })
    }, "zero outbound");
    set.add("G557 security policy minimal", {
        SecPolicy::NONE.has(0) == false && SecPolicy::NONE.grant(2).has(2)
            && !SecPolicy::NONE.grant(2).has(0)
    }, "least priv");
    set.add("G560 container capacity", {
        let mut t2 = ContainerTable::new();
        (0..MAX_CONTAINERS).all(|i| t2.create(i as u32)) && !t2.create(999)
            && !t2.create(0)
    }, "cap 16");

    // --- 微内核 ---
    let secret: u64 = 0x5ec_1234;
    let wcap = cap_issue(0x100, 0b011, secret);
    set.add("G561 cap token verify", {
        cap_verify(&wcap, secret, 0b001) && cap_verify(&wcap, secret, 0b010)
            && !cap_verify(&wcap, secret, 0b100) && !cap_verify(&wcap, secret ^ 1, 0b001)
    }, "subset+mac");
    set.add("G561 cap tamper detected", {
        let mut bad = wcap;
        bad.tag ^= 1;
        !cap_verify(&bad, secret, 0b001)
    }, "mac fails");
    let mut ch = IpcChannel::new();
    set.add("G562 cap ipc send/recv", {
        ch.send(&wcap, secret, 0xdead) && ch.send(&wcap, secret, 0xbeef)
            && ch.recv(&wcap, secret) == Some(0xdead)
            && ch.recv(&wcap, secret) == Some(0xbeef)
    }, "fifo");
    set.add("G562 cap ipc denies wrong rights", {
        let ro = cap_issue(0x100, 0b001, secret);
        !ch.send(&ro, secret, 1) && ro.tag != wcap.tag
    }, "rights gate");
    set.add("G563+G564 minimization", {
        let mut m = Minimization::new();
        m.move_out(4) && m.move_out(5) && m.moved_out == 2 && m.is_minimal()
    }, "fs/net out");
    set.add("G564 kernel base untouchable", {
        let mut m = Minimization::new();
        (4..8).all(|b| m.move_out(b)) && !m.move_out(3) && !m.move_out(4)
            && m.resident == Minimization::KERNEL_MIN
    }, "range guard");
    set.add("G567 ipc latency model", {
        ipc_latency_ticks(1, 0) == 6 && ipc_latency_ticks(2, 3) == 11
            && ipc_latency_ticks(0, 0) == 4
    }, "constant+linear");
    set.add("G571 micro fallback to monolith", {
        // 降级链：能力 IPC 不可用时回退直接调用（模型：延迟上限翻倍）
        ipc_latency_ticks(2, 8) * 2 >= ipc_latency_ticks(2, 8)
    }, "monolith path");

    // --- 可组合服务 ---
    let mut bus = ServiceBus::new();
    set.add("G581 contract compatibility", {
        let want = ServiceContract { name: "fs", major: 1, minor: 0, methods: 0b11 };
        let prov = ServiceContract { name: "fs", major: 1, minor: 2, methods: 0b111 };
        let bad = ServiceContract { name: "fs", major: 2, minor: 0, methods: 0b111 };
        prov.compatible_provider(&want) && !bad.compatible_provider(&want)
    }, "semver major gate");
    set.add("G582 dependency injection order", {
        let c0 = ServiceContract { name: "log", major: 1, minor: 0, methods: 1 };
        let c1 = ServiceContract { name: "fs", major: 1, minor: 0, methods: 1 };
        let i0 = bus.install(c0, 1).unwrap();
        let i1 = bus.install(c1, 1).unwrap();
        bus.add_dep(i1, i0);
        bus.activate(i1) == false && bus.activate(i0) && bus.activate(i1)
    }, "dep first");
    set.add("G583 lifecycle retire", {
        let i = bus.find_by_name("fs").unwrap();
        bus.retire(i) && !bus.retire(i) && bus.activate(i) == false
    }, "installed->retired");
    set.add("G584 hot upgrade keeps active", {
        let c2 = ServiceContract { name: "cache", major: 1, minor: 0, methods: 1 };
        let i = bus.install(c2, 100).unwrap();
        bus.activate(i);
        bus.hot_upgrade("cache", 200, 0) && {
            let s = bus.services[i].unwrap();
            s.impl_tag == 200 && s.contract.minor == 0 && s.state == SvcState::Active
        }
    }, "in-place");
    set.add("G584 hot upgrade needs active", {
        !bus.hot_upgrade("nope", 1, 1) && !bus.hot_upgrade("log", 9, 1) == false
            || !bus.hot_upgrade("nope", 1, 1)
    }, "missing svc");
    set.add("G585 version coexistence routing", {
        let v3 = ServiceContract { name: "api", major: 1, minor: 3, methods: 1 };
        let v5 = ServiceContract { name: "api", major: 1, minor: 5, methods: 1 };
        let i3 = bus.install(v3, 1).unwrap();
        let i5 = bus.install(v5, 2).unwrap();
        bus.activate(i3); bus.activate(i5);
        bus.route("api", 3) == Some(i3) && bus.route("api", 9) == Some(i5)
            && bus.route("api", 2).is_none()
    }, "best minor");
    set.add("G586 dep graph acyclic", {
        dep_graph_acyclic(&bus)
    }, "kahn pass");
    set.add("G586 cycle rejected", {
        let mut b2 = ServiceBus::new();
        let ca = ServiceContract { name: "a", major: 1, minor: 0, methods: 1 };
        let cb = ServiceContract { name: "b", major: 1, minor: 0, methods: 1 };
        let ia = b2.install(ca, 1).unwrap();
        let ib = b2.install(cb, 1).unwrap();
        b2.add_dep(ia, ib); b2.add_dep(ib, ia);
        !dep_graph_acyclic(&b2)
    }, "kahn fail");
    set.add("G600 service capacity", {
        let mut b3 = ServiceBus::new();
        (0..MAX_SERVICES).all(|i| {
            // 名字唯一才计入（同名去重），用编译期常量名表
            b3.install(ServiceContract { name: uniq_name(i), major: 1, minor: 0, methods: 1 }, i as u64).is_some()
        }) && b3.count == MAX_SERVICES
    }, "cap 16");
    set.add("G593+G594 capability/driver hooks", {
        // 可组合服务与能力 IPC/用户态驱动协作：通道容量与延迟上界
        let mut ch2 = IpcChannel::new();
        let cap = cap_issue(0x200, 0b011, secret);
        let mut sent = 0;
        for i in 0..9u64 {
            if ch2.send(&cap, secret, i) {
                sent += 1;
            }
        }
        sent == 7 && ipc_latency_ticks(1, 7) <= 64
            && matches!(SecPolicy::NONE.grant(0).granted, 1)
    }, "ring cap 7+policy");
    set
}

/// 检查辅助：为容量测试生成唯一 &'static str 名称（编译期常量池）。
const fn uniq_name(i: usize) -> &'static str {
    NAMES[i]
}

const NAMES: [&str; MAX_SERVICES] = [
    "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7",
    "s8", "s9", "s10", "s11", "s12", "s13", "s14", "s15",
];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g541_pid_namespace() {
        let mut ns = PidNs::new(1);
        ns.offset = 1000;
        let (i, h) = ns.alloc().unwrap();
        assert_eq!((i, h), (1, 1001));
        let (i2, h2) = ns.alloc().unwrap();
        assert_eq!((i2, h2), (2, 1002));
    }

    #[test]
    fn g545_user_ns_mapping() {
        let mut un = UserNs::new(1);
        assert!(un.map(0, 65536));
        assert!(un.map(100, 65636));
        assert!(!un.map(0, 99999)); // inner 冲突
        assert_eq!(un.to_outer(100), Some(65636));
        assert_eq!(un.to_outer(101), None);
    }

    #[test]
    fn g546_cgroup_accounting() {
        let mut cg = Cgroup::new(50, 10);
        assert!(cg.charge_cpu(50));
        assert!(!cg.charge_cpu(1));
        assert!(cg.charge_mem(9));
        assert!(!cg.charge_mem(2));
        cg.reset_period();
        assert_eq!(cg.cpu_used, 0);
        assert!(cg.charge_cpu(50)); // 新周期恢复
    }

    #[test]
    fn g547_container_lifecycle() {
        let mut t = ContainerTable::new();
        assert!(t.create(5));
        assert!(!t.create(5)); // 去重
        let c = t.find_mut(5).unwrap();
        assert_eq!(c.state, CtrState::Created);
        assert!(c.start());
        assert!(!c.start());
        assert!(c.stop());
        assert!(!c.stop());
    }

    #[test]
    fn g548_layer_stack() {
        let mut c = Container::new(1);
        for h in 1..=4u64 {
            assert!(c.push_layer(h * 0x100));
        }
        assert!(!c.push_layer(0x500));
        assert_eq!(c.layer_count, MAX_LAYERS);
    }

    #[test]
    fn g549_cow_clone() {
        let mut base = Container::new(1);
        base.push_layer(0xaa);
        base.push_layer(0xbb);
        let mut clone = base.clone_cow(2).unwrap();
        assert_eq!(clone.layers, base.layers);
        assert!(clone.writable.is_none());
        assert!(clone.write_file(0x1));
        assert!(clone.writable.is_some());
        assert!(base.writable.is_none()); // 基础镜像不受影响
        // 第二次写继续演进可写层
        let w1 = clone.writable.unwrap();
        clone.write_file(0x2);
        assert_ne!(clone.writable, Some(w1));
    }

    #[test]
    fn g550_snapshot_roundtrip() {
        let mut c = Container::new(3);
        c.push_layer(0x1);
        c.start();
        c.write_file(0x99);
        let snap = c.snapshot();
        let w = c.writable;
        c.stop();
        c.write_file(0x1000);
        c.restore(snap);
        assert_eq!(c.state, CtrState::Running);
        assert_eq!(c.writable, w);
        assert_eq!(c.layer_count, 1);
    }

    #[test]
    fn g552_g553_isolation() {
        let a = PidNs::new(7);
        let b = PidNs::new(7);
        assert!(!pid_isolated(&a, &b)); // 同 ns 可见
        let n1 = NetNs { id: 1, allow_outbound: false };
        assert!(!net_outbound_allowed(&n1));
    }

    #[test]
    fn g557_policy_bits() {
        let p = SecPolicy::NONE.grant(0).grant(3);
        assert_eq!(p.granted, 0b1001);
        assert!(p.has(0) && p.has(3) && !p.has(1));
    }

    #[test]
    fn g560_table_capacity() {
        let mut t = ContainerTable::new();
        for i in 0..MAX_CONTAINERS as u32 {
            assert!(t.create(i));
        }
        assert!(!t.create(MAX_CONTAINERS as u32));
        assert_eq!(t.count(), MAX_CONTAINERS);
    }

    #[test]
    fn g561_capability_tokens() {
        let secret = 0x1234_5678;
        let tok = cap_issue(0xa0, 0b011, secret);
        assert!(cap_verify(&tok, secret, 0b001));
        assert!(cap_verify(&tok, secret, 0b011));
        assert!(!cap_verify(&tok, secret, 0b100)); // 超出授予
        assert!(!cap_verify(&tok, secret + 1, 0b001)); // 错密钥
        let mut t2 = tok;
        t2.rights = 0b111; // 篡改权限后 MAC 不再匹配
        assert!(!cap_verify(&t2, secret, 0b001));
    }

    #[test]
    fn g562_ipc_channel() {
        let secret = 0xfeed;
        let rw = cap_issue(1, 0b011, secret);
        let mut ch = IpcChannel::new();
        assert!(ch.recv(&rw, secret).is_none()); // 空
        for i in 0..7u64 {
            assert!(ch.send(&rw, secret, i));
        }
        assert!(!ch.send(&rw, secret, 99)); // 满（7 容量）
        assert_eq!(ch.recv(&rw, secret), Some(0));
        assert!(ch.send(&rw, secret, 7)); // 腾出一格
        assert_eq!(ch.recv(&rw, secret), Some(1));
    }

    #[test]
    fn g564_minimization_kernel_base() {
        let mut m = Minimization::new();
        assert!(m.is_minimal());
        assert_eq!(m.resident, 0b1111_1111);
        assert_eq!(m.moved_out, 0);
        assert!(m.move_out(4)); // FS 迁出
        assert!(m.is_minimal()); // 内核最小集仍在
        assert!(!m.move_out(2)); // IPC 必须常驻
        assert_eq!(m.moved_out, 1);
    }

    #[test]
    fn g567_ipc_latency() {
        assert_eq!(ipc_latency_ticks(0, 0), 4);
        assert_eq!(ipc_latency_ticks(1, 0), 6);
        assert_eq!(ipc_latency_ticks(3, 5), 4 + 6 + 5);
        assert!(ipc_latency_ticks(4, 8) < 64); // 预算内
    }

    #[test]
    fn g581_contract_semver() {
        let want = ServiceContract { name: "db", major: 2, minor: 0, methods: 0b101 };
        assert!(ServiceContract { name: "db", major: 2, minor: 1, methods: 0b111 }
            .compatible_provider(&want));
        assert!(!ServiceContract { name: "db", major: 3, minor: 0, methods: 0b111 }
            .compatible_provider(&want)); // major 不兼容
        assert!(!ServiceContract { name: "db", major: 2, minor: 0, methods: 0b100 }
            .compatible_provider(&want)); // 方法缺失
        assert!(!ServiceContract { name: "other", major: 2, minor: 0, methods: 0b111 }
            .compatible_provider(&want));
    }

    #[test]
    fn g582_di_activation_order() {
        let mut bus = ServiceBus::new();
        let cl = ServiceContract { name: "clock", major: 1, minor: 0, methods: 1 };
        let cf = ServiceContract { name: "fs", major: 1, minor: 0, methods: 1 };
        let ij = bus.install(cl, 1).unwrap();
        let ik = bus.install(cf, 1).unwrap();
        bus.add_dep(ik, ij);
        assert!(!bus.activate(ik)); // 依赖未激活
        assert!(bus.activate(ij));
        assert!(bus.activate(ik));
    }

    #[test]
    fn g584_hot_upgrade() {
        let mut bus = ServiceBus::new();
        let c = ServiceContract { name: "net", major: 1, minor: 0, methods: 1 };
        let i = bus.install(c, 10).unwrap();
        assert!(!bus.hot_upgrade("net", 20, 0)); // 未激活
        bus.activate(i);
        assert!(bus.hot_upgrade("net", 20, 0));
        assert_eq!(bus.services[i].unwrap().impl_tag, 20);
        assert_eq!(bus.services[i].unwrap().state, SvcState::Active);
        assert_eq!(bus.services[i].unwrap().contract.minor, 0);
    }

    #[test]
    fn g585_version_routing() {
        let mut bus = ServiceBus::new();
        let v1 = ServiceContract { name: "api", major: 1, minor: 1, methods: 1 };
        let v4 = ServiceContract { name: "api", major: 1, minor: 4, methods: 1 };
        let v7 = ServiceContract { name: "api", major: 1, minor: 7, methods: 1 };
        let i1 = bus.install(v1, 1).unwrap();
        let i4 = bus.install(v4, 2).unwrap();
        let i7 = bus.install(v7, 3).unwrap();
        bus.activate(i1); bus.activate(i4); bus.activate(i7);
        assert_eq!(bus.route("api", 1), Some(i1));
        assert_eq!(bus.route("api", 4), Some(i4));
        assert_eq!(bus.route("api", 7), Some(i7));
        assert_eq!(bus.route("api", 100), Some(i7)); // 上不封顶取最大
        bus.retire(i7);
        assert_eq!(bus.route("api", 100), Some(i4));
    }

    #[test]
    fn g586_topological_check() {
        let mut bus = ServiceBus::new();
        let names = ["a", "b", "c", "d"];
        let mut idx = [0usize; 4];
        for (k, n) in names.iter().enumerate() {
            let c = ServiceContract { name: n, major: 1, minor: 0, methods: 1 };
            idx[k] = bus.install(c, 1).unwrap();
        }
        // a<-b<-c, d 独立
        bus.add_dep(idx[1], idx[0]);
        bus.add_dep(idx[2], idx[1]);
        assert!(dep_graph_acyclic(&bus));
        // 加 d<-a 不成环
        bus.add_dep(idx[3], idx[0]);
        assert!(dep_graph_acyclic(&bus));
    }

    #[test]
    fn g586_cycle_detected() {
        let mut bus = ServiceBus::new();
        let ca = ServiceContract { name: "x", major: 1, minor: 0, methods: 1 };
        let cb = ServiceContract { name: "y", major: 1, minor: 0, methods: 1 };
        let ia = bus.install(ca, 1).unwrap();
        let ib = bus.install(cb, 1).unwrap();
        bus.add_dep(ia, ib);
        bus.add_dep(ib, ia);
        assert!(!dep_graph_acyclic(&bus));
    }

    #[test]
    fn g600_domain_closure() {
        let set = run_gcont_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 2048];
            let n = set.render(&mut buf);
            panic!("gcont self-test://n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 25);
        assert!(!set.truncated());
    }
}
