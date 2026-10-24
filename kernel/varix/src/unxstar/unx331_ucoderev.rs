//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0331 · AP 微码修订只读检测（F0187 联动）· 诊断段 0x3d70
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   UNX-F0331-J1 AP 微码修订与 BSP 一致断言，不一致检出有账（只读）。
//! 难点分析：
//!   微码修订号是「同一颗硅片上所有核必须共享」的安全基线——BSP 打了
//!   微码补丁而 AP 没跟上时，同一指令在两颗核上语义分叉（侧面通道
//!   缓解/缺陷修复只在一半核心生效），这是最难查的静默故障。检测
//!   必须只读（读 MSR 0x8B 不许写、不许触发重加载），且必须留账：
//!   哪颗核、读到的修订号、与 BSP 的差、何时发现。
//! 联动：F0187 微码修订登记——BSP 侧登记的修订号快照 ID 必须与本次
//!   检测引用的快照一致，快照漂移即拒检（防拿旧账断新核）。
//! 纪律约束：
//!   - 只读红线：本模块 API 面不存在任何写通道；
//!   - 快照一致性：F0187 快照 ID 不匹配即拒，不猜；
//!   - 不一致有账：核号+双修订号+发现序号，append-only；
//!   - 探针失败显性：读不到修订号按 ProbeFail 入账，不假装一致。
//! 成本记账：单核检测 O(1)；全 AP 巡检 O(核数)；账 O(分叉数)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（全仓普查 0x3d70 空闲后占段）。
pub const SEG_BASE: u16 = 0x3d70;

// ─────────────────────────────────────────────────────────────────────
// 一、错误闭集
// ─────────────────────────────────────────────────────────────────────

/// 微码检测错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UcodeErr {
    /// F0187 快照 ID 不匹配：引用的不是当前登记快照。
    SnapshotMismatch,
    /// 探针失败：修订号读不到。
    ProbeFail,
    /// 核号越界：巡检表没有这颗核。
    CpuOutOfRange,
    /// 账满：不一致账已满（防呆上限）。
    LedgerFull,
}

impl UcodeErr {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE
            + match self {
                UcodeErr::SnapshotMismatch => 1,
                UcodeErr::ProbeFail => 2,
                UcodeErr::CpuOutOfRange => 3,
                UcodeErr::LedgerFull => 4,
            }
    }

    /// 人话 + 建议。
    pub fn detail(self) -> &'static str {
        match self {
            UcodeErr::SnapshotMismatch =>
                "F0187 快照 ID 不匹配——检测引用的登记快照已过期。建议刷新快照后重测。",
            UcodeErr::ProbeFail =>
                "修订号读不到——MSR 读面不可用。建议按探针失败入账并标记该核待检。",
            UcodeErr::CpuOutOfRange =>
                "核号越界——巡检表没有这颗核。建议核对 APIC ID 登记账。",
            UcodeErr::LedgerFull =>
                "不一致账满——分叉规模超出兜底。建议先处置已入账项再继续巡检。",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// 二、F0187 快照引用（联动闸）
// ─────────────────────────────────────────────────────────────────────

/// 当前登记快照 ID（F0187 侧写入，本模块只读比对）。
/// 值取自 F0187 快照账的当前头部序号；构造期冻结。
pub struct SnapshotRef {
    id: u64,
}

impl SnapshotRef {
    /// 从 F0187 头部序号构造（构造即冻结，无 setters——只读红线）。
    pub const fn new(id: u64) -> SnapshotRef {
        SnapshotRef { id }
    }

    /// 快照 ID。
    pub const fn id(&self) -> u64 {
        self.id
    }
}

// ─────────────────────────────────────────────────────────────────────
// 三、修订号探针（只读语义）
// ─────────────────────────────────────────────────────────────────────

/// 修订号读通道：给核号返回 Option<u64>。None=读不到。
/// 本模块不触真机——真机实现注入此闭包，构建侧用夹具对拍。
pub type RevProbe<'a> = dyn Fn(u32) -> Option<u64> + 'a;

/// 单核微码档案。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UcodeRec {
    /// 核号（APIC ID 投影）。
    pub cpu: u32,
    /// 读到的修订号。
    pub revision: u64,
    /// 发现序号（全仓巡检递增）。
    pub seq: u32,
}

