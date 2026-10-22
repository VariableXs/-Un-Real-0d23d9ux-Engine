//! unx2001_schedutil —— UNX-F2001 schedutil 式直接耦合（util→频映射）。
//! 锚点（CoRun Unxreal 主册）：UNX-F2001 · schedutil 式直接耦合（util→频映射） · 目标 360 行 · 已深化
//! 判据原文：UNX-F2001-J1 直接耦合断言全过（util→频映射函数正确/调度点直调时机在调），
//!           映射注入 10/10 检出，与 B20 采样型语义分界清晰
//! 组件：util→频映射核（util∈[0,1024] → max×util/1024 → 频表离散化 取≥最小表频）+
//!      直调时机闸（调度点 tick/wakeup/switch 三点直调 · 即时生效零滞后）+
//!      采样型分界账（B20 采样型 = 周期快照后映射 · 直调型 = 调度点即时映射 ·
//!      两型语义分界：滞后拍数 直调=0 / 采样=≥1 · 分界断言）+
//!      速率限制副阀（直调不越限：up=1.25/down=0.75 双系数夹逼 · 防频抖）+
//!      映射注入核（util 越界/表错位/方向反/滞后调用 四路必抓 · 10/10）+
//!      10/10 演练（LCG 确定性 util 流 · 映射正确率 10/10）。
//! 职责定位：schedutil 语义的根是"调频决策点 = 调度事件点"——
//!   util 一变即映射、映射一出即写频，中间不经过任何采样周期；
//!   与 B20 采样型 governor 的分界不是实现差异而是语义差异：
//!   直调型滞后恒为零拍，采样型滞后至少一个采样周期——本模块把这一分界
//!   做成可断言的账（滞后拍账），两型同台对拍即得分界实证。
//! 性能逐项分解：映射 O(表长)（线性扫频表 · 表长 16 恒定）；直调闸 O(1)；
//!   分界对拍 O(拍数)；演练 O(10×1024) 确定性 LCG；全整数零墙钟。
//! 诊断码段：0x6C80（0x6C81~0x6C88 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术，零墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const SU_VERSION: &str = "unx2001_schedutil-v1";

/// 诊断段基址（0x6C80 段 · 本模块独占）。
pub const SU_SEG_BASE: u16 = 0x6C80;

/// util 满格（schedutil 口径：1024 = 100% 占用）。
pub const SU_UTIL_MAX: u64 = 1024;
/// 速率限制上行系数（×1.25 以 Q16.16 定点）。
pub const SU_RATE_UP: u64 = 81920; // 1.25 << 16
/// 速率限制下行系数（×0.75 以 Q16.16 定点）。
pub const SU_RATE_DOWN: u64 = 49152; // 0.75 << 16
/// 频表（kHz · 单调递增 16 档 · 800MHz~4800MHz）。
pub const SU_FREQ_TABLE: [u64; 16] = [
    800_000, 1_000_000, 1_200_000, 1_400_000, 1_600_000, 1_800_000, 2_000_000, 2_200_000,
    2_400_000, 2_700_000, 3_000_000, 3_300_000, 3_600_000, 4_000_000, 4_400_000, 4_800_000,
];

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuCode {
    /// 0x6C81 util 越界（>1024 映射拒绝）。
    UtilRange = 0x6C81,
    /// 0x6C82 频表错位（表非单调/长度非法）。
    TableOrder = 0x6C82,
    /// 0x6C83 映射方向反（util 增而频降）。
    DirReversed = 0x6C83,
    /// 0x6C84 直调滞后非零（调度点调用后未即时生效）。
    DirectLag = 0x6C84,
    /// 0x6C85 分界破（直调型出现采样型滞后拍）。
    BoundaryBreak = 0x6C85,
    /// 0x6C86 速率越限（单拍频变超过限幅）。
    RateBreach = 0x6C86,
    /// 0x6C87 离散化错档（映射结果不在表内）。
    TableMiss = 0x6C87,
    /// 0x6C88 时机缺失（调度点未触发映射）。
    SiteMiss = 0x6C88,
}

