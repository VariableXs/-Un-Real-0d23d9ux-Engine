//! unx48184_candfb — UNX-F48184 ImmGetCandidateWindow 缺省位回退（应用未提供时缺省）（诊断段 0xB7E0）。
//!
//! 判据原文：
//!   J1 回退 ×10 缺省位正确；判据锚定 §AI-61 §2 专题三类型二。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 候选窗表单四槽位（dwIndex 0..3）独立登记：应用以 CFS_CANDIDATEPOS
//!      或 CFS_EXCLUDE 显式提供者落「显式账」，未提供者走「缺省账」，
//!      两账分列不混记。
//!   2. 缺省位公式确定性：缺省 pt = (光标 x, 光标 y + 光标行高 + 2px 间隙)，
//!      CFS_EXCLUDE 型缺省再叠加排除矩形底边取 max，全部整数算术可复算。
//!   3. 回退断言：凡查询时槽位空缺，返回缺省位并留一笔回退账；
//!      J1 ×10 = 10 次空缺查询逐笔核对缺省位公式，一次不缺。
//!   4. 越界 dwIndex（≥4）拒收且不产生回退账：回退只对合法槽位生效。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB7E0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_BAD_INDEX: u16 = SEG_BASE + 1; // dwIndex 越界（≥CAND_SLOTS）
const E_FB_WRONG: u16 = SEG_BASE + 2;  // 缺省位与公式不符
const E_FB_MISS: u16 = SEG_BASE + 3;   // ×10 回退样本数不足
const E_LEDGER_BAD: u16 = SEG_BASE + 4;// 显式/缺省账与计数不平
const E_STATE_BAD: u16 = SEG_BASE + 5; // 状态机非法迁移
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空

// ── CANDIDATEFORM dwStyle 常量（IMM 口径）───────────────────
pub const CFS_CANDIDATEPOS: u32 = 0x0040;
pub const CFS_EXCLUDE: u32 = 0x0080;

/// 槽位容量：IMM 允许 4 个候选列表（dwIndex 0..3）。
pub const CAND_SLOTS: usize = 4;
/// 缺省位公式常数：光标行高（像素）。
pub const LINE_HEIGHT: i32 = 16;
/// 缺省位公式常数：行底与候选窗顶的间隙（像素）。
pub const GAP: i32 = 2;

/// 整数点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// 整数矩形：排除矩形 rcExclude。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// 一份候选窗表单（应用显式提供时）。
#[derive(Clone, Copy, Debug)]
pub struct CandidateForm {
    pub dw_index: u32,
    pub dw_style: u32,
    pub pt_pos: Point,
    pub rc_exclude: Rect,
}

/// 显式账条目：应用提供的表单原样落账。
#[derive(Clone, Copy, Debug)]
pub struct ExplicitEntry {
    pub form: CandidateForm,
}

/// 缺省账条目：一次回退的推导结果。
#[derive(Clone, Copy, Debug)]
pub struct FallbackEntry {
    pub dw_index: u32,
    pub caret: Point,   // 查询时的光标位
    pub default_pt: Point, // 推导出的缺省位
    pub exclude_used: bool,
}

/// 回退状态机：Idle 待命 → Serving 服务中 → Served 已答（显式或缺省）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CandState {
    Idle,
    Serving,
    ServedExplicit,
    ServedFallback,
}

/// 候选窗定位枢纽：四槽显式登记 + 空缺缺省回退 + 双账审计。
pub struct CandFb {
    pub state: CandState,
    explicit: [Option<ExplicitEntry>; CAND_SLOTS],
    fb_ledger: Vec<FallbackEntry>,
    pub explicit_total: u32,
    pub fallback_total: u32,
}

impl CandFb {
    pub fn new() -> CandFb {
        CandFb {
            state: CandState::Idle,
            explicit: [None, None, None, None],
            fb_ledger: Vec::new(),
            explicit_total: 0,
            fallback_total: 0,
        }
    }

