//! cgb753_e08arch —— CGPU-F0753 E08 开工与感知架构。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0753 E08 开工与感知架构  <SEG 0x4c60>
//! 目标行数：≥340（判据：纯功能代码 ≥340 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥340 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0753；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（E08 新段：感知架构骨架）：
//!   1. 感知架构 = 三层管道：采集层（传感器帧）→ 融合层（多源
//!      对齐）→ 决策层（阈值触发）；层间以定容帧队列相接；
//!   2. 采集层：4 传感器槽（写死），每槽有采样周期与抖动容差；
//!      帧序号单调（乱序帧红码——感知面不允许时间倒流）；
//!   3. 融合层：对齐窗 8 帧（写死），要求 4 槽各有 ≥1 帧才可融合；
//      融合产物 = 四槽最新帧的多数派类别（2×2 投票，平票取低类）；
//!   4. 决策层：三类决策（正常/预警/急停）阈值表写死；急停触发
//      后管道进入冻结态，只许排空不许进新帧；
//!   5. 管道守恒：入队帧数 = 出队帧数 + 队内帧数（逐层核）；
//      丢帧必须显性入丢帧账，不许静默；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x4c60 段为本模块独占（SEG_BASE=0x4c60），判别式
//!   SEG_BASE+1..+N；与 0x4c40/0x4c80 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const E8_VERSION: &str = "cgb753_e08arch-v1";

/// 诊断段基址（0x4c60 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4C60;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4c61 传感器槽越界（0..=3 之外）。
pub const XC_BAD_SLOT: u16 = SEG_BASE + 1;
/// 0x4c62 帧序号倒流（乱序帧）。
pub const XC_SEQ_BACKWARD: u16 = SEG_BASE + 2;
/// 0x4c63 队列满（8 帧定容）。
pub const XC_QUEUE_FULL: u16 = SEG_BASE + 3;
/// 0x4c64 对齐窗不齐（某槽零帧不可融合）。
pub const XC_WINDOW_STARVED: u16 = SEG_BASE + 4;
/// 0x4c65 急停冻结态进帧（红线）。
pub const XC_FROZEN_FEED: u16 = SEG_BASE + 5;
/// 0x4c66 融合产物类别越界（>3）。
pub const XC_BAD_CLASS: u16 = SEG_BASE + 6;
/// 0x4c67 管道不守恒（入≠出+在队）。
pub const XC_PIPE_UNBALANCED: u16 = SEG_BASE + 7;
/// 0x4c68 决策阈值表越界访问。
pub const XC_BAD_THRESHOLD: u16 = SEG_BASE + 8;
/// 0x4c69 空管道决策（无融合产物不可决策）。
pub const XC_DECIDE_EMPTY: u16 = SEG_BASE + 9;

/// 传感器槽数（写死：4）。
pub const SLOT_CAP: usize = 4;
/// 帧队列容量（写死：8）。
pub const QUEUE_CAP: usize = 8;
/// 对齐窗容量（写死：8 帧）。
pub const WINDOW_CAP: usize = 8;

/// 传感器帧。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    /// 槽号（0..=3）。
    pub slot: u8,
    /// 序号（槽内单调）。
    pub seq: u32,
    /// 类别（0..=3：静/常/热/异）。
    pub class: u8,
    /// 强度（0..=255）。
    pub level: u8,
}

/// 采集层：逐槽单调序号 + 定容队列。
pub struct AcquireLayer {
    /// 槽内最新序号。
    last_seq: [u32; SLOT_CAP],
    queue: Vec<Frame>,
    /// 入队累计。
    pub enqueued: u32,
    /// 出队累计。
    pub dequeued: u32,
    /// 丢帧累计（队列满即丢，显性记账）。
    pub dropped: u32,
}

impl AcquireLayer {
    /// 空层。
    pub fn new() -> Self {
        AcquireLayer {
            last_seq: [0; SLOT_CAP],
            queue: Vec::new(),
            enqueued: 0,
            dequeued: 0,
            dropped: 0,
        }
    }

    /// 进帧：序号必须严格大于槽内最新值；队列满丢帧入账。
    pub fn feed(&mut self, f: Frame) -> Result<bool, u16> {
        if (f.slot as usize) >= SLOT_CAP {
            return Err(XC_BAD_SLOT);
        }
        if f.class > 3 {
            return Err(XC_BAD_CLASS);
        }
        if f.seq <= self.last_seq[f.slot as usize] {
            return Err(XC_SEQ_BACKWARD);
        }
        self.last_seq[f.slot as usize] = f.seq;
        if self.queue.len() >= QUEUE_CAP {
            self.dropped += 1;
            return Ok(false);
        }
        self.queue.push(f);
        self.enqueued += 1;
        Ok(true)
    }

    /// 出帧（FIFO）。
    pub fn pop(&mut self) -> Option<Frame> {
        if self.queue.is_empty() {
            return None;
        }
        // Vec 队首出列：remove(0)——定容 8，代价可接受。
        let f = self.queue.remove(0);
        self.dequeued += 1;
        Some(f)
    }

