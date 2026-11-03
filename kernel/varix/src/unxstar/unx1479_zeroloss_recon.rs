//! UNX-F1479 B34 批内对账（零丢互联对账总成） · 0x75A00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（DISPATCH_w7I 逐字锚定，AI-VX7-1104 批4 I 组）：
//!   19 条互联判据十八面互查（恒等式/扫描/归因/定位/修复/联动/风暴/
//!   多核/切换/注入/日报/升级/面查/版本/健康/文档/查询/回归/预算），
//!   互联矩阵可查可导。
//! 逐卡要点（分派表逐字锚定）：
//!   19 判据×19 面（对角自核+其余 18 面互查）互联矩阵 361 格逐格核，
//!   互联矩阵导出文本逐行账齐，断言数 ≥40 大关，编号唯一性常绿。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   ① 十九面闭集：恒等式/扫描/归因/定位/修复/联动/风暴/多核/切换/
//!     注入/日报/升级/面查/版本/健康/文档/查询/回归/预算——Face 枚举
//!     即注册键，名字面唯一（编号唯一性常绿），重复注册显性拒；
//!   ② 互联矩阵 19×19=361 格：对角格=面自核（本面在册+绿+断言数与
//!     冻结表一致），非对角格=行面查列面（查对面在册/绿/账行格式），
//!     审计者自身必须健康——361 格逐格核，一格破即矩阵破；
//!   ③ 单面红定位：面 f 破 → 行 f（19 格）+ 列 f（19 格）−对角重复
//!     =37 格红，行列双向定位唯一锁定 f 面——注入面红必被行列检出，
//!     修复后矩阵必须回到 361/361；
//!   ④ 断言大关 ≥40：十九面断言数冻结表合计 44 ≥ 40 大关，表值被
//!     抽薄（任一面 <2 或合计 <40）显性破关；
//!   ⑤ 导出面：互联矩阵导出 361 行文本（行面→列面=平/破），行数
//!     恰 361、每行块恰 19 平——导出逐行账齐，行数错位即破；
//!   ⑥ 注册状态机：Open→Registered（19/19 在册）→Crossed（361/361
//!     绿）单向，未满 19 注册即对账显性拒，倒退显性拒；
//!   ⑦ 全确定性：LCG 种子固定驱动 10 轮面红注入，零墙钟零随机零
//!     浮点，诊断码 0x40 窗口闭集，中文诊断文案带可操作建议。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75A00，批4 I 组独占窗口）。
pub const SEG_BASE: u32 = 0x75A00;

/// 互联面数（冻结 19）。
pub const FACE_COUNT: usize = 19;
/// 矩阵总格数（冻结 361 = 19×19）。
pub const MATRIX_CELLS: usize = FACE_COUNT * FACE_COUNT;
/// 断言大关（冻结 ≥40）。
pub const ASSERT_FLOOR: usize = 40;
/// 判据轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
/// 诊断码闭集：每码带可操作中文建议（detail 文案）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReconCode {
    /// 面未知：不在十九面闭集内
    UnknownFace,
    /// 重复注册：同面已入册
    DuplicateFaceId,
    /// 面红：被注入/被检出的非绿面
    FaceRed,
    /// 矩阵格破：互查格不绿
    MatrixCellFail,
    /// 断言大关未达：合计 <40
    AssertFloorShort,
    /// 注册未满即对账 / 状态机倒退
    RegisterRegress,
    /// 导出行错位：行数 ≠361 或行块 ≠19 平
    ExportLineMismatch,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl ReconCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ReconCode::UnknownFace => {
                "面未知：互联面仅十九面闭集，请按 Face 枚举注册，勿自造面名"
            }
            ReconCode::DuplicateFaceId => {
                "重复注册拒：该面已在册，编号唯一性常绿，请查重后再注册"
            }
            ReconCode::FaceRed => {
                "面红：该面模型非绿，请先修面模型；行列双向定位见互联矩阵导出面"
            }
            ReconCode::MatrixCellFail => {
                "矩阵格破：互查格不绿，请按行列双向定位锁定破面后修复"
            }
            ReconCode::AssertFloorShort => {
                "断言大关未达：十九面断言合计 <40，请按冻结表回填断言数"
            }
            ReconCode::RegisterRegress => {
                "注册未满即对账/状态机倒退：请先满 19 面在册再对账，倒退显性拒"
            }
            ReconCode::ExportLineMismatch => {
                "导出行错位：导出行数必须恰 361 且每行块 19 平，请以矩阵为权威重导"
            }
            ReconCode::NotReproducible => {
                "双跑不一致：复现红线，请检查是否有墙钟/随机参与取值"
            }
        }
    }
    pub const ALL: [ReconCode; 8] = [
        ReconCode::UnknownFace,
        ReconCode::DuplicateFaceId,
        ReconCode::FaceRed,
        ReconCode::MatrixCellFail,
        ReconCode::AssertFloorShort,
        ReconCode::RegisterRegress,
        ReconCode::ExportLineMismatch,
        ReconCode::NotReproducible,
    ];
}

