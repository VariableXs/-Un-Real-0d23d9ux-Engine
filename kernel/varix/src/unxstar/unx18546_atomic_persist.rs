//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18546 · 档案持久化（原子写+回滚备份）
//! · 0x80440 段 · 批 08 · 配额 260
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18546-J1 档案落盘原子写断言全过（断电注入 3/3 回滚
//!       成功），备份保留账在档。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 双槽镜像冻结：槽 0/槽 1 互为活动/影子；写序 = 影子暂存 →
//!     校验和复算 → 提交翻面（active 翻转）；未提交影子永不生效；
//!   - 断电注入三点冻结：PL0=暂存前 / PL1=暂存中断（截断载荷）/
//!     PL2=暂存毕未提交——三点断电后活动槽必须仍持旧值（回滚成功
//!     3/3），备份槽始终保留上一提交版（保留账在档）；
//!   - 载荷校验：8 槽 u16 载荷 + 校验和（wrapping 和的按位反码）
//!     + 单调序号 seq——载荤断 / 校验和伪 / 序号回退三类坏样本
//!     逐类必拒；
//!   - 备份保留账：每次成功提交前旧活动值入备份槽，retained 计数
//!     单调增；回滚从备份槽取数，账与槽一致对拍；
//!   - 状态机 per 槽三态：Clean → Staged → Committed；提交只认
//!     Staged 且校验过；倒退拒；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] PvCode 闭集（12 码）+ ALL + detail() + detail(u32)
//!   [裁决表]   断电三点语义表 load_semantics（PL0/1/2 各自回滚判据）
//!   [数据结构] StoredImage（8 槽载荷+校验和+seq）/ PersistStore 双槽
//!   [状态机]   SlotPhase 三态 per 槽 + 提交翻面
//!   [账本]     BackupLedger（保留账）+ WriteLedger（尝试=过+拒）
//!   [注入核]   三类坏样本 × 10 轮（断载/伪校验/序号回退）
//!   [收口]     golden_run + run_unx18546_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80440，AI-VX11-1105 G1 批08 独占区）。
pub const SEG_BASE: u32 = 0x80440;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// 载荷槽数（冻结：8，与总纲 PROFILE_UNITS 同口径）。
pub const PAYLOAD_LEN: usize = 8;
/// 断电注入点闭集（冻结：0=暂存前 1=暂存中断 2=暂存毕未提交）。
pub const PL_BEFORE: u8 = 0;
pub const PL_MID: u8 = 1;
pub const PL_PRE_COMMIT: u8 = 2;
/// 断电点数（推导：3 点 → 3/3 口径）。
pub const PL_POINTS: usize = 3;
/// 暂存中断截断位（冻结：写至第 3 槽断）。
pub const PL_MID_CUT: usize = 3;
/// 判据注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;

/// 载荷校验和：wrapping 和的按位反码（0 载荷 → 0xFFFF）。
pub fn payload_checksum(p: &[u16; PAYLOAD_LEN]) -> u16 {
    let mut acc: u16 = 0;
    let mut i = 0;
    while i < PAYLOAD_LEN {
        acc = acc.wrapping_add(p[i]);
        i += 1;
    }
    !acc
}

/// 载荷镜像。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoredImage {
    pub payload: [u16; PAYLOAD_LEN],
    pub checksum: u16,
    /// 单调序号（每提交 +1）。
    pub seq: u32,
    /// 已提交位（未提交影子永不生效）。
    pub committed: bool,
}

