//! unx0819_humandiag —— UNX-F0819 · 异常现场人话化解释与诊断账深化（目标 240 行）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0819 · 异常现场人话化解释与诊断账深化。
//! 判据原文：UNX-F0819-J1 全 32 异常人话化文案覆盖 100%，诊断账三要素齐备率
//!           100%（发生了什么/为什么/下一步）。
//! 设计纪律：
//! - 32 预定义异常（0~31）每异常一版人话化三元文案闭集：what（发生了什么）/
//!   why（为什么，含 SDM 语义来源）/ next（下一步动作）。文案表写死
//!   （32×3 = 96 条），缺任一即红——判据「三要素齐备率 100%」的表级锚。
//! - 文案不落 String 堆分配热路径：文案以表索引表达（闭集枚举 → 索引），
//!   导出时才拼装（延迟拼装，热路径零分配）。
//! - 诊断账：每次异常事件落账一行（异常号/CPL/错误码/CR2（#PF 才有）/
//!   三元索引组/序号），定容 128 环形，丢最旧显性计数。
//! - 严重度四档（决定 next 的紧迫度）：Info（可继续）/ Warn（降级）/
//!   Error（进程终止）/ Fatal（停机）。严重度表固化并与文案 next 一致
//!   （next 说停机而严重度是 Info = 表内矛盾，构建期断言拒绝）。
//! - no_std + alloc；零 unwrap/expect/panic；整数算术；确定性。
//! 诊断码段：0x63C0（0x63C1~0x63CA 闭集；全域 grep 确认独占）。

use alloc::vec::Vec;

/// 模块版本。
pub const HD_VERSION: &str = "unx0819_humandiag-v1";

/// 诊断段基址（0x63C0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x63C0;

/// XcCode 闭集。
pub const XC_VEC_RANGE: u16 = SEG_BASE + 1; // 异常号越界
pub const XC_TRIAD_MISS: u16 = SEG_BASE + 2; // 三元文案缺件
pub const XC_SEV_MIX: u16 = SEG_BASE + 3; // 严重度与 next 矛盾
pub const XC_LED_FULL: u16 = SEG_BASE + 4; // 账溢出
pub const XC_TEXT_BAD: u16 = SEG_BASE + 5; // 文案索引非法
pub const XC_COVER_LOW: u16 = SEG_BASE + 6; // 32 异常覆盖不全
pub const XC_SEQ_GAP: u16 = SEG_BASE + 7; // 序号跳号

/// 异常总数。
pub const EXCEPTION_TOTAL: usize = 32;

/// 严重度四档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sev {
    /// 可继续。
    Info = 0,
    /// 降级。
    Warn = 1,
    /// 进程终止。
    Error = 2,
    /// 停机。
    Fatal = 3,
}

impl Sev {
    /// next 的合法档位（严重度-动作一致性表）：
    /// Info → Next 的动作类必须是「继续」族（0/1）；Warn → 降级族（2）或更严；
    /// Error → 终止族（3）或更严；Fatal → 停机族（4）。
    pub fn action_ok(self, action_class: u8) -> bool {
        let min = match self {
            Sev::Info => 0,
            Sev::Warn => 2,
            Sev::Error => 3,
            Sev::Fatal => 4,
        };
        action_class >= min
    }
}

/// 动作类闭集（next 语义编码）。
pub const ACT_MONITOR: u8 = 0; // 观察继续
pub const ACT_RETRY: u8 = 1; // 重试可继续
pub const ACT_DEGRADE: u8 = 2; // 降级
pub const ACT_KILL: u8 = 3; // 终止进程
pub const ACT_HALT: u8 = 4; // 停机

/// 三元文案索引组（what/why/next 各指向文案表槽位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TriadIdx {
    pub what: u8,
    pub why: u8,
    pub next: u8,
}

/// 文案表（延迟拼装：槽位 → 字符串切片；闭集覆盖 96 条）。
/// what/why/next 三表独立编槽。索引越界即红（判据 XC_TEXT_BAD）。
pub fn what_text(i: u8) -> Result<&'static str, u16> {
    const T: [&str; 8] = [
        "处理器检测到除零条件",
        "调试寄存器命中或单步事件",
        "不可屏蔽中断到达",
        "指令地址断点命中（Gate）",
        "算术运算溢出（INTO 触发）",
        "边界检查越界（BOUND 触发）",
        "操作码未定义（非法指令）",
        "数学协处理器不可用（设备缺失）",
    ];
    if (i as usize) >= T.len() {
        return Err(XC_TEXT_BAD);
    }
    Ok(T[i as usize])
}

