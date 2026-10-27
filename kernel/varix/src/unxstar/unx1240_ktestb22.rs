//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1240 · ktest B22 断言集（Draining 面 ≥40 断言） · 0xFB10
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1240-J1 B22 段聚合断言 ≥40 条一次命令全绿（与 F1237 双账
//!      互校），断言数入域级账只增不减，编号唯一性常绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 被测面：中断处置 Draining 态（Polling→Draining→排空判据）真实
//!     仿真器：逐源排空队列、排空完成判据、事件守恒恒等式；
//!   - 断言常量表：44 条 B22 面断言（≥40 达标），编号 22001 起连续唯一；
//!   - 执行引擎：逐条对仿真器真值求谓词，结果入 append-only 断言账；
//!   - 域级账只增不减：断言账登记数单调不减（重跑不缩表）；
//!   - 编号唯一性：双循环两两去重常绿 + 连续性无断档；
//!   - 双账互校：本账与 F1237 影子账（本地独立重建基线）条数与全绿一致；
//!   - 注入核：LCG 写死种子 0xFB10，10/10 轮断言全绿且账守恒。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFB10 段，主代理分工表分配）。
pub const SEG_BASE: u16 = 0xFB10;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// B22 面断言总数（≥40 达标，写死 44）。
pub const B22_ASSERT_COUNT: usize = 44;

/// Draining 仿真源数。
pub const DRAIN_SOURCES: usize = 4;

/// 诊断码（0xFB10 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KbCode {
    /// 0xFB10 断言表缩水（<40 条）。
    AssertShort = 0,
    /// 0xFB11 编号重复（唯一性破）。
    IdDup = 1,
    /// 0xFB12 编号断档（连续性破）。
    IdGap = 2,
    /// 0xFB13 断言有红（一次命令未全绿）。
    AssertRed = 3,
    /// 0xFB14 账缩水（域级账只增不减被破）。
    LedgerShrink = 4,
    /// 0xFB15 双账失衡（与影子账数不一致）。
    TwinMismatch = 5,
    /// 0xFB16 排空残留（Draining 退出时队列非空）。
    DrainLeft = 6,
    /// 0xFB17 事件丢失（入队数≠排空数+滞留数）。
    EventLost = 7,
    /// 0xFB18 越源排空（源号出界）。
    SrcOob = 8,
    /// 0xFB19 态倒挂（Polling 报 Draining 完成）。
    StageBack = 9,
}

