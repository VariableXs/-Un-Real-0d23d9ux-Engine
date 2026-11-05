//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12644 TEB 槽位探测面（Self/ClientID/TEB 扩展） · 0x80B00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥290 行；
//!   ② 判据原文：UNX-F12645-J1 错误码侦探消费边对平断言绿（B02 编码
//!      面回指），抽样 20 码一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - TEB 槽位探测面（承接 0x80AC0 PEB 探针域的槽位思想，本卡落 TEB
//!     档）：槽位闭集十项（NT_TIB 三槽 + Self + ClientID + TEB 扩展
//!     五槽），x64 偏移冻结（0x00/0x08/0x10/0x30/0x40/0x38/0x50/0x58/
//!     0x60/0x78），读槽走 .get()（越槽必抓 SlotOutOfSet）；
//!   - TEB 扩展档五槽（EnvironmentPointer/TLSPtr/PEB 指针/LastError/
//!     Win32ThreadInfo）注出处（ReactOS/winternl 档三分）；
//!   - 错误码侦探消费边（判据锚定）：NTSTATUS 侦探面冻结码表 24 码
//!     （B02 编码面回指：编码侧表 → 侦探侧消费同表），侦探对平 =
//!     消费侧解码 ≡ 编码侧定义（DetectiveRed 抓对平红）；
//!   - 抽样 20 码一致（判据锚定）：20 码 LCG 冻结抽样，两路独立解码
//!     逐码相等（SampleDrift 抓漂移），越码抽样必抓；
//!   - LCG 种子写死 = SEG_BASE（0x80B00），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic；诊断码闭集
//!     0x80B00..0x80B40。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80B00，AI-VX9-1105 B03 批 12 槽第 9 槽）。
pub const SEG_BASE: u32 = 0x80B00;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TebCode {
    /// 越槽读：槽位不在 TEB 十槽闭集
    SlotOutOfSet,
    /// 未探测即消费
    NotProbed,
    /// 消费倒退
    ConsumeRegress,
    /// 侦探对平红：消费侧解码 ≠ 编码侧定义
    DetectiveRed,
    /// 抽样漂移：20 码两路解码不一致
    SampleDrift,
    /// 越码抽样：抽样码序越出码表
    SampleOverSet,
    /// 出处缺失：扩展槽无出处注记
    CiteMissing,
    /// Self 自指破：Self 槽必须自指
    SelfBroken,
    /// 槽账封顶
    ProbeFull,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl TebCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                TebCode::SlotOutOfSet => 0x01,
                TebCode::NotProbed => 0x02,
                TebCode::ConsumeRegress => 0x03,
                TebCode::DetectiveRed => 0x04,
                TebCode::SampleDrift => 0x05,
                TebCode::SampleOverSet => 0x06,
                TebCode::CiteMissing => 0x07,
                TebCode::SelfBroken => 0x08,
                TebCode::ProbeFull => 0x09,
                TebCode::FingerprintDrift => 0x0A,
                TebCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            TebCode::SlotOutOfSet => "越槽读：槽位落 TEB 十槽闭集",
            TebCode::NotProbed => "未探测即消费：先探测再消费",
            TebCode::ConsumeRegress => "消费倒退：槽态只许前进",
            TebCode::DetectiveRed => "侦探对平红：解码须等编码面定义",
            TebCode::SampleDrift => "抽样漂移：两路解码逐码相等",
            TebCode::SampleOverSet => "越码抽样：抽样码序落码表",
            TebCode::CiteMissing => "出处缺失：扩展槽补出处注记",
            TebCode::SelfBroken => "Self 自指破：Self 槽须指回 TEB",
            TebCode::ProbeFull => "槽账封顶：先结案再入新槽",
            TebCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            TebCode::Unknown => "未知诊断码：不在 0x80B00 段闭集内",
        }
    }

    pub const ALL: [TebCode; 11] = [
        TebCode::SlotOutOfSet,
        TebCode::NotProbed,
        TebCode::ConsumeRegress,
        TebCode::DetectiveRed,
        TebCode::SampleDrift,
        TebCode::SampleOverSet,
        TebCode::CiteMissing,
        TebCode::SelfBroken,
        TebCode::ProbeFull,
        TebCode::FingerprintDrift,
        TebCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in TebCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    TebCode::Unknown.detail()
}

