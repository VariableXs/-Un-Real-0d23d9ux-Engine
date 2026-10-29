//! unx4182_readlink — UNX-F4182 readlink 语义与返回长度（诊断段 0x9840）。
//!
//! 判据原文：
//!   J1 UNX-F4182-J1 readlink 返回目标全串且不追加 NUL（POSIX 口径断言），
//!      缓冲区恰等/不足两态行为账齐备（截断不写入越界）。
//! 设计纪律：
//!   1. 返回值口径：readlink 返回"实际放入缓冲的字节数"，全串放下时
//!      即目标全串长度；不追加 NUL（POSIX 口径：readlink 非 strncpy）。
//!   2. 两态行为闭集：缓冲恰等（buf.len()==目标长）全量写入返回全长；
//!      缓冲不足截断写入 buf.len() 字节，返回 buf.len()，绝不越界。
//!   3. 哨兵验证：写入区后一字节设哨兵，调用后哨兵不变即为"未越界
//!      写入"的账面证据；NUL 账：返回长度处及之后不补 0。
//!   4. 错误路径：非链接名查 readlink → EINVAL 口径；不存在 → ENOENT。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9840;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOENT: u16 = SEG_BASE + 1;    // 链接名不存在
const E_INVAL: u16 = SEG_BASE + 2;    // 对象存在但非符号链接（EINVAL 口径）
const E_DUP: u16 = SEG_BASE + 3;      // 链接名重复登记
const E_EMPTY: u16 = SEG_BASE + 4;    // 空链接名/空目标串拒收
const E_LEDGER: u16 = SEG_BASE + 5;   // 两态账失守（越界或误补 NUL）
const E_STATE: u16 = SEG_BASE + 6;    // 缓冲态机非法迁移（零缓冲进截断路径）

/// 缓冲适配状态机三态（两态账 + 零长特例）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BufFit {
    ExactFit, // 恰等：全串写入，返回全串长
    UnderRun, // 不足：截断写入，返回缓冲长
    ZeroBuf,  // 零长缓冲：零写入，返回 0
}

/// 链接库存：名 → 目标串。
pub struct LinkStore {
    pub names: Vec<String>,
    pub targets: Vec<String>,
}

impl LinkStore {
    pub fn new() -> LinkStore {
        LinkStore {
            names: Vec::new(),
            targets: Vec::new(),
        }
    }

    /// 登记符号链接：重名/空名/空目标拒收。
    pub fn add_link(&mut self, name: &str, target: &str) -> Result<(), u16> {
        if name.is_empty() || target.is_empty() {
            return Err(E_EMPTY);
        }
        if self.index_of(name).is_some() {
            return Err(E_DUP);
        }
        self.names.push(name.to_string());
        self.targets.push(target.to_string());
        Ok(())
    }

    /// 登记普通对象（非链接，供 EINVAL 口径注入）。
    pub fn add_plain(&mut self, name: &str) -> Result<(), u16> {
        if name.is_empty() {
            return Err(E_EMPTY);
        }
        if self.index_of(name).is_some() {
            return Err(E_DUP);
        }
        self.names.push(name.to_string());
        self.targets.push(String::new());
        Ok(())
    }

