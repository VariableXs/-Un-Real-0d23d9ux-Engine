//! F248 窗口置顶（钉住） · 判据实装。
//!
//! **判据锚**：主册 F248「窗口置顶（钉住）」。
//!
//! **验收标准第一句（任务包原文）**：不抢焦点判据（置顶后点击他窗焦点
//! 转移正确）。
//!
//! **判据（主册原文摘录）**：标题栏右键「窗口置顶」：置顶窗升入浮层组
//! （F226 层级）阴影加深+标题栏右上角出现图钉标记，再点解除；置顶不抢
//! 焦点（点击其他窗口焦点正常走，置顶窗只是不被盖住）——「始终在最上面」
//! 和「抢焦点」是两回事，VARIX 只做前者；置顶状态在任务栏缩略图上有
//! 标记，Alt+Tab 列表中同样标注。验收：图钉/标记三处一致（标题栏/缩略
//! 图/Alt+Tab）；多窗置顶叠加顺序（后置顶在上）；重启不记忆置顶（默认
//! 回常态，文档化）。
//!
//! **设计要点**：
//! - 不抢焦点是**结构性**的：`pin()` 只写置顶组，代码路径上不存在对
//!   焦点栈的写入；焦点栈是独立数据结构，`click_focus` 与置顶组零耦合；
//!   `audit_focus_not_stolen` 把判据落成可执行场景（置顶后点击他窗，
//!   焦点必须转移到他窗）；
//! - 浮层组序：置顶带单调递增 order，**后置顶在上**；置顶组整体高于
//!   一切未置顶窗（`effective_top` 消费合成器的 z 值 + 置顶组做最终
//!   裁决——置顶组不重建整个 z 栈，只做覆盖裁决，与 F226 层级解耦）；
//! - 图钉三处一致：标题栏/缩略图/Alt+Tab 三个消费面读**同一个**
//!   `is_pinned` 状态源——一个状态三处读，一致性由构造保证，
//!   `badges_consistent` 留作回归审计面；
//! - 重启不记忆置顶：持久化快照只含版本号（VPIN 头），置顶组结构性
//!   不入快照——解码恢复后所有窗口回常态层，判据「默认回常态」；
//! - 零堆热路径：置顶组/焦点栈/账本全定长；时间一律注入（毫秒戳）。
//!
//! **依赖锚点**：F226（窗口层级与阴影体系——浮层组与阴影档位挂其下）、
//! F221/F081（Alt+Tab 列表消费面）、F062（任务栏缩略图消费面）。

use crate::checks::CheckSet;
use crate::star::sbase::RingLog;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（每条注明主册依据）
// ---------------------------------------------------------------------------

/// 置顶组容量——同屏同时置顶的窗口上限（判据「多窗置顶叠加」的容量面）。
pub const PIN_CAP: usize = 64;

/// 焦点栈深度——最近点击去重栈（不抢焦点判据的独立承载面）。
pub const FOCUS_CAP: usize = 16;

/// 常态阴影档——F226 层级阴影体系锚点（H 域引用值：常态 1 档）。
pub const SHADOW_TIER_NORMAL: u8 = 1;

/// 置顶浮层组阴影档——判据「阴影加深」：升入浮层组取 3 档。
pub const SHADOW_TIER_PINNED: u8 = 3;

/// 置顶事件账本容量（钉住/解除留痕，定容环形）。
pub const LEDGER_CAP: usize = 64;

/// 重启不记忆置顶的文档锚（判据「重启不记忆置顶（默认回常态，文档化）」）。
pub const REBOOT_FORGET_DOC: &str = "置顶状态不进持久化面：编码快照只含版本号（VPIN 头），置顶组结构性不入快照；重启后全部窗口回常态层，需置顶的窗口由用户重新钉住。";

/// 标题栏图钉悬停提示文案（判据「始终在最上面 ≠ 抢焦点」的语义锚——
/// 设置页说明与标题栏悬停共用同一字串，杜绝双源漂移）。
pub const PIN_HINT_DOC: &str = "已置顶：窗口始终保持在最上层，不会被其他窗口盖住；这不会抢占焦点——点击其他窗口时焦点照常转移。再次点击图钉解除。";

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 置顶组登记（order 单调递增：后置顶在上）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PinRec {
    pub win: u32,
    pub order: u32,
    /// 钉住时刻（ms，注入式；审计面用）。
    pub ts: u64,
}

/// 置顶事件账本条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PinEvent {
    pub ts: u64,
    pub win: u32,
    /// true=钉住 false=解除。
    pub pinned: bool,
    /// 事件时的组序号（解除时为解除前的 order）。
    pub order: u32,
}

/// 图钉标记消费面（判据「图钉/标记三处一致」的三个消费点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinSurface {
    /// 标题栏右上角。
    Titlebar,
    /// 任务栏缩略图。
    Thumbnail,
    /// Alt+Tab 列表。
    AltTab,
}

impl PinSurface {
    /// 三消费面全集（审计遍历序恒定）。
    pub const ALL: [PinSurface; 3] =
        [PinSurface::Titlebar, PinSurface::Thumbnail, PinSurface::AltTab];

    /// 该消费面的标记读数——三个面读**同一个** `is_pinned` 状态源
    /// （一处一事实，一致性由构造保证）。
    pub fn badge_on(self, wp: &WinPin, win: u32) -> bool {
        match self {
            PinSurface::Titlebar => wp.badge_titlebar(win),
            PinSurface::Thumbnail => wp.badge_thumbnail(win),
            PinSurface::AltTab => wp.badge_alttab(win),
        }
    }
}

/// 账本统计快照（诊断面：钉/解比例与触达窗口画像）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct LedgerStats {
    /// 钉住事件数。
    pub pinned_ct: u32,
    /// 解除事件数。
    pub unpinned_ct: u32,
    /// 账本内出现过的不重复窗口数。
    pub wins_touched: u32,
}

// ---------------------------------------------------------------------------
// 置顶状态机
// ---------------------------------------------------------------------------

/// 窗口置顶治理器：置顶组 + 独立焦点栈 + 三面标记 + 账本。
pub struct WinPin {
    pins: [Option<PinRec>; PIN_CAP],
    pin_counter: u32,
    /// 焦点栈：index 0 = 栈顶（最近点击），越后越旧。
    focus: [u32; FOCUS_CAP],
    focus_len: usize,
    ledger: RingLog<PinEvent, LEDGER_CAP>,
    /// 状态版本号（置顶/解除/焦点点击各推进——消费面刷新信号）。
    pub version: u32,
}

impl WinPin {
    pub fn new() -> WinPin {
        WinPin {
            pins: [const { None }; PIN_CAP],
            pin_counter: 0,
            focus: [0; FOCUS_CAP],
            focus_len: 0,
            ledger: RingLog::new(),
            version: 0,
        }
    }

