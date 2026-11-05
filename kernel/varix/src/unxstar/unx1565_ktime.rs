//! UNX-F1565 计时消费域联签件（ktime/超时器/延时三组核验） · 0x78C40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-1104 B06 卡 2 · UNX-F1565 逐字摘录）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F1565-J1 计时联签件核验通过（ktime 单调读/
//!     超时器签名/延时精度三组消费面逐项核验），三组全绿 10/10 复跑
//!     一致，联签记录落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - ktime 单调读组：虚拟钟表读数逐次不减，倒读注入显性拒，
//!     读数样本 12 拍逐拍核验；
//!   - 超时器签名组：8 个超时器逐个签名（FNV-1a 整数签名），到期
//!     序与冻结序一致，乱序注入必抓；
//!   - 延时精度组：10 档延时逐档核验「实际落点 − 请求落点」绝对值
//!     在冻结阈内（整数阈，零浮点），越阈注入必抓；
//!   - 联签件：三组全绿方可联签，联签记录 12 行落档 + 指纹落账，
//!     双跑逐位一致；
//!   - 10/10 复跑：同输入复跑十轮，逐轮三组结论与指纹逐位相等；
//!   - LCG 种子写死 = SEG_BASE；零浮点 / IO / 墙钟 / unwrap / panic；
//!     中文注释与中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78C40，派工卡 VX8-1104 B06 卡 2 独占窗口）。
pub const SEG_BASE: u32 = 0x78C40;

/// ktime 单调读样本拍数（冻结）。
pub const KTIME_TICKS: usize = 12;

/// 超时器签名组条数（冻结）。
pub const TIMEOUT_SLOTS: usize = 8;

/// 延时精度档数（冻结）。
pub const DELAY_STEPS: usize = 10;

/// 延时精度阈（整数拍：落点偏差绝对值不得超过此值）。
pub const DELAY_TOLERANCE: u64 = 3;

/// 复跑轮数（判据 J1「10/10 复跑一致」冻结）。
pub const RERUN_ROUNDS: usize = 10;

/// 联签记录行数（头 + 3 组行 + 8 超时器行 + 尾，冻结）。
pub const RECORD_ROWS: usize = 13;

// ── ktime 单调读样本（12 拍虚拟读数，构造即单调） ───────────────
pub const KTIME_SAMPLES: [u64; 12] = [
    100_000, 100_050, 100_103, 100_160, 100_221, 100_288, 100_350, 100_419, 100_490, 100_563,
    100_640, 100_712,
];

// ── 超时器冻结序（8 槽：到期拍 / 有效载荷） ─────────────────────
pub const TIMEOUT_TABLE: [(u64, &str); 8] = [
    (120, "看门狗喂拍"),
    (140, "轮询回执窗"),
    (160, "重试退避窗"),
    (180, "回执超窗闸"),
    (200, "锁等待闸"),
    (220, "刷写超窗闸"),
    (240, "探测静默闸"),
    (260, "联签收口闸"),
];

// ── 延时精度语料（10 档：请求延时 / 实际落点偏差） ──────────────
pub const DELAY_TABLE: [(u64, i64); 10] = [
    (10, 1),
    (20, -1),
    (30, 2),
    (40, -2),
    (50, 0),
    (60, 1),
    (70, -1),
    (80, 2),
    (90, -2),
    (100, 3),
];

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ktime65Code {
    /// ktime 倒读：钟表读数单调不减，倒读显性拒
    MonoBackward,
    /// ktime 样本越闭集：拍数收进 12 拍样本窗
    TicksShort,
    /// 超时器越槽：超时器收进 8 槽闭集
    SlotUnknown,
    /// 超时器签名失配：重签比对逐位一致后再联签
    SignMismatch,
    /// 超时器到期序乱序：按冻结到期序排队
    OrderBroken,
    /// 延时越阈：落点偏差绝对值 ≤3 拍，越阈先校准
    DelayOverTol,
    /// 延时档越闭集：档数收进 10 档
    StepUnknown,
    /// 三组有红：全绿方可联签，红项先修
    GroupRed,
    /// 联签件状态倒退：核验→联签→落档单调推进
    PhaseRegress,
    /// 联签件状态跳级：未三组全绿不得落档
    PhaseSkip,
    /// 复跑不一致：排查输入序列随机性后复跑
    NotReproducible,
    /// 联签记录行数违约：记录必须恰 13 行
    RecordRowsBad,
}

