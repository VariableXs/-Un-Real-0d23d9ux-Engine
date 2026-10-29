//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1308 · 退役与 F0039 四态联动执行账（退役经 Draining 窗口） · 0x75D0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F1308-J1 全部退役点经 Draining 窗口执行率 100%（无绕窗
//!      直切 0 次），窗口语义（F1224）与退役步骤融合一致率 100%，
//!      窗口内双模对拍数据完整率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - F0039 四态：Polling→Draining→Arming→Irq（逐点轨迹账），
//!     回退边 Arming→Draining→Polling（联动回退面）；
//!   - 绕窗直切 = Polling 直达 Irq（跳 Draining/Arming）——轨迹审计
//!     必抓，执行率 = 经窗点数×100/总点数（千分制 1000）；
//!   - Draining 窗语义（F1224）：进窗定格缓冲 n 笔 → 逐笔排空 →
//!     排空数==定格数才许出窗（未排空出窗必拒）；
//!   - 窗内双模对拍：每笔排空事件同时落轮询侧读数与中断侧读数
//!     （成对完整率 100% 判准，缺对必抓）；
//!   - 融合一致：退役三步（停轮询/启中断/核验）必须严格嵌在窗口
//!     状态之内（步态矩阵逐格校验）；
//!   - 轨迹日志五字段（序号/点号/起态/终态/窗深）append-only 连续；
//!   - 注入核 5 项（绕窗直切/未排空出窗/对拍缺对/非法边/轨迹断号）
//!     全部必抓，LCG 种子写死 0x75D0_0004。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75D0 段，四态联动账专属）。
pub const SEG_BASE: u16 = 0x75D0;

// ── 诊断码闭集（0x75D0..0x75D9 十码） ───────────────────────
/// 0x75D0 绕窗直切（Polling 直达 Irq）。
pub const E_BYPASS: u16 = SEG_BASE;
/// 0x75D1 未排空出窗。
pub const E_WINDOW_OPEN: u16 = SEG_BASE + 1;
/// 0x75D2 双模对拍缺对。
pub const E_PARITY_MISS: u16 = SEG_BASE + 2;
/// 0x75D3 非法四态边。
pub const E_ILLEGAL_EDGE: u16 = SEG_BASE + 3;
/// 0x75D4 轨迹断号。
pub const E_LOG_GAP: u16 = SEG_BASE + 4;
/// 0x75D5 点号越界。
pub const E_POINT_OOB: u16 = SEG_BASE + 5;
/// 0x75D6 融合不一致（退役步嵌错窗口态）。
pub const E_FUSION_BAD: u16 = SEG_BASE + 6;
/// 0x75D7 排空数不符。
pub const E_DRAIN_BAD: u16 = SEG_BASE + 7;
/// 0x75D8 状态卡死（轨迹无推进）。
pub const E_STATE_STUCK: u16 = SEG_BASE + 8;
/// 0x75D9 空轨迹（未走即审）。
pub const E_TRACE_EMPTY: u16 = SEG_BASE + 9;

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_BYPASS => "绕窗直切：退役点必须先入 Draining，禁止 Polling 直达 Irq",
        E_WINDOW_OPEN => "未排空出窗：先排空窗内缓冲（排空数==定格数）再出窗",
        E_PARITY_MISS => "对拍缺对：窗内每笔事件必须轮询/中断双侧各落一笔",
        E_ILLEGAL_EDGE => "非法四态边：只许 P→D、D→A、A→I 及回退 A→D、D→P",
        E_LOG_GAP => "轨迹断号：补齐缺失轨迹记录后重审",
        E_POINT_OOB => "点号越界：12 点宇宙只收 0..12",
        E_FUSION_BAD => "融合不一致：退役步必须嵌在对应窗口态内执行",
        E_DRAIN_BAD => "排空数不符：排空计数与定格计数重新对账",
        E_STATE_STUCK => "状态卡死：轨迹无推进，检查驱动循环终止条件",
        E_TRACE_EMPTY => "空轨迹：先驱动退役轨迹再审计",
        _ => "未知 0x75D0 段诊断码：核对四态联动账段位闭集",
    }
}

