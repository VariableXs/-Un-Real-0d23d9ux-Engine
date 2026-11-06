//! UNX-F9381 I 型联调总纲二（C2×C4 联签协议与挂点面） · 0x7CCC0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F9381-J1 联签协议判据过，C2（阻塞挂点）×C4（futex/epoll/
//!       pty 面）双方挂点面互认格过，跨界代写禁令声明格过。
//!
//! 设计要点：
//!   ① 挂点宪章按「挂点 × 规则」闭集登记：本总纲登记 C2×C4 交界处
//!      的 6 个挂点（futex WAIT/WAKE/REQUEUE、epoll CTL/WAIT、pty
//!      IOCTL），每个挂点带阻塞面对照（C2 侧）与实现域（C4 侧）。
//!   ② 联签协议：一个挂点只有 C2、C4 双方各签一笔才构成「互认」；
//!      单边签名、未签名、未知挂点、重复挂点全部显性拒绝，联签账
//!      只增不减。
//!   ③ 跨界代写禁令：挂点实现域归 C4，C2 只能签名互认不能代写实现；
//!      注入核把「C2 代写实现域」作为独立坏样本类，审计器逐次检出，
//!      注入账守恒（注入 = 检出 + 逃逸，逃逸恒 0）。
//!   ④ 规则复算：已登记挂点的规则域（阻塞面名非空、实现域合法、
//!      单源声明为真）逐条复算，与登记时零漂移。
//!   ⑤ 双跑指纹：FNV-1a 覆盖「登记 + 双签 + 复算 + 注入」全程语料，
//!      两次运行逐位一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9381 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7CCC0;

/// 挂点闭集容量。
pub const HOOK_CAP: usize = 8;

/// 复测轮数（注入矩阵每类轮数）。
pub const INJECT_ROUNDS: u32 = 10;

/// C2 侧（阻塞挂点面）域标识。
pub const SIDE_C2: u8 = 2;

/// C4 侧（futex/epoll/pty 实现面）域标识。
pub const SIDE_C4: u8 = 4;

// ── 挂点闭集 ──────────────────────────────────────────────────────────
/// C2×C4 交界挂点（闭集 1..=6）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookPoint {
    /// futex WAIT 阻塞挂点
    FutexWait,
    /// futex WAKE 唤醒挂点
    FutexWake,
    /// futex REQUEUE 迁移挂点
    FutexRequeue,
    /// epoll_ctl 兴趣表挂点
    EpollCtl,
    /// epoll_wait 就绪队列挂点
    EpollWait,
    /// pty ioctl 终端挂点
    PtyIoctl,
}

impl HookPoint {
    /// 挂点号（1 起步闭集）。
    pub fn id(self) -> u32 {
        SEG_BASE_LOW + self as u32
    }

    /// C4 实现域归属名。
    pub fn c4_owner(self) -> &'static str {
        match self {
            HookPoint::FutexWait => "C4.futex",
            HookPoint::FutexWake => "C4.futex",
            HookPoint::FutexRequeue => "C4.futex",
            HookPoint::EpollCtl => "C4.epoll",
            HookPoint::EpollWait => "C4.epoll",
            HookPoint::PtyIoctl => "C4.pty",
        }
    }

    /// C2 阻塞面对照名。
    pub fn c2_blocking(self) -> &'static str {
        match self {
            HookPoint::FutexWait => "C2.waitqueue",
            HookPoint::FutexWake => "C2.waitqueue",
            HookPoint::FutexRequeue => "C2.waitqueue",
            HookPoint::EpollCtl => "C2.blockpoll",
            HookPoint::EpollWait => "C2.blockpoll",
            HookPoint::PtyIoctl => "C2.ttyblock",
        }
    }

    /// 闭集全量。
    pub const ALL: [HookPoint; 6] = [
        HookPoint::FutexWait,
        HookPoint::FutexWake,
        HookPoint::FutexRequeue,
        HookPoint::EpollCtl,
        HookPoint::EpollWait,
        HookPoint::PtyIoctl,
    ];
}

