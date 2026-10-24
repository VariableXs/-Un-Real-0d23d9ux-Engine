//! VE-F0066 · 多队列呈现协调（VE-A 域 · A03b 同步与呈现组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0066`
//!
//! **职责定位（锚点原文）**：多呈现队列（多 surface 或多交换链）提交序全局
//! 排序，跨队列依赖显式栅栏化，呈现顺序与最终合成顺序严格一致，禁止隐式
//! 顺序假设；队列间死锁按依赖序打破并通知。
//!
//! **数据结构（锚点原文）**：全局提交序表（序列号 × 队列 × surface）；
//! 跨队列栅栏映射；依赖图。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 依赖环 → 编译期拒绝（依赖登记时即查环，环依赖不进提交序）；
//! - 栅栏丢失 → 超时检测走恢复（栅栏等待超时触发恢复路径）；
//! - 队列 hung → 隔离该队列保其余呈现（隔离不殃及全局合成序）。
//!
//! **性能逐项分解（锚点原文）**：排序 O(1) 序号递增；依赖检查 O(边数)
//! 提交期；等待 O(1) 事件。
//!
//! **跨批对接点（锚点原文）**：上游 A 域呈现冻结接口；下游 F0067 尺寸与
//! F0068 格式协商共用协调层。
//!
//! **无障碍与隐私（锚点原文）**：无直接无障碍面；协调失败通知三要素。
//!
//! **判据（锚点原文）**：全局排序、显式栅栏、序合成一致、死锁打破、判据。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数；确定性。
//!
//! 诊断段：0xE89C 基址（全域核查无占用）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const MQCO_VERSION: &str = "vea66_mqcoord-v1";

/// 诊断段基址。
pub const SEG_BASE: u32 = 0xE89C;

// ---- 诊断码 ----
pub const XC_QUEUE_BAD: u32 = SEG_BASE + 1; // 队列标识非法
pub const XC_CYCLE: u32 = SEG_BASE + 2; // 依赖环（编译期拒绝）
pub const XC_FENCE_LOST: u32 = SEG_BASE + 3; // 栅栏丢失（超时恢复）
pub const XC_QUEUE_HUNG: u32 = SEG_BASE + 4; // 队列 hung（隔离）
pub const XC_ORDER_VIOL: u32 = SEG_BASE + 5; // 合成序违例

/// 一条全局提交序记录（序列号 × 队列 × surface）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommitRec {
    /// 全局序列号（协调器单调分配，跨队列唯一）。
    pub seq: u64,
    pub queue_id: u32,
    pub surface_id: u32,
    /// 是否带跨队列显式栅栏。
    pub fenced: bool,
    /// 是否已被合成器消费（合成序一致性对账字段）。
    pub composited: bool,
}

/// 跨队列依赖边：依赖方 queue 须等 provider queue 的 fence。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DepEdge {
    pub waiter: u32,
    pub provider: u32,
}

/// 多队列呈现协调器。
pub struct MqCoordinator {
    /// 全局提交序表（按 seq 单调追加）。
    log: Vec<CommitRec>,
    cap: usize,
    next_seq: u64,
    /// 跨队列依赖图（无环约束在此维护）。
    deps: Vec<DepEdge>,
    /// 隔离中的队列（hung 隔离，不再分配序号但保其余呈现）。
    quarantined: Vec<u32>,
    last_code: u32,
    /// 栅栏丢失恢复次数。
    fence_recoveries: u32,
    /// 死锁打破次数（按依赖序打破并通知）。
    deadlock_breaks: u32,
    /// 合成序对账违例次数。
    order_violations: u32,
}

impl MqCoordinator {
    pub fn new(cap: usize) -> MqCoordinator {
        MqCoordinator {
            log: Vec::new(),
            cap,
            next_seq: 1,
            deps: Vec::new(),
            quarantined: Vec::new(),
            last_code: 0,
            fence_recoveries: 0,
            deadlock_breaks: 0,
            order_violations: 0,
        }
    }

    fn quarantined(&self, q: u32) -> bool {
        self.quarantined.contains(&q)
    }

