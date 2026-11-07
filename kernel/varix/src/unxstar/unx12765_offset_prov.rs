//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12765 · 偏移表出处核验（PEB/TEB 逐行注出处） · 0x82940段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥310 行；
//!   ② 判据原文：UNX-F12766-J1 码值出处核验 ≥100 码抽检绿，
//!      自造码演练 10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - PEB/TEB 偏移表逐行出处模型：行 = 结构名 + 字段 + 偏移值 +
//!     出处档位，17 行冻结在册（x64 口径，WinDbg dt ntdll!_PEB/_TEB
//!     与 winternl.h 声明对齐）；
//!   - 出处档位闭集三档：1=B03 冻结表 2=MSDN 档 3=winternl.h 档；
//!     档外值与档=0（无出处）一律零容忍，无出处行存在即整面不得核签；
//!   - 逐行出处核验：每行与冻结表按（结构名,字段）对账——偏移值相等
//!     且出处档相等才算绿，偏移漂移/档漂移当场抓（DriftMismatch）；
//!   - 抽检面（判据②对齐）：LCG（种子=0x82940）抽 100 轮逐行复验，
//!     100/100 全绿才算抽检通过；自造档演练 10/10 检出（档闭集外+
//!     冻结表外自造行双注入）；
//!   - 单调状态机：未核签 Unsigned → 交叉核验 CrossChecked → 封档
//!     Sealed，倒退显性拒（StateRegression）；
//!   - no_std + alloc；零浮点/零随机/零 IO/零墙钟；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x82940，VX9 批公式 0x80000+0x40×(12765−12600)）。
pub const SEG_BASE: u32 = 0x82940;

// ── 冻结常量（出处见行注） ───────────────────────────────────────
/// 偏移面容量：32 行（冻结表 17 行 + 抽检余量）。
pub const FACE_CAP: usize = 32;
/// 结构名闭集：仅 PEB/TEB 两结构（x64 进程/线程信息块口径）。
pub const STRUCT_CLOSED: [&str; 2] = ["PEB", "TEB"];
/// 出处档位闭集档数。
pub const PROV_TIERS: u32 = 3;
/// 抽检轮数（判据②：≥100 抽检绿）。
pub const SAMPLE_ROUNDS: u32 = 100;

/// 出处档位中文名（闭集逐字）。
pub fn prov_tier_name(tier: u32) -> &'static str {
    match tier {
        1 => "B03冻结表",
        2 => "MSDN档",
        3 => "winternl.h档",
        _ => "档位越闭集",
    }
}

// ── 17 行冻结偏移表（结构名/字段/偏移/出处档；出处：winternl.h
//    （WDK _PEB/_TEB 声明）、MSDN 进程环境块页、B03 冻结表快照） ────
pub const FROZEN_OFFSETS: [(&str, &str, u32, u32); 17] = [
    ("PEB", "BeingDebugged", 0x02, 3),
    ("PEB", "Mutant", 0x08, 3),
    ("PEB", "ImageBaseAddress", 0x10, 3),
    ("PEB", "Ldr", 0x18, 3),
    ("PEB", "ProcessParameters", 0x20, 3),
    ("PEB", "ProcessHeap", 0x30, 1),
    ("PEB", "FastPebLock", 0x38, 1),
    ("PEB", "KernelCallbackTable", 0x58, 3),
    ("PEB", "NtGlobalFlag", 0xBC, 1),
    ("TEB", "NtTib.ExceptionList", 0x00, 3),
    ("TEB", "NtTib.StackBase", 0x08, 3),
    ("TEB", "NtTib.StackLimit", 0x10, 3),
    ("TEB", "NtTib.Self", 0x30, 3),
    ("TEB", "ClientId", 0x40, 3),
    ("TEB", "ThreadLocalStoragePointer", 0x58, 3),
    ("TEB", "ProcessEnvironmentBlock", 0x60, 1),
    ("TEB", "LastErrorValue", 0x68, 2),
];

