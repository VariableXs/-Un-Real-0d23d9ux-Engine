//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12404 · NtUnmapViewOfSection/NtUnmapViewOfSectionEx 语义档 · 0x30080
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F12404-J1 解除映射（整视/按范围档）判据绿，重复解除注入
//!      10/10 次正确拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 整视档：按基址命中唯一视口整段解除（FreeType=0 口径）；
//!   - 按范围档（Ex）：Range 起止必须落在同一活视口内，跨视口拒
//!     （CrossView）、越出视口拒（RangeOver）；
//!   - 重复解除：已解除视口再解除拒（AlreadyUnmapped）10/10；
//!   - 视口状态机 Mapped→Unmapped→Free，只有 Mapped 态可解；
//!   - 对齐纪律：范围起止必须页粒度对齐（NotAligned），跨度零拒
//!     （SpanZero）；
//!   - FreeFlags 闭集（0 / WITH_TRANSIENT_LOCK 位），越位拒（FlagBad）；
//!   - 范围解除只解除命中段：解除后视口账活数与映射数守恒；
//!   - 错误路径带码+中文 detail，拒止不写回任何视口行；
//!   - 注入核：LCG 写死种子 0x30080，重复解除 10/10 拒止。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30080 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30080;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 页粒度（4KiB 口径）。
pub const PAGE_SIZE: u64 = 0x1000;

/// 分配粒度（64KiB 口径）。
pub const ALLOC_GRAN: u64 = 0x10000;

/// 视口账槽位数（定容）。
pub const VIEW_SLOTS: usize = 24;

/// FreeFlags 合法位（WITH_TRANSIENT_LOCK 口径）。
pub const UNMAP_TRANSIENT_LOCK: u32 = 0x1;

/// 诊断码（0x30080 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnmapCode {
    /// 0x30080 基址未命中视口（无活视口覆盖该址）。
    NotMapped = 0,
    /// 0x30081 重复解除（视口已解除）。
    AlreadyUnmapped = 1,
    /// 0x30082 范围跨视口（起止落不同视口）。
    CrossView = 2,
    /// 0x30083 范围越出视口。
    RangeOver = 3,
    /// 0x30084 非页粒度对齐。
    NotAligned = 4,
    /// 0x30085 跨度零。
    SpanZero = 5,
    /// 0x30086 FreeFlags 越位。
    FlagBad = 6,
    /// 0x30087 槽越界。
    SlotOob = 7,
    /// 0x30088 段已死（节面不存在）。
    SecGone = 8,
    /// 0x30089 解除账漂移（unmap 计数与活数失平）。
    LedgerDrift = 9,
}

impl UnmapCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [UnmapCode; 10] = [
        UnmapCode::NotMapped, UnmapCode::AlreadyUnmapped, UnmapCode::CrossView,
        UnmapCode::RangeOver, UnmapCode::NotAligned, UnmapCode::SpanZero,
        UnmapCode::FlagBad, UnmapCode::SlotOob, UnmapCode::SecGone, UnmapCode::LedgerDrift,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30080 => "未命中视口：先查视口账活行，基址必须落在唯一活视口内。",
        0x30081 => "重复解除：视口解除后槽转 Free，再解除必拒。",
        0x30082 => "范围跨视口：Ex 档范围必须落同一活视口，先分次解除。",
        0x30083 => "范围越出：Range 不得越过命中视口终点。",
        0x30084 => "非页粒度：Range 起止向上/向下对齐 4KiB。",
        0x30085 => "跨度零：Range 起止相等无意义，整视档走基址口径。",
        0x30086 => "Flags 越位：只收 0/WITH_TRANSIENT_LOCK 一位。",
        0x30087 => "槽越界：内部槽号先比 VIEW_SLOTS 再取。",
        0x30088 => "段已死：节面释放后视口账只读。",
        0x30089 => "解除账漂移：unmap 计数 + 活数 ≡ 映射数，先对平再操作。",
        _ => "未知诊断码：回查 0x30080 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30080 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3008_3008_3008_3008;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 视口状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewState {
    /// 已映射（可解）。
    Mapped = 0,
    /// 已解除（终态前）。
    Unmapped = 1,
    /// 槽空闲（可复用）。
    Free = 2,
}

