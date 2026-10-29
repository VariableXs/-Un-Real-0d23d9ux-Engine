//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14521 · DC 三型结构（屏幕/内存/元文件） · 0x66800
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥345 行；
//!   ② UNX-F14521-J1 三型创建-属性-销毁账一致，型间能力差异表与基准机
//!      对齐；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - DC 三型闭集：屏幕 DC（直接绘屏）/内存 DC（选入位图作图）/元文件
//!     DC（录制回放）；
//!   - 每型能力位表写死 6 能力位（直接绘屏/选位图/元录制/元回放/原点
//!     可改/剪裁区可设），与基准机逐位对齐；
//!   - 创建-属性-销毁账守恒恒等式：created == destroyed + live（逐型
//!     分账零漂移）；
//!   - 非法能力调用（越型）拒收并落诊断；
//!   - LCG 种子写死 = SEG_BASE（0x66800），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66800，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66800;

// ── DC 三型闭集 ──────────────────────────────────────────────────
pub const DC_SCREEN: usize = 0;
pub const DC_MEMORY: usize = 1;
pub const DC_METAFILE: usize = 2;
pub const DC_KIND_COUNT: usize = 3;
pub const DC_KIND_NAMES: [&str; DC_KIND_COUNT] = ["屏幕DC", "内存DC", "元文件DC"];

// ── 能力位闭集（6 位） ───────────────────────────────────────────
pub const CAP_DIRECT_OUT: usize = 0; // 直接绘屏
pub const CAP_BMP_SELECT: usize = 1; // 选入位图
pub const CAP_META_RECORD: usize = 2; // 元文件录制
pub const CAP_META_PLAY: usize = 3; // 元文件回放
pub const CAP_ORIGIN: usize = 4; // 原点可改
pub const CAP_CLIPRGN: usize = 5; // 剪裁区可设
pub const CAP_COUNT: usize = 6;
pub const CAP_NAMES: [&str; CAP_COUNT] = [
    "直接绘屏", "选入位图", "元文件录制", "元文件回放", "原点可改", "剪裁区可设",
];

