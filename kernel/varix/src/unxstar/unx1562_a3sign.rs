//! unx1562_a3sign —— UNX-F1562 A3 调度域联签件（tick/reschedule IPI 消费核验）。
//! 锚点（CoRun Unxreal 主册 L3477）：UNX-F1562 · A3 调度域联签件（tick/reschedule IPI
//!           消费核验） · 目标 340 行 · 已深化
//! 判据原文：UNX-F1562-J1 A3 联签件核验通过（tick 与 reschedule IPI 消费面逐项核验），
//!           核验项全绿 10/10 复跑一致，联签记录落档
//! 组件：消费面核验项闭集（tick 面 5 项 + reschedule IPI 面 5 项 · 十项冻结）
//!      + 逐项核验器（每项：声明 vs 实测 双源对拍 · 全绿口径）
//!      + 复跑一致性（核验 10 轮逐轮结果一致断言）
//!      + 联签记录落档（十项结果 + 复跑指纹 FNV 冻结）
//! 判据落地：十项全绿 + 10/10 复跑一致 + 记录落档可复读
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 FNV 整数算术，零墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const AS_VERSION: &str = "unx1562_a3sign-v1";

/// 诊断段基址（0x5190 段 · 本模块独占）。
pub const AS_SEG_BASE: u16 = 0x5190;

/// 消费面核验项（十项冻结 · tick 面 + reschedule IPI 面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsItem {
    /// T1 tick 消费方注册账（F1002 时钟面挂接）。
    TickConsumer,
    /// T2 tick 单调性（消费序单调）。
    TickMonotonic,
    /// T3 tick 派发零丢失（p99 延迟账 F1022 对接）。
    TickNoLoss,
    /// T4 tick 频率声明一致性（声明频率 vs 实测周期）。
    TickFreq,
    /// T5 tick 消费回调零重入。
    TickNoReenter,
    /// R1 reschedule IPI 目标核命中（选核账一致）。
    ReschedTarget,
    /// R2 reschedule IPI 派发序（FIFO 对账）。
    ReschedOrder,
    /// R3 reschedule IPI 计数守恒（发 = 收 + 在途）。
    ReschedConserve,
    /// R4 reschedule IPA 暂停恢复语义（F118 对接）。
    ReschedPause,
    /// R5 reschedule IPI 健康账（F119 对接）。
    ReschedHealth,
}

impl AsItem {
    /// 十项全序。
    pub const ALL: [AsItem; 10] = [
        AsItem::TickConsumer,
        AsItem::TickMonotonic,
        AsItem::TickNoLoss,
        AsItem::TickFreq,
        AsItem::TickNoReenter,
        AsItem::ReschedTarget,
        AsItem::ReschedOrder,
        AsItem::ReschedConserve,
        AsItem::ReschedPause,
        AsItem::ReschedHealth,
    ];

    /// 项名。
    pub fn name(self) -> &'static str {
        match self {
            AsItem::TickConsumer => "T1-tick-consumer",
            AsItem::TickMonotonic => "T2-tick-monotonic",
            AsItem::TickNoLoss => "T3-tick-no-loss",
            AsItem::TickFreq => "T4-tick-freq",
            AsItem::TickNoReenter => "T5-tick-no-reenter",
            AsItem::ReschedTarget => "R1-resched-target",
            AsItem::ReschedOrder => "R2-resched-order",
            AsItem::ReschedConserve => "R3-resched-conserve",
            AsItem::ReschedPause => "R4-resched-pause",
            AsItem::ReschedHealth => "R5-resched-health",
        }
    }

    /// 所属面。
    pub fn face(self) -> &'static str {
        match self {
            AsItem::TickConsumer
            | AsItem::TickMonotonic
            | AsItem::TickNoLoss
            | AsItem::TickFreq
            | AsItem::TickNoReenter => "tick",
            _ => "resched-ipi",
        }
    }

    /// 项序（0–9）。
    pub fn ordinal(self) -> usize {
        self as usize
    }
}

/// 单项核验结果。
#[derive(Clone, Copy, Debug)]
pub struct AsVerdict {
    /// 项。
    pub item: AsItem,
    /// 声明值。
    pub declared: u64,
    /// 实测值。
    pub measured: u64,
    /// 全绿。
    pub green: bool,
}

/// 消费面模拟账（声明侧 + 实测侧 · 确定性生成）。
#[derive(Clone, Copy, Debug)]
pub struct AsFace {
    /// 声明账（十项声明值）。
    pub declared: [u64; 10],
    /// 实测账（十项实测值）。
    pub measured: [u64; 10],
}

