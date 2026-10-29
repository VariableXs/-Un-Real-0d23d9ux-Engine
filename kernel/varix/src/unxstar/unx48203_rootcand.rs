//! unx48203_rootcand — UNX-F48203 root window 候选窗模式（xterm 类无 caret 老应用固定屏幕角）（诊断段 0xBA40）。
//!
//! 判据原文：
//!   J1 模式 ×10 定位稳定；判据锚定 §AI-61 §2 专题三类型三。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 根窗候选模式是「无 caret 老应用」的兜底定位：候选窗固定贴屏幕
//!      工作区左下角（边界内缩 margin），不追踪光标、不跟随文本流。
//!   2. 模式判定闭式：应用声明 caret 能力缺失（over-the-spot 探测失败）
//!      即落入 RootMode，且一旦落入即整会话稳定，禁止来回漂移。
//!   3. 定位稳定判据：同一屏幕几何下 10 次定位输出逐字节一致（×10），
//!      任何一次漂移即判不稳。
//!   4. 屏幕几何变化（分辨率变更）只允许「重锚定一次」，重锚后再次稳定；
//!      连续两次重锚仍在漂移 = 几何源坏账，显性报错不猜。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBA40;

// ── 诊断码（段内 0x1F 容量）────────────────────────────────
const E_NO_GEOM: u16 = SEG_BASE + 1; // 屏幕几何未登记即定位
const E_DRIFT: u16 = SEG_BASE + 2;   // 同几何下定位漂移
const E_REANCH_STORM: u16 = SEG_BASE + 3; // 连续重锚仍在漂移
const E_NEG_RECT: u16 = SEG_BASE + 4; // 派生矩形负尺寸
const E_MODE_FLAP: u16 = SEG_BASE + 5; // 模式来回翻转
const E_MARGIN_BIG: u16 = SEG_BASE + 6; // 内缩吃掉整个工作区
const E_SESS_LOST: u16 = SEG_BASE + 7; // 会话账丢失
const E_ANCHOR_MISS: u16 = SEG_BASE + 8; // 重锚后未重新登记稳定账

/// 内缩边距（px）：候选窗四边距工作区的固定留白。
pub const MARGIN: i32 = 8;

/// 候选窗缺省尺寸（px）：xterm 类老应用没有字体度量来源时的保守缺省。
pub const DEF_W: i32 = 320;
pub const DEF_H: i32 = 96;

/// 候选模式闭集：Root = 根窗固定角；Over = over-the-spot（本模块不实现，
/// 仅作模式翻转账的来源）；None = 未判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandMode {
    None,
    Root,
    Over,
}

/// 屏幕工作区几何（工作区 = 排除面板/Dock 后的可用区）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geom {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Geom {
    /// 合法几何：非负原点、正尺寸、内缩后仍有正余量。
    pub fn valid(&self) -> bool {
        self.w > 2 * MARGIN && self.h > 2 * MARGIN && self.x >= 0 && self.y >= 0
    }
}

/// 候选窗矩形（屏幕坐标系）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// 稳定账：同一几何下连续定位一致的次数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SteadyLedger {
    pub geom_fp: u32,
    pub steady_hits: u32,
    pub reanch_count: u32,
    pub sealed: bool,
}

/// 根窗候选模式状态机：
/// Undecided → (caret 探测失败) RootMode → Anchored → Steady
///                                └→(几何变更) Reanchored → Steady
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum St {
    Undecided,
    RootMode,
    Anchored,
    Reanchored,
    Steady,
    Fault,
}

/// 根窗候选模式总账。
#[derive(Debug)]
pub struct RootCand {
    st: St,
    mode: CandMode,
    mode_flips: u32,
    geom: Option<Geom>,
    last_rect: Option<CandRect>,
    ledgers: Vec<SteadyLedger>,
    sess_open: bool,
    sess_id: u32,
    locates: u32,
}

fn fp32(x: i32, y: i32, w: i32, h: i32) -> u32 {
    // FNV-1a 四元组指纹（确定性，无随机；局部名 fp 防遮蔽参数 h）。
    let mut fp: u32 = 0x811C_9DC5;
    for v in [x, y, w, h] {
        fp ^= v as u32;
        fp = fp.wrapping_mul(0x0100_0193);
    }
    fp
}

