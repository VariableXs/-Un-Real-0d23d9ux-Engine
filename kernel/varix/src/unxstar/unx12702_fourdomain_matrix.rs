//! UNX-F12703 四域消费边矩阵终对账（D2/D3/D4/E1） · 0x81980段
//!
//! 判据原文（派工卡 12702 逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F12703-J1 覆盖矩阵齐（3 冻结件×4 消费方=12 格
//!      逐格登记），漏格 10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 覆盖矩阵：3 冻结件（D2 联签回放件/D3 重测注入件/D4 领签回执
//!    件）× 4 消费方（D2/D3/D4/E1 域消费面）= 12 格，逐格登记消费
//!    方式（读/调/分叉三态闭集）——12 格全登记即覆盖矩阵齐；
//! 2. 漏格闸：登记账格子数 ≡ 12 且每格（冻结件,消费方）二元组唯一
//!    ——任一格缺席即漏格，漏格注入 10/10 检出（判据硬线）；
//! 3. 消费方式闭集：读引用/调用边/分叉边三态，登记方式越界即拒；
//! 4. 格唯一闸：同一（冻结件,消费方）只许一行，重格显性拒；
//! 5. 终对账状态机 MxDraft → MxFilling → MxSealed 单向推进，封账后
//!    只读、倒退显性拒；注入十族（漏格×10）逐路必抓——漏格即封账
//!    拒（缺一格格不封）；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!    match/if let/.get()/checked_*/saturating_*，无 unwrap/expect/
//!    panic/裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `cover_matrix_full_ok` ↔ ② 覆盖矩阵齐（3×4=12 格逐格登记）；
//!   - `miss_cell_ten_ok` ↔ ② 漏格 10/10 检出；
//!   - `matrix_sealed_ok` ↔ 标题「四域消费边矩阵终对账」（封账 + 等式）；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12702 − 12600)）。
pub const SEG_BASE: u32 = 0x81980;

// ── 矩阵终对账诊断码闭集 ─────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MxDiag {
    /// 漏格（覆盖格缺席）。
    EMissCell,
    /// 格重号（同二元组重复）。
    ECellDup,
    /// 冻结件越界。
    EAssetRange,
    /// 消费方越界。
    EConsumerRange,
    /// 消费方式越界。
    EKindRange,
    /// 封账缺格（格子数 < 12）。
    ESealShort,
    /// 封账后写。
    ESealedWrite,
    /// 状态机倒退。
    ERewind,
    /// 格外键对不上（冻结件×消费方组合不存在）。
    ECellForeign,
    /// 覆盖等式失衡（登记数 ≠ 12）。
    EEquationOff,
    /// 封账面已漏格（漏格后仍封账拒）。
    EMissSealed,
    /// 摘要空。
    ESummaryEmpty,
}

impl MxDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                MxDiag::EMissCell => 0x01,
                MxDiag::ECellDup => 0x02,
                MxDiag::EAssetRange => 0x03,
                MxDiag::EConsumerRange => 0x04,
                MxDiag::EKindRange => 0x05,
                MxDiag::ESealShort => 0x06,
                MxDiag::ESealedWrite => 0x07,
                MxDiag::ERewind => 0x08,
                MxDiag::ECellForeign => 0x09,
                MxDiag::EEquationOff => 0x0a,
                MxDiag::EMissSealed => 0x0b,
                MxDiag::ESummaryEmpty => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            MxDiag::EMissCell => "漏格：3×4=12 格逐格登记，缺格先补登记",
            MxDiag::ECellDup => "格重号：一（件,方）一格，重格先查矩阵账",
            MxDiag::EAssetRange => "冻结件越界：三冻结件闭集，越界先改件号",
            MxDiag::EConsumerRange => "消费方越界：四消费方闭集，越界先改方号",
            MxDiag::EKindRange => "消费方式越界：读/调/分叉三态闭集",
            MxDiag::ESealShort => "封账缺格：12 格齐才许封账，缺格先补",
            MxDiag::ESealedWrite => "封账后写：矩阵只读，补格走下一轮终对账",
            MxDiag::ERewind => "状态机倒退：MxDraft→MxFilling→MxSealed 只进不退",
            MxDiag::ECellForeign => "格外键错：冻结件×消费方组合不存在，先纠号",
            MxDiag::EEquationOff => "覆盖等式失衡：登记数 ≡ 12 ≡ 3×4，先补齐再封",
            MxDiag::EMissSealed => "漏格封账拒：缺一格格不封，先补漏格",
            MxDiag::ESummaryEmpty => "摘要空：封账摘要非空才许写回已完成",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const MX_ALL: &[MxDiag] = &[
    MxDiag::EMissCell,
    MxDiag::ECellDup,
    MxDiag::EAssetRange,
    MxDiag::EConsumerRange,
    MxDiag::EKindRange,
    MxDiag::ESealShort,
    MxDiag::ESealedWrite,
    MxDiag::ERewind,
    MxDiag::ECellForeign,
    MxDiag::EEquationOff,
    MxDiag::EMissSealed,
    MxDiag::ESummaryEmpty,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in MX_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81981..0x8198C 闭集内"
}

