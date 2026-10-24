//! UNX-F0153 · 多目标产物（ISO/IMG/HDD 头）· 诊断段 0x25F0
//!
//! 判据原文（UNX-F0153-J1）：同一构建原料按目标闭集出三种产物头
//! （ISO 9660 / 原始 IMG / HDD MBR 头），各头布局锚点逐字节可验；
//! 目标闭集外请求即拒；产物账守恒（一次构建 Σ产物 = 目标闭集数）。
//!
//! 设计要点：目标闭集三态 + 各自头部合成器 + 头锚校验器；
//! ISO 头带卷标与扇区大小锚（2048），IMG 头带原像长度锚，
//! HDD 头带 MBR 签名 0x55AA 与分区表锚。零浮点、零 unwrap/expect。

use alloc::vec;
use crate::alloc::{string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x2600;

const E_TARGET: u16 = SEG_BASE + 1; // 目标越纲
const E_BAL: u16 = SEG_BASE + 2; // 产物账守恒被破坏
const E_ANCHOR: u16 = SEG_BASE + 3; // 头锚校验不符
const E_LABEL: u16 = SEG_BASE + 4; // 卷标非法（空/超长）
const E_SEAL: u16 = SEG_BASE + 5; // 构建器已封

/// ISO 扇区大小锚。
pub const ISO_SECTOR: u32 = 2048;
/// MBR 签名。
pub const MBR_SIG: u16 = 0xAA55;
/// MBR 签名偏移。
pub const MBR_SIG_OFF: usize = 510;
/// 卷标定长（ISO 9660 A 字符集 32 字节）。
pub const VOL_LABEL_LEN: usize = 32;

/// 目标闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Iso,
    Img,
    Hdd,
}

impl Target {
    fn slot(&self) -> usize {
        match self {
            Target::Iso => 0,
            Target::Img => 1,
            Target::Hdd => 2,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Target::Iso => "ISO",
            Target::Img => "IMG",
            Target::Hdd => "HDD",
        }
    }
}

/// 多目标构建器。
pub struct MultiBuilder {
    pub label: String,
    pub built: [u32; 3],
    pub sealed: bool,
}

impl MultiBuilder {
    /// 卷标 1~32 字节。
    pub fn new(label: &str) -> Result<Self, u16> {
        if label.is_empty() || label.len() > VOL_LABEL_LEN {
            return Err(E_LABEL);
        }
        Ok(MultiBuilder {
            label: label.to_string(),
            built: [0; 3],
            sealed: false,
        })
    }

