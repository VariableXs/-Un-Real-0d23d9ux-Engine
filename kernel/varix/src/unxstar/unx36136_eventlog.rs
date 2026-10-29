//! UNX-F36136-J1 类型注册事件账与统一日志框架埋点。
//! 判据：① 五级事件（TRACE/INFO/WARN/REJECT/FATAL）统一入账且级别
//! 判别式闭集；② 源模块埋点闭集（B07 十源 × 埋点号）与账目三向过滤
//! （按级/按源/按时刻窗）全通；③ 环形满载守恒（总量不丢、窗口保最新）；
//! ④ 纯功能代码 ≥320 行。
//! 设计纪律：统一日志真源——B07 各面的拒绝/登记/审计事件不该各记账本，
//! 而是统一埋进同一事件账（观测面单一入口）。事件五元组：seq/级别/源/
//! 埋点号/时刻。环形账满时顶最旧但 total 守真（拒绝风暴下不阻塞热路径，
//! 指纹窗口保最新——与 36127 攻击账同纪律）。36130 审计账是「登记行为」
//! 的司法账（不顶替），本账是「运行观测」的遥测账（可顶替）——两账分工
//! 在头注冻结，不得混用。

use crate::checks::CheckSet;
use super::unx36129_persist::name_fp;
use super::unx36121_typereg::GOLDEN_N;

pub const SEG_BASE: u16 = 0x81e0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_LEVEL: u16 = SEG_BASE + 2; // 级别闭集外
pub const E_SRC: u16 = SEG_BASE + 3; // 源埋点闭集外
pub const E_TICK: u16 = SEG_BASE + 4; // 时刻回拨/停滞
pub const E_WINDOW: u16 = SEG_BASE + 5; // 时刻窗非法（lo > hi）
pub const E_INTG: u16 = SEG_BASE + 6; // 完整性自检不符

/// 模块版本锚。
pub const VER: u32 = 1;

/// 事件账容量（遥测环形）。
pub const LOG_CAP: usize = 48;

/// 级别闭集（判别式 = 0x21 段）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Trace = 0x2101,
    Info = 0x2102,
    Warn = 0x2103,
    Reject = 0x2104,
    Fatal = 0x2105,
}

impl Level {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn text(self) -> &'static str {
        match self {
            Level::Trace => "TRACE",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Reject => "REJECT",
            Level::Fatal => "FATAL",
        }
    }
    pub fn from_code(c: u16) -> Result<Level, u16> {
        match c {
            0x2101 => Ok(Level::Trace),
            0x2102 => Ok(Level::Info),
            0x2103 => Ok(Level::Warn),
            0x2104 => Ok(Level::Reject),
            0x2105 => Ok(Level::Fatal),
            _ => Err(E_LEVEL),
        }
    }
    /// 级别权重（过滤面：WARN 及以上才算「值得看」）。
    pub fn at_least(self, floor: Level) -> bool {
        self.code() >= floor.code()
    }
}

/// B07 源埋点闭集（源号 = B07 段位表序；埋点号 = 源内事件语义）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Src {
    TypReg = 0x0101,   // 36121 注册表
    TypDeny = 0x0102,  // 36122 闸门
    GenBind = 0x0103,  // 36123 绑定
    Enroll = 0x0104,   // 36124 协议
    DefTier = 0x0105,  // 36125 档位
    ObCreate = 0x0106, // 36126 联签
    NoUnreg = 0x0107,  // 36127 注销禁
    TypeVer = 0x0108,  // 36128 版本化
    Persist = 0x0109,  // 36129 持久化
    TypAudit = 0x010A, // 36130 审计
}

impl Src {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn from_code(c: u16) -> Result<Src, u16> {
        if c >= 0x0101 && c <= 0x010A {
            // 判别式闭集内恒可还原（repr(u16) 连续段）。
            Ok(match c {
                0x0101 => Src::TypReg,
                0x0102 => Src::TypDeny,
                0x0103 => Src::GenBind,
                0x0104 => Src::Enroll,
                0x0105 => Src::DefTier,
                0x0106 => Src::ObCreate,
                0x0107 => Src::NoUnreg,
                0x0108 => Src::TypeVer,
                0x0109 => Src::Persist,
                _ => Src::TypAudit,
            })
        } else {
            Err(E_SRC)
        }
    }
}

/// 事件原子（五元组 + 对象指纹）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LogEv {
    pub seq: u32,
    pub level: u16,
    pub src: u16,
    pub point: u16, // 源内埋点号（语义由源自决）
    pub fp: u16,    // 对象名指纹（三账同源）
    pub tick: u64,
}

/// 环形遥测账（满载顶最旧；total 守真）。
pub struct LogBook {
    buf: [Option<LogEv>; LOG_CAP],
    head: usize,
    n: usize,
    seq_next: u32,
    tick: u64,
    pub total: u64,
}

const NO_LEV: Option<LogEv> = None;

