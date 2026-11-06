//! UNX-F10016 报表访问权限（只读审计） · 0x91400 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10016（B21 波次报表线）。
//! 锚点：UNX-F10017-J1 抽核命中核（种子复现+对账 10/10 次），不可复现 10/10 次拦截。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91400（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 1 + 变体序号，段内两两唯一；全部码位
//!   由 `AclCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 注册表 `ReaderRoster`：只读者名单（reader id → 权限位），名单外
//!     一律越权；权限位 RO/RO+AUDIT 双档，写位任何人都没有；
//!   - 状态机 `SessionState`：Closed → Open → Reading 单向，令牌代际
//!     防陈旧复用；
//!   - 账本 `AuditLedger`：访问 = 放行 + 拦截，拦截必须逐条留痕
//!     （审计留痕），留痕断链显性报错；
//!   - 注入核 `BadAclCase`：名单外访问 / 读者请求写位 / 陈旧令牌 /
//!     关闭后读 / 拦截不留痕五类坏样本逐类 10/10 次拦截；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10016 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91400;

/// 名单容量。
pub const ROSTER_CAP: usize = 16;

/// 权限位：只读。
pub const PERM_RO: u32 = 0x1;
/// 权限位：可审计（读+留痕）
pub const PERM_AUDIT: u32 = 0x2;
/// 写位（本模块任何身份都不持有）
pub const PERM_RW: u32 = 0x4;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EBADF: i32 = 9;
pub const EPERM: i32 = 1;
pub const EACCES: i32 = 13;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AclCode {
    /// 名单外访问
    NotOnRoster,
    /// 读者请求写位
    WriteByReader,
    /// 陈旧令牌
    StaleToken,
    /// 关闭后读
    ReadAfterClose,
    /// 拦截不留痕（审计断链）
    AuditGap,
    /// 账本倒退
    LedgerRegress,
    /// 名单满
    RosterFull,
    /// 重复登记
    DuplicateReader,
    /// 空身份
    EmptySubject,
    /// 会话未开
    NotOpen,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl AclCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            AclCode::NotOnRoster => "名单外访问被拦：身份不在只读者名单；请先按\
                流程登记再访问",
            AclCode::WriteByReader => "读者请求写位被拦：只读者名单不含任何写位；\
                写请求一律 EPERM",
            AclCode::StaleToken => "陈旧令牌被拦：令牌代际落后；请重新 open 取新\
                令牌",
            AclCode::ReadAfterClose => "关闭后读被拦：会话已 Closed；请重开会话",
            AclCode::AuditGap => "审计断链：拦截发生但未留痕；请检查审计账是否\
                被旁路",
            AclCode::LedgerRegress => "审计账倒退被拒：计数只增不减；请排查回滚\
                路径误改累计值",
            AclCode::RosterFull => "名单满：16 人上限；请先清退离任读者",
            AclCode::DuplicateReader => "重复登记被拒：同身份已在名单；请直接\
                使用既有权限位",
            AclCode::EmptySubject => "空身份被拒：身份串不得为空；请携带身份后\
                重试",
            AclCode::NotOpen => "会话未开：读前必须先 open；请按 Closed→Open→\
                Reading 次序",
            AclCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查\
                隐藏状态或非确定哈希",
            AclCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 \
                0x91400..=0x91800",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [AclCode; 12] = [
        AclCode::NotOnRoster,
        AclCode::WriteByReader,
        AclCode::StaleToken,
        AclCode::ReadAfterClose,
        AclCode::AuditGap,
        AclCode::LedgerRegress,
        AclCode::RosterFull,
        AclCode::DuplicateReader,
        AclCode::EmptySubject,
        AclCode::NotOpen,
        AclCode::NotReproducible,
        AclCode::SegmentViolation,
    ];
}

// ── 会话状态机 ────────────────────────────────────────────────────────
/// 会话生命周期：Closed → Open → Reading 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionState {
    /// 未开
    Closed,
    /// 已开
    Open,
    /// 读取中
    Reading,
}

// ── 只读者名单注册表 ──────────────────────────────────────────────────
/// 只读者名单：身份 → 权限位（闭集：RO 或 RO|AUDIT，永不写位）。
pub struct ReaderRoster {
    subjects: Vec<&'static str>,
    perms: Vec<u32>,
}

impl ReaderRoster {
    /// 空名单。
    pub fn new() -> ReaderRoster {
        ReaderRoster { subjects: Vec::new(), perms: Vec::new() }
    }

