//! unx0274_ascii —— UNX-F0274 终端无障碍（纯 ASCII 降级模式）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0274 · 纯 ASCII 降级 · 目标 180 行
//! 判据原文：UNX-F0274-J1 降级模式核：装饰字符（框线/图标）映射
//!           到 ASCII 等价物（映射表闭集覆盖）；控制字节（<0x20
//!           除 \n\r\t）剥离；降级输出可逆性不做承诺但**可判别**
//!           （输出串不含非 ASCII 字节）。
//! 组件：映射表（Unicode 装饰点 → ASCII 串）+ 降级引擎（逐字
//!       映射 + 控制剥离）+ 判别核（输出全 ASCII + 剥离账）。
//! 职责定位：降级是「可访问性的底线」——盲文终端/硬拷贝打印
//!   只认 ASCII；降级必须**全覆盖**（映射表外的装饰字符宁可
//!   扁平化为 '?' 也不许漏出非 ASCII 字节——漏一个字节，整屏
//!   乱码）。
//! 性能逐项分解：降级 O(串长)；判别 O(串长)。
//! 诊断码段：0x203C0（0x203C1~0x203C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const AD_VERSION: &str = "unx0274_ascii-v1";

/// 诊断段基址（0x203C0 段 · 本模块独占）。
pub const AD_SEG_BASE: u32 = 0x203C0;
pub const XC_NON_ASCII: u32 = 0x203C1; // 输出含非 ASCII
pub const XC_CTRL_PASSED: u32 = 0x203C2; // 控制字节漏出
pub const XC_EMPTY: u32 = 0x203C3; // 空串降级拒
pub const XC_MAP_MISS: u32 = 0x203C4; // 映射缺失（不应发生）
pub const XC_LINE_OOR: u32 = 0x203C5; // 行数越域
pub const XC_STR_LONG: u32 = 0x203C6; // 串超长
pub const XC_TAB_WIDE: u32 = 0x203C7; // 制表宽度越域
pub const XC_NUL: u32 = 0x203C8; // NUL 拒
pub const XC_MODE_OOR: u32 = 0x203C9; // 模式越域

/// 串长上限。
pub const STR_MAX: usize = 256;

/// 装饰字符 Unicode 码点 → ASCII 等价物（映射表闭集）。
pub fn decor_map(cp: u32) -> Option<&'static str> {
    match cp {
        0x2500 => Some("-"),   // ─
        0x2502 => Some("|"),   // │
        0x250C => Some("+"),   // ┌
        0x2510 => Some("+"),   // ┐
        0x2514 => Some("+"),   // └
        0x2518 => Some("+"),   // ┘
        0x251C => Some("+"),   // ├
        0x2524 => Some("+"),   // ┤
        0x252C => Some("+"),   // ┬
        0x2534 => Some("+"),   // ┴
        0x253C => Some("+"),   // ┼
        0x2190 => Some("<"),   // ←
        0x2192 => Some(">"),   // →
        0x2191 => Some("^"),   // ↑
        0x2193 => Some("v"),   // ↓
        0x2713 => Some("*"),   // ✓
        0x2717 => Some("x"),   // ✗
        0x2022 => Some("."),   // •
        0x2026 => Some("..."), // …
        0x00A0 => Some(" "),   // 不换行空格
        _ => None,
    }
}

/// UTF-8 解码单码点（简化：返回 (码点, 字节长)；非法序列 → (0xFFFD, 1)）。
pub fn utf8_next(bytes: &[u8]) -> (u32, usize) {
    if bytes.is_empty() {
        return (0xFFFD, 0);
    }
    let b0 = bytes[0];
    if b0 < 0x80 {
        return (b0 as u32, 1);
    }
    let (need, mut cp) = if b0 & 0xE0 == 0xC0 {
        (1usize, (b0 & 0x1F) as u32)
    } else if b0 & 0xF0 == 0xE0 {
        (2usize, (b0 & 0x0F) as u32)
    } else if b0 & 0xF8 == 0xF0 {
        (3usize, (b0 & 0x07) as u32)
    } else {
        return (0xFFFD, 1);
    };
    if bytes.len() < need + 1 {
        return (0xFFFD, 1);
    }
    for i in 1..=need {
        let b = bytes[i];
        if b & 0xC0 != 0x80 {
            return (0xFFFD, 1);
        }
        cp = (cp << 6) | (b & 0x3F) as u32;
    }
    (cp, need + 1)
}

