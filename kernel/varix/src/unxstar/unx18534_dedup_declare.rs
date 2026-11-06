//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18534 探针库防重声明（与 B02–B06 划界） · 0x80140段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18534-J1 20 条主题聚类对前五批可见判据对表，嫌疑清单=空断言。
//! 结构清单：
//!   - 数据结构：ClusterRec（Copy 聚类记录：编号/已声明对数/状态）、
//!     DeclareBoard（20 聚类数组账本 + 512 位判据键位面 + 键账）、
//!     前五批可见判据对表（B02–B06 冻结对照表 40 对，裁决表）；
//!   - 状态机：Scanning→Declaring→Locked 三态，锁定后声明显式拒绝；
//!   - 账本：判据键账（追加序单调）+ 嫌疑清单（碰撞入账）+ 拒因账；
//!   - 错误路径：DdCode 闭集 12 码，code() = SEG_BASE + n（0x80140 段），
//!     每码非空中文建议；
//!   - 注入核：撞前批键 / 批内重键 / 未知聚类 / 空判据对 四类坏样本
//!     各 10 轮逐类断言被显式拒绝（10/10 全抓）。
//! 键段隔离推导（写死）：前五批键闭集 = 10_000 + 批×100 + 槽（B02–B06 →
//!   10_200..10_607，共 40 键）；本批键从 NEW_KEY_BASE = 20_000 起段，
//!   与前五批零重叠 → 正例嫌疑清单结构性为空。
//! 位面注入性推导：键差均 <512 且非 512 整数倍 → bucket = key % 512
//!   对键闭集单射，popcount == 声明数可作硬对账。
//! 纪律：no_std + alloc；内核路径零 panic；无浮点/IO/墙钟/随机。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80140，AI-VX11-1105 G1 批07 独占区，卡表给定）。
pub const SEG_BASE: u32 = 0x80140;

// ── 冻结常量（推导注明） ─────────────────────────────────────────
/// 主题聚类数（J1 冻结：20 条）。
pub const CLUSTER_TOTAL: usize = 20;
/// 聚类编号下限（0 号保留给对照表索引）。
pub const CLUSTER_ID_MIN: u8 = 1;
/// 聚类编号上限。
pub const CLUSTER_ID_MAX: u8 = 20;
/// 声明板容量（判据对上限：64 对，与登记册容量对齐）。
pub const BOARD_CAP: usize = 64;
/// 位面桶数（512 位 = 8 × u64）。
pub const PLANE_BITS: usize = 512;
/// 位面字数（512 / 64）。
pub const PLANE_WORDS: usize = PLANE_BITS / 64;
/// 前五批键基数（B02–B06 键段起点）。
pub const PRIOR_KEY_BASE: u32 = 10_000;
/// 前五批键步距（批 ×100 + 槽 ×1）。
pub const PRIOR_KEY_BATCH_STRIDE: u32 = 100;
/// 每批可见判据对槽位（8 槽 × 5 批 = 40 对）。
pub const PRIOR_SLOTS_PER_BATCH: u32 = 8;
/// 前五批批号下界（B02）。
pub const PRIOR_BATCH_MIN: u32 = 2;
/// 前五批批号上界（B06）。
pub const PRIOR_BATCH_MAX: u32 = 6;
/// 前五批对表总对数（5 批 × 8 槽）。
pub const PRIOR_PAIRS: usize = 40;
/// 本批键基数（20_000：与前五批 10_000 段隔离，结构性零碰撞）。
pub const NEW_KEY_BASE: u32 = 20_000;
/// 本批批号（B07）。
pub const CURRENT_BATCH: u32 = 7;
/// 单聚类声明配额上限（主对 + 副对至多 4）。
pub const CLUSTER_QUOTA_MAX: u8 = 4;
/// 账态：扫描中（对照表就绪）。
pub const STATE_SCANNING: u8 = 1;
/// 账态：声明中（可受理）。
pub const STATE_DECLARING: u8 = 2;
/// 账态：已锁定（终态，只读）。
pub const STATE_LOCKED: u8 = 3;
/// 判据迭代轮数（写死：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入类别数（4 类坏样本）。
pub const INJECT_CLASSES: usize = 4;

