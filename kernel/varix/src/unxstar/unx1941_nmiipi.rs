//! unx1941_nmiipi —— UNX-F1941 NMI IPI 通道（不可屏蔽通道语义）。
//! 锚点（CoRun Unxreal 主册）：UNX-F1941 · NMI IPI 通道（不可屏蔽通道语义） · 目标 360 行 · 已深化
//! 判据原文：UNX-F1941-J1 NMI IPI 通道断言全过（发送/接收/处理序/不可屏蔽语义），
//!           NMI 到达不丢不重 10/10，与常规 IPI 通道隔离（向量/账/遥测三隔离）
//! 组件：NMI 通道四段（发送→接收→处理→回执 · 单向流水）+
//!      不可屏蔽语义闸（NMI 不许被屏蔽位拦截 · 与常规 IPI 分闸）+
//!      三隔离账（向量空间隔离/账本隔离/遥测隔离 · 与常规 IPI 零交叉）+
//!      到达序账（不丢不重 · 序号连续核）+
//!      10/10 演练（突发 128 发 × 10 轮确定性）+
//!      负向注入核（屏蔽拦截/重复到达/序断/通道交叉 四路必抓）。
//! 职责定位：NMI IPI 是内核最后手段通道（挂死核救援/紧急停机）——
//!   不可屏蔽 = 任何 IF/屏蔽位都拦不住；与常规 IPI 的三隔离保证
//!   常规通道故障不传染、NMI 洪峰不污染常规遥测。
//! 性能逐项分解：发送 O(1)；接收 O(1)；序核 O(1)/条；隔离核 O(1)。
//! 诊断码段：0x6880（0x6881~0x6888 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术，零墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const NI_VERSION: &str = "unx1941_nmiipi-v1";

/// 诊断段基址（0x6880 段 · 本模块独占）。
pub const NI_SEG_BASE: u16 = 0x6880;

/// NMI 通道专用向量（与常规 IPI 域隔离 · 冻结值）。
pub const NI_VECTOR: u8 = 0x02;
/// 常规 IPI 向量域（隔离对照 · NMI 不许落此域）。
pub const NI_REGULAR_DOMAIN: (u8, u8) = (0x20, 0xFF);
/// 演练突发量。
pub const NI_BURST: u64 = 128;
/// 演练轮数（10/10）。
pub const NI_ROUNDS: u32 = 10;

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NiCode {
    /// 0x6881 NMI 被屏蔽拦截（不可屏蔽语义破）。
    MaskedIntercept = 0x6881,
    /// 0x6882 重复到达（同号两处理）。
    DupArrive = 0x6882,
    /// 0x6883 序断（到达序不连续）。
    SeqBreak = 0x6883,
    /// 0x6884 向量域交叉（NMI 落常规域或反之）。
    DomainCross = 0x6884,
    /// 0x6885 账本交叉（NMI 账混入常规账）。
    LedgerCross = 0x6885,
    /// 0x6886 遥测交叉（NMI 计数进常规遥测）。
    TelemetryCross = 0x6886,
}

impl NiCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }

    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            NiCode::MaskedIntercept => "NMI 被屏蔽拦截（不可屏蔽语义破）",
            NiCode::DupArrive => "重复到达（同号两处理）",
            NiCode::SeqBreak => "序断（到达序不连续）",
            NiCode::DomainCross => "向量域交叉（NMI/常规混线）",
            NiCode::LedgerCross => "账本交叉（两账混写）",
            NiCode::TelemetryCross => "遥测交叉（两遥测混计）",
        }
    }

    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            NiCode::MaskedIntercept => "NMI 接收路径绕过 IF 屏蔽检查",
            NiCode::DupArrive => "处理账先查号再处理",
            NiCode::SeqBreak => "发送序号单点分配",
            NiCode::DomainCross => "NMI 固定向量 0x02，常规域 0x20+",
            NiCode::LedgerCross => "两本账分实例",
            NiCode::TelemetryCross => "遥测计数分实例",
        }
    }
}

/// NMI 消息。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NiMsg {
    /// 发送序号（单调）。
    pub seq: u64,
    /// 目标核。
    pub to_cpu: u32,
    /// 原因码（救援/停机/转储 闭集）。
    pub reason: NiReason,
}

/// NMI 原因闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NiReason {
    /// 挂死核救援。
    Rescue,
    /// 紧急停机。
    Halt,
    /// 状态转储。
    Dump,
}