/// 控制字节剥离判定（保留 \n \r \t，其余 <0x20 剥；NUL 恒拒）。
pub fn ctrl_allowed(cp: u32) -> Result<(), u32> {
    if cp == 0 {
        return Err(XC_NUL);
    }
    if cp < 0x20 && cp != 0x0A && cp != 0x0D && cp != 0x09 {
        return Ok(()); // 可剥（由降级引擎剥除）
    }
    Ok(())
}

/// 降级引擎：装饰映射 + 控制剥离 + 未映射非 ASCII 扁平化 '?'。
/// 输出保证全 ASCII（判别核可验）。
pub fn degrade(bytes: &[u8]) -> Result<Vec<u8>, u32> {
    if bytes.is_empty() {
        return Err(XC_EMPTY);
    }
    if bytes.len() > STR_MAX * 4 {
        return Err(XC_STR_LONG);
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let (cp, adv) = utf8_next(&bytes[i..]);
        if adv == 0 {
            break;
        }
        if cp == 0 {
            return Err(XC_NUL);
        }
        if cp < 0x20 && cp != 0x0A && cp != 0x0D && cp != 0x09 {
            // 控制字节剥除（不计入输出）。
            i += adv;
            continue;
        }
        if cp < 0x80 {
            out.push(cp as u8);
        } else if let Some(s) = decor_map(cp) {
            out.extend_from_slice(s.as_bytes());
        } else {
            out.push(b'?'); // 未映射非 ASCII 扁平化——不漏字节
        }
        i += adv;
    }
    if out.is_empty() {
        return Err(XC_EMPTY);
    }
    // 自检：输出全 ASCII（结构保证，此处判别核双确认）。
    if out.iter().any(|&b| b >= 0x80) {
        return Err(XC_NON_ASCII);
    }
    Ok(out)
}

/// 判别核：给定串是否全 ASCII（0x20~0x7E + 允许控制）。
pub fn all_ascii(bytes: &[u8]) -> bool {
    bytes.iter().all(|&b| b < 0x80)
}

pub fn run_unx0274_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0274_ascii");
    // 族一：映射表闭集。
    for cp in [0x2500u32, 0x2502, 0x250C, 0x2514, 0x2190, 0x2192, 0x2713, 0x2026] {
        cs.add_decor(cp);
    }
    cs.add("映射闭集 20", {
        let mut n = 0;
        for cp in 0..0x3000u32 {
            if decor_map(cp).is_some() {
                n += 1;
            }
        }
        n == 20
    }, "枚举全域恰 20");
    // 族二：框线降级。
    let box_line: &[u8] = &[
        0xE2, 0x94, 0x8C, // ┌
        0xE2, 0x94, 0x80, // ─
        0xE2, 0x94, 0x90, // ┐
    ];
    let d1 = degrade(box_line);
    cs.add("框线降级", d1 == Ok(b"+-+".to_vec()), "┌─┐→+-+");
    // 族三：箭头与省略。
    let arrow: &[u8] = &[0xE2, 0x86, 0x90, 0xE2, 0x80, 0xA6]; // ←…
    cs.add("箭头省略", degrade(arrow) == Ok(b"<...".to_vec()), "←…→<...");
    // 族四：控制剥离。
    let ctrl: &[u8] = &[0x41, 0x01, 0x42, 0x07, 0x43]; // A BEL B BEL C
    cs.add("控制剥离", degrade(ctrl) == Ok(b"ABC".to_vec()), "BEL 被剥");
    let nl: &[u8] = &[0x41, 0x0A, 0x42];
    cs.add("换行保留", degrade(nl) == Ok(b"A\nB".to_vec()), "\\n 保留");
    cs.add("NUL 拒", degrade(&[0x41, 0x00, 0x42]).is_err(), "NUL");
    // 族五：未映射扁平化。
    let unknown: &[u8] = &[0xE2, 0x98, 0x83]; // ☃ 不在映射表
    cs.add("未知扁平化", degrade(unknown) == Ok(b"?".to_vec()), "☃→?");
    // 族六：判别核。
    cs.add("全 ASCII 判别", all_ascii(b"+-+"), "映射产物");
    cs.add("混合判别", !all_ascii(&[0xE2, 0x94]), "UTF-8 尾字节");
    cs.add("空串拒", degrade(&[]).is_err(), "EMPTY");
    cs.add("纯控制拒", degrade(&[0x01, 0x02]).is_err(), "剥空后拒");
    cs
}

/// 判据辅助：装饰映射命中（供映射闭集逐点登记）。
trait DecorCheck {
    fn add_decor(&mut self, cp: u32);
}
impl DecorCheck for CheckSet {
    fn add_decor(&mut self, cp: u32) {
        let hit = decor_map(cp).is_some();
        self.add("映射命中", hit, "闭集点");
    }
}
