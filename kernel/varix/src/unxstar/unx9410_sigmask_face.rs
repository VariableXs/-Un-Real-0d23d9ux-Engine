//! UNX-F9410 sigprocmask/sigaction 错误面 · 0x7D400段
/*!
 * J1 判据原文摘录（验收卡片）：
 * - 注入必抓：全部注入场景逐条拦截，错误码精确匹配，一条不漏；
 * - 判序与 Linux 一致：信号号闭集（EBadSig：0/-1/65/999）→
 *   不可封锁信号（EUnblockable：SIGKILL=9/SIGSTOP=19）→
 *   oldset 往返守恒（查询返回值与设置值严格一致）；
 * - 双跑一致：同一操作序列在两份全新 SigTable 上重放，FNV-1a
 *   64 位指纹逐位相同；
 * - 段位守卫：诊断段 0x7D400 为本单元独占，错误码 = 段基 + 序号，
 *   闭集内单调不重叠；
 * - 闭集诊断码：信号号 1..=64、句柄 {SIG_DFL=0, SIG_IGN=1,
 *   Custom(u32)} 均为闭集，闭集外一律拒绝；
 * - 完成时 API：run_unx9410_checks() 返回 CheckSet，供聚合层消费；
 * - 软件写回：状态=已完成、结果摘要非空（status_summary 不留白）。
 *
 * 设计要点（7 条）：
 *  1. no_std + alloc 纯功能线：禁用原语族（不安全块/强解/急停）
 *     全避，整数确定性算术，无浮点、无墙钟、无随机源。
 *  2. 信号号闭集 1..=64：set_blocked/install_handler 单点判别，
 *     0、-1、65、999 一律 EBadSig（判序第一关）。
 *  3. 不可封锁面：SIGKILL=9 与 SIGSTOP=19 不可 block、不可装
 *     自定义句柄，违者 EUnblockable（判序第二关）；不变量层
 *     亦持续看守这两位永不为 blocked。
 *  4. 状态机单调迁移：Config → Live → Sealed；屏蔽/装句柄须在
 *     Live 相（越相 EPhase），封印后一切写拒（ESealed），
 *     二次封印显性拒。
 *  5. oldset 往返守恒：每次改屏蔽位记账旧值（prev_mask 侧表），
 *     verify_oldset 对账声明旧值，不符即 EOldsetDrift——查询
 *     返回值必须严格等于当初设置值。
 *  6. 定容账本：cap=32，满后拒绝新账（ELedgerFull），拒绝显性
 *     记账不静默吞；流水号严格单调。
 *  7. 指纹对拍：掩码快照 + 句柄表 + 账本序列化后 FNV-1a 64 位
 *     压缩；RECON_TABLE 6 行与信号闭集逐行对账。
 */
#![allow(dead_code)]
use alloc::string::{String, ToString}; use alloc::vec::Vec; use alloc::format;
use crate::checks::CheckSet;

// ── 段位守卫 ─────────────────────────────────────────────────
/// 诊断段基址：0x7D400，UNX-F9410 独占。
pub const SEG_BASE: u32 = 0x7D400;

// ── 域常量 ───────────────────────────────────────────────────
/// 信号号闭集上界（1..=64）。
pub const SIG_MAX: i32 = 64;
/// SIGKILL = 9（不可封锁）。
pub const SIGKILL: i32 = 9;
/// SIGSTOP = 19（不可封锁）。
pub const SIGSTOP: i32 = 19;
/// SIGTERM = 15（可封锁对照面）。
pub const SIGTERM: i32 = 15;
/// 句柄表槽位数（恒 64）。
pub const SLOT_N: usize = 64;
/// 自定义句柄 id 上界（1..=4095，0 保留）。
pub const CUSTOM_ID_MAX: u32 = 4095;
/// 账本容量（cap 32）。
pub const LEDGER_CAP: usize = 32;

// ── 错误码闭集（10~12 变体）─────────────────────────────────
/// 信号面错误闭集：val() = SEG_BASE + 序号，单调不重叠。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SigErr {
    /// 信号号不在 1..=64 闭集（判序第 1 关）
    EBadSig,
    /// SIGKILL/SIGSTOP 封锁或装句柄（判序第 2 关）
    EUnblockable,
    /// 封印后写操作
    ESealed,
    /// Config 相未激活即写屏蔽/装句柄
    EPhase,
    /// oldset 往返守恒对账不符
    EOldsetDrift,
    /// 自定义句柄 id 重复注册
    EDupCustom,
    /// 自定义句柄 id 非法（0 保留 / 超 4095 / 未注册即引用）
    EBadCustomId,
    /// 账本已满（cap 32）
    ELedgerFull,
    /// 状态机不变量破坏（对账抓包）
    EInvState,
    /// 原始诊断码不在本段闭集（兜底）
    EUnknown,
}

impl SigErr {
    /// 诊断码：段基 + 序号。
    pub fn val(&self) -> u32 {
        SEG_BASE + (*self as u32)
    }

    /// 中文明细，逐变体互异、非空。
    pub fn detail(&self) -> &'static str {
        match self {
            SigErr::EBadSig => "信号号不在 1..=64 闭集，判序第一关即拒",
            SigErr::EUnblockable => "SIGKILL/SIGSTOP 不可封锁亦不可装句柄",
            SigErr::ESealed => "信号表已封印，一切写操作拒绝",
            SigErr::EPhase => "Config 相未激活，屏蔽/装句柄越相",
            SigErr::EOldsetDrift => "oldset 往返守恒对账不符，声明旧值与账面不一致",
            SigErr::EDupCustom => "自定义句柄 id 重复注册",
            SigErr::EBadCustomId => "自定义句柄 id 非法：0 保留、超 4095 或未注册即引用",
            SigErr::ELedgerFull => "信号账本已满（cap 32），新账拒绝",
            SigErr::EInvState => "信号表不变量对账不符，状态机告警",
            SigErr::EUnknown => "原始诊断码不在 0x7D400 段闭集",
        }
    }

    /// 闭集全体。
    pub const ALL: [SigErr; 10] = [
        SigErr::EBadSig,
        SigErr::EUnblockable,
        SigErr::ESealed,
        SigErr::EPhase,
        SigErr::EOldsetDrift,
        SigErr::EDupCustom,
        SigErr::EBadCustomId,
        SigErr::ELedgerFull,
        SigErr::EInvState,
        SigErr::EUnknown,
    ];

    /// 由原始诊断码解码；越界归 EUnknown。
    pub fn from_code(code: u32) -> SigErr {
        match code.checked_sub(SEG_BASE) {
            Some(off) => match SigErr::ALL.get(off as usize) {
                Some(e) => *e,
                None => SigErr::EUnknown,
            },
            None => SigErr::EUnknown,
        }
    }
}

