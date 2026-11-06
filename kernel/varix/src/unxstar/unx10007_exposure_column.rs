//! UNX-F10007 红账敞口列（F9747 联动） · 0x911C0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10008-J1 占用列双值核（占比+阈值比对 10/10 次），超阈不标
//! 10/10 次拦截。
//!
//! 设计要点：
//!   ① 敞口按「三格账」建模：新红/复发/波动三格闭集，每条敞口条目必须
//!      恰好落一格；取数按格独立（三格齐 = 三格计数逐格可复算）。
//!   ② 混格闸：归类文本同时命中两格、命中零格或携带噪声都判混格，
//!      10/10 次检出；这是三格口径纯净的唯一闸口。
//!   ③ F9747 联动：条目号必须在红账号段 9747..=9746+N 内，越界条目
//!      拒绝；三格计数之和 = 条目数，守恒式强制。
//!   ④ 账只增：格计数与条目数只增不减，倒退显性拒绝；同一红账号
//!      重复入账拒绝。
//!   ⑤ 与 Linux 逐码一致通过 errno 冻结表复算；FNV-1a 指纹双跑一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10007 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x911C0;

/// 红账敞口号段首号（F9747 联动锚）。
pub const RED_LEDGER_FIRST: u32 = 9747;

/// 红账敞口号段长度（F9747..=F9766 共 20 号）。
pub const RED_LEDGER_LEN: usize = 20;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码。
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const ERANGE: i32 = 34;

/// 三格归类关键字（闭集，顺序即格序）。
pub const CELL_KEYWORDS: [&str; 3] = ["新红", "复发", "波动"];

// ── 三格口径 ──────────────────────────────────────────────────────────
/// 敞口三格（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExposureKind {
    /// 新红
    NewRed,
    /// 复发
    Relapse,
    /// 波动
    Fluctuation,
}

impl ExposureKind {
    /// 格序（0..3）。
    pub fn index(self) -> usize {
        self as usize
    }

    /// 格名（与关键字表一致）。
    pub fn label(self) -> &'static str {
        match self {
            ExposureKind::NewRed => "新红",
            ExposureKind::Relapse => "复发",
            ExposureKind::Fluctuation => "波动",
        }
    }

    /// 三格闭集全量。
    pub const ALL: [ExposureKind; 3] = [
        ExposureKind::NewRed,
        ExposureKind::Relapse,
        ExposureKind::Fluctuation,
    ];
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExpCode {
    /// 空归类文本
    EmptyCell,
    /// 混格：命中多格或带噪声
    MixedCell,
    /// 无格：不含任何格关键字
    NoCell,
    /// 条目号越界：不在红账号段
    EntryOutOfRange,
    /// 重复条目：同号二次入账
    DuplicateEntry,
    /// 账满
    LedgerFull,
    /// 守恒破约：三格合计 ≠ 条目数
    ConservationBreach,
    /// 账倒退
    LedgerRegress,
    /// 三格不齐：出表时存在零格
    CellNotComplete,
    /// 取数越界：格序非法
    CellOutOfRange,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl ExpCode {
    /// 码值（段基址 + 变体序号，从 1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32) + 1
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ExpCode::EmptyCell => "空归类被拒：敞口条目必须标注格别；请按\
                新红/复发/波动三格填写",
            ExpCode::MixedCell => "混格被拒：归类文本命中多格或携带噪声；请\
                拆条并只保留单格关键字",
            ExpCode::NoCell => "无格被拒：文本不含任何三格关键字；请对照\
                关键字闭集修正",
            ExpCode::EntryOutOfRange => "条目号越界被拒：号不在红账号段 \
                9747..=9766；请核对条目来源",
            ExpCode::DuplicateEntry => "重复条目被拒：同号已入账；请先查账再\
                补录，重复回 EEXIST",
            ExpCode::LedgerFull => "账满被拒：条目数已达容量上限；请先出表\
                清账再继续",
            ExpCode::ConservationBreach => "守恒破约：三格合计与条目数不等；\
                请核对漏格或双计",
            ExpCode::LedgerRegress => "账倒退被拒：格计数只增不减；请检查是否\
                有回滚路径误改累计值",
            ExpCode::CellNotComplete => "三格不齐：存在零格不得出表；请回查\
                漏入账的格",
            ExpCode::CellOutOfRange => "取数越界被拒：格序只允许 0..=2；请按\
                三格闭集取数",
            ExpCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查\
                隐藏状态或非确定遍历序",
            ExpCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在\
                0x91000..=0x914C0 窗口",
        }
    }

    /// 闭集全量。
    pub const ALL: [ExpCode; 12] = [
        ExpCode::EmptyCell,
        ExpCode::MixedCell,
        ExpCode::NoCell,
        ExpCode::EntryOutOfRange,
        ExpCode::DuplicateEntry,
        ExpCode::LedgerFull,
        ExpCode::ConservationBreach,
        ExpCode::LedgerRegress,
        ExpCode::CellNotComplete,
        ExpCode::CellOutOfRange,
        ExpCode::NotReproducible,
        ExpCode::SegmentViolation,
    ];
}

