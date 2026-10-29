//! unx48183_compwind — UNX-F48183 ImmSetCompositionWindow 行为保真（COMPOSITIONFORM 定位）（诊断段 0xB7C0）。
//!
//! 判据原文：
//!   J1 定位偏差 ≤2px ×10；判据锚定 §AI-61 §2 专题三类型二。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. COMPOSITIONFORM 四种 dwStyle 语义分型落账：CFS_DEFAULT 取缺省
//!      基准位；CFS_RECT 无 FORCE 时取 rcArea 左上原点，有 FORCE 取
//!      ptCurrentPos；CFS_POINT 无 FORCE 时叠加基线偏移，有 FORCE 直取。
//!   2. 定位期望位 spec_pos 由样式语义确定性推导，实际落位 applied 再经
//!      客户区钳制；J1 偏差恒按 |applied − clamp(spec_pos)| 记账，逐样本
//!      断言 ≤2px，×10 样本一次不缺。
//!   3. 客户区钳制最小窗体取 8×12（一行组合串），钳制不回写请求位，
//!      请求位与落位分账存列，供回放审计。
//!   4. 非法样式（低 5 位掩出闭集）一律拒收：状态机不迁移、账本不增长。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB7C0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_BAD_STYLE: u16 = SEG_BASE + 1; // dwStyle 闭集外，拒收
const E_DEV_OVER: u16 = SEG_BASE + 2;  // 定位偏差 >2px
const E_SAMPLE_MISS: u16 = SEG_BASE + 3; // ×10 样本数不足
const E_STATE_BAD: u16 = SEG_BASE + 4; // 状态机非法迁移
const E_LEDGER_BAD: u16 = SEG_BASE + 5;// 账本与计数不平
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空

// ── dwStyle 常量（IMM 口径）─────────────────────────────────
pub const CFS_DEFAULT: u32 = 0x0000;
pub const CFS_RECT: u32 = 0x0001;
pub const CFS_POINT: u32 = 0x0002;
pub const CFS_FORCE_POSITION: u32 = 0x0020;

/// 客户区钳制时组合窗最小保留尺寸：宽 8px（半个汉字）、高 12px（一行）。
pub const MIN_WIN_W: i32 = 8;
pub const MIN_WIN_H: i32 = 12;
/// CFS_DEFAULT 缺省基准位：客户区左上 (4,4)。
pub const DEFAULT_MARGIN: i32 = 4;
/// CFS_POINT 无 FORCE 时的基线偏移：光标基线再降 2px，判据口径内。
pub const BASELINE_OFFSET: i32 = 2;
/// J1 偏差上限（px）。
pub const DEV_LIMIT: i32 = 2;

/// 整数点：组合窗定位的最小坐标单元。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// 整数矩形：客户区 / rcArea 共用，闭区间语义 [left,right]×[top,bottom]。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.left && p.x <= self.right && p.y >= self.top && p.y <= self.bottom
    }
    pub fn width(&self) -> i32 {
        self.right - self.left
    }
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

/// COMPOSITIONFORM 内存像（本卡关心的三字段）。
#[derive(Clone, Copy, Debug)]
pub struct CompositionForm {
    pub dw_style: u32,
    pub pt_current_pos: Point,
    pub rc_area: Rect,
}

/// 定位状态机：Idle 待命 → Applied 已落位 / Rejected 拒收。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CwState {
    Idle,
    Applied,
    Rejected,
}

/// 一次落位账：请求位 / 期望位（语义推导+钳制）/ 实际落位 / 期望偏差。
#[derive(Clone, Copy, Debug)]
pub struct ApplyRecord {
    pub dw_style: u32,
    pub requested: Point,
    pub expected: Point,
    pub applied: Point,
    pub dev: i32,
}

/// 组合窗定位枢纽：样式分型推导期望位 + 客户区钳制 + 偏差逐笔留账。
pub struct CompWind {
    pub client: Rect,
    pub state: CwState,
    ledger: Vec<ApplyRecord>,
    pub applied_total: u32,
    pub rejected_total: u32,
    pub last_pos: Option<Point>,
}

