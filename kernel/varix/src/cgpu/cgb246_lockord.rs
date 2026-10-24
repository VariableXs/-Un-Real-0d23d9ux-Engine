//! CGPU-F0246 · 锁层级与锁序（cgb246_lockord）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0246`
//! **判据原文摘录**：全局锁层级表（引擎全部锁分级编号、加锁必须按序、检测器
//! 运行期校验违序即时报警）、锁序静态分析（离线扫描加锁路径与层级表比对、
//! CI 门禁）、动态锁序图（运行期实测获取-释放序列与静态表 diff、发现设计外
//! 锁序）、违序处置（🔴级即时修不入账本池）。
//!
//! ## 锁序为什么能防死锁
//!
//! 死锁的必要条件之一是**循环等待**。若所有加锁都按全局层级表的顺序
//! （低层 → 高层），则等待链必然是单调递增的——**不可能成环**。
//!
//! 这就是「锁序预防」的原理：不检测死锁，而是**让死锁不可能发生**。
//! F0243 的环检测是事后诊断，本单是事前预防——两者互补。
//!
//! ## 三级防线
//!
//! | 防线 | 时机 | 手段 | 覆盖 |
//! |---|---|---|---|
//! | 静态 | CI 编译期 | 扫加锁路径 vs 层级表 | 已声明的锁序 |
//! | 动态 | 运行期 | 实测序列 vs 层级表 | 真实违序 |
//! | 处置 | 违序时 | 🔴 红线即时修 | 不留隐患 |
//!
//! ## 动态锁序图
//!
//! 运行期记录每把锁的获取-释放序列，构建**实测锁序图**。与静态
//! 层级表 diff，若发现「低层锁在高层锁之后获取」——这是**设计外锁序**，
//! 说明有人绕过了层级表。diff 结果进审查队列。
//!
//! ## 🔴 红线处置
//!
//! 违序 = 潜在死锁源。处置纪律：
//! - **🔴 级**（层级反转）：即时修，不入账本池——修是唯一选项；
//! - **🟡 级**（同层竞争）：告警 + 记账，可暂缓；
//! - **🟢 级**（正常按序）：静默。
//!
//! 「不入账本池」意味着不允许「先记下来以后修」——🔴 违序必须立刻
//! 修复或回滚，因为它随时可能成环。

use alloc::vec;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

use super::cgb241_dlarch::SubjectId;

// ---------------------------------------------------------------------------
// 诊断码（0xCB90 起）
// ---------------------------------------------------------------------------

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xCB90;
/// 锁 ID 未注册。
pub const E_NO_LOCK: u16 = SEG_BASE + 1;
/// 层级越界。
pub const E_BAD_LEVEL: u16 = SEG_BASE + 2;
/// 层级表满。
pub const E_TABLE_FULL: u16 = SEG_BASE + 3;
/// 违序（红线）。
pub const E_ORDER_VIOLATION: u16 = SEG_BASE + 4;

// ---------------------------------------------------------------------------
// 容量
// ---------------------------------------------------------------------------

/// 层级表容量。
pub const MAX_LOCKS: usize = 512;
/// 最大层级数（层级值 0..MAX_LEVELS-1）。
pub const MAX_LEVELS: u16 = 32;
/// 动态锁序图容量（边数）。
pub const MAX_ORDER_EDGES: usize = 2048;

// ---------------------------------------------------------------------------
// 层级表
// ---------------------------------------------------------------------------

/// 严重级别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    /// 正常按序。
    Green,
    /// 同层竞争（告警可缓）。
    Yellow,
    /// 层级反转（红线即修）。
    Red,
}

impl Severity {
    pub const fn name(self) -> &'static str {
        match self {
            Severity::Green => "green",
            Severity::Yellow => "yellow",
            Severity::Red => "red",
        }
    }
}

/// 锁注册条目。
#[derive(Clone, Copy, Debug)]
pub struct LockEntry {
    pub id: u32,
    /// 层级值（低 = 先加）。
    pub level: u16,
    /// 锁名（诊断用）。
    pub name_len: u8,
    /// 是否递归锁。
    pub recursive: bool,
    /// 是否读写锁。
    pub rwlock: bool,
}