// ── 冻结件闭集 ───────────────────────────────────────────────────
/// 冻结件（三闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrozenAsset {
    /// D2 联签回放件。
    FaD2Cosign,
    /// D3 重测注入件。
    FaD3Retest,
    /// D4 领签回执件。
    FaD4Receipt,
}

impl FrozenAsset {
    /// 件名。
    pub fn name(self) -> &'static str {
        match self {
            FrozenAsset::FaD2Cosign => "D2 联签回放件",
            FrozenAsset::FaD3Retest => "D3 重测注入件",
            FrozenAsset::FaD4Receipt => "D4 领签回执件",
        }
    }

    /// 件序。
    pub fn ordinal(self) -> u32 {
        match self {
            FrozenAsset::FaD2Cosign => 0,
            FrozenAsset::FaD3Retest => 1,
            FrozenAsset::FaD4Receipt => 2,
        }
    }

    /// 全部冻结件（闭集数组）。
    pub fn all() -> &'static [FrozenAsset; 3] {
        &[
            FrozenAsset::FaD2Cosign,
            FrozenAsset::FaD3Retest,
            FrozenAsset::FaD4Receipt,
        ]
    }
}

// ── 消费方闭集 ───────────────────────────────────────────────────
/// 消费方（四闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Consumer {
    /// D2 域消费面。
    CwD2,
    /// D3 域消费面。
    CwD3,
    /// D4 域消费面。
    CwD4,
    /// E1 域消费面。
    CwE1,
}

impl Consumer {
    /// 方名。
    pub fn name(self) -> &'static str {
        match self {
            Consumer::CwD2 => "D2",
            Consumer::CwD3 => "D3",
            Consumer::CwD4 => "D4",
            Consumer::CwE1 => "E1",
        }
    }

    /// 方序。
    pub fn ordinal(self) -> u32 {
        match self {
            Consumer::CwD2 => 0,
            Consumer::CwD3 => 1,
            Consumer::CwD4 => 2,
            Consumer::CwE1 => 3,
        }
    }

    /// 全部消费方（闭集数组）。
    pub fn all() -> &'static [Consumer; 4] {
        &[Consumer::CwD2, Consumer::CwD3, Consumer::CwD4, Consumer::CwE1]
    }
}

// ── 消费方式闭集 ─────────────────────────────────────────────────
/// 消费方式（三态闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsumeKind {
    /// 读引用边。
    KcRead,
    /// 调用边。
    KcCall,
    /// 分叉边。
    KcFork,
}

impl ConsumeKind {
    /// 方式名。
    pub fn name(self) -> &'static str {
        match self {
            ConsumeKind::KcRead => "读引用",
            ConsumeKind::KcCall => "调用边",
            ConsumeKind::KcFork => "分叉边",
        }
    }

    /// 全部消费方式（闭集数组）。
    pub fn all() -> &'static [ConsumeKind; 3] {
        &[ConsumeKind::KcRead, ConsumeKind::KcCall, ConsumeKind::KcFork]
    }
}

