//! cgb821_media_engine — CGPU-F0821 Intel 媒体引擎联动。
//!
//! 判据原文（验收行摘录）：Intel 媒体引擎联动：MFX（编解码）/VEBox（去隔
//! 行）/Render（后处理）三引擎队列映射；跨引擎依赖链（解码完成才可去隔
//! 行、去隔行完成才可渲染）；引擎能力交集校验（请求特性任一引擎不支持即
//! 拒）；联动账本（提交-完成计数守恒：提交数 = 完成数 + 在途数 + 拒绝数）。
//! 判据：三引擎映射、依赖链、能力交集、守恒账本。
//!
//! 诊断段 0x7120（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；失败路径全部
//! Result<_, u16> 闭集错误码；依赖链用显式阶段状态机表达；定容账本守恒
//! 可验；drill() 自例程返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb821-v1";

/// 诊断段基（本单独占 0x7120）。
pub const SEG_BASE: u16 = 0x7120;

/// 引擎未上线（submit 时对应引擎未就绪）。
pub const XC_ENGINE_OFFLINE: u16 = SEG_BASE + 1;
/// 阶段越序（前置阶段未完成即提交下一阶段——依赖链违约）。
pub const XC_STAGE_ORDER: u16 = SEG_BASE + 2;
/// 能力不支持（请求特性超出引擎能力集交集）。
pub const XC_CAP_UNSUPPORTED: u16 = SEG_BASE + 3;
/// 流水槽满（定容在途数耗尽——防御性拒绝）。
pub const XC_PIPELINE_FULL: u16 = SEG_BASE + 4;
/// 流不匹配（提交的流号与阶段账不符）。
pub const XC_STREAM_MISMATCH: u16 = SEG_BASE + 5;
/// 重复完成（同一槽位二次完成——语义违约）。
pub const XC_DOUBLE_COMPLETE: u16 = SEG_BASE + 6;
/// 守恒破坏（提交数 ≠ 完成+在途+拒绝）。
pub const XC_LEDGER_DRIFT: u16 = SEG_BASE + 7;

/// 在途流水容量（定容）。
pub const PIPELINE_CAP: usize = 16;
/// 阶段数（解码→去隔行→渲染）。
pub const STAGE_NUM: usize = 3;

/// 引擎种类（判据"三引擎队列映射"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Engine {
    /// MFX：编解码引擎。
    Mfx,
    /// VEBox：去隔行/影像增强引擎。
    Vebox,
    /// Render：EU 后处理引擎。
    Render,
}

impl Engine {
    /// 阶段序号（依赖链顺序）。
    pub fn stage(self) -> usize {
        match self {
            Engine::Mfx => 0,
            Engine::Vebox => 1,
            Engine::Render => 2,
        }
    }
    /// 阶段名。
    pub fn name(self) -> &'static str {
        match self {
            Engine::Mfx => "mfx",
            Engine::Vebox => "vebox",
            Engine::Render => "render",
        }
    }
}

/// 特性位（能力交集校验的最小粒度）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Feat {
    /// H.264 解码。
    H264Dec,
    /// HEVC 解码。
    HevcDec,
    /// 去隔行。
    Deinterlace,
    /// 色彩空间转换。
    Csc,
    /// 缩放。
    Scale,
}

/// 三引擎能力矩阵（初始化即冻结——判据"能力交集"数据源）。
pub fn engine_feats(e: Engine) -> Vec<Feat> {
    match e {
        Engine::Mfx => vec![Feat::H264Dec, Feat::HevcDec],
        Engine::Vebox => vec![Feat::Deinterlace, Feat::Csc],
        Engine::Render => vec![Feat::Csc, Feat::Scale],
    }
}

/// 判据"能力交集校验"：请求特性必须被所属阶段引擎支持。
pub fn stage_supports(stage: usize, f: Feat) -> Result<(), u16> {
    let eng = match stage {
        0 => Engine::Mfx,
        1 => Engine::Vebox,
        2 => Engine::Render,
        _ => return Err(XC_STAGE_ORDER),
    };
    for have in engine_feats(eng) {
        if have == f {
            return Ok(());
        }
    }
    Err(XC_CAP_UNSUPPORTED)
}

/// 在途流水槽。
#[derive(Clone, Debug)]
pub struct Slot {
    /// 流号。
    pub stream: u64,
    /// 已完成到的阶段（0 表示尚未完成任何阶段）。
    pub done_stage: usize,
    /// 各阶段请求的特性。
    pub feats: [Feat; STAGE_NUM],
    /// 槽位是否活跃。
    pub live: bool,
}

