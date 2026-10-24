// unx297_dualdog.rs ↔ UNX-F0297 双狗联动复盘（软狗/NMI 触发链归因）
//
// 判据原文：UNX-F0297-J1 软狗/NMI 触发链归因正确，联动复盘报告含时间线与根因。
// 难点分析：双狗系统最容易出的错是「误归因」——软狗先超时但真正死因是 NMI 侧的
//          硬挂，或者 NMI 打进来时软狗恰好也在计时边缘。复盘必须按时间戳重建触发链，
//          区分「首触」与「跟随」，并给出根因分类（真挂/慢/活锁）。
//          联动语义：软狗超时后应给 NMI 侧一次宽限；NMI 先触则软狗必须静默。
// 纪律约束：时间戳单调；首触唯一；跟随有账；根因三分类；宽限窗口显式。
// 成本记账：归因 O(events)；时间线重建 O(events)；根因 O(1)。

#![allow(dead_code)]

use alloc::vec::Vec;

/// 双狗身份。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DogId {
    /// 软狗（定时器中断路径）。
    Soft,
    /// 硬狗 / NMI 路径。
    Nmi,
}

impl DogId {
    /// 名字。
    pub fn name(&self) -> &'static str {
        match self {
            DogId::Soft => "soft",
            DogId::Nmi => "nmi",
        }
    }
}

/// 触发链事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DogEvent {
    /// 哪只狗。
    pub dog: DogId,
    /// 时间戳（TSC 单调，冻结戳语义）。
    pub tsc: u64,
    /// 事件类型。
    pub kind: DogEventKind,
    /// 事件序号（到达序，用于去重）。
    pub seq: u32,
}

/// 事件类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DogEventKind {
    /// 超时触发。
    Fired,
    /// 喂狗（清计数）。
    Fed,
    /// 宽限中（软狗触发后给 NMI 的窗口）。
    GraceWindow,
    /// 静默（被另一只狗压制）。
    Suppressed,
    /// 复盘标记（人工/自动注入的锚点）。
    Marker,
}

/// 根因分类（三选一，外加未知）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootCause {
    /// 真挂（CPU 停止取指/中断全关）。
    TrueHang,
    /// 慢（仍在走但超过阈值）。
    TooSlow,
    /// 活锁（在转但无进展）。
    Livelock,
    /// 证据不足。
    Unknown,
}

impl RootCause {
    /// 名字。
    pub fn name(&self) -> &'static str {
        match self {
            RootCause::TrueHang => "true_hang",
            RootCause::TooSlow => "too_slow",
            RootCause::Livelock => "livelock",
            RootCause::Unknown => "unknown",
        }
    }
}

/// 触发链节点（归因后的有序链）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainNode {
    /// 事件。
    pub event: DogEvent,
    /// 角色：首触 / 跟随 / 压制。
    pub role: ChainRole,
}

/// 链角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainRole {
    /// 首触（唯一）。
    First,
    /// 跟随（后续触发）。
    Follow,
    /// 被压制（本应触发但被另一狗的窗口压掉）。
    Suppressed,
}

/// 联动配置。
#[derive(Debug, Clone, Copy)]
pub struct DualDogConfig {
    /// 软狗超时阈值（TSC 刻度）。
    pub soft_timeout: u64,
    /// NMI 超时阈值。
    pub nmi_timeout: u64,
    /// 宽限窗口（软狗触发后给 NMI 的额外等待）。
    pub grace_window: u64,
    /// 活锁判定：连续 Fired 但无 Fed 的次数阈值。
    pub livelock_threshold: u32,
}

impl DualDogConfig {
    /// 默认配置。
    pub fn default_cfg() -> Self {
        DualDogConfig {
            soft_timeout: 1_000_000,
            nmi_timeout: 2_000_000,
            grace_window: 200_000,
            livelock_threshold: 3,
        }
    }

    /// 校验（超时关系与窗口合理性）。
    pub fn validate(&self) -> bool {
        self.soft_timeout > 0
            && self.nmi_timeout >= self.soft_timeout
            && self.grace_window <= self.nmi_timeout
            && self.livelock_threshold >= 2
    }
}

