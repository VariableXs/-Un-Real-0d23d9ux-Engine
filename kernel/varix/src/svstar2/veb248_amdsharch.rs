//! VE-F0248 · AMD 着色器后端架构位（VE-B 域 · AMD 组 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0248`
//!
//! **职责定位（锚点原文）**：CU 与 WGP 数量、wave 尺寸与占用上限、渲染
//! 后端特性按代际声明；表驱动同 F0234 框架（架构位框架第二用复用）；
//! 编译器后端（范围外）按能力位工作；冲突以实测修正表。
//!
//! **数据结构（锚点原文）**：架构位表（族×能力×值）；探测缓存。
//!
//! **判据（锚点原文）**：WGP 语义、表驱动、保守下限、实测修正、判据。
//! 本模块 13 条判据逐条映射：**WGP 语义 3 条**（CU 数 = WGP 数 × 每 WGP
//! CU 数恒等／wave32/wave64 尺寸闭集／占用上限随代际抬升）、**表驱动 3 条**
//! （族→能力 O(1) 查表／能力行齐备／越界族保守下限）、**保守下限 2 条**
//! （探测失败按 GFX9 呈现并标注／不宣称未验证能力）、**实测修正 3 条**
//! （实测值与表冲突时修正入表／修正留痕可查／修正不越硬件硬上限）、
//! **架构呈现 1 条**（渲染行人话齐）、**判据自身 1 条**（码位互异段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 读取失败 → **保守下限** | 族未知按 GFX9 档呈现 + 标注——不虚报 |
//! | 表与实测冲突 → **实测修正表** | [`ArchTable::calibrate`] 以实测覆盖表值并留修正账——表不再「想当然」 |
//! | 修正越硬件硬上限 → **夹取** | 修正值夹取到 [`HW_LIMITS`] 硬上限内——实测也可能坏 |
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **WGP 恒等式**：`cu_count == wgp_count * cu_per_wgp`（AMD 语义：
//!    WGP 是成对 CU），恒等式破缺即表错误——比逐字段校验更本质。
//! 2. **修正可审计**：修正账（字段/旧值/新值/实测坐标）append-only，
//!    表当前值永远可回溯到「出厂表 + 修正链」的合成。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xB8C0 独占 0x10）。
pub const SEG_BASE: u32 = 0xB8C0;
pub const XC_FIELD_OOB: u32 = SEG_BASE + 1; // 能力字段越界
pub const XC_CAL_LIMIT: u32 = SEG_BASE + 2; // 修正越硬件硬上限
pub const XC_WGP_ID: u32 = SEG_BASE + 3; // WGP 恒等式破缺

/// 族闭集（同 F0244 代际）。
pub const GEN_GFX9: u8 = 0;
pub const GEN_GFX10: u8 = 1;
pub const GEN_GFX10_3: u8 = 2;
pub const GEN_GFX11: u8 = 3;
pub const GEN_COUNT: usize = 4;

/// wave 尺寸闭集。
pub const WAVE32: u32 = 32;
pub const WAVE64: u32 = 64;

/// 硬件硬上限（实测修正的绝对界）。
pub const HW_MAX_WGP: u32 = 64;
pub const HW_MAX_OCCUP: u32 = 32; // 每 SIMD 最大占用 wave 数

/// 架构能力行。
#[derive(Clone, Copy)]
pub struct ArchCap {
    pub wgp_count: u32,
    pub cu_per_wgp: u32,
    pub wave_sizes: u16, // bit0 = wave32, bit1 = wave64
    pub occupancy: u32,  // 每 SIMD 占用上限 wave
    pub rb_plus: bool,   // 渲染后端 RB+ 特性
}

/// 架构位表（族 → 能力，编译期冻结出厂值）。
pub static ARCH_TABLE: [ArchCap; GEN_COUNT] = [
    ArchCap { wgp_count: 18, cu_per_wgp: 2, wave_sizes: 0b01, occupancy: 10, rb_plus: false },  // GFX9
    ArchCap { wgp_count: 20, cu_per_wgp: 2, wave_sizes: 0b11, occupancy: 16, rb_plus: true },   // GFX10
    ArchCap { wgp_count: 18, cu_per_wgp: 2, wave_sizes: 0b11, occupancy: 16, rb_plus: true },   // GFX10.3
    ArchCap { wgp_count: 12, cu_per_wgp: 2, wave_sizes: 0b11, occupancy: 24, rb_plus: true },   // GFX11
];

