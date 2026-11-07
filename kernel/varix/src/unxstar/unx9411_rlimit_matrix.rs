//! UNX-F9411 setrlimit 错误矩阵 · 0x7D440段
/*!
 * J1 判据原文摘录（验收卡片）：
 * - 注入必抓：全部注入场景逐条拦截，错误码精确匹配，一条不漏；
 * - 判序与 Linux 一致：资源闭集（EBadRes：17/-1/10 缺口）→
 *   new cur ≤ new max（ECurGtMax）→ 无特权抬高 cur 越过旧硬限
 *   或抬高硬限（ERaiseHard）→ RLIM_INFINITY（u64::MAX）哨兵
 *   合法放行（零误伤）；
 * - 双跑一致：同一操作序列在两份全新 RlimTable 上重放，FNV-1a
 *   64 位指纹逐位相同；
 * - 段位守卫：诊断段 0x7D440 为本单元独占，错误码 = 段基 + 序号，
 *   闭集内单调不重叠；
 * - 闭集诊断码：资源闭集恰 13 项（CPU=0..RTPRIO=16，10 为缺口），
 *   闭集外一律拒绝；
 * - 完成时 API：run_unx9411_checks() 返回 CheckSet，供聚合层消费；
 * - 软件写回：状态=已完成、结果摘要非空（status_summary 不留白）。
 *
 * 设计要点（7 条）：
 *  1. no_std + alloc 纯功能线：禁用原语族（不安全块/强解/急停）
 *     全避，整数确定性算术，无浮点、无墙钟、无随机源。
 *  2. 资源闭集恰 13 项：RLIMIT_CPU=0 .. RLIMIT_RTPRIO=16，
 *     原始 i32 经 from_raw 单点判别；10 为闭集缺口（拒），
 *     17/-1 亦拒（判序第一关）。
 *  3. 判序冻结：闭集 → cur≤max → 特权关（抬硬限/越旧硬限）→
 *     硬限 ratchet 不得低于现行软限（拒 ECurGtMax）→ 落账；
 *     前面一关不过即返回，绝不做后面的检查。
 *  4. 状态机单调迁移：Config → Live → Sealed；setrlimit 须在
 *     Live 相（越相 EPhase），封印后一切写拒（ESealed），
 *     二次封印显性拒；特权只授不收（单调）。
 *  5. ratchet 纪律：max 可自由下调（记录 lowering 计数），
 *     但低于现行软限即拒；每次 setrlimit 后恒保持 cur ≤ max；
 *     INF 哨兵 cur 配有限 max 拒（ECurGtMax）。
 *  6. 定容账本：cap=32，满后拒绝新账（ELedgerFull），拒绝显性
 *     记账不静默吞；lowered 计数按资源逐项单调，可重放对账。
 *  7. 指纹对拍：13 行限值 + 账本序列化后 FNV-1a 64 位压缩；
 *     RECON_TABLE 13 行与资源闭集逐行对账；getrlimit 声明值
 *     对账漂移（ERowDrift）与 lowering 计数漂移（ECountDrift）
 *     两条对账通道齐备。
 */
#![allow(dead_code)]
use alloc::string::{String, ToString}; use alloc::vec::Vec; use alloc::format;
use crate::checks::CheckSet;

// ── 段位守卫 ─────────────────────────────────────────────────
/// 诊断段基址：0x7D440，UNX-F9411 独占。
pub const SEG_BASE: u32 = 0x7D440;

// ── 域常量 ───────────────────────────────────────────────────
/// RLIM_INFINITY 哨兵。
pub const RLIM_INFINITY: u64 = u64::MAX;
/// 资源闭集成员数（恰 13）。
pub const RES_N: usize = 13;
/// 账本容量（cap 32）。
pub const LEDGER_CAP: usize = 32;
/// NOFILE 经典软限对照值。
pub const NOFILE_DEFAULT: u64 = 1024;
/// 栈默认软限对照值（8 MiB）。
pub const STACK_DEFAULT: u64 = 8 * 1024 * 1024;

// ── 错误码闭集（10~12 变体）─────────────────────────────────
/// setrlimit 错误矩阵闭集：val() = SEG_BASE + 序号，单调不重叠。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RlimErr {
    /// 资源号不在 13 项闭集（判序第 1 关）
    EBadRes,
    /// new cur > new max（含 INF cur 配有限 max）
    ECurGtMax,
    /// 无特权抬高 cur 越过旧硬限 / 抬高硬限
    ERaiseHard,
    /// getrlimit 声明值与账面漂移
    ERowDrift,
    /// lowering 计数声明与账面漂移
    ECountDrift,
    /// 封印后写操作
    ESealed,
    /// Config 相未激活即 setrlimit
    EPhase,
    /// 账本已满（cap 32）
    ELedgerFull,
    /// 状态机不变量破坏（对账抓包）
    EInvState,
    /// 原始诊断码不在本段闭集（兜底）
    EUnknown,
}

impl RlimErr {
    /// 诊断码：段基 + 序号。
    pub fn val(&self) -> u32 {
        SEG_BASE + (*self as u32)
    }