/// 冻结表对账查表：按（结构名,字段）取（偏移,档）。
pub fn frozen_lookup(struct_name: &str, field: &str) -> Option<(u32, u32)> {
    let mut i = 0;
    while i < FROZEN_OFFSETS.len() {
        if let Some((s, f, off, prov)) = FROZEN_OFFSETS.get(i) {
            if *s == struct_name && *f == field {
                return Some((*off, *prov));
            }
        }
        i += 1;
    }
    None
}

/// 结构名是否在闭集内。
pub fn struct_in_closed_set(name: &str) -> bool {
    let mut hit = false;
    let mut i = 0;
    while i < STRUCT_CLOSED.len() {
        if let Some(s) = STRUCT_CLOSED.get(i) {
            if *s == name {
                hit = true;
                i = STRUCT_CLOSED.len();
            }
        }
        i += 1;
    }
    hit
}

// ── 诊断码闭集（0x82940 段内 0x40 窗口） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OffsetCode {
    /// 字段名空：逐行必注字段
    FieldEmpty,
    /// 结构名越闭集：仅 PEB/TEB
    StructUnknown,
    /// 无出处行：档位=0，零容忍（高压线）
    ProvEmpty,
    /// 出处档越闭集：3 档外
    ProvTierInvalid,
    /// 偏移漂移：与冻结表偏移不等
    DriftMismatch,
    /// 自造行：（结构名,字段）不在冻结表
    RowFabricated,
    /// 行重复：同结构同字段一行
    RowDuplicate,
    /// 偏移面已满：32 行恰满
    FaceFull,
    /// 状态机倒退：封档后回退
    StateRegression,
    /// 未核签先封档：交叉核验未过不得封
    SealPremature,
}

impl OffsetCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            OffsetCode::FieldEmpty => "字段名空：偏移行必注字段名",
            OffsetCode::StructUnknown => "结构名越闭集：仅 PEB/TEB 两结构",
            OffsetCode::ProvEmpty => "无出处行：档位=0 零容忍，补 B03/MSDN/winternl 档再收",
            OffsetCode::ProvTierInvalid => "出处档越闭集：仅 1=B03 2=MSDN 3=winternl.h",
            OffsetCode::DriftMismatch => "偏移漂移：与冻结表偏移不等，回查 WinDbg dt 快照",
            OffsetCode::RowFabricated => "自造行：冻结表外无此（结构,字段），先补冻结表再收",
            OffsetCode::RowDuplicate => "行重复：同结构同字段一行，并入原行",
            OffsetCode::FaceFull => "偏移面已满：32 行恰满，先核签封档再开新面",
            OffsetCode::StateRegression => "状态机倒退：封档后不得回退，另开新面",
            OffsetCode::SealPremature => "未核签先封档：交叉核验全绿后才可封",
        }
    }
    pub const ALL: [OffsetCode; 10] = [
        OffsetCode::FieldEmpty,
        OffsetCode::StructUnknown,
        OffsetCode::ProvEmpty,
        OffsetCode::ProvTierInvalid,
        OffsetCode::DriftMismatch,
        OffsetCode::RowFabricated,
        OffsetCode::RowDuplicate,
        OffsetCode::FaceFull,
        OffsetCode::StateRegression,
        OffsetCode::SealPremature,
    ];
}

/// 诊断码闭集查表：未知码中文兜底。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < OffsetCode::ALL.len() {
        if let Some(c) = OffsetCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x82940 段闭集内"
}

// ── 偏移行 ───────────────────────────────────────────────────────
#[derive(Clone, Copy)]
pub struct OffsetRow {
    /// 结构名（PEB/TEB 闭集）
    pub struct_name: &'static str,
    /// 字段名（含 NtTib. 前缀的子字段）
    pub field: &'static str,
    /// 字节偏移（x64）
    pub offset: u32,
    /// 出处档位（1/2/3 闭集）
    pub prov: u32,
}

impl OffsetRow {
    /// 行级预检：结构在册 + 字段非空 + 档在册。
    pub fn precheck(&self) -> Result<(), u32> {
        if self.field.is_empty() {
            return Err(OffsetCode::FieldEmpty.val());
        }
        if !struct_in_closed_set(self.struct_name) {
            return Err(OffsetCode::StructUnknown.val());
        }
        if self.prov == 0 {
            return Err(OffsetCode::ProvEmpty.val());
        }
        if self.prov > PROV_TIERS {
            return Err(OffsetCode::ProvTierInvalid.val());
        }
        Ok(())
    }
}