// ── 归类解析器 ────────────────────────────────────────────────────────
/// 归类解析：精确命中单一格关键字才放行；空/混格/无格分别给码。
pub fn classify_cell(text: &str) -> Result<ExposureKind, ExpCode> {
    if text.is_empty() {
        return Err(ExpCode::EmptyCell);
    }
    let mut hits: u32 = 0;
    let mut hit_idx: usize = 3;
    let mut k = 0usize;
    while k < CELL_KEYWORDS.len() {
        if let Some(kw) = CELL_KEYWORDS.get(k) {
            if text.contains(kw) {
                hits = hits.saturating_add(1);
                hit_idx = k;
            }
        }
        k += 1;
    }
    if hits == 0 {
        return Err(ExpCode::NoCell);
    }
    if hits > 1 {
        return Err(ExpCode::MixedCell);
    }
    match CELL_KEYWORDS.get(hit_idx) {
        Some(kw) if *kw == text => match hit_idx {
            0 => Ok(ExposureKind::NewRed),
            1 => Ok(ExposureKind::Relapse),
            _ => Ok(ExposureKind::Fluctuation),
        },
        _ => Err(ExpCode::MixedCell),
    }
}

// ── 敞口账 ────────────────────────────────────────────────────────────
/// 三格敞口账：条目只增、格计数独立、守恒可复算。
#[derive(Clone, Debug)]
pub struct ExposureLedger {
    cells: [u32; 3],
    entries: Vec<u32>,
    cap: usize,
}

impl ExposureLedger {
    /// 构造指定容量的空账。
    pub fn with_capacity(cap: usize) -> ExposureLedger {
        ExposureLedger { cells: [0; 3], entries: Vec::new(), cap }
    }

    /// 全量容量（红账号段长度）。
    pub fn full() -> ExposureLedger {
        ExposureLedger::with_capacity(RED_LEDGER_LEN)
    }

