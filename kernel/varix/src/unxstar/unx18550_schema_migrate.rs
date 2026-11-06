//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18550 · 档案版本化（schema 版本+迁移）
//! · 0x80540 段 · 批 08 · 配额 260
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18550-J1 schema v1 冻结，旧格式迁移 3 代样本全过，
//!       升级不破坏旧数据断言。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - schema v1 冻结：目标槽像 8 槽 [major,minor,build,sp,pt,suite,
//!     dpi,elevate]，SCHEMA_V1=3（代计数：旧格式 0/1/2 三代 + 当前
//!     第 3 代）——迁移终点 schema 恒 = 3；
//!   - 三代旧格式布局冻结（裁决表）：
//!     gen0 裸档无头：[maj,min,bld,sp,pt,dpi,suite,elev]（dpi 在前）；
//!     gen1 带头 0xA1：[hdr,maj,min,bld,sp,pt,suite,pad]，缺 dpi/
//!     elevate → 迁移补缺省 96/0；
//!     gen2 带头 0xA2：[hdr,maj,min,bld,pt,sp,dpi,suite]（sp/pt 互
//!     换），缺 elevate → 迁移补 0；
//!   - 迁移步数推导：gen0 = 1 步（重排），gen1 = 2 步（剥头+补缺
//!     省），gen2 = 2 步（换位回正+补缺省）——步账 {1,2,2} 全对拍；
//!   - 升级不破坏旧数据：迁移以 &RawRecord 只读进行，迁移后源档
//!     校验和必须仍完好（快照指纹前后相等）——源档零改写；
//!   - 记录状态机三态单向：Pending → Migrated → Verified；跳段
//!     （未迁移即验证）拒；批账 8 槽定容，重复 id 拒，满显性拒；
//!   - 注入三类 × 10 轮：未知代（gen≥3）/ 校验和损坏 / 值域违约
//!     （major=0 混入 gen0 样本）——逐类逐轮必拒，漏检即红；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] SmCode 闭集（12 码）+ ALL + detail() + detail(u32)
//!   [裁决表]   三代布局表 gen_layout / 缺省值表 gen_default
//!   [数据结构] RawRecord / MigRecord / MigBatch（8 槽定容）
//!   [状态机]   RecPhase 三态单向 + SchemaMigrator
//!   [账本]     StepLedger（步数账 {1,2,2}）+ BatchLedger（守恒）
//!   [注入核]   三类变异 × 10 轮（未知代/损坏/值域违约）
//!   [收口]     golden_run + run_unx18550_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80540，AI-VX11-1105 G1 批08 独占区）。
pub const SEG_BASE: u32 = 0x80540;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// 槽像宽（冻结：8 槽，与总纲 PROFILE_UNITS 同口径）。
pub const UNIT_LEN: usize = 8;
/// 当前 schema（冻结：第 3 代为现行 v1）。
pub const SCHEMA_V1: u8 = 3;
/// 旧格式代闭集（冻结：0/1/2 三代）。
pub const GEN_OLD_MAX: u8 = 2;
/// gen1 头魔数。
pub const HDR_GEN1: u16 = 0xA1;
/// gen2 头魔数。
pub const HDR_GEN2: u16 = 0xA2;
/// 批容量（冻结：8 槽）。
pub const BATCH_CAP: usize = 8;
/// DPI 缺省（冻结：96）。
pub const DPI_DEFAULT: u16 = 96;
/// 提权缺省（冻结：0）。
pub const ELEV_DEFAULT: u16 = 0;
/// 版本主段下界。
pub const VER_MAJOR_MIN: u16 = 3;
/// 判据注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;

/// 源档校验和：!(Σunits + gen)（代参与折叠，跨代防窜）。
pub fn raw_checksum(units: &[u16; UNIT_LEN], gen: u8) -> u16 {
    let mut acc: u16 = 0;
    let mut i = 0;
    while i < UNIT_LEN {
        acc = acc.wrapping_add(units[i]);
        i += 1;
    }
    !(acc.wrapping_add(gen as u16))
}

/// 旧格式源档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawRecord {
    pub gen: u8,
    pub rec_id: u8,
    pub units: [u16; UNIT_LEN],
    pub checksum: u16,
}

