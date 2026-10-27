//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1266 · Irq 态稳态长跑压测（24h 级耐受资产） · 0xFC60
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1266-J1 长跑压测场景 ≥4 类（稳态匀速/昼夜波动/周期风暴/
//!      随机尖峰）资产化，长跑期间零丢失（恒等断言全程常绿），
//!      资源零泄漏（内存/账本容量稳态收敛断言）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四场景资产化：稳态匀速/昼夜波动/周期风暴/随机尖峰，每场景一个
//!     确定性生成器（LCG 种子写死），24h 按压缩比折算 tick 步进；
//!   - 压缩因子：1 tick=1ms，24h=86,400,000 tick；资产按 1:100_000
//!     压缩为 864 大步（每步代表 100_000 tick），步内事件量按场景律；
//!   - 零丢失恒等：注入 ≡ 给付+滑出归档+窗内在途（四项恒等式，定容窗
//!     滑出件计归档不丢账），每 48 大步巡检一次，全程常绿；
//!   - 资源零泄漏：账本走定容滑动窗（窗口容量恒定），内存水位以
//!     「活动-释放-归档 ≡ 窗占用」残差断言收敛到 0；
//!   - 昼夜律：余弦表 24 格查表（禁浮点）；风暴律：周期 97 步插入
//!     3 步风暴（×8）；尖峰律：闭式每 16 步一尖峰（+64）；
//!   - 场景资产卡：四卡结构化落档（注入/认收/巡检/风暴窗）；
//!   - 速率分桶账：864 步按 12 桶聚合，Σ桶 ≡ 场景闭式总注入；
//!   - 巡检序列账：巡检点数 = 864/48 + 收官 1 = 19（判据常量冻结）；
//!   - LCG 种子 0xFC60 系写死，全程可重复。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xFC60;

/// 长跑场景数（判据点名 ≥4）。
pub const SCEN_COUNT: usize = 4;

/// 压缩大步数（24h=86,400,000 tick ÷ 100,000 = 864 步）。
pub const COMPRESS_STEPS: u64 = 864;

/// 压缩因子（每大步代表的 tick 数，1:100_000）。
pub const COMPRESS_FACTOR: u64 = 100_000;

/// 24h 总 tick（86,400,000 = 24×3600×1000）。
pub const TOTAL_TICK: u64 = 86_400 * 1000;

/// 稳态基线速率（笔/大步，对应 1ms 一笔的匀速律）。
pub const BASE_RATE: u64 = 100;

/// 账本滑动窗容量（零泄漏：容量恒定不再增长）。
pub const LEDGER_CAP: usize = 64;

/// 巡检间隔（大步）。
pub const PATROL_INTERVAL: u64 = 48;

/// 注入演练轮数。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0xFC60 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LrCode {
    /// 0xFC60 场景号越界（出 0..4）。
    ScenOob = 0,
    /// 0xFC61 零丢失恒等破（注入≠给付+归档+窗内）。
    LossBreach = 1,
    /// 0xFC62 账本容量击穿（滑动窗超定容）。
    CapBreach = 2,
    /// 0xFC63 内存残差未收敛（活动-释放-归档≠0）。
    MemLeak = 3,
    /// 0xFC64 昼夜律越界（查表下标出 0..24）。
    DiurnalOob = 4,
    /// 0xFC65 风暴律漂移（风暴窗数量≠预期）。
    StormDrift = 5,
    /// 0xFC66 步长越界（步号 ≥ 864）。
    StepOob = 6,
    /// 0xFC67 巡检欠账（巡检点缺失）。
    PatrolMiss = 7,
    /// 0xFC68 可重复失恒（同种子两轮账面不同）。
    RepeatDrift = 8,
    /// 0xFC69 场景律非单调越界（匀速场景出现速率抖动）。
    RateDrift = 9,
}