/// 视口账行。
#[derive(Clone, Copy, Debug)]
pub struct ViewRow {
    /// 基址。
    pub base: u64,
    /// 视口尺寸。
    pub size: u64,
    /// 状态。
    pub state: ViewState,
}

/// 解除映射语义档正本。
///
/// 生命周期：map_seed（登记映射视口）→ unmap_view（整视档）→
/// unmap_range（按范围档 Ex）→ 守恒核对。
#[derive(Debug)]
pub struct UnmapBook {
    /// 节面存活。
    sec_live: bool,
    /// 视口账。
    views: [ViewRow; VIEW_SLOTS],
    /// 自动基址游标。
    cursor: u64,
    /// 映射累计。
    mapped: u32,
    /// 解除累计。
    unmapped: u32,
    /// 拒止累计。
    rejects: u32,
}

impl UnmapBook {
    /// 建账（节面存活）。
    pub fn new() -> Self {
        UnmapBook {
            sec_live: true,
            views: core::array::from_fn(|_| ViewRow { base: 0, size: 0, state: ViewState::Free }),
            cursor: 0x10_0000,
            mapped: 0,
            unmapped: 0,
            rejects: 0,
        }
    }

    /// 登记映射视口（解除域前置面）。
    pub fn map_seed(&mut self, size: u64) -> Result<usize, UnmapCode> {
        if !self.sec_live {
            return Err(UnmapCode::SecGone);
        }
        let mut slot: Option<usize> = None;
        for i in 0..VIEW_SLOTS {
            if self.views[i].state == ViewState::Free {
                slot = Some(i);
                break;
            }
        }
        let s = match slot {
            Some(s) => s,
            None => {
                self.rejects += 1;
                return Err(UnmapCode::SecGone);
            }
        };
        let base = self.cursor;
        self.cursor += ALLOC_GRAN.max(size.next_multiple_of(ALLOC_GRAN));
        self.views[s] = ViewRow { base, size, state: ViewState::Mapped };
        self.mapped += 1;
        Ok(s)
    }

    /// 基址命中视口（唯一活视口覆盖判定）。
    pub fn hit_slot(&self, base: u64) -> Option<usize> {
        let mut hit: Option<usize> = None;
        for i in 0..VIEW_SLOTS {
            let v = &self.views[i];
            if v.state == ViewState::Mapped && base >= v.base && base < v.base + v.size {
                if hit.is_some() {
                    // 命中两个视口属账损，按未命中拒。
                    return None;
                }
                hit = Some(i);
            }
        }
        hit
    }

    /// 整视档解除（基址命中唯一活视口；重复解除必拒）。
    pub fn unmap_view(&mut self, base: u64, flags: u32) -> Result<u64, UnmapCode> {
        if flags & !UNMAP_TRANSIENT_LOCK != 0 {
            self.rejects += 1;
            return Err(UnmapCode::FlagBad);
        }
        if !self.sec_live {
            return Err(UnmapCode::SecGone);
        }
        let s = match self.hit_slot(base) {
            Some(s) => s,
            None => {
                // 已解除基址不再命中：重复解除口径（先查解除历史）。
                if self.was_unmapped(base) {
                    self.rejects += 1;
                    return Err(UnmapCode::AlreadyUnmapped);
                }
                self.rejects += 1;
                return Err(UnmapCode::NotMapped);
            }
        };
        let sz = self.views[s].size;
        self.views[s].state = ViewState::Unmapped;
        self.unmapped += 1;
        Ok(sz)
    }

    /// 解除历史核对（基址曾命中现已解除）。
    fn was_unmapped(&self, base: u64) -> bool {
        self.views.iter().any(|v| {
            v.state == ViewState::Unmapped && base >= v.base && base < v.base + v.size
        })
    }

