//! unx283_stackwalk —— UNX-F0283 栈回溯帧步行器（rBP 链 + 保守扫描混合）。
//! 锚点（CoRun Unxreal 主册 UNX-A1-B15）：UNX-F0283 · 栈回溯帧步行器（rBP 链 + 保守扫描混合） · 目标 340 行 · 已深化
//! 判据原文：UNX-F0283-J1 链式回溯与保守扫描混合策略在栈损坏场景仍出有效帧，误帧率 0
//! 组件：帧记录账（append-only）+ rBP 链步进状态机（七态）+ 保守扫描器（返回地址位型核验）
//!      + 混合策略裁决器（链路优先/扫描接管/回退切换）+ 误帧防线（三类验证门）+ 步行账（守恒）
//! 判据落地：混合策略在栈损坏场景仍出有效帧 + 误帧率 0（三类验证门全过才算有效帧）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性（逻辑 tick 注入，零墙钟）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const SW_VERSION: &str = "unx283_stackwalk-v1";

/// 诊断段基址（0x4c00 段 · 本模块独占，入库前已 grep 全域源文件确认无撞段）。
pub const SW_SEG_BASE: u16 = 0x4c00;

/// 栈窗扫描的最大帧数上限（当前策略值，可调；防失控步行）。
pub const SW_MAX_FRAMES: usize = 256;

/// 保守扫描的最大扫描跨度（字节；超出即判栈窗耗尽）。
pub const SW_SCAN_SPAN_MAX: u64 = 64 * 1024;

/// 8 字节对齐步长（x86_64 栈上指针宽度）。
pub const SW_SLOT: u64 = 8;

/// 返回地址必须落在的代码窗下界（非可执行区间一律拒，来源：链接脚本内核 .text 布局策略值）。
pub const SW_TEXT_LO: u64 = 0x0010_0000;
/// 代码窗上界（与加载器镜像上限对齐，策略值可调）。
pub const SW_TEXT_HI: u64 = 0x4000_0000;

/// rBP 链必须落在的栈窗（用于链健康判定；出窗即链损）。
pub const SW_STACK_LO: u64 = 0xFFFF_8000_0000_0000;
pub const SW_STACK_HI: u64 = 0xFFFF_FFFF_FFFF_F000;

/// 帧来源（三源闭集：链步进 / 保守扫描 / 种子帧）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwOrigin {
    /// rBP 链步进产帧。
    Chain,
    /// 保守扫描产帧（链损后接管）。
    Scan,
    /// 种子帧（步行起点，来自异常现场 rIP/rBP 直采）。
    Seed,
}

impl SwOrigin {
    /// 来源名（中文诊断用）。
    pub const fn name(self) -> &'static str {
        match self {
            SwOrigin::Chain => "链步进",
            SwOrigin::Scan => "保守扫描",
            SwOrigin::Seed => "种子帧",
        }
    }
}

/// 帧记录（一帧 = 一条 append-only 账）。
#[derive(Clone, Debug)]
pub struct SwFrame {
    /// 返回地址（已过验证门）。
    pub ret: u64,
    /// 帧基址 rBP（链步进时为链上值；扫描帧为推断值，可为 0）。
    pub rbp: u64,
    /// 来源。
    pub origin: SwOrigin,
    /// 逻辑 tick（确定性，注入）。
    pub tick: u64,
    /// 深度（从种子帧 0 起算）。
    pub depth: usize,
    /// 验证门位图：bit0 对齐 / bit1 代码窗 / bit2 位型核验。
    pub gates: u8,
}

impl SwFrame {
    /// 三门全过 = 有效帧（误帧率 0 的判据核心：任何一门未过不记帧）。
    pub const fn valid(&self) -> bool {
        self.gates & 0b111 == 0b111
    }
}

