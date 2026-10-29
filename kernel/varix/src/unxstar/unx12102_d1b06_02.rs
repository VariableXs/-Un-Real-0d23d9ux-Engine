//! UNX-F12102 Length/MaximumLength/NUL 约定判据（0x1a160→0x19ee0 段）
//!
//! 卡面：三量约定——NUL 不计入 Length、MaximumLength 覆写 NUL 位、
//! 追加须留 NUL 容、容量上限 0xFFFE；账式核逐条过闸。
//! 域：CoRun·UNX-D1-B06。
//!
//! 判据原文：
//! - J1 NUL 不计长：以字符数初始化时 len = n×2，NUL 不在长内。
//! - J2 容量上限：len + 2 > 0xFFFE 时初始化必拒。
//! - J3 NUL 计长拒：len 恰等于 max（NUL 被计入）必拒。
//! - J4 追加留容：追加 n 字符须 len + n×2 + 2 ≤ max，否则必拒。
//! - J5 手工置长：set_len 须偶且 ≤ max − 2，违者必拒。
//! - J6 印账冻结：seal 后操作必拒。
//! - J7 注入五况全抓。
//!
//! 结构：NulBuf 缓冲账（len/max 双量）+ 操作核（init/append/set_len）+
//! 折叠指纹（211 进制卷积）+ 注入核（超上限/NUL 计长/追加无容/坏置长/
//! 印后操作）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19ee0 独占 0x20）。
pub const SEG_BASE: u32 = 0x19ee0;
pub const NU_MAX_OVER: u32 = SEG_BASE + 0x01; // 超容量上限
pub const NU_NUL_COUNTED: u32 = SEG_BASE + 0x02; // NUL 被计入
pub const NU_NO_ROOM: u32 = SEG_BASE + 0x03; // 追加无 NUL 容
pub const NU_BAD_SETLEN: u32 = SEG_BASE + 0x04; // 坏置长
pub const NU_FROZEN: u32 = SEG_BASE + 0x05; // 印账后操作

/// 容量上限。
pub const NUL_CAP: u32 = 0xFFFE;

/// 缓冲账。
#[derive(Clone, Copy)]
pub struct NulBuf {
    pub len: u32,
    pub max: u32,
}

impl NulBuf {
    /// 初始化核：len = n×2，max = len + 2；超上限必拒。
    pub fn init(nchars: u32) -> Result<NulBuf, u32> {
        let len = nchars * 2;
        let max = len + 2;
        if max > NUL_CAP {
            return Err(NU_MAX_OVER);
        }
        Ok(NulBuf { len, max })
    }

    /// 显式容量初始化：len = n×2，max 由调用方预留（须容得下 NUL 位且 ≤ 上限）。
    /// 追加留容判据（J4/账式追加）所需的前置构造面。
    pub fn with_max(nchars: u32, max: u32) -> Result<NulBuf, u32> {
        let len = nchars * 2;
        if max > NUL_CAP || len + 2 > max {
            return Err(NU_MAX_OVER);
        }
        Ok(NulBuf { len, max })
    }

    /// NUL 计长判定：len == max 即 NUL 被计入（无 NUL 容）。
    pub fn nul_counted(&self) -> bool {
        self.len >= self.max
    }

    /// 追加核：须留 NUL 容。
    pub fn append(&mut self, nchars: u32) -> Result<(), u32> {
        let add = nchars * 2;
        if self.len + add + 2 > self.max {
            return Err(NU_NO_ROOM);
        }
        self.len += add;
        Ok(())
    }

    /// 手工置长：偶且 ≤ max − 2。
    pub fn set_len(&mut self, len: u32) -> Result<(), u32> {
        if len % 2 != 0 || len + 2 > self.max {
            return Err(NU_BAD_SETLEN);
        }
        self.len = len;
        Ok(())
    }

    /// 自由字节（不含 NUL 位）。
    pub fn free(&self) -> u32 {
        if self.max >= self.len + 2 {
            self.max - self.len - 2
        } else {
            0
        }
    }

    pub fn render(&self) -> String {
        format!("N{}+{}", self.len, self.max)
    }
}

/// 操作账：逐操作留痕。
pub struct NulLedger {
    pub bufs: Vec<(u32, NulBuf)>,
    pub ops: Vec<String>,
    pub sealed: bool,
}

impl NulLedger {
    pub fn new() -> NulLedger {
        NulLedger { bufs: Vec::new(), ops: Vec::new(), sealed: false }
    }

