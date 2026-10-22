//! CGPU-F0214 · 优先级反转检测与恢复（判据段 0x0700）
//! 优先级反转的运行时检测与恢复：反转检测（高优先级任务被低优先
//! 级任务的资源占用阻塞——等待链分析检测反转模式）、反转类型（有
//! 界/无界——无界反转是严重事件），恢复策略（优先级继承——自动
//! 提升阻塞者优先级/优先级天花板——资源关联优先级预设/人为介入
//! ——报告给调度器重排）。判据：反转检测准确、类型区分、恢复策略、
//! 恢复时间、误报率。
//!
//! 诊断码段 0x0700（本模块独占：0x0700~0x070F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0700;

/// 有界反转的最大容忍等待（微秒）：超此即升格无界。
pub const BOUNDED_WAIT_MAX_US: u32 = 2_000;

/// 恢复时限（微秒）：检测到反转后必须在此内恢复。
pub const RECOVERY_DEADLINE_US: u32 = 1_000;

/// 等待边：任务 v 在等任务 u 持有的资源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WaitEdge {
    /// 持有者。
    pub holder: u32,
    /// 等待者。
    pub waiter: u32,
    /// 已等待时长（微秒）。
    pub waited_us: u32,
}

impl WaitEdge {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.holder == 0 || self.waiter == 0 || self.holder == self.waiter {
            return Err(SEG_BASE);
        }
        Ok(())
    }
}

/// 等待链（判据「反转检测」底座）： holder→waiter 边集合，支持
/// 链上溯。
#[derive(Clone, Debug, Default)]
pub struct WaitGraph {
    edges: Vec<WaitEdge>,
}

impl WaitGraph {
    /// 空图。
    pub fn new() -> Self {
        Self::default()
    }
    /// 加等待边。
    pub fn add(&mut self, e: WaitEdge) -> Result<(), u16> {
        e.validate()?;
        if self.edges.iter().any(|x| x.holder == e.holder && x.waiter == e.waiter) {
            return Err(SEG_BASE + 1);
        }
        self.edges.push(e);
        Ok(())
    }
    /// 边数。
    pub fn len(&self) -> usize {
        self.edges.len()
    }
    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }
    /// 等待者 → 持有者链上溯（判据「等待链分析」）：返回从 start
    /// 开始的链（start 的 holder、holder 的 holder……）。
    pub fn chase(&self, start: u32) -> Result<Vec<u32>, u16> {
        if start == 0 {
            return Err(SEG_BASE + 2);
        }
        let mut chain = vec![start];
        let mut cur = start;
        for _ in 0..self.edges.len() + 1 {
            let next = self.edges.iter().find(|e| e.waiter == cur).map(|e| e.holder);
            match next {
                Some(h) if !chain.contains(&h) => {
                    chain.push(h);
                    cur = h;
                }
                Some(_) => return Err(SEG_BASE + 3), // 链环（死锁）。
                None => break,
            }
        }
        Ok(chain)
    }
}

/// 反转模式（判据「类型区分」一等公民）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InversionKind {
    /// 有界反转：低优先级任务会尽快释放（等待 < 门槛）。
    Bounded,
    /// 无界反转：低优先级任务可能被中优先级无限抢占（严重）。
    Unbounded,
}

impl InversionKind {
    /// 类型码。
    pub fn code(&self) -> u8 {
        match self {
            InversionKind::Bounded => 0,
            InversionKind::Unbounded => 1,
        }
    }
}

/// 反转检测（判据「反转检测准确」主判据）：等待链存在且链首
/// （等待者）优先级高于链尾（最终持有者）→ 反转。等待超门槛或
/// 链中存在中优先级任务可插队 → 无界。
/// 优先级号约定：小 = 高。
pub fn detect_inversion(
    g: &WaitGraph,
    prios: &[u8],
    chain: &[u32],
) -> Result<Option<InversionKind>, u16> {
    if chain.len() < 2 {
        return Ok(None);
    }
    let waiter = chain[0];
    let holder = chain[chain.len() - 1];
    let iw = waiter as usize - 1;
    let ih = holder as usize - 1;
    if iw >= prios.len() || ih >= prios.len() {
        return Err(SEG_BASE + 4);
    }
    if prios[iw] >= prios[ih] {
        return Ok(None); // 等待者优先级不高于持有者 → 无反转。
    }
    // 等待时长判界。
    let waited = g
        .edges
        .iter()
        .filter(|e| e.waiter == waiter)
        .map(|e| e.waited_us)
        .max()
        .unwrap_or(0);
    // 链中间存在「中优先级」任务（介于两者之间）→ 无界反转风险。
    let mid_interferes = chain[1..chain.len() - 1].iter().any(|&m| {
        let im = m as usize - 1;
        im < prios.len() && prios[im] > prios[iw] && prios[im] < prios[ih]
    });
    if mid_interferes || waited > BOUNDED_WAIT_MAX_US {
        Ok(Some(InversionKind::Unbounded))
    } else {
        Ok(Some(InversionKind::Bounded))
    }
}