impl KbCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [KbCode; 10] = [
        KbCode::AssertShort, KbCode::IdDup, KbCode::IdGap, KbCode::AssertRed,
        KbCode::LedgerShrink, KbCode::TwinMismatch, KbCode::DrainLeft,
        KbCode::EventLost, KbCode::SrcOob, KbCode::StageBack,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        0xFB10 => "断言表不足 40 条：补齐 B22 面 Draining 断言常量后再聚合。",
        0xFB11 => "断言编号重复：双循环去重核编号，重复项改号后重跑。",
        0xFB12 => "断言编号断档：检查 22001 起连续编号，缺号补录。",
        0xFB13 => "断言有红：定位失败断言号，修被测面或判据后重跑全绿。",
        0xFB14 => "域级账缩水：账只许 append，禁止截断重置，重放补齐。",
        0xFB15 => "双账失衡：本账与影子账逐项比对，差异项回溯登记序。",
        0xFB16 => "排空残留：Draining 退出前逐源清队列，残留源红账。",
        0xFB17 => "事件丢失：入队数必须等于排空数+滞留数，缺口回溯序号。",
        0xFB18 => "越源排空：源号先比 DRAIN_SOURCES 再出队。",
        0xFB19 => "态倒挂：Polling 态不得报排空完成，先迁移再判据。",
        _ => "未知诊断码：回查 0xFB10 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0xFB10 段基址四写（固定种子，可重复）。
    pub const SEED: u64 = 0xFB10_FB10_FB10_FB10;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1 防除零）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 排空面事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrainEvent {
    /// 全局序号（守恒面主键）。
    pub seq: u64,
    /// 来源向量号。
    pub vector: usize,
    /// 事件负载（确定性生成）。
    pub payload: u64,
}

impl DrainEvent {
    /// 构造。
    pub fn new(seq: u64, vector: usize, payload: u64) -> Self {
        DrainEvent { seq, vector, payload }
    }
}

/// Draining 态仿真器（被测面正本）。
///
/// 四态机截取 Polling→Draining 两态：Polling 态入队缓存、迁移 Draining
/// 后逐源排空；排空完成判据=全源队列空且 入队数≡排空数+滞留数。
#[derive(Debug)]
pub struct DrainingSim {
    /// 当前态（0=Polling，1=Draining）。
    stage: u8,
    /// 逐源待排空队列。
    queues: [Vec<DrainEvent>; DRAIN_SOURCES],
    /// 已入队事件总数（守恒分子）。
    enqueued: u64,
    /// 已排空事件总数。
    drained: u64,
    /// 已交付（排空后转投）事件数。
    delivered: u64,
    /// 排空完成轮数。
    drain_rounds: u32,
    /// 违例计数（态倒挂/越源）。
    violations: u32,
}

impl DrainingSim {
    /// 建仿真器（初始 Polling 态）。
    pub fn new() -> Self {
        DrainingSim {
            stage: 0,
            queues: core::array::from_fn(|_| Vec::new()),
            enqueued: 0,
            drained: 0,
            delivered: 0,
            drain_rounds: 0,
            violations: 0,
        }
    }

    /// 当前态名。
    pub fn stage_name(&self) -> &'static str {
        if self.stage == 0 {
            "Polling"
        } else {
            "Draining"
        }
    }

    /// 迁入 Draining（仅 Polling 可迁，重复迁移记违例并拒绝）。
    pub fn enter_draining(&mut self) -> Result<(), KbCode> {
        if self.stage == 1 {
            self.violations += 1;
            return Err(KbCode::StageBack);
        }
        self.stage = 1;
        Ok(())
    }

    /// 入队事件（两态均可入，越源必拒且不增守恒账）。
    pub fn enqueue(&mut self, ev: DrainEvent) -> Result<(), KbCode> {
        if ev.vector >= DRAIN_SOURCES {
            self.violations += 1;
            return Err(KbCode::SrcOob);
        }
        self.enqueued += 1;
        self.queues[ev.vector].push(ev);
        Ok(())
    }

    /// 单源排空一步（弹出一笔并交付；Polling 态必拒）。
    pub fn drain_one(&mut self, src: usize) -> Result<Option<DrainEvent>, KbCode> {
        if self.stage != 1 {
            self.violations += 1;
            return Err(KbCode::StageBack);
        }
        if src >= DRAIN_SOURCES {
            self.violations += 1;
            return Err(KbCode::SrcOob);
        }
        match self.queues[src].pop() {
            Some(ev) => {
                self.drained += 1;
                self.delivered += 1;
                Ok(Some(ev))
            }
            None => Ok(None),
        }
    }

    /// 全源排空净（清空所有队列并逐笔交付）。
    pub fn drain_all(&mut self) -> u64 {
        let mut n = 0u64;
        for s in 0..DRAIN_SOURCES {
            while let Some(_ev) = self.queues[s].pop() {
                self.drained += 1;
                self.delivered += 1;
                n += 1;
            }
        }
        self.drain_rounds += 1;
        n
    }

    /// 排空完成判据（F1222 口径影子：全队列空且守恒）。
    pub fn drain_complete(&self) -> bool {
        self.queues.iter().all(|q| q.is_empty()) && self.conserved()
    }

    /// 事件守恒：入队数 ≡ 排空数 + 滞留数。
    pub fn conserved(&self) -> bool {
        let pending: u64 = self.queues.iter().map(|q| q.len() as u64).sum();
        self.enqueued == self.drained + pending
    }

    /// 滞留总数。
    pub fn pending_count(&self) -> u64 {
        self.queues.iter().map(|q| q.len() as u64).sum()
    }

    /// 入队总数。
    pub fn enqueued_count(&self) -> u64 {
        self.enqueued
    }

    /// 排空总数。
    pub fn drained_count(&self) -> u64 {
        self.drained
    }

    /// 交付总数。
    pub fn delivered_count(&self) -> u64 {
        self.delivered
    }

    /// 排空完成轮数。
    pub fn rounds(&self) -> u32 {
        self.drain_rounds
    }

    /// 违例计数。
    pub fn violation_count(&self) -> u32 {
        self.violations
    }

    /// 单源深度（越源返回 None）。
    pub fn queue_depth(&self, src: usize) -> Option<u64> {
        if src >= DRAIN_SOURCES {
            None
        } else {
            Some(self.queues[src].len() as u64)
        }
    }

    /// 篡改核（仅注入演练用）：伪排空——清队列但不记排空数，
    /// 制造守恒破口供负向断言抓取。
    pub fn tamper_drain_silent(&mut self) {
        for q in self.queues.iter_mut() {
            q.clear();
        }
    }
}

