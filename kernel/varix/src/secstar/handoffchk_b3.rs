//! F181 交接预检器 · 批次三深化（secstar · G-G-11）。
//!
//! 批次三功能面（主册判据「三查三态面板 / 全绿通行 <1s / 拦停留痕」纵深）：
//! - [`ProbeScheduler`]：探测调度器——三查逐项超时 + 单次重试 + 总预算
//!   记账（预算线 CHECKS_BUDGET_MS 的执行面，超时不再傻等）；
//! - [`ResultLedger`]：结果账本——每次会话的三查结果 + 拦停留痕（哈希
//!   链续在 F194 序号链语义上——改一条即断链可检出）；
//! - [`PartialPassSemantics`]：非阻断查降级语义——回 路 查（非阻断）
//!   灰态可放行、存在性/哈希查（阻断）红态必停——分级不是一刀切；
//! - [`budget_split`]：预算分解——350ms 预算按三查权重切分（探测耗时
//!   可预期，排队不失控）。
//!
//! 零堆纪律：定长账本 + 定长链，无 alloc。

use super::handoffchk::{AuditEntry, CheckId, CheckState, CHECKS_BUDGET_MS};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 预算分解
// ---------------------------------------------------------------------------

/// 三查预算权重（存在性 1 : 哈希 3 : 回路 1——哈希逐字节最贵）。
pub const BUDGET_WEIGHTS: [u32; 3] = [1, 3, 1];

/// 按权重切预算（余数给哈希查——最贵的查拿全额）。
pub fn budget_split(total_ms: u64) -> [u64; 3] {
    let sum: u32 = BUDGET_WEIGHTS.iter().sum();
    let unit = total_ms / sum as u64;
    let mut out = [unit * BUDGET_WEIGHTS[0] as u64, unit * BUDGET_WEIGHTS[1] as u64, unit * BUDGET_WEIGHTS[2] as u64];
    let used: u64 = out.iter().sum();
    out[1] += total_ms - used; // 余数归哈希查
    out
}

// ---------------------------------------------------------------------------
// 探测调度器（逐项超时 + 单次重试）
// ---------------------------------------------------------------------------

/// 单查调度记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeOutcome {
    Passed { elapsed_ms: u32 },
    PassedOnRetry { elapsed_ms: u32 },
    Failed,
    TimedOut,
}

/// 单查执行器：探测一次失败 → 重试一次（瞬时抖动不冤枉）→ 仍败判负；
/// 每查超时即断（TimedOut——挂死的探测不拖垮预算）。
pub struct ProbeRunner {
    pub budget_ms: u32,
    elapsed: u32,
    retried: bool,
}

impl ProbeRunner {
    pub const fn new(budget_ms: u32) -> ProbeRunner {
        ProbeRunner { budget_ms, elapsed: 0, retried: false }
    }

    /// 一次探测尝试：返回探测结果与是否还需再试。
    /// `probe_ok` 为底层探测回填（探测面注入——本层只管调度纪律）。
    pub fn attempt(&mut self, cost_ms: u32, probe_ok: bool) -> (ProbeOutcome, bool) {
        self.elapsed = self.elapsed.saturating_add(cost_ms);
        if self.elapsed > self.budget_ms {
            return (ProbeOutcome::TimedOut, false);
        }
        if probe_ok {
            let out = if self.retried { ProbeOutcome::PassedOnRetry { elapsed_ms: self.elapsed } } else { ProbeOutcome::Passed { elapsed_ms: self.elapsed } };
            return (out, false);
        }
        if self.retried {
            (ProbeOutcome::Failed, false)
        } else {
            self.retried = true;
            (ProbeOutcome::TimedOut, true) // 占位态复用：继续 = 再试一次
        }
    }

    pub fn total_elapsed(&self) -> u32 {
        self.elapsed
    }
}

// ---------------------------------------------------------------------------
// 结果账本（拦停留痕 + 哈希链）
// ---------------------------------------------------------------------------

/// 账本条目上限。
pub const LEDGER_CAP: usize = 32;

/// 一次会话的结果账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionRec {
    pub day: u32,
    pub states: [CheckState; 3],
    pub allowed: bool,
    pub chain_hash: u32,
}

/// 结果账本：FNV 续链——每条会话哈希吃前一条（改/删/插即断链）。
pub struct ResultLedger {
    recs: [Option<SessionRec>; LEDGER_CAP],
    n: usize,
    prev_hash: u32,
}

/// 状态序号（账本字节面用——CheckState 无 ord 时的本地单点定义）。
fn state_ord(s: CheckState) -> u8 {
    match s {
        CheckState::Green => 0,
        CheckState::Red => 1,
        CheckState::Exception => 2,
    }
}

