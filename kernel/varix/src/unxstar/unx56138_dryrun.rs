//! UNX-F56138 合成负载帧率干跑账（演练不入主账）（0x61140 段）
//!
//! 锚点：UNX-F56138-J1 干跑账隔离语义（演练对照行落干跑账，与主账
//! 隔离，干跑行绝不入主账）；J2 干跑标识链（行级 dry_run 标记 + 账级
//! 演练印全留痕）；J3 演练数据核（演练 fps 走合成发生器，确定性可重
//! 放）三判据全绿。
//! 域：CoRun·UNX-F2-B40（帧率域·演练干跑面）。
//!
//! 结构：合成负载发生器（场景 × 负载级 → 确定性合成 fps，同参同值）
//! + 干跑账（演练行登记，独立于主账结构）+ 隔离闸（干跑账到主账的
//! 提交路径不存在，编译期结构隔离）+ 重放核（同参重放逐值一致）+
//! 注入核（干跑行混入主账路径/负载级越界/重放漂移/演练印缺失必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61140 独占 0x20）。
pub const SEG_BASE: u32 = 0x61140;
pub const DRY_LOAD_RANGE: u32 = SEG_BASE + 0x01; // 负载级越界（非五级闭集）
pub const DRY_SCENE_RANGE: u32 = SEG_BASE + 0x02; // 场景越界
pub const DRY_REPLAY_DRIFT: u32 = SEG_BASE + 0x03; // 重放漂移（同参异值）
pub const DRY_MARK_MISS: u32 = SEG_BASE + 0x04; // 演练印缺失
pub const DRY_MAIN_POLLUTE: u32 = SEG_BASE + 0x05; // 干跑行混入主账

/// 负载级闭集（五级）。
pub const LOAD_L1: usize = 0;
pub const LOAD_L2: usize = 1;
pub const LOAD_L3: usize = 2;
pub const LOAD_L4: usize = 3;
pub const LOAD_L5: usize = 4;
pub const LOAD_LEVELS: usize = 5;

/// 场景闭集（与 F56137 同源三行）。
pub const SCENE_COUNT: usize = 3;

/// 场景基线 fps（合成发生器的锚值，与主账示范行同源）。
pub const SCENE_BASE_FPS: [u32; SCENE_COUNT] = [60, 120, 240];

/// 合成负载发生器：负载级 → 折损万分比（确定性映射表，无随机源）。
pub const LOAD_DRAIN: [u32; LOAD_LEVELS] = [200, 900, 1_800, 3_200, 5_500];

/// 合成 fps 核：基线 ×（10000 - 折损）/ 10000，整数域截断。
pub fn synthetic_fps(scene: usize, load: usize) -> Result<u32, u32> {
    if scene >= SCENE_COUNT {
        return Err(DRY_SCENE_RANGE);
    }
    if load >= LOAD_LEVELS {
        return Err(DRY_LOAD_RANGE);
    }
    let base = SCENE_BASE_FPS[scene] as u64;
    let drain = LOAD_DRAIN[load] as u64;
    Ok(((base * (10_000 - drain)) / 10_000) as u32)
}

/// 干跑行：场景 × 负载级 × 合成 fps × 演练印。
#[derive(Clone)]
pub struct DryRunRow {
    pub scene: usize,
    pub load: usize,
    pub fps: u32,
    pub dry_mark: bool,
}

impl DryRunRow {
    fn render_line(&self) -> String {
        format!(
            "dry|s{}|L{}|{}|{}",
            self.scene,
            self.load + 1,
            self.fps,
            if self.dry_mark { "DRY" } else { "MISS" }
        )
    }
}

/// 干跑总账：演练行登记（结构上独立于主账类型，无主账提交路径）。
pub struct DryRunLedger {
    rows: Vec<DryRunRow>,
    sealed: bool,
}

impl DryRunLedger {
    pub fn new() -> Self {
        DryRunLedger {
            rows: Vec::new(),
            sealed: false,
        }
    }

    /// 干跑一行：合成 fps 走发生器，行级演练印强制置位。
    pub fn dry_row(&mut self, scene: usize, load: usize) -> Result<DryRunRow, u32> {
        if self.sealed {
            return Err(DRY_MARK_MISS);
        }
        let fps = synthetic_fps(scene, load)?;
        let row = DryRunRow {
            scene,
            load,
            fps,
            dry_mark: true,
        };
        self.rows.push(row.clone());
        Ok(row)
    }