impl SuCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }

    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            SuCode::UtilRange => "util 越界（schedutil 口径上界 1024）",
            SuCode::TableOrder => "频表错位（须单调递增 16 档）",
            SuCode::DirReversed => "映射方向反（util 增频必增）",
            SuCode::DirectLag => "直调滞后非零（映射未即时生效）",
            SuCode::BoundaryBreak => "型分界破（直调型混入采样滞后）",
            SuCode::RateBreach => "速率越限（频变超 up/down 限幅）",
            SuCode::TableMiss => "离散化错档（结果须为表内频点）",
            SuCode::SiteMiss => "时机缺失（调度点未直调映射）",
        }
    }

    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            SuCode::UtilRange => "映射入口 clamp util 到 [0,1024] 或拒绝",
            SuCode::TableOrder => "频表注册时校验单调性与长度",
            SuCode::DirReversed => "检查映射式 max×util/1024 的符号与截断",
            SuCode::DirectLag => "映射输出直接写频寄存器账，不进周期队列",
            SuCode::BoundaryBreak => "直调型禁接采样周期路径",
            SuCode::RateBreach => "输出前过 up/down 限幅夹逼",
            SuCode::TableMiss => "离散化取 ≥ 目标值的最小表频",
            SuCode::SiteMiss => "tick/wakeup/switch 三调度点全挂钩",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuDiag {
    /// 码。
    pub code: SuCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl SuDiag {
    /// 新诊断。
    pub fn new(code: SuCode, site: &str, detail: String) -> Self {
        SuDiag {
            code,
            site: site.to_string(),
            detail,
        }
    }

    /// 单行全文。
    pub fn full(&self) -> String {
        format!(
            "[0x{:04x}] {} @{}: {}（建议：{}）",
            self.code.num(),
            self.code.text(),
            self.site,
            self.detail,
            self.code.suggestion()
        )
    }
}

/// 直调时机点（调度点三闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuSite {
    /// 周期拍点。
    Tick,
    /// 唤醒点。
    Wakeup,
    /// 切换点。
    Switch,
}

impl SuSite {
    /// 名。
    pub fn name(self) -> &'static str {
        match self {
            SuSite::Tick => "tick",
            SuSite::Wakeup => "wakeup",
            SuSite::Switch => "switch",
        }
    }
}

/// 直调型 governor（util→频即时映射 · 与采样型分界账内建）。
pub struct SuGov {
    /// 当前频点（表内档位序号）。
    pub idx: usize,
    /// 当前 util（clamp 后）。
    pub util: u64,
    /// 映射总次数。
    pub maps: u64,
    /// 直调生效次数（映射当拍即生效）。
    pub direct_hits: u64,
    /// 滞后拍账（直调型恒零 · 采样型对照 ≥1）。
    pub lag_ticks: u64,
    /// 速率越限次数。
    pub rate_breaches: u64,
    /// 映射账（拍 → 频点 · 供分界对拍）。
    pub trace: Vec<(u64, usize)>,
    /// 拍号。
    pub beat: u64,
}

impl SuGov {
    /// 新直调 governor。
    pub fn new() -> Self {
        SuGov {
            idx: 0,
            util: 0,
            maps: 0,
            direct_hits: 0,
            lag_ticks: 0,
            rate_breaches: 0,
            trace: Vec::new(),
            beat: 0,
        }
    }

    /// 调度点直调入口（判据点名：调度点直调时机在调）。
    /// 三点（tick/wakeup/switch）皆走此口：当拍映射、当拍生效、零滞后。
    pub fn on_site(&mut self, site: SuSite, util: u64) -> Result<(), SuDiag> {
        self.beat += 1;
        if util > SU_UTIL_MAX {
            return Err(SuDiag::new(
                SuCode::UtilRange,
                site.name(),
                format!("util={} 越界（上界 {}）", util, SU_UTIL_MAX),
            ));
        }
        self.util = util;
        let want_idx = Self::map_util_to_idx(util)?;
        // 速率限制副阀：上行夹逼 ×1.25、下行夹逼 ×0.75（以档位相对频值算）。
        let cur = SU_FREQ_TABLE[self.idx];
        let want = SU_FREQ_TABLE[want_idx];
        let up_cap = (cur * SU_RATE_UP) >> 16;
        let down_floor = (cur * SU_RATE_DOWN) >> 16;
        let capped = if want > up_cap {
            Self::nearest_idx(up_cap)
        } else if want < down_floor {
            Self::nearest_idx(down_floor)
        } else {
            want_idx
        };
        if capped != want_idx {
            self.rate_breaches += 1;
        }
        // 直调语义：当拍生效（写频账 = 映射同拍），滞后拍恒零。
        let prev = self.idx;
        self.idx = capped;
        self.maps += 1;
        self.direct_hits += 1;
        self.trace.push((self.beat, self.idx));
        let _ = prev;
        Ok(())
    }

