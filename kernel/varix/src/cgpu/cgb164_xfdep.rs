//! cgb164_xfdep.rs —— CGPU-F0164 · 跨帧依赖与围栏传播
//!
//! 任务跨帧引用的处理：上一帧的解码任务在下一帧使用——
//! - 跨帧边：上帧任务围栏 → 本帧任务等待；
//! - 围栏传播链：A 帧依赖 B 帧，B 依赖 C——链式传播；
//! - 跨帧超时：上帧任务 N 帧未完成 → 标记丢失 → 本帧走降级；
//! - 跨帧依赖的图持久化（跨帧边不随帧图释放）。
//! 注意：与 cgb14_xffence（F0227 跨帧围栏）是两册——本册是
//! 依赖语义与传播链，那册是围栏对象本体。
//! 诊断码段 0x0430（本席 WB1022 分配表，全域唯一）。
//! no_std + alloc 零 unwrap、零 panic 路径。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0430;

/// 跨帧等待上限（判据「跨 3 帧用例」硬线）：等待跨度超过此帧数
/// 即判丢失。
pub const MAX_WAIT_FRAMES: u32 = 3;

/// 一条跨帧依赖（持久化单元）。
#[derive(Clone, Copy, Debug)]
pub struct XFrameDep {
    /// 生产帧号。
    pub from_frame: u64,
    /// 生产任务号。
    pub from_task: u32,
    /// 消费帧号。
    pub to_frame: u64,
    /// 消费任务号。
    pub to_task: u32,
}

impl XFrameDep {
    /// 自检：生产帧严格早于消费帧、任务号非零。
    pub fn validate(&self) -> Result<(), u16> {
        if self.from_task == 0 || self.to_task == 0 {
            return Err(SEG_BASE + 1);
        }
        if self.to_frame <= self.from_frame {
            return Err(SEG_BASE + 2);
        }
        if self.to_frame - self.from_frame > MAX_WAIT_FRAMES as u64 {
            return Err(SEG_BASE + 3);
        }
        Ok(())
    }
}

/// 围栏状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FenceState {
    /// 生产帧已提交，未完成。
    Pending,
    /// 已完成（信号）。
    Signaled,
    /// 超时丢失（判据「超时降级」）。
    Lost,
}

/// 围栏账（判据「跨帧等待正确」）。
pub struct FenceLedger {
    /// 生产端围栏：(帧号, 任务号) → 状态。
    rows: Vec<(u64, u32, FenceState)>,
}

impl FenceLedger {
    /// 空账。
    pub fn new() -> Self {
        FenceLedger { rows: Vec::new() }
    }

    /// 生产端登记（Pending）。
    pub fn produce(&mut self, frame: u64, task: u32) -> Result<(), u16> {
        if task == 0 {
            return Err(SEG_BASE + 4);
        }
        if self
            .rows
            .iter()
            .any(|(f, t, _)| *f == frame && *t == task)
        {
            return Err(SEG_BASE + 5);
        }
        self.rows.push((frame, task, FenceState::Pending));
        Ok(())
    }

    /// 完成信号。
    pub fn signal(&mut self, frame: u64, task: u32) -> Result<(), u16> {
        let r = self
            .rows
            .iter_mut()
            .find(|(f, t, _)| *f == frame && *t == task)
            .ok_or(SEG_BASE + 6)?;
        if r.2 == FenceState::Signaled {
            return Err(SEG_BASE + 7);
        }
        r.2 = FenceState::Signaled;
        Ok(())
    }

    /// 状态查询。
    pub fn state_of(&self, frame: u64, task: u32) -> Option<FenceState> {
        self.rows
            .iter()
            .find(|(f, t, _)| *f == frame && *t == task)
            .map(|(_, _, s)| *s)
    }

    /// 超时扫描（判据「跨帧超时」）：Pending 且当前帧已越过
    /// 生产帧 + MAX_WAIT_FRAMES → 标记 Lost。返回本轮标记数。
    pub fn sweep_timeout(&mut self, cur_frame: u64) -> Result<u32, u16> {
        let mut n = 0;
        for r in self.rows.iter_mut() {
            if r.2 == FenceState::Pending
                && cur_frame >= r.0 + MAX_WAIT_FRAMES as u64
            {
                r.2 = FenceState::Lost;
                n += 1;
            }
        }
        Ok(n)
    }

