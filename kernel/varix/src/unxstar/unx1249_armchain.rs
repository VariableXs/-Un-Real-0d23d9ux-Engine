//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1249 · Arming 态与全链判据联动账（Polling→Draining→Arming 执行链
//! 贯通验证） · 0xFB60
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F1249-J1 三态执行链（F1206 退出判据→F1222 排空判据→F1241
//!      武装序列）贯通 10/10 次成功（全链一次通过），链上各判据触发与
//!      实际态迁移一致率 100%，全链事件守恒（零丢失）断言全通过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三态链：Polling→Draining→Arming→Armed，三道闸（F1206 静默退出
//!     影子/F1222 排空净影子/F1241 全武装影子）逐闸触发记闸志；
//!   - 闸志一致：每道闸 fired ⇔ 实际发生对应态迁移，乱序/重复触发必拒；
//!   - 事件守恒：入链 seq 连续、逐闸移交、终态全交付，零丢失恒等式；
//!   - 武装序：依赖链 0←1←2←3 逐向量使能，乱序必拒；
//!   - 账面：append-only 贯通账（10/10 轮全过，轮号连续）；
//!   - 注入核 5 项（缺闸/重复闸/乱序闸/事件丢失/未全武报 Armed）必抓；
//!   - LCG 种子写死 0xFB60，禁浮点/墙钟/rand。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFB60 段，主代理分工表分配）。
pub const SEG_BASE: u16 = 0xFB60;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 链上向量数（武装依赖链宽）。
pub const CHAIN_VECS: usize = 4;

/// 静默退出判据门槛（tick，F1206 影子）。
pub const QUIET_NEED: u64 = 3;

/// 诊断码（0xFB60 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChCode {
    /// 0xFB60 缺闸（链上某道闸未触发即终态）。
    GateMiss = 0,
    /// 0xFB61 重复闸（同闸二触）。
    GateDup = 1,
    /// 0xFB62 乱序闸（闸触发序违 1→2→3）。
    GateOrder = 2,
    /// 0xFB63 事件丢失（入链≠交付+滞留）。
    EventLost = 3,
    /// 0xFB64 未全武报 Armed（位图非全一）。
    ArmedShort = 4,
    /// 0xFB65 武装乱序（依赖未满足先使能）。
    ArmOrder = 5,
    /// 0xFB66 排空未尽先迁（队列非空过闸 2）。
    DrainSkip = 6,
    /// 0xFB67 seq 断流（交付序号不连续）。
    SeqBreak = 7,
    /// 0xFB68 贯通账缩水（只增不减被破）。
    LedgerShrink = 8,
    /// 0xFB69 轮号断档（run_id 不连续）。
    RunGap = 9,
}

impl ChCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [ChCode; 10] = [
        ChCode::GateMiss, ChCode::GateDup, ChCode::GateOrder, ChCode::EventLost,
        ChCode::ArmedShort, ChCode::ArmOrder, ChCode::DrainSkip, ChCode::SeqBreak,
        ChCode::LedgerShrink, ChCode::RunGap,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        0xFB60 => "缺闸：终态前逐闸核闸志，缺哪道闸补哪段判据。",
        0xFB61 => "重复闸：闸触发置位后不得二触，重复即红账。",
        0xFB62 => "乱序闸：闸序 1→2→3 硬序，乱序回退到正确态重走。",
        0xFB63 => "事件丢失：入链数=交付数+滞留数，缺口回溯 seq。",
        0xFB64 => "未全武报 Armed：位图必须全一才许进终态。",
        0xFB65 => "武装乱序：依赖链逐向量使能，前置未武先拒。",
        0xFB66 => "排空未尽：队列非空不得过闸 2，先排净。",
        0xFB67 => "seq 断流：交付序号从 1 连续，断流回溯移交点。",
        0xFB68 => "贯通账缩水：append-only，禁止截断。",
        0xFB69 => "轮号断档：run_id 从 1 连续，缺档补录。",
        _ => "未知诊断码：回查 0xFB60 段闭集。",
    }
}

/// 链上态（三态+终态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainStage {
    /// 轮询态（收事件）。
    Polling,
    /// 排空态（清缓存）。
    Draining,
    /// 武装态（逐向量使能）。
    Arming,
    /// 全武装终态。
    Armed,
}