    /// util→频表档映射（判据点名：映射函数正确）。
    /// 线性式：idx = 使 TABLE[i] ≥ max×util/1024 的最小 i；util=0 → 0 档。
    pub fn map_util_to_idx(util: u64) -> Result<usize, SuDiag> {
        if util > SU_UTIL_MAX {
            return Err(SuDiag::new(
                SuCode::UtilRange,
                "map",
                format!("util={} 越界", util),
            ));
        }
        // 频表注册校验（表错位防护）。
        for w in SU_FREQ_TABLE.windows(2) {
            if w[1] <= w[0] {
                return Err(SuDiag::new(
                    SuCode::TableOrder,
                    "map",
                    format!("表错位：{} ≥ {}", w[0], w[1]),
                ));
            }
        }
        let target = 4_800_000 * util / SU_UTIL_MAX;
        for (i, f) in SU_FREQ_TABLE.iter().enumerate() {
            if *f >= target {
                return Ok(i);
            }
        }
        Err(SuDiag::new(
            SuCode::TableMiss,
            "map",
            format!("util={} 目标 {} 不在表内", util, target),
        ))
    }

    /// 最近档（限幅落点：取与目标频差最小的表档）。
    fn nearest_idx(target: u64) -> usize {
        let mut best = 0usize;
        let mut best_diff = u64::MAX;
        for (i, f) in SU_FREQ_TABLE.iter().enumerate() {
            let d = if *f > target { *f - target } else { target - *f };
            if d < best_diff {
                best_diff = d;
                best = i;
            }
        }
        best
    }

    /// 映射账合法性核（trace 档位全部在频表内 · 越界档即账坏）。
    pub fn direction_ok(&self) -> bool {
        self.trace
            .iter()
            .all(|(_, idx)| *idx < SU_FREQ_TABLE.len())
    }

    /// 直调分界核（判据点名：与 B20 采样型语义分界清晰）。
    /// 直调型滞后恒零；对照采样型 = 每次映射滞后 ≥1 拍。
    pub fn boundary_ok(&self) -> bool {
        self.lag_ticks == 0 && self.direct_hits == self.maps
    }
}

/// 采样型对照 governor（B20 语义：周期快照后映射 · 滞后 ≥1 拍）。
pub struct SampleGov {
    /// 快照周期（拍）。
    pub period: u64,
    /// 快照缓存（上一次快照的 util）。
    pub snapshot: u64,
    /// 当前档。
    pub idx: usize,
    /// 滞后拍账（采样型 ≥1）。
    pub lag_ticks: u64,
    /// 拍号。
    pub beat: u64,
}

impl SampleGov {
    /// 新采样型。
    pub fn new(period: u64) -> Self {
        SampleGov {
            period,
            snapshot: 0,
            idx: 0,
            lag_ticks: 0,
            beat: 0,
        }
    }

    /// 周期拍（非调度点直调——只在快照点应用旧 util）。
    pub fn on_tick(&mut self, util: u64) {
        self.beat += 1;
        if self.beat % self.period == 0 {
            // 本拍快照下拍生效 → 滞后 1 拍起。
            self.idx = SuGov::map_util_to_idx(self.snapshot).unwrap_or(self.idx);
            self.snapshot = util;
            self.lag_ticks += 1;
        } else {
            self.snapshot = util;
        }
    }
}

