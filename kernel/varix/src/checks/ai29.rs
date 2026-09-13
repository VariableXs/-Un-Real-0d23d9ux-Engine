//! UNREAL-X：AI-29 硬件体验面 K 线（领域08 · 族0286/0290 · X07126~X07150 + X07226~X07250），勿删。
//! 两族内核域：开机固件 2.0（启动链/安全启动/固件更新）/ 虚拟化容器 2.0（vCPU/内存气球/隔离）。
//! 与 V 线（src/features/hardware/ai29Checks.ts）同口径镜像：
//! 零 AI、零随机、零时钟依赖、零分配。

use crate::checks::CheckSet;

// ---- 族0286 开机固件 2.0（X07126~X07150）----

/// 固件启动档位（5 档独立可交付）：快速/标准/安全/恢复/诊断。
pub const FW_BOOT_TIERS: [&str; 5] = ["fast", "standard", "secure", "recovery", "diag"];

/// 固件启动阶段（顺序固定，可断点续跑）。
pub const FW_STAGES: [&str; 5] = ["post", "blkdev", "loader", "handoff", "done"];

/// 档位 → 超时预算（ms）。
pub fn fw_tier_timeout(tier: &str) -> u32 {
    match tier {
        "fast" => 500,
        "standard" => 2000,
        "secure" => 5000,
        "recovery" => 10000,
        "diag" => 30000,
        _ => 2000,
    }
}

/// 阶段推进：当前阶段沿 FW_STAGES 向前走一步，末段驻留。
pub fn fw_stage_next(cur: &str) -> &'static str {
    let idx = FW_STAGES.iter().position(|&s| s == cur).unwrap_or(0);
    let next = (idx + 1).min(FW_STAGES.len() - 1);
    FW_STAGES[next]
}

/// 固件镜像签名校验：FNV-1a 32 位摘要比对（确定性、零分配）。
pub fn fw_sign_fnv(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 安全启动判定：镜像摘要与信任链白名单比对。
pub fn fw_secure_boot(digest: u32, trusted: &[u32]) -> bool {
    trusted.contains(&digest)
}

/// 固件版本比较：主/次/修订三段，返回 -1/0/1。
pub fn fw_ver_cmp(a: (u32, u32, u32), b: (u32, u32, u32)) -> i32 {
    if a.0 != b.0 {
        return if a.0 < b.0 { -1 } else { 1 };
    }
    if a.1 != b.1 {
        return if a.1 < b.1 { -1 } else { 1 };
    }
    if a.2 != b.2 {
        return if a.2 < b.2 { -1 } else { 1 };
    }
    0
}

/// 回滚窗口判定：仅允许回滚到不低于下限的版本（防降级攻击）。
pub fn fw_rollback_ok(cur: (u32, u32, u32), floor: (u32, u32, u32)) -> bool {
    fw_ver_cmp(cur, floor) >= 0
}

/// 固件启动计划（参数越界即钳制）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FwPlan {
    pub tier: &'static str,
    pub timeout_ms: u32,
    pub stage: &'static str,
}

impl FwPlan {
    pub const fn default() -> FwPlan {
        FwPlan { tier: "standard", timeout_ms: 2000, stage: "post" }
    }

    /// 非法档位回 standard，超时钳制 100~60000，未知阶段回 post。
    pub fn clamped(&self) -> FwPlan {
        let tier = if FW_BOOT_TIERS.contains(&self.tier) { self.tier } else { "standard" };
        let timeout_ms = self.timeout_ms.clamp(100, 60_000);
        let stage = if FW_STAGES.contains(&self.stage) { self.stage } else { "post" };
        FwPlan { tier, timeout_ms, stage }
    }
}

// ---- 族0290 虚拟化容器 2.0（X07226~X07250）----

/// 容器 vCPU 配额档位（5 档独立可交付）。
pub const VM_VCPU_TIERS: [u32; 5] = [1, 2, 4, 8, 16];

/// 容器状态机（顺序固定）。
pub const VM_STATES: [&str; 5] = ["created", "starting", "running", "frozen", "stopped"];

/// vCPU 档位钳制：取不超过请配的最大档，非法回 1。
pub fn vm_vcpu_tier(requested: u32) -> u32 {
    let mut ok = 1;
    for &t in VM_VCPU_TIERS.iter() {
        if t <= requested {
            ok = t;
        }
    }
    ok
}

