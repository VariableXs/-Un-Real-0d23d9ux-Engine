//! UNX-F0698 · 矩阵门禁（全绿放行与域收官联动）· 诊断段 0x57c0（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0698] 矩阵门禁（全绿放行与域收官联动） <SEG 0x57c0>
//!   UNX-F0698-J1 门禁判定（四条件：成功率达标/零差异全同/失败分类完备/
//!   归档齐备）全过才放行，缺一拦截 100%。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 四条件闭集：Cond::SuccessRate（成功率达标）/ Cond::ZeroDiff（零差异
//!      全同）/ Cond::FailClassified（失败分类完备）/ Cond::ArchiveReady
//!      （归档齐备），条件外无判定对象。
//!   2. 全过才放行：四条件位全真 → GateDecision::Release；任一缺位 →
//!      Blocked 并逐条记录缺件（缺一拦截 100%，无部分放行态）。
//!   3. 判定二值零灰区：Release/Blocked 双臂闭集，无 Conditional 第三态；
//!      条件位取值只认 0/1（灰区位拒收）。
//!   4. 域收官联动：放行即写收官事件（域收官账 +1），拦截不写；收官事件
//!      数 == 放行数恒等（守恒锚）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x57c0~0x57dF 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥580（判据原文「纯功能代码 ≥580 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x57c0 独占 0x57c0~0x57dF） ────────────────────────────
pub const SEG_BASE: u16 = 0x57c0;

/// 0x57c1 门禁已定判仍尝试改条件位
const E_DECIDED: u16 = SEG_BASE + 0x1;
/// 0x57c2 条件号越界（合法 0..4）
const E_COND_RANGE: u16 = SEG_BASE + 0x2;
/// 0x57c3 条件位灰区：非 0/1 取值
const E_COND_GRAY: u16 = SEG_BASE + 0x3;
/// 0x57c4 缺件拦截：四条件未全真即请求放行
const E_GATE_BLOCK: u16 = SEG_BASE + 0x4;
/// 0x57c5 放行后收官事件账失衡：收官数 ≠ 放行数
const E_CLOSE_IMBAL: u16 = SEG_BASE + 0x5;
/// 0x57c6 未放行即收官：拦截门禁无收官资格
const E_NOT_RELEASED: u16 = SEG_BASE + 0x6;
/// 0x57c7 成功率越域：ppm > 10000
const E_PPM_RANGE: u16 = SEG_BASE + 0x7;
/// 0x57c8 未定判即解引判定结论
const E_DECISION_OPEN: u16 = SEG_BASE + 0x8;
/// 0x57c9 缺件账溢出：缺件记录超定容
const E_MISSING_FULL: u16 = SEG_BASE + 0x9;
/// 0x57ca 二次定判：判定已冻结仍请求判定
const E_REJUDGE: u16 = SEG_BASE + 0xA;
/// 0x57cb 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xB;
/// 0x57cc 缺件号悬空：缺件账引用的条件号越界
const E_MISSING_DANGLE: u16 = SEG_BASE + 0xC;
/// 0x57cd 门禁序号越界（合法 1..=MAX_GATES）
const E_GATE_SEQ: u16 = SEG_BASE + 0xD;

/// 诊断码 → 中文可操作诊断（段 0x57c0~0x57dF 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_DECIDED => "门禁已定判：条件位冻结，改判请开新门禁",
        E_COND_RANGE => "条件号越界：四条件闭集合法 0..4",
        E_COND_GRAY => "条件位灰区：取值只认 0/1，其余拒收",
        E_GATE_BLOCK => "缺件拦截：四条件全真才放行，请补齐缺件",
        E_CLOSE_IMBAL => "收官失衡：收官事件数 ≠ 放行数，联动账被污染",
        E_NOT_RELEASED => "未放行即收官：拦截门禁无收官资格",
        E_PPM_RANGE => "成功率越域：ppm 合法 0..=10000",
        E_DECISION_OPEN => "未定判即解引：先 judge 再取结论",
        E_MISSING_FULL => "缺件账溢出：缺件记录超定容上限",
        E_REJUDGE => "二次定判：判定已冻结，拒收重判",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_MISSING_DANGLE => "缺件号悬空：缺件账引用越界条件号",
        E_GATE_SEQ => "门禁序号越界：合法 1..=MAX_GATES",
        _ => "未知诊断码（0x57c0 段外或保留位）",
    }
}

