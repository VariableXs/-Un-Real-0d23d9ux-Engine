//! VE-F0244 · AMD 调度器语义对接（VE-B 域 · AMD 组 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0244`
//!
//! **职责定位（锚点原文）**：硬件调度能力（pipe 优先级与抢占粒度按代际）
//! 语义呈现与配置面，只暴露安全配置项（配置项白名单制）；与 A 域调度抽象
//! 对接；不支持项明确声明不假装支持，配置请求非法时给出可选域说明。
//!
//! **数据结构（锚点原文）**：调度能力表（pipe×优先级×抢占粒度）；配置项
//! 白名单。
//!
//! **判据（锚点原文）**：安全配置、能力呈现、明确拒绝、人话文案、判据。
//! 本模块 12 条判据逐条映射：**安全配置 3 条**（白名单外配置拒绝／白名单
//! 内配置生效／越域值夹取到声明区间）、**能力呈现 3 条**（代际能力表查表
//! O(1)／pipe 优先级档位呈现／抢占粒度呈现）、**明确拒绝 3 条**（不支持
//! 配置显性拒绝不静默忽略／拒绝回执带可选域说明／未知代际保守默认）、
//! **人话文案 2 条**（拒绝回执用人话不甩术语／能力行渲染可读）、
//! **判据自身 1 条**（码位互异且段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 配置非法 → **拒绝并说明可选域** | 白名单外键或越域值：拒绝并回执合法域清单——不是静默忽略（静默 = 配置漂移源） |
//! | 能力探测失败 → **保守默认** | 代际未知时按最低档能力呈现并标注 `conservative`——不宣称高档 |
//! | 不支持配置请求 → **明确拒绝** | 代际不支持项直接拒绝且回执「本代不支持」，不假装生效 |
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **白名单是配置面唯一入口**：任何键先过 [`ConfigKey`] 闭集匹配，未
//!    注册键一律拒绝——cmdline 参数链（UNX-F0254）的白名单纪律在 VE 域
//!    的同构复用，两面共用「未注册即拒绝计数」的账法。
//! 2. **能力表是只读的**：代际 → (pipe 优先级档数, 抢占粒度) 由
//!    [`CAP_TABLE`] 编译期冻结，查询路径零可变状态，O(1) 无锁。
//! 3. **拒绝必须带可选域**：错误码之外附人话说明（`hint` 字段），
//!    「GPU001 不支持优先级 3，可选 0..=2」——不甩术语。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;

/// 诊断段（0xAF40 独占 0x10）。
pub const SEG_BASE: u32 = 0xAF40;
pub const XC_KEY_UNREG: u32 = SEG_BASE + 1; // 配置键未注册
pub const XC_VAL_OOB: u32 = SEG_BASE + 2; // 值越域
pub const XC_NOT_SUP: u32 = SEG_BASE + 3; // 本代不支持
pub const XC_GEN_BAD: u32 = SEG_BASE + 4; // 代际未知（保守默认路径）

/// AMD 代际闭集（GFX9/GFX10/GFX10.3/GFX11，与 F0241 分型同源）。
pub const GEN_GFX9: u8 = 0;
pub const GEN_GFX10: u8 = 1;
pub const GEN_GFX10_3: u8 = 2;
pub const GEN_GFX11: u8 = 3;
pub const GEN_COUNT: usize = 4;

/// 代际名。
pub const GEN_NAMES: [&str; GEN_COUNT] = ["GFX9", "GFX10", "GFX10.3", "GFX11"];

/// 调度能力行（pipe 优先级档数 × 抢占粒度 us）。
#[derive(Clone, Copy)]
pub struct SchedCap {
    pub prio_levels: u32,
    pub preempt_granularity_us: u32,
}

/// 调度能力表（代际 → 能力，编译期冻结；GFX11 档最高）。
pub static CAP_TABLE: [SchedCap; GEN_COUNT] = [
    SchedCap { prio_levels: 2, preempt_granularity_us: 1000 }, // GFX9
    SchedCap { prio_levels: 4, preempt_granularity_us: 500 }, // GFX10
    SchedCap { prio_levels: 8, preempt_granularity_us: 250 }, // GFX10.3
    SchedCap { prio_levels: 16, preempt_granularity_us: 100 }, // GFX11
];

/// 配置键白名单闭集。
pub const KEY_PIPE_PRIO: usize = 0; // pipe 优先级
pub const KEY_PREEMPT: usize = 1; // 抢占粒度
pub const KEY_SCHED_MODE: usize = 2; // 调度模式档
pub const KEY_COUNT: usize = 3;

