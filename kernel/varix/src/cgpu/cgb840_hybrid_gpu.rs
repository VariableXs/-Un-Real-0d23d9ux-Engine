//! cgb840_hybrid_gpu — CGPU-F0840 Intel 混合 GPU 场景。
//!
//! 判据原文（验收行摘录）：Intel 混合 GPU 场景：双适配器登记（iGPU/dGPU
//! 逐台登记含档位）；任务路由表（按任务型别选适配器：媒体→iGPU 优先、
//! 计算→dGPU 优先、缺失则回退另一侧）；路由守恒（提交数 = iGPU 完成 +
//! dGPU 完成 + 回退完成 + 拒绝）；亲和迁移账（任务可在两侧迁移，迁移计
//! 数入账）。判据：双登记、路由表、守恒、迁移账。
//!
//! 诊断段 0x7380（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；失败路径
//! Result<_, u16> 闭集错误码；定容任务账；守恒每步可验；drill() 自例程
//! 返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb840-v1";

/// 诊断段基（本单独占 0x7380）。
pub const SEG_BASE: u16 = 0x7380;

/// 适配器未登记（路由时目标侧不在册）。
pub const XC_ADAP_MISS: u16 = SEG_BASE + 1;
/// 双侧未登记（两侧都缺——任务拒收）。
pub const XC_BOTH_MISS: u16 = SEG_BASE + 2;
/// 适配器重登（同 ID 二次登记）。
pub const XC_ADAP_DUP: u16 = SEG_BASE + 3;
/// 任务不存在（完成/迁移未登记任务）。
pub const XC_TASK_MISS: u16 = SEG_BASE + 4;
/// 任务已完成（二次完成）。
pub const XC_TASK_DONE: u16 = SEG_BASE + 5;
/// 路由守恒破坏。
pub const XC_LEDGER_DRIFT: u16 = SEG_BASE + 6;
/// 任务账满（定容耗尽）。
pub const XC_TASK_FULL: u16 = SEG_BASE + 7;

/// 任务账定容。
pub const TASK_CAP: usize = 24;

/// 适配器侧。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    /// 集成显卡。
    Igpu,
    /// 独立显卡。
    Dgpu,
}

impl Side {
    /// 侧名。
    pub fn name(self) -> &'static str {
        match self {
            Side::Igpu => "igpu",
            Side::Dgpu => "dgpu",
        }
    }
    /// 另一侧。
    pub fn other(self) -> Side {
        match self {
            Side::Igpu => Side::Dgpu,
            Side::Dgpu => Side::Igpu,
        }
    }
}

/// 任务型别（判据"路由表"键）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskKind {
    /// 媒体（解码/编码）。
    Media,
    /// 计算（并行内核）。
    Compute,
    /// 渲染合成。
    Render,
}

impl TaskKind {
    /// 首选侧（判据"媒体→iGPU 优先、计算→dGPU 优先"，渲染不限）。
    pub fn prefer(self) -> Option<Side> {
        match self {
            TaskKind::Media => Some(Side::Igpu),
            TaskKind::Compute => Some(Side::Dgpu),
            TaskKind::Render => None,
        }
    }
}

/// 适配器登记条目。
#[derive(Clone, Debug)]
pub struct Adapter {
    /// PCI ID。
    pub dev_id: u16,
    /// 侧别。
    pub side: Side,
    /// 档位名（如 "xe_hpc"/"uhd"）。
    pub tier: String,
    /// 在线否。
    pub online: bool,
}

/// 任务条目。
#[derive(Clone, Debug)]
pub struct Task {
    /// 任务号。
    pub id: u64,
    /// 型别。
    pub kind: TaskKind,
    /// 当前落侧（None=未路由）。
    pub side: Option<Side>,
    /// 完成否。
    pub done: bool,
    /// 迁移次数。
    pub moves: u32,
}

/// 混合路由引擎。
pub struct HybridRouter {
    /// 引擎名。
    pub name: String,
    /// 适配器表（定容 4：双侧各至多 2）。
    pub adapters: Vec<Adapter>,
    /// 任务账（定容）。
    pub tasks: Vec<Task>,
    /// 各侧完成计数（守恒项）。
    pub done_igpu: u64,
    pub done_dgpu: u64,
    /// 拒绝计数。
    pub rejected: u64,
    /// 迁移计数。
    pub moves: u64,
    /// 任务号发生器。
    next_id: u64,
}

/// 守恒恒等式：tasks.len() == iGPU完成 + dGPU完成 + 未完成数；
/// 且已完成数 == done_igpu + done_dgpu；拒绝不计入任务账。
impl HybridRouter {
    /// 构造。
    pub fn new(name: &str) -> Self {
        HybridRouter {
            name: name.to_string(),
            adapters: Vec::new(),
            tasks: Vec::new(),
            done_igpu: 0,
            done_dgpu: 0,
            rejected: 0,
            moves: 0,
            next_id: 1,
        }
    }