    /// 状态普查。
    pub fn census(&self) -> (usize, usize, usize) {
        (
            self.rows.iter().filter(|r| r.2 == FenceState::Pending).count(),
            self.rows.iter().filter(|r| r.2 == FenceState::Signaled).count(),
            self.rows.iter().filter(|r| r.2 == FenceState::Lost).count(),
        )
    }

    /// 账行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// 跨帧依赖账（判据「图持久化」：跨帧边不随帧图释放——本账
/// 独立于帧图生命周期）。
pub struct XFrameDepBook {
    /// 持久化依赖边。
    deps: Vec<XFrameDep>,
    /// 已降级（走降级路径）的边键集（from_frame<<32|from_task）。
    degraded: Vec<u64>,
    /// 归档。
    archived: Option<u64>,
}

impl XFrameDepBook {
    /// 空账。
    pub fn new() -> Self {
        XFrameDepBook {
            deps: Vec::new(),
            degraded: Vec::new(),
            archived: None,
        }
    }

    /// 依赖边数。
    pub fn len(&self) -> usize {
        self.deps.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.deps.is_empty()
    }

    /// 降级边数。
    pub fn degraded_count(&self) -> usize {
        self.degraded.len()
    }

    /// 登记跨帧依赖（判据「持久化正确」入口）。
    pub fn add(&mut self, d: XFrameDep) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 8);
        }
        d.validate()?;
        if self.deps.iter().any(|e| {
            e.from_frame == d.from_frame
                && e.from_task == d.from_task
                && e.to_frame == d.to_frame
                && e.to_task == d.to_task
        }) {
            return Err(SEG_BASE + 9);
        }
        self.deps.push(d);
        Ok(())
    }

    /// 消费等待判定（判据「跨帧等待正确」）：消费任务 t 在帧 f
    /// 开工前，查其全部上溯围栏——全 Signal 才可开工；有 Pending
    /// 则等待；有 Lost 则必须走降级（登记 degraded）。
    pub fn consume_check(
        &mut self,
        fences: &FenceLedger,
        to_frame: u64,
        to_task: u32,
    ) -> Result<ConsumeVerdict, u16> {
        let mine: Vec<&XFrameDep> = self
            .deps
            .iter()
            .filter(|d| d.to_frame == to_frame && d.to_task == to_task)
            .collect();
        if mine.is_empty() {
            return Ok(ConsumeVerdict::NoDeps);
        }
        let mut all_signaled = true;
        let mut any_lost = false;
        for d in &mine {
            match fences.state_of(d.from_frame, d.from_task) {
                Some(FenceState::Signaled) => {}
                Some(FenceState::Lost) => any_lost = true,
                Some(FenceState::Pending) | None => all_signaled = false,
            }
        }
        if any_lost {
            for d in &mine {
                let key = (d.from_frame << 32) | (d.from_task as u64);
                if !self.degraded.contains(&key) {
                    self.degraded.push(key);
                }
            }
            return Ok(ConsumeVerdict::Degrade);
        }
        if all_signaled {
            Ok(ConsumeVerdict::Go)
        } else {
            Ok(ConsumeVerdict::Wait)
        }
    }

    /// 链式传播（判据「链式传播」）：从 (frame, task) 出发沿
    /// 「谁消费我」方向递归展开消费链（BFS，环不可能——帧号
    /// 严格递增保证 DAG）。
    pub fn propagate_chain(&self, frame: u64, task: u32) -> Result<Vec<(u64, u32)>, u16> {
        if task == 0 {
            return Err(SEG_BASE + 10);
        }
        let mut chain: Vec<(u64, u32)> = Vec::new();
        let mut frontier: Vec<(u64, u32)> = vec![(frame, task)];
        while let Some((f, t)) = frontier.pop() {
            for d in &self.deps {
                if d.from_frame == f && d.from_task == t {
                    let nxt = (d.to_frame, d.to_task);
                    if !chain.contains(&nxt) && !(nxt == (frame, task)) {
                        chain.push(nxt);
                        frontier.push(nxt);
                    }
                }
            }
        }
        Ok(chain)
    }

    /// 链深断言（判据「跨 3 帧用例」）：任一传播链跨度不超上限。
    pub fn chain_depth_ok(&self) -> Result<(), u16> {
        for d in &self.deps {
            if d.to_frame - d.from_frame > MAX_WAIT_FRAMES as u64 {
                return Err(SEG_BASE + 11);
            }
        }
        Ok(())
    }

    /// 归档（判据「持久化正确」收口）：依赖非空 + 深度合规 +
    /// 降级边可追溯（降级键必须能对回账内边），三关全过签章。
    pub fn archive(&mut self, canon: &[u8]) -> Result<u64, u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 12);
        }
        if self.deps.is_empty() {
            return Err(SEG_BASE + 13);
        }
        self.chain_depth_ok()?;
        for k in &self.degraded {
            let f = k >> 32;
            let t = (k & 0xFFFF_FFFF) as u32;
            if !self
                .deps
                .iter()
                .any(|d| d.from_frame == f && d.from_task == t)
            {
                return Err(SEG_BASE + 14);
            }
        }
        let mut fp: u64 = 0xcbf29ce484222325;
        for d in &self.deps {
            fp = fp.wrapping_mul(0x100000001b3) ^ d.from_frame;
            fp = fp.wrapping_mul(0x100000001b3) ^ d.to_frame;
        }
        for &b in canon {
            fp = fp.wrapping_mul(0x100000001b3) ^ (b as u64);
        }
        if fp == 0 {
            return Err(SEG_BASE + 15);
        }
        self.archived = Some(fp);
        Ok(fp)
    }

    /// 归档指纹。
    pub fn archive_fp(&self) -> Option<u64> {
        self.archived
    }
}