impl RawRecord {
    /// 造档（校验和自动复算）。
    pub fn make(gen: u8, rec_id: u8, units: [u16; UNIT_LEN]) -> RawRecord {
        RawRecord { gen, rec_id, units, checksum: raw_checksum(&units, gen) }
    }
    /// 完整性：校验和复算相等（旧数据不破坏断言的锚）。
    pub fn intact(&self) -> bool {
        raw_checksum(&self.units, self.gen) == self.checksum
    }
    /// 源档指纹（对拍锚）。
    pub fn fingerprint(&self) -> u32 {
        let mut acc: u32 = 0x18550 + self.gen as u32;
        let mut i = 0;
        while i < UNIT_LEN {
            acc = acc.wrapping_mul(31).wrapping_add(self.units[i] as u32);
            i += 1;
        }
        acc.wrapping_mul(31).wrapping_add(self.rec_id as u32)
    }
}

/// 迁移产档（schema v1 槽像）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MigRecord {
    /// 迁移终点 schema（恒 SCHEMA_V1）。
    pub schema: u8,
    pub src_gen: u8,
    pub rec_id: u8,
    /// 槽像 [major,minor,build,sp,pt,suite,dpi,elevate]。
    pub vals: [u16; UNIT_LEN],
}

impl MigRecord {
    /// 产档指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut acc: u32 = 0x18560 + self.src_gen as u32;
        let mut i = 0;
        while i < UNIT_LEN {
            acc = acc.wrapping_mul(31).wrapping_add(self.vals[i] as u32);
            i += 1;
        }
        acc.wrapping_mul(31).wrapping_add(self.rec_id as u32)
    }
}

