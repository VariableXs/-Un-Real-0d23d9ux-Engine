//! UNX-F10002 波次维聚合器（wave-01..06） · 0x91080段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10003-J1 批次聚合零缺批（40 批逐批入表 10/10 次），缺批
//! 10/10 次检出。
//!
//! 设计要点：
//!   ① 波维聚合按「六格账」建模：wave-01..wave-06 各占一格，40 个批次
//!      按冻结映射 b→wave = ((b-1)×6)/40 + 1 落格（整除结果分布
//!      7+7+6+7+7+6=40）；映射为纯函数，双跑逐位一致。
//!   ② 聚合守恒：六格行数合计 = 全部批次行数之和；任何一格空格都算
//!      缺波，聚合拒绝出表。
//!   ③ 生命周期单向：Idle → Collecting → Sealed；Sealed 后再入账、
//!      Idle 直接封账均拒绝，封账必须零缺波。
//!   ④ 注入核四类坏样本：缺波/越界批号/重复批号/封账后入账，逐类
//!      10/10 次显性拒绝；注入账守恒逃逸恒为 0。
//!   ⑤ 与 Linux 逐码一致通过 errno 冻结表复算；FNV-1a 指纹双跑一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10002 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91080;

/// 波格数（wave-01..06）。
pub const WAVE_COUNT: usize = 6;

/// 批次总数（B01..B40）。
pub const BATCH_COUNT: usize = 40;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码。
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const ERANGE: i32 = 34;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AggCode {
    /// 批号越界：不在 1..=40
    BatchOutOfRange,
    /// 重复批号
    DuplicateBatch,
    /// 缺波：聚合时存在空波格
    MissingWave,
    /// 行数非法：单批行数为 0
    ZeroLines,
    /// 行数倒退：账内行数出现负增量
    LinesRegress,
    /// 未收账先封账
    SealBeforeCollect,
    /// 封账后再入账
    AddAfterSeal,
    /// 状态回退
    StateRegress,
    /// 守恒破约：六格合计 ≠ 批次合计
    ConservationBreach,
    /// 波映射越界：映射函数输出不在 1..=6
    MapOutOfRange,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl AggCode {
    /// 码值（段基址 + 变体序号，从 1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32) + 1
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            AggCode::BatchOutOfRange => "批号越界被拒：批号只允许 1..=40；请按\
                B01..B40 编号入账",
            AggCode::DuplicateBatch => "重复批号被拒：该批已入账；请先查账再\
                补录，重复回 EEXIST",
            AggCode::MissingWave => "缺波被拒：聚合时存在空波格；请补齐该波\
                批次后再封账",
            AggCode::ZeroLines => "零行数被拒：单批行数必须大于 0；空批不属于\
                任何波，请回查该批账目",
            AggCode::LinesRegress => "行数倒退被拒：累计行数只增不减；请检查\
                是否有回滚路径误改账目",
            AggCode::SealBeforeCollect => "未收账先封账被拒：至少入账一批才能\
                封账；请先走 Collecting 段",
            AggCode::AddAfterSeal => "封账后入账被拒：账已封定即只读；如需\
                增补请新开一期聚合",
            AggCode::StateRegress => "状态回退被拒：Idle→Collecting→Sealed\
                只进不退，回退视为私改账目",
            AggCode::ConservationBreach => "守恒破约：六波行数合计与批次行数\
                合计不等；请核对是否漏格或双计",
            AggCode::MapOutOfRange => "波映射越界被拒：映射函数输出必须落在\
                1..=6；请复核冻结映射式",
            AggCode::NotReproducible => "双跑不一致：同输入两次结果不同；请\
                排查隐藏状态或非确定遍历序",
            AggCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且\
                落在 0x91000..=0x914C0 窗口",
        }
    }

    /// 闭集全量。
    pub const ALL: [AggCode; 12] = [
        AggCode::BatchOutOfRange,
        AggCode::DuplicateBatch,
        AggCode::MissingWave,
        AggCode::ZeroLines,
        AggCode::LinesRegress,
        AggCode::SealBeforeCollect,
        AggCode::AddAfterSeal,
        AggCode::StateRegress,
        AggCode::ConservationBreach,
        AggCode::MapOutOfRange,
        AggCode::NotReproducible,
        AggCode::SegmentViolation,
    ];
}

// ── 冻结波映射 ────────────────────────────────────────────────────────
/// 冻结映射：批号 b（1..=40）→ 波序（1..=6）。
/// 式子 (b-1)×6/40+1 为整除纯函数：分布 7,7,6,7,7,6。
pub fn wave_of_batch(b: usize) -> Result<usize, AggCode> {
    if b < 1 || b > BATCH_COUNT {
        return Err(AggCode::BatchOutOfRange);
    }
    let w = ((b - 1) * WAVE_COUNT) / BATCH_COUNT + 1;
    if w < 1 || w > WAVE_COUNT {
        return Err(AggCode::MapOutOfRange);
    }
    Ok(w)
}

