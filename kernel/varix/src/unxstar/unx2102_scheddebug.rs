//! unx2102_scheddebug —— UNX-F2102 sched_debug 调试面（全量 dump 只读）。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B26）：UNX-F2102 · sched_debug 调试面（全量 dump 只读） · 目标 320 行 · 已深化
//! 判据原文：UNX-F2102-J1 调试面断言全过（runqueue/调度实体全量 dump 只读
//!   语义），dump 与账本一致断言，权限位在册
//! 组件：Se 调度实体（pid/vruntime/权重/队列归属封闭账）+ Runqueue（实体账
//!      + 最小 vruntime + 负载权重和）+ DebugPerm 权限位掩码（DUMP=1/READ=2/
//!      INJECT=4 在册闭集，未知位 0xCD04 拒）+ DumpWriter 只读快照面（全量
//!      dump 生成，**类型级无写接口**——只读语义由「dump 前后调度器指纹不变
//!      断言」+「无 &mut 入口」双保险钉死）+ dump↔账本一致断言（快照逐字段
//!      与账本重算对拍：实体数/逐 pid/vruntime/权重/队列归属/负载和全位对
//!      拍）+ 权限闸（无 DUMP|READ 位拒绝 0xCD03）+ 只读破线探针（注入式
//!      变更后指纹必变——负向证据）+ LCG 压力（多队列多实体全量 dump 轮转
//!      + 一致断言逐轮全过）+ 指纹守恒对拍锚。
//! 对标 Linux：/proc/sched_debug 全量 dump（runqueue per-cfs-vec + se 实体
//!   快照）的只读语义 + root 权限闸——本模块以「封闭权限位 + 只读快照 +
//!   dump↔账本全位对拍 + 指纹不变断言」复刻该面，无真实 procfs 依赖。
//! 性能逐项分解：dump O(实体数)（逐行格式化）；一致断言 O(实体数)（重算对
//!   拍）；指纹 O(实体数)；权限闸 O(1)；全整数算术零浮点零墙钟，同事件序列
//!   恒同账（确定性对拍锚，LCG 注入）。
//! 诊断码段：0xCD（0xCD01~0xCD06，全域 grep 确认无诊断基数占用——命中均为
//!   GB2312 汉字编码偶然值，先入库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性（逻辑
//!   tick 注入，零墙钟），逐条定制数据结构/状态机/错误路径，零模板复述，
//!   零死代码。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与判码
// ---------------------------------------------------------------------------

/// 实体数上限（快照容量封顶，防 dump 失控）。
pub const SE_CAP: usize = 256;
/// 队列数上限。
pub const RQ_CAP: usize = 16;
/// 权限位：DUMP——允许生成全量 dump。
pub const PERM_DUMP: u32 = 1;
/// 权限位：READ——允许读取快照内容。
pub const PERM_READ: u32 = 2;
/// 权限位：INJECT——允许注入调试事件（本面不使用，仅权限在册——判据③）。
pub const PERM_INJECT: u32 = 4;
/// 权限位全集（在册闭集）。
pub const PERM_ALL: u32 = PERM_DUMP | PERM_READ | PERM_INJECT;
/// dump 最低权限：DUMP|READ 同时持有。
pub const PERM_DUMP_NEED: u32 = PERM_DUMP | PERM_READ;

// 判码（段内独占 0xCD01~0xCD06）
/// 0xCD01 只读破线（dump 前后调度器指纹变化）。
pub const C_DUMP_MUTATED: u16 = 0xCD01;
/// 0xCD02 dump 与账本不一致（逐字段对拍失恒）。
pub const C_LEDGER_MISMATCH: u16 = 0xCD02;
/// 0xCD03 权限拒绝（缺 DUMP|READ 位）。
pub const C_PERM_DENIED: u16 = 0xCD03;
/// 0xCD04 未知权限位（越 PERM_ALL 在册闭集）。
pub const C_PERM_UNKNOWN: u16 = 0xCD04;
/// 0xCD05 实体越队（dump 行的队列归属与账不符）。
pub const C_SE_OOB: u16 = 0xCD05;
/// 0xCD06 快照不完整（dump 行数与实体账不符）。
pub const C_SNAP_PARTIAL: u16 = 0xCD06;

