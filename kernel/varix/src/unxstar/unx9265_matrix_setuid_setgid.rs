//! UNX-F9265 setuid/setgid 文件位与执行路径矩阵 · 0x7AFC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（plan.json id=9265 逐字摘录）：
//!   ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9265-J1 s 位语义挂点判据过，
//!   目录 setgid 位继承挂点格过，实装归 C1/B1 联签声明格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! SEG_BASE = 0x7AFC0：诊断码闭集说明——本模块全部诊断码由
//! `SetgidCode` 闭集枚举给出，码值 = SEG_BASE + 变体序号（+0x01 起），
//! 段窗口 0x40 对齐独占；表外码不得进入语义账。
//!
//! 结构清单：
//!   ① `ProcCred`/`ExecFile`/`DirNode`：执行主体、可执行文件、目录三模型。
//!   ② `judge_exec`：文件 setuid/setgid 执行提权语义——s 位只在任一 x 位
//!      存在时生效，euid/egid 提权落账；无 x 位 s 位被忽略。
//!   ③ `judge_create_in_dir`：目录 setgid 继承组——新文件 gid 取目录属组
//!      且 s 位继承，逐格落账。
//!   ④ `mand_lock_semantics`：setgid 无执行位时文件锁强语义判定。
//!   ⑤ `SemLedger`：注入账（注入=检出+逃逸，逃逸恒 0）。
//!   ⑥ `ERRNO_TABLE`：Linux errno 对照表；`fnv1a`/`corpus_fingerprint`
//!      双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9265 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7AFC0;

/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 文件 setuid 位。
pub const S_ISUID: u32 = 0o4000;
/// 文件 setgid 位。
pub const S_ISGID: u32 = 0o2000;
/// 九位权限掩码。
pub const PERM_MASK: u32 = 0o777;

/// Linux errno 数值（正数口径）。
pub const EPERM: i32 = 1;
pub const EINVAL: i32 = 22;
pub const ENOENT: i32 = 2;
pub const EACCES: i32 = 13;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SetgidCode {
    /// 无 x 位时 s 位提权被拒（内核忽略 s 位）
    NoElevationWithoutX,
    /// 非 owner 非 root 设置 s 位
    NotOwnerSetbit,
    /// 执行对象无任何 x 位
    ExecNoX,
    /// 执行对象不存在
    UnknownFile,
    /// 目录 setgid 继承缺失（新文件 gid 未取目录属组）
    InheritMissing,
    /// 目录 setgid 位继承漂移（新文件 s 位未随目录落）
    InheritDrift,
    /// 强锁语义误读（setgid 无 x 位未判为强锁候选）
    MandLockMisread,
    /// 有 x 位的 setgid 文件误判强锁
    MandLockFalseHit,
    /// mode 高位非法
    BadMode,
    /// 语义账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl SetgidCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SetgidCode::NoElevationWithoutX => "无 x 位时 s 位提权被拒：内核对无执行\
                位的 s 位文件忽略提权语义；请先补 x 位或移除 s 位",
            SetgidCode::NotOwnerSetbit => "非 owner 非 root 设置 s 位被拒：s 位只能\
                由属主、root 或持 CAP_FOWNER 者设置",
            SetgidCode::ExecNoX => "执行对象无任何 x 位被拒：三组任一 x 都没有时\
                exec 语义不成立",
            SetgidCode::UnknownFile => "执行对象不存在被拒：请先登记文件再走执行\
                语义",
            SetgidCode::InheritMissing => "目录 setgid 继承缺失被拒：setgid 目录下\
                新建文件必须继承目录属组",
            SetgidCode::InheritDrift => "目录 setgid 位继承漂移被拒：setgid 目录下\
                新建文件应带 setgid 位（子目录/文件继承口径）",
            SetgidCode::MandLockMisread => "强锁语义误读被拒：setgid 且三组全无 x \
                位的文件是强制锁候选，必须判定为强锁语义",
            SetgidCode::MandLockFalseHit => "强锁误判被拒：有 x 位的 setgid 文件是\
                组提权语义，不是强锁候选",
            SetgidCode::BadMode => "mode 高位非法被拒：合法 mode 只允许 12 位加\
                类型位域",
            SetgidCode::LedgerRegress => "语义账倒退被拒：计数只增不减；请排查\
                回滚路径误改累计值",
            SetgidCode::NotReproducible => "双跑不一致：同输入两次结果不同；\
                请排查隐藏状态或非确定哈希",
            SetgidCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: [SetgidCode; 12] = [
        SetgidCode::NoElevationWithoutX,
        SetgidCode::NotOwnerSetbit,
        SetgidCode::ExecNoX,
        SetgidCode::UnknownFile,
        SetgidCode::InheritMissing,
        SetgidCode::InheritDrift,
        SetgidCode::MandLockMisread,
        SetgidCode::MandLockFalseHit,
        SetgidCode::BadMode,
        SetgidCode::LedgerRegress,
        SetgidCode::NotReproducible,
        SetgidCode::SegmentViolation,
    ];
}

