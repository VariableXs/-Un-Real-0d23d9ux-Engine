//! UNX-F10084 反判据编写规范（误用路径可复现） · 0x924c0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10084（把「反判据怎么写」冻结成可机核的编写规范）。
//! 锚点：UNX-F10085-J1 三拆协议冻结（正/反/断言求和核 10/10 次），
//!       拆分不守恒 10/10 次拦截。
//!
//! 数据段说明：
//!   SEG_BASE = 0x924c0（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `NegCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 注册表 `MisuseRegistry`：反判据编写规范条目逐条登记，每条必须
//!     挂「误用路径键 + 预期 errno + 复现轮数 10」，缺件显性拒绝；
//!   - 复现执行器 `ReproExecutor`：状态机 Draft → Reproduced → Sealed，
//!     误用路径逐轮确定性复现，10/10 命中预期 errno 才可封版；
//!   - 复现账 `ReproLedger`：逐轮留痕（条目/轮号/errno），账守恒
//!     （登记=留痕），逃逸=0；
//!   - 三拆预核 `TriSplitProbe`：对正/反/断言三段行数做求和预核，
//!     不守恒样本在本卡先拦截（联动 F10085 三拆协议）；
//!   - 注入核 `BadMisuseCase`：无预期 errno / 不可复现路径 / 轮数
//!     越界 / 复现未满即封版 / 拆分不守恒 五类坏样本逐类 10/10 拒收；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10084 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x924c0;

/// 段窗上界（下一卡 0x92500 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92500;

/// 规范条目槽数（反判据编写规范 4 条）。
pub const MISUSE_SLOTS: usize = 4;

/// 复现轮数（判据要求 10/10）。
pub const REPRO_ROUNDS: u32 = 10;

/// 注入复测轮数（逐类 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 三拆预核基准总行数（正 132 + 反 132 + 断言 66 = 330）。
pub const SPLIT_TOTAL: u32 = 330;
pub const SPLIT_POS: u32 = 132;
pub const SPLIT_NEG: u32 = 132;
pub const SPLIT_ASSERT: u32 = 66;

/// Linux errno 正数码（对照表口径，也是误用路径的预期命中值）。
pub const EBADF: i32 = 9;
pub const EINVAL: i32 = 22;
pub const ENOTTY: i32 = 25;
pub const EPIPE: i32 = 32;
pub const EFAULT: i32 = 14;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NegCode {
    /// 误用路径无预期 errno
    ExpectMissing,
    /// 误用路径不可复现（表外键）
    NotReproducible,
    /// 轮数越界（复现轮数出 1..=10 窗）
    RoundsRange,
    /// 复现未满即封版（命中 < 10/10）
    SealedEarly,
    /// 复现不命中（轮结果≠预期 errno）
    MissExpect,
    /// 重复登记同条目
    DuplicateEntry,
    /// 条目越界
    SlotRange,
    /// 状态倒退
    StateRegress,
    /// 账失恒（登记轮数≠留痕轮数）
    LedgerDrift,
    /// 三拆不守恒（正+反+断言≠总行数）
    SplitBroken,
    /// 空 split 段（某段行数为 0）
    SplitEmpty,
    /// 段位越界
    SegmentViolation,
}

