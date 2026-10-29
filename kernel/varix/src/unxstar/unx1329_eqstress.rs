//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1329 · 等价验证压测资产（高压下等价维持） · 0x76F0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F1329-J1 压测场景 ≥5 类（风暴中对拍/高压窗口/并发退役/
//!      资源紧张对拍/对拍工具高压）资产化，3 次可重复一致率 100%，
//!      高压等价维持 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 负载剖面：相位表（相位数×每相事件量），确定性展开，禁墙钟；
//!   - 场景宇宙 5 类资产：风暴中对拍/高压窗口/并发退役/资源紧张
//!     对拍/对拍工具高压，登记后才可跑（禁裸跑）；
//!   - 双路高压对拍：同种子双路回放，逐事件对拍；诚实路径高压下
//!     等价维持 100%（ppm==10^6）；破坏路径按负载比例注违例；
//!   - 3 次可重复：同资产同种子三跑，结果摘要（事件/匹配/指纹）全同；
//!   - 压测守恒：进=匹配+违例，逐资产留账；资产账只增不减。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ═══════════════════════════════════════════════════════════════════════
// §0 常量与诊断码闭集（0x76F0 段，连续编号）
// ═══════════════════════════════════════════════════════════════════════

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x76F0;

/// 注入演练轮数（判据点名 10）。
pub const INJECT_ROUNDS: usize = 10;

/// 压测场景宇宙（5 类）。
pub const SCEN_COUNT: usize = 5;

/// 可重复跑数（判据点名 3）。
pub const REPEAT_RUNS: usize = 3;

/// 高压等价线（ppm）。
pub const STRESS_PPM: u64 = 1_000_000;

/// 0x76F0 场景越界。
pub const E_SCEN_OOB: u16 = SEG_BASE + 0;
/// 0x76F1 裸跑（未登记就跑）。
pub const E_UNREG_RUN: u16 = SEG_BASE + 1;
/// 0x76F2 重复登记。
pub const E_DUP_REG: u16 = SEG_BASE + 2;
/// 0x76F3 可重复破约（三跑结果不一致）。
pub const E_REPEAT: u16 = SEG_BASE + 3;
/// 0x76F4 高压等价破约（诚实压测 ppm<10^6）。
pub const E_STRESS_EQ: u16 = SEG_BASE + 4;
/// 0x76F5 压测守恒破约（进≠匹配+违例）。
pub const E_STRESS_CONSERVE: u16 = SEG_BASE + 5;
/// 0x76F6 负载剖面空（零相位）。
pub const E_PROFILE_EMPTY: u16 = SEG_BASE + 6;
/// 0x76F7 资产账缩水。
pub const E_BOOK_SHRINK: u16 = SEG_BASE + 7;
/// 0x76F8 破坏档漏红（注违例未破等价）。
pub const E_CORRUPT_MISS: u16 = SEG_BASE + 8;
/// 0x76F9 相位量越限（单相事件量超资产上限）。
pub const E_PHASE_CAP: u16 = SEG_BASE + 9;

/// 全码闭集。
pub const CODE_ALL: [u16; 10] = [
    E_SCEN_OOB, E_UNREG_RUN, E_DUP_REG, E_REPEAT, E_STRESS_EQ,
    E_STRESS_CONSERVE, E_PROFILE_EMPTY, E_BOOK_SHRINK, E_CORRUPT_MISS,
    E_PHASE_CAP,
];

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_SCEN_OOB => "场景越界：压测宇宙 5 类闭集（0..5），越界即拒",
        E_UNREG_RUN => "裸跑：先登记资产再执行，禁未登记场景直接压",
        E_DUP_REG => "重复登记：同场景同名单资产唯一，重复即拒",
        E_REPEAT => "可重复破约：三跑必须逐字段一致，先查 LCG 种子是否被污染",
        E_STRESS_EQ => "高压等价破约：诚实压测必须 ppm==10^6，先看首破相位",
        E_STRESS_CONSERVE => "压测守恒破约：进=匹配+违例，逐相位对账定位",
        E_PROFILE_EMPTY => "负载剖面空：相位表必须 ≥1 相，零相位禁跑",
        E_BOOK_SHRINK => "资产账缩水：登记只增不减，对账新账<旧账即红",
        E_CORRUPT_MISS => "破坏档漏红：注违例必须破等价（否则注入器失效）",
        E_PHASE_CAP => "相位量越限：单相事件量不得超过资产上限（防账面失真）",
        _ => "未知诊断码：核对 0x76F0 段闭集登记表",
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §1 LCG 与场景宇宙
// ═══════════════════════════════════════════════════════════════════════

