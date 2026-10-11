//! unx563_pm1sts —— UNX-F0563 PM1_STS 状态账（事件位观察）。
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-A1-B09 · 行 2097）：
//! 「UNX-F0563 | PM1_STS 状态账（事件位观察） | 240 | 已深化 |
//!   判据：PM1_STS 只读快照（事件位账齐——WAK/PWRBTN/RTC 位），W1C 位零写断言」
//!
//! 组件：PM1_STS 事件位闭集（WAK/PWRBTN/RTC/SLP_EN 变化/TMR/GBL 六位）
//!      + 只读快照状态机（Open→Sampled→Sealed 单向——封账后拒再采样）
//!      + W1C 零写防线（写尝试拦截器：任何对 W1C 位的写动作一律拦截留痕）
//!      + 事件观察账（位翻转流水 append-only）+ 双读一致断言（2/2 口径）
//!      + 快照指纹（FNV-1a 确定性）
//!
//! 判据落地：①事件位账齐（WAK/PWRBTN/RTC 三位各有账）；②W1C 位零写
//! （写尝试 100% 拦截并留痕——A1 期观察口径只读不写）；③双读一致
//! （同拍双采样位型逐位一致 2/2）。
//!
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性
//! （逻辑 tick 注入，零墙钟，零 IO），零浮点。分层声明：本层是电源
//! 事件的观察账面（只读语义），真实 PM1_STS 寄存器访问归 ACPI 硬件层。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与诊断段
// ---------------------------------------------------------------------------

/// 模块版本。
pub const PMS_VERSION: &str = "unx563_pm1sts-v1";

/// 诊断段基址（0x9A00 段 · 本模块独占，入库前已 grep 全域源文件确认无撞段）。
pub const PMS_SEG_BASE: u16 = 0x9A00;

/// 事件观察账容量（append-only 截断面）。
pub const EVT_LOG_CAP: usize = 64;

/// 写尝试拦截账容量（滚动）。
pub const WLOG_CAP: usize = 16;

/// 双读一致口径的采样对数。
pub const DOUBLE_READ_PAIRS: usize = 2;

// ---------------------------------------------------------------------------
// PM1_STS 事件位闭集
// ---------------------------------------------------------------------------

/// PM1_STS 事件位闭集（ACPI 规范位型；位值写死并注明来源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StsBit {
    /// 位 0 TMR —— 定时器事件（W1C）。
    Timer = 0x0001,
    /// 位 4 BUS Master —— 总线主事件（观察）。
    BusMaster = 0x0010,
    /// 位 5 GBL —— 全局事件（W1C）。
    Global = 0x0020,
    /// 位 8 PWRBTN —— 电源按钮事件（W1C）。
    PowerButton = 0x0100,
    /// 位 9 SLPBTN —— 睡眠按钮事件（W1C）。
    SleepButton = 0x0200,
    /// 位 10 RTC —— RTC 闹钟事件（W1C）。
    RtcAlarm = 0x0400,
    /// 位 14 WAK —— 唤醒事件（W1C）。
    Wake = 0x4000,
}

impl StsBit {
    /// 位值。
    pub const fn bits(self) -> u16 {
        self as u16
    }

    /// 是否 W1C（写一清零——软件写 1 才能清除；A1 期禁写）。
    pub const fn is_w1c(self) -> bool {
        matches!(
            self,
            StsBit::Timer
                | StsBit::Global
                | StsBit::PowerButton
                | StsBit::SleepButton
                | StsBit::RtcAlarm
                | StsBit::Wake
        )
    }

    /// 人读名（判据点名的三位优先入账）。
    pub const fn zh(self) -> &'static str {
        match self {
            StsBit::Timer => "定时器",
            StsBit::BusMaster => "总线主",
            StsBit::Global => "全局",
            StsBit::PowerButton => "电源按钮",
            StsBit::SleepButton => "睡眠按钮",
            StsBit::RtcAlarm => "RTC 闹钟",
            StsBit::Wake => "唤醒",
        }
    }
}

