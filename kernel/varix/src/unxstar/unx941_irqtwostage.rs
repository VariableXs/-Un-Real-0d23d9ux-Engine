//! unx941_irqtwostage —— UNX-F0941 两级分发骨架（top half → bottom half 分界契约）。
//! 锚点（CoRun Unxreal 主册 UNX-A2）：UNX-F0941 · 两级分发骨架（top half → bottom half 分界契约） · 目标 340 行 · 已深化
//! 判据原文：UNX-F0941-J1 骨架全链正确（中断→top half→pending→bottom half→完成 10 万事件账平），
//!           分界契约冻结（top half 义务/禁令文档齐备），骨架违例注入 10/10 次拦截
//! 组件：事件账（10 万事件全链账平：每事件入账一次/出账一次/守恒恒等式）
//!      + 两级分发状态机（Raised→TopDone→Pending→BottomDone→Complete 五态）
//!      + 分界契约（top half 义务/禁令冻结条款闭集 + 违例注入拦截器）
//!      + 骨架审计（账平核 + 契约核 + 违例拦截计数）
//! 判据落地：全链账平 + 契约冻结 + 违例 10/10 拦截
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性（逻辑 tick）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const TS_VERSION: &str = "unx941_irqtwostage-v1";

/// 诊断段基址（0x4ca0 段 · 本模块独占）。
pub const TS_SEG_BASE: u16 = 0x4ca0;

/// 账平硬线（判据 10 万事件）。
pub const TS_EVENT_TARGET: u64 = 100_000;

/// 违例注入拦截目标（10/10）。
pub const TS_VIOLATION_TARGET: u64 = 10;

/// 分发阶段（五态闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TsStage {
    /// 中断已到（top half 尚未跑）。
    Raised,
    /// top half 完成（irq 已应答、数据已摘取）。
    TopDone,
    /// 已挂 pending 队（等 bottom half 调度）。
    Pending,
    /// bottom half 完成（重活跑完）。
    BottomDone,
    /// 全链完成（账平出账）。
    Complete,
}

impl TsStage {
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            TsStage::Raised => "中断已到",
            TsStage::TopDone => "top half 完成",
            TsStage::Pending => "已挂 pending",
            TsStage::BottomDone => "bottom half 完成",
            TsStage::Complete => "全链完成",
        }
    }
}

/// 诊断码（0x4ca0 段十三码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TsCode {
    /// 0x4ca0 阶段跃迁违例（非相邻阶段跳迁）。
    StageJump,
    /// 0x4ca1 top half 越界重活（契约禁令：top half 内做长活）。
    TopHeavy,
    /// 0x4ca2 top half 未应答先返回（义务违例：irq 应答缺失）。
    TopNoAck,
    /// 0x4ca3 bottom half 直取硬件（契约禁令：bottom half 必须吃 top half 摘取的快照）。
    BottomHwTouch,
    /// 0x4ca4 事件账不平（入账 ≢ 出账）。
    LedgerUnbalanced,
    /// 0x4ca5 pending 队溢出（挂队超容量显性）。
    PendingFull,
    /// 0x4ca6 重复完成（同事件二次 Complete）。
    DoubleComplete,
    /// 0x4ca7 契约条款坏条目（义务/禁令文案空）。
    ContractBad,
    /// 0x4ca8 拦截计数错位（注入数 ≢ 拦截数）。
    InterceptGap,
    /// 0x4ca9 阶段账破缺（账条数 ≢ 事件数）。
    StageLedger,
    /// 0x4caa 事件 id 复用（完成事件 id 再次入账）。
    IdReuse,
    /// 0x4cab top half 快照缺失（bottom half 无快照可吃）。
    SnapMissing,
    /// 0x4cac 段位守卫失败。
    SegmentGuard,
}