/// 恢复策略（判据「恢复策略」一等公民）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Recovery {
    /// 优先级继承：持有者优先级提升到等待者。
    Inherit,
    /// 优先级天花板：资源预设上限优先级。
    Ceiling(u8),
    /// 人为介入：报告调度器重排。
    Escalate,
}

impl Recovery {
    /// 策略码。
    pub fn code(&self) -> u8 {
        match self {
            Recovery::Inherit => 0,
            Recovery::Ceiling(_) => 1,
            Recovery::Escalate => 2,
        }
    }
}

/// 恢复策略选择（判据「恢复策略」裁决）：有界反转 → 继承即可；
/// 无界但有天花板预设 → 天花板；无界且无天花板 → 升级介入。
pub fn choose_recovery(kind: InversionKind, ceiling_preset: Option<u8>) -> Recovery {
    match kind {
        InversionKind::Bounded => Recovery::Inherit,
        InversionKind::Unbounded => match ceiling_preset {
            Some(c) => Recovery::Ceiling(c),
            None => Recovery::Escalate,
        },
    }
}

/// 继承执行（判据「恢复策略」面一）：持有者优先级号 = min(自身,
/// 等待者)（号小 = 高）。返回新优先级号。
pub fn apply_inherit(holder_prio: u8, waiter_prio: u8) -> Result<u8, u16> {
    Ok(holder_prio.min(waiter_prio))
}

/// 天花板语义验证（判据「恢复策略」面二）：资源天花板必须 ≥
/// （优先级号 ≤）所有潜在使用者的优先级——否则天花板失效。
pub fn ceiling_covers(ceiling: u8, user_prios: &[u8]) -> Result<(), u16> {
    if user_prios.is_empty() {
        return Err(SEG_BASE + 5);
    }
    if user_prios.iter().any(|&p| p < ceiling) {
        return Err(SEG_BASE + 6);
    }
    Ok(())
}

/// 恢复时间核算（判据「恢复时间」主判据）：从检测到恢复的耗时
/// 不得超过 RECOVERY_DEADLINE_US。
pub fn recovery_in_time(detect_us: u32, recovered_us: u32) -> Result<(), u16> {
    if detect_us == 0 || recovered_us < detect_us {
        return Err(SEG_BASE + 7);
    }
    if recovered_us - detect_us > RECOVERY_DEADLINE_US {
        return Err(SEG_BASE + 8);
    }
    Ok(())
}

/// 误报率审计（判据「误报率」主判据）：检测器的误报数 / 总报警
/// 数 ≤ 50‰（误报过多检测器失真）。
pub fn false_positive_rate_ok(alerts: u32, false_positives: u32) -> Result<bool, u16> {
    if alerts == 0 {
        return Err(SEG_BASE + 9);
    }
    if false_positives > alerts {
        return Err(SEG_BASE + 10);
    }
    Ok(false_positives as u64 * 1000 / alerts as u64 <= 50)
}

/// 升级报告（判据「人为介入」载体）：无界反转升级到调度器的报告
/// 单——链 + 类型 + 建议动作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EscalationReport {
    /// 等待者任务号。
    pub waiter: u32,
    /// 持有者任务号。
    pub holder: u32,
    /// 类型码。
    pub kind_code: u8,
    /// 建议恢复策略码。
    pub advice_code: u8,
}

impl EscalationReport {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.waiter == 0 || self.holder == 0 || self.waiter == self.holder {
            return Err(SEG_BASE + 11);
        }
        if self.kind_code > 1 || self.advice_code > 2 {
            return Err(SEG_BASE + 12);
        }
        Ok(())
    }
}

/// 反转检测全链演练（判据「反转检测准确」到「恢复策略」编排）：
/// 检测 → 选策略 → 生成升级单（仅 Escalate 时）。返回 (类型, 策略,
/// 可选升级单)。
pub fn inversion_pipeline(
    g: &WaitGraph,
    prios: &[u8],
    chain: &[u32],
    ceiling_preset: Option<u8>,
) -> Result<(Option<InversionKind>, Option<Recovery>, Option<EscalationReport>), u16> {
    let kind = detect_inversion(g, prios, chain)?;
    match kind {
        None => Ok((None, None, None)),
        Some(k) => {
            let rec = choose_recovery(k, ceiling_preset);
            let esc = match rec {
                Recovery::Escalate => {
                    let r = EscalationReport {
                        waiter: chain[0],
                        holder: chain[chain.len() - 1],
                        kind_code: k.code(),
                        advice_code: rec.code(),
                    };
                    r.validate()?;
                    Some(r)
                }
                _ => None,
            };
            Ok((Some(k), Some(rec), esc))
        }
    }
}

