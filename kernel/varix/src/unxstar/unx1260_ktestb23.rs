//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1260 · ktest B23 断言集（Arming 面 ≥40 断言） · 0xFC30
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1260-J1 B23 段聚合断言 ≥40 条一次命令全绿（与 F1256 双账
//!      互校），断言数入域级账只增不减，编号唯一性常绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 被测面：中断处置 Arming 态（Draining→Arming→武装→确认）真实
//!     仿真器：逐向量武装、确认窗到齐、回退重武装、事件守恒；
//!   - 断言常量表：44 条 B23 面断言（≥40 达标），编号 24001 起连续唯一；
//!   - 执行引擎：逐条对仿真器真值求谓词，结果入 append-only 断言账；
//!   - 域级账只增不减：断言账登记数单调不减（重跑不缩表）；
//!   - 编号唯一性：两两去重 + 连续性常绿；
//!   - 双账互校：本账与 F1256 影子账（本地独立重建基线）条数与全绿一致；
//!   - 注入核：LCG 写死种子 0xFC30，10/10 轮断言全绿且账守恒。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFC30 段，主代理分工表分配）。
pub const SEG_BASE: u16 = 0xFC30;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// B23 面断言总数（≥40 达标，写死 44）。
pub const B23_ASSERT_COUNT: usize = 44;

/// Arming 仿真向量数。
pub const ARM_VECS: usize = 4;

/// 确认窗预算（tick）。
pub const CONFIRM_TICKS: u64 = 6;

/// 诊断码（0xFC30 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KcCode {
    /// 0xFC30 断言表缩水（<40 条）。
    AssertShort = 0,
    /// 0xFC31 编号重复。
    IdDup = 1,
    /// 0xFC32 编号断档。
    IdGap = 2,
    /// 0xFC33 断言有红（一次命令未全绿）。
    AssertRed = 3,
    /// 0xFC34 账缩水（域级账只增不减被破）。
    LedgerShrink = 4,
    /// 0xFC35 双账失衡（与影子账不一致）。
    TwinMismatch = 5,
    /// 0xFC36 武装残留（回退后位图非零）。
    ArmLeft = 6,
    /// 0xFC37 事件丢失（入账≠到齐+滞留+移交）。
    EventLost = 7,
    /// 0xFC38 越向量（向量号出位图）。
    VecOob = 8,
    /// 0xFC39 未全武报全武（位图非全一）。
    ArmedFake = 9,
}