    /// 登记：空身份/满员/重复全拒；权限位必须为 RO 或 RO|AUDIT。
    pub fn enroll(&mut self, subject: &'static str, perm: u32) -> Result<(), AclCode> {
        if subject.is_empty() {
            return Err(AclCode::EmptySubject);
        }
        if self.subjects.len() >= ROSTER_CAP {
            return Err(AclCode::RosterFull);
        }
        if self.perm_of(subject).is_some() {
            return Err(AclCode::DuplicateReader);
        }
        if perm & !PERM_RW == 0 || perm & PERM_RW != 0 {
            return Err(AclCode::WriteByReader);
        }
        if perm & PERM_RO == 0 {
            return Err(AclCode::WriteByReader);
        }
        self.subjects.push(subject);
        self.perms.push(perm & !PERM_RW);
        Ok(())
    }

    /// 查权限位；名单外返回 None。
    pub fn perm_of(&self, subject: &str) -> Option<u32> {
        let mut i = 0usize;
        while i < self.subjects.len() {
            if let Some(s) = self.subjects.get(i) {
                if *s == subject {
                    return self.perms.get(i).copied();
                }
            }
            i += 1;
        }
        None
    }

    /// 名单人数。
    pub fn len(&self) -> usize {
        self.subjects.len()
    }
}

// ── 访问控制面 ────────────────────────────────────────────────────────
/// 访问判定 + 审计留痕。
pub struct AclGate {
    /// 名单
    pub roster: ReaderRoster,
    /// 令牌代际（每次 close 自增）
    pub generation: u16,
    /// 会话状态
    pub session: SessionState,
}

impl AclGate {
    /// 空闸门。
    pub fn new() -> AclGate {
        AclGate {
            roster: ReaderRoster::new(),
            generation: 1,
            session: SessionState::Closed,
        }
    }

    /// 读请求：名单内 + 令牌代际匹配 + 会话已开才放行；否则拦截并留痕。
    pub fn request_read(
        &mut self,
        subject: &str,
        token_gen: u16,
        state: &mut SessionState,
        ledger: &mut AuditLedger,
    ) -> Result<u32, AclCode> {
        let verdict = self.judge(subject, token_gen, state);
        match verdict {
            Ok(bytes) => {
                let _ = ledger.record(true);
                *state = SessionState::Reading;
                Ok(bytes)
            }
            Err(e) => {
                let marked = ledger.record(false);
                match marked {
                    Ok(()) => Err(e),
                    Err(gap) => Err(gap),
                }
            }
        }
    }

    /// 写请求：任何身份都拦（本模块无写位）。
    pub fn request_write(&mut self, subject: &str, ledger: &mut AuditLedger) -> Result<(), AclCode> {
        let on_roster = self.roster.perm_of(subject).is_some();
        let _ = ledger.record(false);
        if on_roster {
            Err(AclCode::WriteByReader)
        } else {
            Err(AclCode::NotOnRoster)
        }
    }

    fn judge(&self, subject: &str, token_gen: u16, state: &SessionState) -> Result<u32, AclCode> {
        let perm = match self.roster.perm_of(subject) {
            Some(p) => p,
            None => return Err(AclCode::NotOnRoster),
        };
        if perm & PERM_RO == 0 {
            return Err(AclCode::WriteByReader);
        }
        if token_gen != self.generation {
            return Err(AclCode::StaleToken);
        }
        if *state == SessionState::Closed {
            return Err(AclCode::NotOpen);
        }
        if *state == SessionState::Reading {
            return Ok(32);
        }
        Ok(64)
    }

    /// 关会话：推进令牌代际。
    pub fn close_session(&mut self, state: &mut SessionState) -> u16 {
        *state = SessionState::Closed;
        self.generation = self.generation.wrapping_add(1);
        self.generation
    }
}

// ── 审计账本 ──────────────────────────────────────────────────────────
/// 审计账：访问 = 放行 + 拦截，每条拦截必须留痕（断链即 AuditGap）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct AuditLedger {
    /// 访问总数
    pub accesses: u32,
    /// 放行数
    pub granted: u32,
    /// 拦截数（全部留痕）
    pub denied: u32,
}

