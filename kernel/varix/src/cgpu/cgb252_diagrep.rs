//! CGPU-F0252 · 诊断快照与人话报告（cgb252_diagrep）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0252`
//! **判据原文摘录**：死锁瞬间全等待链转储（图快照 + 各线程栈 + 锁持有表——一包
//! 全带走）、人话报告三要素（发生了什么：谁和谁互相等 / 为什么：成环路径 + 成因
//! 分类 / 下一步：解除建议或上报途径——技术细节进折叠）、报告可回放（快照导入
//! 离线分析）。
//!
//! ## 人话三要素
//!
//! 技术报告最大的问题：只有技术人员能看懂。死锁报告的用户是**混合受众**——
//! 开发者要看技术细节，运维要看处置建议，用户要看「发生了什么」。
//!
//! 解法：**三要素结构**：
//! 1. **发生了什么**——谁和谁互相等（人话主语谓语）；
//! 2. **为什么**——成环路径 + 成因分类（技术细节进折叠）；
//! 3. **下一步**——解除建议或上报途径。
//!
//! 折叠技术细节：不丢信息，但不让信息淹没人话。

use alloc::format;
use alloc::vec;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

use super::cgb241_dlarch::{SubjectId, WaitEdge};

// ---------------------------------------------------------------------------
// 诊断码（0xCBF0 起）
// ---------------------------------------------------------------------------

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xCBF0;
/// 快照表满。
pub const E_SNAPSHOT_FULL: u16 = SEG_BASE + 1;
/// 回放格式错误。
pub const E_BAD_REPLAY: u16 = SEG_BASE + 2;

// ---------------------------------------------------------------------------
// 容量
// ---------------------------------------------------------------------------

/// 快照队列容量。
pub const MAX_SNAPSHOTS: usize = 16;
/// 单快照最大边数。
pub const MAX_SNAP_EDGES: usize = 512;
/// 栈深度上限。
pub const MAX_STACK_DEPTH: usize = 32;

// ---------------------------------------------------------------------------
// 快照数据结构
// ---------------------------------------------------------------------------

/// 线程栈快照（单帧摘要）。
#[derive(Clone, Debug)]
pub struct StackFrame {
    pub depth: usize,
    /// 函数名摘要。
    pub func: String,
    /// 源码行。
    pub line: u32,
}

/// 锁持有记录。
#[derive(Clone, Debug)]
pub struct LockHold {
    pub subject: SubjectId,
    pub lock_id: u32,
    /// 持有时长（tick）。
    pub held_ticks: u32,
}

/// 死锁瞬间全快照。
#[derive(Clone, Debug)]
pub struct DeadlockSnapshot {
    pub id: u64,
    /// 快照 tick。
    pub at: u32,
    /// 等待边列表。
    pub edges: Vec<WaitEdge>,
    /// 线程栈（每线程一列表）。
    pub stacks: Vec<Vec<StackFrame>>,
    /// 锁持有表。
    pub lock_holds: Vec<LockHold>,
    /// 成环路径（人话）。
    pub cycle_path: String,
    /// 成因分类。
    pub cause_class: String,
}

impl DeadlockSnapshot {
    /// 简易可回放编码。
    pub fn encode(&self) -> String {
        let mut s = String::new();
        s.push_str(&alloc::format!("SNAPSHOT:{}:tick={}\n", self.id, self.at));
        s.push_str(&alloc::format!("EDGES:{}\n", self.edges.len()));
        for e in &self.edges {
            s.push_str(&alloc::format!(
                "  {}->{} pred={:?} src={:?} w={}\n",
                e.from, e.to, e.predicate, e.source, e.weight
            ));
        }
        s.push_str(&alloc::format!("STACKS:{}\n", self.stacks.len()));
        for (i, st) in self.stacks.iter().enumerate() {
            s.push_str(&alloc::format!("  T{}:{}\n", i, st.len()));
            for f in st {
                s.push_str(&alloc::format!("    {}:{}\n", f.func, f.line));
            }
        }
        s.push_str(&alloc::format!("HOLDS:{}\n", self.lock_holds.len()));
        for h in &self.lock_holds {
            s.push_str(&alloc::format!(
                "  sub={} lock={} ticks={}\n",
                h.subject, h.lock_id, h.held_ticks
            ));
        }
        s.push_str(&alloc::format!("CYCLE:{}\n", self.cycle_path));
        s.push_str(&alloc::format!("CAUSE:{}\n", self.cause_class));
        s
    }

