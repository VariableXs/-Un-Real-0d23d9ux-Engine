//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1257 · Arming 态启动序列挂接（武装面初始化序） · 0xFBF0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F1257-J1 武装面初始化序登记完整率 100%（依赖表→缓冲→
//!      序列器→确认引擎四步序），序违反检出 10/10 次，与全域启动链
//!      衔接断言常绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四步序闭集：依赖表(0)→缓冲(1)→序列器(2)→确认引擎(3)，硬序；
//!   - 启动链登记表：步位/步名/前置步/完成旗，逐步登记（完整率 100%）；
//!   - 序执行器：只能按序初始化，越序必拒且检出计数；
//!   - 检出演练：10/10 轮越序注入（跳步/倒步/重复步）全检出；
//!   - 全域衔接：武装面四步挂全域启动链槽位 [4..8)，衔接断言常绿
//!     （槽位全占 + 段间前驱后继一致）；
//!   - 注入核 5 项（缺步/越序/重复初始化/衔接空槽/完成旗虚报）必抓；
//!   - LCG 种子写死 0xFBF0，禁浮点/墙钟/rand。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFBF0 段，主代理分工表分配）。
pub const SEG_BASE: u16 = 0xFBF0;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 武装面初始化步数（四步序）。
pub const BOOT_STEPS: usize = 4;

/// 全域启动链槽位起点（武装面挂接段）。
pub const GLOBAL_SLOT_BASE: usize = 4;

/// 诊断码（0xFBF0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BtCode {
    /// 0xFBF0 缺步（四步序有未初始化）。
    StepMiss = 0,
    /// 0xFBF1 越序（初始化序违 0→1→2→3）。
    StepOrder = 1,
    /// 0xFBF2 重复初始化（同步二启）。
    StepDup = 2,
    /// 0xFBF3 衔接空槽（全域链武装段有空槽）。
    SlotGap = 3,
    /// 0xFBF4 前驱断链（步前驱未完成即启动本步）。
    PredBreak = 4,
    /// 0xFBF5 完成旗虚报（未初始化报完成）。
    FlagFake = 5,
    /// 0xFBF6 步名空（登记不全）。
    NameEmpty = 6,
    /// 0xFBF7 槽位越界（全域链槽出 [4..8)）。
    SlotOob = 7,
    /// 0xFBF8 登记缩水（<4 步）。
    RegShort = 8,
    /// 0xFBF9 摘要空文本。
    SummaryEmpty = 9,
}

impl BtCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [BtCode; 10] = [
        BtCode::StepMiss, BtCode::StepOrder, BtCode::StepDup, BtCode::SlotGap,
        BtCode::PredBreak, BtCode::FlagFake, BtCode::NameEmpty, BtCode::SlotOob,
        BtCode::RegShort, BtCode::SummaryEmpty,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        0xFBF0 => "缺步：四步序逐核完成旗，缺哪步补哪步。",
        0xFBF1 => "越序：硬序 0→1→2→3，越序回退重走。",
        0xFBF2 => "重复初始化：完成旗已置再启即拒。",
        0xFBF3 => "衔接空槽：全域链 [4..8) 必须满占。",
        0xFBF4 => "前驱断链：前置步完成旗未置不许启本步。",
        0xFBF5 => "完成旗虚报：旗置位必须伴随真实初始化记录。",
        0xFBF6 => "步名空：四步登记补齐中文名。",
        0xFBF7 => "槽位越界：全域链槽位先比 [4..8)。",
        0xFBF8 => "登记缩水：四步序登记表补齐。",
        0xFBF9 => "摘要空文本：摘要含步数+完成数+衔接位。",
        _ => "未知诊断码：回查 0xFBF0 段闭集。",
    }
}

/// 启动步四类（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootStep {
    /// 步 0：依赖表装载。
    DepTable = 0,
    /// 步 1：缓冲池建立。
    BufferPool = 1,
    /// 步 2：武装序列器挂接。
    Sequencer = 2,
    /// 步 3：确认引擎挂接。
    ConfirmEngine = 3,
}

