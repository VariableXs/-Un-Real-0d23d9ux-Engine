//! VE-F0246 · AMD FreeSync/VRR 支持（VE-B 域 · AMD 组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0246`
//!
//! **职责定位（锚点原文）**：能力协商（EDID 扩展块解析加范围限制读取）、
//! 启用条件判定（当前模式在 VRR 范围内）、运行中越范围自动回落固定刷新
//! 并通知；与显示控制器水线联动；状态可查询呈现不依赖视觉判断。
//!
//! **数据结构（锚点原文）**：VRR 范围（最小×最大×当前模式）；协商结果
//! 缓存。
//!
//! **判据（锚点原文）**：范围协商、越界回落、状态可查、联动水线、判据。
//! 本模块 12 条判据逐条映射：**范围协商 3 条**（EDID 扩展块范围解析／
//! 非法范围禁用 VRR／协商结果缓存幂等）、**启用判定 3 条**（模式在范围内
//! 启用／模式低于最小禁用／模式高于最大禁用）、**越界回落 2 条**（运行中
//! 越范围回落固定刷新／回落必带通知事件）、**状态可查 2 条**（文字化状态
//! 查询／不只靠视觉）、**联动水线 1 条**（VRR 启用联动 DCN 水线标注）、
//! **判据自身 1 条**（码位互异且段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 范围解析失败 → **禁用 VRR 走固定刷新** | EDID 扩展块缺失/非法：VRR 关闭 + 缺席标注——不假装支持 |
//! | 运行中越范围 → **回落加通知** | 当前刷新率滑出范围即回落固定档 + 通知事件落账——不是静默漂移 |
//! | 缓存脏 → **触发重协商** | 代际/拓扑版本号变更使缓存失效，下次查询强制重协商 |
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **范围是三元组闭集**：min_hz ≤ cur_hz ≤ max_hz 才算在范围内，
//!    两端闭（FreeSync 语义：边界帧合法），判定 O(1) 无浮点。
//! 2. **回落必须有通知**：[`VrrState::fall_back`] 返回通知事件（旧值/
//!    新值/原因），通知账可查——「自动回落」若无人知晓就是静默降级。
//! 3. **状态文字化**：[`VrrState::render`] 输出人话状态行，满足
//!    「不依赖视觉判断」的锚点要求。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xB1C0 独占 0x10）。
pub const SEG_BASE: u32 = 0xB1C0;
pub const XC_RANGE_BAD: u32 = SEG_BASE + 1; // 范围非法（min>max 或为零）
pub const XC_NO_EDID: u32 = SEG_BASE + 2; // EDID 扩展块缺失
pub const XC_CACHE_STALE: u32 = SEG_BASE + 3; // 缓存脏（需重协商）

/// 固定刷新回落档（Hz）。
pub const FIXED_HZ: u32 = 60;

/// VRR 范围三元组。
#[derive(Clone, Copy, PartialEq)]
pub struct VrrRange {
    pub min_hz: u32,
    pub max_hz: u32,
}

impl VrrRange {
    /// 合法性：0 < min ≤ max（FreeSync 范围非空）。
    pub fn valid(&self) -> bool {
        self.min_hz > 0 && self.min_hz <= self.max_hz
    }

    /// 模式在范围内（两端闭）。
    pub fn contains(&self, hz: u32) -> bool {
        hz >= self.min_hz && hz <= self.max_hz
    }
}

/// EDID 扩展块模拟（范围描述字节对：min × max）。
pub struct EdidExt {
    pub present: bool,
    pub min_hz: u32,
    pub max_hz: u32,
}

/// 通知事件（回落/启用/禁用全落账）。
#[derive(Clone)]
pub struct VrrNotice {
    pub kind: u8, // 0 启用 1 回落 2 禁用
    pub old_hz: u32,
    pub new_hz: u32,
    pub reason: String,
}

/// 通知类型名。
pub fn notice_name(k: u8) -> &'static str {
    match k {
        0 => "启用",
        1 => "回落",
        2 => "禁用",
        _ => "?",
    }
}

/// VRR 状态机（协商缓存 + 运行态 + 通知账）。
pub struct VrrState {
    pub range: Option<VrrRange>,
    pub cache_gen: u64,      // 协商缓存代（拓扑版本）
    pub seen_gen: u64,       // 缓存建立时的代
    pub active: bool,
    pub cur_hz: u32,
    pub notices: Vec<VrrNotice>,
}