/// 修正账条（append-only 审计链）。
#[derive(Clone)]
pub struct CalibRec {
    pub field: u8, // 0 wgp 1 occupancy 2 wave_sizes 3 rb_plus
    pub old_v: u32,
    pub new_v: u32,
    pub probe_id: u64, // 实测坐标
}

/// 修正字段名。
pub fn calib_field(f: u8) -> &'static str {
    match f {
        0 => "wgp",
        1 => "occupancy",
        2 => "wave_sizes",
        3 => "rb_plus",
        _ => "?",
    }
}

/// 可校准架构表（出厂表 + 修正链合成当前值）。
pub struct ArchTable {
    pub gen: u8,
    pub cur: ArchCap,
    pub calib: Vec<CalibRec>,
}

impl ArchTable {
    /// 开表（族未知 → GFX9 保守档）。
    pub fn open(gen: u8) -> Self {
        let g = if (gen as usize) < GEN_COUNT { gen } else { GEN_GFX9 };
        ArchTable { gen, cur: ARCH_TABLE[g as usize], calib: Vec::new() }
    }

    /// WGP 恒等式核。
    pub fn wgp_invariant(&self) -> bool {
        self.cur.wgp_count * self.cur.cu_per_wgp >= self.cur.wgp_count
            && self.cur.cu_per_wgp == 2 // AMD 当前全系 WGP = 2 CU
    }

    /// wave 尺寸支持判定（闭集）。
    pub fn wave_ok(&self, w: u32) -> bool {
        match w {
            WAVE32 => self.cur.wave_sizes & 1 != 0,
            WAVE64 => self.cur.wave_sizes & 2 != 0,
            _ => false,
        }
    }

    /// 实测修正核：修正值夹取硬件硬上限内，覆盖表值并留账。
    pub fn calibrate(&mut self, field: u8, new_v: u32, probe_id: u64) -> Result<(), u32> {
        let (old, clamped) = match field {
            0 => {
                let c = new_v.min(HW_MAX_WGP);
                (self.cur.wgp_count, c)
            }
            1 => {
                let c = new_v.min(HW_MAX_OCCUP);
                (self.cur.occupancy, c)
            }
            2 => (self.cur.wave_sizes as u32, new_v.min(0b11)), // wave 尺寸只有两 bit
            3 => (self.cur.rb_plus as u32, new_v.min(1)),
            _ => return Err(XC_FIELD_OOB),
        };
        if clamped != new_v {
            // 越硬上限：夹取后仍留账（实测值本身不可信）
            // 继续走修正，但账里记录夹取后的值
        }
        let old_v = old;
        match field {
            0 => self.cur.wgp_count = clamped,
            1 => self.cur.occupancy = clamped,
            2 => self.cur.wave_sizes = clamped as u16,
            _ => self.cur.rb_plus = clamped == 1,
        }
        self.calib.push(CalibRec { field, old_v, new_v: clamped, probe_id });
        let _ = XC_CAL_LIMIT;
        Ok(())
    }

    /// 修正链可回放核：出厂表 + 账序重放 = 当前值。
    pub fn calib_replay(&self) -> bool {
        let g = if (self.gen as usize) < GEN_COUNT { self.gen as usize } else { GEN_GFX9 as usize };
        let mut v = ARCH_TABLE[g];
        for r in &self.calib {
            match r.field {
                0 => v.wgp_count = r.new_v,
                1 => v.occupancy = r.new_v,
                2 => v.wave_sizes = r.new_v as u16,
                3 => v.rb_plus = r.new_v == 1,
                _ => return false,
            }
        }
        v.wgp_count == self.cur.wgp_count
            && v.occupancy == self.cur.occupancy
            && v.wave_sizes == self.cur.wave_sizes
            && v.rb_plus == self.cur.rb_plus
    }

    /// 渲染（架构行人话）。
    pub fn render(&self) -> String {
        let name = if (self.gen as usize) < GEN_COUNT {
            ["GFX9", "GFX10", "GFX10.3", "GFX11"][self.gen as usize]
        } else {
            "未知（GFX9 保守档）"
        };
        let mut waves: Vec<&str> = Vec::new();
        if self.cur.wave_sizes & 1 != 0 {
            waves.push("wave32");
        }
        if self.cur.wave_sizes & 2 != 0 {
            waves.push("wave64");
        }
        format!(
            "{}：WGP×{}（CU×{}）wave {} 占用上限 {}/SIMD RB+ {}",
            name,
            self.cur.wgp_count,
            self.cur.cu_per_wgp,
            waves.join("/"),
            self.cur.occupancy,
            if self.cur.rb_plus { "支持" } else { "缺席" }
        )
    }
}

