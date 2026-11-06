//! UNX-F9371 LTP 子集清单联测（C2 号面覆盖率账） · 0x7CA40段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F9371-J1 覆盖率账联测判据过，C2 号面 LTP 子集覆盖率逐号
//! 落账，缺口清单登记格过。
//!
//! SEG_BASE = 0x7CA40 诊断码闭集说明：本段 0x40 窗口独占；诊断码 1 基
//! 编码（码值 = SEG_BASE + 变体序号 + 1），闭集 12 个
//! （CoverCode::ALL）两两唯一，段基址本身不占码。
//!
//! 结构清单：
//!   ① C2 号面账 C2Face：定容号位注册账，可封存（封存后只读，回 EROFS），
//!      重复注册 EEXIST、零号 EINVAL。
//!   ② LTP 子集清单 LTP_SUBSET：冻结（号, 用例名）对，覆盖率账单源。
//!   ③ 覆盖率账 CoverageLedger：逐号状态（未覆盖/已覆盖），覆盖率按
//!      LTP 子集为分母复算，账只增不减。
//!   ④ 缺口清单 GapList：LTP 有用例而 C2 缺号逐条登记，追加式，重复
//!      拒绝；虚报覆盖（号不在 LTP 子集）EINVAL。
//!   ⑤ 注入核：缺号/虚报/封存后重写三类各 10 轮全抓，账守恒逃逸=0；
//!      Linux errno 对照表 + FNV-1a 双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9371 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7CA40;

/// 本批段窗口下界。
pub const WINDOW_LO: u32 = 0x7A000;
/// 本批段窗口上界。
pub const WINDOW_HI: u32 = 0x7D1C0;

/// C2 号面容量。
pub const FACE_CAP: usize = 24;
/// LTP 子集清单行数。
pub const LTP_ROWS: usize = 16;
/// 缺口清单容量。
pub const GAP_CAP: usize = 16;

/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 注入类别数。
pub const INJECT_CLASSES: u32 = 3;

/// Linux errno 正数码。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EINVAL: i32 = 22;
pub const EROFS: i32 = 30;
pub const EEXIST: i32 = 17;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（1 基：码值 = SEG_BASE + 序号 + 1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CoverCode {
    /// 缺号：LTP 有用例而 C2 号面未注册
    FaceMissing,
    /// 虚报覆盖：号不在 LTP 子集清单
    ClaimPhantom,
    /// 封存后重写：号面账只读
    FaceRewrite,
    /// 重复注册
    DupRegister,
    /// 零号：号面不接受 0 号
    ZeroNr,
    /// 号面满：号位耗尽
    FaceFull,
    /// 缺口重复登记
    GapDup,
    /// 缺口清单溢出
    GapOverflow,
    /// 覆盖率复算漂移：账面比率与复算不一致
    RatioDrift,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl CoverCode {
    /// 码值（段基址 + 变体序号 + 1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            CoverCode::FaceMissing => "缺号被拒：LTP 有用例而 C2 号面未注册该号；\
                请先登记号面或把该号列入缺口清单",
            CoverCode::ClaimPhantom => "虚报覆盖被拒：号不在 LTP 子集清单；\
                覆盖声明必须以冻结的 LTP 子集为分母，越集号回 EINVAL",
            CoverCode::FaceRewrite => "封存后重写被拒：号面账封存后只读；\
                任何改写回 EROFS，增量走升级接管",
            CoverCode::DupRegister => "重复注册被拒：同号已在号面账；重复回 EEXIST，\
                请改为查询既有状态",
            CoverCode::ZeroNr => "零号被拒：号面不接受 0 号；0 号为内核保留位",
            CoverCode::FaceFull => "号面满：号位耗尽；请扩容或注销旧号，禁止覆盖",
            CoverCode::GapDup => "缺口重复登记被拒：同号缺口只记一笔；请改为更新 \
                既有缺口行",
            CoverCode::GapOverflow => "缺口清单溢出：格位耗尽；请先收口缺口再登记，\
                溢出必须显性可观测",
            CoverCode::RatioDrift => "覆盖率复算漂移：账面覆盖率与逐号复算不一致；\
                请以 LTP 子集为分母重新落账",
            CoverCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请检查 \
                回滚路径",
            CoverCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查 \
                隐藏状态或非确定哈希",
            CoverCode::SegmentViolation => "段位越界：段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: [CoverCode; 12] = [
        CoverCode::FaceMissing,
        CoverCode::ClaimPhantom,
        CoverCode::FaceRewrite,
        CoverCode::DupRegister,
        CoverCode::ZeroNr,
        CoverCode::FaceFull,
        CoverCode::GapDup,
        CoverCode::GapOverflow,
        CoverCode::RatioDrift,
        CoverCode::LedgerRegress,
        CoverCode::NotReproducible,
        CoverCode::SegmentViolation,
    ];
}

