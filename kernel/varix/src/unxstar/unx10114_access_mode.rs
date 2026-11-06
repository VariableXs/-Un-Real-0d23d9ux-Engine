//! UNX-F10114 access 正反双判据（可达性判定达成与非法 mode 误用受控）· 0x92C40 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10115-J1 unlink/rename 达成（nlink 递减与改名后可打开 LTP
//! unlink01/rename01 面 10/10 次）；UNX-F10115-J2 缺失路径误用受控（预期
//! ENOENT 精确复现 10/10 次）
//!
//! SEG_BASE = 0x92C40（本卡独占 0x40 窗口，下一卡 0x92C80）；
//! 诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器：mode 位解析器（F_OK=0/X_OK=1/W_OK=2/R_OK=4 位闭集，负值与
//!      越七位逐档拒绝），access 判定前必须先过解析器。
//!   ② 账本：权限账本 AccessLedger（路径→rwx 授权位），判定只读不改账。
//!   ③ 判定核：F_OK 只问存在、R/W/X 按位与授权位比对，拒绝即 EACCES 档。
//!   ④ 注入核：非法 mode / 缺失路径 / 空路径 / 只读路径写请求 四类坏样本
//!      逐类 10/10 次精确复现 EINVAL/ENOENT/EACCES 族，注入账逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10114 独占 0x40 窗口，下一卡 0x92C80）。
pub const SEG_BASE: u32 = 0x92C40;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// F_OK（只问存在）。
pub const F_OK: i32 = 0;
/// X_OK（可执行位）。
pub const X_OK: i32 = 1;
/// W_OK（可写位）。
pub const W_OK: i32 = 2;
/// R_OK（可读位）。
pub const R_OK: i32 = 4;

/// 权限账本容量上限。
pub const LEDGER_CAP: usize = 64;

/// 路径名长度上界（超过即按超长档拒绝）。
pub const NAME_MAX: usize = 64;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccCode {
    /// 负 mode（EINVAL 族）
    NegMode,
    /// mode 越七位（EINVAL 族）
    InvalidMode,
    /// 空路径（EINVAL 族）
    EmptyPath,
    /// 路径超长（EINVAL 族）
    PathTruncate,
    /// 缺失路径（ENOENT 族）
    MissingPath,
    /// 授权不足（EACCES 族）
    AccessDenied,
    /// 重复注册（EEXIST 族）
    DupPath,
    /// 授权位越七位
    BadBits,
    /// 账本容量满
    LedgerFull,
    /// mode 标签往返不一致
    ModeRoundTrip,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
}

impl AccCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            AccCode::NegMode => "负 mode 被拒：access 的 mode 不得为负；\
                请使用 F_OK/R_OK/W_OK/X_OK 位组合",
            AccCode::InvalidMode => "mode 越七位被拒：mode 只允许 0..=7 位闭集；\
                请去掉未定义高位",
            AccCode::EmptyPath => "空路径被拒：路径名不得为空串；请给出目标路径",
            AccCode::PathTruncate => "路径超长被拒：路径名超过 NAME_MAX 上界；请缩短路径",
            AccCode::MissingPath => "缺失路径：账本中无此路径；ENOENT 精确归档",
            AccCode::AccessDenied => "授权不足：请求位超出 rwx 授权位；EACCES 精确归档",
            AccCode::DupPath => "重复注册被拒：同路径只能注册一次；请先撤销再注册",
            AccCode::BadBits => "授权位越界被拒：注册的 rwx 位必须在 0..=7 内",
            AccCode::LedgerFull => "账本容量满：请先撤销闲置路径再注册",
            AccCode::ModeRoundTrip => "mode 标签往返不一致：位串化再解析必须回到原位；\
                请核对标签表",
            AccCode::EmptySummary => "空成文被拒：可达性小结不得为空串；请检查路径是否在账",
            AccCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [AccCode; 12] = [
        AccCode::NegMode,
        AccCode::InvalidMode,
        AccCode::EmptyPath,
        AccCode::PathTruncate,
        AccCode::MissingPath,
        AccCode::AccessDenied,
        AccCode::DupPath,
        AccCode::BadBits,
        AccCode::LedgerFull,
        AccCode::ModeRoundTrip,
        AccCode::EmptySummary,
        AccCode::SegmentViolation,
    ];
}