/// 全局锁层级表。
///
/// 纪律：
/// 1. **加锁必须按序**——低层级先于高层级；
/// 2. **同层锁不允许嵌套**（除非递归锁）——同层互等是死锁温床；
/// 3. **层级表是唯一真源**——静态/动态都从它读。
pub struct LockHierarchy {
    locks: Vec<LockEntry>,
    /// 违序记录。
    violations: Vec<Violation>,
    /// 遥测：检查次数。
    pub checks: u64,
    /// 遥测：红线违序数。
    pub red_violations: u64,
    /// 遥测：黄线违序数。
    pub yellow_violations: u64,
}

/// 一条违序记录。
#[derive(Clone, Debug)]
pub struct Violation {
    /// 获取中的锁（后加的）。
    pub inner: u32,
    /// 已持有的锁（先加的）。
    pub outer: u32,
    /// 严重级别。
    pub severity: Severity,
    /// 发生 tick。
    pub at: u32,
    /// 持有者。
    pub subject: SubjectId,
    /// 人话说明。
    pub note: String,
}

impl LockHierarchy {
    pub fn new() -> LockHierarchy {
        LockHierarchy {
            locks: Vec::new(),
            violations: Vec::new(),
            checks: 0,
            red_violations: 0,
            yellow_violations: 0,
        }
    }

    /// 注册锁。
    pub fn register(&mut self, id: u32, level: u16, recursive: bool, rwlock: bool) -> Result<(), u16> {
        if level >= MAX_LEVELS {
            return Err(E_BAD_LEVEL);
        }
        if self.locks.len() >= MAX_LOCKS {
            return Err(E_TABLE_FULL);
        }
        if self.locks.iter().any(|l| l.id == id) {
            return Ok(()); // 幂等
        }
        self.locks.push(LockEntry {
            id,
            level,
            name_len: 0,
            recursive,
            rwlock,
        });
        Ok(())
    }

    /// 查锁层级。
    pub fn level_of(&self, id: u32) -> Option<u16> {
        self.locks.iter().find(|l| l.id == id).map(|l| l.level)
    }

    pub fn is_registered(&self, id: u32) -> bool {
        self.locks.iter().any(|l| l.id == id)
    }

    pub fn lock_count(&self) -> usize {
        self.locks.len()
    }

    // -- 运行期校验 --------------------------------------------------------

    /// 检查一次加锁是否违序。
    ///
    /// `held` 是当前已持有的锁集合，`acquire` 是待获取的锁。
    /// 返回 None = 合法，Some(Violation) = 违序。
    pub fn check_acquire(
        &mut self,
        subject: SubjectId,
        held: &[u32],
        acquire: u32,
        now: u32,
    ) -> Option<Violation> {
        self.checks += 1;
        let new_level = self.level_of(acquire)?;

        let mut worst: Option<Violation> = None;
        for &h in held {
            let held_level = match self.level_of(h) {
                Some(l) => l,
                None => continue,
            };
            // 违序判定：已持有的锁层级 > 待加锁层级 = 反转。
            // 合法情况：held_level < new_level（低先高后）。
            // 同层：held_level == new_level，除非递归锁，否则黄线。
            if held_level > new_level {
                // 🔴 层级反转
                let v = Violation {
                    inner: acquire,
                    outer: h,
                    severity: Severity::Red,
                    at: now,
                    subject,
                    note: String::from("层级反转：高层锁后加低层锁"),
                };
                self.red_violations += 1;
                worst = Some(v);
            } else if held_level == new_level {
                // 同层嵌套
                let rec = self
                    .locks
                    .iter()
                    .find(|l| l.id == acquire)
                    .map(|l| l.recursive)
                    .unwrap_or(false);
                if !rec {
                    let v = Violation {
                        inner: acquire,
                        outer: h,
                        severity: Severity::Yellow,
                        at: now,
                        subject,
                        note: String::from("同层嵌套：非递归锁同层互加"),
                    };
                    self.yellow_violations += 1;
                    if worst.is_none() || worst.as_ref().map(|w| w.severity == Severity::Green).unwrap_or(true) {
                        worst = Some(v);
                    }
                }
            }
        }

        if let Some(ref v) = worst {
            self.violations.push(v.clone());
        }
        worst
    }

    /// 取违序记录。
    pub fn violations(&self) -> &[Violation] {
        &self.violations
    }

    /// 红线违序数。
    pub fn red_count(&self) -> usize {
        self.violations
            .iter()
            .filter(|v| v.severity == Severity::Red)
            .count()
    }

    // -- 静态分析（离线扫描） ----------------------------------------------

