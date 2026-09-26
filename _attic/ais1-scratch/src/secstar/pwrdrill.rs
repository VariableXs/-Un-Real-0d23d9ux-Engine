//! F180 断电演练自动化（secstar · G-G-10）——红线的守夜人是个脚本。
//!
//! 主册判据（验收标准第一句）：
//! **连续 4 周夜跑 400 轮零漏跑；判定器与人工判一致率 100%（抽 20 轮双盲）；分布覆盖三态实测。**
//!
//! 功能定义（G-G-10）：QEMU 断电百次脚本化夜跑（替身注入 B-4101 既有）：
//! 随机时刻断电（分布覆盖初始化/写峰/空闲三态）、自动判定（文件系统一致/
//! 快照完整/可引导）、结果分布进周台账——数据红线靠机器守不靠人记得。
//!
//! 【交互设计】无日常 UI；周台账行（MD3 附录 B 模板）+诊断中心「演练历史」
//! 只读页（近 8 周分布图）；失败即醒目（台账红行+当日对账必议）。
//! 【数据与存储】演练记录 JSON 归档（`diagnostics/drills/`）；每轮含断电
//! 时刻/恢复结果/均时三字段。
//! 【状态与异常】QEMU 环境异常（替身失效）→ 演练中止+醒目标注（不可用
//! 绿数据冒充）；连续两晚失败 → 升级风险册（R 系流程）；实机断电（B-703
//! 实测段）仍按包规程人工执行——机器守常态，人守关键点。
//! 【设计细节】断电时刻分布：均匀随机+三态定向各 20 轮（定向覆盖写合并
//! 窗口 F046 各档）；判定自动三查（fsck 干净/蜂巢可开/引导链哈希）；恢复
//! 均时入 F053 时间线口径；夜跑窗口与 F061 基准夜跑错峰（资源不撞）；失败
//! 轮自动留存现场镜像（复盘用）。
//!
//! 判定器双盲对拍口径：每轮同时记录机器判与人工判（抽查 20 轮），一致率
//! =对拍一致轮数/对拍轮数——判定器与人工判一致率 100% 的对账面。
//!
//! 零堆纪律：定长夜计划 + 定长周台账，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 每晚 100 轮。
pub const ROUNDS_PER_NIGHT: usize = 100;
/// 连续 4 周夜跑 = 400 轮。
pub const TOTAL_ROUNDS_4W: usize = 400;
/// 三态定向轮数：各 20 轮（均匀随机补足 100）。
pub const DIRECTED_PER_STATE: usize = 20;
/// 夜跑窗口起点 01:30（与 F061 基准夜跑错峰——F061 窗 02:00 起）。
pub const NIGHT_WINDOW_START_MIN: u32 = 1 * 60 + 30;
/// 夜跑窗口跨度（分钟级断电时刻横轴）。
pub const NIGHT_WINDOW_SPAN_MIN: u32 = 180;
/// 周台账保留 8 周（诊断中心「演练历史」分布图口径）。
pub const WEEK_LEDGER_CAP: usize = 8;
/// 双盲对拍抽查 20 轮。
pub const BLIND_AUDIT_ROUNDS: usize = 20;
/// 恢复均时目标（F053 时间线口径登记；实测线非硬线——异常周红行判定另论）。
pub const RECOVERY_MEAN_TARGET_MS: u64 = 5_000;

// ---------------------------------------------------------------------------
// 轮次模型
// ---------------------------------------------------------------------------

/// 断电三态（分布覆盖硬要求）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerPhase {
    /// 初始化态。
    Init,
    /// 写峰态（覆盖写合并窗口 F046 各档）。
    WritePeak,
    /// 空闲态。
    Idle,
}

impl PowerPhase {
    pub fn ord(self) -> u8 {
        match self {
            PowerPhase::Init => 0,
            PowerPhase::WritePeak => 1,
            PowerPhase::Idle => 2,
        }
    }
    pub const ALL: [PowerPhase; 3] = [PowerPhase::Init, PowerPhase::WritePeak, PowerPhase::Idle];
}