// ─────────────────────────────────────────────────────────────────────
// 四、巡检器与不一致账
// ─────────────────────────────────────────────────────────────────────

/// 不一致账上限（防呆兜底）。
pub const LEDGER_CAP: usize = 32;

/// 巡检结果账：一致表 + 不一致账（append-only）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevLedger {
    /// 快照 ID（建账时冻结）。
    pub snapshot: u64,
    /// BSP 修订号（巡检基准）。
    pub bsp_rev: u64,
    /// 一致核表。
    pub consistent: Vec<UcodeRec>,
    /// 不一致账（append-only，核号+双值+序号）。
    pub divergent: Vec<UcodeRec>,
    /// 探针失败核表（只记核号，按失败序）。
    pub probe_failed: Vec<u32>,
    /// 下一个发现序号。
    next_seq: u32,
}

impl RevLedger {
    /// 建账：快照与 BSP 基线在建账一刻冻结。
    pub fn new(snapshot: &SnapshotRef, bsp_rev: u64) -> RevLedger {
        RevLedger {
            snapshot: snapshot.id(),
            bsp_rev,
            consistent: Vec::new(),
            divergent: Vec::new(),
            probe_failed: Vec::new(),
            next_seq: 1,
        }
    }

    /// 巡检一颗 AP：读修订号 → 对账 → 归桶。只读，零写通道。
    pub fn inspect(
        &mut self,
        cpu: u32,
        probe: &RevProbe,
        expect_snapshot: &SnapshotRef,
    ) -> Result<(), UcodeErr> {
        // 快照一致性闸：巡检引用的快照必须就是建账快照
        if expect_snapshot.id() != self.snapshot {
            return Err(UcodeErr::SnapshotMismatch);
        }
        match probe(cpu) {
            Some(rev) => {
                let rec = UcodeRec { cpu, revision: rev, seq: self.next_seq };
                self.next_seq += 1;
                if rev == self.bsp_rev {
                    self.consistent.push(rec);
                } else {
                    if self.divergent.len() >= LEDGER_CAP {
                        return Err(UcodeErr::LedgerFull);
                    }
                    self.divergent.push(rec);
                }
                Ok(())
            }
            None => {
                let rec = UcodeRec { cpu, revision: 0, seq: self.next_seq };
                self.next_seq += 1;
                let _ = rec; // 失败核不占一致/分叉两桶，只入失败表
                self.probe_failed.push(cpu);
                Ok(())
            }
        }
    }

    /// J1 断言面：全体已检核一致（无分叉无失败）。
    pub fn all_consistent(&self) -> bool {
        self.divergent.is_empty() && self.probe_failed.is_empty()
    }

    /// 账守恒：一致数 + 分叉数 + 失败数 = 发现序号 - 1。
    pub fn conserved(&self) -> bool {
        self.consistent.len() + self.divergent.len() + self.probe_failed.len()
            == (self.next_seq - 1) as usize
    }

    /// append-only 证明：分叉账只增不改——重放同一核必产生新序号行。
    pub fn append_only(&self) -> bool {
        self.divergent
            .windows(2)
            .all(|w| w[1].seq > w[0].seq)
    }
}

// ─────────────────────────────────────────────────────────────────────
// 五、夹具（构建侧对拍）
// ─────────────────────────────────────────────────────────────────────

/// BSP 黄金修订号（来源：F0187 登记快照投影，构建期冻结常量）。
pub const GOLDEN_BSP_REV: u64 = 0x0000_00F0;

/// 黄金快照 ID。
pub const GOLDEN_SNAPSHOT: u64 = 2026_1022_001;

/// 同构探针：所有核都报 BSP 修订号。
pub fn homogeneous_rev_probe() -> impl Fn(u32) -> Option<u64> {
    |_| Some(GOLDEN_BSP_REV)
}

/// 分叉探针：指定核报旧修订号（模拟微码未跟上）。
pub fn stale_rev_probe(bad_cpu: u32, old_rev: u64) -> impl Fn(u32) -> Option<u64> {
    move |cpu| {
        if cpu == bad_cpu {
            Some(old_rev)
        } else {
            Some(GOLDEN_BSP_REV)
        }
    }
}