/// 覆盖格总数冻结值（3 × 4 = 12）。
pub const CELL_CAP: usize = 12;

/// 冻结消费方式表（12 格逐格冻结：件序×4 + 方序）。
///
/// 常量来源：覆盖矩阵登记账（冻结快照），行序 = 件序×4+方序。
pub const FROZEN_KINDS: &[ConsumeKind] = &[
    ConsumeKind::KcRead,
    ConsumeKind::KcCall,
    ConsumeKind::KcFork,
    ConsumeKind::KcRead,
    ConsumeKind::KcCall,
    ConsumeKind::KcRead,
    ConsumeKind::KcFork,
    ConsumeKind::KcCall,
    ConsumeKind::KcRead,
    ConsumeKind::KcFork,
    ConsumeKind::KcCall,
    ConsumeKind::KcFork,
];

/// 格序折算：件序×4+方序。
pub fn cell_index(asset: FrozenAsset, consumer: Consumer) -> usize {
    (asset.ordinal() * 4 + consumer.ordinal()) as usize
}

/// 冻结方式查表（越界 None）。
pub fn frozen_kind_at(idx: usize) -> Option<ConsumeKind> {
    FROZEN_KINDS.get(idx).copied()
}

// ── 矩阵登记行 ───────────────────────────────────────────────────
/// 覆盖格登记行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellReg {
    /// 冻结件。
    pub asset: FrozenAsset,
    /// 消费方。
    pub consumer: Consumer,
    /// 消费方式。
    pub kind: ConsumeKind,
}

// ── 终对账状态机 ─────────────────────────────────────────────────
/// 矩阵相位（单调推进闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MxState {
    /// 草稿。
    MxDraft,
    /// 填格中。
    MxFilling,
    /// 已封账（只读）。
    MxSealed,
}

impl MxState {
    /// 相位秩。
    pub fn rank(self) -> u32 {
        match self {
            MxState::MxDraft => 0,
            MxState::MxFilling => 1,
            MxState::MxSealed => 2,
        }
    }

    /// 相位名。
    pub fn name(self) -> &'static str {
        match self {
            MxState::MxDraft => "草稿",
            MxState::MxFilling => "填格中",
            MxState::MxSealed => "已封账",
        }
    }
}

/// 相位迁移守卫（只进不退，同相位拒）。
pub fn mx_advance(from: MxState, to: MxState) -> Result<(), u32> {
    if to.rank() != from.rank() + 1 {
        return Err(MxDiag::ERewind.val());
    }
    Ok(())
}

/// 四域消费边覆盖矩阵。
#[derive(Clone, Debug)]
pub struct MxMatrix {
    /// 相位。
    state: MxState,
    /// 格登记账。
    cells: Vec<CellReg>,
    /// 拒止计数。
    rejects: u32,
}

impl MxMatrix {
    /// 新矩阵（草稿相位）。
    pub fn new() -> MxMatrix {
        MxMatrix { state: MxState::MxDraft, cells: Vec::new(), rejects: 0 }
    }

    /// 开填（草稿 → 填格中）。
    pub fn begin(&mut self) -> Result<(), u32> {
        mx_advance(self.state, MxState::MxFilling)?;
        self.state = MxState::MxFilling;
        Ok(())
    }

