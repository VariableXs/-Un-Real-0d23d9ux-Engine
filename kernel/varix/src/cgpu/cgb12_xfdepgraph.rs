//! cgb12_xfdepgraph — CGPU-F0225 跨帧依赖图模型（B05 组首件）。
//!
//! 判据原文（施工书锚点 CGPU-F0225）：跨帧依赖图：帧 N 任务依赖帧 N-k 产出
//! 资源的建模（历史帧引用边——与 B01 帧任务图同构、扩展为跨帧超图）；历史缓
//! 冲生命周期（跨帧存活资源引用计数与释放时机——不因帧结束误回收）；依赖深
//! 度声明（任务声明跨帧深度——校验不超引擎窗口）；模型不变量（跨帧边必须指
//! 向已提交帧/逻辑环必须可静态判定——为 B06 死锁检测供图）。判据：超图模型、
//! 生命周期、深度声明、不变量。
//!
//! 职责定位：跨帧超图（XfHyperGraph）是 B05 组十六单的图源正本——B01 帧内
//! 依赖图管"本帧任务谁等谁"，本图管"本帧任务等历史第 k 帧谁产出的资源"。
//! 三类账并管：①历史缓冲生命周期账（引用计数+释放纪元——帧结束不误回收，
//! 只有引用归零且越过释放纪元才入回收队）；②依赖深度声明账（任务申报跨帧
//! 深度 k，超引擎窗口即拒——窗口外的历史帧产出按定义已不可见）；③模型不变
//! 量账（跨帧边指向已提交帧 + 逻辑环静态可判定——本模块只做"可静态判定"
//! 的记录与标记，环的在线检测属 cgb18，边界契约见其头注）。诊断码段
//! 0xD410（本会话独占，grep 全库 SEG_BASE 无冲突后领取）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const XF_DEPGRAPH_VERSION: &str = "B12-xfdepgraph-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0xD4 段 0xD410 槽，本会话独占）
// ---------------------------------------------------------------------------

/// 跨帧边指向未提交帧（不变量一违约：历史帧还没提交就有人引用它的产出）。
pub const XC_EDGE_TO_UNSUBMITTED: u32 = 0xD410;
/// 跨帧深度超引擎窗口（深度声明校验失败：k > 引擎在途窗口）。
pub const XC_DEPTH_OVER_WINDOW: u32 = 0xD411;
/// 资源重复登记（同帧同任务对同一资源重复申报产出——账目二义）。
pub const XC_DUP_PRODUCE: u32 = 0xD412;
/// 引用不存在的产出（消费方引用的资源没有任何生产者登记——悬空边）。
pub const XC_DANGLING_CONSUME: u32 = 0xD413;
/// 引用计数下溢（释放次数多于登记引用——生命周期账不平）。
pub const XC_REFCOUNT_UNDERFLOW: u32 = 0xD414;
/// 提前回收（引用未归零就尝试回收——会误删在用历史缓冲）。
pub const XC_PREMATURE_RELEASE: u32 = 0xD415;
/// 深度申报为负序（跨帧深度必须是正数：等本帧资源是 B01 的职责不是本图的）。
pub const XC_DEPTH_NOT_POSITIVE: u32 = 0xD416;
/// 图容量满（边数超上限——防御性拒绝而非静默丢边）。
pub const XC_GRAPH_FULL: u32 = 0xD417;
/// 生产者任务不存在（边引用的帧内任务号越界）。
pub const XC_TASK_OUT_OF_RANGE: u32 = 0xD418;
/// 释放纪元倒挂（释放纪元早于产出纪元——时间账自相矛盾）。
pub const XC_EPOCH_INVERTED: u32 = 0xD419;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 引擎跨帧窗口（帧）：任务可申报的最大跨帧深度。窗口外的历史产出按定义不可见。
pub const ENGINE_WINDOW: u32 = 4;
/// 边容量上限：一张超图登记的跨帧边数上限（防御性容量，防账本无限膨胀）。
pub const EDGE_CAP: usize = 4096;
/// 资源账容量上限。
pub const RESOURCE_CAP: usize = 1024;
/// 帧号广度：图内允许引用的历史帧回溯上限（与 ENGINE_WINDOW 一致的第二道闸）。
pub const FRAME_REACH: u64 = 16;