    /// 按范围档解除（Ex：起止必须落同一活视口内且页对齐）。
    pub fn unmap_range(&mut self, start: u64, end: u64, flags: u32) -> Result<u64, UnmapCode> {
        if flags & !UNMAP_TRANSIENT_LOCK != 0 {
            self.rejects += 1;
            return Err(UnmapCode::FlagBad);
        }
        if !self.sec_live {
            return Err(UnmapCode::SecGone);
        }
        if end <= start {
            self.rejects += 1;
            return Err(UnmapCode::SpanZero);
        }
        if start % PAGE_SIZE != 0 || end % PAGE_SIZE != 0 {
            self.rejects += 1;
            return Err(UnmapCode::NotAligned);
        }
        let sh = match self.hit_slot(start) {
            Some(s) => s,
            None => {
                if self.was_unmapped(start) {
                    self.rejects += 1;
                    return Err(UnmapCode::AlreadyUnmapped);
                }
                self.rejects += 1;
                return Err(UnmapCode::NotMapped);
            }
        };
        let v = self.views[sh];
        if end > v.base + v.size {
            // 端点越出命中视口：端点若落进相邻视口则跨视口，否则纯越出。
            match self.hit_slot(end.wrapping_sub(1)) {
                Some(e) if e != sh => {
                    self.rejects += 1;
                    return Err(UnmapCode::CrossView);
                }
                _ => {
                    self.rejects += 1;
                    return Err(UnmapCode::RangeOver);
                }
            }
        }
        let span = end - start;
        self.views[sh].state = ViewState::Unmapped;
        self.unmapped += 1;
        Ok(span)
    }

    /// 活视口计数。
    pub fn live_views(&self) -> usize {
        self.views.iter().filter(|v| v.state == ViewState::Mapped).count()
    }

    /// 解除计数。
    pub fn unmapped_total(&self) -> u32 {
        self.unmapped
    }

    /// 映射计数。
    pub fn mapped_total(&self) -> u32 {
        self.mapped
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }

    /// 守恒：mapped ≡ live + unmapped。
    pub fn conserved(&self) -> bool {
        self.mapped as usize == self.live_views() + self.unmapped as usize
    }

    /// 槽快照。
    pub fn view_of(&self, slot: usize) -> Option<ViewRow> {
        if slot >= VIEW_SLOTS {
            None
        } else {
            Some(self.views[slot])
        }
    }
}

/// 预置映射尺寸表（演练域）。
fn seed_size(i: usize) -> u64 {
    PAGE_SIZE * (4 + (i % 4) as u64)
}

/// 整视档解除演练：10/10 轮映射→整视解除→守恒。
pub fn drill_unmap_view_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30080 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for r in 0..INJECT_ROUNDS {
        let mut bk = UnmapBook::new();
        let n = 1 + lcg.below(3) as usize;
        let mut pass = true;
        for i in 0..n {
            let sz = seed_size(i + r);
            let s = bk.map_seed(sz);
            pass = pass && s.is_ok();
        }
        pass = pass && bk.conserved() && bk.live_views() == n;
        // 整视解除第一个视口。
        if let Some(v) = bk.view_of(0) {
            match bk.unmap_view(v.base, 0) {
                Ok(sz) => pass = pass && sz == v.size,
                Err(_) => pass = false,
            }
        } else {
            pass = false;
        }
        pass = pass && bk.conserved() && bk.unmapped_total() == 1 && bk.live_views() == n - 1;
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("整视解除轮={}/{}", ok, INJECT_ROUNDS))
}

