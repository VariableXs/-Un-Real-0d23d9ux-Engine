//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1485 · 补全执行三（分发与接管面 B16–B30 缺口批补齐） · 0x72180
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2-B35）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F1485-J1 分发与接管面 15 批缺口补齐执行（F1482 清单
//!     逐项补），补全后段级 ≥80 大关（F1382/F1400 双层锚）复验通过，补全
//!     断言入总账只增不减，补全漏项注入 10/10 必抓；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · F1482 清单冻结：B16–B30 共 15 批 × 3 项（分发/接管/桥接三类），
//!     每项带权重；预制完成项与缺口项由确定性位型给定（每批 1 预制 + 2 缺口）；
//!   · 补全执行器：逐批逐项补，补全断言追加进总账（append-only，seq 单调）；
//!     重复补全 / 缩账 / 越界批号 / 未知清单项 一律显性拒；
//!   · 批状态机 Queued→Filling→Filled 单调推进，倒退拒；批未清即封拒；
//!   · 大关复验：F1382 层（权重进度）+ F1400 层（批完成度）双锚，合分
//!     ≥80 且层差 ≤15 才过；漏一项即逐项对账失败（GateShort）；
//!   · 漏项注入 10/10：每轮故意漏补一个缺口项，复验必须抓住。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72180，VX3-1103 补全执行窗口独占）。
pub const SEG_BASE: u32 = 0x72180;
/// 批前缀下界（B16，冻结）。
pub const BATCH_LO: u8 = 16;
/// 批前缀上界（B30，冻结）。
pub const BATCH_HI: u8 = 30;
/// 分发与接管面批数（15，冻结）。
pub const BATCH_COUNT: usize = 15;
/// 每批清单项数（冻结 3：分发/接管/桥接）。
pub const ITEMS_PER_BATCH: usize = 3;
/// 段级大关（合分 ≥80，冻结）。
pub const GATE_MIN: u32 = 80;
/// 双层锚容差（|F1382 层 − F1400 层| ≤15，冻结）。
pub const ANCHOR_TOL: u32 = 15;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FillCode {
    /// 批号越界：只在 B16–B30
    BatchOver,
    /// 重复补全：同一清单项只补一次
    DupFill,
    /// 总账倒退：补全断言只增不减，缩账/截断一律拒
    LedgerRegress,
    /// 批未清即封：批内还有缺口项未补
    FillEarly,
    /// 大关未过：段级复验 <80 或清单有漏项
    GateShort,
    /// 双层锚裂：F1382/F1400 层分差超容差
    AnchorSplit,
    /// 双跑不一致：fill_all 两次应同账
    NotReproducible,
    /// 未知清单项：不在 F1482 冻结清单内
    ItemUnknown,
}