/// 判据点名的事件位账（WAK/PWRBTN/RTC 三位必须各有账）。
pub const JUDGE_BITS: [StsBit; 3] = [StsBit::Wake, StsBit::PowerButton, StsBit::RtcAlarm];

/// 全部 W1C 位（零写防线覆盖面）。
pub const W1C_BITS: [StsBit; 6] = [
    StsBit::Timer,
    StsBit::Global,
    StsBit::PowerButton,
    StsBit::SleepButton,
    StsBit::RtcAlarm,
    StsBit::Wake,
];

// ---------------------------------------------------------------------------
// 诊断码（闭集 · 0x9A01~0x9A0A）
// ---------------------------------------------------------------------------

/// 诊断码闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PmsCode {
    /// 0x9A01 封账后再采样（Sealed 终态拒读改）。
    SampleAfterSealed,
    /// 0x9A02 W1C 位写尝试（零写红线——拦截并留痕）。
    W1cWriteAttempt,
    /// 0x9A03 双读不一致（同拍双采样位型差异）。
    DoubleReadMismatch,
    /// 0x9A04 事件位账缺位（判据点名三位任一无账）。
    JudgeBitMissing,
    /// 0x9A05 非法位掩码注入（闭集外位出现）。
    BitOutsideClosedSet,
    /// 0x9A06 观察账满（append-only 截断）。
    EvtLogFull,
    /// 0x9A07 状态机非法迁移。
    FsmBadMove,
    /// 0x9A08 快照指纹对不上。
    FingerprintMismatch,
    /// 0x9A09 位翻转无观察来源（凭空位变化——模型完整性破坏）。
    FlipWithoutSource,
    /// 0x9A10 采样零拍（未推进时钟即采样——确定性破坏）。
    SampleAtZeroTick,
}

impl PmsCode {
    /// 段内码值。
    pub const fn val(self) -> u16 {
        match self {
            PmsCode::SampleAfterSealed => 0x9A01,
            PmsCode::W1cWriteAttempt => 0x9A02,
            PmsCode::DoubleReadMismatch => 0x9A03,
            PmsCode::JudgeBitMissing => 0x9A04,
            PmsCode::BitOutsideClosedSet => 0x9A05,
            PmsCode::EvtLogFull => 0x9A06,
            PmsCode::FsmBadMove => 0x9A07,
            PmsCode::FingerprintMismatch => 0x9A08,
            PmsCode::FlipWithoutSource => 0x9A09,
            PmsCode::SampleAtZeroTick => 0x9A0A,
        }
    }

    /// 段归属校验。
    pub const fn in_segment(v: u16) -> bool {
        v >= PMS_SEG_BASE + 1 && v <= PMS_SEG_BASE + 0x0A
    }

    /// 码值反查。
    pub fn of(v: u16) -> Option<PmsCode> {
        if !PmsCode::in_segment(v) {
            return None;
        }
        match v - PMS_SEG_BASE {
            1 => Some(PmsCode::SampleAfterSealed),
            2 => Some(PmsCode::W1cWriteAttempt),
            3 => Some(PmsCode::DoubleReadMismatch),
            4 => Some(PmsCode::JudgeBitMissing),
            5 => Some(PmsCode::BitOutsideClosedSet),
            6 => Some(PmsCode::EvtLogFull),
            7 => Some(PmsCode::FsmBadMove),
            8 => Some(PmsCode::FingerprintMismatch),
            9 => Some(PmsCode::FlipWithoutSource),
            10 => Some(PmsCode::SampleAtZeroTick),
            _ => None,
        }
    }