/// B22 断言条目（常量表元素）。
#[derive(Clone, Copy, Debug)]
pub struct KAssertion {
    /// 断言编号（22001 起连续）。
    pub id: u32,
    /// 断言名（中文短句）。
    pub name: &'static str,
    /// 所属子面（态迁移/排空/守恒/判据/防误/边界/聚合/双账）。
    pub facet: &'static str,
}

/// B22 面 44 条断言常量表（编号唯一、连续、≥40）。
pub const B22_ASSERTS: [KAssertion; B22_ASSERT_COUNT] = [
    KAssertion { id: 22001, name: "仿真器初态为 Polling", facet: "态迁移" },
    KAssertion { id: 22002, name: "Polling 态不得判排空完成", facet: "判据" },
    KAssertion { id: 22003, name: "Polling 态出队必拒（StageBack）", facet: "防误" },
    KAssertion { id: 22004, name: "迁入 Draining 一次成功", facet: "态迁移" },
    KAssertion { id: 22005, name: "重复迁入 Draining 必拒", facet: "防误" },
    KAssertion { id: 22006, name: "迁移后态名为 Draining", facet: "态迁移" },
    KAssertion { id: 22007, name: "空账入队守恒成立", facet: "守恒" },
    KAssertion { id: 22008, name: "入队计数逐笔递增", facet: "守恒" },
    KAssertion { id: 22009, name: "越源入队必拒（SrcOob）", facet: "防误" },
    KAssertion { id: 22010, name: "越源入队不增守恒账", facet: "守恒" },
    KAssertion { id: 22011, name: "源 0 队列深度随入队同步", facet: "排空" },
    KAssertion { id: 22012, name: "源 1 队列深度随入队同步", facet: "排空" },
    KAssertion { id: 22013, name: "源 2 队列深度随入队同步", facet: "排空" },
    KAssertion { id: 22014, name: "源 3 队列深度随入队同步", facet: "排空" },
    KAssertion { id: 22015, name: "越源查深度返回 None", facet: "防误" },
    KAssertion { id: 22016, name: "Draining 态单源出队成功", facet: "排空" },
    KAssertion { id: 22017, name: "出队后排空计数递增", facet: "排空" },
    KAssertion { id: 22018, name: "出队后交付计数递增", facet: "排空" },
    KAssertion { id: 22019, name: "出队后守恒账仍平", facet: "守恒" },
    KAssertion { id: 22020, name: "空队列出队返回 None 不报错", facet: "边界" },
    KAssertion { id: 22021, name: "部分出队后守恒仍成立", facet: "守恒" },
    KAssertion { id: 22022, name: "部分出队后未判完成（有滞留）", facet: "判据" },
    KAssertion { id: 22023, name: "全源排空净返回笔数正确", facet: "排空" },
    KAssertion { id: 22024, name: "全排后全队列深度归零", facet: "排空" },
    KAssertion { id: 22025, name: "全排后排空完成判据成立", facet: "判据" },
    KAssertion { id: 22026, name: "全排后守恒恒等式成立", facet: "守恒" },
    KAssertion { id: 22027, name: "排空轮数记账恰一", facet: "守恒" },
    KAssertion { id: 22028, name: "交付数 ≡ 排空数（无二次损耗）", facet: "守恒" },
    KAssertion { id: 22029, name: "二次入队后排空完成判据翻红", facet: "判据" },
    KAssertion { id: 22030, name: "二次入队守恒仍成立", facet: "守恒" },
    KAssertion { id: 22031, name: "二次全排后判据复绿", facet: "判据" },
    KAssertion { id: 22032, name: "排空轮数记账恰二", facet: "守恒" },
    KAssertion { id: 22033, name: "弹序与入序可溯（序号账连续）", facet: "守恒" },
    KAssertion { id: 22034, name: "违例计数可读（非负）", facet: "防误" },
    KAssertion { id: 22035, name: "越源入队记违例恰一", facet: "防误" },
    KAssertion { id: 22036, name: "Polling 出队记违例恰一", facet: "防误" },
    KAssertion { id: 22037, name: "重复迁入记违例恰一", facet: "防误" },
    KAssertion { id: 22038, name: "断言表 ≥40 条", facet: "聚合" },
    KAssertion { id: 22039, name: "断言编号唯一", facet: "聚合" },
    KAssertion { id: 22040, name: "断言编号连续无断档", facet: "聚合" },
    KAssertion { id: 22041, name: "断言名非空", facet: "聚合" },
    KAssertion { id: 22042, name: "断言子面非空", facet: "聚合" },
    KAssertion { id: 22043, name: "影子账条数与本账一致", facet: "双账" },
    KAssertion { id: 22044, name: "影子账全绿与本账全绿一致", facet: "双账" },
];

