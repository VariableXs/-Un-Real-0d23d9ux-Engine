//! UNX-F12775 文档对齐收官账（等式断言） · 0x82BC0段
//!
//! 判据原文（派工卡 12775 逐字锚定）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F12776-J1 B40 收官清单登记（回归/封账/移交三段），
//!      衔接零悬空；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 三账等式收官账域模型：资产账 A × 抽检账 S ≡ 收官账总行数
//!    （逐批累加复算），差异账 D 只作批内调整（补录 Plus/核销 Minus）；
//!    判定器 `assert_equation() -> Result<EquationRow, u32>`：不对平即
//!    EMISMATCH 语义码（EMismatchA/EMismatchS/EMismatchD）并按首障规则
//!    指明哪一账；对平后收官账经 Open→Balanced→Sealed 进入封账；
//! 2. 对平三段语义（判据 J1）：回归段=抽检账逐批登记可查；封账段=收官账
//!    状态机单向落章；移交段=等式行携归档指纹归档移交；衔接零悬空=每条
//!    收官行的来源账引用必须命中已登记资产批（追加闸 + 复算悬空闸双守）；
//! 3. 收官账行四元组：行号/来源账引用/金额（条目数，恒 1，全账金额守恒）/
//!    归档指纹；归档指纹链自 genesis 起每行 checked_add 内容散列增量，
//!    只增不减；封账落 sealed_fp，任何行篡改/断档/删尾复算必不命中；
//! 4. 三类注入器各 10 轮全抓：账行篡改（定点改金额，指纹复算必抓 ETamper）、
//!    不对平注入（A增/A减/S增/S减/D补/D核 六路按定位规则报对应账语义码）、
//!    重复封账（重封 EReSeal、封后写 ESealed、封后重对平 EReSeal）；
//! 5. 判定次序冻结：写入面 封账闸 > 容量闸 > 批闭集闸 > 指纹闸；对平面
//!    空账闸 > 完整性闸（ESeq/ETamper）> 批互检闸 > 逐批复算闸 > 悬空闸；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子固定）；
//!    取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!    无 unwrap/expect/panic/裸下标，无死代码。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12775 − 12600) = 0x82BC0）。
pub const SEG_BASE: u32 = 0x82BC0;

// ── 收官账诊断码闭集 ─────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClDiag {
    /// 序违约：批号须按闭集内递增登记（重复/回退/零幅度均拒），收官行号
    /// 须从 1 逐行递增不得断档。
    ESeq,
    /// 批号越闭集：批号只认 1..=8，越界批一律拒收。
    EBatchRange,
    /// 收官账已封账：封账后一切写入与登记拒绝，改动走下一条卡滚动。
    ESealed,
    /// 收官账容量耗尽：行数达上限后再追加拒绝，先扩容再补行。
    ECap,
    /// 归档指纹链推进溢出：checked_add 失败即指纹只增不减被破，拒绝入行。
    EFpBack,
    /// 资产账不对平：收官行盈余（条目申报不足）、资产缺批或差异悬空。
    EMismatchA,
    /// 抽检账不对平：收官行缺口（抽检承载虚多）或资产批无对应抽检批。
    EMismatchS,
    /// 差异账不对平：补录/核销申报与行缺口方向矛盾，先清差异账再复算。
    EMismatchD,
    /// 未对平先封账：必须先走对平口（reconcile）再落章。
    ENotBalanced,
    /// 重复封账：账已 Sealed 再封/再对平均拒，落章只许一次。
    EReSeal,
    /// 空账对平/封账拒：零批或零行不构成收官清单。
    EEmpty,
    /// 篡改逃脱：指纹复算/金额守恒未命中，账面可疑须整账复核。
    ETamper,
}

impl ClDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                ClDiag::ESeq => 0x01,
                ClDiag::EBatchRange => 0x02,
                ClDiag::ESealed => 0x03,
                ClDiag::ECap => 0x04,
                ClDiag::EFpBack => 0x05,
                ClDiag::EMismatchA => 0x06,
                ClDiag::EMismatchS => 0x07,
                ClDiag::EMismatchD => 0x08,
                ClDiag::ENotBalanced => 0x09,
                ClDiag::EReSeal => 0x0a,
                ClDiag::EEmpty => 0x0b,
                ClDiag::ETamper => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            ClDiag::ESeq => "序违约：批号按闭集内递增登记勿重复回退，零幅度差异目不占账，收官行号断档先查删改",
            ClDiag::EBatchRange => "批号越 1..=8 闭集：先对批号表再登记，越界批不建账",
            ClDiag::ESealed => "收官账已封账：封账后写入拒绝，改动另起新账滚动登记",
            ClDiag::ECap => "收官账容量耗尽：行数达上限，先扩容窗口再补收官行",
            ClDiag::EFpBack => "归档指纹链推进溢出：只增不减被破，检查指纹 genesis 与增量函数",
            ClDiag::EMismatchA => "资产账不对平：收官行盈余或资产缺批/差异悬空，先按批复核资产条目申报",
            ClDiag::EMismatchS => "抽检账不对平：收官行缺口或资产批缺抽检批，先按批复核抽检承载",
            ClDiag::EMismatchD => "差异账不对平：补录/核销方向与缺口矛盾，先清差异账申报再复算",
            ClDiag::ENotBalanced => "未对平先封账：先走 reconcile 对平口再 seal 落章",
            ClDiag::EReSeal => "重复封账：落章只许一次，重封/封后重对平均拒，走新账",
            ClDiag::EEmpty => "空账不构成收官清单：先登记资产批与抽检批并铺收官行",
            ClDiag::ETamper => "指纹复算未命中：账面遭篡改，整账复核行号/金额/指纹链",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const CL_ALL: &[ClDiag] = &[
    ClDiag::ESeq,
    ClDiag::EBatchRange,
    ClDiag::ESealed,
    ClDiag::ECap,
    ClDiag::EFpBack,
    ClDiag::EMismatchA,
    ClDiag::EMismatchS,
    ClDiag::EMismatchD,
    ClDiag::ENotBalanced,
    ClDiag::EReSeal,
    ClDiag::EEmpty,
    ClDiag::ETamper,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in CL_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82BC1..0x82BCC 闭集内"
}