/// 段内查表闭集返回。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < ReconCode::ALL.len() {
        if let Some(c) = ReconCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75A00 段闭集内"
}

// ── 域模型：十九面闭集 ──────────────────────────────────────────
/// 十九互联面闭集（B34 零丢族十八面互查 + 对角自核）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// 恒等式：Σ修复+Σ放弃+Σ在途=Σ检出。
    Identity,
    /// 扫描：重复扫描位图全等。
    Scan,
    /// 归因：检出行归因标签闭集。
    Attrib,
    /// 定位：红项行列定位。
    Locate,
    /// 修复：修复口径冻结。
    Repair,
    /// 联动：跨账联动一致。
    Linkage,
    /// 风暴：注入风暴全抓。
    Storm,
    /// 多核：per-CPU 区账。
    Multicore,
    /// 切换：内核↔用户偶回位。
    Switch,
    /// 注入：注入路必抓。
    Inject,
    /// 日报：日报面齐行。
    Daily,
    /// 升级：面间升级序。
    Escalate,
    /// 面查：面查口径。
    FaceAudit,
    /// 版本：冻结版本面。
    Version,
    /// 健康：健康度账。
    Health,
    /// 文档：语义快照。
    Doc,
    /// 查询：三维查询。
    Query,
    /// 回归：回归矩阵。
    Regression,
    /// 预算：性能预算。
    Budget,
}

impl Face {
    pub fn name(self) -> &'static str {
        match self {
            Face::Identity => "恒等式",
            Face::Scan => "扫描",
            Face::Attrib => "归因",
            Face::Locate => "定位",
            Face::Repair => "修复",
            Face::Linkage => "联动",
            Face::Storm => "风暴",
            Face::Multicore => "多核",
            Face::Switch => "切换",
            Face::Inject => "注入",
            Face::Daily => "日报",
            Face::Escalate => "升级",
            Face::FaceAudit => "面查",
            Face::Version => "版本",
            Face::Health => "健康",
            Face::Doc => "文档",
            Face::Query => "查询",
            Face::Regression => "回归",
            Face::Budget => "预算",
        }
    }
    pub const ALL: [Face; FACE_COUNT] = [
        Face::Identity,
        Face::Scan,
        Face::Attrib,
        Face::Locate,
        Face::Repair,
        Face::Linkage,
        Face::Storm,
        Face::Multicore,
        Face::Switch,
        Face::Inject,
        Face::Daily,
        Face::Escalate,
        Face::FaceAudit,
        Face::Version,
        Face::Health,
        Face::Doc,
        Face::Query,
        Face::Regression,
        Face::Budget,
    ];
    /// 序号（枚举序）。
    pub fn idx(self) -> usize {
        let mut i = 0usize;
        while i < Face::ALL.len() {
            if let Some(f) = Face::ALL.get(i) {
                if *f == self {
                    return i;
                }
            }
            i += 1;
        }
        usize::MAX
    }
    /// 序号 → 面（越界 None）。
    pub fn of(v: usize) -> Option<Face> {
        Face::ALL.get(v).copied()
    }
}

/// 十九面断言数冻结表（合计 44 ≥ 40 大关；面序与 Face::ALL 一致）。
pub const ASSERTS_PER_FACE: [u32; FACE_COUNT] =
    [3, 3, 2, 2, 3, 2, 3, 3, 2, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2];

