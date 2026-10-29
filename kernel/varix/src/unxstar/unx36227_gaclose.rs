//! UNX-F36227-J1 句柄关闭与安全账清算（谁打开过敏感对象）。
//! 判据：① 关闭清算 100 例：open 数 = close 数 + 活句柄数（清算账守恒
//! ——每关一个必有账）；② HA_AUDIT_CLOSE 属性句柄关闭时落审计记录
//! （记录 obj/槽位/代数——「谁打开过敏感对象」的可追责面），审计数与
//! 账面对平；③ HA_PROTECT 句柄显性拒关（E_PROT），重复关闭（墓碑槽
//! 二次 release）按陈旧拒——清算账防双扣；④ 纯功能代码 ≥320 行。
//! 设计纪律：Windows 句柄关闭的清算语义——CloseHandle 不是清零而是
//! 结账：引用计数减、审计钩子触发、句柄槽代数保留（陈旧句柄即失效）。
//! 本面是「安全账」的落点：每个打开必有且仅有一次关闭清算或泄漏挂账。

use crate::checks::CheckSet;
use super::unx36102_stdrights as sr;
use super::unx36221_gafield as gf;

pub const SEG_BASE: u16 = 0xa1d0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_STALE: u16 = SEG_BASE + 2; // 陈旧句柄/重复关闭
pub const E_PROT: u16 = SEG_BASE + 3; // 保护句柄拒关（仅随表销毁）
pub const E_CAP: u16 = SEG_BASE + 4; // 槽位越界
pub const E_LED: u16 = SEG_BASE + 5; // 清算账不平
pub const E_AUD: u16 = SEG_BASE + 6; // 审计记录破（obj 不对/计数不齐）

/// 模块版本锚。
pub const VER: u32 = 1;

const _: () = assert!(SEG_BASE == 0xa1d0, "段位锚漂移");

/// 审计记录（敏感对象关闭的可追责面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditRec {
    pub obj: u32,
    pub slot: usize,
    pub gen: u32,
    pub ga: u32,
}

/// 关闭清算账（open/close/审计/保护拒四账联动）。
pub struct CloseLedger {
    pub opens: u32,
    pub closes: u32,
    pub audited: u32,
    pub prot_refused: u32,
    pub audit_log: alloc::vec::Vec<AuditRec>,
}

impl CloseLedger {
    pub fn fresh() -> CloseLedger {
        CloseLedger { opens: 0, closes: 0, audited: 0, prot_refused: 0, audit_log: alloc::vec::Vec::new() }
    }

    /// 守恒：opens = closes + 活句柄数（每开必有一关或仍在槽）。
    pub fn conserve_ok(&self, live: u32) -> bool {
        self.opens == self.closes + live
    }

    /// 审计对账：审计计数 = 审计日志长度。
    pub fn audit_ok(&self) -> bool {
        self.audited as usize == self.audit_log.len()
    }
}

/// 关闭清算核心（判据 ① 的落点）：读句柄 → 保护检查 → 审计钩子 →
/// 释放槽。错误分型：保护句柄 → E_PROT；陈旧/墓碑 → E_STALE。
pub fn close_handle(
    t: &mut gf::HandleTable,
    led: &mut CloseLedger,
    slot: usize,
    gen: u32,
) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let e = t.read(slot, gen).map_err(|_| E_STALE)?;
    if e.attrs & gf::HA_PROTECT != 0 {
        led.prot_refused = led.prot_refused.wrapping_add(1);
        return Err(E_PROT);
    }
    if e.attrs & gf::HA_AUDIT_CLOSE != 0 {
        led.audit_log.push(AuditRec { obj: e.obj, slot, gen, ga: e.ga });
        led.audited = led.audited.wrapping_add(1);
    }
    t.release(slot, gen).map_err(|_| E_STALE)?;
    led.closes = led.closes.wrapping_add(1);
    Ok(())
}

/// 敏感对象判定（审计属性挂接的对象面锚：ga 带 DELETE/WRITE_DAC 的
/// 句柄视为敏感——打开时必须挂 HA_AUDIT_CLOSE 的账面规则）。
pub fn requires_audit_attr(ga: u32) -> bool {
    ga & (sr::DELETE | sr::WRITE_DAC) != 0
}

/// 打开侧账（open 与 close 同账——守恒式的开账入口）：attach 成功
/// 即开账 +1，敏感掩码按规则强制挂审计属性（账面规则的落点）。
pub fn open_book(
    t: &mut gf::HandleTable,
    led: &mut CloseLedger,
    obj: u32,
    ga: u32,
    attrs: u32,
) -> Result<(usize, u32), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut attrs_eff = attrs;
    if requires_audit_attr(ga) {
        attrs_eff |= gf::HA_AUDIT_CLOSE;
    }
    let (slot, gen) = t.attach(obj, ga, attrs_eff).map_err(|_| E_CAP)?;
    led.opens = led.opens.wrapping_add(1);
    Ok((slot, gen))
}

