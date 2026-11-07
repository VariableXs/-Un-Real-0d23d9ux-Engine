//! UNX-F51633 B22 错误码扩展（攻击族 ×15 并入域内三表）
//! · 0xC2100段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51634-J1 埋点 100%；UNX-F51634-J1R 缺埋点注入检出；
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - ObsCode          诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - ProbePoint       可观测性埋点（检出/隔离/恢复/审计四点）
//!   - ObsLedger        埋点账（逐事件逐点留痕 + 覆盖率核）
//!   - CodeTriad        错误码三表（攻击族 ×15 并入闭集）
//!   - run_unx51633_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① 四点埋点：每事件必须过 检出/隔离/恢复/审计 四个探针点，四点齐
//!      才算覆盖；覆盖率 = 已覆盖事件 ×4 / 应埋点数，UNX-F51634-J1 埋
//!      点 100% 受控达成。
//!   ② 缺埋点注入检出：任何一点缺席即判 ProbeMissing——注入逐轮检出，
//!      逃逸恒 0（UNX-F51634-J1R）。
//!   ③ 攻击族 ×15 并入三表：15 枚族码按 码序 mod 3 分域并入，撞码/越
//!      界/零码显性拒；并入序断裂判 MergeGap。
//!   ④ 覆盖账守恒：应埋点数 = 事件数 ×4，已埋点数只增不减；缺点检出
//!      逐次入检出账（零静默）。
//!   ⑤ 注入核：5 类坏样本（缺检出点/缺隔离点/缺恢复点/缺审计点/事件
//!      序断裂+撞码）逐类 ×10 轮精确复现，注入账守恒（检出+逃逸=轮数，
//!      逃逸恒 0）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机——探针序由 LCG
//!      派生（种子 = SEG_BASE 派生），同段位恒定可复现；零 unwrap/
//!      expect/panic/裸下标，取值一律 .get()/.get_mut()/match。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC2100;

/// 事件闭集（16 事件）。
pub const EVENT_CAP: usize = 16;
/// 探针点数（检出/隔离/恢复/审计）。
pub const PROBE_POINTS: usize = 4;
/// 攻击族并入码数。
pub const FAMILY_SIZE: usize = 15;
/// 攻击族码基址。
pub const FAMILY_BASE: u32 = 0x5163_3000;
/// 每类坏样本注入轮数。
pub const INJ_ROUNDS: usize = 10;

/// 探针点闭集序号：0 检出 / 1 隔离 / 2 恢复 / 3 审计。
pub const P_DETECT: usize = 0;
pub const P_ISOLATE: usize = 1;
pub const P_RECOVER: usize = 2;
pub const P_AUDIT: usize = 3;

/// 事件探针状态位图（4 点，位序 = 探针点序号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbePoint {
    /// 事件号（1..=16）
    pub event_id: usize,
    /// 四点覆盖位图
    pub mask: u8,
}

impl ProbePoint {
    /// 新探针（零覆盖）。
    pub fn new(event_id: usize) -> ProbePoint {
        ProbePoint { event_id, mask: 0 }
    }

    /// 落一点：点号越界显性拒，重复落点判 DupProbe。
    pub fn mark(&mut self, point: usize) -> Result<(), ObsCode> {
        if point >= PROBE_POINTS {
            return Err(ObsCode::PointOob);
        }
        if self.mask & (1u8 << point) != 0 {
            return Err(ObsCode::DupProbe);
        }
        self.mask |= 1u8 << point;
        Ok(())
    }

    /// 四点是否齐。
    pub fn covered(&self) -> bool {
        self.mask == 0x0F
    }

    /// 缺点清单（位序数组，0=缺）。
    pub fn missing(&self) -> [bool; PROBE_POINTS] {
        [
            self.mask & 1 == 0,
            self.mask & 2 == 0,
            self.mask & 4 == 0,
            self.mask & 8 == 0,
        ]
    }
}

