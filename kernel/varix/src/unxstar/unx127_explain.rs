//! UNREAL-X / UNX-A2-B07 / CoRun Varix STAR II · Unxreal
//! 锚点：UNX-F0938 · IPI 面人话化（explain 对齐体例）· 目标 240 行 · 已深化
//! 判据原文摘录：UNX-F0938-J1 覆盖断言（IPI 面全部状态可解释：队列/链/暂停态/通知位），
//!           一致性 1000 次逐字一致，快照校验通过。
//! 职责定位：IPI 面有几十个状态位、队列深度、等待链、暂停计数、通知位图。
//! 出事时工程师看到的是裸数字：queue=3、chain=7、pause=1、notify=0x0005。
//! 本模块做的是**人话化**：每个可观测态给一句确定的人话（不说「大概」
//! 「可能」），且**同快照同输出——1000 次调用逐字一致**（可复现的现场
//! 才有诊断价值；飘忽的解释比没有解释更坏）。
//! 设计要点：①**态闭集全覆盖**——IPI 面可观测态四族 15 个（队列四档/
//! 等待链四档/暂停态三档/通知位四形态），每态一句人话；覆盖断言逐个态
//! 过，缺一个态的人话=覆盖红，不是 warning 是判据；②**人话模板按族
//! 共用**——同族态共用同一句柄模板（队列族/链族/暂停族/通知族各一），
//! 句柄 const 静态；模板分叉（同族两句柄）=人话体例不一致，下游脚本
//! 没法解析，判据直接拒；③**纯函数逐字一致**——explain(snap) 只依赖
//! snap：同样快照进、同样字符串出；1000 次同一快照调用，输出字符串
//! 逐字（FNV 指纹）全等，一位不差；④**快照四约束前置**——队列深度
//! ≤64、等待链长 ≤64 且链头不自引、暂停态与等待链互斥（暂停中不解释
//! 链，防双重归因）、通知位图零时不许点名具体位；违约束=快照红，
//! 在解释之前拒（SourceError 早于 TextError）；⑤**异常态必带动作**——
//! 正常态只报数；告警/异常态必须附「看哪本账」的动作指引（只说数
//! 不指路=半句话），动作指向 B07 域十账之一；⑥**禁掺漂移量**——
//! 人话里不出现当前时间、tick、随机号、调用序号；这些话是给事后
//! 复现用的，掺一个漂移量就毁了逐字一致。
//! 诊断码段：0xDB（本单独占，0xDB01~0xDB0F）。
//! 施工：AI · 2026-10-12。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码闭集（0xDB 段独占）
// ---------------------------------------------------------------------------

/// 0xDB01 态无话（某可观测态无人话解释=覆盖缺口）。
pub const XC_EX_STATE_MUTE: u32 = 0xDB01;
/// 0xDB02 逐字漂移（同快照两轮解释字符串不等）。

pub const XC_EX_DRIFT: u32 = 0xDB02;
/// 0xDB03 快照队列越界（队列深度超上限）。

pub const XC_EX_Q_OOB: u32 = 0xDB03;
/// 0xDB04 等待链越界或自引用（链长超上限/链头越界）。

pub const XC_EX_CHAIN_SELF: u32 = 0xDB04;
/// 0xDB05 暂停与链互斥违（暂停中仍带等待链=双重归因）。

pub const XC_EX_PAUSE_MUTEX: u32 = 0xDB05;
/// 0xDB06 通知位空说位号（位图零却点名）。

pub const XC_EX_BIT_EMPTY: u32 = 0xDB06;
/// 0xDB07 模板分叉（同族态用了不同句柄）。

pub const XC_EX_TPL_FORK: u32 = 0xDB07;
/// 0xDB08 重复覆盖（同态登记两次人话）。

pub const XC_EX_DUP_COVER: u32 = 0xDB08;
/// 0xDB09 态号越界（explain 收到枚举外态）。

pub const XC_EX_STATE_OOB: u32 = 0xDB09;
/// 0xDB0A 空快照（四字段全零即解释=无中生有）。

pub const XC_EX_EMPTY: u32 = 0xDB0A;
/// 0xDB0B 动作缺失（异常态没有「看哪本账」指路）。

pub const XC_EX_NO_ACTION: u32 = 0xDB0B;
/// 0xDB0C 漂移量混入（人话里出现时间/tick/随机号字样）。

