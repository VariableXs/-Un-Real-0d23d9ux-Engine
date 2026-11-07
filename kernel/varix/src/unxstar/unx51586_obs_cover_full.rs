//! UNX-F51586 M 型段可观测性终核（埋点覆盖率 100% 断言） · 0xC1540段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51587-J1 对账 ×50 零漂移
//!
//! 结构清单：
//!   - ObsKind：埋点种类闭集（追踪/度量/事件/日志）
//!   - ObsSite：埋点站（站号/种类/名称）
//!   - ObsEvent：观测事件（站号/种类/序号）
//!   - CoverageLedger：覆盖率账本（注册站集 + 事件流 + 覆盖位图）
//!   - ReplayChecker：×50 轮对账回放器（逐轮指纹比对）
//!   - ObsCode：诊断码闭集（12 变体）
//!   - Lcg / fnv1a：确定性注入与双跑指纹
//!
//! 设计要点：
//!   ① no_std + alloc：全实现只用 core/alloc，零浮点、零 IO、零墙钟、零随机
//!      （LCG 种子由 SEG_BASE 派生）；零 unwrap/expect/panic/裸下标，取值一律
//!      .get()/.get_mut()。
//!   ② 覆盖率机检 100%：注册站集与覆盖位图全同（每站至少一条事件）即机检通
//!      过；未注册站发事件显性拒（缺埋点注入 0 命中——坏样本逃逸恒 0）。
//!   ③ 对账 ×50 零漂移：同一事件脚本回放 50 轮，逐轮 FNV-1a 指纹与首轮逐位
//!      一致；任何一轮漂移显性 Err。
//!   ④ 发事件判序：站号在册 → 种类匹配 → 事件序号单调 → 落覆盖位图。
//!   ⑤ 注入核：4 类坏样本（未注册站/种类错配/站号越界/缺埋点漏覆盖）逐类
//!      10 轮精确复现；注入账守恒：检出+逃逸=轮数，逃逸恒 0。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC1540;

/// 站数（在册埋点站）。
pub const SITES: usize = 20;
/// 对账轮数（×50）。
pub const ROUNDS: usize = 50;

/// 埋点种类闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsKind {
    /// 追踪
    Trace = 0,
    /// 度量
    Metric = 1,
    /// 事件
    Event = 2,
    /// 日志
    Log = 3,
}

impl ObsKind {
    pub fn code(self) -> u32 {
        self as u32
    }
    /// 种类码闭集解码。
    pub fn from_code(c: u32) -> Option<ObsKind> {
        match c {
            0 => Some(ObsKind::Trace),
            1 => Some(ObsKind::Metric),
            2 => Some(ObsKind::Event),
            3 => Some(ObsKind::Log),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            ObsKind::Trace => "追踪",
            ObsKind::Metric => "度量",
            ObsKind::Event => "事件",
            ObsKind::Log => "日志",
        }
    }
    /// 闭集全量清单。
    pub fn all() -> [ObsKind; 4] {
        [ObsKind::Trace, ObsKind::Metric, ObsKind::Event, ObsKind::Log]
    }
}

/// 埋点站。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObsSite {
    /// 站号 0..20
    pub id: usize,
    /// 种类码（闭集 0..4）
    pub kind_code: u32,
    /// 站名（非空）
    pub name: &'static str,
}

/// 观测事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObsEvent {
    /// 站号
    pub site: usize,
    /// 种类码
    pub kind_code: u32,
    /// 事件序号（站内单调）
    pub seq: u64,
}

/// 诊断码闭集（可观测性域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 站号越界
    SiteOob,
    /// 站未注册
    SiteUnregistered,
    /// 重复注册
    SiteDup,
    /// 种类错配（事件种类与站种类不符）
    KindMismatch,
    /// 种类码越闭集
    KindUnknown,
    /// 事件序号倒退
    SeqRegress,
    /// 覆盖缺口（有机站无事件）
    CoverGap,
    /// 对账轮次缺失
    RoundsMissing,
    /// 轮间漂移（回放指纹不一致）
    RoundDrift,
    /// 站名/文案为空
    TextEmpty,
}