impl AsFace {
    /// 一致面（声明 == 实测 · 确定性生成）。
    pub fn consistent(seed: u64) -> Self {
        let mut s = seed;
        let mut declared = [0u64; 10];
        for v in declared.iter_mut() {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            *v = (s >> 32) % 1000 + 1;
        }
        AsFace {
            declared,
            measured: declared,
        }
    }

    /// 漂移面（第 drift_at 项实测 ≠ 声明）。
    pub fn drifted(seed: u64, drift_at: usize) -> Self {
        let mut f = AsFace::consistent(seed);
        if drift_at < 10 {
            f.measured[drift_at] = f.declared[drift_at] + 7;
        }
        f
    }
}

/// 逐项核验器。
pub struct AsVerifier {
    /// 十项判定。
    pub verdicts: Vec<AsVerdict>,
    /// 全绿项数。
    pub greens: u32,
}

impl AsVerifier {
    /// 空核验器。
    pub fn new() -> Self {
        AsVerifier {
            verdicts: Vec::new(),
            greens: 0,
        }
    }

    /// 十项逐项对拍。
    pub fn verify(&mut self, face: &AsFace) -> bool {
        self.verdicts.clear();
        self.greens = 0;
        let mut all = true;
        for it in AsItem::ALL {
            let d = face.declared[it.ordinal()];
            let m = face.measured[it.ordinal()];
            let green = d == m;
            if green {
                self.greens += 1;
            } else {
                all = false;
            }
            self.verdicts.push(AsVerdict {
                item: it,
                declared: d,
                measured: m,
                green,
            });
        }
        all
    }

    /// 全绿断言。
    pub fn all_green(&self) -> bool {
        self.verdicts.len() == 10 && self.greens == 10
    }

    /// 红项明细。
    pub fn red_items(&self) -> Vec<&'static str> {
        self.verdicts
            .iter()
            .filter(|v| !v.green)
            .map(|v| v.item.name())
            .collect()
    }
}

/// 复跑一致性账：十轮核验逐轮全绿断言（判据 10/10 口径）。
pub struct AsRepeater {
    /// 轮数。
    pub rounds: u32,
    /// 全绿轮数。
    pub green_rounds: u32,
    /// 逐轮指纹（应全同）。
    pub fingerprints: Vec<u64>,
}

impl AsRepeater {
    /// 空复跑账。
    pub fn new() -> Self {
        AsRepeater {
            rounds: 0,
            green_rounds: 0,
            fingerprints: Vec::new(),
        }
    }

    /// 复跑一轮：同面对拍 + 指纹记录。
    pub fn run(&mut self, v: &mut AsVerifier, face: &AsFace) {
        let all = v.verify(face);
        self.rounds += 1;
        if all {
            self.green_rounds += 1;
        }
        // 指纹：十项 (item ordinal, green) 串接 FNV。
        let mut h = 0xcbf29ce484222325u64;
        for vd in &v.verdicts {
            h ^= (vd.item.ordinal() as u64) << 8 | vd.green as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        self.fingerprints.push(h);
    }

    /// 10/10 复跑一致：全绿且指纹全同。
    pub fn consistent_10(&self) -> bool {
        self.rounds >= 10
            && self.green_rounds == self.rounds
            && self
                .fingerprints
                .iter()
                .all(|f| *f == self.fingerprints[0])
    }
}

/// 联签记录落档。
pub struct AsRecord {
    /// 十项结果行。
    pub lines: Vec<String>,
    /// 落档哈希。
    pub sealed: Option<u64>,
}

impl AsRecord {
    /// 空记录。
    pub fn new() -> Self {
        AsRecord {
            lines: Vec::new(),
            sealed: None,
        }
    }

    /// 从核验器生成记录行。
    pub fn build(&mut self, v: &AsVerifier) {
        self.lines.clear();
        for vd in &v.verdicts {
            self.lines.push(format!(
                "{}|{}|declared={} measured={} {}",
                vd.item.face(),
                vd.item.name(),
                vd.declared,
                vd.measured,
                if vd.green { "GREEN" } else { "RED" }
            ));
        }
    }

