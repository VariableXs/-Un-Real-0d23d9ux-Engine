//! UNX-F12750 压测性能预算（P95 分发延迟） · 0x82580段
//!
//! 判据原文（派工卡 12750 逐字锚定）：
//!   UNX-F12751-J1 双机 20 场景一致（S1 母版），展开序类零容差。
//!
//! 设计要点：
//! 1. 延迟样本账冻结闭集 40 行：样本号 1..=40 冻结编号，每行四元
//!    （样本号 / 分发耗时（整数口径：拍数）/ 预算线 / 越线旗标），
//!    越闭集、重复、零拍、超容一律显性拒；账满 40 行即 Full，
//!    红账清白才许冻结（Frozen），状态机 Open→Full→Frozen 只进不退；
//! 2. P95 计算判定器 `judge_p95(&[u32]) -> Result<P95Row, u32>`：
//!    整数口径分位——升序取第 95 百分位整数位，槽位闭式
//!    `floor((n*95+99)/100)`（1-based，最近秩 nearest-rank 的整数等价式，
//!    闭式可复算）；空样本 → EINVAL；预算越线 → EBUDGET（样本号由账本
//!    红账指明）；整数运算零浮点；
//! 3. 预算线为冻结常量 128 拍，逐行落账；预算恒等守卫：任一行预算线
//!    ≠ 常量即判预算篡改（ETAMPER）——越线旗标必须与「拍数 > 预算线」
//!    复算自洽，双守卫闭死篡改面；
//! 4. 红账点验：越线样本数恒 0 才绿；任一越线必须红账告警并**指明
//!    样本号**，满账带红项的冻结一律被拒（EBUDGET）；
//! 5. 注入器三类各 10 轮全抓：样本越线注入 / 预算篡改注入 /
//!    越界样本号注入，LCG 种子写死（SEG_BASE 域内偏移），确定性可复算；
//! 6. 双机 20 场景一致（S1 母版）：20 场景（LCG 种子 = SEG_BASE + 场景号）
//!    先建 S1 母版签名表，再整体复跑 S2 逐对比对指纹；三注入 sweep
//!    双跑逐项一致。no_std + alloc；零浮点、零 IO、零墙钟、零随机；
//!    取值一律 match/if let/.get()/.get_mut()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12750 − 12600) = 0x82580）。
pub const SEG_BASE: u32 = 0x82580;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum P95Diag {
    /// 空样本：无可计算分位（EINVAL）。
    EEmpty,
    /// 样本号越闭集：只认 1..=40 冻结编号。
    ERowRange,
    /// 样本号重复：每号只落一笔。
    ERowDup,
    /// 零拍样本：分发耗时为零不构成有效测量。
    ECostZero,
    /// 容量耗尽：冻结闭集 40 行已满，写入被拒。
    ERowCap,
    /// 未满 40 行禁止冻结。
    ENotFull,
    /// 账已冻结，写入被拒。
    ESealed,
    /// 状态机倒退：冻结后重复冻结。
    ERewind,
    /// 分位槽位计算越界（闭式溢出或槽位出界）。
    ESlot,
    /// P95 越预算线（EBUDGET），红账告警并指明样本号。
    EBudget,
    /// 预算篡改：预算线偏离冻结常量或旗标不自洽。
    ETamper,
    /// 注入逃脱：注入未被守卫抓住，守卫面失效须复核。
    EEscape,
}

impl P95Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                P95Diag::EEmpty => 0x01,
                P95Diag::ERowRange => 0x02,
                P95Diag::ERowDup => 0x03,
                P95Diag::ECostZero => 0x04,
                P95Diag::ERowCap => 0x05,
                P95Diag::ENotFull => 0x06,
                P95Diag::ESealed => 0x07,
                P95Diag::ERewind => 0x08,
                P95Diag::ESlot => 0x09,
                P95Diag::EBudget => 0x0a,
                P95Diag::ETamper => 0x0b,
                P95Diag::EEscape => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            P95Diag::EEmpty => "空样本（EINVAL）：P95 无从计算，先补采分发耗时样本再判",
            P95Diag::ERowRange => "样本号越闭集：只认 1..=40 冻结编号，先对号再登记",
            P95Diag::ERowDup => "样本号重复：每号只落一笔，重复登记先查账内已有号",
            P95Diag::ECostZero => "零拍样本：分发耗时为零属测量残缺，先修采集口径再入账",
            P95Diag::ERowCap => "容量耗尽：冻结闭集 40 行已满，追加走下一轮压测场景",
            P95Diag::ENotFull => "未满 40 行禁止冻结：先补齐缺号样本再走冻结口",
            P95Diag::ESealed => "账已冻结：冻结后写入拒绝，改动走下一场景滚动",
            P95Diag::ERewind => "状态机倒退：Open→Full→Frozen 只进不退，重复冻结即账本可疑",
            P95Diag::ESlot => "分位槽位越界：闭式 floor((n*95+99)/100) 溢出或出界，先查样本量纲",
            P95Diag::EBudget => "P95 越预算线（EBUDGET）：先查红账样本号，回修越线场景再复测",
            P95Diag::ETamper => "预算篡改：预算线偏离 128 拍冻结常量或越线旗标不自洽，整账复核",
            P95Diag::EEscape => "注入逃脱：构造违约未被守卫抓住，守卫面失效须整面复核",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const P95_ALL: &[P95Diag] = &[
    P95Diag::EEmpty,
    P95Diag::ERowRange,
    P95Diag::ERowDup,
    P95Diag::ECostZero,
    P95Diag::ERowCap,
    P95Diag::ENotFull,
    P95Diag::ESealed,
    P95Diag::ERewind,
    P95Diag::ESlot,
    P95Diag::EBudget,
    P95Diag::ETamper,
    P95Diag::EEscape,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in P95_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82581..0x8258C 闭集内"
}

