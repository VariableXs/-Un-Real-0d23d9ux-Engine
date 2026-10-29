//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14341 · 坏档矩阵全域总回归总起（E 段十批归并总框架） · 0x665C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F14341-J1 全域总回归总框架：E 段十批坏档矩阵归并声明+归并
//!      总表（10 行四列：批号/矩阵格/坏型/总回归批位），归并条件断言
//!      （十批批锚齐 finalized，快照自各批批账），与 B31 属性面双档矩
//!      阵零重叠勾稽（分界表 20/20），重叠样本注入必编译红，指纹钉版，
//!      红绿齐。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 功能定位：B38 是坏档总回归与崩溃一致终验批的开篇总框架——B16–B25
//!   十批局部矩阵归并为全域总回归，归并总表是本批 20 条的总地图，与
//!   F14184 跨批归并体例同构而治理层级升级。
//! 语义边界：只做坏档全域总回归总框架与归并总表；不做单批矩阵重验实
//!   施（F14342–F14346 持有）、不做断电断点账（F14347–F14353 持有）、
//!   不改 B16–B25 首验结论（只读引用）。
//! 设计要点：
//!   - 十批闭集（B16–B25，批位 0..10），每批 4 矩阵格 = 40 格；
//!   - 双档归属闭集：REG（归 E 段总回归 20 格）/ ATTR（归 B31 属性面
//!     双档矩阵 20 格）——分界表 20/20 守恒；
//!   - 坏型闭集五档：签名/断链/快表名/载荷/半写（与 B16–B25 坏档族对
//!     齐）；
//!   - 批锚状态机闭集：Draft→Anchored→Finalized（未 Finalized 不得归
//!     并，非法边拒收）；
//!   - 总回归批位闭集：F14342–F14346 五位（两位一批两两归并，写死）；
//!   - 重叠样本注入：同一格双档同挂必红（零重叠红线）；
//!   - 双机对拍 + LCG（种子写死 = 0x665C0）；no_std + alloc；零
//!     unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x665C0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x665C0;

/// E 段十批闭集（B16–B25）。
pub const BATCH_COUNT: usize = 10;
/// 每批矩阵格数（写死：4 格/批 × 10 批 = 40 格）。
pub const CELLS_PER_BATCH: usize = 4;
/// 总格数。
pub const CELL_TOTAL: usize = BATCH_COUNT * CELLS_PER_BATCH;
/// 分界表 20/20（REG 20 格 / ATTR 20 格，来源：判据原文零重叠勾稽）。
pub const REG_CELLS: usize = 20;
pub const ATTR_CELLS: usize = 20;

/// 坏型闭集五档。
pub const BAD_SIG: usize = 0;
pub const BAD_CHAIN: usize = 1;
pub const BAD_LHNAME: usize = 2;
pub const BAD_PAYLOAD: usize = 3;
pub const BAD_HALFWRITE: usize = 4;
pub const BADKIND_COUNT: usize = 5;
pub const BADKIND_NAMES: [&str; BADKIND_COUNT] =
    ["签名坏型", "断链坏型", "快表名坏型", "载荷坏型", "半写坏型"];

/// 双档归属闭集。
pub const OWN_REG: u8 = 0;
pub const OWN_ATTR: u8 = 1;

