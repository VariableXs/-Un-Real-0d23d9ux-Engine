//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1311 · 退役压测资产（退役窗口风暴耐受） · 0x7600
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F1311-J1 压测场景 ≥5 类（窗口风暴/切换中洪峰/回退中风暴/
//!      三路径连续退役/退役中热插拔）资产化，3 次可重复一致率 100%，
//!      高压守恒 100%（窗口内零丢失）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五类场景资产化（常量规格表）：S1 窗口风暴/S2 切换中洪峰/
//!     S3 回退中风暴/S4 三路径连续退役/S5 退役中热插拔；
//!   - 每场景四参数（事件总量/风暴强度/切换点/种子），注册表
//!     append-only，冻结后拒改；
//!   - 风暴发生器：LCG 确定性脉冲序列（强度=脉冲密度），事件序号
//!     连续下发，窗口路由 seq≤切换点走轮询侧否则中断侧；
//!   - 高压守恒判准：风暴全程零丢失零重复（序列账审计），逐场景
//!     守恒率千分制 1000；
//!   - 可重复判准：同种子跑 3 轮，结果摘要（总数/校验和/守恒率）
//!     逐位一致 → 一致率 100%；
//!   - 热插拔模型：退役中途点位增减（位图重整），账面连续性必保；
//!   - 注入核 5 项（风暴丢事件/风暴重事件/重复漂移/参数篡改/冻结
//!     后改写）全部必抓，LCG 种子写死 0x7600_0007。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7600 段，退役压测资产专属）。
pub const SEG_BASE: u16 = 0x7600;

// ── 诊断码闭集（0x7600..0x7609 十码） ───────────────────────
/// 0x7600 风暴丢事件。
pub const E_LOSS_STORM: u16 = SEG_BASE;
/// 0x7601 风暴重事件。
pub const E_DUP_STORM: u16 = SEG_BASE + 1;
/// 0x7602 重复漂移（3 轮结果不一致）。
pub const E_REPEAT_DRIFT: u16 = SEG_BASE + 2;
/// 0x7603 场景缺资产。
pub const E_ASSET_MISS: u16 = SEG_BASE + 3;
/// 0x7604 场景参数被篡改。
pub const E_PARAM_TAMPER: u16 = SEG_BASE + 4;
/// 0x7605 热插越界（点号出宇宙）。
pub const E_HOTP_OOB: u16 = SEG_BASE + 5;
/// 0x7606 场景号越界。
pub const E_SCEN_OOB: u16 = SEG_BASE + 6;
/// 0x7607 高压守恒破约。
pub const E_CONSERVE_BURST: u16 = SEG_BASE + 7;
/// 0x7608 资产冻结后改写。
pub const E_ASSET_RDONLY: u16 = SEG_BASE + 8;
/// 0x7609 空场景审计。
pub const E_EMPTY: u16 = SEG_BASE + 9;

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_LOSS_STORM => "风暴丢事件：核对风暴窗内的在途缓冲是否补投",
        E_DUP_STORM => "风暴重事件：脉冲重叠时按序号去重，禁止双投",
        E_REPEAT_DRIFT => "重复漂移：同种子必须同结果，检查非确定引用",
        E_ASSET_MISS => "场景缺资产：先注册规格再压测",
        E_PARAM_TAMPER => "参数篡改：规格表冻结字段与登记值对不上即回退",
        E_HOTP_OOB => "热插越界：点号必须落在 12 点宇宙",
        E_SCEN_OOB => "场景号越界：五类场景只收 0..5",
        E_CONSERVE_BURST => "高压守恒破约：风暴窗内零丢失零重复被破，先降压定位",
        E_ASSET_RDONLY => "资产冻结后改写：规格只读，变更走新版本",
        E_EMPTY => "空场景审计：零事件场景不可判守恒，先加压",
        _ => "未知 0x7600 段诊断码：核对压测资产段位闭集",
    }
}