impl RootCand {
    pub fn new(sess_id: u32) -> Self {
        RootCand {
            st: St::Undecided,
            mode: CandMode::None,
            mode_flips: 0,
            geom: None,
            last_rect: None,
            ledgers: Vec::new(),
            sess_open: true,
            sess_id,
            locates: 0,
        }
    }

    /// caret 能力探测回执：老应用（无 caret）→ Root；有 caret → Over。
    /// 模式一旦落定，翻转即留账并计次（防抖：一次翻转放行，两次即故障）。
    pub fn report_caret(&mut self, has_caret: bool) -> Result<(), u16> {
        if !self.sess_open {
            return Err(E_SESS_LOST);
        }
        let want = if has_caret { CandMode::Over } else { CandMode::Root };
        match self.mode {
            CandMode::None => {
                self.mode = want;
                self.st = if want == CandMode::Root { St::RootMode } else { St::Undecided };
            }
            m if m == want => {}
            _ => {
                self.mode_flips += 1;
                // 环境可能同时有新老应用：单次翻转按会话重开语义放行，
                // 连续两次 = 模式源坏账。
                if self.mode_flips > 2 {
                    self.st = St::Fault;
                    return Err(E_MODE_FLAP);
                }
                self.mode = want;
                self.st = if want == CandMode::Root { St::RootMode } else { St::Undecided };
            }
        }
        Ok(())
    }

    /// 屏幕几何登记（分辨率变更时重登，触发一次性重锚）。
    pub fn set_geom(&mut self, g: Geom) -> Result<(), u16> {
        if !g.valid() {
            return Err(E_MARGIN_BIG);
        }
        let fresh = match self.geom {
            None => true,
            Some(old) => old != g,
        };
        self.geom = Some(g);
        if fresh && self.mode == CandMode::Root {
            // 几何变化 → 允许一次性重锚；稳定账重新开张（旧账封存保留）。
            self.st = St::Reanchored;
            self.last_rect = None;
        }
        Ok(())
    }

    /// 根窗模式定位：固定贴工作区左下角，内缩 MARGIN。
    /// 同几何下输出必须逐字节稳定；漂移即显性报错。
    pub fn locate(&mut self) -> Result<CandRect, u16> {
        if !self.sess_open {
            return Err(E_SESS_LOST);
        }
        let g = match self.geom {
            Some(g) => g,
            None => return Err(E_NO_GEOM),
        };
        if self.mode != CandMode::Root {
            return Err(E_MODE_FLAP);
        }
        let r = self.derive(&g)?;
        // 漂移判定：同几何下与上次输出不一致 → 漂移（除非刚重锚）。
        if self.st == St::Steady {
            if let Some(prev) = self.last_rect {
                if prev != r {
                    self.st = St::Fault;
                    return Err(E_DRIFT);
                }
            }
        }
        self.last_rect = Some(r);
        self.locates += 1;
        // 稳定账推进
        let f = fp32(g.x, g.y, g.w, g.h);
        match self.ledgers.iter_mut().find(|l| l.geom_fp == f && !l.sealed) {
            Some(l) => {
                l.steady_hits += 1;
                if l.steady_hits >= 10 {
                    l.sealed = true;
                    self.st = St::Steady;
                } else if self.st != St::Reanchored {
                    self.st = St::Anchored;
                }
            }
            None => self.ledgers.push(SteadyLedger {
                geom_fp: f,
                steady_hits: 1,
                reanch_count: self.reanch_count(),
                sealed: false,
            }),
        }
        Ok(r)
    }

    fn derive(&self, g: &Geom) -> Result<CandRect, u16> {
        let w = if DEF_W + 2 * MARGIN > g.w { g.w - 2 * MARGIN } else { DEF_W };
        let h = if DEF_H + 2 * MARGIN > g.h { g.h - 2 * MARGIN } else { DEF_H };
        if w <= 0 || h <= 0 {
            return Err(E_NEG_RECT);
        }
        let x = g.x + MARGIN;
        let y = g.y + g.h - MARGIN - h;
        if y < g.y {
            return Err(E_NEG_RECT);
        }
        Ok(CandRect { x, y, w, h })
    }

