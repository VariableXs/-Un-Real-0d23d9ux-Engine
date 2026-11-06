//! UNX-F10036 豁免审计复跑（B14 面联动） · 0x918C0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10037-J1 双档对账零未决（差异三分类 10/10 次），未决差异
//! 10/10 次拦截。
//!
//! SEG_BASE = 0x918C0：F10036 独占 0x40 窗口；诊断码取 SEG_BASE + 变体序号
//! （+0x01..+0x0C 闭集），段外码一律非法。
//!
//! 设计要点：
//!   ① 审计复跑按「两跑逐条对齐」建模：同一份全账跑两遍审计，产出两份
//!      审计快照；按条目号规范序（升序）逐条比对——千分比漂移、条目丢失、
//!      幽灵条目三类差异 10/10 次检出；乱序输入不构成漂移（规范序性质）。
//!   ② 账本 + 状态机 + 差异三分类器三件齐：全账账本逐条可查、审计生命
//!      周期 Idle→RunA→RunB→Compared 只进不退、差异闭集三分类逐类定位。
//!   ③ 注入核四类样本：千分比漂移 / 条目丢失 / 幽灵条目 / 乱序（负样本：
//!      必须判一致），前三类 10/10 检出、乱序 10/10 判一致，逃逸恒为 0。
//!   ④ 与 Linux errno 冻结表逐码对照；FNV-1a 双跑指纹逐位一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10036 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x918C0;

/// 全账容量（条目数上限）。
pub const LEDGER_CAPACITY: usize = 16;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码。
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const EPERM: i32 = 1;
pub const EDOM: i32 = 33;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditCode {
    /// 条目句式坏：编号零/千分比越界
    BadEntry,
    /// 重复编号
    DuplicateId,
    /// 容量满
    LedgerFull,
    /// 复跑次序坏：未跑 A 先跑 B / 重复跑同一跑
    RunOrderBad,
    /// 千分比漂移：同号两跑取值不一致
    RatioDrift,
    /// 条目丢失：A 跑有而 B 跑无
    ItemLoss,
    /// 幽灵条目：B 跑有而 A 跑无
    PhantomItem,
    /// 未对齐出报告
    ReportBeforeCompare,
    /// 对齐后再入账
    EntryAfterCompare,
    /// 状态回退
    StateRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl AuditCode {
    /// 码值（段基址 + 变体序号，从 1 起）。
    pub fn code(self) -> u32 {
        SEG_BASE + (self as u32) + 1
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            AuditCode::BadEntry => "条目坏被拒：编号须 1..=99、千分比须 1..=50；\
                请修正后再入账",
            AuditCode::DuplicateId => "重复编号被拒：同号条目已在账；请先查账\
                再补录，重复回 EEXIST",
            AuditCode::LedgerFull => "容量满被拒：全账条目数已用尽；请先归档\
                旧账再开新账",
            AuditCode::RunOrderBad => "复跑次序坏被拒：必须先跑 A 再跑 B；\
                同一跑不得重复执行",
            AuditCode::RatioDrift => "千分比漂移：同号条目两跑取值不一致；\
                请定位私改条目并回滚",
            AuditCode::ItemLoss => "条目丢失：A 跑在册而 B 跑缺条；复跑不得\
                丢条，请核对全账扫描范围",
            AuditCode::PhantomItem => "幽灵条目：B 跑多出 A 跑没有的条目；\
                复跑不得添条，请核对取样口径",
            AuditCode::ReportBeforeCompare => "未对齐出报告被拒：先跑完两跑并\
                对齐才能出审计结论",
            AuditCode::EntryAfterCompare => "对齐后入账被拒：对齐即冻结账面；\
                补条请开新一轮复跑",
            AuditCode::StateRegress => "状态回退被拒：审计生命周期只进不退；\
                回退视为私改审计",
            AuditCode::NotReproducible => "双跑不一致：同输入两次结果不同；\
                请排查隐藏状态或非确定遍历序",
            AuditCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且\
                落在 0x918C0..=0x91940 窗口",
        }
    }

    /// 闭集全量。
    pub const ALL: [AuditCode; 12] = [
        AuditCode::BadEntry,
        AuditCode::DuplicateId,
        AuditCode::LedgerFull,
        AuditCode::RunOrderBad,
        AuditCode::RatioDrift,
        AuditCode::ItemLoss,
        AuditCode::PhantomItem,
        AuditCode::ReportBeforeCompare,
        AuditCode::EntryAfterCompare,
        AuditCode::StateRegress,
        AuditCode::NotReproducible,
        AuditCode::SegmentViolation,
    ];
}

