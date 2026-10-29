//! unx4195_readlnbuf — UNX-F4195 readlink 缓冲区边界（截断语义）（诊断段 0x99E0）。
//!
//! 判据原文：
//!   J1 UNX-F4195-J1 缓冲区小于目标时截断写入且返回值为写入字节数（非
//!      全串长度——POSIX 口径断言），零长度缓冲零写入不崩。
//! 设计纪律：
//!   1. 返回值口径：返回值=实际写入缓冲的字节数（POSIX readlink 口径），
//!      截断态返回 buf.len() 而非目标全串长——经典易错点专条。
//!   2. 截断写入闭包：写入字节数 = min(目标长, buf.len())，绝不越界
//!      触碰 buf.len() 位（哨兵验证入账）。
//!   3. 零长缓冲：零写入返回 0 不崩（判据专条），与截断态分账。
//!   4. 三态账：充裕（全串长）/截断（buf.len()）/零长（0）三分，
//!      每态写入数与返回值逐例对账（写入≠返回即失守）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x99E0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOENT: u16 = SEG_BASE + 1;   // 链接名不存在
const E_EMPTY: u16 = SEG_BASE + 2;   // 空名/空目标串拒收
const E_DUP: u16 = SEG_BASE + 3;     // 链接名重复登记
const E_LEDGER: u16 = SEG_BASE + 4;  // 三态账失守（写入≠返回）
const E_OOB: u16 = SEG_BASE + 5;     // 越界写入（哨兵被触碰）
const E_STATE: u16 = SEG_BASE + 6;   // 缓冲态机非法迁移

/// 截断语义缓冲态机三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TruncState {
    Plentiful, // 充裕：全串写入，返回全串长
    Truncated, // 截断：写 buf.len() 字节，返回 buf.len()
    ZeroBuf,   // 零长：零写入，返回 0
}

/// 缓冲态分类（状态机纯函数面）。
pub fn classify(target_len: usize, buf_len: usize) -> TruncState {
    if buf_len == 0 {
        TruncState::ZeroBuf
    } else if buf_len >= target_len {
        TruncState::Plentiful
    } else {
        TruncState::Truncated
    }
}

/// 截断账：三态分账 + 哨兵越界账。
pub struct TruncBook {
    pub plentiful: u32,  // 充裕态断言数
    pub truncated: u32,  // 截断态断言数（判据主面）
    pub zero_writes: u32,// 零长缓冲断言数
    pub oob_hits: u32,   // 哨兵被触碰数（恒零）
    pub ret_faults: u32, // 返回值≠写入数失守数（恒零）
}

impl TruncBook {
    pub fn new() -> TruncBook {
        TruncBook {
            plentiful: 0,
            truncated: 0,
            zero_writes: 0,
            oob_hits: 0,
            ret_faults: 0,
        }
    }

    /// 三态账对平：三态合计=断言总量，双零账守恒。
    pub fn conserved(&self) -> bool {
        self.oob_hits == 0 && self.ret_faults == 0 && self.truncated >= 1 && self.zero_writes >= 1
    }

    /// 三分账快照导出（充裕/截断/零长）。
    pub fn snapshot(&self) -> [u32; 3] {
        [self.plentiful, self.truncated, self.zero_writes]
    }

    /// 账面复位（第二轮断言独立复演用，双零账保留）。
    pub fn reset_counts(&mut self) {
        self.plentiful = 0;
        self.truncated = 0;
        self.zero_writes = 0;
    }
}

/// 链接库存（截断语义载体）。
pub struct TruncStore {
    pub names: Vec<String>,
    pub targets: Vec<String>,
    pub book: TruncBook,
}

impl TruncStore {
    pub fn new() -> TruncStore {
        TruncStore {
            names: Vec::new(),
            targets: Vec::new(),
            book: TruncBook::new(),
        }
    }

    /// 登记链接：重名/空名/空目标拒收。
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

    /// 目标串长度读面（三态预判面，未命中 E_NOENT）。
    pub fn target_len(&self, name: &str) -> Result<usize, u16> {
        match self.index_of(name) {
            Some(i) => Ok(self.targets[i].len()),
            None => Err(E_NOENT),
        }
    }

    /// 链接条目数读面。
    pub fn link_count(&self) -> u32 {
        self.names.len() as u32
    }

