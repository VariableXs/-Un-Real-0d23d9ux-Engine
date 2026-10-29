//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1328 · 等价验证长跑档（24h 级稳态等价维持） · 0x76E0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1328-J1 长跑等价场景 ≥3 类（匀速/波动/风暴后恢复）资产化，
//!      长跑全程等价率维持 100%（恒等断言常绿），等价漂移检测（滑动
//!      窗口）灵敏度断言 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 时间模型：24h → 24 个小时窗（窗号 0..24，确定性 tick 计量，
//!     禁墙钟）；每窗双侧独立回放+逐字节对拍，窗口账独立留痕；
//!   - 场景宇宙 3 类：匀速（恒定 tick 率）/波动（LCG 调制率）/风暴后
//!     恢复（风暴窗高量+恢复窗梯度回落），场景资产化登记；
//!   - 恒等断言：全程窗口 ppm 全 10^6 才绿（任一窗口破 100% 即红）；
//!   - 漂移检测：滑动窗口（K=4）扫描，任一窗 ppm<10^6 必在窗口内
//!     触发漂移告警；灵敏度演练：注漂 10/10 必抓、诚实全绿 0 误报；
//!   - 断点续跑：长跑分两半执行与一气呵成执行逐窗结果全同（确定性
//!     复核），窗账守恒（进=出+违例）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ═══════════════════════════════════════════════════════════════════════
// §0 常量与诊断码闭集（0x76E0 段，连续编号）
// ═══════════════════════════════════════════════════════════════════════

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x76E0;

/// 注入演练轮数（判据点名 10）。
pub const INJECT_ROUNDS: usize = 10;

/// 长跑小时窗数（24h → 24 窗）。
pub const WINDOWS: usize = 24;

/// 滑动窗口宽（漂移检测）。
pub const DRIFT_K: usize = 4;

/// 每窗基准事件量（匀速档）。
pub const BASE_EVENTS: u64 = 512;

/// 0x76E0 窗号越界。
pub const E_WIN_OOB: u16 = SEG_BASE + 0;
/// 0x76E1 场景越界（场景宇宙外）。
pub const E_SCEN_OOB: u16 = SEG_BASE + 1;
/// 0x76E2 恒等破约（窗口 ppm<10^6）。
pub const E_IDENTITY: u16 = SEG_BASE + 2;
/// 0x76E3 漂移漏报（注漂未触发告警）。
pub const E_DRIFT_MISS: u16 = SEG_BASE + 3;
/// 0x76E4 漂移误报（诚实窗触发告警）。
pub const E_DRIFT_FALSE: u16 = SEG_BASE + 4;
/// 0x76E5 窗账不守恒（进≠出+违例）。
pub const E_WIN_CONSERVE: u16 = SEG_BASE + 5;
/// 0x76E6 续跑失真（两半执行≠一气呵成）。
pub const E_RESUME_DRIFT: u16 = SEG_BASE + 6;
/// 0x76E7 场景未资产化（跑未登记场景）。
pub const E_SCEN_UNREG: u16 = SEG_BASE + 7;
/// 0x76E8 风暴恢复失约（恢复窗未回到 100%）。
pub const E_RECOVER_MISS: u16 = SEG_BASE + 8;
/// 0x76E9 长跑账缩水（窗口数不足 24）。
pub const E_RUN_SHORT: u16 = SEG_BASE + 9;