    /// 钉住窗口（标题栏右键「窗口置顶」）。
    ///
    /// 结构性不抢焦点：本函数只写置顶组与账本，不存在触碰焦点栈的
    /// 代码路径。已置顶时为 no-op（返回 false）。
    pub fn pin(&mut self, win: u32, ts: u64) -> bool {
        if self.is_pinned(win) {
            return false;
        }
        for slot in self.pins.iter_mut() {
            if slot.is_none() {
                self.pin_counter = self.pin_counter.wrapping_add(1);
                let order = self.pin_counter;
                *slot = Some(PinRec { win, order, ts });
                self.version = self.version.wrapping_add(1);
                self.ledger.push(PinEvent { ts, win, pinned: true, order });
                return true;
            }
        }
        false // 表满显性拒绝（容量判据）
    }

    /// 解除置顶（再点一次图钉）。未置顶时为 no-op。
    pub fn unpin(&mut self, win: u32, ts: u64) -> bool {
        let mut order = 0u32;
        for slot in self.pins.iter_mut() {
            if let Some(r) = slot {
                if r.win == win {
                    order = r.order;
                    *slot = None;
                    self.version = self.version.wrapping_add(1);
                    self.ledger.push(PinEvent { ts, win, pinned: false, order });
                    return true;
                }
            }
        }
        false
    }

    /// 置顶与否——三面标记的唯一状态源。
    pub fn is_pinned(&self, win: u32) -> bool {
        self.pins.iter().flatten().any(|r| r.win == win)
    }

    /// 组内序号（后置顶在上：order 越大越靠上）。
    pub fn pin_order(&self, win: u32) -> Option<u32> {
        self.pins.iter().flatten().find(|r| r.win == win).map(|r| r.order)
    }

    pub fn pin_count(&self) -> usize {
        self.pins.iter().flatten().count()
    }

    /// 组序计数器现值（fuzz 单调性审计面）。
    pub fn pin_counter_value(&self) -> u32 {
        self.pin_counter
    }

    /// 焦点栈深度（诊断面）。
    pub fn focus_len(&self) -> usize {
        self.focus_len
    }

    /// 焦点栈第 i 位（0 = 栈顶；越界 None）。
    pub fn focus_at(&self, i: usize) -> Option<u32> {
        if i < self.focus_len {
            Some(self.focus[i])
        } else {
            None
        }
    }

    /// 置顶组列表（后置顶在前），写入 `out` 返回条数。
    pub fn pin_group(&self, out: &mut [u32]) -> usize {
        let mut recs = [const { None }; PIN_CAP];
        let mut k = 0usize;
        for r in self.pins.iter().flatten() {
            recs[k] = Some(*r);
            k += 1;
        }
        // 插入排序：order 降序（后置顶在前）。
        for i in 1..k {
            let cur = recs[i];
            let mut j = i;
            while j > 0 && recs[j - 1].unwrap().order < cur.unwrap().order {
                recs[j] = recs[j - 1];
                j -= 1;
            }
            recs[j] = cur;
        }
        let n = k.min(out.len());
        for (o, r) in out[..n].iter_mut().zip(recs.iter().flatten()) {
            *o = r.win;
        }
        n
    }

    /// 最终层级裁决：谁在视觉最上层。
    ///
    /// 判据语义：置顶组整体高于未置顶窗（即使后者 z 值更大）；两窗同
    /// 置顶比组序（后置顶在上）；两窗同未置顶比合成器 z 值（置顶组
    /// 不重建 z 栈，只做覆盖裁决）。
    pub fn effective_top(&self, wa: u32, za: u32, wb: u32, zb: u32) -> u32 {
        let (pa, pb) = (self.is_pinned(wa), self.is_pinned(wb));
        match (pa, pb) {
            (true, true) => {
                if self.pin_order(wa).unwrap_or(0) >= self.pin_order(wb).unwrap_or(0) {
                    wa
                } else {
                    wb
                }
            }
            (true, false) => wa,
            (false, true) => wb,
            (false, false) => {
                if za >= zb {
                    wa
                } else {
                    wb
                }
            }
        }
    }

    /// 阴影档（F226）：置顶浮层组阴影加深（3 档），常态 1 档。
    pub fn shadow_tier(&self, win: u32) -> u8 {
        if self.is_pinned(win) {
            SHADOW_TIER_PINNED
        } else {
            SHADOW_TIER_NORMAL
        }
    }

    /// 图钉标记 · 标题栏右上角（消费面 1）。
    pub fn badge_titlebar(&self, win: u32) -> bool {
        self.is_pinned(win)
    }

    /// 图钉标记 · 任务栏缩略图（消费面 2）。
    pub fn badge_thumbnail(&self, win: u32) -> bool {
        self.is_pinned(win)
    }

    /// 图钉标记 · Alt+Tab 列表（消费面 3）。
    pub fn badge_alttab(&self, win: u32) -> bool {
        self.is_pinned(win)
    }

    /// 三处一致性回归审计（同一状态源，构造上恒真；留给未来改结构时
    /// 兜底——若有人把三面拆成三份状态，这里率先变红）。
    pub fn badges_consistent(&self, win: u32) -> bool {
        let p = self.is_pinned(win);
        self.badge_titlebar(win) == p && self.badge_thumbnail(win) == p && self.badge_alttab(win) == p
    }

    /// Alt+Tab 列表标注面：对调用方给的窗口列表逐个产出置顶标记。
    pub fn alttab_flags(&self, wins: &[u32], out: &mut [bool]) -> usize {
        let n = wins.len().min(out.len());
        for (o, &w) in out[..n].iter_mut().zip(wins.iter()) {
            *o = self.badge_alttab(w);
        }
        n
    }

    // -- 焦点栈（与置顶组零耦合的独立结构） -------------------------------

    /// 点击窗口 → 焦点转移（最近点击去重栈，move-to-top）。
    /// 置顶窗点击其他窗口时焦点**正常走**——本函数不读置顶组。
    pub fn click_focus(&mut self, win: u32) -> u32 {
        // 去重：找到旧位置并左移抹掉。
        let mut old = None;
        for i in 0..self.focus_len {
            if self.focus[i] == win {
                old = Some(i);
                break;
            }
        }
        if let Some(i) = old {
            for j in i..self.focus_len - 1 {
                self.focus[j] = self.focus[j + 1];
            }
            self.focus_len -= 1;
        }
        // 腾出栈顶：满则挤掉栈底（最旧）。
        if self.focus_len == FOCUS_CAP {
            self.focus_len -= 1;
        }
        for j in (0..self.focus_len).rev() {
            self.focus[j + 1] = self.focus[j];
        }
        self.focus[0] = win;
        self.focus_len += 1;
        self.version = self.version.wrapping_add(1);
        win
    }

    /// 当前焦点窗（栈顶）。
    pub fn focus_top(&self) -> Option<u32> {
        if self.focus_len == 0 {
            None
        } else {
            Some(self.focus[0])
        }
    }