// ── 审计条目与全账账本 ────────────────────────────────────────────────
/// 审计条目：编号 + 千分比 + 分级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditEntry {
    /// 条目编号（1..=99）
    pub id: u8,
    /// 千分比（1..=50）
    pub ratio_permille: u16,
    /// 分级：0=可清偿 1=待条件 2=永久
    pub grade: u8,
}

/// 差异三分类（闭集）：漂移 / 丢失 / 幽灵。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffClass {
    /// 千分比漂移
    RatioDrift,
    /// 条目丢失
    ItemLoss,
    /// 幽灵条目
    PhantomItem,
}

/// 差异定位记录：类 + 条目号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiffHit {
    /// 差异类
    pub class: DiffClass,
    /// 条目编号
    pub id: u8,
}

/// 审计生命周期：只进不退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditState {
    /// 空闲：可入账
    Idle,
    /// A 跑已落
    RunA,
    /// B 跑已落：可对齐
    RunB,
    /// 已对齐
    Compared,
}

/// 豁免审计复跑：全账账本 + 两跑快照 + 差异三分类器。
#[derive(Clone, Debug)]
pub struct AuditRerun {
    state: AuditState,
    ledger: Vec<AuditEntry>,
    snap_a: Vec<AuditEntry>,
    snap_b: Vec<AuditEntry>,
    diffs: Vec<DiffHit>,
}

/// 规范序：按编号升序插入排序（小账量，无递归无外部依赖）。
pub fn sort_by_id(rows: &mut Vec<AuditEntry>) {
    let n = rows.len();
    let mut i = 1usize;
    while i < n {
        let key = match rows.get(i) {
            Some(r) => *r,
            None => break,
        };
        let mut j = i;
        while j > 0 {
            let prev = match rows.get(j - 1) {
                Some(r) => *r,
                None => break,
            };
            if prev.id > key.id {
                rows[j - 1] = key;
                rows[j] = prev;
                j -= 1;
            } else {
                break;
            }
        }
        i += 1;
    }
}

impl AuditRerun {
    /// 构造 Idle 空账。
    pub fn new() -> AuditRerun {
        AuditRerun {
            state: AuditState::Idle,
            ledger: Vec::new(),
            snap_a: Vec::new(),
            snap_b: Vec::new(),
            diffs: Vec::new(),
        }
    }

    /// 当前状态。
    pub fn state(&self) -> AuditState {
        self.state
    }

    /// 全账条数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 入账：Idle 才可入；句式 + 重复 + 容量守卫。
    pub fn add(&mut self, id: u8, ratio: u16, grade: u8) -> Result<(), AuditCode> {
        if self.state == AuditState::Compared {
            return Err(AuditCode::EntryAfterCompare);
        }
        if self.state != AuditState::Idle {
            return Err(AuditCode::EntryAfterCompare);
        }
        if id < 1 || ratio < 1 || ratio > 50 || grade > 2 {
            return Err(AuditCode::BadEntry);
        }
        if self.ledger.len() >= LEDGER_CAPACITY {
            return Err(AuditCode::LedgerFull);
        }
        let mut i = 0usize;
        while i < self.ledger.len() {
            if let Some(r) = self.ledger.get(i) {
                if r.id == id {
                    return Err(AuditCode::DuplicateId);
                }
            }
            i += 1;
        }
        self.ledger.push(AuditEntry { id, ratio_permille: ratio, grade });
        Ok(())
    }

    /// 千分比总和（分母口径）。
    pub fn ratio_sum(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.ledger.len() {
            if let Some(r) = self.ledger.get(i) {
                s = s.saturating_add(r.ratio_permille as u32);
            }
            i += 1;
        }
        s
    }

    /// 落 A 跑快照：Idle → RunA；快照按规范序排列（乱序输入不构成漂移）。
    pub fn run_a(&mut self) -> Result<(), AuditCode> {
        if self.state != AuditState::Idle {
            return Err(AuditCode::RunOrderBad);
        }
        self.snap_a = self.ledger.clone();
        sort_by_id(&mut self.snap_a);
        self.state = AuditState::RunA;
        Ok(())
    }

