//! UNX-F10097 E 型五战线分配表（B26–B30 预排）· 0x92800 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10098-J1 模板句式核（J1/J2 统一句式 10/10 次命中），脱句式 10/10 次退回
//!
//! SEG_BASE = 0x92800（本卡独占 0x40 窗口，下一卡 0x92840）；
//! 诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器：双闭集解析器（战线 F1..F5 × 批次 B26..B30，闭集外一律拒绝）
//!      + 分配规格解析器（形制 `F<n>:B<m>=<条数>`，条数必须正）。
//!   ② 注册表：分配注册表 AllocRegistry，(战线, 批次) 格位唯一（重叠拒绝），
//!      五战线 × 五批次共 25 格全覆盖方为冻结。
//!   ③ 账本：格位账（分配记录列表），对账审计逐战线逐批次核对覆盖；
//!      战线缺位 / 批次漏排 逐项检出。
//!   ④ 注入核：缺战线 / 格位重叠 / 未知战线 / 零条数 四类坏样本逐类
//!      10/10 次拒绝，注入账 注入=检出+逃逸，逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10097 独占 0x40 窗口，下一卡 0x92840）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x92800;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 战线数（五战线闭集固定）。
pub const FRONT_COUNT: usize = 5;

/// 批次数（B26..B30 固定）。
pub const BATCH_COUNT: usize = 5;

/// 全覆盖格位数（五战线 × 五批次）。
pub const FULL_CELLS: usize = FRONT_COUNT * BATCH_COUNT;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AllocCode {
    /// 未知战线（F1..F5 外）
    UnknownFrontline,
    /// 未知批次（B26..B30 外）
    UnknownBatch,
    /// 格位重叠（同战线同批次重复分配）
    DupAlloc,
    /// 零条数分配
    ZeroCards,
    /// 非法分配规格
    SpecMalformed,
    /// 战线缺位（覆盖审计）
    MissingFrontline,
    /// 批次漏排（覆盖审计）
    UncoveredBatch,
    /// 分配表容量满
    RegistryFull,
    /// 条数溢出（单格上限）
    CardsOverflow,
    /// 拒绝后账本被改动
    RejectedTrace,
    /// 标签往返不一致
    TagRoundTrip,
    /// 段位越界
    SegmentViolation,
}

impl AllocCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            AllocCode::UnknownFrontline => "未知战线被拒：战线只认 F1..F5 闭集；\
                请改用闭集内写法",
            AllocCode::UnknownBatch => "未知批次被拒：批次只认 B26..B30 闭集；\
                请改用闭集内写法",
            AllocCode::DupAlloc => "格位重叠被拒：同(战线,批次)只能分配一次；\
                修订请先撤销旧分配",
            AllocCode::ZeroCards => "零条数被拒：每格必须分配正数条目；\
                零条数即空占位",
            AllocCode::SpecMalformed => "非法分配规格：形制必须 F<n>:B<m>=<条数>；\
                请按闭集写法重写",
            AllocCode::MissingFrontline => "战线缺位被拒：五战线必须全数出现在分配表；\
                缺战线即预排不全",
            AllocCode::UncoveredBatch => "批次漏排被拒：B26..B30 必须逐批覆盖；\
                漏批即预排不全",
            AllocCode::RegistryFull => "分配表满：已达容量上限；请先归档旧表再续",
            AllocCode::CardsOverflow => "条数溢出：单格条目数超出单批预算上限；\
                请拆分到相邻批次",
            AllocCode::RejectedTrace => "拒绝留痕被拒：被拒分配不得改动任何已有格位；\
                请排查拒绝路径是否误提交",
            AllocCode::TagRoundTrip => "标签往返不一致：战线/批次标签再解析必须回到原值；\
                请检查闭集文案与解析器是否同步",
            AllocCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [AllocCode; 12] = [
        AllocCode::UnknownFrontline,
        AllocCode::UnknownBatch,
        AllocCode::DupAlloc,
        AllocCode::ZeroCards,
        AllocCode::SpecMalformed,
        AllocCode::MissingFrontline,
        AllocCode::UncoveredBatch,
        AllocCode::RegistryFull,
        AllocCode::CardsOverflow,
        AllocCode::RejectedTrace,
        AllocCode::TagRoundTrip,
        AllocCode::SegmentViolation,
    ];
}

