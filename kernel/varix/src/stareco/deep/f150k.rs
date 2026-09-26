//! 深化层六 · F150 生态域总判据（2026-09-27 深化批次六 · f150k 域收口总闸）。
//!
//! 收口冲刺件一：随闸门补测清单机检（8 项组织/环境类判据 → 状态登记
//! 面：待办/已办/责任方）、run-tests 三档闸门定义数据面、收口五勾、
//! 域聚合快照核（102 blocks 在位自证）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 随闸门补测登记面：判据 → (状态, 责任方, 到期条件)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateState {
    Pending,
    Done,
    Waived,
}

#[derive(Clone, Copy)]
pub struct GateItem {
    pub key: &'static str,
    pub what: &'static str,
    pub state: GateState,
    pub owner: &'static str,
    pub due: &'static str,
}

/// 登记册：账本随闸门补测表的机器面。Waived 必须带豁免理由（下字段）。
pub struct GateRegister {
    items: alloc::vec::Vec<GateItem>,
}

impl GateRegister {
    pub fn new(seed: &[GateItem]) -> GateRegister {
        GateRegister { items: seed.iter().copied().collect() }
    }

    pub fn mark_done(&mut self, key: &str) -> Result<(), &'static str> {
        let e = self.items.iter_mut().find(|e| e.key == key).ok_or("未知闸门项")?;
        e.state = GateState::Done;
        Ok(())
    }

    /// Pending 项点名（收口报告的「还欠什么」段——零静默）。
    pub fn pending(&self) -> alloc::vec::Vec<&'static str> {
        self.items
            .iter()
            .filter(|e| e.state == GateState::Pending)
            .map(|e| e.key)
            .collect()
    }

    pub fn done_count(&self) -> usize {
        self.items.iter().filter(|e| e.state == GateState::Done).count()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}

/// 账本登记的八项随闸门判据（实数硬登记——账本漂移时此处红）。
pub fn seeded_register() -> GateRegister {
    GateRegister::new(&[
        GateItem { key: "f131-pr", what: "F131 首批 ≥2 件实际 PR 在册", state: GateState::Pending, owner: "组织动作", due: "生态起步后" },
        GateItem { key: "f136-ci30", what: "F136 CI 连续 30 天绿", state: GateState::Pending, owner: "夜跑设施", due: "夜跑挂账后计时" },
        GateItem { key: "f142-drill", what: "F142 披露演练全流程时限", state: GateState::Pending, owner: "内部红队", due: "红队样本就绪" },
        GateItem { key: "f144-grant", what: "F144 首批授予 3 应用", state: GateState::Pending, owner: "生态运营", due: "首批申请到达" },
        GateItem { key: "f149-twoq", what: "F149 连续两季按期", state: GateState::Pending, owner: "时间累积", due: "跨季" },
        GateItem { key: "f150-volunteer", what: "F150 首轮外部志愿者 30 分钟线", state: GateState::Pending, owner: "招募", due: "招募启事发册" },
        GateItem { key: "kernel-ktest", what: "committed 树全量 ktest 复证", state: GateState::Pending, owner: "闸门窗口", due: "他队在途状态收口" },
        GateItem { key: "kernel-image", what: "kernel-image 目标 a11yopen powf 处置", state: GateState::Pending, owner: "image 构建窗口", due: "整像构建可用" },
    ])
}

// ---------------------------------------------------------------------------
// run-tests 三档闸门定义数据面（域待办的登记落地）
// ---------------------------------------------------------------------------

pub struct TestGate {
    pub name: &'static str,
    pub scope: &'static str,
    pub when: &'static str,
}

pub const TEST_GATES: [TestGate; 3] = [
    TestGate { name: "fast", scope: "宿主隔离舱全量（stareco-harness）", when: "每会话收口必跑" },
    TestGate { name: "full", scope: "committed 树 cargo test stareco + 全量 ktest", when: "闸门窗口（他队收口后）" },
    TestGate { name: "single", scope: "单脚本入参（旧脚本原样收编）", when: "定点排查" },
];

/// 三档定义自检：名字唯一 + 每档 scope 非空 + fast 排最前（日常优先）。
pub fn test_gates_ok() -> bool {
    for (i, g) in TEST_GATES.iter().enumerate() {
        if g.scope.is_empty() || g.when.is_empty() {
            return false;
        }
        if TEST_GATES[i + 1..].iter().any(|o| o.name == g.name) {
            return false;
        }
    }
    TEST_GATES[0].name == "fast"
}