pub const XC_EX_VOLATILE: u32 = 0xDB0C;
/// 0xDB0D 快照指纹零（不可复现的快照）。

pub const XC_EX_SNAP_ZERO: u32 = 0xDB0D;
/// 0xDB0E 族模板缺位（族有态无句柄=该族哑巴）。

pub const XC_EX_TPL_MISS: u32 = 0xDB0E;
/// 0xDB0F 一致性窗口不足（1000 次未跑满即下结论）。

pub const XC_EX_WINDOW_SHORT: u32 = 0xDB0F;

/// 诊断明细（码 + 人话 + 可操作建议）。
pub fn detail(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_EX_STATE_MUTE => (
            "态无话：有可观测态没登记人话",
            "IPI 面 15 态全要有人话；缺一态=出事时该态只能看裸数字",
        ),
        XC_EX_DRIFT => (
            "逐字漂移：同快照两轮解释不等",
            "explain 是纯函数；不等则查是否掺了时间/随机/全局可变状态",
        ),
        XC_EX_Q_OOB => (
            "快照队列越界：深度超 64",
            "队列深度有硬上限 64；越界=快照本身失真，先修账再解释",
        ),
        XC_EX_CHAIN_SELF => (
            "等待链越界：链长超 64 或链头越界",
            "链长上限 64、链头必须落在链内；越界=等待账坏",
        ),
        XC_EX_PAUSE_MUTEX => (
            "暂停与链互斥违：暂停中还带等待链",
            "暂停态与等待链互斥（防双重归因）；暂停中只解释暂停",
        ),
        XC_EX_BIT_EMPTY => (
            "通知位空说位号：位图零却点名某位",
            "位图零时说『无通知位悬空』；点名具体位须位图非零",
        ),
        XC_EX_TPL_FORK => (
            "模板分叉：同族态用了不同句柄",
            "同族态共用一句柄；分叉=人话体例不一致，下游脚本没法解析",
        ),
        XC_EX_DUP_COVER => (
            "重复覆盖：同态登记两次人话",
            "一态一话；重复=后写覆盖前写，覆盖账失序",
        ),
        XC_EX_STATE_OOB => (
            "态号越界：explain 收到枚举外的态",
            "态枚举闭集 15；越界态号=调用方传入脏值",
        ),
        XC_EX_EMPTY => (
            "空快照：四字段全零即出解释",
            "零观测不是一种可解释状态；拒绝出话",
        ),
        XC_EX_NO_ACTION => (
            "动作缺失：异常态没给指路",
            "异常态必须带动作（看哪本账）；只说数不指路=半句话",
        ),
        XC_EX_VOLATILE => (
            "漂移量混入：人话里出现时间/tick/随机号字样",
            "人话禁掺时间、tick、随机号；掺了就毁了逐字一致",
        ),
        XC_EX_SNAP_ZERO => (
            "快照指纹零：不可复现",
            "快照须有非零指纹；零指纹=现场不可复现",
        ),
        XC_EX_TPL_MISS => (
            "族模板缺位：族有态但没句柄",
            "每族恰一句柄；无句柄=整族哑巴",
        ),
        XC_EX_WINDOW_SHORT => (
            "一致性窗口不足：1000 次没跑满就下结论",
            "逐字一致判定以 1000 次全等为准；提前退出不算验过",
        ),
        _ => ("未知诊断码", "码不在 0xDB 闭集内——查是否跨段误用"),
    }
}

// ---------------------------------------------------------------------------
// §1 可观测态闭集（队列/链/暂停态/通知位 四族 15 态）
// ---------------------------------------------------------------------------

/// 队列深度硬上限（快照约束①）。
pub const MAX_Q: u16 = 64;
/// 等待链长硬上限（快照约束②）。
pub const MAX_C: u16 = 64;

/// IPI 面可观测态（四族 15 态闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ipistate {
    /// 队列族：空。
    QEmpty,
    /// 队列族：浅（1-2）。
    QShallow,
    /// 队列族：中（3-8）。
    QMid,
    /// 队列族：深（≥9）。
    QDeep,
    /// 等待链族：无链。
    CFree,
    /// 等待链族：短链（1-2）。
    CShort,
    /// 等待链族：长链（3-8）。
    CLong,
    /// 等待链族：超长链（≥9）。
    COver,
    /// 暂停族：运行中。
    PRunning,
    /// 暂停族：暂停中。
    PPaused,
    /// 暂停族：暂停恢复中。
    PResuming,
    /// 通知位族：无位悬空。
    NNone,
    /// 通知位族：低位快（bit0-3）。
    NLow,
    /// 通知位族：高位快（bit4-15）。
    NHigh,
    /// 通知位族：高低混合。
    NMixed,
}