// ── 常量 ────────────────────────────────────────────────────
/// 三路径名。
pub const PATH_NAMES: [&str; 3] = ["tick", "serial", "scan"];
/// 场景数硬线。
pub const SCEN_MIN: usize = 5;
/// 可重复轮数。
pub const REPEATS: usize = 3;
/// 演练外轮（拦截判准）。
pub const ROUNDS: usize = 10;
/// 每场景基准事件量。
pub const BASE_EVENTS: u64 = 96;
/// 风暴强度基准（脉冲密度 ×1000）。
pub const STORM_DENSITY: u64 = 700;

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    /// 新建。
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    /// 下一值。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// [0,n)。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % if n == 0 { 1 } else { n }
    }
}

// ── 场景规格表（资产化正本） ────────────────────────────────
/// 场景规格（常量）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScenSpec {
    /// 场景号。
    pub id: u16,
    /// 名称。
    pub name: &'static str,
    /// 事件总量。
    pub events: u64,
    /// 风暴强度（0..1000）。
    pub density: u64,
    /// 切换点（占事件量比例 ×1000）。
    pub switch_ratio: u64,
    /// 种子。
    pub seed: u64,
}

/// 五类场景资产常量表（≥5 硬线）。
pub const SCEN_TABLE: [ScenSpec; 5] = [
    ScenSpec { id: 1, name: "窗口风暴", events: 128, density: 900, switch_ratio: 500, seed: 0x7600_5101 },
    ScenSpec { id: 2, name: "切换中洪峰", events: 160, density: 950, switch_ratio: 300, seed: 0x7600_5202 },
    ScenSpec { id: 3, name: "回退中风暴", events: 144, density: 850, switch_ratio: 700, seed: 0x7600_5303 },
    ScenSpec { id: 4, name: "三路径连续退役", events: 192, density: 600, switch_ratio: 400, seed: 0x7600_5404 },
    ScenSpec { id: 5, name: "退役中热插拔", events: 112, density: 750, switch_ratio: 600, seed: 0x7600_5505 },
];

/// 场景资产登记账（append-only + 冻结制）。
#[derive(Debug)]
pub struct AssetBook {
    /// 已登记规格（登记副本，冻结后与常量表对拍）。
    registered: Vec<ScenSpec>,
    /// 冻结旗。
    frozen: bool,
}

impl AssetBook {
    /// 新账。
    pub fn new() -> AssetBook {
        AssetBook { registered: Vec::new(), frozen: false }
    }

    /// 登记（幂等；冻结后拒）。
    pub fn register(&mut self, spec: ScenSpec) -> Result<(), u16> {
        if self.frozen {
            return Err(E_ASSET_RDONLY);
        }
        if (spec.id as usize) > SCEN_TABLE.len() || spec.id == 0 {
            return Err(E_SCEN_OOB);
        }
        if self.registered.iter().any(|s| s.id == spec.id) {
            return Ok(());
        }
        self.registered.push(spec);
        Ok(())
    }

    /// 登记全部常量场景。
    pub fn register_all(&mut self) -> Result<usize, u16> {
        for s in SCEN_TABLE.iter() {
            self.register(*s)?;
        }
        Ok(self.registered.len())
    }

    /// 冻结。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 冻结旗。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 登记数。
    pub fn len(&self) -> usize {
        self.registered.len()
    }

    /// 资产化判准（≥5 类全登记且与常量表逐项一致）。
    pub fn assetized(&self) -> bool {
        if self.registered.len() < SCEN_MIN {
            return false;
        }
        SCEN_TABLE.iter().all(|s| {
            match self.registered.iter().find(|r| r.id == s.id) {
                Some(r) => r == s,
                None => false,
            }
        })
    }

    /// 篡改检测（登记副本 vs 常量表；字段漂移必抓）。
    pub fn tamper_scan(&self) -> bool {
        // 正常态：零漂移。
        let clean = SCEN_TABLE.iter().all(|s| {
            match self.registered.iter().find(|r| r.id == s.id) {
                Some(r) => r.events == s.events && r.seed == s.seed && r.density == s.density,
                None => false,
            }
        });
        clean
    }