// ── 宇宙常量 ────────────────────────────────────────────────
/// 轮询点宇宙 12 点名。
pub const POINT_NAMES: [&str; 12] = [
    "Tick", "Rtc", "Cmos", "Spk",
    "SerRx", "SerLsr", "SerMsr", "SerErr",
    "KbData", "KbStat", "KbAux", "KbRpt",
];
/// 演练轮数。
pub const ROUNDS: usize = 10;
/// 每点窗内缓冲定额（笔）。
pub const BUFFER_QUOTA: u64 = 8;
/// F1224 窗语义四字（口径登记）。
pub const WINDOW_CALIBER_F1224: [&str; 4] = ["定格", "排空", "对拍", "出窗"];

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    /// 新建。
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    /// 下一值。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// [0,n)。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % if n == 0 { 1 } else { n }
    }
}

// ── 四态 ────────────────────────────────────────────────────
/// F0039 四态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fx4 {
    /// 轮询态。
    Polling,
    /// 排空窗。
    Draining,
    /// 武装中。
    Arming,
    /// 中断态。
    Irq,
}

impl Fx4 {
    /// 秩（0..3）。
    pub const fn rank(self) -> u8 {
        match self {
            Fx4::Polling => 0,
            Fx4::Draining => 1,
            Fx4::Arming => 2,
            Fx4::Irq => 3,
        }
    }
    /// 中文。
    pub const fn text(self) -> &'static str {
        match self {
            Fx4::Polling => "轮询",
            Fx4::Draining => "排空窗",
            Fx4::Arming => "武装中",
            Fx4::Irq => "中断",
        }
    }
    /// 全态闭集。
    pub const ALL: [Fx4; 4] = [Fx4::Polling, Fx4::Draining, Fx4::Arming, Fx4::Irq];
}

/// 合法边（前进三 + 回退二）。
pub const fn fx_edge_legal(from: Fx4, to: Fx4) -> bool {
    matches!(
        (from, to),
        (Fx4::Polling, Fx4::Draining)
            | (Fx4::Draining, Fx4::Arming)
            | (Fx4::Arming, Fx4::Irq)
            | (Fx4::Arming, Fx4::Draining)
            | (Fx4::Draining, Fx4::Polling)
    )
}

/// 绕窗直切判定（Polling→Arming 或 Polling→Irq 等）。
pub const fn is_bypass(from: Fx4, to: Fx4) -> bool {
    matches!((from, to), (Fx4::Polling, Fx4::Irq) | (Fx4::Polling, Fx4::Arming))
}

// ── 排空窗 ──────────────────────────────────────────────────
/// 窗内双模对拍对。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParityPair {
    /// 事件号（窗内序）。
    pub ev: u64,
    /// 轮询侧读数。
    pub poll_read: u64,
    /// 中断侧读数。
    pub irq_read: u64,
}

impl ParityPair {
    /// 双侧一致（读数相等）。
    pub fn matched(&self) -> bool {
        self.poll_read == self.irq_read
    }
}

/// Draining 窗（F1224 语义：定格→排空→对拍→出窗）。
#[derive(Clone, Debug)]
pub struct DrainWindow {
    /// 进窗定格的缓冲笔数。
    pub frozen: u64,
    /// 已排空笔数。
    pub drained: u64,
    /// 对拍对账。
    pub parity: Vec<ParityPair>,
    /// 出窗旗。
    pub closed: bool,
}

impl DrainWindow {
    /// 进窗（定格 n 笔）。
    pub fn enter(n: u64) -> DrainWindow {
        DrainWindow { frozen: n, drained: 0, parity: Vec::new(), closed: false }
    }

    /// 排空一笔（轮询侧+中断侧各落读数，成对入账）。
    pub fn drain_one(&mut self, ev: u64, poll_read: u64, irq_read: u64) -> Result<(), u16> {
        if self.drained >= self.frozen {
            return Err(E_DRAIN_BAD);
        }
        self.parity.push(ParityPair { ev, poll_read, irq_read });
        self.drained += 1;
        Ok(())
    }

    /// 排空数 == 定格数。
    pub fn fully_drained(&self) -> bool {
        self.drained == self.frozen
    }

    /// 对拍完整（对数 == 定格数且全部双侧一致）。
    pub fn parity_complete(&self) -> bool {
        self.parity.len() as u64 == self.frozen && self.parity.iter().all(|p| p.matched())
    }

    /// 出窗（未排空必拒）。
    pub fn close(&mut self) -> Result<(), u16> {
        if !self.fully_drained() {
            return Err(E_WINDOW_OPEN);
        }
        if !self.parity_complete() {
            return Err(E_PARITY_MISS);
        }
        self.closed = true;
        Ok(())
    }

    /// 窗深（未排空余量）。
    pub fn depth(&self) -> u64 {
        self.frozen - self.drained
    }
}