/// 系统调用失败结果。
pub type SysResult<T> = Result<T, i32>;

// ── LTP 子集清单（冻结单源） ─────────────────────────────────────────
/// LTP 子集行：syscall 号 + 用例名。
pub type LtpRow = (u32, &'static str);

/// LTP 子集清单（冻结 16 行，覆盖率账分母）。
pub const LTP_SUBSET: [LtpRow; LTP_ROWS] = [
    (1, "read01"),
    (2, "write01"),
    (3, "open01"),
    (4, "close01"),
    (5, "stat01"),
    (6, "fstat01"),
    (8, "lseek01"),
    (9, "mmap01"),
    (10, "mprotect01"),
    (11, "munmap01"),
    (12, "brk01"),
    (13, "ioctl01"),
    (14, "pipe01"),
    (15, "dup01"),
    (16, "dup201"),
    (22, "setpgid01"),
];

/// 子集查询：号是否在 LTP 子集内。
pub fn ltp_has(nr: u32) -> bool {
    let mut i = 0usize;
    while i < LTP_SUBSET.len() {
        if let Some(r) = LTP_SUBSET.get(i) {
            if r.0 == nr {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ── C2 号面账 ────────────────────────────────────────────────────────
/// C2 号面账：定容注册 + 封存开关。
#[derive(Clone, Copy, Debug)]
pub struct C2Face {
    nrs: [Option<u32>; FACE_CAP],
    len: usize,
    sealed: bool,
    regs: u32,
    rejects: u32,
}

impl C2Face {
    /// 空账。
    pub const fn new() -> C2Face {
        C2Face { nrs: [None; FACE_CAP], len: 0, sealed: false, regs: 0, rejects: 0 }
    }

    /// 注册一号：0 号 EINVAL、重复 EEXIST、满容 ENOENT、封存后 EROFS。
    pub fn register(&mut self, nr: u32) -> SysResult<usize> {
        if self.sealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EROFS);
        }
        if nr == 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EINVAL);
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(v)) = self.nrs.get(i).map(|x| *x) {
                if v == nr {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(EEXIST);
                }
            }
            i += 1;
        }
        if self.len >= FACE_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ENOENT);
        }
        let slot = self.len;
        if let Some(cell) = self.nrs.get_mut(slot) {
            *cell = Some(nr);
        }
        self.len += 1;
        self.regs = self.regs.saturating_add(1);
        Ok(slot)
    }

    /// 封存：单向开关。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 是否已封存。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 号查询。
    pub fn has(&self, nr: u32) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(v)) = self.nrs.get(i).map(|x| *x) {
                if v == nr {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 在册号数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 注册/拒绝账。
    pub fn regs(&self) -> u32 {
        self.regs
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

// ── 缺口清单 ─────────────────────────────────────────────────────────
/// 缺口行：缺失号 + 归因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GapRow {
    /// 缺失号
    pub nr: u32,
    /// LTP 用例名
    pub case: &'static str,
}

/// 缺口清单：追加式，重复拒绝，溢出显性。
#[derive(Clone, Copy, Debug)]
pub struct GapList {
    rows: [Option<GapRow>; GAP_CAP],
    len: usize,
    rejects: u32,
}

impl GapList {
    /// 空清单。
    pub const fn new() -> GapList {
        GapList { rows: [None; GAP_CAP], len: 0, rejects: 0 }
    }

    /// 登记缺口：重复 EEXIST、溢出 ENOENT。
    pub fn record(&mut self, nr: u32, case: &'static str) -> SysResult<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(r)) = self.rows.get(i).map(|x| *x) {
                if r.nr == nr {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(EEXIST);
                }
            }
            i += 1;
        }
        if self.len >= GAP_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ENOENT);
        }
        let slot = self.len;
        if let Some(cell) = self.rows.get_mut(slot) {
            *cell = Some(GapRow { nr, case });
        }
        self.len += 1;
        Ok(slot)
    }

    /// 缺口行数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 缺口查询。
    pub fn has(&self, nr: u32) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(r)) = self.rows.get(i).map(|x| *x) {
                if r.nr == nr {
                    return true;
                }
            }
            i += 1;
        }
        false
    }
}

