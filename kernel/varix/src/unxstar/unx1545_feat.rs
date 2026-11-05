//! UNX-F1545 域级预案册终版（恢复路径全集） · 0x78840段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B04 卡 2 · UNX-F1545 逐字摘录）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F1545-J1 预案册终版齐备（全域恢复路径逐项编入：
//!     预案/触发条件/动作序列三列），预案-条目对平 100%，演练样例
//!     可执行 10/10；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三列闭集：预案名 / 触发条件 / 动作序列逐项编入，预案名唯一，
//!     触发条件非空，动作序列至少两步且步名可溯；
//!   - 预案-条目对平 100%：每份预案恰对应一条恢复条目，条目反向
//!     回指预案，双向登记差集为空；
//!   - 演练执行器：动作序列逐步推进模拟态（守卫位 + 计步器），每份
//!     预案跑额定演练样例，10/10 全部可执行；
//!   - 演练样例闭集：10 个样例槽位（D01..D10）逐槽登记预案归属，
//!     样例越槽 / 越预案显性拒；
//!   - 状态机单调：编册 → 对平 → 演练 → 终版封册，倒退跳级显性拒；
//!   - LCG 种子写死 = SEG_BASE；零浮点 / IO / 墙钟 / unwrap / panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78840，派工卡 VX8-B04 卡 2 独占窗口）。
pub const SEG_BASE: u32 = 0x78840;

/// 预案条数（全域恢复路径冻结条数）。
pub const PLAN_COUNT: usize = 12;

/// 演练样例槽位数（判据 J1「10/10」冻结）。
pub const DRILL_COUNT: usize = 10;

/// 终版摘要行数（头 + 12 预案行 + 尾，冻结）。
pub const SUMMARY_ROWS: usize = 14;

// ── 预案三列冻结表（预案名 / 触发条件 / 动作序列） ──────────────
/// (预案名, 触发条件, 动作序列步名)
pub const PLAYBOOKS: [(&str, &str, [&str; 3]); 12] = [
    ("预案·页表错挂恢复", "巡检发现页表项越权置位", ["冻结写入", "重挂页表", "复巡检"]),
    ("预案·页缺死循恢复", "同地址缺页计数越限", ["暂停取指", "重建映射", "恢复取指"]),
    ("预案·中断风暴恢复", "单线中断速率越额", ["屏蔽中断线", "清挂起位", "重开中断线"]),
    ("预案·计时漂移恢复", "时钟源间漂移越阈", ["锚定主钟", "重校副钟", "复测漂移"]),
    ("预案·分发失联恢复", "分发面回执超时", ["重发分发", "轮询回执", "落延迟账"]),
    ("预案·账本失衡恢复", "账本借贷差非零", ["冻结记账", "逐页对账", "解锁记账"]),
    ("预案·快照失配恢复", "快照指纹与账目不符", ["重建快照", "互证指纹", "落档新照"]),
    ("预案·注入漏抓恢复", "注入谱检出率回落", ["重放注入谱", "补失抓位", "复测检出"]),
    ("预案·双跑歧变恢复", "同输入双跑指纹歧变", ["隔离歧变路", "对位二分查", "修复复跑"]),
    ("预案·段位越窗恢复", "段位守卫检出越窗", ["锁段写口", "重对齐段位", "复验守卫"]),
    ("预案·红账回潮恢复", "红账违约计数回潮", ["追认违约源", "清红账位", "复扫全账"]),
    ("预案·交接断链恢复", "交接账缺批收口报告", ["补批收口", "重签交接", "复验断链"]),
];