// ── 波格与聚合器 ──────────────────────────────────────────────────────
/// 单个波格：收本波批号与行数。
#[derive(Clone, Debug)]
pub struct WaveCell {
    /// 波序（1..=6）
    pub id: usize,
    /// 本波已收批号
    pub batches: Vec<usize>,
    /// 本波累计行数
    pub lines: u32,
}

impl WaveCell {
    /// 空格。
    pub fn empty(id: usize) -> WaveCell {
        WaveCell { id, batches: Vec::new(), lines: 0 }
    }

    /// 空格判定。
    pub fn is_empty(&self) -> bool {
        self.batches.is_empty()
    }

    /// 入一批：重复拒绝、零行数拒绝、行数只增。
    pub fn add(&mut self, batch: usize, lines: u32) -> Result<(), AggCode> {
        if lines == 0 {
            return Err(AggCode::ZeroLines);
        }
        let mut i = 0usize;
        while i < self.batches.len() {
            if let Some(b) = self.batches.get(i) {
                if *b == batch {
                    return Err(AggCode::DuplicateBatch);
                }
            }
            i += 1;
        }
        self.batches.push(batch);
        let before = self.lines;
        self.lines = self.lines.saturating_add(lines);
        if self.lines < before {
            return Err(AggCode::LinesRegress);
        }
        Ok(())
    }
}

/// 聚合器生命周期：只进不退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AggState {
    /// 未收账
    Idle,
    /// 收账中
    Collecting,
    /// 已封账
    Sealed,
}

/// 波维聚合器：六格账 + 单向状态机 + 守恒复核。
#[derive(Clone, Debug)]
pub struct WaveAggregator {
    state: AggState,
    cells: Vec<WaveCell>,
    /// 已收批号总数
    received: u32,
    /// 批次行数合计（守恒基准）
    batch_lines: u32,
}

impl WaveAggregator {
    /// 构造 Idle 空聚合器。
    pub fn new() -> WaveAggregator {
        let mut cells = Vec::new();
        let mut i = 1usize;
        while i <= WAVE_COUNT {
            cells.push(WaveCell::empty(i));
            i += 1;
        }
        WaveAggregator { state: AggState::Idle, cells, received: 0, batch_lines: 0 }
    }

    /// 当前状态。
    pub fn state(&self) -> AggState {
        self.state
    }

    /// 入账一批（行数 lines）：Idle 自动转 Collecting；Sealed 拒绝。
    pub fn add_batch(&mut self, batch: usize, lines: u32) -> Result<usize, AggCode> {
        if self.state == AggState::Sealed {
            return Err(AggCode::AddAfterSeal);
        }
        let w = wave_of_batch(batch)?;
        self.state = AggState::Collecting;
        let idx = w - 1;
        {
            let cell = match self.cells.get_mut(idx) {
                Some(c) => c,
                None => return Err(AggCode::MapOutOfRange),
            };
            cell.add(batch, lines)?;
        }
        self.received = self.received.saturating_add(1);
        self.batch_lines = self.batch_lines.saturating_add(lines);
        Ok(w)
    }

    /// 取波格（越界返回 None）。
    pub fn cell(&self, wave: usize) -> Option<&WaveCell> {
        if wave < 1 || wave > WAVE_COUNT {
            return None;
        }
        self.cells.get(wave - 1)
    }