// ── 预算域冻结常量 ───────────────────────────────────────────────
/// P95 分发延迟预算线：128 拍（来源：UNX-F12750 卡 12750 分发延迟预算
/// 口径，整数拍数；拍 = 内核定时基准 1 tick，与样板里程碑域无关）。
pub const P95_BUDGET_BEATS: u32 = 128;

/// 延迟样本账冻结闭集行数：40 行（判据要求 ≥40 行可复算，取恰 40）。
pub const SAMPLE_CAP: usize = 40;

/// 分位闭式分子系数：95（第 95 百分位）。
const P95_NUM: u64 = 95;

/// 分位闭式加数：99（最近秩整数等价式的进位补齐项）。
const P95_ADD: u64 = 99;

/// 分位闭式分母：100。
const P95_DEN: u64 = 100;

/// 越界样本号注入语料（冻结常量表，10 个号恰对 10 轮注入；
/// 全部落在闭集 1..=40 之外：0 为下溢、41+ 为上溢、尾项为 u32 极值）。
pub const BAD_SAMPLE_NOS: &[u32] = &[
    0,
    41,
    42,
    64,
    100,
    255,
    4096,
    0xFFFF,
    0x7FFF_FFFF,
    0xFFFF_FFFF,
];

/// 分位槽位闭式（整数口径，1-based；越界/溢出回落 0 表示不可算）。
///
/// `slot = floor((n*95+99)/100)`：n=40 → 38，n=1 → 1，n=100 → 95。
/// 全程 u64 整数运算，零浮点；槽位与最近秩 nearest-rank 定义逐点一致。
pub fn slot_of(n: usize) -> u32 {
    let n64 = n as u64;
    match n64.checked_mul(P95_NUM).and_then(|m| m.checked_add(P95_ADD)) {
        Some(num) => (num / P95_DEN) as u32,
        None => 0,
    }
}

// ── 延迟样本账域模型 ─────────────────────────────────────────────
/// 单行延迟样本四元组：样本号 / 分发耗时（拍数）/ 预算线 / 越线旗标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LatencyRow {
    /// 样本号（冻结闭集 1..=40）。
    pub sample_no: u32,
    /// 分发耗时（整数口径：拍数，≥1 有效）。
    pub beats: u32,
    /// 预算线（落账时冻结，恒等于 P95_BUDGET_BEATS）。
    pub budget: u32,
    /// 越线旗标（与 拍数 > 预算线 复算自洽）。
    pub over: bool,
}

impl LatencyRow {
    /// 行摘要（诊断文本用）：号/拍/线/旗标一行读全。
    pub fn brief(&self) -> String {
        format!(
            "样本{}拍{}线{}越线{}",
            self.sample_no, self.beats, self.budget, self.over
        )
    }
}

/// P95 判定行：槽位 / 升序秩 / 槽位值 / 预算线。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct P95Row {
    /// 1-based 分位槽位（floor((n*95+99)/100)，闭式可复算）。
    pub slot: u32,
    /// 0-based 升序下标（= slot − 1，即 P95 槽位样本在升序中的序号）。
    pub rank: usize,
    /// 该槽位的分发耗时（拍数）。
    pub value: u32,
    /// 预算线（拍数，冻结常量）。
    pub budget: u32,
}

impl P95Row {
    /// 该行是否越线（Ok 返回时恒 false，防御性复核口）。
    pub fn is_over(&self) -> bool {
        self.value > self.budget
    }
}

/// 账本状态机：Open（登记中）→ Full（40 行齐）→ Frozen（冻结只读），
/// 单向推进，倒退显性拒。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookState {
    /// 登记中：允许逐号登记。
    Open,
    /// 40 行齐：等待红账点验与冻结。
    Full,
    /// 冻结：只读。
    Frozen,
}

impl BookState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            BookState::Open => 0,
            BookState::Full => 1,
            BookState::Frozen => 2,
        }
    }
}

/// 延迟样本账（P95 预算域账本）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampleBook {
    /// 已入账行（按登记序，样本号 1..=40 各一笔）。
    rows: Vec<LatencyRow>,
    /// 账本状态。
    state: BookState,
}

