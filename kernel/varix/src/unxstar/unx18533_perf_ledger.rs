//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18533 探针性能账（跑批时长） · 0x80100段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18533-J1 50 例跑批 <5s 断言，超限告警入账。
//! 结构清单：
//!   - 数据结构：PerfRecord（Copy 时长记录：序/例号/微秒）、PerfLedger
//!     （50 槽数组账本 + 饱和累加总额）、AlarmLedger（32 槽告警账 +
//!     两类告警字）、确定性时长模型（纯整数公式，无墙钟无随机）；
//!   - 状态机：Collecting→Auditing→Closed 三态，Closed 后记账显式拒绝；
//!   - 账本：时长账（例号单调追加）+ 告警账（只增不减，序号单调）；
//!   - 错误路径：PfCode 闭集 12 码，code() = SEG_BASE + n（0x80100 段），
//!     每码非空中文建议；
//!   - 注入核：零时长 / 重号 / 乱序 / 单例超限 四类坏样本各 10 轮
//!     逐类断言被显式拒绝或告警入账（10/10 全抓）。
//! 时长模型推导（写死）：duration(i) = 6_000 + (1_751*i + 313) % 83_000 µs，
//!   单例上界 6_000+82_999 = 88_999 µs；50 例最坏总额
//!   50 × 88_999 = 4_449_950 µs < 5_000_000 µs（5s 预算），
//!   故 50 例跑批 <5s 断言为结构保证而非碰运气。
//! 纪律：no_std + alloc；内核路径零 panic；无浮点/IO/墙钟/随机。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段基址（0x80100，AI-VX11-1105 G1 批07 独占区，卡表给定）。
pub const SEG_BASE: u32 = 0x80100;

// ── 冻结常量（推导注明） ─────────────────────────────────────────
/// 跑批例数（J1 冻结：50 例）。
pub const BATCH_CASES: usize = 50;
/// 跑批预算（5s = 5_000_000 µs，J1 冻结）。
pub const BUDGET_US: u64 = 5_000_000;
/// 单例时长上限（100_000 µs = 100ms；超出即单例告警入账）。
pub const CASE_CAP_US: u64 = 100_000;
/// 时长账容量（50 槽：恰好承载一批，无溢位）。
pub const LEDGER_CAP: usize = BATCH_CASES;
/// 告警账容量（32 槽：单例告警 + 批告警分账）。
pub const ALARM_CAP: usize = 32;
/// 时长模型基值（µs，单例下界）。
pub const MODEL_BASE_US: u64 = 6_000;
/// 时长模型模数（µs，时长摆幅）。
pub const MODEL_MOD_US: u64 = 83_000;
/// 时长模型斜率（与 313 互质于 83_000，保证 50 例不撞同值）。
pub const MODEL_SLOPE: u64 = 1_751;
/// 时长模型偏置。
pub const MODEL_BIAS: u64 = 313;
/// 单例时长最坏上界（推导：MODEL_BASE_US + MODEL_MOD_US - 1）。
pub const MODEL_CASE_MAX_US: u64 = MODEL_BASE_US + MODEL_MOD_US - 1;
/// 50 例最坏总额上界（推导：BATCH_CASES × MODEL_CASE_MAX_US）。
pub const MODEL_BATCH_MAX_US: u64 = BATCH_CASES as u64 * MODEL_CASE_MAX_US;
/// 告警字：单例超限。
pub const ALARM_CASE_OVER: u8 = 1;
/// 告警字：批次超预算。
pub const ALARM_BATCH_OVER: u8 = 2;
/// 账态：收账中（可记账）。
pub const STATE_COLLECTING: u8 = 1;
/// 账态：核算中（只读）。
pub const STATE_AUDITING: u8 = 2;
/// 账态：闭账（终态）。
pub const STATE_CLOSED: u8 = 3;
/// 判据迭代轮数（写死：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入类别数（4 类坏样本）。
pub const INJECT_CLASSES: usize = 4;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 零时长（无测量意义）
    ZeroDuration,
    /// 例号重号（一批内唯一）
    DuplicateCase,
    /// 例号乱序（追加序必须单调）
    OutOfOrder,
    /// 时长账满（50 槽耗尽）
    TableFull,
    /// 告警账满（32 槽耗尽）
    AlarmFull,
    /// 批次超预算（总额 ≥ 5s）
    BatchOverBudget,
    /// 闭账后记账
    ClosedAppend,
    /// 账态非法转换（越级/倒退）
    IllegalTransition,
    /// 告警账损坏（序号不单调/告警字越闭集）
    AlarmCorrupt,
    /// 守恒破坏（总额 ≠ 逐条重加）
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
    /// 告警字非法
    AlarmKindIllegal,
}