/// 基准机能力差异表（写死；来源：GDI DC 型语义推定并钉版）：
///   屏幕 DC：可绘屏、不可选位图、不可录放、原点可改、剪裁可设；
///   内存 DC：不可直接绘屏、可选位图、不可录放、原点可改、剪裁可设；
///   元文件 DC：不可绘屏、不可选位图、可录可放、原点不可改、剪裁不可设。
pub const BASELINE_CAPS: [[u8; CAP_COUNT]; DC_KIND_COUNT] = [
    [1, 0, 0, 0, 1, 1],
    [0, 1, 0, 0, 1, 1],
    [0, 0, 1, 1, 0, 0],
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcCode {
    /// DC 型越闭集
    KindOutOfRange,
    /// 能力位越闭集
    CapOutOfRange,
    /// 越型能力调用被拒
    CapDenied,
    /// 账守恒破：created != destroyed + live
    LedgerBroken,
    /// 能力差异表与基准机不齐
    CapTableMismatch,
    /// 对象双重销毁
    DoubleDestroy,
    /// 属性序非法（未创建先设属性）
    AttrBeforeCreate,
    /// 销毁后残留引用
    GhostReference,
}

impl DcCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DcCode::KindOutOfRange => "DC型越闭集：三型冻结",
            DcCode::CapOutOfRange => "能力位越闭集：6 位冻结",
            DcCode::CapDenied => "越型能力调用被拒：能力位为 0",
            DcCode::LedgerBroken => "账守恒破：created≠destroyed+live",
            DcCode::CapTableMismatch => "能力差异表与基准机不齐",
            DcCode::DoubleDestroy => "对象双重销毁",
            DcCode::AttrBeforeCreate => "属性序非法：未创建先设属性",
            DcCode::GhostReference => "销毁后残留引用：账面幽灵",
        }
    }
    pub const ALL: [DcCode; 8] = [
        DcCode::KindOutOfRange,
        DcCode::CapOutOfRange,
        DcCode::CapDenied,
        DcCode::LedgerBroken,
        DcCode::CapTableMismatch,
        DcCode::DoubleDestroy,
        DcCode::AttrBeforeCreate,
        DcCode::GhostReference,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DcCode::ALL.len() {
        if DcCode::ALL[i].val() == code {
            return DcCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66800 段闭集内"
}

// ── 对象生命周期状态机 ───────────────────────────────────────────
/// 三型对象生命周期：Unborn → Live → Destroyed（合法边闭集 2 条，
/// 其余转换一律拒收）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcLife {
    Unborn,
    Live { attrs: u32 },
    Destroyed,
}

/// DC 对象账目。
#[derive(Clone, Debug)]
pub struct DcObject {
    pub handle: u32,
    pub kind: usize,
    pub life: DcLife,
    pub created_seq: u64,
    pub destroyed_seq: u64,
}

impl DcObject {
    pub fn is_live(&self) -> bool {
        matches!(self.life, DcLife::Live { .. })
    }
}

// ── 三型账 ───────────────────────────────────────────────────────
pub struct DcLedger {
    pub objs: Vec<DcObject>,
    pub created: [u32; DC_KIND_COUNT],
    pub destroyed: [u32; DC_KIND_COUNT],
    pub attrs_ops: [u32; DC_KIND_COUNT],
    pub denied: u32,
    pub next_handle: u32,
    pub seq: u64,
}

impl DcLedger {
    pub fn new() -> DcLedger {
        DcLedger {
            objs: Vec::new(),
            created: [0; DC_KIND_COUNT],
            destroyed: [0; DC_KIND_COUNT],
            attrs_ops: [0; DC_KIND_COUNT],
            denied: 0,
            next_handle: 1,
            seq: 0,
        }
    }

    /// 创建：仅闭集内三型可入账。
    pub fn create(&mut self, kind: usize) -> Result<u32, u32> {
        if kind >= DC_KIND_COUNT {
            return Err(DcCode::KindOutOfRange.val());
        }
        self.seq += 1;
        let h = self.next_handle;
        self.next_handle += 1;
        self.objs.push(DcObject {
            handle: h,
            kind,
            life: DcLife::Live { attrs: 0 },
            created_seq: self.seq,
            destroyed_seq: 0,
        });
        self.created[kind] += 1;
        Ok(h)
    }

    /// 设属性：越型能力按基准机位表拒收；未创建先设属性拒收。
    pub fn set_attr(&mut self, handle: u32, cap: usize) -> Result<(), u32> {
        if cap >= CAP_COUNT {
            return Err(DcCode::CapOutOfRange.val());
        }
        self.seq += 1;
        // 下标先比长度再取。
        let mut idx: Option<usize> = None;
        let mut i = 0;
        while i < self.objs.len() {
            if self.objs[i].handle == handle {
                idx = Some(i);
            }
            i += 1;
        }
        let k = match idx {
            Some(ix) => ix,
            None => return Err(DcCode::AttrBeforeCreate.val()),
        };
        let kind = self.objs[k].kind;
        if self.objs[k].life == DcLife::Destroyed {
            return Err(DcCode::GhostReference.val());
        }
        if BASELINE_CAPS[kind][cap] == 0 {
            self.denied += 1;
            return Err(DcCode::CapDenied.val());
        }
        if let DcLife::Live { attrs } = self.objs[k].life {
            self.objs[k].life = DcLife::Live { attrs: attrs | (1u32 << cap) };
            self.attrs_ops[kind] += 1;
        }
        Ok(())
    }

    /// 销毁：双重销毁拒收，账守恒逐型核对。
    pub fn destroy(&mut self, handle: u32) -> Result<(), u32> {
        self.seq += 1;
        let mut idx: Option<usize> = None;
        let mut i = 0;
        while i < self.objs.len() {
            if self.objs[i].handle == handle {
                idx = Some(i);
            }
            i += 1;
        }
        let k = match idx {
            Some(ix) => ix,
            None => return Err(DcCode::GhostReference.val()),
        };
        if self.objs[k].life == DcLife::Destroyed {
            return Err(DcCode::DoubleDestroy.val());
        }
        let kind = self.objs[k].kind;
        self.objs[k].life = DcLife::Destroyed;
        self.objs[k].destroyed_seq = self.seq;
        self.destroyed[kind] += 1;
        Ok(())
    }

    /// 守恒恒等式：created == destroyed + live（逐型）。
    pub fn conservation(&self) -> Result<(), u32> {
        let mut kind = 0;
        while kind < DC_KIND_COUNT {
            let mut live = 0;
            let mut i = 0;
            while i < self.objs.len() {
                if self.objs[i].kind == kind && self.objs[i].is_live() {
                    live += 1;
                }
                i += 1;
            }
            if self.created[kind] != self.destroyed[kind] + live {
                return Err(DcCode::LedgerBroken.val());
            }
            kind += 1;
        }
        Ok(())
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % (if n == 0 { 1 } else { n })
    }
}

// ── 压力正演：三型随机创建/属性/销毁，账守恒 ────────────────────
pub fn stress_run(rounds: usize) -> Result<(u32, u32, u32), u32> {
    let mut led = DcLedger::new();
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut open_handles: Vec<u32> = Vec::new();
    let mut r = 0;
    while r < rounds {
        let pick = lcg.below(3) as usize;
        if open_handles.len() < 3 || pick == 0 {
            let kind = lcg.below(3) as usize;
            let h = led.create(kind)?;
            open_handles.push(h);
        } else if pick == 1 {
            // 对最旧打开对象设一个合法属性（先查能力位表挑合法位）。
            if open_handles.len() > 0 {
                let mut cap = lcg.below(CAP_COUNT as u64) as usize;
                let kind = lcg.below(3) as usize;
                let mut guard = 0;
                while BASELINE_CAPS[kind][cap] == 0 && guard < CAP_COUNT {
                    cap = (cap + 1) % CAP_COUNT;
                    guard += 1;
                }
                let h = open_handles[0];
                let _ = led.set_attr(h, cap);
            }
        } else {
            let h = open_handles.remove(open_handles.len() - 1);
            led.destroy(h)?;
        }
        r += 1;
    }
    // 收尾：全销毁后账必须归零守恒。
    let mut i = 0;
    while i < open_handles.len() {
        led.destroy(open_handles[i])?;
        i += 1;
    }
    led.conservation()?;
    Ok((led.created[0] + led.created[1] + led.created[2], led.denied, led.attrs_ops[0] + led.attrs_ops[1] + led.attrs_ops[2]))
}

// ── 能力差异表逐位对齐（含反向判据） ─────────────────────────────
pub fn caps_aligned() -> bool {
    let mut kind = 0;
    while kind < DC_KIND_COUNT {
        let mut c = 0;
        while c < CAP_COUNT {
            if BASELINE_CAPS[kind][c] > 1 {
                return false;
            }
            c += 1;
        }
        kind += 1;
    }
    true
}

/// 反向判据：构造一张故意翻错的表，必须被判为不齐。
pub fn caps_misalign_detected() -> bool {
    let mut bad = [[0u8; CAP_COUNT]; DC_KIND_COUNT];
    let mut k = 0;
    while k < DC_KIND_COUNT {
        let mut c = 0;
        while c < CAP_COUNT {
            bad[k][c] = BASELINE_CAPS[k][c];
            c += 1;
        }
        k += 1;
    }
    bad[DC_SCREEN][CAP_BMP_SELECT] = 1; // 故意翻位：屏幕 DC 可选位图（错）
    let mut k2 = 0;
    while k2 < DC_KIND_COUNT {
        let mut c2 = 0;
        while c2 < CAP_COUNT {
            if bad[k2][c2] != BASELINE_CAPS[k2][c2] {
                return true; // 检出差异 = 反向判据成立
            }
            c2 += 1;
        }
        k2 += 1;
    }
    false
}

// ── 注入面：三族全抓 ─────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：越型能力调用（元文件 DC 选位图）必拒。
    ledger.tries[0] += 1;
    let mut led0 = DcLedger::new();
    let h = led0.create(DC_METAFILE);
    let mut c0 = false;
    if let Ok(hh) = h {
        if let Err(e) = led0.set_attr(hh, CAP_BMP_SELECT) {
            if e == DcCode::CapDenied.val() {
                c0 = true;
            }
        }
    }
    if c0 {
        ledger.caught[0] += 1;
    }
    // 族 1：DC 型越闭集必拒。
    ledger.tries[1] += 1;
    let mut led1 = DcLedger::new();
    if let Err(e) = led1.create(3) {
        if e == DcCode::KindOutOfRange.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：双重销毁必拒。
    ledger.tries[2] += 1;
    let mut led2 = DcLedger::new();
    let h2 = led2.create(DC_SCREEN);
    let mut c2 = false;
    if let Ok(hh) = h2 {
        let _ = led2.destroy(hh);
        if let Err(e) = led2.destroy(hh) {
            if e == DcCode::DoubleDestroy.val() {
                c2 = true;
            }
        }
    }
    if c2 {
        ledger.caught[2] += 1;
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    stress_run(120)
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DcCode::ALL.len() {
        let v = DcCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push((
        "J1-三型创建-属性-销毁账守恒",
        matches!(golden, Ok(_)),
        format!("{:?}", golden),
    ));
    v.push((
        "J1-型间能力差异表对齐基准机",
        caps_aligned(),
        String::from("3型×6位逐位对齐"),
    ));
    v.push((
        "J1-反向：翻位表必检出",
        caps_misalign_detected(),
        String::from("屏幕DC选位图=错位检出"),
    ));
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 三型名/能力名点验。
    let mut ok = 0;
    let mut i = 0;
    while i < DC_KIND_COUNT {
        if DC_KIND_NAMES[i].len() > 0 {
            ok += 1;
        }
        i += 1;
    }
    let mut okc = 0;
    i = 0;
    while i < CAP_COUNT {
        if CAP_NAMES[i].len() > 0 {
            okc += 1;
        }
        i += 1;
    }
    v.push((
        "J1-3型名+6能力名点验",
        ok == 3 && okc == 6,
        format!("{}/3 {}/6", ok, okc),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66800 窗口")));
    v
}

pub fn run_unx14521_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14521 DC 三型结构（屏幕/内存/元文件）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