/// 挂点号段低位（1..=6），与 SEG_BASE 解耦。
const SEG_BASE_LOW: u32 = 1;

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)。
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏地址族"),
    ("EINTR", -4, 995, "Windows ERROR_OPERATION_ABORTED；中断族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("EAGAIN", -11, 997, "Windows ERROR_IO_PENDING；重试族"),
    ("ENOSYS", -38, 1, "Windows ERROR_NOT_SUPPORTED；未实装族"),
    ("ENOTTY", -25, 1, "Windows ERROR_INVALID_FUNCTION；终端缺席族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；特权缺失族"),
];

/// 按 errno 正数查负值；表外返回 0。
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

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    code > 0 && linux_neg(code) == -code
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HkCode {
    /// 未知挂点号
    UnknownHook,
    /// 挂点重复登记
    DuplicateHook,
    /// C2 侧缺签
    MissingC2Sign,
    /// C4 侧缺签
    MissingC4Sign,
    /// 跨界代写（C2 写了 C4 实现域）
    CrossWrite,
    /// 规则复算漂移
    RuleDrift,
    /// 注册表超容
    RegistryFull,
    /// 联签账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
    /// 注入账守恒破坏
    LedgerEscape,
    /// 文案闭集为空
    EmptyDetail,
}

impl HkCode {
    /// 码值（SEG_BASE + 变体序号）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            HkCode::UnknownHook => "未知挂点被拒：挂点号必须落在 1..=6 闭集；\
                新挂点须先修宪章再登记",
            HkCode::DuplicateHook => "重复挂点被拒：同号挂点只允许登记一次；\
                如需变更请走规则修订而非重登记",
            HkCode::MissingC2Sign => "C2 侧缺签被拒：联签要求双方各签一笔；\
                请先取得阻塞挂点面签名",
            HkCode::MissingC4Sign => "C4 侧缺签被拒：实现域未确认不得声明互认；\
                请先取得 futex/epoll/pty 面签名",
            HkCode::CrossWrite => "跨界代写被拒：挂点实现域归 C4，C2 只能签名互认；\
                代写路径必须整体撤销",
            HkCode::RuleDrift => "规则复算漂移：登记后的规则域与复算值不一致；\
                请检查登记口是否绕过宪章闭集",
            HkCode::RegistryFull => "注册表超容被拒：挂点闭集容量已满；\
                本总纲闭集 6 点，超容即越权扩面",
            HkCode::LedgerRegress => "联签账倒退被拒：签名/拒绝计数只增不减；\
                请检查回滚路径是否误改累计值",
            HkCode::NotReproducible => "双跑不一致：同语料两次指纹不同；请排查隐藏\
                状态或非确定遍历",
            HkCode::SegmentViolation => "段位越界：段基址必须落在 \
                0x7CC00..=0x7D1C0 且 0x40 对齐",
            HkCode::LedgerEscape => "账守恒破坏：注入必须等于检出加逃逸，逃逸恒 0",
            HkCode::EmptyDetail => "诊断文案为空：闭集每码必须携带非空中文建议",
        }
    }

    /// 闭集全量。
    pub const ALL: [HkCode; 12] = [
        HkCode::UnknownHook,
        HkCode::DuplicateHook,
        HkCode::MissingC2Sign,
        HkCode::MissingC4Sign,
        HkCode::CrossWrite,
        HkCode::RuleDrift,
        HkCode::RegistryFull,
        HkCode::LedgerRegress,
        HkCode::NotReproducible,
        HkCode::SegmentViolation,
        HkCode::LedgerEscape,
        HkCode::EmptyDetail,
    ];
}

// ── 挂点规则（宪章登记项） ────────────────────────────────────────────
/// 一条挂点规则：挂点 × 实现域 × 阻塞面 × 单源声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HookRule {
    /// 挂点号（1..=6）
    pub hook_id: u32,
    /// C4 实现域归属（必须与 HookPoint::c4_owner 一致）
    pub owner: &'static str,
    /// C2 阻塞面对照（必须与 HookPoint::c2_blocking 一致）
    pub blocking: &'static str,
    /// 单源声明（挂点语义单源，禁止双实现）
    pub single_source: bool,
    /// 写入方域标识（只允许 SIDE_C4）
    pub written_by: u8,
}