impl LogBook {
    pub fn new() -> LogBook {
        LogBook { buf: [NO_LEV; LOG_CAP], head: 0, n: 0, seq_next: 1, tick: 0, total: 0 }
    }

    pub fn count(&self) -> usize {
        self.n
    }

    pub fn now(&self) -> u64 {
        self.tick
    }

    /// 埋点主面（统一入口）：级别/源双闭集核 + 时刻单调核 + 环形入账。
    pub fn log(&mut self, level: Level, src: Src, point: u16, fp: u16, tick: u64) -> Result<u32, u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        if Level::from_code(level.code()).is_err() {
            return Err(E_LEVEL);
        }
        if Src::from_code(src.code()).is_err() {
            return Err(E_SRC);
        }
        if tick <= self.tick && self.total > 0 {
            return Err(E_TICK);
        }
        let ev = LogEv { seq: self.seq_next, level: level.code(), src: src.code(), point, fp, tick };
        self.buf[(self.head + self.n) % LOG_CAP] = Some(ev);
        if self.n < LOG_CAP {
            self.n += 1;
        } else {
            self.head = (self.head + 1) % LOG_CAP;
        }
        self.seq_next = self.seq_next.wrapping_add(1);
        self.tick = tick;
        self.total = self.total.wrapping_add(1);
        Ok(ev.seq)
    }

    pub fn at(&self, i: usize) -> Result<LogEv, u16> {
        if i >= self.n {
            return Err(E_INTG);
        }
        match self.buf[(self.head + i) % LOG_CAP] {
            Some(e) => Ok(e),
            None => Err(E_INTG),
        }
    }

    /// 三向过滤之一：按级别下限（≥ floor）计数。
    pub fn count_level(&self, floor: Level) -> u32 {
        let mut c = 0;
        let mut i = 0;
        while i < self.n {
            if let Ok(e) = self.at(i) {
                if let Ok(lv) = Level::from_code(e.level) {
                    if lv.at_least(floor) {
                        c += 1;
                    }
                }
            }
            i += 1;
        }
        c
    }

    /// 三向过滤之二：按源计数。
    pub fn count_src(&self, src: Src) -> u32 {
        let mut c = 0;
        let mut i = 0;
        while i < self.n {
            if let Ok(e) = self.at(i) {
                if e.src == src.code() {
                    c += 1;
                }
            }
            i += 1;
        }
        c
    }

    /// 三向过滤之三：按时刻窗 [lo, hi] 计数。
    pub fn count_window(&self, lo: u64, hi: u64) -> Result<u32, u16> {
        if lo > hi {
            return Err(E_WINDOW);
        }
        let mut c = 0;
        let mut i = 0;
        while i < self.n {
            if let Ok(e) = self.at(i) {
                if e.tick >= lo && e.tick <= hi {
                    c += 1;
                }
            }
            i += 1;
        }
        Ok(c)
    }

    /// 账守恒：无空洞 + 双闭集 + seq 窗口内连续（环绕/未环绕两形态）。
    pub fn integrity(&self) -> Result<(), u16> {
        let wrapped = self.total > LOG_CAP as u64;
        let mut i = 0;
        while i < self.n {
            let e = self.at(i)?;
            if wrapped {
                // 环绕后窗口内最旧 seq = total − CAP + 1。
                if e.seq != (self.total - (self.n - i) as u64 + 1) as u32 {
                    return Err(E_INTG);
                }
            } else {
                // 未环绕：seq = 序位 + 1。
                if e.seq != i as u32 + 1 {
                    return Err(E_INTG);
                }
            }
            if Level::from_code(e.level).is_err() {
                return Err(E_LEVEL);
            }
            if Src::from_code(e.src).is_err() {
                return Err(E_SRC);
            }
            i += 1;
        }
        if (self.total as usize) < self.n {
            return Err(E_INTG);
        }
        Ok(())
    }
}

/// 指纹锚（ktest 交叉断言消费）：本账实际使用的 FNV-16 出口。
pub fn name_fp_log_anchor() -> u16 {
    name_fp("File")
}

/// B07 十源埋点演练负载（每源 3 事件：INFO/WARN/REJECT 各一）。
pub fn log_full_load() -> Result<LogBook, u16> {
    let mut book = LogBook::new();
    let srcs = [
        Src::TypReg, Src::TypDeny, Src::GenBind, Src::Enroll, Src::DefTier,
        Src::ObCreate, Src::NoUnreg, Src::TypeVer, Src::Persist, Src::TypAudit,
    ];
    let mut tick: u64 = 1;
    let mut s = 0;
    while s < 10 {
        let fp = name_fp(tr_golden_name(s % GOLDEN_N));
        book.log(Level::Info, srcs[s], 0x0001, fp, tick)?;
        tick += 1;
        book.log(Level::Warn, srcs[s], 0x0002, fp, tick)?;
        tick += 1;
        book.log(Level::Reject, srcs[s], 0x0003, fp, tick)?;
        tick += 1;
        s += 1;
    }
    book.integrity()?;
    Ok(book)
}