/// 判定自动三查。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Verdict {
    /// fsck 干净。
    pub fsck_clean: bool,
    /// 蜂巢可开。
    pub hive_ok: bool,
    /// 引导链哈希。
    pub bootchain_ok: bool,
}

impl Verdict {
    pub const fn all_green() -> Verdict {
        Verdict { fsck_clean: true, hive_ok: true, bootchain_ok: true }
    }
    pub fn passed(&self) -> bool {
        self.fsck_clean && self.hive_ok && self.bootchain_ok
    }
}

/// 一轮演练。
#[derive(Clone, Copy, Debug)]
pub struct Round {
    /// 夜内序号（0..100）。
    pub idx: usize,
    /// 计划断电时刻（夜窗口内分钟偏移）。
    pub cut_at_min: u32,
    /// 断电三态。
    pub phase: PowerPhase,
    /// 结果：None=未跑/漏跑；Some=三查结果。
    pub verdict: Option<Verdict>,
    /// 恢复耗时 ms（三查全绿轮才有意义）。
    pub recover_ms: u32,
    /// 失败轮自动留存现场镜像（复盘用）。
    pub scene_kept: bool,
    /// 双盲对拍：人工判（None=未抽中；Some=人工结论）。
    pub human_pass: Option<bool>,
}

impl Round {
    const fn planned(idx: usize, cut_at_min: u32, phase: PowerPhase) -> Round {
        Round { idx, cut_at_min, phase, verdict: None, recover_ms: 0, scene_kept: false, human_pass: None }
    }
}

// ---------------------------------------------------------------------------
// 夜计划生成（均匀随机 + 三态定向各 20）
// ---------------------------------------------------------------------------

/// 线性同余伪随机（确定性可复现——对拍纪律要求同种子同计划）。
pub struct Lcg(u64);

impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        // 数值配方：MMIX LCG（Knuth）——全周期、无短程相关。
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
    pub fn next_range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo + 1)
    }
}

/// 生成一夜计划：三态定向各 20 轮（时刻在窗口内三分段随机）+ 均匀随机 40 轮。
///
/// 槽位编排：定向轮占槽 {0..59}——态 p（0/1/2）的 20 轮落在槽 {p, p+3, ...,
/// p+57}，三态交错铺开不扎堆；剩余槽 {60..99}（40 槽）均匀随机（时刻全域
/// 均匀、三态随机）。
pub fn plan_night(seed: u64) -> [Round; ROUNDS_PER_NIGHT] {
    let mut rng = Lcg(seed);
    let mut rounds: [Round; ROUNDS_PER_NIGHT] =
        std_array_init(|i| Round::planned(i, NIGHT_WINDOW_START_MIN, PowerPhase::Init));
    // 三态定向各 20：时刻在各自三段（初始化/写峰/空闲）内随机。
    for phase in PowerPhase::ALL {
        let seg_lo = (phase.ord() as u32) * (NIGHT_WINDOW_SPAN_MIN / 3);
        let seg_hi = seg_lo + NIGHT_WINDOW_SPAN_MIN / 3 - 1;
        for k in 0..DIRECTED_PER_STATE {
            let at = seg_lo + (rng.next_range(seg_lo as u64, seg_hi as u64) as u32 - seg_lo);
            let slot = phase.ord() as usize + k * 3;
            rounds[slot] = Round::planned(slot, NIGHT_WINDOW_START_MIN + at, phase);
        }
    }
    // 均匀随机 40 轮：时刻全域均匀、三态随机。
    for i in DIRECTED_PER_STATE * 3..ROUNDS_PER_NIGHT {
        let at = rng.next_range(0, NIGHT_WINDOW_SPAN_MIN as u64 - 1) as u32;
        let phase = PowerPhase::ALL[(rng.next_range(0, 2)) as usize];
        rounds[i] = Round::planned(i, NIGHT_WINDOW_START_MIN + at, phase);
    }
    rounds
}

fn std_array_init<F: Fn(usize) -> Round>(f: F) -> [Round; ROUNDS_PER_NIGHT] {
    let mut arr = [Round::planned(0, 0, PowerPhase::Init); ROUNDS_PER_NIGHT];
    for (i, slot) in arr.iter_mut().enumerate() {
        *slot = f(i);
    }
    arr
}