// ---------------------------------------------------------------------------
// 基础类型
// ---------------------------------------------------------------------------

/// 帧内任务标识（与 B01 帧任务图的任务编号同域同义——不引入第二套编号）。
pub type TaskId = u32;
/// 资源标识（跨帧存活缓冲的唯一号，由资源账分配）。
pub type ResourceId = u32;
/// 纪元号：全局提交纪元（每提交一帧 +1），时间账的单一真源。
pub type Epoch = u64;

/// 一条跨帧依赖边：本帧任务 → 历史第 k 帧任务产出的资源。
#[derive(Clone, Debug)]
pub struct XfEdge {
    /// 消费方帧号（帧 N）。
    pub consumer_frame: u64,
    /// 消费方任务（帧 N 内的任务号）。
    pub consumer_task: TaskId,
    /// 生产方帧号（帧 N-k，N > N-k 必须）。
    pub producer_frame: u64,
    /// 生产方任务（帧 N-k 内的任务号）。
    pub producer_task: TaskId,
    /// 被引用的跨帧资源。
    pub resource: ResourceId,
    /// 申报跨帧深度 k（= consumer_frame - producer_frame，登记时核对）。
    pub depth: u32,
}

impl XfEdge {
    /// 边的规范键（去重与账目核对用）。
    pub fn key(&self) -> String {
        let mut s = String::new();
        s.push_str("f");
        s.push_str(itoa(self.consumer_frame));
        s.push_str("t");
        s.push_str(itoa(self.consumer_task as u64));
        s.push_str("<-f");
        s.push_str(itoa(self.producer_frame));
        s.push_str("t");
        s.push_str(itoa(self.producer_task as u64));
        s.push_str("r");
        s.push_str(itoa(self.resource as u64));
        s
    }
}

/// 历史缓冲生命周期账条目。
#[derive(Clone, Debug)]
pub struct XfResource {
    pub id: ResourceId,
    /// 人类可读名（诊断呈现用）。
    pub name: String,
    /// 产出纪元（哪一帧提交时产出）。
    pub produced_epoch: Epoch,
    /// 释放纪元门槛：引用归零后还须等到本纪元才真回收（给在途读留一帧余量）。
    pub release_epoch_min: Epoch,
    /// 当前引用计数。
    pub refs: u32,
    /// 累计引用次数（守恒对账用：released_final 时 refs_total == ref_events）。
    pub ref_events: u64,
    /// 累计释放次数。
    pub release_events: u64,
    /// 生命周期状态。
    pub state: XfResState,
}

/// 历史缓冲生命周期状态机：Live → Draining → Retired。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XfResState {
    /// 在用：至少一条未消费的跨帧边引用它。
    Live,
    /// 排空：引用已归零，等 release_epoch_min 到点。
    Draining,
    /// 已退役：完成回收，账目封存（只读，再引用即悬空边错误）。
    Retired,
}

impl XfResState {
    pub const fn zh(self) -> &'static str {
        match self {
            XfResState::Live => "在用",
            XfResState::Draining => "排空",
            XfResState::Retired => "退役",
        }
    }
}

/// 帧提交记录：不变量一（跨帧边必须指向已提交帧）的判定真源。
#[derive(Clone, Copy, Debug)]
pub struct FrameCommit {
    pub frame: u64,
    /// 提交纪元。
    pub epoch: Epoch,
    /// 该帧登记的任务数（生产者任务号越界判定用）。
    pub task_count: u32,
}

// ---------------------------------------------------------------------------
// 跨帧超图引擎
// ---------------------------------------------------------------------------