    /// 不抢焦点场景审计（判据本体的可执行形态）：
    /// 1) 焦点在 A；2) 钉住 B（焦点不动）；3) 点击 C（焦点转移 C）。
    pub fn audit_focus_not_stolen(&mut self, ts: u64) -> bool {
        self.click_focus(101);
        if self.focus_top() != Some(101) {
            return false;
        }
        let _ = self.pin(202, ts);
        if self.focus_top() != Some(101) {
            return false; // 钉住不得改变焦点
        }
        let _ = self.click_focus(303);
        self.focus_top() == Some(303) && self.is_pinned(202)
    }

    // -- 组序与裁决扩展 ----------------------------------------------------

    /// 组内最上窗（order 最大者；空组 None）——Alt+Tab「最近置顶」消费面。
    pub fn topmost_pinned(&self) -> Option<u32> {
        self.pins.iter().flatten().max_by_key(|r| r.order).map(|r| r.win)
    }

    /// 某窗是否为组内最上（单窗速查面）。
    pub fn is_topmost(&self, win: u32) -> bool {
        self.topmost_pinned() == Some(win)
    }

    /// 组序区间：(最小, 最大)；空组 None。
    pub fn order_range(&self) -> Option<(u32, u32)> {
        let mut lo = u32::MAX;
        let mut hi = 0u32;
        for r in self.pins.iter().flatten() {
            lo = lo.min(r.order);
            hi = hi.max(r.order);
        }
        if lo == u32::MAX {
            None
        } else {
            Some((lo, hi))
        }
    }

    /// 置顶窗清单（组内存储序——展示序请用 [`Self::pin_group`]）。
    pub fn pinned_wins(&self) -> Vec<u32> {
        self.pins.iter().flatten().map(|r| r.win).collect()
    }

    /// 全栈裁决（合成器消费面）：给 (窗口, z) 列表产出视觉叠放序
    /// （输出先位 = 最上）。裁决律：置顶组优先 → 组序大者优先 →
    /// z 大者优先 → 输入序稳定兜底——[`Self::effective_top`] 的全栈推广。
    pub fn effective_stack(&self, wins: &[u32], zs: &[u32]) -> Vec<u32> {
        let n = wins.len().min(zs.len());
        let mut rows: Vec<(u8, u32, u32, u32, u32)> = Vec::with_capacity(n);
        for i in 0..n {
            let w = wins[i];
            let pinned = if self.is_pinned(w) { 0u8 } else { 1u8 }; // 反键：0 在前
            let inv_ord = u32::MAX - self.pin_order(w).unwrap_or(0);
            let inv_z = u32::MAX - zs[i];
            rows.push((pinned, inv_ord, inv_z, i as u32, w));
        }
        rows.sort_by(|a, b| (a.0, a.1, a.2, a.3).cmp(&(b.0, b.1, b.2, b.3)));
        rows.iter().map(|r| r.4).collect()
    }

