//! UNX-F36056-J1 特权表版本化与扩展点（新特权 ADR 通道锚）。
//! 判据：① 表版本头+未知名拒绝语义 20 例断言；② 扩展登记接口
//! （文档化扩展点）fake 联签 10 例；③ 纯功能代码 ≥300 行。
//! 设计纪律：基础表（unx36041）冻结于 v1；扩展走登记通道——
//! 每条扩展必须携带 ADR 决议号（格式 ADR-<y>-<n>）与联签指纹，
//! 无 ADR 的扩展登记一律拒绝（ADR 通道锚）。

use super::unx36041_privtable::{self, name_wellformed, lookup_by_name, NAME_CAP};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xfe60;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本头不符
pub const E_NOENT: u16 = SEG_BASE + 2; // 未知名（拒绝语义）
pub const E_TABLE: u16 = SEG_BASE + 3; // 上游表纪律破
pub const E_NOADR: u16 = SEG_BASE + 4; // 无 ADR 决议号
pub const E_ADR: u16 = SEG_BASE + 5; // ADR 格式非法
pub const E_SIGN: u16 = SEG_BASE + 6; // 联签指纹缺失
pub const E_DUP: u16 = SEG_BASE + 7; // 扩展名/LUID 重复
pub const E_CAP: u16 = SEG_BASE + 8; // 扩展容量耗尽

/// 版本头：base 版 + 扩展数 + 头校验和。
pub const HEAD_VER_BASE: u32 = 1;
pub const EXT_CAP: usize = 16;

/// ADR 决议号缓冲上界："ADR-2026-100" = 12。
pub const ADR_CAP: usize = 16;

/// 扩展项：名称（规范形式）+ LUID low（≥1000 扩展段）+ ADR 号 + 联签。
pub struct ExtPriv {
    pub name: [u8; NAME_CAP],
    pub name_len: usize,
    pub luid_low: u32,
    pub adr: [u8; ADR_CAP],
    pub adr_len: usize,
    pub sign_fp: u64,
}

/// 扩展登记册。
pub struct ExtBook {
    pub items: [Option<ExtPriv>; EXT_CAP],
    pub len: usize,
}

impl ExtBook {
    pub const fn new() -> ExtBook {
        const NONE: Option<ExtPriv> = None;
        ExtBook {
            items: [NONE; EXT_CAP],
            len: 0,
        }
    }

    /// 扩展登记：ADR 格式 + 联签指纹 + 名称规范 + LUID 段 + 去重。
    pub fn register(
        &mut self,
        name: &[u8],
        luid_low: u32,
        adr: &[u8],
        sign_fp: u64,
    ) -> Result<usize, u16> {
        // ADR 通道锚：无决议号/格式非法一律拒绝。
        if adr.is_empty() {
            return Err(E_NOADR);
        }
        if !adr_wellformed(adr) {
            return Err(E_ADR);
        }
        if sign_fp == 0 {
            return Err(E_SIGN);
        }
        // 名称规范形式（复用 unx36041 判读面）。
        if !name_wellformed(name) {
            return Err(super::unx36041_privtable::E_NAME);
        }
        // 扩展 LUID 段：≥1000（避开基础表 2..18 与表外试探区）。
        if luid_low < 1000 {
            return Err(super::unx36041_privtable::E_RANGE);
        }
        // 去重：名称/LUID 均不得与基础表及既有扩展冲突。
        if lookup_by_name(name).is_ok() {
            return Err(E_DUP);
        }
        if super::unx36041_privtable::lookup_by_luid(luid_low).is_ok() {
            return Err(E_DUP);
        }
        let mut i = 0;
        while i < self.len {
            let e = self.items[i].as_ref().ok_or(E_CAP)?;
            if &e.name[..e.name_len] == name || e.luid_low == luid_low {
                return Err(E_DUP);
            }
            i += 1;
        }
        if self.len >= EXT_CAP {
            return Err(E_CAP);
        }
        let mut item = ExtPriv {
            name: [0; NAME_CAP],
            name_len: name.len(),
            luid_low,
            adr: [0; ADR_CAP],
            adr_len: adr.len(),
            sign_fp,
        };
        item.name[..name.len()].copy_from_slice(name);
        item.adr[..adr.len()].copy_from_slice(adr);
        self.items[self.len] = Some(item);
        self.len += 1;
        Ok(self.len - 1)
    }

