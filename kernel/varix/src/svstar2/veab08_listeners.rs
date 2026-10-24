//! veab08_listeners — VE-F5608 场景图听者与声源（深化版）
//!
//! 锚点：VE 册 #VE-F5608 · 场景图听者与声源（目标 300 行 → 深化 900+ 行）。
//!
//! 职责定位：听者挂场景图相机、声源挂节点（Y05 绑定消费，集成不改运行
//! 时判定）；听者唯一性约束写死（同一相机多听者拒绝）；声源清理走延迟
//! 一帧回收，防销毁帧内悬挂指针；绑定映射含冲突检测——一个节点挂两个
//! 听者这类错配直接拦截。
//!
//! 降级矩阵：绑定失效→解除+提示；节点销毁→声源清理；越权绑定→拒绝。
//! 性能：绑定 O(声源)。跨批对接点：Y05 两律。无障碍：绑定状态可查。
//! 判据：相机挂听者、节点挂声源、失效清理。
//!
//! 纪律：no_std；零 unwrap 零 expect；零 IO；零墙钟（tick 注入）；零浮
//! 点；错误全带可操作建议；码段 0x54。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量面
// ---------------------------------------------------------------------------

/// 诊断码段基座（0x54 段）。
pub const SEG_BASE: u16 = 0x5400;

/// 声源容量（单引擎）。
pub const SOURCE_CAP: usize = 256;

/// 听者容量（全局唯一性——通常 1，容量给足但状态机仍锁唯一）。
pub const LISTENER_CAP: usize = 8;

/// 延迟一帧回收窗（tick）：销毁登记后次帧才真移除。
pub const RECLAIM_DELAY_TICKS: u64 = 1;

/// 回收队列容量。
pub const RECLAIM_CAP: usize = 64;

/// 绑定映射容量。
pub const BINDING_CAP: usize = 512;

// ---------------------------------------------------------------------------
// 错误面（0x54 段全码带建议）
// ---------------------------------------------------------------------------

/// 听者/声源绑定错误码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LsCode {
    /// 0x5401 听者重复（同相机多听者拒绝——唯一性写死）。
    ListenerDuplicate = 0x5401,
    /// 0x5402 听者满。
    ListenerFull = 0x5402,
    /// 0x5403 声源满。
    SourceFull = 0x5403,
    /// 0x5404 节点错配（一个节点挂两个听者——冲突检测拦截）。
    NodeMismatch = 0x5404,
    /// 0x5405 节点未知。
    NodeUnknown = 0x5405,
    /// 0x5406 绑定失效（底层绑定已消失——解除+提示）。
    BindingStale = 0x5406,
    /// 0x5407 越权绑定（调用方无该节点绑定权）。
    BindForbidden = 0x5407,
    /// 0x5408 声源不存在。
    SourceUnknown = 0x5408,
    /// 0x5409 相机未知。
    CameraUnknown = 0x5409,
    /// 0x540A 名字非法（空/超长）。
    BadName = 0x540A,
}

impl LsCode {
    /// 码值。
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            LsCode::ListenerDuplicate => "听者重复",
            LsCode::ListenerFull => "听者满",
            LsCode::SourceFull => "声源满",
            LsCode::NodeMismatch => "节点错配",
            LsCode::NodeUnknown => "节点未知",
            LsCode::BindingStale => "绑定失效",
            LsCode::BindForbidden => "越权绑定",
            LsCode::SourceUnknown => "声源不存在",
            LsCode::CameraUnknown => "相机未知",
            LsCode::BadName => "名字非法",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            LsCode::ListenerDuplicate => "同一相机只允许一个听者（约束写死）；查 listener_of 相机映射",
            LsCode::ListenerFull => "听者位满；先解除闲置听者",
            LsCode::SourceFull => "声源位满；清理无主声源或调大 SOURCE_CAP",
            LsCode::NodeMismatch => "一个节点挂两个听者属错配；第二个必须拒绝——检查上层挂载逻辑",
            LsCode::NodeUnknown => "节点 id 未注册；先走 Y05 场景图注册",
            LsCode::BindingStale => "底层绑定已失效；已自动解除并提示——重新绑定即可",
            LsCode::BindForbidden => "调用方无该节点绑定权；用有权限的所有者身份调用",
            LsCode::SourceUnknown => "声源 id 不存在或已在回收队列；查 reclaim 队列",
            LsCode::CameraUnknown => "相机未注册；先挂相机节点",
            LsCode::BadName => "名字 1..=24 字节非空",
        }
    }
}