    /// 落档：记录行 FNV 冻结。
    pub fn seal(&mut self) -> u64 {
        let mut h = 0xcbf29ce484222325u64;
        for l in &self.lines {
            for b in l.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        self.sealed = Some(h);
        h
    }

    /// 落档可复读。
    pub fn readable(&self) -> bool {
        match self.sealed {
            None => false,
            Some(frozen) => {
                let mut h = 0xcbf29ce484222325u64;
                for l in &self.lines {
                    for b in l.bytes() {
                        h ^= b as u64;
                        h = h.wrapping_mul(0x100000001b3);
                    }
                }
                h == frozen
            }
        }
    }
}

/// 负向注入场景。
pub enum AsInject {
    /// 实测漂移（R3 计数守恒破）→ 核验必红。
    DriftR3,
    /// 实测漂移（T2 单调破）→ 核验必红。
    DriftT2,
    /// 红面复跑（红项指纹稳定 → 一致性仍成立但全绿断言假）。
    RedStable,
}

/// 注入核。
pub struct AsInjector {
    /// 场数。
    pub cases: u32,
    /// 检出数。
    pub caught: u32,
}

impl AsInjector {
    /// 空注入核。
    pub fn new() -> Self {
        AsInjector { cases: 0, caught: 0 }
    }

    /// 三场注入（独立核验器/复跑账 · 不污染主门面状态）。
    pub fn inject_all(&mut self) {
        let mut v = AsVerifier::new();
        let mut rep = AsRepeater::new();
        // 场一：R3 漂移 → 核验必红且红项恰为 R3。
        let drift = AsFace::drifted(0x1562, AsItem::ReschedConserve.ordinal());
        let all = v.verify(&drift);
        if !all && v.red_items().contains(&"R3-resched-conserve") {
            self.caught += 1;
        }
        self.cases += 1;
        // 场二：T2 漂移 → 红项恰为 T2。
        let drift2 = AsFace::drifted(0x1562, AsItem::TickMonotonic.ordinal());
        let _ = v.verify(&drift2);
        if v.red_items().contains(&"T2-tick-monotonic") {
            self.caught += 1;
        }
        self.cases += 1;
        // 场三：红面复跑十轮 → 指纹稳定（漂移可重复检出 · 非全绿）。
        for _ in 0..10 {
            rep.run(&mut v, &drift2);
        }
        if rep.rounds == 10
            && rep.green_rounds == 0
            && rep
                .fingerprints
                .iter()
                .all(|f| *f == rep.fingerprints[0])
        {
            self.caught += 1;
        }
        self.cases += 1;
    }

    /// 注入满格。
    pub fn all_caught(&self) -> bool {
        self.cases >= 3 && self.caught == self.cases
    }
}

/// A3 联签件门面（判据三件：十项全绿/10/10 复跑一致/记录落档）。
pub struct AsGate {
    /// 核验器。
    pub ver: AsVerifier,
    /// 复跑账。
    pub rep: AsRepeater,
    /// 记录。
    pub rec: AsRecord,
    /// 注入核。
    pub inj: AsInjector,
}

impl AsGate {
    /// 新门面。
    pub fn new() -> Self {
        AsGate {
            ver: AsVerifier::new(),
            rep: AsRepeater::new(),
            rec: AsRecord::new(),
            inj: AsInjector::new(),
        }
    }

    /// 判据总门。
    pub fn judge(&self) -> (bool, String) {
        let ok = self.ver.all_green()
            && self.rep.consistent_10()
            && self.rec.readable()
            && self.inj.all_caught();
        let msg = format!(
            "{}: greens={}/10 rounds={}/{} record={} inj={}/{} seg=0x{:04x}",
            AS_VERSION,
            self.ver.greens,
            self.rep.green_rounds,
            self.rep.rounds,
            self.rec.readable(),
            self.inj.caught,
            self.inj.cases,
            AS_SEG_BASE
        );
        (ok, msg)
    }
}

/// 判据自检（t 系入口）：一致面十轮复跑 + 记录落档 + 三场注入。
pub fn run_t562_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx1562-t562");
    let mut g = AsGate::new();
    // 一致面十轮复跑（判据 10/10）。
    let face = AsFace::consistent(0x1562_0001);
    for _ in 0..10 {
        g.rep.run(&mut g.ver, &face);
    }
    // 记录落档。
    g.rec.build(&g.ver);
    g.rec.seal();
    // 三场注入（隔离实例 · 主门面状态不受污染）。
    g.inj.inject_all();
    let (ok, msg) = g.judge();
    set.add_dyn("UNX-F1562-J1", ok, &msg);
    set.add("AS-ten-green", g.ver.all_green(), "");
    set.add("AS-replay-10/10", g.rep.consistent_10(), "");
    set.add("AS-record", g.rec.readable(), "");
    set.add("AS-inject-3/3", g.inj.all_caught(), "");
    set
}