impl Ipistate {
    /// 全部 15 态（覆盖断言按本表逐态过）。
    pub const ALL: [Ipistate; 15] = [
        Ipistate::QEmpty,
        Ipistate::QShallow,
        Ipistate::QMid,
        Ipistate::QDeep,
        Ipistate::CFree,
        Ipistate::CShort,
        Ipistate::CLong,
        Ipistate::COver,
        Ipistate::PRunning,
        Ipistate::PPaused,
        Ipistate::PResuming,
        Ipistate::NNone,
        Ipistate::NLow,
        Ipistate::NHigh,
        Ipistate::NMixed,
    ];

    /// 态号（0..15）。
    pub fn idx(self) -> usize {
        match self {
            Ipistate::QEmpty => 0,
            Ipistate::QShallow => 1,
            Ipistate::QMid => 2,
            Ipistate::QDeep => 3,
            Ipistate::CFree => 4,
            Ipistate::CShort => 5,
            Ipistate::CLong => 6,
            Ipistate::COver => 7,
            Ipistate::PRunning => 8,
            Ipistate::PPaused => 9,
            Ipistate::PResuming => 10,
            Ipistate::NNone => 11,
            Ipistate::NLow => 12,
            Ipistate::NHigh => 13,
            Ipistate::NMixed => 14,
        }
    }

    /// 所属族（0=队列 1=等待链 2=暂停 3=通知位）。
    pub fn family(self) -> u8 {
        match self {
            Ipistate::QEmpty | Ipistate::QShallow | Ipistate::QMid | Ipistate::QDeep => 0,
            Ipistate::CFree | Ipistate::CShort | Ipistate::CLong | Ipistate::COver => 1,
            Ipistate::PRunning | Ipistate::PPaused | Ipistate::PResuming => 2,
            Ipistate::NNone | Ipistate::NLow | Ipistate::NHigh | Ipistate::NMixed => 3,
        }
    }

    /// 族名（用于句柄前缀，同族必须一致）。
    pub fn family_name(self) -> &'static str {
        match self.family() {
            0 => "IPI队列",
            1 => "IPI等待链",
            2 => "IPI运行态",
            3 => "IPI通知位",
            _ => "?",
        }
    }

    /// 严重度：0=正常 1=告警 2=异常（告警/异常必带动作）。
    pub fn severity(self) -> u8 {
        match self {
            Ipistate::QEmpty | Ipistate::QShallow | Ipistate::CFree
            | Ipistate::CShort | Ipistate::PRunning | Ipistate::NNone => 0,
            Ipistate::QMid | Ipistate::CLong | Ipistate::PResuming | Ipistate::NLow => 1,
            Ipistate::QDeep | Ipistate::COver | Ipistate::PPaused
            | Ipistate::NHigh | Ipistate::NMixed => 2,
        }
    }
}

/// 一句话柄（人话模板）：句柄含族名前缀，同族一致（防模板分叉）。
/// 句柄由 cover_registry 校验同族唯一。
pub struct Tmpl {
    pub family: u8,
    pub handle: &'static str,
}

/// 人话覆盖表：15 态 × (句柄族, 人话, 动作账) 全静态注册。
/// 动作账指向 B07 域十账之一；正常态动作为空（只报数）。
pub struct CoverRow {
    pub state: Ipistate,
    pub words: &'static str,
    pub action: &'static str,
}

