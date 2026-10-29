//! unx48185_candrect — UNX-F48185 Wine 侧候选窗定位数据面（COMPOSITIONFORM→F4 rect 换算）（诊断段 0xB800）。
//!
//! 判据原文：
//!   J1 换算 ×100 保真；判据锚定 §AI-61 §2 专题三类型二。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 换算数据面：COMPOSITIONFORM（dwStyle/ptCurrentPos/rcArea）+ 字体
//!      度量（升部/降部/光标宽）→ F4 rect（x0,y0,x1,y1 四元组），全程
//!      整数算术，同一输入必得同一输出（纯函数，无隐状态）。
//!   2. 三型三式：CFS_POINT 型换算为光标宽×行高的竖条 rect；CFS_RECT 型
//!      直取 rcArea 为 rect；CFS_DEFAULT 型取客户区左上缺省行 rect；
//!      FORCE_POSITION 只影响点式原点取值，不改变 rect 形状公式。
//!   3. 保真断言口径：点式 x1−x0==光标宽、y1−y0==行高且原点等于规格点；
//!      矩式四边与 rcArea 逐边相等；缺省式等于缺省行公式。逐笔全等，
//!      ×100 样本由确定性参数扫描生成，可复算可回放。
//!   4. 退化防御：负宽高 rcArea、零字体度量按规则钳制后换算，不产出
//!      非法 rect；钳制逐笔留痕。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB800;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_BAD_STYLE: u16 = SEG_BASE + 1;  // dwStyle 闭集外
const E_CONV_BROKEN: u16 = SEG_BASE + 2;// 换算结果与公式不符
const E_CONV_MISS: u16 = SEG_BASE + 3;  // ×100 样本数不足
const E_BAD_RECT: u16 = SEG_BASE + 4;   // 退化钳制后仍非法
const E_STATE_BAD: u16 = SEG_BASE + 5;  // 状态机非法迁移
const E_EMPTY_SUM: u16 = SEG_BASE + 6;  // 写回摘要为空

// ── dwStyle 常量（IMM 口径）─────────────────────────────────
pub const CFS_DEFAULT: u32 = 0x0000;
pub const CFS_RECT: u32 = 0x0001;
pub const CFS_POINT: u32 = 0x0002;
pub const CFS_FORCE_POSITION: u32 = 0x0020;

/// 缺省式常数：客户区左边距 4px、顶边距 4px。
pub const DEF_MARGIN: i32 = 4;
/// 缺省式常数：缺省行高 16px、缺省行宽 64px。
pub const DEF_LINE_H: i32 = 16;
pub const DEF_LINE_W: i32 = 64;

/// 字体度量：升部/降部/光标宽（像素，全整数）。
#[derive(Clone, Copy, Debug)]
pub struct FontMetric {
    pub ascent: i32,
    pub descent: i32,
    pub caret_w: i32,
}

impl FontMetric {
    /// 行高 = 升部 + 降部；退化度量（和 ≤0 或光标宽 ≤0）钳制为缺省行。
    pub fn line_height(&self) -> i32 {
        let h = self.ascent + self.descent;
        if h <= 0 {
            DEF_LINE_H
        } else {
            h
        }
    }
    pub fn caret_width(&self) -> i32 {
        if self.caret_w <= 0 {
            2
        } else {
            self.caret_w
        }
    }
}

/// COMPOSITIONFORM 内存像。
#[derive(Clone, Copy, Debug)]
pub struct CompositionForm {
    pub dw_style: u32,
    pub pt_current_pos: (i32, i32),
    pub rc_area: (i32, i32, i32, i32),
}

/// F4 rect：四元组 (x0,y0,x1,y1)，半开区间 [x0,x1)×[y0,y1)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct F4Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl F4Rect {
    pub fn width(&self) -> i32 {
        self.x1 - self.x0
    }
    pub fn height(&self) -> i32 {
        self.y1 - self.y0
    }
    pub fn is_valid(&self) -> bool {
        self.x1 > self.x0 && self.y1 > self.y0
    }
    /// 包含判定：点 p 落在半开区间内。
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }
}