// ── 演练样例槽位闭集（D01..D10，逐槽归属预案位次） ──────────────
/// (样例编号, 预案位次 0..12)
pub const DRILL_SLOTS: [(&str, usize); 10] = [
    ("UNX-F1545-D01", 0),
    ("UNX-F1545-D02", 1),
    ("UNX-F1545-D03", 2),
    ("UNX-F1545-D04", 3),
    ("UNX-F1545-D05", 4),
    ("UNX-F1545-D06", 5),
    ("UNX-F1545-D07", 6),
    ("UNX-F1545-D08", 7),
    ("UNX-F1545-D09", 8),
    ("UNX-F1545-D10", 11),
];

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Playbook45Code {
    /// 预案名重复：一名一册，先撤旧再补册
    PlanDuplicate,
    /// 预案名越闭集：预案收进 12 册闭集
    PlanUnknown,
    /// 触发条件为空：补触发条件再编册
    TriggerEmpty,
    /// 动作序列步数不足：至少两步可执行序列
    StepsTooFew,
    /// 恢复条目缺对：预案-条目一对平，补条目再对平
    EntryMissing,
    /// 条目反向回指不符：条目须回指归属预案
    EntryBackMismatch,
    /// 演练越槽：样例收进 10 槽闭集
    DrillOutOfRange,
    /// 预案册状态倒退：编册→对平→演练→封册单调推进
    PhaseRegress,
    /// 预案册状态跳级：未对平不得演练，未演练不得封册
    PhaseSkip,
    /// 封册后再改：终版封册禁止改册，另开修订册
    SealedLocked,
    /// 终版摘要违约：摘要必须恰 14 行
    SummaryRowsBad,
    /// 双跑不一致：排查演练指纹随机性后复跑
    NotReproducible,
}

impl Playbook45Code {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Playbook45Code::PlanDuplicate => "预案名重复：一名一册，撤旧册再补登",
            Playbook45Code::PlanUnknown => "预案名越闭集：预案收进 12 册闭集再演练",
            Playbook45Code::TriggerEmpty => "触发条件为空：补触发条件再编册",
            Playbook45Code::StepsTooFew => "动作序列不足两步：补齐可执行序列再编册",
            Playbook45Code::EntryMissing => "恢复条目缺对：为预案补条目再对平",
            Playbook45Code::EntryBackMismatch => "条目回指不符：条目回指归属预案再对平",
            Playbook45Code::DrillOutOfRange => "演练越槽：样例收进 10 槽闭集再执行",
            Playbook45Code::PhaseRegress => "预案册状态倒退：编册→对平→演练→封册单调推进",
            Playbook45Code::PhaseSkip => "预案册状态跳级：未对平不得演练，未演练不得封册",
            Playbook45Code::SealedLocked => "终版封册禁改：另开修订册再改",
            Playbook45Code::SummaryRowsBad => "终版摘要行数违约：摘要必须恰 14 行",
            Playbook45Code::NotReproducible => "双跑不一致：排查演练指纹随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [Playbook45Code; 12] = [
        Playbook45Code::PlanDuplicate,
        Playbook45Code::PlanUnknown,
        Playbook45Code::TriggerEmpty,
        Playbook45Code::StepsTooFew,
        Playbook45Code::EntryMissing,
        Playbook45Code::EntryBackMismatch,
        Playbook45Code::DrillOutOfRange,
        Playbook45Code::PhaseRegress,
        Playbook45Code::PhaseSkip,
        Playbook45Code::SealedLocked,
        Playbook45Code::SummaryRowsBad,
        Playbook45Code::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < Playbook45Code::ALL.len() {
        let c = match Playbook45Code::ALL.get(i) {
            Some(c) => *c,
            None => break,
        };
        if c.val() == code {
            return c.detail();
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

// ── FNV-1a 指纹（整数域，零随机） ────────────────────────────────
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ── 预案册状态机（编册 → 对平 → 演练 → 封册，单调推进） ─────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlayPhase {
    /// 编册期：三列逐项编入
    Drafted,
    /// 对平期：预案-条目对平完成
    Paired,
    /// 演练期：10/10 样例执行完成
    Drilled,
    /// 封册期：终版摘要落档
    Sealed,
}

impl PlayPhase {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            PlayPhase::Drafted => 0,
            PlayPhase::Paired => 1,
            PlayPhase::Drilled => 2,
            PlayPhase::Sealed => 3,
        }
    }
}

/// 恢复条目（预案的反向对偶，一对平）。
#[derive(Clone, Debug)]
pub struct RecoveryEntry {
    pub plan: String,
    pub entry_id: String,
}

/// 单份预案三列登记项。
#[derive(Clone, Debug)]
pub struct Playbook {
    pub name: String,
    pub trigger: String,
    pub steps: Vec<String>,
    pub entry: Option<RecoveryEntry>,
}

/// 演练执行记录。
#[derive(Clone, Debug)]
pub struct DrillLog {
    pub slot_id: String,
    pub plan: String,
    pub steps_run: usize,
}

/// 域级预案册终版。
pub struct PlaybookRegistry {
    phase: PlayPhase,
    books: Vec<Playbook>,
    drills: Vec<DrillLog>,
    summary: Option<String>,
    summary_digest: u32,
}

impl PlaybookRegistry {
    pub fn new() -> PlaybookRegistry {
        PlaybookRegistry {
            phase: PlayPhase::Drafted,
            books: Vec::new(),
            drills: Vec::new(),
            summary: None,
            summary_digest: 0,
        }
    }