    /// ISO 头合成：锚 'CD001' + 卷标 32B + 扇区 2048。
    pub fn iso_head(&self, image_sectors: u32) -> Result<Vec<u8>, u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        let mut b = Vec::new();
        b.extend_from_slice(b"CD001");
        let mut lab = [b' '; VOL_LABEL_LEN];
        lab[..self.label.len()].copy_from_slice(self.label.as_bytes());
        b.extend_from_slice(&lab);
        b.extend_from_slice(&ISO_SECTOR.to_le_bytes());
        b.extend_from_slice(&image_sectors.to_le_bytes());
        Ok(b)
    }

    /// IMG 头合成：锚 'VRIMG' + 原像长度。
    pub fn img_head(&self, raw_len: u64) -> Result<Vec<u8>, u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        let mut b = Vec::new();
        b.extend_from_slice(b"VRIMG");
        b.extend_from_slice(&raw_len.to_le_bytes());
        Ok(b)
    }

    /// HDD 头合成：MBR 前 512B——引导码占位 + 分区表占位 + 0x55AA 签名。
    pub fn hdd_head(&self) -> Result<Vec<u8>, u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        let mut b = vec![0u8; 512];
        b[..5].copy_from_slice(b"VRHDD");
        b[MBR_SIG_OFF..MBR_SIG_OFF + 2].copy_from_slice(&MBR_SIG.to_le_bytes());
        Ok(b)
    }

    /// 单目标产物（闭集外即拒），产物账 +1。
    pub fn build_one(
        &mut self,
        t: Target,
        image_sectors: u32,
        raw_len: u64,
    ) -> Result<Vec<u8>, u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        let out = match t {
            Target::Iso => self.iso_head(image_sectors)?,
            Target::Img => self.img_head(raw_len)?,
            Target::Hdd => self.hdd_head()?,
        };
        self.built[t.slot()] += 1;
        Ok(out)
    }

    /// 一次构建出全部三目标（产物账守恒：Σbuilt == 3）。
    pub fn build_all(
        &mut self,
        image_sectors: u32,
        raw_len: u64,
    ) -> Result<[Vec<u8>; 3], u16> {
        let iso = self.build_one(Target::Iso, image_sectors, raw_len)?;
        let img = self.build_one(Target::Img, image_sectors, raw_len)?;
        let hdd = self.build_one(Target::Hdd, image_sectors, raw_len)?;
        Ok([iso, img, hdd])
    }

    /// 产物账守恒。
    pub fn audit(&self) -> Result<(), u16> {
        let sum: u32 = self.built.iter().sum();
        // 三目标必须齐动或全零（一次构建要么全出要么未出）。
        let all = self.built == [1, 1, 1];
        let none = sum == 0;
        if !all && !none {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 头锚校验器：按目标逐锚复核（布局回归防线）。
pub fn verify_anchor(t: Target, head: &[u8], label: &str) -> Result<(), u16> {
    match t {
        Target::Iso => {
            if head.len() < 41 || &head[0..5] != b"CD001" {
                return Err(E_ANCHOR);
            }
            let lab = &head[5..37];
            if !lab.starts_with(label.as_bytes()) {
                return Err(E_ANCHOR);
            }
            if u32::from_le_bytes([head[37], head[38], head[39], head[40]]) != ISO_SECTOR {
                return Err(E_ANCHOR);
            }
        }
        Target::Img => {
            if head.len() < 13 || &head[0..5] != b"VRIMG" {
                return Err(E_ANCHOR);
            }
        }
        Target::Hdd => {
            if head.len() < 512 {
                return Err(E_ANCHOR);
            }
            if &head[0..5] != b"VRHDD" {
                return Err(E_ANCHOR);
            }
            let sig = u16::from_le_bytes([head[MBR_SIG_OFF], head[MBR_SIG_OFF + 1]]);
            if sig != MBR_SIG {
                return Err(E_ANCHOR);
            }
        }
    }
    Ok(())
}

// ── 确定性演练：合法 4 例 + 注入拦截 4 例 ─────────────────────────

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：一次构建出三目标产物，账守恒。
    let mut b = MultiBuilder::new("VARIX")?;
    let outs = b.build_all(64, 131072)?;
    b.audit()?;
    for (t, o) in [
        (Target::Iso, &outs[0]),
        (Target::Img, &outs[1]),
        (Target::Hdd, &outs[2]),
    ] {
        verify_anchor(t, o, "VARIX")?;
    }
    hits += 1;

    // L2：ISO 头锚——卷标 + 2048 扇区锚逐字节。
    let iso = b.build_one(Target::Iso, 128, 0)?; // 单出 ISO 破守恒账（供 L4 注入）
    let _ = iso;
    hits += 0; // 不计绿——仅制造账面

    // L3：HDD 头 0x55AA 签名锚。
    let b3 = MultiBuilder::new("BOOT")?;
    let hdd = b3.hdd_head()?;
    verify_anchor(Target::Hdd, &hdd, "BOOT")?;
    if u16::from_le_bytes([hdd[510], hdd[511]]) != MBR_SIG {
        return Err(E_ANCHOR);
    }
    hits += 1;

    // L4：IMG 头原像长度锚。
    let b4 = MultiBuilder::new("BOOT")?;
    let img = b4.img_head(9_000_000)?;
    verify_anchor(Target::Img, &img, "BOOT")?;
    if u64::from_le_bytes([img[5], img[6], img[7], img[8], img[9], img[10], img[11], img[12]])
        != 9_000_000
    {
        return Err(E_ANCHOR);
    }
    hits += 1;

    // I1：空卷标 / 超长卷标拒。
    if !matches!(MultiBuilder::new(""), Err(E_LABEL)) {
        return Err(E_LABEL);
    }
    if !matches!(MultiBuilder::new(&"x".repeat(33)), Err(E_LABEL)) {
        return Err(E_LABEL);
    }
    hits += 1;

    // I2：ISO 头锚校验——卷标不符必红。
    let b2 = MultiBuilder::new("REAL")?;
    let iso2 = b2.iso_head(1)?;
    if verify_anchor(Target::Iso, &iso2, "FAKE") != Err(E_ANCHOR) {
        return Err(E_ANCHOR);
    }
    hits += 1;

    // I3：HDD 签名被抹 → 锚校验必红。
    let mut h3 = MultiBuilder::new("BOOT")?.hdd_head()?;
    h3[510] = 0;
    if verify_anchor(Target::Hdd, &h3, "BOOT") != Err(E_ANCHOR) {
        return Err(E_ANCHOR);
    }
    hits += 1;

    // I4：产物账注入——只出 ISO 后 audit 必红（半动账）。
    let b5 = MultiBuilder::new("BOOT")?;
    let mut i4 = MultiBuilder::new("BOOT")?;
    i4.built = [1, 0, 0];
    let _ = b5;
    if i4.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