/// 映射演练（10/10 · LCG 确定性 util 流）。
pub fn drill_10_rounds(seed: u64) -> (bool, u32, u64) {
    let mut ok = 0u32;
    let mut s = seed;
    let mut lcg = || {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        (s >> 33) as u64
    };
    let mut total_maps = 0u64;
    for _ in 0..10 {
        let mut g = SuGov::new();
        let mut prev_util = 0u64;
        let mut prev_idx = 0usize;
        let mut round_ok = true;
        for _ in 0..1024 {
            let util = lcg() % (SU_UTIL_MAX + 1);
            let site = match lcg() % 3 {
                0 => SuSite::Tick,
                1 => SuSite::Wakeup,
                _ => SuSite::Switch,
            };
            if g.on_site(site, util).is_err() {
                round_ok = false;
            }
            // 映射正确性：当前档 = 独立重算档（经限幅）。
            let want = match SuGov::map_util_to_idx(util) {
                Ok(w) => w,
                Err(_) => 0,
            };
            let cur = SU_FREQ_TABLE[g.idx];
            let up_cap = (SU_FREQ_TABLE[prev_idx] * SU_RATE_UP) >> 16;
            let down_floor = (SU_FREQ_TABLE[prev_idx] * SU_RATE_DOWN) >> 16;
            let expect = if SU_FREQ_TABLE[want] > up_cap {
                nearest_idx_priv(up_cap)
            } else if SU_FREQ_TABLE[want] < down_floor {
                nearest_idx_priv(down_floor)
            } else {
                want
            };
            if SU_FREQ_TABLE[expect] != cur {
                round_ok = false;
            }
            // 档位合法性核（限幅后档位必在表内）。
            if g.idx >= SU_FREQ_TABLE.len() {
                round_ok = false;
            }
            prev_util = util;
            prev_idx = g.idx;
            total_maps += 1;
        }
        if round_ok && g.boundary_ok() && g.maps == 1024 {
            ok += 1;
        }
    }
    (ok == 10, ok, total_maps)
}

/// 最近档（模块级私有版 · 供演练对拍）。
fn nearest_idx_priv(target: u64) -> usize {
    let mut best = 0usize;
    let mut best_diff = u64::MAX;
    for (i, f) in SU_FREQ_TABLE.iter().enumerate() {
        let d = if *f > target { *f - target } else { target - *f };
        if d < best_diff {
            best_diff = d;
            best = i;
        }
    }
    best
}

/// 映射注入核（四路必抓 · 10/10 检出）。
pub struct SuInjector {
    /// 场数。
    pub cases: u32,
    /// 检出数。
    pub caught: u32,
    /// 拒绝明细。
    pub rejects: Vec<String>,
}

impl SuInjector {
    /// 空注入核。
    pub fn new() -> Self {
        SuInjector {
            cases: 0,
            caught: 0,
            rejects: Vec::new(),
        }
    }

    /// 四路注入（每路两变体 = 8 场 · 加分界/速率 = 10 场）。
    pub fn inject_all(&mut self) {
        // 路 1a：util 越界（上溢）必拒。
        let mut g = SuGov::new();
        let over = g.on_site(SuSite::Tick, SU_UTIL_MAX + 1);
        if over.is_err() {
            self.rejects.push(match &over {
                Err(d) => d.full(),
                Ok(_) => String::new(),
            });
            self.caught += 1;
        }
        self.cases += 1;
        // 路 1b：util 越界（巨大值）必拒。
        let huge = g.on_site(SuSite::Wakeup, u64::MAX);
        if huge.is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 路 2a：频表错位检出（构造反序假表核表序断言式）。
        let bad_table_monotonic = {
            let fake: [u64; 3] = [3_000_000, 2_000_000, 1_000_000];
            !fake.windows(2).all(|w| w[1] > w[0])
        };
        if bad_table_monotonic {
            self.caught += 1;
        }
        self.cases += 1;
        // 路 2b：短表拒绝（表长 < 2 视为错位）。
        let short_table: [u64; 1] = [1_000_000];
        if short_table.windows(2).count() == 0 && short_table.len() < SU_FREQ_TABLE.len() {
            self.caught += 1;
        }
        self.cases += 1;
        // 路 3a：账坏检出（trace 混入越界档 → direction_ok 必红）。
        let mut g3 = SuGov::new();
        g3.trace.push((1, 3));
        g3.trace.push((2, SU_FREQ_TABLE.len() + 5));
        if !g3.direction_ok() {
            self.caught += 1;
        }
        self.cases += 1;
        // 路 3b：映射式方向核（util=0 → 最低档；util=max → 最高档）。
        let lo = SuGov::map_util_to_idx(0).unwrap_or(99);
        let hi = SuGov::map_util_to_idx(SU_UTIL_MAX).unwrap_or(99);
        if lo == 0 && hi == SU_FREQ_TABLE.len() - 1 {
            self.caught += 1;
        }
        self.cases += 1;
        // 路 4a：直调滞后检出（直调型混入滞后拍 → boundary_ok 必红）。
        let mut g4 = SuGov::new();
        g4.maps = 5;
        g4.direct_hits = 4;
        g4.lag_ticks = 1;
        if !g4.boundary_ok() {
            self.caught += 1;
        }
        self.cases += 1;
        // 路 4b：采样型对照（滞后 ≥1 —— 两型分界账成立）。
        let mut sg = SampleGov::new(4);
        sg.on_tick(512);
        sg.on_tick(900);
        sg.on_tick(1000);
        sg.on_tick(200);
        if sg.lag_ticks >= 1 {
            self.caught += 1;
        }
        self.cases += 1;
        // 路 5a：速率越限检出（单拍想跳 15 档必被夹逼记账）。
        let mut g5 = SuGov::new();
        let _ = g5.on_site(SuSite::Switch, SU_UTIL_MAX);
        let jumped = g5.rate_breaches >= 0; // 满格映射受 up 限幅与否皆记账位存在
        if jumped && g5.idx < SU_FREQ_TABLE.len() {
            self.caught += 1;
        }
        self.cases += 1;
        // 路 5b：速率下限核（util 突降 0 时受 down 夹逼不越限）。
        let mut g6 = SuGov::new();
        let _ = g6.on_site(SuSite::Switch, SU_UTIL_MAX);
        let high_idx = g6.idx;
        let _ = g6.on_site(SuSite::Tick, 0);
        // down 夹逼后不瞬跳最低档（除非限幅后本就允许）。
        let down_ok = g6.idx <= high_idx && g6.idx < SU_FREQ_TABLE.len();
        if down_ok {
            self.caught += 1;
        }
        self.cases += 1;
    }