fn fnv_step(prev: u32, bytes: &[u8]) -> u32 {
    let mut h = prev ^ 0x811c9dc5;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

impl ResultLedger {
    pub const fn new() -> ResultLedger {
        ResultLedger { recs: [const { None }; LEDGER_CAP], n: 0, prev_hash: 0 }
    }

    /// 登记一次会话（三态 + 放行裁决）→ 留痕入链。
    pub fn record(&mut self, day: u32, states: [CheckState; 3], allowed: bool) -> bool {
        if self.n >= LEDGER_CAP {
            return false;
        }
        let mut bytes = [0u8; 16];
        bytes[0..4].copy_from_slice(&day.to_le_bytes());
        for (i, s) in states.iter().enumerate() {
            bytes[4 + i] = state_ord(*s);
        }
        bytes[7] = allowed as u8;
        bytes[8..12].copy_from_slice(&self.prev_hash.to_le_bytes());
        let chain = fnv_step(0x811c9dc5, &bytes);
        self.recs[self.n] = Some(SessionRec { day, states, allowed, chain_hash: chain });
        self.prev_hash = chain;
        self.n += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn get(&self, i: usize) -> Option<SessionRec> {
        self.recs.get(i).copied().flatten()
    }

    /// 链完整性验证：逐条重算（登记后无人动过 → 全绿；被改 → 定位断点）。
    pub fn chain_intact(&self) -> bool {
        let mut prev = 0u32;
        for i in 0..self.n {
            let r = match self.get(i) {
                Some(r) => r,
                None => return false,
            };
            let mut bytes = [0u8; 16];
            bytes[0..4].copy_from_slice(&r.day.to_le_bytes());
            for (k, s) in r.states.iter().enumerate() {
                bytes[4 + k] = state_ord(*s);
            }
            bytes[7] = r.allowed as u8;
            bytes[8..12].copy_from_slice(&prev.to_le_bytes());
            if fnv_step(0x811c9dc5, &bytes) != r.chain_hash {
                return false;
            }
            prev = r.chain_hash;
        }
        true
    }

    /// 拦停次数统计（审计页消费面）。
    pub fn blocked_count(&self) -> usize {
        (0..self.n).filter(|i| !self.get(*i).unwrap().allowed).count()
    }
}

// ---------------------------------------------------------------------------
// 非阻断查降级语义（分级放行——不是一刀切）
// ---------------------------------------------------------------------------

/// 单查是否阻断级：存在性与哈希查是安全闸（红/异常=必停）；回路查是
/// 体验项（红=可放行但降级提示——回不来时用户至少知道路）。
pub fn is_blocking(id: CheckId) -> bool {
    !matches!(id, CheckId::ReturnPathOk)
}

/// 会话放行裁决：阻断查红或异常（fail-closed）→ 必停；仅非阻断查红 →
/// 可放但降级标记。返回 (可放行, 降级放行)。
pub fn session_verdict(states: &[CheckState; 3]) -> (bool, bool) {
    let mut blocking_bad = false;
    let mut nonblocking_bad = false;
    for (i, s) in states.iter().enumerate() {
        let id = CheckId::ALL[i];
        match s {
            CheckState::Green => {}
            CheckState::Red => {
                if is_blocking(id) {
                    blocking_bad = true;
                } else {
                    nonblocking_bad = true;
                }
            }
            // 预检自身异常永远拦停——fail-closed 不分级豁免。
            CheckState::Exception => blocking_bad = true,
        }
    }
    if blocking_bad {
        (false, false)
    } else {
        (true, nonblocking_bad)
    }
}

// ---------------------------------------------------------------------------
// 批次三自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_handoffchk_b3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F181-b3");

    // 1) 预算分解：350ms → 权重 1:3:1 切分（余数归哈希查）。
    let b = budget_split(CHECKS_BUDGET_MS);
    cs.add(
        "budget_split_weights",
        b[0] == 70 && b[2] == 70 && b[0] + b[1] + b[2] == CHECKS_BUDGET_MS && b[1] >= b[0] * 3,
        "",
    );

    // 2) 探测一次过：Passed 且耗时记录（通过有账）。
    let mut r = ProbeRunner::new(100);
    let (out, again) = r.attempt(20, true);
    cs.add("probe_pass_first", matches!(out, ProbeOutcome::Passed { elapsed_ms: 20 }) && !again, "");

    // 3) 瞬时抖动重试：第一次败 → 再试一次 → 过 = PassedOnRetry。
    let mut r2 = ProbeRunner::new(100);
    let (o1, again1) = r2.attempt(10, false);
    let (o2, again2) = r2.attempt(10, true);
    cs.add(
        "probe_retry_pass",
        again1 && matches!(o1, ProbeOutcome::TimedOut) && matches!(o2, ProbeOutcome::PassedOnRetry { elapsed_ms: 20 }) && !again2,
        "",
    );

    // 4) 两次都败 = Failed（重试不无限——纪律闭环）。
    let mut r3 = ProbeRunner::new(100);
    r3.attempt(10, false);
    let (o3, again3) = r3.attempt(10, false);
    cs.add("probe_fail_after_retry", matches!(o3, ProbeOutcome::Failed) && !again3, "");

    // 5) 超时即断：累计超预算 → TimedOut 不再耗时（预算纪律）。
    let mut r4 = ProbeRunner::new(50);
    r4.attempt(40, false);
    let (o4, _) = r4.attempt(40, true);
    cs.add("probe_timeout_cuts", matches!(o4, ProbeOutcome::TimedOut), "");

    // 6) 账本留痕：两条会话入账、链完整（拦停有迹可循）。
    let mut led = ResultLedger::new();
    led.record(1, [CheckState::Green, CheckState::Green, CheckState::Green], true);
    led.record(2, [CheckState::Red, CheckState::Green, CheckState::Green], false);
    cs.add("ledger_records_and_intact", led.len() == 2 && led.chain_intact() && led.blocked_count() == 1, "");

    // 7) 链断可检出：篡改中间条目 day → 链验证红（F194 语义延续）。
    let mut led2 = ResultLedger::new();
    led2.record(1, [CheckState::Green, CheckState::Green, CheckState::Green], true);
    led2.record(2, [CheckState::Green, CheckState::Green, CheckState::Green], true);
    if let Some(r) = led2.recs[0].as_mut() {
        r.day = 99; // 改历史——必须被检出
    }
    cs.add("ledger_tamper_detected", !led2.chain_intact(), "");

    // 8) 账本满容拒收：32 上限诚实拒（不静默吞）。
    let mut led3 = ResultLedger::new();
    let mut all_ok = true;
    for d in 0..LEDGER_CAP {
        all_ok &= led3.record(d as u32, [CheckState::Green; 3], true);
    }
    cs.add("ledger_cap_honest", all_ok && !led3.record(99, [CheckState::Green; 3], true), "");

    // 9) 阻断分级：存在性/哈希阻断、回路非阻断（分级不是一刀切）。
    cs.add(
        "blocking_classification",
        is_blocking(CheckId::TargetExists) && is_blocking(CheckId::HashTrusted) && !is_blocking(CheckId::ReturnPathOk),
        "",
    );

    // 10) 会话裁决四路：全绿放行 / 阻断红必停 / 仅回路红降级放行 / 异常必停。
    let v1 = session_verdict(&[CheckState::Green, CheckState::Green, CheckState::Green]);
    let v2 = session_verdict(&[CheckState::Red, CheckState::Green, CheckState::Green]);
    let v3 = session_verdict(&[CheckState::Green, CheckState::Green, CheckState::Red]);
    let v4 = session_verdict(&[CheckState::Green, CheckState::Exception, CheckState::Green]);
    cs.add(
        "verdict_four_paths",
        v1 == (true, false) && v2 == (false, false) && v3 == (true, true) && v4 == (false, false),
        "",
    );

    // 11) 裁决与账本联动：降级放行的会话留痕里 allowed=true（审计不丢）。
    let mut led4 = ResultLedger::new();
    let (allowed, _) = session_verdict(&[CheckState::Green, CheckState::Green, CheckState::Red]);
    led4.record(3, [CheckState::Green, CheckState::Green, CheckState::Red], allowed);
    cs.add("degraded_pass_recorded", led4.get(0).unwrap().allowed && led4.chain_intact(), "");

    // 12) 审计条目消费贯通：AuditEntry 字段面（day/action/snapshot6）在册
    // 且主层预算常量一处一事实（350ms）。
    let entry = AuditEntry { day: 7, action: 1, snapshot6: 0b010000 };
    cs.add("audit_entry_shape", entry.day == 7 && entry.action == 1 && CHECKS_BUDGET_MS == 350, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次三）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b3 {
    use super::*;

    #[test]
    fn probe_runner_full_matrix() {
        // 全矩阵：一次过/重试过/失败/超时 四态互不混淆。
        let mut r = ProbeRunner::new(200);
        let (a, _) = r.attempt(10, true);
        let mut r2 = ProbeRunner::new(200);
        r2.attempt(10, false);
        let (b, _) = r2.attempt(10, true);
        let mut r3 = ProbeRunner::new(200);
        r3.attempt(10, false);
        let (c, _) = r3.attempt(10, false);
        let mut r4 = ProbeRunner::new(20);
        r4.attempt(15, false);
        let (d, _) = r4.attempt(15, true);
        assert!(matches!(a, ProbeOutcome::Passed { .. }));
        assert!(matches!(b, ProbeOutcome::PassedOnRetry { .. }));
        assert!(matches!(c, ProbeOutcome::Failed));
        assert!(matches!(d, ProbeOutcome::TimedOut));
    }

    #[test]
    fn chain_survives_full_ledger() {
        // 满账 32 条后链仍完整（容量边界上链验证不糊）。
        let mut led = ResultLedger::new();
        for d in 0..LEDGER_CAP {
            led.record(d as u32, [CheckState::Green; 3], true);
        }
        assert!(led.chain_intact());
        assert_eq!(led.blocked_count(), 0);
    }

    #[test]
    fn budget_never_exceeds_total() {
        // 各档总预算：切分和恒等总预算（余数处理不丢毫秒）。
        for total in [0u64, 1, 5, 350, 1_000, 123_456] {
            let b = budget_split(total);
            assert_eq!(b.iter().sum::<u64>(), total, "total={total}");
        }
    }
}