    /// 中文明细，逐变体互异、非空。
    pub fn detail(&self) -> &'static str {
        match self {
            RlimErr::EBadRes => "资源号不在 13 项闭集（17/-1/10 缺口均拒）",
            RlimErr::ECurGtMax => "新软限大于新硬限（含 INF 软限配有限硬限）",
            RlimErr::ERaiseHard => "无特权抬高软限越过旧硬限或抬高硬限",
            RlimErr::ERowDrift => "getrlimit 声明值与账面漂移，对账不符",
            RlimErr::ECountDrift => "lowering 计数声明与账面漂移，重放不符",
            RlimErr::ESealed => "限值表已封印，一切 setrlimit 拒绝",
            RlimErr::EPhase => "Config 相未激活，setrlimit 越相",
            RlimErr::ELedgerFull => "限值账本已满（cap 32），新账拒绝",
            RlimErr::EInvState => "限值表不变量对账不符，状态机告警",
            RlimErr::EUnknown => "原始诊断码不在 0x7D440 段闭集",
        }
    }

    /// 闭集全体。
    pub const ALL: [RlimErr; 10] = [
        RlimErr::EBadRes,
        RlimErr::ECurGtMax,
        RlimErr::ERaiseHard,
        RlimErr::ERowDrift,
        RlimErr::ECountDrift,
        RlimErr::ESealed,
        RlimErr::EPhase,
        RlimErr::ELedgerFull,
        RlimErr::EInvState,
        RlimErr::EUnknown,
    ];

    /// 由原始诊断码解码；越界归 EUnknown。
    pub fn from_code(code: u32) -> RlimErr {
        match code.checked_sub(SEG_BASE) {
            Some(off) => match RlimErr::ALL.get(off as usize) {
                Some(e) => *e,
                None => RlimErr::EUnknown,
            },
            None => RlimErr::EUnknown,
        }
    }
}

// ── 资源闭集（恰 13 项）─────────────────────────────────────
/// 资源闭集：Linux 原始编号，10 为缺口（不在闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RlimRes {
    /// RLIMIT_CPU = 0
    Cpu,
    /// RLIMIT_FSIZE = 1
    Fsize,
    /// RLIMIT_DATA = 2
    Data,
    /// RLIMIT_STACK = 3
    Stack,
    /// RLIMIT_CORE = 4
    Core,
    /// RLIMIT_RSS = 5
    Rss,
    /// RLIMIT_NPROC = 6
    Nproc,
    /// RLIMIT_NOFILE = 7
    Nofile,
    /// RLIMIT_MEMLOCK = 8
    Memlock,
    /// RLIMIT_AS = 9
    As,
    /// RLIMIT_MSGQUEUE = 14
    Msgqueue,
    /// RLIMIT_NICE = 15
    Nice,
    /// RLIMIT_RTPRIO = 16
    Rtprio,
}

impl RlimRes {
    /// 原始 i32 → 闭集单点判别；17/-1/10（缺口）返回 None。
    pub fn from_raw(raw: i32) -> Option<RlimRes> {
        match raw {
            0 => Some(RlimRes::Cpu),
            1 => Some(RlimRes::Fsize),
            2 => Some(RlimRes::Data),
            3 => Some(RlimRes::Stack),
            4 => Some(RlimRes::Core),
            5 => Some(RlimRes::Rss),
            6 => Some(RlimRes::Nproc),
            7 => Some(RlimRes::Nofile),
            8 => Some(RlimRes::Memlock),
            9 => Some(RlimRes::As),
            14 => Some(RlimRes::Msgqueue),
            15 => Some(RlimRes::Nice),
            16 => Some(RlimRes::Rtprio),
            _ => None,
        }
    }

    /// 原始编号回读。
    pub fn raw(&self) -> i32 {
        match self {
            RlimRes::Cpu => 0,
            RlimRes::Fsize => 1,
            RlimRes::Data => 2,
            RlimRes::Stack => 3,
            RlimRes::Core => 4,
            RlimRes::Rss => 5,
            RlimRes::Nproc => 6,
            RlimRes::Nofile => 7,
            RlimRes::Memlock => 8,
            RlimRes::As => 9,
            RlimRes::Msgqueue => 14,
            RlimRes::Nice => 15,
            RlimRes::Rtprio => 16,
        }
    }

    /// 名称（RECON 对账用）。
    pub fn name(&self) -> &'static str {
        match self {
            RlimRes::Cpu => "RLIMIT_CPU",
            RlimRes::Fsize => "RLIMIT_FSIZE",
            RlimRes::Data => "RLIMIT_DATA",
            RlimRes::Stack => "RLIMIT_STACK",
            RlimRes::Core => "RLIMIT_CORE",
            RlimRes::Rss => "RLIMIT_RSS",
            RlimRes::Nproc => "RLIMIT_NPROC",
            RlimRes::Nofile => "RLIMIT_NOFILE",
            RlimRes::Memlock => "RLIMIT_MEMLOCK",
            RlimRes::As => "RLIMIT_AS",
            RlimRes::Msgqueue => "RLIMIT_MSGQUEUE",
            RlimRes::Nice => "RLIMIT_NICE",
            RlimRes::Rtprio => "RLIMIT_RTPRIO",
        }
    }

    /// 闭集全体，序与 RECON_TABLE 一致。
    pub const ALL: [RlimRes; 13] = [
        RlimRes::Cpu,
        RlimRes::Fsize,
        RlimRes::Data,
        RlimRes::Stack,
        RlimRes::Core,
        RlimRes::Rss,
        RlimRes::Nproc,
        RlimRes::Nofile,
        RlimRes::Memlock,
        RlimRes::As,
        RlimRes::Msgqueue,
        RlimRes::Nice,
        RlimRes::Rtprio,
    ];
}

// ── 对账表（名称, 原始编号, 闭集序号）───────────────────────
/// RECON_TABLE：13 行逐行与资源闭集对账。
pub const RECON_TABLE: [(&str, i32, i32); 13] = [
    ("RLIMIT_CPU", 0, 0),
    ("RLIMIT_FSIZE", 1, 1),
    ("RLIMIT_DATA", 2, 2),
    ("RLIMIT_STACK", 3, 3),
    ("RLIMIT_CORE", 4, 4),
    ("RLIMIT_RSS", 5, 5),
    ("RLIMIT_NPROC", 6, 6),
    ("RLIMIT_NOFILE", 7, 7),
    ("RLIMIT_MEMLOCK", 8, 8),
    ("RLIMIT_AS", 9, 9),
    ("RLIMIT_MSGQUEUE", 14, 10),
    ("RLIMIT_NICE", 15, 11),
    ("RLIMIT_RTPRIO", 16, 12),
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
    "F9411 setrlimit 错误矩阵".to_string()
}