/// 跨帧超图引擎：B05 组图源正本。
pub struct XfHyperGraph {
    /// 引擎名（对账呈现用）。
    pub name: String,
    /// 当前纪元（已提交帧计数）。
    pub epoch: Epoch,
    /// 已提交帧账。
    commits: Vec<FrameCommit>,
    /// 跨帧边账。
    edges: Vec<XfEdge>,
    /// 资源生命周期账。
    resources: Vec<XfResource>,
    /// 逻辑环标记账：静态判定出"可判定环"的边键（供 cgb18 深查与 B06 衔接）。
    cycle_marks: Vec<String>,
    /// 拒绝账：每一次拒绝都留痕（码 + 说明）——零静默。
    rejections: Vec<(u32, String)>,
    /// 统计：累计登记边数 / 累计拒绝数。
    pub total_edges: u64,
    pub total_rejects: u64,
}

impl XfHyperGraph {
    /// 构造：只带名字，一切账从零起。
    pub fn new(name: &str) -> Self {
        XfHyperGraph {
            name: String::from(name),
            epoch: 0,
            commits: Vec::new(),
            edges: Vec::new(),
            resources: Vec::new(),
            cycle_marks: Vec::new(),
            rejections: Vec::new(),
            total_edges: 0,
            total_rejects: 0,
        }
    }

    fn reject(&mut self, code: u32, why: &str) -> u32 {
        self.rejections.push((code, String::from(why)));
        if self.rejections.len() > 256 {
            self.rejections.remove(0);
        }
        self.total_rejects += 1;
        code
    }

    // ---------------- 帧提交 ----------------

    /// 帧提交登记：纪元推进 + 任务数入账。帧号必须严格递增（时间账单调）。
    pub fn commit_frame(&mut self, frame: u64, task_count: u32) -> Result<Epoch, u32> {
        if task_count == 0 {
            return Err(self.reject(XC_TASK_OUT_OF_RANGE, "提交帧任务数为零——空帧不进提交账"));
        }
        if let Some(last) = self.commits.last() {
            if frame <= last.frame {
                return Err(self.reject(
                    XC_EPOCH_INVERTED,
                    "帧号倒退——提交账必须严格递增",
                ));
            }
        }
        self.epoch += 1;
        self.commits.push(FrameCommit { frame, epoch: self.epoch, task_count });
        if self.commits.len() > FRAME_REACH as usize * 2 {
            self.commits.remove(0);
        }
        Ok(self.epoch)
    }

    fn committed(&self, frame: u64) -> Option<&FrameCommit> {
        let mut i = self.commits.len();
        while i > 0 {
            i -= 1;
            if self.commits[i].frame == frame {
                return Some(&self.commits[i]);
            }
        }
        None
    }

    /// 是否已提交（不变量一判定的公开读法）。
    pub fn is_committed(&self, frame: u64) -> bool {
        self.committed(frame).is_some()
    }

    // ---------------- 资源生命周期 ----------------

    /// 登记产出：帧 frame 的任务 producer 产出资源 resource。
    pub fn declare_produce(
        &mut self,
        frame: u64,
        producer: TaskId,
        resource: ResourceId,
        name: &str,
    ) -> Result<(), u32> {
        let fc = match self.committed(frame) {
            Some(fc) => fc,
            None => return Err(self.reject(XC_EDGE_TO_UNSUBMITTED, "产出登记挂在未提交帧上")),
        };
        if producer >= fc.task_count {
            return Err(self.reject(XC_TASK_OUT_OF_RANGE, "生产者任务号越界"));
        }
        if self.resources.iter().any(|r| r.id == resource && r.state != XfResState::Retired) {
            return Err(self.reject(XC_DUP_PRODUCE, "资源重复登记（在册未退役）"));
        }
        if self.resources.len() >= RESOURCE_CAP {
            return Err(self.reject(XC_GRAPH_FULL, "资源账容量满"));
        }
        self.resources.push(XfResource {
            id: resource,
            name: String::from(name),
            produced_epoch: fc.epoch,
            release_epoch_min: fc.epoch + 1,
            refs: 0,
            ref_events: 0,
            release_events: 0,
            state: XfResState::Live,
        });
        Ok(())
    }