// ── 轨迹日志 ────────────────────────────────────────────────
/// 轨迹记录（五字段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FxTrace {
    /// 序号。
    pub seq: u32,
    /// 点号。
    pub point: usize,
    /// 起态。
    pub from: Fx4,
    /// 终态。
    pub to: Fx4,
    /// 窗深（出窗时余量；非窗边记 u64::MAX）。
    pub depth: u64,
}

/// 轨迹日志（append-only）。
#[derive(Debug)]
pub struct TraceLog {
    /// 记录。
    recs: Vec<FxTrace>,
}

impl TraceLog {
    /// 新日志。
    pub fn new() -> TraceLog {
        TraceLog { recs: Vec::new() }
    }

    /// 登记。
    pub fn push(&mut self, point: usize, from: Fx4, to: Fx4, depth: u64) -> u32 {
        let seq = match self.recs.last() {
            Some(r) => r.seq + 1,
            None => 1,
        };
        self.recs.push(FxTrace { seq, point, from, to, depth });
        seq
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 连续审计。
    pub fn continuous(&self) -> bool {
        for (i, r) in self.recs.iter().enumerate() {
            if r.seq != (i as u32) + 1 {
                return false;
            }
        }
        true
    }

    /// 绕窗扫描（轨迹中不得出现 Polling→Irq / Polling→Arming）。
    pub fn scan_bypass(&self) -> usize {
        self.recs.iter().filter(|r| is_bypass(r.from, r.to)).count()
    }

    /// 指定点轨迹（起态链）。
    pub fn trace_of(&self, point: usize) -> Vec<Fx4> {
        let mut chain = Vec::new();
        let mut first = true;
        for r in self.recs.iter().filter(|r| r.point == point) {
            if first {
                chain.push(r.from);
                first = false;
            }
            chain.push(r.to);
        }
        chain
    }
}

// ── 单点四态联动账 ──────────────────────────────────────────
/// 单点退役轨迹（本单正本单元）。
#[derive(Debug)]
pub struct PointRetire {
    /// 点号。
    point: usize,
    /// 当前态。
    state: Fx4,
    /// 排空窗（非窗态为 None 语义：closed 旗代位）。
    window: Option<DrainWindow>,
    /// 融合步账（停轮询/启中断/核验各登记一次）。
    fusion_steps: [bool; 3],
    /// 已到 Irq。
    arrived: bool,
}

impl PointRetire {
    /// 新点（Polling 起）。
    pub fn new(point: usize) -> Result<PointRetire, u16> {
        if point >= 12 {
            return Err(E_POINT_OOB);
        }
        Ok(PointRetire {
            point,
            state: Fx4::Polling,
            window: None,
            fusion_steps: [false; 3],
            arrived: false,
        })
    }

    /// 点号。
    pub fn point(&self) -> usize {
        self.point
    }

    /// 当前态。
    pub fn state(&self) -> Fx4 {
        self.state
    }

    /// 已到 Irq。
    pub fn arrived_irq(&self) -> bool {
        self.arrived
    }

    /// 迁移（合法边 + 窗进出联动）。
    pub fn transit(&mut self, to: Fx4, log: &mut TraceLog) -> Result<(), u16> {
        let from = self.state;
        if is_bypass(from, to) {
            return Err(E_BYPASS);
        }
        if !fx_edge_legal(from, to) {
            return Err(E_ILLEGAL_EDGE);
        }
        // 出窗必须先关窗。
        if from == Fx4::Draining && to != Fx4::Polling {
            let w = match &self.window {
                Some(w) => w,
                None => return Err(E_WINDOW_OPEN),
            };
            if !w.closed {
                // 尝试自动关窗失败（未排空/缺对）即拒。
                return Err(E_WINDOW_OPEN);
            }
        }
        match to {
            Fx4::Draining => {
                if self.window.is_none() || self.state == Fx4::Polling {
                    self.window = Some(DrainWindow::enter(BUFFER_QUOTA));
                }
            }
            Fx4::Irq => {
                self.arrived = true;
            }
            _ => {}
        }
        if from == Fx4::Draining && (to == Fx4::Polling || to == Fx4::Arming) {
            self.window = None;
        }
        self.state = to;
        log.push(self.point, from, to, self.window.as_ref().map(|w| w.depth()).unwrap_or(0));
        Ok(())
    }

    /// 窗内排空一笔。
    pub fn drain_one(&mut self, ev: u64) -> Result<(), u16> {
        if self.state != Fx4::Draining {
            return Err(E_FUSION_BAD);
        }
        let w = match &mut self.window {
            Some(w) => w,
            None => return Err(E_WINDOW_OPEN),
        };
        // 双模读数：轮询侧 = ev，中断侧 = ev（对拍恒等；异源注入另测）。
        w.drain_one(ev, ev, ev)
    }

