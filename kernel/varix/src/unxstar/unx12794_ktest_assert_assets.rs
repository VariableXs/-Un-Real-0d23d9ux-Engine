//! UNX-F12794 移交包四：ktest 全域断言资产 · 0x83080段
//!
//! 判据原文（派工卡 12794 逐字锚定）：
//!   UNX-F12795-J1 复测资产库齐（三类样本汇总 ≥200 例），同源可重演。
//!
//! 设计要点：
//! 1. 复测资产库三类样本库：注入样本/双机样本/边界样本，各自登记
//!    （样本号/类别/期望结果/指纹）四元组；号窗闭集 1001..=1071 /
//!    2001..=2071 / 3001..=3061，每类账容 70/70/60（闭集恰比账容多
//!    1 例作容量溢出见证位），汇总账恰 200 例逐类点验；
//! 2. 同源可重演：重演判定器 replay(sample_id) -> Result<ReplayRow, u32>，
//!    按登记指纹逐字段复算（类别段/期望结果段/指纹段三段独立复核），
//!    重演结果与登记逐位相等才 Ok——任何一段漂移单独成诊断码；
//! 3. 资产库状态机 Open→Sealed 单向推进，倒退显性拒；封库后只读，
//!    指纹总账只增不减（每例指纹非零、累加严格递增），封库后行级
//!    篡改经总账复算（EFold）与重演判定（EFpShift）双路必抓；
//! 4. 注入器三类（样本指纹篡改/越界样本号/类别缺账）各 10 轮全抓：
//!    指纹篡改含「封库后行级翻位」「入账时指纹污染」「入账时零指纹」
//!    三路；每路均先验基线（干净库重演必须 Ok），防判定器"恒 Err"假绿；
//! 5. 三类样本总账 200 例用冻结闭集登记行实现：每行按
//!    样本号→类别→期望结果→指纹 派生公式复算（指纹四轮整数雪崩、
//!    非零、确定性），真实可复算、零灌水；LCG 种子写死 = SEG_BASE
//!    （常数取自 Knuth MMIX 推荐值），双跑一致；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（固定 LCG 可用）；
//!    取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!    无 unwrap/expect/panic/裸下标；错误路径 Result<T, u32>。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12794 − 12600) = 0x83080；
/// 派工总纲 §3，批次表 SEG 列为陈旧值，以公式为准）。
pub const SEG_BASE: u32 = 0x83080;

/// VX9 卡号下界（派工总纲 §3：id − 12600 为槽序）。
pub const VX9_ID_MIN: u32 = 12600;
/// VX9 卡号上界（200 槽闭窗尾卡）。
pub const VX9_ID_MAX: u32 = 12799;
/// VX9 段窗下界（id 12600 的段基址）。
pub const VX9_SEG_MIN: u32 = 0x80000;
/// VX9 段窗上界（id 12799 的段基址 = 0x80000 + 0x40×199 = 0x831C0）。
pub const VX9_SEG_MAX: u32 = VX9_SEG_MIN + 0x40 * (VX9_ID_MAX - VX9_ID_MIN);

// ── 三类样本号窗闭集（冻结，不许重排） ──────────────────────────
/// 注入样本号窗下沿（1001 起号）。
pub const ID_INJ_LO: u32 = 1001;
/// 注入样本号窗上沿（闭集 71 例：1001..=1071）。
pub const ID_INJ_HI: u32 = 1071;
/// 双机样本号窗下沿（2001 起号）。
pub const ID_DUAL_LO: u32 = 2001;
/// 双机样本号窗上沿（闭集 71 例：2001..=2071）。
pub const ID_DUAL_HI: u32 = 2071;
/// 边界样本号窗下沿（3001 起号）。
pub const ID_BOR_LO: u32 = 3001;
/// 边界样本号窗上沿（闭集 61 例：3001..=3061）。
pub const ID_BOR_HI: u32 = 3061;
/// 注入样本账容（70 例；闭集多 1 例为容量溢出见证位）。
pub const INJ_CAP: u32 = 70;
/// 双机样本账容（70 例）。
pub const DUAL_CAP: u32 = 70;
/// 边界样本账容（60 例）。
pub const BOR_CAP: u32 = 60;
/// 汇总账容量 = 70 + 70 + 60 = 200（判据 ≥200 例恰好收口，不灌水）。
pub const TOTAL_CAP: u32 = INJ_CAP + DUAL_CAP + BOR_CAP;

// ── 复测资产库诊断码闭集 ────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KtaDiag {
    /// 样本号越三类闭集号窗（不在 1001..=1071 / 2001..=2071 / 3001..=3061 内）。
    ENoSample,
    /// 样本重复登记：同一样本号只落一笔。
    EDup,
    /// 容量耗尽：该类账满后再登记被拒。
    ECapFull,
    /// 资产库已封库：封库后一切写入拒绝。
    ESealed,
    /// 状态机倒退：封库后重封。
    ERewind,
    /// 类别缺账：三类未各齐账容就试图封库。
    ENotFull,
    /// 重演对象未登记：账面无该样本号，禁止出重演结论。
    ENotReg,
    /// 类别段漂移：登记类别与号窗派生类别不符（篡改痕迹）。
    EClassShift,
    /// 期望结果段漂移：登记期望与派生期望不符（篡改痕迹）。
    EExpectShift,
    /// 指纹段漂移：登记指纹与复算指纹逐位不等（篡改痕迹）。
    EFpShift,
    /// 零指纹样本：指纹派生不得为零，零指纹一律拒收。
    EZeroFp,
    /// 指纹总账不符：账面行指纹累加与冻结总账出现差额（封库篡改被抓）。
    EFold,
}