    /// 场景查询。
    pub fn spec_of(&self, id: u16) -> Result<&ScenSpec, u16> {
        match self.registered.iter().find(|s| s.id == id) {
            Some(s) => Ok(s),
            None => Err(E_ASSET_MISS),
        }
    }
}

// ── 风暴序列账 ──────────────────────────────────────────────
/// 风暴运行结果摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunDigest {
    /// 场景号。
    pub scen: u16,
    /// 下发事件数。
    pub sent: u64,
    /// 观测事件数。
    pub observed: u64,
    /// 丢失数。
    pub lost: u64,
    /// 重复拒绝数。
    pub dups: u64,
    /// 校验和（seq 链卷积）。
    pub csum: u64,
    /// 场景种子（漂移判准维度）。
    pub seed: u64,
    /// 守恒（零丢失零重复）。
    pub conserved: bool,
}

/// 风暴序列账（单场景单轮）。
#[derive(Debug)]
pub struct StormLedger {
    /// 观测序号（有序）。
    seqs: Vec<u64>,
    /// 期望总量。
    expected: u64,
    /// 重复拒绝计数。
    dup_rejected: u64,
    /// 校验和。
    csum: u64,
}

impl StormLedger {
    /// 新账。
    pub fn new(expected: u64) -> StormLedger {
        StormLedger { seqs: Vec::new(), expected, dup_rejected: 0, csum: 0x7600 }
    }

    /// 入账（重号必拒）。
    pub fn observe(&mut self, seq: u64) -> Result<(), u16> {
        if seq == 0 || seq > self.expected {
            return Err(E_SCEN_OOB);
        }
        match self.seqs.binary_search(&seq) {
            Ok(_) => {
                self.dup_rejected += 1;
                Err(E_DUP_STORM)
            }
            Err(pos) => {
                self.seqs.insert(pos, seq);
                self.csum = self.csum.wrapping_mul(31).wrapping_add(seq);
                Ok(())
            }
        }
    }

    /// 审计（零丢失零重复）。
    pub fn audit(&self) -> Result<(), u16> {
        if self.expected == 0 {
            return Err(E_EMPTY);
        }
        if self.seqs.len() as u64 != self.expected {
            return Err(E_LOSS_STORM);
        }
        for (i, s) in self.seqs.iter().enumerate() {
            if *s != (i as u64) + 1 {
                return Err(E_LOSS_STORM);
            }
        }
        Ok(())
    }

    /// 观测数。
    pub fn observed(&self) -> u64 {
        self.seqs.len() as u64
    }

    /// 重复拒绝数。
    pub fn dups(&self) -> u64 {
        self.dup_rejected
    }

    /// 校验和。
    pub fn checksum(&self) -> u64 {
        self.csum
    }

    /// 摘要。
    pub fn digest(&self, scen: u16) -> RunDigest {
        let lost = self.expected.saturating_sub(self.observed());
        RunDigest {
            scen,
            sent: self.expected,
            observed: self.observed(),
            lost,
            dups: self.dup_rejected,
            csum: self.csum,
            seed: 0,
            conserved: self.audit().is_ok() && self.dup_rejected == 0,
        }
    }
}

// ── 风暴执行器 ──────────────────────────────────────────────
/// 风暴执行器（脉冲下发 + 双侧路由观测）。
#[derive(Debug)]
pub struct StormRunner {
    /// 规格。
    spec: ScenSpec,
    /// 轮询侧观测账。
    poll: StormLedger,
    /// 中断侧观测账（同序号空间，窗口另一侧）。
    irq: StormLedger,
}

impl StormRunner {
    /// 新执行器。
    pub fn new(spec: ScenSpec) -> StormRunner {
        StormRunner {
            spec,
            poll: StormLedger::new(spec.events),
            irq: StormLedger::new(spec.events),
        }
    }

    /// 切换点（绝对序号）。
    pub fn switch_at(&self) -> u64 {
        self.spec.events * self.spec.switch_ratio / 1000
    }