    /// 登记适配器（判据"双适配器登记"）。
    pub fn add_adapter(&mut self, dev_id: u16, side: Side, tier: &str, online: bool) -> Result<(), u16> {
        if self.adapters.len() >= 4 {
            return Err(XC_ADAP_DUP);
        }
        for a in self.adapters.iter() {
            if a.dev_id == dev_id {
                return Err(XC_ADAP_DUP);
            }
        }
        self.adapters.push(Adapter { dev_id, side, tier: tier.to_string(), online });
        Ok(())
    }

    /// 侧是否在线在册。
    pub fn side_online(&self, s: Side) -> Result<(), u16> {
        for a in self.adapters.iter() {
            if a.side == s && a.online {
                return Ok(());
            }
        }
        Err(XC_ADAP_MISS)
    }

    /// 提交任务（判据"路由表"：首选侧在线即落，否则回退另一侧）。
    pub fn submit(&mut self, kind: TaskKind) -> Result<u64, u16> {
        if self.tasks.len() >= TASK_CAP {
            self.rejected += 1;
            return Err(XC_TASK_FULL);
        }
        // 选侧。
        let mut picked: Option<Side> = None;
        match kind.prefer() {
            Some(p) => {
                if self.side_online(p).is_ok() {
                    picked = Some(p);
                } else if self.side_online(p.other()).is_ok() {
                    picked = Some(p.other());
                }
            }
            None => {
                // 渲染不限：任一侧在线即可（优先 iGPU）。
                if self.side_online(Side::Igpu).is_ok() {
                    picked = Some(Side::Igpu);
                } else if self.side_online(Side::Dgpu).is_ok() {
                    picked = Some(Side::Dgpu);
                }
            }
        }
        let side = match picked {
            Some(s) => s,
            None => {
                self.rejected += 1;
                return Err(XC_BOTH_MISS);
            }
        };
        let id = self.next_id;
        self.next_id += 1;
        self.tasks.push(Task { id, kind, side: Some(side), done: false, moves: 0 });
        self.verify()?;
        Ok(id)
    }

    /// 完成任务（判据"路由守恒"落地）。
    pub fn complete(&mut self, id: u64) -> Result<(), u16> {
        for t in self.tasks.iter_mut() {
            if t.id == id {
                if t.done {
                    return Err(XC_TASK_DONE);
                }
                match t.side {
                    Some(Side::Igpu) => self.done_igpu += 1,
                    Some(Side::Dgpu) => self.done_dgpu += 1,
                    None => return Err(XC_TASK_MISS),
                }
                t.done = true;
                self.verify()?;
                return Ok(());
            }
        }
        Err(XC_TASK_MISS)
    }

    /// 迁移任务（判据"亲和迁移账"：未完成任务换侧）。
    pub fn migrate(&mut self, id: u64, to: Side) -> Result<(), u16> {
        self.side_online(to)?;
        for t in self.tasks.iter_mut() {
            if t.id == id {
                if t.done {
                    return Err(XC_TASK_DONE);
                }
                let from = match t.side {
                    Some(s) => s,
                    None => return Err(XC_TASK_MISS),
                };
                if from == to {
                    return Err(XC_TASK_MISS);
                }
                t.side = Some(to);
                t.moves += 1;
                self.moves += 1;
                self.verify()?;
                return Ok(());
            }
        }
        Err(XC_TASK_MISS)
    }

    /// 未完成计数。
    pub fn inflight(&self) -> u64 {
        let mut n = 0;
        for t in self.tasks.iter() {
            if !t.done {
                n += 1;
            }
        }
        n
    }

    /// 已完成计数。
    pub fn done_total(&self) -> u64 {
        self.done_igpu + self.done_dgpu
    }

