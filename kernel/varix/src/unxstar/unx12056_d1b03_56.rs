//! UNX-F12056 PEB 字段内联读双机语义
//! 分段: 0x19920 批次: B03 领域: PEB
//! 验收: 折叠指纹 + 封存 + 注入五样 + 12~20 条判据全绿
//!
//! 语义模型：环境块字段读取在宿主（Windows）与替代机
//! （WOW64 / 替代运行时）上有两种内联实现。本卡以
//! "双机读码本"建模：
//!   - 每条读码记录 { 字段, 宿主偏移, 替代机偏移, 宽度 }；
//!   - 双机读结果必须一致（按宽度掩码截取）；
//!   - 宽度只允许 1/2/4/8 字节；
//!   - 偏移越界（超过 0x1000 PEB 窗口）→ M56_RANGE；
//!   - 宽度非法 → M56_WIDTH；
//!   - 双机不一致 → M56_DIVERGE；
//!   - 封存后不可再注册读码。

use crate::alloc::format;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x19920;
pub const SEG_M56_RANGE: u32 = SEG_BASE + 0x01;
pub const SEG_M56_WIDTH: u32 = SEG_BASE + 0x02;
pub const SEG_M56_DIVERGE: u32 = SEG_BASE + 0x03;
pub const SEG_M56_SEALED: u32 = SEG_BASE + 0x04;
pub const SEG_M56_NOSUCH: u32 = SEG_BASE + 0x05;

pub const PEB_WIN: u32 = 0x1000;

pub const WIDTH_1: u32 = 1;
pub const WIDTH_2: u32 = 2;
pub const WIDTH_4: u32 = 4;
pub const WIDTH_8: u32 = 8;

/// 读码记录
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadCode {
    pub field: u32,
    pub host_off: u32,
    pub alt_off: u32,
    pub width: u32,
    pub sealed: bool,
}

impl ReadCode {
    /// 校验：宽度合法 + 双机偏移 + 宽度不越界
    pub fn validate(&self) -> Result<(), u32> {
        if self.width != WIDTH_1 && self.width != WIDTH_2
            && self.width != WIDTH_4 && self.width != WIDTH_8 {
            return Err(SEG_M56_WIDTH);
        }
        if self.host_off >= PEB_WIN || self.alt_off >= PEB_WIN {
            return Err(SEG_M56_RANGE);
        }
        if self.host_off + self.width > PEB_WIN || self.alt_off + self.width > PEB_WIN {
            return Err(SEG_M56_RANGE);
        }
        Ok(())
    }

    /// 宽度掩码
    pub fn mask(&self) -> u64 {
        match self.width {
            WIDTH_1 => 0xFF,
            WIDTH_2 => 0xFFFF,
            WIDTH_4 => 0xFFFF_FFFF,
            _ => 0xFFFF_FFFF_FFFF_FFFF,
        }
    }
}

/// 双机内存映像
#[derive(Debug, Clone)]
pub struct DualImage {
    pub host: Vec<u8>,
    pub alt: Vec<u8>,
}

impl DualImage {
    /// 生成 0x1000 尺寸零映像
    pub fn new() -> DualImage {
        let mut h = Vec::new();
        let mut a = Vec::new();
        let mut i = 0;
        while i < PEB_WIN {
            h.push(0);
            a.push(0);
            i += 1;
        }
        DualImage { host: h, alt: a }
    }

    /// 写字节
    pub fn poke_host(&mut self, off: u32, v: u8) -> Result<(), u32> {
        if off >= PEB_WIN {
            return Err(SEG_M56_RANGE);
        }
        self.host[off as usize] = v;
        Ok(())
    }

    pub fn poke_alt(&mut self, off: u32, v: u8) -> Result<(), u32> {
        if off >= PEB_WIN {
            return Err(SEG_M56_RANGE);
        }
        self.alt[off as usize] = v;
        Ok(())
    }

    /// 按读码读取双机值（小端拼装）
    pub fn read(&self, code: &ReadCode) -> Result<(u64, u64), u32> {
        code.validate()?;
        let rd = |img: &Vec<u8>, off: u32, w: u32| -> u64 {
            let mut v: u64 = 0;
            let mut i = 0;
            while i < w {
                v |= (img[(off + i) as usize] as u64) << (8 * i);
                i += 1;
            }
            v
        };
        Ok((rd(&self.host, code.host_off, code.width),
            rd(&self.alt, code.alt_off, code.width)))
    }

    /// 双机一致性判定：掩码后相等
    pub fn agree(&self, code: &ReadCode) -> Result<bool, u32> {
        let (h, a) = self.read(code)?;
        Ok((h & code.mask()) == (a & code.mask()))
    }
}