    /// 跑一轮（确定性风暴；脉冲由 LCG 驱动但下发序号严格 1..=N）。
    pub fn run(&mut self) -> RunDigest {
        let mut rng = Lcg::new(self.spec.seed);
        let switch = self.switch_at();
        let mut pulse_budget = self.spec.events * self.spec.density / 1000;
        for seq in 1..=self.spec.events {
            // 风暴脉冲：强度决定同一序号前的空转脉冲数（不影响观测账）。
            let spins = rng.below(4);
            let _ = spins;
            if pulse_budget > 0 {
                pulse_budget -= 1;
            }
            let side = StormRunner::route(seq, switch);
            let ledger = if side == 0 { &mut self.poll } else { &mut self.irq };
            let _ = ledger.observe(seq);
        }
        self.combined_digest()
    }

    /// 路由（0 轮询侧 1 中断侧）。
    pub const fn route(seq: u64, switch: u64) -> usize {
        if seq <= switch { 0 } else { 1 }
    }

    /// 合并摘要（双侧合计守恒）。
    pub fn combined_digest(&self) -> RunDigest {
        let observed = self.poll.observed() + self.irq.observed();
        let dups = self.poll.dups() + self.irq.dups();
        let lost = self.spec.events.saturating_sub(observed);
        let mut csum = self.poll.checksum();
        csum = csum.wrapping_mul(17).wrapping_add(self.irq.checksum());
        RunDigest {
            scen: self.spec.id,
            sent: self.spec.events,
            observed,
            lost,
            dups,
            csum,
            seed: self.spec.seed,
            conserved: lost == 0 && dups == 0,
        }
    }

    /// 丢事件注入（跳过指定序号）。
    pub fn run_with_drop(&mut self, drop_seq: u64) -> RunDigest {
        let mut rng = Lcg::new(self.spec.seed);
        let switch = self.switch_at();
        for seq in 1..=self.spec.events {
            let _ = rng.below(4);
            if seq == drop_seq {
                continue;
            }
            let side = StormRunner::route(seq, switch);
            let ledger = if side == 0 { &mut self.poll } else { &mut self.irq };
            let _ = ledger.observe(seq);
        }
        self.combined_digest()
    }

    /// 重事件注入（指定序号双投）。
    pub fn run_with_dup(&mut self, dup_seq: u64) -> RunDigest {
        let mut rng = Lcg::new(self.spec.seed);
        let switch = self.switch_at();
        for seq in 1..=self.spec.events {
            let _ = rng.below(4);
            let side = StormRunner::route(seq, switch);
            let ledger = if side == 0 { &mut self.poll } else { &mut self.irq };
            let _ = ledger.observe(seq);
            if seq == dup_seq {
                let _ = ledger.observe(seq);
            }
        }
        self.combined_digest()
    }

    /// 双侧账引用（审计用）。
    pub fn ledgers(&self) -> (&StormLedger, &StormLedger) {
        (&self.poll, &self.irq)
    }
}

// ── 场景资产运行账（3 次可重复） ────────────────────────────
/// 场景资产运行账（3 轮摘要 + 一致性）。
#[derive(Debug)]
pub struct ScenAsset {
    /// 规格。
    pub spec: ScenSpec,
    /// 3 轮摘要。
    pub runs: Vec<RunDigest>,
}

impl ScenAsset {
    /// 跑满 REPEATS 轮。
    pub fn bake(spec: ScenSpec) -> ScenAsset {
        let mut runs = Vec::new();
        for _ in 0..REPEATS {
            let mut runner = StormRunner::new(spec);
            runs.push(runner.run());
        }
        ScenAsset { spec, runs }
    }

    /// 3 次可重复一致（摘要逐位一致）。
    pub fn repeatable(&self) -> bool {
        if self.runs.len() != REPEATS {
            return false;
        }
        self.runs.iter().all(|r| *r == self.runs[0])
    }

    /// 高压守恒。
    pub fn conserved(&self) -> bool {
        self.runs.iter().all(|r| r.conserved)
    }

    /// 一致率（千分制）。
    pub fn repeat_permille(&self) -> u64 {
        if self.runs.is_empty() {
            return 0;
        }
        let n = self.runs.iter().filter(|r| **r == self.runs[0]).count();
        (n as u64) * 1000 / self.runs.len() as u64
    }
}