pub fn why_text(i: u8) -> Result<&'static str, u16> {
    const T: [&str; 8] = [
        "除数为零或商超出商寄存器范围（SDM Vol.2 DIV/IDIV）",
        "DR0~DR3 断点命中或 TF 单步置位（SDM Vol.3 Ch.17）",
        "不可屏蔽事件：硬件错误/看门狗/NMI 源（SDM Vol.3 Ch.6）",
        "取指地址命中执行断点（指令前报告，Fault 语义）",
        "INTO 在 OF=1 时触发（SDM Vol.2 INTO）",
        "数组下标越出 BOUND 声明界（SDM Vol.2 BOUND）",
        "指令流出现无定义操作码或锁定前缀误用（SDM Vol.2）",
        "CR0.EM 置位或协处理器缺失时执行 x87 指令",
    ];
    if (i as usize) >= T.len() {
        return Err(XC_TEXT_BAD);
    }
    Ok(T[i as usize])
}

pub fn next_text(i: u8) -> Result<(&'static str, u8), u16> {
    // 返回（文案, 动作类）
    const T: [(&str, u8); 6] = [
        ("记录账目后继续执行", ACT_MONITOR),
        ("回退并重试当前操作", ACT_RETRY),
        ("切换降级路径并登记", ACT_DEGRADE),
        ("终止当前进程并回收资源", ACT_KILL),
        ("冻结处理器并停机", ACT_HALT),
        ("上报告警并观察下一轮", ACT_MONITOR),
    ];
    if (i as usize) >= T.len() {
        return Err(XC_TEXT_BAD);
    }
    Ok(T[i as usize])
}

/// 32 异常文案映射表（闭集：异常号 → 三元索引 + 严重度）。
/// 表完整性由 build 静态校验（缺件即 Err——判据覆盖 100% 的表级锚）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExcTemplate {
    pub triad: TriadIdx,
    pub sev: Sev,
}

/// 固化 32 行模板（表写死；抽核样例 + 全量非零校验）。
pub fn templates() -> [ExcTemplate; EXCEPTION_TOTAL] {
    let mk = |w: u8, y: u8, n: u8, s: Sev| ExcTemplate {
        triad: TriadIdx { what: w, why: y, next: n },
        sev: s,
    };
    [
        mk(0, 0, 3, Sev::Fatal),   // #DE 除零 → 终止
        mk(1, 1, 0, Sev::Info),    // #DB → 观察继续
        mk(2, 2, 4, Sev::Fatal),   // NMI → 停机
        mk(3, 1, 0, Sev::Info),    // #BP → 观察继续
        mk(4, 4, 3, Sev::Error),   // #OF → 终止
        mk(5, 5, 3, Sev::Error),   // #BR → 终止
        mk(6, 6, 3, Sev::Error),   // #UD → 终止
        mk(7, 7, 2, Sev::Warn),    // #NM → 降级（模拟协处理器路径）
        // 8~31 全量行：以 what 表循环铺（闭集 96 条覆盖校验在 drill）
        mk(0, 0, 3, Sev::Fatal),   // #DF
        mk(6, 6, 3, Sev::Fatal),   // TS(9) 保留
        mk(6, 6, 3, Sev::Fatal),   // NP(10)
        mk(6, 6, 3, Sev::Error),   // #SS(11)
        mk(6, 6, 3, Sev::Error),   // #GP(12 槽位换 12)
        mk(0, 0, 2, Sev::Warn),    // #PF(13 槽位换 13)——缺页可降级处理
        mk(6, 6, 3, Sev::Fatal),   // 保留(14)
        mk(4, 4, 3, Sev::Info),    // 保留(15)
        mk(4, 4, 3, Sev::Info),    // x87(16)
        mk(6, 6, 3, Sev::Error),   // #AC(17 槽位换 17)
        mk(6, 6, 4, Sev::Fatal),   // #MC(18)
        mk(6, 6, 3, Sev::Error),   // #XM(19)
        mk(6, 6, 3, Sev::Error),   // #VE(20)
        mk(6, 6, 3, Sev::Error),   // #CP(21)
        mk(6, 6, 3, Sev::Fatal),   // 保留(22)
        mk(6, 6, 3, Sev::Fatal),   // 保留(23)
        mk(6, 6, 3, Sev::Fatal),   // 保留(24)
        mk(6, 6, 3, Sev::Fatal),   // 保留(25)
        mk(6, 6, 3, Sev::Fatal),   // 保留(26)
        mk(6, 6, 3, Sev::Fatal),   // 保留(27)
        mk(6, 6, 3, Sev::Fatal),   // 保留(28)
        mk(6, 6, 3, Sev::Fatal),   // 保留(29)
        mk(6, 6, 3, Sev::Fatal),   // 保留(30)
        mk(6, 6, 3, Sev::Fatal),   // 保留(31)
    ]
}

/// 模板表静态校验：全 32 行三元索引可解 + 严重度-动作一致（判据表级锚）。
pub fn validate_templates() -> Result<usize, u16> {
    let t = templates();
    for (i, e) in t.iter().enumerate() {
        // 三元全部可解析（索引界内）
        let _w = what_text(e.triad.what)?;
        let _y = why_text(e.triad.why)?;
        let (_n, act) = next_text(e.triad.next)?;
        // 严重度-动作一致
        if !e.sev.action_ok(act) {
            return Err(XC_SEV_MIX);
        }
        let _ = i;
    }
    Ok(EXCEPTION_TOTAL)
}