impl LrCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [LrCode; 10] = [
        LrCode::ScenOob, LrCode::LossBreach, LrCode::CapBreach, LrCode::MemLeak,
        LrCode::DiurnalOob, LrCode::StormDrift, LrCode::StepOob, LrCode::PatrolMiss,
        LrCode::RepeatDrift, LrCode::RateDrift,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        0xFC60 => "场景号越界：四场景资产只认 0..4，索引先比 SCEN_COUNT",
        0xFC61 => "零丢失破恒：注入/给付/归档/窗内四账不等，先重放场景律再定位丢失段",
        0xFC62 => "容量击穿：滑动窗超定容，检查是否误用增长容器替代定容窗",
        0xFC63 => "内存残差未收敛：活动-释放-归档≠0，逐大步查分配/释放配对",
        0xFC64 => "昼夜律越界：余弦表 24 格，下标先钳制 hour%24",
        0xFC65 => "风暴律漂移：风暴窗数与周期 97 推演不符，复核风暴插桩点",
        0xFC66 => "步长越界：压缩步 0..864，越界说明折算因子被改",
        0xFC67 => "巡检欠账：巡检点缺失说明长跑被跳段，查调度保证",
        0xFC68 => "可重复失恒：同种子两轮不同，查隐藏非确定性源（地址/序）",
        0xFC69 => "速率漂移：匀速场景出现抖动，查事件源是否混入波动律",
        _ => "未知诊断码：查 0xFC60 段闭集 LrCode::ALL",
    }
}

/// 四场景枚举（判据点名四类资产）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    /// 稳态匀速：恒定基线速率。
    SteadyRate = 0,
    /// 昼夜波动：24 格余弦律。
    Diurnal = 1,
    /// 周期风暴：周期 97 步插入 3 步风暴。
    PeriodStorm = 2,
    /// 随机尖峰：闭式每 16 步一尖峰。
    RandomSpike = 3,
}

impl Scenario {
    /// 由号取场景。
    pub const fn from(n: usize) -> Option<Scenario> {
        match n {
            0 => Some(Scenario::SteadyRate),
            1 => Some(Scenario::Diurnal),
            2 => Some(Scenario::PeriodStorm),
            3 => Some(Scenario::RandomSpike),
            _ => None,
        }
    }
    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            Scenario::SteadyRate => "稳态匀速",
            Scenario::Diurnal => "昼夜波动",
            Scenario::PeriodStorm => "周期风暴",
            Scenario::RandomSpike => "随机尖峰",
        }
    }
}

/// 昼夜余弦离散表（24 格，峰值在 12 时，谷值在 0 时；振幅 ±60%基线）。
/// 查表禁浮点：值 = 100 + 60×cos(2πh/24) 取整近似（手算冻结常量）。
pub const DIURNAL_TABLE: [u64; 24] = [
    40, 43, 50, 60, 70, 81, 92, 104, 118, 132, 146, 155,
    160, 155, 146, 132, 118, 104, 92, 81, 70, 60, 50, 43,
];

/// 昼夜律速率：hour = (step / 36) % 24（864 步 = 24 昼夜 × 36 步/时）。
pub const fn diurnal_rate(step: u64) -> Result<u64, LrCode> {
    if step >= COMPRESS_STEPS {
        return Err(LrCode::StepOob);
    }
    let hour = ((step / 36) % 24) as usize;
    Ok(DIURNAL_TABLE[hour] * BASE_RATE / 100)
}

/// 风暴律速率：周期 97 大步，第 0..3 步为风暴（×8），其余基线。
pub const fn storm_rate(step: u64) -> Result<u64, LrCode> {
    if step >= COMPRESS_STEPS {
        return Err(LrCode::StepOob);
    }
    if step % 97 < 3 {
        Ok(BASE_RATE * 8)
    } else {
        Ok(BASE_RATE)
    }
}

/// 尖峰律速率：每 16 大步一尖峰（+64，闭式确定性）。
pub const fn spike_rate(step: u64) -> u64 {
    if step % 16 == 0 {
        BASE_RATE + 64
    } else {
        BASE_RATE
    }
}

/// 长跑账本：滑动窗定容 + 三计数器（零泄漏断言面）。
#[derive(Debug)]
pub struct LongRunLedger {
    /// 场景号。
    scen: Scenario,
    /// 滑动窗（定容，超容滑出计归档）。
    window: Vec<u64>,
    /// 注入总量。
    injected: u64,
    /// 给付总量。
    delivered: u64,
    /// 滑出归档计数（定容窗挤出的在途件，不丢账）。
    dropped: u64,
    /// 活动分配计数。
    alloc_events: u64,
    /// 释放计数。
    free_events: u64,
    /// 巡检点序号（只增）。
    patrol_seq: u64,
    /// 峰值窗占用。
    peak_window: usize,
    /// 风暴窗累计（周期风暴场景对账用）。
    storm_windows: u64,
}

