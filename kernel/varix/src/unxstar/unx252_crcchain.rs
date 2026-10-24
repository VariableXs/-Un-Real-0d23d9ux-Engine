//! UNX-F0252 早传完整性：十二步齐备 CRC 链（0x3CD0 段）
//!
//! 锚点：UNX-F0252-J1 逐节 CRC + 总 CRC 双层校验，单节损坏
//! 定位到节，齐备率断言 100%。
//! 域：CoRun·UNX-A1-B13（完整性链面）。
//!
//! 结构：节载荷（十二步节字节载荷）+ CRC32 逐节校验表 + 总 CRC
//! （全节折叠）双层模型 + 完整性审计（逐节比对 → 齐备率）+ 单节
//! 损坏定位核（返回节号）+ 注入核（单节翻转 / 总表漂移 / 空节 /
//! 封账拒写必抓）。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;

/// 诊断段（0x3CD0 独占 0x10）。
pub const SEG_BASE: u32 = 0x3CD0;
pub const IC_NODEF: u32 = SEG_BASE + 0x01; // 节未定义（无载荷）
pub const IC_CRC_MISS: u32 = SEG_BASE + 0x02; // CRC 表缺节
pub const IC_CRC_BAD: u32 = SEG_BASE + 0x03; // 节 CRC 不符
pub const IC_TOTAL_BAD: u32 = SEG_BASE + 0x04; // 总 CRC 不符
pub const IC_SEALED: u32 = SEG_BASE + 0x05; // 链已封账拒写

/// 十二步节数（F0241 总纲同源）。
pub const STEP_COUNT: usize = 12;

/// CRC32（IEEE 多项式，逐字节位法，no_std 无查表依赖）。
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= b as u32;
        let mut i = 0;
        while i < 8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            i += 1;
        }
    }
    !crc
}

/// 十二步节载荷（每节一条字节载荷；空 = 未定义）。
pub struct StepPayload {
    pub loads: [Option<Vec<u8>>; STEP_COUNT],
    pub sealed: bool,
}

impl StepPayload {
    pub fn new() -> Self {
        StepPayload { loads: Default::default(), sealed: false }
    }

    /// 置节载荷（封账后拒；节号 0..12）。
    pub fn put(&mut self, step: usize, data: &[u8]) -> Result<(), u32> {
        if self.sealed {
            return Err(IC_SEALED);
        }
        if step >= STEP_COUNT || data.is_empty() {
            return Err(IC_NODEF);
        }
        self.loads[step] = Some(Vec::from(data));
        Ok(())
    }

    /// 十二步齐备核。
    pub fn complete(&self) -> bool {
        self.loads.iter().all(|l| l.is_some())
    }