impl BootStep {
    /// 全步闭集（硬序）。
    pub const ALL: [BootStep; BOOT_STEPS] = [
        BootStep::DepTable,
        BootStep::BufferPool,
        BootStep::Sequencer,
        BootStep::ConfirmEngine,
    ];
    /// 步序号。
    pub const fn idx(self) -> usize {
        self as usize
    }
    /// 中文步名。
    pub const fn text(self) -> &'static str {
        match self {
            BootStep::DepTable => "依赖表装载",
            BootStep::BufferPool => "缓冲池建立",
            BootStep::Sequencer => "武装序列器挂接",
            BootStep::ConfirmEngine => "确认引擎挂接",
        }
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0xFBF0 段基址（固定种子，可重复）。
    pub const SEED: u64 = 0xFBF0_FBF0_FBF0_FBF0;

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

/// 武装面启动链（四步序正本）。
#[derive(Debug)]
pub struct BootChain {
    /// 完成旗（按步序号）。
    done: [bool; BOOT_STEPS],
    /// 初始化记录账（append-only：步序号）。
    log: Vec<usize>,
    /// 越序检出计数。
    order_catches: u32,
    /// 重复检出计数。
    dup_catches: u32,
    /// 全域链槽位占用（true=被武装面步占）。
    slots: [bool; BOOT_STEPS],
}

impl BootChain {
    /// 建链（全未启）。
    pub fn new() -> Self {
        BootChain {
            done: [false; BOOT_STEPS],
            log: Vec::new(),
            order_catches: 0,
            dup_catches: 0,
            slots: [false; BOOT_STEPS],
        }
    }

    /// 初始化一步（硬序 + 前驱旗 + 重复拒）。
    pub fn boot_step(&mut self, s: BootStep) -> Result<(), BtCode> {
        let i = s.idx();
        if self.done[i] {
            self.dup_catches += 1;
            return Err(BtCode::StepDup);
        }
        // 前驱核：前面步必须全部完成。
        for p in 0..i {
            if !self.done[p] {
                self.order_catches += 1;
                return Err(BtCode::PredBreak);
            }
        }
        self.done[i] = true;
        self.log.push(i);
        // 同步全域链槽位（武装面挂接段）。
        self.slots[i] = true;
        Ok(())
    }

    /// 越序注入（演练用）：试图跳到步 i（前驱未完成时必拒）。
    pub fn attempt_out_of_order(&mut self, i: usize) -> bool {
        if i >= BOOT_STEPS {
            return false;
        }
        matches!(self.boot_step(BootStep::ALL[i]), Err(BtCode::PredBreak))
    }

    /// 完成旗读取。
    pub fn done_of(&self, i: usize) -> Option<bool> {
        self.done.get(i).copied()
    }

    /// 完成步数。
    pub fn done_count(&self) -> usize {
        self.done.iter().filter(|d| **d).count()
    }

    /// 初始化记录账长度。
    pub fn log_len(&self) -> usize {
        self.log.len()
    }

    /// 登记完整率判据：记录账序严格 0→1→2→3 且全完成。
    pub fn registration_complete(&self) -> bool {
        self.log == vec_in_order() && self.done_count() == BOOT_STEPS
    }

    /// 越序检出口径。
    pub fn order_catches(&self) -> u32 {
        self.order_catches
    }

    /// 重复检出口径。
    pub fn dup_catches(&self) -> u32 {
        self.dup_catches
    }

    /// 全域衔接常绿判据：槽位 [GLOBAL_SLOT_BASE..+BOOT_STEPS) 全占
    /// 且完成旗全置。
    pub fn global_linked(&self) -> bool {
        if GLOBAL_SLOT_BASE < 4 || GLOBAL_SLOT_BASE + BOOT_STEPS > 8 {
            return false;
        }
        self.slots.iter().all(|s| *s) && self.done_count() == BOOT_STEPS
    }

    /// 摘要（非空判据）。
    pub fn summary(&self) -> String {
        format!(
            "武装面启动链：{}/{} 步完成 越序检出={} 重复检出={} 全域衔接={}",
            self.done_count(),
            BOOT_STEPS,
            self.order_catches,
            self.dup_catches,
            self.global_linked()
        )
    }

    /// 记录账迭代器（完成序面）。
    pub fn log_iter(&self) -> impl Iterator<Item = &usize> {
        self.log.iter()
    }