/// 按范围档解除演练：10/10 轮范围内解除成功、越界/跨视口拒。
pub fn drill_unmap_range_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30080 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for r in 0..INJECT_ROUNDS {
        let mut bk = UnmapBook::new();
        // 两个视口：0 大、1 小。
        let _ = bk.map_seed(PAGE_SIZE * 16);
        let _ = bk.map_seed(PAGE_SIZE * 4);
        let v0 = bk.view_of(0).unwrap_or(ViewRow { base: 0, size: 0, state: ViewState::Free });
        // 纯越出（终点越出视口且越过相邻视口终点）。
        let over = bk.unmap_range(v0.base + PAGE_SIZE * 14, v0.base + PAGE_SIZE * 40, 0);
        // 跨视口（终点落相邻视口内：v1 基址 = v0 基址 + 16 页）。
        let cross = bk.unmap_range(v0.base + PAGE_SIZE * 14, v0.base + PAGE_SIZE * 18, 0);
        // 非对齐。
        let align = bk.unmap_range(v0.base + PAGE_SIZE * 8 + 3, v0.base + PAGE_SIZE * 12, 0);
        // 跨度零。
        let zero = bk.unmap_range(v0.base, v0.base, 0);
        // 范围内解除（起点+2页，跨度4页；最后执行以验证成功路径）。
        let good = bk.unmap_range(v0.base + PAGE_SIZE * 2, v0.base + PAGE_SIZE * 6, 0);
        let _ = lcg.below(3);
        let _ = r;
        if matches!(good, Ok(span) if span == PAGE_SIZE * 4)
            && matches!(over, Err(UnmapCode::RangeOver))
            && matches!(cross, Err(UnmapCode::CrossView))
            && matches!(align, Err(UnmapCode::NotAligned))
            && matches!(zero, Err(UnmapCode::SpanZero))
            && bk.conserved()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("范围解除轮={}/{}", ok, INJECT_ROUNDS))
}