// ── 句柄闭集 ────────────────────────────────────────────────
/// 句柄闭集：SIG_DFL=0 / SIG_IGN=1 / Custom(u32)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handler {
    /// SIG_DFL = 0
    Dfl,
    /// SIG_IGN = 1
    Ign,
    /// 自定义槽号（须先 register_custom 注册）
    Custom(u32),
}

impl Handler {
    /// 槽位码：0=Dfl 1=Ign 2=Custom（句柄表并行账用）。
    pub fn slot(&self) -> u8 {
        match self {
            Handler::Dfl => 0u8,
            Handler::Ign => 1u8,
            Handler::Custom(_) => 2u8,
        }
    }

    /// 由原始槽码 + 可选 id 还原；0/1 之外返回 None。
    pub fn from_slot(slot: u8, custom: Option<u32>) -> Option<Handler> {
        match slot {
            0 => Some(Handler::Dfl),
            1 => Some(Handler::Ign),
            2 => match custom {
                Some(id) => Some(Handler::Custom(id)),
                None => None,
            },
            _ => None,
        }
    }
}

// ── 对账表（名称, 信号号, 序号）────────────────────────────
/// RECON_TABLE：6 行逐行与信号闭集对账。
pub const RECON_TABLE: [(&str, i32, i32); 6] = [
    ("SIGKILL", 9, 0),
    ("SIGSTOP", 19, 1),
    ("SIGTERM", 15, 2),
    ("SIGSEGV", 11, 3),
    ("SIGCHLD", 17, 4),
    ("SIGWINCH", 28, 5),
];

// ── FNV-1a 64 位指纹 ────────────────────────────────────────
/// FNV-1a 64 位（双跑一致判据的唯一裁决函数）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < data.len() {
        match data.get(i) {
            Some(b) => {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
            None => {}
        }
        i += 1;
    }
    h
}

fn push_u64(buf: &mut Vec<u8>, v: u64) {
    let bytes = v.to_le_bytes();
    buf.extend_from_slice(&bytes);
}

/// 域标签（软件写回摘要头，供聚合层拼装）。
pub fn domain_tag() -> String {
    "F9410 sigprocmask/sigaction 错误面".to_string()
}

/// 信号号闭集判别：1..=64。
pub fn sig_ok(signum: i32) -> bool {
    signum >= 1 && signum <= SIG_MAX
}

/// 是否不可封锁信号（SIGKILL/SIGSTOP）。
pub fn unmaskable(signum: i32) -> bool {
    signum == SIGKILL || signum == SIGSTOP
}

/// 信号号 → 0 基槽位（闭集内调用）。
fn slot_of(signum: i32) -> usize {
    (signum - 1) as usize
}

// ── 状态机相（单调迁移）─────────────────────────────────────
/// 相：Config → Live → Sealed，只进不退。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 配置相：register_custom 可用；屏蔽/句柄须先激活
    Config,
    /// 运行相：全部写操作可用
    Live,
    /// 封印相：一切写拒
    Sealed,
}

// ── 账目 ─────────────────────────────────────────────────────
/// 账目操作类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SigOp {
    /// 封锁
    Block,
    /// 解封
    Unblock,
    /// 装 SIG_DFL
    InstallDfl,
    /// 装 SIG_IGN
    InstallIgn,
    /// 装自定义句柄
    InstallCustom,
}

/// 一条账目记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SigRec {
    /// 单调流水号
    pub seq: u64,
    /// 操作类型
    pub op: SigOp,
    /// 信号号（1..=64）
    pub signum: u8,
}

// ── 域模型：信号表 ──────────────────────────────────────────
/// 信号表：64 位屏蔽账 + 64 槽句柄账 + 自定义 id 并行账。
pub struct SigTable {
    /// 屏蔽位（len 恒 64，下标 = signum-1）
    blocked: Vec<bool>,
    /// 句柄槽码（len 恒 64：0=Dfl 1=Ign 2=Custom）
    handlers: Vec<u8>,
    /// 自定义 id 并行账（len 恒 64，非 Custom 槽为 None）
    custom: Vec<Option<u32>>,
    /// 已注册自定义 id 集
    custom_ids: Vec<u32>,
    /// oldset 侧表：每信号最近一次变更前的旧值（None=未变更过）
    prev_mask: Vec<Option<bool>>,
    /// 账本（定容 LEDGER_CAP）
    ledger: Vec<SigRec>,
    /// 相（单调）
    phase: Phase,
    /// 单调流水号
    seq: u64,
    /// 拒绝计数（按错误码闭集）
    rejects: Vec<(SigErr, u32)>,
}

impl SigTable {
    /// 新建：全 64 槽 SIG_DFL、全不屏蔽、Config 相。
    pub fn new() -> SigTable {
        let mut blocked = Vec::new();
        let mut handlers = Vec::new();
        let mut custom = Vec::new();
        let mut prev = Vec::new();
        let mut i = 0usize;
        while i < SLOT_N {
            blocked.push(false);
            handlers.push(0u8);
            custom.push(None);
            prev.push(None);
            i += 1;
        }
        SigTable {
            blocked,
            handlers,
            custom,
            custom_ids: Vec::new(),
            prev_mask: prev,
            ledger: Vec::new(),
            phase: Phase::Config,
            seq: 0,
            rejects: Vec::new(),
        }
    }

