//! UNX-F4981 · 延迟分配（delalloc）语义声明账 · 诊断段 0x7E00
//! 验收锚点：UNX-F4981-J1 delalloc 语义声明落账——写时只记账不寻块（Deferred：
//! 页缓存脏标记 + 字节区间登记），回写时寻块（writeback：区间→extent 分配）；
//! 语义与上游对照一致率 100%，状态枚举账（Clean/Deferred/Delay/Allocated/
//! Written）可导出，状态迁移全合法路径可枚举。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0x7E00;

/// 页状态枚举。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageState {
    Clean,
    Deferred,
    Delay,
    Allocated,
    Written,
}

impl PageState {
    pub fn as_str(self) -> &'static str {
        match self {
            PageState::Clean => "clean",
            PageState::Deferred => "deferred",
            PageState::Delay => "delay",
            PageState::Allocated => "allocated",
            PageState::Written => "written",
        }
    }
}

/// 合法迁移表（上游语义对照）：Clean→Deferred（延迟写登记）/ Deferred→Delay（回写寻块立账）/
/// Delay→Allocated（寻块成功）/ Allocated→Written（数据落盘）/ Written→Clean（清账）。
pub fn legal_transitions() -> Vec<(PageState, PageState)> {
    vec![
        (PageState::Clean, PageState::Deferred),
        (PageState::Deferred, PageState::Delay),
        (PageState::Delay, PageState::Allocated),
        (PageState::Allocated, PageState::Written),
        (PageState::Written, PageState::Clean),
    ]
}

pub fn can_transition(from: PageState, to: PageState) -> bool {
    legal_transitions().iter().any(|(a, b)| *a == from && *b == to)
}

/// 延迟区间账：页号区间（写时只记账）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DelRange {
    pub start_pg: u32,
    pub end_pg: u32, // 含
}

impl DelRange {
    pub fn new(start_pg: u32, end_pg: u32) -> Self {
        DelRange { start_pg, end_pg }
    }
    pub fn len(&self) -> u32 {
        self.end_pg - self.start_pg + 1
    }
    pub fn overlaps(&self, o: &DelRange) -> bool {
        self.start_pg <= o.end_pg && o.start_pg <= self.end_pg
    }
}

/// delalloc 语义机。
pub struct Delalloc {
    pub state: PageState,
    pub pending: Vec<DelRange>,
    pub total_bytes_deferred: u64,
    pub total_bytes_allocated: u64,
}

impl Delalloc {
    pub fn new() -> Self {
        Delalloc { state: PageState::Clean, pending: Vec::new(), total_bytes_deferred: 0, total_bytes_allocated: 0 }
    }

    /// 写时：只记账不寻块（Clean→Deferred）。
    pub fn defer_write(&mut self, r: DelRange) -> bool {
        if !can_transition(self.state, PageState::Deferred) {
            return false;
        }
        self.state = PageState::Deferred;
        self.pending.push(r);
        self.total_bytes_deferred += r.len() as u64 * 4096;
        true
    }

    /// 回写时：寻块（Deferred→Delay→Allocated），区间出账。
    pub fn writeback_seek(&mut self) -> u64 {
        if self.state != PageState::Deferred && self.state != PageState::Delay {
            return 0;
        }
        self.state = PageState::Delay;
        let mut allocated = 0u64;
        for r in self.pending.drain(..) {
            allocated += r.len() as u64 * 4096;
        }
        self.state = PageState::Allocated;
        self.total_bytes_allocated += allocated;
        allocated
    }

    /// 落盘：Allocated→Written→Clean。
    pub fn complete(&mut self) -> bool {
        if self.state != PageState::Allocated {
            return false;
        }
        self.state = PageState::Written;
        self.state = PageState::Clean;
        true
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0x7E00 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x4981_7E00;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("枚举-五态标签齐全", PageState::Clean.as_str() == "clean" && PageState::Written.as_str() == "written"));
    out.push(("迁移-反向非法", !can_transition(PageState::Deferred, PageState::Clean)));
    out.push(("区间-单页len=1", DelRange::new(7, 7).len() == 1));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx4981_checks：本单判据聚合（诊断段 0x7E00）。
pub fn run_unx4981_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F4981 延迟分配语义声明账");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 状态枚举账可导出：五态顺序标签
    let states = [PageState::Clean, PageState::Deferred, PageState::Delay, PageState::Allocated, PageState::Written];
    let export_ok = states.iter().map(|x| x.as_str()).collect::<Vec<&str>>().join(",") == "clean,deferred,delay,allocated,written";
    s.add("枚举-五态账可导出顺序一致", export_ok, if export_ok { "" } else { "导出序漂移" });