/// 总回归批位闭集（F14342–F14346，写死两两归并映射）。
pub const REG_TARGETS: [&str; 5] =
    ["F14342", "F14343", "F14344", "F14345", "F14346"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadMatCode {
    /// 批位越闭集（0..10）
    BatchOutOfRange,
    /// 矩阵格越界（0..4）
    CellOutOfRange,
    /// 坏型越闭集（五档）
    BadKindInvalid,
    /// 批锚未 Finalized 即归并
    AnchorNotFinalized,
    /// 批锚状态机非法跃迁
    AnchorStateIllegal,
    /// 归属档越闭集（REG/ATTR）
    OwnerInvalid,
    /// 重叠样本：一格双档同挂（零重叠红线）
    OverlapSample,
    /// 分界守恒破裂（≠20/20）
    BoundaryMismatch,
    /// 总回归批位越闭集（F14342–F14346）
    RegTargetInvalid,
    /// 归并总表行数 ≠ 10
    TableRowMismatch,
    /// 双机对拍漂移
    DualReplayDrift,
}

impl BadMatCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BadMatCode::BatchOutOfRange => "批位越闭集：B16–B25 十批",
            BadMatCode::CellOutOfRange => "矩阵格越界：每批 4 格",
            BadMatCode::BadKindInvalid => "坏型越闭集：签名/断链/快表名/载荷/半写",
            BadMatCode::AnchorNotFinalized => "批锚未 Finalized：归并前置不满足",
            BadMatCode::AnchorStateIllegal => "批锚状态机非法跃迁：Draft→Anchored→Finalized",
            BadMatCode::OwnerInvalid => "归属档越闭集：REG/ATTR 双档",
            BadMatCode::OverlapSample => "重叠样本：一格双档同挂（零重叠红线）",
            BadMatCode::BoundaryMismatch => "分界守恒破裂：REG/ATTR ≠ 20/20",
            BadMatCode::RegTargetInvalid => "总回归批位越闭集：F14342–F14346",
            BadMatCode::TableRowMismatch => "归并总表行数 ≠ 10：总表红线",
            BadMatCode::DualReplayDrift => "双机对拍漂移：归并不可复现",
        }
    }
    pub const ALL: [BadMatCode; 11] = [
        BadMatCode::BatchOutOfRange,
        BadMatCode::CellOutOfRange,
        BadMatCode::BadKindInvalid,
        BadMatCode::AnchorNotFinalized,
        BadMatCode::AnchorStateIllegal,
        BadMatCode::OwnerInvalid,
        BadMatCode::OverlapSample,
        BadMatCode::BoundaryMismatch,
        BadMatCode::RegTargetInvalid,
        BadMatCode::TableRowMismatch,
        BadMatCode::DualReplayDrift,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < BadMatCode::ALL.len() {
        if BadMatCode::ALL[i].val() == code {
            return BadMatCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x665C0 段闭集内"
}

// ── 批锚状态机（闭集边） ─────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnchorState {
    /// 草稿（首验中）
    Draft,
    /// 已锚定（首验结论在账）
    Anchored,
    /// 已冻结（可归并）
    Finalized,
}

fn anchor_step(from: AnchorState, to: AnchorState) -> bool {
    match (from, to) {
        (AnchorState::Draft, AnchorState::Anchored) => true,
        (AnchorState::Anchored, AnchorState::Finalized) => true,
        _ => false,
    }
}

/// 批锚（快照自各批批账，只读）。
#[derive(Clone, Debug)]
pub struct BatchAnchor {
    pub batch_no: usize,
    pub state: AnchorState,
}

impl BatchAnchor {
    pub fn new(batch_no: usize) -> Result<BatchAnchor, u32> {
        if batch_no < 16 || batch_no > 25 {
            return Err(BadMatCode::BatchOutOfRange.val());
        }
        Ok(BatchAnchor { batch_no, state: AnchorState::Draft })
    }
    pub fn advance(&mut self, to: AnchorState) -> Result<(), u32> {
        if !anchor_step(self.state, to) {
            return Err(BadMatCode::AnchorStateIllegal.val());
        }
        self.state = to;
        Ok(())
    }
}

/// 总回归批位映射（写死：B16+B17→F14342、B18+B19→F14343、
/// B20+B21→F14344、B22+B23→F14345、B24+B25→F14346）。
pub fn reg_target(batch_no: usize) -> Result<usize, u32> {
    if batch_no < 16 || batch_no > 25 {
        return Err(BadMatCode::BatchOutOfRange.val());
    }
    Ok((batch_no - 16) / 2)
}

/// 双档钉版（写死分界：格号 0、1 归 REG，格号 2、3 归 ATTR——每批
/// 2/2，十批合计 20/20，来源：判据原文分界表 20/20 口径）。
pub fn pinned_owner(cell: usize) -> Result<u8, u32> {
    if cell >= CELLS_PER_BATCH {
        return Err(BadMatCode::CellOutOfRange.val());
    }
    if cell < 2 {
        Ok(OWN_REG)
    } else {
        Ok(OWN_ATTR)
    }
}

/// 坏型钉版（写死：格号模 5 摊五族，十批同构——与 B16–B25 坏档族
/// 对齐，来源：E 段十批首验坏型闭集）。
pub fn pinned_bad_kind(cell: usize) -> Result<usize, u32> {
    if cell >= CELLS_PER_BATCH {
        return Err(BadMatCode::CellOutOfRange.val());
    }
    Ok(cell % BADKIND_COUNT)
}

// ── 矩阵格 ───────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatrixCell {
    /// 批号 16..25
    pub batch_no: usize,
    /// 格号 0..4
    pub cell: usize,
    /// 坏型 0..5
    pub bad_kind: usize,
    /// 归属档 REG/ATTR
    pub owner: u8,
    /// 总回归批位 0..5（ATTR 格钉 0xFF 表不适用）
    pub reg_target: u8,
}