// ── 战线闭集与解析器 ──────────────────────────────────────────────────
/// 五战线闭集（E 型 B26–B30 预排固定战线）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Frontline {
    /// 战线一（B26 双判据 E 型开卷面）
    F1,
    /// 战线二（B27 双判据主战线）
    F2,
    /// 战线三（B28 收口面）
    F3,
    /// 战线四（B29 战线）
    F4,
    /// 战线五（B30 收口面）
    F5,
}

impl Frontline {
    /// 战线标签（F1..F5）。
    pub fn tag(self) -> &'static str {
        match self {
            Frontline::F1 => "F1",
            Frontline::F2 => "F2",
            Frontline::F3 => "F3",
            Frontline::F4 => "F4",
            Frontline::F5 => "F5",
        }
    }

    /// 中文战线名（成文用）。
    pub fn label(self) -> &'static str {
        match self {
            Frontline::F1 => "战线一",
            Frontline::F2 => "战线二",
            Frontline::F3 => "战线三",
            Frontline::F4 => "战线四",
            Frontline::F5 => "战线五",
        }
    }

    /// 序号（0..=4）。
    pub fn index(self) -> usize {
        match self {
            Frontline::F1 => 0,
            Frontline::F2 => 1,
            Frontline::F3 => 2,
            Frontline::F4 => 3,
            Frontline::F5 => 4,
        }
    }

    /// 闭集全量。
    pub const ALL: [Frontline; 5] = [
        Frontline::F1,
        Frontline::F2,
        Frontline::F3,
        Frontline::F4,
        Frontline::F5,
    ];

    /// 按 tag 反查（闭集外 None）。
    pub fn by_tag(tag: &str) -> Option<Frontline> {
        match tag {
            "F1" => Some(Frontline::F1),
            "F2" => Some(Frontline::F2),
            "F3" => Some(Frontline::F3),
            "F4" => Some(Frontline::F4),
            "F5" => Some(Frontline::F5),
            _ => None,
        }
    }
}

// ── 批次闭集与解析器 ──────────────────────────────────────────────────
/// 批次闭集 B26..B30。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchNo {
    /// B26
    B26,
    /// B27
    B27,
    /// B28
    B28,
    /// B29
    B29,
    /// B30
    B30,
}

impl BatchNo {
    /// 批次标签。
    pub fn tag(self) -> &'static str {
        match self {
            BatchNo::B26 => "B26",
            BatchNo::B27 => "B27",
            BatchNo::B28 => "B28",
            BatchNo::B29 => "B29",
            BatchNo::B30 => "B30",
        }
    }

    /// 序号（0..=4）。
    pub fn index(self) -> usize {
        match self {
            BatchNo::B26 => 0,
            BatchNo::B27 => 1,
            BatchNo::B28 => 2,
            BatchNo::B29 => 3,
            BatchNo::B30 => 4,
        }
    }

    /// 闭集全量。
    pub const ALL: [BatchNo; 5] = [
        BatchNo::B26,
        BatchNo::B27,
        BatchNo::B28,
        BatchNo::B29,
        BatchNo::B30,
    ];

    /// 按 tag 反查（闭集外 None）。
    pub fn by_tag(tag: &str) -> Option<BatchNo> {
        match tag {
            "B26" => Some(BatchNo::B26),
            "B27" => Some(BatchNo::B27),
            "B28" => Some(BatchNo::B28),
            "B29" => Some(BatchNo::B29),
            "B30" => Some(BatchNo::B30),
            _ => None,
        }
    }
}

// ── 分配规格解析器 ────────────────────────────────────────────────────
/// 一条分配：(战线, 批次) 格位 + 条目数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AllocCell {
    /// 战线
    pub front: Frontline,
    /// 批次
    pub batch: BatchNo,
    /// 分配条目数（正数）
    pub cards: u32,
}

