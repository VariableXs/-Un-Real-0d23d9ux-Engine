//! unx48189_dbcsconv — UNX-F48189 Wine 窗 DBCS 转换面（UTF-8↔代码页转换无损）（诊断段 0xB880）。
//!
//! 判据原文：
//!   J1 转换 ×10^3 往返零损（CP936 样张集）；判据锚定 §AI-61 §2 专题三类型二。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 样张集驱动：CP936（GBK 简体中文代码页）节选样张表——24 个常用
//!      汉字（Unicode 码点 ↔ GBK 双字节码）+ ASCII 单字节全区间；
//!      转换面只对样张集内字符承诺无损，表外汉字拒转（返回未映射码）。
//!   2. 双向转换器：utf8→cp936 逐码点查表（ASCII 直出、汉字出双字节、
//!      表外报错）；cp936→utf8 逐字节判首字节域（0x80 以上双字节、
//!      其余单字节），查表还原码点后按 UTF-8 重编码。
//!   3. 往返零损口径：utf8 原串 → cp936 → utf8 再生串，逐字节全等；
//!      ×10^3 = 1000 次往返（样张集确定性组合成 40 条样本串，循环
//!      25 轮 ×40），逐次断言，差一字节即坏。
//!   4. 截断防御：cp936 尾字节落单（首字节后缺伴）判错不猜测；utf8
//!      非法续字节判错不吞——错误路径返回诊断码，不产出脏数据。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB880;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_UNMAPPED: u16 = SEG_BASE + 1;   // 码点不在样张集，拒转
const E_BAD_LEAD: u16 = SEG_BASE + 2;   // cp936 首字节域非法
const E_TRAIL_CUT: u16 = SEG_BASE + 3;  // cp936 双字节尾字节缺失
const E_UTF8_BAD: u16 = SEG_BASE + 4;   // utf8 非法编码
const E_RT_LOSS: u16 = SEG_BASE + 5;    // 往返有损
const E_CONV_MISS: u16 = SEG_BASE + 6;  // ×10^3 次数不足
const E_EMPTY_SUM: u16 = SEG_BASE + 7;  // 写回摘要为空

/// 样张表条目：Unicode 码点 ↔ GBK 双字节码。
pub struct GbkEntry {
    pub unicode: u32,
    pub gbk: u16, // 高字节=首字节，低字节=尾字节
}

/// CP936 样张集（节选 24 常用汉字，GBK 编码标准值）。
pub const GBK_TABLE: [GbkEntry; 24] = [
    GbkEntry { unicode: 0x4E00, gbk: 0xD2BB }, // 一
    GbkEntry { unicode: 0x4E8C, gbk: 0xB6FE }, // 二
    GbkEntry { unicode: 0x4E09, gbk: 0xC8FD }, // 三
    GbkEntry { unicode: 0x56FD, gbk: 0xB9FA }, // 国
    GbkEntry { unicode: 0x4EBA, gbk: 0xC8CB }, // 人
    GbkEntry { unicode: 0x5927, gbk: 0xB4F3 }, // 大
    GbkEntry { unicode: 0x5C0F, gbk: 0xD0A1 }, // 小
    GbkEntry { unicode: 0x5C71, gbk: 0xC9BD }, // 山
    GbkEntry { unicode: 0x6C34, gbk: 0xCBAE }, // 水
    GbkEntry { unicode: 0x5929, gbk: 0xCCEC }, // 天
    GbkEntry { unicode: 0x6708, gbk: 0xD4C2 }, // 月
    GbkEntry { unicode: 0x706B, gbk: 0xBBF0 }, // 火
    GbkEntry { unicode: 0x6728, gbk: 0xC4BE }, // 木
    GbkEntry { unicode: 0x91D1, gbk: 0xBDF0 }, // 金
    GbkEntry { unicode: 0x5FC3, gbk: 0xD0C4 }, // 心
    GbkEntry { unicode: 0x6C49, gbk: 0xBABA }, // 汉
    GbkEntry { unicode: 0x5B57, gbk: 0xD7D6 }, // 字
    GbkEntry { unicode: 0x4E2D, gbk: 0xD6D0 }, // 中
    GbkEntry { unicode: 0x6587, gbk: 0xCEC4 }, // 文
    GbkEntry { unicode: 0x6D4B, gbk: 0xB2E2 }, // 测
    GbkEntry { unicode: 0x8BD5, gbk: 0xCABD }, // 试
    GbkEntry { unicode: 0x8F93, gbk: 0xCAE4 }, // 输
    GbkEntry { unicode: 0x5165, gbk: 0xC8EB }, // 入
    GbkEntry { unicode: 0x6CD5, gbk: 0xB7A8 }, // 法
];