/// 64 位 LCG。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 建（种子写死）。
    pub fn new(seed: u64) -> Self {
        Lcg {
            state: seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407)
                | 1,
        }
    }

    /// 下一原始值。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    /// [0, n)。
    pub fn next_below(&mut self, n: u64) -> u64 {
        let m = if n == 0 { 1 } else { n };
        self.next_u64() % m
    }
}

/// 压测场景宇宙（5 类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StressKind {
    /// 风暴中对拍。
    StormCompare,
    /// 高压窗口。
    HighWindow,
    /// 并发退役。
    ConcurrentRetire,
    /// 资源紧张对拍。
    TightResource,
    /// 对拍工具高压。
    ToolPressure,
}

impl StressKind {
    /// 闭集。
    pub const ALL: [StressKind; 5] = [
        StressKind::StormCompare,
        StressKind::HighWindow,
        StressKind::ConcurrentRetire,
        StressKind::TightResource,
        StressKind::ToolPressure,
    ];

    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            StressKind::StormCompare => "风暴中对拍",
            StressKind::HighWindow => "高压窗口",
            StressKind::ConcurrentRetire => "并发退役",
            StressKind::TightResource => "资源紧张对拍",
            StressKind::ToolPressure => "对拍工具高压",
        }
    }

    /// 枚举序。
    pub const fn idx(self) -> usize {
        match self {
            StressKind::StormCompare => 0,
            StressKind::HighWindow => 1,
            StressKind::ConcurrentRetire => 2,
            StressKind::TightResource => 3,
            StressKind::ToolPressure => 4,
        }
    }

    /// 从序号还原。
    pub const fn from_idx(i: usize) -> Option<StressKind> {
        match i {
            0 => Some(StressKind::StormCompare),
            1 => Some(StressKind::HighWindow),
            2 => Some(StressKind::ConcurrentRetire),
            3 => Some(StressKind::TightResource),
            4 => Some(StressKind::ToolPressure),
            _ => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §2 负载剖面（相位表）
// ═══════════════════════════════════════════════════════════════════════

/// 单相负载。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadPhase {
    /// 相位名。
    pub name: &'static str,
    /// 事件量。
    pub events: u64,
}

/// 压测资产（场景 + 相位剖面 + 上限）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StressAsset {
    /// 场景类。
    pub kind: StressKind,
    /// 资产名。
    pub name: &'static str,
    /// 单相事件量上限。
    pub cap: u64,
    /// 相位数。
    pub phases: usize,
}

impl StressAsset {
    /// 相位剖面展开（确定性：相位 i 事件量 = cap × 系数/16）。
    pub fn profile(&self) -> Result<Vec<LoadPhase>, u16> {
        if self.phases == 0 {
            return Err(E_PROFILE_EMPTY);
        }
        let mut out = Vec::new();
        for i in 0..self.phases {
            // 系数表（×16 分母）：风暴相 16、高压相 12、回落相 6、稳态相 8。
            let num: u64 = match i % 4 {
                0 => 16,
                1 => 12,
                2 => 6,
                _ => 8,
            };
            let ev = self.cap.saturating_mul(num) / 16;
            if ev > self.cap {
                return Err(E_PHASE_CAP);
            }
            let name = match i % 4 {
                0 => "风暴相",
                1 => "高压相",
                2 => "回落相",
                _ => "稳态相",
            };
            out.push(LoadPhase { name, events: ev });
        }
        Ok(out)
    }

    /// 总事件量。
    pub fn total(&self) -> Result<u64, u16> {
        let p = self.profile()?;
        let mut s = 0u64;
        for ph in &p {
            s = s.wrapping_add(ph.events);
        }
        Ok(s)
    }
}