// ── 模型 ─────────────────────────────────────────────────────────────
/// 执行主体凭证。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProcCred {
    /// 实际 uid
    pub uid: u32,
    /// 有效 uid（提权后变化）
    pub euid: u32,
    /// 实际 gid
    pub gid: u32,
    /// 有效 gid
    pub egid: u32,
}

impl ProcCred {
    /// 构造：初始 euid=uid、egid=gid。
    pub const fn new(uid: u32, gid: u32) -> ProcCred {
        ProcCred { uid, euid: uid, gid, egid: gid }
    }
}

/// 可执行文件：属主 + 属组 + mode。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExecFile {
    /// 属主 uid
    pub uid: u32,
    /// 属组 gid
    pub gid: u32,
    /// mode（12 位口径）
    pub mode: u32,
}

impl ExecFile {
    /// 三组中是否存在任一 x 位。
    pub fn any_x(&self) -> bool {
        self.mode & 0o111 != 0
    }
}

/// 目录节点：属主 + 属组 + mode（参与 setgid 继承）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DirNode {
    /// 属主 uid
    pub uid: u32,
    /// 属组 gid
    pub gid: u32,
    /// mode
    pub mode: u32,
}

// ── 执行提权语义 ─────────────────────────────────────────────────────
/// 执行结果：提权后的主体凭证或错误。
pub type ExecResult = Result<ProcCred, SetgidCode>;

/// judge_exec：文件 setuid/setgid 执行提权语义。
/// 判定序：文件存在 → 任一 x 位（无 x 即拒绝 exec 语义）→ s 位生效。
/// setuid 生效：euid ← file.uid；setgid 生效：egid ← file.gid。
pub fn judge_exec(p: &ProcCred, f: &ExecFile) -> ExecResult {
    if !f.any_x() {
        if f.mode & (S_ISUID | S_ISGID) != 0 {
            // 有 s 位但无 x 位：提权语义被忽略，执行面拒绝
            return Err(SetgidCode::NoElevationWithoutX);
        }
        return Err(SetgidCode::ExecNoX);
    }
    let mut out = ProcCred { uid: p.uid, euid: p.euid, gid: p.gid, egid: p.egid };
    if f.mode & S_ISUID != 0 {
        out.euid = f.uid;
    }
    if f.mode & S_ISGID != 0 {
        out.egid = f.gid;
    }
    Ok(out)
}

/// setbit 校验：设置 s 位的特权检查——owner/root/持 CAP_FOWNER 可设。
pub fn judge_setbit(caller_uid: u32, file_uid: u32, is_root: bool, has_cap_fowner: bool)
    -> Result<(), SetgidCode> {
    if is_root || caller_uid == file_uid || has_cap_fowner {
        Ok(())
    } else {
        Err(SetgidCode::NotOwnerSetbit)
    }
}