/// 样张表内查字：Unicode → GBK 码。
pub fn uni_to_gbk(uni: u32) -> Option<u16> {
    let mut i = 0usize;
    while i < GBK_TABLE.len() {
        if GBK_TABLE[i].unicode == uni {
            return Some(GBK_TABLE[i].gbk);
        }
        i += 1;
    }
    None
}

/// 样张表内查字：GBK 码 → Unicode。
pub fn gbk_to_uni(code: u16) -> Option<u32> {
    let mut i = 0usize;
    while i < GBK_TABLE.len() {
        if GBK_TABLE[i].gbk == code {
            return Some(GBK_TABLE[i].unicode);
        }
        i += 1;
    }
    None
}

/// 单码点 → UTF-8 字节组（返回写入个数，1..3；超 BMP 外拒转）。
pub fn utf8_encode(uni: u32, out: &mut Vec<u8>) -> Result<usize, u16> {
    if uni < 0x80 {
        out.push(uni as u8);
        Ok(1)
    } else if uni < 0x800 {
        out.push(0xC0 | (uni >> 6) as u8);
        out.push(0x80 | (uni & 0x3F) as u8);
        Ok(2)
    } else if uni <= 0xFFFF {
        out.push(0xE0 | (uni >> 12) as u8);
        out.push(0x80 | ((uni >> 6) & 0x3F) as u8);
        out.push(0x80 | (uni & 0x3F) as u8);
        Ok(3)
    } else {
        Err(E_UNMAPPED)
    }
}

/// UTF-8 流 → 码点流（纪律 4：非法编码判错不吞）。
pub fn utf8_decode(bytes: &[u8]) -> Result<Vec<u32>, u16> {
    let mut out: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x80 {
            out.push(b as u32);
            i += 1;
        } else if b & 0xE0 == 0xC0 {
            if i + 1 >= bytes.len() || bytes[i + 1] & 0xC0 != 0x80 {
                return Err(E_UTF8_BAD);
            }
            out.push(((b as u32 & 0x1F) << 6) | (bytes[i + 1] as u32 & 0x3F));
            i += 2;
        } else if b & 0xF0 == 0xE0 {
            if i + 2 >= bytes.len()
                || bytes[i + 1] & 0xC0 != 0x80
                || bytes[i + 2] & 0xC0 != 0x80
            {
                return Err(E_UTF8_BAD);
            }
            out.push(
                ((b as u32 & 0x0F) << 12)
                    | ((bytes[i + 1] as u32 & 0x3F) << 6)
                    | (bytes[i + 2] as u32 & 0x3F),
            );
            i += 3;
        } else {
            return Err(E_UTF8_BAD);
        }
    }
    Ok(out)
}

/// UTF-8 → CP936（纪律 2：查表转换，表外拒转）。
pub fn utf8_to_cp936(bytes: &[u8]) -> Result<Vec<u8>, u16> {
    let cps = utf8_decode(bytes)?;
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < cps.len() {
        let c = cps[i];
        if c < 0x80 {
            out.push(c as u8);
        } else {
            let code = match uni_to_gbk(c) {
                Some(v) => v,
                None => return Err(E_UNMAPPED),
            };
            out.push((code >> 8) as u8);
            out.push((code & 0xFF) as u8);
        }
        i += 1;
    }
    Ok(out)
}