    /// 层级理由（属性页 tooltip/调试：该窗处于哪一层、为何）。
    pub fn layer_reason(&self, win: u32) -> &'static str {
        if self.is_pinned(win) {
            "浮层组（置顶，组序内后置顶在上）"
        } else {
            "常态层（合成器 z 序裁决）"
        }
    }

    /// 标题栏图钉悬停提示（置顶窗给语义锚文案，常态窗空串）。
    pub fn titlebar_hint(&self, win: u32) -> &'static str {
        if self.is_pinned(win) {
            PIN_HINT_DOC
        } else {
            ""
        }
    }

    // -- 批量操作 ----------------------------------------------------------

    /// 批量钉住（工作区恢复面），返回成功数。
    pub fn pin_many(&mut self, wins: &[u32], ts: u64) -> usize {
        let mut ok = 0;
        for &w in wins.iter() {
            if self.pin(w, ts) {
                ok += 1;
            }
        }
        ok
    }

    /// 解除全部置顶（工作区切换/清理面），返回解除数。
    pub fn unpin_all(&mut self, ts: u64) -> usize {
        let wins = self.pinned_wins();
        let mut ok = 0;
        for w in wins.iter() {
            if self.unpin(*w, ts) {
                ok += 1;
            }
        }
        ok
    }

    /// 解除除 keep 清单外的全部置顶（布局恢复面），返回解除数。
    pub fn unpin_except(&mut self, keep: &[u32], ts: u64) -> usize {
        let mut ok = 0;
        for w in self.pinned_wins().iter() {
            if !keep.contains(w) && self.unpin(*w, ts) {
                ok += 1;
            }
        }
        ok
    }

    // -- 账本读出扩展 ------------------------------------------------------

    /// 账本条数（容量有界性核对）。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 某窗的全部事件（旧→新）。
    pub fn events_for_win(&self, win: u32) -> Vec<PinEvent> {
        let mut evs: Vec<PinEvent> = self
            .ledger
            .newest_first()
            .iter()
            .copied()
            .filter(|e| e.win == win)
            .collect();
        evs.reverse();
        evs
    }

    /// 某窗最近一次事件。
    pub fn last_event_for(&self, win: u32) -> Option<PinEvent> {
        self.ledger.newest_first().iter().copied().find(|e| e.win == win)
    }

    /// 同窗「解除后再钉」次数（钉/解反复操作的画像面——提示过繁的
    /// 量化依据）。
    pub fn repins_for(&self, win: u32) -> u32 {
        let evs = self.events_for_win(win);
        let mut repins = 0u32;
        let mut prev_unpinned = false;
        for e in evs.iter() {
            if e.pinned {
                if prev_unpinned {
                    repins += 1;
                }
                prev_unpinned = false;
            } else {
                prev_unpinned = true;
            }
        }
        repins
    }

    /// 账本统计（钉/解计数 + 不重复触达窗口数——诊断面）。
    pub fn ledger_stats(&self) -> LedgerStats {
        let evs = self.ledger.newest_first();
        let mut st = LedgerStats::default();
        let mut wins: Vec<u32> = Vec::new();
        for e in evs.iter() {
            if e.pinned {
                st.pinned_ct += 1;
            } else {
                st.unpinned_ct += 1;
            }
            if !wins.contains(&e.win) {
                wins.push(e.win);
            }
        }
        st.wins_touched = wins.len() as u32;
        st
    }

    /// 事件回放（诊断/同步面）：把一段账本事件流重演到当前状态——
    /// 钉/解走公共路径（order 由现计数器分配，不改写历史组序）。
    pub fn replay(&mut self, events: &[PinEvent]) -> usize {
        let mut done = 0;
        for ev in events.iter() {
            let ok = if ev.pinned {
                self.pin(ev.win, ev.ts)
            } else {
                self.unpin(ev.win, ev.ts)
            };
            if ok {
                done += 1;
            }
        }
        done
    }

    /// 组内登记全量导出（诊断面；非持久化——重启不记忆置顶）。
    pub fn debug_dump_group(&self) -> Vec<PinRec> {
        let mut recs: Vec<PinRec> = self.pins.iter().flatten().copied().collect();
        recs.sort_by_key(|r| core::cmp::Reverse(r.order));
        recs
    }

    // -- 焦点栈扩展（与置顶组零耦合） --------------------------------------

    /// 次新焦点窗（Alt+Tab 第一切的语义：栈顶之下的那个）。
    pub fn focus_prev(&self) -> Option<u32> {
        if self.focus_len >= 2 {
            Some(self.focus[1])
        } else {
            None
        }
    }

    /// 焦点栈导出（栈顶在先），写入 out 返回条数。
    pub fn focus_stack(&self, out: &mut [u32]) -> usize {
        let n = self.focus_len.min(out.len());
        out[..n].copy_from_slice(&self.focus[..n]);
        n
    }

    /// 窗口是否在焦点栈中。
    pub fn focus_contains(&self, win: u32) -> bool {
        self.focus[..self.focus_len].contains(&win)
    }

    // -- 审计面 ------------------------------------------------------------

    /// 三面标记多窗一致性批量审计（[`Self::badges_consistent`] 的列表级
    /// 推广——「三处一致」判据的批量走查面）。
    pub fn badges_consistent_many(&self, wins: &[u32]) -> bool {
        wins.iter().all(|&w| self.badges_consistent(w))
    }

    /// 三面标记矩阵：窗口清单 × 三消费面（一致性走查的数据面）。
    pub fn badge_matrix(&self, wins: &[u32], out: &mut [(bool, bool, bool)]) -> usize {
        let n = wins.len().min(out.len());
        for (o, &w) in out[..n].iter_mut().zip(wins.iter()) {
            *o = (self.badge_titlebar(w), self.badge_thumbnail(w), self.badge_alttab(w));
        }
        n
    }

    /// 组序输出审计：`pin_group` 输出严格按 order 降序（「后置顶在上」
    /// 的独立复核面，与 fuzz 镜像法互补）。
    pub fn audit_group_sorted(&self) -> bool {
        let mut out = [0u32; PIN_CAP];
        let n = self.pin_group(&mut out);
        for k in 1..n {
            let a = self.pin_order(out[k - 1]).unwrap_or(0);
            let b = self.pin_order(out[k]).unwrap_or(0);
            if a <= b {
                return false;
            }
        }
        true
    }

    /// 全量不变式审计（回归兜底面）：组序唯一、三面标记对全部置顶窗
    /// 一致、焦点栈无重复——任何一处被未来改动破坏，这里率先变红。
    pub fn audit_invariants(&self) -> bool {
        let recs: Vec<PinRec> = self.pins.iter().flatten().copied().collect();
        for i in 0..recs.len() {
            for j in (i + 1)..recs.len() {
                if recs[i].order == recs[j].order {
                    return false;
                }
            }
        }
        for r in recs.iter() {
            if !self.badges_consistent(r.win) {
                return false;
            }
        }
        for i in 0..self.focus_len {
            for j in (i + 1)..self.focus_len {
                if self.focus[i] == self.focus[j] {
                    return false;
                }
            }
        }
        true
    }

    // -- 持久化（重启不记忆置顶） -----------------------------------------

    /// 持久化快照：只含版本号——置顶组结构性不入快照（判据「重启不
    /// 记忆置顶」的架构面，文档见 [`REBOOT_FORGET_DOC`]）。
    pub fn encode(&self) -> [u8; 8] {
        let mut out = [0u8; 8];
        out[0] = b'V';
        out[1] = b'P';
        out[2] = b'I';
        out[3] = b'N';
        out[4..8].copy_from_slice(&self.version.to_le_bytes());
        out
    }

    /// 从快照恢复（启动路径）：只回放版本号；置顶组保持空——全部窗口
    /// 回常态层。魔数不符显性拒绝。
    pub fn decode_restore(&mut self, blob: &[u8; 8]) -> bool {
        if blob[0..4] != [b'V', b'P', b'I', b'N'] {
            return false;
        }
        self.version = u32::from_le_bytes([blob[4], blob[5], blob[6], blob[7]]);
        true
    }

    /// 快照恢复（只在新快照版本更新时应用——旧快照重放不倒退审计面；
    /// 置顶组同样保持空，判据「重启回常态」不受版本号影响）。
    pub fn restore_if_newer(&mut self, blob: &[u8; 8]) -> bool {
        if blob[0..4] != [b'V', b'P', b'I', b'N'] {
            return false;
        }
        let ver = u32::from_le_bytes([blob[4], blob[5], blob[6], blob[7]]);
        if ver > self.version {
            self.version = ver;
            true
        } else {
            false
        }
    }

    /// 账本读出（新→旧，诊断/设置面）。
    pub fn recent_events(&self) -> [Option<PinEvent>; LEDGER_CAP] {
        let mut out = [None; LEDGER_CAP];
        for (k, ev) in self.ledger.newest_first().iter().enumerate() {
            out[k] = Some(*ev);
        }
        out
    }
}

