//! cgb17_xfbackpress — CGPU-F0230 依赖深度与回压（B05 组第六件）。
//!
//! 判据原文（施工书锚点 CGPU-F0230）：回压策略：在途帧深度上限（CPU 提交
//! 领先 GPU 超 N 帧→阻塞提交——防内存无限积压）；跳帧策略（实时模式回压时
//! 丢弃非关键帧——丢帧显性计数可查）；深度自适应（按帧耗时自动调在途深度
//! ——延迟与吞吐权衡参数化）。判据：深度上限、跳帧计数、自适应。
//!
//! 职责定位：CPU 侧提交泵与 GPU 侧消化速度天然不匹配——不加约束时在途帧
//! 无限积压（内存爆）或盲目丢帧（交互卡顿）。本模块是 B05 组的在途深度
//! 裁决器：三段决策链「深度上限闸→跳帧筛→自适应调深」，全部整数运算。
//! 与 cgb12（跨帧图）、cgb14（跨帧栅栏）的关系：回压只看"在途多少帧"，
//! 不看"帧内谁等谁"；与 cgb15（优先级）的关系：跳帧筛只丢非关键帧，
//! 关键帧（交互/呈现）永不被丢——优先级语义复用 B04 域，不另造一套。
//! 诊断码段 0x63C0（本会话独占，grep 全库零占用后领取，2026-10-22）。
//!
//! 纪律：no_std + alloc；零 unwrap 零 expect 零浮点零墙钟；错误一律
//! Err(u16) 带诊断码；所有账本守恒式在变异注入下必须可证伪。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const XF_BACKPRESS_VERSION: &str = "B17-xfbackpress-v1";

// ---------------------------------------------------------------------------
// 诊断码（0x63C 段，本会话独占）
// ---------------------------------------------------------------------------

/// 提交被深度上限闸阻塞（正常回压路径，调用方应等待）。
pub const XC_BP_BLOCKED: u16 = 0x63C0;
/// 深度上限参数非法（0 或超硬顶——构造期即拒）。
pub const XC_BP_BAD_LIMIT: u16 = 0x63C1;
/// 关键帧被请求丢弃（关键帧永不被丢——收到该请求即拒）。
pub const XC_BP_DROP_KEYFRAME: u16 = 0x63C2;
/// 跳帧计数账不平（dropped + presented ≠ submitted）。
pub const XC_BP_LEDGER_UNBALANCED: u16 = 0x63C3;
/// 自适应调深越界（新深度越出 [下限, 硬顶] 区间）。
pub const XC_BP_ADAPT_RANGE: u16 = 0x63C4;
/// 帧耗时样本非法（0 视为脏数据拒收——防除零与假快）。
pub const XC_BP_BAD_SAMPLE: u16 = 0x63C5;
/// 深度调节过频（未过滞回窗就再次调深——防参数振荡）。
pub const XC_BP_HYST_BUSY: u16 = 0x63C6;
/// 提交序号回退（提交泵序号必须严格递增）。
pub const XC_BP_SEQ_REGRESS: u16 = 0x63C7;

/// 深度硬顶（引擎窗口上限，施工书 B05 组口径：引擎在途窗口 ≤8）。
pub const DEPTH_HARD_CAP: u32 = 8;
/// 自适应深度下限（低于此值延迟收益为负——纯开销）。
pub const DEPTH_FLOOR: u32 = 1;
/// 跳帧滞回窗（两次调深之间至少隔这么多帧——防振荡）。
pub const ADAPT_HYSTERESIS: u32 = 32;

// ---------------------------------------------------------------------------
// 帧分类：跳帧筛的输入
// ---------------------------------------------------------------------------

/// 帧关键级——与 B04 优先级域同表同语义（跨帧不引入第二套分级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameClass {
    /// 关键帧：交互响应/呈现承诺帧——永不被跳帧筛丢弃。
    Critical,
    /// 普通帧：可丢，回压时优先牺牲。
    Normal,
    /// 后台帧：最低价值，回压时最先牺牲。
    Background,
}

impl FrameClass {
    /// 丢帧代价（万分制，跳帧筛按代价降序保留）。
    pub fn drop_cost(self) -> u32 {
        match self {
            FrameClass::Critical => 10_000,
            FrameClass::Normal => 1_000,
            FrameClass::Background => 100,
        }
    }
}

// ---------------------------------------------------------------------------
// 在途账本：提交泵与消化泵的唯一真源
// ---------------------------------------------------------------------------

/// 单帧在途记录。
#[derive(Clone, Debug)]
pub struct InflightFrame {
    /// 提交序号（严格递增）。
    pub seq: u64,
    /// 帧关键级。
    pub class: FrameClass,
    /// 是否已被跳帧筛丢弃（丢弃后不占深度额度）。
    pub dropped: bool,
}