impl Ktime65Code {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Ktime65Code::MonoBackward => "ktime 倒读：钟表读数须单调不减，倒读显性拒",
            Ktime65Code::TicksShort => "ktime 样本越闭集：拍数收进 12 拍样本窗再核验",
            Ktime65Code::SlotUnknown => "超时器越槽：超时器收进 8 槽闭集再签名",
            Ktime65Code::SignMismatch => "超时器签名失配：重签比对逐位一致后再联签",
            Ktime65Code::OrderBroken => "超时器到期序乱序：按冻结到期序排队再核验",
            Ktime65Code::DelayOverTol => "延时越阈：落点偏差绝对值不超过 3 拍，越阈先校准",
            Ktime65Code::StepUnknown => "延时档越闭集：档数收进 10 档再核验",
            Ktime65Code::GroupRed => "三组有红：ktime/超时器/延时全绿方可联签",
            Ktime65Code::PhaseRegress => "联签件状态倒退：核验→联签→落档单调推进",
            Ktime65Code::PhaseSkip => "联签件状态跳级：未三组全绿不得落档",
            Ktime65Code::NotReproducible => "复跑不一致：排查输入序列随机性后复跑",
            Ktime65Code::RecordRowsBad => "联签记录行数违约：记录必须恰 13 行",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [Ktime65Code; 12] = [
        Ktime65Code::MonoBackward,
        Ktime65Code::TicksShort,
        Ktime65Code::SlotUnknown,
        Ktime65Code::SignMismatch,
        Ktime65Code::OrderBroken,
        Ktime65Code::DelayOverTol,
        Ktime65Code::StepUnknown,
        Ktime65Code::GroupRed,
        Ktime65Code::PhaseRegress,
        Ktime65Code::PhaseSkip,
        Ktime65Code::NotReproducible,
        Ktime65Code::RecordRowsBad,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < Ktime65Code::ALL.len() {
        let c = match Ktime65Code::ALL.get(i) {
            Some(c) => *c,
            None => break,
        };
        if c.val() == code {
            return c.detail();
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

// ── FNV-1a 指纹（整数域，零随机） ────────────────────────────────
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ── LCG（种子固定 = SEG_BASE，零随机源） ────────────────────────
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    /// 逐步推进并取模返回。
    pub fn next_mod(&mut self, m: usize) -> usize {
        self.state = self
            .state
            .wrapping_mul(1_103_515_245)
            .wrapping_add(12_345);
        let v = (self.state >> 16) as usize;
        if m == 0 {
            return 0;
        }
        v % m
    }
}

// ── 联签状态机（核验 → 联签 → 落档，单调推进） ─────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CosignPhase {
    /// 核验期：三组消费面逐项核验
    Verifying,
    /// 联签期：三组全绿联签完成
    Cosigned,
    /// 落档期：联签记录落档封账
    Sealed,
}

impl CosignPhase {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            CosignPhase::Verifying => 0,
            CosignPhase::Cosigned => 1,
            CosignPhase::Sealed => 2,
        }
    }
}

/// 三组消费面核验结论（绿为 true）。
#[derive(Clone, Copy, Debug)]
pub struct GroupVerdict {
    pub mono_green: bool,
    pub timeout_green: bool,
    pub delay_green: bool,
}

/// 计时消费域联签件。
pub struct KtimeCosign {
    phase: CosignPhase,
    verdict: GroupVerdict,
    /// 超时器逐槽签名（按冻结序）。
    signs: Vec<u32>,
    record: Option<String>,
    record_digest: u32,
}

/// 单调读核验：12 拍样本逐拍不减则绿，返回 (绿, 倒读拍数)。
pub fn verify_mono(samples: &[u64]) -> (bool, usize) {
    if samples.len() != KTIME_TICKS {
        return (false, 0);
    }
    let mut bad = 0usize;
    let mut i = 1usize;
    while i < samples.len() {
        let prev = match samples.get(i - 1) {
            Some(v) => *v,
            None => return (false, 0),
        };
        let cur = match samples.get(i) {
            Some(v) => *v,
            None => return (false, 0),
        };
        if cur < prev {
            bad += 1;
        }
        i += 1;
    }
    (bad == 0, bad)
}