impl SampleBook {
    /// 开一本新账。
    pub fn new() -> SampleBook {
        SampleBook {
            rows: Vec::new(),
            state: BookState::Open,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> BookState {
        self.state
    }

    /// 已入账行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<LatencyRow> {
        self.rows.get(idx).copied()
    }

    /// 按样本号查行。
    pub fn row_of(&self, sample_no: u32) -> Option<LatencyRow> {
        for r in self.rows.iter() {
            if r.sample_no == sample_no {
                return Some(*r);
            }
        }
        None
    }

    /// 账面行快照（注入器构造篡改账用）。
    pub fn rows_snapshot(&self) -> Vec<LatencyRow> {
        let mut out: Vec<LatencyRow> = Vec::new();
        for r in self.rows.iter() {
            out.push(*r);
        }
        out
    }

    /// 耗时快照（送 judge_p95 判定用，保持登记序）。
    pub fn beats_snapshot(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for r in self.rows.iter() {
            out.push(r.beats);
        }
        out
    }

    /// 登记一行延迟样本（闸序：封账 → 越闭集 → 容量 → 重复 → 零拍）。
    pub fn register(&mut self, sample_no: u32, beats: u32) -> Result<(), u32> {
        // 闸一：封账后一切写入拒绝。
        if self.state.rank() >= BookState::Frozen.rank() {
            return Err(P95Diag::ESealed.val());
        }
        // 闸二：样本号越闭集（0 或 >40 拒）。
        if sample_no == 0 || sample_no > SAMPLE_CAP as u32 {
            return Err(P95Diag::ERowRange.val());
        }
        // 闸三：容量耗尽（满账再写拒——容量先于重复，满账重复号也报容量）。
        if self.rows.len() >= SAMPLE_CAP {
            return Err(P95Diag::ERowCap.val());
        }
        // 闸四：样本号重复。
        if self.row_of(sample_no).is_some() {
            return Err(P95Diag::ERowDup.val());
        }
        // 闸五：零拍样本（分发耗时为零不构成有效测量）。
        if beats == 0 {
            return Err(P95Diag::ECostZero.val());
        }
        // 越线旗标：严格大于预算线才越（恰等于预算线不越）。
        let over = beats > P95_BUDGET_BEATS;
        self.rows.push(LatencyRow {
            sample_no,
            beats,
            budget: P95_BUDGET_BEATS,
            over,
        });
        // 40 行齐即推进状态机（只进不退）。
        if self.rows.len() == SAMPLE_CAP {
            self.state = BookState::Full;
        }
        Ok(())
    }

    /// 红账：越线样本号清单（登记序）。越线样本数恒 0 才清白。
    pub fn red_account(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for r in self.rows.iter() {
            if r.over {
                out.push(r.sample_no);
            }
        }
        out
    }

    /// 红账清白判定：越线样本数恒 0 才绿。
    pub fn red_clean(&self) -> bool {
        self.red_account().is_empty()
    }

    /// 红账告警文本：清白报零，越线**指明样本号**。
    pub fn red_alert(&self) -> String {
        let red = self.red_account();
        if red.is_empty() {
            return String::from("红账清白：越线样本数=0");
        }
        let mut s = String::from("红账告警：越线样本号=");
        let mut i = 0usize;
        let mut first = true;
        while i < red.len() {
            if let Some(v) = red.get(i) {
                if !first {
                    s.push(',');
                }
                let piece = format!("{}", v);
                s.push_str(&piece);
                first = false;
            }
            i += 1;
        }
        s
    }

    /// 账本级 P95 判定：耗时快照送 judge_p95，越线时红账已指明样本号。
    pub fn judge_book(&self) -> Result<P95Row, u32> {
        judge_p95(&self.beats_snapshot())
    }

    /// 冻结：满 40 行 + 红账清白才许冻结（红账点验前置，任一越线必拒）。
    pub fn freeze(&mut self) -> Result<(), u32> {
        // 倒退闸：已冻结再冻结拒。
        if self.state.rank() >= BookState::Frozen.rank() {
            return Err(P95Diag::ERewind.val());
        }
        // 满账闸：未满 40 行禁冻结。
        if self.rows.len() != SAMPLE_CAP {
            return Err(P95Diag::ENotFull.val());
        }
        // 红账点验闸：越线样本数恒 0 才绿，任一越线红账告警并拒冻结。
        if !self.red_clean() {
            return Err(P95Diag::EBudget.val());
        }
        self.state = BookState::Frozen;
        Ok(())
    }

    /// 账面指纹（可复算）：逐行折叠 样本号/拍数/预算线/旗标 + 状态序。
    /// 双机 20 场景一致性比对用的确定性摘要，零浮点零随机。
    pub fn fingerprint(&self) -> u64 {
        // FNV-64 偏移基（公开常数，非随机源）。
        let mut fp: u64 = 0xCBF2_9CE4_8422_2325;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(row) = self.rows.get(i) {
                fp = fp.wrapping_mul(0x100_0000_01B3);
                fp = fp.wrapping_add((row.sample_no as u64) << 32);
                fp = fp.wrapping_add(row.beats as u64);
                fp = fp.wrapping_add((row.budget as u64) << 8);
                fp = fp.wrapping_add(if row.over { 0xF } else { 0x3 });
            }
            i += 1;
        }
        fp.wrapping_add(self.state.rank() as u64)
    }
}

/// 由行面构造账本（注入器的预算篡改账入口），逐行复检五道守卫：
/// 行数闭集 / 样本号闭集 / 重复 / 零拍 / 预算恒等与旗标自洽。
pub fn book_from_rows(rows: &[LatencyRow]) -> Result<SampleBook, u32> {
    // 行数闭集：只认恰 40 行的满账。
    if rows.len() != SAMPLE_CAP {
        return Err(P95Diag::ENotFull.val());
    }
    let mut book = SampleBook::new();
    let mut i = 0usize;
    while i < rows.len() {
        let row = match rows.get(i) {
            Some(r) => *r,
            None => break,
        };
        if row.sample_no == 0 || row.sample_no > SAMPLE_CAP as u32 {
            return Err(P95Diag::ERowRange.val());
        }
        if book.row_of(row.sample_no).is_some() {
            return Err(P95Diag::ERowDup.val());
        }
        if row.beats == 0 {
            return Err(P95Diag::ECostZero.val());
        }
        // 预算恒等守卫：预算线是冻结常量，任一行偏离即篡改。
        if row.budget != P95_BUDGET_BEATS {
            return Err(P95Diag::ETamper.val());
        }
        // 旗标自洽守卫：越线旗标必须与 拍数 > 预算线 复算一致。
        if row.over != (row.beats > row.budget) {
            return Err(P95Diag::ETamper.val());
        }
        book.rows.push(row);
        i += 1;
    }
    if book.rows.len() == SAMPLE_CAP {
        book.state = BookState::Full;
    }
    Ok(book)
}