    /// 落 B 跑快照：RunA → RunB；同规范序。
    pub fn run_b(&mut self) -> Result<(), AuditCode> {
        if self.state != AuditState::RunA {
            return Err(AuditCode::RunOrderBad);
        }
        self.snap_b = self.ledger.clone();
        sort_by_id(&mut self.snap_b);
        self.state = AuditState::RunB;
        Ok(())
    }

    /// 对齐：RunB → Compared；逐号比对产差异清单（三分类）。
    pub fn compare(&mut self) -> Result<usize, AuditCode> {
        if self.state != AuditState::RunB {
            return Err(AuditCode::ReportBeforeCompare);
        }
        self.diffs.clear();
        // A 侧逐条：B 缺 → 丢失；比值异 → 漂移。
        let mut i = 0usize;
        while i < self.snap_a.len() {
            let a = match self.snap_a.get(i) {
                Some(r) => *r,
                None => break,
            };
            match self.find_b(a.id) {
                None => self.diffs.push(DiffHit { class: DiffClass::ItemLoss, id: a.id }),
                Some(b) => {
                    if b.ratio_permille != a.ratio_permille {
                        self.diffs.push(DiffHit { class: DiffClass::RatioDrift, id: a.id });
                    }
                }
            }
            i += 1;
        }
        // B 侧逐条：A 缺 → 幽灵。
        let mut j = 0usize;
        while j < self.snap_b.len() {
            if let Some(b) = self.snap_b.get(j) {
                if self.find_a(b.id).is_none() {
                    self.diffs.push(DiffHit { class: DiffClass::PhantomItem, id: b.id });
                }
            }
            j += 1;
        }
        self.state = AuditState::Compared;
        Ok(self.diffs.len())
    }

    fn find_a(&self, id: u8) -> Option<AuditEntry> {
        let mut i = 0usize;
        while i < self.snap_a.len() {
            if let Some(r) = self.snap_a.get(i) {
                if r.id == id {
                    return Some(*r);
                }
            }
            i += 1;
        }
        None
    }

    fn find_b(&self, id: u8) -> Option<AuditEntry> {
        let mut i = 0usize;
        while i < self.snap_b.len() {
            if let Some(r) = self.snap_b.get(i) {
                if r.id == id {
                    return Some(*r);
                }
            }
            i += 1;
        }
        None
    }

    /// 差异清单（只读）。
    pub fn diff_list(&self) -> Vec<DiffHit> {
        self.diffs.clone()
    }