// ── 目录 setgid 继承语义 ─────────────────────────────────────────────
/// 新建文件继承结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CreatedFile {
    /// 继承后的属组
    pub gid: u32,
    /// 继承后的 mode
    pub mode: u32,
}

/// judge_create_in_dir：目录 setgid 继承组。
/// setgid 目录下新建文件：gid ← 目录属组；setgid 位继承到新文件。
/// 非 setgid 目录：新文件 gid 取创建者 egid。
pub fn judge_create_in_dir(dir: &DirNode, creator: &ProcCred, want_mode: u32)
    -> Result<CreatedFile, SetgidCode> {
    if want_mode & !0o7777 != 0 {
        return Err(SetgidCode::BadMode);
    }
    if dir.mode & S_ISGID != 0 {
        // 继承：gid 取目录属组，setgid 位落新文件
        Ok(CreatedFile { gid: dir.gid, mode: want_mode | S_ISGID })
    } else {
        Ok(CreatedFile { gid: creator.egid, mode: want_mode })
    }
}

/// 继承对账：判定结果必须与期望一致，违例逐格拒绝。
pub fn reconcile_inherit(dir: &DirNode, creator: &ProcCred, want_mode: u32,
                         expect_gid: u32, expect_sgid: bool) -> bool {
    match judge_create_in_dir(dir, creator, want_mode) {
        Ok(f) => f.gid == expect_gid && (f.mode & S_ISGID != 0) == expect_sgid,
        Err(_) => false,
    }
}

// ── 强锁语义 ─────────────────────────────────────────────────────────
/// setgid 无执行位 → 文件锁强语义（mandatory locking 候选）。
/// 有任一 x 位 → 组提权语义，不是强锁。
pub fn mand_lock_semantics(f: &ExecFile) -> Result<bool, SetgidCode> {
    if f.mode & !0o7777 != 0 {
        return Err(SetgidCode::BadMode);
    }
    if f.mode & S_ISGID != 0 && !f.any_x() {
        Ok(true)
    } else if f.mode & S_ISGID != 0 && f.any_x() {
        Err(SetgidCode::MandLockFalseHit)
    } else {
        Ok(false)
    }
}

// ── 语义账 ───────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SemLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数
    pub escaped: u32,
    /// 提权成功笔数（exec 语义账）
    pub elevations: u32,
    /// 继承成功笔数（目录 setgid 账）
    pub inherits: u32,
}