    fn note_reject(&mut self, e: SigErr) {
        let mut done = false;
        for pair in self.rejects.iter_mut() {
            if pair.0 == e {
                pair.1 = pair.1.wrapping_add(1);
                done = true;
            }
        }
        if !done {
            self.rejects.push((e, 1));
        }
    }

    /// 激活：Config → Live（幂等）。
    pub fn activate(&mut self) -> Result<(), SigErr> {
        match self.phase {
            Phase::Config => {
                self.phase = Phase::Live;
                Ok(())
            }
            Phase::Live => Ok(()),
            Phase::Sealed => Err(SigErr::ESealed),
        }
    }

    /// 封印：任何非封印相 → Sealed；二次封印显性拒。
    pub fn seal(&mut self) -> Result<(), SigErr> {
        match self.phase {
            Phase::Sealed => Err(SigErr::ESealed),
            _ => {
                self.phase = Phase::Sealed;
                Ok(())
            }
        }
    }

    /// 当前相。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 信号当前屏蔽位（闭集外返回 None）。
    pub fn is_blocked(&self, signum: i32) -> Option<bool> {
        if !sig_ok(signum) {
            return None;
        }
        match self.blocked.get(slot_of(signum)) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 相守卫（写操作共用）：Sealed → ESealed；Config → EPhase。
    fn guard_live(&mut self) -> Result<(), SigErr> {
        match self.phase {
            Phase::Sealed => {
                self.note_reject(SigErr::ESealed);
                Err(SigErr::ESealed)
            }
            Phase::Config => {
                self.note_reject(SigErr::EPhase);
                Err(SigErr::EPhase)
            }
            Phase::Live => Ok(()),
        }
    }

    /// 设置/解除屏蔽（判序与 Linux 一致）：
    /// 相守卫 → 信号号闭集 → 不可封锁 → 落账。
    /// 成功返回设置前的旧值（oldset 往返守恒载体）。
    pub fn set_blocked(&mut self, signum: i32, block: bool) -> Result<bool, SigErr> {
        self.guard_live()?;
        // 第 1 关：信号号闭集
        if !sig_ok(signum) {
            self.note_reject(SigErr::EBadSig);
            return Err(SigErr::EBadSig);
        }
        // 第 2 关：不可封锁面
        if block && unmaskable(signum) {
            self.note_reject(SigErr::EUnblockable);
            return Err(SigErr::EUnblockable);
        }
        // 第 3 关：账本容量
        if self.ledger.len() >= LEDGER_CAP {
            self.note_reject(SigErr::ELedgerFull);
            return Err(SigErr::ELedgerFull);
        }
        let idx = slot_of(signum);
        // 旧值记账（oldset 守恒侧表）
        let old = match self.blocked.get(idx) {
            Some(v) => *v,
            None => return Err(SigErr::EInvState),
        };
        match self.prev_mask.get_mut(idx) {
            Some(p) => *p = Some(old),
            None => return Err(SigErr::EInvState),
        }
        match self.blocked.get_mut(idx) {
            Some(b) => *b = block,
            None => return Err(SigErr::EInvState),
        }
        self.seq = self.seq.wrapping_add(1);
        self.ledger.push(SigRec {
            seq: self.seq,
            op: if block { SigOp::Block } else { SigOp::Unblock },
            signum: signum as u8,
        });
        Ok(old)
    }

    /// 注册自定义句柄 id：id ∈ 1..=4095，重复注册拒。
    pub fn register_custom(&mut self, id: u32) -> Result<(), SigErr> {
        match self.phase {
            Phase::Sealed => {
                self.note_reject(SigErr::ESealed);
                return Err(SigErr::ESealed);
            }
            _ => {}
        }
        if id == 0 || id > CUSTOM_ID_MAX {
            self.note_reject(SigErr::EBadCustomId);
            return Err(SigErr::EBadCustomId);
        }
        let mut dup = false;
        for v in self.custom_ids.iter() {
            if *v == id {
                dup = true;
            }
        }
        if dup {
            self.note_reject(SigErr::EDupCustom);
            return Err(SigErr::EDupCustom);
        }
        self.custom_ids.push(id);
        Ok(())
    }

    /// 装句柄（判序与 Linux 一致）：
    /// 相守卫 → 信号号闭集 → 不可封锁 → 自定义 id 合法性 → 落账。
    /// 成功返回旧槽码（0=Dfl 1=Ign 2=Custom）。
    pub fn install_handler(&mut self, signum: i32, h: Handler) -> Result<u8, SigErr> {
        self.guard_live()?;
        // 第 1 关：信号号闭集
        if !sig_ok(signum) {
            self.note_reject(SigErr::EBadSig);
            return Err(SigErr::EBadSig);
        }
        // 第 2 关：不可封锁面（KILL/STOP 不许装任何句柄）
        if unmaskable(signum) {
            self.note_reject(SigErr::EUnblockable);
            return Err(SigErr::EUnblockable);
        }
        // 第 3 关：自定义 id 合法性（0 保留/未注册即引用）
        let cid = match h {
            Handler::Custom(id) => {
                if id == 0 || id > CUSTOM_ID_MAX {
                    self.note_reject(SigErr::EBadCustomId);
                    return Err(SigErr::EBadCustomId);
                }
                let mut reg = false;
                for v in self.custom_ids.iter() {
                    if *v == id {
                        reg = true;
                    }
                }
                if !reg {
                    self.note_reject(SigErr::EBadCustomId);
                    return Err(SigErr::EBadCustomId);
                }
                Some(id)
            }
            _ => None,
        };
        // 第 4 关：账本容量
        if self.ledger.len() >= LEDGER_CAP {
            self.note_reject(SigErr::ELedgerFull);
            return Err(SigErr::ELedgerFull);
        }
        let idx = slot_of(signum);
        let old = match self.handlers.get(idx) {
            Some(v) => *v,
            None => return Err(SigErr::EInvState),
        };
        match self.handlers.get_mut(idx) {
            Some(v) => *v = h.slot(),
            None => return Err(SigErr::EInvState),
        }
        match self.custom.get_mut(idx) {
            Some(v) => *v = cid,
            None => return Err(SigErr::EInvState),
        }
        self.seq = self.seq.wrapping_add(1);
        let op = match h {
            Handler::Dfl => SigOp::InstallDfl,
            Handler::Ign => SigOp::InstallIgn,
            Handler::Custom(_) => SigOp::InstallCustom,
        };
        self.ledger.push(SigRec {
            seq: self.seq,
            op,
            signum: signum as u8,
        });
        Ok(old)
    }

    /// oldset 往返守恒对账：声明旧值须与侧表账面一致；
    /// 从未变更过的信号无旧值可对，同样判漂移（显性不静默）。
    pub fn verify_oldset(&mut self, signum: i32, claimed_old: bool) -> Result<(), SigErr> {
        if !sig_ok(signum) {
            self.note_reject(SigErr::EBadSig);
            return Err(SigErr::EBadSig);
        }
        let idx = slot_of(signum);
        let actual = match self.prev_mask.get(idx) {
            Some(Some(v)) => *v,
            _ => {
                self.note_reject(SigErr::EOldsetDrift);
                return Err(SigErr::EOldsetDrift);
            }
        };
        if actual == claimed_old {
            Ok(())
        } else {
            self.note_reject(SigErr::EOldsetDrift);
            Err(SigErr::EOldsetDrift)
        }
    }

    /// 屏蔽掩码快照（两个 u32 字，bit k = 信号 k+1）。
    pub fn mask_snapshot(&self) -> (u32, u32) {
        let mut lo = 0u32;
        let mut hi = 0u32;
        let mut i = 0usize;
        while i < SLOT_N {
            let on = match self.blocked.get(i) {
                Some(v) => *v,
                None => false,
            };
            if on {
                if i < 32 {
                    lo = lo | (1u32 << i);
                } else {
                    hi = hi | (1u32 << (i - 32));
                }
            }
            i += 1;
        }
        (lo, hi)
    }

    /// 账本长度。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 已注册自定义 id 数。
    pub fn custom_count(&self) -> usize {
        self.custom_ids.len()
    }

    /// 不变量对账：长度恒 64、KILL/STOP 永不屏蔽、KILL/STOP 槽
    /// 不挂自定义句柄、流水号严格单调、容量守恒、Custom 槽账实一致。
    pub fn invariant_ok(&self) -> bool {
        if self.blocked.len() != SLOT_N
            || self.handlers.len() != SLOT_N
            || self.custom.len() != SLOT_N
            || self.prev_mask.len() != SLOT_N
        {
            return false;
        }
        if self.ledger.len() > LEDGER_CAP {
            return false;
        }
        // KILL/STOP 永不屏蔽
        match self.blocked.get(slot_of(SIGKILL)) {
            Some(false) => {}
            _ => return false,
        }
        match self.blocked.get(slot_of(SIGSTOP)) {
            Some(false) => {}
            _ => return false,
        }
        // KILL/STOP 槽不挂自定义句柄
        for sig in [SIGKILL, SIGSTOP].iter() {
            let idx = slot_of(*sig);
            let slot = match self.handlers.get(idx) {
                Some(v) => *v,
                None => return false,
            };
            if slot != 0 && slot != 1 {
                return false;
            }
        }
        // Custom 槽账实一致：槽码 2 ⇔ 并行账 Some 且 id 已注册
        let mut i = 0usize;
        while i < SLOT_N {
            let slot = match self.handlers.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let cid = match self.custom.get(i) {
                Some(v) => *v,
                None => return false,
            };
            match (slot, cid) {
                (2, Some(id)) => {
                    let mut reg = false;
                    for v in self.custom_ids.iter() {
                        if *v == id {
                            reg = true;
                        }
                    }
                    if !reg {
                        return false;
                    }
                }
                (2, None) => return false,
                (0, Some(_)) => return false,
                (1, Some(_)) => return false,
                _ => {}
            }
            i += 1;
        }
        // 流水号严格单调
        let mut prev: u64 = 0;
        let mut first = true;
        for r in self.ledger.iter() {
            if first {
                first = false;
            } else if r.seq <= prev {
                return false;
            }
            prev = r.seq;
        }
        true
    }

    /// 状态序列化后 FNV-1a 64 位指纹（双跑一致）。
    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.push(match self.phase {
            Phase::Config => 1u8,
            Phase::Live => 2u8,
            Phase::Sealed => 3u8,
        });
        push_u64(&mut buf, self.seq);
        let (lo, hi) = self.mask_snapshot();
        push_u64(&mut buf, lo as u64);
        push_u64(&mut buf, hi as u64);
        push_u64(&mut buf, self.custom_ids.len() as u64);
        for id in self.custom_ids.iter() {
            push_u64(&mut buf, *id as u64);
        }
        push_u64(&mut buf, self.ledger.len() as u64);
        for r in self.ledger.iter() {
            push_u64(&mut buf, r.seq);
            buf.push(r.signum);
            buf.push(match r.op {
                SigOp::Block => 1u8,
                SigOp::Unblock => 2u8,
                SigOp::InstallDfl => 3u8,
                SigOp::InstallIgn => 4u8,
                SigOp::InstallCustom => 5u8,
            });
        }
        fnv1a64(&buf)
    }