/// 失联探针：指定核读不到。
pub fn blind_probe(bad_cpu: u32) -> impl Fn(u32) -> Option<u64> {
    move |cpu| if cpu == bad_cpu { None } else { Some(GOLDEN_BSP_REV) }
}

// ─────────────────────────────────────────────────────────────────────
// 五·五、修订史账（每核修订号时间线，append-only）
// ─────────────────────────────────────────────────────────────────────

/// 一条修订史行：某核在某序号读到的修订号。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevHistoryRow {
    pub cpu: u32,
    pub revision: u64,
    /// 采集序号（全局递增）。
    pub seq: u32,
}

/// 修订史账：跨多轮巡检的 append-only 时间线。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevHistory {
    rows: Vec<RevHistoryRow>,
}

impl RevHistory {
    pub fn new() -> RevHistory {
        RevHistory { rows: Vec::new() }
    }

    /// 记一行（append-only：只 push，不排序不改写）。
    pub fn record(&mut self, cpu: u32, revision: u64) -> u32 {
        let seq = self.rows.len() as u32 + 1;
        self.rows.push(RevHistoryRow { cpu, revision, seq });
        seq
    }

    /// append-only 性质：seq 严格递增且与行号一一对应。
    pub fn append_only(&self) -> bool {
        self.rows
            .iter()
            .enumerate()
            .all(|(i, r)| r.seq == i as u32 + 1)
    }

    /// 修订回退检测：同一核后读到的修订号 < 前读到的 → 真回退（微码不可能降级）。
    pub fn rollbacks(&self) -> Vec<(u32, u64, u64)> {
        let mut out = Vec::new();
        for i in 0..self.rows.len() {
            for j in (i + 1)..self.rows.len() {
                if self.rows[i].cpu == self.rows[j].cpu
                    && self.rows[j].revision < self.rows[i].revision
                {
                    out.push((
                        self.rows[j].cpu,
                        self.rows[i].revision,
                        self.rows[j].revision,
                    ));
                }
            }
        }
        out
    }

    /// 核数。
    pub fn cpus(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        for r in self.rows.iter() {
            if !v.contains(&r.cpu) {
                v.push(r.cpu);
            }
        }
        v
    }
}

impl Default for RevHistory {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────
// 六、判据入口
// ─────────────────────────────────────────────────────────────────────

/// UNX-F0331 判据：AP 微码修订与 BSP 一致断言，不一致检出有账（只读）。
pub fn run_unx331_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0331 AP 微码修订只读检测");

    // J1 主判据：四核同构巡检 → 全一致
    let snap = SnapshotRef::new(GOLDEN_SNAPSHOT);
    let mut lg = RevLedger::new(&snap, GOLDEN_BSP_REV);
    let probe = homogeneous_rev_probe();
    let mut all_inspected = true;
    for cpu in 1..4u32 {
        if lg.inspect(cpu, &probe, &snap).is_err() {
            all_inspected = false;
        }
    }
    cs.add("巡检核1~3受理", all_inspected, "一致桶");
    cs.add("J1 全体一致", lg.all_consistent(), "同构断言");
    cs.add("账守恒", lg.conserved(), "三桶和=序号-1");
    cs.add("一致桶=3", lg.consistent.len() == 3, "核1/2/3");
    cs.add("快照冻结", lg.snapshot == GOLDEN_SNAPSHOT, "F0187 联动");

    // 注入 1：分叉核 → 检出且账行序号递增（append-only）
    let mut lg2 = RevLedger::new(&snap, GOLDEN_BSP_REV);
    let p2 = stale_rev_probe(2, GOLDEN_BSP_REV - 0x10);
    for cpu in 1..4u32 {
        let _ = lg2.inspect(cpu, &p2, &snap);
    }
    cs.add("注入1 分叉检出", lg2.divergent.len() == 1, "恰核2");
    cs.add("注入1 定位准确",
        lg2.divergent.first().map(|r| r.cpu) == Some(2),
        "cpu=2");
    cs.add("注入1 旧修订号入账",
        lg2.divergent.first().map(|r| r.revision) == Some(GOLDEN_BSP_REV - 0x10),
        "双值账");
    cs.add("注入1 append-only", lg2.append_only(), "序号严格递增");
    cs.add("注入1 不全体一致", !lg2.all_consistent(), "断言翻转");
    cs.add("注入1 守恒", lg2.conserved(), "1+2+0=3");

