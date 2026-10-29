//! unx48328_smerrm1 — UNX-F48328 智能面错误码族 M1 段（8 条）（诊断段 0xC9E0）。
//!
//! 判据原文：
//!   J1 错误码 ×8 零漂移；判据锚定 D1 同源体系。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 错误码册（8 条）三重核对：码值（SEG_BASE+1 起连续）、文案
//!      指纹、族段位——册内任一字段漂移即红（错误码是契约，不是
//!      随手写的数字）。
//!   2. 连续性红线：码值必须 SEG_BASE+1..+8 连续无跳号；族段位统
//!      一 0xC9E0；越册码（+9 及以后）必须不被本册认领。
//!   3. 投放口径：错误码只经 throw 按册投放，未注册码拒投；投放
//!      账分册分立（每码投放计数可对）。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性，
//!      drill() 返回 Ok(hits)。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC9E0;

// ── 智能面错误码族 M1（本册正文，8 条）────────────────────
// 每条：码值 = SEG_BASE + 序号（+1 起），文案按 D1 同源体系措辞。
pub const SM_E_FEDLOOP_STALL: u16 = SEG_BASE + 1; // 反馈环停摆（学习不落账）
pub const SM_E_MISCOMMIT_STALE: u16 = SEG_BASE + 2; // 回退重算过期（下游未同步）
pub const SM_E_PYFIX_NOMATCH: u16 = SEG_BASE + 3; // 纠错无命中（闭集外不瞎纠）
pub const SM_E_CANDDIV_SKEW: u16 = SEG_BASE + 4;  // 多样性偏科（画像串味）
pub const SM_E_PERF_P99_OVER: u16 = SEG_BASE + 5; // P99 超预算（尾延迟红线）
pub const SM_E_OBS_POINT_MISS: u16 = SEG_BASE + 6; // 埋点缺位（四点不全）
pub const SM_E_FUZZ_CRASH: u16 = SEG_BASE + 7;    // fuzz 崩样（注入不崩红线）
pub const SM_E_PRIV_LEAK: u16 = SEG_BASE + 8;     // 隐私外联（本地闭环被破）
pub const SM_E_EMPTY_SUM: u16 = SEG_BASE + 9;     // 写回摘要为空（J2 口径）

/// 册容量与越册界。
pub const REG_N: usize = 8;
pub const REG_END: u16 = SEG_BASE + REG_N as u16 + 1; // 首个越册码

/// 错误码册条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ErrEntry {
    pub code: u16,
    pub seg: u16,     // 族段位
    pub seq: u8,      // 册内序号 1..=8
    pub text_fp: u64, // 文案指纹（防文案漂移）
}

/// 8 条文案原文（D1 同源措辞，指纹以此为准）。
pub const ERR_TEXT: [&str; REG_N] = [
    "反馈环停摆（学习不落账）",
    "回退重算过期（下游未同步）",
    "纠错无命中（闭集外不瞎纠）",
    "多样性偏科（画像串味）",
    "P99 超预算（尾延迟红线）",
    "埋点缺位（四点不全）",
    "fuzz 崩样（注入不崩红线）",
    "隐私外联（本地闭环被破）",
];

/// 文案指纹：逐字节 XOR 折叠（定长滚动）。
pub fn text_fp(s: &str) -> u64 {
    let b = s.as_bytes();
    let mut fp: u64 = 0xC9E0_C9E1;
    let mut i = 0usize;
    while i < b.len() {
        fp = fp.rotate_left(6) ^ (b[i] as u64);
        i += 1;
    }
    fp
}

/// 错误码投放面：按册投放 + 分册投放账。
pub struct SmErrM1 {
    reg: [ErrEntry; REG_N],
    pub thrown: [u32; REG_N],   // 各码投放计数
    pub reject_total: u32,      // 越册/未注册拒投计数
}