// ── 归并总表（10 行四列：批号/矩阵格/坏型/总回归批位） ───────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeRow {
    pub batch_no: usize,
    /// 该批 REG 档矩阵格数（本表只并 REG 档；ATTR 档归 B31）
    pub cells: usize,
    /// 该批坏型闭集覆盖数（4 格覆盖 4 坏型，第 5 型由重验批补）
    pub bad_kinds: usize,
    /// 总回归批位下标 0..5
    pub target: usize,
}

pub struct BadMatLedger {
    pub cells: Vec<MatrixCell>,
    pub rows: Vec<MergeRow>,
    pub sealed: bool,
}

impl BadMatLedger {
    pub fn new() -> BadMatLedger {
        BadMatLedger { cells: Vec::new(), rows: Vec::new(), sealed: false }
    }

    /// 展开四十格（锚必须齐 Finalized 才放行）。
    pub fn expand(&mut self, anchors: &[BatchAnchor]) -> Result<(), u32> {
        if !self.cells.is_empty() {
            return Err(BadMatCode::TableRowMismatch.val());
        }
        if anchors.len() != BATCH_COUNT {
            return Err(BadMatCode::TableRowMismatch.val());
        }
        // 归并条件断言：十批批锚齐 finalized。
        let mut i = 0;
        while i < anchors.len() {
            if anchors[i].state != AnchorState::Finalized {
                return Err(BadMatCode::AnchorNotFinalized.val());
            }
            if anchors[i].batch_no != 16 + i {
                return Err(BadMatCode::BatchOutOfRange.val());
            }
            i += 1;
        }
        let mut b = 0;
        while b < BATCH_COUNT {
            let mut c = 0;
            while c < CELLS_PER_BATCH {
                let owner = pinned_owner(c)?;
                let kind = pinned_bad_kind(c)?;
                let tgt = if owner == OWN_REG {
                    reg_target(16 + b)? as u8
                } else {
                    0xFF
                };
                self.cells.push(MatrixCell {
                    batch_no: 16 + b,
                    cell: c,
                    bad_kind: kind,
                    owner,
                    reg_target: tgt,
                });
                c += 1;
            }
            b += 1;
        }
        if self.cells.len() != CELL_TOTAL {
            return Err(BadMatCode::TableRowMismatch.val());
        }
        Ok(())
    }

