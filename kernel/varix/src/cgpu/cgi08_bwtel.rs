//! cgi08_bwtel — CGPU-F1287 带宽遥测（四源汇一，带宽瀑布每帧可拆）。
//!
//! 判据原文（施工书锚点 CGPU-F1287）：遥测：带宽遥测（账户消耗/仲裁决策/
//! 队列水位/瓶颈事件四源——遥测进 B08 总线复用 F0274 schema；带宽瀑布（每
//! 帧带宽消耗分解）——可视化数据对接 VE-Y；遥测开销 <1% 声明）。判据：四
//! 源、schema、瀑布、开销、判据。
//!
//! 职责定位：四源采集面（账户消耗源/cgi03 账本口径、仲裁决策源/cgi05 裁
//! 定口径、队列水位源/cgi06 水位口径、瓶颈事件源/触墙与超时事件）+ F0274
//! schema 版本化发布（B08 总线接缝——下游失配显性拒）+ 带宽瀑布（每帧四
//! 源消耗分解树——VE-Y 可视化对接面：账户×仲裁×队列×事件四维正交分解）+
//! 开销声明（遥测自身步数账/帧预算 <1%——遥测不能比被测的东西还贵）。红
//! 线：瀑布分解必须守恒（四源之和≡总消耗——分解不是编故事）；无事件帧的
//! 事件源显性空（不发零值事件谎报）。诊断码段 0x6001~0x6005。

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const BW_TEL_VERSION: &str = "I08-bwtel-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x60 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_SOURCE_UNKNOWN: u32 = 0x6001; // 四源闭集外（采集准入拒）
pub const XC_WATERFALL_LEAK: u32 = 0x6002; // 瀑布分解不守恒（四源和≠总消耗）
pub const XC_TEL_OVERHEAD: u32 = 0x6003; // 遥测开销 ≥1%（遥测比被测还贵）
pub const XC_SCHEMA_F0274_STALE: u32 = 0x6004; // F0274 schema 版本失配
pub const XC_FRAME_NOT_CLOSED: u32 = 0x6005; // 帧未闭合即发布（半帧数据谎报整帧）

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 四源闭集（判据"四源"）。
pub const BW_SOURCES: [&str; 4] = ["account", "arbiter", "queue", "event"];
/// F0274 schema 版本。
pub const BW_SCHEMA_V: u32 = 3;
/// 遥测开销红线：1% = 10_000 ppm。
pub const TEL_OVERHEAD_REDLINE_PPM: u64 = 10_000;
/// 帧预算步数代理（与 cgd13 同口径）。
pub const FRAME_BUDGET_STEPS: u64 = 1_250_000;
/// 帧瀑布桶容量。
pub const WATERFALL_CAP: usize = 64;

/// 带宽瀑布帧分解（判据"每帧带宽消耗分解"）。
#[derive(Clone, Debug, PartialEq)]
pub struct FrameWaterfall {
    pub frame_no: u64,
    /// 四源消耗（MB×100 定点——与 cgi03 账本口径一致）。
    pub by_source: [u64; 4],
    /// 总消耗（守恒锚：≡Σby_source）。
    pub total: u64,
    /// 步数开销（遥测自身——判据"开销 <1% 声明"）。
    pub tel_steps: u64,
    /// 事件明细数（事件源的事件条数——零=空帧事件显性）。
    pub event_count: u32,
}

/// 带宽遥测采集器。
pub struct BwTelemetry {
    /// 当前帧四源累计。
    cur: [u64; 4],
    cur_frame: u64,
    cur_events: u32,
    cur_steps: u64,
    frame_open: bool,
    /// 已闭合帧的瀑布册。
    waterfalls: Vec<FrameWaterfall>,
    /// 遥测步数总账。
    pub tel_steps_total: u64,
    /// 遥测计数。
    pub records: u64,
    pub rejected: u64,
}

impl BwTelemetry {
    pub fn new() -> Self {
        BwTelemetry {
            cur: [0; 4],
            cur_frame: 0,
            cur_events: 0,
            cur_steps: 0,
            frame_open: false,
            waterfalls: Vec::with_capacity(WATERFALL_CAP),
            tel_steps_total: 0,
            records: 0,
            rejected: 0,
        }
    }

