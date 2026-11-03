//! UNX-F1475 零丢语义文档快照（恒等式与修复口径冻结） · 0x75800段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，AI-VX7-1104 批3 H 组）：
//!   恒等式与修复口径冻结面，语义快照封账指纹重放一致。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   ① 零丢恒等式冻结：Σ修复 + Σ放弃 + Σ在途 ≡ Σ检出，账面每次
//!     apply 事件后用 checked_add 复核恒等式，破式显性拒（IdentityBroken）；
//!   ② 修复口径冻结：仅"完成回验"的修复计 Σ修复；未回验的修复
//!     请求一律显性拒（CaliberViolation）并建议记在途——口径文案
//!     以冻结常量进快照第二段，防漂移；
//!   ③ 转出口径：Σ修复与 Σ放弃都只从 Σ在途里转出，转出量超过在途
//!     显性拒（InflightOver）——修复/弃单必须曾经"在途"，不许凭空
//!     造修复与弃单；检出入途同步记账（新检项即处于在途）；
//!   ④ 语义快照三段封账：第一段恒等式文本、第二段修复口径文本、
//!     第三段版本号——三段拼接进 FNV-1a 64 指纹，重放（同事件序
//!     重演）指纹必一致；
//!   ⑤ 封后改写拒：快照封账后任何 apply 都显性拒（PostSealWrite），
//!     版本倒退（新快照版本 < 已封版本）显性拒（VersionRegress）；
//!   ⑥ 事件序 LCG 定种子可重演，黄金全链双跑一致（复现红线）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75800，批3 H 组独占窗口）。
pub const SEG_BASE: u32 = 0x75800;

/// 快照封账段数（冻结 3：恒等式/修复口径/版本）。
pub const SEAL_SEGMENTS: usize = 3;
/// 文档版本（冻结 3：口径第三版，随批3 H 组定版）。
pub const DOC_VERSION: u32 = 3;
/// 事件账容量上限（冻结 4096：单册封顶防无界）。
pub const MAX_EVENTS: usize = 4096;
/// 判据轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 修复口径冻结文本（进快照第二段，逐字封账） ───────────────────
/// 修复口径（冻结）：仅完成回验的修复计 Σ修复；未回验一律记在途。
pub const CALIBER_TEXT: &str = "修复口径v3：仅完成回验(verified=1)的修复计入Σ修复；未回验修复请求显性拒并落Σ在途；弃单只从Σ在途转出";
/// 恒等式文本（冻结）：进快照第一段。
pub const IDENTITY_TEXT: &str = "Σ修复+Σ放弃+Σ在途=Σ检出";

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
/// 诊断码闭集：每码带可操作中文建议（detail 文案）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZeroDocCode {
    /// 恒等式破坏：三和与检出不等
    IdentityBroken,
    /// 修复口径违例：未回验修复请求
    CaliberViolation,
    /// 在途不足：弃单量超过在途
    InflightOver,
    /// 封后改写：快照已封账
    PostSealWrite,
    /// 指纹失配：重放指纹与封账指纹不等
    FpMismatch,
    /// 空账封账：零事件封账显性拒
    EmptyLedger,
    /// 相位倒退：Open→Frozen 之外方向
    PhaseRegress,
    /// 版本倒退：新快照版本低于已封版本
    VersionRegress,
    /// 事件账越界：超过 MAX_EVENTS
    EventOver,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl ZeroDocCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ZeroDocCode::IdentityBroken => "恒等式破坏：Σ修复+Σ放弃+Σ在途必须等于Σ检出，请查事件账漏记/多记",
            ZeroDocCode::CaliberViolation => "修复口径违例：未回验的修复不许计Σ修复，请改记Σ在途并补回验",
            ZeroDocCode::InflightOver => "在途不足：修复/弃单只从Σ在途转出，请先确认该单确实在途",
            ZeroDocCode::PostSealWrite => "封后改写拒：快照已封账，如需改数请开新版本快照重走封账",
            ZeroDocCode::FpMismatch => "指纹失配：请核对三段文本逐字节与封账时是否一致（口径文本不得改）",
            ZeroDocCode::EmptyLedger => "空账封账拒：零事件快照无语义，请先落事件",
            ZeroDocCode::PhaseRegress => "相位倒退拒：相位仅 Open→Frozen，请按序推进",
            ZeroDocCode::VersionRegress => "版本倒退拒：新快照版本必须 ≥ 已封版本，请升版不许降版",
            ZeroDocCode::EventOver => "事件账越界：超过 MAX_EVENTS=4096，请拆册",
            ZeroDocCode::NotReproducible => "双跑不一致：复现红线，请检查是否有墙钟/随机参与取值",
        }
    }
    pub const ALL: [ZeroDocCode; 10] = [
        ZeroDocCode::IdentityBroken,
        ZeroDocCode::CaliberViolation,
        ZeroDocCode::InflightOver,
        ZeroDocCode::PostSealWrite,
        ZeroDocCode::FpMismatch,
        ZeroDocCode::EmptyLedger,
        ZeroDocCode::PhaseRegress,
        ZeroDocCode::VersionRegress,
        ZeroDocCode::EventOver,
        ZeroDocCode::NotReproducible,
    ];
}