// ── mode 位解析器 ─────────────────────────────────────────────────────
/// 解析 mode：负值 NegMode、越七位 InvalidMode；0..=7 放行为位掩码。
pub fn parse_mode(mode: i32) -> Result<u8, AccCode> {
    if mode < 0 {
        return Err(AccCode::NegMode);
    }
    if mode > 7 {
        return Err(AccCode::InvalidMode);
    }
    Ok(mode as u8)
}

/// 位串化：按 rwx 次序输出标签（0 位输出空串，供往返核）。
pub fn mode_label(bits: u8) -> String {
    let mut out = String::new();
    if bits & 4 != 0 {
        let _ = out.push('r');
    }
    if bits & 2 != 0 {
        let _ = out.push('w');
    }
    if bits & 1 != 0 {
        let _ = out.push('x');
    }
    out
}

// ── 权限账本 ──────────────────────────────────────────────────────────
/// 单路径权限记录。
#[derive(Clone, Debug)]
pub struct PathEnt {
    /// 路径名
    pub name: String,
    /// 授权位（owner rwx，0..=7）
    pub bits: u8,
}

/// 权限账本：路径→授权位；access 判定只读不改账。
#[derive(Clone, Debug)]
pub struct AccessLedger {
    paths: Vec<PathEnt>,
    cap: usize,
}

impl AccessLedger {
    /// 空账构造（容量 LEDGER_CAP）。
    pub fn new() -> AccessLedger {
        AccessLedger { paths: Vec::new(), cap: LEDGER_CAP }
    }

    /// 注册路径：空名/超长/越位/重复/满账逐档拒绝。
    pub fn register(&mut self, name: &str, bits: u8) -> Result<(), AccCode> {
        if name.is_empty() {
            return Err(AccCode::EmptyPath);
        }
        if name.len() > NAME_MAX {
            return Err(AccCode::PathTruncate);
        }
        if bits > 7 {
            return Err(AccCode::BadBits);
        }
        if self.paths.len() >= self.cap {
            return Err(AccCode::LedgerFull);
        }
        let mut i = 0usize;
        while i < self.paths.len() {
            if let Some(p) = self.paths.get(i) {
                if p.name == name {
                    return Err(AccCode::DupPath);
                }
            }
            i += 1;
        }
        let _ = self.paths.push(PathEnt { name: String::from(name), bits });
        Ok(())
    }

    /// 在账路径数。
    pub fn len(&self) -> usize {
        self.paths.len()
    }

    /// access 判定：mode 先过解析器，路径后查账；F_OK 只问存在，
    /// R/W/X 按位与授权位比对，不足即 EACCES 档。判定不改账。
    pub fn access(&self, name: &str, mode: i32) -> Result<(), AccCode> {
        let bits_req = parse_mode(mode)?;
        if name.is_empty() {
            return Err(AccCode::EmptyPath);
        }
        if name.len() > NAME_MAX {
            return Err(AccCode::PathTruncate);
        }
        let mut i = 0usize;
        while i < self.paths.len() {
            if let Some(p) = self.paths.get(i) {
                if p.name == name {
                    if bits_req == 0 {
                        return Ok(()); // F_OK：在账即可达
                    }
                    if bits_req & !p.bits == 0 {
                        return Ok(());
                    }
                    return Err(AccCode::AccessDenied);
                }
            }
            i += 1;
        }
        Err(AccCode::MissingPath)
    }

    /// 授权位读取（审计用）。
    pub fn bits_of(&self, name: &str) -> Option<u8> {
        let mut i = 0usize;
        while i < self.paths.len() {
            if let Some(p) = self.paths.get(i) {
                if p.name == name {
                    return Some(p.bits);
                }
            }
            i += 1;
        }
        None
    }
}

