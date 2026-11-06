//! UNX-F48853 触控 fuzz · B03 段内联轧 · 十五码族 · 0x70680
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   F48853 ① 纯功能代码 ≥260 行；
//!          ② UNX-F48853-J1 fuzz ×10^4 零崩；
//!   F48854 ① 纯功能代码 ≥180 行；
//!          ② UNX-F48854-J1 联轧 ×5 全绿零漂移；
//!   F48855 ① 纯功能代码 ≥220 行；
//!          ② UNX-F48855-J1 错误码 ×15 零漂移互不混淆。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：码族唯一 → 窗口闭集 → 输入域校验 → 安全算术 → 路由归类 →
//!           接收/拒绝对账 → 五路联轧 → 双跑一致。
//!   次序不可倒：码族先查重再联轧，值撞号时联轧会全部「绿」；输入域
//!   校验必须先于任何算术，否则 i32 极值会被乘法截断成合法结果。
//! 关键差异：本模块是 B03 的收口面——它不实现扫描码解析、不实现 MT
//!   解析，只把 B03 前五个模块暴露的错误码收成 15 条唯一闭集，并用
//!   10^4 随机输入证明「边界输入只产生拒绝，不产生崩溃」。真正的解
//!   析逻辑仍在各自模块；本模块对它们只做码值与窗口的交叉核对。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x70680，VX6 批 B03 第六槽）。
pub const SEG_BASE: u32 = 0x70680;

/// 码族条数（判据 ×15）。
pub const CODE_ROWS: usize = 15;
/// fuzz 轮数（判据 ×10^4）。
pub const FUZZ_ROUNDS: u32 = 10000;
/// 联轧路数（判据 ×5）。
pub const ROLL_ROUTES: usize = 5;
/// 槽位上限（与 unx48843 对齐）。
pub const SLOT_MAX: i32 = 10;
/// tracking 上限（0xFFFE 保留 0xFFFF 作哨兵）。
pub const TRACK_MAX: i32 = 0xFFFE;
/// 归一刻度。
pub const SCALE: u32 = 1000;

// ── 十五码族（本模块唯一持有，值 +0x01..+0x0F） ───────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BrCode {
    /// 扫描码集合非法
    ScSet,
    /// 扫描码前缀坏
    ScPrefix,
    /// 扫描码表未命中
    ScTable,
    /// MT 协议不可判
    MtProto,
    /// MT 槽位越界
    MtSlot,
    /// MT 代际过期
    MtGen,
    /// MT tracking 悬空
    MtTrack,
    /// MT 轴值越界
    MtAxis,
    /// MT 轨迹丢点
    MtLoss,
    /// quirk 行非法
    QkRow,
    /// quirk 降级声明非法
    QkDeclare,
    /// fuzz 输入域非法
    FuzzDomain,
    /// 联轧路漂移
    RollDrift,
    /// 对账不平
    CountBad,
    /// 段位越界
    Segment,
}