    /// 逐节 CRC 表（未定义节缺席）。
    pub fn crc_table(&self) -> Vec<(usize, u32)> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < STEP_COUNT {
            if let Some(d) = &self.loads[i] {
                out.push((i, crc32(d)));
            }
            i += 1;
        }
        out
    }

    /// 总 CRC：十二节载荷按节序折叠（缺节视为空字节串仍参与序位）。
    pub fn total_crc(&self) -> u32 {
        let mut acc: u64 = 0xcbf29ce484222325;
        let mut i = 0usize;
        while i < STEP_COUNT {
            match &self.loads[i] {
                Some(d) => acc = acc.wrapping_mul(0x100000001b3) ^ (crc32(d) as u64),
                None => acc = acc.wrapping_mul(0x100000001b3) ^ 0,
            }
            i += 1;
        }
        (acc ^ (acc >> 32)) as u32
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 双层审计核：逐节 CRC + 总 CRC 与参考表比对。
/// 返回 Ok(齐备率万分比) 或 Err(首个坏节号 + 码)。
pub fn audit(
    p: &StepPayload,
    ref_table: &[(usize, u32)],
    ref_total: u32,
) -> Result<u32, (usize, u32)> {
    if !p.complete() {
        let mut i = 0usize;
        while i < STEP_COUNT {
            if p.loads[i].is_none() {
                return Err((i, IC_NODEF));
            }
            i += 1;
        }
    }
    // 逐节比对（CRC 表按节号索引，缺表项即 IC_CRC_MISS）
    let table = p.crc_table();
    let mut i = 0usize;
    while i < STEP_COUNT {
        let mine = table.iter().find(|&&(s, _)| s == i).map(|&(_, c)| c);
        let theirs = ref_table.iter().find(|&&(s, _)| s == i).map(|&(_, c)| c);
        match (mine, theirs) {
            (Some(a), Some(b)) => {
                if a != b {
                    return Err((i, IC_CRC_BAD));
                }
            }
            _ => return Err((i, IC_CRC_MISS)),
        }
        i += 1;
    }
    if p.total_crc() != ref_total {
        return Err((STEP_COUNT, IC_TOTAL_BAD));
    }
    Ok(10_000) // 齐备率 100.00%
}

/// 单节损坏定位核：翻转单节载荷后定位（返回 (节号, 损坏类型)）。
pub fn locate_damage(p: &StepPayload, ref_table: &[(usize, u32)], ref_total: u32) -> (usize, u32) {
    match audit(p, ref_table, ref_total) {
        Err(loc) => loc,
        Ok(_) => (STEP_COUNT, 0),
    }
}

/// 标准夹具：十二节各置辨识载荷（节号派生字节，长度随节号变化）。
pub fn standard_payload() -> StepPayload {
    let mut p = StepPayload::new();
    let mut i = 0usize;
    while i < STEP_COUNT {
        let len = 8 + i * 4;
        let mut data = Vec::new();
        let mut j = 0usize;
        while j < len {
            data.push(((i * 31 + j * 7) & 0xff) as u8);
            j += 1;
        }
        let _ = p.put(i, &data);
        i += 1;
    }
    p
}

/// 夹具参考表（与夹具同源生成，模拟登记的期望值）。
pub fn fixture_refs() -> (Vec<(usize, u32)>, u32) {
    let p = standard_payload();
    (p.crc_table(), p.total_crc())
}

/// 注入核：7 注入必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空载荷节拒
    let mut p = StepPayload::new();
    if p.put(0, &[]) == Err(IC_NODEF) {
        caught += 1;
    }
    // 2) 节号越界拒
    if p.put(12, &[1]) == Err(IC_NODEF) {
        caught += 1;
    }
    // 3) 单节翻转定位到节（节 5）
    let (refs, total) = fixture_refs();
    let mut q = standard_payload();
    if let Some(d) = &mut q.loads[5] {
        d[0] ^= 0xff;
    }
    let loc = locate_damage(&q, &refs, total);
    if loc.0 == 5 && loc.1 == IC_CRC_BAD {
        caught += 1;
    }
    // 4) 缺节定位（拔掉节 9 载荷）
    let mut r = standard_payload();
    r.loads[9] = None;
    let loc = locate_damage(&r, &refs, total);
    if loc.0 == 9 && loc.1 == IC_NODEF {
        caught += 1;
    }
    // 5) 总 CRC 漂移必抓（逐节全对但参考总 CRC 错）
    let mut s = standard_payload();
    if audit(&s, &refs, total ^ 1) == Err((STEP_COUNT, IC_TOTAL_BAD)) {
        caught += 1;
    }
    // 6) 封账拒写
    s.seal();
    if s.put(0, &[1, 2, 3]) == Err(IC_SEALED) {
        caught += 1;
    }
    // 7) 干净载荷审计绿（双向实证）
    let t = standard_payload();
    if audit(&t, &refs, total) == Ok(10_000) {
        caught += 1;
    }
    (caught, 7)
}

/// 判据入口（CheckSet）。
pub fn run_unx252_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0252");
    // J1-1 双层校验建立（逐节表 12 项 + 总 CRC 可复算稳定）
    let (refs, total) = fixture_refs();
    let p = standard_payload();
    let ok = refs.len() == STEP_COUNT && p.total_crc() == total && p.complete();
    s.add("B13-29-双层校验建立", ok, if ok { "" } else { "双层缺层" });
    // J1-2 齐备率 100%
    let ok = audit(&p, &refs, total) == Ok(10_000);
    s.add("B13-30-齐备率100%", ok, if ok { "" } else { "齐备率不足" });
    // J1-3 单节损坏定位到节
    let mut q = standard_payload();
    if let Some(d) = &mut q.loads[7] {
        d[3] ^= 0x5a;
    }
    let loc = locate_damage(&q, &refs, total);
    let ok = loc.0 == 7 && loc.1 == IC_CRC_BAD;
    s.add("B13-31-单节损坏定位", ok, if ok { "" } else { "定位失准" });
    // J1-4 CRC32 已知向量锚（"123456789" = 0xCBF43926）
    let ok = crc32(b"123456789") == 0xCBF4_3926;
    s.add("B13-32-CRC32标准向量", ok, if ok { "" } else { "CRC 实现漂移" });
    // J1-5 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B13-33-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx252_all_checks_green() {
        let set = run_unx252_checks();
        assert!(set.all_passed(), "UNX-F0252 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx252_crc32_vectors() {
        assert_eq!(crc32(b""), 0x0000_0000);
        assert_eq!(crc32(b"a"), 0xE8B7_BE43);
    }
}