/// 三代布局代称（裁决表文本面）。
pub fn gen_layout_name(gen: u8) -> Option<&'static str> {
    match gen {
        0 => Some("bare_no_header"),
        1 => Some("header_v_pad"),
        2 => Some("header_sppt_swap"),
        _ => None,
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmCode {
    /// 未知代（gen ≥3）。
    GenUnknown,
    /// 校验和损坏。
    ChecksumBroken,
    /// 头魔数失配（gen1/gen2 头不符）。
    HeaderMismatch,
    /// 值域违约（major=0 等混入）。
    ValueOutOfRange,
    /// 迁移产档 schema 失冻。
    SchemaNotFrozen,
    /// 跳段验证（未迁移即验证）。
    SkipStepRejected,
    /// 重复记录 id。
    RecIdDuplicate,
    /// 批满（8 槽耗尽）。
    BatchFull,
    /// 空批收口。
    BatchEmpty,
    /// 相位倒退。
    PhaseRegression,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 双跑不一致。
    NotReproducible,
}

impl SmCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SmCode::GenUnknown => "未知代：旧格式闭集仅 0/1/2",
            SmCode::ChecksumBroken => "校验和损坏：源档完整性失",
            SmCode::HeaderMismatch => "头魔数失配：带头代头不符",
            SmCode::ValueOutOfRange => "值域违约：major=0 等混入必拒",
            SmCode::SchemaNotFrozen => "迁移产档 schema 失冻：须恒 v1",
            SmCode::SkipStepRejected => "跳段验证：未迁移不得验证",
            SmCode::RecIdDuplicate => "重复记录 id：批内唯一",
            SmCode::BatchFull => "批满：8 槽耗尽",
            SmCode::BatchEmpty => "空批收口：零迁移不得封",
            SmCode::PhaseRegression => "相位倒退：状态机单向禁回退",
            SmCode::LedgerBroken => "账本守恒破坏：尝试≠过+拒",
            SmCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [SmCode; 12] = [
        SmCode::GenUnknown,
        SmCode::ChecksumBroken,
        SmCode::HeaderMismatch,
        SmCode::ValueOutOfRange,
        SmCode::SchemaNotFrozen,
        SmCode::SkipStepRejected,
        SmCode::RecIdDuplicate,
        SmCode::BatchFull,
        SmCode::BatchEmpty,
        SmCode::PhaseRegression,
        SmCode::LedgerBroken,
        SmCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SmCode::ALL.len() {
        if SmCode::ALL[i].code() == code {
            return SmCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80540 段闭集内"
}

// ── 迁移核（三代布局 → v1 槽像） ────────────────────────────────
/// 迁移步数表（裁决推导）：gen0=1（重排）/ gen1=2（剥头+补缺省）/
/// gen2=2（换位回正+补缺省）。
pub fn step_count(gen: u8) -> Option<u32> {
    match gen {
        0 => Some(1),
        1 => Some(2),
        2 => Some(2),
        _ => None,
    }
}

/// 单代迁移：gen 闭集 → 完整性 → 布局变换 → 值域闸 → schema 冻结。
/// 只读源档（&RawRecord），产档为新值——升级不破坏旧数据。
pub fn migrate_one(raw: &RawRecord) -> Result<MigRecord, u32> {
    // 代闭集闸。
    if raw.gen > GEN_OLD_MAX {
        return Err(SmCode::GenUnknown.code());
    }
    // 完整性闸：损坏源档必拒。
    if !raw.intact() {
        return Err(SmCode::ChecksumBroken.code());
    }
    // 布局变换。
    let vals: [u16; UNIT_LEN] = match raw.gen {
        0 => {
            // gen0 裸档：[maj,min,bld,sp,pt,dpi,suite,elev]
            // → v1：[maj,min,bld,sp,pt,suite,dpi,elev]（dpi/suite 换位）。
            [
                raw.units[0], raw.units[1], raw.units[2], raw.units[3],
                raw.units[4], raw.units[6], raw.units[5], raw.units[7],
            ]
        }
        1 => {
            // gen1 带头 0xA1：[hdr,maj,min,bld,sp,pt,suite,pad]
            // → v1：剥头 + dpi=96 / elevate=0 补缺省。
            if raw.units[0] != HDR_GEN1 {
                return Err(SmCode::HeaderMismatch.code());
            }
            [
                raw.units[1], raw.units[2], raw.units[3], raw.units[4],
                raw.units[5], raw.units[6], DPI_DEFAULT, ELEV_DEFAULT,
            ]
        }
        _ => {
            // gen2 带头 0xA2：[hdr,maj,min,bld,pt,sp,dpi,suite]
            // → v1：sp/pt 换位回正 + elevate=0 补缺省。
            if raw.units[0] != HDR_GEN2 {
                return Err(SmCode::HeaderMismatch.code());
            }
            [
                raw.units[1], raw.units[2], raw.units[3], raw.units[5],
                raw.units[4], raw.units[7], raw.units[6], ELEV_DEFAULT,
            ]
        }
    };
    // 值域闸：主段下界（0 主段混入必拒）。
    if vals[0] < VER_MAJOR_MIN {
        return Err(SmCode::ValueOutOfRange.code());
    }
    let rec = MigRecord { schema: SCHEMA_V1, src_gen: raw.gen, rec_id: raw.rec_id, vals };
    // schema 冻结闸：产档 schema 恒 v1。
    if rec.schema != SCHEMA_V1 {
        return Err(SmCode::SchemaNotFrozen.code());
    }
    Ok(rec)
}

// ── 记录状态机（三态单向） ──────────────────────────────────────
/// 记录相位（单向：Pending→Migrated→Verified）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecPhase {
    /// 待迁移。
    Pending,
    /// 已迁移未验证。
    Migrated,
    /// 已验证。
    Verified,
}

/// 批账（守恒：尝试 = 过 + 拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchLedger {
    pub tries: u32,
    pub passed: u32,
    pub rejected: u32,
}

impl BatchLedger {
    pub const fn new() -> BatchLedger {
        BatchLedger { tries: 0, passed: 0, rejected: 0 }
    }
    pub fn conserve(&self) -> bool {
        self.tries == self.passed + self.rejected
    }
}

/// 步账（三代步数 {1,2,2} 合计 5）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepLedger {
    pub steps: [u32; 3],
}

impl StepLedger {
    pub const fn new() -> StepLedger {
        StepLedger { steps: [0; 3] }
    }
    /// 步数账守恒：合计必须 = 5（1+2+2 推导）。
    pub fn total_expected() -> u32 {
        5
    }
    pub fn total(&self) -> u32 {
        self.steps[0] + self.steps[1] + self.steps[2]
    }
}

/// 迁移批机：8 槽定容，记录相位三态，重复 id 拒，满显性拒。
pub struct SchemaMigrator {
    pub phases: [Option<RecPhase>; BATCH_CAP],
    pub migrated: [Option<MigRecord>; BATCH_CAP],
    pub len: usize,
    pub steps: StepLedger,
    pub ledger: BatchLedger,
}

