//! UNX-F50050 水印埋点（合成 / 变量 / 层序三点）（0x43260 段）
//!
//! 锚点：UNX-F50050-J1 埋点 100% 覆盖；判据锚定 体验十三·补。
//!
//! 水印链路要可观测，但埋点不能靠「日志里有没有看到」来判断——那既不可
//! 复算也不可入账。本模块把埋点收敛成**三点闭集**：
//!
//! * `Composite`：合成阶段（水印与底图的叠加点）；
//! * `Variable`：变量替换阶段（可变数据落值点，记录落值快照）；
//! * `LayerOrder`：层序阶段（水印插入层的顺序判定点）。
//!
//! 覆盖率的定义：**三点中至少有一次 Emit 的点数 / 3**，千分比取整。只
//! `Begin`/`End` 不算覆盖——空 span 等于没埋。这样「100% 覆盖」是可对平的
//! 度量，而不是自我声明的口号。
//!
//! 事件流进定容环形缓冲（48 条），满则 `CapFull` 并显性记账 `dropped`，
//! 绝不静默丢事件。span 用 LIFO 栈管理：Begin/End 必须成对、End 必须闭合
//! 栈顶，层号在 span 内只许不降（层序埋点若回退说明合成顺序错了）。
//!
//! 数据段 `SEG_BASE = 0x43260`，诊断码闭集 `0x43261..=0x43269`（9 条）。
//! 结构清单：`TracePoint`/`Phase`（闭集）、`TraceEvent`（事件）、
//! `TraceSink`（定容缓冲 + 计数）、`Tracer`（span 栈状态机）、
//! `TraceRegistry`（key 注册表）、`injection_core`（9 类坏样本注入核）。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 诊断段基址（0x43260）。
pub const SEG_BASE: u32 = 0x43260;

pub const E_CLOSED: u16 = 1;
pub const E_CAP: u16 = 2;
pub const E_UNKNOWN: u16 = 3;
pub const E_UNBALANCED: u16 = 4;
pub const E_ORDER: u16 = 5;
pub const E_EMPTY_KEY: u16 = 6;
pub const E_DUP_KEY: u16 = 7;
pub const E_LAYER: u16 = 8;
pub const E_PARAM: u16 = 9;

/// 埋点域错误。`code()` 落在 `SEG_BASE + n` 闭集内。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceError {
    /// 已封账（seal 之后）仍写入
    Closed,
    /// 事件缓冲或 span 栈容量满
    CapFull,
    /// 埋点码不在三点闭集内
    UnknownPoint,
    /// 无 span 时 Emit/Close，或封账时仍有未闭合 span
    Unbalanced,
    /// Close 的 span 不是栈顶（跨层闭合）
    Order,
    /// 空 key（匿名埋点无法对账）
    EmptyKey,
    /// key 重名（同一 key 两处埋点会让覆盖统计失真）
    DupKey,
    /// 层号回退（层序埋点必须单调）
    LayerRegress,
    /// 变量值越界（|var| > 1_000_000 视为采集错误）
    Param,
}

impl TraceError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            TraceError::Closed => E_CLOSED,
            TraceError::CapFull => E_CAP,
            TraceError::UnknownPoint => E_UNKNOWN,
            TraceError::Unbalanced => E_UNBALANCED,
            TraceError::Order => E_ORDER,
            TraceError::EmptyKey => E_EMPTY_KEY,
            TraceError::DupKey => E_DUP_KEY,
            TraceError::LayerRegress => E_LAYER,
            TraceError::Param => E_PARAM,
        };
        SEG_BASE + n as u32
    }

    /// 一句话诊断说明。
    pub fn message(self) -> &'static str {
        match self {
            TraceError::Closed => "tracer sealed",
            TraceError::CapFull => "trace buffer or span stack full",
            TraceError::UnknownPoint => "trace point code invalid",
            TraceError::Unbalanced => "span begin/end unbalanced",
            TraceError::Order => "span closed out of order",
            TraceError::EmptyKey => "empty trace key",
            TraceError::DupKey => "duplicate trace key",
            TraceError::LayerRegress => "layer order regressed",
            TraceError::Param => "trace variable out of range",
        }
    }
}