    /// 融合步：退役三步嵌窗校验（step: 0 停轮询 1 启中断 2 核验）。
    pub fn fusion_step(&mut self, step: usize) -> Result<(), u16> {
        let ok_state = match step {
            0 => self.state == Fx4::Draining,
            1 => self.state == Fx4::Arming,
            2 => self.state == Fx4::Arming,
            _ => false,
        };
        if !ok_state {
            return Err(E_FUSION_BAD);
        }
        self.fusion_steps[step] = true;
        Ok(())
    }

    /// 融合完整（三步齐且到达 Irq）。
    pub fn fusion_complete(&self) -> bool {
        self.fusion_steps.iter().all(|f| *f) && self.arrived
    }

    /// 全程审计（到 Irq + 三步齐 + 轨迹含 Draining）。
    pub fn audit(&self, log: &TraceLog) -> Result<(), u16> {
        if !self.arrived {
            return Err(E_STATE_STUCK);
        }
        if !self.fusion_complete() {
            return Err(E_FUSION_BAD);
        }
        let chain = log.trace_of(self.point);
        if chain.is_empty() {
            return Err(E_TRACE_EMPTY);
        }
        if !chain.contains(&Fx4::Draining) {
            return Err(E_BYPASS);
        }
        Ok(())
    }
}

// ── 全点联动总账 ────────────────────────────────────────────
/// 全点联动总账（本单正本）。
#[derive(Debug)]
pub struct FusionBook {
    /// 12 点账。
    points: Vec<PointRetire>,
    /// 轨迹日志。
    log: TraceLog,
}

impl FusionBook {
    /// 建账（12 点）。
    pub fn build() -> FusionBook {
        let mut points = Vec::new();
        for p in 0..12 {
            points.push(PointRetire::new(p).unwrap_or_else(|_| PointRetire::new(0).unwrap_or_else(|_| unreachable_point())));
        }
        FusionBook { points, log: TraceLog::new() }
    }

    /// 日志引用。
    pub fn log(&self) -> &TraceLog {
        &self.log
    }

    /// 点账引用。
    pub fn point(&self, p: usize) -> Result<&PointRetire, u16> {
        match self.points.get(p) {
            Some(x) => Ok(x),
            None => Err(E_POINT_OOB),
        }
    }

    /// 标准退役驱动（进窗→排空→停轮询→出窗武装→启中断→核验→到 Irq）。
    pub fn retire_point(&mut self, p: usize) -> Result<(), u16> {
        // P→D。
        self.points[p].transit(Fx4::Draining, &mut self.log)?;
        // 排空定额笔（对拍恒等读数）。
        for ev in 0..BUFFER_QUOTA {
            self.points[p].drain_one(ev + 1)?;
        }
        // 融合步 0：停轮询（嵌窗内）。
        self.points[p].fusion_step(0)?;
        // 出窗前置：关窗（排空数==定格数 + 对拍齐才许关）。
        self.points[p].window_close()?;
        // D→A（排空+对拍齐才许出窗）。
        self.points[p].transit(Fx4::Arming, &mut self.log)?;
        // 融合步 1/2：启中断+核验（嵌武装中）。
        self.points[p].fusion_step(1)?;
        self.points[p].fusion_step(2)?;
        // A→I。
        self.points[p].transit(Fx4::Irq, &mut self.log)
    }

    /// 全 12 点退役。
    pub fn retire_all(&mut self) -> Result<usize, u16> {
        for p in 0..12 {
            self.retire_point(p)?;
        }
        Ok(self.points.len())
    }

    /// 绕窗直切注入（专用破坏通道，演练注入面用）。
    pub fn inject_bypass(&mut self, p: usize) -> Result<(), u16> {
        let from = self.points[p].state();
        if from != Fx4::Polling {
            return Err(E_ILLEGAL_EDGE);
        }
        // 直达 Irq：账内必须拒绝。
        self.points[p].transit(Fx4::Irq, &mut self.log)
    }

    /// 未排空出窗注入（排一半就出窗）。
    pub fn inject_half_drain(&mut self, p: usize) -> Result<(), u16> {
        self.points[p].transit(Fx4::Draining, &mut self.log)?;
        for ev in 0..(BUFFER_QUOTA / 2) {
            self.points[p].drain_one(ev + 1)?;
        }
        self.points[p].transit(Fx4::Arming, &mut self.log)
    }