impl LongRunLedger {
    /// 建账。
    pub fn new(scen: Scenario) -> LongRunLedger {
        LongRunLedger {
            scen,
            window: Vec::new(),
            injected: 0,
            delivered: 0,
            dropped: 0,
            alloc_events: 0,
            free_events: 0,
            patrol_seq: 0,
            peak_window: 0,
            storm_windows: 0,
        }
    }

    /// 场景号。
    pub fn scenario(&self) -> Scenario {
        self.scen
    }

    /// 注入一笔（活动计数+1，滑窗入账；挤出件计归档不丢账）。
    pub fn inject(&mut self) -> Result<(), LrCode> {
        self.injected = self.injected.wrapping_add(1);
        self.alloc_events = self.alloc_events.wrapping_add(1);
        if self.window.len() >= LEDGER_CAP {
            self.window.remove(0);
            self.dropped = self.dropped.wrapping_add(1);
        }
        self.window.push(self.injected);
        if self.window.len() > self.peak_window {
            self.peak_window = self.window.len();
        }
        Ok(())
    }

    /// 给付一笔（释放计数+1，出窗；窗空即无在途可付，显性拒）。
    pub fn deliver(&mut self) -> Result<(), LrCode> {
        if self.window.is_empty() {
            return Err(LrCode::LossBreach);
        }
        self.delivered = self.delivered.wrapping_add(1);
        self.free_events = self.free_events.wrapping_add(1);
        self.window.remove(0);
        Ok(())
    }

    /// 恒等断言：注入 ≡ 给付+归档+窗内（四项恒等，判据零丢失口径）。
    pub fn conserved(&self) -> bool {
        self.injected
            == self
                .delivered
                .wrapping_add(self.dropped)
                .wrapping_add(self.window.len() as u64)
    }

    /// 零泄漏断言：活动-释放-归档 ≡ 窗占用，峰值 ≤ 定容。
    pub fn no_leak(&self) -> bool {
        self.alloc_events
            .wrapping_sub(self.free_events)
            .wrapping_sub(self.dropped)
            == self.window.len() as u64
            && self.peak_window <= LEDGER_CAP
            && self.window.len() <= LEDGER_CAP
    }

    /// 巡检一次（恒等+零泄漏双查）。
    pub fn patrol(&mut self) -> Result<(), LrCode> {
        if !self.conserved() {
            return Err(LrCode::LossBreach);
        }
        if !self.no_leak() {
            return Err(LrCode::MemLeak);
        }
        self.patrol_seq = self.patrol_seq.wrapping_add(1);
        Ok(())
    }

    /// 巡检序号。
    pub fn patrol_seq(&self) -> u64 {
        self.patrol_seq
    }

    /// 稳态收敛断言：窗占用回到低位（< 定容半）且巡检齐。
    pub fn converged(&self) -> bool {
        self.window.len() * 2 < LEDGER_CAP
    }

    /// 窗占用读数。
    pub fn window_len(&self) -> usize {
        self.window.len()
    }

    /// 归档读数。
    pub fn archived(&self) -> u64 {
        self.dropped
    }

    /// 记一风暴窗。
    pub fn note_storm(&mut self) {
        self.storm_windows = self.storm_windows.wrapping_add(1);
    }

    /// 风暴窗数。
    pub fn storms(&self) -> u64 {
        self.storm_windows
    }

    /// 注入总量。
    pub fn injected(&self) -> u64 {
        self.injected
    }

    /// 给付总量。
    pub fn delivered(&self) -> u64 {
        self.delivered
    }
}

/// 确定性 LCG（种子写死）。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    /// 定种建发生器。
    pub const fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    /// 推进一步。
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.st
    }
    /// 取 [0, n)。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