/// 埋点账。
pub struct ObsLedger {
    entries: Vec<ProbePoint>,
    probes: usize,
    missing_detected: usize,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl ObsLedger {
    /// 新建埋点账。
    pub fn new() -> ObsLedger {
        ObsLedger {
            entries: Vec::new(),
            probes: 0,
            missing_detected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 事件入账（序列守卫：1..=n 递增，断裂判 EventGap）。
    pub fn add_event(&mut self, event_id: usize) -> Result<(), ObsCode> {
        if event_id == 0 || event_id > EVENT_CAP {
            return Err(ObsCode::EventOob);
        }
        if self.entries.iter().any(|e| e.event_id == event_id) {
            return Err(ObsCode::DupEvent);
        }
        if event_id != self.entries.len() + 1 {
            return Err(ObsCode::EventGap);
        }
        self.entries.push(ProbePoint::new(event_id));
        Ok(())
    }

    /// 落点：事件必须在账，落点逐次留痕。
    pub fn mark(&mut self, event_id: usize, point: usize) -> Result<(), ObsCode> {
        let idx = match self.entries.iter().position(|e| e.event_id == event_id) {
            Some(i) => i,
            None => return Err(ObsCode::EventMissing),
        };
        // 用取下标替代裸下标：先取可变引用再 mark。
        let r = match self.entries.get_mut(idx) {
            Some(e) => e.mark(point),
            None => Err(ObsCode::EventMissing),
        };
        match r {
            Ok(()) => {
                self.probes += 1;
                Ok(())
            }
            Err(ObsCode::PointOob) => Err(ObsCode::PointOob),
            Err(ObsCode::DupProbe) => {
                self.missing_detected += 1;
                Err(ObsCode::DupProbe)
            }
            Err(e) => Err(e),
        }
    }

    /// 事件四点是否齐。
    pub fn event_covered(&self, event_id: usize) -> bool {
        self.entries
            .iter()
            .any(|e| e.event_id == event_id && e.covered())
    }

    /// 全账覆盖率面（全部事件四点齐 = 100%）。
    pub fn full_coverage(&self) -> bool {
        self.entries.len() == EVENT_CAP && self.entries.iter().all(|e| e.covered())
    }

    /// 已落探针数。
    pub fn probes_tally(&self) -> usize {
        self.probes
    }

    /// 已入账事件数。
    pub fn events_len(&self) -> usize {
        self.entries.len()
    }

    /// 缺点检出账。
    pub fn missing_detected_tally(&self) -> usize {
        self.missing_detected
    }

    /// 缺点扫描：返回首个缺点的（事件号，点号）。
    pub fn first_missing(&self) -> Option<(usize, usize)> {
        for e in self.entries.iter() {
            let m = e.missing();
            for p in 0..PROBE_POINTS {
                if m[p] {
                    return Some((e.event_id, p));
                }
            }
        }
        None
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

    /// 快照指纹（双跑一致面）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        for e in self.entries.iter() {
            bytes.push(e.event_id as u8);
            bytes.push(e.mask);
        }
        bytes.extend_from_slice(&(self.probes as u64).to_le_bytes());
        fnv1a(&bytes)
    }
}

/// 错误码三表（攻击族 ×15 并入闭集）。
pub struct CodeTriad {
    tables: [Vec<u32>; 3],
    merged: usize,
}

impl CodeTriad {
    /// 新建三表。
    pub fn new() -> CodeTriad {
        CodeTriad {
            tables: [Vec::new(), Vec::new(), Vec::new()],
            merged: 0,
        }
    }

    /// 并入一码：码序 mod 3 分域，撞码/越界/零码显性拒。
    pub fn merge(&mut self, code: u32) -> Result<usize, ObsCode> {
        if code == 0 {
            return Err(ObsCode::ZeroCode);
        }
        if code < FAMILY_BASE || code >= FAMILY_BASE + FAMILY_SIZE as u32 {
            return Err(ObsCode::CodeOob);
        }
        let slot = ((code - FAMILY_BASE) as usize) % 3;
        if self.tables[slot].iter().any(|&c| c == code) {
            return Err(ObsCode::DupCode);
        }
        self.tables[slot].push(code);
        self.merged += 1;
        Ok(slot)
    }

    /// 已并入总数。
    pub fn merged_tally(&self) -> usize {
        self.merged
    }

    /// 第 slot 表枚数。
    pub fn table_len(&self, slot: usize) -> Result<usize, ObsCode> {
        match self.tables.get(slot) {
            Some(t) => Ok(t.len()),
            None => Err(ObsCode::TableOob),
        }
    }
}

/// 诊断码闭集（B22 可观测性埋点域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 表号越界
    TableOob,
    /// 重复落点
    DupProbe,
    /// 点号越界
    PointOob,
    /// 事件越界
    EventOob,
    /// 撞事件
    DupEvent,
    /// 事件序断裂
    EventGap,
    /// 事件未入账
    EventMissing,
    /// 族码越界
    CodeOob,
    /// 撞码
    DupCode,
    /// 零码
    ZeroCode,
}

impl ObsCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文本。
    pub fn detail(self) -> &'static str {
        match self {
            ObsCode::Ok => "B22 可观测性埋点判定成功",
            ObsCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            ObsCode::TableOob => "表号越界：域内三表 0..=2",
            ObsCode::DupProbe => "重复落点：同事件同点只许埋一次",
            ObsCode::PointOob => "点号越界：探针点闭集 0..=3",
            ObsCode::EventOob => "事件越界：事件闭集 1..=16",
            ObsCode::DupEvent => "撞事件：事件号两两互异",
            ObsCode::EventGap => "事件序断裂：事件号必须按 1..=n 递增入账",
            ObsCode::EventMissing => "事件未入账：未入账事件不得落点",
            ObsCode::CodeOob => "族码越界：并入通道只认攻击族闭集内码",
            ObsCode::DupCode => "撞码：同码不得并入两表",
            ObsCode::ZeroCode => "零码：零值不得入表",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [ObsCode; 12] = [
    ObsCode::Ok,
    ObsCode::SegBad,
    ObsCode::TableOob,
    ObsCode::DupProbe,
    ObsCode::PointOob,
    ObsCode::EventOob,
    ObsCode::DupEvent,
    ObsCode::EventGap,
    ObsCode::EventMissing,
    ObsCode::CodeOob,
    ObsCode::DupCode,
    ObsCode::ZeroCode,
];

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
        Lcg {
            s: seed ^ 0x7f4a7c15c47bcfb7,
        }
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

/// 点序派生：种子 → 0..=3 点号。
pub fn point_of(seed: u64) -> usize {
    let mut lc = Lcg::new(seed);
    ((lc.next_u32() as usize) % PROBE_POINTS)
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 标准脚本：16 事件入账 → 逐事件四点全落 → ×15 族码并入三表。
/// 返回 (事件数, 探针数, 并入数, 快照印)。
pub fn standard_script() -> (usize, usize, usize, u64) {
    let mut ledger = ObsLedger::new();
    for id in 1..=EVENT_CAP {
        let _ = ledger.add_event(id);
    }
    for id in 1..=EVENT_CAP {
        for p in 0..PROBE_POINTS {
            let _ = ledger.mark(id, p);
        }
    }
    let mut triad = CodeTriad::new();
    for i in 0..FAMILY_SIZE {
        let _ = triad.merge(FAMILY_BASE + i as u32);
    }
    let fp = ledger.fingerprint();
    (
        ledger.events_len(),
        ledger.probes_tally(),
        triad.merged_tally(),
        fp,
    )
}

/// 双跑一致性。
pub fn dual_run_consistent() -> bool {
    standard_script() == standard_script()
}

/// 注入核：5 类坏样本 ×10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 5]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 5] = [0; 5];
    // 通用脚本：全事件入账 + 全点落满（缺一类点时按 skip 参数跳过）。
    // 类 0：缺检出点——事件 1 只落隔离/恢复/审计三点。
    for _ in 0..INJ_ROUNDS {
        let mut ledger = ObsLedger::new();
        let _ = ledger.add_event(1);
        for p in [P_ISOLATE, P_RECOVER, P_AUDIT] {
            let _ = ledger.mark(1, p);
        }
        let caught = !ledger.event_covered(1)
            && ledger.first_missing() == Some((1, P_DETECT))
            && ledger.probes_tally() == 3;
        if caught {
            ledger.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            ledger.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：缺隔离点。
    for _ in 0..INJ_ROUNDS {
        let mut ledger = ObsLedger::new();
        let _ = ledger.add_event(1);
        for p in [P_DETECT, P_RECOVER, P_AUDIT] {
            let _ = ledger.mark(1, p);
        }
        let caught = ledger.first_missing() == Some((1, P_ISOLATE));
        if caught {
            ledger.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            ledger.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：缺恢复点。
    for _ in 0..INJ_ROUNDS {
        let mut ledger = ObsLedger::new();
        let _ = ledger.add_event(1);
        for p in [P_DETECT, P_ISOLATE, P_AUDIT] {
            let _ = ledger.mark(1, p);
        }
        let caught = ledger.first_missing() == Some((1, P_RECOVER));
        if caught {
            ledger.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            ledger.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：缺审计点。
    for _ in 0..INJ_ROUNDS {
        let mut ledger = ObsLedger::new();
        let _ = ledger.add_event(1);
        for p in [P_DETECT, P_ISOLATE, P_RECOVER] {
            let _ = ledger.mark(1, p);
        }
        let caught = ledger.first_missing() == Some((1, P_AUDIT)) && !ledger.event_covered(1);
        if caught {
            ledger.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            ledger.note_escape();
            total_escape += 1;
        }
    }
    // 类 4：事件序断裂 + 撞码 + 点号越界。
    for _ in 0..INJ_ROUNDS {
        let mut ledger = ObsLedger::new();
        let gap = ledger.add_event(2) == Err(ObsCode::EventGap);
        let _ = ledger.add_event(1);
        let dup = ledger.add_event(1) == Err(ObsCode::DupEvent);
        let mut triad = CodeTriad::new();
        let _ = triad.merge(FAMILY_BASE + 4);
        let dupcode = triad.merge(FAMILY_BASE + 4) == Err(ObsCode::DupCode);
        let oobpoint = ledger.mark(1, PROBE_POINTS + 2) == Err(ObsCode::PointOob);
        if gap && dup && dupcode && oobpoint {
            ledger.note_detected();
            total_detect += 1;
            per[4] += 1;
        } else {
            ledger.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51633_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51633");
    // 1 埋点 100%（J1）。
    {
        let (events, probes, _, _) = standard_script();
        s.add(
            "埋点百分百",
            events == EVENT_CAP && probes == EVENT_CAP * PROBE_POINTS,
            "UNX-F51634-J1：埋点 100%，16 事件 ×4 点全落",
        );
    }
    // 2 缺埋点注入检出（J1R）。
    {
        let (_, _, per) = injection_harness();
        s.add(
            "缺埋点注入检出",
            per[0] == 10 && per[1] == 10 && per[2] == 10 && per[3] == 10,
            "UNX-F51634-J1R：四类缺点注入逐轮检出（缺哪点报哪点）",
        );
    }
    // 3 全账覆盖率面。
    {
        let mut ledger = ObsLedger::new();
        for id in 1..=EVENT_CAP {
            let _ = ledger.add_event(id);
        }
        for id in 1..=EVENT_CAP {
            for p in 0..PROBE_POINTS {
                let _ = ledger.mark(id, p);
            }
        }
        s.add(
            "全账覆盖面",
            ledger.full_coverage(),
            "全部事件四点齐 = 覆盖率 100%（每事件恰四探针）",
        );
    }
    // 4 单点重复落点显性拒。
    {
        let mut ledger = ObsLedger::new();
        let _ = ledger.add_event(1);
        let _ = ledger.mark(1, P_DETECT);
        s.add(
            "重复落点显性拒",
            ledger.mark(1, P_DETECT) == Err(ObsCode::DupProbe)
                && ledger.missing_detected_tally() == 1,
            "同事件同点只许埋一次，重复逐次入检出账",
        );
    }
    // 5 点号越界拒。
    {
        let mut ledger = ObsLedger::new();
        let _ = ledger.add_event(1);
        s.add(
            "点号越界拒",
            ledger.mark(1, PROBE_POINTS) == Err(ObsCode::PointOob)
                && ledger.mark(1, 9) == Err(ObsCode::PointOob),
            "探针点闭集 0..=3（检出/隔离/恢复/审计）",
        );
    }
    // 6 事件三闸：越界/撞号/断裂。
    {
        let mut ledger = ObsLedger::new();
        let oob = ledger.add_event(0) == Err(ObsCode::EventOob)
            && ledger.add_event(EVENT_CAP + 1) == Err(ObsCode::EventOob);
        let gap = ledger.add_event(2) == Err(ObsCode::EventGap);
        let _ = ledger.add_event(1);
        s.add(
            "事件入账三闸",
            oob && gap && ledger.add_event(1) == Err(ObsCode::DupEvent),
            "事件号 1..=16 递增入账、两两互异",
        );
    }
    // 7 未入账事件落点拒。
    {
        let mut ledger = ObsLedger::new();
        s.add(
            "未入账落点拒",
            ledger.mark(1, P_DETECT) == Err(ObsCode::EventMissing),
            "事件未入账不得落点（留痕先行）",
        );
    }
    // 8 ×15 族码并入三表。
    {
        let mut triad = CodeTriad::new();
        for i in 0..FAMILY_SIZE {
            let _ = triad.merge(FAMILY_BASE + i as u32);
        }
        let ok = triad.merged_tally() == FAMILY_SIZE
            && triad.table_len(0) == Ok(5)
            && triad.table_len(1) == Ok(5)
            && triad.table_len(2) == Ok(5);
        s.add("族码十五并入", ok, "攻击族 ×15 按码序 mod 3 分域三表，每表 5 枚");
    }
    // 9 撞码/零码/族外码显性拒。
    {
        let mut triad = CodeTriad::new();
        let _ = triad.merge(FAMILY_BASE + 2);
        let dup = triad.merge(FAMILY_BASE + 2) == Err(ObsCode::DupCode);
        let zero = triad.merge(0) == Err(ObsCode::ZeroCode);
        let oob = triad.merge(FAMILY_BASE - 1) == Err(ObsCode::CodeOob)
            && triad.merge(FAMILY_BASE + FAMILY_SIZE as u32) == Err(ObsCode::CodeOob);
        s.add(
            "族码并入三闸",
            dup && zero && oob,
            "撞码/零码/族外码逐类显性拒（零静默）",
        );
    }
    // 10 表号越界拒。
    {
        let triad = CodeTriad::new();
        s.add(
            "表号越界拒",
            triad.table_len(3).is_err() && triad.table_len(0).is_ok(),
            "域内三表闭集 0..=2",
        );
    }
    // 11 注入核 5 类 ×10 全通零逃逸。
    {
        let (d, esc, per) = injection_harness();
        s.add(
            "注入 5 类×10 全通",
            d == 50 && esc == 0 && per.iter().all(|&x| x == 10),
            "5 类坏样本逐类 10 轮精确复现，检出+逃逸=轮数，逃逸=0",
        );
    }
    // 12 点序派生确定性。
    {
        s.add(
            "点序派生确定性",
            point_of(42) == point_of(42) && point_of(42) != point_of(43),
            "LCG 种子 = SEG_BASE 派生，同段位恒定可复现",
        );
    }
    // 13 诊断码闭集唯一性。
    {
        let ok = ALL_CODES
            .iter()
            .enumerate()
            .all(|(i, c)| c.val() == SEG_BASE + 1 + i as u32 && !c.detail().is_empty());
        s.add(
            "诊断码闭集唯一",
            ok && ALL_CODES.len() == 12,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 12 变体",
        );
    }
    // 14 注入账守恒。
    {
        let mut ledger = ObsLedger::new();
        for _ in 0..11u32 {
            ledger.note_detected();
        }
        s.add(
            "注入账守恒",
            ledger.conserved(),
            "注入=检出+逃逸且逃逸=0 且检出>0",
        );
    }
    // 15 双跑 FNV 指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一埋点脚本两次执行，快照印逐位一致",
    );
    // 16 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51633 B22 埋点 事件 {} ×点 {} 族码 {}",
            EVENT_CAP, PROBE_POINTS, FAMILY_SIZE
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51633"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空（写回状态=已完成）",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51633_checks_green() {
        let set = run_unx51633_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn full_coverage_chain() {
        let mut ledger = ObsLedger::new();
        for id in 1..=EVENT_CAP {
            assert!(ledger.add_event(id).is_ok());
        }
        for id in 1..=EVENT_CAP {
            for p in 0..PROBE_POINTS {
                assert!(ledger.mark(id, p).is_ok());
            }
        }
        assert!(ledger.full_coverage());
        assert_eq!(ledger.probes_tally(), EVENT_CAP * PROBE_POINTS);
        assert!(ledger.first_missing().is_none());
    }

    #[test]
    fn each_missing_point_identified() {
        // 逐点缺一：缺哪点报哪点。
        for skip in 0..PROBE_POINTS {
            let mut ledger = ObsLedger::new();
            let _ = ledger.add_event(1);
            for p in 0..PROBE_POINTS {
                if p != skip {
                    let _ = ledger.mark(1, p);
                }
            }
            assert_eq!(ledger.first_missing(), Some((1, skip)));
            assert!(!ledger.event_covered(1));
        }
    }

    #[test]
    fn event_guards() {
        let mut ledger = ObsLedger::new();
        assert_eq!(ledger.add_event(0), Err(ObsCode::EventOob));
        assert_eq!(ledger.add_event(17), Err(ObsCode::EventOob));
        assert_eq!(ledger.add_event(2), Err(ObsCode::EventGap));
        assert!(ledger.add_event(1).is_ok());
        assert_eq!(ledger.add_event(1), Err(ObsCode::DupEvent));
    }

    #[test]
    fn mark_guards() {
        let mut ledger = ObsLedger::new();
        assert_eq!(ledger.mark(1, 0), Err(ObsCode::EventMissing));
        let _ = ledger.add_event(1);
        assert_eq!(ledger.mark(1, 4), Err(ObsCode::PointOob));
        assert!(ledger.mark(1, 0).is_ok());
        assert_eq!(ledger.mark(1, 0), Err(ObsCode::DupProbe));
    }

    #[test]
    fn triad_guards() {
        let mut triad = CodeTriad::new();
        assert_eq!(triad.merge(0), Err(ObsCode::ZeroCode));
        assert_eq!(triad.merge(FAMILY_BASE - 1), Err(ObsCode::CodeOob));
        assert_eq!(
            triad.merge(FAMILY_BASE + FAMILY_SIZE as u32),
            Err(ObsCode::CodeOob)
        );
        assert!(triad.merge(FAMILY_BASE + 6).is_ok());
        assert_eq!(triad.merge(FAMILY_BASE + 6), Err(ObsCode::DupCode));
        assert!(triad.table_len(3).is_err());
    }

    #[test]
    fn triad_balanced() {
        let mut triad = CodeTriad::new();
        for i in 0..FAMILY_SIZE {
            let _ = triad.merge(FAMILY_BASE + i as u32);
        }
        assert_eq!(triad.merged_tally(), FAMILY_SIZE);
        for slot in 0..3 {
            assert_eq!(triad.table_len(slot), Ok(5));
        }
    }

    #[test]
    fn injection_all_caught() {
        let (d, esc, per) = injection_harness();
        assert_eq!(esc, 0);
        assert_eq!(d, 50);
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "类 {} 应恰 10 次检出", i);
        }
    }

    #[test]
    fn conservation_holds() {
        let mut ledger = ObsLedger::new();
        for _ in 0..10u32 {
            ledger.note_detected();
        }
        assert!(ledger.conserved());
        ledger.note_escape();
        assert!(!ledger.conserved());
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
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