    /// 登记初始化。
    pub fn init_reg(&mut self, id: u32, nchars: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(NU_FROZEN);
        }
        let b = NulBuf::init(nchars)?;
        self.bufs.push((id, b));
        self.ops.push(format!("init{}", id));
        Ok(())
    }

    /// 登记初始化（显式容量预留，供追加留容样例）。
    pub fn init_reg_with(&mut self, id: u32, nchars: u32, max: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(NU_FROZEN);
        }
        let b = NulBuf::with_max(nchars, max)?;
        self.bufs.push((id, b));
        self.ops.push(format!("init{}", id));
        Ok(())
    }

    /// 按号取缓冲。
    pub fn buf_of(&self, id: u32) -> Option<NulBuf> {
        self.bufs.iter().find(|b| b.0 == id).map(|b| b.1)
    }

    /// 按号追加。
    pub fn append_to(&mut self, id: u32, nchars: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(NU_FROZEN);
        }
        let i = match self.bufs.iter().position(|b| b.0 == id) {
            Some(i) => i,
            None => return Err(NU_NO_ROOM), // 未知号按容量面拒
        };
        self.bufs[i].1.append(nchars)?;
        self.ops.push(format!("app{}", id));
        Ok(())
    }

    /// 操作账条数。
    pub fn op_count(&self) -> usize {
        self.ops.len()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for b in &self.bufs {
            h = h.wrapping_mul(211).wrapping_add(b.0);
            h = h.wrapping_mul(211).wrapping_add(b.1.len);
            h = h.wrapping_mul(211).wrapping_add(b.1.max);
        }
        for o in &self.ops {
            for by in o.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("bufs={} ops={}", self.bufs.len(), self.op_count())
    }

    /// 诊断面：缓冲渲染。
    pub fn line_of(&self, id: u32) -> String {
        match self.buf_of(id) {
            Some(b) => b.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 超容量上限必拒（0x7FFF 字符 → max 0x10000 > 0xFFFE）。
    got[0] = NulBuf::init(0x7FFF).err() == Some(NU_MAX_OVER);
    // 2) NUL 计长必拒（len == max 无 NUL 容）。
    got[1] = NulBuf { len: 8, max: 8 }.nul_counted();
    // 3) 追加无容必拒。
    let mut b = NulBuf::init(3).unwrap_or(NulBuf { len: 0, max: 2 });
    got[2] = b.append(9).err() == Some(NU_NO_ROOM);
    // 4) 坏置长（奇长）必拒。
    got[3] = b.set_len(3).err() == Some(NU_BAD_SETLEN);
    // 5) 印账后操作必抓。
    let mut l = NulLedger::new();
    l.seal();
    got[4] = l.init_reg(1, 2).err() == Some(NU_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12102_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12102");

    // J1 NUL 不计长。
    let j1 = {
        let b = NulBuf::init(5).unwrap_or(NulBuf { len: 0, max: 0 });
        b.len == 10 && b.max == 12 && b.free() == 0
    };
    s.add("J1-NUL不计长", j1, "len=n×2、max=len+2");

    // J2 容量上限边界。
    let j2 = {
        let edge = NulBuf::init(0x7FFD); // max = 0xFFFE 恰达
        let over = NulBuf::init(0x7FFF);
        edge.is_ok() && over.err() == Some(NU_MAX_OVER)
    };
    s.add("J2-容量上限", j2, "边界过、越界拒");

    // J3 NUL 计长拒。
    let j3 = {
        let mut b = NulBuf::init(2).unwrap_or(NulBuf { len: 0, max: 0 });
        b.max = b.len; // 伪造 NUL 被计入
        b.nul_counted()
    };
    s.add("J3-NUL计长拒", j3, "len==max 判红");

    // J4 追加留容。
    let j4 = {
        let mut b = NulBuf::with_max(2, 20).unwrap_or(NulBuf { len: 0, max: 0 });
        b.append(3).is_ok() && b.len == 10
            && b.append(99).err() == Some(NU_NO_ROOM)
    };
    s.add("J4-追加留容", j4, "足容放行、无容拒");

    // J5 手工置长。
    let j5 = {
        let mut b = NulBuf { len: 0, max: 16 };
        b.set_len(6).is_ok()
            && b.set_len(5).err() == Some(NU_BAD_SETLEN)
            && b.set_len(16).err() == Some(NU_BAD_SETLEN) // 偶长但侵 NUL 位（16+2>16）
    };
    s.add("J5-手工置长", j5, "奇长与侵 NUL 位皆拒");

    // J6 账式初始化与回读。
    let j6 = {
        let mut l = NulLedger::new();
        let _ = l.init_reg(1, 3);
        l.buf_of(1).map(|b| b.len) == Some(6)
            && l.line_of(2) == "missing"
    };
    s.add("J6-账式回读", j6, "按号可查");

    // J7 账式追加。
    let j7 = {
        let mut l = NulLedger::new();
        let _ = l.init_reg_with(1, 2, 16);
        let _ = l.append_to(1, 2);
        l.buf_of(1).map(|b| b.len) == Some(8) && l.op_count() == 2
    };
    s.add("J7-账式追加", j7, "追加入账");

    // J8 渲染行。
    let j8 = {
        let b = NulBuf::init(4).unwrap_or(NulBuf { len: 0, max: 0 });
        b.render() == "N8+10"
    };
    s.add("J8-渲染行", j8, "N<len>+<max> 口径");

    // J9 印账冻结。
    let j9 = {
        let mut l = NulLedger::new();
        let _ = l.init_reg(1, 1);
        l.seal();
        l.init_reg(2, 1).err() == Some(NU_FROZEN)
            && l.append_to(1, 1).err() == Some(NU_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后皆拒");

    // J10 指纹幂等。
    let j10 = {
        let mut l = NulLedger::new();
        let _ = l.init_reg(1, 1);
        l.fold() == l.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut l = NulLedger::new();
    let _ = l.init_reg_with(1, 6, 24);
    let _ = l.append_to(1, 2);

    s.add("摘要回读", l.summary().contains("bufs=1 ops=2"), "一缓冲两操作");
    s.add("指纹非零", l.fold() != 0, "指纹有效");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12102_nul_flow() {
        let mut b = NulBuf::init(2).unwrap_or(NulBuf { len: 0, max: 0 });
        assert!(b.append(2).is_ok());
        assert_eq!(b.append(99), Err(NU_NO_ROOM));
        assert!(b.set_len(2).is_ok());
        assert_eq!(b.set_len(3), Err(NU_BAD_SETLEN));
    }

    #[test]
    fn unx12102_checks_green() {
        let s = run_unx12102_checks();
        assert!(s.all_passed(), "UNX-F12102 判据未全绿");
    }

    #[test]
    fn init_overflow_guard() {
        assert!(NulBuf::init(0x7FFE).is_err());
        assert!(NulBuf::init(0).is_ok());
    }
}