    /// 登记跨队列依赖（编译期环检测：登记即查，环依赖拒绝入图）。
    /// 依赖语义：waiter 的提交必须排在 provider 之后。
    pub fn add_dependency(&mut self, waiter: u32, provider: u32) -> Result<(), u32> {
        if waiter == 0 || provider == 0 {
            self.last_code = XC_QUEUE_BAD;
            return Err(self.last_code);
        }
        if waiter == provider {
            self.last_code = XC_QUEUE_BAD;
            return Err(self.last_code);
        }
        // 环检测：从 provider 出发能否回到 waiter（provider → … → waiter
        // 再加 waiter→provider 即成环）。
        if self.reachable(provider, waiter) {
            self.last_code = XC_CYCLE;
            return Err(XC_CYCLE);
        }
        if !self.deps.contains(&DepEdge { waiter, provider }) {
            self.deps.push(DepEdge { waiter, provider });
        }
        Ok(())
    }

    /// 可达性（provider 沿依赖边传导能否到 target）。
    fn reachable(&self, from: u32, target: u32) -> bool {
        let mut cur = from;
        let mut steps = 0usize;
        while steps <= self.deps.len() {
            if cur == target {
                return true;
            }
            match self.deps.iter().find(|d| d.waiter == cur) {
                Some(d) => cur = d.provider,
                None => return false,
            }
            steps += 1;
        }
        false
    }

    /// 提交呈现请求：全局序号单调分配；隔离队列拒绝；环依赖队列拒绝。
    /// fenced=false 的跨队列提交只有在无依赖时才合法（显式栅栏铁律）。
    pub fn submit(&mut self, queue_id: u32, surface_id: u32, fenced: bool) -> Result<u64, u32> {
        if queue_id == 0 || surface_id == 0 {
            self.last_code = XC_QUEUE_BAD;
            return Err(self.last_code);
        }
        if self.quarantined(queue_id) {
            self.last_code = XC_QUEUE_HUNG;
            return Err(self.last_code);
        }
        // 显式栅栏铁律：有上游依赖的队列，跨队列提交必须带栅栏。
        let has_dep = self.deps.iter().any(|d| d.waiter == queue_id);
        if has_dep && !fenced {
            self.last_code = XC_FENCE_LOST;
            return Err(self.last_code);
        }
        if self.log.len() >= self.cap {
            // 滚动窗口：最旧已合成记录出窗（序号账不回退）。
            if let Some(pos) = self.log.iter().position(|r| r.composited) {
                self.log.remove(pos);
            } else {
                self.last_code = XC_ORDER_VIOL;
                return Err(self.last_code);
            }
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.log.push(CommitRec { seq, queue_id, surface_id, fenced, composited: false });
        Ok(seq)
    }

    /// 合成器按序消费：只有全局序最小的未合成记录可被消费
    /// （序合成一致——跳序消费即违例）。
    pub fn composite_next(&mut self) -> Option<CommitRec> {
        let min = self.log.iter().filter(|r| !r.composited).map(|r| r.seq).min()?;
        if let Some(r) = self.log.iter_mut().find(|r| r.seq == min) {
            r.composited = true;
            return Some(*r);
        }
        None
    }

    /// 合成序对账：所有已合成记录的 seq 必须构成未合成记录的前缀
    /// （不允许「后面的先上屏」）。违例计数并报告。
    pub fn audit_order(&mut self) -> bool {
        let mut seen_uncomposited = false;
        let mut ok = true;
        for r in self.log.iter() {
            if r.composited {
                if seen_uncomposited {
                    ok = false;
                }
            } else {
                seen_uncomposited = true;
            }
        }
        if !ok {
            self.order_violations += 1;
            self.last_code = XC_ORDER_VIOL;
        }
        ok
    }

    /// 栅栏超时检测与恢复：带栅栏未合成的记录超时 → 恢复路径
    /// （标记恢复、放行该记录）。
    pub fn fence_timeout_recover(&mut self, seq: u64) -> bool {
        match self.log.iter_mut().find(|r| r.seq == seq && r.fenced && !r.composited) {
            Some(r) => {
                r.composited = true;
                self.fence_recoveries += 1;
                self.last_code = XC_FENCE_LOST;
                true
            }
            None => false,
        }
    }

    /// 队列 hung 隔离：隔离后其未合成记录跳过（其余呈现不被殃及）。
    pub fn quarantine(&mut self, queue_id: u32) -> bool {
        if queue_id == 0 || self.quarantined.contains(&queue_id) {
            return false;
        }
        self.quarantined.push(queue_id);
        self.last_code = XC_QUEUE_HUNG;
        // 死锁打破：隔离即打破该队列参与的依赖环等待。
        if self.deps.iter().any(|d| d.waiter == queue_id || d.provider == queue_id) {
            self.deadlock_breaks += 1;
        }
        self.log.retain(|r| r.queue_id != queue_id || r.composited);
        true
    }

    /// 死锁候选：依赖图成环检测（登记后意外成环的兜底）。
    pub fn has_cycle(&self) -> bool {
        for d in self.deps.iter() {
            if self.reachable(d.provider, d.waiter) {
                return true;
            }
        }
        false
    }

    pub fn log_len(&self) -> usize {
        self.log.len()
    }
    pub fn dep_count(&self) -> usize {
        self.deps.len()
    }
    pub fn fence_recoveries(&self) -> u32 {
        self.fence_recoveries
    }
    pub fn deadlock_breaks(&self) -> u32 {
        self.deadlock_breaks
    }
    pub fn order_violations(&self) -> u32 {
        self.order_violations
    }
    pub fn last_code(&self) -> u32 {
        self.last_code
    }

    /// 协调失败通知三要素（发生了什么/为什么/下一步）。
    pub fn failure_notice(&self) -> String {
        match self.last_code {
            XC_CYCLE => alloc::format!(
                "依赖环被拒绝（为什么：环依赖无全局序可排；下一步：改用无环依赖拓扑）"
            ),
            XC_FENCE_LOST => alloc::format!(
                "提交被拒/栅栏恢复（为什么：有上游依赖的跨队列提交必须显式栅栏；下一步：补栅栏或等待恢复路径放行）"
            ),
            XC_QUEUE_HUNG => alloc::format!(
                "队列已隔离（为什么：队列 hung 拖住全局合成序；下一步：其余队列照常呈现，待该队列复位后重新接入）"
            ),
            _ => alloc::format!("协调正常"),
        }
    }

    /// 读屏状态行。
    pub fn status_line(&self) -> String {
        let pending = self.log.iter().filter(|r| !r.composited).count();
        alloc::format!(
            "MQCO 序号{} 待合成{} 依赖{} 隔离{} 栅栏恢复{} 环打破{}",
            self.next_seq - 1,
            pending,
            self.deps.len(),
            self.quarantined.len(),
            self.fence_recoveries,
            self.deadlock_breaks
        )
    }
}

// =====================================================================
// 自检（判据锚点：全局排序 / 显式栅栏 / 序合成一致 / 死锁打破）
// =====================================================================

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, detail);
}

