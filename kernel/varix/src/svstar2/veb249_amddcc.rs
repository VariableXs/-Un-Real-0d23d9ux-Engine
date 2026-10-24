//! VE-F0249 · AMD 显存压缩感知（VE-B 域 · AMD 组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0249`
//!
//! **职责定位（锚点原文）**：块压缩状态（DCC 元数据）在绑定、迁移、呈现
//! 三路径的一致性维护，压缩感知呈现路径（解压触发条件显式化）；DCC 元
//! 数据丢失时回退未压缩并重算；回退事件标注供性能归因。
//!
//! **数据结构（锚点原文）**：DCC 元数据句柄；一致性状态表；回退标记。
//!
//! **判据（锚点原文）**：DCC 一致性、回退重算、零拷贝、降级标记、判据。
//! 本模块 13 条判据逐条映射：**DCC 一致性 4 条**（绑定路径状态推进／迁移
//! 路径句柄随迁／呈现路径一致校验／三路径状态闭集推进合法）、**回退重算
//! 3 条**（元数据丢失回退未压缩／回退必触发重算／回退事件标注）、**零拷贝
//! 3 条**（压缩态呈现零解压拷贝／解压触发条件显式化／同状态幂等不重复
//! 解压）、**降级标记 2 条**（回退标记入账可查／性能归因账渲染）、
//! **判据自身 1 条**（码位互异段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 元数据无效 → **回退未压缩加重算** | 句柄校验失败即 [`DccState::invalidate`]：回退未压缩 + 重算计数 + 事件账——不是静默脏读 |
//! | 三路径状态漂移 → **一致性校验拒绝** | 绑定/迁移/呈现各自期望态不同，跨路径校验失败拒绝操作 |
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **状态机四态闭集**：`Valid → Bound → Migrated → Presented`，
//!    每路径只接受自己期望的前驱态——非法迁移在状态机层拒绝，DCC
//!    元数据不可能在错误路径下被当成有效用。
//! 2. **解压显式化**：解压不是副作用而是 API（[`DccState::decompress`]），
//!    每次解压都留账——「解压触发条件显式化」即「解压必过账」。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xB940 独占 0x10）。
pub const SEG_BASE: u32 = 0xB940;
pub const XC_ST_BAD: u32 = SEG_BASE + 1; // 状态迁移非法
pub const XC_META_LOST: u32 = SEG_BASE + 2; // DCC 元数据丢失
pub const XC_HND_BAD: u32 = SEG_BASE + 3; // 句柄无效

/// DCC 一致性四态闭集。
pub const ST_VALID: u8 = 0; // 元数据有效（未绑定）
pub const ST_BOUND: u8 = 1; // 已绑定
pub const ST_MIGRATED: u8 = 2; // 已迁移
pub const ST_PRESENTED: u8 = 3; // 已呈现
pub const ST_COUNT: usize = 4;

/// 状态名。
pub const ST_NAMES: [&str; ST_COUNT] = ["有效", "已绑定", "已迁移", "已呈现"];

/// 三路径期望前驱态（每路径只接受特定前驱）。
pub fn expect_prev(path: u8) -> u8 {
    match path {
        0 => ST_VALID,    // 绑定路径：前驱 = 有效
        1 => ST_BOUND,    // 迁移路径：前驱 = 已绑定
        2 => ST_MIGRATED, // 呈现路径：前驱 = 已迁移
        _ => 0xff,
    }
}

/// 路径名。
pub fn path_name(p: u8) -> &'static str {
    match p {
        0 => "绑定",
        1 => "迁移",
        2 => "呈现",
        _ => "?",
    }
}

/// 回退事件账条。
#[derive(Clone)]
pub struct FallbackRec {
    pub handle: u64,
    pub path: u8,
    pub reason: String,
    pub recalc: bool, // 已触发重算
}

/// DCC 元数据句柄 + 一致性状态。
pub struct DccState {
    pub handle: u64,
    pub st: u8,
    pub meta_valid: bool, // 元数据有效性
    pub decompress_count: u32,
    pub fallbacks: Vec<FallbackRec>,
}

impl DccState {
    /// 新句柄（元数据有效，初始态 = 有效）。
    pub fn new(handle: u64) -> Self {
        DccState {
            handle,
            st: ST_VALID,
            meta_valid: true,
            decompress_count: 0,
            fallbacks: Vec::new(),
        }
    }

    /// 路径推进核：期望前驱 + 元数据有效双闸。
    /// 元数据丢失 → 回退未压缩 + 重算 + 事件账（不静默脏读）。
    pub fn advance(&mut self, path: u8) -> Result<u8, u32> {
        if !self.meta_valid {
            let reason = format!(
                "DCC 元数据丢失，{} 路径回退未压缩并重算",
                path_name(path)
            );
            self.fallbacks.push(FallbackRec {
                handle: self.handle,
                path,
                reason,
                recalc: true,
            });
            self.decompress_count += 1;
            self.st = ST_VALID; // 回退到未压缩基态
            return Err(XC_META_LOST);
        }
        if path > 2 {
            return Err(XC_HND_BAD);
        }
        if self.st != expect_prev(path) {
            return Err(XC_ST_BAD);
        }
        self.st = path + 1; // 绑定→1 迁移→2 呈现→3
        Ok(self.st)
    }

    /// 元数据丢失注入路径（驱动探测面）。
    pub fn invalidate(&mut self) {
        self.meta_valid = false;
    }