/// 三点闭集：合成 / 变量 / 层序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TracePoint {
    Composite,
    Variable,
    LayerOrder,
}

impl TracePoint {
    /// 三点全表（覆盖统计的分母）。
    pub const ALL: [TracePoint; 3] = [
        TracePoint::Composite,
        TracePoint::Variable,
        TracePoint::LayerOrder,
    ];

    /// 埋点码闭集：0/1/2，其余拒。
    pub fn from_u8(v: u8) -> Result<TracePoint, TraceError> {
        match v {
            0 => Ok(TracePoint::Composite),
            1 => Ok(TracePoint::Variable),
            2 => Ok(TracePoint::LayerOrder),
            _ => Err(TraceError::UnknownPoint),
        }
    }

    /// 埋点码。
    pub fn as_u8(self) -> u8 {
        match self {
            TracePoint::Composite => 0,
            TracePoint::Variable => 1,
            TracePoint::LayerOrder => 2,
        }
    }

    /// 数组下标（0..3）。
    pub fn index(self) -> usize {
        self.as_u8() as usize
    }

    /// 稳定短名（诊断/日志用，不随语言变化）。
    pub fn name(self) -> &'static str {
        match self {
            TracePoint::Composite => "wm.composite",
            TracePoint::Variable => "wm.variable",
            TracePoint::LayerOrder => "wm.layer",
        }
    }
}

/// 事件相位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Begin,
    Emit,
    End,
}

/// 一条埋点事件。`seq` 全局严格递增，用于对账事件是否有空缺/重排。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceEvent {
    pub seq: u32,
    pub point: TracePoint,
    pub phase: Phase,
    pub span: u16,
    pub layer: u8,
    pub var: i32,
}

/// 事件缓冲容量（48 条足够覆盖一次三阶段合成；超出即显性记 drop）。
pub const TRACE_CAP: usize = 48;

/// span 栈深度上限（水印合成最多三层嵌套）。
pub const STACK_CAP: usize = 8;

/// 定容事件缓冲 + 每点命中计数。
pub struct TraceSink {
    events: [Option<TraceEvent>; TRACE_CAP],
    len: usize,
    dropped: u16,
    next_seq: u32,
    hits: [u32; 3],
}

impl TraceSink {
    /// 空缓冲。
    pub const fn new() -> TraceSink {
        TraceSink {
            events: [None; TRACE_CAP],
            len: 0,
            dropped: 0,
            next_seq: 0,
            hits: [0; 3],
        }
    }

    /// 追加一条事件；缓冲满 → `CapFull` 并累加 `dropped`。
    pub fn push(&mut self, point: TracePoint, phase: Phase, span: u16, layer: u8, var: i32) -> Result<(), TraceError> {
        if self.len >= TRACE_CAP {
            self.dropped += 1;
            return Err(TraceError::CapFull);
        }
        let ev = TraceEvent {
            seq: self.next_seq,
            point,
            phase,
            span,
            layer,
            var,
        };
        if let Some(slot) = self.events.get_mut(self.len) {
            *slot = Some(ev);
            self.len += 1;
        }
        self.next_seq = self.next_seq.wrapping_add(1);
        if phase == Phase::Emit {
            let idx = point.index();
            if let Some(h) = self.hits.get_mut(idx) {
                *h = h.saturating_add(1);
            }
        }
        Ok(())
    }

    /// 事件条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 丢弃条数（显性记账）。
    pub fn dropped(&self) -> u16 {
        self.dropped
    }

    /// 某点的 Emit 命中次数。
    pub fn hits(&self, p: TracePoint) -> u32 {
        match self.hits.get(p.index()) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 取第 i 条事件。
    pub fn event(&self, i: usize) -> Option<TraceEvent> {
        self.events.get(i).and_then(|v| *v)
    }
}

/// span 栈帧：一个 span 的 id / 埋点 / 起始层号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SpanFrame {
    id: u16,
    point: TracePoint,
    layer: u8,
}