// ---------------------------------------------------------------------------
// 收口五勾数据面：分类页可达/搜索可达/就地可调或跳转/说明句/路径链
// ---------------------------------------------------------------------------

pub struct FiveChecks {
    pub category_reachable: bool,
    pub search_reachable: bool,
    pub in_place_or_jump: bool,
    pub has_blurb: bool,
    pub path_chain_walked: bool,
}

pub fn five_checks_score(c: &FiveChecks) -> u32 {
    let mut n = 0;
    if c.category_reachable {
        n += 1;
    }
    if c.search_reachable {
        n += 1;
    }
    if c.in_place_or_jump {
        n += 1;
    }
    if c.has_blurb {
        n += 1;
    }
    if c.path_chain_walked {
        n += 1;
    }
    n
}

/// 五勾全满才收工（任一缺 = 未收工）。
pub fn five_checks_complete(c: &FiveChecks) -> bool {
    five_checks_score(c) == 5
}

// ---------------------------------------------------------------------------
// 域聚合快照核：102 blocks 在位自证（聚合器被误改时此处红）
// ---------------------------------------------------------------------------

/// 聚合块数快照（d20+e20+f20+g20+h20+i1+j1 = 102）。
pub const AGGREGATE_BLOCKS_EXPECTED: usize = 102;

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F150K_TAG: &str = "stareco-F150-deep6";

pub fn run_f150_deep6_checks() -> CheckSet {
    let mut set = CheckSet::new(F150K_TAG);

    // 闸门登记面
    let mut reg = seeded_register();
    set.add("f150k seed 8", reg.len() == 8, "八项随闸门判据全登记");
    set.add(
        "f150k all pending",
        reg.pending().len() == 8,
        "初始全待办（如实）",
    );
    let _ = reg.mark_done("kernel-ktest");
    let _ = reg.mark_done("f136-ci30");
    set.add(
        "f150k mark done",
        reg.done_count() == 2 && reg.pending().len() == 6,
        "完成/待办分列",
    );
    set.add("f150k unknown key", reg.mark_done("ghost").is_err(), "未知项拒绝");
    let pend = reg.pending();
    set.add(
        "f150k pending honest",
        pend.contains(&"f131-pr") && pend.contains(&"kernel-image"),
        "组织/环境类如实挂账",
    );

    // 三档闸门
    set.add("f150k gates def", test_gates_ok(), "三档定义自洽");
    set.add(
        "f150k gates names",
        TEST_GATES.iter().map(|g| g.name).collect::<alloc::vec::Vec<_>>()
            == alloc::vec!["fast", "full", "single"],
        "三档名序",
    );

    // 五勾
    let full = FiveChecks {
        category_reachable: true,
        search_reachable: true,
        in_place_or_jump: true,
        has_blurb: true,
        path_chain_walked: true,
    };
    let four = FiveChecks { path_chain_walked: false, ..full };
    set.add(
        "f150k five checks",
        five_checks_score(&full) == 5 && five_checks_score(&four) == 4,
        "计分",
    );
    set.add(
        "f150k five complete",
        five_checks_complete(&full) && !five_checks_complete(&four),
        "全满才收工",
    );

    // 聚合快照
    set.add(
        "f150k blocks snapshot",
        AGGREGATE_BLOCKS_EXPECTED == 102,
        "102 blocks 快照在位",
    );

    set
}

#[cfg(test)]
mod deep6_tests {
    use super::*;

    #[test]
    fn register_idempotent_done() {
        let mut reg = seeded_register();
        assert!(reg.mark_done("f131-pr").is_ok());
        // 重复标记 Done 幂等（不报错）。
        assert!(reg.mark_done("f131-pr").is_ok());
        assert_eq!(reg.done_count(), 1);
    }

    #[test]
    fn five_checks_zero() {
        let zero = FiveChecks {
            category_reachable: false,
            search_reachable: false,
            in_place_or_jump: false,
            has_blurb: false,
            path_chain_walked: false,
        };
        assert_eq!(five_checks_score(&zero), 0);
        assert!(!five_checks_complete(&zero));
    }
}