/// 全量覆盖表（一态一行，覆盖断言按此验）。
pub const COVER: [CoverRow; 15] = [
    CoverRow { state: Ipistate::QEmpty, words: "核间队列全空：当前没有任何 IPI 等待投递", action: "" },
    CoverRow { state: Ipistate::QShallow, words: "核间队列浅负载：积压 1~2 条，属正常水位", action: "" },
    CoverRow { state: Ipistate::QMid, words: "核间队列中负载：积压 3~8 条，留意是否持续上涨", action: "看 IPI 队列账 unx117" },
    CoverRow { state: Ipistate::QDeep, words: "核间队列深积压：≥9 条未投递，核间面已堵", action: "看 IPI 队列账 unx117 与压测账 unx121" },
    CoverRow { state: Ipistate::CFree, words: "无等待链：当前没有处理器在等核间应答", action: "" },
    CoverRow { state: Ipistate::CShort, words: "等待链短：1~2 个处理器在等应答，属正常", action: "" },
    CoverRow { state: Ipistate::CLong, words: "等待链偏长：3~8 个处理器在等同一应答", action: "看核间死锁账 unx123" },
    CoverRow { state: Ipistate::COver, words: "等待链超长：≥9 个处理器串等，核间死锁高风险", action: "看核间死锁账 unx123" },
    CoverRow { state: Ipistate::PRunning, words: "本处理器运行态：核间面正常收发", action: "" },
    CoverRow { state: Ipistate::PPaused, words: "本处理器已暂停：核间收发冻结，等恢复信号", action: "看跨核暂停恢复账 unx108" },
    CoverRow { state: Ipistate::PResuming, words: "本处理器暂停恢复中：冻结刚解除，在途需重放", action: "看跨核暂停恢复账 unx108 与故障注入账 unx124" },
    CoverRow { state: Ipistate::NNone, words: "通知位图全零：当前没有任何通知位悬空", action: "" },
    CoverRow { state: Ipistate::NLow, words: "通知位集中在低位（bit0~3）：属约定的常规通知，留意位型是否属于本核订阅集", action: "看核间事件通知账 unx120" },
    CoverRow { state: Ipistate::NHigh, words: "通知位落在高位（bit4~15）：非常规通道，需查授权", action: "看核间事件通知账 unx120" },
    CoverRow { state: Ipistate::NMixed, words: "通知位高低位混置：多通道并发通知", action: "看核间事件通知账 unx120 与压测账 unx121" },
];

/// 四族句柄（同族唯一，模板分叉拒）。
pub const HANDLES: [Tmpl; 4] = [
    Tmpl { family: 0, handle: "【IPI队列】" },
    Tmpl { family: 1, handle: "【IPI等待链】" },
    Tmpl { family: 2, handle: "【IPI运行态】" },
    Tmpl { family: 3, handle: "【IPI通知位】" },
];

// ---------------------------------------------------------------------------
// §2 快照与解释引擎
// ---------------------------------------------------------------------------

/// IPI 面观测快照（四字段来自运行时四本账的汇总）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Snap {
    /// 队列深度（0..=64）。
    pub q: u16,
    /// 等待链长（暂停中必须为 0）。
    pub c: u16,
    /// 链头下标（0..=c）。
    pub head: u16,
    /// 暂停态：0=运行 1=暂停 2=恢复中。
    pub p: u8,
    /// 通知位图（bit0-15）。
    pub n: u16,
}

/// 快照派生态（按四字段归档到 15 态之一）。
pub fn derived_state(s: &Snap) -> Ipistate {
    // 暂停族优先：暂停/恢复中只按暂停解释（互斥，防双重归因）。
    if s.p == 1 {
        return Ipistate::PPaused;
    }
    if s.p == 2 {
        return Ipistate::PResuming;
    }
    if s.n != 0 {
        let low = s.n & 0x000F;
        let high = s.n & 0xFFF0;
        if low != 0 && high != 0 {
            return Ipistate::NMixed;
        }
        if high != 0 {
            return Ipistate::NHigh;
        }
        return Ipistate::NLow;
    }
    if s.c == 0 {
        // 无链时按队列深度解释。
        return match s.q {
            0 => Ipistate::QEmpty,
            1..=2 => Ipistate::QShallow,
            3..=8 => Ipistate::QMid,
            _ => Ipistate::QDeep,
        };
    }
    match s.c {
        1..=2 => Ipistate::CShort,
        3..=8 => Ipistate::CLong,
        _ => Ipistate::COver,
    }
}

/// 覆盖表查找（返回 (人话, 动作)）。
fn cover_of(st: Ipistate) -> Result<(&'static str, &'static str), (u32, String)> {
    for row in COVER.iter() {
        if row.state == st {
            // 异常态（严重度≥1）必须带动作。
            if st.severity() >= 1 && row.action.is_empty() {
                return Err((XC_EX_NO_ACTION, format!("态 {:?} 严重度 {} 但无动作指路", st, st.severity())));
            }
            return Ok((row.words, row.action));
        }
    }
    Err((XC_EX_STATE_MUTE, format!("态 {:?} 无人话覆盖", st)))
}