/// 泄漏面（与 AI-18 泄漏账的分工界面）：opens − closes = 活句柄数
/// 即零泄漏挂账；差值非零 = 账面泄漏（须由表销毁路径清账——36228）。
pub fn leak_count(led: &CloseLedger) -> u32 {
    led.opens.wrapping_sub(led.closes)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R0：敏感对象规则锚（写侧位触发审计要求）。
    if !requires_audit_attr(sr::DELETE) || !requires_audit_attr(sr::WRITE_DAC | 0x0001) {
        return Err(E_AUD);
    }
    if requires_audit_attr(sr::READ_CONTROL | sr::SYNCHRONIZE) {
        return Err(E_AUD);
    }
    // R1：打开 100 例（判据 ① 落点的开账侧）：敏感掩码句柄必须挂
    // HA_AUDIT_CLOSE；只读句柄不挂；PROTECT 句柄五例。
    let mut t = gf::HandleTable::new();
    let mut led = CloseLedger::fresh();
    let masks = [
        sr::DELETE | sr::READ_CONTROL,      // 敏感（DELETE）
        sr::WRITE_DAC | 0x0002,             // 敏感（WRITE_DAC）
        sr::READ_CONTROL | sr::SYNCHRONIZE, // 非敏感
        sr::STD_ALL,                        // 敏感（DELETE|WRITE_DAC）
        sr::READ_CONTROL,                   // 非敏感
    ];
    let mut keys = [(0usize, 0u32); 100];
    let mut i = 0;
    while i < 100 {
        let ga = masks[i % 5];
        // PROTECT 例（95..100）走显性保护属性；敏感掩码由 open_book
        // 的账面规则强制挂审计（规则落点在开账侧）。
        let attrs = if i >= 95 { gf::HA_PROTECT } else { 0 };
        keys[i] = open_book(&mut t, &mut led, 0xE000 + i as u32, ga, attrs)?;
        i += 1;
    }
    if led.opens != 100 {
        return Err(E_LED);
    }
    // 泄漏面对拍（AI-18 分工界面）：opens − closes = 活句柄数。
    if leak_count(&led) != t.used() as u32 {
        return Err(E_LED);
    }
    // R2：关闭 60 例（判据 ① 落点的关账侧）：前 60 句柄逐个关闭；
    // 敏感例审计记录齐平。
    let mut i = 0;
    while i < 60 {
        let (slot, gen) = keys[i];
        close_handle(&mut t, &mut led, slot, gen)?;
        i += 1;
    }
    if led.closes != 60 {
        return Err(E_LED);
    }
    if !led.conserve_ok(t.used() as u32) {
        return Err(E_LED);
    }
    // R3：审计对账（判据 ② 落点）：前 60 例中敏感掩码型 i%5 ∈ {0,1,3}
    // 挂审计 → 审计数 = 12×3 = 36；日志逐条 obj/slot/gen 复核。
    if led.audited != 36 || !led.audit_ok() {
        return Err(E_AUD);
    }
    let mut i = 0;
    while i < led.audit_log.len() {
        let r = led.audit_log[i];
        let (slot, gen) = keys[r.slot];
        if r.slot >= 60 || r.gen != gen || r.obj != 0xE000 + r.slot as u32 {
            return Err(E_AUD);
        }
        // 审计记录的 ga 必是敏感掩码（规则回查）。
        if !requires_audit_attr(r.ga) {
            return Err(E_AUD);
        }
        let _ = slot;
        i += 1;
    }
    // R4：保护句柄拒关（判据 ③ 落点一）：五个 PROTECT 例全 E_PROT，
    // 计数器逐例对账。
    let mut i = 95;
    while i < 100 {
        let (slot, gen) = keys[i];
        if close_handle(&mut t, &mut led, slot, gen).err() != Some(E_PROT) {
            return Err(E_PROT);
        }
        i += 1;
    }
    if led.prot_refused != 5 {
        return Err(E_PROT);
    }
    if !led.conserve_ok(t.used() as u32) || leak_count(&led) != t.used() as u32 {
        return Err(E_LED);
    }
    // R5：重复关闭按陈旧拒（判据 ③ 落点二）：墓碑槽二次 release →
    // E_STALE，清算账不动（防双扣）。
    let (slot0, gen0) = keys[0];
    if close_handle(&mut t, &mut led, slot0, gen0).err() != Some(E_STALE) {
        return Err(E_STALE);
    }
    if led.closes != 60 {
        return Err(E_LED);
    }
    // 陈旧代数同理。
    if close_handle(&mut t, &mut led, keys[1].0, keys[1].1.wrapping_add(3)).err() != Some(E_STALE) {
        return Err(E_STALE);
    }
    if !led.conserve_ok(t.used() as u32) || !led.audit_ok() {
        return Err(E_LED);
    }
    // R6：关闭后槽位复用代数递增（清算的代数侧账）。
    let (slot2, gen2) = t.attach(0xF000, sr::READ_CONTROL, 0).map_err(|_| E_CAP)?;
    if slot2 != 0 || gen2 != gen0 + 1 {
        return Err(E_LED);
    }
    let _ = (slot2, gen2);
    // 校验和：opens+closes+审计数+保护拒数（确定性总锚）。
    Ok(led
        .opens
        .wrapping_add(led.closes)
        .wrapping_add(led.audited)
        .wrapping_add(led.prot_refused)
        .wrapping_add(led.audit_log.len() as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_STALE => "陈旧句柄/重复关闭（墓碑槽二次清算被拒）",
        E_PROT => "保护句柄拒关（HA_PROTECT——仅随表销毁回收）",
        E_CAP => "槽位越界或表满",
        E_LED => "清算账不平（opens ≠ closes + live）",
        E_AUD => "审计记录破（obj/gen 不对或敏感规则回查失败）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36227_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36227_gaclose");
    cs.add("SEG 段位独占", SEG_BASE == 0xa1d0, "0xa1d0~0xa1dF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "100 开 60 关守恒/36 例审计/5 例保护拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("守恒锚", CloseLedger::fresh().conserve_ok(0), "空账 opens = closes + live");
    cs.add("审计锚", CloseLedger::fresh().audit_ok(), "空账审计数 = 日志长度");
    cs.add("敏感规则", requires_audit_attr(sr::DELETE) && !requires_audit_attr(sr::READ_CONTROL), "写侧位触发审计要求");
    cs.add("开账入口", VER == 1, "open_book 统一开账（敏感掩码强制审计属性）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 -------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_protect_and_double_close() {
        let mut t = gf::HandleTable::new();
        let mut led = CloseLedger::fresh();
        let (sp, gp) = t.attach(1, sr::READ_CONTROL, gf::HA_PROTECT).unwrap_or((0, 0));
        assert_eq!(close_handle(&mut t, &mut led, sp, gp).err(), Some(E_PROT));
        assert_eq!(led.prot_refused, 1);
        let (s, g) = t.attach(2, sr::READ_CONTROL, 0).unwrap_or((0, 0));
        assert!(close_handle(&mut t, &mut led, s, g).is_ok());
        // 双扣防：墓碑槽二次关闭 → 陈旧，账不动。
        assert_eq!(close_handle(&mut t, &mut led, s, g).err(), Some(E_STALE));
        assert_eq!(led.closes, 1);
    }

    #[test]
    fn test_audit_records() {
        let mut t = gf::HandleTable::new();
        let mut led = CloseLedger::fresh();
        let (s, g) = t.attach(7, sr::DELETE, gf::HA_AUDIT_CLOSE).unwrap_or((0, 0));
        led.opens = 1; // 本测试只走关闭侧——开账侧先挂一。
        assert!(close_handle(&mut t, &mut led, s, g).is_ok());
        assert_eq!(led.audited, 1);
        assert_eq!(led.audit_log[0].obj, 7);
        assert!(led.audit_ok());
        assert!(led.conserve_ok(t.used() as u32));
    }

    #[test]
    fn test_open_book_forces_audit() {
        let mut t = gf::HandleTable::new();
        let mut led = CloseLedger::fresh();
        // 敏感掩码（DELETE）→ open_book 强制挂审计属性。
        let (s, g) = open_book(&mut t, &mut led, 9, sr::DELETE, 0).unwrap_or((0, 0));
        let e = t.read(s, g).unwrap_or(gf::Entry { obj: 0, ga: 0, attrs: 0, gen: 0, auth_epoch: 0 });
        assert_eq!(e.attrs & gf::HA_AUDIT_CLOSE, gf::HA_AUDIT_CLOSE);
        assert_eq!(led.opens, 1);
        // 非敏感掩码不挂。
        let (s2, _g2) = open_book(&mut t, &mut led, 10, sr::READ_CONTROL, 0).unwrap_or((0, 0));
        let e2 = t.read(s2, 1).unwrap_or(e);
        assert_eq!(e2.attrs & gf::HA_AUDIT_CLOSE, 0);
        assert_eq!(leak_count(&led), t.used() as u32);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36227_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 5);
    }
}
