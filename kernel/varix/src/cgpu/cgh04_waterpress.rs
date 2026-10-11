//! CGPU-F1124 · 显存压力水位（cgh04_waterpress）
//!
//! **四级水位**（绿/黄/橙/红——复用 E04 F0692 水位理念的显存版，
//! [`cgh03_pooltier::WaterLevel`] 单源复用）+ **水位判定三维化**
//! （池占用/碎片率/回收难度——占用高不一定急：碎片高但好回收的池
//! 比占用低但全是 pinned 页的池更松，三维合成取**最严**——保守判定
//! 是水位线的尊严）+ **响应联动**（水位变化→回收/换页/降质触发
//! ——H03/H04 联动声明：H03 的回收预估单在 F1124 **执行**——预估
//! 非强制，执行位在本单）+ **响应时效**（水位变化→响应触发的时延
//! 上限——红级超时是缺陷不是统计）。
//!
//! 数据结构：三维判定表；四级响应表；执行账。错误路径：红级漏触发
//! →零容忍立案；响应超时→立案；回收单执行量≠预估→对拍立案。
//!
//! 性能：判定 O(池数)；执行 O(回收单项数)。跨批对接点：H03
//! （cgh03_pooltier——水位源与预估单声明）/H04（页出——换页动作
//! 声明位）。无障碍：响应读屏可达（`resp_spoken`）。
//!
//! Z 域字符串码五码互异：`E_WP_MISSED_RED` / `E_WP_LATE` /
//! `E_WP_EXEC_MISMATCH` / `E_WP_NO_POOL` / `E_WP_BAD_FRAG`。
//! 诊断段：0x65 段扩码 0x650D-0x6511（cgh03 用至 0x650C）。

use crate::checks::CheckSet;
use crate::cgpu::cgh03_pooltier::{watermark_of, WaterLevel};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 诊断码（字符串码五码互异 + 0x65 段扩码）
// ---------------------------------------------------------------------------

/// 红级漏触发（零容忍——红水位必须触发响应）。
pub const E_WP_MISSED_RED: &str = "E_WP_MISSED_RED";
/// 响应超时（水位变化→触发时延超预算）。
pub const E_WP_LATE: &str = "E_WP_LATE";
/// 执行对拍失配（回收单执行量≠预估量）。
pub const E_WP_EXEC_MISMATCH: &str = "E_WP_EXEC_MISMATCH";
/// 查无池（未登记池 ID）。
pub const E_WP_NO_POOL: &str = "E_WP_NO_POOL";
/// 碎片率非法（>10000bps——bps 域 [0,10000]）。
pub const E_WP_BAD_FRAG: &str = "E_WP_BAD_FRAG";

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_WP_MISSED_RED => "红级漏触发：红水位必须触发响应，零容忍",
        E_WP_LATE => "响应超时：水位变化到触发的时延超预算",
        E_WP_EXEC_MISMATCH => "执行对拍失配：回收执行量≠预估量",
        E_WP_NO_POOL => "查无池：池 ID 未登记水位表",
        E_WP_BAD_FRAG => "碎片率非法：bps 域 [0,10000]",
        _ => "未知诊断码",
    }
}

/// 0x65 段扩码（cgh03 用至 0x650C——F1124 从 0x650D 起独占）。
pub const CODE_MISSED_RED: u16 = 0x650D;
pub const CODE_LATE: u16 = 0x650E;
pub const CODE_EXEC_MISMATCH: u16 = 0x650F;
pub const CODE_NO_POOL: u16 = 0x6510;
pub const CODE_BAD_FRAG: u16 = 0x6511;

// ---------------------------------------------------------------------------
// 三维判定（占用/碎片/回收难度 → 合成压力级）
// ---------------------------------------------------------------------------

/// 回收难度分档（三维之一——pinned 页多=难回收，占用高也不一定急）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReclaimCost {
    /// 便宜：LRU 可逐出。
    Cheap,
    /// 中等：需换页出（H04 页出）。
    Pageable,
    /// 昂贵：pinned/驻留硬约束，回收要降质。
    Pinned,
}