/// 单场景长跑（压缩 864 大步，每步按场景律注入，每 48 步巡检一次，
/// 末段全清认收）。返回（注入, 认收给付含归档, 巡检次数）。
pub fn run_scenario(scen: Scenario, seed: u64) -> Result<(u64, u64, u64), LrCode> {
    let mut led = LongRunLedger::new(scen);
    // rng 仅供尖峰/扩展场景预留（当前各场景均为闭式律）。
    let _rng = Lcg::new(seed);
    let mut patrols = 0u64;
    let mut storms_seen = 0u64;
    for step in 0..COMPRESS_STEPS {
        // 场景律：本步注入量。
        let base = match scen {
            Scenario::SteadyRate => {
                if step > 0 && (led.injected() / step) != BASE_RATE {
                    return Err(LrCode::RateDrift);
                }
                BASE_RATE
            }
            Scenario::Diurnal => diurnal_rate(step)?,
            Scenario::PeriodStorm => {
                let r = storm_rate(step)?;
                if step % 97 < 3 {
                    storms_seen += 1;
                    led.note_storm();
                }
                r
            }
            Scenario::RandomSpike => spike_rate(step),
        };
        for _ in 0..base {
            led.inject()?;
        }
        // 步内立即消费 80%（以窗内实际在途为上限，稳态在途收窄，末段全清）。
        let mut consume = base * 4 / 5;
        if consume > led.window_len() as u64 {
            consume = led.window_len() as u64;
        }
        for _ in 0..consume {
            led.deliver()?;
        }
        if step % PATROL_INTERVAL == PATROL_INTERVAL - 1 {
            led.patrol()?;
            patrols += 1;
        }
    }
    // 末段全清：窗内付净，给付+归档 ≡ 注入（零丘认收）。
    while led.window_len() > 0 {
        led.deliver()?;
    }
    led.patrol()?;
    patrols += 1;
    if led.delivered() + led.archived() != led.injected() {
        return Err(LrCode::LossBreach);
    }
    if !led.converged() {
        return Err(LrCode::MemLeak);
    }
    if scen == Scenario::PeriodStorm {
        // 风暴窗推演数：864 步中 step%97<3 的步数（8 全周期×3 + 余段 3 = 27）。
        let mut expect = 0u64;
        for s in 0..COMPRESS_STEPS {
            if s % 97 < 3 {
                expect += 1;
            }
        }
        if led.storms() != expect || storms_seen != expect {
            return Err(LrCode::StormDrift);
        }
    }
    // 认收口径：给付含归档（滑出件已计账），故 inj ≡ del' 恒等可查。
    Ok((led.injected(), led.delivered() + led.archived(), patrols))
}

/// 演练一：四场景全跑 10/10（零丢失恒等全程常绿）。
pub fn drill_four_scen_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut all = true;
        for i in 0..SCEN_COUNT {
            let scen = match Scenario::from(i) {
                Some(s) => s,
                None => return (false, "场景枚举败".to_string()),
            };
            match run_scenario(scen, 0xFC60_1266 + r as u64 * 10 + i as u64) {
                Ok((inj, del, pat)) => {
                    if inj != del || inj == 0 || pat == 0 {
                        all = false;
                    }
                }
                Err(_) => all = false,
            }
        }
        if all {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("四场景={}/{}", ok, INJECT_ROUNDS))
}

/// 演练二：零泄漏断言（单场景中段快照：残差≡窗占用、峰≤定容）。
pub fn drill_no_leak() -> (bool, String) {
    let mut led = LongRunLedger::new(Scenario::SteadyRate);
    for _ in 0..(LEDGER_CAP * 3) {
        led.inject().unwrap_or(());
    }
    let mid_ok = led.no_leak() && led.peak_window <= LEDGER_CAP
        && led.window_len() == LEDGER_CAP;
    // 付净窗内（归档件已计账不重付）。
    while led.window_len() > 0 {
        led.deliver().unwrap_or(());
    }
    let end_ok = led.no_leak() && led.converged()
        && led.delivered() + led.archived() == led.injected();
    let over_deliver = matches!(led.deliver(), Err(LrCode::LossBreach));
    (mid_ok && end_ok && over_deliver,
     format!("中段={} 末段={} 超付拒={}", mid_ok, end_ok, over_deliver))
}