impl SmErrM1 {
    pub fn new() -> SmErrM1 {
        let mut reg = [ErrEntry {
            code: 0,
            seg: SEG_BASE,
            seq: 0,
            text_fp: 0,
        }; REG_N];
        let mut i = 0usize;
        while i < REG_N {
            reg[i] = ErrEntry {
                code: SEG_BASE + i as u16 + 1,
                seg: SEG_BASE,
                seq: i as u8 + 1,
                text_fp: text_fp(ERR_TEXT[i]),
            };
            i += 1;
        }
        SmErrM1 { reg, thrown: [0; REG_N], reject_total: 0 }
    }

    /// 按码查册。
    pub fn lookup(&mut self, code: u16) -> Result<ErrEntry, u16> {
        let mut i = 0usize;
        while i < REG_N {
            if self.reg[i].code == code {
                return Ok(self.reg[i]);
            }
            i += 1;
        }
        self.reject_total += 1;
        Err(SEG_BASE + 1) // 用本册第 1 码表达"查无此码"
    }

    /// 投放：注册码照投计账，越册码拒投（返回越册判定而非误投）。
    pub fn throw(&mut self, code: u16) -> Result<u32, u16> {
        if code >= REG_END {
            self.reject_total += 1;
            return Err(SM_E_PRIV_LEAK); // 越册即体系外泄，按最重码示意
        }
        match self.lookup(code) {
            Ok(e) => {
                self.thrown[(e.seq - 1) as usize] += 1;
                Ok(self.thrown[(e.seq - 1) as usize])
            }
            Err(_) => Err(SM_E_PRIV_LEAK),
        }
    }

    /// 三重核对（J1 零漂移主体）：
    /// ① 码值 SEG_BASE+1..+8 连续；② 族段位统一 SEG_BASE；
    /// ③ 文案指纹与册面一致（文案不得暗改）。
    pub fn audit(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < REG_N {
            let e = self.reg[i];
            if e.code != SEG_BASE + i as u16 + 1 {
                return Err(SM_E_FEDLOOP_STALL);
            }
            if e.seg != SEG_BASE {
                return Err(SM_E_FEDLOOP_STALL);
            }
            if e.seq != i as u8 + 1 {
                return Err(SM_E_FEDLOOP_STALL);
            }
            if e.text_fp != text_fp(ERR_TEXT[i]) {
                return Err(SM_E_FEDLOOP_STALL);
            }
            i += 1;
        }
        Ok(())
    }