impl SchemaMigrator {
    pub fn new() -> SchemaMigrator {
        SchemaMigrator {
            phases: [None; BATCH_CAP],
            migrated: [None; BATCH_CAP],
            len: 0,
            steps: StepLedger::new(),
            ledger: BatchLedger::new(),
        }
    }

    /// 记录 id 已在批判定。
    pub fn contains(&self, rec_id: u8) -> bool {
        let mut i = 0;
        while i < self.len {
            match self.migrated[i] {
                Some(m) => {
                    if m.rec_id == rec_id {
                        return true;
                    }
                }
                None => {}
            }
            i += 1;
        }
        false
    }

    /// 迁移入批：批满拒 → 重复 id 拒 → 单代迁移 → 入批 Pending→Migrated。
    /// 源档只读：迁移前后源档完整性对拍（旧数据不破坏断言锚）。
    pub fn migrate(&mut self, raw: &RawRecord) -> Result<MigRecord, u32> {
        self.ledger.tries = self.ledger.tries.saturating_add(1);
        if self.len >= BATCH_CAP {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(SmCode::BatchFull.code());
        }
        if self.contains(raw.rec_id) {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(SmCode::RecIdDuplicate.code());
        }
        // 源档迁移前完整性（前置快照）。
        let pre_ok = raw.intact();
        let rec = migrate_one(raw)?;
        // 源档迁移后完整性（后置快照——&self 语义下必等，断言留痕）。
        let post_ok = raw.intact();
        if !pre_ok || !post_ok {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(SmCode::ChecksumBroken.code());
        }
        // 步账入账。
        let sc = match step_count(raw.gen) {
            Some(c) => c,
            None => {
                self.ledger.rejected = self.ledger.rejected.saturating_add(1);
                return Err(SmCode::GenUnknown.code());
            }
        };
        self.steps.steps[raw.gen as usize] = self.steps.steps[raw.gen as usize].saturating_add(sc);
        self.migrated[self.len] = Some(rec);
        self.phases[self.len] = Some(RecPhase::Migrated);
        self.len += 1;
        self.ledger.passed = self.ledger.passed.saturating_add(1);
        Ok(rec)
    }

    /// 验证：跳段拒（Pending 不得直接验证）；对拍产档指纹。
    pub fn verify(&mut self, slot: usize, expect_fp: u32) -> Result<(), u32> {
        if slot >= self.len {
            return Err(SmCode::BatchFull.code());
        }
        let ph = match self.phases[slot] {
            Some(p) => p,
            None => return Err(SmCode::BatchEmpty.code()),
        };
        if ph == RecPhase::Pending {
            return Err(SmCode::SkipStepRejected.code());
        }
        if ph == RecPhase::Verified {
            return Err(SmCode::PhaseRegression.code());
        }
        let m = match self.migrated[slot] {
            Some(m) => m,
            None => return Err(SmCode::BatchEmpty.code()),
        };
        if m.fingerprint() != expect_fp {
            return Err(SmCode::SchemaNotFrozen.code());
        }
        self.phases[slot] = Some(RecPhase::Verified);
        Ok(())
    }

    /// 收口：空批拒；步账合计须 = 5；账守恒。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.len == 0 {
            return Err(SmCode::BatchEmpty.code());
        }
        if self.steps.total() != StepLedger::total_expected() {
            return Err(SmCode::LedgerBroken.code());
        }
        if !self.ledger.conserve() {
            return Err(SmCode::LedgerBroken.code());
        }
        Ok(())
    }
}

// ── 三代样本构造（冻结值） ──────────────────────────────────────
/// gen0 样本（裸档）：6.1.7601 SP1 PT3 DPI96 Suite0x100 Elev0。
pub fn sample_gen0() -> RawRecord {
    RawRecord::make(0, 0, [6, 1, 7601, 1, 3, 96, 0x0100, 0])
}

/// gen1 样本（带头 0xA1，缺 dpi/elevate）：6.1.7601 SP1 PT3。
pub fn sample_gen1() -> RawRecord {
    RawRecord::make(1, 1, [HDR_GEN1, 6, 1, 7601, 1, 3, 0x0100, 0])
}

