//! cgb849_amdarch —— CGPU-F0849 AMD 开工与架构适配总览。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0849 AMD 开工与架构适配总览  <SEG 0x74A0>
//! 目标行数：≥380（判据：纯功能代码 ≥380 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥380 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0849；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（AMD 开工与架构适配总览，整数账面）：
//!   1. 架构代际闭集五档（写死）：GCN4 / GCN5 / RDNA1 / RDNA2 /
//!      RDNA3；代际号 0..=4，越界红码（XC_BAD_GEN）；
//!   2. 适配工作项闭集八类（写死）：能力探测 / 驱动差异 / 显存
//!      适配 / 媒体引擎 / 功耗画像 / 遥测 / fuzz / API 冻结；每类
//!      每代际一个开工状态机：Idle → Prep → Active → Done；
//!   3. 开工账守恒：立项数 = 完工数 + 在做数 + 待动数；漂移红码
//!      （XC_KANBAN_DRIFT）；
//!   4. 里程碑账：每代际定容里程碑环（4 槽），入账 = 在环 + 归档；
//!   5. 完工判据：八类工作项在目标代际全部 Done 且里程碑齐备，
//!      未齐报完工红码（XC_PREMATURE_DONE）；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x74A0 段为本模块独占（SEG_BASE=0x74A0），判别式
//!   SEG_BASE+1..+N；与 0x7480/0x74C0 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const AA_VERSION: &str = "cgb849_amdarch-v1";

/// 诊断段基址（0x74A0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x74A0;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x74A1 代际越界（0..=4 之外）。
pub const XC_BAD_GEN: u16 = SEG_BASE + 1;
/// 0x74A2 工作项类别越界（0..=7 之外）。
pub const XC_BAD_WORK: u16 = SEG_BASE + 3 - 2;
/// 0x74A3 看板账漂移（立项 ≠ 完工 + 在做 + 待动）。
pub const XC_KANBAN_DRIFT: u16 = SEG_BASE + 3;
/// 0x74A4 状态迁移非法。
pub const XC_BAD_TRANS: u16 = SEG_BASE + 4;
/// 0x74A5 里程碑账漂移。
pub const XC_MILE_DRIFT: u16 = SEG_BASE + 5;
/// 0x74A6 未齐完工（工作项或里程碑不足即报 Done）。
pub const XC_PREMATURE_DONE: u16 = SEG_BASE + 6;
/// 0x74A7 里程碑重复入账。
pub const XC_MILE_DUP: u16 = SEG_BASE + 7;
/// 0x74A8 工作项未立项即操作。
pub const XC_NOT_OPENED: u16 = SEG_BASE + 8;
/// 0x74A9 里程碑号越界。
pub const XC_BAD_MILE: u16 = SEG_BASE + 9;
/// 0x74AA 总览未齐即出报告。
pub const XC_OVERVIEW_INCOMPLETE: u16 = SEG_BASE + 10;

/// 架构代际数（写死：5）。
pub const GEN_CAP: usize = 5;
/// 工作项类别数（写死：8）。
pub const WORK_CAP: usize = 8;
/// 里程碑环容量（写死：4）。
pub const MILE_CAP: usize = 4;
/// 里程碑闭集数（每代际 4 座）。
pub const MILE_SET: u32 = 4;

/// 代际闭集：0=GCN4 1=GCN5 2=RDNA1 3=RDNA2 4=RDNA3。
pub const G_GCN4: usize = 0;
pub const G_GCN5: usize = 1;
pub const G_RDNA1: usize = 2;
pub const G_RDNA2: usize = 3;
pub const G_RDNA3: usize = 4;

/// 工作项闭集：0=能力探测 1=驱动差异 2=显存适配 3=媒体引擎
/// 4=功耗画像 5=遥测 6=fuzz 7=API 冻结。
pub const W_CAPS: usize = 0;
pub const W_DRVDF: usize = 1;
pub const W_VRAM: usize = 2;
pub const W_MEDIA: usize = 3;
pub const W_POWER: usize = 4;
pub const W_TELEM: usize = 5;
pub const W_FUZZ: usize = 6;
pub const W_FREEZE: usize = 7;