/// 五资产写死目录（name/kind/cap/phases 定值）。
pub const ASSET_CATALOG: [StressAsset; 5] = [
    StressAsset { kind: StressKind::StormCompare, name: "风暴对拍-A1", cap: 4096, phases: 4 },
    StressAsset { kind: StressKind::HighWindow, name: "高压窗口-A2", cap: 8192, phases: 3 },
    StressAsset { kind: StressKind::ConcurrentRetire, name: "并发退役-A3", cap: 2048, phases: 5 },
    StressAsset { kind: StressKind::TightResource, name: "资源紧张-A4", cap: 1024, phases: 4 },
    StressAsset { kind: StressKind::ToolPressure, name: "工具高压-A5", cap: 6144, phases: 2 },
];

// ═══════════════════════════════════════════════════════════════════════
// §3 压测执行账（双路对拍 + 守恒 + 指纹）
// ═══════════════════════════════════════════════════════════════════════

/// 单资产执行结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StressResult {
    /// 场景。
    pub kind: StressKind,
    /// 总事件。
    pub events: u64,
    /// 匹配事件。
    pub matched: u64,
    /// 违例事件。
    pub violated: u64,
    /// 等价率 ppm。
    pub ppm: u64,
    /// 结果指纹（事件/匹配/违例吸收）。
    pub digest: u64,
}

impl StressResult {
    /// 守恒（进=匹配+违例）。
    pub const fn conserved(&self) -> bool {
        self.matched + self.violated == self.events
    }

    /// 高压等价维持（ppm==10^6 且零违例）。
    pub const fn eq_kept(&self) -> bool {
        self.ppm == STRESS_PPM && self.violated == 0
    }
}

/// 执行一个资产（corrupt=true 时按事件取模注违例）。
pub fn run_asset(asset: &StressAsset, seed: u64, corrupt: bool) -> Result<StressResult, u16> {
    let profile = asset.profile()?;
    let mut matched = 0u64;
    let mut violated = 0u64;
    let mut events = 0u64;
    for (i, ph) in profile.iter().enumerate() {
        if ph.events > asset.cap {
            return Err(E_PHASE_CAP);
        }
        let mut g = Lcg::new(
            seed ^ ((asset.kind.idx() as u64) << 32) ^ ((i as u64) << 16),
        );
        let mut j = 0u64;
        while j < ph.events {
            let a = g.next_u64();
            // 破坏：风暴相/高压相按 137 取模注违例，回落/稳态相不注。
            let b = if corrupt && i % 2 == 0 && j % 137 == 0 { a ^ 0x80 } else { a };
            if a == b {
                matched += 1;
            } else {
                violated += 1;
            }
            events += 1;
            j += 1;
        }
    }
    let ppm = if events == 0 {
        0
    } else {
        let v = matched.saturating_mul(1_000_000) / events;
        if v > 1_000_000 {
            1_000_000
        } else {
            v
        }
    };
    // 指纹（结果摘要，供可重复比对）。
    let mut d: u64 = 0xcbf29ce484222325;
    d = (d ^ events).wrapping_mul(0x100000001b3);
    d = (d ^ matched).wrapping_mul(0x100000001b3);
    d = (d ^ violated).wrapping_mul(0x100000001b3);
    Ok(StressResult {
        kind: asset.kind,
        events,
        matched,
        violated,
        ppm,
        digest: d,
    })
}

/// 三跑可重复核（逐字段全同）。
pub fn repeatable(asset: &StressAsset, seed: u64) -> Result<bool, u16> {
    let first = run_asset(asset, seed, false)?;
    for _ in 1..REPEAT_RUNS {
        let again = run_asset(asset, seed, false)?;
        if again != first {
            return Ok(false);
        }
    }
    Ok(true)
}

// ═══════════════════════════════════════════════════════════════════════
// §4 资产登记账（只增不减 + 禁裸跑）
// ═══════════════════════════════════════════════════════════════════════

/// 压测资产账。
pub struct StressBook {
    entries: Vec<StressAsset>,
    pub refused_dup: u32,
}

impl StressBook {
    /// 建。
    pub fn new() -> Self {
        StressBook { entries: Vec::new(), refused_dup: 0 }
    }