impl Default for WinPin {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F248 自检（判据：不抢焦点、三处一致、后置顶在上、重启不记忆；含
/// xors32 fuzz）。
pub fn run_winpin_checks() -> CheckSet {
    let mut set = CheckSet::new("F248-winpin");

    // 1. 钉住/解除基本流转：状态翻转、账本留痕、版本推进。
    let mut wp = WinPin::new();
    let p1 = wp.pin(10, 100);
    let p2 = wp.pin(10, 150); // 重复钉住 no-op
    let u1 = wp.unpin(10, 200);
    let u2 = wp.unpin(10, 250); // 未置顶解除 no-op
    set.add(
        "pin/unpin flow & ledger",
        p1 && !p2 && u1 && !u2
            && !wp.is_pinned(10)
            && wp.recent_events()[1] == Some(PinEvent { ts: 100, win: 10, pinned: true, order: 1 })
            && wp.recent_events()[0] == Some(PinEvent { ts: 200, win: 10, pinned: false, order: 1 }),
        "",
    );

    // 2. 多窗置顶叠加顺序：A→B→C 依次钉住，组序 = [C,B,A]（后置顶在上）。
    let mut wp2 = WinPin::new();
    let _ = wp2.pin(1, 10);
    let _ = wp2.pin(2, 20);
    let _ = wp2.pin(3, 30);
    let mut group = [0u32; 8];
    let n = wp2.pin_group(&mut group);
    set.add(
        "later pin stacks on top",
        n == 3 && group[0] == 3 && group[1] == 2 && group[2] == 1,
        "",
    );

    // 3. 解除回落 + 重新置顶排到最上：unpin(2) → [3,1]；解除 1 后重钉
    //    → [1,3]。
    let _ = wp2.unpin(2, 40);
    // 修障登记（check 3 红）：pin 对已置顶窗口是 no-op（返回 false），
    // 旧场景未先解除窗口 1 就「重新置顶」，重钉不会发生、组序停在
    // [3,1]——补 unpin(1) 使「解除→重钉」闭环，并以两步成功位补强。
    let unpin1 = wp2.unpin(1, 45);
    let repin1 = wp2.pin(1, 50);
    let n2 = wp2.pin_group(&mut group);
    set.add(
        "unpin reflows, re-pin goes top",
        n2 == 2 && group[0] == 1 && group[1] == 3 && wp2.pin_count() == 2
            && unpin1 && repin1,
        "",
    );

    // 4. 不抢焦点（判据本体）：场景审计——焦点在 A、钉住 B 焦点不动、
    //    点击 C 焦点转移 C。
    let mut wp3 = WinPin::new();
    set.add(
        "pin never steals focus (scenario)",
        wp3.audit_focus_not_stolen(100),
        "",
    );

    // 5. 钉住事件不触碰焦点栈的直接断言：pin 前后 focus_top 相同。
    let mut wp4 = WinPin::new();
    let _ = wp4.click_focus(77);
    let before = wp4.focus_top();
    let _ = wp4.pin(88, 10);
    set.add(
        "pin leaves focus stack untouched",
        before == Some(77) && wp4.focus_top() == Some(77),
        "",
    );

    // 6. 图钉三处一致（标题栏/缩略图/Alt+Tab 同源）：置顶与解除两态核对。
    let _ = wp4.pin(88, 20);
    let pinned_ok = wp4.badges_consistent(88)
        && wp4.badge_titlebar(88)
        && wp4.badge_thumbnail(88)
        && wp4.badge_alttab(88);
    let _ = wp4.unpin(88, 30);
    set.add(
        "pin badge consistent across three surfaces",
        pinned_ok && !wp4.badges_consistent(88) == false && wp4.badges_consistent(88),
        "",
    );

    // 7. 阴影加深：置顶 3 档、常态 1 档。
    let _ = wp4.pin(88, 40);
    set.add(
        "shadow tier deepens when pinned",
        wp4.shadow_tier(88) == SHADOW_TIER_PINNED
            && wp4.shadow_tier(77) == SHADOW_TIER_NORMAL,
        "",
    );

    // 8. 层级裁决：置顶压过高 z 未置顶窗；双置顶比组序；双未置顶比 z。
    let mut wp5 = WinPin::new();
    let _ = wp5.pin(1, 10); // order 1
    let _ = wp5.pin(2, 20); // order 2（后置顶在上）
    let t1 = wp5.effective_top(1, 999, 30, 5); // 置顶 vs 未置顶
    let t2 = wp5.effective_top(1, 0, 2, 0); // 双置顶 → 组序大者
    let t3 = wp5.effective_top(30, 5, 40, 9); // 双未置顶 → z 大者
    let t4 = wp5.effective_top(30, 999, 1, 0); // 未置顶高 z vs 置顶
    set.add(
        "effective top: pinned group overrides z",
        t1 == 1 && t2 == 2 && t3 == 40 && t4 == 1,
        "",
    );

    // 9. Alt+Tab 列表标注：给定窗口清单的标记与状态源全一致。
    let list = [1u32, 2, 30, 40];
    let mut flags = [false; 4];
    let m = wp5.alttab_flags(&list, &mut flags);
    set.add(
        "alttab flags match pin state",
        m == 4 && flags[0] && flags[1] && !flags[2] && !flags[3] && wp5.badges_consistent(2),
        "",
    );

    // 10. 重启不记忆置顶：快照 → 新实例恢复 → 置顶组为空 + 文档锚。
    let _ = wp5.pin(99, 60);
    let blob = wp5.encode();
    let mut wp6 = WinPin::new();
    let ok = wp6.decode_restore(&blob);
    set.add(
        "reboot forgets pins (documented)",
        ok
            && wp6.pin_count() == 0
            && !wp6.is_pinned(99)
            && REBOOT_FORGET_DOC.contains("重启")
            && REBOOT_FORGET_DOC.contains("常态"),
        "",
    );

    // 11. 容量 64 显性拒绝：第 65 个钉住被拒且不留痕。
    let mut wp7 = WinPin::new();
    let mut ok = true;
    for w in 0..PIN_CAP as u32 {
        ok &= wp7.pin(w, w as u64);
    }
    set.add(
        "pin table full rejects explicitly",
        ok
            && wp7.pin_count() == PIN_CAP
            && !wp7.pin(9999, 9999)
            && wp7.pin_count() == PIN_CAP,
        "",
    );

    // 12. xors32 fuzz：随机钉住/解除/点击流（16 窗口小宇宙）——钉住
    //     返回值与镜像集合一致、组序相邻对 order 降序、focus_top 恒等
    //     于最近点击者、pin_counter 单调、无 panic。
    let mut x: u32 = 0x2545_F491;
    let mut fz = WinPin::new();
    let mut mirror = [false; 16];
    let mut last_click: Option<u32> = None;
    let mut prev_counter = fz.pin_counter_value();
    let mut survived = true;
    let mut ts: u64 = 0;
    for _ in 0..3000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let win = 100 + (x % 16) as u32;
        let i = (win - 100) as usize;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        ts += (x % 30 + 1) as u64;
        match x % 3 {
            0 => {
                let want = !mirror[i];
                let got = fz.pin(win, ts);
                if got != want {
                    survived = false;
                }
                if got {
                    mirror[i] = true;
                }
            }
            1 => {
                let want = mirror[i];
                let got = fz.unpin(win, ts);
                if got != want {
                    survived = false;
                }
                if got {
                    mirror[i] = false;
                }
            }
            _ => {
                fz.click_focus(win);
                last_click = Some(win);
            }
        }
        if fz.pin_counter_value() < prev_counter {
            survived = false;
        }
        prev_counter = fz.pin_counter_value();
        if let Some(lc) = last_click {
            if fz.focus_top() != Some(lc) {
                survived = false;
            }
        }
    }
    let mut group = [0u32; PIN_CAP];
    let n = fz.pin_group(&mut group);
    let mut group_ok = n == mirror.iter().filter(|&&p| p).count();
    for k in 1..n {
        if fz.pin_order(group[k - 1]).unwrap_or(0) < fz.pin_order(group[k]).unwrap_or(0) {
            group_ok = false;
        }
    }
    set.add(
        "fuzz 3000 ops mirror & ordering hold",
        survived && group_ok,
        "",
    );

    // 13. 全栈裁决与消费面枚举：置顶组整体压在常态层之上，组序内后置顶
    //     在上；三消费面枚举读同一状态源；层级理由/悬停提示同源。
    let mut wp8 = WinPin::new();
    let _ = wp8.pin(11, 10); // order 1
    let _ = wp8.pin(12, 20); // order 2（后置顶在上）
    let stack = wp8.effective_stack(&[11, 12, 13, 14], &[9, 1, 100, 200]);
    let mut marks = [(false, false, false); 4];
    let mrows = wp8.badge_matrix(&[11, 12, 13, 14], &mut marks);
    let mut surf_ok = true;
    for s in PinSurface::ALL {
        if s.badge_on(&wp8, 12) != wp8.is_pinned(12) {
            surf_ok = false;
        }
        if s.badge_on(&wp8, 13) != wp8.is_pinned(13) {
            surf_ok = false;
        }
    }
    set.add(
        "effective stack & surface enum & hints",
        stack == [12, 11, 14, 13]
            && mrows == 4
            && marks[0] == (true, true, true)
            && marks[3] == (false, false, false)
            && surf_ok
            && wp8.is_topmost(12)
            && !wp8.is_topmost(11)
            && wp8.order_range() == Some((1, 2))
            && wp8.topmost_pinned() == Some(12)
            && wp8.titlebar_hint(11).contains("不会抢占焦点")
            && wp8.titlebar_hint(13) == ""
            && wp8.layer_reason(11).contains("浮层组")
            && wp8.layer_reason(13).contains("常态层")
            && wp8.badges_consistent_many(&[11, 12, 13, 14]),
        "",
    );