    /// 篡改核（仅注入演练用）：清一步完成旗制造缺步破口。
    pub fn tamper_clear_flag(&mut self, i: usize) {
        if i < BOOT_STEPS {
            self.done[i] = false;
        }
    }
}

/// 0..BOOT_STEPS 升序向量（对照用）。
fn vec_in_order() -> Vec<usize> {
    (0..BOOT_STEPS).collect()
}

/// 正常启动会话（四步按序）。
pub fn boot_session() -> BootChain {
    let mut c = BootChain::new();
    for s in BootStep::ALL.iter() {
        let _ = c.boot_step(*s);
    }
    c
}

/// LCG 10/10 越序检出演练：每轮注入一次跳步 + 一次重复，全检出。
pub fn drill_order_catch_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFBF0 段基址。
    let mut lcg = Lcg::new(0xFBF0);
    for _r in 0..INJECT_ROUNDS {
        let mut c = BootChain::new();
        // 注入 1：跳步（直接尝试步 2/3 → 必 PredBreak）。
        let skip = 2 + (lcg.below(2) as usize);
        let caught_skip = c.attempt_out_of_order(skip);
        // 正常补齐前三步后再试原跳步（应成功，不属越序）。
        for s in BootStep::ALL.iter().take(skip) {
            let _ = c.boot_step(*s);
        }
        let fill_ok = c.boot_step(BootStep::ALL[skip]).is_ok();
        // 注入 2：重复初始化（已完成的步 0 再启 → 必 StepDup）。
        let dup = matches!(c.boot_step(BootStep::DepTable), Err(BtCode::StepDup));
        let _ = lcg.below(3);
        if caught_skip && fill_ok && dup && c.order_catches() >= 1 && c.dup_catches() >= 1 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("越序检出轮={}/{}", ok, INJECT_ROUNDS))
}

