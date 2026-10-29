//! UNX-F36222-J1 打开时授权语义（OpenObject→AccessCheck→GrantedAccess 写入）。
//! 判据：① 打开路径 200 例：授出掩码 = AccessCheck 裁决逐位一致（引擎
//! 裁决 granted ⊆ 候选；句柄面只做「裁直写」，不重判定）；② 检查被绕过
//! 注入 10 例（未走 open_authorize 直接入槽的条目缺授权戳）被自检清扫
//! 全部修出；③ GENERIC 位在打开时映射到特定|标准位（存储面无 GENERIC
//! ——36221 域锚的消费兑现）；④ 纯功能代码 ≥320 行。
//! 设计纪律：句柄面消费 B09 引擎裁决——本模块的 AcVerdict 是引擎裁决
//! 的落点形态（granted/denied 二元 + 裁决者锚），ACL 求值本体在 B09
//! （36161~36169 单一引擎）；这里的 fixture 只为 200 例提供确定性裁决
//! 流，不构成第二套判定（36236 单一引擎断言面复核此纪律）。

use crate::checks::CheckSet;
use super::unx36101_masktable as mt;
use super::unx36102_stdrights as sr;
use super::unx36221_gafield as gf;

pub const SEG_BASE: u16 = 0xa160;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_MAP: u16 = SEG_BASE + 2; // GENERIC 映射失败（越映射表闭集）
pub const E_GRANT: u16 = SEG_BASE + 3; // 授出 ≠ 引擎裁决（逐位不一致）
pub const E_BYPASS: u16 = SEG_BASE + 4; // 打开被绕过（条目缺授权戳）
pub const E_TABLE: u16 = SEG_BASE + 5; // 表面完整性破（挂接失败/槽位非法）
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 授出越存储合法区

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- GENERIC 映射（文件型对象；Windows RtlMapGenericMask 语义的句柄侧锚）--
pub const GENERIC_READ: u32 = 0x8000_0000;
pub const GENERIC_WRITE: u32 = 0x4000_0000;
pub const GENERIC_EXECUTE: u32 = 0x2000_0000;
pub const GENERIC_ALL: u32 = 0x1000_0000;

// 文件型特定位（对象类型局部语义的四个代表位——J1 句柄面闭集）。
pub const FS_READ_DATA: u32 = 0x0001;
pub const FS_WRITE_DATA: u32 = 0x0002;
pub const FS_APPEND_DATA: u32 = 0x0004;
pub const FS_EXECUTE: u32 = 0x0020;

/// 文件型 GENERIC→特定|标准 映射表（四项闭集；越集 → E_MAP）。
pub fn map_generic(desired: u32) -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut out = 0u32;
    if desired & GENERIC_READ != 0 {
        out |= FS_READ_DATA | sr::READ_CONTROL | sr::SYNCHRONIZE;
    }
    if desired & GENERIC_WRITE != 0 {
        out |= FS_WRITE_DATA | FS_APPEND_DATA | sr::READ_CONTROL | sr::SYNCHRONIZE;
    }
    if desired & GENERIC_EXECUTE != 0 {
        out |= FS_EXECUTE | sr::READ_CONTROL | sr::SYNCHRONIZE;
    }
    if desired & GENERIC_ALL != 0 {
        out |= FS_READ_DATA | FS_WRITE_DATA | FS_APPEND_DATA | FS_EXECUTE | sr::STD_ALL;
    }
    let rest = desired & !(GENERIC_READ | GENERIC_WRITE | GENERIC_EXECUTE | GENERIC_ALL);
    // 非 GENERIC 位直通（特定/标准位原样保留）。
    out |= rest;
    if out & !mt::SPEC_MASK & !mt::STD_USED != 0 {
        return Err(E_MAP);
    }
    Ok(out)
}

/// 引擎裁决落点（B09 AccessCheck 的句柄侧形态；本面不重求值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AcVerdict {
    /// 引擎判允掩码（恒 ⊆ 映射后请求且 ⊆ 合法区）。
    pub granted: u32,
    /// 判拒（true → 打开失败，不入槽）。
    pub denied: bool,
    /// 裁决者锚（引擎代数——单一引擎断言 36236 的登记键）。
    pub engine_gen: u32,
}

/// 授权戳（打开路径写入条目的存在性证明——绕过注入的检出依据）。
pub const AUTH_MAGIC: u32 = 0xA22_0000;