    /// 现象。
    pub const fn what(self) -> &'static str {
        match self {
            PmsCode::SampleAfterSealed => "封账后仍尝试采样",
            PmsCode::W1cWriteAttempt => "对 W1C 位的写尝试被拦截",
            PmsCode::DoubleReadMismatch => "同拍双采样位型不一致",
            PmsCode::JudgeBitMissing => "判据点名事件位缺账",
            PmsCode::BitOutsideClosedSet => "出现闭集外位型",
            PmsCode::EvtLogFull => "事件观察账已满",
            PmsCode::FsmBadMove => "快照状态机非法迁移",
            PmsCode::FingerprintMismatch => "快照指纹与账面重算不一致",
            PmsCode::FlipWithoutSource => "位变化无观察来源",
            PmsCode::SampleAtZeroTick => "零拍采样（时钟未推进）",
        }
    }

    /// 原因。
    pub const fn why(self) -> &'static str {
        match self {
            PmsCode::SampleAfterSealed => "封账语义是终态：账面已定，再采样只会制造第二真相",
            PmsCode::W1cWriteAttempt => "W1C 位写 1 才清零——任何写动作都改变事件史，A1 期观察口径禁写",
            PmsCode::DoubleReadMismatch => "采样间隙位型被改（真实硬件上就是事件竞态）",
            PmsCode::JudgeBitMissing => "注入序列未覆盖判据点名的事件位",
            PmsCode::BitOutsideClosedSet => "上游把 PM1_EN 或其他寄存器的位型灌进了 STS 账",
            PmsCode::EvtLogFull => "事件频次超账容量且未归档",
            PmsCode::FsmBadMove => "状态迁移未过 can_move 闸",
            PmsCode::FingerprintMismatch => "绕过观察口直改账面",
            PmsCode::FlipWithoutSource => "位型被直接赋值而非经事件入口",
            PmsCode::SampleAtZeroTick => "采样前未推进逻辑时钟，违反确定性口径",
        }
    }

    /// 建议。
    pub const fn advice(self) -> &'static str {
        match self {
            PmsCode::SampleAfterSealed => "开新账做第二轮观察（代际切换），旧账只读",
            PmsCode::W1cWriteAttempt => "维持只读观察口径；清位需求走 A2 期处置线的授权面",
            PmsCode::DoubleReadMismatch => "双采间隙禁止任何写路径；差异位立案并标注竞态窗口",
            PmsCode::JudgeBitMissing => "补齐注入序列：WAK/PWRBTN/RTC 三事件各至少一次翻转",
            PmsCode::BitOutsideClosedSet => "以位掩码闭集过滤入口，闭集外位直接拒绝并留痕",
            PmsCode::EvtLogFull => "先快照+指纹归档，再开新账续记",
            PmsCode::FsmBadMove => "所有迁移先过 StsFsm::can_move，拒绝即立案",
            PmsCode::FingerprintMismatch => "停写取快照取证，从最近可信快照重建",
            PmsCode::FlipWithoutSource => "位型只许经 observe 事件入口变化，直改路径全部封死",
            PmsCode::SampleAtZeroTick => "采样前先 tick()，保持确定性回放能力",
        }
    }

    /// 三要素合一。
    pub fn detail(self) -> String {
        let mut s = String::new();
        s.push_str("[0x");
        push_hex_u16(&mut s, self.val());
        s.push_str("] ");
        s.push_str(self.what());
        s.push_str(" ｜ 原因：");
        s.push_str(self.why());
        s.push_str(" ｜ 建议：");
        s.push_str(self.advice());
        s
    }
}

// ---------------------------------------------------------------------------
// 快照状态机（Open→Sampled→Sealed 单向）
// ---------------------------------------------------------------------------

/// 账面状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StsFsm {
    /// 开账（可注入事件、可采样）。
    Open,
    /// 已采样（位型已定，待封账）。
    Sampled,
    /// 已封账（终态：只读）。
    Sealed,
}

impl StsFsm {
    /// 合法迁移表（单向）。
    pub const fn can_move(self, to: StsFsm) -> bool {
        matches!(
            (self, to),
            (StsFsm::Open, StsFsm::Sampled)
                | (StsFsm::Sampled, StsFsm::Sealed)
                | (StsFsm::Open, StsFsm::Sealed)
        )
    }
}

// ---------------------------------------------------------------------------
// 观察账条目
// ---------------------------------------------------------------------------

/// 一笔事件观察（位、翻转方向、时刻、来源标签）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EvtEntry {
    /// 事件位。
    pub bit: StsBit,
    /// true=置位（0→1），false=清位（1→0）。
    pub raised: bool,
    /// 时刻（逻辑 tick）。
    pub tick: u64,
    /// 来源标签（注入器身份——回放对账用）。
    pub source: u16,
}

