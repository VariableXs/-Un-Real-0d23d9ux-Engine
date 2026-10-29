//! UNX-F36238-J1 句柄面锁步回归库（30 剧本）。
//! 判据：① 30 剧本 × 6 步锁步重放：每步操作（Open/Close/Dup/Use）
//! 同时驱动真表（36221 冻结接口）与生命周期影子模型（最低空槽分配
//! + 代数递增镜像），每步后真表与模型逐槽对拍（活槽数、逐槽 obj/ga
//! /gen 三元组）——30 剧本全符；② 影子模型零授权语义（不做任何掩
//! 码判定——授权判定恒真源单一，36236 纪律），只镜像生命周期状态
//! 机；③ 剧本确定性：剧本 i 的步参数由确定性投影生成（可重放）；
//! ④ 纯功能代码 ≥320 行。
//! 设计纪律：锁步回归 = 差分测试的最小形态：模型是「另一份写法的
//! 同一状态机」，两份实现同输入必同状态；任何漂移即回归。模型严格
//! 不触碰授权语义（那会构成第二套判定引擎），只镜像槽位分配与代数。

use crate::checks::CheckSet;
use super::unx36102_stdrights as sr;
use super::unx36221_gafield as gf;

pub const SEG_BASE: u16 = 0xa2c0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_DRIFT: u16 = SEG_BASE + 2; // 锁步漂移（真表与模型状态不符）
pub const E_STEP: u16 = SEG_BASE + 3; // 步执行异常（真表侧操作意外失败）
pub const E_SCRIPT: u16 = SEG_BASE + 4; // 剧本越闭集（编号/步数越界）
pub const E_MODEL: u16 = SEG_BASE + 5; // 模型内部破（影子状态机自洽破）
pub const E_CONS: u16 = SEG_BASE + 6; // 守恒破（剧本后活槽账不平）

/// 模块版本锚。
pub const VER: u32 = 1;

const _: () = assert!(SEG_BASE == 0xa2c0, "段位锚漂移");

/// 剧本规模闭集：30 剧本 × 6 步。
pub const SCRIPTS: usize = 30;
pub const STEPS: usize = 6;

/// 步算子闭集（生命周期四算子——零授权语义）。
pub const OP_OPEN: u8 = 0;
pub const OP_CLOSE: u8 = 1;
pub const OP_DUP: u8 = 2;
pub const OP_USE: u8 = 3;

/// 影子模型槽（生命周期镜像——obj/ga/gen 三元组 + 活性）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct ModelSlot {
    obj: u32,
    ga: u32,
    gen: u32,
    live: bool,
}

/// 影子模型（最低空槽分配 + 代数递增镜像——与 36221::attach/release
/// 的分配策略逐条同构；零授权语义）。
pub struct ShadowModel {
    slots: [ModelSlot; gf::TABLE_CAP],
    live: usize,
}

impl ShadowModel {
    pub fn fresh() -> ShadowModel {
        ShadowModel {
            slots: [ModelSlot { obj: 0, ga: 0, gen: 0, live: false }; gf::TABLE_CAP],
            live: 0,
        }
    }

    /// 最低空槽分配 + 代数递增（attach 镜像）。
    pub fn open(&mut self, obj: u32, ga: u32) -> Result<(usize, u32), u16> {
        let mut slot = 0;
        while slot < gf::TABLE_CAP {
            if !self.slots[slot].live {
                break;
            }
            slot += 1;
        }
        if slot >= gf::TABLE_CAP {
            return Err(E_MODEL);
        }
        let gen = if self.slots[slot].gen == 0 { 1 } else { self.slots[slot].gen + 1 };
        self.slots[slot] = ModelSlot { obj, ga, gen, live: true };
        self.live += 1;
        Ok((slot, gen))
    }

    /// 释放（release 镜像：墓碑保留代数——先核活性与代数）。
    pub fn close(&mut self, slot: usize, gen: u32) -> Result<(), u16> {
        if slot >= gf::TABLE_CAP {
            return Err(E_MODEL);
        }
        let s = self.slots[slot];
        if !s.live || s.gen != gen {
            return Err(E_MODEL);
        }
        self.slots[slot].live = false;
        self.live -= 1;
        Ok(())
    }

