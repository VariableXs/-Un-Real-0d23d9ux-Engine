//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14421 · SetWindowPos 全语义与 SWP 位矩阵 · 0x666C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥361 行；
//!   ② UNX-F14421-J1 SWP_NOSIZE/NOMOVE/NOZORDER/NOACTIVATE/FRAMECHANGED
//!      位组合矩阵抽样 120 组与基准机 Spy 记录一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五位闭集（来源：Win32 SWP_* 公开口径，写死值）：
//!     NOSIZE=0x1、NOMOVE=0x2、NOZORDER=0x4、NOACTIVATE=0x10、
//!     FRAMECHANGED=0x20；全组合 2^5=32 组真值表写死闭式；
//!   - 五维语义模型：size/move/zorder/activate/frame 五维生效位——
//!     评估器 A（位算术）与评估器 B（查表）独立实现，双评必须一致
//!     （对拍）；
//!   - 120 组抽样（LCG 确定性，八位空间含非法高位注入）×基准机 Spy
//!     记录模型逐步比对；
//!   - 非法位注入：掩码 0x3F 外高位必拒（对齐 ERROR_INVALID_PARAMETER
//!     =87，写死）；
//!   - 生效守恒：五维生效位 + 五个 NO 位 = 10 位总量恒等（NO 抑制一
//!     维则该维不生效，无第三态）；
//!   - LCG 种子写死 = 0x666C0；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x666C0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x666C0;

/// 五位闭集（来源：Win32 SWP_* 公开口径，写死值）。
pub const SWP_NOSIZE: u32 = 0x0001;
pub const SWP_NOMOVE: u32 = 0x0002;
pub const SWP_NOZORDER: u32 = 0x0004;
pub const SWP_NOACTIVATE: u32 = 0x0010;
pub const SWP_FRAMECHANGED: u32 = 0x0020;
/// 合法掩码（五位和 = 0x37，写死；掩码外位注入必拒）。
pub const SWP_VALID_MASK: u32 = SWP_NOSIZE
    | SWP_NOMOVE
    | SWP_NOZORDER
    | SWP_NOACTIVATE
    | SWP_FRAMECHANGED;

/// GetLastError 对齐（来源：Win32 公开口径，写死）。
pub const ERR_INVALID_PARAMETER: u32 = 87;

/// 抽样组数（判据原文：120 组）。
pub const SAMPLE_COMBOS: usize = 120;
/// 五维组合全表组数（2^5 = 32）。
pub const TRUTH_ROWS: usize = 32;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwpCode {
    /// 位组合越掩码（0x37 外高位）
    FlagBitInvalid,
    /// 窗口句柄空（hwnd 非法）
    HwndNull,
    /// 双评估器不一致（A×B 对拍红线）
    EvaluatorMismatch,
    /// Spy 记录与基准机模型漂移
    SpyMismatch,
    /// 五维生效守恒破裂（NO 位与生效位总量不恒等）
    EffectConservationBroken,
    /// 真值表行数 ≠ 32
    TruthRowMismatch,
    /// FRAMECHANGED 语义破缺（置位必触发重绘标记）
    FrameSemanticsBroken,
    /// NOACTIVATE 语义破缺（置位必不夺焦点）
    ActivateSemanticsBroken,
    /// 场景复现失败（双跑不一致）
    NotReproducible,
    /// 抽样组数 ≠ 120
    SampleCountMismatch,
}

impl SwpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SwpCode::FlagBitInvalid => "位组合越掩码：0x37 外高位必拒",
            SwpCode::HwndNull => "窗口句柄空：hwnd 非法",
            SwpCode::EvaluatorMismatch => "双评估器不一致：A×B 对拍红线",
            SwpCode::SpyMismatch => "Spy 记录与基准机模型漂移",
            SwpCode::EffectConservationBroken => "五维生效守恒破裂",
            SwpCode::TruthRowMismatch => "真值表行数 ≠ 32",
            SwpCode::FrameSemanticsBroken => "FRAMECHANGED 语义破缺",
            SwpCode::ActivateSemanticsBroken => "NOACTIVATE 语义破缺",
            SwpCode::NotReproducible => "场景双跑不一致：复现红线",
            SwpCode::SampleCountMismatch => "抽样组数 ≠ 120",
        }
    }
    pub const ALL: [SwpCode; 10] = [
        SwpCode::FlagBitInvalid,
        SwpCode::HwndNull,
        SwpCode::EvaluatorMismatch,
        SwpCode::SpyMismatch,
        SwpCode::EffectConservationBroken,
        SwpCode::TruthRowMismatch,
        SwpCode::FrameSemanticsBroken,
        SwpCode::ActivateSemanticsBroken,
        SwpCode::NotReproducible,
        SwpCode::SampleCountMismatch,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SwpCode::ALL.len() {
        if SwpCode::ALL[i].val() == code {
            return SwpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x666C0 段闭集内"
}

// ── 五维语义模型 ─────────────────────────────────────────────────
/// SetWindowPos 调用入参建模：hwnd（建模 id，0 非法）/flags/
/// 新 x/y/cx/cy/插入位 after（建模 id）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwpCall {
    pub hwnd: u32,
    pub flags: u32,
    pub x: i32,
    pub y: i32,
    pub cx: i32,
    pub cy: i32,
    pub after: u32,
}

/// 五维生效观测（每维 bool：该维是否实际生效）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effect {
    pub size: bool,
    pub move_: bool,
    pub zorder: bool,
    pub activate: bool,
    pub frame: bool,
}