// ── 常量（来源：判据四条件与矩阵口径） ───────────────────────────────
/// 门禁条件数（判据原文：四条件）。
pub const COND_COUNT: usize = 4;
/// 达标线：成功率 ≥99% = 9900 ppm。
pub const PASS_PPM: u64 = 9_900;
/// ppm 基数。
pub const PPM_SCALE: u64 = 10_000;
/// 门禁账定容（同批域收官门禁数上限）。
pub const MAX_GATES: usize = 32;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(COND_COUNT == 4);
const _: () = assert!(PASS_PPM == 9_900);

// ── FNV-1a 64 ────────────────────────────────────────────────────────
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_mix(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

// ── 四条件闭集 ───────────────────────────────────────────────────────
/// 门禁四条件闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateCond {
    /// 条件 0：成功率达标（成功率 ppm ≥ 9900）。
    SuccessRate,
    /// 条件 1：零差异全同（复跑三面 diff 零漂移）。
    ZeroDiff,
    /// 条件 2：失败分类完备（失败样本全部落类）。
    FailClassified,
    /// 条件 3：归档齐备（归档件 6/6 齐）。
    ArchiveReady,
}

impl GateCond {
    /// 条件号。
    pub const fn id(self) -> usize {
        match self {
            GateCond::SuccessRate => 0,
            GateCond::ZeroDiff => 1,
            GateCond::FailClassified => 2,
            GateCond::ArchiveReady => 3,
        }
    }

    /// 按条件号重建。
    pub fn from_id(v: usize) -> Result<GateCond, u16> {
        match v {
            0 => Ok(GateCond::SuccessRate),
            1 => Ok(GateCond::ZeroDiff),
            2 => Ok(GateCond::FailClassified),
            3 => Ok(GateCond::ArchiveReady),
            _ => Err(E_COND_RANGE),
        }
    }

    /// 条件名（摘要拼装）。
    pub const fn name(self) -> &'static str {
        match self {
            GateCond::SuccessRate => "成功率达标",
            GateCond::ZeroDiff => "零差异全同",
            GateCond::FailClassified => "失败分类完备",
            GateCond::ArchiveReady => "归档齐备",
        }
    }
}

// ── 判定双臂闭集 ─────────────────────────────────────────────────────
/// 门禁判定双臂闭集（二值零灰区，无第三态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateDecision {
    /// 放行：四条件全真。
    Release,
    /// 拦截：任一缺位。
    Blocked,
}

impl GateDecision {
    /// 臂位。
    pub const fn bit(self) -> u8 {
        match self {
            GateDecision::Release => 1,
            GateDecision::Blocked => 0,
        }
    }
}

// ── 门禁输入面 ───────────────────────────────────────────────────────
/// 门禁判定输入：成功率 ppm + 四条件位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GateInput {
    /// 成功率（ppm 0..=10000，条件 0 的数值源）。
    pub ppm: u64,
    /// 四条件位（逐位 0/1；条件 0 位由 ppm 派生，此处为复算锚）。
    pub cond_bits: [u8; COND_COUNT],
}

impl GateInput {
    /// 输入合法性：ppm 域 + 条件位二值。
    pub fn validate(&self) -> Result<(), u16> {
        if self.ppm > PPM_SCALE {
            return Err(E_PPM_RANGE);
        }
        let mut c = 0usize;
        while c < COND_COUNT {
            if self.cond_bits[c] > 1 {
                return Err(E_COND_GRAY);
            }
            c += 1;
        }
        Ok(())
    }

    /// 条件 0 复算：ppm ≥ PASS_PPM（≥ 语义，边界 9900 恰达标）。
    pub const fn rate_pass(&self) -> bool {
        self.ppm >= PASS_PPM
    }
}

// ── 单门禁 ───────────────────────────────────────────────────────────
/// 单个门禁：条件位 + 缺件账 + 判定结论 + 收官联动。
pub struct GateUnit {
    /// 门禁号 1..=MAX_GATES。
    pub seq: u32,
    /// 判定输入。
    input: GateInput,
    /// 判定结论（None=未定判）。
    decision: Option<GateDecision>,
    /// 缺件账（拦截时逐条记录缺位条件号）。
    missing: Vec<u8>,
    /// 收官事件位（放行后写 1）。
    closed: bool,
}