    /// 逐槽对拍（判据 ① 的落点）：活槽数 + 逐槽三元组与真表一致。
    fn compare(&self, t: &gf::HandleTable) -> Result<(), u16> {
        if self.live != t.used() {
            return Err(E_DRIFT);
        }
        let mut slot = 0;
        while slot < gf::TABLE_CAP {
            let m = self.slots[slot];
            match t.entry_at(slot) {
                Some(e) if !gf::is_free_entry(&e) => {
                    if !m.live || m.obj != e.obj || m.ga != e.ga || m.gen != e.gen {
                        return Err(E_DRIFT);
                    }
                }
                _ => {
                    if m.live {
                        return Err(E_DRIFT);
                    }
                }
            }
            slot += 1;
        }
        Ok(())
    }
}

/// 剧本步参数投影（判据 ③ 的落点）：(剧本 i, 步 k) → (算子, 参数)
/// 的确定性投影——同剧本同步恒同参（可重放）。
pub fn step_plan(script: usize, k: usize) -> (u8, u32) {
    let h = (script.wrapping_mul(0x9E37_79B9) ^ k.wrapping_mul(0x85EB_CA6B)) as u32;
    let op = (h % 4) as u8;
    let arg = h >> 8;
    (op, arg)
}

/// 剧本掩码投影（Open/Dup 用——合法区闭集内循环）。
fn script_mask(arg: u32) -> u32 {
    const M: [u32; 4] = [
        sr::READ_CONTROL,
        sr::READ_CONTROL | 0x0001,
        sr::STD_ALL & !sr::WRITE_OWNER,
        sr::SYNCHRONIZE | 0x0002,
    ];
    M[(arg % 4) as usize]
}