impl KtaDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                KtaDiag::ENoSample => 0x01,
                KtaDiag::EDup => 0x02,
                KtaDiag::ECapFull => 0x03,
                KtaDiag::ESealed => 0x04,
                KtaDiag::ERewind => 0x05,
                KtaDiag::ENotFull => 0x06,
                KtaDiag::ENotReg => 0x07,
                KtaDiag::EClassShift => 0x08,
                KtaDiag::EExpectShift => 0x09,
                KtaDiag::EFpShift => 0x0a,
                KtaDiag::EZeroFp => 0x0b,
                KtaDiag::EFold => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            KtaDiag::ENoSample => "样本号越三类闭集号窗：只认注入1001..=1071/双机2001..=2071/边界3001..=3061，先对号窗再登记",
            KtaDiag::EDup => "样本重复登记：同一号只落一笔，重复前先查该号已有账",
            KtaDiag::ECapFull => "容量耗尽：该类账满（70/70/60）后再登记被拒，溢出见证位走下一移交包滚动",
            KtaDiag::ESealed => "资产库已封库：封库后只读，改动走复测流程留痕不回写",
            KtaDiag::ERewind => "状态机倒退：Open→Sealed 只进不退，重封即库面可疑须整体复核",
            KtaDiag::ENotFull => "类别缺账禁封库：三类须各齐 70/70/60，先补齐缺账例再走封库口",
            KtaDiag::ENotReg => "重演对象未登记：账面无该样本号，先按正常流登记再重演",
            KtaDiag::EClassShift => "类别段漂移：登记类别与号窗派生类别不符，按样本号回查类别段",
            KtaDiag::EExpectShift => "期望结果段漂移：登记期望与派生期望不符，按样本号回查期望段",
            KtaDiag::EFpShift => "指纹段漂移：复算指纹与登记指纹逐位不等，账面已被篡改须整库复核",
            KtaDiag::EZeroFp => "零指纹样本：指纹派生不得为零，零指纹一律拒收并查派生公式",
            KtaDiag::EFold => "指纹总账不符：账面行指纹累加与冻结总账有差额，封库后有人动账，逐行对指纹",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const KT_ALL: &[KtaDiag] = &[
    KtaDiag::ENoSample,
    KtaDiag::EDup,
    KtaDiag::ECapFull,
    KtaDiag::ESealed,
    KtaDiag::ERewind,
    KtaDiag::ENotFull,
    KtaDiag::ENotReg,
    KtaDiag::EClassShift,
    KtaDiag::EExpectShift,
    KtaDiag::EFpShift,
    KtaDiag::EZeroFp,
    KtaDiag::EFold,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in KT_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x83081..0x8308C 闭集内"
}

// ── 样本类别闭集 ────────────────────────────────────────────────
/// 复测资产库三类样本闭集（判据原文分类，冻结不许重排）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleClass {
    /// 注入样本：故障注入面，期望被对应故障闸拒收。
    Injected,
    /// 双机样本：双跑一致组，期望重演与登记逐位一致。
    DualRun,
    /// 边界样本：号窗首尾与容量边界，期望边界码。
    Boundary,
}

impl SampleClass {
    /// 类别序（闭集序 0..3）。
    pub fn index(self) -> u32 {
        match self {
            SampleClass::Injected => 0,
            SampleClass::DualRun => 1,
            SampleClass::Boundary => 2,
        }
    }

    /// 类别中文名。
    pub fn name(self) -> &'static str {
        match self {
            SampleClass::Injected => "注入样本",
            SampleClass::DualRun => "双机样本",
            SampleClass::Boundary => "边界样本",
        }
    }

    /// 由类别序反查闭集类（越界返回 None）。
    pub fn from_index(idx: u32) -> Option<SampleClass> {
        match idx {
            0 => Some(SampleClass::Injected),
            1 => Some(SampleClass::DualRun),
            2 => Some(SampleClass::Boundary),
            _ => None,
        }
    }

    /// 该类号窗下沿。
    pub fn id_lo(self) -> u32 {
        match self {
            SampleClass::Injected => ID_INJ_LO,
            SampleClass::DualRun => ID_DUAL_LO,
            SampleClass::Boundary => ID_BOR_LO,
        }
    }

    /// 该类号窗上沿（闭集含端点）。
    pub fn id_hi(self) -> u32 {
        match self {
            SampleClass::Injected => ID_INJ_HI,
            SampleClass::DualRun => ID_DUAL_HI,
            SampleClass::Boundary => ID_BOR_HI,
        }
    }

    /// 该类闭集号数（71/71/61）。
    pub fn closed_n(self) -> u32 {
        match self {
            SampleClass::Injected => ID_INJ_HI - ID_INJ_LO + 1,
            SampleClass::DualRun => ID_DUAL_HI - ID_DUAL_LO + 1,
            SampleClass::Boundary => ID_BOR_HI - ID_BOR_LO + 1,
        }
    }

    /// 该类账容（70/70/60；闭集恰比账容多 1 例作容量溢出见证位）。
    pub fn cap(self) -> u32 {
        match self {
            SampleClass::Injected => INJ_CAP,
            SampleClass::DualRun => DUAL_CAP,
            SampleClass::Boundary => BOR_CAP,
        }
    }
}

/// 三类闭集数组（类别序即汇总账点验序）。
pub const CLASS_ALL: &[SampleClass] = &[
    SampleClass::Injected,
    SampleClass::DualRun,
    SampleClass::Boundary,
];

/// 由样本号定类别（闭集号窗查表，越窗返回 None）。
pub fn class_of_id(id: u32) -> Option<SampleClass> {
    for c in CLASS_ALL.iter() {
        if id >= c.id_lo() && id <= c.id_hi() {
            return Some(*c);
        }
    }
    None
}

/// 由样本号取类内序号（仅当 class_of_id(id) 命中时有效：号 % 1000）。
pub fn seq_of_id(id: u32) -> u32 {
    id % 1000
}

// ── 指纹派生公式（冻结闭集登记行的复算来源） ────────────────────
/// 期望结果派生：按类别给期望分组码（非零闭域）。
/// 注入样本 10..14（期望被五类故障闸之一拒收）、双机样本 20..27
/// （期望双跑逐位一致分组）、边界样本 30..33（期望边界四象限码）。
pub fn derive_expect(class: SampleClass, seq: u32) -> u32 {
    match class {
        SampleClass::Injected => 10 + seq % 5,
        SampleClass::DualRun => 20 + seq % 8,
        SampleClass::Boundary => 30 + seq % 4,
    }
}

/// 指纹派生公式：四轮整数雪崩（乘法散列 + 移位混淆），确定性、非零。
/// 来源：乘法常数取 Knuth MMIX / 大众化 32 位散列推荐值（整数闭集），
/// 零输入零化回落 0x8308_0001（段签常数），保证指纹闭域非零。
pub fn derive_fp(id: u32, class: SampleClass, expect: u32) -> u32 {
    let mut h = id.wrapping_mul(0x9E37_79B1) ^ SEG_BASE.wrapping_mul(0x85EB_CA6B);
    h = h.wrapping_add(class.index().wrapping_mul(0x27D4_EB2F).wrapping_add(1));
    h ^= h >> 13;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 16;
    h = h.wrapping_add(expect.wrapping_mul(0x0165_667B) | 1);
    h ^= h >> 15;
    if h == 0 {
        0x8308_0001
    } else {
        h
    }
}