    // 注入 2：失联核 → ProbeFail 桶，不假装一致
    let mut lg3 = RevLedger::new(&snap, GOLDEN_BSP_REV);
    let p3 = blind_probe(3);
    for cpu in 1..5u32 {
        let _ = lg3.inspect(cpu, &p3, &snap);
    }
    cs.add("注入2 失联入账", lg3.probe_failed == vec![3], "核3失败表");
    cs.add("注入2 不占分叉桶", lg3.divergent.is_empty(), "失败≠分叉");
    cs.add("注入2 守恒", lg3.conserved(), "3+0+1=4");

    // 注入 3：快照漂移 → 拒检（不拿旧账断新核）
    let stale = SnapshotRef::new(GOLDEN_SNAPSHOT - 1);
    let mut lg4 = RevLedger::new(&snap, GOLDEN_BSP_REV);
    let r4 = lg4.inspect(1, &homogeneous_rev_probe(), &stale);
    cs.add("注入3 快照漂移拒检", r4 == Err(UcodeErr::SnapshotMismatch), UcodeErr::SnapshotMismatch.detail());
    cs.add("注入3 拒检不落桶", lg4.consistent.is_empty() && lg4.next_seq == 1, "零副作用");

    // 注入 4：账满兜底 → LedgerFull（灌 33 个分叉核）
    let mut lg5 = RevLedger::new(&snap, GOLDEN_BSP_REV);
    let mut last = Ok(());
    for cpu in 1..36u32 {
        last = lg5.inspect(cpu, &stale_rev_probe(cpu, GOLDEN_BSP_REV.wrapping_sub(1)), &snap);
    }
    cs.add("注入4 账满拒续", last == Err(UcodeErr::LedgerFull), UcodeErr::LedgerFull.detail());
    cs.add("注入4 账容量=32", lg5.divergent.len() == LEDGER_CAP, "兜底不越界");

    // 只读红线：API 面静态证明——结构体无可变 pub 字段写通道之外，
    // 这里验证巡检后 BSP 基线与快照不被改写
    cs.add("只读红线-BSP基线不变", lg5.bsp_rev == GOLDEN_BSP_REV, "巡检零写");
    cs.add("只读红线-快照不变", lg5.snapshot == GOLDEN_SNAPSHOT, "引用冻结");

    // 修订史账：三轮采集，核2 出现回退（不可能事件）必被抓
    let mut hist = RevHistory::new();
    hist.record(1, GOLDEN_BSP_REV);
    hist.record(2, GOLDEN_BSP_REV);
    hist.record(1, GOLDEN_BSP_REV);
    hist.record(2, GOLDEN_BSP_REV - 0x08);
    cs.add("史账append-only", hist.append_only(), "seq 与行号一一对应");
    cs.add("史账核数=2", hist.cpus() == vec![1, 2], "首现序");
    cs.add("史账回退检出", hist.rollbacks() == vec![(2, GOLDEN_BSP_REV, GOLDEN_BSP_REV - 0x08)], "微码不可能降级");
    cs.add("史账零回退场景", {
        let mut h2 = RevHistory::new();
        h2.record(1, GOLDEN_BSP_REV);
        h2.record(2, GOLDEN_BSP_REV);
        h2.rollbacks().is_empty()
    }, "同构无回退");

    // 错误码闭集互异
    let codes = [
        UcodeErr::SnapshotMismatch, UcodeErr::ProbeFail,
        UcodeErr::CpuOutOfRange, UcodeErr::LedgerFull,
    ];
    cs.add("错误码四码互异",
        codes.iter().all(|a| codes.iter().all(|b| a == b || a.code() != b.code())),
        "闭集无重码");
    cs.add("错误码全部带建议", codes.iter().all(|c| !c.detail().is_empty()), "detail 非空");

    cs
}
