//! UNX-F50027 布局与双面联签（N-up+duplex 组合页序单源）（0x42f80 段）
//!
//! 锚点：UNX-F50027-J1 组合 ×20 页序一致，B24 联签零改写；判据锚定 §AI-63 §1 防重声明。
//!
//! 数据段：`SEG_BASE = 0x42f80`，本模块独占 0x20；诊断码闭集为
//! `0x42f80 + 1..=12`（`LayoutError` 十二型）。新增码必须整段扩容，
//! 不得借位相邻卡（0x42f60 旋转卡 / 0x42fa0 预览卡）。
//!
//! 本模块结构：
//! - 拼版计划：`NUp`（七档：1/2/4/6/8/9/16）与 `Duplex`（单面/长边/短边）
//!   组合出 `Placement` 列表；`slot` 是**单源阅读槽位**（页序的唯一真值），
//!   `cell` 是**物理格位**（双面翻页后可能镜像），两者分离处置——这是
//!   页序一致与「B24 联签零改写」能同时成立的先决条件；
//! - 联签记录：`JointSign`（计划摘要 + 签署人 + B24 盐封签），`seal()` 只
//!   接受通过 `verify_plan()` 的计划；封签后 `repage()` 一律 `SealedPlan`；
//! - 状态机：未封签（可重排页数）→ 封签（内容摘要冻结）；摘要是内容的
//!   纯函数，同一 (页数, N-up, 双面) 重算必然逐位相同，因此「零改写」可判；
//! - 错误路径：空活 / 非法 N-up / 超页数 / 格越界 / 页号重复 / 未知页 /
//!   页序断裂 / 封签后改写 / 网格溢出 / 超张数 / 非法双面 / 摘要不符；
//! - 注入核：`injection_core()` 构造 6 类坏样本（空活、N-up=7、超页数、
//!   页号重复、封签后改写、摘要翻转），逐类断言被显式拒绝。
//!
//! 零 panic 纪律：所有入口返回 `Result<_, LayoutError>`；容量相关运算先用
//! `u32` 承载再转型，避免 `u16` 溢出；测试块之外不做空解包、不触发断言。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x42f80 段，独占 0x20）。
pub const SEG_BASE: u32 = 0x42f80;

/// 单活页数上限：4000 页对应 8 开 16-up 双面 125 张，已覆盖办公上限；
/// 再大不是拼版问题而是作业拆分问题，显式拒绝比静默截断更可审计。
pub const MAX_PAGES: u16 = 4000;
/// 单活张数上限：999 张是 PCL/PS 驱动侧的通用上限，超出必然在驱动侧再拒。
pub const MAX_SHEETS: u16 = 999;
/// B24 联签盐：与摘要一起封入 `JointSign.seal`，防止摘要在联签后被重算附会。
pub const B24_SALT: u64 = 0xB24A_5E17_C0DE_0001;

const E_EMPTY: u16 = 1;
const E_NUP: u16 = 2;
const E_PAGES: u16 = 3;
const E_CELL: u16 = 4;
const E_DUP: u16 = 5;
const E_UNKNOWN_PAGE: u16 = 6;
const E_ORDER: u16 = 7;
const E_SEALED: u16 = 8;
const E_GRID: u16 = 9;
const E_SHEETS: u16 = 10;
const E_DUPLEX: u16 = 11;
const E_SIGN: u16 = 12;

/// 布局错误十二型。`code()` 恒为 `SEG_BASE + n`，与全仓诊断码闭集同构。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutError {
    EmptyJob,
    BadNUp,
    PageOverflow,
    CellOutOfRange,
    DuplicatePage,
    UnknownPage,
    OrderBroken,
    SealedPlan,
    GridOverflow,
    TooManySheets,
    BadDuplex,
    SignMismatch,
}

impl LayoutError {
    /// 段内序号（1 起），与 `code()` 一一对应。
    fn ordinal(self) -> u16 {
        match self {
            LayoutError::EmptyJob => E_EMPTY,
            LayoutError::BadNUp => E_NUP,
            LayoutError::PageOverflow => E_PAGES,
            LayoutError::CellOutOfRange => E_CELL,
            LayoutError::DuplicatePage => E_DUP,
            LayoutError::UnknownPage => E_UNKNOWN_PAGE,
            LayoutError::OrderBroken => E_ORDER,
            LayoutError::SealedPlan => E_SEALED,
            LayoutError::GridOverflow => E_GRID,
            LayoutError::TooManySheets => E_SHEETS,
            LayoutError::BadDuplex => E_DUPLEX,
            LayoutError::SignMismatch => E_SIGN,
        }
    }