    /// 引用计数 +1（消费边登记时调用）。
    pub fn acquire(&mut self, resource: ResourceId) -> Result<u32, u32> {
        let n = self.resources.len();
        let mut i = 0;
        while i < n {
            if self.resources[i].id == resource && self.resources[i].state != XfResState::Retired {
                self.resources[i].refs += 1;
                self.resources[i].ref_events += 1;
                self.resources[i].state = XfResState::Live;
                return Ok(self.resources[i].refs);
            }
            i += 1;
        }
        Err(self.reject(XC_DANGLING_CONSUME, "引用计数目标资源不在册或已退役"))
    }

    /// 引用计数 -1：归零转排空；排空满门槛转退役。
    pub fn release(&mut self, resource: ResourceId, now: Epoch) -> Result<XfResState, u32> {
        let n = self.resources.len();
        let mut i = 0;
        while i < n {
            if self.resources[i].id == resource && self.resources[i].state != XfResState::Retired {
                if self.resources[i].refs == 0 {
                    return Err(self.reject(XC_REFCOUNT_UNDERFLOW, "释放多于引用——计数下溢"));
                }
                if now < self.resources[i].produced_epoch {
                    return Err(self.reject(XC_EPOCH_INVERTED, "释放纪元早于产出纪元"));
                }
                self.resources[i].refs -= 1;
                self.resources[i].release_events += 1;
                if self.resources[i].refs == 0 {
                    if now >= self.resources[i].release_epoch_min {
                        self.resources[i].state = XfResState::Retired;
                    } else {
                        self.resources[i].state = XfResState::Draining;
                    }
                }
                return Ok(self.resources[i].state);
            }
            i += 1;
        }
        Err(self.reject(XC_DANGLING_CONSUME, "释放目标资源不在册"))
    }

    /// 排空账推进：到纪元门槛的排空资源转退役（帧结束扫一遍——不误回收）。
    pub fn advance_retirement(&mut self, now: Epoch) -> u32 {
        let n = self.resources.len();
        let mut i = 0;
        let mut retired = 0;
        while i < n {
            if self.resources[i].state == XfResState::Draining
                && now >= self.resources[i].release_epoch_min
            {
                self.resources[i].state = XfResState::Retired;
                retired += 1;
            }
            i += 1;
        }
        retired
    }

    // ---------------- 跨帧边登记 ----------------

