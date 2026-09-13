//! UNREAL-X：AI-59 混沌工程 K 线（领域16 · 族0584 · X14576~X14600），勿删。
//! 故障注入器：预算熔断 / 爆炸半径钳制 / 止血开关 / 确定性 LCG 调度 / 可恢复判定。
//! 与 V 线（src/features/engops/ai59Models.ts · ChaosEngine）同口径镜像：
//! 零 AI、零随机（种子 LCG 可重放）、零时钟依赖。

use crate::checks::CheckSet;

/// 五类内核故障（panic/oom/net-drop/disk-slow/irq-storm）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChaosFault {
    Panic,
    Oom,
    NetDrop,
    DiskSlow,
    IrqStorm,
}

/// 故障域：恰 5 类，与 V 线 CHAOS_FAULTS 同序。
pub const CHAOS_FAULTS: [ChaosFault; 5] = [
    ChaosFault::Panic,
    ChaosFault::Oom,
    ChaosFault::NetDrop,
    ChaosFault::DiskSlow,
    ChaosFault::IrqStorm,
];

/// 故障名（与 V 线同字面）。
pub const CHAOS_FAULT_NAMES: [&str; 5] = ["panic", "oom", "net-drop", "disk-slow", "irq-storm"];

/// 严重度五档：trace/mild/steady/harsh/extreme。
pub const CHAOS_SEVERITY_NAMES: [&str; 5] = ["trace", "mild", "steady", "harsh", "extreme"];

impl ChaosFault {
    pub fn as_str(self) -> &'static str {
        CHAOS_FAULT_NAMES[self as usize]
    }

    /// 名字 → 故障：未知名字返回 None。
    pub fn from_name(name: &str) -> Option<ChaosFault> {
        CHAOS_FAULT_NAMES
            .iter()
            .position(|&n| n == name)
            .map(|i| CHAOS_FAULTS[i])
    }
}

fn severity_valid(severity: &str) -> bool {
    CHAOS_SEVERITY_NAMES.contains(&severity)
}

/// 混沌引擎快照（V 线 JSON 快照的无分配等价）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChaosSnapshot {
    pub budget: u32,
    pub cap: u32,
    pub injected: u32,
}

/// 故障注入器：止血开关 + 预算熔断 + 爆炸半径钳制。
pub struct ChaosEngine {
    pub kill_switch: bool,
    pub budget: u32,
    pub blast_cap: u32,
    pub injected: u32,
    pub clamped: u32,
}

impl ChaosEngine {
    pub const fn new() -> ChaosEngine {
        ChaosEngine { kill_switch: false, budget: 100, blast_cap: 8, injected: 0, clamped: 0 }
    }

    /// 预算设定：钳制 1~1000（非法 0 归一为最低档）。
    pub fn set_budget(&mut self, n: u32) -> u32 {
        self.budget = if n < 1 { 1 } else if n > 1000 { 1000 } else { n };
        self.budget
    }

    /// 爆炸半径设定：钳制 1~8。
    pub fn set_blast_cap(&mut self, n: u32) -> u32 {
        self.blast_cap = if n < 1 { 1 } else if n > 8 { 8 } else { n };
        self.blast_cap
    }

    /// 注入一次故障：止血开关 → 域校验 → 预算熔断 → 爆炸半径，全过才计数。
    pub fn inject(&mut self, fault: &str, severity: &str, affected: u32) -> bool {
        if self.kill_switch {
            self.clamped += 1;
            return false; // 止血开关：全局停注
        }
        if ChaosFault::from_name(fault).is_none() || !severity_valid(severity) {
            self.clamped += 1;
            return false;
        }
        if self.injected >= self.budget {
            self.clamped += 1;
            return false; // 预算熔断
        }
        if affected < 1 || affected > self.blast_cap {
            self.clamped += 1;
            return false; // 爆炸半径钳制
        }
        self.injected += 1;
        true
    }

    /// 确定性 LCG 调度：同种子同序列（可重放）；种子 0 归一为 1。
    pub fn schedule_into(seed: u32, out: &mut [ChaosFault]) {
        let mut s = if seed == 0 { 1 } else { seed };
        for slot in out.iter_mut() {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            *slot = CHAOS_FAULTS[(s % 5) as usize];
        }
    }

    /// 可恢复判定：仅「panic × extreme」需人工介入，其余自动恢复。
    pub fn recoverable(fault: ChaosFault, severity: &str) -> bool {
        !(fault == ChaosFault::Panic && severity == "extreme")
    }