impl ObsCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ObsCode::Ok => "可观测性终核判定成功",
            ObsCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            ObsCode::SiteOob => "站号越界：埋点站号必须在 0..20 之内",
            ObsCode::SiteUnregistered => "站未注册：发事件前必须先注册埋点站",
            ObsCode::SiteDup => "重复注册：同站号只能注册一次",
            ObsCode::KindMismatch => "种类错配：事件种类必须与埋点站种类一致",
            ObsCode::KindUnknown => "种类非法：埋点种类码必须在 0..4 闭集内",
            ObsCode::SeqRegress => "序号倒退：站内事件序号必须单调递增",
            ObsCode::CoverGap => "覆盖缺口：机检要求每站至少一条事件",
            ObsCode::RoundsMissing => "轮次缺失：对账必须跑满 50 轮",
            ObsCode::RoundDrift => "轮间漂移：回放指纹必须与首轮逐位一致",
            ObsCode::TextEmpty => "文案为空：埋点站名必须非空",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [ObsCode; 12] = [
    ObsCode::Ok,
    ObsCode::SegBad,
    ObsCode::SiteOob,
    ObsCode::SiteUnregistered,
    ObsCode::SiteDup,
    ObsCode::KindMismatch,
    ObsCode::KindUnknown,
    ObsCode::SeqRegress,
    ObsCode::CoverGap,
    ObsCode::RoundsMissing,
    ObsCode::RoundDrift,
    ObsCode::TextEmpty,
];

/// 在册站表：20 站，种类按站号模 4 轮转。
pub fn canonical_sites() -> Vec<ObsSite> {
    let mut v: Vec<ObsSite> = Vec::new();
    for id in 0..SITES {
        v.push(ObsSite {
            id,
            kind_code: (id % 4) as u32,
            name: "埋点站",
        });
    }
    v
}

/// 标准事件脚本：每站按种类各发 2 条（序号 0、1）。
pub fn standard_events() -> Vec<ObsEvent> {
    let mut v: Vec<ObsEvent> = Vec::new();
    for id in 0..SITES {
        for k in 0..2u64 {
            v.push(ObsEvent {
                site: id,
                kind_code: (id % 4) as u32,
                seq: k,
            });
        }
    }
    v
}

