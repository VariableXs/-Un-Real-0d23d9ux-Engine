//! VE-F3569 · 一致性违规报告与处置 —— 三段流（编号/分级/闭环）+ 责任标注 + 闭环率 + 时限。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3569`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三段流**（[`DispositionFlow`]）：编号登记 / 分级处置 /
//!   闭环核验三段。
//! - **责任标注**：每条违规带责任人。
//! - **闭环率看板**（[`ClosureBoard`]）：闭环率可见。
//! - **处置时限分级**：阻断级 48h / 建议级 2 周两档。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 未闭环 | 催办 |
//! | 责任人缺 | 补标 |
//! | 分级错 | 复评 |
//!
//! # 复杂度
//!
//! O(违规)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E20 段续：6560 起）
// ---------------------------------------------------------------------------

/// 处置流诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DispoCode {
    /// 未闭环（催办）。
    NotClosed = 6560,
    /// 责任人缺失。
    OwnerMissing = 6561,
    /// 分级错误。
    GradeWrong = 6562,
}

impl DispoCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            DispoCode::NotClosed => "未闭环催办",
            DispoCode::OwnerMissing => "责任人缺失",
            DispoCode::GradeWrong => "分级错误",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 二、分级与时限
// ---------------------------------------------------------------------------

/// 违规分级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViolGrade {
    /// 阻断级（48h 时限）。
    Blocking,
    /// 建议级（2 周时限）。
    Advisory,
}

impl ViolGrade {
    /// 时限（小时）。
    pub const fn deadline_hours(self) -> u32 {
        match self {
            ViolGrade::Blocking => 48,
            ViolGrade::Advisory => 14 * 24,
        }
    }

    /// 级短名。
    pub const fn label(self) -> &'static str {
        match self {
            ViolGrade::Blocking => "阻断级",
            ViolGrade::Advisory => "建议级",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、违规单与处置流
// ---------------------------------------------------------------------------

/// 违规单。
#[derive(Clone, Debug)]
pub struct ViolTicket {
    /// 编号。
    pub id: u32,
    /// 分级。
    pub grade: ViolGrade,
    /// 责任人（0 = 未标）。
    pub owner: u32,
    /// 是否已闭环。
    pub closed: bool,
}

/// 处置流。
pub struct DispositionFlow {
    tickets: Vec<ViolTicket>,
    next_id: u32,
}

impl DispositionFlow {
    /// 新建。
    pub fn new() -> Self {
        Self {
            tickets: Vec::new(),
            next_id: 1,
        }
    }

    /// 登记（编号自动分配，责任必标）。
    pub fn register(&mut self, grade: ViolGrade, owner: u32) -> Result<u32, DispoCode> {
        if owner == 0 {
            return Err(DispoCode::OwnerMissing);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.tickets.push(ViolTicket {
            id,
            grade,
            owner,
            closed: false,
        });
        Ok(id)
    }

    /// 闭环核验。
    pub fn close(&mut self, id: u32) -> bool {
        for t in self.tickets.iter_mut() {
            if t.id == id {
                t.closed = true;
                return true;
            }
        }
        false
    }

    /// 未闭环列表（催办）。
    pub fn overdue(&self) -> Vec<u32> {
        self.tickets
            .iter()
            .filter(|t| !t.closed)
            .map(|t| t.id)
            .collect()
    }

    /// 在册数。
    pub fn len(&self) -> usize {
        self.tickets.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.tickets.is_empty()
    }
}

impl Default for DispositionFlow {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 四、闭环率看板
// ---------------------------------------------------------------------------

/// 闭环率看板。
pub struct ClosureBoard;

impl ClosureBoard {
    /// 闭环率（千分比）。
    pub fn rate(flow: &DispositionFlow) -> u32 {
        if flow.tickets.is_empty() {
            return 1000;
        }
        let closed = flow.tickets.iter().filter(|t| t.closed).count();
        (closed as u32 * 1000) / flow.tickets.len() as u32
    }
}

// ---------------------------------------------------------------------------
// 五、判据
// ---------------------------------------------------------------------------

/// 处置流判据集。
pub fn run_dispo_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver11i");

    // 1. 登记编号递增。
    let mut f = DispositionFlow::new();
    let id1 = f.register(ViolGrade::Blocking, 7).unwrap_or(0);
    let id2 = f.register(ViolGrade::Advisory, 8).unwrap_or(0);
    chk.add("6560-编号递增", id1 == 1 && id2 == 2, "");

    // 2. 责任人缺失拒。
    chk.add(
        "6561-责任拒",
        f.register(ViolGrade::Blocking, 0).err() == Some(DispoCode::OwnerMissing),
        "",
    );

    // 3. 闭环。
    chk.add("6562-闭环", f.close(1) && f.overdue().len() == 1, "");

    // 4. 闭环率。
    chk.add("6563-闭环率", ClosureBoard::rate(&f) == 500, "");

    // 5. 时限分级。
    chk.add(
        "6564-时限分级",
        ViolGrade::Blocking.deadline_hours() == 48
            && ViolGrade::Advisory.deadline_hours() == 336,
        "",
    );

    // 6. 空看板满分。
    let empty = DispositionFlow::new();
    chk.add("6565-空看板", ClosureBoard::rate(&empty) == 1000, "");

    chk
}