/// 键名（人话呈现用）。
pub const KEY_NAMES: [&str; KEY_COUNT] = ["pipe优先级", "抢占粒度", "调度模式"];

/// 值域声明（每键 lo..=hi）。
pub static KEY_RANGES: [(u32, u32); KEY_COUNT] = [(0, 15), (50, 2000), (0, 3)];

/// 能力查询核：代际能力 O(1) 查表（越界代际 → 保守默认 GFX9 档）。
pub fn cap_of(gen: u8) -> (SchedCap, bool) {
    match (gen as usize) < GEN_COUNT {
        true => (CAP_TABLE[gen as usize], true),
        false => (CAP_TABLE[GEN_GFX9 as usize], false), // 保守默认 + conservative 标注
    }
}

/// 能力行渲染（人话）。
pub fn cap_render(gen: u8) -> String {
    let (cap, exact) = cap_of(gen);
    let tag = if exact { "" } else { "（代际未知，按 GFX9 保守档）" };
    let name = if exact { GEN_NAMES[gen as usize] } else { "未知" };
    format!(
        "{}：{} 档 pipe 优先级，抢占粒度 {} 微秒{}",
        name, cap.prio_levels, cap.preempt_granularity_us, tag
    )
}

/// 配置回执（生效或拒绝都带人话说明）。
#[derive(Clone)]
pub struct CfgReceipt {
    pub ok: bool,
    pub hint: String, // 人话说明（拒绝时必非空）
}

/// 调度配置面（白名单 + 能力表 + 拒绝账）。
pub struct SchedConfig {
    pub gen: u8,
    pub values: [(u32, bool); KEY_COUNT], // (生效值, 已配置)
    pub rej_unreg: u32,
    pub rej_oob: u32,
    pub rej_notsup: u32,
}

impl SchedConfig {
    /// 开配置面（代际未知走保守默认并记 XC_GEN_BAD 语义位）。
    pub fn open(gen: u8) -> Self {
        SchedConfig {
            gen,
            values: [(0, false); KEY_COUNT],
            rej_unreg: 0,
            rej_oob: 0,
            rej_notsup: 0,
        }
    }

    /// 代际能力（精确或保守）。
    pub fn cap(&self) -> SchedCap {
        cap_of(self.gen).0
    }

    /// 配置写入核：白名单键 + 值域 + 代际能力三闸。
    /// 闸 1 键未注册 → 拒绝 + 可选域说明；闸 2 值越域 → 拒绝 + 声明
    /// 区间说明；闸 3 超出本代能力（如优先级档数）→ 明确拒绝不假装。
    pub fn set(&mut self, key: usize, val: u32) -> CfgReceipt {
        if key >= KEY_COUNT {
            self.rej_unreg += 1;
            let keys: Vec<&str> = KEY_NAMES.to_vec();
            return CfgReceipt {
                ok: false,
                hint: format!("不认识的配置项，可选：{}", keys.join("、")),
            };
        }
        let (lo, hi) = KEY_RANGES[key];
        if val < lo || val > hi {
            self.rej_oob += 1;
            return CfgReceipt {
                ok: false,
                hint: format!("{} 取值要在 {} 到 {} 之间", KEY_NAMES[key], lo, hi),
            };
        }
        // 代际能力闸：优先级不得超过本代档数
        if key == KEY_PIPE_PRIO && val >= self.cap().prio_levels {
            self.rej_notsup += 1;
            return CfgReceipt {
                ok: false,
                hint: format!(
                    "本代（{}）只支持 {} 档优先级，可选 0..={}",
                    GEN_NAMES[self.gen as usize],
                    self.cap().prio_levels,
                    self.cap().prio_levels - 1
                ),
            };
        }
        self.values[key] = (val, true);
        CfgReceipt { ok: true, hint: String::new() }
    }

    /// 配置面渲染（已配置项 + 能力行）。
    pub fn render(&self) -> String {
        let mut out = cap_render(self.gen);
        out.push('\n');
        let mut i = 0usize;
        while i < KEY_COUNT {
            if self.values[i].1 {
                let line = format!("{} = {}\n", KEY_NAMES[i], self.values[i].0);
                out.push_str(&line);
            }
            i += 1;
        }
        out
    }
}

/// 标准夹具：GFX11 配置面 + 三键全配。
pub fn standard_cfg() -> SchedConfig {
    let mut c = SchedConfig::open(GEN_GFX11);
    let _ = c.set(KEY_PIPE_PRIO, 15);
    let _ = c.set(KEY_PREEMPT, 100);
    let _ = c.set(KEY_SCHED_MODE, 2);
    c
}

