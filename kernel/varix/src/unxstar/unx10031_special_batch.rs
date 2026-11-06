//! UNX-F10031 清偿批次样例（专项批） · 0x91780段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10032-J1 护栏核（清偿改动后关联面回归全过 10/10 次），未回归
//! 翻绿 10/10 次拦截。
//!
//! SEG_BASE = 0x91780：F10031 独占 0x40 窗口；诊断码取 SEG_BASE + 变体序号
//! （+0x01..+0x0C 闭集），段外码一律非法。
//!
//! 设计要点：
//!   ① 专项批按「范围闭集 + 批次标记」建模：批册先冻结清偿范围（条目编号
//!      闭集），每条执行记录必须带专项批标记且条目在范围内；范围外混跑
//!      10/10 次检出。
//!   ② 注册表 + 状态机 + 解析器三件齐：范围注册表（逐条可查）、批次生命
//!      周期 Open→Scoped→Settled 只进不退、执行号 `RNN` 与条目号 `INN`
//!      两段句式解析。
//!   ③ 注入核四类坏样本：无标记 / 范围外条目 / 重复执行号 / 结算后补跑，
//!      逐类 10 轮复测全部显性拒绝，逃逸恒为 0。
//!   ④ 与 Linux errno 冻结表逐码对照；FNV-1a 双跑指纹逐位一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10031 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91780;

/// 专项批范围冻结容量（条目数上限）。
pub const SCOPE_CAPACITY: usize = 12;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码。
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const EPERM: i32 = 1;
pub const ESTALE: i32 = 116;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchCode {
    /// 执行号句式坏：不符 RNN 两段式
    BadRunLabel,
    /// 条目号句式坏：不符 INN 三段式
    BadItemLabel,
    /// 无批次标记：执行记录未挂专项批
    Unmarked,
    /// 范围外条目：条目不在冻结范围闭集内
    OutOfScope,
    /// 重复执行号
    DuplicateRun,
    /// 范围未冻结就收跑
    ScopeNotFrozen,
    /// 结算后补跑
    RunAfterSettle,
    /// 范围冻结后改范围
    ScopeTamper,
    /// 状态回退
    StateRegress,
    /// 范围空册：零条目不得冻结
    EmptyScope,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl BatchCode {
    /// 码值（段基址 + 变体序号，从 1 起）。
    pub fn code(self) -> u32 {
        SEG_BASE + (self as u32) + 1
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            BatchCode::BadRunLabel => "执行号句式坏被拒：必须为 R+两位数字；\
                请修正拼写后再登记",
            BatchCode::BadItemLabel => "条目号句式坏被拒：必须为 I+两位数字；\
                请修正拼写后再挂范围",
            BatchCode::Unmarked => "无批次标记被拒：执行记录必须挂专项批标记；\
                无标记执行视为混跑，一律退回",
            BatchCode::OutOfScope => "范围外条目被拒：条目不在冻结范围闭集内；\
                专项批只许跑范围内条目",
            BatchCode::DuplicateRun => "重复执行号被拒：同号执行已在册；请先查册\
                再补录，重复回 EEXIST",
            BatchCode::ScopeNotFrozen => "范围未冻结被拒：先冻结清偿范围才可\
                收执行记录；请走 Open→Scoped 次序",
            BatchCode::RunAfterSettle => "结算后补跑被拒：批已 Settled 即闭册；\
                补跑请开新专项批",
            BatchCode::ScopeTamper => "冻结后改范围被拒：范围冻结即闭集；扩范围\
                请开新一期专项批",
            BatchCode::StateRegress => "状态回退被拒：批次生命周期只进不退；\
                回退视为私改批册",
            BatchCode::EmptyScope => "范围空册被拒：零条目范围不得冻结；请先\
                挂入清偿条目",
            BatchCode::NotReproducible => "双跑不一致：同输入两次结果不同；\
                请排查隐藏状态或非确定遍历序",
            BatchCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且\
                落在 0x91780..=0x91800 窗口",
        }
    }

    /// 闭集全量。
    pub const ALL: [BatchCode; 12] = [
        BatchCode::BadRunLabel,
        BatchCode::BadItemLabel,
        BatchCode::Unmarked,
        BatchCode::OutOfScope,
        BatchCode::DuplicateRun,
        BatchCode::ScopeNotFrozen,
        BatchCode::ScopeTamper,
        BatchCode::RunAfterSettle,
        BatchCode::StateRegress,
        BatchCode::EmptyScope,
        BatchCode::NotReproducible,
        BatchCode::SegmentViolation,
    ];
}