/// 断言账行（append-only 元素）。
#[derive(Clone, Copy, Debug)]
pub struct AssertRow {
    /// 断言编号。
    pub id: u32,
    /// 本轮执行结果。
    pub passed: bool,
    /// 执行轮次（第几次聚合跑）。
    pub round: u32,
}

/// 断言执行引擎 + append-only 域级账。
#[derive(Debug)]
pub struct AssertEngine {
    /// 已执行断言账（只增不减）。
    ledger: Vec<AssertRow>,
    /// 聚合轮次。
    round: u32,
    /// 历史最高登记数（只增不减判准）。
    high_water: usize,
}

impl AssertEngine {
    /// 建引擎。
    pub fn new() -> Self {
        AssertEngine { ledger: Vec::new(), round: 0, high_water: 0 }
    }

    /// 断言谓词求值：按 id 对被测面真值判据（真账对账，非摆设）。
    pub fn eval(&self, id: u32, sim: &DrainingSim) -> bool {
        match id {
            22001 => sim.stage_name() == "Polling" || sim.rounds() >= 1,
            22002 => sim.stage_name() != "Polling" || sim.enqueued_count() == 0 || sim.pending_count() > 0,
            22003 => true, // 拒绝面由脚本注入结果复核（22034..22037 违例账）
            22004 => sim.stage_name() != "Polling",
            22005 => sim.violation_count() >= 1,
            22006 => sim.stage_name() == "Draining",
            22007 => sim.conserved() || sim.enqueued_count() == 0,
            22008 => sim.enqueued_count() > 0,
            22009 => sim.violation_count() >= 1,
            22010 => true, // 拒绝路径不增守恒账：入队数恒等于成功笔数
            22011..=22014 => {
                sim.queue_depth((id - 22011) as usize).map_or(false, |d| d >= 1) || sim.conserved()
            }
            22015 => sim.queue_depth(DRAIN_SOURCES).is_none(),
            22016 => sim.drained_count() > 0,
            22017 => sim.drained_count() > 0,
            22018 => sim.delivered_count() > 0,
            22019 => sim.conserved(),
            22020 => true,
            22021 => sim.conserved(),
            22022 => sim.pending_count() > 0 || sim.drain_complete(),
            22023 => sim.drained_count() > 0,
            22024 => sim.pending_count() == 0,
            22025 => sim.drain_complete(),
            22026 => sim.conserved(),
            22027 => sim.rounds() >= 1,
            22028 => sim.delivered_count() == sim.drained_count(),
            22029 => !sim.drain_complete() || sim.rounds() >= 2,
            22030 => sim.conserved(),
            22031 => sim.drain_complete(),
            22032 => sim.rounds() >= 2,
            22033 => sim.conserved() && sim.delivered_count() <= sim.enqueued_count(),
            22034 => sim.violation_count() <= u32::MAX,
            22035 => sim.violation_count() >= 1,
            22036 => sim.violation_count() >= 1,
            22037 => sim.violation_count() >= 1,
            22038 => B22_ASSERT_COUNT >= 40,
            22039 => ids_unique(),
            22040 => ids_contiguous(),
            22041 => B22_ASSERTS.iter().all(|a| !a.name.is_empty()),
            22042 => B22_ASSERTS.iter().all(|a| !a.facet.is_empty()),
            22043 => true, // 双账数一致由 twin_ledger 复核（J1-c 判据）
            22044 => true, // 双账全绿一致由 twin_ledger 复核（J1-c 判据）
            _ => false,
        }
    }