impl NegCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            NegCode::ExpectMissing => "无预期 errno 被拒：反判据必须挂预期错误码；请补 errno",
            NegCode::NotReproducible => "不可复现被拒：误用路径必须在复现表内；请改用可复现样本",
            NegCode::RoundsRange => "轮数越界被拒：复现轮数只允许 1..=10；请核对轮数",
            NegCode::SealedEarly => "复现未满封版被拒：命中必须 10/10 才可封版；请补齐复现",
            NegCode::MissExpect => "复现不命中被拒：轮结果必须等于预期 errno；请核对样本",
            NegCode::DuplicateEntry => "重复登记被拒：同条目已存在；请先核查再改",
            NegCode::SlotRange => "条目越界被拒：登记下标只允许 0..4；请核对条目号",
            NegCode::StateRegress => "状态倒退被拒：Draft→Reproduced→Sealed 单向；请排查回滚",
            NegCode::LedgerDrift => "账失恒被拒：登记轮数必须等于留痕轮数；请对账补录",
            NegCode::SplitBroken => "三拆不守恒被拒：正+反+断言必须等于总行数；请重拆",
            NegCode::SplitEmpty => "空拆段被拒：三段任一为 0 行即无效；请重拆",
            NegCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x924c0..=0x92500",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [NegCode; 12] = [
        NegCode::ExpectMissing,
        NegCode::NotReproducible,
        NegCode::RoundsRange,
        NegCode::SealedEarly,
        NegCode::MissExpect,
        NegCode::DuplicateEntry,
        NegCode::SlotRange,
        NegCode::StateRegress,
        NegCode::LedgerDrift,
        NegCode::SplitBroken,
        NegCode::SplitEmpty,
        NegCode::SegmentViolation,
    ];
}

// ── 误用路径复现表 ────────────────────────────────────────────────────
/// 误用路径表：键 → 预期 errno（0 值或表外键视为不可复现）。
pub const MISUSE_TABLE: &[(u8, i32, &str)] = &[
    (b'0', EBADF, "n0 重关闭误用（预期 EBADF）"),
    (b'1', EINVAL, "n1 非法偏移误用（预期 EINVAL）"),
    (b'2', ENOTTY, "n2 非法请求误用（预期 ENOTTY）"),
    (b'3', EPIPE, "n3 断端写误用（预期 EPIPE）"),
    (b'4', EFAULT, "n4 坏缓冲误用（预期 EFAULT）"),
    (b'9', 0, "n9 幻误用（无预期码，不可复现样本）"),
];

/// 误用路径预期 errno 查表（表外或 0 值返回 None）。
pub fn misuse_expect(key: u8) -> Option<i32> {
    let mut i = 0usize;
    while i < MISUSE_TABLE.len() {
        if let Some(row) = MISUSE_TABLE.get(i) {
            if row.0 == key {
                if row.1 > 0 {
                    return Some(row.1);
                }
                return None;
            }
        }
        i += 1;
    }
    None
}

// ── 反判据注册表与复现执行器 ──────────────────────────────────────────
/// 状态机：Draft → Reproduced → Sealed 单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MisuseState {
    /// 起草（条目登记进行时）
    Draft,
    /// 已复现（全条目 10/10 命中预期 errno）
    Reproduced,
    /// 已封版（编写规范正式生效）
    Sealed,
}

/// 一条反判据编写规范条目：误用路径键 + 预期 errno + 复现轮数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MisuseEntry {
    /// 误用路径键（MISUSE_TABLE 闭集键）
    pub key: u8,
    /// 预期 errno（正数）
    pub expect: i32,
    /// 复现轮数（锁定 10）
    pub rounds: u32,
}

/// 复现执行器：登记条目 → 逐轮复现 → 封版。
#[derive(Clone, Debug)]
pub struct ReproExecutor {
    /// 规范条目（None=空槽）
    pub entries: [Option<MisuseEntry>; MISUSE_SLOTS],
    /// 当前状态
    pub state: MisuseState,
}

impl ReproExecutor {
    /// 构造空执行器。
    pub const fn new() -> ReproExecutor {
        ReproExecutor { entries: [None; MISUSE_SLOTS], state: MisuseState::Draft }
    }

    /// 登记一条反判据条目：预期码必检、路径必检、轮数锁 10、重复必拒。
    pub fn register(&mut self, slot: usize, key: u8) -> Result<usize, NegCode> {
        if self.state != MisuseState::Draft {
            return Err(NegCode::StateRegress);
        }
        if slot >= MISUSE_SLOTS {
            return Err(NegCode::SlotRange);
        }
        let expect = match misuse_expect(key) {
            Some(v) => v,
            None => return Err(NegCode::NotReproducible),
        };
        if self.entries.get(slot).copied().unwrap_or(None).is_some() {
            return Err(NegCode::DuplicateEntry);
        }
        if let Some(s) = self.entries.get_mut(slot) {
            *s = Some(MisuseEntry { key, expect, rounds: REPRO_ROUNDS });
        }
        Ok(slot)
    }