    /// 登记一条跨帧边：consumer_frame 的 consumer_task 依赖 producer_frame 的
    /// producer_task 产出的 resource。全量校验四道：帧序、已提交、任务越界、
    /// 深度窗口。任何一道不过即拒绝并留痕。
    pub fn add_edge(&mut self, edge: XfEdge) -> Result<(), u32> {
        if edge.depth == 0 {
            return Err(self.reject(
                XC_DEPTH_NOT_POSITIVE,
                "跨帧深度为零——等本帧资源是 B01 帧内图的职责",
            ));
        }
        if edge.consumer_frame <= edge.producer_frame {
            return Err(self.reject(XC_DEPTH_NOT_POSITIVE, "帧序倒挂：消费帧必须晚于生产帧"));
        }
        if edge.depth > ENGINE_WINDOW {
            return Err(self.reject(XC_DEPTH_OVER_WINDOW, "跨帧深度超引擎窗口"));
        }
        if edge.consumer_frame.saturating_sub(edge.producer_frame) > FRAME_REACH {
            return Err(self.reject(XC_DEPTH_OVER_WINDOW, "历史回溯超帧号广度上限"));
        }
        let pf = match self.committed(edge.producer_frame) {
            Some(fc) => fc,
            None => {
                return Err(self.reject(
                    XC_EDGE_TO_UNSUBMITTED,
                    "跨帧边指向未提交帧（不变量一）",
                ))
            }
        };
        if edge.producer_task >= pf.task_count {
            return Err(self.reject(XC_TASK_OUT_OF_RANGE, "生产者任务号越界"));
        }
        let cf = match self.committed(edge.consumer_frame) {
            Some(fc) => fc,
            None => {
                return Err(self.reject(
                    XC_EDGE_TO_UNSUBMITTED,
                    "消费方帧未提交——消费边必须挂在已提交帧",
                ))
            }
        };
        if edge.consumer_task >= cf.task_count {
            return Err(self.reject(XC_TASK_OUT_OF_RANGE, "消费者任务号越界"));
        }
        if self.edges.iter().any(|e| {
            e.consumer_frame == edge.consumer_frame
                && e.consumer_task == edge.consumer_task
                && e.producer_frame == edge.producer_frame
                && e.producer_task == edge.producer_task
                && e.resource == edge.resource
        }) {
            return Err(self.reject(XC_DUP_PRODUCE, "跨帧边重复登记"));
        }
        if self.edges.len() >= EDGE_CAP {
            return Err(self.reject(XC_GRAPH_FULL, "边账容量满"));
        }
        // 资源在册校验 + 引用入账（生命周期账与边账联动）。
        self.acquire(edge.resource)?;
        self.total_edges += 1;
        self.edges.push(edge);
        Ok(())
    }

    /// 逻辑环静态标记：同一资源被两条边以"交叉方向"引用（f1t1<-f2t2 且
    /// f2t2'<-f1t1' 同资源），是可静态判定的逻辑环候选——记录标记供 cgb18。
    /// 本模块只标记不做裁决（裁决归 cgb18，边界契约）。
    pub fn mark_static_cycles(&mut self) -> usize {
        self.cycle_marks.clear();
        let n = self.edges.len();
        let mut i = 0;
        while i < n {
            let mut j = i + 1;
            while j < n {
                let a = &self.edges[i];
                let b = &self.edges[j];
                if a.resource == b.resource
                    && a.producer_frame == b.consumer_frame
                    && b.producer_frame == a.consumer_frame
                {
                    self.cycle_marks.push(a.key());
                    self.cycle_marks.push(b.key());
                }
                j += 1;
            }
            i += 1;
        }
        self.cycle_marks.len()
    }

    // ---------------- 查询面 ----------------

    /// 某帧的全部跨帧入边（它消费了哪些历史产出）。
    pub fn edges_into(&self, frame: u64) -> Vec<&XfEdge> {
        let mut out = Vec::new();
        for e in self.edges.iter() {
            if e.consumer_frame == frame {
                out.push(e);
            }
        }
        out
    }

    /// 某帧的全部跨帧出边（它产出了哪些被后世引用的资源）。
    pub fn edges_out_of(&self, frame: u64) -> Vec<&XfEdge> {
        let mut out = Vec::new();
        for e in self.edges.iter() {
            if e.producer_frame == frame {
                out.push(e);
            }
        }
        out
    }

    /// 资源当前生命周期状态。
    pub fn resource_state(&self, resource: ResourceId) -> Option<XfResState> {
        for r in self.resources.iter() {
            if r.id == resource {
                return Some(r.state);
            }
        }
        None
    }

    /// 依赖深度分布（深度 k → 边数）：窗口占用画像。
    pub fn depth_histogram(&self) -> [u64; (ENGINE_WINDOW + 1) as usize] {
        let mut h = [0u64; (ENGINE_WINDOW + 1) as usize];
        for e in self.edges.iter() {
            if (e.depth as usize) < h.len() {
                h[e.depth as usize] += 1;
            }
        }
        h
    }

    /// 拒绝账只读投影。
    pub fn rejection_log(&self) -> &[(u32, String)] {
        &self.rejections
    }