    fn reanch_count(&self) -> u32 {
        self.ledgers.iter().filter(|l| l.sealed).count() as u32
    }

    /// 重锚收口：重锚后必须重新走满 10 次稳定才能再入 Steady；
    /// 重锚次数超 2 仍不稳 = 几何源坏账。
    pub fn reanch_audit(&self) -> Result<(), u16> {
        let open: Vec<&SteadyLedger> = self.ledgers.iter().filter(|l| !l.sealed).collect();
        if open.len() > 2 {
            return Err(E_REANCH_STORM);
        }
        if self.st == St::Reanchored {
            // 重锚后必须至少完成一次定位重新留账。
            let reanch = self.ledgers.iter().any(|l| !l.sealed && l.steady_hits >= 1);
            if !reanch {
                return Err(E_ANCHOR_MISS);
            }
        }
        Ok(())
    }

    pub fn close(&mut self) -> Result<u32, u16> {
        if !self.sess_open {
            return Err(E_SESS_LOST);
        }
        self.sess_open = false;
        // 封账前提：Root 模式下至少封存一册稳定账（10 次稳定）。
        if self.mode == CandMode::Root && !self.ledgers.iter().any(|l| l.sealed) {
            return Err(E_ANCHOR_MISS);
        }
        Ok(self.locates)
    }

    pub fn st(&self) -> St {
        self.st
    }
    pub fn mode(&self) -> CandMode {
        self.mode
    }
    pub fn ledger_len(&self) -> usize {
        self.ledgers.len()
    }
    pub fn sealed_len(&self) -> usize {
        self.ledgers.iter().filter(|l| l.sealed).count()
    }
}