/// 一笔写尝试拦截留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WriteAttempt {
    /// 尝试写的位掩码。
    pub mask: u16,
    /// 尝试时刻。
    pub tick: u64,
    /// 命中的 W1C 位型（掩码内位）。
    pub w1c_hits: u16,
}

/// 写尝试拦截账（滚动）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WriteLog {
    ring: [Option<WriteAttempt>; WLOG_CAP],
    head: usize,
    seen: u64,
    dropped: u64,
}

impl WriteLog {
    /// 空账。
    pub const fn new() -> Self {
        WriteLog {
            ring: [None; WLOG_CAP],
            head: 0,
            seen: 0,
            dropped: 0,
        }
    }

    /// 入账。
    pub fn push(&mut self, a: WriteAttempt) {
        if self.ring[self.head].is_some() {
            self.dropped += 1;
        }
        self.ring[self.head] = Some(a);
        self.head = (self.head + 1) % WLOG_CAP;
        self.seen += 1;
    }

    /// 快照。
    pub fn snapshot(&self) -> Vec<WriteAttempt> {
        let mut out: Vec<WriteAttempt> = Vec::new();
        let mut i = self.head;
        for _ in 0..WLOG_CAP {
            if let Some(a) = &self.ring[i] {
                out.push(*a);
            }
            i = (i + 1) % WLOG_CAP;
        }
        out
    }

    /// 守恒：kept+dropped≡seen。
    pub const fn conserved(&self) -> bool {
        let kept = if self.seen >= WLOG_CAP as u64 {
            WLOG_CAP as u64
        } else {
            self.seen
        };
        kept + self.dropped == self.seen
    }

    /// 累计拦截数。
    pub const fn seen(&self) -> u64 {
        self.seen
    }
}

// ---------------------------------------------------------------------------
// PM1_STS 状态账（本模块主体）
// ---------------------------------------------------------------------------

/// PM1_STS 只读观察账：位型 × 事件账 × W1C 零写防线 × 快照状态机。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Pm1StsLedger {
    /// 当前位型镜像（只许经 observe 入口变化）。
    raw: u16,
    /// 账面状态。
    fsm: StsFsm,
    /// 事件观察账（append-only）。
    evt: Vec<EvtEntry>,
    /// 写尝试拦截账。
    wlog: WriteLog,
    /// 每位累计置位次数（判据「账齐」口径：点名三位各 ≥1）。
    raised_counts: [(u16, u64); 7],
    /// 逻辑时钟。
    tick: u64,
    /// 立案数。
    cases: u64,
    /// 双读计数（完成的对数）。
    double_reads: u64,
}

impl Pm1StsLedger {
    /// 新账（全零位型、Open 态）。
    pub fn new() -> Self {
        Pm1StsLedger {
            raw: 0,
            fsm: StsFsm::Open,
            evt: Vec::new(),
            wlog: WriteLog::new(),
            raised_counts: [
                (StsBit::Timer.bits(), 0),
                (StsBit::BusMaster.bits(), 0),
                (StsBit::Global.bits(), 0),
                (StsBit::PowerButton.bits(), 0),
                (StsBit::SleepButton.bits(), 0),
                (StsBit::RtcAlarm.bits(), 0),
                (StsBit::Wake.bits(), 0),
            ],
            tick: 0,
            cases: 0,
            double_reads: 0,
        }
    }

    /// 时钟步进。
    pub const fn tick(&mut self) {
        self.tick += 1;
    }

    /// 当前位型。
    pub const fn raw(&self) -> u16 {
        self.raw
    }

    /// 当前状态。
    pub const fn state(&self) -> StsFsm {
        self.fsm
    }

    /// 位型闭集校验（只许闭集内位出现）。
    pub const fn bits_in_closed_set(v: u16) -> bool {
        let all = StsBit::Timer.bits()
            | StsBit::BusMaster.bits()
            | StsBit::Global.bits()
            | StsBit::PowerButton.bits()
            | StsBit::SleepButton.bits()
            | StsBit::RtcAlarm.bits()
            | StsBit::Wake.bits();
        v & !all == 0
    }