    // 14. 批量钉/解、事件回放、账本统计、不变式与组序审计、焦点栈扩展。
    let mut wp9 = WinPin::new();
    let many = wp9.pin_many(&[21, 22, 23, 24], 10);
    let except = wp9.unpin_except(&[22], 20);
    let all_left = wp9.pin_count() == 1 && wp9.is_pinned(22);
    let _ = wp9.pin(23, 30);
    let events = wp9.events_for_win(23);
    let replayed = {
        let mut fresh = WinPin::new();
        let _ = fresh.pin(21, 1);
        let n = fresh.replay(&events);
        n == 3 && fresh.is_pinned(21) && fresh.is_pinned(23)
    };
    let st = wp9.ledger_stats();
    let inv = wp9.audit_invariants() && wp9.audit_group_sorted();
    let _ = wp9.click_focus(21);
    let _ = wp9.click_focus(23);
    let _ = wp9.click_focus(21);
    let mut fstack = [0u32; FOCUS_CAP];
    let fl = wp9.focus_stack(&mut fstack);
    let dump_ok = {
        let dump = wp9.debug_dump_group();
        dump.len() == wp9.pin_count() && dump[0].win == 23
    };
    let mut boot = WinPin::new();
    let restored = boot.restore_if_newer(&wp9.encode());
    set.add(
        "pin_many/unpin_except & replay & stats & invariants",
        many == 4
            && except == 3
            && all_left
            && events.len() == 3
            && events[0].pinned
            && !events[1].pinned
            && events[2].pinned
            && replayed
            && st.pinned_ct >= 5
            && st.unpinned_ct >= 3
            && st.wins_touched >= 4
            && wp9.last_event_for(24).unwrap().pinned == false
            && wp9.repins_for(23) == 1
            && wp9.ledger_len() >= 8
            && inv
            && wp9.focus_prev() == Some(23)
            && wp9.focus_contains(21)
            && !wp9.focus_contains(99)
            && fl == 2
            && fstack[0] == 21
            && dump_ok
            && restored
            && boot.version == wp9.version
            && boot.pin_count() == 0,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_twice_is_noop_and_keeps_first_order() {
        let mut wp = WinPin::new();
        assert!(wp.pin(5, 10));
        let o1 = wp.pin_order(5);
        assert!(!wp.pin(5, 20), "重复钉住应 no-op");
        assert_eq!(wp.pin_order(5), o1, "重复钉住不得推进组序");
        assert_eq!(wp.pin_count(), 1);
    }

    #[test]
    fn effective_top_tie_breaks() {
        let wp = WinPin::new();
        // 双未置顶同 z：先到者胜（稳定裁决）。
        assert_eq!(wp.effective_top(7, 5, 8, 5), 7);
        assert_eq!(wp.effective_top(7, 3, 8, 9), 8);
    }

    #[test]
    fn focus_stack_dedup_and_cap() {
        let mut wp = WinPin::new();
        for w in 1..=20u32 {
            wp.click_focus(w);
        }
        assert_eq!(wp.focus_top(), Some(20));
        assert_eq!(wp.focus_len(), FOCUS_CAP, "容量裁剪：只留最近 16");
        // 重新点击旧窗 → 移到栈顶（不重复入栈）。
        wp.click_focus(5);
        assert_eq!(wp.focus_top(), Some(5));
        // 栈内无重复。
        let mut seen = [false; 64];
        for i in 0..wp.focus_len() {
            let w = wp.focus_at(i).unwrap() as usize;
            assert!(w < 64 && !seen[w], "焦点栈出现重复窗口");
            seen[w] = true;
        }
    }

    #[test]
    fn decode_rejects_bad_magic() {
        let mut wp = WinPin::new();
        assert!(!wp.decode_restore(&[0, 0, 0, 0, 1, 0, 0, 0]));
        let _ = wp.pin(1, 1);
        let blob = wp.encode();
        assert!(blob[0..4] == *b"VPIN");
        let mut wp2 = WinPin::new();
        assert!(wp2.decode_restore(&blob));
        assert_eq!(wp2.pin_count(), 0, "重启不记忆置顶");
    }

    #[test]
    fn effective_stack_full_ordering() {
        let mut wp = WinPin::new();
        let _ = wp.pin(101, 1); // order 1
        let _ = wp.pin(102, 2); // order 2（后置顶在上）
        // 置顶组整体在常态层之上；组内后置顶在上；常态层内 z 降序；
        // 输入顺序不影响裁决（稳定排序的输入无关性抽查）。
        assert_eq!(wp.effective_stack(&[103, 101, 102, 104], &[5, 9, 1, 50]), [102, 101, 104, 103]);
        assert_eq!(wp.effective_stack(&[104, 103, 102, 101], &[50, 5, 1, 9]), [102, 101, 104, 103]);
        // 空组与单窗退化。
        let empty = WinPin::new();
        assert_eq!(empty.effective_stack(&[7, 8], &[2, 9]), [8, 7]);
        assert_eq!(empty.effective_stack(&[7], &[3]), [7]);
        // 组序区间与最上窗。
        assert_eq!(wp.order_range(), Some((1, 2)));
        assert_eq!(wp.topmost_pinned(), Some(102));
        assert!(wp.is_topmost(102) && !wp.is_topmost(101));
        // 组序输出审计与不变式全绿。
        assert!(wp.audit_group_sorted());
        assert!(wp.audit_invariants());
    }

    #[test]
    fn batch_ops_replay_and_stats() {
        let mut wp = WinPin::new();
        assert_eq!(wp.pin_many(&[1, 2, 3, 4, 5], 10), 5);
        // 保留 2、3，解除其余 3 窗。
        assert_eq!(wp.unpin_except(&[2, 3], 20), 3);
        assert_eq!(wp.pin_count(), 2);
        assert!(wp.is_pinned(2) && wp.is_pinned(3) && !wp.is_pinned(1));
        // 解除 3 再钉回 → repins=1；事件流：pin, unpin, pin（旧→新）。
        assert!(wp.unpin(3, 30));
        assert!(wp.pin(3, 40));
        assert_eq!(wp.repins_for(3), 1);
        let evs = wp.events_for_win(3);
        assert_eq!(evs.len(), 3);
        assert!(evs[0].pinned && !evs[1].pinned && evs[2].pinned);
        // 全清 → 0；账本统计与实际事件数对账（6 钉 + 6 解 = 12 条）。
        assert_eq!(wp.unpin_all(50), 2);
        assert_eq!(wp.pin_count(), 0);
        let st = wp.ledger_stats();
        assert_eq!(st.pinned_ct, 6);
        assert_eq!(st.unpinned_ct, 6);
        assert_eq!(st.wins_touched, 5);
        assert_eq!(wp.ledger_len(), 12);
        // 回放整段账本（旧→新）到新实例 → 终态与源一致（全解）。
        let mut events: Vec<PinEvent> = wp
            .recent_events()
            .iter()
            .flatten()
            .copied()
            .collect();
        events.reverse();
        let mut fresh = WinPin::new();
        assert_eq!(fresh.replay(&events), 12);
        assert_eq!(fresh.pin_count(), 0);
        // 不变式与多窗三面一致性。
        assert!(wp.audit_invariants());
        assert!(wp.badges_consistent_many(&[1, 2, 3, 4, 5, 99]));
        // 悬停提示与层级理由同源。
        let _ = wp.pin(7, 50);
        assert!(wp.titlebar_hint(7).contains("不会抢占焦点"));
        assert_eq!(wp.titlebar_hint(8), "");
        assert!(wp.layer_reason(7).contains("浮层组"));
    }

    #[test]
    fn winpin_selfcheck_all_green() {
        let set = run_winpin_checks();
        assert!(set.all_passed(), "F248 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 8 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
// 主册锚 F248（窗口置顶）。v2 三件事：
// 1) 持久化 I/O：诊断快照册（版本号 + 账本计数）v2 定长容器序列化——
//    magic b"VXH1" + 版本 1 + 定长 payload + FNV-1a 校验和，四类损坏
//    显性拒绝；置顶组**结构性不入容器**（重启不记忆置顶判据在 v2 容器
//    上再锚定，与既有 VPIN 8 字节位包并存于追加段）；
// 2) UI 壳接线：任务栏槽位几何（命中测试）+ Alt+Tab 行清单（图钉标记
//    直读三面同源状态）——「置顶状态在任务栏缩略图/Alt+Tab 上有标记」
//    的几何承载；
// 3) 判定面扩展：run_winpin_v2_checks，首条即持久化 round-trip。

// -- 持久化 I/O 面 ---------------------------------------------------------

/// v2 容器 payload 定长：版本号 u32 + 钉住计数 u32 + 解除计数 u32。
pub const VX2_WP_PAYLOAD: usize = 12;
/// v2 容器全长 = magic 4 + version 1 + payload + checksum 4。
pub const VX2_WP_BLOB: usize = 9 + VX2_WP_PAYLOAD;

/// v2 损坏分类（显性拒绝面——各归其名，不静默回默认）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vx2Error {
    BadMagic,
    BadVersion,
    /// 总长 ≠ 定长容器。
    BadLength,
    BadChecksum,
}

/// FNV-1a 32 位校验和（offset 0x811C9DC5、素数 0x01000193）。
fn vx2_fnv(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 诊断快照册（遥测面）：只含版本号与账本计数——置顶组结构性缺席。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WinPinDumpBook {
    pub version: u32,
    pub pinned_ct: u32,
    pub unpinned_ct: u32,
}

impl WinPinDumpBook {
    /// 从治理器读出（账本计数取 ledger_stats；置顶组不读——结构缺席）。
    pub fn snapshot(wp: &WinPin) -> WinPinDumpBook {
        let st = wp.ledger_stats();
        WinPinDumpBook { version: wp.version, pinned_ct: st.pinned_ct, unpinned_ct: st.unpinned_ct }
    }

    /// 推到治理器：只同步版本号（旧快照不倒退）；置顶组保持空——
    /// 判据「重启不记忆置顶」由本函数的结构构造保证。
    pub fn apply_to(&self, wp: &mut WinPin) -> bool {
        let mut blob = [0u8; 8];
        blob[0..4].copy_from_slice(b"VPIN");
        blob[4..8].copy_from_slice(&self.version.to_le_bytes());
        wp.restore_if_newer(&blob)
    }

    /// 序列化：b"VXH1" + 版本 1 + 定长 payload + FNV-1a。缓冲不足返回 0。
    pub fn to_bytes(&self, out: &mut [u8]) -> usize {
        if out.len() < VX2_WP_BLOB {
            return 0;
        }
        out[0..4].copy_from_slice(b"VXH1");
        out[4] = 1;
        out[5..9].copy_from_slice(&self.version.to_le_bytes());
        out[9..13].copy_from_slice(&self.pinned_ct.to_le_bytes());
        out[13..17].copy_from_slice(&self.unpinned_ct.to_le_bytes());
        let crc = vx2_fnv(&out[..9 + VX2_WP_PAYLOAD - 4]);
        out[9 + VX2_WP_PAYLOAD - 4..9 + VX2_WP_PAYLOAD].copy_from_slice(&crc.to_le_bytes());
        VX2_WP_BLOB
    }

    /// 反序列化：四类损坏显性拒绝。
    pub fn from_bytes(blob: &[u8]) -> Result<WinPinDumpBook, Vx2Error> {
        if blob.len() != VX2_WP_BLOB {
            return Err(Vx2Error::BadLength);
        }
        if blob[0..4] != *b"VXH1" {
            return Err(Vx2Error::BadMagic);
        }
        if blob[4] != 1 {
            return Err(Vx2Error::BadVersion);
        }
        let end = 9 + VX2_WP_PAYLOAD;
        let crc = u32::from_le_bytes([blob[end - 4], blob[end - 3], blob[end - 2], blob[end - 1]]);
        if vx2_fnv(&blob[..end - 4]) != crc {
            return Err(Vx2Error::BadChecksum);
        }
        Ok(WinPinDumpBook {
            version: u32::from_le_bytes([blob[5], blob[6], blob[7], blob[8]]),
            pinned_ct: u32::from_le_bytes([blob[9], blob[10], blob[11], blob[12]]),
            unpinned_ct: u32::from_le_bytes([blob[13], blob[14], blob[15], blob[16]]),
        })
    }
}

// -- UI 壳接线面 -----------------------------------------------------------

/// 任务栏槽规格（px）——v2 布局常量：F248 槽 40 宽、条 48 高。
pub const VX2_SLOT_W: i32 = 40;
pub const VX2_TASKBAR_H: i32 = 48;
/// Alt+Tab 行高（px）。
pub const VX2_ALT_ROW_H: i32 = 36;

/// 任务栏槽位矩形（左起第 slot 槽，图标槽垂直居中）。
pub fn taskbar_slot_rect(slot: usize) -> crate::h1star::h1base::Rect {
    crate::h1star::h1base::Rect::new(
        slot as i32 * VX2_SLOT_W,
        (VX2_TASKBAR_H - VX2_SLOT_W) / 2,
        VX2_SLOT_W,
        VX2_SLOT_W,
    )
}

/// 任务栏槽位命中测试 → 槽下标（条带外 None）。
pub fn taskbar_slot_hit(slots: usize, px: i32, py: i32) -> Option<usize> {
    (0..slots).find(|&s| {
        let r = taskbar_slot_rect(s);
        px >= r.x && px < r.right() && py >= r.y && py < r.bottom()
    })
}

/// Alt+Tab 行绘制条目：行矩形 + 图钉标记（直读三面同源状态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AltRow {
    pub win: u32,
    pub y: i32,
    pub h: i32,
    pub pinned: bool,
}

/// 生成 Alt+Tab 行清单（列表序 = 输入序；标记读 badge_alttab——
/// 渲染面不自行判定）。
pub fn alttab_rows(wp: &WinPin, wins: &[u32], out: &mut [AltRow]) -> usize {
    let m = wins.len().min(out.len());
    for k in 0..m {
        out[k] = AltRow {
            win: wins[k],
            y: k as i32 * VX2_ALT_ROW_H,
            h: VX2_ALT_ROW_H,
            pinned: wp.badge_alttab(wins[k]),
        };
    }
    m
}

/// Alt+Tab 行命中测试（面板内坐标；x ∈ [0, w) 且落在行内）。
pub fn alttab_row_hit(rows: &[AltRow], n: usize, px: i32, py: i32, w: i32) -> Option<usize> {
    (0..n.min(rows.len())).find(|&k| px >= 0 && px < w && py >= rows[k].y && py < rows[k].y + rows[k].h)
}

// -- 判定面扩展 ------------------------------------------------------------

/// F248 v2 自检（锚注见各条注释；首条 = 持久化 round-trip）。
pub fn run_winpin_v2_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("F248-winpin-v2");

    // 1. 持久化 round-trip：诊断册编→解→推新治理器→版本同步且置顶组
    //    保持空（重启不记忆置顶判据在 v2 容器上的再锚定）。
    let mut src = WinPin::new();
    let _ = src.pin(10, 100);
    let _ = src.pin(20, 200);
    let _ = src.unpin(10, 300);
    let _ = src.click_focus(30);
    let book = WinPinDumpBook::snapshot(&src);
    let mut buf = [0u8; VX2_WP_BLOB];
    let len = book.to_bytes(&mut buf);
    let mut dst = WinPin::new();
    match WinPinDumpBook::from_bytes(&buf[..len]) {
        Ok(b2) => {
            let ok = b2 == book && b2.apply_to(&mut dst);
            set.add(
                "v2 persistence round-trip",
                ok && dst.version == src.version && dst.pin_count() == 0
                    && !dst.is_pinned(20) && dst.focus_top().is_none(),
                "",
            );
        }
        Err(_) => set.add("v2 persistence round-trip", false, ""),
    }

    // 2. 四类损坏显性拒绝（截断 / magic / 版本 / payload 翻位）。
    let mut m = buf;
    m[0] = b'X';
    let mut v = buf;
    v[4] = 4;
    let mut c = buf;
    c[9] ^= 0xFF;
    set.add(
        "v2 corruption explicitly rejected",
        WinPinDumpBook::from_bytes(&buf[..len - 1]) == Err(Vx2Error::BadLength)
            && WinPinDumpBook::from_bytes(&m) == Err(Vx2Error::BadMagic)
            && WinPinDumpBook::from_bytes(&v) == Err(Vx2Error::BadVersion)
            && WinPinDumpBook::from_bytes(&c) == Err(Vx2Error::BadChecksum),
        "",
    );

    // 3. 任务栏槽位几何：槽 0 贴左缘、相邻槽不重叠、命中界内/界外。
    let s0 = taskbar_slot_rect(0);
    let s1 = taskbar_slot_rect(1);
    set.add(
        "v2 taskbar slot geometry & hit",
        s0.x == 0 && s1.x == VX2_SLOT_W && s1.x >= s0.right()
            && taskbar_slot_hit(3, VX2_SLOT_W + 5, VX2_TASKBAR_H / 2) == Some(1)
            && taskbar_slot_hit(3, -1, VX2_TASKBAR_H / 2).is_none()
            && taskbar_slot_hit(3, 5, VX2_TASKBAR_H + 5).is_none(),
        "",
    );

    // 4. Alt+Tab 行清单：图钉标记与状态源一致、行距铺排、命中测试。
    let mut wp4 = WinPin::new();
    let _ = wp4.pin(11, 10);
    let _ = wp4.pin(12, 20);
    let wins = [11u32, 12, 13];
    let mut rows = [AltRow { win: 0, y: 0, h: 0, pinned: false }; 8];
    let rn = alttab_rows(&wp4, &wins, &mut rows);
    set.add(
        "v2 alttab rows pinned flags & hit",
        rn == 3 && rows[0].pinned && rows[1].pinned && !rows[2].pinned
            && rows[2].y == 2 * VX2_ALT_ROW_H
            && alttab_row_hit(&rows, rn, 60, VX2_ALT_ROW_H + 5, 300) == Some(1)
            && alttab_row_hit(&rows, rn, 60, -1, 300).is_none(),
        "",
    );

    // 5. xors32 fuzz 500 轮：随机诊断册 round-trip 逐字段相等、payload
    //    任一字节翻位必被校验和捕获。
    let mut x: u32 = 0x2488_D4FA;
    let mut ok = true;
    for _ in 0..500u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let b = WinPinDumpBook { version: x, pinned_ct: x % 64, unpinned_ct: (x >> 6) % 64 };
        let mut tbuf = [0u8; VX2_WP_BLOB];
        ok &= b.to_bytes(&mut tbuf) == VX2_WP_BLOB && WinPinDumpBook::from_bytes(&tbuf) == Ok(b);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        tbuf[5 + (x as usize) % VX2_WP_PAYLOAD] ^= 0x22;
        ok &= WinPinDumpBook::from_bytes(&tbuf) == Err(Vx2Error::BadChecksum);
    }
    set.add("v2 fuzz 500 round-trips & checksum", ok, "");

    set
}

// ---------------------------------------------------------------------------
// v2 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_dump_book_roundtrip_and_reject() {
        let mut wp = WinPin::new();
        let _ = wp.pin(7, 1);
        let _ = wp.pin(8, 2);
        let _ = wp.unpin(7, 3);
        let b = WinPinDumpBook::snapshot(&wp);
        assert_eq!(b.pinned_ct, 2);
        assert_eq!(b.unpinned_ct, 1);
        let mut buf = [0u8; VX2_WP_BLOB];
        assert_eq!(b.to_bytes(&mut buf), VX2_WP_BLOB);
        assert_eq!(WinPinDumpBook::from_bytes(&buf), Ok(b));
        let mut bad = buf;
        bad[11] ^= 0x01;
        assert_eq!(WinPinDumpBook::from_bytes(&bad), Err(Vx2Error::BadChecksum));
        assert_eq!(WinPinDumpBook::from_bytes(&buf[..9]), Err(Vx2Error::BadLength));
    }

    #[test]
    fn v2_dump_never_restores_pins() {
        let mut src = WinPin::new();
        let _ = src.pin(99, 1);
        let b = WinPinDumpBook::snapshot(&src);
        let mut dst = WinPin::new();
        assert!(b.apply_to(&mut dst));
        assert_eq!(dst.pin_count(), 0, "v2 容器同样结构性不携带置顶组");
        assert!(!dst.is_pinned(99));
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_winpin_v2_checks();
        assert!(set.all_passed(), "F248 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
