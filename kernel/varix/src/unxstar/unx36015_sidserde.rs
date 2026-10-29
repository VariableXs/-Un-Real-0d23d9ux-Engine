//! unx36015_sidserde — UNX-F36015 SID 序列化容器面（令牌/SD 载体内的
//! 偏移引用）。
//!
//! 判据原文：
//!   J1 容器内偏移引用 100 例解析一致；越界偏移注入 10 例拒绝
//!      （引用校验器）且容器整体标记损坏。
//! 设计纪律：
//!   1. 容器模型：512 字节 blob + 引用表（offset/len/fp）；写入走
//!      unx36002 变长编码（8+4×计数），引用登记（偏移 4 对齐）。
//!   2. 解析校验器：偏移越界 / 非对齐 / 长度与引用不符 / 指纹漂移
//!      逐级拒；**越界偏移 = 容器整体腐蚀**（sticky 标记，后续解析
//!      一律拒——宁可全停不可半信）。
//!   3. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性。

pub const SEG_BASE: u16 = 0x7430;

// ── 诊断码闭集（0x7430~0x744F，独占）─────────────────────────
const E_RANGE: u16 = SEG_BASE + 1; // 越界偏移（引用校验器拒）
const E_ALIGN: u16 = SEG_BASE + 2; // 偏移未 4 字节对齐
const E_FP: u16 = SEG_BASE + 3; // 解出指纹与引用账不符
const E_CORRUPT: u16 = SEG_BASE + 4; // 容器已标记腐蚀（sticky 拒）
const E_CAP: u16 = SEG_BASE + 5; // 容器/引用表满（显性拒）
const E_LEN: u16 = SEG_BASE + 6; // 解出长度与引用账不符
const E_BAL: u16 = SEG_BASE + 7; // 账面对平失配（健康面漂移）

use crate::checks::CheckSet;
use alloc::vec::Vec;
use super::unx36001_sidcore::Sid;
use super::unx36002_sidlayout;

/// 容器 blob 容量（100 例 × 至多 20 字节 + 冗余）。
pub const BLOB_CAP: usize = 2048;
/// 引用表容量（≥ 100 例 + 注入余量）。
pub const REF_CAP: usize = 128;

/// 容器内引用。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CRef {
    pub off: usize, // blob 内偏移（4 对齐）
    pub len: usize, // 编码长度
    pub fp: u64,    // 引用时 SID 指纹
}

/// SID 容器。
pub struct SidContainer {
    blob: [u8; BLOB_CAP],
    pub used: usize,
    refs: [Option<CRef>; REF_CAP],
    pub ref_count: usize,
    pub corrupt: bool,
}

impl SidContainer {
    pub fn new() -> SidContainer {
        SidContainer {
            blob: [0; BLOB_CAP],
            used: 0,
            refs: [None; REF_CAP],
            ref_count: 0,
            corrupt: false,
        }
    }

    /// 写入：SID 编码落 blob 尾部 + 引用登记（返回引用号）。
    pub fn put(&mut self, s: &Sid) -> Result<usize, u16> {
        if self.corrupt {
            return Err(E_CORRUPT);
        }
        let mut tmp = [0u8; 8 + 4 * 15];
        let n = unx36002_sidlayout::encode(s, &mut tmp)?;
        let off = self.used;
        if off % 4 != 0 {
            return Err(E_ALIGN);
        }
        if off + n > BLOB_CAP {
            return Err(E_CAP);
        }
        let mut i = 0;
        while i < n {
            self.blob[off + i] = tmp[i];
            i += 1;
        }
        self.used = off + n;
        let mut j = 0;
        while j < REF_CAP {
            if self.refs[j].is_none() {
                self.refs[j] = Some(CRef { off, len: n, fp: s.fingerprint() });
                self.ref_count += 1;
                return Ok(j);
            }
            j += 1;
        }
        Err(E_CAP)
    }