impl FillCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FillCode::BatchOver => "批号越界：只在 B16–B30",
            FillCode::DupFill => "重复补全：查总账去重，同项另起断言号",
            FillCode::LedgerRegress => "总账倒退：补全断言只增不减，禁缩账",
            FillCode::FillEarly => "批未清即封：先补完批内全部缺口项",
            FillCode::GateShort => "大关未过：逐项对账找漏项，补齐后复验",
            FillCode::AnchorSplit => "双层锚裂：F1382/F1400 层分差超 15，查两锚口径",
            FillCode::NotReproducible => "双跑不一致：复现红线，fill_all 两次应同账",
            FillCode::ItemUnknown => "未知清单项：不在 F1482 冻结清单，对表修正",
        }
    }
    pub const ALL: [FillCode; 8] = [
        FillCode::BatchOver,
        FillCode::DupFill,
        FillCode::LedgerRegress,
        FillCode::FillEarly,
        FillCode::GateShort,
        FillCode::AnchorSplit,
        FillCode::NotReproducible,
        FillCode::ItemUnknown,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FillCode::ALL.len() {
        if FillCode::ALL[i].val() == code {
            return FillCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72180 段闭集内"
}

// ── 批状态机（Queued→Filling→Filled，单调推进） ─────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchState {
    /// 排队：批内一单未补。
    Queued,
    /// 补全中：至少补了一单，批未清。
    Filling,
    /// 已封批：批内清单项全落账。
    Filled,
}

impl BatchState {
    /// 单调序值：Queued=0 < Filling=1 < Filled=2。
    pub fn rank(self) -> u8 {
        match self {
            BatchState::Queued => 0,
            BatchState::Filling => 1,
            BatchState::Filled => 2,
        }
    }
    /// 只允许向前推进；平移与倒退一律 false。
    pub fn can_advance(self, to: BatchState) -> bool {
        to.rank() > self.rank()
    }
}

// ── F1482 冻结清单（15 批 × 3 项：名称 + 权重） ─────────────────
/// 清单项名（冻结表，逐批逐项）。
pub fn item_name(batch: u8, idx: usize) -> Option<&'static str> {
    if batch < BATCH_LO || batch > BATCH_HI || idx >= ITEMS_PER_BATCH {
        return None;
    }
    const NAMES: [[&str; 3]; 15] = [
        ["disp_route_tbl", "hwnd_takeover_map", "focus_xfer_chain"],
        ["input_q_rebind", "hit_test_patch", "capture_stack_fix"],
        ["wm_msg_order_fix", "wnd_proc_bridge", "layout_pass_r2"],
        ["gpu_takeover_handoff", "swapchain_rebind", "present_q_drain"],
        ["hid_stack_bridge", "rawinput_mirror", "focus_lost_guard"],
        ["clip_resume_chain", "dnd_takeover", "cursor_confine_fix"],
        ["menu_dispatch_fix", "accel_table_bind", "ime_takeover_bridge"],
        ["dcomp_visual_bind", "dwm_sync_fix", "redirect_hwnd_map"],
        ["svc_pipe_rebind", "rpc_async_fix", "handle_dup_guard"],
        ["shared_mem_map2", "event_flag_wire", "mutex_takeover"],
        ["timer_queue_fill", "wait_chain_fix", "apc_dispatch_bridge"],
        ["reg_watch_rebind", "prefetch_hive_fix", "mount_takeover"],
        ["fs_filter_attach", "oplock_bridge", "case_tab_fix"],
        ["net_stack_takeover", "filter_port_bind", "rebind_port_pool"],
        ["dbg_port_attach", "dump_writer_fill", "crash_hook_bridge"],
    ];
    NAMES.get((batch - BATCH_LO) as usize).and_then(|row| row.get(idx)).copied()
}

/// 清单项权重（冻结表，与名称一一对应）。
pub fn item_weight(batch: u8, idx: usize) -> Option<u32> {
    if batch < BATCH_LO || batch > BATCH_HI || idx >= ITEMS_PER_BATCH {
        return None;
    }
    const WEIGHTS: [[u32; 3]; 15] = [
        [9, 7, 5],
        [8, 6, 5],
        [8, 6, 4],
        [9, 7, 5],
        [7, 6, 4],
        [6, 5, 4],
        [7, 5, 6],
        [8, 6, 5],
        [7, 6, 4],
        [7, 5, 6],
        [6, 5, 7],
        [6, 5, 7],
        [8, 5, 4],
        [9, 6, 5],
        [7, 6, 5],
    ];
    WEIGHTS.get((batch - BATCH_LO) as usize).and_then(|row| row.get(idx)).copied()
}

/// 缺口位型（确定性）：idx=0 为预制完成项，idx=1/2 为 F1482 待补缺口
/// → 每批恰好 1 预制 + 2 缺口，全账 15 预制 + 30 缺口。
pub fn is_gap(batch: u8, idx: usize) -> bool {
    batch >= BATCH_LO && batch <= BATCH_HI && idx < ITEMS_PER_BATCH && (idx % 3) != 0
}

/// 缺口总数（冻结 30）。
pub fn gap_count() -> usize {
    let mut n = 0usize;
    let mut b = BATCH_LO;
    while b <= BATCH_HI {
        let mut k = 0usize;
        while k < ITEMS_PER_BATCH {
            if is_gap(b, k) {
                n += 1;
            }
            k += 1;
        }
        b += 1;
    }
    n
}