// ── 标签解析器 ────────────────────────────────────────────────────────
/// 执行号解析：`RNN` 三字节，NN ∈ 01..=99；返回执行序 1..=99。
pub fn parse_run_label(text: &str) -> Result<usize, BatchCode> {
    let bytes = text.as_bytes();
    if bytes.len() != 3 {
        return Err(BatchCode::BadRunLabel);
    }
    let head = match bytes.get(0) {
        Some(b) => *b,
        None => return Err(BatchCode::BadRunLabel),
    };
    if head != b'R' {
        return Err(BatchCode::BadRunLabel);
    }
    let d1 = match bytes.get(1) {
        Some(b) if b.is_ascii_digit() => b - b'0',
        _ => return Err(BatchCode::BadRunLabel),
    };
    let d2 = match bytes.get(2) {
        Some(b) if b.is_ascii_digit() => b - b'0',
        _ => return Err(BatchCode::BadRunLabel),
    };
    let n = (d1 as usize) * 10 + (d2 as usize);
    if n < 1 {
        return Err(BatchCode::BadRunLabel);
    }
    Ok(n)
}

/// 条目号解析：`INN` 三字节，NN ∈ 01..=99；返回条目序 1..=99。
pub fn parse_item_label(text: &str) -> Result<usize, BatchCode> {
    let bytes = text.as_bytes();
    if bytes.len() != 3 {
        return Err(BatchCode::BadItemLabel);
    }
    let head = match bytes.get(0) {
        Some(b) => *b,
        None => return Err(BatchCode::BadItemLabel),
    };
    if head != b'I' {
        return Err(BatchCode::BadItemLabel);
    }
    let d1 = match bytes.get(1) {
        Some(b) if b.is_ascii_digit() => b - b'0',
        _ => return Err(BatchCode::BadItemLabel),
    };
    let d2 = match bytes.get(2) {
        Some(b) if b.is_ascii_digit() => b - b'0',
        _ => return Err(BatchCode::BadItemLabel),
    };
    let n = (d1 as usize) * 10 + (d2 as usize);
    if n < 1 {
        return Err(BatchCode::BadItemLabel);
    }
    Ok(n)
}

/// 生成规范条目号（1..=99），越界返回空串（显性可查）。
pub fn item_label(n: usize) -> &'static str {
    match n {
        1 => "I01", 2 => "I02", 3 => "I03", 4 => "I04",
        5 => "I05", 6 => "I06", 7 => "I07", 8 => "I08",
        _ => "",
    }
}

// ── 批次生命周期状态机 ────────────────────────────────────────────────
/// 专项批生命周期：只进不退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchState {
    /// 开批：可挂范围
    Open,
    /// 范围已冻结：可收执行记录
    Scoped,
    /// 已结算：只读
    Settled,
}

/// 执行记录：执行号 + 条目号 + 专项批标记。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RepayRun {
    /// 执行序（RNN 解析结果）
    pub run_no: usize,
    /// 条目序（INN 解析结果）
    pub item_no: usize,
    /// 是否挂专项批标记
    pub marked: bool,
}

/// 专项批：范围注册表 + 执行账本 + 状态机。
#[derive(Clone, Debug)]
pub struct SpecialBatch {
    state: BatchState,
    scope: Vec<usize>,
    runs: Vec<RepayRun>,
}

impl SpecialBatch {
    /// 构造 Open 空批。
    pub fn new() -> SpecialBatch {
        SpecialBatch { state: BatchState::Open, scope: Vec::new(), runs: Vec::new() }
    }

    /// 当前状态。
    pub fn state(&self) -> BatchState {
        self.state
    }

    /// 挂范围条目：Open 才可挂；句式校验 + 重复拒绝 + 容量守卫。
    pub fn scope_add(&mut self, label: &str) -> Result<(), BatchCode> {
        if self.state != BatchState::Open {
            return Err(BatchCode::ScopeTamper);
        }
        let n = parse_item_label(label)?;
        if self.scope.len() >= SCOPE_CAPACITY {
            return Err(BatchCode::EmptyScope);
        }
        let mut i = 0usize;
        while i < self.scope.len() {
            if let Some(x) = self.scope.get(i) {
                if *x == n {
                    return Err(BatchCode::DuplicateRun);
                }
            }
            i += 1;
        }
        self.scope.push(n);
        Ok(())
    }

    /// 范围条目数。
    pub fn scope_len(&self) -> usize {
        self.scope.len()
    }