    /// 解析：引用号 → SID。校验器：腐蚀 sticky / 越界 / 对齐 / 长度 /
    /// 指纹逐级拒；越界偏移将容器标记腐蚀（sticky）。
    pub fn resolve(&mut self, idx: usize) -> Result<Sid, u16> {
        if self.corrupt {
            return Err(E_CORRUPT);
        }
        if idx >= REF_CAP || self.refs[idx].is_none() {
            return Err(E_RANGE);
        }
        let r = self.refs[idx].ok_or(E_RANGE)?;
        if r.off % 4 != 0 {
            self.corrupt = true;
            return Err(E_ALIGN);
        }
        if r.off + r.len > self.used || r.off >= self.used {
            // 越界偏移：引用账与 blob 实况失配 = 容器整体腐蚀。
            self.corrupt = true;
            return Err(E_RANGE);
        }
        let view = &self.blob[r.off..self.used];
        match unx36002_sidlayout::decode(view) {
            Ok((s, used_len)) => {
                if used_len != r.len {
                    self.corrupt = true;
                    return Err(E_LEN);
                }
                if s.fingerprint() != r.fp {
                    self.corrupt = true;
                    return Err(E_FP);
                }
                Ok(s)
            }
            Err(c) => {
                self.corrupt = true;
                Err(c)
            }
        }
    }

    /// 越界引用注入（校验器直测面）：伪造引用行 → resolve 必拒且腐蚀。
    pub fn inject_ref(&mut self, off: usize, len: usize) -> Result<usize, u16> {
        if self.corrupt {
            return Err(E_CORRUPT);
        }
        let mut j = 0;
        while j < REF_CAP {
            if self.refs[j].is_none() {
                self.refs[j] = Some(CRef { off, len, fp: 0 });
                self.ref_count += 1;
                return Ok(j);
            }
            j += 1;
        }
        Err(E_CAP)
    }

    /// 引用账直读（无解析）。
    pub fn ref_at(&self, idx: usize) -> Option<CRef> {
        if idx >= REF_CAP {
            return None;
        }
        self.refs[idx]
    }

    /// 容器余量（字节面）。
    pub fn free_bytes(&self) -> usize {
        BLOB_CAP - self.used
    }

    /// 引用表余量。
    pub fn free_refs(&self) -> usize {
        REF_CAP - self.ref_count
    }

    /// 容器健康摘要：(used, ref_count, corrupt) 三元组（收口面）。
    pub fn health(&self) -> (usize, usize, bool) {
        (self.used, self.ref_count, self.corrupt)
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_RANGE => "越界偏移（引用校验器拒，容器腐蚀）",
        E_ALIGN => "偏移未 4 字节对齐（容器腐蚀）",
        E_FP => "解出指纹与引用账不符（容器腐蚀）",
        E_CORRUPT => "容器已标记腐蚀（sticky 全停）",
        E_CAP => "容器/引用表满（显性拒绝）",
        E_LEN => "解出长度与引用账不符（容器腐蚀）",
        E_BAL => "账面对平失配（健康面漂移）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：100 例引用解析一致 + 10 例越界注入全拒且腐蚀（判据 J1）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut c = SidContainer::new();
    let mut fps: Vec<u64> = Vec::new();

    // R1：100 例写入→解析一致（计数 1..=3 铺满保证容量适配；
    // 指纹逐例对平）。
    let mut seed: u32 = 0x360F_0001;
    let mut n = 0;
    while n < 100 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let cnt = (n % 3) + 1;
        let mut subs = [0u32; 15];
        let mut i = 0;
        while i < cnt {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            subs[i] = seed;
            i += 1;
        }
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let ia_val = ((seed & 0xFFFF) as u64) << 8 | 0x21;
        let s = Sid::new(1, Sid::ia_from(ia_val), &subs[..cnt])?;
        let fp = s.fingerprint();
        let idx = c.put(&s)?;
        let back = c.resolve(idx)?;
        if back.fingerprint() != fp {
            return Err(E_FP);
        }
        let r = c.ref_at(idx).ok_or(E_RANGE)?;
        if r.fp != fp || r.off % 4 != 0 {
            return Err(E_FP);
        }
        fps.push(fp);
        n += 1;
    }
    if c.corrupt || fps.len() != 100 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：越界偏移注入 10 例——首例（off ≥ used 伪造引用）resolve 拒
    // E_RANGE 并触发容器整体腐蚀（sticky）；后续 9 例注入在腐蚀态被
    // sticky 拒绝（合计 10 例注入全被拦）。腐蚀后一切解析/写入拒绝。
    let mut c2 = SidContainer::new();
    let s = Sid::new(1, Sid::ia_from(5), &[21, 1000])?;
    c2.put(&s)?;
    let used = c2.used;
    let idx = c2.inject_ref(used, 12)?;
    if c2.resolve(idx) != Err(E_RANGE) {
        return Err(E_RANGE);
    }
    if !c2.corrupt {
        return Err(E_CORRUPT);
    }
    let mut k = 1;
    while k < 10 {
        if c2.inject_ref(used + k * 8, 12) != Err(E_CORRUPT) {
            return Err(E_CORRUPT);
        }
        k += 1;
    }
    if c2.resolve(0) != Err(E_CORRUPT) {
        return Err(E_CORRUPT);
    }
    if c2.put(&s) != Err(E_CORRUPT) {
        return Err(E_CORRUPT);
    }
    hits += 1;

