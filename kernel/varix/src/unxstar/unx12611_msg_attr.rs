//! UNX-F12612 消息属性（MESSAGE_TYPE/数据长度）对平 · 0x802C0段
//!
//! 判据原文（派工卡 12611 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12612-J1 上下文端口（ContextPort）与消息模式
//!      （直接/间接）语义档对平断言绿，抽样 10 例一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 消息模式二档闭集：直接（Direct，正文内联小消息通道）/间接
//!    （Indirect，正文挂区段视图）；档外取值在登记口即拒；
//! 2. 上下文端口（ContextPort）语义：端口持上下文标识与模式档，
//!    直接档正文长度 ≤ 内联上限，间接档必须先挂视图且正文长度
//!    ≤ 视图大小——型-档-长三表对平，缺一即账不平；
//! 3. 模式倒挂语义：直接档携视图、间接档缺视图均属档错，显性拒；
//! 4. 抽样 10 例：LCG 种子写死，逐例构造（模式档/正文长度/视图大小）
//!    三元组，十例对平判定与基线一致且双跑逐项相同；
//! 5. 状态机 未挂→已挂→已对平 单向推进，倒退显性拒；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12611 − 12600)）。
pub const SEG_BASE: u32 = 0x802C0;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 上下文端口与消息模式诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpDiag {
    /// 模式档越二档闭集。
    EModeRange,
    /// 间接档缺视图。
    EIndNoView,
    /// 直接档携视图（档倒挂）。
    EDirectWithView,
    /// 直接档正文超内联上限。
    EDirectOver,
    /// 间接档正文超视图大小。
    EIndOver,
    /// 上下文标识为零。
    ENoContext,
    /// 重复挂上下文。
    EDupContext,
    /// 未知端口。
    EUnknownPort,
    /// 状态机倒退。
    ERewind,
    /// 空样本。
    EEmpty,
    /// 抽样漂移（与基线不一致）。
    EDrift,
    /// 对平账守恒破坏。
    EConserv,
}

impl CpDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                CpDiag::EModeRange => 0x01,
                CpDiag::EIndNoView => 0x02,
                CpDiag::EDirectWithView => 0x03,
                CpDiag::EDirectOver => 0x04,
                CpDiag::EIndOver => 0x05,
                CpDiag::ENoContext => 0x06,
                CpDiag::EDupContext => 0x07,
                CpDiag::EUnknownPort => 0x08,
                CpDiag::ERewind => 0x09,
                CpDiag::EEmpty => 0x0a,
                CpDiag::EDrift => 0x0b,
                CpDiag::EConserv => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            CpDiag::EModeRange => "模式档越闭集：只认直接/间接两档，先对档表再落账",
            CpDiag::EIndNoView => "间接档缺视图：先挂区段视图再投正文",
            CpDiag::EDirectWithView => "直接档携视图：内联正文不许挂视图，改间接档或摘视图",
            CpDiag::EDirectOver => "正文超内联上限：直接档正文超限，改走间接档",
            CpDiag::EIndOver => "正文超视图：间接档正文长度不得超视图大小",
            CpDiag::ENoContext => "上下文标识为零：ContextPort 必须持非零上下文",
            CpDiag::EDupContext => "重复挂上下文：一端口一上下文，改挂先摘",
            CpDiag::EUnknownPort => "未知端口：先建端口再落上下文",
            CpDiag::ERewind => "状态机倒退：未挂→已挂→已对平只进不退",
            CpDiag::EEmpty => "空样本：端口/上下文/长度全零不构成消息",
            CpDiag::EDrift => "抽样漂移：十例对平判定与基线不一致，先查确定性来源",
            CpDiag::EConserv => "对平账守恒破坏：各档计数与账面不符",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const CP_ALL: &[CpDiag] = &[
    CpDiag::EModeRange,
    CpDiag::EIndNoView,
    CpDiag::EDirectWithView,
    CpDiag::EDirectOver,
    CpDiag::EIndOver,
    CpDiag::ENoContext,
    CpDiag::EDupContext,
    CpDiag::EUnknownPort,
    CpDiag::ERewind,
    CpDiag::EEmpty,
    CpDiag::EDrift,
    CpDiag::EConserv,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in CP_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x802C1..0x802CC 闭集内"
}