// ── 状态机相（单调迁移）─────────────────────────────────────
/// 相：Config → Live → Sealed，只进不退。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 配置相：grant_priv 可用；setrlimit 须先激活
    Config,
    /// 运行相：setrlimit 可用
    Live,
    /// 封印相：一切写拒
    Sealed,
}

// ── 账目 ─────────────────────────────────────────────────────
/// 一条 setrlimit 账目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RlimRec {
    /// 单调流水号
    pub seq: u64,
    /// 资源原始编号（闭集内）
    pub res: u8,
    /// 旧软限
    pub old_cur: u64,
    /// 旧硬限
    pub old_max: u64,
    /// 新软限
    pub new_cur: u64,
    /// 新硬限
    pub new_max: u64,
}

/// 一行限值账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RlimRow {
    /// 资源闭集成员
    pub res: RlimRes,
    /// 软限
    pub cur: u64,
    /// 硬限
    pub max: u64,
    /// 硬限下调次数（逐资源单调计数）
    pub lowered: u32,
}

// ── 域模型：限值表 ──────────────────────────────────────────
/// 限值表：13 行 (res, cur, max, lowered) + 定容账本。
pub struct RlimTable {
    rows: Vec<RlimRow>,
    ledger: Vec<RlimRec>,
    cap: usize,
    phase: Phase,
    /// 特权位（只授不收，单调）
    priv_flag: bool,
    seq: u64,
    /// 拒绝计数（按错误码闭集）
    rejects: Vec<(RlimErr, u32)>,
}

impl RlimTable {
    /// 新建：13 行按 Linux 常识初始化。
    pub fn new(privileged: bool) -> RlimTable {
        let mut rows = Vec::new();
        let specs: [(RlimRes, u64, u64); RES_N] = [
            (RlimRes::Cpu, RLIM_INFINITY, RLIM_INFINITY),
            (RlimRes::Fsize, RLIM_INFINITY, RLIM_INFINITY),
            (RlimRes::Data, RLIM_INFINITY, RLIM_INFINITY),
            (RlimRes::Stack, STACK_DEFAULT, RLIM_INFINITY),
            (RlimRes::Core, 0, RLIM_INFINITY),
            (RlimRes::Rss, RLIM_INFINITY, RLIM_INFINITY),
            (RlimRes::Nproc, 63_471, RLIM_INFINITY),
            (RlimRes::Nofile, NOFILE_DEFAULT, 4096),
            (RlimRes::Memlock, 65_536, 65_536),
            (RlimRes::As, RLIM_INFINITY, RLIM_INFINITY),
            (RlimRes::Msgqueue, 819_200, 819_200),
            (RlimRes::Nice, 0, 0),
            (RlimRes::Rtprio, 0, 0),
        ];
        let mut i = 0usize;
        while i < RES_N {
            match specs.get(i) {
                Some((res, cur, max)) => {
                    rows.push(RlimRow {
                        res: *res,
                        cur: *cur,
                        max: *max,
                        lowered: 0,
                    });
                }
                None => {}
            }
            i += 1;
        }
        RlimTable {
            rows,
            ledger: Vec::new(),
            cap: LEDGER_CAP,
            phase: Phase::Config,
            priv_flag: privileged,
            seq: 0,
            rejects: Vec::new(),
        }
    }