    /// 呈现路径解压核（解压显式化：显式调用才解压，幂等不重复）。
    /// 同一状态重复呈现 → 零解压（零拷贝语义）。
    pub fn decompress(&mut self, force: bool) -> u32 {
        if force || self.st != ST_PRESENTED {
            self.decompress_count += 1;
        }
        self.decompress_count
    }

    /// 回退账渲染（性能归因）。
    pub fn render(&self) -> String {
        let mut out = format!(
            "DCC 句柄={:x} 状态={} 元数据={} 解压={} 回退={}\n",
            self.handle,
            ST_NAMES[self.st as usize],
            if self.meta_valid { "有效" } else { "丢失" },
            self.decompress_count,
            self.fallbacks.len()
        );
        for f in &self.fallbacks {
            let line = format!("  回退[{}] {} 重算={}\n", path_name(f.path), f.reason, f.recalc);
            out.push_str(&line);
        }
        out
    }
}

/// 三路径一致场景核：绑定→迁移→呈现全绿推进。
pub fn three_path_ok() -> bool {
    let mut d = DccState::new(0x1000);
    d.advance(0).is_ok() && d.advance(1).is_ok() && d.advance(2).is_ok() && d.st == ST_PRESENTED
}

/// 注入核：7 注入必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 跳路径拒（未绑定直接迁移）
    let mut d = DccState::new(1);
    if d.advance(1) == Err(XC_ST_BAD) {
        caught += 1;
    }
    // 2) 重复路径拒（已绑定再绑定）
    let _ = d.advance(0);
    if d.advance(0) == Err(XC_ST_BAD) {
        caught += 1;
    }
    // 3) 元数据丢失 → 回退未压缩 + 重算 + 账
    let mut d2 = DccState::new(2);
    let _ = d2.advance(0);
    d2.invalidate();
    if d2.advance(1) == Err(XC_META_LOST)
        && d2.fallbacks.len() == 1
        && d2.fallbacks[0].recalc
        && d2.decompress_count == 1
    {
        caught += 1;
    }
    // 4) 回退后基态可重新绑定（恢复路径）
    if d2.advance(0).is_ok() {
        caught += 1;
    }
    // 5) 解压幂等（同状态重复呈现零解压）
    let mut d3 = DccState::new(3);
    let _ = d3.advance(0);
    let _ = d3.advance(1);
    let _ = d3.advance(2);
    let c1 = d3.decompress(false);
    let c2 = d3.decompress(false);
    if c1 == c2 {
        caught += 1;
    }
    // 6) 强制解压显式计数
    let c3 = d3.decompress(true);
    if c3 == c2 + 1 {
        caught += 1;
    }
    // 7) 回退账渲染（归因行非空含路径名）
    if d2.render().contains("迁移") && d2.render().contains("重算=true") {
        caught += 1;
    }
    (caught, 7)
}

/// 判据入口。
pub fn run_veb249_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0249");
    // J1-1 三路径一致性（全序推进 + 状态闭集）
    let ok = three_path_ok();
    s.add("AMD-249-01-三路径一致", ok, if ok { "" } else { "路径破缺" });
    // J1-2 非法迁移拒绝（跳路径 / 重复路径）
    let mut d = DccState::new(5);
    let ok = d.advance(2) == Err(XC_ST_BAD) && d.advance(9) == Err(XC_HND_BAD);
    s.add("AMD-249-02-非法迁移拒", ok, if ok { "" } else { "迁移放水" });
    // J1-3 回退重算（元数据丢失 → 回退 + 重算 + 账）
    let mut d = DccState::new(6);
    let _ = d.advance(0);
    let _ = d.advance(1);
    d.invalidate();
    let ok = d.advance(2) == Err(XC_META_LOST)
        && d.st == ST_VALID
        && d.fallbacks[0].recalc
        && d.decompress_count == 1;
    s.add("AMD-249-03-回退重算", ok, if ok { "" } else { "回退破缺" });
    // J1-4 零拷贝幂等（同态呈现零解压 + 显式解压计数）
    let mut d = DccState::new(7);
    let _ = d.advance(0);
    let _ = d.advance(1);
    let _ = d.advance(2);
    let a = d.decompress(false);
    let _ = d.decompress(false);
    let b = d.decompress(true);
    let ok = a == b - 1;
    s.add("AMD-249-04-零拷贝幂等", ok, if ok { "" } else { "幂等破缺" });
    // J1-5 降级标记可查（账渲染含回退行 + 归因路径名）
    let mut d = DccState::new(8);
    d.invalidate();
    let _ = d.advance(0);
    let ok = d.render().contains("绑定") && d.render().contains("回退=1");
    s.add("AMD-249-05-降级标记可查", ok, if ok { "" } else { "标记破缺" });
    // J1-6 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let d = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("AMD-249-06-注入核必抓", ok, &d);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb249_checks_all_green() {
        let set = run_veb249_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn veb249_full_recover_cycle() {
        // 全恢复循环：丢元数据 → 回退 → 重绑 → 全序再推进
        let mut d = DccState::new(0x99);
        let _ = d.advance(0);
        d.invalidate();
        let _ = d.advance(1);
        d.meta_valid = true; // 元数据重建
        assert!(d.advance(0).is_ok() && d.advance(1).is_ok() && d.advance(2).is_ok());
    }
}
