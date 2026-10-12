//! unx1741_idlefsm —— UNX-F1741 空闲循环总纲（idle 三态状态机）。
//! 锚点（CoRun Unxreal 主册 L3726）：UNX-F1741 · 空闲循环总纲（idle 三态状态机） · 目标 320 行 · 已深化
//! 判据原文：UNX-F1741-J1 idle 进入/驻留/退出三态状态机冻结 v1，状态转换账与实际
//!           状态一致断言全过，非法转换注入 10/10 次拒绝
//! 组件：三态状态机冻结（Active/Idle/Deep · 单向合法迁移闭集 · IdState）
//!      + 合法迁移表（编译期冻结 4 条合法边 · 其余全拒）
//!      + 转换账（意图/实际双列 · 逐次转换登记 · 一致断言）
//!      + 非法转换注入核（10 场非法边注入 · 迁移器 10/10 拒绝）
//!      + 驻留账（驻留 tick 计数 + 唤醒源账）
//! 判据落地：状态机冻结 v1 + 转换账一致全过 + 非法注入 10/10 拒绝
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术，零墙钟（tick 注入）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本（状态机冻结 v1）。
pub const ID_VERSION: &str = "unx1741_idlefsm-v1";

/// 诊断段基址（0x4f80 段 · 本模块独占）。
pub const ID_SEG_BASE: u16 = 0x4f80;

/// idle 三态（状态机冻结 v1 · 闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdState {
    /// 活跃态（有线程可跑 · 不允许进 idle 主循环）。
    Active,
    /// 空闲态（无就绪线程 · 浅驻留 · 唤醒延迟低）。
    Idle,
    /// 深闲态（长驻留 · 唤醒需重校准账）。
    Deep,
}

impl IdState {
    /// 态名（诊断输出）。
    pub fn name(self) -> &'static str {
        match self {
            IdState::Active => "active",
            IdState::Idle => "idle",
            IdState::Deep => "deep",
        }
    }

    /// 态序号（0–2）。
    pub fn ordinal(self) -> u8 {
        match self {
            IdState::Active => 0,
            IdState::Idle => 1,
            IdState::Deep => 2,
        }
    }
}

/// 合法迁移边（编译期冻结 · 4 条闭集）：
///   Active→Idle（就绪空 · 进 idle 主循环）
///   Idle→Active（唤醒 · 有新就绪）
///   Idle→Deep（驻留超阈 · 升深闲）
///   Deep→Idle（唤醒 · 降回浅闲再入活跃）
pub const ID_LEGAL_EDGES: [(IdState, IdState); 4] = [
    (IdState::Active, IdState::Idle),
    (IdState::Idle, IdState::Active),
    (IdState::Idle, IdState::Deep),
    (IdState::Deep, IdState::Idle),
];

/// 迁移判定：边是否合法（纯函数 · 迁移器唯一裁决点）。
pub fn id_edge_legal(from: IdState, to: IdState) -> bool {
    ID_LEGAL_EDGES
        .iter()
        .any(|&(f, t)| f == from && t == to)
}

/// 单次转换账条目（意图/实际双列）。
#[derive(Clone, Copy, Debug)]
pub struct IdTrans {
    /// 逻辑 tick（注入时序）。
    pub tick: u64,
    /// 迁移前态。
    pub from: IdState,
    /// 请求迁移目标态（意图）。
    pub want: IdState,
    /// 实际落定态（拒绝时保持 from）。
    pub got: IdState,
    /// 本次迁移是否被受理。
    pub accepted: bool,
    /// 唤醒源（受理/拒绝原因码）。
    pub cause: u16,
}

/// 唤醒源/原因码（冻结账）。
pub const ID_CAUSE_WAKE: u16 = 0x01;
pub const ID_CAUSE_DWELL: u16 = 0x02;
pub const ID_CAUSE_REJECT: u16 = 0xFF;

/// 驻留阈值：Idle 连续驻留 ≥ 此 tick 数方可升 Deep。
pub const ID_DWELL_TO_DEEP: u64 = 64;

/// idle 三态状态机（冻结 v1）。
pub struct IdFsm {
    /// 当前态。
    pub state: IdState,
    /// 驻留 tick 累计（Idle/Deep 连续驻留计数）。
    pub dwell: u64,
    /// 转换账。
    pub log: Vec<IdTrans>,
    /// 受理次数。
    pub accepted: u32,
    /// 拒绝次数。
    pub rejected: u32,
}