    fn note_reject(&mut self, e: RlimErr) {
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
    pub fn activate(&mut self) -> Result<(), RlimErr> {
        match self.phase {
            Phase::Config => {
                self.phase = Phase::Live;
                Ok(())
            }
            Phase::Live => Ok(()),
            Phase::Sealed => Err(RlimErr::ESealed),
        }
    }

    /// 封印：任何非封印相 → Sealed；二次封印显性拒。
    pub fn seal(&mut self) -> Result<(), RlimErr> {
        match self.phase {
            Phase::Sealed => Err(RlimErr::ESealed),
            _ => {
                self.phase = Phase::Sealed;
                Ok(())
            }
        }
    }

    /// 授特权（只授不收；仅 Config 相可授）。
    pub fn grant_priv(&mut self) -> Result<(), RlimErr> {
        match self.phase {
            Phase::Config => {
                self.priv_flag = true;
                Ok(())
            }
            Phase::Live => Ok(()),
            Phase::Sealed => Err(RlimErr::ESealed),
        }
    }

    fn row_index(&self, res: RlimRes) -> Option<usize> {
        let mut i = 0usize;
        while i < self.rows.len() {
            match self.rows.get(i) {
                Some(r) => {
                    if r.res == res {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 当前相。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 账本长度。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 读一行限值（getrlimit 载体；闭集外 None）。
    pub fn get_row(&self, raw: i32) -> Option<(u64, u64)> {
        let res = match RlimRes::from_raw(raw) {
            Some(r) => r,
            None => return None,
        };
        let idx = self.row_index(res)?;
        match self.rows.get(idx) {
            Some(r) => Some((r.cur, r.max)),
            None => None,
        }
    }

    /// lowering 计数读数。
    pub fn lowered_of(&self, raw: i32) -> Option<u32> {
        let res = match RlimRes::from_raw(raw) {
            Some(r) => r,
            None => return None,
        };
        let idx = self.row_index(res)?;
        match self.rows.get(idx) {
            Some(r) => Some(r.lowered),
            None => None,
        }
    }

    /// getrlimit 声明值对账：声明 (cur,max) 与账面不符即漂移。
    pub fn verify_row(&mut self, raw: i32, claimed_cur: u64, claimed_max: u64) -> Result<(), RlimErr> {
        let (cur, max) = match self.get_row(raw) {
            Some(v) => v,
            None => {
                self.note_reject(RlimErr::EBadRes);
                return Err(RlimErr::EBadRes);
            }
        };
        if cur == claimed_cur && max == claimed_max {
            Ok(())
        } else {
            self.note_reject(RlimErr::ERowDrift);
            Err(RlimErr::ERowDrift)
        }
    }

    /// lowering 计数对账：声明计数与账面不符即漂移。
    pub fn verify_lowered(&mut self, raw: i32, claimed: u32) -> Result<(), RlimErr> {
        let n = match self.lowered_of(raw) {
            Some(v) => v,
            None => {
                self.note_reject(RlimErr::EBadRes);
                return Err(RlimErr::EBadRes);
            }
        };
        if n == claimed {
            Ok(())
        } else {
            self.note_reject(RlimErr::ECountDrift);
            Err(RlimErr::ECountDrift)
        }
    }

    /// setrlimit 主判序（与 Linux 一致，逐关冻结）：
    /// 相守卫 → 资源闭集 → new cur ≤ new max → 特权关（抬硬限 /
    /// 越旧硬限）→ 硬限 ratchet 不低于现行软限 → 账本容量 → 落账。
    /// 成功返回旧 (cur, max)。
    pub fn setrlimit(&mut self, raw: i32, new_cur: u64, new_max: u64) -> Result<(u64, u64), RlimErr> {
        // 第 0 关：相守卫
        match self.phase {
            Phase::Sealed => {
                self.note_reject(RlimErr::ESealed);
                return Err(RlimErr::ESealed);
            }
            Phase::Config => {
                self.note_reject(RlimErr::EPhase);
                return Err(RlimErr::EPhase);
            }
            Phase::Live => {}
        }
        // 第 1 关：资源闭集
        let res = match RlimRes::from_raw(raw) {
            Some(r) => r,
            None => {
                self.note_reject(RlimErr::EBadRes);
                return Err(RlimErr::EBadRes);
            }
        };
        let idx = match self.row_index(res) {
            Some(i) => i,
            None => {
                self.note_reject(RlimErr::EInvState);
                return Err(RlimErr::EInvState);
            }
        };
        let (old_cur, old_max) = match self.rows.get(idx) {
            Some(r) => (r.cur, r.max),
            None => {
                self.note_reject(RlimErr::EInvState);
                return Err(RlimErr::EInvState);
            }
        };
        // 第 2 关：new cur ≤ new max（INF cur 配有限 max 亦在此拒）
        if new_cur > new_max {
            self.note_reject(RlimErr::ECurGtMax);
            return Err(RlimErr::ECurGtMax);
        }
        // 第 3 关：特权（抬硬限 / 抬软限越过旧硬限）
        if !self.priv_flag {
            if new_max > old_max {
                self.note_reject(RlimErr::ERaiseHard);
                return Err(RlimErr::ERaiseHard);
            }
            if new_cur > old_max {
                self.note_reject(RlimErr::ERaiseHard);
                return Err(RlimErr::ERaiseHard);
            }
        }
        // 第 4 关：硬限 ratchet 不得低于现行软限（选拒：ECurGtMax）
        if new_max < old_cur {
            self.note_reject(RlimErr::ECurGtMax);
            return Err(RlimErr::ECurGtMax);
        }
        // 第 5 关：账本容量
        if self.ledger.len() >= self.cap {
            self.note_reject(RlimErr::ELedgerFull);
            return Err(RlimErr::ELedgerFull);
        }
        // 落账 + 应用（lowering 记数）
        let lowering = new_max < old_max;
        self.seq = self.seq.wrapping_add(1);
        self.ledger.push(RlimRec {
            seq: self.seq,
            res: res.raw() as u8,
            old_cur,
            old_max,
            new_cur,
            new_max,
        });
        match self.rows.get_mut(idx) {
            Some(r) => {
                r.cur = new_cur;
                r.max = new_max;
                if lowering {
                    r.lowered = r.lowered.wrapping_add(1);
                }
            }
            None => {
                self.note_reject(RlimErr::EInvState);
                return Err(RlimErr::EInvState);
            }
        }
        Ok((old_cur, old_max))
    }

    /// 不变量对账：13 行、逐行 cur ≤ max、流水号严格单调、
    /// 容量守恒、lowered 计数与账本重放一致、账目资源号在闭集。
    pub fn invariant_ok(&self) -> bool {
        if self.rows.len() != RES_N || self.ledger.len() > self.cap {
            return false;
        }
        for r in self.rows.iter() {
            if r.cur > r.max {
                return false;
            }
        }
        // 资源号无重复且在闭集
        let mut i = 0usize;
        while i < RES_N {
            let a = match self.rows.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if RlimRes::from_raw(a.res.raw()) != Some(a.res) {
                return false;
            }
            let mut j = i + 1;
            while j < RES_N {
                let b = match self.rows.get(j) {
                    Some(v) => *v,
                    None => return false,
                };
                if a.res == b.res {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        // 流水号严格单调
        let mut prev: u64 = 0;
        let mut first = true;
        for rec in self.ledger.iter() {
            if first {
                first = false;
            } else if rec.seq <= prev {
                return false;
            }
            prev = rec.seq;
            if RlimRes::from_raw(rec.res as i32).is_none() {
                return false;
            }
        }
        // lowered 计数与账本重放一致
        let mut i = 0usize;
        while i < RES_N {
            let row = match self.rows.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let mut replay = 0u32;
            for rec in self.ledger.iter() {
                if rec.res as i32 == row.res.raw() && rec.new_max < rec.old_max {
                    replay += 1;
                }
            }
            if replay != row.lowered {
                return false;
            }
            i += 1;
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
        buf.push(if self.priv_flag { 1u8 } else { 0u8 });
        push_u64(&mut buf, self.seq);
        push_u64(&mut buf, self.rows.len() as u64);
        for r in self.rows.iter() {
            push_u64(&mut buf, r.res.raw() as u64);
            push_u64(&mut buf, r.cur);
            push_u64(&mut buf, r.max);
            push_u64(&mut buf, r.lowered as u64);
        }
        push_u64(&mut buf, self.ledger.len() as u64);
        for rec in self.ledger.iter() {
            push_u64(&mut buf, rec.seq);
            push_u64(&mut buf, rec.res as u64);
            push_u64(&mut buf, rec.old_cur);
            push_u64(&mut buf, rec.old_max);
            push_u64(&mut buf, rec.new_cur);
            push_u64(&mut buf, rec.new_max);
        }
        fnv1a64(&buf)
    }

    /// 软件写回：状态=已完成、结果摘要非空。
    pub fn status_summary(&self) -> String {
        format!(
            "{} 状态=已完成 段=0x{:X} 相={:?} 特权={} 行数={} 账目={} 指纹={:#018X}",
            domain_tag(),
            SEG_BASE,
            self.phase,
            self.priv_flag,
            self.rows.len(),
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
    pub fn case(&mut self, name: &str, expect: RlimErr, got: Result<(u64, u64), RlimErr>) {
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

        // 场景 1：资源号 17（闭集外）
        let mut t = fresh_live(false);
        inj.case("资源17", RlimErr::EBadRes, t.setrlimit(17, 1, 2));

        // 场景 2：资源号 -1（闭集外）
        let mut t = fresh_live(false);
        inj.case("资源-1", RlimErr::EBadRes, t.setrlimit(-1, 1, 2));

        // 场景 3：资源号 10（闭集缺口）
        let mut t = fresh_live(false);
        inj.case("资源10缺口", RlimErr::EBadRes, t.setrlimit(10, 1, 2));

        // 场景 4：new cur > new max（NOFILE cur=4097 max=1024）
        let mut t = fresh_live(false);
        inj.case("cur超max", RlimErr::ECurGtMax, t.setrlimit(7, 4097, NOFILE_DEFAULT));

        // 场景 5：INF cur 配有限 max
        let mut t = fresh_live(false);
        inj.case("INF配有限", RlimErr::ECurGtMax, t.setrlimit(7, RLIM_INFINITY, 4096));

        // 场景 6：无特权抬硬限
        let mut t = fresh_live(false);
        inj.case("抬硬限无特权", RlimErr::ERaiseHard, t.setrlimit(7, NOFILE_DEFAULT, 8192));

        // 场景 7：无特权抬软限越过旧硬限
        let mut t = fresh_live(false);
        inj.case("越旧硬限", RlimErr::ERaiseHard, t.setrlimit(7, 5000, 5000));

        // 场景 8：硬限 ratchet 低于现行软限（NOFILE 旧 cur=1024，新 max=1000）
        let mut t = fresh_live(false);
        inj.case("硬限低于软限", RlimErr::ECurGtMax, t.setrlimit(7, 500, 1000));

        // 场景 9：封印后 setrlimit
        let mut t = fresh_live(false);
        let _ = t.seal();
        inj.case("封印后set", RlimErr::ESealed, t.setrlimit(7, 1024, 4096));

        // 场景 10：二次封印
        let mut t = fresh_live(false);
        let _ = t.seal();
        inj.case("二次封印", RlimErr::ESealed, t.seal().map(|_| (0, 0)));

        // 场景 11：账本满（cap 32，第 33 笔拒）
        let mut t = fresh_live(false);
        let mut i = 0u64;
        while i < 32 {
            let _ = t.setrlimit(0, i, RLIM_INFINITY); // CPU 软限任意下调均合法
            i += 1;
        }
        inj.case("账本满", RlimErr::ELedgerFull, t.setrlimit(0, 100, RLIM_INFINITY));

        // 场景 12：getrlimit 声明漂移
        let mut t = fresh_live(false);
        let r = t.verify_row(7, 999, 4096);
        inj.case("行对账漂移", RlimErr::ERowDrift, r.map(|_| (0, 0)));

        // 场景 13：lowering 计数声明漂移
        let mut t = fresh_live(false);
        let r = t.verify_lowered(7, 3);
        inj.case("计数对账漂移", RlimErr::ECountDrift, r.map(|_| (0, 0)));

        // 场景 14：Config 相未激活即 setrlimit
        let mut t = RlimTable::new(false);
        inj.case("未激活set", RlimErr::EPhase, t.setrlimit(7, 1024, 4096));

        (inj.caught, inj.total)
    }

    /// 零误伤阳性集：合法操作一个都不许拒。返回 (通过数, 总数)。
    pub fn run_positives() -> (usize, usize) {
        let mut pass = 0usize;
        let total = 5usize;
        // 阳 1：恒等幂等 set（NOFILE (1024,4096) 原样再设）
        let mut t = fresh_live(false);
        if t.setrlimit(7, NOFILE_DEFAULT, 4096).is_ok() && t.setrlimit(7, NOFILE_DEFAULT, 4096).is_ok() {
            pass += 1;
        }
        // 阳 2：NOFILE 恰设 1024
        let mut t = fresh_live(false);
        match t.setrlimit(7, NOFILE_DEFAULT, 4096) {
            Ok((1024, 4096)) => pass += 1,
            _ => {}
        }
        // 阳 3：栈软限设 0（0 ≤ INF，合法）
        let mut t = fresh_live(false);
        if t.setrlimit(3, 0, RLIM_INFINITY).is_ok() {
            pass += 1;
        }
        // 阳 4：有特权抬硬限
        let mut t = fresh_live(true);
        if t.setrlimit(7, NOFILE_DEFAULT, 8192).is_ok() {
            pass += 1;
        }
        // 阳 5：硬限自由下调（不低于现行软限），lowering 计数 +1
        let mut t = fresh_live(false);
        if t.setrlimit(7, NOFILE_DEFAULT, 2048).is_ok() {
            match t.lowered_of(7) {
                Some(1) => pass += 1,
                _ => {}
            }
        }
        (pass, total)
    }

    /// 注入日志（逐行中文，供聚合摘要）。
    pub fn log(&self) -> Vec<String> {
        self.log.clone()
    }
}

/// 造一台已激活的限值表（可选特权）。
fn fresh_live(privileged: bool) -> RlimTable {
    let mut t = RlimTable::new(privileged);
    let _ = t.activate();
    t
}

// ── 完成时 API：域自检 ──────────────────────────────────────
/// UNX-F9411 域自检（J1 全判据落点）。
pub fn run_unx9411_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9411");

    // J1-01 注入必抓
    let (c, t) = Injector::run_all();
    s.add("J1-01 注入必抓", c == t && t >= 10, "注入场景逐条拦截且错误码精确匹配，总数≥10");

    // J1-02 零误伤
    let (pc, pt) = Injector::run_positives();
    s.add("J1-02 零误伤阳性", pc == pt, "幂等set/NOFILE=1024/栈0/特权抬硬限/自由下调全过");

    // J1-03 资源闭集恰 13 项且 roundtrip
    let mut set_ok = RlimRes::ALL.len() == 13;
    for r in RlimRes::ALL.iter() {
        match RlimRes::from_raw(r.raw()) {
            Some(b) => {
                if *r != b {
                    set_ok = false;
                }
            }
            None => set_ok = false,
        }
    }
    s.add("J1-03 资源闭集13项", set_ok, "13 项原始编号逐项 roundtrip 恒等");

    // J1-04 闭集外拒绝（17/-1/10 缺口）
    let rej = RlimRes::from_raw(17).is_none()
        && RlimRes::from_raw(-1).is_none()
        && RlimRes::from_raw(10).is_none()
        && RlimRes::from_raw(11).is_none()
        && RlimRes::from_raw(12).is_none()
        && RlimRes::from_raw(13).is_none();
    s.add("J1-04 闭集外拒绝", rej, "17/-1/10/11/12/13（缺口带）全部 None");

    // J1-05 判序：闭集关先于 cur≤max 关
    let mut t = fresh_live(false);
    let o1 = match t.setrlimit(17, 9999, 1) {
        Err(RlimErr::EBadRes) => true,
        _ => false,
    };
    s.add("J1-05 判序闭集先行", o1, "坏资源号+坏限值 → EBadRes，判序第一关");

    // J1-06 判序：cur≤max 关先于特权关
    let mut t = fresh_live(false);
    let o2 = match t.setrlimit(7, 5000, 1) {
        Err(RlimErr::ECurGtMax) => true,
        _ => false,
    };
    s.add("J1-06 判序cur先行", o2, "cur>max 且越旧硬限 → ECurGtMax（cur≤max 关在前）");

    // J1-07 INF 哨兵语义（抬硬限到 INF 属特权面，须持特权表）
    let mut t = fresh_live(true);
    let i1 = t.setrlimit(7, NOFILE_DEFAULT, RLIM_INFINITY).is_ok();
    let i2 = match t.setrlimit(7, RLIM_INFINITY, 4096) {
        Err(RlimErr::ECurGtMax) => true,
        _ => false,
    };
    let i3 = t.setrlimit(0, RLIM_INFINITY, RLIM_INFINITY).is_ok();
    s.add("J1-07 INF哨兵语义", i1 && i2 && i3, "max=INF（特权）放行；cur=INF 配有限 max 拒；双 INF 放行");

    // J1-08 特权关：无特权两路拒 / 有特权过
    let mut t = fresh_live(false);
    let p1 = match t.setrlimit(7, NOFILE_DEFAULT, 8192) {
        Err(RlimErr::ERaiseHard) => true,
        _ => false,
    };
    let p2 = match t.setrlimit(7, 5000, 5000) {
        Err(RlimErr::ERaiseHard) => true,
        _ => false,
    };
    let mut tp = fresh_live(true);
    let p3 = tp.setrlimit(7, NOFILE_DEFAULT, 8192).is_ok() && tp.setrlimit(7, 5000, 8192).is_ok();
    s.add("J1-08 特权关", p1 && p2 && p3, "无特权抬硬限/越旧硬限拒；有特权全过");

    // J1-09 ratchet 纪律：低于现行软限拒；高于则自由下调并记数
    let mut t = fresh_live(false);
    let r1 = match t.setrlimit(7, 500, 1000) {
        Err(RlimErr::ECurGtMax) => true,
        _ => false,
    };
    let r2 = t.setrlimit(7, NOFILE_DEFAULT, 2048).is_ok();
    let r3 = match t.lowered_of(7) {
        Some(1) => true,
        _ => false,
    };
    s.add("J1-09 ratchet纪律", r1 && r2 && r3, "新硬限<现行软限拒；合法下调过且 lowered=1");

    // J1-10 相守卫：未激活拒 / 封印后拒 / 二次封印拒
    let mut t = RlimTable::new(false);
    let g1 = match t.setrlimit(7, 1024, 4096) {
        Err(RlimErr::EPhase) => true,
        _ => false,
    };
    let _ = t.activate();
    let _ = t.seal();
    let g2 = match t.setrlimit(7, 1024, 4096) {
        Err(RlimErr::ESealed) => true,
        _ => false,
    };
    let g3 = match t.seal() {
        Err(RlimErr::ESealed) => true,
        _ => false,
    };
    s.add("J1-10 相守卫三连", g1 && g2 && g3, "未激活EPhase；封印后ESealed；二次封印ESealed");

    // J1-11 账本容量守卫（cap 32）
    let mut t = fresh_live(false);
    let mut i = 0u64;
    while i < 32 {
        let _ = t.setrlimit(0, i, RLIM_INFINITY);
        i += 1;
    }
    let cap_ok = t.ledger_len() == LEDGER_CAP
        && match t.setrlimit(0, 100, RLIM_INFINITY) {
            Err(RlimErr::ELedgerFull) => true,
            _ => false,
        };
    s.add("J1-11 账本容量32", cap_ok, "cap=32 满 后第 33 笔 ELedgerFull");

    // J1-12 对账双通道：行漂移 + 计数漂移
    let mut t = fresh_live(false);
    let d1 = match t.verify_row(7, 999, 4096) {
        Err(RlimErr::ERowDrift) => true,
        _ => false,
    };
    let d2 = t.verify_row(7, NOFILE_DEFAULT, 4096).is_ok();
    let d3 = match t.verify_lowered(7, 3) {
        Err(RlimErr::ECountDrift) => true,
        _ => false,
    };
    let d4 = t.verify_lowered(7, 0).is_ok();
    s.add("J1-12 对账双通道", d1 && d2 && d3 && d4, "行声明漂移/真值过；计数漂移/真值过");

    // J1-13 不变量对账
    let mut t = fresh_live(true);
    let _ = t.setrlimit(7, NOFILE_DEFAULT, 8192);
    let _ = t.setrlimit(7, NOFILE_DEFAULT, 2048);
    let _ = t.setrlimit(3, 0, RLIM_INFINITY);
    s.add("J1-13 不变量对账", t.invariant_ok(), "13行 cur≤max/流水单调/lowered 重放一致");

    // J1-14 RECON_TABLE 13 行对账
    let mut recon_ok = RECON_TABLE.len() == 13;
    for row in RECON_TABLE.iter() {
        let (name, raw, idx) = *row;
        match RlimRes::from_raw(raw) {
            Some(a) => {
                if a.name() != name {
                    recon_ok = false;
                }
                match RlimRes::ALL.get(idx as usize) {
                    Some(b) => {
                        if *b != a {
                            recon_ok = false;
                        }
                    }
                    None => recon_ok = false,
                }
            }
            None => recon_ok = false,
        }
    }
    s.add("J1-14 RECON对账", recon_ok, "13 行名称/编号/闭集序号三方一致");

    // J1-15 段位守卫
    let mut seg_ok = SEG_BASE == 0x7D440;
    let mut prev = 0u32;
    let mut first = true;
    for e in RlimErr::ALL.iter() {
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
    s.add("J1-15 段位守卫", seg_ok, "0x7D440 独占；10 诊断码段内单调不重叠");

    // J1-16 软件写回摘要非空
    let t = fresh_live(false);
    let summary = t.status_summary();
    s.add("J1-16 摘要非空", summary.len() > 0 && summary.contains("已完成"), "状态=已完成、结果摘要非空");

    // 动态明细：双跑指纹一致
    let fp1 = replay_fingerprint();
    let fp2 = replay_fingerprint();
    s.add_dyn("J1-17 双跑指纹", fp1 == fp2, "同序列两台新表指纹逐位相等");

    // 动态明细：注入计数
    let (c2, t2) = Injector::run_all();
    s.add_dyn("J1-18 注入计数", c2 == t2 && t2 >= 10, "抓到=总数且≥10");

    // 动态明细：诊断明细互异
    let mut det_ok = true;
    let n = RlimErr::ALL.len();
    let mut i = 0usize;
    while i < n {
        let mut j = i + 1;
        while j < n {
            let a = match RlimErr::ALL.get(i) {
                Some(v) => v.detail(),
                None => "",
            };
            let b = match RlimErr::ALL.get(j) {
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
    s.add_dyn("J1-19 诊断明细互异", det_ok, "10 变体中文明细非空互异");

    s
}

/// 固定操作序列的重放指纹（双跑一致判据载体）。
fn replay_fingerprint() -> u64 {
    let mut t = RlimTable::new(true);
    let _ = t.activate();
    let _ = t.setrlimit(7, NOFILE_DEFAULT, 8192);
    let _ = t.setrlimit(7, NOFILE_DEFAULT, 2048);
    let _ = t.setrlimit(3, 0, RLIM_INFINITY);
    let _ = t.setrlimit(7, 4097, 1024); // 拒绝也计入序列（不改状态）
    let _ = t.setrlimit(17, 1, 2); // 拒绝
    let _ = t.seal();
    t.fingerprint()
}

// ── 测试（≥8 条；断言族仅在此允许）─────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 用 match 断错（无 unwrap/expect）。
    fn expect_err(got: Result<(u64, u64), RlimErr>, want: RlimErr) -> bool {
        match got {
            Err(e) => e == want,
            Ok(_) => false,
        }
    }

    #[test]
    fn seg_base_and_err_vals() {
        assert_eq!(SEG_BASE, 0x7D440);
        let mut prev = 0u32;
        let mut first = true;
        for e in RlimErr::ALL.iter() {
            let v = e.val();
            assert!(v >= SEG_BASE && v < SEG_BASE + 10);
            if !first {
                assert!(v > prev);
            }
            first = false;
            prev = v;
        }
        assert_eq!(RlimErr::ALL.len(), 10);
        for e in RlimErr::ALL.iter() {
            assert_eq!(RlimErr::from_code(e.val()), *e);
        }
        assert_eq!(RlimErr::from_code(SEG_BASE + 55), RlimErr::EUnknown);
        assert_eq!(RlimErr::from_code(1), RlimErr::EUnknown);
    }

    #[test]
    fn res_closed_set() {
        assert_eq!(RlimRes::ALL.len(), 13);
        for r in RlimRes::ALL.iter() {
            match RlimRes::from_raw(r.raw()) {
                Some(b) => assert_eq!(*r, b),
                None => assert!(false, "闭集成员解析失败"),
            }
        }
        assert!(RlimRes::from_raw(17).is_none());
        assert!(RlimRes::from_raw(-1).is_none());
        assert!(RlimRes::from_raw(10).is_none());
        assert!(RlimRes::from_raw(11).is_none());
        assert!(RlimRes::from_raw(13).is_none());
        assert!(RlimRes::from_raw(16).is_some());
        assert!(RlimRes::from_raw(14).is_some());
    }

    #[test]
    fn judge_order_frozen() {
        // 闭集关第一
        let mut t = fresh_live(false);
        assert!(expect_err(t.setrlimit(17, 9999, 1), RlimErr::EBadRes));
        assert!(expect_err(t.setrlimit(10, 1, 2), RlimErr::EBadRes));
        // cur≤max 关第二
        assert!(expect_err(t.setrlimit(7, 5000, 1), RlimErr::ECurGtMax));
        // 特权关第三
        assert!(expect_err(t.setrlimit(7, 5000, 5000), RlimErr::ERaiseHard));
        // ratchet 关第四
        assert!(expect_err(t.setrlimit(7, 500, 1000), RlimErr::ECurGtMax));
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
        // INF 哨兵显式放行
        let mut tab = fresh_live(false);
        match tab.setrlimit(0, RLIM_INFINITY, RLIM_INFINITY) {
            Ok((RLIM_INFINITY, RLIM_INFINITY)) => {}
            _ => assert!(false, "双 INF 应放行"),
        }
        // 栈软限 0
        match tab.setrlimit(3, 0, RLIM_INFINITY) {
            Ok((STACK_DEFAULT, RLIM_INFINITY)) => {}
            _ => assert!(false, "栈 0 应放行"),
        }
    }

    #[test]
    fn ratchet_and_lowered_count() {
        let mut t = fresh_live(false);
        // 低于现行软限拒
        assert!(expect_err(t.setrlimit(7, 500, 1000), RlimErr::ECurGtMax));
        // 合法下调：4096 → 2048（cur=1024 不动）
        match t.setrlimit(7, NOFILE_DEFAULT, 2048) {
            Ok((NOFILE_DEFAULT, 4096)) => {}
            _ => assert!(false, "合法下调应过且返回旧值"),
        }
        match t.lowered_of(7) {
            Some(1) => {}
            _ => assert!(false, "lowered 应为 1"),
        }
        // 再下调：2048 → 1500
        let _ = t.setrlimit(7, NOFILE_DEFAULT, 1500);
        match t.lowered_of(7) {
            Some(2) => {}
            _ => assert!(false, "lowered 应为 2"),
        }
        // 每次操作后恒 cur ≤ max
        match t.get_row(7) {
            Some((cur, max)) => assert!(cur <= max),
            None => assert!(false, "NOFILE 行缺失"),
        }
        assert!(t.invariant_ok());
    }

    #[test]
    fn privilege_gate() {
        let mut t = fresh_live(false);
        // 无特权：抬硬限拒
        assert!(expect_err(t.setrlimit(7, NOFILE_DEFAULT, 8192), RlimErr::ERaiseHard));
        // 无特权：抬软限越旧硬限拒
        assert!(expect_err(t.setrlimit(7, 5000, 5000), RlimErr::ERaiseHard));
        // 有特权：全过
        let mut tp = fresh_live(true);
        match tp.setrlimit(7, NOFILE_DEFAULT, 8192) {
            Ok((NOFILE_DEFAULT, 4096)) => {}
            _ => assert!(false, "特权抬硬限应过"),
        }
        match tp.setrlimit(7, 5000, 8192) {
            Ok((NOFILE_DEFAULT, 8192)) => {}
            _ => assert!(false, "特权抬软限应过"),
        }
        assert!(tp.invariant_ok());
    }

    #[test]
    fn phase_monotonic_and_seal() {
        let mut t = RlimTable::new(false);
        assert!(expect_err(t.setrlimit(7, 1024, 4096), RlimErr::EPhase));
        assert!(t.activate().is_ok());
        assert!(t.activate().is_ok()); // 幂等
        assert!(t.setrlimit(7, 1024, 4096).is_ok());
        assert!(t.seal().is_ok());
        assert!(expect_err(t.setrlimit(7, 1024, 4096), RlimErr::ESealed));
        assert!(expect_err(t.seal().map(|_| (0, 0)), RlimErr::ESealed));
        // 封印后对账通道仍可读（只读不写）
        assert!(t.verify_row(7, NOFILE_DEFAULT, 4096).is_ok());
    }

    #[test]
    fn recon_and_drift_channels() {
        assert_eq!(RECON_TABLE.len(), 13);
        for row in RECON_TABLE.iter() {
            let (name, raw, idx) = *row;
            let a = match RlimRes::from_raw(raw) {
                Some(v) => v,
                None => {
                    assert!(false, "RECON 行解析失败");
                    continue;
                }
            };
            assert_eq!(a.name(), name);
            let b = match RlimRes::ALL.get(idx as usize) {
                Some(v) => *v,
                None => {
                    assert!(false, "RECON 序号越界");
                    continue;
                }
            };
            assert_eq!(a, b);
        }
        // 对账双通道
        let mut t = fresh_live(false);
        assert!(expect_err(t.verify_row(7, 999, 4096).map(|_| (0, 0)), RlimErr::ERowDrift));
        assert!(t.verify_row(7, NOFILE_DEFAULT, 4096).is_ok());
        assert!(expect_err(t.verify_lowered(7, 3).map(|_| (0, 0)), RlimErr::ECountDrift));
        assert!(t.verify_lowered(7, 0).is_ok());
        assert!(t.verify_row(10, 0, 0).map(|_| ()).is_err()); // 缺口资源 → EBadRes
    }

    #[test]
    fn ledger_cap_and_invariant() {
        let mut t = fresh_live(false);
        let mut i = 0u64;
        while i < 32 {
            match t.setrlimit(0, i, RLIM_INFINITY) {
                Ok(_) => {}
                Err(_) => assert!(false, "前 32 笔应全过"),
            }
            i += 1;
        }
        assert_eq!(t.ledger_len(), LEDGER_CAP);
        assert!(expect_err(t.setrlimit(0, 100, RLIM_INFINITY), RlimErr::ELedgerFull));
        assert!(t.invariant_ok());
    }

    #[test]
    fn dual_run_fingerprint_equal() {
        let a = replay_fingerprint();
        let b = replay_fingerprint();
        assert_eq!(a, b, "双跑指纹漂移");
        let mut t = RlimTable::new(false);
        let _ = t.activate();
        let c = t.fingerprint();
        assert_ne!(a, c);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9411_checks();
        assert!(s.all_passed(), "域自检有红项");
        assert!(!s.truncated(), "域自检被截断");
        assert!(s.len() >= 12, "检查项不足 12");
        let t = fresh_live(false);
        let sum = t.status_summary();
        assert!(sum.len() > 0);
        assert!(sum.contains("0x7D440"));
    }
}