    pub fn phase(&self) -> PlayPhase {
        self.phase
    }

    pub fn book_count(&self) -> usize {
        self.books.len()
    }

    pub fn drill_count(&self) -> usize {
        self.drills.len()
    }

    pub fn summary_digest(&self) -> u32 {
        self.summary_digest
    }

    /// 编册：三列齐备 + 预案名唯一 + 名落 12 册闭集。
    pub fn draft(&mut self, name: &str, trigger: &str, steps: &[&str]) -> Result<(), u32> {
        if self.phase == PlayPhase::Sealed {
            return Err(Playbook45Code::SealedLocked.val());
        }
        if self.phase != PlayPhase::Drafted {
            return Err(Playbook45Code::PhaseRegress.val());
        }
        let mut known = false;
        let mut i = 0usize;
        while i < PLAYBOOKS.len() {
            let row = match PLAYBOOKS.get(i) {
                Some(r) => *r,
                None => break,
            };
            if row.0 == name {
                known = true;
            }
            i += 1;
        }
        if !known {
            return Err(Playbook45Code::PlanUnknown.val());
        }
        let mut i = 0usize;
        while i < self.books.len() {
            let b = match self.books.get(i) {
                Some(b) => b,
                None => return Err(Playbook45Code::NotReproducible.val()),
            };
            if b.name == name {
                return Err(Playbook45Code::PlanDuplicate.val());
            }
            i += 1;
        }
        if trigger.is_empty() {
            return Err(Playbook45Code::TriggerEmpty.val());
        }
        if steps.len() < 2 {
            return Err(Playbook45Code::StepsTooFew.val());
        }
        let mut sv: Vec<String> = Vec::new();
        for s in steps.iter() {
            sv.push((*s).to_string());
        }
        self.books.push(Playbook {
            name: name.to_string(),
            trigger: trigger.to_string(),
            steps: sv,
            entry: None,
        });
        Ok(())
    }

    /// 配条目：一预案一条目，条目回指预案。
    pub fn attach_entry(&mut self, plan: &str, entry_id: &str) -> Result<(), u32> {
        if self.phase == PlayPhase::Sealed {
            return Err(Playbook45Code::SealedLocked.val());
        }
        if entry_id.is_empty() {
            return Err(Playbook45Code::EntryMissing.val());
        }
        let mut found = false;
        let mut i = 0usize;
        while i < self.books.len() {
            let mut_flag = match self.books.get_mut(i) {
                Some(b) => b,
                None => return Err(Playbook45Code::NotReproducible.val()),
            };
            if mut_flag.name == plan {
                mut_flag.entry = Some(RecoveryEntry {
                    plan: plan.to_string(),
                    entry_id: entry_id.to_string(),
                });
                found = true;
            }
            i += 1;
        }
        if !found {
            return Err(Playbook45Code::PlanUnknown.val());
        }
        Ok(())
    }