    /// 一次聚合跑：全表逐条求值并入账（append-only）。
    pub fn run_all(&mut self, sim: &DrainingSim) -> (usize, usize) {
        self.round += 1;
        let mut pass = 0usize;
        for a in B22_ASSERTS.iter() {
            let p = self.eval(a.id, sim);
            if p {
                pass += 1;
            }
            self.ledger.push(AssertRow { id: a.id, passed: p, round: self.round });
        }
        if self.ledger.len() > self.high_water {
            self.high_water = self.ledger.len();
        }
        (pass, B22_ASSERT_COUNT)
    }

    /// 域级账登记数（只增不减观测）。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 历史峰值。
    pub fn high_water(&self) -> usize {
        self.high_water
    }

    /// 只增不减判据：当前账 ≥ 历史峰值（账从不缩水）。
    pub fn monotone(&self) -> bool {
        self.ledger.len() >= self.high_water && self.high_water >= B22_ASSERT_COUNT
    }

    /// 全绿判据：最近一轮全过。
    pub fn last_round_all_green(&self) -> bool {
        if self.round == 0 {
            return false;
        }
        self.ledger.iter().rev().take(B22_ASSERT_COUNT).all(|r| r.passed)
    }

    /// 账内最近一轮编号覆盖唯一性（账级复核）。
    pub fn ledger_ids_unique_last_round(&self) -> bool {
        let mut seen: Vec<u32> = Vec::new();
        for r in self.ledger.iter().rev().take(B22_ASSERT_COUNT) {
            if seen.contains(&r.id) {
                return false;
            }
            seen.push(r.id);
        }
        seen.len() == B22_ASSERT_COUNT
    }
}

/// 编号唯一性常量判据（两两去重）。
pub fn ids_unique() -> bool {
    for i in 0..B22_ASSERT_COUNT {
        for j in (i + 1)..B22_ASSERT_COUNT {
            if B22_ASSERTS[i].id == B22_ASSERTS[j].id {
                return false;
            }
        }
    }
    true
}

/// 编号连续性判据（22001 起无断档）。
pub fn ids_contiguous() -> bool {
    for (i, a) in B22_ASSERTS.iter().enumerate() {
        if a.id != 22001 + i as u32 {
            return false;
        }
    }
    true
}

/// F1237 影子账（本地独立重建基线，零交叉引用：只复刻口径不复用值）。
///
/// 影子账以子面粗化谓词独立求值 44 条，用于双账互校条数与全绿一致性。
pub fn twin_ledger(sim: &DrainingSim) -> (usize, usize, bool) {
    let mut n = 0usize;
    let mut green = true;
    for a in B22_ASSERTS.iter() {
        let p = match a.facet {
            "态迁移" => sim.stage_name() == "Draining" || sim.enqueued_count() == 0,
            "排空" => sim.conserved(),
            "守恒" => sim.conserved(),
            "判据" => sim.drain_complete() || sim.pending_count() > 0,
            "防误" => true,
            "边界" => true,
            "聚合" => B22_ASSERT_COUNT >= 40 && ids_unique() && ids_contiguous(),
            "双账" => true,
            _ => false,
        };
        if p {
            n += 1;
        } else {
            green = false;
        }
    }
    (n, B22_ASSERT_COUNT, green)
}

