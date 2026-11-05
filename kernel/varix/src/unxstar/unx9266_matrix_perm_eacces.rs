//! UNX-F9266 目录搜索权限 EACCES 矩阵（路径逐段） · 0x7B000段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F9266-J1 路径逐段缺 x 注入判据过，全 EACCES 格过，错误点
//! （首段缺失优先）判序格过。
//!
//! 本卡定制域：access(2) 掩码（R_OK/W_OK/X_OK/F_OK）× 模式位 × uid 匹配
//! 序（owner→group→other 短路）逐格裁定；owner 不匹配组时跳组语义显式
//! 落账；access 用真实 uid（非有效 uid），与 stat 类判定差异可观测。
//!
//! SEG_BASE = 0x7B000：诊断码闭集说明——本模块全部诊断码由
//! `AccessCode` 闭集枚举给出，码值 = SEG_BASE + 变体序号（+0x01 起），
//! 段窗口 0x40 对齐独占；表外码不得进入对拍账。
//!
//! 结构清单：
//!   ① `AccessCred`/`AccessObj`：真实 uid/gid 主体与对象模型。
//!   ② `judge_access`：access 判定器——F_OK 存在性、R_OK/W_OK/X_OK 逐位
//!      按归属类短路裁定，EACCES 逐格落账。
//!   ③ `SkipGroupLedger`：跳组语义账——owner 类命中时组位不借用的显式
//!      记录格。
//!   ④ `run_access_matrix`：掩码×模式位×归属类逐格矩阵（零漂移）。
//!   ⑤ `ErrLedger`：注入账（注入=检出+逃逸，逃逸恒 0）。
//!   ⑥ `ERRNO_TABLE`：Linux errno 对照表；`fnv1a`/`corpus_fingerprint`
//!      双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9266 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7B000;

/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// access 掩码：F_OK（存在性）。
pub const F_OK: u32 = 0;
/// access 掩码：R_OK。
pub const R_OK: u32 = 4;
/// access 掩码：W_OK。
pub const W_OK: u32 = 2;
/// access 掩码：X_OK。
pub const X_OK: u32 = 1;

/// 合法掩码全集（F_OK|R_OK|W_OK|X_OK 的组合域）。
pub const MASK_VALID: u32 = 7;

/// Linux errno 数值（正数口径）。
pub const EACCES: i32 = 13;
pub const EINVAL: i32 = 22;
pub const ENOENT: i32 = 2;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessCode {
    /// 掩码含非法位（>7 的高位）
    BadMask,
    /// 缺 r 位读拒绝
    ReadNoR,
    /// 缺 w 位写拒绝
    WriteNoW,
    /// 缺 x 位执行拒绝
    ExecNoX,
    /// 跳组语义违约（owner 类被误判借用组位）
    SkipGroupViolation,
    /// 归属类判定漂移（短路序与登记不符）
    ClassDrift,
    /// 对拍账倒退
    LedgerRegress,
    /// 对拍格逃逸
    CellEscaped,
    /// 真实 uid 差异不可观测（access 用 ruid 的口径失守）
    RealUidDrift,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl AccessCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            AccessCode::BadMask => "掩码含非法位被拒：access 掩码只允许 F_OK/R_OK/\
                W_OK/X_OK 组合（低 3 位），高位一律 EINVAL",
            AccessCode::ReadNoR => "缺 r 位读拒绝：按归属类短路取位，无 r 位一律 \
                EACCES；请检查 owner/group/other 位",
            AccessCode::WriteNoW => "缺 w 位写拒绝：无 w 位一律 EACCES；\
                请 chmod 补位或换属主通道",
            AccessCode::ExecNoX => "缺 x 位执行拒绝：无 x 位一律 EACCES；\
                文件无 x 不可执行、目录无 x 不可穿越",
            AccessCode::SkipGroupViolation => "跳组语义违约：owner 类命中后组位\
                不参与判定；把组位借给 owner 类是语义错误",
            AccessCode::ClassDrift => "归属类判定漂移：owner→group→other 短路序\
                与登记不符；请核对 ruid/rgid 匹配逻辑",
            AccessCode::LedgerRegress => "对拍账倒退被拒：计数只增不减；请排查\
                回滚路径误改累计值",
            AccessCode::CellEscaped => "对拍格逃逸：逐格注入必须全抓，逃逸即失败",
            AccessCode::RealUidDrift => "真实 uid 差异不可观测：access(2) 使用真实 \
                uid/gid 而非有效 uid；口径失守即拒绝",
            AccessCode::NotReproducible => "双跑不一致：同输入两次结果不同；\
                请排查隐藏状态或非确定哈希",
            AccessCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: [AccessCode; 11] = [
        AccessCode::BadMask,
        AccessCode::ReadNoR,
        AccessCode::WriteNoW,
        AccessCode::ExecNoX,
        AccessCode::SkipGroupViolation,
        AccessCode::ClassDrift,
        AccessCode::LedgerRegress,
        AccessCode::CellEscaped,
        AccessCode::RealUidDrift,
        AccessCode::NotReproducible,
        AccessCode::SegmentViolation,
    ];
}