    fn source_idx(src: &str) -> Option<usize> {
        let mut i = 0;
        while i < BW_SOURCES.len() {
            if BW_SOURCES[i] == src {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 帧开（采集窗起点）。
    pub fn begin_frame(&mut self, frame_no: u64) -> Result<(), u32> {
        if self.frame_open {
            return Err(XC_FRAME_NOT_CLOSED); // 上一帧没闭合就开新帧
        }
        self.cur = [0; 4];
        self.cur_frame = frame_no;
        self.cur_events = 0;
        self.cur_steps = 0;
        self.frame_open = true;
        self.tel_steps_total += 2;
        self.cur_steps += 2;
        Ok(())
    }

    /// 源消耗录入（判据"四源"——闭集准入）。
    pub fn observe(&mut self, source: &str, amount_mb100: u64) -> Result<(), u32> {
        if !self.frame_open {
            self.rejected += 1;
            return Err(XC_FRAME_NOT_CLOSED);
        }
        let idx = match Self::source_idx(source) {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return Err(XC_SOURCE_UNKNOWN);
            }
        };
        self.cur[idx] += amount_mb100;
        self.cur_steps += 1;
        self.tel_steps_total += 1;
        Ok(())
    }

    /// 事件录入（瓶颈事件——判据"瓶颈事件"源的事件明细）。
    pub fn observe_event(&mut self, what: &str) -> Result<(), u32> {
        if !self.frame_open {
            self.rejected += 1;
            return Err(XC_FRAME_NOT_CLOSED);
        }
        if what.len() == 0 {
            self.rejected += 1;
            return Err(XC_SOURCE_UNKNOWN);
        }
        self.cur_events += 1;
        self.cur_steps += 1;
        self.tel_steps_total += 1;
        Ok(())
    }

    /// 帧闭合（守恒核验+瀑布入册——判据"瀑布"）。
    pub fn close_frame(&mut self) -> Result<FrameWaterfall, u32> {
        if !self.frame_open {
            self.rejected += 1;
            return Err(XC_FRAME_NOT_CLOSED);
        }
        let total = self.cur[0] + self.cur[1] + self.cur[2] + self.cur[3];
        if self.waterfalls.len() >= WATERFALL_CAP {
            self.waterfalls.remove(0);
        }
        let wf = FrameWaterfall {
            frame_no: self.cur_frame,
            by_source: self.cur,
            total,
            tel_steps: self.cur_steps,
            event_count: self.cur_events,
        };
        self.waterfalls.push(wf.clone());
        self.records += 1;
        self.frame_open = false;
        self.tel_steps_total += 2;
        Ok(wf)
    }

    /// 守恒核验（判据核心：瀑布四源之和 ≡ total——分解不是编故事）。
    pub fn verify_waterfall(wf: &FrameWaterfall) -> Result<(), u32> {
        let sum = wf.by_source[0] + wf.by_source[1] + wf.by_source[2] + wf.by_source[3];
        if sum != wf.total {
            return Err(XC_WATERFALL_LEAK);
        }
        Ok(())
    }

    /// 开销声明核验（判据"遥测开销 <1%"）：册内最贵帧的步数比 < 1%。
    pub fn overhead_check(&self) -> Result<u64, u32> {
        let mut worst = 0u64;
        let mut i = 0;
        while i < self.waterfalls.len() {
            let wf = &self.waterfalls[i];
            if wf.tel_steps > worst {
                worst = wf.tel_steps;
            }
            i += 1;
        }
        let ppm = worst * 1_000_000 / FRAME_BUDGET_STEPS;
        if ppm >= TEL_OVERHEAD_REDLINE_PPM {
            return Err(XC_TEL_OVERHEAD);
        }
        Ok(ppm)
    }

    /// schema 核验（判据"复用 F0274 schema"）。
    pub fn check_schema(&self, v: u32) -> Result<(), u32> {
        if v != BW_SCHEMA_V {
            return Err(XC_SCHEMA_F0274_STALE);
        }
        Ok(())
    }

    /// 瀑布册查询（VE-Y 可视化对接面——按帧号）。
    pub fn waterfall_of(&self, frame_no: u64) -> Option<&FrameWaterfall> {
        let mut i = 0;
        while i < self.waterfalls.len() {
            if self.waterfalls[i].frame_no == frame_no {
                return Some(&self.waterfalls[i]);
            }
            i += 1;
        }
        None
    }

    /// 瀑布册全量（VE-Y 拉取面）。
    pub fn all_waterfalls(&self) -> &Vec<FrameWaterfall> {
        &self.waterfalls
    }

    /// 摘要。
    pub fn summary(&self) -> String {
        alloc::format!(
            "带宽遥测{} 帧记录{} 遥测步{} 拒{}",
            BW_TEL_VERSION,
            self.records,
            self.tel_steps_total,
            self.rejected,
        )
    }
}

// ---------------------------------------------------------------------------
// 自检（判据五组：四源 / schema / 瀑布守恒 / 开销 / 帧闭合）
// ---------------------------------------------------------------------------

impl BwTelemetry {
    /// 组一：四源采集——闭集外拒、未开帧拒。
    pub fn selfcheck_sources(&mut self) -> Result<(), u32> {
        let mut t = BwTelemetry::new();
        // 未开帧录入拒。
        if t.observe("account", 100) != Err(XC_FRAME_NOT_CLOSED) {
            return Err(XC_FRAME_NOT_CLOSED);
        }
        t.begin_frame(1)?;
        t.observe("account", 1200)?;
        t.observe("arbiter", 300)?;
        t.observe("queue", 200)?;
        t.observe("event", 50)?;
        if t.observe("mystery", 10) != Err(XC_SOURCE_UNKNOWN) || t.rejected != 1 {
            return Err(XC_SOURCE_UNKNOWN);
        }
        let wf = t.close_frame()?;
        if wf.by_source[0] != 1200 || wf.total != 1750 {
            return Err(XC_WATERFALL_LEAK);
        }
        Ok(())
    }

    /// 组二：schema——版本核验。
    pub fn selfcheck_schema(&mut self) -> Result<(), u32> {
        let t = BwTelemetry::new();
        t.check_schema(BW_SCHEMA_V)?;
        if t.check_schema(2) != Err(XC_SCHEMA_F0274_STALE) {
            return Err(XC_SCHEMA_F0274_STALE);
        }
        Ok(())
    }

    /// 组三：瀑布守恒——多帧分解守恒、篡改 total 即破守恒被查。
    pub fn selfcheck_waterfall(&mut self) -> Result<(), u32> {
        let mut t = BwTelemetry::new();
        let mut f = 1;
        while f <= 4 {
            t.begin_frame(f)?;
            t.observe("account", 1000 + f)?;
            t.observe("arbiter", 200)?;
            t.observe("queue", 50)?;
            t.observe_event("wall_hit")?;
            t.close_frame()?;
            f += 1;
        }
        let all = t.all_waterfalls();
        if all.len() != 4 {
            return Err(XC_WATERFALL_LEAK);
        }
        let mut i = 0;
        while i < all.len() {
            BwTelemetry::verify_waterfall(&all[i])?;
            if all[i].event_count != 1 {
                return Err(XC_SOURCE_UNKNOWN);
            }
            i += 1;
        }
        // 按帧号查询。
        if t.waterfall_of(2).is_none() || t.waterfall_of(99).is_some() {
            return Err(XC_WATERFALL_LEAK);
        }
        // 篡改守恒锚。
        let mut bad = all[0].clone();
        bad.total += 1;
        if BwTelemetry::verify_waterfall(&bad) != Err(XC_WATERFALL_LEAK) {
            return Err(XC_WATERFALL_LEAK);
        }
        Ok(())
    }

    /// 组四：开销——遥测步数比 <1%、超限模型拒。
    pub fn selfcheck_overhead(&mut self) -> Result<(), u32> {
        let mut t = BwTelemetry::new();
        t.begin_frame(1)?;
        // 16 次录入（16 步）+ 帧开闭 4 步 = 20 步 → 16 ppm << 1%。
        let mut i = 0;
        while i < 16 {
            t.observe("account", 10)?;
            i += 1;
        }
        t.close_frame()?;
        let ppm = t.overhead_check()?;
        if ppm >= TEL_OVERHEAD_REDLINE_PPM {
            return Err(XC_TEL_OVERHEAD);
        }
        // 超限模型：手工构造重开销帧（50000 步 → 40000 ppm 超红线）。
        let heavy = FrameWaterfall {
            frame_no: 99,
            by_source: [100, 0, 0, 0],
            total: 100,
            tel_steps: 50_000,
            event_count: 0,
        };
        let mut t2 = BwTelemetry::new();
        t2.waterfalls.push(heavy);
        if t2.overhead_check() != Err(XC_TEL_OVERHEAD) {
            return Err(XC_TEL_OVERHEAD);
        }
        Ok(())
    }

    /// 组五：帧闭合——未闭合发布拒、重开帧拒、FIFO 淘汰。
    pub fn selfcheck_close(&mut self) -> Result<(), u32> {
        let mut t = BwTelemetry::new();
        // close 未开帧。
        if t.close_frame() != Err(XC_FRAME_NOT_CLOSED) {
            return Err(XC_FRAME_NOT_CLOSED);
        }
        t.begin_frame(1)?;
        // 未闭合就重开拒。
        if t.begin_frame(2) != Err(XC_FRAME_NOT_CLOSED) {
            return Err(XC_FRAME_NOT_CLOSED);
        }
        t.close_frame()?;
        // FIFO：灌 70 帧只留 64。
        let mut f = 10;
        while f < 80 {
            t.begin_frame(f)?;
            t.observe("queue", 1)?;
            t.close_frame()?;
            f += 1;
        }
        if t.all_waterfalls().len() != WATERFALL_CAP {
            return Err(XC_WATERFALL_LEAK);
        }
        // 最老帧（10）已淘汰、最新帧（79）在册。
        if t.waterfall_of(10).is_some() || t.waterfall_of(79).is_none() {
            return Err(XC_WATERFALL_LEAK);
        }
        Ok(())
    }

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_sources()?;
        self.selfcheck_schema()?;
        self.selfcheck_waterfall()?;
        self.selfcheck_overhead()?;
        self.selfcheck_close()?;
        Ok(())
    }
}