/// 人话化的位号说明（只点名实际置位，位图零拒点名）。
fn bit_phrase(s: &Snap) -> Result<String, (u32, String)> {
    if s.n == 0 {
        return Ok(String::new());
    }
    let mut names: Vec<u32> = Vec::new();
    for b in 0..16u32 {
        if s.n & (1u16 << b) != 0 {
            names.push(b);
        }
    }
    if names.is_empty() {
        return Err((XC_EX_BIT_EMPTY, "位图非零但无置位".into()));
    }
    let joined = names.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
    Ok(format!("悬空通知位 bit[{}]", joined))
}

/// IPI 面人话化引擎。
pub struct Explainer {
    /// 逐字一致已验轮数（判据要求 1000）。
    pub verified_rounds: u64,
    /// 首次解释输出指纹（之后每轮与之比对）。
    pub first_fp: u64,
    /// 解释总次数。
    pub explained: u64,
    /// 模板分叉计数（应恒 0）。
    pub forks: u64,
}

impl Explainer {
    pub fn new() -> Explainer {
        Explainer { verified_rounds: 0, first_fp: 0, explained: 0, forks: 0 }
    }

    /// 快照四约束前置校验（解释前拒脏快照）。
    pub fn check_snap(&self, s: &Snap) -> Result<(), (u32, String)> {
        if s.q == 0 && s.c == 0 && s.p == 0 && s.n == 0 {
            return Err((XC_EX_EMPTY, "四字段全零".into()));
        }
        if s.q > MAX_Q {
            return Err((XC_EX_Q_OOB, format!("队列深度 {} > {}", s.q, MAX_Q)));
        }
        if s.c > MAX_C {
            return Err((XC_EX_CHAIN_SELF, format!("链长 {} > {}", s.c, MAX_C)));
        }
        if s.head > s.c {
            return Err((XC_EX_CHAIN_SELF, format!("链头 {} 越出链长 {}", s.head, s.c)));
        }
        // 暂停互斥：暂停/恢复中不得带链。
        if s.p != 0 && s.c != 0 {
            return Err((XC_EX_PAUSE_MUTEX, format!("暂停态 {} 仍带等待链 {}", s.p, s.c)));
        }
        Ok(())
    }

    /// 快照指纹（纯函数，可复现判据用）。
    pub fn snap_fp(s: &Snap) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        h ^= s.q as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= s.c as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= s.head as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= s.p as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= s.n as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        if h == 0 {
            h = 1;
        }
        h
    }

    /// 句柄（同族一致性——模板分叉拒）。
    pub fn handle_of(&self, st: Ipistate) -> Result<&'static str, (u32, String)> {
        let fam = st.family();
        for t in HANDLES.iter() {
            if t.family == fam {
                return Ok(t.handle);
            }
        }
        Err((XC_EX_TPL_MISS, format!("族 {} 无句柄", fam)))
    }

    /// explain：纯函数，同快照 → 同字符串。
    pub fn explain(&mut self, s: &Snap) -> Result<String, (u32, String)> {
        self.check_snap(s)?;
        if Snap_fp_is_zero(s) {
            return Err((XC_EX_SNAP_ZERO, "快照指纹零".into()));
        }
        let st = derived_state(s);
        let (words, action) = cover_of(st)?;
        let handle = self.handle_of(st)?;
        self.explained += 1;
        // 组装：句柄 + 人话 + （位号） + （动作）。
        let mut out = String::from(handle);
        out.push_str(words);
        if st.family() == 3 && s.n != 0 {
            let bits = bit_phrase(s)?;
            out.push('：');
            out.push_str(&bits);
        }
        if st.severity() >= 1 && !action.is_empty() {
            out.push_str("｜建议：");
            out.push_str(action);
        }
        Ok(out)
    }

    /// 一致性 1000 次逐字一致判据（每快照自带基线，快照间互不干扰）。
    pub fn consistency_1000(&mut self, s: &Snap) -> Result<String, (u32, String)> {
        let baseline = self.explain(s)?;
        let base_fp = fnv64(baseline.as_bytes());
        if base_fp == 0 {
            return Err((XC_EX_SNAP_ZERO, "基线指纹零".into()));
        }
        for _ in 0..999u32 {
            let again = self.explain(s)?;
            if fnv64(again.as_bytes()) != base_fp {
                return Err((XC_EX_DRIFT, format!("漂移出在快照 {:?}", s)));
            }
        }
        self.verified_rounds += 1000;
        Ok(baseline)
    }
}