/// 段守卫：判码落 0xCD00..0xCE00 开区间且严格递增互异。
pub const fn seg_guard() -> bool {
    C_DUMP_MUTATED >= 0xCD00
        && C_DUMP_MUTATED < 0xCE00
        && C_LEDGER_MISMATCH == C_DUMP_MUTATED + 1
        && C_PERM_DENIED == C_DUMP_MUTATED + 2
        && C_PERM_UNKNOWN == C_DUMP_MUTATED + 3
        && C_SE_OOB == C_DUMP_MUTATED + 4
        && C_SNAP_PARTIAL == C_DUMP_MUTATED + 5
        && C_SNAP_PARTIAL < 0xCE00
}

/// 判码 → 文本（闭集穷举）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_DUMP_MUTATED => "只读破线",
        C_LEDGER_MISMATCH => "dump与账本不一致",
        C_PERM_DENIED => "权限拒绝",
        C_PERM_UNKNOWN => "未知权限位",
        C_SE_OOB => "实体越队",
        C_SNAP_PARTIAL => "快照不完整",
        _ => "未知判码",
    }
}

/// 权限位在册核：掩码必须落 PERM_ALL 子集——判据③「权限位在册」。
pub const fn perm_known(mask: u32) -> bool {
    mask & !PERM_ALL == 0
}

// ---------------------------------------------------------------------------
// 调度实体与队列
// ---------------------------------------------------------------------------

/// 调度实体（封闭账字段）。
#[derive(Clone, Copy, Debug)]
pub struct Se {
    /// pid（>0，账内唯一）。
    pub pid: u64,
    /// vruntime（纳秒口径整数）。
    pub vruntime: u64,
    /// 权重（1..=1024）。
    pub weight: u64,
    /// 队列归属（0..RQ_CAP）。
    pub rq: usize,
}

/// 调度器账：全实体封闭账 + 多队列。
pub struct SchedLedger {
    /// 实体账（账内 pid 唯一）。
    pub ses: Vec<Se>,
    /// 逻辑 tick（确定性推进锚）。
    pub tick: u64,
}

impl SchedLedger {
    /// 空账。
    pub fn new() -> Self {
        SchedLedger { ses: Vec::new(), tick: 0 }
    }
    /// 入队（pid 重复 / 越队 / 权重越界 拒绝）。
    pub fn admit(&mut self, se: Se) -> Result<(), u16> {
        if self.ses.len() >= SE_CAP {
            return Err(C_SNAP_PARTIAL);
        }
        if se.pid == 0 || self.ses.iter().any(|s| s.pid == se.pid) {
            return Err(C_LEDGER_MISMATCH);
        }
        if se.rq >= RQ_CAP || se.weight == 0 || se.weight > 1024 {
            return Err(C_SE_OOB);
        }
        self.ses.push(se);
        Ok(())
    }
    /// 队列负载：rq 队列的权重和。
    pub fn rq_load(&self, rq: usize) -> u64 {
        self.ses.iter().filter(|s| s.rq == rq).map(|s| s.weight).sum()
    }
    /// 队列实体数。
    pub fn rq_count(&self, rq: usize) -> u64 {
        self.ses.iter().filter(|s| s.rq == rq).count() as u64
    }
    /// 最小 vruntime（rq 队列；空队列返回 None）。
    pub fn rq_min_vruntime(&self, rq: usize) -> Option<u64> {
        self.ses.iter().filter(|s| s.rq == rq).map(|s| s.vruntime).min()
    }
    /// 调度器指纹：全实体四元组混合 FNV（只读断言基准）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xCD02_0000_0002_102A;
        h ^= self.tick;
        h = h.wrapping_mul(0x100_0000_01B3);
        for s in &self.ses {
            for v in [s.pid, s.vruntime, s.weight, s.rq as u64] {
                h ^= v;
                h = h.wrapping_mul(0x100_0000_01B3);
            }
        }
        h
    }
    /// tick 推进（vruntime 增长模拟——与 dump 无关，供压力面）。
    pub fn advance(&mut self, n: u64) {
        self.tick += n;
        for s in self.ses.iter_mut() {
            s.vruntime = s.vruntime.wrapping_add(n * 1024 / s.weight.max(1));
        }
    }
}