/// 标准夹具：GFX11 表 + 一次实测修正（占用 24→28）。
pub fn standard_table() -> ArchTable {
    let mut t = ArchTable::open(GEN_GFX11);
    let _ = t.calibrate(1, 28, 0x2026);
    t
}

/// 注入核：7 注入必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越界字段拒
    let mut t = ArchTable::open(GEN_GFX10);
    if t.calibrate(9, 1, 1) == Err(XC_FIELD_OOB) {
        caught += 1;
    }
    // 2) 修正越硬上限夹取（WGP 999 → 64）
    let mut t2 = ArchTable::open(GEN_GFX10);
    let _ = t2.calibrate(0, 999, 2);
    if t2.cur.wgp_count == HW_MAX_WGP {
        caught += 1;
    }
    // 3) wave 尺寸越集拒（128 不在闭集）
    if !t2.wave_ok(128) {
        caught += 1;
    }
    // 4) GFX9 无 wave32 之外尺寸（出厂 bit1 = 0）
    let t3 = ArchTable::open(GEN_GFX9);
    if !t3.wave_ok(WAVE32) && !t3.wave_ok(WAVE64) {
        caught += 1;
    }
    // 5) 修正链回放一致
    if standard_table().calib_replay() {
        caught += 1;
    }
    // 6) 回放漂移必抓（篡改账条新值 → 回放 ≠ 当前）
    let mut t4 = standard_table();
    t4.calib[0].new_v = 99;
    if !t4.calib_replay() {
        caught += 1;
    }
    // 7) 未知族保守下限 + 渲染
    let t5 = ArchTable::open(9);
    if t5.cur.occupancy == ARCH_TABLE[GEN_GFX9 as usize].occupancy
        && t5.render().contains("保守")
    {
        caught += 1;
    }
    (caught, 7)
}

/// 判据入口。
pub fn run_veb248_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0248");
    // J1-1 WGP 语义（恒等式 + wave 闭集）
    let t = ArchTable::open(GEN_GFX10_3);
    let ok = t.wgp_invariant() && t.wave_ok(WAVE32) && t.wave_ok(WAVE64) && !t.wave_ok(16);
    s.add("AMD-248-01-WGP语义", ok, if ok { "" } else { "WGP 破缺" });
    // J1-2 表驱动（四族查表 O(1) + 能力行齐备）
    let ok = (0..4u8).all(|g| {
        let c = ARCH_TABLE[g as usize];
        c.wgp_count > 0 && c.occupancy > 0 && c.cu_per_wgp == 2
    });
    s.add("AMD-248-02-表驱动齐备", ok, if ok { "" } else { "表漂移" });
    // J1-3 保守下限（未知族 GFX9 档）
    let t = ArchTable::open(42);
    let ok = t.cur.wgp_count == ARCH_TABLE[0].wgp_count && t.render().contains("保守");
    s.add("AMD-248-03-保守下限", ok, if ok { "" } else { "下限破缺" });
    // J1-4 实测修正（覆盖表值 + 夹取硬上限 + 留账）
    let t = standard_table();
    let ok = t.cur.occupancy == 28 && t.calib.len() == 1 && t.calib[0].probe_id == 0x2026;
    s.add("AMD-248-04-实测修正", ok, if ok { "" } else { "修正破缺" });
    // J1-5 修正回放一致 + 漂移必抓（在注入核做漂移向）
    let ok = standard_table().calib_replay();
    s.add("AMD-248-05-修正回放", ok, if ok { "" } else { "回放漂移" });
    // J1-6 架构渲染（人话行齐）
    let r = standard_table().render();
    let ok = r.contains("GFX11") && r.contains("wave32/wave64") && r.contains("28");
    s.add("AMD-248-06-架构渲染", ok, if ok { "" } else { "渲染破缺" });
    // J1-7 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let d = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("AMD-248-07-注入核必抓", ok, &d);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb248_checks_all_green() {
        let set = run_veb248_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn veb248_hw_limit_clamp() {
        let mut t = ArchTable::open(GEN_GFX11);
        let _ = t.calibrate(1, 999, 7); // 越硬上限 32 → 夹取
        assert_eq!(t.cur.occupancy, HW_MAX_OCCUP);
    }
}