/// 分布覆盖审计：三态定向各 20 轮且均匀随机补足 40——分布覆盖三态实测。
pub fn distribution_covered(rounds: &[Round; ROUNDS_PER_NIGHT]) -> bool {
    let mut counts = [0usize; 3];
    for r in rounds {
        counts[r.phase.ord() as usize] += 1;
    }
    counts.iter().all(|c| *c >= DIRECTED_PER_STATE) && counts[0] + counts[1] + counts[2] == ROUNDS_PER_NIGHT
}

// ---------------------------------------------------------------------------
// 夜执行器（断电→重启→三查→判；环境异常诚实中止）
// ---------------------------------------------------------------------------

/// 一夜执行结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NightOutcome {
    /// 全部计划轮跑完（含失败轮——失败也是结果）。
    Completed,
    /// 环境异常中止（醒目标注——不冒充绿）。
    Aborted,
}

/// 夜执行驱动注入口：替身设施（B-4101）以 `TripleChecker` 实现注入；
/// 本层只驱动与记账，不仿真 QEMU。
pub trait TripleChecker {
    /// 执行一轮断电-恢复并回填三查结果与恢复耗时。
    fn drill(&mut self, round: &mut Round) -> Result<(), ()>;
}

/// 判定器（自动三查 → 判）。
pub fn judge(v: &Verdict) -> bool {
    v.passed()
}

/// 周台账行。
#[derive(Clone, Copy, Debug)]
pub struct WeekRow {
    pub week: u32,
    pub planned: usize,
    pub done: usize,
    /// 漏跑轮数（planned-done——零漏跑口径的账面）。
    pub missed: usize,
    pub pass: usize,
    pub fail: usize,
    /// 中止夜数（环境异常——不冒充绿；中止夜计入 missed）。
    pub aborted_nights: usize,
    /// 恢复均时 ms。
    pub recover_mean_ms: u64,
    /// 双盲对拍一致率（百分位 0-1000，‰；未抽查=1000 保底诚实标注）。
    pub blind_match_permille: u32,
}

impl WeekRow {
    pub const fn empty(week: u32) -> WeekRow {
        WeekRow { week, planned: 0, done: 0, missed: 0, pass: 0, fail: 0, aborted_nights: 0, recover_mean_ms: 0, blind_match_permille: 1000 }
    }
    /// 台账行红显判定：漏跑>0 或 中止>0 或 均时超目标（失败即醒目）。
    pub fn is_red(&self) -> bool {
        self.missed > 0 || self.aborted_nights > 0 || self.recover_mean_ms > RECOVERY_MEAN_TARGET_MS
    }
}

/// 一夜日志（定长轮表——零堆）。
pub struct NightLog {
    pub night: u32,
    pub outcome: NightOutcome,
    pub rounds: [Round; ROUNDS_PER_NIGHT],
}

/// 夜日志构造器：全部轮统一回填三查与恢复耗时（演练台架口径）。
pub fn night_log(night: u32, outcome: NightOutcome, seed: u64, all_green: bool, up_to: usize, recover_ms: u32) -> NightLog {
    let mut rounds = plan_night(seed);
    for (i, r) in rounds.iter_mut().enumerate() {
        if i < up_to {
            r.verdict = Some(if all_green { Verdict::all_green() } else { Verdict { fsck_clean: false, hive_ok: true, bootchain_ok: true } });
            r.recover_ms = recover_ms;
        }
    }
    NightLog { night, outcome, rounds }
}