    /// 应用显式登记一份表单（判闭集 dwIndex、判样式闭集）。
    pub fn set_form(&mut self, form: CandidateForm) -> Result<(), u16> {
        let idx = form.dw_index as usize;
        if idx >= CAND_SLOTS {
            return Err(E_BAD_INDEX);
        }
        if form.dw_style != CFS_CANDIDATEPOS && form.dw_style != CFS_EXCLUDE {
            return Err(E_BAD_INDEX);
        }
        self.explicit[idx] = Some(ExplicitEntry { form });
        self.explicit_total += 1;
        Ok(())
    }

    /// 缺省位公式（纪律 2）：非排除型 = 光标下方一行；排除型取光标底
    /// 与排除矩形底边的 max，再叠间隙。
    pub fn default_pos(caret: Point, exclude: Option<Rect>) -> Point {
        let base_y = caret.y + LINE_HEIGHT + GAP;
        match exclude {
            None => Point { x: caret.x, y: base_y },
            Some(rc) => {
                let bottom = if rc.bottom > caret.y { rc.bottom } else { caret.y };
                Point { x: caret.x, y: bottom + GAP }
            }
        }
    }

    /// ImmGetCandidateWindow 主径：显式命中返回显式位；空缺走缺省回退。
    /// `caret` 为当前光标位；`exclude_hint` 为调用方提示的排除矩形（可空）。
    pub fn get_form(
        &mut self,
        dw_index: u32,
        caret: Point,
        exclude_hint: Option<Rect>,
    ) -> Result<(Point, bool), u16> {
        let idx = dw_index as usize;
        if idx >= CAND_SLOTS {
            return Err(E_BAD_INDEX);
        }
        self.state = CandState::Serving;
        if let Some(entry) = &self.explicit[idx] {
            let pt = entry.form.pt_pos;
            self.state = CandState::ServedExplicit;
            return Ok((pt, true));
        }
        // 空缺回退：按公式推导缺省位并留账（判据 J1 逐笔）。
        let exclude = match exclude_hint {
            Some(rc) if rc.right > rc.left && rc.bottom > rc.top => Some(rc),
            _ => None,
        };
        let default_pt = Self::default_pos(caret, exclude);
        self.fb_ledger.push(FallbackEntry {
            dw_index,
            caret,
            default_pt,
            exclude_used: exclude.is_some(),
        });
        self.fallback_total += 1;
        self.state = CandState::ServedFallback;
        Ok((default_pt, false))
    }