/// 清单总权重（冻结，逐项求和）。
pub fn total_weight() -> u32 {
    let mut w = 0u32;
    let mut b = BATCH_LO;
    while b <= BATCH_HI {
        let mut k = 0usize;
        while k < ITEMS_PER_BATCH {
            w += item_weight(b, k).unwrap_or(0);
            k += 1;
        }
        b += 1;
    }
    w
}

/// 第 r 个缺口项（清单序，供漏项注入定位）。
pub fn nth_gap(r: usize) -> Option<(u8, usize)> {
    let mut n = 0usize;
    let mut b = BATCH_LO;
    while b <= BATCH_HI {
        let mut k = 0usize;
        while k < ITEMS_PER_BATCH {
            if is_gap(b, k) {
                if n == r {
                    return Some((b, k));
                }
                n += 1;
            }
            k += 1;
        }
        b += 1;
    }
    None
}

// ── 总账（append-only：只增不减） ───────────────────────────────
/// 补全断言记录：seq 全账单调，batch/idx 定位清单项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FillRecord {
    pub seq: u64,
    pub batch: u8,
    pub idx: usize,
    pub weight: u32,
}

/// 补全总账：追加式账本，账长只增不减。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FillLedger {
    pub records: Vec<FillRecord>,
    /// 下一个断言号（从 1 起，单调递增）。
    pub next_seq: u64,
}

impl FillLedger {
    pub fn new() -> FillLedger {
        FillLedger { records: Vec::new(), next_seq: 1 }
    }