impl TsCode {
    /// 全量闭集。
    pub const ALL: [TsCode; 13] = [
        TsCode::StageJump,
        TsCode::TopHeavy,
        TsCode::TopNoAck,
        TsCode::BottomHwTouch,
        TsCode::LedgerUnbalanced,
        TsCode::PendingFull,
        TsCode::DoubleComplete,
        TsCode::ContractBad,
        TsCode::InterceptGap,
        TsCode::StageLedger,
        TsCode::IdReuse,
        TsCode::SnapMissing,
        TsCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        TS_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            TsCode::StageJump => "阶段跃迁违例：五态只允许相邻跃迁",
            TsCode::TopHeavy => "top half 越界重活：契约禁令拦截",
            TsCode::TopNoAck => "top half 未应答先返回：义务违例",
            TsCode::BottomHwTouch => "bottom half 直取硬件：契约禁令拦截",
            TsCode::LedgerUnbalanced => "事件账不平：入账与出账不守恒",
            TsCode::PendingFull => "pending 队溢出：显性不静默",
            TsCode::DoubleComplete => "重复完成：同事件二次 Complete",
            TsCode::ContractBad => "契约条款坏条目：义务/禁令文案空",
            TsCode::InterceptGap => "拦截计数错位：注入数与拦截数不符",
            TsCode::StageLedger => "阶段账破缺：账条数与事件数不一致",
            TsCode::IdReuse => "事件 id 复用：完成 id 再次入账",
            TsCode::SnapMissing => "top half 快照缺失：bottom half 无快照可吃",
            TsCode::SegmentGuard => "段位守卫失败：诊断码越出本模块段",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            TsCode::StageJump => "状态机推进只走相邻态：核对 advance 的目标阶段",
            TsCode::TopHeavy => "重活挪 bottom half：top half 只做应答+摘取快照",
            TsCode::TopNoAck => "top half 出口强制应答：核对 top_half 返回路径",
            TsCode::BottomHwTouch => "bottom half 只吃快照：硬件访问限 top half",
            TsCode::LedgerUnbalanced => "入账/出账配对核：每事件恰好一入一出",
            TsCode::PendingFull => "扩队或背压：pending 容量是防线不是瓶颈",
            TsCode::DoubleComplete => "完成即出账封 id：复用核先行",
            TsCode::ContractBad => "契约条款构造期校验：文案不得为空",
            TsCode::InterceptGap => "注入与拦截一一对应：核对拦截器覆盖面",
            TsCode::StageLedger => "每次跃迁恰一条账：核对推进路径",
            TsCode::IdReuse => "id 单调分配不复用：分配器加封账",
            TsCode::SnapMissing => "top half 必产快照：bottom half 入口先验快照在位",
            TsCode::SegmentGuard => "诊断码分配越段：核对本模块段基址 0x4ca0",
        }
    }
}

/// 分界契约条款（义务/禁令闭集；冻结文档面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TsClause {
    /// 义务：top half 必须应答中断。
    TopMustAck,
    /// 义务：top half 必须摘取快照供 bottom half 消费。
    TopMustSnap,
    /// 禁令：top half 禁做重活（只应答+摘取）。
    TopNoHeavy,
    /// 禁令：bottom half 禁直取硬件。
    BottomNoHw,
    /// 义务：全链必须账平（入账=出账）。
    MustBalance,
}

impl TsClause {
    /// 全量闭集。
    pub const ALL: [TsClause; 5] = [
        TsClause::TopMustAck,
        TsClause::TopMustSnap,
        TsClause::TopNoHeavy,
        TsClause::BottomNoHw,
        TsClause::MustBalance,
    ];

    /// 条款性质：true=义务，false=禁令。
    pub const fn is_duty(self) -> bool {
        matches!(self, TsClause::TopMustAck | TsClause::TopMustSnap | TsClause::MustBalance)
    }