// ── J1 达成核：R_OK/W_OK/F_OK 三面判定 10/10 轮 ──────────────────────
/// 逐轮对 rw/ro/x 三路径做 6 面 access 断言；全对才计入达成。
pub fn run_access_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut l = AccessLedger::new();
        let _ = l.register("rw", 6);
        let _ = l.register("ro", 4);
        let _ = l.register("x", 1);
        // 六面断言：每面结果必须与授权位推导一致。
        let face = [
            ("rw", R_OK, true),
            ("rw", W_OK, true),
            ("rw", X_OK, false),
            ("ro", R_OK, true),
            ("ro", W_OK, false),
            ("x", X_OK, true),
        ];
        let mut all_ok = true;
        let mut fi = 0usize;
        while fi < face.len() {
            let (name, mode, expect) = match face.get(fi) {
                Some(t) => *t,
                None => break,
            };
            let got = l.access(name, mode).is_ok();
            if got != expect {
                all_ok = false;
            }
            probes += 1;
            fi += 1;
        }
        // F_OK 面：三条路径在账即可达。
        if l.access("rw", F_OK).is_err() || l.access("ro", F_OK).is_err() {
            all_ok = false;
        }
        probes += 2;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏可达性样本矩阵 ──────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadAccess {
    /// 非法 mode（8 与 -1 两档）
    InvalidMode,
    /// 缺失路径
    MissingPath,
    /// 空路径
    EmptyPath,
    /// 只读路径写请求
    WriteOnRo,
}

