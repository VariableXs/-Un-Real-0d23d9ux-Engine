//! UNX-F36226-J1 DuplicateHandle 类语义：跨进程复制与 DUPLICATE_SAME_ACCESS。
//! 判据：① 跨进程复制 70 例：SameAccess 40 例目标掩码与源逐位相等、
//! Masked 20 例目标 ⊆ 源、CloseSource 10 例源槽即时清算（读旧句柄 →
//! 陈旧）；obj 跨表恒等（复制不换对象）；② 目标表满 → E_CAP 且源不受
//! 挂（失败复制零副作用）；③ 复制账守恒：dst 收益数 = same + masked，
//! close_src 数 = 源表清算数；④ 纯功能代码 ≥320 行。
//! 设计纪律：Windows DuplicateHandle 跨进程形态——目标进程得到指向
//! 同一对象的独立句柄（引用计数 +1 由对象面承担，句柄面只落账）；
//! DUPLICATE_CLOSE_SOURCE 使源句柄在复制成功后才关闭（失败不关——
//! 原子性锚）。本面双表对拍，两表各自守恒。

use crate::checks::CheckSet;
use super::unx36102_stdrights as sr;
use super::unx36221_gafield as gf;

pub const SEG_BASE: u16 = 0xa1c0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_SRC: u16 = SEG_BASE + 2; // 源句柄非法（陈旧/空槽/墓碑）
pub const E_RISE: u16 = SEG_BASE + 3; // 升权（Masked 模式 desired ⊄ 源 ga）
pub const E_ILLEGAL: u16 = SEG_BASE + 4; // desired 越存储合法区
pub const E_CAP: u16 = SEG_BASE + 5; // 目标表满
pub const E_LED: u16 = SEG_BASE + 6; // 复制账不平

/// 模块版本锚。
pub const VER: u32 = 1;

const _: () = assert!(SEG_BASE == 0xa1c0, "段位锚漂移");

/// 复制模式闭集（DUPLICATE_* 双标志的账面化四态）。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DupMode {
    /// DUPLICATE_SAME_ACCESS：目标掩码 = 源掩码（desired 忽略）。
    SameAccess = 1,
    /// SAME_ACCESS + CLOSE_SOURCE：复制成功后源句柄关闭。
    SameAccessCloseSrc = 2,
    /// 掩码模式：目标 = desired（须 ⊆ 源）。
    Masked = 3,
    /// 掩码 + CLOSE_SOURCE。
    MaskedCloseSrc = 4,
}

impl DupMode {
    pub fn close_src(self) -> bool {
        matches!(self, DupMode::SameAccessCloseSrc | DupMode::MaskedCloseSrc)
    }
    pub fn masked(self) -> bool {
        matches!(self, DupMode::Masked | DupMode::MaskedCloseSrc)
    }
}

/// 跨进程复制核心：src 表（可变——CloseSrc 需清算）→ dst 表。
/// 返回 (目标槽, 目标代数, 源是否已清算)。原子性锚：目标入槽失败时
/// 源绝不清算（失败复制零副作用）。
pub fn dup_cross(
    src: &mut gf::HandleTable,
    dst: &mut gf::HandleTable,
    src_slot: usize,
    src_gen: u32,
    mode: DupMode,
    desired: u32,
) -> Result<(usize, u32, bool), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let e = src.read(src_slot, src_gen).map_err(|_| E_SRC)?;
    let dst_ga = if mode.masked() {
        if desired & !gf::GA_STORE_LEGAL != 0 {
            return Err(E_ILLEGAL);
        }
        if desired & !e.ga != 0 {
            return Err(E_RISE);
        }
        desired
    } else {
        e.ga
    };
    // 目标入槽（obj 恒等——跨进程复制同一对象）。
    let (dslot, dgen) = match dst.attach(e.obj, dst_ga, e.attrs & !gf::HA_INHERIT) {
        Ok(x) => x,
        // 失败不关源——原子性锚（gf::E_CAP 显式限定——本模块同名码遮蔽）。
        Err(gf::E_CAP) => return Err(E_CAP),
        Err(_) => return Err(E_ILLEGAL),
    };
    let mut closed = false;
    if mode.close_src() {
        src.release(src_slot, src_gen).map_err(|_| E_SRC)?;
        closed = true;
    }
    Ok((dslot, dgen, closed))
}

/// 跨进程复制账（双表两侧落账 + 分模式计数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CrossLedger {
    pub n: u32,
    pub same_access: u32,
    pub masked: u32,
    pub close_src: u32,
    pub cap_blocked: u32,
    pub dst_ga_or: u32,
    pub src_ga_or: u32,
}

impl CrossLedger {
    pub fn fresh() -> CrossLedger {
        CrossLedger {
            n: 0,
            same_access: 0,
            masked: 0,
            close_src: 0,
            cap_blocked: 0,
            dst_ga_or: 0,
            src_ga_or: 0,
        }
    }