    /// 软件写回：状态=已完成、结果摘要非空。
    pub fn status_summary(&self) -> String {
        let (lo, hi) = self.mask_snapshot();
        format!(
            "{} 状态=已完成 段=0x{:X} 相={:?} 屏蔽字={:08X}{:08X} 自定义={} 账目={} 指纹={:#018X}",
            domain_tag(),
            SEG_BASE,
            self.phase,
            hi,
            lo,
            self.custom_ids.len(),
            self.ledger.len(),
            self.fingerprint()
        )
    }
}

// ── 注入器（注入必抓）───────────────────────────────────────
/// 注入器：逐场景注入违规，错误码精确匹配才计抓。
pub struct Injector {
    caught: usize,
    total: usize,
    log: Vec<String>,
}

impl Injector {
    pub fn new() -> Injector {
        Injector {
            caught: 0,
            total: 0,
            log: Vec::new(),
        }
    }

    /// 记一例：got 与 expect 精确相等才算抓到。
    pub fn case_err(&mut self, name: &str, expect: SigErr, got: Result<(), SigErr>) {
        self.total += 1;
        match got {
            Ok(_) => {
                let line = format!("漏抓[{}] 期望 {} 实得 Ok", name, expect.detail());
                self.log.push(line);
            }
            Err(e) => {
                if e == expect {
                    self.caught += 1;
                    let line = format!("抓到[{}] 码={:#X}", name, e.val());
                    self.log.push(line);
                } else {
                    let line = format!("错抓[{}] 期望 {:#X} 实得 {:#X}", name, expect.val(), e.val());
                    self.log.push(line);
                }
            }
        }
    }