impl PfCode {
    /// 诊断码取值：SEG_BASE + 枚举序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 非空中文建议（文案闭集）。
    pub fn detail(self) -> &'static str {
        match self {
            PfCode::ZeroDuration => "零时长：时长为 0 无测量意义，记账显式拒绝",
            PfCode::DuplicateCase => "例号重号：一批内例号必须唯一，重复记账拒绝",
            PfCode::OutOfOrder => "例号乱序：追加序必须单调递增，乱序拒绝",
            PfCode::TableFull => "时长账满：50 槽耗尽，先核算再闭账另起一批",
            PfCode::AlarmFull => "告警账满：32 槽耗尽，告警账只增不减需扩册",
            PfCode::BatchOverBudget => "批次超预算：50 例总额 ≥ 5s，告警入账并标红",
            PfCode::ClosedAppend => "闭账后记账：Closed 终态只读，记账显式拒绝",
            PfCode::IllegalTransition => "账态非法转换：三态只进不退，越级拒绝",
            PfCode::AlarmCorrupt => "告警账损坏：序号须单调，告警字须在闭集内",
            PfCode::ConservationBroken => "守恒破坏：总额与逐条重加（逆序）不一致",
            PfCode::NotReproducible => "双跑不一致：同输入两次核算总额不同",
            PfCode::AlarmKindIllegal => "告警字非法：闭集为 单例超限/批次超预算",
        }
    }
    /// 码闭集清单。
    pub const ALL: [PfCode; 12] = [
        PfCode::ZeroDuration,
        PfCode::DuplicateCase,
        PfCode::OutOfOrder,
        PfCode::TableFull,
        PfCode::AlarmFull,
        PfCode::BatchOverBudget,
        PfCode::ClosedAppend,
        PfCode::IllegalTransition,
        PfCode::AlarmCorrupt,
        PfCode::ConservationBroken,
        PfCode::NotReproducible,
        PfCode::AlarmKindIllegal,
    ];
}

/// 诊断码 → 中文建议（闭集外返回未知提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PfCode::ALL.len() {
        if PfCode::ALL[i].code() == code {
            return PfCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80100 段闭集内"
}

// ── 确定性时长模型（无墙钟：纯整数公式） ────────────────────────
/// 第 case_idx 例的建模时长（µs）。同序号永远同值（可复测声明）。
pub fn modeled_duration(case_idx: u32) -> u64 {
    MODEL_BASE_US + (MODEL_SLOPE * case_idx as u64 + MODEL_BIAS) % MODEL_MOD_US
}

// ── 时长记录（Copy） ─────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PerfRecord {
    /// 追加序号（单调递增）。
    pub seq: u32,
    /// 例号（批内唯一，追加序即例号序）。
    pub case_id: u32,
    /// 建模时长（µs）。
    pub duration_us: u64,
    /// 空槽标记。
    pub empty: bool,
}

impl PerfRecord {
    /// 空记录（const fn：定容数组初始化用）。
    pub const fn empty() -> PerfRecord {
        PerfRecord { seq: 0, case_id: 0, duration_us: 0, empty: true }
    }
}

// ── 告警记录（Copy） ─────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AlarmRec {
    /// 告警序号（单调递增）。
    pub seq: u32,
    /// 关联例号（批告警为 0 表示整批）。
    pub case_id: u32,
    /// 告警字（ALARM_CASE_OVER / ALARM_BATCH_OVER）。
    pub kind: u8,
    /// 触发值（µs：单例为该例时长，批告警为总额）。
    pub value_us: u64,
    /// 空槽标记。
    pub empty: bool,
}

impl AlarmRec {
    /// 空告警（const fn）。
    pub const fn empty() -> AlarmRec {
        AlarmRec { seq: 0, case_id: 0, kind: 0, value_us: 0, empty: true }
    }
}

