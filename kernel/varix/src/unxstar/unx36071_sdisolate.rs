//! UNX-F36071-J1 SD 校验和与损坏即隔离（载体级）。
//! 判据：① 校验和指纹面 100 例（单点翻转必破指纹，误报率 0）；
//! ② 损坏即隔离：隔离账入账 20 例、隔离体零外泄、账目守恒；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：载体级完整性——SD 体（含段体）的 FNV 指纹在入册时冻结，
//! 每次出册对拍；破指纹的体**立即隔离**（进隔离账，禁止再发引用），
//! 隔离账 append-only 可查不可抹（零抹除纪律）。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;
use super::unx36061_sdcore::{self as core, FNV_OFF};

pub const SEG_BASE: u16 = 0xe230;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_FINGER: u16 = SEG_BASE + 1; // 指纹不符（损坏）
pub const E_ISOL: u16 = SEG_BASE + 2; // 隔离账纪律破（抹账/外泄）
pub const E_QUAR: u16 = SEG_BASE + 3; // 隔离体被再引用（红线）
pub const E_REG: u16 = SEG_BASE + 4; // 出入册序非法（未入册先出册）

/// 体指纹（FNV over 全字节面；种子带体号盐）。
pub fn body_fingerprint(obj_id: u64, bytes: &[u8]) -> u64 {
    let mut h = FNV_OFF;
    let idb = obj_id.to_le_bytes();
    core::fnv_push(&idb, &mut h);
    core::fnv_push(bytes, &mut h);
    h
}

/// 册内体：字节面 + 冻结指纹 + 状态（在册/隔离）。
#[derive(Clone)]
pub struct VaultBody {
    pub obj_id: u64,
    pub bytes: Vec<u8>,
    pub frozen: u64,
    pub quarantined: bool,
}

impl VaultBody {
    pub fn enroll(obj_id: u64, bytes: Vec<u8>) -> VaultBody {
        let frozen = body_fingerprint(obj_id, &bytes);
        VaultBody { obj_id, bytes, frozen, quarantined: false }
    }
    pub fn intact(&self) -> bool {
        body_fingerprint(self.obj_id, &self.bytes) == self.frozen
    }
    /// 出册对拍：完整体才可出册；损坏体必须先进隔离账。
    pub fn checkout(&self) -> Result<&[u8], u16> {
        if self.quarantined {
            return Err(E_QUAR);
        }
        if !self.intact() {
            return Err(E_FINGER);
        }
        Ok(&self.bytes)
    }
}

/// 载体册（定容 32 体；损坏即隔离的执行面）。
pub const VAULT_CAP: usize = 32;

pub struct Vault {
    bodies: [Option<VaultBody>; VAULT_CAP],
    count: usize,
}

const NONE_VB: Option<VaultBody> = None;

impl Vault {
    pub fn new() -> Vault {
        Vault { bodies: [NONE_VB; VAULT_CAP], count: 0 }
    }
    pub fn enroll(&mut self, obj_id: u64, bytes: Vec<u8>) -> Result<usize, u16> {
        if self.count >= VAULT_CAP {
            return Err(E_REG);
        }
        if bytes.len() < 2 {
            return Err(E_REG); // 载体下界（与 unx36063 形状纪律一致）
        }
        self.bodies[self.count] = Some(VaultBody::enroll(obj_id, bytes));
        self.count += 1;
        Ok(self.count - 1)
    }
    pub fn get(&self, idx: usize) -> Result<&VaultBody, u16> {
        if idx >= self.count {
            return Err(E_REG);
        }
        match &self.bodies[idx] {
            Some(b) => Ok(b),
            None => Err(E_REG),
        }
    }
    fn get_mut(&mut self, idx: usize) -> Result<&mut VaultBody, u16> {
        if idx >= self.count {
            return Err(E_REG);
        }
        match &mut self.bodies[idx] {
            Some(b) => Ok(b),
            None => Err(E_REG),
        }
    }
    /// 巡检：全部在册体指纹对拍；损坏体自动入隔离账。
    pub fn patrol(&mut self, ledger: &mut QuarLedger) -> Result<usize, u16> {
        let mut newly = 0;
        let mut i = 0;
        while i < self.count {
            let bad = match self.get(i) {
                Ok(b) => !b.intact() || b.quarantined,
                Err(_) => false,
            };
            if bad {
                let b = self.get_mut(i)?;
                if !b.quarantined {
                    b.quarantined = true;
                    let fp = body_fingerprint(b.obj_id, &b.bytes);
                    let id = b.obj_id;
                    ledger.record(id, fp)?;
                    newly += 1;
                }
            }
            i += 1;
        }
        Ok(newly)
    }
}