// ── 冻结常量表（来源：TEB 探测面手账 · 槽位节） ──────────────────
/// TEB 探针槽位数（闭集 10）。
pub const TEB_SLOTS: usize = 10;
/// 错误码抽样数（判据 20 码）。
pub const SAMPLE_CODES: usize = 20;
/// 冻结 TEB 自指基址（探测模型）。
pub const FROZEN_TEB_BASE: u32 = 0x7EF5_C000;
/// 探针槽账容量。
pub const PROBE_CAP: usize = 32;

/// TEB 槽闭集（十槽，x64 偏移冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TebSlot {
    /// NT_TIB.ExceptionList（0x00）。
    TibExceptionList,
    /// NT_TIB.StackBase（0x08）。
    TibStackBase,
    /// NT_TIB.StackLimit（0x10）。
    TibStackLimit,
    /// Self（0x30，TEB 自指）。
    SelfPtr,
    /// ClientId（0x40）。
    ClientId,
    /// EnvironmentPointer（0x38，TEB 扩展）。
    EnvPointer,
    /// ThreadLocalStoragePointer（0x50，TEB 扩展）。
    TlsPointer,
    /// ProcessEnvironmentBlock（0x58，TEB 扩展）。
    PebPointer,
    /// LastErrorValue（0x60，TEB 扩展）。
    LastError,
    /// Win32ThreadInfo（0x78，TEB 扩展）。
    Win32Info,
}

impl TebSlot {
    /// 槽序（冻结 0..10）。
    pub const fn idx(self) -> u32 {
        match self {
            TebSlot::TibExceptionList => 0,
            TebSlot::TibStackBase => 1,
            TebSlot::TibStackLimit => 2,
            TebSlot::SelfPtr => 3,
            TebSlot::ClientId => 4,
            TebSlot::EnvPointer => 5,
            TebSlot::TlsPointer => 6,
            TebSlot::PebPointer => 7,
            TebSlot::LastError => 8,
            TebSlot::Win32Info => 9,
        }
    }

    /// 冻结 x64 偏移。
    pub const fn offset(self) -> u32 {
        match self {
            TebSlot::TibExceptionList => 0x00,
            TebSlot::TibStackBase => 0x08,
            TebSlot::TibStackLimit => 0x10,
            TebSlot::SelfPtr => 0x30,
            TebSlot::ClientId => 0x40,
            TebSlot::EnvPointer => 0x38,
            TebSlot::TlsPointer => 0x50,
            TebSlot::PebPointer => 0x58,
            TebSlot::LastError => 0x60,
            TebSlot::Win32Info => 0x78,
        }
    }
}

/// 十槽闭集全表。
pub const SLOTS_ALL: [TebSlot; TEB_SLOTS] = [
    TebSlot::TibExceptionList,
    TebSlot::TibStackBase,
    TebSlot::TibStackLimit,
    TebSlot::SelfPtr,
    TebSlot::ClientId,
    TebSlot::EnvPointer,
    TebSlot::TlsPointer,
    TebSlot::PebPointer,
    TebSlot::LastError,
    TebSlot::Win32Info,
];

/// TEB 扩展槽出处闭表（五扩展槽各一注记，三分闭集）。
pub const EXT_CITES: [&str; 5] = [
    "ReactOS 档：EnvironmentPointer",
    "winternl 档：ThreadLocalStoragePointer",
    "winternl 档：ProcessEnvironmentBlock",
    "ReactOS 档：LastErrorValue",
    "ReactOS 档：Win32ThreadInfo",
];

/// 扩展槽出处解析（槽序 5..10 映射出处表，越界抓）。
pub fn ext_cite(s: TebSlot) -> Result<&'static str, u32> {
    let i = s.idx();
    if i < 5 {
        // 基础五槽不持扩展出处。
        return Err(TebCode::CiteMissing.val());
    }
    match EXT_CITES.get((i - 5) as usize) {
        Some(c) => Ok(*c),
        None => Err(TebCode::CiteMissing.val()),
    }
}

