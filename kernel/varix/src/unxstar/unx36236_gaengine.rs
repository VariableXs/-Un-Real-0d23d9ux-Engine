//! UNX-F36236-J1 句柄面与 B09 单一引擎断言（无第二套判定复核）。
//! 判据：① 引擎锚登记：句柄面所有打开路径的裁决者代数（engine_gen）
//! 必须登记进单一注册表——100 例全绿打开的裁决者锚逐例登记后，注册
//! 表内 DISTINCT 代数集恰为 1（B09 引擎 36161~36169 单一裁定的句柄
//! 侧兑现）；② 外来裁决者注入 10 例（engine_gen 漂移）全被 E_DUAL
//! 拒截且不入账——第二套判定在句柄入口即被结构性排除；③ 交叉对拍：
//! 36222 fixture 的裁决者锚与本面注册表锚同值（锚真源一致）；④ 纯
//! 功能代码 ≥320 行。
//! 设计纪律：单一引擎断言的句柄面形态——「谁裁的」必须可追责且唯
//! 一：AcVerdict.engine_gen 是裁决者身份锚，注册表是身份账；出现第
//! 二个锚 = 第二套判定引擎 = 架构红线破。本面零复述任何 ACL 求值
//!（求值在 B09），只做身份登记与唯一性断言。

use crate::checks::CheckSet;
use super::unx36102_stdrights as sr;
use super::unx36221_gafield as gf;
use super::unx36222_gaopen as go;

pub const SEG_BASE: u16 = 0xa2a0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_DUAL: u16 = SEG_BASE + 2; // 双引擎检出（裁决者锚漂移）
pub const E_REG: u16 = SEG_BASE + 3; // 注册表破（登记丢失/计数不齐）
pub const E_EMPTY: u16 = SEG_BASE + 4; // 空注册断言（无锚可查）
pub const E_XREF: u16 = SEG_BASE + 5; // 交叉对拍破（36222 锚与本表异值）
pub const E_TABLE: u16 = SEG_BASE + 6; // 表面破（打开路径异常）

/// 模块版本锚。
pub const VER: u32 = 1;

const _: () = assert!(SEG_BASE == 0xa2a0, "段位锚漂移");

/// 引擎锚注册表（判据 ① 的载体：单代数闭集 + 登记计数 + 拒截计数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EngineRegistry {
    gen: u32,
    set: bool,
    pub n: u32,
    pub dual_blocked: u32,
}

impl EngineRegistry {
    pub fn fresh() -> EngineRegistry {
        EngineRegistry { gen: 0, set: false, n: 0, dual_blocked: 0 }
    }

    /// 登记（判据 ① 的落点）：首登定锚、重登对拍、漂移即拒（E_DUAL
    /// ——第二套判定在入口被结构性排除）。
    pub fn register(&mut self, engine_gen: u32) -> Result<(), u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        if !self.set {
            self.gen = engine_gen;
            self.set = true;
            self.n = self.n.wrapping_add(1);
            return Ok(());
        }
        if self.gen != engine_gen {
            self.dual_blocked = self.dual_blocked.wrapping_add(1);
            return Err(E_DUAL);
        }
        self.n = self.n.wrapping_add(1);
        Ok(())
    }

    /// 唯一性断言（判据 ① 的判定）：已定锚 = DISTINCT 代数集恰 1。
    pub fn single_engine(&self) -> bool {
        self.set && self.n > 0
    }

    pub fn anchor(&self) -> Result<u32, u16> {
        if !self.set {
            return Err(E_EMPTY);
        }
        Ok(self.gen)
    }
}

/// 注册统计快照（四账并读：锚/登记/判拒登记/拒截）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegistryStats {
    pub anchor: u32,
    pub registered: u32,
    pub denied_registered: u32,
    pub dual_blocked: u32,
}

impl EngineRegistry {
    /// 快照导出（判拒登记数由调用侧落账传入——本表只记成功登记）。
    pub fn stats(&self, denied_registered: u32) -> Result<RegistryStats, u16> {
        Ok(RegistryStats {
            anchor: self.anchor()?,
            registered: self.n,
            denied_registered,
            dual_blocked: self.dual_blocked,
        })
    }
}