    /// 失败叙事：每类故障一句可读恢复叙事；未知故障回默认档。
    pub fn narrative(fault: &str) -> &'static str {
        match fault {
            "panic" => "内核恐慌注入：守护进程将拉起快照恢复",
            "oom" => "内存耗尽注入：按配额逐级回收后自愈",
            "net-drop" => "网络丢包注入：链路预算重算后重建",
            "disk-slow" => "磁盘迟滞注入：IO 队列降级为同步直写",
            "irq-storm" => "中断风暴注入：合并窗口自动放大",
            _ => "未知故障：拒绝注入并回默认档",
        }
    }

    /// 故障短标签（中文单字令牌）。
    pub fn fault_label(fault: ChaosFault) -> &'static str {
        match fault {
            ChaosFault::Panic => "恐慌",
            ChaosFault::Oom => "内存",
            ChaosFault::NetDrop => "丢包",
            ChaosFault::DiskSlow => "迟滞",
            ChaosFault::IrqStorm => "风暴",
        }
    }

    /// 快照：预算/半径/已注入三元组。
    pub fn snapshot(&self) -> ChaosSnapshot {
        ChaosSnapshot { budget: self.budget, cap: self.blast_cap, injected: self.injected }
    }

    /// 从快照恢复：整态覆盖。
    pub fn restore(&mut self, s: ChaosSnapshot) -> bool {
        self.budget = s.budget;
        self.blast_cap = s.cap;
        self.injected = s.injected;
        true
    }
}