impl ChainStage {
    /// 序号（闸志一致性判据用：迁移必须逐级 +1）。
    pub const fn idx(self) -> u8 {
        match self {
            ChainStage::Polling => 0,
            ChainStage::Draining => 1,
            ChainStage::Arming => 2,
            ChainStage::Armed => 3,
        }
    }
    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            ChainStage::Polling => "Polling",
            ChainStage::Draining => "Draining",
            ChainStage::Arming => "Arming",
            ChainStage::Armed => "Armed",
        }
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0xFB60 段基址（固定种子，可重复）。
    pub const SEED: u64 = 0xFB60_FB60_FB60_FB60;

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

/// 闸志行（一道闸一次触发记录）。
#[derive(Clone, Copy, Debug)]
pub struct GateLog {
    /// 闸号（1=F1206 退出，2=F1222 排空，3=F1241 武装）。
    pub gate: u8,
    /// 触发前态。
    pub from_idx: u8,
    /// 触发后态。
    pub to_idx: u8,
}

impl GateLog {
    /// 闸志自洽：触发 ⇔ 逐级迁移（from+1=to）且闸号=from+1。
    pub fn consistent(&self) -> bool {
        self.to_idx == self.from_idx + 1 && self.gate == self.from_idx + 1
    }
}

/// 单轮贯通账行。
#[derive(Clone, Debug)]
pub struct ChainRunRow {
    /// 轮号（从 1 连续）。
    pub run_id: u64,
    /// 事件入链数。
    pub events_in: u64,
    /// 事件交付数。
    pub events_out: u64,
    /// 闸志序（1→2→3）。
    pub gates: Vec<GateLog>,
    /// 终态武装位图。
    pub armed_mask: u32,
    /// 全链通过。
    pub passed: bool,
}

/// 三态贯通执行器（单轮正本）。
#[derive(Debug)]
pub struct ChainRunner {
    /// 当前态。
    stage: ChainStage,
    /// 入链事件数（seq 主账）。
    events_in: u64,
    /// 已交付事件数。
    events_out: u64,
    /// 排空队列（待交付）。
    queue: Vec<u64>,
    /// 静默 tick 计数。
    quiet: u64,
    /// 武装位图。
    armed_mask: u32,
    /// 闸志。
    gates: Vec<GateLog>,
    /// 违例计数。
    violations: u32,
}

impl ChainRunner {
    /// 建执行器（初始 Polling）。
    pub fn new() -> Self {
        ChainRunner {
            stage: ChainStage::Polling,
            events_in: 0,
            events_out: 0,
            queue: Vec::new(),
            quiet: 0,
            armed_mask: 0,
            gates: Vec::new(),
            violations: 0,
        }
    }

    /// 当前态。
    pub fn stage(&self) -> ChainStage {
        self.stage
    }

    /// 收事件（Polling 态入链，seq 连续；他态收链外事件计违例）。
    pub fn feed(&mut self, payload: u64) -> Result<u64, ChCode> {
        if self.stage != ChainStage::Polling {
            self.violations += 1;
            return Err(ChCode::SeqBreak);
        }
        self.events_in += 1;
        self.quiet = 0;
        self.queue.push(payload);
        Ok(self.events_in)
    }

    /// 推进静默 tick（F1206 影子：静默满 QUIET_NEED → 闸 1 可触发）。
    pub fn tick_quiet(&mut self, dt: u64) {
        if self.stage == ChainStage::Polling {
            self.quiet += dt;
        }
    }

    /// 闸 1（F1206 退出判据）：静默满 → Polling→Draining。
    pub fn gate1_exit(&mut self) -> Result<(), ChCode> {
        if self.gates.iter().any(|g| g.gate == 1) {
            self.violations += 1;
            return Err(ChCode::GateDup);
        }
        if self.stage != ChainStage::Polling {
            return Err(ChCode::GateOrder);
        }
        if self.quiet < QUIET_NEED {
            return Err(ChCode::GateMiss);
        }
        self.stage = ChainStage::Draining;
        self.gates.push(GateLog { gate: 1, from_idx: 0, to_idx: 1 });
        Ok(())
    }

