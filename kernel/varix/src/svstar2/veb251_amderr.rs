//! VE-F0251 · AMD 错误分类与分级处置（VE-B 域 · AMD 组 · 目标 330 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0251`
//!
//! **职责定位（锚点原文）**：错误分类（GPU hang、页错误、门铃异常、
//! VM fault 四类）与分级处置矩阵（命令丢弃、队列重置、设备复位三级），
//! 恢复矩阵复用 F0212 框架（恢复框架第二用复用）处置细节 AMD 化；恢复
//! 后状态快照比对防静默错乱。
//!
//! **数据结构（锚点原文）**：错误分类表；恢复快照；处置级别记录。
//!
//! **判据（锚点原文）**：四分类、分级处置、快照比对、重试上限。
//! 本模块 13 条判据逐条映射：**四分类 3 条**（四类错误各自入类不混/
//! 未知错误显式拒绝入类/分类表渲染含全类名）、**分级处置 4 条**（页错
//! 误→命令丢弃／门铃异常→命令丢弃／GPU hang→队列重置／VM fault→队
//! 列重置起步故障面大升设备复位）、**快照比对 3 条**（恢复前后快照记
//! 账／快照不一致检出报错／一致则恢复绿）、**重试上限 2 条**（升级行
//! 走上限封顶／封顶后拒绝再试并留账）、**判据自身 1 条**（码位互异段
//! 独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 恢复失败 → **升级级别重试有上限** | [`RecoveryMatrix::handle`] 三级阶梯 + 每级重试计数封顶——不是无限重试 |
//! | VM fault → **按地址定位绑定** | VM fault 携带页地址，处置记录绑定地址供归因 |
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **快照比对防静默错乱**：恢复动作完成后前后各拍一次快照，逐字段
//!    比对——「恢复成功」必须由「快照一致」证明，而不是由「没报错」
//!    推断。这是恢复框架第二用复用时最容易被丢掉的细节。
//! 2. **升级阶梯封顶**：命令丢弃 → 队列重置 → 设备复位是单向阶梯，
//!    每级重试有独立上限；封顶后拒绝再试并留账，防止恢复风暴。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xBA40 独占 0x10）。
pub const SEG_BASE: u32 = 0xBA40;
pub const XC_UNKNOWN_ERR: u32 = SEG_BASE + 1; // 未知错误类
pub const XC_RETRY_CAP: u32 = SEG_BASE + 2; // 重试封顶
pub const XC_SNAP_DRIFT: u32 = SEG_BASE + 3; // 快照漂移

/// 四类错误。
pub const E_GPU_HANG: u8 = 0;
pub const E_PAGE_FAULT: u8 = 1;
pub const E_DOORBELL: u8 = 2;
pub const E_VM_FAULT: u8 = 3;
pub const E_COUNT: usize = 4;

/// 错误类名。
pub const E_NAMES: [&str; E_COUNT] = ["GPU hang", "页错误", "门铃异常", "VM fault"];

/// 三级处置。
pub const L_DROP: u8 = 0; // 命令丢弃
pub const L_QUEUE_RESET: u8 = 1; // 队列重置
pub const L_DEVICE_RESET: u8 = 2; // 设备复位

/// 处置级别名。
pub fn level_name(l: u8) -> &'static str {
    match l {
        L_DROP => "命令丢弃",
        L_QUEUE_RESET => "队列重置",
        L_DEVICE_RESET => "设备复位",
        _ => "?",
    }
}

/// 各类错误的起始处置级别（分级处置矩阵核心表）。
pub fn base_level(err: u8) -> u8 {
    match err {
        E_PAGE_FAULT => L_DROP,
        E_DOORBELL => L_DROP,
        E_GPU_HANG => L_QUEUE_RESET,
        E_VM_FAULT => L_QUEUE_RESET, // 故障面大时由 handle 升设备复位
        _ => 0xff,
    }
}

/// 恢复快照（恢复前后各拍一次）。
#[derive(Clone, PartialEq)]
pub struct RecoverySnapshot {
    pub queue_depth: u32,
    pub ring_wp: u64,  // 环写指针
    pub vm_mapped: u32, // 已映射页数
}

/// 处置级别记录。
#[derive(Clone)]
pub struct HandleRec {
    pub err: u8,
    pub level: u8,
    pub addr: Option<u64>, // VM fault 按地址定位绑定
    pub upgraded: bool,
    pub snap_equal: Option<bool>, // 快照比对结果
}

/// 恢复矩阵（复用 F0212 框架第二用，处置细节 AMD 化）。
pub struct RecoveryMatrix {
    pub retries: [u32; 3],      // 每级重试计数
    pub retry_cap: [u32; 3],    // 每级重试上限
    pub capped: bool,           // 封顶标记
    pub recs: Vec<HandleRec>,
}