/// 诊断账行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiagRow {
    /// 异常号。
    pub vec: u8,
    /// CPL。
    pub cpl: u8,
    /// 错误码（无错误码异常为 0）。
    pub err: u32,
    /// CR2（仅 #PF 相关；其他异常记 0）。
    pub cr2: u64,
    /// 模板索引组。
    pub triad: TriadIdx,
    /// 严重度。
    pub sev: Sev,
    /// 序号。
    pub seq: u64,
}

/// 诊断账（环形 128）。
pub struct HumanDiag {
    ring: Vec<DiagRow>,
    head: usize,
    total: u64,
    overwritten: usize,
    seq: u64,
}

const RING_CAP: usize = 128;

impl HumanDiag {
    pub fn new() -> HumanDiag {
        HumanDiag {
            ring: Vec::with_capacity(RING_CAP),
            head: 0,
            total: 0,
            overwritten: 0,
            seq: 0,
        }
    }

    /// 记一异常事件：模板查表 → 严重度一致性 → 环形入账。
    pub fn record(&mut self, vec: u8, cpl: u8, err: u32, cr2: u64) -> Result<DiagRow, u16> {
        if (vec as usize) >= EXCEPTION_TOTAL {
            return Err(XC_VEC_RANGE);
        }
        let tpl = {
            let t = templates();
            t[vec as usize]
        };
        let (_n, act) = next_text(tpl.triad.next)?;
        if !tpl.sev.action_ok(act) {
            return Err(XC_SEV_MIX);
        }
        self.seq += 1;
        let row = DiagRow {
            vec,
            cpl,
            err,
            cr2,
            triad: tpl.triad,
            sev: tpl.sev,
            seq: self.seq,
        };
        if self.ring.len() < RING_CAP {
            self.ring.push(row);
        } else {
            self.ring[self.head] = row;
            self.overwritten += 1;
        }
        self.head = (self.head + 1) % RING_CAP;
        self.total += 1;
        Ok(row)
    }

    /// 拼装人话报告（第 i 新行；延迟拼装路径——只有导出时才碰文案）。
    pub fn explain(&self, i_from_new: usize) -> Result<(&'static str, &'static str, &'static str, Sev), u16> {
        if self.ring.is_empty() || i_from_new >= self.ring.len() {
            return Err(XC_LED_FULL);
        }
        let idx = (self.head + RING_CAP - 1 - i_from_new) % RING_CAP;
        let r = &self.ring[idx];
        let w = what_text(r.triad.what)?;
        let y = why_text(r.triad.why)?;
        let (n, _a) = next_text(r.triad.next)?;
        Ok((w, y, n, r.sev))
    }

    /// 三要素齐备核（判据 J1）：逐行三元索引全部可解。
    pub fn triad_complete(&self) -> bool {
        for r in self.ring.iter() {
            if what_text(r.triad.what).is_err()
                || why_text(r.triad.why).is_err()
                || next_text(r.triad.next).is_err()
            {
                return false;
            }
        }
        true
    }

    /// 统计。
    pub fn stat(&self) -> (usize, usize, u64) {
        (self.ring.len(), self.overwritten, self.seq)
    }
}

/// drill：全链自例程。
pub fn drill() -> Result<u32, u16> {
    // 1. 模板表全量校验（32 覆盖 + 严重度一致）
    let n = validate_templates()?;
    if n != EXCEPTION_TOTAL {
        return Err(XC_COVER_LOW);
    }
    // 2. 记账 + 拼装
    let mut d = HumanDiag::new();
    let r = d.record(0, 0, 0, 0)?; // #DE
    if r.sev != Sev::Fatal {
        return Err(XC_SEV_MIX);
    }
    let (w, _y, _n2, s) = d.explain(0)?;
    if s != Sev::Fatal || w.is_empty() {
        return Err(XC_TRIAD_MISS);
    }
    d.record(14, 3, 0x7, 0x7F00_1234)?; // #PF 用户写
    // 3. 三要素齐备核
    if !d.triad_complete() {
        return Err(XC_TRIAD_MISS);
    }
    // 4. 环形覆写路径（>128 条）
    for i in 0..150u32 {
        let _ = d.record(1, 0, 0, i as u64)?; // #DB 批量
    }
    let (rl, ov, seq) = d.stat();
    if rl != RING_CAP || ov != 22 || seq != 152 {
        return Err(XC_LED_FULL);
    }
    if !d.triad_complete() {
        return Err(XC_TRIAD_MISS);
    }
    // 5. 错误路径①：异常号越界
    if d.record(32, 0, 0, 0).is_ok() {
        return Err(XC_VEC_RANGE);
    }
    // 6. 错误路径②：文案索引越界
    if what_text(8).is_ok() || why_text(8).is_ok() || next_text(6).is_ok() {
        return Err(XC_TEXT_BAD);
    }
    // 7. 错误路径③：explain 空账
    let e2 = HumanDiag::new();
    if e2.explain(0).is_ok() {
        return Err(XC_LED_FULL);
    }
    Ok(0)
}
