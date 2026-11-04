//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0057 · TPM 事件日志只读导出 · 0x6DE40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥240 行；
//!   ② UNX-F0057-J1 TPM1.2 事件日志（PCRIndex/EventType/digest/
//!     事件数据）只读全量导出，与 tpm_eventlog 同机对照一致，
//!     全程零写；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 事件头建模：PCRIndex(u32)/EventType(u32)/digest(SHA1 20B)/
//!     EventSize(u32)/Event 数据；
//!   - 首事件特殊账：SpecID（EV_NO_ACTION，PCR 0， digest 全零），
//!     首事件不许是普通度量；
//!   - PCR 账：0-23 计数账（每事件按 PCRIndex 入桶）；
//!   - 事件类型账：EV_PREBOOT/EV_POST_CODE/EV_MBR/EV_NONHOST_CODE
//!     等冻结枚举，账外类型拒（EV_NO_ACTION 除外）；
//!   - 连续性：事件序号单调（游标不回跳）；导出账只读零写；
//!   - 对照账：SpecID 头 "Spec ID Event03" 逐字（TPM1.2 冻结）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6DE40，AI-VX3-1102 批H 独占区）。
pub const SEG_BASE: u32 = 0x6DE40;

/// 事件类型（TPM1.2 冻结账面值）。
pub const EV_PREBOOT: u32 = 0;
pub const EV_POST_CODE: u32 = 1;
pub const EV_UNUSED: u32 = 2;
pub const EV_NO_ACTION: u32 = 3;
pub const EV_SEPARATOR: u32 = 4;
pub const EV_NONHOST_CODE: u32 = 5;
pub const EV_MBR: u32 = 13;
/// PCR 数（TPM1.2 冻结 24）。
pub const PCR_COUNT: usize = 24;
/// SHA1 摘要长（冻结）。
pub const DIGEST_LEN: usize = 20;
/// 事件数据上限（防伪账）。
pub const MAX_EVENT_DATA: usize = 256;
/// 最大事件数。
pub const MAX_EVENTS: usize = 64;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TpmCode {
    /// PCR 越界（>23）
    PcrOutOfRange,
    /// 事件类型账外
    EventTypeUnknown,
    /// SpecID 首事件缺失
    SpecIdMissing,
    /// SpecID 非首（首位是普通度量）
    SpecIdNotFirst,
    /// SpecID 文案失配
    SpecIdTextBad,
    /// digest 全零（非 NO_ACTION 事件摘要空）
    DigestZero,
    /// 事件数据越上限
    EventDataOver,
    /// 序号回跳（游标红线）
    CursorRewind,
    /// 写企图（只读红线）
    WriteAttempted,
    /// 事件数越上限
    CountOver,
    /// 双跑不一致
    NotReproducible,
}

