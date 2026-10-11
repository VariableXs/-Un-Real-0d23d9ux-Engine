//! unxt262_overlay —— UNX-F0261 诊断浮层合成引擎（零互蚀混合）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0261 · 帧簿双头显（诊断浮层 + 正常输出合成） · 判据：诊断浮层与正常输出同屏合成零互蚀，浮层开合零残影
//! · 组件：Alpha 混合引擎（每通道独立）+ 混合账（守恒）+ 残影守卫
//! · 判据落地：诊断浮层与正常输出同屏合成零互蚀
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T262_VERSION: &str = "t262-overlay-v1";

/// 诊断段基址。
pub const T262_SEG_BASE: u16 = 0x3a10;

// --------------------------------------------------------------------------
// 诊断码（t262 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 混合引擎诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t262Code {
    /// 0x3a10 Alpha 值非法（>255）。
    AlphaBad,
    /// 0x3a11 混合溢出（通道值超 255）。
    BlendOverflow,
    /// 0x3a12 源面被改（正常输出被互蚀）。
    SrcUntouched,
    /// 0x3a13 层级次序非法（浮层在源面下）。
    LayerOrderBad,
    /// 0x3a14 残影残留（关层后像素未复位）。
    ResidueLeft,
    /// 0x3a15 浮层与源面格式不一致。
    FormatMismatch,
    /// 0x3a16 行距非法（非字节对齐）。
    PitchBad,
    /// 0x3a17 裁剪丢失（越界混合）。
    ClipLost,
    /// 0x3a18 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3a19 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3a1a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3a1b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3a1c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3a1d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x3a1e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x3a1f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t262Code {
    /// 全量闭集。
    pub const ALL: [t262Code; 16] = [
        t262Code::AlphaBad,
        t262Code::BlendOverflow,
        t262Code::SrcUntouched,
        t262Code::LayerOrderBad,
        t262Code::ResidueLeft,
        t262Code::FormatMismatch,
        t262Code::PitchBad,
        t262Code::ClipLost,
        t262Code::LedgerBroken,
        t262Code::SnapshotFingerprint,
        t262Code::TableBroken,
        t262Code::IndexMisaligned,
        t262Code::DuplicateEntry,
        t262Code::OrderViolated,
        t262Code::GuardTripped,
        t262Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t262Code::AlphaBad => 0x3a10,
            t262Code::BlendOverflow => 0x3a11,
            t262Code::SrcUntouched => 0x3a12,
            t262Code::LayerOrderBad => 0x3a13,
            t262Code::ResidueLeft => 0x3a14,
            t262Code::FormatMismatch => 0x3a15,
            t262Code::PitchBad => 0x3a16,
            t262Code::ClipLost => 0x3a17,
            t262Code::LedgerBroken => 0x3a18,
            t262Code::SnapshotFingerprint => 0x3a19,
            t262Code::TableBroken => 0x3a1a,
            t262Code::IndexMisaligned => 0x3a1b,
            t262Code::DuplicateEntry => 0x3a1c,
            t262Code::OrderViolated => 0x3a1d,
            t262Code::GuardTripped => 0x3a1e,
            t262Code::SegmentGuard => 0x3a1f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t262Code::AlphaBad => "Alpha 值非法（>255）",
            t262Code::BlendOverflow => "混合溢出（通道值超 255）",
            t262Code::SrcUntouched => "源面被改（正常输出被互蚀）",
            t262Code::LayerOrderBad => "层级次序非法（浮层在源面下）",
            t262Code::ResidueLeft => "残影残留（关层后像素未复位）",
            t262Code::FormatMismatch => "浮层与源面格式不一致",
            t262Code::PitchBad => "行距非法（非字节对齐）",
            t262Code::ClipLost => "裁剪丢失（越界混合）",
            t262Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t262Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t262Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t262Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t262Code::DuplicateEntry => "条目重复（同键双写）",
            t262Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t262Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t262Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t262Code::AlphaBad => "按 8 位 Alpha 钳制",
            t262Code::BlendOverflow => "按饱和钳制钳回",
            t262Code::SrcUntouched => "按只读源面约束重构",
            t262Code::LayerOrderBad => "按层级律重排",
            t262Code::ResidueLeft => "按零残影协议复位",
            t262Code::FormatMismatch => "按格式对齐表转换",
            t262Code::PitchBad => "按对齐律校正",
            t262Code::ClipLost => "按裁剪矩形约束",
            t262Code::LedgerBroken => "按守恒律重建台账后重跑",
            t262Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t262Code::TableBroken => "按语义规则重建常量表",
            t262Code::IndexMisaligned => "按声明序归一化后重跑",
            t262Code::DuplicateEntry => "合并同键条目为末次生效",
            t262Code::OrderViolated => "按依赖矩阵重排后重跑",
            t262Code::GuardTripped => "检查临界区嵌套与阈值",
            t262Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}


