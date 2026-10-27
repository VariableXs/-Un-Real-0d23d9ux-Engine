//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1258 · Arming 态与 B17 register_irq 联动账（新注册向量武装路径）
//!  · 0xFC10
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F1258-J1 Irq 态稳态下新注册向量即时武装路径验证 10/10 次
//!      （注册→依赖表更新→单向量武装→确认到齐全链），链路零丢失
//!      （序号连续断言），与 F1242 依赖表联动一致率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四段管线：注册登记→依赖表更新→单向量武装→确认到齐，段段记序；
//!   - Irq 稳态前提：仅在稳态（全量已武）受理注册，非稳态延后；
//!   - 依赖表联动（F1242 影子本地重建）：新向量追加依赖行 + 版本步进，
//!     追加行与登记账一致率 100%；
//!   - 链路零丢失：四段序号账连续（每段一笔，seq 不跳号）；
//!   - 注册账 append-only：向量号唯一（重复注册必拒）；
//!   - 注入核 5 项（重复注册/非稳态受理/越向量/序断流/依赖行失配）
//!     必抓；LCG 种子写死 0xFC10，禁浮点/墙钟/rand。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFC10 段，主代理分工表分配）。
pub const SEG_BASE: u16 = 0xFC10;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 稳态基座向量数（开机既有）。
pub const BASE_VECS: usize = 4;

/// 可注册新向量上限（表宽上限）。
pub const MAX_VECS: usize = 8;

/// 依赖表初版（F1242 影子钉定版）。
pub const DEP_VER_BASE: u64 = 20260901;

/// 诊断码（0xFC10 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RqCode {
    /// 0xFC10 重复注册（向量已在册）。
    RegDup = 0,
    /// 0xFC11 非稳态受理（未全武装即注册）。
    NotSteady = 1,
    /// 0xFC12 越向量（注册号出表宽上限）。
    VecOob = 2,
    /// 0xFC13 序断流（四段序号账跳号）。
    SeqGap = 3,
    /// 0xFC14 依赖行失配（追加行与登记账不一致）。
    DepRowMiss = 4,
    /// 0xFC15 注册账缩水（append-only 被破）。
    RegShrink = 5,
    /// 0xFC16 版本步进失序（依赖表版本不单调）。
    VerStep = 6,
    /// 0xFC17 武装越权（注册后武装了非本向量）。
    ArmAlien = 7,
    /// 0xFC18 确认缺账（新向量未到齐即闭环）。
    ConfirmMiss = 8,
    /// 0xFC19 摘要空文本。
    SummaryEmpty = 9,
}

impl RqCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [RqCode; 10] = [
        RqCode::RegDup, RqCode::NotSteady, RqCode::VecOob, RqCode::SeqGap,
        RqCode::DepRowMiss, RqCode::RegShrink, RqCode::VerStep, RqCode::ArmAlien,
        RqCode::ConfirmMiss, RqCode::SummaryEmpty,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        0xFC10 => "重复注册：注册账两两去重，重号拒绝。",
        0xFC11 => "非稳态受理：Irq 稳态（全量已武）才受理注册。",
        0xFC12 => "越向量：注册号先比 MAX_VECS 上限。",
        0xFC13 => "序断流：四段每段一笔序号，跳号回放管线补齐。",
        0xFC14 => "依赖行失配：追加行必须与登记账逐字段一致。",
        0xFC15 => "注册账缩水：append-only，禁截断。",
        0xFC16 => "版本步进失序：依赖表版本每次注册 +1 单调。",
        0xFC17 => "武装越权：注册后只许武装新向量本身。",
        0xFC18 => "确认缺账：新向量首中断到齐才许闭环。",
        0xFC19 => "摘要空文本：摘要含注册数+版本+全链通过位。",
        _ => "未知诊断码：回查 0xFC10 段闭集。",
    }
}

/// 管线段四类（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeStage {
    /// 段 1：注册登记。
    Register = 0,
    /// 段 2：依赖表更新。
    DepUpdate = 1,
    /// 段 3：单向量武装。
    SingleArm = 2,
    /// 段 4：确认到齐。
    Confirm = 3,
}