impl Default for XFrameDepBook {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for FenceLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 消费判定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsumeVerdict {
    /// 无跨帧依赖，直接开工。
    NoDeps,
    /// 围栏全信号，开工。
    Go,
    /// 有围栏未决，等待。
    Wait,
    /// 围栏丢失，走降级。
    Degrade,
}

/// 三帧链用例校验（判据「跨 3 帧用例」专用面）：给定跨 3 帧的
/// 依赖序列（帧 N 生产 → N+1 中转 → N+2 终点），校验全链合法。
pub fn three_frame_case(
    book: &XFrameDepBook,
    fences: &FenceLedger,
    base_frame: u64,
    prod_task: u32,
) -> Result<usize, u16> {
    if prod_task == 0 {
        return Err(SEG_BASE + 16);
    }
    let chain = book.propagate_chain(base_frame, prod_task)?;
    if chain.len() < 2 {
        return Err(SEG_BASE + 17);
    }
    let span = chain.last().unwrap_or(&(0, 0)).0.saturating_sub(base_frame);
    if span > MAX_WAIT_FRAMES as u64 {
        return Err(SEG_BASE + 18);
    }
    let _ = fences;
    Ok(chain.len())
}

impl FenceLedger {
    /// 围栏指纹（判据「持久化正确」的账面摘要）。
    pub fn ledger_fp(&self) -> u64 {
        let mut acc: u64 = 0xcbf29ce484222325;
        for (f, t, s) in &self.rows {
            acc = acc.wrapping_mul(0x100000001b3) ^ *f;
            acc = acc.wrapping_mul(0x100000001b3) ^ (*t as u64);
            acc = acc.wrapping_mul(0x100000001b3) ^ (s.clone() as u64);
        }
        acc
    }

    /// 帧内未决清单（判据「跨帧等待正确」可见性）：指定帧的
    /// Pending 围栏。
    pub fn pending_in_frame(&self, frame: u64) -> Vec<u32> {
        self.rows
            .iter()
            .filter(|(f, _, s)| *f == frame && *s == FenceState::Pending)
            .map(|(_, t, _)| *t)
            .collect()
    }
}

impl XFrameDepBook {
    /// 帧跨度普查（判据「跨 3 帧用例」量化面）：边跨度分布。
    pub fn span_census(&self) -> [usize; 4] {
        let mut out = [0usize; 4];
        for d in &self.deps {
            let s = (d.to_frame - d.from_frame) as usize;
            if s >= 1 && s <= 3 {
                out[s] += 1;
            }
        }
        out
    }
}