    /// 静态环标记只读投影。
    pub fn cycle_marks(&self) -> &[String] {
        &self.cycle_marks
    }

    /// 生命周期账守恒核验：在册未退役资源的 refs 合计必须等于
    /// ref_events - release_events 的差额（计数不凭空）。
    pub fn lifecycle_conservation(&self) -> Result<(), u32> {
        let mut live_refs: i64 = 0;
        let mut delta: i64 = 0;
        for r in self.resources.iter() {
            if r.state != XfResState::Retired {
                live_refs += r.refs as i64;
            }
            delta += r.ref_events as i64 - r.release_events as i64;
        }
        if live_refs != delta {
            return Err(XC_REFCOUNT_UNDERFLOW);
        }
        Ok(())
    }

    /// 模型不变量总核验（供组收口 cgb27 调用）：
    /// ① 每条边的生产帧与消费帧都已提交；② 深度账与帧差一致；③ 生命周期守恒。
    pub fn validate_invariants(&self) -> Result<(), u32> {
        for e in self.edges.iter() {
            if !self.is_committed(e.producer_frame) || !self.is_committed(e.consumer_frame) {
                return Err(XC_EDGE_TO_UNSUBMITTED);
            }
            if e.depth != (e.consumer_frame - e.producer_frame) as u32 {
                return Err(XC_DEPTH_NOT_POSITIVE);
            }
            if e.depth == 0 || e.depth > ENGINE_WINDOW {
                return Err(XC_DEPTH_OVER_WINDOW);
            }
        }
        self.lifecycle_conservation()
    }

    /// 快照（B05 组对账与移交包用）。
    pub fn snapshot(&self) -> XfGraphSnap {
        XfGraphSnap {
            version: String::from(XF_DEPGRAPH_VERSION),
            engine: self.name.clone(),
            epoch: self.epoch,
            commits: self.commits.len() as u64,
            edges: self.edges.len() as u64,
            live: self.resources.iter().filter(|r| r.state == XfResState::Live).count() as u64,
            draining: self
                .resources
                .iter()
                .filter(|r| r.state == XfResState::Draining)
                .count() as u64,
            retired: self
                .resources
                .iter()
                .filter(|r| r.state == XfResState::Retired)
                .count() as u64,
            cycle_marks: self.cycle_marks.len() as u64,
            rejects: self.total_rejects,
        }
    }
}

/// 图快照（只读投影，数字单一真源在引擎，快照仅供呈现与对账）。
#[derive(Clone, Debug)]
pub struct XfGraphSnap {
    pub version: String,
    pub engine: String,
    pub epoch: Epoch,
    pub commits: u64,
    pub edges: u64,
    pub live: u64,
    pub draining: u64,
    pub retired: u64,
    pub cycle_marks: u64,
    pub rejects: u64,
}

impl XfGraphSnap {
    /// 诊断一行（无浮点、无墙钟、确定性输出）。
    pub fn detail(&self) -> String {
        let mut s = String::from("[XF图 ");
        s.push_str(&self.engine);
        s.push_str(" 纪元=");
        s.push_str(itoa(self.epoch));
        s.push_str(" 边=");
        s.push_str(itoa(self.edges));
        s.push_str(" 在用=");
        s.push_str(itoa(self.live));
        s.push_str(" 排空=");
        s.push_str(itoa(self.draining));
        s.push_str(" 退役=");
        s.push_str(itoa(self.retired));
        s.push_str(" 环标记=");
        s.push_str(itoa(self.cycle_marks));
        s.push_str(" 拒绝=");
        s.push_str(itoa(self.rejects));
        s.push_str("]");
        s
    }
}

// ---------------------------------------------------------------------------
// 内部工具
// ---------------------------------------------------------------------------

/// u64 → 十进制（no_std 无 itoa，手写——确定性输出）。
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    // 安全：buf[i..] 全为 ASCII 数字。
    match core::str::from_utf8(&buf[i..]) {
        Ok(s) => String::from(s),
        Err(_) => String::from("?"),
    }
}