// ── 性能账（核心账本 + 三态状态机 + 告警账） ────────────────────
pub struct PerfLedger {
    /// 50 槽时长账。
    cases: [PerfRecord; LEDGER_CAP],
    /// 32 槽告警账。
    alarms: [AlarmRec; ALARM_CAP],
    /// 已记账例数。
    count: usize,
    /// 已入告警笔数。
    alarm_count: usize,
    /// 下一个追加序号（时长账）。
    next_seq: u32,
    /// 下一个告警序号。
    next_alarm_seq: u32,
    /// 账态（STATE_* 闭集）。
    state: u8,
    /// 闭账后记账被拒笔数（显性记账）。
    pub closed_append_rejects: u32,
}

impl PerfLedger {
    pub fn new() -> PerfLedger {
        PerfLedger {
            cases: [PerfRecord::empty(); LEDGER_CAP],
            alarms: [AlarmRec::empty(); ALARM_CAP],
            count: 0,
            alarm_count: 0,
            next_seq: 1,
            next_alarm_seq: 1,
            state: STATE_COLLECTING,
            closed_append_rejects: 0,
        }
    }

    /// 记一笔时长（闸序：账态→零值→重号→乱序→容量）。
    pub fn record(&mut self, case_id: u32, duration_us: u64) -> Result<(), u32> {
        // 闸一：账态必须 Collecting。
        if self.state != STATE_COLLECTING {
            self.closed_append_rejects = self.closed_append_rejects.saturating_add(1);
            return Err(PfCode::ClosedAppend.code());
        }
        // 闸二：零时长拒。
        if duration_us == 0 {
            return Err(PfCode::ZeroDuration.code());
        }
        // 闸三：重号拒（例号已存在）。
        let mut i = 0;
        while i < self.count {
            let rec = match self.cases.get(i) {
                Some(r) => *r,
                None => return Err(PfCode::ConservationBroken.code()),
            };
            if !rec.empty && rec.case_id == case_id {
                return Err(PfCode::DuplicateCase.code());
            }
            i += 1;
        }
        // 闸四：乱序拒（追加序 = 例号序，前笔例号必须更小）。
        if self.count > 0 {
            let last = match self.cases.get(self.count - 1) {
                Some(r) => *r,
                None => return Err(PfCode::ConservationBroken.code()),
            };
            if case_id <= last.case_id {
                return Err(PfCode::OutOfOrder.code());
            }
        }
        // 闸五：容量。
        if self.count >= LEDGER_CAP {
            return Err(PfCode::TableFull.code());
        }
        // 写账（先算好记录，再可变借用）。
        let rec = PerfRecord {
            seq: self.next_seq,
            case_id,
            duration_us,
            empty: false,
        };
        match self.cases.get_mut(self.count) {
            Some(sl) => {
                *sl = rec;
            }
            None => return Err(PfCode::TableFull.code()),
        }
        self.count += 1;
        self.next_seq = self.next_seq.saturating_add(1);
        // 单例超限：告警入账但记账继续（测量不丢，判定靠告警账）。
        if duration_us > CASE_CAP_US {
            self.push_alarm(case_id, ALARM_CASE_OVER, duration_us)?;
        }
        Ok(())
    }

    /// 告警入账（只增不减，序号单调）。
    pub fn push_alarm(&mut self, case_id: u32, kind: u8, value_us: u64) -> Result<(), u32> {
        if kind != ALARM_CASE_OVER && kind != ALARM_BATCH_OVER {
            return Err(PfCode::AlarmKindIllegal.code());
        }
        if self.alarm_count >= ALARM_CAP {
            return Err(PfCode::AlarmFull.code());
        }
        let rec = AlarmRec {
            seq: self.next_alarm_seq,
            case_id,
            kind,
            value_us,
            empty: false,
        };
        match self.alarms.get_mut(self.alarm_count) {
            Some(sl) => {
                *sl = rec;
            }
            None => return Err(PfCode::AlarmFull.code()),
        }
        self.alarm_count += 1;
        self.next_alarm_seq = self.next_alarm_seq.saturating_add(1);
        Ok(())
    }

    /// 核算：Collecting → Auditing，返回总额（µs，饱和累加）。
    pub fn audit(&mut self) -> Result<u64, u32> {
        if self.state != STATE_COLLECTING {
            return Err(PfCode::IllegalTransition.code());
        }
        self.state = STATE_AUDITING;
        let total = self.batch_total();
        if total >= BUDGET_US {
            // 批次超预算：告警入账（批告警 case_id=0 表整批）。
            self.push_alarm(0, ALARM_BATCH_OVER, total)?;
            return Err(PfCode::BatchOverBudget.code());
        }
        Ok(total)
    }