    /// 诊断码：段基址 + 序号，闭集内唯一。
    pub fn code(self) -> u32 {
        SEG_BASE + self.ordinal() as u32
    }

    /// 一行诊断文本（日志/联签回执用）。
    pub fn message(self) -> &'static str {
        match self {
            LayoutError::EmptyJob => "job has no pages",
            LayoutError::BadNUp => "n-up value not supported",
            LayoutError::PageOverflow => "page count above job cap",
            LayoutError::CellOutOfRange => "cell index outside grid",
            LayoutError::DuplicatePage => "source page placed twice",
            LayoutError::UnknownPage => "source page outside 1..=pages",
            LayoutError::OrderBroken => "reading-slot order diverged",
            LayoutError::SealedPlan => "plan sealed by B24 joint sign",
            LayoutError::GridOverflow => "placement count vs grid mismatch",
            LayoutError::TooManySheets => "sheet count above cap",
            LayoutError::BadDuplex => "duplex mode code invalid",
            LayoutError::SignMismatch => "plan digest / seal mismatch",
        }
    }
}

/// 全错误表：判据据此验证「码值 = 基址 + 序号」的零漂移性质。
pub const ALL_ERRORS: [LayoutError; 12] = [
    LayoutError::EmptyJob,
    LayoutError::BadNUp,
    LayoutError::PageOverflow,
    LayoutError::CellOutOfRange,
    LayoutError::DuplicatePage,
    LayoutError::UnknownPage,
    LayoutError::OrderBroken,
    LayoutError::SealedPlan,
    LayoutError::GridOverflow,
    LayoutError::TooManySheets,
    LayoutError::BadDuplex,
    LayoutError::SignMismatch,
];

/// N-up 七档。档位取自驱动通用集：1/2/4/6/8/9/16，其余值一律 `BadNUp`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NUp {
    One,
    Two,
    Four,
    Six,
    Eight,
    Nine,
    Sixteen,
}

impl NUp {
    /// 全档位（判据遍历用）。
    pub const ALL: [NUp; 7] = [
        NUp::One,
        NUp::Two,
        NUp::Four,
        NUp::Six,
        NUp::Eight,
        NUp::Nine,
        NUp::Sixteen,
    ];

    /// 从外部码值解析；非档位值显式拒绝，不做「就近取整」的静默降级。
    pub fn from_code(v: u8) -> Result<NUp, LayoutError> {
        match v {
            1 => Ok(NUp::One),
            2 => Ok(NUp::Two),
            4 => Ok(NUp::Four),
            6 => Ok(NUp::Six),
            8 => Ok(NUp::Eight),
            9 => Ok(NUp::Nine),
            16 => Ok(NUp::Sixteen),
            _ => Err(LayoutError::BadNUp),
        }
    }

    pub fn code(self) -> u8 {
        match self {
            NUp::One => 1,
            NUp::Two => 2,
            NUp::Four => 4,
            NUp::Six => 6,
            NUp::Eight => 8,
            NUp::Nine => 9,
            NUp::Sixteen => 16,
        }
    }

    /// 每面格数。
    pub fn count(self) -> u8 {
        self.code()
    }

    /// 网格 (列, 行)。选择依据：6-up 走 3×2（横向阅读），8-up 走 4×2，
    /// 16-up 走 4×4；行列决定短边翻页的镜像轴向。
    pub fn grid(self) -> (u8, u8) {
        match self {
            NUp::One => (1, 1),
            NUp::Two => (2, 1),
            NUp::Four => (2, 2),
            NUp::Six => (3, 2),
            NUp::Eight => (4, 2),
            NUp::Nine => (3, 3),
            NUp::Sixteen => (4, 4),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            NUp::One => "1-up",
            NUp::Two => "2-up",
            NUp::Four => "4-up",
            NUp::Six => "6-up",
            NUp::Eight => "8-up",
            NUp::Nine => "9-up",
            NUp::Sixteen => "16-up",
        }
    }
}

/// 双面模式三档。短边翻页的镜像轴是「行」，长边是「整体反向」——两者在
/// 2 行以上网格上可区分，这正是判据要钉住的差异。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Duplex {
    Simplex,
    LongEdge,
    ShortEdge,
}

impl Duplex {
    pub fn from_code(v: u8) -> Result<Duplex, LayoutError> {
        match v {
            0 => Ok(Duplex::Simplex),
            1 => Ok(Duplex::LongEdge),
            2 => Ok(Duplex::ShortEdge),
            _ => Err(LayoutError::BadDuplex),
        }
    }