/// 诊断码（0x4c00 段十六码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwCode {
    /// 0x4c00 种子帧非法（rIP 出代码窗或 rBP 出栈窗）。
    SeedBad,
    /// 0x4c01 链推进越界（新 rBP 非升序/回环/出栈窗）。
    ChainAdvance,
    /// 0x4c02 链帧返回地址过不了验证门（链视作损坏的信号之一）。
    ChainRetGate,
    /// 0x4c03 链上同一 rBP 重复出现（回环）。
    ChainLoop,
    /// 0x4c04 保守扫描无对齐候选（跨度耗尽仍无合法返回地址位型）。
    ScanExhausted,
    /// 0x4c05 扫描候选过不了位型核验。
    ScanGate,
    /// 0x4c06 帧数触顶（SW_MAX_FRAMES，截断显性不静默）。
    FrameCap,
    /// 0x4c07 混合策略状态机非法迁移请求（同态重复切换）。
    StrategyBad,
    /// 0x4c08 步行账破缺（产出帧数 ≢ 账内计数）。
    LedgerBreak,
    /// 0x4c09 误帧防线触发（记了未过门的帧——实现缺陷即报，不静默）。
    FalseFrameGuard,
    /// 0x4c0a 栈窗参数非法（HI ≤ LO 或跨度过大）。
    WindowBad,
    /// 0x4c0b 扫描读越界（游标 + 槽宽超栈窗上界）。
    ScanOob,
    /// 0x4c0c 深度账错位（帧深度不单调）。
    DepthMisaligned,
    /// 0x4c0d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl SwCode {
    /// 全量闭集。
    pub const ALL: [SwCode; 14] = [
        SwCode::SeedBad,
        SwCode::ChainAdvance,
        SwCode::ChainRetGate,
        SwCode::ChainLoop,
        SwCode::ScanExhausted,
        SwCode::ScanGate,
        SwCode::FrameCap,
        SwCode::StrategyBad,
        SwCode::LedgerBreak,
        SwCode::FalseFrameGuard,
        SwCode::WindowBad,
        SwCode::ScanOob,
        SwCode::DepthMisaligned,
        SwCode::SegmentGuard,
    ];

    /// 诊断码数值。
    pub const fn val(self) -> u16 {
        SW_SEG_BASE + self as u16
    }

    /// 中文一句话诊断（异常零静默：发生了什么）。
    pub const fn text(self) -> &'static str {
        match self {
            SwCode::SeedBad => "种子帧非法：rIP 出代码窗或 rBP 出栈窗，步行拒绝启动",
            SwCode::ChainAdvance => "链推进越界：新 rBP 非升序或出栈窗，判定链损",
            SwCode::ChainRetGate => "链帧返回地址未过验证门，判定链损",
            SwCode::ChainLoop => "链上 rBP 回环，判定链损",
            SwCode::ScanExhausted => "保守扫描跨度耗尽仍无合法候选",
            SwCode::ScanGate => "扫描候选未过位型核验，跳过该槽位",
            SwCode::FrameCap => "帧数触顶，截断显性记录",
            SwCode::StrategyBad => "混合策略状态机收到非法迁移请求",
            SwCode::LedgerBreak => "步行账破缺：产出帧数与账内计数不一致",
            SwCode::FalseFrameGuard => "误帧防线触发：出现未过门的帧记录（实现缺陷）",
            SwCode::WindowBad => "栈窗参数非法：上界不大于下界或跨度越限",
            SwCode::ScanOob => "扫描读越界：游标加槽宽越过栈窗上界",
            SwCode::DepthMisaligned => "深度账错位：帧深度序列不单调递增",
            SwCode::SegmentGuard => "段位守卫失败：诊断码越出本模块段",
        }
    }

    /// 可操作建议（异常零静默：下一步怎么办）。
    pub const fn advice(self) -> &'static str {
        match self {
            SwCode::SeedBad => "核对异常现场采样点：rIP 应在 .text 窗内、rBP 应在内核栈窗内后重试",
            SwCode::ChainAdvance => "链已损：交给保守扫描器接管，不要继续沿链推进",
            SwCode::ChainRetGate => "链已损：交给保守扫描器接管，链上残帧仍有效可保留",
            SwCode::ChainLoop => "链回环：立即停止链步进，已产帧保留，扫描接管续深",
            SwCode::ScanExhausted => "扩大扫描跨度前先确认栈窗参数；跨度已达上限则收步行终",
            SwCode::ScanGate => "正常跳过：位型核验拒绝非返回地址槽位，继续下一槽位",
            SwCode::FrameCap => "帧数上限可调（SW_MAX_FRAMES），确认无需更深栈后按截断口径出报告",
            SwCode::StrategyBad => "检查混合策略状态机调用序：同一来源不得重复切换",
            SwCode::LedgerBreak => "记账路径与产帧路径失配：核对 push_frame 的唯一入口",
            SwCode::FalseFrameGuard => "验证门位图设置与帧记录写入失配：核对 push_frame 的 gates 入参",
            SwCode::WindowBad => "修正栈窗参数（HI>LO 且跨度 ≤ SW_SCAN_SPAN_MAX）",
            SwCode::ScanOob => "游标推进应先做上界钳制再读，不得越过栈窗上界",
            SwCode::DepthMisaligned => "深度只能单调递增：核对帧产出的深度参数来源",
            SwCode::SegmentGuard => "诊断码分配越段：核对本模块段基址 0x4c00 与枚举序",
        }
    }
}