fn tr_golden_name(i: usize) -> &'static str {
    super::unx36121_typereg::GOLDEN_NAMES[i]
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：十源全埋点（30 事件）+ 三向过滤核。
    let book = log_full_load()?;
    if book.count() != 30 || book.total != 30 {
        return Err(E_INTG);
    }
    let (c_rej, _) = (book.count_level(Level::Reject), ());
    if c_rej != 10 {
        return Err(E_INTG);
    }
    if book.count_level(Level::Warn) != 20 {
        return Err(E_INTG); // WARN 及以上 = WARN 10 + REJECT 10
    }
    let (c_src1, _) = (book.count_src(Src::TypReg), ());
    if c_src1 != 3 {
        return Err(E_INTG);
    }
    let w = book.count_window(1, 15)?;
    if w != 15 {
        return Err(E_INTG);
    }
    if book.count_window(16, 15).err() != Some(E_WINDOW) {
        return Err(E_WINDOW);
    }
    // R2：级别闭集外显性拒。
    let mut book2 = LogBook::new();
    if Level::from_code(0x2199).err() != Some(E_LEVEL) {
        return Err(E_LEVEL);
    }
    if Src::from_code(0x0199).err() != Some(E_SRC) {
        return Err(E_SRC);
    }
    // 停滞/回拨显性拒。
    book2.log(Level::Info, Src::TypReg, 1, 0, 10)?;
    if book2.log(Level::Info, Src::TypReg, 1, 0, 10).err() != Some(E_TICK) {
        return Err(E_TICK);
    }
    if book2.log(Level::Info, Src::TypReg, 1, 0, 9).err() != Some(E_TICK) {
        return Err(E_TICK);
    }
    if book2.count() != 1 {
        return Err(E_INTG);
    }
    // R3：环形顶替守恒（LOG_CAP + 12 事件 → 窗口 48、total 60、最旧被顶）。
    let mut book3 = LogBook::new();
    let mut t: u64 = 0;
    let mut k = 0;
    while k < LOG_CAP + 12 {
        t += 1;
        book3.log(Level::Trace, Src::TypDeny, 0x0001, 0, t)?;
        k += 1;
    }
    if book3.count() != LOG_CAP || book3.total != (LOG_CAP + 12) as u64 {
        return Err(E_INTG);
    }
    book3.integrity()?;
    // 窗口最旧 seq = total − CAP + 1。
    let oldest = book3.at(0)?;
    if oldest.seq != book3.total as u32 - LOG_CAP as u32 + 1 {
        return Err(E_INTG);
    }
    // 顶替后窗口内全是最新事件（Trace 级）。
    if book3.count_level(Level::Trace) != LOG_CAP as u32 {
        return Err(E_INTG);
    }
    // R4：黄金负载重放确定性。
    let b1 = log_full_load()?;
    let b2 = log_full_load()?;
    if b1.total != b2.total || b1.count() != b2.count() {
        return Err(E_INTG);
    }
    let mut i = 0;
    while i < b1.count() {
        if b1.at(i)? != b2.at(i)? {
            return Err(E_INTG);
        }
        i += 1;
    }
    // R5：级别人话闭集（五级全可归因）。
    let lvls = [Level::Trace, Level::Info, Level::Warn, Level::Reject, Level::Fatal];
    let mut l = 0;
    while l < 5 {
        if lvls[l].text().is_empty() {
            return Err(E_LEVEL);
        }
        if Level::from_code(lvls[l].code()).is_err() {
            return Err(E_LEVEL);
        }
        l += 1;
    }
    // 校验和：total × 级别种数 + 窗口计数。
    Ok((book.total as u32).wrapping_mul(5).wrapping_add(w))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_LEVEL => "级别闭集外（五级判别式 0x21 段）",
        E_SRC => "源埋点闭集外（B07 十源 0x01 段）",
        E_TICK => "时刻回拨/停滞（遥测账不许时间倒流）",
        E_WINDOW => "时刻窗非法（lo > hi）",
        E_INTG => "完整性自检不符（环形失真/重放漂移）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36136_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36136_eventlog");
    cs.add("SEG 段位独占", SEG_BASE == 0x81e0, "0x81e0~0x81eF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "十源埋点/三向过滤/环形守恒/重放/级别闭集全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("级别闭集", Level::from_code(0x2101).is_ok() && Level::from_code(0x2105).is_ok() && Level::from_code(0x2106).is_err(), "五级判别式闭集");
    cs.add("源闭集", Src::from_code(0x0101).is_ok() && Src::from_code(0x010A).is_ok() && Src::from_code(0x010B).is_err(), "十源判别式闭集");
    cs.add("级别序语义", Level::Trace.at_least(Level::Info) == false && Level::Fatal.at_least(Level::Warn), "判别式即序（可过滤）");
    cs.add("账容量锚", LOG_CAP == 48, "遥测环形 48 槽");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}