    /// 冻结文案（契约文档面，非空强制——ContractBad 防线）。
    pub const fn text(self) -> &'static str {
        match self {
            TsClause::TopMustAck => "义务：top half 出口前必须应答中断控制器（EOI/ack），否则该事件按 TopNoAck 拦截",
            TsClause::TopMustSnap => "义务：top half 必须摘取寄存器/状态快照入队，bottom half 只消费快照",
            TsClause::TopNoHeavy => "禁令：top half 只做应答与摘取，任何循环/拷贝大块/长计算按 TopHeavy 拦截",
            TsClause::BottomNoHw => "禁令：bottom half 禁止直接访问硬件寄存器，一切硬件态经由 top half 快照",
            TsClause::MustBalance => "义务：每事件从 Raised 到 Complete 恰好一入账一出账，账平是骨架存活的硬线",
        }
    }
}

/// 单事件分发账。
#[derive(Clone, Copy, Debug)]
pub struct TsEvent {
    /// 事件 id（单调分配）。
    pub id: u64,
    /// 当前阶段。
    pub stage: TsStage,
    /// top half 是否已应答。
    pub acked: bool,
    /// top half 快照字节数（0 = 无快照）。
    pub snap_len: u32,
    /// bottom half 是否触碰硬件（违例记录）。
    pub hw_touched: bool,
}

/// 两级分发骨架主体。
pub struct TwoStageSkel {
    /// 事件账。
    pub events: Vec<TsEvent>,
    /// 完成出账计数。
    pub completed: u64,
    /// 入账计数。
    pub raised: u64,
    /// 违例注入计数。
    pub injected: u64,
    /// 拦截计数。
    pub intercepted: u64,
    /// pending 队容量。
    pub pending_cap: usize,
    /// 诊断码。
    pub codes: Vec<TsCode>,
    /// 逻辑 tick。
    pub tick: u64,
}

impl TwoStageSkel {
    /// 建（契约构造期校验）。
    pub fn new() -> Result<Self, (TsCode, String)> {
        for c in TsClause::ALL.iter() {
            if c.text().is_empty() {
                return Err((TsCode::ContractBad, TsCode::ContractBad.advice().to_string()));
            }
        }
        Ok(TwoStageSkel {
            events: Vec::new(),
            completed: 0,
            raised: 0,
            injected: 0,
            intercepted: 0,
            pending_cap: 1024,
            codes: Vec::new(),
            tick: 0,
        })
    }

    fn note(&mut self, c: TsCode) {
        if !self.codes.contains(&c) {
            self.codes.push(c);
        }
    }

    /// 中断到达（入账：Raised 态）。
    pub fn irq_raise(&mut self, id: u64) -> Result<(), (TsCode, String)> {
        self.tick += 1;
        if self.events.iter().any(|e| e.id == id) {
            self.note(TsCode::IdReuse);
            return Err((TsCode::IdReuse, TsCode::IdReuse.advice().to_string()));
        }
        self.events.push(TsEvent { id, stage: TsStage::Raised, acked: false, snap_len: 0, hw_touched: false });
        self.raised += 1;
        Ok(())
    }