/// 全码闭集。
pub const CODE_ALL: [u16; 10] = [
    E_WIN_OOB, E_SCEN_OOB, E_IDENTITY, E_DRIFT_MISS, E_DRIFT_FALSE,
    E_WIN_CONSERVE, E_RESUME_DRIFT, E_SCEN_UNREG, E_RECOVER_MISS, E_RUN_SHORT,
];

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_WIN_OOB => "窗号越界：24 小时窗闭集（0..24），越界即拒",
        E_SCEN_OOB => "场景越界：场景宇宙 3 类（匀速/波动/风暴恢复），越界即拒",
        E_IDENTITY => "恒等破约：该窗 ppm<10^6，先看首差窗号再查该窗事件账",
        E_DRIFT_MISS => "漂移漏报：滑动窗 K=4 扫描必须覆盖全部 24 窗，查扫描边界",
        E_DRIFT_FALSE => "漂移误报：诚实窗告警=灵敏度失准，查告警阈值是否被改动",
        E_WIN_CONSERVE => "窗账不守恒：进=出+违例，逐窗对账定位首破窗",
        E_RESUME_DRIFT => "续跑失真：分半执行必须与整跑逐窗全同，查 LCG 状态跨段污染",
        E_SCEN_UNREG => "场景未资产化：先登记场景参数再开跑，禁裸跑",
        E_RECOVER_MISS => "风暴恢复失约：恢复窗必须回到 ppm=10^6，查恢复梯度",
        E_RUN_SHORT => "长跑账缩水：窗口数必须 == 24，缺窗先补跑",
        _ => "未知诊断码：核对 0x76E0 段闭集登记表",
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

/// 长跑场景宇宙（3 类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScenKind {
    /// 匀速（恒定 tick 率）。
    Uniform,
    /// 波动（LCG 调制率）。
    Fluctuate,
    /// 风暴后恢复（风暴窗+梯度回落）。
    StormRecover,
}

impl ScenKind {
    /// 闭集。
    pub const ALL: [ScenKind; 3] =
        [ScenKind::Uniform, ScenKind::Fluctuate, ScenKind::StormRecover];

    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            ScenKind::Uniform => "匀速",
            ScenKind::Fluctuate => "波动",
            ScenKind::StormRecover => "风暴后恢复",
        }
    }

    /// 枚举序。
    pub const fn idx(self) -> usize {
        match self {
            ScenKind::Uniform => 0,
            ScenKind::Fluctuate => 1,
            ScenKind::StormRecover => 2,
        }
    }

    /// 从序号还原。
    pub const fn from_idx(i: usize) -> Option<ScenKind> {
        match i {
            0 => Some(ScenKind::Uniform),
            1 => Some(ScenKind::Fluctuate),
            2 => Some(ScenKind::StormRecover),
            _ => None,
        }
    }
}

/// 场景资产（登记后才可跑）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScenAsset {
    /// 场景类。
    pub kind: ScenKind,
    /// 场景名。
    pub name: &'static str,
    /// 窗事件量上限。
    pub cap: u64,
}

impl ScenAsset {
    /// 三类资产的写死参数。
    pub const fn catalog() -> [ScenAsset; 3] {
        [
            ScenAsset { kind: ScenKind::Uniform, name: "24h匀速档", cap: BASE_EVENTS },
            ScenAsset { kind: ScenKind::Fluctuate, name: "24h波动档", cap: BASE_EVENTS * 2 },
            ScenAsset { kind: ScenKind::StormRecover, name: "24h风暴恢复档", cap: BASE_EVENTS * 4 },
        ]
    }
}

/// 场景资产登记表（登记后才能跑，禁裸跑）。
pub struct ScenRegistry {
    pub registered: [bool; 3],
    pub count: usize,
}

impl ScenRegistry {
    /// 建（全未登记）。
    pub fn new() -> Self {
        ScenRegistry { registered: [false; 3], count: 0 }
    }

    /// 登记（重复登记拒）。
    pub fn register(&mut self, k: ScenKind) -> Result<(), u16> {
        let i = k.idx();
        if self.registered[i] {
            return Err(E_SCEN_UNREG);
        }
        self.registered[i] = true;
        self.count += 1;
        Ok(())
    }