    pub fn code(self) -> u8 {
        match self {
            Duplex::Simplex => 0,
            Duplex::LongEdge => 1,
            Duplex::ShortEdge => 2,
        }
    }

    /// 每张纸的面数。
    pub fn sides(self) -> u8 {
        match self {
            Duplex::Simplex => 1,
            Duplex::LongEdge | Duplex::ShortEdge => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Duplex::Simplex => "simplex",
            Duplex::LongEdge => "duplex-long-edge",
            Duplex::ShortEdge => "duplex-short-edge",
        }
    }
}

/// 单条落位记录。`slot` 是阅读顺序（0..n_up），`cell` 是物理格位；
/// 单面时两者恒等，双面时 `cell` 依翻页轴镜像。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub sheet: u16,
    pub side: u8,
    pub slot: u8,
    pub cell: u8,
    pub page: u16,
}

/// 拼版计划：页序单源 + 封签状态 + 内容摘要。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImpositionPlan {
    pub n_up: NUp,
    pub duplex: Duplex,
    pub page_count: u16,
    pub sheets: u16,
    pub sealed: bool,
    pub digest: u64,
    pub placements: Vec<Placement>,
}

/// 作业描述（唯一输入源）。三元组之外的任何信息都不参与页序推导。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutJob {
    pub pages: u16,
    pub n_up: u8,
    pub duplex: u8,
}

impl LayoutJob {
    pub fn new(pages: u16, n_up: u8, duplex: u8) -> LayoutJob {
        LayoutJob { pages, n_up, duplex }
    }

    /// 单源推导：由 (页数, N-up, 双面) 生成完整落位表。
    pub fn plan(&self) -> Result<ImpositionPlan, LayoutError> {
        let nup = NUp::from_code(self.n_up)?;
        let dpx = Duplex::from_code(self.duplex)?;
        let (sheets, placements) = build_placements(nup, dpx, self.pages)?;
        let mut plan = ImpositionPlan {
            n_up: nup,
            duplex: dpx,
            page_count: self.pages,
            sheets,
            sealed: false,
            digest: 0,
            placements,
        };
        plan.digest = digest_of(&plan);
        Ok(plan)
    }
}

/// 物理格位换算：正面保持阅读格；反面按翻页轴镜像。
fn back_cell(dpx: Duplex, cols: u8, rows: u8, side: u8, slot: u8) -> u8 {
    if side == 0 {
        return slot;
    }
    let n = cols.saturating_mul(rows);
    if n == 0 {
        return slot;
    }
    match dpx {
        Duplex::Simplex => slot,
        // 长边翻页：整面相对阅读方向反向（左↔右、上↔下同时翻）。
        Duplex::LongEdge => n.saturating_sub(1).saturating_sub(slot),
        // 短边翻页：只沿行轴镜像（列不变），因此与长边在 2 行以上可区分。
        Duplex::ShortEdge => {
            let col = slot % cols;
            let row = slot / cols;
            let flip = rows.saturating_sub(1).saturating_sub(row);
            flip.saturating_mul(cols).saturating_add(col)
        }
    }
}

/// 构造落位表。顺序即阅读顺序：跨张、张内先正后反、面内按 slot 递增。
fn build_placements(
    nup: NUp,
    dpx: Duplex,
    pages: u16,
) -> Result<(u16, Vec<Placement>), LayoutError> {
    if pages == 0 {
        return Err(LayoutError::EmptyJob);
    }
    if pages > MAX_PAGES {
        return Err(LayoutError::PageOverflow);
    }
    let per_sheet = (nup.count() as u32) * (dpx.sides() as u32);
    if per_sheet == 0 {
        return Err(LayoutError::GridOverflow);
    }
    let sheets32 = ((pages as u32) + per_sheet - 1) / per_sheet;
    if sheets32 > MAX_SHEETS as u32 {
        return Err(LayoutError::TooManySheets);
    }
    let total = sheets32 * per_sheet;
    let (cols, rows) = nup.grid();
    let n = nup.count() as u32;
    let mut placements = Vec::with_capacity(total as usize);
    let mut k = 0u32;
    while k < total {
        let sheet = (k / per_sheet) as u16;
        let within = (k % per_sheet) as u8;
        let side = within / nup.count();
        let slot = within % nup.count();
        let page = if k < pages as u32 { (k as u16) + 1 } else { 0 };
        let cell = back_cell(dpx, cols, rows, side, slot);
        placements.push(Placement {
            sheet,
            side,
            slot,
            cell,
            page,
        });
        k += 1;
        // 防御：k 溢出前退出（total ≤ 999*32，u32 安全，此判断只为纪律完整）。
        if k > MAX_SHEETS as u32 * 32 {
            break;
        }
    }
    let _ = n;
    Ok((sheets32 as u16, placements))
}