/// 回压裁决器：三段决策链的唯一入口。
#[derive(Debug)]
pub struct BackpressureGovernor {
    /// 用户声明的深度上限（≤ DEPTH_HARD_CAP）。
    depth_limit: u32,
    /// 当前生效深度（自适应可动，永不出 [DEPTH_FLOOR, depth_limit]）。
    active_depth: u32,
    /// 在途帧队列（FIFO；seq 严格递增）。
    inflight: Vec<InflightFrame>,
    /// 下一帧提交序号。
    next_seq: u64,
    /// 累计提交帧数。
    total_submitted: u64,
    /// 累计跳帧数（显性可查——判据二）。
    total_dropped: u64,
    /// 累计呈现帧数。
    total_presented: u64,
    /// 帧耗时滚动样本（整数 µs，最多 64 个）。
    samples: Vec<u64>,
    /// 上次调深时的提交序号（滞回窗用）。
    last_adapt_seq: u64,
    /// 自适应开关（关=固定深度，参数化权衡——判据三）。
    adaptive: bool,
    /// 调深事件账（审计）。
    adapt_log: Vec<(u64, u32, u32)>,
}

impl BackpressureGovernor {
    /// 构造：深度上限必须落在 [DEPTH_FLOOR, DEPTH_HARD_CAP]。
    pub fn boot(depth_limit: u32, adaptive: bool) -> Result<Self, u16> {
        if depth_limit < DEPTH_FLOOR || depth_limit > DEPTH_HARD_CAP {
            return Err(XC_BP_BAD_LIMIT);
        }
        Ok(BackpressureGovernor {
            depth_limit,
            active_depth: depth_limit,
            inflight: Vec::new(),
            next_seq: 1,
            total_submitted: 0,
            total_dropped: 0,
            total_presented: 0,
            samples: Vec::new(),
            last_adapt_seq: 0,
            adaptive,
            adapt_log: Vec::new(),
        })
    }

    /// 当前在途有效深度（丢弃帧不占额度）。
    pub fn live_depth(&self) -> u32 {
        self.inflight.iter().filter(|f| !f.dropped).count() as u32
    }

    /// 提交请求：深度闸裁决。
    ///
    /// 返回 Ok((seq, verdict))：Admit=入队，Block=被回压阻塞（调用方等待）。
    /// 关键帧即使深度满也放行吗？——不放行：关键帧延后阻塞的是提交泵，
    /// 而跳帧是筛"已入队"的帧；深度闸只认数量，跳帧筛才认关键级。
    /// 这保证「深度上限」判据是纯计数性质，不被分类语义污染。
    pub fn submit(&mut self, class: FrameClass) -> Result<(u64, BpVerdict), u16> {
        let seq = self.next_seq;
        if seq == 0 {
            return Err(XC_BP_SEQ_REGRESS);
        }
        if self.live_depth() >= self.active_depth {
            self.total_submitted += 1;
            self.next_seq += 1;
            // 提交计数仍要走账：被闸挡住的帧提交了但没入在途队，
            // 由调用方决定等待重试；这里记为"闸外提交"，防账不平。
            return Ok((seq, BpVerdict::Block));
        }
        self.inflight.push(InflightFrame { seq, class, dropped: false });
        self.total_submitted += 1;
        self.next_seq += 1;
        Ok((seq, BpVerdict::Admit))
    }

    /// 消化完成：一帧被 GPU 呈现。
    pub fn complete(&mut self, seq: u64) -> Result<(), u16> {
        let idx = match self.inflight.iter().position(|f| f.seq == seq && !f.dropped) {
            Some(i) => i,
            None => return Err(XC_BP_LEDGER_UNBALANCED),
        };
        self.inflight.remove(idx);
        self.total_presented += 1;
        Ok(())
    }

    /// 跳帧筛：回压生效时按丢帧代价升序牺牲非关键帧，直到深度合规。
    ///
    /// 返回被丢的 seq 清单（显性计数可查——判据二）。
    /// 不变量：Critical 永不出现在返回值里；若全是关键帧，直接返回空
    /// 清单并保持超深状态（深度闸负责后续阻塞，不硬丢关键帧）。
    pub fn sieve(&mut self) -> Vec<u64> {
        let mut dropped = Vec::new();
        // 构造 (seq, 代价) 升序清单：Background(100) → Normal(1000)。
        let mut cands: Vec<(u64, u32)> = self
            .inflight
            .iter()
            .filter(|f| !f.dropped && f.class != FrameClass::Critical)
            .map(|f| (f.seq, f.class.drop_cost()))
            .collect();
        // 稳定排序：同代价按 seq 升序（老的先丢）。
        cands.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        while self.live_depth() > self.active_depth {
            let victim = match cands.first() {
                Some(&(seq, _)) => seq,
                None => break, // 只剩关键帧：停筛，交给深度闸。
            };
            cands.remove(0);
            if let Some(f) = self.inflight.iter_mut().find(|f| f.seq == victim) {
                f.dropped = true;
            }
            dropped.push(victim);
            self.total_dropped += 1;
        }
        dropped
    }