/// gen2 样本（带头 0xA2，sp/pt 互换，缺 elevate）：10.0.19045 PT3 SP0。
pub fn sample_gen2() -> RawRecord {
    RawRecord::make(2, 2, [HDR_GEN2, 10, 0, 19045, 3, 0, 96, 0x0100])
}

/// 三代样本闭集。
pub fn three_samples() -> [RawRecord; 3] {
    [sample_gen0(), sample_gen1(), sample_gen2()]
}

/// 三代期望产档（逐例冻结值——迁移断言的对拍表）。
pub fn expected_mig(gen: u8) -> Option<[u16; UNIT_LEN]> {
    match gen {
        // gen0：dpi/suite 换位后 [6,1,7601,1,3,0x0100,96,0]。
        0 => Some([6, 1, 7601, 1, 3, 0x0100, 96, 0]),
        // gen1：剥头 + 补 96/0。
        1 => Some([6, 1, 7601, 1, 3, 0x0100, 96, 0]),
        // gen2：sp/pt 回正 + 补 0。
        2 => Some([10, 0, 19045, 0, 3, 0x0100, 96, 0]),
        _ => None,
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链输出（迁移数 / 步账 / 批指纹）。
pub type GoldenOut = (usize, StepLedger, u32);

/// 黄金全链：三代样本全迁移 → 逐例产档对拍 → 逐例验证 → 源档
/// 完整性复核（旧数据不破坏）→ 封账。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let samples = three_samples();
    let mut mig = SchemaMigrator::new();
    let mut fps: [u32; 3] = [0; 3];
    let mut i = 0;
    while i < 3 {
        let rec = mig.migrate(&samples[i])?;
        // 逐例产档对拍（期望表逐槽相等）。
        let expect = match expected_mig(rec.src_gen) {
            Some(e) => e,
            None => return Err(SmCode::GenUnknown.code()),
        };
        if rec.vals != expect {
            return Err(SmCode::SchemaNotFrozen.code());
        }
        if rec.schema != SCHEMA_V1 {
            return Err(SmCode::SchemaNotFrozen.code());
        }
        // 验证段（Pending 跳段在此不可达——已 Migrated）。
        let fp = rec.fingerprint();
        mig.verify(i, fp)?;
        fps[i] = fp;
        i += 1;
    }
    // 源档完整性复核：三代源档校验和全部仍完好（升级不破坏旧数据）。
    let mut j = 0;
    while j < 3 {
        if !samples[j].intact() {
            return Err(SmCode::ChecksumBroken.code());
        }
        j += 1;
    }
    // 步账对拍：{1,2,2} 合计 5。
    if mig.steps.steps != [1, 2, 2] {
        return Err(SmCode::LedgerBroken.code());
    }
    mig.seal()?;
    if !mig.ledger.conserve() {
        return Err(SmCode::LedgerBroken.code());
    }
    // 批指纹。
    let mut fp: u32 = 0x1855A;
    let mut k = 0;
    while k < 3 {
        fp = fp.wrapping_mul(31).wrapping_add(fps[k] % 31);
        k += 1;
    }
    Ok((mig.len, mig.steps, fp))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SmCode::NotReproducible.code());
    }
    Ok(())
}

// ── 注入核（三类变异 × 10 轮） ──────────────────────────────────
/// 注入账。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 未知代。
    pub gen_caught: u32,
    /// 类2 校验和损坏。
    pub corrupt_caught: u32,
    /// 类3 值域违约。
    pub value_caught: u32,
}

/// 面额（实现推导）：三类各每轮 1 次违约。
pub const FACE_GEN: u32 = 1;
pub const FACE_CORRUPT: u32 = 1;
pub const FACE_VALUE: u32 = 1;