/// 代际名（闭集原文）。
pub fn gen_name(g: usize) -> &'static str {
    match g {
        G_GCN4 => "GCN4",
        G_GCN5 => "GCN5",
        G_RDNA1 => "RDNA1",
        G_RDNA2 => "RDNA2",
        _ => "RDNA3",
    }
}

/// 工作项名（闭集原文）。
pub fn work_name(w: usize) -> &'static str {
    match w {
        W_CAPS => "caps",
        W_DRVDF => "drvdiff",
        W_VRAM => "vram",
        W_MEDIA => "media",
        W_POWER => "power",
        W_TELEM => "telem",
        W_FUZZ => "fuzz",
        _ => "freeze",
    }
}

/// 工作项状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WState {
    /// 待动。
    Idle,
    /// 备料。
    Prep,
    /// 在做。
    Active,
    /// 完工。
    Done,
}

/// 状态迁移合法性（Idle→Prep→Active→Done，Active 可回 Prep）。
pub fn trans_legal(from: WState, to: WState) -> bool {
    matches!(
        (from, to),
        (WState::Idle, WState::Prep)
            | (WState::Prep, WState::Active)
            | (WState::Prep, WState::Idle)
            | (WState::Active, WState::Prep)
            | (WState::Active, WState::Done)
    )
}

/// 单座里程碑。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Milestone {
    /// 座号（1..=4）。
    pub no: u32,
    /// 达成轮次。
    pub round: u32,
}

/// 代际看板。
pub struct GenBoard {
    /// 代际号。
    pub gen: usize,
    /// 八类工作项状态。
    pub work: [WState; WORK_CAP],
    /// 里程碑账（定容 4）。
    pub miles: Vec<Milestone>,
    /// 里程碑归档数（环满后覆盖计入）。
    pub mile_archived: u64,
    /// 立项数（离开 Idle 计一次）。
    pub opened: u64,
    /// 完工数。
    pub done: u64,
    /// 看板归档标志。
    pub sealed: bool,
}

impl GenBoard {
    /// 开看板（全 Idle、无里程碑）。
    pub fn new(gen: usize) -> Result<Self, u16> {
        if gen >= GEN_CAP {
            return Err(XC_BAD_GEN);
        }
        Ok(GenBoard {
            gen,
            work: [WState::Idle; WORK_CAP],
            miles: Vec::new(),
            mile_archived: 0,
            opened: 0,
            done: 0,
            sealed: false,
        })
    }

    /// 推进工作项（非法迁移、未立项、代际已封即红）。
    pub fn advance(&mut self, w: usize, to: WState) -> Result<(), u16> {
        if self.sealed {
            return Err(XC_PREMATURE_DONE);
        }
        if w >= WORK_CAP {
            return Err(XC_BAD_WORK);
        }
        let cur = self.work[w];
        if !trans_legal(cur, to) {
            return Err(XC_BAD_TRANS);
        }
        if cur == WState::Idle {
            self.opened += 1;
        }
        if to == WState::Done {
            // 完工前置：必须先过 Prep 与 Active（由真值表保证）。
            self.done += 1;
        }
        self.work[w] = to;
        Ok(())
    }

    /// 达成里程碑（座号 1..=4、不重复、账满覆盖）。
    pub fn hit_mile(&mut self, no: u32, round: u32) -> Result<(), u16> {
        if no == 0 || no > MILE_SET {
            return Err(XC_BAD_MILE);
        }
        let mut i = 0;
        while i < self.miles.len() {
            if self.miles[i].no == no {
                return Err(XC_MILE_DUP);
            }
            i += 1;
        }
        if self.miles.len() == MILE_CAP {
            self.miles.remove(0);
            self.mile_archived += 1;
        }
        self.miles.push(Milestone { no, round });
        Ok(())
    }

    /// 里程碑账守恒：座号集 = 4 且无重复即齐备。
    pub fn miles_complete(&self) -> bool {
        let mut mask: u32 = 0;
        let mut i = 0;
        while i < self.miles.len() {
            mask |= 1u32 << (self.miles[i].no - 1);
            i += 1;
        }
        mask == 0b1111
    }