/// 复盘引擎。
pub struct DualDogReview {
    cfg: DualDogConfig,
    events: Vec<DogEvent>,
    next_seq: u32,
}

impl DualDogReview {
    /// 新建。
    pub fn new(cfg: DualDogConfig) -> Self {
        DualDogReview { cfg, events: Vec::new(), next_seq: 0 }
    }

    /// 推入事件（自动编号，要求 tsc 单调不减）。
    pub fn push(&mut self, dog: DogId, tsc: u64, kind: DogEventKind) -> bool {
        // tsc 单调性：允许相等（同刻多事件），不允许回退
        if let Some(last) = self.events.last() {
            if tsc < last.tsc {
                return false;
            }
        }
        self.events.push(DogEvent { dog, tsc, kind, seq: self.next_seq });
        self.next_seq += 1;
        true
    }

    /// 取事件表。
    pub fn events(&self) -> &[DogEvent] {
        &self.events
    }

    /// 归因重建触发链。
    ///
    /// 规则：
    /// 1. 只有 Fired 参与首触竞争。
    /// 2. 时间最早的 Fired 为 First；同刻则 Soft 优先（软狗是第一道）。
    /// 3. 其余 Fired：若落在首触 + grace_window 内且狗不同 → Suppressed；
    ///    否则 Follow。
    /// 4. Fed 不入链但参与根因判定。
    pub fn attribute(&self) -> Vec<ChainNode> {
        let mut fired: Vec<&DogEvent> =
            self.events.iter().filter(|e| e.kind == DogEventKind::Fired).collect();
        if fired.is_empty() {
            return Vec::new();
        }
        // 稳定排序：tsc 升序，同刻 Soft 优先，再按 seq
        fired.sort_by(|a, b| {
            a.tsc
                .cmp(&b.tsc)
                .then_with(|| match (a.dog, b.dog) {
                    (DogId::Soft, DogId::Nmi) => core::cmp::Ordering::Less,
                    (DogId::Nmi, DogId::Soft) => core::cmp::Ordering::Greater,
                    _ => core::cmp::Ordering::Equal,
                })
                .then_with(|| a.seq.cmp(&b.seq))
        });

        let first = fired[0];
        let mut chain = Vec::with_capacity(fired.len());
        chain.push(ChainNode { event: *first, role: ChainRole::First });

        for e in fired.iter().skip(1) {
            let in_grace = e.tsc <= first.tsc.saturating_add(self.cfg.grace_window);
            let different_dog = e.dog != first.dog;
            let role = if in_grace && different_dog {
                ChainRole::Suppressed
            } else {
                ChainRole::Follow
            };
            chain.push(ChainNode { event: **e, role });
        }
        chain
    }

    /// 根因判定。
    ///
    /// 规则：
    /// - 无 Fed 且首触为 Nmi → TrueHang（NMI 能进说明 APIC 活，但无喂狗 → 真挂嫌疑；
    ///   若首触是 Nmi 且无任何 Fed，倾向 TrueHang）。
    /// - 有 Fed 且 Fired 次数 >= livelock_threshold 且最后 Fed 在第一次 Fired 之前 → Livelock。
    /// - 有 Fed 且最近 Fed 晚于首触 → TooSlow（还在走，只是慢）。
    /// - 其余 → Unknown。
    pub fn root_cause(&self) -> RootCause {
        let fired: Vec<&DogEvent> =
            self.events.iter().filter(|e| e.kind == DogEventKind::Fired).collect();
        let fed: Vec<&DogEvent> =
            self.events.iter().filter(|e| e.kind == DogEventKind::Fed).collect();

        if fired.is_empty() {
            return RootCause::Unknown;
        }

        if fed.is_empty() {
            // 无喂狗：首触 Nmi 更像硬挂；首触 Soft 可能是中断关了
            return match fired[0].dog {
                DogId::Nmi => RootCause::TrueHang,
                DogId::Soft => RootCause::TrueHang,
            };
        }

        let first_fire = fired[0].tsc;
        let last_fed = fed[fed.len() - 1].tsc;

        // 连续 Fired 后无 Fed → 活锁嫌疑
        if fired.len() as u32 >= self.cfg.livelock_threshold && last_fed < first_fire {
            return RootCause::Livelock;
        }

        // 最近 Fed 晚于首触 → 仍在走，只是慢
        if last_fed >= first_fire {
            return RootCause::TooSlow;
        }

        RootCause::Unknown
    }