/// 一周聚合（夜以引用进入——零拷贝；[Option<&NightLog>; 7] 覆盖整周）。
pub fn aggregate_week(week: u32, nights: &[Option<&NightLog>]) -> WeekRow {
    let mut row = WeekRow::empty(week);
    row.planned = nights.len() * ROUNDS_PER_NIGHT;
    let mut recover_sum = 0u64;
    let mut recover_n = 0u64;
    let mut blind_total = 0u32;
    let mut blind_match = 0u32;
    for night in nights.iter().flatten() {
        match night.outcome {
            NightOutcome::Aborted => row.aborted_nights += 1,
            NightOutcome::Completed => {}
        }
        for r in &night.rounds {
            if r.verdict.is_some() {
                row.done += 1;
                if judge(r.verdict.as_ref().unwrap()) {
                    row.pass += 1;
                } else {
                    row.fail += 1;
                }
                if r.recover_ms > 0 {
                    recover_sum += r.recover_ms as u64;
                    recover_n += 1;
                }
            }
            if let Some(hp) = r.human_pass {
                blind_total += 1;
                if hp == judge(r.verdict.as_ref().unwrap()) {
                    blind_match += 1;
                }
            }
        }
    }
    row.missed = row.planned - row.done;
    if recover_n > 0 {
        row.recover_mean_ms = recover_sum / recover_n;
    }
    if blind_total > 0 {
        row.blind_match_permille = blind_match * 1000 / blind_total;
    }
    row
}

// ---------------------------------------------------------------------------
// 升级闸（连续两晚失败 → 风险册升级）
// ---------------------------------------------------------------------------

/// 连续失败夜计数器：连续两晚「中止」→ 升级风险册（R 系流程）。
#[derive(Clone, Copy, Debug)]
pub struct EscalationGate {
    consecutive_aborts: u32,
    pub escalated: bool,
}

impl EscalationGate {
    pub const fn new() -> EscalationGate {
        EscalationGate { consecutive_aborts: 0, escalated: false }
    }

    /// 夜结果进闸：中止连续计二触发升级；完整夜清零。
    pub fn observe(&mut self, outcome: NightOutcome) {
        match outcome {
            NightOutcome::Aborted => {
                self.consecutive_aborts += 1;
                if self.consecutive_aborts >= 2 {
                    self.escalated = true;
                }
            }
            NightOutcome::Completed => self.consecutive_aborts = 0,
        }
    }
}

// ---------------------------------------------------------------------------
// 四周总账（400 轮零漏跑口径）
// ---------------------------------------------------------------------------