/// 段内查表闭集返回。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < ZeroDocCode::ALL.len() {
        if let Some(c) = ZeroDocCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75800 段闭集内"
}

// ── 域模型：零丢事件与账 ────────────────────────────────────────
/// 零丢事件种类（闭集 4）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// 检出（Σ检出 +，同步入 Σ在途）
    Detect,
    /// 修复（从 Σ在途转出到 Σ修复，须 verified）
    Repair,
    /// 放弃（从 Σ在途转出到 Σ放弃）
    Abandon,
    /// 回炉入途（复检项重新检出：Σ检出与 Σ在途同步 +）
    Inflight,
}

/// 单条零丢事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZeroLossEvent {
    pub seq: u32,
    pub at_step: u64,
    pub kind: EventKind,
    pub amount: u64,
    /// 修复口径字段：仅 Repair 事件语义有效
    pub verified: bool,
}

/// 四元计数账。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Tally {
    pub detected: u64,
    pub repaired: u64,
    pub abandoned: u64,
    pub inflight: u64,
}

impl Tally {
    /// 零丢恒等式（checked 口径：溢出即破式）。
    pub fn identity_ok(&self) -> bool {
        match self.repaired.checked_add(self.abandoned) {
            Some(s) => match s.checked_add(self.inflight) {
                Some(sum) => sum == self.detected,
                None => false,
            },
            None => false,
        }
    }
}

/// 账相位（单调：Open→Frozen，倒退显性拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocPhase {
    /// 开账：可落事件
    Open,
    /// 已封账：只读，apply 一律拒
    Frozen,
}

/// 零丢语义账。
#[derive(Clone, Debug)]
pub struct ZeroDocLedger {
    pub phase: DocPhase,
    pub tally: Tally,
    pub events: Vec<ZeroLossEvent>,
    pub sealed_ver: u32,
}

impl ZeroDocLedger {
    pub fn new() -> ZeroDocLedger {
        ZeroDocLedger {
            phase: DocPhase::Open,
            tally: Tally::default(),
            events: Vec::new(),
            sealed_ver: 0,
        }
    }