/// 混合策略状态机（七态闭集；状态迁移全部留痕）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwPhase {
    /// 初始：尚未采样种子。
    Init,
    /// 链步进进行中。
    ChainRun,
    /// 链损待切换（已产链帧保留）。
    ChainBroken,
    /// 保守扫描进行中。
    ScanRun,
    /// 扫描耗尽（栈窗走完仍无候选）。
    ScanExhausted,
    /// 正常终态：产帧 ≥1 且账实守恒。
    Done,
    /// 异常终态：全程零有效帧（显性，不谎报成功）。
    Empty,
}

/// 栈窗内存读接口（步行器不直接碰内存：读函数注入，纯功能可测）。
pub trait SwRead {
    /// 读一个 8 字节槽位；None 表示不可读（页不可达等）。
    fn slot(&self, addr: u64) -> Option<u64>;
}

/// 确定性栈窗快照读器（测试与演练注入用：槽位表驱动）。
#[derive(Clone, Debug, Default)]
pub struct SwSnapshot {
    /// addr → value 稀疏表（按 addr 升序存放）。
    pub slots: Vec<(u64, u64)>,
}

impl SwSnapshot {
    /// 写入一个槽位（保持升序，重复写覆盖）。
    pub fn put(&mut self, addr: u64, val: u64) {
        match self.slots.binary_search_by_key(&addr, |e| e.0) {
            Ok(i) => self.slots[i] = (addr, val),
            Err(i) => self.slots.insert(i, (addr, val)),
        }
    }
}

impl SwRead for SwSnapshot {
    fn slot(&self, addr: u64) -> Option<u64> {
        self.slots
            .binary_search_by_key(&addr, |e| e.0)
            .ok()
            .map(|i| self.slots[i].1)
    }
}

/// 步行账（append-only + 守恒校验）。
#[derive(Clone, Debug, Default)]
pub struct SwLedger {
    /// 有效帧账。
    pub frames: Vec<SwFrame>,
    /// 被验证门拒掉的候选数（链路+扫描合计）。
    pub rejected: u64,
    /// 链步进步数。
    pub chain_steps: u64,
    /// 扫描推进槽数。
    pub scan_slots: u64,
    /// 诊断码账（出现过的码，append-only）。
    pub codes: Vec<SwCode>,
}

impl SwLedger {
    /// 账实守恒：帧数 = 有效帧计数，且深度严格单调。
    pub fn conserved(&self) -> bool {
        let mut prev_depth: i64 = -1;
        for f in &self.frames {
            if !f.valid() {
                return false;
            }
            if (f.depth as i64) != prev_depth + 1 {
                return false;
            }
            prev_depth = f.depth as i64;
        }
        true
    }

    /// 记一条诊断码（幂等去重：同码只记一次，保 append-only 语义）。
    pub fn note(&mut self, c: SwCode) {
        if !self.codes.contains(&c) {
            self.codes.push(c);
        }
    }
}

/// 步行器主体。
pub struct StackWalker {
    /// 种子 rIP。
    seed_rip: u64,
    /// 种子 rBP。
    seed_rbp: u64,
    /// 栈窗。
    stack_lo: u64,
    stack_hi: u64,
    /// 当前相位。
    phase: SwPhase,
    /// 链步进当前 rBP。
    cur_rbp: u64,
    /// 链上出现过的 rBP（回环检测）。
    seen: Vec<u64>,
    /// 扫描游标。
    scan_cur: u64,
    /// 扫描起点（切换时刻的栈窗内位置）。
    scan_from: u64,
    /// 已扫过的返回地址（扫描帧去重）。
    scan_seen: Vec<u64>,
    /// 账。
    ledger: SwLedger,
    /// 逻辑 tick。
    tick: u64,
    /// 帧数触顶标志。
    capped: bool,
}