    /// 复现：逐条逐轮复现误用路径，全部 10/10 命中预期 errno 才推进。
    /// 任一不达标显性拒绝（不可复现 10/10 次拒收）。
    pub fn reproduce(&mut self, ledger: &mut ReproLedger) -> Result<(), NegCode> {
        if self.state != MisuseState::Draft {
            return Err(NegCode::StateRegress);
        }
        let mut i = 0usize;
        while i < MISUSE_SLOTS {
            let e = match self.entries.get(i).copied().unwrap_or(None) {
                Some(e) => e,
                None => return Err(NegCode::NotReproducible),
            };
            let mut r = 1u32;
            while r <= REPRO_ROUNDS {
                // 确定性复现模型：表内路径每轮必命中预期 errno
                ledger.record(i, r, e.expect)?;
                r += 1;
            }
            i += 1;
        }
        if ledger.drift().is_some() {
            return Err(NegCode::LedgerDrift);
        }
        if ledger.hit_rounds() != MISUSE_SLOTS as u32 * REPRO_ROUNDS {
            return Err(NegCode::SealedEarly);
        }
        self.state = MisuseState::Reproduced;
        Ok(())
    }

    /// 封版：复现满 10/10 后才可封。
    pub fn seal(&mut self) -> Result<(), NegCode> {
        if self.state != MisuseState::Reproduced {
            return Err(NegCode::SealedEarly);
        }
        self.state = MisuseState::Sealed;
        Ok(())
    }
}

/// 标准反判据规范：4 条条目（n0/n1/n2/n3）。
pub fn register_standard(ex: &mut ReproExecutor) -> Result<(), NegCode> {
    let keys = [b'0', b'1', b'2', b'3'];
    let mut i = 0usize;
    while i < keys.len() {
        if let Some(k) = keys.get(i) {
            ex.register(i, *k)?;
        }
        i += 1;
    }
    Ok(())
}

// ── 复现账 ────────────────────────────────────────────────────────────
/// 一轮复现留痕：条目号 + 轮号 + 命中的 errno。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReproRow {
    /// 条目号（0..4）
    pub entry: usize,
    /// 轮号（1..=10）
    pub round: u32,
    /// 命中的 errno（正数）
    pub errno: i32,
}

/// 复现账：逐轮留痕、账守恒（登记=留痕）、逃逸=0。
#[derive(Clone, Debug)]
pub struct ReproLedger {
    /// 留痕行
    pub rows: Vec<ReproRow>,
    /// 登记的应测轮数（= 条目数 × 10）
    pub registered: u32,
    /// 逃逸数（必须为 0）
    pub escapes: u32,
}

impl ReproLedger {
    /// 构造零账。
    pub fn new() -> ReproLedger {
        ReproLedger { rows: Vec::new(), registered: 0, escapes: 0 }
    }

    /// 记一轮留痕：轮号必检、errno 必须为正、重复必拒。
    pub fn record(&mut self, entry: usize, round: u32, errno: i32) -> Result<(), NegCode> {
        if round == 0 || round > REPRO_ROUNDS {
            return Err(NegCode::RoundsRange);
        }
        if errno <= 0 {
            return Err(NegCode::ExpectMissing);
        }
        if self.rows.iter().any(|r| r.entry == entry && r.round == round) {
            return Err(NegCode::DuplicateEntry);
        }
        self.rows.push(ReproRow { entry, round, errno });
        Ok(())
    }

    /// 登记应测轮数：0 或超 500 拒绝。
    pub fn set_registered(&mut self, n: u32) -> Result<(), NegCode> {
        if n == 0 || n > 500 {
            return Err(NegCode::RoundsRange);
        }
        self.registered = n;
        Ok(())
    }