/// FNV-1a 混入一档：内容摘要的唯一混合原语。
fn mix(h: u64, v: u64) -> u64 {
    let x = h ^ v;
    x.wrapping_mul(0x100_0000_01b3)
}

/// 计划内容摘要。摘要只覆盖内容（档位/页数/张数/落位），不覆盖封签状态，
/// 因此「重新推导同一作业」必然得到同一摘要——联签零改写的判据基础。
pub fn digest_of(p: &ImpositionPlan) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    h = mix(h, p.n_up.code() as u64);
    h = mix(h, p.duplex.code() as u64);
    h = mix(h, p.page_count as u64);
    h = mix(h, p.sheets as u64);
    h = mix(h, p.placements.len() as u64);
    let mut i = 0usize;
    while i < p.placements.len() {
        if let Some(pl) = p.placements.get(i) {
            let packed = ((pl.sheet as u64) << 24)
                | ((pl.side as u64) << 16)
                | ((pl.slot as u64) << 8)
                | pl.cell as u64;
            h = mix(h, packed);
            h = mix(h, pl.page as u64);
        }
        i += 1;
    }
    h
}

/// 定长 bool 表（no_std 下不用 `vec!` 宏，保持与内核侧同一构造路径）。
fn bools(n: usize) -> Vec<bool> {
    let mut v = Vec::with_capacity(n);
    let mut i = 0usize;
    while i < n {
        v.push(false);
        i += 1;
    }
    v
}

/// 计划校验：结构 -> 页号唯一 -> 阅读序 -> 摘要。任一不符显式返回错误。
///
/// 校验顺序刻意把「页号唯一」放在「阅读序」之前：同页重复是独立的坏样本
/// 类别，先判唯一才能让注入核区分「重复」与「错位」。
pub fn verify_plan(p: &ImpositionPlan) -> Result<(), LayoutError> {
    if p.page_count == 0 {
        return Err(LayoutError::EmptyJob);
    }
    if p.page_count > MAX_PAGES {
        return Err(LayoutError::PageOverflow);
    }
    let n = p.n_up.count() as usize;
    let sides = p.duplex.sides() as usize;
    let per_sheet = n * sides;
    let expect_sheets = ((p.page_count as usize) + per_sheet - 1) / per_sheet;
    if expect_sheets != p.sheets as usize {
        return Err(LayoutError::GridOverflow);
    }
    if p.placements.len() != p.sheets as usize * per_sheet {
        return Err(LayoutError::GridOverflow);
    }
    let (cols, rows) = p.n_up.grid();
    let mut occupied = bools(p.sheets as usize * sides * n);
    let mut seen = bools(p.page_count as usize + 1);
    let mut idx = 0usize;
    while idx < p.placements.len() {
        let pl = match p.placements.get(idx) {
            Some(v) => *v,
            None => return Err(LayoutError::GridOverflow),
        };
        let expect_sheet = idx / per_sheet;
        let within = idx % per_sheet;
        if pl.sheet as usize != expect_sheet
            || pl.side as usize != within / n
            || pl.slot as usize != within % n
        {
            return Err(LayoutError::OrderBroken);
        }
        if pl.cell as usize >= n {
            return Err(LayoutError::CellOutOfRange);
        }
        let want_cell = back_cell(p.duplex, cols, rows, pl.side, pl.slot);
        if pl.cell != want_cell {
            return Err(LayoutError::CellOutOfRange);
        }
        let occ = (pl.sheet as usize * sides + pl.side as usize) * n + pl.cell as usize;
        match occupied.get_mut(occ) {
            Some(b) => {
                if *b {
                    return Err(LayoutError::CellOutOfRange);
                }
                *b = true;
            }
            None => return Err(LayoutError::CellOutOfRange),
        }
        if pl.page != 0 {
            match seen.get_mut(pl.page as usize) {
                Some(b) => {
                    if *b {
                        return Err(LayoutError::DuplicatePage);
                    }
                    *b = true;
                }
                None => return Err(LayoutError::UnknownPage),
            }
        }
        let expect_page = if idx < p.page_count as usize {
            (idx as u16) + 1
        } else {
            0
        };
        if pl.page != expect_page {
            return Err(LayoutError::OrderBroken);
        }
        idx += 1;
    }
    let mut i = 1usize;
    while i < seen.len() {
        match seen.get(i) {
            Some(true) => {}
            _ => return Err(LayoutError::UnknownPage),
        }
        i += 1;
    }
    if digest_of(p) != p.digest {
        return Err(LayoutError::SignMismatch);
    }
    Ok(())
}