// ── 前五批可见判据对表（冻结裁决表：B02–B06 × 8 槽） ────────────
/// (批号, 判据键) 40 对。键 = 10_000 + 批×100 + 槽。
pub const PRIOR_TABLE: [(u32, u32); PRIOR_PAIRS] = [
    (2, 10_200), (2, 10_201), (2, 10_202), (2, 10_203),
    (2, 10_204), (2, 10_205), (2, 10_206), (2, 10_207),
    (3, 10_300), (3, 10_301), (3, 10_302), (3, 10_303),
    (3, 10_304), (3, 10_305), (3, 10_306), (3, 10_307),
    (4, 10_400), (4, 10_401), (4, 10_402), (4, 10_403),
    (4, 10_404), (4, 10_405), (4, 10_406), (4, 10_407),
    (5, 10_500), (5, 10_501), (5, 10_502), (5, 10_503),
    (5, 10_504), (5, 10_505), (5, 10_506), (5, 10_507),
    (6, 10_600), (6, 10_601), (6, 10_602), (6, 10_603),
    (6, 10_604), (6, 10_605), (6, 10_606), (6, 10_607),
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DdCode {
    /// 未知聚类（编号 0 或 >20）
    ClusterUnknown,
    /// 空判据对（键为 0）
    EmptyPair,
    /// 撞前五批键（嫌疑入账）
    PriorCollision,
    /// 批内重键（内部防重命中）
    InternalDuplicate,
    /// 声明板满（64 对耗尽）
    BoardFull,
    /// 聚类配额超限（>4 对）
    PairQuotaExceeded,
    /// 批号越界（本批只受理 B07）
    BatchOutOfRange,
    /// 账态非法转换（锁定后声明/越级）
    IllegalTransition,
    /// 位面账损坏（popcount ≠ 声明数）
    PlaneCorrupt,
    /// 嫌疑清单非空（划界失败）
    SuspectsFound,
    /// 守恒破坏
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
}

impl DdCode {
    /// 诊断码取值：SEG_BASE + 枚举序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 非空中文建议（文案闭集）。
    pub fn detail(self) -> &'static str {
        match self {
            DdCode::ClusterUnknown => "未知聚类：编号闭集为 1..=20，0 号为对照表保留",
            DdCode::EmptyPair => "空判据对：键为 0 无划界意义，声明显式拒绝",
            DdCode::PriorCollision => "撞前五批键：与 B02–B06 可见判据对表命中，改键或并册",
            DdCode::InternalDuplicate => "批内重键：同键本批已声明，防重命中拒绝",
            DdCode::BoardFull => "声明板满：64 对耗尽，账本只增不减需扩册",
            DdCode::PairQuotaExceeded => "聚类配额超限：单聚类至多 4 对",
            DdCode::BatchOutOfRange => "批号越界：本批声明板只受理 B07",
            DdCode::IllegalTransition => "账态非法转换：三态只进不退，锁定后只读",
            DdCode::PlaneCorrupt => "位面账损坏：popcount 须与声明数逐位吻合",
            DdCode::SuspectsFound => "嫌疑清单非空：与前五批划界失败，逐条列出待裁决",
            DdCode::ConservationBroken => "守恒破坏：声明数 = 位面 1 位数 不成立",
            DdCode::NotReproducible => "双跑不一致：同输入两次声明结果不同",
        }
    }
    /// 码闭集清单。
    pub const ALL: [DdCode; 12] = [
        DdCode::ClusterUnknown,
        DdCode::EmptyPair,
        DdCode::PriorCollision,
        DdCode::InternalDuplicate,
        DdCode::BoardFull,
        DdCode::PairQuotaExceeded,
        DdCode::BatchOutOfRange,
        DdCode::IllegalTransition,
        DdCode::PlaneCorrupt,
        DdCode::SuspectsFound,
        DdCode::ConservationBroken,
        DdCode::NotReproducible,
    ];
}

/// 诊断码 → 中文建议（闭集外返回未知提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DdCode::ALL.len() {
        if DdCode::ALL[i].code() == code {
            return DdCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80140 段闭集内"
}