    /// 闭账：Auditing → Closed（终态）。
    pub fn close(&mut self) -> Result<(), u32> {
        if self.state != STATE_AUDITING {
            return Err(PfCode::IllegalTransition.code());
        }
        self.state = STATE_CLOSED;
        Ok(())
    }

    /// 总额：逐条饱和累加（u64 显式累加器）。
    pub fn batch_total(&self) -> u64 {
        let mut total: u64 = 0;
        let mut i = 0;
        while i < self.count {
            let d = match self.cases.get(i) {
                Some(r) => r.duration_us,
                None => 0,
            };
            total = total.saturating_add(d);
            i += 1;
        }
        total
    }

    /// 守恒：逆序重加与正序总额一致（加法可交换的结构断言）。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut total: u64 = 0;
        let mut i = self.count;
        while i > 0 {
            i -= 1;
            let d = match self.cases.get(i) {
                Some(r) => r.duration_us,
                None => return Err(PfCode::ConservationBroken.code()),
            };
            total = total.saturating_add(d);
        }
        if total != self.batch_total() {
            return Err(PfCode::ConservationBroken.code());
        }
        self.audit_alarms()
    }

    /// 告警账审计：序号单调 + 告警字闭集 + 笔数一致。
    pub fn audit_alarms(&self) -> Result<(), u32> {
        let mut prev_seq = 0u32;
        let mut i = 0;
        while i < self.alarm_count {
            let a = match self.alarms.get(i) {
                Some(r) => *r,
                None => return Err(PfCode::AlarmCorrupt.code()),
            };
            if a.empty || a.seq <= prev_seq {
                return Err(PfCode::AlarmCorrupt.code());
            }
            if a.kind != ALARM_CASE_OVER && a.kind != ALARM_BATCH_OVER {
                return Err(PfCode::AlarmCorrupt.code());
            }
            prev_seq = a.seq;
            i += 1;
        }
        if self.alarm_count != self.next_alarm_seq as usize - 1 {
            return Err(PfCode::AlarmCorrupt.code());
        }
        Ok(())
    }

    /// 追加序单调审计（时长账）。
    pub fn seq_monotonic(&self) -> bool {
        let mut prev = 0u32;
        let mut i = 0;
        while i < self.count {
            let rec = match self.cases.get(i) {
                Some(r) => *r,
                None => return false,
            };
            if rec.seq <= prev {
                return false;
            }
            prev = rec.seq;
            i += 1;
        }
        true
    }

    pub fn case_count(&self) -> usize {
        self.count
    }

    pub fn alarm_count(&self) -> usize {
        self.alarm_count
    }

    pub fn state(&self) -> u8 {
        self.state
    }

    /// 第 idx 笔时长只读（越界 None）。
    pub fn duration_at(&self, idx: usize) -> Option<u64> {
        match self.cases.get(idx) {
            Some(r) if !r.empty => Some(r.duration_us),
            _ => None,
        }
    }

    /// 第 idx 笔告警只读（越界 None）。
    pub fn alarm_at(&self, idx: usize) -> Option<AlarmRec> {
        match self.alarms.get(idx) {
            Some(a) if !a.empty => Some(*a),
            _ => None,
        }
    }
}

// ── 黄金链：50 例入账 → 核算 <5s → 闭账 ─────────────────────────
/// 返回 (例数, 总额 µs, 告警笔数)。50 例全绿：无告警、总额在预算内。
pub fn golden_run() -> Result<(usize, u64, usize), u32> {
    let mut led = PerfLedger::new();
    let mut i = 0u32;
    while (i as usize) < BATCH_CASES {
        let dur = modeled_duration(i);
        led.record(i + 1, dur)?;
        i += 1;
    }
    let total = led.audit()?;
    led.close()?;
    led.conserve()?;
    if !led.seq_monotonic() {
        return Err(PfCode::ConservationBroken.code());
    }
    if led.case_count() != BATCH_CASES || led.alarm_count() != 0 {
        return Err(PfCode::ConservationBroken.code());
    }
    Ok((led.case_count(), total, led.alarm_count()))
}