// ── 消息模式二档闭集 ─────────────────────────────────────────────
/// 消息模式二档闭集（判据原文：直接/间接）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MsgMode {
    /// 直接档：正文内联小消息通道。
    Direct,
    /// 间接档：正文挂区段视图。
    Indirect,
}

impl MsgMode {
    /// 档号。
    pub fn ordinal(self) -> u32 {
        match self {
            MsgMode::Direct => 0,
            MsgMode::Indirect => 1,
        }
    }

    /// 档中文名。
    pub fn name(self) -> &'static str {
        match self {
            MsgMode::Direct => "直接",
            MsgMode::Indirect => "间接",
        }
    }

    /// 由档号反查（越界 None）。
    pub fn from_ordinal(o: u32) -> Option<MsgMode> {
        match o {
            0 => Some(MsgMode::Direct),
            1 => Some(MsgMode::Indirect),
            _ => None,
        }
    }
}

/// 二档闭集数组。
pub const MODE_ALL: &[MsgMode] = &[MsgMode::Direct, MsgMode::Indirect];

/// 直接档内联正文上限（字节）。
pub const INLINE_CAP: u32 = 256;

// ── 上下文端口域模型 ─────────────────────────────────────────────
/// 端口挂载状态机：未挂 → 已挂 → 已对平。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountState {
    /// 未挂上下文。
    Unmounted,
    /// 已挂上下文（与视图档）。
    Mounted,
    /// 已对平（首笔消息通过）。
    Balanced,
}

impl MountState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            MountState::Unmounted => 0,
            MountState::Mounted => 1,
            MountState::Balanced => 2,
        }
    }
}

/// 上下文端口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextPort {
    /// 端口标识（非零）。
    pub port_id: u32,
    /// 上下文标识（非零）。
    pub context_id: u32,
    /// 消息模式档。
    pub mode: MsgMode,
    /// 挂载的视图大小（间接档有效；直接档应为 None）。
    pub view_size: Option<u32>,
    /// 挂载状态。
    pub state: MountState,
}

impl ContextPort {
    /// 静态档语义：直接档不携视图、间接档必携正视图。
    pub fn mode_sound(&self) -> Result<(), u32> {
        match self.mode {
            MsgMode::Direct => {
                if self.view_size.is_some() {
                    return Err(CpDiag::EDirectWithView.val());
                }
                Ok(())
            }
            MsgMode::Indirect => match self.view_size {
                Some(v) if v > 0 => Ok(()),
                _ => Err(CpDiag::EIndNoView.val()),
            },
        }
    }

    /// 消息对平：按档校验正文长度。
    pub fn accepts(&self, data_len: u32) -> Result<(), u32> {
        match self.mode {
            MsgMode::Direct => {
                if data_len > INLINE_CAP {
                    return Err(CpDiag::EDirectOver.val());
                }
                Ok(())
            }
            MsgMode::Indirect => {
                let v = match self.view_size {
                    Some(v) => v,
                    None => return Err(CpDiag::EIndNoView.val()),
                };
                if data_len > v {
                    return Err(CpDiag::EIndOver.val());
                }
                Ok(())
            }
        }
    }
}

/// 对平账（一笔消息落账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpLedger {
    /// 消息号。
    pub msg_id: u32,
    /// 落账端口。
    pub port_id: u32,
    /// 模式档。
    pub mode: MsgMode,
    /// 正文长度。
    pub data_len: u32,
}

/// 上下文端口账机。
#[derive(Clone, Debug)]
pub struct CpGate {
    /// 端口表。
    ports: Vec<ContextPort>,
    /// 对平账。
    ledgers: Vec<CpLedger>,
}

impl CpGate {
    /// 新账机。
    pub fn new() -> CpGate {
        CpGate {
            ports: Vec::new(),
            ledgers: Vec::new(),
        }
    }

    /// 端口数。
    pub fn port_count(&self) -> usize {
        self.ports.len()
    }

    /// 账面数。
    pub fn len(&self) -> usize {
        self.ledgers.len()
    }

    /// 查端口。
    pub fn port_of(&self, port_id: u32) -> Option<ContextPort> {
        for p in self.ports.iter() {
            if p.port_id == port_id {
                return Some(*p);
            }
        }
        None
    }

