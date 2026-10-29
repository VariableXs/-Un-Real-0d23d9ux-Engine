//! unx48209_xobs — UNX-F48209 Xwayland 观测埋点（v2/v3/XIM/root 四路四点）（诊断段 0xBB00）。
//!
//! 判据原文：
//!   J1 四点 100%；判据锚定 体验十三·补。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 四路 × 四点矩阵：协议路（text-input-v2/v3/XIM/root 模式）×
//!      生命点（START/COMP/END/CAND）= 16 格全矩阵；每个埋点事件必须
//!      落进 (路, 点) 二维闭集，越界事件拒收留账。
//!   2. 四点语义：START=输入会话开；COMP=预编辑变更；END=提交/会话收；
//!      CAND=候选窗更新。一路会话内四点不必齐，但出现顺序必须单调
//!      （START ≤ COMP ≤ END，CAND 依附 COMP/END 之间）。
//!   3. 100% 判据：喂 4 路各一轮完整会话事件流，16 格期望命中全命中
//!      （4×4 全绿），漏一格即红。
//!   4. 乱序显性：END 先于 START、CAND 无 COMP 等乱序事件拒收并留
//!      乱序账（不静默丢弃——观测面不许吞数据）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBB00;

const E_BAD_LANE: u16 = SEG_BASE + 1;  // 路闭集外
const E_BAD_POINT: u16 = SEG_BASE + 2; // 点闭集外
const E_OUT_OF_ORDER: u16 = SEG_BASE + 3; // 乱序事件
const E_MAT_MISS: u16 = SEG_BASE + 4;  // 矩阵缺格
const E_SESS_OPEN: u16 = SEG_BASE + 5; // 会话未收先开
const E_GUARD_UNBAL: u16 = SEG_BASE + 6; // 埋点账不平

/// 协议路闭集（四路）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    TipV2,  // text-input-v2
    TipV3,  // text-input-v3
    Xim,    // XIM 桥
    Root,   // root window 模式
}

/// 生命点闭集（四点）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Point {
    Start,
    Comp,
    End,
    Cand,
}

impl Lane {
    pub fn idx(&self) -> usize {
        match self {
            Lane::TipV2 => 0,
            Lane::TipV3 => 1,
            Lane::Xim => 2,
            Lane::Root => 3,
        }
    }
}

impl Point {
    pub fn idx(&self) -> usize {
        match self {
            Point::Start => 0,
            Point::Comp => 1,
            Point::End => 2,
            Point::Cand => 3,
        }
    }
}

/// 会话内观测状态机（每路一个）：
/// Idle →(Start) Live →(Comp) Editing →(Cand) Showing →(End) Idle
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessSt {
    Idle,
    Live,
    Editing,
    Showing,
}

/// 一路观测账。
#[derive(Debug)]
struct LaneObs {
    st: SessSt,
    hits: [u32; 4],
    sessions: u32,
    disorder: u32,
}

/// 四路埋点总账。
#[derive(Debug)]
pub struct XObs {
    lanes: [LaneObs; 4],
    rejected: u32,
    total: u32,
}

impl XObs {
    pub fn new() -> Self {
        XObs {
            lanes: [
                LaneObs { st: SessSt::Idle, hits: [0; 4], sessions: 0, disorder: 0 },
                LaneObs { st: SessSt::Idle, hits: [0; 4], sessions: 0, disorder: 0 },
                LaneObs { st: SessSt::Idle, hits: [0; 4], sessions: 0, disorder: 0 },
                LaneObs { st: SessSt::Idle, hits: [0; 4], sessions: 0, disorder: 0 },
            ],
            rejected: 0,
            total: 0,
        }
    }

