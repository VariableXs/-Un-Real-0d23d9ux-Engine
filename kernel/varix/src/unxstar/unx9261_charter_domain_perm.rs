//! UNX-F9261 权限错误面总纲（EPERM vs EACCES 判定域） · 0x7AEC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9261 逐字）：
//!   两错误码判定域判据过，EPERM（操作特权缺失）与 EACCES（对象权限
//!   不足）分界逐格落账，与 Linux 语义逐例一致格过。
//!
//! SEG_BASE = 0x7AEC0：诊断码闭集说明——本模块全部诊断码由
//! `CharterCode` 闭集枚举给出，码值 = SEG_BASE + 变体序号（+0x01 起），
//! 段窗口 0x40 对齐独占；表外码不得进入宪章账。
//!
//! 设计要点：
//!   ① `Principal`/`FileObj`：四域判定主体与对象模型（uid/gid/权限位/
//!      能力位四域规则的输入）。
//!   ② `CharterRule` + `RULE_TABLE`：宪章规则闭集登记表（12 条，四域
//!      各 3 条），每条带正典场景与期望裁决，全部可复算。
//!   ③ `eval_*` 判定器族：uid 域→gid 域→权限位域→能力位域按序复算，
//!      EPERM 与 EACCES 分界逐格裁定。
//!   ④ `CharterLedger`：复算账 + 违例注入账（注入=检出+逃逸，逃逸恒 0）。
//!   ⑤ `ERRNO_TABLE`：Linux errno 对照表；`fnv1a`/`corpus_fingerprint`
//!      双跑指纹。
//! ─────────────────────────────────────────────────────────────────────
use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
/// 诊断段基址（F9261 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7AEC0;
/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 宪章规则闭集条数（四域共 12 条）。
pub const RULE_COUNT: usize = 12;
/// 权限位：owner 组基移 6、group 组基移 3、other 组基移 0。
pub const PERM_R: u32 = 4;
/// 权限位：写。
pub const PERM_W: u32 = 2;
/// 权限位：执行/穿越。
pub const PERM_X: u32 = 1;
/// CAP_CHOWN（bit0）：chown/chmod 特权。
pub const CAP_CHOWN: u32 = 1 << 0;
/// CAP_FOWNER（bit1）：对非属主文件操作文件属主特权。
pub const CAP_FOWNER: u32 = 1 << 1;
/// CAP_DAC_OVERRIDE（bit2）：绕过 r/w 对象权限。
pub const CAP_DAC_OVERRIDE: u32 = 1 << 2;
/// CAP_DAC_READ_SEARCH（bit3）：绕过读/搜索对象权限。
pub const CAP_DAC_READ_SEARCH: u32 = 1 << 3;
/// root uid（Linux 口径）。
pub const ROOT_UID: u32 = 0;
/// 判定主体：进程凭证。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Principal {
    /// 有效 uid（对象归属类判定的基准）
    pub fsuid: u32,
    /// 有效 gid
    pub fsgid: u32,
    /// 附加组（定容 4，count 内有效）
    pub groups: [u32; 4],
    /// 附加组数量
    pub group_count: usize,
    /// 能力位集合
    pub caps: u32,
    /// 是否处于特权执行体（root 口径：fsuid==0 视为 root）
    pub is_root: bool,
}
impl Principal {
    /// 构造普通用户。
    pub fn user(uid: u32, gid: u32) -> Principal {
        Principal {
            fsuid: uid,
            fsgid: gid,
            groups: [0; 4],
            group_count: 0,
            caps: 0,
            is_root: uid == ROOT_UID,
        }
    }
    /// 构造 root。
    pub fn root() -> Principal {
        Principal {
            fsuid: ROOT_UID,
            fsgid: ROOT_UID,
            groups: [0; 4],
            group_count: 0,
            caps: 0xFFFF_FFFF,
            is_root: true,
        }
    }
    /// 附加组追加（满则拒绝）。
    pub fn add_group(&mut self, gid: u32) -> Result<(), CharterCode> {
        if self.group_count >= self.groups.len() {
            return Err(CharterCode::GroupFull);
        }
        if let Some(slot) = self.groups.get_mut(self.group_count) {
            *slot = gid;
        }
        self.group_count += 1;
        Ok(())
    }
    /// 是否属于某组（有效组或附加组）。
    pub fn in_group(&self, gid: u32) -> bool {
        if self.fsgid == gid {
            return true;
        }
        let mut i = 0usize;
        while i < self.group_count && i < self.groups.len() {
            if let Some(g) = self.groups.get(i) {
                if *g == gid {
                    return true;
                }
            }
            i += 1;
        }
        false
    }
    /// 是否持有能力位。
    pub fn has_cap(&self, bit: u32) -> bool {
        self.caps & bit == bit
    }
}
/// 判定对象：文件/目录凭证。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FileObj {
    /// 属主 uid
    pub uid: u32,
    /// 属组 gid
    pub gid: u32,
    /// st_mode 低 9 位（三组 rwx）
    pub mode: u32,
    /// 是否目录（目录参与 x 穿越语义）
    pub is_dir: bool,
}
impl FileObj {
    /// 构造。
    pub const fn new(uid: u32, gid: u32, mode: u32, is_dir: bool) -> FileObj {
        FileObj { uid, gid, mode, is_dir }
    }
    /// 取某组的 3 位权限（class：0=owner 1=group 2=other）。
    pub fn class_bits(&self, class: usize) -> u32 {
        match class {
            0 => (self.mode >> 6) & 0o7,
            1 => (self.mode >> 3) & 0o7,
            _ => self.mode & 0o7,
        }
    }
}
/// 访问请求方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessKind {
    /// 读
    Read,
    /// 写
    Write,
    /// 执行/穿越
    Exec,
}
/// 三态裁决：允许 / EACCES（对象权限不足）/ EPERM（操作特权缺失）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 允许
    Allow,
    /// 对象权限不足
    DenyEacces,
    /// 操作特权缺失
    DenyEperm,
}
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharterCode {
    /// 规则号不在闭集
    UnknownRule,
    /// 规则所属域与判定域不符
    DomainMismatch,
    /// 规则复算裁决与登记期望漂移
    VerdictDrift,
    /// EPERM/EACCES 分界误用（对象权限缺口报成特权缺失或反之）
    BoundaryMisuse,
    /// 能力位缺失却执行特权操作
    CapAbsent,
    /// 规则集不完整（四域缺条）
    RuleSetIncomplete,
    /// 附加组满
    GroupFull,
    /// root 豁免面越界（root 也不豁免的格被豁免）
    RootExemptOverreach,
    /// 复算账倒退
    LedgerRegress,
    /// 注入逃逸
    ViolationEscaped,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}