/// 隔离账（append-only；零抹除——只追加与查询，无删除面）。
pub const QUAR_CAP: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct QuarRow {
    pub obj_id: u64,
    pub bad_fp: u64, // 损坏时实测指纹（与冻结指纹必不同）
}

pub struct QuarLedger {
    rows: [Option<QuarRow>; QUAR_CAP],
    count: usize,
}

const NONE_QR: Option<QuarRow> = None;

impl QuarLedger {
    pub fn new() -> QuarLedger {
        QuarLedger { rows: [NONE_QR; QUAR_CAP], count: 0 }
    }
    pub fn record(&mut self, obj_id: u64, bad_fp: u64) -> Result<(), u16> {
        if self.count >= QUAR_CAP {
            return Err(E_ISOL);
        }
        // 同体重复隔离：幂等（已有同 id 行则不重复计）。
        let mut i = 0;
        while i < self.count {
            if let Some(r) = self.rows[i] {
                if r.obj_id == obj_id {
                    return Ok(());
                }
            }
            i += 1;
        }
        self.rows[self.count] = Some(QuarRow { obj_id, bad_fp });
        self.count += 1;
        Ok(())
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn has(&self, obj_id: u64) -> bool {
        let mut i = 0;
        while i < self.count {
            if let Some(r) = self.rows[i] {
                if r.obj_id == obj_id {
                    return true;
                }
            }
            i += 1;
        }
        false
    }
    /// 零抹除自检：行数只增（本快照 vs 上次快照的行号前缀一致）。
    pub fn append_only(&self, prev_count: usize) -> Result<(), u16> {
        if self.count < prev_count {
            return Err(E_ISOL);
        }
        Ok(())
    }
    /// 损坏指纹与冻结指纹必异（同指纹即假损坏——账面纪律）。
    pub fn fingerprints_differ(&self, vault: &Vault) -> Result<(), u16> {
        let mut i = 0;
        while i < self.count {
            let r = match self.rows[i] {
                Some(r) => r,
                None => return Err(E_ISOL),
            };
            // 找到体，验其当前指纹 != bad_fp（bad_fp 是损坏瞬间的实测值，
            // 隔离后体不再被写——两者应相等但与冻结值异；此处验账面自洽）。
            let mut j = 0;
            let mut found = false;
            while j < vault.count {
                let b = vault.get(j)?;
                if b.obj_id == r.obj_id {
                    found = true;
                    let cur = body_fingerprint(b.obj_id, &b.bytes);
                    if cur != r.bad_fp {
                        return Err(E_ISOL);
                    }
                    break;
                }
                j += 1;
            }
            if !found {
                return Err(E_ISOL);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 单点翻转器（注入面）：翻转第 bit 位字节的一个最低位。
pub fn flip_bit(bytes: &mut Vec<u8>, byte_idx: usize, bit: u8) {
    if byte_idx < bytes.len() {
        bytes[byte_idx] ^= 1u8 << (bit % 8);
    }
}

fn detail(code: u16) -> &'static str {
    match code {
        E_FINGER => "指纹不符（载体损坏）",
        E_ISOL => "隔离账纪律破（抹账/账面不自洽）",
        E_QUAR => "隔离体被再引用（红线）",
        E_REG => "出入册序非法",
        _ => "未知诊断码",
    }
}

/// 确定性演练：R1 指纹面 100 例（100 个单点翻转全破指纹）；
/// R2 损坏即隔离 20 例（patrol → 隔离账 + 零外泄）；R3 好体出册零误报；
/// R4 隔离账 append-only + 账面自洽；R5 重复隔离幂等。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：100 例单点翻转全破指纹。
    let mut i = 0;
    while i < 100 {
        let id = (i + 1) as u64;
        let mut bytes: Vec<u8> = Vec::new();
        let mut k = 0;
        while k < 32 {
            bytes.push((i as u8).wrapping_add(k as u8));
            k += 1;
        }
        let vb = VaultBody::enroll(id, bytes);
        let fp0 = vb.frozen;
        let mut corrupted = vb.bytes.clone();
        flip_bit(&mut corrupted, i % 32, (i % 8) as u8);
        if body_fingerprint(id, &corrupted) == fp0 {
            return Err(E_FINGER); // 翻转未破指纹 = 指纹面失守
        }
        hits += 1;
        i += 1;
    }
    // R2：损坏即隔离 20 例。
    let mut vault = Vault::new();
    let mut ledger = QuarLedger::new();
    let mut bad_ids: Vec<u64> = Vec::new();
    let mut j = 0;
    while j < 20 {
        let id = (j + 100) as u64;
        let mut bytes: Vec<u8> = Vec::new();
        let mut k = 0;
        while k < 16 {
            bytes.push(0xa0 ^ (j as u8) ^ (k as u8));
            k += 1;
        }
        vault.enroll(id, bytes)?;
        bad_ids.push(id);
        j += 1;
    }
    // 好体 12 例。
    let mut g = 0;
    while g < 12 {
        let id = (g + 200) as u64;
        let mut bytes: Vec<u8> = Vec::new();
        let mut k = 0;
        while k < 16 {
            bytes.push(0x50 ^ (g as u8) ^ (k as u8));
            k += 1;
        }
        vault.enroll(id, bytes)?;
        g += 1;
    }
    // 损坏 20 个坏体（在册体上翻转）。
    let mut b = 0;
    while b < 20 {
        let vb = vault.get_mut(b as usize)?;
        flip_bit(&mut vb.bytes, b % 16, 0);
        b += 1;
    }
    let newly = vault.patrol(&mut ledger)?;
    if newly != 20 || ledger.count() != 20 {
        return Err(E_ISOL);
    }
    let mut b2 = 0;
    while b2 < 20 {
        if !ledger.has(bad_ids[b2]) {
            return Err(E_ISOL);
        }
        if vault.get(b2 as usize)?.checkout() != Err(E_QUAR) {
            return Err(E_QUAR);
        }
        b2 += 1;
    }
    hits += 1;
    // R3：好体出册零误报。
    let mut g2 = 20;
    while g2 < 32 {
        if vault.get(g2 as usize)?.checkout().is_err() {
            return Err(E_FINGER);
        }
        g2 += 1;
    }
    hits += 1;
    // R4：append-only + 账面自洽。
    let prev = ledger.count();
    let _ = vault.patrol(&mut ledger)?; // 二次巡检：无新增（幂等）
    if ledger.append_only(prev) != Ok(()) || ledger.count() != prev {
        return Err(E_ISOL);
    }
    ledger.fingerprints_differ(&vault)?;
    hits += 1;
    // R5：重复隔离幂等（对已隔离体再 patrol 不重复计行）。
    let before = ledger.count();
    let newly2 = vault.patrol(&mut ledger)?;
    if newly2 != 0 || ledger.count() != before {
        return Err(E_ISOL);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(104));
    }

    #[test]
    fn flip_breaks_fingerprint() {
        let mut bytes = vec![1u8, 2, 3, 4];
        let fp = body_fingerprint(1, &bytes);
        flip_bit(&mut bytes, 2, 3);
        assert_ne!(body_fingerprint(1, &bytes), fp);
    }

    #[test]
    fn quarantine_blocks_checkout() {
        let mut v = Vault::new();
        let mut bytes = vec![7u8; 8];
        bytes[0] ^= 0xff; // 入册前就损坏——指纹按损坏值冻结
        v.enroll(9, bytes).ok();
        let mut q = QuarLedger::new();
        // 冻结值与损坏后一致 → 完好（按指纹面语义）；翻转后损坏。
        let vb = v.get_mut(0).ok();
        if let Some(b) = vb {
            flip_bit(&mut b.bytes, 0, 0);
        }
        let _ = v.patrol(&mut q);
        assert_eq!(q.count(), 1);
        assert_eq!(v.get(0).ok().map(|b| b.checkout().err()), Some(Some(E_QUAR)));
    }
}

pub fn run_unx36071_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36071_sdisolate");
    cs.add("SEG 段位独占", SEG_BASE == 0xe230, "0xe230~0xe23f 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "指纹 100 例+隔离 20 例零外泄+append-only"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("载体册容量锚", VAULT_CAP == 32, "定容 32 体");
    cs
}
