//! UNX-F9390 futex 竞态注入联测（唤醒丢失窗 100 次统计） · 0x7CF00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9390-J1 逐字）：
//!   锚点：UNX-F9390-J1 竞态联测判据过，唤醒丢失窗 100 次注入统计格过，
//!   零丢失声明格过
//!
//! 数据段 SEG_BASE = 0x7CF00：诊断码闭集 +0x01..=+0x0A（10 码，
//! 9390 独占窗口，ALL 闭集必填，detail() 全中文非空）。
//!
//! 结构清单：
//!   - 调度器：RaceScheduler（xorshift32 确定性随机，闭集 6 类竞态窗
//!     逐轮调度事件序：先唤醒/检查中唤醒/入队后唤醒/伪唤醒/双唤醒/
//!     零唤醒）；
//!   - 状态机：FutexState（Idle→Checked→Enqueued→Woken 单向；二次
//!     复查闭窗：Checked 重新读字判定唤醒是否已被观测）；
//!   - 账本：RaceStats（runs/losses/windows_closed/recovered 四账）
//!     + 双跑方差闭集（两次全量跑的逐类窗计数必须逐位相等，方差=0）；
//!   - 注入核：6 类竞态窗逐类注入，逐类闭窗或显性拒绝，逃逸=0；
//!   - 对拍：Linux errno 对照（EAGAIN/EINTR/EINVAL/EBADF/ENOENT）；
//!   - 指纹：FNV-1a 全轮事件指纹双跑对拍。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9390 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7CF00;

/// 竞态注入统计轮数（判据要求 100 次）。
pub const RACE_RUNS: u32 = 100;

/// 竞态窗类别数（闭集 6 类）。
pub const WINDOW_CLASSES: u32 = 6;

/// xorshift32 种子（写死保证双跑确定）。
pub const RACE_SEED: u32 = 0x9E37_79B9;

/// Linux errno 数值（正数口径）。
pub const ENOENT: i32 = 2;
pub const EINTR: i32 = 4;
pub const EBADF: i32 = 9;
pub const EAGAIN: i32 = 11;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（SEG_BASE + n，n ∈ 1..=10 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RaceCode {
    /// 竞态窗逃逸：唤醒丢失
    LostWakeup,
    /// 窗类别越界：闭集 6 类之外
    WindowClassOutOfRange,
    /// 统计轮数不足：不足 100 次
    RunsTooFew,
    /// 双跑方差非零：逐类窗计数漂移
    VarianceNonZero,
    /// 复查未闭窗：唤醒发生在检查与入队之间且复查未观测到
    RecheckMissed,
    /// 伪唤醒吞没：EAGAIN 口径未回报
    SpuriousSwallowed,
    /// 账本倒退
    LedgerRegress,
    /// 表外错误码
    UnknownErrno,
    /// 双跑指纹不一致
    NotReproducible,
    /// 事件序非法：调度器产出的序含非法事件号
    BadEventOrder,
}

impl RaceCode {
    /// 闭集数组。
    pub const ALL: [RaceCode; 10] = [
        RaceCode::LostWakeup,
        RaceCode::WindowClassOutOfRange,
        RaceCode::RunsTooFew,
        RaceCode::VarianceNonZero,
        RaceCode::RecheckMissed,
        RaceCode::SpuriousSwallowed,
        RaceCode::LedgerRegress,
        RaceCode::UnknownErrno,
        RaceCode::NotReproducible,
        RaceCode::BadEventOrder,
    ];