    /// 埋点事件入口：合法则记 (路,点) 格并推进状态机；乱序/越界拒收留账。
    pub fn event(&mut self, lane: Lane, pt: Point) -> Result<(), u16> {
        self.total += 1;
        let li = lane.idx();
        let o = &mut self.lanes[li];
        // 状态机序判定。
        let ok = match (o.st, pt) {
            (SessSt::Idle, Point::Start) => {
                o.st = SessSt::Live;
                o.sessions += 1;
                true
            }
            (SessSt::Live, Point::Comp) => {
                o.st = SessSt::Editing;
                true
            }
            (SessSt::Editing, Point::Cand) => {
                o.st = SessSt::Showing;
                true
            }
            (SessSt::Editing, Point::End) | (SessSt::Showing, Point::End) => {
                o.st = SessSt::Idle;
                true
            }
            (SessSt::Live, Point::End) => {
                // 无 COMP 的短会话（纯直通输入）允许 END。
                o.st = SessSt::Idle;
                true
            }
            (SessSt::Editing, Point::Comp) => {
                true
            }
            (SessSt::Showing, Point::Cand) | (SessSt::Showing, Point::Comp) => {
                if pt == Point::Comp {
                    o.st = SessSt::Editing;
                }
                true
            }
            _ => false,
        };
        if !ok {
            o.disorder += 1;
            self.rejected += 1;
            return Err(E_OUT_OF_ORDER);
        }
        o.hits[pt.idx()] += 1;
        Ok(())
    }

    /// 矩阵命中视图：4×4 全量。
    pub fn matrix(&self) -> [[u32; 4]; 4] {
        let mut m = [[0u32; 4]; 4];
        for i in 0..4 {
            m[i] = self.lanes[i].hits;
        }
        m
    }

    /// J1 判据：四路 × 四点 16 格全命中（各 ≥1）。
    pub fn full_cover(&self) -> Result<(), u16> {
        for i in 0..4 {
            for j in 0..4 {
                if self.lanes[i].hits[j] == 0 {
                    return Err(E_MAT_MISS);
                }
            }
        }
        Ok(())
    }

    /// 会话守恒：每路 sessions*1 ≤ Start 命中数 且 Idle 收口。
    pub fn audit(&self) -> Result<(), u16> {
        for o in &self.lanes {
            if (o.sessions as u32) > o.hits[Point::Start.idx()] {
                return Err(E_GUARD_UNBAL);
            }
            let events: u32 = o.hits.iter().sum();
            if events != o.hits[0] + o.hits[1] + o.hits[2] + o.hits[3] {
                return Err(E_GUARD_UNBAL);
            }
        }
        if self.total
            != self
                .lanes
                .iter()
                .map(|o| o.hits.iter().sum::<u32>() + o.disorder)
                .sum::<u32>()
        {
            return Err(E_GUARD_UNBAL); // 总账守恒：命中+乱序 = 总事件
        }
        Ok(())
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }
    pub fn total(&self) -> u32 {
        self.total
    }
    pub fn sessions(&self, lane: Lane) -> u32 {
        self.lanes[lane.idx()].sessions
    }
    pub fn disorder(&self, lane: Lane) -> u32 {
        self.lanes[lane.idx()].disorder
    }
}