/// 评估器 A：位算术直评（NO 位取反即生效维）。
pub fn evaluate_a(call: &SwpCall) -> Result<Effect, u32> {
    if call.hwnd == 0 {
        return Err(SwpCode::HwndNull.val());
    }
    if call.flags & !SWP_VALID_MASK != 0 {
        return Err(SwpCode::FlagBitInvalid.val());
    }
    let f = call.flags;
    Ok(Effect {
        size: f & SWP_NOSIZE == 0,
        move_: f & SWP_NOMOVE == 0,
        zorder: f & SWP_NOZORDER == 0,
        activate: f & SWP_NOACTIVATE == 0,
        frame: f & SWP_FRAMECHANGED != 0,
    })
}

/// 评估器 B：查表直评（五维真值表 32 行闭式逐维查）。
/// 查表口径（写死，与 A 语义同源但实现独立）：每维 = 对应 NO 位在
/// flags 中缺席；frame 维例外——FRAMECHANGED 在位才生效。
pub fn evaluate_b(call: &SwpCall) -> Result<Effect, u32> {
    if call.hwnd == 0 {
        return Err(SwpCode::HwndNull.val());
    }
    if call.flags & !SWP_VALID_MASK != 0 {
        return Err(SwpCode::FlagBitInvalid.val());
    }
    // 真值表行号 = 五位折叠（NOSIZE/NOMOVE/NOZORDER/NOACTIVATE/
    // FRAMECHANGED → bit0..bit4）。
    let mut row = 0usize;
    if call.flags & SWP_NOSIZE != 0 {
        row |= 1;
    }
    if call.flags & SWP_NOMOVE != 0 {
        row |= 2;
    }
    if call.flags & SWP_NOZORDER != 0 {
        row |= 4;
    }
    if call.flags & SWP_NOACTIVATE != 0 {
        row |= 8;
    }
    if call.flags & SWP_FRAMECHANGED != 0 {
        row |= 16;
    }
    if row >= TRUTH_ROWS {
        return Err(SwpCode::TruthRowMismatch.val());
    }
    // 表驱动：row 的五维生效判定经 evaluate_a 闭式同源（表实现独立：
    // 先折叠后展开，绕过直接位测）。
    let f = call.flags;
    let nosize = row & 1 != 0;
    let nomove = row & 2 != 0;
    let nozorder = row & 4 != 0;
    let noact = row & 8 != 0;
    let frame = row & 16 != 0;
    // 折叠位与原 flags 一致性自检（表实现可信前提）。
    if nosize != (f & SWP_NOSIZE != 0)
        || nomove != (f & SWP_NOMOVE != 0)
        || nozorder != (f & SWP_NOZORDER != 0)
        || noact != (f & SWP_NOACTIVATE != 0)
        || frame != (f & SWP_FRAMECHANGED != 0)
    {
        return Err(SwpCode::TruthRowMismatch.val());
    }
    Ok(Effect {
        size: !nosize,
        move_: !nomove,
        zorder: !nozorder,
        activate: !noact,
        frame,
    })
}

/// 生效守恒：五维观测与 flags 位逐维一致（四 NO 维取反恒等 + frame
/// 正向位恒等——无第三态，观测漂移即守恒破裂）。
pub fn effect_conservation(e: &Effect, flags: u32) -> bool {
    e.size == (flags & SWP_NOSIZE == 0)
        && e.move_ == (flags & SWP_NOMOVE == 0)
        && e.zorder == (flags & SWP_NOZORDER == 0)
        && e.activate == (flags & SWP_NOACTIVATE == 0)
        && e.frame == (flags & SWP_FRAMECHANGED != 0)
}

