//! UNX-F12774 ktest 聚合器（B39 20 条） · 0x82B80段
//!
//! 判据原文（派工卡 12774 逐字锚定）：
//!   UNX-F12775-J1 对齐收官账等式断言绿（资产×抽检×差异三账对平）。
//!
//! 设计要点：
//! 1. 20 条单跑入口登记表为冻结闭集：入口号 1..=20 / 入口名代号（A01..A20）/
//!    所属批号（B39A..B39D 四批各 5 条）/ 聚合旗标（15 置位 5 未置），逐条
//!    可复算——入口号互异无缺号、批号落 0..4、单跑条数 = 4 + 入口号 mod 5
//!    独立累减路径复算一致；
//! 2. 聚合判定器 aggregate(entry_no) 三闸定序：未登记入口 → ENOENT，
//!    封账倒退 → ERewind，旗标未置 → EPERM，重复聚合 → EALREADY；
//!    每行产出登记值/实算值/差异三元组，差异必须为 0，非零行拒入账；
//! 3. 聚合账等式断言：单跑条数合计 = 聚合账条数（旗标置位合计 90 =
//!    逐批 22+23+24+21 逐批累加可复算），聚合账行随批生成并携带批内序号，
//!    序号必须逐批 1..=n 连续无缺号无重号；
//! 4. 三账对平面：资产账（Σ 登记值）× 抽检账（Σ 实算值）× 聚合账条数
//!    三列等式 + 批累加等式 + 批内序号连续性，任一破即对平失败，封账被拒；
//! 5. 注入器三类（未登记入口 / 重复聚合 / 账行篡改【判定行登记值 +Δ 与
//!    账行批号错位两路轮替】）各 10 轮确定性注入（LCG 种子写死），必须
//!    全数被抓，任何一路逃脱即判据失效；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!    派生且写死）；取值一律 match/if let/.get()/checked_*/saturating_*，
//!    无 unwrap/expect/panic/裸下标/浮点，状态机单向只进不退。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12774 − 12600) = 0x80000 + 0x2B80）。
pub const SEG_BASE: u32 = 0x82B80;

// ── 聚合器诊断码闭集 ─────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KtDiag {
    /// 未登记入口（ENOENT）：入口号不在 1..=20 冻结闭集内。
    ENoEntry,
    /// 重复聚合（EALREADY）：该入口已聚合过一次，只许落一笔。
    EDupAgg,
    /// 旗标未置（EPERM）：入口聚合旗标未置位，禁止聚合。
    ENoPerm,
    /// 行三元组不平（账行篡改痕迹）：登记值 ≠ 实算值，差异非零拒入账。
    ERowDiff,
    /// 聚合账容量耗尽：账行数已达定容上限，追加显性拒。
    EAggFull,
    /// 未聚合齐禁止封账：旗标置位入口未全部聚合完。
    ENotFull,
    /// 对平断言未全绿：三账对平面有红项，封账失败。
    ERecon,
    /// 状态机倒退：封账后再写、或重复封账，只进不退。
    ERewind,
    /// 批号越界：批号不在 0..4 闭集内。
    EBatch,
    /// 批内序号断裂：序号必须逐批 1..=n 连续，缺号重号即账行可疑。
    ESeq,
    /// 篡改注入逃脱：注入后对平断言仍绿，断言面失效须整账复核。
    ETamper,
    /// 双跑指纹不一致：同种子两次构建指纹不同，确定性被破坏。
    EFingerprint,
}

impl KtDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                KtDiag::ENoEntry => 0x01,
                KtDiag::EDupAgg => 0x02,
                KtDiag::ENoPerm => 0x03,
                KtDiag::ERowDiff => 0x04,
                KtDiag::EAggFull => 0x05,
                KtDiag::ENotFull => 0x06,
                KtDiag::ERecon => 0x07,
                KtDiag::ERewind => 0x08,
                KtDiag::EBatch => 0x09,
                KtDiag::ESeq => 0x0a,
                KtDiag::ETamper => 0x0b,
                KtDiag::EFingerprint => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            KtDiag::ENoEntry => "未登记入口：入口号不在 1..=20 冻结闭集内，先对登记表核对入口号再聚合",
            KtDiag::EDupAgg => "重复聚合：每入口只落一笔聚合判定行，重复请求先查已聚合清单",
            KtDiag::ENoPerm => "旗标未置：该入口聚合旗标未置位，先走旗标置位审批再聚合",
            KtDiag::ERowDiff => "行三元组不平：登记值与实算值差异非零，逐条复算定位被篡改行",
            KtDiag::EAggFull => "聚合账容量耗尽：账行数已达定容 90，先封账归档再开新账",
            KtDiag::ENotFull => "未聚合齐禁止封账：旗标置位入口未全部聚合，先补齐缺项",
            KtDiag::ERecon => "三账对平有红项：按红项批号回修对应账行，全绿后再封账",
            KtDiag::ERewind => "状态机倒退：开账→聚合齐→封账只进不退，倒退即账本可疑须复核",
            KtDiag::EBatch => "批号越界：批号只认 B39A..B39D（0..4），先对批号表再落账",
            KtDiag::ESeq => "批内序号断裂：序号须逐批 1..=n 连续，缺号重号行整批复核",
            KtDiag::ETamper => "篡改注入逃脱：注入后对平仍绿即断言面失效，整账复核断言覆盖",
            KtDiag::EFingerprint => "双跑指纹不一致：同种子两次构建结果不同，查非确定性来源",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const KT_ALL: &[KtDiag] = &[
    KtDiag::ENoEntry,
    KtDiag::EDupAgg,
    KtDiag::ENoPerm,
    KtDiag::ERowDiff,
    KtDiag::EAggFull,
    KtDiag::ENotFull,
    KtDiag::ERecon,
    KtDiag::ERewind,
    KtDiag::EBatch,
    KtDiag::ESeq,
    KtDiag::ETamper,
    KtDiag::EFingerprint,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in KT_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82B81..0x82B8C 闭集内"
}