impl SemLedger {
    const fn zero() -> SemLedger {
        SemLedger { injected: 0, detected: 0, escaped: 0, elevations: 0, inherits: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), SetgidCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(SetgidCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒判定。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── 坏样本注入矩阵 ───────────────────────────────────────────────────
/// 四类违例样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadSemCase {
    /// setuid 文件无 x 位却要求提权
    ElevateWithoutX,
    /// 非 owner 非 root 设置 s 位
    SetbitNotOwner,
    /// setgid 目录继承违约（期望 gid 不符）
    InheritViolated,
    /// 强锁语义误判（有 x 位误判强锁）
    MandLockFalseHit,
}

impl BadSemCase {
    /// 执行注入：返回是否被语义判定器抓到。
    pub fn probe(self) -> bool {
        let p = ProcCred::new(2000, 3000);
        match self {
            BadSemCase::ElevateWithoutX => {
                let f = ExecFile { uid: 0, gid: 1001, mode: S_ISUID | 0o644 };
                judge_exec(&p, &f) == Err(SetgidCode::NoElevationWithoutX)
            }
            BadSemCase::SetbitNotOwner => {
                judge_setbit(2000, 1000, false, false) == Err(SetgidCode::NotOwnerSetbit)
            }
            BadSemCase::InheritViolated => {
                // setgid 目录属组 1001：创建后 gid 必须是 1001；
                // 违约构造为期望 3000 → 对账器必须判不一致
                let dir = DirNode { uid: 0, gid: 1001, mode: 0o2755 };
                !reconcile_inherit(&dir, &p, 0o644, 3000, true)
            }
            BadSemCase::MandLockFalseHit => {
                let f = ExecFile { uid: 0, gid: 1001, mode: S_ISGID | 0o755 };
                mand_lock_semantics(&f) == Err(SetgidCode::MandLockFalseHit)
            }
        }
    }

    /// 闭集全量。
    pub const ALL: [BadSemCase; 4] = [
        BadSemCase::ElevateWithoutX,
        BadSemCase::SetbitNotOwner,
        BadSemCase::InheritViolated,
        BadSemCase::MandLockFalseHit,
    ];
}

/// 执行注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_bad_sem_matrix() -> (SemLedger, [u32; 4]) {
    let mut ledger = SemLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadSemCase::ALL.len() {
        if let Some(case) = BadSemCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.probe();
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

// ── s 位语义矩阵（逐格账） ───────────────────────────────────────────
/// s 位 × x 位 × 文件/目录 逐格矩阵：
///   文件：suid×x、sgid×x（提权语义）与 sgid×无x（强锁语义）
///   目录：setgid 继承
/// 返回（提权账, 继承账, 强锁账, 语义漂移标记）。
pub fn run_sem_matrix() -> (u32, u32, u32, bool) {
    let p = ProcCred::new(2000, 3000);
    let mut elev = 0u32;
    let mut inherit = 0u32;
    let mut mand = 0u32;
    let mut drift = false;
    // 文件格：suid=1/0 × x=1/0
    let mut su = 0usize;
    while su < 2 {
        let mut x = 0usize;
        while x < 2 {
            let mode = S_IF_NONE | (if su == 1 { S_ISUID } else { 0 })
                | (if x == 1 { 0o755 } else { 0o644 });
            let f = ExecFile { uid: 0, gid: 1001, mode };
            let want_elev = su == 1 && x == 1;
            match judge_exec(&p, &f) {
                Ok(out) => {
                    if out.euid == 0 && want_elev {
                        elev += 1;
                    } else if !want_elev {
                        elev += 1;
                    } else {
                        drift = true;
                    }
                }
                Err(SetgidCode::NoElevationWithoutX) => {
                    if !want_elev {
                        elev += 1;
                    } else {
                        drift = true;
                    }
                }
                Err(_) => {
                    if !want_elev {
                        elev += 1;
                    } else {
                        drift = true;
                    }
                }
            }
            x += 1;
        }
        su += 1;
    }
    // 目录格：setgid=1/0
    let mut sg = 0usize;
    while sg < 2 {
        let dir = DirNode {
            uid: 0,
            gid: 1001,
            mode: 0o755 | (if sg == 1 { S_ISGID } else { 0 }),
        };
        let want_gid = if sg == 1 { 1001 } else { 3000 };
        if reconcile_inherit(&dir, &p, 0o644, want_gid, sg == 1) {
            inherit += 1;
        } else {
            drift = true;
        }
        sg += 1;
    }
    // 强锁格：sgid × x
    let cases = [
        (S_ISGID | 0o644, true),
        (S_ISGID | 0o755, false),
        (0o644, false),
        (0o755, false),
    ];
    let mut i = 0usize;
    while i < cases.len() {
        if let Some(row) = cases.get(i) {
            let f = ExecFile { uid: 0, gid: 1001, mode: row.0 };
            match mand_lock_semantics(&f) {
                Ok(v) => {
                    if v == row.1 {
                        mand += 1;
                    } else {
                        drift = true;
                    }
                }
                Err(SetgidCode::MandLockFalseHit) => {
                    if !row.1 {
                        mand += 1;
                    } else {
                        drift = true;
                    }
                }
                Err(_) => drift = true,
            }
        }
        i += 1;
    }
    (elev, inherit, mand, !drift)
}

/// 占位常量（文件无类型位的口径：0）。
pub const S_IF_NONE: u32 = 0;

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 正值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", 1, 5, "Windows ERROR_ACCESS_DENIED；s 位设置特权族"),
    ("EACCES", 13, 5, "Windows ERROR_ACCESS_DENIED；执行位缺失族"),
    ("ENOENT", 2, 2, "Windows ERROR_FILE_NOT_FOUND；文件缺失族"),
    ("EINVAL", 22, 87, "Windows ERROR_INVALID_PARAMETER；mode 非法族"),
];

/// 查表复算：正值必须在表内。
pub fn agrees_with_linux(code: i32) -> bool {
    let mut hit = false;
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                hit = true;
            }
        }
        i += 1;
    }
    hit && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
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

