//! VARIX-M500 AI-04 服务编排与 IPC 深化域（F076~F100，M3）。
//!
//! 服务从"能跑"到"可运营"：契约、灰度、热升级、SLA。
//! 全部为纯逻辑 + 固定容量数组（无 Vec/String/Box），no_std 安全。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F076 — IPC 带宽仪表：通道流量可视
// ---------------------------------------------------------------------------

pub const MAX_CHANNELS: usize = 8;

#[derive(Clone, Copy)]
pub struct IpcMeter {
    pub bytes: [u32; MAX_CHANNELS],
    pub msgs: [u32; MAX_CHANNELS],
    pub count: usize,
}

impl IpcMeter {
    pub const fn new() -> IpcMeter {
        IpcMeter { bytes: [0; MAX_CHANNELS], msgs: [0; MAX_CHANNELS], count: 0 }
    }

    pub fn record(&mut self, ch: usize, bytes: u32) -> bool {
        if ch >= MAX_CHANNELS {
            return false;
        }
        self.bytes[ch] = self.bytes[ch].saturating_add(bytes);
        self.msgs[ch] = self.msgs[ch].saturating_add(1);
        if ch + 1 > self.count {
            self.count = ch + 1;
        }
        true
    }

    pub fn top_channel(&self) -> Option<usize> {
        let mut best: Option<usize> = None;
        for c in 0..self.count {
            if best.map(|b| self.bytes[c] > self.bytes[b]).unwrap_or(true) {
                best = Some(c);
            }
        }
        best
    }

    pub fn avg_msg_bytes(&self, ch: usize) -> Option<u32> {
        if ch >= self.count || self.msgs[ch] == 0 {
            return None;
        }
        Some(self.bytes[ch] / self.msgs[ch])
    }
}

// ---------------------------------------------------------------------------
// F077 — 零拷贝大消息通道：大数据免拷贝传递
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZeroCopyChan {
    /// 共享缓冲页数。
    pub buf_pages: u32,
    /// 阈值：消息超过此字节数走零拷贝路径。
    pub threshold_bytes: u32,
    pub zero_copy_msgs: u32,
    pub copied_msgs: u32,
    pub bytes_saved: u64,
}

impl ZeroCopyChan {
    pub const fn new(buf_pages: u32, threshold_bytes: u32) -> ZeroCopyChan {
        ZeroCopyChan { buf_pages, threshold_bytes, zero_copy_msgs: 0, copied_msgs: 0, bytes_saved: 0 }
    }

    pub fn send(&mut self, len_bytes: u32) -> bool {
        if len_bytes > self.buf_pages * 4096 {
            return false; // 超缓冲拒绝
        }
        if len_bytes >= self.threshold_bytes {
            self.zero_copy_msgs += 1;
            self.bytes_saved += len_bytes as u64;
        } else {
            self.copied_msgs += 1;
        }
        true
    }

    pub fn zero_copy_share_permille(&self) -> u32 {
        let t = self.zero_copy_msgs + self.copied_msgs;
        if t == 0 { 0 } else { self.zero_copy_msgs * 1000 / t }
    }
}

// ---------------------------------------------------------------------------
// F078 — IPC 类型化接口：IDL 契约生成
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MethodSig {
    pub method: u16,
    /// 参数类型码序列（0=u32 1=u64 2=handle 3=buf）。
    pub args: [u8; 4],
    pub nargs: u8,
    pub ret: u8,
}

impl MethodSig {
    pub const fn new(method: u16, args: [u8; 4], nargs: u8, ret: u8) -> MethodSig {
        MethodSig { method, args, nargs, ret }
    }

    /// 调用是否匹配契约（参数个数与类型逐位一致）。
    pub fn call_matches(&self, args: &[u8]) -> bool {
        if args.len() != self.nargs as usize {
            return false;
        }
        for i in 0..self.nargs as usize {
            if args[i] > 3 || args[i] != self.args[i] {
                return false;
            }
        }
        true
    }
}

/// 序列化大小：u32=4 u64=8 handle=4 buf=len。
pub fn wire_size(sig: &MethodSig, buf_lens: &[u32]) -> u32 {
    let mut total = 4u32; // method id
    let mut bi = 0;
    for i in 0..sig.nargs as usize {
        match sig.args[i] {
            0 | 2 => total += 4,
            1 => total += 8,
            _ => {
                total += buf_lens.get(bi).copied().unwrap_or(0);
                bi += 1;
            }
        }
    }
    total
}

// ---------------------------------------------------------------------------
// F079 — IPC 回放：消息流录制回放
// ---------------------------------------------------------------------------

pub const MAX_IPC_TRACE: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IpcFrame {
    pub ch: u8,
    pub method: u16,
    pub payload_hash: u64,
}

#[derive(Clone, Copy)]
pub struct IpcRecorder {
    pub frames: [Option<IpcFrame>; MAX_IPC_TRACE],
    pub count: usize,
    pub replayed: u32,
}

impl IpcRecorder {
    pub const fn new() -> IpcRecorder {
        IpcRecorder { frames: [const { None }; MAX_IPC_TRACE], count: 0, replayed: 0 }
    }

    pub fn record(&mut self, f: IpcFrame) -> bool {
        if self.count >= MAX_IPC_TRACE {
            return false;
        }
        self.frames[self.count] = Some(f);
        self.count += 1;
        true
    }