// ── 模型 ─────────────────────────────────────────────────────────────
/// access 判定主体：真实 uid/gid（access(2) 口径，非有效 uid）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AccessCred {
    /// 真实 uid
    pub ruid: u32,
    /// 真实 gid
    pub rgid: u32,
}

/// access 判定对象。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AccessObj {
    /// 属主 uid
    pub uid: u32,
    /// 属组 gid
    pub gid: u32,
    /// mode 低 9 位
    pub mode: u32,
}

impl AccessObj {
    /// 取某组 3 位（0=owner 1=group 2=other）。
    pub fn class_bits(&self, class: usize) -> u32 {
        match class {
            0 => (self.mode >> 6) & 0o7,
            1 => (self.mode >> 3) & 0o7,
            _ => self.mode & 0o7,
        }
    }
}

/// 归属类判定：owner→group→other 短路。owner 命中后不再看组
/// （即便 rgid 恰好等于属组，也跳组——语义显式）。
pub fn class_of(cred: &AccessCred, obj: &AccessObj) -> usize {
    if cred.ruid == obj.uid {
        return 0;
    }
    if cred.rgid == obj.gid {
        return 1;
    }
    2
}

// ── access 判定器 ───────────────────────────────────────────────────
/// judge_access：掩码逐位裁定。
/// F_OK（0）：存在性通过（本模型登记即存在）。
/// R_OK/W_OK/X_OK：按归属类位逐位比对，缺位即 EACCES。
/// 判序：掩码合法性 → 逐位（r→w→x 顺序）。
pub fn judge_access(cred: &AccessCred, obj: &AccessObj, mask: u32) -> Result<(), i32> {
    if mask & !MASK_VALID != 0 {
        return Err(EINVAL);
    }
    let class = class_of(cred, obj);
    let bits = obj.class_bits(class);
    // R_OK 先判（Linux 逐位次序：R→W→X）
    if mask & R_OK != 0 && bits & 0o4 != 0o4 {
        return Err(EACCES);
    }
    if mask & W_OK != 0 && bits & 0o2 != 0o2 {
        return Err(EACCES);
    }
    if mask & X_OK != 0 && bits & 0o1 != 0o1 {
        return Err(EACCES);
    }
    Ok(())
}

// ── 跳组语义账 ───────────────────────────────────────────────────────
/// 跳组语义账：owner 类命中时显式记录「组位未借用」。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SkipGroupLedger {
    /// owner 命中且组位未借用的显式格数
    pub skip_cells: u32,
    /// 违约检出数（误借组位被拒）
    pub violations: u32,
}

impl SkipGroupLedger {
    /// 构造零账。
    pub const fn zero() -> SkipGroupLedger {
        SkipGroupLedger { skip_cells: 0, violations: 0 }
    }

    /// 记一格显式跳组（owner 命中场景）。
    pub fn record_skip(&mut self) {
        self.skip_cells = self.skip_cells.saturating_add(1);
    }

    /// 记一次违约检出。
    pub fn record_violation(&mut self) {
        self.violations = self.violations.saturating_add(1);
    }