/// 计数账本（守恒：submitted == done + inflight + rejected）。
#[derive(Clone, Copy, Debug)]
pub struct Counters {
    /// 累计阶段提交。
    pub submitted: u64,
    /// 累计阶段完成。
    pub done: u64,
    /// 累计拒绝。
    pub rejected: u64,
}

impl Counters {
    fn new() -> Self {
        Counters { submitted: 0, done: 0, rejected: 0 }
    }
    /// 当前在途 = 已完成阶段数总和的补数意义上的活跃槽数（本账简化为活跃槽数）。
    fn inflight(&self, live_slots: usize) -> u64 {
        // 恒等式采用：submitted == done + rejected + 未完成阶段账
        // 未完成阶段账由 verify() 遍历活跃槽累计。
        live_slots as u64
    }
}

/// 媒体联动引擎。
pub struct MediaEngine {
    /// 引擎名。
    pub name: String,
    /// 三引擎上线位。
    online: [bool; STAGE_NUM],
    /// 在途流水（定容）。
    slots: Vec<Slot>,
    /// 计数账本。
    pub cnt: Counters,
    /// 每阶段完成计数。
    stage_done: [u64; STAGE_NUM],
}

impl MediaEngine {
    /// 构造：全引擎离线。
    pub fn new(name: &str) -> Self {
        MediaEngine {
            name: name.to_string(),
            online: [false; STAGE_NUM],
            slots: Vec::new(),
            cnt: Counters::new(),
            stage_done: [0; STAGE_NUM],
        }
    }

    /// 引擎上线/下线。
    pub fn set_online(&mut self, stage: usize, on: bool) -> Result<(), u16> {
        if stage >= STAGE_NUM {
            return Err(XC_ENGINE_OFFLINE);
        }
        self.online[stage] = on;
        Ok(())
    }

    /// 引擎是否上线。
    pub fn is_online(&self, stage: usize) -> Result<(), u16> {
        if stage >= STAGE_NUM {
            return Err(XC_ENGINE_OFFLINE);
        }
        if !self.online[stage] {
            return Err(XC_ENGINE_OFFLINE);
        }
        Ok(())
    }

    /// 查活跃槽（流号），无则返回错误。
    fn find_slot(&self, stream: u64) -> Result<usize, u16> {
        for (i, s) in self.slots.iter().enumerate() {
            if s.live && s.stream == stream {
                return Ok(i);
            }
        }
        Err(XC_STREAM_MISMATCH)
    }

    /// 提交一帧的解码阶段（阶段 0，入口）。
    pub fn submit_decode(&mut self, stream: u64, f: Feat) -> Result<(), u16> {
        self.cnt.submitted += 1;
        self.is_online(0)?;
        stage_supports(0, f)?;
        if self.find_slot(stream).is_ok() {
            self.cnt.rejected += 1;
            return Err(XC_STREAM_MISMATCH);
        }
        if self.live_slots() >= PIPELINE_CAP {
            self.cnt.rejected += 1;
            return Err(XC_PIPELINE_FULL);
        }
        self.slots.push(Slot { stream, done_stage: 0, feats: [f, f, f], live: true });
        Ok(())
    }

    /// 提交下一阶段（判据"跨引擎依赖链"：前置阶段须已完成）。
    pub fn submit_next(&mut self, stream: u64, f: Feat) -> Result<usize, u16> {
        self.cnt.submitted += 1;
        let i = self.find_slot(stream)?;
        let cur = self.slots[i].done_stage;
        if cur == 0 {
            self.cnt.rejected += 1;
            return Err(XC_STAGE_ORDER);
        }
        if cur >= STAGE_NUM {
            self.cnt.rejected += 1;
            return Err(XC_STAGE_ORDER);
        }
        self.is_online(cur)?;
        stage_supports(cur, f)?;
        self.slots[i].feats[cur] = f;
        Ok(cur)
    }

    /// 完成当前阶段（MFX/VEBOX/RENDER 完成中断的账务面）。
    pub fn complete(&mut self, stream: u64) -> Result<usize, u16> {
        let i = self.find_slot(stream)?;
        let stage = self.slots[i].done_stage;
        if stage >= STAGE_NUM {
            self.cnt.rejected += 1;
            return Err(XC_DOUBLE_COMPLETE);
        }
        self.slots[i].done_stage += 1;
        self.stage_done[stage] += 1;
        let now = self.slots[i].done_stage;
        if now >= STAGE_NUM {
            self.slots[i].live = false;
            self.slots.remove(i);
        }
        self.cnt.done += 1;
        Ok(now)
    }