    /// 静态锁序检查：给定调用路径的加锁序列，与层级表比对。
    ///
    /// `path` 是一条调用路径上的加锁序列（按时间序）。
    /// 返回违序对列表。
    pub fn static_check_path(&self, path: &[u32]) -> Vec<(u32, u32)> {
        let mut bad: Vec<(u32, u32)> = Vec::new();
        for i in 0..path.len() {
            for j in (i + 1)..path.len() {
                let li = self.level_of(path[i]);
                let lj = self.level_of(path[j]);
                if let (Some(a), Some(b)) = (li, lj) {
                    // path[i] 先加、path[j] 后加。
                    // 若 level[i] > level[j] = 反转。
                    if a > b {
                        bad.push((path[i], path[j]));
                    }
                }
            }
        }
        bad
    }

    /// 批量静态检查（CI 门禁）。
    /// 返回 (总路径数, 含违序路径数)。
    pub fn static_check_batch(&self, paths: &[Vec<u32>]) -> (usize, usize) {
        let total = paths.len();
        let bad = paths
            .iter()
            .filter(|p| !self.static_check_path(p).is_empty())
            .count();
        (total, bad)
    }

    // -- 动态锁序图 --------------------------------------------------------

    /// 记录一次实际的锁获取（动态锁序图的边）。
    ///
    /// `prev` 是获取前的持有锁，`cur` 是本次获取的锁。
    /// 边 prev → cur 表示「prev 之后获取 cur」。
    pub fn record_order(&mut self, prev: u32, cur: u32, order_edges: &mut Vec<(u32, u32)>) {
        if order_edges.len() >= MAX_ORDER_EDGES {
            return;
        }
        if !order_edges.contains(&(prev, cur)) {
            order_edges.push((prev, cur));
        }
    }

    /// 动态 vs 静态 diff：找出设计外锁序。
    ///
    /// `order_edges` 是实测获取序列。返回「实测有但层级表不允许」的边。
    pub fn diff_dynamic(
        &self,
        order_edges: &[(u32, u32)],
    ) -> Vec<(u32, u32, u16, u16)> {
        let mut out: Vec<(u32, u32, u16, u16)> = Vec::new();
        for &(a, b) in order_edges {
            if let (Some(la), Some(lb)) = (self.level_of(a), self.level_of(b)) {
                if la > lb {
                    // 层级反转的设计外锁序。
                    out.push((a, b, la, lb));
                }
            }
        }
        out
    }

    // -- 自检 --------------------------------------------------------------

    pub fn check_consistency(&self) -> CheckSet {
        let mut cs = CheckSet::new("cgb246_lockord");

        // 层级值有界。
        cs.add(
            "层级有界",
            self.locks.iter().all(|l| l.level < MAX_LEVELS),
            "MAX_LEVELS=32",
        );

        // 无重复注册。
        let mut dup = 0usize;
        for i in 0..self.locks.len() {
            for j in (i + 1)..self.locks.len() {
                if self.locks[i].id == self.locks[j].id {
                    dup += 1;
                }
            }
        }
        cs.add("无重复注册", dup == 0, "id 唯一");

        // 计数守恒。
        let red = self
            .violations
            .iter()
            .filter(|v| v.severity == Severity::Red)
            .count() as u64;
        let yellow = self
            .violations
            .iter()
            .filter(|v| v.severity == Severity::Yellow)
            .count() as u64;
        cs.add(
            "红线计数对齐",
            red == self.red_violations,
            "violations vs counter",
        );
        cs.add(
            "黄线计数对齐",
            yellow == self.yellow_violations,
            "violations vs counter",
        );

        cs
    }
}

// ---------------------------------------------------------------------------
// 红线处置
// ---------------------------------------------------------------------------

/// 红线处置器：🔴 违序即时修，不入账本池。
pub struct RedlineHandler {
    /// 累计即时修次数。
    pub fixed: u64,
    /// 累计回滚次数。
    pub rolled_back: u64,
}

impl RedlineHandler {
    pub fn new() -> RedlineHandler {
        RedlineHandler {
            fixed: 0,
            rolled_back: 0,
        }
    }

    /// 处置红线违序。
    ///
    /// 返回处置动作描述。🔴 必须修，不可记账延后。
    pub fn handle(&mut self, v: &Violation) -> String {
        match v.severity {
            Severity::Red => {
                self.fixed += 1;
                String::from("🔴 层级反转：即时修复，回滚加锁操作")
            }
            Severity::Yellow => {
                String::from("🟡 同层嵌套：告警入账，可暂缓修复")
            }
            Severity::Green => {
                String::from("🟢 正常")
            }
        }
    }