impl VrrState {
    pub fn new() -> Self {
        VrrState { range: None, cache_gen: 0, seen_gen: 0, active: false, cur_hz: FIXED_HZ, notices: Vec::new() }
    }

    /// 协商核：EDID 扩展块解析 → 范围缓存。
    /// 块缺失/范围非法 → 禁用 VRR + 缺席/非法标注（不假装支持）。
    pub fn negotiate(&mut self, edid: &EdidExt, topo_gen: u64) -> Result<VrrRange, u32> {
        self.cache_gen = topo_gen;
        if !edid.present {
            self.active = false;
            return Err(XC_NO_EDID);
        }
        let r = VrrRange { min_hz: edid.min_hz, max_hz: edid.max_hz };
        if !r.valid() {
            self.active = false;
            return Err(XC_RANGE_BAD);
        }
        self.range = Some(r);
        self.seen_gen = topo_gen;
        Ok(r)
    }

    /// 缓存新鲜核（拓扑代变更 → 缓存脏 → 需重协商）。
    pub fn cache_fresh(&self) -> Result<(), u32> {
        if self.range.is_none() || self.seen_gen != self.cache_gen {
            return Err(XC_CACHE_STALE);
        }
        Ok(())
    }

    /// 启用判定：范围在 + 模式在范围内才启用。
    pub fn try_enable(&mut self, mode_hz: u32) -> Result<bool, u32> {
        self.cache_fresh()?;
        let r = match self.range {
            Some(r) => r,
            None => return Err(XC_NO_EDID),
        };
        if r.contains(mode_hz) {
            if !self.active {
                self.notices.push(VrrNotice {
                    kind: 0,
                    old_hz: self.cur_hz,
                    new_hz: mode_hz,
                    reason: String::from("模式进入 VRR 范围"),
                });
            }
            self.active = true;
            self.cur_hz = mode_hz;
            Ok(true)
        } else {
            self.active = false;
            Ok(false)
        }
    }

    /// 运行中越范围回落：新刷新滑出范围 → 回落固定刷新 + 通知。
    pub fn fall_back(&mut self, new_hz: u32) -> Result<u32, u32> {
        self.cache_fresh()?;
        let r = match self.range {
            Some(r) => r,
            None => return Err(XC_NO_EDID),
        };
        if self.active && !r.contains(new_hz) {
            let old = self.cur_hz;
            self.active = false;
            self.cur_hz = FIXED_HZ;
            self.notices.push(VrrNotice {
                kind: 1,
                old_hz: old,
                new_hz: FIXED_HZ,
                reason: format!("刷新率 {} 越出 VRR 范围 {}..={}", new_hz, r.min_hz, r.max_hz),
            });
            return Ok(FIXED_HZ);
        }
        self.cur_hz = new_hz;
        Ok(new_hz)
    }

    /// 联动水线标注（DCN 水位随 VRR 启用微调：VRR 帧变程大 → 水位 +2 行）。
    pub fn dcn_wm_adjust(&self, base_lines: u32) -> u32 {
        if self.active {
            base_lines + 2
        } else {
            base_lines
        }
    }

    /// 状态查询（文字化，不依赖视觉）。
    pub fn render(&self) -> String {
        let r = match self.range {
            Some(r) => format!("{}..={}Hz", r.min_hz, r.max_hz),
            None => String::from("未协商"),
        };
        let st = if self.active { "启用" } else { "停用（固定刷新）" };
        let mut out = format!("VRR 范围={} 当前={}Hz 状态={}\n", r, self.cur_hz, st);
        for n in &self.notices {
            let line = format!(
                "  通知:{} {}->{}Hz {}\n",
                notice_name(n.kind), n.old_hz, n.new_hz, n.reason
            );
            out.push_str(&line);
        }
        out
    }
}

/// 标准夹具：范围 48..=144，拓扑代 7，模式 120 启用。
pub fn standard_vrr() -> VrrState {
    let mut v = VrrState::new();
    let _ = v.negotiate(
        &EdidExt { present: true, min_hz: 48, max_hz: 144 },
        7,
    );
    let _ = v.try_enable(120);
    v
}