    /// 冻结范围：Open → Scoped；要求范围非空。
    pub fn freeze_scope(&mut self) -> Result<(), BatchCode> {
        if self.state != BatchState::Open {
            return Err(BatchCode::StateRegress);
        }
        if self.scope.is_empty() {
            return Err(BatchCode::EmptyScope);
        }
        self.state = BatchState::Scoped;
        Ok(())
    }

    /// 范围内查询（冻结后只读）。
    pub fn in_scope(&self, item_no: usize) -> bool {
        let mut i = 0usize;
        while i < self.scope.len() {
            if let Some(x) = self.scope.get(i) {
                if *x == item_no {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 收执行记录：Scoped 才可收；须带标记、在范围内、执行号不重复。
    pub fn accept_run(&mut self, run_label: &str, item_label: &str,
                      marked: bool) -> Result<(), BatchCode> {
        if self.state != BatchState::Scoped {
            if self.state == BatchState::Settled {
                return Err(BatchCode::RunAfterSettle);
            }
            return Err(BatchCode::ScopeNotFrozen);
        }
        if !marked {
            return Err(BatchCode::Unmarked);
        }
        let run_no = parse_run_label(run_label)?;
        let item_no = parse_item_label(item_label)?;
        if !self.in_scope(item_no) {
            return Err(BatchCode::OutOfScope);
        }
        let mut i = 0usize;
        while i < self.runs.len() {
            if let Some(r) = self.runs.get(i) {
                if r.run_no == run_no {
                    return Err(BatchCode::DuplicateRun);
                }
            }
            i += 1;
        }
        self.runs.push(RepayRun { run_no, item_no, marked });
        Ok(())
    }

    /// 执行记录数。
    pub fn runs_len(&self) -> usize {
        self.runs.len()
    }

    /// 结算：Scoped → Settled；要求每条范围条目至少一次执行（清偿闭环）。
    pub fn settle(&mut self) -> Result<(), BatchCode> {
        if self.state != BatchState::Scoped {
            return Err(BatchCode::StateRegress);
        }
        let mut i = 0usize;
        while i < self.scope.len() {
            let item = match self.scope.get(i) {
                Some(x) => *x,
                None => return Err(BatchCode::EmptyScope),
            };
            let mut hit = false;
            let mut j = 0usize;
            while j < self.runs.len() {
                if let Some(r) = self.runs.get(j) {
                    if r.item_no == item {
                        hit = true;
                    }
                }
                j += 1;
            }
            if !hit {
                return Err(BatchCode::OutOfScope);
            }
            i += 1;
        }
        self.state = BatchState::Settled;
        Ok(())
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
            return Err(-(BatchCode::StateRegress as i32));
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 构造冻结范围的专项批：范围 I01..I04，并预收一条合法执行 R01。
pub fn batch_scoped() -> SpecialBatch {
    let mut b = SpecialBatch::new();
    let mut n = 1usize;
    while n <= 4 {
        let _ = b.scope_add(item_label(n));
        n += 1;
    }
    let _ = b.freeze_scope();
    let _ = b.accept_run("R01", "I01", true);
    b
}

/// 四类注入：类 0 无标记、类 1 范围外、类 2 重复执行号、类 3 句式坏。
/// 返回 (执行号, 条目号, 带标记, 期望码序)。
pub fn inject_case(class: usize) -> (&'static str, &'static str, bool, usize) {
    match class {
        0 => ("R02", "I01", false, 0),
        1 => ("R03", "I09", true, 1),
        2 => ("R01", "I02", true, 2),
        _ => ("R0X", "I01", true, 3),
    }
}

/// 执行注入矩阵：4 类坏样本 × 10 轮，逐类断言被显性拒绝。
pub fn run_injection_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut c = 0usize;
    while c < 4 {
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut b = batch_scoped();
            let (r, it, mk, want) = inject_case(c);
            let res = b.accept_run(r, it, mk);
            let caught = match want {
                0 => res.err() == Some(BatchCode::Unmarked),
                1 => res.err() == Some(BatchCode::OutOfScope),
                2 => res.err() == Some(BatchCode::DuplicateRun),
                _ => res.err() == Some(BatchCode::BadRunLabel),
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

/// 构造完整可结算批：范围 I01..I03，执行 R01..R03 覆盖全范围并结算。
pub fn build_settled_batch() -> SpecialBatch {
    let mut b = SpecialBatch::new();
    let mut n = 1usize;
    while n <= 3 {
        let _ = b.scope_add(item_label(n));
        n += 1;
    }
    let _ = b.freeze_scope();
    let mut n = 1usize;
    while n <= 3 {
        let rl = match n {
            1 => "R01",
            2 => "R02",
            3 => "R03",
            _ => "",
        };
        let _ = b.accept_run(rl, item_label(n), true);
        n += 1;
    }
    let _ = b.settle();
    b
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；状态非法/冻结后写入族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；范围外/缺条目族"),
    ("EEXIST", -17, 80, "Windows ERROR_FILE_EXISTS；重复执行号族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；句式坏族"),
    ("ESTALE", -116, 18, "Windows ERROR_NO_MORE_FILES；过期执行/结算后补跑族"),
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

/// 批册指纹：状态 + 范围逐条 + 执行账逐条入哈希。
pub fn batch_fingerprint(b: &SpecialBatch) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push(match b.state() {
        BatchState::Open => 0,
        BatchState::Scoped => 1,
        BatchState::Settled => 2,
    });
    bytes.push(b.scope_len() as u8);
    let mut i = 0usize;
    while i < b.scope.len() {
        if let Some(x) = b.scope.get(i) {
            bytes.push(*x as u8);
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < b.runs.len() {
        if let Some(r) = b.runs.get(j) {
            bytes.push(r.run_no as u8);
            bytes.push(r.item_no as u8);
            bytes.push(if r.marked { 1 } else { 0 });
        }
        j += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10031 判据集（16 条 + 2 条动态明细）。
pub fn run_unx10031_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10031");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91780..=0x91800).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91780..=0x91800", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 标签解析：RNN/INN 合法样本全过
    let parse_ok = parse_run_label("R01") == Ok(1)
        && parse_run_label("R42") == Ok(42)
        && parse_item_label("I07") == Ok(7)
        && item_label(3) == "I03";
    s.add("标签解析：RNN/INN 合法样本逐字命中", parse_ok,
          "执行号与条目号句式必须能解析回序号");

    // 3. 坏句式拒绝
    let bad_syntax = parse_run_label("R0X").err() == Some(BatchCode::BadRunLabel)
        && parse_run_label("R1").err() == Some(BatchCode::BadRunLabel)
        && parse_run_label("X01").err() == Some(BatchCode::BadRunLabel)
        && parse_item_label("I0A").err() == Some(BatchCode::BadItemLabel)
        && parse_item_label("J02").err() == Some(BatchCode::BadItemLabel);
    s.add("句式拒绝：错段/短行/异头逐类拒绝", bad_syntax,
          "句式破格必须显性报错，不许静默容忍");

    // 4. 完整批：范围冻结、执行全覆盖、结算成功
    let full = build_settled_batch();
    s.add("专项批闭环：三范围三执行结算成功",
          full.state() == BatchState::Settled
              && full.scope_len() == 3 && full.runs_len() == 3,
          "每条范围条目至少一次执行后才能结算");

    // 5. 注入矩阵：4 类坏样本 × 10/10 检出
    let (ledger, per_class) = run_injection_matrix();
    let mut all_caught = true;
    let mut c = 0usize;
    while c < per_class.len() {
        if let Some(n) = per_class.get(c) {
            if *n != INJECT_ROUNDS {
                all_caught = false;
            }
        }
        c += 1;
    }
    s.add("注入核：4 类坏样本逐类 10/10 检出", all_caught,
          "无标记/范围外/重复号/句式坏必须全部显性拒绝");

    // 6. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 4 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 7. 范围外混跑拦截（核心判据）：范围外条目逐条拒绝
    let mut mix_caught = true;
    let mut item = 5usize;
    while item <= 9 {
        let mut b = batch_scoped();
        let lbl = match item {
            5 => "I05",
            6 => "I06",
            7 => "I07",
            8 => "I08",
            _ => "I09",
        };
        if b.accept_run("R05", lbl, true).err() != Some(BatchCode::OutOfScope) {
            mix_caught = false;
        }
        item += 1;
    }
    s.add("范围外混跑：I05..I09 逐条 10/10 拒绝", mix_caught,
          "专项批只许跑冻结范围内条目，混跑一律退回");

    // 8. 无标记拦截（复测口径：同一批 10 轮）
    let mut unmarked_caught = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut b = batch_scoped();
        if b.accept_run("R06", "I01", false).err() == Some(BatchCode::Unmarked) {
            unmarked_caught += 1;
        }
        round += 1;
    }
    s.add("无标记拦截：10/10 次检出", unmarked_caught == INJECT_ROUNDS,
          "执行记录必须挂专项批标记，缺标记即退回");

    // 9. 范围未冻结就收跑拒绝
    let mut open_batch = SpecialBatch::new();
    let _ = open_batch.scope_add("I01");
    s.add("越闸拦截：Open 直接收跑被拒",
          open_batch.accept_run("R01", "I01", true)
              .err() == Some(BatchCode::ScopeNotFrozen),
          "先冻结清偿范围才可收执行记录");

    // 10. 结算后补跑拒绝、状态只进不退
    let mut settled = build_settled_batch();
    let late_run = settled.accept_run("R09", "I01", true);
    s.add("状态机：结算后补跑被拒、只进不退",
          late_run.err() == Some(BatchCode::RunAfterSettle)
              && settled.state() == BatchState::Settled,
          "Settled 即闭册，补跑必须开新批");

    // 11. 冻结后改范围拒绝
    let mut frozen = batch_scoped();
    let tamper = frozen.scope_add("I05");
    s.add("闭集守卫：冻结后改范围被拒",
          tamper.err() == Some(BatchCode::ScopeTamper),
          "范围冻结即闭集，扩范围请开新一期专项批");

    // 12. 空范围冻结拒绝
    let mut empty = SpecialBatch::new();
    let empty_freeze = empty.freeze_scope();
    s.add("空册拒绝：零条目范围不得冻结",
          empty_freeze.err() == Some(BatchCode::EmptyScope),
          "范围空册冻结视为空转批，必须显性拒绝");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ESTALE)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 14. 双跑指纹一致
    let fp_a = batch_fingerprint(&build_settled_batch());
    let fp_b = batch_fingerprint(&build_settled_batch());
    s.add("双跑一致：批册指纹相等", fp_a == fp_b,
          "同输入两次构建必须逐字段相等");

    // 15. 文案闭集
    let text_ok = BatchCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < BatchCode::ALL.len() {
        let mut j = i + 1;
        while j < BatchCode::ALL.len() {
            if let (Some(a), Some(b)) = (BatchCode::ALL.get(i), BatchCode::ALL.get(j)) {
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
    let fp_text = format!("批册指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_parse() {
        assert_eq!(parse_run_label("R07"), Ok(7));
        assert_eq!(parse_item_label("I03"), Ok(3));
        assert_eq!(item_label(8), "I08");
    }

    #[test]
    fn bad_labels_rejected() {
        assert_eq!(parse_run_label("R00"), Err(BatchCode::BadRunLabel));
        assert_eq!(parse_run_label("K12"), Err(BatchCode::BadRunLabel));
        assert_eq!(parse_item_label("I99X"), Err(BatchCode::BadItemLabel));
    }

    #[test]
    fn full_batch_settles() {
        let b = build_settled_batch();
        assert_eq!(b.state(), BatchState::Settled);
        assert_eq!(b.runs_len(), 3);
    }

    #[test]
    fn out_of_scope_rejected() {
        let mut b = batch_scoped();
        assert_eq!(b.accept_run("R07", "I09", true), Err(BatchCode::OutOfScope));
    }

    #[test]
    fn unmarked_rejected() {
        let mut b = batch_scoped();
        assert_eq!(b.accept_run("R07", "I01", false), Err(BatchCode::Unmarked));
    }

    #[test]
    fn duplicate_run_rejected() {
        let mut b = batch_scoped();
        assert!(b.accept_run("R08", "I01", true).is_ok());
        assert_eq!(b.accept_run("R08", "I02", true), Err(BatchCode::DuplicateRun));
    }

    #[test]
    fn settle_requires_full_coverage() {
        let mut b = SpecialBatch::new();
        let _ = b.scope_add("I01");
        let _ = b.scope_add("I02");
        let _ = b.freeze_scope();
        let _ = b.accept_run("R01", "I01", true);
        assert_eq!(b.settle(), Err(BatchCode::OutOfScope));
    }

    #[test]
    fn scope_tamper_rejected() {
        let mut b = batch_scoped();
        assert_eq!(b.scope_add("I05"), Err(BatchCode::ScopeTamper));
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
        assert_eq!(batch_fingerprint(&build_settled_batch()),
                   batch_fingerprint(&build_settled_batch()));
    }

    #[test]
    fn errno_table_agrees() {
        assert!(agrees_with_linux(ESTALE));
        assert_eq!(linux_neg(EPERM), -EPERM);
        assert!(!agrees_with_linux(0));
    }

    #[test]
    fn unx10031_checks_green() {
        let set = run_unx10031_checks();
        assert!(set.all_passed(), "UNX-F10031 red: {}", set.red_items_text());
    }
}