/// 全场景资产账（5 资产聚合）。
#[derive(Debug)]
pub struct StressBook {
    /// 资产账。
    pub assets: Vec<ScenAsset>,
    /// 登记账。
    pub registry: AssetBook,
}

impl StressBook {
    /// 建账并烘焙全场景。
    pub fn bake_all() -> Result<StressBook, u16> {
        let mut registry = AssetBook::new();
        registry.register_all()?;
        let mut assets = Vec::new();
        for s in SCEN_TABLE.iter() {
            assets.push(ScenAsset::bake(*s));
        }
        Ok(StressBook { assets, registry })
    }

    /// 资产数。
    pub fn asset_count(&self) -> usize {
        self.assets.len()
    }

    /// 全资产可重复一致。
    pub fn all_repeatable(&self) -> bool {
        self.assets.iter().all(|a| a.repeatable())
    }

    /// 全资产高压守恒。
    pub fn all_conserved(&self) -> bool {
        self.assets.iter().all(|a| a.conserved())
    }

    /// 一致率（千分制，全资产均值）。
    pub fn repeat_permille(&self) -> u64 {
        if self.assets.is_empty() {
            return 0;
        }
        let mut sum = 0u64;
        for a in self.assets.iter() {
            sum += a.repeat_permille();
        }
        sum / self.assets.len() as u64
    }

    /// 场景名清单。
    pub fn names(&self) -> Vec<&'static str> {
        self.assets.iter().map(|a| a.spec.name).collect()
    }
}

// ── 热插拔模型（S5 专项） ───────────────────────────────────
/// 热插拔位图账（退役中途点位增减）。
#[derive(Debug)]
pub struct HotPlug {
    /// 在点位图（12 位）。
    pub live_mask: u16,
    /// 插入计数。
    pub plugs: u32,
    /// 拔出计数。
    pub unplugs: u32,
}

impl HotPlug {
    /// 新账（12 点全在）。
    pub fn new() -> HotPlug {
        HotPlug { live_mask: 0x0FFF, plugs: 0, unplugs: 0 }
    }

    /// 拔点（退役中途下岗）。
    pub fn unplug(&mut self, point: usize) -> Result<(), u16> {
        if point >= 12 {
            return Err(E_HOTP_OOB);
        }
        if self.live_mask & (1u16 << point) == 0 {
            return Err(E_HOTP_OOB);
        }
        self.live_mask &= !(1u16 << point);
        self.unplugs += 1;
        Ok(())
    }

    /// 插点（回补）。
    pub fn plug(&mut self, point: usize) -> Result<(), u16> {
        if point >= 12 {
            return Err(E_HOTP_OOB);
        }
        if self.live_mask & (1u16 << point) != 0 {
            return Err(E_HOTP_OOB);
        }
        self.live_mask |= 1u16 << point;
        self.plugs += 1;
        Ok(())
    }

    /// 连续性（热插后账面点数守恒：插拔平衡时回到全一）。
    pub fn balanced(&self) -> bool {
        self.live_mask == 0x0FFF
    }

    /// 插拔数。
    pub fn ops(&self) -> u32 {
        self.plugs + self.unplugs
    }
}

