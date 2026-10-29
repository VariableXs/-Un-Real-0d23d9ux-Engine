//! UNX-F36225-J1 DuplicateHandle 类语义：降权掩码复制。
//! 判据：① 降权复制 100 例：新句柄 ga = 复制请求 ∩ 源 ga（逐位子集，
//! obj 恒等——复制不换对象），同权复制形态（desired == src.ga）合法；
//! ② 升权注入 10 例（desired 带源掩码没有的合法位）全被 E_RISE 拦截，
//! 越合法区注入（保留位/GENERIC 位）先于子集检查被 E_ILLEGAL 拦——
//! 两型分归因；③ 陈旧源（release 后复制）→ E_SRC；④ 纯功能代码 ≥320 行。
//! 设计纪律：Windows DuplicateHandle 的核心红线——目标句柄权限恒是
//! 源句柄权限的子集（dwDesiredAccess 只能裁不能加）；本面把该红线
//! 账面化：先合法性后子集性的双门顺序，越权形态各自有显性错误码。

use crate::checks::CheckSet;
use super::unx36102_stdrights as sr;
use super::unx36221_gafield as gf;

pub const SEG_BASE: u16 = 0xa1b0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_SRC: u16 = SEG_BASE + 2; // 源句柄非法（陈旧/空槽）
pub const E_ILLEGAL: u16 = SEG_BASE + 3; // desired 越存储合法区
pub const E_RISE: u16 = SEG_BASE + 4; // 升权（desired ⊄ 源 ga）
pub const E_ATTR: u16 = SEG_BASE + 5; // 属性越合法集
pub const E_CAP: u16 = SEG_BASE + 6; // 目标表满

/// 模块版本锚。
pub const VER: u32 = 1;

const _: () = assert!(SEG_BASE == 0xa1b0, "段位锚漂移");

/// 降权复制核心（判据 ① 的落点）：目标句柄 = 同对象 + 裁剪后掩码。
/// 双门顺序：合法性门（E_ILLEGAL）先于子集门（E_RISE）——非法值不进
/// 子集对拍，两型越权各自可归因。
pub fn dup_downgrade(
    t: &mut gf::HandleTable,
    src_slot: usize,
    src_gen: u32,
    desired: u32,
    attrs: u32,
) -> Result<(usize, u32), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 源句柄读回（陈旧/空槽/墓碑 → E_SRC——复制源必须是活句柄）。
    let src = t.read(src_slot, src_gen).map_err(|_| E_SRC)?;
    // 门一：合法性（保留位/GENERIC 位在存储面恒非法）。
    if desired & !gf::GA_STORE_LEGAL != 0 {
        return Err(E_ILLEGAL);
    }
    // 门二：子集性（升权 = desired 带源掩码没有的位——红线）。
    if desired & !src.ga != 0 {
        return Err(E_RISE);
    }
    if attrs & !gf::ATTR_LEGAL != 0 {
        return Err(E_ATTR);
    }
    // 同对象入槽（obj 恒等——复制的语义锚）。
    t.attach(src.obj, desired, attrs)
}

/// 复制账（源→目标对落账 + 拒绝分型计数；账面只收绿复制）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DupLedger {
    pub n: u32,
    pub same_access: u32,
    pub downgraded: u32,
    pub rise_blocked: u32,
    pub illegal_blocked: u32,
    pub attr_blocked: u32,
    pub src_ga_or: u32,
    pub dst_ga_or: u32,
}

impl DupLedger {
    pub fn fresh() -> DupLedger {
        DupLedger {
            n: 0,
            same_access: 0,
            downgraded: 0,
            rise_blocked: 0,
            illegal_blocked: 0,
            attr_blocked: 0,
            src_ga_or: 0,
            dst_ga_or: 0,
        }
    }

    /// 记一绿复制（子集性复核后落账——账面自证 new ⊆ src）。
    pub fn record(&mut self, src_ga: u32, dst_ga: u32) -> Result<(), u16> {
        if dst_ga & !src_ga != 0 {
            return Err(E_RISE);
        }
        self.n = self.n.wrapping_add(1);
        if dst_ga == src_ga {
            self.same_access = self.same_access.wrapping_add(1);
        } else {
            self.downgraded = self.downgraded.wrapping_add(1);
        }
        self.src_ga_or |= src_ga;
        self.dst_ga_or |= dst_ga;
        Ok(())
    }

    /// 记一拒（按错误码分型计数）。
    pub fn record_blocked(&mut self, code: u16) {
        match code {
            E_RISE => self.rise_blocked = self.rise_blocked.wrapping_add(1),
            E_ILLEGAL => self.illegal_blocked = self.illegal_blocked.wrapping_add(1),
            E_ATTR => self.attr_blocked = self.attr_blocked.wrapping_add(1),
            _ => {}
        }
    }