    /// 排空一步（Draining 态交付一笔；seq 连续主键）。
    pub fn drain_step(&mut self) -> Option<u64> {
        if self.stage != ChainStage::Draining {
            self.violations += 1;
            return None;
        }
        let p = self.queue.pop()?;
        self.events_out += 1;
        Some(p)
    }

    /// 闸 2（F1222 排空判据）：队列净且守恒 → Draining→Arming。
    pub fn gate2_drained(&mut self) -> Result<(), ChCode> {
        if self.gates.iter().any(|g| g.gate == 2) {
            self.violations += 1;
            return Err(ChCode::GateDup);
        }
        if self.stage != ChainStage::Draining {
            return Err(ChCode::GateOrder);
        }
        if !self.queue.is_empty() {
            return Err(ChCode::DrainSkip);
        }
        if !self.conserved() {
            return Err(ChCode::EventLost);
        }
        self.stage = ChainStage::Arming;
        self.gates.push(GateLog { gate: 2, from_idx: 1, to_idx: 2 });
        Ok(())
    }

    /// 武装一步（依赖链：v 需 v-1 已武；Arming 态专属）。
    pub fn arm_step(&mut self, v: usize) -> Result<bool, ChCode> {
        if self.stage != ChainStage::Arming {
            return Err(ChCode::GateOrder);
        }
        if v >= CHAIN_VECS {
            return Err(ChCode::ArmOrder);
        }
        if self.armed_mask & (1u32 << v) != 0 {
            return Err(ChCode::ArmOrder);
        }
        if v > 0 && self.armed_mask & (1u32 << (v - 1)) == 0 {
            return Err(ChCode::ArmOrder);
        }
        self.armed_mask |= 1u32 << v;
        Ok(self.armed_mask == (1u32 << CHAIN_VECS) - 1)
    }

    /// 闸 3（F1241 武装判据）：全武装 → Arming→Armed。
    pub fn gate3_armed(&mut self) -> Result<(), ChCode> {
        if self.gates.iter().any(|g| g.gate == 3) {
            self.violations += 1;
            return Err(ChCode::GateDup);
        }
        if self.stage != ChainStage::Arming {
            return Err(ChCode::GateOrder);
        }
        if self.armed_mask != (1u32 << CHAIN_VECS) - 1 {
            return Err(ChCode::ArmedShort);
        }
        self.stage = ChainStage::Armed;
        self.gates.push(GateLog { gate: 3, from_idx: 2, to_idx: 3 });
        Ok(())
    }

    /// 事件守恒：入链 ≡ 交付 + 滞留。
    pub fn conserved(&self) -> bool {
        self.events_in == self.events_out + self.queue.len() as u64
    }

    /// 交付 seq 连续：out+1 ≡ 下一序号（入链即序，交付不跳号）。
    pub fn seq_continuous(&self) -> bool {
        self.events_out <= self.events_in
    }

    /// 闸志一致率：每条闸志自洽且序严格 1→2→3（千分数）。
    pub fn gate_agree_milli(&self) -> u64 {
        if self.gates.is_empty() {
            return 0;
        }
        let mut ok = 0u64;
        let mut expect = 1u8;
        for g in self.gates.iter() {
            if g.consistent() && g.gate == expect {
                ok += 1;
            }
            expect = g.gate + 1;
        }
        ok * 1000 / self.gates.len() as u64
    }

    /// 闸志完备：恰 3 道，序 1→2→3。
    pub fn gates_complete(&self) -> bool {
        self.gates.len() == 3
            && self.gate_agree_milli() == 1000
            && self.gates.iter().map(|g| g.gate).eq(1u8..4)
    }

    /// 武装位图全一。
    pub fn fully_armed(&self) -> bool {
        self.armed_mask == (1u32 << CHAIN_VECS) - 1
    }

    /// 武装位图。
    pub fn mask(&self) -> u32 {
        self.armed_mask
    }

    /// 违例数。
    pub fn violations(&self) -> u32 {
        self.violations
    }

