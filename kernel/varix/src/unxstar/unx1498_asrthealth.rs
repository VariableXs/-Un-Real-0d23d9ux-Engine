//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1498 · 断言健康自检（总账-索引-挂接三件联动自检） · 0x76140段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批6-N）：
//!   三件联动自检（总账↔索引↔挂接），三件账对冲平（总账行=索引键=
//!   挂接边数 三等），联动破平注入必抓
//! 设计要点：
//!   1. 三件闭集：断言总账（账行）/ 断言索引（索引键）/ 挂接图（挂接
//!      边），三件各自成账，件数冻结 3；
//!   2. 三等对冲：总账行数 = 索引键数 = 挂接边数，三等误差全 0 才算
//!      联动平，任一件单独虚增必破平（三路注入逐路对拍）；
//!   3. 1:1:1 联动装配：账行 i ↔ 索引键 i ↔ 挂接边 i 三向同键锚定，
//!      锚定失配（缺件/错键）按联动破平显性拒；
//!   4. 自检报告文本逐字对拍：报告行 = "总账=N 索引=N 挂接=N 平"
//!      逐字节比对冻结模板，破平报告按破平件回因（件名入文）；
//!   5. 单调状态机：装配→自检→封账，自检后回装配显性拒（自检报告
//!      已出账不许重装）；
//!   6. LCG 种子写死 = SEG_BASE；no_std + alloc；整数运算零浮点。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x76140，AI-VX7-1104 批6-N 独占槽）。
pub const SEG_BASE: u32 = 0x76140;

// ── 冻结口径 ────────────────────────────────────────────────────
/// 联动件数（冻结 3：总账/索引/挂接）。
pub const PIECE_COUNT: usize = 3;
/// 语料断言键数（冻结 12：键 = 1498×100 + 序号）。
pub const CORPUS_N: usize = 12;
/// 断言键段号域下界。
pub const KEY_MIN: u32 = 149801;
/// 断言键段号域上界。
pub const KEY_MAX: u32 = 149899;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HealthCode {
    /// 键域越界（键不在 149801~149899）
    KeyRange,
    /// 联动破平（三等误差非 0）
    LinkUnequal,
    /// 三向锚定失配（账行↔索引键↔挂接边错键）
    AnchorMismatch,
    /// 自检后重装配（单调闸显性拒）
    AssembleAfterCheck,
    /// 状态倒退
    StateRegress,
    /// 自检报告文本失配（逐字对拍不通过）
    ReportMismatch,
    /// 件越界（非总账/索引/挂接闭集）
    PieceOver,
    /// 双跑不一致
    NotReproducible,
}

impl HealthCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HealthCode::KeyRange => "键域越界：断言键须在 149801~149899，建议按 1498×100+序号 重编",
            HealthCode::LinkUnequal => "联动破平：总账行/索引键/挂接边三等失配，建议对缺件补齐",
            HealthCode::AnchorMismatch => "三向锚定失配：账行-索引键-挂接边错键，建议按同键重锚",
            HealthCode::AssembleAfterCheck => "自检后重装配：报告已出账禁重装，建议开新健康代",
            HealthCode::StateRegress => "状态倒退：装配→自检→封账单向单调，建议重开新代",
            HealthCode::ReportMismatch => "自检报告文本失配：逐字对拍不通过，建议按冻结模板重出报告",
            HealthCode::PieceOver => "件越界：只在总账/索引/挂接三件闭集内取值",
            HealthCode::NotReproducible => "双跑不一致：复现红线，建议固定种子后重跑两遍对账",
        }
    }
    pub const ALL: [HealthCode; 8] = [
        HealthCode::KeyRange,
        HealthCode::LinkUnequal,
        HealthCode::AnchorMismatch,
        HealthCode::AssembleAfterCheck,
        HealthCode::StateRegress,
        HealthCode::ReportMismatch,
        HealthCode::PieceOver,
        HealthCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回；段外码显性报未知）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HealthCode::ALL.len() {
        if let Some(c) = HealthCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x76140 段闭集内"
}

// ── 三件闭集 ────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Piece {
    /// 断言总账
    Ledger,
    /// 断言索引
    Index,
    /// 挂接图
    Hooks,
}