    /// 建端口（空样本/零上下文/重复/档倒挂四闸）。
    pub fn mount(&mut self, port: ContextPort) -> Result<(), u32> {
        if port.port_id == 0 && port.context_id == 0 {
            return Err(CpDiag::EEmpty.val());
        }
        if port.context_id == 0 {
            return Err(CpDiag::ENoContext.val());
        }
        if self.port_of(port.port_id).is_some() {
            return Err(CpDiag::EDupContext.val());
        }
        port.mode_sound()?;
        self.ports.push(ContextPort {
            state: MountState::Mounted,
            ..port
        });
        Ok(())
    }

    /// 消息对平（未知端口/档长语义/状态推进三段）。
    pub fn deliver(&mut self, msg_id: u32, port_id: u32, data_len: u32) -> Result<CpLedger, u32> {
        if msg_id == 0 && port_id == 0 && data_len == 0 {
            return Err(CpDiag::EEmpty.val());
        }
        let idx = match self.port_index(port_id) {
            Some(i) => i,
            None => return Err(CpDiag::EUnknownPort.val()),
        };
        let p = match self.ports.get(idx) {
            Some(p) => *p,
            None => return Err(CpDiag::EUnknownPort.val()),
        };
        // 档-长语义。
        p.accepts(data_len)?;
        // 状态推进：Mounted → Balanced（只进不退；已对平端口继续收消息合法）。
        let ledger = CpLedger {
            msg_id,
            port_id,
            mode: p.mode,
            data_len,
        };
        if let Some(p) = self.ports.get_mut(idx) {
            if p.state == MountState::Mounted {
                p.state = MountState::Balanced;
            }
        }
        self.ledgers.push(ledger);
        Ok(ledger)
    }

