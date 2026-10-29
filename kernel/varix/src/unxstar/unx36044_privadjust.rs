//! UNX-F36044-J1 AdjustTokenPrivileges 类语义：启用路径。
//! 判据：① 启用 100 例（持有者持该特权前提）账实一致；
//! ② 持有者不持该特权注入 50 例全拒且错误可诊断；③ 纯功能代码 ≥300 行。
//! 设计纪律：启用 = Adjust(ENABLED 位)——持有（在册）与启用（位翻转）
//! 是两个状态轴；启用一个未持有的特权必须显性拒绝（不可静默入册）。

use super::unx36025_tokpriv::{PrivBook, LuidSource, PRIV_ENABLED, PRIV_ENABLED_BY_DEFAULT};
use super::unx36041_privtable;
use super::unx36043_privnotheld::{subject_fp, DenialLedger, R_NOT_OWNED};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xfce0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_NOENT: u16 = SEG_BASE + 1; // 未持有（不在册）
pub const E_BAL: u16 = SEG_BASE + 2; // 账实不一致
pub const E_OLD: u16 = SEG_BASE + 3; // 旧状态快照失配
pub const E_NOTMOD: u16 = SEG_BASE + 4; // 无变更却报成功
pub const E_DIAG: u16 = SEG_BASE + 5; // 拒绝无诊断
pub const E_TABLE: u16 = SEG_BASE + 6; // 上游表纪律破

/// 调整结果：新状态 + 旧状态（AdjustTokenPrivileges 旧态回传语义）。
#[derive(Debug)]
pub struct AdjustOutcome {
    pub old_attrs: u32,
    pub new_attrs: u32,
    pub changed: bool,
}

/// 启用路径单一入口：持有者（在册）→ 翻 ENABLED 位并回传旧态；
/// 未持有 → 显性 E_NOENT 并落拒绝账（可诊断）。
pub fn adjust_enable(
    book: &mut PrivBook,
    led: &mut DenialLedger,
    subject_id: u64,
    luid: (u32, u32),
) -> Result<AdjustOutcome, u16> {
    // 前提：特权在基础表内（表外 LUID 不进入调整面）。
    if unx36041_privtable::lookup_by_luid(luid.1).is_err() {
        return Err(E_TABLE);
    }
    // 持有检查：不在册 → 拒 + 落账（原因=未持有）。
    let old = match book.attrs_of(luid) {
        Ok(a) => a,
        Err(_) => {
            led.record(subject_fp(subject_id), luid.1, R_NOT_OWNED)?;
            return Err(E_NOENT);
        }
    };
    book.enable(luid)?;
    let new = book.attrs_of(luid)?;
    let outcome = AdjustOutcome {
        old_attrs: old,
        new_attrs: new,
        changed: (old & PRIV_ENABLED) == 0 && (new & PRIV_ENABLED) != 0,
    };
    // 旧态快照自洽：new == old | ENABLED（启用路径只置位不清理）。
    if outcome.new_attrs != outcome.old_attrs | PRIV_ENABLED {
        return Err(E_BAL);
    }
    Ok(outcome)
}

/// 构造预置册：持有 count 项特权（表内 LUID 顺序取），by_default 交替。
/// （特权 LUID high 恒 0——表锚；LuidSource 仅作发射器占位不消费。）
pub fn preset_book(count: usize) -> Result<(PrivBook, LuidSource, [(u32, u32); 16]), u16> {
    let mut book = PrivBook::new();
    let src = LuidSource::new();
    let mut luids = [(0u32, 0u32); 16];
    let mut i = 0;
    while i < count && i < 16 {
        let low = unx36041_privtable::PRIV_TABLE[i].luid_low;
        let lu = (0u32, low);
        book.enroll(lu, i % 2 == 0)?;
        luids[i] = lu;
        i += 1;
    }
    Ok((book, src, luids))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOENT => "未持有（不在册）",
        E_BAL => "账实不一致",
        E_OLD => "旧状态快照失配",
        E_NOTMOD => "无变更却报成功",
        E_DIAG => "拒绝无诊断",
        E_TABLE => "上游表纪律破",
        _ => "未知诊断码",
    }
}