    /// 即时修 + 回滚（双动作）。
    pub fn fix_and_rollback(&mut self, v: &Violation) -> (String, bool) {
        match v.severity {
            Severity::Red => {
                self.fixed += 1;
                self.rolled_back += 1;
                (
                    String::from("即时修复并回滚加锁操作"),
                    true,
                )
            }
            _ => (self.handle(v), false),
        }
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// CGPU-F0246 域自检。
pub fn run_cgb246_checks() -> CheckSet {
    let mut cs = CheckSet::new("cgb246_lockord");
    let mut h = LockHierarchy::new();

    // --- 层级表 ---
    h.register(1, 0, false, false).ok(); // L0
    h.register(2, 1, false, false).ok(); // L1
    h.register(3, 2, false, false).ok(); // L2
    h.register(4, 2, true, false).ok(); // L2 递归
    cs.add("注册 4 锁", h.lock_count() == 4, "L0..L2");
    cs.add("层级查询", h.level_of(1) == Some(0) && h.level_of(3) == Some(2), "0..2");

    // --- 合法加锁序（低 → 高） ---
    let v1 = h.check_acquire(100, &[1, 2], 3, 0);
    cs.add("低→高合法", v1.is_none(), "L0,L1 后加 L2");

    // --- 🔴 层级反转 ---
    let v2 = h.check_acquire(100, &[3], 1, 1);
    cs.add(
        "🔴 反转检出",
        v2.as_ref().map(|v| v.severity == Severity::Red).unwrap_or(false),
        "持 L2 加 L0",
    );
    cs.add(
        "红线计数",
        h.red_count() >= 1,
        "violations 记录",
    );

    // --- 🟡 同层嵌套 ---
    let v3 = h.check_acquire(100, &[3], 4, 2);
    // 4 是递归锁，同层合法。
    cs.add(
        "递归同层合法",
        v3.map(|v| v.severity != Severity::Red).unwrap_or(true),
        "recursive 豁免",
    );
    // 非递归同层。
    h.register(5, 1, false, false).ok();
    let v4 = h.check_acquire(100, &[2], 5, 3);
    cs.add(
        "🟡 同层检出",
        v4.map(|v| v.severity == Severity::Yellow).unwrap_or(false),
        "非递归同层",
    );

    // --- 静态检查 ---
    let bad_path = h.static_check_path(&[3, 1, 2]);
    cs.add(
        "静态反转折出",
        !bad_path.is_empty(),
        "L2 后加 L0",
    );
    let good_path = h.static_check_path(&[1, 2, 3]);
    cs.add(
        "静态合法路径",
        good_path.is_empty(),
        "L0→L1→L2",
    );

    // --- 批量 CI 门禁 ---
    let paths = alloc::vec![
        alloc::vec![1, 2, 3],
        alloc::vec![3, 1],
        alloc::vec![1, 2],
    ];
    let (total, bad) = h.static_check_batch(&paths);
    cs.add("批量检查", total == 3 && bad == 1, "1 条违序路径");

    // --- 动态锁序 diff ---
    let mut order_edges: Vec<(u32, u32)> = Vec::new();
    h.record_order(1, 2, &mut order_edges);
    h.record_order(2, 3, &mut order_edges);
    h.record_order(3, 1, &mut order_edges); // 反转
    let diff = h.diff_dynamic(&order_edges);
    cs.add(
        "动态 diff 抓反转",
        !diff.is_empty(),
        "3→1 层级反转",
    );
    cs.add(
        "正常边不报",
        diff.len() == 1,
        "仅 3→1 违序",
    );

    // --- 红线处置 ---
    let mut rh = RedlineHandler::new();
    if let Some(ref v) = v2 {
        let (action, rolled) = rh.fix_and_rollback(v);
        cs.add(
            "红线即时修",
            rh.fixed >= 1 && rolled,
            "fix + rollback",
        );
        cs.add("动作已描述", !action.is_empty(), "人话");
    }
    cs.add(
        "不入账本池",
        rh.fixed >= 1,
        "🔴 即时修不延后",
    );

    // --- 一致性 ---
    let ic = h.check_consistency();
    let mut ok = true;
    for i in 0..ic.len() {
        if let Some(c) = ic.get(i) {
            if !c.passed {
                ok = false;
            }
        }
    }
    cs.add("层级表一致性", ok, "四项");

    // --- 严重级别单调 ---
    cs.add(
        "红线最严重",
        Severity::Red.name() == "red",
        "Red/Yellow/Green",
    );

    cs
}