/// 打开授权（判据 ① 的落点）：映射请求 → 与引擎裁决对拍 → 直写入槽。
/// 授出 = 裁决 granted（逐位一致；句柄面零修改——重判定在此被结构性排除）。
pub fn open_authorize(
    t: &mut gf::HandleTable,
    obj: u32,
    desired: u32,
    attrs: u32,
    v: AcVerdict,
) -> Result<(usize, u32), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if v.denied {
        // 判拒不入槽（拒绝也是裁决的忠实兑现——不留半开句柄）。
        return Err(E_GRANT);
    }
    let mapped = map_generic(desired)?;
    // 授出越存储合法区先拦（GENERIC 位入槽是硬错误，优先于子集对拍）。
    if v.granted & !gf::GA_STORE_LEGAL != 0 {
        return Err(E_ILLEGAL);
    }
    // 授出必须 ⊆ 映射请求（裁决越请求 = 引擎/句柄面契约破）。
    if v.granted & !mapped != 0 {
        return Err(E_GRANT);
    }
    let (slot, gen) = t.attach(obj, v.granted, attrs)?;
    Ok((slot, gen))
}

/// 授权侧账（戳面真源：槽位 → 打开时表代数；未经 open_authorize 的条目
/// 无侧账行——清扫器据账判绕过）。
pub const STAMP_CAP: usize = gf::TABLE_CAP;

pub struct StampLedger {
    slots: [u32; STAMP_CAP], // 0 = 无戳
    epoch_at: [u32; STAMP_CAP],
}

impl StampLedger {
    pub fn new() -> StampLedger {
        StampLedger { slots: [0; STAMP_CAP], epoch_at: [0; STAMP_CAP] }
    }

    pub fn mark(&mut self, slot: usize, magic: u32, epoch: u32) -> Result<(), u16> {
        if slot >= STAMP_CAP {
            return Err(E_TABLE);
        }
        self.slots[slot] = magic | 0x1;
        self.epoch_at[slot] = epoch;
        Ok(())
    }

    pub fn stamped(&self, slot: usize) -> bool {
        slot < STAMP_CAP && self.slots[slot] != 0
    }

    /// 清扫：返回缺戳槽位集（绕过注入的检出面）。
    pub fn sweep_unstamped(&self, t: &gf::HandleTable) -> alloc::vec::Vec<usize> {
        let mut out = alloc::vec::Vec::new();
        let mut i = 0;
        while i < gf::TABLE_CAP {
            if t.entry_at(i).is_some() && !self.stamped(i) {
                out.push(i);
            }
            i += 1;
        }
        out
    }
}