/// 规则合法性：与闭集宪章逐字段复算。
pub fn rule_consistent(r: &HookRule) -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < HookPoint::ALL.len() {
        if let Some(p) = HookPoint::ALL.get(i) {
            if p.id() == r.hook_id {
                ok = p.c4_owner() == r.owner
                    && p.c2_blocking() == r.blocking
                    && r.single_source
                    && r.written_by == SIDE_C4;
            }
        }
        i += 1;
    }
    ok && r.hook_id >= 1 && r.hook_id <= 6
}

/// 联签账：单挂点双签状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CosignSlot {
    /// 挂点号
    pub hook_id: u32,
    /// C2 侧签名（0 = 未签）
    pub c2_sign: u32,
    /// C4 侧签名（0 = 未签）
    pub c4_sign: u32,
}

impl CosignSlot {
    /// 互认：双方均已签名。
    pub fn mutual(&self) -> bool {
        self.c2_sign != 0 && self.c4_sign != 0
    }
}

/// 挂点注册表 + 联签账本（双结构合一：登记面与签名面分离）。
#[derive(Clone, Debug)]
pub struct HookCharter {
    rules: Vec<HookRule>,
    signs: Vec<CosignSlot>,
    /// 已达成互认的挂点数（账只增）
    mutuals: u32,
    /// 显性拒绝笔数（账只增）
    rejects: u32,
}

impl HookCharter {
    /// 构造空宪章。
    pub fn new() -> HookCharter {
        HookCharter { rules: Vec::new(), signs: Vec::new(), mutuals: 0, rejects: 0 }
    }

    /// 宪章登记：未知号/重号/规则漂移/跨界代写/超容逐口拒绝。
    pub fn register(&mut self, r: HookRule) -> Result<(), HkCode> {
        if r.hook_id == 0 || r.hook_id > 6 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HkCode::UnknownHook);
        }
        if !rule_consistent(&r) {
            self.rejects = self.rejects.saturating_add(1);
            // 写入方越域是漂移中最典型的一类，单独成码便于定位。
            if r.written_by != SIDE_C4 {
                return Err(HkCode::CrossWrite);
            }
            return Err(HkCode::RuleDrift);
        }
        if self.rules.len() >= HOOK_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HkCode::RegistryFull);
        }
        let mut i = 0usize;
        while i < self.rules.len() {
            if let Some(old) = self.rules.get(i) {
                if old.hook_id == r.hook_id {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(HkCode::DuplicateHook);
                }
            }
            i += 1;
        }
        self.rules.push(r);
        self.signs.push(CosignSlot { hook_id: r.hook_id, c2_sign: 0, c4_sign: 0 });
        Ok(())
    }

    /// 侧签名：side ∈ {C2, C4}；未知挂点显性拒绝；账只增。
    pub fn sign(&mut self, hook_id: u32, side: u8, token: u32) -> Result<(), HkCode> {
        if token == 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(if side == SIDE_C2 { HkCode::MissingC2Sign } else { HkCode::MissingC4Sign });
        }
        let mut found = false;
        let mut i = 0usize;
        while i < self.signs.len() {
            if let Some(slot) = self.signs.get_mut(i) {
                if slot.hook_id == hook_id {
                    found = true;
                    let was_mutual = slot.mutual();
                    if side == SIDE_C2 {
                        slot.c2_sign = token;
                    } else if side == SIDE_C4 {
                        slot.c4_sign = token;
                    } else {
                        self.rejects = self.rejects.saturating_add(1);
                        return Err(HkCode::UnknownHook);
                    }
                    if !was_mutual && slot.mutual() {
                        self.mutuals = self.mutuals.saturating_add(1);
                    }
                }
            }
            i += 1;
        }
        if !found {
            return Err(HkCode::UnknownHook);
        }
        Ok(())
    }

    /// 互认查询：双方签名齐备即互认。
    pub fn is_mutual(&self, hook_id: u32) -> Result<bool, HkCode> {
        let mut i = 0usize;
        while i < self.signs.len() {
            if let Some(slot) = self.signs.get(i) {
                if slot.hook_id == hook_id {
                    return Ok(slot.mutual());
                }
            }
            i += 1;
        }
        Err(HkCode::UnknownHook)
    }

    /// 互认挂点总数（账面）。
    pub fn mutual_count(&self) -> u32 {
        self.mutuals
    }

    /// 登记数。
    pub fn registered(&self) -> usize {
        self.rules.len()
    }

    /// 拒绝笔数。
    pub fn rejected(&self) -> u32 {
        self.rejects
    }

    /// 全量互认复算：账面互认数 == 逐格互认数 且 全部已登记。
    pub fn reconcile(&self) -> bool {
        let mut count = 0u32;
        let mut i = 0usize;
        while i < self.signs.len() {
            if let Some(slot) = self.signs.get(i) {
                if slot.mutual() {
                    count = count.saturating_add(1);
                }
            }
            i += 1;
        }
        count == self.mutuals && self.rules.len() == self.signs.len()
    }
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 五类违例样本（注入核）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViolationCase {
    /// 未知挂点号 99
    UnknownHook,
    /// 重复登记同一挂点
    DuplicateHook,
    /// C2 代写实现域
    CrossWrite,
    /// 规则域漂移（owner 伪造）
    RuleDrift,
    /// 零令牌签名（缺签）
    ZeroTokenSign,
}