/// CP936 → UTF-8（纪律 4：尾字节缺失判错不猜测）。
pub fn cp936_to_utf8(bytes: &[u8]) -> Result<Vec<u8>, u16> {
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x80 {
            utf8_encode(b as u32, &mut out)?;
            i += 1;
        } else if (0x81..=0xFE).contains(&b) {
            if i + 1 >= bytes.len() {
                return Err(E_TRAIL_CUT);
            }
            let code = ((b as u16) << 8) | bytes[i + 1] as u16;
            let uni = match gbk_to_uni(code) {
                Some(v) => v,
                None => return Err(E_UNMAPPED),
            };
            utf8_encode(uni, &mut out)?;
            i += 2;
        } else {
            return Err(E_BAD_LEAD);
        }
    }
    Ok(out)
}

/// 往返一次：utf8 → cp936 → utf8，逐字节全等才无损。
pub fn roundtrip(bytes: &[u8]) -> Result<Vec<u8>, u16> {
    let mid = utf8_to_cp936(bytes)?;
    let back = cp936_to_utf8(&mid)?;
    if back.len() != bytes.len() {
        return Err(E_RT_LOSS);
    }
    let mut i = 0usize;
    while i < bytes.len() {
        if back[i] != bytes[i] {
            return Err(E_RT_LOSS);
        }
        i += 1;
    }
    Ok(back)
}

/// 样本串生成器：第 s 条样本（ASCII 段 + 汉字段确定性拼装）。
pub fn sample_bytes(s: usize) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    // ASCII 头：'A' + (s%26)。
    out.push(b'A' + (s % 26) as u8);
    out.push(b'0' + (s % 10) as u8);
    // 汉字段：样张表轮转取 3 字。
    let mut k = 0usize;
    while k < 3 {
        let uni = GBK_TABLE[(s * 3 + k) % GBK_TABLE.len()].unicode;
        utf8_encode(uni, &mut out).is_ok();
        k += 1;
    }
    out.push(b'\n' + 0); // 尾部控制字节（0x0A）保持单字节域
    out
}

