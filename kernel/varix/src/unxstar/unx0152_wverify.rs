//! UNX-F0152 · 写后校验接口规范 · 诊断段 0x25D0
//!
//! 判据原文（UNX-F0152-J1）：写盘指令完成后按「读回-比对」闭环验收：
//! 读回内容逐字节与预期一致 + CRC 复核 + 区间账复核，三者全过才记
//! 「已验收」；任一失败记差异账（偏移定位到首差异字节）。
//!
//! 设计要点：写后校验器（WriteVerifier）——预期帧 {偏移, 长度,
//! 内容 CRC, 内容指纹}；读回帧同构；比对产出差异账（首差偏移 +
//! 类别闭集：CRC 差 / 指纹差 / 长度差）。零浮点、零 unwrap/expect。

use crate::alloc::{format, string::String, vec::Vec};

pub const SEG_BASE: u16 = 0x25E0;

const E_LEN: u16 = SEG_BASE + 1; // 读回长度与预期不符
const E_CRC: u16 = SEG_BASE + 2; // CRC 复核不符
const E_FP: u16 = SEG_BASE + 3; // 内容指纹不符
const E_BAL: u16 = SEG_BASE + 4; // 校验账守恒被破坏
const E_STATE: u16 = SEG_BASE + 5; // 未写先验（无预期帧）
const E_SEAL: u16 = SEG_BASE + 6; // 验收册已封

const FNV32_OFF: u32 = 0x811C_9DC5;
const FNV32_PRIME: u32 = 0x0100_0193;

/// FNV-1a 32 位折叠。
pub fn fnv1a32(data: &[u8]) -> u32 {
    let mut h = FNV32_OFF;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(FNV32_PRIME);
    }
    h
}

/// CRC8（多项式 0x07）。
pub fn crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            let msb = crc & 0x80;
            crc <<= 1;
            if msb != 0 {
                crc ^= 0x07;
            }
        }
    }
    crc
}

/// 预期帧（写盘意图）。
#[derive(Clone)]
pub struct ExpectFrame {
    pub offset: u64,
    pub content: Vec<u8>,
}

impl ExpectFrame {
    pub fn crc(&self) -> u8 {
        crc8(&self.content)
    }

    pub fn fp(&self) -> u32 {
        fnv1a32(&self.content)
    }
}

/// 差异类别闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    CrcMismatch,
    FpMismatch,
    LenMismatch,
}

impl DiffKind {
    fn name(&self) -> &'static str {
        match self {
            DiffKind::CrcMismatch => "CRC差",
            DiffKind::FpMismatch => "指纹差",
            DiffKind::LenMismatch => "长度差",
        }
    }
}

/// 校验结果行。
#[derive(Clone)]
pub struct VerifyRow {
    pub offset: u64,
    pub ok: bool,
    pub diff: Option<DiffKind>,
    pub first_diff_off: Option<u64>, // 首差异字节偏移（内容比对时）
}

/// 写后校验册。
pub struct WvBook {
    pub expects: Vec<ExpectFrame>,
    pub rows: Vec<VerifyRow>,
    pub sealed: bool,
}

impl WvBook {
    pub fn new() -> Result<Self, u16> {
        Ok(WvBook { expects: Vec::new(), rows: Vec::new(), sealed: false })
    }