// ── 位面原语（无 panic） ────────────────────────────────────────
/// 键 → 位面桶下标（key % 512；键闭集内单射，见文件头推导）。
pub fn bucket_of(key: u32) -> usize {
    (key % PLANE_BITS as u32) as usize
}

/// 置位（越界忽略）。
fn plane_set(planes: &mut [u64; PLANE_WORDS], idx: usize) {
    let w = idx / 64;
    if let Some(slot) = planes.get_mut(w) {
        *slot |= 1u64 << (idx % 64);
    }
}

/// 读位（越界 false）。
fn plane_get(planes: &[u64; PLANE_WORDS], idx: usize) -> bool {
    match planes.get(idx / 64) {
        Some(v) => (v >> (idx % 64)) & 1 == 1,
        None => false,
    }
}

/// 位面 1 位计数（手写）。
fn plane_popcount(planes: &[u64; PLANE_WORDS]) -> usize {
    let mut n = 0usize;
    let mut w = 0;
    while w < planes.len() {
        let mut bits = *planes.get(w).unwrap_or(&0);
        while bits != 0 {
            bits &= bits.wrapping_sub(1);
            n += 1;
        }
        w += 1;
    }
    n
}

// ── 聚类记录（Copy） ─────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClusterRec {
    /// 聚类编号（1..=20）。
    pub id: u8,
    /// 已声明判据对数（≤4）。
    pub declared: u8,
    /// 账态（STATE_* 闭集，随板同步）。
    pub state: u8,
    /// 空槽标记。
    pub empty: bool,
}

impl ClusterRec {
    /// 空聚类（const fn）。
    pub const fn empty() -> ClusterRec {
        ClusterRec { id: 0, declared: 0, state: STATE_SCANNING, empty: true }
    }
}

// ── 声明板（核心账本 + 三态状态机 + 位面 + 嫌疑清单） ───────────
pub struct DeclareBoard {
    /// 20 聚类数组账本。
    clusters: [ClusterRec; CLUSTER_TOTAL],
    /// 判据键账（追加序单调）。
    keys: [u32; BOARD_CAP],
    /// 已声明键位面（512 位）。
    declared_plane: [u64; PLANE_WORDS],
    /// 已声明对数。
    count: usize,
    /// 账态。
    state: u8,
    /// 嫌疑清单（撞前批键逐条入账，正例必须为空）。
    pub suspects: Vec<u32>,
    /// 拒因账（只增不减）。
    reject_ledger: Vec<u32>,
}

impl DeclareBoard {
    pub fn new() -> DeclareBoard {
        DeclareBoard {
            clusters: [ClusterRec::empty(); CLUSTER_TOTAL],
            keys: [0u32; BOARD_CAP],
            declared_plane: [0u64; PLANE_WORDS],
            count: 0,
            state: STATE_SCANNING,
            suspects: Vec::new(),
            reject_ledger: Vec::new(),
        }
    }

    /// 开板：Scanning → Declaring。
    pub fn begin(&mut self) -> Result<(), u32> {
        if self.state != STATE_SCANNING {
            self.reject_ledger.push(DdCode::IllegalTransition.code());
            return Err(DdCode::IllegalTransition.code());
        }
        self.state = STATE_DECLARING;
        Ok(())
    }

    /// 锁板：Declaring → Locked（终态）。
    pub fn lock(&mut self) -> Result<(), u32> {
        if self.state != STATE_DECLARING {
            self.reject_ledger.push(DdCode::IllegalTransition.code());
            return Err(DdCode::IllegalTransition.code());
        }
        self.state = STATE_LOCKED;
        Ok(())
    }