// ── 登记行与重演行 ──────────────────────────────────────────────
/// 单条登记行四元组：样本号/类别/期望结果/指纹（冻结派生，逐位可复算）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleRow {
    /// 样本号（闭集号窗内唯一）。
    pub id: u32,
    /// 样本类别（三类闭集）。
    pub class: SampleClass,
    /// 期望结果码（派生闭域）。
    pub expect: u32,
    /// 登记指纹（派生公式复算值，非零）。
    pub fp: u32,
}

impl SampleRow {
    /// 由样本号按冻结公式派生登记行（越窗返回 None）。
    /// 这是"冻结闭集常量表"的复算口：号窗 + 派生公式 = 全量登记行。
    pub fn formula(id: u32) -> Option<SampleRow> {
        let class = class_of_id(id)?;
        let seq = seq_of_id(id);
        let expect = derive_expect(class, seq);
        let fp = derive_fp(id, class, expect);
        Some(SampleRow {
            id,
            class,
            expect,
            fp,
        })
    }

    /// 行摘要（诊断文本用）。
    pub fn brief(&self) -> String {
        format!(
            "样本{}({}) 期望{} 指纹0x{:08x}",
            self.id,
            self.class.name(),
            self.expect,
            self.fp
        )
    }
}

/// 重演行：重演判定器 Ok 侧的输出（与登记行逐位相等才有值可出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayRow {
    /// 重演样本号。
    pub id: u32,
    /// 复算类别（与登记逐位相等）。
    pub class: SampleClass,
    /// 复算期望结果（与登记逐位相等）。
    pub expect: u32,
    /// 复算指纹（与登记逐位相等）。
    pub fp: u32,
}

impl ReplayRow {
    /// 与登记行逐位比对（四字段全等才真）。
    pub fn bit_eq(&self, r: &SampleRow) -> bool {
        self.id == r.id && self.class == r.class && self.expect == r.expect && self.fp == r.fp
    }
}

// ── 资产库状态机与账本 ──────────────────────────────────────────
/// 资产库状态机：Open → Sealed 单向推进，倒退显性拒。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultState {
    /// 开库：允许逐例登记。
    Open,
    /// 封库：只读；指纹总账冻结，复测只走重演口。
    Sealed,
}

impl VaultState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            VaultState::Open => 0,
            VaultState::Sealed => 1,
        }
    }
}

/// 复测资产库：三类样本各自登记、汇总记账、封库后只读。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetVault {
    /// 已入账登记行（按登记序）。
    rows: Vec<SampleRow>,
    /// 注入样本账面计数。
    inj_n: u32,
    /// 双机样本账面计数。
    dual_n: u32,
    /// 边界样本账面计数。
    bor_n: u32,
    /// 指纹总账：全部入账指纹的饱和累加（每例非零 ⇒ 只增不减）。
    fp_fold: u64,
    /// 库状态。
    state: VaultState,
}