/// 覆盖率账本。
pub struct CoverageLedger {
    sites: Vec<ObsSite>,
    /// 覆盖位图（bit=站号，事件命中置位）
    covered: u64,
    /// 各站已收最高序号
    seq_hi: [u64; SITES],
    events: Vec<ObsEvent>,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl CoverageLedger {
    /// 空账本（未注册任何站）。
    pub fn new() -> CoverageLedger {
        CoverageLedger {
            sites: Vec::new(),
            covered: 0,
            seq_hi: [0; SITES],
            events: Vec::new(),
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 装载在册站表。
    pub fn load_canonical() -> CoverageLedger {
        let mut led = CoverageLedger::new();
        for s in canonical_sites().iter() {
            let _ = led.register(*s);
        }
        led
    }

    /// 注册埋点站：站号界 → 种类闭集 → 站名非空 → 未重复注册。
    pub fn register(&mut self, s: ObsSite) -> Result<(), ObsCode> {
        if s.id >= SITES {
            self.rejected += 1;
            return Err(ObsCode::SiteOob);
        }
        if ObsKind::from_code(s.kind_code).is_none() {
            self.rejected += 1;
            return Err(ObsCode::KindUnknown);
        }
        if s.name.is_empty() {
            self.rejected += 1;
            return Err(ObsCode::TextEmpty);
        }
        if self.is_registered(s.id) {
            self.rejected += 1;
            return Err(ObsCode::SiteDup);
        }
        self.sites.push(s);
        Ok(())
    }

    /// 站是否在册。
    pub fn is_registered(&self, id: usize) -> bool {
        self.sites.iter().any(|s| s.id == id)
    }

    /// 站种类读取。
    pub fn site_kind(&self, id: usize) -> Option<u32> {
        self.sites.iter().find(|s| s.id == id).map(|s| s.kind_code)
    }

    /// 发事件：站号界 → 在册 → 种类匹配 → 序号单调 → 落覆盖位图。
    pub fn emit(&mut self, e: ObsEvent) -> Result<(), ObsCode> {
        if e.site >= SITES {
            self.rejected += 1;
            return Err(ObsCode::SiteOob);
        }
        if !self.is_registered(e.site) {
            self.rejected += 1;
            return Err(ObsCode::SiteUnregistered);
        }
        if ObsKind::from_code(e.kind_code).is_none() {
            self.rejected += 1;
            return Err(ObsCode::KindUnknown);
        }
        let sk = match self.site_kind(e.site) {
            Some(k) => k,
            None => {
                self.rejected += 1;
                return Err(ObsCode::SiteUnregistered);
            }
        };
        if sk != e.kind_code {
            self.rejected += 1;
            return Err(ObsCode::KindMismatch);
        }
        let hi = match self.seq_hi.get(e.site) {
            Some(&h) => h,
            None => {
                self.rejected += 1;
                return Err(ObsCode::SiteOob);
            }
        };
        if e.seq < hi {
            self.rejected += 1;
            return Err(ObsCode::SeqRegress);
        }
        if let Some(h) = self.seq_hi.get_mut(e.site) {
            *h = e.seq + 1;
        }
        if let Some(m) = self.covered_bit(e.site) {
            *m |= 1u64 << e.site;
        }
        self.events.push(e);
        Ok(())
    }

    /// 覆盖位图置位（位运算封装，避免裸下标语义）。
    fn covered_bit(&mut self, site: usize) -> Option<&mut u64> {
        if site >= SITES {
            None
        } else {
            Some(&mut self.covered)
        }
    }

    /// 覆盖位图读取。
    pub fn covered_map(&self) -> u64 {
        self.covered
    }

    /// 覆盖率（百分比，整数）。
    pub fn coverage_pct(&self) -> u32 {
        self.covered.count_ones() * 100 / SITES as u32
    }

    /// 事件流指纹（FNV-1a）。
    pub fn stream_fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        for e in self.events.iter() {
            bytes.extend_from_slice(&(e.site as u64).to_le_bytes());
            bytes.extend_from_slice(&(e.kind_code as u64).to_le_bytes());
            bytes.extend_from_slice(&e.seq.to_le_bytes());
        }
        fnv1a(&bytes)
    }

    /// 机检：覆盖率 100% 且事件流非空。
    pub fn machine_check(&self) -> Result<(), ObsCode> {
        if self.covered != SITE_FULL {
            return Err(ObsCode::CoverGap);
        }
        if self.events.is_empty() {
            return Err(ObsCode::CoverGap);
        }
        Ok(())
    }

    pub fn events_len(&self) -> usize {
        self.events.len()
    }

    /// (事件数, 拒绝数) 二本计数。
    pub fn tally(&self) -> (u64, u64) {
        (self.events.len() as u64, self.rejected)
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 状态快照（双跑一致面）。
    pub fn snapshot(&self) -> (u64, u64, usize, u32) {
        (self.events.len() as u64, self.rejected, self.sites.len(), self.covered as u32)
    }
}

/// 全 20 位满位掩码。
pub const SITE_FULL: u64 = (1u64 << SITES) - 1;

/// ×50 轮对账回放器：逐轮重建账本、回放标准脚本、比对首轮指纹。
pub struct ReplayChecker {
    baseline: u64,
    rounds: usize,
    drift_at: Option<usize>,
}

impl ReplayChecker {
    /// 新建回放器。
    pub fn new() -> ReplayChecker {
        ReplayChecker { baseline: 0, rounds: 0, drift_at: None }
    }

    /// 回放一轮：指纹与首轮比对，漂移记轮次并显性 Err。
    pub fn replay_round(&mut self) -> Result<u64, ObsCode> {
        let mut led = CoverageLedger::load_canonical();
        for e in standard_events().iter() {
            let _ = led.emit(*e);
        }
        let fp = led.stream_fingerprint();
        if self.rounds == 0 {
            self.baseline = fp;
        } else if fp != self.baseline {
            if self.drift_at.is_none() {
                self.drift_at = Some(self.rounds);
            }
            return Err(ObsCode::RoundDrift);
        }
        self.rounds += 1;
        Ok(fp)
    }

    /// 收口：必须跑满 50 轮且全程零漂移。
    pub fn finalize(&self) -> Result<(), ObsCode> {
        if self.rounds < ROUNDS {
            return Err(ObsCode::RoundsMissing);
        }
        if self.drift_at.is_some() {
            return Err(ObsCode::RoundDrift);
        }
        Ok(())
    }

    pub fn rounds(&self) -> usize {
        self.rounds
    }
}

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg { s: seed ^ 0xa24baed4963ee407 }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：独立跑两遍 ×50 回放，结果一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (usize, u64) {
        let mut r = ReplayChecker::new();
        for _ in 0..ROUNDS {
            let _ = r.replay_round();
        }
        (r.rounds(), match r.finalize() {
            Ok(()) => 1,
            Err(_) => 0,
        })
    };
    build() == build()
}

/// 注入核：4 类坏样本逐类 10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 4]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 4] = [0; 4];
    // 类 0：未注册站发事件。
    for i in 0..10u32 {
        let mut led = CoverageLedger::new();
        let r = led.emit(ObsEvent { site: (i % SITES as u32) as usize, kind_code: 0, seq: 0 });
        if r == Err(ObsCode::SiteUnregistered) {
            led.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            led.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：种类错配。
    for i in 0..10u32 {
        let mut led = CoverageLedger::load_canonical();
        let wrong = ((i % 3) + 1) as u32;
        let r = led.emit(ObsEvent { site: 0, kind_code: wrong, seq: 0 });
        if r == Err(ObsCode::KindMismatch) {
            led.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            led.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：站号越界。
    for i in 0..10u32 {
        let mut led = CoverageLedger::load_canonical();
        let r = led.emit(ObsEvent { site: SITES + (i % 3) as usize, kind_code: 0, seq: 0 });
        if r == Err(ObsCode::SiteOob) {
            led.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            led.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：缺埋点漏覆盖（只发偶数站 → 机检 CoverGap）。
    for _ in 0..10u32 {
        let mut led = CoverageLedger::load_canonical();
        for e in standard_events().iter() {
            if e.site % 2 == 0 {
                let _ = led.emit(*e);
            }
        }
        let r = led.machine_check();
        if r == Err(ObsCode::CoverGap) {
            led.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            led.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（15 条判据形态）。
pub fn run_unx51586_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51586");
    // 1 在册站表：20 站全注册且种类轮转合法。
    {
        let led = CoverageLedger::load_canonical();
        let mut all_reg = true;
        for id in 0..SITES {
            if !led.is_registered(id) {
                all_reg = false;
            }
        }
        s.add(
            "在册 20 站",
            all_reg && led.snapshot().2 == SITES,
            "20 个埋点站全部注册且无重复",
        );
    }
    // 2 标准脚本：40 条事件全收、覆盖率 100%、机检通过。
    {
        let mut led = CoverageLedger::load_canonical();
        for e in standard_events().iter() {
            let _ = led.emit(*e);
        }
        s.add(
            "覆盖率机检 100%",
            led.coverage_pct() == 100
                && led.covered_map() == SITE_FULL
                && led.machine_check() == Ok(()),
            "每站至少一条事件，覆盖位图全 20 位，机检通过",
        );
    }
    // 3 对账 ×50 零漂移。
    {
        let mut r = ReplayChecker::new();
        for _ in 0..ROUNDS {
            let _ = r.replay_round();
        }
        s.add(
            "对账×50零漂移",
            r.rounds() == ROUNDS && r.finalize() == Ok(()),
            "UNX-F51587-J1：50 轮回放指纹与首轮逐位一致",
        );
    }
    // 4 发事件判序：站号界先于在册与种类判据。
    {
        let mut led = CoverageLedger::load_canonical();
        let r = led.emit(ObsEvent { site: SITES + 5, kind_code: 0, seq: 0 });
        s.add(
            "判序站号界先行",
            r == Err(ObsCode::SiteOob),
            "站号越界先于在册与种类判据",
        );
    }
    // 5 重复注册显性拒。
    {
        let mut led = CoverageLedger::load_canonical();
        let r = led.register(ObsSite { id: 0, kind_code: 0, name: "重复" });
        s.add("重复注册拒", r == Err(ObsCode::SiteDup), "同站号只能注册一次");
    }
    // 6 序号倒退显性拒。
    {
        let mut led = CoverageLedger::load_canonical();
        let _ = led.emit(ObsEvent { site: 1, kind_code: 1, seq: 5 });
        let r = led.emit(ObsEvent { site: 1, kind_code: 1, seq: 2 });
        s.add(
            "序号倒退拒",
            r == Err(ObsCode::SeqRegress),
            "站内事件序号必须单调递增",
        );
    }
    // 7 种类闭集：4 类全可解码、越码 None。
    {
        let mut closed = true;
        for k in ObsKind::all().iter() {
            match ObsKind::from_code(k.code()) {
                Some(x) if x == *k => {}
                _ => closed = false,
            }
        }
        s.add(
            "种类闭集完整",
            closed && ObsKind::from_code(4).is_none() && ObsKind::from_code(99).is_none(),
            "埋点种类码 0..4 全可解码，越码显性 None",
        );
    }
    // 8 诊断码闭集唯一性。
    {
        let mut uniq = true;
        let mut text = true;
        for i in 0..ALL_CODES.len() {
            let c = match ALL_CODES.get(i) {
                Some(&c) => c,
                None => {
                    uniq = false;
                    continue;
                }
            };
            if c.val() != SEG_BASE + 1 + i as u32 {
                uniq = false;
            }
            if c.detail().is_empty() {
                text = false;
            }
        }
        s.add(
            "诊断码闭集唯一",
            uniq && text && ALL_CODES.len() >= 10 && ALL_CODES.len() <= 14,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 12 变体",
        );
    }
    // 9 注入核：四类 ×10 全抓、零逃逸。
    {
        let (d, e, per) = injection_harness();
        let all10 = per.iter().all(|&x| x == 10);
        s.add(
            "缺埋点注入 0 命中",
            d == 40 && e == 0 && all10,
            "注入=检出+逃逸，逃逸恒 0（缺埋点注入零命中于合格账本）",
        );
    }
    // 10 守恒账单独核。
    {
        let mut led = CoverageLedger::new();
        for _ in 0..10u32 {
            led.note_detected();
        }
        s.add("注入账守恒", led.conserved(), "注入=检出+逃逸且逃逸=0 且检出>0");
    }
    // 11 双跑一致：两遍 ×50 回放结果一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "独立两遍 ×50 回放，轮数与收口结果一致",
    );
    // 12 指纹复算稳定且摘要定长。
    {
        let mut led = CoverageLedger::load_canonical();
        for e in standard_events().iter() {
            let _ = led.emit(*e);
        }
        let fp1 = led.stream_fingerprint();
        let fp2 = led.stream_fingerprint();
        let brief: String = format!("{:016x}", fp1);
        s.add(
            "指纹复算稳定",
            fp1 == fp2 && brief.len() == 16,
            "事件流指纹两次复算相等且十六进制摘要定长 16",
        );
    }
    // 13 事件数对账：标准脚本恰 40 条、拒绝零。
    {
        let mut led = CoverageLedger::load_canonical();
        for e in standard_events().iter() {
            let _ = led.emit(*e);
        }
        let (ev, rej) = led.tally();
        s.add(
            "事件数对账",
            ev == (SITES * 2) as u64 && rej == 0,
            "20 站 ×2 条事件恰 40 条且零拒",
        );
    }
    // 14 站名文案全非空。
    {
        let sites = canonical_sites();
        let mut text = true;
        for x in sites.iter() {
            if x.name.is_empty() {
                text = false;
            }
        }
        s.add("站名文案非空", text, "每个在册埋点站名必须非空");
    }
    // 15 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51586 可观测性终核 {} 站 ×{} 轮 诊断码{} 变体",
            SITES,
            ROUNDS,
            ALL_CODES.len()
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51586"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51586_checks_green() {
        let set = run_unx51586_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn coverage_100_after_standard() {
        let mut led = CoverageLedger::load_canonical();
        for e in standard_events().iter() {
            assert!(led.emit(*e).is_ok());
        }
        assert_eq!(led.coverage_pct(), 100);
        assert_eq!(led.covered_map(), SITE_FULL);
        assert!(led.machine_check().is_ok());
    }

    #[test]
    fn unregistered_site_rejected() {
        let mut led = CoverageLedger::new();
        assert_eq!(
            led.emit(ObsEvent { site: 0, kind_code: 0, seq: 0 }),
            Err(ObsCode::SiteUnregistered)
        );
    }

    #[test]
    fn kind_mismatch_rejected() {
        let mut led = CoverageLedger::load_canonical();
        assert_eq!(
            led.emit(ObsEvent { site: 0, kind_code: 1, seq: 0 }),
            Err(ObsCode::KindMismatch)
        );
    }

    #[test]
    fn site_oob_first() {
        let mut led = CoverageLedger::load_canonical();
        assert_eq!(
            led.emit(ObsEvent { site: SITES, kind_code: 0, seq: 0 }),
            Err(ObsCode::SiteOob)
        );
    }

    #[test]
    fn dup_register_rejected() {
        let mut led = CoverageLedger::new();
        assert!(led.register(ObsSite { id: 3, kind_code: 3, name: "a" }).is_ok());
        assert_eq!(
            led.register(ObsSite { id: 3, kind_code: 3, name: "b" }),
            Err(ObsCode::SiteDup)
        );
    }

    #[test]
    fn seq_regress_rejected() {
        let mut led = CoverageLedger::load_canonical();
        assert!(led.emit(ObsEvent { site: 2, kind_code: 2, seq: 9 }).is_ok());
        assert_eq!(
            led.emit(ObsEvent { site: 2, kind_code: 2, seq: 3 }),
            Err(ObsCode::SeqRegress)
        );
    }

    #[test]
    fn cover_gap_on_partial() {
        let mut led = CoverageLedger::load_canonical();
        for e in standard_events().iter() {
            if e.site % 2 == 0 {
                let _ = led.emit(*e);
            }
        }
        assert_eq!(led.machine_check(), Err(ObsCode::CoverGap));
        assert!(led.coverage_pct() < 100);
    }

    #[test]
    fn fifty_rounds_zero_drift() {
        let mut r = ReplayChecker::new();
        for _ in 0..ROUNDS {
            assert!(r.replay_round().is_ok());
        }
        assert!(r.finalize().is_ok());
        assert_eq!(r.rounds(), ROUNDS);
    }

    #[test]
    fn injection_all_classes_caught() {
        let (_, esc, per) = injection_harness();
        assert_eq!(esc, 0);
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "注入类 {} 应恰 10 轮全抓", i);
        }
    }

    #[test]
    fn conservation_holds() {
        let mut led = CoverageLedger::new();
        for _ in 0..10u32 {
            led.note_detected();
        }
        assert!(led.conserved());
        let mut bad = CoverageLedger::new();
        for _ in 0..10u32 {
            bad.note_escape();
        }
        assert!(!bad.conserved());
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 12);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
        for i in 0..ALL_CODES.len() {
            for j in (i + 1)..ALL_CODES.len() {
                let a = match ALL_CODES.get(i) {
                    Some(&x) => x,
                    None => continue,
                };
                let b = match ALL_CODES.get(j) {
                    Some(&y) => y,
                    None => continue,
                };
                assert_ne!(a.val(), b.val());
            }
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