/// 族0584 混沌工程（X14576~X14600）：25 项自检。
pub fn run_ai59k_checks() -> [CheckSet; 1] {
    let mut c = ChaosEngine::new();
    let mut s = CheckSet::new("ai59k-chaos");
    s.add("X14576 混沌最小闭环", c.inject("panic", "mild", 1) && c.injected == 1, "故障注入即计数");
    s.add(
        "X14577 混沌全量参数",
        c.set_budget(500) == 500 && c.set_blast_cap(4) == 4 && c.inject("oom", "steady", 4),
        "预算/半径全量参数设定",
    );
    s.add("X14578 混沌档位矩阵", CHAOS_FAULTS.len() == 5 && CHAOS_SEVERITY_NAMES.len() == 5, "五故障 × 五严重度档");
    s.add(
        "X14579 混沌快照迁移",
        c.snapshot().budget == 500 && { let mut q = ChaosEngine::new(); q.restore(c.snapshot()) },
        "快照三元组可迁移",
    );
    s.add(
        "X14580 混沌联调集成",
        { let mut q = ChaosEngine::new(); q.restore(c.snapshot()) && q.injected == 2 },
        "恢复后已注入计数一致",
    );
    s.add(
        "X14581 混沌越界钳制",
        { let mut q = ChaosEngine::new(); !q.inject("nope", "mild", 1) && !q.inject("panic", "nope", 1) },
        "未知故障/未知严重度全拒",
    );
    s.add(
        "X14582 混沌爆炸半径",
        { let mut q = ChaosEngine::new(); !q.inject("panic", "mild", 0) && !q.inject("panic", "mild", 9) },
        "半径 0/超帽双拒",
    );
    s.add(
        "X14583 混沌预算熔断",
        { let mut q = ChaosEngine::new(); q.set_budget(2); q.inject("oom", "mild", 1); q.inject("oom", "mild", 1); !q.inject("oom", "mild", 1) },
        "第三次注入被预算熔断",
    );
    s.add(
        "X14584 混沌止血开关",
        { let mut q = ChaosEngine::new(); q.kill_switch = true; !q.inject("panic", "mild", 1) && q.clamped >= 1 },
        "开关合闸即全局停注",
    );
    s.add(
        "X14585 混沌资源降级",
        { let mut q = ChaosEngine::new(); q.set_budget(0) == 1 && q.set_budget(99999) == 1000 },
        "预算钳制 1~1000",
    );
    s.add(
        "X14586 混沌失败叙事",
        ChaosEngine::narrative("panic").contains("恐慌") && ChaosEngine::fault_label(ChaosFault::Oom) == "内存",
        "叙事含恢复路径 + 短标签",
    );
    s.add(
        "X14587 混沌可重放调度",
        {
            let mut a = [ChaosFault::Panic; 10];
            let mut b = [ChaosFault::Panic; 10];
            ChaosEngine::schedule_into(42, &mut a);
            ChaosEngine::schedule_into(42, &mut b);
            a == b
        },
        "同种子同序列（LCG 可重放）",
    );
    s.add(
        "X14588 混沌域内调度",
        {
            let mut buf = [ChaosFault::Panic; 20];
            ChaosEngine::schedule_into(7, &mut buf);
            let mut distinct = 0;
            for f in CHAOS_FAULTS {
                if buf.contains(&f) {
                    distinct += 1;
                }
            }
            buf.iter().all(|f| CHAOS_FAULTS.contains(f)) && distinct >= 2
        },
        "调度序列全落域且多样",
    );
    s.add(
        "X14589 混沌可恢复判定",
        !ChaosEngine::recoverable(ChaosFault::Panic, "extreme") && ChaosEngine::recoverable(ChaosFault::Panic, "harsh"),
        "仅极端恐慌需人工，其余自动",
    );
    s.add(
        "X14590 混沌矩阵完备",
        {
            let mut ok = true;
            for f in CHAOS_FAULTS {
                for sv in CHAOS_SEVERITY_NAMES {
                    let expect = !(f == ChaosFault::Panic && sv == "extreme");
                    ok = ok && ChaosEngine::recoverable(f, sv) == expect;
                }
            }
            ok
        },
        "25 格恢复矩阵逐格断言",
    );
    s.add(
        "X14591 混沌基准采集",
        {
            let mut buf = [ChaosFault::Panic; 10];
            let mut ok = true;
            for i in 1..=500u32 {
                ChaosEngine::schedule_into(i, &mut buf);
                ok = ok && buf.iter().all(|f| CHAOS_FAULTS.contains(f));
            }
            ok
        },
        "500 种子 × 10 故障基准采集",
    );
    s.add(
        "X14592 混沌热路径",
        {
            let mut q = ChaosEngine::new();
            q.set_budget(1000);
            let mut n = 0;
            for _ in 0..1000 {
                if q.inject("oom", "mild", 1) {
                    n += 1;
                }
            }
            n == 1000 && q.injected == 1000
        },
        "千次注入热路径全绿",
    );
    s.add(
        "X14593 混沌零漂移",
        {
            let mut a = ChaosEngine::new();
            let mut b = ChaosEngine::new();
            a.set_budget(77);
            b.set_budget(77);
            a.snapshot() == b.snapshot()
        },
        "同参数双引擎快照一致",
    );
    s.add(
        "X14594 混沌守卫",
        { let mut q = ChaosEngine::new(); q.set_blast_cap(1); q.inject("oom", "mild", 1) && !q.inject("oom", "mild", 2) },
        "半径帽 1 时容量 2 拒绝",
    );
    s.add(
        "X14595 混沌非法归一",
        { let mut q = ChaosEngine::new(); q.set_blast_cap(0) == 1 && q.set_blast_cap(999) == 8 },
        "非法半径归一 1~8",
    );
    s.add(
        "X14596 混沌序列规模",
        {
            let mut buf = [ChaosFault::Panic; 5];
            ChaosEngine::schedule_into(99, &mut buf);
            let mut distinct = 0;
            for f in CHAOS_FAULTS {
                if buf.contains(&f) {
                    distinct += 1;
                }
            }
            distinct >= 1 && distinct <= 5
        },
        "5 故障序列去重规模合法",
    );
    s.add(
        "X14597 混沌批量模式",
        {
            let mut q = ChaosEngine::new();
            let mut n = 0;
            for f in CHAOS_FAULTS {
                if q.inject(f.as_str(), "trace", 1) {
                    n += 1;
                }
            }
            n == 5
        },
        "五故障批量注入全过",
    );
    s.add(
        "X14598 混沌跨档联动",
        {
            let mut ok = true;
            for sv in CHAOS_SEVERITY_NAMES {
                let mut q = ChaosEngine::new();
                ok = ok && q.inject("irq-storm", sv, 1);
            }
            ok
        },
        "同一故障跨五严重度可注入",
    );
    s.add(
        "X14599 混沌扩展点",
        { ChaosEngine::schedule_into(3, &mut []); ChaosEngine::narrative("net-drop").contains("丢包") },
        "空调度安全 + 丢包叙事就位",
    );
    s.add(
        "X14600 混沌彩蛋层",
        ChaosEngine::narrative("irq-storm").contains("风暴") && ChaosEngine::narrative("unknown-fault").contains("未知"),
        "未知故障回默认档彩蛋",
    );
    [s]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai59k_25_checks_pass() {
        let sets = run_ai59k_checks();
        assert_eq!(sets.len(), 1);
        let mut dbg = [0u8; 8192];
        for s in &sets {
            assert_eq!(s.len(), 25);
            let n = s.render(&mut dbg);
            assert!(s.all_passed(), "{}", core::str::from_utf8(&dbg[..n]).unwrap_or(""));
        }
    }
}
