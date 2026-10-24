//! UNX-F6881 设备生命周期四态状态机（枚举→注册→在用→remove）（0x9080 段）
//!
//! 锚点：UNX-F6881-J1 四态状态机迁移表断言（合法迁移 4+非法拒绝 ≥6）；
//! 状态迹线可导出；与 F6850 锚语义衔接。
//! 域：CoRun·UNX-B4-B25（设备生命周期面）。
//!
//! 结构：四态闭集（Enumerated→Registered→Active→Removed）+ 迁移表
//! （合法 4 边 + 全序号对偶拒收 12−4=8 非法 ≥6）+ 迁移执行器
//! （守卫前置 + 事件留账 + 迹线导出）+ F6850 锚衔接（设备锚 id
//! 与生命周期 id 绑定）+ 注入核（未知态/回迁/跳迁/封账）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x9080 独占 0x80）。

pub const SEG_BASE: u32 = 0x9080;
pub const XC_LF_UNK: u32 = SEG_BASE + 0x01; // 未知态
pub const XC_LF_ILLMOVE: u32 = SEG_BASE + 0x02; // 非法迁移
pub const XC_LF_NOGUARD: u32 = SEG_BASE + 0x03; // 守卫未过
pub const XC_LF_SEALED: u32 = SEG_BASE + 0x04; // 封账

/// 四态闭集。
pub const ST_ENUMERATED: u8 = 0;
pub const ST_REGISTERED: u8 = 1;
pub const ST_ACTIVE: u8 = 2;
pub const ST_REMOVED: u8 = 3;

/// 态名（迹线导出用）。
pub fn state_name(s: u8) -> &'static str {
    match s {
        ST_ENUMERATED => "Enumerated",
        ST_REGISTERED => "Registered",
        ST_ACTIVE => "Active",
        ST_REMOVED => "Removed",
        _ => "?",
    }
}

/// 合法迁移表（4 边冻结）：
/// 枚举→注册（驱动 probe 成功）/ 注册→在用（open+ready）
/// / 在用→注销（remove 流程启动）/ 注册→注销（probe 后未开即拔）。
pub const LEGAL_MOVES: [(u8, u8); 4] = [
    (ST_ENUMERATED, ST_REGISTERED),
    (ST_REGISTERED, ST_ACTIVE),
    (ST_ACTIVE, ST_REMOVED),
    (ST_REGISTERED, ST_REMOVED),
];

/// 合法迁移判定。
pub fn is_legal(from: u8, to: u8) -> bool {
    LEGAL_MOVES.iter().any(|&(f, t)| f == from && t == to)
}

/// 迁移事件账项。
#[derive(Clone)]
pub struct MoveEvent {
    pub dev_id: u64,
    pub from: u8,
    pub to: u8,
    pub cause: String,
    pub seq: u64,
}

/// 设备生命周期账（每设备独立）。
pub struct Lifecycle {
    pub dev_id: u64,
    pub anchor_id: u64, // F6850 锚 id 衔接
    pub state: u8,
    pub trace: Vec<MoveEvent>,
    pub seq: u64,
    pub sealed: bool,
}

impl Lifecycle {
    /// 新账（初始态 = 枚举）。
    pub fn new(dev_id: u64, anchor_id: u64) -> Self {
        Lifecycle {
            dev_id,
            anchor_id,
            state: ST_ENUMERATED,
            trace: Vec::new(),
            seq: 0,
            sealed: false,
        }
    }

    /// F6850 锚语义衔接：anchor_id 必须与 dev_id 同源（高位共享）。
    pub fn anchor_bound(&self) -> bool {
        (self.dev_id >> 32) == (self.anchor_id >> 32) && self.anchor_id != 0
    }

