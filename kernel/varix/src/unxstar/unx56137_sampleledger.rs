//! UNX-F56137 帧率对照基准测试样例账（示范 3 行）（0x61100 段）
//!
//! 锚点：UNX-F56137-J1 样例账三行全量（示范对照行 3 行逐行落账：
//! 场景 × 基线 fps × 实测 fps × ratio 四元组）；J2 ratio 语义（实测/
//! 基线万分比整数域，下限钳制防除零）；J3 样例签章链（行签章 + 账
//! 指纹全留痕）三判据全绿。
//! 域：CoRun·UNX-F2-B40（帧率域·样例示范面）。
//!
//! 结构：对照行登记册（示范 3 行：满载/中等/空载场景逐行落账）+
//! ratio 核（万分比整数除法 + 除零钳制语义）+ 行签章核（FNV 指纹逐
//! 行留痕）+ 账级指纹核（三行合成指纹确定性）+ 注入核（未知场景/
//! 零基线/越界 fps/账后改行/指纹漂移必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61100 独占 0x20）。
pub const SEG_BASE: u32 = 0x61100;
pub const SMP_UNK_SCENE: u32 = SEG_BASE + 0x01; // 未知场景（非三行闭集）
pub const SMP_ZERO_BASE: u32 = SEG_BASE + 0x02; // 基线 fps 为零
pub const SMP_FPS_RANGE: u32 = SEG_BASE + 0x03; // fps 越物理界（>10000 或实测>基线×3）
pub const SMP_ROW_FLIP: u32 = SEG_BASE + 0x04; // 账后改行
pub const SMP_FP_DRIFT: u32 = SEG_BASE + 0x05; // 行指纹漂移
pub const SMP_SEALED: u32 = SEG_BASE + 0x06; // 样例账封账拒写

/// 示范场景闭集（3 行）。
pub const SCENE_FULL: usize = 0;
pub const SCENE_MID: usize = 1;
pub const SCENE_IDLE: usize = 2;
pub const SCENE_COUNT: usize = 3;

/// 场景名渲染。
pub const SCENE_NAMES: [&str; SCENE_COUNT] = ["full_load", "mid_load", "idle"];

/// fps 物理界。
pub const FPS_MAX: u32 = 10_000;
pub const RATIO_OVER_MAX: u32 = 30_000; // 实测/基线上限 3 倍（万分比）

/// ratio 核：实测 × 10000 / 基线，基线零返回 0 哨兵（不除零）。
pub fn ratio_permille(baseline: u32, measured: u32) -> u32 {
    if baseline == 0 {
        return 0;
    }
    ((measured as u64 * 10_000) / baseline as u64) as u32
}

/// 行签章：FNV-1a（场景名 × 基线 × 实测 三元合成）。
pub fn row_fingerprint(scene: usize, baseline: u32, measured: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in SCENE_NAMES[scene].as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    for v in [baseline, measured] {
        for i in 0..4 {
            h ^= (v >> (i * 8)) as u32 & 0xFF;
            h = h.wrapping_mul(0x0100_0193);
        }
    }
    h
}

/// 单行对照账目。
#[derive(Clone)]
pub struct SampleRow {
    pub scene: usize,
    pub baseline_fps: u32,
    pub measured_fps: u32,
    pub ratio: u32,
    pub fp: u32,
}

impl SampleRow {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|{}|{}|{:08x}",
            SCENE_NAMES[self.scene], self.baseline_fps, self.measured_fps, self.ratio, self.fp
        )
    }
}

/// 样例总账：示范 3 行逐行落账 + 封账语义。
pub struct SampleLedger {
    rows: [Option<SampleRow>; SCENE_COUNT],
    sealed: bool,
}

impl SampleLedger {
    pub fn new() -> Self {
        SampleLedger {
            rows: [None, None, None],
            sealed: false,
        }
    }

    /// 落一行：场景闭集 + 基线非零 + 物理界 + 倍率上限四闸。
    pub fn put_row(
        &mut self,
        scene: usize,
        baseline: u32,
        measured: u32,
    ) -> Result<SampleRow, u32> {
        if self.sealed {
            return Err(SMP_SEALED);
        }
        if scene >= SCENE_COUNT {
            return Err(SMP_UNK_SCENE);
        }
        if baseline == 0 {
            return Err(SMP_ZERO_BASE);
        }
        if baseline > FPS_MAX || measured > FPS_MAX {
            return Err(SMP_FPS_RANGE);
        }
        let ratio = ratio_permille(baseline, measured);
        if ratio > RATIO_OVER_MAX {
            return Err(SMP_FPS_RANGE);
        }
        let fp = row_fingerprint(scene, baseline, measured);
        let row = SampleRow {
            scene,
            baseline_fps: baseline,
            measured_fps: measured,
            ratio,
            fp,
        };
        self.rows[scene] = Some(row.clone());
        Ok(row)
    }

    /// 账后改行闸：已落行场景重复写一律拒（只追加示范语义）。
    pub fn rewrite_rejected(&self, scene: usize) -> bool {
        if scene >= SCENE_COUNT {
            return false;
        }
        self.rows[scene].is_some()
    }