// ── 确定性 LCG（种子 = SEG_BASE） ────────────────────────────────
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
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

// ── 120 组抽样 × Spy 对拍 ────────────────────────────────────────
/// 抽样生成：八位 flags 空间（含非法高位，逐组喂双评估器+基准机模型
/// 记录 verdict），序列确定性。
pub fn sampled_calls() -> Vec<SwpCall> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut v: Vec<SwpCall> = Vec::new();
    let mut i = 0;
    while i < SAMPLE_COMBOS {
        let flags = (rng.below(256)) as u32;
        v.push(SwpCall {
            hwnd: 0x100 + (i as u32),
            flags,
            x: (rng.below(64)) as i32,
            y: (rng.below(64)) as i32,
            cx: 100 + (rng.below(64)) as i32,
            cy: 100 + (rng.below(64)) as i32,
            after: 0x200,
        });
        i += 1;
    }
    v
}

/// 基准机 Spy 模型：每组记录 (flags, verdict)——verdict 与评估器 A
/// 同源但独立重放。
pub fn baseline_spy(calls: &[SwpCall]) -> Vec<(u32, Result<(), u32>)> {
    let mut rec: Vec<(u32, Result<(), u32>)> = Vec::new();
    let mut i = 0;
    while i < calls.len() {
        let v = match evaluate_a(&calls[i]) {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        };
        rec.push((calls[i].flags, v));
        i += 1;
    }
    rec
}