impl CharterCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            CharterCode::UnknownRule => "规则号不在宪章闭集内；请使用 \
                RULE_TABLE 内 12 条规则的编号，表外号一律拒绝",
            CharterCode::DomainMismatch => "规则所属域与判定域不符：uid/gid/权限位/\
                能力位四域各管各的输入，跨域复算一律拒绝",
            CharterCode::VerdictDrift => "规则复算裁决与登记期望漂移：宪章规则必须\
                真实可复算，请核对正典场景的输入是否被改动",
            CharterCode::BoundaryMisuse => "EPERM/EACCES 分界误用：对象权限缺口\
                报 EACCES，操作特权缺失报 EPERM，两者不得互换",
            CharterCode::CapAbsent => "能力位缺失却执行特权操作：请先确认持有对应 \
                CAP_* 位或以 root 身份执行",
            CharterCode::RuleSetIncomplete => "宪章规则集不完整：四域 12 条必须\
                全部登记，缺条即拒绝收口",
            CharterCode::GroupFull => "附加组已满：定容 4 组；请复用已登记组或\
                扩容主体模型",
            CharterCode::RootExemptOverreach => "root 豁免面越界：root 不豁免的格\
                （如 fd 域界、无任一 x 位的执行）被误豁免；请按豁免表收紧",
            CharterCode::LedgerRegress => "复算账倒退被拒：计数只增不减；请排查\
                回滚路径误改累计值",
            CharterCode::ViolationEscaped => "违例注入逃逸：注入的违例必须全部被\
                宪章抓到，逃逸即收口失败",
            CharterCode::NotReproducible => "双跑不一致：同输入两次结果不同；\
                请排查隐藏状态或非确定哈希",
            CharterCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }
    /// 闭集全量。
    pub const ALL: [CharterCode; 12] = [
        CharterCode::UnknownRule,
        CharterCode::DomainMismatch,
        CharterCode::VerdictDrift,
        CharterCode::BoundaryMisuse,
        CharterCode::CapAbsent,
        CharterCode::RuleSetIncomplete,
        CharterCode::GroupFull,
        CharterCode::RootExemptOverreach,
        CharterCode::LedgerRegress,
        CharterCode::ViolationEscaped,
        CharterCode::NotReproducible,
        CharterCode::SegmentViolation,
    ];
}
/// uid 域：判定主体属于对象的哪个归属类（0=owner 1=group 2=other）。
/// Linux 语义：fsuid 匹配即 owner 类，短路；不匹配看组；再不匹配 other。
pub fn class_of(p: &Principal, obj: &FileObj) -> usize {
    if p.fsuid == obj.uid {
        return 0;
    }
    if p.in_group(obj.gid) {
        return 1;
    }
    2
}
/// 权限位域：按归属类取 3 位并比对请求方向。
/// 目录的 x 位是穿越权；文件三组独立。
pub fn perm_check(obj: &FileObj, class: usize, kind: AccessKind) -> Verdict {
    let bits = obj.class_bits(class);
    let need = match kind {
        AccessKind::Read => PERM_R,
        AccessKind::Write => PERM_W,
        AccessKind::Exec => PERM_X,
    };
    if bits & need == need {
        Verdict::Allow
    } else {
        Verdict::DenyEacces
    }
}
/// 能力位域：CAP_DAC_OVERRIDE 可绕 r/w；x 位仍要求任一 x 存在
/// （owner/group/other 三组任一组有 x 即可）。
pub fn cap_check(p: &Principal, obj: &FileObj, kind: AccessKind) -> Verdict {
    match kind {
        AccessKind::Read | AccessKind::Write => {
            if p.has_cap(CAP_DAC_OVERRIDE) || p.has_cap(CAP_DAC_READ_SEARCH) {
                Verdict::Allow
            } else {
                Verdict::DenyEacces
            }
        }
        AccessKind::Exec => {
            let any_x = (obj.mode & 0o111) != 0;
            if any_x && p.has_cap(CAP_DAC_OVERRIDE) {
                Verdict::Allow
            } else {
                Verdict::DenyEacces
            }
        }
    }
}
/// 全链判定：owner→group→other 短路序 + root 豁免 + 能力位兜底。
/// 判定序与 Linux 一致：先 root 豁免（x 位仍需任一 x），再归属类短路。
pub fn eval_access(p: &Principal, obj: &FileObj, kind: AccessKind) -> Verdict {
    if p.is_root {
        if kind == AccessKind::Exec && obj.mode & 0o111 == 0 {
            return Verdict::DenyEacces;
        }
        return Verdict::Allow;
    }
    let class = class_of(p, obj);
    let v = perm_check(obj, class, kind);
    if v == Verdict::Allow {
        return Verdict::Allow;
    }
    // 权限位不足：能力位兜底（真实 uid 语义，不降权回退到其他组）
    cap_check(p, obj, kind)
}
/// 特权操作判定（EPERM 域）：非 owner 非 root 执行 chown/chmod → EPERM。
/// 与对象权限位无关——这是判定域分界的核心例。
pub fn eval_chown(p: &Principal, obj: &FileObj, new_uid: u32) -> Result<(), CharterCode> {
    if p.is_root || p.fsuid == obj.uid {
        return Ok(());
    }
    // 非 owner 也可以持有 CAP_CHOWN 对「自己的文件」操作，但目标文件
    // 不是自己的：需要 CAP_FOWNER。
    if p.has_cap(CAP_FOWNER) {
        return Ok(());
    }
    let _ = new_uid;
    Err(CharterCode::CapAbsent)
}
/// 宪章规则：编号 + 域 + 陈述 + 正典场景期望裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuleDomain {
    /// uid 域
    Uid,
    /// gid 域
    Gid,
    /// 权限位域
    PermBits,
    /// 能力位域
    CapBits,
}
/// 规则编号闭集（R01..R12）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuleId {
    R01,
    R02,
    R03,
    R04,
    R05,
    R06,
    R07,
    R08,
    R09,
    R10,
    R11,
    R12,
}
impl RuleId {
    /// 闭集全量。
    pub const ALL: [RuleId; 12] = [
        RuleId::R01, RuleId::R02, RuleId::R03, RuleId::R04,
        RuleId::R05, RuleId::R06, RuleId::R07, RuleId::R08,
        RuleId::R09, RuleId::R10, RuleId::R11, RuleId::R12,
    ];
    /// 表内序号。
    pub fn index(self) -> usize {
        self as usize
    }
}
/// 单条宪章规则。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CharterRule {
    /// 规则编号
    pub id: RuleId,
    /// 所属域
    pub domain: RuleDomain,
    /// 规则陈述（宪章原文口径）
    pub statement: &'static str,
    /// 正典场景期望裁决描述
    pub expect: &'static str,
}
/// 宪章规则闭集登记表：四域共 12 条。
pub const RULE_TABLE: [CharterRule; RULE_COUNT] = [
    CharterRule { id: RuleId::R01, domain: RuleDomain::Uid,
        statement: "uid 域：fsuid 等于对象属主即归 owner 类并短路",
        expect: "owner 类命中即不再查组" },
    CharterRule { id: RuleId::R02, domain: RuleDomain::Uid,
        statement: "uid 域：fsuid 不等属主时降组类复算，不直接拒绝",
        expect: "uid 不匹配不是拒绝理由，是降类信号" },
    CharterRule { id: RuleId::R03, domain: RuleDomain::Gid,
        statement: "gid 域：fsgid 等于对象属组即归 group 类",
        expect: "有效组命中归组类" },
    CharterRule { id: RuleId::R04, domain: RuleDomain::Gid,
        statement: "gid 域：附加组包含对象属组同样归 group 类",
        expect: "附加组与有效组同权" },
    CharterRule { id: RuleId::R05, domain: RuleDomain::PermBits,
        statement: "权限位域：按归属类取 3 位比对请求方向，缺位即 EACCES",
        expect: "缺位 EACCES，不越类借用" },
    CharterRule { id: RuleId::R06, domain: RuleDomain::PermBits,
        statement: "权限位域：owner 类不匹配组位（owner 不在组通道上）",
        expect: "owner 类只看 owner 位，跳组语义显式" },
    CharterRule { id: RuleId::R07, domain: RuleDomain::PermBits,
        statement: "权限位域：目录 x 位是穿越权，缺 x 穿越即 EACCES",
        expect: "目录缺 x 穿越拒绝" },
    CharterRule { id: RuleId::R08, domain: RuleDomain::CapBits,
        statement: "能力位域：CAP_DAC_OVERRIDE 绕 r/w 但执行仍需任一 x 位存在",
        expect: "能力不创造 x 位" },
    CharterRule { id: RuleId::R09, domain: RuleDomain::CapBits,
        statement: "能力位域：缺 CAP_* 执行特权操作即 EPERM（CapAbsent）",
        expect: "缺能力报特权缺失" },
    CharterRule { id: RuleId::R10, domain: RuleDomain::PermBits,
        statement: "EPERM/EACCES 分界：非 owner 非 root chown 报特权缺失",
        expect: "操作特权缺失归 EPERM 域，与对象位无关" },
    CharterRule { id: RuleId::R11, domain: RuleDomain::Uid,
        statement: "root 豁免：root 绕过对象权限位但 x 位仍需任一 x 存在",
        expect: "root 不创造 x 位" },
    CharterRule { id: RuleId::R12, domain: RuleDomain::Gid,
        statement: "分界声明：EACCES 属对象权限不足，EPERM 属操作特权缺失",
        expect: "两码判定域互斥不重叠" },
];
/// 域一致性：规则域与判定域必须同域。
pub fn rule_domain_ok(rule: &CharterRule, domain: RuleDomain) -> bool {
    rule.domain == domain
}
/// 规则集完整性：12 条全登记、编号互异、域覆盖四域。
pub fn rule_set_complete() -> bool {
    if RULE_TABLE.len() != RULE_COUNT {
        return false;
    }
    let mut uid = 0u32;
    let mut gid = 0u32;
    let mut perm = 0u32;
    let mut cap = 0u32;
    let mut i = 0usize;
    while i < RULE_TABLE.len() {
        if let Some(r) = RULE_TABLE.get(i) {
            if r.id.index() != i {
                return false;
            }
            if r.statement.is_empty() || r.expect.is_empty() {
                return false;
            }
            match r.domain {
                RuleDomain::Uid => uid += 1,
                RuleDomain::Gid => gid += 1,
                RuleDomain::PermBits => perm += 1,
                RuleDomain::CapBits => cap += 1,
            }
        }
        i += 1;
    }
    uid >= 1 && gid >= 1 && perm >= 1 && cap >= 1
        && uid.saturating_add(gid).saturating_add(perm).saturating_add(cap) == RULE_COUNT as u32
}
/// 逐条复算：按规则编号跑正典场景，裁决必须与登记期望一致。
/// 返回 Err(CharterCode::VerdictDrift) 即复算失败。
pub fn recompute_rule(id: RuleId) -> Result<bool, CharterCode> {
    let rule = match RULE_TABLE.get(id.index()) {
        Some(r) => *r,
        None => return Err(CharterCode::UnknownRule),
    };
    // 各规则的正典场景与复算谓词（真实可复算，不是抄列表）
    let ok = match rule.id {
        RuleId::R01 => {
            // fsuid==obj.uid → class_of == 0
            let p = Principal::user(1000, 1000);
            let obj = FileObj::new(1000, 1001, 0o640, false);
            class_of(&p, &obj) == 0
        }
        RuleId::R02 => {
            // fsuid 不匹配 → 降组类（gid 相同则 1），不拒绝
            let p = Principal::user(2000, 1001);
            let obj = FileObj::new(1000, 1001, 0o640, false);
            class_of(&p, &obj) == 1
        }
        RuleId::R03 => {
            // fsgid==obj.gid → group 类
            let p = Principal::user(2000, 1001);
            let obj = FileObj::new(1000, 1001, 0o004, false);
            class_of(&p, &obj) == 1
        }
        RuleId::R04 => {
            // 附加组包含 obj.gid → group 类
            let mut p = Principal::user(2000, 3000);
            let _ = p.add_group(1001);
            let obj = FileObj::new(1000, 1001, 0o004, false);
            class_of(&p, &obj) == 1
        }
        RuleId::R05 => {
            // other 只有 r：读允许、写 EACCES
            let obj = FileObj::new(1000, 1001, 0o604, false);
            perm_check(&obj, 2, AccessKind::Read) == Verdict::Allow
                && perm_check(&obj, 2, AccessKind::Write) == Verdict::DenyEacces
        }
        RuleId::R06 => {
            // owner 类只看 owner 位：owner 位无 w，组位有 w 时 owner 写仍拒
            let obj = FileObj::new(1000, 1001, 0o470, false);
            perm_check(&obj, 0, AccessKind::Write) == Verdict::DenyEacces
        }
        RuleId::R07 => {
            // 目录缺 x：穿越 EACCES
            let p = Principal::user(2000, 3000);
            let obj = FileObj::new(1000, 1001, 0o755, true);
            let nox = FileObj::new(1000, 1001, 0o644, true);
            eval_access(&p, &obj, AccessKind::Exec) == Verdict::Allow
                && eval_access(&p, &nox, AccessKind::Exec) == Verdict::DenyEacces
        }
        RuleId::R08 => {
            // CAP_DAC_OVERRIDE 绕 w，但全无 x 位时执行仍拒
            let mut p = Principal::user(2000, 3000);
            p.caps |= CAP_DAC_OVERRIDE;
            let obj = FileObj::new(1000, 1001, 0o600, false);
            let nox = FileObj::new(1000, 1001, 0o600, false);
            eval_access(&p, &obj, AccessKind::Write) == Verdict::Allow
                && eval_access(&p, &nox, AccessKind::Exec) == Verdict::DenyEacces
        }
        RuleId::R09 => {
            // 缺 CAP_FOWNER 对他人文件 chown → CapAbsent（EPERM 域）
            let p = Principal::user(2000, 3000);
            let obj = FileObj::new(1000, 1001, 0o666, false);
            eval_chown(&p, &obj, 2000) == Err(CharterCode::CapAbsent)
        }
        RuleId::R10 => {
            // 非 owner 非 root 且对象 0o777：chown 仍拒（与对象位无关）
            let p = Principal::user(2000, 3000);
            let obj = FileObj::new(1000, 1001, 0o777, false);
            eval_chown(&p, &obj, 2000) == Err(CharterCode::CapAbsent)
        }
        RuleId::R11 => {
            // root 绕 rwx 但全无 x 位执行仍拒
            let p = Principal::root();
            let obj = FileObj::new(1000, 1001, 0o600, false);
            let nox = FileObj::new(1000, 1001, 0o000, false);
            eval_access(&p, &obj, AccessKind::Write) == Verdict::Allow
                && eval_access(&p, &nox, AccessKind::Exec) == Verdict::DenyEacces
        }
        RuleId::R12 => {
            // 分界互斥：对象位缺口走 EACCES，特权缺失走 EPERM 域码
            let p = Principal::user(2000, 3000);
            let obj = FileObj::new(1000, 1001, 0o600, false);
            eval_access(&p, &obj, AccessKind::Write) == Verdict::DenyEacces
                && eval_chown(&p, &obj, 2000) == Err(CharterCode::CapAbsent)
        }
    };
    if ok {
        Ok(true)
    } else {
        Err(CharterCode::VerdictDrift)
    }
}
/// 全表复算：12 条全部通过才算宪章复算收口。
pub fn recompute_all() -> Result<u32, CharterCode> {
    let mut passed = 0u32;
    let mut i = 0usize;
    while i < RuleId::ALL.len() {
        if let Some(id) = RuleId::ALL.get(i).copied() {
            if recompute_rule(id)? {
                passed += 1;
            }
        }
        i += 1;
    }
    Ok(passed)
}
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CharterLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数
    pub escaped: u32,
}
impl CharterLedger {
    const fn zero() -> CharterLedger {
        CharterLedger { injected: 0, detected: 0, escaped: 0 }
    }
    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), CharterCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(CharterCode::LedgerRegress);
        }
        Ok(())
    }
    /// 守恒判定。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}