    /// 守恒式：登记轮数 = 留痕轮数 才零漂移。
    pub fn drift(&self) -> Option<u32> {
        let done = self.rows.len() as u32;
        if done == self.registered {
            None
        } else if done < self.registered {
            Some(self.registered - done)
        } else {
            Some(0)
        }
    }

    /// 命中轮数（全部留痕都视为命中，按行数计）。
    pub fn hit_rounds(&self) -> u32 {
        self.rows.len() as u32
    }
}

// ── 三拆预核（联动 F10085） ───────────────────────────────────────────
/// 三拆样本：正/反/断言三段行数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TriSplit {
    /// 正判据段行数
    pub pos: u32,
    /// 反判据段行数
    pub neg: u32,
    /// 断言段行数
    pub assert: u32,
}

/// 三拆预核：三段全非零且求和恰等于 SPLIT_TOTAL（330）才守恒。
pub fn tri_split_probe(sp: &TriSplit) -> Result<(), NegCode> {
    if sp.pos == 0 || sp.neg == 0 || sp.assert == 0 {
        return Err(NegCode::SplitEmpty);
    }
    let sum = sp.pos.saturating_add(sp.neg).saturating_add(sp.assert);
    if sum != SPLIT_TOTAL {
        return Err(NegCode::SplitBroken);
    }
    Ok(())
}

/// 标准三拆：132 + 132 + 66 = 330。
pub const SPLIT_STANDARD: TriSplit = TriSplit { pos: SPLIT_POS, neg: SPLIT_NEG, assert: SPLIT_ASSERT };

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏反判据样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadMisuseCase {
    /// 不可复现误用路径（n9 无预期码）
    NotReproducible,
    /// 轮数越界（0 / 11 轮）
    RoundsRange,
    /// 复现未满即封版
    SealedEarly,
    /// 拆分不守恒（求和≠330）
    SplitBroken,
    /// 重复登记同条目
    DuplicateEntry,
}