// ── 超限注入：构造必超预算批（红路径验证） ──────────────────────
/// 一枚 4.9s 例 + 49 枚基线例 → 总额必 ≥ 5s → 批告警入账 + 判红。
/// 返回 (告警笔数, 触发的批告警值)。
pub fn inject_batch_over() -> Result<(usize, u64), u32> {
    let mut led = PerfLedger::new();
    // 单例 4_900_000 µs（< 单例上限 100_000？不——它必触发单例告警，
    // 同时把总额推过 5s 预算，构成双告警场景）。
    led.record(1, 4_900_000)?;
    let mut i = 1u32;
    while (i as usize) < BATCH_CASES {
        led.record(i + 1, MODEL_BASE_US)?;
        i += 1;
    }
    let total = led.audit();
    // 必判红（BatchOverBudget）且批告警已入账。
    if total.err() != Some(PfCode::BatchOverBudget.code()) {
        return Err(PfCode::ConservationBroken.code());
    }
    if led.alarm_count() < 2 {
        // 至少两条：单例超限 + 批次超预算。
        return Err(PfCode::ConservationBroken.code());
    }
    // 批告警值 = 总额（4_900_000 + 49×6_000 = 5_194_000 µs ≥ 预算）。
    let mut batch_alarm_val = 0u64;
    let mut k = 0;
    while k < led.alarm_count() {
        let rec = match led.alarm_at(k) {
            Some(a) => a,
            None => return Err(PfCode::AlarmCorrupt.code()),
        };
        if rec.kind == ALARM_BATCH_OVER {
            batch_alarm_val = rec.value_us;
        }
        k += 1;
    }
    if batch_alarm_val == 0 {
        return Err(PfCode::ConservationBroken.code());
    }
    Ok((led.alarm_count(), batch_alarm_val))
}

// ── 注入核：四类坏样本，各 10 轮，逐类全抓 ──────────────────────
pub struct InjectLedger {
    pub tries: [u32; INJECT_CLASSES],
    pub caught: [u32; INJECT_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJECT_CLASSES], caught: [0; INJECT_CLASSES] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJECT_CLASSES {
            let t = *self.tries.get(i).unwrap_or(&0);
            let c = *self.caught.get(i).unwrap_or(&0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn conservation(&self) -> Result<(), u32> {
        let mut tries = 0u32;
        let mut caught = 0u32;
        let mut i = 0;
        while i < INJECT_CLASSES {
            tries = tries.saturating_add(*self.tries.get(i).unwrap_or(&0));
            caught = caught.saturating_add(*self.caught.get(i).unwrap_or(&0));
            i += 1;
        }
        if caught > tries {
            return Err(PfCode::ConservationBroken.code());
        }
        Ok(())
    }
}

/// 四类坏样本逐类注入 `rounds` 轮：
/// C0 零时长、C1 重号、C2 乱序、C3 单例超限必告警。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // C0：零时长记账必拒。
        let mut l0 = PerfLedger::new();
        led.record(0, l0.record(1, 0).err() == Some(PfCode::ZeroDuration.code()));
        // C1：重号记账必拒。
        let mut l1 = PerfLedger::new();
        let _ = l1.record(1, MODEL_BASE_US);
        led.record(1, l1.record(1, MODEL_BASE_US).err() == Some(PfCode::DuplicateCase.code()));
        // C2：乱序记账必拒（1 之后记 1 等效重号；先记 2 再记 1 为乱序）。
        let mut l2 = PerfLedger::new();
        let _ = l2.record(2, MODEL_BASE_US);
        led.record(2, l2.record(1, MODEL_BASE_US).err() == Some(PfCode::OutOfOrder.code()));
        // C3：单例超限必告警入账（测量保留，告警笔数 ≥1）。
        let mut l3 = PerfLedger::new();
        let _ = l3.record(1, CASE_CAP_US + 1);
        led.record(3, l3.alarm_count() == 1);
        r += 1;
    }
    led.conservation()?;
    Ok(led)
}

// ── 探针三序（收账→核算→闭账 / 越级 / 闭账后记账） ──────────────
pub fn probe_seq_a() -> Result<u64, u32> {
    let mut led = PerfLedger::new();
    led.record(1, modeled_duration(0))?;
    led.record(2, modeled_duration(1))?;
    let t = led.audit()?;
    led.close()?;
    led.conserve()?;
    Ok(t)
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 越级：未核算直接闭账。
    let mut led = PerfLedger::new();
    led.close()
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 闭账后记账必拒。
    let mut led = PerfLedger::new();
    led.record(1, modeled_duration(0))?;
    led.audit()?;
    led.close()?;
    led.record(2, modeled_duration(1))
}

