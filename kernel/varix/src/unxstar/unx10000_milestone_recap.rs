//! UNX-F10000 万号收口样例（F9601–F10000 半程小结） · 0x91000段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10001-J1 报表口径成文落账（五维：波/批/战线/模块/判据），
//! 维度歧义 10/10 次检出。
//!
//! 设计要点：
//!   ① 半程小结按「五维口径」建模：波/批/战线/模块/判据五个维度构成
//!      闭集，任何小结条目必须恰好落在一个维度上；空标签、复合标签、
//!      带噪声标签三类歧义样本必须 10/10 次显性检出，不许静默归并。
//!   ② 单位号空间冻结为 F9601..=F10000（400 条），波维 5 波 × 80 条、
//!      批维 40 批 × 10 条，两套切分都须整除 400；越界号一律拒绝。
//!   ③ 账本守恒：入账条数 = 五维计数之和；任何一次 add 要么全额入账
//!      要么返回错误码，不存在半入账状态；重复单位号按重复拒绝。
//!   ④ 注入账守恒：注入次数 = 检出次数 + 逃逸次数，逃逸恒为 0；
//!      账只增不减，倒退显性拒绝。
//!   ⑤ 与 Linux 逐码一致通过 errno 冻结表复算，表外码一律拒绝；
//!      Windows 对照码仅作说明不参与判定。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10000 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91000;

/// 半程首号（F9601）。
pub const FIRST_UNIT: u32 = 9601;

/// 半程末号（F10000）。
pub const LAST_UNIT: u32 = 10000;

/// 半程总条数（F9601..=F10000 共 400 条）。
pub const TOTAL_UNITS: usize = 400;

/// 批维批数（40 批 × 10 条）。
pub const BATCH_COUNT: usize = 40;

/// 波维波数（5 波 × 80 条）。
pub const WAVE_COUNT: usize = 5;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 五维标签关键字表（闭集；顺序即诊断码偏移依据）。
pub const DIM_KEYWORDS: [&str; 5] = ["波", "批", "战线", "模块", "判据"];

/// Linux errno 正数码。
pub const ENOENT: i32 = 2;
pub const EINVAL: i32 = 22;
pub const ERANGE: i32 = 34;
pub const EEXIST: i32 = 17;

// ── 五维口径 ──────────────────────────────────────────────────────────
/// 报表五维（闭集）。判据原文钉死这五个维度，缺一即半程小结不成立。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DimTag {
    /// 波维（wave）
    Wave,
    /// 批维（batch）
    Batch,
    /// 战线维（front）
    Front,
    /// 模块维（module）
    Module,
    /// 判据维（criterion）
    Criterion,
}

impl DimTag {
    /// 维度序号（0..5），用于账本下标（经由 get 访问）。
    pub fn index(self) -> usize {
        self as usize
    }