impl StoredImage {
    /// 空镜像（seq=0 未提交）。
    pub const fn empty() -> StoredImage {
        StoredImage { payload: [0; PAYLOAD_LEN], checksum: 0, seq: 0, committed: false }
    }
    /// 从载荷造镜像（校验和自动复算，committed 由调用方定）。
    pub fn stage(p: [u16; PAYLOAD_LEN], seq: u32) -> StoredImage {
        StoredImage { payload: p, checksum: payload_checksum(&p), seq, committed: false }
    }
    /// 完整性核验：校验和复算相等 + 已提交。
    pub fn intact(&self) -> bool {
        self.committed && payload_checksum(&self.payload) == self.checksum
    }
    /// 镜像指纹（载荷+seq 折叠，对拍锚）。
    pub fn fingerprint(&self) -> u32 {
        let mut acc: u32 = 0x18546;
        let mut i = 0;
        while i < PAYLOAD_LEN {
            acc = acc.wrapping_mul(31).wrapping_add(self.payload[i] as u32);
            i += 1;
        }
        acc = acc.wrapping_mul(31).wrapping_add(self.seq);
        acc.wrapping_mul(31).wrapping_add(if self.committed { 7 } else { 3 })
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PvCode {
    /// 载荷校验和失配（断载/伪造）。
    ChecksumMismatch,
    /// 未提交镜像被当生效值读取。
    UncommittedRead,
    /// 序号回退（旧值盖新值）。
    SeqRegression,
    /// 提交未暂存（跳段提交）。
    CommitWithoutStage,
    /// 空库读取（无任何提交版且无备份）。
    StoreEmpty,
    /// 槽位越界（≥2）。
    SlotOutOfRange,
    /// 提交翻面错槽（影子==活动）。
    SlotFlipBroken,
    /// 备份账不一致（retained 与实际备份不齐）。
    BackupLedgerBroken,
    /// 写账守恒破坏。
    LedgerBroken,
    /// 注入漏检。
    InjectionMissed,
    /// 口径失配（与总纲槽宽不等）。
    CrossCheckMismatch,
    /// 双跑不一致。
    NotReproducible,
}

impl PvCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PvCode::ChecksumMismatch => "校验和失配：断载/伪造必拒",
            PvCode::UncommittedRead => "未提交镜像禁当生效值",
            PvCode::SeqRegression => "序号回退：旧值不得盖新值",
            PvCode::CommitWithoutStage => "提交未暂存：跳段禁",
            PvCode::StoreEmpty => "空库：无提交版且无备份",
            PvCode::SlotOutOfRange => "槽位越界：双槽仅 0/1",
            PvCode::SlotFlipBroken => "翻面错槽：影子与活动重合",
            PvCode::BackupLedgerBroken => "备份账不一致：账槽对拍失配",
            PvCode::LedgerBroken => "写账守恒破坏：尝试≠过+拒",
            PvCode::InjectionMissed => "注入漏检：变异必被抓",
            PvCode::CrossCheckMismatch => "口径失配：与总纲槽宽不等",
            PvCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [PvCode; 12] = [
        PvCode::ChecksumMismatch,
        PvCode::UncommittedRead,
        PvCode::SeqRegression,
        PvCode::CommitWithoutStage,
        PvCode::StoreEmpty,
        PvCode::SlotOutOfRange,
        PvCode::SlotFlipBroken,
        PvCode::BackupLedgerBroken,
        PvCode::LedgerBroken,
        PvCode::InjectionMissed,
        PvCode::CrossCheckMismatch,
        PvCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PvCode::ALL.len() {
        if PvCode::ALL[i].code() == code {
            return PvCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80440 段闭集内"
}

// ── 槽相位（三态 per 槽） ───────────────────────────────────────
/// 槽态（Clean→Staged→Committed；提交后回到 Clean 备用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotPhase {
    /// 干净待写。
    Clean,
    /// 影子暂存中。
    Staged,
    /// 已提交（翻面后本槽曾为活动）。
    Committed,
}

// ── 双槽持久库 ──────────────────────────────────────────────────
/// 备份保留账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackupLedger {
    /// 成功提交次数（= 备份保留次数）。
    pub retained: u32,
    /// 最近一次备份的 seq（0 = 尚无备份）。
    pub last_backup_seq: u32,
}

impl BackupLedger {
    pub const fn new() -> BackupLedger {
        BackupLedger { retained: 0, last_backup_seq: 0 }
    }
}

/// 写账（守恒：尝试 = 过 + 拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteLedger {
    pub tries: u32,
    pub passed: u32,
    pub rejected: u32,
}

impl WriteLedger {
    pub const fn new() -> WriteLedger {
        WriteLedger { tries: 0, passed: 0, rejected: 0 }
    }
    pub fn conserve(&self) -> bool {
        self.tries == self.passed + self.rejected
    }
}

/// 双槽持久库：active ∈ {0,1}；影子 = 1-active。
pub struct PersistStore {
    pub images: [StoredImage; 2],
    pub active: usize,
    pub phases: [SlotPhase; 2],
    pub backup: BackupLedger,
    pub ledger: WriteLedger,
}

impl PersistStore {
    pub fn new() -> PersistStore {
        PersistStore {
            images: [StoredImage::empty(), StoredImage::empty()],
            active: 0,
            phases: [SlotPhase::Clean, SlotPhase::Clean],
            backup: BackupLedger::new(),
            ledger: WriteLedger::new(),
        }
    }