/// 等待图指纹（可审计面）。
pub fn wait_graph_fp(g: &WaitGraph) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    for e in &g.edges {
        acc = acc.wrapping_mul(0x100000001b3) ^ e.holder as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ e.waiter as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ e.waited_us as u64;
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    fn edge(h: u32, w: u32, us: u32) -> WaitEdge {
        WaitEdge { holder: h, waiter: w, waited_us: us }
    }
    #[test]
    fn chase_builds_chain_and_rejects_cycles() {
        let mut g = WaitGraph::new();
        g.add(edge(2, 1, 100)).unwrap(); // 1 等 2。
        g.add(edge(3, 2, 100)).unwrap(); // 2 等 3。
        assert_eq!(g.chase(1).unwrap(), vec![1, 2, 3]);
        // 环：4 等 5、5 等 4。
        let mut g2 = WaitGraph::new();
        g2.add(edge(5, 4, 10)).unwrap();
        g2.add(edge(4, 5, 10)).unwrap();
        assert!(g2.chase(4).is_err());
    }
    #[test]
    fn inversion_detected_with_kind() {
        let mut g = WaitGraph::new();
        g.add(edge(2, 1, 100)).unwrap(); // 高优先级 1（号小）等低优先级 2。
        let prios = vec![10u8, 100, 50]; // 任务 3 是中优先级。
        // 链 [1,2]：1 等 2，无中插 → 有界。
        let k = detect_inversion(&g, &prios, &[1, 2]).unwrap();
        assert_eq!(k, Some(InversionKind::Bounded));
        // 链 [1,3,2]：3 是中优先级（可插队）→ 无界。
        g.add(edge(3, 1, 100)).unwrap();
        g.add(edge(2, 3, 100)).unwrap();
        let k2 = detect_inversion(&g, &prios, &[1, 3, 2]).unwrap();
        assert_eq!(k2, Some(InversionKind::Unbounded));
    }
    #[test]
    fn no_inversion_when_prio_order_fine() {
        let mut g = WaitGraph::new();
        g.add(edge(1, 2, 50)).unwrap(); // 低优先级 2 等高优先级 1 → 正常。
        let prios = vec![10u8, 100];
        assert_eq!(detect_inversion(&g, &prios, &[2, 1]).unwrap(), None);
    }
    #[test]
    fn unbounded_by_wait_time() {
        let mut g = WaitGraph::new();
        g.add(edge(2, 1, 5000)).unwrap(); // 等待超 2ms 门槛。
        let prios = vec![10u8, 100];
        assert_eq!(detect_inversion(&g, &prios, &[1, 2]).unwrap(), Some(InversionKind::Unbounded));
    }
    #[test]
    fn recovery_choice_and_execution() {
        assert_eq!(choose_recovery(InversionKind::Bounded, None), Recovery::Inherit);
        assert_eq!(choose_recovery(InversionKind::Unbounded, Some(5)), Recovery::Ceiling(5));
        assert!(matches!(choose_recovery(InversionKind::Unbounded, None), Recovery::Escalate));
        // 继承：持有者 100 提升到等待者 10。
        assert_eq!(apply_inherit(100, 10).unwrap(), 10);
        // 天花板覆盖校验。
        assert!(ceiling_covers(5, &[10, 50]).is_ok());
        assert!(ceiling_covers(5, &[3]).is_err());
    }
    #[test]
    fn recovery_time_gate() {
        assert!(recovery_in_time(100, 800).is_ok());
        assert!(recovery_in_time(100, 2000).is_err());
        assert!(recovery_in_time(0, 100).is_err());
    }
    #[test]
    fn false_positive_audited() {
        assert!(false_positive_rate_ok(100, 4).unwrap());
        assert!(!false_positive_rate_ok(100, 60).unwrap());
        assert!(false_positive_rate_ok(0, 0).is_err());
    }
    #[test]
    fn pipeline_end_to_end() {
        let mut g = WaitGraph::new();
        g.add(edge(2, 1, 5000)).unwrap();
        let prios = vec![10u8, 100];
        let (k, r, e) = inversion_pipeline(&g, &prios, &[1, 2], None).unwrap();
        assert_eq!(k, Some(InversionKind::Unbounded));
        assert!(matches!(r, Some(Recovery::Escalate)));
        let rep = e.unwrap();
        assert_eq!((rep.waiter, rep.holder), (1, 2));
        assert!(rep.validate().is_ok());
        // 无反转 → 全 None。
        let (k2, r2, e2) = inversion_pipeline(&g, &[100u8, 10], &[1, 2], None).unwrap();
        assert_eq!((k2, r2, e2), (None, None, None));
    }
}