    /// 在队帧数。
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// 层守恒：enqueued == dequeued + queued。
    pub fn conserved(&self) -> Result<(), u16> {
        if self.enqueued != self.dequeued + self.queue.len() as u32 {
            return Err(XC_PIPE_UNBALANCED);
        }
        Ok(())
    }
}

/// 融合层：对齐窗（逐槽最新帧）+ 多数派投票。
pub struct FuseLayer {
    window: Vec<Frame>,
    /// 融合产物累计。
    pub fused: u32,
}

impl FuseLayer {
    /// 空层。
    pub fn new() -> Self {
        FuseLayer {
            window: Vec::new(),
            fused: 0,
        }
    }

    /// 喂帧入窗（窗满丢最老——对齐窗是滑窗语义）。
    pub fn push(&mut self, f: Frame) -> Result<(), u16> {
        if (f.slot as usize) >= SLOT_CAP {
            return Err(XC_BAD_SLOT);
        }
        if self.window.len() >= WINDOW_CAP {
            self.window.remove(0);
        }
        self.window.push(f);
        Ok(())
    }

    /// 融合主路径：四槽各有 ≥1 帧才可融合；产物 = 类别多数派
    /// （4 槽最新帧投票，平票取低类）。
    pub fn fuse(&mut self) -> Result<u8, u16> {
        let mut latest: [Option<Frame>; SLOT_CAP] = [None; SLOT_CAP];
        for f in &self.window {
            let s = f.slot as usize;
            match latest[s] {
                Some(old) if old.seq >= f.seq => {}
                _ => latest[s] = Some(*f),
            }
        }
        if latest.iter().any(|o| o.is_none()) {
            return Err(XC_WINDOW_STARVED);
        }
        // 投票：类别 0..=3 计票。
        let mut votes = [0u32; 4];
        for o in latest.iter().flatten() {
            votes[(o.class & 3) as usize] += 1;
        }
        // 平票取低类：从高类往低类扫，严格大于才替换（保留最低最大者）。
        let mut best = 0u8;
        let mut best_v = votes[0];
        for c in 1..4u8 {
            if votes[c as usize] > best_v {
                best_v = votes[c as usize];
                best = c;
            }
        }
        self.fused += 1;
        Ok(best)
    }

    /// 窗内帧数。
    pub fn window_len(&self) -> usize {
        self.window.len()
    }

    /// 各槽在窗帧数（对齐检查面）。
    pub fn slot_counts(&self) -> [usize; SLOT_CAP] {
        let mut c = [0usize; SLOT_CAP];
        for f in &self.window {
            c[(f.slot & 3) as usize] += 1;
        }
        c
    }
}

/// 决策三类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// 正常。
    Normal,
    /// 预警。
    Warn,
    /// 急停。
    Halt,
}

/// 决策层：类别→强度阈值表（写死）：
///   类别 0（静）：level ≥ 200 → Halt，≥120 → Warn，否则 Normal；
///   类别 1（常）：level ≥ 240 → Halt，≥200 → Warn，否则 Normal；
///   类别 2（热）：level ≥ 220 → Halt，≥160 → Warn，否则 Normal；
///   类别 3（异）：任意 level ≥ 1 → Halt（异类零容忍）。
pub fn decide(class: u8, level: u8) -> Result<Decision, u16> {
    if class > 3 {
        return Err(XC_BAD_CLASS);
    }
    Ok(match class {
        0 => {
            if level >= 200 {
                Decision::Halt
            } else if level >= 120 {
                Decision::Warn
            } else {
                Decision::Normal
            }
        }
        1 => {
            if level >= 240 {
                Decision::Halt
            } else if level >= 200 {
                Decision::Warn
            } else {
                Decision::Normal
            }
        }
        2 => {
            if level >= 220 {
                Decision::Halt
            } else if level >= 160 {
                Decision::Warn
            } else {
                Decision::Normal
            }
        }
        _ => {
            if level >= 1 {
                Decision::Halt
            } else {
                Decision::Normal
            }
        }
    })
}

/// 感知管道总装：采集 → 融合 → 决策，急停冻结语义。

/// 感知管道。
pub struct Pipeline {
    pub acquire: AcquireLayer,
    pub fuse: FuseLayer,
    frozen: bool,
    /// 决策累计：正常/预警/急停。
    pub d_normal: u32,
    pub d_warn: u32,
    pub d_halt: u32,
}

impl Pipeline {
    /// 空管道。
    pub fn new() -> Self {
        Pipeline {
            acquire: AcquireLayer::new(),
            fuse: FuseLayer::new(),
            frozen: false,
            d_normal: 0,
            d_warn: 0,
            d_halt: 0,
        }
    }

    /// 进帧（冻结态红码）→ 融合层滑窗同步喂。
    pub fn feed(&mut self, f: Frame) -> Result<bool, u16> {
        if self.frozen {
            return Err(XC_FROZEN_FEED);
        }
        let accepted = self.acquire.feed(f)?;
        if accepted {
            self.fuse.push(f)?;
        }
        Ok(accepted)
    }