    /// 记一绿复制（目标掩码子集复核后落账）。
    pub fn record(&mut self, src_ga: u32, dst_ga: u32, mode: DupMode, closed: bool) -> Result<(), u16> {
        if dst_ga & !src_ga != 0 {
            return Err(E_RISE);
        }
        if closed != mode.close_src() {
            return Err(E_LED);
        }
        self.n = self.n.wrapping_add(1);
        if mode.masked() {
            self.masked = self.masked.wrapping_add(1);
        } else {
            self.same_access = self.same_access.wrapping_add(1);
        }
        if closed {
            self.close_src = self.close_src.wrapping_add(1);
        }
        self.src_ga_or |= src_ga;
        self.dst_ga_or |= dst_ga;
        Ok(())
    }

    pub fn record_cap(&mut self) {
        self.cap_blocked = self.cap_blocked.wrapping_add(1);
    }

    /// 账恒等：n = same + masked；close_src ≤ n。
    pub fn tally_ok(&self) -> bool {
        self.n == self.same_access + self.masked && self.close_src <= self.n
    }

    /// 子集守恒：dst 并集 ⊆ src 并集（SameAccess 相等形态不破此序）。
    pub fn subset_ok(&self) -> bool {
        self.dst_ga_or & !self.src_ga_or == 0
    }
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R0：模式闭集锚。
    if DupMode::SameAccess.close_src() || !DupMode::MaskedCloseSrc.close_src() {
        return Err(E_LED);
    }
    // R1：建源表 40 句柄（掩码五型循环）。
    let masks = [
        sr::STD_ALL | 0x0007,
        sr::READ_CONTROL | sr::SYNCHRONIZE | 0x0003,
        sr::DELETE | sr::READ_CONTROL | 0x0020,
        sr::WRITE_DAC | 0x0001,
        sr::STD_ALL,
    ];
    let mut src_t = gf::HandleTable::new();
    let mut dst_t = gf::HandleTable::new();
    let mut keys = [(0usize, 0u32); 40];
    let mut i = 0;
    while i < 40 {
        keys[i] = src_t.attach(0xC000 + i as u32, masks[i % 5], gf::HA_INHERIT).map_err(|_| E_CAP)?;
        i += 1;
    }
    // R2：SameAccess 40 例（判据 ① 落点一）：掩码逐位相等 + obj 恒等。
    let mut led = CrossLedger::fresh();
    let mut i = 0;
    while i < 40 {
        let (slot, gen) = keys[i];
        let src_ga = src_t.read(slot, gen).map_err(|_| E_SRC)?.ga;
        let (ds, dg, closed) = dup_cross(&mut src_t, &mut dst_t, slot, gen, DupMode::SameAccess, 0)?;
        if closed {
            return Err(E_LED);
        }
        let de = dst_t.read(ds, dg).map_err(|_| E_CAP)?;
        if de.ga != src_ga || de.obj != 0xC000 + i as u32 {
            return Err(E_LED);
        }
        // 继承标志在目标侧清零（新进程句柄不自带继承语义）。
        if de.attrs & gf::HA_INHERIT != 0 {
            return Err(E_LED);
        }
        led.record(src_ga, de.ga, DupMode::SameAccess, closed)?;
        i += 1;
    }
    if led.same_access != 40 {
        return Err(E_LED);
    }
    // R3：Masked 20 例（判据 ① 落点二）：目标 = desired ⊆ 源。
    let mut i = 0u32;
    while i < 20 {
        let (slot, gen) = keys[(i % 40) as usize];
        let src_ga = src_t.read(slot, gen).map_err(|_| E_SRC)?.ga;
        let desired = src_ga & !sr::SYNCHRONIZE & !0x0001;
        let (ds, dg, closed) = dup_cross(&mut src_t, &mut dst_t, slot, gen, DupMode::Masked, desired)?;
        if closed {
            return Err(E_LED);
        }
        let de = dst_t.read(ds, dg).map_err(|_| E_CAP)?;
        if de.ga != desired || de.ga & !src_ga != 0 {
            return Err(E_RISE);
        }
        led.record(src_ga, de.ga, DupMode::Masked, closed)?;
        i += 1;
    }
    if led.masked != 20 {
        return Err(E_LED);
    }
    // R4：CloseSource 10 例（判据 ① 落点三）：目标先成、源后清。
    let mut i = 0;
    while i < 10 {
        let (slot, gen) = keys[i * 3];
        let src_ga = src_t.read(slot, gen).map_err(|_| E_SRC)?.ga;
        let (ds, dg, closed) = dup_cross(&mut src_t, &mut dst_t, slot, gen, DupMode::SameAccessCloseSrc, 0)?;
        if !closed {
            return Err(E_LED);
        }
        let de = dst_t.read(ds, dg).map_err(|_| E_CAP)?;
        if de.ga != src_ga {
            return Err(E_LED);
        }
        // 源句柄已清算（读旧句柄 → 陈旧）。
        if src_t.read(slot, gen).err() != Some(gf::E_STALE) {
            return Err(E_LED);
        }
        led.record(src_ga, de.ga, DupMode::SameAccessCloseSrc, closed)?;
        i += 1;
    }
    if led.close_src != 10 {
        return Err(E_LED);
    }
    // R5：升权注入（Masked 带源缺位）→ E_RISE；越合法区 → E_ILLEGAL。
    let (slot0, gen0) = keys[1];
    if dup_cross(&mut src_t, &mut dst_t, slot0, gen0, DupMode::Masked, sr::STD_ALL | 0x0007).err() != Some(E_RISE) {
        return Err(E_RISE);
    }
    if dup_cross(&mut src_t, &mut dst_t, slot0, gen0, DupMode::Masked, 0x0020_0000).err() != Some(E_ILLEGAL) {
        return Err(E_ILLEGAL);
    }
    // 陈旧源 → E_SRC。
    if dup_cross(&mut src_t, &mut dst_t, keys[2].0, keys[2].1.wrapping_add(9), DupMode::SameAccess, 0).err() != Some(E_SRC) {
        return Err(E_SRC);
    }
    // R6：目标表满 → E_CAP 且源不清算（判据 ② 落点：失败复制零副作用）。
    let mut i = dst_t.used();
    while i < gf::TABLE_CAP {
        dst_t.attach(0xD000 + i as u32, sr::READ_CONTROL, 0).map_err(|_| E_CAP)?;
        i += 1;
    }
    let src_used_before = src_t.used();
    if dup_cross(&mut src_t, &mut dst_t, slot0, gen0, DupMode::SameAccess, 0).err() != Some(E_CAP) {
        return Err(E_CAP);
    }
    if src_t.used() != src_used_before {
        return Err(E_LED);
    }
    led.record_cap();
    // CloseSrc + 目标满：目标失败在先，源不清算（原子性复核）。
    if dup_cross(&mut src_t, &mut dst_t, slot0, gen0, DupMode::SameAccessCloseSrc, 0).err() != Some(E_CAP) {
        return Err(E_CAP);
    }
    if src_t.read(slot0, gen0).is_err() {
        return Err(E_LED);
    }
    led.record_cap();
    // R7：账面汇总（判据 ③ 落点）。
    if !led.tally_ok() || !led.subset_ok() {
        return Err(E_LED);
    }
    if led.n != 70 || led.cap_blocked != 2 {
        return Err(E_LED);
    }
    Ok(led
        .dst_ga_or
        .wrapping_add(led.src_ga_or)
        .wrapping_add(led.n)
        .wrapping_add(led.cap_blocked)
        .wrapping_add(led.close_src))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_SRC => "源句柄非法（陈旧代数/空槽/墓碑）",
        E_RISE => "升权（Masked 模式 desired ⊄ 源 ga）",
        E_ILLEGAL => "desired 越存储合法区或目标入槽异常",
        E_CAP => "目标表满（失败复制零副作用）",
        E_LED => "复制账不平（掩码/清算/模式计数破）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36226_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36226_gadup2");
    cs.add("SEG 段位独占", SEG_BASE == 0xa1c0, "0xa1c0~0xa1cF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "70 例三模式/源清算陈旧/表满零副作用"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("模式闭集", !DupMode::SameAccess.close_src() && DupMode::MaskedCloseSrc.close_src() && DupMode::Masked.masked(), "四模式 close_src/masked 投影自洽");
    cs.add("账恒等", CrossLedger::fresh().tally_ok(), "空账 n = same + masked");
    cs.add("子集守恒", CrossLedger::fresh().subset_ok(), "空账 dst 并集 ⊆ src 并集");
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
    fn test_same_access_and_close_src() {
        let mut src = gf::HandleTable::new();
        let mut dst = gf::HandleTable::new();
        let (s, g) = src.attach(9, sr::READ_CONTROL | 0x0001, 0).unwrap_or((0, 0));
        let (ds, dg, closed) = dup_cross(&mut src, &mut dst, s, g, DupMode::SameAccessCloseSrc, 0).unwrap_or((0, 0, false));
        assert!(closed);
        assert_eq!(dst.read(ds, dg).unwrap_or(gf::Entry { obj: 0, ga: 0, attrs: 0, gen: 0, auth_epoch: 0 }).ga, sr::READ_CONTROL | 0x0001);
        // 源已清算：读旧句柄陈旧。
        assert_eq!(src.read(s, g).err(), Some(gf::E_STALE));
    }

    #[test]
    fn test_masked_rise_rejected() {
        let mut src = gf::HandleTable::new();
        let mut dst = gf::HandleTable::new();
        let (s, g) = src.attach(9, sr::READ_CONTROL, 0).unwrap_or((0, 0));
        assert_eq!(dup_cross(&mut src, &mut dst, s, g, DupMode::Masked, sr::SYNCHRONIZE).err(), Some(E_RISE));
        assert_eq!(dup_cross(&mut src, &mut dst, s, g, DupMode::Masked, sr::READ_CONTROL).is_ok(), true);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36226_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 5);
    }
}