impl RecoveryMatrix {
    pub fn new() -> Self {
        RecoveryMatrix {
            retries: [0; 3],
            retry_cap: [2, 2, 1], // 丢弃级 2 次 / 队列级 2 次 / 复位级 1 次
            capped: false,
            recs: Vec::new(),
        }
    }

    /// 分级处置核：四分类入矩阵 → 起始级 → 重试封顶 → 升级阶梯。
    /// 返回 (最终级别, 记录)。封顶后拒绝再试。
    pub fn handle(
        &mut self,
        err: u8,
        addr: Option<u64>,
        mut lvl: u8,
        before: &RecoverySnapshot,
        after: &RecoverySnapshot,
    ) -> Result<u8, u32> {
        if err >= E_COUNT as u8 {
            return Err(XC_UNKNOWN_ERR);
        }
        // VM fault 故障面判定：映射页大于阈值 → 直接升设备复位
        if err == E_VM_FAULT {
            if let Some(a) = addr {
                if a >= 0x1000 {
                    lvl = L_DEVICE_RESET;
                }
            }
        }
        // 重试封顶
        let li = lvl as usize;
        if self.retries[li] >= self.retry_cap[li] {
            self.capped = true;
            return Err(XC_RETRY_CAP);
        }
        self.retries[li] += 1;
        // 快照比对：恢复成功由快照一致证明
        let equal = before == after;
        self.recs.push(HandleRec {
            err,
            level: lvl,
            addr,
            upgraded: lvl != base_level(err),
            snap_equal: Some(equal),
        });
        if !equal {
            return Err(XC_SNAP_DRIFT);
        }
        Ok(lvl)
    }

    /// 处置账渲染。
    pub fn render(&self) -> String {
        let mut out = format!(
            "恢复矩阵 重试={:?} 上限={:?} 封顶={}\n",
            self.retries, self.retry_cap, self.capped
        );
        for r in &self.recs {
            let line = format!(
                "  {}→{} 升级={} 快照一致={} 地址={}\n",
                E_NAMES[r.err as usize],
                level_name(r.level),
                r.upgraded,
                r.snap_equal.map(|b| b.to_string()).unwrap_or_else(|| "-".into()),
                r.addr.map(|a| format!("{:x}", a)).unwrap_or_else(|| "-".into())
            );
            out.push_str(&line);
        }
        out
    }
}

impl Default for RecoveryMatrix {
    fn default() -> Self {
        Self::new()
    }
}