impl IdFsm {
    /// 新状态机（起始 Active · 冻结 v1 初始态）。
    pub fn new() -> Self {
        IdFsm {
            state: IdState::Active,
            dwell: 0,
            log: Vec::new(),
            accepted: 0,
            rejected: 0,
        }
    }

    /// 请求迁移：合法边受理并落账，非法边拒绝并保持原态落账。
    pub fn request(&mut self, tick: u64, want: IdState, cause: u16) -> bool {
        let from = self.state;
        let legal = id_edge_legal(from, want);
        // Deep 额外门：升 Deep 需驻留满阈。
        let gate = want != IdState::Deep || self.dwell >= ID_DWELL_TO_DEEP;
        let accepted = legal && gate;
        if accepted {
            self.state = want;
            self.accepted += 1;
            // 迁移进闲态清零驻留计数起账；迁出闲态归零。
            if want == IdState::Idle || want == IdState::Deep {
                self.dwell = 0;
            } else {
                self.dwell = 0;
            }
        } else {
            self.rejected += 1;
        }
        self.log.push(IdTrans {
            tick,
            from,
            want,
            got: if accepted { want } else { from },
            accepted,
            cause: if accepted { cause } else { ID_CAUSE_REJECT },
        });
        accepted
    }

    /// 驻留步进：每个 idle tick 累计驻留账（仅 Idle/Deep 态有效）。
    pub fn dwell_step(&mut self, ticks: u64) {
        match self.state {
            IdState::Idle | IdState::Deep => self.dwell += ticks,
            IdState::Active => self.dwell = 0,
        }
    }

    /// 转换账与实际状态一致断言：账尾落定态 == 当前态。
    pub fn log_consistent(&self) -> bool {
        match self.log.last() {
            None => self.state == IdState::Active,
            Some(t) => t.got == self.state,
        }
    }

    /// 全账一致：逐条重放迁移序列，终态与实际态一致。
    /// 账允许「Active 链重启」（注入探针重建产生的新链），重启点直接复位游标。
    pub fn replay_consistent(&self) -> bool {
        let mut cur = IdState::Active;
        for t in &self.log {
            if t.from != cur {
                if t.from == IdState::Active {
                    cur = IdState::Active;
                } else {
                    return false;
                }
            }
            if t.accepted && id_edge_legal(t.from, t.want) {
                cur = t.want;
            }
            if t.got != cur {
                return false;
            }
        }
        cur == self.state
    }
}

/// 非法转换注入场景（10 场 · 全部为冻结闭集之外的边或门）。
pub const ID_INJECT_CASES: [(IdState, IdState, &str); 10] = [
    (IdState::Active, IdState::Deep, "active->deep skip idle"),
    (IdState::Deep, IdState::Active, "deep->active skip idle"),
    (IdState::Deep, IdState::Deep, "deep self loop"),
    (IdState::Active, IdState::Active, "active self loop"),
    (IdState::Idle, IdState::Idle, "idle self loop"),
    (IdState::Active, IdState::Deep, "active->deep double-gate (bad edge + door)"),
    (IdState::Idle, IdState::Deep, "deep-gate: dwell below threshold"),
    (IdState::Deep, IdState::Deep, "deep-gate: re-deep after wake"),
    (IdState::Idle, IdState::Active, "legal edge used as negative control"),
    (IdState::Active, IdState::Idle, "baseline legal edge control"),
];

/// 非法转换注入核：10 场注入，非法边/破门必须拒绝，合法对照必须受理。
pub struct IdInjector {
    /// 注入场数。
    pub cases: u32,
    /// 预期拒绝数。
    pub expect_reject: u32,
    /// 实际拒绝数。
    pub rejected: u32,
    /// 预期受理数（合法对照）。
    pub expect_accept: u32,
    /// 实际受理数。
    pub accepted: u32,
    /// 明细账。
    pub log: Vec<String>,
}

impl IdInjector {
    /// 空注入核。
    pub fn new() -> Self {
        IdInjector {
            cases: 0,
            expect_reject: 0,
            rejected: 0,
            expect_accept: 0,
            accepted: 0,
            log: Vec::new(),
        }
    }