// ── 20 条单跑入口登记表（冻结闭集） ─────────────────────────────
/// 登记表容量：B39 批 20 条，闭集冻结不许增删。
pub const ENTRY_CAP: usize = 20;
/// 批数：B39A..B39D 四批，每批 5 条。
pub const BATCH_COUNT: usize = 4;
/// 批号代号表（来源：派工卡 12774 B39 批定义，冻结）。
pub const BATCH_TAGS: [&str; BATCH_COUNT] = ["B39A", "B39B", "B39C", "B39D"];
/// 旗标置位入口数（入口号 mod 4 ≠ 0 者，冻结闭集可复算）。
pub const ARMED_COUNT: usize = 15;
/// 全表单跑条数合计（4+入口号 mod 5 逐条累加，冻结 = 120）。
pub const TABLE_ROWS_TOTAL: u32 = 120;
/// 旗标置位入口单跑条数合计（= 聚合账定容，冻结 = 90）。
pub const ARMED_ROWS_TOTAL: u32 = 90;
/// 逐批旗标置位条数合计（冻结 = 22/23/24/21，与 90 对账）。
pub const BATCH_ARMED_SUM: [u32; BATCH_COUNT] = [22, 23, 24, 21];
/// 聚合账定容：旗标置位入口全部聚合后的账行总数，满容显性拒。
pub const AGG_CAP: usize = 90;

/// 单跑入口登记条目（冻结闭集的行模型）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KtEntry {
    /// 入口号：1..=20 闭集。
    pub entry_no: u32,
    /// 入口名代号：A01..A20。
    pub code: &'static str,
    /// 所属批号：0..=4 对应 B39A..B39D。
    pub batch: u32,
    /// 聚合旗标：置位方可聚合（EPERM 闸）。
    pub armed: bool,
    /// 单跑条数（登记值）：= 4 + 入口号 mod 5，逐条可复算。
    pub rows: u32,
}

/// 20 条单跑入口登记表（冻结闭集，行序即入口号序，不许重排）。
/// 常量来源：派工卡 VX9 id=12774 · ktest 聚合器 B39 批 20 条。
pub const ENTRY_TABLE: [KtEntry; ENTRY_CAP] = [
    KtEntry { entry_no: 1, code: "A01", batch: 0, armed: true, rows: 5 },
    KtEntry { entry_no: 2, code: "A02", batch: 0, armed: true, rows: 6 },
    KtEntry { entry_no: 3, code: "A03", batch: 0, armed: true, rows: 7 },
    KtEntry { entry_no: 4, code: "A04", batch: 0, armed: false, rows: 8 },
    KtEntry { entry_no: 5, code: "A05", batch: 0, armed: true, rows: 4 },
    KtEntry { entry_no: 6, code: "A06", batch: 1, armed: true, rows: 5 },
    KtEntry { entry_no: 7, code: "A07", batch: 1, armed: true, rows: 6 },
    KtEntry { entry_no: 8, code: "A08", batch: 1, armed: false, rows: 7 },
    KtEntry { entry_no: 9, code: "A09", batch: 1, armed: true, rows: 8 },
    KtEntry { entry_no: 10, code: "A10", batch: 1, armed: true, rows: 4 },
    KtEntry { entry_no: 11, code: "A11", batch: 2, armed: true, rows: 5 },
    KtEntry { entry_no: 12, code: "A12", batch: 2, armed: false, rows: 6 },
    KtEntry { entry_no: 13, code: "A13", batch: 2, armed: true, rows: 7 },
    KtEntry { entry_no: 14, code: "A14", batch: 2, armed: true, rows: 8 },
    KtEntry { entry_no: 15, code: "A15", batch: 2, armed: true, rows: 4 },
    KtEntry { entry_no: 16, code: "A16", batch: 3, armed: false, rows: 5 },
    KtEntry { entry_no: 17, code: "A17", batch: 3, armed: true, rows: 6 },
    KtEntry { entry_no: 18, code: "A18", batch: 3, armed: true, rows: 7 },
    KtEntry { entry_no: 19, code: "A19", batch: 3, armed: true, rows: 8 },
    KtEntry { entry_no: 20, code: "A20", batch: 3, armed: false, rows: 4 },
];

/// 按入口号查登记表（越闭集返回 None）。
pub fn entry_of(no: u32) -> Option<KtEntry> {
    for e in ENTRY_TABLE.iter() {
        if e.entry_no == no {
            return Some(*e);
        }
    }
    None
}

/// 旗标置位入口清单（按入口号升序，确定性）。
pub fn armed_entries() -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    for e in ENTRY_TABLE.iter() {
        if e.armed {
            v.push(e.entry_no);
        }
    }
    v
}

/// 实算值：与登记表同一闭式（条数 = 4 + 入口号 mod 5），
/// 但走独立累减路径逐条复算——两路必须一致，不一致即账不平。
pub fn recomputed_rows(no: u32) -> u32 {
    let mut rem = no;
    while rem >= 5 {
        rem = rem - 5;
    }
    4u32.saturating_add(rem)
}

/// 指定批号的旗标置位条数合计（从冻结表逐条累加复算）。
pub fn expected_batch_sum(batch: u32) -> u32 {
    let mut sum = 0u32;
    for e in ENTRY_TABLE.iter() {
        if e.armed && e.batch == batch {
            sum = sum.saturating_add(e.rows);
        }
    }
    sum
}

// ── 聚合判定行 / 聚合账行 / 状态机 ──────────────────────────────
/// 聚合判定行：登记值/实算值/差异三元组，差异必须为 0。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AggRow {
    /// 入口号。
    pub entry_no: u32,
    /// 所属批号。
    pub batch: u32,
    /// 登记值（登记表侧单跑条数）。
    pub registered: u32,
    /// 实算值（独立累减路径复算）。
    pub computed: u32,
    /// 差异（登记 − 实算，checked 减法，非零即不平）。
    pub diff: u32,
}

impl AggRow {
    /// 行级对平：差异为零且两值相等。
    pub fn balanced(&self) -> bool {
        self.diff == 0 && self.registered == self.computed
    }
}

/// 聚合账行：随批生成，携带批内序号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AggLine {
    /// 入口号。
    pub entry_no: u32,
    /// 所属批号。
    pub batch: u32,
    /// 批内序号：1..=n 连续（缺号重号即账行可疑）。
    pub seq: u32,
}

/// 聚合器状态机：开账 → 聚合齐 → 封账，单向只进不退。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AggState {
    /// 开账：允许逐入口聚合。
    Open,
    /// 聚合齐：旗标置位入口全部聚合完，等待封账。
    Full,
    /// 封账：只读，一切写入显性拒。
    Sealed,
}