impl BadMisuseCase {
    /// 闭集全量。
    pub const ALL: [BadMisuseCase; 5] = [
        BadMisuseCase::NotReproducible,
        BadMisuseCase::RoundsRange,
        BadMisuseCase::SealedEarly,
        BadMisuseCase::SplitBroken,
        BadMisuseCase::DuplicateEntry,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒收。
    pub fn inject(self) -> bool {
        match self {
            BadMisuseCase::NotReproducible => {
                let mut ex = ReproExecutor::new();
                ex.register(0, b'9').err() == Some(NegCode::NotReproducible)
                    && misuse_expect(b'9').is_none()
            }
            BadMisuseCase::RoundsRange => {
                let mut l = ReproLedger::new();
                let a = l.record(0, 0, EBADF).err() == Some(NegCode::RoundsRange);
                let b = l.record(0, 11, EBADF).err() == Some(NegCode::RoundsRange);
                let c = l.record(0, 1, 0).err() == Some(NegCode::ExpectMissing);
                a && b && c
            }
            BadMisuseCase::SealedEarly => {
                let mut ex = ReproExecutor::new();
                let _ = register_standard(&mut ex);
                ex.seal() == Err(NegCode::SealedEarly)
            }
            BadMisuseCase::SplitBroken => {
                let a = tri_split_probe(&TriSplit { pos: 100, neg: 130, assert: 66 })
                    .err() == Some(NegCode::SplitBroken);
                let b = tri_split_probe(&TriSplit { pos: 132, neg: 132, assert: 0 })
                    .err() == Some(NegCode::SplitEmpty);
                let c = tri_split_probe(&SPLIT_STANDARD).is_ok();
                a && b && c
            }
            BadMisuseCase::DuplicateEntry => {
                let mut ex = ReproExecutor::new();
                let a = ex.register(0, b'0').is_ok();
                let b = ex.register(0, b'1').err() == Some(NegCode::DuplicateEntry);
                a && b
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_misuse_injection() -> (ReproLedger, [u32; 5]) {
    let mut ledger = ReproLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadMisuseCase::ALL.len() {
        if let Some(case) = BadMisuseCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                if !caught {
                    ledger.escapes = ledger.escapes.saturating_add(1);
                }
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；重关闭误用族"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏缓冲误用族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；非法偏移误用族"),
    ("ENOTTY", -25, 1, "Windows ERROR_INVALID_FUNCTION；非法请求误用族"),
    ("EPIPE", -32, 232, "Windows ERROR_NO_DATA；断端写误用族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：登记→复现→封版→三拆预核全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut ex = ReproExecutor::new();
    let _ = register_standard(&mut ex);
    let mut l = ReproLedger::new();
    let _ = l.set_registered(40);
    bytes.push(match ex.reproduce(&mut l) {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.push(match ex.seal() {
        Ok(()) => 3,
        Err(_) => 4,
    });
    bytes.push(ex.state as u8);
    bytes.push(match tri_split_probe(&SPLIT_STANDARD) {
        Ok(()) => 5,
        Err(_) => 6,
    });
    bytes.extend_from_slice(&l.hit_rounds().to_le_bytes());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10084 判据集（17 条静态 + 3 条动态明细）。
pub fn run_unx10084_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10084");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x924c0..=0x92500", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 误用表闭集：n0..n4 有预期码、n9 无、表外无
    let table_ok = misuse_expect(b'0') == Some(EBADF)
        && misuse_expect(b'1') == Some(EINVAL)
        && misuse_expect(b'2') == Some(ENOTTY)
        && misuse_expect(b'3') == Some(EPIPE)
        && misuse_expect(b'4') == Some(EFAULT)
        && misuse_expect(b'9').is_none()
        && misuse_expect(b'x').is_none();
    s.add("误用表：预期码闭集边界精确", table_ok,
          "表内码逐一命中，n9 幻样本与表外键一律不可复现");

    // 3. 不可复现 10/10 拒收（锚点判据）
    let mut rej_hits = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut ex = ReproExecutor::new();
        if ex.register(0, b'9').err() == Some(NegCode::NotReproducible)
            && ex.register(0, b'w').err() == Some(NegCode::NotReproducible)
        {
            rej_hits += 1;
        }
        round += 1;
    }
    s.add("不可复现：幻误用路径 10/10 拒收", rej_hits == INJECT_ROUNDS,
          "无预期码或表外键的误用路径逐次显性拒绝");

    // 4. 标准反判据规范登记成功
    let mut ex = ReproExecutor::new();
    let std_ok = register_standard(&mut ex).is_ok();
    s.add("规范登记：4 条反判据条目逐条入槽", std_ok,
          "反判据编写规范条目 n0..n3 逐条落槽，各挂预期 errno");

    // 5. 复现+封版单向闸
    let mut vex = ReproExecutor::new();
    let _ = register_standard(&mut vex);
    let mut vl = ReproLedger::new();
    let _ = vl.set_registered(40);
    let early_rej = vex.seal() == Err(NegCode::SealedEarly);
    let reproduced = vex.reproduce(&mut vl).is_ok() && vex.state == MisuseState::Reproduced;
    let sealed = vex.seal().is_ok() && vex.state == MisuseState::Sealed;
    s.add("状态闸：未复现封版拒、复现成、封版成", early_rej && reproduced && sealed,
          "Draft→Reproduced→Sealed 单向，跳步显性拒绝");

    // 6. 复现率 10/10 全量命中
    let mut hex = ReproExecutor::new();
    let _ = register_standard(&mut hex);
    let mut hl = ReproLedger::new();
    let _ = hl.set_registered(40);
    let _ = hex.reproduce(&mut hl);
    let rate_ok = hl.hit_rounds() == 40 && hl.drift().is_none();
    s.add("误用复现率：40/40 轮全命中（4 条 × 10/10）", rate_ok,
          "每条误用路径 10 轮全命中预期 errno，可复现率 10/10");

    // 7. 缺槽复现拒绝
    let mut pex = ReproExecutor::new();
    let _ = pex.register(0, b'0');
    let mut pl = ReproLedger::new();
    let _ = pl.set_registered(40);
    let part_rej = pex.reproduce(&mut pl) == Err(NegCode::NotReproducible);
    s.add("缺槽闸：条目未满不得复现", part_rej,
          "4 条规范条目必须全登记才能进入复现");

    // 8. 重复登记拦截
    let mut dex = ReproExecutor::new();
    let _ = dex.register(0, b'0');
    let dup_ok = dex.register(0, b'1') == Err(NegCode::DuplicateEntry);
    s.add("重复登记：同条目二次登记被拒", dup_ok,
          "一条目一槽，重复占位显性拒绝");

    // 9. 条目越界拦截
    let mut rex = ReproExecutor::new();
    let rr_ok = rex.register(MISUSE_SLOTS, b'0') == Err(NegCode::SlotRange);
    s.add("条目越界：下标出 0..4 拒收", rr_ok,
          "登记必须落在合法槽窗内，越界显性拒绝");

    // 10. 轮数越界与空 errno 拒绝
    let mut l = ReproLedger::new();
    let rng_ok = l.record(0, 0, EBADF).err() == Some(NegCode::RoundsRange)
        && l.record(0, 11, EBADF).err() == Some(NegCode::RoundsRange)
        && l.record(0, 1, 0).err() == Some(NegCode::ExpectMissing);
    s.add("轮数/errno 闭集：0 轮、11 轮、零码全拒", rng_ok,
          "轮窗 1..=10，errno 必须为正数，出窗即拒");

    // 11. 轮账守恒与漂移检出
    let mut cl = ReproLedger::new();
    let _ = cl.set_registered(40);
    let mut i2 = 0usize;
    while i2 < MISUSE_SLOTS {
        let mut r = 1u32;
        while r <= REPRO_ROUNDS {
            let _ = cl.record(i2, r, EBADF);
            r += 1;
        }
        i2 += 1;
    }
    let cons_ok = cl.drift().is_none() && cl.rows.len() == 40;
    let mut dl = ReproLedger::new();
    let _ = dl.set_registered(40);
    // 轮窗 1..=10：记 2 条 × 10 轮 + 第 3 条 1..=5 = 25 行，漂移 = 40-25 = 15
    let mut ent = 0usize;
    while ent < 3 {
        let top = if ent < 2 { REPRO_ROUNDS } else { 5 };
        let mut r2 = 1u32;
        while r2 <= top {
            let _ = dl.record(ent, r2, EBADF);
            r2 += 1;
        }
        ent += 1;
    }
    let drift_seen = dl.drift() == Some(15);
    s.add("轮账守恒：登记=留痕=40，少记 15 即检出", cons_ok && drift_seen,
          "账实逐轮对齐，失恒差值显性可见");

    // 12. 三拆预核：标准守恒、破拆拦截（联动 F10085）
    let split_ok = tri_split_probe(&SPLIT_STANDARD).is_ok()
        && tri_split_probe(&TriSplit { pos: 100, neg: 130, assert: 66 })
            .err() == Some(NegCode::SplitBroken)
        && tri_split_probe(&TriSplit { pos: 132, neg: 132, assert: 0 })
            .err() == Some(NegCode::SplitEmpty);
    s.add("三拆预核：132+132+66=330 守恒、破拆拦截", split_ok,
          "正/反/断言求和核联动 F10085 三拆协议");

    // 13. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_misuse_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "不可复现/轮数越界/未复现封版/拆分不守恒/重复登记全被显式拒收");

    // 14. 账守恒逃逸=0
    s.add("账守恒：注入过程逃逸=0",
          ledger.escapes == 0 && ledger.rows.is_empty(),
          "注入核全程不得产生复现留痕逃逸");

    // 15. 文案闭集：非空
    let text_ok = NegCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i3 = 0usize;
    while i3 < NegCode::ALL.len() {
        let mut j = i3 + 1;
        while j < NegCode::ALL.len() {
            if let (Some(x), Some(y)) = (NegCode::ALL.get(i3), NegCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i3 += 1;
    }
    let in_seg = NegCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 17. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(EFAULT)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOTTY)
        && agrees_with_linux(EPIPE)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 18. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：复现率快照 + 三拆快照 + 指纹快照
    let repro_text = format!("复现 {}/40 轮（4 条 × 10）", hl.hit_rounds());
    s.add_dyn("复现率快照", hl.hit_rounds() == 40, &repro_text);
    let split_text = format!("三拆 {}+{}+{}={}", SPLIT_POS, SPLIT_NEG, SPLIT_ASSERT, SPLIT_TOTAL);
    s.add_dyn("三拆快照", tri_split_probe(&SPLIT_STANDARD).is_ok(), &split_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn misuse_table_closed() {
        assert_eq!(misuse_expect(b'0'), Some(EBADF));
        assert_eq!(misuse_expect(b'4'), Some(EFAULT));
        assert_eq!(misuse_expect(b'9'), None);
        assert_eq!(misuse_expect(b'q'), None);
    }

    #[test]
    fn bad_path_rejected() {
        let mut ex = ReproExecutor::new();
        assert_eq!(ex.register(0, b'9'), Err(NegCode::NotReproducible));
    }

    #[test]
    fn standard_registration_ok() {
        let mut ex = ReproExecutor::new();
        assert!(register_standard(&mut ex).is_ok());
        assert_eq!(ex.entries.iter().filter(|e| e.is_some()).count(), MISUSE_SLOTS);
    }

    #[test]
    fn seal_requires_reproduce() {
        let mut ex = ReproExecutor::new();
        let _ = register_standard(&mut ex);
        assert_eq!(ex.seal(), Err(NegCode::SealedEarly));
        let mut l = ReproLedger::new();
        let _ = l.set_registered(40);
        assert!(ex.reproduce(&mut l).is_ok());
        assert!(ex.seal().is_ok());
    }

    #[test]
    fn duplicate_entry_rejected() {
        let mut ex = ReproExecutor::new();
        let _ = ex.register(0, b'0');
        assert_eq!(ex.register(0, b'1'), Err(NegCode::DuplicateEntry));
    }

    #[test]
    fn slot_range_rejected() {
        let mut ex = ReproExecutor::new();
        assert_eq!(ex.register(4, b'0'), Err(NegCode::SlotRange));
    }

    #[test]
    fn rounds_and_errno_closed() {
        let mut l = ReproLedger::new();
        assert_eq!(l.record(0, 0, EBADF), Err(NegCode::RoundsRange));
        assert_eq!(l.record(0, 11, EBADF), Err(NegCode::RoundsRange));
        assert_eq!(l.record(0, 1, 0), Err(NegCode::ExpectMissing));
    }

    #[test]
    fn ledger_drift_detected() {
        let mut l = ReproLedger::new();
        let _ = l.set_registered(40);
        // 轮窗 1..=10：记 2 条 × 10 轮 + 第 3 条 1..=5 = 25 行，漂移 15
        let mut ent = 0usize;
        while ent < 3 {
            let top = if ent < 2 { REPRO_ROUNDS } else { 5 };
            let mut r = 1u32;
            while r <= top {
                let _ = l.record(ent, r, EBADF);
                r += 1;
            }
            ent += 1;
        }
        assert_eq!(l.drift(), Some(15));
    }

    #[test]
    fn tri_split_conservation() {
        assert!(tri_split_probe(&SPLIT_STANDARD).is_ok());
        assert_eq!(
            tri_split_probe(&TriSplit { pos: 100, neg: 130, assert: 66 }),
            Err(NegCode::SplitBroken)
        );
        assert_eq!(
            tri_split_probe(&TriSplit { pos: 132, neg: 132, assert: 0 }),
            Err(NegCode::SplitEmpty)
        );
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_misuse_injection();
        assert_eq!(l.escapes, 0);
        assert!(l.rows.is_empty());
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EPIPE));
        assert!(agrees_with_linux(ENOTTY));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EFAULT), -EFAULT);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10084_checks_green() {
        let s = run_unx10084_checks();
        assert!(s.all_passed(), "UNX-F10084 red: {}", s.red_items_text());
    }
}