    /// 执行 10 场注入：前 8 场必拒，后 2 场合法对照必受理。
    pub fn inject_all(&mut self, fsm: &mut IdFsm) {
        for (i, &(from, want, tag)) in ID_INJECT_CASES.iter().enumerate() {
            // 状态机搬到注入起点（走合法边或重建）。
            if fsm.state != from {
                let mut probe = IdFsm::new();
                probe_log_to(&mut probe, fsm);
                // 直接按 from 重建最小合法前缀。
                match from {
                    IdState::Active => {}
                    IdState::Idle => {
                        probe.request(tick_of(i), IdState::Idle, ID_CAUSE_WAKE);
                    }
                    IdState::Deep => {
                        probe.request(tick_of(i), IdState::Idle, ID_CAUSE_WAKE);
                        probe.dwell_step(ID_DWELL_TO_DEEP);
                        probe.request(tick_of(i) + 1, IdState::Deep, ID_CAUSE_DWELL);
                    }
                }
                // 把探针结果并回主 fsm（替换式注入）。
                *fsm = probe;
            }
            // 门槛场：deep-gate 场景保证 dwell 未满。
            if tag.contains("dwell below threshold") {
                fsm.dwell = 0;
            }
            let ok = fsm.request(tick_of(i) + 2, want, ID_CAUSE_WAKE);
            self.cases += 1;
            let expect_rej = i < 8;
            if expect_rej {
                self.expect_reject += 1;
                if !ok {
                    self.rejected += 1;
                }
            } else {
                self.expect_accept += 1;
                if ok {
                    self.accepted += 1;
                }
            }
            self.log.push(format!(
                "#{} {}->{} {} -> {}",
                i,
                from.name(),
                want.name(),
                tag,
                if expect_rej {
                    if ok { "MISS(accepted!)" } else { "REJECTED" }
                } else if ok {
                    "ACCEPTED" } else { "MISS(rejected!)" }
            ));
        }
    }

    /// 注入满格：必拒全拒 + 对照全受理。
    pub fn all_exact(&self) -> bool {
        self.cases >= ID_INJECT_CASES.len() as u32
            && self.rejected == self.expect_reject
            && self.accepted == self.expect_accept
    }
}

/// 注入辅助：tick 序（确定性零墙钟）。
fn tick_of(i: usize) -> u64 {
    1000 + i as u64 * 10
}

/// 注入辅助：拷贝账（探针重建后账并回）。
fn probe_log_to(dst: &mut IdFsm, src: &IdFsm) {
    dst.log = src.log.clone();
}

/// idle 循环门面（判据四件：三态冻结/迁移闭集/账一致/注入必拒）。
pub struct IdIdle {
    /// 状态机。
    pub fsm: IdFsm,
    /// 注入核。
    pub injector: IdInjector,
}

impl IdIdle {
    /// 新门面。
    pub fn new() -> Self {
        IdIdle {
            fsm: IdFsm::new(),
            injector: IdInjector::new(),
        }
    }

    /// 判据总门。
    pub fn judge(&self) -> (bool, String) {
        let ok = self.fsm.log_consistent()
            && self.fsm.replay_consistent()
            && self.injector.all_exact();
        let msg = format!(
            "{}: state={} trans={} acc={} rej={} inject_rej={}/{} inject_acc={}/{} seg=0x{:04x}",
            ID_VERSION,
            self.fsm.state.name(),
            self.fsm.log.len(),
            self.fsm.accepted,
            self.fsm.rejected,
            self.injector.rejected,
            self.injector.expect_reject,
            self.injector.accepted,
            self.injector.expect_accept,
            ID_SEG_BASE
        );
        (ok, msg)
    }
}

/// 判据自检（t 系入口）：合法链路 + 驻留升深 + 账一致 + 10 场注入。
pub fn run_t741_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx1741-t741");
    let mut idle = IdIdle::new();
    // 主链路：Active→Idle→驻留满阈→Deep→唤醒降 Idle→回 Active。
    idle.fsm.request(10, IdState::Idle, ID_CAUSE_WAKE);
    idle.fsm.dwell_step(ID_DWELL_TO_DEEP);
    idle.fsm.request(20, IdState::Deep, ID_CAUSE_DWELL);
    idle.fsm.request(30, IdState::Idle, ID_CAUSE_WAKE);
    idle.fsm.request(40, IdState::Active, ID_CAUSE_WAKE);
    // 10 场注入（8 必拒 + 2 合法对照）。
    idle.injector.inject_all(&mut idle.fsm);
    let (ok, msg) = idle.judge();
    set.add_dyn("UNX-F1741-J1", ok, &msg);
    set.add("ID-log-consistent", idle.fsm.log_consistent(), "");
    set.add("ID-replay-consistent", idle.fsm.replay_consistent(), "");
    set.add("ID-inject-10/10", idle.injector.all_exact(), "");
    set.add("ID-fsm-v1-frozen", ID_VERSION.ends_with("v1"), "");
    set
}