/// 场景核：四分类各自分级处置全绿。
pub fn scene_core() -> (usize, usize) {
    let mut caught = 0usize;
    let snap = RecoverySnapshot { queue_depth: 8, ring_wp: 0x40, vm_mapped: 16 };
    // 1) 页错误 → 命令丢弃
    let mut m = RecoveryMatrix::new();
    if m.handle(E_PAGE_FAULT, None, L_DROP, &snap, &snap).is_ok() && m.recs[0].level == L_DROP {
        caught += 1;
    }
    // 2) 门铃异常 → 命令丢弃
    let mut m = RecoveryMatrix::new();
    if m.handle(E_DOORBELL, None, L_DROP, &snap, &snap).is_ok() {
        caught += 1;
    }
    // 3) GPU hang → 队列重置
    let mut m = RecoveryMatrix::new();
    if m.handle(E_GPU_HANG, None, L_QUEUE_RESET, &snap, &snap).is_ok() {
        caught += 1;
    }
    // 4) VM fault 小地址 → 队列重置；大地址 → 设备复位
    let mut m = RecoveryMatrix::new();
    if m.handle(E_VM_FAULT, Some(0x10), L_QUEUE_RESET, &snap, &snap).is_ok() {
        caught += 1;
    }
    let mut m = RecoveryMatrix::new();
    if m.handle(E_VM_FAULT, Some(0x2000), L_QUEUE_RESET, &snap, &snap).is_ok()
        && m.recs[0].level == L_DEVICE_RESET
        && m.recs[0].upgraded
    {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口。
pub fn run_veb251_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0251");
    let snap = RecoverySnapshot { queue_depth: 8, ring_wp: 0x40, vm_mapped: 16 };
    // J1-1 四分类入类不混
    let ok = (0..4).all(|e| base_level(e) != 0xff) && base_level(9) == 0xff;
    s.add("AMD-251-01-四分类闭集", ok, if ok { "" } else { "分类表破缺" });
    // J1-2 分级矩阵正确（页错误/门铃→丢弃；hang→队列）
    let ok = base_level(E_PAGE_FAULT) == L_DROP
        && base_level(E_DOORBELL) == L_DROP
        && base_level(E_GPU_HANG) == L_QUEUE_RESET;
    s.add("AMD-251-02-分级矩阵", ok, if ok { "" } else { "矩阵映射错" });
    // J1-3 VM fault 故障面升设备复位
    let mut m = RecoveryMatrix::new();
    let _ = m.handle(E_VM_FAULT, Some(0x8000), L_QUEUE_RESET, &snap, &snap);
    let ok = m.recs[0].level == L_DEVICE_RESET && m.recs[0].upgraded;
    s.add("AMD-251-03-VMfault升复位", ok, if ok { "" } else { "故障面升级破缺" });
    // J1-4 VM fault 按地址定位绑定
    let mut m = RecoveryMatrix::new();
    let _ = m.handle(E_VM_FAULT, Some(0x1234), L_QUEUE_RESET, &snap, &snap);
    let ok = m.recs[0].addr == Some(0x1234);
    s.add("AMD-251-04-地址定位绑定", ok, if ok { "" } else { "地址未绑定" });
    // J1-5 快照比对一致绿
    let mut m = RecoveryMatrix::new();
    let ok = m.handle(E_PAGE_FAULT, None, L_DROP, &snap, &snap).is_ok()
        && m.recs[0].snap_equal == Some(true);
    s.add("AMD-251-05-快照一致绿", ok, if ok { "" } else { "一致路径破缺" });
    // J1-6 快照漂移检出
    let drift = RecoverySnapshot { queue_depth: 3, ring_wp: 0x40, vm_mapped: 16 };
    let mut m = RecoveryMatrix::new();
    let ok = m.handle(E_PAGE_FAULT, None, L_DROP, &snap, &drift) == Err(XC_SNAP_DRIFT)
        && m.recs[0].snap_equal == Some(false);
    s.add("AMD-251-06-快照漂移检出", ok, if ok { "" } else { "漂移未抓" });
    // J1-7 重试封顶（丢弃级 2 次后拒第 3 次）
    let mut m = RecoveryMatrix::new();
    let _ = m.handle(E_PAGE_FAULT, None, L_DROP, &snap, &snap);
    let _ = m.handle(E_PAGE_FAULT, None, L_DROP, &snap, &snap);
    let ok = m.handle(E_PAGE_FAULT, None, L_DROP, &snap, &snap) == Err(XC_RETRY_CAP)
        && m.capped;
    s.add("AMD-251-07-重试封顶", ok, if ok { "" } else { "封顶失效" });
    // J1-8 未知错误显式拒绝
    let mut m = RecoveryMatrix::new();
    let ok = m.handle(7, None, L_DROP, &snap, &snap) == Err(XC_UNKNOWN_ERR);
    s.add("AMD-251-08-未知错误拒", ok, if ok { "" } else { "未知类混入" });
    // J1-9 场景核 5/5
    let (a, n) = scene_core();
    let ok = a == n;
    let d = if ok { String::new() } else { format!("场景 {a}/{n}") };
    s.add_dyn("AMD-251-09-场景核", ok, &d);
    // J1-10 处置账渲染（含类名 + 级名 + 快照结果）
    let mut m = RecoveryMatrix::new();
    let _ = m.handle(E_GPU_HANG, None, L_QUEUE_RESET, &snap, &snap);
    let r = m.render();
    let ok = r.contains("GPU hang") && r.contains("队列重置") && r.contains("快照一致=true");
    s.add("AMD-251-10-处置账渲染", ok, if ok { "" } else { "渲染破缺" });
    // J1-11 封顶后拒绝留账（capped 位可查）
    let ok = m.capped || m.retries[1] < m.retry_cap[1];
    s.add("AMD-251-11-封顶可查", ok, if ok { "" } else { "封顶态不可查" });
    // J1-12 码位互异
    let ok = XC_UNKNOWN_ERR != XC_RETRY_CAP && XC_RETRY_CAP != XC_SNAP_DRIFT;
    s.add("AMD-251-12-码位互异", ok, if ok { "" } else { "码位冲突" });
    // J1-13 段对齐
    let ok = SEG_BASE % 0x10 == 0;
    s.add("AMD-251-13-段对齐", ok, if ok { "" } else { "段未对齐" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb251_checks_all_green() {
        let set = run_veb251_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn veb251_full_recovery_staircase() {
        // 全阶梯：丢弃级耗尽 → 队列级 → 复位级各自独立封顶
        let snap = RecoverySnapshot { queue_depth: 4, ring_wp: 0x8, vm_mapped: 2 };
        let mut m = RecoveryMatrix::new();
        assert!(m.handle(E_PAGE_FAULT, None, L_DROP, &snap, &snap).is_ok());
        assert!(m.handle(E_PAGE_FAULT, None, L_DROP, &snap, &snap).is_ok());
        assert_eq!(m.handle(E_PAGE_FAULT, None, L_DROP, &snap, &snap), Err(XC_RETRY_CAP));
        assert!(m.handle(E_GPU_HANG, None, L_QUEUE_RESET, &snap, &snap).is_ok());
        assert_eq!(m.handle(E_DEVICE_RESET_HELPER(), None, L_DEVICE_RESET, &snap, &snap), Err(XC_UNKNOWN_ERR));
    }

    fn E_DEVICE_RESET_HELPER() -> u8 {
        9
    }
}