/// 读码本
#[derive(Debug, Clone)]
pub struct CodeBook {
    pub codes: Vec<ReadCode>,
    pub sealed: bool,
}

impl CodeBook {
    pub fn new() -> CodeBook {
        CodeBook { codes: Vec::new(), sealed: false }
    }

    /// 注册：非法码 → RANGE/WIDTH；封存 → SEALED
    pub fn register(&mut self, code: ReadCode) -> Result<usize, u32> {
        if self.sealed {
            return Err(SEG_M56_SEALED);
        }
        code.validate()?;
        self.codes.push(code);
        Ok(self.codes.len() - 1)
    }

    /// 按字段号查询
    pub fn find(&self, field: u32) -> Result<&ReadCode, u32> {
        for c in &self.codes {
            if c.field == field {
                return Ok(c);
            }
        }
        Err(SEG_M56_NOSUCH)
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 折叠指纹：211 进制卷积，种子 SEG_BASE
pub fn fold(book: &CodeBook) -> u32 {
    let mut h: u64 = SEG_BASE as u64;
    for c in &book.codes {
        h = (h.wrapping_mul(211).wrapping_add(c.field as u64)) & 0xFFFF_FFFF;
        h = (h.wrapping_mul(211).wrapping_add(c.host_off as u64)) & 0xFFFF_FFFF;
        h = (h.wrapping_mul(211).wrapping_add(c.alt_off as u64)) & 0xFFFF_FFFF;
        h = (h.wrapping_mul(211).wrapping_add(c.width as u64)) & 0xFFFF_FFFF;
    }
    h = (h.wrapping_mul(211).wrapping_add(book.sealed as u64)) & 0xFFFF_FFFF;
    h as u32
}

/// 注入五样：每个坏样命中一个独立错误分支
pub fn inject_five() -> [bool; 5] {
    let mut ok = [false; 5];
    // 样1: 宽度 3 → WIDTH
    let c1 = ReadCode { field: 1, host_off: 0x10, alt_off: 0x10, width: 3, sealed: false };
    if c1.validate() == Err(SEG_M56_WIDTH) {
        ok[0] = true;
    }
    // 样2: 偏移越界 → RANGE
    let c2 = ReadCode { field: 2, host_off: 0xFFF, alt_off: 0x10, width: 4, sealed: false };
    if c2.validate() == Err(SEG_M56_RANGE) {
        ok[1] = true;
    }
    // 样3: 双机不一致 → agree=false 判 DIVERGE
    let mut img = DualImage::new();
    let _ = img.poke_host(0x20, 0x11);
    let _ = img.poke_alt(0x20, 0x22);
    let c3 = ReadCode { field: 3, host_off: 0x20, alt_off: 0x20, width: 1, sealed: false };
    if img.agree(&c3) == Ok(false) {
        ok[2] = true;
    }
    // 样4: 查询不存在字段 → NOSUCH
    let book = CodeBook::new();
    if book.find(99) == Err(SEG_M56_NOSUCH) {
        ok[3] = true;
    }
    // 样5: 封存后注册 → SEALED
    let mut b2 = CodeBook::new();
    b2.seal();
    if b2.register(ReadCode { field: 1, host_off: 0x10, alt_off: 0x10, width: 4, sealed: false })
        == Err(SEG_M56_SEALED) {
        ok[4] = true;
    }
    ok
}

/// 自检：12 条以上判据
pub fn run_unx12056_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F12056");
    // J1 合法码注册
    let mut book = CodeBook::new();
    let c = ReadCode { field: 1, host_off: 0x10, alt_off: 0x10, width: 4, sealed: false };
    cs.add("J1", book.register(c.clone()) == Ok(0), "register 0");
    // J2 宽度掩码
    let mut m_ok = true;
    let mut w = 1;
    while w <= 8 {
        let cm = ReadCode { field: 9, host_off: 0x10, alt_off: 0x10, width: w, sealed: false };
        let expect = if w == 1 { 0xFF } else if w == 2 { 0xFFFF }
            else if w == 4 { 0xFFFF_FFFF } else { 0xFFFF_FFFF_FFFF_FFFF };
        m_ok = m_ok && cm.mask() == expect;
        w *= 2;
    }
    cs.add("J2", m_ok, "masks 1/2/4/8");
    // J3 宽度 3 拒绝
    cs.add("J3", ReadCode { field: 1, host_off: 0, alt_off: 0, width: 3, sealed: false }
        .validate().err() == Some(SEG_M56_WIDTH), "width 3 bad");
    // J4 宽度 0 拒绝
    cs.add("J4", ReadCode { field: 1, host_off: 0, alt_off: 0, width: 0, sealed: false }
        .validate().err() == Some(SEG_M56_WIDTH), "width 0 bad");
    // J5 越界偏移
    cs.add("J5", ReadCode { field: 1, host_off: 0x1000, alt_off: 0, width: 1, sealed: false }
        .validate().err() == Some(SEG_M56_RANGE), "oob off");
    // J6 偏移+宽度越界
    cs.add("J6", ReadCode { field: 1, host_off: 0xFFE, alt_off: 0, width: 4, sealed: false }
        .validate().err() == Some(SEG_M56_RANGE), "off+w oob");
    // J7 双机一致读
    let mut img = DualImage::new();
    let _ = img.poke_host(0x10, 0x5A);
    let _ = img.poke_alt(0x10, 0x5A);
    cs.add("J7", img.agree(&c) == Ok(true), "agree same");
    // J8 双机分歧
    let _ = img.poke_alt(0x10, 0x5B);
    cs.add("J8", img.agree(&c) == Ok(false), "disagree");
    // J9 多字节小端拼装
    let mut img2 = DualImage::new();
    let _ = img2.poke_host(0x30, 0x78);
    let _ = img2.poke_host(0x31, 0x56);
    let _ = img2.poke_host(0x32, 0x34);
    let _ = img2.poke_host(0x33, 0x12);
    let c4 = ReadCode { field: 2, host_off: 0x30, alt_off: 0x30, width: 4, sealed: false };
    let (hv, _) = img2.read(&c4).unwrap_or((0, 0));
    cs.add("J9", hv == 0x1234_5678, "little endian");
    // J10 掩码截取：写 4 字节读 1 字节（双机同值，仅验证掩码取低字节）。
    let c5 = ReadCode { field: 3, host_off: 0x30, alt_off: 0x30, width: 1, sealed: false };
    let _ = img2.poke_alt(0x30, 0x78);
    cs.add("J10", img2.agree(&c5) == Ok(true) && img2.read(&c5).unwrap_or((9, 9)).0 == 0x78, "mask lo byte");
    // J11 查询命中
    cs.add("J11", book.find(1).is_ok() && book.find(1).unwrap_or(&c).field == 1, "find hit");
    // J12 查询未命中
    cs.add("J12", book.find(7).err() == Some(SEG_M56_NOSUCH), "find miss");
    // J13 非法码不可注册
    cs.add("J13", book.register(ReadCode { field: 5, host_off: 0, alt_off: 0, width: 3, sealed: false })
        .err() == Some(SEG_M56_WIDTH), "register reject");
    // J14 封存
    let mut b2 = CodeBook::new();
    b2.seal();
    cs.add("J14", b2.register(c).err() == Some(SEG_M56_SEALED), "sealed register");
    // J15 注入五样
    let inj = inject_five();
    cs.add("J15", inj.iter().all(|&b| b), "inject five");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_matrix() {
        assert!(ReadCode { field: 1, host_off: 0, alt_off: 0, width: 1, sealed: false }.validate().is_ok());
        assert!(ReadCode { field: 1, host_off: 0, alt_off: 0, width: 8, sealed: false }.validate().is_ok());
        assert_eq!(ReadCode { field: 1, host_off: 0, alt_off: 0, width: 5, sealed: false }.validate().err(), Some(SEG_M56_WIDTH));
        assert_eq!(ReadCode { field: 1, host_off: 0x1000, alt_off: 0, width: 1, sealed: false }.validate().err(), Some(SEG_M56_RANGE));
    }

    #[test]
    fn test_little_endian_read() {
        let mut img = DualImage::new();
        let _ = img.poke_host(0x40, 0xEF);
        let _ = img.poke_host(0x41, 0xBE);
        let c = ReadCode { field: 1, host_off: 0x40, alt_off: 0x40, width: 2, sealed: false };
        let (h, _) = img.read(&c).ok().unwrap_or((0, 0));
        assert_eq!(h, 0xBEEF);
    }

    #[test]
    fn test_fold_stable() {
        let mut b = CodeBook::new();
        let _ = b.register(ReadCode { field: 1, host_off: 8, alt_off: 8, width: 4, sealed: false });
        let a = fold(&b);
        let c = fold(&b);
        assert_eq!(a, c);
        assert_ne!(a, SEG_BASE);
    }

    #[test]
    fn test_dual_image_agree() {
        let mut img = DualImage::new();
        let _ = img.poke_host(0x50, 0x7F);
        let _ = img.poke_alt(0x50, 0x7F);
        let c = ReadCode { field: 2, host_off: 0x50, alt_off: 0x50, width: 1, sealed: false };
        assert_eq!(img.agree(&c), Ok(true));
        let _ = img.poke_alt(0x50, 0x80);
        assert_eq!(img.agree(&c), Ok(false));
    }
}