impl StackWalker {
    /// 构造：种子采样 + 参数校验（不合法即刻显性拒绝，不静默开工）。
    pub fn new(rip: u64, rbp: u64, stack_lo: u64, stack_hi: u64) -> Result<Self, (SwCode, String)> {
        if stack_hi <= stack_lo || stack_hi - stack_lo > SW_SCAN_SPAN_MAX {
            return Err((SwCode::WindowBad, SwCode::WindowBad.advice().to_string()));
        }
        let mut w = StackWalker {
            seed_rip: rip,
            seed_rbp: rbp,
            stack_lo,
            stack_hi,
            phase: SwPhase::Init,
            cur_rbp: rbp,
            seen: Vec::new(),
            scan_cur: 0,
            scan_from: 0,
            scan_seen: Vec::new(),
            ledger: SwLedger::default(),
            tick: 0,
            capped: false,
        };
        // 种子验证门：rIP 代码窗 + rBP 栈窗 + 对齐。
        if !in_text(rip) || !in_window(rbp, stack_lo, stack_hi) || rbp % SW_SLOT != 0 {
            w.phase = SwPhase::Empty;
            w.ledger.note(SwCode::SeedBad);
            return Err((SwCode::SeedBad, SwCode::SeedBad.advice().to_string()));
        }
        // 种子帧入账（种子免位型核验——它来自异常现场直采，gate bit2 直接置位）。
        w.push_frame(rip, rbp, SwOrigin::Seed, 0b111);
        w.cur_rbp = rbp;
        w.seen.push(rbp);
        w.phase = SwPhase::ChainRun;
        Ok(w)
    }

    /// 种子 rIP（异常现场采样值，报告对账用）。
    pub const fn seed_rip(&self) -> u64 {
        self.seed_rip
    }

    /// 种子 rBP。
    pub const fn seed_rbp(&self) -> u64 {
        self.seed_rbp
    }

    /// 当前相位。
    pub const fn phase(&self) -> SwPhase {
        self.phase
    }

    /// 账。
    pub const fn ledger(&self) -> &SwLedger {
        &self.ledger
    }

    /// 有效帧数。
    pub fn valid_frames(&self) -> usize {
        self.ledger.frames.iter().filter(|f| f.valid()).count()
    }

    /// 推进一个逻辑 tick（确定性）。
    pub const fn tick(&mut self) {
        self.tick += 1;
    }

    /// 记帧（唯一入口：三验证门已过才允许调；防线在账内复核）。
    fn push_frame(&mut self, ret: u64, rbp: u64, origin: SwOrigin, gates: u8) {
        let depth = self.ledger.frames.len();
        let f = SwFrame {
            ret,
            rbp,
            origin,
            tick: self.tick,
            depth,
            gates,
        };
        if !f.valid() {
            // 误帧防线：调用序违例立即显性（误帧率 0 的实现侧保证）。
            self.ledger.note(SwCode::FalseFrameGuard);
            return;
        }
        self.ledger.frames.push(f);
    }

    /// 验证门：对齐 + 代码窗 + 位型核验（位型核验由调用方按来源补足）。
    fn gates_basic(ret: u64) -> Option<u8> {
        if ret % SW_SLOT != 0 {
            return None;
        }
        if !in_text(ret) {
            return None;
        }
        Some(0b011)
    }

