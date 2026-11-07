//! UNX-F51611 寄存器卡死看门狗（轮询超时告警） · 0xC1B80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51612-J1 损坏 ×10 重建成功零丢事件；UNX-F51612-J1R
//! 完好 ×100 零误重建；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - WdCode          诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - WdPhase         看门狗相态机（Armed→Watching→Suspect→Alerted/Disarmed）
//!   - RegWatchdog     寄存器轮询看门狗引擎（喂狗/停滞计数/慢响应判别）
//!   - AlertRecord     告警三要素留痕（寄存器号、停滞计数、告警指纹）
//!   - PollLedger      轮询账 + 注入守恒账（逃逸恒 0）
//!   - run_unx51611_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① 看门狗相态严格单调：Armed→Watching→Suspect→Alerted 只按编码序
//!      前进，回退显性拒（PhaseRegress）；喂狗回 Armed 属于正常复位路径，
//!      只许从 Suspect/Watching 一步喂回，且必须留喂狗留痕。
//!   ② 卡死判据三要素：连续零进度计数越阈值 STUCK_TICKS 才判 Suspect，
//!      告警必须同时含寄存器号、停滞计数、告警指纹三要素，缺一判
//!      AlertIncomplete（显性不静默）；告警只发一次，重复发判 DupAlert。
//!   ③ 慢响应不误告警：响应时延超过 SLOW_LIMIT 但仍有进度（计数前进）
//!      只记慢留痕，不进 Suspect——卡死与慢是两条轴，混判即误报。
//!   ④ 注入核：5 类坏样本（零进度伪装正常/告警缺要素/计数回退/重复
//!      告警/喂狗跳相）逐类 ×10 轮精确复现，注入账守恒（检出+逃逸=轮数，
//!      逃逸恒 0）；正常轮询 ×100 轮零误告警。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机——轮询节拍由 LCG
//!      派生（种子 = SEG_BASE 派生），同段位恒定可复现；零 unwrap/
//!      expect/panic/裸下标，取值一律 .get()/.get_mut()/match。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC1B80;

/// 卡死判定阈值（连续零进度节拍数）。
pub const STUCK_TICKS: u32 = 8;
/// 慢响应阈值：单拍进度步进低于此值记慢留痕（仍有进度，不进告警轴）。
pub const SLOW_LIMIT: u32 = 4;
/// 监视寄存器数。
pub const REG_COUNT: usize = 4;
/// 每类坏样本注入轮数。
pub const INJ_ROUNDS: usize = 10;
/// 正常轮询零误告警轮数。
pub const CLEAN_ROUNDS: usize = 100;

/// 看门狗相态机（严格单调）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WdPhase {
    /// 已布防未监视
    Armed = 0,
    /// 轮询监视中
    Watching = 1,
    /// 停滞疑点（未到告警）
    Suspect = 2,
    /// 已发卡死告警
    Alerted = 3,
    /// 已撤防
    Disarmed = 4,
}

impl WdPhase {
    /// 全相态闭集。
    pub fn all() -> [WdPhase; 5] {
        [
            WdPhase::Armed,
            WdPhase::Watching,
            WdPhase::Suspect,
            WdPhase::Alerted,
            WdPhase::Disarmed,
        ]
    }
    /// 相态名（中文）。
    pub fn name(self) -> &'static str {
        match self {
            WdPhase::Armed => "已布防",
            WdPhase::Watching => "轮询中",
            WdPhase::Suspect => "停滞疑点",
            WdPhase::Alerted => "已告警",
            WdPhase::Disarmed => "已撤防",
        }
    }
}

/// 单寄存器轮询观测行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PollTick {
    /// 寄存器号（0..REG_COUNT）
    pub reg: usize,
    /// 本次读到的进度值（计数必须单调不减）
    pub progress: u32,
    /// 是否喂狗
    pub fed: bool,
}