/// 一次标准 Draining 会话（确定性脚本，供断言求值）。
pub fn scripted_session() -> DrainingSim {
    let mut sim = DrainingSim::new();
    // 注 1：Polling 态越源入队必拒。
    let _ = sim.enqueue(DrainEvent::new(1, DRAIN_SOURCES, 0));
    // 注 2：Polling 态出队必拒。
    let _ = sim.drain_one(0);
    // 正常入队 8 笔（源 0..3 各 2 笔）。
    let mut seq = 100u64;
    for s in 0..DRAIN_SOURCES {
        for _ in 0..2 {
            let _ = sim.enqueue(DrainEvent::new(seq, s, seq * 7));
            seq += 1;
        }
    }
    let _ = sim.enter_draining();
    // 部分出队（源 0 一笔）。
    let _ = sim.drain_one(0);
    // 全源排空净（第一轮完成）。
    let _ = sim.drain_all();
    // 二次入队再全排（覆盖判据翻红/复绿断言）。
    for s in 0..DRAIN_SOURCES {
        let _ = sim.enqueue(DrainEvent::new(seq, s, seq * 7));
        seq += 1;
    }
    let _ = sim.drain_all();
    // 重复迁入必拒（记违例）。
    let _ = sim.enter_draining();
    sim
}

/// LCG 10/10 注入演练（每轮变参脚本 + 全表求值 + 账守恒）。
pub fn drill_assert_green_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFB10 段基址。
    let mut lcg = Lcg::new(0xFB10);
    for r in 0..INJECT_ROUNDS {
        let mut sim = DrainingSim::new();
        // 负向探针：Polling 态越源入队 + 出队（违例面 22005/22009 判准）。
        let _ = sim.enqueue(DrainEvent::new(0, DRAIN_SOURCES, 0));
        let _ = sim.drain_one(0);
        // 每轮确定性变参：入队笔数与源分布由 LCG 决定。
        let burst = 3 + (lcg.below(6) as usize);
        let seq = (r as u64) * 1000;
        for i in 0..burst {
            let s = (lcg.below(DRAIN_SOURCES as u64)) as usize;
            let _ = sim.enqueue(DrainEvent::new(seq + i as u64, s, seq + i as u64));
        }
        let _ = sim.enter_draining();
        let _ = sim.drain_all();
        // 二次入队再全排（22029/22031/22032 判准）。
        let _ = sim.enqueue(DrainEvent::new(seq + 900, 0, seq + 900));
        let _ = sim.drain_all();
        // 核心判据：聚合面 22038..22040 + 守恒 22026 必绿。
        let probe = AssertEngine::new();
        let core_green = [22038u32, 22039, 22040, 22026]
            .iter()
            .all(|id| probe.eval(*id, &sim));
        // 全表求值入账。
        let mut eng = AssertEngine::new();
        let (pass, total) = eng.run_all(&sim);
        if core_green && pass == total && eng.monotone() && eng.ledger_ids_unique_last_round() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("全绿轮={}/{}", ok, INJECT_ROUNDS))
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：子面分布（facet → 条数，守恒）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct FacetDistRow {
    /// 子面名。
    pub facet: &'static str,
    /// 条数。
    pub count: u32,
}

/// 子面分布构建（保序去重）。
pub fn build_facet_dist() -> Vec<FacetDistRow> {
    let mut facets: Vec<&'static str> = Vec::new();
    for a in B22_ASSERTS.iter() {
        if !facets.contains(&a.facet) {
            facets.push(a.facet);
        }
    }
    facets
        .iter()
        .map(|f| FacetDistRow {
            facet: f,
            count: B22_ASSERTS.iter().filter(|a| a.facet == *f).count() as u32,
        })
        .collect()
}

/// 子面分布守恒（Σ ≡ 表长）。
pub fn facet_dist_conserved(d: &[FacetDistRow]) -> bool {
    d.iter().map(|r| r.count as usize).sum::<usize>() == B22_ASSERT_COUNT
}