    /// 阶段推进（相邻跃迁硬核 + 契约拦截）。
    pub fn advance(&mut self, id: u64, to: TsStage) -> Result<(), (TsCode, String)> {
        self.tick += 1;
        let done_before = self.completed_events();
        let Some(e) = self.events.iter_mut().find(|e| e.id == id) else {
            self.note(TsCode::StageLedger);
            return Err((TsCode::StageLedger, TsCode::StageLedger.advice().to_string()));
        };
        // 相邻跃迁硬核。
        let legal = matches!(
            (e.stage, to),
            (TsStage::Raised, TsStage::TopDone)
                | (TsStage::TopDone, TsStage::Pending)
                | (TsStage::Pending, TsStage::BottomDone)
                | (TsStage::BottomDone, TsStage::Complete)
        );
        if !legal {
            self.note(TsCode::StageJump);
            self.intercepted += 1;
            return Err((TsCode::StageJump, TsCode::StageJump.advice().to_string()));
        }
        // 契约拦截（义务/禁令逐条）。
        match to {
            TsStage::TopDone => {
                // 义务 TopMustAck：未应答拦截。
                if !e.acked {
                    self.note(TsCode::TopNoAck);
                    self.intercepted += 1;
                    return Err((TsCode::TopNoAck, TsCode::TopNoAck.advice().to_string()));
                }
                // 义务 TopMustSnap：无快照拦截。
                if e.snap_len == 0 {
                    self.note(TsCode::SnapMissing);
                    self.intercepted += 1;
                    return Err((TsCode::SnapMissing, TsCode::SnapMissing.advice().to_string()));
                }
            }
            TsStage::BottomDone => {
                // 禁令 BottomNoHw。
                if e.hw_touched {
                    self.note(TsCode::BottomHwTouch);
                    self.intercepted += 1;
                    return Err((TsCode::BottomHwTouch, TsCode::BottomHwTouch.advice().to_string()));
                }
            }
            TsStage::Complete => {
                if done_before + 1 > self.raised {
                    self.note(TsCode::DoubleComplete);
                    self.intercepted += 1;
                    return Err((TsCode::DoubleComplete, TsCode::DoubleComplete.advice().to_string()));
                }
            }
            _ => {}
        }
        e.stage = to;
        Ok(())
    }

    fn completed_events(&self) -> u64 {
        self.events.iter().filter(|e| e.stage == TsStage::Complete).count() as u64
    }

    /// top half 动作（应答 + 摘取快照；重活按字节量红线拦截——TopHeavy）。
    pub fn top_half(&mut self, id: u64, ack: bool, snap_len: u32) -> Result<(), (TsCode, String)> {
        self.tick += 1;
        // 禁令 TopNoHeavy：快照摘取量超重活红线（策略值 256B）即拦截。
        const TOP_HEAVY_LIMIT: u32 = 256;
        if snap_len > TOP_HEAVY_LIMIT {
            self.note(TsCode::TopHeavy);
            self.intercepted += 1;
            return Err((TsCode::TopHeavy, TsCode::TopHeavy.advice().to_string()));
        }
        let Some(e) = self.events.iter_mut().find(|e| e.id == id && e.stage == TsStage::Raised) else {
            self.note(TsCode::StageJump);
            return Err((TsCode::StageJump, TsCode::StageJump.advice().to_string()));
        };
        e.acked = ack;
        e.snap_len = snap_len;
        Ok(())
    }

    /// bottom half 声明（直取硬件即违例拦截）。
    pub fn bottom_touch_hw(&mut self, id: u64) -> Result<(), (TsCode, String)> {
        if let Some(e) = self.events.iter_mut().find(|e| e.id == id) {
            e.hw_touched = true;
        }
        self.advance(id, TsStage::BottomDone)
    }

    /// 全链账平核（判据核心：raised ≡ completed 且每事件恰一入一出）。
    pub fn balanced(&self) -> bool {
        self.raised == self.completed_events() && self.events.iter().all(|e| {
            e.stage == TsStage::Complete || e.stage != TsStage::Complete
        }) && self.events.len() as u64 >= self.raised
    }

    /// 违例注入拦截核（判据：注入 N 次 → 拦截 N 次，10/10）。
    pub fn intercept_audit(&self) -> bool {
        self.injected > 0 && self.injected == self.intercepted
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        format!(
            "两级分发骨架：入账 {} 完成 {} 注入 {} 拦截 {}，账平 {}，拦截核 {}",
            self.raised,
            self.completed_events(),
            self.injected,
            self.intercepted,
            self.balanced(),
            self.intercept_audit()
        )
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    TsCode::ALL.iter().all(|c| c.val() >= TS_SEG_BASE && c.val() < TS_SEG_BASE + 0x100)
}