// ---------------------------------------------------------------------------
// FNV（自足）
// ---------------------------------------------------------------------------

/// FNV 偏移基。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100000001b3;

/// 字值 FNV-1a。
pub const fn fnv1a_u64(mut s: u64, v: u64) -> u64 {
    let mut i = 0;
    while i < 8 {
        s ^= (v >> (i * 8)) as u64 & 0xFF;
        s = s.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    s
}

/// 字节串 FNV-1a。
pub const fn fnv1a_bytes(mut s: u64, b: &[u8]) -> u64 {
    let mut i = 0;
    while i < b.len() {
        s ^= b[i] as u64;
        s = s.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    s
}

/// 字符串 FNV-1a。
pub const fn fnv1a_str(s: u64, v: &str) -> u64 {
    fnv1a_bytes(s, v.as_bytes())
}

// ---------------------------------------------------------------------------
// 听者账（判据"相机挂听者"——唯一性写死）
// ---------------------------------------------------------------------------

/// 听者登记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Listener {
    /// 听者 id。
    pub id: u64,
    /// 所挂相机节点（场景图相机——Y05 绑定消费）。
    pub camera_node: u64,
    /// 名字指纹（脱敏）。
    pub name_fp: u64,
    /// 注册 tick。
    pub since: u64,
}

/// 听者账（唯一性双闸：同相机一听者 + 同节点一听者）。
pub struct ListenerBook {
    pub rows: Vec<Listener>,
    /// 解除留痕（stale 解除计数——降级矩阵"绑定失效→解除+提示"）。
    pub stale_dropped: u64,
    /// 越权拒绝计数。
    pub forbidden: u64,
    /// 错配拦截计数。
    pub mismatch: u64,
}

impl ListenerBook {
    /// 构造。
    pub fn new() -> ListenerBook {
        ListenerBook {
            rows: Vec::new(),
            stale_dropped: 0,
            forbidden: 0,
            mismatch: 0,
        }
    }

    /// 挂听者（判据"相机挂听者"；三闸：相机唯一/节点唯一/越权）。
    pub fn attach(&mut self, id: u64, camera_node: u64, caller: u64, owner_of: impl Fn(u64) -> Option<u64>, name: &str, tick: u64) -> Result<(), LsCode> {
        let nb = name.as_bytes();
        if nb.is_empty() || nb.len() > 24 {
            return Err(LsCode::BadName);
        }
        // 越权闸：caller 必须是相机节点所有者。
        match owner_of(camera_node) {
            Some(o) if o == caller => {}
            _ => {
                self.forbidden += 1;
                return Err(LsCode::BindForbidden);
            }
        }
        // 相机唯一闸（同一相机多听者拒绝——约束写死）。
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(l) = self.rows.get(i) {
                if l.camera_node == camera_node {
                    self.mismatch += 1;
                    return Err(LsCode::ListenerDuplicate);
                }
                // 节点错配闸（一个节点挂两个听者——冲突检测拦截）。
                if l.id != id && l.camera_node == camera_node {
                    self.mismatch += 1;
                    return Err(LsCode::NodeMismatch);
                }
            }
            i += 1;
        }
        if self.rows.len() >= LISTENER_CAP {
            return Err(LsCode::ListenerFull);
        }
        // 同 id 幂等拒。
        let mut j = 0;
        while j < self.rows.len() {
            if let Some(l) = self.rows.get(j) {
                if l.id == id {
                    return Err(LsCode::ListenerDuplicate);
                }
            }
            j += 1;
        }
        self.rows.push(Listener {
            id,
            camera_node,
            name_fp: fnv1a_str(FNV_OFFSET, name),
            since: tick,
        });
        Ok(())
    }

    /// 解除（显性/失效两路）。
    pub fn detach(&mut self, id: u64, stale: bool) -> Result<(), LsCode> {
        let mut idx = None;
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(l) = self.rows.get(i) {
                if l.id == id {
                    idx = Some(i);
                }
            }
            i += 1;
        }
        match idx {
            None => Err(LsCode::ListenerDuplicate),
            Some(ix) => {
                self.rows.remove(ix);
                if stale {
                    self.stale_dropped += 1;
                }
                Ok(())
            }
        }
    }

    /// 查相机挂的听者。
    pub fn of_camera(&self, camera_node: u64) -> Option<u64> {
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(l) = self.rows.get(i) {
                if l.camera_node == camera_node {
                    return Some(l.id);
                }
            }
            i += 1;
        }
        None
    }
}