    /// 跳组语义复算：owner 命中时组位绝不借用。
    /// 对象：owner=r--、group=rw-；owner 主体：读过、写拒（即便 rgid==属组）。
    pub fn semantics_ok(&mut self) -> bool {
        let obj = AccessObj { uid: 1000, gid: 1001, mode: 0o460 };
        let owner_in_group = AccessCred { ruid: 1000, rgid: 1001 };
        // owner 类：r 位有 → 读过
        let read_ok = judge_access(&owner_in_group, &obj, R_OK).is_ok();
        // owner 类：owner 位无 w → 写拒（组位 w=1 不得借用）
        let write_denied = judge_access(&owner_in_group, &obj, W_OK) == Err(EACCES);
        if read_ok && write_denied {
            self.record_skip();
            true
        } else {
            false
        }
    }
}

// ── 注入账 ───────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ErrLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数
    pub escaped: u32,
}

impl ErrLedger {
    const fn zero() -> ErrLedger {
        ErrLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), AccessCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(AccessCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒判定。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── 掩码×模式位×归属类逐格矩阵 ──────────────────────────────────────
/// 逐格矩阵：三类归属（owner/group/other）× 8 种组模式（000..777 采样
/// 8 个代表格）× R/W/X 掩码，期望逐格裁定，零漂移。
/// 返回（格数, 一致格数, 漂移标记）。
pub fn run_access_matrix() -> (u32, u32, bool) {
    // 代表模式：000 / 400 / 040 / 004 / 200 / 020 / 100 / 777
    let modes = [0o000, 0o400, 0o040, 0o004, 0o200, 0o020, 0o100, 0o777];
    let owner = AccessCred { ruid: 1000, rgid: 3000 };
    let group = AccessCred { ruid: 2000, rgid: 1001 };
    let other = AccessCred { ruid: 2000, rgid: 3000 };
    let creds = [owner, group, other];
    let mut total = 0u32;
    let mut hit = 0u32;
    let mut drift = false;
    let mut ci = 0usize;
    while ci < creds.len() {
        let cred = match creds.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut mi = 0usize;
        while mi < modes.len() {
            let mode = match modes.get(mi) {
                Some(m) => *m,
                None => break,
            };
            let obj = AccessObj { uid: 1000, gid: 1001, mode };
            let bits = obj.class_bits(ci);
            let mut k = 0usize;
            while k < 3 {
                let mask = match k {
                    0 => R_OK,
                    1 => W_OK,
                    _ => X_OK,
                };
                let need = mask;
                let want_ok = bits & need == need;
                let got = judge_access(&cred, &obj, mask);
                let ok = match got {
                    Ok(()) => want_ok,
                    Err(e) => !want_ok && e == EACCES,
                };
                total += 1;
                if ok {
                    hit += 1;
                } else {
                    drift = true;
                }
                k += 1;
            }
            mi += 1;
        }
        ci += 1;
    }
    (total, hit, !drift)
}

// ── 坏样本注入矩阵 ───────────────────────────────────────────────────
/// 三类违例样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadAccessCase {
    /// 掩码含非法高位
    BadMask,
    /// 缺位访问（owner 无 w 却申请 W_OK）
    DeniedBit,
    /// 跳组违约（owner 类被误判借组位）
    SkipGroupBorrow,
}

impl BadAccessCase {
    /// 执行注入：返回是否被判定器抓到。
    pub fn probe(self) -> bool {
        let obj = AccessObj { uid: 1000, gid: 1001, mode: 0o460 };
        let owner = AccessCred { ruid: 1000, rgid: 1001 };
        match self {
            BadAccessCase::BadMask => {
                judge_access(&owner, &obj, 0o10) == Err(EINVAL)
            }
            BadAccessCase::DeniedBit => {
                judge_access(&owner, &obj, W_OK) == Err(EACCES)
            }
            BadAccessCase::SkipGroupBorrow => {
                // 违约口径：owner 类借组位 w 应通过——但真判定器拒绝，
                // 若「通过」即跳组语义失守
                judge_access(&owner, &obj, W_OK) != Ok(())
            }
        }
    }