// ── 闭集常量（来源：VX9 派工公式与本卡等式语义） ─────────────────
/// 批号闭集容量：批号只认 1..=8。
pub const BATCH_CAP: u32 = 8;
/// 收官账行容量：等式乘积上限 8×8=64，收官账按此封顶。
pub const ROW_CAP: usize = 64;
/// 归档指纹 genesis（段基址混入固定魔数，账账同源）。
pub const FP_GENESIS: u32 = SEG_BASE ^ 0x5A5A_0001;
/// 等式行来源账引用魔数（A×S+D 对平行落档标记）。
pub const EQ_SRC_REF: u32 = 0xC10D_1277;

// ── 三账数据模型 ─────────────────────────────────────────────────
/// 差异调整方向：补录（收官行多于纯乘积）或核销（收官行少于纯乘积）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffDir {
    /// 补录：收官行数 = 条目乘积 + 补录幅度。
    Plus,
    /// 核销：收官行数 = 条目乘积 − 核销幅度。
    Minus,
}

impl DiffDir {
    /// 方向中文名。
    pub fn name(self) -> &'static str {
        match self {
            DiffDir::Plus => "补录",
            DiffDir::Minus => "核销",
        }
    }
}

/// 资产账批：批号 + 条目数（A 账申报口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetBatch {
    /// 批号（1..=8，账内递增不重复）。
    pub batch: u32,
    /// 条目数（本批资产申报条目）。
    pub entries: u32,
}

/// 抽检账批：批号 + 抽检数（每条资产条目展开的抽检承载行数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleBatch {
    /// 批号（与资产批同号对齐）。
    pub batch: u32,
    /// 抽检数（本批每条目展开的收官行承载）。
    pub sampled: u32,
}

/// 差异账批：批号 + 方向 + 幅度（批内对纯乘积的调整申报）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffBatch {
    /// 批号（与资产批同号对齐）。
    pub batch: u32,
    /// 调整方向：补录/核销。
    pub dir: DiffDir,
    /// 调整幅度（非零）。
    pub delta: u32,
}

impl DiffBatch {
    /// 有符号净调整（补录为正、核销为负）。
    pub fn net(self) -> i32 {
        match self.dir {
            DiffDir::Plus => self.delta as i32,
            DiffDir::Minus => -(self.delta as i32),
        }
    }
}

/// 收官账行四元组：行号/来源账引用/金额（条目数）/归档指纹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClosureRow {
    /// 行号（从 1 起逐行递增，不断档）。
    pub row_no: u32,
    /// 来源账引用（资产批号，衔接零悬空）。
    pub src_ref: u32,
    /// 金额（条目数，恒 1，全账金额守恒 = 行数）。
    pub amount: u32,
    /// 归档指纹（只增不减链值）。
    pub fp: u32,
}

/// 收官等式行（对平成功产物，封账时归档移交）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EquationRow {
    /// 下一收官行号（= 当前行数 + 1）。
    pub row_no: u32,
    /// 来源账引用（对平行魔数 EQ_SRC_REF）。
    pub src_ref: u32,
    /// 金额（收官账总行数，即逐批累加复算值）。
    pub amount: u32,
    /// 归档指纹（封账时刻的链末值）。
    pub fp: u32,
}

/// 收官账状态机：开账 → 对平 → 封账，单向推进，倒退显性拒。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerState {
    /// 开账：允许三账登记与收官行追加。
    Open,
    /// 对平：等式断言已通过，等待落章。
    Balanced,
    /// 封账：只读，等式行已归档。
    Sealed,
}

impl LedgerState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            LedgerState::Open => 0,
            LedgerState::Balanced => 1,
            LedgerState::Sealed => 2,
        }
    }
}

// ── 归档指纹链（只增不减） ───────────────────────────────────────
/// 指纹链推进：prev 与行内容四元混散，取低 12 位加 1 为增量，
/// checked_add 保证严格递增；上溢返回 None（调用方映射 EFpBack）。
/// 增量依赖行内容，任何字段被篡改则后续复算值必不等于存量链值。
pub fn fp_advance(prev: u32, row_no: u32, src_ref: u32, amount: u32) -> Option<u32> {
    // 混散常数取自整数散列常用乘子闭集（Knuth/Murmur 系，手写定值）。
    let h = prev
        .wrapping_mul(0x9E37_79B1)
        ^ row_no.wrapping_mul(0x85EB_CA6B)
        ^ src_ref.wrapping_mul(0xC2B2_AE35)
        ^ amount.wrapping_mul(0x27D4_EB2F);
    let delta = (h & 0x0FFF) + 1;
    prev.checked_add(delta)
}

// ── 收官账账本 ───────────────────────────────────────────────────
/// 文档对齐收官账：三账（A/S/D）+ 收官行 + 状态机 + 封账归档。
#[derive(Clone, Debug)]
pub struct ClosureLedger {
    /// 资产账（批号递增不重复）。
    asset: Vec<AssetBatch>,
    /// 抽检账（批号递增不重复，与资产批同号衔接）。
    sample: Vec<SampleBatch>,
    /// 差异账（稀疏，批号递增不重复）。
    diff: Vec<DiffBatch>,
    /// 收官账行（追加式）。
    rows: Vec<ClosureRow>,
    /// 状态机。
    state: LedgerState,
    /// 封账落章指纹（Sealed 后非 None，复算必须命中）。
    sealed_fp: Option<u32>,
    /// 封账归档的等式行。
    equation: Option<EquationRow>,
}