impl AggState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            AggState::Open => 0,
            AggState::Full => 1,
            AggState::Sealed => 2,
        }
    }
}

/// 三账对平结论：资产账 × 抽检账 × 聚合账条数 + 批等式全绿才算对平。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recon {
    /// 资产账：Σ 登记值。
    pub asset: u32,
    /// 抽检账：Σ 实算值。
    pub spot: u32,
    /// 聚合账条数。
    pub ledger_n: u32,
    /// 资产 − 抽检 差异（None = 借位越界，必不平）。
    pub diff_as: Option<u32>,
    /// 资产 − 聚合账条数 差异。
    pub diff_al: Option<u32>,
    /// 行级三元组（登记/实算/差异）是否全零差。
    pub rows_balanced: bool,
    /// 批累加等式 + 批内序号连续性是否全过。
    pub batch_ok: bool,
}

impl Recon {
    /// 对平判定：三列等式 + 行三元组 + 批等式全绿。
    pub fn balanced(&self) -> bool {
        self.diff_as == Some(0) && self.diff_al == Some(0) && self.rows_balanced && self.batch_ok
    }
}

/// ktest 聚合器账本。
#[derive(Clone, Debug)]
pub struct Aggregator {
    /// 已聚合入口号（按聚合序）。
    done: Vec<u32>,
    /// 聚合判定行（与 done 同序同长）。
    rows: Vec<AggRow>,
    /// 聚合账行（随批生成，定容 AGG_CAP）。
    ledger: Vec<AggLine>,
    /// 状态机。
    state: AggState,
}

impl Aggregator {
    /// 开一本新聚合账（全体入口未聚合，状态 Open）。
    pub fn new() -> Aggregator {
        Aggregator {
            done: Vec::new(),
            rows: Vec::new(),
            ledger: Vec::new(),
            state: AggState::Open,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> AggState {
        self.state
    }

    /// 已聚合入口数。
    pub fn done_count(&self) -> usize {
        self.done.len()
    }

    /// 聚合账行数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 聚合判定行数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 该入口是否已聚合。
    pub fn is_done(&self, entry_no: u32) -> bool {
        let mut hit = false;
        for n in self.done.iter() {
            if *n == entry_no {
                hit = true;
            }
        }
        hit
    }

    /// 按入口号取聚合判定行。
    pub fn row_of(&self, entry_no: u32) -> Option<AggRow> {
        for r in self.rows.iter() {
            if r.entry_no == entry_no {
                return Some(*r);
            }
        }
        None
    }

    /// 取第 idx 条聚合账行（越界 None）。
    pub fn line_at(&self, idx: usize) -> Option<AggLine> {
        self.ledger.get(idx).copied()
    }

    /// 指定批号的账行数（批累加等式复算用）。
    pub fn batch_subtotal(&self, batch: u32) -> u32 {
        let mut n = 0u32;
        for l in self.ledger.iter() {
            if l.batch == batch {
                n = n.saturating_add(1);
            }
        }
        n
    }

    /// 指定入口的账行数（聚合账等式逐入口复算用）。
    pub fn lines_of_entry(&self, entry_no: u32) -> u32 {
        let mut n = 0u32;
        for l in self.ledger.iter() {
            if l.entry_no == entry_no {
                n = n.saturating_add(1);
            }
        }
        n
    }

    /// 落一条聚合账行（容量闸 + 批内序号自增）。
    fn push_line(&mut self, entry_no: u32, batch: u32) -> Result<(), u32> {
        if self.ledger.len() >= AGG_CAP {
            return Err(KtDiag::EAggFull.val());
        }
        let seq = self.batch_subtotal(batch).saturating_add(1);
        self.ledger.push(AggLine { entry_no, batch, seq });
        Ok(())
    }

    /// 账外追加口（容量耗尽边界与注入面专用诊断通道）：
    /// 越过聚合三闸直写账行，仅验登记闭集、批号闭集与容量闸。
    pub fn push_line_direct(&mut self, entry_no: u32, batch: u32) -> Result<(), u32> {
        if entry_of(entry_no).is_none() {
            return Err(KtDiag::ENoEntry.val());
        }
        if batch >= BATCH_COUNT as u32 {
            return Err(KtDiag::EBatch.val());
        }
        self.push_line(entry_no, batch)
    }

    /// 聚合判定器：四闸定序——登记闭集（ENOENT）→ 封账倒退（ERewind）
    /// → 旗标（EPERM）→ 重复（EALREADY）；行三元组差异非零拒入账。
    /// 成功时逐条落账（单跑条数条聚合账行随批生成）并推进状态机。
    pub fn aggregate(&mut self, entry_no: u32) -> Result<AggRow, u32> {
        // 第一闸：登记闭集——未登记入口无论其余条件一律 ENOENT。
        let entry = match entry_of(entry_no) {
            Some(e) => e,
            None => return Err(KtDiag::ENoEntry.val()),
        };
        // 第二闸：封账倒退——封账后一切聚合显性拒。
        if self.state.rank() >= AggState::Sealed.rank() {
            return Err(KtDiag::ERewind.val());
        }
        // 第三闸：旗标——未置位入口禁止聚合（先于重复闸）。
        if !entry.armed {
            return Err(KtDiag::ENoPerm.val());
        }
        // 第四闸：重复——已聚合入口只许落一笔。
        if self.is_done(entry_no) {
            return Err(KtDiag::EDupAgg.val());
        }
        // 行三元组：登记值 × 实算值，差异必须为 0。
        let registered = entry.rows;
        let computed = recomputed_rows(entry_no);
        let diff = match registered.checked_sub(computed) {
            Some(d) if d == 0 => 0,
            _ => return Err(KtDiag::ERowDiff.val()),
        };
        // 逐条落账：单跑条数条聚合账行随批生成。
        let mut seq = 0u32;
        while seq < registered {
            self.push_line(entry_no, entry.batch)?;
            seq = seq.saturating_add(1);
        }
        let row = AggRow {
            entry_no,
            batch: entry.batch,
            registered,
            computed,
            diff,
        };
        self.rows.push(row);
        self.done.push(entry_no);
        // 聚合齐即推进状态机（只进不退）。
        if self.done.len() == ARMED_COUNT && self.ledger.len() == AGG_CAP {
            self.state = AggState::Full;
        }
        Ok(row)
    }

    /// 指定批内序号连续性校验：逐批 1..=n 各恰一条，缺号重号即 ESeq。
    fn batch_seq_strict(&self, batch: u32) -> Result<(), u32> {
        let n = self.batch_subtotal(batch);
        let mut s = 1u32;
        while s <= n {
            let mut hits = 0u32;
            for l in self.ledger.iter() {
                if l.batch == batch && l.seq == s {
                    hits = hits.saturating_add(1);
                }
            }
            if hits != 1 {
                return Err(KtDiag::ESeq.val());
            }
            s = s.saturating_add(1);
        }
        Ok(())
    }

    /// 三账对平面：资产账（Σ 登记值）× 抽检账（Σ 实算值）× 聚合账条数
    /// 三列等式 + 行三元组 + 批累加等式 + 批内序号连续性。
    pub fn reconcile(&self) -> Recon {
        let mut asset = 0u32;
        let mut spot = 0u32;
        let mut rows_balanced = true;
        for r in self.rows.iter() {
            asset = asset.saturating_add(r.registered);
            spot = spot.saturating_add(r.computed);
            if !r.balanced() {
                rows_balanced = false;
            }
        }
        let ledger_n = self.ledger.len() as u32;
        let mut batch_ok = true;
        let mut b = 0u32;
        while (b as usize) < BATCH_COUNT {
            if self.batch_subtotal(b) != expected_batch_sum(b) {
                batch_ok = false;
            }
            if self.batch_seq_strict(b).is_err() {
                batch_ok = false;
            }
            b = b.saturating_add(1);
        }
        Recon {
            asset,
            spot,
            ledger_n,
            diff_as: asset.checked_sub(spot),
            diff_al: asset.checked_sub(ledger_n),
            rows_balanced,
            batch_ok,
        }
    }

    /// 封账：聚合齐 + 三账对平全绿才许封；封后只读，重封倒退拒。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= AggState::Sealed.rank() {
            return Err(KtDiag::ERewind.val());
        }
        if self.state.rank() < AggState::Full.rank() {
            return Err(KtDiag::ENotFull.val());
        }
        if !self.reconcile().balanced() {
            return Err(KtDiag::ERecon.val());
        }
        self.state = AggState::Sealed;
        Ok(())
    }
}