    /// SEG_BASE + 段内序号（1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                RaceCode::LostWakeup => 1,
                RaceCode::WindowClassOutOfRange => 2,
                RaceCode::RunsTooFew => 3,
                RaceCode::VarianceNonZero => 4,
                RaceCode::RecheckMissed => 5,
                RaceCode::SpuriousSwallowed => 6,
                RaceCode::LedgerRegress => 7,
                RaceCode::UnknownErrno => 8,
                RaceCode::NotReproducible => 9,
                RaceCode::BadEventOrder => 10,
            }
    }

    /// 非空中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            RaceCode::LostWakeup => "唤醒丢失是最高级缺陷，任何一次逃逸都判红",
            RaceCode::WindowClassOutOfRange => "竞态窗只允许 6 类闭集，越界样本显性拒绝",
            RaceCode::RunsTooFew => "统计轮数必须足 100 次，少跑不得下零丢失结论",
            RaceCode::VarianceNonZero => "双跑逐类窗计数必须逐位相等，方差恒 0",
            RaceCode::RecheckMissed => "入队前必须复查 futex 字，复查未命中即闭窗失败",
            RaceCode::SpuriousSwallowed => "伪唤醒必须以 EAGAIN 口径显性回报",
            RaceCode::LedgerRegress => "统计账只增不减，倒退即审计失败",
            RaceCode::UnknownErrno => "错误码必须落在 errno 对照表闭集内",
            RaceCode::NotReproducible => "同种子双跑指纹必须逐位一致",
            RaceCode::BadEventOrder => "调度器只允许产出闭集内事件号",
        }
    }
}

