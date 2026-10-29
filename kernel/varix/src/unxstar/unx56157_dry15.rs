//! UNX-F56157 骨架干跑总账（演练对照行 ×15）（0x613A0 段）
//!
//! 锚点：UNX-F56157-J1 干跑总账（15 演练对照行逐行落账：行号 × 场景
//! × 合成值 × 演练印四元组）；J2 演练隔离语义（演练账独立于主账结构
//! + 演练印全真 + 干跑值走确定性合成器）；J3 十五行闭集核（行号连续
//! 1..15 + 场景轮转闭集）三判据全绿。
//! 域：CoRun·UNX-F2-B40（账本域·骨架干跑面）。
//!
//! 结构：合成发生器（场景轮转 × 负载轮转 → 确定性合成值）+ 干跑总账
//! （15 行登记 + 演练印）+ 行闭集核（连续行号 + 场景轮转）+ 总签（十
//! 五行折叠）+ 注入核（行断档/演练印缺失/合成漂移/总签漂移/封账后写
//! 必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x613A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x613A0;
pub const DR15_GAP: u32 = SEG_BASE + 0x01; // 行断档
pub const DR15_MARK_MISS: u32 = SEG_BASE + 0x02; // 演练印缺失
pub const DR15_SYNTH_DRIFT: u32 = SEG_BASE + 0x03; // 合成漂移
pub const DR15_FP_DRIFT: u32 = SEG_BASE + 0x04; // 总签漂移
pub const DR15_SEALED: u32 = SEG_BASE + 0x05; // 封账后写

/// 演练行数闭集（15 行）。
pub const DRY_ROWS: usize = 15;

/// 场景轮转闭集（3 场景 × 5 负载级 = 15 行）。
pub const SCENE_ROTATE: [&str; 3] = ["full_load", "mid_load", "idle"];
pub const LOAD_ROTATE: [u32; 5] = [200, 900, 1800, 3200, 5500];

/// 合成值核：场景基线 ×（10000 - 折损）/ 10000。
/// 场景基线：full 60 / mid 120 / idle 240（fps）。
pub fn synth_fps(scene_idx: usize, load_idx: usize) -> u32 {
    let base: u64 = match scene_idx % 3 {
        0 => 60,
        1 => 120,
        _ => 240,
    };
    let drain = LOAD_ROTATE[load_idx % 5] as u64;
    ((base * (10_000 - drain)) / 10_000) as u32
}

/// 行号 → (场景, 负载) 轮转映射。
pub fn rotate_of(row_no: usize) -> (usize, usize) {
    ((row_no - 1) % 3, (row_no - 1) / 3 % 5)
}

/// 单条演练行。
#[derive(Clone)]
pub struct Dry15Row {
    pub row_no: usize,
    pub scene: String,
    pub load_desc: String,
    pub synth: u32,
    pub dry_mark: bool,
}

impl Dry15Row {
    fn render_line(&self) -> String {
        format!(
            "r{}|{}|{}|{}|{}",
            self.row_no,
            self.scene,
            self.load_desc,
            self.synth,
            if self.dry_mark { "DRY" } else { "MISS" }
        )
    }
}

/// 干跑总账。
pub struct Dry15Ledger {
    rows: Vec<Dry15Row>,
    sealed: bool,
}

impl Dry15Ledger {
    pub fn new() -> Self {
        Dry15Ledger {
            rows: Vec::new(),
            sealed: false,
        }
    }

    /// 干跑一行：行号连续闸 + 合成器确定性值 + 演练印强制。
    pub fn dry_row(&mut self, scene_idx: usize, load_idx: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(DR15_SEALED);
        }
        if self.rows.len() >= DRY_ROWS {
            return Err(DR15_GAP);
        }
        let row_no = self.rows.len() + 1;
        let (si, li) = rotate_of(row_no);
        let scene = SCENE_ROTATE[scene_idx % 3].to_string();
        // 行号轮转一致性闸：传入轮转必须与行号轮转一致（防断档错位）
        if si != scene_idx % 3 {
            return Err(DR15_GAP);
        }
        let synth = synth_fps(si, load_idx % 5);
        let _ = li;
        self.rows.push(Dry15Row {
            row_no,
            scene,
            load_desc: format!("L{}", load_idx % 5 + 1),
            synth,
            dry_mark: true,
        });
        Ok(row_no)
    }

    pub fn seal(&mut self) -> bool {
        if self.sealed || self.rows.len() != DRY_ROWS {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 行闭集核：行号 1..15 连续 + 演练印全真。
    pub fn closed_ok(&self) -> bool {
        self.rows.iter().enumerate().all(|(i, r)| r.row_no == i + 1) && self.all_dry()
    }

    /// 演练印全真。
    pub fn all_dry(&self) -> bool {
        !self.rows.is_empty() && self.rows.iter().all(|r| r.dry_mark)
    }

    /// 合成重放核：逐行重算一致。
    pub fn replay_ok(&self) -> bool {
        self.rows.iter().all(|r| {
            let si = SCENE_ROTATE.iter().position(|s| *s == r.scene).unwrap_or(9);
            let li = r.load_desc.trim_start_matches('L').parse::<usize>().unwrap_or(9) - 1;
            synth_fps(si, li) == r.synth
        })
    }

    /// 总签：逐行（行号 × 合成值）折叠。
    pub fn total_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for r in &self.rows {
            h = h.wrapping_mul(0x0100_0193) ^ r.row_no as u32;
            h = h.wrapping_mul(0x0100_0193) ^ r.synth;
        }
        h
    }

    /// 渲染导出。
    pub fn render_ledger(&self) -> String {
        let mut out = String::new();
        for r in &self.rows {
            out.push_str(&r.render_line());
            out.push('\n');
        }
        out.push_str(&format!("TOTAL|{:08x}\n", self.total_fp()));
        if self.sealed {
            out.push_str("DRY15-SEAL\n");
        }
        out
    }
}