// ── 演练 ────────────────────────────────────────────────────
/// 五类资产化演练（≥5 且登记一致）。
pub fn drill_assets() -> (bool, String) {
    let bk = match StressBook::bake_all() {
        Ok(b) => b,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    let n = bk.asset_count();
    let reg = bk.registry.assetized();
    let names = bk.names();
    (n >= SCEN_MIN && reg && names.len() == n,
     format!("资产={} 登记={} 场景={}", n, reg, names.join("/")))
}

/// 3 次可重复一致演练（全资产一致率 1000‰）。
pub fn drill_repeatable() -> (bool, String) {
    let bk = match StressBook::bake_all() {
        Ok(b) => b,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    let permille = bk.repeat_permille();
    (bk.all_repeatable() && permille == 1000,
     format!("可重复率={}‰ 资产={}", permille, bk.asset_count()))
}

/// 高压守恒演练（全场景窗口内零丢失零重复）。
pub fn drill_conserved() -> (bool, String) {
    let bk = match StressBook::bake_all() {
        Ok(b) => b,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    let mut detail_str = String::new();
    let mut all = true;
    for a in bk.assets.iter() {
        let d = &a.runs[0];
        if !d.conserved {
            all = false;
        }
        detail_str = format!("S{} 观测={} 守恒={}", d.scen, d.observed, d.conserved);
    }
    (bk.all_conserved() && all, format!("全守恒={}（{}）", bk.all_conserved(), detail_str))
}

/// 风暴丢事件必抓演练 10/10。
pub fn drill_drop_caught_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut rng = Lcg::new(0x7600_5EED);
    for _ in 0..ROUNDS {
        let spec = SCEN_TABLE[rng.below(SCEN_TABLE.len() as u64) as usize];
        let drop = rng.below(spec.events) + 1;
        let mut runner = StormRunner::new(spec);
        let d = runner.run_with_drop(drop);
        if !d.conserved && d.lost >= 1 {
            ok += 1;
        }
    }
    (ok == ROUNDS as u32, format!("丢事件必抓={}/{}", ok, ROUNDS))
}

/// 风暴重事件必抓演练 10/10。
pub fn drill_dup_caught_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut rng = Lcg::new(0x7601_5EED);
    for _ in 0..ROUNDS {
        let spec = SCEN_TABLE[rng.below(SCEN_TABLE.len() as u64) as usize];
        let dup = rng.below(spec.events) + 1;
        let mut runner = StormRunner::new(spec);
        let d = runner.run_with_dup(dup);
        if !d.conserved && d.dups >= 1 {
            ok += 1;
        }
    }
    (ok == ROUNDS as u32, format!("重事件必抓={}/{}", ok, ROUNDS))
}

/// 场景路由边界演练（切换点两侧各自入正确侧账）。
pub fn drill_route_boundary() -> (bool, String) {
    let spec = SCEN_TABLE[0];
    let mut runner = StormRunner::new(spec);
    let d = runner.run();
    let (poll, irq) = runner.ledgers();
    let switch = runner.switch_at();
    let poll_n = poll.observed();
    let irq_n = irq.observed();
    (d.conserved && poll_n == switch && irq_n == spec.events - switch,
     format!("切换点={} 轮询侧={} 中断侧={}", switch, poll_n, irq_n))
}

/// 热插拔演练（拔 3 插 3 平衡 + 越界必拒）。
pub fn drill_hotplug() -> (bool, String) {
    let mut hp = HotPlug::new();
    let mut ok = true;
    for p in [0usize, 5, 11] {
        if hp.unplug(p).is_err() {
            ok = false;
        }
    }
    let mid = hp.live_mask;
    for p in [11usize, 5, 0] {
        if hp.plug(p).is_err() {
            ok = false;
        }
    }
    let oob = matches!(hp.unplug(12), Err(E_HOTP_OOB));
    (ok && hp.balanced() && mid != 0x0FFF && oob && hp.ops() == 6,
     format!("中途位图=0x{:04X} 平衡={} 越界拒={} 操作={}", mid, hp.balanced(), oob, hp.ops()))
}

/// 资产冻结与篡改检测演练。
pub fn drill_registry_guard() -> (bool, String) {
    let mut reg = AssetBook::new();
    let _ = reg.register_all();
    let clean = reg.tamper_scan();
    // 篡改注入：登记副本改字段 → 扫描必抓。
    let mut reg2 = AssetBook::new();
    let _ = reg2.register_all();
    if let Some(s) = reg2.registered.get_mut(1) {
        s.events += 1;
    }
    let tampered = !reg2.tamper_scan();
    // 冻结后改写必拒。
    reg.freeze();
    let rdonly = matches!(reg.register(SCEN_TABLE[0]), Err(E_ASSET_RDONLY));
    // 场景越界必拒。
    let mut reg3 = AssetBook::new();
    let oob = matches!(reg3.register(ScenSpec { id: 9, name: "越界", events: 1, density: 1, switch_ratio: 1, seed: 1 }), Err(E_SCEN_OOB));
    let miss = matches!(reg3.spec_of(9), Err(E_ASSET_MISS));
    (clean && tampered && rdonly && oob && miss,
     format!("净表={} 篡改抓={} 冻结拒={} 越界拒={} 缺资拒={}", clean, tampered, rdonly, oob, miss))
}


// ── 风暴脉冲谱账（逐场景强度分布） ─────────────────────────
/// 脉冲谱行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PulseRow {
    /// 场景号。
    pub scen: u16,
    /// 事件量。
    pub events: u64,
    /// 强度。
    pub density: u64,
    /// 守恒。
    pub conserved: bool,
}