    /// 账守恒：n = 同权 + 降权（分账/总账对平）。
    pub fn tally_ok(&self) -> bool {
        self.n == self.same_access + self.downgraded
    }

    /// 子集守恒：账面并集逐位保序（dst_or ⊆ src_or）。
    pub fn subset_ok(&self) -> bool {
        self.dst_ga_or & !self.src_ga_or == 0
    }
}

/// 复制请求样本生成（确定性：源掩码 × 剥位集循环投影）。
pub fn strip_set(i: usize) -> u32 {
    // 七种剥位形态循环：剥标准位/剥特定位/不剥（同权）。
    match i % 7 {
        0 => sr::SYNCHRONIZE,
        1 => sr::WRITE_DAC | sr::WRITE_OWNER,
        2 => 0x0004,
        3 => 0x0001 | sr::DELETE,
        4 => sr::DELETE | sr::WRITE_DAC,
        5 => 0x0002,
        _ => 0,
    }
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：降权复制 100 例（判据 ① 落点）。降权复制是同进程语义
    //（36226 才是跨表）——源与副本同表，槽位/代数各自独立。
    let mut t = gf::HandleTable::new();
    let masks = [
        sr::STD_ALL | 0x0007,
        sr::READ_CONTROL | sr::SYNCHRONIZE | 0x0003,
        sr::DELETE | sr::READ_CONTROL | 0x0020,
        sr::WRITE_DAC | sr::SYNCHRONIZE | 0x0001,
        sr::STD_ALL,
    ];
    // 建 20 个源句柄（掩码循环五型）。
    let mut src_keys = [(0usize, 0u32); 20];
    let mut i = 0;
    while i < 20 {
        src_keys[i] = t.attach(0xA000 + i as u32, masks[i % 5], 0).map_err(|_| E_CAP)?;
        i += 1;
    }
    let mut led = DupLedger::fresh();
    let mut i = 0u32;
    while i < 100 {
        let k = (i % 20) as usize;
        let (slot, gen) = src_keys[k];
        let src_ga = t.read(slot, gen).map_err(|_| E_SRC)?.ga;
        // desired = 源掩码剥掉确定性位集（i%7==6 → 同权复制形态）。
        let desired = src_ga & !strip_set(i as usize);
        let (dslot, dgen) = dup_downgrade(&mut t, slot, gen, desired, 0)?;
        let dst_ga = t.read(dslot, dgen).map_err(|_| E_CAP)?.ga;
        if dst_ga != desired {
            return Err(E_RISE);
        }
        if dst_ga & !src_ga != 0 {
            return Err(E_RISE);
        }
        // obj 恒等（复制不换对象）。
        if t.read(dslot, dgen).map_err(|_| E_CAP)?.obj != 0xA000 + (k as u32) {
            return Err(E_SRC);
        }
        led.record(src_ga, dst_ga)?;
        i += 1;
    }
    if led.n != 100 || !led.tally_ok() || !led.subset_ok() {
        return Err(E_RISE);
    }
    // 同权复制形态计数（i%7==6 → 100/7 ≈ 15 例）。
    if led.same_access == 0 || led.downgraded == 0 {
        return Err(E_RISE);
    }
    // R2：升权注入 10 例（判据 ② 落点）——desired = 源掩码 + 源没有的位。
    // 注入位全部是合法区内的位（越权而非越界）。
    let extra = [sr::WRITE_DAC, sr::DELETE, sr::WRITE_OWNER, 0x0001, 0x0002, 0x0004, 0x0020, sr::SYNCHRONIZE, sr::READ_CONTROL, 0x0040];
    let mut i = 0;
    while i < 10 {
        let k = i % 20;
        let (slot, gen) = src_keys[k];
        let src_ga = t.read(slot, gen).map_err(|_| E_SRC)?.ga;
        // 选取一个源掩码必缺的合法位：从 extra 里挑不在 src_ga 的最低位；
        // 若源全权（无缺位），先剥掉该位再造源缺位形态。
        let mut inj_bit = 0u32;
        let mut j = 0;
        while j < 10 {
            if extra[j] & src_ga == 0 {
                inj_bit = extra[j];
                break;
            }
            j += 1;
        }
        let desired = if inj_bit != 0 { src_ga | inj_bit } else { (src_ga | extra[0]) & gf::GA_STORE_LEGAL };
        match dup_downgrade(&mut t, slot, gen, desired, 0) {
            Err(code) => led.record_blocked(code),
            Ok(_) => return Err(E_RISE),
        }
        i += 1;
    }
    if led.rise_blocked != 10 {
        return Err(E_RISE);
    }
    // R3：越合法区注入（保留位 bit21 / GENERIC 位）→ E_ILLEGAL（门一先拦）。
    let (slot0, gen0) = src_keys[0];
    if dup_downgrade(&mut t, slot0, gen0, 0x0020_0000, 0).err() != Some(E_ILLEGAL) {
        return Err(E_ILLEGAL);
    }
    if dup_downgrade(&mut t, slot0, gen0, 0x8000_0000, 0).err() != Some(E_ILLEGAL) {
        return Err(E_ILLEGAL);
    }
    if dup_downgrade(&mut t, slot0, gen0, sr::STD_ALL | 0x0040_0000, 0).err() != Some(E_ILLEGAL) {
        return Err(E_ILLEGAL);
    }
    led.illegal_blocked = led.illegal_blocked.wrapping_add(3);
    // 属性越合法集 → E_ATTR。
    if dup_downgrade(&mut t, slot0, gen0, sr::READ_CONTROL, 1 << 28).err() != Some(E_ATTR) {
        return Err(E_ATTR);
    }
    led.attr_blocked = led.attr_blocked.wrapping_add(1);
    // R4：陈旧源（release 后复制）→ E_SRC；墓碑源同拒。
    let (tmp_slot, tmp_gen) = t.attach(0xB000, sr::READ_CONTROL, 0).map_err(|_| E_CAP)?;
    t.release(tmp_slot, tmp_gen).map_err(|_| E_SRC)?;
    if dup_downgrade(&mut t, tmp_slot, tmp_gen, sr::READ_CONTROL, 0).err() != Some(E_SRC) {
        return Err(E_SRC);
    }
    // 陈旧代数（活槽旧 gen）→ E_SRC。
    let (slot1, gen1) = src_keys[1];
    if dup_downgrade(&mut t, slot1, gen1.wrapping_add(7), sr::READ_CONTROL, 0).err() != Some(E_SRC) {
        return Err(E_SRC);
    }
    // R5：账面汇总（分型计数与总数对平——账是拦截的计数器）。
    let blocked = led.rise_blocked + led.illegal_blocked + led.attr_blocked;
    if blocked != 14 {
        return Err(E_RISE);
    }
    // 校验和：源/目标并集 + 例数（确定性总锚）。
    Ok(led
        .src_ga_or
        .wrapping_add(led.dst_ga_or)
        .wrapping_add(led.n)
        .wrapping_add(blocked))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_SRC => "源句柄非法（陈旧代数/空槽/墓碑）",
        E_ILLEGAL => "desired 越存储合法区（保留位/GENERIC 位）",
        E_RISE => "升权（desired ⊄ 源 ga——复制红线破）",
        E_ATTR => "属性越合法集",
        E_CAP => "目标表满",
        _ => "未知诊断码",
    }
}