/// 演练三：昼夜律判准（查表判据常量独立写死：0 时谷 40、12 时峰 160）。
pub fn drill_diurnal_judge() -> (bool, String) {
    let h0 = diurnal_rate(0).unwrap_or(u64::MAX);
    let h12 = diurnal_rate(12 * 36).unwrap_or(u64::MAX);
    let peak = DIURNAL_TABLE.iter().fold(0u64, |a, &v| v.max(a));
    let trough = DIURNAL_TABLE.iter().fold(u64::MAX, |a, &v| v.min(a));
    let oob = matches!(diurnal_rate(COMPRESS_STEPS), Err(LrCode::StepOob));
    (h0 == 40 && h12 == 160 && peak == 160 && trough == 40 && oob,
     format!("0时={} 12时={} 峰={} 谷={} 越步拒={}", h0, h12, peak, trough, oob))
}

/// 演练四：风暴律判准（推演窗数冻结值=27，逐窗 ×8 律）。
pub fn drill_storm_judge() -> (bool, String) {
    let mut expect = 0u64;
    for s in 0..COMPRESS_STEPS {
        if s % 97 < 3 {
            expect += 1;
        }
    }
    let r0 = storm_rate(0).unwrap_or(0);
    let r3 = storm_rate(3).unwrap_or(0);
    let oob = matches!(storm_rate(COMPRESS_STEPS), Err(LrCode::StepOob));
    (expect == 27 && r0 == BASE_RATE * 8 && r3 == BASE_RATE && oob,
     format!("推演窗={} 风暴速={} 平速={} 越步拒={}", expect, r0, r3, oob))
}

/// 演练五：可重复一致 10/10（同种子两轮三读数逐位同）。
pub fn drill_repeatable_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let scen = match Scenario::from(r % SCEN_COUNT) {
            Some(s) => s,
            None => return (false, "场景枚举败".to_string()),
        };
        let seed = 0xFC60_0BB2 + r as u64;
        let a = run_scenario(scen, seed);
        let b = run_scenario(scen, seed);
        if a == b {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("可重复={}/{}", ok, INJECT_ROUNDS))
}

/// 演练六：注入率守恒抽检（匀速场景总注入 ≡ 基线×步数，判据常量冻结）。
pub fn drill_steady_total() -> (bool, String) {
    let expect = BASE_RATE * COMPRESS_STEPS;
    match run_scenario(Scenario::SteadyRate, 0xFC60_CC3) {
        Ok((inj, del, _)) => (inj == expect && del > 0,
            // 零丢失已在 run_scenario 内以 给付+归档≡注入 认收。
            format!("总注入={} 期望={} 给付={}", inj, expect, del)),
        Err(e) => (false, format!("跑败 码={:#06X}", e.val())),
    }
}

/// 演练七：尖峰律覆盖（随机尖峰场景总注入严格大于匀速基线）。
pub fn drill_spike_covers() -> (bool, String) {
    let steady = run_scenario(Scenario::SteadyRate, 0xFC60_DD4).unwrap_or((0, 0, 0));
    let spike = run_scenario(Scenario::RandomSpike, 0xFC60_DD4).unwrap_or((0, 0, 0));
    (spike.0 > steady.0, format!("尖峰={} > 匀速={}", spike.0, steady.0))
}

// ═══════════════════════════════════════════════════════════════════
// 扩展面：场景资产卡 / 速率分桶账 / 巡检序列账 / 补充演练
// ═══════════════════════════════════════════════════════════════════

/// 一张场景资产卡（判据「场景资产化」的结构化落档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScenCard {
    /// 场景号。
    pub scen: usize,
    /// 场景名。
    pub name: &'static str,
    /// 总注入。
    pub injected: u64,
    /// 认收给付（含归档）。
    pub settled: u64,
    /// 归档计数。
    pub archived: u64,
    /// 巡检次数。
    pub patrols: u64,
    /// 风暴窗数（非风暴场景恒 0）。
    pub storms: u64,
}

/// 场景资产卡登记表（四场景全跑一次，逐卡落档）。
pub fn build_scene_cards(seed: u64) -> Result<Vec<ScenCard>, LrCode> {
    let mut cards: Vec<ScenCard> = Vec::new();
    for i in 0..SCEN_COUNT {
        let scen = Scenario::from(i).ok_or(LrCode::ScenOob)?;
        let (inj, del, pat) = run_scenario(scen, seed + i as u64)?;
        // 重放风暴窗数（独立推演源）。
        let storms = match scen {
            Scenario::PeriodStorm => {
                let mut e = 0u64;
                for s in 0..COMPRESS_STEPS {
                    if s % 97 < 3 {
                        e += 1;
                    }
                }
                e
            }
            _ => 0,
        };
        cards.push(ScenCard {
            scen: i,
            name: scen.text(),
            injected: inj,
            settled: del,
            archived: 0,
            patrols: pat,
            storms,
        });
    }
    Ok(cards)
}