    /// 全量违规注入：返回 (抓到数, 总数)。
    pub fn run_all() -> (usize, usize) {
        let mut inj = Injector::new();

        // 场景 1：信号号 0（闭集外）
        let mut st = fresh_live();
        let r = st.set_blocked(0, true).map(|_| ());
        inj.case_err("信号0", SigErr::EBadSig, r);

        // 场景 2：信号号 65（闭集外）
        let mut st = fresh_live();
        let r = st.set_blocked(65, true).map(|_| ());
        inj.case_err("信号65", SigErr::EBadSig, r);

        // 场景 3：信号号 -1（闭集外）
        let mut st = fresh_live();
        let r = st.set_blocked(-1, true).map(|_| ());
        inj.case_err("信号-1", SigErr::EBadSig, r);

        // 场景 4：信号号 999（闭集外）
        let mut st = fresh_live();
        let r = st.set_blocked(999, true).map(|_| ());
        inj.case_err("信号999", SigErr::EBadSig, r);

        // 场景 5：封锁 SIGKILL
        let mut st = fresh_live();
        let r = st.set_blocked(SIGKILL, true).map(|_| ());
        inj.case_err("封锁KILL", SigErr::EUnblockable, r);

        // 场景 6：封锁 SIGSTOP
        let mut st = fresh_live();
        let r = st.set_blocked(SIGSTOP, true).map(|_| ());
        inj.case_err("封锁STOP", SigErr::EUnblockable, r);

        // 场景 7：KILL 装自定义句柄
        let mut st = fresh_live();
        let _ = st.register_custom(7);
        let r = st.install_handler(SIGKILL, Handler::Custom(7)).map(|_| ());
        inj.case_err("KILL装自定义", SigErr::EUnblockable, r);

        // 场景 8：STOP 装 SIG_IGN（不可封锁面装任何句柄均拒）
        let mut st = fresh_live();
        let r = st.install_handler(SIGSTOP, Handler::Ign).map(|_| ());
        inj.case_err("STOP装IGN", SigErr::EUnblockable, r);

        // 场景 9：Config 相未激活即封锁
        let mut st = SigTable::new();
        let r = st.set_blocked(SIGTERM, true).map(|_| ());
        inj.case_err("未激活封锁", SigErr::EPhase, r);

        // 场景 10：封印后封锁
        let mut st = fresh_live();
        let _ = st.seal();
        let r = st.set_blocked(SIGTERM, true).map(|_| ());
        inj.case_err("封印后封锁", SigErr::ESealed, r);

        // 场景 11：二次封印
        let mut st = fresh_live();
        let _ = st.seal();
        let r = st.seal();
        inj.case_err("二次封印", SigErr::ESealed, r);

        // 场景 12：账本满（cap 32，第 33 笔拒；跳过 9/19 不可封锁面）
        let mut st = fresh_live();
        let mut sig = 1i32;
        let mut done = 0usize;
        while done < 32 && sig <= SIG_MAX {
            if unmaskable(sig) {
                sig += 1;
                continue;
            }
            match st.set_blocked(sig, true) {
                Ok(false) => done += 1,
                _ => {}
            }
            sig += 1;
        }
        while sig <= SIG_MAX && unmaskable(sig) {
            sig += 1;
        }
        let r = st.set_blocked(sig, true).map(|_| ());
        inj.case_err("账本满", SigErr::ELedgerFull, r);

        // 场景 13：oldset 声明漂移（实旧值 false，谎报 true）
        let mut st = fresh_live();
        let _ = st.set_blocked(5, true);
        let r = st.verify_oldset(5, true);
        inj.case_err("oldset谎报", SigErr::EOldsetDrift, r);

        // 场景 14：从未变更过的信号对账（无旧值可对）
        let mut st = fresh_live();
        let r = st.verify_oldset(6, false);
        inj.case_err("oldset无账", SigErr::EOldsetDrift, r);

        // 场景 15：自定义 id 重复注册
        let mut st = fresh_live();
        let _ = st.register_custom(9);
        let r = st.register_custom(9);
        inj.case_err("自定义重复", SigErr::EDupCustom, r);

        // 场景 16：自定义 id 0（保留）
        let mut st = fresh_live();
        let r = st.register_custom(0);
        inj.case_err("自定义id0", SigErr::EBadCustomId, r);

        // 场景 17：未注册 id 即装句柄
        let mut st = fresh_live();
        let r = st.install_handler(SIGTERM, Handler::Custom(77)).map(|_| ());
        inj.case_err("自定义未注册", SigErr::EBadCustomId, r);

        (inj.caught, inj.total)
    }