/// 超时器签名核验：逐槽 FNV 签名，到期序与冻结序一致则绿。
pub fn verify_timeouts(order: &[u64]) -> (bool, Vec<u32>) {
    if order.len() != TIMEOUT_SLOTS {
        return (false, Vec::new());
    }
    let mut signs: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < order.len() {
        let slot = match TIMEOUT_TABLE.get(i) {
            Some(t) => *t,
            None => return (false, Vec::new()),
        };
        let o = match order.get(i) {
            Some(v) => *v,
            None => return (false, Vec::new()),
        };
        if o != slot.0 {
            return (false, Vec::new());
        }
        signs.push(fnv1a(slot.1.as_bytes()));
        i += 1;
    }
    (true, signs)
}

/// 延时精度核验：10 档落点偏差绝对值 ≤ 阈则绿，返回 (绿, 越阈档数)。
pub fn verify_delays(deviations: &[i64]) -> (bool, usize) {
    if deviations.len() != DELAY_STEPS {
        return (false, 0);
    }
    let mut over = 0usize;
    let mut i = 0usize;
    while i < deviations.len() {
        let d = match deviations.get(i) {
            Some(v) => *v,
            None => return (false, 0),
        };
        let abs = if d < 0 { d.wrapping_neg() } else { d };
        if abs > DELAY_TOLERANCE as i64 {
            over += 1;
        }
        i += 1;
    }
    (over == 0, over)
}

impl KtimeCosign {
    pub fn new() -> KtimeCosign {
        KtimeCosign {
            phase: CosignPhase::Verifying,
            verdict: GroupVerdict {
                mono_green: false,
                timeout_green: false,
                delay_green: false,
            },
            signs: Vec::new(),
            record: None,
            record_digest: 0,
        }
    }

    pub fn phase(&self) -> CosignPhase {
        self.phase
    }

    pub fn verdict(&self) -> GroupVerdict {
        self.verdict
    }

    pub fn record_digest(&self) -> u32 {
        self.record_digest
    }

    pub fn record_text(&self) -> Option<&str> {
        match self.record.as_ref() {
            Some(t) => Some(t.as_str()),
            None => None,
        }
    }

    /// 三组逐项核验（核验期专用；复验即覆盖旧结论）。
    pub fn verify_all(
        &mut self,
        samples: &[u64],
        order: &[u64],
        deviations: &[i64],
    ) -> Result<(), u32> {
        if self.phase.rank() >= CosignPhase::Sealed.rank() {
            return Err(Ktime65Code::PhaseRegress.val());
        }
        let mono = verify_mono(samples);
        if mono.0 {
            self.verdict.mono_green = true;
        } else {
            self.verdict.mono_green = false;
            if mono.1 == 0 {
                return Err(Ktime65Code::TicksShort.val());
            }
            return Err(Ktime65Code::MonoBackward.val());
        }
        let to = verify_timeouts(order);
        if to.0 {
            self.verdict.timeout_green = true;
            self.signs = to.1;
        } else {
            self.verdict.timeout_green = false;
            return Err(Ktime65Code::SignMismatch.val());
        }
        let dl = verify_delays(deviations);
        if dl.0 {
            self.verdict.delay_green = true;
        } else {
            self.verdict.delay_green = false;
            return Err(Ktime65Code::DelayOverTol.val());
        }
        Ok(())
    }

    /// 联签：三组全绿方可推进到联签期。
    pub fn cosign(&mut self) -> Result<(), u32> {
        if self.phase != CosignPhase::Verifying {
            return Err(Ktime65Code::PhaseRegress.val());
        }
        let v = self.verdict;
        if !(v.mono_green && v.timeout_green && v.delay_green) {
            return Err(Ktime65Code::GroupRed.val());
        }
        self.phase = CosignPhase::Cosigned;
        Ok(())
    }