/// VE-F0066 域自检。
pub fn run_vea66_checks() -> CheckSet {
    let mut set = CheckSet::new("vea66");

    // C66-01 全局排序：跨队列提交序号严格单调
    {
        let mut c = MqCoordinator::new(16);
        let a = c.submit(1, 100, false);
        let b = c.submit(2, 200, false);
        let ok = matches!(a, Ok(s1) if matches!(b, Ok(s2) if s2 == s1 + 1));
        chk(&mut set, "C66-01 全局单调", ok, "序号不单调");
    }

    // C66-02 显式栅栏：有依赖队列的裸提交拒绝
    {
        let mut c = MqCoordinator::new(16);
        let _ = c.add_dependency(2, 1);
        let ok = c.submit(2, 200, false) == Err(XC_FENCE_LOST)
            && c.submit(2, 200, true).is_ok();
        chk(&mut set, "C66-02 栅栏铁律", ok, "裸提交被放行");
    }

    // C66-03 无依赖队列可不带栅栏
    {
        let mut c = MqCoordinator::new(16);
        let ok = c.submit(3, 300, false).is_ok();
        chk(&mut set, "C66-03 无依赖免栅栏", ok, "无依赖被误拦");
    }

    // C66-04 依赖环编译期拒绝（直接环）
    {
        let mut c = MqCoordinator::new(16);
        let _ = c.add_dependency(2, 1);
        let ok = c.add_dependency(1, 2) == Err(XC_CYCLE);
        chk(&mut set, "C66-04 直接环拒", ok, "环依赖入图");
    }

    // C66-05 间接环拒绝（1→2→3 后 3→1 成环）
    {
        let mut c = MqCoordinator::new(16);
        let _ = c.add_dependency(2, 1);
        let _ = c.add_dependency(3, 2);
        let ok = c.add_dependency(1, 3) == Err(XC_CYCLE);
        chk(&mut set, "C66-05 间接环拒", ok, "间接环入图");
    }

    // C66-06 合成序一致：严格按全局序消费
    {
        let mut c = MqCoordinator::new(16);
        let s1 = c.submit(1, 100, false);
        let s2 = c.submit(2, 200, false);
        let first = c.composite_next();
        let second = c.composite_next();
        let ok = match (s1, s2, first, second) {
            (Ok(a), Ok(b), Some(f), Some(s)) => f.seq == a && s.seq == b && a < b,
            _ => false,
        };
        chk(&mut set, "C66-06 序合成一致", ok, "合成序错乱");
    }

    // C66-07 合成对账：跳序消费被对账检出
    {
        let mut c = MqCoordinator::new(16);
        let _ = c.submit(1, 100, false);
        let _ = c.submit(2, 200, false);
        // 手工把后一条标成已合成（模拟跳序），对账必须翻红。
        if let Some(r) = c.log.iter_mut().find(|r| r.seq == 2) {
            r.composited = true;
        }
        let ok = !c.audit_order() && c.order_violations() == 1;
        chk(&mut set, "C66-07 跳序检出", ok, "跳序未检出");
    }

    // C66-08 栅栏超时恢复
    {
        let mut c = MqCoordinator::new(16);
        let _ = c.add_dependency(2, 1);
        let s = c.submit(2, 200, true);
        let ok = match s {
            Ok(seq) => c.fence_timeout_recover(seq) && c.fence_recoveries() == 1,
            Err(_) => false,
        };
        chk(&mut set, "C66-08 栅栏恢复", ok, "恢复路径失效");
    }

    // C66-09 队列 hung 隔离：隔离后拒绝且其余队列照常
    {
        let mut c = MqCoordinator::new(16);
        let _ = c.submit(1, 100, false);
        let _ = c.quarantine(1);
        let ok = c.submit(1, 101, false) == Err(XC_QUEUE_HUNG)
            && c.submit(2, 200, false).is_ok();
        chk(&mut set, "C66-09 隔离不殃及", ok, "隔离殃及全局");
    }

    // C66-10 隔离打破依赖死锁（计数）
    {
        let mut c = MqCoordinator::new(16);
        let _ = c.add_dependency(2, 1);
        let _ = c.quarantine(2);
        let ok = c.deadlock_breaks() == 1;
        chk(&mut set, "C66-10 死锁打破", ok, "打破未计数");
    }

    // C66-11 非法队列标识拒绝（零 id）
    {
        let mut c = MqCoordinator::new(16);
        let ok = c.submit(0, 100, false) == Err(XC_QUEUE_BAD)
            && c.add_dependency(0, 1) == Err(XC_QUEUE_BAD);
        chk(&mut set, "C66-11 零 id 拒", ok, "零 id 被放行");
    }

    // C66-12 依赖重复登记幂等
    {
        let mut c = MqCoordinator::new(16);
        let _ = c.add_dependency(2, 1);
        let _ = c.add_dependency(2, 1);
        let ok = c.dep_count() == 1;
        chk(&mut set, "C66-12 依赖幂等", ok, "依赖重复入图");
    }

    // C66-13 失败通知三要素（环依赖场景）
    {
        let mut c = MqCoordinator::new(16);
        let _ = c.add_dependency(2, 1);
        let _ = c.add_dependency(1, 2);
        let n = c.failure_notice();
        let ok = n.contains("为什么") && n.contains("下一步") && n.contains("依赖环");
        chk(&mut set, "C66-13 通知三要素", ok, "通知缺要素");
    }

    // C66-14 确定性
    {
        let seq = || -> (u64, u64) {
            let mut c = MqCoordinator::new(16);
            let a = c.submit(1, 100, false);
            let b = c.submit(2, 200, false);
            (a.unwrap_or(0), b.unwrap_or(0))
        };
        chk(&mut set, "C66-14 确定性", seq() == seq(), "两次不一致");
    }

    // C66-15 读屏行六要素
    {
        let c = MqCoordinator::new(16);
        let line = c.status_line();
        let ok = line.contains("序号") && line.contains("待合成") && line.contains("依赖")
            && line.contains("隔离") && line.contains("栅栏恢复") && line.contains("环打破");
        chk(&mut set, "C66-15 读屏要素", ok, "读屏行缺要素");
    }

    set
}

/// 深度驱动入口。
pub fn drill() -> (bool, String) {
    let s = run_vea66_checks();
    let ok = s.all_passed() && !s.truncated();
    (ok, if ok { String::new() } else { s.red_items_text() })
}