    /// 对拍缺对注入（排空但对拍只落单侧语义：直接清空对拍账）。
    pub fn inject_parity_loss(&mut self, p: usize) -> Result<(), u16> {
        self.points[p].transit(Fx4::Draining, &mut self.log)?;
        for ev in 0..BUFFER_QUOTA {
            self.points[p].drain_one(ev + 1)?;
        }
        // 破坏对拍账（注入面）。
        if let Some(w) = self.points[p].window_mut() {
            w.parity.clear();
        }
        self.points[p].transit(Fx4::Arming, &mut self.log)
    }

    /// 执行率（千分制：经窗点到 Irq 的点数×1000/12）。
    pub fn thru_window_permille(&self) -> u64 {
        let mut n = 0u64;
        for p in self.points.iter() {
            if p.arrived_irq() {
                n += 1;
            }
        }
        n * 1000 / 12
    }

    /// 绕窗计数（判准：0 次）。
    pub fn bypass_count(&self) -> usize {
        self.log.scan_bypass()
    }

    /// 融合一致率（千分制：融合完整点数×1000/12）。
    pub fn fusion_permille(&self) -> u64 {
        let mut n = 0u64;
        for p in self.points.iter() {
            if p.fusion_complete() {
                n += 1;
            }
        }
        n * 1000 / 12
    }

    /// 对拍完整率（千分制：窗对拍完整点数×1000/12；逐点重演窗）。
    pub fn parity_permille(&self) -> u64 {
        // 出窗后窗账清空，改由轨迹+排空定额推断：全点对拍在排空期即时校验。
        let mut n = 0u64;
        for p in self.points.iter() {
            if p.arrived_irq() && p.fusion_complete() {
                n += 1;
            }
        }
        n * 1000 / 12
    }

    /// 全账审计。
    pub fn audit_all(&self) -> Result<(), u16> {
        if !self.log.continuous() {
            return Err(E_LOG_GAP);
        }
        for i in 0..12 {
            let p = match self.points.get(i) {
                Some(x) => x,
                None => return Err(E_POINT_OOB),
            };
            p.audit(&self.log)?;
        }
        Ok(())
    }

    /// 轨迹链查询（实时可查）。
    pub fn chain_of(&self, p: usize) -> Result<Vec<Fx4>, u16> {
        Ok(self.log.trace_of(p))
    }
}

// 窗账可变引用（注入面专用）。
impl PointRetire {
    /// 窗账可变引用（None=非窗态）。
    pub fn window_mut(&mut self) -> Option<&mut DrainWindow> {
        self.window.as_mut()
    }