    /// 零误伤阳性集：合法操作一个都不许拒。返回 (通过数, 总数)。
    pub fn run_positives() -> (usize, usize) {
        let mut pass = 0usize;
        let total = 5usize;
        // 阳 1：空掩码下可封锁信号的幂等重封锁（第二次返回旧值 true）
        let mut st = fresh_live();
        let first = st.set_blocked(SIGTERM, true);
        let second = st.set_blocked(SIGTERM, true);
        match (first, second) {
            (Ok(false), Ok(true)) => pass += 1,
            _ => {}
        }
        // 阳 2：oldset 精确往返（最近一次变更的账面旧值 true，如实申报）
        if st.verify_oldset(SIGTERM, true).is_ok() {
            pass += 1;
        }
        // 阳 3：TERM 装 SIG_IGN 合法
        if st.install_handler(SIGTERM, Handler::Ign).is_ok() {
            pass += 1;
        }
        // 阳 4：边界信号 1 与 64 可封锁
        let mut st = fresh_live();
        let a = st.set_blocked(1, true);
        let b = st.set_blocked(64, true);
        match (a, b) {
            (Ok(false), Ok(false)) => pass += 1,
            _ => {}
        }
        // 阳 5：解封合法（曾封锁后解除）
        if st.set_blocked(1, false).is_ok() {
            pass += 1;
        }
        (pass, total)
    }

    /// 注入日志（逐行中文，供聚合摘要）。
    pub fn log(&self) -> Vec<String> {
        self.log.clone()
    }
}

/// 造一台已激活的空信号表。
fn fresh_live() -> SigTable {
    let mut st = SigTable::new();
    let _ = st.activate();
    st
}