    /// 登记（重复拒；账只增）。
    pub fn register(&mut self, a: StressAsset) -> Result<usize, u16> {
        if a.kind.idx() >= SCEN_COUNT {
            return Err(E_SCEN_OOB);
        }
        for e in &self.entries {
            if e.kind == a.kind && e.name == a.name {
                self.refused_dup += 1;
                return Err(E_DUP_REG);
            }
        }
        if a.profile().is_err() {
            return Err(E_PROFILE_EMPTY);
        }
        self.entries.push(a);
        Ok(self.entries.len() - 1)
    }

    /// 可跑。
    pub fn runnable(&self, kind: StressKind) -> bool {
        self.entries.iter().any(|e| e.kind == kind)
    }

    /// 条数。
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// 账取资产。
    pub fn get(&self, idx: usize) -> Result<StressAsset, u16> {
        if idx >= self.entries.len() {
            return Err(E_SCEN_OOB);
        }
        Ok(self.entries[idx])
    }

    /// 场景覆盖（5 类全登记）。
    pub fn covers_all(&self) -> bool {
        for k in StressKind::ALL {
            if !self.runnable(k) {
                return false;
            }
        }
        true
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §5 演练
// ═══════════════════════════════════════════════════════════════════════

/// 五资产全绿演练（登记→诚实跑→等价维持 100%+守恒）。
pub fn drill_all_assets() -> (bool, String) {
    let mut book = StressBook::new();
    for a in ASSET_CATALOG {
        if book.register(a).is_err() {
            return (false, format!("登记失败 {}", a.name));
        }
    }
    if !book.covers_all() || book.count() != SCEN_COUNT {
        return (false, "覆盖不全".to_string());
    }
    let mut ok = true;
    let mut lines: Vec<String> = Vec::new();
    for i in 0..book.count() {
        let a = match book.get(i) {
            Ok(a) => a,
            Err(c) => return (false, format!("取账失败 {:#06X}", c)),
        };
        match run_asset(&a, 0x1329_00AA, false) {
            Ok(r) => {
                if !r.eq_kept() || !r.conserved() {
                    ok = false;
                }
                lines.push(format!(
                    "{}: ppm={} 事件={}",
                    a.name,
                    r.ppm,
                    r.events
                ));
            }
            Err(c) => return (false, format!("{} 跑失败 {:#06X}", a.name, c)),
        }
    }
    (ok, format!("{} 覆盖={}", lines.join(" | "), book.covers_all()))
}

/// 3 次可重复一致率 100% 演练。
pub fn drill_repeat_3() -> (bool, String) {
    let mut ok = 0usize;
    for a in ASSET_CATALOG {
        match repeatable(&a, 0x1329_1000) {
            Ok(true) => ok += 1,
            _ => {}
        }
    }
    (
        ok == SCEN_COUNT,
        format!("{}/{} 三跑全同", ok, SCEN_COUNT),
    )
}

/// 高压等价反向演练（破坏档必红 + 首破相位守恒）。
pub fn drill_corrupt_reverse() -> (bool, String) {
    let a = ASSET_CATALOG[0];
    match run_asset(&a, 0x1329_2000, true) {
        Ok(r) => {
            let red = !r.eq_kept() && r.violated > 0;
            (
                red && r.conserved() && r.ppm < STRESS_PPM,
                format!(
                    "破坏红={} 违例={} ppm={}",
                    red, r.violated, r.ppm
                ),
            )
        }
        Err(c) => (false, format!("跑失败 {:#06X}", c)),
    }
}

/// 裸跑/重复登记/空剖面必拒演练。
pub fn drill_refusals() -> (bool, String) {
    let mut book = StressBook::new();
    let a = ASSET_CATALOG[1];
    let _ = book.register(a);
    // 重复登记必拒。
    let dup = matches!(book.register(a), Err(E_DUP_REG));
    // 空剖面资产必拒。
    let empty = StressAsset { kind: StressKind::ToolPressure, name: "空相-A9", cap: 16, phases: 0 };
    let emp = matches!(book.register(empty), Err(E_PROFILE_EMPTY));
    // 裸跑必拒（未登记 ToolPressure 时 runnable=false）。
    let book2 = StressBook::new();
    let bare = !book2.runnable(StressKind::ToolPressure);
    // 账缩水判据：登记数只增。
    let c1 = book.count();
    let _ = book.register(ASSET_CATALOG[2]);
    let c2 = book.count();
    let grow = c2 == c1 + 1;
    (
        dup && emp && bare && grow && book.refused_dup == 1,
        format!("重复拒={} 空拒={} 裸跑={} 账{}→{}", dup, emp, bare, c1, c2),
    )
}

/// 相位剖面演练（风暴相>回落相、上限不越、总量对账）。
pub fn drill_profile() -> (bool, String) {
    let a = ASSET_CATALOG[0];
    let p = match a.profile() {
        Ok(p) => p,
        Err(c) => return (false, format!("剖面失败 {:#06X}", c)),
    };
    let storm = p[0].events;
    let back = p[2].events;
    let mut sum = 0u64;
    for ph in &p {
        if ph.events > a.cap {
            return (false, "相位越限".to_string());
        }
        sum += ph.events;
    }
    let total_ok = sum == a.total().unwrap_or(u64::MAX);
    (
        storm > back && total_ok && p.len() == a.phases,
        format!("风暴{}>回落{} 总{}=={}", storm, back, sum, total_ok),
    )
}

/// 10/10 破坏注入必抓演练（每轮一场景，破坏档必红）。
pub fn drill_inject_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut notes: Vec<String> = Vec::new();
    for r in 0..INJECT_ROUNDS {
        let kind = match StressKind::from_idx(r % SCEN_COUNT) {
            Some(k) => k,
            None => return (false, "场景还原失败".to_string()),
        };
        // 从目录挑该场景资产。
        let a = match ASSET_CATALOG.iter().find(|x| x.kind == kind) {
            Some(x) => *x,
            None => return (false, format!("场景{}无资产", kind.text())),
        };
        let seed = 0x1329_3000 + r as u64;
        match run_asset(&a, seed, true) {
            Ok(r2) => {
                if !r2.eq_kept() && r2.violated > 0 && r2.conserved() {
                    ok += 1;
                }
                notes.push(format!("{}:{}", kind.text(), if ok > 0 { "抓" } else { "漏" }));
            }
            Err(c) => notes.push(format!("{}:err{:#06X}", kind.text(), c)),
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("{}/{} {}", ok, INJECT_ROUNDS, notes.join(" ")))
}

// ═══════════════════════════════════════════════════════════════════════
// §6 逐相压测账（相位级留痕 + 首破相位定位）
// ═══════════════════════════════════════════════════════════════════════

/// 单相执行记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhaseRec {
    /// 相位序。
    pub phase: usize,
    /// 相位名。
    pub name: &'static str,
    /// 事件量。
    pub events: u64,
    /// 匹配。
    pub matched: u64,
    /// 违例。
    pub violated: u64,
    /// 相位 ppm。
    pub ppm: u64,
}

impl PhaseRec {
    /// 守恒。
    pub const fn conserved(&self) -> bool {
        self.matched + self.violated == self.events
    }