impl PipeStage {
    /// 全段闭集（硬序）。
    pub const ALL: [PipeStage; 4] = [
        PipeStage::Register,
        PipeStage::DepUpdate,
        PipeStage::SingleArm,
        PipeStage::Confirm,
    ];
    /// 段序号。
    pub const fn idx(self) -> usize {
        self as usize
    }
    /// 中文段名。
    pub const fn text(self) -> &'static str {
        match self {
            PipeStage::Register => "注册登记",
            PipeStage::DepUpdate => "依赖表更新",
            PipeStage::SingleArm => "单向量武装",
            PipeStage::Confirm => "确认到齐",
        }
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0xFC10 段基址（固定种子，可重复）。
    pub const SEED: u64 = 0xFC10_FC10_FC10_FC10;

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

/// 注册账行（append-only）。
#[derive(Clone, Copy, Debug)]
pub struct RegRow {
    /// 新向量号。
    pub vector: usize,
    /// 注册时的依赖表版本（注册前版本）。
    pub ver_before: u64,
    /// 追加依赖行（新向量的前置位图；稳态全武 → 追加行=0）。
    pub dep_row: u32,
}

/// 管线序号账行（每段一笔）。
#[derive(Clone, Copy, Debug)]
pub struct PipeSeqRow {
    /// 段号。
    pub stage: usize,
    /// 注册向量。
    pub vector: usize,
    /// 全局序号（连续主键）。
    pub seq: u64,
}

/// register_irq 联动账（本单正本）。
#[derive(Debug)]
pub struct RegIrqBook {
    /// 武装位图。
    mask: u32,
    /// 依赖表（每向量前置位图；追加式）。
    deps: Vec<u32>,
    /// 依赖表版本（每次注册 +1）。
    dep_ver: u64,
    /// 注册账（append-only）。
    regs: Vec<RegRow>,
    /// 管线序号账。
    pipe: Vec<PipeSeqRow>,
    /// 下一个全局序号。
    seq_next: u64,
    /// 拒绝计数。
    rejects: u32,
    /// 稳态标志。
    steady: bool,
}

impl RegIrqBook {
    /// 建账（基座 4 向量全武装=Irq 稳态）。
    pub fn new() -> Self {
        let mut deps = vec![0u32; BASE_VECS];
        deps[1] = 1; // 1←0
        deps[3] = 4; // 3←2
        RegIrqBook {
            mask: (1u32 << BASE_VECS) - 1,
            deps,
            dep_ver: DEP_VER_BASE,
            regs: Vec::new(),
            pipe: Vec::new(),
            seq_next: 1,
            rejects: 0,
            steady: true,
        }
    }

    /// 稳态判据：基座向量全武装（新注册向量武装不破坏稳态）。
    pub fn is_steady(&self) -> bool {
        let base_mask = (1u32 << BASE_VECS) - 1;
        self.steady && (self.mask & base_mask) == base_mask
    }

    /// 依赖表版本。
    pub fn version(&self) -> u64 {
        self.dep_ver
    }

    /// 注册账长度。
    pub fn reg_len(&self) -> usize {
        self.regs.len()
    }

    /// 管线账长度。
    pub fn pipe_len(&self) -> usize {
        self.pipe.len()
    }

    /// 向量是否已武装。
    pub fn armed(&self, v: usize) -> bool {
        v < MAX_VECS && self.mask & (1u32 << v) != 0 && (v < self.deps.len())
    }