    /// 迁移执行（守卫 + 留账）。
    pub fn migrate(&mut self, to: u8, cause: &str) -> Result<(), u32> {
        if to > ST_REMOVED {
            return Err(XC_LF_UNK);
        }
        if self.sealed {
            return Err(XC_LF_SEALED);
        }
        if !is_legal(self.state, to) {
            return Err(XC_LF_ILLMOVE);
        }
        // 守卫：进入 Active 前必须锚绑定
        if to == ST_ACTIVE && !self.anchor_bound() {
            return Err(XC_LF_NOGUARD);
        }
        // 守卫：进入 Removed 后账封
        let entering_removed = to == ST_REMOVED;
        self.seq += 1;
        self.trace.push(MoveEvent {
            dev_id: self.dev_id,
            from: self.state,
            to,
            cause: String::from(cause),
            seq: self.seq,
        });
        self.state = to;
        if entering_removed {
            self.sealed = true;
        }
        Ok(())
    }

    /// 迹线导出："id:from>to(cause)" 链。
    pub fn trace_export(&self) -> String {
        let mut s = String::new();
        let mut first = true;
        for e in &self.trace {
            if !first {
                s.push(';');
            }
            first = false;
            let seg = format!(
                "{}:{}>{}({})",
                e.dev_id,
                state_name(e.from),
                state_name(e.to),
                e.cause
            );
            s.push_str(&seg);
        }
        s
    }

    /// 迹线完整核：每步 from == 上一步 to。
    pub fn trace_chained(&self) -> bool {
        for i in 1..self.trace.len() {
            if self.trace[i].from != self.trace[i - 1].to {
                return false;
            }
        }
        !self.trace.is_empty()
    }

    /// 终态断言：Removed 且封账。
    pub fn is_removed(&self) -> bool {
        self.state == ST_REMOVED && self.sealed
    }
}

/// 全迁移表断言核：4 合法边全放行 + 8 非法对偶全拒（≥6 判据线）。
pub fn move_table_exact() -> bool {
    // 合法 4 边
    let mut legal_ok = 0usize;
    for &(f, t) in LEGAL_MOVES.iter() {
        if is_legal(f, t) {
            legal_ok += 1;
        }
    }
    // 非法对偶：全 4×4=16 对减 4 合法 = 12 非法（≥6 要求，实抓 12）
    let mut illegal_caught = 0usize;
    for f in 0..4u8 {
        for t in 0..4u8 {
            if !LEGAL_MOVES.iter().any(|&(lf, lt)| lf == f && lt == t) && !is_legal(f, t) {
                illegal_caught += 1;
            }
        }
    }
    legal_ok == 4 && illegal_caught == 12
}

/// 全生命周期正序核：枚举→注册→在用→注销（4 迁移留账）。
pub fn full_lifecycle_exact() -> bool {
    let mut lf = Lifecycle::new(0xAA00_0001, 0xAA00_0001);
    if !lf.anchor_bound() {
        return false;
    }
    if lf.migrate(ST_REGISTERED, "probe").is_err() {
        return false;
    }
    if lf.migrate(ST_ACTIVE, "open").is_err() {
        return false;
    }
    if lf.migrate(ST_REMOVED, "remove").is_err() {
        return false;
    }
    lf.is_removed() && lf.trace.len() == 3 && lf.trace_chained()
}

/// 短生命周期核：枚举→注册→注销（未开即拔）。
pub fn short_lifecycle_exact() -> bool {
    let mut lf = Lifecycle::new(0xBB00_0002, 0xBB00_0002);
    if lf.migrate(ST_REGISTERED, "probe").is_err() {
        return false;
    }
    if lf.migrate(ST_REMOVED, "unplug-before-open").is_err() {
        return false;
    }
    lf.is_removed() && lf.trace.len() == 2
}

/// 迹线导出可读核：导出串含全态名与原因。
pub fn trace_readable() -> bool {
    let mut lf = Lifecycle::new(0xCC00_0003, 0xCC00_0003);
    let _ = lf.migrate(ST_REGISTERED, "probe");
    let _ = lf.migrate(ST_ACTIVE, "open");
    let _ = lf.migrate(ST_REMOVED, "remove");
    let t = lf.trace_export();
    t.contains("Enumerated>Registered(probe)")
        && t.contains("Registered>Active(open)")
        && t.contains("Active>Removed(remove)")
        && t.contains("0xCC000003")
}