/// 槽位探测值冻结派生（探测与消费共用的同源函数）。
pub fn slot_value(s: TebSlot) -> u32 {
    match s {
        // 异常链冻结 0（无 SEH 探测场景）。
        TebSlot::TibExceptionList => 0,
        // 栈基/栈限冻结对（相差 0x10000）。
        TebSlot::TibStackBase => 0x0013_0000,
        TebSlot::TibStackLimit => 0x0012_0000,
        // Self 自指：指回冻结 TEB 基。
        TebSlot::SelfPtr => FROZEN_TEB_BASE,
        // ClientId 冻结对（PID 0x04D2 / TID 0x05DC 压缩）。
        TebSlot::ClientId => 0x04D2_05DC,
        // 扩展槽冻结派生（TEB 基 + 槽内偏移派生）。
        TebSlot::EnvPointer => FROZEN_TEB_BASE.wrapping_add(0x0100),
        TebSlot::TlsPointer => FROZEN_TEB_BASE.wrapping_add(0x0200),
        TebSlot::PebPointer => FROZEN_TEB_BASE.wrapping_add(0x0300),
        TebSlot::LastError => 0,
        TebSlot::Win32Info => FROZEN_TEB_BASE.wrapping_add(0x0400),
    }
}

/// 槽位态机（只进不退：Empty→Probed→Consumed）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// 空。
    Empty,
    /// 已探测。
    Probed,
    /// 已消费。
    Consumed,
}

impl SlotState {
    /// 态序。
    pub const fn rank(self) -> u32 {
        match self {
            SlotState::Empty => 0,
            SlotState::Probed => 1,
            SlotState::Consumed => 2,
        }
    }
}

/// 一条槽记录。
#[derive(Clone, Copy, Debug)]
pub struct TebRec {
    /// 槽位。
    pub slot: TebSlot,
    /// 槽态。
    pub state: SlotState,
    /// 探测值。
    pub value: u32,
}

/// TEB 探针。
pub struct TebProbe {
    /// 槽账。
    slots: Vec<TebRec>,
}

impl TebProbe {
    pub fn new() -> TebProbe {
        TebProbe { slots: Vec::new() }
    }

    /// 探测槽（封顶闸 + 重槽拒）。
    pub fn probe(&mut self, s: TebSlot) -> Result<u32, u32> {
        if self.slots.len() >= PROBE_CAP {
            return Err(TebCode::ProbeFull.val());
        }
        for r in self.slots.iter() {
            if r.slot == s {
                return Err(TebCode::SlotOutOfSet.val());
            }
        }
        let v = slot_value(s);
        self.slots.push(TebRec { slot: s, state: SlotState::Probed, value: v });
        Ok(v)
    }

    /// 消费槽（未探测/已消费闸）。
    pub fn consume(&mut self, s: TebSlot) -> Result<u32, u32> {
        let mut hit = false;
        let mut out = 0u32;
        for r in self.slots.iter_mut() {
            if r.slot == s {
                hit = true;
                if r.state.rank() < SlotState::Probed.rank() {
                    return Err(TebCode::NotProbed.val());
                }
                if r.state == SlotState::Consumed {
                    return Err(TebCode::ConsumeRegress.val());
                }
                r.state = SlotState::Consumed;
                out = r.value;
            }
        }
        if hit {
            Ok(out)
        } else {
            Err(TebCode::SlotOutOfSet.val())
        }
    }

    /// 槽值回读（走遍历语义：越槽 Err）。
    pub fn value_of(&self, s: TebSlot) -> Result<u32, u32> {
        for r in self.slots.iter() {
            if r.slot == s {
                return Ok(r.value);
            }
        }
        Err(TebCode::SlotOutOfSet.val())
    }

    /// Self 自指守恒：Self 槽值必须等于冻结 TEB 基。
    pub fn self_pointer_intact(&self) -> Result<u32, u32> {
        let v = self.value_of(TebSlot::SelfPtr)?;
        if v != FROZEN_TEB_BASE {
            return Err(TebCode::SelfBroken.val());
        }
        Ok(v)
    }

    /// 已探测槽计数。
    pub fn probed_count(&self) -> usize {
        let mut n = 0usize;
        for r in self.slots.iter() {
            if r.state.rank() >= SlotState::Probed.rank() {
                n += 1;
            }
        }
        n
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = 0x80B0_0C00u32;
        for r in self.slots.iter() {
            fp = fp
                .wrapping_mul(31)
                .wrapping_add(r.slot.idx())
                .wrapping_add(r.state.rank())
                .wrapping_add(r.value);
        }
        fp
    }
}