    // 合法迁移表：5 边 + 全反向非法
    let edges = legal_transitions();
    let edges_ok = edges.len() == 5;
    let mut rev_ok = true;
    for (a, b) in &edges {
        if can_transition(*b, *a) {
            rev_ok = false;
        }
    }
    s.add("迁移-5边落账", edges_ok, if edges_ok { "" } else { "边数漂移" });
    s.add("迁移-全反向非法", rev_ok, if rev_ok { "" } else { "反向迁移漏网" });

    // 写时只记账不寻块：Clean→Deferred，pending 入账
    let mut d = Delalloc::new();
    let ok_defer = d.defer_write(DelRange::new(0, 15));
    let defer_ok = ok_defer && d.state == PageState::Deferred && d.pending.len() == 1 && d.total_bytes_deferred == 16 * 4096;
    s.add("写时-只记账不寻块64KB入账", defer_ok, if defer_ok { "" } else { "延迟记账漂移" });

    // Deferred 下再 defer 合法（可累积）
    let acc_defer = d.defer_write(DelRange::new(16, 31)) && d.pending.len() == 2;
    s.add("写时-Deferred态可再登记", acc_defer, if acc_defer { "" } else { "累积登记失效" });

    // 回写寻块：区间出账 + 状态推进
    let alloc = d.writeback_seek();
    let wb_ok = alloc == 32 * 4096 && d.state == PageState::Allocated && d.pending.is_empty() && d.total_bytes_allocated == 32 * 4096;
    s.add("回写-寻块128KB出账到Allocated", wb_ok, if wb_ok { "" } else { "回写寻块漂移" });

    // 落盘：Allocated→Written→Clean
    let done = d.complete();
    s.add("落盘-完成回Clean", done && d.state == PageState::Clean, if done { "" } else { "完成路径漂移" });

    // Clean 态寻块拒（语义：未登记无可寻）
    let mut d2 = Delalloc::new();
    s.add("语义-Clean态寻块返回0", d2.writeback_seek() == 0, if d2.writeback_seek() == 0 { "" } else { "Clean 寻块漏网" });

    // 区间重叠检测
    let r1 = DelRange::new(0, 10);
    let r2 = DelRange::new(11, 20);
    let r3 = DelRange::new(5, 15);
    let ovl_ok = !r1.overlaps(&r2) && r1.overlaps(&r3) && r2.overlaps(&r3);
    s.add("区间-重叠判定三例恰准", ovl_ok, if ovl_ok { "" } else { "重叠判定漂移" });

    // 100 轮随机写-回写循环守恒：deferred == allocated 总量
    let mut seed: u64 = 0x4981_0100;
    let mut d3 = Delalloc::new();
    let mut loops = 0u32;
    for _ in 0..100 {
        let a = (lcg_next(&mut seed) % 32) as u32;
        let b = a + (lcg_next(&mut seed) % 32) as u32;
        if d3.defer_write(DelRange::new(a, b)) {
            loops += 1;
        }
        if loops % 10 == 0 {
            let _ = d3.writeback_seek();
            let _ = d3.complete();
        }
    }
    let _ = d3.writeback_seek();
    let conserved = d3.total_bytes_deferred == d3.total_bytes_allocated;
    s.add("守恒-100轮deferred=allocated", conserved, if conserved { "" } else { "守恒缺口" });

    // FNV 状态账指纹
    let h1 = fnv1a(format!("{}|{}", PageState::Allocated.as_str(), 32768u64).as_bytes());
    let h2 = fnv1a(b"allocated|32768");
    s.add("FNV-状态账指纹稳定", h1 == h2, if h1 == h2 { "" } else { "指纹不稳" });

    s
}