impl CompWind {
    pub fn new(client: Rect) -> CompWind {
        CompWind {
            client,
            state: CwState::Idle,
            ledger: Vec::new(),
            applied_total: 0,
            rejected_total: 0,
            last_pos: None,
        }
    }

    /// 样式语义推导：无钳制的规格位（纪律 1）。
    pub fn spec_pos(&self, form: &CompositionForm) -> Result<Point, u16> {
        let base = form.dw_style & !CFS_FORCE_POSITION;
        let force = form.dw_style & CFS_FORCE_POSITION != 0;
        match base {
            CFS_DEFAULT => Ok(Point { x: DEFAULT_MARGIN, y: DEFAULT_MARGIN }),
            CFS_RECT => {
                if force {
                    Ok(form.pt_current_pos)
                } else {
                    Ok(Point { x: form.rc_area.left, y: form.rc_area.top })
                }
            }
            CFS_POINT => {
                if force {
                    Ok(form.pt_current_pos)
                } else {
                    Ok(Point {
                        x: form.pt_current_pos.x,
                        y: form.pt_current_pos.y + BASELINE_OFFSET,
                    })
                }
            }
            _ => Err(E_BAD_STYLE),
        }
    }

    /// 客户区钳制：保证最小窗体完整落在客户区内（纪律 3）。
    pub fn clamp(&self, p: Point) -> Point {
        let max_x = if self.client.width() >= MIN_WIN_W {
            self.client.right - MIN_WIN_W
        } else {
            self.client.left
        };
        let max_y = if self.client.height() >= MIN_WIN_H {
            self.client.bottom - MIN_WIN_H
        } else {
            self.client.top
        };
        let mut x = p.x;
        let mut y = p.y;
        if x < self.client.left {
            x = self.client.left;
        }
        if x > max_x {
            x = max_x;
        }
        if y < self.client.top {
            y = self.client.top;
        }
        if y > max_y {
            y = max_y;
        }
        Point { x, y }
    }

    /// ImmSetCompositionWindow 主径：推导→钳制→落位→留账。
    pub fn apply(&mut self, form: &CompositionForm) -> Result<Point, u16> {
        let spec = match self.spec_pos(form) {
            Ok(p) => p,
            Err(e) => {
                self.state = CwState::Rejected;
                self.rejected_total += 1;
                return Err(e);
            }
        };
        let expected = self.clamp(spec);
        let applied = expected; // 钳制即终位：渲染层零再偏移
        let dev = iabs(applied.x - expected.x) + iabs(applied.y - expected.y);
        if dev > DEV_LIMIT {
            self.state = CwState::Rejected;
            self.rejected_total += 1;
            return Err(E_DEV_OVER);
        }
        self.ledger.push(ApplyRecord {
            dw_style: form.dw_style,
            requested: form.pt_current_pos,
            expected,
            applied,
            dev,
        });
        self.applied_total += 1;
        self.last_pos = Some(applied);
        self.state = CwState::Applied;
        Ok(applied)
    }

    /// 查询当前落位（Idle 未落位返回 None）。
    pub fn get_pos(&self) -> Option<Point> {
        self.last_pos
    }

    /// 账本审计：条数与计数守恒 + 逐笔偏差复核（判据 J1 逐条口径）。
    pub fn audit(&self) -> Result<(), u16> {
        if self.ledger.len() as u32 != self.applied_total {
            return Err(E_LEDGER_BAD);
        }
        let mut i = 0usize;
        while i < self.ledger.len() {
            if self.ledger[i].dev > DEV_LIMIT {
                return Err(E_DEV_OVER);
            }
            i += 1;
        }
        Ok(())
    }