/// 四类违例样本（逐类 10 轮全抓）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadCharterCase {
    /// 缺 CAP_FOWNER 对他人文件 chown
    ChownNoCap,
    /// 缺 w 位写（EACCES 域缺口必须被抓）
    WriteNoWBit,
    /// 目录缺 x 穿越
    TraverseNoX,
    /// root 对全无 x 位文件执行（root 豁免面越界必须被抓）
    RootExecNoX,
}
impl BadCharterCase {
    /// 执行违例：返回是否被宪章抓到。
    pub fn probe(self) -> bool {
        match self {
            BadCharterCase::ChownNoCap => {
                let p = Principal::user(2000, 3000);
                let obj = FileObj::new(1000, 1001, 0o777, false);
                eval_chown(&p, &obj, 2000) == Err(CharterCode::CapAbsent)
            }
            BadCharterCase::WriteNoWBit => {
                let p = Principal::user(2000, 3000);
                let obj = FileObj::new(1000, 1001, 0o644, false);
                eval_access(&p, &obj, AccessKind::Write) == Verdict::DenyEacces
            }
            BadCharterCase::TraverseNoX => {
                let p = Principal::user(2000, 3000);
                let dir = FileObj::new(1000, 1001, 0o644, true);
                eval_access(&p, &dir, AccessKind::Exec) == Verdict::DenyEacces
            }
            BadCharterCase::RootExecNoX => {
                let p = Principal::root();
                let obj = FileObj::new(1000, 1001, 0o000, false);
                eval_access(&p, &obj, AccessKind::Exec) == Verdict::DenyEacces
            }
        }
    }
    /// 闭集全量。
    pub const ALL: [BadCharterCase; 4] = [
        BadCharterCase::ChownNoCap,
        BadCharterCase::WriteNoWBit,
        BadCharterCase::TraverseNoX,
        BadCharterCase::RootExecNoX,
    ];
}
/// 执行违例注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_bad_charter_matrix() -> (CharterLedger, [u32; 4]) {
    let mut ledger = CharterLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadCharterCase::ALL.len() {
        if let Some(case) = BadCharterCase::ALL.get(ci).copied() {
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
/// (名称, 正值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", 1, 5, "Windows ERROR_ACCESS_DENIED；操作特权缺失族"),
    ("EACCES", 13, 5, "Windows ERROR_ACCESS_DENIED；对象权限不足族"),
    ("EINVAL", 22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("ENOENT", 2, 2, "Windows ERROR_FILE_NOT_FOUND；坏路径族"),
    ("EBADF", 9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
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
/// 规范语料双跑指纹：12 条规则复算结果 + 注入账入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < RuleId::ALL.len() {
        if let Some(id) = RuleId::ALL.get(i).copied() {
            let v = match recompute_rule(id) {
                Ok(true) => 1u8,
                _ => 0,
            };
            bytes.push(v);
        }
        i += 1;
    }
    let (ledger, per) = run_bad_charter_matrix();
    bytes.push((ledger.detected & 0xFF) as u8);
    let mut k = 0usize;
    while k < per.len() {
        if let Some(n) = per.get(k) {
            bytes.push((*n & 0xFF) as u8);
        }
        k += 1;
    }
    fnv1a(&bytes)
}
/// F9261 判据集。
pub fn run_unx9261_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9261");
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x7AEC0 0x40 对齐且在批窗口内", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");
    let p = Principal::user(1000, 1000);
    let obj = FileObj::new(1000, 1001, 0o640, false);
    s.add("正常流：owner 命中短路且 0o640 读允许",
          class_of(&p, &obj) == 0 && eval_access(&p, &obj, AccessKind::Read) == Verdict::Allow,
          "fsuid 等属主即 owner 类，owner 位 r=1 读允许");
    let mut pg = Principal::user(2000, 3000);
    let _ = pg.add_group(1001);
    let obj_g = FileObj::new(1000, 1001, 0o640, false);
    s.add("正常流：附加组命中 group 类且组位 r=1 读允许",
          class_of(&pg, &obj_g) == 1 && eval_access(&pg, &obj_g, AccessKind::Read) == Verdict::Allow,
          "附加组与有效组同权，组位 r=1 允许读");
    let po = Principal::user(2000, 3000);
    let obj0 = FileObj::new(1000, 1001, 0o600, false);
    s.add("边界：other 类 0o000 三方向全 EACCES",
          eval_access(&po, &obj0, AccessKind::Read) == Verdict::DenyEacces
              && eval_access(&po, &obj0, AccessKind::Write) == Verdict::DenyEacces
              && eval_access(&po, &obj0, AccessKind::Exec) == Verdict::DenyEacces,
          "other 无任何位时读/写/执行全拒");
    s.add("宪章闭集：12 条规则四域覆盖且编号连续",
          rule_set_complete() && RULE_TABLE.len() == RULE_COUNT,
          "uid/gid/权限位/能力位四域规则全部登记");
    let recomputed = recompute_all();
    s.add("宪章复算：12 条规则全部真实可复算通过",
          recomputed == Ok(12),
          "每条规则跑正典场景，裁决漂移即拒绝");
    let pb = Principal::user(2000, 3000);
    let objb = FileObj::new(1000, 1001, 0o777, false);
    let boundary_ok = eval_access(&pb, &objb, AccessKind::Write) == Verdict::Allow
        && eval_chown(&pb, &objb, 2000) == Err(CharterCode::CapAbsent);
    s.add("分界：0o777 可写但非 owner chown 仍拒（EPERM 域与位无关）", boundary_ok,
          "对象权限位管 EACCES，操作特权管 EPERM，互不借用");
    let objw = FileObj::new(1000, 1001, 0o644, false);
    s.add("分界：缺 w 位写报 EACCES 不报特权缺失",
          eval_access(&pb, &objw, AccessKind::Write) == Verdict::DenyEacces,
          "对象权限不足归 EACCES 判定域");
    let (ledger, per_class) = run_bad_charter_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("违例注入必抓：四类各 10/10 检出", all_caught,
          "缺能 chown/缺 w 写/缺 x 穿越/root 无 x 执行全抓");
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == BadCharterCase::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");
    let pr = Principal::root();
    let objr = FileObj::new(1000, 1001, 0o000, false);
    s.add("root 豁免面：读豁免、全无 x 位执行不豁免",
          eval_access(&pr, &objr, AccessKind::Read) == Verdict::Allow
              && eval_access(&pr, &objr, AccessKind::Exec) == Verdict::DenyEacces,
          "root 绕对象位但不创造 x 位");
    let mut pc = Principal::user(2000, 3000);
    pc.caps |= CAP_DAC_OVERRIDE;
    let objc = FileObj::new(1000, 1001, 0o600, false);
    s.add("能力位域：CAP_DAC_OVERRIDE 绕写、全无 x 位执行仍拒",
          eval_access(&pc, &objc, AccessKind::Write) == Verdict::Allow
              && eval_access(&pc, &objc, AccessKind::Exec) == Verdict::DenyEacces,
          "能力位不创造 x 位，执行需任一 x 存在");
    let codes_ok = agrees_with_linux(1) && agrees_with_linux(13) && agrees_with_linux(22)
        && agrees_with_linux(2) && agrees_with_linux(9)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：EPERM=1/EACCES=13 表内精确、表外拒绝", codes_ok,
          "错误码只允许 errno 对照表内正值");
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");
    let text_ok = CharterCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：12 个诊断码均带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");
    let mut uniq = true;
    let mut i17 = 0usize;
    while i17 < CharterCode::ALL.len() {
        let mut j17 = i17 + 1;
        while j17 < CharterCode::ALL.len() {
            if let (Some(x), Some(y)) = (CharterCode::ALL.get(i17), CharterCode::ALL.get(j17)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j17 += 1;
        }
        i17 += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");
    // 动态明细（≤8）
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}",
                              ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);
    let rule_text = format!("宪章规则 {} 条 / 复算通过 {} 条",
                            RULE_TABLE.len(), match recomputed { Ok(v) => v, Err(_) => 0 });
    s.add_dyn("宪章快照", recomputed == Ok(12), &rule_text);
    s
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owner_class_short_circuit() {
        let p = Principal::user(1000, 1000);
        let obj = FileObj::new(1000, 1001, 0o440, false);
        assert_eq!(class_of(&p, &obj), 0);
        // owner 有 r：可读；owner 类只看 owner 位，组位 r 不借用
        assert_eq!(eval_access(&p, &obj, AccessKind::Read), Verdict::Allow);
        // 组位 4 有 r 但主体是 owner 类：写仍拒
        assert_eq!(eval_access(&p, &obj, AccessKind::Write), Verdict::DenyEacces);
    }
    #[test]
    fn group_class_via_primary_and_supplementary() {
        let p1 = Principal::user(2000, 1001);
        let mut p2 = Principal::user(2000, 3000);
        let _ = p2.add_group(1001);
        let obj = FileObj::new(1000, 1001, 0o040, false);
        assert_eq!(class_of(&p1, &obj), 1);
        assert_eq!(class_of(&p2, &obj), 1);
        assert_eq!(eval_access(&p1, &obj, AccessKind::Read), Verdict::Allow);
    }
    #[test]
    fn rule_set_complete_and_recomputable() {
        assert!(rule_set_complete());
        assert_eq!(recompute_all(), Ok(12));
    }
    #[test]
    fn eperm_eacces_boundary() {
        let p = Principal::user(2000, 3000);
        let obj = FileObj::new(1000, 1001, 0o777, false);
        // 对象位全开：写允许；但 chown 仍拒（EPERM 域）
        assert_eq!(eval_access(&p, &obj, AccessKind::Write), Verdict::Allow);
        assert_eq!(eval_chown(&p, &obj, 2000), Err(CharterCode::CapAbsent));
    }
    #[test]
    fn cap_dac_override_not_x() {
        let mut p = Principal::user(2000, 3000);
        p.caps |= CAP_DAC_OVERRIDE;
        let obj = FileObj::new(1000, 1001, 0o600, false);
        assert_eq!(eval_access(&p, &obj, AccessKind::Write), Verdict::Allow);
        assert_eq!(eval_access(&p, &obj, AccessKind::Exec), Verdict::DenyEacces);
    }
    #[test]
    fn cap_fowner_allows_chown() {
        let mut p = Principal::user(2000, 3000);
        p.caps |= CAP_FOWNER;
        let obj = FileObj::new(1000, 1001, 0o644, false);
        assert!(eval_chown(&p, &obj, 2000).is_ok());
        let owner = Principal::user(1000, 1001);
        assert!(eval_chown(&owner, &obj, 2000).is_ok());
    }
    #[test]
    fn root_exempt_but_not_x() {
        let p = Principal::root();
        let obj = FileObj::new(1000, 1001, 0o000, false);
        assert_eq!(eval_access(&p, &obj, AccessKind::Read), Verdict::Allow);
        assert_eq!(eval_access(&p, &obj, AccessKind::Write), Verdict::Allow);
        assert_eq!(eval_access(&p, &obj, AccessKind::Exec), Verdict::DenyEacces);
    }
    #[test]
    fn injection_matrix_all_caught() {
        let (l, per) = run_bad_charter_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }
    #[test]
    fn unx9261_checks_green() {
        let s = run_unx9261_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}