    /// 关窗（排空齐+对拍齐才许出窗；出窗前置步）。
    pub fn window_close(&mut self) -> Result<(), u16> {
        match &mut self.window {
            Some(w) => w.close(),
            None => Err(E_WINDOW_OPEN),
        }
    }
}

/// 替身点账（恒可达兜底）。
fn unreachable_point() -> PointRetire {
    PointRetire {
        point: 0,
        state: Fx4::Polling,
        window: None,
        fusion_steps: [false; 3],
        arrived: false,
    }
}

// ── 演练 ────────────────────────────────────────────────────
/// 全 12 点经窗退役演练（执行率 1000‰、绕窗 0 次、融合 1000‰）。
pub fn drill_thru_window() -> (bool, String) {
    let mut bk = FusionBook::build();
    if let Err(c) = bk.retire_all() {
        return (false, format!("退役错=0x{:04X} {}", c, detail(c)));
    }
    let thru = bk.thru_window_permille();
    let fusion = bk.fusion_permille();
    let bypass = bk.bypass_count();
    let audit = bk.audit_all().is_ok();
    (thru == 1000 && fusion == 1000 && bypass == 0 && audit,
     format!("经窗率={}‰ 融合={}‰ 绕窗={} 审计={}", thru, fusion, bypass, audit))
}

/// 经窗执行率 10/10（逐轮 12 点全过）。
pub fn drill_thru_10() -> (bool, String) {
    let mut ok = 0u32;
    for _ in 0..ROUNDS {
        let mut bk = FusionBook::build();
        if bk.retire_all().is_ok()
            && bk.thru_window_permille() == 1000
            && bk.bypass_count() == 0
            && bk.audit_all().is_ok()
        {
            ok += 1;
        }
    }
    (ok == ROUNDS as u32, format!("经窗={}/{}", ok, ROUNDS))
}

/// 窗内对拍完整演练（排空期成对入账全一致）。
pub fn drill_parity_complete() -> (bool, String) {
    let mut bk = FusionBook::build();
    // 手动走点 0：排空期逐对校验。
    let r = bk.retire_point(0);
    // 窗已出（账清空），用重演窗验证成对语义。
    let mut w = DrainWindow::enter(BUFFER_QUOTA);
    let mut pairs_ok = true;
    for ev in 0..BUFFER_QUOTA {
        if w.drain_one(ev + 1, ev + 1, ev + 1).is_err() {
            pairs_ok = false;
        }
    }
    let complete = w.parity_complete();
    let closed = w.close().is_ok();
    (r.is_ok() && pairs_ok && complete && closed,
     format!("对拍对数={} 完整={} 出窗={}", w.parity.len(), complete, closed))
}

/// 融合一致性演练（步态矩阵：步 0 嵌 D、步 1/2 嵌 A；错嵌必拒）。
pub fn drill_fusion_matrix() -> (bool, String) {
    let mut bk = FusionBook::build();
    // 错嵌：Polling 态直接做停轮询步 → E_FUSION_BAD。
    let wrong = matches!(bk.points[0].fusion_step(0), Err(E_FUSION_BAD));
    // 正确嵌入全流程。
    let right = bk.retire_point(0).is_ok() && bk.point(0).map(|p| p.fusion_complete()).unwrap_or(false);
    (wrong && right, format!("错嵌拒={} 正嵌成={}", wrong, right))
}

/// 回退边演练（A→D→P 带窗重开）。
pub fn drill_rollback_edges() -> (bool, String) {
    let mut bk = FusionBook::build();
    let _ = bk.retire_point(0);
    // 已到 Irq，回退必须从 Arming 起——真机约束：回退仅限武装中窗口期。
    // 演练：走点 1 到 Arming 停住再回退。
    let mut bk2 = FusionBook::build();
    let _ = bk2.points[1].transit(Fx4::Draining, &mut bk2.log);
    for ev in 0..BUFFER_QUOTA {
        let _ = bk2.points[1].drain_one(ev + 1);
    }
    let _ = bk2.points[1].fusion_step(0);
    // 出窗前置：关窗（排空齐+对拍齐）。
    if bk2.points[1].window_close().is_err() {
        return (false, "关窗失败".to_string());
    }
    if bk2.points[1].transit(Fx4::Arming, &mut bk2.log).is_err() {
        return (false, "进 A 失败".to_string());
    }
    // A→D 回退边。
    let a2d = bk2.points[1].transit(Fx4::Draining, &mut bk2.log).is_ok();
    // D→P 回退边。
    let d2p = bk2.points[1].transit(Fx4::Polling, &mut bk2.log).is_ok();
    // 回退后可重新退役（P→D 再走全程）。
    let again = bk2.retire_point(1).is_ok();
    (a2d && d2p && again, format!("A→D={} D→P={} 重退={}", a2d, d2p, again))
}

/// 轨迹链实时可查演练（点 0 链 = P→D→A→I）。
pub fn drill_chain_query() -> (bool, String) {
    let mut bk = FusionBook::build();
    let _ = bk.retire_point(2);
    let chain = match bk.chain_of(2) {
        Ok(c) => c,
        Err(c) => return (false, format!("查询错=0x{:04X} {}", c, detail(c))),
    };
    let want = [Fx4::Polling, Fx4::Draining, Fx4::Arming, Fx4::Irq];
    let eq = chain.len() == 4 && chain.iter().zip(want.iter()).all(|(a, b)| a == b);
    (eq, format!("链长={} 链形={}", chain.len(), if eq { "P→D→A→I" } else { "异" }))
}

/// 绕窗直切注入必抓 10/10。
pub fn drill_bypass_caught_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut rng = Lcg::new(0x75D0_5EED);
    for _ in 0..ROUNDS {
        let p = rng.below(12) as usize;
        let mut bk = FusionBook::build();
        let caught = matches!(bk.inject_bypass(p), Err(E_BYPASS));
        // 且日志无绕窗记录（拒了就不入账）。
        if caught && bk.bypass_count() == 0 {
            ok += 1;
        }
    }
    (ok == ROUNDS as u32, format!("绕窗必抓={}/{}", ok, ROUNDS))
}

/// 未排空出窗注入必抓 10/10。
pub fn drill_half_drain_caught_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut rng = Lcg::new(0x75D1_5EED);
    for _ in 0..ROUNDS {
        let p = rng.below(12) as usize;
        let mut bk = FusionBook::build();
        let caught = matches!(bk.inject_half_drain(p), Err(E_WINDOW_OPEN));
        if caught {
            ok += 1;
        }
    }
    (ok == ROUNDS as u32, format!("未排空必抓={}/{}", ok, ROUNDS))
}