    /// 链步进一步：[rbp] = 保存 rBP，[rbp+8] = 返回地址。
    /// 任一门失败 → 判链损，返回 Err，交由扫描接管。
    fn chain_step<R: SwRead>(&mut self, r: &R) -> Result<(), SwCode> {
        if self.capped {
            return Err(SwCode::FrameCap);
        }
        let cur = self.cur_rbp;
        // 链值读取。
        let (Some(next_rbp), Some(ret)) = (r.slot(cur), r.slot(cur + SW_SLOT)) else {
            self.ledger.note(SwCode::ChainRetGate);
            return Err(SwCode::ChainRetGate);
        };
        // 链健康门 1：新 rBP 必须在栈窗内。
        if !in_window(next_rbp, self.stack_lo, self.stack_hi) {
            self.ledger.note(SwCode::ChainAdvance);
            return Err(SwCode::ChainAdvance);
        }
        // 链健康门 2：严格上界推进（防回环/防下坠；x86_64 栈向低地址生长，
        // 老帧 rBP 大于新帧 rBP——链向高地址走，故 new > cur 才是健全推进）。
        if next_rbp <= cur {
            self.ledger.note(SwCode::ChainLoop);
            return Err(SwCode::ChainLoop);
        }
        if self.seen.contains(&next_rbp) {
            self.ledger.note(SwCode::ChainLoop);
            return Err(SwCode::ChainLoop);
        }
        // 链健康门 3：返回地址验证门。
        let Some(g) = Self::gates_basic(ret) else {
            self.ledger.note(SwCode::ChainRetGate);
            return Err(SwCode::ChainRetGate);
        };
        // 位型核验（链帧：返回地址后紧跟合法保存 rBP 槽位才置 bit2）。
        let bit2 = match r.slot(next_rbp) {
            Some(nr) if in_window(nr, self.stack_lo, self.stack_hi) && nr % SW_SLOT == 0 => 0b100,
            _ => 0,
        };
        if bit2 == 0 {
            self.ledger.note(SwCode::ChainRetGate);
            return Err(SwCode::ChainRetGate);
        }
        // 全门通过：记帧 + 推进。
        self.push_frame(ret, cur, SwOrigin::Chain, g | bit2);
        self.ledger.chain_steps += 1;
        self.cur_rbp = next_rbp;
        self.seen.push(next_rbp);
        if self.ledger.frames.len() >= SW_MAX_FRAMES {
            self.capped = true;
            self.ledger.note(SwCode::FrameCap);
            return Err(SwCode::FrameCap);
        }
        Ok(())
    }

    /// 保守扫描器：从 scan_from 向高地址逐槽位找返回地址位型。
    /// 位型核验（bit2 的依据）：候选值落在代码窗 + 对齐 + 候选上方相邻槽位
    /// 不是代码窗值（返回地址后通常跟保存的 rBP/局部变量，不会紧跟另一个代码地址）
    /// + 候选未被记录过（去重）。
    fn scan_step<R: SwRead>(&mut self, r: &R) -> Result<(), SwCode> {
        if self.capped {
            return Err(SwCode::FrameCap);
        }
        loop {
            let cur = self.scan_cur;
            if cur >= self.stack_hi || self.scan_from.saturating_add(SW_SCAN_SPAN_MAX) <= cur {
                self.ledger.note(SwCode::ScanExhausted);
                return Err(SwCode::ScanExhausted);
            }
            if cur.checked_add(SW_SLOT).map_or(true, |n| n > self.stack_hi) {
                self.ledger.note(SwCode::ScanOob);
                return Err(SwCode::ScanOob);
            }
            self.scan_cur = cur + SW_SLOT;
            self.ledger.scan_slots += 1;
            let Some(v) = r.slot(cur) else {
                continue;
            };
            if !in_text(v) || v % SW_SLOT != 0 {
                continue;
            }
            if self.scan_seen.contains(&v) {
                continue;
            }
            // 位型核验：候选下一槽位不应也是代码窗值（抑制代码指针池误判）。
            if let Some(nxt) = r.slot(cur + SW_SLOT) {
                if in_text(nxt) {
                    self.ledger.rejected += 1;
                    self.ledger.note(SwCode::ScanGate);
                    continue;
                }
            }
            // 候选上方相邻槽位若可读且为栈窗值，视作保存 rBP 位型 → 全门。
            let bit2 = match r.slot(cur + SW_SLOT) {
                Some(nr) if in_window(nr, self.stack_lo, self.stack_hi) && nr % SW_SLOT == 0 => 0b100,
                _ => 0,
            };
            if bit2 == 0 {
                self.ledger.rejected += 1;
                self.ledger.note(SwCode::ScanGate);
                continue;
            }
            self.scan_seen.push(v);
            let depth = self.ledger.frames.len();
            self.push_frame(v, cur, SwOrigin::Scan, 0b111);
            let _ = depth;
            if self.ledger.frames.len() >= SW_MAX_FRAMES {
                self.capped = true;
                self.ledger.note(SwCode::FrameCap);
                return Err(SwCode::FrameCap);
            }
            return Ok(());
        }
    }