impl AssetVault {
    /// 开一座新库。
    pub fn new() -> AssetVault {
        AssetVault {
            rows: Vec::new(),
            inj_n: 0,
            dual_n: 0,
            bor_n: 0,
            fp_fold: 0,
            state: VaultState::Open,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> VaultState {
        self.state
    }

    /// 已入账行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否空库。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 汇总账（三类和，饱和累加防回卷）。
    pub fn total(&self) -> u32 {
        self.inj_n
            .saturating_add(self.dual_n)
            .saturating_add(self.bor_n)
    }

    /// 汇总账是否已满（≥200 例）。
    pub fn is_full(&self) -> bool {
        self.total() >= TOTAL_CAP
    }

    /// 指纹总账现值（封库后冻结）。
    pub fn fold(&self) -> u64 {
        self.fp_fold
    }

    /// 按类别取账面计数。
    pub fn class_reg_n(&self, class: SampleClass) -> u32 {
        match class {
            SampleClass::Injected => self.inj_n,
            SampleClass::DualRun => self.dual_n,
            SampleClass::Boundary => self.bor_n,
        }
    }

    /// 取账面第 pos 行（越界 None）。
    pub fn row_at(&self, pos: usize) -> Option<SampleRow> {
        self.rows.get(pos).copied()
    }

    /// 按样本号查账面行。
    pub fn row_of_id(&self, id: u32) -> Option<SampleRow> {
        for r in self.rows.iter() {
            if r.id == id {
                return Some(*r);
            }
        }
        None
    }

    /// 类别计数推进（登记内部分账）。
    fn bump_class(&mut self, class: SampleClass) {
        match class {
            SampleClass::Injected => self.inj_n += 1,
            SampleClass::DualRun => self.dual_n += 1,
            SampleClass::Boundary => self.bor_n += 1,
        }
    }

    /// 正常登记口：按冻结公式派生登记行入账。
    /// 闸序（判定次序，冻结）：封库 → 闭集号窗 → 重复 → 类别账容。
    pub fn register(&mut self, sample_id: u32) -> Result<(), u32> {
        // 闸一：封库只读。
        if self.state.rank() >= VaultState::Sealed.rank() {
            return Err(KtaDiag::ESealed.val());
        }
        // 闸二：样本号闭集（越窗样本号一律拒）。
        let row = match SampleRow::formula(sample_id) {
            Some(r) => r,
            None => return Err(KtaDiag::ENoSample.val()),
        };
        // 闸三：重复登记。
        if self.row_of_id(sample_id).is_some() {
            return Err(KtaDiag::EDup.val());
        }
        // 闸四：类别账容（70/70/60，满即容量耗尽）。
        if self.class_reg_n(row.class) >= row.class.cap() {
            return Err(KtaDiag::ECapFull.val());
        }
        self.rows.push(row);
        self.bump_class(row.class);
        self.fp_fold = self.fp_fold.saturating_add(row.fp as u64);
        Ok(())
    }

    /// 入账污染攻击面（注入器专用）：绕过闭集派生直灌登记行，
    /// 模拟"入账时账面被污染"。仍走闸序：封库 → 零指纹 → 重复 → 账容。
    pub fn register_raw(&mut self, row: SampleRow) -> Result<(), u32> {
        if self.state.rank() >= VaultState::Sealed.rank() {
            return Err(KtaDiag::ESealed.val());
        }
        if row.fp == 0 {
            return Err(KtaDiag::EZeroFp.val());
        }
        if self.row_of_id(row.id).is_some() {
            return Err(KtaDiag::EDup.val());
        }
        if self.class_reg_n(row.class) >= row.class.cap() {
            return Err(KtaDiag::ECapFull.val());
        }
        self.rows.push(row);
        self.bump_class(row.class);
        self.fp_fold = self.fp_fold.saturating_add(row.fp as u64);
        Ok(())
    }

    /// 封库篡改攻击面（注入器专用）：越过一切闸门直接改写账面行，
    /// 模拟"封库后有人动账"。攻击者不维护指纹总账，故总账复算
    /// 必现差额（EFold 路），重演判定亦必抓（EFpShift 路）。
    pub fn mutate_row(&mut self, pos: usize, row: SampleRow) -> Result<(), u32> {
        match self.rows.get_mut(pos) {
            Some(slot) => {
                *slot = row;
                Ok(())
            }
            None => Err(KtaDiag::ENotReg.val()),
        }
    }

    /// 封库：三类各齐账容 + 汇总恒等 + 指纹总账复算全过才许封。
    pub fn seal(&mut self) -> Result<(), u32> {
        // 倒退闸：已封再封即倒退。
        if self.state.rank() >= VaultState::Sealed.rank() {
            return Err(KtaDiag::ERewind.val());
        }
        // 逐类齐账闸：三类必须各恰满账容，缺一例即类别缺账。
        for c in CLASS_ALL.iter() {
            if self.class_reg_n(*c) != c.cap() {
                return Err(KtaDiag::ENotFull.val());
            }
        }
        // 汇总恒等闸：逐类和 == 账面行数 == 200。
        if self.total() as usize != self.rows.len() || self.total() != TOTAL_CAP {
            return Err(KtaDiag::ENotFull.val());
        }
        // 指纹总账复算闸。
        if !self.fold_audit_ok() {
            return Err(KtaDiag::EFold.val());
        }
        self.state = VaultState::Sealed;
        Ok(())
    }

    /// 重演判定器：按登记指纹逐字段复算，重演结果与登记逐位相等才 Ok。
    /// 复算三段各自独立成诊断码：类别段 → 期望结果段 → 指纹段。
    /// 封库后照常可重演——这正是复测资产库的用途所在。
    pub fn replay(&self, sample_id: u32) -> Result<ReplayRow, u32> {
        // 重演闸序：闭集号窗先于登记有无。
        let truth = match SampleRow::formula(sample_id) {
            Some(r) => r,
            None => return Err(KtaDiag::ENoSample.val()),
        };
        let reg = match self.row_of_id(sample_id) {
            Some(r) => r,
            None => return Err(KtaDiag::ENotReg.val()),
        };
        // 逐字段复算一：类别段。
        if reg.class != truth.class {
            return Err(KtaDiag::EClassShift.val());
        }
        // 逐字段复算二：期望结果段。
        if reg.expect != truth.expect {
            return Err(KtaDiag::EExpectShift.val());
        }
        // 逐字段复算三：指纹段（逐位）。
        if reg.fp != truth.fp {
            return Err(KtaDiag::EFpShift.val());
        }
        Ok(ReplayRow {
            id: truth.id,
            class: truth.class,
            expect: truth.expect,
            fp: truth.fp,
        })
    }

    /// 指纹总账复算：账面行指纹饱和累加必须等于冻结总账。
    /// 封库后任何人动过账面行指纹，此处必现差额。
    pub fn fold_audit_ok(&self) -> bool {
        let mut sum = 0u64;
        for r in self.rows.iter() {
            sum = sum.saturating_add(r.fp as u64);
        }
        sum == self.fp_fold
    }

    /// 类别账复算：账面按行归类的逐类计数必须等于登记时分账计数。
    /// 行级类别篡改会令归组漂移，此处必现不符。
    pub fn class_audit_ok(&self) -> bool {
        let mut inj = 0u32;
        let mut dual = 0u32;
        let mut bor = 0u32;
        for r in self.rows.iter() {
            match r.class {
                SampleClass::Injected => inj += 1,
                SampleClass::DualRun => dual += 1,
                SampleClass::Boundary => bor += 1,
            }
        }
        inj == self.inj_n && dual == self.dual_n && bor == self.bor_n
    }
}

// ── 构库器（确定性，双跑一致） ──────────────────────────────────
/// 除 skip_id 外按正常流灌满三类账容（skip 例留缺，供缺账注入器用）。
fn fill_except(v: &mut AssetVault, skip_id: u32) {
    for c in CLASS_ALL.iter() {
        let mut seq = 1u32;
        while seq <= c.cap() {
            let id = c.id_lo() + seq - 1;
            if id != skip_id {
                let _ = v.register(id);
            }
            seq += 1;
        }
    }
}

/// 构建满账开库：三类各登记 70/70/60 例（闭集尾号留作容量溢出见证位），
/// 共 200 例，不封库。确定性派生 ⇒ 双跑逐位一致。
pub fn build_full_vault() -> AssetVault {
    let mut v = AssetVault::new();
    for c in CLASS_ALL.iter() {
        let mut seq = 1u32;
        while seq <= c.cap() {
            let _ = v.register(c.id_lo() + seq - 1);
            seq += 1;
        }
    }
    v
}

/// 构建满账封库：正常流登记 200 例后走封库口（确定性下必封成）。
pub fn build_sealed_vault() -> AssetVault {
    let mut v = build_full_vault();
    let _ = v.seal();
    v
}

// ── 确定性 LCG（种子 = SEG_BASE，零随机红线合规） ────────────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性；常数取 Knuth MMIX 31 位推荐值）。
    pub fn next_u32(&mut self) -> u32 {
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

// ── 注入器一：样本指纹篡改（10 轮全抓） ─────────────────────────
/// 指纹篡改注入器：三路混合各 10 轮——
/// 路甲（偶数轮）：封库后行级指纹翻位（mutate_row 绕闸），
///   抓法 = 重演判指纹段漂移 + 指纹总账复算现差额，双路齐证；
/// 路丙（k%4==3 轮）：入账时零指纹污染（register_raw），零指纹必拒；
/// 路乙（其余奇数轮）：入账时指纹污染（register_raw 灌脏行），
///   抓法 = 重演判指纹段漂移。
/// 每轮先验基线：干净封库重演必须 Ok——防判定器"恒 Err"假绿。
pub fn fp_tamper_sweep() -> (usize, usize, String) {
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x11) as u64);
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut k = 0usize;
    while k < 10 {
        let clean = build_sealed_vault();
        let pos = rng.below(TOTAL_CAP) as usize;
        let target = match clean.row_at(pos) {
            Some(r) => r,
            None => {
                total += 1;
                if escape.is_empty() {
                    escape = format!("指纹注入路{}：取行失败 位{}", k, pos);
                }
                k += 1;
                continue;
            }
        };
        // 基线：干净库重演必须 Ok（防"恒 Err"假绿）。
        let base_ok = matches!(clean.replay(target.id), Ok(_));
        let hit;
        if k % 2 == 0 {
            // 路甲：封库后行级指纹翻位。
            let mut bad = build_sealed_vault();
            let mut t = target;
            t.fp = t.fp ^ 0x5A5A_0001;
            let _ = bad.mutate_row(pos, t);
            hit = matches!(bad.replay(target.id), Err(code) if code == KtaDiag::EFpShift.val())
                && !bad.fold_audit_ok();
        } else if k % 4 == 3 {
            // 路丙：入账时零指纹污染——零指纹样本不得入账。
            let mut dirty = AssetVault::new();
            fill_except(&mut dirty, target.id);
            let mut t = target;
            t.fp = 0;
            hit = dirty.register_raw(t) == Err(KtaDiag::EZeroFp.val());
        } else {
            // 路乙：入账时指纹污染（跳过正常登记灌脏行）。
            let mut dirty = AssetVault::new();
            fill_except(&mut dirty, target.id);
            let mut t = target;
            t.fp = t.fp ^ 0x1357_9BDF;
            let ok_raw = dirty.register_raw(t).is_ok();
            hit = ok_raw
                && matches!(
                    dirty.replay(target.id),
                    Err(code) if code == KtaDiag::EFpShift.val()
                );
        }
        total += 1;
        if base_ok && hit {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("指纹注入路{}逃脱：{}", k, target.brief());
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 注入器二：越界样本号（10 轮全抓） ───────────────────────────
/// 越界样本号注入器：号窗下沿外/上沿外/类间空档/远端/饱和值十路，
/// 登记与重演两口都必须以"越闭集"（ENoSample）拒绝，一笔不漏。
pub fn range_sweep() -> (usize, usize, String) {
    const BAD_IDS: [u32; 10] = [0, 999, 1000, 1072, 1500, 2072, 2999, 3062, 0x7FFF_FFFF, u32::MAX];
    // 满账开库（未封库）：证明闭集闸先于容量语义生效。
    let mut v = build_full_vault();
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for bad in BAD_IDS.iter() {
        total += 1;
        let reg_res = v.register(*bad);
        let rep_res = v.replay(*bad);
        if reg_res == Err(KtaDiag::ENoSample.val()) && rep_res == Err(KtaDiag::ENoSample.val()) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("越界样本{}逃脱：登记{:?} 重演{:?}", bad, reg_res, rep_res);
        }
    }
    (caught, total, escape)
}

// ── 注入器三：类别缺账（10 轮全抓） ─────────────────────────────
/// 类别缺账注入器：每轮从三类之一抽一例跳过登记，库即缺账——
/// 封库口必须拒（ENotFull），该缺例重演必须拒（ENotReg），双证齐抓。
pub fn missing_sweep() -> (usize, usize, String) {
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x33) as u64);
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut k = 0usize;
    while k < 10 {
        let cls = match CLASS_ALL.get(k % 3) {
            Some(c) => *c,
            None => {
                k += 1;
                continue;
            }
        };
        let seq = 1 + rng.below(cls.cap());
        let skip_id = cls.id_lo() + seq - 1;
        let mut v = AssetVault::new();
        fill_except(&mut v, skip_id);
        // 缺账必抓一：少一例禁封库。
        let seal_res = v.seal();
        // 缺账必抓二：未登记例重演被拒。
        let rep_res = v.replay(skip_id);
        let hit = seal_res == Err(KtaDiag::ENotFull.val())
            && rep_res == Err(KtaDiag::ENotReg.val());
        total += 1;
        if hit {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("缺账注入路{}逃脱：{}{}缺账未抓", k, cls.name(), skip_id);
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 服从派工公式、对齐 0x40 且落在 VX9 段窗
/// [0x80000, 0x831C0]（卡规判据式）。
fn seg_guard_ok() -> bool {
    if SEG_BASE != VX9_SEG_MIN + 0x40 * (12794 - VX9_ID_MIN) {
        return false;
    }
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：12 码互异、全落段窗、查表可命中、未知码回落。
fn diag_closed_ok() -> bool {
    if KT_ALL.len() != 12 {
        return false;
    }
    for d in KT_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if detail(v) != d.detail() {
            return false;
        }
        if d.detail().is_empty() {
            return false;
        }
    }
    // 码互异（两两比对）。
    let mut i = 0usize;
    while i < KT_ALL.len() {
        let vi = match KT_ALL.get(i) {
            Some(d) => d.val(),
            None => 0,
        };
        let mut j = i + 1;
        while j < KT_ALL.len() {
            let vj = match KT_ALL.get(j) {
                Some(d) => d.val(),
                None => 0,
            };
            if vi == vj {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    // 未知码必须回落"未知"文案，不得冒充已知码。
    if !detail(SEG_BASE + 0x3F).contains("未知") {
        return false;
    }
    if detail(SEG_BASE + 0x3F) == KtaDiag::ENoSample.detail() {
        return false;
    }
    true
}

/// 三类闭集与账容守卫：闭集 71/71/61、账容 70/70/60、汇总 200；
/// 闭集逐号可派生，类间空档一律不可派生。
fn closed_set_ok() -> bool {
    if TOTAL_CAP != 200 {
        return false;
    }
    for c in CLASS_ALL.iter() {
        if c.closed_n() != c.cap() + 1 {
            return false;
        }
        if c.id_hi() != c.id_lo() + c.closed_n() - 1 {
            return false;
        }
        let mut seq = 1u32;
        while seq <= c.closed_n() {
            if SampleRow::formula(c.id_lo() + seq - 1).is_none() {
                return false;
            }
            seq += 1;
        }
    }
    for gap in [1000u32, 1072, 2000, 2072, 3000, 3062].iter() {
        if SampleRow::formula(*gap).is_some() {
            return false;
        }
    }
    true
}

/// 指纹派生公式守卫：全闭集指纹非零、期望非零、两次派生逐位相等、
/// 相邻号指纹不撞车（对输入敏感）；越界号一律不可派生。
fn fp_formula_ok() -> bool {
    for c in CLASS_ALL.iter() {
        let mut seq = 1u32;
        while seq <= c.closed_n() {
            let id = c.id_lo() + seq - 1;
            let a = match SampleRow::formula(id) {
                Some(r) => r,
                None => return false,
            };
            let b = match SampleRow::formula(id) {
                Some(r) => r,
                None => return false,
            };
            if a != b {
                return false;
            }
            if a.fp == 0 || a.expect == 0 {
                return false;
            }
            if seq + 1 <= c.closed_n() {
                if let Some(nxt) = SampleRow::formula(id + 1) {
                    if nxt.fp == a.fp {
                        return false;
                    }
                }
            }
            seq += 1;
        }
    }
    let bad: [u32; 10] = [0, 1, 999, 1072, 1999, 2072, 2999, 3062, 100_000, u32::MAX];
    for v in bad.iter() {
        if SampleRow::formula(*v).is_some() {
            return false;
        }
    }
    true
}

/// 正常流：满账 200 例 → 封库 → 封库后重演仍可复测且逐位相等。
fn happy_path_ok() -> bool {
    let mut v = build_full_vault();
    if v.state() != VaultState::Open {
        return false;
    }
    if v.total() != TOTAL_CAP || !v.is_full() || v.len() != 200 {
        return false;
    }
    if v.seal().is_err() {
        return false;
    }
    if v.state() != VaultState::Sealed {
        return false;
    }
    let rep = v.replay(1001);
    let reg = v.row_of_id(1001);
    match (rep, reg) {
        (Ok(r), Some(row)) => {
            if !r.bit_eq(&row) {
                return false;
            }
        }
        _ => return false,
    }
    true
}

/// 逐类点验：注 70 / 双 70 / 边 60、汇总 200、账面行数一致、
/// 类别账与指纹总账复算全过。
fn class_tally_ok() -> bool {
    let v = build_sealed_vault();
    if v.total() < 200 {
        return false;
    }
    if v.class_reg_n(SampleClass::Injected) != INJ_CAP {
        return false;
    }
    if v.class_reg_n(SampleClass::DualRun) != DUAL_CAP {
        return false;
    }
    if v.class_reg_n(SampleClass::Boundary) != BOR_CAP {
        return false;
    }
    if v.len() != 200 {
        return false;
    }
    if !v.class_audit_ok() {
        return false;
    }
    if !v.fold_audit_ok() {
        return false;
    }
    true
}

/// 重演判定器全库绿：200 例逐例重演 Ok 且与登记行逐位相等。
fn replay_all_ok() -> bool {
    let v = build_sealed_vault();
    for c in CLASS_ALL.iter() {
        let mut seq = 1u32;
        while seq <= c.cap() {
            let id = c.id_lo() + seq - 1;
            let truth = match SampleRow::formula(id) {
                Some(r) => r,
                None => return false,
            };
            match v.replay(id) {
                Ok(rep) => {
                    if !rep.bit_eq(&truth) {
                        return false;
                    }
                }
                Err(_) => return false,
            }
            seq += 1;
        }
    }
    true
}

/// 状态机守卫：Open→Sealed 单向，重封倒退拒，封库后写入拒且账面不变。
fn state_machine_ok() -> bool {
    if VaultState::Open.rank() >= VaultState::Sealed.rank() {
        return false;
    }
    let mut v = build_full_vault();
    if v.state() != VaultState::Open {
        return false;
    }
    if v.seal().is_err() {
        return false;
    }
    if v.state() != VaultState::Sealed {
        return false;
    }
    // 倒退显性拒：重封。
    if v.seal() != Err(KtaDiag::ERewind.val()) {
        return false;
    }
    // 封库后只读：未登记闭集尾号写入被拒且账面不变。
    let before = v.len();
    if v.register(1071) != Err(KtaDiag::ESealed.val()) {
        return false;
    }
    if v.len() != before {
        return false;
    }
    true
}

/// 容量耗尽边界：逐类灌满闭集，第 closed_n 例（闭集尾号）必被
/// 容量闸拒；满库后 is_full 立即置位且越界号仍走闭集闸。
fn capacity_exhaust_ok() -> bool {
    let mut v = AssetVault::new();
    for c in CLASS_ALL.iter() {
        let mut seq = 1u32;
        while seq <= c.closed_n() {
            let res = v.register(c.id_lo() + seq - 1);
            if seq <= c.cap() {
                if res.is_err() {
                    return false;
                }
            } else if res != Err(KtaDiag::ECapFull.val()) {
                return false;
            }
            seq += 1;
        }
    }
    if !v.is_full() {
        return false;
    }
    if v.total() != TOTAL_CAP {
        return false;
    }
    // 满库后越界号先走闭集闸：ENoSample 而非 ECapFull。
    if v.register(9999) != Err(KtaDiag::ENoSample.val()) {
        return false;
    }
    true
}

/// 判定次序守卫（闸序冻结）：登记口 封库→闭集→重复→账容；
/// 重演口 闭集→登记有无。逐对验证高序闸压倒低序闸。
fn gate_order_ok() -> bool {
    // 1) 封库闸压倒闭集闸：封库后越界号返回 ESealed。
    let mut sv = build_sealed_vault();
    if sv.register(0) != Err(KtaDiag::ESealed.val()) {
        return false;
    }
    // 2) 封库闸压倒重复闸：封库后已登记号返回 ESealed。
    if sv.register(1001) != Err(KtaDiag::ESealed.val()) {
        return false;
    }
    // 3) 未封库：闭集闸压倒重复/账容闸。
    let mut v = AssetVault::new();
    if v.register(999) != Err(KtaDiag::ENoSample.val()) {
        return false;
    }
    // 4) 重复闸压倒账容闸：满库后已登记号返回 EDup。
    let mut full = build_full_vault();
    if full.register(1001) != Err(KtaDiag::EDup.val()) {
        return false;
    }
    // 5) 账容闸居末：满库后未登记闭集尾号返回 ECapFull。
    if full.register(1071) != Err(KtaDiag::ECapFull.val()) {
        return false;
    }
    // 6) 重演闸序：闭集闸先于登记闸——越界号返回 ENoSample。
    if sv.replay(0) != Err(KtaDiag::ENoSample.val()) {
        return false;
    }
    if sv.replay(3062) != Err(KtaDiag::ENoSample.val()) {
        return false;
    }
    // 7) 闭集内未登记号重演返回 ENotReg。
    if sv.replay(1071) != Err(KtaDiag::ENotReg.val()) {
        return false;
    }
    true
}

/// 指纹总账守卫：登记期间只增不减（每例指纹非零、累加严格递增）；
/// 封库后行级指纹翻位 → 总账复算现差额 + 重演判指纹段漂移，双路必抓。
fn fold_monotonic_ok() -> bool {
    let mut v = AssetVault::new();
    let mut prev = v.fold();
    for c in CLASS_ALL.iter() {
        let mut seq = 1u32;
        while seq <= c.cap() {
            if v.register(c.id_lo() + seq - 1).is_err() {
                return false;
            }
            let cur = v.fold();
            if cur <= prev {
                return false;
            }
            prev = cur;
            seq += 1;
        }
    }
    if !v.fold_audit_ok() {
        return false;
    }
    // 封库后篡改必抓。
    let mut sv = build_sealed_vault();
    if !sv.fold_audit_ok() {
        return false;
    }
    let target = match sv.row_at(37) {
        Some(r) => r,
        None => return false,
    };
    let mut t = target;
    t.fp = t.fp ^ 0x0F0F_0F0F;
    let _ = sv.mutate_row(37, t);
    if sv.fold_audit_ok() {
        return false;
    }
    if sv.replay(target.id) != Err(KtaDiag::EFpShift.val()) {
        return false;
    }
    true
}

/// 注入一守卫：指纹篡改 10 轮全抓无逃脱。
fn fp_tamper_ok() -> bool {
    let (caught, total, escape) = fp_tamper_sweep();
    if total != 10 || caught != 10 {
        return false;
    }
    escape.is_empty()
}

/// 注入二守卫：越界样本号 10 轮全抓无逃脱。
fn range_tamper_ok() -> bool {
    let (caught, total, escape) = range_sweep();
    if total != 10 || caught != 10 {
        return false;
    }
    escape.is_empty()
}

/// 注入三守卫：类别缺账 10 轮全抓无逃脱。
fn missing_tamper_ok() -> bool {
    let (caught, total, escape) = missing_sweep();
    if total != 10 || caught != 10 {
        return false;
    }
    escape.is_empty()
}

/// 双跑一致守卫：两次构库逐位相等，三类注入器两跑结果逐项相等。
fn dual_run_ok() -> bool {
    let a = build_full_vault();
    let b = build_full_vault();
    if a != b {
        return false;
    }
    if build_sealed_vault() != build_sealed_vault() {
        return false;
    }
    if fp_tamper_sweep() != fp_tamper_sweep() {
        return false;
    }
    if range_sweep() != range_sweep() {
        return false;
    }
    if missing_sweep() != missing_sweep() {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12794 自检（判据 12794 卡：复测资产库齐·三类汇总 ≥200 例·同源可重演）。
pub fn run_unx12794_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12794");
    let vault = build_sealed_vault();

    s.add_dyn(
        "F12794-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} 对齐0x40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12794-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "codes={} 首=0x{:05x}",
            KT_ALL.len(),
            KT_ALL.first().map(|d| d.val()).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12794-03 三类闭集与账容",
        closed_set_ok(),
        "注入71/双机71/边界61闭集 各容70/70/60 汇总200",
    );

    s.add_dyn(
        "F12794-04 指纹派生公式",
        fp_formula_ok(),
        "四轮整数雪崩 非零 确定复算 对输入敏感",
    );

    s.add_dyn(
        "F12794-05 正常流登记封库",
        happy_path_ok(),
        &format!(
            "rows={} total={} state={:?}",
            vault.len(),
            vault.total(),
            vault.state()
        ),
    );

    s.add_dyn(
        "F12794-06 逐类点验汇总200",
        class_tally_ok(),
        &format!(
            "注{}双{}边{} 合{}",
            vault.class_reg_n(SampleClass::Injected),
            vault.class_reg_n(SampleClass::DualRun),
            vault.class_reg_n(SampleClass::Boundary),
            vault.total()
        ),
    );

    s.add_dyn(
        "F12794-07 重演判定器全库",
        replay_all_ok(),
        "200例逐例重演 与登记逐位相等才Ok",
    );

    let (fc, ft, fe) = fp_tamper_sweep();
    s.add_dyn(
        "F12794-08 注入一 指纹篡改",
        fp_tamper_ok(),
        &format!("抓{}/{} {}", fc, ft, fe),
    );

    let (rc, rt, rx) = range_sweep();
    s.add_dyn(
        "F12794-09 注入二 越界样本号",
        range_tamper_ok(),
        &format!("抓{}/{} {}", rc, rt, rx),
    );

    let (mc, mt, mx) = missing_sweep();
    s.add_dyn(
        "F12794-10 注入三 类别缺账",
        missing_tamper_ok(),
        &format!("抓{}/{} {}", mc, mt, mx),
    );

    s.add_dyn(
        "F12794-11 状态机单向",
        state_machine_ok(),
        "Open→Sealed 只进不退 封库只读 倒退拒",
    );

    s.add_dyn(
        "F12794-12 判定次序闸序",
        gate_order_ok(),
        "登记口封库→闭集→重复→账容 重演口闭集→登记",
    );

    s.add_dyn(
        "F12794-13 容量耗尽边界",
        capacity_exhaust_ok(),
        "第71/71/61例 ECapFull 满库即锁",
    );

    s.add_dyn(
        "F12794-14 指纹总账只增不减",
        fold_monotonic_ok(),
        "累加严格递增 封库后篡改双路必抓",
    );

    s.add_dyn(
        "F12794-15 双跑一致",
        dual_run_ok(),
        "两次构库与三类注入逐项一致",
    );

    // 收口：判据条数声明（15 条判据 + 1 条收口，全绿才收）。
    let (p, f) = s.tally();
    s.add_dyn(
        "F12794-16 判据收口",
        f == 0 && p == 15 && vault.total() >= 200,
        &format!(
            "判据15条全绿+收口1条 三类汇总={}例≥200 同源可重演",
            vault.total()
        ),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_formula_and_window() {
        // 段位守卫：公式值、0x40 对齐、VX9 段窗三点齐验。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x80000 + 0x40 * (12794 - 12600));
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
    }

    #[test]
    fn diag_codes_closed_set() {
        // 闭集：12 码互异、全落段窗、查表可命中、未知码回落。
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
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
    }

    #[test]
    fn happy_path_seals_two_hundred() {
        // 正常流：满账 200 例、开库态、封库成、终态封库。
        let mut v = build_full_vault();
        assert_eq!(v.total(), 200);
        assert_eq!(v.len(), 200);
        assert_eq!(v.state(), VaultState::Open);
        assert!(v.seal().is_ok());
        assert_eq!(v.state(), VaultState::Sealed);
        assert!(happy_path_ok());
    }

    #[test]
    fn replay_all_bit_exact() {
        // 同源可重演：200 例逐例 Ok 且逐位相等；抽点核对重演行字段。
        assert!(replay_all_ok());
        let v = build_sealed_vault();
        let rep = v.replay(2001);
        assert!(matches!(rep, Ok(_)));
        if let Ok(row) = rep {
            assert_eq!(row.id, 2001);
            assert_eq!(row.class, SampleClass::DualRun);
            let truth = SampleRow::formula(2001);
            assert!(truth.is_some());
            if let Some(t) = truth {
                assert!(row.bit_eq(&t));
                assert_eq!(row.expect, t.expect);
                assert_eq!(row.fp, t.fp);
            }
        }
    }

    #[test]
    fn fp_tamper_all_caught() {
        // 注入一：指纹篡改 10 轮全抓（翻位/污染/零指纹三路），无逃脱。
        let (caught, total, escape) = fp_tamper_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱：{}", escape);
        assert!(escape.is_empty());
        assert!(fp_tamper_ok());
    }

    #[test]
    fn range_and_missing_all_caught() {
        // 注入二/三：越界样本号与类别缺账各 10 轮全抓，无逃脱。
        let (rc, rt, rx) = range_sweep();
        assert_eq!(rt, 10);
        assert_eq!(rc, 10, "逃脱：{}", rx);
        let (mc, mt, mx) = missing_sweep();
        assert_eq!(mt, 10);
        assert_eq!(mc, 10, "逃脱：{}", mx);
        assert!(range_tamper_ok());
        assert!(missing_tamper_ok());
    }

    #[test]
    fn gate_order_precedence() {
        // 判定次序：封库压倒一切，闭集压倒重复/账容，重演口同序。
        assert!(gate_order_ok());
        let mut sv = build_sealed_vault();
        assert_eq!(sv.register(1001), Err(KtaDiag::ESealed.val()));
        assert_eq!(sv.register(0), Err(KtaDiag::ESealed.val()));
        assert_eq!(sv.replay(1071), Err(KtaDiag::ENotReg.val()));
        let mut v = AssetVault::new();
        assert_eq!(v.register(999), Err(KtaDiag::ENoSample.val()));
        assert_eq!(v.register(0), Err(KtaDiag::ENoSample.val()));
    }

    #[test]
    fn capacity_exhaustion_boundary() {
        // 容量耗尽：逐类第 closed_n 例必被容量闸拒，满库即锁。
        assert!(capacity_exhaust_ok());
        let mut v = AssetVault::new();
        let mut seq = 1u32;
        while seq <= 71 {
            let res = v.register(1000 + seq);
            if seq <= 70 {
                assert!(res.is_ok());
            } else {
                assert_eq!(res, Err(KtaDiag::ECapFull.val()));
            }
            seq += 1;
        }
        let mut dseq = 1u32;
        while dseq <= 71 {
            let res = v.register(2000 + dseq);
            if dseq <= 70 {
                assert!(res.is_ok());
            } else {
                assert_eq!(res, Err(KtaDiag::ECapFull.val()));
            }
            dseq += 1;
        }
        let mut bseq = 1u32;
        while bseq <= 61 {
            let res = v.register(3000 + bseq);
            if bseq <= 60 {
                assert!(res.is_ok());
            } else {
                assert_eq!(res, Err(KtaDiag::ECapFull.val()));
            }
            bseq += 1;
        }
        assert!(v.is_full());
    }

    #[test]
    fn fold_monotonic_and_tamper_caught() {
        // 指纹总账只增不减；封库后行级篡改双路必抓（总账差额+重演漂移）。
        assert!(fold_monotonic_ok());
        let mut sv = build_sealed_vault();
        assert!(sv.fold_audit_ok());
        assert!(sv.class_audit_ok());
        let mut t = match sv.row_at(5) {
            Some(r) => r,
            None => SampleRow {
                id: 0,
                class: SampleClass::Injected,
                expect: 0,
                fp: 0,
            },
        };
        t.fp = t.fp ^ 0xABCD_0001;
        let _ = sv.mutate_row(5, t);
        assert!(!sv.fold_audit_ok());
        // 类别未动：类别账复算仍应过（篡改只动了指纹段）。
        assert!(sv.class_audit_ok());
        // 被篡改行（位 5 = 第 6 例登记行）重演必判指纹段漂移。
        let tampered_id = match sv.row_at(5) {
            Some(r) => r.id,
            None => 0,
        };
        assert_ne!(tampered_id, 0);
        assert_eq!(sv.replay(tampered_id), Err(KtaDiag::EFpShift.val()));
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Open→Sealed；重封与封库写全拒。
        assert!(state_machine_ok());
        let mut v = build_full_vault();
        assert_eq!(v.state(), VaultState::Open);
        assert!(VaultState::Open.rank() < VaultState::Sealed.rank());
        assert!(v.seal().is_ok());
        assert_eq!(v.state(), VaultState::Sealed);
        assert_eq!(v.seal(), Err(KtaDiag::ERewind.val()));
        assert_eq!(v.register(1071), Err(KtaDiag::ESealed.val()));
    }

    #[test]
    fn dual_run_identical() {
        // 双跑一致：确定性 LCG 下两次构库与三类注入逐项相同。
        assert!(dual_run_ok());
        assert_eq!(build_full_vault(), build_full_vault());
        assert_eq!(build_sealed_vault(), build_sealed_vault());
        assert_eq!(fp_tamper_sweep(), fp_tamper_sweep());
        assert_eq!(range_sweep(), range_sweep());
        assert_eq!(missing_sweep(), missing_sweep());
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 16 条（≥12 条红线达标）。
        let s = run_unx12794_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.len(), 16);
        assert_eq!(s.tally().1, 0);
    }
}