    /// 扩展查找：名称 → 索引（登记面精确匹配）。
    pub fn find(&self, name: &[u8]) -> Result<usize, u16> {
        let mut i = 0;
        while i < self.len {
            let e = self.items[i].as_ref().ok_or(E_CAP)?;
            if e.name_len == name.len() && &e.name[..e.name_len] == name {
                return Ok(i);
            }
            i += 1;
        }
        Err(E_NOENT)
    }
}

/// ADR 号格式："ADR-<4位年>-<1..4位序号>" 全 ASCII。
pub fn adr_wellformed(a: &[u8]) -> bool {
    if a.len() < 9 || a.len() > ADR_CAP - 1 {
        return false;
    }
    let pfx = b"ADR-";
    let mut i = 0;
    while i < 4 {
        if a[i] != pfx[i] {
            return false;
        }
        i += 1;
    }
    // 年 4 位数字。
    i = 4;
    while i < 8 {
        if !a[i].is_ascii_digit() {
            return false;
        }
        i += 1;
    }
    if a[8] != b'-' {
        return false;
    }
    // 序号 ≥1 位数字。
    if a.len() < 10 {
        return false;
    }
    i = 9;
    while i < a.len() {
        if !a[i].is_ascii_digit() {
            return false;
        }
        i += 1;
    }
    true
}

/// 版本头校验：base 版 + 扩展数 ≤ 容量 + 头和稳定。
pub fn head_check(book: &ExtBook) -> Result<u64, u16> {
    if HEAD_VER_BASE != 1 {
        return Err(E_VER);
    }
    if book.len > EXT_CAP {
        return Err(E_CAP);
    }
    // 头校验和：FNV over (ver, len, 逐项 luid+sign)。
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let push = |bytes: &[u8], h: &mut u64| {
        let mut i = 0;
        while i < bytes.len() {
            *h ^= bytes[i] as u64;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
    };
    push(&HEAD_VER_BASE.to_le_bytes(), &mut h);
    push(&(book.len as u64).to_le_bytes(), &mut h);
    let mut i = 0;
    while i < book.len {
        let e = book.items[i].as_ref().ok_or(E_CAP)?;
        push(&e.luid_low.to_le_bytes(), &mut h);
        push(&e.sign_fp.to_le_bytes(), &mut h);
        i += 1;
    }
    Ok(h)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本头不符",
        E_NOENT => "未知名（拒绝语义）",
        E_TABLE => "上游表纪律破",
        E_NOADR => "无 ADR 决议号",
        E_ADR => "ADR 格式非法",
        E_SIGN => "联签指纹缺失",
        E_DUP => "扩展名/LUID 重复",
        E_CAP => "扩展容量耗尽",
        _ => "未知诊断码",
    }
}