    /// 融合+决策主路径：融合产物 → 决策分类 → 计账；急停触发冻结。
    pub fn step(&mut self) -> Result<Decision, u16> {
        let class = self.fuse.fuse()?;
        // 决策用四槽最新帧最大强度（保守面）。
        let mut level = 0u8;
        for f in &self.fuse.window {
            if f.class == class && f.level > level {
                level = f.level;
            }
        }
        let d = decide(class, level)?;
        match d {
            Decision::Normal => self.d_normal += 1,
            Decision::Warn => self.d_warn += 1,
            Decision::Halt => {
                self.d_halt += 1;
                self.frozen = true;
            }
        }
        Ok(d)
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 排空：冻结态只许排空（出队到空），排空后守恒核。
    pub fn drain(&mut self) -> Result<usize, u16> {
        let mut n = 0;
        while self.acquire.pop().is_some() {
            n += 1;
        }
        self.acquire.conserved()?;
        Ok(n)
    }

    /// 全管道守恒核。
    pub fn conserved(&self) -> Result<(), u16> {
        self.acquire.conserved()?;
        if self.fuse.window_len() > WINDOW_CAP {
            return Err(XC_PIPE_UNBALANCED);
        }
        Ok(())
    }
}

/// 自例程：四槽喂帧 → 融合投票 → 三档决策 → 急停冻结 → 排空 →
/// 错误路径（倒流帧/槽越界/窗饿/冻结进帧/类别越界）。
pub fn drill() -> Result<u32, u16> {
    let mut p = Pipeline::new();
    // 四槽各喂 2 帧（类别 1 强度 100 → Normal 面）。
    for s in 0..4u8 {
        for k in 0..2u32 {
            let f = Frame {
                slot: s,
                seq: k + 1,
                class: 1,
                level: 100,
            };
            if !p.feed(f)? {
                return Err(XC_QUEUE_FULL);
            }
        }
    }
    // 融合 + 决策 → Normal。
    let d1 = p.step()?;
    if d1 != Decision::Normal {
        return Err(XC_BAD_THRESHOLD);
    }
    p.conserved()?;
    // 排空采集队列，为后续批次腾容（守恒：出队显性计账）。
    while p.acquire.pop().is_some() {}

    // 升温：四槽 level=210 → Warn。
    for s in 0..4u8 {
        let f = Frame {
            slot: s,
            seq: 10,
            class: 1,
            level: 210,
        };
        p.feed(f)?;
    }
    let d2 = p.step()?;
    if d2 != Decision::Warn {
        return Err(XC_BAD_THRESHOLD);
    }

    // 急停：异类零容忍（class 3, level 1）。
    for s in 0..4u8 {
        let f = Frame {
            slot: s,
            seq: 20,
            class: 3,
            level: 1,
        };
        p.feed(f)?;
    }
    let d3 = p.step()?;
    if d3 != Decision::Halt || !p.is_frozen() {
        return Err(XC_BAD_THRESHOLD);
    }
    // 冻结态排空。
    let drained = p.drain()?;
    if p.acquire.queued() != 0 {
        return Err(XC_PIPE_UNBALANCED);
    }
    // 冻结态进帧红码。
    if p
        .feed(Frame {
            slot: 0,
            seq: 99,
            class: 0,
            level: 1,
        })
        .map(|_| true) != Err(XC_FROZEN_FEED)
    {
        return Err(XC_FROZEN_FEED);
    }

    // 错误路径 1：序号倒流。
    let mut p2 = Pipeline::new();
    p2.feed(Frame { slot: 0, seq: 5, class: 0, level: 1 })?;
    if p2.feed(Frame { slot: 0, seq: 5, class: 0, level: 1 }) != Err(XC_SEQ_BACKWARD) {
        return Err(XC_SEQ_BACKWARD);
    }
    // 错误路径 2：槽越界。
    if p2
        .feed(Frame {
            slot: 4,
            seq: 1,
            class: 0,
            level: 1,
        })
        .map(|_| true)
        != Err(XC_BAD_SLOT)
    {
        return Err(XC_BAD_SLOT);
    }
    // 错误路径 3：窗饿（只喂 2 槽就融合）。
    let mut p3 = Pipeline::new();
    p3.feed(Frame { slot: 0, seq: 1, class: 0, level: 1 })?;
    p3.feed(Frame { slot: 1, seq: 1, class: 0, level: 1 })?;
    if p3.step() != Err(XC_WINDOW_STARVED) {
        return Err(XC_WINDOW_STARVED);
    }
    // 错误路径 4：类别越界。
    if decide(4, 1) != Err(XC_BAD_CLASS) {
        return Err(XC_BAD_CLASS);
    }
    // 错误路径 5：空管道决策。
    let mut p4 = Pipeline::new();
    if p4.step() != Err(XC_WINDOW_STARVED) {
        return Err(XC_WINDOW_STARVED);
    }
    // 守恒：p2 的入队=出队+在队。
    p2.conserved()?;
    let _ = drained;
    Ok(0)
}