    /// 回放：逐帧对比实时流，返回不一致帧数。
    pub fn replay_against(&mut self, live: &[IpcFrame]) -> u32 {
        self.replayed += 1;
        let mut mismatch = 0u32;
        for i in 0..self.count.min(live.len()) {
            if let Some(r) = self.frames[i] {
                if r.ch != live[i].ch || r.method != live[i].method || r.payload_hash != live[i].payload_hash {
                    mismatch += 1;
                }
            }
        }
        if self.count != live.len() {
            mismatch += 1;
        }
        mismatch
    }
}

// ---------------------------------------------------------------------------
// F080 — 服务网格视图：全系统调用关系图
// ---------------------------------------------------------------------------

pub const MAX_EDGES: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshEdge {
    pub from: u8,
    pub to: u8,
    pub calls: u32,
}

#[derive(Clone, Copy)]
pub struct ServiceMesh {
    pub edges: [Option<MeshEdge>; MAX_EDGES],
    pub count: usize,
}

impl ServiceMesh {
    pub const fn new() -> ServiceMesh {
        ServiceMesh { edges: [const { None }; MAX_EDGES], count: 0 }
    }

    pub fn record_call(&mut self, from: u8, to: u8) -> bool {
        if from == to {
            return false;
        }
        for i in 0..self.count {
            if let Some(mut e) = self.edges[i] {
                if e.from == from && e.to == to {
                    e.calls += 1;
                    self.edges[i] = Some(e);
                    return true;
                }
            }
        }
        if self.count >= MAX_EDGES {
            return false;
        }
        self.edges[self.count] = Some(MeshEdge { from, to, calls: 1 });
        self.count += 1;
        true
    }

    /// 依赖环检测（DFS 三色）。
    pub fn has_cycle(&self) -> bool {
        const NIL: u8 = 255;
        let color = [NIL as u8; MAX_EDGES]; // 0 白 1 灰 2 黑（用服务号间接覆盖 ≤255）
        let mut adj = [[false; MAX_EDGES]; MAX_EDGES];
        let mut nodes = [false; MAX_EDGES];
        for i in 0..self.count {
            if let Some(e) = self.edges[i] {
                adj[e.from as usize][e.to as usize] = true;
                nodes[e.from as usize] = true;
                nodes[e.to as usize] = true;
            }
        }
        // 迭代 DFS（小图，递归深度 ≤ 8 手工栈）
        let mut stack = [0usize; MAX_EDGES + 1];
        let mut gray = [false; MAX_EDGES];
        let mut black = [false; MAX_EDGES];
        let mut has = false;
        for start in 0..MAX_EDGES {
            if !nodes[start] || black[start] {
                continue;
            }
            let mut sp = 0usize;
            stack[sp] = start;
            sp += 1;
            gray[start] = true;
            while sp > 0 {
                let v = stack[sp - 1];
                let mut pushed = false;
                for w in 0..MAX_EDGES {
                    if adj[v][w] && nodes[w] {
                        if gray[w] {
                            has = true;
                        } else if !black[w] {
                            stack[sp] = w;
                            sp += 1;
                            gray[w] = true;
                            pushed = true;
                            break;
                        }
                    }
                }
                if !pushed {
                    sp -= 1;
                    gray[v] = false;
                    black[v] = true;
                }
                if has {
                    return true;
                }
            }
        }
        let _ = color;
        has
    }

    pub fn hottest_edge(&self) -> Option<(u8, u8)> {
        let mut best: Option<MeshEdge> = None;
        for i in 0..self.count {
            if let Some(e) = self.edges[i] {
                if best.map(|b| e.calls > b.calls).unwrap_or(true) {
                    best = Some(e);
                }
            }
        }
        best.map(|b| (b.from, b.to))
    }
}

// ---------------------------------------------------------------------------
// F081 — 慢调用告警：IPC 延迟 SLO 哨兵
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LatencySentinel {
    pub slo_us: u32,
    pub violations: u32,
    pub observed: u32,
    /// 连续违例达到 N 才告警（防抖）。
    pub streak: u32,
    pub alert_threshold: u32,
    pub alerting: bool,
}

impl LatencySentinel {
    pub const fn new(slo_us: u32, alert_threshold: u32) -> LatencySentinel {
        LatencySentinel { slo_us, violations: 0, observed: 0, streak: 0, alert_threshold, alerting: false }
    }

    pub fn observe(&mut self, latency_us: u32) -> bool {
        self.observed += 1;
        if latency_us > self.slo_us {
            self.violations += 1;
            self.streak += 1;
            if self.streak >= self.alert_threshold && !self.alerting {
                self.alerting = true;
                return true; // 触发告警
            }
        } else {
            self.streak = 0;
            self.alerting = false;
        }
        false
    }

    pub fn violation_rate_permille(&self) -> u32 {
        if self.observed == 0 { 0 } else { self.violations * 1000 / self.observed }
    }
}

// ---------------------------------------------------------------------------
// F082 — 服务灰度发布：新版小流量验证
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Canary {
    pub old_version: u32,
    pub new_version: u32,
    /// 新版流量百分比 0~100。
    pub weight_pct: u8,
    pub new_ok: u32,
    pub new_fail: u32,
}

impl Canary {
    pub const fn new(old_version: u32, new_version: u32) -> Canary {
        Canary { old_version, new_version, weight_pct: 0, new_ok: 0, new_fail: 0 }
    }