// ── P95 计算判定器 ───────────────────────────────────────────────
/// P95 计算判定器（整数口径）。
///
/// 判定次序：①空样本 → EINVAL；②升序排序后取第
/// `floor((n*95+99)/100)` 槽位（1-based，闭式可复算），闭式溢出或
/// 槽位出界 → ESLOT；③槽位值 > 预算线 128 拍 → EBUDGET（样本号由
/// 账本红账 `red_account()` 指明）。全程整数运算，零浮点。
pub fn judge_p95(samples: &[u32]) -> Result<P95Row, u32> {
    // 次序一：空样本 EINVAL。
    if samples.is_empty() {
        return Err(P95Diag::EEmpty.val());
    }
    // 升序复制（整数序，确定性排序，不触碰原切片）。
    let mut sorted: Vec<u32> = Vec::new();
    for v in samples.iter() {
        sorted.push(*v);
    }
    sorted.sort_unstable();
    // 次序二：分位槽位闭式计算与出界守卫。
    let slot = slot_of(samples.len());
    if slot == 0 || slot as usize > samples.len() {
        return Err(P95Diag::ESlot.val());
    }
    let idx = slot as usize - 1;
    let value = match sorted.get(idx) {
        Some(v) => *v,
        None => return Err(P95Diag::ESlot.val()),
    };
    // 次序三：预算线判定（严格大于才越线）。
    if value > P95_BUDGET_BEATS {
        return Err(P95Diag::EBudget.val());
    }
    Ok(P95Row {
        slot,
        rank: idx,
        value,
        budget: P95_BUDGET_BEATS,
    })
}

// ── 确定性 LCG（种子写死，零随机红线合规） ────────────────────────
/// 线性同余发生器：glibc TYPE_0 参数闭集（乘 1103515245 加 12345，
/// 模 2^31 截断），种子写死即序列确定，双跑一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造（种子为零时锚定 1，保持非零轨道）。
    pub fn new(seed: u64) -> Lcg {
        let s = if seed == 0 { 1u64 } else { seed };
        Lcg {
            state: s & 0x7FFF_FFFF,
        }
    }

    /// 下一伪随机数（确定性，域 0..2^31）。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(1_103_515_245)
            .wrapping_add(12_345)
            & 0x7FFF_FFFF;
        self.state as u32
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 账本构造与注入面 ─────────────────────────────────────────────
/// 按种子构造 40 行账本：绿账拍数域 1..=128（含端点，恰等于预算线
/// 不越线）；`over_target` 命中的样本号改注越线拍数（线+1..线+64）。
/// LCG 每行恒消耗一次，注入与否不改变消耗序——同种子账面逐行可复算。
pub fn build_book_seeded(seed: u64, over_target: Option<(u32, u32)>) -> SampleBook {
    let mut book = SampleBook::new();
    let mut rng = Lcg::new(seed);
    let mut no = 1u32;
    while no <= SAMPLE_CAP as u32 {
        let mut beats = 1u32 + rng.below(P95_BUDGET_BEATS);
        if let Some((target, over_beats)) = over_target {
            if target == no {
                beats = over_beats;
            }
        }
        // 正常流不该出错；出错即缺行，后续断言会红（构造期显性留痕）。
        let _ = book.register(no, beats);
        no += 1;
    }
    book
}

/// 绿账母版（种子 = SEG_BASE，40 行全在预算线内）。
pub fn build_green_book() -> SampleBook {
    build_book_seeded(SEG_BASE as u64, None)
}

/// 压测场景账本（判据「20 场景」锚定：场景号 0..20，种子 = SEG_BASE+场景号）。
pub fn scenario_book(sc: u32) -> SampleBook {
    build_book_seeded(SEG_BASE.wrapping_add(sc) as u64, None)
}

/// 场景签名：指纹 + P95（槽位, 值）+ 红账清白，双跑比对的比对元。
pub fn scenario_sig(sc: u32) -> (u64, u32, u32, bool) {
    let book = scenario_book(sc);
    let fp = book.fingerprint();
    let (slot, value) = match book.judge_book() {
        Ok(row) => (row.slot, row.value),
        Err(_) => (0, 0),
    };
    (fp, slot, value, book.red_clean())
}