    /// 空格波数。
    pub fn empty_waves(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.cells.len() {
            if let Some(c) = self.cells.get(i) {
                if c.is_empty() {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 六波行数合计。
    pub fn wave_lines_total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.cells.len() {
            if let Some(c) = self.cells.get(i) {
                sum = sum.saturating_add(c.lines);
            }
            i += 1;
        }
        sum
    }

    /// 批次行数合计（守恒基准）。
    pub fn batch_lines_total(&self) -> u32 {
        self.batch_lines
    }

    /// 已收批数。
    pub fn received_batches(&self) -> u32 {
        self.received
    }

    /// 封账：Idle 直接封被拒；存在空波格被拒；封后只读。
    pub fn seal(&mut self) -> Result<(), AggCode> {
        if self.state == AggState::Idle {
            return Err(AggCode::SealBeforeCollect);
        }
        if self.state == AggState::Sealed {
            return Err(AggCode::StateRegress);
        }
        if self.empty_waves() != 0 {
            return Err(AggCode::MissingWave);
        }
        if self.wave_lines_total() != self.batch_lines {
            return Err(AggCode::ConservationBreach);
        }
        self.state = AggState::Sealed;
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
            return Err(-(AggCode::LinesRegress as i32));
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
    /// 缺波：抽走第六波全部批次后封账
    MissingWave,
    /// 越界批号：B00 与 B41
    OutOfRange,
    /// 重复批号：同批二次入账
    Duplicate,
    /// 封账后入账
    AddAfterSeal,
}

impl BadCase {
    /// 闭集全量。
    pub const ALL: [BadCase; 4] = [
        BadCase::MissingWave,
        BadCase::OutOfRange,
        BadCase::Duplicate,
        BadCase::AddAfterSeal,
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
                    BadCase::MissingWave => {
                        // 只收第一波的 7 批（1..=7），其余波空，封账必须报缺波。
                        let mut agg = WaveAggregator::new();
                        let mut b = 1usize;
                        while b <= 7 {
                            let _ = agg.add_batch(b, 10);
                            b += 1;
                        }
                        agg.seal().err() == Some(AggCode::MissingWave)
                    }
                    BadCase::OutOfRange => {
                        let mut agg = WaveAggregator::new();
                        agg.add_batch(0, 10).err() == Some(AggCode::BatchOutOfRange)
                            && agg.add_batch(BATCH_COUNT + 1, 10)
                                == Err(AggCode::BatchOutOfRange)
                    }
                    BadCase::Duplicate => {
                        let mut agg = WaveAggregator::new();
                        let first = agg.add_batch(3, 10).is_ok();
                        first && agg.add_batch(3, 10) == Err(AggCode::DuplicateBatch)
                    }
                    BadCase::AddAfterSeal => {
                        let mut agg = WaveAggregator::new();
                        let mut b = 1usize;
                        while b <= BATCH_COUNT {
                            let _ = agg.add_batch(b, 10);
                            b += 1;
                        }
                        agg.seal().is_ok()
                            && agg.add_batch(1, 10) == Err(AggCode::AddAfterSeal)
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

/// 全量聚合：40 批逐批入账（行数 = 10 + 批号 mod 7，确定性）。
pub fn build_full_aggregator() -> WaveAggregator {
    let mut agg = WaveAggregator::new();
    let mut b = 1usize;
    while b <= BATCH_COUNT {
        let lines = (10 + (b % 7)) as u32;
        let _ = agg.add_batch(b, lines);
        b += 1;
    }
    let _ = agg.seal();
    agg
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺员/空格族"),
    ("EEXIST", -17, 80, "Windows ERROR_FILE_EXISTS；重复批号族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；越界批号族"),
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

/// 聚合指纹：六波行数 + 状态逐字节入哈希。
pub fn agg_fingerprint(agg: &WaveAggregator) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut w = 1usize;
    while w <= WAVE_COUNT {
        let lines = match agg.cell(w) {
            Some(c) => c.lines,
            None => 0,
        };
        bytes.push(((lines >> 8) & 0xFF) as u8);
        bytes.push((lines & 0xFF) as u8);
        w += 1;
    }
    bytes.push(match agg.state() {
        AggState::Idle => 0,
        AggState::Collecting => 1,
        AggState::Sealed => 2,
    });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10002 判据集（16 条 + 2 条动态明细）。
pub fn run_unx10002_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10002");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91000..=0x914C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91000..=0x914C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 冻结映射：40 批逐一映射且全部落 1..=6
    let mut map_ok = true;
    let mut b = 1usize;
    while b <= BATCH_COUNT {
        if wave_of_batch(b).is_err() {
            map_ok = false;
        }
        b += 1;
    }
    s.add("冻结映射：40 批逐一落波且无越界", map_ok,
          "映射式 (b-1)×6/40+1 必须把全部批号映进 1..=6");

    // 3. 映射分布：整除结果 7,7,6,7,7,6（合计 40）
    let full = build_full_aggregator();
    let mut dist_ok = true;
    let mut w = 1usize;
    while w <= WAVE_COUNT {
        let expect = if w == 3 || w == 6 { 6usize } else { 7usize };
        match full.cell(w) {
            Some(c) if c.batches.len() == expect => {}
            _ => dist_ok = false,
        }
        w += 1;
    }
    s.add("映射分布：7+7+6+7+7+6=40", dist_ok,
          "波格批数分布必须与冻结映射式的整除结果一致");

    // 4. 零缺波：六格全非空
    s.add("零缺波：六格逐波非空", full.empty_waves() == 0,
          "40 批入账后任何一格都不许为空");

    // 5. 聚合守恒：六波合计 = 批次合计
    s.add("聚合守恒：六波行数合计=批次合计",
          full.wave_lines_total() == full.batch_lines_total()
              && full.received_batches() == BATCH_COUNT as u32,
          "行数在格间不许增减，守恒式必须逐位相等");

    // 6. 封账成功且只读
    s.add("封账：全量入账后封账成功", full.state() == AggState::Sealed,
          "零缺波 + 守恒过才允许封账");

    // 7. 缺波注入：抽波后封账 10/10 拒
    let (ledger, per_class) = run_bad_case_matrix();
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
    s.add("注入矩阵：四类坏样本各 10/10 拒绝", all_caught,
          "缺波/越界/重复/封后入账必须全部显性拒绝");

    // 8. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 4 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 9. 越界批号精确拒绝
    let mut r = WaveAggregator::new();
    let lo = r.add_batch(0, 10);
    let hi = r.add_batch(BATCH_COUNT + 1, 10);
    s.add("越界拒绝：B00/B41 回 BatchOutOfRange",
          lo.err() == Some(AggCode::BatchOutOfRange)
              && hi.err() == Some(AggCode::BatchOutOfRange),
          "批号域 1..=40 之外一律拒绝，不许静默截断");

    // 10. 零行数拒绝
    let mut z = WaveAggregator::new();
    s.add("零行数拒绝：空批不许入波",
          z.add_batch(5, 0).err() == Some(AggCode::ZeroLines),
          "行数为 0 的批不属于任何波，必须显性拒绝");

    // 11. 重复批号拒绝
    let mut d = WaveAggregator::new();
    let _ = d.add_batch(9, 10);
    s.add("重复拒绝：B09 二次入账被拒",
          d.add_batch(9, 10).err() == Some(AggCode::DuplicateBatch),
          "重复批号必须显性拒绝，不许静默双计");

    // 12. 状态机：Idle 封账拒、Sealed 入账拒
    let mut sm = WaveAggregator::new();
    let idle_seal = sm.seal();
    let mut full2 = build_full_aggregator();
    let sealed_add = full2.add_batch(1, 10);
    s.add("状态机：Idle 封账/Sealed 入账均拒",
          idle_seal.err() == Some(AggCode::SealBeforeCollect)
              && sealed_add.err() == Some(AggCode::AddAfterSeal),
          "生命周期只进不退，越位操作全部拦截");

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
    let fp_a = agg_fingerprint(&build_full_aggregator());
    let fp_b = agg_fingerprint(&build_full_aggregator());
    s.add("双跑一致：聚合指纹相等", fp_a == fp_b,
          "同输入两次构建必须逐字段相等");

    // 15. 文案闭集
    let text_ok = AggCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < AggCode::ALL.len() {
        let mut j = i + 1;
        while j < AggCode::ALL.len() {
            if let (Some(a), Some(b)) = (AggCode::ALL.get(i), AggCode::ALL.get(j)) {
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
    let fp_text = format!("聚合指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_covers_all_batches() {
        let mut b = 1usize;
        while b <= 40 {
            let w = wave_of_batch(b).unwrap_or(0);
            assert!(w >= 1 && w <= 6);
            b += 1;
        }
    }

    #[test]
    fn map_rejects_out_of_range() {
        assert_eq!(wave_of_batch(0), Err(AggCode::BatchOutOfRange));
        assert_eq!(wave_of_batch(41), Err(AggCode::BatchOutOfRange));
    }

    #[test]
    fn full_agg_conserves() {
        let agg = build_full_aggregator();
        assert_eq!(agg.received_batches(), 40);
        assert_eq!(agg.wave_lines_total(), agg.batch_lines_total());
        assert_eq!(agg.empty_waves(), 0);
    }

    #[test]
    fn zero_lines_rejected() {
        let mut agg = WaveAggregator::new();
        assert_eq!(agg.add_batch(1, 0), Err(AggCode::ZeroLines));
    }

    #[test]
    fn duplicate_batch_rejected() {
        let mut agg = WaveAggregator::new();
        assert!(agg.add_batch(4, 10).is_ok());
        assert_eq!(agg.add_batch(4, 10), Err(AggCode::DuplicateBatch));
    }

    #[test]
    fn seal_requires_no_empty_wave() {
        let mut agg = WaveAggregator::new();
        let mut b = 1usize;
        while b <= 7 {
            let _ = agg.add_batch(b, 10);
            b += 1;
        }
        assert_eq!(agg.seal(), Err(AggCode::MissingWave));
    }

    #[test]
    fn idle_seal_rejected() {
        let mut agg = WaveAggregator::new();
        assert_eq!(agg.seal(), Err(AggCode::SealBeforeCollect));
    }

    #[test]
    fn sealed_add_rejected() {
        let mut agg = build_full_aggregator();
        assert_eq!(agg.add_batch(1, 10), Err(AggCode::AddAfterSeal));
    }

    #[test]
    fn bad_case_matrix_green() {
        let (l, per) = run_bad_case_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 4 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(agg_fingerprint(&build_full_aggregator()), agg_fingerprint(&build_full_aggregator()));
    }

    #[test]
    fn unx10002_checks_green() {
        let set = run_unx10002_checks();
        assert!(set.all_passed(), "UNX-F10002 red: {}", set.red_items_text());
    }
}