// ── 确定性 LCG（种子 = SEG_BASE 派生写死，零随机红线合规） ──────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性）。
    pub fn next_u32(&mut self) -> u32 {
        // LCG 常数取自 Knuth MMIX 31 位推荐值（整数手册常用闭集）。
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 构建 / 注入面 ───────────────────────────────────────────────
/// 构建满账聚合器：旗标置位入口按入口号序全部聚合。
/// 正常流不该出错；出错即终止构造（缺项会让对平断言红）。
pub fn build_full_aggregator() -> Aggregator {
    let mut ag = Aggregator::new();
    for no in armed_entries() {
        match ag.aggregate(no) {
            Ok(_) => {}
            Err(_) => break,
        }
    }
    ag
}

/// 账行篡改（路一）：把指定入口判定行的登记值加 Δ，
/// 资产账偏移、行三元组破平——对平断言必须抓住。
pub fn tamper_registered(ag: &Aggregator, entry_no: u32, delta: u32) -> Result<Aggregator, u32> {
    if delta == 0 {
        return Err(KtDiag::ERowDiff.val());
    }
    if ag.row_of(entry_no).is_none() {
        return Err(KtDiag::ERowDiff.val());
    }
    let mut out = Aggregator::new();
    for r in ag.rows.iter() {
        let reg = if r.entry_no == entry_no {
            r.registered.saturating_add(delta)
        } else {
            r.registered
        };
        out.rows.push(AggRow {
            entry_no: r.entry_no,
            batch: r.batch,
            registered: reg,
            computed: r.computed,
            diff: reg.checked_sub(r.computed).unwrap_or(0),
        });
        out.done.push(r.entry_no);
    }
    for l in ag.ledger.iter() {
        out.ledger.push(*l);
    }
    out.state = ag.state;
    Ok(out)
}

/// 账行篡改（路二）：把指定账行的批号错位改批，
/// 批累加等式与批内序号连续性双破——对平断言必须抓住。
pub fn tamper_line(ag: &Aggregator, line_idx: usize, new_batch: u32) -> Result<Aggregator, u32> {
    if line_idx >= ag.ledger.len() || new_batch >= BATCH_COUNT as u32 {
        return Err(KtDiag::EBatch.val());
    }
    let orig = match ag.ledger.get(line_idx) {
        Some(l) => l.batch,
        None => return Err(KtDiag::EBatch.val()),
    };
    // 同批"改批"是无操作，显性拒以免制造假注入。
    if orig == new_batch {
        return Err(KtDiag::EBatch.val());
    }
    let mut out = Aggregator::new();
    for r in ag.rows.iter() {
        out.rows.push(*r);
        out.done.push(r.entry_no);
    }
    let mut i = 0usize;
    for l in ag.ledger.iter() {
        let batch = if i == line_idx { new_batch } else { l.batch };
        out.ledger.push(AggLine {
            entry_no: l.entry_no,
            batch,
            seq: l.seq,
        });
        i = i.saturating_add(1);
    }
    out.state = ag.state;
    Ok(out)
}

/// 聚合账指纹：对判定行与账行逐条卷积的确定性摘要（双跑一致判据）。
pub fn fingerprint(ag: &Aggregator) -> u32 {
    let mut h = SEG_BASE.wrapping_mul(2_654_435_761);
    for r in ag.rows.iter() {
        h = h
            .wrapping_mul(2_654_435_761)
            .wrapping_add(r.entry_no.wrapping_mul(31));
        h = h.wrapping_add(r.registered).wrapping_add(r.computed).wrapping_add(r.diff);
    }
    for l in ag.ledger.iter() {
        h = h
            .wrapping_mul(31)
            .wrapping_add(l.entry_no)
            .wrapping_add(l.batch.wrapping_mul(7))
            .wrapping_add(l.seq);
    }
    h.wrapping_add((ag.ledger.len() as u32).wrapping_mul(0x9E37))
        .wrapping_add(ag.state.rank() as u32)
}

/// 注入器一（未登记入口）：10 轮确定性注入，轮 0 用入口号 0（下界外）、
/// 轮 1 用 21（上界外）、其余 LCG 取 21..521 域，全部必须 ENOENT。
pub fn sweep_no_entry() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x11) as u64);
    let mut k = 0usize;
    while k < 10 {
        let no = match k {
            0 => 0u32,
            1 => 21u32,
            _ => 21u32 + rng.below(500),
        };
        let mut ag = Aggregator::new();
        total = total.saturating_add(1);
        match ag.aggregate(no) {
            Err(e) if e == KtDiag::ENoEntry.val() => caught = caught.saturating_add(1),
            _ => {
                if escape.is_empty() {
                    escape = format!("未登记入口路{}逃脱：入口{}未被 ENOENT 拒", k, no);
                }
            }
        }
        k = k.saturating_add(1);
    }
    (caught, total, escape)
}