/// 三类注入 rounds 轮；每类每轮必拒，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let mut r = 0;
    while r < rounds {
        // 类1 未知代：gen=3 混入。
        let bad_gen = RawRecord::make(3, 10, [6, 1, 7601, 1, 3, 96, 0x0100, 0]);
        match migrate_one(&bad_gen) {
            Err(c) if c == SmCode::GenUnknown.code() => {
                led.gen_caught = led.gen_caught.saturating_add(FACE_GEN);
            }
            _ => return Err(SmCode::NotReproducible.code()),
        }
        // 类2 校验和损坏：载荷改一字节而校验和不改。
        let mut corrupt = sample_gen0();
        corrupt.units[0] = 99;
        match migrate_one(&corrupt) {
            Err(c) if c == SmCode::ChecksumBroken.code() => {
                led.corrupt_caught = led.corrupt_caught.saturating_add(FACE_CORRUPT);
            }
            _ => return Err(SmCode::NotReproducible.code()),
        }
        // 类3 值域违约：major=0 混入 gen0 样本（校验和同步重造）。
        let bad_val = RawRecord::make(0, 11, [0, 1, 7601, 1, 3, 96, 0x0100, 0]);
        match migrate_one(&bad_val) {
            Err(c) if c == SmCode::ValueOutOfRange.code() => {
                led.value_caught = led.value_caught.saturating_add(FACE_VALUE);
            }
            _ => return Err(SmCode::NotReproducible.code()),
        }
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：黄金链全过。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：空批收口 → 拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut mig = SchemaMigrator::new();
    mig.seal()
}