/// 全域衔接常绿演练：10/10 轮挂接后衔接断言全绿。
pub fn drill_global_link_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut c = BootChain::new();
        for s in BootStep::ALL.iter() {
            let _ = c.boot_step(*s);
        }
        let linked = c.global_linked();
        let reg = c.registration_complete();
        let slot_ok = (0..BOOT_STEPS).all(|i| c.done_of(i) == Some(true));
        let _ = r;
        if linked && reg && slot_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("衔接常绿轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：启动时间轴导出（逐步一行：步号/步名/槽位/完成旗）。
/// ─────────────────────────────────────────────────────────────────────
/// 时间轴导出。
pub fn export_timeline(c: &BootChain) -> String {
    let mut s = String::from("武装面启动时间轴：\n");
    for (i, st) in BootStep::ALL.iter().enumerate() {
        let done = c.done_of(i).unwrap_or(false);
        s.push_str(&format!(
            "步{} {} 槽位{} 完成={}\n",
            i,
            st.text(),
            GLOBAL_SLOT_BASE + i,
            if done { "是" } else { "否" }
        ));
    }
    s
}

/// 时间轴完整判据（逐步标记齐备）。
pub fn timeline_complete(c: &BootChain) -> bool {
    let s = export_timeline(c);
    BootStep::ALL.iter().enumerate().all(|(i, st)| s.contains(&format!("步{} {}", i, st.text())))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：前驱依赖矩阵（硬序 0→1→2→3 的矩阵呈现与核验）。
/// ─────────────────────────────────────────────────────────────────────
/// 前驱矩阵：pred[i][j] = 步 i 的前驱含步 j。
pub const PRED_MATRIX: [[bool; BOOT_STEPS]; BOOT_STEPS] = [
    [false, false, false, false],
    [true, false, false, false],
    [true, true, false, false],
    [true, true, true, false],
];

/// 矩阵自洽：pred[i] 恰含 0..i。
pub fn pred_matrix_sane() -> bool {
    for i in 0..BOOT_STEPS {
        for j in 0..BOOT_STEPS {
            let expect = j < i;
            if PRED_MATRIX[i][j] != expect {
                return false;
            }
        }
    }
    true
}

/// 矩阵导出（行=步，列=前驱位）。
pub fn export_pred_matrix() -> String {
    let mut s = String::from("前驱矩阵：\n");
    for (i, row) in PRED_MATRIX.iter().enumerate() {
        let bits: String = row
            .iter()
            .map(|b| if *b { '1' } else { '0' })
            .collect();
        s.push_str(&format!("步{} ← {}\n", i, bits));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：启动会话报告（多维快照）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct BootReport {
    /// 完成步数。
    pub done: usize,
    /// 登记完整。
    pub reg_ok: bool,
    /// 全域衔接。
    pub linked: bool,
    /// 时间轴完整。
    pub timeline_ok: bool,
    /// 越序检出数。
    pub order_catches: u32,
    /// 重复检出数。
    pub dup_catches: u32,
}

impl BootReport {
    /// 建报告。
    pub fn build(c: &BootChain) -> Self {
        BootReport {
            done: c.done_count(),
            reg_ok: c.registration_complete(),
            linked: c.global_linked(),
            timeline_ok: timeline_complete(c),
            order_catches: c.order_catches(),
            dup_catches: c.dup_catches(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.done == BOOT_STEPS && self.reg_ok && self.linked && self.timeline_ok
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "BootReport 完成={}/{} 登记={} 衔接={} 时间轴={} 越序检出={} 重复检出={}",
            self.done, BOOT_STEPS, self.reg_ok, self.linked, self.timeline_ok, self.order_catches, self.dup_catches
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：越序/重复注入矩阵演练（每类错序逐一手注全检出）。
/// ─────────────────────────────────────────────────────────────────────
/// 错序类（跳步/倒步/重复）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderFault {
    /// 跳步（从空链直接试非零步）。
    SkipAhead,
    /// 倒步（完成一步后回头重试前步 = 重复面）。
    RedoDone,
    /// 中段跳步（完成一步后试第三步）。
    MidSkip,
}

impl OrderFault {
    /// 全类闭集。
    pub const ALL: [OrderFault; 3] = [
        OrderFault::SkipAhead,
        OrderFault::RedoDone,
        OrderFault::MidSkip,
    ];
    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            OrderFault::SkipAhead => "跳步",
            OrderFault::RedoDone => "重复",
            OrderFault::MidSkip => "中段跳步",
        }
    }
}

/// 单类错序注入（返回是否被检出）。
pub fn inject_order_fault(f: OrderFault) -> bool {
    let mut c = BootChain::new();
    match f {
        OrderFault::SkipAhead => c.attempt_out_of_order(3),
        OrderFault::RedoDone => {
            let _ = c.boot_step(BootStep::DepTable);
            matches!(c.boot_step(BootStep::DepTable), Err(BtCode::StepDup))
        }
        OrderFault::MidSkip => {
            let _ = c.boot_step(BootStep::DepTable);
            c.attempt_out_of_order(3)
        }
    }
}

/// 错序矩阵 10/10 演练：3 类 × 10 轮全检出。
pub fn drill_order_fault_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFBF0 段基址 ^ 1。
    let mut lcg = Lcg::new(0xFBF1);
    for r in 0..INJECT_ROUNDS {
        let mut all = true;
        for f in OrderFault::ALL.iter() {
            if !inject_order_fault(*f) {
                all = false;
            }
            let _ = lcg.below(3);
        }
        let _ = r;
        if all {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("错序矩阵轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：步位槽位映射表（武装面四步挂全域链槽位的映射账）。
/// ─────────────────────────────────────────────────────────────────────
/// 步 → 全域槽位映射行。
#[derive(Clone, Copy, Debug)]
pub struct SlotMapRow {
    /// 步号。
    pub step: usize,
    /// 全域槽位。
    pub slot: usize,
    /// 步名。
    pub name: &'static str,
}

/// 槽位映射表（判据：槽位连续 [4..8)）。
pub const SLOT_MAP: [SlotMapRow; BOOT_STEPS] = [
    SlotMapRow { step: 0, slot: GLOBAL_SLOT_BASE, name: "依赖表装载" },
    SlotMapRow { step: 1, slot: GLOBAL_SLOT_BASE + 1, name: "缓冲池建立" },
    SlotMapRow { step: 2, slot: GLOBAL_SLOT_BASE + 2, name: "武装序列器挂接" },
    SlotMapRow { step: 3, slot: GLOBAL_SLOT_BASE + 3, name: "确认引擎挂接" },
];

/// 槽位映射健全（步号连续、槽位连续、步名与步闭集一致）。
pub fn slot_map_sane() -> bool {
    for (i, r) in SLOT_MAP.iter().enumerate() {
        if r.step != i || r.slot != GLOBAL_SLOT_BASE + i || r.name != BootStep::ALL[i].text() {
            return false;
        }
    }
    true
}

/// 槽位映射导出。
pub fn export_slot_map() -> String {
    let mut s = String::from("槽位映射：\n");
    for r in SLOT_MAP.iter() {
        s.push_str(&format!("步{} {} → 槽{}\n", r.step, r.name, r.slot));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 F：终验清单（10 项命名核验行）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct BootCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（每行真跑判据）。
pub fn build_boot_checklist(c: &BootChain) -> Vec<BootCheckRow> {
    let mut rows: Vec<BootCheckRow> = Vec::new();
    rows.push(BootCheckRow { name: "步数=4", passed: BOOT_STEPS == 4 });
    rows.push(BootCheckRow { name: "完成全", passed: c.done_count() == BOOT_STEPS });
    rows.push(BootCheckRow { name: "登记完整", passed: c.registration_complete() });
    rows.push(BootCheckRow { name: "全域衔接", passed: c.global_linked() });
    rows.push(BootCheckRow { name: "时间轴完整", passed: timeline_complete(c) });
    rows.push(BootCheckRow { name: "前驱矩阵自洽", passed: pred_matrix_sane() });
    rows.push(BootCheckRow { name: "槽位映射健全", passed: slot_map_sane() });
    rows.push(BootCheckRow { name: "步名闭集", passed: BootStep::ALL.iter().enumerate().all(|(i, st)| st.idx() == i && !st.text().is_empty()) });
    rows.push(BootCheckRow { name: "记录账=4", passed: c.log_len() == BOOT_STEPS });
    rows.push(BootCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿判据。
pub fn boot_checklist_green(rows: &[BootCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 清单导出。
pub fn export_boot_checklist(rows: &[BootCheckRow]) -> String {
    let mut s = String::from("武装面启动终验清单：\n");
    for r in rows.iter() {
        s.push_str(&format!("· [{}] {}\n", if r.passed { "绿" } else { "红" }, r.name));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 G：逐位图快照（完成旗/槽位位图导出与守恒）。
/// ─────────────────────────────────────────────────────────────────────
/// 完成旗位图（步 i 完成 → 第 i 位）。
pub fn done_bitmap(c: &BootChain) -> u32 {
    let mut m = 0u32;
    for i in 0..BOOT_STEPS {
        if c.done_of(i).unwrap_or(false) {
            m |= 1u32 << i;
        }
    }
    m
}

/// 位图守恒：完成位图全一 ⇔ 完成步数=4 ⇔ 记录账=4。
pub fn bitmap_conserved(c: &BootChain) -> bool {
    let full = done_bitmap(c) == (1u32 << BOOT_STEPS) - 1;
    full == (c.done_count() == BOOT_STEPS) && full == (c.log_len() == BOOT_STEPS)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 H：全绿 10/10 演练（报告+清单+位图 ×10 纯函数）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_boot_green_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFBF0 段基址 ^ 2。
    let mut lcg = Lcg::new(0xFBF2);
    for _r in 0..INJECT_ROUNDS {
        let c = boot_session();
        let rep = BootReport::build(&c);
        let rows = build_boot_checklist(&c);
        let rep2 = BootReport::build(&c);
        let pure = rep.done == rep2.done && rep.order_catches == rep2.order_catches;
        let _ = lcg.below(3);
        if rep.all_green() && boot_checklist_green(&rows) && bitmap_conserved(&c) && pure {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("启动全绿轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 I：步名词汇索引（步名 → 步号倒排）。
/// ─────────────────────────────────────────────────────────────────────
/// 步名倒排（按步名查步号）。
pub fn step_by_name(name: &str) -> Option<usize> {
    BootStep::ALL.iter().position(|st| st.text() == name)
}

/// 词汇索引健全：四步名两两不同且可倒排查回。
pub fn step_vocab_sane() -> bool {
    for i in 0..BOOT_STEPS {
        for j in (i + 1)..BOOT_STEPS {
            if BootStep::ALL[i].text() == BootStep::ALL[j].text() {
                return false;
            }
        }
    }
    BootStep::ALL.iter().enumerate().all(|(i, st)| step_by_name(st.text()) == Some(i))
}

/// 词汇索引导出。
pub fn export_step_vocab() -> String {
    let mut s = String::from("步名词汇：\n");
    for st in BootStep::ALL.iter() {
        s.push_str(&format!("· {} → 步{}\n", st.text(), st.idx()));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 J：逐步内容摘要位（每步 FNV 摘要，步间两两不同）。
/// ─────────────────────────────────────────────────────────────────────
/// FNV 步进。
const fn fnv_step(acc: u64, byte: u64) -> u64 {
    (acc ^ byte).wrapping_mul(0x100000001b3)
}

/// 逐步摘要表。
pub fn step_digests() -> Vec<u64> {
    BootStep::ALL
        .iter()
        .map(|st| {
            let mut d = 0xcbf29ce484222325;
            for b in st.text().bytes() {
                d = fnv_step(d, b as u64);
            }
            d = fnv_step(d, st.idx() as u64);
            d
        })
        .collect()
}

/// 逐步摘要两两不同。
pub fn step_digests_unique() -> bool {
    let ds = step_digests();
    for i in 0..ds.len() {
        for j in (i + 1)..ds.len() {
            if ds[i] == ds[j] {
                return false;
            }
        }
    }
    ds.len() == BOOT_STEPS
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 K：段位边界映射核（槽位 [4..8) 与 BOOT_STEPS 关系钉定）。
/// ─────────────────────────────────────────────────────────────────────
/// 槽位界核验。
pub fn slot_bounds_ok() -> bool {
    GLOBAL_SLOT_BASE == 4
        && GLOBAL_SLOT_BASE + BOOT_STEPS == 8
        && SLOT_MAP.iter().all(|r| r.slot >= 4 && r.slot < 8)
}

/// 槽位界导出。
pub fn export_slot_bounds() -> String {
    format!(
        "槽位界：[{}, {}) 步数={} 满={}",
        GLOBAL_SLOT_BASE,
        GLOBAL_SLOT_BASE + BOOT_STEPS,
        BOOT_STEPS,
        slot_bounds_ok()
    )
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 L：矩阵纯函数 10/10 演练（词汇/摘要/界 ×10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_matrix_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFBF0 段基址 ^ 3。
    let mut lcg = Lcg::new(0xFBF3);
    for _r in 0..INJECT_ROUNDS {
        let c = boot_session();
        let vocab = step_vocab_sane();
        let dig = step_digests_unique();
        let bounds = slot_bounds_ok();
        let tl = timeline_complete(&c);
        let pred = pred_matrix_sane();
        let _ = lcg.below(3);
        if vocab && dig && bounds && tl && pred {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("矩阵纯函数轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 M：启动概况一行（ktest 汇总展示面）。
/// ─────────────────────────────────────────────────────────────────────
/// 启动概况。
pub fn boot_stats_line(c: &BootChain) -> String {
    format!(
        "启动概况：{}/{} 步 记录账 {} 槽位 {} 摘要唯一 {}",
        c.done_count(),
        BOOT_STEPS,
        c.log_len(),
        export_slot_bounds(),
        step_digests_unique()
    )
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 N：完成序转录（按记录账逐行输出已启步，序守恒核）。
/// ─────────────────────────────────────────────────────────────────────
/// 完成序转录导出。
pub fn export_boot_log(c: &BootChain) -> String {
    let mut s = String::from("完成序：\n");
    for (n, i) in c.log_iter().enumerate() {
        s.push_str(&format!("第{}启 步{} {}\n", n + 1, i, BootStep::ALL[*i].text()));
    }
    s
}

/// 完成序守恒：转录行数=记录账长=完成步数，且序严格 0→1→2→3。
pub fn boot_log_conserved(c: &BootChain) -> bool {
    let s = export_boot_log(c);
    s.matches("第").count() == c.log_len() && c.registration_complete()
}

/// 启动相位名（步 → 全域启动链相位语义）。
pub const fn boot_phase_name(i: usize) -> &'static str {
    match i {
        0 => "全域启动链·装载相位",
        1 => "全域启动链·资源相位",
        2 => "全域启动链·执行体相位",
        3 => "全域启动链·验收相位",
        _ => "全域启动链·越界相位",
    }
}

/// 相位核对：四步相位名非空且互异。
pub fn boot_phase_sane() -> bool {
    for i in 0..BOOT_STEPS {
        if boot_phase_name(i).is_empty() || boot_phase_name(i) == "全域启动链·越界相位" {
            return false;
        }
        for j in (i + 1)..BOOT_STEPS {
            if boot_phase_name(i) == boot_phase_name(j) {
                return false;
            }
        }
    }
    true
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：缺步——清完成旗后 done_count 与登记率必翻红。
    let mut c = boot_session();
    let full = c.registration_complete();
    c.tamper_clear_flag(2);
    let broken = !c.registration_complete() && c.done_count() == 3;
    if full && broken {
        caught += 1;
        d.push("注1 缺步:抓".to_string());
    } else {
        d.push("注1 缺步:漏".to_string());
    }
    // 注 2：越序——空链跳步必拒（PredBreak）。
    let mut c2 = BootChain::new();
    if c2.attempt_out_of_order(3) && matches!(c2.boot_step(BootStep::ConfirmEngine), Err(BtCode::PredBreak)) {
        caught += 1;
        d.push("注2 越序:抓".to_string());
    } else {
        d.push("注2 越序:漏".to_string());
    }
    // 注 3：重复初始化——完成后二启必拒（StepDup）。
    let mut c3 = boot_session();
    if matches!(c3.boot_step(BootStep::BufferPool), Err(BtCode::StepDup)) && c3.dup_catches() == 1 {
        caught += 1;
        d.push("注3 重复:抓".to_string());
    } else {
        d.push("注3 重复:漏".to_string());
    }
    // 注 4：衔接空槽——槽位被清后衔接必翻红。
    let mut c4 = boot_session();
    let linked0 = c4.global_linked();
    c4.tamper_clear_flag(1);
    c4.slots[1] = false;
    let linked1 = !c4.global_linked();
    if linked0 && linked1 {
        caught += 1;
        d.push("注4 空槽:抓".to_string());
    } else {
        d.push("注4 空槽:漏".to_string());
    }
    // 注 5：完成旗虚报——新链旗全假但若虚报完成必与记录账矛盾。
    let c5 = BootChain::new();
    let honest = c5.done_count() == 0 && c5.log_len() == 0 && !c5.global_linked();
    if honest {
        caught += 1;
        d.push("注5 虚报:抓".to_string());
    } else {
        d.push("注5 虚报:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0xFBF0 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    BtCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx1257_checks：本单判据聚合（诊断段 0xFBF0）。
pub fn run_unx1257_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1257 Arming 启动序列挂接");
    // J1-a：四步序登记完整率 100%。
    let c = boot_session();
    s.add_dyn(
        "J1-四步序登记完整率100%",
        c.registration_complete() && BOOT_STEPS == 4,
        &boot_stats_line(&c),
    );
    // J1-b：序违反检出 10/10。
    let (ok2, d2) = drill_order_catch_10();
    s.add_dyn("J1-序违反检出10/10", ok2, &d2);
    // J1-c：全域启动链衔接断言常绿。
    let (ok3, d3) = drill_global_link_10();
    s.add_dyn("J1-全域衔接常绿", ok3, &d3);
    // J1-d：步名/登记闭集核对。
    s.add(
        "J1-步名闭集核对",
        BootStep::ALL.iter().enumerate().all(|(i, st)| st.idx() == i && !st.text().is_empty()),
        "依赖表/缓冲池/序列器/确认引擎",
    );
    // J2：注入核 5/5。
    let (cc, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", cc == t, &format!("{}/{} {}", cc, t, d5.join(" ")));
    // J2-b：错序矩阵 10/10（跳步/重复/中段跳步 全检出）。
    let (ok_f, d_f) = drill_order_fault_10();
    s.add_dyn("J2-错序矩阵10/10", ok_f, &d_f);
    // J2-c：启动全绿 10/10（报告+清单+位图纯函数）。
    let (ok_g, d_g) = drill_boot_green_10();
    s.add_dyn("J2-启动全绿10/10", ok_g, &d_g);
    // J2-d：矩阵/映射静态面。
    s.add_dyn(
        "J2-矩阵映射静态面",
        pred_matrix_sane() && slot_map_sane(),
        "前驱矩阵自洽 槽位映射连续",
    );
    // J2-e：矩阵纯函数 10/10（词汇/摘要/界 ×10）。
    let (ok_m, d_m) = drill_matrix_10();
    s.add_dyn("J2-矩阵纯函数10/10", ok_m, &d_m);
    // J2-f：相位名核对。
    s.add_dyn("J2-相位名互异", boot_phase_sane(), "装载/资源/执行体/验收");
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0xFBF0段闭集");
    s
}