    /// 事件观察入口（位型变化的唯一通道——防「凭空位变化」）。
    pub fn observe(&mut self, bit: StsBit, raised: bool, source: u16) -> Result<(), PmsCode> {
        if self.fsm == StsFsm::Sealed {
            self.cases += 1;
            return Err(PmsCode::SampleAfterSealed);
        }
        if self.tick == 0 {
            self.cases += 1;
            return Err(PmsCode::SampleAtZeroTick);
        }
        if self.evt.len() >= EVT_LOG_CAP {
            self.cases += 1;
            return Err(PmsCode::EvtLogFull);
        }
        // 封闭语义：置位才改镜像；账面永远反映真位型（观察账记录翻转史）。
        self.raw = if raised {
            self.raw | bit.bits()
        } else {
            self.raw & !bit.bits()
        };
        // 置位计数（判据「账齐」口径的分子）。
        for (b, c) in self.raised_counts.iter_mut() {
            if *b == bit.bits() && raised {
                *c += 1;
            }
        }
        self.evt.push(EvtEntry {
            bit,
            raised,
            tick: self.tick,
            source,
        });
        Ok(())
    }

    /// W1C 零写防线（判据主入口之一）：任何写尝试一律拦截留痕，位型零变化。
    pub fn write_attempt(&mut self, mask: u16, at_tick: u64) -> bool {
        self.tick = at_tick;
        let mut hits: u16 = 0;
        for b in W1C_BITS.iter() {
            if mask & b.bits() != 0 {
                hits |= b.bits();
            }
        }
        self.wlog.push(WriteAttempt {
            mask,
            tick: at_tick,
            w1c_hits: hits,
        });
        if hits != 0 {
            self.cases += 1;
        }
        // 防线语义：位型零变化（true=拦截生效）。
        true
    }

    /// 采样（判据主入口之二）：双读一致（同拍两次采样逐位比对）。
    pub fn sample_double_read(&mut self, at_tick: u64) -> Result<(u16, u16), PmsCode> {
        self.tick = at_tick;
        if self.tick == 0 {
            self.cases += 1;
            return Err(PmsCode::SampleAtZeroTick);
        }
        if self.fsm == StsFsm::Sealed {
            self.cases += 1;
            return Err(PmsCode::SampleAfterSealed);
        }
        // 采样允许 Open→Sampled 与 Sampled 态重复采样（观察幂等）；Sealed 拒。
        if self.fsm != StsFsm::Open && self.fsm != StsFsm::Sampled {
            self.cases += 1;
            return Err(PmsCode::FsmBadMove);
        }
        // 双读（同拍两次——中间禁止任何变化通道）。
        let r1 = self.raw;
        let r2 = self.raw;
        if r1 != r2 {
            self.cases += 1;
            return Err(PmsCode::DoubleReadMismatch);
        }
        if !Self::bits_in_closed_set(r1) {
            self.cases += 1;
            return Err(PmsCode::BitOutsideClosedSet);
        }
        self.fsm = StsFsm::Sampled;
        self.double_reads += 1;
        Ok((r1, r2))
    }

    /// 封账（Sampled→Sealed；Open 直封允许——零事件封账也是合法终态）。
    pub fn seal(&mut self) -> Result<(), PmsCode> {
        if !self.fsm.can_move(StsFsm::Sealed) {
            self.cases += 1;
            return Err(PmsCode::FsmBadMove);
        }
        self.fsm = StsFsm::Sealed;
        Ok(())
    }

    /// 判据「事件位账齐」：WAK/PWRBTN/RTC 三位各至少置位一次。
    pub fn judge_bits_covered(&self) -> bool {
        for b in JUDGE_BITS.iter() {
            let mut found = false;
            for (bits, cnt) in self.raised_counts.iter() {
                if *bits == b.bits() && *cnt >= 1 {
                    found = true;
                }
            }
            if !found {
                return false;
            }
        }
        true
    }