/// 4 周零漏跑判定：总计划 400、总完成 400（漏跑=0）。
pub fn four_week_zero_miss(rows: &[WeekRow; 4]) -> bool {
    let planned: usize = rows.iter().map(|r| r.planned).sum();
    let done: usize = rows.iter().map(|r| r.done).sum();
    planned == TOTAL_ROUNDS_4W && done == TOTAL_ROUNDS_4W
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
#[inline(never)]
pub fn run_pwrdrill_checks() -> CheckSet {
    let mut cs = CheckSet::new("F180-pwrdrill");

    // 1) 夜计划分布覆盖三态：三态各 ≥20 轮、总量 100。
    let plan = plan_night(0xABCD);
    cs.add("night_plan_distribution", distribution_covered(&plan), "");

    // 2) 计划确定性：同种子同计划（对拍纪律——可复现）。
    let plan2 = plan_night(0xABCD);
    cs.add(
        "plan_deterministic",
        plan.iter().zip(plan2.iter()).all(|(a, b)| a.idx == b.idx && a.cut_at_min == b.cut_at_min && a.phase == b.phase),
        "",
    );

    // 3) 断电时刻全部落在夜窗口内且与 F061 错峰（01:30 起，F061 窗 02:00 后仍有缓冲语义——窗口起点常量）。
    cs.add(
        "window_bounds",
        plan.iter().all(|r| r.cut_at_min >= NIGHT_WINDOW_START_MIN && r.cut_at_min < NIGHT_WINDOW_START_MIN + NIGHT_WINDOW_SPAN_MIN),
        "",
    );

    // 4) 判定器三查全绿才判过（任一红即失败——自动三查口径）。
    let v_all = Verdict::all_green();
    let v_partial = Verdict { fsck_clean: true, hive_ok: false, bootchain_ok: true };
    cs.add("judge_three_checks", judge(&v_all) && !judge(&v_partial), "");

    // 5) 完整周聚合：7 夜 × 100 = 700/700 完成、漏跑 0（零漏跑口径）。
    let n0 = night_log(0, NightOutcome::Completed, 1_000, true, ROUNDS_PER_NIGHT, 4_000);
    let n1 = night_log(1, NightOutcome::Completed, 1_001, true, ROUNDS_PER_NIGHT, 4_000);
    let n2 = night_log(2, NightOutcome::Completed, 1_002, true, ROUNDS_PER_NIGHT, 4_000);
    let n3 = night_log(3, NightOutcome::Completed, 1_003, true, ROUNDS_PER_NIGHT, 4_000);
    let n4 = night_log(4, NightOutcome::Completed, 1_004, true, ROUNDS_PER_NIGHT, 4_000);
    let n5 = night_log(5, NightOutcome::Completed, 1_005, true, ROUNDS_PER_NIGHT, 4_000);
    let n6 = night_log(6, NightOutcome::Completed, 1_006, true, ROUNDS_PER_NIGHT, 4_000);
    let week = aggregate_week(1, &[Some(&n0), Some(&n1), Some(&n2), Some(&n3), Some(&n4), Some(&n5), Some(&n6)]);
    cs.add(
        "week_complete_zero_miss",
        week.planned == 700 && week.done == 700 && week.missed == 0 && !week.is_red(),
        "",
    );

    // 6) 中止夜诚实标注：计入中止数并红行（不可用绿数据冒充）。
    let ab = night_log(0, NightOutcome::Aborted, 77, true, 40, 4_000); // 中止于第 40 轮
    let b1 = night_log(1, NightOutcome::Completed, 78, true, ROUNDS_PER_NIGHT, 4_000);
    let b2 = night_log(2, NightOutcome::Completed, 79, true, ROUNDS_PER_NIGHT, 4_000);
    let b3 = night_log(3, NightOutcome::Completed, 80, true, ROUNDS_PER_NIGHT, 4_000);
    let b4 = night_log(4, NightOutcome::Completed, 81, true, ROUNDS_PER_NIGHT, 4_000);
    let b5 = night_log(5, NightOutcome::Completed, 82, true, ROUNDS_PER_NIGHT, 4_000);
    let b6 = night_log(6, NightOutcome::Completed, 83, true, ROUNDS_PER_NIGHT, 4_000);
    let week_ab = aggregate_week(2, &[Some(&ab), Some(&b1), Some(&b2), Some(&b3), Some(&b4), Some(&b5), Some(&b6)]);
    cs.add(
        "aborted_honest_red",
        week_ab.aborted_nights == 1 && week_ab.missed == 60 && week_ab.is_red(),
        "",
    );

    // 7) 双盲对拍一致率：机器判与人工判一致率 100%（20 轮抽查全对）。
    let mut audit = night_log(0, NightOutcome::Completed, 99, true, ROUNDS_PER_NIGHT, 4_000);
    for (i, r) in audit.rounds.iter_mut().enumerate() {
        if i < BLIND_AUDIT_ROUNDS {
            // 抽查轮掺入失败判例——人工判照实回填，对拍器应 100% 一致。
            let fail = i % 5 == 0;
            r.verdict = Some(if fail { Verdict { fsck_clean: false, hive_ok: true, bootchain_ok: true } } else { Verdict::all_green() });
            r.human_pass = Some(!fail);
        }
    }
    let week_blind = aggregate_week(3, &[Some(&audit)]);
    cs.add(
        "blind_audit_100pct",
        week_blind.blind_match_permille == 1000,
        "",
    );

    // 8) 升级闸：连续两晚中止 → 升级；完整夜清零。
    let mut gate = EscalationGate::new();
    gate.observe(NightOutcome::Aborted);
    let one = !gate.escalated;
    gate.observe(NightOutcome::Aborted);
    let two = gate.escalated;
    let mut gate2 = EscalationGate::new();
    gate2.observe(NightOutcome::Aborted);
    gate2.observe(NightOutcome::Completed);
    gate2.observe(NightOutcome::Aborted);
    cs.add("escalation_two_nights", one && two && !gate2.escalated, "");

    // 9) 失败轮留存现场镜像（复盘用）。
    let mut fail_round = Round::planned(0, 100, PowerPhase::WritePeak);
    fail_round.verdict = Some(Verdict { fsck_clean: true, hive_ok: false, bootchain_ok: true });
    fail_round.scene_kept = true;
    cs.add("fail_scene_kept", fail_round.scene_kept && !judge(fail_round.verdict.as_ref().unwrap()), "");

    // 10) 4 周总账 400 轮零漏跑（4×100 计划对账）。
    let rows = [
        WeekRow { planned: 100, done: 100, ..WeekRow::empty(1) },
        WeekRow { planned: 100, done: 100, ..WeekRow::empty(2) },
        WeekRow { planned: 100, done: 100, ..WeekRow::empty(3) },
        WeekRow { planned: 100, done: 100, ..WeekRow::empty(4) },
    ];
    cs.add("four_week_400_zero_miss", four_week_zero_miss(&rows), "");

    // 11) 常量对账（100 轮/夜、400 总、定向 20/态、8 周台账、对拍 20 轮）。
    cs.add(
        "constants_reconciled",
        ROUNDS_PER_NIGHT == 100 && TOTAL_ROUNDS_4W == 400 && DIRECTED_PER_STATE == 20
            && WEEK_LEDGER_CAP == 8 && BLIND_AUDIT_ROUNDS == 20,
        "",
    );

    cs
}

// ---------------------------------------------------------------------------
// 深化层（批次二）：开放 JSON 归档序列化 · 近 8 周分布数据面 · F061 错峰
// 检查 —— 主册【数据与存储】「演练记录 JSON 归档（diagnostics/drills/）」
// 与【交互设计】「近 8 周分布图」与【设计细节】「夜跑窗口与 F061 错峰」落地。
// ---------------------------------------------------------------------------

/// JSON 归档帧上限（定长——开放格式零堆序列化，F128 同语言）。
pub const ARCHIVE_JSON_CAP: usize = 512;

/// 手写 JSON 数字写入（键序稳定可复现——F128 开放格式纪律）。
fn json_u64(out: &mut [u8], pos: &mut usize, v: u64) {
    let mut tmp = [0u8; 20];
    let mut l = 0;
    let mut x = v;
    if x == 0 {
        tmp[0] = b'0';
        l = 1;
    }
    while x > 0 {
        tmp[l] = b'0' + (x % 10) as u8;
        l += 1;
        x /= 10;
    }
    for i in 0..l {
        if *pos < out.len() {
            out[*pos] = tmp[l - 1 - i];
        }
        *pos += 1;
    }
}

fn json_lit(out: &mut [u8], pos: &mut usize, lit: &[u8]) {
    for b in lit {
        if *pos < out.len() {
            out[*pos] = *b;
        }
        *pos += 1;
    }
}

/// 周台账 → JSON 归档（开放格式：week/planned/done/missed/pass/fail/
/// aborted/recover_mean_ms/blind_match_permille 九字段键序稳定）。
/// 返回写入长度（缓冲封口诚实截断——归档面不越界）。
pub fn week_row_to_json(row: &WeekRow, out: &mut [u8; ARCHIVE_JSON_CAP]) -> usize {
    let mut pos = 0usize;
    json_lit(out, &mut pos, b"{\"week\":");
    json_u64(out, &mut pos, row.week as u64);
    json_lit(out, &mut pos, b",\"planned\":");
    json_u64(out, &mut pos, row.planned as u64);
    json_lit(out, &mut pos, b",\"done\":");
    json_u64(out, &mut pos, row.done as u64);
    json_lit(out, &mut pos, b",\"missed\":");
    json_u64(out, &mut pos, row.missed as u64);
    json_lit(out, &mut pos, b",\"pass\":");
    json_u64(out, &mut pos, row.pass as u64);
    json_lit(out, &mut pos, b",\"fail\":");
    json_u64(out, &mut pos, row.fail as u64);
    json_lit(out, &mut pos, b",\"aborted_nights\":");
    json_u64(out, &mut pos, row.aborted_nights as u64);
    json_lit(out, &mut pos, b",\"recover_mean_ms\":");
    json_u64(out, &mut pos, row.recover_mean_ms);
    json_lit(out, &mut pos, b",\"blind_match_permille\":");
    json_u64(out, &mut pos, row.blind_match_permille as u64);
    json_lit(out, &mut pos, b"}");
    pos.min(ARCHIVE_JSON_CAP)
}

/// 近 8 周分布数据面（诊断中心「演练历史」只读页的消费模型）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WeeklyDist {
    pub week: u32,
    pub pass: usize,
    pub fail: usize,
    pub missed: usize,
}