/// 注入器二（重复聚合）：10 轮确定性注入，每轮对旗标置位入口
/// 二连聚合——第二回必须 EALREADY，且第一回必须成功。
pub fn sweep_double() -> (usize, usize, String) {
    let armed = armed_entries();
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut k = 0usize;
    while k < 10 {
        let no = match armed.get(k) {
            Some(&n) => n,
            None => 0,
        };
        let mut ag = Aggregator::new();
        total = total.saturating_add(1);
        let first = ag.aggregate(no);
        let second = ag.aggregate(no);
        match (first, second) {
            (Ok(_), Err(e)) if e == KtDiag::EDupAgg.val() => caught = caught.saturating_add(1),
            _ => {
                if escape.is_empty() {
                    escape = format!("重复聚合路{}逃脱：入口{}二连聚合未被 EALREADY 拒", k, no);
                }
            }
        }
        k = k.saturating_add(1);
    }
    (caught, total, escape)
}

/// 注入器三（账行篡改）：10 轮确定性注入，偶数轮改判定行登记值 +Δ、
/// 奇数轮错位账行批号——每轮对平断言必须红，绿即逃脱。
pub fn sweep_tamper() -> (usize, usize, String) {
    let full = build_full_aggregator();
    let armed = armed_entries();
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut k = 0usize;
    while k < 10 {
        let no = match armed.get(k) {
            Some(&n) => n,
            None => 1,
        };
        total = total.saturating_add(1);
        let balanced = if k % 2 == 0 {
            match tamper_registered(&full, no, 1 + k as u32) {
                Ok(t) => t.reconcile().balanced(),
                Err(_) => true,
            }
        } else {
            match tamper_line(&full, 0, 1 + ((k as u32) % 3)) {
                Ok(t) => t.reconcile().balanced(),
                Err(_) => true,
            }
        };
        if balanced {
            if escape.is_empty() {
                escape = format!("账行篡改路{}逃脱：入口{}注入后对平仍绿", k, no);
            }
        } else {
            caught = caught.saturating_add(1);
        }
        k = k.saturating_add(1);
    }
    (caught, total, escape)
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 段窗 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：全部落在 [SEG_BASE, SEG_BASE+0x40) 窗口且查表命中。
fn diag_closed_ok() -> bool {
    if KT_ALL.len() != 12 {
        return false;
    }
    for d in KT_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    // 查表闭集：任意已知码必须命中文案，未知码必须回落。
    if detail(KtDiag::ENoEntry.val()) != KtDiag::ENoEntry.detail() {
        return false;
    }
    if detail(KtDiag::EFingerprint.val()) != KtDiag::EFingerprint.detail() {
        return false;
    }
    if detail(SEG_BASE + 0x3F) == KtDiag::ENoEntry.detail() {
        return false;
    }
    true
}

/// 登记表冻结闭集守卫：20 条互异无缺号、批号落 0..4 每批 5 条、
/// 单跑条数逐条可复算、冻结总账（120/90/22/23/24/21）与复算一致。
fn table_closed_ok() -> bool {
    // 入口号闭集：恰为 1..=20 各一。
    let mut seen = [false; ENTRY_CAP];
    for e in ENTRY_TABLE.iter() {
        if e.entry_no == 0 || e.entry_no as usize > ENTRY_CAP {
            return false;
        }
        let idx = (e.entry_no - 1) as usize;
        match seen.get_mut(idx) {
            Some(slot) => {
                if *slot {
                    return false;
                }
                *slot = true;
            }
            None => return false,
        }
    }
    let mut k = 0usize;
    while k < ENTRY_CAP {
        match seen.get(k) {
            Some(true) => {}
            _ => return false,
        }
        k = k.saturating_add(1);
    }
    // 逐条复算：批号/代号/条数闭式。
    for e in ENTRY_TABLE.iter() {
        if e.batch >= BATCH_COUNT as u32 {
            return false;
        }
        if e.code.is_empty() {
            return false;
        }
        if e.rows != recomputed_rows(e.entry_no) {
            return false;
        }
        if e.rows < 4 || e.rows > 8 {
            return false;
        }
    }
    // 批规模：每批恰 5 条。
    let mut b = 0u32;
    while (b as usize) < BATCH_COUNT {
        let n = ENTRY_TABLE.iter().filter(|e| e.batch == b).count();
        if n != 5 {
            return false;
        }
        b = b.saturating_add(1);
    }
    // 冻结总账：全表合计、旗标置位合计与逐批合计。
    let mut total = 0u32;
    for e in ENTRY_TABLE.iter() {
        total = total.saturating_add(e.rows);
    }
    if total != TABLE_ROWS_TOTAL {
        return false;
    }
    let mut armed_sum = 0u32;
    let mut armed_n = 0usize;
    for e in ENTRY_TABLE.iter() {
        if e.armed {
            armed_n = armed_n.saturating_add(1);
            armed_sum = armed_sum.saturating_add(e.rows);
        }
    }
    if armed_n != ARMED_COUNT || armed_sum != ARMED_ROWS_TOTAL {
        return false;
    }
    let mut b = 0u32;
    while (b as usize) < BATCH_COUNT {
        if expected_batch_sum(b) != BATCH_ARMED_SUM.get(b as usize).copied().unwrap_or(0) {
            return false;
        }
        b = b.saturating_add(1);
    }
    true
}

/// 正常流：满账聚合 → 状态 Full → 封账 Sealed。
fn happy_path_ok() -> bool {
    let mut ag = build_full_aggregator();
    if ag.done_count() != ARMED_COUNT {
        return false;
    }
    if ag.state() != AggState::Full {
        return false;
    }
    if ag.ledger_len() as u32 != ARMED_ROWS_TOTAL {
        return false;
    }
    if ag.seal().is_err() {
        return false;
    }
    ag.state() == AggState::Sealed
}

/// 行三元组：全部判定行差异为零、登记 = 实算 = 表值；
/// 旗标未置入口不得产生判定行。
fn row_triad_ok() -> bool {
    let ag = build_full_aggregator();
    for no in armed_entries() {
        match ag.row_of(no) {
            Some(r) => {
                if r.entry_no != no || !r.balanced() {
                    return false;
                }
                match entry_of(no) {
                    Some(e) => {
                        if r.registered != e.rows {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
            None => return false,
        }
    }
    for e in ENTRY_TABLE.iter() {
        if !e.armed && ag.row_of(e.entry_no).is_some() {
            return false;
        }
    }
    true
}

/// 三账对平面：资产 = 抽检 = 聚合账条数 = 90，差异二元组全零。
fn three_account_ok() -> bool {
    let ag = build_full_aggregator();
    let rc = ag.reconcile();
    if !rc.balanced() {
        return false;
    }
    if rc.asset != ARMED_ROWS_TOTAL || rc.spot != ARMED_ROWS_TOTAL || rc.ledger_n != ARMED_ROWS_TOTAL
    {
        return false;
    }
    if rc.diff_as != Some(0) || rc.diff_al != Some(0) {
        return false;
    }
    true
}

/// 批累加等式：逐批账行数 = 冻结批合计（22/23/24/21）且批内序号连续，
/// 四批累加 = 聚合账条数 = 90。
fn batch_equation_ok() -> bool {
    let ag = build_full_aggregator();
    let mut b = 0u32;
    let mut total = 0u32;
    while (b as usize) < BATCH_COUNT {
        let sub = ag.batch_subtotal(b);
        if sub != expected_batch_sum(b) {
            return false;
        }
        if sub != BATCH_ARMED_SUM.get(b as usize).copied().unwrap_or(0) {
            return false;
        }
        if ag.batch_seq_strict(b).is_err() {
            return false;
        }
        total = total.saturating_add(sub);
        b = b.saturating_add(1);
    }
    total == ag.ledger_len() as u32 && total == ARMED_ROWS_TOTAL
}

/// 聚合账等式：单跑条数合计 = 聚合账条数，且逐入口账行数 = 其单跑条数。
fn ledger_equation_ok() -> bool {
    let ag = build_full_aggregator();
    if ag.ledger_len() as u32 != ARMED_ROWS_TOTAL {
        return false;
    }
    for r in ag.rows.iter() {
        if ag.lines_of_entry(r.entry_no) != r.registered {
            return false;
        }
    }
    let mut sum = 0u32;
    for no in armed_entries() {
        match entry_of(no) {
            Some(e) => sum = sum.saturating_add(e.rows),
            None => return false,
        }
    }
    sum == ag.ledger_len() as u32
}

/// 注入必抓总闸：三类注入器各 10 轮全数命中，无逃脱。
fn inject_all_caught_ok() -> bool {
    let (c1, t1, e1) = sweep_no_entry();
    if t1 != 10 || c1 != 10 || !e1.is_empty() {
        return false;
    }
    let (c2, t2, e2) = sweep_double();
    if t2 != 10 || c2 != 10 || !e2.is_empty() {
        return false;
    }
    let (c3, t3, e3) = sweep_tamper();
    if t3 != 10 || c3 != 10 || !e3.is_empty() {
        return false;
    }
    true
}

/// 判定次序：登记闭集最先（无论封账与否，越界一律 ENOENT），
/// 其次封账倒退、旗标（EPERM 先于 EALREADY）、重复。
fn judge_order_ok() -> bool {
    let mut ag = Aggregator::new();
    // 第一闸：登记闭集。
    if ag.aggregate(0) != Err(KtDiag::ENoEntry.val()) {
        return false;
    }
    if ag.aggregate(21) != Err(KtDiag::ENoEntry.val()) {
        return false;
    }
    // 第二闸：旗标——未置位入口从未聚合，EPERM 而非 EALREADY。
    if ag.aggregate(4) != Err(KtDiag::ENoPerm.val()) {
        return false;
    }
    // 第三闸：重复——置位入口聚合成功后再聚合，EALREADY。
    if ag.aggregate(1).is_err() {
        return false;
    }
    if ag.aggregate(1) != Err(KtDiag::EDupAgg.val()) {
        return false;
    }
    // 封账后：登记闸仍最先；越闸写路 ERewind 先于旗标/重复闸。
    let mut full = build_full_aggregator();
    if full.seal().is_err() {
        return false;
    }
    if full.aggregate(21) != Err(KtDiag::ENoEntry.val()) {
        return false;
    }
    if full.aggregate(1) != Err(KtDiag::ERewind.val()) {
        return false;
    }
    if full.aggregate(4) != Err(KtDiag::ERewind.val()) {
        return false;
    }
    true
}

/// 边界四类：入口号 0（下界外）/ 21（上界外）/ 首个旗标未置入口 4 /
/// 末条旗标置位入口 19（单跑条数 8 恰增），另验未齐封账与重封倒退。
fn boundary_ok() -> bool {
    let mut ag = Aggregator::new();
    // 边界①：下界外入口 0。
    if ag.aggregate(0) != Err(KtDiag::ENoEntry.val()) {
        return false;
    }
    // 边界②：上界外入口 21。
    if ag.aggregate(21) != Err(KtDiag::ENoEntry.val()) {
        return false;
    }
    // 边界③：首个旗标未置入口 4（旗标闸）。
    if ag.aggregate(4) != Err(KtDiag::ENoPerm.val()) {
        return false;
    }
    // 边界④：末条旗标置位入口 19，账行恰增 8 条。
    let before = ag.ledger_len();
    match ag.aggregate(19) {
        Ok(r) => {
            if r.entry_no != 19 || !r.balanced() {
                return false;
            }
        }
        Err(_) => return false,
    }
    if ag.ledger_len() != before + 8 {
        return false;
    }
    // 未齐封账拒绝。
    if ag.seal() != Err(KtDiag::ENotFull.val()) {
        return false;
    }
    // 满账封账成功，重封倒退拒绝。
    let mut full = build_full_aggregator();
    if full.seal().is_err() {
        return false;
    }
    if full.seal() != Err(KtDiag::ERewind.val()) {
        return false;
    }
    true
}

/// 容量耗尽：满账恰为定容 90/90，账外追加显性拒且账面不变；
/// 空账逐条直写至满容，第 91 条被拒。
fn capacity_ok() -> bool {
    let mut ag = build_full_aggregator();
    if ag.ledger_len() != AGG_CAP {
        return false;
    }
    if ag.push_line_direct(1, 0) != Err(KtDiag::EAggFull.val()) {
        return false;
    }
    if ag.ledger_len() != AGG_CAP {
        return false;
    }
    let mut ag2 = Aggregator::new();
    let mut k = 0usize;
    while k < AGG_CAP {
        if ag2.push_line_direct(1, 0).is_err() {
            return false;
        }
        k = k.saturating_add(1);
    }
    ag2.push_line_direct(1, 0) == Err(KtDiag::EAggFull.val())
}

/// 双跑一致指纹：同种子两次构建指纹相同、三注入器结果逐项相同、
/// 篡改体指纹必须偏离原账（指纹非恒等）。
fn dual_run_ok() -> bool {
    let a = build_full_aggregator();
    let b = build_full_aggregator();
    if fingerprint(&a) != fingerprint(&b) {
        return false;
    }
    let s1 = (sweep_no_entry(), sweep_double(), sweep_tamper());
    let s2 = (sweep_no_entry(), sweep_double(), sweep_tamper());
    if s1 != s2 {
        return false;
    }
    match tamper_registered(&a, 1, 3) {
        Ok(t) => fingerprint(&t) != fingerprint(&a),
        Err(_) => false,
    }
}

/// 完成写回语义：封账即收官且摘要非空。
fn writeback_ok() -> bool {
    let mut ag = build_full_aggregator();
    if ag.seal().is_err() {
        return false;
    }
    let summary = format!("done={} rows={} state=Sealed", ag.done_count(), ag.ledger_len());
    !summary.is_empty() && ag.state() == AggState::Sealed
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12774 自检（判据 12774 卡：UNX-F12775-J1 三账对平断言绿）。
pub fn run_unx12774_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12774");
    let ag = build_full_aggregator();
    let rc = ag.reconcile();

    s.add_dyn(
        "F12775-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12775-02 诊断码闭集",
        diag_closed_ok(),
        &format!("n={} first=0x{:05x}", KT_ALL.len(), SEG_BASE + 0x01),
    );

    s.add_dyn(
        "F12775-03 登记表冻结闭集",
        table_closed_ok(),
        &format!(
            "entries={} armed={} 总条数={} 批{}..{}",
            ENTRY_CAP,
            ARMED_COUNT,
            TABLE_ROWS_TOTAL,
            BATCH_TAGS.first().copied().unwrap_or("?"),
            BATCH_TAGS.last().copied().unwrap_or("?")
        ),
    );

    s.add_dyn(
        "F12775-04 正常流全聚合封账",
        happy_path_ok(),
        &format!("done={} rows={} state={:?}", ag.done_count(), ag.ledger_len(), ag.state()),
    );

    s.add_dyn(
        "F12775-05 行三元组差异全零",
        row_triad_ok(),
        &format!("rows={} 全零差={}", ag.row_count(), rc.rows_balanced),
    );

    s.add_dyn(
        "F12775-06 三账对平面",
        three_account_ok(),
        &format!("资产={} 抽检={} 账行={}", rc.asset, rc.spot, rc.ledger_n),
    );

    s.add_dyn(
        "F12775-07 批累加等式",
        batch_equation_ok(),
        &format!(
            "B39A={} B39B={} B39C={} B39D={}",
            ag.batch_subtotal(0),
            ag.batch_subtotal(1),
            ag.batch_subtotal(2),
            ag.batch_subtotal(3)
        ),
    );

    s.add_dyn(
        "F12775-08 聚合账等式",
        ledger_equation_ok(),
        &format!("单跑合计={} 账行={}", ARMED_ROWS_TOTAL, ag.ledger_len()),
    );

    let (c1, t1, e1) = sweep_no_entry();
    s.add_dyn(
        "F12775-09 注入一 未登记入口 10 轮全抓",
        t1 == 10 && c1 == 10 && e1.is_empty(),
        &format!("抓 {}/{} {}", c1, t1, e1),
    );

    let (c2, t2, e2) = sweep_double();
    s.add_dyn(
        "F12775-10 注入二 重复聚合 10 轮全抓",
        t2 == 10 && c2 == 10 && e2.is_empty(),
        &format!("抓 {}/{} {}", c2, t2, e2),
    );

    let (c3, t3, e3) = sweep_tamper();
    s.add_dyn(
        "F12775-11 注入三 账行篡改 10 轮全抓",
        t3 == 10 && c3 == 10 && e3.is_empty(),
        &format!("抓 {}/{} {}", c3, t3, e3),
    );

    s.add_dyn(
        "F12775-12 判定次序三闸",
        judge_order_ok(),
        "登记→封账倒退→旗标→重复 定序不可换",
    );

    s.add_dyn(
        "F12775-13 边界四类",
        boundary_ok(),
        "入口0/21 越界、旗标未置首条4、末条19聚合",
    );

    s.add_dyn(
        "F12775-14 容量耗尽",
        capacity_ok(),
        &format!("cap={} 满={} 超=拒", AGG_CAP, ag.ledger_len()),
    );

    s.add_dyn(
        "F12775-15 双跑一致指纹",
        dual_run_ok(),
        "两次构建+三注入器逐项一致，篡改体指纹偏离",
    );

    s.add_dyn(
        "F12775-16 完成写回语义",
        writeback_ok(),
        "封账即收官且摘要非空",
    );

    let n_before = s.len();
    s.add_dyn(
        "F12775-17 判据收口",
        n_before == 16,
        &format!(
            "UNX-F12775-J1 共 1 条判据，检查项 {}+1 条全绿收口",
            n_before
        ),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 段窗 [0x80000, 0x831C0]。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x82B80);
    }

    #[test]
    fn diag_codes_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中、未知码回落。
        assert_eq!(KT_ALL.len(), 12);
        let mut i = 0usize;
        while i < KT_ALL.len() {
            let vi = KT_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < KT_ALL.len() {
                let vj = KT_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert_eq!(detail(KtDiag::ENoEntry.val()), KtDiag::ENoEntry.detail());
        assert_eq!(
            detail(KtDiag::EFingerprint.val()),
            KtDiag::EFingerprint.detail()
        );
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
    }

    #[test]
    fn entry_table_frozen() {
        // 登记表冻结闭集：20 条互异、旗标 15 置位、冻结总账逐批可复算。
        assert!(table_closed_ok());
        assert_eq!(ENTRY_TABLE.len(), ENTRY_CAP);
        assert_eq!(armed_entries().len(), ARMED_COUNT);
        assert_eq!(ENTRY_TABLE.first().map(|e| e.entry_no), Some(1));
        assert_eq!(ENTRY_TABLE.last().map(|e| e.entry_no), Some(20));
        assert_eq!(expected_batch_sum(0), 22);
        assert_eq!(expected_batch_sum(1), 23);
        assert_eq!(expected_batch_sum(2), 24);
        assert_eq!(expected_batch_sum(3), 21);
        assert_eq!(TABLE_ROWS_TOTAL, 120);
        assert_eq!(ARMED_ROWS_TOTAL, 90);
        // 实算值独立复算：首条 5、末条 4、逐条与表值一致。
        assert_eq!(recomputed_rows(1), 5);
        assert_eq!(recomputed_rows(20), 4);
        let mut no = 1u32;
        while no <= 20 {
            let e = entry_of(no);
            assert!(e.is_some(), "入口{}缺登记", no);
            match e {
                Some(e) => assert_eq!(e.rows, recomputed_rows(no)),
                None => return,
            }
            no += 1;
        }
    }

    #[test]
    fn happy_path_full_seal() {
        // 正常流：满账聚合 → Full → 三账对平 → 封账 Sealed。
        let mut ag = build_full_aggregator();
        assert_eq!(ag.done_count(), ARMED_COUNT);
        assert_eq!(ag.state(), AggState::Full);
        assert_eq!(ag.ledger_len() as u32, ARMED_ROWS_TOTAL);
        assert!(three_account_ok());
        assert!(row_triad_ok());
        assert!(ledger_equation_ok());
        assert!(batch_equation_ok());
        assert!(ag.seal().is_ok());
        assert_eq!(ag.state(), AggState::Sealed);
    }

    #[test]
    fn injections_all_caught() {
        // 三类注入各 10 轮全数命中，无逃脱。
        let (c1, t1, e1) = sweep_no_entry();
        assert_eq!(t1, 10);
        assert_eq!(c1, 10, "{}", e1);
        assert!(e1.is_empty());
        let (c2, t2, e2) = sweep_double();
        assert_eq!(t2, 10);
        assert_eq!(c2, 10, "{}", e2);
        assert!(e2.is_empty());
        let (c3, t3, e3) = sweep_tamper();
        assert_eq!(t3, 10);
        assert_eq!(c3, 10, "{}", e3);
        assert!(e3.is_empty());
        assert!(inject_all_caught_ok());
    }

    #[test]
    fn judge_order_gates() {
        // 判定次序：登记→旗标→重复；封账后倒退闸居旗标/重复之前。
        assert!(judge_order_ok());
        let mut ag = Aggregator::new();
        assert_eq!(ag.aggregate(4), Err(KtDiag::ENoPerm.val()));
        assert_eq!(ag.aggregate(4), Err(KtDiag::ENoPerm.val()));
        assert!(ag.aggregate(2).is_ok());
        assert_eq!(ag.aggregate(2), Err(KtDiag::EDupAgg.val()));
        assert_eq!(ag.aggregate(0), Err(KtDiag::ENoEntry.val()));
        assert_eq!(ag.aggregate(21), Err(KtDiag::ENoEntry.val()));
    }

    #[test]
    fn boundary_four_classes() {
        // 四类边界 + 未齐封账 + 重封倒退。
        assert!(boundary_ok());
        let mut ag = Aggregator::new();
        assert_eq!(ag.seal(), Err(KtDiag::ENotFull.val()));
        let _ = ag.aggregate(1);
        assert_eq!(ag.seal(), Err(KtDiag::ENotFull.val()));
        // 末条 19：登记值 8、差异 0、账行恰增 8。
        let mut ag2 = Aggregator::new();
        let r = ag2.aggregate(19);
        match r {
            Ok(row) => {
                assert_eq!(row.registered, 8);
                assert_eq!(row.computed, 8);
                assert_eq!(row.diff, 0);
            }
            Err(e) => panic!("入口19聚合失败：0x{:05x}", e),
        }
        assert_eq!(ag2.ledger_len(), 8);
    }

    #[test]
    fn capacity_exhausted() {
        // 容量耗尽：满账 90/90 后账外追加显性拒；批号/入口闭集闸前置。
        assert!(capacity_ok());
        let mut ag = build_full_aggregator();
        assert_eq!(ag.ledger_len(), AGG_CAP);
        assert_eq!(ag.push_line_direct(1, 0), Err(KtDiag::EAggFull.val()));
        assert_eq!(ag.ledger_len(), AGG_CAP);
        assert_eq!(ag.push_line_direct(1, 9), Err(KtDiag::EBatch.val()));
        assert_eq!(ag.push_line_direct(0, 0), Err(KtDiag::ENoEntry.val()));
    }

    #[test]
    fn dual_run_identical() {
        // 双跑一致：两次构建逐账行相同、指纹相同、注入器逐项相同。
        assert!(dual_run_ok());
        let a = build_full_aggregator();
        let b = build_full_aggregator();
        assert_eq!(fingerprint(&a), fingerprint(&b));
        assert_eq!(a.ledger_len(), b.ledger_len());
        let mut i = 0usize;
        while i < a.ledger_len() {
            assert_eq!(a.line_at(i), b.line_at(i));
            i += 1;
        }
        assert_eq!(sweep_no_entry(), sweep_no_entry());
        assert_eq!(sweep_double(), sweep_double());
        assert_eq!(sweep_tamper(), sweep_tamper());
        // 篡改体指纹必须偏离原账。
        match tamper_registered(&a, 2, 5) {
            Ok(t) => assert_ne!(fingerprint(&t), fingerprint(&a)),
            Err(e) => panic!("篡改构造失败：0x{:05x}", e),
        }
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条、无红项。
        let s = run_unx12774_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