    /// 时间线行（`tsc|dog|kind|seq`）。
    pub fn timeline_rows(&self) -> Vec<(u64, &'static str, &'static str, u32)> {
        self.events
            .iter()
            .map(|e| {
                let kind = match e.kind {
                    DogEventKind::Fired => "fired",
                    DogEventKind::Fed => "fed",
                    DogEventKind::GraceWindow => "grace",
                    DogEventKind::Suppressed => "suppressed",
                    DogEventKind::Marker => "marker",
                };
                (e.tsc, e.dog.name(), kind, e.seq)
            })
            .collect()
    }

    /// 首触事件（链头）。
    pub fn first_trigger(&self) -> Option<DogEvent> {
        self.attribute().first().map(|n| n.event)
    }

    /// 被压制数。
    pub fn suppressed_count(&self) -> usize {
        self.attribute()
            .iter()
            .filter(|n| n.role == ChainRole::Suppressed)
            .count()
    }
}

/// 复盘报告（结构化摘要）。
pub struct ReviewReport {
    /// 首触狗。
    pub first_dog: DogId,
    /// 根因。
    pub cause: RootCause,
    /// 链长度。
    pub chain_len: usize,
    /// 被压制数。
    pub suppressed: usize,
    /// 事件总数。
    pub event_count: usize,
}

impl ReviewReport {
    /// 从引擎生成。
    pub fn build(engine: &DualDogReview) -> Self {
        let chain = engine.attribute();
        let first_dog = chain.first().map(|n| n.event.dog).unwrap_or(DogId::Soft);
        ReviewReport {
            first_dog,
            cause: engine.root_cause(),
            chain_len: chain.len(),
            suppressed: engine.suppressed_count(),
            event_count: engine.events().len(),
        }
    }

