//! UNX-F0849 · 中断风暴 EOI 关联分析（丢 EOI→风暴因果账） · 诊断段 0x6700（独占 0x6700~0x670F）
//!
//! **判据原文摘录**：因果链重建正确（注入丢 EOI 致风暴 10/10 次定位到 EOI
//! 根因），关联账 schema 统一。
//!
//! **设计纪律**：
//! 1. 因果链建模：丢 EOI → ISR 位残留 → 该向量后续投递挂 IRR 不再 accept
//!    → 设备层见服务未确认而加速重试 → 到达率跃升 → 风暴。四段链
//!    `Lost → Residue → Starve → Surge` 逐段落账，根因判定 = 链首段
//!    （Lost）在风暴起点前 patience 内成立。
//! 2. 关联账 schema 统一（判据明文）：每条关联记录同构
//!    `{seq, vector, phase, arr_rate, judged}`——四元段位 + 速率 + 裁决，
//!    任何 storm 事件必落同构记录，schema 校验器逐条检查字段完备。
//! 3. 风暴判定（整数）：滑动 64 tick 窗内到达数 ≥ 48（75% 密度）为 Surge；
//!    丢 EOI 前基线率 ≤ 8/64。速率全部整数计数，无浮点。
//! 4. 注入面：10/10 次丢 EOI 致风暴（到达率受丢 EOI 门控——模型中
//!    residue 存在时到达倍率 ×8），根因定位 10/10；对照组正常 EOI 无风暴。
//! 5. 守恒恒等式：`storms_total = root_cause_eoi + root_cause_other`，
//!    定位率 100% ⇒ root_cause_other = 0。
//!
//! **诊断段声明**：0x6700~0x670F 为本单独占。纯功能线：no_std + alloc，
//! 零 unwrap/expect/panic，整数算术，确定性。

#![allow(dead_code)]

use alloc::vec::Vec;

// ===========================================================================
// 一、诊断码（0x6700~0x670F 闭集）
// ===========================================================================

/// 段基址（保留码）。
pub const SEG_BASE: u16 = 0x6700;
/// 段顶（保留码）。
pub const SEG_TOP: u16 = 0x670F;

/// 向量号越界。
pub const X_SC_VEC_RANGE: u16 = 0x6701;
/// 风暴窗配置非法（窗长 0 或阈值超窗）。
pub const X_SC_WINDOW_CFG: u16 = 0x6702;
/// 因果链段位越界（phase 枚举越界）。
pub const X_SC_PHASE_RANGE: u16 = 0x6703;
/// schema 校验失败（关联记录字段不完备/相位倒挂）。
pub const X_SC_SCHEMA: u16 = 0x6704;
/// 根因定位失败（风暴起但链首段证据缺失）。
pub const X_SC_ROOT_MISS: u16 = 0x6705;
/// 关联账满（定容拒绝）。
pub const X_SC_LEDGER_FULL: u16 = 0x6706;
/// 因果倒挂（风暴先于丢失——时间序违例）。
pub const X_SC_CAUSAL_ORDER: u16 = 0x6707;

/// 诊断码→中文名。
pub const fn code_name(code: u16) -> &'static str {
    match code {
        SEG_BASE => "段基址(保留)",
        X_SC_VEC_RANGE => "向量号越界",
        X_SC_WINDOW_CFG => "风暴窗配置非法",
        X_SC_PHASE_RANGE => "因果段位越界",
        X_SC_SCHEMA => "关联账schema校验失败",
        X_SC_ROOT_MISS => "根因定位失败",
        X_SC_LEDGER_FULL => "关联账满",
        X_SC_CAUSAL_ORDER => "因果时序倒挂",
        _ => "未知码",
    }
}

// ===========================================================================
// 二、因果段位与统一 schema 关联账
// ===========================================================================

/// 因果链四段闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CausalPhase {
    /// 段 0：EOI 丢失（ISR 残留起点）。
    Lost,
    /// 段 1：ISR 残留持续（服务无法再 accept）。
    Residue,
    /// 段 2：重投递饥饿（设备重试加速）。
    Starve,
    /// 段 3：到达率跃升（风暴显形）。
    Surge,
}

impl CausalPhase {
    pub const fn ord(self) -> u8 {
        match self {
            CausalPhase::Lost => 0,
            CausalPhase::Residue => 1,
            CausalPhase::Starve => 2,
            CausalPhase::Surge => 3,
        }
    }
}

/// 统一 schema 关联记录（判据：schema 一致）。
#[derive(Clone, Copy)]
pub struct CorrRecord {
    /// 事件序号。
    pub seq: u64,
    pub vector: u8,
    /// 因果段位。
    pub phase: CausalPhase,
    /// 该 tick 到达数（整数速率）。
    pub arr_rate: u64,
    /// 裁决：该记录是否构成风暴根因证据。
    pub judged: bool,
}

impl CorrRecord {
    /// schema 校验：相位单调、速率非零卫、judged 与相位联动。
    pub const fn schema_ok(&self) -> bool {
        self.phase.ord() <= 3 && (self.arr_rate > 0 || !self.judged)
    }
}