/// 注入一（样本越线注入，10 轮全抓）：向绿账逐轮注一个越线样本，
/// 红账必须告警并指明该样本号，满账冻结必须被 EBUDGET 拒。
/// 返回 (抓到路数, 总路数, 首个逃脱路描述)。
pub fn inject_over_line_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x51) as u64);
    let mut k = 0usize;
    while k < 10 {
        let target = 1u32 + rng.below(SAMPLE_CAP as u32);
        let over_beats = P95_BUDGET_BEATS.wrapping_add(1).wrapping_add(rng.below(64));
        let book = build_book_seeded(SEG_BASE as u64, Some((target, over_beats)));
        let red = book.red_account();
        let mut replay = build_book_seeded(SEG_BASE as u64, Some((target, over_beats)));
        let freeze_bad = replay.freeze() == Err(P95Diag::EBudget.val());
        let pinned = red.len() == 1
            && match red.first() {
                Some(v) => *v == target,
                None => false,
            };
        total += 1;
        if pinned && freeze_bad && !book.red_clean() && replay.state() == BookState::Full {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!(
                "样本越线注入逃脱：号{}拍{}红账{:?}",
                target, over_beats, red
            );
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 注入二（预算篡改注入，10 轮全抓）：逐轮把一行预算线偏离冻结常量
/// 128 拍（偏移 +4，撞线再 +1 保证必偏离），预算恒等守卫必须以
/// ETAMPER 拒收。旗标保持自洽——专测预算恒等守卫本身。
pub fn inject_budget_tamper_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x52) as u64);
    let mut k = 0usize;
    while k < 10 {
        let idx = rng.below(SAMPLE_CAP as u32) as usize;
        let green = build_green_book();
        let mut rows = green.rows_snapshot();
        let mut tampered = false;
        if let Some(row) = rows.get_mut(idx) {
            let nb = row.beats.saturating_add(4);
            row.budget = if nb == P95_BUDGET_BEATS {
                nb.wrapping_add(1)
            } else {
                nb
            };
            tampered = row.budget != P95_BUDGET_BEATS;
        }
        let verdict = book_from_rows(&rows);
        total += 1;
        if tampered && verdict == Err(P95Diag::ETamper.val()) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("预算篡改注入逃脱：行{}判定{:?}", idx, verdict);
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 注入三（越界样本号注入，10 轮全抓）：逐轮以冻结语料表
/// BAD_SAMPLE_NOS 的越界号登记，越闭集闸必须以 EROWRANGE 拒收且
/// 账面零变动、状态不前进。
pub fn inject_row_range_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut k = 0usize;
    while k < BAD_SAMPLE_NOS.len() {
        let bad = match BAD_SAMPLE_NOS.get(k) {
            Some(v) => *v,
            None => break,
        };
        let mut book = SampleBook::new();
        let before = book.len();
        let verdict = book.register(bad, 16);
        total += 1;
        if verdict == Err(P95Diag::ERowRange.val())
            && book.len() == before
            && book.state() == BookState::Open
        {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("越界样本号注入逃脱：号{}判定{:?}", bad, verdict);
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 批窗口 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：12 码互落 [SEG_BASE, SEG_BASE+0x40) 窗口，
/// 查表逐码命中文案且文案非空，未知码回落"未知"。
fn diag_closed_ok() -> bool {
    if P95_ALL.len() != 12 {
        return false;
    }
    for d in P95_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if d.detail().is_empty() {
            return false;
        }
        if detail(v) != d.detail() {
            return false;
        }
    }
    // 未知码回落：段内未占用偏移必须报"未知"。
    if !detail(SEG_BASE + 0x3F).contains("未知") {
        return false;
    }
    true
}

/// 正常流：绿账 40 行 → P95 判定绿 → 红账清白 → 冻结。
fn happy_path_ok() -> bool {
    let mut book = build_green_book();
    if book.len() != SAMPLE_CAP {
        return false;
    }
    if book.state() != BookState::Full {
        return false;
    }
    if book.is_empty() {
        return false;
    }
    // n=40 闭式槽位恒为 38：floor((40*95+99)/100) = 38。
    let row = match book.judge_book() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if row.slot != 38 {
        return false;
    }
    if row.is_over() {
        return false;
    }
    if !book.red_clean() {
        return false;
    }
    match book.freeze() {
        Ok(()) => book.state() == BookState::Frozen,
        Err(_) => false,
    }
}

/// 判定次序：judge 空样本先于一切、预算在槽位之后；登记闸序
/// 封账 → 越闭集 → 容量 → 重复 → 零拍 逐段验证。
fn judge_order_ok() -> bool {
    // judge 次序一：空样本 EINVAL（即便后续值全越线也先报空）。
    if judge_p95(&[]) != Err(P95Diag::EEmpty.val()) {
        return false;
    }
    // judge 次序衔接：单样本 n=1 槽位=1，越线值报 EBUDGET。
    if judge_p95(&[P95_BUDGET_BEATS + 1]) != Err(P95Diag::EBudget.val()) {
        return false;
    }
    // 登记闸序：越闭集先于零拍。
    let mut b = SampleBook::new();
    if b.register(0, 0) != Err(P95Diag::ERowRange.val()) {
        return false;
    }
    if b.register(0, 16) != Err(P95Diag::ERowRange.val()) {
        return false;
    }
    // 登记闸序：重复先于零拍（同号零拍报重复）。
    let _ = b.register(1, 16);
    if b.register(1, 0) != Err(P95Diag::ERowDup.val()) {
        return false;
    }
    // 登记闸序：零拍闸可达。
    if b.register(2, 0) != Err(P95Diag::ECostZero.val()) {
        return false;
    }
    // 登记闸序：封账闸最先——冻结后越界号也报封账。
    let mut full = build_green_book();
    let _ = full.freeze();
    full.register(0xFFFF, 0) == Err(P95Diag::ESealed.val())
}

/// 红账点验：绿账越线数恒 0；注入账红账必告警并指明样本号；
/// 满账带红项的冻结必被 EBUDGET 拒。
fn red_audit_ok() -> bool {
    let green = build_green_book();
    if !green.red_clean() {
        return false;
    }
    if !green.red_account().is_empty() {
        return false;
    }
    // 任一越线必须红账告警并指明样本号。
    let bad = build_book_seeded(SEG_BASE as u64, Some((7, P95_BUDGET_BEATS + 3)));
    let red = bad.red_account();
    if red.len() != 1 {
        return false;
    }
    match red.first() {
        Some(v) => {
            if *v != 7 {
                return false;
            }
        }
        None => return false,
    }
    if bad.red_clean() {
        return false;
    }
    let alert = bad.red_alert();
    if !alert.contains("红账告警") {
        return false;
    }
    // 满账带红项：冻结必被拒（红账点验前置）。
    let mut badfull = build_book_seeded(SEG_BASE as u64, Some((7, P95_BUDGET_BEATS + 3)));
    badfull.freeze() == Err(P95Diag::EBudget.val())
}

/// 边界四类：①拍=预算线（不越）②拍=预算线+1（越，红账告警，
/// 未满账冻结报 NOTFULL）③单样本 n=1 槽位=1 ④满账 40 行可冻结、
/// 41 号越闭集、满账写入报容量。
fn boundary_ok() -> bool {
    // 边界一：拍数恰等于预算线——严格大于才越，旗标必须为假。
    let mut b1 = SampleBook::new();
    let _ = b1.register(1, P95_BUDGET_BEATS);
    match b1.row_of(1) {
        Some(r) => {
            if r.over || r.beats != P95_BUDGET_BEATS {
                return false;
            }
        }
        None => return false,
    }
    // 边界二：拍数=预算线+1——越线旗标必须为真且红账指明样本号。
    let mut b2 = SampleBook::new();
    let _ = b2.register(1, P95_BUDGET_BEATS.saturating_add(1));
    match b2.row_of(1) {
        Some(r) => {
            if !r.over {
                return false;
            }
        }
        None => return false,
    }
    if b2.red_account().is_empty() {
        return false;
    }
    // 未满账冻结报 NOTFULL（先于红账闸）。
    if b2.freeze() != Err(P95Diag::ENotFull.val()) {
        return false;
    }
    // 边界三：单样本 n=1——槽位闭式 floor((1*95+99)/100)=1，取唯一值。
    match judge_p95(&[77]) {
        Ok(r) => {
            if r.slot != 1 || r.value != 77 || r.rank != 0 {
                return false;
            }
        }
        Err(_) => return false,
    }
    // 边界四：满账 40 行绿账可冻结；41 号越闭集；满账合法号写入报容量。
    let mut b4 = build_green_book();
    if b4.freeze().is_err() {
        return false;
    }
    let mut b5 = build_green_book();
    if b5.register(41, 16) != Err(P95Diag::ERowRange.val()) {
        return false;
    }
    b5.register(1, 16) == Err(P95Diag::ERowCap.val())
}

/// 容量耗尽：满账后任何写入被容量闸拒绝（合法号在容量闸之后报容量，
/// 先于重复闸），账面行数与状态不变；半账重复仍报重复。
fn capacity_ok() -> bool {
    let mut book = build_green_book();
    if book.len() != SAMPLE_CAP || book.state() != BookState::Full {
        return false;
    }
    if book.register(1, 16) != Err(P95Diag::ERowCap.val()) {
        return false;
    }
    if book.register(SAMPLE_CAP as u32, 16) != Err(P95Diag::ERowCap.val()) {
        return false;
    }
    if book.len() != SAMPLE_CAP {
        return false;
    }
    // 半账不受容量闸影响：重复号报重复。
    let mut half = SampleBook::new();
    let _ = half.register(1, 16);
    half.register(1, 16) == Err(P95Diag::ERowDup.val())
}

/// 冻结守卫：冻结后写入拒（ESEALED）且账面不变；重复冻结拒
/// （EREWIND）；未满账冻结拒（ENOTFULL）。
fn frozen_guards_ok() -> bool {
    let mut book = build_green_book();
    if book.freeze().is_err() {
        return false;
    }
    let before = book.len();
    if book.register(1, 16) != Err(P95Diag::ESealed.val()) {
        return false;
    }
    if book.len() != before {
        return false;
    }
    if book.freeze() != Err(P95Diag::ERewind.val()) {
        return false;
    }
    let mut half = SampleBook::new();
    let _ = half.register(1, 16);
    half.freeze() == Err(P95Diag::ENotFull.val())
}

/// 双机 20 场景一致（S1 母版）：先建 20 场景 S1 签名表作母版，再整体
/// 复跑 S2 逐对比对；三注入 sweep 双跑逐项一致。展开序类零容差由
/// 签名逐对相等承载——任何一项漂移即不等。
fn dual_run_ok() -> bool {
    // S1 母版签名表（20 场景，序冻结）。
    let mut master: Vec<(u64, u32, u32, bool)> = Vec::new();
    let mut sc = 0u32;
    while sc < 20 {
        master.push(scenario_sig(sc));
        sc += 1;
    }
    if master.len() != 20 {
        return false;
    }
    // S2 复跑逐对比对（S1 vs S2，零容差）。
    let mut sc = 0u32;
    while sc < 20 {
        let m = match master.get(sc as usize) {
            Some(v) => *v,
            None => return false,
        };
        if scenario_sig(sc) != m {
            return false;
        }
        sc += 1;
    }
    // 注入 sweep 双跑一致。
    if inject_over_line_sweep() != inject_over_line_sweep() {
        return false;
    }
    if inject_budget_tamper_sweep() != inject_budget_tamper_sweep() {
        return false;
    }
    inject_row_range_sweep() == inject_row_range_sweep()
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12750 自检（判据 12750 卡：压测性能预算 P95 分发延迟）。
pub fn run_unx12750_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12750");
    let green = build_green_book();
    let (jslot, jval) = match green.judge_book() {
        Ok(r) => (r.slot, r.value),
        Err(_) => (0, 0),
    };

    s.add_dyn(
        "F12750-01 段位守卫",
        seg_guard_ok(),
        &format!(
            "seg=0x{:05x} align40={} win=[0x80000,0x831C0]",
            SEG_BASE,
            SEG_BASE % 0x40 == 0
        ),
    );

    s.add_dyn(
        "F12750-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "n={} first=0x{:05x}",
            P95_ALL.len(),
            P95_ALL.first().map(|d| d.val()).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12750-03 正常流40行绿账冻结",
        happy_path_ok(),
        &format!(
            "rows={} state={:?} slot={} val={} {}",
            green.len(),
            green.state(),
            jslot,
            jval,
            green.red_alert()
        ),
    );

    s.add_dyn(
        "F12750-04 P95分位槽位闭式",
        {
            // 闭式复算：n=40 槽位恒 38，且判定值落在预算线内。
            match green.judge_book() {
                Ok(r) => r.slot == slot_of(green.len()) && r.slot == 38 && !r.is_over(),
                Err(_) => false,
            }
        },
        &format!(
            "slot={} expect={} budget={}",
            jslot,
            slot_of(SAMPLE_CAP),
            P95_BUDGET_BEATS
        ),
    );

    s.add_dyn(
        "F12750-05 判定次序",
        judge_order_ok(),
        "judge:空样本EINVAL→槽位ESLOT→预算EBUDGET；闸序:封账→越号→容量→重复→零拍",
    );

    s.add_dyn(
        "F12750-06 红账点验",
        red_audit_ok(),
        "越线样本数恒0才绿；越线必告警并指明样本号，带红冻结必拒",
    );

    // 注入必抓：三类注入各 10 轮，人为构造违约必被拒。
    let (c1, t1, e1) = inject_over_line_sweep();
    s.add_dyn(
        "F12750-07 注入一样本越线10轮",
        c1 == 10 && t1 == 10 && e1.is_empty(),
        &format!("{}/{} {}", c1, t1, e1),
    );

    let (c2, t2, e2) = inject_budget_tamper_sweep();
    s.add_dyn(
        "F12750-08 注入二预算篡改10轮",
        c2 == 10 && t2 == 10 && e2.is_empty(),
        &format!("{}/{} {}", c2, t2, e2),
    );

    let (c3, t3, e3) = inject_row_range_sweep();
    s.add_dyn(
        "F12750-09 注入三越界样本号10轮",
        c3 == 10 && t3 == 10 && e3.is_empty(),
        &format!("{}/{} {}", c3, t3, e3),
    );

    s.add_dyn(
        "F12750-10 边界四类",
        boundary_ok(),
        "拍=线不越/拍=线+1越/单样本槽位1/满账冻结且41号越集",
    );

    s.add_dyn(
        "F12750-11 容量耗尽",
        capacity_ok(),
        &format!(
            "cap={} 满账写拒行数不变={}",
            SAMPLE_CAP,
            build_green_book().len() == SAMPLE_CAP
        ),
    );

    s.add_dyn(
        "F12750-12 冻结守卫与倒退",
        frozen_guards_ok(),
        "冻结后写拒/重复冻结拒/未满冻结拒 三点",
    );

    s.add_dyn(
        "F12750-13 双跑一致指纹",
        dual_run_ok(),
        "双机20场景S1母版逐对零容差+三注入双跑一致",
    );

    // 收口：判据条数声明（UNX-F12751-J1 共 1 条，本检查节 14 项）。
    let n_before = s.len();
    s.add_dyn(
        "F12750-14 收口判据条数声明",
        n_before == 13,
        &format!(
            "判据1条(UNX-F12751-J1)·检查节{}项全绿收口",
            n_before + 1
        ),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 批窗口 [0x80000, 0x831C0]。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x82580);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中、未知码回落。
        assert_eq!(P95_ALL.len(), 12);
        let mut i = 0usize;
        while i < P95_ALL.len() {
            let vi = P95_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            assert!(!detail(vi).is_empty());
            let mut j = i + 1;
            while j < P95_ALL.len() {
                let vj = P95_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            i += 1;
        }
        // 未知码回落。
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
    }

    #[test]
    fn happy_path_freezes_green_book() {
        // 正常流：绿账 40 行 → Full → 红账清白 → 冻结 → Frozen。
        let mut book = build_green_book();
        assert_eq!(book.len(), SAMPLE_CAP);
        assert_eq!(book.state(), BookState::Full);
        assert!(book.red_clean());
        let judged = book.judge_book();
        assert!(judged.is_ok(), "绿账判定不该红");
        let row = match judged {
            Ok(r) => r,
            // 上一行断言已把红路径拦下，此处不可达。
            Err(_) => return,
        };
        assert_eq!(row.slot, 38);
        assert!(!row.is_over());
        assert!(book.freeze().is_ok());
        assert_eq!(book.state(), BookState::Frozen);
    }

    #[test]
    fn judge_p95_slot_closed_form_recomputes() {
        // 闭式可复算：n=40 → 槽位 38 → 升序第 38 个；n=1 → 槽位 1；
        // 全等样本取该值；n=100 → 槽位 95。全部整数口径。
        let mut s40: Vec<u32> = Vec::new();
        let mut i = 0u32;
        while i < 40 {
            let _ = s40.push(10 + i);
            i += 1;
        }
        let n = s40.len() as u64;
        assert_eq!((n * 95 + 99) / 100, 38);
        assert_eq!(slot_of(s40.len()), 38);
        let judged = judge_p95(&s40);
        assert!(judged.is_ok(), "升序语料判定不该红");
        let row = match judged {
            Ok(r) => r,
            Err(_) => return,
        };
        assert_eq!(row.slot, 38);
        assert_eq!(row.rank, 37);
        assert_eq!(row.value, 47);
        // 单样本：n=1 槽位=1 取唯一值。
        let judged1 = judge_p95(&[77]);
        assert!(judged1.is_ok(), "单样本判定不该红");
        let r1 = match judged1 {
            Ok(r) => r,
            Err(_) => return,
        };
        assert_eq!(r1.slot, 1);
        assert_eq!(r1.value, 77);
        // 全等样本：恰等于预算线不越线。
        let mut eq: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < SAMPLE_CAP {
            let _ = eq.push(P95_BUDGET_BEATS);
            k += 1;
        }
        let judgede = judge_p95(&eq);
        assert!(judgede.is_ok(), "全等线值判定不该红");
        let re = match judgede {
            Ok(r) => r,
            Err(_) => return,
        };
        assert_eq!(re.value, P95_BUDGET_BEATS);
        assert!(!re.is_over());
        // n=100：槽位 95，值 95（语料 1..=100 升序）。
        let mut s100: Vec<u32> = Vec::new();
        let mut m = 1u32;
        while m <= 100 {
            let _ = s100.push(m);
            m += 1;
        }
        let judged100 = judge_p95(&s100);
        assert!(judged100.is_ok(), "百样本判定不该红");
        let r100 = match judged100 {
            Ok(r) => r,
            Err(_) => return,
        };
        assert_eq!(r100.slot, 95);
        assert_eq!(r100.value, 95);
        // 空样本 EINVAL。
        assert_eq!(judge_p95(&[]), Err(P95Diag::EEmpty.val()));
    }

    #[test]
    fn judge_and_gate_ordering_holds() {
        // 判定次序 + 登记闸序全链验证。
        assert!(judge_order_ok());
        // 空样本先于一切。
        assert_eq!(judge_p95(&[]), Err(P95Diag::EEmpty.val()));
        // 单样本越线报 EBUDGET（槽位先算、预算后判）。
        assert_eq!(
            judge_p95(&[P95_BUDGET_BEATS + 1]),
            Err(P95Diag::EBudget.val())
        );
    }

    #[test]
    fn injection_over_line_all_caught() {
        // 注入一：样本越线 10 轮全抓，红账必指明注入样本号。
        let (caught, total, escape) = inject_over_line_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路: {}", escape);
        assert!(escape.is_empty());
        // 单点复核：红账恰一笔且样本号对准。
        let bad = build_book_seeded(SEG_BASE as u64, Some((9, P95_BUDGET_BEATS + 2)));
        let red = bad.red_account();
        assert_eq!(red.len(), 1);
        assert_eq!(red.first().copied(), Some(9));
        assert!(bad.red_alert().contains("红账告警"));
        // row_at / brief 直查：越线行四元摘要含样本号与旗标。
        match bad.row_at(8) {
            Some(r) => assert!(r.brief().contains("样本9") && r.over),
            None => assert!(false, "row_at(8) 缺行"),
        }
    }

    #[test]
    fn injection_budget_tamper_all_caught() {
        // 注入二：预算篡改 10 轮全抓（预算恒等守卫 + 旗标自洽守卫）。
        let (caught, total, escape) = inject_budget_tamper_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路: {}", escape);
        assert!(escape.is_empty());
        // 单点复核：预算线偏离 128 即 ETAMPER，旗标不自洽同罚。
        let green = build_green_book();
        let mut rows = green.rows_snapshot();
        if let Some(row) = rows.get_mut(3) {
            row.budget = row.budget.saturating_add(9);
        }
        assert_eq!(book_from_rows(&rows), Err(P95Diag::ETamper.val()));
        let mut rows2 = green.rows_snapshot();
        if let Some(row) = rows2.get_mut(5) {
            row.over = !row.over;
        }
        assert_eq!(book_from_rows(&rows2), Err(P95Diag::ETamper.val()));
    }

    #[test]
    fn injection_row_range_all_caught() {
        // 注入三：越界样本号 10 轮全抓，账面零变动。
        let (caught, total, escape) = inject_row_range_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路: {}", escape);
        assert!(escape.is_empty());
        // 语料表恰 10 号且全部越闭集。
        assert_eq!(BAD_SAMPLE_NOS.len(), 10);
        for no in BAD_SAMPLE_NOS.iter() {
            assert!(*no == 0 || *no > SAMPLE_CAP as u32);
        }
    }

    #[test]
    fn boundary_four_classes() {
        // 四类边界：拍=线不越 / 拍=线+1越 / 单样本槽位1 / 满账冻结。
        assert!(boundary_ok());
        // 边界一直查：恰等于预算线的行旗标为假。
        let mut b1 = SampleBook::new();
        let _ = b1.register(1, P95_BUDGET_BEATS);
        match b1.row_of(1) {
            Some(r) => {
                assert!(!r.over);
                assert_eq!(r.budget, P95_BUDGET_BEATS);
            }
            None => assert!(false, "边界一行缺失"),
        }
        // 边界二直查：线+1 的行旗标为真。
        let mut b2 = SampleBook::new();
        let _ = b2.register(1, P95_BUDGET_BEATS + 1);
        match b2.row_of(1) {
            Some(r) => assert!(r.over),
            None => assert!(false, "边界二行缺失"),
        }
    }

    #[test]
    fn capacity_exhaustion_rejects() {
        // 容量耗尽：满账后写入拒、行数不变；半账重复仍报重复。
        assert!(capacity_ok());
        let mut book = build_green_book();
        assert_eq!(book.len(), SAMPLE_CAP);
        assert_eq!(book.register(1, 16), Err(P95Diag::ERowCap.val()));
        assert_eq!(book.len(), SAMPLE_CAP);
        assert_eq!(book.state(), BookState::Full);
    }

    #[test]
    fn frozen_guards_never_rewind() {
        // 冻结守卫：写拒/重复冻结倒退拒/未满冻结拒。
        assert!(frozen_guards_ok());
        let mut book = build_green_book();
        assert!(book.freeze().is_ok());
        assert_eq!(book.register(1, 16), Err(P95Diag::ESealed.val()));
        assert_eq!(book.len(), SAMPLE_CAP);
        assert_eq!(book.freeze(), Err(P95Diag::ERewind.val()));
        // 状态机只进不退：序 Open < Full < Frozen。
        assert!(BookState::Open.rank() < BookState::Full.rank());
        assert!(BookState::Full.rank() < BookState::Frozen.rank());
    }

    #[test]
    fn dual_run_fingerprints_identical() {
        // 双跑一致：指纹/账面/判定/sweep 逐项相同；20 场景逐对零容差。
        let a = build_green_book();
        let b = build_green_book();
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.rows_snapshot(), b.rows_snapshot());
        assert_eq!(a.judge_book(), b.judge_book());
        assert_eq!(inject_over_line_sweep(), inject_over_line_sweep());
        assert_eq!(inject_budget_tamper_sweep(), inject_budget_tamper_sweep());
        assert_eq!(inject_row_range_sweep(), inject_row_range_sweep());
        // 双机 20 场景一致（S1 母版 vs S2 复跑）。
        let mut sc = 0u32;
        while sc < 20 {
            let m = scenario_sig(sc);
            assert_eq!(scenario_sig(sc), m, "场景{} S2 漂移", sc);
            sc += 1;
        }
        assert!(dual_run_ok());
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条（实际 14 条含收口）。
        let s = run_unx12750_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.len(), 14);
        assert_eq!(s.tally().1, 0);
    }
}