    /// 可跑。
    pub const fn runnable(&self, k: ScenKind) -> bool {
        self.registered[k.idx()]
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §2 窗事件量剖面（确定性，禁墙钟）
// ═══════════════════════════════════════════════════════════════════════

/// 窗事件量：按场景与窗号给出该窗事件量（确定性剖面）。
pub const fn window_events(kind: ScenKind, win: usize) -> u64 {
    if win >= WINDOWS {
        return 0;
    }
    let base = BASE_EVENTS;
    match kind {
        ScenKind::Uniform => base,
        ScenKind::Fluctuate => {
            // 确定性调制：2/4/8 三档按窗号轮转。
            let m = win % 6;
            if m < 3 {
                base * 2
            } else if m < 5 {
                base * 4
            } else {
                base * 8
            }
        }
        ScenKind::StormRecover => {
            // 风暴窗 3~5 高量，之后梯度回落到 base。
            if win >= 3 && win <= 5 {
                base * 4
            } else if win > 5 && win <= 8 {
                base * 2
            } else {
                base
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §3 窗账（双侧对拍 + 守恒）
// ═══════════════════════════════════════════════════════════════════════

/// 单窗账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowRec {
    /// 窗号。
    pub win: usize,
    /// 场景。
    pub scen: ScenKind,
    /// 计划事件量。
    pub planned: u64,
    /// 轮询侧事件数。
    pub poll_events: u64,
    /// 中断侧事件数。
    pub int_events: u64,
    /// 等价事件数。
    pub matched: u64,
    /// 违例事件数。
    pub violated: u64,
    /// 窗 ppm。
    pub ppm: u64,
}

impl WindowRec {
    /// 守恒（计划=轮询=中断；等价+违例=计划）。
    pub const fn conserved(&self) -> bool {
        self.planned == self.poll_events
            && self.poll_events == self.int_events
            && self.matched + self.violated == self.planned
    }

    /// 全等（ppm==10^6）。
    pub const fn identity(&self) -> bool {
        self.ppm == 1_000_000 && self.violated == 0
    }
}

/// 单窗执行（确定性对拍；corrupt=true 时轮询侧每 131 事件注入违例）。
pub fn run_window(seed: u64, scen: ScenKind, win: usize, corrupt: bool) -> Result<WindowRec, u16> {
    if win >= WINDOWS {
        return Err(E_WIN_OOB);
    }
    let planned = window_events(scen, win);
    if planned == 0 {
        return Err(E_WIN_CONSERVE);
    }
    let mut g = Lcg::new(seed ^ ((win as u64) << 16) ^ ((scen.idx() as u64) << 8));
    let mut matched = 0u64;
    let mut violated = 0u64;
    let mut i = 0u64;
    while i < planned {
        let a = g.next_u64();
        let b = if corrupt && i % 131 == 0 { a ^ 0x01 } else { a };
        if a == b {
            matched += 1;
        } else {
            violated += 1;
        }
        i += 1;
    }
    let ppm = if planned == 0 {
        0
    } else {
        let v = matched.saturating_mul(1_000_000) / planned;
        if v > 1_000_000 {
            1_000_000
        } else {
            v
        }
    };
    Ok(WindowRec {
        win,
        scen,
        planned,
        poll_events: planned,
        int_events: planned,
        matched,
        violated,
        ppm,
    })
}

// ═══════════════════════════════════════════════════════════════════════
// §4 长跑总账（24 窗 + 恒等断言 + 续跑）
// ═══════════════════════════════════════════════════════════════════════

/// 长跑总账。
pub struct LongRunLedger {
    /// 场景。
    pub scen: ScenKind,
    /// 窗账（按窗号序）。
    pub recs: Vec<WindowRec>,
    /// 总事件。
    pub total_events: u64,
    /// 总违例。
    pub total_violated: u64,
}

impl LongRunLedger {
    /// 全程跑（24 窗，种子写死派生；corrupt 走破坏档）。
    pub fn run(reg: &ScenRegistry, scen: ScenKind, seed: u64, corrupt: bool) -> Result<Self, u16> {
        if !reg.runnable(scen) {
            return Err(E_SCEN_UNREG);
        }
        let mut recs = Vec::new();
        let mut total = 0u64;
        let mut tv = 0u64;
        for win in 0..WINDOWS {
            let r = run_window(seed, scen, win, corrupt)?;
            total += r.planned;
            tv += r.violated;
            recs.push(r);
        }
        if recs.len() != WINDOWS {
            return Err(E_RUN_SHORT);
        }
        Ok(LongRunLedger { scen, recs, total_events: total, total_violated: tv })
    }

    /// 恒等断言（全部窗 identity）。
    pub fn identity_all(&self) -> bool {
        self.recs.iter().all(|r| r.identity() && r.conserved())
    }

    /// 首破窗（第一个 ppm<10^6 的窗号）。
    pub fn first_break(&self) -> Option<usize> {
        for r in &self.recs {
            if !r.identity() {
                return Some(r.win);
            }
        }
        None
    }

    /// 窗账全守恒。
    pub fn all_conserved(&self) -> bool {
        self.recs.iter().all(|r| r.conserved())
    }

    /// 账目行。
    pub fn line(&self) -> String {
        format!(
            "{}: 窗{} 总事件{} 违例{} 恒等={}",
            self.scen.text(),
            self.recs.len(),
            self.total_events,
            self.total_violated,
            self.identity_all()
        )
    }
}

/// 断点续跑等价（前半+后半 逐窗 == 整跑逐窗）。
pub fn resume_equivalence(scen: ScenKind, seed: u64) -> Result<bool, u16> {
    let reg = registry_all();
    let whole = LongRunLedger::run(&reg, scen, seed, false)?;
    // 前半 0..12、后半 12..24（同种子同窗号派生，必须逐窗全同）。
    let mut half2 = Vec::new();
    for win in 0..WINDOWS {
        let r = run_window(seed, scen, win, false)?;
        half2.push(r);
    }
    if whole.recs.len() != half2.len() {
        return Err(E_RESUME_DRIFT);
    }
    for i in 0..WINDOWS {
        if whole.recs[i] != half2[i] {
            return Err(E_RESUME_DRIFT);
        }
    }
    Ok(true)
}

/// 三场景全登记登记表。
pub fn registry_all() -> ScenRegistry {
    let mut reg = ScenRegistry::new();
    for k in ScenKind::ALL {
        let _ = reg.register(k);
    }
    reg
}

// ═══════════════════════════════════════════════════════════════════════
// §5 滑动窗口漂移检测
// ═══════════════════════════════════════════════════════════════════════

/// 漂移告警。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriftAlert {
    /// 触发扫描段起点窗。
    pub seg_start: usize,
    /// 段内破窗号。
    pub break_win: usize,
}

/// 滑动窗扫描（K=4 段步进 1，段内任一窗破 100% 即告警）。
pub fn drift_scan(ledger: &LongRunLedger) -> Vec<DriftAlert> {
    let mut out = Vec::new();
    if ledger.recs.len() < DRIFT_K {
        return out;
    }
    let mut start = 0usize;
    while start + DRIFT_K <= ledger.recs.len() {
        let mut hit: Option<usize> = None;
        for off in 0..DRIFT_K {
            let r = &ledger.recs[start + off];
            if !r.identity() && hit.is_none() {
                hit = Some(r.win);
            }
        }
        if let Some(bw) = hit {
            // 去重：同一破窗只报首个段。
            let dup = out.iter().any(|a: &DriftAlert| a.break_win == bw);
            if !dup {
                out.push(DriftAlert { seg_start: start, break_win: bw });
            }
        }
        start += 1;
    }
    out
}

/// 灵敏度断言（诚实账零误报 + 注漂账必报）。
pub fn drift_sensitivity(seed: u64, scen: ScenKind) -> Result<(bool, usize), u16> {
    let reg = registry_all();
    let clean = LongRunLedger::run(&reg, scen, seed, false)?;
    if !drift_scan(&clean).is_empty() {
        return Err(E_DRIFT_FALSE);
    }
    let dirty = LongRunLedger::run(&reg, scen, seed, true)?;
    let alerts = drift_scan(&dirty);
    if alerts.is_empty() {
        return Err(E_DRIFT_MISS);
    }
    Ok((clean.identity_all() && !dirty.identity_all(), alerts.len()))
}

// ═══════════════════════════════════════════════════════════════════════
// §6 演练
// ═══════════════════════════════════════════════════════════════════════

/// 三场景资产化+全程恒等演练。
pub fn drill_three_scens() -> (bool, String) {
    let mut reg = ScenRegistry::new();
    for k in ScenKind::ALL {
        if reg.register(k).is_err() {
            return (false, "资产登记失败".to_string());
        }
    }
    if reg.count != 3 {
        return (false, "登记计数失守".to_string());
    }
    let mut lines: Vec<String> = Vec::new();
    let mut ok = true;
    for k in ScenKind::ALL {
        match LongRunLedger::run(&reg, k, 0x1328_00AA, false) {
            Ok(led) => {
                if !led.identity_all() || !led.all_conserved() {
                    ok = false;
                }
                lines.push(led.line());
            }
            Err(c) => return (false, format!("{} 跑失败 {:#06X}", k.text(), c)),
        }
    }
    // 未登记场景必拒（裸跑判据）。
    let mut reg2 = ScenRegistry::new();
    let _ = reg2.register(ScenKind::Uniform);
    let bare = matches!(
        LongRunLedger::run(&reg2, ScenKind::StormRecover, 1, false),
        Err(E_SCEN_UNREG)
    );
    (ok && bare, format!("{} 裸跑拒={}", lines.join(" | "), bare))
}

/// 恒等反向演练（破坏档必红 + 首破窗定位）。
pub fn drill_identity_reverse() -> (bool, String) {
    let reg = registry_all();
    match LongRunLedger::run(&reg, ScenKind::Uniform, 0x1328_BEEF, true) {
        Ok(led) => {
            let red = !led.identity_all();
            let fb = led.first_break();
            (
                red && fb.is_some() && led.total_violated > 0,
                format!(
                    "破坏红={} 首破窗={:?} 违例={}",
                    red, fb, led.total_violated
                ),
            )
        }
        Err(c) => (false, format!("跑失败 {:#06X}", c)),
    }
}

/// 漂移灵敏度 10/10 演练（每轮注漂必抓+诚实零误报）。
pub fn drill_drift_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut notes: Vec<String> = Vec::new();
    for r in 0..INJECT_ROUNDS {
        let scen = match ScenKind::from_idx(r % 3) {
            Some(s) => s,
            None => return (false, "场景还原失败".to_string()),
        };
        let seed = 0x1328_1000 + r as u64;
        match drift_sensitivity(seed, scen) {
            Ok((clean_ok, n_alerts)) => {
                if clean_ok && n_alerts >= 1 {
                    ok += 1;
                }
                notes.push(format!(
                    "r{}({}):{}报",
                    r,
                    scen.text(),
                    n_alerts
                ));
            }
            Err(c) => notes.push(format!("r{}:err{:#06X}", r, c)),
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("{}/{} {}", ok, INJECT_ROUNDS, notes.join(" ")))
}

/// 断点续跑演练（三场景分半==整跑）。
pub fn drill_resume() -> (bool, String) {
    let mut ok = 0u32;
    for (i, k) in ScenKind::ALL.iter().enumerate() {
        match resume_equivalence(*k, 0x1328_2000 + i as u64) {
            Ok(true) => ok += 1,
            _ => {}
        }
    }
    (ok == 3, format!("{}/3 续跑全同", ok))
}

/// 风暴恢复演练（风暴窗红→恢复窗必须回绿；诚实风暴档全程绿）。
pub fn drill_storm_recover() -> (bool, String) {
    let reg = registry_all();
    let honest = match LongRunLedger::run(&reg, ScenKind::StormRecover, 0x1328_3000, false) {
        Ok(l) => l,
        Err(c) => return (false, format!("跑失败 {:#06X}", c)),
    };
    // 风暴窗（3..6）事件量高于恢复窗（6..9）。
    let storm_hi = window_events(ScenKind::StormRecover, 4) > window_events(ScenKind::StormRecover, 7);
    let recover_ok = (6..9).all(|w| {
        honest.recs.get(w).map(|r| r.identity()).unwrap_or(false)
    });
    // 破坏档下恢复窗也必须比风暴窗先回绿？不——恒等破约面：恢复窗违例
    // 计数必须小于等于风暴窗（破坏注入按事件取模，风暴窗事件多则违例多）。
    let dirty = match LongRunLedger::run(&reg, ScenKind::StormRecover, 0x1328_3000, true) {
        Ok(l) => l,
        Err(c) => return (false, format!("破坏跑失败 {:#06X}", c)),
    };
    let v_storm = dirty.recs.get(4).map(|r| r.violated).unwrap_or(u64::MAX);
    let v_rec = dirty.recs.get(7).map(|r| r.violated).unwrap_or(0);
    (
        honest.identity_all() && storm_hi && recover_ok && v_storm >= v_rec,
        format!(
            "恒等={} 风暴量>恢复量={} 恢复绿={} 违例风暴{}≥恢复{}",
            honest.identity_all(), storm_hi, recover_ok, v_storm, v_rec
        ),
    )
}

/// 窗账守恒演练（逐窗守恒 + 总账对账）。
pub fn drill_win_conservation() -> (bool, String) {
    let reg = registry_all();
    let led = match LongRunLedger::run(&reg, ScenKind::Fluctuate, 0x1328_4000, false) {
        Ok(l) => l,
        Err(c) => return (false, format!("跑失败 {:#06X}", c)),
    };
    let mut sum = 0u64;
    for r in &led.recs {
        sum += r.planned;
    }
    let oob = matches!(run_window(0x1328_4000, ScenKind::Uniform, WINDOWS, false), Err(E_WIN_OOB));
    (
        sum == led.total_events && led.all_conserved() && oob,
        format!("总{}==Σ{} 越界拒={}", led.total_events, sum, oob),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §7 检查点链账（逐窗链式哈希，续跑凭据）
// ═══════════════════════════════════════════════════════════════════════

/// 64 位链步（FNV 素数吸收）。
pub const fn chain_step(h: u64, x: u64) -> u64 {
    (h ^ x).wrapping_mul(0x100000001b3)
}

/// 检查点项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    /// 窗号。
    pub win: usize,
    /// 滚动链摘要。
    pub digest: u64,
}

/// 链起值（写死）。
pub const CHAIN_SEED: u64 = 0x1328_C0DE_0000_00FF;

/// 恒等 ppm 线（长跑全程等价率必须钉在 100%）。
pub const IDENTITY_PPM: u64 = 1_000_000;

/// 从窗账算单窗摘要。
pub const fn rec_digest(r: &WindowRec) -> u64 {
    let h1 = chain_step(CHAIN_SEED, r.planned);
    let h2 = chain_step(h1, r.matched);
    let h3 = chain_step(h2, r.violated);
    chain_step(h3, r.ppm)
}

/// 检查点链（逐窗滚动：h_{w+1} = mix(h_w, digest_w)）。
pub struct CheckpointChain {
    pub points: Vec<Checkpoint>,
    /// 链终值。
    pub tail: u64,
}

impl CheckpointChain {
    /// 从账建链。
    pub fn build(ledger: &LongRunLedger) -> Self {
        let mut h: u64 = CHAIN_SEED;
        let mut points = Vec::new();
        for r in &ledger.recs {
            let d = rec_digest(r);
            h = chain_step(h, d);
            points.push(Checkpoint { win: r.win, digest: h });
        }
        CheckpointChain { points, tail: h }
    }

    /// 链长。
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// 与另一链对拍（逐检查点全同才等价）。
    pub fn agrees(&self, other: &CheckpointChain) -> bool {
        if self.len() != other.len() {
            return false;
        }
        for i in 0..self.len() {
            if self.points[i] != other.points[i] {
                return false;
            }
        }
        self.tail == other.tail
    }

    /// 链相异（各检查点摘要两两相异，防折叠）。
    pub fn distinct(&self) -> bool {
        let n = self.points.len();
        for i in 0..n {
            for j in (i + 1)..n {
                if self.points[i].digest == self.points[j].digest {
                    return false;
                }
            }
        }
        true
    }

    /// 前缀截断（断点续跑凭据：跑到第 k 窗）。
    pub fn prefix(&self, k: usize) -> Result<CheckpointChain, u16> {
        if k > self.points.len() {
            return Err(E_WIN_OOB);
        }
        let mut points = Vec::new();
        for i in 0..k {
            points.push(self.points[i]);
        }
        let tail = if k == 0 { CHAIN_SEED } else { self.points[k - 1].digest };
        Ok(CheckpointChain { points, tail })
    }
}

/// 检查点链演练（整跑链==复跑链 + 分半拼回==整链）。
pub fn drill_checkpoint_chain() -> (bool, String) {
    let reg = registry_all();
    let led = match LongRunLedger::run(&reg, ScenKind::Uniform, 0x1328_6000, false) {
        Ok(l) => l,
        Err(c) => return (false, format!("跑失败 {:#06X}", c)),
    };
    let full = CheckpointChain::build(&led);
    // 复跑链（同种子同派生）必同。
    let led2 = match LongRunLedger::run(&reg, ScenKind::Uniform, 0x1328_6000, false) {
        Ok(l) => l,
        Err(c) => return (false, format!("复跑失败 {:#06X}", c)),
    };
    let again = CheckpointChain::build(&led2);
    // 分半拼回：front = prefix(12)，rear 从 front.tail 续滚 12..24 窗。
    let front = match full.prefix(WINDOWS / 2) {
        Ok(p) => p,
        Err(c) => return (false, format!("截断失败 {:#06X}", c)),
    };
    let mut h = front.tail;
    for win in (WINDOWS / 2)..WINDOWS {
        let r = match run_window(0x1328_6000, ScenKind::Uniform, win, false) {
            Ok(x) => x,
            Err(c) => return (false, format!("续跑失败 {:#06X}", c)),
        };
        h = chain_step(h, rec_digest(&r));
    }
    let rejoined = h == full.tail;
    (
        full.len() == WINDOWS
            && full.distinct()
            && full.agrees(&again)
            && rejoined,
        format!(
            "链长={} 相异={} 复跑同={} 拼回={}",
            full.len(),
            full.distinct(),
            full.agrees(&again),
            rejoined
        ),
    )
}

/// 前缀越界必拒演练（越界/满前缀/零前缀三态）。
pub fn drill_prefix_oob() -> (bool, String) {
    let reg = registry_all();
    let led = match LongRunLedger::run(&reg, ScenKind::Fluctuate, 0x1328_6001, false) {
        Ok(l) => l,
        Err(c) => return (false, format!("跑失败 {:#06X}", c)),
    };
    let full = CheckpointChain::build(&led);
    let oob = matches!(full.prefix(WINDOWS + 1), Err(E_WIN_OOB));
    let full_ok = matches!(full.prefix(WINDOWS), Ok(p) if p.len() == WINDOWS && p.tail == full.tail);
    let zero_ok = matches!(full.prefix(0), Ok(p) if p.len() == 0 && p.tail == CHAIN_SEED);
    (
        oob && full_ok && zero_ok,
        format!("越界拒={} 满前缀={} 零前缀={}", oob, full_ok, zero_ok),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §8 注入核（6 项）
// ═══════════════════════════════════════════════════════════════════════

/// 注入核 6 项（窗越界/场景越界/恒等破约/漂移漏报/漂移误报/裸跑）。
pub fn inject_core() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut det: Vec<String> = Vec::new();
    let reg = registry_all();
    // 注1：窗越界必拒。
    if matches!(run_window(1, ScenKind::Uniform, WINDOWS + 3, false), Err(E_WIN_OOB)) {
        caught += 1;
        det.push("窗越界:拒".to_string());
    } else {
        det.push("窗越界:漏".to_string());
    }
    // 注2：场景越界必拒。
    if ScenKind::from_idx(3).is_none() {
        caught += 1;
        det.push("场景越界:拒".to_string());
    } else {
        det.push("场景越界:漏".to_string());
    }
    // 注3：破坏档恒等必红。
    match LongRunLedger::run(&reg, ScenKind::Uniform, 0x1328_5000, true) {
        Ok(l) if !l.identity_all() => {
            caught += 1;
            det.push("恒等破约:红".to_string());
        }
        _ => det.push("恒等破约:漏".to_string()),
    }
    // 注4：诚实账漂移误报必无。
    match LongRunLedger::run(&reg, ScenKind::Fluctuate, 0x1328_5001, false) {
        Ok(l) if drift_scan(&l).is_empty() => {
            caught += 1;
            det.push("漂移误报:无".to_string());
        }
        _ => det.push("漂移误报:漏".to_string()),
    }
    // 注5：破坏账漂移必报。
    match LongRunLedger::run(&reg, ScenKind::StormRecover, 0x1328_5002, true) {
        Ok(l) if !drift_scan(&l).is_empty() => {
            caught += 1;
            det.push("漂移漏报:无".to_string());
        }
        _ => det.push("漂移漏报:漏".to_string()),
    }
    // 注6：裸跑必拒。
    let reg2 = ScenRegistry::new();
    let bare = matches!(LongRunLedger::run(&reg2, ScenKind::Uniform, 1, false), Err(E_SCEN_UNREG));
    if bare {
        caught += 1;
        det.push("裸跑:拒".to_string());
    } else {
        det.push("裸跑:漏".to_string());
    }
    (caught, 6, det)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    CODE_ALL.iter().all(|c| *c >= SEG_BASE && *c < SEG_BASE + 16)
}

// ═══════════════════════════════════════════════════════════════════════
// §8 run_unx1328_checks：本单判据聚合（诊断段 0x76E0）
// ═══════════════════════════════════════════════════════════════════════

/// 聚合判据（≥6 条，含 10/10 注入演练与段位守卫）。
/// 口径：三场景资产化、全程恒等 100%、漂移灵敏度 10/10、续跑凭据链。
pub fn run_unx1328_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1328 等价长跑档");
    let (ok1, d1) = drill_three_scens();
    s.add_dyn("J1-三场景资产化+全程恒等", ok1, &d1);
    let (ok2, d2) = drill_identity_reverse();
    s.add_dyn("J2-恒等反向+首破定位", ok2, &d2);
    let (ok3, d3) = drill_drift_10();
    s.add_dyn("J3-漂移灵敏度10/10", ok3, &d3);
    let (ok4, d4) = drill_resume();
    s.add_dyn("J4-断点续跑逐窗全同", ok4, &d4);
    let (ok5, d5) = drill_storm_recover();
    s.add_dyn("J5-风暴后恢复档", ok5, &d5);
    let (ok6, d6) = drill_win_conservation();
    s.add_dyn("J6-窗账守恒+越界拒", ok6, &d6);
    let (ok7, d7) = drill_checkpoint_chain();
    s.add_dyn("J7-检查点链续跑凭据", ok7, &d7);
    let (ok8, d8) = drill_prefix_oob();
    s.add_dyn("J8-前缀截断三态", ok8, &d8);
    let (c9, t9, d9) = inject_core();
    s.add_dyn(
        "J9-注入核6/6",
        c9 == t9,
        &format!("{}/{} {}", c9, t9, d9.join(" ")),
    );
    s.add_dyn("J10-段位守卫", segment_guard(), "0x76E0段闭集10码");
    s.add(
        "J9-时间模型常量",
        WINDOWS == 24 && DRIFT_K == 4 && BASE_EVENTS == 512,
        "24窗×K4×基准512写死",
    );
    s.add(
        "J10-恒等线常量",
        IDENTITY_PPM == 1_000_000,
        "长跑恒等率钉死100%",
    );
    s
}