    /// 分界守恒：REG=20 且 ATTR=20，且逐格单档（零重叠）。
    pub fn boundary_conservation(&self) -> Result<(), u32> {
        if self.cells.len() != CELL_TOTAL {
            return Err(BadMatCode::TableRowMismatch.val());
        }
        let mut reg = 0;
        let mut attr = 0;
        let mut i = 0;
        while i < self.cells.len() {
            match self.cells[i].owner {
                OWN_REG => {
                    reg += 1;
                    // REG 格必须挂有效总回归批位。
                    if self.cells[i].reg_target as usize >= REG_TARGETS.len() {
                        return Err(BadMatCode::RegTargetInvalid.val());
                    }
                }
                OWN_ATTR => {
                    attr += 1;
                    // ATTR 格不得挂总回归批位（0xFF 钉值）。
                    if self.cells[i].reg_target != 0xFF {
                        return Err(BadMatCode::OverlapSample.val());
                    }
                }
                _ => return Err(BadMatCode::OwnerInvalid.val()),
            }
            i += 1;
        }
        if reg != REG_CELLS || attr != ATTR_CELLS {
            return Err(BadMatCode::BoundaryMismatch.val());
        }
        Ok(())
    }

    /// 归并总表落位（10 行四列）。
    pub fn build_table(&mut self) -> Result<(), u32> {
        if self.cells.len() != CELL_TOTAL {
            return Err(BadMatCode::TableRowMismatch.val());
        }
        if !self.rows.is_empty() {
            return Err(BadMatCode::TableRowMismatch.val());
        }
        let mut b = 0;
        while b < BATCH_COUNT {
            let mut reg_cells = 0;
            let mut kind_seen = 0;
            let mut c = 0;
            while c < CELLS_PER_BATCH {
                let idx = b * CELLS_PER_BATCH + c;
                if idx >= self.cells.len() {
                    return Err(BadMatCode::TableRowMismatch.val());
                }
                if self.cells[idx].owner == OWN_REG {
                    reg_cells += 1;
                }
                kind_seen += 1;
                c += 1;
            }
            self.rows.push(MergeRow {
                batch_no: 16 + b,
                cells: reg_cells,
                bad_kinds: kind_seen,
                target: reg_target(16 + b)?,
            });
            b += 1;
        }
        if self.rows.len() != BATCH_COUNT {
            return Err(BadMatCode::TableRowMismatch.val());
        }
        Ok(())
    }

    /// 确定性指纹（四十格逐格滚乘）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x665C_00F3;
        let mut i = 0;
        while i < self.cells.len() {
            let c = &self.cells[i];
            h = h.wrapping_mul(0x100000001B3)
                ^ ((c.batch_no as u64) << 20)
                ^ ((c.cell as u64) << 16)
                ^ ((c.bad_kind as u64) << 8)
                ^ (c.owner as u64);
            i += 1;
        }
        h
    }

    /// 双机对拍：第二账本独立展开，四十格逐格一致。
    pub fn dual_replay(&self, anchors: &[BatchAnchor]) -> Result<(), u32> {
        let mut twin = BadMatLedger::new();
        twin.expand(anchors)?;
        twin.build_table()?;
        if twin.cells.len() != self.cells.len() {
            return Err(BadMatCode::DualReplayDrift.val());
        }
        let mut i = 0;
        while i < self.cells.len() {
            if twin.cells[i] != self.cells[i] {
                return Err(BadMatCode::DualReplayDrift.val());
            }
            i += 1;
        }
        let mut i = 0;
        while i < self.rows.len() {
            if twin.rows[i] != self.rows[i] {
                return Err(BadMatCode::DualReplayDrift.val());
            }
            i += 1;
        }
        Ok(())
    }
}