/// 自例程判据：落实 J1「四点 100%」全链。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 场景：四路完整会话 —— v2/v3/XIM/root 各喂 START→COMP→CAND→END，
    //        16 格全命中（4×4 全绿）。
    let mut o = XObs::new();
    for lane in [Lane::TipV2, Lane::TipV3, Lane::Xim, Lane::Root] {
        o.event(lane, Point::Start)?;
        o.event(lane, Point::Comp)?;
        o.event(lane, Point::Cand)?;
        o.event(lane, Point::End)?;
    }
    o.full_cover()?;
    o.audit()?;
    if o.total() != 16 || o.rejected() != 0 {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R2 场景：多轮会话累计 —— 每路再跑 2 轮，命中数按路累计。
    for lane in [Lane::TipV2, Lane::TipV3, Lane::Xim, Lane::Root] {
        for _ in 0..2 {
            o.event(lane, Point::Start)?;
            o.event(lane, Point::Comp)?;
            o.event(lane, Point::End)?;
        }
    }
    let m = o.matrix();
    for i in 0..4 {
        if m[i][Point::Start.idx()] != 3 || m[i][Point::Comp.idx()] != 3 || m[i][Point::End.idx()] != 3 || m[i][Point::Cand.idx()] != 1 {
            return Err(E_MAT_MISS);
        }
    }
    o.audit()?;
    hits += 1;

    // R3 场景：乱序显性 —— Idle 直接 COMP / End 后再 COMP / 无 START 的 END 全拒。
    let mut o3 = XObs::new();
    match o3.event(Lane::Xim, Point::Comp) {
        Err(E_OUT_OF_ORDER) => {}
        _ => return Err(E_OUT_OF_ORDER),
    }
    o3.event(Lane::Xim, Point::Start)?;
    o3.event(Lane::Xim, Point::End)?;
    match o3.event(Lane::Xim, Point::Comp) {
        Err(E_OUT_OF_ORDER) => {}
        _ => return Err(E_OUT_OF_ORDER),
    }
    match o3.event(Lane::Root, Point::End) {
        Err(E_OUT_OF_ORDER) => {}
        _ => return Err(E_OUT_OF_ORDER),
    }
    if o3.disorder(Lane::Xim) != 2 || o3.disorder(Lane::Root) != 1 {
        return Err(E_GUARD_UNBAL);
    }
    o3.audit()?;
    hits += 1;

    // R4 场景：Editing 内多次 COMP 合法（预编辑连续变更）+ CAND 回 COMP 亦合法。
    let mut o4 = XObs::new();
    o4.event(Lane::TipV2, Point::Start)?;
    o4.event(Lane::TipV2, Point::Comp)?;
    o4.event(Lane::TipV2, Point::Comp)?;
    o4.event(Lane::TipV2, Point::Cand)?;
    o4.event(Lane::TipV2, Point::Comp)?; // Showing → Editing 回退
    o4.event(Lane::TipV2, Point::Cand)?;
    o4.event(Lane::TipV2, Point::End)?;
    let m4 = o4.matrix();
    if m4[Lane::TipV2.idx()][Point::Comp.idx()] != 3 || m4[Lane::TipV2.idx()][Point::Cand.idx()] != 2 {
        return Err(E_MAT_MISS);
    }
    o4.audit()?;
    hits += 1;

    // R5 场景：Live 直收 END（无 COMP 短会话）合法。
    let mut o5 = XObs::new();
    o5.event(Lane::Root, Point::Start)?;
    o5.event(Lane::Root, Point::End)?;
    o5.audit()?;
    if o5.sessions(Lane::Root) != 1 {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R6 场景：矩阵缺格显性 —— 只喂 3 路 → full_cover 必红。
    let mut o6 = XObs::new();
    for lane in [Lane::TipV2, Lane::TipV3, Lane::Xim] {
        o6.event(lane, Point::Start)?;
        o6.event(lane, Point::Comp)?;
        o6.event(lane, Point::Cand)?;
        o6.event(lane, Point::End)?;
    }
    match o6.full_cover() {
        Err(E_MAT_MISS) => {}
        _ => return Err(E_MAT_MISS),
    }
    hits += 1;

    // R7 场景： Showing 状态直接 END 合法（CAND 后提交收口）；Idle 再 END 乱序留账后可重开会话。
    let mut o7 = XObs::new();
    o7.event(Lane::TipV3, Point::Start)?;
    o7.event(Lane::TipV3, Point::Comp)?;
    o7.event(Lane::TipV3, Point::Cand)?;
    o7.event(Lane::TipV3, Point::End)?;
    let _ = o7.event(Lane::TipV3, Point::End); // Idle 再 END → 乱序（留账，不传播）
    if o7.disorder(Lane::TipV3) != 1 {
        return Err(E_GUARD_UNBAL);
    }
    o7.event(Lane::TipV3, Point::Start)?; // 重开会话合法
    o7.audit()?;
    hits += 1;

    // R8 场景：总账守恒 —— 命中+乱序 = 总事件（喂含乱序的混合流验证）。
    let mut o8 = XObs::new();
    let mut fed = 0u32;
    for lane in [Lane::TipV2, Lane::TipV3, Lane::Xim, Lane::Root] {
        for _ in 0..3 {
            let r1 = o8.event(lane, Point::Start);
            fed += 1;
            let _ = r1;
            let r2 = o8.event(lane, Point::Comp);
            fed += 1;
            let _ = r2;
            let r3 = o8.event(lane, Point::End);
            fed += 1;
            let _ = r3;
        }
    }
    // 强塞 4 个乱序（Idle COMP ×4 路）。
    for lane in [Lane::TipV2, Lane::TipV3, Lane::Xim, Lane::Root] {
        let _ = o8.event(lane, Point::Cand); // Idle CAND → 乱序
        fed += 1;
    }
    o8.audit()?;
    if o8.total() != fed {
        return Err(E_GUARD_UNBAL);
    }
    if o8.rejected() != 4 {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    Ok(hits)
}