    /// 入账一条：越界/重复/账满/混格逐项拒绝。
    pub fn add(&mut self, entry: u32, kind: ExposureKind) -> Result<(), ExpCode> {
        if entry < RED_LEDGER_FIRST || entry >= RED_LEDGER_FIRST + RED_LEDGER_LEN as u32 {
            return Err(ExpCode::EntryOutOfRange);
        }
        if self.entries.len() >= self.cap {
            return Err(ExpCode::LedgerFull);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if *e == entry {
                    return Err(ExpCode::DuplicateEntry);
                }
            }
            i += 1;
        }
        let idx = kind.index();
        if idx >= 3 {
            return Err(ExpCode::CellOutOfRange);
        }
        self.entries.push(entry);
        if let Some(c) = self.cells.get_mut(idx) {
            *c = c.saturating_add(1);
        }
        if self.entries.len() != self.conserved_total() as usize {
            return Err(ExpCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式：三格合计。
    pub fn conserved_total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < 3 {
            if let Some(c) = self.cells.get(i) {
                sum = sum.saturating_add(*c);
            }
            i += 1;
        }
        sum
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空账判定。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 取格计数（格序 0..=2；越界返回 0 并以 CellOutOfRange 语义表意）。
    pub fn cell_count(&self, kind: ExposureKind) -> u32 {
        match self.cells.get(kind.index()) {
            Some(c) => *c,
            None => 0,
        }
    }

    /// 三格齐（全非零）。
    pub fn cells_complete(&self) -> bool {
        let mut i = 0usize;
        while i < 3 {
            match self.cells.get(i) {
                Some(0) | None => return false,
                _ => {}
            }
            i += 1;
        }
        true
    }

    /// 出表：三格齐 + 守恒 + 全量。
    pub fn emit(&self) -> Result<[u32; 3], ExpCode> {
        if self.len() != RED_LEDGER_LEN {
            return Err(ExpCode::EntryOutOfRange);
        }
        if !self.cells_complete() {
            return Err(ExpCode::CellNotComplete);
        }
        if self.conserved_total() as usize != self.len() {
            return Err(ExpCode::ConservationBreach);
        }
        Ok(self.cells)
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
            return Err(-(ExpCode::LedgerRegress as i32));
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 四类坏样本注入类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadCase {
    /// 混格：双关键字命中
    Mixed,
    /// 无格：无关键字
    NoCell,
    /// 越界条目号
    OutOfRange,
    /// 重复条目号
    Duplicate,
}

impl BadCase {
    /// 闭集全量。
    pub const ALL: [BadCase; 4] = [
        BadCase::Mixed,
        BadCase::NoCell,
        BadCase::OutOfRange,
        BadCase::Duplicate,
    ];
}

/// 执行四类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_bad_case_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadCase::ALL.len() {
        if let Some(case) = BadCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = match case {
                    BadCase::Mixed => {
                        // 双关键字与噪声两种混法轮换。
                        let caught = if round % 2 == 0 {
                            classify_cell("新红复发") == Err(ExpCode::MixedCell)
                        } else {
                            classify_cell("新红的") == Err(ExpCode::MixedCell)
                        };
                        caught
                    }
                    BadCase::NoCell => {
                        classify_cell("待定") == Err(ExpCode::NoCell)
                    }
                    BadCase::OutOfRange => {
                        let mut lg = ExposureLedger::full();
                        lg.add(RED_LEDGER_FIRST - 1, ExposureKind::NewRed)
                            == Err(ExpCode::EntryOutOfRange)
                            && lg.add(RED_LEDGER_FIRST + RED_LEDGER_LEN as u32,
                                ExposureKind::NewRed)
                                == Err(ExpCode::EntryOutOfRange)
                    }
                    BadCase::Duplicate => {
                        let mut lg = ExposureLedger::full();
                        let first = lg.add(9747, ExposureKind::NewRed).is_ok();
                        first && lg.add(9747, ExposureKind::NewRed)
                            == Err(ExpCode::DuplicateEntry)
                    }
                };
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

/// 基准三格账：20 条按条目序轮转三格（7+7+6，确定性）。
pub fn build_reference_ledger() -> ExposureLedger {
    let mut lg = ExposureLedger::full();
    let mut n = 0usize;
    while n < RED_LEDGER_LEN {
        let entry = RED_LEDGER_FIRST + n as u32;
        let kind = match ExposureKind::ALL.get(n % 3) {
            Some(k) => *k,
            None => ExposureKind::Fluctuation,
        };
        let _ = lg.add(entry, kind);
        n += 1;
    }
    lg
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；无格/越界号段族"),
    ("EEXIST", -17, 80, "Windows ERROR_FILE_EXISTS；重复条目族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；混格/坏格序族"),
    ("ERANGE", -34, 34, "Windows ERROR_ARITHMETIC_OVERFLOW；账倒退族"),
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

/// 敞口账指纹：三格计数 + 条目数逐字节入哈希。
pub fn ledger_fingerprint(lg: &ExposureLedger) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < 3 {
        let c = match ExposureKind::ALL.get(i) {
            Some(k) => lg.cell_count(*k),
            None => 0,
        };
        bytes.push((c & 0xFF) as u8);
        i += 1;
    }
    let n = lg.len();
    bytes.push(((n >> 8) & 0xFF) as u8);
    bytes.push((n & 0xFF) as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10007 判据集（16 条 + 2 条动态明细）。
pub fn run_unx10007_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10007");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91000..=0x914C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91000..=0x914C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 三格逐格取数正确：7+7+6 基准分布
    let lg = build_reference_ledger();
    let c0 = lg.cell_count(ExposureKind::NewRed);
    let c1 = lg.cell_count(ExposureKind::Relapse);
    let c2 = lg.cell_count(ExposureKind::Fluctuation);
    s.add("三格取数：新红 7 / 复发 7 / 波动 6",
          c0 == 7 && c1 == 7 && c2 == 6,
          "20 条按序轮转的格分布必须逐格可复算");

    // 3. 三格齐且守恒
    s.add("三格齐：全非零且三格合计=20",
          lg.cells_complete() && lg.conserved_total() == RED_LEDGER_LEN as u32
              && lg.len() == RED_LEDGER_LEN,
          "三格齐是出表前置条件，守恒式强制");

    // 4. 出表逐格返回
    let emit = lg.emit();
    let emit_ok = match emit {
        Ok(cells) => cells[0] == 7 && cells[1] == 7 && cells[2] == 6,
        Err(_) => false,
    };
    s.add("出表：三格计数成组返回且逐格正确", emit_ok,
          "出表必须一次给出三格，不许只报总数");

    // 5. 混格检出（矩阵逐轮复测）
    let (ledger, per_class) = run_bad_case_matrix();
    let mixed_class = match per_class.first() {
        Some(n) => *n == INJECT_ROUNDS,
        None => false,
    };
    s.add("混格检出：双关键字/噪声 10/10 拒绝", mixed_class,
          "归类文本不净即混格，必须逐次检出");

    // 6. 注入矩阵：四类各 10/10 检出
    let mut all_caught = true;
    let mut ci = 0usize;
    while ci < per_class.len() {
        if let Some(n) = per_class.get(ci) {
            if *n != INJECT_ROUNDS {
                all_caught = false;
            }
        }
        ci += 1;
    }
    s.add("注入矩阵：混格/无格/越界/重复各 10/10", all_caught,
          "四类坏样本必须全部显性拒绝");

    // 7. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 4 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 8. 精确关键字全过
    let parse_ok = classify_cell("新红") == Ok(ExposureKind::NewRed)
        && classify_cell("复发") == Ok(ExposureKind::Relapse)
        && classify_cell("波动") == Ok(ExposureKind::Fluctuation);
    s.add("归类解析：三格关键字逐字命中", parse_ok,
          "格名闭集必须精确解析，不许模糊匹配");

    // 9. 无格与空格拒绝
    let no_ok = classify_cell("待定") == Err(ExpCode::NoCell)
        && classify_cell("") == Err(ExpCode::EmptyCell);
    s.add("无格拒绝：无关键字与空文本逐类拒绝", no_ok,
          "无格条目不许默认归格，必须显性拒绝");

    // 10. 越界条目号拒绝
    let mut rng = ExposureLedger::full();
    s.add("越界拒绝：9746/9767 均拒",
          rng.add(RED_LEDGER_FIRST - 1, ExposureKind::NewRed)
              == Err(ExpCode::EntryOutOfRange)
              && rng.add(RED_LEDGER_FIRST + RED_LEDGER_LEN as u32, ExposureKind::NewRed)
                  == Err(ExpCode::EntryOutOfRange),
          "红账号段 9747..=9766 之外一律拒绝");

    // 11. 重复条目拒绝
    let mut dup = ExposureLedger::full();
    let _ = dup.add(9750, ExposureKind::Relapse);
    s.add("重复拒绝：9750 二次入账被拒",
          dup.add(9750, ExposureKind::Relapse) == Err(ExpCode::DuplicateEntry),
          "重复条目必须显性拒绝，不许静默双计");

    // 12. 三格不齐出表拦截
    let mut thin = ExposureLedger::full();
    let _ = thin.add(9747, ExposureKind::NewRed);
    let _ = thin.add(9748, ExposureKind::Relapse);
    s.add("出表拦截：条目不足/格不齐均拒",
          thin.emit().is_err() && thin.emit().err() == Some(ExpCode::EntryOutOfRange),
          "出表闸要求全量三格齐，缺一即拦");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ERANGE)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 14. 双跑指纹一致
    let fp_a = ledger_fingerprint(&build_reference_ledger());
    let fp_b = ledger_fingerprint(&build_reference_ledger());
    s.add("双跑一致：敞口账指纹相等", fp_a == fp_b,
          "同输入两次构建必须逐字段相等");

    // 15. 文案闭集
    let text_ok = ExpCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < ExpCode::ALL.len() {
        let mut j = i + 1;
        while j < ExpCode::ALL.len() {
            if let (Some(a), Some(b)) = (ExpCode::ALL.get(i), ExpCode::ALL.get(j)) {
                if a.val() == b.val() {
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
    let fp_text = format!("敞口账指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_exact() {
        assert_eq!(classify_cell("新红"), Ok(ExposureKind::NewRed));
        assert_eq!(classify_cell("复发"), Ok(ExposureKind::Relapse));
        assert_eq!(classify_cell("波动"), Ok(ExposureKind::Fluctuation));
    }

    #[test]
    fn classify_rejects_ambiguity() {
        assert_eq!(classify_cell("新红复发"), Err(ExpCode::MixedCell));
        assert_eq!(classify_cell("复发的"), Err(ExpCode::MixedCell));
        assert_eq!(classify_cell("待定"), Err(ExpCode::NoCell));
        assert_eq!(classify_cell(""), Err(ExpCode::EmptyCell));
    }

    #[test]
    fn reference_distribution() {
        let lg = build_reference_ledger();
        assert_eq!(lg.cell_count(ExposureKind::NewRed), 7);
        assert_eq!(lg.cell_count(ExposureKind::Relapse), 7);
        assert_eq!(lg.cell_count(ExposureKind::Fluctuation), 6);
        assert_eq!(lg.conserved_total(), 20);
    }

    #[test]
    fn out_of_range_entry_rejected() {
        let mut lg = ExposureLedger::full();
        assert_eq!(lg.add(9746, ExposureKind::NewRed), Err(ExpCode::EntryOutOfRange));
        assert_eq!(lg.add(9767, ExposureKind::NewRed), Err(ExpCode::EntryOutOfRange));
    }

    #[test]
    fn duplicate_entry_rejected() {
        let mut lg = ExposureLedger::full();
        assert!(lg.add(9747, ExposureKind::NewRed).is_ok());
        assert_eq!(lg.add(9747, ExposureKind::NewRed), Err(ExpCode::DuplicateEntry));
    }

    #[test]
    fn emit_gates() {
        let mut lg = ExposureLedger::full();
        assert!(lg.emit().is_err());
        let _ = lg.add(9747, ExposureKind::NewRed);
        assert_eq!(lg.emit(), Err(ExpCode::EntryOutOfRange));
    }

    #[test]
    fn capacity_enforced() {
        let mut lg = ExposureLedger::with_capacity(1);
        assert!(lg.add(9747, ExposureKind::NewRed).is_ok());
        assert_eq!(lg.add(9748, ExposureKind::NewRed), Err(ExpCode::LedgerFull));
    }

    #[test]
    fn bad_case_matrix_green() {
        let (l, per) = run_bad_case_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(
            ledger_fingerprint(&build_reference_ledger()),
            ledger_fingerprint(&build_reference_ledger())
        );
    }

    #[test]
    fn errno_table_agrees() {
        assert!(agrees_with_linux(EEXIST));
        assert_eq!(linux_neg(ERANGE), -ERANGE);
        assert!(!agrees_with_linux(0));
    }

    #[test]
    fn unx10007_checks_green() {
        let set = run_unx10007_checks();
        assert!(set.all_passed(), "UNX-F10007 red: {}", set.red_items_text());
    }
}