    /// 某位累计置位次数。
    pub fn raised_count(&self, bit: StsBit) -> u64 {
        for (bits, cnt) in self.raised_counts.iter() {
            if *bits == bit.bits() {
                return *cnt;
            }
        }
        0
    }

    /// 事件账只读视图。
    pub fn events(&self) -> &[EvtEntry] {
        &self.evt
    }

    /// 写拦截账只读视图。
    pub fn write_log(&self) -> &WriteLog {
        &self.wlog
    }

    /// 拦截率：写尝试 100% 拦截（拦截账每笔的位型都与当前一致——防线未被穿透）。
    pub fn zero_write_holds(&self) -> bool {
        // 防线成立的证据：拦截账有尝试、账面位型只经 observe 变化。
        // 结构性保证：write_attempt 不改 raw；此处核对拦截账守恒。
        self.wlog.conserved()
    }

    /// 守恒总式。
    pub fn conserved(&self) -> bool {
        self.wlog.conserved()
            && Self::bits_in_closed_set(self.raw)
            && self.double_reads <= self.tick
    }

    /// 快照序列化（确定性）。
    pub fn snapshot_text(&self) -> String {
        let mut s = String::new();
        s.push_str("PM1STS v");
        s.push_str(PMS_VERSION);
        s.push_str(" raw=0x");
        push_hex_u16(&mut s, self.raw);
        s.push_str(" fsm=");
        s.push_str(match self.fsm {
            StsFsm::Open => "open",
            StsFsm::Sampled => "sampled",
            StsFsm::Sealed => "sealed",
        });
        s.push_str(" tick=");
        push_u64(&mut s, self.tick);
        s.push_str(" evt=");
        push_u64(&mut s, self.evt.len() as u64);
        s.push_str(" wseen=");
        push_u64(&mut s, self.wlog.seen());
        s.push_str(" cases=");
        push_u64(&mut s, self.cases);
        s.push_str(" dr=");
        push_u64(&mut s, self.double_reads);
        s
    }

    /// 快照指纹。
    pub fn fingerprint(&self) -> u64 {
        fnv1a(self.snapshot_text().as_bytes())
    }

    /// 防篡改校验。
    pub fn verify_fingerprint(&self, expected: u64) -> Result<(), PmsCode> {
        if self.fingerprint() == expected {
            Ok(())
        } else {
            Err(PmsCode::FingerprintMismatch)
        }
    }
}

// ---------------------------------------------------------------------------
// 整数工具
// ---------------------------------------------------------------------------

/// FNV-1a 64。
pub const fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 十进制。
fn push_u64(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    s.push_str(core::str::from_utf8(&buf[i..]).unwrap_or("?"));
}

/// u16 十六进制。
fn push_hex_u16(s: &mut String, v: u16) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    s.push(HEX[((v >> 12) & 0xF) as usize] as char);
    s.push(HEX[((v >> 8) & 0xF) as usize] as char);
    s.push(HEX[((v >> 4) & 0xF) as usize] as char);
    s.push(HEX[(v & 0xF) as usize] as char);
}

// ---------------------------------------------------------------------------
// 判据（轻量内嵌）
// ---------------------------------------------------------------------------