impl GateUnit {
    /// 新门禁。
    pub fn new(seq: u32, input: GateInput) -> Result<Self, u16> {
        if seq < 1 || seq as usize > MAX_GATES {
            return Err(E_GATE_SEQ);
        }
        input.validate()?;
        Ok(GateUnit {
            seq,
            input,
            decision: None,
            missing: Vec::new(),
            closed: false,
        })
    }

    /// 判定：四条件全真 → Release；否则 Blocked + 逐条缺件记录。
    /// 判定后条件位冻结（E_DECIDED）。
    pub fn judge(&mut self) -> Result<GateDecision, u16> {
        if self.decision.is_some() {
            return Err(E_REJUDGE);
        }
        // 条件 0：数值复算（与输入位对冲——输入位必须与复算一致）。
        let rate = self.input.rate_pass() as u8;
        if rate != self.input.cond_bits[GateCond::SuccessRate.id()] {
            return Err(E_COND_GRAY);
        }
        let mut all = true;
        let mut c = 0usize;
        while c < COND_COUNT {
            if self.input.cond_bits[c] == 0 {
                all = false;
                if self.missing.len() >= COND_COUNT {
                    return Err(E_MISSING_FULL);
                }
                self.missing.push(c as u8);
            }
            c += 1;
        }
        let d = if all {
            GateDecision::Release
        } else {
            GateDecision::Blocked
        };
        self.decision = Some(d);
        Ok(d)
    }

    /// 收官联动：放行门禁写收官事件；拦截门禁拒收。
    pub fn close_domain(&mut self) -> Result<(), u16> {
        match self.decision {
            Some(GateDecision::Release) => {
                if self.closed {
                    return Err(E_REJUDGE);
                }
                self.closed = true;
                Ok(())
            }
            Some(GateDecision::Blocked) => Err(E_NOT_RELEASED),
            None => Err(E_DECISION_OPEN),
        }
    }

    /// 判定结论解引。
    pub fn decision(&self) -> Result<GateDecision, u16> {
        match self.decision {
            Some(d) => Ok(d),
            None => Err(E_DECISION_OPEN),
        }
    }

    /// 缺件账只读。
    pub fn missing(&self) -> &[u8] {
        &self.missing
    }

    /// 收官位。
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// 输入只读。
    pub const fn input(&self) -> &GateInput {
        &self.input
    }
}

// ── 域门禁账（收官联动载体） ─────────────────────────────────────────
/// 域门禁账：多门禁定容账 + 收官联动守恒（收官数 == 放行数恒等）。
pub struct DomainGate {
    /// 门禁账（定容 MAX_GATES）。
    gates: Vec<GateUnit>,
    /// 放行计数。
    released: u32,
    /// 拦截计数。
    blocked: u32,
    /// 收官事件计数（守恒：== released）。
    closed_events: u32,
    /// 封板位。
    sealed: bool,
}

impl DomainGate {
    /// 新域账。
    pub fn new() -> Self {
        DomainGate {
            gates: Vec::new(),
            released: 0,
            blocked: 0,
            closed_events: 0,
            sealed: false,
        }
    }