/// 埋点器：span 栈 + 事件缓冲 + 封账标志。
pub struct Tracer {
    sink: TraceSink,
    stack: [Option<SpanFrame>; STACK_CAP],
    depth: usize,
    next_span: u16,
    closed: bool,
}

impl Tracer {
    /// 新建埋点器。
    pub const fn new() -> Tracer {
        Tracer {
            sink: TraceSink::new(),
            stack: [None; STACK_CAP],
            depth: 0,
            next_span: 0,
            closed: false,
        }
    }

    /// 开启一个 span；key 不能为空，嵌套 span 的层号不得低于父层。
    pub fn open(&mut self, key: &'static str, point: TracePoint, layer: u8) -> Result<u16, TraceError> {
        if self.closed {
            return Err(TraceError::Closed);
        }
        if key.is_empty() {
            return Err(TraceError::EmptyKey);
        }
        if self.depth >= STACK_CAP {
            return Err(TraceError::CapFull);
        }
        if self.depth > 0 {
            if let Some(parent) = self.stack.get(self.depth - 1).and_then(|v| *v) {
                if layer < parent.layer {
                    return Err(TraceError::LayerRegress);
                }
            }
        }
        let id = self.next_span;
        // 先落 Begin 事件：缓冲满时直接失败，栈保持不变（不留下「有 span 无事件」）。
        self.sink.push(point, Phase::Begin, id, layer, 0)?;
        let frame = SpanFrame { id, point, layer };
        if let Some(slot) = self.stack.get_mut(self.depth) {
            *slot = Some(frame);
            self.depth += 1;
        }
        self.next_span = self.next_span.wrapping_add(1);
        Ok(id)
    }

    /// 在当前 span 内打点；层号低于当前帧即 `LayerRegress`，变量绝对值越界即 `Param`。
    pub fn emit(&mut self, point: TracePoint, layer: u8, var: i32) -> Result<(), TraceError> {
        if self.closed {
            return Err(TraceError::Closed);
        }
        if self.depth == 0 {
            return Err(TraceError::Unbalanced);
        }
        if var > 1_000_000 || var < -1_000_000 {
            return Err(TraceError::Param);
        }
        let frame = match self.stack.get(self.depth - 1).and_then(|v| *v) {
            Some(f) => f,
            None => return Err(TraceError::Unbalanced),
        };
        if layer < frame.layer {
            return Err(TraceError::LayerRegress);
        }
        self.sink.push(point, Phase::Emit, frame.id, layer, var)
    }

    /// 闭合栈顶 span；跨层闭合（span 不是栈顶）即 `Order`。
    pub fn close(&mut self, span: u16) -> Result<(), TraceError> {
        if self.closed {
            return Err(TraceError::Closed);
        }
        if self.depth == 0 {
            return Err(TraceError::Unbalanced);
        }
        let frame = match self.stack.get(self.depth - 1).and_then(|v| *v) {
            Some(f) => f,
            None => return Err(TraceError::Unbalanced),
        };
        if frame.id != span {
            return Err(TraceError::Order);
        }
        self.sink.push(frame.point, Phase::End, frame.id, frame.layer, 0)?;
        self.depth -= 1;
        Ok(())
    }

    /// 封账：仍有未闭合 span → `Unbalanced`；封账后任何写入 → `Closed`。
    pub fn seal(&mut self) -> Result<(), TraceError> {
        if self.closed {
            return Ok(());
        }
        if self.depth != 0 {
            return Err(TraceError::Unbalanced);
        }
        self.closed = true;
        Ok(())
    }

    /// 是否已封账。
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 当前 span 栈深度。
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// 事件条数。
    pub fn event_count(&self) -> usize {
        self.sink.len()
    }

    /// 取第 i 条事件。
    pub fn event(&self, i: usize) -> Option<TraceEvent> {
        self.sink.event(i)
    }

    /// 丢弃条数。
    pub fn dropped(&self) -> u16 {
        self.sink.dropped()
    }