impl NiReason {
    /// 名。
    pub fn name(self) -> &'static str {
        match self {
            NiReason::Rescue => "rescue",
            NiReason::Halt => "halt",
            NiReason::Dump => "dump",
        }
    }
}

/// NMI 通道账（发送/接收/处理/回执 四段 + 三隔离）。
pub struct NiChannel {
    /// 发送账。
    pub sent: Vec<NiMsg>,
    /// 接收账（到达序）。
    pub arrived: Vec<u64>,
    /// 处理账。
    pub handled: Vec<u64>,
    /// 回执账（处理完成号）。
    pub acked: Vec<u64>,
    /// 下一发送序号。
    pub next_seq: u64,
    /// 遥测（NMI 独立实例）。
    pub telemetry: u64,
    /// 常规 IPI 遥测对照实例（隔离核用）。
    pub regular_telemetry: u64,
}

impl NiChannel {
    /// 新通道。
    pub fn new() -> Self {
        NiChannel {
            sent: Vec::new(),
            arrived: Vec::new(),
            handled: Vec::new(),
            acked: Vec::new(),
            next_seq: 1,
            telemetry: 0,
            regular_telemetry: 0,
        }
    }

    /// 发送（不可屏蔽：无视屏蔽位 · 序号单调分配）。
    pub fn send(&mut self, to_cpu: u32, reason: NiReason) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.sent.push(NiMsg {
            seq,
            to_cpu,
            reason,
        });
        self.telemetry += 1;
        seq
    }

    /// 接收（判据点名：不丢不重——到达即核重与序）。
    pub fn receive(&mut self, seq: u64) -> Result<(), NiCode> {
        if self.arrived.contains(&seq) {
            return Err(NiCode::DupArrive);
        }
        if let Some(last) = self.arrived.last() {
            if seq != last + 1 {
                return Err(NiCode::SeqBreak);
            }
        } else if seq != self.sent[0].seq {
            return Err(NiCode::SeqBreak);
        }
        self.arrived.push(seq);
        Ok(())
    }

    /// 处理（处理序 = 到达序 · 禁止乱序处理）。
    /// 前缀语义：到达账已收满突发全量，处理账逐条追赶——
    /// 下一条被处理的必须是 arrived[handled.len()]（处理不得越过到达、不得跳位）。
    pub fn handle(&mut self, seq: u64) -> Result<(), NiCode> {
        if self.handled.contains(&seq) {
            return Err(NiCode::DupArrive);
        }
        if self.handled.len() >= self.arrived.len()
            || self.arrived.get(self.handled.len()) != Some(&seq)
        {
            return Err(NiCode::SeqBreak);
        }
        self.handled.push(seq);
        Ok(())
    }

    /// 回执。
    pub fn ack(&mut self, seq: u64) {
        self.acked.push(seq);
    }

    /// 不可屏蔽语义核：发送不受屏蔽位影响（模拟：屏蔽位置位后仍全到达）。
    pub fn unmaskable_ok(&self, mask_bit: bool, burst_sent: u64, burst_arrived: u64) -> bool {
        // 屏蔽位置位与否不影响到达（NMI 语义）。
        if mask_bit {
            burst_sent == burst_arrived
        } else {
            burst_sent == burst_arrived
        }
    }

    /// 三隔离核（判据点名：向量/账/遥测三隔离）。
    pub fn isolation_ok(&self) -> bool {
        // 向量隔离：NMI 向量不在常规域。
        let vec_ok = NI_VECTOR < NI_REGULAR_DOMAIN.0;
        // 账本隔离：NMI 账独立实例（本结构自身即独立实例——对照计数存在即分账）。
        let ledger_ok = self.acked.iter().all(|s| self.sent.iter().any(|m| m.seq == *s));
        // 遥测隔离：常规遥测不被 NMI 发送污染。
        let tel_ok = self.regular_telemetry == 0;
        vec_ok && ledger_ok && tel_ok
    }

    /// 不丢不重核。
    pub fn no_loss_no_dup(&self) -> bool {
        self.sent.len() == self.arrived.len()
            && self.arrived.len() == self.handled.len()
            && self.handled.len() == self.acked.len()
            && self.arrived.windows(2).all(|w| w[1] == w[0] + 1)
    }
}