    /// 挂门禁。
    pub fn attach(&mut self, input: GateInput) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_DECIDED);
        }
        if self.gates.len() >= MAX_GATES {
            return Err(E_GATE_SEQ);
        }
        let seq = self.gates.len() as u32 + 1;
        self.gates.push(GateUnit::new(seq, input)?);
        Ok(seq)
    }

    /// 门禁只读。
    pub fn gate(&self, seq: usize) -> Result<&GateUnit, u16> {
        if seq < 1 || seq > self.gates.len() {
            return Err(E_GATE_SEQ);
        }
        Ok(&self.gates[seq - 1])
    }

    /// 门禁判定 + 联动记账。
    pub fn judge_gate(&mut self, seq: usize) -> Result<GateDecision, u16> {
        if seq < 1 || seq > self.gates.len() {
            return Err(E_GATE_SEQ);
        }
        let d = self.gates[seq - 1].judge()?;
        match d {
            GateDecision::Release => {
                self.released = self.released.wrapping_add(1);
            }
            GateDecision::Blocked => {
                self.blocked = self.blocked.wrapping_add(1);
            }
        }
        Ok(d)
    }

    /// 收官联动：放行门禁逐个写收官事件。
    pub fn close_gate(&mut self, seq: usize) -> Result<(), u16> {
        if seq < 1 || seq > self.gates.len() {
            return Err(E_GATE_SEQ);
        }
        self.gates[seq - 1].close_domain()?;
        self.closed_events = self.closed_events.wrapping_add(1);
        Ok(())
    }

    /// 守恒审计：收官数 == 放行数；released + blocked == 门禁总数。
    pub fn audit_conservation(&self) -> Result<(), u16> {
        if self.closed_events != self.released {
            return Err(E_CLOSE_IMBAL);
        }
        if self.released.wrapping_add(self.blocked) != self.gates.len() as u32 {
            return Err(E_CONSERVATION_UNUSED);
        }
        Ok(())
    }

    /// 缺件账逐条核对：拦截门禁缺件号全在闭集内（悬空检出）。
    pub fn audit_missing(&self) -> Result<(), u16> {
        let mut g = 0usize;
        while g < self.gates.len() {
            let gate = &self.gates[g];
            let m = gate.missing();
            let mut i = 0usize;
            while i < m.len() {
                if m[i] as usize >= COND_COUNT {
                    return Err(E_MISSING_DANGLE);
                }
                i += 1;
            }
            // 拦截门禁缺件非空；放行门禁缺件必空。
            match gate.decision() {
                Ok(GateDecision::Blocked) => {
                    if m.is_empty() {
                        return Err(E_GATE_BLOCK);
                    }
                }
                Ok(GateDecision::Release) => {
                    if !m.is_empty() {
                        return Err(E_GATE_BLOCK);
                    }
                }
                Err(_) => {}
            }
            g += 1;
        }
        Ok(())
    }

    /// 放行/拦截计数。
    pub const fn released(&self) -> u32 {
        self.released
    }

    pub const fn blocked(&self) -> u32 {
        self.blocked
    }

    pub const fn closed_events(&self) -> u32 {
        self.closed_events
    }

    /// 全指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut g = 0usize;
        while g < self.gates.len() {
            let gate = &self.gates[g];
            h = fnv_mix(h, (gate.seq & 0xFF) as u8);
            let inp = gate.input();
            let mut shift = 56i32;
            while shift >= 0 {
                h = fnv_mix(h, ((inp.ppm >> shift) & 0xFF) as u8);
                shift -= 8;
            }
            let mut c = 0usize;
            while c < COND_COUNT {
                h = fnv_mix(h, inp.cond_bits[c] | 0x10);
                c += 1;
            }
            if let Ok(d) = gate.decision() {
                h = fnv_mix(h, d.bit() | 0x20);
            }
            h = fnv_mix(h, gate.is_closed() as u8 | 0x40);
            g += 1;
        }
        h
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.gates.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("域门禁摘要: 门禁=");
        out.push_str(&itoa(self.gates.len() as u64));
        out.push_str(" 放行=");
        out.push_str(&itoa(self.released as u64));
        out.push_str(" 拦截=");
        out.push_str(&itoa(self.blocked as u64));
        out.push_str(" 收官=");
        out.push_str(&itoa(self.closed_events as u64));
        out.push_str(" 指纹=");
        out.push_str(&hex16(self.fingerprint()));
        if out.len() < SUMMARY_MIN_LEN {
            return Err(E_SUMMARY_EMPTY);
        }
        Ok(out)
    }
}

// 占位常量（守恒审计第二式专用码，并入本段）。
const E_CONSERVATION_UNUSED: u16 = SEG_BASE + 0xE;

// ── 整数工具 ─────────────────────────────────────────────────────────
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return "0".to_string();
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