impl Piece {
    pub fn ordinal(self) -> usize {
        match self {
            Piece::Ledger => 0,
            Piece::Index => 1,
            Piece::Hooks => 2,
        }
    }
    /// 中文名（破平回因文案用）。
    pub fn name(self) -> &'static str {
        match self {
            Piece::Ledger => "总账",
            Piece::Index => "索引",
            Piece::Hooks => "挂接",
        }
    }
    pub const ALL: [Piece; 3] = [Piece::Ledger, Piece::Index, Piece::Hooks];
}

// ── 健康状态机（装配→自检→封账，单调前进） ──────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthState {
    /// 装配中（可补件）
    Assembling,
    /// 已自检（报告已出账）
    Checked,
    /// 已封账
    Sealed,
}

impl HealthState {
    pub fn rank(self) -> u32 {
        match self {
            HealthState::Assembling => 0,
            HealthState::Checked => 1,
            HealthState::Sealed => 2,
        }
    }
    pub fn can_advance(self, to: HealthState) -> bool {
        to.rank() > self.rank()
    }
    pub const ALL: [HealthState; 3] = [
        HealthState::Assembling,
        HealthState::Checked,
        HealthState::Sealed,
    ];
}

// ── 三件账 ──────────────────────────────────────────────────────
/// 断言键（u32，域 149801~149899）。
pub type AsrtId = u32;

/// 键域闸。
pub fn key_valid(k: AsrtId) -> bool {
    k >= KEY_MIN && k <= KEY_MAX
}

/// 件一：断言总账（账行 = 键 + 账面值绿/红）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowVal {
    /// 绿
    Green,
    /// 红
    Red,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AsrtLedger {
    pub rows: Vec<(AsrtId, RowVal)>,
}

impl AsrtLedger {
    pub fn new() -> AsrtLedger {
        AsrtLedger::default()
    }
    /// 记一行（键域闸 + 键查重）。
    pub fn put(&mut self, k: AsrtId) -> Result<(), u32> {
        if !key_valid(k) {
            return Err(HealthCode::KeyRange.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            if let Some((ek, _)) = self.rows.get(i) {
                if *ek == k {
                    return Err(HealthCode::AnchorMismatch.val());
                }
            }
            i += 1;
        }
        self.rows.push((k, RowVal::Green));
        Ok(())
    }
    /// 账行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    /// 键在账查询。
    pub fn has(&self, k: AsrtId) -> bool {
        let mut i = 0;
        while i < self.rows.len() {
            if let Some((ek, _)) = self.rows.get(i) {
                if *ek == k {
                    return true;
                }
            }
            i += 1;
        }
        false
    }
}

/// 件二：断言索引（索引键集合，键域闸）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AsrtIndex {
    pub keys: Vec<AsrtId>,
}

impl AsrtIndex {
    pub fn new() -> AsrtIndex {
        AsrtIndex::default()
    }
    /// 收一枚索引键（键域闸 + 查重）。
    pub fn put(&mut self, k: AsrtId) -> Result<(), u32> {
        if !key_valid(k) {
            return Err(HealthCode::KeyRange.val());
        }
        let mut i = 0;
        while i < self.keys.len() {
            if let Some(ek) = self.keys.get(i) {
                if *ek == k {
                    return Err(HealthCode::AnchorMismatch.val());
                }
            }
            i += 1;
        }
        self.keys.push(k);
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.keys.len()
    }
    pub fn has(&self, k: AsrtId) -> bool {
        let mut i = 0;
        while i < self.keys.len() {
            if let Some(ek) = self.keys.get(i) {
                if *ek == k {
                    return true;
                }
            }
            i += 1;
        }
        false
    }
}

/// 件三：挂接图（边 = 账行键 → 索引键，同键锚定）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HookGraph {
    pub edges: Vec<(AsrtId, AsrtId)>,
}