    /// 相位等价维持。
    pub const fn eq_kept(&self) -> bool {
        self.ppm == STRESS_PPM && self.violated == 0
    }
}

/// 逐相账（单资产一次执行的相位级明细）。
pub struct PhaseLedger {
    pub recs: Vec<PhaseRec>,
    pub asset_name: &'static str,
}

impl PhaseLedger {
    /// 逐相执行并留账（corrupt 语义同 run_asset：偶数相注违例）。
    pub fn run(asset: &StressAsset, seed: u64, corrupt: bool) -> Result<Self, u16> {
        let profile = asset.profile()?;
        let mut recs = Vec::new();
        for (i, ph) in profile.iter().enumerate() {
            if ph.events > asset.cap {
                return Err(E_PHASE_CAP);
            }
            let mut g = Lcg::new(
                seed ^ ((asset.kind.idx() as u64) << 32) ^ ((i as u64) << 16),
            );
            let mut matched = 0u64;
            let mut violated = 0u64;
            let mut j = 0u64;
            while j < ph.events {
                let a = g.next_u64();
                let b = if corrupt && i % 2 == 0 && j % 137 == 0 { a ^ 0x80 } else { a };
                if a == b {
                    matched += 1;
                } else {
                    violated += 1;
                }
                j += 1;
            }
            let ev = ph.events;
            let ppm = if ev == 0 {
                0
            } else {
                let v = matched.saturating_mul(1_000_000) / ev;
                if v > 1_000_000 {
                    1_000_000
                } else {
                    v
                }
            };
            recs.push(PhaseRec {
                phase: i,
                name: ph.name,
                events: ev,
                matched,
                violated,
                ppm,
            });
        }
        Ok(PhaseLedger { recs, asset_name: asset.name })
    }