fn hex16(v: u64) -> String {
    const HEXD: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::new();
    s.push_str("0x");
    let mut shift = 60i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEXD[nib] as char);
        shift -= 4;
    }
    s
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 主路径：全绿门禁放行 + 收官联动 ——
    let mut dg = DomainGate::new();
    let full = GateInput {
        ppm: 9_900,
        cond_bits: [1, 1, 1, 1],
    };
    let s1 = dg.attach(full)?;
    if s1 != 1 {
        return Err(E_GATE_SEQ);
    }
    if dg.judge_gate(1)? != GateDecision::Release {
        return Err(E_GATE_BLOCK);
    }
    dg.close_gate(1)?;
    dg.audit_conservation()?;
    dg.audit_missing()?;

    // —— 缺件拦截 100%：逐条件缺位必拦 ——
    let mut c = 0usize;
    while c < COND_COUNT {
        let mut bits = [1u8; COND_COUNT];
        bits[c] = 0;
        let inp = GateInput {
            ppm: 10_000,
            cond_bits: bits,
        };
        let seq = dg.attach(inp)?;
        if dg.judge_gate(seq as usize)? != GateDecision::Blocked {
            return Err(E_GATE_BLOCK);
        }
        // 缺件账恰 1 条且号码正确。
        let g = dg.gate(seq as usize)?;
        if g.missing().len() != 1 || g.missing()[0] as usize != c {
            return Err(E_MISSING_DANGLE);
        }
        // 拦截门禁收官必拦。
        if dg.close_gate(seq as usize) != Err(E_NOT_RELEASED) {
            return Err(E_NOT_RELEASED);
        }
        c += 1;
    }
    // 多缺件：两位缺 → 缺件账 2 条。
    let two = GateInput {
        ppm: 9_000,
        cond_bits: [0, 1, 0, 1],
    };
    let seq2 = dg.attach(two)?;
    if dg.judge_gate(seq2 as usize)? != GateDecision::Blocked {
        return Err(E_GATE_BLOCK);
    }
    if dg.gate(seq2 as usize)?.missing().len() != 2 {
        return Err(E_MISSING_DANGLE);
    }
    // 拦截门禁放行守恒不被破坏：收官事件数仍 == 放行数。
    dg.audit_conservation()?;
    let sm = dg.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }

    // —— 边界：ppm 恰 9900 达标（≥ 语义） ——
    let mut edge = GateUnit::new(1, GateInput {
        ppm: PASS_PPM,
        cond_bits: [1, 1, 1, 1],
    })?;
    if edge.judge()? != GateDecision::Release {
        return Err(E_GATE_BLOCK);
    }
    // 边界下沿：9800 不达标。
    let mut under = GateUnit::new(2, GateInput {
        ppm: 9_800,
        cond_bits: [0, 1, 1, 1],
    })?;
    if under.judge()? != GateDecision::Blocked {
        return Err(E_GATE_BLOCK);
    }

    // —— 错误路径样本 ——
    if GateCond::from_id(4) != Err(E_COND_RANGE) {
        return Err(E_COND_RANGE);
    }
    if GateUnit::new(0, full).err() != Some(E_GATE_SEQ) {
        return Err(E_GATE_SEQ);
    }
    if GateUnit::new(MAX_GATES as u32 + 1, full).err() != Some(E_GATE_SEQ) {
        return Err(E_GATE_SEQ);
    }
    let gray = GateInput {
        ppm: 9_900,
        cond_bits: [1, 2, 1, 1],
    };
    if GateUnit::new(1, gray).err() != Some(E_COND_GRAY) {
        return Err(E_COND_GRAY);
    }
    // ppm 越域。
    let big = GateInput {
        ppm: 10_001,
        cond_bits: [1, 1, 1, 1],
    };
    if GateUnit::new(1, big).err() != Some(E_PPM_RANGE) {
        return Err(E_PPM_RANGE);
    }
    // 未判即收官 / 未判即解引。
    let mut open = GateUnit::new(1, full)?;
    if open.close_domain() != Err(E_DECISION_OPEN) {
        return Err(E_DECISION_OPEN);
    }
    if open.decision() != Err(E_DECISION_OPEN) {
        return Err(E_DECISION_OPEN);
    }
    open.judge()?;
    // 二次判必拦。
    if open.judge() != Err(E_REJUDGE) {
        return Err(E_REJUDGE);
    }
    // 条件 0 位与 ppm 复算不一致（输入位灰区语义冲突）。
    let mut clash = GateUnit::new(1, GateInput {
        ppm: 9_900,
        cond_bits: [0, 1, 1, 1],
    })?;
    if clash.judge() != Err(E_COND_GRAY) {
        return Err(E_COND_GRAY);
    }
    // 悬空门禁号。
    if dg.gate(0).err() != Some(E_GATE_SEQ) {
        return Err(E_GATE_SEQ);
    }
    if dg.gate(dg.gates.len() + 1).err() != Some(E_GATE_SEQ) {
        return Err(E_GATE_SEQ);
    }
    // 收官失衡注入：收官账拉爆必拦。
    let mut bad = DomainGate::new();
    let sb = bad.attach(full)?;
    bad.judge_gate(sb as usize)?;
    bad.closed_events = bad.closed_events.wrapping_add(1);
    if bad.audit_conservation() != Err(E_CLOSE_IMBAL) {
        return Err(E_CLOSE_IMBAL);
    }

    // 诊断表抽查。
    if diag_detail(E_GATE_BLOCK) == "未知诊断码（0x57c0 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x57c0 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}