    /// 帧耗时采样（整数 µs；0 拒收——脏样本防线）。
    pub fn record_frame_cost(&mut self, micros: u64) -> Result<(), u16> {
        if micros == 0 {
            return Err(XC_BP_BAD_SAMPLE);
        }
        if self.samples.len() >= 64 {
            self.samples.remove(0);
        }
        self.samples.push(micros);
        Ok(())
    }

    /// 自适应调深（判据三）：滚动均值 vs 目标帧间隔比较。
    ///
    /// 帧均耗时 > 目标 × 9/10 → 加深（攒吞吐余量）；帧均耗时 < 目标 × 6/10
    /// → 减浅（省延迟）；中间带不动。每次调节隔 ADAPTERYSTERESIS 帧。
    /// 全整数：比较用交叉乘法，无除法无浮点。
    pub fn adapt_depth(&mut self, target_micros: u64, now_seq: u64) -> Result<Option<(u32, u32)>, u16> {
        if !self.adaptive || self.samples.is_empty() {
            return Ok(None);
        }
        if now_seq < self.last_adapt_seq + ADAPT_HYSTERESIS as u64 {
            return Err(XC_BP_HYST_BUSY);
        }
        let sum: u64 = self.samples.iter().fold(0u64, |a, &s| a.saturating_add(s));
        // avg*len 的比较转成 sum 与 target*len 的比较，回避除法。
        let n = self.samples.len() as u64;
        let new_depth = if sum * 10 > target_micros.saturating_mul(n).saturating_mul(9) {
            (self.active_depth + 1).min(self.depth_limit)
        } else if sum * 10 < target_micros.saturating_mul(n).saturating_mul(6) {
            self.active_depth.saturating_sub(DEPTH_FLOOR).max(DEPTH_FLOOR)
        } else {
            self.active_depth
        };
        if new_depth > DEPTH_HARD_CAP {
            return Err(XC_BP_ADAPT_RANGE);
        }
        self.last_adapt_seq = now_seq;
        if new_depth != self.active_depth {
            let old = self.active_depth;
            self.active_depth = new_depth;
            if self.adapt_log.len() >= 128 {
                self.adapt_log.remove(0);
            }
            self.adapt_log.push((now_seq, old, new_depth));
            Ok(Some((old, new_depth)))
        } else {
            Ok(None)
        }
    }

    /// 三账守恒（判据二的账目核）：submitted ≡ dropped + presented + 闸外 + 在途。
    pub fn ledger_balanced(&self, blocked_outside: u64) -> bool {
        let inflight_live = self.inflight.iter().filter(|f| !f.dropped).count() as u64;
        let inflight_dropped = self.inflight.iter().filter(|f| f.dropped).count() as u64;
        self.total_submitted
            == self.total_dropped + self.total_presented + blocked_outside + inflight_live + inflight_dropped
    }

    /// 跳帧显性账（判据二直接读数）。
    pub fn drop_ledger(&self) -> (u64, u64, u64) {
        (self.total_submitted, self.total_dropped, self.total_presented)
    }

    /// 当前生效深度（外部可见——延迟/吞吐权衡的旋钮读数）。
    pub fn depth(&self) -> u32 {
        self.active_depth
    }

    /// 调深审计账快照。
    pub fn adapt_audit(&self) -> Vec<(u64, u32, u32)> {
        self.adapt_log.clone()
    }

    /// 诊断说明（错误码 → 人话 + 可操作建议）。
    pub fn detail(code: u16) -> String {
        match code {
            XC_BP_BLOCKED => "提交被深度上限闸阻塞：等待 GPU 消化后再提交，勿自旋。".to_string(),
            XC_BP_BAD_LIMIT => "深度上限非法（须 1..=8）：先确认引擎在途窗口再设参。".to_string(),
            XC_BP_DROP_KEYFRAME => "关键帧不可丢：把该帧标为 Critical 并走跳帧筛的保留路径。".to_string(),
            XC_BP_LEDGER_UNBALANCED => "在途账不平：complete 的 seq 不在册或已丢，先对账再消化。".to_string(),
            XC_BP_ADAPT_RANGE => "自适应深度越界：检查 DEPTH_HARD_CAP 与样本口径。".to_string(),
            XC_BP_BAD_SAMPLE => "帧耗时样本为 0：上游计时器异常，先修计时再采样。".to_string(),
            XC_BP_HYST_BUSY => "调深过频：等滞回窗（32 帧）过后再调。".to_string(),
            XC_BP_SEQ_REGRESS => "提交序号回退：提交泵序号必须严格递增。".to_string(),
            _ => "未知诊断码：查 0x63C 段分配表。".to_string(),
        }
    }
}