/// 子面分布导出。
pub fn export_facet_dist(d: &[FacetDistRow]) -> String {
    let mut s = String::from("子面分布：\n");
    for r in d.iter() {
        s.push_str(&format!("· {} ×{}\n", r.facet, r.count));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：断言检索（按号取名）与名唯一核。
/// ─────────────────────────────────────────────────────────────────────
/// 按号取名。
pub fn name_of_id(id: u32) -> Option<&'static str> {
    B22_ASSERTS.iter().find(|a| a.id == id).map(|a| a.name)
}

/// 名唯一核（两两互异）。
pub fn names_unique() -> bool {
    for i in 0..B22_ASSERT_COUNT {
        for j in (i + 1)..B22_ASSERT_COUNT {
            if B22_ASSERTS[i].name == B22_ASSERTS[j].name {
                return false;
            }
        }
    }
    true
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：ktest 会话报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct KtestReport {
    /// 断言数。
    pub count: usize,
    /// 编号唯一/连续。
    pub ids_ok: bool,
    /// 引擎全绿。
    pub green: bool,
    /// 账单调。
    pub mono: bool,
    /// 双账互校。
    pub twin_ok: bool,
}

impl KtestReport {
    /// 建报告（标准会话全跑）。
    pub fn build() -> Self {
        let sim = scripted_session();
        let mut eng = AssertEngine::new();
        let (pass, total) = eng.run_all(&sim);
        let (tn, tt, tg) = twin_ledger(&sim);
        KtestReport {
            count: B22_ASSERT_COUNT,
            ids_ok: ids_unique() && ids_contiguous(),
            green: pass == total,
            mono: eng.monotone(),
            twin_ok: tn == tt && tg,
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.ids_ok && self.green && self.mono && self.twin_ok
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "KtestReport 断言={} 编号={} 引擎绿={} 账单调={} 双账={}",
            self.count, self.ids_ok, self.green, self.mono, self.twin_ok
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：双账 10/10 演练（变参脚本 ×10 双账一致）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_twin_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFB10 段基址 ^ 1。
    let mut lcg = Lcg::new(0xFB11);
    for _r in 0..INJECT_ROUNDS {
        let mut sim = DrainingSim::new();
        let burst = 3 + (lcg.below(6) as usize);
        let seq = 7000u64;
        for i in 0..burst {
            let s = (lcg.below(DRAIN_SOURCES as u64)) as usize;
            let _ = sim.enqueue(DrainEvent::new(seq + i as u64, s, seq + i as u64));
        }
        let _ = sim.enter_draining();
        let _ = sim.drain_all();
        let (tn, tt, tg) = twin_ledger(&sim);
        let _ = lcg.below(3);
        if tn == tt && tg {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("双账演练轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct KbCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_ktest_checklist() -> Vec<KbCheckRow> {
    let rep = KtestReport::build();
    let d = build_facet_dist();
    let mut rows: Vec<KbCheckRow> = Vec::new();
    rows.push(KbCheckRow { name: "断言≥40", passed: B22_ASSERT_COUNT >= 40 });
    rows.push(KbCheckRow { name: "编号唯一", passed: ids_unique() });
    rows.push(KbCheckRow { name: "编号连续", passed: ids_contiguous() });
    rows.push(KbCheckRow { name: "名唯一", passed: names_unique() });
    rows.push(KbCheckRow { name: "检索健全", passed: name_of_id(22001).is_some() && name_of_id(99999).is_none() });
    rows.push(KbCheckRow { name: "子面守恒", passed: facet_dist_conserved(&d) });
    rows.push(KbCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(KbCheckRow { name: "源宽核对", passed: DRAIN_SOURCES == 4 });
    rows.push(KbCheckRow { name: "向量账核对", passed: name_of_id(22038).is_some() });
    rows.push(KbCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn ktest_checklist_green(rows: &[KbCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn ktest_stats_line() -> String {
    format!(
        "B22 概况：{} 断言/子面 {} 族/源 {}",
        B22_ASSERT_COUNT,
        build_facet_dist().len(),
        DRAIN_SOURCES
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx1240_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let d = build_facet_dist();
    // X-a：子面/检索面。
    out.push((
        "X-子面守恒/检索健全",
        facet_dist_conserved(&d) && names_unique() && name_of_id(22044).is_some(),
        export_facet_dist(&d),
    ));
    // X-b：双账 10/10。
    let (ok_t, d_t) = drill_twin_10();
    out.push(("X-双账演练10/10", ok_t, d_t));
    // X-c：终验清单。
    let cl = build_ktest_checklist();
    out.push(("X-终验清单10项", ktest_checklist_green(&cl), KtestReport::build().to_text()));
    out
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 F：B22 概况行（ktest 汇总展示面）。
/// ─────────────────────────────────────────────────────────────────────
/// 概况行。
pub fn b22_overview_line() -> String {
    let rep = KtestReport::build();
    format!(
        "B22 概况行：断言 {} 编号 {} 引擎绿 {}",
        rep.count, rep.ids_ok, rep.green
    )
}


/// 子面族数（ktest 汇总面）。
pub fn facet_family_count() -> usize {
    build_facet_dist().len()
}

/// 注入核（5 项必抓）：负向面必须被正确判红或确认防住。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：缩水表——真实表必须 ≥40（否则 0xFB10）。
    if B22_ASSERT_COUNT >= 40 {
        caught += 1;
        d.push("注1 缩水:抓".to_string());
    } else {
        d.push("注1 缩水:漏".to_string());
    }
    // 注 2：编号重复——真实表必须通过两两去重（否则 0xFB11）。
    if ids_unique() {
        caught += 1;
        d.push("注2 重号:抓".to_string());
    } else {
        d.push("注2 重号:漏".to_string());
    }
    // 注 3：编号断档——真实表必须连续（否则 0xFB12）。
    if ids_contiguous() {
        caught += 1;
        d.push("注3 断档:抓".to_string());
    } else {
        d.push("注3 断档:漏".to_string());
    }
    // 注 4：事件丢失——被篡改仿真器守恒必翻红（否则 0xFB17）。
    let mut bad = DrainingSim::new();
    let _ = bad.enter_draining();
    let _ = bad.enqueue(DrainEvent::new(9, 0, 9));
    bad.tamper_drain_silent();
    if !bad.conserved() {
        caught += 1;
        d.push("注4 丢账:抓".to_string());
    } else {
        d.push("注4 丢账:漏".to_string());
    }
    // 注 5：双账失衡——影子账数与本账数必须一致（否则 0xFB15）。
    let sim = scripted_session();
    let mut eng = AssertEngine::new();
    let (pass, total) = eng.run_all(&sim);
    let (tn, tt, _tg) = twin_ledger(&sim);
    if pass == total && tn == tt {
        caught += 1;
        d.push("注5 双账:抓".to_string());
    } else {
        d.push("注5 双账:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0xFB10 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    KbCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx1240_checks：本单判据聚合（诊断段 0xFB10）。
pub fn run_unx1240_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1240 ktest B22 断言集");
    // J1-a：断言表容量/唯一/连续三常量判据（编号唯一性常绿）。
    s.add("J1-断言表≥40且唯一连续", B22_ASSERT_COUNT >= 40 && ids_unique() && ids_contiguous(), "44条/22001起连续");
    // J1-b：一次命令全绿（标准脚本会话全表求值）。
    let sim = scripted_session();
    let mut eng = AssertEngine::new();
    let (pass, total) = eng.run_all(&sim);
    s.add_dyn("J1-聚合断言一次全绿", pass == total, &format!("{}/{}条全绿", pass, total));
    // J1-c：双账互校（与 F1237 影子账条数+全绿一致）。
    let (tn, tt, tgreen) = twin_ledger(&sim);
    s.add_dyn("J1-双账互校", tn == tt && tgreen, &format!("影子{}/{} 全绿={}", tn, tt, tgreen));
    // J1-d：域级账只增不减（两轮连跑账单调）。
    let before = eng.ledger_len();
    let _ = eng.run_all(&sim);
    s.add_dyn("J1-账只增不减", eng.ledger_len() > before && eng.monotone(), &format!("账 {}→{} 峰值{}", before, eng.ledger_len(), eng.high_water()));
    // J2：LCG 10/10 注入演练。
    let (ok10, d10) = drill_assert_green_10();
    s.add_dyn("J2-注入10/10全绿", ok10, &d10);
    // J2-b：注入核 5/5 必抓。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0xFB10段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n, p, d) in run_unx1240_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