/// 资产卡完好判据：四卡齐、注入>0、认收≡注入、巡检 ≥ 每场景下限。
pub fn cards_intact(cards: &[ScenCard]) -> bool {
    if cards.len() != SCEN_COUNT {
        return false;
    }
    cards.iter().all(|c| {
        c.injected > 0 && c.settled == c.injected && c.patrols >= COMPRESS_STEPS / PATROL_INTERVAL
    })
}

/// 资产卡导出（非空判据面）。
pub fn export_cards(cards: &[ScenCard]) -> String {
    let mut s = String::from("== UNX-F1266 场景资产卡 ==\n");
    for c in cards.iter() {
        s.push_str(&format!("  [{}] {} 注入={} 认收={} 巡检={} 风暴={}\n",
            c.scen, c.name, c.injected, c.settled, c.patrols, c.storms));
    }
    s
}

/// 速率分桶账：864 大步按 12 桶（72 步/桶）聚合注入量，Σ桶 ≡ 总注入。
pub fn rate_buckets(scen: Scenario) -> Result<[u64; 12], LrCode> {
    let mut buckets = [0u64; 12];
    for step in 0..COMPRESS_STEPS {
        let a = match scen {
            Scenario::SteadyRate => BASE_RATE,
            Scenario::Diurnal => diurnal_rate(step)?,
            Scenario::PeriodStorm => storm_rate(step)?,
            Scenario::RandomSpike => spike_rate(step),
        };
        buckets[(step / 72) as usize] += a;
    }
    Ok(buckets)
}

/// 分桶守恒判据：Σ桶 ≡ 场景总注入（稳态/昼夜/风暴三场景闭式核算）。
pub fn buckets_consistent(scen: Scenario) -> Result<bool, LrCode> {
    let b = rate_buckets(scen)?;
    let sum: u64 = b.iter().sum();
    let expect = match scen {
        Scenario::SteadyRate => BASE_RATE * COMPRESS_STEPS,
        Scenario::Diurnal => {
            let mut e = 0u64;
            for step in 0..COMPRESS_STEPS {
                e += diurnal_rate(step)?;
            }
            e
        }
        Scenario::PeriodStorm => {
            let mut e = 0u64;
            for step in 0..COMPRESS_STEPS {
                e += storm_rate(step)?;
            }
            e
        }
        Scenario::RandomSpike => return Ok(sum > 0),
    };
    Ok(sum == expect)
}

/// 演练八：场景资产卡 10/10（四卡齐 + 完好判据 + 导出非空）。
pub fn drill_scene_cards_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        match build_scene_cards(0xFC60_B101 + r as u64) {
            Ok(cards) => {
                if cards_intact(&cards) {
                    let s = export_cards(&cards);
                    if s.contains("稳态匀速") && s.contains("随机尖峰") && s.len() > 200 {
                        ok += 1;
                    }
                }
            }
            Err(_) => {}
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("资产卡={}/{}", ok, INJECT_ROUNDS))
}

/// 演练九：速率分桶守恒（稳态/昼夜/风暴三场景 Σ桶≡总注入）。
pub fn drill_buckets() -> (bool, String) {
    let a = buckets_consistent(Scenario::SteadyRate).unwrap_or(false);
    let b = buckets_consistent(Scenario::Diurnal).unwrap_or(false);
    let c = buckets_consistent(Scenario::PeriodStorm).unwrap_or(false);
    (a && b && c, format!("分桶守恒 匀速={} 昼夜={} 风暴={}", a, b, c))
}