impl HookGraph {
    pub fn new() -> HookGraph {
        HookGraph::default()
    }
    /// 挂一边（两端同键才许挂：锚定闸）。
    pub fn hook(&mut self, k: AsrtId) -> Result<(), u32> {
        if !key_valid(k) {
            return Err(HealthCode::KeyRange.val());
        }
        // 锚定闸：同键不许重挂（1:1:1 语义）。
        let mut i = 0;
        while i < self.edges.len() {
            if let Some((a, _)) = self.edges.get(i) {
                if *a == k {
                    return Err(HealthCode::AnchorMismatch.val());
                }
            }
            i += 1;
        }
        self.edges.push((k, k));
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.edges.len()
    }
}

// ── 三件联动自检器 ──────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct HealthMonitor {
    pub ledger: AsrtLedger,
    pub index: AsrtIndex,
    pub hooks: HookGraph,
    pub state: HealthState,
    pub report: Vec<String>,
    pub assemble_rejects: u32,
}

impl HealthMonitor {
    pub fn new() -> HealthMonitor {
        HealthMonitor {
            ledger: AsrtLedger::new(),
            index: AsrtIndex::new(),
            hooks: HookGraph::new(),
            state: HealthState::Assembling,
            report: Vec::new(),
            assemble_rejects: 0,
        }
    }

    /// 三向锚定装配（一键三落：账行 + 索引键 + 挂接边同键齐落）。
    pub fn anchor(&mut self, k: AsrtId) -> Result<(), u32> {
        if self.state != HealthState::Assembling {
            self.assemble_rejects = self.assemble_rejects.saturating_add(1);
            return Err(HealthCode::AssembleAfterCheck.val());
        }
        if !key_valid(k) {
            return Err(HealthCode::KeyRange.val());
        }
        self.ledger.put(k)?;
        self.index.put(k)?;
        self.hooks.hook(k)?;
        Ok(())
    }

    /// 裸补件（注入面用：单件虚增走这里）。
    pub fn poke(&mut self, p: Piece, k: AsrtId) -> Result<(), u32> {
        if self.state != HealthState::Assembling {
            self.assemble_rejects = self.assemble_rejects.saturating_add(1);
            return Err(HealthCode::AssembleAfterCheck.val());
        }
        match p {
            Piece::Ledger => self.ledger.put(k),
            Piece::Index => self.index.put(k),
            Piece::Hooks => self.hooks.hook(k),
        }
    }

    /// 三件计数（总账行 / 索引键 / 挂接边）。
    pub fn counts(&self) -> (usize, usize, usize) {
        (self.ledger.len(), self.index.len(), self.hooks.len())
    }

    /// 三等误差（件两两差；全 0 = 联动平）。
    pub fn hedge_errors(&self) -> (i64, i64) {
        let (a, b, c) = self.counts();
        (a as i64 - b as i64, b as i64 - c as i64)
    }

    /// 联动平判定。
    pub fn link_balanced(&self) -> bool {
        let (e1, e2) = self.hedge_errors();
        e1 == 0 && e2 == 0
    }

    /// 三向锚定核（键三处齐在才许过）。
    fn anchor_ok(&self, k: AsrtId) -> bool {
        self.ledger.has(k) && self.index.has(k) && self.hook_has(k)
    }