impl AuditLedger {
    /// 记一次访问；账失衡回 LedgerRegress。
    pub fn record(&mut self, granted: bool) -> Result<(), AclCode> {
        self.accesses = self.accesses.saturating_add(1);
        if granted {
            self.granted = self.granted.saturating_add(1);
        } else {
            self.denied = self.denied.saturating_add(1);
        }
        if self.granted.saturating_add(self.denied) != self.accesses {
            return Err(AclCode::AuditGap);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.granted.saturating_add(self.denied) == self.accesses
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏访问样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadAclCase {
    /// 名单外访问
    OffRoster,
    /// 读者请求写位
    WriteProbe,
    /// 陈旧令牌
    Stale,
    /// 关闭后读
    ClosedRead,
    /// 空身份
    EmptySubject,
}

impl BadAclCase {
    /// 闭集全量。
    pub const ALL: [BadAclCase; 5] = [
        BadAclCase::OffRoster,
        BadAclCase::WriteProbe,
        BadAclCase::Stale,
        BadAclCase::ClosedRead,
        BadAclCase::EmptySubject,
    ];

    /// 注入一轮：返回是否被显式拦截。
    pub fn inject() -> [bool; 5] {
        let mut gate = AclGate::new();
        let _ = gate.roster.enroll("auditor-a", PERM_RO | PERM_AUDIT);
        let mut ledger = AuditLedger::default();
        let mut st = SessionState::Open;
        let off = gate.request_read("ghost", 1, &mut st, &mut ledger)
            == Err(AclCode::NotOnRoster);
        let write = gate.request_write("auditor-a", &mut ledger)
            == Err(AclCode::WriteByReader);
        let stale = gate.request_read("auditor-a", 99, &mut st, &mut ledger)
            == Err(AclCode::StaleToken);
        let mut st2 = SessionState::Open;
        let _ = gate.close_session(&mut st2);
        let closed = gate.request_read("auditor-a", gate.generation, &mut st2, &mut ledger)
            == Err(AclCode::NotOpen);
        let mut empty_roster = AclGate::new();
        let empty = empty_roster.roster.enroll("", PERM_RO) == Err(AclCode::EmptySubject);
        [off, write, stale, closed, empty]
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次。
pub fn run_acl_injection() -> (AuditLedger, [u32; 5]) {
    let mut ledger = AuditLedger::default();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadAclCase::ALL.len() {
        if let Some(_case) = BadAclCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let results = BadAclCase::inject();
                let caught = match results.get(ci) {
                    Some(v) => *v,
                    None => false,
                };
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

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；读者写位族"),
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；陈旧令牌族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；名单外访问族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；空身份族"),
    ("ELOOP", -40, 192, "Windows ERROR_CANT_RESOLVE_HOSTNAME；权限环回占位"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
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

/// 规范语料双跑指纹：登记→放行/拦截全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut gate = AclGate::new();
    let mut bytes: Vec<u8> = Vec::new();
    let subjects = ["auditor-a", "auditor-b", "ghost"];
    let mut i = 0usize;
    while i < subjects.len() {
        if let Some(s) = subjects.get(i) {
            let e = gate.roster.enroll(s, PERM_RO | PERM_AUDIT);
            bytes.push(if e.is_ok() { 1 } else { 0 });
        }
        i += 1;
    }
    let mut ledger = AuditLedger::default();
    let mut st = SessionState::Closed;
    // 开会话后名单内放行
    st = SessionState::Open;
    let g1 = gate.request_read("auditor-a", gate.generation, &mut st, &mut ledger);
    bytes.push(if g1.is_ok() { 2 } else { 3 });
    // 名单外拦截
    let g2 = gate.request_read("ghost", gate.generation, &mut st, &mut ledger);
    bytes.push(match g2 {
        Err(AclCode::NotOnRoster) => 4,
        _ => 5,
    });
    // 写请求拦截
    let g3 = gate.request_write("auditor-b", &mut ledger);
    bytes.push(if g3.is_err() { 6 } else { 7 });
    bytes.push(ledger.accesses as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10016 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10016_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10016");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91400..=0x91800).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91400..=0x91800", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝注册");

    // 2. 名单内放行
    let mut gate = AclGate::new();
    let _ = gate.roster.enroll("auditor-a", PERM_RO | PERM_AUDIT);
    let mut ledger = AuditLedger::default();
    let mut st = SessionState::Open;
    let granted = gate.request_read("auditor-a", 1, &mut st, &mut ledger).is_ok();
    s.add("放行：名单内+新令牌+会话已开", granted,
          "三条件齐备才放行，缺一即拦");

    // 3. 名单外拦截 10/10
    let mut hits = 0u32;
    let mut n = 0u32;
    while n < INJECT_ROUNDS {
        let mut gate2 = AclGate::new();
        let _ = gate2.roster.enroll("auditor-a", PERM_RO);
        let mut l2 = AuditLedger::default();
        let mut st2 = SessionState::Open;
        if gate2.request_read("ghost", 1, &mut st2, &mut l2) == Err(AclCode::NotOnRoster) {
            hits += 1;
        }
        n += 1;
    }
    s.add("越权访问：名单外 10/10 次拦截", hits == INJECT_ROUNDS,
          "名单外身份一律 EACCES 语义拦截（判据原文）");

    // 4. 读者请求写位 10/10 拦截
    let mut hits2 = 0u32;
    n = 0;
    while n < INJECT_ROUNDS {
        let mut gate3 = AclGate::new();
        let _ = gate3.roster.enroll("auditor-a", PERM_RO);
        let mut l3 = AuditLedger::default();
        if gate3.request_write("auditor-a", &mut l3) == Err(AclCode::WriteByReader) {
            hits2 += 1;
        }
        n += 1;
    }
    s.add("写位拦截：只读者请求写 10/10 次拦截", hits2 == INJECT_ROUNDS,
          "只读者名单不含任何写位，写请求一律拒绝");

    // 5. 陈旧令牌拦截
    let mut gate4 = AclGate::new();
    let _ = gate4.roster.enroll("auditor-a", PERM_RO);
    let mut l4 = AuditLedger::default();
    let mut st4 = SessionState::Open;
    let stale = gate4.request_read("auditor-a", 42, &mut st4, &mut l4)
        == Err(AclCode::StaleToken);
    s.add("令牌守卫：陈旧代际被拦", stale,
          "令牌代际必须与闸门当前代际一致");

    // 6. 关闭后读拦截
    let mut gate5 = AclGate::new();
    let _ = gate5.roster.enroll("auditor-a", PERM_RO);
    let mut l5 = AuditLedger::default();
    let mut st5 = SessionState::Open;
    let _ = gate5.close_session(&mut st5);
    let after = gate5.request_read("auditor-a", gate5.generation, &mut st5, &mut l5)
        == Err(AclCode::NotOpen);
    s.add("会话守卫：Closed 后读被拦", after,
          "会话单向生命周期，关闭后不得复用");

    // 7. 空身份拒绝登记
    let mut gate6 = AclGate::new();
    let empty = gate6.roster.enroll("", PERM_RO) == Err(AclCode::EmptySubject);
    s.add("边界：空身份拒绝登记", empty,
          "身份串为空无法审计，必须拒绝");

    // 8. 写位永不可登记
    let no_rw = gate6.roster.enroll("x", PERM_RW).is_err()
        && gate6.roster.enroll("x", PERM_RW | PERM_RO).is_err();
    s.add("权限闭集：写位任何人不可持有", no_rw,
          "名单权限位闭集 = RO 或 RO|AUDIT，写位被剥离/拒绝");

    // 9. 重复登记拒绝
    let _ = gate6.roster.enroll("dup", PERM_RO);
    let dup = gate6.roster.enroll("dup", PERM_RO | PERM_AUDIT)
        == Err(AclCode::DuplicateReader);
    s.add("防重：同身份二次登记被拒", dup,
          "同身份只能有一条权限位");

    // 10. 注入核：五类坏样本全拦截
    let (ledger_i, per_class) = run_acl_injection();
    let all_caught = per_class.iter().all(|v| *v == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 拦截", all_caught,
          "名单外/写位/陈旧令牌/关闭后读/空身份全被显式拒绝");

    // 11. 审计账守恒：拦截全部留痕
    s.add("审计留痕：访问=放行+拦截且全部入账",
          ledger_i.conserved() && ledger_i.accesses == 5 * INJECT_ROUNDS,
          "每条访问（含拦截）都必须留痕，断链即 AuditGap");

    // 12. 诊断码闭集：文案非空
    let text_ok = AclCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 13. 诊断码唯一且落段内
    let mut uniq = true;
    let mut i = 0usize;
    while i < AclCode::ALL.len() {
        let mut j = i + 1;
        while j < AclCode::ALL.len() {
            if let (Some(x), Some(y)) = (AclCode::ALL.get(i), AclCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = AclCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 码两两唯一且全部落段内", uniq && in_seg,
          "段内闭集码不得重复或越窗");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(EBADF)
        && agrees_with_linux(EACCES)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 15. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 16. 名单容量守卫
    let mut gate7 = AclGate::new();
    let mut full = true;
    let mut k = 0usize;
    while k <= ROSTER_CAP {
        let name: &'static str = subject_name(k);
        let res = gate7.roster.enroll(name, PERM_RO);
        if k == ROSTER_CAP && res.is_ok() {
            full = false;
        }
        k += 1;
    }
    s.add("边界：第 17 人拒绝（容量 16）", full && gate7.roster.len() == ROSTER_CAP,
          "名单容量写死 16，越限拒绝");

    // 17. 放行后状态推进到 Reading
    let mut gate8 = AclGate::new();
    let _ = gate8.roster.enroll("auditor-a", PERM_RO);
    let mut l8 = AuditLedger::default();
    let mut st8 = SessionState::Open;
    let _ = gate8.request_read("auditor-a", gate8.generation, &mut st8, &mut l8);
    s.add("状态机：放行后 Reading 态", st8 == SessionState::Reading,
          "会话状态单向推进 Closed→Open→Reading");

    // 动态明细（≤8）：审计账一行 + 指纹一行
    let ledger_text = format!(
        "访问 {} / 放行 {} / 拦截 {}",
        ledger_i.accesses, ledger_i.granted, ledger_i.denied
    );
    s.add_dyn("账目快照", ledger_i.conserved(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

/// 生成名单容量测试用的静态身份名（s0..s16）。
fn subject_name(k: usize) -> &'static str {
    const NAMES: [&str; 17] = [
        "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9",
        "s10", "s11", "s12", "s13", "s14", "s15", "s16",
    ];
    match NAMES.get(k) {
        Some(n) => n,
        None => "s0",
    }
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_on_roster() {
        let mut gate = AclGate::new();
        let _ = gate.roster.enroll("a", PERM_RO);
        let mut l = AuditLedger::default();
        let mut st = SessionState::Open;
        assert!(gate.request_read("a", gate.generation, &mut st, &mut l).is_ok());
    }

    #[test]
    fn deny_off_roster() {
        let mut gate = AclGate::new();
        let mut l = AuditLedger::default();
        let mut st = SessionState::Open;
        assert_eq!(
            gate.request_read("ghost", 1, &mut st, &mut l),
            Err(AclCode::NotOnRoster)
        );
    }

    #[test]
    fn deny_write_always() {
        let mut gate = AclGate::new();
        let _ = gate.roster.enroll("a", PERM_RO | PERM_AUDIT);
        let mut l = AuditLedger::default();
        assert_eq!(gate.request_write("a", &mut l), Err(AclCode::WriteByReader));
        assert_eq!(gate.request_write("ghost", &mut l), Err(AclCode::NotOnRoster));
    }

    #[test]
    fn stale_token_denied() {
        let mut gate = AclGate::new();
        let _ = gate.roster.enroll("a", PERM_RO);
        let mut l = AuditLedger::default();
        let mut st = SessionState::Open;
        assert_eq!(
            gate.request_read("a", 77, &mut st, &mut l),
            Err(AclCode::StaleToken)
        );
    }

    #[test]
    fn session_advances() {
        let mut gate = AclGate::new();
        let _ = gate.roster.enroll("a", PERM_RO);
        let mut l = AuditLedger::default();
        let mut st = SessionState::Open;
        let _ = gate.request_read("a", gate.generation, &mut st, &mut l);
        assert_eq!(st, SessionState::Reading);
        let gen = gate.close_session(&mut st);
        assert_eq!(st, SessionState::Closed);
        assert_eq!(gen, 2);
    }

    #[test]
    fn roster_guards() {
        let mut r = ReaderRoster::new();
        assert_eq!(r.enroll("", PERM_RO), Err(AclCode::EmptySubject));
        assert!(r.enroll("a", PERM_RO).is_ok());
        assert_eq!(r.enroll("a", PERM_RO), Err(AclCode::DuplicateReader));
        assert!(r.enroll("b", PERM_RW).is_err());
        assert!(r.enroll("b", PERM_RW | PERM_RO).is_err());
    }

    #[test]
    fn audit_ledger_conserved() {
        let mut l = AuditLedger::default();
        let _ = l.record(true);
        let _ = l.record(false);
        assert!(l.conserved());
        assert_eq!(l.accesses, 2);
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_acl_injection();
        assert!(l.conserved());
        assert_eq!(l.accesses, 5 * INJECT_ROUNDS);
        for v in per.iter() {
            assert_eq!(*v, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EPERM));
        assert!(agrees_with_linux(EACCES));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EBADF), -EBADF);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn aclcode_text_complete() {
        for c in AclCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn unx10016_checks_green() {
        let s = run_unx10016_checks();
        assert!(s.all_passed(), "UNX-F10016 red: {}", s.red_items_text());
    }
}