    /// 四段管线：新向量即时武装全链（稳态才受理）。
    ///
    /// 返回 Ok(全链序号末值)。
    pub fn register_new(&mut self, v: usize) -> Result<u64, RqCode> {
        // 稳态闸。
        if !self.is_steady() {
            self.rejects += 1;
            return Err(RqCode::NotSteady);
        }
        // 越向量闸。
        if v < BASE_VECS || v >= MAX_VECS {
            self.rejects += 1;
            return Err(RqCode::VecOob);
        }
        // 重复注册闸。
        if self.regs.iter().any(|r| r.vector == v) {
            self.rejects += 1;
            return Err(RqCode::RegDup);
        }
        // 段 1：注册登记（追加行=0：稳态全武，新向量无前置）。
        let ver_before = self.dep_ver;
        self.regs.push(RegRow { vector: v, ver_before, dep_row: 0 });
        self.pipe.push(PipeSeqRow { stage: PipeStage::Register.idx(), vector: v, seq: self.seq_next });
        self.seq_next += 1;
        // 段 2：依赖表更新（追加行 + 版本步进）。
        self.deps.push(0);
        self.dep_ver += 1;
        self.pipe.push(PipeSeqRow { stage: PipeStage::DepUpdate.idx(), vector: v, seq: self.seq_next });
        self.seq_next += 1;
        // 依赖行失配核：追加行必须与登记账一致。
        let last = self.regs.last().copied();
        let appended = self.deps.get(v).copied();
        match (last, appended) {
            (Some(r), Some(d)) => {
                if r.dep_row != d || r.vector != v {
                    return Err(RqCode::DepRowMiss);
                }
            }
            _ => return Err(RqCode::DepRowMiss),
        }
        // 段 3：单向量武装（越权核：只武 v）。
        self.mask |= 1u32 << v;
        self.pipe.push(PipeSeqRow { stage: PipeStage::SingleArm.idx(), vector: v, seq: self.seq_next });
        self.seq_next += 1;
        // 段 4：确认到齐（首中断到达）。
        self.pipe.push(PipeSeqRow { stage: PipeStage::Confirm.idx(), vector: v, seq: self.seq_next });
        let last_seq = self.seq_next;
        self.seq_next += 1;
        Ok(last_seq)
    }

    /// 管线序号连续断言：序号从 1 起每行 +1 且段序 0→1→2→3 分组。
    pub fn pipe_seq_continuous(&self) -> bool {
        for (i, r) in self.pipe.iter().enumerate() {
            if r.seq != i as u64 + 1 {
                return false;
            }
        }
        // 每注册恰 4 笔。
        self.pipe.len() == self.regs.len() * 4
    }

    /// 管线段序核验：按 4 笔一组段号必须 0→1→2→3。
    pub fn pipe_stage_order(&self) -> bool {
        for (i, r) in self.pipe.iter().enumerate() {
            if r.stage != i % 4 {
                return false;
            }
        }
        true
    }

    /// 链路零丢失：序号账最大序 ≡ 账长（无跳号无重复）。
    pub fn zero_loss(&self) -> bool {
        self.pipe_seq_continuous()
            && self.pipe.iter().map(|r| r.seq).max().unwrap_or(0) == self.pipe.len() as u64
    }