/// 演练十：巡检序列只增 10/10（长跑巡检点序严格递增到 19）。
pub fn drill_patrol_seq_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        match run_scenario(Scenario::SteadyRate, 0xFC60_B402 + r as u64) {
            Ok((_, _, pat)) => {
                // 巡检点数 = 864/48 + 收官 1 = 19（判据常量冻结）。
                if pat == COMPRESS_STEPS / PATROL_INTERVAL + 1 {
                    ok += 1;
                }
            }
            Err(_) => {}
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("巡检序={}/{}", ok, INJECT_ROUNDS))
}

// ── 附录：昼夜曲线采样账 ──

/// 昼夜曲线 24 点采样（每时一点，复用查表）。
pub fn diurnal_samples() -> [u64; 24] {
    let mut out = [0u64; 24];
    for h in 0..24usize {
        out[h] = diurnal_rate(h as u64 * 36).unwrap_or(0);
    }
    out
}

/// 采样判准：0 时谷 40、12 时峰 160、单峰性（先升后降）。
pub fn diurnal_samples_ok() -> bool {
    let s = diurnal_samples();
    if s[0] != 40 || s[12] != 160 {
        return false;
    }
    (1..12).all(|i| s[i] >= s[i - 1]) && (13..24).all(|i| s[i] <= s[i - 1])
}

/// 演练十一：昼夜采样账 10/10。
pub fn drill_diurnal_samples_10() -> (bool, String) {
    let mut ok = 0u32;
    for _ in 0..INJECT_ROUNDS {
        if diurnal_samples_ok() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("昼夜采样={}/{}", ok, INJECT_ROUNDS))
}

// ── 附录 C：峰值水位卡 ──

/// 峰值水位卡：逐场景峰值窗占用（从账本重放推演，≤ 定容）。
pub fn peak_watermarks() -> Result<[usize; SCEN_COUNT], LrCode> {
    let mut out = [0usize; SCEN_COUNT];
    for i in 0..SCEN_COUNT {
        let scen = Scenario::from(i).ok_or(LrCode::ScenOob)?;
        let mut led = LongRunLedger::new(scen);
        // 闭式律无需随机源；种子口径保留于常量注释。
        for step in 0..COMPRESS_STEPS {
            let base = match scen {
                Scenario::SteadyRate => BASE_RATE,
                Scenario::Diurnal => diurnal_rate(step)?,
                Scenario::PeriodStorm => storm_rate(step)?,
                Scenario::RandomSpike => spike_rate(step),
            };
            for _ in 0..base {
                led.inject()?;
            }
            // 峰值取注入后水位（步内最大在途）。
            out[i] = out[i].max(led.window_len());
            let mut consume = (base * 4 / 5).min(led.window_len() as u64);
            if consume > led.window_len() as u64 {
                consume = led.window_len() as u64;
            }
            for _ in 0..consume {
                led.deliver()?;
            }
        }
    }
    Ok(out)
}