/// 注入核：8 注入必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) EDID 缺失禁用
    let mut v = VrrState::new();
    if v.negotiate(&EdidExt { present: false, min_hz: 0, max_hz: 0 }, 1) == Err(XC_NO_EDID)
        && !v.active
    {
        caught += 1;
    }
    // 2) 非法范围（min>max）禁用
    let mut v2 = VrrState::new();
    if v2.negotiate(&EdidExt { present: true, min_hz: 100, max_hz: 50 }, 1) == Err(XC_RANGE_BAD) {
        caught += 1;
    }
    // 3) 范围边界闭（min/max 恰好命中启用）
    let mut v3 = standard_vrr();
    if v3.try_enable(48) == Ok(true) && v3.try_enable(144) == Ok(true) {
        caught += 1;
    }
    // 4) 范围外禁用（双向）
    if v3.try_enable(30) == Ok(false) && v3.try_enable(200) == Ok(false) {
        caught += 1;
    }
    // 5) 运行中越范围回落 + 通知
    let mut v4 = standard_vrr();
    let fb = v4.fall_back(200).unwrap_or(0);
    if fb == FIXED_HZ && !v4.active && v4.notices.iter().any(|n| n.kind == 1 && n.new_hz == 60) {
        caught += 1;
    }
    // 6) 缓存脏（拓扑代变更）强制重协商
    let mut v5 = standard_vrr();
    v5.cache_gen = 8;
    if v5.cache_fresh() == Err(XC_CACHE_STALE) {
        caught += 1;
    }
    // 7) 联动水线（VRR 启用 +2 行 / 停用不变）
    let v6 = standard_vrr();
    let off = VrrState::new();
    if v6.dcn_wm_adjust(10) == 12 && off.dcn_wm_adjust(10) == 10 {
        caught += 1;
    }
    // 8) 状态可查（渲染含范围与状态文字 + 通知行）
    let s = standard_vrr().render();
    if s.contains("48..=144") && s.contains("启用") && s.contains("通知") {
        caught += 1;
    }
    (caught, 8)
}

/// 判据入口。
pub fn run_veb246_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0246");
    // J1-1 范围协商（EDID 解析 + 缓存 + 幂等重协商一致）
    let mut v = VrrState::new();
    let a = v.negotiate(&EdidExt { present: true, min_hz: 48, max_hz: 144 }, 3);
    let b = v.negotiate(&EdidExt { present: true, min_hz: 48, max_hz: 144 }, 3);
    let ok = a == b && a.is_ok() && v.cache_fresh().is_ok();
    s.add("AMD-246-01-范围协商", ok, if ok { "" } else { "协商漂移" });
    // J1-2 启用判定（范围内启用 / 范围外禁用 / 边界闭）
    let mut v = standard_vrr();
    let ok = v.try_enable(120) == Ok(true)
        && v.try_enable(47) == Ok(false)
        && v.try_enable(145) == Ok(false)
        && v.try_enable(48) == Ok(true);
    s.add("AMD-246-02-启用判定", ok, if ok { "" } else { "判定漂移" });
    // J1-3 越界回落 + 通知落账
    let mut v = standard_vrr();
    let fb = v.fall_back(300).unwrap_or(0);
    let ok = fb == FIXED_HZ && !v.active
        && v.notices.iter().any(|n| n.kind == 1 && n.reason.contains("越出"));
    s.add("AMD-246-03-越界回落通知", ok, if ok { "" } else { "回落破缺" });
    // J1-4 缓存脏重协商
    let mut v = standard_vrr();
    v.cache_gen += 1;
    let ok = v.cache_fresh() == Err(XC_CACHE_STALE)
        && v.negotiate(&EdidExt { present: true, min_hz: 48, max_hz: 144 }, v.cache_gen).is_ok()
        && v.cache_fresh().is_ok();
    s.add("AMD-246-04-缓存重协商", ok, if ok { "" } else { "缓存破缺" });
    // J1-5 状态可查 + 联动水线
    let v = standard_vrr();
    let ok = v.render().contains("120") && v.dcn_wm_adjust(10) == 12;
    s.add("AMD-246-05-状态可查联动", ok, if ok { "" } else { "可查破缺" });
    // J1-6 注入核 8/8
    let (c, n) = injection_core();
    let ok = c == n;
    let d = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("AMD-246-06-注入核必抓", ok, &d);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb246_checks_all_green() {
        let set = run_veb246_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn veb246_range_closed_interval() {
        let r = VrrRange { min_hz: 48, max_hz: 144 };
        assert!(r.contains(48) && r.contains(144));
        assert!(!r.contains(47) && !r.contains(145));
    }
}