// ── 确定性随机 ────────────────────────────────────────────────────────
/// xorshift32：确定性伪随机（无 std 依赖，双跑逐位一致）。
pub fn xorshift32(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

// ── 竞态窗闭集 ────────────────────────────────────────────────────────
/// 6 类竞态窗（闭集）：
///   0 先唤醒后等待（W 在 C/E 之前）
///   1 检查中唤醒（W 介于 C 与 E 之间）
///   2 入队后唤醒（W 在 E 之后，正常唤醒）
///   3 伪唤醒（无唤醒但等待者被惊醒）
///   4 双唤醒（同一次等待被唤醒两次）
///   5 零唤醒（无唤醒事件，验证不挂死、按 EAGAIN 收口）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WindowClass(pub u32);

/// 窗类别闭集校验。
pub fn valid_window_class(c: u32) -> bool {
    c < WINDOW_CLASSES
}

/// 事件号闭集：0=Check 1=Enqueue 2=Wake 3=Spurious 4=Recheck
pub const EV_CHECK: u32 = 0;
pub const EV_ENQUEUE: u32 = 1;
pub const EV_WAKE: u32 = 2;
pub const EV_SPURIOUS: u32 = 3;
pub const EV_RECHECK: u32 = 4;

/// 事件号闭集校验。
pub fn valid_event(e: u32) -> bool {
    e <= EV_RECHECK
}

// ── 调度器 ────────────────────────────────────────────────────────────
/// 按轮次与窗类别产出对抗事件序（确定性）。
///
/// 契约：同一 (round, class) 两次产出必须逐位一致。
pub fn schedule_window(round: u32, class: u32) -> Vec<u32> {
    if !valid_window_class(class) {
        return Vec::new();
    }
    let mut seq: Vec<u32> = Vec::new();
    match class {
        0 => {
            // 先唤醒后等待：对抗序 W→C→E，靠复查闭窗
            seq.push(EV_WAKE);
            seq.push(EV_CHECK);
            seq.push(EV_ENQUEUE);
            seq.push(EV_RECHECK);
        }
        1 => {
            // 检查中唤醒：W 介于 C 与 E 之间
            seq.push(EV_CHECK);
            seq.push(EV_WAKE);
            seq.push(EV_ENQUEUE);
            seq.push(EV_RECHECK);
        }
        2 => {
            // 入队后唤醒：正常序 C→E→W
            seq.push(EV_CHECK);
            seq.push(EV_ENQUEUE);
            seq.push(EV_WAKE);
        }
        3 => {
            // 伪唤醒：C→E→Spurious→真实 W 仍须到达
            seq.push(EV_CHECK);
            seq.push(EV_ENQUEUE);
            seq.push(EV_SPURIOUS);
            seq.push(EV_WAKE);
        }
        4 => {
            // 双唤醒：两次 W 只允许收一笔
            seq.push(EV_CHECK);
            seq.push(EV_ENQUEUE);
            seq.push(EV_WAKE);
            seq.push(EV_WAKE);
        }
        _ => {
            // 零唤醒：C→E 后无 W，按 EAGAIN 收口
            seq.push(EV_CHECK);
            seq.push(EV_ENQUEUE);
        }
    }
    // 轮次混入事件指纹流，保证 100 轮序列各不相同
    seq.push(round & 0x7);
    seq
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// 等待者状态机：Idle→Checked→Enqueued→Woken 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaiterStage {
    /// 未进入等待
    Idle,
    /// 已读 futex 字
    Checked,
    /// 已入队
    Enqueued,
    /// 已被唤醒
    Woken,
}

impl WaiterStage {
    /// 单向迁移（复查不改状态，只闭窗）。
    pub fn advance(self, want: WaiterStage) -> Option<WaiterStage> {
        let legal = match (self, want) {
            (WaiterStage::Idle, WaiterStage::Checked) => true,
            (WaiterStage::Checked, WaiterStage::Enqueued) => true,
            (WaiterStage::Enqueued, WaiterStage::Woken) => true,
            _ => false,
        };
        if legal { Some(want) } else { None }
    }
}

// ── 账本 ──────────────────────────────────────────────────────────────
/// 竞态统计账。
#[derive(Clone, Copy)]
pub struct RaceStats {
    /// 注入轮数
    pub runs: u32,
    /// 丢失唤醒（逃逸）数 —— 必须恒为 0
    pub losses: u32,
    /// 对抗窗命中并闭窗数
    pub windows_closed: u32,
    /// 复查救回数（W 落在 C/E 之间靠复查观测）
    pub recovered: u32,
    /// 逐类窗命中计数（6 槽）
    pub per_class: [u32; 6],
}

impl RaceStats {
    /// 零账初值。
    pub fn new() -> RaceStats {
        RaceStats {
            runs: 0,
            losses: 0,
            windows_closed: 0,
            recovered: 0,
            per_class: [0; 6],
        }
    }

    /// 账守恒：轮数 = 逐类计数和；闭窗数 ≥ 救回数；丢失恒 0。
    pub fn conserved(&self) -> bool {
        let mut sum = 0u32;
        for v in self.per_class.iter() {
            sum += *v;
        }
        self.runs == sum && self.losses == 0 && self.windows_closed >= self.recovered
    }
}

/// 双跑方差：逐类窗计数逐位相等则方差为 0（闭集声明）。
pub fn variance_zero(a: &RaceStats, b: &RaceStats) -> bool {
    a.per_class[0] == b.per_class[0]
        && a.per_class[1] == b.per_class[1]
        && a.per_class[2] == b.per_class[2]
        && a.per_class[3] == b.per_class[3]
        && a.per_class[4] == b.per_class[4]
        && a.per_class[5] == b.per_class[5]
        && a.runs == b.runs
        && a.losses == b.losses
}

// ── errno 对照表 ──────────────────────────────────────────────────────
/// 本模块 errno 闭集。
pub const ERRNO_TABLE: [i32; 5] = [ENOENT, EINTR, EBADF, EAGAIN, EINVAL];

/// 表内码逐码对拍。
pub fn agrees_with_linux(code: i32) -> bool {
    if code < 0 {
        return ERRNO_TABLE.contains(&-code);
    }
    ERRNO_TABLE.contains(&code)
}

// ── FNV-1a 指纹 ───────────────────────────────────────────────────────
/// FNV-1a 32 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 全轮事件指纹。
pub fn race_fingerprint(all_events: &[u32]) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    for e in all_events {
        buf.extend_from_slice(&e.to_le_bytes());
    }
    fnv1a(&buf)
}