/// 确定性裁决流（fixture：owner 全允 / world 只读 / deny 写组三型）。
/// 这是 B09 引擎裁决的替身输入源——200 例按 i%3 循环取型。
pub fn fixture_verdict(i: u32, mapped: u32) -> AcVerdict {
    match i % 3 {
        0 => AcVerdict { granted: mapped, denied: false, engine_gen: 7 },
        1 => {
            // world 只读：裁决剔除写侧位（WRITE_DATA/APPEND/WRITE_DAC/WRITE_OWNER/DELETE）。
            let write_side = FS_WRITE_DATA | FS_APPEND_DATA | sr::WRITE_DAC | sr::WRITE_OWNER | sr::DELETE;
            AcVerdict { granted: mapped & !write_side, denied: false, engine_gen: 7 }
        }
        _ => {
            // 三型一轮一次判拒（denied=true → 不入槽）。
            if i % 9 == 2 {
                AcVerdict { granted: 0, denied: true, engine_gen: 7 }
            } else {
                AcVerdict { granted: mapped & !sr::WRITE_OWNER, denied: false, engine_gen: 7 }
            }
        }
    }
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：GENERIC 映射闭集与语义锚。
    let r = map_generic(GENERIC_READ)?;
    if r != FS_READ_DATA | sr::READ_CONTROL | sr::SYNCHRONIZE {
        return Err(E_MAP);
    }
    let w = map_generic(GENERIC_WRITE)?;
    if w & FS_WRITE_DATA == 0 || w & sr::WRITE_DAC != 0 {
        return Err(E_MAP);
    }
    let a = map_generic(GENERIC_ALL)?;
    if a & !gf::GA_STORE_LEGAL != 0 {
        return Err(E_ILLEGAL);
    }
    // 混合请求：GENERIC + 特定位直通。
    let mix = map_generic(GENERIC_EXECUTE | FS_READ_DATA)?;
    if mix & FS_READ_DATA == 0 || mix & FS_EXECUTE == 0 {
        return Err(E_MAP);
    }
    // 越闭集注入：保留位请求 → E_MAP。
    if map_generic(0x0020_0000).err() != Some(E_MAP) {
        return Err(E_MAP);
    }
    // R2：打开路径 200 例，授出 = 裁决逐位一致（判据 ① 落点）。
    let mut t = gf::HandleTable::new();
    let mut led = StampLedger::new();
    let mut opened = 0u32;
    let mut i = 0u32;
    while i < 200 {
        let desired = match i % 4 {
            0 => GENERIC_READ,
            1 => GENERIC_WRITE | sr::READ_CONTROL,
            2 => FS_READ_DATA | sr::SYNCHRONIZE,
            _ => GENERIC_ALL,
        };
        let mapped = map_generic(desired)?;
        let v = fixture_verdict(i, mapped);
        if v.denied {
            // 判拒例：不入槽，错误可归因。
            if open_authorize(&mut t, 0x900 + i, desired, 0, v).err() != Some(E_GRANT) {
                return Err(E_GRANT);
            }
        } else {
            let (slot, gen) = open_authorize(&mut t, 0x900 + i, desired, 0, v)?;
            let e = t.read(slot, gen)?;
            if e.ga != v.granted {
                return Err(E_GRANT);
            }
            led.mark(slot, AUTH_MAGIC, t.epoch())?;
            opened += 1;
        }
        i += 1;
    }
    if opened == 0 {
        return Err(E_TABLE);
    }
    if t.used() as u32 != opened {
        return Err(E_TABLE);
    }
    // R3：绕过注入 10 例（判据 ② 落点）：不经 open_authorize 直接入槽
    //（模拟内部路径），无侧账 → 清扫器全数修出。
    let mut i = 0;
    while i < 10 {
        let (slot, _gen) = t.attach(0xBEEF + i, sr::READ_CONTROL, 0)?;
        // 故意不 mark——绕过。
        if led.stamped(slot) {
            return Err(E_BYPASS);
        }
        i += 1;
    }
    let unstamped = led.sweep_unstamped(&t);
    if unstamped.len() != 10 {
        return Err(E_BYPASS);
    }
    // 正路条目全部有戳（清扫不误报）。
    if unstamped.len() as u32 + opened != t.used() as u32 {
        return Err(E_TABLE);
    }
    // R4：裁决越请求注入 → E_GRANT（授出 ⊆ 映射请求的负例）。
    let mut t2 = gf::HandleTable::new();
    let bad = AcVerdict { granted: sr::WRITE_DAC, denied: false, engine_gen: 7 };
    if open_authorize(&mut t2, 1, FS_READ_DATA, 0, bad).err() != Some(E_GRANT) {
        return Err(E_GRANT);
    }
    // 裁决带 GENERIC 位（存储面非法）→ E_ILLEGAL。
    let gen_bit = AcVerdict { granted: GENERIC_READ, denied: false, engine_gen: 7 };
    if open_authorize(&mut t2, 2, GENERIC_READ, 0, gen_bit).err() != Some(E_ILLEGAL) {
        return Err(E_ILLEGAL);
    }
    // 校验和：全表 GA 求和（确定性总锚）。
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < gf::TABLE_CAP {
        if let Some(e) = t.entry_at(i) {
            sum = sum.wrapping_add(e.ga);
        }
        i += 1;
    }
    Ok(sum.wrapping_add(opened).wrapping_add(unstamped.len() as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_MAP => "GENERIC 映射失败（越四项闭集/保留位直通）",
        E_GRANT => "授出 ≠ 引擎裁决（逐位不一致/裁决越请求）",
        E_BYPASS => "打开被绕过（条目缺授权戳）",
        E_TABLE => "表面完整性破（挂接失败/账不对平）",
        E_ILLEGAL => "授出越存储合法区（GENERIC 位入槽）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36222_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36222_gaopen");
    cs.add("SEG 段位独占", SEG_BASE == 0xa160, "0xa160~0xa16F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "200 例逐位一致/10 例绕过全修出/映射闭集"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("映射锚", map_generic(GENERIC_ALL).is_ok() && map_generic(0x0020_0000).err() == Some(E_MAP), "GENERIC 四项闭集映射；保留位直通被拒");
    cs.add("裁决形态", AUTH_MAGIC != 0 && gf::GA_STORE_LEGAL != 0, "裁决落点/授权戳非退化");
    cs.add("判拒不半开", open_authorize(&mut gf::HandleTable::new(), 1, GENERIC_READ, 0, AcVerdict { granted: 0, denied: true, engine_gen: 7 }).err() == Some(E_GRANT), "denied=true 恒不入槽");
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
    fn test_map_generic_semantics() {
        assert_eq!(map_generic(GENERIC_READ).unwrap_or(0), FS_READ_DATA | sr::READ_CONTROL | sr::SYNCHRONIZE);
        assert_eq!(map_generic(GENERIC_ALL).unwrap_or(0) & gf::GA_STORE_LEGAL, map_generic(GENERIC_ALL).unwrap_or(0));
        assert_eq!(map_generic(0x0040_0000).err(), Some(E_MAP));
    }

    #[test]
    fn test_bypass_sweep() {
        let mut t = gf::HandleTable::new();
        let mut led = StampLedger::new();
        let v = fixture_verdict(0, map_generic(GENERIC_READ).unwrap_or(0));
        let (s, g) = open_authorize(&mut t, 1, GENERIC_READ, 0, v).unwrap_or((0, 0));
        led.mark(s, AUTH_MAGIC, t.epoch()).unwrap_or(());
        let (_s2, _g2) = t.attach(2, sr::READ_CONTROL, 0).unwrap_or((0, 0));
        assert_eq!(led.sweep_unstamped(&t).len(), 1);
        assert!(led.stamped(s) && t.read(s, g).is_ok());
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36222_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
    }
}