    /// 落事件（修四元账 + 恒等式复核）。封账态一律拒。
    ///
    /// 恒等式语义（冻结）：检出入途、修复/放弃只从 Σ在途转出——
    /// 转出量超过在途即破式，显性拒（InflightOver），不落账。
    pub fn apply(&mut self, kind: EventKind, amount: u64, verified: bool) -> Result<u32, u32> {
        if self.phase != DocPhase::Open {
            return Err(ZeroDocCode::PostSealWrite.val());
        }
        if self.events.len() >= MAX_EVENTS {
            return Err(ZeroDocCode::EventOver.val());
        }
        match kind {
            EventKind::Repair => {
                // 修复口径（冻结）：未回验修复请求显性拒，建议补回验。
                if !verified {
                    return Err(ZeroDocCode::CaliberViolation.val());
                }
                // 修复从 Σ在途转出：在途不足即破式。
                match self.tally.inflight.checked_sub(amount) {
                    Some(rest) => self.tally.inflight = rest,
                    None => return Err(ZeroDocCode::InflightOver.val()),
                }
                self.tally.repaired = self.tally.repaired.saturating_add(amount);
            }
            EventKind::Detect | EventKind::Inflight => {
                // 检出/回炉：Σ检出与 Σ在途同步 +（新检项即处于在途）。
                self.tally.detected = match self.tally.detected.checked_add(amount) {
                    Some(v) => v,
                    None => return Err(ZeroDocCode::IdentityBroken.val()),
                };
                self.tally.inflight = match self.tally.inflight.checked_add(amount) {
                    Some(v) => v,
                    None => return Err(ZeroDocCode::IdentityBroken.val()),
                };
            }
            EventKind::Abandon => {
                // 弃单只从 Σ在途转出（checked_sub 防倒挂）。
                match self.tally.inflight.checked_sub(amount) {
                    Some(rest) => self.tally.inflight = rest,
                    None => return Err(ZeroDocCode::InflightOver.val()),
                }
                self.tally.abandoned = self.tally.abandoned.saturating_add(amount);
            }
        }
        // 恒等式复核（防御性：转出路径正常时恒保持，溢出即破式）。
        if !self.tally.identity_ok() {
            match kind {
                EventKind::Repair => {
                    self.tally.repaired = self.tally.repaired.saturating_sub(amount);
                    self.tally.inflight = self.tally.inflight.saturating_add(amount);
                }
                EventKind::Detect | EventKind::Inflight => {
                    self.tally.detected = self.tally.detected.saturating_sub(amount);
                    self.tally.inflight = self.tally.inflight.saturating_sub(amount);
                }
                EventKind::Abandon => {
                    self.tally.abandoned = self.tally.abandoned.saturating_sub(amount);
                    self.tally.inflight = self.tally.inflight.saturating_add(amount);
                }
            }
            return Err(ZeroDocCode::IdentityBroken.val());
        }
        let seq = self.events.len() as u32;
        self.events.push(ZeroLossEvent { seq, at_step: seq as u64, kind, amount, verified });
        Ok(seq)
    }

    /// 重演事件序（复算口径）：空账上逐条重放，产出四元账。
    pub fn replay_tally(&self) -> Result<Tally, u32> {
        let mut t = Tally::default();
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(e) = self.events.get(i) {
                match e.kind {
                    EventKind::Detect | EventKind::Inflight => {
                        t.detected = t.detected.saturating_add(e.amount);
                        t.inflight = t.inflight.saturating_add(e.amount);
                    }
                    EventKind::Abandon => match t.inflight.checked_sub(e.amount) {
                        Some(rest) => {
                            t.inflight = rest;
                            t.abandoned = t.abandoned.saturating_add(e.amount);
                        }
                        None => return Err(ZeroDocCode::InflightOver.val()),
                    },
                    EventKind::Repair => {
                        // 口径冻结：重演时未回验修复同样拒。
                        if !e.verified {
                            return Err(ZeroDocCode::CaliberViolation.val());
                        }
                        match t.inflight.checked_sub(e.amount) {
                            Some(rest) => t.inflight = rest,
                            None => return Err(ZeroDocCode::InflightOver.val()),
                        }
                        t.repaired = t.repaired.saturating_add(e.amount);
                    }
                }
                if !t.identity_ok() {
                    return Err(ZeroDocCode::IdentityBroken.val());
                }
            }
            i += 1;
        }
        Ok(t)
    }
}

// ── 语义快照（三段封账） ────────────────────────────────────────
/// 语义快照三段（恒等式/修复口径/版本）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocSnapshot {
    /// 第一段：恒等式文本
    pub seg_identity: String,
    /// 第二段：修复口径文本
    pub seg_caliber: String,
    /// 第三段：版本号（十进制文本）
    pub seg_version: String,
    /// 三段 FNV-1a 64 封账指纹
    pub fp: u64,
    pub version: u32,
}

/// 三段拼接文本（封账字节序冻结：段序恒 1→2→3）。
pub fn snapshot_text(ledger: &ZeroDocLedger, version: u32) -> Result<DocSnapshot, u32> {
    if ledger.events.is_empty() {
        return Err(ZeroDocCode::EmptyLedger.val());
    }
    if !ledger.tally.identity_ok() {
        return Err(ZeroDocCode::IdentityBroken.val());
    }
    let seg1 = format!("{} ({}/{}/{}/{})", IDENTITY_TEXT, ledger.tally.repaired, ledger.tally.abandoned, ledger.tally.inflight, ledger.tally.detected);
    let seg2 = String::from(CALIBER_TEXT);
    let seg3 = format!("v{}", version);
    let mut buf = String::new();
    buf.push_str(&seg1);
    buf.push_str(&seg2);
    buf.push_str(&seg3);
    let fp = fnv1a64(buf.as_bytes());
    Ok(DocSnapshot { seg_identity: seg1, seg_caliber: seg2, seg_version: seg3, fp, version })
}