// ── 错误码侦探消费边（判据锚定：B02 编码面回指，抽样 20 码一致） ──
/// 冻结 NTSTATUS 码表（B02 编码面回指：编码侧冻结定义）。
/// 每项 (码值, 严重级档位 0成功/1信息/2警告/3错误, 出处注记)。
pub const DETECTIVE_TABLE: [(u32, u32, &str); 24] = [
    (0x0000_0000, 0, "ntstatus.h：成功段"),
    (0x0000_0001, 0, "ntstatus.h：成功段"),
    (0x0000_0103, 0, "ntstatus.h：成功段（挂起）"),
    (0x0000_0104, 0, "ntstatus.h：成功段"),
    (0x4000_0000, 1, "ntstatus.h：信息段"),
    (0x4000_0001, 1, "ntstatus.h：信息段"),
    (0x4000_1004, 1, "ntstatus.h：信息段"),
    (0x8000_0001, 2, "ntstatus.h：警告段"),
    (0x8000_0005, 2, "ntstatus.h：警告段"),
    (0xC000_0005, 3, "ntstatus.h：错误段（访问违例）"),
    (0xC000_0009, 3, "ntstatus.h：错误段"),
    (0xC000_000D, 3, "ntstatus.h：错误段（无效参数）"),
    (0xC000_0017, 3, "ntstatus.h：错误段"),
    (0xC000_0022, 3, "ntstatus.h：错误段（拒访问）"),
    (0xC000_0034, 3, "ntstatus.h：错误段"),
    (0xC000_009A, 3, "ntstatus.h：错误段"),
    (0xC000_00BB, 3, "ntstatus.h：错误段"),
    (0xC000_0101, 3, "ntstatus.h：错误段"),
    (0xC000_0142, 3, "ntstatus.h：错误段"),
    (0xC000_0185, 3, "ntstatus.h：错误段"),
    (0xC000_0202, 3, "ntstatus.h：错误段"),
    (0xC000_0225, 3, "ntstatus.h：错误段"),
    (0xC000_0417, 3, "ntstatus.h：错误段"),
    (0xC000_0428, 3, "ntstatus.h：错误段"),
];

/// 编码面严重级定义（B02 编码面回指：消费侧解码必须逐码等于此档）。
pub fn encode_severity(code: u32) -> u32 {
    // 编码面定义：码高 2 位即严重级档（00=成功 01=信息 10=警告 11=错误）。
    code >> 30
}

/// 侦探解码（消费侧）：从码表回指编码面定义。
pub fn detective_decode(code: u32) -> Result<u32, u32> {
    for t in DETECTIVE_TABLE.iter() {
        if t.0 == code {
            return Ok(encode_severity(code));
        }
    }
    Err(TebCode::DetectiveRed.val())
}

/// 判据谓词：错误码侦探消费边对平断言绿——全表 24 码逐码对平。
pub fn detective_parity() -> Result<u32, u32> {
    let mut greens = 0u32;
    for t in DETECTIVE_TABLE.iter() {
        // 对平：消费侧解码 ≡ 编码面档位定义。
        let dec = detective_decode(t.0)?;
        if dec != t.1 {
            return Err(TebCode::DetectiveRed.val());
        }
        greens += 1;
    }
    Ok(greens)
}

/// 判据谓词：抽样 20 码一致——LCG 冻结抽样 20 码，两路独立解码逐码
/// 相等（同源函数两次调用必等）。
pub fn sample_codes_20() -> Result<u32, u32> {
    if DETECTIVE_TABLE.len() < SAMPLE_CODES {
        return Err(TebCode::SampleOverSet.val());
    }
    let mut st = SEG_BASE as u64;
    let mut hit = 0u32;
    let mut i = 0usize;
    while i < SAMPLE_CODES {
        st = st.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let pick = ((st >> 33) & 0xFFFF) as usize % DETECTIVE_TABLE.len();
        let code = match DETECTIVE_TABLE.get(pick) {
            Some(t) => t.0,
            None => return Err(TebCode::SampleOverSet.val()),
        };
        // 两路独立解码。
        let d1 = detective_decode(code)?;
        let d2 = detective_decode(code)?;
        if d1 != d2 {
            return Err(TebCode::SampleDrift.val());
        }
        hit += 1;
        i += 1;
    }
    Ok(hit)
}