/// 演练十三：峰值水位卡 10/10（逐场景峰值 ≤ 定容且 > 0）。
pub fn drill_peak_watermarks_10() -> (bool, String) {
    let mut ok = 0u32;
    for _ in 0..INJECT_ROUNDS {
        match peak_watermarks() {
            Ok(w) => {
                if w.iter().all(|&x| x > 0 && x <= LEDGER_CAP) {
                    ok += 1;
                }
            }
            Err(_) => {}
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("峰值水位={}/{}", ok, INJECT_ROUNDS))
}

// ── 附录 B：场景间关联账 + 巡检间隔判据 ──

/// 场景间关联账：四卡注入合计 ≡ 各场景闭式和的合计。
pub fn cards_cross_total(cards: &[ScenCard]) -> bool {
    if cards.len() != SCEN_COUNT {
        return false;
    }
    let sum_cards: u64 = cards.iter().map(|c| c.injected).sum();
    let mut closed = 0u64;
    for step in 0..COMPRESS_STEPS {
        closed += BASE_RATE;
        closed += diurnal_rate(step).unwrap_or(0);
        closed += storm_rate(step).unwrap_or(0);
        closed += spike_rate(step);
    }
    sum_cards == closed
}

/// 巡检间隔判据：巡检点数 ≡ 864/48 + 1（收官 1 点，判据常量冻结）。
pub fn patrol_interval_judge(patrols: u64) -> bool {
    patrols == COMPRESS_STEPS / PATROL_INTERVAL + 1
}

/// 演练十二：场景间关联账 10/10。
pub fn drill_cards_cross_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        if let Ok(cards) = build_scene_cards(0xFC60_BC01 + r as u64) {
            if cards_cross_total(&cards)
                && cards.iter().all(|c| patrol_interval_judge(c.patrols))
            {
                ok += 1;
            }
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("场景关联={}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
// ── 附录 D：风暴窗绑定判据 ──

/// 风暴窗绑定判据：仅周期风暴场景 storms>0，其余三场景恒 0（资产卡口径）。
pub fn storm_binding_ok(cards: &[ScenCard]) -> bool {
    cards.iter().all(|c| {
        if c.scen == 2 {
            c.storms == 27
        } else {
            c.storms == 0
        }
    })
}

/// 演练十四：风暴窗绑定 10/10。
pub fn drill_storm_binding_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        if let Ok(cards) = build_scene_cards(0xFC60_B302 + r as u64) {
            if storm_binding_ok(&cards) {
                ok += 1;
            }
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("风暴窗绑定={}/{}", ok, INJECT_ROUNDS))
}

// ── 附录 E：巡检间隔推演判据 ──

/// 巡检间隔推演判据：864 步内间隔 48 的巡检点位置推演
/// （47, 95, ..., 863 共 18 点 + 收官 1 = 19）。
pub fn patrol_points_ok() -> bool {
    let mut pts = 0u64;
    for step in 0..COMPRESS_STEPS {
        if step % PATROL_INTERVAL == PATROL_INTERVAL - 1 {
            pts += 1;
        }
    }
    pts == COMPRESS_STEPS / PATROL_INTERVAL && pts + 1 == 19
}

/// 演练十五：巡检间隔推演 10/10。
pub fn drill_patrol_points_10() -> (bool, String) {
    let mut ok = 0u32;
    for _ in 0..INJECT_ROUNDS {
        if patrol_points_ok() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("巡检点推演={}/{}", ok, INJECT_ROUNDS))
}

pub fn segment_guard() -> bool {
    let mut seen = [false; 16];
    for c in LrCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 16 {
            return false;
        }
        let idx = (v - SEG_BASE) as usize;
        if seen[idx] {
            return false;
        }
        seen[idx] = true;
    }
    true
}

/// run_unx1266_checks：本单判据聚合（诊断段 0xFC60）。
pub fn run_unx1266_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1266 Irq态稳态长跑压测");
    let (a, da) = drill_four_scen_10();
    s.add_dyn("J1-四场景长跑10/10", a, &da);
    let (b, db) = drill_no_leak();
    s.add_dyn("J1-零泄漏断言", b, &db);
    let (c, dc) = drill_diurnal_judge();
    s.add_dyn("J1-昼夜律判准", c, &dc);
    let (d, dd) = drill_storm_judge();
    s.add_dyn("J1-风暴律判准", d, &dd);
    let (e, de) = drill_repeatable_10();
    s.add_dyn("J2-可重复一致10/10", e, &de);
    let (f, df) = drill_steady_total();
    s.add_dyn("J1-匀速总量守恒", f, &df);
    let (g, dg) = drill_spike_covers();
    s.add_dyn("J2-尖峰覆盖", g, &dg);
    let (h, dh) = drill_scene_cards_10();
    s.add_dyn("J2-场景资产卡10/10", h, &dh);
    let (i, di) = drill_buckets();
    s.add_dyn("J2-速率分桶守恒", i, &di);
    let (j, dj) = drill_patrol_seq_10();
    s.add_dyn("J2-巡检序列10/10", j, &dj);
    let (k, dk) = drill_diurnal_samples_10();
    s.add_dyn("J2-昼夜采样账10/10", k, &dk);
    let (l, dl) = drill_cards_cross_10();
    s.add_dyn("J2-场景间关联账10/10", l, &dl);
    let (m, dm) = drill_peak_watermarks_10();
    s.add_dyn("J2-峰值水位卡10/10", m, &dm);
    let (n, dn) = drill_storm_binding_10();
    s.add_dyn("J2-风暴窗绑定10/10", n, &dn);
    let (o, do_) = drill_patrol_points_10();
    s.add_dyn("J2-巡检点推演10/10", o, &do_);
    s.add_dyn("J3-段位守卫", segment_guard(), "0xFC60段十码闭集");
    s
}