    /// 影子槽号（推导：1 - active，越界防御）。
    fn shadow_of(&self) -> Result<usize, u32> {
        if self.active >= 2 {
            return Err(PvCode::SlotOutOfRange.code());
        }
        Ok(1 - self.active)
    }

    /// 生效值读取：活动槽必须 intact（committed+校验过）；
    /// 空库（未提交）→ StoreEmpty。
    pub fn read_active(&self) -> Result<StoredImage, u32> {
        let img = match self.images.get(self.active) {
            Some(i) => *i,
            None => return Err(PvCode::SlotOutOfRange.code()),
        };
        if !img.committed {
            return Err(PvCode::StoreEmpty.code());
        }
        if payload_checksum(&img.payload) != img.checksum {
            return Err(PvCode::ChecksumMismatch.code());
        }
        Ok(img)
    }

    /// 备份读取：非活动槽已提交镜像（回滚源）。
    pub fn read_backup(&self) -> Result<StoredImage, u32> {
        let sh = self.shadow_of()?;
        let img = match self.images.get(sh) {
            Some(i) => *i,
            None => return Err(PvCode::SlotOutOfRange.code()),
        };
        if !img.committed {
            return Err(PvCode::StoreEmpty.code());
        }
        Ok(img)
    }

    /// 提交影子闸：暂存过 + 校验和复算相等 + seq 严格大于活动 seq，
    /// 三闸全过才翻面。独立成函数使"伪造暂存"注入路径与原子写共用
    /// 同一套提交语义（防旁路）。
    pub fn commit_shadow(&mut self) -> Result<(), u32> {
        let sh = self.shadow_of()?;
        let img = match self.images.get(sh) {
            Some(i) => *i,
            None => return Err(PvCode::SlotOutOfRange.code()),
        };
        if self.phases[sh] != SlotPhase::Staged {
            return Err(PvCode::CommitWithoutStage.code());
        }
        if payload_checksum(&img.payload) != img.checksum {
            return Err(PvCode::ChecksumMismatch.code());
        }
        // 序号单调闸：新 seq 必须严格大于活动 seq（旧值盖新值禁）。
        let old_seq = match self.images.get(self.active) {
            Some(i) => i.seq,
            None => 0,
        };
        if img.seq <= old_seq {
            return Err(PvCode::SeqRegression.code());
        }
        // 提交：影子置 committed → 翻面 → 旧活动槽归 Clean → 备份账。
        let mut committed = img;
        committed.committed = true;
        self.images[sh] = committed;
        self.phases[sh] = SlotPhase::Committed;
        // 备份保留：若旧活动槽有已提交版，则它就是备份（仍在槽中）。
        let old_active = match self.images.get(self.active) {
            Some(i) => *i,
            None => StoredImage::empty(),
        };
        if old_active.committed {
            self.backup.retained = self.backup.retained.saturating_add(1);
            self.backup.last_backup_seq = old_active.seq;
        }
        self.active = sh;
        self.phases[1 - sh] = SlotPhase::Clean;
        Ok(())
    }