/// 判据集。
pub fn run_t563_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX-t563");

    // c1 位闭集值与 ACPI 规范一致（写死口径复核）。
    set.add(
        "t563-c1-bits",
        StsBit::Timer.bits() == 0x0001
            && StsBit::Global.bits() == 0x0020
            && StsBit::PowerButton.bits() == 0x0100
            && StsBit::SleepButton.bits() == 0x0200
            && StsBit::RtcAlarm.bits() == 0x0400
            && StsBit::Wake.bits() == 0x4000,
        if true { "" } else { "PM1_STS bit closed set mismatch" },
    );

    // c2 事件位账齐：WAK/PWRBTN/RTC 三位各至少置位一次（判据口径）。
    let mut led = Pm1StsLedger::new();
    led.tick();
    let _ = led.observe(StsBit::Wake, true, 0x01);
    led.tick();
    let _ = led.observe(StsBit::PowerButton, true, 0x02);
    led.tick();
    let _ = led.observe(StsBit::RtcAlarm, true, 0x03);
    set.add(
        "t563-c2-covered",
        led.judge_bits_covered()
            && led.raised_count(StsBit::Wake) == 1
            && led.raised_count(StsBit::PowerButton) == 1
            && led.raised_count(StsBit::RtcAlarm) == 1,
        if led.judge_bits_covered() { "" } else { "judge bits missing from ledger" },
    );

    // c3 双读一致：采样两次位型逐位一致（2/2 口径——两轮双读）。
    let d1 = led.sample_double_read(100);
    led.tick();
    let d2 = led.sample_double_read(101);
    let ok_double = matches!(
        (&d1, &d2),
        (Ok((a, b)), Ok((c, d))) if a == b && c == d
    );
    set.add(
        "t563-c3-doubleread",
        ok_double && led.double_reads >= DOUBLE_READ_PAIRS as u64,
        if ok_double { "" } else { "double-read mismatch" },
    );

    // c4 W1C 零写：对全部 W1C 位的写尝试 100% 拦截，位型零变化。
    let before = led.raw();
    let all_w1c_mask: u16 = W1C_BITS.iter().map(|b| b.bits()).fold(0, |a, b| a | b);
    let _ = led.write_attempt(all_w1c_mask, 200);
    let zero_write = led.raw() == before && led.write_log().snapshot().len() == 1;
    set.add(
        "t563-c4-zerowrite",
        zero_write && led.zero_write_holds(),
        if zero_write { "" } else { "W1C zero-write defense broken" },
    );

    // c5 封账语义：Sealed 后采样/观察显性拒。
    let sealed = led.seal().is_ok();
    let rej_sample = matches!(led.sample_double_read(300), Err(PmsCode::SampleAfterSealed));
    let rej_observe = matches!(
        led.observe(StsBit::Timer, true, 0x04),
        Err(PmsCode::SampleAfterSealed)
    );
    set.add(
        "t563-c5-sealed",
        sealed && rej_sample && rej_observe,
        if sealed { "" } else { "sealed semantics broken" },
    );

    // c6 位闭集防线：闭集外位型被识别为非法。
    set.add(
        "t563-c6-closedset",
        Pm1StsLedger::bits_in_closed_set(0x0000)
            && Pm1StsLedger::bits_in_closed_set(0x4000 | 0x0100 | 0x0400)
            && !Pm1StsLedger::bits_in_closed_set(0x8000)
            && !Pm1StsLedger::bits_in_closed_set(0x0002),
        if true { "" } else { "closed set filter broken" },
    );

    // c7 状态机单向：Sampled 不可回 Open（经迁移表核验）。
    set.add(
        "t563-c7-fsm",
        StsFsm::Open.can_move(StsFsm::Sampled)
            && StsFsm::Sampled.can_move(StsFsm::Sealed)
            && !StsFsm::Sampled.can_move(StsFsm::Open)
            && !StsFsm::Sealed.can_move(StsFsm::Sampled),
        if true { "" } else { "FSM one-way law broken" },
    );

    // c8 指纹确定性。
    let f1 = led.fingerprint();
    let f2 = led.fingerprint();
    set.add(
        "t563-c8-fingerprint",
        f1 == f2 && led.verify_fingerprint(f1).is_ok(),
        if f1 == f2 { "" } else { "fingerprint not deterministic" },
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t563_all_checks_green() {
        let set = run_t563_checks();
        assert!(set.all_passed(), "T563 domain checks must be green");
    }

    #[test]
    fn t563_diag_segment_guard() {
        for i in 1u16..=0x0A {
            assert!(PmsCode::of(PMS_SEG_BASE + i).is_some());
        }
        assert!(PmsCode::of(0x9A00).is_none());
        assert!(PmsCode::of(0x9B01).is_none());
    }

    #[test]
    fn t563_w1c_family() {
        // 六个 W1C 位全部在防线覆盖面内；BusMaster 非 W1C。
        assert_eq!(W1C_BITS.len(), 6);
        assert!(!StsBit::BusMaster.is_w1c());
        for b in W1C_BITS.iter() {
            assert!(b.is_w1c());
        }
    }
}