impl ImpositionPlan {
    /// 有效格容量（张 × 面 × 格）。
    pub fn capacity(&self) -> usize {
        (self.sheets as usize)
            * (self.duplex.sides() as usize)
            * (self.n_up.count() as usize)
    }

    /// 空白格数 = 容量 - 页数；拼版必须把尾部补白，否则正反页码错位。
    pub fn blank_count(&self) -> usize {
        self.capacity().saturating_sub(self.page_count as usize)
    }

    /// 页数重排（未封签时允许）。封签后任何改写都必须被拒绝——这是
    /// 「B24 联签零改写」的执行点：摘要冻结在联签记录里，计划本体也不许动。
    pub fn repage(&mut self, pages: u16) -> Result<(), LayoutError> {
        if self.sealed {
            return Err(LayoutError::SealedPlan);
        }
        let (sheets, placements) = build_placements(self.n_up, self.duplex, pages)?;
        self.page_count = pages;
        self.sheets = sheets;
        self.placements = placements;
        self.digest = digest_of(self);
        verify_plan(self)
    }

    /// B24 联签：先校验再封签，返回不可改写的联签记录。
    pub fn seal(&mut self, signer: u32) -> Result<JointSign, LayoutError> {
        verify_plan(self)?;
        self.sealed = true;
        Ok(JointSign {
            plan_digest: self.digest,
            signer,
            seal: mix(self.digest, (signer as u64) ^ B24_SALT),
        })
    }

    /// 联签复核：摘要与封签双项一致才算通过（任一被改写都返回 SignMismatch）。
    pub fn verify_sign(&self, s: &JointSign) -> Result<(), LayoutError> {
        if s.plan_digest != self.digest {
            return Err(LayoutError::SignMismatch);
        }
        let want = mix(self.digest, (s.signer as u64) ^ B24_SALT);
        if want != s.seal {
            return Err(LayoutError::SignMismatch);
        }
        Ok(())
    }
}

/// B24 联签记录（摘要 + 签署人 + 盐封签三元组）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JointSign {
    pub plan_digest: u64,
    pub signer: u32,
    pub seal: u64,
}

// ---------------------------------------------------------------------------
// 注入核：六类坏样本必须全部被抓
// ---------------------------------------------------------------------------