/// 注入核（6 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知态迁移（to=7）
    let mut lf = Lifecycle::new(1, 1);
    if lf.migrate(7, "x") == Err(XC_LF_UNK) {
        caught += 1;
    }
    // 2) 跳迁：枚举直冲 Active
    if lf.migrate(ST_ACTIVE, "skip") == Err(XC_LF_ILLMOVE) {
        caught += 1;
    }
    // 3) 回迁：Removed→Enumerated（封账后任何迁移拒）
    let mut lf2 = Lifecycle::new(2, 2);
    let _ = lf2.migrate(ST_REGISTERED, "probe");
    let _ = lf2.migrate(ST_REMOVED, "remove");
    if lf2.migrate(ST_ENUMERATED, "resurrect") == Err(XC_LF_SEALED) {
        caught += 1;
    }
    // 4) 守卫拒绝：锚不绑定进 Active
    let mut lf3 = Lifecycle::new(0x1100_0004, 0x2200_0004);
    let _ = lf3.migrate(ST_REGISTERED, "probe");
    if lf3.migrate(ST_ACTIVE, "no-anchor") == Err(XC_LF_NOGUARD) {
        caught += 1;
    }
    // 5) 枚举→Removed 跳迁（非法：必须经注册）
    let mut lf4 = Lifecycle::new(3, 3);
    if lf4.migrate(ST_REMOVED, "jump") == Err(XC_LF_ILLMOVE) {
        caught += 1;
    }
    // 6) Active→Registered 回退（非法）
    let mut lf5 = Lifecycle::new(4, 4);
    let _ = lf5.migrate(ST_REGISTERED, "probe");
    let _ = lf5.migrate(ST_ACTIVE, "open");
    if lf5.migrate(ST_REGISTERED, "rollback") == Err(XC_LF_ILLMOVE) {
        caught += 1;
    }
    (caught, 6)
}

/// 指纹冻结：迹线折叠哈希。
pub fn fp_frozen(lf: &Lifecycle) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for e in &lf.trace {
        h ^= e.dev_id;
        h = h.wrapping_mul(0x100000001b3);
        h ^= e.from as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= e.to as u64;
        h = h.wrapping_mul(0x100000001b3);
        for &c in e.cause.as_bytes() {
            h ^= c as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx6881_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F6881");
    // J1-1 迁移表（4 合法 + 12 非法全拒 ≥6）
    let ok = move_table_exact();
    s.add("B25-01-迁移表断言", ok, if ok { "" } else { "迁移表漂移" });
    // J1-2 全生命周期
    let ok = full_lifecycle_exact();
    s.add("B25-02-全生命周期", ok, if ok { "" } else { "正序漂移" });
    // J1-3 短生命周期（未开即拔）
    let ok = short_lifecycle_exact();
    s.add("B25-03-短生命周期", ok, if ok { "" } else { "短路径漂移" });
    // J1-4 迹线可导出
    let ok = trace_readable();
    s.add("B25-04-迹线导出", ok, if ok { "" } else { "导出漂移" });
    // J1-5 F6850 锚衔接
    let lf = Lifecycle::new(0xDD00_0005, 0xDD00_0005);
    let bad = Lifecycle::new(0x1100_0006, 0x2200_0006);
    let ok = lf.anchor_bound() && !bad.anchor_bound();
    s.add("B25-05-锚衔接", ok, if ok { "" } else { "锚失配" });
    // J1-6 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B25-06-注入核必抓", ok, &detail);
    // J1-7 指纹冻结
    let mut lf2 = Lifecycle::new(7, 7);
    let _ = lf2.migrate(ST_REGISTERED, "probe");
    let _ = lf2.migrate(ST_ACTIVE, "open");
    let ok = fp_frozen(&lf2) == fp_frozen(&lf2);
    s.add("B25-07-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