    /// 五维闭集全量。
    pub const ALL: [DimTag; 5] = [
        DimTag::Wave,
        DimTag::Batch,
        DimTag::Front,
        DimTag::Module,
        DimTag::Criterion,
    ];
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecapCode {
    /// 空标签：维度文本为空
    EmptyTag,
    /// 复合标签：命中两个及以上维度关键字
    CompoundTag,
    /// 噪声标签：命中单一关键字但携带额外字符
    NoisyTag,
    /// 未知标签：不含任何维度关键字
    UnknownTag,
    /// 单位号越界：不在 F9601..=F10000
    UnitOutOfRange,
    /// 重复条目：同一单位号二次入账
    DuplicateEntry,
    /// 账满：条目数达到容量上限
    LedgerFull,
    /// 五维有缺：小结时存在零计数维度
    MissingDim,
    /// 总数不符：小结总数与冻结值偏离
    TotalMismatch,
    /// 账倒退：计数出现负增量
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl RecapCode {
    /// 码值（段基址 + 变体序号，从 1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32) + 1
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RecapCode::EmptyTag => "空标签被拒：维度文本为空无法归维；请按五维关键字\
                （波/批/战线/模块/判据）填写后再入账",
            RecapCode::CompoundTag => "复合标签被拒：文本同时命中多个维度关键字，\
                归维歧义；请拆成单维条目逐条入账",
            RecapCode::NoisyTag => "噪声标签被拒：文本命中单一关键字但携带额外字符，\
                口径不净；请只保留维度关键字本身",
            RecapCode::UnknownTag => "未知标签被拒：文本不含任何五维关键字；请对照\
                关键字闭集修正拼写",
            RecapCode::UnitOutOfRange => "单位号越界被拒：号不在 F9601..=F10000；\
                半程小结只收半程号段，越界回 EINVAL",
            RecapCode::DuplicateEntry => "重复条目被拒：同一单位号已入账；请先查账\
                再补录，重复回 EEXIST",
            RecapCode::LedgerFull => "账满被拒：条目数已达容量上限；请扩容或先小结\
                清账再继续",
            RecapCode::MissingDim => "五维有缺：存在零计数维度，半程小结不成立；\
                请回查漏入账的维度条目",
            RecapCode::TotalMismatch => "总数不符：小结总数偏离冻结值 400；请核对\
                是否漏收或双收条目",
            RecapCode::LedgerRegress => "账倒退被拒：计数只增不减；请检查是否有\
                回滚路径误改累计值",
            RecapCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查\
                隐藏状态或非确定遍历序",
            RecapCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在\
                0x91000..=0x914C0 窗口",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [RecapCode; 12] = [
        RecapCode::EmptyTag,
        RecapCode::CompoundTag,
        RecapCode::NoisyTag,
        RecapCode::UnknownTag,
        RecapCode::UnitOutOfRange,
        RecapCode::DuplicateEntry,
        RecapCode::LedgerFull,
        RecapCode::MissingDim,
        RecapCode::TotalMismatch,
        RecapCode::LedgerRegress,
        RecapCode::NotReproducible,
        RecapCode::SegmentViolation,
    ];
}

// ── 维度解析器 ────────────────────────────────────────────────────────
/// 统计文本中第 k 个关键字是否出现（k 越界返回 false）。
fn keyword_hit(text: &str, k: usize) -> bool {
    match DIM_KEYWORDS.get(k) {
        Some(kw) => text.contains(kw),
        None => false,
    }
}

/// 维度解析：精确命中单一关键字才放行；空/复合/噪声/未知分别给码。
/// 这是「维度歧义 10/10 次检出」的唯一闸口。
pub fn parse_dim_tag(text: &str) -> Result<DimTag, RecapCode> {
    if text.is_empty() {
        return Err(RecapCode::EmptyTag);
    }
    let mut hits: u32 = 0;
    let mut hit_idx: usize = 5;
    let mut k = 0usize;
    while k < DIM_KEYWORDS.len() {
        if keyword_hit(text, k) {
            hits = hits.saturating_add(1);
            hit_idx = k;
        }
        k += 1;
    }
    if hits == 0 {
        return Err(RecapCode::UnknownTag);
    }
    if hits > 1 {
        return Err(RecapCode::CompoundTag);
    }
    // 单一命中：文本必须与关键字逐字相等，任何额外字符都算噪声歧义。
    match DIM_KEYWORDS.get(hit_idx) {
        Some(kw) if *kw == text => match hit_idx {
            0 => Ok(DimTag::Wave),
            1 => Ok(DimTag::Batch),
            2 => Ok(DimTag::Front),
            3 => Ok(DimTag::Module),
            _ => Ok(DimTag::Criterion),
        },
        _ => Err(RecapCode::NoisyTag),
    }
}

/// 单位号归属波（1..=5）：每波 80 条。
pub fn wave_of(unit: u32) -> Result<usize, RecapCode> {
    if unit < FIRST_UNIT || unit > LAST_UNIT {
        return Err(RecapCode::UnitOutOfRange);
    }
    Ok(((unit - FIRST_UNIT) / 80 + 1) as usize)
}

/// 单位号归属批（1..=40）：每批 10 条。
pub fn batch_of(unit: u32) -> Result<usize, RecapCode> {
    if unit < FIRST_UNIT || unit > LAST_UNIT {
        return Err(RecapCode::UnitOutOfRange);
    }
    Ok(((unit - FIRST_UNIT) / 10 + 1) as usize)
}

// ── 半程账本 ──────────────────────────────────────────────────────────
/// 半程小结账本：条目只增、五维分计、守恒可复算。
#[derive(Clone, Debug)]
pub struct RecapLedger {
    /// 已入账单位号（升序插入校验用）
    units: Vec<u32>,
    /// 五维分计
    tallies: [u32; 5],
    /// 容量上限
    cap: usize,
}

impl RecapLedger {
    /// 构造指定容量的空账本。
    pub fn with_capacity(cap: usize) -> RecapLedger {
        RecapLedger { units: Vec::new(), tallies: [0; 5], cap }
    }