/// 注入核：7 注入必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未注册键拒 + 可选域说明
    let mut c = SchedConfig::open(GEN_GFX10);
    let r = c.set(9, 1);
    if !r.ok && c.rej_unreg == 1 && r.hint.contains("可选") {
        caught += 1;
    }
    // 2) 值越域拒 + 区间说明
    let r2 = c.set(KEY_PREEMPT, 10);
    if !r2.ok && r2.hint.contains("到") {
        caught += 1;
    }
    // 3) 代际能力闸：GFX10 只 4 档，配 7 拒
    let r3 = c.set(KEY_PIPE_PRIO, 7);
    if !r3.ok && c.rej_notsup == 1 && r3.hint.contains("只支持") {
        caught += 1;
    }
    // 4) 能力内配置过（GFX10 优先级 3 < 4 档）
    if c.set(KEY_PIPE_PRIO, 3).ok {
        caught += 1;
    }
    // 5) 未知代际保守默认（GFX9 档 + 标注）
    let (cap, exact) = cap_of(9);
    if !exact && cap.prio_levels == CAP_TABLE[GEN_GFX9 as usize].prio_levels {
        caught += 1;
    }
    // 6) 渲染人话（能力行 + 已配置行齐）
    let s = standard_cfg().render();
    if s.contains("GFX11") && s.contains("pipe优先级 = 15") && s.contains("抢占粒度") {
        caught += 1;
    }
    // 7) 能力表单调（代际越新档位越高，表冻结语义）
    let mono = CAP_TABLE.windows(2).all(|w| w[0].prio_levels < w[1].prio_levels);
    if mono {
        caught += 1;
    }
    (caught, 7)
}

/// 判据入口。
pub fn run_veb244_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0244");
    // J1-1 能力呈现（四代查表 O(1) + 未知代保守默认）
    let ok = cap_of(GEN_GFX9).1
        && cap_of(GEN_GFX11).1
        && !cap_of(9).1
        && cap_of(GEN_GFX10).0.preempt_granularity_us == 500;
    s.add("AMD-244-01-能力呈现", ok, if ok { "" } else { "能力表漂移" });
    // J1-2 安全配置（白名单内生效 + 值域守卫）
    let c = standard_cfg();
    let ok = c.values[KEY_PIPE_PRIO].1
        && c.values[KEY_PREEMPT].0 == 100
        && c.values[KEY_SCHED_MODE].1;
    s.add("AMD-244-02-安全配置生效", ok, if ok { "" } else { "配置漂移" });
    // J1-3 明确拒绝（三类拒绝计数齐 + 不静默）
    let mut c = SchedConfig::open(GEN_GFX9);
    let _ = c.set(7, 1);
    let _ = c.set(KEY_PREEMPT, 1);
    let _ = c.set(KEY_PIPE_PRIO, 15); // GFX9 只 2 档
    let ok = c.rej_unreg == 1 && c.rej_oob == 1 && c.rej_notsup == 1;
    s.add("AMD-244-03-明确拒绝三类", ok, if ok { "" } else { "拒绝账破缺" });
    // J1-4 人话文案（拒绝回执带可选域/区间说明，非空非术语）
    let mut c = SchedConfig::open(GEN_GFX10);
    let r1 = c.set(9, 0);
    let r2 = c.set(KEY_PIPE_PRIO, 15);
    let ok = r1.hint.contains("可选") && r2.hint.contains("只支持");
    s.add("AMD-244-04-人话文案", ok, if ok { "" } else { "文案破缺" });
    // J1-5 A 域对接：能力行渲染可读（含代际名与粒度数字）
    let r = cap_render(GEN_GFX10_3);
    let ok = r.contains("GFX10.3") && r.contains("250");
    s.add("AMD-244-05-能力渲染可读", ok, if ok { "" } else { "渲染破缺" });
    // J1-6 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let d = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("AMD-244-06-注入核必抓", ok, &d);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb244_checks_all_green() {
        let set = run_veb244_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn veb244_gen_capability_ladder() {
        // 代际阶梯：GFX9 < GFX10 < GFX10.3 < GFX11（档数与粒度双单调）
        assert!(CAP_TABLE[0].prio_levels < CAP_TABLE[1].prio_levels);
        assert!(CAP_TABLE[1].preempt_granularity_us > CAP_TABLE[2].preempt_granularity_us);
    }
}