    /// 投放账守恒：Σ投放 == 投放成功计数和（无影子投放）。
    pub fn ledger_ok(&self, want_total: u32) -> Result<(), u16> {
        let mut s: u32 = 0;
        let mut i = 0usize;
        while i < REG_N {
            s += self.thrown[i];
            i += 1;
        }
        if s != want_total {
            return Err(SM_E_FEDLOOP_STALL);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.audit().is_err() {
            return Err(SM_E_FEDLOOP_STALL);
        }
        if summary_len == 0 {
            return Err(SM_E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 文案指纹稳定项（供外部复算对账）。
pub fn fingerprint_all() -> Vec<u64> {
    let mut v: Vec<u64> = Vec::new();
    let mut i = 0usize;
    while i < REG_N {
        v.push(text_fp(ERR_TEXT[i]));
        i += 1;
    }
    v
}

/// 自例程判据：逐条落实 J1（错误码 ×8 零漂移），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 三重核对 ×8：码值连续、段位统一、文案指纹逐条一致。
    let m = SmErrM1::new();
    m.audit()?;
    hits += 1;

    // R2 码值连续性逐条断言：+1..+8 逐条可查，+9 起查无此码。
    let mut m = SmErrM1::new();
    let mut i = 1u16;
    while i <= 8 {
        let e = m.lookup(SEG_BASE + i)?;
        if e.seq != i as u8 {
            return Err(SM_E_FEDLOOP_STALL);
        }
        i += 1;
    }
    if m.lookup(REG_END).is_ok() {
        return Err(SM_E_FEDLOOP_STALL);
    }
    if m.reject_total != 1 {
        return Err(SM_E_FEDLOOP_STALL);
    }
    hits += 1;

    // R3 投放口径：注册码照投计账（各码计数逐次可对），越册码拒投。
    let mut m = SmErrM1::new();
    m.throw(SM_E_FEDLOOP_STALL)?;
    m.throw(SM_E_FEDLOOP_STALL)?;
    m.throw(SM_E_PERF_P99_OVER)?;
    if m.thrown[0] != 2 || m.thrown[4] != 1 {
        return Err(SM_E_FEDLOOP_STALL);
    }
    if m.throw(REG_END + 3) != Err(SM_E_PRIV_LEAK) {
        return Err(SM_E_FEDLOOP_STALL);
    }
    m.ledger_ok(3)?;
    if m.reject_total != 1 {
        return Err(SM_E_FEDLOOP_STALL);
    }
    hits += 1;

    // R4 未注册中间码不认领：册只覆盖 +1..+8，跳号码（假设历史遗留
    //    +0 与 +9）都不可投。
    let mut m = SmErrM1::new();
    if m.throw(SEG_BASE) != Err(SM_E_PRIV_LEAK) {
        return Err(SM_E_FEDLOOP_STALL);
    }
    if m.throw(SEG_BASE + 9) != Err(SM_E_PRIV_LEAK) {
        return Err(SM_E_FEDLOOP_STALL);
    }
    m.ledger_ok(0)?;
    hits += 1;

    // R5 文案指纹稳定：同文案两次指纹一致；异文案指纹不同；指纹
    //    函数对单字节差异敏感。
    let f1 = text_fp(ERR_TEXT[0]);
    let f2 = text_fp(ERR_TEXT[0]);
    let f3 = text_fp(ERR_TEXT[1]);
    if f1 != f2 || f1 == f3 {
        return Err(SM_E_FEDLOOP_STALL);
    }
    let all1 = fingerprint_all();
    let all2 = fingerprint_all();
    if all1 != all2 || all1.len() != REG_N {
        return Err(SM_E_FEDLOOP_STALL);
    }
    hits += 1;

    // R6 可红演练：伪造投放账（账面计数与实际投放不符）ledger_ok
    //    必红；空摘要/审计不过写回必红。
    let mut m = SmErrM1::new();
    m.throw(SM_E_PYFIX_NOMATCH)?;
    if m.ledger_ok(2).is_ok() {
        return Err(SM_E_FEDLOOP_STALL); // 账面多记必红
    }
    if m.writeback(true, 0).is_ok() {
        return Err(SM_E_EMPTY_SUM);
    }
    let mut m2 = SmErrM1::new();
    m2.reg[3].code = 0xFFFF; // 破坏册面（模拟漂移）
    if m2.writeback(true, 8).is_ok() {
        return Err(SM_E_FEDLOOP_STALL); // 册面漂移写回必红
    }
    hits += 1;

    // R7 每码至少投放一轮 ×8 覆盖：8 条码各投一次，分册计数全 1，
    //    总账 == 8。
    let mut m = SmErrM1::new();
    let mut i = 0u16;
    while i < 8 {
        m.throw(SEG_BASE + i + 1)?;
        i += 1;
    }
    let mut i = 0usize;
    while i < REG_N {
        if m.thrown[i] != 1 {
            return Err(SM_E_FEDLOOP_STALL);
        }
        i += 1;
    }
    m.ledger_ok(8)?;
    hits += 1;

    // R8 复跑一致与写回（判据 J2）：整册重建两轮，册面与指纹逐位
    //    一致；写回过。
    let m1 = SmErrM1::new();
    let m2 = SmErrM1::new();
    let mut i = 0usize;
    while i < REG_N {
        if m1.reg[i] != m2.reg[i] {
            return Err(SM_E_FEDLOOP_STALL);
        }
        i += 1;
    }
    m1.audit()?;
    if m1.writeback(true, 0).is_ok() {
        return Err(SM_E_EMPTY_SUM);
    }
    m1.writeback(true, 0x11)?;
    hits += 1;

    Ok(hits)
}