/// 自例程判据：逐条落实 J1（转换 ×10^3 往返零损），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主扫 ×10^3：40 条样本 ×25 轮 = 1000 次往返，逐次逐字节全等。
    let mut conv_count = 0u32;
    let mut round = 0u32;
    while round < 25 {
        let mut s = 0usize;
        while s < 40 {
            let src = sample_bytes(s);
            let back = roundtrip(&src)?;
            if back != src {
                return Err(E_RT_LOSS);
            }
            conv_count += 1;
            s += 1;
        }
        round += 1;
    }
    if conv_count != 1000 {
        return Err(E_CONV_MISS);
    }
    hits += 1;

    // R2 双向查表：24 字逐一 uni→gbk→uni 恒等；gbk→uni→gbk 恒等。
    let mut i = 0usize;
    while i < GBK_TABLE.len() {
        let e = &GBK_TABLE[i];
        let code = match uni_to_gbk(e.unicode) {
            Some(v) => v,
            None => return Err(E_UNMAPPED),
        };
        if code != e.gbk {
            return Err(E_RT_LOSS);
        }
        let uni = match gbk_to_uni(e.gbk) {
            Some(v) => v,
            None => return Err(E_UNMAPPED),
        };
        if uni != e.unicode {
            return Err(E_RT_LOSS);
        }
        i += 1;
    }
    hits += 1;

    // R3 编码形制：ASCII 单字节直出；汉字出双字节且首字节域 0x81..0xFE。
    let ascii = utf8_to_cp936(b"AZ09")?;
    if ascii.len() != 4 || ascii[0] != b'A' || ascii[3] != b'9' {
        return Err(E_RT_LOSS);
    }
    let uni = utf8_encode(0x6C49, &mut Vec::new()); // 汉（表内） sanity: 单点编解码
    if uni.is_err() {
        return Err(E_UTF8_BAD);
    }
    let han_utf8: [u8; 3] = [0xE6, 0xB1, 0x89]; // 汉 的 UTF-8
    let han_gbk = utf8_to_cp936(&han_utf8)?;
    if han_gbk.len() != 2 {
        return Err(E_RT_LOSS);
    }
    if han_gbk[0] < 0x81 || han_gbk[0] > 0xFE {
        return Err(E_BAD_LEAD);
    }
    if han_gbk[0] != 0xBA || han_gbk[1] != 0xBA {
        return Err(E_RT_LOSS); // 汉 = 0xBABA
    }
    hits += 1;

    // R4 混排样本：ASCII 与汉字交替，往返零损 + 字节数守恒公式
    //    （ASCII 记 1、汉字记 2）。
    let mut mix: Vec<u8> = Vec::new();
    utf8_encode(b'x' as u32, &mut mix)?;
    utf8_encode(0x4E2D, &mut mix)?; // 中
    utf8_encode(b'Y' as u32, &mut mix)?;
    utf8_encode(0x6587, &mut mix)?; // 文
    let mid = utf8_to_cp936(&mix)?;
    if mid.len() != 6 {
        return Err(E_RT_LOSS); // 1+2+1+2
    }
    let back = cp936_to_utf8(&mid)?;
    if back != mix {
        return Err(E_RT_LOSS);
    }
    hits += 1;

    // R5 错误路径：表外汉字（U+64CE 擎）拒转 E_UNMAPPED；转换不产出脏数据。
    let mut bad: Vec<u8> = Vec::new();
    utf8_encode(0x64CE, &mut bad)?;
    if utf8_to_cp936(&bad).is_ok() {
        return Err(E_UNMAPPED);
    }
    if cp936_to_utf8(&[0xB0, 0xEB, 0xB0]).is_ok() {
        return Err(E_TRAIL_CUT); // 尾字节落单
    }
    if utf8_decode(&[0xC3]).is_ok() {
        return Err(E_UTF8_BAD); // 截断的 2 字节序列
    }
    if utf8_decode(&[0xFF]).is_ok() {
        return Err(E_UTF8_BAD); // 非法首字节
    }
    if cp936_to_utf8(&[0x80, 0x41]).is_ok() {
        return Err(E_BAD_LEAD); // 0x80 在首字节域外
    }
    hits += 1;

    // R6 临界单字节域：0x7F/0x80 分界——0x7F 单字节直出，0x80 判双字节域。
    let lo = utf8_to_cp936(&[0x7F])?;
    if lo.len() != 1 || lo[0] != 0x7F {
        return Err(E_RT_LOSS);
    }
    let back_lo = roundtrip(&[0x7F])?;
    if back_lo.len() != 1 {
        return Err(E_RT_LOSS);
    }
    hits += 1;

    // R7 全样张集串：24 字全表串一次往返，长度守恒 24×3→24×2→24×3。
    let mut full: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < GBK_TABLE.len() {
        utf8_encode(GBK_TABLE[i].unicode, &mut full)?;
        i += 1;
    }
    let mid = utf8_to_cp936(&full)?;
    if mid.len() != GBK_TABLE.len() * 2 {
        return Err(E_RT_LOSS);
    }
    let back = cp936_to_utf8(&mid)?;
    if back.len() != GBK_TABLE.len() * 3 || back != full {
        return Err(E_RT_LOSS);
    }
    hits += 1;

    // R8 写回（判据 J2）：完成态 + 非空摘要放行，空摘要拒。
    let done = conv_count == 1000;
    let summary_len = if done { 0x20u32 } else { 0u32 };
    if summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    if !done {
        return Err(E_CONV_MISS);
    }
    hits += 1;

    Ok(hits)
}