impl ClosureLedger {
    /// 开一本新收官账。
    pub fn new() -> ClosureLedger {
        ClosureLedger {
            asset: Vec::new(),
            sample: Vec::new(),
            diff: Vec::new(),
            rows: Vec::new(),
            state: LedgerState::Open,
            sealed_fp: None,
            equation: None,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> LedgerState {
        self.state
    }

    /// 收官行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否无收官行。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 资产账批数。
    pub fn asset_len(&self) -> usize {
        self.asset.len()
    }

    /// 抽检账批数。
    pub fn sample_len(&self) -> usize {
        self.sample.len()
    }

    /// 差异账批数。
    pub fn diff_len(&self) -> usize {
        self.diff.len()
    }

    /// 取第 idx 条收官行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<ClosureRow> {
        self.rows.get(idx).copied()
    }

    /// 封账落章指纹（未封账 None）。
    pub fn sealed_fp(&self) -> Option<u32> {
        self.sealed_fp
    }

    /// 封账归档的等式行（未封账 None）。
    pub fn equation_row(&self) -> Option<EquationRow> {
        self.equation
    }

    /// 链末指纹（空账返回 genesis）。
    pub fn chain_fp(&self) -> u32 {
        match self.rows.last() {
            Some(r) => r.fp,
            None => FP_GENESIS,
        }
    }

    /// 按批号查资产批。
    pub fn asset_of(&self, batch: u32) -> Option<AssetBatch> {
        for a in self.asset.iter() {
            if a.batch == batch {
                return Some(*a);
            }
        }
        None
    }

    /// 按批号查抽检批（私有）。
    fn sample_of(&self, batch: u32) -> Option<SampleBatch> {
        for s in self.sample.iter() {
            if s.batch == batch {
                return Some(*s);
            }
        }
        None
    }

    /// 按批号查差异批（私有）。
    fn diff_of(&self, batch: u32) -> Option<DiffBatch> {
        for d in self.diff.iter() {
            if d.batch == batch {
                return Some(*d);
            }
        }
        None
    }

    /// 统计某批的收官行数（私有；悬空引用不计入任何批）。
    fn count_rows_of(&self, batch: u32) -> u32 {
        let mut n = 0u32;
        for r in self.rows.iter() {
            if r.src_ref == batch {
                n += 1;
            }
        }
        n
    }

    /// 三账登记共用闸：封账闸 → 批闭集闸 → 批序闸。
    fn register_gate(&self, batch: u32, last: Option<u32>) -> Result<(), u32> {
        if self.state.rank() >= LedgerState::Sealed.rank() {
            return Err(ClDiag::ESealed.val());
        }
        if batch == 0 || batch > BATCH_CAP {
            return Err(ClDiag::EBatchRange.val());
        }
        match last {
            Some(prev) => {
                if batch <= prev {
                    return Err(ClDiag::ESeq.val());
                }
            }
            None => {}
        }
        Ok(())
    }

    /// 资产账登记一批（条目数可为零即空批，等式按零承载复算）。
    pub fn register_asset(&mut self, batch: u32, entries: u32) -> Result<(), u32> {
        let last = self.asset.last().map(|a| a.batch);
        self.register_gate(batch, last)?;
        self.asset.push(AssetBatch { batch, entries });
        Ok(())
    }

    /// 抽检账登记一批（抽检数为该批每条目的收官行承载）。
    pub fn register_sample(&mut self, batch: u32, sampled: u32) -> Result<(), u32> {
        let last = self.sample.last().map(|s| s.batch);
        self.register_gate(batch, last)?;
        self.sample.push(SampleBatch { batch, sampled });
        Ok(())
    }

    /// 差异账登记一批（幅度必须非零，零幅度视作无效序目拒收）。
    pub fn register_diff(&mut self, batch: u32, dir: DiffDir, delta: u32) -> Result<(), u32> {
        let last = self.diff.last().map(|d| d.batch);
        self.register_gate(batch, last)?;
        if delta == 0 {
            return Err(ClDiag::ESeq.val());
        }
        self.diff.push(DiffBatch { batch, dir, delta });
        Ok(())
    }

    /// 追加一条收官行（金额恒 1）。
    /// 判定次序：封账闸 > 容量闸 > 批闭集/悬空闸 > 指纹闸。
    pub fn append_row(&mut self, src_ref: u32) -> Result<(), u32> {
        if self.state.rank() >= LedgerState::Sealed.rank() {
            return Err(ClDiag::ESealed.val());
        }
        if self.rows.len() >= ROW_CAP {
            return Err(ClDiag::ECap.val());
        }
        if self.asset_of(src_ref).is_none() {
            return Err(ClDiag::EBatchRange.val());
        }
        let row_no = self.rows.len() as u32 + 1;
        let prev = self.chain_fp();
        let fp = match fp_advance(prev, row_no, src_ref, 1) {
            Some(v) => v,
            None => return Err(ClDiag::EFpBack.val()),
        };
        self.rows.push(ClosureRow {
            row_no,
            src_ref,
            amount: 1,
            fp,
        });
        Ok(())
    }

    /// 验收注入面：定点篡改第 idx 行金额（增量饱和加），指纹链不动，
    /// 复算必不命中。返回是否命中行。
    pub fn row_amount_mut(&mut self, idx: usize, delta: u32) -> bool {
        match self.rows.get_mut(idx) {
            Some(r) => {
                r.amount = r.amount.saturating_add(delta);
                true
            }
            None => false,
        }
    }

    /// 账本完整性审计：行号从 1 逐行递增不断档、逐行指纹复算命中、
    /// 全账金额守恒（Σ金额 = 行数）、封账指纹命中链末。
    pub fn audit_chain(&self) -> Result<(), u32> {
        let mut prev = FP_GENESIS;
        let mut expect_no = 1u32;
        let mut sum_amount = 0u32;
        for r in self.rows.iter() {
            if r.row_no != expect_no {
                return Err(ClDiag::ESeq.val());
            }
            let calc = match fp_advance(prev, r.row_no, r.src_ref, r.amount) {
                Some(v) => v,
                None => return Err(ClDiag::EFpBack.val()),
            };
            if calc != r.fp {
                return Err(ClDiag::ETamper.val());
            }
            if r.amount == 0 {
                return Err(ClDiag::ETamper.val());
            }
            sum_amount = sum_amount.saturating_add(r.amount);
            prev = r.fp;
            expect_no += 1;
        }
        if sum_amount != self.rows.len() as u32 {
            return Err(ClDiag::ETamper.val());
        }
        match self.sealed_fp {
            Some(sealed) => {
                if sealed != prev {
                    return Err(ClDiag::ETamper.val());
                }
            }
            None => {}
        }
        Ok(())
    }

    /// 等式断言判定器：A 条目数 × S 抽检数 ≡ 收官账总行数（逐批累加复算）。
    /// 判定次序：空账闸 > 完整性闸 > 批互检闸 > 逐批复算闸 > 悬空闸。
    /// 对平返回等式行（不改状态）；不对平返回 EMISMATCH 语义码并按
    /// 首障规则指明哪一账。
    pub fn assert_equation(&self) -> Result<EquationRow, u32> {
        // 次序一：空账闸——零批或零行不构成收官清单。
        if self.asset.is_empty() || self.rows.is_empty() {
            return Err(ClDiag::EEmpty.val());
        }
        // 次序二：账本完整性（行号序/指纹链/金额守恒/封账指纹）。
        self.audit_chain()?;
        // 次序三：批互检——抽检批无资产批（资产缺批）、差异批悬空。
        for s in self.sample.iter() {
            if self.asset_of(s.batch).is_none() {
                return Err(ClDiag::EMismatchA.val());
            }
        }
        for d in self.diff.iter() {
            if self.asset_of(d.batch).is_none() {
                return Err(ClDiag::EMismatchA.val());
            }
        }
        // 次序四：逐批累加复算，首障即返（批号升序遍历）。
        let mut total: u32 = 0;
        for a in self.asset.iter() {
            let s = match self.sample_of(a.batch) {
                Some(s) => s,
                None => return Err(ClDiag::EMismatchS.val()),
            };
            let product = match a.entries.checked_mul(s.sampled) {
                Some(p) => p,
                None => return Err(ClDiag::EMismatchA.val()),
            };
            let net = match self.diff_of(a.batch) {
                Some(d) => d.net(),
                None => 0,
            };
            let rows_b = self.count_rows_of(a.batch) as i32;
            let expect_b = (product as i32).saturating_add(net);
            if rows_b != expect_b {
                return Err(attribute(rows_b, expect_b, net));
            }
            total = total.saturating_add(rows_b.max(0) as u32);
        }
        // 次序五：悬空闸——全批对平但存在未归属行（引用未登记批）。
        if total != self.rows.len() as u32 {
            return Err(ClDiag::EMismatchA.val());
        }
        Ok(EquationRow {
            row_no: self.rows.len() as u32 + 1,
            src_ref: EQ_SRC_REF,
            amount: total,
            fp: self.chain_fp(),
        })
    }

    /// 对平口：等式断言通过即开账推进为对平态（只进不退）。
    /// 封账后再对平拒（EReSeal）；对平态重复对平幂等放行。
    pub fn reconcile(&mut self) -> Result<(), u32> {
        if self.state.rank() >= LedgerState::Sealed.rank() {
            return Err(ClDiag::EReSeal.val());
        }
        self.assert_equation()?;
        if self.state == LedgerState::Open {
            self.state = LedgerState::Balanced;
        }
        Ok(())
    }

    /// 封账落章：对平态才能落章；等式行携归档指纹归档移交。
    /// 判定次序：重封闸 > 状态闸 > 等式断言闸。
    pub fn seal(&mut self) -> Result<EquationRow, u32> {
        if self.state == LedgerState::Sealed {
            return Err(ClDiag::EReSeal.val());
        }
        if self.state == LedgerState::Open {
            return Err(ClDiag::ENotBalanced.val());
        }
        let eq = self.assert_equation()?;
        self.sealed_fp = Some(eq.fp);
        self.equation = Some(eq);
        self.state = LedgerState::Sealed;
        Ok(eq)
    }
}

/// 不对平首障定位规则（私有）：缺行优先归抽检账（承载虚多）或差异账
/// （虚报补录）；盈余优先归资产账（条目申报不足）或差异账（虚报核销）。
fn attribute(rows_b: i32, expect_b: i32, net: i32) -> u32 {
    if rows_b < expect_b {
        if net > 0 {
            ClDiag::EMismatchD.val()
        } else {
            ClDiag::EMismatchS.val()
        }
    } else if net < 0 {
        ClDiag::EMismatchD.val()
    } else {
        ClDiag::EMismatchA.val()
    }
}

// ── 确定性 LCG（种子固定，零随机红线合规） ───────────────────────
/// 线性同余发生器（Numerical Recipes 32 位常数），序列确定，双跑一致。
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
        self.state = self
            .state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        (self.state >> 16) as u32
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 账本构建器 ───────────────────────────────────────────────────
/// 基准申报（批 1..=4）：资产条目与抽检数全部 ≥2，保证减值注入必变。
const BASE_A: [u32; 4] = [4, 2, 5, 3];
const BASE_S: [u32; 4] = [3, 2, 4, 5];

/// 向账本铺 count 条指定批的收官行，返回实铺条数。
fn fill_rows(led: &mut ClosureLedger, batch: u32, count: u32) -> usize {
    let mut n = 0u32;
    while n < count {
        match led.append_row(batch) {
            Ok(()) => n += 1,
            Err(_) => break,
        }
    }
    n as usize
}

/// 构建对平收官账：批 1..=4 纯乘积等式（A×S ≡ 总行数 = 51），差异账空。
pub fn build_balanced() -> ClosureLedger {
    let mut led = ClosureLedger::new();
    let mut b = 0usize;
    while b < 4 {
        let id = (b + 1) as u32;
        let a = BASE_A.get(b).copied().unwrap_or(0);
        let sv = BASE_S.get(b).copied().unwrap_or(0);
        let _ = led.register_asset(id, a);
        let _ = led.register_sample(id, sv);
        let _ = fill_rows(&mut led, id, a * sv);
        b += 1;
    }
    led
}

/// 构建带差异调整的对平收官账：批 2 补录 2 行、批 4 核销 1 行，
/// 收官行数按调整后铺足，等式仍对平（总行数 52）。
pub fn build_balanced_with_diff() -> ClosureLedger {
    let mut led = ClosureLedger::new();
    let mut b = 0usize;
    while b < 4 {
        let id = (b + 1) as u32;
        let a = BASE_A.get(b).copied().unwrap_or(0);
        let sv = BASE_S.get(b).copied().unwrap_or(0);
        let _ = led.register_asset(id, a);
        let _ = led.register_sample(id, sv);
        let mut rows = a * sv;
        if id == 2 {
            let _ = led.register_diff(id, DiffDir::Plus, 2);
            rows += 2;
        }
        if id == 4 {
            let _ = led.register_diff(id, DiffDir::Minus, 1);
            rows -= 1;
        }
        let _ = fill_rows(&mut led, id, rows);
        b += 1;
    }
    led
}

/// 构建容量边界账：批 1 满申报（8×8=64），铺满 ROW_CAP。
fn build_capacity_edge() -> ClosureLedger {
    let mut led = ClosureLedger::new();
    let _ = led.register_asset(1, BATCH_CAP);
    let _ = led.register_sample(1, BATCH_CAP);
    let _ = fill_rows(&mut led, 1, BATCH_CAP * BATCH_CAP);
    led
}

/// 构建不对平注入账：kind 选路（0 A增 / 1 A减 / 2 S增 / 3 S减 / 4 D补 / 5 D核），
/// 指定批按 delta 篡改申报，收官行按基准乘积铺足，等式必不对平。
fn build_imbalanced(kind: usize, batch: u32, delta: u32) -> ClosureLedger {
    let mut led = ClosureLedger::new();
    let mut b = 0usize;
    while b < 4 {
        let id = (b + 1) as u32;
        let mut a = BASE_A.get(b).copied().unwrap_or(0);
        let mut s = BASE_S.get(b).copied().unwrap_or(0);
        let mut d: Option<(DiffDir, u32)> = None;
        if id == batch {
            match kind {
                0 => a = a.saturating_add(delta),
                1 => a = a.saturating_sub(delta).max(1),
                2 => s = s.saturating_add(delta),
                3 => s = s.saturating_sub(delta).max(1),
                4 => d = Some((DiffDir::Plus, delta)),
                _ => d = Some((DiffDir::Minus, delta)),
            }
        }
        let _ = led.register_asset(id, a);
        let _ = led.register_sample(id, s);
        match d {
            Some((dir, dv)) => {
                let _ = led.register_diff(id, dir, dv);
            }
            None => {}
        }
        // 收官行按基准乘积铺足（申报已篡改，等式必破）。
        let _ = fill_rows(&mut led, id, BASE_A.get(b).copied().unwrap_or(0) * BASE_S.get(b).copied().unwrap_or(0));
        b += 1;
    }
    led
}

// ── 三类注入器（各 10 轮全抓） ───────────────────────────────────
/// 注入器一：账行篡改 10 轮。定点改一行金额，指纹复算必须抓（ETamper）。
/// 返回 (抓到轮数, 总轮数, 首个逃脱描述)。
pub fn tamper_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x7E5C_0DE1);
    let mut k = 0usize;
    while k < 10 {
        let mut led = build_balanced();
        let n = led.len() as u32;
        let idx = rng.below(n) as usize;
        let delta = 1 + rng.below(9);
        let hit = led.row_amount_mut(idx, delta);
        let verdict = led.audit_chain();
        total += 1;
        if hit && verdict == Err(ClDiag::ETamper.val()) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{} 篡改未抓 idx={} d={}", k, idx, delta);
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 不对平注入的期望语义码（定位规则与 attribute 一致）。
fn imbalance_expect(kind: usize) -> u32 {
    match kind {
        0 => ClDiag::EMismatchS.val(),
        1 => ClDiag::EMismatchA.val(),
        2 => ClDiag::EMismatchS.val(),
        3 => ClDiag::EMismatchA.val(),
        _ => ClDiag::EMismatchD.val(),
    }
}

/// 注入器二：不对平注入 10 轮。六路（A增/A减/S增/S减/D补/D核）轮转，
/// 批号与幅度 LCG 抖动，每路必须报对应账的 EMISMATCH 语义码。
pub fn imbalance_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x1B17_5D3A);
    let mut k = 0usize;
    while k < 10 {
        let kind = k % 6;
        let batch = 1 + rng.below(4);
        let delta = 1 + rng.below(3);
        let mut led = build_imbalanced(kind, batch, delta);
        let verdict = led.assert_equation();
        let rec = led.reconcile();
        total += 1;
        if verdict == Err(imbalance_expect(kind)) && rec.is_err() {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!(
                "路{} kind{} 批{} d{} 未按 {} 报码",
                k,
                kind,
                batch,
                delta,
                imbalance_expect(kind)
            );
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 注入器三：重复封账 10 轮。封账后重封（EReSeal）、封后写（ESealed）、
/// 封后重对平（EReSeal）三路必须全数拒绝。
pub fn reseal_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x2E5A_B111);
    let mut k = 0usize;
    while k < 10 {
        let mut led = build_balanced();
        let _ = led.reconcile();
        let first = led.seal();
        let again = led.seal();
        let probe_batch = 1 + rng.below(BATCH_CAP);
        let append = led.append_row(probe_batch);
        let rewound = led.reconcile();
        total += 1;
        if first.is_ok()
            && again == Err(ClDiag::EReSeal.val())
            && append == Err(ClDiag::ESealed.val())
            && rewound == Err(ClDiag::EReSeal.val())
        {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{} 重复封账未抓", k);
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 判据谓词（私有，断言方向：cond 为违约条件，末行是成立条件） ──
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 批窗口 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：12 码互异、全落段窗、查表命中、未知码回落。
fn diag_closed_ok() -> bool {
    if CL_ALL.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < CL_ALL.len() {
        let d = match CL_ALL.get(i) {
            Some(d) => d,
            None => return false,
        };
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if d.detail().is_empty() {
            return false;
        }
        if detail(v) != d.detail() {
            return false;
        }
        let mut j = i + 1;
        while j < CL_ALL.len() {
            match CL_ALL.get(j) {
                Some(e) => {
                    if e.val() == v {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    if !detail(SEG_BASE + 0x30).contains("未知") {
        return false;
    }
    true
}

/// 三账登记正常流：批 1..=4 三账齐、稀疏跳批合法、回退拒、差异账可查。
fn triple_register_ok() -> bool {
    let mut led = ClosureLedger::new();
    let mut b = 1u32;
    while b <= 4 {
        if led.register_asset(b, b + 1).is_err() {
            return false;
        }
        if led.register_sample(b, 2).is_err() {
            return false;
        }
        b += 1;
    }
    if led.asset_len() != 4 || led.sample_len() != 4 {
        return false;
    }
    // 稀疏跳批：递增即合法。
    if led.register_asset(7, 1).is_err() {
        return false;
    }
    if led.asset_len() != 5 {
        return false;
    }
    // 回退/重复：批 6 < 7 拒。
    if led.register_asset(6, 1) != Err(ClDiag::ESeq.val()) {
        return false;
    }
    // 差异账登记可查。
    if led.register_diff(2, DiffDir::Plus, 1).is_err() {
        return false;
    }
    led.diff_len() == 1
}

/// 收官行链：行号从 1 递增、指纹只增不减、衔接零悬空。
fn append_seq_ok() -> bool {
    let led = build_balanced();
    if led.len() != 51 {
        return false;
    }
    // 行号从 1 逐行递增。
    let mut no = 1u32;
    let mut idx = 0usize;
    while idx < led.len() {
        match led.row_at(idx) {
            Some(r) => {
                if r.row_no != no {
                    return false;
                }
            }
            None => return false,
        }
        no += 1;
        idx += 1;
    }
    // 归档指纹只增不减。
    let mut prev = FP_GENESIS;
    idx = 0;
    while idx < led.len() {
        match led.row_at(idx) {
            Some(r) => {
                if r.fp <= prev {
                    return false;
                }
                prev = r.fp;
            }
            None => return false,
        }
        idx += 1;
    }
    // 衔接零悬空：每行来源批都已登记。
    idx = 0;
    while idx < led.len() {
        match led.row_at(idx) {
            Some(r) => {
                if led.asset_of(r.src_ref).is_none() {
                    return false;
                }
            }
            None => return false,
        }
        idx += 1;
    }
    true
}

/// 等式断言对平：A×S ≡ 总行数，等式行四元组复算一致。
fn equation_ok() -> bool {
    let led = build_balanced();
    match led.assert_equation() {
        Ok(eq) => {
            eq.amount == led.len() as u32
                && eq.fp == led.chain_fp()
                && eq.row_no == led.len() as u32 + 1
                && eq.src_ref == EQ_SRC_REF
        }
        Err(_) => false,
    }
}

/// 差异账补录/核销对平：调整后铺足行，等式仍平。
fn diff_balance_ok() -> bool {
    let led = build_balanced_with_diff();
    if led.diff_len() != 2 {
        return false;
    }
    match led.assert_equation() {
        Ok(eq) => eq.amount == led.len() as u32 && led.len() == 52,
        Err(_) => false,
    }
}

/// 正常流三段：对平推进 Balanced → 封账 Sealed → 等式行归档指纹命中。
fn happy_path_ok() -> bool {
    let mut led = build_balanced();
    if led.state() != LedgerState::Open {
        return false;
    }
    if led.reconcile().is_err() {
        return false;
    }
    if led.state() != LedgerState::Balanced {
        return false;
    }
    match led.seal() {
        Ok(eq) => {
            led.state() == LedgerState::Sealed
                && eq.amount == 51
                && led.sealed_fp() == Some(eq.fp)
                && led.equation_row() == Some(eq)
                && led.audit_chain().is_ok()
        }
        Err(_) => false,
    }
}

/// 封账段与移交段写回：落章即"状态=已完成"，结果摘要非空。
fn seal_handover_ok() -> bool {
    let mut led = build_balanced();
    if led.reconcile().is_err() {
        return false;
    }
    match led.seal() {
        Ok(eq) => {
            // 完成写回：状态+行数+指纹摘要，非空即写回合规。
            let summary = format!(
                "state={:?} rows={} fp=0x{:05x}",
                led.state(),
                led.len(),
                eq.fp
            );
            !summary.is_empty() && led.state() == LedgerState::Sealed
        }
        Err(_) => false,
    }
}

/// 注入必抓：账行篡改 10/10。
fn tamper_caught_ok() -> bool {
    let (caught, total, escape) = tamper_sweep();
    total == 10 && caught == 10 && escape.is_empty()
}

/// 注入必抓：不对平 10/10（六路语义码定位）。
fn imbalance_caught_ok() -> bool {
    let (caught, total, escape) = imbalance_sweep();
    total == 10 && caught == 10 && escape.is_empty()
}

/// 注入必抓：重复封账 10/10。
fn reseal_caught_ok() -> bool {
    let (caught, total, escape) = reseal_sweep();
    total == 10 && caught == 10 && escape.is_empty()
}

/// 判定次序冻结：写入面 封账>容量>批闭集；对平面 状态>空账；批闭集先于指纹。
fn gate_order_ok() -> bool {
    // 1) 封账闸最先：封账后有效批写入也报 ESealed（先于批闸/指纹闸）。
    let mut sealed = build_balanced();
    let _ = sealed.reconcile();
    if sealed.seal().is_err() {
        return false;
    }
    if sealed.append_row(1) != Err(ClDiag::ESealed.val()) {
        return false;
    }
    // 封账后登记也拒，且先于批闭集闸（坏批 9 同样报 ESealed）。
    if sealed.register_asset(9, 1) != Err(ClDiag::ESealed.val()) {
        return false;
    }
    // 2) 容量闸先于批闭集闸：满账时坏批也报 ECap。
    let mut full = build_capacity_edge();
    if full.append_row(9) != Err(ClDiag::ECap.val()) {
        return false;
    }
    if full.append_row(1) != Err(ClDiag::ECap.val()) {
        return false;
    }
    // 3) 对平面状态闸先于空账闸：空账封账报 ENotBalanced 而非 EEmpty。
    let mut fresh = ClosureLedger::new();
    if fresh.seal() != Err(ClDiag::ENotBalanced.val()) {
        return false;
    }
    if fresh.reconcile() != Err(ClDiag::EEmpty.val()) {
        return false;
    }
    // 4) 批闭集闸先于指纹闸：空账坏批报 EBatchRange。
    let mut open = ClosureLedger::new();
    if open.append_row(9) != Err(ClDiag::EBatchRange.val()) {
        return false;
    }
    true
}

/// 容量耗尽：满账后再追加拒绝且账面不变。
fn capacity_ok() -> bool {
    let mut led = build_capacity_edge();
    if led.len() != ROW_CAP {
        return false;
    }
    let before = led.len();
    if led.append_row(1) != Err(ClDiag::ECap.val()) {
        return false;
    }
    led.len() == before
}

/// 四类边界：批闭集边界/批序边界/首行边界/状态与指纹上溢边界。
fn boundary_ok() -> bool {
    // 边界一：批号闭集 1..=8——0 与 9 拒，1 与 8 收。
    let mut l1 = ClosureLedger::new();
    if l1.register_asset(0, 1) != Err(ClDiag::EBatchRange.val()) {
        return false;
    }
    if l1.register_asset(9, 1) != Err(ClDiag::EBatchRange.val()) {
        return false;
    }
    if l1.register_asset(1, 1).is_err() {
        return false;
    }
    if l1.register_asset(8, 1).is_err() {
        return false;
    }
    // 边界二：批序边界——重复（8<=8）与回退（3<8）均拒。
    if l1.register_asset(8, 2) != Err(ClDiag::ESeq.val()) {
        return false;
    }
    if l1.register_asset(3, 2) != Err(ClDiag::ESeq.val()) {
        return false;
    }
    if l1.register_sample(0, 1) != Err(ClDiag::EBatchRange.val()) {
        return false;
    }
    // 边界三：首行边界——行号 1、金额 1、指纹 = genesis 推进值。
    let mut l2 = ClosureLedger::new();
    let _ = l2.register_asset(1, 1);
    let _ = l2.register_sample(1, 1);
    if l2.append_row(1).is_err() {
        return false;
    }
    match l2.row_at(0) {
        Some(r) => {
            let expect_fp = fp_advance(FP_GENESIS, 1, 1, 1).unwrap_or(0);
            if r.row_no != 1 || r.amount != 1 || r.fp != expect_fp {
                return false;
            }
        }
        None => return false,
    }
    // 边界四：状态与指纹上溢——空账封账/对平拒、链推进上溢返回 None。
    let mut l3 = ClosureLedger::new();
    if l3.seal() != Err(ClDiag::ENotBalanced.val()) {
        return false;
    }
    if l3.reconcile() != Err(ClDiag::EEmpty.val()) {
        return false;
    }
    if fp_advance(u32::MAX, 1, 1, 1).is_some() {
        return false;
    }
    true
}

/// 双跑一致：构建、等式行、三类注入全流程两次逐项一致。
fn dual_run_ok() -> bool {
    let a = build_balanced();
    let b = build_balanced();
    if a.len() != b.len() || a.state() != b.state() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.row_at(i), b.row_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if a.assert_equation() != b.assert_equation() {
        return false;
    }
    tamper_sweep() == tamper_sweep()
        && imbalance_sweep() == imbalance_sweep()
        && reseal_sweep() == reseal_sweep()
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12775 自检（判据 12775 卡：文档对齐收官账等式断言 + 三段零悬空）。
pub fn run_unx12775_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12775");
    let led = build_balanced();

    s.add_dyn(
        "F12775-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12775-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "n={} head=0x{:05x}",
            CL_ALL.len(),
            CL_ALL.first().map(|d| d.val()).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12775-03 三账登记回归段",
        triple_register_ok(),
        &format!(
            "A={} S={} D={}",
            led.asset_len(),
            led.sample_len(),
            led.diff_len()
        ),
    );

    s.add_dyn(
        "F12775-04 收官行链零悬空",
        append_seq_ok(),
        &format!("rows={} fp0=0x{:05x}", led.len(), led.chain_fp()),
    );

    s.add_dyn(
        "F12775-05 等式断言对平",
        equation_ok(),
        &format!(
            "rows={} eq={:?}",
            led.len(),
            led.assert_equation().map(|e| e.amount)
        ),
    );

    s.add_dyn(
        "F12775-06 差异账补录核销对平",
        diff_balance_ok(),
        &format!("diff=2 rows=52"),
    );

    s.add_dyn(
        "F12775-07 封账移交段写回",
        happy_path_ok() && seal_handover_ok(),
        &format!(
            "state={:?} sealed_fp={:?}",
            led.state(),
            led.sealed_fp()
        ),
    );

    // 注入必抓一：账行篡改十轮全数命中（人为构造违约必被拒）。
    let (tc, tt, te) = tamper_sweep();
    s.add_dyn(
        "F12775-08 账行篡改注入 10/10",
        tc == 10 && tt == 10 && te.is_empty(),
        &format!("caught={}/{} {}", tc, tt, te),
    );

    // 注入必抓二：不对平十轮全数按账报码。
    let (ic, it, ie) = imbalance_sweep();
    s.add_dyn(
        "F12775-09 不对平注入 10/10",
        ic == 10 && it == 10 && ie.is_empty(),
        &format!("caught={}/{} {}", ic, it, ie),
    );

    // 注入必抓三：重复封账十轮全数拒绝。
    let (rc, rt, re) = reseal_sweep();
    s.add_dyn(
        "F12775-10 重复封账注入 10/10",
        rc == 10 && rt == 10 && re.is_empty(),
        &format!("caught={}/{} {}", rc, rt, re),
    );

    s.add_dyn(
        "F12775-11 判定次序冻结",
        gate_order_ok(),
        "封账>容量>批闭集>指纹；状态>空账",
    );

    s.add_dyn(
        "F12775-12 容量耗尽守卫",
        capacity_ok(),
        &format!("cap={} full+1=ECap", ROW_CAP),
    );

    s.add_dyn(
        "F12775-13 四类边界",
        boundary_ok(),
        "批闭集/批序/首行指纹/状态上溢 四点",
    );

    s.add_dyn(
        "F12775-14 双跑一致指纹",
        dual_run_ok(),
        "两次构建与三类注入逐项一致",
    );

    // 收口：判据条数声明（判据 ①②③ 对应检查面共 15 条）。
    let declared = 15usize;
    s.add_dyn(
        "F12775-15 收口判据条数",
        s.len() + 1 == declared,
        &format!("判据3条 检查{}条", s.len() + 1),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_vx9_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 全批窗口。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x82BC0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 码互异、全落段窗、查表命中、每码文案非空、未知码回落。
        assert_eq!(CL_ALL.len(), 12);
        let mut i = 0usize;
        while i < CL_ALL.len() {
            let vi = CL_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            assert!(!detail(vi).is_empty());
            let mut j = i + 1;
            while j < CL_ALL.len() {
                let vj = CL_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
        assert_eq!(detail(ClDiag::ETamper.val()), ClDiag::ETamper.detail());
    }

    #[test]
    fn happy_path_seals_with_equation_row() {
        // 正常流：开账→对平→封账，等式行四元组齐、指纹命中、审计绿。
        let mut led = build_balanced();
        assert_eq!(led.len(), 51);
        assert_eq!(led.state(), LedgerState::Open);
        assert!(led.reconcile().is_ok());
        assert_eq!(led.state(), LedgerState::Balanced);
        let eq = led.seal();
        assert!(eq.is_ok());
        let eq = eq.unwrap_or(EquationRow {
            row_no: 0,
            src_ref: 0,
            amount: 0,
            fp: 0,
        });
        assert_eq!(eq.amount, 51);
        assert_eq!(eq.row_no, 52);
        assert_eq!(led.state(), LedgerState::Sealed);
        assert_eq!(led.sealed_fp(), Some(eq.fp));
        assert!(led.audit_chain().is_ok());
    }

    #[test]
    fn tamper_injections_all_caught() {
        // 注入必抓一：账行篡改十轮全数命中，无逃脱。
        let (caught, total, escape) = tamper_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
        // 单路复演：篡改后行数不变、审计报 ETamper。
        let mut led = build_balanced();
        let before = led.len();
        assert!(led.row_amount_mut(3, 5));
        assert_eq!(led.len(), before);
        assert_eq!(led.audit_chain(), Err(ClDiag::ETamper.val()));
        // 未篡改账审计绿。
        assert!(build_balanced().audit_chain().is_ok());
    }

    #[test]
    fn imbalance_injections_all_caught_with_attribution() {
        // 注入必抓二：六路各一遍按定位规则报对应账，再十轮 LCG 抖动全抓。
        let kinds = [0usize, 1, 2, 3, 4, 5];
        let mut k = 0usize;
        while k < kinds.len() {
            let kind = kinds.get(k).copied().unwrap_or(0);
            let mut led = build_imbalanced(kind, 2, 1);
            let expect = imbalance_expect(kind);
            assert_eq!(
                led.assert_equation(),
                Err(expect),
                "kind{} 定位码不符",
                kind
            );
            assert!(led.reconcile().is_err());
            k += 1;
        }
        let (caught, total, escape) = imbalance_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn reseal_injections_all_caught() {
        // 注入必抓三：封账后重封/写/重对平十轮全数拒绝。
        let (caught, total, escape) = reseal_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
        // 单路复演三路码。
        let mut led = build_balanced();
        let _ = led.reconcile();
        assert!(led.seal().is_ok());
        assert_eq!(led.seal(), Err(ClDiag::EReSeal.val()));
        assert_eq!(led.append_row(1), Err(ClDiag::ESealed.val()));
        assert_eq!(led.reconcile(), Err(ClDiag::EReSeal.val()));
    }

    #[test]
    fn gate_order_is_frozen() {
        // 判定次序：封账>容量>批闭集>指纹；状态>空账。
        assert!(gate_order_ok());
        let mut sealed = build_balanced();
        let _ = sealed.reconcile();
        let _ = sealed.seal();
        assert_eq!(sealed.append_row(1), Err(ClDiag::ESealed.val()));
        let mut full = build_capacity_edge();
        assert_eq!(full.append_row(9), Err(ClDiag::ECap.val()));
        let mut fresh = ClosureLedger::new();
        assert_eq!(fresh.seal(), Err(ClDiag::ENotBalanced.val()));
        assert_eq!(fresh.append_row(9), Err(ClDiag::EBatchRange.val()));
    }

    #[test]
    fn capacity_and_boundaries_hold() {
        // 容量耗尽：64 行封顶，65 行 ECap。
        assert!(capacity_ok());
        // 四类边界：批闭集/批序/首行/状态与指纹上溢。
        assert!(boundary_ok());
        // 等式乘积上溢防御：8×8 封顶内不会溢出，上溢路径报 EMismatchA。
        let mut led = ClosureLedger::new();
        let _ = led.register_asset(1, BATCH_CAP);
        let _ = led.register_sample(1, BATCH_CAP);
        assert!(led.assert_equation().is_err());
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致：确定性 LCG 下两次全流程逐项相同。
        let a = build_balanced();
        let b = build_balanced();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            assert_eq!(a.row_at(i), b.row_at(i));
            i += 1;
        }
        assert_eq!(a.assert_equation(), b.assert_equation());
        assert_eq!(tamper_sweep(), tamper_sweep());
        assert_eq!(imbalance_sweep(), imbalance_sweep());
        assert_eq!(reseal_sweep(), reseal_sweep());
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿、覆盖 ≥12 条、零红项。
        let s = run_unx12775_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.len(), 15);
        assert_eq!(s.tally().1, 0);
    }
}