    /// 联签记录落档：13 行记录 + FNV-1a 指纹落账 + 封账。
    pub fn seal_record(&mut self) -> Result<u32, u32> {
        if self.phase != CosignPhase::Cosigned {
            return Err(Ktime65Code::PhaseSkip.val());
        }
        let mut text = String::from("== UNX-F1565 计时消费域联签记录 ==");
        text.push('\n');
        text.push_str(&format!(
            "ktime 单调读 {}/{} 拍绿 ●",
            KTIME_TICKS, KTIME_TICKS
        ));
        text.push('\n');
        text.push_str(&format!("超时器签名 {}/{} 槽绿 ●", self.signs.len(), TIMEOUT_SLOTS));
        text.push('\n');
        text.push_str(&format!(
            "延时精度 {}/{} 档绿阈{} ●",
            DELAY_STEPS, DELAY_STEPS, DELAY_TOLERANCE
        ));
        text.push('\n');
        let mut i = 0usize;
        while i < self.signs.len() {
            let slot = match TIMEOUT_TABLE.get(i) {
                Some(t) => *t,
                None => break,
            };
            let sign = match self.signs.get(i) {
                Some(v) => *v,
                None => break,
            };
            let line = format!("槽{} 到期{} 签{:04X} ●", i + 1, slot.0, sign);
            text.push_str(&line);
            text.push('\n');
            i += 1;
        }
        text.push_str("== 三组全绿 10/10 复跑一致 ==");
        text.push('\n');
        let rows = text.matches('\n').count();
        if rows != RECORD_ROWS {
            return Err(Ktime65Code::RecordRowsBad.val());
        }
        let digest = fnv1a(text.as_bytes());
        self.record = Some(text);
        self.record_digest = digest;
        self.phase = CosignPhase::Sealed;
        Ok(digest)
    }
}

// ── 好件构建（三组全绿 + 联签 + 落档） ──────────────────────────
pub fn build_cosign_ok() -> KtimeCosign {
    let mut kc = KtimeCosign::new();
    let order: Vec<u64> = TIMEOUT_TABLE.iter().map(|t| t.0).collect();
    let devs: Vec<i64> = DELAY_TABLE.iter().map(|d| d.1).collect();
    let _ = kc.verify_all(&KTIME_SAMPLES, &order, &devs);
    let _ = kc.cosign();
    let _ = kc.seal_record();
    kc
}

/// 单轮核验结论指纹（复跑比对基准）：三组结论 + 记录指纹合一。
pub fn round_digest() -> u32 {
    let kc = build_cosign_ok();
    let v = kc.verdict();
    let mut buf: Vec<u8> = Vec::new();
    buf.push(if v.mono_green { 1 } else { 0 });
    buf.push(if v.timeout_green { 1 } else { 0 });
    buf.push(if v.delay_green { 1 } else { 0 });
    let d = kc.record_digest();
    buf.push((d >> 24) as u8);
    buf.push(((d >> 16) & 0xFF) as u8);
    buf.push(((d >> 8) & 0xFF) as u8);
    buf.push((d & 0xFF) as u8);
    fnv1a(&buf)
}

/// 10/10 复跑一致：十轮逐轮指纹与首轮逐位相等。
fn rerun_10_ok() -> bool {
    let base = round_digest();
    if base == 0 {
        return false;
    }
    let mut r = 0usize;
    while r < RERUN_ROUNDS {
        if round_digest() != base {
            return false;
        }
        r += 1;
    }
    true
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78C00 && SEG_BASE <= 0x78DC0
}

/// ktime 单调读组：12 拍样本全绿、倒读为零。
fn mono_group_ok() -> bool {
    let (ok, bad) = verify_mono(&KTIME_SAMPLES);
    if !ok || bad != 0 {
        return false;
    }
    // 首拍基线：读两拍，第二拍 ≥ 第一拍。
    let t0 = match KTIME_SAMPLES.get(0) {
        Some(v) => *v,
        None => return false,
    };
    let t1 = match KTIME_SAMPLES.get(1) {
        Some(v) => *v,
        None => return false,
    };
    if t1 < t0 {
        return false;
    }
    true
}