    /// 前五批键精确扫描（权威判定，线性 40 对）。
    pub fn prior_has(&self, key: u32) -> bool {
        let mut i = 0;
        while i < PRIOR_TABLE.len() {
            match PRIOR_TABLE.get(i) {
                Some(&(b, k)) => {
                    if k == key && b >= PRIOR_BATCH_MIN && b <= PRIOR_BATCH_MAX {
                        return true;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        false
    }

    /// 批内重键精确扫描（位面为快速预筛，账内扫描为权威）。
    fn internal_has(&self, key: u32) -> bool {
        let mut i = 0;
        while i < self.count {
            match self.keys.get(i) {
                Some(&k) => {
                    if k == key {
                        return true;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        false
    }

    /// 受理一笔声明：(聚类, 判据键, 批号)。
    /// 闸序：账态→聚类→空键→批号→前批碰撞→批内重键→配额→容量。
    pub fn declare(&mut self, cluster_id: u8, key: u32, batch_no: u32) -> Result<(), u32> {
        // 闸一：账态。
        if self.state != STATE_DECLARING {
            self.reject_ledger.push(DdCode::IllegalTransition.code());
            return Err(DdCode::IllegalTransition.code());
        }
        // 闸二：聚类闭集。
        if cluster_id < CLUSTER_ID_MIN || cluster_id > CLUSTER_ID_MAX {
            self.reject_ledger.push(DdCode::ClusterUnknown.code());
            return Err(DdCode::ClusterUnknown.code());
        }
        // 闸三：空键。
        if key == 0 {
            self.reject_ledger.push(DdCode::EmptyPair.code());
            return Err(DdCode::EmptyPair.code());
        }
        // 闸四：批号。
        if batch_no != CURRENT_BATCH {
            self.reject_ledger.push(DdCode::BatchOutOfRange.code());
            return Err(DdCode::BatchOutOfRange.code());
        }
        // 闸五：撞前五批键（嫌疑入账，权威拒绝）。
        if self.prior_has(key) {
            self.suspects.push(key);
            self.reject_ledger.push(DdCode::PriorCollision.code());
            return Err(DdCode::PriorCollision.code());
        }
        // 闸六：批内重键（先位面预筛，命中再精确扫描复核）。
        if plane_get(&self.declared_plane, bucket_of(key)) && self.internal_has(key) {
            self.reject_ledger.push(DdCode::InternalDuplicate.code());
            return Err(DdCode::InternalDuplicate.code());
        }
        // 闸七：聚类配额。
        let cidx = cluster_id as usize - 1; // 1..=20 → 0..=19
        let cur_declared = match self.clusters.get(cidx) {
            Some(c) => c.declared,
            None => {
                self.reject_ledger.push(DdCode::PlaneCorrupt.code());
                return Err(DdCode::PlaneCorrupt.code());
            }
        };
        if cur_declared >= CLUSTER_QUOTA_MAX {
            self.reject_ledger.push(DdCode::PairQuotaExceeded.code());
            return Err(DdCode::PairQuotaExceeded.code());
        }
        // 闸八：容量。
        if self.count >= BOARD_CAP {
            self.reject_ledger.push(DdCode::BoardFull.code());
            return Err(DdCode::BoardFull.code());
        }
        // 写账：键账 + 位面 + 聚类记录（先算后写，避免双借用）。
        match self.keys.get_mut(self.count) {
            Some(sl) => {
                *sl = key;
            }
            None => {
                self.reject_ledger.push(DdCode::BoardFull.code());
                return Err(DdCode::BoardFull.code());
            }
        }
        plane_set(&mut self.declared_plane, bucket_of(key));
        match self.clusters.get_mut(cidx) {
            Some(c) => {
                if c.empty {
                    c.id = cluster_id;
                    c.empty = false;
                    c.state = STATE_DECLARING;
                }
                c.declared += 1;
            }
            None => {
                self.reject_ledger.push(DdCode::PlaneCorrupt.code());
                return Err(DdCode::PlaneCorrupt.code());
            }
        }
        self.count += 1;
        Ok(())
    }

    /// 位面对账：popcount == 声明数，且每个已声明键同位有置位。
    pub fn audit_plane(&self) -> Result<(), u32> {
        if plane_popcount(&self.declared_plane) != self.count {
            return Err(DdCode::PlaneCorrupt.code());
        }
        let mut i = 0;
        while i < self.count {
            let key = match self.keys.get(i) {
                Some(&k) => k,
                None => return Err(DdCode::PlaneCorrupt.code()),
            };
            if !plane_get(&self.declared_plane, bucket_of(key)) {
                return Err(DdCode::PlaneCorrupt.code());
            }
            i += 1;
        }
        Ok(())
    }

    /// 划界断言：嫌疑清单必须为空（J1 红线）。
    pub fn suspects_empty(&self) -> Result<(), u32> {
        if !self.suspects.is_empty() {
            return Err(DdCode::SuspectsFound.code());
        }
        Ok(())
    }

    /// 键账追加序单调（键段自 20_000 起严格递增声明时成立）。
    pub fn keys_monotonic(&self) -> bool {
        let mut prev = 0u32;
        let mut i = 0;
        while i < self.count {
            let k = match self.keys.get(i) {
                Some(&v) => v,
                None => return false,
            };
            if k <= prev {
                return false;
            }
            prev = k;
            i += 1;
        }
        true
    }

    /// 声明数守恒：各聚类 declared 之和 == 总声明数。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut sum = 0u32;
        let mut i = 0;
        while i < CLUSTER_TOTAL {
            let c = match self.clusters.get(i) {
                Some(c) => *c,
                None => return Err(DdCode::PlaneCorrupt.code()),
            };
            if !c.empty {
                sum += c.declared as u32;
            }
            i += 1;
        }
        if sum as usize != self.count {
            return Err(DdCode::ConservationBroken.code());
        }
        self.audit_plane()
    }

    pub fn declared_count(&self) -> usize {
        self.count
    }

    pub fn state(&self) -> u8 {
        self.state
    }

    pub fn reject_count(&self) -> usize {
        self.reject_ledger.len()
    }

    /// 第 idx 键只读（越界 None）。
    pub fn key_at(&self, idx: usize) -> Option<u32> {
        match self.keys.get(idx) {
            Some(&k) if k != 0 => Some(k),
            _ => None,
        }
    }
}

// ── 黄金链：20 聚类各声明 1 对 → 锁板 → 嫌疑清单=空 ─────────────
/// 返回 (声明数, 嫌疑笔数, 拒因笔数)。20 对全绿：嫌疑空、位面对账齐。
pub fn golden_run() -> Result<(usize, usize, usize), u32> {
    let mut board = DeclareBoard::new();
    board.begin()?;
    // 20 聚类：键 = 20_000 + 编号×10（20_010..20_200，段内严格递增）。
    let mut cid = CLUSTER_ID_MIN;
    while cid <= CLUSTER_ID_MAX {
        let key = NEW_KEY_BASE + cid as u32 * 10;
        board.declare(cid, key, CURRENT_BATCH)?;
        cid += 1;
    }
    board.lock()?;
    board.conserve()?;
    board.suspects_empty()?;
    if board.declared_count() != CLUSTER_TOTAL {
        return Err(DdCode::ConservationBroken.code());
    }
    if !board.keys_monotonic() {
        return Err(DdCode::ConservationBroken.code());
    }
    Ok((board.declared_count(), board.suspects.len(), board.reject_count()))
}

// ── 注入核：四类坏样本，各 10 轮，逐类全抓 ──────────────────────
pub struct InjectLedger {
    pub tries: [u32; INJECT_CLASSES],
    pub caught: [u32; INJECT_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJECT_CLASSES], caught: [0; INJECT_CLASSES] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJECT_CLASSES {
            let t = *self.tries.get(i).unwrap_or(&0);
            let c = *self.caught.get(i).unwrap_or(&0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn conservation(&self) -> Result<(), u32> {
        let mut tries = 0u32;
        let mut caught = 0u32;
        let mut i = 0;
        while i < INJECT_CLASSES {
            tries = tries.saturating_add(*self.tries.get(i).unwrap_or(&0));
            caught = caught.saturating_add(*self.caught.get(i).unwrap_or(&0));
            i += 1;
        }
        if caught > tries {
            return Err(DdCode::ConservationBroken.code());
        }
        Ok(())
    }
}

/// 四类坏样本逐类注入 `rounds` 轮：
/// C0 撞前批键（嫌疑入账）、C1 批内重键、C2 未知聚类、C3 空判据对。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // C0：撞前批键（10200 在 B02 对表内）→ 拒 + 嫌疑入账。
        let mut b0 = DeclareBoard::new();
        let _ = b0.begin();
        let hit0 = b0.declare(1, PRIOR_KEY_BASE + 200, CURRENT_BATCH).err()
            == Some(DdCode::PriorCollision.code())
            && b0.suspects.len() == 1;
        led.record(0, hit0);
        // C1：批内重键。
        let mut b1 = DeclareBoard::new();
        let _ = b1.begin();
        let _ = b1.declare(1, NEW_KEY_BASE + 10, CURRENT_BATCH);
        led.record(
            1,
            b1.declare(2, NEW_KEY_BASE + 10, CURRENT_BATCH).err()
                == Some(DdCode::InternalDuplicate.code()),
        );
        // C2：未知聚类（0 与 21 双探）。
        let mut b2 = DeclareBoard::new();
        let _ = b2.begin();
        let hit2 = b2.declare(0, NEW_KEY_BASE + 10, CURRENT_BATCH).is_err()
            && b2.declare(21, NEW_KEY_BASE + 20, CURRENT_BATCH).err()
                == Some(DdCode::ClusterUnknown.code());
        led.record(2, hit2);
        // C3：空判据对。
        let mut b3 = DeclareBoard::new();
        let _ = b3.begin();
        led.record(3, b3.declare(1, 0, CURRENT_BATCH).err() == Some(DdCode::EmptyPair.code()));
        r += 1;
    }
    led.conservation()?;
    Ok(led)
}

/// 对照表审计：40 对键全在 B02–B06 键段内且批号吻合。
pub fn prior_table_audit() -> Result<(), u32> {
    if PRIOR_TABLE.len() != PRIOR_PAIRS {
        return Err(DdCode::ConservationBroken.code());
    }
    let mut i = 0;
    while i < PRIOR_TABLE.len() {
        let (b, k) = match PRIOR_TABLE.get(i) {
            Some(&p) => p,
            None => return Err(DdCode::ConservationBroken.code()),
        };
        if b < PRIOR_BATCH_MIN || b > PRIOR_BATCH_MAX {
            return Err(DdCode::ConservationBroken.code());
        }
        let want = PRIOR_KEY_BASE + b * PRIOR_KEY_BATCH_STRIDE + (i % PRIOR_SLOTS_PER_BATCH as usize) as u32;
        if k != want {
            return Err(DdCode::ConservationBroken.code());
        }
        i += 1;
    }
    Ok(())
}

// ── 探针三序（开板→声明→锁板 / 越级 / 锁板后声明） ──────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let mut board = DeclareBoard::new();
    board.begin()?;
    board.declare(1, NEW_KEY_BASE + 10, CURRENT_BATCH)?;
    board.lock()?;
    board.conserve()?;
    board.suspects_empty()
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 越级：未开板直接声明。
    let mut board = DeclareBoard::new();
    board.declare(1, NEW_KEY_BASE + 10, CURRENT_BATCH)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 锁板后声明必拒。
    let mut board = DeclareBoard::new();
    board.begin()?;
    board.declare(1, NEW_KEY_BASE + 10, CURRENT_BATCH)?;
    board.lock()?;
    board.declare(2, NEW_KEY_BASE + 20, CURRENT_BATCH)
}

/// 容量耗尽：64 对写满后第 65 对必拒且账不丢。
pub fn capacity_probe() -> Result<usize, u32> {
    let mut board = DeclareBoard::new();
    board.begin()?;
    // 64 对：20 聚类轮转派键（键保持段内递增且唯一）。
    let mut i = 0usize;
    while i < BOARD_CAP {
        let cid = (i % CLUSTER_TOTAL) as u8 + 1;
        let key = NEW_KEY_BASE + 1000 + i as u32; // 21_000..21_063 唯一递增
        board.declare(cid, key, CURRENT_BATCH)?;
        i += 1;
    }
    let before = board.declared_count();
    // 用配额未满的聚类 5（仅 3 对）发第 65 笔，确保命中容量闸而非配额闸。
    let r = board.declare(5, NEW_KEY_BASE + 2000, CURRENT_BATCH);
    if r.err() != Some(DdCode::BoardFull.code()) {
        return Err(DdCode::ConservationBroken.code());
    }
    if board.declared_count() != before {
        return Err(DdCode::ConservationBroken.code());
    }
    Ok(before)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DdCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x80000 || SEG_BASE >= 0x80400 {
        return false;
    }
    let mut i = 0;
    while i < DdCode::ALL.len() {
        let v = DdCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集。
pub fn detail_closed_set() -> bool {
    let mut i = 0;
    while i < DdCode::ALL.len() {
        if DdCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 判据集（J1 验收） ───────────────────────────────────────────
pub fn run_unx18534_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18534 探针库防重声明");
    // 静态明细判据。
    s.add("J1-段位守卫", segment_guard(), "0x80140 段 0x40 窗口，码闭集不越界");
    s.add(
        "J1-对照表审计",
        prior_table_audit().is_ok(),
        "B02–B06 冻结对表 40 对逐对键段/批号吻合",
    );
    s.add(
        "J1-键段隔离",
        NEW_KEY_BASE >= PRIOR_KEY_BASE + PRIOR_BATCH_MAX * PRIOR_KEY_BATCH_STRIDE + PRIOR_SLOTS_PER_BATCH,
        "本批键段 20_000 与前五批 10_200..10_607 零重叠",
    );
    s.add(
        "J1-账态三序",
        probe_seq_a().is_ok()
            && probe_seq_b().err() == Some(DdCode::IllegalTransition.code())
            && probe_seq_c().err() == Some(DdCode::IllegalTransition.code()),
        "开板→声明→锁板全过；越级/锁板后声明均拒",
    );
    s.add(
        "J1-边界四类闸",
        {
            // 聚类 0/21、键 0、批号 6（前批批号）、配额 5 连发全拒。
            let mut board = DeclareBoard::new();
            let _ = board.begin();
            let c0 = board.declare(0, NEW_KEY_BASE + 10, CURRENT_BATCH).is_err();
            let c21 = board.declare(21, NEW_KEY_BASE + 20, CURRENT_BATCH).is_err();
            let k0 = board.declare(1, 0, CURRENT_BATCH).is_err();
            let b6 = board.declare(1, NEW_KEY_BASE + 30, PRIOR_BATCH_MAX).is_err();
            let mut quota = DeclareBoard::new();
            let _ = quota.begin();
            let mut q = 0;
            while q < CLUSTER_QUOTA_MAX {
                let _ = quota.declare(1, NEW_KEY_BASE + 100 + q as u32, CURRENT_BATCH);
                q += 1;
            }
            let over_quota = quota
                .declare(1, NEW_KEY_BASE + 200, CURRENT_BATCH)
                .err()
                == Some(DdCode::PairQuotaExceeded.code());
            c0 && c21 && k0 && b6 && over_quota
        },
        "聚类 0/21、空键、前批批号、配额超限全部显式拒绝",
    );
    s.add(
        "J1-文案闭集",
        detail_closed_set(),
        "12 码逐码非空中文建议，闭集外落未知提示",
    );
    // 动态明细判据（≤8 条）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, sus, _)) => *n == CLUSTER_TOTAL && *sus == 0,
        Err(_) => false,
    };
    s.add_dyn(
        "J1-20聚类嫌疑清单=空",
        g_ok,
        &format!("声明/嫌疑/拒因 = {:?}", g),
    );
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn(
        "J1-注入四类10/10全抓",
        inj_ok,
        &format!(
            "tries={:?} caught={:?}",
            inj.as_ref().map(|l| l.tries).unwrap_or([0; INJECT_CLASSES]),
            inj.as_ref().map(|l| l.caught).unwrap_or([0; INJECT_CLASSES])
        ),
    );
    let cap = capacity_probe();
    s.add_dyn(
        "J1-容量耗尽账不丢",
        cap.is_ok(),
        &format!("满板对数 = {:?}", cap),
    );
    let rep = repro_check();
    s.add_dyn("J1-双跑一致", rep.is_ok(), &format!("{:?}", rep));
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_20_clusters_suspects_empty() {
        let g = golden_run().unwrap_or((0, 99, 99));
        assert_eq!(g.0, CLUSTER_TOTAL);
        assert_eq!(g.1, 0, "嫌疑清单必须为空");
        assert_eq!(g.2, 0, "正例零拒因");
    }

    #[test]
    fn test_prior_collision_suspect_entry() {
        let mut board = DeclareBoard::new();
        let _ = board.begin();
        assert!(board.declare(1, 10_200, CURRENT_BATCH).is_err());
        assert_eq!(board.suspects.len(), 1);
        assert!(board.suspects_empty().is_err());
        // 权威扫描：前批键命中，新批键不命中。
        assert!(board.prior_has(10_607));
        assert!(!board.prior_has(20_010));
    }

    #[test]
    fn test_internal_duplicate() {
        let mut board = DeclareBoard::new();
        let _ = board.begin();
        assert!(board.declare(1, NEW_KEY_BASE + 10, CURRENT_BATCH).is_ok());
        // 同键换聚类声明也拒（键账全局防重）。
        assert!(board.declare(2, NEW_KEY_BASE + 10, CURRENT_BATCH).is_err());
        assert_eq!(board.declared_count(), 1);
    }

    #[test]
    fn test_inject_all_caught_10_rounds() {
        let led = inject_rounds(JUDGE_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(led.all_caught(), "四类坏样本 10 轮必须全抓");
        assert!(led.conservation().is_ok());
    }

    #[test]
    fn test_state_machine_transitions() {
        // 越级声明拒。
        let mut board = DeclareBoard::new();
        assert!(board.declare(1, NEW_KEY_BASE + 10, CURRENT_BATCH).is_err());
        // 正常链 + 锁板后声明拒。
        let _ = board.begin();
        assert!(board.declare(1, NEW_KEY_BASE + 10, CURRENT_BATCH).is_ok());
        assert!(board.lock().is_ok());
        assert!(board.declare(2, NEW_KEY_BASE + 20, CURRENT_BATCH).is_err());
        // 二次开板拒（Locked 终态）。
        assert!(board.begin().is_err());
    }

    #[test]
    fn test_boundary_four_classes() {
        let mut board = DeclareBoard::new();
        let _ = board.begin();
        assert!(board.declare(0, NEW_KEY_BASE + 10, CURRENT_BATCH).is_err());
        assert!(board.declare(21, NEW_KEY_BASE + 10, CURRENT_BATCH).is_err());
        assert!(board.declare(1, 0, CURRENT_BATCH).is_err());
        assert!(board.declare(1, NEW_KEY_BASE + 10, 2).is_err());
        assert!(board.declare(1, NEW_KEY_BASE + 10, 8).is_err());
    }

    #[test]
    fn test_cluster_quota() {
        let mut board = DeclareBoard::new();
        let _ = board.begin();
        let mut q = 0;
        while q < CLUSTER_QUOTA_MAX {
            assert!(board.declare(1, NEW_KEY_BASE + 100 + q as u32, CURRENT_BATCH).is_ok());
            q += 1;
        }
        assert!(board.declare(1, NEW_KEY_BASE + 200, CURRENT_BATCH).is_err());
        assert_eq!(board.declared_count(), CLUSTER_QUOTA_MAX as usize);
    }

    #[test]
    fn test_capacity_exhaustion() {
        assert_eq!(capacity_probe().unwrap_or(0), BOARD_CAP);
    }

    #[test]
    fn test_plane_audit_and_conserve() {
        let mut board = DeclareBoard::new();
        let _ = board.begin();
        let mut i = 0;
        while i < 10 {
            let cid = (i % CLUSTER_TOTAL) as u8 + 1;
            let _ = board.declare(cid, NEW_KEY_BASE + 10 + i as u32, CURRENT_BATCH);
            i += 1;
        }
        assert!(board.audit_plane().is_ok());
        assert!(board.conserve().is_ok());
        assert!(board.keys_monotonic());
    }

    #[test]
    fn test_double_run_reproducible() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_prior_table_and_segment_guard() {
        assert!(prior_table_audit().is_ok());
        assert!(segment_guard());
        assert!(detail_closed_set());
        assert_eq!(DdCode::SuspectsFound.code(), SEG_BASE + 9);
    }

    #[test]
    fn test_checks_all_passed() {
        let set = run_unx18534_checks();
        assert!(set.all_passed(), "UNX-F18534 判据必须全绿：{}", set.red_items_text());
    }
}