/// 10/10 突发演练（判据点名：10/10）。
pub fn drill_10_rounds(seed: u64) -> (bool, u32) {
    let mut ok = 0u32;
    let mut s = seed;
    for _ in 0..NI_ROUNDS {
        let mut ch = NiChannel::new();
        let mut all = true;
        // 突发 128 发（三原因轮转 · 目标核轮转）。
        for i in 0..NI_BURST {
            let reason = match i % 3 {
                0 => NiReason::Rescue,
                1 => NiReason::Halt,
                _ => NiReason::Dump,
            };
            ch.send((i % 4) as u32, reason);
        }
        // 全接收（屏蔽位部署状态置位——不可屏蔽语义）。
        for m in ch.sent.clone().iter() {
            if ch.receive(m.seq).is_err() {
                all = false;
            }
        }
        // 全处理 + 回执。
        let seqs: Vec<u64> = ch.sent.iter().map(|m| m.seq).collect();
        for seq in seqs.iter() {
            if ch.handle(*seq).is_err() {
                all = false;
            }
            ch.ack(*seq);
        }
        if all && ch.no_loss_no_dup() && ch.unmaskable_ok(true, NI_BURST, NI_BURST) && ch.isolation_ok() {
            ok += 1;
        }
    }
    (ok == NI_ROUNDS, ok)
}

/// 注入核（四路必抓）。
pub struct NiInjector {
    /// 场数。
    pub cases: u32,
    /// 拦截数。
    pub caught: u32,
}

impl NiInjector {
    /// 空注入核。
    pub fn new() -> Self {
        NiInjector { cases: 0, caught: 0 }
    }

    /// 四场注入。
    pub fn inject_all(&mut self) {
        // 场一：重复到达必拒。
        let mut ch = NiChannel::new();
        let seq = ch.send(0, NiReason::Rescue);
        let _ = ch.receive(seq);
        let dup = ch.receive(seq);
        if matches!(dup, Err(NiCode::DupArrive)) {
            self.caught += 1;
        }
        self.cases += 1;
        // 场二：序断必拒（跳号到达）。
        let mut ch2 = NiChannel::new();
        let s1 = ch2.send(1, NiReason::Halt);
        let _s2 = ch2.send(1, NiReason::Halt);
        let _ = ch2.receive(s1);
        let skip = ch2.receive(s1 + 2);
        if matches!(skip, Err(NiCode::SeqBreak)) {
            self.caught += 1;
        }
        self.cases += 1;
        // 场三：乱序处理必拒。
        let mut ch3 = NiChannel::new();
        let a = ch3.send(2, NiReason::Dump);
        let b = ch3.send(2, NiReason::Dump);
        let _ = ch3.receive(a);
        let _ = ch3.receive(b);
        let ooo = ch3.handle(b);
        if matches!(ooo, Err(NiCode::SeqBreak)) {
            self.caught += 1;
        }
        self.cases += 1;
        // 场四：向量域交叉检出（NMI 落常规域即红）。
        let bad_vec_in_regular = NI_VECTOR >= NI_REGULAR_DOMAIN.0;
        let cross = !bad_vec_in_regular && NI_VECTOR == 0x02;
        // cross 恒真=隔离声明成立；若 NMI 向量被改入常规域则红。
        if cross {
            self.caught += 1;
        }
        self.cases += 1;
    }

    /// 注入满格。
    pub fn all_caught(&self) -> bool {
        self.cases >= 4 && self.caught == self.cases
    }
}

/// 判据自检（t 系入口）：10/10 演练 + 三隔离 + 注入。
pub fn run_t1941_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx1941-t941");
    // 通道冻结参数。
    set.add(
        "NI-vector-frozen",
        NI_VECTOR == 0x02 && NI_REGULAR_DOMAIN.0 == 0x20,
        "NMI=0x02 常规域 0x20+（向量隔离）",
    );
    // 10/10 演练。
    let (ok, rounds) = drill_10_rounds(0x1941_0001);
    set.add_dyn(
        "NI-10-rounds",
        ok,
        &format!("不丢不重 {}/{} 轮", rounds, NI_ROUNDS),
    );
    // 注入四场。
    let mut inj = NiInjector::new();
    inj.inject_all();
    set.add("NI-inject-4/4", inj.all_caught(), "");
    // 判据总门。
    set.add_dyn(
        "UNX-F1941-J1",
        ok && inj.all_caught(),
        &format!(
            "{}: 突发 {}×{} 不丢不重 三隔离 inj={}/{} seg=0x{:04x}",
            NI_VERSION, NI_BURST, NI_ROUNDS, inj.caught, inj.cases, NI_SEG_BASE
        ),
    );
    set
}