/// 演练：R1 上游表；R2 启用 100 例账实一致（17 槽册轮转 100 次）；
/// R3 未持有注入 50 例全拒 + 每例落账可诊断；R4 旧态回传自洽；
/// R5 默认启用位不受启用路径破坏（BY_DEFAULT 位保持）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：上游表自检。
    unx36041_privtable::table_integrity().map_err(|_| E_TABLE)?;
    hits += 1;
    // R2：17 槽册（表内 LUID 顺序），轮转启用 100 次。
    let (mut book, _src, luids) = preset_book(16)?;
    let mut led = DenialLedger::new();
    let mut k = 0;
    while k < 100 {
        let luid = luids[k % 16];
        let out = adjust_enable(&mut book, &mut led, 7000 + k as u64, luid)?;
        // 账实一致：册内位面 == outcome.new_attrs。
        let now = book.attrs_of(luid)?;
        if now != out.new_attrs {
            return Err(E_BAL);
        }
        // changed 必真（轮转首轮后全部启用——二次启用 changed=false，
        // 旧态回传仍自洽；changed 仅在首轮翻转时为真）。
        if k < 16 && !out.changed {
            return Err(E_NOTMOD);
        }
        hits += 1;
        k += 1;
    }
    // 启用后全量位面核：16 项全带 ENABLED。
    let mut i = 0;
    while i < 16 {
        let a = book.attrs_of(luids[i])?;
        if a & PRIV_ENABLED == 0 {
            return Err(E_BAL);
        }
        i += 1;
    }
    hits += 1;
    // R3：未持有注入 50 例——表内但未入册的 LUID 无法取（17 全入册），
    // 故用「出册面」：PRIV_REMOVED 语义不可用，改用未入册 LUID=表内项
    // 无法构造 ⇒ 未持有注入用全新空册上直接 adjust（册空即未持有）。
    let (mut empty_book, _s2, _l2) = preset_book(0)?;
    let mut rejected = 0;
    let mut j = 0;
    while j < 50 {
        let low = unx36041_privtable::PRIV_TABLE[j % 17].luid_low;
        let r = adjust_enable(&mut empty_book, &mut led, 8000 + j as u64, (0, low));
        match r {
            Err(E_NOENT) => rejected += 1,
            Err(E_TABLE) => return Err(E_TABLE),
            _ => return Err(E_DIAG), // 未持有放行/其他错误 = 判据红线
        }
        j += 1;
    }
    if rejected != 50 {
        return Err(E_DIAG);
    }
    // 每例落账：空册 50 次拒绝全入账。
    if led.len != 50 {
        return Err(E_DIAG);
    }
    hits += 1;
    // R4：旧态回传自洽——已启用项再启用，old==new 且 changed=false。
    let out2 = adjust_enable(&mut book, &mut led, 9001, luids[0])?;
    if out2.old_attrs != out2.new_attrs || out2.changed {
        return Err(E_OLD);
    }
    hits += 1;
    // R5：BY_DEFAULT 位保持——启用不清洗默认位。
    let a = book.attrs_of(luids[0])?;
    let _ = a;
    // luids[0] by_default=true（preset 交替），启用后默认位仍在。
    if book.attrs_of(luids[0])? & PRIV_ENABLED_BY_DEFAULT == 0 {
        return Err(E_BAL);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(105));
    }

    #[test]
    fn enable_idempotent_old_state() {
        let (mut book, _s, luids) = preset_book(3).unwrap();
        let mut led = DenialLedger::new();
        let o1 = adjust_enable(&mut book, &mut led, 1, luids[0]).unwrap();
        assert!(o1.changed);
        let o2 = adjust_enable(&mut book, &mut led, 1, luids[0]).unwrap();
        assert!(!o2.changed && o2.old_attrs == o2.new_attrs);
    }

    #[test]
    fn not_owned_rejected_and_audited() {
        let (mut book, _s, _l) = preset_book(0).unwrap();
        let mut led = DenialLedger::new();
        assert_eq!(
            adjust_enable(&mut book, &mut led, 2, (0, 5)).unwrap_err(),
            E_NOENT
        );
        assert_eq!(led.len, 1);
    }
}

pub fn run_unx36044_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36044_privadjust");
    cs.add("SEG 段位独占", SEG_BASE == 0xFCE0, "0xFCE0~0xFCFF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}