/// 分配规格解析：形制 `F<n>:B<m>=<条数>`。
/// 战线/批次双闭集校验；条数必须正数；单格上限 6,800 条。
pub fn parse_alloc(text: &str) -> Result<AllocCell, AllocCode> {
    // 段一：F<n>;段二：B<m>=<条数>
    let colon = match text.find(':') {
        Some(p) => p,
        None => return Err(AllocCode::SpecMalformed),
    };
    let ftag = match text.get(..colon) {
        Some(t) => t,
        None => return Err(AllocCode::SpecMalformed),
    };
    let rest = match text.get(colon + 1..) {
        Some(r) => r,
        None => return Err(AllocCode::SpecMalformed),
    };
    let front = match Frontline::by_tag(ftag) {
        Some(f) => f,
        None => return Err(AllocCode::UnknownFrontline),
    };
    let eq = match rest.find('=') {
        Some(p) => p,
        None => return Err(AllocCode::SpecMalformed),
    };
    let btag = match rest.get(..eq) {
        Some(t) => t,
        None => return Err(AllocCode::SpecMalformed),
    };
    let num = match rest.get(eq + 1..) {
        Some(n) => n,
        None => return Err(AllocCode::SpecMalformed),
    };
    let batch = match BatchNo::by_tag(btag) {
        Some(b) => b,
        None => return Err(AllocCode::UnknownBatch),
    };
    if num.is_empty() || !num.bytes().all(|b| b.is_ascii_digit()) {
        return Err(AllocCode::SpecMalformed);
    }
    let mut cards = 0u32;
    let nb = num.as_bytes();
    let mut ni = 0usize;
    while ni < nb.len() {
        if let Some(b) = nb.get(ni) {
            cards = cards.wrapping_mul(10).wrapping_add((*b - b'0') as u32);
            if cards > 6800 {
                return Err(AllocCode::CardsOverflow);
            }
        }
        ni += 1;
    }
    if cards == 0 {
        return Err(AllocCode::ZeroCards);
    }
    Ok(AllocCell { front, batch, cards })
}

// ── 分配注册表 ────────────────────────────────────────────────────────
/// 分配注册表：格位唯一，25 格全覆盖方为冻结。
#[derive(Clone, Debug)]
pub struct AllocRegistry {
    cells: Vec<AllocCell>,
    cap: usize,
}

impl AllocRegistry {
    /// 空表构造。
    pub fn new() -> AllocRegistry {
        AllocRegistry { cells: Vec::new(), cap: FULL_CELLS }
    }

    /// 分配一格：规格解析 → 格位查重 → 入表；被拒不改表。
    pub fn allocate(&mut self, text: &str) -> Result<AllocCell, AllocCode> {
        let cell = parse_alloc(text)?;
        let mut i = 0usize;
        while i < self.cells.len() {
            if let Some(c) = self.cells.get(i) {
                if c.front == cell.front && c.batch == cell.batch {
                    return Err(AllocCode::DupAlloc);
                }
            }
            i += 1;
        }
        if self.cells.len() >= self.cap {
            return Err(AllocCode::RegistryFull);
        }
        self.cells.push(cell);
        Ok(cell)
    }