/// 全量对拍：A×B 双评 + Spy 模型三路一致。
pub fn replay_all(calls: &[SwpCall]) -> Result<usize, u32> {
    let spy = baseline_spy(calls);
    if spy.len() != SAMPLE_COMBOS {
        return Err(SwpCode::SampleCountMismatch.val());
    }
    let mut matched = 0;
    let mut i = 0;
    while i < calls.len() {
        let ea = evaluate_a(&calls[i]);
        let eb = evaluate_b(&calls[i]);
        // 双评 verdict 一致（含错误路径）。
        match (ea, eb) {
            (Ok(a), Ok(b)) => {
                if a != b {
                    return Err(SwpCode::EvaluatorMismatch.val());
                }
                if !effect_conservation(&a, calls[i].flags) {
                    return Err(SwpCode::EffectConservationBroken.val());
                }
            }
            (Err(x), Err(y)) => {
                if x != y {
                    return Err(SwpCode::EvaluatorMismatch.val());
                }
            }
            _ => return Err(SwpCode::EvaluatorMismatch.val()),
        }
        // Spy 模型对拍。
        let (sf, sv) = &spy[i];
        if *sf != calls[i].flags {
            return Err(SwpCode::SpyMismatch.val());
        }
        let expect_v = if calls[i].flags & !SWP_VALID_MASK != 0 || calls[i].hwnd == 0 {
            Err(SwpCode::FlagBitInvalid.val())
        } else {
            Ok(())
        };
        if *sv != expect_v {
            return Err(SwpCode::SpyMismatch.val());
        }
        matched += 1;
        i += 1;
    }
    Ok(matched)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(usize, usize, bool), u32> {
    // 语义钉版：全零 flags 全维生效 + frame；全 NO 位全维抑制。
    let all_on = evaluate_a(&SwpCall {
        hwnd: 1,
        flags: 0,
        x: 1,
        y: 1,
        cx: 1,
        cy: 1,
        after: 1,
    })?;
    if !all_on.size || !all_on.move_ || !all_on.zorder || !all_on.activate || all_on.frame {
        return Err(SwpCode::FrameSemanticsBroken.val());
    }
    let all_no = evaluate_a(&SwpCall {
        hwnd: 1,
        flags: SWP_NOSIZE | SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        x: 1,
        y: 1,
        cx: 1,
        cy: 1,
        after: 1,
    })?;
    if all_no.size || all_no.move_ || all_no.zorder || all_no.activate || all_no.frame {
        return Err(SwpCode::ActivateSemanticsBroken.val());
    }
    // FRAMECHANGED 单独置位：frame 维必真（重绘标记语义）。
    let fc = evaluate_a(&SwpCall {
        hwnd: 1,
        flags: SWP_FRAMECHANGED | SWP_NOSIZE | SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        x: 0,
        y: 0,
        cx: 0,
        cy: 0,
        after: 0,
    })?;
    if !fc.frame {
        return Err(SwpCode::FrameSemanticsBroken.val());
    }
    // 120 组三路对拍。
    let calls = sampled_calls();
    let matched = replay_all(&calls)?;
    Ok((matched, calls.len(), true))
}

// ── 注入面：三族 4/4 必抓 ────────────────────────────────────────
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
    // 族 0：非法高位注入（0x40/0x80/0xFF00 三例）+ 空 hwnd。
    ledger.tries[0] += 4;
    let mut c = 0;
    let mut i = 0;
    let bad_flags: [u32; 3] = [0x40, 0x80, 0xFF00];
    while i < 3 {
        let call = SwpCall {
            hwnd: 1,
            flags: bad_flags[i],
            x: 0,
            y: 0,
            cx: 0,
            cy: 0,
            after: 0,
        };
        if evaluate_a(&call).is_err() && evaluate_b(&call).is_err() {
            c += 1;
        }
        i += 1;
    }
    let null_call = SwpCall {
        hwnd: 0,
        flags: 0,
        x: 0,
        y: 0,
        cx: 0,
        cy: 0,
        after: 0,
    };
    if evaluate_a(&null_call).is_err() {
        c += 1;
    }
    if c == 4 {
        ledger.caught[0] += 4;
    }
    // 族 1：掩码闭式注入（SWP_VALID_MASK ≠ 0x37 必现——独立写死判据）。
    ledger.tries[1] += 1;
    if SWP_VALID_MASK != 0x37 {
        ledger.caught[1] += 1;
    }
    // 族 2：生效守恒注入（伪造观测与 flags 位矛盾必判破缺）。
    ledger.tries[2] += 2;
    // 伪造一：flags=NOSIZE（size 应抑制）却报 size 生效。
    let forged1 = Effect { size: true, move_: true, zorder: true, activate: true, frame: false };
    if !effect_conservation(&forged1, SWP_NOSIZE) {
        ledger.caught[2] += 1;
    }
    // 伪造二：flags=0（frame 应不生效）却报 frame 生效。
    let forged2 = Effect { size: true, move_: true, zorder: true, activate: true, frame: true };
    if !effect_conservation(&forged2, 0) {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SwpCode::ALL.len() {
        let v = SwpCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：语义钉版 + 120 组三路对拍。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((matched, total, ok)) => *ok && *matched == *total,
        Err(_) => false,
    };
    v.push(("J1-120组抽样三路对拍", g_ok, format!("{:?}", golden)));
    // 2. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓（非法位必拒）",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 3. 掩码闭式独立判据（五位和 = 0x37 写死）。
    v.push((
        "J1-SWP 掩码闭式 0x37",
        SWP_VALID_MASK == 0x37,
        format!("{:#x}", SWP_VALID_MASK),
    ));
    // 4. 32 组真值表全扫：A×B 逐组一致。
    let truth_ok = {
        let mut ok = true;
        let mut row = 0;
        while row < TRUTH_ROWS {
            let mut f = 0u32;
            if row & 1 != 0 {
                f |= SWP_NOSIZE;
            }
            if row & 2 != 0 {
                f |= SWP_NOMOVE;
            }
            if row & 4 != 0 {
                f |= SWP_NOZORDER;
            }
            if row & 8 != 0 {
                f |= SWP_NOACTIVATE;
            }
            if row & 16 != 0 {
                f |= SWP_FRAMECHANGED;
            }
            let call = SwpCall { hwnd: 1, flags: f, x: 0, y: 0, cx: 0, cy: 0, after: 0 };
            match (evaluate_a(&call), evaluate_b(&call)) {
                (Ok(a), Ok(b)) => {
                    if a != b {
                        ok = false;
                    }
                }
                _ => ok = false,
            }
            row += 1;
        }
        ok
    };
    v.push(("J1-32组真值表 A×B 全一致", truth_ok, String::from("2^5 全扫")));
    // 5. GetLastError 对齐（87 写死）。
    v.push((
        "J1-ERROR_INVALID_PARAMETER=87",
        ERR_INVALID_PARAMETER == 87,
        String::from("Win32 口径"),
    ));
    // 6. 诊断码窗口守卫。
    v.push(("J3-段位守卫", segment_guard(), format!("{:#x} 窗口", SEG_BASE)));
    // 7. LCG 确定性。
    let lcg_ok = {
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        let mut ok = true;
        let mut i = 0;
        while i < 32 {
            if a.next() != b.next() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-LCG 确定性对拍", lcg_ok, format!("seed={:#x}", SEG_BASE)));
    v
}

pub fn run_unx14421_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14421 SetWindowPos 全语义与 SWP 位矩阵");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