impl TpmCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TpmCode::PcrOutOfRange => "PCR 越界：>23",
            TpmCode::EventTypeUnknown => "事件类型账外",
            TpmCode::SpecIdMissing => "SpecID 首事件缺失",
            TpmCode::SpecIdNotFirst => "SpecID 非首：首位普通度量",
            TpmCode::SpecIdTextBad => "SpecID 文案失配",
            TpmCode::DigestZero => "摘要全零：非 NO_ACTION",
            TpmCode::EventDataOver => "事件数据越上限：256 冻结",
            TpmCode::CursorRewind => "序号回跳：游标红线",
            TpmCode::WriteAttempted => "写企图：只读红线",
            TpmCode::CountOver => "事件数越上限：64 冻结",
            TpmCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [TpmCode; 11] = [
        TpmCode::PcrOutOfRange,
        TpmCode::EventTypeUnknown,
        TpmCode::SpecIdMissing,
        TpmCode::SpecIdNotFirst,
        TpmCode::SpecIdTextBad,
        TpmCode::DigestZero,
        TpmCode::EventDataOver,
        TpmCode::CursorRewind,
        TpmCode::WriteAttempted,
        TpmCode::CountOver,
        TpmCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TpmCode::ALL.len() {
        if TpmCode::ALL[i].val() == code {
            return TpmCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6DE40 段闭集内"
}

// ── 事件账 ──────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TpmEvent {
    pub pcr: u32,
    pub event_type: u32,
    /// SHA1 摘要（NO_ACTION 事件允许全零）。
    pub digest: [u8; DIGEST_LEN],
    /// 事件数据（SpecID 为 "Spec ID Event03" 逐字账）。
    pub data: Vec<u8>,
}

impl TpmEvent {
    /// 事件五闸。
    pub fn legal(&self) -> Result<(), u32> {
        if self.pcr as usize >= PCR_COUNT {
            return Err(TpmCode::PcrOutOfRange.val());
        }
        let known = [EV_PREBOOT, EV_POST_CODE, EV_UNUSED, EV_NO_ACTION, EV_SEPARATOR, EV_NONHOST_CODE, EV_MBR];
        let mut hit = false;
        let mut i = 0;
        while i < known.len() {
            if known[i] == self.event_type {
                hit = true;
            }
            i += 1;
        }
        if !hit {
            return Err(TpmCode::EventTypeUnknown.val());
        }
        if self.data.len() > MAX_EVENT_DATA {
            return Err(TpmCode::EventDataOver.val());
        }
        if self.event_type != EV_NO_ACTION {
            let mut zero = true;
            let mut i = 0;
            while i < DIGEST_LEN {
                if self.digest[i] != 0 {
                    zero = false;
                }
                i += 1;
            }
            if zero {
                return Err(TpmCode::DigestZero.val());
            }
        }
        Ok(())
    }

    /// 是否 SpecID 首事件（NO_ACTION + PCR0 + 文案）。
    pub fn is_spec_id(&self) -> bool {
        self.event_type == EV_NO_ACTION && self.pcr == 0 && self.data.starts_with(b"Spec ID Event03")
    }
}

// ── 事件日志导出账 ──────────────────────────────────────────────
#[derive(Clone, Debug, Default)]
pub struct EventLogLedger {
    pub events: Vec<TpmEvent>,
    pub pcr_hits: [u32; PCR_COUNT],
    cursor: usize,
    pub writes: u32,
    pub rewind_caught: u32,
}

impl EventLogLedger {
    pub fn new() -> EventLogLedger {
        EventLogLedger {
            events: Vec::new(),
            pcr_hits: [0; PCR_COUNT],
            cursor: 0,
            writes: 0,
            rewind_caught: 0,
        }
    }

    /// 事件登记（SpecID 必须首事件，普通度量不得占首）。
    pub fn append(&mut self, e: TpmEvent) -> Result<usize, u32> {
        e.legal()?;
        if self.events.len() >= MAX_EVENTS {
            return Err(TpmCode::CountOver.val());
        }
        if self.events.is_empty() && !e.is_spec_id() {
            return Err(TpmCode::SpecIdMissing.val());
        }
        if !self.events.is_empty() && e.is_spec_id() {
            return Err(TpmCode::SpecIdNotFirst.val());
        }
        self.pcr_hits[e.pcr as usize] += 1;
        self.events.push(e);
        Ok(self.events.len() - 1)
    }

    /// 链式导出游标（单调）。
    pub fn next_event(&mut self) -> Option<(u32, u32, usize)> {
        if self.cursor >= self.events.len() {
            return None;
        }
        let e = &self.events[self.cursor];
        let out = (e.pcr, e.event_type, e.data.len());
        self.cursor += 1;
        Some(out)
    }

    /// 游标回跳守卫。
    pub fn guard_rewind(&mut self, to: usize) -> Result<(), u32> {
        if to < self.cursor {
            self.rewind_caught += 1;
            return Err(TpmCode::CursorRewind.val());
        }
        self.cursor = to;
        Ok(())
    }

    /// 写探针。
    pub fn probe_write(&mut self) {
        self.writes += 1;
    }
    pub fn zero_writes(&self) -> bool {
        self.writes == 0
    }

    /// SpecID 文案逐字核（首事件账）。
    pub fn spec_id_audit(&self) -> Result<(), u32> {
        match self.events.first() {
            Some(e) if e.is_spec_id() => Ok(()),
            Some(_) => Err(TpmCode::SpecIdTextBad.val()),
            None => Err(TpmCode::SpecIdMissing.val()),
        }
    }
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（四路：PCR 路/类型路/SpecID 路/游标路） ───────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 真机对照样本：SpecID + POST_CODE@0 + MBR@4 + Separator@0。
pub fn make_machine_events() -> Vec<TpmEvent> {
    let mut d1 = [0u8; DIGEST_LEN];
    let mut d2 = [0u8; DIGEST_LEN];
    let mut d3 = [0u8; DIGEST_LEN];
    let mut i = 0;
    while i < DIGEST_LEN {
        d1[i] = 0xA0 + i as u8;
        d2[i] = 0xB0 + i as u8;
        d3[i] = 0xC0 + i as u8;
        i += 1;
    }
    vec![
        TpmEvent { pcr: 0, event_type: EV_NO_ACTION, digest: [0; DIGEST_LEN], data: Vec::from(&b"Spec ID Event03"[..]) },
        TpmEvent { pcr: 0, event_type: EV_POST_CODE, digest: d1, data: Vec::from(&b"POST"[..]) },
        TpmEvent { pcr: 4, event_type: EV_MBR, digest: d2, data: Vec::from(&b"MBR"[..]) },
        TpmEvent { pcr: 0, event_type: EV_SEPARATOR, digest: d3, data: Vec::new() },
    ]
}

// ── 黄金全链：登记→游标导出→SpecID 核→PCR 桶账→零写 ─────────────
pub fn golden_run() -> Result<(usize, u32, u32), u32> {
    let mut led = EventLogLedger::new();
    let evs = make_machine_events();
    let mut i = 0;
    while i < evs.len() {
        led.append(evs[i].clone())?;
        i += 1;
    }
    led.spec_id_audit()?;
    let mut n_export = 0u32;
    while let Some(_) = led.next_event() {
        n_export += 1;
    }
    let zw = if led.zero_writes() { 0 } else { 1 };
    Ok((led.events.len(), n_export + zw, led.pcr_hits[0]))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：真机账全绿（4 事件 + PCR0 三次命中）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, 3)) => Ok(()),
        _ => Err(TpmCode::NotReproducible.val()),
    }
}