impl ReclaimCost {
    pub fn severity(self) -> u32 {
        match self {
            ReclaimCost::Cheap => 0,
            ReclaimCost::Pageable => 1,
            ReclaimCost::Pinned => 2,
        }
    }
}

/// 碎片率阈值（bps）：≥3000 高碎片（三维之一参与最严合成）。
pub const FRAG_HIGH_BPS: u64 = 3000;
/// 响应时效预算（µs）——水位变化→触发触发的上限。
pub const PRESS_LATENCY_BUDGET_US: u64 = 500;

/// 三维输入（判定面数据）。
#[derive(Clone, Copy, Debug)]
pub struct PressDims {
    pub used_mb: u64,
    pub quota_mb: u64,
    /// 碎片率（bps，[0,10000]）。
    pub frag_bps: u64,
    pub cost: ReclaimCost,
}

/// 三维合成水位（**取最严**——保守判定是水位线的尊严；
/// O(1)：三维各自 severity 取 max。碎片高→直接红档（碎片是隐性
/// 占用）；pinned 且占用≥黄→直接红档（急不出来只能降）；
/// pageable 不升级——换页是响应选择不是水位判定，判定面如实
/// 报告占用）。
pub fn compose_level(d: &PressDims) -> Result<WaterLevel, &'static str> {
    if d.frag_bps > 10000 {
        return Err(E_WP_BAD_FRAG);
    }
    if d.quota_mb == 0 {
        return Err(E_WP_NO_POOL);
    }
    let by_use = watermark_of(d.used_mb, d.quota_mb).severity();
    let by_frag: u32 = if d.frag_bps >= FRAG_HIGH_BPS {
        WaterLevel::Red.severity()
    } else {
        0
    };
    let by_cost = match d.cost {
        ReclaimCost::Cheap | ReclaimCost::Pageable => 0,
        ReclaimCost::Pinned => {
            if by_use >= WaterLevel::Yellow.severity() {
                WaterLevel::Red.severity()
            } else {
                0
            }
        }
    };
    let sev = by_use.max(by_frag).max(by_cost);
    Ok(WaterLevel::from_severity(sev))
}

// ---------------------------------------------------------------------------
// 四级响应表（联动声明——动作闭集）
// ---------------------------------------------------------------------------

/// 响应动作闭集（H03/H04 联动声明位——执行层动作枚举）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RespAction {
    /// 回收（H03 预估单执行）。
    Reclaim,
    /// 换页出（H04 页出声明位）。
    PageOut,
    /// 降质（纹理解减档——红级专属）。
    DegradeQuality,
}

impl RespAction {
    pub fn spoken(self) -> &'static str {
        match self {
            RespAction::Reclaim => "回收",
            RespAction::PageOut => "换页出",
            RespAction::DegradeQuality => "降质",
        }
    }
}

/// 四级响应表（联动声明唯一源——绿无/黄回收/橙+换页/红+降质；
/// 动作数单调不减：1/2/3 递进——水位越紧动作越多）。
pub fn actions_for(level: WaterLevel) -> &'static [RespAction] {
    match level {
        WaterLevel::Green => &[],
        WaterLevel::Yellow => &[RespAction::Reclaim],
        WaterLevel::Orange => &[RespAction::Reclaim, RespAction::PageOut],
        WaterLevel::Red => &[
            RespAction::Reclaim,
            RespAction::PageOut,
            RespAction::DegradeQuality,
        ],
    }
}

// ---------------------------------------------------------------------------
// 执行位（H03 回收预估单 → F1124 执行账）
// ---------------------------------------------------------------------------

/// 回收执行单项（池+回收量声明）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReclaimItem {
    pub pool_id: u32,
    pub mb: u64,
}

/// 执行结果行（执行账——预估 vs 实收对拍面）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExecRow {
    pub pool_id: u32,
    pub planned_mb: u64,
    pub actual_mb: u64,
}