/// 构造 6 类坏样本并断言拒绝类型。返回 (抓到, 总数)。
pub fn injection_core() -> (usize, usize) {
    let total = 6usize;
    let mut caught = 0usize;

    // 1. 空活：0 页不是「补白 0 张」的合法作业。
    if LayoutJob::new(0, 4, 1).plan().err() == Some(LayoutError::EmptyJob) {
        caught += 1;
    }
    // 2. 非法 N-up：7 不在七档内，必须拒绝而非就近取 8。
    if LayoutJob::new(8, 7, 0).plan().err() == Some(LayoutError::BadNUp) {
        caught += 1;
    }
    // 3. 超页数：4001 > MAX_PAGES。
    if LayoutJob::new(MAX_PAGES + 1, 4, 0).plan().err() == Some(LayoutError::PageOverflow) {
        caught += 1;
    }
    // 4. 页号重复：把第 2 格改成第 1 格同页，唯一性检查必须先抓。
    match LayoutJob::new(9, 4, 0).plan() {
        Ok(mut p) => {
            if let Some(pl) = p.placements.get_mut(1) {
                pl.page = 1;
            }
            p.digest = digest_of(&p);
            if verify_plan(&p).err() == Some(LayoutError::DuplicatePage) {
                caught += 1;
            }
        }
        Err(_) => {}
    }
    // 5. 封签后改写：repage 必须返回 SealedPlan。
    match LayoutJob::new(8, 4, 1).plan() {
        Ok(mut p) => match p.seal(24) {
            Ok(_) => {
                if p.repage(12).err() == Some(LayoutError::SealedPlan) {
                    caught += 1;
                }
            }
            Err(_) => {}
        },
        Err(_) => {}
    }
    // 6. 摘要翻转：任何字节级改写都要被 SignMismatch 抓出。
    match LayoutJob::new(8, 4, 2).plan() {
        Ok(mut p) => {
            p.digest ^= 0x1;
            if verify_plan(&p).err() == Some(LayoutError::SignMismatch) {
                caught += 1;
            }
        }
        Err(_) => {}
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 20 组 (页数, N-up, 双面) 组合：档位 × 面数 × 页数三轴交叠，覆盖补白、
/// 跨张与镜像全路径。
fn combo_jobs() -> Vec<LayoutJob> {
    let nup = [1u8, 2, 4, 6, 8, 9, 16];
    let dpx = [0u8, 1, 2];
    let pages = [1u16, 7, 20, 33, 64];
    let mut out = Vec::with_capacity(20);
    let mut i = 0usize;
    while out.len() < 20 {
        let n = nup[i % nup.len()];
        let d = dpx[(i / 7) % dpx.len()];
        let p = pages[i % pages.len()];
        out.push(LayoutJob::new(p, n, d));
        i += 1;
    }
    out
}

/// J1 主判据的逐项核算器：返回 (坏组合数, 总页数, 总空白数)。
pub fn audit_combos() -> (usize, usize, usize) {
    let jobs = combo_jobs();
    let mut bad = 0usize;
    let mut pages = 0usize;
    let mut blanks = 0usize;
    let mut i = 0usize;
    while i < jobs.len() {
        let job = match jobs.get(i) {
            Some(j) => *j,
            None => {
                bad += 1;
                i += 1;
                continue;
            }
        };
        let p = match job.plan() {
            Ok(v) => v,
            Err(_) => {
                bad += 1;
                i += 1;
                continue;
            }
        };
        if verify_plan(&p).is_err() {
            bad += 1;
            i += 1;
            continue;
        }
        // 页序单源：非空白页必须严格 1,2,3,...；空白只允许出现在尾部。
        let mut expect = 1u16;
        let mut tail_blank = false;
        let mut k = 0usize;
        while k < p.placements.len() {
            match p.placements.get(k) {
                Some(pl) => {
                    if pl.page == 0 {
                        tail_blank = true;
                    } else if tail_blank || pl.page != expect {
                        bad += 1;
                        break;
                    } else {
                        expect += 1;
                    }
                }
                None => {
                    bad += 1;
                    break;
                }
            }
            k += 1;
        }
        if expect != job.pages + 1 {
            bad += 1;
        }
        pages += job.pages as usize;
        blanks += p.blank_count();
        // 确定性：同一作业重算，摘要与落位逐位一致。
        match job.plan() {
            Ok(q) => {
                if q.digest != p.digest || q.placements != p.placements {
                    bad += 1;
                }
            }
            Err(_) => bad += 1,
        }
        i += 1;
    }
    (bad, pages, blanks)
}

pub fn run_unx50027_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50027");

    // J1-1 组合 ×20 页序一致（含确定性与补白守恒）。
    let (bad, pages, blanks) = audit_combos();
    let ok = bad == 0 && pages > 0 && blanks > 0;
    let det = format!("bad={} pages={} blanks={}", bad, pages, blanks);
    s.add_dyn("50027-J1-组合20页序", ok, &det);

    // J1-2 页号全覆盖：每个源页在落位表中恰好出现一次。
    let (dup, missing) = match LayoutJob::new(37, 6, 1).plan() {
        Ok(plan) => {
            let mut counts = bools(38);
            let mut d = 0usize;
            let mut i = 0usize;
            while i < plan.placements.len() {
                if let Some(pl) = plan.placements.get(i) {
                    if pl.page != 0 {
                        match counts.get_mut(pl.page as usize) {
                            Some(b) => {
                                if *b {
                                    d += 1;
                                }
                                *b = true;
                            }
                            None => d += 1,
                        }
                    }
                }
                i += 1;
            }
            let mut m = 0usize;
            let mut k = 1usize;
            while k < counts.len() {
                if counts.get(k) == Some(&false) {
                    m += 1;
                }
                k += 1;
            }
            (d, m)
        }
        Err(_) => (1usize, 1usize),
    };
    let ok = dup == 0 && missing == 0;
    let det = format!("dup={} missing={}", dup, missing);
    s.add_dyn("50027-J1-页号全覆盖", ok, &det);

    // J1-3 空白补白：容量 - 页数，且空白只在尾部。
    let p7 = LayoutJob::new(7, 4, 1).plan();
    let ok = match &p7 {
        Ok(p) => p.blank_count() == 1 && p.capacity() == 8 && verify_plan(p).is_ok(),
        Err(_) => false,
    };
    s.add("50027-J1-补白守恒", ok, if ok { "" } else { "空白计数漂移" });

    // J1-4 落位数 = 张 × 面 × 格（六档抽查）。
    let mut len_bad = 0usize;
    for job in [
        LayoutJob::new(1, 1, 0),
        LayoutJob::new(8, 4, 1),
        LayoutJob::new(33, 6, 2),
        LayoutJob::new(64, 8, 1),
        LayoutJob::new(20, 9, 0),
        LayoutJob::new(100, 16, 2),
    ]
    .iter()
    {
        match job.plan() {
            Ok(p) => {
                if p.placements.len() != p.capacity() {
                    len_bad += 1;
                }
            }
            Err(_) => len_bad += 1,
        }
    }
    let ok = len_bad == 0;
    s.add("50027-J1-落位数守恒", ok, if ok { "" } else { "落位数与容量不符" });

    // J1-5 长边翻页：反面格序整体反向（4-up 2×2）。
    let mut long_ok = false;
    if let Ok(p) = LayoutJob::new(16, 4, 1).plan() {
        let mut back = [255u8; 4];
        let mut n = 0usize;
        let mut i = 0usize;
        while i < p.placements.len() {
            if let Some(pl) = p.placements.get(i) {
                if pl.side == 1 && n < 4 {
                    back[n] = pl.cell;
                    n += 1;
                }
            }
            i += 1;
        }
        long_ok = n == 4 && back == [3, 2, 1, 0];
    }
    s.add("50027-J1-长边镜像", long_ok, if long_ok { "" } else { "长边反面格序非反向" });

    // J1-6 短边翻页：只沿行轴镜像（slot 0/1 落到行 1，2/3 落到行 0）。
    let mut short_ok = false;
    if let Ok(p) = LayoutJob::new(16, 4, 2).plan() {
        let mut back = [255u8; 4];
        let mut n = 0usize;
        let mut i = 0usize;
        while i < p.placements.len() {
            if let Some(pl) = p.placements.get(i) {
                if pl.side == 1 && n < 4 {
                    back[n] = pl.cell;
                    n += 1;
                }
            }
            i += 1;
        }
        short_ok = n == 4 && back == [2, 3, 0, 1];
    }
    s.add("50027-J1-短边行镜", short_ok, if short_ok { "" } else { "短边反面行镜错误" });

    // J1-7 单面与 1-up：落位顺序即页序（恒等基线）。
    let mut ident_ok = false;
    if let Ok(p) = LayoutJob::new(5, 1, 0).plan() {
        ident_ok = p.placements.len() == 5
            && p.placements.iter().enumerate().all(|(i, pl)| {
                pl.page as usize == i + 1 && pl.slot == 0 && pl.cell == 0 && pl.side == 0
            });
    }
    s.add("50027-J1-1up恒等", ident_ok, if ident_ok { "" } else { "1-up 未保持恒等序" });

    // J1-8 B24 联签零改写：同作业两次封签，摘要与封签逐位相同。
    let mut sign_ok = false;
    let mut sign_det = String::new();
    if let Ok(mut a) = LayoutJob::new(21, 4, 1).plan() {
        if let Ok(sa) = a.seal(24) {
            if let Ok(mut b) = LayoutJob::new(21, 4, 1).plan() {
                if let Ok(sb) = b.seal(24) {
                    sign_ok = sa == sb && a.verify_sign(&sa).is_ok() && b.verify_sign(&sb).is_ok();
                    sign_det = format!("seal={:016x}", sa.seal);
                }
            }
        }
    }
    s.add_dyn("50027-J1-联签零改写", sign_ok, &sign_det);

    // J1-9 封签后 repage 必拒，且码值 = 段基址 + 8。
    let mut sealed_ok = false;
    if let Ok(mut p) = LayoutJob::new(8, 4, 1).plan() {
        if p.seal(1).is_ok() {
            sealed_ok = p.repage(9).err() == Some(LayoutError::SealedPlan)
                && LayoutError::SealedPlan.code() == SEG_BASE + 8;
        }
    }
    s.add("50027-J1-封签拒改写", sealed_ok, if sealed_ok { "" } else { "封签后仍可改写" });

    // J1-10 未封签重排：页数与摘要同步更新，校验仍通过。
    let mut repage_ok = false;
    if let Ok(mut p) = LayoutJob::new(8, 4, 1).plan() {
        let before = p.digest;
        if p.repage(15).is_ok() {
            repage_ok = p.page_count == 15
                && p.digest != before
                && verify_plan(&p).is_ok()
                && p.sheets == 2;
        }
    }
    s.add("50027-J1-未封重排", repage_ok, if repage_ok { "" } else { "未封重排不自洽" });

    // J1-11 空活 / 非法 N-up / 非法双面 三档显式拒绝。
    let ok = LayoutJob::new(0, 4, 0).plan().err() == Some(LayoutError::EmptyJob)
        && LayoutJob::new(4, 7, 0).plan().err() == Some(LayoutError::BadNUp)
        && LayoutJob::new(4, 4, 9).plan().err() == Some(LayoutError::BadDuplex);
    s.add("50027-J1-输入显式拒绝", ok, if ok { "" } else { "坏输入未拒绝" });

    // J1-12 上限两档：超页数与超张数分别命中的不同码。
    let ok = LayoutJob::new(MAX_PAGES + 1, 4, 0).plan().err()
        == Some(LayoutError::PageOverflow)
        && LayoutJob::new(4000, 1, 0).plan().err() == Some(LayoutError::TooManySheets);
    s.add("50027-J1-上限分码", ok, if ok { "" } else { "上限错误码混淆" });

    // J1-13 错误码闭集：12 条，码值 = SEG_BASE + (1..=12) 且两两不同。
    let mut codes_ok = ALL_ERRORS.len() == 12;
    let mut seen_codes = bools(13);
    let mut i = 0usize;
    while i < ALL_ERRORS.len() {
        if let Some(e) = ALL_ERRORS.get(i) {
            let c = e.code();
            if c != SEG_BASE + (i as u32) + 1 {
                codes_ok = false;
            }
            let off = (c - SEG_BASE) as usize;
            match seen_codes.get_mut(off) {
                Some(b) => {
                    if *b {
                        codes_ok = false;
                    }
                    *b = true;
                }
                None => codes_ok = false,
            }
        }
        i += 1;
    }
    s.add("50027-J1-错误码闭集", codes_ok, if codes_ok { "" } else { "码值漂移" });

    // J1-14 校验器抓三类人工破坏：页号重复 / 格位错配 / 摘要翻转。
    let mut caught = 0usize;
    if let Ok(mut p) = LayoutJob::new(12, 4, 1).plan() {
        if let Some(pl) = p.placements.get_mut(1) {
            pl.page = 1;
        }
        p.digest = digest_of(&p);
        if verify_plan(&p).err() == Some(LayoutError::DuplicatePage) {
            caught += 1;
        }
    }
    if let Ok(mut p) = LayoutJob::new(12, 4, 1).plan() {
        if let Some(pl) = p.placements.get_mut(2) {
            pl.cell = 0;
        }
        p.digest = digest_of(&p);
        if verify_plan(&p).err() == Some(LayoutError::CellOutOfRange) {
            caught += 1;
        }
    }
    if let Ok(mut p) = LayoutJob::new(12, 4, 1).plan() {
        p.digest ^= 0x80;
        if verify_plan(&p).err() == Some(LayoutError::SignMismatch) {
            caught += 1;
        }
    }
    let ok = caught == 3;
    let det = format!("{}/3", caught);
    s.add_dyn("50027-J1-校验抓破坏", ok, &det);

    // J1-15 满容量边界：3968 页 16-up 双面 = 124 张恰好满载，无补白。
    let mut full_ok = false;
    if let Ok(p) = LayoutJob::new(3968, 16, 1).plan() {
        full_ok = p.sheets == 124 && p.blank_count() == 0 && verify_plan(&p).is_ok();
    }
    s.add("50027-J1-满载边界", full_ok, if full_ok { "" } else { "满载张数/补白错误" });

    // J1-16 摘要敏感性：页数、档位、面数任一变化都要改摘要。
    let mut sens_ok = false;
    if let (Ok(a), Ok(b), Ok(c), Ok(d)) = (
        LayoutJob::new(4, 4, 0).plan(),
        LayoutJob::new(5, 4, 0).plan(),
        LayoutJob::new(4, 6, 0).plan(),
        LayoutJob::new(4, 4, 1).plan(),
    ) {
        let uniq = a.digest != b.digest
            && a.digest != c.digest
            && a.digest != d.digest
            && b.digest != c.digest;
        sens_ok = uniq;
    }
    s.add("50027-J1-摘要敏感", sens_ok, if sens_ok { "" } else { "摘要对输入不敏感" });

    // J1-17 注入核 6 类全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50027-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50027_checks_green() {
        let set = run_unx50027_checks();
        assert!(set.all_passed(), "UNX-F50027 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50027_plan_is_deterministic() {
        let a = LayoutJob::new(31, 9, 2).plan();
        let b = LayoutJob::new(31, 9, 2).plan();
        match (a, b) {
            (Ok(x), Ok(y)) => {
                assert_eq!(x.digest, y.digest);
                assert_eq!(x.placements, y.placements);
            }
            _ => panic!("plan failed"),
        }
    }
}