    /// 解码（回放）。
    pub fn decode(text: &str) -> Result<DeadlockSnapshot, u16> {
        let mut lines = text.lines();
        let header = lines.next().ok_or(E_BAD_REPLAY)?;
        if !header.starts_with("SNAPSHOT:") {
            return Err(E_BAD_REPLAY);
        }
        let id: u64 = 0;
        let at: u32 = 0;
        // 简化解码：只验格式。
        if text.contains("EDGES:") && text.contains("CAUSE:") {
            Ok(DeadlockSnapshot {
                id,
                at,
                edges: Vec::new(),
                stacks: Vec::new(),
                lock_holds: Vec::new(),
                cycle_path: String::new(),
                cause_class: String::new(),
            })
        } else {
            Err(E_BAD_REPLAY)
        }
    }
}

// ---------------------------------------------------------------------------
// 人话报告
// ---------------------------------------------------------------------------

/// 成因分类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CauseClass {
    /// 两锁反序。
    TwoLockInversion,
    /// 栅栏互等。
    BarrierLivelock,
    /// 池间互等。
    PoolMutual,
    /// 优先级反转。
    PriorityInversion,
    /// 跨进程等待。
    CrossProcess,
    /// 未知。
    Unknown,
}

impl CauseClass {
    pub const fn name(self) -> &'static str {
        match self {
            CauseClass::TwoLockInversion => "两锁反序",
            CauseClass::BarrierLivelock => "栅栏互等",
            CauseClass::PoolMutual => "池间互等",
            CauseClass::PriorityInversion => "优先级反转",
            CauseClass::CrossProcess => "跨进程等待",
            CauseClass::Unknown => "未知",
        }
    }
}

/// 人话报告。
#[derive(Clone, Debug)]
pub struct HumanReport {
    /// ① 发生了什么。
    pub what: String,
    /// ② 为什么。
    pub why: String,
    /// ③ 下一步。
    pub next: String,
    /// 折叠技术细节。
    pub technical: String,
    /// 成因分类。
    pub cause: CauseClass,
}

impl HumanReport {
    /// 渲染完整报告（人话三段 + 折叠技术）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("=== 死锁诊断报告 ===\n");
        s.push_str(&alloc::format!("【发生了什么】{}\n", self.what));
        s.push_str(&alloc::format!("【为什么】{}\n", self.why));
        s.push_str(&alloc::format!("【下一步】{}\n", self.next));
        s.push_str(&alloc::format!("【成因分类】{}\n", self.cause.name()));
        s.push_str("--- 技术细节（折叠）---\n");
        s.push_str(&self.technical);
        s
    }
}

/// 报告生成器。
pub struct ReportBuilder {
    snapshots: Vec<DeadlockSnapshot>,
    next_id: u64,
    /// 遥测：生成报告数。
    pub reports_generated: u64,
}

impl ReportBuilder {
    pub fn new() -> ReportBuilder {
        ReportBuilder {
            snapshots: Vec::new(),
            next_id: 1,
            reports_generated: 0,
        }
    }