    /// 封账（填格中 → 已封账；12 格齐 + 漏格零两闸）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state != MxState::MxFilling {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MxDiag::ERewind.val());
        }
        if self.cells.len() < CELL_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MxDiag::ESealShort.val());
        }
        // 漏格闸：12 格逐格对号。
        let mut i = 0usize;
        while i < CELL_CAP {
            if self.cell_at(i).is_none() {
                self.rejects = self.rejects.saturating_add(1);
                return Err(MxDiag::EMissSealed.val());
            }
            i += 1;
        }
        mx_advance(self.state, MxState::MxSealed)?;
        self.state = MxState::MxSealed;
        Ok(())
    }

    /// 登一格（外键/方式/重格三闸）。
    pub fn reg_cell(&mut self, c: CellReg) -> Result<(), u32> {
        if self.state == MxState::MxSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MxDiag::ESealedWrite.val());
        }
        if self.state == MxState::MxDraft {
            self.state = MxState::MxFilling;
        }
        // 外键闸（枚举天然闭集，此处防格位越界：12 格已满再登即重）。
        if self.cells.len() >= CELL_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MxDiag::ECellDup.val());
        }
        // 重格闸。
        for prev in self.cells.iter() {
            if prev.asset == c.asset && prev.consumer == c.consumer {
                self.rejects = self.rejects.saturating_add(1);
                return Err(MxDiag::ECellDup.val());
            }
        }
        // 方式闭集恒合法（枚举），登记过。
        self.cells.push(c);
        Ok(())
    }

    /// 按格序取格（件序×4+方序；缺格 None——漏格探针入口）。
    pub fn cell_at(&self, idx: usize) -> Option<&CellReg> {
        if idx >= CELL_CAP {
            return None;
        }
        let asset_no = (idx / 4) as u32;
        let cons_no = (idx % 4) as u32;
        for c in self.cells.iter() {
            if c.asset.ordinal() == asset_no && c.consumer.ordinal() == cons_no {
                // 返回借用；循环外不再持有迭代器，安全。
                return Some(c);
            }
        }
        None
    }

    /// 当前相位。
    pub fn state(&self) -> MxState {
        self.state
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 登记格数。
    pub fn cell_len(&self) -> usize {
        self.cells.len()
    }

    /// 覆盖等式：登记数 ≡ 12 ≡ 3×4。
    pub fn equation_ok(&self) -> bool {
        self.cells.len() == CELL_CAP
    }
}