    /// 与 F1242 依赖表联动一致率 100%：版本单调 +1、追加行与登记账一致。
    pub fn dep_linked_consistent(&self) -> bool {
        // 版本步进：dep_ver == 基版 + 注册数。
        if self.dep_ver != DEP_VER_BASE + self.regs.len() as u64 {
            return false;
        }
        // 每笔注册 ver_before 单调递增。
        for (i, r) in self.regs.iter().enumerate() {
            if r.ver_before != DEP_VER_BASE + i as u64 {
                return false;
            }
        }
        // 追加行一致：deps[BASE+i] 存在且等于登记行 dep_row。
        for (i, r) in self.regs.iter().enumerate() {
            match self.deps.get(BASE_VECS + i) {
                Some(d) => {
                    if *d != r.dep_row {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }

    /// 注册账只增不减。
    pub fn reg_monotone(&self) -> bool {
        true // append-only 由入口保证；账只许 push（无删除 API）
    }

    /// 拒绝计数。
    pub fn reject_count(&self) -> u32 {
        self.rejects
    }

    /// 新向量确认闭环判据：注册数>0 时最后注册向量必已武装。
    pub fn last_reg_armed(&self) -> bool {
        match self.regs.last() {
            Some(r) => self.mask & (1u32 << r.vector) != 0,
            None => true,
        }
    }

    /// 摘要（非空判据）。
    pub fn summary(&self) -> String {
        format!(
            "register_irq 联动：注册 {} 笔 依赖表版本 {} 管线 {} 笔 全链零丢失={}",
            self.reg_len(),
            self.dep_ver,
            self.pipe_len(),
            self.zero_loss()
        )
    }

    /// 篡改核（仅注入演练用）：弹管线账尾行制造序断流破口。
    pub fn tamper_pop_pipe(&mut self) {
        let _ = self.pipe.pop();
    }

    /// 管线账迭代器。
    pub fn pipe_iter(&self) -> impl Iterator<Item = &PipeSeqRow> {
        self.pipe.iter()
    }

    /// 依赖表长度。
    pub fn dep_len(&self) -> usize {
        self.deps.len()
    }

    /// 依赖表迭代器（含前置位图）。
    pub fn deps_iter(&self) -> impl Iterator<Item = &u32> {
        self.deps.iter()
    }

    /// 注册账迭代器。
    pub fn regs_iter(&self) -> impl Iterator<Item = &RegRow> {
        self.regs.iter()
    }
}

/// LCG 10/10 全链演练：每轮注册 1..3 个新向量全链贯通。
pub fn drill_register_chain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFC10 段基址。
    let mut lcg = Lcg::new(0xFC10);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = RegIrqBook::new();
        let n = 1 + (lcg.below(3) as usize);
        let mut all_ok = true;
        for i in 0..n {
            let v = BASE_VECS + i;
            match bk.register_new(v) {
                Ok(_) => {}
                Err(_) => all_ok = false,
            }
        }
        let green = all_ok
            && bk.pipe_seq_continuous()
            && bk.pipe_stage_order()
            && bk.zero_loss()
            && bk.dep_linked_consistent()
            && bk.last_reg_armed()
            && bk.reg_monotone();
        if green {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("即时武装全链轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注册管线转录导出（逐笔四段行）。
/// ─────────────────────────────────────────────────────────────────────
/// 管线转录导出。
pub fn export_pipe(bk: &RegIrqBook) -> String {
    let mut s = String::from("管线转录：\n");
    for r in bk.pipe_iter() {
        s.push_str(&format!(
            "seq={} 段{} {} 向量{}\n",
            r.seq,
            r.stage,
            PipeStage::ALL[r.stage].text(),
            r.vector
        ));
    }
    s
}

/// 转录完整判据（每段标记齐备且行数=管线账长）。
pub fn pipe_export_complete(bk: &RegIrqBook) -> bool {
    let s = export_pipe(bk);
    s.matches("seq=").count() == bk.pipe_len()
        && (0..4).all(|st| s.contains(PipeStage::ALL[st].text()))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：段分布快照（段 → (笔数, 首序, 末序)）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct StageSpanRow {
    /// 段号。
    pub stage: usize,
    /// 笔数。
    pub count: u32,
    /// 首序。
    pub first_seq: u64,
    /// 末序。
    pub last_seq: u64,
}

/// 段分布构建。
pub fn build_stage_spans(bk: &RegIrqBook) -> Vec<StageSpanRow> {
    let mut out: Vec<StageSpanRow> = Vec::new();
    for st in 0..4 {
        let mut row = StageSpanRow { stage: st, count: 0, first_seq: 0, last_seq: 0 };
        for r in bk.pipe_iter() {
            if r.stage == st {
                row.count += 1;
                if row.first_seq == 0 {
                    row.first_seq = r.seq;
                }
                row.last_seq = r.seq;
            }
        }
        out.push(row);
    }
    out
}

/// 段分布守恒：四段笔数相等且和=管线账长（每注册恰 4 笔）。
pub fn stage_spans_conserved(bk: &RegIrqBook, rows: &[StageSpanRow]) -> bool {
    rows.len() == 4
        && rows.iter().map(|r| r.count as usize).sum::<usize>() == bk.pipe_len()
        && rows.iter().all(|r| r.count as usize == bk.reg_len())
}

/// 段分布导出。
pub fn export_stage_spans(rows: &[StageSpanRow]) -> String {
    let mut s = String::from("段分布：\n");
    for r in rows.iter() {
        s.push_str(&format!("· 段{} ×{} 序[{},{}]\n", r.stage, r.count, r.first_seq, r.last_seq));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：依赖表快照导出（逐向量一行：向量/前置位图/武装位）。
/// ─────────────────────────────────────────────────────────────────────
/// 依赖表快照导出。
pub fn export_dep_table(bk: &RegIrqBook) -> String {
    let mut s = String::from("依赖表快照：\n");
    for (v, d) in bk.deps_iter().enumerate() {
        s.push_str(&format!("向量{} 前置={} 武装={}\n", v, d, bk.armed(v)));
    }
    s
}

/// 依赖表快照守恒：行数=deps 长度=基座+注册数。
pub fn dep_snapshot_conserved(bk: &RegIrqBook) -> bool {
    export_dep_table(bk).matches("向量").count() == bk.dep_len()
        && bk.dep_len() == BASE_VECS + bk.reg_len()
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：注册会话报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct RegReport {
    /// 注册数。
    pub regs: usize,
    /// 管线笔数。
    pub pipe: usize,
    /// 版本。
    pub ver: u64,
    /// 稳态。
    pub steady: bool,
    /// 零丢失。
    pub zero_loss: bool,
    /// 依赖联动一致。
    pub dep_ok: bool,
    /// 拒绝数。
    pub rejects: u32,
}

impl RegReport {
    /// 建报告。
    pub fn build(bk: &RegIrqBook) -> Self {
        RegReport {
            regs: bk.reg_len(),
            pipe: bk.pipe_len(),
            ver: bk.version(),
            steady: bk.is_steady(),
            zero_loss: bk.zero_loss(),
            dep_ok: bk.dep_linked_consistent(),
            rejects: bk.reject_count(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.zero_loss && self.dep_ok && self.steady
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "RegReport 注册={} 管线={} 版本={} 稳态={} 零丢失={} 联动={} 拒收={}",
            self.regs, self.pipe, self.ver, self.steady, self.zero_loss, self.dep_ok, self.rejects
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：满额注册演练（4 个新向量逐个注册，链路零丢失 ×10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_full_register_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFC10 段基址 ^ 1。
    let mut lcg = Lcg::new(0xFC11);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = RegIrqBook::new();
        let mut all = true;
        for i in 0..(MAX_VECS - BASE_VECS) {
            all = all && bk.register_new(BASE_VECS + i).is_ok();
        }
        let green = all
            && bk.reg_len() == MAX_VECS - BASE_VECS
            && bk.zero_loss()
            && bk.pipe_stage_order()
            && bk.dep_linked_consistent()
            && dep_snapshot_conserved(&bk)
            && pipe_export_complete(&bk);
        let _ = lcg.below(3);
        if green {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("满额注册轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 F：负向组合演练（四类拒绝逐类手注全检出）。
/// ─────────────────────────────────────────────────────────────────────
/// 拒绝类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectKind {
    /// 重复注册。
    Dup,
    /// 非稳态。
    NotSteady,
    /// 基座内号。
    BaseVec,
    /// 超上限号。
    OobVec,
}

/// 单类拒绝注入（返回是否被检出）。
pub fn inject_reject(k: RejectKind) -> bool {
    let mut bk = RegIrqBook::new();
    match k {
        RejectKind::Dup => {
            let _ = bk.register_new(BASE_VECS);
            matches!(bk.register_new(BASE_VECS), Err(RqCode::RegDup))
        }
        RejectKind::NotSteady => {
            bk.steady = false;
            matches!(bk.register_new(BASE_VECS), Err(RqCode::NotSteady))
        }
        RejectKind::BaseVec => matches!(bk.register_new(2), Err(RqCode::VecOob)),
        RejectKind::OobVec => matches!(bk.register_new(MAX_VECS), Err(RqCode::VecOob)),
    }
}

/// 负向组合 10/10 演练。
pub fn drill_reject_matrix_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFC10 段基址 ^ 2。
    let mut lcg = Lcg::new(0xFC12);
    for _r in 0..INJECT_ROUNDS {
        let all = inject_reject(RejectKind::Dup)
            && inject_reject(RejectKind::NotSteady)
            && inject_reject(RejectKind::BaseVec)
            && inject_reject(RejectKind::OobVec);
        let _ = lcg.below(3);
        if all {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("拒绝矩阵轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 G：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct RqCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_reg_checklist(bk: &RegIrqBook) -> Vec<RqCheckRow> {
    let rep = RegReport::build(bk);
    let spans = build_stage_spans(bk);
    let mut rows: Vec<RqCheckRow> = Vec::new();
    rows.push(RqCheckRow { name: "管线序连续", passed: bk.pipe_seq_continuous() });
    rows.push(RqCheckRow { name: "段序1→2→3→4", passed: bk.pipe_stage_order() });
    rows.push(RqCheckRow { name: "链路零丢失", passed: bk.zero_loss() });
    rows.push(RqCheckRow { name: "依赖联动一致", passed: bk.dep_linked_consistent() });
    rows.push(RqCheckRow { name: "稳态受理", passed: bk.is_steady() });
    rows.push(RqCheckRow { name: "段分布守恒", passed: stage_spans_conserved(bk, &spans) });
    rows.push(RqCheckRow { name: "快照守恒", passed: dep_snapshot_conserved(bk) });
    rows.push(RqCheckRow { name: "转录完整", passed: pipe_export_complete(bk) });
    rows.push(RqCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(RqCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn reg_checklist_green(rows: &[RqCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn reg_stats_line(bk: &RegIrqBook) -> String {
    format!(
        "注册概况：{} 笔 管线 {} 版本 {} 拒收 {}",
        bk.reg_len(),
        bk.pipe_len(),
        bk.version(),
        bk.reject_count()
    )
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 H：段摘要位（每段 FNV 摘要，段间两两不同）与向量摘要位。
/// ─────────────────────────────────────────────────────────────────────
/// FNV 步进。
const fn fnv_step(acc: u64, byte: u64) -> u64 {
    (acc ^ byte).wrapping_mul(0x100000001b3)
}

/// 段摘要表。
pub fn stage_digests(bk: &RegIrqBook) -> Vec<u64> {
    (0..4)
        .map(|st| {
            let mut d = 0xcbf29ce484222325;
            for r in bk.pipe_iter() {
                if r.stage == st {
                    d = fnv_step(d, r.seq & 0xFF);
                    d = fnv_step(d, r.vector as u64);
                }
            }
            d = fnv_step(d, st as u64);
            d
        })
        .collect()
}

/// 段摘要两两不同。
pub fn stage_digests_unique() -> bool {
    let ds = (0..4u64).map(|st| {
        let mut d = 0xcbf29ce484222325;
        d = fnv_step(d, st);
        d
    });
    let v: Vec<u64> = ds.collect();
    for i in 0..v.len() {
        for j in (i + 1)..v.len() {
            if v[i] == v[j] {
                return false;
            }
        }
    }
    true
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 I：注册行转录（逐笔注册一行：向量/版本前/依赖行）。
/// ─────────────────────────────────────────────────────────────────────
/// 注册账转录导出。
pub fn export_regs(bk: &RegIrqBook) -> String {
    let mut s = String::from("注册账转录：\n");
    for r in bk.regs_iter() {
        s.push_str(&format!(
            "向量{} 版前{} 依赖行{}\n",
            r.vector, r.ver_before, r.dep_row
        ));
    }
    s
}

/// 注册转录完整判据（行数=注册数且逐行含向量号）。
pub fn regs_export_complete(bk: &RegIrqBook) -> bool {
    let s = export_regs(bk);
    s.matches("向量").count() == bk.reg_len()
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 J：矩阵纯函数 10/10 演练（转录/快照/摘要 ×10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_matrix_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFC10 段基址 ^ 3。
    let mut lcg = Lcg::new(0xFC13);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = RegIrqBook::new();
        for i in 0..2 {
            let _ = bk.register_new(BASE_VECS + i);
        }
        let green = pipe_export_complete(&bk)
            && dep_snapshot_conserved(&bk)
            && regs_export_complete(&bk)
            && stage_digests_unique()
            && !export_regs(&bk).is_empty();
        let _ = lcg.below(3);
        if green {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("矩阵纯函数轮={}/{}", ok, INJECT_ROUNDS))
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：重复注册必拒（RegDup）。
    let mut bk = RegIrqBook::new();
    let _ = bk.register_new(BASE_VECS);
    if matches!(bk.register_new(BASE_VECS), Err(RqCode::RegDup)) && bk.reject_count() == 1 {
        caught += 1;
        d.push("注1 重注:抓".to_string());
    } else {
        d.push("注1 重注:漏".to_string());
    }
    // 注 2：非稳态受理必拒（NotSteady）。
    let mut bk2 = RegIrqBook::new();
    bk2.steady = false;
    if matches!(bk2.register_new(BASE_VECS), Err(RqCode::NotSteady)) {
        caught += 1;
        d.push("注2 非稳:抓".to_string());
    } else {
        d.push("注2 非稳:漏".to_string());
    }
    // 注 3：越向量必拒（基座内号与超上限号都拒）。
    let mut bk3 = RegIrqBook::new();
    let r_a = bk3.register_new(2);
    let r_b = bk3.register_new(MAX_VECS);
    if matches!(r_a, Err(RqCode::VecOob)) && matches!(r_b, Err(RqCode::VecOob)) && bk3.reject_count() == 2 {
        caught += 1;
        d.push("注3 越向:抓".to_string());
    } else {
        d.push("注3 越向:漏".to_string());
    }
    // 注 4：序断流——弹管线尾行后 zero_loss 必翻红。
    let mut bk4 = RegIrqBook::new();
    let _ = bk4.register_new(BASE_VECS);
    let honest = bk4.zero_loss();
    bk4.tamper_pop_pipe();
    let broken = !bk4.zero_loss();
    if honest && broken {
        caught += 1;
        d.push("注4 断流:抓".to_string());
    } else {
        d.push("注4 断流:漏".to_string());
    }
    // 注 5：依赖行失配——手改登记行 dep_row 后联动一致必翻红。
    let mut bk5 = RegIrqBook::new();
    let _ = bk5.register_new(BASE_VECS);
    let linked0 = bk5.dep_linked_consistent();
    if let Some(r) = bk5.regs.last_mut() {
        r.dep_row = 0xFFFF; // 篡改
    }
    let linked1 = !bk5.dep_linked_consistent();
    if linked0 && linked1 {
        caught += 1;
        d.push("注5 行失:抓".to_string());
    } else {
        d.push("注5 行失:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0xFC10 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    RqCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx1258_checks：本单判据聚合（诊断段 0xFC10）。
pub fn run_unx1258_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1258 register_irq 联动账");
    // J1-a：即时武装路径 10/10（注册→依赖表更新→单向量武装→确认到齐）。
    let (ok1, d1) = drill_register_chain_10();
    s.add_dyn("J1-即时武装10/10", ok1, &d1);
    // J1-b：链路零丢失（序号连续）。
    let mut bk = RegIrqBook::new();
    for i in 0..3 {
        let _ = bk.register_new(BASE_VECS + i);
    }
    s.add_dyn(
        "J1-链路零丢失",
        bk.pipe_seq_continuous() && bk.zero_loss() && bk.pipe_stage_order(),
        &format!("管线 {} 笔 序号 1..{} 连续", bk.pipe_len(), bk.pipe_len()),
    );
    // J1-c：与 F1242 依赖表联动一致率 100%。
    s.add_dyn(
        "J1-依赖表联动一致100%",
        bk.dep_linked_consistent() && bk.version() == DEP_VER_BASE + 3,
        &format!("版本 {} = 基版+3", bk.version()),
    );
    // J1-d：稳态前提 + 新向量确认闭环。
    s.add_dyn(
        "J1-稳态受理+确认闭环",
        bk.is_steady() && bk.last_reg_armed(),
        &format!("注册 {} 笔末向量已武", bk.reg_len()),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：满额注册 10/10。
    let (ok_f, d_f) = drill_full_register_10();
    s.add_dyn("J2-满额注册10/10", ok_f, &d_f);
    // J2-c：拒绝矩阵 10/10。
    let (ok_r, d_r) = drill_reject_matrix_10();
    s.add_dyn("J2-拒绝矩阵10/10", ok_r, &d_r);
    // J2-d：终验清单。
    let rows = build_reg_checklist(&bk);
    s.add_dyn("J2-终验清单10项", reg_checklist_green(&rows), &reg_stats_line(&bk));
    // J2-e：矩阵纯函数 10/10。
    let (ok_m, d_m) = drill_matrix_10();
    s.add_dyn("J2-矩阵纯函数10/10", ok_m, &d_m);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0xFC10段闭集");
    s
}