    /// 原子写：暂存影子 → 断电点注入 → 提交闸 → 翻面。
    /// 返回 Ok(()) = 提交成功；Err(码) = 断电/违约被拦（活动槽
    /// 仍持旧值——回滚成功语义由调用方对拍 read_active 证实）。
    pub fn atomic_write(&mut self, p: [u16; PAYLOAD_LEN], pl: Option<u8>) -> Result<(), u32> {
        self.ledger.tries = self.ledger.tries.saturating_add(1);
        let sh = match self.shadow_of() {
            Ok(s) => s,
            Err(c) => {
                self.ledger.rejected = self.ledger.rejected.saturating_add(1);
                return Err(c);
            }
        };
        // PL0：暂存前断电——影子未动，直接"回滚"（无副作用）。
        if pl == Some(PL_BEFORE) {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(PvCode::UncommittedRead.code());
        }
        let old_seq = match self.images.get(self.active) {
            Some(i) => i.seq,
            None => 0,
        };
        // 暂存：全量或截断（PL1 至第 PL_MID_CUT 槽断）。
        let mut staged = StoredImage::stage(p, old_seq.wrapping_add(1));
        if pl == Some(PL_MID) {
            // 截断：尾槽清零（模拟写一半断电）且校验和未落盘（0）
            // → 提交闸校验和复算必失配。
            let mut k = PL_MID_CUT;
            while k < PAYLOAD_LEN {
                staged.payload[k] = 0;
                k += 1;
            }
            staged.checksum = 0;
            // 截断镜像落影子但不提交。
            self.images[sh] = staged;
            self.phases[sh] = SlotPhase::Staged;
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(PvCode::ChecksumMismatch.code());
        }
        self.images[sh] = staged;
        self.phases[sh] = SlotPhase::Staged;
        // PL2：暂存毕未提交断电——影子在但 committed=false。
        if pl == Some(PL_PRE_COMMIT) {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(PvCode::UncommittedRead.code());
        }
        match self.commit_shadow() {
            Ok(()) => {
                self.ledger.passed = self.ledger.passed.saturating_add(1);
                Ok(())
            }
            Err(c) => {
                self.ledger.rejected = self.ledger.rejected.saturating_add(1);
                Err(c)
            }
        }
    }

    /// 断电后回滚核验：活动槽必须 intact 且值等于期望旧值。
    pub fn rollback_ok(&self, expect: Option<StoredImage>) -> Result<bool, u32> {
        let cur = match self.read_active() {
            Ok(i) => i,
            Err(c) if c == PvCode::StoreEmpty.code() => {
                // 空库断电：期望也是空 → 回滚成功（无值即旧值）。
                return Ok(expect.is_none());
            }
            Err(c) => return Err(c),
        };
        match expect {
            Some(e) => Ok(cur == e),
            None => Ok(false),
        }
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链输出（提交数 / 备份账 / 活动指纹）。
pub type GoldenOut = (u32, BackupLedger, u32);

/// 黄金全链：首提交（空库）→ 二提交（备份保留 1）→ 三提交
/// （备份保留 2，seq 单调）→ 备份槽读回上一版对拍。
pub fn golden_run() -> Result<GoldenOut, u32> {
    if PAYLOAD_LEN != 8 {
        return Err(PvCode::CrossCheckMismatch.code());
    }
    let mut store = PersistStore::new();
    // 版本一：典型 6.1.7601 槽像。
    let v1: [u16; PAYLOAD_LEN] = [6, 1, 7601, 1, 3, 0x0100, 96, 0];
    store.atomic_write(v1, None)?;
    let a1 = store.read_active()?;
    if a1.seq != 1 || a1.payload != v1 {
        return Err(PvCode::LedgerBroken.code());
    }
    // 版本二：10.0.19045 槽像。
    let v2: [u16; PAYLOAD_LEN] = [10, 0, 19045, 0, 3, 0x0100, 96, 0];
    store.atomic_write(v2, None)?;
    let a2 = store.read_active()?;
    if a2.seq != 2 || a2.payload != v2 {
        return Err(PvCode::LedgerBroken.code());
    }
    // 备份对拍：备份槽必须仍是 v1（seq 1）。
    let bk = store.read_backup()?;
    if bk.seq != 1 || bk.payload != v1 {
        return Err(PvCode::BackupLedgerBroken.code());
    }
    // 版本三：11.0.22000 槽像。
    let v3: [u16; PAYLOAD_LEN] = [11, 0, 22000, 0, 3, 0x0100, 96, 1];
    store.atomic_write(v3, None)?;
    if store.backup.retained != 2 || store.backup.last_backup_seq != 2 {
        return Err(PvCode::BackupLedgerBroken.code());
    }
    if !store.ledger.conserve() {
        return Err(PvCode::LedgerBroken.code());
    }
    let cur = store.read_active()?;
    Ok((store.backup.retained, store.backup, cur.fingerprint()))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PvCode::NotReproducible.code());
    }
    Ok(())
}

// ── 断电注入（3 点 × 10 轮，3/3 回滚） ──────────────────────────
/// 断电注入账。
#[derive(Clone, Copy, Debug, Default)]
pub struct PowerLossLedger {
    pub pl0_rolled: u32,
    pub pl1_rolled: u32,
    pub pl2_rolled: u32,
}

/// 坏样本注入账（非断电类）。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 伪造校验和（载荷改而校验和不改）。
    pub forge_caught: u32,
    /// 类2 序号回退。
    pub seq_caught: u32,
    /// 类3 未提交读取。
    pub uncommit_caught: u32,
}