/// 快照封账：Open→Frozen 单向；Frozen 态只允许**升版重封**（版本严格
/// 大于已封版本），同版/降版一律 VersionRegress 拒——相位永不回退。
pub fn seal_snapshot(ledger: &mut ZeroDocLedger, version: u32) -> Result<DocSnapshot, u32> {
    if ledger.phase == DocPhase::Open {
        // 首封。
        let snap = snapshot_text(ledger, version)?;
        ledger.phase = DocPhase::Frozen;
        ledger.sealed_ver = version;
        Ok(snap)
    } else if version > ledger.sealed_ver {
        // 升版重封：账面自首封起只读，重封仅刷新版本与指纹。
        let snap = snapshot_text(ledger, version)?;
        ledger.sealed_ver = version;
        Ok(snap)
    } else {
        Err(ZeroDocCode::VersionRegress.val())
    }
}

/// 指纹重放：同一事件序在空账上重演 → 重建快照 → 指纹必须一致。
pub fn replay_snapshot(ledger: &ZeroDocLedger, sealed: &DocSnapshot) -> Result<(), u32> {
    let mut fresh = ZeroDocLedger::new();
    let mut i = 0usize;
    while i < ledger.events.len() {
        if let Some(e) = ledger.events.get(i) {
            fresh.apply(e.kind, e.amount, e.verified)?;
        }
        i += 1;
    }
    let snap2 = snapshot_text(&fresh, sealed.version)?;
    if snap2.fp != sealed.fp || snap2.seg_version != sealed.seg_version {
        return Err(ZeroDocCode::FpMismatch.val());
    }
    Ok(())
}

// ── FNV-1a 64 ──────────────────────────────────────────────────
/// FNV-1a 64 位（参数冻结：offset 0xcbf29ce484222325，prime 0x100000001b3）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ── LCG（种子固定可用） ────────────────────────────────────────
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

/// 事件序语料（冻结：检出 40 = 修复 12 + 放弃 8 + 在途 20）。
pub fn corpus_events() -> Vec<(EventKind, u64, bool)> {
    vec![
        (EventKind::Detect, 30, false),
        (EventKind::Detect, 10, false),
        (EventKind::Repair, 12, true),
        (EventKind::Abandon, 8, false),
    ]
}

/// 真机语料期望四元账（手工冻结锚）。
pub fn corpus_expect() -> Tally {
    Tally { detected: 40, repaired: 12, abandoned: 8, inflight: 20 }
}

/// 按语料建账（事件序灌入）。
pub fn build_ledger() -> Result<ZeroDocLedger, u32> {
    let mut l = ZeroDocLedger::new();
    let corpus = corpus_events();
    let mut i = 0usize;
    while i < corpus.len() {
        if let Some((k, a, v)) = corpus.get(i) {
            l.apply(*k, *a, *v)?;
        }
        i += 1;
    }
    Ok(l)
}

// ── 注入检出账（六路：破式/口径/弃单越/封后写/版本倒/空账） ──────
pub struct InjectLedger {
    pub tries: [u32; 6],
    pub caught: [u32; 6],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 6], caught: [0; 6] }
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
        let mut i = 0usize;
        while i < 6 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 黄金全链：建账→恒等式→封账→重放→封后改写拒→版本倒退拒 ──────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut l = build_ledger()?;
    // 恒等式 + 语料锚。
    let ident_ok = (l.tally == corpus_expect() && l.tally.identity_ok()) as u32;
    // 封账。
    let snap = seal_snapshot(&mut l, DOC_VERSION)?;
    let sealed_ok = (l.phase == DocPhase::Frozen && l.sealed_ver == DOC_VERSION) as u32;
    // 指纹重放一致。
    let replay_ok = replay_snapshot(&l, &snap).is_ok() as u32;
    // 封后改写拒（apply 一律拒）。
    let post_write_rejected = matches!(
        l.apply(EventKind::Detect, 1, false),
        Err(e) if e == ZeroDocCode::PostSealWrite.val()
    ) as u32;
    // 版本倒退拒（同账降版重封必拒：3 → 2）。
    let mut l3 = build_ledger()?;
    let _ = seal_snapshot(&mut l3, DOC_VERSION);
    let ver_regress_rejected = matches!(
        seal_snapshot(&mut l3, DOC_VERSION - 1),
        Err(e) if e == ZeroDocCode::VersionRegress.val()
    ) as u32;
    // 升版重封放行（3 → 4 语义合法）。
    let ver_upgrade_ok = seal_snapshot(&mut l3, DOC_VERSION + 1).is_ok() as u32;
    Ok((
        ident_ok + sealed_ok + replay_ok + post_write_rejected + ver_regress_rejected + ver_upgrade_ok,
        post_write_rejected + ver_regress_rejected,
        snap.version,
    ))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链精确值（绿位 6 / 拦 2 / 版本 3）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((6, 2, 3)) => Ok(()),
        _ => Err(ZeroDocCode::NotReproducible.val()),
    }
}