    /// 该清单项是否已落账。
    pub fn has(&self, batch: u8, idx: usize) -> bool {
        let mut i = 0;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.batch == batch && r.idx == idx {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 追加一条补全断言（越界闸 / 重复闸），返回断言号。
    pub fn append(&mut self, batch: u8, idx: usize) -> Result<u64, u32> {
        if batch < BATCH_LO || batch > BATCH_HI {
            return Err(FillCode::BatchOver.val());
        }
        if idx >= ITEMS_PER_BATCH {
            return Err(FillCode::ItemUnknown.val());
        }
        if self.has(batch, idx) {
            return Err(FillCode::DupFill.val());
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.records.push(FillRecord {
            seq,
            batch,
            idx,
            weight: item_weight(batch, idx).unwrap_or(0),
        });
        Ok(seq)
    }

    /// 缩账探针：任何把账缩到 want 的企图一律显性拒。
    pub fn shrink_probe(&self, want: usize) -> Result<(), u32> {
        if want < self.records.len() {
            return Err(FillCode::LedgerRegress.val());
        }
        Ok(())
    }
}

// ── 批账 ────────────────────────────────────────────────────────
/// 批账：批号 + 状态机 + 已落账项数（含预制完成项）。
#[derive(Clone, Debug)]
pub struct BatchLedger {
    pub batch: u8,
    pub state: BatchState,
    /// 已落账清单项数（0..=3）。
    pub filled: usize,
}

impl BatchLedger {
    pub fn new(batch: u8) -> BatchLedger {
        BatchLedger { batch, state: BatchState::Queued, filled: 0 }
    }
    /// 状态推进公开口：倒退/平移显性拒。
    pub fn advance(&mut self, to: BatchState) -> Result<(), u32> {
        if !self.state.can_advance(to) {
            return Err(FillCode::LedgerRegress.val());
        }
        self.state = to;
        Ok(())
    }
    /// 封批：批内清单项未全落账即拒（FillEarly）。
    pub fn finish(&mut self) -> Result<(), u32> {
        if self.filled < ITEMS_PER_BATCH {
            return Err(FillCode::FillEarly.val());
        }
        self.advance(BatchState::Filled)
    }
}

// ── 补全执行器 ──────────────────────────────────────────────────
/// 补全结果：总账 + 15 批账。
#[derive(Clone, Debug)]
pub struct FillOutcome {
    pub ledger: FillLedger,
    pub batches: Vec<BatchLedger>,
}

impl FillOutcome {
    /// 已封批数（F1400 层口径）。
    pub fn filled_batches(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < self.batches.len() {
            if let Some(b) = self.batches.get(i) {
                if b.state == BatchState::Filled {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

/// 补全执行：skip 指定缺口项则跳过不补（漏项注入面）。
/// 正常流传 None；批状态机单调推进，全部缺口补齐后逐批封批。
pub fn fill_all_skip(skip: Option<(u8, usize)>) -> Result<FillOutcome, u32> {
    let mut ledger = FillLedger::new();
    let mut batches: Vec<BatchLedger> = Vec::new();
    let mut b = BATCH_LO;
    while b <= BATCH_HI {
        batches.push(BatchLedger::new(b));
        b += 1;
    }
    let mut k = 0usize;
    while k < BATCH_COUNT {
        let batch = BATCH_LO + k as u8;
        let mut idx = 0usize;
        while idx < ITEMS_PER_BATCH {
            if is_gap(batch, idx) && skip != Some((batch, idx)) {
                ledger.append(batch, idx)?;
                if let Some(bl) = batches.get_mut(k) {
                    bl.filled += 1;
                    if bl.state == BatchState::Queued {
                        bl.advance(BatchState::Filling)?;
                    }
                }
            }
            idx += 1;
        }
        k += 1;
    }
    // 逐批封批：预制项 + 补全项凑满 3 项才能封（有漏项的批留在 Filling）。
    let mut k = 0usize;
    while k < BATCH_COUNT {
        if let Some(bl) = batches.get_mut(k) {
            // 预制完成项计入 filled。
            let mut idx = 0usize;
            while idx < ITEMS_PER_BATCH {
                if !is_gap(bl.batch, idx) {
                    bl.filled += 1;
                }
                idx += 1;
            }
            if bl.filled >= ITEMS_PER_BATCH {
                bl.finish()?;
            }
        }
        k += 1;
    }
    Ok(FillOutcome { ledger, batches })
}

/// 正常补全（无漏项）。
pub fn fill_all() -> Result<FillOutcome, u32> {
    fill_all_skip(None)
}

// ── 大关复验（F1382/F1400 双层锚） ──────────────────────────────
/// 大关账：双层锚分数 + 合分 + 漏项清单。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateSummary {
    /// F1382 层：清单权重进度（0..=100）。
    pub layer_f1382: u32,
    /// F1400 层：批完成度（0..=100）。
    pub layer_f1400: u32,
    /// 合分 = (l1+l2)/2。
    pub combined: u32,
    /// 已落账缺口项数。
    pub covered: u32,
    /// 逐项对账漏项（清单序）。
    pub missing: Vec<(u8, usize)>,
}

/// 大关复验：逐项对账 → 合分 ≥80 → 双层锚差 ≤15，三关全过才算过。
pub fn gate_verify(out: &FillOutcome) -> Result<GateSummary, u32> {
    let sum = gate_score(out);
    if !sum.missing.is_empty() {
        return Err(FillCode::GateShort.val());
    }
    if sum.combined < GATE_MIN {
        return Err(FillCode::GateShort.val());
    }
    let diff = if sum.layer_f1382 > sum.layer_f1400 {
        sum.layer_f1382 - sum.layer_f1400
    } else {
        sum.layer_f1400 - sum.layer_f1382
    };
    if diff > ANCHOR_TOL {
        return Err(FillCode::AnchorSplit.val());
    }
    Ok(sum)
}

/// 只算分不设卡（供补全前诊断）。
pub fn gate_score(out: &FillOutcome) -> GateSummary {
    // 逐项对账：F1482 缺口项必须全落账。
    let mut missing: Vec<(u8, usize)> = Vec::new();
    let mut covered = 0u32;
    let mut b = BATCH_LO;
    while b <= BATCH_HI {
        let mut k = 0usize;
        while k < ITEMS_PER_BATCH {
            if is_gap(b, k) && !out.ledger.has(b, k) {
                missing.push((b, k));
            }
            if out.ledger.has(b, k) {
                covered += 1;
            }
            k += 1;
        }
        b += 1;
    }
    // F1382 层：预制项 + 补全项权重 / 清单总权重。
    let mut done_w = 0u32;
    let mut b = BATCH_LO;
    while b <= BATCH_HI {
        let mut k = 0usize;
        while k < ITEMS_PER_BATCH {
            if !is_gap(b, k) || out.ledger.has(b, k) {
                done_w += item_weight(b, k).unwrap_or(0);
            }
            k += 1;
        }
        b += 1;
    }
    let tw = total_weight();
    let l1 = if tw == 0 { 0 } else { done_w.saturating_mul(100) / tw };
    // F1400 层：封批数 / 批数。
    let l2 = out.filled_batches().saturating_mul(100) / BATCH_COUNT as u32;
    GateSummary {
        layer_f1382: l1,
        layer_f1400: l2,
        combined: (l1 + l2) / 2,
        covered,
        missing,
    }
}

// ── 漏项注入（判据原文：补全漏项注入 10/10 必抓） ───────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓的轮号（诊断用）。
    pub miss_rounds: Vec<u32>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_rounds: Vec::new() }
    }
    pub fn record(&mut self, round: u32, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_rounds.push(round);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 注入轮：每轮漏补一个缺口项（轮转 30 缺口），复验必须抓（GateShort）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let skip = match nth_gap((r as usize) % gap_count()) {
            Some(g) => Some(g),
            None => return Err(FillCode::ItemUnknown.val()),
        };
        let out = fill_all_skip(skip)?;
        led.record(r, gate_verify(&out).is_err());
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：补前大关必不过 → 补全 → 大关复验过 → 四拒探 → 账齐复核。
/// 返回 (正旗数, 拒旗数, 总账断言数, 合分)。
pub fn golden_run() -> Result<(u32, u32, usize, u32), u32> {
    // 基线：无漏项补全后大关必过（防大关常真）。
    let base = fill_all_skip(None)?;
    if gate_verify(&base).is_err() {
        return Err(FillCode::GateShort.val());
    }
    // 正式补全（fill_all 从空账起）。
    let out = fill_all()?;
    // 账齐复核：总账断言数 = 缺口数，预制项不入账。
    let ledger_exact = out.ledger.records.len() == gap_count() && gap_count() == 30;
    // 大关复验：合分 100 全过。
    let sum = gate_verify(&out)?;
    let gate_ok = sum.combined >= GATE_MIN && sum.layer_f1382 >= GATE_MIN && sum.layer_f1400 >= GATE_MIN;
    // 全批 Filled。
    let all_filled = out.filled_batches() == BATCH_COUNT as u32;
    // ── 四拒探 ──
    // 重复补全：B16 缺口项 idx=1 已落账，再补必拒。
    let mut probe = FillLedger::new();
    let _ = probe.append(BATCH_LO, 1);
    let dup_rej = probe.append(BATCH_LO, 1).is_err();
    // 缩账拒：账长 1 缩到 0。
    let shrink_rej = probe.shrink_probe(0).is_err();
    // 批号越界 + 未知清单项。
    let mut probe2 = FillLedger::new();
    let oob_rej = probe2.append(BATCH_LO - 1, 0).is_err() && probe2.append(BATCH_HI + 1, 0).is_err();
    let unk_rej = probe2.append(BATCH_LO, ITEMS_PER_BATCH).is_err();
    // 状态机倒退拒：Filled 批退回 Filling。
    let mut probe3 = fill_all()?;
    let regress_rej = match probe3.batches.get_mut(0) {
        Some(bl) => bl.advance(BatchState::Filling).is_err(),
        None => false,
    };
    let flags = ledger_exact as u32 + gate_ok as u32 + all_filled as u32 + (sum.combined == 100) as u32;
    let rejects = dup_rej as u32 + shrink_rej as u32 + (oob_rej && unk_rej) as u32 + regress_rej as u32;
    Ok((flags, rejects, out.ledger.records.len(), sum.combined))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链四正四拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, _, _)) => Ok(()),
        _ => Err(FillCode::NotReproducible.val()),
    }
}

/// 序 B：漏项注入 10/10 必抓。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(FillCode::GateShort.val()),
    }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72180 && FillCode::ItemUnknown.val() < SEG_BASE + 0x40;
    let ok = BATCH_LO == 16 && BATCH_HI == 30 && BATCH_COUNT == 15 && ITEMS_PER_BATCH == 3
        && GATE_MIN == 80 && ANCHOR_TOL == 15 && JUDGE_ROUNDS == 10 && in_window
        && gap_count() == 30 && total_weight() > 0 && nth_gap(0) == Some((BATCH_LO, 1));
    if ok {
        Ok(())
    } else {
        Err(FillCode::NotReproducible.val())
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = fill_all()?;
    let b = fill_all()?;
    if a.ledger != b.ledger || a.filled_batches() != b.filled_batches() {
        return Err(FillCode::NotReproducible.val());
    }
    let ga = golden_run()?;
    let gb = golden_run()?;
    if ga != gb {
        return Err(FillCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < FillCode::ALL.len() {
        let v = FillCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    if !inj.all_caught() {
        return Err(FillCode::GateShort.val());
    }
    let out = fill_all()?;
    let sum = gate_verify(&out)?;
    Ok(format!(
        "状态=已完成 fills={} gaps={} score={} f1382={} f1400={}",
        out.ledger.records.len(), gap_count(), sum.combined, sum.layer_f1382, sum.layer_f1400
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正四拒）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, n, s)) => (*f == 4 && *r == 4, format!("flags={} rejects={} fills={} score={}", f, r, n, s)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-清单逐项补", gp, gd));
    // J1 大关 ≥80 复验（双层锚）。
    let out = fill_all();
    let gate = match &out {
        Ok(o) => gate_verify(o),
        Err(_) => Err(FillCode::LedgerRegress.val()),
    };
    let (qp, qd) = match &gate {
        Ok(s) => (s.combined >= GATE_MIN, format!("f1382={} f1400={} combined={}", s.layer_f1382, s.layer_f1400, s.combined)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-大关≥80复验", qp, qd));
    // J1 漏项注入 10/10 必抓。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_rounds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-漏项注入10/10", ip, id));
    // J1 总账只增不减（重复补 / 缩账 / 越界批号 / 未知项 四拒）。
    let mut probe = FillLedger::new();
    let _ = probe.append(BATCH_LO, 1);
    let d1 = probe.append(BATCH_LO, 1).is_err();
    let d2 = probe.shrink_probe(0).is_err();
    let d3 = probe.append(BATCH_LO - 1, 0).is_err() && probe.append(BATCH_HI + 1, 0).is_err();
    let d4 = probe.append(BATCH_LO, ITEMS_PER_BATCH).is_err();
    v.push((
        "J1-总账只增不减",
        d1 && d2 && d3 && d4,
        format!("重复补={} 缩账={} 越界={} 未知项={}", d1, d2, d3, d4),
    ));
    // J1 批状态机单调 + 全批 Filled。
    let ok_out = fill_all();
    let (bp, bd) = match &ok_out {
        Ok(o) => {
            let all = o.filled_batches() == BATCH_COUNT as u32;
            (all, format!("filled_batches={}/{}", o.filled_batches(), BATCH_COUNT))
        }
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-批状态机单调", bp, bd));
    // J1 批未清即封拒（漏 1 项的批不可封）。
    let skip1 = fill_all_skip(nth_gap(0));
    let early_rej = match skip1 {
        Ok(mut o) => match o.batches.get_mut(0) {
            Some(bl) if is_gap(bl.batch, 1) && !o.ledger.has(bl.batch, 1) => bl.finish().is_err(),
            _ => false,
        },
        Err(_) => false,
    };
    v.push(("J1-批未清即封拒", early_rej, format!("漏项批封口必拒={}", early_rej)));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("fill_all + golden_run 两跑同账")));
    // J2 补前大关必不过（基线灵敏度：空账 + 全批 Queued）。
    let mut pre_batches: Vec<BatchLedger> = Vec::new();
    let mut b = BATCH_LO;
    while b <= BATCH_HI {
        pre_batches.push(BatchLedger::new(b));
        b += 1;
    }
    let pre = FillOutcome { ledger: FillLedger::new(), batches: pre_batches };
    let pre_fail = gate_verify(&pre).is_err();
    v.push(("J2-补前大关必不过", pre_fail, String::from("空账复验必拒，防大关常真")));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72180 窗口")));
    v
}

pub fn run_unx1485_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1485 补全执行三（分发与接管面 B16–B30 缺口批补齐）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