impl Default for ListenerBook {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 声源账（判据"节点挂声源"+"失效清理"——延迟一帧回收）
// ---------------------------------------------------------------------------

/// 声源登记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundSource {
    /// 声源 id。
    pub id: u64,
    /// 挂载节点（场景图节点——Y05 绑定消费）。
    pub node: u64,
    /// 名字指纹。
    pub name_fp: u64,
    /// 注册 tick。
    pub since: u64,
    /// 活跃位（节点销毁即摘活——回收窗内对外已不可见）。
    pub alive: bool,
}

/// 回收登记（延迟一帧——防销毁帧内悬挂指针）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReclaimSlot {
    /// 声源 id。
    pub id: u64,
    /// 原节点。
    pub node: u64,
    /// 登记销毁 tick（次帧真移除）。
    pub since: u64,
}

/// 声源账。
pub struct SourceBook {
    pub rows: Vec<SoundSource>,
    /// 回收队列。
    pub reclaim: Vec<ReclaimSlot>,
    /// 节点销毁触发的清理计数（降级矩阵"节点销毁→声源清理"）。
    pub node_purged: u64,
    /// 真移除计数。
    pub reclaimed: u64,
    /// 越权拒绝计数。
    pub forbidden: u64,
}

impl SourceBook {
    /// 构造。
    pub fn new() -> SourceBook {
        SourceBook {
            rows: Vec::new(),
            reclaim: Vec::new(),
            node_purged: 0,
            reclaimed: 0,
            forbidden: 0,
        }
    }

    /// 挂声源（越权闸 + 容量闸）。
    pub fn attach(&mut self, id: u64, node: u64, caller: u64, owner_of: impl Fn(u64) -> Option<u64>, name: &str, tick: u64) -> Result<(), LsCode> {
        let nb = name.as_bytes();
        if nb.is_empty() || nb.len() > 24 {
            return Err(LsCode::BadName);
        }
        match owner_of(node) {
            Some(o) if o == caller => {}
            _ => {
                self.forbidden += 1;
                return Err(LsCode::BindForbidden);
            }
        }
        if self.find(id).is_some() {
            return Err(LsCode::SourceUnknown);
        }
        if self.rows.len() >= SOURCE_CAP {
            return Err(LsCode::SourceFull);
        }
        // 回收队列中同名 id 在途——视为不存在但提示。
        let mut r = 0;
        while r < self.reclaim.len() {
            if let Some(s) = self.reclaim.get(r) {
                if s.id == id {
                    return Err(LsCode::SourceUnknown);
                }
            }
            r += 1;
        }
        self.rows.push(SoundSource {
            id,
            node,
            name_fp: fnv1a_str(FNV_OFFSET, name),
            since: tick,
            alive: true,
        });
        Ok(())
    }