    /// 活跃槽数。
    pub fn live_slots(&self) -> usize {
        let mut n = 0;
        for s in self.slots.iter() {
            if s.live {
                n += 1;
            }
        }
        n
    }

    /// 未完成阶段账（活跃槽剩余阶段数总和——守恒第三项）。
    pub fn pending_stages(&self) -> u64 {
        let mut n = 0u64;
        for s in self.slots.iter() {
            if s.live {
                n += (STAGE_NUM - s.done_stage) as u64;
            }
        }
        n
    }

    /// 阶段完成查询。
    pub fn stage_done_count(&self, stage: usize) -> Result<u64, u16> {
        if stage >= STAGE_NUM {
            return Err(XC_STAGE_ORDER);
        }
        Ok(self.stage_done[stage])
    }

    /// 守恒校验：submitted == done + rejected + pending_stages。
    pub fn verify(&self) -> Result<(), u16> {
        if self.cnt.submitted != self.cnt.done + self.cnt.rejected + self.pending_stages() {
            return Err(XC_LEDGER_DRIFT);
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb821[");
        s.push_str(&self.name);
        s.push_str("] online=");
        s.push_str(if self.online[0] { "1" } else { "0" });
        s.push_str(if self.online[1] { "1" } else { "0" });
        s.push_str(if self.online[2] { "1" } else { "0" });
        s.push_str(" sub=");
        s.push_str(&self.cnt.submitted.to_string());
        s.push_str(" done=");
        s.push_str(&self.cnt.done.to_string());
        s.push_str(" rej=");
        s.push_str(&self.cnt.rejected.to_string());
        s.push_str(" pend=");
        s.push_str(&self.pending_stages().to_string());
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    let mut e = MediaEngine::new("w3-drill-821");

    // 负样本 1：引擎离线提交——应拒。
    match e.submit_decode(1, Feat::H264Dec) {
        Err(code) if code == XC_ENGINE_OFFLINE => {}
        Ok(_) => return Err(XC_ENGINE_OFFLINE),
        Err(other) => return Err(other),
    }
    e.set_online(0, true)?;
    e.set_online(1, true)?;
    e.set_online(2, true)?;

    // 负样本 2：能力交集——MFX 不支持缩放。
    match e.submit_decode(1, Feat::Scale) {
        Err(code) if code == XC_CAP_UNSUPPORTED => {}
        Ok(_) => return Err(XC_CAP_UNSUPPORTED),
        Err(other) => return Err(other),
    }

    // 正样本：全链路解码→去隔行→渲染。
    e.submit_decode(1, Feat::HevcDec)?;
    e.verify()?;
    // 负样本 3：跳过依赖链直接渲染（阶段 0 未完成）——应拒。
    match e.submit_next(1, Feat::Scale) {
        Err(code) if code == XC_STAGE_ORDER => {}
        Ok(_) => return Err(XC_STAGE_ORDER),
        Err(other) => return Err(other),
    }
    e.complete(1)?; // 解码完成
    e.submit_next(1, Feat::Deinterlace)?; // 去隔行提交（能力交集中 Csc 也可）
    e.complete(1)?; // 去隔行完成
    e.submit_next(1, Feat::Scale)?; // 渲染提交
    e.complete(1)?; // 渲染完成——流收口
    e.verify()?;

    // 负样本 4：重复完成已收口流——应拒。
    match e.complete(1) {
        Err(code) if code == XC_STREAM_MISMATCH || code == XC_DOUBLE_COMPLETE => {}
        Ok(_) => return Err(XC_DOUBLE_COMPLETE),
        Err(other) => return Err(other),
    }
    // 负样本 5：未知流提交下一阶段——应拒。
    match e.submit_next(99, Feat::Csc) {
        Err(code) if code == XC_STREAM_MISMATCH => {}
        Ok(_) => return Err(XC_STREAM_MISMATCH),
        Err(other) => return Err(other),
    }

    // 阶段账核对：每阶段恰好完成 1 次。
    if e.stage_done_count(0)? != 1 || e.stage_done_count(1)? != 1 || e.stage_done_count(2)? != 1 {
        return Err(XC_LEDGER_DRIFT);
    }
    e.verify()?;
    if e.cnt.rejected < 3 {
        return Err(XC_LEDGER_DRIFT);
    }
    Ok(0)
}