    /// 看板账守恒：立项 = 完工 + 在做 + 待动（按当前态盘点）。
    pub fn kanban_balanced(&self) -> bool {
        let mut done = 0u64;
        let mut inflight = 0u64;
        let mut idle = 0u64;
        let mut w = 0;
        while w < WORK_CAP {
            match self.work[w] {
                WState::Done => done += 1,
                WState::Active | WState::Prep => inflight += 1,
                WState::Idle => idle += 1,
            }
            w += 1;
        }
        self.opened == done + inflight + idle
    }

    /// 看板封存（八项全 Done 且里程碑齐备）。
    pub fn seal(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Ok(());
        }
        let mut w = 0;
        while w < WORK_CAP {
            if self.work[w] != WState::Done {
                return Err(XC_PREMATURE_DONE);
            }
            w += 1;
        }
        if !self.miles_complete() {
            return Err(XC_PREMATURE_DONE);
        }
        if !self.kanban_balanced() {
            return Err(XC_KANBAN_DRIFT);
        }
        self.sealed = true;
        Ok(())
    }
}

/// AMD 开工与架构适配总览。
pub struct AmdOverview {
    /// 五代际看板。
    pub boards: [Option<GenBoard>; GEN_CAP],
    /// 累计红码次数。
    pub red_count: u64,
}

impl AmdOverview {
    /// 开总览（空板）。
    pub fn new() -> Self {
        AmdOverview {
            boards: [None, None, None, None, None],
            red_count: 0,
        }
    }

    /// 立代际看板。
    pub fn open_gen(&mut self, gen: usize) -> Result<(), u16> {
        if gen >= GEN_CAP {
            self.red_count += 1;
            return Err(XC_BAD_GEN);
        }
        if self.boards[gen].is_some() {
            self.red_count += 1;
            return Err(XC_MILE_DUP);
        }
        self.boards[gen] = Some(GenBoard::new(gen)?);
        Ok(())
    }

    /// 按代际取可变看板（未立项红码）。
    fn board_mut(&mut self, gen: usize) -> Result<&mut GenBoard, u16> {
        if gen >= GEN_CAP {
            return Err(XC_BAD_GEN);
        }
        match self.boards[gen].as_mut() {
            Some(b) => Ok(b),
            None => Err(XC_NOT_OPENED),
        }
    }

    /// 推进某代际某工作项。
    pub fn advance(&mut self, gen: usize, w: usize, to: WState) -> Result<(), u16> {
        if self.board_mut(gen)?.advance(w, to).is_err() {
            self.red_count += 1;
            return Err(XC_BAD_TRANS);
        }
        Ok(())
    }

    /// 达成某代际里程碑。
    pub fn hit_mile(&mut self, gen: usize, no: u32, round: u32) -> Result<(), u16> {
        if self.board_mut(gen)?.hit_mile(no, round).is_err() {
            self.red_count += 1;
            return Err(XC_MILE_DUP);
        }
        Ok(())
    }

    /// 封某代际看板。
    pub fn seal_gen(&mut self, gen: usize) -> Result<(), u16> {
        if self.board_mut(gen)?.seal().is_err() {
            self.red_count += 1;
            return Err(XC_PREMATURE_DONE);
        }
        Ok(())
    }

    /// 总览齐备判定：五代际全部立项且封存。
    pub fn complete(&self) -> bool {
        let mut g = 0;
        while g < GEN_CAP {
            match &self.boards[g] {
                None => return false,
                Some(b) => {
                    if !b.sealed {
                        return false;
                    }
                }
            }
            g += 1;
        }
        true
    }

    /// 出总览报告（未齐备红码；齐备给摘要）。
    pub fn report(&self) -> Result<String, u16> {
        if !self.complete() {
            return Err(XC_OVERVIEW_INCOMPLETE);
        }
        let mut s = String::from("amdarch overview:");
        let mut g = 0;
        while g < GEN_CAP {
            s += " ";
            s += gen_name(g);
            s += "=done";
            g += 1;
        }
        Ok(s)
    }