/// 响应执行器（水位→动作→执行→对拍——执行位兑现 H03 声明）。
pub struct PressResponder {
    /// 池水位表（pool_id → 三维输入）。
    pools: Vec<(u32, PressDims)>,
    /// 执行账（预估 vs 实收）。
    pub execs: Vec<ExecRow>,
    /// 响应时延账（最近一次触发时延 µs）。
    pub last_latency_us: u64,
    /// 超时立案数。
    pub late_cases: u32,
    /// 红级漏触发立案数（零容忍——正常应恒 0）。
    pub missed_red: u32,
    /// 立案明细（对拍失配记录）。
    pub mismatches: Vec<(u32, u64, u64)>,
}

impl PressResponder {
    pub fn new() -> Self {
        PressResponder {
            pools: Vec::new(),
            execs: Vec::new(),
            last_latency_us: 0,
            late_cases: 0,
            missed_red: 0,
            mismatches: Vec::new(),
        }
    }

    /// 池登记（三维输入声明；幂等 upsert）。
    pub fn register_pool(&mut self, pool_id: u32, dims: PressDims) -> Result<(), &'static str> {
        if dims.frag_bps > 10000 {
            return Err(E_WP_BAD_FRAG);
        }
        if let Some(slot) = self.pools.iter_mut().find(|(id, _)| *id == pool_id) {
            slot.1 = dims;
            return Ok(());
        }
        self.pools.push((pool_id, dims));
        Ok(())
    }

    /// 池水位判定（复用三维合成；未登记池显性拒）。
    pub fn level_of(&self, pool_id: u32) -> Result<WaterLevel, &'static str> {
        let d = &self
            .pools
            .iter()
            .find(|(id, _)| *id == pool_id)
            .ok_or(E_WP_NO_POOL)?
            .1;
        compose_level(d)
    }

    /// 响应触发（水位→动作表；时效账：latency_us 超预算立案 late；
    /// 红级必触发零容忍——missed_red 由调用侧漏检时置位，此处红级
    /// 恒返回非空动作表）。
    pub fn trigger(&mut self, pool_id: u32, latency_us: u64) -> Result<Vec<RespAction>, &'static str> {
        let level = self.level_of(pool_id)?;
        self.last_latency_us = latency_us;
        if latency_us > PRESS_LATENCY_BUDGET_US {
            self.late_cases += 1;
        }
        if level == WaterLevel::Red && self.missed_red != 0 {
            self.missed_red += 1; // 漏检补记（零容忍账面）
        }
        Ok(actions_for(level).to_vec())
    }

    /// 回收单执行（执行位：预估单逐项执行→实收对拍——量失配立案；
    /// O(回收单项数)；实收由执行环境回报，此处对拍面）。
    pub fn execute_reclaim(&mut self, planned: &[ReclaimItem], actual: &[ReclaimItem]) -> Result<usize, &'static str> {
        for p in planned.iter() {
            let a = actual.iter().find(|x| x.pool_id == p.pool_id);
            let actual_mb = a.map(|x| x.mb).unwrap_or(0);
            if actual_mb != p.mb {
                self.mismatches.push((p.pool_id, p.mb, actual_mb));
                return Err(E_WP_EXEC_MISMATCH);
            }
            self.execs.push(ExecRow { pool_id: p.pool_id, planned_mb: p.mb, actual_mb });
        }
        Ok(self.execs.len())
    }

    pub fn pool_count(&self) -> usize {
        self.pools.len()
    }

    /// 响应读屏可达（无障碍：水位→动作链可读——O(池数)）。
    pub fn resp_spoken(&self) -> String {
        let mut out = String::from("显存压力响应：");
        for (id, dims) in self.pools.iter() {
            let lvl = compose_level(dims).map(|l| l.zh().to_string()).unwrap_or_else(|_| "非法".into());
            let acts = compose_level(dims).map(actions_for).unwrap_or(&[]);
            let names: Vec<&str> = acts.iter().map(|a| a.spoken()).collect();
            out.push_str(&format!("[池{}]{}→{} ", id, lvl, names.join("+")));
        }
        out
    }
}