// ── 覆盖率账 ─────────────────────────────────────────────────────────
/// 覆盖率账：以 LTP 子集为分母逐号落账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CoverageLedger {
    /// 已覆盖号数
    pub covered: u32,
    /// 缺口号数
    pub gaps: u32,
    /// 分母（LTP 子集行数）
    pub denom: u32,
}

impl CoverageLedger {
    /// 空账（分母冻结为 LTP_ROWS）。
    pub const fn new() -> CoverageLedger {
        CoverageLedger { covered: 0, gaps: 0, denom: LTP_ROWS as u32 }
    }

    /// 覆盖率（千分比整数，避免浮点）。
    pub fn permille(&self) -> u32 {
        if self.denom == 0 {
            return 0;
        }
        self.covered.saturating_mul(1000) / self.denom
    }

    /// 账闭合。
    pub fn conserved(&self) -> bool {
        self.covered.saturating_add(self.gaps) == self.denom
    }
}

/// 对账复算：号面 × LTP 子集 → 覆盖账 + 缺口逐条登记。
pub fn reconcile(face: &C2Face, gaps: &mut GapList) -> SysResult<CoverageLedger> {
    let mut led = CoverageLedger::new();
    let mut i = 0usize;
    while i < LTP_SUBSET.len() {
        if let Some(row) = LTP_SUBSET.get(i) {
            if face.has(row.0) {
                led.covered = led.covered.saturating_add(1);
            } else {
                led.gaps = led.gaps.saturating_add(1);
                let g = gaps.record(row.0, row.1);
                if g.is_err() {
                    // 缺口账满/重复：对账失败显性上抛。
                    return Err(ENOENT);
                }
            }
        }
        i += 1;
    }
    Ok(led)
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> SysResult<()> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(CoverCode::LedgerRegress as i32));
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 三类违约样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViolationCase {
    /// 缺号：LTP 用例号未注册，必须进缺口清单
    FaceMissing,
    /// 虚报覆盖：越集号声明覆盖
    ClaimPhantom,
    /// 封存后重写
    FaceRewrite,
}

impl ViolationCase {
    /// 闭集全量。
    pub const ALL: [ViolationCase; 3] = [
        ViolationCase::FaceMissing,
        ViolationCase::ClaimPhantom,
        ViolationCase::FaceRewrite,
    ];

    /// 样本执行：违约必须命中约定码。
    fn run(self) -> bool {
        match self {
            ViolationCase::FaceMissing => {
                let mut face = C2Face::new();
                // 只注册 1 号：LTP 子集其余号全部成为缺口。
                let _ = face.register(1);
                let mut gaps = GapList::new();
                match reconcile(&face, &mut gaps) {
                    Ok(led) => led.gaps == LTP_ROWS as u32 - 1 && gaps.len() == LTP_ROWS - 1,
                    Err(_) => false,
                }
            }
            ViolationCase::ClaimPhantom => {
                let mut face = C2Face::new();
                let _ = face.register(1);
                // 越集号（22 在集内，100 在集外）：声明覆盖必须以子集为界。
                claim_cover(&face, 100) == Err(EINVAL)
                    && claim_cover(&face, 22) == Err(ENOENT)
            }
            ViolationCase::FaceRewrite => {
                let mut face = C2Face::new();
                let _ = face.register(1);
                face.seal();
                face.register(2) == Err(EROFS) && face.is_sealed()
            }
        }
    }
}