/// 规范语料双跑指纹：语义矩阵四账 + 注入账入哈希。
pub fn corpus_fingerprint() -> u32 {
    let (elev, inherit, mand, no_drift) = run_sem_matrix();
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push(elev as u8);
    bytes.push(inherit as u8);
    bytes.push(mand as u8);
    bytes.push(if no_drift { 1 } else { 0 });
    let (ledger, _) = run_bad_sem_matrix();
    bytes.push(ledger.detected as u8);
    // 提权语料：setuid+x 文件执行后 euid=属主
    let p = ProcCred::new(2000, 3000);
    let f = ExecFile { uid: 0, gid: 1001, mode: S_ISUID | 0o755 };
    match judge_exec(&p, &f) {
        Ok(out) => {
            bytes.push((out.euid & 0xFF) as u8);
            bytes.push((out.egid & 0xFF) as u8);
        }
        Err(_) => bytes.push(0),
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9265 判据集。
pub fn run_unx9265_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9265");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x7AFC0 0x40 对齐且在批窗口内", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：setuid+x 文件执行后 euid 提到属主
    let p = ProcCred::new(2000, 3000);
    let f = ExecFile { uid: 0, gid: 1001, mode: S_ISUID | 0o755 };
    let elev = judge_exec(&p, &f);
    s.add("正常流：setuid+x 执行后 euid=0（属主）",
          match elev {
              Ok(out) => out.euid == 0 && out.uid == 2000,
              Err(_) => false,
          },
          "s 位提权语义挂点：euid 换属主、实际 uid 不动");

    // 3. 正常流：setgid+x 文件执行后 egid 提到属组
    let fg = ExecFile { uid: 0, gid: 1001, mode: S_ISGID | 0o755 };
    s.add("正常流：setgid+x 执行后 egid=1001（属组）",
          match judge_exec(&p, &fg) {
              Ok(out) => out.egid == 1001 && out.gid == 3000,
              Err(_) => false,
          },
          "g 位提权语义挂点：egid 换属组");

    // 4. s 位无 x 位：提权语义被忽略
    let fnx = ExecFile { uid: 0, gid: 1001, mode: S_ISUID | 0o644 };
    s.add("注入：setuid 无 x 位执行面拒绝（提权不生效）",
          judge_exec(&p, &fnx) == Err(SetgidCode::NoElevationWithoutX),
          "内核对无 x 位 s 位文件忽略提权，执行语义不成立");

    // 5. 全无 s 位无 x 位：纯执行拒绝
    let fplain = ExecFile { uid: 0, gid: 1001, mode: 0o644 };
    s.add("注入：无 s 无 x 执行拒绝（ExecNoX）",
          judge_exec(&p, &fplain) == Err(SetgidCode::ExecNoX),
          "任一 x 都没有时 exec 语义不成立");

    // 6. s 位设置特权：owner/root/CAP_FOWNER 三通道
    let set_ok = judge_setbit(1000, 1000, false, false).is_ok()
        && judge_setbit(0, 1000, true, false).is_ok()
        && judge_setbit(2000, 1000, false, true).is_ok()
        && judge_setbit(2000, 1000, false, false) == Err(SetgidCode::NotOwnerSetbit);
    s.add("s 位特权：owner/root/CAP_FOWNER 可设、其余全拒", set_ok,
          "s 位设置三通道正判据 + 非属主拒绝格");

    // 7. 目录 setgid 继承组：gid 取目录属组且 s 位落新文件
    let sdir = DirNode { uid: 0, gid: 1001, mode: 0o2755 };
    s.add("目录 setgid：新文件 gid=1001 且带 setgid 位",
          reconcile_inherit(&sdir, &p, 0o644, 1001, true),
          "继承挂点：属组与 s 位双落账");

    // 8. 非 setgid 目录：新文件 gid 取创建者 egid 且不带 s 位
    let pdir = DirNode { uid: 0, gid: 1001, mode: 0o755 };
    s.add("普通目录：新文件 gid=创建者 egid=3000 无 s 位",
          reconcile_inherit(&pdir, &p, 0o644, 3000, false),
          "无 setgid 时继承面关闭");

    // 9. 强锁语义：setgid 且全无 x 位 → 强锁候选
    let fmand = ExecFile { uid: 0, gid: 1001, mode: S_ISGID | 0o644 };
    s.add("强锁语义：setgid 无 x 位判为强锁候选",
          mand_lock_semantics(&fmand) == Ok(true),
          "g 位无 x 时文件锁强语义成立");

    // 10. 强锁误判拒绝：setgid 有 x 位是组提权不是强锁
    let fg2 = ExecFile { uid: 0, gid: 1001, mode: S_ISGID | 0o755 };
    s.add("强锁误判：setgid 有 x 位拒绝强锁判定",
          mand_lock_semantics(&fg2) == Err(SetgidCode::MandLockFalseHit),
          "有 x 位走组提权语义，强锁面关闭");

    // 11. 语义矩阵：逐格账零漂移
    let (elev_n, inherit_n, mand_n, no_drift) = run_sem_matrix();
    s.add("语义矩阵：提权 4 格 + 继承 2 格 + 强锁 4 格全落账零漂移",
          elev_n == 4 && inherit_n == 2 && mand_n == 4 && no_drift,
          "s位×x位×文件/目录逐格裁定，漂移即拒");

    // 12. 注入必抓：四类坏样本各 10/10
    let (ledger, per_class) = run_bad_sem_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入必抓：无x提权/非属主设位/继承违约/强锁误判各 10/10", all_caught,
          "三类注入拒绝面逐类全抓");

    // 13. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == BadSemCase::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM) && agrees_with_linux(EACCES)
        && agrees_with_linux(ENOENT) && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内精确、表外拒绝", codes_ok,
          "错误码只允许 errno 对照表内正值");

    // 15. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 16. 文案闭集 + 码唯一
    let text_ok = SetgidCode::ALL.iter().all(|cc| !cc.detail().is_empty());
    let mut uniq = true;
    let mut i16 = 0usize;
    while i16 < SetgidCode::ALL.len() {
        let mut j16 = i16 + 1;
        while j16 < SetgidCode::ALL.len() {
            if let (Some(x), Some(y)) = (SetgidCode::ALL.get(i16), SetgidCode::ALL.get(j16)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j16 += 1;
        }
        i16 += 1;
    }
    s.add("文案闭集与码唯一：12 码非空且两两唯一", text_ok && uniq,
          "每个码必须能取到非空中文建议且段内不重复");

    // 动态明细（≤8）
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}",
                              ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);
    let sem_text = format!("提权 {} / 继承 {} / 强锁 {}", elev_n, inherit_n, mand_n);
    s.add_dyn("语义快照", elev_n == 4 && inherit_n == 2 && mand_n == 4, &sem_text);

    // 17. 账目单调
    let mut mono = ledger;
    let before = mono.injected;
    let mut k = 0u32;
    while k < 4 {
        let _ = mono.record(true);
        k += 1;
    }
    s.add("账目单调：再注入 4 次只增不减", mono.injected == before + 4,
          "累计计数禁止回退或饱和到旧值");

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setuid_exec_elevates_euid() {
        let p = ProcCred::new(2000, 3000);
        let f = ExecFile { uid: 0, gid: 1001, mode: S_ISUID | 0o755 };
        let out = judge_exec(&p, &f).unwrap_or(ProcCred::new(0, 0));
        assert_eq!(out.euid, 0);
        assert_eq!(out.uid, 2000);
    }

    #[test]
    fn setgid_exec_elevates_egid() {
        let p = ProcCred::new(2000, 3000);
        let f = ExecFile { uid: 0, gid: 1001, mode: S_ISGID | 0o755 };
        let out = judge_exec(&p, &f).unwrap_or(ProcCred::new(0, 0));
        assert_eq!(out.egid, 1001);
    }

    #[test]
    fn suid_without_x_ignored() {
        let p = ProcCred::new(2000, 3000);
        let f = ExecFile { uid: 0, gid: 1001, mode: S_ISUID | 0o644 };
        assert_eq!(judge_exec(&p, &f), Err(SetgidCode::NoElevationWithoutX));
    }

    #[test]
    fn plain_no_x_exec_rejected() {
        let p = ProcCred::new(2000, 3000);
        let f = ExecFile { uid: 0, gid: 1001, mode: 0o644 };
        assert_eq!(judge_exec(&p, &f), Err(SetgidCode::ExecNoX));
    }

    #[test]
    fn setbit_permission_channels() {
        assert!(judge_setbit(1000, 1000, false, false).is_ok());
        assert!(judge_setbit(0, 1000, true, false).is_ok());
        assert!(judge_setbit(2000, 1000, false, true).is_ok());
        assert_eq!(judge_setbit(2000, 1000, false, false),
                   Err(SetgidCode::NotOwnerSetbit));
    }

    #[test]
    fn dir_setgid_inherits() {
        let p = ProcCred::new(2000, 3000);
        let dir = DirNode { uid: 0, gid: 1001, mode: 0o2755 };
        let f = judge_create_in_dir(&dir, &p, 0o644).unwrap_or(CreatedFile { gid: 0, mode: 0 });
        assert_eq!(f.gid, 1001);
        assert_eq!(f.mode & S_ISGID, S_ISGID);
    }

    #[test]
    fn plain_dir_no_inherit() {
        let p = ProcCred::new(2000, 3000);
        let dir = DirNode { uid: 0, gid: 1001, mode: 0o755 };
        let f = judge_create_in_dir(&dir, &p, 0o644).unwrap_or(CreatedFile { gid: 0, mode: 0 });
        assert_eq!(f.gid, 3000);
        assert_eq!(f.mode & S_ISGID, 0);
    }

    #[test]
    fn mandatory_lock_semantics() {
        let f1 = ExecFile { uid: 0, gid: 1001, mode: S_ISGID | 0o644 };
        assert_eq!(mand_lock_semantics(&f1), Ok(true));
        let f2 = ExecFile { uid: 0, gid: 1001, mode: S_ISGID | 0o755 };
        assert_eq!(mand_lock_semantics(&f2), Err(SetgidCode::MandLockFalseHit));
        let f3 = ExecFile { uid: 0, gid: 1001, mode: 0o644 };
        assert_eq!(mand_lock_semantics(&f3), Ok(false));
    }

    #[test]
    fn sem_matrix_no_drift() {
        let (elev, inherit, mand, no_drift) = run_sem_matrix();
        assert_eq!(elev, 4);
        assert_eq!(inherit, 2);
        assert_eq!(mand, 4);
        assert!(no_drift);
    }

    #[test]
    fn injection_matrix_all_caught() {
        let (l, per) = run_bad_sem_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(1));
        assert!(!agrees_with_linux(0));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx9265_checks_green() {
        let s = run_unx9265_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