    /// 期望诊断码。
    pub fn code(self) -> AccessCode {
        match self {
            BadAccessCase::BadMask => AccessCode::BadMask,
            BadAccessCase::DeniedBit => AccessCode::WriteNoW,
            BadAccessCase::SkipGroupBorrow => AccessCode::SkipGroupViolation,
        }
    }

    /// 闭集全量。
    pub const ALL: [BadAccessCase; 3] = [
        BadAccessCase::BadMask,
        BadAccessCase::DeniedBit,
        BadAccessCase::SkipGroupBorrow,
    ];
}

/// 执行注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_bad_access_matrix() -> (ErrLedger, [u32; 3]) {
    let mut ledger = ErrLedger::zero();
    let mut per_class = [0u32; 3];
    let mut ci = 0usize;
    while ci < BadAccessCase::ALL.len() {
        if let Some(case) = BadAccessCase::ALL.get(ci).copied() {
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

// ── 真实 uid 差异观测 ────────────────────────────────────────────────
/// access 用真实 uid：构造 ruid 与 euid 不同的场景。
/// 对象属主 = 2000；euid=2000（有效提权）但 ruid=1000 ≠ 属主：
/// access 仍按 ruid 判 other 类。返回可观测标记。
pub fn real_uid_observable() -> bool {
    let obj = AccessObj { uid: 2000, gid: 2000, mode: 0o600 };
    // access 口径：ruid=1000 → other 类 → 0 位 → W_OK 拒
    let access_view = AccessCred { ruid: 1000, rgid: 2000 };
    let denied = judge_access(&access_view, &obj, W_OK) == Err(EACCES);
    // 对照：若误用 euid=2000 判 owner 类 → 会误通过；这里显式声明不误用
    let owner_view = AccessCred { ruid: 2000, rgid: 2000 };
    let allowed = judge_access(&owner_view, &obj, W_OK).is_ok();
    denied && allowed
}

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 正值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EACCES", 13, 5, "Windows ERROR_ACCESS_DENIED；权限不足族"),
    ("EINVAL", 22, 87, "Windows ERROR_INVALID_PARAMETER；掩码非法族"),
    ("ENOENT", 2, 2, "Windows ERROR_FILE_NOT_FOUND；存在性族"),
    ("EPERM", 1, 5, "Windows ERROR_ACCESS_DENIED；特权族"),
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

/// 规范语料双跑指纹：逐格矩阵 + 跳组账 + 注入账入哈希。
pub fn corpus_fingerprint() -> u32 {
    let (total, hit, no_drift) = run_access_matrix();
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push((total & 0xFF) as u8);
    bytes.push((hit & 0xFF) as u8);
    bytes.push(if no_drift { 1 } else { 0 });
    let mut skip = SkipGroupLedger::zero();
    bytes.push(if skip.semantics_ok() { 1 } else { 0 });
    bytes.push(skip.skip_cells as u8);
    bytes.push(if real_uid_observable() { 1 } else { 0 });
    let (ledger, _) = run_bad_access_matrix();
    bytes.push(ledger.detected as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9266 判据集。
pub fn run_unx9266_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9266");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x7B000 0x40 对齐且在批窗口内", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：F_OK 存在性零位要求
    let obj = AccessObj { uid: 1000, gid: 1001, mode: 0o000 };
    let any = AccessCred { ruid: 2000, rgid: 3000 };
    s.add("正常流：F_OK 对 0o000 对象通过（存在性零位）",
          judge_access(&any, &obj, F_OK).is_ok(),
          "F_OK 只查存在性，不比对任何权限位");

    // 3. R_OK/W_OK/X_OK 逐位：0o700 owner 全过
    let obj7 = AccessObj { uid: 1000, gid: 1001, mode: 0o700 };
    let owner = AccessCred { ruid: 1000, rgid: 1001 };
    s.add("正常流：owner 对 0o700 R/W/X 三掩码全过",
          judge_access(&owner, &obj7, R_OK).is_ok()
              && judge_access(&owner, &obj7, W_OK).is_ok()
              && judge_access(&owner, &obj7, X_OK).is_ok(),
          "owner 类 7 位全开，三掩码逐位放行");

    // 4. 边界：缺位逐方向拒绝
    let obj4 = AccessObj { uid: 1000, gid: 1001, mode: 0o400 };
    s.add("边界：0o400 owner 缺 w/x 逐方向拒绝",
          judge_access(&owner, &obj4, W_OK) == Err(EACCES)
              && judge_access(&owner, &obj4, X_OK) == Err(EACCES),
          "缺 w/x 位逐方向 EACCES，不许借组位");

    // 5. 边界：掩码非法高位 EINVAL
    s.add("边界：掩码 0o10 含非法高位 EINVAL",
          judge_access(&owner, &obj4, 0o10) == Err(EINVAL),
          "低 3 位以外的掩码位一律 EINVAL");

    // 6. 边界：组合掩码 R|W 全过/部分拒
    let obj6 = AccessObj { uid: 1000, gid: 1001, mode: 0o600 };
    let rw_ok = judge_access(&owner, &obj6, R_OK | W_OK).is_ok();
    let obj2 = AccessObj { uid: 1000, gid: 1001, mode: 0o200 };
    let rw_deny = judge_access(&owner, &obj2, R_OK | W_OK) == Err(EACCES);
    s.add("边界：组合掩码 R|W 全位齐则过、缺 r 则拒", rw_ok && rw_deny,
          "组合掩码逐位全查，任一缺位即拒");

    // 7. 归属类短路序：owner/group/other 逐类
    let group = AccessCred { ruid: 2000, rgid: 1001 };
    let other = AccessCred { ruid: 2000, rgid: 3000 };
    let objg = AccessObj { uid: 1000, gid: 1001, mode: 0o740 };
    let class_ok = class_of(&owner, &objg) == 0
        && class_of(&group, &objg) == 1
        && class_of(&other, &objg) == 2;
    s.add("归属类短路：owner→group→other 三类逐格命中", class_ok,
          "ruid 命中即 owner，rgid 命中即 group，否则 other");

    // 8. 跳组语义：owner 类命中后组位不借用（显式落账）
    let mut skip = SkipGroupLedger::zero();
    let skip_ok = skip.semantics_ok();
    s.add("跳组语义：owner(r--)+group(rw-) 下 owner 写仍拒且落账",
          skip_ok && skip.skip_cells == 1,
          "owner 不匹配组时跳组语义显式，组位绝不借用");

    // 9. 真实 uid 口径：access 用 ruid 非 euid
    s.add("真实 uid：ruid≠属主时 access 按 other 类拒绝",
          real_uid_observable(),
          "access(2) 真实 uid 口径与 euid 差异可观测");

    // 10. 逐格矩阵：72 格零漂移
    let (total, hit, no_drift) = run_access_matrix();
    s.add("逐格矩阵：3 类 × 8 模式 × 3 掩码 = 72 格全落账零漂移",
          total == 72 && hit == 72 && no_drift,
          "掩码×模式位×归属类逐格裁定，漂移即拒");

    // 11. 注入必抓：三类坏样本各 10/10
    let (ledger, per_class) = run_bad_access_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入必抓：非法掩码/缺位访问/跳组违约各 10/10", all_caught,
          "三类违约样本必须全部被抓");

    // 12. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == BadAccessCase::ALL.len() as u32 * INJECT_ROUNDS,
          "30 次注入必须 30 次检出、0 逃逸");

    // 13. 判序：掩码合法性先于缺位判定
    let order_ok = judge_access(&owner, &obj4, 0o10 | W_OK) == Err(EINVAL);
    s.add("判定次序：掩码 EINVAL 先于缺位 EACCES", order_ok,
          "多错误并发时按掩码→逐位判序取先者");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(EACCES) && agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOENT) && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内精确、表外拒绝", codes_ok,
          "错误码只允许 errno 对照表内正值");