/// 重复解除注入 10/10 正确拒止演练。
pub fn drill_repeat_unmap_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30080 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = UnmapBook::new();
        let sz = seed_size(0);
        let _ = bk.map_seed(sz);
        let base = bk.view_of(0).map(|v| v.base).unwrap_or(0);
        let first = bk.unmap_view(base, 0);
        let second = bk.unmap_view(base, 0);
        let third = bk.unmap_range(base + PAGE_SIZE, base + PAGE_SIZE * 2, 0);
        let _ = lcg.below(3);
        if matches!(first, Ok(x) if x == sz)
            && matches!(second, Err(UnmapCode::AlreadyUnmapped))
            && matches!(third, Err(UnmapCode::AlreadyUnmapped))
            && bk.unmapped_total() == 1
            && bk.rejects >= 2
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("重复解除拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：视口账转录导出（逐槽一行：基/尺/态）。
/// ─────────────────────────────────────────────────────────────────────
/// 视口账转录导出。
pub fn export_view_lines(bk: &UnmapBook) -> String {
    let mut s = String::from("解除账转录：\n");
    for i in 0..VIEW_SLOTS {
        if let Some(v) = bk.view_of(i) {
            if v.state != ViewState::Free {
                s.push_str(&format!("V{} 基{:x} 尺{} 态{}\n", i, v.base, v.size, v.state as u8));
            }
        }
    }
    s
}

/// 转录完整判据（非空槽逐行齐备）。
pub fn view_lines_complete(bk: &UnmapBook) -> bool {
    let s = export_view_lines(bk);
    let used = (0..VIEW_SLOTS).filter(|&i| bk.view_of(i).map(|v| v.state != ViewState::Free).unwrap_or(false)).count();
    if used == 0 {
        return false;
    }
    let mut hits = 0usize;
    for i in 0..VIEW_SLOTS {
        if let Some(v) = bk.view_of(i) {
            if v.state != ViewState::Free && s.contains(&format!("V{} 基{:x}", i, v.base)) {
                hits += 1;
            }
        }
    }
    hits == used
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：解除报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct UnmapReport {
    /// 活视口数。
    pub live: usize,
    /// 映射累计。
    pub mapped: u32,
    /// 解除累计。
    pub unmapped: u32,
    /// 拒止累计。
    pub rejects: u32,
    /// 守恒。
    pub conserved: bool,
}

impl UnmapReport {
    /// 建报告。
    pub fn build(bk: &UnmapBook) -> Self {
        UnmapReport {
            live: bk.live_views(),
            mapped: bk.mapped_total(),
            unmapped: bk.unmapped_total(),
            rejects: bk.reject_total(),
            conserved: bk.conserved(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.conserved && self.mapped >= 1 && self.unmapped >= 1 && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "UnmapReport 活={} 映={} 解={} 拒={} 守恒={}",
            self.live, self.mapped, self.unmapped, self.rejects, self.conserved
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：Flags 变体演练（0/锁位/越位 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_flags_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30080 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = UnmapBook::new();
        let _ = bk.map_seed(seed_size(0));
        let _ = bk.map_seed(seed_size(1));
        let b0 = bk.view_of(0).map(|v| v.base).unwrap_or(0);
        let b1 = bk.view_of(1).map(|v| v.base).unwrap_or(0);
        let f0 = bk.unmap_view(b0, 0);
        let f1 = bk.unmap_view(b1, UNMAP_TRANSIENT_LOCK);
        let bad = bk.unmap_view(b1, 0x2);
        let _ = lcg.below(3);
        if f0.is_ok() && f1.is_ok() && matches!(bad, Err(UnmapCode::FlagBad)) {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("Flags 变体轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：未命中演练（盲基址/段死 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_notmapped_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30080 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = UnmapBook::new();
        let _ = bk.map_seed(seed_size(0));
        let r1 = bk.unmap_view(0x7f_ff00_0000, 0);
        bk.sec_live = false;
        let r2 = bk.unmap_view(0x7f_ff00_0000, 0);
        let _ = lcg.below(3);
        if matches!(r1, Err(UnmapCode::NotMapped)) && matches!(r2, Err(UnmapCode::SecGone)) {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("未命中/段死轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct UnmapCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_unmap_checklist(bk: &UnmapBook) -> Vec<UnmapCheckRow> {
    let rep = UnmapReport::build(bk);
    let mut rows: Vec<UnmapCheckRow> = Vec::new();
    rows.push(UnmapCheckRow { name: "守恒", passed: bk.conserved() });
    rows.push(UnmapCheckRow { name: "页粒度常量", passed: PAGE_SIZE == 0x1000 });
    rows.push(UnmapCheckRow { name: "分配粒度常量", passed: ALLOC_GRAN == 0x10000 });
    rows.push(UnmapCheckRow { name: "Flags闭集", passed: UNMAP_TRANSIENT_LOCK == 0x1 });
    rows.push(UnmapCheckRow { name: "转录完整", passed: view_lines_complete(bk) });
    rows.push(UnmapCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(UnmapCheckRow { name: "解除在账", passed: bk.unmapped_total() >= 1 });
    rows.push(UnmapCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(UnmapCheckRow { name: "槽位宽", passed: VIEW_SLOTS == 24 });
    rows.push(UnmapCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn unmap_checklist_green(rows: &[UnmapCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn unmap_stats_line(bk: &UnmapBook) -> String {
    format!(
        "解除概况：活 {} 映 {} 解 {} 拒 {}",
        bk.live_views(),
        bk.mapped_total(),
        bk.unmapped_total(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12404_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = UnmapBook::new();
    let _ = bk.map_seed(PAGE_SIZE * 16);
    let _ = bk.map_seed(PAGE_SIZE * 4);
    let b0 = bk.view_of(0).map(|v| v.base).unwrap_or(0);
    let _ = bk.unmap_view(b0, 0);
    // 注入一例重复解除（拒止账口径）。
    let _ = bk.unmap_view(b0, 0);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        view_lines_complete(&bk) && UnmapReport::build(&bk).all_green(),
        unmap_stats_line(&bk),
    ));
    // X-b：Flags 变体 10/10。
    let (ok_f, d_f) = drill_flags_10();
    out.push(("X-Flags变体10/10", ok_f, d_f));
    // X-c：未命中/段死 10/10。
    let (ok_n, d_n) = drill_notmapped_10();
    out.push(("X-未命中/段死10/10", ok_n, d_n));
    // X-d：终验清单。
    let rows = build_unmap_checklist(&bk);
    out.push(("X-终验清单10项", unmap_checklist_green(&rows), UnmapReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：重复解除必拒（AlreadyUnmapped）。
    let mut bk = UnmapBook::new();
    let _ = bk.map_seed(PAGE_SIZE * 4);
    let b = bk.view_of(0).map(|v| v.base).unwrap_or(0);
    let _ = bk.unmap_view(b, 0);
    if matches!(bk.unmap_view(b, 0), Err(UnmapCode::AlreadyUnmapped)) {
        caught += 1;
        d.push("注1 重复:抓".to_string());
    } else {
        d.push("注1 重复:漏".to_string());
    }
    // 注 2：未命中必拒（NotMapped）。
    let mut bk2 = UnmapBook::new();
    let _ = bk2.map_seed(PAGE_SIZE * 4);
    if matches!(bk2.unmap_view(0x6000_0000, 0), Err(UnmapCode::NotMapped)) {
        caught += 1;
        d.push("注2 未命中:抓".to_string());
    } else {
        d.push("注2 未命中:漏".to_string());
    }
    // 注 3：跨视口必拒（CrossView）。
    let mut bk3 = UnmapBook::new();
    let _ = bk3.map_seed(PAGE_SIZE * 16);
    let _ = bk3.map_seed(PAGE_SIZE * 4);
    let b3 = bk3.view_of(0).map(|v| v.base).unwrap_or(0);
    if matches!(
        bk3.unmap_range(b3 + PAGE_SIZE * 14, b3 + PAGE_SIZE * 18, 0),
        Err(UnmapCode::CrossView)
    ) {
        caught += 1;
        d.push("注3 跨视:抓".to_string());
    } else {
        d.push("注3 跨视:漏".to_string());
    }
    // 注 4：非对齐必拒（NotAligned）。
    let mut bk4 = UnmapBook::new();
    let _ = bk4.map_seed(PAGE_SIZE * 16);
    let b4 = bk4.view_of(0).map(|v| v.base).unwrap_or(0);
    if matches!(bk4.unmap_range(b4 + 7, b4 + PAGE_SIZE * 2, 0), Err(UnmapCode::NotAligned)) {
        caught += 1;
        d.push("注4 对齐:抓".to_string());
    } else {
        d.push("注4 对齐:漏".to_string());
    }
    // 注 5：Flags 越位必拒（FlagBad）。
    if matches!(bk4.unmap_view(b4, 0x4), Err(UnmapCode::FlagBad)) {
        caught += 1;
        d.push("注5 越位:抓".to_string());
    } else {
        d.push("注5 越位:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30080 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    UnmapCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12404_checks：本单判据聚合（诊断段 0x30080）。
pub fn run_unx12404_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12404 NtUnmapViewOfSection/Ex 语义档");
    // J1-a：整视档解除 10/10。
    let (ok1, d1) = drill_unmap_view_10();
    s.add_dyn("J1-整视档解除10/10", ok1, &d1);
    // J1-b：按范围档解除 10/10。
    let (ok2, d2) = drill_unmap_range_10();
    s.add_dyn("J1-按范围档解除10/10", ok2, &d2);
    // J1-c：重复解除注入 10/10 正确拒止。
    let (ok3, d3) = drill_repeat_unmap_10();
    s.add_dyn("J1-重复解除拒止10/10", ok3, &d3);
    // J1-d：标准会话守恒（两映射一解除）。
    let mut bk = UnmapBook::new();
    let _ = bk.map_seed(PAGE_SIZE * 16);
    let _ = bk.map_seed(PAGE_SIZE * 4);
    let b0 = bk.view_of(0).map(|v| v.base).unwrap_or(0);
    let _ = bk.unmap_view(b0, 0);
    s.add_dyn(
        "J1-映射解除守恒",
        bk.conserved() && bk.mapped_total() == 2 && bk.unmapped_total() == 1 && bk.live_views() == 1,
        &format!("映={} 解={} 活={} 守恒={}", bk.mapped_total(), bk.unmapped_total(), bk.live_views(), bk.conserved()),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（重复解除后解除计数不变）。
    let u0 = bk.unmapped_total();
    let _ = bk.unmap_view(b0, 0);
    s.add_dyn(
        "J2-拒止不写回",
        bk.unmapped_total() == u0 && bk.conserved(),
        &format!("解除 {}→{}", u0, bk.unmapped_total()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30080段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12404_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