    /// 报告是否自洽（链非空时首触唯一、根因非 Unknown 才算闭合）。
    pub fn coherent(&self) -> bool {
        if self.event_count == 0 {
            return true;
        }
        self.chain_len >= 1 && self.cause != RootCause::Unknown
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

use super::super::checks::CheckSet;

pub fn run_unx297_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0297 双狗联动复盘");

    // --- 配置校验 ---
    let cfg = DualDogConfig::default_cfg();
    cs.add("默认配置合法", cfg.validate(), "soft<nmi, grace<=nmi, live>=2");
    let bad = DualDogConfig {
        soft_timeout: 10,
        nmi_timeout: 5,
        grace_window: 0,
        livelock_threshold: 1,
    };
    cs.add("倒置配置拒收", !bad.validate(), "nmi<soft 非法");
    let bad2 = DualDogConfig {
        soft_timeout: 10,
        nmi_timeout: 20,
        grace_window: 50,
        livelock_threshold: 3,
    };
    cs.add("超窗宽限拒收", !bad2.validate(), "grace>nmi 非法");

    // --- tsc 单调 ---
    let mut e = DualDogReview::new(cfg);
    cs.add("首事件可推", e.push(DogId::Soft, 100, DogEventKind::Fed), "seq=0");
    cs.add("同刻可推", e.push(DogId::Nmi, 100, DogEventKind::Fed), "tsc 相等允许");
    cs.add("递增可推", e.push(DogId::Soft, 200, DogEventKind::Fired), "tsc 递增");
    cs.add("回退被拒", !e.push(DogId::Nmi, 50, DogEventKind::Fired), "tsc<last 拒收");

    // --- 空链 ---
    let empty = DualDogReview::new(cfg);
    cs.add("空事件链空", empty.attribute().is_empty(), "无 Fired");
    cs.add("空根因 Unknown", empty.root_cause() == RootCause::Unknown, "证据不足");

    // --- 首触唯一 + 同刻 Soft 优先 ---
    let mut e2 = DualDogReview::new(cfg);
    e2.push(DogId::Nmi, 500, DogEventKind::Fired);
    e2.push(DogId::Soft, 500, DogEventKind::Fired);
    let chain2 = e2.attribute();
    cs.add("同刻 Soft 首触", chain2.first().map(|n| n.event.dog) == Some(DogId::Soft), "Soft 优先");
    cs.add("首触角色 First", chain2.first().map(|n| n.role) == Some(ChainRole::First), "role=First");

    // --- 宽限压制 ---
    let mut e3 = DualDogReview::new(cfg);
    e3.push(DogId::Soft, 1000, DogEventKind::Fired);
    e3.push(DogId::Nmi, 1000 + cfg.grace_window, DogEventKind::Fired); // 窗口内不同狗 → 压制
    let chain3 = e3.attribute();
    cs.add("窗口内异狗压制", chain3.len() == 2, "两节点");
    cs.add("第二节点 Suppressed", chain3.get(1).map(|n| n.role) == Some(ChainRole::Suppressed), "grace 内异狗");
    cs.add("压制计数=1", e3.suppressed_count() == 1, "suppressed==1");

    // --- 窗口外跟随 ---
    let mut e4 = DualDogReview::new(cfg);
    e4.push(DogId::Soft, 1000, DogEventKind::Fired);
    e4.push(DogId::Nmi, 1000 + cfg.grace_window + 1, DogEventKind::Fired);
    let chain4 = e4.attribute();
    cs.add("窗口外 Follow", chain4.get(1).map(|n| n.role) == Some(ChainRole::Follow), "超窗");

    // --- 同狗窗口内也是 Follow（不压制同狗） ---
    let mut e5 = DualDogReview::new(cfg);
    e5.push(DogId::Soft, 1000, DogEventKind::Fired);
    e5.push(DogId::Soft, 1000 + 10, DogEventKind::Fired);
    let chain5 = e5.attribute();
    cs.add("同狗不压制", chain5.get(1).map(|n| n.role) == Some(ChainRole::Follow), "同狗 Follow");

    // --- 根因：无 Fed → TrueHang ---
    let mut e6 = DualDogReview::new(cfg);
    e6.push(DogId::Nmi, 100, DogEventKind::Fired);
    cs.add("无 Fed 根因真挂", e6.root_cause() == RootCause::TrueHang, "true_hang");

    // --- 根因：有 Fed 且 Fed 晚于首触 → TooSlow ---
    let mut e7 = DualDogReview::new(cfg);
    e7.push(DogId::Soft, 100, DogEventKind::Fired);
    e7.push(DogId::Soft, 300, DogEventKind::Fed);
    cs.add("Fed 晚于首触 → 慢", e7.root_cause() == RootCause::TooSlow, "too_slow");

    // --- 根因：多次 Fired 且 Fed 全在首触前 → Livelock ---
    let mut e8 = DualDogReview::new(cfg);
    e8.push(DogId::Soft, 50, DogEventKind::Fed);
    e8.push(DogId::Soft, 100, DogEventKind::Fired);
    e8.push(DogId::Soft, 200, DogEventKind::Fired);
    e8.push(DogId::Nmi, 300, DogEventKind::Fired);
    cs.add("三触无后续 Fed → 活锁", e8.root_cause() == RootCause::Livelock, "livelock");

    // --- 时间线 ---
    let rows = e3.timeline_rows();
    cs.add("时间线条数=事件数", rows.len() == e3.events().len(), "1:1");
    cs.add("时间线首条 tsc", rows.first().map(|r| r.0) == Some(1000), "首 tsc=1000");

    // --- 报告 ---
    let rpt = ReviewReport::build(&e3);
    cs.add("报告首触 Soft", rpt.first_dog == DogId::Soft, "first_dog");
    cs.add("报告链长 2", rpt.chain_len == 2, "chain_len");
    cs.add("报告压制 1", rpt.suppressed == 1, "suppressed");
    cs.add("报告自洽", rpt.coherent(), "非 Unknown 且链闭合");

    let rpt_empty = ReviewReport::build(&empty);
    cs.add("空报告自洽", rpt_empty.coherent(), "空事件集合法");

    // --- 根因名字 ---
    cs.add("根因名真挂", RootCause::TrueHang.name() == "true_hang", "name");
    cs.add("狗名软", DogId::Soft.name() == "soft", "soft");
    cs.add("狗名硬", DogId::Nmi.name() == "nmi", "nmi");

    cs
}