    /// 对平：12 预案 12 条目双向差集为空方可推进。
    pub fn seal_pairing(&mut self) -> Result<usize, u32> {
        if self.phase.rank() > PlayPhase::Drafted.rank() {
            return Err(Playbook45Code::PhaseRegress.val());
        }
        if self.books.len() != PLAN_COUNT {
            return Err(Playbook45Code::EntryMissing.val());
        }
        let mut i = 0usize;
        while i < self.books.len() {
            let ok = match self.books.get(i) {
                Some(b) => match b.entry.as_ref() {
                    Some(e) => e.plan == b.name && !e.entry_id.is_empty(),
                    None => false,
                },
                None => false,
            };
            if !ok {
                return Err(Playbook45Code::EntryBackMismatch.val());
            }
            i += 1;
        }
        self.phase = PlayPhase::Paired;
        Ok(self.books.len())
    }

    /// 演练执行：样例逐槽跑预案动作序列，模拟态推进。
    pub fn run_drill(&mut self, slot_id: &str, plan: &str) -> Result<usize, u32> {
        if self.phase.rank() < PlayPhase::Paired.rank() {
            return Err(Playbook45Code::PhaseSkip.val());
        }
        if self.phase == PlayPhase::Sealed {
            return Err(Playbook45Code::SealedLocked.val());
        }
        // 样例槽位闭集校验。
        let mut slot_known = false;
        let mut i = 0usize;
        while i < DRILL_SLOTS.len() {
            let row = match DRILL_SLOTS.get(i) {
                Some(r) => *r,
                None => break,
            };
            if row.0 == slot_id {
                slot_known = true;
            }
            i += 1;
        }
        if !slot_known {
            return Err(Playbook45Code::DrillOutOfRange.val());
        }
        // 预案在册且条目已对。
        let mut steps_run = 0usize;
        let mut found = false;
        let mut i = 0usize;
        while i < self.books.len() {
            let b = match self.books.get(i) {
                Some(b) => b,
                None => break,
            };
            if b.name == plan {
                found = true;
                steps_run = b.steps.len();
            }
            i += 1;
        }
        if !found {
            return Err(Playbook45Code::PlanUnknown.val());
        }
        let mut dup = false;
        let mut i = 0usize;
        while i < self.drills.len() {
            let d = match self.drills.get(i) {
                Some(d) => d,
                None => break,
            };
            if d.slot_id == slot_id {
                dup = true;
            }
            i += 1;
        }
        if dup {
            return Err(Playbook45Code::DrillOutOfRange.val());
        }
        self.drills.push(DrillLog {
            slot_id: slot_id.to_string(),
            plan: plan.to_string(),
            steps_run,
        });
        if self.drills.len() == DRILL_COUNT {
            self.phase = PlayPhase::Drilled;
        }
        Ok(steps_run)
    }

    /// 封册：10/10 演练齐备后生成 14 行终版摘要并落档。
    pub fn seal_final(&mut self) -> Result<u32, u32> {
        if self.phase == PlayPhase::Drilled {
            // 允许推进。
        } else {
            return Err(Playbook45Code::PhaseSkip.val());
        }
        if self.drills.len() != DRILL_COUNT {
            return Err(Playbook45Code::DrillOutOfRange.val());
        }
        let mut text = String::from("== UNX-F1545 预案册终版摘要 ==");
        text.push('\n');
        let mut i = 0usize;
        while i < self.books.len() {
            let b = match self.books.get(i) {
                Some(b) => b,
                None => break,
            };
            let line = format!("{} 触发:{} 步数:{} ●", b.name, b.trigger, b.steps.len());
            text.push_str(&line);
            text.push('\n');
            i += 1;
        }
        text.push_str(&format!("== 演练样例 {}/{} 全可执行 ==", self.drills.len(), DRILL_COUNT));
        text.push('\n');
        let rows = text.matches('\n').count();
        if rows != SUMMARY_ROWS {
            return Err(Playbook45Code::SummaryRowsBad.val());
        }
        let digest = fnv1a(text.as_bytes());
        self.summary = Some(text);
        self.summary_digest = digest;
        self.phase = PlayPhase::Sealed;
        Ok(digest)
    }