    /// 构造全量容量（400 条）的空账本。
    pub fn full() -> RecapLedger {
        RecapLedger::with_capacity(TOTAL_UNITS)
    }

    /// 入账一条：单位号越界、重复、账满、维度歧义逐项拒绝。
    pub fn add(&mut self, unit: u32, tag: DimTag) -> Result<(), RecapCode> {
        if unit < FIRST_UNIT || unit > LAST_UNIT {
            return Err(RecapCode::UnitOutOfRange);
        }
        if self.units.len() >= self.cap {
            return Err(RecapCode::LedgerFull);
        }
        let mut i = 0usize;
        while i < self.units.len() {
            if let Some(u) = self.units.get(i) {
                if *u == unit {
                    return Err(RecapCode::DuplicateEntry);
                }
            }
            i += 1;
        }
        let idx = tag.index();
        if idx >= 5 {
            return Err(RecapCode::UnknownTag);
        }
        self.units.push(unit);
        if let Some(t) = self.tallies.get_mut(idx) {
            *t = t.saturating_add(1);
        }
        if self.units.len().saturating_add(0) != self.conserved_total() {
            return Err(RecapCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式：条目数 = 五维计数之和。
    pub fn conserved_total(&self) -> usize {
        let mut sum = 0usize;
        let mut i = 0usize;
        while i < 5 {
            if let Some(t) = self.tallies.get(i) {
                sum = sum.saturating_add(*t as usize);
            }
            i += 1;
        }
        sum
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.units.len()
    }

    /// 空账判定。
    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    /// 取某维计数（越界维返回 0，显性可查）。
    pub fn tally(&self, tag: DimTag) -> u32 {
        match self.tallies.get(tag.index()) {
            Some(t) => *t,
            None => 0,
        }
    }

    /// 五维是否齐（无零计数维）。
    pub fn all_dims_present(&self) -> bool {
        let mut i = 0usize;
        while i < 5 {
            match self.tallies.get(i) {
                Some(0) | None => return false,
                _ => {}
            }
            i += 1;
        }
        true
    }

    /// 批覆盖数：入账条目映射到的不同批号个数。
    pub fn batch_cover(&self) -> usize {
        let mut seen = [false; BATCH_COUNT];
        let mut i = 0usize;
        while i < self.units.len() {
            if let Some(u) = self.units.get(i) {
                if let Ok(b) = batch_of(*u) {
                    if b >= 1 && b <= BATCH_COUNT {
                        seen[b - 1] = true;
                    }
                }
            }
            i += 1;
        }
        let mut n = 0usize;
        let mut k = 0usize;
        while k < BATCH_COUNT {
            if seen[k] {
                n = n.saturating_add(1);
            }
            k += 1;
        }
        n
    }

    /// 波覆盖数：入账条目映射到的不同波号个数。
    pub fn wave_cover(&self) -> usize {
        let mut seen = [false; WAVE_COUNT];
        let mut i = 0usize;
        while i < self.units.len() {
            if let Some(u) = self.units.get(i) {
                if let Ok(w) = wave_of(*u) {
                    if w >= 1 && w <= WAVE_COUNT {
                        seen[w - 1] = true;
                    }
                }
            }
            i += 1;
        }
        let mut n = 0usize;
        let mut k = 0usize;
        while k < WAVE_COUNT {
            if seen[k] {
                n = n.saturating_add(1);
            }
            k += 1;
        }
        n
    }

    /// 半程小结：全量、五维齐、总数守恒三项全过才算小结成立。
    pub fn recap(&self) -> Result<usize, RecapCode> {
        if self.len() != TOTAL_UNITS {
            return Err(RecapCode::TotalMismatch);
        }
        if !self.all_dims_present() {
            return Err(RecapCode::MissingDim);
        }
        if self.conserved_total() != TOTAL_UNITS {
            return Err(RecapCode::TotalMismatch);
        }
        Ok(self.len())
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 四类歧义/坏样本注入类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadCase {
    /// 空标签
    Empty,
    /// 复合标签（波+批）
    Compound,
    /// 噪声标签（波次）
    Noisy,
    /// 越界单位号（F10001）
    OutOfRange,
}

impl BadCase {
    /// 期望诊断码。
    pub fn code(self) -> RecapCode {
        match self {
            BadCase::Empty => RecapCode::EmptyTag,
            BadCase::Compound => RecapCode::CompoundTag,
            BadCase::Noisy => RecapCode::NoisyTag,
            BadCase::OutOfRange => RecapCode::UnitOutOfRange,
        }
    }

    /// 构造样本文本（越界类不用文本）。
    pub fn sample_text(self) -> &'static str {
        match self {
            BadCase::Empty => "",
            BadCase::Compound => "波批",
            BadCase::Noisy => "波次",
            BadCase::OutOfRange => "判据",
        }
    }

    /// 闭集全量。
    pub const ALL: [BadCase; 4] = [
        BadCase::Empty,
        BadCase::Compound,
        BadCase::Noisy,
        BadCase::OutOfRange,
    ];
}

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
            return Err(-(RecapCode::LedgerRegress as i32));
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行四类歧义样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_ambiguity_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadCase::ALL.len() {
        if let Some(case) = BadCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = if case == BadCase::OutOfRange {
                    // 越界号走账本入口拒绝，歧义文本走解析器拒绝。
                    let mut lg = RecapLedger::full();
                    lg.add(LAST_UNIT + 1, DimTag::Criterion).err() == Some(RecapCode::UnitOutOfRange)
                } else {
                    parse_dim_tag(case.sample_text()).err() == Some(case.code())
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

/// 全量样本账：400 条按单位号轮转五维入账（确定性分布）。
pub fn build_full_ledger() -> RecapLedger {
    let mut lg = RecapLedger::full();
    let mut unit = FIRST_UNIT;
    while unit <= LAST_UNIT {
        // 轮转维度：保证五维各 80 条、批覆盖 40/40、波覆盖 5/5。
        let slot = ((unit - FIRST_UNIT) % 5) as usize;
        let tag = match DimTag::ALL.get(slot) {
            Some(t) => *t,
            None => DimTag::Criterion,
        };
        let _ = lg.add(unit, tag);
        unit = unit.saturating_add(1);
    }
    lg
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；越界/坏参数族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；未知标签归此族"),
    ("EEXIST", -17, 80, "Windows ERROR_FILE_EXISTS；重复条目族"),
    ("ERANGE", -34, 34, "Windows ERROR_ARITHMETIC_OVERFLOW；计数溢出族"),
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

/// 小结指纹：五维计数 + 守恒总数逐字节入哈希，双跑必须一致。
pub fn recap_fingerprint(lg: &RecapLedger) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < 5 {
        let t = match DimTag::ALL.get(i) {
            Some(tag) => lg.tally(*tag),
            None => 0,
        };
        bytes.push(((t >> 24) & 0xFF) as u8);
        bytes.push(((t >> 16) & 0xFF) as u8);
        bytes.push(((t >> 8) & 0xFF) as u8);
        bytes.push((t & 0xFF) as u8);
        i += 1;
    }
    let n = lg.len();
    bytes.push(((n >> 8) & 0xFF) as u8);
    bytes.push((n & 0xFF) as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10000 判据集（16 条 + 2 条动态明细）。
pub fn run_unx10000_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10000");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91000..=0x914C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91000..=0x914C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 五维精确解析
    let parse_ok = parse_dim_tag("波") == Ok(DimTag::Wave)
        && parse_dim_tag("批") == Ok(DimTag::Batch)
        && parse_dim_tag("战线") == Ok(DimTag::Front)
        && parse_dim_tag("模块") == Ok(DimTag::Module)
        && parse_dim_tag("判据") == Ok(DimTag::Criterion);
    s.add("五维解析：关键字闭集逐字命中", parse_ok,
          "五个维度关键字必须各归其维，不得串维");

    // 3. 全量 400 条入账且守恒
    let lg = build_full_ledger();
    s.add("全量入账：400 条且账目守恒",
          lg.len() == TOTAL_UNITS && lg.conserved_total() == TOTAL_UNITS,
          "F9601..=F10000 逐条入账，条数与分计之和必须一致");

    // 4. 五维无缺
    s.add("五维无缺：波/批/战线/模块/判据全非零", lg.all_dims_present(),
          "半程小结成立要求五维各有计数");

    // 5. 批覆盖 40/40
    s.add("批覆盖：40 批逐批命中", lg.batch_cover() == BATCH_COUNT,
          "400 条按每批 10 条切分必须覆盖全部 40 批");

    // 6. 波覆盖 5/5
    s.add("波覆盖：5 波逐波命中", lg.wave_cover() == WAVE_COUNT,
          "400 条按每波 80 条切分必须覆盖全部 5 波");

    // 7. 半程小结成立
    s.add("半程小结：recap 返回 400", lg.recap() == Ok(TOTAL_UNITS),
          "全量 + 五维齐 + 守恒三项全过小结才算成立");

    // 8. 重复条目拒绝
    let mut dup_lg = RecapLedger::full();
    let first = dup_lg.add(9601, DimTag::Wave);
    let second = dup_lg.add(9601, DimTag::Wave);
    s.add("重复拒绝：同号二次入账回 EEXIST 语义",
          first.is_ok() && second.err() == Some(RecapCode::DuplicateEntry),
          "重复单位号必须显性拒绝，不许静默覆盖");

    // 9. 越界拒绝
    let mut rng_lg = RecapLedger::full();
    let lo = rng_lg.add(FIRST_UNIT - 1, DimTag::Wave);
    let hi = rng_lg.add(LAST_UNIT + 1, DimTag::Wave);
    s.add("越界拒绝：F9600/F10001 均拒", lo.err() == Some(RecapCode::UnitOutOfRange)
              && hi.err() == Some(RecapCode::UnitOutOfRange),
          "半程号段之外一律 EINVAL 语义拒绝");

    // 10. 账满拒绝
    let mut tiny = RecapLedger::with_capacity(2);
    let a = tiny.add(9601, DimTag::Wave);
    let b = tiny.add(9602, DimTag::Batch);
    let c = tiny.add(9603, DimTag::Front);
    s.add("账满拒绝：容量 2 装第 3 条被拒",
          a.is_ok() && b.is_ok() && c.err() == Some(RecapCode::LedgerFull),
          "容量上限必须硬约束，不许静默扩容");

    // 11. 歧义注入矩阵：四类各 10/10 检出
    let (ledger, per_class) = run_ambiguity_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("歧义注入：四类各 10/10 检出", all_caught,
          "空/复合/噪声/越界四类坏样本必须全部显性拒绝");

    // 12. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 4 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(ERANGE)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 14. 双跑指纹一致
    let fp_a = recap_fingerprint(&lg);
    let fp_b = recap_fingerprint(&build_full_ledger());
    s.add("双跑一致：全量账指纹相等", fp_a == fp_b,
          "同输入两次构建必须逐字段相等");

    // 15. 文案闭集
    let text_ok = RecapCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < RecapCode::ALL.len() {
        let mut j = i + 1;
        while j < RecapCode::ALL.len() {
            if let (Some(a), Some(b)) = (RecapCode::ALL.get(i), RecapCode::ALL.get(j)) {
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
    let fp_text = format!("全量账指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_exact_keywords() {
        assert_eq!(parse_dim_tag("波"), Ok(DimTag::Wave));
        assert_eq!(parse_dim_tag("判据"), Ok(DimTag::Criterion));
    }

    #[test]
    fn parse_rejects_ambiguity() {
        assert_eq!(parse_dim_tag(""), Err(RecapCode::EmptyTag));
        assert_eq!(parse_dim_tag("波批"), Err(RecapCode::CompoundTag));
        assert_eq!(parse_dim_tag("波次"), Err(RecapCode::NoisyTag));
        assert_eq!(parse_dim_tag("xyz"), Err(RecapCode::UnknownTag));
    }

    #[test]
    fn unit_mapping_partitions() {
        assert_eq!(wave_of(9601), Ok(1));
        assert_eq!(wave_of(10000), Ok(5));
        assert_eq!(batch_of(9701), Ok(11));
        assert!(wave_of(9600).is_err());
        assert!(batch_of(10001).is_err());
    }

    #[test]
    fn full_ledger_conserved() {
        let lg = build_full_ledger();
        assert_eq!(lg.len(), 400);
        assert_eq!(lg.conserved_total(), 400);
        assert_eq!(lg.batch_cover(), 40);
        assert_eq!(lg.wave_cover(), 5);
        assert!(lg.all_dims_present());
    }

    #[test]
    fn duplicate_rejected() {
        let mut lg = RecapLedger::full();
        assert!(lg.add(9601, DimTag::Wave).is_ok());
        assert_eq!(lg.add(9601, DimTag::Wave), Err(RecapCode::DuplicateEntry));
    }

    #[test]
    fn out_of_range_rejected() {
        let mut lg = RecapLedger::full();
        assert_eq!(lg.add(10001, DimTag::Wave), Err(RecapCode::UnitOutOfRange));
        assert_eq!(lg.add(0, DimTag::Wave), Err(RecapCode::UnitOutOfRange));
    }

    #[test]
    fn capacity_enforced() {
        let mut lg = RecapLedger::with_capacity(1);
        assert!(lg.add(9601, DimTag::Wave).is_ok());
        assert_eq!(lg.add(9602, DimTag::Wave), Err(RecapCode::LedgerFull));
    }

    #[test]
    fn ambiguity_matrix_green() {
        let (l, per) = run_ambiguity_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 4 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_deterministic() {
        let a = recap_fingerprint(&build_full_ledger());
        let b = recap_fingerprint(&build_full_ledger());
        assert_eq!(a, b);
    }

    #[test]
    fn errno_table_agrees() {
        assert!(agrees_with_linux(EINVAL));
        assert_eq!(linux_neg(EEXIST), -EEXIST);
        assert!(!agrees_with_linux(0));
    }

    #[test]
    fn unx10000_checks_green() {
        let set = run_unx10000_checks();
        assert!(set.all_passed(), "UNX-F10000 red: {}", set.red_items_text());
    }
}