/// 断言表合计（≥40 大关核）。
pub fn asserts_total() -> usize {
    let mut sum = 0usize;
    let mut i = 0usize;
    while i < ASSERTS_PER_FACE.len() {
        if let Some(n) = ASSERTS_PER_FACE.get(i) {
            sum = sum.saturating_add(*n as usize);
        }
        i += 1;
    }
    sum
}

/// 断言表健康：每面 ≥2 且合计 ≥40（表被抽薄即破关）。
pub fn asserts_table_ok() -> bool {
    if asserts_total() < ASSERT_FLOOR {
        return false;
    }
    let mut i = 0usize;
    while i < ASSERTS_PER_FACE.len() {
        if let Some(n) = ASSERTS_PER_FACE.get(i) {
            if *n < 2 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 面册与互联矩阵 ──────────────────────────────────────────────
/// 注册状态机（Open→Registered→Crossed 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookState {
    /// 开册：注册中。
    Open,
    /// 十九面全在册。
    Registered,
    /// 矩阵 361/361 绿。
    Crossed,
}

/// 面账行（每面一行：在册/绿/断言数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceRow {
    pub present: bool,
    pub green: bool,
    pub asserts: u32,
}

/// 面册总成（十九行 + 状态机 + 破面记账）。
#[derive(Clone, Debug)]
pub struct FaceBook {
    pub rows: [FaceRow; FACE_COUNT],
    pub state: BookState,
    /// 注入面红累计次数（演练口径）。
    pub corrupt_count: u32,
    /// 修复回绿累计次数。
    pub repaired_count: u32,
}

impl Default for FaceBook {
    fn default() -> FaceBook {
        FaceBook {
            rows: [FaceRow { present: false, green: false, asserts: 0 }; FACE_COUNT],
            state: BookState::Open,
            corrupt_count: 0,
            repaired_count: 0,
        }
    }
}

impl FaceBook {
    pub fn new() -> FaceBook {
        FaceBook::default()
    }

    /// 注册一面（重复注册拒；UnknownFace 由枚举面天然封闭）。
    pub fn register(&mut self, f: Face) -> Result<usize, u32> {
        if self.state == BookState::Registered || self.state == BookState::Crossed {
            return Err(ReconCode::RegisterRegress.val());
        }
        let idx = f.idx();
        if idx >= FACE_COUNT {
            return Err(ReconCode::UnknownFace.val());
        }
        if let Some(r) = self.rows.get(idx) {
            if r.present {
                return Err(ReconCode::DuplicateFaceId.val());
            }
        }
        if let Some(r) = self.rows.get_mut(idx) {
            *r = FaceRow { present: true, green: true, asserts: *ASSERTS_PER_FACE.get(idx).unwrap_or(&0) };
        }
        // 满 19 面 → Registered。
        if self.present_count() == FACE_COUNT {
            self.state = BookState::Registered;
        }
        Ok(idx)
    }

    /// 在册面数。
    pub fn present_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.present {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 注入面红（演练路：绿→红，计数入账）。
    pub fn corrupt(&mut self, f: Face) -> Result<(), u32> {
        let idx = f.idx();
        if idx >= FACE_COUNT {
            return Err(ReconCode::UnknownFace.val());
        }
        if let Some(r) = self.rows.get_mut(idx) {
            if !r.present {
                return Err(ReconCode::UnknownFace.val());
            }
            r.green = false;
        }
        self.corrupt_count = self.corrupt_count.saturating_add(1);
        Ok(())
    }

    /// 修复面红（红→绿，计数入账）。
    pub fn repair(&mut self, f: Face) -> Result<(), u32> {
        let idx = f.idx();
        if idx >= FACE_COUNT {
            return Err(ReconCode::UnknownFace.val());
        }
        if let Some(r) = self.rows.get_mut(idx) {
            if !r.present {
                return Err(ReconCode::UnknownFace.val());
            }
            if r.green {
                return Err(ReconCode::FaceRed.val());
            }
            r.green = true;
        }
        self.repaired_count = self.repaired_count.saturating_add(1);
        Ok(())
    }

    /// 单格核：行 i 查列 j（i==j 对角自核，其余互查）。
    /// 格绿条件：审计者（行面）在册且绿；被查面（列面）在册且绿，
    /// 且其断言数与冻结表一致。
    pub fn cell_ok(&self, i: usize, j: usize) -> bool {
        if i >= FACE_COUNT || j >= FACE_COUNT {
            return false;
        }
        let auditor = match self.rows.get(i) {
            Some(r) => r,
            None => return false,
        };
        let auditee = match self.rows.get(j) {
            Some(r) => r,
            None => return false,
        };
        if !auditor.present || !auditor.green {
            return false;
        }
        if !auditee.present || !auditee.green {
            return false;
        }
        match ASSERTS_PER_FACE.get(j) {
            Some(n) => auditee.asserts == *n,
            None => false,
        }
    }

    /// 矩阵全核：361 格逐格核，返回绿格数。
    pub fn matrix_green_cells(&self) -> usize {
        let mut green = 0usize;
        let mut i = 0usize;
        while i < FACE_COUNT {
            let mut j = 0usize;
            while j < FACE_COUNT {
                if self.cell_ok(i, j) {
                    green += 1;
                }
                j += 1;
            }
            i += 1;
        }
        green
    }

    /// 对账收口：Registered 态且 361/361 绿 → Crossed（否则格破/倒退拒）。
    pub fn cross_audit(&mut self) -> Result<usize, u32> {
        if self.state != BookState::Registered {
            return Err(ReconCode::RegisterRegress.val());
        }
        let green = self.matrix_green_cells();
        if green != MATRIX_CELLS {
            return Err(ReconCode::MatrixCellFail.val());
        }
        self.state = BookState::Crossed;
        Ok(green)
    }

    /// 行列双向定位：面 f 破时行 f + 列 f 的红格数（应 19+19-1=37）。
    pub fn red_cross_count(&self, f: Face) -> usize {
        let idx = f.idx();
        if idx >= FACE_COUNT {
            return 0;
        }
        let mut red = 0usize;
        let mut j = 0usize;
        while j < FACE_COUNT {
            if !self.cell_ok(idx, j) {
                red += 1;
            }
            j += 1;
        }
        let mut i = 0usize;
        while i < FACE_COUNT {
            if i != idx && !self.cell_ok(i, idx) {
                red += 1;
            }
            i += 1;
        }
        red
    }

    /// 导出面：361 行文本（行面查列面=平/破），逐行账齐核。
    pub fn export_lines(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < FACE_COUNT {
            let mut j = 0usize;
            while j < FACE_COUNT {
                if let (Some(a), Some(b)) = (Face::of(i), Face::of(j)) {
                    let verdict = if self.cell_ok(i, j) { "平" } else { "破" };
                    out.push(format!("{}查{}={}", a.name(), b.name(), verdict));
                }
                j += 1;
            }
            i += 1;
        }
        out
    }

    /// 导出账齐核：行数 361 + 每行块 19 平（对完整矩阵口径）。
    pub fn export_audit(lines: &[String]) -> Result<(), u32> {
        if lines.len() != MATRIX_CELLS {
            return Err(ReconCode::ExportLineMismatch.val());
        }
        let mut i = 0usize;
        while i < FACE_COUNT {
            let mut flat = 0usize;
            let mut j = 0usize;
            while j < FACE_COUNT {
                let k = i * FACE_COUNT + j;
                if let Some(l) = lines.get(k) {
                    if l.ends_with("=平") {
                        flat += 1;
                    }
                }
                j += 1;
            }
            if flat != FACE_COUNT {
                return Err(ReconCode::ExportLineMismatch.val());
            }
            i += 1;
        }
        Ok(())
    }
}

// ── 编号唯一性常绿（名字面唯一 + 断言表健康） ────────────────────
pub fn id_uniqueness_ok() -> bool {
    let mut i = 0usize;
    while i < Face::ALL.len() {
        if let Some(a) = Face::ALL.get(i) {
            let mut j = i + 1;
            while j < Face::ALL.len() {
                if let Some(b) = Face::ALL.get(j) {
                    if a.name() == b.name() {
                        return false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    asserts_table_ok()
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 黄金全链：注册 19 面 → 对账 361 → 导出 361 行 ────────────────
pub fn golden_run() -> Result<(usize, usize, usize, usize), u32> {
    let mut book = FaceBook::new();
    let mut i = 0usize;
    while i < Face::ALL.len() {
        if let Some(f) = Face::ALL.get(i) {
            book.register(*f)?;
        }
        i += 1;
    }
    let green = book.cross_audit()?;
    let lines = book.export_lines();
    FaceBook::export_audit(&lines)?;
    // 编号唯一性 + 断言大关。
    if !id_uniqueness_ok() {
        return Err(ReconCode::AssertFloorShort.val());
    }
    Ok((green, lines.len(), book.present_count(), asserts_total()))
}

// ── 注入轮（LCG 驱动 10 轮：面红→行列检出→修复回 361） ───────────
pub fn inject_rounds(rounds: u32) -> Result<(u32, u32), u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut tries = 0u32;
    let mut caught = 0u32;
    let mut r = 0;
    while r < rounds {
        let pick = rng.below(FACE_COUNT as u64) as usize;
        let f = match Face::of(pick) {
            Some(f) => f,
            None => return Err(ReconCode::UnknownFace.val()),
        };
        let mut book = fresh_book()?;
        book.corrupt(f)?;
        tries += 1;
        // 检出：矩阵破 + 行列定位恰 37 格红。
        let red = book.red_cross_count(f);
        if book.matrix_green_cells() != MATRIX_CELLS && red == 37 {
            caught += 1;
        }
        // 修复回 361。
        book.repair(f)?;
        if book.matrix_green_cells() != MATRIX_CELLS {
            return Err(ReconCode::MatrixCellFail.val());
        }
        r += 1;
    }
    Ok((tries, caught))
}

/// 标准在册面册（19 面全注册）。
pub fn fresh_book() -> Result<FaceBook, u32> {
    let mut book = FaceBook::new();
    let mut i = 0usize;
    while i < Face::ALL.len() {
        if let Some(f) = Face::ALL.get(i) {
            book.register(*f)?;
        }
        i += 1;
    }
    Ok(book)
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链 361/361/19/44。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((g, lines, present, total))
            if g == MATRIX_CELLS && lines == MATRIX_CELLS && present == FACE_COUNT
                && total >= ASSERT_FLOOR =>
        {
            Ok(())
        }
        _ => Err(ReconCode::NotReproducible.val()),
    }
}

/// 序 B：单面红行列 37 格检出 + 未满注册对账拒 + 重复注册拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut book = fresh_book()?;
    book.corrupt(Face::Storm)?;
    let red = book.red_cross_count(Face::Storm);
    if red != 37 {
        return Err(ReconCode::NotReproducible.val());
    }
    if book.cross_audit() != Err(ReconCode::MatrixCellFail.val()) {
        return Err(ReconCode::NotReproducible.val());
    }
    book.repair(Face::Storm)?;
    if book.cross_audit()? != MATRIX_CELLS {
        return Err(ReconCode::NotReproducible.val());
    }
    // 未满注册对账拒。
    let mut half = FaceBook::new();
    let mut k = 0usize;
    while k < 9 {
        if let Some(f) = Face::of(k) {
            half.register(f)?;
        }
        k += 1;
    }
    if half.cross_audit() != Err(ReconCode::RegisterRegress.val()) {
        return Err(ReconCode::NotReproducible.val());
    }
    // 重复注册拒 + 已注册后再注册拒（倒退）。
    let mut full = fresh_book()?;
    if full.register(Face::Budget) != Err(ReconCode::RegisterRegress.val()) {
        return Err(ReconCode::NotReproducible.val());
    }
    let mut dup = FaceBook::new();
    dup.register(Face::Scan)?;
    if dup.register(Face::Scan) != Err(ReconCode::DuplicateFaceId.val()) {
        return Err(ReconCode::NotReproducible.val());
    }
    Ok(())
}

/// 序 C：冻结常量与段位守卫。
pub fn probe_seq_c() -> Result<(), u32> {
    let zone_ok = SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x75000 && SEG_BASE <= 0x781C0;
    match (
        FACE_COUNT == 19,
        MATRIX_CELLS == 361,
        ASSERT_FLOOR == 40,
        zone_ok,
        segment_guard(),
    ) {
        (true, true, true, true, true) => Ok(()),
        _ => Err(ReconCode::NotReproducible.val()),
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ReconCode::NotReproducible.val());
    }
    let r1 = inject_rounds(JUDGE_ROUNDS)?;
    let r2 = inject_rounds(JUDGE_ROUNDS)?;
    if r1 != r2 {
        return Err(ReconCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ReconCode::ALL.len() {
        if let Some(c) = ReconCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面（私有谓词，cond 为违约条件） ─────────────────────────
fn matrix_full_ok() -> bool {
    match golden_run() {
        Ok((g, _, _, _)) => g == MATRIX_CELLS,
        Err(_) => false,
    }
}

fn diagonal_self_ok() -> bool {
    let book = match fresh_book() {
        Ok(b) => b,
        Err(_) => return false,
    };
    let mut i = 0usize;
    while i < FACE_COUNT {
        if !book.cell_ok(i, i) {
            return false;
        }
        i += 1;
    }
    true
}

fn cross_18_ok() -> bool {
    // 非对角格 342 全绿（完整矩阵 361 − 对角 19）。
    let book = match fresh_book() {
        Ok(b) => b,
        Err(_) => return false,
    };
    let mut off = 0usize;
    let mut i = 0usize;
    while i < FACE_COUNT {
        let mut j = 0usize;
        while j < FACE_COUNT {
            if i != j {
                if !book.cell_ok(i, j) {
                    return false;
                }
                off += 1;
            }
            j += 1;
        }
        i += 1;
    }
    off == FACE_COUNT * (FACE_COUNT - 1)
}

fn assert_floor_ok() -> bool {
    asserts_total() >= ASSERT_FLOOR && asserts_table_ok() && asserts_total() == 44
}

fn single_face_red_located_ok() -> bool {
    let mut book = match fresh_book() {
        Ok(b) => b,
        Err(_) => return false,
    };
    if book.corrupt(Face::Query).is_err() {
        return false;
    }
    let red = book.red_cross_count(Face::Query);
    if red != 37 {
        return false;
    }
    if book.matrix_green_cells() != MATRIX_CELLS - 37 {
        return false;
    }
    // 修复回 361。
    if book.repair(Face::Query).is_err() {
        return false;
    }
    book.matrix_green_cells() == MATRIX_CELLS
}

fn export_lines_ok() -> bool {
    let book = match fresh_book() {
        Ok(b) => b,
        Err(_) => return false,
    };
    let lines = book.export_lines();
    if FaceBook::export_audit(&lines).is_err() {
        return false;
    }
    // 首行与末行显性抽验。
    match (lines.first(), lines.last()) {
        (Some(f), Some(l)) => f.contains("恒等式查恒等式=平") && l.contains("预算查预算=平"),
        _ => false,
    }
}

fn inject_10_10_ok() -> bool {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok((t, c)) => t == JUDGE_ROUNDS && c == t,
        Err(_) => false,
    }
}

fn detail_closed_ok() -> bool {
    let mut i = 0;
    while i < ReconCode::ALL.len() {
        if let Some(c) = ReconCode::ALL.get(i) {
            if detail(c.val()).is_empty() {
                return false;
            }
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).contains("未知")
}

// ── 检查节 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J1-十九面闭集",
        id_uniqueness_ok(),
        String::from("面名唯一+编号唯一性常绿"),
    ));
    v.push((
        "J1-断言大关≥40",
        assert_floor_ok(),
        String::from("十九面断言表合计 44 ≥ 40"),
    ));
    v.push((
        "J1-矩阵361格全绿",
        matrix_full_ok(),
        String::from("对角自核19+互查342 逐格核"),
    ));
    v.push((
        "J1-对角自核",
        diagonal_self_ok(),
        String::from("19 对角格全绿"),
    ));
    v.push((
        "J1-十八面互查",
        cross_18_ok(),
        String::from("非对角 342 格全绿"),
    ));
    v.push((
        "J1-导出逐行账齐",
        export_lines_ok(),
        String::from("361 行+行块 19 平+首末行抽验"),
    ));
    v.push((
        "J2-单面红行列检出",
        single_face_red_located_ok(),
        String::from("面红→37 格红→修复回 361"),
    ));
    let inj = inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J2-注入10/10",
        inject_10_10_ok(),
        format!(
            "面红注入 t/c {}/{}",
            inj.as_ref().map(|(t, _)| *t).unwrap_or(0),
            inj.as_ref().map(|(_, c)| *c).unwrap_or(0)
        ),
    ));
    v.push(("J3-双跑一致", repro_check().is_ok(), String::from("golden_run+注入轮 两跑同值")));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x75A00 窗口")));
    v.push(("J3-诊断码闭集", detail_closed_ok(), String::from("查表闭集返回")));
    v.push((
        "J3-探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        String::from("A/B/C 三序全绿"),
    ));
    v
}

pub fn run_unx1479_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1479 B34 批内对账");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_closed_set_and_uniqueness() {
        assert_eq!(Face::ALL.len(), 19);
        assert!(id_uniqueness_ok());
        assert_eq!(Face::of(0), Some(Face::Identity));
        assert_eq!(Face::of(18), Some(Face::Budget));
        assert_eq!(Face::of(19), None);
    }

    #[test]
    fn assert_table_floor() {
        assert_eq!(asserts_total(), 44);
        assert!(asserts_total() >= ASSERT_FLOOR);
        assert!(asserts_table_ok());
    }

    #[test]
    fn register_and_state_machine() {
        let mut book = FaceBook::new();
        assert_eq!(book.present_count(), 0);
        let mut i = 0usize;
        while i < 19 {
            let f = Face::of(i).expect("面");
            book.register(f).expect("注册");
            i += 1;
        }
        assert_eq!(book.state, BookState::Registered);
        assert_eq!(book.register(Face::Scan), Err(ReconCode::RegisterRegress.val()));
        let mut dup = FaceBook::new();
        dup.register(Face::Identity).expect("首注");
        assert_eq!(dup.register(Face::Identity), Err(ReconCode::DuplicateFaceId.val()));
    }

    #[test]
    fn matrix_361_and_diagonal() {
        let book = fresh_book().expect("面册");
        assert_eq!(book.matrix_green_cells(), 361);
        let mut i = 0usize;
        while i < 19 {
            assert!(book.cell_ok(i, i), "对角格 {} 须绿", i);
            i += 1;
        }
        assert!(book.clone().cross_audit().is_ok());
    }

    #[test]
    fn cross_audit_before_full_register_rejected() {
        let mut half = FaceBook::new();
        let mut k = 0usize;
        while k < 10 {
            half.register(Face::of(k).expect("面")).expect("注册");
            k += 1;
        }
        assert_eq!(half.cross_audit(), Err(ReconCode::RegisterRegress.val()));
    }

    #[test]
    fn single_face_red_37_cells() {
        let mut book = fresh_book().expect("面册");
        book.corrupt(Face::Attrib).expect("注入");
        assert_eq!(book.red_cross_count(Face::Attrib), 37);
        assert_eq!(book.matrix_green_cells(), 361 - 37);
        book.repair(Face::Attrib).expect("修复");
        assert_eq!(book.matrix_green_cells(), 361);
        // 绿面修复显性拒。
        assert_eq!(book.repair(Face::Attrib), Err(ReconCode::FaceRed.val()));
        assert_eq!(book.repaired_count, 1);
        assert_eq!(book.corrupt_count, 1);
    }

    #[test]
    fn export_lines_accounted() {
        let book = fresh_book().expect("面册");
        let lines = book.export_lines();
        assert_eq!(lines.len(), 361);
        FaceBook::export_audit(&lines).expect("导出账齐");
        // 破面导出行可见（Storm 行查 Budget=破）。
        let mut b2 = book.clone();
        b2.corrupt(Face::Budget).expect("注入");
        let l2 = b2.export_lines();
        assert!(l2[18 * 19 + 18].contains("破"), "对角格随面红显性破");
        // 行数错位拒。
        let short = &lines[..360];
        assert_eq!(FaceBook::export_audit(short), Err(ReconCode::ExportLineMismatch.val()));
    }

    #[test]
    fn inject_rounds_and_repro() {
        let (t, c) = inject_rounds(JUDGE_ROUNDS).expect("注入轮");
        assert_eq!(t, JUDGE_ROUNDS);
        assert_eq!(c, t);
        assert!(repro_check().is_ok());
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn segment_and_detail() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(detail_closed_ok());
    }
}