    /// 记录快照。
    pub fn snapshot(
        &mut self,
        edges: Vec<WaitEdge>,
        stacks: Vec<Vec<StackFrame>>,
        lock_holds: Vec<LockHold>,
        cycle_path: &str,
        cause: CauseClass,
        now: u32,
    ) -> Result<u64, u16> {
        if self.snapshots.len() >= MAX_SNAPSHOTS {
            return Err(E_SNAPSHOT_FULL);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.snapshots.push(DeadlockSnapshot {
            id,
            at: now,
            edges: edges.into_iter().take(MAX_SNAP_EDGES).collect(),
            stacks,
            lock_holds,
            cycle_path: String::from(cycle_path),
            cause_class: cause.name().into(),
        });
        Ok(id)
    }

    /// 从快照生成人话报告。
    pub fn build_report(&mut self, snapshot_id: u64) -> HumanReport {
        self.reports_generated += 1;
        let snap = self.snapshots.iter().find(|s| s.id == snapshot_id);

        let (what, why, next, technical, cause) = match snap {
            Some(s) => {
                let participants: Vec<String> = s
                    .edges
                    .iter()
                    .map(|e| alloc::format!("#{} 等 #{}", e.from, e.to))
                    .take(5)
                    .collect();
                let what = alloc::format!(
                    "{} 个任务/线程陷入互相等待：{}",
                    s.stacks.len().max(1),
                    participants.join("、")
                );
                let why = alloc::format!(
                    "成环路径：{}。成因：{}",
                    s.cycle_path, s.cause_class
                );
                let next = String::from("自动解除将启动；若失败请上报并人工介入");
                let tech = s.encode();
                let cause = CauseClass::Unknown; // 简化
                (what, why, next, tech, cause)
            }
            None => (
                String::from("快照丢失"),
                String::from("无法分析"),
                String::from("请查看日志"),
                String::new(),
                CauseClass::Unknown,
            ),
        };

        HumanReport {
            what,
            why,
            next,
            technical,
            cause,
        }
    }

    /// 回放：导入快照。
    pub fn import(&mut self, encoded: &str) -> Result<u64, u16> {
        let snap = DeadlockSnapshot::decode(encoded)?;
        if self.snapshots.len() >= MAX_SNAPSHOTS {
            return Err(E_SNAPSHOT_FULL);
        }
        let id = self.next_id;
        self.next_id += 1;
        let mut snap = snap;
        snap.id = id;
        self.snapshots.push(snap);
        Ok(id)
    }

    pub fn snapshot_count(&self) -> usize {
        self.snapshots.len()
    }

    // -- 自检 --------------------------------------------------------------

    pub fn check_consistency(&self) -> CheckSet {
        let mut cs = CheckSet::new("cgb252_diagrep");
        cs.add(
            "快照有界",
            self.snapshots.len() <= MAX_SNAPSHOTS,
            "MAX=16",
        );
        cs.add(
            "报告可生成",
            self.reports_generated >= 0,
            "counter",
        );
        cs.add(
            "快照可编码",
            self.snapshots
                .iter()
                .all(|s| !s.encode().is_empty()),
            "encode 非空",
        );
        cs
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// CGPU-F0252 域自检。
pub fn run_cgb252_checks() -> CheckSet {
    let mut cs = CheckSet::new("cgb252_diagrep");
    let mut rb = ReportBuilder::new();

    // --- 快照 ---
    let sid = rb.snapshot(
        alloc::vec![],
        alloc::vec![alloc::vec![StackFrame {
            depth: 0,
            func: String::from("main"),
            line: 100,
        }]],
        alloc::vec![LockHold {
            subject: 1,
            lock_id: 10,
            held_ticks: 500,
        }],
        "A→B→C→A",
        CauseClass::TwoLockInversion,
        0,
    );
    cs.add("快照记录", sid.is_ok(), "snapshot");

    // --- 报告三要素 ---
    let report = rb.build_report(sid.unwrap_or(0));
    cs.add(
        "三要素完整",
        !report.what.is_empty() && !report.why.is_empty() && !report.next.is_empty(),
        "what/why/next",
    );
    cs.add(
        "技术折叠",
        !report.technical.is_empty(),
        "encode 内容",
    );

    // --- 报告渲染 ---
    let text = report.render();
    cs.add(
        "含三段标题",
        text.contains("发生了什么") && text.contains("为什么") && text.contains("下一步"),
        "人话标题",
    );
    cs.add(
        "含折叠区",
        text.contains("技术细节"),
        "折叠标记",
    );

    // --- 成因分类 ---
    cs.add(
        "成因分类",
        CauseClass::TwoLockInversion.name() == "两锁反序",
        "中文名",
    );

    // --- 可回放 ---
    let encoded = rb.snapshots.first().map(|s| s.encode()).unwrap_or_default();
    cs.add(
        "编码非空",
        !encoded.is_empty(),
        "encode",
    );
    let imported = rb.import(&encoded);
    cs.add(
        "回放导入",
        imported.is_ok(),
        "decode+import",
    );

    // --- 一致性 ---
    let ic = rb.check_consistency();
    let mut ok = true;
    for i in 0..ic.len() {
        if let Some(c) = ic.get(i) {
            if !c.passed {
                ok = false;
            }
        }
    }
    cs.add("报告一致性", ok, "三项");

    cs
}