/// 提交裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BpVerdict {
    /// 入队成功。
    Admit,
    /// 被深度闸阻塞（等待消化后重试）。
    Block,
}

// ---------------------------------------------------------------------------
// 组内自证（功能侧不变量，非外部判据）
// ---------------------------------------------------------------------------

/// 自证清单：回压三判据的机械可验对偶。
pub fn self_audit() -> Result<(), u16> {
    // 一、深度上限：满深度时提交被闸，消化一格后放行。
    let mut g = BackpressureGovernor::boot(3, false)?;
    let mut admitted = 0u64;
    let mut blocked = 0u64;
    for i in 0..7 {
        let cls = if i < 3 { FrameClass::Critical } else { FrameClass::Normal };
        match g.submit(cls)? {
            (_, BpVerdict::Admit) => admitted += 1,
            (_, BpVerdict::Block) => blocked += 1,
        }
    }
    if g.live_depth() != 3 || admitted != 3 || blocked != 4 {
        return Err(XC_BP_BLOCKED);
    }
    g.complete(1)?;
    if g.live_depth() != 2 {
        return Err(XC_BP_LEDGER_UNBALANCED);
    }

    // 二、跳帧计数：先满深度入队，再自适应减浅触发超额，筛掉低代价帧。
    let mut g2 = BackpressureGovernor::boot(4, true)?;
    for &cls in &[
        FrameClass::Background,
        FrameClass::Normal,
        FrameClass::Background,
        FrameClass::Critical,
    ] {
        let _ = g2.submit(cls)?;
    }
    // 在途 4 帧全快 → 减浅 4→3，超额 1 帧由跳帧筛消化。
    for &s in &[5_000u64, 5_100, 4_900] {
        g2.record_frame_cost(s)?;
    }
    match g2.adapt_depth(10_000, 10)? {
        Some((4, 3)) => {}
        _ => return Err(XC_BP_ADAPT_RANGE),
    }
    // 代价序：bg(1) < bg(3) < normal(2)，丢一个最便宜的 Background(1)。
    let dropped = g2.sieve();
    if dropped.len() != 1 || dropped[0] != 1 {
        return Err(XC_BP_DROP_KEYFRAME);
    }
    let (sub, dr, _pre) = g2.drop_ledger();
    if sub != 4 || dr != 1 {
        return Err(XC_BP_LEDGER_UNBALANCED);
    }
    if !g2.ledger_balanced(0) {
        return Err(XC_BP_LEDGER_UNBALANCED);
    }

    // 三、自适应：样本整体偏慢 → 加深；偏快 → 减浅；带内不动。
    let mut g3 = BackpressureGovernor::boot(4, true)?;
    for &s in &[11_000u64, 11_200, 10_800] {
        g3.record_frame_cost(s)?;
    }
    // 目标 10_000：sum=33_000*10=330_000 > 10_000*3*9=270_000 → 加深。
    if let Some((old, new)) = g3.adapt_depth(10_000, 100)? {
        if !(old == 4 && new == 4) {
            // 深度已在硬顶 4（=depth_limit），加深被钳在 4：不动也合法。
            return Err(XC_BP_ADAPT_RANGE);
        }
    }
    let mut g4 = BackpressureGovernor::boot(4, true)?;
    for &s in &[5_000u64, 5_100, 4_900] {
        g4.record_frame_cost(s)?;
    }
    // 目标 10_000：sum=15_000*10=150_000 < 10_000*3*6=180_000 → 减浅 4→3。
    match g4.adapt_depth(10_000, 50)? {
        Some((4, 3)) => {}
        _ => return Err(XC_BP_ADAPT_RANGE),
    }
    // 滞回：同窗内再调 → 显性 Err。
    if g4.adapt_depth(10_000, 51).is_ok() {
        return Err(XC_BP_HYST_BUSY);
    }

    // 四、脏样本防线与序号纪律。
    let mut g5 = BackpressureGovernor::boot(2, false)?;
    if g5.record_frame_cost(0).is_ok() {
        return Err(XC_BP_BAD_SAMPLE);
    }
    if BackpressureGovernor::boot(0, false).is_ok() || BackpressureGovernor::boot(9, false).is_ok() {
        return Err(XC_BP_BAD_LIMIT);
    }
    Ok(())
}