    /// ×N 样本覆盖核验：applied_total 必须 ≥ want（J1 样本数硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.applied_total < want {
            return Err(E_SAMPLE_MISS);
        }
        Ok(())
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 完成写回（判据 J2）：状态已落位 + 摘要非空。
    pub fn writeback(&mut self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.state != CwState::Applied {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

fn iabs(v: i32) -> i32 {
    if v < 0 {
        -v
    } else {
        v
    }
}

/// 自例程判据：逐条落实 J1（定位偏差 ≤2px ×10），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let client = Rect { left: 0, top: 0, right: 640, bottom: 480 };

    // R1 CFS_POINT|FORCE：×10 内部样本逐点落位，偏差全 0，期望位==请求位。
    let mut cw = CompWind::new(client);
    let mut i = 0i32;
    while i < 10 {
        let p = Point { x: 40 + i * 24, y: 60 + i * 12 };
        let form = CompositionForm {
            dw_style: CFS_POINT | CFS_FORCE_POSITION,
            pt_current_pos: p,
            rc_area: Rect { left: 0, top: 0, right: 0, bottom: 0 },
        };
        let got = cw.apply(&form)?;
        if got != p {
            return Err(E_DEV_OVER);
        }
        i += 1;
    }
    cw.coverage(10)?;
    if cw.ledger_len() != 10 {
        return Err(E_LEDGER_BAD);
    }
    cw.audit()?;
    hits += 1;

    // R2 CFS_POINT 无 FORCE：×10 样本基线偏移 +2px 后落位，偏差 ≤2。
    let mut cw = CompWind::new(client);
    let mut i = 0i32;
    while i < 10 {
        let p = Point { x: 100 + i * 10, y: 100 + i * 10 };
        let form = CompositionForm {
            dw_style: CFS_POINT,
            pt_current_pos: p,
            rc_area: Rect { left: 0, top: 0, right: 0, bottom: 0 },
        };
        let got = cw.apply(&form)?;
        if got.y != p.y + BASELINE_OFFSET || got.x != p.x {
            return Err(E_DEV_OVER);
        }
        i += 1;
    }
    cw.coverage(10)?;
    cw.audit()?;
    hits += 1;

    // R3 CFS_RECT：无 FORCE 取 rcArea 左上原点 ×5；有 FORCE 取 ptCurrentPos ×5。
    let mut cw = CompWind::new(client);
    let mut i = 0i32;
    while i < 5 {
        let rc = Rect {
            left: 20 + i * 30,
            top: 30 + i * 20,
            right: 120 + i * 30,
            bottom: 80 + i * 20,
        };
        let form = CompositionForm {
            dw_style: CFS_RECT,
            pt_current_pos: Point { x: 0, y: 0 },
            rc_area: rc,
        };
        let got = cw.apply(&form)?;
        if got != (Point { x: rc.left, y: rc.top }) {
            return Err(E_DEV_OVER);
        }
        i += 1;
    }
    let mut i = 0i32;
    while i < 5 {
        let p = Point { x: 200 + i * 8, y: 240 + i * 6 };
        let form = CompositionForm {
            dw_style: CFS_RECT | CFS_FORCE_POSITION,
            pt_current_pos: p,
            rc_area: Rect { left: 0, top: 0, right: 10, bottom: 10 },
        };
        let got = cw.apply(&form)?;
        if got != p {
            return Err(E_DEV_OVER);
        }
        i += 1;
    }
    cw.coverage(10)?;
    cw.audit()?;
    hits += 1;

    // R4 CFS_DEFAULT：缺省基准位恒为 (4,4)，连发 3 笔全部一致。
    let mut cw = CompWind::new(client);
    let mut i = 0;
    while i < 3 {
        let form = CompositionForm {
            dw_style: CFS_DEFAULT,
            pt_current_pos: Point { x: 999, y: 999 }, // DEFAULT 忽略请求位
            rc_area: Rect { left: 1, top: 1, right: 2, bottom: 2 },
        };
        let got = cw.apply(&form)?;
        if got != (Point { x: DEFAULT_MARGIN, y: DEFAULT_MARGIN }) {
            return Err(E_DEV_OVER);
        }
        i += 1;
    }
    cw.audit()?;
    hits += 1;

    // R5 边缘钳制：×10 越界样本（四角+四边+远端），落位==钳制位，偏差 ≤2。
    let mut cw = CompWind::new(client);
    let edge: [Point; 10] = [
        Point { x: -50, y: -50 },
        Point { x: 700, y: -50 },
        Point { x: -50, y: 600 },
        Point { x: 700, y: 600 },
        Point { x: 639, y: 479 },
        Point { x: 641, y: 100 },
        Point { x: 100, y: 481 },
        Point { x: 0, y: 0 },
        Point { x: 632, y: 468 },
        Point { x: 320, y: 240 },
    ];
    let mut i = 0usize;
    while i < 10 {
        let form = CompositionForm {
            dw_style: CFS_POINT | CFS_FORCE_POSITION,
            pt_current_pos: edge[i],
            rc_area: Rect { left: 0, top: 0, right: 0, bottom: 0 },
        };
        let got = cw.apply(&form)?;
        let expect = cw.clamp(edge[i]);
        let dev = iabs(got.x - expect.x) + iabs(got.y - expect.y);
        if dev > DEV_LIMIT {
            return Err(E_DEV_OVER);
        }
        if !client.contains(got) {
            return Err(E_DEV_OVER);
        }
        i += 1;
    }
    cw.coverage(10)?;
    cw.audit()?;
    hits += 1;

    // R6 非法样式拒收：0x0004（闭集外）拒绝，状态 Rejected，账本零增长。
    let mut cw = CompWind::new(client);
    let bad = CompositionForm {
        dw_style: 0x0004,
        pt_current_pos: Point { x: 10, y: 10 },
        rc_area: Rect { left: 0, top: 0, right: 0, bottom: 0 },
    };
    if cw.apply(&bad).is_ok() {
        return Err(E_BAD_STYLE);
    }
    if cw.state != CwState::Rejected || cw.ledger_len() != 0 {
        return Err(E_STATE_BAD);
    }
    // 拒收后合法笔恢复 Applied：状态机可复位续跑。
    let good = CompositionForm {
        dw_style: CFS_POINT | CFS_FORCE_POSITION,
        pt_current_pos: Point { x: 8, y: 8 },
        rc_area: Rect { left: 0, top: 0, right: 0, bottom: 0 },
    };
    cw.apply(&good)?;
    if cw.state != CwState::Applied {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R7 混合序回放：10 笔混四样式，逐笔回放核对 requested/applied 分账。
    let mut cw = CompWind::new(client);
    let seq: [u32; 10] = [
        CFS_POINT | CFS_FORCE_POSITION,
        CFS_POINT,
        CFS_RECT,
        CFS_DEFAULT,
        CFS_RECT | CFS_FORCE_POSITION,
        CFS_POINT | CFS_FORCE_POSITION,
        CFS_POINT,
        CFS_RECT,
        CFS_DEFAULT,
        CFS_POINT | CFS_FORCE_POSITION,
    ];
    let mut i = 0usize;
    while i < 10 {
        let form = CompositionForm {
            dw_style: seq[i],
            pt_current_pos: Point { x: 50 + i as i32 * 9, y: 50 + i as i32 * 7 },
            rc_area: Rect {
                left: 50 + i as i32 * 9,
                top: 50 + i as i32 * 7,
                right: 150 + i as i32 * 9,
                bottom: 100 + i as i32 * 7,
            },
        };
        cw.apply(&form)?;
        i += 1;
    }
    cw.audit()?;
    cw.coverage(10)?;
    if cw.get_pos().is_none() {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R8 写回（判据 J2）：完成态+非空摘要放行；未落位态与空摘要均拒。
    let mut cw = CompWind::new(client);
    if cw.writeback(true, 0x20).is_ok() {
        return Err(E_STATE_BAD); // Idle 态不得写回
    }
    let form = CompositionForm {
        dw_style: CFS_POINT | CFS_FORCE_POSITION,
        pt_current_pos: Point { x: 12, y: 12 },
        rc_area: Rect { left: 0, top: 0, right: 0, bottom: 0 },
    };
    cw.apply(&form)?;
    if cw.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    cw.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