    /// 某点是否被真正埋到（至少一次 Emit）。
    pub fn covered(&self, p: TracePoint) -> bool {
        self.sink.hits(p) > 0
    }

    /// 覆盖率千分比：命中点数 / 3 × 1000。
    pub fn coverage_permille(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TracePoint::ALL.len() {
            if let Some(p) = TracePoint::ALL.get(i) {
                if self.sink.hits(*p) > 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n * 1000 / 3
    }

    /// 事件流指纹（确定性对平用）。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.event_count() {
            if let Some(e) = self.event(i) {
                h = (h ^ e.seq as u64).wrapping_mul(0x0000_0100_0000_01b3);
                h = (h ^ e.point.as_u8() as u64).wrapping_mul(0x0000_0100_0000_01b3);
                h = (h ^ match e.phase {
                    Phase::Begin => 1,
                    Phase::Emit => 2,
                    Phase::End => 3,
                })
                .wrapping_mul(0x0000_0100_0000_01b3);
                h = (h ^ e.span as u64).wrapping_mul(0x0000_0100_0000_01b3);
                h = (h ^ e.layer as u64).wrapping_mul(0x0000_0100_0000_01b3);
                h = (h ^ e.var as u32 as u64).wrapping_mul(0x0000_0100_0000_01b3);
            }
            i += 1;
        }
        h
    }

    /// 事件摘要（日志用）。
    pub fn summary(&self) -> String {
        format!(
            "events={} coverage={}permille dropped={}",
            self.event_count(),
            self.coverage_permille(),
            self.dropped()
        )
    }
}

/// key 表容量。
pub const KEY_CAP: usize = 12;

/// 一条埋点 key 定义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceKey {
    pub name: &'static str,
    pub point: TracePoint,
}

/// key 注册表：同名两处埋点会让覆盖统计失真，故重名拒。
pub struct TraceRegistry {
    keys: [Option<TraceKey>; KEY_CAP],
    len: usize,
}

impl TraceRegistry {
    /// 空注册表。
    pub const fn new() -> TraceRegistry {
        TraceRegistry {
            keys: [None; KEY_CAP],
            len: 0,
        }
    }

    /// 注册 key；空名 → `EmptyKey`，重名 → `DupKey`，容量满 → `CapFull`。
    pub fn register(&mut self, name: &'static str, point: TracePoint) -> Result<(), TraceError> {
        if name.is_empty() {
            return Err(TraceError::EmptyKey);
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(k) = self.keys.get(i).and_then(|v| *v) {
                if k.name == name {
                    return Err(TraceError::DupKey);
                }
            }
            i += 1;
        }
        if self.len >= KEY_CAP {
            return Err(TraceError::CapFull);
        }
        if let Some(slot) = self.keys.get_mut(self.len) {
            *slot = Some(TraceKey { name, point });
            self.len += 1;
        }
        Ok(())
    }

