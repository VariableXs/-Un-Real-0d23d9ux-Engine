//! unx48295_earlyrcpt — UNX-F48295 B01–B02 提前放行消费回执归档（F4 波 12 与 L2 出厂消费证据）（诊断段 0xC5C0）。
//!
//! 判据原文：
//!   J1 回执归档零缺件；判据锚定 附则二 提前放行总表。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 回执清单制：B01/B02 两批提前放行项，每项须归档两份消费证据
//!      ——F4 波 12 证据与 L2 出厂证据；清单槽位（批次×证据类）预登记，
//!      归档按槽位填装，槽外证据拒收。
//!   2. 证据形制：回执 = (槽位号, 批次, 证据类, 凭证码, 校验和)，
//!      校验和 = 批次^证据类^凭证码 XOR 折叠；校验和不符的证据拒收
//!      （防伪造凭证入档）。
//!   3. 零缺件口径（J1）：清单槽位全满（无空槽）且逐槽校验和复算
//!      通过、无重复槽号、无跨批错装——四项全过才零缺件；缺件清单
//!      逐槽留痕（槽号+缺因），缺件态禁止归档关闭。
//!   4. 归档关闭：零缺件复核通过后一次封档；封档后补装/换装/重封
//!      一律拒收并留拒收痕——归档即终态。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC5C0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_SLOT_BAD: u16 = SEG_BASE + 1;   // 槽号越界/重复
const E_CRC_BAD: u16 = SEG_BASE + 2;    // 凭证校验和不符
const E_SLOT_MISSING: u16 = SEG_BASE + 3; // 缺件
const E_SEALED_EDIT: u16 = SEG_BASE + 4; // 封档后改动
const E_BATCH_BAD: u16 = SEG_BASE + 5;   // 跨批错装
const E_LEDGER_BAD: u16 = SEG_BASE + 6;  // 归档账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 7;   // 写回摘要为空

/// 批次闭集。
pub const BATCH_B01: u8 = 0;
pub const BATCH_B02: u8 = 1;
/// 证据类闭集。
pub const EV_F4_WAVE12: u8 = 0; // F4 波 12 证据
pub const EV_L2_FACTORY: u8 = 1; // L2 出厂证据

/// 清单槽位数：2 批 × 2 证据类。
pub const SLOT_COUNT: usize = 4;

/// 槽位号规则：slot = batch * 2 + evidence。
pub fn slot_of(batch: u8, ev: u8) -> Result<usize, u16> {
    if batch > BATCH_B02 || ev > EV_L2_FACTORY {
        return Err(E_BATCH_BAD);
    }
    Ok(batch as usize * 2 + ev as usize)
}

/// 一份回执。
#[derive(Clone, Copy, Debug)]
pub struct Receipt {
    pub slot: usize,
    pub batch: u8,
    pub ev: u8,
    pub voucher: u32,
    pub crc: u32,
}

impl Receipt {
    /// 打包：crc = 批次 ^ 证据类 ^ 凭证码 ^ 槽号（异或折叠）。
    pub fn pack(batch: u8, ev: u8, voucher: u32) -> Result<Receipt, u16> {
        let slot = slot_of(batch, ev)?;
        let crc = batch as u32 ^ ev as u32 ^ voucher ^ slot as u32;
        Ok(Receipt { slot, batch, ev, voucher, crc })
    }
    pub fn validate(&self) -> Result<(), u16> {
        let want = self.batch as u32 ^ self.ev as u32 ^ self.voucher ^ self.slot as u32;
        if self.crc != want {
            return Err(E_CRC_BAD);
        }
        Ok(())
    }
}

/// 一条缺件账。
#[derive(Clone, Copy, Debug)]
pub struct MissEntry {
    pub slot: usize,
    pub reason: u16, // E_SLOT_MISSING / E_CRC_BAD
}

/// 回执归档枢纽。
pub struct EarlyRcpt {
    slots: Vec<Option<Receipt>>,
    pub archived_total: u32,
    pub reject_total: u32,
    pub sealed: bool,
    missing: Vec<MissEntry>,
}

impl EarlyRcpt {
    pub fn new() -> EarlyRcpt {
        let mut slots: Vec<Option<Receipt>> = Vec::new();
        let mut i = 0usize;
        while i < SLOT_COUNT {
            slots.push(None);
            i += 1;
        }
        EarlyRcpt { slots, archived_total: 0, reject_total: 0, sealed: false, missing: Vec::new() }
    }

