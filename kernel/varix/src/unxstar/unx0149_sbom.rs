//! UNX-F0149 · 构建物料清单（SBOM 雏形）· 诊断段 0x2580
//!
//! 判据原文（UNX-F0149-J1）：构建产物列出全部物料（名+指纹+来源），
//! 清单账守恒（Σ物料 = Σ登记），清单可序列化读回逐行一致；
//! 未登记物料混入即红（闭环校验）。
//!
//! 设计要点：SBOM 册（登记制）——物料进镜像前必须登记（名唯一 +
//! 指纹 FNV-1a32 + 来源闭集：源码/三方/生成）；清单导出为规范化
//! 文本（字典序）；验收时逐行对账，多/少/改任一即红。
//! 零浮点、零 unwrap/expect。

use alloc::vec;
use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x2580;

const E_DUP: u16 = SEG_BASE + 1; // 物料重名
const E_NAME: u16 = SEG_BASE + 2; // 物料名非法（空）
const E_SRC: u16 = SEG_BASE + 3; // 来源越纲
const E_BAL: u16 = SEG_BASE + 4; // 清单账守恒被破坏
const E_MISS: u16 = SEG_BASE + 5; // 验收发现未登记物料
const E_FP: u16 = SEG_BASE + 6; // 指纹不符（内容被换）
const E_SEAL: u16 = SEG_BASE + 7; // 册已封拒写

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

/// 物料来源闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// 源码内产物。
    Src,
    /// 三方引入。
    ThirdParty,
    /// 构建期生成。
    Generated,
}

impl Origin {
    fn code(&self) -> u8 {
        match self {
            Origin::Src => 1,
            Origin::ThirdParty => 2,
            Origin::Generated => 3,
        }
    }

    fn from_code(c: u8) -> Result<Self, u16> {
        match c {
            1 => Ok(Origin::Src),
            2 => Ok(Origin::ThirdParty),
            3 => Ok(Origin::Generated),
            _ => Err(E_SRC),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Origin::Src => "源码",
            Origin::ThirdParty => "三方",
            Origin::Generated => "生成",
        }
    }
}

/// SBOM 物料行。
#[derive(Clone)]
pub struct BomLine {
    pub name: String,
    pub fp: u32,
    pub origin: Origin,
}

/// SBOM 册。
pub struct SbomBook {
    pub lines: Vec<BomLine>,
    pub sealed: bool,
}

impl SbomBook {
    pub fn new() -> Result<Self, u16> {
        Ok(SbomBook { lines: Vec::new(), sealed: false })
    }

    /// 登记物料（重名/空名/封册即拒）。
    pub fn register(&mut self, name: &str, content: &[u8], origin: Origin) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        if name.is_empty() {
            return Err(E_NAME);
        }
        if self.lines.iter().any(|l| l.name == name) {
            return Err(E_DUP);
        }
        self.lines.push(BomLine {
            name: name.to_string(),
            fp: fnv1a32(content),
            origin,
        });
        Ok(())
    }

    /// 封册（清单定稿，只读化）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 清单账守恒：Σ行 == Σ登记且名字典序可重排（行序无关，键唯一）。
    pub fn audit(&self) -> Result<(), u16> {
        for i in 0..self.lines.len() {
            for j in (i + 1)..self.lines.len() {
                if self.lines[i].name == self.lines[j].name {
                    return Err(E_DUP);
                }
            }
        }
        Ok(())
    }

    /// 规范化导出（名字典序行）："名 指纹hex 来源"。
    pub fn export_sorted(&self) -> Result<Vec<String>, u16> {
        if !self.sealed {
            return Err(E_SEAL); // 未封册不导出
        }
        let mut idx: Vec<usize> = (0..self.lines.len()).collect();
        idx.sort_by(|&a, &b| self.lines[a].name.cmp(&self.lines[b].name));
        let mut out = Vec::new();
        for i in idx {
            let l = &self.lines[i];
            out.push(format!("{} {:08X} {}", l.name, l.fp, l.origin.name()));
        }
        Ok(out)
    }

    /// 序列化（登记序二进制）：行数 + (名长+名+fp+origin)。
    pub fn serialize(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&(self.lines.len() as u32).to_le_bytes());
        for l in &self.lines {
            b.extend_from_slice(&(l.name.len() as u16).to_le_bytes());
            b.extend_from_slice(l.name.as_bytes());
            b.extend_from_slice(&l.fp.to_le_bytes());
            b.push(l.origin.code());
        }
        b
    }

    /// 反序列化逐行还原。
    pub fn deserialize(data: &[u8]) -> Result<Self, u16> {
        if data.len() < 4 {
            return Err(E_BAL);
        }
        let n = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let mut off = 4usize;
        let mut book = SbomBook::new()?;
        for _ in 0..n {
            if off + 2 > data.len() {
                return Err(E_BAL);
            }
            let nl = u16::from_le_bytes([data[off], data[off + 1]]) as usize;
            off += 2;
            if off + nl + 5 > data.len() {
                return Err(E_BAL);
            }
            let name = core::str::from_utf8(&data[off..off + nl]).map_err(|_| E_NAME)?;
            off += nl;
            let fp = u32::from_le_bytes([
                data[off],
                data[off + 1],
                data[off + 2],
                data[off + 3],
            ]);
            off += 4;
            let origin = Origin::from_code(data[off])?;
            off += 1;
            book.register(name, &[], origin)?; // 注册时重算指纹会被覆盖
            let li = book.lines.len() - 1;
            book.lines[li].fp = fp;
        }
        Ok(book)
    }
}