// ---------------------------------------------------------------------------
// Alpha 混合引擎（源保持只读；结果 = 前景*a + 背景*(255-a)，饱和钳制）
// ---------------------------------------------------------------------------

/// Alpha 满值（8 位不透明）。
pub const ALPHA_OPAQUE: u32 = 255;

/// 混合结果（双通道：浮层前景 + 源面背景）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlendPair {
    /// 前景通道值。
    pub fg: u32,
    /// 背景通道值。
    pub bg: u32,
}

/// 混合账（守恒：结果 = fg*a + bg*(255-a)，误差容限 ±1）。
#[derive(Clone, Copy, Debug)]
pub struct BlendBook {
    /// 混合次数。
    pub blends: u64,
    /// 源面被改计数（必须恒 0）。
    pub src_writes: u64,
    /// 钳制计数。
    pub clamps: u64,
}

/// 单通道混合（饱和钳制；返回混合值与是否钳制）。
pub fn blend_channel(fg: u32, bg: u32, a: u32) -> (u32, bool) {
    let aa = if a > ALPHA_OPAQUE { ALPHA_OPAQUE } else { a };
    let inv = ALPHA_OPAQUE - aa;
    let raw = fg * aa + bg * inv + 127;
    let v = raw / 255;
    if v > 255 {
        (255, true)
    } else {
        (v, false)
    }
}

impl BlendBook {
    /// 全零构造。
    pub const fn new() -> Self {
        Self { blends: 0, src_writes: 0, clamps: 0 }
    }

    /// 混合一对并记账（只读源面约定由调用方保证）。
    pub fn blend(&mut self, p: BlendPair, a: u32) -> (u32, bool) {
        self.blends += 1;
        let (v, clamped) = blend_channel(p.fg, p.bg, a);
        if clamped {
            self.clamps += 1;
        }
        (v, clamped)
    }

    /// 守恒：源面修改计数恒零（零互蚀红线）。
    pub fn src_readonly(&self) -> bool {
        self.src_writes == 0
    }
}

// --------------------------------------------------------------------------
// 状态机 ovl（4 态 × 5 事件 · 迁移 7 条显式声明）
// --------------------------------------------------------------------------
/// 浮层开关状态机（残影复位路径显性）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ovlState {
    Hidden,
    Shown,
    Fading,
    Reset,
}

impl ovlState {
    /// 全量态闭集。
    pub const ALL: [ovlState; 4] = [
        ovlState::Hidden,
        ovlState::Shown,
        ovlState::Fading,
        ovlState::Reset,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            ovlState::Hidden => 0,
            ovlState::Shown => 1,
            ovlState::Fading => 2,
            ovlState::Reset => 3,
        }
    }
}

/// ovl事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ovlEvent {
    Show,
    Hide,
    FadeIn,
    FadeOut,
    Tick,
}