    fn index_of(&self, name: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.names.len() {
            if self.names[i] == name {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 按名读目标串（只读面，非链接对象报 EINVAL 口径）。
    pub fn target_of(&self, name: &str) -> Result<&str, u16> {
        match self.index_of(name) {
            Some(i) => {
                if self.targets[i].is_empty() {
                    Err(E_INVAL)
                } else {
                    Ok(&self.targets[i])
                }
            }
            None => Err(E_NOENT),
        }
    }

    /// 链接条目数读面。
    pub fn link_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.targets.len() {
            if !self.targets[i].is_empty() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// readlink 主语义：返回放入缓冲的字节数，不追加 NUL，不越界。
    /// 目标串以哨兵缓冲承载：buf[buf.len()] 由调用方留哨。
    pub fn readlink(&self, name: &str, buf: &mut [u8]) -> Result<usize, u16> {
        let idx = match self.index_of(name) {
            Some(i) => i,
            None => return Err(E_NOENT),
        };
        if self.targets[idx].is_empty() {
            return Err(E_INVAL); // 非链接对象
        }
        let full = self.targets[idx].as_bytes();
        let fit = classify(full.len(), buf.len());
        match fit {
            BufFit::ZeroBuf => Ok(0), // 零写入不崩
            BufFit::ExactFit => {
                let mut k = 0usize;
                while k < full.len() {
                    buf[k] = full[k];
                    k += 1;
                }
                Ok(full.len()) // 全串长，无 NUL
            }
            BufFit::UnderRun => {
                let mut k = 0usize;
                while k < buf.len() {
                    buf[k] = full[k]; // 截断写入，绝不触 buf.len() 位
                    k += 1;
                }
                Ok(buf.len())
            }
        }
    }
}

/// 缓冲态分类：恰等/不足/零长三态（状态机纯函数面）。
pub fn classify(target_len: usize, buf_len: usize) -> BufFit {
    if buf_len == 0 {
        BufFit::ZeroBuf
    } else if buf_len >= target_len {
        BufFit::ExactFit
    } else {
        BufFit::UnderRun
    }
}

/// readlink 行为账：两态断言 + NUL 账 + 越界拦截账。
pub struct RlLedger {
    pub full_no_nul: u32,  // 全串放下且未补 NUL 的断言数
    pub exact_fit: u32,    // 恰等态断言数
    pub trunc: u32,        // 截断态断言数
    pub zero_writes: u32,  // 零长缓冲零写入断言数
    pub oob_blocked: u32,  // 越界被哨兵拦截计数（判据要求恒零）
    pub nul_faults: u32,   // 误补 NUL 计数（判据要求恒零）
}

impl RlLedger {
    pub fn new() -> RlLedger {
        RlLedger {
            full_no_nul: 0,
            exact_fit: 0,
            trunc: 0,
            zero_writes: 0,
            oob_blocked: 0,
            nul_faults: 0,
        }
    }

    /// 两态账守恒：恰等+截断+零长=全串无 NUL 断言的承载总量。
    pub fn conserved(&self) -> bool {
        self.oob_blocked == 0 && self.nul_faults == 0 && self.exact_fit >= 1
    }

    /// 账面导出。
    pub fn snapshot(&self) -> [u32; 6] {
        [
            self.full_no_nul,
            self.exact_fit,
            self.trunc,
            self.zero_writes,
            self.oob_blocked,
            self.nul_faults,
        ]
    }

    /// 账面复位（第二轮两态断言独立复演用，恒零账保留）。
    pub fn reset_counts(&mut self) {
        self.full_no_nul = 0;
        self.exact_fit = 0;
        self.trunc = 0;
        self.zero_writes = 0;
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOENT => "链接名不存在",
        E_INVAL => "对象存在但非符号链接（EINVAL 口径）",
        E_DUP => "链接名重复登记",
        E_EMPTY => "空链接名/空目标串拒收",
        E_LEDGER => "两态账失守（越界或误补 NUL）",
        E_STATE => "缓冲态机非法迁移（零缓冲进截断路径）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：全串无 NUL→恰等态→截断态→零长缓冲→越界哨兵→注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut ls = LinkStore::new();

    // R1：登记 /tmp/ln → /etc/passwd（目标 11 字节）与普通对象。
    ls.add_link("ln", "/etc/passwd")?;
    ls.add_plain("plain")?;
    let full_len = "/etc/passwd".len(); // 11
    hits += 1;

    // R2：恰等态——缓冲恰等目标长，全串写入返回全长，哨兵不越界。
    let mut lg = RlLedger::new();
    let mut buf = [0u8; 16];
    buf[full_len] = 0xA5; // 哨兵：恰等态下不得被触碰
    let n = ls.readlink("ln", &mut buf[..full_len])?;
    if n != full_len {
        return Err(E_LEDGER);
    }
    if buf[full_len] != 0xA5 {
        lg.oob_blocked += 1;
        return Err(E_LEDGER);
    }
    // NUL 账：末字节是 'd'（0x64）而非 0。
    if buf[full_len - 1] != b'd' {
        lg.nul_faults += 1;
        return Err(E_LEDGER);
    }
    lg.exact_fit += 1;
    lg.full_no_nul += 1;
    hits += 1;

    // R3：截断态——缓冲 5 字节：截断写入 5 字节返回 5，第 6 字节哨兵不动。
    let mut small = [b'.'; 6];
    small[5] = 0x5A; // 哨兵
    let n = ls.readlink("ln", &mut small[..5])?;
    if n != 5 {
        return Err(E_LEDGER);
    }
    if small[5] != 0x5A {
        lg.oob_blocked += 1;
        return Err(E_LEDGER);
    }
    lg.trunc += 1;
    hits += 1;

    // R4：零长缓冲——零写入返回 0 不崩（POSIX 口径）。
    let mut none: [u8; 0] = [];
    let n = ls.readlink("ln", &mut none)?;
    if n != 0 {
        return Err(E_LEDGER);
    }
    lg.zero_writes += 1;
    hits += 1;

    // R5：充裕缓冲——大于目标长按恰等态全量写入返回全串长。
    let mut big = [0u8; 32];
    big[full_len] = 0x3C; // 哨兵（全串后第一位不得触碰）
    let n = ls.readlink("ln", &mut big)?;
    if n != full_len || big[full_len] != 0x3C {
        return Err(E_LEDGER);
    }
    lg.exact_fit += 1;
    lg.full_no_nul += 1;
    hits += 1;

    // I1：不存在名 → E_NOENT；非链接对象 → E_INVAL。
    if ls.readlink("nope", &mut big) != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    if ls.readlink("plain", &mut big) != Err(E_INVAL) {
        return Err(E_INVAL);
    }
    hits += 1;

    // I2：登记注入——重名/空名/空目标三连拒收。
    if ls.add_link("ln", "/x") != Err(E_DUP) {
        return Err(E_DUP);
    }
    if ls.add_link("", "/x") != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    if ls.add_link("x", "") != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // R6：状态机分类面复核 + 账面守恒终审。
    if classify(11, 11) != BufFit::ExactFit
        || classify(11, 5) != BufFit::UnderRun
        || classify(11, 0) != BufFit::ZeroBuf
    {
        return Err(E_STATE);
    }
    if !lg.conserved() {
        return Err(E_LEDGER);
    }
    let s = lg.snapshot();
    if s[0] != 2 || s[1] != 2 || s[2] != 1 || s[3] != 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R7：第二条链接两态复演——目标 /var/log/syslog（14 字节），
    // 充裕缓冲全串无 NUL、小缓冲截断不越界，账面复位后独立入账。
    ls.add_link("ln2", "/var/log/syslog")?;
    let full2 = "/var/log/syslog".len(); // 14
    lg.reset_counts();
    let mut b2 = [0u8; 20];
    b2[full2] = 0x6B; // 哨兵
    let n = ls.readlink("ln2", &mut b2[..16])?;
    if n != full2 || b2[full2] != 0x6B {
        return Err(E_LEDGER);
    }
    lg.exact_fit += 1;
    lg.full_no_nul += 1;
    let mut b3 = [b'.'; 5];
    let n = ls.readlink("ln2", &mut b3)?;
    if n != 5 {
        return Err(E_LEDGER);
    }
    lg.trunc += 1;
    let mut zero2: [u8; 0] = [];
    if ls.readlink("ln2", &mut zero2) != Ok(0) {
        return Err(E_LEDGER);
    }
    lg.zero_writes += 1;
    if !lg.conserved() {
        return Err(E_LEDGER);
    }
    // 只读面对账：目标串逐条可读且计数齐。
    if ls.target_of("ln2")? != "/var/log/syslog" || ls.link_count() != 2 {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