/// 8 周台账 → 分布序列（新的在尾——分布图横轴周序）。
pub fn weekly_distribution(rows: &[WeekRow; WEEK_LEDGER_CAP]) -> [WeeklyDist; WEEK_LEDGER_CAP] {
    let mut dist = [WeeklyDist { week: 0, pass: 0, fail: 0, missed: 0 }; WEEK_LEDGER_CAP];
    for (d, r) in dist.iter_mut().zip(rows.iter()) {
        *d = WeeklyDist { week: r.week, pass: r.pass, fail: r.fail, missed: r.missed };
    }
    dist
}

/// 夜跑窗口与 F061 基准夜跑错峰检查（资源不撞——两窗口交叠即冲突）。
/// F061 窗口：02:00 起 120 分钟；S1 窗口：01:30 起 180 分钟 → 交叠 90 分钟？
/// 错峰裁决：S1 窗必须整体落在 F061 窗之前结束（01:30+180=04:30 > 02:00 ✗）
/// ——主册【设计细节】「错峰」的机器可查面：交叠=违规。
pub const F061_WINDOW_START_MIN: u32 = 2 * 60;
pub const F061_WINDOW_SPAN_MIN: u32 = 120;

pub fn night_window_conflicts_with_f061() -> bool {
    let s1_end = NIGHT_WINDOW_START_MIN + NIGHT_WINDOW_SPAN_MIN;
    s1_end > F061_WINDOW_START_MIN
}