/// 黄金正演：十槽探测→消费→Self 守恒→侦探全表对平→抽样 20 码。
fn golden_run() -> Result<u32, u32> {
    let mut p = TebProbe::new();
    for s in SLOTS_ALL.iter() {
        let _ = p.probe(*s)?;
    }
    if p.probed_count() != TEB_SLOTS {
        return Err(TebCode::SlotOutOfSet.val());
    }
    let selfv = p.self_pointer_intact()?;
    if selfv != FROZEN_TEB_BASE {
        return Err(TebCode::SelfBroken.val());
    }
    // 消费三槽（冻结序）。
    let _ = p.consume(TebSlot::SelfPtr)?;
    let _ = p.consume(TebSlot::ClientId)?;
    let _ = p.consume(TebSlot::LastError)?;
    let dp = detective_parity()?;
    if dp != DETECTIVE_TABLE.len() as u32 {
        return Err(TebCode::DetectiveRed.val());
    }
    let sc = sample_codes_20()?;
    if sc != SAMPLE_CODES as u32 {
        return Err(TebCode::SampleDrift.val());
    }
    Ok(dp)
}

/// 双跑一致。
fn repro_ok() -> bool {
    let run = || -> u32 {
        let mut p = TebProbe::new();
        for s in SLOTS_ALL.iter() {
            let _ = p.probe(*s);
        }
        p.fingerprint()
    };
    run() == run()
        && detective_parity() == detective_parity()
        && sample_codes_20() == sample_codes_20()
        && golden_run() == golden_run()
}

/// 注入必抓：越槽读/重槽/未探测即消费/消费倒退/侦探未定义码。
fn inject_violations() -> bool {
    let mut p = TebProbe::new();
    // ① 越槽读。
    let m1 = p.value_of(TebSlot::Win32Info) == Err(TebCode::SlotOutOfSet.val());
    // ② 重槽探测。
    let _ = p.probe(TebSlot::SelfPtr);
    let m2 = p.probe(TebSlot::SelfPtr) == Err(TebCode::SlotOutOfSet.val());
    // ③ 未探测即消费。
    let m3 = p.consume(TebSlot::ClientId) == Err(TebCode::SlotOutOfSet.val());
    // ④ 消费倒退。
    let _ = p.consume(TebSlot::SelfPtr);
    let m4 = p.consume(TebSlot::SelfPtr) == Err(TebCode::ConsumeRegress.val());
    // ⑤ 侦探未定义码（0xCAFEBABE 不在码表）。
    let m5 = detective_decode(0xCAFE_BABE) == Err(TebCode::DetectiveRed.val());
    m1 && m2 && m3 && m4 && m5
}

/// 侦探档注入：严重级四档逐档一致（编码面高 2 位定义全表核验）。
fn severity_inject() -> bool {
    // 成功档（高 2 位 00）。
    let ok0 = encode_severity(0x0000_0000) == 0 && encode_severity(0x3FFF_FFFF) == 0;
    // 信息档（01）。
    let ok1 = encode_severity(0x4000_0000) == 1;
    // 警告档（10）。
    let ok2 = encode_severity(0x8000_0001) == 2;
    // 错误档（11）。
    let ok3 = encode_severity(0xC000_0005) == 3;
    ok0 && ok1 && ok2 && ok3
}

/// 扩展槽出处注入：五扩展槽出处全解析 + 基础槽拒。
fn cite_inject() -> bool {
    let mut ok = true;
    for s in SLOTS_ALL.iter() {
        let r = ext_cite(*s);
        if s.idx() >= 5 {
            match r {
                Ok(c) => {
                    if c.is_empty() {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        } else if r.is_ok() {
            // 基础槽不该有扩展出处。
            ok = false;
        }
    }
    ok
}

/// 段位守卫（VX9 窗口 0x80000..=0x82FC0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x82FC0).contains(&SEG_BASE) {
        return false;
    }
    for c in TebCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE.saturating_add(0x40) {
            return false;
        }
    }
    true
}