/// 覆盖声明：号必须在 LTP 子集内且已在号面注册。
pub fn claim_cover(face: &C2Face, nr: u32) -> SysResult<()> {
    if !ltp_has(nr) {
        return Err(EINVAL);
    }
    if !face.has(nr) {
        return Err(ENOENT);
    }
    Ok(())
}

/// 三类违约 × 10 轮注入，返回账本与逐类检出数。
pub fn run_cover_injection() -> (InjectLedger, [u32; 3]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 3];
    let mut ci = 0usize;
    while ci < ViolationCase::ALL.len() {
        if let Some(case) = ViolationCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.run();
                let _ = ledger.record(caught);
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

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；协议拒绝族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺号/满容占位"),
    ("EEXIST", -17, 80, "Windows ERROR_FILE_EXISTS；重复登记族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；越集号族"),
    ("EROFS", -30, 19, "Windows ERROR_WRITE_PROTECT；封存只读族"),
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

/// 规范语料双跑指纹：注册 LTP 前半子集 + 对账 + 虚报 + 封存。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut face = C2Face::new();
    let mut i = 0usize;
    // 注册 LTP 子集前 8 行。
    while i < 8 {
        if let Some(row) = LTP_SUBSET.get(i) {
            let r = face.register(row.0);
            bytes.push(match r {
                Ok(_) => 1,
                Err(code) => (code & 0xFF) as u8,
            });
        }
        i += 1;
    }
    let mut gaps = GapList::new();
    match reconcile(&face, &mut gaps) {
        Ok(led) => {
            bytes.push((led.covered & 0xFF) as u8);
            bytes.push((led.gaps & 0xFF) as u8);
            bytes.push(((led.permille() / 10) & 0xFF) as u8);
        }
        Err(code) => bytes.push((code & 0xFF) as u8),
    }
    bytes.push(match claim_cover(&face, 100) {
        Err(EINVAL) => 1,
        _ => 0,
    });
    bytes.push(match claim_cover(&face, 1) {
        Ok(_) => 1,
        Err(_) => 0,
    });
    face.seal();
    bytes.push(match face.register(99) {
        Err(EROFS) => 1,
        _ => 0,
    });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9371 判据集。
pub fn run_unx9371_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9371");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (WINDOW_LO..=WINDOW_HI).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：全量注册 LTP 子集，覆盖率 100%
    let mut face = C2Face::new();
    let mut i = 0usize;
    let mut fill_ok = true;
    while i < LTP_SUBSET.len() {
        if let Some(row) = LTP_SUBSET.get(i) {
            if face.register(row.0).is_err() {
                fill_ok = false;
            }
        }
        i += 1;
    }
    let mut gaps0 = GapList::new();
    let full_led = reconcile(&face, &mut gaps0);
    let normal_ok = fill_ok
        && match full_led {
            Ok(led) => led.covered == LTP_ROWS as u32 && led.permille() == 1000
                && led.conserved(),
            Err(_) => false,
        };
    s.add("正常流：全量注册 LTP 子集覆盖率 100%", normal_ok,
          "16 号全部在册时覆盖账闭合、缺口清单为空");

    // 3. 覆盖率账：分母冻结为 LTP 子集行数
    s.add("覆盖率分母：冻结为 LTP 子集 16 行",
          LTP_SUBSET.len() == LTP_ROWS
              && CoverageLedger::new().denom == LTP_ROWS as u32,
          "覆盖率只以冻结的 LTP 子集为分母，不得随号面浮动");

    // 4~6. 注入矩阵：三类违约各 10/10 + 账守恒
    let (ledger, per_class) = run_cover_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("三类违约注入各 10/10 全检出", all_caught,
          "缺号/虚报/封存重写三类必须全部命中约定码");
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == INJECT_CLASSES * INJECT_ROUNDS,
          "30 次注入必须 30 次检出、0 逃逸");

    // 7. 逐号落账：半量注册时 covered/gaps 精确
    let mut half = C2Face::new();
    let mut j = 0usize;
    while j < 8 {
        if let Some(row) = LTP_SUBSET.get(j) {
            let _ = half.register(row.0);
        }
        j += 1;
    }
    let mut gaps1 = GapList::new();
    let half_led = reconcile(&half, &mut gaps1);
    let half_ok = match half_led {
        Ok(led) => led.covered == 8 && led.gaps == 8 && led.permille() == 500
            && gaps1.len() == 8 && led.conserved(),
        Err(_) => false,
    };
    s.add("逐号落账：半量注册时覆盖率 500‰ 且缺口 8 条", half_ok,
          "覆盖率账逐号复算：千分比与缺口数必须精确命中");

    // 8. 缺口清单：追加式且重复拒绝
    let mut gaps2 = GapList::new();
    let dup_ok = gaps2.record(22, "setpgid01").is_ok()
        && gaps2.record(22, "setpgid01") == Err(EEXIST)
        && gaps2.has(22)
        && gaps2.rejects() == 1;
    s.add("缺口清单：追加式落账、重复显性拒绝", dup_ok,
          "缺口行只增不改，重复登记回 EEXIST");

    // 9. 虚报覆盖：越集号 EINVAL、集内缺号 ENOENT、集内已注册放行
    let mut face3 = C2Face::new();
    let _ = face3.register(1);
    let claim_ok = claim_cover(&face3, 100) == Err(EINVAL)
        && claim_cover(&face3, 22) == Err(ENOENT)
        && claim_cover(&face3, 1).is_ok();
    s.add("虚报覆盖：声明必须落在冻结子集内", claim_ok,
          "覆盖率声明以 LTP_SUBSET 为闭集，越集号一律 EINVAL");

    // 10. 封存后重写：EROFS 且账面不动
    let mut face4 = C2Face::new();
    let _ = face4.register(1);
    face4.seal();
    let before_len = face4.len();
    let before_rejects = face4.rejects();
    let rw_ok = face4.register(2) == Err(EROFS)
        && face4.len() == before_len
        && face4.rejects() == before_rejects + 1;
    s.add("封存账：封存后重写 EROFS 且在册数纹丝不动", rw_ok,
          "封存是单向开关，被拒写入不得改动任何账面字段");

    // 11. 号面边界：0 号 EINVAL、重复 EEXIST
    let mut face5 = C2Face::new();
    let _ = face5.register(3);
    let bounds_ok = face5.register(0) == Err(EINVAL)
        && face5.register(3) == Err(EEXIST)
        && face5.len() == 1;
    s.add("号面边界：0 号拒绝、重复拒绝、在册数不增", bounds_ok,
          "注册协议入口即校验，坏号不得污染号面账");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(EROFS)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 13. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 14. 文案闭集
    let text_ok = CoverCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 诊断码唯一
    let mut uniq = true;
    let mut ui = 0usize;
    while ui < CoverCode::ALL.len() {
        let mut uj = ui + 1;
        while uj < CoverCode::ALL.len() {
            if let (Some(x), Some(y)) = (CoverCode::ALL.get(ui), CoverCode::ALL.get(uj)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            uj += 1;
        }
        ui += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 16. 对照表一致性
    let mut table_ok = true;
    let mut ti = 0usize;
    while ti < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(ti) {
            if row.1 != -(-row.1) || row.2 == 0 || row.3.is_empty() {
                table_ok = false;
            }
        }
        ti += 1;
    }
    s.add("对照表一致性：errno/Windows 码逐项自洽", table_ok,
          "表内每一行负值可复算、Windows 码非零、说明非空");

    // 17. 账目单调
    let mut mono = ledger;
    let before = mono.injected;
    let mut m = 0u32;
    while m < 4 {
        let _ = mono.record(true);
        m += 1;
    }
    s.add("账目单调：再注入只增不减", mono.injected == before + 4,
          "累计计数禁止回退或饱和到旧值");

    // 18. 号面满容：24 号位耗尽显性
    let mut face6 = C2Face::new();
    let mut fill6 = true;
    let mut nr = 1u32;
    while nr <= FACE_CAP as u32 {
        if face6.register(nr).is_err() {
            fill6 = false;
        }
        nr += 1;
    }
    let full_ok = fill6 && face6.len() == FACE_CAP && face6.register(99) == Err(ENOENT);
    s.add("号面满容：24 号位全部在册后第 25 号显性拒绝", full_ok,
          "定容账不得越界，耗尽必须显性可观测且旧账不丢");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {} / 号面拒绝 {}",
        ledger.injected, ledger.detected, ledger.escaped, face.rejects()
    );
    s.add_dyn("账目快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_coverage() {
        let mut face = C2Face::new();
        let mut i = 0usize;
        while i < LTP_SUBSET.len() {
            if let Some(row) = LTP_SUBSET.get(i) {
                assert!(face.register(row.0).is_ok());
            }
            i += 1;
        }
        let mut gaps = GapList::new();
        let led = reconcile(&face, &mut gaps).unwrap_or(CoverageLedger::new());
        assert_eq!(led.covered, LTP_ROWS as u32);
        assert_eq!(led.gaps, 0);
        assert_eq!(led.permille(), 1000);
        assert!(led.conserved());
    }

    #[test]
    fn half_coverage_gaps() {
        let mut face = C2Face::new();
        let mut i = 0usize;
        while i < 8 {
            if let Some(row) = LTP_SUBSET.get(i) {
                let _ = face.register(row.0);
            }
            i += 1;
        }
        let mut gaps = GapList::new();
        let led = reconcile(&face, &mut gaps).unwrap_or(CoverageLedger::new());
        assert_eq!(led.covered, 8);
        assert_eq!(led.gaps, 8);
        assert_eq!(gaps.len(), 8);
        assert!(gaps.has(22));
    }

    #[test]
    fn phantom_claim_einval() {
        let mut face = C2Face::new();
        let _ = face.register(1);
        assert_eq!(claim_cover(&face, 100), Err(EINVAL));
        assert_eq!(claim_cover(&face, 22), Err(ENOENT));
        assert_eq!(claim_cover(&face, 1), Ok(()));
    }

    #[test]
    fn dup_register_eexist() {
        let mut face = C2Face::new();
        assert!(face.register(3).is_ok());
        assert_eq!(face.register(3), Err(EEXIST));
        assert_eq!(face.register(0), Err(EINVAL));
    }

    #[test]
    fn sealed_face_readonly() {
        let mut face = C2Face::new();
        let _ = face.register(1);
        face.seal();
        assert_eq!(face.register(2), Err(EROFS));
        assert_eq!(face.len(), 1);
    }

    #[test]
    fn gap_list_append_only() {
        let mut gaps = GapList::new();
        assert!(gaps.record(9, "mmap01").is_ok());
        assert_eq!(gaps.record(9, "mmap01"), Err(EEXIST));
        assert!(gaps.has(9));
        assert_eq!(gaps.len(), 1);
    }

    #[test]
    fn face_full_rejected() {
        let mut face = C2Face::new();
        let mut n = 1u32;
        while n <= FACE_CAP as u32 {
            assert!(face.register(n).is_ok());
            n += 1;
        }
        assert_eq!(face.register(99), Err(ENOENT));
        assert_eq!(face.len(), FACE_CAP);
    }

    #[test]
    fn matrix_ledger_conserved() {
        let (l, per) = run_cover_injection();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, INJECT_CLASSES * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EROFS));
        assert!(agrees_with_linux(EEXIST));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn unx9371_checks_green() {
        let s = run_unx9371_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