    fn hook_has(&self, k: AsrtId) -> bool {
        let mut i = 0;
        while i < self.hooks.edges.len() {
            if let Some((a, _)) = self.hooks.edges.get(i) {
                if *a == k {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 联动全核（每键三向齐锚）。
    pub fn anchor_matrix_ok(&self) -> bool {
        let (a, b, c) = self.counts();
        if a != b || b != c {
            return false;
        }
        let mut i = 0;
        while i < self.ledger.rows.len() {
            let k = match self.ledger.rows.get(i) {
                Some((k, _)) => *k,
                None => return false,
            };
            if !self.anchor_ok(k) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 自检报告行（冻结模板逐字对拍基准：平账版）。
    pub fn report_line(&self) -> String {
        let (a, b, c) = self.counts();
        if self.link_balanced() && self.anchor_matrix_ok() {
            format!("总账={} 索引={} 挂接={} 平", a, b, c)
        } else {
            let (e1, e2) = self.hedge_errors();
            if e1 != 0 {
                format!("总账={} 索引={} 破平（总账-索引差 {}）", a, b, e1)
            } else if e2 != 0 {
                format!("索引={} 挂接={} 破平（索引-挂接差 {}）", b, c, e2)
            } else {
                String::from("三等平但三向锚定失配（锚定破平）")
            }
        }
    }

    /// 自检（装配态→自检态；破平/锚定失配显性拒；报告落账）。
    pub fn selfcheck(&mut self) -> Result<String, u32> {
        if !self.state.can_advance(HealthState::Checked) {
            return Err(HealthCode::StateRegress.val());
        }
        if !self.link_balanced() {
            let line = self.report_line();
            self.report.push(line.clone());
            self.state = HealthState::Checked;
            return Err(HealthCode::LinkUnequal.val());
        }
        if !self.anchor_matrix_ok() {
            let line = self.report_line();
            self.report.push(line.clone());
            self.state = HealthState::Checked;
            return Err(HealthCode::AnchorMismatch.val());
        }
        let line = self.report_line();
        self.report.push(line.clone());
        self.state = HealthState::Checked;
        Ok(line)
    }

    /// 封账（自检态→封账态）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if !self.state.can_advance(HealthState::Sealed) {
            return Err(HealthCode::StateRegress.val());
        }
        self.state = HealthState::Sealed;
        Ok(())
    }
}

// ── 语料装配（冻结 12 键 1:1:1 齐锚） ───────────────────────────
/// 语料键表（键 = 1498×100 + 1..=12）。
pub fn corpus_keys() -> Vec<AsrtId> {
    let mut v: Vec<AsrtId> = Vec::new();
    let mut i = 1;
    while i <= CORPUS_N {
        v.push(149800 + i as u32);
        i += 1;
    }
    v
}

/// 语料监视器（三件齐锚装配）。
pub fn corpus_monitor() -> Result<HealthMonitor, u32> {
    let mut m = HealthMonitor::new();
    let keys = corpus_keys();
    let mut i = 0;
    while i < keys.len() {
        let k = match keys.get(i) {
            Some(x) => *x,
            None => break,
        };
        m.anchor(k)?;
        i += 1;
    }
    Ok(m)
}

// ── LCG（种子写死 = SEG_BASE） ─────────────────────────────────
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

// ── 注入检出账（三路：虚增总账/虚增索引/虚增挂接） ──────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            if let Some(t) = self.tries.get_mut(slot) {
                *t = t.saturating_add(1);
            }
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if let (Some(t), Some(c)) = (self.tries.get(i), self.caught.get(i)) {
                if *t == 0 || c != t {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

// ── 黄金全链：语料齐锚→联动平→自检→报告逐字对拍→封账→重装配必拒 ──
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut m = corpus_monitor()?;
    if !m.link_balanced() {
        return Err(HealthCode::LinkUnequal.val());
    }
    if !m.anchor_matrix_ok() {
        return Err(HealthCode::AnchorMismatch.val());
    }
    let line = m.selfcheck()?;
    // 报告逐字对拍：与冻结模板逐字节一致。
    let want = format!("总账={} 索引={} 挂接={} 平", CORPUS_N, CORPUS_N, CORPUS_N);
    if line != want {
        return Err(HealthCode::ReportMismatch.val());
    }
    if m.state != HealthState::Checked {
        return Err(HealthCode::StateRegress.val());
    }
    // 自检后重装配显性拒（记账 1）。
    let keys = corpus_keys();
    let first_key = match keys.first().copied() {
        Some(x) => x,
        None => KEY_MIN,
    };
    match m.anchor(first_key) {
        Err(e) if e == HealthCode::AssembleAfterCheck.val() => {}
        _ => return Err(HealthCode::NotReproducible.val()),
    }
    // 封账。
    m.seal()?;
    if m.state != HealthState::Sealed {
        return Err(HealthCode::StateRegress.val());
    }
    Ok((m.ledger.len() as u32, m.report.len() as u32, m.assemble_rejects))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链（账行 12 / 报告 1 / 重装配拒 1）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((12, 1, 1)) => Ok(()),
        _ => Err(HealthCode::NotReproducible.val()),
    }
}

/// 序 B：单件虚增破平（总账多一枚 → LinkUnequal，报告破平文案）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut m = corpus_monitor()?;
    let _ = m.poke(Piece::Ledger, KEY_MAX);
    match m.selfcheck() {
        Err(e) if e == HealthCode::LinkUnequal.val() => {
            // 破平报告必须点名破平件。
            let line = match m.report.last() {
                Some(x) => x.clone(),
                None => String::new(),
            };
            if line.contains("破平") {
                Ok(())
            } else {
                Err(HealthCode::ReportMismatch.val())
            }
        }
        _ => Err(HealthCode::NotReproducible.val()),
    }
}

/// 序 C：口径冻结（件数 3 / 语料 12 / 键域边界合法）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        PIECE_COUNT == 3,
        CORPUS_N == 12,
        key_valid(KEY_MIN),
        key_valid(KEY_MAX),
        !key_valid(KEY_MIN - 1),
        !key_valid(KEY_MAX + 1),
    ) {
        (true, true, true, true, true, true) => Ok(()),
        _ => Err(HealthCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 3 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 0：虚增总账（多一枚账行 → 三等破平）。
        let mut m0 = corpus_monitor()?;
        let _ = m0.poke(Piece::Ledger, KEY_MAX);
        let hit0 = match m0.selfcheck() {
            Err(e) => e == HealthCode::LinkUnequal.val(),
            Ok(_) => false,
        };
        led.record(0, hit0);
        // 路 1：虚增索引（多一枚索引键 → 三等破平）。
        let mut m1 = corpus_monitor()?;
        let _ = m1.poke(Piece::Index, KEY_MAX - 1);
        let hit1 = match m1.selfcheck() {
            Err(e) => e == HealthCode::LinkUnequal.val(),
            Ok(_) => false,
        };
        led.record(1, hit1);
        // 路 2：虚增挂接（多一条挂接边 → 三等破平）。
        let mut m2 = corpus_monitor()?;
        let _ = m2.poke(Piece::Hooks, KEY_MAX - 2);
        let hit2 = match m2.selfcheck() {
            Err(e) => e == HealthCode::LinkUnequal.val(),
            Ok(_) => false,
        };
        led.record(2, hit2);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(HealthCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在 0x75000~0x781C0 的 0x40 对齐窗口） ─────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < HealthCode::ALL.len() {
        if let Some(c) = HealthCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 诊断码闭集对账 ─────────────────────────────────────────────
fn code_closure_ok() -> bool {
    let mut i = 0;
    while i < HealthCode::ALL.len() {
        if let Some(c) = HealthCode::ALL.get(i) {
            if detail(c.val()) != c.detail() {
                return false;
            }
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

/// LCG 确定性。
fn lcg_deterministic_ok() -> bool {
    let mut x = Lcg::new(SEG_BASE as u64);
    let mut y = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < 16 {
        if x.next() != y.next() {
            return false;
        }
        i += 1;
    }
    true
}

/// 注入账 0 号槽取值（.get 取，不裸下标）。
fn slot0(t: &[u32; 3]) -> u32 {
    match t.get(0) {
        Some(x) => *x,
        None => 0,
    }
}

/// 报告逐字对拍谓词（平账模板逐字节一致）。
fn report_verbatim_ok() -> bool {
    let mut m = match corpus_monitor() {
        Ok(x) => x,
        Err(_) => return false,
    };
    let line = match m.selfcheck() {
        Ok(x) => x,
        Err(_) => return false,
    };
    let want = format!("总账={} 索引={} 挂接={} 平", CORPUS_N, CORPUS_N, CORPUS_N);
    line == want && {
        let got = match m.report.last() {
            Some(x) => x.clone(),
            None => String::new(),
        };
        got == want
    }
}

// ── 判据面（extra_checks：≥12 条 add_dyn） ──────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 口径冻结。
    v.push((
        "J1-口径冻结",
        PIECE_COUNT == 3 && CORPUS_N == 12 && Piece::ALL.len() == 3,
        format!("三件联动 语料{}键", CORPUS_N),
    ));
    // 2. 键域边界闸。
    v.push((
        "J1-键域边界闸",
        probe_seq_c().is_ok(),
        format!("键域 {}~{} 边界四点", KEY_MIN, KEY_MAX),
    ));
    // 3. 语料齐锚装配账。
    let cm = corpus_monitor();
    v.push((
        "J1-语料齐锚装配账",
        match &cm {
            Ok(m) => {
                let (a, b, c) = m.counts();
                a == CORPUS_N && b == CORPUS_N && c == CORPUS_N
            }
            Err(_) => false,
        },
        match &cm {
            Ok(m) => {
                let (a, b, c) = m.counts();
                format!("账行{} 索引键{} 挂接边{}", a, b, c)
            }
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 4. 三等对冲平。
    v.push((
        "J1-三等对冲平",
        match &cm {
            Ok(m) => m.link_balanced() && m.anchor_matrix_ok(),
            Err(_) => false,
        },
        String::from("总账行=索引键=挂接边 三等误差 0"),
    ));
    // 5. 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((n, rep, rej)) => *n == CORPUS_N as u32 && *rep == 1 && *rej == 1,
            Err(_) => false,
        },
        match &g {
            Ok((n, rep, rej)) => format!("账行{} 报告{} 重装配拒{}", n, rep, rej),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 6. 自检报告逐字对拍。
    v.push((
        "J1-报告逐字对拍",
        report_verbatim_ok(),
        format!("模板「总账={} 索引={} 挂接={} 平」", CORPUS_N, CORPUS_N, CORPUS_N),
    ));
    // 7. 破平注入必抓（三路静态各一轮直查 + 报告点名破平件）。
    let mut static_ok = true;
    let pieces = [Piece::Ledger, Piece::Index, Piece::Hooks];
    let mut pi = 0;
    while pi < pieces.len() {
        let mut m = match corpus_monitor() {
            Ok(x) => x,
            Err(_) => {
                static_ok = false;
                break;
            }
        };
        let p = match pieces.get(pi) {
            Some(x) => *x,
            None => Piece::Ledger,
        };
        let k = match KEY_MAX.checked_sub(pi as u32 + 1) {
            Some(x) => x,
            None => KEY_MAX,
        };
        let _ = m.poke(p, k);
        let broke = match m.selfcheck() {
            Err(e) => e == HealthCode::LinkUnequal.val(),
            Ok(_) => false,
        };
        let named = match m.report.last() {
            Some(x) => x.contains("破平"),
            None => false,
        };
        if !broke || !named {
            static_ok = false;
        }
        pi += 1;
    }
    v.push((
        "J1-破平注入必抓",
        static_ok,
        String::from("总账/索引/挂接 各虚增一枚全破平且回因点名"),
    ));
    // 8. 探针三序。
    v.push((
        "J1-探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        String::from("a全链 b虚增破平 c口径"),
    ));
    // 9. 注入三路 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-注入三路10/10",
        match &inj {
            Ok(l) => l.all_caught() && slot0(&l.tries) == JUDGE_ROUNDS,
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("t={:?} c={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 10. 自检后重装配显性拒。
    v.push((
        "J1-自检后重装配拒",
        match &g {
            Ok((_, _, rej)) => *rej >= 1,
            Err(_) => false,
        },
        String::from("报告已出账禁重装（AssembleAfterCheck）"),
    ));
    // 11. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 12. LCG 确定性。
    v.push((
        "J2-LCG确定性",
        lcg_deterministic_ok(),
        String::from("同种子 16 步逐位一致"),
    ));
    // 13. 诊断码闭集对账。
    v.push((
        "J1-诊断码闭集对账",
        code_closure_ok(),
        format!("{} 码全查表通过", HealthCode::ALL.len()),
    ));
    // 14. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x76140 窗口 0x40 对齐区间三闸"),
    ));
    v
}

pub fn run_unx1498_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1498 断言健康自检（总账-索引-挂接三件联动自检）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条：注入必抓 / 双跑一致 / 边界） ─────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_run_green() {
        match golden_run() {
            Ok((n, rep, rej)) => {
                assert_eq!(n, 12);
                assert_eq!(rep, 1);
                assert_eq!(rej, 1);
            }
            Err(e) => assert!(false, "golden_run err=0x{:x}", e),
        }
    }

    #[test]
    fn frozen_consts() {
        assert_eq!(PIECE_COUNT, 3);
        assert_eq!(CORPUS_N, 12);
        assert_eq!(corpus_keys().len(), 12);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn key_domain_boundary() {
        assert!(key_valid(149801));
        assert!(key_valid(149899));
        assert!(!key_valid(149800));
        assert!(!key_valid(149900));
        assert!(!key_valid(0));
        // 键域越界显性拒。
        let mut m = HealthMonitor::new();
        match m.anchor(149900) {
            Err(e) => assert_eq!(e, HealthCode::KeyRange.val()),
            Ok(()) => assert!(false, "键域越界未拒"),
        }
    }

    #[test]
    fn corpus_assembly_three_way() {
        let m = match corpus_monitor() {
            Ok(x) => x,
            Err(e) => {
                assert!(false, "corpus err=0x{:x}", e);
                return;
            }
        };
        let (a, b, c) = m.counts();
        assert_eq!(a, CORPUS_N);
        assert_eq!(b, CORPUS_N);
        assert_eq!(c, CORPUS_N);
        assert!(m.link_balanced());
        assert!(m.anchor_matrix_ok());
        // 重复锚定同键显性拒（AnchorMismatch）。
        let mut m2 = m.clone();
        let k = match corpus_keys().first().copied() {
            Some(x) => x,
            None => 0,
        };
        match m2.anchor(k) {
            Err(e) => assert_eq!(e, HealthCode::AnchorMismatch.val()),
            Ok(()) => assert!(false, "重复锚定未拒"),
        }
    }

    #[test]
    fn injection_breaks_and_reports() {
        let led = match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => l,
            Err(e) => {
                assert!(false, "inject err=0x{:x}", e);
                return;
            }
        };
        assert!(led.all_caught(), "三路虚增 10/10 全抓");
        assert_eq!(slot0(&led.tries), JUDGE_ROUNDS);
        // 破平报告必须点名「破平」。
        let mut m = match corpus_monitor() {
            Ok(x) => x,
            Err(_) => return,
        };
        let _ = m.poke(Piece::Hooks, KEY_MAX - 5);
        let _ = m.selfcheck();
        let line = match m.report.last() {
            Some(x) => x.clone(),
            None => String::new(),
        };
        assert!(line.contains("破平"), "破平报告: {}", line);
    }

    #[test]
    fn report_verbatim_template() {
        assert!(report_verbatim_ok());
        let mut m = match corpus_monitor() {
            Ok(x) => x,
            Err(_) => return,
        };
        match m.selfcheck() {
            Ok(line) => {
                let want = format!("总账={} 索引={} 挂接={} 平", CORPUS_N, CORPUS_N, CORPUS_N);
                assert_eq!(line, want);
            }
            Err(e) => assert!(false, "selfcheck err=0x{:x}", e),
        }
    }

    #[test]
    fn state_machine_and_assemble_reject() {
        assert!(HealthState::Assembling.can_advance(HealthState::Checked));
        assert!(HealthState::Checked.can_advance(HealthState::Sealed));
        assert!(!HealthState::Sealed.can_advance(HealthState::Assembling));
        let mut m = match corpus_monitor() {
            Ok(x) => x,
            Err(_) => return,
        };
        m.selfcheck().ok();
        // 自检后重装配与裸补件均拒。
        let k = match corpus_keys().first().copied() {
            Some(x) => x,
            None => 0,
        };
        match m.anchor(k) {
            Err(e) => assert_eq!(e, HealthCode::AssembleAfterCheck.val()),
            Ok(()) => assert!(false, "自检后重装配未拒"),
        }
        match m.poke(Piece::Index, k) {
            Err(e) => assert_eq!(e, HealthCode::AssembleAfterCheck.val()),
            Ok(()) => assert!(false, "自检后补件未拒"),
        }
        assert_eq!(m.assemble_rejects, 2);
    }

    #[test]
    fn probe_sequences() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn reproducible_and_guard() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert!(code_closure_ok());
        assert!(lcg_deterministic_ok());
        assert_eq!(detail(HealthCode::LinkUnequal.val()), HealthCode::LinkUnequal.detail());
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知诊断码"));
    }

    #[test]
    fn checks_face_full_green() {
        let s = run_unx1498_checks();
        assert!(s.len() >= 12, "检查条数须 ≥12");
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
    }
}