    /// 已注册 key 数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 某点是否已注册（覆盖配置的期望集合）。
    pub fn has_point(&self, p: TracePoint) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if let Some(k) = self.keys.get(i).and_then(|v| *v) {
                if k.point == p {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 配置覆盖率千分比（三点各注册一次才算 1000）。
    pub fn configured_permille(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TracePoint::ALL.len() {
            if let Some(p) = TracePoint::ALL.get(i) {
                if self.has_point(*p) {
                    n += 1;
                }
            }
            i += 1;
        }
        n * 1000 / 3
    }
}

/// 标准三点注册表（水印作业的默认埋点配置）。
pub fn standard_registry() -> TraceRegistry {
    let mut reg = TraceRegistry::new();
    let mut i = 0usize;
    while i < TracePoint::ALL.len() {
        if let Some(p) = TracePoint::ALL.get(i) {
            let _ = reg.register(p.name(), *p);
        }
        i += 1;
    }
    reg
}

/// 脚本化跑一遍三点埋点；`full=false` 时故意漏掉 Variable 的 Emit（负控）。
pub fn scripted_run(full: bool) -> Option<Tracer> {
    let mut t = Tracer::new();
    let s0 = t.open(TracePoint::Composite.name(), TracePoint::Composite, 0).ok()?;
    t.emit(TracePoint::Composite, 0, 1).ok()?;
    t.close(s0).ok()?;

    let s1 = t.open(TracePoint::Variable.name(), TracePoint::Variable, 1).ok()?;
    if full {
        t.emit(TracePoint::Variable, 1, 42).ok()?;
    }
    t.close(s1).ok()?;

    let s2 = t.open(TracePoint::LayerOrder.name(), TracePoint::LayerOrder, 1).ok()?;
    t.emit(TracePoint::LayerOrder, 1, 0).ok()?;
    t.emit(TracePoint::LayerOrder, 2, 0).ok()?;
    t.close(s2).ok()?;

    t.seal().ok()?;
    Some(t)
}

/// 事件流里 Begin/End 是否成对且全部闭合。
pub fn spans_balanced(t: &Tracer) -> bool {
    let mut balance: i32 = 0;
    let mut bad = false;
    let mut i = 0usize;
    while i < t.event_count() {
        if let Some(e) = t.event(i) {
            match e.phase {
                Phase::Begin => balance += 1,
                Phase::End => balance -= 1,
                Phase::Emit => {}
            }
            if balance < 0 {
                bad = true;
            }
        }
        i += 1;
    }
    !bad && balance == 0
}

// ---------------------------------------------------------------------------
// 注入核：9 类坏样本逐类命中指定诊断码
// ---------------------------------------------------------------------------

/// 构造 9 类坏样本并逐类断言诊断码，返回 (命中数, 总数)。
pub fn injection_core() -> (usize, usize) {
    let total = 9usize;
    let mut caught = 0usize;

    // ① 埋点码越界
    if TracePoint::from_u8(3).err() == Some(TraceError::UnknownPoint) {
        caught += 1;
    }
    // ② 空 key
    let mut t = Tracer::new();
    if t.open("", TracePoint::Composite, 0).err() == Some(TraceError::EmptyKey) {
        caught += 1;
    }
    // ③ key 重名
    let mut reg = TraceRegistry::new();
    if reg.register("wm.composite", TracePoint::Composite).is_ok()
        && reg.register("wm.composite", TracePoint::Composite).err() == Some(TraceError::DupKey)
    {
        caught += 1;
    }
    // ④ 无 span 时 Emit（不配平）
    if t.emit(TracePoint::Composite, 0, 0).err() == Some(TraceError::Unbalanced) {
        caught += 1;
    }
    // ⑤ 跨层闭合
    let open_a = t.open("wm.a", TracePoint::Composite, 0);
    let open_b = t.open("wm.b", TracePoint::LayerOrder, 1);
    let order_hit = match (open_a, open_b) {
        (Ok(a), Ok(_b)) => t.close(a).err() == Some(TraceError::Order),
        _ => false,
    };
    if order_hit {
        caught += 1;
    }
    // ⑥ 层号回退
    let layer_hit = t.emit(TracePoint::LayerOrder, 0, 0).err() == Some(TraceError::LayerRegress);
    if layer_hit {
        caught += 1;
    }
    // ⑦ 变量越界
    if t.emit(TracePoint::Composite, 1, 2_000_000).err() == Some(TraceError::Param) {
        caught += 1;
    }
    // ⑧ 封账后仍写入（先把残留 span 收干净）
    let s_b = 1u16;
    let _ = t.close(s_b);
    let _ = t.close(0);
    let sealed = t.seal().is_ok() && t.emit(TracePoint::Composite, 0, 0).err() == Some(TraceError::Closed);
    if sealed {
        caught += 1;
    }
    // ⑨ 缓冲打满：灌 49 条事件
    let mut t2 = Tracer::new();
    let mut cap_hit = false;
    if let Ok(sp) = t2.open("wm.flood", TracePoint::Composite, 0) {
        let mut k = 0usize;
        while k < TRACE_CAP + 2 {
            if t2.emit(TracePoint::Composite, 0, k as i32).err() == Some(TraceError::CapFull) {
                cap_hit = true;
                break;
            }
            k += 1;
        }
        let _ = sp;
    }
    if cap_hit && t2.dropped() >= 1 {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50050 的 13 条判据。
pub fn run_unx50050_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50050");

    // J1-1 三点闭集：3 合法 + 越界拒
    let set_ok = TracePoint::from_u8(0) == Ok(TracePoint::Composite)
        && TracePoint::from_u8(1) == Ok(TracePoint::Variable)
        && TracePoint::from_u8(2) == Ok(TracePoint::LayerOrder)
        && TracePoint::from_u8(3).err() == Some(TraceError::UnknownPoint)
        && TracePoint::ALL.len() == 3;
    s.add("50050-三点闭集", set_ok, "埋点闭集长度或码值漂移");

    // J1-2 三点配置覆盖率 100%
    let reg = standard_registry();
    let cfg = reg.configured_permille();
    let det = format!("cfg={}permille", cfg);
    s.add_dyn("50050-配置覆盖", cfg == 1000 && reg.len() == 3, &det);

    // J1-3 埋点 100% 覆盖（三点各至少一次 Emit）
    let full = scripted_run(true);
    let full_ok = match &full {
        Some(t) => {
            t.coverage_permille() == 1000
                && t.covered(TracePoint::Composite)
                && t.covered(TracePoint::Variable)
                && t.covered(TracePoint::LayerOrder)
                && t.dropped() == 0
        }
        None => false,
    };
    let det = match &full {
        Some(t) => format!("{}", t.coverage_permille()),
        None => String::from("run failed"),
    };
    s.add_dyn("50050-J1-埋点全覆盖", full_ok, &det);

    // J1-4 负控：漏掉 Variable 后覆盖率真掉到 666‰
    let part = scripted_run(false);
    let part_ok = match &part {
        Some(t) => t.coverage_permille() == 666 && !t.covered(TracePoint::Variable),
        None => false,
    };
    let det = match &part {
        Some(t) => format!("{}permille", t.coverage_permille()),
        None => String::from("run failed"),
    };
    s.add_dyn("50050-覆盖负控", part_ok, &det);

    // J1-5 事件序号严格递增且无空洞
    let seq_ok = match &full {
        Some(t) => {
            let mut ok = true;
            let mut i = 0usize;
            while i < t.event_count() {
                if let Some(e) = t.event(i) {
                    if e.seq != i as u32 {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && t.event_count() > 0
        }
        None => false,
    };
    s.add("50050-事件序号", seq_ok, "事件序号有空洞或重排");

    // J1-6 span 配平：Begin/End 成对且深度归零
    let balance_ok = match &full {
        Some(t) => spans_balanced(t) && t.depth() == 0,
        None => false,
    };
    s.add("50050-跨度配平", balance_ok, "Begin/End 不配平");

    // J1-7 层序单调：同一 span 内 Emit 层号不降
    let layer_ok = match &full {
        Some(t) => {
            // 只看 Emit 的层号：Begin/End 记录的是 span 起始层，
            // 不参与「层序推进」的比较。
            let mut last: [i32; 8] = [-1; 8];
            let mut ok = true;
            let mut i = 0usize;
            while i < t.event_count() {
                if let Some(e) = t.event(i) {
                    if e.phase == Phase::Emit {
                        let idx = e.span as usize % 8;
                        if let Some(v) = last.get_mut(idx) {
                            if e.layer as i32 >= *v {
                                *v = e.layer as i32;
                            } else {
                                ok = false;
                            }
                        }
                    }
                }
                i += 1;
            }
            ok
        }
        None => false,
    };
    s.add("50050-层序单调", layer_ok, "层序事件回退");

    // J1-8 变量捕获：Variable 埋点带上落值 42
    let var_ok = match &full {
        Some(t) => {
            let mut found = false;
            let mut i = 0usize;
            while i < t.event_count() {
                if let Some(e) = t.event(i) {
                    if e.point == TracePoint::Variable && e.phase == Phase::Emit && e.var == 42 {
                        found = true;
                    }
                }
                i += 1;
            }
            found
        }
        None => false,
    };
    s.add("50050-变量捕获", var_ok, "点值未落事件");

    // J1-9 确定性：两次同样脚本的指纹一致
    let d1 = match scripted_run(true) {
        Some(t) => t.digest(),
        None => 0,
    };
    let d2 = match scripted_run(true) {
        Some(t) => t.digest(),
        None => 1,
    };
    s.add("50050-确定性", d1 == d2 && d1 != 0, "两次埋点流不一致");

    // J1-10 封账后写入必拒
    let closed_ok = match scripted_run(true) {
        Some(mut t) => {
            let once = t.emit(TracePoint::Composite, 0, 0).err();
            let open_after = t.open("wm.x", TracePoint::Composite, 0).err();
            once == Some(TraceError::Closed) && open_after == Some(TraceError::Closed)
        }
        None => false,
    };
    s.add("50050-封账拒写", closed_ok, "封账后仍可写入");

    // J1-11 key 表：空名 / 重名 / 容量
    let mut kr = TraceRegistry::new();
    let empty_hit = kr.register("", TracePoint::Composite).err() == Some(TraceError::EmptyKey);
    let first = kr.register("wm.a", TracePoint::Composite).is_ok();
    let dup_hit = kr.register("wm.a", TracePoint::Composite).err() == Some(TraceError::DupKey);
    let mut cap_hit = false;
    let mut k = 1u8;
    while k < 20 {
        let name: &'static str = match k {
            1 => "wm.b",
            2 => "wm.c",
            3 => "wm.d",
            4 => "wm.e",
            5 => "wm.f",
            6 => "wm.g",
            7 => "wm.h",
            8 => "wm.i",
            9 => "wm.j",
            10 => "wm.k",
            11 => "wm.l",
            12 => "wm.m",
            _ => "wm.z",
        };
        match kr.register(name, TracePoint::Variable) {
            Err(TraceError::CapFull) => {
                cap_hit = true;
                break;
            }
            _ => {}
        }
        k += 1;
    }
    let key_ok = empty_hit && first && dup_hit && cap_hit && kr.len() == KEY_CAP;
    s.add("50050-key表纪律", key_ok, "key 表空名/重名/容量失效");

    // J1-12 栈纪律：无 span 时 Emit/Close 拒，跨层闭合拒
    let mut t3 = Tracer::new();
    let no_span_emit = t3.emit(TracePoint::Composite, 0, 0).err() == Some(TraceError::Unbalanced);
    let no_span_close = t3.close(0).err() == Some(TraceError::Unbalanced);
    let a = t3.open("wm.p", TracePoint::Composite, 0);
    let b = t3.open("wm.q", TracePoint::LayerOrder, 1);
    let cross = match (a, b) {
        (Ok(ia), Ok(_ib)) => t3.close(ia).err() == Some(TraceError::Order),
        _ => false,
    };
    let regress = t3.emit(TracePoint::LayerOrder, 0, 0).err() == Some(TraceError::LayerRegress);
    let sealed_unbalanced = {
        let mut t4 = Tracer::new();
        let _ = t4.open("wm.r", TracePoint::Composite, 0);
        t4.seal().err() == Some(TraceError::Unbalanced)
    };
    s.add(
        "50050-栈纪律",
        no_span_emit && no_span_close && cross && regress && sealed_unbalanced,
        "span 栈纪律失效",
    );

    // J1-13 注入核 9/9
    let (caught, total) = injection_core();
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50050-注入必抓", caught == total, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50050_checks_green() {
        let set = run_unx50050_checks();
        assert!(set.all_passed(), "UNX-F50050 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50050_full_coverage() {
        match scripted_run(true) {
            Some(t) => {
                assert_eq!(t.coverage_permille(), 1000);
                assert!(spans_balanced(&t));
                assert_eq!(t.dropped(), 0);
            }
            None => assert!(false, "scripted run failed"),
        }
    }

    #[test]
    fn unx50050_negative_control() {
        match scripted_run(false) {
            Some(t) => assert_eq!(t.coverage_permille(), 666),
            None => assert!(false, "scripted run failed"),
        }
    }
}