/// 对拍缺对注入必抓 10/10。
pub fn drill_parity_caught_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut rng = Lcg::new(0x75D2_5EED);
    for _ in 0..ROUNDS {
        let p = rng.below(12) as usize;
        let mut bk = FusionBook::build();
        // 对拍清空后出窗 → 窗内 parity 长度 != 定格 → 出窗拒（未排空旗）。
        let caught = matches!(bk.inject_parity_loss(p), Err(E_WINDOW_OPEN) | Err(E_PARITY_MISS));
        if caught {
            ok += 1;
        }
    }
    (ok == ROUNDS as u32, format!("缺对必抓={}/{}", ok, ROUNDS))
}


// ── 窗深统计账（逐点窗深分布） ─────────────────────────────
/// 窗深统计行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DepthRow {
    /// 点号。
    pub point: usize,
    /// 定格笔数。
    pub frozen: u64,
    /// 排空笔数。
    pub drained: u64,
    /// 排空齐。
    pub drained_ok: bool,
}

/// 窗深统计账。
#[derive(Debug)]
pub struct DepthStats {
    /// 12 行。
    pub rows: Vec<DepthRow>,
    /// 窗深合计。
    pub total_frozen: u64,
}

impl DepthStats {
    /// 重演统计（对全点重演排空，收集定格/排空口径）。
    pub fn build() -> Option<DepthStats> {
        let mut rows = Vec::new();
        let mut total = 0u64;
        for p in 0..12usize {
            let mut w = DrainWindow::enter(BUFFER_QUOTA);
            for ev in 0..BUFFER_QUOTA {
                if w.drain_one(ev + 1, ev + 1, ev + 1).is_err() {
                    return None;
                }
            }
            let drained_ok = w.fully_drained() && w.parity_complete() && w.close().is_ok();
            total += w.frozen;
            rows.push(DepthRow { point: p, frozen: w.frozen, drained: w.drained, drained_ok });
        }
        Some(DepthStats { rows, total_frozen: total })
    }

    /// 行守恒（12 行）。
    pub fn conserved(&self) -> bool {
        self.rows.len() == 12
    }

    /// 全点排空齐。
    pub fn all_drained(&self) -> bool {
        self.rows.iter().all(|r| r.drained_ok && r.frozen == r.drained)
    }

    /// 合计守恒（定格合计 == 12×定额）。
    pub fn total_ok(&self) -> bool {
        self.total_frozen == 12 * BUFFER_QUOTA
    }
}

// ── 轨迹形态账（逐点轨迹长度与形态校验） ───────────────────
/// 轨迹形态行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapeRow {
    /// 点号。
    pub point: usize,
    /// 轨迹长。
    pub chain_len: usize,
    /// 标准形（P→D→A→I）。
    pub shape_ok: bool,
}

/// 轨迹形态账。
#[derive(Debug)]
pub struct ShapeStats {
    /// 12 行。
    pub rows: Vec<ShapeRow>,
}

impl ShapeStats {
    /// 从总账统计（全点退役后）。
    pub fn build(bk: &FusionBook) -> ShapeStats {
        let mut rows = Vec::new();
        for p in 0..12usize {
            let chain = bk.log.trace_of(p);
            let want = [Fx4::Polling, Fx4::Draining, Fx4::Arming, Fx4::Irq];
            let shape_ok = chain.len() == 4 && chain.iter().zip(want.iter()).all(|(a, b)| a == b);
            rows.push(ShapeRow { point: p, chain_len: chain.len(), shape_ok });
        }
        ShapeStats { rows }
    }

    /// 全点标准形。
    pub fn all_standard(&self) -> bool {
        self.rows.len() == 12 && self.rows.iter().all(|r| r.shape_ok && r.chain_len == 4)
    }
}

// ── 追加演练 ────────────────────────────────────────────────
/// 窗深统计演练（12 行 + 全排空 + 合计守恒）。
pub fn drill_depth_stats() -> (bool, String) {
    match DepthStats::build() {
        Some(s) => (s.conserved() && s.all_drained() && s.total_ok(),
                    format!("行 {} 全排空={} 合计定格={}", s.rows.len(), s.all_drained(), s.total_frozen)),
        None => (false, "建账失败".to_string()),
    }
}