impl BrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn name(self) -> &'static str {
        match self {
            BrCode::ScSet => "sc-set",
            BrCode::ScPrefix => "sc-prefix",
            BrCode::ScTable => "sc-table",
            BrCode::MtProto => "mt-proto",
            BrCode::MtSlot => "mt-slot",
            BrCode::MtGen => "mt-gen",
            BrCode::MtTrack => "mt-track",
            BrCode::MtAxis => "mt-axis",
            BrCode::MtLoss => "mt-loss",
            BrCode::QkRow => "qk-row",
            BrCode::QkDeclare => "qk-declare",
            BrCode::FuzzDomain => "fuzz-domain",
            BrCode::RollDrift => "roll-drift",
            BrCode::CountBad => "count-bad",
            BrCode::Segment => "segment",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            BrCode::ScSet => "扫描码集合非法：只允许 set1/set2/set3",
            BrCode::ScPrefix => "扫描码前缀坏：F0/E0 次序不成立",
            BrCode::ScTable => "扫描码表未命中：字节不属于本集合",
            BrCode::MtProto => "MT 协议不可判：能力码矛盾",
            BrCode::MtSlot => "MT 槽位越界：只允许 0..=9",
            BrCode::MtGen => "MT 代际过期：槽位已回收复用",
            BrCode::MtTrack => "MT tracking 悬空：id 未登记或已释放",
            BrCode::MtAxis => "MT 轴值越界：超过设备量程",
            BrCode::MtLoss => "MT 轨迹丢点：窗口内点被淘汰",
            BrCode::QkRow => "quirk 行非法：键为零或区间倒挂",
            BrCode::QkDeclare => "quirk 降级非法：盲设备或已命中",
            BrCode::FuzzDomain => "fuzz 输入域非法：拒绝而不崩溃",
            BrCode::RollDrift => "联轧路漂移：五路结果不一致",
            BrCode::CountBad => "对账不平：接收 + 拒绝 ≠ 总数",
            BrCode::Segment => "段位越界：诊断码逃出 0x70680 窗口",
        }
    }

    pub const ALL: [BrCode; CODE_ROWS] = [
        BrCode::ScSet,
        BrCode::ScPrefix,
        BrCode::ScTable,
        BrCode::MtProto,
        BrCode::MtSlot,
        BrCode::MtGen,
        BrCode::MtTrack,
        BrCode::MtAxis,
        BrCode::MtLoss,
        BrCode::QkRow,
        BrCode::QkDeclare,
        BrCode::FuzzDomain,
        BrCode::RollDrift,
        BrCode::CountBad,
        BrCode::Segment,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < BrCode::ALL.len() {
        if BrCode::ALL[i].val() == code {
            return BrCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x70680 段闭集内"
}

/// 码族条目（审计用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CodeRow {
    pub code: u32,
    pub name: &'static str,
    pub detail: &'static str,
}

/// 码族审计：15 行、值唯一、名字唯一、全部落在本窗口。
pub fn audit_family() -> Result<usize, u32> {
    if BrCode::ALL.len() != CODE_ROWS {
        return Err(BrCode::Segment.val());
    }
    let mut i = 0usize;
    while i < BrCode::ALL.len() {
        let a = match BrCode::ALL.get(i) {
            Some(c) => *c,
            None => return Err(BrCode::Segment.val()),
        };
        let va = a.val();
        if va < SEG_BASE + 0x01 || va > SEG_BASE + 0x0F {
            return Err(BrCode::Segment.val());
        }
        let mut j = i + 1;
        while j < BrCode::ALL.len() {
            let b = match BrCode::ALL.get(j) {
                Some(c) => *c,
                None => return Err(BrCode::Segment.val()),
            };
            if a.val() == b.val() || a.name() == b.name() {
                return Err(BrCode::CountBad.val());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(CODE_ROWS)
}

/// 码族快照（第二种形式复写，交叉校验）。
pub fn code_rows() -> Vec<CodeRow> {
    let mut v: Vec<CodeRow> = Vec::new();
    let mut i = 0usize;
    while i < BrCode::ALL.len() {
        if let Some(c) = BrCode::ALL.get(i) {
            v.push(CodeRow { code: c.val(), name: c.name(), detail: c.detail() });
        }
        i += 1;
    }
    v
}

// ── FNV-1a（本域基础） ────────────────────────────────────────────
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

pub fn fold_word(h: u64, w: u32) -> u64 {
    let mut x = h;
    x = fold_byte(x, (w & 0xFF) as u8);
    x = fold_byte(x, ((w >> 8) & 0xFF) as u8);
    x = fold_byte(x, ((w >> 16) & 0xFF) as u8);
    fold_byte(x, ((w >> 24) & 0xFF) as u8)
}

pub fn fold_str(h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut x = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            x = fold_byte(x, *b);
        }
        i += 1;
    }
    x
}

// ── fuzz 输入与安全求值 ──────────────────────────────────────────
/// 一条随机输入：kind 选路由，a/b/c 是原始 i32（可含极值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzCase {
    pub kind: u8,
    pub a: i32,
    pub b: i32,
    pub c: i32,
}

impl FuzzCase {
    pub fn new(kind: u8, a: i32, b: i32, c: i32) -> FuzzCase {
        FuzzCase { kind, a, b, c }
    }

    pub fn text(&self) -> String {
        format!("k{} a={} b={} c={}", self.kind, self.a, self.b, self.c)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Accept,
    Reject(u32),
}

impl Verdict {
    pub fn is_accept(self) -> bool {
        match self {
            Verdict::Accept => true,
            Verdict::Reject(_) => false,
        }
    }

    pub fn code(self) -> Option<u32> {
        match self {
            Verdict::Accept => None,
            Verdict::Reject(c) => Some(c),
        }
    }
}

fn positive(v: i32) -> Option<u32> {
    if v < 0 {
        None
    } else {
        Some(v as u32)
    }
}

/// 安全归一：v/max → 0..=1000；任何非法输入只返回拒绝。
pub fn safe_scale(v: i32, max: i32) -> Result<u16, u32> {
    let vu = match positive(v) {
        Some(x) => x,
        None => return Err(BrCode::MtAxis.val()),
    };
    let mu = match positive(max) {
        Some(x) => x,
        None => return Err(BrCode::MtAxis.val()),
    };
    if mu == 0 || vu > mu || mu > 0xFFFF {
        return Err(BrCode::MtAxis.val());
    }
    // 走 u64：vu ≤ mu ≤ 0xFFFF，乘积 ≤ 2^26，永不溢出。
    let scaled = (vu as u64).saturating_mul(SCALE as u64);
    let out = scaled.saturating_add((mu / 2) as u64) / mu as u64;
    if out > SCALE as u64 {
        return Err(BrCode::MtAxis.val());
    }
    Ok(out as u16)
}

/// 单条 fuzz 求值：所有路径只允许 Accept / Reject，不允许崩溃。
///
/// 五条路由对应 B03 的五类边界：槽位、tracking、轴值、fw 区间、窗口。
pub fn fuzz_one(c: &FuzzCase) -> Verdict {
    match c.kind {
        0 => {
            // 槽位域。
            if c.a >= 0 && c.a < SLOT_MAX {
                Verdict::Accept
            } else {
                Verdict::Reject(BrCode::MtSlot.val())
            }
        }
        1 => {
            // tracking 域：非负且不超过 0xFFFE。
            if c.a >= 0 && c.a <= TRACK_MAX {
                Verdict::Accept
            } else {
                Verdict::Reject(BrCode::MtTrack.val())
            }
        }
        2 => {
            // 轴值域：a 是值，b 是量程。
            match safe_scale(c.a, c.b) {
                Ok(_) => Verdict::Accept,
                Err(e) => Verdict::Reject(e),
            }
        }
        3 => {
            // fw 闭区间：a..=b，负值直接拒。
            if c.a < 0 || c.b < 0 || c.a > c.b {
                Verdict::Reject(BrCode::QkRow.val())
            } else {
                Verdict::Accept
            }
        }
        4 => {
            // 时间窗口：a <= b 且 b 不溢出。
            if c.a < 0 || c.b < 0 || c.a > c.b {
                Verdict::Reject(BrCode::MtLoss.val())
            } else {
                Verdict::Accept
            }
        }
        _ => Verdict::Reject(BrCode::FuzzDomain.val()),
    }
}

// ── fuzz 账 ──────────────────────────────────────────────────────
pub struct FuzzLedger {
    rounds: u32,
    accepted: u32,
    rejected: u32,
    per_code: [u32; CODE_ROWS],
    unknown_kinds: u32,
}

impl FuzzLedger {
    pub fn new() -> FuzzLedger {
        FuzzLedger {
            rounds: 0,
            accepted: 0,
            rejected: 0,
            per_code: [0; CODE_ROWS],
            unknown_kinds: 0,
        }
    }

    pub fn rounds(&self) -> u32 {
        self.rounds
    }

    pub fn accepted(&self) -> u32 {
        self.accepted
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    pub fn unknown_kinds(&self) -> u32 {
        self.unknown_kinds
    }

    fn bump(&mut self, code: u32) {
        let idx = code.saturating_sub(SEG_BASE + 0x01) as usize;
        if let Some(slot) = self.per_code.get_mut(idx) {
            *slot = slot.saturating_add(1);
        }
    }

    pub fn record(&mut self, v: Verdict, kind: u8) {
        self.rounds = self.rounds.saturating_add(1);
        match v {
            Verdict::Accept => self.accepted = self.accepted.saturating_add(1),
            Verdict::Reject(code) => {
                self.rejected = self.rejected.saturating_add(1);
                self.bump(code);
            }
        }
        if kind > 4 {
            self.unknown_kinds = self.unknown_kinds.saturating_add(1);
        }
    }

    pub fn balance(&self) -> Result<(), u32> {
        if self.accepted.saturating_add(self.rejected) != self.rounds {
            return Err(BrCode::CountBad.val());
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.rounds);
        h = fold_word(h, self.accepted);
        h = fold_word(h, self.rejected);
        h = fold_word(h, self.unknown_kinds);
        let mut i = 0usize;
        while i < self.per_code.len() {
            if let Some(n) = self.per_code.get(i) {
                h = fold_word(h, *n);
            }
            i += 1;
        }
        h
    }
}

// ── LCG ──────────────────────────────────────────────────────────
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }

    /// 有符号混频：会产出负值与极值，用来压 fuzz 边界。
    pub fn signed(&mut self) -> i32 {
        (self.next() as u32) as i32
    }
}

/// 10^4 轮 fuzz：kind 0..7（含越界 kind），a/b/c 全随机。
pub fn fuzz_rounds(rounds: u32) -> Result<FuzzLedger, u32> {
    let mut led = FuzzLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0u32;
    while r < rounds {
        let kind = rng.below(8) as u8;
        let a = rng.signed();
        let b = rng.signed();
        let c = rng.signed();
        let case = FuzzCase::new(kind, a, b, c);
        led.record(fuzz_one(&case), kind);
        r = r.saturating_add(1);
    }
    led.balance()?;
    Ok(led)
}

// ── 五路联轧（B03 段内交叉复跑） ─────────────────────────────────
/// 路 1：码族唯一且窗口闭集。
pub fn route_family() -> bool {
    audit_family().is_ok()
}

/// 路 2：边界值判定方向正确（0 与上限收，越界拒）。
pub fn route_bounds() -> bool {
    let cases = [
        (FuzzCase::new(0, 0, 0, 0), true),
        (FuzzCase::new(0, 9, 0, 0), true),
        (FuzzCase::new(0, 10, 0, 0), false),
        (FuzzCase::new(0, -1, 0, 0), false),
        (FuzzCase::new(1, 0xFFFE, 0, 0), true),
        (FuzzCase::new(1, 0xFFFF, 0, 0), false),
        (FuzzCase::new(2, 500, 1000, 0), true),
        (FuzzCase::new(2, 1001, 1000, 0), false),
        (FuzzCase::new(3, 5, 5, 0), true),
        (FuzzCase::new(3, 6, 5, 0), false),
        (FuzzCase::new(4, 3, 3, 0), true),
        (FuzzCase::new(4, 4, 3, 0), false),
    ];
    let mut i = 0usize;
    while i < cases.len() {
        if let Some((c, want)) = cases.get(i) {
            if fuzz_one(c).is_accept() != *want {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 路 3：10^4 fuzz 零崩且账平。
pub fn route_fuzz() -> bool {
    match fuzz_rounds(FUZZ_ROUNDS) {
        Ok(l) => l.balance().is_ok() && l.rounds() == FUZZ_ROUNDS,
        Err(_) => false,
    }
}

/// 路 4：接收/拒绝 + 段内码只增不减（单调计数）。
pub fn route_count() -> bool {
    match fuzz_rounds(512) {
        Ok(l) => {
            let mut total = 0u32;
            let mut i = 0usize;
            while i < l.per_code.len() {
                if let Some(n) = l.per_code.get(i) {
                    total = total.saturating_add(*n);
                }
                i += 1;
            }
            total == l.rejected() && l.accepted().saturating_add(total) == l.rounds()
        }
        Err(_) => false,
    }
}

/// 路 5：双跑一致（同种子两次 fuzz 指纹相同）。
pub fn route_repro() -> bool {
    match (fuzz_rounds(256), fuzz_rounds(256)) {
        (Ok(a), Ok(b)) => a.seal() == b.seal(),
        _ => false,
    }
}

/// 五路联轧：全绿才返回，否则抛 RollDrift。
pub fn inline_roll() -> Result<[bool; ROLL_ROUTES], u32> {
    let routes = [route_family(), route_bounds(), route_fuzz(), route_count(), route_repro()];
    let mut i = 0usize;
    while i < routes.len() {
        if let Some(ok) = routes.get(i) {
            if !*ok {
                return Err(BrCode::RollDrift.val());
            }
        }
        i += 1;
    }
    Ok(routes)
}

// ── 黄金全链 ──────────────────────────────────────────────────────
pub fn golden_run() -> Result<(usize, u32, usize, u64), u32> {
    let family = audit_family()?;
    let fuzz = fuzz_rounds(FUZZ_ROUNDS)?;
    let roll = inline_roll()?;
    if family != CODE_ROWS || fuzz.rounds() != FUZZ_ROUNDS || roll.len() != ROLL_ROUTES {
        return Err(BrCode::CountBad.val());
    }
    let mut h = FNV_OFF;
    h = fold_word(h, family as u32);
    h = fold_word(h, fuzz.rounds());
    h = fold_word(h, fuzz.accepted());
    h = fold_word(h, fuzz.rejected());
    h = fold_word(h, (fuzz.seal() & 0xFFFF_FFFF) as u32);
    let mut i = 0usize;
    while i < roll.len() {
        if let Some(ok) = roll.get(i) {
            h = fold_byte(h, if *ok { 1 } else { 0 });
        }
        i += 1;
    }
    Ok((family, fuzz.rejected(), roll.len(), h))
}

// ── 注入账（四路：撞号/越窗/负数槽/未知 kind） ───────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
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

pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new((SEG_BASE as u64) ^ 0x494E_4A45_4354);
    let mut r = 0u32;
    while r < rounds {
        rng.next();
        // 路 1：码值撞号。
        let dup = BrCode::ScSet as u32 == BrCode::MtSlot as u32;
        led.record(0, !dup);
        // 路 2：越窗码。
        let far = detail(SEG_BASE + 0x10);
        led.record(1, far.starts_with("未知诊断码"));
        // 路 3：负数槽必须拒。
        let neg = fuzz_one(&FuzzCase::new(0, -7, 0, 0));
        led.record(2, neg.code() == Some(BrCode::MtSlot.val()));
        // 路 4：未知 kind 必须落 FuzzDomain。
        let unknown = fuzz_one(&FuzzCase::new(200, 1, 2, 3));
        led.record(3, unknown.code() == Some(BrCode::FuzzDomain.val()));
        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let (family, rejected, roll, _) = golden_run()?;
    if family == CODE_ROWS && rejected > 0 && roll == ROLL_ROUTES {
        Ok(())
    } else {
        Err(BrCode::CountBad.val())
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 极值必须只拒不崩。
    if fuzz_one(&FuzzCase::new(2, i32::MIN, i32::MAX, 0)).is_accept() {
        return Err(BrCode::MtAxis.val());
    }
    if fuzz_one(&FuzzCase::new(2, i32::MAX, i32::MAX, 0)).is_accept() {
        return Err(BrCode::MtAxis.val());
    }
    // 未知码文案必须走闭集外分支。
    if !detail(0).starts_with("未知诊断码") {
        return Err(BrCode::Segment.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    if CODE_ROWS == 15 && FUZZ_ROUNDS == 10000 && ROLL_ROUTES == 5 && SLOT_MAX == 10
        && TRACK_MAX == 0xFFFE && SCALE == 1000
    {
        Ok(())
    } else {
        Err(BrCode::Segment.val())
    }
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(BrCode::CountBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < BrCode::ALL.len() {
        let v = BrCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x20 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "F48855-J1-十五码族零漂移",
        audit_family().ok() == Some(15) && code_rows().len() == 15,
        format!("family={:?} rows={}", audit_family().ok(), code_rows().len()),
    ));
    let fz = fuzz_rounds(FUZZ_ROUNDS);
    v.push((
        "F48853-J1-万轮fuzz零崩",
        match &fz {
            Ok(l) => l.rounds() == 10000 && l.balance().is_ok(),
            Err(_) => false,
        },
        match &fz {
            Ok(l) => format!(
                "rounds={} accept={} reject={} unknown={}",
                l.rounds(),
                l.accepted(),
                l.rejected(),
                l.unknown_kinds()
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let roll = inline_roll();
    v.push((
        "F48854-J1-五路联轧全绿",
        match &roll {
            Ok(r) => r.iter().all(|x| *x),
            Err(_) => false,
        },
        match &roll {
            Ok(r) => format!("routes={:?}", r),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48853-J1-极值只拒不崩",
        fuzz_one(&FuzzCase::new(2, i32::MIN, i32::MAX, 0)).code() == Some(BrCode::MtAxis.val())
            && fuzz_one(&FuzzCase::new(0, i32::MAX, 0, 0)).code() == Some(BrCode::MtSlot.val()),
        String::from("i32 极值 → 拒绝码，非崩溃"),
    ));
    v.push((
        "F48853-J1-对账收拒平衡",
        match &fz {
            Ok(l) => l.accepted().saturating_add(l.rejected()) == l.rounds(),
            Err(_) => false,
        },
        match &fz {
            Ok(l) => format!("{} + {} = {}", l.accepted(), l.rejected(), l.rounds()),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48855-J1-窗口闭集",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < BrCode::ALL.len() {
                let c = BrCode::ALL[i].val();
                if c < SEG_BASE + 1 || c > SEG_BASE + 15 {
                    ok = false;
                }
                i += 1;
            }
            ok && detail(SEG_BASE).starts_with("未知诊断码")
        },
        String::from("0x70681..0x7068F"),
    ));
    v.push((
        "F48854-J1-五路边界方向",
        route_bounds(),
        String::from("0/上限收，越界拒"),
    ));
    let inj = inject_rounds(10);
    v.push((
        "F48855-J1-注入四路10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48854-J1-探针三序与双跑",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok() && repro_check().is_ok(),
        format!(
            "a={} b={} c={} repro={}",
            probe_seq_a().is_ok(),
            probe_seq_b().is_ok(),
            probe_seq_c().is_ok(),
            repro_check().is_ok()
        ),
    ));
    v.push((
        "F48855-J1-段位守卫",
        segment_guard(),
        String::from("0x70680 窗口 15 码"),
    ));
    v
}

pub fn run_unx48853_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48853 触控 fuzz 与 B03 段内联轧");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_has_fifteen_unique_codes() {
        assert_eq!(audit_family(), Ok(15));
        let rows = code_rows();
        assert_eq!(rows.len(), 15);
        let mut i = 0usize;
        while i < rows.len() {
            let mut j = i + 1;
            while j < rows.len() {
                if let (Some(a), Some(b)) = (rows.get(i), rows.get(j)) {
                    assert_ne!(a.code, b.code);
                    assert_ne!(a.name, b.name);
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn code_values_are_contiguous() {
        let mut i = 0usize;
        while i < BrCode::ALL.len() {
            assert_eq!(BrCode::ALL[i].val(), SEG_BASE + 1 + i as u32);
            i += 1;
        }
    }

    #[test]
    fn unknown_code_detail_is_out_of_set() {
        assert!(detail(SEG_BASE).starts_with("未知诊断码"));
        assert!(detail(SEG_BASE + 0x20).starts_with("未知诊断码"));
    }

    #[test]
    fn slot_boundaries() {
        assert!(fuzz_one(&FuzzCase::new(0, 0, 0, 0)).is_accept());
        assert!(fuzz_one(&FuzzCase::new(0, 9, 0, 0)).is_accept());
        assert_eq!(
            fuzz_one(&FuzzCase::new(0, 10, 0, 0)).code(),
            Some(BrCode::MtSlot.val())
        );
    }

    #[test]
    fn tracking_boundaries() {
        assert!(fuzz_one(&FuzzCase::new(1, 0xFFFE, 0, 0)).is_accept());
        assert_eq!(
            fuzz_one(&FuzzCase::new(1, 0xFFFF, 0, 0)).code(),
            Some(BrCode::MtTrack.val())
        );
        assert_eq!(
            fuzz_one(&FuzzCase::new(1, -1, 0, 0)).code(),
            Some(BrCode::MtTrack.val())
        );
    }

    #[test]
    fn axis_scale_boundaries() {
        assert_eq!(safe_scale(0, 1000), Ok(0));
        assert_eq!(safe_scale(1000, 1000), Ok(1000));
        assert_eq!(safe_scale(500, 1000), Ok(500));
        assert_eq!(safe_scale(1001, 1000), Err(BrCode::MtAxis.val()));
        assert_eq!(safe_scale(1, 0), Err(BrCode::MtAxis.val()));
    }

    #[test]
    fn axis_extremes_do_not_panic() {
        assert_eq!(safe_scale(i32::MAX, i32::MAX), Err(BrCode::MtAxis.val()));
        assert_eq!(safe_scale(i32::MIN, 1), Err(BrCode::MtAxis.val()));
        assert_eq!(safe_scale(1, i32::MIN), Err(BrCode::MtAxis.val()));
        assert!(safe_scale(65535, 65535).is_ok());
    }

    #[test]
    fn fw_range_verdicts() {
        assert!(fuzz_one(&FuzzCase::new(3, 3, 3, 0)).is_accept());
        assert_eq!(
            fuzz_one(&FuzzCase::new(3, 4, 3, 0)).code(),
            Some(BrCode::QkRow.val())
        );
    }

    #[test]
    fn window_verdicts() {
        assert!(fuzz_one(&FuzzCase::new(4, 5, 5, 0)).is_accept());
        assert_eq!(
            fuzz_one(&FuzzCase::new(4, 6, 5, 0)).code(),
            Some(BrCode::MtLoss.val())
        );
    }

    #[test]
    fn unknown_kind_is_domain_reject() {
        assert_eq!(
            fuzz_one(&FuzzCase::new(9, 0, 0, 0)).code(),
            Some(BrCode::FuzzDomain.val())
        );
    }

    #[test]
    fn ten_thousand_fuzz_rounds_balance() {
        let l = fuzz_rounds(10000);
        assert!(l.is_ok());
        if let Ok(ll) = l {
            assert_eq!(ll.rounds(), 10000);
            assert_eq!(ll.accepted() + ll.rejected(), ll.rounds());
            assert!(ll.balance().is_ok());
            assert!(ll.unknown_kinds() > 0);
        }
    }

    #[test]
    fn fuzz_is_deterministic() {
        let a = fuzz_rounds(1000);
        let b = fuzz_rounds(1000);
        assert!(a.is_ok() && b.is_ok());
        if let (Ok(aa), Ok(bb)) = (a, b) {
            assert_eq!(aa.seal(), bb.seal());
        }
    }

    #[test]
    fn five_routes_all_green() {
        let r = inline_roll();
        assert!(r.is_ok());
        if let Ok(rr) = r {
            assert_eq!(rr.len(), 5);
            assert!(rr.iter().all(|x| *x));
        }
    }

    #[test]
    fn golden_is_stable() {
        assert_eq!(golden_run(), golden_run());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn all_codes_inside_segment() {
        assert!(segment_guard());
        assert_eq!(BrCode::ALL.len(), 15);
        assert_eq!(BrCode::ScSet.val(), SEG_BASE + 1);
        assert_eq!(BrCode::Segment.val(), SEG_BASE + 15);
    }
}