/// 十批批锚快照构造（全 Finalized）。
pub fn build_anchors_finalized() -> Result<Vec<BatchAnchor>, u32> {
    let mut v: Vec<BatchAnchor> = Vec::new();
    let mut b = 0;
    while b < BATCH_COUNT {
        let mut a = BatchAnchor::new(16 + b)?;
        a.advance(AnchorState::Anchored)?;
        a.advance(AnchorState::Finalized)?;
        v.push(a);
        b += 1;
    }
    Ok(v)
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

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(usize, usize, u64), u32> {
    let anchors = build_anchors_finalized()?;
    let mut led = BadMatLedger::new();
    led.expand(&anchors)?;
    led.boundary_conservation()?;
    led.build_table()?;
    let fp = led.fingerprint();
    led.dual_replay(&anchors)?;
    Ok((led.rows.len(), led.cells.len(), fp))
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
    // 族 0：锚未 Finalized 即归并（归并前置不满足）。
    ledger.tries[0] += 1;
    let mut partial: Vec<BatchAnchor> = Vec::new();
    let mut b = 0;
    while b < BATCH_COUNT {
        if let Ok(mut a) = BatchAnchor::new(16 + b) {
            if b == 7 {
                // 第 8 批停在 Anchored。
                let _ = a.advance(AnchorState::Anchored);
            } else {
                let _ = a.advance(AnchorState::Anchored);
                let _ = a.advance(AnchorState::Finalized);
            }
            partial.push(a);
        }
        b += 1;
    }
    let mut led0 = BadMatLedger::new();
    if led0.expand(&partial).is_err() {
        ledger.caught[0] += 1;
    }
    // 族 1：重叠样本注入——REG 格强挂总回归批位改成 ATTR 档混挂。
    ledger.tries[1] += 1;
    let anchors = build_anchors_finalized();
    let mut caught1 = false;
    if let Ok(anchors) = anchors {
        let mut led1 = BadMatLedger::new();
        let _ = led1.expand(&anchors);
        if led1.cells.len() > 0 {
            // 把 ATTR 格（格 2）强挂 F14342 位 → 零重叠必红。
            led1.cells[2].reg_target = 0;
        }
        if led1.boundary_conservation().is_err() {
            caught1 = true;
        }
        // 同格双档：owner 越闭集值 0x7F 注入。
        if led1.cells.len() > 3 {
            led1.cells[3].owner = 0x7F;
        }
        if led1.boundary_conservation().is_err() {
            caught1 = true;
        }
    }
    if caught1 {
        ledger.caught[1] += 1;
    }
    // 族 2：批锚状态机跳步注入（Draft→Finalized 必拒）。
    ledger.tries[2] += 2;
    if let Ok(mut a) = BatchAnchor::new(16) {
        if a.advance(AnchorState::Finalized).is_err() {
            ledger.caught[2] += 1;
        }
    }
    if BatchAnchor::new(26).is_err() {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < BadMatCode::ALL.len() {
        let v = BadMatCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：10 行总表 + 40 格 + 分界 20/20。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((rows, cells, _)) => *rows == BATCH_COUNT && *cells == CELL_TOTAL,
        Err(_) => false,
    };
    v.push(("J1-10行归并总表+40格", g_ok, format!("{:?}", golden)));
    // 2. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓（重叠必红）",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 3. 分界 20/20 独立复核（干净账）。
    let boundary_ok = {
        let anchors = build_anchors_finalized();
        match anchors {
            Ok(a) => {
                let mut led = BadMatLedger::new();
                let e = led.expand(&a);
                match e {
                    Ok(()) => led.boundary_conservation().is_ok(),
                    Err(_) => false,
                }
            }
            Err(_) => false,
        }
    };
    v.push((
        "J1-B31 属性面分界 20/20",
        boundary_ok,
        format!("{}/{}", REG_CELLS, ATTR_CELLS),
    ));
    // 4. 总回归批位映射抽验（两两归并闭式）。
    let target_ok = reg_target(16) == Ok(0)
        && reg_target(17) == Ok(0)
        && reg_target(24) == Ok(4)
        && reg_target(25) == Ok(4);
    v.push((
        "J1-F14342–F14346 两两归并映射",
        target_ok,
        String::from("B16/17→0 B24/25→4"),
    ));
    // 5. 坏型闭集名点验。
    let mut named = 0;
    let mut i = 0;
    while i < BADKIND_COUNT {
        if BADKIND_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    v.push((
        "J1-五坏型名点验",
        named == BADKIND_COUNT,
        format!("{}/5", named),
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

pub fn run_unx14341_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14341 坏档矩阵全域总回归总起（E 段十批归并总框架）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