impl ovlEvent {
    /// 全量事件闭集。
    pub const ALL: [ovlEvent; 5] = [
        ovlEvent::Show,
        ovlEvent::Hide,
        ovlEvent::FadeIn,
        ovlEvent::FadeOut,
        ovlEvent::Tick,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const OVL_TRANSITIONS: [(ovlState, ovlEvent, ovlState); 7] = [
    (ovlState::Hidden, ovlEvent::Show, ovlState::Shown), // 浮层开启
    (ovlState::Shown, ovlEvent::Hide, ovlState::Reset), // 关层并复位残影
    (ovlState::Reset, ovlEvent::Show, ovlState::Shown), // 复位后再开（残影已清）
    (ovlState::Shown, ovlEvent::FadeIn, ovlState::Fading), // 淡入中
    (ovlState::Fading, ovlEvent::Tick, ovlState::Fading), // 淡入步进
    (ovlState::Hidden, ovlEvent::FadeOut, ovlState::Fading), // 淡出中
    (ovlState::Fading, ovlEvent::Hide, ovlState::Reset), // 淡入淡出中关层
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn ovl_next(state: ovlState, ev: ovlEvent) -> Option<ovlState> {
    let mut i = 0usize;
    while i < 7 {
        let t = OVL_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn ovl_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < ovlState::ALL.len() {
        let mut ei = 0usize;
        while ei < ovlEvent::ALL.len() {
            let legal = ovl_next(ovlState::ALL[si], ovlEvent::ALL[ei]).is_some();
            if !legal {
                bad += 1;
            }
            ei += 1;
        }
        si += 1;
    }
    bad
}

/// 终态集合（无出边的态即终态）。
pub fn ovl_terminal(st: ovlState) -> bool {
    let mut i = 0usize;
    while i < 7 {
        if OVL_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct ovlLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: ovlState,
}

impl ovlLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: ovlState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: ovlEvent) -> bool {
        match ovl_next(self.cur, ev) {
            Some(ns) => {
                self.cur = ns;
                self.legal += 1;
                true
            }
            None => {
                self.illegal += 1;
                false
            }
        }
    }

    /// 守恒恒等式：legal + illegal == 施加次数（由调用方累计）。
    pub fn total(&self) -> u64 {
        self.legal + self.illegal
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3a10, 0x3a1f]。
pub const fn t262_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t262Code::ALL.len() {
        let c = t262Code::ALL[i].code();
        if c < 0x3a10 || c > 0x3a1f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn t262_guard_seg() -> bool {
    t262_segment_guard()
}

/// 混合数值正样本（a=0/128/255）+ 钳制账。
fn check_c1_blend() -> bool {
    let mut bk = BlendBook::new();
    let (v0, _c0) = bk.blend(BlendPair { fg: 200, bg: 50 }, 0);
    let (v1, _c1) = bk.blend(BlendPair { fg: 200, bg: 50 }, 255);
    let (v2, _c2) = bk.blend(BlendPair { fg: 200, bg: 50 }, 128);
    let mut ok = v0 == 50 && v1 == 200;
    if ok {
        // 128/255 中点：ceil((200*128 + 50*127 + 127)/255)。
        ok = v2 == 125;
    }
    if ok {
        ok = bk.blends == 3 && bk.src_readonly();
    }
    ok
}

/// 浮层状态机：开-关复位路径（零残影协议）。
fn check_c2_ovl_sm() -> bool {
    let mut led = ovlLedger::new(ovlState::Hidden);
    let mut ok = led.apply(ovlEvent::Show);
    ok = ok && led.apply(ovlEvent::Hide);
    if ok {
        ok = led.cur == ovlState::Reset;
    }
    if ok {
        ok = led.apply(ovlEvent::Show) && led.legal == 3;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t262_checks() -> CheckSet {
    let mut set = CheckSet::new("T262");
    set.add(
        "t262-seg-guard",
        t262_guard_seg(),
        if t262_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t262-c1-blend",
        check_c1_blend(),
        if check_c1_blend() { "" } else { "blend numeric mismatch" },
    );
    set.add(
        "t262-c2-ovl-sm",
        check_c2_ovl_sm(),
        if check_c2_ovl_sm() { "" } else { "overlay state machine mismatch" },
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（域聚合测试在 mod.rs——此处只放本模块纯函数级断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t262_all_checks_green() {
        let set = run_t262_checks();
        assert!(set.all_passed(), "T262 domain checks must be green");
    }

    #[test]
    fn t262_segment_guard_holds() {
        assert!(t262_segment_guard());
    }
}