/// 验收：实际进镜像的物料表 vs SBOM 册——少登记/多物料/指纹不符任一即红。
pub fn accept(book: &SbomBook, actual: &[BomLine]) -> Result<(), u16> {
    book.audit()?;
    for a in actual {
        match book.lines.iter().find(|l| l.name == a.name) {
            None => return Err(E_MISS), // 未登记混入
            Some(l) => {
                if l.fp != a.fp {
                    return Err(E_FP); // 登记了但内容被换
                }
                if l.origin != a.origin {
                    return Err(E_SRC); // 来源被改
                }
            }
        }
    }
    if actual.len() != book.lines.len() {
        return Err(E_BAL); // 有登记物料缺席镜像
    }
    Ok(())
}

// ── 确定性演练：合法 4 例 + 注入拦截 4 例 ─────────────────────────

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：登记三物料 → 封册 → 导出字典序。
    let mut b = SbomBook::new()?;
    b.register("kernel.elf", b"KELF", Origin::Src)?;
    b.register("initrd.img", b"IRD", Origin::Generated)?;
    b.register("lic.txt", b"MIT", Origin::ThirdParty)?;
    b.seal();
    b.audit()?;
    let exp = b.export_sorted()?;
    if exp.len() != 3 || !exp[0].starts_with("initrd.img") || !exp[2].starts_with("lic.txt") {
        return Err(E_BAL);
    }
    hits += 1;

    // L2：序列化往返逐行一致（含指纹与来源）。
    let snap = b.serialize();
    let back = SbomBook::deserialize(&snap)?;
    if back.lines.len() != 3 {
        return Err(E_BAL);
    }
    for (a, c) in b.lines.iter().zip(back.lines.iter()) {
        if a.name != c.name || a.fp != c.fp || a.origin != c.origin {
            return Err(E_BAL);
        }
    }
    hits += 1;

    // L3：验收闭环——镜像物料与册一致放行。
    let actual = vec![
        BomLine { name: "kernel.elf".to_string(), fp: fnv1a32(b"KELF"), origin: Origin::Src },
        BomLine { name: "initrd.img".to_string(), fp: fnv1a32(b"IRD"), origin: Origin::Generated },
        BomLine { name: "lic.txt".to_string(), fp: fnv1a32(b"MIT"), origin: Origin::ThirdParty },
    ];
    accept(&b, &actual)?;
    hits += 1;

    // L4：指纹一致性——同内容登记指纹相等。
    let mut b2 = SbomBook::new()?;
    b2.register("x", b"payload", Origin::Src)?;
    b2.register("y", b"payload", Origin::Src)?; // 同内容异名合法
    if b2.lines[0].fp != b2.lines[1].fp {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：重名登记拒。
    let mut i1 = SbomBook::new()?;
    i1.register("a", b"1", Origin::Src)?;
    if i1.register("a", b"2", Origin::Src) != Err(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // I2：未封册导出拒。
    let i2 = SbomBook::new()?;
    if i2.export_sorted() != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    hits += 1;

    // I3：验收——未登记物料混入必红（E_MISS）。
    let mixed = vec![
        BomLine { name: "rogue.bin".to_string(), fp: 1, origin: Origin::Src },
    ];
    if accept(&b, &mixed) != Err(E_MISS) {
        return Err(E_MISS);
    }
    hits += 1;

    // I4：验收——登记物料内容被换必红（E_FP）；缺席必红（E_BAL）。
    let swapped = vec![
        BomLine { name: "kernel.elf".to_string(), fp: fnv1a32(b"TAMPER"), origin: Origin::Src },
        BomLine { name: "initrd.img".to_string(), fp: fnv1a32(b"IRD"), origin: Origin::Generated },
        BomLine { name: "lic.txt".to_string(), fp: fnv1a32(b"MIT"), origin: Origin::ThirdParty },
    ];
    if accept(&b, &swapped) != Err(E_FP) {
        return Err(E_FP);
    }
    let absent = vec![BomLine {
        name: "kernel.elf".to_string(),
        fp: fnv1a32(b"KELF"),
        origin: Origin::Src,
    }];
    if accept(&b, &absent) != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    // I5：封册后拒登记。
    let mut i5 = SbomBook::new()?;
    i5.seal();
    if i5.register("late", b"x", Origin::Src) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