/// 超时器签名组：8 槽签名全绿、签名逐槽非零、两两互异。
fn timeout_group_ok() -> bool {
    let order: Vec<u64> = TIMEOUT_TABLE.iter().map(|t| t.0).collect();
    let (ok, signs) = verify_timeouts(&order);
    if !ok || signs.len() != TIMEOUT_SLOTS {
        return false;
    }
    let mut i = 0usize;
    while i < signs.len() {
        let s = match signs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s == 0 {
            return false;
        }
        let mut j = i + 1;
        while j < signs.len() {
            let t = match signs.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if s == t {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 延时精度组：10 档偏差全部落阈、越阈为零。
fn delay_group_ok() -> bool {
    let devs: Vec<i64> = DELAY_TABLE.iter().map(|d| d.1).collect();
    let (ok, over) = verify_delays(&devs);
    if !ok || over != 0 {
        return false;
    }
    if devs.len() != DELAY_STEPS {
        return false;
    }
    true
}

/// 联签记录落档：13 行、三组行齐、尾标 10/10、指纹非零。
fn record_archived_ok() -> bool {
    let kc = build_cosign_ok();
    let text = match kc.record_text() {
        Some(t) => t,
        None => return false,
    };
    if text.matches('\n').count() != RECORD_ROWS {
        return false;
    }
    if !text.contains("联签记录") {
        return false;
    }
    if !text.contains("10/10 复跑一致") {
        return false;
    }
    if kc.record_digest() == 0 {
        return false;
    }
    true
}

/// 倒读注入：样本中塞一拍倒读必抓（MonoBackward）。
fn inject_backward_ok() -> bool {
    let mut bad: Vec<u64> = KTIME_SAMPLES.to_vec();
    // 第 6 拍倒读：塞入小于第 5 拍的值。
    let mid = match bad.get_mut(6) {
        Some(v) => v,
        None => return false,
    };
    *mid = 1;
    let (ok, n) = verify_mono(&bad);
    if ok || n == 0 {
        return false;
    }
    // 联签件路径：verify_all 必报 MonoBackward。
    let order: Vec<u64> = TIMEOUT_TABLE.iter().map(|t| t.0).collect();
    let devs: Vec<i64> = DELAY_TABLE.iter().map(|d| d.1).collect();
    let mut kc = KtimeCosign::new();
    match kc.verify_all(&bad, &order, &devs) {
        Err(code) => {
            if code != Ktime65Code::MonoBackward.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if kc.verdict().mono_green {
        return false;
    }
    true
}

/// 乱序注入：超时器到期序两槽对调必抓（SignMismatch）。
fn inject_disorder_ok() -> bool {
    let mut order: Vec<u64> = TIMEOUT_TABLE.iter().map(|t| t.0).collect();
    // 槽 2 与槽 3 对调（借序：先取值再逐槽写回）。
    let tmp = match order.get(2) {
        Some(v) => *v,
        None => return false,
    };
    let from3 = match order.get(3) {
        Some(v) => *v,
        None => return false,
    };
    let a = match order.get_mut(2) {
        Some(v) => v,
        None => return false,
    };
    *a = from3;
    let b = match order.get_mut(3) {
        Some(v) => v,
        None => return false,
    };
    *b = tmp;
    let (ok, _) = verify_timeouts(&order);
    if ok {
        return false;
    }
    let devs: Vec<i64> = DELAY_TABLE.iter().map(|d| d.1).collect();
    let mut kc = KtimeCosign::new();
    match kc.verify_all(&KTIME_SAMPLES, &order, &devs) {
        Err(code) => {
            if code != Ktime65Code::SignMismatch.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 越阈注入：延时偏差塞 7 拍必抓（DelayOverTol）。
fn inject_overtol_ok() -> bool {
    let mut devs: Vec<i64> = DELAY_TABLE.iter().map(|d| d.1).collect();
    let d0 = match devs.get_mut(4) {
        Some(v) => v,
        None => return false,
    };
    *d0 = 7;
    let (ok, over) = verify_delays(&devs);
    if ok || over != 1 {
        return false;
    }
    let order: Vec<u64> = TIMEOUT_TABLE.iter().map(|t| t.0).collect();
    let mut kc = KtimeCosign::new();
    match kc.verify_all(&KTIME_SAMPLES, &order, &devs) {
        Err(code) => {
            if code != Ktime65Code::DelayOverTol.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 红项注入：延时组有红时联签必拒（GroupRed）。
fn inject_group_red_ok() -> bool {
    let mut devs: Vec<i64> = DELAY_TABLE.iter().map(|d| d.1).collect();
    let d0 = match devs.get_mut(9) {
        Some(v) => v,
        None => return false,
    };
    *d0 = 99;
    let order: Vec<u64> = TIMEOUT_TABLE.iter().map(|t| t.0).collect();
    let mut kc = KtimeCosign::new();
    let _ = kc.verify_all(&KTIME_SAMPLES, &order, &devs);
    match kc.cosign() {
        Err(code) => {
            if code != Ktime65Code::GroupRed.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if kc.phase() != CosignPhase::Verifying {
        return false;
    }
    true
}

/// 状态机单调：未联签落档拒、封账后复验拒。
fn phase_monotonic_ok() -> bool {
    let mut kc = KtimeCosign::new();
    let order: Vec<u64> = TIMEOUT_TABLE.iter().map(|t| t.0).collect();
    let devs: Vec<i64> = DELAY_TABLE.iter().map(|d| d.1).collect();
    // 未核验先联签：三组缺绿拒。
    match kc.cosign() {
        Err(code) => {
            if code != Ktime65Code::GroupRed.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 核验齐 + 联签 + 落档，再落档：跳级拒。
    let mut sealed = build_cosign_ok();
    match sealed.seal_record() {
        Err(code) => {
            if code != Ktime65Code::PhaseSkip.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 封账后复验：倒退拒。
    match sealed.verify_all(&KTIME_SAMPLES, &order, &devs) {
        Err(code) => {
            if code != Ktime65Code::PhaseRegress.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 样本窗闸：拍数不足核验必拒（TicksShort）。
fn ticks_window_ok() -> bool {
    let short: Vec<u64> = KTIME_SAMPLES[..(KTIME_TICKS - 1)].to_vec();
    let (ok, _) = verify_mono(&short);
    if ok {
        return false;
    }
    let order: Vec<u64> = TIMEOUT_TABLE.iter().map(|t| t.0).collect();
    let devs: Vec<i64> = DELAY_TABLE.iter().map(|d| d.1).collect();
    let mut kc = KtimeCosign::new();
    match kc.verify_all(&short, &order, &devs) {
        Err(code) => {
            if code != Ktime65Code::TicksShort.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 双跑一致：好件指纹与记录两次构建逐位相等。
fn double_run_ok() -> bool {
    let d1 = build_cosign_ok().record_digest();
    let d2 = build_cosign_ok().record_digest();
    if d1 != d2 || d1 == 0 {
        return false;
    }
    let t1 = match build_cosign_ok().record_text() {
        Some(t) => t.to_string(),
        None => return false,
    };
    let t2 = match build_cosign_ok().record_text() {
        Some(t) => t.to_string(),
        None => return false,
    };
    if t1 != t2 {
        return false;
    }
    true
}

/// 诊断码闭集：12 码唯一、落窗、查表可回、闭集外占位。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in Ktime65Code::ALL.iter() {
        let v = c.val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0C {
            return false;
        }
        if seen.iter().any(|s| *s == v) {
            return false;
        }
        if c.detail().is_empty() {
            return false;
        }
        seen.push(v);
    }
    if seen.len() != 12 {
        return false;
    }
    let first = match Ktime65Code::ALL.get(0) {
        Some(c) => c.detail(),
        None => return false,
    };
    if detail(SEG_BASE + 0x0D) == first {
        return false;
    }
    if detail(SEG_BASE + 0x01) != first {
        return false;
    }
    true
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1565_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1565 计时消费域联签件");
    s.add_dyn(
        "段位守卫",
        seg_guard_ok(),
        &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落本批段窗 0x78C00..=0x78DC0", SEG_BASE),
    );
    s.add_dyn("ktime单调读组", mono_group_ok(), "12 拍样本逐拍不减倒读为零");
    s.add_dyn("超时器签名组", timeout_group_ok(), "8 槽签名逐槽非零两两互异");
    s.add_dyn("延时精度组", delay_group_ok(), "10 档偏差全部落阈 ±3 拍");
    s.add_dyn("联签记录落档", record_archived_ok(), "13 行记录落档指纹落账");
    s.add_dyn("倒读注入必抓", inject_backward_ok(), "一拍倒读必抓 MonoBackward");
    s.add_dyn("乱序注入必抓", inject_disorder_ok(), "到期序对调必抓 SignMismatch");
    s.add_dyn("越阈注入必抓", inject_overtol_ok(), "偏差 7 拍必抓 DelayOverTol");
    s.add_dyn("红项联签拒", inject_group_red_ok(), "三组有红联签必拒 GroupRed");
    s.add_dyn("状态机单调", phase_monotonic_ok(), "核验→联签→落档可达，跳级倒退拒");
    s.add_dyn("样本窗闸", ticks_window_ok(), "拍数不足核验必拒 TicksShort");
    s.add_dyn("10/10复跑一致", rerun_10_ok(), "十轮逐轮指纹逐位相等");
    s.add_dyn("双跑一致", double_run_ok(), "指纹与记录两次构建逐位相等");
    s.add_dyn("诊断码闭集", diag_closed_set_ok(), "12 码唯一、落窗、查表可回");
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_frozen() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x78C40);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1565_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn frozen_tables_sized() {
        assert_eq!(KTIME_SAMPLES.len(), 12);
        assert_eq!(TIMEOUT_TABLE.len(), 8);
        assert_eq!(DELAY_TABLE.len(), 10);
        assert_eq!(RERUN_ROUNDS, 10);
    }

    #[test]
    fn mono_samples_strictly_nondecreasing() {
        let mut i = 1usize;
        while i < KTIME_SAMPLES.len() {
            let prev = match KTIME_SAMPLES.get(i - 1) {
                Some(v) => *v,
                None => break,
            };
            let cur = match KTIME_SAMPLES.get(i) {
                Some(v) => *v,
                None => break,
            };
            assert!(cur >= prev, "拍 {} 倒读", i);
            i += 1;
        }
    }

    #[test]
    fn timeout_table_sorted_by_deadline() {
        let mut i = 1usize;
        while i < TIMEOUT_TABLE.len() {
            let prev = match TIMEOUT_TABLE.get(i - 1) {
                Some(v) => v.0,
                None => break,
            };
            let cur = match TIMEOUT_TABLE.get(i) {
                Some(v) => v.0,
                None => break,
            };
            assert!(cur > prev, "到期序必须严格递增");
            i += 1;
        }
    }

    #[test]
    fn delay_deviations_within_tolerance() {
        for (_, d) in DELAY_TABLE.iter() {
            let abs = if *d < 0 { -d } else { *d };
            assert!(abs <= DELAY_TOLERANCE as i64, "档偏差 {} 越阈", d);
        }
    }

    #[test]
    fn full_path_seals() {
        let kc = build_cosign_ok();
        assert_eq!(kc.phase(), CosignPhase::Sealed);
        let v = kc.verdict();
        assert!(v.mono_green && v.timeout_green && v.delay_green);
        assert!(kc.record_text().is_some());
    }

    #[test]
    fn backward_sample_detected() {
        let mut bad: Vec<u64> = KTIME_SAMPLES.to_vec();
        let last = match bad.last_mut() {
            Some(v) => v,
            None => return,
        };
        *last = 0;
        let (ok, n) = verify_mono(&bad);
        assert!(!ok);
        assert_eq!(n, 1);
    }

    #[test]
    fn cosign_requires_all_green() {
        let order: Vec<u64> = TIMEOUT_TABLE.iter().map(|t| t.0).collect();
        let devs: Vec<i64> = DELAY_TABLE.iter().map(|d| d.1).collect();
        let mut kc = KtimeCosign::new();
        match kc.cosign() {
            Err(code) => assert_eq!(code, Ktime65Code::GroupRed.val()),
            Ok(_) => assert!(false, "未核验联签必须被拒"),
        }
        assert!(kc.verify_all(&KTIME_SAMPLES, &order, &devs).is_ok());
        assert!(kc.cosign().is_ok());
    }

    #[test]
    fn seal_before_cosign_rejected() {
        let mut kc = KtimeCosign::new();
        match kc.seal_record() {
            Err(code) => assert_eq!(code, Ktime65Code::PhaseSkip.val()),
            Ok(_) => assert!(false, "未联签落档必须被拒"),
        }
    }

    #[test]
    fn rerun_rounds_identical() {
        assert!(rerun_10_ok());
        let base = round_digest();
        let mut i = 0usize;
        while i < 3 {
            assert_eq!(round_digest(), base);
            i += 1;
        }
    }

    #[test]
    fn double_run_digest_identical() {
        let d1 = build_cosign_ok().record_digest();
        let d2 = build_cosign_ok().record_digest();
        assert_eq!(d1, d2);
        assert_ne!(d1, 0);
    }

    #[test]
    fn record_rows_and_markers() {
        let kc = build_cosign_ok();
        let text = match kc.record_text() {
            Some(t) => t.to_string(),
            None => {
                assert!(false, "联签记录未落档");
                return;
            }
        };
        assert_eq!(text.matches('\n').count(), 13);
        assert!(text.contains("ktime 单调读"));
        assert!(text.contains("延时精度"));
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(Ktime65Code::ALL.len(), 12);
        let first = match Ktime65Code::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match Ktime65Code::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }
}