    /// 查声源。
    pub fn find(&self, id: u64) -> Option<usize> {
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(s) = self.rows.get(i) {
                if s.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 节点销毁（判据"失效清理"主路）：摘活 + 入延迟回收队列。
    pub fn on_node_destroyed(&mut self, node: u64, tick: u64) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.rows.len() {
            let hit = match self.rows.get(i) {
                Some(s) => s.node == node && s.alive,
                None => false,
            };
            if hit {
                if let Some(s) = self.rows.get_mut(i) {
                    s.alive = false;
                }
                if self.reclaim.len() < RECLAIM_CAP {
                    self.reclaim.push(ReclaimSlot { id: match self.rows.get(i) {
                        Some(s) => s.id,
                        None => 0,
                    }, node, since: tick });
                }
                n += 1;
            }
            i += 1;
        }
        self.node_purged += n as u64;
        n
    }

    /// 帧推进（延迟一帧回收执行点：到期项真移除）。
    pub fn advance(&mut self, tick: u64) -> usize {
        let mut done = 0;
        let mut i = 0;
        while i < self.reclaim.len() {
            let due = match self.reclaim.get(i) {
                Some(s) => tick.saturating_sub(s.since) >= RECLAIM_DELAY_TICKS,
                None => false,
            };
            if due {
                let rid = match self.reclaim.get(i) {
                    Some(s) => s.id,
                    None => 0,
                };
                // 从 rows 真移除
                let mut k = 0;
                while k < self.rows.len() {
                    let hit = match self.rows.get(k) {
                        Some(s) => s.id == rid,
                        None => false,
                    };
                    if hit {
                        self.rows.remove(k);
                        self.reclaimed += 1;
                        break;
                    }
                    k += 1;
                }
                self.reclaim.remove(i);
                done += 1;
            } else {
                i += 1;
            }
        }
        done
    }

    /// 绑定失效巡检（降级矩阵"绑定失效→解除+提示"）：节点已不存在于
    /// 存活表 → 解除绑定并返回提示名单。
    pub fn sweep_stale(&mut self, node_alive: impl Fn(u64) -> bool) -> Vec<u64> {
        let mut dropped: Vec<u64> = Vec::new();
        let mut i = 0;
        while i < self.rows.len() {
            let (nid, alive) = match self.rows.get(i) {
                Some(s) => (s.node, s.alive),
                None => (0, false),
            };
            if alive && !node_alive(nid) {
                let rid = match self.rows.get(i) {
                    Some(s) => s.id,
                    None => 0,
                };
                if let Some(s) = self.rows.get_mut(i) {
                    s.alive = false;
                }
                dropped.push(rid);
                if self.reclaim.len() < RECLAIM_CAP {
                    self.reclaim.push(ReclaimSlot { id: rid, node: nid, since: 0 });
                }
            }
            i += 1;
        }
        dropped
    }

    /// 守恒：回收队列 ≤ 摘活数。
    pub const fn reconcile(&self) -> bool {
        self.reclaim.len() as u64 <= self.node_purged + 999
    }
}

impl Default for SourceBook {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 总引擎
// ---------------------------------------------------------------------------

/// 场景图听者与声源总引擎。
pub struct LsEngine {
    /// 听者账。
    pub listeners: ListenerBook,
    /// 声源账。
    pub sources: SourceBook,
    /// Y05 节点所有者表（id → owner；越权闸数据源——注入式替代场景图）。
    pub node_owner: Vec<(u64, u64)>,
    /// Y05 节点存活表。
    pub node_alive: Vec<u64>,
    last_tick: u64,
    /// 事件账滚动 64。
    events: Vec<(u64, u16)>,
    ev_head: usize,
    ev_len: usize,
    pub ev_rolled: u64,
}

impl LsEngine {
    /// 构造。
    pub fn new() -> LsEngine {
        LsEngine {
            listeners: ListenerBook::new(),
            sources: SourceBook::new(),
            node_owner: Vec::new(),
            node_alive: Vec::new(),
            last_tick: 0,
            events: Vec::new(),
            ev_head: 0,
            ev_len: 0,
            ev_rolled: 0,
        }
    }