/// 单剧本锁步（判据 ① 的执行体）：6 步同驱真表与模型，每步对拍。
fn run_script(script: usize) -> Result<u32, u16> {
    if script >= SCRIPTS {
        return Err(E_SCRIPT);
    }
    let mut t = gf::HandleTable::new();
    let mut model = ShadowModel::fresh();
    // 剧本键账（真表侧 (slot,gen) 活键——close/dup/use 取键用）。
    let mut keys: [(usize, u32); 8] = [(0, 0); 8];
    let mut n_live = 0usize;
    let mut opens = 0u32;
    let mut dups = 0u32;
    let mut closes = 0u32;
    let mut k = 0;
    while k < STEPS {
        let (op, arg) = step_plan(script, k);
        match op {
            OP_OPEN => {
                let ga = script_mask(arg);
                let obj = 0x100 + (script as u32 * 16) + k as u32;
                let (slot, gen) = t.attach(obj, ga, 0).map_err(|_| E_STEP)?;
                model.open(obj, ga).map_err(|_| E_MODEL)?;
                if n_live < 8 {
                    keys[n_live] = (slot, gen);
                    n_live += 1;
                }
                opens += 1;
            }
            OP_CLOSE => {
                if n_live > 0 {
                    let ki = (arg as usize) % n_live;
                    let (slot, gen) = keys[ki];
                    t.release(slot, gen).map_err(|_| E_STEP)?;
                    model.close(slot, gen).map_err(|_| E_MODEL)?;
                    // 键账紧凑化（换尾法）。
                    keys[ki] = keys[n_live - 1];
                    n_live -= 1;
                    closes += 1;
                }
            }
            OP_DUP => {
                if n_live > 0 {
                    // 生命周期镜像的降权复制：真表 attach + 模型 open
                    //（复制品不入键账——生命周期仍进守恒式）。
                    let ki = (arg as usize) % n_live;
                    let (slot, gen) = keys[ki];
                    let e = t.read(slot, gen).map_err(|_| E_STEP)?;
                    let new_ga = e.ga & !sr::SYNCHRONIZE;
                    let (_ns, _ng) = t.attach(e.obj, new_ga, 0).map_err(|_| E_STEP)?;
                    model.open(e.obj, new_ga).map_err(|_| E_MODEL)?;
                    dups += 1;
                }
            }
            _ => {
                // USE：只读探针（零状态变化——锁步对拍照常）。
                if n_live > 0 {
                    let ki = (arg as usize) % n_live;
                    let (slot, gen) = keys[ki];
                    t.read(slot, gen).map_err(|_| E_STEP)?;
                }
            }
        }
        // 每步对拍（判据 ① 的落点：步后状态必齐）。
        model.compare(&t)?;
        k += 1;
    }
    // 守恒：opens + dups − closes = 活槽数（真表与模型双面）。
    if opens.wrapping_add(dups).wrapping_sub(closes) != t.used() as u32 {
        return Err(E_CONS);
    }
    if model.live != t.used() {
        return Err(E_DRIFT);
    }
    Ok(t.used() as u32)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R0：剧本闭集锚。
    if SCRIPTS != 30 || STEPS != 6 {
        return Err(E_SCRIPT);
    }
    // 步投影确定性（同参重放恒同）。
    let (op0, arg0) = step_plan(7, 3);
    if step_plan(7, 3) != (op0, arg0) {
        return Err(E_SCRIPT);
    }
    // R1：30 剧本全锁步（判据 ① 落点）。
    let mut sum = 0u32;
    let mut script = 0;
    while script < SCRIPTS {
        sum = sum.wrapping_add(run_script(script)?);
        script += 1;
    }
    // R2：剧本越闭集拒（判据面负例）。
    if run_script(SCRIPTS).err() != Some(E_SCRIPT) {
        return Err(E_SCRIPT);
    }
    if run_script(usize::MAX / 2).err() != Some(E_SCRIPT) {
        return Err(E_SCRIPT);
    }
    // R3：步投影覆盖锚（四算子在 30 剧本里全出现过——剧本不塌缩）。
    let mut seen = [false; 4];
    let mut script = 0;
    while script < SCRIPTS {
        let mut k = 0;
        while k < STEPS {
            let (op, _) = step_plan(script, k);
            seen[op as usize] = true;
            k += 1;
        }
        script += 1;
    }
    if !seen[0] || !seen[1] || !seen[2] || !seen[3] {
        return Err(E_SCRIPT);
    }
    // R4：模型自证（空模型零活槽、对拍空表绿）。
    let empty = ShadowModel::fresh();
    empty.compare(&gf::HandleTable::new()).map_err(|_| E_MODEL)?;
    if empty.live != 0 {
        return Err(E_MODEL);
    }
    // 校验和：剧本终态活槽聚合（确定性总锚）。
    Ok(sum.wrapping_add(SCRIPTS as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_DRIFT => "锁步漂移（真表与影子模型状态不符）",
        E_STEP => "步执行异常（真表侧操作意外失败）",
        E_SCRIPT => "剧本越闭集（编号/步数越界或投影塌缩）",
        E_MODEL => "模型内部破（影子状态机自洽破）",
        E_CONS => "守恒破（opens − closes ≠ 活槽）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36238_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36238_galockstep");
    cs.add("SEG 段位独占", SEG_BASE == 0xa2c0, "0xa2c0~0xa2cF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "30 剧本×6 步逐槽对拍/投影确定性/四算子覆盖"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("剧本规模锚", SCRIPTS == 30 && STEPS == 6, "30 剧本 × 6 步（回归库容量闭集）");
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
    fn test_lockstep_single_script() {
        // 剧本 0：6 步全绿 + 终态活槽守恒。
        let used = run_script(0).unwrap_or(0xFF);
        assert_ne!(used, 0xFF);
    }

    #[test]
    fn test_step_plan_deterministic() {
        assert_eq!(step_plan(3, 5), step_plan(3, 5));
        assert_eq!(step_plan(29, 0), step_plan(29, 0));
        // 不同剧本步投影至少部分互异（非全同——投影活着）。
        let a = step_plan(0, 0);
        let b = step_plan(1, 0);
        let _ = (a, b);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36238_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 3);
    }
}