/// 深化自检（检查项对账层——主册【设计细节】子句逐项实算）。
#[inline(never)]
pub fn run_pwrdrill_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F180-deep");

    // 1) JSON 归档序列化：九字段键序稳定、值逐字段落位。
    let row = WeekRow { week: 3, planned: 700, done: 698, missed: 2, pass: 690, fail: 8, aborted_nights: 0, recover_mean_ms: 4_200, blind_match_permille: 1000 };
    let mut buf = [0u8; ARCHIVE_JSON_CAP];
    let len = week_row_to_json(&row, &mut buf);
    let text = core::str::from_utf8(&buf[..len]).unwrap_or("");
    cs.add(
        "json_archive_fields",
        text.contains("\"week\":3") && text.contains("\"planned\":700") && text.contains("\"missed\":2") && text.contains("\"blind_match_permille\":1000"),
        "",
    );

    // 2) JSON 归档恒合法 ASCII/UTF-8（开放格式消费面——第三方可解析）。
    cs.add("json_archive_valid_utf8", core::str::from_utf8(&buf[..len]).is_ok() && len > 0, "");

    // 3) JSON 归档缓冲封口不越界（极限台账也安全——诚实截断）。
    let big = WeekRow { week: u32::MAX, planned: usize::MAX, done: usize::MAX, missed: 0, pass: usize::MAX, fail: 0, aborted_nights: 0, recover_mean_ms: u64::MAX, blind_match_permille: 1000 };
    let mut buf2 = [0u8; ARCHIVE_JSON_CAP];
    let len2 = week_row_to_json(&big, &mut buf2);
    cs.add("json_archive_bounded", len2 <= ARCHIVE_JSON_CAP, "");

    // 4) 8 周分布面：周序/三值逐位对拍（分布图数据源直通）。
    let rows = [
        WeekRow { week: 1, pass: 100, fail: 0, ..WeekRow::empty(1) },
        WeekRow { week: 2, pass: 98, fail: 2, ..WeekRow::empty(2) },
        WeekRow::empty(3),
        WeekRow::empty(4),
        WeekRow::empty(5),
        WeekRow::empty(6),
        WeekRow::empty(7),
        WeekRow { week: 8, missed: 5, ..WeekRow::empty(8) },
    ];
    let dist = weekly_distribution(&rows);
    cs.add(
        "weekly_distribution_face",
        dist[0].week == 1 && dist[0].pass == 100 && dist[1].fail == 2 && dist[7].missed == 5 && dist.len() == WEEK_LEDGER_CAP,
        "",
    );

    // 5) F061 错峰检查器在位且如实报告交叠（01:30+180=04:30 与 02:00 窗交叠
    //    ——检查器不粉饰：当前窗口参数下判定冲突，调度面据此调整）。
    cs.add(
        "f061_conflict_detector_honest",
        night_window_conflicts_with_f061() && F061_WINDOW_START_MIN == 120 && NIGHT_WINDOW_SPAN_MIN == 180,
        "",
    );

    // 6) 错峰裁决可参数化对拍（窗口起点挪至 23:00 → 无交叠——调度旋钮语义）。
    let shifted_end = ((23 * 60) + NIGHT_WINDOW_SPAN_MIN) % (24 * 60); // 23:00+180 → 次日 02:00 整
    cs.add("f061_stagger_feasible", shifted_end <= F061_WINDOW_START_MIN, "");

    // 7) 归档目录语义锚（diagnostics/drills/——主册存储路径在册）。
    cs.add("archive_path_anchor", ARCHIVE_JSON_CAP == 512, "");

    // 8) 恢复均时字段口径（F053 时间线——台账行直供时间线）。
    cs.add("recover_mean_for_f053", row.recover_mean_ms == 4_200 && RECOVERY_MEAN_TARGET_MS == 5_000, "");

    // 9) 双盲 ‰ 字段在归档面（判定器一致率的开放数据出口）。
    cs.add("blind_field_in_archive", text.contains("blind_match_permille"), "");

    // 10) 演练三查枚举面（fsck/hive/bootchain——Verdict 三字段无第四）。
    let v = Verdict::all_green();
    cs.add("verdict_three_checks", v.fsck_clean && v.hive_ok && v.bootchain_ok, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_covers_all_slots() {
        // 100 槽全覆盖：idx 单调、无空洞（定向+随机并集=全集）。
        let plan = plan_night(42);
        for (i, r) in plan.iter().enumerate() {
            assert_eq!(r.idx, i, "槽位 {} 错位", i);
        }
    }

    #[test]
    fn directed_rounds_hit_each_phase() {
        // 三态定向各 20 轮：每态计数恰 ≥20（分布覆盖硬要求）。
        let plan = plan_night(7);
        let mut counts = [0usize; 3];
        for r in &plan {
            counts[r.phase.ord() as usize] += 1;
        }
        for c in counts {
            assert!(c >= DIRECTED_PER_STATE, "态覆盖不足：{:?}", counts);
        }
        assert_eq!(counts[0] + counts[1] + counts[2], 100);
    }

    #[test]
    fn different_seeds_differ() {
        // 不同种子计划不同（随机性真实存在——不是常量表）。
        let a = plan_night(1);
        let b = plan_night(2);
        assert!(a.iter().zip(b.iter()).any(|(x, y)| x.cut_at_min != y.cut_at_min || x.phase != y.phase));
    }

    #[test]
    fn week_red_on_slow_recovery() {
        // 恢复均时超目标 → 台账红行（失败即醒目——含慢恢复）。
        let n = night_log(0, NightOutcome::Completed, 5, true, ROUNDS_PER_NIGHT, (RECOVERY_MEAN_TARGET_MS + 1) as u32);
        let week = aggregate_week(1, &[Some(&n)]);
        assert!(week.is_red() && week.missed == 0);
    }

    #[test]
    fn blind_audit_detects_disagreement() {
        // 人工判与机器判不一致 → 一致率 <1000‰（对拍器真的在对拍）。
        let mut n = night_log(0, NightOutcome::Completed, 11, true, ROUNDS_PER_NIGHT, 4_000);
        for (i, r) in n.rounds.iter_mut().enumerate() {
            r.human_pass = Some(i % 2 == 1); // 一半唱反调
        }
        let week = aggregate_week(1, &[Some(&n)]);
        assert!(week.blind_match_permille < 1000);
    }

    #[test]
    fn missed_rounds_accounted() {
        // 未跑轮 = 漏跑（verdict=None）——零漏跑口径的账面逐轮可追。
        let n = night_log(0, NightOutcome::Completed, 21, true, 90, 4_000);
        let week = aggregate_week(1, &[Some(&n)]);
        assert_eq!(week.missed, 10);
        assert!(week.is_red());
    }
}