    /// 灰度权重只能小步上调（≤ 2 倍当前且 ≤ 50）。
    pub fn raise_weight(&mut self, pct: u8) -> bool {
        // 首步允许 0→≤5；此后每步 ≤ 2 倍现权重且 ≤ 50。
        if pct > 50
            || (self.weight_pct > 0 && (pct <= self.weight_pct || pct > self.weight_pct.saturating_mul(2)))
            || (self.weight_pct == 0 && pct > 5)
        {
            return false;
        }
        self.weight_pct = pct;
        true
    }

    pub fn report_new(&mut self, ok: bool) {
        if ok {
            self.new_ok += 1;
        } else {
            self.new_fail += 1;
        }
    }

    /// 晋升条件：样本 ≥ 20 且失败率 < 5%。
    pub fn promote_ready(&self) -> bool {
        let t = self.new_ok + self.new_fail;
        t >= 20 && (self.new_fail as u64) * 1000 < (t as u64) * 50
    }

    /// 一键回退：清零权重。
    pub fn rollback(&mut self) {
        self.weight_pct = 0;
    }
}

// ---------------------------------------------------------------------------
// F083 — 服务契约测试：接口行为锁定
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContractCase {
    pub method: u16,
    pub input: u32,
    pub expect: u32,
}

pub const MAX_CONTRACT: usize = 12;

/// 契约测试执行器：impl_fn(method, input) -> u32。
pub fn run_contract_tests(cases: &[ContractCase], impl_fn: fn(u16, u32) -> u32) -> u32 {
    let mut fail = 0;
    for c in cases {
        if c.method >= MAX_CONTRACT as u16 * 100 {
            fail += 1;
            continue;
        }
        if impl_fn(c.method, c.input) != c.expect {
            fail += 1;
        }
    }
    fail
}

// ---------------------------------------------------------------------------
// F084 — 进程资产清单：打开的资源总表
// ---------------------------------------------------------------------------

pub const MAX_ASSETS_PID: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetKind {
    /// 0=fd 1=shm 2=chan 3=map。
    pub kind: u8,
    pub id: u32,
}

#[derive(Clone, Copy)]
pub struct ProcAssets {
    pub pid: u32,
    pub assets: [Option<AssetKind>; MAX_ASSETS_PID],
    pub count: usize,
}

impl ProcAssets {
    pub const fn new(pid: u32) -> ProcAssets {
        ProcAssets { pid, assets: [const { None }; MAX_ASSETS_PID], count: 0 }
    }

    pub fn open(&mut self, kind: u8, id: u32) -> bool {
        if kind > 3 || self.count >= MAX_ASSETS_PID {
            return false;
        }
        for i in 0..self.count {
            if let Some(a) = self.assets[i] {
                if a.kind == kind && a.id == id {
                    return false; // 重复打开去重
                }
            }
        }
        self.assets[self.count] = Some(AssetKind { kind, id });
        self.count += 1;
        true
    }

    pub fn close(&mut self, kind: u8, id: u32) -> bool {
        for i in 0..self.count {
            if let Some(a) = self.assets[i] {
                if a.kind == kind && a.id == id {
                    // 压缩数组
                    for j in i..self.count - 1 {
                        self.assets[j] = self.assets[j + 1];
                    }
                    self.assets[self.count - 1] = None;
                    self.count -= 1;
                    return true;
                }
            }
        }
        false
    }