impl Default for Dry15Ledger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：15 行全演练（3 场景 × 5 负载轮转）。
fn standard_ledger() -> Dry15Ledger {
    let mut led = Dry15Ledger::new();
    for load in 0..5 {
        for scene in 0..3 {
            let _ = led.dry_row(scene, load);
        }
    }
    led
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：超行数闭集（第 16 行拒）
    let mut led = standard_ledger();
    if led.dry_row(0, 0) == Err(DR15_GAP) {
        caught += 1;
    }
    // 注 2：演练印缺失检测（手工抹印后 all_dry 必假）
    let mut led = standard_ledger();
    if let Some(r) = led.rows.get_mut(0) {
        r.dry_mark = false;
    }
    if !led.all_dry() {
        caught += 1;
    }
    // 注 3：合成漂移（手工改合成值后 replay_ok 必假）
    let mut led = standard_ledger();
    if let Some(r) = led.rows.get_mut(0) {
        r.synth = 1;
    }
    if !led.replay_ok() {
        caught += 1;
    }
    // 注 4：总签漂移（改值后 total_fp 变）
    let fp0 = standard_ledger().total_fp();
    let mut led = standard_ledger();
    if let Some(r) = led.rows.get_mut(7) {
        r.synth ^= 1;
    }
    if led.total_fp() != fp0 {
        caught += 1;
    }
    // 注 5：封账后写
    let mut led = standard_ledger();
    let _ = led.seal();
    if led.dry_row(0, 0) == Err(DR15_SEALED) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 十五行账；J2 隔离语义；J3 闭集核。
pub fn run_unx56157_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56157");
    // J1-1 十五行全落账
    let led = standard_ledger();
    let ok = led.len() == DRY_ROWS && led.closed_ok();
    s.add("56157-J1-十五行全落账", ok, if ok { "" } else { "干跑账不全" });
    // J1-2 渲染导出齐
    let text = standard_ledger().render_ledger();
    let ok = text.contains("r1|full_load|L1|58|DRY") && text.contains("TOTAL|") && text.contains("r15|");
    s.add("56157-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 演练印全真
    let ok = standard_ledger().all_dry();
    s.add("56157-J2-演练印全真", ok, if ok { "" } else { "演练印缺失" });
    // J2-2 合成器确定性：r1 = full L1 = 60×0.98 = 58
    let ok = synth_fps(0, 0) == 58;
    s.add("56157-J2-合成锚r1", ok, if ok { "" } else { "合成算术漂移" });
    // J2-3 合成锚：r15 = idle L5 = 240×0.45 = 108
    let ok = synth_fps(2, 4) == 108;
    s.add("56157-J2-合成锚r15", ok, if ok { "" } else { "合成算术漂移" });
    // J2-4 轮转映射：r7 → (0, 2)
    let ok = rotate_of(7) == (0, 2) && rotate_of(1) == (0, 0) && rotate_of(15) == (2, 4);
    s.add("56157-J2-轮转映射", ok, if ok { "" } else { "轮转漂移" });
    // J2-5 合成重放全绿
    let ok = standard_ledger().replay_ok();
    s.add("56157-J2-合成重放", ok, if ok { "" } else { "重放漂移" });
    // J3-1 行号连续
    let ok = standard_ledger().rows.iter().enumerate().all(|(i, r)| r.row_no == i + 1);
    s.add("56157-J3-行号连续", ok, if ok { "" } else { "行号漂移" });
    // J3-2 场景轮转闭集（每场景 5 次）
    let led = standard_ledger();
    let ok = (0..3).all(|si| {
        let name = SCENE_ROTATE[si];
        led.rows.iter().filter(|r| r.scene == name).count() == 5
    });
    s.add("56157-J3-场景轮转", ok, if ok { "" } else { "轮转漂移" });
    // J3-3 总签确定性
    let ok = standard_ledger().total_fp() == standard_ledger().total_fp();
    s.add("56157-J3-总签确定", ok, if ok { "" } else { "总签漂移" });
    // J3-4 封账前提：不足 15 行拒封
    let mut led2 = Dry15Ledger::new();
    let _ = led2.dry_row(0, 0);
    let ok = !led2.seal() && !led2.sealed();
    s.add("56157-J3-不足拒封", ok, if ok { "" } else { "封账闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56157-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56157_all_checks_green() {
        let set = run_unx56157_checks();
        assert!(set.all_passed(), "UNX-F56157 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56157_synth_stable() {
        assert_eq!(synth_fps(0, 0), 58);
        assert_eq!(synth_fps(2, 4), 108);
    }
}