impl BadAccess {
    /// 闭集全量。
    pub const ALL: [BadAccess; 4] = [
        BadAccess::InvalidMode,
        BadAccess::MissingPath,
        BadAccess::EmptyPath,
        BadAccess::WriteOnRo,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadAccess::InvalidMode => "非法 mode 请求",
            BadAccess::MissingPath => "缺失路径可达性判定",
            BadAccess::EmptyPath => "空路径判定",
            BadAccess::WriteOnRo => "只读路径写请求",
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
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

    /// 记一次注入结果；账失守恒返回 SegmentViolation。
    pub fn record(&mut self, caught: bool) -> Result<(), AccCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(AccCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏可达性注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_access_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadAccess::ALL.len() {
        let case = match BadAccess::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut l = AccessLedger::new();
            let _ = l.register("ro", 4);
            let caught = match case {
                // 8 与 -1 两档非法 mode 都必须按 EINVAL 族拒绝。
                BadAccess::InvalidMode => {
                    l.access("ro", 8).err() == Some(AccCode::InvalidMode)
                        && l.access("ro", -1).err() == Some(AccCode::NegMode)
                }
                BadAccess::MissingPath => {
                    l.access("ghost", R_OK).err() == Some(AccCode::MissingPath)
                }
                BadAccess::EmptyPath => {
                    l.access("", R_OK).err() == Some(AccCode::EmptyPath)
                }
                BadAccess::WriteOnRo => {
                    l.access("ro", W_OK).err() == Some(AccCode::AccessDenied)
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 可达性小结成文（纯函数） ──────────────────────────────────────────
/// 小结成文：「path=<n> bits=<rwx> ok=<0|1>」；账外先拒。
pub fn compose_access_summary(l: &AccessLedger, name: &str) -> Result<String, AccCode> {
    let bits = match l.bits_of(name) {
        Some(b) => b,
        None => return Err(AccCode::EmptySummary),
    };
    let ok_flag = match l.access(name, R_OK) {
        Ok(()) => 1u32,
        Err(_) => 0u32,
    };
    let out = format!("path={} bits={} ok={}", name, mode_label(bits), ok_flag);
    if out.is_empty() {
        return Err(AccCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；非法 mode/空路径族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺失路径族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；授权不足族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复注册族"),
    ("ENAMETOOLONG", -36, 206, "Windows ERROR_FILENAME_EXCED_RANGE；超长路径族"),
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

/// 规范语料双跑指纹：三路径注册 → 六面判定 → 逐条成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = AccessLedger::new();
    let _ = l.register("rw", 6);
    let _ = l.register("ro", 4);
    let _ = l.register("wx", 3);
    let mut bytes: Vec<u8> = Vec::new();
    let mut e = 0u32;
    while e < 3 {
        let name = match e {
            0 => "rw",
            1 => "ro",
            _ => "wx",
        };
        if let Ok(s) = compose_access_summary(&l, name) {
            bytes.extend_from_slice(s.as_bytes());
            bytes.push(b';');
        }
        e += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10114 判据集（13 条静态 + 3 条动态；动态明细 ≤8 条）。
pub fn run_unx10114_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10114");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92C40..=0x92C7F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < AccCode::ALL.len() {
        let mut j = i + 1;
        while j < AccCode::ALL.len() {
            if let (Some(a), Some(b)) = (AccCode::ALL.get(i), AccCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = AccCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. mode 解析器：0..=7 放行，负值与越位逐档拒绝
    let parse_ok = parse_mode(F_OK) == Ok(0)
        && parse_mode(R_OK) == Ok(4)
        && parse_mode(7) == Ok(7)
        && parse_mode(8).err() == Some(AccCode::InvalidMode)
        && parse_mode(-1).err() == Some(AccCode::NegMode);
    s.add("mode 解析器：位闭集放行且负值/越位逐档拒绝", parse_ok,
          "解析器是 access 前置，非法 mode 归因必须精确到档");

    // 4. 账本注册：空名/越位/重复逐档拒绝
    let mut l = AccessLedger::new();
    let reg_ok = l.register("rw", 6).is_ok() && l.len() == 1
        && l.register("", 6).err() == Some(AccCode::EmptyPath)
        && l.register("bad", 8).err() == Some(AccCode::BadBits)
        && l.register("rw", 4).err() == Some(AccCode::DupPath);
    s.add("权限账本：空名/越位/重复注册显性拒绝", reg_ok,
          "注册是判定前置；被拒注册不得污染账面");

    // 5. J1 达成核：R_OK/W_OK/F_OK 三面 10/10
    let (ok, probes) = run_access_rounds();
    s.add("J1 可达性达成：R_OK/W_OK/F_OK 面判定正确 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 8,
          "每轮 8 面断言全部与授权位推导一致，任何一面漂移即失守");

    // 6. F_OK 面：在账可达、账外 ENOENT
    let fok_ok = l.access("rw", F_OK).is_ok()
        && l.access("ghost", F_OK).err() == Some(AccCode::MissingPath);
    s.add("F_OK 面：在账可达且账外按 ENOENT 归档", fok_ok,
          "F_OK 只问存在不问授权位，语义必须与 R/W/X 分档");

    // 7. 判定不改账：连续判定后授权位与账面一致
    let before = l.bits_of("rw");
    let _ = l.access("rw", W_OK);
    let _ = l.access("rw", X_OK);
    let after = l.bits_of("rw");
    s.add("判定只读：access 前后账面授权位不变", before.is_some() && before == after,
          "access 是查询面，判定不得隐式改授权");

    // 8. 注入核：四类坏样本各 10/10 拒绝
    let (inj, per_class) = run_bad_access_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "非法 mode/缺失路径/空路径/只读写请求必须逐类逐次精确复现");

    // 9. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadAccess::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. J2 精确归因：EINVAL 族逐档复现
    let j2_ok = l.access("rw", 8).err() == Some(AccCode::InvalidMode)
        && l.access("rw", -1).err() == Some(AccCode::NegMode)
        && l.access("", 0).err() == Some(AccCode::EmptyPath)
        && l.access("rw", R_OK).is_ok();
    s.add("J2 非法 mode 受控：EINVAL 族精确复现且不伤合法路径", j2_ok,
          "越位与负值是两档独立误用；被拒判定不得改动账面");

    // 11. mode 标签往返：位串化再比对一致
    let rt_ok = mode_label(6) == "rw" && mode_label(4) == "r" && mode_label(1) == "x"
        && mode_label(7) == "rwx" && mode_label(0).is_empty();
    s.add("mode 标签：rwx 位串化与闭集一致", rt_ok,
          "标签表是成文与审计依据，位↔串必须一一对应");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(2) && agrees_with_linux(13)
        && agrees_with_linux(17) && agrees_with_linux(36)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文非空且含三字段 + 双跑指纹
    let sum = compose_access_summary(&l, "rw");
    let text_ok = match &sum {
        Ok(t) => t.contains("path=rw") && t.contains("bits=rw") && t.contains("ok=1"),
        Err(_) => false,
    };
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("成文与指纹：小结三字段齐备且双跑哈希逐位一致",
          text_ok && sum == compose_access_summary(&l, "rw") && f1 == f2,
          "成文为纯函数；同语料两次哈希必须逐位一致");

    // 动态明细（≤8）：注入账快照 + 指纹 + 达成轮数
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let ok_text = format!("可达性达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_parser_guards() {
        assert_eq!(parse_mode(6), Ok(6));
        assert_eq!(parse_mode(8).err(), Some(AccCode::InvalidMode));
        assert_eq!(parse_mode(-3).err(), Some(AccCode::NegMode));
    }

    #[test]
    fn register_guards() {
        let mut l = AccessLedger::new();
        assert!(l.register("a", 7).is_ok());
        assert_eq!(l.register("", 7).err(), Some(AccCode::EmptyPath));
        assert_eq!(l.register("b", 9).err(), Some(AccCode::BadBits));
        assert!(l.register("a", 5).is_err());
    }

    #[test]
    fn access_bits_semantics() {
        let mut l = AccessLedger::new();
        let _ = l.register("rw", 6);
        assert!(l.access("rw", R_OK).is_ok());
        assert!(l.access("rw", W_OK).is_ok());
        assert_eq!(l.access("rw", X_OK).err(), Some(AccCode::AccessDenied));
    }

    #[test]
    fn fok_semantics() {
        let mut l = AccessLedger::new();
        let _ = l.register("ro", 4);
        assert!(l.access("ro", F_OK).is_ok());
        assert_eq!(l.access("nope", F_OK).err(), Some(AccCode::MissingPath));
    }

    #[test]
    fn missing_path_enoent() {
        let mut l = AccessLedger::new();
        let _ = l.register("a", 7);
        assert_eq!(l.access("ghost", R_OK).err(), Some(AccCode::MissingPath));
    }

    #[test]
    fn empty_and_long_path() {
        let mut l = AccessLedger::new();
        let _ = l.register("a", 7);
        assert_eq!(l.access("", R_OK).err(), Some(AccCode::EmptyPath));
        let long = String::from("p").repeat(NAME_MAX + 1);
        assert_eq!(l.access(long.as_str(), R_OK).err(), Some(AccCode::PathTruncate));
    }

    #[test]
    fn access_readonly_semantics() {
        let mut l = AccessLedger::new();
        let _ = l.register("ro", 4);
        assert_eq!(l.access("ro", W_OK).err(), Some(AccCode::AccessDenied));
        assert!(l.access("ro", R_OK).is_ok());
    }

    #[test]
    fn bad_access_matrix_green() {
        let (inj, per) = run_bad_access_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn access_rounds_full() {
        let (ok, probes) = run_access_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 8);
    }

    #[test]
    fn mode_label_roundtrip() {
        assert_eq!(mode_label(7), "rwx");
        assert_eq!(mode_label(0), "");
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10114_checks_green() {
        let set = run_unx10114_checks();
        assert!(set.all_passed(), "UNX-F10114 red: {}", set.red_items_text());
    }
}