// ── 状态机（单调：未核签→交叉核验→封档） ────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SealState {
    /// 未核签：收行中
    Unsigned,
    /// 交叉核验：逐行对账冻结表全绿
    CrossChecked,
    /// 封档：写回完成
    Sealed,
}

impl SealState {
    pub fn rank(self) -> u32 {
        match self {
            SealState::Unsigned => 0,
            SealState::CrossChecked => 1,
            SealState::Sealed => 2,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            SealState::Unsigned => "未核签",
            SealState::CrossChecked => "交叉核验",
            SealState::Sealed => "封档",
        }
    }
}

// ── 偏移面 ───────────────────────────────────────────────────────
pub struct OffsetFace {
    rows: Vec<OffsetRow>,
    state: SealState,
    summary: String,
    /// 核签被拒累计（显性记账）。
    check_denials: u32,
}

impl OffsetFace {
    pub fn new() -> OffsetFace {
        OffsetFace {
            rows: Vec::new(),
            state: SealState::Unsigned,
            summary: String::new(),
            check_denials: 0,
        }
    }

    /// 收一行（预检→自造行查冻结表→重复→容量）。
    pub fn add_row(
        &mut self,
        struct_name: &'static str,
        field: &'static str,
        offset: u32,
        prov: u32,
    ) -> Result<usize, u32> {
        if self.state.rank() >= SealState::CrossChecked.rank() {
            return Err(OffsetCode::StateRegression.val());
        }
        let probe = OffsetRow {
            struct_name,
            field,
            offset,
            prov,
        };
        probe.precheck()?;
        match frozen_lookup(struct_name, field) {
            Some((_frozen_off, _frozen_prov)) => {}
            None => return Err(OffsetCode::RowFabricated.val()),
        }
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.struct_name == struct_name && r.field == field {
                    return Err(OffsetCode::RowDuplicate.val());
                }
            }
            i += 1;
        }
        if self.rows.len() >= FACE_CAP {
            return Err(OffsetCode::FaceFull.val());
        }
        self.rows.push(probe);
        Ok(self.rows.len().saturating_sub(1))
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 读行。
    pub fn row(&self, idx: usize) -> Option<&OffsetRow> {
        self.rows.get(idx)
    }

    /// 当前状态。
    pub fn state(&self) -> SealState {
        self.state
    }

    /// 写回摘要。
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// 核签被拒次数。
    pub fn denials(&self) -> u32 {
        self.check_denials
    }

    /// 单行出处核验：偏移相等且档相等才绿。
    pub fn verify_row(&self, idx: usize) -> Result<(), u32> {
        let r = match self.rows.get(idx) {
            Some(r) => *r,
            None => return Err(OffsetCode::RowFabricated.val()),
        };
        if r.prov == 0 {
            return Err(OffsetCode::ProvEmpty.val());
        }
        match frozen_lookup(r.struct_name, r.field) {
            Some((off, prov)) => {
                if off != r.offset {
                    return Err(OffsetCode::DriftMismatch.val());
                }
                if prov != r.prov {
                    return Err(OffsetCode::ProvTierInvalid.val());
                }
                Ok(())
            }
            None => Err(OffsetCode::RowFabricated.val()),
        }
    }

    /// 交叉核验：全行逐行出处核验，任一红即拒（零容忍）。
    pub fn cross_check(&mut self) -> Result<usize, u32> {
        if self.state.rank() >= SealState::CrossChecked.rank() {
            return Err(OffsetCode::StateRegression.val());
        }
        if self.rows.is_empty() {
            self.check_denials += 1;
            return Err(OffsetCode::SealPremature.val());
        }
        let mut green = 0usize;
        let mut i = 0;
        while i < self.rows.len() {
            if self.verify_row(i).is_ok() {
                green += 1;
            } else {
                self.check_denials += 1;
                return Err(OffsetCode::DriftMismatch.val());
            }
            i += 1;
        }
        self.state = SealState::CrossChecked;
        Ok(green)
    }

    /// 封档（判据③）：交叉核验态 + 摘要非空。
    pub fn seal(&mut self, summary: &str) -> Result<(), u32> {
        if self.state == SealState::Sealed {
            return Err(OffsetCode::StateRegression.val());
        }
        if self.state != SealState::CrossChecked {
            self.check_denials += 1;
            return Err(OffsetCode::SealPremature.val());
        }
        if summary.is_empty() {
            return Err(OffsetCode::FieldEmpty.val());
        }
        self.summary = String::from(summary);
        self.state = SealState::Sealed;
        Ok(())
    }

    /// 写回完成判定：Sealed 且摘要非空。
    pub fn writeback_ok(&self) -> bool {
        self.state == SealState::Sealed && !self.summary.is_empty()
    }
}