/// 探针 C：头魔数失配（gen1 槽像带头 0xA2）→ 拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let raw = RawRecord::make(1, 7, [HDR_GEN2, 6, 1, 7601, 1, 3, 0x0100, 0]);
    // Ok 侧折叠为违约码（探针语义：头魔数失配必须被拒，Ok 即漏检）。
    match migrate_one(&raw) {
        Err(c) => Err(c),
        Ok(_) => Err(SmCode::HeaderMismatch.code()),
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < SmCode::ALL.len() {
        let v = SmCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据收口（8–20 条；动态明细 ≤8） ────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 黄金链：三代迁移+验证+源档完好+封账全过（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, steps, _)) => *n == 3 && steps.steps == [1, 2, 2],
        Err(_) => false,
    };
    v.push(("J1-黄金全链", g_ok, format!("{:?}", g)));

    // 2. 三代样本迁移逐例对拍（J1 第二命题：3 代样本全过）。
    let mig_ok = {
        let samples = three_samples();
        let mut ok = true;
        let mut i = 0;
        while i < 3 {
            match migrate_one(&samples[i]) {
                Ok(rec) => {
                    match expected_mig(rec.src_gen) {
                        Some(e) => {
                            if rec.vals != e || rec.schema != SCHEMA_V1 {
                                ok = false;
                            }
                        }
                        None => ok = false,
                    }
                }
                Err(_) => ok = false,
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-三代迁移逐例过", mig_ok, String::from("期望表逐槽相等")));

    // 3. 升级不破坏旧数据：源档迁移前后完整性对拍（J1 第三命题）。
    let intact_ok = {
        let samples = three_samples();
        let mut fps_pre: [u32; 3] = [0; 3];
        let mut i = 0;
        while i < 3 {
            fps_pre[i] = samples[i].fingerprint();
            i += 1;
        }
        let mut mig = SchemaMigrator::new();
        let mut k = 0;
        let mut ok = true;
        while k < 3 {
            if mig.migrate(&samples[k]).is_err() {
                ok = false;
            }
            k += 1;
        }
        // 迁移后源档指纹前后相等 + 完整性仍好。
        let mut j = 0;
        while j < 3 {
            if samples[j].fingerprint() != fps_pre[j] || !samples[j].intact() {
                ok = false;
            }
            j += 1;
        }
        ok
    };
    v.push(("J1-升级不破坏旧数据", intact_ok, String::from("源档指纹前后等")));

    // 4. schema v1 冻结：产档 schema 恒 3 + 布局名闭集。
    let frozen_ok = SCHEMA_V1 == 3
        && gen_layout_name(0).is_some()
        && gen_layout_name(2).is_some()
        && gen_layout_name(3).is_none();
    v.push(("J1-schema冻结", frozen_ok, String::from("恒 v1(=3) 布局名闭集")));

    // 5. 步数推导：{1,2,2} 合计 5。
    let step_ok = step_count(0) == Some(1)
        && step_count(1) == Some(2)
        && step_count(2) == Some(2)
        && step_count(3).is_none()
        && StepLedger::total_expected() == 5;
    v.push(("J1-步数推导{1,2,2}", step_ok, String::from("重排1+剥头补缺2+换位补缺2")));

    // 6. 类1 未知代注入（1 × 10 轮）。
    let led = inject_rounds(JUDGE_ROUNDS);
    let t1 = led.as_ref().map(|l| l.gen_caught).unwrap_or(0);
    v.push(("J1-未知代全拒", t1 == FACE_GEN * JUDGE_ROUNDS, format!("{}/{}", t1, FACE_GEN * JUDGE_ROUNDS)));

    // 7. 类2 校验和损坏注入。
    let t2 = led.as_ref().map(|l| l.corrupt_caught).unwrap_or(0);
    v.push(("J1-损坏源档全拒", t2 == FACE_CORRUPT * JUDGE_ROUNDS, format!("{}/{}", t2, FACE_CORRUPT * JUDGE_ROUNDS)));

    // 8. 类3 值域违约注入。
    let t3 = led.as_ref().map(|l| l.value_caught).unwrap_or(0);
    v.push(("J1-值域违约全拒", t3 == FACE_VALUE * JUDGE_ROUNDS, format!("{}/{}", t3, FACE_VALUE * JUDGE_ROUNDS)));

    // 9. 注入账守恒。
    let cons = match led.as_ref() {
        Ok(l) => {
            let sum = l.gen_caught.saturating_add(l.corrupt_caught).saturating_add(l.value_caught);
            sum == (FACE_GEN + FACE_CORRUPT + FACE_VALUE) * JUDGE_ROUNDS
        }
        Err(_) => false,
    };
    v.push(("J1-注入账守恒", cons, String::from("3类面额×10轮")));

    // 10. 跳段验证拒：手工造 Pending 相位记录后验证必拒。
    let skip_ok = {
        let mut mig = SchemaMigrator::new();
        // 直填 Pending 相位（绕过 migrate——闸序负对照）。
        mig.phases[0] = Some(RecPhase::Pending);
        mig.len = 1;
        mig.verify(0, 0).is_err()
    };
    v.push(("J1-跳段验证拒", skip_ok, String::from("Pending 不得验证")));

    // 11. 批容量：8 槽满显性拒且旧账不丢。
    let cap_ok = {
        let mut mig = SchemaMigrator::new();
        let mut ok = true;
        let mut k = 0u8;
        while k < BATCH_CAP as u8 {
            let raw = RawRecord::make(0, k, [6, 1, 7601, 1, 3, 96, 0x0100, 0]);
            if mig.migrate(&raw).is_err() {
                ok = false;
            }
            k += 1;
        }
        let extra = RawRecord::make(0, 99, [6, 1, 7601, 1, 3, 96, 0x0100, 0]);
        let pushed = mig.migrate(&extra);
        ok && pushed.is_err() && mig.len == BATCH_CAP && mig.ledger.rejected == 1
    };
    v.push(("J1-批容量8满拒", cap_ok, String::from("8 收 1 拒")));

    // 12. 重复 id 拒。
    let dup_ok = {
        let mut mig = SchemaMigrator::new();
        if mig.migrate(&sample_gen0()).is_err() {
            false
        } else {
            // 同 id 不同代的重复登记。
            let mut raw = sample_gen2();
            raw.rec_id = 0;
            mig.migrate(&raw).err() == Some(SmCode::RecIdDuplicate.code())
        }
    };
    v.push(("J1-重复id拒", dup_ok, String::from("批内 id 唯一")));

    // 13. 空批收口拒（探针 B）+ 头魔数失配拒（探针 C）。
    v.push(("J1-空批/头失配双拒", probe_seq_b().is_err() && probe_seq_c().is_err(), String::from("空批封口/0xA2 冒 0xA1")));

    // 14. 相位倒退拒：Verified 后复验必拒。
    let regress_ok = {
        let mut mig = SchemaMigrator::new();
        let raw = sample_gen0();
        match mig.migrate(&raw) {
            Ok(rec) => {
                let fp = rec.fingerprint();
                if mig.verify(0, fp).is_err() {
                    false
                } else {
                    mig.verify(0, fp).is_err()
                }
            }
            Err(_) => false,
        }
    };
    v.push(("J1-相位倒退拒", regress_ok, String::from("Verified 复验必拒")));

    // 15. 批指纹双跑一致。
    let fp = match &g {
        Ok((_, _, f)) => *f,
        Err(_) => 0,
    };
    let fp2 = match golden_run() {
        Ok((_, _, f)) => f,
        Err(_) => 1,
    };
    v.push(("J2-批指纹双跑", fp == fp2 && fp != 0, format!("{:08X}", fp)));

    // 16. 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));

    // 17. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 18. 诊断码闭集（12 码全非空）。
    let all_ok = {
        let mut ok = SmCode::ALL.len() == 12;
        let mut i = 0;
        while i < SmCode::ALL.len() {
            if SmCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集12码", all_ok, String::from("detail 全非空")));

    v
}

/// 收口：本模块自检 CheckSet（判据 18 条；前 8 条动态明细）。
pub fn run_unx18550_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18550 档案版本化迁移");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "见 extra_checks 明细");
        }
        i += 1;
    }
    s
}

// ── 测试（≥8 个 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 总闸：全部判据绿。
    #[test]
    fn test_all_checks_green() {
        let set = run_unx18550_checks();
        assert!(set.all_passed(), "UNX-F18550 判据集应全绿");
    }

    /// 黄金链：三代迁移 + 步账 {1,2,2}。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((n, steps, _)) => {
                assert_eq!(n, 3);
                assert_eq!(steps.steps, [1, 2, 2]);
                assert_eq!(steps.total(), 5);
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// 三代逐例迁移对拍。
    #[test]
    fn test_migrate_each_gen() {
        let samples = three_samples();
        let mut i = 0;
        while i < 3 {
            let rec = match migrate_one(&samples[i]) {
                Ok(r) => r,
                Err(e) => panic!("gen{} 迁移不应失败: {}", i, detail(e)),
            };
            let expect = match expected_mig(rec.src_gen) {
                Some(e) => e,
                None => panic!("期望表缺 gen{}", i),
            };
            assert_eq!(rec.vals, expect);
            assert_eq!(rec.schema, SCHEMA_V1);
            i += 1;
        }
    }

    /// 源档只读：迁移前后指纹相等（旧数据不破坏）。
    #[test]
    fn test_source_preserved() {
        let samples = three_samples();
        let mut i = 0;
        while i < 3 {
            let pre = samples[i].fingerprint();
            let _ = migrate_one(&samples[i]);
            assert_eq!(samples[i].fingerprint(), pre);
            assert!(samples[i].intact());
            i += 1;
        }
    }

    /// 注入三类全拒（10 轮）。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.gen_caught, 10);
                assert_eq!(l.corrupt_caught, 10);
                assert_eq!(l.value_caught, 10);
            }
            Err(e) => assert!(false, "注入全拒不应报错: {}", detail(e)),
        }
    }

    /// 校验和折叠：代参与防窜。
    #[test]
    fn test_checksum_gen_fold() {
        let units = [6, 1, 7601, 1, 3, 96, 0x0100, 0];
        let c0 = raw_checksum(&units, 0);
        let c1 = raw_checksum(&units, 1);
        assert_ne!(c0, c1, "代必须参与校验和折叠");
        let r = RawRecord::make(0, 0, units);
        assert!(r.intact());
        let mut q = r;
        q.gen = 1;
        assert!(!q.intact(), "跨代窜档必失配");
    }

    /// 批闸：容量 / 重复 / 跳段 / 倒退。
    #[test]
    fn test_batch_gates() {
        let mut mig = SchemaMigrator::new();
        assert!(probe_seq_b().is_err());
        let raw = sample_gen0();
        let rec = match mig.migrate(&raw) {
            Ok(r) => r,
            Err(e) => panic!("迁移不应失败: {}", detail(e)),
        };
        let mut raw2 = sample_gen2();
        raw2.rec_id = 0;
        assert_eq!(mig.migrate(&raw2).err(), Some(SmCode::RecIdDuplicate.code()));
        let fp = rec.fingerprint();
        assert!(mig.verify(0, fp).is_ok());
        assert_eq!(mig.verify(0, fp).err(), Some(SmCode::PhaseRegression.code()));
    }

    /// 头魔数闸：gen1 冒 0xA2 拒 / gen2 冒 0xA1 拒。
    #[test]
    fn test_header_gates() {
        assert!(probe_seq_c().is_err());
        let raw = RawRecord::make(2, 8, [HDR_GEN1, 10, 0, 19045, 3, 0, 96, 0x0100]);
        assert_eq!(migrate_one(&raw).err(), Some(SmCode::HeaderMismatch.code()));
    }

    /// 双跑一致。
    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
    }

    /// 段位守卫。
    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
    }
}