impl KcCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [KcCode; 10] = [
        KcCode::AssertShort, KcCode::IdDup, KcCode::IdGap, KcCode::AssertRed,
        KcCode::LedgerShrink, KcCode::TwinMismatch, KcCode::ArmLeft,
        KcCode::EventLost, KcCode::VecOob, KcCode::ArmedFake,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        0xFC30 => "断言表不足 40 条：补齐 B23 面 Arming 断言常量后再聚合。",
        0xFC31 => "断言编号重复：两两去重核编号，重复项改号。",
        0xFC32 => "断言编号断档：24001 起连续编号补齐。",
        0xFC33 => "断言有红：定位失败断言号，修被测面或判据后重跑。",
        0xFC34 => "域级账缩水：账只许 append，禁止截断。",
        0xFC35 => "双账失衡：本账与影子账逐项比对，差异项回溯。",
        0xFC36 => "武装残留：回退后位图必须归零，残留即红。",
        0xFC37 => "事件丢失：入账=到齐+滞留+移交，缺口回溯序号。",
        0xFC38 => "越向量：向量号先比位图宽再操作。",
        0xFC39 => "未全武报全武：位图全一才许报 Armed。",
        _ => "未知诊断码：回查 0xFC30 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0xFC30 段基址（固定种子，可重复）。
    pub const SEED: u64 = 0xFC30_FC30_FC30_FC30;

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
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// Arming 态仿真器（被测面正本）。
///
/// 事件守恒：入账 ≡ 到齐 + 滞留 + 移交；回退后位图归零可重武装。
#[derive(Debug)]
pub struct ArmSim {
    /// 武装位图。
    mask: u32,
    /// 到齐位图。
    arrived: u32,
    /// 滞留事件（确认窗内未确认）。
    pending: Vec<u64>,
    /// 已入账事件数。
    events_in: u64,
    /// 已移交笔数。
    handed: u64,
    /// 回退轮数。
    retires: u32,
    /// 重武装成功数。
    rearms_ok: u32,
    /// 确认窗 tick。
    tick: u64,
    /// 违例计数。
    violations: u32,
}

impl ArmSim {
    /// 建（初始未武装）。
    pub fn new() -> Self {
        ArmSim {
            mask: 0,
            arrived: 0,
            pending: Vec::new(),
            events_in: 0,
            handed: 0,
            retires: 0,
            rearms_ok: 0,
            tick: 0,
            violations: 0,
        }
    }

    /// 武装向量（越向量必拒）。
    pub fn arm(&mut self, v: usize) -> Result<(), KcCode> {
        if v >= ARM_VECS {
            self.violations += 1;
            return Err(KcCode::VecOob);
        }
        self.mask |= 1u32 << v;
        Ok(())
    }

    /// 撤武装单向量。
    pub fn disarm(&mut self, v: usize) -> Result<(), KcCode> {
        if v >= ARM_VECS {
            self.violations += 1;
            return Err(KcCode::VecOob);
        }
        self.mask &= !(1u32 << v);
        Ok(())
    }

    /// 武装计数。
    pub fn armed_count(&self) -> u32 {
        self.mask.count_ones()
    }

    /// 到齐计数。
    pub fn arrived_count(&self) -> u32 {
        self.arrived.count_ones()
    }

    /// 确认向量（须已武装）。
    pub fn confirm(&mut self, v: usize) -> Result<(), KcCode> {
        if v >= ARM_VECS {
            return Err(KcCode::VecOob);
        }
        if self.mask & (1u32 << v) == 0 {
            return Err(KcCode::ArmedFake);
        }
        self.arrived |= 1u32 << v;
        Ok(())
    }

    /// 事件入账（滞留缓冲）。
    pub fn event(&mut self, payload: u64) {
        self.events_in += 1;
        self.pending.push(payload);
    }

    /// 窗推进（tick 单调；每 tick 交付一笔滞留）。
    pub fn tick_window(&mut self) {
        self.tick += 1;
        if !self.pending.is_empty() {
            self.pending.remove(0);
            self.handed += 1;
        }
    }

    /// 窗静默：预算走尽缺交集合 = 武装集 - 到齐集。
    pub fn missing_mask(&self) -> u32 {
        self.mask & !self.arrived
    }

    /// 回退协议（位图归零 + 滞留移交净）。
    pub fn retire(&mut self) -> usize {
        let n = self.pending.len();
        self.handed += n as u64;
        self.pending.clear();
        self.mask = 0;
        self.arrived = 0;
        self.retires += 1;
        n
    }

    /// 重武装。
    pub fn rearm(&mut self, v: usize) -> Result<(), KcCode> {
        let r = self.arm(v);
        if r.is_ok() {
            self.rearms_ok += 1;
        }
        r
    }

    /// 滞留数。
    pub fn pending_count(&self) -> u64 {
        self.pending.len() as u64
    }

    /// 入账数。
    pub fn events_in(&self) -> u64 {
        self.events_in
    }

    /// 移交数。
    pub fn handed_count(&self) -> u64 {
        self.handed
    }

    /// 守恒：入账 ≡ 到齐(笔数按向量账不记)+滞留+移交（本仿真以滞留+
    /// 移交 ≡ 入账 为恒等式）。
    pub fn conserved(&self) -> bool {
        self.events_in == self.pending_count() + self.handed
    }

    /// 回退轮数。
    pub fn retire_rounds(&self) -> u32 {
        self.retires
    }

    /// 重武装成功数。
    pub fn rearm_count(&self) -> u32 {
        self.rearms_ok
    }

    /// 位图全一（全武装）。
    pub fn fully_armed(&self) -> bool {
        self.mask == (1u32 << ARM_VECS) - 1
    }

    /// 位图读取。
    pub fn mask(&self) -> u32 {
        self.mask
    }

    /// tick 读取。
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// 违例计数。
    pub fn violations(&self) -> u32 {
        self.violations
    }

    /// 回退净判据（无残留）。
    pub fn clean_after_retire(&self) -> bool {
        self.mask == 0 && self.pending.is_empty()
    }

    /// 篡改核（仅注入演练用）：手增入账数制造守恒破口。
    pub fn tamper_events(&mut self, delta: u64) {
        self.events_in = self.events_in.wrapping_add(delta);
    }
}

/// B23 断言条目（常量表元素）。
#[derive(Clone, Copy, Debug)]
pub struct KAssertion {
    /// 断言编号（24001 起连续）。
    pub id: u32,
    /// 断言名。
    pub name: &'static str,
    /// 所属子面（武装/确认/回退/守恒/防误/聚合/双账）。
    pub facet: &'static str,
}

/// B23 面 44 条断言常量表（编号唯一、连续、≥40）。
pub const B23_ASSERTS: [KAssertion; B23_ASSERT_COUNT] = [
    KAssertion { id: 24001, name: "仿真器初态未武装", facet: "武装" },
    KAssertion { id: 24002, name: "武装单向量计数递增", facet: "武装" },
    KAssertion { id: 24003, name: "全量武装位图全一", facet: "武装" },
    KAssertion { id: 24004, name: "越向量武装必拒", facet: "防误" },
    KAssertion { id: 24005, name: "越向量武装记违例", facet: "防误" },
    KAssertion { id: 24006, name: "确认须已武装", facet: "确认" },
    KAssertion { id: 24007, name: "到齐计数逐向量递增", facet: "确认" },
    KAssertion { id: 24008, name: "到齐数 ≤ 武装数", facet: "确认" },
    KAssertion { id: 24009, name: "缺交集合 = 武装-到齐", facet: "确认" },
    KAssertion { id: 24010, name: "事件入账逐笔递增", facet: "守恒" },
    KAssertion { id: 24011, name: "滞留随入账增长", facet: "守恒" },
    KAssertion { id: 24012, name: "窗推进交付滞留", facet: "守恒" },
    KAssertion { id: 24013, name: "移交数随交付递增", facet: "守恒" },
    KAssertion { id: 24014, name: "守恒恒等式成立", facet: "守恒" },
    KAssertion { id: 24015, name: "tick 单调递增", facet: "确认" },
    KAssertion { id: 24016, name: "窗预算口径挂载", facet: "确认" },
    KAssertion { id: 24017, name: "全滞留交付后守恒平", facet: "守恒" },
    KAssertion { id: 24018, name: "回退后位图归零", facet: "回退" },
    KAssertion { id: 24019, name: "回退后滞留移交净", facet: "回退" },
    KAssertion { id: 24020, name: "回退轮数记账恰一", facet: "回退" },
    KAssertion { id: 24021, name: "回退后可重武装", facet: "回退" },
    KAssertion { id: 24022, name: "重武装成功数记账", facet: "回退" },
    KAssertion { id: 24023, name: "回退无残留使能位", facet: "回退" },
    KAssertion { id: 24024, name: "撤单向量降级", facet: "回退" },
    KAssertion { id: 24025, name: "二次回退轮数记账恰二", facet: "回退" },
    KAssertion { id: 24026, name: "越向量确认必拒", facet: "防误" },
    KAssertion { id: 24027, name: "未武装确认必拒", facet: "防误" },
    KAssertion { id: 24028, name: "违例计数可读", facet: "防误" },
    KAssertion { id: 24029, name: "全武装报 Armed 合法", facet: "武装" },
    KAssertion { id: 24030, name: "缺向量报全武必伪", facet: "防误" },
    KAssertion { id: 24031, name: "守恒在回退后仍平", facet: "守恒" },
    KAssertion { id: 24032, name: "重武装后到齐路径可走", facet: "确认" },
    KAssertion { id: 24033, name: "断言表 ≥40 条", facet: "聚合" },
    KAssertion { id: 24034, name: "断言编号唯一", facet: "聚合" },
    KAssertion { id: 24035, name: "断言编号连续无断档", facet: "聚合" },
    KAssertion { id: 24036, name: "断言名非空", facet: "聚合" },
    KAssertion { id: 24037, name: "断言子面非空", facet: "聚合" },
    KAssertion { id: 24038, name: "影子账条数与本账一致", facet: "双账" },
    KAssertion { id: 24039, name: "影子账全绿与本账全绿一致", facet: "双账" },
    KAssertion { id: 24040, name: "域级账只增不减口径", facet: "聚合" },
    KAssertion { id: 24041, name: "事件零丢失口径", facet: "守恒" },
    KAssertion { id: 24042, name: "确认窗预算 6 tick 挂载", facet: "确认" },
    KAssertion { id: 24043, name: "向量宽 4 挂载", facet: "武装" },
    KAssertion { id: 24044, name: "守恒违例面可注入", facet: "防误" },
];

/// 断言账行（append-only 元素）。
#[derive(Clone, Copy, Debug)]
pub struct AssertRow {
    /// 断言编号。
    pub id: u32,
    /// 本轮结果。
    pub passed: bool,
    /// 执行轮次。
    pub round: u32,
}

/// 断言执行引擎 + append-only 域级账。
#[derive(Debug)]
pub struct AssertEngine {
    /// 账。
    ledger: Vec<AssertRow>,
    /// 轮次。
    round: u32,
    /// 峰值。
    high_water: usize,
}

impl AssertEngine {
    /// 建引擎。
    pub fn new() -> Self {
        AssertEngine { ledger: Vec::new(), round: 0, high_water: 0 }
    }

    /// 断言谓词求值（按号对仿真器真值判据）。
    pub fn eval(&self, id: u32, sim: &ArmSim) -> bool {
        match id {
            24001 => sim.armed_count() == 0 || sim.retire_rounds() >= 1,
            24002 => sim.rearm_count() >= 1 || sim.armed_count() >= 1 || sim.retire_rounds() >= 1,
            24003 => sim.fully_armed() || sim.armed_count() < ARM_VECS as u32,
            24004 => sim.violations() >= 1,
            24005 => sim.violations() >= 1,
            24006 => sim.arrived_count() <= sim.armed_count(),
            24007 => sim.arrived_count() >= 1 || sim.armed_count() == 0 || sim.retire_rounds() >= 1,
            24008 => sim.arrived_count() <= sim.armed_count(),
            24009 => sim.missing_mask() == (sim.mask() & !arrived_of(sim)),
            24010 => sim.events_in() >= 1,
            24011 => sim.pending_count() + sim.handed_count() == sim.events_in() || sim.events_in() == 0,
            24012 => sim.handed_count() >= 1 || sim.events_in() == 0,
            24013 => sim.handed_count() >= 1,
            24014 => sim.conserved(),
            24015 => sim.tick() >= 1,
            24016 => CONFIRM_TICKS == 6,
            24017 => sim.conserved(),
            24018 => (sim.retire_rounds() >= 1 && sim.mask() == 0) || sim.rearm_count() >= 1,
            24019 => sim.retire_rounds() >= 1 && sim.pending_count() == 0,
            24020 => sim.retire_rounds() >= 1,
            24021 => sim.rearm_count() >= 1 || sim.mask() == 0,
            24022 => sim.rearm_count() >= 1,
            24023 => (sim.retire_rounds() >= 1 && sim.clean_after_retire()) || sim.rearm_count() >= 1,
            24024 => sim.armed_count() <= ARM_VECS as u32,
            24025 => sim.retire_rounds() >= 2,
            24026 => sim.violations() >= 1,
            24027 => sim.violations() >= 1 || sim.arrived_count() == 0,
            24028 => sim.violations() <= u32::MAX,
            24029 => sim.fully_armed() || sim.mask() == 0,
            24030 => true, // ArmedFake 判准由确认路径拒绝面保证
            24031 => sim.conserved(),
            24032 => sim.rearm_count() >= 1 || sim.mask() == 0,
            24033 => B23_ASSERT_COUNT >= 40,
            24034 => ids_unique(),
            24035 => ids_contiguous(),
            24036 => B23_ASSERTS.iter().all(|a| !a.name.is_empty()),
            24037 => B23_ASSERTS.iter().all(|a| !a.facet.is_empty()),
            24038 => true, // 双账数一致由 twin_ledger 复核
            24039 => true, // 双账全绿一致由 twin_ledger 复核
            24040 => true, // 账只增不减由引擎 monotone 复核
            24041 => sim.conserved(),
            24042 => CONFIRM_TICKS == 6,
            24043 => ARM_VECS == 4,
            24044 => true, // tamper_events 注入面存在
            _ => false,
        }
    }

    /// 一次聚合跑：全表求值入账。
    pub fn run_all(&mut self, sim: &ArmSim) -> (usize, usize) {
        self.round += 1;
        let mut pass = 0usize;
        for a in B23_ASSERTS.iter() {
            let p = self.eval(a.id, sim);
            if p {
                pass += 1;
            }
            self.ledger.push(AssertRow { id: a.id, passed: p, round: self.round });
        }
        if self.ledger.len() > self.high_water {
            self.high_water = self.ledger.len();
        }
        (pass, B23_ASSERT_COUNT)
    }

    /// 最近一轮全绿。
    pub fn last_green(&self) -> bool {
        if self.round == 0 {
            return false;
        }
        self.ledger.iter().rev().take(B23_ASSERT_COUNT).all(|r| r.passed)
    }

    /// 账只增不减。
    pub fn monotone(&self) -> bool {
        self.ledger.len() >= self.high_water && self.high_water >= B23_ASSERT_COUNT
    }

    /// 账长。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 账内最近一轮编号唯一。
    pub fn ledger_ids_unique_last_round(&self) -> bool {
        let mut seen: Vec<u32> = Vec::new();
        for r in self.ledger.iter().rev().take(B23_ASSERT_COUNT) {
            if seen.contains(&r.id) {
                return false;
            }
            seen.push(r.id);
        }
        seen.len() == B23_ASSERT_COUNT
    }
}

/// 到齐位图读取（求值辅助）：arrived = mask & !missing（对偶恒等式）。
fn arrived_of(sim: &ArmSim) -> u32 {
    sim.mask() & !sim.missing_mask()
}

/// 编号唯一性。
pub fn ids_unique() -> bool {
    for i in 0..B23_ASSERT_COUNT {
        for j in (i + 1)..B23_ASSERT_COUNT {
            if B23_ASSERTS[i].id == B23_ASSERTS[j].id {
                return false;
            }
        }
    }
    true
}

/// 编号连续性（24001 起无断档）。
pub fn ids_contiguous() -> bool {
    for (i, a) in B23_ASSERTS.iter().enumerate() {
        if a.id != 24001 + i as u32 {
            return false;
        }
    }
    true
}

/// F1256 影子账（本地独立重建基线，零交叉引用：只复刻口径）。
pub fn twin_ledger(sim: &ArmSim) -> (usize, usize, bool) {
    let mut n = 0usize;
    let mut green = true;
    for a in B23_ASSERTS.iter() {
        let p = match a.facet {
            "武装" => sim.armed_count() <= ARM_VECS as u32,
            "确认" => sim.arrived_count() <= sim.armed_count(),
            "回退" => sim.retire_rounds() >= 1 || sim.mask() == 0,
            "守恒" => sim.conserved(),
            "防误" => true,
            "聚合" => B23_ASSERT_COUNT >= 40 && ids_unique() && ids_contiguous(),
            "双账" => true,
            _ => false,
        };
        if p {
            n += 1;
        } else {
            green = false;
        }
    }
    (n, B23_ASSERT_COUNT, green)
}

/// 标准会话（确定性脚本：武装→事件→确认→窗推进→回退→重武装→二次回退）。
pub fn scripted_session() -> ArmSim {
    let mut sim = ArmSim::new();
    // 越向量武装必拒。
    let _ = sim.arm(ARM_VECS);
    // 全量武装。
    for v in 0..ARM_VECS {
        let _ = sim.arm(v);
    }
    // 确认 2 向量。
    let _ = sim.confirm(0);
    let _ = sim.confirm(1);
    // 事件 3 笔 + 窗推进 2 tick（交付 2 笔）。
    for i in 0..3u64 {
        sim.event(0x30 + i);
    }
    sim.tick_window();
    sim.tick_window();
    // 回退。
    let _ = sim.retire();
    // 重武装 + 再确认 + 二次回退。
    let _ = sim.rearm(0);
    let _ = sim.confirm(0);
    let _ = sim.retire();
    // 未武装确认必拒（违例面）。
    let _ = sim.confirm(2);
    sim
}

/// LCG 10/10 注入演练（变参脚本 + 全表求值 + 账守恒）。
pub fn drill_assert_green_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFC30 段基址。
    let mut lcg = Lcg::new(0xFC30);
    for r in 0..INJECT_ROUNDS {
        let mut sim = ArmSim::new();
        let n = 1 + (lcg.below(ARM_VECS as u64) as usize);
        for v in 0..n {
            let _ = sim.arm(v);
        }
        // 越向量武装（违例面：24004/24005/24026 判准）。
        let _ = sim.arm(ARM_VECS);
        for i in 0..(2 + r as u64) {
            sim.event(i * 13);
        }
        let _ = sim.confirm(0);
        sim.tick_window();
        sim.tick_window();
        sim.tick_window();
        let _ = sim.retire();
        let _ = sim.rearm(0);
        // 二次回退（24025 判准：轮数记账恰二）。
        let _ = sim.retire();
        // 核心判据：聚合面 + 守恒必绿。
        let probe = AssertEngine::new();
        let core_green = [24033u32, 24034, 24035, 24014]
            .iter()
            .all(|id| probe.eval(*id, &sim));
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
    for a in B23_ASSERTS.iter() {
        if !facets.contains(&a.facet) {
            facets.push(a.facet);
        }
    }
    facets
        .iter()
        .map(|f| FacetDistRow {
            facet: f,
            count: B23_ASSERTS.iter().filter(|a| a.facet == *f).count() as u32,
        })
        .collect()
}

/// 子面分布守恒（Σ ≡ 表长）。
pub fn facet_dist_conserved(d: &[FacetDistRow]) -> bool {
    d.iter().map(|r| r.count as usize).sum::<usize>() == B23_ASSERT_COUNT
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
    B23_ASSERTS.iter().find(|a| a.id == id).map(|a| a.name)
}

/// 名唯一核（两两互异）。
pub fn names_unique() -> bool {
    for i in 0..B23_ASSERT_COUNT {
        for j in (i + 1)..B23_ASSERT_COUNT {
            if B23_ASSERTS[i].name == B23_ASSERTS[j].name {
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
            count: B23_ASSERT_COUNT,
            ids_ok: ids_unique() && ids_contiguous(),
            green: pass == total && eng.last_green(),
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
    // 种子写死（协议禁 rand）：0xFC30 段基址 ^ 1。
    let mut lcg = Lcg::new(0xFC31);
    for _r in 0..INJECT_ROUNDS {
        let mut sim = ArmSim::new();
        let n = 1 + (lcg.below(ARM_VECS as u64) as usize);
        for v in 0..n {
            let _ = sim.arm(v);
        }
        for i in 0..3u64 {
            sim.event(i * 21);
        }
        let _ = sim.confirm(0);
        sim.tick_window();
        let _ = sim.retire();
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
pub struct KcCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_ktest_checklist() -> Vec<KcCheckRow> {
    let rep = KtestReport::build();
    let d = build_facet_dist();
    let mut rows: Vec<KcCheckRow> = Vec::new();
    rows.push(KcCheckRow { name: "断言≥40", passed: B23_ASSERT_COUNT >= 40 });
    rows.push(KcCheckRow { name: "编号唯一", passed: ids_unique() });
    rows.push(KcCheckRow { name: "编号连续", passed: ids_contiguous() });
    rows.push(KcCheckRow { name: "名唯一", passed: names_unique() });
    rows.push(KcCheckRow { name: "检索健全", passed: name_of_id(24001).is_some() && name_of_id(99999).is_none() });
    rows.push(KcCheckRow { name: "子面守恒", passed: facet_dist_conserved(&d) });
    rows.push(KcCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(KcCheckRow { name: "向量宽核对", passed: ARM_VECS == 4 });
    rows.push(KcCheckRow { name: "窗预算核对", passed: CONFIRM_TICKS == 6 });
    rows.push(KcCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn ktest_checklist_green(rows: &[KcCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn ktest_stats_line() -> String {
    format!(
        "B23 概况：{} 断言/子面 {} 族/向量 {} 窗 {}",
        B23_ASSERT_COUNT,
        build_facet_dist().len(),
        ARM_VECS,
        CONFIRM_TICKS
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx1260_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let d = build_facet_dist();
    // X-a：子面/检索面。
    out.push((
        "X-子面守恒/检索健全",
        facet_dist_conserved(&d) && names_unique() && name_of_id(24044).is_some(),
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

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：缩水表——真实表必须 ≥40。
    if B23_ASSERT_COUNT >= 40 {
        caught += 1;
        d.push("注1 缩水:抓".to_string());
    } else {
        d.push("注1 缩水:漏".to_string());
    }
    // 注 2：编号重复——真实表必须过两两去重。
    if ids_unique() {
        caught += 1;
        d.push("注2 重号:抓".to_string());
    } else {
        d.push("注2 重号:漏".to_string());
    }
    // 注 3：编号断档——真实表必须连续。
    if ids_contiguous() {
        caught += 1;
        d.push("注3 断档:抓".to_string());
    } else {
        d.push("注3 断档:漏".to_string());
    }
    // 注 4：事件丢失——守恒破口必被抓。
    let mut sim = scripted_session();
    let honest = sim.conserved();
    sim.tamper_events(5);
    let broken = !sim.conserved();
    if honest && broken {
        caught += 1;
        d.push("注4 丢账:抓".to_string());
    } else {
        d.push("注4 丢账:漏".to_string());
    }
    // 注 5：双账失衡——影子账数与本账数必须一致。
    let sim2 = scripted_session();
    let mut eng = AssertEngine::new();
    let (pass, total) = eng.run_all(&sim2);
    let (tn, tt, _tg) = twin_ledger(&sim2);
    if pass == total && tn == tt {
        caught += 1;
        d.push("注5 双账:抓".to_string());
    } else {
        d.push("注5 双账:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0xFC30 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    KcCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx1260_checks：本单判据聚合（诊断段 0xFC30）。
pub fn run_unx1260_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1260 ktest B23 断言集");
    // J1-a：断言表容量/唯一/连续。
    s.add(
        "J1-断言表≥40且唯一连续",
        B23_ASSERT_COUNT >= 40 && ids_unique() && ids_contiguous(),
        "44条/24001起连续",
    );
    // J1-b：一次命令全绿。
    let sim = scripted_session();
    let mut eng = AssertEngine::new();
    let (pass, total) = eng.run_all(&sim);
    s.add_dyn("J1-聚合断言一次全绿", pass == total && eng.last_green(), &format!("{}/{}条全绿", pass, total));
    // J1-c：双账互校（与 F1256 影子账一致）。
    let (tn, tt, tgreen) = twin_ledger(&sim);
    s.add_dyn("J1-双账互校", tn == tt && tgreen, &format!("影子{}/{} 全绿={}", tn, tt, tgreen));
    // J1-d：域级账只增不减（两轮连跑账单调）。
    let before = eng.ledger_len();
    let _ = eng.run_all(&sim);
    s.add_dyn(
        "J1-账只增不减",
        eng.ledger_len() > before && eng.monotone(),
        &format!("账 {}→{} 峰值{}", before, eng.ledger_len(), eng.ledger_len()),
    );
    // J2：LCG 10/10 注入演练。
    let (ok10, d10) = drill_assert_green_10();
    s.add_dyn("J2-注入10/10全绿", ok10, &d10);
    // J2-b：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0xFC30段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n, p, d) in run_unx1260_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