impl ViolationCase {
    /// 期望诊断码。
    pub fn expected(self) -> HkCode {
        match self {
            ViolationCase::UnknownHook => HkCode::UnknownHook,
            ViolationCase::DuplicateHook => HkCode::DuplicateHook,
            ViolationCase::CrossWrite => HkCode::CrossWrite,
            ViolationCase::RuleDrift => HkCode::RuleDrift,
            ViolationCase::ZeroTokenSign => HkCode::MissingC2Sign,
        }
    }

    /// 闭集全量。
    pub const ALL: [ViolationCase; 5] = [
        ViolationCase::UnknownHook,
        ViolationCase::DuplicateHook,
        ViolationCase::CrossWrite,
        ViolationCase::RuleDrift,
        ViolationCase::ZeroTokenSign,
    ];
}

/// 注入核：五类违例各注入 INJECT_ROUNDS 次，逐类断言显性拒绝。
pub fn run_violation_matrix() -> (InjectLedger, [u32; 5]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < ViolationCase::ALL.len() {
        let case = match ViolationCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut charter = HookCharter::new();
            // 底表：登记 futex WAIT（1 号）并完成双签互认
            let _ = charter.register(HookRule {
                hook_id: 1,
                owner: "C4.futex",
                blocking: "C2.waitqueue",
                single_source: true,
                written_by: SIDE_C4,
            });
            let _ = charter.sign(1, SIDE_C2, 0x2AAA);
            let _ = charter.sign(1, SIDE_C4, 0x4BBB);
            let caught = match case {
                ViolationCase::UnknownHook => {
                    charter
                        .register(HookRule {
                            hook_id: 99,
                            owner: "C4.futex",
                            blocking: "C2.waitqueue",
                            single_source: true,
                            written_by: SIDE_C4,
                        })
                        .err() == Some(HkCode::UnknownHook)
                }
                ViolationCase::DuplicateHook => {
                    charter
                        .register(HookRule {
                            hook_id: 1,
                            owner: "C4.futex",
                            blocking: "C2.waitqueue",
                            single_source: true,
                            written_by: SIDE_C4,
                        })
                        .err() == Some(HkCode::DuplicateHook)
                }
                ViolationCase::CrossWrite => {
                    charter
                        .register(HookRule {
                            hook_id: 2,
                            owner: "C4.futex",
                            blocking: "C2.waitqueue",
                            single_source: true,
                            written_by: SIDE_C2,
                        })
                        .err() == Some(HkCode::CrossWrite)
                }
                ViolationCase::RuleDrift => {
                    charter
                        .register(HookRule {
                            hook_id: 3,
                            owner: "C4.pseudo",
                            blocking: "C2.waitqueue",
                            single_source: true,
                            written_by: SIDE_C4,
                        })
                        .err() == Some(HkCode::RuleDrift)
                }
                ViolationCase::ZeroTokenSign => {
                    charter.sign(1, SIDE_C2, 0).err() == Some(HkCode::MissingC2Sign)
                }
            };
            ledger.record(caught);
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

// ── FNV-1a 双跑指纹 ───────────────────────────────────────────────────
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

/// 规范语料指纹：全闭集登记 + 双签 + 复算 + 注入逐格落语料。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut charter = HookCharter::new();
    let mut i = 0usize;
    while i < HookPoint::ALL.len() {
        if let Some(p) = HookPoint::ALL.get(i) {
            let reg = charter.register(HookRule {
                hook_id: p.id(),
                owner: p.c4_owner(),
                blocking: p.c2_blocking(),
                single_source: true,
                written_by: SIDE_C4,
            });
            bytes.push(match reg {
                Ok(()) => 1,
                Err(_) => 0,
            });
        }
        i += 1;
    }
    // 双签互认：C2 侧逐点签 + C4 侧逐点签
    let mut j = 0usize;
    while j < HookPoint::ALL.len() {
        if let Some(p) = HookPoint::ALL.get(j) {
            let s2 = charter.sign(p.id(), SIDE_C2, 0x2000 + p.id());
            let s4 = charter.sign(p.id(), SIDE_C4, 0x4000 + p.id());
            bytes.push(match s2 {
                Ok(()) => 2,
                Err(_) => 0,
            });
            bytes.push(match s4 {
                Ok(()) => 4,
                Err(_) => 0,
            });
            let mutual = match charter.is_mutual(p.id()) {
                Ok(m) => m,
                Err(_) => false,
            };
            bytes.push(mutual as u8);
        }
        j += 1;
    }
    bytes.push(charter.mutual_count() as u8);
    bytes.push(charter.reconcile() as u8);
    // 一轮跨界代写注入
    let cross = charter.register(HookRule {
        hook_id: 6,
        owner: "C4.pty",
        blocking: "C2.ttyblock",
        single_source: true,
        written_by: SIDE_C2,
    });
    bytes.push(match cross.err() {
        Some(HkCode::CrossWrite) => 1,
        _ => 0,
    });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9381 判据集（静态 ≥14 条 + 动态 ≤4 条）。
pub fn run_unx9381_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9381");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7CC00..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7CC00..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 宪章登记：6 挂点全量入册
    let mut charter = HookCharter::new();
    let mut reg_ok = true;
    let mut i = 0usize;
    while i < HookPoint::ALL.len() {
        if let Some(p) = HookPoint::ALL.get(i) {
            let r = charter.register(HookRule {
                hook_id: p.id(),
                owner: p.c4_owner(),
                blocking: p.c2_blocking(),
                single_source: true,
                written_by: SIDE_C4,
            });
            if r.is_err() {
                reg_ok = false;
            }
        }
        i += 1;
    }
    s.add("宪章登记：6 挂点全量入册", reg_ok && charter.registered() == 6,
          "闭集 6 点必须全部可登记且零拒绝");

    // 3. 联签协议：双方各签一笔后才互认
    let mut proto = HookCharter::new();
    let _ = proto.register(HookRule {
        hook_id: 1,
        owner: "C4.futex",
        blocking: "C2.waitqueue",
        single_source: true,
        written_by: SIDE_C4,
    });
    let before_mutual = match proto.is_mutual(1) {
        Ok(m) => m,
        Err(_) => true,
    };
    let _ = proto.sign(1, SIDE_C2, 0x21);
    let after_c2 = match proto.is_mutual(1) {
        Ok(m) => m,
        Err(_) => false,
    };
    let _ = proto.sign(1, SIDE_C4, 0x41);
    let after_both = match proto.is_mutual(1) {
        Ok(m) => m,
        Err(_) => false,
    };
    s.add("联签协议：单边不互认、双边才互认",
          !before_mutual && !after_c2 && after_both,
          "互认必须双方签名齐备，单边只是待签");

    // 4. 挂点面互认格：全闭集双签后互认数 = 6
    let mut full = HookCharter::new();
    let mut j = 0usize;
    while j < HookPoint::ALL.len() {
        if let Some(p) = HookPoint::ALL.get(j) {
            let _ = full.register(HookRule {
                hook_id: p.id(),
                owner: p.c4_owner(),
                blocking: p.c2_blocking(),
                single_source: true,
                written_by: SIDE_C4,
            });
            let _ = full.sign(p.id(), SIDE_C2, 0x2000 + p.id());
            let _ = full.sign(p.id(), SIDE_C4, 0x4000 + p.id());
        }
        j += 1;
    }
    s.add("挂点面互认：6 挂点全部互认且复算闭合",
          full.mutual_count() == 6 && full.reconcile(),
          "C2×C4 双方挂点面逐点互认，账面与逐格一致");

    // 5. 跨界代写禁令：C2 写实现域被拒
    let cross = full.register(HookRule {
        hook_id: 6,
        owner: "C4.pty",
        blocking: "C2.ttyblock",
        single_source: true,
        written_by: SIDE_C2,
    });
    s.add("跨界代写禁令：C2 代写显性 CrossWrite",
          cross.err() == Some(HkCode::CrossWrite) && full.registered() == 6,
          "实现域只归 C4，代写路径必须整体拒绝");

    // 6. 注入核：五类违例各 10/10 拒绝
    let (inj, per_class) = run_violation_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类违例各 10/10 显性拒绝", all_caught,
          "未知号/重号/代写/漂移/缺签逐类拒绝");

    // 7. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == 5 * INJECT_ROUNDS,
          "50 次注入必须 50 次检出、0 逃逸");

    // 8. 规则复算零漂移：闭集全量逐条复算
    let mut drift_ok = true;
    let mut k = 0usize;
    while k < HookPoint::ALL.len() {
        if let Some(p) = HookPoint::ALL.get(k) {
            let r = HookRule {
                hook_id: p.id(),
                owner: p.c4_owner(),
                blocking: p.c2_blocking(),
                single_source: true,
                written_by: SIDE_C4,
            };
            if !rule_consistent(&r) {
                drift_ok = false;
            }
        }
        k += 1;
    }
    s.add("规则复算：闭集规则逐条零漂移", drift_ok,
          "宪章字段必须与闭集定义逐位一致");

    // 9. 规则漂移检出：owner 伪造即拒
    let fake = rule_consistent(&HookRule {
        hook_id: 5,
        owner: "C4.fake",
        blocking: "C2.blockpoll",
        single_source: true,
        written_by: SIDE_C4,
    });
    let single_off = rule_consistent(&HookRule {
        hook_id: 5,
        owner: "C4.epoll",
        blocking: "C2.blockpoll",
        single_source: false,
        written_by: SIDE_C4,
    });
    s.add("漂移检出：伪造 owner/关单源声明均判漂移", !fake && !single_off,
          "规则域任一字段偏离闭集即漂移");

    // 10. 未知挂点与重复登记
    let mut dup = HookCharter::new();
    let _ = dup.register(HookRule {
        hook_id: 4,
        owner: "C4.epoll",
        blocking: "C2.blockpoll",
        single_source: true,
        written_by: SIDE_C4,
    });
    let unknown_ok = dup
        .register(HookRule {
            hook_id: 0,
            owner: "C4.epoll",
            blocking: "C2.blockpoll",
            single_source: true,
            written_by: SIDE_C4,
        })
        .err() == Some(HkCode::UnknownHook)
        && dup
            .register(HookRule {
                hook_id: 4,
                owner: "C4.epoll",
                blocking: "C2.blockpoll",
                single_source: true,
                written_by: SIDE_C4,
            })
            .err() == Some(HkCode::DuplicateHook);
    s.add("闭集守卫：0 号拒绝、4 号重登拒绝", unknown_ok,
          "挂点号闭集 1..=6，重号必须显性拒绝");

    // 11. 互认查询对未登记挂点显性拒绝
    let unreg = proto.is_mutual(99);
    s.add("互认查询：未登记挂点显性 UnknownHook",
          unreg.err() == Some(HkCode::UnknownHook),
          "查询失败必须回闭集码而非静默假");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(1) && agrees_with_linux(11)
        && agrees_with_linux(25) && !agrees_with_linux(0)
        && !agrees_with_linux(999) && linux_neg(22) == -22;
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "errno 表正值与负值逐项可复算");

    // 13. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 14. 文案闭集
    let text_ok = HkCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 码值唯一
    let mut uniq = true;
    let mut m = 0usize;
    while m < HkCode::ALL.len() {
        let mut n = m + 1;
        while n < HkCode::ALL.len() {
            if let (Some(x), Some(y)) = (HkCode::ALL.get(m), HkCode::ALL.get(n)) {
                if x.code() == y.code() {
                    uniq = false;
                }
            }
            n += 1;
        }
        m += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let ch_text = format!(
        "登记 {} / 互认 {} / 拒绝 {} / 注入检出 {}",
        full.registered(),
        full.mutual_count(),
        full.rejected(),
        inj.detected
    );
    s.add_dyn("联签账快照", full.reconcile() && inj.conserved(), &ch_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: u32, owner: &'static str, blocking: &'static str) -> HookRule {
        HookRule { hook_id: id, owner, blocking, single_source: true, written_by: SIDE_C4 }
    }

    #[test]
    fn charter_full_registration() {
        let mut c = HookCharter::new();
        let mut i = 0;
        while i < HookPoint::ALL.len() {
            if let Some(p) = HookPoint::ALL.get(i) {
                assert!(c.register(rule(p.id(), p.c4_owner(), p.c2_blocking())).is_ok());
            }
            i += 1;
        }
        assert_eq!(c.registered(), 6);
        assert_eq!(c.rejected(), 0);
    }

    #[test]
    fn single_side_sign_not_mutual() {
        let mut c = HookCharter::new();
        let _ = c.register(rule(1, "C4.futex", "C2.waitqueue"));
        let _ = c.sign(1, SIDE_C2, 7);
        assert_eq!(c.is_mutual(1), Ok(false));
        let _ = c.sign(1, SIDE_C4, 9);
        assert_eq!(c.is_mutual(1), Ok(true));
    }

    #[test]
    fn cross_write_rejected() {
        let mut c = HookCharter::new();
        assert_eq!(
            c.register(HookRule { hook_id: 3, owner: "C4.futex", blocking: "C2.waitqueue", single_source: true, written_by: SIDE_C2 }).err(),
            Some(HkCode::CrossWrite)
        );
    }

    #[test]
    fn unknown_and_duplicate_rejected() {
        let mut c = HookCharter::new();
        let _ = c.register(rule(2, "C4.futex", "C2.waitqueue"));
        assert_eq!(c.register(rule(2, "C4.futex", "C2.waitqueue")).err(), Some(HkCode::DuplicateHook));
        assert_eq!(c.register(rule(9, "C4.futex", "C2.waitqueue")).err(), Some(HkCode::UnknownHook));
    }

    #[test]
    fn rule_drift_detected() {
        assert!(!rule_consistent(&HookRule {
            hook_id: 5,
            owner: "C4.fake",
            blocking: "C2.blockpoll",
            single_source: true,
            written_by: SIDE_C4,
        }));
        assert!(rule_consistent(&HookRule {
            hook_id: 5,
            owner: "C4.epoll",
            blocking: "C2.blockpoll",
            single_source: true,
            written_by: SIDE_C4,
        }));
    }

    #[test]
    fn mutual_ledger_monotonic() {
        let mut c = HookCharter::new();
        let _ = c.register(rule(6, "C4.pty", "C2.ttyblock"));
        let _ = c.sign(6, SIDE_C2, 1);
        assert_eq!(c.mutual_count(), 0);
        let _ = c.sign(6, SIDE_C4, 1);
        assert_eq!(c.mutual_count(), 1);
        // 重复签名不得重复计互认
        let _ = c.sign(6, SIDE_C4, 2);
        assert_eq!(c.mutual_count(), 1);
    }

    #[test]
    fn violation_matrix_green() {
        let (l, per) = run_violation_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn unregistered_mutual_query_rejected() {
        let c = HookCharter::new();
        assert_eq!(c.is_mutual(3).err(), Some(HkCode::UnknownHook));
    }

    #[test]
    fn linux_table_agrees() {
        assert!(agrees_with_linux(1));
        assert_eq!(linux_neg(38), -38);
        assert!(!agrees_with_linux(0));
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx9381_checks_green() {
        let s = run_unx9381_checks();
        assert!(s.all_passed(), "UNX-F9381 red: {}", s.red_items_text());
    }
}