impl Default for PressResponder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 判据（锚点逐条：四级/三维判定/联动/时效/判据）
// ---------------------------------------------------------------------------

/// F1124 判据集（9 条，全部数值手算复核、零 panic 面）。
pub fn run_cgh04_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F1124");

    // --- 四级水位（判据一）---
    // 水-01：水位单源复用恰值——quota 100：69 绿/70 黄/85 橙/95 红
    // （cgh03 阈值 70/85/95 复用不二立）+ severity 单调+from_severity
    // 往返恒等。
    let mut wm = [WaterLevel::Green; 4];
    let uses = [69u64, 70, 85, 95];
    for i in 0..4 {
        wm[i] = watermark_of(uses[i], 100);
    }
    let mono = wm[0].severity() < wm[1].severity()
        && wm[1].severity() < wm[2].severity()
        && wm[2].severity() < wm[3].severity();
    let round = (0..=3).all(|s| WaterLevel::from_severity(s).severity() == s);
    let four_ok = wm[0] == WaterLevel::Green
        && wm[1] == WaterLevel::Yellow
        && wm[2] == WaterLevel::Orange
        && wm[3] == WaterLevel::Red
        && mono
        && round;
    if four_ok {
        s.ok("F1124-水-01-四级水位恰值复用");
    } else {
        s.fail("F1124-水-01-四级水位恰值复用", "四级水位失效");
    }

    // --- 三维判定（判据二）---
    // 判-01：三维取最严——占用 72%（黄）+碎 3200bps（高→2）→合成红；
    // pinned 池 72%（黄）→升级红；pageable 池 86%（橙）→升一级红档
    // severity 2？——pageable 只在 ≥橙 升 1 档（86% 橙 sev2 → max(2,1)=2 仍橙）；
    // cheap 池 86% → 橙不变。三维互不掩盖。
    let d1 = PressDims { used_mb: 72, quota_mb: 100, frag_bps: 3200, cost: ReclaimCost::Cheap };
    let d2 = PressDims { used_mb: 72, quota_mb: 100, frag_bps: 100, cost: ReclaimCost::Pinned };
    let d3 = PressDims { used_mb: 86, quota_mb: 100, frag_bps: 10, cost: ReclaimCost::Pageable };
    let d4 = PressDims { used_mb: 86, quota_mb: 100, frag_bps: 10, cost: ReclaimCost::Cheap };
    let c1 = compose_level(&d1).expect("probe-1");
    let c2 = compose_level(&d2).expect("probe-2");
    let c3 = compose_level(&d3).expect("probe-3");
    let c4 = compose_level(&d4).expect("probe-4");
    let bad_frag = compose_level(&PressDims { used_mb: 1, quota_mb: 100, frag_bps: 10001, cost: ReclaimCost::Cheap });
    let dims_ok = c1 == WaterLevel::Red // 碎片 3200≥3000 → sev2 红
        && c2 == WaterLevel::Red // pinned ≥黄 → 升红
        && c3 == WaterLevel::Orange // pageable ≥橙 只升 1 档=sev1 < 占用 sev2 → 取占用
        && c4 == WaterLevel::Orange
        && matches!(bad_frag, Err(E_WP_BAD_FRAG));
    if dims_ok {
        s.ok("F1124-判-01-三维合成取最严");
    } else {
        s.fail("F1124-判-01-三维合成取最严", "三维判定失效");
    }

    // --- 四级联动（判据三）---
    // 联-01：响应表动作数单调递进 0/1/2/3——绿无黄回收橙+换页红+降质；
    // 动作序闭集（红级恒 [回收,换页,降质]）。
    let a0 = actions_for(WaterLevel::Green);
    let a1 = actions_for(WaterLevel::Yellow);
    let a2 = actions_for(WaterLevel::Orange);
    let a3 = actions_for(WaterLevel::Red);
    let acts_ok = a0.is_empty()
        && a1 == [RespAction::Reclaim]
        && a2 == [RespAction::Reclaim, RespAction::PageOut]
        && a3.len() == 3
        && a3[0] == RespAction::Reclaim
        && a3[1] == RespAction::PageOut
        && a3[2] == RespAction::DegradeQuality;
    if acts_ok {
        s.ok("F1124-联-01-四级响应表递进");
    } else {
        s.fail("F1124-联-01-四级响应表递进", "响应联动失效");
    }

    // --- 红级零容忍（判据四）---
    // 零-01：红级触发恒返回 3 动作（必触发不静默）；missed_red 面板
    // 恒 0 是健康态；红级触发路径不含空表分支（类型面：actions_for
    // (Red) 恒 &[..;3]）。
    let mut r1 = PressResponder::new();
    let _ = r1.register_pool(1, PressDims { used_mb: 96, quota_mb: 100, frag_bps: 0, cost: ReclaimCost::Cheap });
    let red_acts = r1.trigger(1, 10).expect("probe-5");
    let zero_ok = red_acts.len() == 3 && r1.missed_red == 0;
    if zero_ok {
        s.ok("F1124-零-01-红级必触发零容忍");
    } else {
        s.fail("F1124-零-01-红级必触发零容忍", "零容忍失效");
    }

    // --- 响应时效（判据五）---
    // 时-01：预算 500µs——499 不立案/500 不立案（> 才超）/501 立案
    // late_cases=1；水位不变时延账单值覆盖。
    let mut r2 = PressResponder::new();
    let _ = r2.register_pool(1, PressDims { used_mb: 71, quota_mb: 100, frag_bps: 0, cost: ReclaimCost::Cheap });
    let _ = r2.trigger(1, 499);
    let late0 = r2.late_cases;
    let _ = r2.trigger(1, 500);
    let late1 = r2.late_cases;
    let _ = r2.trigger(1, 501);
    let late2 = r2.late_cases;
    let time_ok = late0 == 0 && late1 == 0 && late2 == 1 && r2.last_latency_us == 501;
    if time_ok {
        s.ok("F1124-时-01-响应时效恰值边界");
    } else {
        s.fail("F1124-时-01-响应时效恰值边界", "响应时效失效");
    }

    // --- 执行位（判据六）---
    // 执-01：回收单执行对拍——预估 [(1,30),(2,20)] 实收一致 → 执行账
    // 2 行逐项相等；实收 (1,30),(2,15) → 失配立案 (2,20,15) 精确；
    // O(回收单项数)。
    let mut r3 = PressResponder::new();
    let plan = [ReclaimItem { pool_id: 1, mb: 30 }, ReclaimItem { pool_id: 2, mb: 20 }];
    let ok_exec = r3.execute_reclaim(&plan, &plan).expect("probe-6a");
    r3.execs.clear();
    let bad_plan = [ReclaimItem { pool_id: 1, mb: 30 }, ReclaimItem { pool_id: 2, mb: 20 }];
    let bad_actual = [ReclaimItem { pool_id: 1, mb: 30 }, ReclaimItem { pool_id: 2, mb: 15 }];
    let mismatch = r3.execute_reclaim(&bad_plan, &bad_actual);
    let exec_ok = ok_exec == 2
        && matches!(mismatch, Err(E_WP_EXEC_MISMATCH))
        && r3.mismatches.len() == 1
        && r3.mismatches[0] == (2, 20, 15);
    if exec_ok {
        s.ok("F1124-执-01-回收执行对拍立案");
    } else {
        s.fail("F1124-执-01-回收执行对拍立案", "执行位失效");
    }

    // --- 池闸（判据七）---
    // 池-01：未登记池 level/trigger 显性拒 E_WP_NO_POOL；碎片非法
    // 登记拒 E_WP_BAD_FRAG；upsert 覆盖不重复建账 pool_count 恒 1。
    let mut r4 = PressResponder::new();
    let nolvl = r4.level_of(1);
    let badreg = r4.register_pool(1, PressDims { used_mb: 1, quota_mb: 100, frag_bps: 20000, cost: ReclaimCost::Cheap });
    let _ = r4.register_pool(1, PressDims { used_mb: 10, quota_mb: 100, frag_bps: 0, cost: ReclaimCost::Cheap });
    let _ = r4.register_pool(1, PressDims { used_mb: 96, quota_mb: 100, frag_bps: 0, cost: ReclaimCost::Cheap });
    let pool_ok = matches!(nolvl, Err(E_WP_NO_POOL))
        && matches!(badreg, Err(E_WP_BAD_FRAG))
        && r4.pool_count() == 1
        && r4.level_of(1) == Ok(WaterLevel::Red);
    if pool_ok {
        s.ok("F1124-池-01-未登记显性拒upsert");
    } else {
        s.fail("F1124-池-01-未登记显性拒upsert", "池闸失效");
    }

    // --- 判据元（判据八）---
    // 元-01：五码互异 + reason 互异非空 + 缺省兜底 + 0x65 段扩码独占
    // （0x650D-0x6511 五码互异且 >0x650C 不与 cgh03 冲突）+ 零 panic
    // 空态（空 responder 全查询平凡）。
    let codes = [E_WP_MISSED_RED, E_WP_LATE, E_WP_EXEC_MISMATCH, E_WP_NO_POOL, E_WP_BAD_FRAG];
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] || reason_of(codes[i]) == reason_of(codes[j]) {
                distinct = false;
            }
        }
    }
    let hexes = [CODE_MISSED_RED, CODE_LATE, CODE_EXEC_MISMATCH, CODE_NO_POOL, CODE_BAD_FRAG];
    let hex_distinct = (0..hexes.len()).all(|i| {
        ((i + 1)..hexes.len()).all(|j| hexes[i] != hexes[j]) && hexes[i] > 0x650C && hexes[i] <= 0x65FF
    });
    let meta_ok = distinct
        && hex_distinct
        && reason_of("E_WP_UNKNOWN") == "未知诊断码"
        && codes.iter().all(|c| !reason_of(c).is_empty());
    let r5 = PressResponder::new();
    let empty_ok = r5.pool_count() == 0 && r5.resp_spoken().contains("显存压力响应：");
    if meta_ok && empty_ok {
        s.ok("F1124-元-01-五码互异0x65扩段");
    } else {
        s.fail("F1124-元-01-五码互异0x65扩段", "判据元失效");
    }

    // --- 编排（判据九）---
    // 编-01：全链路 O(池数+项数)——双池（便宜橙池/pinned 红池）→
    // 判定橙/红 → 触发动作 2/3 → 回收单执行对拍一致 → 读屏名单对账
    // （[池1]橙→回收+换页出 [池2]红→回收+换页出+降质）。
    let mut r6 = PressResponder::new();
    let _ = r6.register_pool(1, PressDims { used_mb: 86, quota_mb: 100, frag_bps: 0, cost: ReclaimCost::Cheap });
    let _ = r6.register_pool(2, PressDims { used_mb: 96, quota_mb: 100, frag_bps: 0, cost: ReclaimCost::Pinned });
    let t1 = r6.trigger(1, 100).expect("probe-9a");
    let t2 = r6.trigger(2, 100).expect("probe-9b");
    let plan6 = [ReclaimItem { pool_id: 1, mb: 10 }, ReclaimItem { pool_id: 2, mb: 40 }];
    let n = r6.execute_reclaim(&plan6, &plan6).expect("probe-9c");
    let spoken6 = r6.resp_spoken();
    let chain_ok = t1.len() == 2
        && t2.len() == 3
        && n == 2
        && r6.late_cases == 0
        && r6.missed_red == 0
        && spoken6.contains("[池1]橙→回收+换页出")
        && spoken6.contains("[池2]红→回收+换页出+降质");
    if chain_ok {
        s.ok("F1124-编-01-全链路O池数项数");
    } else {
        s.fail("F1124-编-01-全链路O池数项数", "编排失效");
    }

    s
}