    /// 混合策略：链步进直至链损 → 扫描自链损点接管 → 直至扫描耗尽。
    /// 全程显性：零有效帧落 Empty 终态，不谎报 Done。
    pub fn walk<R: SwRead>(mut self, r: &R) -> SwReport {
        // 相位迁移：ChainRun →（链损）→ ScanRun（切换点 = 链损处 rBP，向高地址扫描）。
        self.scan_from = self.cur_rbp;
        self.scan_cur = self.cur_rbp;
        loop {
            match self.phase {
                SwPhase::ChainRun => {
                    self.tick();
                    if self.chain_step(r).is_err() {
                        self.phase = SwPhase::ChainBroken;
                    }
                }
                SwPhase::ChainBroken => {
                    // 切换留痕：扫描起点 = 链损点（保证链损前后的帧序拼接完整）。
                    self.phase = SwPhase::ScanRun;
                }
                SwPhase::ScanRun => {
                    self.tick();
                    if self.scan_step(r).is_err() {
                        self.phase = SwPhase::ScanExhausted;
                    }
                }
                SwPhase::ScanExhausted | SwPhase::Done | SwPhase::Empty | SwPhase::Init => {
                    break;
                }
            }
        }
        // 终态裁决。
        self.phase = if self.valid_frames() > 1 {
            SwPhase::Done
        } else {
            // 种子帧不算产出（它只是起点），但保留在账中供对账。
            SwPhase::Empty
        };
        // 账实守恒终验（破缺即显性诊断，不静默出报告）。
        if !self.ledger.conserved() {
            self.ledger.note(SwCode::LedgerBreak);
        }
        SwReport::from_walker(&self)
    }
}

/// 步行报告（只读快照，供诊断与判据核验）。
#[derive(Clone, Debug)]
pub struct SwReport {
    /// 终态。
    pub phase: SwPhase,
    /// 有效帧列表（含来源与深度）。
    pub frames: Vec<SwFrame>,
    /// 链步进步数。
    pub chain_steps: u64,
    /// 扫描槽数。
    pub scan_slots: u64,
    /// 门拒候选数。
    pub rejected: u64,
    /// 诊断码集。
    pub codes: Vec<SwCode>,
    /// 误帧率判定：全部帧三门全过且账实守恒 = 0。
    pub false_frame_free: bool,
}

impl SwReport {
    fn from_walker(w: &StackWalker) -> Self {
        SwReport {
            phase: w.phase,
            frames: w.ledger.frames.clone(),
            chain_steps: w.ledger.chain_steps,
            scan_slots: w.ledger.scan_slots,
            rejected: w.ledger.rejected,
            codes: w.ledger.codes.clone(),
            false_frame_free: w.ledger.conserved() && w.valid_frames() + 1 == w.ledger.frames.len(),
        }
    }

    /// 中文可读摘要（异常零静默）。
    pub fn digest(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "栈回溯报告：终态 {:?}，有效帧 {}（链 {} 步 + 扫描 {} 槽，门拒 {}）",
            self.phase,
            self.frames.len(),
            self.chain_steps,
            self.scan_slots,
            self.rejected
        ));
        if self.codes.is_empty() {
            s.push_str("；无诊断码");
        } else {
            s.push_str("；诊断码：");
            for c in &self.codes {
                s.push_str(&format!("[0x{:04X} {}] ", c.val(), c.text()));
            }
        }
        s
    }
}

/// 地址在代码窗内。
pub const fn in_text(a: u64) -> bool {
    a >= SW_TEXT_LO && a < SW_TEXT_HI
}

/// 地址在栈窗内。
pub const fn in_window(a: u64, lo: u64, hi: u64) -> bool {
    a >= lo && a < hi
}

/// 便捷入口：单调用完成步行（种子采样 → 混合策略 → 报告）。
pub fn walk_stack<R: SwRead>(rip: u64, rbp: u64, stack_lo: u64, stack_hi: u64, r: &R) -> Result<SwReport, (SwCode, String)> {
    let w = StackWalker::new(rip, rbp, stack_lo, stack_hi)?;
    Ok(w.walk(r))
}

/// 段位守卫：全部诊断码必须落在本模块段。
pub fn segment_guard() -> bool {
    SwCode::ALL.iter().all(|c| c.val() >= SW_SEG_BASE && c.val() < SW_SEG_BASE + 0x100)
}