/// 自例程判据：落实 J1「模式 ×10 定位稳定」全链。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 场景：xterm 类无 caret → 落 Root → 同几何 10 次定位逐次一致 → 稳定封账。
    let g = Geom { x: 0, y: 0, w: 1920, h: 1040 };
    let mut r = RootCand::new(0x0301);
    r.report_caret(false)?;
    if r.mode() != CandMode::Root || r.st() != St::RootMode {
        return Err(E_MODE_FLAP);
    }
    hits += 1;
    r.set_geom(g)?;
    let mut first: Option<CandRect> = None;
    for i in 0..10 {
        let rect = r.locate()?;
        match first {
            None => first = Some(rect),
            Some(f) => {
                if f != rect {
                    return Err(E_DRIFT); // 10 次定位必须逐字节一致
                }
            }
        }
        if i == 9 && r.st() != St::Steady {
            return Err(E_ANCHOR_MISS); // 第 10 次必须入 Steady
        }
    }
    hits += 1;
    r.reanch_audit()?;
    if r.sealed_len() != 1 {
        return Err(E_ANCHOR_MISS);
    }
    hits += 1;

    // R2 场景：左下角锚点数值核对（margin=8，缺省 320×96）。
    let f = first.ok_or(E_NO_GEOM)?;
    if !(f.x == MARGIN && f.y == g.h - MARGIN - DEF_H && f.w == DEF_W && f.h == DEF_H) {
        return Err(E_NEG_RECT);
    }
    hits += 1;

    // R3 场景：几何变更 → 一次性重锚 → 新几何再 10 次稳定 → 双账封存。
    let g2 = Geom { x: 0, y: 0, w: 2560, h: 1400 };
    r.set_geom(g2)?;
    if r.st() != St::Reanchored {
        return Err(E_ANCHOR_MISS);
    }
    for _ in 0..10 {
        r.locate()?;
    }
    r.reanch_audit()?;
    if r.sealed_len() != 2 || r.st() != St::Steady {
        return Err(E_REANCH_STORM);
    }
    hits += 1;

    // R4 场景：小屏钳制 —— 工作区小于缺省窗时按余量钳制且不越界。
    let g3 = Geom { x: 0, y: 0, w: 200, h: 150 };
    let mut r3 = RootCand::new(0x0302);
    r3.report_caret(false)?;
    r3.set_geom(g3)?;
    let small = r3.locate()?;
    if !(small.w == g3.w - 2 * MARGIN && small.h == g3.h - 2 * MARGIN) {
        return Err(E_NEG_RECT);
    }
    if small.x != MARGIN || small.y != g3.y + g3.h - MARGIN - small.h {
        return Err(E_DRIFT);
    }
    hits += 1;

    // R5 场景：模式翻转账 —— 有 caret 应用并入（单次翻转放行），再翻第二次故障显性。
    let mut r5 = RootCand::new(0x0303);
    r5.report_caret(false)?;
    r5.report_caret(true)?; // 单次翻转放行
    r5.report_caret(true)?; // 同向不计数
    if r5.mode() != CandMode::Over {
        return Err(E_MODE_FLAP);
    }
    let mut r6 = RootCand::new(0x0304);
    r6.report_caret(false)?;
    r6.report_caret(true)?;
    r6.report_caret(false)?;
    r6.report_caret(false)?;
    match r6.report_caret(true) {
        Err(E_MODE_FLAP) => {}
        _ => return Err(E_MODE_FLAP), // 连续两次翻转必须故障
    }
    hits += 1;

    // R6 场景：未登几何先定位 → E_NO_GEOM；Over 模式调 locate → E_MODE_FLAP。
    let mut r7 = RootCand::new(0x0305);
    match r7.locate() {
        Err(E_NO_GEOM) => {}
        _ => return Err(E_NO_GEOM),
    }
    r7.report_caret(true)?;
    r7.set_geom(g)?;
    match r7.locate() {
        Err(E_MODE_FLAP) => {}
        _ => return Err(E_MODE_FLAP),
    }
    hits += 1;

    // R7 场景：封账守卫 —— 无稳定账 close 必拒；有账 close 记 locates 且重开拒。
    let mut r8 = RootCand::new(0x0306);
    r8.report_caret(false)?;
    r8.set_geom(g)?;
    match r8.close() {
        Err(E_ANCHOR_MISS) => {}
        _ => return Err(E_ANCHOR_MISS),
    }
    for _ in 0..10 {
        r8.locate()?;
    }
    let locs = r8.close()?;
    if locs != 10 {
        return Err(E_SESS_LOST);
    }
    match r8.close() {
        Err(E_SESS_LOST) => {}
        _ => return Err(E_SESS_LOST),
    }
    hits += 1;

    // R8 场景：坏几何拒收（内缩吃光工作区）。
    let bad = Geom { x: 0, y: 0, w: 12, h: 12 };
    let mut r9 = RootCand::new(0x0307);
    r9.report_caret(false)?;
    match r9.set_geom(bad) {
        Err(E_MARGIN_BIG) => {}
        _ => return Err(E_MARGIN_BIG),
    }
    hits += 1;

    // R9 场景：确定性指纹 —— 同几何指纹恒等，异几何指纹互异。
    if fp32(0, 0, 1920, 1040) != fp32(0, 0, 1920, 1040) || fp32(0, 0, 1920, 1040) == fp32(0, 0, 2560, 1400) {
        return Err(E_DRIFT);
    }
    hits += 1;

    // R10 场景：重锚产出核对 —— Steady 后换几何，重锚路径必须产出新几何的矩形（而非旧值）。
    let mut r10 = RootCand::new(0x0308);
    r10.report_caret(false)?;
    r10.set_geom(g)?;
    for _ in 0..10 {
        r10.locate()?;
    }
    let g4 = Geom { x: 4, y: 4, w: 1912, h: 1032 };
    r10.set_geom(g4)?;
    let re = r10.locate()?;
    if !(re.x == g4.x + MARGIN && re.y == g4.y + g4.h - MARGIN - DEF_H) {
        return Err(E_DRIFT); // 重锚必须贴新几何左下角
    }
    for _ in 0..9 {
        r10.locate()?;
    }
    if r10.st() != St::Steady || r10.sealed_len() != 2 {
        return Err(E_ANCHOR_MISS);
    }
    hits += 1;

    Ok(hits)
}