pub fn run_unx36225_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36225_gadup1");
    cs.add("SEG 段位独占", SEG_BASE == 0xa1b0, "0xa1b0~0xa1bF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "100 例降权子集/10 例升权全拦/双门分型"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("子集红线", DupLedger::fresh().subset_ok(), "空账并集保序（dst ⊆ src 结构锚）");
    cs.add("同权合法", strip_set(6) == 0, "剥位集闭集含零剥形态（同权复制合法）");
    cs.add("账恒等", DupLedger::fresh().tally_ok(), "空账 n = 同权 + 降权");
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
    fn test_downgrade_subset_only() {
        let mut t = gf::HandleTable::new();
        let (s, g) = t.attach(1, sr::READ_CONTROL | 0x0001, 0).unwrap_or((0, 0));
        // 同权复制合法。
        assert!(dup_downgrade(&mut t, s, g, sr::READ_CONTROL | 0x0001, 0).is_ok());
        // 升权拒（0x0002 不在源掩码）。
        assert_eq!(dup_downgrade(&mut t, s, g, sr::READ_CONTROL | 0x0002, 0).err(), Some(E_RISE));
        // 越合法区拒（门一先于门二）。
        assert_eq!(dup_downgrade(&mut t, s, g, 0x0020_0000, 0).err(), Some(E_ILLEGAL));
    }

    #[test]
    fn test_ledger_counts_blocks() {
        let mut led = DupLedger::fresh();
        led.record(sr::STD_ALL, sr::READ_CONTROL).unwrap_or(());
        led.record(sr::READ_CONTROL, sr::READ_CONTROL).unwrap_or(());
        led.record_blocked(E_RISE);
        led.record_blocked(E_ILLEGAL);
        assert_eq!(led.n, 2);
        assert_eq!(led.same_access, 1);
        assert_eq!(led.downgraded, 1);
        assert_eq!(led.rise_blocked, 1);
        assert!(led.tally_ok() && led.subset_ok());
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36225_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 5);
    }
}