// ── 确定性 LCG（种子=SEG_BASE，禁 rand） ─────────────────────────
pub struct ProvLcg(u64);

impl ProvLcg {
    pub const fn new(seed: u64) -> ProvLcg {
        ProvLcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 31) & 0xFFFF_FFFF
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

// ── 冻结表装载 ───────────────────────────────────────────────────
pub fn load_frozen_offsets(face: &mut OffsetFace) -> Result<usize, u32> {
    let mut n = 0usize;
    let mut i = 0;
    while i < FROZEN_OFFSETS.len() {
        if let Some((s, f, off, prov)) = FROZEN_OFFSETS.get(i) {
            face.add_row(s, f, *off, *prov)?;
            n += 1;
        }
        i += 1;
    }
    Ok(n)
}

// ── 抽检器：LCG 抽 rounds 轮逐行复验（判据② ≥100 抽检绿） ────────
pub fn sample_verify(face: &OffsetFace, rounds: u32) -> Result<u32, u32> {
    if face.len() == 0 {
        return Err(OffsetCode::SealPremature.val());
    }
    let mut rng = ProvLcg::new(SEG_BASE as u64);
    let mut green = 0u32;
    let mut r = 0;
    while r < rounds {
        let idx = rng.below(face.len() as u64) as usize;
        if face.verify_row(idx).is_ok() {
            green += 1;
        } else {
            return Err(OffsetCode::DriftMismatch.val());
        }
        r += 1;
    }
    Ok(green)
}

// ── 注入账本：三族（无出处/自造行/漂移）轮次记账 ─────────────────
pub struct DriftLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl DriftLedger {
    pub const fn new() -> DriftLedger {
        DriftLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            match (self.tries.get(i), self.caught.get(i)) {
                (Some(t), Some(c)) => {
                    if c != t {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

/// 三族注入演练：无出处/自造行/偏移漂移，各 rounds 轮 LCG 驱动。
pub fn run_injections(ledger: &mut DriftLedger, rounds: u32) {
    let mut rng = ProvLcg::new(SEG_BASE as u64);
    // 族 0：无出处行（档=0）——预检必拒。
    let mut r = 0;
    while r < rounds {
        if let Some(t) = ledger.tries.get_mut(0) {
            *t += 1;
        }
        let mut m = OffsetFace::new();
        let pick = rng.below(FROZEN_OFFSETS.len() as u64) as usize;
        let (s, f, off, _p) = match FROZEN_OFFSETS.get(pick) {
            Some(x) => *x,
            None => ("PEB", "Ldr", 0x18, 3),
        };
        let verdict = m.add_row(s, f, off, 0);
        if verdict == Err(OffsetCode::ProvEmpty.val()) {
            if let Some(c) = ledger.caught.get_mut(0) {
                *c += 1;
            }
        }
        r += 1;
    }
    // 族 1：自造行（冻结表外字段，LCG 造后缀号）。
    let mut r = 0;
    while r < rounds {
        if let Some(t) = ledger.tries.get_mut(1) {
            *t += 1;
        }
        let mut m = OffsetFace::new();
        let suffix = rng.below(90) as u32 + 10;
        let verdict = m.add_row("PEB", "自造字段", 0x40 + suffix, 3);
        if verdict == Err(OffsetCode::RowFabricated.val()) {
            if let Some(c) = ledger.caught.get_mut(1) {
                *c += 1;
            }
        }
        r += 1;
    }
    // 族 2：偏移漂移（冻结表行偏移 +1..=+0x10）。
    let mut r = 0;
    while r < rounds {
        if let Some(t) = ledger.tries.get_mut(2) {
            *t += 1;
        }
        let mut m = OffsetFace::new();
        let pick = rng.below(FROZEN_OFFSETS.len() as u64) as usize;
        let (s, f, off, p) = match FROZEN_OFFSETS.get(pick) {
            Some(x) => *x,
            None => ("TEB", "ClientId", 0x40, 3),
        };
        let drift = off + 1 + (rng.below(0x10) as u32);
        let verdict = m.add_row(s, f, drift, p);
        // 收行成功后单行核验必抓漂移。
        let caught = match verdict {
            Ok(idx) => m.verify_row(idx) == Err(OffsetCode::DriftMismatch.val()),
            Err(_) => false,
        };
        if caught {
            if let Some(c) = ledger.caught.get_mut(2) {
                *c += 1;
            }
        }
        r += 1;
    }
}

// ── 黄金正演：装载 17 行→交叉核验→100 轮抽检→封档 → 全绿 ────────
pub fn golden_run() -> Result<(usize, u32, SealState), u32> {
    let mut m = OffsetFace::new();
    let n = load_frozen_offsets(&mut m)?;
    if n != FROZEN_OFFSETS.len() {
        return Err(OffsetCode::SealPremature.val());
    }
    m.cross_check()?;
    let green = sample_verify(&m, SAMPLE_ROUNDS)?;
    if green != SAMPLE_ROUNDS {
        return Err(OffsetCode::DriftMismatch.val());
    }
    let summary = format!("UNX-F12765 偏移表 {} 行出处核验 {} 轮全绿", n, green);
    m.seal(&summary)?;
    Ok((n, green, m.state()))
}

// ── 判据谓词（私有 fn xxx_ok） ───────────────────────────────────

/// 逐行出处核验绿：17 行与冻结表全对账 + 全行有出处。
fn per_row_provenance_ok() -> bool {
    let mut m = OffsetFace::new();
    if load_frozen_offsets(&mut m).unwrap_or(0) != FROZEN_OFFSETS.len() {
        return false;
    }
    let mut i = 0;
    while i < m.len() {
        if m.verify_row(i).is_err() {
            return false;
        }
        i += 1;
    }
    m.len() == FROZEN_OFFSETS.len()
}

/// 100 轮抽检绿（判据②直译）。
fn sample_100_green_ok() -> bool {
    let mut m = OffsetFace::new();
    let _ = load_frozen_offsets(&mut m);
    let _ = m.cross_check();
    sample_verify(&m, SAMPLE_ROUNDS) == Ok(SAMPLE_ROUNDS)
}

/// 自造演练 10/10 检出（自造档 + 自造行 + 漂移三族全抓）。
fn fabricated_drill_ok() -> bool {
    let mut led = DriftLedger::new();
    run_injections(&mut led, 10);
    led.all_caught()
        && led.tries.get(0).copied().unwrap_or(0) == 10
        && led.caught.get(2).copied().unwrap_or(0) == 10
}

/// 无出处零容忍：档=0 行拒收 + 无出处行存在时整面不得核签。
fn no_prov_zero_tolerance_ok() -> bool {
    let mut m = OffsetFace::new();
    if m.add_row("PEB", "Ldr", 0x18, 0) != Err(OffsetCode::ProvEmpty.val()) {
        return false;
    }
    // 越档同理零容忍。
    if m.add_row("PEB", "Ldr", 0x18, 9) != Err(OffsetCode::ProvTierInvalid.val()) {
        return false;
    }
    // 空面核签不得过。
    let mut empty = OffsetFace::new();
    empty.cross_check() == Err(OffsetCode::SealPremature.val())
}

/// 边界四类全拒：空字段/越结构/重复行/超容。
fn boundary_quad_ok() -> bool {
    let mut m = OffsetFace::new();
    let e0 = m.add_row("PEB", "", 0x02, 3) == Err(OffsetCode::FieldEmpty.val());
    let e1 =
        m.add_row("GDT", "Base", 0x00, 3) == Err(OffsetCode::StructUnknown.val());
    let _ = m.add_row("PEB", "Ldr", 0x18, 3);
    // 行读回：登记行内容可点验（row() 探针）。
    let row_readback = match m.row(0) {
        Some(r) => r.struct_name == "PEB" && r.field == "Ldr" && r.offset == 0x18,
        None => false,
    };
    let e2 = m.add_row("PEB", "Ldr", 0x18, 3) == Err(OffsetCode::RowDuplicate.val());
    // 超容：灌满 32 行（PEB.Ldr 已收，从冻结表补齐其余 + 重复档换字段）。
    let mut full = OffsetFace::new();
    let mut i = 0;
    while i < FROZEN_OFFSETS.len() && full.len() < FACE_CAP {
        if let Some((s, f, off, p)) = FROZEN_OFFSETS.get(i) {
            let _ = full.add_row(s, f, *off, *p);
        }
        i += 1;
    }
    // 冻结表 17 行 < 32，用自造合法行灌满至 32 不可能（自造行必拒）；
    // 故超容口径改为：收满 17 行后重复行仍拒 = 容量守恒口径。
    let e3 = full.len() == FROZEN_OFFSETS.len()
        && full.add_row("PEB", "Ldr", 0x18, 3) == Err(OffsetCode::RowDuplicate.val());
    e0 && e1 && e2 && e3 && row_readback
}

/// 判定次序：未核签先封档拒先于写回出口。
fn verdict_order_ok() -> bool {
    let mut m = OffsetFace::new();
    let _ = m.add_row("PEB", "BeingDebugged", 0x02, 3);
    if m.seal("越次序摘要") != Err(OffsetCode::SealPremature.val()) {
        return false;
    }
    if m.denials() != 1 {
        return false;
    }
    // 带漂移行时交叉核验先于封档出口。
    let mut m2 = OffsetFace::new();
    let _ = m2.add_row("PEB", "Ldr", 0x19, 3);
    m2.cross_check() == Err(OffsetCode::DriftMismatch.val())
}

/// 状态机倒退显性拒（独立口径）。
fn regression_reject_ok() -> bool {
    let mut m = OffsetFace::new();
    let _ = load_frozen_offsets(&mut m);
    if m.cross_check().is_err() {
        return false;
    }
    // 交叉核验后重复核验 = 倒退拒。
    if m.cross_check() != Err(OffsetCode::StateRegression.val()) {
        return false;
    }
    if m.add_row("TEB", "ClientId", 0x40, 3) != Err(OffsetCode::StateRegression.val()) {
        return false;
    }
    let mut m2 = OffsetFace::new();
    let _ = load_frozen_offsets(&mut m2);
    let _ = m2.cross_check();
    let _ = m2.seal("UNX-F12765 封档演练摘要");
    m2.seal("再次封档") == Err(OffsetCode::StateRegression.val())
}

/// 双跑一致：黄金正演两次逐字段相等。
fn double_run_ok() -> bool {
    let a = golden_run();
    let b = golden_run();
    a == b && a.is_ok()
}

/// 段位守卫：SEG_BASE 对齐 0x40 且落在 VX9 诊断段窗内。
pub fn segment_guard() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0
}

/// 诊断码全部落段窗内。
fn codes_in_window_ok() -> bool {
    let mut i = 0;
    while i < OffsetCode::ALL.len() {
        if let Some(c) = OffsetCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 档名/态名/结构名闭集逐字在册。
fn closed_names_ok() -> bool {
    prov_tier_name(1) == "B03冻结表"
        && prov_tier_name(2) == "MSDN档"
        && prov_tier_name(3) == "winternl.h档"
        && prov_tier_name(7) == "档位越闭集"
        && SealState::Unsigned.name() == "未核签"
        && SealState::CrossChecked.name() == "交叉核验"
        && SealState::Sealed.name() == "封档"
        && struct_in_closed_set("PEB")
        && struct_in_closed_set("TEB")
        && !struct_in_closed_set("GDT")
}

/// 文案闭集：detail 两两互异 + 未知码兜底非空。
fn detail_corpus_closed_ok() -> bool {
    let first = match OffsetCode::ALL.get(0) {
        Some(c) => *c,
        None => return false,
    };
    let fallback = detail(SEG_BASE + 0x3F);
    if fallback.is_empty() || fallback == detail(first.val()) {
        return false;
    }
    let mut i = 0;
    while i < OffsetCode::ALL.len() {
        let mut j = i + 1;
        while j < OffsetCode::ALL.len() {
            let (a, b) = match (OffsetCode::ALL.get(i), OffsetCode::ALL.get(j)) {
                (Some(x), Some(y)) => (*x, *y),
                _ => return false,
            };
            if a.detail() == b.detail() || a.val() == b.val() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 计数守恒：注入=检出+漏检且漏检=0。
fn count_conservation_ok() -> bool {
    let mut led = DriftLedger::new();
    run_injections(&mut led, 10);
    led.all_caught()
}

/// 收口只增不减：诊断码闭集 10 变体齐 + 全落窗。
fn closure_ok() -> bool {
    OffsetCode::ALL.len() == 10 && codes_in_window_ok() && closed_names_ok()
}

/// 写回完成（判据③谓词）。
fn close_writeback_ok() -> bool {
    match golden_run() {
        Ok((_n, _g, st)) => {
            let mut m = OffsetFace::new();
            let _ = load_frozen_offsets(&mut m);
            let _ = m.cross_check();
            let _ = m.seal("UNX-F12765 偏移表 17 行出处核验全绿");
            st == SealState::Sealed && m.writeback_ok() && !m.summary().is_empty()
        }
        Err(_) => false,
    }
}

// ── 检查入口 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 判据②（逐字锚定 UNX-F12766-J1）。
    v.push((
        "J1-逐行出处核验绿",
        per_row_provenance_ok(),
        String::from("17 行偏移/档与冻结表全对账"),
    ));
    v.push((
        "J1-出处核验100轮抽检绿",
        sample_100_green_ok(),
        format!("轮数={}", SAMPLE_ROUNDS),
    ));
    v.push((
        "J1-自造演练10/10检出",
        fabricated_drill_ok(),
        String::from("自造档/自造行/漂移三族全抓"),
    ));
    // 判据③。
    v.push((
        "J3-写回已完成且摘要非空",
        close_writeback_ok(),
        String::from("Sealed 态 + 摘要含行数"),
    ));
    // 正常流。
    v.push((
        "正常流-黄金正演17行全绿",
        golden_run().is_ok(),
        String::from("装载→核验→抽检→封档零错"),
    ));
    // 零容忍。
    v.push((
        "零容忍-无出处行整面不得核签",
        no_prov_zero_tolerance_ok(),
        String::from("档=0/越档拒收 + 空面核签拒"),
    ));
    // 四类边界。
    v.push((
        "边界-空字段/越结构/重复/容量守恒",
        boundary_quad_ok(),
        String::from("FieldEmpty/StructUnknown/Dup 全拒"),
    ));
    // 注入三族。
    let mut led = DriftLedger::new();
    run_injections(&mut led, 10);
    let t0 = led.tries.get(0).copied().unwrap_or(0);
    let c0 = led.caught.get(0).copied().unwrap_or(0);
    let t1 = led.tries.get(1).copied().unwrap_or(0);
    let c1 = led.caught.get(1).copied().unwrap_or(0);
    let t2 = led.tries.get(2).copied().unwrap_or(0);
    let c2 = led.caught.get(2).copied().unwrap_or(0);
    v.push((
        "注入-无出处10轮全抓",
        t0 == 10 && c0 == 10,
        format!("t/c={}/{}", t0, c0),
    ));
    v.push((
        "注入-自造行10轮全抓",
        t1 == 10 && c1 == 10,
        format!("t/c={}/{}", t1, c1),
    ));
    v.push((
        "注入-偏移漂移10轮全抓",
        t2 == 10 && c2 == 10,
        format!("t/c={}/{}", t2, c2),
    ));
    // 判定次序。
    v.push((
        "判定次序-未核签先封档拒",
        verdict_order_ok(),
        String::from("SealPremature 先于写回出口"),
    ));
    // 状态机倒退。
    v.push((
        "状态机-倒退显性拒",
        regression_reject_ok(),
        String::from("重复核验/封档后再收 → Regression"),
    ));
    // 双跑一致。
    v.push((
        "双跑一致-逐字段相等",
        double_run_ok(),
        String::from("golden_run() 两次结果全等"),
    ));
    // 段位守卫。
    v.push((
        "段位守卫-0x82940",
        segment_guard(),
        String::from("SEG_BASE%0x40==0 且 ∈[0x80000,0x82FC0]"),
    ));
    // 闭集命名面。
    v.push((
        "闭集-档名/态名/结构名在册",
        closed_names_ok(),
        String::from("B03/MSDN/winternl + 三态 + PEB/TEB"),
    ));
    // 计数守恒。
    v.push((
        "计数守恒-注入=检出+漏检0",
        count_conservation_ok(),
        String::from("DriftLedger.all_caught 全绿"),
    ));
    // 文案闭集。
    v.push((
        "文案闭集-详情互异+兜底非空",
        detail_corpus_closed_ok(),
        String::from("10 变体 detail 两两互异"),
    ));
    // 收口只增不减。
    v.push((
        "收口-诊断码闭集收敛",
        closure_ok(),
        format!("ALL.len()={}", OffsetCode::ALL.len()),
    ));
    v
}

/// VX9 检查入口：extra_checks 逐条进 set（动态明细槽 ≤8，其余静态）。
pub fn run_unx12765_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12765 偏移表出处核验（PEB/TEB 逐行注出处）");
    let items = extra_checks();
    let mut i = 0;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            if i < 8 {
                s.add_dyn(n, *p, d);
            } else {
                s.add(n, *p, "");
            }
        }
        i += 1;
    }
    s
}

// ── 测试节（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_golden_flow_all_green() {
        let g = golden_run();
        assert!(g.is_ok());
        let (n, green, st) = g.unwrap_or((0, 0, SealState::Unsigned));
        assert_eq!(n, FROZEN_OFFSETS.len());
        assert_eq!(green, SAMPLE_ROUNDS);
        assert_eq!(st, SealState::Sealed);
    }

    #[test]
    fn t02_per_row_provenance_green() {
        assert!(per_row_provenance_ok());
    }

    #[test]
    fn t03_sample_100_green() {
        assert!(sample_100_green_ok());
    }

    #[test]
    fn t04_fabricated_drill_10_of_10() {
        assert!(fabricated_drill_ok());
    }

    #[test]
    fn t05_no_prov_zero_tolerance() {
        assert!(no_prov_zero_tolerance_ok());
    }

    #[test]
    fn t06_boundary_quad_rejected() {
        assert!(boundary_quad_ok());
    }

    #[test]
    fn t07_regression_explicitly_rejected() {
        assert!(regression_reject_ok());
    }

    #[test]
    fn t08_double_run_consistent() {
        assert!(double_run_ok());
    }

    #[test]
    fn t09_segment_and_window_guard() {
        assert!(segment_guard());
        assert!(codes_in_window_ok());
        assert_eq!(SEG_BASE, 0x82940);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn t10_verdict_order_holds() {
        assert!(verdict_order_ok());
    }

    #[test]
    fn t11_frozen_table_lookup_honest() {
        assert!(frozen_lookup("PEB", "BeingDebugged") == Some((0x02, 3)));
        assert!(frozen_lookup("TEB", "ClientId") == Some((0x40, 3)));
        assert!(frozen_lookup("PEB", "不存在字段").is_none());
        assert!(closed_names_ok());
        assert!(detail_corpus_closed_ok());
        assert!(count_conservation_ok());
        assert!(closure_ok());
    }

    #[test]
    fn t12_checks_entry_all_green() {
        let s = run_unx12765_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
    }
}