/// 外来锚扫描（判据 ② 的账面化）：一批裁决者锚中与注册锚异值的个数
/// ——第二套判定的批量检出面。
pub fn foreign_scan(reg: &EngineRegistry, anchors: &[u32]) -> u32 {
    let a = match reg.anchor() {
        Ok(x) => x,
        Err(_) => return 0,
    };
    let mut n = 0u32;
    let mut i = 0;
    while i < anchors.len() {
        if anchors[i] != a {
            n = n.wrapping_add(1);
        }
        i += 1;
    }
    n
}

/// 打开并登记（句柄面全打开路径的统一入口形态）：裁决 → 入槽 →
/// 锚登记。锚漂移则打开失败（裁决可信度先于入槽）。
pub fn open_with_anchor(
    reg: &mut EngineRegistry,
    t: &mut gf::HandleTable,
    obj: u32,
    desired: u32,
    attrs: u32,
    v: go::AcVerdict,
) -> Result<(usize, u32), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 锚先行（登记失败 → 不入槽——双重防线的第一道）。
    reg.register(v.engine_gen)?;
    let (slot, gen) = go::open_authorize(t, obj, desired, attrs, v)?;
    Ok((slot, gen))
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R0：交叉对拍（判据 ③ 落点）：36222 fixture 的裁决者锚非零且
    // 稳定——锚真源一致（两面对同一引擎）。
    let probe = go::map_generic(go::GENERIC_READ).map_err(|_| E_XREF)?;
    let v0 = go::fixture_verdict(0, probe);
    if v0.engine_gen == 0 {
        return Err(E_XREF);
    }
    let anchor_val = v0.engine_gen;
    // R1：100 例全绿打开 + 锚登记（判据 ① 落点）。
    let mut reg = EngineRegistry::fresh();
    let mut t = gf::HandleTable::new();
    let mut keys = [(0usize, 0u32); 100];
    let mut i = 0u32;
    while i < 100 {
        let desired = match i % 4 {
            0 => go::GENERIC_READ,
            1 => go::GENERIC_WRITE | sr::READ_CONTROL,
            2 => go::FS_READ_DATA | sr::SYNCHRONIZE,
            _ => go::GENERIC_ALL,
        };
        let mapped = go::map_generic(desired).map_err(|_| E_TABLE)?;
        let v = go::fixture_verdict(i, mapped);
        if v.denied {
            // 判拒例：不入槽但登记锚（拒的裁决者同一引擎——身份账齐平）。
            reg.register(v.engine_gen)?;
        } else {
            let (slot, gen) = open_with_anchor(&mut reg, &mut t, 0x1000 + i, desired, 0, v)?;
            keys[i as usize] = (slot, gen);
        }
        i += 1;
    }
    // R2：唯一性断言（判据 ① 的判定）：DISTINCT 代数集恰 1、锚同值。
    if t.read(keys[0].0, keys[0].1).is_err() {
        return Err(E_TABLE);
    }
    if !reg.single_engine() {
        return Err(E_REG);
    }
    if reg.anchor().map_err(|_| E_REG)? != anchor_val {
        return Err(E_XREF);
    }
    if reg.dual_blocked != 0 || reg.n == 0 {
        return Err(E_REG);
    }
    // R3：外来裁决者注入 10 例（判据 ② 落点）：engine_gen 漂移全拒，
    // 拒截计数逐例对账，入槽零发生；外来锚批量扫描与逐例拒截对平。
    let used_before = t.used();
    let n_before = reg.n;
    let mut foreigns = [0u32; 10];
    let mut i = 0;
    while i < 10 {
        foreigns[i] = anchor_val.wrapping_add(1 + i as u32);
        let foreign = go::AcVerdict {
            granted: sr::READ_CONTROL,
            denied: false,
            engine_gen: foreigns[i],
        };
        if open_with_anchor(&mut reg, &mut t, 0x2000 + i as u32, sr::READ_CONTROL, 0, foreign).err() != Some(E_DUAL) {
            return Err(E_DUAL);
        }
        i += 1;
    }
    if reg.dual_blocked != 10 || reg.n != n_before || t.used() != used_before {
        return Err(E_REG);
    }
    if foreign_scan(&reg, &foreigns) != 10 {
        return Err(E_DUAL);
    }
    // 判拒例的登记计数复核（R1 中 denied 例也登记了锚——计数齐平）。
    if reg.n != 100 {
        return Err(E_REG);
    }
    // R2b：注册统计快照（锚/登记/拒截三账并读一致）。
    let st = reg.stats(0).map_err(|_| E_REG)?;
    if st.anchor != anchor_val || st.registered != 100 || st.dual_blocked != 10 {
        return Err(E_REG);
    }
    // R4：空注册断言（判据面负例）：未定锚的注册表 anchor → E_EMPTY。
    if EngineRegistry::fresh().anchor().err() != Some(E_EMPTY) {
        return Err(E_EMPTY);
    }
    // R5：表面负例：外来裁决直走 open_authorize（绕锚）虽能入槽，但
    // 注册表断言随后翻红——防线纵深（登记侧独立于入槽侧）。
    let mut t2 = gf::HandleTable::new();
    let sneaky = go::AcVerdict { granted: sr::READ_CONTROL, denied: false, engine_gen: 0xDEAD };
    let (_s, _g) = go::open_authorize(&mut t2, 9, sr::READ_CONTROL, 0, sneaky).map_err(|_| E_TABLE)?;
    let mut reg2 = EngineRegistry::fresh();
    reg2.register(anchor_val).map_err(|_| E_REG)?;
    if reg2.register(0xDEAD).err() != Some(E_DUAL) {
        return Err(E_DUAL);
    }
    // R6：锚跨关闭存活（判据 ① 的生命周期锚）：关闭全部活句柄后同锚
    // 重开——注册表只认裁决者身份，不随句柄生灭（判拒例键为空跳过）。
    let mut i = 0;
    while i < 100 {
        let (slot, gen) = keys[i];
        if gen != 0 {
            t.release(slot, gen).map_err(|_| E_TABLE)?;
        }
        i += 1;
    }
    if t.used() != 0 {
        return Err(E_TABLE);
    }
    let mapped = go::map_generic(go::GENERIC_READ).map_err(|_| E_TABLE)?;
    let v = go::fixture_verdict(0, mapped);
    let (_s2, _g2) = open_with_anchor(&mut reg, &mut t, 0x3000, go::GENERIC_READ, 0, v)?;
    if reg.anchor().map_err(|_| E_REG)? != anchor_val {
        return Err(E_XREF);
    }
    if reg.n != 101 {
        return Err(E_REG);
    }
    // 校验和：锚值 + 登记数 + 拒截数（确定性总锚）。
    Ok(anchor_val
        .wrapping_add(reg.n)
        .wrapping_add(reg.dual_blocked))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_DUAL => "双引擎检出（裁决者锚漂移——第二套判定被拒）",
        E_REG => "注册表破（登记计数不齐/拒截账不平）",
        E_EMPTY => "空注册断言（无锚可查）",
        E_XREF => "交叉对拍破（36222 锚与本表异值）",
        E_TABLE => "表面破（打开路径异常）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36236_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36236_gaengine");
    cs.add("SEG 段位独占", SEG_BASE == 0xa2a0, "0xa2a0~0xa2aF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "100 例单锚登记/10 例漂移全拒/交叉对拍一致"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("空表诚实", EngineRegistry::fresh().anchor().is_err() && !EngineRegistry::fresh().single_engine(), "未定锚注册表不可查（无锚不假绿）");
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
    fn test_single_anchor_and_drift() {
        let mut reg = EngineRegistry::fresh();
        assert!(reg.register(7).is_ok());
        assert!(reg.register(7).is_ok());
        assert_eq!(reg.n, 2);
        assert_eq!(reg.register(9).err(), Some(E_DUAL));
        assert_eq!(reg.dual_blocked, 1);
        assert_eq!(reg.anchor().unwrap_or(0), 7);
        assert!(reg.single_engine());
    }

    #[test]
    fn test_open_with_anchor_blocks_foreign() {
        let mut reg = EngineRegistry::fresh();
        let mut t = gf::HandleTable::new();
        let mapped = go::map_generic(go::GENERIC_READ).unwrap_or(0);
        let v = go::fixture_verdict(0, mapped);
        assert!(open_with_anchor(&mut reg, &mut t, 1, go::GENERIC_READ, 0, v).is_ok());
        let foreign = go::AcVerdict { granted: sr::READ_CONTROL, denied: false, engine_gen: 99 };
        assert_eq!(open_with_anchor(&mut reg, &mut t, 2, sr::READ_CONTROL, 0, foreign).err(), Some(E_DUAL));
        // 被拒的打开零入槽。
        assert_eq!(t.used(), 1);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36236_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 3);
    }
}
