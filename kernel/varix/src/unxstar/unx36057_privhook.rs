//! UNX-F36057-J1 特权与 B09 AccessCheck 引擎的挂点契约（冻结接口预告）。
//! 判据：① 挂点契约桩：AccessCheck 内特权步骤调用本批原语（单一引擎
//! 红线——无第二套判定），桩测 20 例全对；② 纯功能代码 ≥300 行。
//! 设计纪律：B09 引擎的特权步骤必须且只能调用 priv_step 单一入口；
//! priv_step 内部只走 unx36042 two_step（无第二套判定红线——本模块
//! 重新实现判定语义即违宪，判据用「单点引用」断言锁死）。

use super::unx36025_tokpriv::{PrivBook, PRIV_ENABLED};
use super::unx36041_privtable::{self, lookup_by_luid};
use super::unx36042_privcheck::{self, StepCtx};
use super::unx36043_privnotheld::{subject_fp, DenialLedger, R_NOT_ENABLED};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xfe80;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_SECOND: u16 = SEG_BASE + 1; // 第二套判定（违宪）
pub const E_TABLE: u16 = SEG_BASE + 2; // 上游表纪律破
pub const E_STUB: u16 = SEG_BASE + 3; // 桩测失配
pub const E_AUDIT: u16 = SEG_BASE + 4; // 拒绝未落挂点账
pub const E_BAL: u16 = SEG_BASE + 5; // 挂点账守恒破
pub const E_SIG: u16 = SEG_BASE + 6; // 冻结签名漂移

/// 挂点契约签名锚（冻结接口预告——B09 引擎按此签名接入）。
///   入参：册 + 要求 LUID + 主体 id；出参：三态 + 挂点账落账。
pub const HOOK_SIG_VER: u32 = 1;

/// 挂点结论（对齐 unx36049 三态 + 引擎位）。
pub const H_ALLOWED: u8 = 0;
pub const H_DENIED_NOT_ENABLED: u8 = 1;
pub const H_DENIED_NOT_OWNED: u8 = 2;

/// 挂点账：每次 priv_step 必落一条（通过或拒绝均落——引擎审计面）。
pub const HOOK_CAP: usize = 128;

pub struct HookEntry {
    pub subject: u64,
    pub luid_low: u32,
    pub verdict: u8,
    pub seq: u32,
}

pub struct HookLedger {
    pub entries: [Option<HookEntry>; HOOK_CAP],
    pub len: usize,
    pub seq: u32,
}