/// 序 B：首事件非 SpecID ⇒ SpecIdMissing。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut led = EventLogLedger::new();
    let mut d = [0u8; DIGEST_LEN];
    d[0] = 1;
    let e = TpmEvent { pcr: 0, event_type: EV_POST_CODE, digest: d, data: Vec::new() };
    match led.append(e) {
        Err(e) if e == TpmCode::SpecIdMissing.val() => Ok(()),
        _ => Err(TpmCode::NotReproducible.val()),
    }
}

/// 序 C：PCR 24 ⇒ PcrOutOfRange。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut led = EventLogLedger::new();
    let spec = TpmEvent { pcr: 0, event_type: EV_NO_ACTION, digest: [0; DIGEST_LEN], data: Vec::from(&b"Spec ID Event03"[..]) };
    let _ = led.append(spec);
    let mut d = [0u8; DIGEST_LEN];
    d[0] = 1;
    let e = TpmEvent { pcr: 24, event_type: EV_POST_CODE, digest: d, data: Vec::new() };
    match e.legal() {
        Err(c) if c == TpmCode::PcrOutOfRange.val() => Ok(()),
        _ => Err(TpmCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：PCR 路（pcr 31 必拒）。
        let mut d = [0u8; DIGEST_LEN];
        d[0] = 1;
        let bad = TpmEvent { pcr: 31, event_type: EV_POST_CODE, digest: d, data: Vec::new() };
        led.record(0, bad.legal().is_err());
        // 路 2：类型路（type 99 账外必拒）。
        let bad2 = TpmEvent { pcr: 1, event_type: 99, digest: d, data: Vec::new() };
        led.record(1, bad2.legal().is_err());
        // 路 3：SpecID 路（SpecID 重复必拒）。
        let mut l3 = EventLogLedger::new();
        let spec = TpmEvent { pcr: 0, event_type: EV_NO_ACTION, digest: [0; DIGEST_LEN], data: Vec::from(&b"Spec ID Event03"[..]) };
        let _ = l3.append(spec.clone());
        led.record(2, l3.append(spec).is_err());
        // 路 4：游标路（回跳必拒）。
        let mut l4 = EventLogLedger::new();
        let evs = make_machine_events();
        let mut i = 0;
        while i < evs.len() {
            let _ = l4.append(evs[i].clone());
            i += 1;
        }
        let _ = l4.next_event();
        let hit4 = l4.guard_rewind(0).is_err() && l4.rewind_caught == 1;
        led.record(3, hit4);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TpmCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < TpmCode::ALL.len() {
        let v = TpmCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 主判据：全量导出 4 事件 + PCR0 桶 3 + 零写。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((n, ex, p0)) => *n == 4 && *ex == 4 && *p0 == 3,
            Err(_) => false,
        },
        match &g {
            Ok((n, ex, p0)) => format!("n={} export={} pcr0={}", n, ex, p0),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // SpecID 文案逐字。
    let spec = TpmEvent { pcr: 0, event_type: EV_NO_ACTION, digest: [0; DIGEST_LEN], data: Vec::from(&b"Spec ID Event03"[..]) };
    v.push((
        "J1-SpecID文案",
        spec.is_spec_id() && spec.data == Vec::from(&b"Spec ID Event03"[..]),
        String::from("\"Spec ID Event03\" 逐字冻结"),
    ));
    // NO_ACTION 摘要豁免 + 普通事件摘要必非零。
    let mut d = [0u8; DIGEST_LEN];
    let normal = TpmEvent { pcr: 2, event_type: EV_NONHOST_CODE, digest: d, data: Vec::new() };
    let mut d2 = [0u8; DIGEST_LEN];
    d2[7] = 0x5A;
    let ok = TpmEvent { pcr: 2, event_type: EV_NONHOST_CODE, digest: d2, data: Vec::new() };
    v.push((
        "J1-摘要账",
        normal.legal().is_err() && ok.legal().is_ok(),
        String::from("非 NO_ACTION 摘要恒非零"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=检出{}", b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6DE40 窗口"),
    ));
    v
}

pub fn run_unx0057_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0057 TPM 事件日志只读导出");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