    /// 归档一份回执：按槽装填，重复槽/坏校验/跨批错装拒收留痕。
    pub fn archive(&mut self, r: Receipt) -> Result<(), u16> {
        if self.sealed {
            self.reject_total += 1;
            return Err(E_SEALED_EDIT);
        }
        if r.slot >= SLOT_COUNT {
            self.reject_total += 1;
            return Err(E_SLOT_BAD);
        }
        // 槽号与批次×证据类重排一致（防跨批错装）。
        let want_slot = slot_of(r.batch, r.ev)?;
        if want_slot != r.slot {
            self.reject_total += 1;
            return Err(E_BATCH_BAD);
        }
        r.validate()?;
        if self.slots[r.slot].is_some() {
            self.reject_total += 1;
            return Err(E_SLOT_BAD); // 重复归档
        }
        self.slots[r.slot] = Some(r);
        self.archived_total += 1;
        Ok(())
    }

    /// 零缺件复核（J1）：槽全满 + 逐槽校验复算 + 无缺件；缺件留痕。
    pub fn audit_complete(&mut self) -> Result<(), u16> {
        self.missing.clear();
        let mut i = 0usize;
        while i < SLOT_COUNT {
            match self.slots[i] {
                None => {
                    self.missing.push(MissEntry { slot: i, reason: E_SLOT_MISSING });
                }
                Some(r) => {
                    if r.validate().is_err() {
                        self.missing.push(MissEntry { slot: i, reason: E_CRC_BAD });
                    }
                }
            }
            i += 1;
        }
        if !self.missing.is_empty() {
            return Err(E_SLOT_MISSING);
        }
        if self.archived_total != SLOT_COUNT as u32 {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 封档：零缺件后才可封；重复封档拒。
    pub fn seal(&mut self) -> Result<(), u16> {
        if self.sealed {
            self.reject_total += 1;
            return Err(E_SEALED_EDIT);
        }
        self.audit_complete()?;
        self.sealed = true;
        Ok(())
    }

    /// 缺件清单长度（留痕口径）。
    pub fn missing_len(&self) -> usize {
        self.missing.len()
    }

    /// ×N 覆盖核验。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.archived_total < want {
            return Err(E_SLOT_MISSING);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || !self.sealed {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（回执归档零缺件），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主归档：4 槽全装（B01/F4、B01/L2、B02/F4、B02/L2）→
    //    零缺件复核 → 封档 → 复核封档态。
    let mut ar = EarlyRcpt::new();
    ar.archive(Receipt::pack(BATCH_B01, EV_F4_WAVE12, 0x1101)?)?;
    ar.archive(Receipt::pack(BATCH_B01, EV_L2_FACTORY, 0x1102)?)?;
    ar.archive(Receipt::pack(BATCH_B02, EV_F4_WAVE12, 0x2201)?)?;
    ar.archive(Receipt::pack(BATCH_B02, EV_L2_FACTORY, 0x2202)?)?;
    ar.coverage(4)?;
    ar.audit_complete()?;
    ar.seal()?;
    if !ar.sealed || ar.missing_len() != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 缺件抓红：只装 3 槽，audit_complete 必红且缺件账指认空槽。
    let mut ar = EarlyRcpt::new();
    ar.archive(Receipt::pack(BATCH_B01, EV_F4_WAVE12, 0x1101)?)?;
    ar.archive(Receipt::pack(BATCH_B01, EV_L2_FACTORY, 0x1102)?)?;
    ar.archive(Receipt::pack(BATCH_B02, EV_F4_WAVE12, 0x2201)?)?;
    if ar.audit_complete().is_ok() {
        return Err(E_SLOT_MISSING);
    }
    if ar.missing_len() != 1 {
        return Err(E_LEDGER_BAD);
    }
    // 缺件态封档拒（防带缺关闭）。
    if ar.seal().is_ok() {
        return Err(E_SLOT_MISSING);
    }
    hits += 1;

    // R3 凭证校验拦截：手工翻 CRC 的回执拒收、不占槽、拒收留痕。
    let mut ar = EarlyRcpt::new();
    let mut bad = Receipt::pack(BATCH_B01, EV_F4_WAVE12, 0x1101)?;
    bad.crc ^= 0xFF;
    if ar.archive(bad).is_ok() {
        return Err(E_CRC_BAD);
    }
    if ar.archived_total != 0 || ar.reject_total != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R4 跨批错装拦截：B02 批证据装到 B01 槽位（slot 手改）必拒。
    let mut ar = EarlyRcpt::new();
    let mut wrong = Receipt::pack(BATCH_B02, EV_F4_WAVE12, 0x2201)?;
    wrong.slot = slot_of(BATCH_B01, EV_F4_WAVE12)?; // 错装槽
    // 重算 crc 使其过 CRC 关——错装拦截必须独立于 CRC 关。
    wrong.crc = wrong.batch as u32 ^ wrong.ev as u32 ^ wrong.voucher ^ wrong.slot as u32;
    if ar.archive(wrong).is_ok() {
        return Err(E_BATCH_BAD);
    }
    hits += 1;

    // R5 重复槽与越槽拦截：同槽二装拒、slot=4 越界拒，账零污染。
    let mut ar = EarlyRcpt::new();
    let r0 = Receipt::pack(BATCH_B01, EV_F4_WAVE12, 0x1101)?;
    ar.archive(r0)?;
    let dup = Receipt::pack(BATCH_B01, EV_F4_WAVE12, 0x9999)?;
    if ar.archive(dup).is_ok() {
        return Err(E_SLOT_BAD);
    }
    let mut oob = Receipt::pack(BATCH_B02, EV_L2_FACTORY, 0x2202)?;
    oob.slot = 4;
    oob.crc = oob.batch as u32 ^ oob.ev as u32 ^ oob.voucher ^ oob.slot as u32;
    if ar.archive(oob).is_ok() {
        return Err(E_SLOT_BAD);
    }
    if ar.archived_total != 1 || ar.reject_total != 2 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 封档不可逆：封档后补装/换装/重封全拒且拒收逐次留痕。
    let mut ar = EarlyRcpt::new();
    ar.archive(Receipt::pack(BATCH_B01, EV_F4_WAVE12, 0x1101)?)?;
    ar.archive(Receipt::pack(BATCH_B01, EV_L2_FACTORY, 0x1102)?)?;
    ar.archive(Receipt::pack(BATCH_B02, EV_F4_WAVE12, 0x2201)?)?;
    ar.archive(Receipt::pack(BATCH_B02, EV_L2_FACTORY, 0x2202)?)?;
    ar.seal()?;
    if ar.archive(Receipt::pack(BATCH_B02, EV_L2_FACTORY, 0x7777)?).is_ok() {
        return Err(E_SEALED_EDIT);
    }
    if ar.seal().is_ok() {
        return Err(E_SEALED_EDIT);
    }
    if ar.reject_total != 2 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 槽位映射闭集：slot_of 四组合与逆推一致；越批/越证据类拒。
    if slot_of(BATCH_B01, EV_F4_WAVE12)? != 0
        || slot_of(BATCH_B01, EV_L2_FACTORY)? != 1
        || slot_of(BATCH_B02, EV_F4_WAVE12)? != 2
        || slot_of(BATCH_B02, EV_L2_FACTORY)? != 3
    {
        return Err(E_LEDGER_BAD);
    }
    if slot_of(2, EV_F4_WAVE12).is_ok() || slot_of(BATCH_B01, 3).is_ok() {
        return Err(E_BATCH_BAD);
    }
    hits += 1;

    // R8 缺件补齐路径与写回（判据 J2）：缺 1 槽→补装→复核转绿→封档；
    //    空摘要与未封档写回拒。
    let mut ar = EarlyRcpt::new();
    ar.archive(Receipt::pack(BATCH_B01, EV_F4_WAVE12, 0x1101)?)?;
    ar.archive(Receipt::pack(BATCH_B01, EV_L2_FACTORY, 0x1102)?)?;
    ar.archive(Receipt::pack(BATCH_B02, EV_F4_WAVE12, 0x2201)?)?;
    if ar.audit_complete().is_ok() {
        return Err(E_SLOT_MISSING);
    }
    ar.archive(Receipt::pack(BATCH_B02, EV_L2_FACTORY, 0x2202)?)?;
    ar.audit_complete()?;
    ar.seal()?;
    if ar.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    ar.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