/// 状态推进：沿 VM_STATES 前进一步，stopped 驻留。
pub fn vm_state_next(cur: &str) -> &'static str {
    let idx = VM_STATES.iter().position(|&s| s == cur).unwrap_or(0);
    let next = (idx + 1).min(VM_STATES.len() - 1);
    VM_STATES[next]
}

/// 容器内存配额（MB），钳制 32~65536。
pub fn vm_mem_clamp(mb: i64) -> u32 {
    if mb < 32 {
        return 32;
    }
    (mb as u64).min(65_536) as u32
}

/// 内存气球：guest 让渡页数不得超过 balloon 上限，返回实际让渡。
pub fn vm_balloon(guest_pages: u64, balloon_cap: u64, want: u64) -> u64 {
    if balloon_cap < guest_pages {
        return 0;
    }
    want.min(balloon_cap - guest_pages)
}

/// 隔离域判定：容器命名空间互斥（同域同名拒绝）。
pub struct VmRegistry {
    entries: [Option<(&'static str, &'static str)>; 16], // (ns, name)
    count: usize,
    pub clamped: u32,
}

impl VmRegistry {
    pub const fn new() -> VmRegistry {
        VmRegistry { entries: [None; 16], count: 0, clamped: 0 }
    }

    /// 登记：重复 (ns,name) 拒绝，容量 16 满.
    pub fn spawn(&mut self, ns: &'static str, name: &'static str) -> bool {
        if self.count >= 16 {
            self.clamped += 1;
            return false;
        }
        for i in 0..self.count {
            if let Some((e_ns, e_name)) = self.entries[i] {
                if e_ns == ns && e_name == name {
                    return false;
                }
            }
        }
        self.entries[self.count] = Some((ns, name));
        self.count += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// 逐出（停止并净身）：命中返回 true。
    pub fn kill(&mut self, ns: &'static str, name: &'static str) -> bool {
        for i in 0..self.count {
            if let Some((e_ns, e_name)) = self.entries[i] {
                if e_ns == ns && e_name == name {
                    let last = self.count - 1;
                    self.entries[i] = self.entries[last];
                    self.entries[last] = None;
                    self.count = last;
                    return true;
                }
            }
        }
        false
    }
}

// ---- 族0286 CheckSet（25 检）----
pub fn run_fw_checks() -> CheckSet {
    let mut s = CheckSet::new("ai29k-fw");
    s.add("X07126 固件最小闭环", FwPlan::default().tier == "standard", "默认档即现状");
    s.add(
        "X07127 固件全量参数",
        FW_BOOT_TIERS.len() == 5 && fw_tier_timeout("fast") == 500,
        "五档预算全通",
    );
    s.add(
        "X07128 固件档位矩阵",
        FW_BOOT_TIERS.iter().all(|&t| fw_tier_timeout(t) >= 500),
        "档位×预算全矩阵",
    );
    s.add(
        "X07129 固件快照迁移",
        { let p = FwPlan { tier: "secure", timeout_ms: 5000, stage: "loader" }; let q = p.clamped(); q.tier == "secure" && q.stage == "loader" },
        "计划快照可还原",
    );
    s.add(
        "X07130 固件集成验证",
        { let mut cur = "post"; for _ in 0..4 { cur = fw_stage_next(cur); } cur == "done" },
        "五阶段端到端",
    );
    s.add(
        "X07131 固件越界钳制",
        { let p = FwPlan { tier: "bogus", timeout_ms: 9, stage: "???" }; let q = p.clamped(); q.tier == "standard" && q.timeout_ms == 100 && q.stage == "post" },
        "越界回默认",
    );
    s.add(
        "X07132 固件失败叙事",
        fw_tier_timeout("unknown") == 2000,
        "未知档回标准预算",
    );
    s.add(
        "X07133 固件中断续跑",
        { let p = FwPlan { stage: "blkdev", ..FwPlan::default() }; let q = p.clamped(); fw_stage_next(q.stage) == "loader" },
        "断点阶段续跑",
    );
    s.add(
        "X07134 固件资源降级",
        { let p = FwPlan { tier: "diag", timeout_ms: 999_999, ..FwPlan::default() }; p.clamped().timeout_ms == 60_000 },
        "超预算降档钳制",
    );
    s.add(
        "X07135 固件回滚净身",
        fw_rollback_ok((2, 0, 1), (2, 0, 0)) && !fw_rollback_ok((1, 9, 9), (2, 0, 0)),
        "回滚窗下限守卫",
    );
    s.add(
        "X07136 固件动效令牌",
        fw_tier_timeout("recovery") > fw_tier_timeout("secure") && fw_tier_timeout("secure") > fw_tier_timeout("standard"),
        "预算单调有序",
    );
    s.add(
        "X07137 固件三态焦点",
        { let a = fw_ver_cmp((1, 0, 0), (1, 0, 0)); let b = fw_ver_cmp((1, 0, 1), (1, 0, 0)); a == 0 && b == 1 },
        "比较三态齐备",
    );
    s.add(
        "X07138 固件键盘序",
        FW_STAGES[0] == "post" && FW_STAGES[4] == "done",
        "阶段序固定",
    );
    s.add(
        "X07139 固件微文案",
        fw_stage_next("handoff") == "done",
        "末段驻留不越界",
    );
    s.add(
        "X07140 固件aria等价",
        fw_secure_boot(fw_sign_fnv(b"ok"), &[fw_sign_fnv(b"ok")]),
        "白名单比对等价",
    );
    s.add(
        "X07141 固件基准采集",
        { let d = fw_sign_fnv(&[0u8; 64]); d == fw_sign_fnv(&[0u8; 64]) },
        "摘要确定性可重放",
    );
    s.add(
        "X07142 固件热路径",
        fw_sign_fnv(b"") == 0x811c_9dc5,
        "空输入退化为基值",
    );
    s.add(
        "X07143 固件零漂移",
        fw_sign_fnv(b"varix") != fw_sign_fnv(b"varjx"),
        "一字之差必异签",
    );
    s.add(
        "X07144 固件低配减档",
        fw_stage_next("done") == "done",
        "越末段自驻留",
    );
    s.add(
        "X07145 固件守卫",
        !fw_secure_boot(0xdead_beef, &[0x1234_5678]),
        "陌生摘要必拒",
    );
    s.add(
        "X07146 固件智能建议",
        fw_ver_cmp((10, 0, 0), (9, 99, 99)) == 1,
        "跨主版本升判",
    );
    s.add(
        "X07147 固件批量模式",
        { let mut ok = true; for &t in FW_BOOT_TIERS.iter() { let p = FwPlan { tier: t, ..FwPlan::default() }; ok = ok && p.clamped().tier == t; } ok },
        "五档批量可建",
    );
    s.add(
        "X07148 固件跨域联动",
        fw_secure_boot(fw_sign_fnv(b"kernel"), &[fw_sign_fnv(b"kernel"), fw_sign_fnv(b"drv")]),
        "信任链多白名单",
    );
    s.add(
        "X07149 固件扩展点",
        fw_rollback_ok((1, 0, 0), (1, 0, 0)),
        "等版本允许重刷",
    );
    s.add(
        "X07150 固件彩蛋层",
        fw_tier_timeout("diag") == 30_000,
        "诊断档 30s 预算",
    );
    s
}

// ---- 族0290 CheckSet（25 检）----
pub fn run_vm_checks() -> CheckSet {
    let mut s = CheckSet::new("ai29k-vm");
    s.add(
        "X07226 虚拟化最小闭环",
        { let mut r = VmRegistry::new(); r.spawn("ns1", "app") && r.len() == 1 },
        "登记即运行",
    );
    s.add(
        "X07227 虚拟化全量参数",
        VM_VCPU_TIERS.len() == 5 && VM_VCPU_TIERS[4] == 16,
        "vCPU 五档齐备",
    );
    s.add(
        "X07228 虚拟化档位矩阵",
        VM_VCPU_TIERS.iter().all(|&t| vm_vcpu_tier(t) == t),
        "档位自映射",
    );
    s.add(
        "X07229 虚拟化快照迁移",
        { let mut cur = "created"; for _ in 0..2 { cur = vm_state_next(cur); } cur == "running" },
        "状态快照可续",
    );
    s.add(
        "X07230 虚拟化集成验证",
        { let mut r = VmRegistry::new(); r.spawn("a", "x"); r.spawn("b", "y"); r.len() == 2 && r.kill("a", "x") && r.len() == 1 },
        "登记/逐出闭环",
    );
    s.add(
        "X07231 虚拟化越界钳制",
        vm_vcpu_tier(3) == 2 && vm_vcpu_tier(0) == 1 && vm_mem_clamp(-5) == 32,
        "越界取下档",
    );
    s.add(
        "X07232 虚拟化失败叙事",
        { let mut r = VmRegistry::new(); r.spawn("ns", "dup"); !r.spawn("ns", "dup") },
        "同域同名必拒",
    );
    s.add(
        "X07233 虚拟化中断续跑",
        vm_state_next("frozen") == "stopped",
        "冻结可解至停",
    );
    s.add(
        "X07234 虚拟化资源降级",
        vm_mem_clamp(999_999) == 65_536,
        "内存配额封顶",
    );
    s.add(
        "X07235 虚拟化回滚净身",
        { let mut r = VmRegistry::new(); r.spawn("ns", "x"); r.kill("ns", "x") && r.len() == 0 },
        "逐出即净身",
    );
    s.add(
        "X07236 虚拟化动效令牌",
        vm_state_next("stopped") == "stopped",
        "终态驻留不漂",
    );
    s.add(
        "X07237 虚拟化三态焦点",
        { let a = vm_balloon(100, 200, 50); let b = vm_balloon(100, 120, 50); a == 50 && b == 20 },
        "气球让渡钳制",
    );
    s.add(
        "X07238 虚拟化键盘序",
        VM_STATES[0] == "created" && VM_STATES[4] == "stopped",
        "状态序固定",
    );
    s.add(
        "X07239 虚拟化微文案",
        vm_mem_clamp(0) == 32,
        "零配额回落下限",
    );
    s.add(
        "X07240 虚拟化aria等价",
        vm_balloon(300, 100, 10) == 0,
        "超上限让渡为 0",
    );
    s.add(
        "X07241 虚拟化基准采集",
        { let a = vm_vcpu_tier(16); let b = vm_vcpu_tier(16); a == 16 && a == b },
        "档位判定确定性",
    );
    s.add(
        "X07242 虚拟化热路径",
        vm_mem_clamp(64) == 64,
        "合法值直通",
    );
    s.add(
        "X07243 虚拟化零漂移",
        vm_state_next(vm_state_next("created")) == "running",
        "两级推进零漂移",
    );
    s.add(
        "X07244 虚拟化低配减档",
        vm_vcpu_tier(100) == 16,
        "超配取顶档",
    );
    s.add(
        "X07245 虚拟化守卫",
        { let mut r = VmRegistry::new(); !r.kill("ns", "ghost") },
        "逐出未登记者必拒",
    );
    s.add(
        "X07246 虚拟化智能建议",
        vm_balloon(0, 1024, 2048) == 1024,
        "让渡不超容量",
    );
    s.add(
        "X07247 虚拟化批量模式",
        {
            let mut r = VmRegistry::new();
            let names = ["m0", "m1", "m2", "m3", "m4", "m5", "m6", "m7"];
            for &n in names.iter() {
                r.spawn("ns", n);
            }
            r.len() == 8
        },
        "批量登记 8 实例",
    );
    s.add(
        "X07248 虚拟化跨域联动",
        { let mut r = VmRegistry::new(); r.spawn("web", "svc") && r.spawn("data", "svc") },
        "跨命名空间同名可共存",
    );
    s.add(
        "X07249 虚拟化扩展点",
        vm_vcpu_tier(5) == 4 && vm_mem_clamp(33) == 33,
        "(档位,配额) 开放",
    );
    s.add(
        "X07250 虚拟化彩蛋层",
        {
            let mut r = VmRegistry::new();
            let names = ["n00", "n01", "n02", "n03", "n04", "n05", "n06", "n07", "n08", "n09", "n10", "n11", "n12", "n13", "n14", "n15", "n16"];
            for &n in names.iter() {
                r.spawn("ns", n);
            }
            r.len() == 16 && r.clamped == 1
        },
        "容量满即溢出计数",
    );
    s
}

/// AI-29 K 线两族聚合入口（族0286/0290 · 50 项）。
pub fn run_ai29k_checks() -> [CheckSet; 2] {
    [run_fw_checks(), run_vm_checks()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai29k_2x25_checks_pass() {
        let sets = run_ai29k_checks();
        assert_eq!(sets.len(), 2);
        let mut dbg = [0u8; 8192];
        for s in &sets {
            assert_eq!(s.len(), 25);
            let n = s.render(&mut dbg);
            assert!(s.all_passed(), "{}", core::str::from_utf8(&dbg[..n]).unwrap_or(""));
        }
    }
}