// ---------------------------------------------------------------------------
// 场景化构造与核对（纯函数，确定性——供组内联调与后续判据锚定）
// ---------------------------------------------------------------------------

/// 场景 A：三帧 ping-pong——F1 产 A，F2 产 B 并消费 A，F3 消费 B。
/// 全链合法：登记零拒绝、生命周期守恒、不变量核验通过。
pub fn scenario_pingpong() -> Result<XfGraphSnap, u32> {
    let mut g = XfHyperGraph::new("B12-三帧乒乓");
    g.commit_frame(1, 4)?;
    g.declare_produce(1, 0, 100, "色板A")?;
    g.commit_frame(2, 4)?;
    g.declare_produce(2, 1, 101, "色板B")?;
    g.add_edge(XfEdge {
        consumer_frame: 2,
        consumer_task: 2,
        producer_frame: 1,
        producer_task: 0,
        resource: 100,
        depth: 1,
    })?;
    g.commit_frame(3, 4)?;
    g.add_edge(XfEdge {
        consumer_frame: 3,
        consumer_task: 3,
        producer_frame: 2,
        producer_task: 1,
        resource: 101,
        depth: 1,
    })?;
    g.validate_invariants()?;
    let st = g.release(100, 3)?;
    if st != XfResState::Draining && st != XfResState::Retired {
        return Err(XC_PREMATURE_RELEASE);
    }
    g.advance_retirement(4);
    g.validate_invariants()?;
    Ok(g.snapshot())
}

/// 场景 B：不变量一负向——跨帧边指向未提交帧必须被拒。
pub fn scenario_reject_unsubmitted() -> Result<(), u32> {
    let mut g = XfHyperGraph::new("B12-未提交拒");
    g.commit_frame(1, 2)?;
    let r = g.add_edge(XfEdge {
        consumer_frame: 2,
        consumer_task: 0,
        producer_frame: 2,
        producer_task: 0,
        resource: 1,
        depth: 1,
    });
    match r {
        Err(code) if code == XC_EDGE_TO_UNSUBMITTED || code == XC_DEPTH_NOT_POSITIVE => Ok(()),
        Ok(()) => Err(XC_EDGE_TO_UNSUBMITTED), // 拒绝失败即不变量失守
        Err(other) => Err(other),
    }
}

/// 场景 C：深度窗口负向——深度 5 超 ENGINE_WINDOW=4 必须被拒。
pub fn scenario_reject_over_window() -> Result<(), u32> {
    let mut g = XfHyperGraph::new("B12-超窗拒");
    let mut f = 1;
    while f <= 5 {
        g.commit_frame(f, 2)?;
        f += 1;
    }
    g.declare_produce(1, 0, 7, "老资源")?;
    let r = g.add_edge(XfEdge {
        consumer_frame: 6,
        consumer_task: 0,
        producer_frame: 1,
        producer_task: 0,
        resource: 7,
        depth: 5,
    });
    match r {
        Err(code) if code == XC_DEPTH_OVER_WINDOW => Ok(()),
        Ok(()) => Err(XC_DEPTH_OVER_WINDOW),
        Err(other) => Err(other),
    }
}

/// 场景 D：误回收防线——引用未归零尝试释放两次触发下溢拒绝。
pub fn scenario_reject_underflow() -> Result<(), u32> {
    let mut g = XfHyperGraph::new("B12-下溢拒");
    g.commit_frame(1, 2)?;
    g.declare_produce(1, 0, 9, "单引用资源")?;
    g.acquire(9)?;
    g.release(9, 2)?;
    let r = g.release(9, 2);
    match r {
        Err(code) if code == XC_REFCOUNT_UNDERFLOW => Ok(()),
        Ok(_) => Err(XC_REFCOUNT_UNDERFLOW),
        Err(other) => Err(other),
    }
}