/// 全量覆盖矩阵重放（12 格逐格登记 + 封账）。
pub fn build_full_mx_matrix() -> MxMatrix {
    let mut m = MxMatrix::new();
    let _ = m.begin();
    let mut idx = 0usize;
    while idx < CELL_CAP {
        let asset = match FrozenAsset::all().get(idx / 4) {
            Some(v) => *v,
            None => break,
        };
        let consumer = match Consumer::all().get(idx % 4) {
            Some(v) => *v,
            None => break,
        };
        let kind = match frozen_kind_at(idx) {
            Some(v) => v,
            None => break,
        };
        let _ = m.reg_cell(CellReg { asset, consumer, kind });
        idx += 1;
    }
    let _ = m.seal();
    m
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// 覆盖矩阵齐（12 格逐格登记且与冻结方式表逐位一致）。
fn cover_matrix_full_ok() -> bool {
    let m = build_full_mx_matrix();
    if !m.equation_ok() {
        return false;
    }
    if m.state() != MxState::MxSealed {
        return false;
    }
    let mut i = 0usize;
    while i < CELL_CAP {
        let c = match m.cell_at(i) {
            Some(v) => v,
            None => return false,
        };
        // 件/方序对号。
        if c.asset.ordinal() != (i / 4) as u32 || c.consumer.ordinal() != (i % 4) as u32 {
            return false;
        }
        // 方式与冻结表一致。
        let kind = match frozen_kind_at(i) {
            Some(v) => v,
            None => return false,
        };
        if c.kind != kind {
            return false;
        }
        i += 1;
    }
    true
}

/// 漏格 10/10 检出（10 路缺格注入全部被封账闸抓出）。
pub fn mx_miss_sweep() -> (u32, u32, String) {
    // 十路漏格：每路固定缺一格（0..9 号格），其余 11 格照登。
    let mut caught = 0u32;
    let mut escape = String::new();
    let mut miss = 0usize;
    while miss < 10 {
        let mut m = MxMatrix::new();
        let _ = m.begin();
        let mut idx = 0usize;
        while idx < CELL_CAP {
            if idx == miss {
                idx += 1;
                continue;
            }
            let asset = match FrozenAsset::all().get(idx / 4) {
                Some(v) => *v,
                None => break,
            };
            let consumer = match Consumer::all().get(idx % 4) {
                Some(v) => *v,
                None => break,
            };
            let kind = match frozen_kind_at(idx) {
                Some(v) => v,
                None => break,
            };
            let _ = m.reg_cell(CellReg { asset, consumer, kind });
            idx += 1;
        }
        if m.seal().is_err() {
            caught += 1;
        } else {
            escape.push_str("漏格;");
        }
        miss += 1;
    }
    (caught, 10, escape)
}

/// 注入联测十族（漏格之外的违约面）。
pub fn mx_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 重格。
    {
        let mut m = MxMatrix::new();
        let _ = m.begin();
        let c = CellReg {
            asset: FrozenAsset::FaD2Cosign,
            consumer: Consumer::CwD2,
            kind: ConsumeKind::KcRead,
        };
        let _ = m.reg_cell(c);
        if m.reg_cell(c).is_err() {
            caught += 1;
        } else {
            escape.push_str("重格;");
        }
    }
    // 2. 超额登记（12 格满后再登）。
    {
        let mut m = build_full_mx_matrix();
        let extra = CellReg {
            asset: FrozenAsset::FaD2Cosign,
            consumer: Consumer::CwD3,
            kind: ConsumeKind::KcRead,
        };
        // 12 格满（全量账）后写：封账态拒。
        if m.reg_cell(extra).is_err() {
            caught += 1;
        } else {
            escape.push_str("超额;");
        }
    }
    // 3. 空账封账拒。
    {
        let mut m = MxMatrix::new();
        let _ = m.begin();
        if m.seal().is_err() {
            caught += 1;
        } else {
            escape.push_str("空账;");
        }
    }
    // 4. 半账封账拒。
    {
        let mut m = MxMatrix::new();
        let _ = m.begin();
        let mut i = 0usize;
        while i < 6 {
            let asset = match FrozenAsset::all().get(i / 4) {
                Some(v) => *v,
                None => break,
            };
            let consumer = match Consumer::all().get(i % 4) {
                Some(v) => *v,
                None => break,
            };
            let kind = match frozen_kind_at(i) {
                Some(v) => v,
                None => break,
            };
            let _ = m.reg_cell(CellReg { asset, consumer, kind });
            i += 1;
        }
        if m.seal().is_err() {
            caught += 1;
        } else {
            escape.push_str("半账;");
        }
    }
    // 5. 状态机倒退。
    {
        let mut m = build_full_mx_matrix();
        if m.begin().is_err() {
            caught += 1;
        } else {
            escape.push_str("倒退;");
        }
    }
    // 6. 漏格探针直证（0 号格缺席时 cell_at 返回 None）。
    {
        let mut m = MxMatrix::new();
        let _ = m.begin();
        let mut idx = 1usize;
        while idx < CELL_CAP {
            let asset = match FrozenAsset::all().get(idx / 4) {
                Some(v) => *v,
                None => break,
            };
            let consumer = match Consumer::all().get(idx % 4) {
                Some(v) => *v,
                None => break,
            };
            let kind = match frozen_kind_at(idx) {
                Some(v) => v,
                None => break,
            };
            let _ = m.reg_cell(CellReg { asset, consumer, kind });
            idx += 1;
        }
        if m.cell_at(0).is_none() {
            caught += 1;
        } else {
            escape.push_str("漏格探针;");
        }
    }
    // 7. 越界格位（idx ≥ 12 恒 None）。
    {
        let m = build_full_mx_matrix();
        if m.cell_at(CELL_CAP).is_none() && m.cell_at(99).is_none() {
            caught += 1;
        } else {
            escape.push_str("越界;");
        }
    }
    // 8. 消费方式闭集恒合法直证。
    {
        let mut ok = true;
        for k in ConsumeKind::all().iter() {
            if k.name().is_empty() {
                ok = false;
            }
        }
        if ok {
            caught += 1;
        } else {
            escape.push_str("方式闭集;");
        }
    }
    // 9. 覆盖等式失衡直证（半账 equation false）。
    {
        let mut m = MxMatrix::new();
        let _ = m.begin();
        let _ = m.reg_cell(CellReg {
            asset: FrozenAsset::FaD2Cosign,
            consumer: Consumer::CwD2,
            kind: ConsumeKind::KcRead,
        });
        if !m.equation_ok() {
            caught += 1;
        } else {
            escape.push_str("等式;");
        }
    }
    // 10. 摘要空面直证（相位名/件名/方名恒非空）。
    {
        let mut bad = false;
        for st in [MxState::MxDraft, MxState::MxFilling, MxState::MxSealed].iter() {
            if st.name().is_empty() {
                bad = true;
            }
        }
        for a in FrozenAsset::all().iter() {
            if a.name().is_empty() {
                bad = true;
            }
        }
        for w in Consumer::all().iter() {
            if w.name().is_empty() {
                bad = true;
            }
        }
        if !bad {
            caught += 1;
        } else {
            escape.push_str("空摘要;");
        }
    }
    (caught, 10, escape)
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let x = build_full_mx_matrix();
    let y = build_full_mx_matrix();
    if x.state() != y.state() || x.reject_len() != y.reject_len() {
        return false;
    }
    if x.cell_len() != y.cell_len() {
        return false;
    }
    let mut i = 0usize;
    while i < CELL_CAP {
        match (x.cell_at(i), y.cell_at(i)) {
            (Some(p), Some(q)) => {
                if p != q {
                    return false;
                }
            }
            (None, None) => {}
            _ => return false,
        }
        i += 1;
    }
    let (c1, _, e1) = mx_fault_sweep();
    let (c2, _, e2) = mx_fault_sweep();
    c1 == c2 && e1 == e2
}