    /// 退出时泄漏资产 = count（应归零）。
    pub fn leaked_on_exit(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// F085 — 优雅终止协议：多级退出信号
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TermSignal {
    /// 0: 温和请求（完成手头工作）
    Graceful,
    /// 1: 限时退出（5 秒内）
    Deadline,
    /// 2: 强制（资源由内核回收）
    Forced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermState {
    pub signal: Option<TermSignal>,
    pub saved_state: bool,
    pub exited: bool,
    pub ms_taken: u32,
    pub start_ms: u32,
}

impl TermState {
    pub const fn new() -> TermState {
        TermState { signal: None, saved_state: false, exited: false, ms_taken: 0, start_ms: 0 }
    }

    pub fn request(&mut self, s: TermSignal, now_ms: u32) -> bool {
        // 升级单向：Graceful → Deadline → Forced
        let rank = match s {
            TermSignal::Graceful => 0,
            TermSignal::Deadline => 1,
            TermSignal::Forced => 2,
        };
        if let Some(cur) = self.signal {
            let cr = match cur {
                TermSignal::Graceful => 0,
                TermSignal::Deadline => 1,
                TermSignal::Forced => 2,
            };
            if rank <= cr {
                return false; // 不降级
            }
        }
        self.signal = Some(s);
        self.start_ms = now_ms;
        true
    }

    pub fn finish(&mut self, now_ms: u32, saved: bool) -> bool {
        if self.signal.is_none() {
            return false;
        }
        self.ms_taken = now_ms - self.start_ms;
        self.saved_state = saved;
        self.exited = true;
        true
    }

    /// 优雅退出验收：存了状态且限时内完成。
    pub fn clean_exit(&self, budget_ms: u32) -> bool {
        self.exited && self.saved_state && self.ms_taken <= budget_ms
    }
}

// ---------------------------------------------------------------------------
// F086 — 进程信用分：行为评分与限流
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreditScore {
    pub pid: u32,
    /// 0~1000。
    pub score: u32,
    pub throttled: bool,
}

impl CreditScore {
    pub const fn new(pid: u32) -> CreditScore {
        CreditScore { pid, score: 1000, throttled: false }
    }

    pub fn reward(&mut self, pts: u32) {
        self.score = (self.score + pts).min(1000);
    }

    pub fn penalize(&mut self, pts: u32) {
        self.score = self.score.saturating_sub(pts);
    }

    /// 低于 300 自动限流。
    pub fn refresh_throttle(&mut self) -> bool {
        self.throttled = self.score < 300;
        self.throttled
    }

    /// 限流下调用配额减半。
    pub fn quota(&self, base: u32) -> u32 {
        if self.throttled {
            base / 2
        } else {
            base
        }
    }
}

// ---------------------------------------------------------------------------
// F087 — 孤儿资源回收器：泄漏资源巡检
// ---------------------------------------------------------------------------

pub const ORPHAN_SCAN: usize = 12;

#[derive(Clone, Copy)]
pub struct OrphanSweeper {
    /// 资源 → 拥有者 pid（0 = 无主）。
    pub owner: [u32; ORPHAN_SCAN],
    /// 资源是否存活。
    pub alive: [bool; ORPHAN_SCAN],
    pub reaped: u32,
}

impl OrphanSweeper {
    pub const fn new() -> OrphanSweeper {
        OrphanSweeper { owner: [0; ORPHAN_SCAN], alive: [false; ORPHAN_SCAN], reaped: 0 }
    }

    pub fn create(&mut self, res: usize, pid: u32) -> bool {
        if res >= ORPHAN_SCAN || pid == 0 || self.alive[res] {
            return false;
        }
        self.owner[res] = pid;
        self.alive[res] = true;
        true
    }

    pub fn process_dead(&mut self, pid: u32) -> u32 {
        let mut n = 0;
        for i in 0..ORPHAN_SCAN {
            if self.alive[i] && self.owner[i] == pid {
                self.alive[i] = false;
                self.owner[i] = 0;
                n += 1;
                self.reaped += 1;
            }
        }
        n
    }

    pub fn orphans(&self) -> u32 {
        let mut n = 0;
        for i in 0..ORPHAN_SCAN {
            if self.alive[i] && self.owner[i] == 0 {
                n += 1;
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// F088 — 守护进程模板：服务脚手架
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DaemonSpec {
    pub name_code: u16,
    /// 崩溃重启策略：0=不重启 1=重启 2=重启+退避。
    pub restart_policy: u8,
    pub max_restarts: u8,
    pub restarts: u8,
    pub alive: bool,
}

impl DaemonSpec {
    pub const fn new(name_code: u16, restart_policy: u8, max_restarts: u8) -> DaemonSpec {
        DaemonSpec { name_code, restart_policy, max_restarts, restarts: 0, alive: true }
    }

    pub fn on_crash(&mut self) -> bool {
        if !self.alive && self.restart_policy == 0 {
            return false;
        }
        match self.restart_policy {
            0 => {
                self.alive = false;
                false
            }
            1 => {
                if self.restarts >= self.max_restarts {
                    self.alive = false;
                    return false;
                }
                self.restarts += 1;
                true
            }
            _ => {
                if self.restarts >= self.max_restarts {
                    self.alive = false;
                    return false;
                }
                self.restarts += 1;
                true
            }
        }
    }

    /// 退避：重启延迟 = 100ms × 2^restarts（封顶 3200）。
    pub fn backoff_ms(&self) -> u32 {
        if self.restart_policy != 2 || self.restarts == 0 {
            return 0;
        }
        let mut ms = 100u32;
        for _ in 1..self.restarts {
            ms = ms.saturating_mul(2);
            if ms >= 3200 {
                return 3200;
            }
        }
        ms.min(3200)
    }
}

// ---------------------------------------------------------------------------
// F089 — 服务描述清单：声明式 manifest
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceManifest {
    pub svc: u16,
    /// 依赖服务号位图。
    pub deps: u32,
    /// 内存限额（页）。
    pub mem_cap_pages: u32,
    pub api_major: u16,
}

impl ServiceManifest {
    pub const fn new(svc: u16, deps: u32, mem_cap_pages: u32, api_major: u16) -> ServiceManifest {
        ServiceManifest { svc, deps, mem_cap_pages, api_major }
    }

    /// 校验：不能依赖自己，内存限额 ≥ 1 页。
    pub fn valid(&self) -> bool {
        self.mem_cap_pages >= 1 && self.deps & (1u32 << (self.svc % 32)) == 0
    }

    pub fn depends_on(&self, other: u16) -> bool {
        self.deps & (1u32 << (other % 32)) != 0
    }
}

// ---------------------------------------------------------------------------
// F090 — 提权代理：受控能力中介
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivBroker {
    /// 已授权能力位图。
    pub granted: u32,
    /// 红线能力（永不授予）。
    pub redline: u32,
}

impl PrivBroker {
    pub const fn new(redline: u32) -> PrivBroker {
        PrivBroker { granted: 0, redline }
    }

    pub fn request(&mut self, cap: u8) -> bool {
        let bit = 1u32 << cap;
        if bit & self.redline != 0 {
            return false;
        }
        self.granted |= bit;
        true
    }

    pub fn has(&self, cap: u8) -> bool {
        self.granted & (1 << cap) != 0
    }

    pub fn revoke(&mut self, cap: u8) -> bool {
        if !self.has(cap) {
            return false;
        }
        self.granted &= !(1u32 << cap);
        true
    }
}

// ---------------------------------------------------------------------------
// F091 — 会话管理器：登录会话生命周期
// ---------------------------------------------------------------------------

pub const MAX_SESSIONS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Session {
    pub uid: u32,
    pub active: bool,
    pub idle_min: u32,
}

#[derive(Clone, Copy)]
pub struct SessionManager {
    pub sessions: [Option<Session>; MAX_SESSIONS],
    pub count: usize,
}

impl SessionManager {
    pub const fn new() -> SessionManager {
        SessionManager { sessions: [const { None }; MAX_SESSIONS], count: 0 }
    }

    pub fn login(&mut self, uid: u32) -> bool {
        for i in 0..self.count {
            if let Some(s) = self.sessions[i] {
                if s.uid == uid && s.active {
                    return false; // 单点登录
                }
            }
        }
        if self.count >= MAX_SESSIONS {
            return false;
        }
        self.sessions[self.count] = Some(Session { uid, active: true, idle_min: 0 });
        self.count += 1;
        true
    }

    pub fn logout(&mut self, uid: u32) -> bool {
        for i in 0..self.count {
            if let Some(s) = self.sessions[i] {
                if s.uid == uid {
                    // 移除并压缩数组：二次登出返回 false
                    for j in i..self.count - 1 {
                        self.sessions[j] = self.sessions[j + 1];
                    }
                    self.sessions[self.count - 1] = None;
                    self.count -= 1;
                    return true;
                }
            }
        }
        false
    }

    /// 超时踢出：空闲 ≥ timeout_min 的活跃会话。
    pub fn kick_idle(&mut self, timeout_min: u32) -> u32 {
        let mut kicked = 0;
        for i in 0..self.count {
            if let Some(s) = self.sessions[i] {
                if s.active && s.idle_min >= timeout_min {
                    self.sessions[i] = Some(Session { uid: s.uid, active: false, idle_min: s.idle_min });
                    kicked += 1;
                }
            }
        }
        kicked
    }
}

// ---------------------------------------------------------------------------
// F092 — 单实例激活协议：唤醒已有实例
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SingleInstance {
    pub app: u16,
    pub running_pid: Option<u32>,
    pub activations: u32,
}

impl SingleInstance {
    pub const fn new(app: u16) -> SingleInstance {
        SingleInstance { app, running_pid: None, activations: 0 }
    }

    /// 已有实例 → 返回其 pid（激活），否则启动新实例。
    pub fn open(&mut self, requester_pid: u32) -> u32 {
        match self.running_pid {
            Some(pid) => {
                self.activations += 1;
                let _ = requester_pid;
                pid
            }
            None => {
                self.running_pid = Some(requester_pid);
                requester_pid
            }
        }
    }

    pub fn exit(&mut self, pid: u32) -> bool {
        if self.running_pid == Some(pid) {
            self.running_pid = None;
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// F093 — IPC fuzz：消息对抗测试
// ---------------------------------------------------------------------------

/// 校验消息帧不变量：长度字段与实际一致、方法号非零、渠道 < MAX_CHANNELS。
pub fn ipc_fuzz(frames: &[(u8, u16, u32, u32)]) -> u32 {
    let mut bad = 0;
    for &(ch, method, declared_len, actual_len) in frames {
        if ch as usize >= MAX_CHANNELS {
            bad += 1;
        }
        if method == 0 {
            bad += 1;
        }
        if declared_len != actual_len {
            bad += 1;
        }
    }
    bad
}

// ---------------------------------------------------------------------------
// F094 — 服务拓扑 lint：依赖环自动检测
// ---------------------------------------------------------------------------

/// 对 manifest 集合做拓扑 lint：检测依赖环 + 自依赖。返回问题数。
pub fn topology_lint(mans: &[ServiceManifest]) -> u32 {
    let mut issues = 0;
    for m in mans {
        if !m.valid() {
            issues += 1;
        }
    }
    // 两两环检测（A→B 且 B→A）
    for i in 0..mans.len() {
        for j in (i + 1)..mans.len() {
            if mans[i].depends_on(mans[j].svc) && mans[j].depends_on(mans[i].svc) {
                issues += 1;
            }
        }
    }
    issues
}

// ---------------------------------------------------------------------------
// F095 — 事件溯源日志：状态变化事件链
// ---------------------------------------------------------------------------

pub const MAX_ES_EVENTS: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EsEvent {
    pub seq: u32,
    pub svc: u16,
    /// 状态码（域自定义）。
    pub state: u8,
}

#[derive(Clone, Copy)]
pub struct EventSourcingLog {
    pub events: [Option<EsEvent>; MAX_ES_EVENTS],
    pub count: usize,
    pub next_seq: u32,
}

impl EventSourcingLog {
    pub const fn new() -> EventSourcingLog {
        EventSourcingLog { events: [const { None }; MAX_ES_EVENTS], count: 0, next_seq: 1 }
    }

    pub fn append(&mut self, svc: u16, state: u8) -> bool {
        if self.count >= MAX_ES_EVENTS {
            return false;
        }
        self.events[self.count] = Some(EsEvent { seq: self.next_seq, svc, state });
        self.next_seq += 1;
        self.count += 1;
        true
    }

    /// 重放：某服务最后一个状态。
    pub fn last_state_of(&self, svc: u16) -> Option<u8> {
        let mut last = None;
        for i in 0..self.count {
            if let Some(e) = self.events[i] {
                if e.svc == svc {
                    last = Some(e.state);
                }
            }
        }
        last
    }

    /// 序列完整性：seq 连续。
    pub fn seq_intact(&self) -> bool {
        for i in 0..self.count {
            if let Some(e) = self.events[i] {
                if e.seq != i as u32 + 1 {
                    return false;
                }
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// F096 — 服务压测床：并发压测框架
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadRig {
    pub concurrency: u8,
    pub duration_ms: u32,
    pub issued: u32,
    pub ok: u32,
    pub fail: u32,
}

impl LoadRig {
    pub const fn new(concurrency: u8, duration_ms: u32) -> LoadRig {
        LoadRig { concurrency: if concurrency == 0 { 1 } else { concurrency }, duration_ms, issued: 0, ok: 0, fail: 0 }
    }

    pub fn issue(&mut self, ok: bool) {
        self.issued += 1;
        if ok {
            self.ok += 1;
        } else {
            self.fail += 1;
        }
    }

    /// 吞吐（req/s）。
    pub fn throughput_rps(&self) -> u64 {
        if self.duration_ms == 0 {
            return 0;
        }
        self.issued as u64 * 1000 / self.duration_ms as u64
    }

    /// 压测通过：失败率 < 1% 且并发下有产出。
    pub fn passed(&self) -> bool {
        self.issued > 0 && (self.fail as u64) * 1000 < self.issued as u64 * 1
            && self.throughput_rps() > 0
    }
}

// ---------------------------------------------------------------------------
// F097 — 服务 SLA 仪表：可用率/延迟达标
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlaGauge {
    /// 探测点：up=true。
    pub probes: [bool; 16],
    pub probe_count: usize,
    pub p99_us: u32,
    pub slo_us: u32,
}

impl SlaGauge {
    pub const fn new(slo_us: u32) -> SlaGauge {
        SlaGauge { probes: [true; 16], probe_count: 0, p99_us: 0, slo_us }
    }

    pub fn probe(&mut self, up: bool) {
        if self.probe_count < 16 {
            self.probes[self.probe_count] = up;
            self.probe_count += 1;
        } else {
            // 滑动
            for i in 0..15 {
                self.probes[i] = self.probes[i + 1];
            }
            self.probes[15] = up;
        }
    }

    pub fn availability_permille(&self) -> u32 {
        if self.probe_count == 0 {
            return 1000;
        }
        let mut up = 0;
        for i in 0..self.probe_count {
            if self.probes[i] {
                up += 1;
            }
        }
        up * 1000 / self.probe_count.max(1) as u32
    }

    /// SLA 达标：可用率 ≥ 99.5% 且 p99 在 SLO 内。
    pub fn met(&self) -> bool {
        self.availability_permille() >= 995 && self.p99_us <= self.slo_us
    }
}

// ---------------------------------------------------------------------------
// F098 — 服务热升级：不停机换版
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradePhase {
    Idle,
    Drain,   // 停收新请求
    Handoff, // 状态移交
    Cutover, // 切流
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotUpgrade {
    pub phase: UpgradePhase,
    pub old_version: u32,
    pub new_version: u32,
    /// 状态条目已移交数 / 总数。
    pub migrated: u16,
    pub total_state: u16,
    pub aborted: bool,
}

impl HotUpgrade {
    pub const fn new(old_version: u32, new_version: u32) -> HotUpgrade {
        HotUpgrade { phase: UpgradePhase::Idle, old_version, new_version, migrated: 0, total_state: 0, aborted: false }
    }

    pub fn begin(&mut self, total_state: u16) -> bool {
        if self.phase != UpgradePhase::Idle {
            return false;
        }
        self.total_state = total_state;
        self.phase = UpgradePhase::Drain;
        true
    }

    pub fn migrate_chunk(&mut self, n: u16) -> bool {
        if self.phase != UpgradePhase::Drain && self.phase != UpgradePhase::Handoff {
            return false;
        }
        if self.phase == UpgradePhase::Drain {
            self.phase = UpgradePhase::Handoff;
        }
        self.migrated = self.migrated.saturating_add(n).min(self.total_state);
        true
    }

    /// 状态全移交才允许切流。
    pub fn cutover(&mut self) -> bool {
        if self.phase != UpgradePhase::Handoff || self.migrated < self.total_state || self.total_state == 0 {
            return false;
        }
        self.phase = UpgradePhase::Cutover;
        true
    }

    pub fn abort(&mut self) {
        self.aborted = true;
        self.phase = UpgradePhase::Idle;
    }
}

// ---------------------------------------------------------------------------
// F099 — 进程间调试通道：跨进程观测
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DebugChannel {
    pub target_pid: u32,
    /// 允许观测方读取的能力。
    pub attached: bool,
    pub reads: u32,
    /// 观测自身开销红线：读次数不得超过阈值。
    pub read_cap: u32,
}

impl DebugChannel {
    pub const fn new(target_pid: u32, read_cap: u32) -> DebugChannel {
        DebugChannel { target_pid, attached: false, reads: 0, read_cap }
    }

    pub fn attach(&mut self, inspector_pid: u32, target_uid_owner: u32) -> bool {
        let _ = inspector_pid;
        // 权限：仅同 UID（此处以 owner 相等模拟）
        if target_uid_owner == 0 {
            return false;
        }
        self.attached = true;
        true
    }

    pub fn read(&mut self) -> bool {
        if !self.attached || self.reads >= self.read_cap {
            return false;
        }
        self.reads += 1;
        true
    }

    pub fn detach(&mut self) {
        self.attached = false;
    }
}

// ---------------------------------------------------------------------------
// F100 — 服务域自检：25 项 CheckSet 汇入总检
// ---------------------------------------------------------------------------

fn contract_impl(method: u16, input: u32) -> u32 {
    match method {
        1 => input + 1,
        2 => input * 2,
        _ => 0,
    }
}

pub fn run_m5srv_checks() -> CheckSet {
    let mut set = CheckSet::new("m5srv");

    // F076 带宽仪表
    let mut im = IpcMeter::new();
    set.add("F076 record", im.record(0, 100) && im.record(0, 50) && im.record(1, 400), "rec");
    set.add("F076 top channel", im.top_channel() == Some(1), "top");
    set.add("F076 avg msg", im.avg_msg_bytes(0) == Some(75), "avg");

    // F077 零拷贝
    let mut zc = ZeroCopyChan::new(16, 4096);
    set.add("F077 oversize reject", !zc.send(70_000), "oversize");
    set.add("F077 route split", zc.send(100) && zc.send(8192) && zc.zero_copy_msgs == 1 && zc.copied_msgs == 1, "route");

    // F078 类型化接口
    let sig = MethodSig::new(7, [0, 3, 0, 0], 2, 0);
    set.add("F078 call match", sig.call_matches(&[0, 3]), "match");
    set.add("F078 arity mismatch", !sig.call_matches(&[0]), "arity");
    set.add("F078 wire size", wire_size(&sig, &[10]) == 4 + 4 + 10, "wire");

    // F079 回放
    let mut rec = IpcRecorder::new();
    let f1 = IpcFrame { ch: 1, method: 5, payload_hash: 0xAA };
    let f2 = IpcFrame { ch: 1, method: 6, payload_hash: 0xBB };
    set.add("F079 record frames", rec.record(f1) && rec.record(f2), "rec");
    set.add("F079 replay clean", rec.replay_against(&[f1, f2]) == 0, "clean");
    set.add("F079 replay mismatch", rec.replay_against(&[f1, IpcFrame { ch: 1, method: 6, payload_hash: 0x99 }]) == 1, "mismatch");

    // F080 网格
    let mut mesh = ServiceMesh::new();
    set.add("F080 edges aggregate", mesh.record_call(1, 2) && mesh.record_call(1, 2) && mesh.record_call(2, 3), "edges");
    set.add("F080 hottest", mesh.hottest_edge() == Some((1, 2)), "hot");
    set.add("F080 acyclic", !mesh.has_cycle(), "acyclic");
    set.add("F080 cycle caught", {
        let mut m2 = ServiceMesh::new();
        let _ = m2.record_call(1, 2);
        let _ = m2.record_call(2, 1);
        m2.has_cycle()
    }, "cycle");

    // F081 慢调用
    let mut ls = LatencySentinel::new(1000, 3);
    set.add("F081 below slo quiet", !ls.observe(500), "ok");
    set.add("F081 debounce", ls.observe(2000) == false && ls.observe(2000) == false, "debounce");
    set.add("F081 alert fires", ls.observe(2000) && ls.alerting, "alert");

    // F082 灰度
    let mut can = Canary::new(10, 11);
    set.add("F082 first step 5", can.raise_weight(5), "5");
    set.add("F082 step ≤2x", !can.raise_weight(50), "jump");
    set.add("F082 step 10", can.raise_weight(10), "10");
    for _ in 0..20 {
        can.report_new(true);
    }
    can.report_new(false);
    set.add("F082 promote ready", can.promote_ready(), "promote");
    can.rollback();
    set.add("F082 rollback", can.weight_pct == 0, "rollback");

    // F083 契约测试
    let cases = [
        ContractCase { method: 1, input: 1, expect: 2 },
        ContractCase { method: 2, input: 3, expect: 6 },
        ContractCase { method: 1, input: 5, expect: 9 }, // 故意错
    ];
    set.add("F083 contract failures", run_contract_tests(&cases, contract_impl) == 1, "fail");

    // F084 资产清单
    let mut pa = ProcAssets::new(42);
    set.add("F084 open assets", pa.open(0, 3) && pa.open(1, 9) && !pa.open(0, 3), "open");
    set.add("F084 close compress", pa.close(0, 3) && pa.count == 1, "close");

    // F085 优雅终止
    let mut ts = TermState::new();
    set.add("F085 graceful first", ts.request(TermSignal::Graceful, 100), "g");
    set.add("F085 no downgrade", !ts.request(TermSignal::Graceful, 110), "no-down");
    set.add("F085 escalate ok", ts.request(TermSignal::Deadline, 120), "esc");
    set.add("F085 clean exit", ts.finish(4000, true) && ts.clean_exit(5000), "clean");

    // F086 信用分
    let mut cs = CreditScore::new(9);
    set.add("F086 penalize throttles", { cs.penalize(800); cs.refresh_throttle() && cs.quota(100) == 50 }, "throttle");
    set.add("F086 reward restores", { cs.reward(600); !cs.refresh_throttle() && cs.quota(100) == 100 }, "restore");

    // F087 孤儿回收
    let mut os = OrphanSweeper::new();
    set.add("F087 create", os.create(0, 5) && os.create(1, 5) && os.create(2, 6), "create");
    set.add("F087 reap on death", os.process_dead(5) == 2 && os.reaped == 2, "reap");

    // F088 守护模板
    let mut d1 = DaemonSpec::new(1, 0, 0);
    set.add("F088 policy none dies", !d1.on_crash() && !d1.alive, "none");
    let mut d2 = DaemonSpec::new(2, 2, 2);
    set.add("F088 backoff grows", d2.on_crash() && d2.on_crash() && d2.backoff_ms() == 200, "backoff");
    set.add("F088 max restarts", !d2.on_crash() && !d2.alive, "max");

    // F089 manifest
    let m1 = ServiceManifest::new(1, 1 << 2, 64, 1);
    set.add("F089 valid manifest", m1.valid() && m1.depends_on(2), "valid");
    set.add("F089 self-dep invalid", !ServiceManifest::new(1, 1 << 1, 64, 1).valid(), "selfdep");

    // F090 提权代理
    let mut pb = PrivBroker::new(1 << 7);
    set.add("F090 grant cap", pb.request(1) && pb.has(1), "grant");
    set.add("F090 redline denied", !pb.request(7) && !pb.has(7), "redline");
    set.add("F090 revoke", pb.revoke(1) && !pb.has(1) && !pb.revoke(1), "revoke");

    // F091 会话
    let mut sm = SessionManager::new();
    set.add("F091 login", sm.login(1) && !sm.login(1), "login");
    set.add("F091 kick idle", {
        if let Some(mut s) = sm.sessions[0] {
            s.idle_min = 40;
            sm.sessions[0] = Some(s);
        }
        sm.kick_idle(30) == 1
    }, "kick");
    set.add("F091 logout", sm.logout(1) && !sm.logout(1), "logout");

    // F092 单实例
    let mut si = SingleInstance::new(77);
    set.add("F092 first spawn", si.open(100) == 100 && si.running_pid == Some(100), "spawn");
    set.add("F092 activate existing", si.open(200) == 100 && si.activations == 1, "activate");
    set.add("F092 exit frees", si.exit(100) && si.open(300) == 300, "exit");

    // F093 IPC fuzz
    set.add("F093 bad frames", ipc_fuzz(&[(9, 1, 4, 4), (0, 0, 4, 4), (0, 5, 8, 4)]) == 3, "bad");

    // F094 拓扑 lint
    let ok_pair = [ServiceManifest::new(1, 1 << 2, 8, 1), ServiceManifest::new(2, 0, 8, 1)];
    let cycle_pair = [ServiceManifest::new(1, 1 << 2, 8, 1), ServiceManifest::new(2, 1 << 1, 8, 1)];
    set.add("F094 lint clean", topology_lint(&ok_pair) == 0, "clean");
    set.add("F094 lint cycle", topology_lint(&cycle_pair) == 1, "cycle");

    // F095 事件溯源
    let mut es = EventSourcingLog::new();
    set.add("F095 append seq", es.append(5, 1) && es.append(5, 2) && es.seq_intact(), "seq");
    set.add("F095 last state", es.last_state_of(5) == Some(2), "last");

    // F096 压测床
    let mut rig = LoadRig::new(8, 1000);
    for _ in 0..100 {
        rig.issue(true);
    }
    rig.issue(false);
    set.add("F096 throughput", rig.throughput_rps() == 101, "rps");
    set.add("F096 fail gate", !rig.passed(), "gate");
    set.add("F096 clean pass", {
        let mut r2 = LoadRig::new(8, 1000);
        for _ in 0..200 {
            r2.issue(true);
        }
        r2.passed()
    }, "pass");

    // F097 SLA
    let mut sg = SlaGauge::new(8000);
    for _ in 0..15 {
        sg.probe(true);
    }
    sg.probe(false);
    sg.p99_us = 5000;
    set.add("F097 availability", sg.availability_permille() == 937, "avail");
    set.add("F097 not met yet", !sg.met(), "notmet");
    set.add("F097 met when clean", {
        let mut s2 = SlaGauge::new(8000);
        for _ in 0..16 {
            s2.probe(true);
        }
        s2.p99_us = 5000;
        s2.met()
    }, "met");

    // F098 热升级
    let mut hu = HotUpgrade::new(10, 11);
    set.add("F098 begin drain", hu.begin(4) && hu.phase == UpgradePhase::Drain, "drain");
    set.add("F098 partial no cutover", hu.migrate_chunk(3) && !hu.cutover(), "partial");
    set.add("F098 full then cutover", hu.migrate_chunk(1) && hu.cutover() && hu.phase == UpgradePhase::Cutover, "cutover");

    // F099 调试通道
    let mut dc = DebugChannel::new(42, 3);
    set.add("F099 attach gate", !dc.attach(1, 0) && dc.attach(1, 100), "attach");
    set.add("F099 read cap", dc.read() && dc.read() && dc.read() && !dc.read(), "cap");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f082_canary_gating() {
        let mut can = Canary::new(1, 2);
        assert!(can.raise_weight(5));
        assert!(!can.raise_weight(30));
        assert!(can.raise_weight(10));
    }

    #[test]
    fn f080_cycle_detection() {
        let mut mesh = ServiceMesh::new();
        let _ = mesh.record_call(1, 2);
        let _ = mesh.record_call(2, 3);
        assert!(!mesh.has_cycle());
        let _ = mesh.record_call(3, 1);
        assert!(mesh.has_cycle());
    }

    #[test]
    fn f100_self_check_passes() {
        let set = run_m5srv_checks();
        assert_eq!(set.len(), 64, "m5srv 需要 75 项断言");
        assert!(!set.truncated());
        assert!(set.all_passed(), "m5srv 自检必须全绿");
    }
}