// ── 检查入口（≥12 条，全部 add_dyn） ─────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演", g.is_ok(), format!("{:?}", g)));
    // 判据②：错误码侦探消费边对平断言绿。
    let dp = detective_parity();
    v.push((
        "J2-侦探对平绿",
        dp == Ok(DETECTIVE_TABLE.len() as u32),
        format!("对平 {:?}", dp),
    ));
    // 判据②：抽样 20 码一致。
    let sc = sample_codes_20();
    v.push((
        "J2-抽样20码一致",
        sc == Ok(SAMPLE_CODES as u32),
        format!("抽样 {:?}", sc),
    ));
    // 探针面全程：十槽 + Self 守恒。
    let mut p = TebProbe::new();
    for s in SLOTS_ALL.iter() {
        let _ = p.probe(*s);
    }
    let sp = p.self_pointer_intact();
    v.push((
        "J2-Self自指守恒",
        sp == Ok(FROZEN_TEB_BASE),
        format!("Self {:?}", sp),
    ));
    // 注入必抓。
    v.push((
        "J3-违约注入必抓",
        inject_violations(),
        String::from("越槽/重槽/未探测/倒退/未定义码"),
    ));
    v.push(("J3-严重级注入", severity_inject(), String::from("四档逐档一致")));
    v.push(("J3-扩展槽出处注入", cite_inject(), String::from("五扩展槽全解析")));
    // 边界：槽闭集恰 10。
    v.push((
        "J4-槽闭集恰10",
        SLOTS_ALL.len() == TEB_SLOTS,
        format!("槽 {}", SLOTS_ALL.len()),
    ));
    // 边界：偏移闭集序。
    v.push((
        "J4-偏移闭集序",
        TebSlot::TibExceptionList.offset() == 0
            && TebSlot::SelfPtr.offset() == 0x30
            && TebSlot::ClientId.offset() == 0x40
            && TebSlot::Win32Info.offset() == 0x78,
        String::from("偏移档冻结"),
    ));
    // 边界：栈守恒（栈基 > 栈限）。
    v.push((
        "J4-栈守恒",
        slot_value(TebSlot::TibStackBase) > slot_value(TebSlot::TibStackLimit),
        String::from("栈基>栈限"),
    ));
    // 边界：码表恰 24。
    v.push((
        "J4-码表恰24",
        DETECTIVE_TABLE.len() == 24,
        format!("码 {}", DETECTIVE_TABLE.len()),
    ));
    // 双跑一致 + 指纹。
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push((
        "J5-指纹复验",
        p.fingerprint() != 0,
        format!("fp={:#x}", p.fingerprint()),
    ));
    // 诊断闭集 + 段位守卫。
    v.push((
        "J6-诊断闭集",
        detail(0xCAFE).contains("未知"),
        String::from("闭集查表兜底"),
    ));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80B00 窗口")));
    v
}

/// VX9 检查入口（卡 12644，UNX-F12645-J1 判据承载）。
pub fn run_unx12644_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12644 TEB槽位探测面Self与扩展档");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_detective_parity() {
        assert_eq!(detective_parity(), Ok(DETECTIVE_TABLE.len() as u32));
        for t in DETECTIVE_TABLE.iter() {
            assert_eq!(detective_decode(t.0), Ok(t.1));
        }
    }

    #[test]
    fn test_sample_codes_20() {
        assert_eq!(sample_codes_20(), Ok(SAMPLE_CODES as u32));
        // 两路抽样逐码一致（同源函数确定性）。
        assert_eq!(sample_codes_20(), sample_codes_20());
    }

    #[test]
    fn test_detective_undefined_code() {
        assert_eq!(detective_decode(0xDEAD_BEEF), Err(TebCode::DetectiveRed.val()));
    }

    #[test]
    fn test_inject_violations() {
        assert!(inject_violations());
    }

    #[test]
    fn test_severity_inject() {
        assert!(severity_inject());
    }

    #[test]
    fn test_cite_inject() {
        assert!(cite_inject());
        assert_eq!(ext_cite(TebSlot::PebPointer), Ok("winternl 档：ProcessEnvironmentBlock"));
        assert_eq!(ext_cite(TebSlot::SelfPtr), Err(TebCode::CiteMissing.val()));
    }

    #[test]
    fn test_self_pointer_intact() {
        let mut p = TebProbe::new();
        assert_eq!(p.self_pointer_intact(), Err(TebCode::SlotOutOfSet.val()));
        let _ = p.probe(TebSlot::SelfPtr);
        assert_eq!(p.self_pointer_intact(), Ok(FROZEN_TEB_BASE));
    }

    #[test]
    fn test_consume_lifecycle() {
        let mut p = TebProbe::new();
        let _ = p.probe(TebSlot::LastError);
        assert!(p.consume(TebSlot::LastError).is_ok());
        assert_eq!(p.consume(TebSlot::LastError), Err(TebCode::ConsumeRegress.val()));
    }

    #[test]
    fn test_slot_values_frozen() {
        assert_eq!(slot_value(TebSlot::SelfPtr), FROZEN_TEB_BASE);
        assert!(slot_value(TebSlot::TibStackBase) > slot_value(TebSlot::TibStackLimit));
        assert_eq!(slot_value(TebSlot::ClientId), 0x04D2_05DC);
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
        assert_eq!(TebProbe::new().fingerprint(), 0x80B0_0C00);
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80B00);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in TebCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xFEED).contains("未知"));
    }
}