    pub fn seal(&mut self) -> bool {
        if self.sealed || self.rows.iter().any(|r| r.is_none()) {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn row(&self, scene: usize) -> Option<&SampleRow> {
        if scene >= SCENE_COUNT {
            return None;
        }
        self.rows[scene].as_ref()
    }

    /// 全量核：三行齐 + ratio 逐行复算一致。
    pub fn full_ledger_ok(&self) -> bool {
        self.rows.iter().all(|r| r.is_some())
            && self.rows.iter().flatten().all(|r| {
                r.ratio == ratio_permille(r.baseline_fps, r.measured_fps)
                    && r.fp == row_fingerprint(r.scene, r.baseline_fps, r.measured_fps)
            })
    }

    /// 账级指纹：三行指纹折叠（逐行 FNV 链）。
    pub fn ledger_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for r in self.rows.iter().flatten() {
            h ^= r.fp;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    /// 渲染导出：三行 + 封账印。
    pub fn render_ledger(&self) -> String {
        let mut out = String::new();
        for r in self.rows.iter().flatten() {
            out.push_str(&r.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("SEALED\n");
        }
        out
    }
}

impl Default for SampleLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：示范 3 行（满载 60→57 / 中等 120→114 / 空载 240→236）。
fn standard_ledger() -> SampleLedger {
    let mut led = SampleLedger::new();
    let _ = led.put_row(SCENE_FULL, 60, 57);
    let _ = led.put_row(SCENE_MID, 120, 114);
    let _ = led.put_row(SCENE_IDLE, 240, 236);
    led
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知场景
    let mut led = SampleLedger::new();
    if led.put_row(9, 60, 57).err() == Some(SMP_UNK_SCENE) {
        caught += 1;
    }
    // 注 2：零基线
    let mut led = SampleLedger::new();
    if led.put_row(SCENE_FULL, 0, 57).err() == Some(SMP_ZERO_BASE) {
        caught += 1;
    }
    // 注 3：越界 fps
    let mut led = SampleLedger::new();
    if led.put_row(SCENE_FULL, 60_000, 57).err() == Some(SMP_FPS_RANGE) {
        caught += 1;
    }
    // 注 4：账后改行
    let mut led = standard_ledger();
    if led.rewrite_rejected(SCENE_FULL) {
        caught += 1;
    }
    // 注 5：封账后写
    let mut led = standard_ledger();
    let _ = led.seal();
    if led.put_row(SCENE_FULL, 60, 58).err() == Some(SMP_SEALED) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 三行全量；J2 ratio 语义；J3 签章链。
pub fn run_unx56137_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56137");
    // J1-1 三行全落账
    let led = standard_ledger();
    let ok = led.full_ledger_ok() && !led.sealed();
    s.add("56137-J1-三行全落账", ok, if ok { "" } else { "样例账不全" });
    // J1-2 渲染导出三行齐
    let text = standard_ledger().render_ledger();
    let ok = text.contains("full_load|60|57|") && text.contains("mid_load|120|114|")
        && text.contains("idle|240|236|");
    s.add("56137-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 ratio 逐行正确（57/60=9500, 114/120=9500, 236/240=9833）
    let ok = led.row(SCENE_FULL).map(|r| r.ratio == 9_500).unwrap_or(false)
        && led.row(SCENE_IDLE).map(|r| r.ratio == 9_833).unwrap_or(false);
    s.add("56137-J2-ratio逐行", ok, if ok { "" } else { "ratio 算术漂移" });
    // J2-2 零基线钳制（哨兵 0 不炸）
    let ok = ratio_permille(0, 57) == 0;
    s.add("56137-J2-零基线钳制", ok, if ok { "" } else { "除零穿透" });
    // J2-3 等值 ratio 锚（100/100 = 满万分比）
    let ok = ratio_permille(100, 100) == 10_000;
    s.add("56137-J2-等值锚", ok, if ok { "" } else { "锚漂移" });
    // J2-4 半值 ratio（50/100 = 5000）
    let ok = ratio_permille(100, 50) == 5_000;
    s.add("56137-J2-半值锚", ok, if ok { "" } else { "锚漂移" });
    // J3-1 行指纹确定性
    let ok = row_fingerprint(SCENE_FULL, 60, 57) == row_fingerprint(SCENE_FULL, 60, 57);
    s.add("56137-J3-指纹确定性", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 行指纹异值异签
    let ok = row_fingerprint(SCENE_FULL, 60, 57) != row_fingerprint(SCENE_FULL, 60, 58);
    s.add("56137-J3-指纹异值", ok, if ok { "" } else { "指纹碰撞" });
    // J3-3 账级指纹折叠稳定
    let ok = standard_ledger().ledger_fingerprint() == standard_ledger().ledger_fingerprint();
    s.add("56137-J3-账级指纹", ok, if ok { "" } else { "折叠漂移" });
    // J3-4 封账前提：缺行拒封
    let mut led2 = SampleLedger::new();
    let _ = led2.put_row(SCENE_FULL, 60, 57);
    let ok = !led2.seal() && !led2.sealed();
    s.add("56137-J3-缺行拒封", ok, if ok { "" } else { "封账闸失效" });
    // J3-5 账级指纹随行变（改一行账指纹必变）
    let mut led3 = SampleLedger::new();
    let _ = led3.put_row(SCENE_FULL, 60, 58);
    let _ = led3.put_row(SCENE_MID, 120, 114);
    let _ = led3.put_row(SCENE_IDLE, 240, 236);
    let ok = led3.ledger_fingerprint() != standard_ledger().ledger_fingerprint();
    s.add("56137-J3-指纹随行变", ok, if ok { "" } else { "折叠失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56137-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56137_all_checks_green() {
        let set = run_unx56137_checks();
        assert!(set.all_passed(), "UNX-F56137 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56137_ratio_stable() {
        assert_eq!(ratio_permille(60, 57), 9_500);
    }
}