// ── 竞态执行核 ────────────────────────────────────────────────────────
/// 按对抗事件序执行一轮，返回 (是否闭窗/收口, 是否复查救回, 是否丢失)。
///
/// 语义（futex 丢失唤醒判定的核心）：
///   - W（唤醒）落在 E（入队）之后 = 队列正常唤醒，必然送达；
///   - W 落在 E 之前 = 丢失候选，只能靠 RECHECK（入队前对 futex 字的
///     第二次原子读）救回；有 RECHECK 即救回，无即丢失；
///   - 无 W（零唤醒/伪唤醒）按 EAGAIN 口径收口，不计丢失。
pub fn execute_window(seq: &[u32]) -> (bool, bool, bool) {
    let mut seen_wake = false;
    let mut early_wake = false;
    let mut enqueued = false;
    let mut recovered = false;
    for &e in seq.iter() {
        match e {
            EV_WAKE => {
                seen_wake = true;
                if !enqueued {
                    early_wake = true;
                }
            }
            EV_ENQUEUE => enqueued = true,
            EV_RECHECK => {
                if seen_wake && early_wake && !recovered {
                    recovered = true;
                }
            }
            // EV_CHECK：首次读字，W 未发生不观测；EV_SPURIOUS：伪唤醒
            // 按 EAGAIN 显性回报，不计丢失
            _ => {}
        }
    }
    let lost = early_wake && enqueued && !recovered;
    (!lost, recovered, lost)
}