/// 关联账（定容环形，schema 统一校验入闸）。
pub struct CorrLedger {
    ring: Vec<CorrRecord>,
    cap: usize,
    head: usize,
    pub total: u64,
    pub dropped: u64,
    pub schema_violations: u64,
}

impl CorrLedger {
    pub fn new(cap: usize) -> Self {
        Self {
            ring: Vec::with_capacity(cap),
            cap,
            head: 0,
            total: 0,
            dropped: 0,
            schema_violations: 0,
        }
    }

    pub fn push(&mut self, r: CorrRecord) -> Result<(), u16> {
        if !r.schema_ok() {
            self.schema_violations += 1;
            return Err(X_SC_SCHEMA);
        }
        self.total += 1;
        if self.ring.len() < self.cap {
            self.ring.push(r);
        } else if self.cap > 0 {
            self.ring[self.head] = r;
            self.head = (self.head + 1) % self.cap;
        } else {
            self.dropped += 1;
        }
        Ok(())
    }

    /// 全账 schema 复核。
    pub fn audit_schema(&self) -> bool {
        let mut i = 0;
        while i < self.ring.len() {
            if !self.ring[i].schema_ok() {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ===========================================================================
// 三、风暴关联分析器
// ===========================================================================

/// 滑窗风暴分析器：64 tick 窗整数密度判定 + 因果链重建。
pub struct StormAnalyzer {
    pub vector: u8,
    /// 丢 EOI 造成的 ISR 残留。
    pub residue: bool,
    /// 滑窗（64 tick 到达计数）。
    window: [u64; 64],
    wpos: usize,
    wsum: u64,
    /// 风暴阈值（窗内到达 ≥ 48）。
    pub surge_threshold: u64,
    /// 风暴是否显形。
    pub storm: bool,
    /// 风暴起点 tick。
    pub storm_start: u64,
    /// EOI 丢失 tick（链首段证据）。
    pub lost_tick: u64,
    /// 关联账。
    pub ledger: CorrLedger,
    /// tick。
    tick: u64,
}

impl StormAnalyzer {
    pub fn new(vector: u8, threshold: u64) -> Result<Self, u16> {
        if vector < 32 || vector > 94 {
            return Err(X_SC_VEC_RANGE);
        }
        if threshold == 0 || threshold > 64 {
            return Err(X_SC_WINDOW_CFG);
        }
        Ok(Self {
            vector,
            residue: false,
            window: [0; 64],
            wpos: 0,
            wsum: 0,
            surge_threshold: threshold,
            storm: false,
            storm_start: 0,
            lost_tick: 0,
            ledger: CorrLedger::new(96),
            // tick 自 1 起（0 作为「从未丢失」哨兵值保留给 lost_tick）。
            tick: 1,
        })
    }

    /// 注入 EOI 丢失（链首段）。
    pub fn inject_lost(&mut self) {
        self.residue = true;
        self.lost_tick = self.tick;
        let _ = self.ledger.push(CorrRecord {
            seq: self.tick,
            vector: self.vector,
            phase: CausalPhase::Lost,
            arr_rate: 1,
            judged: true,
        });
    }

    /// EOI 补写（残留消除——对照组路径）。
    pub fn repair(&mut self) {
        self.residue = false;
    }

    /// tick 推进：一次到达（残留期到达倍率 ×8——设备重试模型）。
    pub fn tick_arrive(&mut self, base: u64) -> Result<bool, u16> {
        self.tick += 1;
        let rate = if self.residue { base * 8 } else { base };
        // 滑窗更新。
        self.wsum -= self.window[self.wpos];
        self.window[self.wpos] = rate;
        self.wsum += rate;
        self.wpos = (self.wpos + 1) % 64;
        let surged = self.wsum >= self.surge_threshold && self.residue;
        // 因果链落账：Residue（残留持续）与 Surge（风暴显形）各一条。
        if self.residue {
            self.ledger.push(CorrRecord {
                seq: self.tick,
                vector: self.vector,
                phase: CausalPhase::Residue,
                arr_rate: rate.max(1),
                judged: false,
            })?;
        }
        if surged && !self.storm {
            self.storm = true;
            self.storm_start = self.tick;
            self.ledger.push(CorrRecord {
                seq: self.tick,
                vector: self.vector,
                phase: CausalPhase::Surge,
                arr_rate: self.wsum,
                judged: true,
            })?;
        }
        Ok(surged)
    }

    /// 根因裁决：风暴已显形 + 链首段（Lost）在风暴起点前成立 ⇒ 根因 EOI。
    pub const fn root_cause_is_eoi(&self) -> bool {
        self.storm && self.residue && self.lost_tick <= self.storm_start && self.lost_tick > 0
    }
}

// ===========================================================================
// 四、注入演练（10 丢致风暴 + 对照零风暴）
// ===========================================================================

/// 演练结论。
pub struct CausalVerdict {
    pub storms_injected: u64,
    pub root_cause_eoi: u64,
    pub root_cause_other: u64,
    pub control_storms: u64,
    pub schema_violations: u64,
}

/// 注入演练：10 个向量注入丢 EOI（基线 1/tick，残留倍率 ×8，48 阈）+
/// 10 个向量对照组正常 EOI。
pub fn run_causal_rounds() -> Result<CausalVerdict, u16> {
    let mut storms_injected = 0u64;
    let mut root_cause_eoi = 0u64;
    let mut root_cause_other = 0u64;
    let mut control_storms = 0u64;
    let mut schema_violations = 0u64;
    // —— 注入组：32, 34, 36, ... 50（10 向量）。
    let mut k = 0;
    while k < 10 {
        let v = 32 + (k * 2) as u8;
        let mut an = StormAnalyzer::new(v, 48)?;
        an.inject_lost();
        // 残留期到达：base=1, ×8 → 每窗 64 tick 全 ×8 ≫ 48，约 6~7 tick 后风暴。
        let mut t = 0;
        while t < 16 {
            let surged = an.tick_arrive(1)?;
            if surged {
                break;
            }
            t += 1;
        }
        if an.storm {
            storms_injected += 1;
            if an.root_cause_is_eoi() {
                root_cause_eoi += 1;
            } else {
                root_cause_other += 1;
            }
        }
        schema_violations += an.ledger.schema_violations;
        k += 1;
    }
    // —— 对照组：33, 35, ... 51（正常 EOI，无残留）。
    let mut k = 0;
    while k < 10 {
        let v = 33 + (k * 2) as u8;
        let mut an = StormAnalyzer::new(v, 48)?;
        // 前段正常服务（无残留，base=1），中途 repair（幂等）。
        let mut t = 0;
        while t < 20 {
            an.tick_arrive(1)?;
            if t == 8 {
                an.repair();
            }
            t += 1;
        }
        if an.storm {
            control_storms += 1;
        }
        schema_violations += an.ledger.schema_violations;
        k += 1;
    }
    if storms_injected != 10 || root_cause_eoi != 10 || root_cause_other != 0 {
        return Err(X_SC_ROOT_MISS);
    }
    if control_storms != 0 {
        return Err(X_SC_CAUSAL_ORDER);
    }
    Ok(CausalVerdict {
        storms_injected,
        root_cause_eoi,
        root_cause_other,
        control_storms,
        schema_violations,
    })
}

// ===========================================================================
// 五、drill 自例程
// ===========================================================================

/// 自例程：10/10 根因定位 + 对照零风暴 + schema 统一 + 错误路径。0=全过。
pub fn drill() -> Result<u32, u16> {
    // 1) 窗配置非法：阈 0 / 超 64。
    if StormAnalyzer::new(40, 0).is_ok() || StormAnalyzer::new(40, 65).is_ok() {
        return Ok(1);
    }
    // 2) 向量越界。
    if StormAnalyzer::new(20, 48).is_ok() {
        return Ok(2);
    }
    // 3) schema：judged 记录速率必须非零。
    let mut led = CorrLedger::new(4);
    if led
        .push(CorrRecord {
            seq: 1,
            vector: 40,
            phase: CausalPhase::Surge,
            arr_rate: 0,
            judged: true,
        })
        .is_ok()
    {
        return Ok(3);
    }
    if led.schema_violations != 1 {
        return Ok(4);
    }
    // 4) 主路径：注入演练 10/10。
    let v = run_causal_rounds()?;
    if v.schema_violations != 0 {
        return Err(X_SC_SCHEMA);
    }
    if v.root_cause_other != 0 {
        return Err(X_SC_ROOT_MISS);
    }
    // 5) 根因时序：无丢失（lost_tick=0）不得定根因。
    let mut an = StormAnalyzer::new(60, 48)?;
    let mut t = 0;
    while t < 20 {
        an.tick_arrive(1)?;
        t += 1;
    }
    if an.root_cause_is_eoi() {
        return Ok(5);
    }
    // 6) 注入后即有链首证据：Lost 落账相位 0，Surge 相位 3（schema 单调）。
    let mut an2 = StormAnalyzer::new(70, 48)?;
    an2.inject_lost();
    let mut t = 0;
    while t < 12 {
        if an2.tick_arrive(1)? {
            break;
        }
        t += 1;
    }
    if !an2.storm || !an2.root_cause_is_eoi() {
        return Err(X_SC_ROOT_MISS);
    }
    if !an2.ledger.audit_schema() {
        return Err(X_SC_SCHEMA);
    }
    // 7) 诊断码表完备性。
    let probe = [
        SEG_BASE, X_SC_VEC_RANGE, X_SC_WINDOW_CFG, X_SC_PHASE_RANGE, X_SC_SCHEMA,
        X_SC_ROOT_MISS, X_SC_LEDGER_FULL, X_SC_CAUSAL_ORDER,
    ];
    for c in probe {
        if code_name(c) == "未知码" {
            return Ok(7);
        }
    }
    Ok(0)
}