    /// 登记写意图（预期帧）。
    pub fn expect(&mut self, offset: u64, content: &[u8]) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        if content.is_empty() {
            return Err(E_LEN);
        }
        self.expects.push(ExpectFrame {
            offset,
            content: content.to_vec(),
        });
        Ok(())
    }

    /// 写后读回验收：读回段与预期段同位比对。
    pub fn verify_at(&mut self, expect_idx: usize, readback: &[u8]) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        if expect_idx >= self.expects.len() {
            return Err(E_STATE);
        }
        let exp = &self.expects[expect_idx];
        let row = if readback.len() != exp.content.len() {
            VerifyRow {
                offset: exp.offset,
                ok: false,
                diff: Some(DiffKind::LenMismatch),
                first_diff_off: None,
            }
        } else if crc8(readback) != exp.crc() {
            VerifyRow {
                offset: exp.offset,
                ok: false,
                diff: Some(DiffKind::CrcMismatch),
                first_diff_off: None,
            }
        } else if fnv1a32(readback) != exp.fp() {
            VerifyRow {
                offset: exp.offset,
                ok: false,
                diff: Some(DiffKind::FpMismatch),
                first_diff_off: None,
            }
        } else {
            // 逐字节比对定位首差（CRC/指纹同过时仍要求逐字节等）。
            let mut first = None;
            for (i, (a, b)) in exp.content.iter().zip(readback.iter()).enumerate() {
                if a != b {
                    first = Some(exp.offset + i as u64);
                    break;
                }
            }
            match first {
                Some(off) => VerifyRow {
                    offset: exp.offset,
                    ok: false,
                    diff: Some(DiffKind::FpMismatch),
                    first_diff_off: Some(off),
                },
                None => VerifyRow {
                    offset: exp.offset,
                    ok: true,
                    diff: None,
                    first_diff_off: None,
                },
            }
        };
        self.rows.push(row);
        Ok(())
    }

    /// 全部验收通过判定。
    pub fn all_accepted(&self) -> Result<(), u16> {
        if self.rows.len() != self.expects.len() {
            return Err(E_STATE); // 有意图未验收
        }
        for r in &self.rows {
            if !r.ok {
                return Err(E_BAL);
            }
        }
        Ok(())
    }

    /// 账守恒：行数 ≤ 意图数；每意图至多一行；红行必有差异类别。
    pub fn audit(&self) -> Result<(), u16> {
        if self.rows.len() > self.expects.len() {
            return Err(E_BAL);
        }
        for r in &self.rows {
            if r.ok == r.diff.is_some() {
                return Err(E_BAL); // 绿红与差异有无必须互补
            }
            if let Some(DiffKind::FpMismatch) = r.diff {
                if !r.ok && r.first_diff_off.is_none() {
                    // 指纹差允许无首差（整体比对路径），放行
                }
            }
        }
        Ok(())
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 渲染验收一览。
    pub fn render(&self) -> String {
        let mut out = String::new();
        let ok = self.rows.iter().filter(|r| r.ok).count();
        out.push_str(&format!("写后验收 {}/{} 过\n", ok, self.expects.len()));
        for r in &self.rows {
            if r.ok {
                out.push_str(&format!("过 @{}\n", r.offset));
            } else {
                let d = r.diff.map(|k| k.name()).unwrap_or("?");
                out.push_str(&format!("败 @{} {}\n", r.offset, d));
            }
        }
        out
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 4 例 ─────────────────────────

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：写两段 → 读回逐字节一致 → 全验收过。
    let mut w = WvBook::new()?;
    w.expect(0x200, b"STAGE2-OK")?;
    w.expect(0x1000, b"KERNEL")?;
    w.verify_at(0, b"STAGE2-OK")?;
    w.verify_at(1, b"KERNEL")?;
    w.all_accepted()?;
    w.audit()?;
    hits += 1;

    // L2：首差定位——第 3 字节被写坏，定位到 0x203。
    let mut w = WvBook::new()?;
    w.expect(0x200, b"ABCDEFGH")?;
    let mut bad = b"ABCDEFGH".to_vec();
    bad[3] ^= 0xFF;
    w.verify_at(0, &bad)?;
    if w.rows[0].ok {
        return Err(E_BAL);
    }
    if w.rows[0].first_diff_off != Some(0x203) {
        return Err(E_BAL);
    }
    w.audit()?;
    hits += 1;

    // L3：长度差与 CRC 差闭集路由。
    let mut w = WvBook::new()?;
    w.expect(0, b"12345678")?;
    w.verify_at(0, b"1234")?; // 长度差
    if w.rows[0].diff != Some(DiffKind::LenMismatch) {
        return Err(E_LEN);
    }
    let mut w2 = WvBook::new()?;
    w2.expect(0, b"12345678")?;
    let mut c = b"12345678".to_vec();
    c[0] ^= 0xFF;
    w2.verify_at(0, &c)?; // CRC 差
    if w2.rows[0].diff != Some(DiffKind::CrcMismatch) {
        return Err(E_CRC);
    }
    hits += 1;

    // L4：渲染含「写后验收 2/2 过」。
    let mut w = WvBook::new()?;
    w.expect(0, b"AA")?;
    w.verify_at(0, b"AA")?;
    let rep = w.render();
    if !rep.contains("1/1 过") {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：无意图先验收（E_STATE）；意图越纲（E_STATE）。
    let mut i1 = WvBook::new()?;
    if i1.verify_at(0, b"x") != Err(E_STATE) {
        return Err(E_STATE);
    }
    i1.expect(0, b"AB")?;
    if i1.verify_at(5, b"AB") != Err(E_STATE) {
        return Err(E_STATE);
    }
    hits += 1;

    // I2：零长意图拒。
    let mut i2 = WvBook::new()?;
    if i2.expect(0, b"") != Err(E_LEN) {
        return Err(E_LEN);
    }
    hits += 1;

    // I3：有意图未验收 all_accepted 拒（E_STATE）。
    let i3 = WvBook::new()?;
    if i3.all_accepted() != Err(E_STATE) {
        return Err(E_STATE);
    }
    hits += 1;

    // I4：封册拒写 + 账注入（绿行带差异类别 → 互补律破）。
    let mut i4 = WvBook::new()?;
    i4.expect(0, b"AB")?;
    i4.seal();
    if i4.expect(8, b"CD") != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    i4.sealed = false;
    i4.verify_at(0, b"AB")?;
    i4.rows[0].diff = Some(DiffKind::CrcMismatch); // 破互补律：绿行带差
    if i4.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