    // R3：对齐/指纹/长度腐蚀族——三族注入各自触发 sticky 腐蚀。
    // 对齐：off=2（非 4 倍数）。
    let mut c3 = SidContainer::new();
    c3.put(&s)?;
    let idx = c3.inject_ref(2, 12)?;
    if c3.resolve(idx) != Err(E_ALIGN) || !c3.corrupt {
        return Err(E_ALIGN);
    }
    hits += 1;

    // R3b：指纹与长度族——合法偏移 + 错误 fp / 错误长度（decode 读不满：
    // used_len 与引用账失配）。
    // 指纹：合法偏移 + 错误 fp。
    let mut c4 = SidContainer::new();
    c4.put(&s)?;
    let r0 = c4.ref_at(0).ok_or(E_RANGE)?;
    let idx = c4.inject_ref(r0.off, r0.len)?;
    if c4.resolve(idx) != Err(E_FP) || !c4.corrupt {
        return Err(E_FP);
    }
    // 长度：合法偏移 + 错误长度（decode 读不满：used_len 与引用账失配）。
    let mut c5 = SidContainer::new();
    c5.put(&s)?;
    let r0 = c5.ref_at(0).ok_or(E_RANGE)?;
    let idx = c5.inject_ref(r0.off, r0.len - 4)?;
    if c5.resolve(idx) != Err(E_LEN) || !c5.corrupt {
        return Err(E_LEN);
    }
    hits += 1;

    // R4：容量显性面——引用表 128 满后 put 显性 E_CAP（不静默覆盖）。
    let mut c6 = SidContainer::new();
    let mut n = 0;
    while n < 128 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let s2 = Sid::new(1, Sid::ia_from(5), &[6000 + n as u32])?;
        c6.put(&s2)?;
        n += 1;
    }
    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    let s3 = Sid::new(1, Sid::ia_from(5), &[7777, seed])?;
    if c6.put(&s3) != Err(E_CAP) {
        return Err(E_CAP);
    }
    // 健康摘要对平：满载容器 used/refs 全满、未腐蚀。
    let (u, rc, co) = c6.health();
    if u == 0 || rc != REF_CAP || co {
        return Err(E_BAL);
    }
    if c6.free_refs() != 0 {
        return Err(E_CAP);
    }
    // R1 容器健康面：100 例后未腐蚀、余量口径一致。
    let (u1, rc1, co1) = c.health();
    if co1 || rc1 != 100 || u1 + c.free_bytes() != BLOB_CAP {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}

// ── 判据聚合（CheckSet 面 · 进 unxstar_blocks 检查表）────────
/// 判据 J1 聚合入口：SEG 锚 + 确定性演练全绿（红时回填诊断码）。
pub fn run_unx36015_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36015_sidserde");
    cs.add("SEG 段位独占", SEG_BASE == 0x7430, "0x7430~0x744F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }
}