    /// 守恒校验（判据"路由守恒"）。
    pub fn verify(&self) -> Result<(), u16> {
        if self.done_total() + self.inflight() != self.tasks.len() as u64 {
            return Err(XC_LEDGER_DRIFT);
        }
        // 明细对位：每完成任务的侧计数与任务账一致。
        let mut ig = 0u64;
        let mut dg = 0u64;
        for t in self.tasks.iter() {
            if t.done {
                match t.side {
                    Some(Side::Igpu) => ig += 1,
                    Some(Side::Dgpu) => dg += 1,
                    None => return Err(XC_LEDGER_DRIFT),
                }
            }
        }
        if ig != self.done_igpu || dg != self.done_dgpu {
            return Err(XC_LEDGER_DRIFT);
        }
        // 迁移计数与任务账一致。
        let mut mv = 0u64;
        for t in self.tasks.iter() {
            mv += t.moves as u64;
        }
        if mv != self.moves {
            return Err(XC_LEDGER_DRIFT);
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb840[");
        s.push_str(&self.name);
        s.push_str("] adap=");
        s.push_str(&self.adapters.len().to_string());
        s.push_str(" tasks=");
        s.push_str(&self.tasks.len().to_string());
        s.push_str(" done_i=");
        s.push_str(&self.done_igpu.to_string());
        s.push_str(" done_d=");
        s.push_str(&self.done_dgpu.to_string());
        s.push_str(" rej=");
        s.push_str(&self.rejected.to_string());
        s.push_str(" mv=");
        s.push_str(&self.moves.to_string());
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    let mut r = HybridRouter::new("w3-drill-840");

    // 负样本 1：双侧未登记即提交——应拒。
    match r.submit(TaskKind::Media) {
        Err(code) if code == XC_BOTH_MISS => {}
        Ok(_) => return Err(XC_BOTH_MISS),
        Err(other) => return Err(other),
    }
    if r.rejected != 1 {
        return Err(XC_LEDGER_DRIFT);
    }

    // 登记 iGPU（在线）。
    r.add_adapter(0x46A0, Side::Igpu, "iris_xe", true)?;
    // 负样本 2：重登——应拒。
    match r.add_adapter(0x46A0, Side::Igpu, "iris_xe", true) {
        Err(code) if code == XC_ADAP_DUP => {}
        Ok(_) => return Err(XC_ADAP_DUP),
        Err(other) => return Err(other),
    }
    // 媒体首选 iGPU（在线）→ 落 iGPU；计算首选 dGPU（离线）→ 回退 iGPU。
    let t1 = r.submit(TaskKind::Media)?;
    let t2 = r.submit(TaskKind::Compute)?;
    if r.tasks[0].side != Some(Side::Igpu) || r.tasks[1].side != Some(Side::Igpu) {
        return Err(XC_LEDGER_DRIFT);
    }
    // 登记 dGPU（在线）。
    r.add_adapter(0x56A0, Side::Dgpu, "xe_hpc", true)?;
    let t3 = r.submit(TaskKind::Compute)?;
    if r.tasks[2].side != Some(Side::Dgpu) {
        return Err(XC_LEDGER_DRIFT);
    }
    let t4 = r.submit(TaskKind::Render)?;
    let t5 = r.submit(TaskKind::Media)?;

    // 亲和迁移：t1 从 iGPU 迁 dGPU。
    r.migrate(t1, Side::Dgpu)?;
    if r.tasks[0].moves != 1 {
        return Err(XC_LEDGER_DRIFT);
    }
    // 负样本 3：迁到当前侧——应拒。
    match r.migrate(t1, Side::Dgpu) {
        Err(code) if code == XC_TASK_MISS => {}
        Ok(_) => return Err(XC_TASK_MISS),
        Err(other) => return Err(other),
    }
    // 负样本 4：迁到未登记/离线侧（构造离线：无第三侧可迁——用下线接口不便，
    // 改为查未登记侧拒绝：Side 只有两侧且都已在线，此处验证重复迁移被拒即可）。

    // 完成任务。
    r.complete(t1)?;
    r.complete(t3)?;
    r.complete(t4)?;
    // 负样本 5：重复完成。
    match r.complete(t1) {
        Err(code) if code == XC_TASK_DONE => {}
        Ok(_) => return Err(XC_TASK_DONE),
        Err(other) => return Err(other),
    }
    // 负样本 6：完成已迁移任务的旧侧统计应随迁移走——t1 迁至 dGPU 完成，
    // done_dgpu 应含 t1。
    if r.done_dgpu != 2 || r.done_igpu != 1 {
        return Err(XC_LEDGER_DRIFT);
    }
    // 负样本 7：完成不存在任务。
    match r.complete(999) {
        Err(code) if code == XC_TASK_MISS => {}
        Ok(_) => return Err(XC_TASK_MISS),
        Err(other) => return Err(other),
    }

    // 离线路径：下线 dGPU 后提交计算回退 iGPU。
    r.adapters[1].online = false;
    let t6 = r.submit(TaskKind::Compute)?;
    if r.tasks[5].side != Some(Side::Igpu) {
        return Err(XC_LEDGER_DRIFT);
    }
    // 双侧全离线——拒收。
    r.adapters[0].online = false;
    match r.submit(TaskKind::Render) {
        Err(code) if code == XC_BOTH_MISS => {}
        Ok(_) => return Err(XC_BOTH_MISS),
        Err(other) => return Err(other),
    }
    if r.rejected != 2 {
        return Err(XC_LEDGER_DRIFT);
    }
    // 恢复在线。
    r.adapters[0].online = true;
    r.adapters[1].online = true;

    // 迁移到离线侧应拒：下线 dGPU 后 t2 迁 dGPU。
    r.adapters[1].online = false;
    match r.migrate(t2, Side::Dgpu) {
        Err(code) if code == XC_ADAP_MISS => {}
        Ok(_) => return Err(XC_ADAP_MISS),
        Err(other) => return Err(other),
    }
    r.adapters[1].online = true;

    r.verify()?;
    // 收尾完成剩余任务。
    r.complete(t2)?;
    r.complete(t5)?;
    r.complete(t6)?;
    if r.inflight() != 0 || r.done_total() != r.tasks.len() as u64 {
        return Err(XC_LEDGER_DRIFT);
    }
    r.verify()?;
    Ok(0)
}