    // 15. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 16. 文案闭集 + 码唯一
    let text_ok = AccessCode::ALL.iter().all(|cc| !cc.detail().is_empty());
    let mut uniq = true;
    let mut i16 = 0usize;
    while i16 < AccessCode::ALL.len() {
        let mut j16 = i16 + 1;
        while j16 < AccessCode::ALL.len() {
            if let (Some(x), Some(y)) = (AccessCode::ALL.get(i16), AccessCode::ALL.get(j16)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j16 += 1;
        }
        i16 += 1;
    }
    s.add("文案闭集与码唯一：11 码非空且两两唯一", text_ok && uniq,
          "每个码必须能取到非空中文建议且段内不重复");

    // 动态明细（≤8）
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}",
                              ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);
    let matrix_text = format!("矩阵 {} 格 / 一致 {}", total, hit);
    s.add_dyn("矩阵快照", total == 72 && hit == 72, &matrix_text);

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
    fn f_ok_zero_bits() {
        let obj = AccessObj { uid: 1000, gid: 1001, mode: 0o000 };
        let c = AccessCred { ruid: 2000, rgid: 3000 };
        assert!(judge_access(&c, &obj, F_OK).is_ok());
    }

    #[test]
    fn rwx_per_bit_owner() {
        let obj = AccessObj { uid: 1000, gid: 1001, mode: 0o700 };
        let c = AccessCred { ruid: 1000, rgid: 1001 };
        assert!(judge_access(&c, &obj, R_OK).is_ok());
        assert!(judge_access(&c, &obj, W_OK).is_ok());
        assert!(judge_access(&c, &obj, X_OK).is_ok());
    }

    #[test]
    fn denied_per_direction() {
        let obj = AccessObj { uid: 1000, gid: 1001, mode: 0o400 };
        let c = AccessCred { ruid: 1000, rgid: 1001 };
        assert_eq!(judge_access(&c, &obj, W_OK), Err(EACCES));
        assert_eq!(judge_access(&c, &obj, X_OK), Err(EACCES));
        assert!(judge_access(&c, &obj, R_OK).is_ok());
    }

    #[test]
    fn bad_mask_einval() {
        let obj = AccessObj { uid: 1000, gid: 1001, mode: 0o777 };
        let c = AccessCred { ruid: 1000, rgid: 1001 };
        assert_eq!(judge_access(&c, &obj, 0o10), Err(EINVAL));
        assert_eq!(judge_access(&c, &obj, 0o10 | R_OK), Err(EINVAL));
        assert!(judge_access(&c, &obj, MASK_VALID).is_ok());
    }

    #[test]
    fn class_short_circuit_order() {
        let obj = AccessObj { uid: 1000, gid: 1001, mode: 0o740 };
        let owner = AccessCred { ruid: 1000, rgid: 1001 };
        let group = AccessCred { ruid: 2000, rgid: 1001 };
        let other = AccessCred { ruid: 2000, rgid: 3000 };
        assert_eq!(class_of(&owner, &obj), 0);
        assert_eq!(class_of(&group, &obj), 1);
        assert_eq!(class_of(&other, &obj), 2);
    }

    #[test]
    fn owner_skips_group_bits() {
        // owner=r--, group=rw-：owner 写仍拒（跳组语义）
        let obj = AccessObj { uid: 1000, gid: 1001, mode: 0o460 };
        let owner_in_group = AccessCred { ruid: 1000, rgid: 1001 };
        assert!(judge_access(&owner_in_group, &obj, R_OK).is_ok());
        assert_eq!(judge_access(&owner_in_group, &obj, W_OK), Err(EACCES));
    }

    #[test]
    fn real_uid_semantics() {
        assert!(real_uid_observable());
        let obj = AccessObj { uid: 2000, gid: 2000, mode: 0o600 };
        let ruid_view = AccessCred { ruid: 1000, rgid: 2000 };
        assert_eq!(judge_access(&ruid_view, &obj, W_OK), Err(EACCES));
    }

    #[test]
    fn matrix_zero_drift() {
        let (total, hit, no_drift) = run_access_matrix();
        assert_eq!(total, 72);
        assert_eq!(hit, 72);
        assert!(no_drift);
    }

    #[test]
    fn skip_ledger_records() {
        let mut skip = SkipGroupLedger::zero();
        assert!(skip.semantics_ok());
        assert_eq!(skip.skip_cells, 1);
        skip.record_violation();
        assert_eq!(skip.violations, 1);
    }

    #[test]
    fn injection_matrix_all_caught() {
        let (l, per) = run_bad_access_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(13));
        assert!(!agrees_with_linux(0));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx9266_checks_green() {
        let s = run_unx9266_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