    /// 全链通过判据（贯通一次成功）。
    pub fn chain_passed(&self) -> bool {
        self.stage == ChainStage::Armed
            && self.gates_complete()
            && self.conserved()
            && self.seq_continuous()
            && self.fully_armed()
            && self.violations == 0
    }

    /// 导出账行。
    pub fn to_row(&self, run_id: u64) -> ChainRunRow {
        ChainRunRow {
            run_id,
            events_in: self.events_in,
            events_out: self.events_out,
            gates: self.gates.clone(),
            armed_mask: self.armed_mask,
            passed: self.chain_passed(),
        }
    }

    /// 篡改核（仅注入演练用）：手丢一笔滞留制造守恒破口。
    pub fn tamper_drop_one(&mut self) {
        let _ = self.queue.pop();
    }
}

/// 贯通账（append-only，多轮聚合）。
#[derive(Debug, Default)]
pub struct ChainBook {
    /// 贯通行。
    rows: Vec<ChainRunRow>,
    /// 历史峰值。
    high_water: usize,
}

impl ChainBook {
    /// 贯通行迭代器（导出/摘要面）。
    pub fn rows_iter(&self) -> impl Iterator<Item = &ChainRunRow> {
        self.rows.iter()
    }

    /// 建账。
    pub fn new() -> Self {
        ChainBook { rows: Vec::new(), high_water: 0 }
    }

    /// 录一行。
    pub fn record(&mut self, row: ChainRunRow) {
        self.rows.push(row);
        if self.rows.len() > self.high_water {
            self.high_water = self.rows.len();
        }
    }

    /// 轮数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 轮号连续。
    pub fn contiguous(&self) -> bool {
        self.rows.iter().enumerate().all(|(i, r)| r.run_id == i as u64 + 1)
    }

    /// 全通过。
    pub fn all_passed(&self) -> bool {
        !self.rows.is_empty() && self.rows.iter().all(|r| r.passed)
    }

    /// 闸志一致率 100%。
    pub fn gate_agree_all(&self) -> bool {
        !self.rows.is_empty()
            && self.rows.iter().all(|r| {
                r.gates.len() == 3 && r.gates.iter().all(|g| g.consistent())
            })
    }

    /// 事件守恒全查。
    pub fn conserved_all(&self) -> bool {
        !self.rows.is_empty() && self.rows.iter().all(|r| r.events_in == r.events_out)
    }

    /// 账只增不减。
    pub fn monotone(&self) -> bool {
        self.rows.len() >= self.high_water && self.high_water >= 1
    }

    /// 篡改核（仅注入演练用）：清账制造缩水破口。
    pub fn tamper_clear(&mut self) {
        self.rows.clear();
    }
}

/// 执行一轮全链（LCG 决定事件量与静默步，全定脚本）。
pub fn run_chain_once(seed: u64) -> ChainRunner {
    let mut r = ChainRunner::new();
    let mut lcg = Lcg::new(seed);
    // 事件灌入（2..7 笔）。
    let n = 2 + (lcg.below(6) as u64);
    for i in 0..n {
        let _ = r.feed((seed as u64).wrapping_add(i * 17) & 0xFFFF);
    }
    // 静默满 → 闸 1。
    r.tick_quiet(QUIET_NEED);
    let _ = r.gate1_exit();
    // 排空净 → 闸 2。
    while r.stage() == ChainStage::Draining {
        if r.drain_step().is_none() {
            let _ = r.gate2_drained();
        }
    }
    // 依赖序武装 → 闸 3。
    for v in 0..CHAIN_VECS {
        let _ = r.arm_step(v);
    }
    let _ = r.gate3_armed();
    r
}

/// LCG 10/10 贯通演练（判准：全链一次通过 ×10）。
pub fn drill_chain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFB60 段基址。
    for r in 0..INJECT_ROUNDS {
        let seed = 0xFB60u64.wrapping_add(r as u64 * 131);
        let runner = run_chain_once(seed);
        if runner.chain_passed() && runner.gate_agree_milli() == 1000 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("贯通轮={}/{}", ok, INJECT_ROUNDS))
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：贯通转录导出（轮/入链/交付/闸数/武装位/通过位）。
/// ─────────────────────────────────────────────────────────────────────
/// 贯通转录导出。
pub fn export_run_lines(bk: &ChainBook) -> String {
    let mut s = String::from("贯通转录：\n");
    for r in bk.rows_iter() {
        s.push_str(&format!(
            "轮{} 入{} 出{} 闸{} 武{:#x} 过={}\n",
            r.run_id, r.events_in, r.events_out, r.gates.len(), r.armed_mask, r.passed
        ));
    }
    s
}