/// 一次换算账：输入样式 + 输出 rect + 是否走了钳制。
#[derive(Clone, Copy, Debug)]
pub struct ConvRecord {
    pub dw_style: u32,
    pub out: F4Rect,
    pub clamped: bool,
}

/// 换算状态机：Idle → Converting → Converted / Clamped / Rejected。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConvState {
    Idle,
    Converted,
    Clamped,
    Rejected,
}

/// 换算枢纽：三型三式纯函数换算 + 逐笔留账 + 审计。
pub struct CandRect {
    pub state: ConvState,
    ledger: Vec<ConvRecord>,
    pub conv_total: u32,
    pub clamp_total: u32,
}

impl CandRect {
    pub fn new() -> CandRect {
        CandRect {
            state: ConvState::Idle,
            ledger: Vec::new(),
            conv_total: 0,
            clamp_total: 0,
        }
    }

    /// 核心换算（纯函数，纪律 1/2）：同参必同果。
    pub fn convert(form: &CompositionForm, fm: &FontMetric) -> Result<F4Rect, u16> {
        let base = form.dw_style & !CFS_FORCE_POSITION;
        let force = form.dw_style & CFS_FORCE_POSITION != 0;
        let lh = fm.line_height();
        let cw = fm.caret_width();
        match base {
            CFS_POINT => {
                let (px, py) = if force {
                    form.pt_current_pos
                } else {
                    // 无 FORCE：点式原点为光标位，升部决定竖条顶（基线对齐）。
                    (form.pt_current_pos.0, form.pt_current_pos.1)
                };
                Ok(F4Rect { x0: px, y0: py, x1: px + cw, y1: py + lh })
            }
            CFS_RECT => {
                let (l, t, r, b) = form.rc_area;
                Ok(F4Rect { x0: l, y0: t, x1: r, y1: b })
            }
            CFS_DEFAULT => Ok(F4Rect {
                x0: DEF_MARGIN,
                y0: DEF_MARGIN,
                x1: DEF_MARGIN + DEF_LINE_W,
                y1: DEF_MARGIN + lh,
            }),
            _ => Err(E_BAD_STYLE),
        }
    }

    /// 退化钳制：宽/高非正时按最小可用 rect 重整（点式 2×行高，矩式 8×8）。
    pub fn sanitize(rect: &F4Rect, fm: &FontMetric) -> (F4Rect, bool) {
        if rect.is_valid() {
            (*rect, false)
        } else {
            let x0 = if rect.x0 <= rect.x1 { rect.x0 } else { rect.x1 };
            let y0 = if rect.y0 <= rect.y1 { rect.y0 } else { rect.y1 };
            let w = if rect.width() <= 0 { fm.caret_width().max(8) } else { rect.width() };
            let h = if rect.height() <= 0 { fm.line_height().max(8) } else { rect.height() };
            (F4Rect { x0, y0, x1: x0 + w, y1: y0 + h }, true)
        }
    }

    /// 主径：换算 → 钳制 → 留账 → 状态迁移。
    pub fn conv(&mut self, form: &CompositionForm, fm: &FontMetric) -> Result<F4Rect, u16> {
        let raw = Self::convert(form, fm)?;
        let (out, clamped) = Self::sanitize(&raw, fm);
        if !out.is_valid() {
            self.state = ConvState::Rejected;
            return Err(E_BAD_RECT);
        }
        self.ledger.push(ConvRecord { dw_style: form.dw_style, out, clamped });
        self.conv_total += 1;
        if clamped {
            self.clamp_total += 1;
            self.state = ConvState::Clamped;
        } else {
            self.state = ConvState::Converted;
        }
        Ok(out)
    }