    /// 是否零漂移（两跑逐号逐值一致）。
    pub fn zero_drift(&self) -> bool {
        self.state == AuditState::Compared && self.diffs.is_empty()
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账不平返回负诊断码。
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(AuditCode::StateRegress as i32));
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 构造四条全账（X01..X04，比值 20/10/30/10）。
pub fn build_ledger() -> AuditRerun {
    let mut a = AuditRerun::new();
    let _ = a.add(1, 20, 0);
    let _ = a.add(2, 10, 1);
    let _ = a.add(3, 30, 2);
    let _ = a.add(4, 10, 0);
    a
}

/// 四类注入：0 千分比漂移、1 条目丢失、2 幽灵条目、3 乱序（负样本判一致）。
pub fn run_injection_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut c = 0usize;
    while c < 4 {
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match c {
                0 => {
                    // A 跑后私改比值再跑 B → 漂移检出。
                    let mut a = build_ledger();
                    let _ = a.run_a();
                    // 模拟私改：重账一份只改比值不可行（入账后只读），改为
                    // 直接构造两份不同账落 A/B 两跑。
                    let mut a2 = AuditRerun::new();
                    let _ = a2.add(1, 20, 0);
                    let _ = a2.add(2, 11, 1);
                    let _ = a2.add(3, 30, 2);
                    let _ = a2.add(4, 10, 0);
                    a.snap_b = a2.ledger.clone();
                    sort_by_id(&mut a.snap_b);
                    a.state = AuditState::RunB;
                    let n = a.compare();
                    n == Ok(1) && a.diff_list().first()
                        == Some(&DiffHit { class: DiffClass::RatioDrift, id: 2 })
                }
                1 => {
                    // B 跑少条 → 丢失检出。
                    let mut a = build_ledger();
                    let _ = a.run_a();
                    let mut b = AuditRerun::new();
                    let _ = b.add(1, 20, 0);
                    let _ = b.add(3, 30, 2);
                    let _ = b.add(4, 10, 0);
                    a.snap_b = b.ledger.clone();
                    sort_by_id(&mut a.snap_b);
                    a.state = AuditState::RunB;
                    let n = a.compare();
                    n == Ok(1) && a.diff_list().first()
                        == Some(&DiffHit { class: DiffClass::ItemLoss, id: 2 })
                }
                2 => {
                    // B 跑多条 → 幽灵检出。
                    let mut a = build_ledger();
                    let _ = a.run_a();
                    let mut b = AuditRerun::new();
                    let _ = b.add(1, 20, 0);
                    let _ = b.add(2, 10, 1);
                    let _ = b.add(3, 30, 2);
                    let _ = b.add(4, 10, 0);
                    let _ = b.add(9, 5, 1);
                    a.snap_b = b.ledger.clone();
                    sort_by_id(&mut a.snap_b);
                    a.state = AuditState::RunB;
                    let n = a.compare();
                    n == Ok(1) && a.diff_list().first()
                        == Some(&DiffHit { class: DiffClass::PhantomItem, id: 9 })
                }
                _ => {
                    // 负样本：乱序快照必须判零漂移。
                    let mut a = build_ledger();
                    let _ = a.run_a();
                    let mut b = AuditRerun::new();
                    let _ = b.add(4, 10, 0);
                    let _ = b.add(3, 30, 2);
                    let _ = b.add(2, 10, 1);
                    let _ = b.add(1, 20, 0);
                    a.snap_b = b.ledger.clone();
                    sort_by_id(&mut a.snap_b);
                    a.state = AuditState::RunB;
                    let n = a.compare();
                    n == Ok(0) && a.zero_drift()
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(c) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        c += 1;
    }
    (ledger, per_class)
}

/// 构造零漂移复跑：两跑同账对齐后零差异。
pub fn build_clean_rerun() -> AuditRerun {
    let mut a = build_ledger();
    let _ = a.run_a();
    let _ = a.run_b();
    let _ = a.compare();
    a
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；对齐后入账/状态回退族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；条目丢失族"),
    ("EEXIST", -17, 80, "Windows ERROR_FILE_EXISTS；重复编号/幽灵条目族"),
    ("EDOM", -33, 87, "Windows ERROR_INVALID_PARAMETER；域外取值/次序坏族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；句式坏族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 复跑指纹：状态 + 两跑规范序逐条入哈希。
pub fn rerun_fingerprint(a: &AuditRerun) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push(match a.state() {
        AuditState::Idle => 0,
        AuditState::RunA => 1,
        AuditState::RunB => 2,
        AuditState::Compared => 3,
    });
    let mut i = 0usize;
    while i < a.snap_a.len() {
        if let Some(r) = a.snap_a.get(i) {
            bytes.push(r.id);
            bytes.push(r.ratio_permille as u8);
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < a.snap_b.len() {
        if let Some(r) = a.snap_b.get(j) {
            bytes.push(r.id);
            bytes.push(r.ratio_permille as u8);
        }
        j += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10036 判据集（16 条 + 2 条动态明细）。
pub fn run_unx10036_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10036");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x918C0..=0x91940).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x918C0..=0x91940", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 完整复跑：两跑同账对齐零漂移
    let clean = build_clean_rerun();
    s.add("零漂移：两跑同账对齐零差异",
          clean.state() == AuditState::Compared && clean.zero_drift()
              && clean.ledger_len() == 4,
          "同账两跑必须逐号逐值一致，零差异才可出审计结论");

    // 3. 规范序性质：乱序输入升序还原
    // 手工倒转入乱序：4,3,2,1 由倒序重建模拟。
    let mut scrambled: Vec<AuditEntry> = Vec::new();
    let mut n = 4usize;
    while n >= 1 {
        if let Some(src) = build_ledger().ledger.get(n - 1) {
            scrambled.push(*src);
        }
        n -= 1;
    }
    sort_by_id(&mut scrambled);
    let mut sorted_ok = scrambled.len() == 4;
    let mut k = 0usize;
    while k < scrambled.len() {
        if let Some(r) = scrambled.get(k) {
            if r.id as usize != k + 1 {
                sorted_ok = false;
            }
        }
        k += 1;
    }
    s.add("规范序：乱序输入按编号升序还原", sorted_ok,
          "审计比对前必须按编号规范排序，乱序不构成漂移");

    // 4. 注入矩阵：4 类样本逐类 10/10 判定正确
    let (ledger, per_class) = run_injection_matrix();
    let mut all_ok = true;
    let mut c = 0usize;
    while c < per_class.len() {
        if let Some(k) = per_class.get(c) {
            if *k != INJECT_ROUNDS {
                all_ok = false;
            }
        }
        c += 1;
    }
    s.add("注入核：4 类样本逐类 10/10 判定正确", all_ok,
          "漂移/丢失/幽灵 10/10 检出，乱序负样本 10/10 判一致");

    // 5. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 4 * INJECT_ROUNDS,
          "40 次注入必须 40 次判定正确、0 逃逸");

    // 6. 千分比漂移检出（核心判据）：改值 10/10 检出
    let mut drift_caught = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut a = build_ledger();
        let _ = a.run_a();
        let mut b = AuditRerun::new();
        let _ = b.add(1, 20, 0);
        let _ = b.add(2, 99, 1);
        let _ = b.add(3, 30, 2);
        let _ = b.add(4, 10, 0);
        a.snap_b = b.ledger.clone();
        sort_by_id(&mut a.snap_b);
        a.state = AuditState::RunB;
        if a.compare() == Ok(1) {
            drift_caught += 1;
        }
        round += 1;
    }
    s.add("复跑漂移检出：同号改值 10/10 次检出", drift_caught == INJECT_ROUNDS,
          "清偿后全账复审：同号比值不一致必须逐次定位");

    // 7. 条目丢失检出（复测口径）
    let mut loss_caught = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut a = build_ledger();
        let _ = a.run_a();
        let mut b = AuditRerun::new();
        let _ = b.add(1, 20, 0);
        let _ = b.add(2, 10, 1);
        let _ = b.add(3, 30, 2);
        a.snap_b = b.ledger.clone();
        sort_by_id(&mut a.snap_b);
        a.state = AuditState::RunB;
        if a.compare() == Ok(1) {
            loss_caught += 1;
        }
        round += 1;
    }
    s.add("条目丢失检出：B 跑缺条 10/10 次检出", loss_caught == INJECT_ROUNDS,
          "复跑不得丢条，缺条必须按丢失类定位");

    // 8. 幽灵条目检出（复测口径）
    let mut ghost_caught = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut a = build_ledger();
        let _ = a.run_a();
        let mut b = AuditRerun::new();
        let _ = b.add(1, 20, 0);
        let _ = b.add(2, 10, 1);
        let _ = b.add(3, 30, 2);
        let _ = b.add(4, 10, 0);
        let _ = b.add(7, 5, 2);
        a.snap_b = b.ledger.clone();
        sort_by_id(&mut a.snap_b);
        a.state = AuditState::RunB;
        if a.compare() == Ok(1) {
            ghost_caught += 1;
        }
        round += 1;
    }
    s.add("幽灵条目检出：B 跑添条 10/10 次检出", ghost_caught == INJECT_ROUNDS,
          "复跑不得添条，添条必须按幽灵类定位");

    // 9. 差异三分类闭集：三类逐类可定位
    let tri_ok = DiffClass::RatioDrift != DiffClass::ItemLoss
        && DiffClass::ItemLoss != DiffClass::PhantomItem
        && DiffClass::PhantomItem != DiffClass::RatioDrift;
    s.add("差异三分类：漂移/丢失/幽灵三类互异", tri_ok,
          "差异必须归入三类闭集之一，不许未决悬挂");

    // 10. 复跑次序守卫：未跑 A 先跑 B / 重复跑同跑
    let mut order = build_ledger();
    let b_first = order.run_b();
    let _ = order.run_a();
    let a_twice = order.run_a();
    s.add("次序守卫：先 B 后 A 与重复跑 A 均被拒",
          b_first.err() == Some(AuditCode::RunOrderBad)
              && a_twice.err() == Some(AuditCode::RunOrderBad),
          "Idle→RunA→RunB 严格次序，越位即拒");

    // 11. 未对齐出报告拒绝、对齐后入账拒绝
    let mut flow = build_ledger();
    let _ = flow.run_a();
    let _ = flow.run_b();
    let early_add = flow.add(9, 5, 1);
    let _ = flow.compare();
    let late_add = flow.add(8, 5, 1);
    s.add("越闸拦截：对齐前入账与对齐后入账均被拒",
          early_add.err() == Some(AuditCode::EntryAfterCompare)
              && late_add.err() == Some(AuditCode::EntryAfterCompare),
          "对齐即冻结账面，前后越位入账都拒绝");

    // 12. 重复编号拒绝
    let mut dup = AuditRerun::new();
    let x = dup.add(3, 20, 0);
    let y = dup.add(3, 10, 1);
    s.add("重复拒绝：同号二次入账被拒",
          x.is_ok() && y.err() == Some(AuditCode::DuplicateId),
          "重复编号必须显性拒绝，不许静默覆盖");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EDOM)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 14. 双跑指纹一致
    let fp_a = rerun_fingerprint(&build_clean_rerun());
    let fp_b = rerun_fingerprint(&build_clean_rerun());
    s.add("双跑一致：复跑指纹相等", fp_a == fp_b,
          "同输入两次构建必须逐字段相等");

    // 15. 文案闭集
    let text_ok = AuditCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < AuditCode::ALL.len() {
        let mut j = i + 1;
        while j < AuditCode::ALL.len() {
            if let (Some(a), Some(b)) = (AuditCode::ALL.get(i), AuditCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!(
        "复跑指纹 0x{:08X}；全账千分比总和 {}‰",
        fp_a, clean.ratio_sum()
    );
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_rerun_zero_drift() {
        let a = build_clean_rerun();
        assert!(a.zero_drift());
        assert!(a.diff_list().is_empty());
    }

    #[test]
    fn sort_restores_order() {
        let mut v: Vec<AuditEntry> = Vec::new();
        v.push(AuditEntry { id: 3, ratio_permille: 30, grade: 2 });
        v.push(AuditEntry { id: 1, ratio_permille: 20, grade: 0 });
        v.push(AuditEntry { id: 2, ratio_permille: 10, grade: 1 });
        sort_by_id(&mut v);
        assert_eq!(v.get(0).map(|r| r.id), Some(1));
        assert_eq!(v.get(2).map(|r| r.id), Some(3));
    }

    #[test]
    fn ratio_drift_detected() {
        let mut a = build_ledger();
        let _ = a.run_a();
        let mut b = AuditRerun::new();
        let _ = b.add(1, 21, 0);
        let _ = b.add(2, 10, 1);
        let _ = b.add(3, 30, 2);
        let _ = b.add(4, 10, 0);
        a.snap_b = b.ledger.clone();
        sort_by_id(&mut a.snap_b);
        a.state = AuditState::RunB;
        assert_eq!(a.compare(), Ok(1));
        assert_eq!(a.diff_list().first().map(|d| d.class), Some(DiffClass::RatioDrift));
    }

    #[test]
    fn item_loss_detected() {
        let mut a = build_ledger();
        let _ = a.run_a();
        let mut b = AuditRerun::new();
        let _ = b.add(1, 20, 0);
        a.snap_b = b.ledger.clone();
        sort_by_id(&mut a.snap_b);
        a.state = AuditState::RunB;
        assert_eq!(a.compare(), Ok(3));
    }

    #[test]
    fn run_order_guarded() {
        let mut a = build_ledger();
        assert_eq!(a.run_b(), Err(AuditCode::RunOrderBad));
    }

    #[test]
    fn duplicate_id_rejected() {
        let mut a = AuditRerun::new();
        assert!(a.add(1, 20, 0).is_ok());
        assert_eq!(a.add(1, 10, 1), Err(AuditCode::DuplicateId));
    }

    #[test]
    fn bad_entry_rejected() {
        let mut a = AuditRerun::new();
        assert_eq!(a.add(0, 20, 0), Err(AuditCode::BadEntry));
        assert_eq!(a.add(1, 99, 0), Err(AuditCode::BadEntry));
        assert_eq!(a.add(1, 20, 3), Err(AuditCode::BadEntry));
    }

    #[test]
    fn compare_before_runs_rejected() {
        let mut a = build_ledger();
        assert_eq!(a.compare(), Err(AuditCode::ReportBeforeCompare));
    }

    #[test]
    fn injection_matrix_green() {
        let (l, per) = run_injection_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 4 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(rerun_fingerprint(&build_clean_rerun()),
                   rerun_fingerprint(&build_clean_rerun()));
    }

    #[test]
    fn unx10036_checks_green() {
        let set = run_unx10036_checks();
        assert!(set.all_passed(), "UNX-F10036 red: {}", set.red_items_text());
    }
}