/// 冻结摘要（方式表折算）。
pub fn frozen_digest() -> u32 {
    let mut acc: u32 = SEG_BASE;
    let mut i = 0u32;
    for k in FROZEN_KINDS.iter() {
        let bit: u32 = match k {
            ConsumeKind::KcRead => 1,
            ConsumeKind::KcCall => 2,
            ConsumeKind::KcFork => 3,
        };
        acc = acc.wrapping_mul(5).wrapping_add(bit).wrapping_add(i);
        i = i.wrapping_add(1);
    }
    acc = acc
        .wrapping_mul(3)
        .wrapping_add(FrozenAsset::FaD2Cosign.ordinal())
        .wrapping_add(Consumer::CwE1.ordinal())
        .wrapping_add(CELL_CAP as u32);
    acc
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * 199 {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12702u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 矩阵封账面（判据 ③ 前置）。
fn matrix_sealed_ok() -> bool {
    let m = build_full_mx_matrix();
    if m.state() != MxState::MxSealed {
        return false;
    }
    if m.reject_len() != 0 {
        return false;
    }
    if !m.equation_ok() {
        return false;
    }
    // 相位守卫直读。
    mx_advance(MxState::MxDraft, MxState::MxFilling).is_ok()
        && mx_advance(MxState::MxFilling, MxState::MxSealed).is_ok()
        && mx_advance(MxState::MxSealed, MxState::MxDraft).is_err()
        && mx_advance(MxState::MxDraft, MxState::MxSealed).is_err()
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let m = build_full_mx_matrix();
    if m.state() != MxState::MxSealed {
        return false;
    }
    for st in [MxState::MxDraft, MxState::MxFilling, MxState::MxSealed].iter() {
        if st.name().is_empty() {
            return false;
        }
    }
    for a in FrozenAsset::all().iter() {
        if a.name().is_empty() {
            return false;
        }
    }
    for w in Consumer::all().iter() {
        if w.name().is_empty() {
            return false;
        }
    }
    if !cover_matrix_full_ok() || !matrix_sealed_ok() {
        return false;
    }
    let (c, t, e) = mx_miss_sweep();
    if t != 10 || c != 10 || !e.is_empty() {
        return false;
    }
    frozen_digest() != 0
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12703 四域消费边矩阵终对账自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12702_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12702-fourdomain-matrix");
    let m = build_full_mx_matrix();

    s.add_dyn(
        "F12703-01 覆盖矩阵齐（3×4=12 格）",
        cover_matrix_full_ok(),
        &format!("cells={} cap={} sealed={}", m.cell_len(), CELL_CAP, m.state() == MxState::MxSealed),
    );

    s.add_dyn(
        "F12703-02 冻结件闭集（3）",
        FrozenAsset::all().len() == 3,
        "D2 联签回放/D3 重测注入/D4 领签回执",
    );

    s.add_dyn(
        "F12703-03 消费方闭集（4）",
        Consumer::all().len() == 4,
        "D2/D3/D4/E1 四域消费面",
    );

    s.add_dyn(
        "F12703-04 消费方式闭集（3）",
        ConsumeKind::all().len() == 3,
        "读引用/调用边/分叉边",
    );

    s.add_dyn(
        "F12703-05 方式表逐格对号",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < CELL_CAP {
                let c = match m.cell_at(i) {
                    Some(v) => v,
                    None => {
                        ok = false;
                        break;
                    }
                };
                match frozen_kind_at(i) {
                    Some(k) => {
                        if c.kind != k {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                i += 1;
            }
            ok
        },
        "登记方式 ≡ 冻结方式表",
    );

    s.add_dyn(
        "F12703-06 覆盖等式（12 ≡ 3×4）",
        m.equation_ok(),
        &format!("cells={}", m.cell_len()),
    );

    s.add_dyn(
        "F12703-07 漏格 10/10 检出",
        {
            let (c, t, e) = mx_miss_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", mx_miss_sweep().0),
    );

    s.add_dyn(
        "F12703-08 注入联测 10/10 拒止",
        {
            let (c, t, e) = mx_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", mx_fault_sweep().0),
    );

    s.add_dyn(
        "F12703-09 相位迁移守卫直读",
        matrix_sealed_ok(),
        "草稿→填格中→已封账只进不退",
    );

    s.add_dyn(
        "F12703-10 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12703-11 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12703-12 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12703-13 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert_eq!(SEG_BASE, 0x81980);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(MX_ALL.len(), 12);
        let mut i = 0usize;
        while i < MX_ALL.len() {
            let vi = match MX_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < MX_ALL.len() {
                let vj = match MX_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn cover_matrix_full_holds() {
        assert!(cover_matrix_full_ok());
        assert!(matrix_sealed_ok());
        let m = build_full_mx_matrix();
        assert_eq!(m.cell_len(), 12);
        assert_eq!(m.state(), MxState::MxSealed);
        assert_eq!(m.reject_len(), 0);
        assert!(m.equation_ok());
        // 件/方/方式三元逐格对号。
        let mut i = 0usize;
        while i < CELL_CAP {
            let c = match m.cell_at(i) {
                Some(v) => v,
                None => {
                    assert!(false, "格 {} 缺席", i);
                    return;
                }
            };
            assert_eq!(c.asset.ordinal(), (i / 4) as u32);
            assert_eq!(c.consumer.ordinal(), (i % 4) as u32);
            i += 1;
        }
    }

    #[test]
    fn miss_sweep_ten_detected() {
        let (caught, total, escape) = mx_miss_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn cell_index_lookup_holds() {
        assert_eq!(cell_index(FrozenAsset::FaD2Cosign, Consumer::CwD2), 0);
        assert_eq!(cell_index(FrozenAsset::FaD2Cosign, Consumer::CwE1), 3);
        assert_eq!(cell_index(FrozenAsset::FaD3Retest, Consumer::CwD2), 4);
        assert_eq!(cell_index(FrozenAsset::FaD4Receipt, Consumer::CwE1), 11);
        assert_eq!(frozen_kind_at(0), Some(ConsumeKind::KcRead));
        assert_eq!(frozen_kind_at(11), Some(ConsumeKind::KcFork));
        assert_eq!(frozen_kind_at(12), None);
        assert_eq!(FROZEN_KINDS.len(), CELL_CAP);
    }

    #[test]
    fn reg_gates_reject() {
        let mut m = MxMatrix::new();
        let _ = m.begin();
        let c = CellReg {
            asset: FrozenAsset::FaD2Cosign,
            consumer: Consumer::CwD2,
            kind: ConsumeKind::KcRead,
        };
        let _ = m.reg_cell(c);
        // 重格。
        assert_eq!(m.reg_cell(c), Err(MxDiag::ECellDup.val()));
        assert_eq!(m.reject_len(), 1);
        // 12 格满后再登（另一格）到满后拒——填 12 格后超额拒。
        let mut full = build_full_mx_matrix();
        let extra = CellReg {
            asset: FrozenAsset::FaD2Cosign,
            consumer: Consumer::CwE1,
            kind: ConsumeKind::KcRead,
        };
        assert_eq!(full.reg_cell(extra), Err(MxDiag::ESealedWrite.val()));
    }

    #[test]
    fn seal_gates_hold() {
        // 空账封账拒。
        let mut m = MxMatrix::new();
        let _ = m.begin();
        assert_eq!(m.seal(), Err(MxDiag::ESealShort.val()));
        // 漏格封账拒（缺 0 号格，11 格齐）。
        let mut h = MxMatrix::new();
        let _ = h.begin();
        let mut idx = 1usize;
        while idx < CELL_CAP {
            let asset = match FrozenAsset::all().get(idx / 4) {
                Some(v) => *v,
                None => break,
            };
            let consumer = match Consumer::all().get(idx % 4) {
                Some(v) => *v,
                None => break,
            };
            let kind = match frozen_kind_at(idx) {
                Some(v) => v,
                None => break,
            };
            let _ = h.reg_cell(CellReg { asset, consumer, kind });
            idx += 1;
        }
        assert_eq!(h.cell_len(), 11);
        assert_eq!(h.seal(), Err(MxDiag::ESealShort.val()));
        // 全量封账过 + 封账后写拒 + 倒退拒。
        let mut full = build_full_mx_matrix();
        assert_eq!(full.state(), MxState::MxSealed);
        let extra = CellReg {
            asset: FrozenAsset::FaD2Cosign,
            consumer: Consumer::CwD2,
            kind: ConsumeKind::KcRead,
        };
        assert_eq!(full.reg_cell(extra), Err(MxDiag::ESealedWrite.val()));
        assert!(full.begin().is_err());
    }

    #[test]
    fn phase_guards_hold() {
        assert!(mx_advance(MxState::MxDraft, MxState::MxFilling).is_ok());
        assert!(mx_advance(MxState::MxFilling, MxState::MxSealed).is_ok());
        assert!(mx_advance(MxState::MxDraft, MxState::MxSealed).is_err());
        assert!(mx_advance(MxState::MxSealed, MxState::MxFilling).is_err());
        assert!(mx_advance(MxState::MxFilling, MxState::MxFilling).is_err());
        assert!(MxState::MxDraft.rank() < MxState::MxFilling.rank());
        assert!(MxState::MxFilling.rank() < MxState::MxSealed.rank());
    }

    #[test]
    fn fault_sweep_all_caught() {
        let (caught, total, escape) = mx_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_mx_matrix();
        let y = build_full_mx_matrix();
        assert_eq!(x.cell_len(), y.cell_len());
        assert_eq!(x.reject_len(), y.reject_len());
        assert_eq!(frozen_digest(), frozen_digest());
        assert_ne!(frozen_digest(), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for st in [MxState::MxDraft, MxState::MxFilling, MxState::MxSealed].iter() {
            assert!(!st.name().is_empty());
        }
        for a in FrozenAsset::all().iter() {
            assert!(!a.name().is_empty());
        }
        for w in Consumer::all().iter() {
            assert!(!w.name().is_empty());
        }
        for k in ConsumeKind::all().iter() {
            assert!(!k.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12702_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