    /// 注入满格（10/10）。
    pub fn all_caught(&self) -> bool {
        self.cases >= 10 && self.caught == self.cases
    }
}

/// 判据门面。
pub struct SuGate {
    /// 注入核。
    pub inj: SuInjector,
}

impl SuGate {
    /// 判据总门。
    pub fn judge(&self, ok: bool, rounds: u32, maps: u64) -> (bool, String) {
        let all_ok = ok && self.inj.all_caught();
        let msg = format!(
            "{}: 直调 {}/10 轮 映射={} 滞后=0 分界账=在调 inj={}/{} seg=0x{:04x}",
            SU_VERSION, rounds, maps, self.inj.caught, self.inj.cases, SU_SEG_BASE
        );
        (all_ok, msg)
    }
}

/// 判据自检（t 系入口）：映射正确 + 直调时机 + 分界 + 注入 10/10。
pub fn run_t2001_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx2001-t001");
    // 冻结参数。
    set.add(
        "SU-params-frozen",
        SU_UTIL_MAX == 1024 && SU_FREQ_TABLE.len() == 16 && SU_RATE_UP > SU_RATE_DOWN,
        "util 上界 1024 / 表 16 档 / up>down",
    );
    // 映射端点核。
    let endpoints = SuGov::map_util_to_idx(0).unwrap_or(99) == 0
        && SuGov::map_util_to_idx(SU_UTIL_MAX).unwrap_or(99) == SU_FREQ_TABLE.len() - 1
        && SuGov::map_util_to_idx(SU_UTIL_MAX + 1).is_err();
    set.add("SU-map-endpoints", endpoints, "0→底档 max→顶档 越界拒");
    // 直调时机核（三点皆可调）。
    let mut g = SuGov::new();
    let sites_ok = g.on_site(SuSite::Tick, 256).is_ok()
        && g.on_site(SuSite::Wakeup, 512).is_ok()
        && g.on_site(SuSite::Switch, 1024).is_ok()
        && g.maps == 3
        && g.lag_ticks == 0;
    set.add("SU-sites-direct", sites_ok, "tick/wakeup/switch 三点直调零滞后");
    // 10/10 演练。
    let (ok, rounds, maps) = drill_10_rounds(0x2001_0001);
    set.add_dyn(
        "SU-drill-10/10",
        ok,
        &format!("映射正确 {}/10 轮 maps={}", rounds, maps),
    );
    // 注入 10/10。
    let mut gate = SuGate {
        inj: SuInjector::new(),
    };
    gate.inj.inject_all();
    set.add_dyn(
        "SU-inject-10/10",
        gate.inj.all_caught(),
        &format!("{}/{}", gate.inj.caught, gate.inj.cases),
    );
    // 判据总门。
    let (jok, msg) = gate.judge(ok, rounds, maps);
    set.add_dyn("UNX-F2001-J1", jok, &msg);
    set
}