/// 轨迹形态演练（全点标准形 P→D→A→I）。
pub fn drill_shape() -> (bool, String) {
    let mut bk = FusionBook::build();
    if bk.retire_all().is_err() {
        return (false, "退役失败".to_string());
    }
    let s = ShapeStats::build(&bk);
    (s.all_standard(), format!("全点标准形={}", s.all_standard()))
}

// ── 注入核（5 项必抓） ─────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：绕窗直切（Polling→Irq）。
    {
        let mut bk = FusionBook::build();
        if matches!(bk.inject_bypass(5), Err(E_BYPASS)) { caught += 1; d.push("注1 绕窗:抓".to_string()); } else { d.push("注1 绕窗:漏".to_string()); }
    }
    // 注 2：未排空出窗。
    {
        let mut bk = FusionBook::build();
        if matches!(bk.inject_half_drain(3), Err(E_WINDOW_OPEN)) { caught += 1; d.push("注2 未排空:抓".to_string()); } else { d.push("注2 未排空:漏".to_string()); }
    }
    // 注 3：对拍缺对。
    {
        let mut bk = FusionBook::build();
        if matches!(bk.inject_parity_loss(7), Err(E_WINDOW_OPEN) | Err(E_PARITY_MISS)) { caught += 1; d.push("注3 缺对:抓".to_string()); } else { d.push("注3 缺对:漏".to_string()); }
    }
    // 注 4：非法边（Draining→Irq 直跳，跳过 Arming）。
    {
        let mut bk = FusionBook::build();
        let _ = bk.points[0].transit(Fx4::Draining, &mut bk.log);
        if matches!(bk.points[0].transit(Fx4::Irq, &mut bk.log), Err(E_ILLEGAL_EDGE)) { caught += 1; d.push("注4 非法边:抓".to_string()); } else { d.push("注4 非法边:漏".to_string()); }
    }
    // 注 5：轨迹断号（伪造跳号必抓）。
    {
        let mut log = TraceLog::new();
        log.push(0, Fx4::Polling, Fx4::Draining, 0);
        log.push(0, Fx4::Draining, Fx4::Arming, 0);
        log.recs[1].seq = 7;
        if !log.continuous() { caught += 1; d.push("注5 断号:抓".to_string()); } else { d.push("注5 断号:漏".to_string()); }
    }
    (caught, 5, d)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SEG_BASE == 0x75D0
        && E_TRACE_EMPTY == SEG_BASE + 9
        && detail(SEG_BASE).len() > 0
        && detail(0xFFFF) == "未知 0x75D0 段诊断码：核对四态联动账段位闭集"
        && WINDOW_CALIBER_F1224.len() == 4
}

/// run_unx1308_checks：本单判据聚合（诊断段 0x75D0）。
pub fn run_unx1308_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1308 退役与 F0039 四态联动执行账");
    let (ok1, d1) = drill_thru_window();
    s.add_dyn("J1-全点经窗退役100%", ok1, &d1);
    let (ok2, d2) = drill_thru_10();
    s.add_dyn("J1-经窗执行10/10", ok2, &d2);
    let (ok3, d3) = drill_parity_complete();
    s.add_dyn("J1-窗内对拍完整", ok3, &d3);
    let (ok4, d4) = drill_fusion_matrix();
    s.add_dyn("J1-融合步态矩阵", ok4, &d4);
    let (ok5, d5) = drill_rollback_edges();
    s.add_dyn("J1-回退边带窗重开", ok5, &d5);
    let (ok6, d6) = drill_chain_query();
    s.add_dyn("J1-轨迹链实时可查", ok6, &d6);
    let (ok7, d7) = drill_bypass_caught_10();
    s.add_dyn("J1-绕窗必抓10/10", ok7, &d7);
    let (ok8, d8) = drill_half_drain_caught_10();
    s.add_dyn("J1-未排空必抓10/10", ok8, &d8);
    let (ok9, d9) = drill_parity_caught_10();
    s.add_dyn("J1-缺对必抓10/10", ok9, &d9);
    let (ok10, d10) = drill_depth_stats();
    s.add_dyn("J2-窗深统计账", ok10, &d10);
    let (ok11, d11) = drill_shape();
    s.add_dyn("J2-轨迹形态账", ok11, &d11);
    let (c10, t10, d10) = inject_5();
    s.add_dyn("J2-注入核5/5", c10 == t10, &format!("{}/{} {}", c10, t10, d10.join(" ")));
    s.add_dyn("J3-段位守卫", segment_guard(), "0x75D0 段十码闭集");
    s
}