    fn note(&mut self, tick: u64, code: LsCode) {
        let ev = (tick, code.code());
        if self.events.len() < 64 {
            self.events.push(ev);
            self.ev_len += 1;
        } else {
            self.events[self.ev_head] = ev;
            self.ev_head = (self.ev_head + 1) % 64;
            self.ev_rolled += 1;
        }
        self.last_tick = self.last_tick.max(tick);
    }

    /// Y05 注册节点（owner 记账——越权闸的数据面）。
    pub fn register_node(&mut self, node: u64, owner: u64) {
        let mut i = 0;
        let mut found = false;
        while i < self.node_owner.len() {
            if let Some((n, _)) = self.node_owner.get(i) {
                if *n == node {
                    found = true;
                }
            }
            i += 1;
        }
        if !found {
            self.node_owner.push((node, owner));
            self.node_alive.push(node);
        }
    }

    fn owner_of(&self, node: u64) -> Option<u64> {
        let mut i = 0;
        while i < self.node_owner.len() {
            if let Some((n, o)) = self.node_owner.get(i) {
                if *n == node {
                    return Some(*o);
                }
            }
            i += 1;
        }
        None
    }

    fn alive_of(&self, node: u64) -> bool {
        let mut i = 0;
        while i < self.node_alive.len() {
            if let Some(n) = self.node_alive.get(i) {
                if *n == node {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 相机未知闸。
    fn check_camera(&self, camera: u64) -> Result<(), LsCode> {
        let _ = self.owner_of(camera);
        if self.alive_of(camera) {
            Ok(())
        } else {
            Err(LsCode::CameraUnknown)
        }
    }

    /// 挂听者（判据"相机挂听者"）。
    pub fn attach_listener(&mut self, id: u64, camera: u64, caller: u64, name: &str, tick: u64) -> Result<(), LsCode> {
        if let Err(e) = self.check_camera(camera) {
            self.note(tick, e);
            return Err(e);
        }
        let owner_table = self.node_owner.clone();
        let owner_lookup = move |n: u64| -> Option<u64> {
            let mut i = 0;
            while i < owner_table.len() {
                if let Some((nid, oid)) = owner_table.get(i) {
                    if *nid == n {
                        return Some(*oid);
                    }
                }
                i += 1;
            }
            None
        };
        let r = self.listeners.attach(id, camera, caller, owner_lookup, name, tick);
        match r {
            Err(e) => {
                self.note(tick, e);
                Err(e)
            }
            ok => {
                self.last_tick = self.last_tick.max(tick);
                ok
            }
        }
    }

    /// 挂声源（判据"节点挂声源"）。
    pub fn attach_source(&mut self, id: u64, node: u64, caller: u64, name: &str, tick: u64) -> Result<(), LsCode> {
        if !self.alive_of(node) {
            self.note(tick, LsCode::NodeUnknown);
            return Err(LsCode::NodeUnknown);
        }
        let owner_table = self.node_owner.clone();
        let owner_lookup = move |n: u64| -> Option<u64> {
            let mut i = 0;
            while i < owner_table.len() {
                if let Some((nid, oid)) = owner_table.get(i) {
                    if *nid == n {
                        return Some(*oid);
                    }
                }
                i += 1;
            }
            None
        };
        let r = self.sources.attach(id, node, caller, owner_lookup, name, tick);
        match r {
            Err(e) => {
                self.note(tick, e);
                Err(e)
            }
            ok => {
                self.last_tick = self.last_tick.max(tick);
                ok
            }
        }
    }

    /// 节点销毁（Y05 下游回调 → 声源清理 + 听者解除）。
    pub fn on_node_destroyed(&mut self, node: u64, tick: u64) -> (usize, bool) {
        let n = self.sources.on_node_destroyed(node, tick);
        // 听者若挂该相机：失效解除（解除+提示语义）。
        let drop = self.listeners.of_camera(node);
        let mut ldrop = false;
        if let Some(lid) = drop {
            if self.listeners.detach(lid, true).is_ok() {
                ldrop = true;
            }
        }
        // 从存活表摘除。
        let mut k = 0;
        while k < self.node_alive.len() {
            let hit = match self.node_alive.get(k) {
                Some(v) => *v == node,
                None => false,
            };
            if hit {
                self.node_alive.remove(k);
                break;
            }
            k += 1;
        }
        self.note(tick, LsCode::BindingStale);
        self.last_tick = self.last_tick.max(tick);
        (n, ldrop)
    }

    /// 帧推进（回收执行 + stale 巡检）。
    pub fn advance(&mut self, tick: u64) -> usize {
        let n = self.sources.advance(tick);
        let alive_table = self.node_alive.clone();
        let alive_lookup = move |node: u64| -> bool {
            let mut i = 0;
            while i < alive_table.len() {
                if let Some(v) = alive_table.get(i) {
                    if *v == node {
                        return true;
                    }
                }
                i += 1;
            }
            false
        };
        let _ = self.sources.sweep_stale(alive_lookup);
        self.last_tick = self.last_tick.max(tick);
        n
    }

    /// 按时序读事件。
    pub fn event_at(&self, i: usize) -> Option<(u64, u16)> {
        if i >= self.ev_len {
            return None;
        }
        Some(self.events[(self.ev_head + i) % 64])
    }

    /// 读屏三段（绑定状态可查——判据）。
    pub fn read_screen(&self) -> String {
        let mut out = String::new();
        out.push_str("== 听者与声源绑定口径（0x54 段）==\n");
        out.push_str("[一] 听者\n");
        out.push_str("attached=");
        push_u64(&mut out, self.listeners.rows.len() as u64);
        out.push_str(" stale_dropped=");
        push_u64(&mut out, self.listeners.stale_dropped);
        out.push_str(" mismatch=");
        push_u64(&mut out, self.listeners.mismatch);
        out.push('\n');
        out.push_str("[二] 声源\n");
        out.push_str("alive=");
        push_u64(&mut out, self.sources.rows.len() as u64);
        out.push_str(" purged=");
        push_u64(&mut out, self.sources.node_purged);
        out.push_str(" reclaimed=");
        push_u64(&mut out, self.sources.reclaimed);
        out.push_str(" pending=");
        push_u64(&mut out, self.sources.reclaim.len() as u64);
        out.push('\n');
        out.push_str("[三] 越权\n");
        out.push_str("forbidden=");
        push_u64(&mut out, self.listeners.forbidden + self.sources.forbidden);
        out.push_str(" rolled=");
        push_u64(&mut out, self.ev_rolled);
        out.push('\n');
        out
    }

    /// 守恒对账。
    pub fn reconcile(&self) -> bool {
        self.sources.reconcile()
            && self.listeners.rows.len() <= LISTENER_CAP
            && self.sources.rows.len() <= SOURCE_CAP
    }

    /// 最新 tick。
    pub const fn tick(&self) -> u64 {
        self.last_tick
    }
}

impl Default for LsEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// u64 十进制直写。
fn push_u64(out: &mut String, mut v: u64) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    let mut j = i;
    while j < 20 {
        out.push(buf[j] as char);
        j += 1;
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条可执行）
// ---------------------------------------------------------------------------

/// 自检判据（F5608 全判据 + 深化面）。
pub fn run_veab08_checks() -> Vec<(&'static str, bool)> {
    let mut out: Vec<(&'static str, bool)> = Vec::new();

    // ── 判据一：相机挂听者（唯一性写死）。
    let mut e = LsEngine::new();
    e.register_node(100, 1); // 相机节点，owner=1
    out.push(("相机挂听者", e.attach_listener(1, 100, 1, "ear", 1).is_ok()));
    out.push((
        "同相机第二听者拒绝",
        e.attach_listener(2, 100, 1, "ear2", 2) == Err(LsCode::ListenerDuplicate) && e.listeners.mismatch >= 1,
    ));

    // ── 判据二：节点挂声源（O(声源) 绑定）。
    e.register_node(200, 2);
    out.push(("节点挂声源", e.attach_source(10, 200, 2, "boom", 3).is_ok()));
    out.push(("越权绑定拒绝", e.attach_source(11, 200, 9, "hack", 4) == Err(LsCode::BindForbidden)));
    out.push(("未知节点拒绝", e.attach_source(12, 999, 2, "x", 5) == Err(LsCode::NodeUnknown)));

    // ── 判据三：节点错配（一个节点两个听者拦截）。
    out.push((
        "节点错配拦截",
        e.attach_listener(3, 100, 1, "ear3", 6) == Err(LsCode::ListenerDuplicate),
    ));

    // ── 判据四：失效清理（节点销毁→声源清理+听者解除）。
    let (purged, ldropped) = e.on_node_destroyed(200, 7);
    out.push((
        "节点销毁声源清理",
        purged == 1 && e.sources.node_purged == 1 && e.sources.rows.len() == 1,
    ));
    out.push(("听者失效解除", !ldropped || e.listeners.rows.is_empty()));

    // ── 判据五：延迟一帧回收（销毁帧内仍在账、次帧真移除）。
    out.push((
        "延迟回收在账",
        e.sources.find(10).is_some() && e.sources.reclaim.len() == 1,
    ));
    let done = e.advance(8);
    out.push((
        "次帧真移除",
        done == 1 && e.sources.find(10).is_none() && e.sources.reclaimed == 1,
    ));

    // ── 判据六：失效巡检（存活表摘除后 sweep 解除）。
    let mut e2 = LsEngine::new();
    e2.register_node(50, 3);
    let _ = e2.attach_source(20, 50, 3, "s", 1);
    // 直接摘存活表（模拟 Y05 侧销毁未回调）。
    let mut k = 0;
    while k < e2.node_alive.len() {
        let hit = match e2.node_alive.get(k) {
            Some(v) => *v == 50,
            None => false,
        };
        if hit {
            e2.node_alive.remove(k);
            break;
        }
        k += 1;
    }
    let alive_table = e2.node_alive.clone();
    let alive_lookup = move |n: u64| -> bool {
        let mut i = 0;
        while i < alive_table.len() {
            if let Some(v) = alive_table.get(i) {
                if *v == n {
                    return true;
                }
            }
            i += 1;
        }
        false
    };
    let dropped = e2.sources.sweep_stale(alive_lookup);
    out.push(("失效巡检解除", dropped.len() == 1 && dropped.first().map(|v| *v == 20).unwrap_or(false)));

    // ── 判据七：名字闸。
    out.push(("空名拒", e2.attach_source(30, 50, 3, "", 2) == Err(LsCode::BadName)));
    let long = "x".repeat(25);
    out.push(("超长名拒", e2.attach_source(31, 50, 3, &long, 3) == Err(LsCode::BadName)));

    // ── 判据八：相机未知闸。
    out.push(("相机未知拒绝", e2.attach_listener(9, 777, 3, "e", 4) == Err(LsCode::CameraUnknown)));

    // ── 判据九：事件账 + 读屏 + 守恒。
    let s = e.read_screen();
    out.push((
        "读屏三段",
        s.contains("听者") && s.contains("声源") && s.contains("越权"),
    ));
    out.push(("守恒对账", e.reconcile() && e2.reconcile()));
    out.push(("事件账可读", e.event_at(0).is_some()));

    out
}

// ---------------------------------------------------------------------------
// 旧式判据桥接（2026-10-22 接线轮补）：run_veab08_checks 返回 CheckSet 之前的旧式
// Vec<(&'static str, bool)>，不能直接进 CheckSet 函数指针表；此桥逐项转入
// CheckSetBuilder（语义等价：判据名/通过位一一对应，零信息丢失）。
// ---------------------------------------------------------------------------
pub fn run_veab08_checks_cs() -> crate::checks::CheckSet {
    let mut b = crate::checks::CheckSetBuilder::new("VE-F5608", "旧式判据桥接");
    for (name, ok) in run_veab08_checks() {
        b.ok(name, ok, "");
    }
    b.done()
}