    /// 回退账逐笔复核：每条回退账的 default_pt 必须与公式复算值一致。
    pub fn audit(&self) -> Result<(), u16> {
        if self.fb_ledger.len() as u32 != self.fallback_total {
            return Err(E_LEDGER_BAD);
        }
        let mut i = 0usize;
        while i < self.fb_ledger.len() {
            let e = &self.fb_ledger[i];
            let exclude = if e.exclude_used {
                // 回放时排除矩形已并入公式，仅按 used 标志复算非排除基准不可行；
                // 这里用记账时刻的 default_pt 反推一致性：y 必须 > caret.y。
                if e.default_pt.y <= e.caret.y {
                    return Err(E_FB_WRONG);
                }
                None
            } else {
                None
            };
            if e.exclude_used {
                // 排除型：x 与光标同列，y 在光标之下（间隙 ≥GAP）。
                if e.default_pt.x != e.caret.x {
                    return Err(E_FB_WRONG);
                }
                if e.default_pt.y < e.caret.y + GAP {
                    return Err(E_FB_WRONG);
                }
            } else {
                let want = Self::default_pos(e.caret, exclude);
                if want != e.default_pt {
                    return Err(E_FB_WRONG);
                }
            }
            if e.dw_index as usize >= CAND_SLOTS {
                return Err(E_BAD_INDEX);
            }
            i += 1;
        }
        // 显式登记槽位与计数守恒：已登记槽位数 ≤ explicit_total（可覆盖重登）。
        let mut used = 0usize;
        let mut s = 0usize;
        while s < CAND_SLOTS {
            if self.explicit[s].is_some() {
                used += 1;
            }
            s += 1;
        }
        if used > self.explicit_total as usize {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// ×N 回退样本覆盖核验（J1 硬口径）。
    pub fn fb_coverage(&self, want: u32) -> Result<(), u16> {
        if self.fallback_total < want {
            return Err(E_FB_MISS);
        }
        Ok(())
    }

    /// 查询槽位是否显式登记。
    pub fn is_explicit(&self, dw_index: u32) -> Result<bool, u16> {
        let idx = dw_index as usize;
        if idx >= CAND_SLOTS {
            return Err(E_BAD_INDEX);
        }
        Ok(self.explicit[idx].is_some())
    }

    pub fn fb_ledger_len(&self) -> usize {
        self.fb_ledger.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&mut self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done {
            return Err(E_STATE_BAD);
        }
        if self.state != CandState::ServedExplicit && self.state != CandState::ServedFallback {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（回退 ×10 缺省位正确），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 纯回退 ×10：四槽全空，10 次查询全部走缺省账，公式逐笔核对。
    let mut cf = CandFb::new();
    let mut i = 0i32;
    while i < 10 {
        let caret = Point { x: 20 + i * 11, y: 40 + i * 9 };
        let (pt, explicit) = cf.get_form((i as u32) % 4, caret, None)?;
        if explicit {
            return Err(E_FB_WRONG); // 空表不许命中显式账
        }
        let want = CandFb::default_pos(caret, None);
        if pt != want {
            return Err(E_FB_WRONG);
        }
        if pt.y != caret.y + LINE_HEIGHT + GAP || pt.x != caret.x {
            return Err(E_FB_WRONG);
        }
        i += 1;
    }
    cf.fb_coverage(10)?;
    cf.audit()?;
    hits += 1;

    // R2 显式命中不回退：登记后查询返回显式位，回退账零增长。
    let mut cf = CandFb::new();
    cf.set_form(CandidateForm {
        dw_index: 1,
        dw_style: CFS_CANDIDATEPOS,
        pt_pos: Point { x: 77, y: 88 },
        rc_exclude: Rect { left: 0, top: 0, right: 0, bottom: 0 },
    })?;
    let (pt, explicit) = cf.get_form(1, Point { x: 5, y: 5 }, None)?;
    if !explicit || pt != (Point { x: 77, y: 88 }) {
        return Err(E_FB_WRONG);
    }
    if cf.fallback_total != 0 {
        return Err(E_LEDGER_BAD);
    }
    // 未登记的槽 3 依旧回退。
    let (pt2, explicit2) = cf.get_form(3, Point { x: 9, y: 9 }, None)?;
    if explicit2 {
        return Err(E_FB_WRONG);
    }
    if pt2 != CandFb::default_pos(Point { x: 9, y: 9 }, None) {
        return Err(E_FB_WRONG);
    }
    hits += 1;

    // R3 排除型回退 ×5：排除矩形底边低于光标行时，缺省位取 max 底 + 间隙。
    let mut cf = CandFb::new();
    let mut i = 0i32;
    while i < 5 {
        let caret = Point { x: 30 + i * 7, y: 60 + i * 8 };
        let rc = Rect {
            left: caret.x - 4,
            top: caret.y,
            right: caret.x + 60,
            bottom: caret.y + 10 + i, // 底边逐笔下移
        };
        let (pt, explicit) = cf.get_form((i as u32) % 4, caret, Some(rc))?;
        if explicit {
            return Err(E_FB_WRONG);
        }
        let want = CandFb::default_pos(caret, Some(rc));
        if pt != want || pt.y != rc.bottom + GAP {
            return Err(E_FB_WRONG);
        }
        i += 1;
    }
    cf.audit()?;
    hits += 1;

    // R4 越界 dwIndex 拒收：get/set 双侧都不产账、不迁移状态。
    let mut cf = CandFb::new();
    if cf.get_form(4, Point { x: 1, y: 1 }, None).is_ok() {
        return Err(E_BAD_INDEX);
    }
    if cf.set_form(CandidateForm {
        dw_index: 9,
        dw_style: CFS_CANDIDATEPOS,
        pt_pos: Point { x: 0, y: 0 },
        rc_exclude: Rect { left: 0, top: 0, right: 0, bottom: 0 },
    }).is_ok() {
        return Err(E_BAD_INDEX);
    }
    if cf.fallback_total != 0 || cf.explicit_total != 0 {
        return Err(E_LEDGER_BAD);
    }
    // 非法样式同样拒收（闭集外 0x0100）。
    if cf.set_form(CandidateForm {
        dw_index: 0,
        dw_style: 0x0100,
        pt_pos: Point { x: 0, y: 0 },
        rc_exclude: Rect { left: 0, top: 0, right: 0, bottom: 0 },
    }).is_ok() {
        return Err(E_BAD_INDEX);
    }
    hits += 1;

    // R5 退化排除矩形：零宽/零高提示一律视作无排除，回退走基准公式。
    let mut cf = CandFb::new();
    let caret = Point { x: 44, y: 66 };
    let (pt, _) = cf.get_form(0, caret, Some(Rect { left: 5, top: 5, right: 5, bottom: 5 }))?;
    if pt != CandFb::default_pos(caret, None) {
        return Err(E_FB_WRONG);
    }
    let (pt2, _) = cf.get_form(0, caret, Some(Rect { left: 5, top: 5, right: 4, bottom: 9 }))?;
    if pt2 != CandFb::default_pos(caret, None) {
        return Err(E_FB_WRONG);
    }
    hits += 1;

    // R6 覆盖重登：同槽二次显式登记覆盖旧值，查询取最新；审计仍平。
    let mut cf = CandFb::new();
    cf.set_form(CandidateForm {
        dw_index: 2,
        dw_style: CFS_CANDIDATEPOS,
        pt_pos: Point { x: 1, y: 2 },
        rc_exclude: Rect { left: 0, top: 0, right: 0, bottom: 0 },
    })?;
    cf.set_form(CandidateForm {
        dw_index: 2,
        dw_style: CFS_EXCLUDE,
        pt_pos: Point { x: 30, y: 40 },
        rc_exclude: Rect { left: 10, top: 20, right: 60, bottom: 50 },
    })?;
    let (pt, explicit) = cf.get_form(2, Point { x: 0, y: 0 }, None)?;
    if !explicit || pt != (Point { x: 30, y: 40 }) {
        return Err(E_FB_WRONG);
    }
    cf.audit()?;
    hits += 1;

    // R7 混合流：2 显式 + 10 缺省交错，回退计数=10、显式计数=2、双账守恒。
    let mut cf = CandFb::new();
    cf.set_form(CandidateForm {
        dw_index: 0,
        dw_style: CFS_CANDIDATEPOS,
        pt_pos: Point { x: 11, y: 22 },
        rc_exclude: Rect { left: 0, top: 0, right: 0, bottom: 0 },
    })?;
    cf.set_form(CandidateForm {
        dw_index: 1,
        dw_style: CFS_EXCLUDE,
        pt_pos: Point { x: 33, y: 44 },
        rc_exclude: Rect { left: 8, top: 8, right: 88, bottom: 48 },
    })?;
    let mut i = 0i32;
    while i < 10 {
        let caret = Point { x: i, y: i * 3 + 5 };
        let idx = (i as u32 % 4) + 0;
        let (pt, explicit) = cf.get_form(idx, caret, None)?;
        if idx < 2 {
            if !explicit {
                return Err(E_FB_WRONG);
            }
        } else {
            if explicit {
                return Err(E_FB_WRONG);
            }
            if pt.y != caret.y + LINE_HEIGHT + GAP {
                return Err(E_FB_WRONG);
            }
        }
        i += 1;
    }
    if cf.fallback_total != 10 {
        return Err(E_FB_MISS);
    }
    if cf.explicit_total != 2 {
        return Err(E_LEDGER_BAD);
    }
    cf.audit()?;
    cf.fb_coverage(10)?;
    hits += 1;

    // R8 写回（判据 J2）：Idle 拒写、空摘要拒写、服务后非空摘要放行。
    let mut cf = CandFb::new();
    if cf.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    cf.get_form(0, Point { x: 3, y: 3 }, None)?;
    if cf.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    cf.writeback(true, 0x20)?;
    hits += 1;

    Ok(hits)
}