/// 演练：R1 上游表+版本头；R2 未知名拒绝 20 例（基础表+扩展面）；
/// R3 fake 联签扩展登记 10 例全对（ADR 格式合法）；R4 无 ADR/坏
/// ADR/零签登记全拒；R5 头校验和随登记单调变化（版本化面）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：上游表 + 版本头。
    unx36041_privtable::table_integrity().map_err(|_| E_TABLE)?;
    let mut book = ExtBook::new();
    head_check(&book)?;
    hits += 1;
    // R2：未知名拒绝 20 例——基础表面 10 例（E_NOENT）+ 扩展面 10 例。
    let unknown: [&[u8]; 10] = [
        b"SeDebugPrivilege", b"SeXyzPrivilege", b"", b"Se Privilege",
        b"SeAaaPrivilege", b"SeBbbPrivilege", b"SeCccPrivilege", b"SeDddPrivilege",
        b"SeEeePrivilege", b"SeFffPrivilege",
    ];
    let mut u = 0;
    while u < 10 {
        if lookup_by_name(unknown[u]).is_ok() {
            return Err(E_NOENT);
        }
        if book.find(unknown[u]).is_ok() {
            return Err(E_NOENT);
        }
        hits += 1;
        u += 1;
    }
    // R3：fake 联签登记 10 例——名称 XxxYyyPrivilege、LUID 1000+、
    // ADR-2026-N、联签 FNV 指纹。
    let mut c = 0;
    while c < 10 {
        let mut name = [0u8; NAME_CAP];
        let nm = b"SeExtPrivilege";
        name[..nm.len()].copy_from_slice(nm);
        // 名称去重：后缀加数字（c=0 → SeExt0Privilege 形变——保持规范）。
        // 规范形式要求主体段字母——改用不同主体词根。
        let roots: [&[u8]; 10] = [
            b"AuditLog", b"QuotaAdj", b"MemProf", b"TimeSync", b"DevInst",
            b"CredMgr", b"NetCfg", b"PowerMgr", b"LicMgr", b"VaultMgr",
        ];
        let root = roots[c];
        // name = "Se" + root + "Privilege"。
        name = [0; NAME_CAP];
        name[..2].copy_from_slice(b"Se");
        let mut n = 2;
        name[n..n + root.len()].copy_from_slice(root);
        n += root.len();
        name[n..n + 9].copy_from_slice(b"Privilege");
        n += 9;
        let luid = 1000 + c as u32;
        let mut adr = [0u8; ADR_CAP];
        let (adr_s, adr_len) = format_no_alloc(c);
        adr[..adr_len].copy_from_slice(&adr_s[..adr_len]);
        let sign = fnv64(&name[..n]);
        let idx = book.register(&name[..n], luid, &adr[..adr_len], sign)?;
        if idx != c {
            return Err(E_CAP);
        }
        // 回查命中。
        if book.find(&name[..n])? != idx {
            return Err(E_NOENT);
        }
        hits += 1;
        c += 1;
    }
    // R4：无 ADR/坏 ADR/零签全拒。
    let nm = b"SeTestZzzPrivilege";
    if book.register(nm, 2000, &[], 7) != Err(E_NOADR) {
        return Err(E_NOADR);
    }
    if book.register(nm, 2000, b"ADR-26-1", 7) != Err(E_ADR) {
        return Err(E_ADR);
    }
    if book.register(nm, 2000, b"ADR-2026-", 7) != Err(E_ADR) {
        return Err(E_ADR);
    }
    if book.register(nm, 2000, b"ADR-2026-1", 0) != Err(E_SIGN) {
        return Err(E_SIGN);
    }
    if book.register(b"SeBack-upPrivilege", 2000, b"ADR-2026-1", 7)
        != Err(super::unx36041_privtable::E_NAME)
    {
        return Err(super::unx36041_privtable::E_NAME);
    }
    // 基础表重名拒绝。
    if book.register(b"SeBackupPrivilege", 2000, b"ADR-2026-1", 7) != Err(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;
    // R5：头校验和随登记变化（版本化面）。
    let h0 = head_check(&ExtBook::new())?;
    let h1 = head_check(&book)?;
    if h0 == h1 {
        return Err(E_VER);
    }
    hits += 1;
    Ok(hits)
}

/// 无 alloc 的 ADR 号格式化："ADR-2026-<n>"，返回 (缓冲, 实长)。
fn format_no_alloc(n: usize) -> ([u8; ADR_CAP], usize) {
    let mut out = [0u8; ADR_CAP];
    let pfx = b"ADR-2026-";
    out[..9].copy_from_slice(&pfx[..]);
    // 序号十进制（≤2 位足够 10 例）。
    let tens = (n / 10) as u8;
    let ones = (n % 10) as u8;
    let mut w = 9;
    if tens > 0 {
        out[w] = b'0' + tens;
        w += 1;
    }
    out[w] = b'0' + ones;
    w += 1;
    (out, w)
}

/// FNV-1a 64（联签指纹）。
fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(23));
    }

    #[test]
    fn adr_format_gate() {
        assert!(adr_wellformed(b"ADR-2026-1"));
        assert!(adr_wellformed(b"ADR-2026-100"));
        assert!(!adr_wellformed(b"ADR-26-1"));
        assert!(!adr_wellformed(b"adr-2026-1"));
        assert!(!adr_wellformed(b"ADR-2026-"));
    }

    #[test]
    fn no_adr_rejected() {
        let mut b = ExtBook::new();
        assert_eq!(
            b.register(b"SeXyzPrivilege", 1000, &[], 1),
            Err(E_NOADR)
        );
    }
}

pub fn run_unx36056_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36056_privversion");
    cs.add("SEG 段位独占", SEG_BASE == 0xFE60, "0xFE60~0xFE7F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}