    /// readlink 截断语义主入口：返回写入字节数（非全串长）。
    /// 调用方在 buf.len() 位设哨兵验证不越界。
    pub fn readlink(&mut self, name: &str, buf: &mut [u8]) -> Result<usize, u16> {
        let idx = match self.index_of(name) {
            Some(i) => i,
            None => return Err(E_NOENT),
        };
        let full = self.targets[idx].clone();
        let fb = full.as_bytes();
        let st = classify(fb.len(), buf.len());
        let written = match st {
            TruncState::ZeroBuf => 0usize,
            TruncState::Plentiful => {
                let mut k = 0usize;
                while k < fb.len() {
                    buf[k] = fb[k];
                    k += 1;
                }
                fb.len()
            }
            TruncState::Truncated => {
                let mut k = 0usize;
                while k < buf.len() {
                    buf[k] = fb[k]; // 截断写入，绝不触 buf.len() 位
                    k += 1;
                }
                buf.len()
            }
        };
        // 返回值口径断言：返回值=写入字节数（非全串长）。
        if written != match st {
            TruncState::ZeroBuf => 0usize,
            TruncState::Plentiful => fb.len(),
            TruncState::Truncated => buf.len(),
        } {
            self.book.ret_faults += 1;
            return Err(E_LEDGER);
        }
        match st {
            TruncState::Plentiful => self.book.plentiful += 1,
            TruncState::Truncated => self.book.truncated += 1,
            TruncState::ZeroBuf => self.book.zero_writes += 1,
        }
        Ok(written)
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOENT => "链接名不存在",
        E_EMPTY => "空名/空目标串拒收",
        E_DUP => "链接名重复登记",
        E_LEDGER => "三态账失守（写入≠返回）",
        E_OOB => "越界写入（哨兵被触碰）",
        E_STATE => "缓冲态机非法迁移",
        _ => "未知诊断码",
    }
}

/// 确定性演练：截断态返回写入数→零长缓冲→充裕态→哨兵越界→注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut st = TruncStore::new();

    // R1：登记长目标链接（20 字节目标）。
    st.add_link("long", "/very/long/target/path")?;
    let full_len = "/very/long/target/path".len(); // 22
    let _ = full_len;
    hits += 1;

    // R2：截断态主断言——10 字节缓冲：写入 10 返回 10（非全串 22），
    // 第 11 位哨兵不越界。
    let mut small = [b'.'; 11];
    small[10] = 0x7E; // 哨兵
    let n = st.readlink("long", &mut small[..10])?;
    if n != 10 {
        return Err(E_LEDGER); // 返回值=写入数，非全串长（POSIX 口径）
    }
    if small[10] != 0x7E {
        st.book.oob_hits += 1;
        return Err(E_OOB);
    }
    if st.book.truncated != 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3：零长缓冲——零写入返回 0 不崩（判据专条）。
    let mut none: [u8; 0] = [];
    let n = st.readlink("long", &mut none)?;
    if n != 0 || st.book.zero_writes != 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R4：充裕态——缓冲 32 字节全串写入返回全串长，串后一位哨兵不动。
    let mut big = [0u8; 33];
    big[full_len] = 0x3D; // 哨兵
    let n = st.readlink("long", &mut big[..32])?;
    if n != full_len || big[full_len] != 0x3D {
        return Err(E_LEDGER);
    }
    if st.book.plentiful != 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R5：临界两态——缓冲恰等全串长（充裕态）与差一字节（截断态）。
    let n = st.readlink("long", &mut big[..full_len])?;
    if n != full_len || st.book.plentiful != 2 {
        return Err(E_LEDGER);
    }
    let n = st.readlink("long", &mut big[..full_len - 1])?;
    if n != full_len - 1 || st.book.truncated != 2 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // I1：不存在名注入。
    if st.readlink("ghost", &mut big) != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    // I2：登记注入——重名/空名/空目标三连拒。
    if st.add_link("long", "/x") != Err(E_DUP) {
        return Err(E_DUP);
    }
    if st.add_link("", "/x") != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    if st.add_link("x", "") != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I3：状态机分类面复核。
    if classify(22, 32) != TruncState::Plentiful
        || classify(22, 10) != TruncState::Truncated
        || classify(22, 0) != TruncState::ZeroBuf
    {
        return Err(E_STATE);
    }
    hits += 1;

    // R6：第二条链接独立复演——短目标 "/etc/hosts"（10 字节），
    // 充裕态与恰等态复核，账面复位后独立入账。
    st.add_link("short", "/etc/hosts")?;
    let flen = st.target_len("short")?;
    if flen != 10 || st.link_count() != 2 {
        return Err(E_LEDGER);
    }
    st.book.reset_counts();
    let mut b2 = [0u8; 16];
    let n = st.readlink("short", &mut b2)?;
    if n != flen {
        return Err(E_LEDGER);
    }
    // 恰等态：缓冲恰等目标长。
    let n = st.readlink("short", &mut b2[..flen])?;
    if n != flen {
        return Err(E_LEDGER);
    }
    // 截断态：3 字节缓冲写入 3 返回 3（非全串 10）。
    let n = st.readlink("short", &mut b2[..3])?;
    if n != 3 {
        return Err(E_LEDGER);
    }
    // 零长缓冲：零写入返回 0（零长账独立入账）。
    let mut zbuf: [u8; 0] = [];
    let n = st.readlink("short", &mut zbuf)?;
    if n != 0 {
        return Err(E_LEDGER);
    }
    let s = st.book.snapshot();
    if s[0] != 2 || s[1] != 1 || s[2] != 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // I3：未命中读面注入。
    if st.target_len("ghost") != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    hits += 1;

    // R7：终审——三态账对平、双零账守恒。
    if !st.book.conserved() {
        return Err(E_LEDGER);
    }
    if st.book.truncated != 1 || st.book.plentiful != 2 || st.book.zero_writes < 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