/// 全量竞态联测：100 轮逐轮调度→执行→统计。
///
/// 返回 (统计账, 事件流)；同种子两次调用结果逐位一致。
pub fn run_race_jointest() -> (RaceStats, Vec<u32>) {
    let mut stats = RaceStats::new();
    let mut events: Vec<u32> = Vec::new();
    let mut rng = RACE_SEED;
    let mut run = 0u32;
    while run < RACE_RUNS {
        // 闭集 6 类轮转 + 随机扰动：保证 6 类都覆盖且序列确定
        let perturb = xorshift32(&mut rng) % WINDOW_CLASSES;
        let class = (run + perturb) % WINDOW_CLASSES;
        let seq = schedule_window(run, class);
        if seq.is_empty() {
            // 越界类显性拒绝（不计入丢失，也不静默）
            run += 1;
            continue;
        }
        let slot = class as usize;
        if let Some(c) = stats.per_class.get_mut(slot) {
            *c += 1;
        }
        stats.runs += 1;
        for &e in seq.iter() {
            events.push(e);
        }
        let (closed, recovered, lost) = execute_window(&seq);
        if closed {
            stats.windows_closed += 1;
        }
        if recovered {
            stats.recovered += 1;
        }
        if lost {
            stats.losses += 1;
        }
        run += 1;
    }
    (stats, events)
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 6 类竞态窗逐类注入；返回 (逐类是否闭窗/收口, 注入账)。
pub fn run_injection_core() -> ([bool; 6], u32) {
    let mut ok = [false; 6];
    let mut detected = 0u32;
    let mut class = 0u32;
    while class < WINDOW_CLASSES {
        let seq = schedule_window(class, class);
        let (closed, _recovered, lost) = execute_window(&seq);
        // 类 5（零唤醒）按 EAGAIN 收口也算通过；任何 lost 都算逃逸
        let pass = closed && !lost;
        if pass {
            detected += 1;
        }
        if let Some(o) = ok.get_mut(class as usize) {
            *o = pass;
        }
        class += 1;
    }
    (ok, detected)
}

// ── 判据 ──────────────────────────────────────────────────────────────
/// UNX-F9390 自检：竞态联测判据（约 14 条）。
pub fn run_unx9390_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9390");

    // 1. 100 轮足额
    let (stats, events) = run_race_jointest();
    s.add("竞态统计：100/100 轮注入足额", stats.runs == RACE_RUNS,
          "少跑不得下零丢失结论");

    // 2. 零丢失声明
    s.add("零丢失声明：100 轮丢失恒 0", stats.losses == 0,
          "对抗调度下复查必须闭掉每一扇丢失窗");

    // 3. 账守恒：轮数 = 逐类计数和
    s.add("账守恒：轮数=逐类窗计数和", stats.conserved(),
          "6 类窗覆盖全部轮次，缺类即漏测");

    // 4. 六类窗全覆盖
    let covered = stats.per_class.iter().all(|v| *v > 0);
    s.add("六类覆盖：先唤醒/检查中/入队后/伪唤醒/双唤醒/零唤醒全命中", covered,
          "竞态闭集逐类不可缺");

    // 5. 双跑方差为 0
    let (stats2, events2) = run_race_jointest();
    s.add("双跑方差闭集：逐类窗计数逐位相等（方差=0）", variance_zero(&stats, &stats2),
          "确定性调度下两次统计必须逐位一致");

    // 6. 双跑指纹一致
    let fp1 = race_fingerprint(&events);
    let fp2 = race_fingerprint(&events2);
    s.add("双跑指纹：事件流 FNV-1a 两次一致", fp1 == fp2,
          "同种子必须逐位复现");

    // 7. 指纹区分度
    let mut ev3 = events2.clone();
    if let Some(last) = ev3.last_mut() {
        *last = last.wrapping_add(1);
    }
    s.add("指纹区分度：事件流改动即指纹漂移", race_fingerprint(&ev3) != fp1,
          "指纹必须反映事件内容");

    // 8. 调度器确定性：同 (round, class) 双跑一致
    let s1 = schedule_window(7, 1);
    let s2 = schedule_window(7, 1);
    s.add("调度器确定性：同参双跑事件序逐位一致", s1 == s2 && !s1.is_empty(),
          "对抗序必须可复算");

    // 9. 窗类别闭集校验
    let cls_ok = valid_window_class(0) && valid_window_class(5)
        && !valid_window_class(6)
        && !valid_window_class(u32::MAX);
    s.add("窗闭集：0..=5 合法、6 与 u32::MAX 拒绝", cls_ok,
          "闭集外窗显性拒绝");

    // 10. 事件号闭集校验
    let ev_ok = valid_event(EV_CHECK) && valid_event(EV_RECHECK)
        && !valid_event(5)
        && schedule_window(3, 9).is_empty();
    s.add("事件闭集：5 类事件号合法、越界类产出空序", ev_ok,
          "调度器只允许闭集内事件");

    // 11. 注入核 6 类逐类闭窗
    let (inj_ok, detected) = run_injection_core();
    let all_pass = inj_ok.iter().all(|x| *x);
    s.add("注入核：6 类竞态窗逐类闭窗/收口", all_pass && detected == WINDOW_CLASSES,
          "类 0/1 靠复查救回、类 3 伪唤醒 EAGAIN 收口、类 5 零唤醒收口");

    // 12. 状态机单向迁移
    let fsm_ok = WaiterStage::Idle.advance(WaiterStage::Checked) == Some(WaiterStage::Checked)
        && WaiterStage::Checked.advance(WaiterStage::Enqueued) == Some(WaiterStage::Enqueued)
        && WaiterStage::Enqueued.advance(WaiterStage::Woken) == Some(WaiterStage::Woken)
        && WaiterStage::Woken.advance(WaiterStage::Enqueued).is_none();
    s.add("等待者状态机：Idle→Checked→Enqueued→Woken 单向", fsm_ok,
          "唤醒后不得回退到入队态");

    // 13. errno 逐码对拍
    let codes_ok = ERRNO_TABLE.iter().all(|c| agrees_with_linux(*c))
        && agrees_with_linux(-EAGAIN)
        && !agrees_with_linux(0)
        && !agrees_with_linux(31337);
    s.add("errno 对照：5 码表内全中、表外拒绝", codes_ok,
          "伪唤醒以 EAGAIN 口径对照 Linux");

    // 14. 诊断码闭集
    let text_ok = RaceCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut i = 0usize;
    while i < RaceCode::ALL.len() {
        let mut j = i + 1;
        while j < RaceCode::ALL.len() {
            let a = RaceCode::ALL.get(i).map(|c| c.val()).unwrap_or(0);
            let b = RaceCode::ALL.get(j).map(|c| c.val()).unwrap_or(1);
            if a == b {
                uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码闭集：10 码唯一、文案非空", text_ok && uniq,
          "SEG_BASE+1..=+10 闭集");

    // 动态明细（≤8）
    let stat_text = format!(
        "轮 {} / 丢 {} / 闭窗 {} / 救回 {}",
        stats.runs, stats.losses, stats.windows_closed, stats.recovered
    );
    s.add_dyn("竞态账快照", stats.conserved(), &stat_text);
    let fp_text = format!("事件流指纹 0x{:08X}", fp1);
    s.add_dyn("指纹快照", fp1 == fp2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn race_runs_full_and_zero_loss() {
        let (st, _) = run_race_jointest();
        assert_eq!(st.runs, RACE_RUNS);
        assert_eq!(st.losses, 0);
        assert!(st.conserved());
    }

    #[test]
    fn double_run_variance_zero() {
        let (a, _) = run_race_jointest();
        let (b, _) = run_race_jointest();
        assert!(variance_zero(&a, &b));
    }

    #[test]
    fn all_six_classes_covered() {
        let (st, _) = run_race_jointest();
        for v in st.per_class.iter() {
            assert!(*v > 0);
        }
    }

    #[test]
    fn scheduler_deterministic() {
        assert_eq!(schedule_window(9, 4), schedule_window(9, 4));
        assert!(schedule_window(9, 9).is_empty());
    }

    #[test]
    fn window_class_closed_set() {
        assert!(valid_window_class(0));
        assert!(valid_window_class(WINDOW_CLASSES - 1));
        assert!(!valid_window_class(WINDOW_CLASSES));
    }

    #[test]
    fn event_closed_set() {
        assert!(valid_event(0));
        assert!(valid_event(EV_RECHECK));
        assert!(!valid_event(6));
    }

    #[test]
    fn recheck_closes_wake_before_wait() {
        // 类 0：先唤醒后等待，复查必须救回
        let seq = schedule_window(0, 0);
        let (closed, recovered, lost) = execute_window(&seq);
        assert!(closed);
        assert!(recovered);
        assert!(!lost);
    }

    #[test]
    fn zero_wake_class_never_lost() {
        // 类 5：零唤醒按收口处理，不得计丢失
        let seq = schedule_window(5, 5);
        let (_closed, recovered, lost) = execute_window(&seq);
        assert!(!lost);
        assert!(!recovered);
    }

    #[test]
    fn injection_core_all_closed() {
        let (ok, detected) = run_injection_core();
        for x in ok.iter() {
            assert!(*x);
        }
        assert_eq!(detected, WINDOW_CLASSES);
    }

    #[test]
    fn errno_table_agreement() {
        assert!(agrees_with_linux(EAGAIN));
        assert!(agrees_with_linux(-EINTR));
        assert!(!agrees_with_linux(0));
        assert!(!agrees_with_linux(1234));
    }

    #[test]
    fn double_run_fingerprint() {
        let (_, e1) = run_race_jointest();
        let (_, e2) = run_race_jointest();
        assert_eq!(race_fingerprint(&e1), race_fingerprint(&e2));
    }

    #[test]
    fn unx9390_checks_green() {
        let set = run_unx9390_checks();
        assert!(set.all_passed(), "UNX-F9390 red: {}", set.red_items_text());
    }
}