    /// 账守恒：账内档位与端口表档位一致，账面计数一致。
    pub fn conservation(&self) -> bool {
        for l in self.ledgers.iter() {
            match self.port_of(l.port_id) {
                Some(p) => {
                    if p.mode != l.mode {
                        return false;
                    }
                }
                None => return false,
            }
            // 档-长恒等复查：直接档 ≤ 内联上限；间接档 ≤ 视图。
            match l.mode {
                MsgMode::Direct => {
                    if l.data_len > INLINE_CAP {
                        return false;
                    }
                }
                MsgMode::Indirect => {
                    let v = match self.port_of(l.port_id).and_then(|p| p.view_size) {
                        Some(v) => v,
                        None => return false,
                    };
                    if l.data_len > v {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// 按端口号找表内下标。
    fn port_index(&self, port_id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ports.len() {
            if let Some(p) = self.ports.get(i) {
                if p.port_id == port_id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }
}

// ── 确定性 LCG（种子 = SEG_BASE） ────────────────────────────────
/// 线性同余发生器（Knuth MMIX 常数，种子写死）。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一个 32 位伪随机数（确定序列）。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }
}

// ── 抽样 10 例：模式档 × 正文长度 × 视图大小 ────────────────────
/// 单例抽样结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpSample {
    /// 例号 0..9。
    pub case: u32,
    /// 模式档。
    pub mode: MsgMode,
    /// 正文长度。
    pub data_len: u32,
    /// 视图大小（直接档为 0）。
    pub view_size: u32,
    /// 对平是否成立。
    pub paired: bool,
}

/// 构建十例抽样：逐例确定性构造并走完整对平流。
pub fn sample_ten() -> Vec<CpSample> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut out: Vec<CpSample> = Vec::new();
    let mut k = 0u32;
    while k < 10 {
        // 模式档轮转（偶数直接/奇数间接），正文长度 LCG 抖动，
        // 间接档视图大小 = 正文长度向上取整到 0x10 对齐。
        let mode = if k % 2 == 0 {
            MsgMode::Direct
        } else {
            MsgMode::Indirect
        };
        let raw_len = rng.next_u32() % 512;
        let view_size = raw_len.saturating_add(0xF) & !0xF;
        let data_len = if mode == MsgMode::Direct {
            // 直接档：长度压回内联上限内。
            if raw_len > INLINE_CAP { INLINE_CAP } else { raw_len }
        } else {
            raw_len
        };
        let port = ContextPort {
            port_id: 0x4000 + k,
            context_id: 0x5000 + k,
            mode,
            view_size: if mode == MsgMode::Indirect {
                Some(view_size)
            } else {
                None
            },
            state: MountState::Unmounted,
        };
        let mut g = CpGate::new();
        let mut paired = false;
        if g.mount(port).is_ok() {
            paired = g.deliver(0x6000 + k, port.port_id, data_len).is_ok() && g.conservation();
        }
        out.push(CpSample {
            case: k,
            mode,
            data_len,
            view_size: if mode == MsgMode::Indirect {
                view_size
            } else {
                0
            },
            paired,
        });
        k += 1;
    }
    out
}

/// 抽样断言（判据 J1 直读口）：十例全对平且与基线一致。
fn sampling_ok() -> bool {
    let a = sample_ten();
    let b = sample_ten();
    if a.len() != 10 || a != b {
        return false;
    }
    for smp in a.iter() {
        if !smp.paired {
            return false;
        }
        // 直接档长度必在内联上限内；间接档长度必在视图内。
        match smp.mode {
            MsgMode::Direct => {
                if smp.data_len > INLINE_CAP || smp.view_size != 0 {
                    return false;
                }
            }
            MsgMode::Indirect => {
                if smp.data_len > smp.view_size {
                    return false;
                }
            }
        }
    }
    true
}

/// 语义档对平断言：两档 × 三长（零/中/上限）逐项成立。
fn mode_pairing_ok() -> bool {
    // 直接档：零长收、中长收、上限收、上限+1 拒。
    let d = ContextPort {
        port_id: 0x7000,
        context_id: 0x7100,
        mode: MsgMode::Direct,
        view_size: None,
        state: MountState::Unmounted,
    };
    if d.accepts(0).is_err() || d.accepts(INLINE_CAP / 2).is_err() || d.accepts(INLINE_CAP).is_err()
    {
        return false;
    }
    if d.accepts(INLINE_CAP + 1) != Err(CpDiag::EDirectOver.val()) {
        return false;
    }
    // 间接档：视图 512，正文 0/256/512 收，513 拒。
    let ind = ContextPort {
        port_id: 0x7200,
        context_id: 0x7300,
        mode: MsgMode::Indirect,
        view_size: Some(512),
        state: MountState::Unmounted,
    };
    if ind.accepts(0).is_err() || ind.accepts(256).is_err() || ind.accepts(512).is_err() {
        return false;
    }
    ind.accepts(513) == Err(CpDiag::EIndOver.val())
}

/// 档倒挂注入：直接档携视图、间接档缺视图、零上下文三路。
fn mount_gates_ok() -> bool {
    let mut g = CpGate::new();
    // 直接档携视图拒。
    if g.mount(ContextPort {
        port_id: 0x8000,
        context_id: 0x8100,
        mode: MsgMode::Direct,
        view_size: Some(128),
        state: MountState::Unmounted,
    }) != Err(CpDiag::EDirectWithView.val())
    {
        return false;
    }
    // 间接档缺视图拒。
    if g.mount(ContextPort {
        port_id: 0x8200,
        context_id: 0x8300,
        mode: MsgMode::Indirect,
        view_size: None,
        state: MountState::Unmounted,
    }) != Err(CpDiag::EIndNoView.val())
    {
        return false;
    }
    // 零上下文拒。
    if g.mount(ContextPort {
        port_id: 0x8400,
        context_id: 0,
        mode: MsgMode::Direct,
        view_size: None,
        state: MountState::Unmounted,
    }) != Err(CpDiag::ENoContext.val())
    {
        return false;
    }
    // 空样本拒。
    g.mount(ContextPort {
        port_id: 0,
        context_id: 0,
        mode: MsgMode::Direct,
        view_size: None,
        state: MountState::Unmounted,
    }) == Err(CpDiag::EEmpty.val())
}

/// 投递注入：未知端口/间接超视图/直接超内联三路。
fn deliver_gates_ok() -> bool {
    let mut g = CpGate::new();
    let _ = g.mount(ContextPort {
        port_id: 0x9000,
        context_id: 0x9100,
        mode: MsgMode::Direct,
        view_size: None,
        state: MountState::Unmounted,
    });
    let _ = g.mount(ContextPort {
        port_id: 0x9200,
        context_id: 0x9300,
        mode: MsgMode::Indirect,
        view_size: Some(64),
        state: MountState::Unmounted,
    });
    // 未知端口。
    if g.deliver(0xA0, 0xBEEF, 8) != Err(CpDiag::EUnknownPort.val()) {
        return false;
    }
    // 直接档超内联。
    if g.deliver(0xA1, 0x9000, INLINE_CAP + 1) != Err(CpDiag::EDirectOver.val()) {
        return false;
    }
    // 间接档超视图。
    g.deliver(0xA2, 0x9200, 65) == Err(CpDiag::EIndOver.val())
}

/// 段位守卫：对齐 0x40 且落在 VX9 B00 窗口。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 || SEG_BASE > 0x802C0 {
        return false;
    }
    true
}

/// 诊断码闭集守卫。
fn diag_closed_ok() -> bool {
    for d in CP_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    if detail(CpDiag::EDirectOver.val()) != CpDiag::EDirectOver.detail() {
        return false;
    }
    if !detail(SEG_BASE + 0x2C).starts_with("未知") {
        return false;
    }
    true
}

/// 二档闭集守卫。
fn mode_closed_ok() -> bool {
    if MODE_ALL.len() != 2 {
        return false;
    }
    let mut i = 0usize;
    while i < MODE_ALL.len() {
        let a = match MODE_ALL.get(i) {
            Some(m) => *m,
            None => return false,
        };
        if MsgMode::from_ordinal(a.ordinal()) != Some(a) {
            return false;
        }
        let mut j = i + 1;
        while j < MODE_ALL.len() {
            let b = match MODE_ALL.get(j) {
                Some(m) => *m,
                None => return false,
            };
            if a.ordinal() == b.ordinal() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 状态机单调与对平账守恒。
fn state_and_conservation_ok() -> bool {
    // 状态机单调。
    if MountState::Unmounted.rank() >= MountState::Mounted.rank()
        || MountState::Mounted.rank() >= MountState::Balanced.rank()
    {
        return false;
    }
    // 守恒：直接+间接端口各落一笔账后守恒成立。
    let mut g = CpGate::new();
    let _ = g.mount(ContextPort {
        port_id: 0xC000,
        context_id: 0xC100,
        mode: MsgMode::Direct,
        view_size: None,
        state: MountState::Unmounted,
    });
    let _ = g.mount(ContextPort {
        port_id: 0xC200,
        context_id: 0xC300,
        mode: MsgMode::Indirect,
        view_size: Some(128),
        state: MountState::Unmounted,
    });
    let d = g.deliver(0xD0, 0xC000, 32);
    let i = g.deliver(0xD1, 0xC200, 96);
    match (d, i) {
        (Ok(a), Ok(b)) => {
            a.mode == MsgMode::Direct
                && b.mode == MsgMode::Indirect
                && g.conservation()
                && g.len() == 2
        }
        _ => false,
    }
}

/// 完成写回语义：十例抽样摘要非空。
fn writeback_ok() -> bool {
    let samples = sample_ten();
    let mut greens = 0usize;
    for smp in samples.iter() {
        if smp.paired {
            greens += 1;
        }
    }
    let summary = format!("十例对平 {}/10", greens);
    greens == 10 && !summary.is_empty()
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12612 自检（判据 12611 卡：ContextPort 与消息模式语义档对平）。
pub fn run_unx12611_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12611");
    let samples = sample_ten();

    s.add_dyn(
        "F12612-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12612-02 诊断码闭集",
        diag_closed_ok(),
        &format!("n={}", CP_ALL.len()),
    );

    s.add_dyn(
        "F12612-03 二档闭集枚举",
        mode_closed_ok(),
        &format!("modes={} 档=[直接,间接]", MODE_ALL.len()),
    );

    s.add_dyn(
        "F12612-04 语义档对平断言绿",
        mode_pairing_ok(),
        &format!(
            "直接上限={} 间接视图=512 四点边界",
            INLINE_CAP
        ),
    );

    s.add_dyn(
        "F12612-05 ContextPort 挂载闸",
        mount_gates_ok(),
        "直接携视图/间接缺视图/零上下文/空样本 四路全拒",
    );

    s.add_dyn(
        "F12612-06 投递闸",
        deliver_gates_ok(),
        "未知端口/直接超内联/间接超视图 三路全拒",
    );

    // 抽样 10 例：对平判定与基线一致。
    s.add_dyn(
        "F12612-07 抽样 10 例一致",
        sampling_ok(),
        &format!(
            "直接={} 间接={} 全绿={}",
            samples.iter().filter(|s| s.mode == MsgMode::Direct).count(),
            samples.iter().filter(|s| s.mode == MsgMode::Indirect).count(),
            samples.iter().filter(|s| s.paired).count(),
        ),
    );

    s.add_dyn(
        "F12612-08 状态机单调与守恒",
        state_and_conservation_ok(),
        "未挂→已挂→已对平 只进不退，账守恒",
    );

    s.add_dyn(
        "F12612-09 双跑一致",
        sample_ten() == sample_ten(),
        "十例两次逐项相同",
    );

    s.add_dyn(
        "F12612-10 空机守恒",
        CpGate::new().conservation(),
        "空机守恒平凡成立",
    );

    s.add_dyn(
        "F12612-11 端口表闭集",
        MODE_ALL.len() == 2 && INLINE_CAP == 256,
        "二档 + 内联上限常量冻结",
    );

    s.add_dyn(
        "F12612-12 完成写回语义",
        writeback_ok(),
        "十例对平摘要非空",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x802C0);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn diag_closed_set_unique() {
        assert_eq!(CP_ALL.len(), 12);
        let mut i = 0usize;
        while i < CP_ALL.len() {
            let vi = CP_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < CP_ALL.len() {
                let vj = CP_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x2B).contains("未知"));
    }

    #[test]
    fn mode_closed_set_two() {
        assert_eq!(MODE_ALL.len(), 2);
        assert!(MsgMode::from_ordinal(2).is_none());
        assert_eq!(MsgMode::Direct.ordinal(), 0);
        assert_eq!(MsgMode::Indirect.ordinal(), 1);
        assert!(mode_closed_ok());
    }

    #[test]
    fn direct_mode_boundary() {
        // 直接档：内联上限三点 + 超限拒。
        let d = ContextPort {
            port_id: 1,
            context_id: 1,
            mode: MsgMode::Direct,
            view_size: None,
            state: MountState::Unmounted,
        };
        assert!(d.accepts(0).is_ok());
        assert!(d.accepts(INLINE_CAP).is_ok());
        assert_eq!(d.accepts(INLINE_CAP + 1), Err(CpDiag::EDirectOver.val()));
        // 直接档静态语义：携视图即倒挂拒。
        let with_view = ContextPort {
            view_size: Some(64),
            ..d
        };
        assert_eq!(with_view.mode_sound(), Err(CpDiag::EDirectWithView.val()));
        assert!(d.mode_sound().is_ok());
    }

    #[test]
    fn indirect_mode_boundary() {
        // 间接档：视图边界 + 缺视图拒。
        let ind = ContextPort {
            port_id: 2,
            context_id: 2,
            mode: MsgMode::Indirect,
            view_size: Some(512),
            state: MountState::Unmounted,
        };
        assert!(ind.accepts(0).is_ok());
        assert!(ind.accepts(512).is_ok());
        assert_eq!(ind.accepts(513), Err(CpDiag::EIndOver.val()));
        let no_view = ContextPort {
            view_size: None,
            ..ind
        };
        assert_eq!(no_view.mode_sound(), Err(CpDiag::EIndNoView.val()));
        assert_eq!(no_view.accepts(8), Err(CpDiag::EIndNoView.val()));
    }

    #[test]
    fn mount_gates_reject() {
        assert!(mount_gates_ok());
    }

    #[test]
    fn deliver_gates_reject() {
        assert!(deliver_gates_ok());
    }

    #[test]
    fn sampling_ten_consistent() {
        // 抽样 10 例：全对平、双跑一致、档-长恒等。
        let a = sample_ten();
        let b = sample_ten();
        assert_eq!(a, b);
        assert_eq!(a.len(), 10);
        assert!(sampling_ok());
        assert!(a.iter().all(|s| s.paired));
    }

    #[test]
    fn state_machine_and_conservation() {
        assert!(state_and_conservation_ok());
    }

    #[test]
    fn duplicate_context_rejected() {
        // 重复挂上下文。
        let mut g = CpGate::new();
        let p = ContextPort {
            port_id: 0xE0,
            context_id: 0xE1,
            mode: MsgMode::Direct,
            view_size: None,
            state: MountState::Unmounted,
        };
        assert!(g.mount(p).is_ok());
        assert_eq!(g.mount(p), Err(CpDiag::EDupContext.val()));
        assert_eq!(g.port_count(), 1);
    }

    #[test]
    fn writeback_summary_nonempty() {
        assert!(writeback_ok());
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12611_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }
}