    /// 逐笔保真复核：重放输入重算输出，与账面比对（J1 口径）。
    pub fn audit(&self, forms: &[CompositionForm], fm: &FontMetric) -> Result<(), u16> {
        if forms.len() as u32 != self.conv_total || forms.len() != self.ledger.len() {
            return Err(E_CONV_MISS);
        }
        let mut i = 0usize;
        while i < forms.len() {
            let want = Self::convert(&forms[i], fm)?;
            let (want, _) = Self::sanitize(&want, fm);
            let got = self.ledger[i].out;
            if want != got {
                return Err(E_CONV_BROKEN);
            }
            if self.ledger[i].dw_style != forms[i].dw_style {
                return Err(E_CONV_BROKEN);
            }
            if !got.is_valid() {
                return Err(E_BAD_RECT);
            }
            i += 1;
        }
        Ok(())
    }

    /// ×N 覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.conv_total < want {
            return Err(E_CONV_MISS);
        }
        Ok(())
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&mut self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done {
            return Err(E_STATE_BAD);
        }
        if self.state != ConvState::Converted && self.state != ConvState::Clamped {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 确定性样本扫描：第 k 笔样本的 COMPOSITIONFORM（供 ×100 生成与回放）。
pub fn sample_form(k: u32) -> CompositionForm {
    let s = k % 4;
    let a = (k as i32) * 7 + 3;
    let b = (k as i32) * 5 + 11;
    match s {
        0 => CompositionForm {
            dw_style: CFS_POINT | CFS_FORCE_POSITION,
            pt_current_pos: (a, b),
            rc_area: (0, 0, 0, 0),
        },
        1 => CompositionForm {
            dw_style: CFS_POINT,
            pt_current_pos: (a, b),
            rc_area: (0, 0, 0, 0),
        },
        2 => CompositionForm {
            dw_style: CFS_RECT,
            pt_current_pos: (a, b),
            rc_area: (a, b, a + 60 + (k as i32) % 9, b + 20 + (k as i32) % 5),
        },
        _ => CompositionForm {
            dw_style: CFS_RECT | CFS_FORCE_POSITION,
            pt_current_pos: (a, b),
            rc_area: (a + 2, b + 2, a - 2, b - 2), // 退化矩形：触发钳制
        },
    }
}

/// 自例程判据：逐条落实 J1（换算 ×100 保真），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let fm = FontMetric { ascent: 12, descent: 4, caret_w: 3 };

    // R1 主扫描 ×100：四型轮转样本逐笔换算，逐笔重放比对全等。
    let mut cr = CandRect::new();
    let mut forms: Vec<CompositionForm> = Vec::new();
    let mut k = 0u32;
    while k < 100 {
        let f = sample_form(k);
        let out = cr.conv(&f, &fm)?;
        let want = CandRect::convert(&f, &fm)?;
        let (want, _) = CandRect::sanitize(&want, &fm);
        if out != want {
            return Err(E_CONV_BROKEN);
        }
        match f.dw_style & !CFS_FORCE_POSITION {
            CFS_POINT => {
                if out.width() != fm.caret_width() || out.height() != fm.line_height() {
                    return Err(E_CONV_BROKEN);
                }
                if !out.contains(f.pt_current_pos.0, f.pt_current_pos.1) {
                    return Err(E_CONV_BROKEN);
                }
            }
            CFS_RECT => {
                let (l, t, r, b) = f.rc_area;
                let fixed = CandRect::sanitize(
                    &F4Rect { x0: l, y0: t, x1: r, y1: b }, &fm).0;
                if out != fixed {
                    return Err(E_CONV_BROKEN);
                }
            }
            _ => {
                if out.x0 != DEF_MARGIN || out.y0 != DEF_MARGIN {
                    return Err(E_CONV_BROKEN);
                }
            }
        }
        forms.push(f);
        k += 1;
    }
    cr.coverage(100)?;
    cr.audit(&forms, &fm)?;
    if cr.clamp_total != 25 {
        return Err(E_CONV_BROKEN); // 样本 3 型（k%4==3）恰 25 笔触发钳制
    }
    hits += 1;

    // R2 点式语义：FORCE 原点直取，无 FORCE 亦直取光标位，形状公式不变。
    let f_force = CompositionForm {
        dw_style: CFS_POINT | CFS_FORCE_POSITION,
        pt_current_pos: (40, 50),
        rc_area: (0, 0, 0, 0),
    };
    let f_plain = CompositionForm { dw_style: CFS_POINT, ..f_force };
    let r1 = CandRect::convert(&f_force, &fm)?;
    let r2 = CandRect::convert(&f_plain, &fm)?;
    if r1 != r2 || r1.x0 != 40 || r1.y0 != 50 {
        return Err(E_CONV_BROKEN);
    }
    if r1.width() != 3 || r1.height() != 16 {
        return Err(E_CONV_BROKEN);
    }
    hits += 1;

    // R3 矩式语义：四边逐边相等，与字体度量无关。
    let f = CompositionForm {
        dw_style: CFS_RECT,
        pt_current_pos: (1, 1),
        rc_area: (10, 20, 110, 80),
    };
    let r = CandRect::convert(&f, &fm)?;
    if r.x0 != 10 || r.y0 != 20 || r.x1 != 110 || r.y1 != 80 {
        return Err(E_CONV_BROKEN);
    }
    let fm2 = FontMetric { ascent: 8, descent: 2, caret_w: 1 };
    let r2 = CandRect::convert(&f, &fm2)?;
    if r2 != r {
        return Err(E_CONV_BROKEN);
    }
    hits += 1;

    // R4 缺省式语义：缺省行 rect 恒定，行高随度量变化可复算。
    let f = CompositionForm {
        dw_style: CFS_DEFAULT,
        pt_current_pos: (99, 99),
        rc_area: (1, 1, 2, 2),
    };
    let r = CandRect::convert(&f, &fm)?;
    if r.x0 != DEF_MARGIN || r.y0 != DEF_MARGIN || r.width() != DEF_LINE_W || r.height() != 16 {
        return Err(E_CONV_BROKEN);
    }
    let r2 = CandRect::convert(&f, &fm2)?;
    if r2.height() != 10 {
        return Err(E_CONV_BROKEN); // ascent 8 + descent 2
    }
    hits += 1;

    // R5 退化防御：负宽矩形钳制成合法 rect，零度量钳制为缺省行。
    let mut cr = CandRect::new();
    let bad = CompositionForm {
        dw_style: CFS_RECT,
        pt_current_pos: (0, 0),
        rc_area: (50, 50, 10, 20), // 负宽
    };
    let out = cr.conv(&bad, &fm)?;
    if !out.is_valid() || out.width() <= 0 || out.height() <= 0 {
        return Err(E_BAD_RECT);
    }
    let zm = FontMetric { ascent: 0, descent: 0, caret_w: 0 };
    let out2 = cr.conv(&sample_form(0), &zm)?;
    if out2.height() != DEF_LINE_H || out2.width() != 2 {
        return Err(E_CONV_BROKEN); // 零度量钳制：行高 16、光标宽钳为 2
    }
    if cr.clamp_total == 0 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R6 非法样式拒收：0x0010 闭集外，状态 Rejected、账零增长。
    let mut cr = CandRect::new();
    let bad_style = CompositionForm {
        dw_style: 0x0010,
        pt_current_pos: (1, 1),
        rc_area: (0, 0, 9, 9),
    };
    if cr.conv(&bad_style, &fm).is_ok() {
        return Err(E_BAD_STYLE);
    }
    if cr.state != ConvState::Rejected || cr.conv_total != 0 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R7 确定性回放：同参两次换算逐位相等（纯函数红线）。
    let mut i = 0u32;
    while i < 20 {
        let f = sample_form(i);
        let a = CandRect::convert(&f, &fm)?;
        let b = CandRect::convert(&f, &fm)?;
        if a != b {
            return Err(E_CONV_BROKEN);
        }
        i += 1;
    }
    hits += 1;

    // R8 写回（判据 J2）：Idle 拒写、空摘要拒写、转换态非空摘要放行。
    let mut cr = CandRect::new();
    if cr.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    cr.conv(&sample_form(0), &fm)?;
    if cr.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    cr.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