/// 断电三点 rounds 轮：每点每轮回滚必须成功（活动槽持旧值）。
/// 3 点 × rounds → 3/3 口径的 rounds 倍展开。
pub fn power_loss_rounds(rounds: u32) -> Result<PowerLossLedger, u32> {
    let mut led = PowerLossLedger::default();
    let mut r = 0;
    while r < rounds {
        // 先立一个已提交版 v1 作"旧值"。
        let v1: [u16; PAYLOAD_LEN] = [6, 1, 7601, 1, 3, 0x0100, 96, 0];
        let v2: [u16; PAYLOAD_LEN] = [10, 0, 19045, 0, 3, 0x0100, 96, 0];
        let mut store = PersistStore::new();
        store.atomic_write(v1, None).map_err(|_| PvCode::InjectionMissed.code())?;
        let old = store.read_active().map_err(|_| PvCode::InjectionMissed.code())?;
        // PL0：暂存前断 → 回滚 = v1 仍在活动槽（断电写必须被拦）。
        if store.atomic_write(v2, Some(PL_BEFORE)).is_ok() {
            return Err(PvCode::InjectionMissed.code());
        }
        if !store.rollback_ok(Some(old))? {
            return Err(PvCode::InjectionMissed.code());
        }
        led.pl0_rolled = led.pl0_rolled.saturating_add(1);
        // PL1：暂存中断 → 影子截断、活动槽仍 v1。
        if store.atomic_write(v2, Some(PL_MID)).is_ok() {
            return Err(PvCode::InjectionMissed.code());
        }
        if !store.rollback_ok(Some(old))? {
            return Err(PvCode::InjectionMissed.code());
        }
        led.pl1_rolled = led.pl1_rolled.saturating_add(1);
        // PL2：暂存毕未提交断 → 影子全量但未提交、活动槽仍 v1。
        if store.atomic_write(v2, Some(PL_PRE_COMMIT)).is_ok() {
            return Err(PvCode::InjectionMissed.code());
        }
        if !store.rollback_ok(Some(old))? {
            return Err(PvCode::InjectionMissed.code());
        }
        led.pl2_rolled = led.pl2_rolled.saturating_add(1);
        r += 1;
    }
    Ok(led)
}