    /// 全账自检（各看板看板账 + 里程碑账）。
    pub fn audit(&self) -> Result<(), u16> {
        let mut g = 0;
        while g < GEN_CAP {
            if let Some(b) = &self.boards[g] {
                if !b.kanban_balanced() {
                    return Err(XC_KANBAN_DRIFT);
                }
                if b.miles.len() > MILE_CAP {
                    return Err(XC_MILE_DRIFT);
                }
            }
            g += 1;
        }
        Ok(())
    }
}

/// 自例程：五代际各开板 → 八项推进 → 里程碑齐 → 封板 → 报告。
pub fn drill() -> Result<u32, u16> {
    let mut ov = AmdOverview::new();
    let mut g = 0;
    while g < GEN_CAP {
        ov.open_gen(g)?;
        g += 1;
    }
    // 负例：重复开板。
    if ov.open_gen(G_RDNA2) != Err(XC_MILE_DUP) {
        return Err(XC_MILE_DUP);
    }
    // 负例：代际越界。
    if ov.open_gen(GEN_CAP) != Err(XC_BAD_GEN) {
        return Err(XC_BAD_GEN);
    }
    // 负例：未立项代际操作。
    let mut solo = AmdOverview::new();
    if solo.advance(G_RDNA3, W_CAPS, WState::Prep) != Err(XC_NOT_OPENED) {
        return Err(XC_NOT_OPENED);
    }
    // 正链：RDNA2 八项全推完 + 四里程碑。
    let mut w = 0;
    let mut round: u32 = 0;
    while w < WORK_CAP {
        ov.advance(G_RDNA2, w, WState::Prep)?;
        ov.advance(G_RDNA2, w, WState::Active)?;
        ov.advance(G_RDNA2, w, WState::Done)?;
        w += 1;
    }
    let mut m = 1;
    while m <= MILE_SET {
        round += 10;
        ov.hit_mile(G_RDNA2, m, round)?;
        m += 1;
    }
    // 负例：重复里程碑。
    if ov.hit_mile(G_RDNA2, 2, 99) != Err(XC_MILE_DUP) {
        return Err(XC_MILE_DUP);
    }
    ov.seal_gen(G_RDNA2)?;
    // 负例：封板后再推。
    if ov.advance(G_RDNA2, W_CAPS, WState::Prep) != Err(XC_BAD_TRANS) {
        return Err(XC_BAD_TRANS);
    }
    // 负例：未齐总览出报告。
    if ov.report() != Err(XC_OVERVIEW_INCOMPLETE) {
        return Err(XC_OVERVIEW_INCOMPLETE);
    }
    // 其余代际全推完。
    let mut g2 = 0;
    while g2 < GEN_CAP {
        if g2 != G_RDNA2 {
            let mut w2 = 0;
            let mut r2: u32 = 0;
            while w2 < WORK_CAP {
                ov.advance(g2, w2, WState::Prep)?;
                ov.advance(g2, w2, WState::Active)?;
                ov.advance(g2, w2, WState::Done)?;
                w2 += 1;
            }
            let mut m2 = 1;
            while m2 <= MILE_SET {
                r2 += 5;
                ov.hit_mile(g2, m2, r2)?;
                m2 += 1;
            }
            ov.seal_gen(g2)?;
        }
        g2 += 1;
    }
    ov.audit()?;
    let rep = ov.report()?;
    if !ov.complete() || rep.len() == 0 {
        return Err(XC_OVERVIEW_INCOMPLETE);
    }
    // 负例：非法迁移直跳。
    let mut probe = GenBoard::new(G_GCN4)?;
    if probe.advance(W_VRAM, WState::Done) != Err(XC_BAD_TRANS) {
        return Err(XC_BAD_TRANS);
    }
    probe.advance(W_VRAM, WState::Prep)?;
    probe.advance(W_VRAM, WState::Idle)?;
    probe.advance(W_VRAM, WState::Prep)?;
    probe.advance(W_VRAM, WState::Active)?;
    probe.advance(W_VRAM, WState::Done)?;
    if !probe.kanban_balanced() {
        return Err(XC_KANBAN_DRIFT);
    }
    Ok(0)
}
