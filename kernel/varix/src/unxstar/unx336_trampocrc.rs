//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0336 · trampoline 完整性校验（跳板代码 CRC）· 诊断段 0x3ef0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   UNX-F0336-J1 跳板代码 CRC 落位前校验，篡改检出 100%。
//! 难点分析：
//!   trampoline 是 AP 醒来跑的第一段代码——它被篡改时没有内核在
//!   看着（内核还在 BSP 上初始化）。唯一防线是「落位前校验」：
//!   镜像生成时算一次 CRC（黄金值），落到低 1MB 后再算一次，两者
//!   不等就不许发 SIPI。篡改可能是硬件坏位、固件残留、也可能是
//!   别的组件越界写——CRC 定位不到字节，但能 100% 判「坏了」，
//!   本模块再加二分定位：翻一个字节能报出偏移。
//! 纪律约束：
//!   - 落位前校验：未过 CRC 禁发 SIPI（无绕过开关）；
//!   - 黄金值同源：生成侧与校验侧用同一 CRC32 函数；
//!   - 篡改检出 100%：任意单字节翻转必报；
//!   - 定位尽力：单字节翻转用逐段比对报偏移。
//! 成本记账：CRC O(n)；定位 O(n)（单字节情形）。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x3ef0;

/// 标准 CRC32（IEEE 0xEDB88320，查表法——与生成侧同源）。
pub fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// 跳板镜像：代码 + 黄金 CRC。
#[derive(Debug, Clone)]
pub struct TrampolineImage {
    pub code: Vec<u8>,
    pub golden_crc: u32,
}

impl TrampolineImage {
    /// 黄金跳板：伪指令流（确定性生成）。
    pub fn golden() -> Self {
        // 确定性代码体：64 字节
        let mut code = Vec::with_capacity(64);
        for i in 0..64u8 {
            code.push(i.wrapping_mul(31).wrapping_add(7));
        }
        let crc = crc32(&code);
        TrampolineImage { code, golden_crc: crc }
    }

    /// 落位前校验。
    pub fn verify(&self) -> Result<(), &'static str> {
        if crc32(&self.code) == self.golden_crc {
            Ok(())
        } else {
            Err("跳板 CRC 不符，禁发 SIPI")
        }
    }
}

/// 单字节翻转定位器：逐字节试探还原 → 报偏移。
/// 仅对「单字节差异」有效；多字节差异返回 None（诚实告知不可定位）。
pub fn locate_single_flip(good: &[u8], bad: &[u8]) -> Option<usize> {
    if good.len() != bad.len() {
        return None;
    }
    let diffs: Vec<usize> = good.iter().zip(bad.iter()).enumerate().filter(|(_, (a, b))| a != b).map(|(i, _)| i).collect();
    if diffs.len() == 1 {
        Some(diffs[0])
    } else {
        None
    }
}

/// 黄金落位流程：镜像 → 校验 → 允许 SIPI。
pub fn placement_gate(img: &TrampolineImage) -> Result<(), &'static str> {
    img.verify()
}

/// UNX-F0336 判据（J1 落位前校验 + 篡改检出 100%）。
pub fn run_unx336_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0336 trampoline 完整性校验");

    // CRC32 自检：标准向量 "123456789" → 0xCBF43926
    cs.add("CRC32 标准向量", crc32(b"123456789") == 0xCBF4_3926, "IEEE 向量");

    // J1 主线：黄金镜像过闸
    let img = TrampolineImage::golden();
    cs.add("J1 黄金镜像过闸", placement_gate(&img).is_ok(), "CRC 一致");
    cs.add("J1 黄金值非零", img.golden_crc != 0, "空 CRC 语义");

    // 注入 1：单字节翻转 → 拒闸 + 定位
    let mut bad1 = img.clone();
    bad1.code[17] ^= 0x01;
    cs.add("注入1 单翻拒闸", bad1.verify().is_err(), "禁发 SIPI");
    cs.add("注入1 定位偏移 17", locate_single_flip(&img.code, &bad1.code) == Some(17), "逐段比对");
    cs.add("注入1 每一翻转都检出", {
        // 100% 抽验：64 字节逐个翻
        (0..64).all(|i| {
            let mut b = img.clone();
            b.code[i] ^= 0xA5;
            b.verify().is_err()
        })
    }, "64/64 拒闸");

    // 注入 2：黄金 CRC 被改（镜像好、账坏）→ 同样拒
    let mut bad2 = img.clone();
    bad2.golden_crc ^= 0xFF;
    cs.add("注入2 账坏也拒", bad2.verify().is_err(), "双向校验");

    // 注入 3：多字节篡改 → 不可定位诚实告知
    let mut bad3 = img.clone();
    bad3.code[3] ^= 1;
    bad3.code[40] ^= 1;
    cs.add("注入3 双翻拒闸", bad3.verify().is_err(), "检出");
    cs.add("注入3 多翻不假定位", locate_single_flip(&img.code, &bad3.code).is_none(), "诚实告知");
    cs.add("注入3 长度不等拒定位", locate_single_flip(&img.code, &[]).is_none(), "防御");

    // 注入 4：截断检出
    let mut bad4 = img.clone();
    bad4.code.truncate(60);
    cs.add("注入4 截断检出", bad4.verify().is_err(), "长度变化即坏");

    // 定位器全谱：每个偏移都准
    cs.add("定位器全谱 64/64", {
        (0..64).all(|i| {
            let mut b = img.clone();
            b.code[i] ^= 0x80;
            locate_single_flip(&img.code, &b.code) == Some(i)
        })
    }, "逐偏移验证");

    cs
}