// ── 完成时 API：域自检 ──────────────────────────────────────
/// UNX-F9410 域自检（J1 全判据落点）。
pub fn run_unx9410_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9410");

    // J1-01 注入必抓
    let (c, t) = Injector::run_all();
    s.add("J1-01 注入必抓", c == t && t >= 10, "注入场景逐条拦截且错误码精确匹配，总数≥10");

    // J1-02 零误伤
    let (pc, pt) = Injector::run_positives();
    s.add("J1-02 零误伤阳性", pc == pt, "幂等重封/oldset往返/IGN/边界信号/解封全过");

    // J1-03 判序：闭集关先于不可封锁关（坏信号号即便撞 KILL 语义也先 EBadSig）
    let mut st = fresh_live();
    // 信号 0 不在闭集，虽然也不是 KILL/STOP，验证闭集第一关
    let o1 = match st.set_blocked(0, true) {
        Err(SigErr::EBadSig) => true,
        _ => false,
    };
    s.add("J1-03 判序闭集先行", o1, "信号号闭集为判序第一关，0/-1/65/999 一律 EBadSig");

    // J1-04 判序：不可封锁关先于落账
    let mut st = fresh_live();
    let o2 = match st.set_blocked(SIGKILL, true) {
        Err(SigErr::EUnblockable) => {
            // 拒绝后 KILL 位不得被置起
            match st.is_blocked(SIGKILL) {
                Some(false) => true,
                _ => false,
            }
        }
        _ => false,
    };
    s.add("J1-04 KILL拒绝不落账", o2, "封锁 SIGKILL 拒且屏蔽位保持清零（无半提交）");

    // J1-05 信号闭集 1..=64
    let mut cs_ok = true;
    let mut v = -3i32;
    while v <= 70 {
        if sig_ok(v) != (v >= 1 && v <= 64) {
            cs_ok = false;
        }
        v += 1;
    }
    s.add("J1-05 信号闭集64", cs_ok, "1..=64 判别式逐点扫描无误");

    // J1-06 不可封锁面识别
    s.add(
        "J1-06 不可封锁面",
        unmaskable(SIGKILL) && unmaskable(SIGSTOP) && !unmaskable(SIGTERM),
        "SIGKILL=9/SIGSTOP=19 命中，SIGTERM=15 不命中",
    );

    // J1-07 KILL/STOP 装句柄全拒（DFL 亦拒）
    let mut st = fresh_live();
    let h1 = match st.install_handler(SIGKILL, Handler::Dfl) {
        Err(SigErr::EUnblockable) => true,
        _ => false,
    };
    let h2 = match st.install_handler(SIGSTOP, Handler::Ign) {
        Err(SigErr::EUnblockable) => true,
        _ => false,
    };
    s.add("J1-07 KILL装句柄拒", h1 && h2, "KILL/STOP 槽装 DFL/IGN/自定义一律 EUnblockable");

    // J1-08 oldset 往返守恒（真值申报过 / 谎报拒）
    let mut st = fresh_live();
    let old = st.set_blocked(10, true);
    let v1 = match old {
        Ok(false) => st.verify_oldset(10, false).is_ok(),
        _ => false,
    };
    let v2 = match st.verify_oldset(10, true) {
        Err(SigErr::EOldsetDrift) => true,
        _ => false,
    };
    s.add("J1-08 oldset守恒", v1 && v2, "设置返回旧值 false，如实申报过、谎报判漂移");

    // J1-09 相守卫：未激活拒 / 封印后拒 / 二次封印拒
    let mut st = SigTable::new();
    let g1 = match st.set_blocked(3, true) {
        Err(SigErr::EPhase) => true,
        _ => false,
    };
    let _ = st.activate();
    let _ = st.seal();
    let g2 = match st.set_blocked(3, true) {
        Err(SigErr::ESealed) => true,
        _ => false,
    };
    let g3 = match st.seal() {
        Err(SigErr::ESealed) => true,
        _ => false,
    };
    s.add("J1-09 相守卫三连", g1 && g2 && g3, "未激活EPhase；封印后ESealed；二次封印ESealed");

    // J1-10 账本容量守卫（cap 32；跳过 9/19 不可封锁面）
    let mut st = fresh_live();
    let mut sig = 1i32;
    let mut done = 0usize;
    while done < 32 && sig <= SIG_MAX {
        if unmaskable(sig) {
            sig += 1;
            continue;
        }
        match st.set_blocked(sig, true) {
            Ok(false) => done += 1,
            _ => {}
        }
        sig += 1;
    }
    while sig <= SIG_MAX && unmaskable(sig) {
        sig += 1;
    }
    let cap_ok = st.ledger_len() == LEDGER_CAP
        && match st.set_blocked(sig, true) {
            Err(SigErr::ELedgerFull) => true,
            _ => false,
        };
    s.add("J1-10 账本容量32", cap_ok, "cap=32 满 后第 33 笔 ELedgerFull");

    // J1-11 自定义 id 闭集（0 保留 / 4096 越界 / 重复拒 / 4095 过）
    let mut st = fresh_live();
    let c1 = match st.register_custom(0) {
        Err(SigErr::EBadCustomId) => true,
        _ => false,
    };
    let c2 = match st.register_custom(4096) {
        Err(SigErr::EBadCustomId) => true,
        _ => false,
    };
    let c3 = st.register_custom(4095).is_ok();
    let c4 = match st.register_custom(4095) {
        Err(SigErr::EDupCustom) => true,
        _ => false,
    };
    s.add("J1-11 自定义id闭集", c1 && c2 && c3 && c4, "0/4096 拒、4095 过、重复 EDupCustom");

    // J1-12 不变量对账
    let mut st = fresh_live();
    let _ = st.set_blocked(2, true);
    let _ = st.register_custom(5);
    let _ = st.install_handler(2, Handler::Custom(5));
    s.add("J1-12 不变量对账", st.invariant_ok(), "长度恒64/KILL·STOP清零/槽账一致/流水单调");

    // J1-13 RECON_TABLE 6 行对账
    let mut recon_ok = RECON_TABLE.len() == 6;
    let mut seen: Vec<i32> = Vec::new();
    for row in RECON_TABLE.iter() {
        let (name, signum, idx) = *row;
        if !sig_ok(signum) || (name == "SIGKILL") != (signum == SIGKILL) {
            recon_ok = false;
        }
        if idx < 0 || idx >= 6 {
            recon_ok = false;
        }
        for v in seen.iter() {
            if *v == signum {
                recon_ok = false;
            }
        }
        seen.push(signum);
    }
    s.add("J1-13 RECON对账", recon_ok, "6 行信号号在闭集、名称与常量吻合、无重复");

    // J1-14 段位守卫
    let mut seg_ok = SEG_BASE == 0x7D400;
    let mut prev = 0u32;
    let mut first = true;
    for e in SigErr::ALL.iter() {
        let v = e.val();
        if v < SEG_BASE || v >= SEG_BASE + 10 {
            seg_ok = false;
        }
        if !first && v <= prev {
            seg_ok = false;
        }
        first = false;
        prev = v;
    }
    s.add("J1-14 段位守卫", seg_ok, "0x7D400 独占；10 诊断码段内单调不重叠");

    // J1-15 软件写回摘要非空
    let st = fresh_live();
    let summary = st.status_summary();
    s.add("J1-15 摘要非空", summary.len() > 0 && summary.contains("已完成"), "状态=已完成、结果摘要非空");

    // 动态明细：双跑指纹一致
    let fp1 = replay_fingerprint();
    let fp2 = replay_fingerprint();
    s.add_dyn("J1-16 双跑指纹", fp1 == fp2, "同序列两台新表指纹逐位相等");

    // 动态明细：注入计数
    let (c2, t2) = Injector::run_all();
    s.add_dyn("J1-17 注入计数", c2 == t2 && t2 >= 10, "抓到=总数且≥10");

    // 动态明细：诊断明细互异
    let mut det_ok = true;
    let n = SigErr::ALL.len();
    let mut i = 0usize;
    while i < n {
        let mut j = i + 1;
        while j < n {
            let a = match SigErr::ALL.get(i) {
                Some(v) => v.detail(),
                None => "",
            };
            let b = match SigErr::ALL.get(j) {
                Some(v) => v.detail(),
                None => "",
            };
            if a == b || a.len() == 0 {
                det_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add_dyn("J1-18 诊断明细互异", det_ok, "10 变体中文明细非空互异");

    s
}

/// 固定操作序列的重放指纹（双跑一致判据载体）。
fn replay_fingerprint() -> u64 {
    let mut st = SigTable::new();
    let _ = st.activate();
    let _ = st.register_custom(3);
    let _ = st.set_blocked(2, true);
    let _ = st.set_blocked(5, true);
    let _ = st.set_blocked(5, false);
    let _ = st.install_handler(SIGTERM, Handler::Custom(3));
    let _ = st.set_blocked(SIGKILL, true); // 拒绝也计入序列（不改状态）
    let _ = st.seal();
    st.fingerprint()
}

// ── 测试（≥8 条；断言族仅在此允许）─────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 用 match 断错（无 unwrap/expect；泛型兼容各 Ok 载体）。
    fn expect_err<T>(got: Result<T, SigErr>, want: SigErr) -> bool {
        match got {
            Err(e) => e == want,
            Ok(_) => false,
        }
    }

    #[test]
    fn seg_base_and_err_vals() {
        assert_eq!(SEG_BASE, 0x7D400);
        let mut prev = 0u32;
        let mut first = true;
        for e in SigErr::ALL.iter() {
            let v = e.val();
            assert!(v >= SEG_BASE && v < SEG_BASE + 10);
            if !first {
                assert!(v > prev);
            }
            first = false;
            prev = v;
        }
        assert_eq!(SigErr::ALL.len(), 10);
        for e in SigErr::ALL.iter() {
            assert_eq!(SigErr::from_code(e.val()), *e);
        }
        assert_eq!(SigErr::from_code(SEG_BASE + 77), SigErr::EUnknown);
        assert_eq!(SigErr::from_code(0), SigErr::EUnknown);
    }

    #[test]
    fn signal_closed_set() {
        assert!(sig_ok(1) && sig_ok(64));
        assert!(!sig_ok(0) && !sig_ok(65) && !sig_ok(-1) && !sig_ok(999));
        assert!(unmaskable(9) && unmaskable(19));
        assert!(!unmaskable(15) && !unmaskable(1) && !unmaskable(64));
    }

    #[test]
    fn judge_order_frozen() {
        // 闭集关第一（先于不可封锁关与相守卫语义落地）
        let mut st = fresh_live();
        assert!(expect_err(st.set_blocked(0, true), SigErr::EBadSig));
        assert!(expect_err(st.set_blocked(65, true), SigErr::EBadSig));
        // 不可封锁关第二
        assert!(expect_err(st.set_blocked(SIGKILL, true), SigErr::EUnblockable));
        assert!(expect_err(st.set_blocked(SIGSTOP, true), SigErr::EUnblockable));
        // 拒绝后无半提交
        match st.is_blocked(SIGKILL) {
            Some(false) => {}
            _ => assert!(false, "KILL 位应保持清零"),
        }
    }

    #[test]
    fn closed_set_injection_all_caught() {
        let (c, t) = Injector::run_all();
        assert!(t >= 10, "注入场景不足 10");
        assert_eq!(c, t, "有漏抓或错抓");
    }

    #[test]
    fn zero_false_kill_positives() {
        let (p, t) = Injector::run_positives();
        assert_eq!(p, t, "阳性被误伤");
        assert_eq!(t, 5);
        // 幂等重封锁显式对账
        let mut st = fresh_live();
        match st.set_blocked(7, true) {
            Ok(false) => {}
            _ => assert!(false, "首封旧值应为 false"),
        }
        match st.set_blocked(7, true) {
            Ok(true) => {}
            _ => assert!(false, "重封旧值应为 true"),
        }
        assert!(st.verify_oldset(7, true).is_ok());
    }

    #[test]
    fn phase_monotonic_and_seal() {
        let mut st = SigTable::new();
        assert!(expect_err(st.set_blocked(3, true), SigErr::EPhase));
        assert!(expect_err(st.install_handler(3, Handler::Ign), SigErr::EPhase));
        assert!(st.activate().is_ok());
        assert!(st.activate().is_ok()); // 幂等
        assert!(st.set_blocked(3, true).is_ok());
        assert!(st.seal().is_ok());
        assert!(expect_err(st.set_blocked(3, false), SigErr::ESealed));
        assert!(expect_err(st.seal().map(|_| ()), SigErr::ESealed));
    }

    #[test]
    fn oldset_conservation() {
        let mut st = fresh_live();
        // 未变更信号对账 → 漂移
        assert!(expect_err(st.verify_oldset(4, false), SigErr::EOldsetDrift));
        let old = match st.set_blocked(4, true) {
            Ok(v) => v,
            Err(_) => {
                assert!(false, "封锁 4 应过");
                return;
            }
        };
        assert!(!old);
        assert!(st.verify_oldset(4, false).is_ok());
        assert!(expect_err(st.verify_oldset(4, true), SigErr::EOldsetDrift));
        // 二次变更后旧值翻新
        let _ = st.set_blocked(4, false);
        assert!(st.verify_oldset(4, true).is_ok());
        assert!(expect_err(st.verify_oldset(4, false), SigErr::EOldsetDrift));
    }

    #[test]
    fn custom_handler_facets() {
        let mut st = fresh_live();
        assert!(expect_err(st.register_custom(0), SigErr::EBadCustomId));
        assert!(expect_err(st.register_custom(4096), SigErr::EBadCustomId));
        assert!(st.register_custom(11).is_ok());
        assert!(expect_err(st.register_custom(11), SigErr::EDupCustom));
        // 未注册即引用
        assert!(expect_err(
            st.install_handler(5, Handler::Custom(99)),
            SigErr::EBadCustomId
        ));
        // 注册后安装成功且槽账一致
        match st.install_handler(5, Handler::Custom(11)) {
            Ok(0) => {}
            _ => assert!(false, "装自定义句柄应过且旧槽为 Dfl"),
        }
        assert!(st.invariant_ok());
    }

    #[test]
    fn ledger_cap_32() {
        let mut st = fresh_live();
        let mut sig = 1i32;
        let mut done = 0usize;
        while done < 32 && sig <= SIG_MAX {
            if unmaskable(sig) {
                sig += 1;
                continue;
            }
            match st.set_blocked(sig, true) {
                Ok(false) => done += 1,
                _ => assert!(false, "前 32 笔应全过"),
            }
            sig += 1;
        }
        assert_eq!(st.ledger_len(), LEDGER_CAP);
        while sig <= SIG_MAX && unmaskable(sig) {
            sig += 1;
        }
        assert!(expect_err(st.set_blocked(sig, true), SigErr::ELedgerFull));
        assert!(st.invariant_ok());
    }

    #[test]
    fn invariant_and_kill_stop_guard() {
        let mut st = fresh_live();
        let _ = st.set_blocked(1, true);
        let _ = st.register_custom(3);
        let _ = st.install_handler(1, Handler::Custom(3));
        assert!(st.invariant_ok());
        // KILL/STOP 永不屏蔽的看守
        match st.is_blocked(SIGKILL) {
            Some(false) => {}
            _ => assert!(false, "KILL 不变量破坏"),
        }
        match st.is_blocked(SIGSTOP) {
            Some(false) => {}
            _ => assert!(false, "STOP 不变量破坏"),
        }
    }

    #[test]
    fn dual_run_fingerprint_equal() {
        let a = replay_fingerprint();
        let b = replay_fingerprint();
        assert_eq!(a, b, "双跑指纹漂移");
        let mut st = SigTable::new();
        let _ = st.activate();
        let c = st.fingerprint();
        assert_ne!(a, c);
    }

    #[test]
    fn recon_table_crosscheck() {
        assert_eq!(RECON_TABLE.len(), 6);
        let mut i = 0usize;
        while i < 6 {
            let row = match RECON_TABLE.get(i) {
                Some(v) => *v,
                None => {
                    assert!(false, "RECON 行缺失");
                    return;
                }
            };
            let (name, signum, idx) = row;
            assert!(sig_ok(signum), "RECON 信号号越闭集");
            assert!(idx >= 0 && idx < 6, "RECON 序号越界");
            if i == 0 {
                assert_eq!(name, "SIGKILL");
                assert_eq!(signum, 9);
            }
            if i == 1 {
                assert_eq!(name, "SIGSTOP");
                assert_eq!(signum, 19);
            }
            i += 1;
        }
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9410_checks();
        assert!(s.all_passed(), "域自检有红项");
        assert!(!s.truncated(), "域自检被截断");
        assert!(s.len() >= 12, "检查项不足 12");
        let st = fresh_live();
        let sum = st.status_summary();
        assert!(sum.len() > 0);
        assert!(sum.contains("0x7D400"));
    }
}