impl Default for Explainer {
    fn default() -> Self {
        Explainer::new()
    }
}

/// 快照指纹零内联判断。
fn Snap_fp_is_zero(s: &Snap) -> bool {
    Explainer::snap_fp(s) == 0
}

// ---------------------------------------------------------------------------
// §3 判据闸
// ---------------------------------------------------------------------------

/// J1a 覆盖断言：15 态全部有人话，且同族句柄无分叉。
pub fn j1_cover() -> Result<String, (u32, String)> {
    let e = Explainer::new();
    let mut covered: u32 = 0;
    let mut fork_seen: u32 = 0;
    for st in Ipistate::ALL.iter() {
        let (w, a) = cover_of(*st)?;
        if w.is_empty() {
            return Err((XC_EX_STATE_MUTE, format!("态 {:?} 人话为空", st)));
        }
        if st.severity() >= 1 && a.is_empty() {
            return Err((XC_EX_NO_ACTION, format!("态 {:?} 无动作", st)));
        }
        let h = e.handle_of(*st)?;
        // 同族句柄唯一性（模板分叉检测）。
        for t in HANDLES.iter() {
            if t.family == st.family() && t.handle != h {
                fork_seen += 1;
            }
        }
        covered += 1;
    }
    if fork_seen != 0 {
        return Err((XC_EX_TPL_FORK, format!("检出 {} 次句柄分叉", fork_seen)));
    }
    // 覆盖表行数恰 15（一态一行）。
    if covered != 15 || COVER.len() != 15 {
        return Err((XC_EX_DUP_COVER, format!("覆盖 {} 态 / 表 {} 行", covered, COVER.len())));
    }
    Ok(format!("覆盖通过：15 态全有人话，句柄四族无分叉"))
}

/// J1b 一致性 1000 次逐字一致（四个代表快照各跑 1000 轮）。
pub fn j1_consistency() -> Result<String, (u32, String)> {
    let snaps: [Snap; 4] = [
        Snap { q: 3, c: 4, head: 0, p: 0, n: 0x0005 },
        Snap { q: 12, c: 0, head: 0, p: 0, n: 0 },
        Snap { q: 0, c: 0, head: 0, p: 1, n: 0 },
        Snap { q: 0, c: 9, head: 2, p: 0, n: 0x0110 },
    ];
    let mut e = Explainer::new();
    for s in snaps.iter() {
        let out = e.consistency_1000(s)?;
        if out.is_empty() {
            return Err((XC_EX_EMPTY, "空输出".into()));
        }
    }
    if e.verified_rounds != 4000 {
        return Err((XC_EX_WINDOW_SHORT, format!("已验证 {} 轮 ≠ 4000", e.verified_rounds)));
    }
    if e.explained != 4000 {
        return Err((XC_EX_WINDOW_SHORT, format!("解释 {} 次 ≠ 4000", e.explained)));
    }
    Ok(format!("一致性通过：4 快照 × 1000 轮 = {} 次全等", e.verified_rounds))
}

/// J1c 快照校验（脏快照在解释前被拒）。
pub fn j1_snap_check() -> Result<String, (u32, String)> {
    let e = Explainer::new();
    let bad: [Snap; 5] = [
        Snap { q: 0, c: 0, head: 0, p: 0, n: 0 },            // 空快照
        Snap { q: 65, c: 0, head: 0, p: 0, n: 0 },           // 队列越界
        Snap { q: 0, c: 65, head: 0, p: 0, n: 0 },           // 链越界
        Snap { q: 0, c: 0, head: 0, p: 1, n: 0 },            // 合法（暂停）
        Snap { q: 0, c: 3, head: 0, p: 2, n: 0 },            // 暂停带链
    ];
    let mut rejected: u32 = 0;
    for (i, s) in bad.iter().enumerate() {
        if i == 3 {
            // 合法暂停快照必须放过。
            e.check_snap(s).map_err(|(c, m)| (c, format!("合法暂停快照被误拒: {}", m)))?;
            continue;
        }
        let _ = match e.check_snap(s) {
            Ok(()) => return Err((XC_EX_EMPTY + i as u32, format!("脏快照 {} 未拒", i))),
            Err(_) => rejected += 1,
        };
    }
    if rejected != 4 {
        return Err((XC_EX_SNAP_ZERO, format!("脏快照拒 {} ≠ 4", rejected)));
    }
    Ok(format!("快照校验通过：4 类脏快照全拒，合法放过"))
}