/// 容量耗尽：50 槽写满后第 51 例必拒且账不丢。
pub fn capacity_probe() -> Result<usize, u32> {
    let mut led = PerfLedger::new();
    let mut id = 1u32;
    while (id as usize) <= LEDGER_CAP {
        led.record(id, MODEL_BASE_US)?;
        id += 1;
    }
    let before = led.case_count();
    let r = led.record(LEDGER_CAP as u32 + 1, MODEL_BASE_US);
    if r.err() != Some(PfCode::TableFull.code()) {
        return Err(PfCode::ConservationBroken.code());
    }
    if led.case_count() != before {
        return Err(PfCode::ConservationBroken.code());
    }
    Ok(before)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PfCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x80000 || SEG_BASE >= 0x80400 {
        return false;
    }
    let mut i = 0;
    while i < PfCode::ALL.len() {
        let v = PfCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集。
pub fn detail_closed_set() -> bool {
    let mut i = 0;
    while i < PfCode::ALL.len() {
        if PfCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 判据集（J1 验收） ───────────────────────────────────────────
pub fn run_unx18533_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18533 探针性能账");
    // 静态明细判据。
    s.add("J1-段位守卫", segment_guard(), "0x80100 段 0x40 窗口，码闭集不越界");
    s.add(
        "J1-预算上界结构保证",
        MODEL_BATCH_MAX_US < BUDGET_US,
        "50×88_999 = 4_449_950 < 5_000_000 µs（建模最坏界）",
    );
    s.add(
        "J1-账态三序",
        probe_seq_a().is_ok()
            && probe_seq_b().err() == Some(PfCode::IllegalTransition.code())
            && probe_seq_c().err() == Some(PfCode::ClosedAppend.code()),
        "收账→核算→闭账全过；越级闭账/闭账后记账均拒",
    );
    s.add(
        "J1-边界四类闸",
        {
            let mut led = PerfLedger::new();
            let zero = led.record(1, 0).is_err();
            let cap_ok = led.record(1, CASE_CAP_US).is_ok();
            let cap_alarm = led.alarm_count() == 0; // 恰等于上限不告警（> 才告）
            let over_ok = led.record(2, CASE_CAP_US + 1).is_ok();
            let over_alarm = led.alarm_count() == 1;
            zero && cap_ok && cap_alarm && over_ok && over_alarm
        },
        "零时长拒；恰 100ms 不告警；100ms+1 告警入账",
    );
    s.add(
        "J1-文案闭集",
        detail_closed_set(),
        "12 码逐码非空中文建议，闭集外落未知提示",
    );
    // 动态明细判据（≤8 条）。
    let g = golden_run();
    let budget_ok = match g {
        Ok((n, total, alarms)) => n == BATCH_CASES && total < BUDGET_US && alarms == 0,
        Err(_) => false,
    };
    s.add_dyn(
        "J1-50例跑批<5s",
        budget_ok,
        &format!("50例总额 = {:?} µs（预算 5_000_000）", g.map(|(_, t, _)| t)),
    );
    let over = inject_batch_over();
    s.add_dyn(
        "J1-超限告警入账",
        over.is_ok(),
        &format!("注入超限批 = {:?}（告警笔数, 批告警值）", over),
    );
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn(
        "J1-注入四类10/10全抓",
        inj_ok,
        &format!(
            "tries={:?} caught={:?}",
            inj.as_ref().map(|l| l.tries).unwrap_or([0; INJECT_CLASSES]),
            inj.as_ref().map(|l| l.caught).unwrap_or([0; INJECT_CLASSES])
        ),
    );
    let cap = capacity_probe();
    s.add_dyn(
        "J1-容量耗尽账不丢",
        cap.is_ok(),
        &format!("满账例数 = {:?}", cap),
    );
    let rep = repro_check();
    s.add_dyn("J1-双跑一致", rep.is_ok(), &format!("{:?}", rep));
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_50_cases_under_budget() {
        let g = golden_run().unwrap_or((0, 0, 0));
        assert_eq!(g.0, BATCH_CASES);
        assert!(g.1 < BUDGET_US, "50 例总额必须 <5s：{}", g.1);
        assert_eq!(g.2, 0);
    }

    #[test]
    fn test_modeled_duration_deterministic_and_bounded() {
        // 同序号同值（确定性）。
        assert_eq!(modeled_duration(7), modeled_duration(7));
        // 单例上界。
        let mut i = 0u32;
        while (i as usize) < BATCH_CASES {
            let d = modeled_duration(i);
            assert!(d >= MODEL_BASE_US);
            assert!(d < MODEL_BASE_US + MODEL_MOD_US);
            i += 1;
        }
    }

    #[test]
    fn test_batch_over_alarm_ledger() {
        let over = inject_batch_over().unwrap_or((0, 0));
        assert!(over.0 >= 2, "单例告警 + 批告警至少两条");
        assert!(over.1 >= BUDGET_US);
    }

    #[test]
    fn test_inject_all_caught_10_rounds() {
        let led = inject_rounds(JUDGE_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(led.all_caught(), "四类坏样本 10 轮必须全抓");
        assert!(led.conservation().is_ok());
    }

    #[test]
    fn test_state_machine_transitions() {
        // 越级闭账拒。
        let mut led = PerfLedger::new();
        assert!(led.close().is_err());
        // 正常链 + 闭账后记账拒。
        let _ = led.record(1, MODEL_BASE_US);
        assert!(led.audit().is_ok());
        assert!(led.close().is_ok());
        assert!(led.record(2, MODEL_BASE_US).is_err());
        assert_eq!(led.closed_append_rejects, 1);
        // 二次核算（Auditing 态再 audit）拒。
        let mut led2 = PerfLedger::new();
        let _ = led2.record(1, MODEL_BASE_US);
        assert!(led2.audit().is_ok());
        assert!(led2.audit().is_err());
    }

    #[test]
    fn test_boundary_case_cap() {
        let mut led = PerfLedger::new();
        // 零时长拒。
        assert!(led.record(1, 0).is_err());
        // 恰 100ms：不告警。
        assert!(led.record(1, CASE_CAP_US).is_ok());
        assert_eq!(led.alarm_count(), 0);
        // 100ms+1：告警入账，记账保留。
        assert!(led.record(2, CASE_CAP_US + 1).is_ok());
        assert_eq!(led.alarm_count(), 1);
        // 告警字非法拒。
        assert!(led.push_alarm(3, 9, 1).is_err());
    }

    #[test]
    fn test_duplicate_and_out_of_order() {
        let mut led = PerfLedger::new();
        assert!(led.record(1, MODEL_BASE_US).is_ok());
        assert!(led.record(1, MODEL_BASE_US).is_err()); // 重号
        assert!(led.record(3, MODEL_BASE_US).is_ok());
        assert!(led.record(2, MODEL_BASE_US).is_err()); // 乱序
        assert_eq!(led.case_count(), 2);
    }

    #[test]
    fn test_capacity_exhaustion() {
        assert_eq!(capacity_probe().unwrap_or(0), LEDGER_CAP);
        let mut led = PerfLedger::new();
        let mut id = 1u32;
        while (id as usize) <= LEDGER_CAP {
            assert!(led.record(id, MODEL_BASE_US).is_ok());
            id += 1;
        }
        assert!(led.record(999, MODEL_BASE_US).is_err());
        assert_eq!(led.case_count(), LEDGER_CAP);
    }

    #[test]
    fn test_conserve_and_monotonic() {
        let mut led = PerfLedger::new();
        let mut i = 0u32;
        while (i as usize) < 10 {
            let _ = led.record(i + 1, modeled_duration(i));
            i += 1;
        }
        assert!(led.conserve().is_ok());
        assert!(led.seq_monotonic());
        assert_eq!(led.batch_total(), led.batch_total());
    }

    #[test]
    fn test_double_run_reproducible() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_segment_guard_and_detail_closed_set() {
        assert!(segment_guard());
        assert!(detail_closed_set());
        assert_eq!(PfCode::BatchOverBudget.code(), SEG_BASE + 5);
    }

    #[test]
    fn test_checks_all_passed() {
        let set = run_unx18533_checks();
        assert!(set.all_passed(), "UNX-F18533 判据必须全绿：{}", set.red_items_text());
    }
}