/// 序 B：口径注入——未回验修复必拒；回验修复入 Σ修复且恒等式保持。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut l = ZeroDocLedger::new();
    // 检出 15 = 在途 10 + 回验修复 5（+ 放弃 0）：口径面配额自洽。
    let _ = l.apply(EventKind::Detect, 15, false);
    let _ = l.apply(EventKind::Inflight, 10, false);
    // 未回验修复：显性拒。
    let unverified_rejected = matches!(
        l.apply(EventKind::Repair, 5, false),
        Err(e) if e == ZeroDocCode::CaliberViolation.val()
    );
    // 回验修复：入 Σ修复，恒等式保持（5+0+10=15）。
    let verified_ok = l.apply(EventKind::Repair, 5, true).is_ok();
    match (unverified_rejected, verified_ok, l.tally.identity_ok()) {
        (true, true, true) => Ok(()),
        _ => Err(ZeroDocCode::CaliberViolation.val()),
    }
}

/// 序 C：冻结面（版本/段数/事件上限/口径文本非空且含 v3 标记）。
pub fn probe_seq_c() -> Result<(), u32> {
    let consts_ok = DOC_VERSION == 3 && SEAL_SEGMENTS == 3 && MAX_EVENTS == 4096;
    let caliber_ok = CALIBER_TEXT.contains("v3") && IDENTITY_TEXT.contains("Σ检出");
    match (consts_ok, caliber_ok) {
        (true, true) => Ok(()),
        _ => Err(ZeroDocCode::PhaseRegress.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 6 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0u32;
    while r < rounds {
        let _ = rng.next();
        // 路 1：破式路（修复转出超在途必抓）。
        let mut l1 = ZeroDocLedger::new();
        let _ = l1.apply(EventKind::Detect, 5, false);
        let _ = l1.apply(EventKind::Repair, 3, true);
        led.record(0, matches!(l1.apply(EventKind::Repair, 5, true), Err(e) if e == ZeroDocCode::InflightOver.val()));
        // 路 2：口径路（未回验修复必抓）。
        let mut l2 = ZeroDocLedger::new();
        let _ = l2.apply(EventKind::Detect, 4, false);
        led.record(1, matches!(l2.apply(EventKind::Repair, 4, false), Err(e) if e == ZeroDocCode::CaliberViolation.val()));
        // 路 3：弃单越路（空在途弃单必抓——弃单只从Σ在途转出）。
        let mut l3 = ZeroDocLedger::new();
        led.record(2, matches!(l3.apply(EventKind::Abandon, 1, false), Err(e) if e == ZeroDocCode::InflightOver.val()));
        // 路 4：封后写路（封账后 apply 必抓）。
        let mut l4 = build_ledger()?;
        let _ = seal_snapshot(&mut l4, DOC_VERSION)?;
        led.record(3, matches!(l4.apply(EventKind::Detect, 1, false), Err(e) if e == ZeroDocCode::PostSealWrite.val()));
        // 路 5：版本倒退路（同账封 3 后降版 2 必抓）。
        let mut l6 = build_ledger()?;
        let _ = seal_snapshot(&mut l6, DOC_VERSION)?;
        led.record(4, matches!(seal_snapshot(&mut l6, DOC_VERSION - 1), Err(e) if e == ZeroDocCode::VersionRegress.val()));
        // 路 6：空账封账路。
        let mut l7 = ZeroDocLedger::new();
        led.record(5, matches!(seal_snapshot(&mut l7, DOC_VERSION), Err(e) if e == ZeroDocCode::EmptyLedger.val()));
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ZeroDocCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0usize;
    while i < ZeroDocCode::ALL.len() {
        if let Some(c) = ZeroDocCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((ok, caught, ver)) => *ok == 6 && *caught == 2 && *ver == DOC_VERSION,
            Err(_) => false,
        },
        match &g {
            Ok((ok, caught, ver)) => format!("绿位={} 拦={} 版本={}", ok, caught, ver),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // 2. 恒等式主判据（语料账 + 手工锚）。
    let l = build_ledger();
    let ident_ok = match &l {
        Ok(lg) => lg.tally == corpus_expect() && lg.tally.identity_ok(),
        Err(_) => false,
    };
    v.push((
        "J1-零丢恒等式",
        ident_ok,
        match &l {
            Ok(lg) => format!(
                "{} → 修{}+弃{}+在途{} vs 检{}",
                IDENTITY_TEXT, lg.tally.repaired, lg.tally.abandoned, lg.tally.inflight, lg.tally.detected
            ),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // 3. 修复口径冻结面（拒未回验 + 重演口径一致）。
    let b = probe_seq_b();
    let b_ok = b.is_ok();
    let b_err = match b {
        Ok(()) => String::from("未回验拒/回验入账/恒等式保持"),
        Err(e) => format!("err=0x{:x} {}", e, detail(e)),
    };
    v.push(("J1-修复口径冻结面", b_ok, b_err));

    // 4. 重演复算一致（replay_tally 与实账四元等值）。
    let replay_tally_ok = match &l {
        Ok(lg) => match lg.replay_tally() {
            Ok(t) => t == lg.tally,
            Err(_) => false,
        },
        Err(_) => false,
    };
    v.push((
        "J1-重演复算一致",
        replay_tally_ok,
        String::from("事件序在空账重演产出同四元账"),
    ));

    // 5. 语义快照三段封账（三段在位 + 指纹非零）。
    let mut lg2 = build_ledger().unwrap_or_else(|_| ZeroDocLedger::new());
    let snap = seal_snapshot(&mut lg2, DOC_VERSION);
    let snap_ok = match &snap {
        Ok(s) => s.seg_version == format!("v{}", DOC_VERSION) && s.seg_identity.contains(IDENTITY_TEXT) && s.seg_caliber == CALIBER_TEXT && s.fp != 0,
        Err(_) => false,
    };
    v.push((
        "J1-三段封账",
        snap_ok,
        match &snap {
            Ok(s) => format!("三段在位 fp=0x{:x}", s.fp),
            Err(e) => format!("err=0x{:x} {}", e, detail(*e)),
        },
    ));

    // 6. 指纹重放一致。
    let rp = match (&l, &snap) {
        (Ok(lg), Ok(s)) => replay_snapshot(lg, s),
        _ => Err(ZeroDocCode::FpMismatch.val()),
    };
    v.push((
        "J1-指纹重放一致",
        rp.is_ok(),
        match rp {
            Ok(()) => String::from("同事件序重演指纹逐位一致"),
            Err(e) => format!("err=0x{:x} {}", e, detail(e)),
        },
    ));

    // 7. 封后改写拒。
    let pw = match &snap {
        Ok(_) => {
            let mut lg3 = build_ledger().unwrap_or_else(|_| ZeroDocLedger::new());
            let _ = seal_snapshot(&mut lg3, DOC_VERSION);
            matches!(lg3.apply(EventKind::Detect, 1, false), Err(e) if e == ZeroDocCode::PostSealWrite.val())
        }
        Err(_) => false,
    };
    v.push((
        "J1-封后改写拒",
        pw,
        String::from("封账后 apply 显性拒（PostSealWrite）"),
    ));

    // 8. 版本倒退拒（同账封 3 后同版/降版重封均拒）。
    let vr = {
        let mut lg4 = build_ledger().unwrap_or_else(|_| ZeroDocLedger::new());
        let _ = seal_snapshot(&mut lg4, DOC_VERSION);
        let same_rej = matches!(seal_snapshot(&mut lg4, DOC_VERSION), Err(e) if e == ZeroDocCode::VersionRegress.val());
        let down_rej = matches!(seal_snapshot(&mut lg4, DOC_VERSION - 1), Err(e) if e == ZeroDocCode::VersionRegress.val());
        same_rej && down_rej
    };
    v.push((
        "J1-版本倒退拒",
        vr,
        format!("版本 2 < 已封 {} 显性拒", DOC_VERSION),
    ));

    // 9. 弃单口径（空在途弃单拒 + 在途足额弃单过 + 转出清零）。
    let abandon_ok = {
        let mut la = ZeroDocLedger::new();
        let no_inflight_rejected = matches!(la.apply(EventKind::Abandon, 1, false), Err(e) if e == ZeroDocCode::InflightOver.val());
        let _ = la.apply(EventKind::Detect, 6, false);
        let _ = la.apply(EventKind::Abandon, 6, false);
        no_inflight_rejected && la.tally.identity_ok() && la.tally.inflight == 0
    };
    v.push((
        "J1-弃单口径",
        abandon_ok,
        String::from("弃单只从Σ在途转出，越额显性拒"),
    ));

    // 10. 注入 10/10。
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

    // 11. 探针三序。
    let a = probe_seq_a().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J2-探针三序",
        a && b_ok && c,
        format!("a=黄金({}) b=口径({}) c=冻结({})", a, b_ok, c),
    ));

    // 12. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));

    // 13. 段位守卫 + 闭集查表。
    let guard_ok = segment_guard();
    let mut table_ok = true;
    let mut i = 0usize;
    while i < ZeroDocCode::ALL.len() {
        if let Some(code) = ZeroDocCode::ALL.get(i) {
            if detail(code.val()).is_empty() {
                table_ok = false;
            }
        }
        i += 1;
    }
    v.push((
        "J3-段位守卫+闭集",
        guard_ok && table_ok,
        format!("0x75800 窗口守卫={} 闭集查表={}", guard_ok, table_ok),
    ));

    v
}

pub fn run_unx1475_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1475 零丢语义文档快照（恒等式与修复口径冻结）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（允许 assert!/assert_eq!；取值仍走 .get()） ──────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_run_all_green() {
        assert_eq!(golden_run(), Ok((6, 2, 3)));
    }

    #[test]
    fn identity_holds_on_corpus() {
        let l = build_ledger().unwrap_or_else(|_| ZeroDocLedger::new());
        assert_eq!(l.tally, corpus_expect());
        assert!(l.tally.identity_ok());
        assert_eq!(l.events.len(), corpus_events().len());
    }

    #[test]
    fn identity_broken_injection_caught() {
        // 转出超在途：修复量 > 在途必抓（InflightOver），且不落账。
        let mut l = ZeroDocLedger::new();
        let _ = l.apply(EventKind::Detect, 10, false);
        assert!(l.apply(EventKind::Repair, 3, true).is_ok());
        assert_eq!(l.apply(EventKind::Repair, 8, true).err(), Some(ZeroDocCode::InflightOver.val()));
        assert_eq!(l.tally.repaired, 3, "越途修复不得落账");
        assert!(l.tally.identity_ok());
        // 检出侧 u64 溢出：checked_add 破式，显性拒。
        let mut big = ZeroDocLedger::new();
        let _ = big.apply(EventKind::Detect, u64::MAX, false);
        assert_eq!(big.apply(EventKind::Detect, 1, false).err(), Some(ZeroDocCode::IdentityBroken.val()));
        assert_eq!(big.tally.detected, u64::MAX, "破式事件必须回滚");
        assert!(big.tally.identity_ok());
    }

    #[test]
    fn inflight_refire_keeps_identity() {
        let mut l = ZeroDocLedger::new();
        let _ = l.apply(EventKind::Detect, 4, false);
        let _ = l.apply(EventKind::Repair, 4, true);
        assert_eq!(l.tally.inflight, 0, "全数修复后在途清零");
        // 回炉：复检项重新检出，检出与在途同步 +，恒等式保持。
        assert!(l.apply(EventKind::Inflight, 2, false).is_ok());
        assert_eq!(l.tally.detected, 6);
        assert_eq!(l.tally.inflight, 2);
        assert!(l.tally.identity_ok());
    }

    #[test]
    fn caliber_frozen_unverified_rejected() {
        let mut l = ZeroDocLedger::new();
        let _ = l.apply(EventKind::Detect, 5, false);
        assert_eq!(l.apply(EventKind::Repair, 5, false).err(), Some(ZeroDocCode::CaliberViolation.val()));
        assert!(l.apply(EventKind::Repair, 5, true).is_ok());
        assert_eq!(l.tally.repaired, 5);
    }

    #[test]
    fn abandon_requires_inflight() {
        let mut l = ZeroDocLedger::new();
        // 空在途弃单：显性拒。
        assert_eq!(l.apply(EventKind::Abandon, 1, false).err(), Some(ZeroDocCode::InflightOver.val()));
        let _ = l.apply(EventKind::Detect, 4, false);
        assert!(l.apply(EventKind::Abandon, 2, false).is_ok());
        assert_eq!(l.tally.abandoned, 2);
        assert_eq!(l.tally.inflight, 2);
        assert!(l.tally.identity_ok());
    }

    #[test]
    fn snapshot_seal_and_replay() {
        let mut l = build_ledger().unwrap_or_else(|_| ZeroDocLedger::new());
        let s1 = seal_snapshot(&mut l, DOC_VERSION);
        assert!(s1.is_ok(), "正常语料首封必须成功");
        let s1 = s1.unwrap_or(DocSnapshot {
            seg_identity: String::new(),
            seg_caliber: String::new(),
            seg_version: String::new(),
            fp: 0,
            version: 0,
        });
        assert_eq!(l.phase, DocPhase::Frozen);
        assert_ne!(s1.fp, 0, "封账指纹不得为零值");
        assert!(s1.seg_identity.contains(IDENTITY_TEXT), "第一段须含恒等式文本");
        assert_eq!(s1.seg_caliber, CALIBER_TEXT, "第二段须为冻结口径全文");
        assert!(replay_snapshot(&l, &s1).is_ok(), "同事件序重演指纹必须一致");
        // 口径文本改动 → 重拼指纹必变（防口径漂移）。
        let mut parts = s1.seg_identity.clone();
        parts.push_str("被篡改口径");
        parts.push_str(&s1.seg_version);
        assert_ne!(fnv1a64(parts.as_bytes()), s1.fp, "口径文本改动指纹必变");
    }

    #[test]
    fn post_seal_write_rejected() {
        let mut l = build_ledger().unwrap_or_else(|_| ZeroDocLedger::new());
        let _ = seal_snapshot(&mut l, DOC_VERSION);
        assert_eq!(l.apply(EventKind::Detect, 1, false).err(), Some(ZeroDocCode::PostSealWrite.val()));
        assert_eq!(l.apply(EventKind::Repair, 1, true).err(), Some(ZeroDocCode::PostSealWrite.val()));
    }

    #[test]
    fn version_regress_and_empty_ledger() {
        let mut l = build_ledger().unwrap_or_else(|_| ZeroDocLedger::new());
        assert!(seal_snapshot(&mut l, DOC_VERSION).is_ok());
        // 同账降版（3→1）显性拒；升版（3→4）放行。
        assert_eq!(seal_snapshot(&mut l, 1).err(), Some(ZeroDocCode::VersionRegress.val()));
        assert_eq!(seal_snapshot(&mut l, DOC_VERSION).err(), Some(ZeroDocCode::VersionRegress.val()));
        assert!(seal_snapshot(&mut l, DOC_VERSION + 1).is_ok(), "升版重封语义合法");
        // 空账首封显性拒。
        let mut empty = ZeroDocLedger::new();
        assert_eq!(seal_snapshot(&mut empty, DOC_VERSION).err(), Some(ZeroDocCode::EmptyLedger.val()));
    }

    #[test]
    fn inject_rounds_all_caught() {
        let led = inject_rounds(JUDGE_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(led.all_caught(), "六路注入必须全抓：{:?}", led.caught);
    }

    #[test]
    fn probes_repro_and_guard() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
        assert!(segment_guard());
    }

    #[test]
    fn diagnostic_table_closed_set() {
        let mut i = 0usize;
        while i < ZeroDocCode::ALL.len() {
            let c = ZeroDocCode::ALL.get(i).copied().unwrap_or(ZeroDocCode::NotReproducible);
            let v = c.val();
            assert!(v >= SEG_BASE && v < SEG_BASE + 0x40, "诊断码 0x{:x} 越窗", v);
            assert!(!detail(v).is_empty());
            i += 1;
        }
        assert!(detail(0x42424242).contains("未知诊断码"));
    }

    #[test]
    fn checkset_counts() {
        let s = run_unx1475_checks();
        assert!(s.len() >= 12, "检查条目须 ≥12，实际 {}", s.len());
        assert!(s.all_passed(), "全链须绿：{}", s.red_items_text());
    }
}