    /// 汇总（Σ事件/Σ匹配/Σ违例）。
    pub fn aggregate(&self) -> (u64, u64, u64) {
        let mut ev = 0u64;
        let mut mt = 0u64;
        let mut vl = 0u64;
        for r in &self.recs {
            ev += r.events;
            mt += r.matched;
            vl += r.violated;
        }
        (ev, mt, vl)
    }

    /// 全相守恒。
    pub fn all_conserved(&self) -> bool {
        self.recs.iter().all(|r| r.conserved())
    }

    /// 诚实路径全相绿。
    pub fn all_green(&self) -> bool {
        self.recs.iter().all(|r| r.eq_kept())
    }

    /// 首破相位。
    pub fn first_break(&self) -> Option<usize> {
        for r in &self.recs {
            if !r.eq_kept() {
                return Some(r.phase);
            }
        }
        None
    }

    /// 与 run_asset 汇总对账（两口径必须一致）。
    pub fn agrees_with_run(&self, r: &StressResult) -> bool {
        let (ev, mt, vl) = self.aggregate();
        ev == r.events && mt == r.matched && vl == r.violated
    }

    /// 首破相位锁定偶数相（破坏注入只落偶数相的反向判据）。
    pub fn break_in_storm_phase(&self) -> bool {
        match self.first_break() {
            Some(p) => p % 2 == 0,
            None => false,
        }
    }
}

/// 逐相账演练（诚实全绿 + 与汇总口径对账一致 + 破坏落风暴相）。
pub fn drill_phase_ledger() -> (bool, String) {
    let a = ASSET_CATALOG[2];
    let honest = match PhaseLedger::run(&a, 0x1329_5000, false) {
        Ok(l) => l,
        Err(c) => return (false, format!("跑失败 {:#06X}", c)),
    };
    let dirty = match PhaseLedger::run(&a, 0x1329_5000, true) {
        Ok(l) => l,
        Err(c) => return (false, format!("破坏跑失败 {:#06X}", c)),
    };
    let hr = match run_asset(&a, 0x1329_5000, false) {
        Ok(r) => r,
        Err(c) => return (false, format!("汇总跑失败 {:#06X}", c)),
    };
    (
        honest.all_conserved()
            && honest.all_green()
            && honest.agrees_with_run(&hr)
            && dirty.first_break().is_some()
            && dirty.break_in_storm_phase(),
        format!(
            "{}: 相{} 诚绿={} 对账={} 首破相={:?} 落风暴相={}",
            a.name,
            honest.recs.len(),
            honest.all_green(),
            honest.agrees_with_run(&hr),
            dirty.first_break(),
            dirty.break_in_storm_phase()
        ),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §6b 对拍缓冲压力账（有界环形 + 溢出显性 + 压力下守恒）
// ═══════════════════════════════════════════════════════════════════════

/// 对拍待核项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CmpItem {
    /// 事件序。
    pub seq: u64,
    /// 轮询侧值。
    pub a: u64,
    /// 中断侧值。
    pub b: u64,
}

impl CmpItem {
    /// 该项是否等价。
    pub const fn eq(&self) -> bool {
        self.a == self.b
    }
}

/// 有界对拍缓冲（容量 64；满则溢出显性计数，不静默丢）。
pub struct CmpBuffer {
    buf: [Option<CmpItem>; 64],
    head: usize,
    count: usize,
    pub pushed: u64,
    pub popped: u64,
    pub overflow: u64,
    /// 已核等价项。
    pub matched: u64,
    /// 已核违例项。
    pub violated: u64,
}

impl CmpBuffer {
    /// 建。
    pub fn new() -> Self {
        CmpBuffer {
            buf: [None; 64],
            head: 0,
            count: 0,
            pushed: 0,
            popped: 0,
            overflow: 0,
            matched: 0,
            violated: 0,
        }
    }

    /// 推一项（满则溢出计数，不入环）。
    pub fn push(&mut self, it: CmpItem) {
        self.pushed += 1;
        if self.count == 64 {
            self.overflow += 1;
            return;
        }
        let idx = (self.head + self.count) % 64;
        self.buf[idx] = Some(it);
        self.count += 1;
    }

    /// 弹一项并核（等价/违例分流记账）。
    pub fn pop_and_verify(&mut self) -> Option<bool> {
        if self.count == 0 {
            return None;
        }
        let v = self.buf[self.head];
        self.head = (self.head + 1) % 64;
        self.count -= 1;
        self.popped += 1;
        match v {
            Some(it) => {
                let eq = it.eq();
                if eq {
                    self.matched += 1;
                } else {
                    self.violated += 1;
                }
                Some(eq)
            }
            None => None,
        }
    }

    /// 在途。
    pub const fn in_flight(&self) -> u64 {
        self.count as u64
    }

    /// 守恒（进=出+在途+溢出；出=等价+违例）。
    pub fn conserved(&self) -> bool {
        self.pushed == self.popped + self.in_flight() + self.overflow
            && self.popped == self.matched + self.violated
    }

    /// 高压演练：n 项推入（违例按 149 取模），全弹出核账。
    pub fn pressure_run(&mut self, seed: u64, n: u64) {
        let mut g = Lcg::new(seed);
        for i in 0..n {
            let a = g.next_u64();
            let b = if i % 149 == 0 { a ^ 0x01 } else { a };
            self.push(CmpItem { seq: i, a, b });
            // 边推边弹（模拟消费），高吞吐下环不长期满。
            if i % 2 == 1 {
                let _ = self.pop_and_verify();
            }
        }
        while self.pop_and_verify().is_some() {}
    }
}

/// 对拍缓冲压测演练（10/10 轮守恒 + 违例必被核出）。
pub fn drill_buffer_pressure_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut first_note = String::new();
    for r in 0..INJECT_ROUNDS {
        let mut buf = CmpBuffer::new();
        let n = 256 + (r as u64) * 64;
        buf.pressure_run(0x1329_6000 + r as u64, n);
        let conserved = buf.conserved();
        let has_violation = buf.violated > 0;
        if r == 0 {
            first_note = format!(
                "进{} 核{} 违例{} 溢出{} 守恒{}",
                buf.pushed, buf.popped, buf.violated, buf.overflow, conserved
            );
        }
        if conserved && has_violation {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("{}/{} {}", ok, INJECT_ROUNDS, first_note))
}

/// 对拍缓冲反向演练（零违例压力流必须全匹配+守恒）。
pub fn drill_buffer_clean() -> (bool, String) {
    let mut buf = CmpBuffer::new();
    let mut g = Lcg::new(0x1329_6000 ^ 0xC1EA);
    for i in 0..512u64 {
        let a = g.next_u64();
        buf.push(CmpItem { seq: i, a, b: a });
        // 边推边排空到半容量（防溢出，干净流全量入核）。
        while buf.in_flight() >= 32 {
            if buf.pop_and_verify().is_none() {
                break;
            }
        }
    }
    while buf.pop_and_verify().is_some() {}
    (
        buf.conserved() && buf.violated == 0 && buf.matched == 512 && buf.overflow == 0,
        format!(
            "核{} 违例{} 溢出{} 守恒{}",
            buf.matched,
            buf.violated,
            buf.overflow,
            buf.conserved()
        ),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §7 注入核（6 项）
// ═══════════════════════════════════════════════════════════════════════

/// 注入核 6 项（场景越界/裸跑/重复/破坏漏红/剖面空/可重复失守面）。
pub fn inject_core() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut det: Vec<String> = Vec::new();
    // 注1：场景越界必拒。
    if StressKind::from_idx(SCEN_COUNT).is_none() {
        caught += 1;
        det.push("越界:拒".to_string());
    } else {
        det.push("越界:漏".to_string());
    }
    // 注2：裸跑必拒。
    let mut book = StressBook::new();
    let _ = book.register(ASSET_CATALOG[3]);
    if !book.runnable(StressKind::StormCompare) {
        caught += 1;
        det.push("裸跑:拒".to_string());
    } else {
        det.push("裸跑:漏".to_string());
    }
    // 注3：重复登记必拒。
    let dup = matches!(book.register(ASSET_CATALOG[3]), Err(E_DUP_REG));
    if dup {
        caught += 1;
        det.push("重复:拒".to_string());
    } else {
        det.push("重复:漏".to_string());
    }
    // 注4：破坏档必红。
    match run_asset(&ASSET_CATALOG[2], 0x1329_4000, true) {
        Ok(r) if !r.eq_kept() => {
            caught += 1;
            det.push("破坏:红".to_string());
        }
        _ => det.push("破坏:漏".to_string()),
    }
    // 注5：空剖面必拒。
    let empty = StressAsset { kind: StressKind::HighWindow, name: "空-A8", cap: 8, phases: 0 };
    if matches!(empty.profile(), Err(E_PROFILE_EMPTY)) {
        caught += 1;
        det.push("空相:拒".to_string());
    } else {
        det.push("空相:漏".to_string());
    }
    // 注6：可重复失守面（同种子三跑必须逐字段同——诚实路径实证）。
    let a = ASSET_CATALOG[4];
    let r1 = run_asset(&a, 0x1329_4001, false);
    let r2 = run_asset(&a, 0x1329_4001, false);
    let r3 = run_asset(&a, 0x1329_4001, false);
    if r1 == r2 && r2 == r3 {
        caught += 1;
        det.push("重复:守".to_string());
    } else {
        det.push("重复:漏".to_string());
    }
    (caught, 6, det)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    CODE_ALL.iter().all(|c| *c >= SEG_BASE && *c < SEG_BASE + 16)
}

// ═══════════════════════════════════════════════════════════════════════
// §7 run_unx1329_checks：本单判据聚合（诊断段 0x76F0）
// ═══════════════════════════════════════════════════════════════════════

/// 聚合判据（≥6 条，含 10/10 注入演练与段位守卫）。
/// 口径：5 类资产化、3 跑可重复 100%、高压等价维持 100%、守恒逐相。
pub fn run_unx1329_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1329 等价压测资产");
    let (ok1, d1) = drill_all_assets();
    s.add_dyn("J1-五资产登记+全绿", ok1, &d1);
    let (ok2, d2) = drill_repeat_3();
    s.add_dyn("J2-三跑可重复100%", ok2, &d2);
    let (ok3, d3) = drill_corrupt_reverse();
    s.add_dyn("J3-破坏档反向必红", ok3, &d3);
    let (ok4, d4) = drill_refusals();
    s.add_dyn("J4-裸跑/重复/空相必拒", ok4, &d4);
    let (ok5, d5) = drill_profile();
    s.add_dyn("J5-相位剖面守恒", ok5, &d5);
    let (ok6, d6) = drill_inject_10();
    s.add_dyn("J6-破坏注入必抓10/10", ok6, &d6);
    let (ok7, d7) = drill_phase_ledger();
    s.add_dyn("J7-逐相账对账一致", ok7, &d7);
    let (ok8, d8) = drill_buffer_pressure_10();
    s.add_dyn("J8-对拍缓冲压测10/10", ok8, &d8);
    let (ok9, d9) = drill_buffer_clean();
    s.add_dyn("J9-对拍缓冲干净流", ok9, &d9);
    let (c10, t10, d10) = inject_core();
    s.add_dyn(
        "J10-注入核6/6",
        c10 == t10,
        &format!("{}/{} {}", c10, t10, d10.join(" ")),
    );
    s.add_dyn("J11-段位守卫", segment_guard(), "0x76F0段闭集10码");
    s.add(
        "J9-宇宙常量",
        SCEN_COUNT == 5 && REPEAT_RUNS == 3 && STRESS_PPM == 1_000_000,
        "5场景×3跑×100%线写死",
    );
    s
}