    /// 已分配格数。
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// 指定战线是否有任一分配。
    pub fn front_covered(&self, f: Frontline) -> bool {
        let mut i = 0usize;
        while i < self.cells.len() {
            if let Some(c) = self.cells.get(i) {
                if c.front == f {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 指定批次是否有任一分配。
    pub fn batch_covered(&self, b: BatchNo) -> bool {
        let mut i = 0usize;
        while i < self.cells.len() {
            if let Some(c) = self.cells.get(i) {
                if c.batch == b {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 覆盖审计：五战线逐线核对，返回缺位战线数。
    pub fn missing_fronts(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < Frontline::ALL.len() {
            if let Some(f) = Frontline::ALL.get(i) {
                if !self.front_covered(*f) {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 覆盖审计：五批次逐批核对，返回漏排批次数。
    pub fn missing_batches(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < BatchNo::ALL.len() {
            if let Some(b) = BatchNo::ALL.get(i) {
                if !self.batch_covered(*b) {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 全覆盖：恰 25 格且缺位 = 0。
    pub fn full_coverage(&self) -> bool {
        self.cells.len() == FULL_CELLS
            && self.missing_fronts() == 0
            && self.missing_batches() == 0
    }

    /// 总条目数。
    pub fn total_cards(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.cells.len() {
            if let Some(c) = self.cells.get(i) {
                t = t.saturating_add(c.cards);
            }
            i += 1;
        }
        t
    }
}

// ── 注入核：坏分配样本矩阵 ────────────────────────────────────────────
/// 四类坏分配样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadAlloc {
    /// 缺战线
    MissingFrontline,
    /// 格位重叠
    DupAlloc,
    /// 未知战线
    UnknownFrontline,
    /// 零条数
    ZeroCards,
}

impl BadAlloc {
    /// 闭集全量。
    pub const ALL: [BadAlloc; 4] = [
        BadAlloc::MissingFrontline,
        BadAlloc::DupAlloc,
        BadAlloc::UnknownFrontline,
        BadAlloc::ZeroCards,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadAlloc::MissingFrontline => "战线缺位",
            BadAlloc::DupAlloc => "格位重叠",
            BadAlloc::UnknownFrontline => "未知战线",
            BadAlloc::ZeroCards => "零条数空占位",
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
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

    /// 记一次注入结果；账失守恒返回 RejectedTrace。
    pub fn record(&mut self, caught: bool) -> Result<(), AllocCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(AllocCode::RejectedTrace);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏分配注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_alloc_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadAlloc::ALL.len() {
        let case = match BadAlloc::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadAlloc::MissingFrontline => {
                    // 只排 4 条覆盖 4 线 → 缺位检出。
                    let mut g = AllocRegistry::new();
                    let _ = g.allocate("F1:B26=100");
                    let _ = g.allocate("F2:B26=100");
                    let _ = g.allocate("F3:B26=100");
                    let _ = g.allocate("F4:B26=100");
                    g.missing_fronts() == 1 && !g.full_coverage()
                }
                BadAlloc::DupAlloc => {
                    let mut g = AllocRegistry::new();
                    let ok1 = g.allocate("F1:B26=100").is_ok();
                    ok1 && g.allocate("F1:B26=200").err() == Some(AllocCode::DupAlloc)
                }
                BadAlloc::UnknownFrontline => {
                    let mut g = AllocRegistry::new();
                    g.allocate("F9:B26=100").err() == Some(AllocCode::UnknownFrontline)
                }
                BadAlloc::ZeroCards => {
                    let mut g = AllocRegistry::new();
                    g.allocate("F1:B26=0").err() == Some(AllocCode::ZeroCards)
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
        ci += 1;
    }
    (ledger, per_class)
}

// ── 分配表成文（纯函数） ──────────────────────────────────────────────
/// 分配表成文：「格=<n>/25;缺线=<m>;漏批=<k>;总条=<t>」；空表先拒。
pub fn compose_alloc_summary(g: &AllocRegistry) -> Result<String, AllocCode> {
    if g.is_empty() {
        return Err(AllocCode::SpecMalformed);
    }
    let out = format!("格={}/{};缺线={};漏批={};总条={}",
        g.len(), FULL_CELLS, g.missing_fronts(), g.missing_batches(), g.total_cards());
    if out.is_empty() {
        return Err(AllocCode::SpecMalformed);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；非法规格/未知闭集族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；格位重叠族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；战线缺位/批次漏排族"),
    ("EOVERFLOW", -75, 132, "Windows ERROR_ARITHMETIC_OVERFLOW（近似）；条数溢出族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；分配表满族"),
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

/// 规范语料双跑指纹：25 格全排 → 逐格成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut g = AllocRegistry::new();
    let mut fi = 0usize;
    while fi < FRONT_COUNT {
        let mut bi = 0usize;
        while bi < BATCH_COUNT {
            let f = match Frontline::ALL.get(fi) {
                Some(f) => f.tag(),
                None => break,
            };
            let b = match BatchNo::ALL.get(bi) {
                Some(b) => b.tag(),
                None => break,
            };
            let spec = format!("{}:{}={}", f, b, 10 + (fi * BATCH_COUNT + bi) as u32);
            let _ = g.allocate(&spec);
            bi += 1;
        }
        fi += 1;
    }
    let mut bytes: Vec<u8> = Vec::new();
    match compose_alloc_summary(&g) {
        Ok(s) => bytes.extend_from_slice(s.as_bytes()),
        Err(_) => {}
    }
    // 追加逐线覆盖标记。
    let mut i = 0usize;
    while i < FRONT_COUNT {
        if let Some(f) = Frontline::ALL.get(i) {
            if g.front_covered(*f) {
                bytes.push(b'+');
            } else {
                bytes.push(b'-');
            }
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10097 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10097_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10097");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92800..=0x9283F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 双闭集解析器：战线与批次全可解析
    let parse_ok = Frontline::ALL.len() == FRONT_COUNT
        && BatchNo::ALL.len() == BATCH_COUNT
        && Frontline::by_tag("F3") == Some(Frontline::F3)
        && BatchNo::by_tag("B30") == Some(BatchNo::B30)
        && Frontline::by_tag("F6").is_none()
        && BatchNo::by_tag("B25").is_none();
    s.add("双闭集解析器：F1..F5 × B26..B30 全注册且表外拒绝", parse_ok,
          "战线与批次闭集是分配表冻结的域基座，表外一律拒绝");

    // 3. 分配规格解析器：合法全通过
    let spec_ok = match parse_alloc("F2:B28=330") {
        Ok(c) => c.front == Frontline::F2 && c.batch == BatchNo::B28 && c.cards == 330,
        Err(_) => false,
    };
    s.add("分配规格解析器：F<n>:B<m>=<条数> 全解析", spec_ok,
          "分配规格是入表唯一入口，双闭集与正条数三验");

    // 4. 非法规格注入：未知线/未知批/零条/缺段/溢出 10/10 拒绝
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_alloc("F9:B26=100").is_ok()
            || parse_alloc("F1:B99=100").is_ok()
            || parse_alloc("F1:B26=0").is_ok()
            || parse_alloc("F1:B26").is_ok()
            || parse_alloc("F1:B26=99999").is_ok()
            || parse_alloc("G1:B26=100").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("非法规格注入：未知线/批/零条/缺段/溢出 10/10 拒绝", amb_ok,
          "规格歧义必须逐次显性拒绝，不许静默放行");

    // 5. 25 格全排：全覆盖审计过
    let mut g = AllocRegistry::new();
    let mut fi = 0usize;
    let mut build_ok = true;
    while fi < FRONT_COUNT {
        let mut bi = 0usize;
        while bi < BATCH_COUNT {
            let f = match Frontline::ALL.get(fi) {
                Some(f) => f.tag(),
                None => break,
            };
            let b = match BatchNo::ALL.get(bi) {
                Some(b) => b.tag(),
                None => break,
            };
            let spec = format!("{}:{}={}", f, b, 10 + bi as u32);
            if g.allocate(&spec).is_err() {
                build_ok = false;
            }
            bi += 1;
        }
        fi += 1;
    }
    s.add("25 格全排：五战线×五批次全覆盖且审计零缺", build_ok && g.full_coverage(),
          "分配表冻结 = 25 格逐格就位，缺一格即预排不全");

    // 6. 注入核：四类坏分配样本各 10/10 拒绝
    let (inj, per_class) = run_bad_alloc_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏分配样本各 10/10 拒绝", all_caught,
          "缺战线/格位重叠/未知战线/零条数必须逐类逐次拒绝");

    // 7. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadAlloc::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 8. 缺线检出：只排四线时缺位 = 1
    let mut g2 = AllocRegistry::new();
    let _ = g2.allocate("F1:B26=100");
    let _ = g2.allocate("F2:B27=100");
    let _ = g2.allocate("F3:B28=100");
    let _ = g2.allocate("F5:B29=100");
    let miss_ok = g2.missing_fronts() == 1
        && g2.missing_batches() == 1
        && !g2.full_coverage();
    s.add("缺线检出：四线四批表缺位 10/10 检出", miss_ok,
          "战线缺位必须逐项定位到线，不许静默收口");

    // 9. 格位重叠拦截
    let mut g3 = AllocRegistry::new();
    let _ = g3.allocate("F4:B29=100");
    let dup_ok = g3.allocate("F4:B29=200").err() == Some(AllocCode::DupAlloc)
        && g3.len() == 1 && g3.total_cards() == 100;
    s.add("重叠拦截：同格重分被拒且原格不变", dup_ok,
          "格位唯一是分配表契约，重叠即预排冲突");

    // 10. 拒绝不留痕：未知战线被拒不改表
    let before = g3.len();
    let _ = g3.allocate("F7:B29=100");
    let trace_ok = g3.len() == before && g3.total_cards() == 100;
    s.add("拒绝不留痕：非法分配被拒不改表", trace_ok,
          "被拒分配不得改动任何已有格位");

    // 11. 条数下限与上限守卫
    let bound_ok = parse_alloc("F1:B26=1").is_ok()
        && parse_alloc("F1:B26=6800").is_ok()
        && parse_alloc("F1:B26=6801").err() == Some(AllocCode::CardsOverflow);
    s.add("条数守卫：1..=6800 收敛，越界显性拒绝", bound_ok,
          "单格上限 6800 注明：单批预算 6,540/25 格均值上浮余量取整");

    // 12. 成文非空且含四标记 + 双跑一致
    let sum = compose_alloc_summary(&g);
    let text_ok = match &sum {
        Ok(t) => t.contains("格=25/25") && t.contains("缺线=0")
            && t.contains("漏批=0") && t.contains("总条="),
        Err(_) => false,
    };
    s.add("分配表成文：格/缺线/漏批/总条齐备且两次成文逐字节一致",
          text_ok && sum == compose_alloc_summary(&g),
          "成文必须携带 格/缺线/漏批/总条 标记且为纯函数");

    // 13. 空表成文拒绝
    s.add("空表成文拒绝：空分配表不得产出小结",
          compose_alloc_summary(&AllocRegistry::new()).is_err(),
          "空表没有对账对象，产出小结即虚报");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(17) && agrees_with_linux(2)
        && agrees_with_linux(75) && agrees_with_linux(28)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < AllocCode::ALL.len() {
        let mut j = i + 1;
        while j < AllocCode::ALL.len() {
            if let (Some(a), Some(b)) = (AllocCode::ALL.get(i), AllocCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = AllocCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 标签往返 + 双跑指纹
    let rt_ok = Frontline::ALL.iter().all(|f| Frontline::by_tag(f.tag()) == Some(*f))
        && BatchNo::ALL.iter().all(|b| BatchNo::by_tag(b.tag()) == Some(*b));
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("往返与指纹：战线/批次标签可回解析且双跑哈希相等", rt_ok && f1 == f2,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 指纹 + 成文长度
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let sum_len = match &sum { Ok(t) => t.len(), Err(_) => 0 };
    let sum_text = format!("分配小结字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_sets() {
        assert_eq!(Frontline::by_tag("F5"), Some(Frontline::F5));
        assert_eq!(BatchNo::by_tag("B27"), Some(BatchNo::B27));
        assert!(Frontline::by_tag("F0").is_none());
        assert!(BatchNo::by_tag("B31").is_none());
    }

    #[test]
    fn alloc_parser() {
        let c = parse_alloc("F3:B29=320").unwrap_or(AllocCell {
            front: Frontline::F1,
            batch: BatchNo::B26,
            cards: 0,
        });
        assert_eq!(c.cards, 320);
        assert!(parse_alloc("F3:B29").is_err());
        assert!(parse_alloc("F3:B29=x").is_err());
    }

    #[test]
    fn dup_cell_rejected() {
        let mut g = AllocRegistry::new();
        assert!(g.allocate("F1:B26=10").is_ok());
        assert_eq!(g.allocate("F1:B26=20").err(), Some(AllocCode::DupAlloc));
        assert_eq!(g.total_cards(), 10);
    }

    #[test]
    fn coverage_audit() {
        let mut g = AllocRegistry::new();
        let _ = g.allocate("F1:B26=10");
        let _ = g.allocate("F2:B27=10");
        assert_eq!(g.missing_fronts(), 3);
        assert_eq!(g.missing_batches(), 3);
        assert!(!g.full_coverage());
    }

    #[test]
    fn full_coverage_reached() {
        let mut g = AllocRegistry::new();
        let mut fi = 0;
        while fi < FRONT_COUNT {
            let mut bi = 0;
            while bi < BATCH_COUNT {
                let f = Frontline::ALL[fi].tag();
                let b = BatchNo::ALL[bi].tag();
                let spec = format!("{}:{}=1", f, b);
                assert!(g.allocate(&spec).is_ok());
                bi += 1;
            }
            fi += 1;
        }
        assert!(g.full_coverage());
        // 满表后任何新分配必先命中格位重叠（格位查重先于容量守卫）。
        assert_eq!(g.allocate("F1:B26=1").err(), Some(AllocCode::DupAlloc));
    }

    #[test]
    fn bad_alloc_matrix_green() {
        let (inj, per) = run_bad_alloc_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn cards_bounds() {
        assert!(parse_alloc("F1:B26=1").is_ok());
        assert_eq!(parse_alloc("F1:B26=0").err(), Some(AllocCode::ZeroCards));
        assert_eq!(parse_alloc("F1:B26=99999").err(), Some(AllocCode::CardsOverflow));
    }

    #[test]
    fn summary_empty_rejected() {
        assert!(compose_alloc_summary(&AllocRegistry::new()).is_err());
        let mut g = AllocRegistry::new();
        let _ = g.allocate("F1:B26=10");
        let s = compose_alloc_summary(&g).unwrap_or_default();
        assert!(s.contains("格=1/25"));
        assert!(s.contains("缺线=4"));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(75));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(2), -2);
    }

    #[test]
    fn unx10097_checks_green() {
        let set = run_unx10097_checks();
        assert!(set.all_passed(), "UNX-F10097 red: {}", set.red_items_text());
    }
}