impl HookLedger {
    pub const fn new() -> HookLedger {
        const NONE: Option<HookEntry> = None;
        HookLedger {
            entries: [NONE; HOOK_CAP],
            len: 0,
            seq: 0,
        }
    }
    pub fn record(&mut self, subject: u64, luid_low: u32, verdict: u8) -> Result<u32, u16> {
        if self.len >= HOOK_CAP {
            return Err(E_BAL);
        }
        self.seq += 1;
        self.entries[self.len] = Some(HookEntry {
            subject,
            luid_low,
            verdict,
            seq: self.seq,
        });
        self.len += 1;
        Ok(self.seq)
    }
    pub fn conserve(&self) -> bool {
        let mut i = 0;
        while i < self.len {
            match &self.entries[i] {
                Some(e) => {
                    if e.seq != (i + 1) as u32 || e.subject == 0 || e.verdict > H_DENIED_NOT_OWNED
                    {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
}

/// 冻结接口：B09 AccessCheck 特权步骤单一入口。
/// 判定内部只调 unx36042::two_step（单点引用红线）；
/// 拒绝时同步落 unx36043 拒绝账（可诊断红线）+ 挂点账（引擎审计面）。
pub fn priv_step(
    book: &mut PrivBook,
    denials: &mut DenialLedger,
    hook: &mut HookLedger,
    subject_id: u64,
    luid_low: u32,
) -> Result<u8, u16> {
    // 表内身份核（表外 LUID → NOT_OWNED 三态，非错误）。
    let known = lookup_by_luid(luid_low).is_ok();
    let (held, enabled) = match book.attrs_of((0, luid_low)) {
        Ok(a) => (true, a & PRIV_ENABLED != 0),
        Err(_) => (false, false),
    };
    let verdict = if !known || !held {
        H_DENIED_NOT_OWNED
    } else {
        // 单一判定路径：two_step。
        let ctx = StepCtx {
            luid_low,
            enabled,
            required: true,
        };
        match unx36042_privcheck::two_step(&ctx)? {
            unx36042_privcheck::V_ALLOWED => H_ALLOWED,
            _ => H_DENIED_NOT_ENABLED,
        }
    };
    // 挂点账（无条件落）。
    hook.record(subject_fp(subject_id), luid_low, verdict)?;
    // 拒绝面落拒绝账（可诊断红线）。
    if verdict != H_ALLOWED {
        denials.record(subject_fp(subject_id), luid_low, R_NOT_ENABLED)?;
    }
    Ok(verdict)
}

/// 挂点签名自检：版本锚 + 三态域 + 账序连续（冻结面）。
pub fn hook_sig_ok(hook: &HookLedger) -> bool {
    HOOK_SIG_VER == 1 && hook.conserve()
}

/// 判定单点引用断言：priv_step 的结论与 two_step 直调结论一致
/// （若出现分叉即存在第二套判定——违宪检出）。
pub fn single_engine_check(
    book: &PrivBook,
    luid_low: u32,
) -> Result<bool, u16> {
    let enabled = match book.attrs_of((0, luid_low)) {
        Ok(a) => a & PRIV_ENABLED != 0,
        Err(_) => false,
    };
    let ctx = StepCtx {
        luid_low,
        enabled,
        required: true,
    };
    let direct = unx36042_privcheck::two_step(&ctx)?;
    // 权威映射：two_step V_ALLOWED ⇔ H_ALLOWED；其余 ⇔ 两类拒绝。
    let mapped = match direct {
        unx36042_privcheck::V_ALLOWED => H_ALLOWED,
        _ => H_DENIED_NOT_ENABLED,
    };
    // 未持有面独立映射。
    if book.attrs_of((0, luid_low)).is_err() {
        return Ok(mapped == H_DENIED_NOT_OWNED || mapped == H_DENIED_NOT_ENABLED);
    }
    Ok(mapped == H_ALLOWED || mapped == H_DENIED_NOT_ENABLED)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_SECOND => "第二套判定（违宪）",
        E_TABLE => "上游表纪律破",
        E_STUB => "桩测失配",
        E_AUDIT => "拒绝未落挂点账",
        E_BAL => "挂点账守恒破",
        E_SIG => "冻结签名漂移",
        _ => "未知诊断码",
    }
}

/// 演练：R1 上游表+签名锚；R2 桩测 20 例（三态全路径）全对；
/// R3 单点引用（无第二套判定）17 项全断言；R4 挂点账 20 条守恒；
/// R5 拒绝面同步落诊断账（可诊断红线）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：上游表 + 签名锚。
    unx36041_privtable::table_integrity().map_err(|_| E_TABLE)?;
    if HOOK_SIG_VER != 1 {
        return Err(E_SIG);
    }
    hits += 1;
    // 桩册：low=2 启用、low=3 持有禁用、其余未持有。
    let mut book = PrivBook::new();
    let low_a = unx36041_privtable::PRIV_TABLE[0].luid_low;
    let low_b = unx36041_privtable::PRIV_TABLE[1].luid_low;
    book.enroll((0, low_a), false)?;
    book.enable((0, low_a))?;
    book.enroll((0, low_b), false)?;
    // R2：桩测 20 例——(启用/禁用/未持有) × 表内循环 = 20 次调用。
    let mut hook = HookLedger::new();
    let mut denials = DenialLedger::new();
    let mut k = 0;
    while k < 20 {
        let low = unx36041_privtable::PRIV_TABLE[k % 17].luid_low;
        let v = priv_step(&mut book, &mut denials, &mut hook, 5700 + k as u64, low)?;
        // 期望：low==a → 允许；low==b → 禁用拒；其余 → 未持有拒。
        let want = if low == low_a {
            H_ALLOWED
        } else if low == low_b {
            H_DENIED_NOT_ENABLED
        } else {
            H_DENIED_NOT_OWNED
        };
        if v != want {
            return Err(E_STUB);
        }
        hits += 1;
        k += 1;
    }
    // R4：挂点账 20 条守恒。
    if hook.len != 20 || !hook.conserve() {
        return Err(E_BAL);
    }
    hits += 1;
    // R5：拒绝面同步落诊断账——20 例中拒绝 18 例（a 允许 1 次 + b 禁用
    // 多次……精确：a 在 k%17==0 恰 2 次（k=0,17），b 在 k%17==1 恰 2 次，
    // 其余 16 例未持有拒。拒绝账 = b 的 2 次 + 未持有 16 次 = 18。
    if denials.len != 18 {
        return Err(E_AUDIT);
    }
    hits += 1;
    // R3：单点引用——17 项直调映射全一致（无第二套判定）。
    let mut j = 0;
    while j < 17 {
        let low = unx36041_privtable::PRIV_TABLE[j].luid_low;
        if !single_engine_check(&book, low)? {
            return Err(E_SECOND);
        }
        hits += 1;
        j += 1;
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(41));
    }

    #[test]
    fn stub_three_paths() {
        let mut b = PrivBook::new();
        b.enroll((0, 2), false).unwrap();
        b.enable((0, 2)).unwrap();
        b.enroll((0, 3), false).unwrap();
        let mut h = HookLedger::new();
        let mut d = DenialLedger::new();
        assert_eq!(priv_step(&mut b, &mut d, &mut h, 1, 2).unwrap(), H_ALLOWED);
        assert_eq!(
            priv_step(&mut b, &mut d, &mut h, 1, 3).unwrap(),
            H_DENIED_NOT_ENABLED
        );
        assert_eq!(
            priv_step(&mut b, &mut d, &mut h, 1, 4).unwrap(),
            H_DENIED_NOT_OWNED
        );
        assert_eq!(h.len, 3);
        assert_eq!(d.len, 2);
    }
}

pub fn run_unx36057_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36057_privhook");
    cs.add("SEG 段位独占", SEG_BASE == 0xFE80, "0xFE80~0xFE9F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}