// ---------------------------------------------------------------------------
// 只读 dump 面
// ---------------------------------------------------------------------------

/// dump 快照行（封闭字段，与账本对拍口径一一对应）。
#[derive(Clone, Debug, PartialEq)]
pub struct DumpLine {
    /// pid。
    pub pid: u64,
    /// vruntime。
    pub vruntime: u64,
    /// 权重。
    pub weight: u64,
    /// 队列归属。
    pub rq: usize,
    /// 所在队列负载（dump 时刻）。
    pub rq_load: u64,
}

/// 全量 dump：权限闸 → 快照生成（只读——&self 入口，无写路径）。
/// 返回 Ok(行集)；权限缺失 Err(C_PERM_DENIED)。
pub fn dump_all(led: &SchedLedger, perm: u32) -> Result<Vec<DumpLine>, u16> {
    if !perm_known(perm) {
        return Err(C_PERM_UNKNOWN);
    }
    if perm & PERM_DUMP_NEED != PERM_DUMP_NEED {
        return Err(C_PERM_DENIED);
    }
    let mut lines = Vec::new();
    for s in &led.ses {
        lines.push(DumpLine {
            pid: s.pid,
            vruntime: s.vruntime,
            weight: s.weight,
            rq: s.rq,
            rq_load: led.rq_load(s.rq),
        });
    }
    Ok(lines)
}

/// dump ↔ 账本一致断言：快照逐字段与账本重算对拍——判据②。
/// 核面：行数 == 实体数；逐行四字段 + 队列负载与账重算一致。
pub fn dump_ledger_consistent(led: &SchedLedger, lines: &[DumpLine]) -> bool {
    if lines.len() != led.ses.len() {
        return false;
    }
    for (i, l) in lines.iter().enumerate() {
        let s = &led.ses[i];
        if l.pid != s.pid
            || l.vruntime != s.vruntime
            || l.weight != s.weight
            || l.rq != s.rq
            || l.rq_load != led.rq_load(s.rq)
        {
            return false;
        }
    }
    true
}

/// 只读语义断言：dump 前后调度器指纹必须不变——判据①核心。
/// （dump_all 以 &self 为唯一入口，此处以指纹不变做运行期证据。）
pub fn readonly_assert(led: &SchedLedger, perm: u32) -> Result<bool, u16> {
    let before = led.fingerprint();
    let lines = dump_all(led, perm)?;
    let after = led.fingerprint();
    if before != after {
        return Err(C_DUMP_MUTATED);
    }
    Ok(dump_ledger_consistent(led, &lines))
}

/// 权限矩阵核：四组权限组合的预期结果闭集（判据③在册面）。
/// [全权→Ok, 仅DUMP→Denied, 仅READ→Denied, 零权→Denied, 未知位→Unknown]
pub fn perm_matrix(led: &SchedLedger) -> [bool; 5] {
    let m1 = dump_all(led, PERM_ALL).is_ok();
    let m2 = dump_all(led, PERM_DUMP) == Err(C_PERM_DENIED);
    let m3 = dump_all(led, PERM_READ) == Err(C_PERM_DENIED);
    let m4 = dump_all(led, 0) == Err(C_PERM_DENIED);
    let m5 = dump_all(led, PERM_ALL | 0x8000) == Err(C_PERM_UNKNOWN);
    [m1, m2, m3, m4, m5]
}

/// 只读破线探针（负向证据）：dump 后变更账 → 指纹必变，一致断言必破。
/// 返回 (指纹变检测 ok, 一致断言破检测 ok)。
pub fn mutation_probe(led: &mut SchedLedger) -> (bool, bool) {
    let lines = match dump_all(led, PERM_ALL) {
        Ok(l) => l,
        Err(_) => return (false, false),
    };
    let fp_before = led.fingerprint();
    led.advance(7); // 变更调度器状态
    let fp_after = led.fingerprint();
    let mutated = fp_before != fp_after; // 指纹变 = 只读断言能抓到变更
    let inconsistent = !dump_ledger_consistent(led, &lines); // 旧快照与新城一致破
    (mutated, inconsistent)
}