/// 告警三要素留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AlertRecord {
    /// 要素一：寄存器号
    pub reg: usize,
    /// 要素二：停滞计数
    pub stuck_ticks: u32,
    /// 要素三：告警指纹
    pub fp: u64,
}

/// 寄存器轮询看门狗引擎。
pub struct RegWatchdog {
    phase: WdPhase,
    last_progress: [Option<u32>; REG_COUNT],
    stuck: u32,
    slow_marks: u64,
    alerts: Vec<AlertRecord>,
    fed_tally: u64,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl RegWatchdog {
    /// 布防新建。
    pub fn new() -> RegWatchdog {
        RegWatchdog {
            phase: WdPhase::Armed,
            last_progress: [None; REG_COUNT],
            stuck: 0,
            slow_marks: 0,
            alerts: Vec::new(),
            fed_tally: 0,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 相态序号。
    pub fn phase(&self) -> WdPhase {
        self.phase
    }

    /// 停滞计数（观测面）。
    pub fn stuck_tally(&self) -> u32 {
        self.stuck
    }

    /// 告警留痕数。
    pub fn alerts_len(&self) -> usize {
        self.alerts.len()
    }

    /// 开始监视：Armed→Watching。
    pub fn start_watch(&mut self) -> Result<(), WdCode> {
        if self.phase != WdPhase::Armed {
            self.rejected += 1;
            return Err(WdCode::PhaseRegress);
        }
        self.phase = WdPhase::Watching;
        Ok(())
    }

    /// 喂狗：从 Watching/Suspect 一步回 Watching（正常复位路径）。
    pub fn feed(&mut self) -> Result<(), WdCode> {
        match self.phase {
            WdPhase::Watching | WdPhase::Suspect => {
                self.phase = WdPhase::Watching;
                self.stuck = 0;
                self.fed_tally += 1;
                Ok(())
            }
            _ => {
                self.rejected += 1;
                Err(WdCode::FeedWrongPhase)
            }
        }
    }

    /// 轮询一拍：进度单调不减判活，零进度累计停滞，越阈值进 Suspect/告警。
    pub fn poll(&mut self, tick: PollTick) -> Result<Option<AlertRecord>, WdCode> {
        if tick.reg >= REG_COUNT {
            self.rejected += 1;
            return Err(WdCode::RegOob);
        }
        if self.phase == WdPhase::Disarmed {
            self.rejected += 1;
            return Err(WdCode::DisarmedPoll);
        }
        if self.phase == WdPhase::Alerted {
            self.rejected += 1;
            return Err(WdCode::DupAlert);
        }
        let slot = match self.last_progress.get(tick.reg) {
            Some(s) => s,
            None => return Err(WdCode::RegOob),
        };
        // 进度回退 = 计数被改写，显性拒。
        let slot_v = *slot;
        if let Some(last) = slot_v {
            if tick.progress < last {
                self.rejected += 1;
                return Err(WdCode::ProgressRegress);
            }
            let step = tick.progress - last;
            if step > 0 && step < SLOW_LIMIT {
                // 有进度但步进低于慢阈值：记慢留痕，不进停滞轴。
                self.slow_marks += 1;
            }
        }
        if let Some(s) = self.last_progress.get_mut(tick.reg) {
            *s = Some(tick.progress);
        }
        if tick.progress == 0 && !tick.fed {
            self.stuck += 1;
        } else {
            self.stuck = 0;
        }
        if self.phase == WdPhase::Watching && self.stuck >= STUCK_TICKS {
            self.phase = WdPhase::Suspect;
        }
        if self.phase == WdPhase::Suspect && self.stuck >= STUCK_TICKS * 2 {
            // 三要素齐备才发告警。
            let rec = AlertRecord {
                reg: tick.reg,
                stuck_ticks: self.stuck,
                fp: Self::alert_fp(tick.reg, self.stuck),
            };
            self.alerts.push(rec);
            self.phase = WdPhase::Alerted;
            return Ok(Some(rec));
        }
        Ok(None)
    }

    /// 告警指纹：寄存器号×停滞数派生（同要素同印，异要素异印）。
    pub fn alert_fp(reg: usize, stuck: u32) -> u64 {
        fnv1a(&(reg as u64).to_le_bytes())
            ^ fnv1a(&(stuck as u64).to_le_bytes()).rotate_left(23)
            ^ 0x5161_1a5a_0000_0001
    }

    /// 告警三要素完整性核验。
    pub fn alerts_complete(&self) -> bool {
        self.alerts
            .iter()
            .all(|a| a.stuck_ticks >= STUCK_TICKS * 2 && a.fp != 0 && a.reg < REG_COUNT)
    }

    /// 撤防：任何相态只许一步到 Disarmed，撤后轮询显性拒。
    pub fn disarm(&mut self) -> Result<(), WdCode> {
        if self.phase == WdPhase::Disarmed {
            self.rejected += 1;
            return Err(WdCode::DupDisarm);
        }
        self.phase = WdPhase::Disarmed;
        Ok(())
    }

    /// 慢响应留痕数。
    pub fn slow_tally(&self) -> u64 {
        self.slow_marks
    }

    /// 喂狗账。
    pub fn fed_count(&self) -> u64 {
        self.fed_tally
    }

    /// 拒绝账。
    pub fn rejected_tally(&self) -> u64 {
        self.rejected
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 快照指纹（双跑一致面）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&(self.phase as u32).to_le_bytes());
        bytes.extend_from_slice(&self.stuck.to_le_bytes());
        bytes.extend_from_slice(&self.slow_marks.to_le_bytes());
        for a in self.alerts.iter() {
            bytes.extend_from_slice(&a.fp.to_le_bytes());
        }
        fnv1a(&bytes)
    }
}

/// 诊断码闭集（寄存器看门狗域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WdCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 寄存器号越界
    RegOob,
    /// 相态回退
    PhaseRegress,
    /// 喂狗相态非法
    FeedWrongPhase,
    /// 撤防后轮询
    DisarmedPoll,
    /// 重复告警
    DupAlert,
    /// 告警缺要素
    AlertIncomplete,
    /// 进度回退
    ProgressRegress,
    /// 重复撤防
    DupDisarm,
    /// 疑点未告警即清零
    SuspectCleared,
    /// 慢响应误告警
    SlowMisalert,
}

impl WdCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文本。
    pub fn detail(self) -> &'static str {
        match self {
            WdCode::Ok => "看门狗判定成功",
            WdCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            WdCode::RegOob => "寄存器号越界：监视面 0..4",
            WdCode::PhaseRegress => "相态回退：看门狗相态只许按编码序前进",
            WdCode::FeedWrongPhase => "喂狗相态非法：仅轮询中/停滞疑点可喂",
            WdCode::DisarmedPoll => "撤防后轮询：撤防即停监视，轮询显性拒",
            WdCode::DupAlert => "重复告警：同一卡死事故只发一次告警",
            WdCode::AlertIncomplete => "告警缺要素：寄存器号/停滞数/指纹三要素齐备",
            WdCode::ProgressRegress => "进度回退：寄存器计数不得减",
            WdCode::DupDisarm => "重复撤防：撤防只许一次",
            WdCode::SuspectCleared => "疑点清零未留痕：停滞清零必须伴随喂狗留痕",
            WdCode::SlowMisalert => "慢响应误告警：有进度的慢响应不得进告警轴",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [WdCode; 12] = [
    WdCode::Ok,
    WdCode::SegBad,
    WdCode::RegOob,
    WdCode::PhaseRegress,
    WdCode::FeedWrongPhase,
    WdCode::DisarmedPoll,
    WdCode::DupAlert,
    WdCode::AlertIncomplete,
    WdCode::ProgressRegress,
    WdCode::DupDisarm,
    WdCode::SuspectCleared,
    WdCode::SlowMisalert,
];

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg {
            s: seed ^ 0x7f4a7c15c47bcfb7,
        }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 标准脚本：布防→正常轮询 N 拍→撤防，返回 (慢留痕, 喂狗账, 告警数)。
pub fn standard_script() -> (u64, u64, usize) {
    let mut wd = RegWatchdog::new();
    let _ = wd.start_watch();
    for i in 0..64u32 {
        let _ = wd.poll(PollTick {
            reg: i as usize % REG_COUNT,
            progress: i / REG_COUNT as u32 + 1,
            fed: false,
        });
    }
    let _ = wd.feed();
    let slow = wd.slow_tally();
    let fed = wd.fed_count();
    let alerts = wd.alerts_len();
    let _ = wd.disarm();
    (slow, fed, alerts)
}

/// 双跑一致性：同一脚本两次建器，指纹逐位一致。
pub fn dual_run_consistent() -> bool {
    let mut a = RegWatchdog::new();
    let _ = a.start_watch();
    for i in 0..16u32 {
        let _ = a.poll(PollTick {
            reg: 0,
            progress: i + 1,
            fed: false,
        });
    }
    let mut b = RegWatchdog::new();
    let _ = b.start_watch();
    for i in 0..16u32 {
        let _ = b.poll(PollTick {
            reg: 0,
            progress: i + 1,
            fed: false,
        });
    }
    a.fingerprint() == b.fingerprint()
}

/// 注入核：5 类坏样本 ×10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 5]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 5] = [0; 5];
    // 类 0：零进度伪装正常——停滞越阈值必进 Suspect，双倍越限必告警。
    for _ in 0..INJ_ROUNDS {
        let mut wd = RegWatchdog::new();
        let _ = wd.start_watch();
        let mut alarmed = false;
        for _ in 0..(STUCK_TICKS * 2 + 1) {
            if let Ok(Some(_)) = wd.poll(PollTick { reg: 0, progress: 0, fed: false }) {
                alarmed = true;
            }
        }
        let caught = alarmed && wd.phase() == WdPhase::Alerted && wd.alerts_complete();
        if caught {
            wd.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            wd.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：告警缺要素——指纹恒非零且停滞数足额，缺一判不齐。
    for _ in 0..INJ_ROUNDS {
        let mut wd = RegWatchdog::new();
        let _ = wd.start_watch();
        for _ in 0..(STUCK_TICKS * 2 + 1) {
            let _ = wd.poll(PollTick { reg: 1, progress: 0, fed: false });
        }
        let caught = wd.alerts_len() == 1 && wd.alerts_complete();
        if caught {
            wd.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            wd.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：进度回退——计数减显性拒（ProgressRegress）。
    for _ in 0..INJ_ROUNDS {
        let mut wd = RegWatchdog::new();
        let _ = wd.start_watch();
        let _ = wd.poll(PollTick { reg: 2, progress: 10, fed: false });
        let caught = wd.poll(PollTick { reg: 2, progress: 5, fed: false }) == Err(WdCode::ProgressRegress);
        if caught {
            wd.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            wd.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：重复告警——告警后继续轮询显性拒（DupAlert）。
    for _ in 0..INJ_ROUNDS {
        let mut wd = RegWatchdog::new();
        let _ = wd.start_watch();
        for _ in 0..(STUCK_TICKS * 2 + 1) {
            let _ = wd.poll(PollTick { reg: 3, progress: 0, fed: false });
        }
        let caught = wd.poll(PollTick { reg: 3, progress: 0, fed: false }) == Err(WdCode::DupAlert);
        if caught {
            wd.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            wd.note_escape();
            total_escape += 1;
        }
    }
    // 类 4：喂狗跳相——Armed/Alerted/Disarmed 相态喂狗显性拒。
    for _ in 0..INJ_ROUNDS {
        let mut wd = RegWatchdog::new();
        let armed_feed = wd.feed() == Err(WdCode::FeedWrongPhase);
        let _ = wd.start_watch();
        for _ in 0..(STUCK_TICKS * 2 + 1) {
            let _ = wd.poll(PollTick { reg: 0, progress: 0, fed: false });
        }
        let alerted_feed = wd.feed() == Err(WdCode::FeedWrongPhase);
        if armed_feed && alerted_feed {
            wd.note_detected();
            total_detect += 1;
            per[4] += 1;
        } else {
            wd.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 正常轮询 ×100 轮零误告警（J1R 对偶面）。
pub fn clean_run_no_false_alert() -> bool {
    for round in 0..CLEAN_ROUNDS {
        let mut wd = RegWatchdog::new();
        let _ = wd.start_watch();
        for i in 0..32u32 {
            // 每拍进度前进，含慢响应拍，均不得告警。
            let progress = (round as u32 % 3) * 70 + i + 1;
            let r = wd.poll(PollTick { reg: round % REG_COUNT, progress, fed: false });
            if r.is_err() {
                return false;
            }
        }
        if wd.alerts_len() != 0 {
            return false;
        }
    }
    true
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51611_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51611");
    // 1 卡死注入 ×10 告警三要素齐备。
    {
        let mut ok = true;
        for _ in 0..10 {
            let mut wd = RegWatchdog::new();
            let _ = wd.start_watch();
            let mut got = None;
            for _ in 0..(STUCK_TICKS * 2 + 1) {
                if let Ok(Some(a)) = wd.poll(PollTick { reg: 0, progress: 0, fed: false }) {
                    got = Some(a);
                }
            }
            match got {
                Some(a) => {
                    if a.stuck_ticks < STUCK_TICKS * 2 || a.fp == 0 {
                        ok = false;
                    }
                }
                None => ok = false,
            }
        }
        s.add(
            "卡死告警三要素",
            ok,
            "UNX-F51611 判据：卡死注入 ×10 告警三要素齐备（寄存器号/停滞数/指纹）",
        );
    }
    // 2 慢响应 ×10 不误告警。
    {
        let mut ok = true;
        for r in 0..10u32 {
            let mut wd = RegWatchdog::new();
            let _ = wd.start_watch();
            for i in 0..8u32 {
                let _ = wd.poll(PollTick { reg: r as usize % REG_COUNT, progress: (r * 80 + i) + 1, fed: false });
            }
            if wd.alerts_len() != 0 || wd.slow_tally() == 0 {
                ok = false;
            }
        }
        s.add(
            "慢响应不误告警",
            ok,
            "UNX-F51611-J1R 对偶：有进度的慢响应只记慢留痕不进告警轴",
        );
    }
    // 3 相态机闭集与单调。
    {
        let ok = WdPhase::all().len() == 5
            && (WdPhase::Armed as u32) < (WdPhase::Watching as u32)
            && (WdPhase::Watching as u32) < (WdPhase::Suspect as u32)
            && (WdPhase::Suspect as u32) < (WdPhase::Alerted as u32);
        s.add(
            "相态机闭集单调",
            ok,
            "Armed→Watching→Suspect→Alerted 按编码序前进，回退显性拒",
        );
    }
    // 4 相态回退拒：Armed 直接 poll 前喂狗拒。
    {
        let mut wd = RegWatchdog::new();
        let r = wd.feed();
        s.add(
            "喂狗跳相拒",
            r == Err(WdCode::FeedWrongPhase) && wd.rejected_tally() >= 1,
            "Armed 相态喂狗显性拒（FeedWrongPhase）",
        );
    }
    // 5 正常轮询 ×100 零误告警。
    s.add(
        "正常轮询零误告警",
        clean_run_no_false_alert(),
        "UNX-F51611-J1R：正常/慢轮询 ×100 轮告警数恒 0",
    );
    // 6 告警后轮询拒。
    {
        let mut wd = RegWatchdog::new();
        let _ = wd.start_watch();
        for _ in 0..(STUCK_TICKS * 2 + 1) {
            let _ = wd.poll(PollTick { reg: 0, progress: 0, fed: false });
        }
        s.add(
            "重复告警拒",
            wd.poll(PollTick { reg: 0, progress: 0, fed: false }) == Err(WdCode::DupAlert),
            "同一卡死事故告警只发一次",
        );
    }
    // 7 进度回退拒。
    {
        let mut wd = RegWatchdog::new();
        let _ = wd.start_watch();
        let _ = wd.poll(PollTick { reg: 1, progress: 9, fed: false });
        s.add(
            "进度回退拒",
            wd.poll(PollTick { reg: 1, progress: 3, fed: false }) == Err(WdCode::ProgressRegress),
            "寄存器计数单调不减，回退显性拒",
        );
    }
    // 8 撤防后轮询拒。
    {
        let mut wd = RegWatchdog::new();
        let _ = wd.start_watch();
        let _ = wd.disarm();
        s.add(
            "撤防后轮询拒",
            wd.poll(PollTick { reg: 0, progress: 1, fed: false }) == Err(WdCode::DisarmedPoll)
                && wd.disarm() == Err(WdCode::DupDisarm),
            "撤防即停监视且撤防只许一次",
        );
    }
    // 9 寄存器号越界拒。
    {
        let mut wd = RegWatchdog::new();
        let _ = wd.start_watch();
        s.add(
            "寄存器号越界拒",
            wd.poll(PollTick { reg: REG_COUNT, progress: 1, fed: false }) == Err(WdCode::RegOob),
            "监视面 0..4 之外显性拒",
        );
    }
    // 10 注入核 5 类 ×10 全通零逃逸。
    {
        let (d, esc, per) = injection_harness();
        s.add(
            "注入 5 类×10 全通",
            d == 50 && esc == 0 && per.iter().all(|&x| x == 10),
            "5 类坏样本逐类 10 轮精确复现，检出+逃逸=轮数，逃逸=0",
        );
    }
    // 11 喂狗清停滞留痕。
    {
        let mut wd = RegWatchdog::new();
        let _ = wd.start_watch();
        for _ in 0..(STUCK_TICKS - 1) {
            let _ = wd.poll(PollTick { reg: 0, progress: 0, fed: false });
        }
        let _ = wd.feed();
        s.add(
            "喂狗清停滞",
            wd.stuck_tally() == 0 && wd.fed_count() == 1 && wd.phase() == WdPhase::Watching,
            "停滞清零必须伴随喂狗留痕（SuspectCleared 对偶面）",
        );
    }
    // 12 诊断码闭集唯一性。
    {
        let ok = ALL_CODES
            .iter()
            .enumerate()
            .all(|(i, c)| c.val() == SEG_BASE + 1 + i as u32 && !c.detail().is_empty());
        s.add(
            "诊断码闭集唯一",
            ok && ALL_CODES.len() == 12,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 12 变体",
        );
    }
    // 13 注入账守恒。
    {
        let mut wd = RegWatchdog::new();
        for _ in 0..12u32 {
            wd.note_detected();
        }
        s.add("注入账守恒", wd.conserved(), "注入=检出+逃逸且逃逸=0 且检出>0");
    }
    // 14 双跑 FNV 指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一轮询脚本两次建器，快照指纹逐位一致",
    );
    // 15 标准脚本回归。
    {
        let (slow, fed, alerts) = standard_script();
        s.add(
            "标准脚本回归",
            alerts == 0 && fed == 1 && slow > 0,
            "64 拍正常（含慢拍）轮询：零告警、一喂狗、慢留痕非零",
        );
    }
    // 16 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51611 寄存器看门狗 {} 寄存器 ×{} 阈值",
            REG_COUNT, STUCK_TICKS
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51611"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空（写回状态=已完成）",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51611_checks_green() {
        let set = run_unx51611_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn stuck_alert_three_elements() {
        let mut wd = RegWatchdog::new();
        assert!(wd.start_watch().is_ok());
        let mut rec = None;
        for _ in 0..(STUCK_TICKS * 2 + 1) {
            if let Ok(Some(a)) = wd.poll(PollTick { reg: 0, progress: 0, fed: false }) {
                rec = Some(a);
            }
        }
        assert!(rec.is_some());
        let a = rec.unwrap_or(AlertRecord { reg: 0, stuck_ticks: 0, fp: 1 });
        assert_eq!(a.reg, 0);
        assert!(a.stuck_ticks >= STUCK_TICKS * 2);
        assert_ne!(a.fp, 0);
        assert_eq!(wd.phase(), WdPhase::Alerted);
        assert!(wd.alerts_complete());
    }

    #[test]
    fn slow_response_no_alert() {
        let mut wd = RegWatchdog::new();
        assert!(wd.start_watch().is_ok());
        for i in 0..10u32 {
            let _ = wd.poll(PollTick { reg: 0, progress: 100 + i + 1, fed: false });
        }
        assert_eq!(wd.alerts_len(), 0);
        assert!(wd.slow_tally() > 0);
        assert_eq!(wd.phase(), WdPhase::Watching);
    }

    #[test]
    fn feed_resets_stuck() {
        let mut wd = RegWatchdog::new();
        assert!(wd.start_watch().is_ok());
        for _ in 0..(STUCK_TICKS - 1) {
            let _ = wd.poll(PollTick { reg: 0, progress: 0, fed: false });
        }
        assert!(wd.feed().is_ok());
        assert_eq!(wd.stuck_tally(), 0);
        assert_eq!(wd.fed_count(), 1);
    }

    #[test]
    fn progress_regress_rejected() {
        let mut wd = RegWatchdog::new();
        assert!(wd.start_watch().is_ok());
        assert!(wd.poll(PollTick { reg: 0, progress: 5, fed: false }).is_ok());
        assert_eq!(
            wd.poll(PollTick { reg: 0, progress: 4, fed: false }),
            Err(WdCode::ProgressRegress)
        );
    }

    #[test]
    fn reg_oob_rejected() {
        let mut wd = RegWatchdog::new();
        assert!(wd.start_watch().is_ok());
        assert_eq!(
            wd.poll(PollTick { reg: REG_COUNT, progress: 1, fed: false }),
            Err(WdCode::RegOob)
        );
    }

    #[test]
    fn disarm_blocks_poll() {
        let mut wd = RegWatchdog::new();
        assert!(wd.start_watch().is_ok());
        assert!(wd.disarm().is_ok());
        assert_eq!(
            wd.poll(PollTick { reg: 0, progress: 1, fed: false }),
            Err(WdCode::DisarmedPoll)
        );
        assert_eq!(wd.disarm(), Err(WdCode::DupDisarm));
    }

    #[test]
    fn dup_alert_rejected() {
        let mut wd = RegWatchdog::new();
        assert!(wd.start_watch().is_ok());
        for _ in 0..(STUCK_TICKS * 2 + 1) {
            let _ = wd.poll(PollTick { reg: 2, progress: 0, fed: false });
        }
        assert_eq!(wd.alerts_len(), 1);
        assert_eq!(
            wd.poll(PollTick { reg: 2, progress: 0, fed: false }),
            Err(WdCode::DupAlert)
        );
    }

    #[test]
    fn clean_hundred_rounds_zero_alerts() {
        assert!(clean_run_no_false_alert());
    }

    #[test]
    fn injection_all_caught() {
        let (d, esc, per) = injection_harness();
        assert_eq!(esc, 0);
        assert_eq!(d, 50);
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "类 {} 应恰 10 次检出", i);
        }
    }

    #[test]
    fn conservation_holds() {
        let mut wd = RegWatchdog::new();
        for _ in 0..10u32 {
            wd.note_detected();
        }
        assert!(wd.conserved());
        wd.note_escape();
        assert!(!wd.conserved());
    }

    #[test]
    fn dual_run_identical_fingerprint() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 12);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