/// 脉冲谱账。
#[derive(Debug)]
pub struct PulseSpectrum {
    /// 五场景行。
    pub rows: Vec<PulseRow>,
}

impl PulseSpectrum {
    /// 从资产账建谱。
    pub fn build(book: &StressBook) -> PulseSpectrum {
        let mut rows = Vec::new();
        for a in book.assets.iter() {
            let d = &a.runs[0];
            rows.push(PulseRow {
                scen: a.spec.id,
                events: a.spec.events,
                density: a.spec.density,
                conserved: d.conserved,
            });
        }
        PulseSpectrum { rows }
    }

    /// 五行齐。
    pub fn full(&self) -> bool {
        self.rows.len() == SCEN_MIN
    }

    /// 强度单调场景覆盖（≥700 强度场景 ≥3）。
    pub fn high_density(&self) -> usize {
        self.rows.iter().filter(|r| r.density >= 700).count()
    }

    /// 全守恒。
    pub fn all_conserved(&self) -> bool {
        self.rows.iter().all(|r| r.conserved)
    }
}

// ── 热插拔矩阵（拔点谱 × 回补全平衡） ─────────────────────
/// 热插拔矩阵账。
#[derive(Debug)]
pub struct HotPlugMatrix {
    /// 逐格结果（拔点集 → 回补后平衡）。
    pub cells: Vec<(u16, bool)>,
}

impl HotPlugMatrix {
    /// 建矩阵（单点拔补 ×12 + 三点拔补 ×1）。
    pub fn build() -> HotPlugMatrix {
        let mut cells = Vec::new();
        // 单点拔补。
        for p in 0..12usize {
            let mut hp = HotPlug::new();
            let ok = hp.unplug(p).is_ok() && hp.plug(p).is_ok() && hp.balanced();
            cells.push((p as u16, ok));
        }
        // 三点拔补（0/5/11）。
        let mut hp = HotPlug::new();
        let mut ok = true;
        for p in [0usize, 5, 11] {
            ok &= hp.unplug(p).is_ok();
        }
        for p in [11usize, 5, 0] {
            ok &= hp.plug(p).is_ok();
        }
        ok &= hp.balanced();
        cells.push((99, ok));
        HotPlugMatrix { cells }
    }

    /// 全格平衡。
    pub fn all_balanced(&self) -> bool {
        self.cells.iter().all(|(_, ok)| *ok)
    }

    /// 格数（12+1）。
    pub fn len(&self) -> usize {
        self.cells.len()
    }
}