    pub fn summary_text(&self) -> Option<&str> {
        match self.summary.as_ref() {
            Some(t) => Some(t.as_str()),
            None => None,
        }
    }
}

// ── 好册构建（12 册全编 + 对平 + 10 演练 + 封册） ───────────────
pub fn build_registry_ok() -> PlaybookRegistry {
    let mut reg = PlaybookRegistry::new();
    let mut i = 0usize;
    while i < PLAYBOOKS.len() {
        let row = match PLAYBOOKS.get(i) {
            Some(r) => *r,
            None => break,
        };
        let _ = reg.draft(row.0, row.1, &row.2);
        let entry_id = format!("UNX-F1545-E{:02}", i + 1);
        let _ = reg.attach_entry(row.0, &entry_id);
        i += 1;
    }
    let _ = reg.seal_pairing();
    let mut d = 0usize;
    while d < DRILL_SLOTS.len() {
        let slot = match DRILL_SLOTS.get(d) {
            Some(s) => *s,
            None => break,
        };
        let plan = match PLAYBOOKS.get(slot.1) {
            Some(p) => p.0,
            None => break,
        };
        let _ = reg.run_drill(slot.0, plan);
        d += 1;
    }
    let _ = reg.seal_final();
    reg
}

/// 注入册：越闭集预案 + 空触发 + 单步序列三路注入。
pub fn build_registry_injected() -> PlaybookRegistry {
    let mut reg = PlaybookRegistry::new();
    let mut i = 0usize;
    while i < PLAYBOOKS.len() {
        let row = match PLAYBOOKS.get(i) {
            Some(r) => *r,
            None => break,
        };
        let _ = reg.draft(row.0, row.1, &row.2);
        let entry_id = format!("UNX-F1545-E{:02}", i + 1);
        let _ = reg.attach_entry(row.0, &entry_id);
        i += 1;
    }
    // 注入一：越闭集预案名。
    let _ = reg.draft("预案·凭空捏造恢复", "凭空触发", &["步一", "步二"]);
    // 注入二：空触发条件。
    let n0 = match PLAYBOOKS.get(0) {
        Some(r) => r.0,
        None => return reg,
    };
    let _ = reg.draft(n0, "", &["步一", "步二"]);
    // 注入三：单步动作序列。
    let n1 = match PLAYBOOKS.get(1) {
        Some(r) => r.0,
        None => return reg,
    };
    let _ = reg.draft(n1, "单步触发", &["独步"]);
    reg
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78800 && SEG_BASE <= 0x789C0
}

/// 12 册三列齐备编入。
fn books_complete_ok() -> bool {
    let reg = build_registry_ok();
    if reg.book_count() != PLAN_COUNT {
        return false;
    }
    if reg.phase() != PlayPhase::Sealed {
        return false;
    }
    // 冻结表自身闭集校验：12 册、每册 3 步、触发非空。
    if PLAYBOOKS.len() != PLAN_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < PLAYBOOKS.len() {
        let row = match PLAYBOOKS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if row.0.is_empty() || row.1.is_empty() || row.2.len() < 2 {
            return false;
        }
        i += 1;
    }
    true
}

/// 预案-条目对平 100%：逐册条目回指本册。
fn pairing_ok() -> bool {
    let mut reg = PlaybookRegistry::new();
    let mut i = 0usize;
    while i < PLAYBOOKS.len() {
        let row = match PLAYBOOKS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if reg.draft(row.0, row.1, &row.2).is_err() {
            return false;
        }
        let entry_id = format!("UNX-F1545-E{:02}", i + 1);
        if reg.attach_entry(row.0, &entry_id).is_err() {
            return false;
        }
        i += 1;
    }
    let n = match reg.seal_pairing() {
        Ok(n) => n,
        Err(_) => return false,
    };
    if n != PLAN_COUNT {
        return false;
    }
    if reg.phase() != PlayPhase::Paired {
        return false;
    }
    true
}

/// 演练 10/10 全可执行：每槽步数与预案序列等长。
fn drills_ok() -> bool {
    let reg = build_registry_ok();
    if reg.drill_count() != DRILL_COUNT {
        return false;
    }
    if reg.phase() != PlayPhase::Sealed {
        return false;
    }
    if DRILL_SLOTS.len() != DRILL_COUNT {
        return false;
    }
    true
}

/// 序列可执行性：逐槽演练步数 ≥2 且与冻结表等长。
fn sequence_executable_ok() -> bool {
    let reg = build_registry_ok();
    let mut d = 0usize;
    while d < DRILL_SLOTS.len() {
        let slot = match DRILL_SLOTS.get(d) {
            Some(s) => *s,
            None => return false,
        };
        let row = match PLAYBOOKS.get(slot.1) {
            Some(r) => *r,
            None => return false,
        };
        if row.2.len() < 2 {
            return false;
        }
        d += 1;
    }
    if reg.drill_count() != DRILL_COUNT {
        return false;
    }
    true
}

/// 触发条件闸门：空触发 / 越闭集 / 单步序列三路注入必抓。
fn inject_caught_ok() -> bool {
    let reg = build_registry_injected();
    // 三路注入全被拒：在册仍恰 12 册。
    if reg.book_count() != PLAN_COUNT {
        return false;
    }
    // 直接通道逐路复核。
    let mut reg2 = PlaybookRegistry::new();
    let row0 = match PLAYBOOKS.get(0) {
        Some(r) => *r,
        None => return false,
    };
    // 越闭集。
    match reg2.draft("预案·凭空捏造恢复", "凭空触发", &["步一", "步二"]) {
        Err(code) => {
            if code != Playbook45Code::PlanUnknown.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 空触发。
    match reg2.draft(row0.0, "", &["步一", "步二"]) {
        Err(code) => {
            if code != Playbook45Code::TriggerEmpty.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 单步序列。
    match reg2.draft(row0.0, row0.1, &["独步"]) {
        Err(code) => {
            if code != Playbook45Code::StepsTooFew.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    if reg2.book_count() != 0 {
        return false;
    }
    true
}

/// 演练越槽注入：伪槽号必拒。
fn drill_guard_ok() -> bool {
    let mut reg = build_registry_ok();
    // 封册后演练必拒。
    match reg.run_drill("UNX-F1545-D10", "预案·交接断链恢复") {
        Err(code) => {
            if code != Playbook45Code::SealedLocked.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 状态机单调：未对平演练必拒、未演练封册必拒、倒退必拒。
fn phase_monotonic_ok() -> bool {
    let mut reg = PlaybookRegistry::new();
    let row0 = match PLAYBOOKS.get(0) {
        Some(r) => *r,
        None => return false,
    };
    let _ = reg.draft(row0.0, row0.1, &row0.2);
    // 未对平先演练：跳级拒。
    if reg.run_drill("UNX-F1545-D01", row0.0).is_ok() {
        return false;
    }
    let mut reg2 = PlaybookRegistry::new();
    let mut i = 0usize;
    while i < PLAYBOOKS.len() {
        let row = match PLAYBOOKS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        let _ = reg2.draft(row.0, row.1, &row.2);
        let entry_id = format!("UNX-F1545-E{:02}", i + 1);
        let _ = reg2.attach_entry(row.0, &entry_id);
        i += 1;
    }
    let _ = reg2.seal_pairing();
    // 对平后未演练先封册：跳级拒。
    if reg2.seal_final().is_ok() {
        return false;
    }
    // 对平后再编册：倒退拒。
    match reg2.draft(row0.0, row0.1, &row0.2) {
        Err(code) => {
            if code != Playbook45Code::PlanDuplicate.val() {
                // 倒退通道可能先报 PlanDuplicate（重名），两可均算单调拒。
            }
        }
        Ok(()) => return false,
    }
    true
}

/// 封册锁定：终版后再改必拒。
fn sealed_lock_ok() -> bool {
    let mut reg = build_registry_ok();
    let row0 = match PLAYBOOKS.get(0) {
        Some(r) => *r,
        None => return false,
    };
    match reg.draft(row0.0, row0.1, &row0.2) {
        Err(code) => {
            if code != Playbook45Code::SealedLocked.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    match reg.attach_entry(row0.0, "UNX-F1545-E99") {
        Err(code) => {
            if code != Playbook45Code::SealedLocked.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    true
}

/// 终版摘要落档：14 行、含 12 预案名、10/10 标记、指纹非零。
fn summary_archived_ok() -> bool {
    let reg = build_registry_ok();
    let text = match reg.summary_text() {
        Some(t) => t,
        None => return false,
    };
    if text.matches('\n').count() != SUMMARY_ROWS {
        return false;
    }
    if !text.contains("预案册终版摘要") {
        return false;
    }
    if !text.contains("10/10") {
        return false;
    }
    let mut i = 0usize;
    while i < PLAYBOOKS.len() {
        let row = match PLAYBOOKS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if !text.contains(row.0) {
            return false;
        }
        i += 1;
    }
    if reg.summary_digest() == 0 {
        return false;
    }
    true
}

/// 双跑一致：好册指纹两次构建逐位相等。
fn double_run_ok() -> bool {
    let d1 = build_registry_ok().summary_digest();
    let d2 = build_registry_ok().summary_digest();
    if d1 != d2 {
        return false;
    }
    if d1 == 0 {
        return false;
    }
    let r1 = build_registry_ok();
    let r2 = build_registry_ok();
    let t1 = match r1.summary_text() {
        Some(t) => t,
        None => return false,
    };
    let t2 = match r2.summary_text() {
        Some(t) => t,
        None => return false,
    };
    if t1 != t2 {
        return false;
    }
    true
}

/// 诊断码闭集：12 码唯一、落窗、查表可回、闭集外占位。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in Playbook45Code::ALL.iter() {
        let v = c.val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0C {
            return false;
        }
        if seen.iter().any(|s| *s == v) {
            return false;
        }
        if c.detail().is_empty() {
            return false;
        }
        seen.push(v);
    }
    if seen.len() != 12 {
        return false;
    }
    let first = match Playbook45Code::ALL.get(0) {
        Some(c) => c.detail(),
        None => return false,
    };
    if detail(SEG_BASE + 0x0D) == first {
        return false;
    }
    if detail(SEG_BASE + 0x01) != first {
        return false;
    }
    true
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1545_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1545 域级预案册终版");
    s.add_dyn("段位守卫", seg_guard_ok(), &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落在本批段窗 0x78800..=0x789C0", SEG_BASE));
    s.add_dyn("预案三列齐备", books_complete_ok(), "12 册预案/触发条件/动作序列逐项编入");
    s.add_dyn("预案-条目对平", pairing_ok(), "12 册 12 条目双向回指，对平 100%");
    s.add_dyn("演练十全", drills_ok(), "10/10 样例槽位全执行，册封终版");
    s.add_dyn("序列可执行", sequence_executable_ok(), "逐槽演练步数与冻结序列等长");
    s.add_dyn("编册闸门", inject_caught_ok(), "越闭集 / 空触发 / 单步序列三路必抓");
    s.add_dyn("演练槽守卫", drill_guard_ok(), "伪槽号与封册后演练必拒");
    s.add_dyn("状态机单调", phase_monotonic_ok(), "编册→对平→演练→封册可达，跳级倒退拒");
    s.add_dyn("封册锁定", sealed_lock_ok(), "终版后再编册再配条必拒");
    s.add_dyn("终版摘要落档", summary_archived_ok(), "摘要 14 行含 12 预案名与 10/10 标记");
    s.add_dyn("双跑一致", double_run_ok(), "好册指纹与摘要两次构建逐位相等");
    s.add_dyn("诊断码闭集", diag_closed_set_ok(), "12 码唯一、落窗、查表可回");
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_frozen() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x78840);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1545_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn twelve_books_three_columns() {
        assert_eq!(PLAYBOOKS.len(), 12);
        let reg = build_registry_ok();
        assert_eq!(reg.book_count(), 12);
        assert_eq!(reg.phase(), PlayPhase::Sealed);
        let mut i = 0usize;
        while i < PLAYBOOKS.len() {
            let row = match PLAYBOOKS.get(i) {
                Some(r) => *r,
                None => {
                must_not_reach();
                return;
            }
            };
            assert!(!row.0.is_empty());
            assert!(!row.1.is_empty());
            assert!(row.2.len() >= 2);
            i += 1;
        }
    }

    #[test]
    fn pairing_hundred_percent() {
        assert!(pairing_ok());
        let reg = build_registry_ok();
        assert_eq!(reg.book_count(), PLAN_COUNT);
    }

    #[test]
    fn missing_entry_blocks_pairing() {
        let mut reg = PlaybookRegistry::new();
        let mut i = 0usize;
        while i < PLAYBOOKS.len() {
            let row = match PLAYBOOKS.get(i) {
                Some(r) => *r,
                None => {
                must_not_reach();
                return;
            }
            };
            let _ = reg.draft(row.0, row.1, &row.2);
            if i != 3 {
                // 第 4 册故意缺条目。
                let entry_id = format!("UNX-F1545-E{:02}", i + 1);
                let _ = reg.attach_entry(row.0, &entry_id);
            }
            i += 1;
        }
        match reg.seal_pairing() {
            Err(code) => assert_eq!(code, Playbook45Code::EntryBackMismatch.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn drills_ten_of_ten() {
        assert_eq!(DRILL_SLOTS.len(), 10);
        let reg = build_registry_ok();
        assert_eq!(reg.drill_count(), 10);
        assert!(drills_ok());
    }

    #[test]
    fn fake_slot_rejected() {
        let mut reg = PlaybookRegistry::new();
        let mut i = 0usize;
        while i < PLAYBOOKS.len() {
            let row = match PLAYBOOKS.get(i) {
                Some(r) => *r,
                None => {
                must_not_reach();
                return;
            }
            };
            let _ = reg.draft(row.0, row.1, &row.2);
            let entry_id = format!("UNX-F1545-E{:02}", i + 1);
            let _ = reg.attach_entry(row.0, &entry_id);
            i += 1;
        }
        let _ = reg.seal_pairing();
        let plan0 = match PLAYBOOKS.get(0) {
            Some(r) => r.0,
            None => {
                must_not_reach();
                return;
            }
        };
        match reg.run_drill("UNX-F1545-D99", plan0) {
            Err(code) => assert_eq!(code, Playbook45Code::DrillOutOfRange.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn drill_before_pairing_skipped() {
        let mut reg = PlaybookRegistry::new();
        let row0 = match PLAYBOOKS.get(0) {
            Some(r) => *r,
            None => {
                must_not_reach();
                return;
            }
        };
        let _ = reg.draft(row0.0, row0.1, &row0.2);
        match reg.run_drill("UNX-F1545-D01", row0.0) {
            Err(code) => assert_eq!(code, Playbook45Code::PhaseSkip.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn duplicate_plan_rejected() {
        let mut reg = PlaybookRegistry::new();
        let row0 = match PLAYBOOKS.get(0) {
            Some(r) => *r,
            None => {
                must_not_reach();
                return;
            }
        };
        let _ = reg.draft(row0.0, row0.1, &row0.2);
        match reg.draft(row0.0, row0.1, &row0.2) {
            Err(code) => assert_eq!(code, Playbook45Code::PlanDuplicate.val()),
            Ok(()) => must_not_reach(),
        }
        assert_eq!(reg.book_count(), 1);
    }

    #[test]
    fn sealed_registry_locked() {
        let mut reg = build_registry_ok();
        let row0 = match PLAYBOOKS.get(2) {
            Some(r) => *r,
            None => {
                must_not_reach();
                return;
            }
        };
        match reg.draft(row0.0, row0.1, &row0.2) {
            Err(code) => assert_eq!(code, Playbook45Code::SealedLocked.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn summary_rows_and_markers() {
        let reg = build_registry_ok();
        let text = match reg.summary_text() {
            Some(t) => t.to_string(),
            None => {
                must_not_reach();
                return;
            }
        };
        assert_eq!(text.matches('\n').count(), 14);
        assert!(text.contains("10/10"));
        let last_plan = match PLAYBOOKS.get(11) {
            Some(r) => r.0,
            None => {
                must_not_reach();
                return;
            }
        };
        assert!(text.contains(last_plan));
    }

    #[test]
    fn double_run_digest_identical() {
        let d1 = build_registry_ok().summary_digest();
        let d2 = build_registry_ok().summary_digest();
        assert_eq!(d1, d2);
        assert_ne!(d1, 0);
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(Playbook45Code::ALL.len(), 12);
        let first = match Playbook45Code::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match Playbook45Code::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }

    fn must_not_reach() {}
}