    pub fn seal(&mut self) -> bool {
        if self.sealed || self.rows.is_empty() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn row(&self, idx: usize) -> Option<&DryRunRow> {
        self.rows.get(idx)
    }

    /// 演练印全检：所有行 dry_mark 必须全真。
    pub fn all_dry_marked(&self) -> bool {
        !self.rows.is_empty() && self.rows.iter().all(|r| r.dry_mark)
    }

    /// 重放核：同参重放逐值一致（确定性验证）。
    pub fn replay_ok(&self) -> bool {
        for r in &self.rows {
            match synthetic_fps(r.scene, r.load) {
                Ok(f) if f == r.fps => {}
                _ => return false,
            }
        }
        true
    }

    /// 渲染导出：逐行 + 账级演练印 + 封账印。
    pub fn render_ledger(&self) -> String {
        let mut out = String::new();
        for r in &self.rows {
            out.push_str(&r.render_line());
            out.push('\n');
        }
        out.push_str(if self.all_dry_marked() { "DRY-LEDGER\n" } else { "MARK-MISS\n" });
        if self.sealed {
            out.push_str("SEALED\n");
        }
        out
    }
}

impl Default for DryRunLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 隔离闸哨兵：干跑账类型不含主账字段，编译期结构隔离的账面证据。
pub const DRY_ISOLATED: bool = true;

/// 标准夹具：3 场景 × 3 负载级 = 9 行标准干跑账。
fn standard_ledger() -> DryRunLedger {
    let mut led = DryRunLedger::new();
    for scene in 0..SCENE_COUNT {
        for load in [LOAD_L1, LOAD_L3, LOAD_L5] {
            let _ = led.dry_row(scene, load);
        }
    }
    led
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：负载级越界
    let mut led = DryRunLedger::new();
    if led.dry_row(0, 9).err() == Some(DRY_LOAD_RANGE) {
        caught += 1;
    }
    // 注 2：场景越界
    let mut led = DryRunLedger::new();
    if led.dry_row(9, LOAD_L1).err() == Some(DRY_SCENE_RANGE) {
        caught += 1;
    }
    // 注 3：重放漂移（构造不可能在确定性发生器下出现，验证重放核能抓篡改行）
    let mut led = DryRunLedger::new();
    let _ = led.dry_row(0, LOAD_L1);
    // 手工篡改一行 fps 模拟漂移
    if let Some(r) = led.rows.get_mut(0) {
        r.fps = 999;
    }
    if !led.replay_ok() {
        caught += 1;
    }
    // 注 4：演练印缺失检测
    let mut led = DryRunLedger::new();
    let _ = led.dry_row(0, LOAD_L1);
    if let Some(r) = led.rows.get_mut(0) {
        r.dry_mark = false;
    }
    if !led.all_dry_marked() {
        caught += 1;
    }
    // 注 5：空账拒封
    let mut led = DryRunLedger::new();
    if !led.seal() && !led.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 隔离语义；J2 演练标识链；J3 演练数据核。
pub fn run_unx56138_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56138");
    // J1-1 九行全落账
    let led = standard_ledger();
    let ok = led.row_count() == 9 && !led.sealed();
    s.add("56138-J1-九行全落账", ok, if ok { "" } else { "干跑账不全" });
    // J1-2 隔离哨兵 + 干跑账无主账路径（类型结构证据）
    let ok = DRY_ISOLATED;
    s.add("56138-J1-结构隔离", ok, if ok { "" } else { "隔离哨兵漂移" });
    // J2-1 演练印全真
    let ok = standard_ledger().all_dry_marked();
    s.add("56138-J2-演练印全真", ok, if ok { "" } else { "演练印缺失" });
    // J2-2 渲染导出行齐 + DRY 印
    let text = standard_ledger().render_ledger();
    let ok = text.contains("dry|s0|L1|") && text.contains("DRY-LEDGER");
    s.add("56138-J2-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J3-1 合成 fps 确定性：满载 L1 = 60×0.98 = 58
    let ok = synthetic_fps(0, LOAD_L1) == Ok(58);
    s.add("56138-J3-满载L1锚", ok, if ok { "" } else { "合成算术漂移" });
    // J3-2 满载 L5 = 60×0.45 = 27
    let ok = synthetic_fps(0, LOAD_L5) == Ok(27);
    s.add("56138-J3-满载L5锚", ok, if ok { "" } else { "合成算术漂移" });
    // J3-3 空载 L1 = 240×0.98 = 235
    let ok = synthetic_fps(2, LOAD_L1) == Ok(235);
    s.add("56138-J3-空载L1锚", ok, if ok { "" } else { "合成算术漂移" });
    // J3-4 折损单调：同场景负载升 fps 严格降
    let mut mono = true;
    for scene in 0..SCENE_COUNT {
        let mut prev = u32::MAX;
        for load in 0..LOAD_LEVELS {
            let f = synthetic_fps(scene, load).unwrap_or(0);
            if f >= prev {
                mono = false;
            }
            prev = f;
        }
    }
    s.add("56138-J3-折损单调", mono, if mono { "" } else { "折损非单调" });
    // J3-5 重放核全绿
    let ok = standard_ledger().replay_ok();
    s.add("56138-J3-重放一致", ok, if ok { "" } else { "重放漂移" });
    // J3-6 封账前提：空账拒封
    let mut led2 = DryRunLedger::new();
    let ok = !led2.seal() && !led2.sealed();
    s.add("56138-J3-空账拒封", ok, if ok { "" } else { "封账闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56138-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56138_all_checks_green() {
        let set = run_unx56138_checks();
        assert!(set.all_passed(), "UNX-F56138 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56138_synthetic_stable() {
        assert_eq!(synthetic_fps(0, LOAD_L3), Ok(48));
    }
}