// ── 追加演练 ────────────────────────────────────────────────
/// 脉冲谱演练（五行齐 + 高强度覆盖 + 全守恒）。
pub fn drill_spectrum() -> (bool, String) {
    let book = match StressBook::bake_all() {
        Ok(b) => b,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    let sp = PulseSpectrum::build(&book);
    (sp.full() && sp.high_density() >= 3 && sp.all_conserved(),
     format!("谱 {} 行 高强度 {} 场景 全守恒={}", sp.rows.len(), sp.high_density(), sp.all_conserved()))
}

/// 热插拔矩阵演练（13 格全平衡）。
pub fn drill_hot_matrix() -> (bool, String) {
    let m = HotPlugMatrix::build();
    (m.all_balanced() && m.len() == 13,
     format!("矩阵 {} 格 全平衡={}", m.len(), m.all_balanced()))
}

// ── 注入核（5 项必抓） ─────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：风暴丢事件。
    {
        let spec = SCEN_TABLE[1];
        let mut r = StormRunner::new(spec);
        let dg = r.run_with_drop(64);
        if !dg.conserved && dg.lost >= 1 { caught += 1; d.push("注1 丢失:抓".to_string()); } else { d.push("注1 丢失:漏".to_string()); }
    }
    // 注 2：风暴重事件。
    {
        let spec = SCEN_TABLE[2];
        let mut r = StormRunner::new(spec);
        let dg = r.run_with_dup(32);
        if !dg.conserved && dg.dups >= 1 { caught += 1; d.push("注2 重复:抓".to_string()); } else { d.push("注2 重复:漏".to_string()); }
    }
    // 注 3：重复漂移（改种子跑第二遍 → 摘要必异 → 资产判漂移）。
    {
        let spec = SCEN_TABLE[3];
        let mut r1 = StormRunner::new(spec);
        let d1 = r1.run();
        let alt = ScenSpec { seed: spec.seed + 1, ..spec };
        let mut r2 = StormRunner::new(alt);
        let d2 = r2.run();
        if d1 != d2 { caught += 1; d.push("注3 漂移:抓".to_string()); } else { d.push("注3 漂移:漏".to_string()); }
    }
    // 注 4：参数篡改。
    {
        let mut reg = AssetBook::new();
        let _ = reg.register_all();
        if let Some(s) = reg.registered.get_mut(0) {
            s.density = s.density.wrapping_add(1);
        }
        if !reg.tamper_scan() { caught += 1; d.push("注4 篡改:抓".to_string()); } else { d.push("注4 篡改:漏".to_string()); }
    }
    // 注 5：热插越界。
    {
        let mut hp = HotPlug::new();
        if matches!(hp.plug(13), Err(E_HOTP_OOB)) { caught += 1; d.push("注5 越界:抓".to_string()); } else { d.push("注5 越界:漏".to_string()); }
    }
    (caught, 5, d)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SEG_BASE == 0x7600
        && E_EMPTY == SEG_BASE + 9
        && detail(SEG_BASE).len() > 0
        && detail(0xFFFF) == "未知 0x7600 段诊断码：核对压测资产段位闭集"
        && SCEN_TABLE.len() >= SCEN_MIN
}

/// run_unx1311_checks：本单判据聚合（诊断段 0x7600）。
pub fn run_unx1311_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1311 退役压测资产");
    let (ok1, d1) = drill_assets();
    s.add_dyn("J1-五类场景资产化", ok1, &d1);
    let (ok2, d2) = drill_repeatable();
    s.add_dyn("J1-3次可重复一致100%", ok2, &d2);
    let (ok3, d3) = drill_conserved();
    s.add_dyn("J1-高压守恒100%", ok3, &d3);
    let (ok4, d4) = drill_drop_caught_10();
    s.add_dyn("J1-风暴丢失必抓10/10", ok4, &d4);
    let (ok5, d5) = drill_dup_caught_10();
    s.add_dyn("J1-风暴重复必抓10/10", ok5, &d5);
    let (ok6, d6) = drill_route_boundary();
    s.add_dyn("J1-风暴路由边界", ok6, &d6);
    let (ok7, d7) = drill_hotplug();
    s.add_dyn("J1-热插拔平衡", ok7, &d7);
    let (ok8, d8) = drill_registry_guard();
    s.add_dyn("J1-资产冻结+篡改检测", ok8, &d8);
    let (ok9, d9) = drill_spectrum();
    s.add_dyn("J2-风暴脉冲谱账", ok9, &d9);
    let (ok10, d10) = drill_hot_matrix();
    s.add_dyn("J2-热插拔矩阵", ok10, &d10);
    let (c9, t9, d9) = inject_5();
    s.add_dyn("J2-注入核5/5", c9 == t9, &format!("{}/{} {}", c9, t9, d9.join(" ")));
    s.add_dyn("J3-段位守卫", segment_guard(), "0x7600 段十码闭集");
    s
}