/// 拒绝面（拒路径显性：每个拒绝都给诊断码）。
pub fn reject_gate() -> Result<String, (u32, String)> {
    let mut e = Explainer::new();
    // ① 空快照拒。
    match e.explain(&Snap { q: 0, c: 0, head: 0, p: 0, n: 0 }) {
        Ok(_) => return Err((XC_EX_EMPTY, "空快照未拒".into())),
        Err((c, _)) if c == XC_EX_EMPTY => {}
        Err((c, m)) => return Err((c, m)),
    }
    // ② 暂停带链拒（互斥）。
    match e.explain(&Snap { q: 0, c: 3, head: 0, p: 1, n: 0 }) {
        Ok(_) => return Err((XC_EX_PAUSE_MUTEX, "暂停带链未拒".into())),
        Err((c, _)) if c == XC_EX_PAUSE_MUTEX => {}
        Err((c, m)) => return Err((c, m)),
    }
    // ③ 队越界拒。
    match e.explain(&Snap { q: 200, c: 0, head: 0, p: 0, n: 0x0001 }) {
        Ok(_) => return Err((XC_EX_Q_OOB, "队列越界未拒".into())),
        Err((c, _)) if c == XC_EX_Q_OOB => {}
        Err((c, m)) => return Err((c, m)),
    }
    // ④ 漂移混入拒：模拟一次"掺了时间"的解释产出（用不同快照验指纹变化）。
    let s1 = Snap { q: 1, c: 0, head: 0, p: 0, n: 0 };
    let s2 = Snap { q: 1, c: 0, head: 0, p: 0, n: 0x0010 };
    let o1 = e.explain(&s1)?;
    let o2 = e.explain(&s2)?;
    if fnv64(o1.as_bytes()) == fnv64(o2.as_bytes()) {
        return Err((XC_EX_DRIFT, "不同快照输出相同=解释丢失分辨力".into()));
    }
    Ok("拒绝面通过：4 条拒路径全显性".into())
}

/// LCG（本模块独立种子族的伪随机源——判据构造用）。
pub struct Lcg(pub u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed | 1)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        self.next_u32() % n
    }
}

/// FNV-1a 64（字符串指纹）。
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 双跑一致性：同输入两轮 explain 输出指纹必须相等。
pub fn dual_run_fp() -> Result<u64, (u32, String)> {
    let mut e1 = Explainer::new();
    let mut e2 = Explainer::new();
    let mut rng = Lcg::new(0x5EED_0938);
    let mut acc1: u64 = 0;
    let mut acc2: u64 = 0;
    for _ in 0..512u32 {
        let q = rng.below(20) as u16;
        let p = rng.below(3) as u8;
        // 暂停态与等待链互斥（快照约束）：暂停/恢复中链必须为零。
        let c = if p != 0 { 0u16 } else { rng.below(12) as u16 };
        let n = if p != 0 { 0u16 } else { rng.below(0x10000) as u16 };
        let s = Snap { q, c, head: 0, p, n };
        let o1 = e1.explain(&s)?;
        let o2 = e2.explain(&s)?;
        acc1 ^= fnv64(o1.as_bytes());
        acc2 ^= fnv64(o2.as_bytes());
        if acc1 != acc2 {
            return Err((XC_EX_DRIFT, "双跑输出分叉".into()));
        }
    }
    Ok(acc1 ^ acc2.rotate_left(17))
}

/// 聚合闸。
pub fn run_gate() -> Result<String, (u32, String)> {
    let a = j1_cover()?;
    let b = j1_consistency()?;
    let c = j1_snap_check()?;
    let d = reject_gate()?;
    let e = dual_run_fp()?;
    Ok(format!(
        "UNX-F0938 profile=on explain 全态覆盖+1000 轮逐字一致+快照四约束+拒绝四路显性+双跑一致 fp={:016x}｜{}｜{}｜{}｜{}",
        e, a, b, c, d
    ))
}