// ---------------------------------------------------------------------------
// 确定性压力（LCG 注入）
// ---------------------------------------------------------------------------

/// LCG：64 位（Numerical Recipes 口径），确定性零墙钟。
pub struct Lcg(u64);

impl Lcg {
    /// 种子注入。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    /// 下一伪随机值。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// 压力结果。
pub struct DebugStress {
    /// dump 轮数。
    pub rounds: u64,
    /// 一致断言通过轮数。
    pub consistent: u64,
    /// 只读断言通过轮数。
    pub readonly_ok: u64,
    /// 实体数峰值。
    pub peak_ses: u64,
    /// 总结论。
    pub ok: bool,
}

/// 调试面压力：n 轮 LCG（入队/推进/dump/断言轮转），全量只读核。
pub fn stress_debug(n: usize, seed: u64) -> DebugStress {
    let mut rng = Lcg::new(seed);
    let mut led = SchedLedger::new();
    let mut rounds: u64 = 0;
    let mut consistent: u64 = 0;
    let mut readonly_ok: u64 = 0;
    let mut peak: u64 = 0;
    for i in 0..n {
        let r = rng.next();
        match i % 4 {
            0 => {
                // 入队：pid 单调，队列/权重随机。
                let se = Se {
                    pid: (i as u64) + 1,
                    vruntime: (r >> 8) % 10_000,
                    weight: 1 + (r >> 20) % 1024,
                    rq: ((r >> 32) % RQ_CAP as u64) as usize,
                };
                let _ = led.admit(se);
                peak = peak.max(led.ses.len() as u64);
            }
            1 => led.advance(1 + r % 8),
            2 | 3 => {
                rounds += 1;
                if readonly_assert(&led, PERM_ALL) == Ok(true) {
                    consistent += 1;
                    readonly_ok += 1;
                }
            }
            _ => {}
        }
    }
    let ok = consistent == rounds && readonly_ok == rounds;
    DebugStress { rounds, consistent, readonly_ok, peak_ses: peak, ok }
}

/// 指纹：压力四柱混合 FNV。
pub fn fingerprint(s: &DebugStress) -> u64 {
    let mut h: u64 = 0xCD06_0000_0002_102B;
    for v in [s.rounds, s.consistent, s.readonly_ok, s.peak_ses] {
        h ^= v;
        h = h.wrapping_mul(0x100_0000_01B3);
    }
    h
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 自检聚合（防自调：不由本模块任何函数调用）。
pub fn run_unx2102_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F2102");

    // J-1 段守卫 + 权限位在册。
    cs.add("0xCD段独占", seg_guard() && perm_known(PERM_ALL) && !perm_known(0x8000), "");

    // J-2 权限矩阵：五组组合闭集（判据③）。
    let mut led = SchedLedger::new();
    for i in 0..8u64 {
        let _ = led.admit(Se { pid: i + 1, vruntime: i * 100, weight: 100 + i, rq: (i % 4) as usize });
    }
    let pm = perm_matrix(&led);
    cs.add("权限矩阵闭集", pm.iter().all(|&b| b), "");

    // J-3 只读语义断言：dump 前后指纹不变 + 一致断言全过（判据①②）。
    match readonly_assert(&led, PERM_ALL) {
        Ok(true) => cs.add("只读语义+账本一致", true, ""),
        Ok(false) => cs.add("只读语义+账本一致", false, "一致断言未过"),
        Err(c) => cs.add("只读语义+账本一致", false, code_text(c)),
    }

    // J-4 只读破线探针：注入变更后指纹必变 + 旧快照一致必破（负向证据）。
    let mut led2 = SchedLedger::new();
    for i in 0..6u64 {
        let _ = led2.admit(Se { pid: i + 1, vruntime: i * 50, weight: 64 + i, rq: (i % 2) as usize });
    }
    let (mutated, inconsistent) = mutation_probe(&mut led2);
    cs.add("只读破线探针", mutated && inconsistent, "");

    // J-5 dump ↔ 账本逐字段：篡改一行 pid 必被对拍抓出。
    let lines = match dump_all(&led, PERM_ALL) {
        Ok(l) => l,
        Err(_) => {
            cs.add("篡改对拍", false, "");
            return cs;
        }
    };
    let mut bad = lines.clone();
    bad[0].pid = 0x9999;
    cs.add("篡改对拍", !dump_ledger_consistent(&led, &bad), "");

    // J-6 入队闸：重复 pid / 越队 / 零权重 / 超重 全拒。
    let dup = led.admit(Se { pid: 1, vruntime: 0, weight: 1, rq: 0 }) == Err(C_LEDGER_MISMATCH);
    let oob = led.admit(Se { pid: 100, vruntime: 0, weight: 1, rq: RQ_CAP }) == Err(C_SE_OOB);
    let zw = led.admit(Se { pid: 101, vruntime: 0, weight: 0, rq: 0 }) == Err(C_SE_OOB);
    let heavy = led.admit(Se { pid: 102, vruntime: 0, weight: 1025, rq: 0 }) == Err(C_SE_OOB);
    cs.add("入队闸闭集", dup && oob && zw && heavy, "");

    // J-7 队列账：负载和/实体数/最小 vruntime 重算面。
    let load_ok = {
        let mut total = 0u64;
        let mut ok = true;
        for rq in 0..RQ_CAP {
            let load = led.rq_load(rq);
            let cnt = led.rq_count(rq);
            total += load;
            // 实体数 > 0 时最小 vruntime 必有值。
            if cnt > 0 && led.rq_min_vruntime(rq).is_none() {
                ok = false;
            }
            if cnt == 0 && load != 0 {
                ok = false;
            }
        }
        ok && total == led.ses.iter().map(|s| s.weight).sum::<u64>()
    };
    cs.add("队列账重算", load_ok, "");

    // J-8 压力：512 轮全维核。
    let st = stress_debug(512, 0xCD02_0001);
    cs.add("压力全维", st.ok && st.rounds > 0 && st.peak_ses > 0, "");

    // J-9 指纹重放对拍：同种子恒同指纹。
    let f1 = fingerprint(&stress_debug(128, 0xCD02_0002));
    let f2 = fingerprint(&stress_debug(128, 0xCD02_0002));
    let f3 = fingerprint(&stress_debug(128, 0xCD02_0003));
    cs.add("指纹重放对拍", f1 == f2 && f1 != f3, "");

    // J-10 判码文本闭集：六码各得其所。
    let texts_ok = code_text(C_DUMP_MUTATED) != "未知判码"
        && code_text(C_LEDGER_MISMATCH) != "未知判码"
        && code_text(C_PERM_DENIED) != "未知判码"
        && code_text(C_PERM_UNKNOWN) != "未知判码"
        && code_text(C_SE_OOB) != "未知判码"
        && code_text(C_SNAP_PARTIAL) != "未知判码"
        && code_text(0xCD00) == "未知判码";
    cs.add("判码文本闭集", texts_ok, "");

    cs
}

// ---------------------------------------------------------------------------
// 单元测试（std 环境对拍）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_led() -> SchedLedger {
        let mut led = SchedLedger::new();
        for i in 0..8u64 {
            let _ = led.admit(Se { pid: i + 1, vruntime: i * 100, weight: 100 + i, rq: (i % 4) as usize });
        }
        led
    }

    #[test]
    fn perm_matrix_exact() {
        let led = mk_led();
        let pm = perm_matrix(&led);
        assert!(pm.iter().all(|&b| b));
    }

    #[test]
    fn readonly_and_consistent() {
        let led = mk_led();
        assert_eq!(readonly_assert(&led, PERM_ALL), Ok(true));
        assert!(matches!(readonly_assert(&led, PERM_DUMP), Err(C_PERM_DENIED)));
        assert!(matches!(readonly_assert(&led, 0x8000), Err(C_PERM_UNKNOWN)));
    }

    #[test]
    fn mutation_caught() {
        let mut led = mk_led();
        let (mutated, inconsistent) = mutation_probe(&mut led);
        assert!(mutated && inconsistent);
    }

    #[test]
    fn stress_replay_stable() {
        let a = stress_debug(128, 42);
        let b = stress_debug(128, 42);
        assert_eq!(a.rounds, b.rounds);
        assert_eq!(a.peak_ses, b.peak_ses);
        assert!(a.ok);
    }
}