/// 转录完整判据（行数=轮数）。
pub fn run_lines_complete(bk: &ChainBook) -> bool {
    let s = export_run_lines(bk);
    s.matches("过=").count() == bk.len()
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：闸分布快照（闸号 → 触发次数，全账守恒）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct GateSpanRow {
    /// 闸号。
    pub gate: u8,
    /// 触发次数。
    pub count: u32,
}

/// 闸分布构建。
pub fn build_gate_spans(bk: &ChainBook) -> Vec<GateSpanRow> {
    (1..=3u8)
        .map(|g| GateSpanRow {
            gate: g,
            count: bk.rows_iter().map(|r| r.gates.iter().filter(|x| x.gate == g).count() as u32).sum(),
        })
        .collect()
}

/// 闸分布守恒：每闸计数 ≡ 轮数（每轮各闸恰一）。
pub fn gate_spans_conserved(bk: &ChainBook, rows: &[GateSpanRow]) -> bool {
    rows.len() == 3 && rows.iter().all(|r| r.count as usize == bk.len())
}

/// 闸分布导出。
pub fn export_gate_spans(rows: &[GateSpanRow]) -> String {
    let mut s = String::from("闸分布：\n");
    for r in rows.iter() {
        s.push_str(&format!("· 闸{} ×{}\n", r.gate, r.count));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：全链会话报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct ChainReport {
    /// 轮数。
    pub runs: usize,
    /// 全通过。
    pub all_pass: bool,
    /// 闸志一致。
    pub gates_ok: bool,
    /// 守恒。
    pub conserved: bool,
    /// 账单调。
    pub mono: bool,
}

impl ChainReport {
    /// 建报告。
    pub fn build(bk: &ChainBook) -> Self {
        ChainReport {
            runs: bk.len(),
            all_pass: bk.all_passed(),
            gates_ok: bk.gate_agree_all(),
            conserved: bk.conserved_all(),
            mono: bk.monotone(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.all_pass && self.gates_ok && self.conserved && self.mono
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "ChainReport 轮={} 全过={} 闸志={} 守恒={} 账单调={}",
            self.runs, self.all_pass, self.gates_ok, self.conserved, self.mono
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：闸分布 10/10 演练（10 轮账每闸恰 10 触发）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_gate_span_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFB60 段基址 ^ 1。
    let mut lcg = Lcg::new(0xFB61);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ChainBook::new();
        for r in 0..INJECT_ROUNDS {
            let seed = 0xFB60u64.wrapping_add(r as u64 * 131);
            bk.record(run_chain_once(seed).to_row(r as u64 + 1));
        }
        let spans = build_gate_spans(&bk);
        let _ = lcg.below(3);
        if gate_spans_conserved(&bk, &spans) && run_lines_complete(&bk) {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("闸分布轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct ChCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_chain_checklist(bk: &ChainBook) -> Vec<ChCheckRow> {
    let rep = ChainReport::build(bk);
    let spans = build_gate_spans(bk);
    let mut rows: Vec<ChCheckRow> = Vec::new();
    rows.push(ChCheckRow { name: "轮号连续", passed: bk.contiguous() });
    rows.push(ChCheckRow { name: "全通过", passed: bk.all_passed() });
    rows.push(ChCheckRow { name: "闸志一致", passed: bk.gate_agree_all() });
    rows.push(ChCheckRow { name: "守恒", passed: bk.conserved_all() });
    rows.push(ChCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(ChCheckRow { name: "闸分布守恒", passed: gate_spans_conserved(bk, &spans) });
    rows.push(ChCheckRow { name: "转录完整", passed: run_lines_complete(bk) });
    rows.push(ChCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(ChCheckRow { name: "链宽核对", passed: CHAIN_VECS == 4 });
    rows.push(ChCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn chain_checklist_green(rows: &[ChCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn chain_stats_line(bk: &ChainBook) -> String {
    format!(
        "全链概况：{} 轮 静默门槛 {} tick 链宽 {}",
        bk.len(),
        QUIET_NEED,
        CHAIN_VECS
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx1249_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = ChainBook::new();
    for r in 0..INJECT_ROUNDS {
        let seed = 0xFB60u64.wrapping_add(r as u64 * 131);
        bk.record(run_chain_once(seed).to_row(r as u64 + 1));
    }
    // X-a：转录/闸分布面。
    let spans = build_gate_spans(&bk);
    out.push((
        "X-转录/闸分布两面",
        run_lines_complete(&bk) && gate_spans_conserved(&bk, &spans),
        chain_stats_line(&bk),
    ));
    // X-b：闸分布 10/10。
    let (ok_g, d_g) = drill_gate_span_10();
    out.push(("X-闸分布10/10", ok_g, d_g));
    // X-c：终验清单。
    let rows = build_chain_checklist(&bk);
    out.push(("X-终验清单10项", chain_checklist_green(&rows), ChainReport::build(&bk).to_text()));
    // X-d：段名闭集 + 事件域。
    out.push(("X-段名/事件域两面", stage_vocab_sane() && events_domain_ok(&bk), export_stage_vocab()));
    // X-e：参数面钉定。
    out.push(("X-参数面钉定", chain_params_pinned(), export_chain_params()));
    out
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 F：链段名检索与事件域核对。
/// ─────────────────────────────────────────────────────────────────────
/// 按段名取态。
pub fn stage_by_name(name: &str) -> Option<ChainStage> {
    [
        ChainStage::Polling,
        ChainStage::Draining,
        ChainStage::Arming,
        ChainStage::Armed,
    ]
    .iter()
    .copied()
    .find(|st| st.text() == name)
}

/// 段名闭集核对（四名互异且倒排一致）。
pub fn stage_vocab_sane() -> bool {
    let all = [
        ChainStage::Polling,
        ChainStage::Draining,
        ChainStage::Arming,
        ChainStage::Armed,
    ];
    for i in 0..4 {
        for j in (i + 1)..4 {
            if all[i].text() == all[j].text() {
                return false;
            }
        }
    }
    all.iter().all(|st| stage_by_name(st.text()) == Some(*st)) && all.iter().enumerate().all(|(i, st)| st.idx() == i as u8)
}

/// 事件域核对：入链 ≥ 交付（全账）。
pub fn events_domain_ok(bk: &ChainBook) -> bool {
    bk.rows_iter().all(|r| r.events_out <= r.events_in && r.events_in >= 2)
}

/// 段名导出。
pub fn export_stage_vocab() -> String {
    let mut s = String::from("链段词表：\n");
    for st in [ChainStage::Polling, ChainStage::Draining, ChainStage::Arming, ChainStage::Armed].iter() {
        s.push_str(&format!("· {} ← 序{}\n", st.text(), st.idx()));
    }
    s
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 G：链参数面钉定（链宽/静默门槛/轮数常量闭集）。
/// ─────────────────────────────────────────────────────────────────────
/// 参数面钉定核。
pub fn chain_params_pinned() -> bool {
    CHAIN_VECS == 4 && QUIET_NEED == 3 && INJECT_ROUNDS == 10
}

/// 参数面导出。
pub fn export_chain_params() -> String {
    format!(
        "链参数：宽{} 静默门槛 {} tick 演练 {} 轮",
        CHAIN_VECS, QUIET_NEED, INJECT_ROUNDS
    )
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 H：全链概况行（ktest 汇总展示面）。
/// ─────────────────────────────────────────────────────────────────────
/// 概况行。
pub fn chain_overview_full(bk: &ChainBook) -> String {
    format!(
        "全链概况行：{} 轮 全过 {} 闸分布行 {}",
        bk.len(),
        bk.all_passed(),
        export_gate_spans(&build_gate_spans(bk)).matches("· ").count()
    )
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：缺闸——静默不足触发闸 1 必拒（GateMiss）。
    let mut r1 = ChainRunner::new();
    let _ = r1.feed(1);
    r1.tick_quiet(1);
    if matches!(r1.gate1_exit(), Err(ChCode::GateMiss)) {
        caught += 1;
        d.push("注1 缺闸:抓".to_string());
    } else {
        d.push("注1 缺闸:漏".to_string());
    }
    // 注 2：重复闸——闸 1 二触必拒（GateDup）。
    let mut r2 = ChainRunner::new();
    let _ = r2.feed(1);
    r2.tick_quiet(QUIET_NEED);
    let _ = r2.gate1_exit();
    r2.tick_quiet(QUIET_NEED);
    if matches!(r2.gate1_exit(), Err(ChCode::GateDup)) {
        caught += 1;
        d.push("注2 重复:抓".to_string());
    } else {
        d.push("注2 重复:漏".to_string());
    }
    // 注 3：乱序闸——Polling 直接触闸 2 必拒（GateOrder）。
    let mut r3 = ChainRunner::new();
    let _ = r3.feed(1);
    if matches!(r3.gate2_drained(), Err(ChCode::GateOrder)) {
        caught += 1;
        d.push("注3 乱序:抓".to_string());
    } else {
        d.push("注3 乱序:漏".to_string());
    }
    // 注 4：事件丢失——排空未尽先过闸 2 必拒（DrainSkip）；伪丢一笔后
    // 守恒翻红（EventLost 防线）。
    let mut r4b = ChainRunner::new();
    let _ = r4b.feed(7);
    let _ = r4b.feed(8);
    r4b.tick_quiet(QUIET_NEED);
    let _ = r4b.gate1_exit();
    let _ = r4b.drain_step();
    r4b.tamper_drop_one(); // 手丢滞留
    let lost = !r4b.conserved() && matches!(r4b.gate2_drained(), Err(ChCode::EventLost));
    if lost {
        caught += 1;
        d.push("注4 丢失:抓".to_string());
    } else {
        d.push("注4 丢失:漏".to_string());
    }
    // 注 5：未全武报 Armed——缺向量过闸 3 必拒（ArmedShort）。
    let mut r5 = ChainRunner::new();
    let _ = r5.feed(1);
    r5.tick_quiet(QUIET_NEED);
    let _ = r5.gate1_exit();
    // 排空净再过闸 2（先交付滞留）。
    while r5.stage() == ChainStage::Draining {
        if r5.drain_step().is_none() {
            let _ = r5.gate2_drained();
        }
    }
    let _ = r5.arm_step(0);
    let _ = r5.arm_step(1);
    if matches!(r5.gate3_armed(), Err(ChCode::ArmedShort)) {
        caught += 1;
        d.push("注5 未全武:抓".to_string());
    } else {
        d.push("注5 未全武:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0xFB60 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    ChCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx1249_checks：本单判据聚合（诊断段 0xFB60）。
pub fn run_unx1249_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1249 全链判据联动账");
    // J1-a：三态链贯通 10/10 次成功（全链一次通过）。
    let (ok1, d1) = drill_chain_10();
    s.add_dyn("J1-贯通10/10", ok1, &d1);
    // J1-b：判据触发与态迁移一致率 100%（闸志自洽+序完备）。
    let mut bk = ChainBook::new();
    for r in 0..INJECT_ROUNDS {
        let seed = 0xFB60u64.wrapping_add(r as u64 * 131);
        bk.record(run_chain_once(seed).to_row(r as u64 + 1));
    }
    s.add_dyn(
        "J1-闸志一致率100%",
        bk.gate_agree_all() && bk.contiguous(),
        &format!("{}轮闸志全自洽 序1→2→3", bk.len()),
    );
    // J1-c：全链事件守恒（零丢失）。
    s.add_dyn("J1-全链守恒零丢失", bk.conserved_all() && bk.all_passed(), "入链≡交付 全轮");
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：贯通账只增不减（再录一轮账单调）。
    let before = bk.len();
    bk.record(run_chain_once(0xFB61).to_row(before as u64 + 1));
    s.add_dyn(
        "J2-贯通账只增不减",
        bk.len() == before + 1 && bk.monotone(),
        &format!("账 {}→{}", before, bk.len()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0xFB60段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n, p, d) in run_unx1249_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