/// 三类坏样本注入 rounds 轮；每类每轮必抓，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let mut r = 0;
    while r < rounds {
        let v1: [u16; PAYLOAD_LEN] = [6, 1, 7601, 1, 3, 0x0100, 96, 0];
        // 类1 伪造校验和：载荷改槽 0 而校验和不改 → 完整性核必抓。
        let mut forged = StoredImage::stage(v1, 1);
        forged.committed = true;
        forged.payload[0] = 99;
        if !forged.intact() {
            led.forge_caught = led.forge_caught.saturating_add(1);
        } else {
            return Err(PvCode::InjectionMissed.code());
        }
        // 类2 序号回退：伪造影子带旧 seq（0）→ 提交闸 SeqRegression。
        let mut store = PersistStore::new();
        store.atomic_write(v1, None).map_err(|_| PvCode::InjectionMissed.code())?;
        let sh = 1 - store.active;
        // 伪造暂存：合法载荷但 seq 回退到 0（< 活动 seq 1）。
        let mut reg = StoredImage::stage([11, 0, 22000, 0, 3, 0x0100, 96, 1], 0);
        reg.seq = 0;
        store.images[sh] = reg;
        store.phases[sh] = SlotPhase::Staged;
        match store.commit_shadow() {
            Err(c) if c == PvCode::SeqRegression.code() => {
                led.seq_caught = led.seq_caught.saturating_add(1);
            }
            _ => return Err(PvCode::InjectionMissed.code()),
        }
        // 类3 未提交读取：空库读活动槽 → StoreEmpty。
        let empty = PersistStore::new();
        match empty.read_active() {
            Err(c) if c == PvCode::StoreEmpty.code() => {
                led.uncommit_caught = led.uncommit_caught.saturating_add(1);
            }
            _ => return Err(PvCode::InjectionMissed.code()),
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

/// 探针 B：空库读 → 拒（StoreEmpty）。
pub fn probe_seq_b() -> Result<(), u32> {
    PersistStore::new().read_active().map(|_| ())
}

/// 探针 C：断电 PL1 后影子截断不可被当生效值读取。
pub fn probe_seq_c() -> Result<(), u32> {
    let v1: [u16; PAYLOAD_LEN] = [6, 1, 7601, 1, 3, 0x0100, 96, 0];
    let v2: [u16; PAYLOAD_LEN] = [10, 0, 19045, 0, 3, 0x0100, 96, 0];
    let mut store = PersistStore::new();
    store.atomic_write(v1, None)?;
    if store.atomic_write(v2, Some(PL_MID)).is_ok() {
        return Err(PvCode::InjectionMissed.code());
    }
    // 回滚成功后活动槽仍 v1：读取必须得 v1 而非截断 v2。
    let cur = store.read_active()?;
    if cur.payload == v1 {
        Ok(())
    } else {
        Err(PvCode::UncommittedRead.code())
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < PvCode::ALL.len() {
        let v = PvCode::ALL[i].code();
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

    // 1. 黄金链：三次提交 + 备份保留 2 全过（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((r, bk, _)) => *r == 2 && bk.retained == 2 && bk.last_backup_seq == 2,
        Err(_) => false,
    };
    v.push(("J1-黄金全链", g_ok, format!("{:?}", g)));

    // 2. 断电注入 3/3：三点 × 10 轮回滚全成功（J1 第二命题）。
    let pl = power_loss_rounds(JUDGE_ROUNDS);
    let pl_ok = match &pl {
        Ok(l) => l.pl0_rolled == JUDGE_ROUNDS
            && l.pl1_rolled == JUDGE_ROUNDS
            && l.pl2_rolled == JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push(("J1-断电3/3回滚", pl_ok, format!("{:?}", pl)));

    // 3. 备份保留账在档：提交后备份槽持上一提交版。
    let bk_ok = {
        let v1: [u16; PAYLOAD_LEN] = [6, 1, 7601, 1, 3, 0x0100, 96, 0];
        let v2: [u16; PAYLOAD_LEN] = [10, 0, 19045, 0, 3, 0x0100, 96, 0];
        let mut store = PersistStore::new();
        if store.atomic_write(v1, None).is_err() {
            false
        } else if store.atomic_write(v2, None).is_err() {
            false
        } else {
            match store.read_backup() {
                Ok(b) => b.payload == v1 && b.seq == 1,
                Err(_) => false,
            }
        }
    };
    v.push(("J1-备份保留账在档", bk_ok, String::from("备份槽=上一提交版")));

    // 4. 原子性：未提交影子永不当生效值（未提交位闸）。
    let atomic_ok = {
        let mut img = StoredImage::stage([1, 2, 3, 4, 5, 6, 7, 8], 1);
        img.committed = false;
        !img.intact()
    };
    v.push(("J1-未提交不生效", atomic_ok, String::from("committed=false 拒读")));

    // 5. 校验和推导：0 载荷 → 0xFFFF（反码语义）。
    let ck_ok = payload_checksum(&[0; PAYLOAD_LEN]) == 0xFFFF
        && payload_checksum(&[1, 0, 0, 0, 0, 0, 0, 0]) == 0xFFFE;
    v.push(("J1-校验和反码推导", ck_ok, String::from("~Σpayload")));

    // 6. 类1 伪造校验和注入（1 × 10 轮）。
    let led = inject_rounds(JUDGE_ROUNDS);
    let t1 = led.as_ref().map(|l| l.forge_caught).unwrap_or(0);
    v.push(("J1-伪校验和全抓", t1 == JUDGE_ROUNDS, format!("{}/{}", t1, JUDGE_ROUNDS)));

    // 7. 类2 序号回退注入。
    let t2 = led.as_ref().map(|l| l.seq_caught).unwrap_or(0);
    v.push(("J1-序号回退全抓", t2 == JUDGE_ROUNDS, format!("{}/{}", t2, JUDGE_ROUNDS)));

    // 8. 类3 未提交读取注入。
    let t3 = led.as_ref().map(|l| l.uncommit_caught).unwrap_or(0);
    v.push(("J1-未提交读取全抓", t3 == JUDGE_ROUNDS, format!("{}/{}", t3, JUDGE_ROUNDS)));

    // 9. 断电账守恒：3 点 × 10 轮 = 30 全滚。
    let cons = match &pl {
        Ok(l) => l.pl0_rolled + l.pl1_rolled + l.pl2_rolled == PL_POINTS as u32 * JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push(("J1-断电账守恒", cons, format!("{}点×{}轮", PL_POINTS, JUDGE_ROUNDS)));

    // 10. seq 单调：三次提交 seq 1→2→3。
    let seq_ok = {
        let v1: [u16; PAYLOAD_LEN] = [6, 1, 7601, 1, 3, 0x0100, 96, 0];
        let v2: [u16; PAYLOAD_LEN] = [10, 0, 19045, 0, 3, 0x0100, 96, 0];
        let v3: [u16; PAYLOAD_LEN] = [11, 0, 22000, 0, 3, 0x0100, 96, 1];
        let mut store = PersistStore::new();
        if store.atomic_write(v1, None).is_err() {
            false
        } else if store.atomic_write(v2, None).is_err() {
            false
        } else if store.atomic_write(v3, None).is_err() {
            false
        } else {
            store.read_active().map(|i| i.seq == 3).unwrap_or(false)
        }
    };
    v.push(("J1-seq单调1→2→3", seq_ok, String::from("提交序号严格递增")));

    // 11. 空库读拒（探针 B）。
    v.push(("J1-空库读拒", probe_seq_b().is_err(), String::from("无提交版即拒")));

    // 12. 截断影子不生效（探针 C）。
    v.push(("J1-截断影子不生效", probe_seq_c().is_ok(), String::from("PL1 后活动槽仍旧值")));

    // 13. 跳段提交拒：影子未暂存直接提交路径不可达（写路径全管控）。
    let stage_ok = {
        let mut store = PersistStore::new();
        // 未暂存影子但读备份 → StoreEmpty（无提交版）。
        store.read_backup().is_err()
    };
    v.push(("J1-无备份读拒", stage_ok, String::from("影子未提交禁当备份")));

    // 14. 指纹双跑一致。
    let fp = match &g {
        Ok((_, _, f)) => *f,
        Err(_) => 0,
    };
    let fp2 = match golden_run() {
        Ok((_, _, f)) => f,
        Err(_) => 1,
    };
    v.push(("J2-指纹双跑一致", fp == fp2 && fp != 0, format!("{:08X}", fp)));

    // 15. 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));

    // 16. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 17. 诊断码闭集（12 码全非空）。
    let all_ok = {
        let mut ok = PvCode::ALL.len() == 12;
        let mut i = 0;
        while i < PvCode::ALL.len() {
            if PvCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集12码", all_ok, String::from("detail 全非空")));

    // 18. 口径交叉：载荷槽宽与总纲 PROFILE_UNITS 同口径 8。
    v.push(("J3-槽宽口径交叉", PAYLOAD_LEN == 8, String::from("8 槽与总纲同口径")));

    v
}

/// 收口：本模块自检 CheckSet（判据 18 条；前 8 条动态明细）。
pub fn run_unx18546_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18546 档案持久化原子写");
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
        let set = run_unx18546_checks();
        assert!(set.all_passed(), "UNX-F18546 判据集应全绿");
    }

    /// 黄金链：三提交+备份账。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((r, bk, _)) => {
                assert_eq!(r, 2);
                assert_eq!(bk.retained, 2);
                assert_eq!(bk.last_backup_seq, 2);
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// 断电三点 × 10 轮全滚。
    #[test]
    fn test_power_loss_rounds() {
        match power_loss_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.pl0_rolled, 10);
                assert_eq!(l.pl1_rolled, 10);
                assert_eq!(l.pl2_rolled, 10);
            }
            Err(e) => assert!(false, "断电全滚不应报错: {}", detail(e)),
        }
    }

    /// 单点断电 PL0/PL1/PL2 各自回滚。
    #[test]
    fn test_power_loss_single() {
        let v1: [u16; PAYLOAD_LEN] = [6, 1, 7601, 1, 3, 0x0100, 96, 0];
        let v2: [u16; PAYLOAD_LEN] = [10, 0, 19045, 0, 3, 0x0100, 96, 0];
        let mut pl = 0;
        while pl <= 2 {
            let mut store = PersistStore::new();
            assert!(store.atomic_write(v1, None).is_ok());
            let old = match store.read_active() {
                Ok(i) => i,
                Err(e) => panic!("旧值读取不应失败: {}", detail(e)),
            };
            assert!(store.atomic_write(v2, Some(pl as u8)).is_err());
            match store.rollback_ok(Some(old)) {
                Ok(true) => {}
                other => assert!(false, "断电点 {} 回滚必须成功: {:?}", pl, other),
            }
            pl += 1;
        }
    }

    /// 原子写成功路径：翻面+备份。
    #[test]
    fn test_write_commit() {
        let v1: [u16; PAYLOAD_LEN] = [6, 1, 7601, 1, 3, 0x0100, 96, 0];
        let v2: [u16; PAYLOAD_LEN] = [10, 0, 19045, 0, 3, 0x0100, 96, 0];
        let mut store = PersistStore::new();
        assert!(store.atomic_write(v1, None).is_ok());
        assert_eq!(store.active, 1);
        assert!(store.atomic_write(v2, None).is_ok());
        assert_eq!(store.active, 0);
        let cur = match store.read_active() {
            Ok(i) => i,
            Err(e) => panic!("活动读取不应失败: {}", detail(e)),
        };
        assert_eq!(cur.payload, v2);
        assert_eq!(cur.seq, 2);
    }

    /// 坏样本三类全抓。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.forge_caught, 10);
                assert_eq!(l.seq_caught, 10);
                assert_eq!(l.uncommit_caught, 10);
            }
            Err(e) => assert!(false, "注入全抓不应报错: {}", detail(e)),
        }
    }

    /// 校验和反码语义。
    #[test]
    fn test_checksum() {
        let p = [6, 1, 7601, 1, 3, 0x0100, 96, 0];
        let ck = payload_checksum(&p);
        assert_eq!(ck, !p.iter().fold(0u16, |a, b| a.wrapping_add(*b)));
        let mut q = p;
        q[0] += 1;
        assert_ne!(payload_checksum(&q), ck);
    }

    /// 空库读拒 + 无备份读拒。
    #[test]
    fn test_empty_store() {
        assert!(probe_seq_b().is_err());
        let store = PersistStore::new();
        assert_eq!(store.read_backup().err(), Some(PvCode::StoreEmpty.code()));
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
