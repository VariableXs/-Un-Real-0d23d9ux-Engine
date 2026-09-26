//! F237 窗口位置与尺寸记忆 · 判据实装（H 基础通用域）。
//!
//! **判据锚**：F237（主册 H-1 深化设计报告 · H 基础通用域）。
//!
//! **验收标准（主册第一句）**：每个应用记住上次窗口的位置、尺寸、最大化
//! 状态，下次打开原样恢复——带两条保护：恢复位置若在已拔掉的显示器
//! 区域外，自动拉回主屏并提示一次；恢复尺寸不小于 F214 法定最小尺寸。
//!
//! **设计要点**：
//! - 「应用+文档」双级记忆：键 = (app, doc)，doc=0 即应用级默认，
//!   多文档应用按文档各自记忆（互不覆盖）；
//! - LRU 容量上限（双级键共享一表，淘汰最久未用；持久化面允许 alloc）；
//! - 恢复管线：读记忆 → 显示器区域校验（不与任何在位显示器正面积
//!   相交则 clamped_into 主屏拉回 + 每键提示一次）→ F214 最小尺寸
//!   钳制 → 应用。记忆本体保持用户原位置（显示器回插后仍回原位），
//!   拉回只作用于本次恢复；
//! - 「提示一次」：per-key 提示位，发过即记档，重新记忆才复位——
//!   同一窗口反复恢复不刷屏；
//! - 热拔/分辨率变更同一条判定通路（显示器表摘槽或缩小即生效）；
//! - 遗忘接口：单键遗忘与整应用遗忘（卸载场景）；
//! - 操作账本：分钟聚合 [remember/restore/pullback] 三计数器，
//!   保留 30 天（复用 sbase MinuteBook）。
//!
//! **依赖锚点**：拉回唯一落点 [`crate::h1star::h1base::Rect::clamped_into`]；
//! 账本复用 [`crate::star::sbase::MinuteBook`]；最小尺寸锚 F214（本模块
//! 自带取值，接线时以 F214 实装常量为准）；时间一律注入毫秒戳。

use crate::checks::CheckSet;
use crate::h1star::h1base::Rect;
use crate::star::sbase::MinuteBook;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// F214 法定最小宽度锚点（本模块取值；接线时以 F214 实装常量为准）。
pub const MIN_WIN_W: i32 = 160;

/// F214 法定最小高度锚点（本模块取值；接线时以 F214 实装常量为准）。
pub const MIN_WIN_H: i32 = 120;

/// 记忆表 LRU 容量——双级键共享一个表，超限淘汰最久未用。
pub const MEM_CAP: usize = 64;

/// 显示器槽容量（笔记本 + 外扩三屏，足够覆盖验收场景）。
pub const MONITOR_CAP: usize = 4;

/// 拉回主屏时的边距（贴边留 8px，避免完全顶死可再拖动）。
pub const PULLBACK_MARGIN_PX: i32 = 8;

// ---------------------------------------------------------------------------
// 数据面
// ---------------------------------------------------------------------------

/// 记忆键：应用 + 文档双级（doc=0 即应用级默认）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MemKey {
    pub app: u32,
    /// 0 = 应用级默认；>0 = 文档句柄（多文档独立记忆）。
    pub doc: u32,
}

/// 一条记忆：几何 + 最大化态 + 拉回提示位。
#[derive(Clone, Copy, Debug)]
pub struct MemEntry {
    pub key: MemKey,
    pub rect: Rect,
    pub maximized: bool,
    /// 最近使用毫秒戳（LRU 依据，调用方注入时钟）。
    pub last_used: u64,
    /// 「显示器外拉回」提示已发一次（下次 remember 才复位）。
    pull_notified: bool,
}

/// 恢复结果（行为级信息合一，判定与诊断都读它）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestoreResult {
    /// 无记忆（该键首次打开）。
    pub miss: bool,
    /// 应用几何（miss 时为最小安全窗）。
    pub rect: Rect,
    pub maximized: bool,
    /// 本次发生了显示器外拉回。
    pub pulled_back: bool,
    /// 本次发出「窗口已拉回主屏」提示（每键只一次）。
    pub notify: bool,
}

// ---------------------------------------------------------------------------
// 记忆表
// ---------------------------------------------------------------------------

/// 窗口位置与尺寸记忆：双级键 LRU 表 + 显示器表 + 恢复管线。
pub struct WinMemory {
    /// LRU 序：index 0 = 最近使用，尾 = 最久未用（淘汰位）。
    entries: Vec<MemEntry>,
    monitors: [Option<Rect>; MONITOR_CAP],
    /// 累计拉回次数（诊断面）。
    pub pull_count: u64,
    /// 累计提示次数（「提示一次」的审计计数）。
    pub notify_count: u64,
    /// 累计 miss（诊断面）。
    pub miss_count: u64,
    /// 累计最小尺寸钳制次数（诊断面）。
    pub clamp_count: u64,
}

impl WinMemory {
    pub fn new() -> WinMemory {
        WinMemory {
            entries: Vec::with_capacity(MEM_CAP),
            monitors: [const { None }; MONITOR_CAP],
            pull_count: 0,
            notify_count: 0,
            miss_count: 0,
            clamp_count: 0,
        }
    }

    // -- 显示器表 -----------------------------------------------------------

    /// 登记显示器（idx 0 = 主屏；重复登记覆盖——热插/分辨率变更同入口）。
    pub fn monitor_on(&mut self, idx: usize, rect: Rect) -> bool {
        if idx >= MONITOR_CAP || rect.w <= 0 || rect.h <= 0 {
            return false;
        }
        self.monitors[idx] = Some(rect);
        true
    }

    /// 摘除显示器（热拔：该区域即刻失效）。
    pub fn monitor_off(&mut self, idx: usize) -> bool {
        if idx >= MONITOR_CAP || self.monitors[idx].is_none() {
            return false;
        }
        self.monitors[idx] = None;
        true
    }

    /// 读显示器槽（诊断/fuzz 不变量用）。
    pub fn monitor(&self, idx: usize) -> Option<Rect> {
        if idx < MONITOR_CAP {
            self.monitors[idx]
        } else {
            None
        }
    }

    /// 主屏（idx 0 缺席时回退第一个在位显示器；全空 None）。
    fn primary(&self) -> Option<Rect> {
        if let Some(r) = self.monitors[0] {
            return Some(r);
        }
        self.monitors.iter().filter_map(|m| *m).next()
    }

    // -- 记忆表 -------------------------------------------------------------

    /// 记忆条数（诊断面）。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn find(&self, key: &MemKey) -> Option<usize> {
        self.entries.iter().position(|e| e.key == *key)
    }

    /// 记住一个窗口几何（新几何复位拉回提示位；LRU 提到队首）。
    pub fn remember(&mut self, key: MemKey, rect: Rect, maximized: bool, now: u64) {
        if let Some(idx) = self.find(&key) {
            let mut e = self.entries[idx];
            e.rect = rect;
            e.maximized = maximized;
            e.last_used = now;
            e.pull_notified = false;
            self.entries.remove(idx);
            self.entries.insert(0, e);
            return;
        }
        let entry = MemEntry { key, rect, maximized, last_used: now, pull_notified: false };
        self.entries.insert(0, entry);
        if self.entries.len() > MEM_CAP {
            self.entries.pop();
        }
    }

    /// 裸读记忆（不推进 LRU、不走恢复管线——诊断面）。
    pub fn peek(&self, key: &MemKey) -> Option<(Rect, bool)> {
        self.find(key).map(|i| (self.entries[i].rect, self.entries[i].maximized))
    }

    /// 遗忘一条记忆（用户显性「不再记住此窗」）。
    pub fn forget(&mut self, key: &MemKey) -> bool {
        match self.find(key) {
            Some(idx) => {
                self.entries.remove(idx);
                true
            }
            None => false,
        }
    }

    /// 遗忘某应用全部记忆（卸载场景）。返回清除条数。
    pub fn forget_app(&mut self, app: u32) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| e.key.app != app);
        before - self.entries.len()
    }

    /// 恢复管线：读记忆 → 显示器校验（拉回+提示一次）→ 最小尺寸钳制。
    ///
    /// 记忆本体不改动：拉回只作用于本次恢复的窗口几何，显示器回插后
    /// 下次恢复仍回用户原位置（用户意图优先）。
    pub fn restore(&mut self, key: &MemKey) -> RestoreResult {
        let idx = match self.find(key) {
            Some(i) => i,
            None => {
                self.miss_count += 1;
                return RestoreResult {
                    miss: true,
                    rect: Rect::new(0, 0, MIN_WIN_W, MIN_WIN_H),
                    maximized: false,
                    pulled_back: false,
                    notify: false,
                };
            }
        };
        // LRU 提前（先取值再动表，避免借用冲突）。
        let mut e = self.entries[idx];
        self.entries.remove(idx);
        self.entries.insert(0, e);

        let primary = self.primary();
        let mut pulled_back = false;
        let mut notify = false;
        // 显示器校验：与任一在位显示器正面积相交 → 原样恢复；
        // 全不相交（热拔/分辨率缩小场景）→ 拉回主屏 + 每键提示一次。
        let on_screen =
            self.monitors.iter().filter_map(|m| *m).any(|m| e.rect.intersect_area(&m) > 0);
        if !on_screen {
            if let Some(p) = primary {
                // 贴边留 PULLBACK_MARGIN_PX，保持窗口可再拖动。
                let work = Rect::new(
                    p.x + PULLBACK_MARGIN_PX,
                    p.y + PULLBACK_MARGIN_PX,
                    (p.w - 2 * PULLBACK_MARGIN_PX).max(1),
                    (p.h - 2 * PULLBACK_MARGIN_PX).max(1),
                );
                e.rect = e.rect.clamped_into(&work);
                self.pull_count += 1;
                pulled_back = true;
                // 提示位写回存储（re-insert 后条目在队首）。
                if !self.entries[0].pull_notified {
                    self.entries[0].pull_notified = true;
                    self.notify_count += 1;
                    notify = true;
                }
            }
        }
        // F214 最小尺寸钳制（保护二，逐次恢复都把关）。
        if e.rect.w < MIN_WIN_W || e.rect.h < MIN_WIN_H {
            let nw = e.rect.w.max(MIN_WIN_W);
            let nh = e.rect.h.max(MIN_WIN_H);
            e.rect = Rect::new(e.rect.x, e.rect.y, nw, nh);
            self.clamp_count += 1;
        }
        RestoreResult { miss: false, rect: e.rect, maximized: e.maximized, pulled_back, notify }
    }

    // -- 持久化面（alloc 允许：非热路径，重启恢复一次） ---------------------

    /// 记忆表持久化（键+几何+最大化态+戳）。
    pub fn persist(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + self.entries.len() * 33);
        out.push(self.entries.len() as u8);
        for e in &self.entries {
            out.extend_from_slice(&e.key.app.to_le_bytes());
            out.extend_from_slice(&e.key.doc.to_le_bytes());
            out.extend_from_slice(&e.rect.x.to_le_bytes());
            out.extend_from_slice(&e.rect.y.to_le_bytes());
            out.extend_from_slice(&e.rect.w.to_le_bytes());
            out.extend_from_slice(&e.rect.h.to_le_bytes());
            out.push(e.maximized as u8);
            out.extend_from_slice(&e.last_used.to_le_bytes());
        }
        out
    }

    /// 从持久化字节恢复（校验失败整体拒绝，不留半态）。
    pub fn restore_from(&mut self, buf: &[u8]) -> bool {
        if buf.is_empty() {
            return false;
        }
        let n = buf[0] as usize;
        if 1 + n * 33 != buf.len() {
            return false;
        }
        let mut entries = Vec::with_capacity(n);
        for k in 0..n {
            let base = 1 + k * 33;
            let app = u32::from_le_bytes([buf[base], buf[base + 1], buf[base + 2], buf[base + 3]]);
            let doc = u32::from_le_bytes([buf[base + 4], buf[base + 5], buf[base + 6], buf[base + 7]]);
            let x = i32::from_le_bytes([buf[base + 8], buf[base + 9], buf[base + 10], buf[base + 11]]);
            let y = i32::from_le_bytes([buf[base + 12], buf[base + 13], buf[base + 14], buf[base + 15]]);
            let w = i32::from_le_bytes([buf[base + 16], buf[base + 17], buf[base + 18], buf[base + 19]]);
            let h = i32::from_le_bytes([buf[base + 20], buf[base + 21], buf[base + 22], buf[base + 23]]);
            let maximized = buf[base + 24] != 0;
            let mut lu = [0u8; 8];
            lu.copy_from_slice(&buf[base + 25..base + 33]);
            let last_used = u64::from_le_bytes(lu);
            if w <= 0 || h <= 0 {
                return false;
            }
            entries.push(MemEntry {
                key: MemKey { app, doc },
                rect: Rect::new(x, y, w, h),
                maximized,
                last_used,
                pull_notified: false,
            });
        }
        self.entries = entries;
        true
    }
}

impl Default for WinMemory {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 操作账本（分钟聚合，30 天保留）
// ---------------------------------------------------------------------------

/// 账本计数器列：0 remember / 1 restore / 2 pullback。
pub const LEDGER_COLS: usize = 3;

/// 记忆操作账本：分钟聚合三计数器，保留 30 天（43200 分钟）。
pub struct WinMemLedger {
    book: MinuteBook,
}

impl WinMemLedger {
    pub fn new() -> WinMemLedger {
        WinMemLedger { book: MinuteBook::new(LEDGER_COLS, 30 * 1440) }
    }

    /// 记一笔 remember（几何更新）。
    pub fn record_remember(&mut self, minute: u64) {
        self.book.record_minute(minute, &[1, 0, 0]);
    }

    /// 记一笔 restore（含是否拉回）。
    pub fn record_restore(&mut self, pulled: bool, minute: u64) {
        self.book.record_minute(minute, &[0, 1, pulled as u64]);
    }

    /// 区间聚合 [remember, restore, pullback]。
    pub fn range_sum(&self, from_min: u64, to_min: u64) -> [u64; LEDGER_COLS] {
        let v = self.book.range_sum(from_min, to_min);
        [v[0], v[1], v[2]]
    }

    /// 保留窗驱逐。
    pub fn evict_by_now(&mut self, now_min: u64) -> usize {
        self.book.evict_by_now(now_min)
    }

    /// 现存分钟槽数（诊断面）。
    pub fn slot_count(&self) -> usize {
        self.book.slot_count()
    }
}

impl Default for WinMemLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// xors32 随机步进（范式照 touchpad.rs）。
fn xors32(x: &mut u32) -> u32 {
    *x ^= *x << 13;
    *x ^= *x >> 17;
    *x ^= *x << 5;
    *x
}

/// 自检场景显示器：主屏 1920×1080 + 副屏横接。
const MAIN: Rect = Rect::new(0, 0, 1920, 1080);
const SIDE: Rect = Rect::new(1920, 0, 1920, 1080);

/// F237 自检（判据：恢复精度 <1px ×10 + 热拔拉回提示一次 + 双级独立
/// + 最小尺寸钳制 + LRU + 遗忘 + 持久化 + 账本）。
pub fn run_winmem_checks() -> CheckSet {
    let mut set = CheckSet::new("F237-winmem");

    // 1. 恢复精度 <1px（10 例：整像素域恢复逐项差恒 0）。
    let mut m = WinMemory::new();
    let mut all_exact = true;
    for i in 0..10u32 {
        let key = MemKey { app: 10 + i, doc: 0 };
        let r = Rect::new((i * 37) as i32, (i * 53) as i32, 400 + i as i32 * 10, 300 + i as i32 * 10);
        m.remember(key, r, i % 2 == 0, i as u64);
        let g = m.restore(&key);
        if g.miss
            || g.rect != r
            || g.maximized != (i % 2 == 0)
            || (g.rect.x - r.x).abs()
                + (g.rect.y - r.y).abs()
                + (g.rect.w - r.w).abs()
                + (g.rect.h - r.h).abs()
                >= 1
        {
            all_exact = false;
        }
    }
    set.add("restore precision <1px x10", all_exact, "");

    // 2. 热拔模拟：记忆位置在拔掉的副屏区域 → 拉回主屏（贴边留边距）。
    let mut m2 = WinMemory::new();
    assert!(m2.monitor_on(0, MAIN));
    assert!(m2.monitor_on(1, SIDE));
    let key2 = MemKey { app: 1, doc: 0 };
    m2.remember(key2, Rect::new(2200, 100, 800, 600), false, 10);
    assert!(m2.monitor_off(1));
    let g2 = m2.restore(&key2);
    let within_main = g2.rect.x >= MAIN.x + PULLBACK_MARGIN_PX
        && g2.rect.y >= MAIN.y
        && g2.rect.right() <= MAIN.right()
        && g2.rect.bottom() <= MAIN.bottom();
    set.add("hot-unplug pulls back to primary", g2.pulled_back && within_main, "");

    // 3. 「提示一次」全链：首拉提示；同键复拉不重复；重新记忆后复位。
    let g3 = m2.restore(&key2);
    let once = g3.pulled_back && !g3.notify && m2.notify_count == 1;
    assert!(m2.monitor_on(1, SIDE));
    m2.remember(key2, Rect::new(2500, 200, 400, 300), false, 20);
    assert!(m2.monitor_off(1));
    let g4 = m2.restore(&key2);
    set.add("notify once, re-arms after remember", once && g4.notify && m2.notify_count == 2, "");

    // 4. 记忆本体不动：拉回后原记忆仍指向副屏位置（屏回插即回原位）。
    assert!(m2.monitor_on(1, SIDE));
    set.add(
        "memory keeps user position",
        m2.peek(&key2).map(|(r, _)| r) == Some(Rect::new(2500, 200, 400, 300)),
        "",
    );

    // 5. 多文档独立记忆 + 应用级默认并存。
    let mut m3 = WinMemory::new();
    let d1 = MemKey { app: 7, doc: 1 };
    let d2 = MemKey { app: 7, doc: 2 };
    let app_key = MemKey { app: 7, doc: 0 };
    let r1 = Rect::new(10, 10, 500, 400);
    let r2 = Rect::new(900, 500, 640, 480);
    let r0 = Rect::new(50, 50, 700, 500);
    m3.remember(d1, r1, false, 1);
    m3.remember(d2, r2, true, 2);
    m3.remember(app_key, r0, false, 3);
    let g1 = m3.restore(&d1);
    let g2b = m3.restore(&d2);
    let g0 = m3.restore(&app_key);
    set.add(
        "dual-level memory independent",
        g1.rect == r1 && !g1.maximized && g2b.rect == r2 && g2b.maximized && g0.rect == r0,
        "",
    );

    // 6. 最小尺寸钳制：记忆 100×80 → 恢复 ≥ F214 最小（160×120）。
    let mut m4 = WinMemory::new();
    let key4 = MemKey { app: 3, doc: 0 };
    m4.remember(key4, Rect::new(0, 0, 100, 80), false, 5);
    let g5 = m4.restore(&key4);
    set.add(
        "min size clamp on restore",
        g5.rect.w >= MIN_WIN_W && g5.rect.h >= MIN_WIN_H && m4.clamp_count == 1,
        "",
    );

    // 7. LRU 容量：塞 70 条 → 最旧 6 条淘汰、总数 ≤ 64；摸活条目幸存。
    let mut m5 = WinMemory::new();
    for i in 0..70u32 {
        m5.remember(MemKey { app: i, doc: 0 }, Rect::new(0, 0, 300, 200), false, i as u64);
    }
    let evicted_ok = m5.peek(&MemKey { app: 0, doc: 0 }).is_none()
        && m5.peek(&MemKey { app: 5, doc: 0 }).is_none()
        && m5.peek(&MemKey { app: 69, doc: 0 }).is_some()
        && m5.len() <= MEM_CAP;
    let _ = m5.restore(&MemKey { app: 62, doc: 0 }); // 摸活
    m5.remember(MemKey { app: 70, doc: 0 }, Rect::new(0, 0, 300, 200), false, 999);
    let touch_ok = m5.peek(&MemKey { app: 62, doc: 0 }).is_some()
        && m5.peek(&MemKey { app: 6, doc: 0 }).is_none();
    set.add("LRU cap 64 & touch keeps alive", evicted_ok && touch_ok, "");

    // 8. 遗忘接口：单键遗忘 + 整应用遗忘（卸载场景）。
    let mut m6 = WinMemory::new();
    m6.remember(MemKey { app: 1, doc: 1 }, Rect::new(0, 0, 300, 200), false, 1);
    m6.remember(MemKey { app: 1, doc: 2 }, Rect::new(5, 5, 300, 200), false, 2);
    m6.remember(MemKey { app: 2, doc: 0 }, Rect::new(9, 9, 300, 200), false, 3);
    let f1 = m6.forget(&MemKey { app: 1, doc: 1 });
    let n_app1 = m6.forget_app(1);
    let f_again = m6.forget(&MemKey { app: 1, doc: 1 });
    set.add(
        "forget key & forget app",
        f1 && n_app1 == 1 && !f_again && m6.peek(&MemKey { app: 2, doc: 0 }).is_some(),
        "",
    );

    // 9. 持久化 round-trip（重启后记忆保持）+ 破损拒绝。
    let snap = m3.persist();
    let mut m7 = WinMemory::new();
    let rt = m7.restore_from(&snap)
        && m7.restore(&d1).rect == r1
        && m7.restore(&d2).rect == r2
        && m7.restore(&app_key).rect == r0;
    let mut bad = snap.clone();
    bad[0] = 9; // 条数失配。
    let mut bad2 = snap.clone();
    bad2[1 + 16 + 3] = 0x80; // 首条宽度最高位置负 → 拒绝。
    set.add(
        "persist round-trip & rejects corruption",
        rt && !m7.restore_from(&bad) && !m7.restore_from(&bad2) && !m7.restore_from(&[]),
        "",
    );

    // 10. 双显示器在位时副屏位置原样恢复（不误拉回）。
    let mut m8 = WinMemory::new();
    assert!(m8.monitor_on(0, MAIN) && m8.monitor_on(1, SIDE));
    let key8 = MemKey { app: 5, doc: 0 };
    let side_r = Rect::new(2100, 80, 600, 400);
    m8.remember(key8, side_r, false, 1);
    let g8 = m8.restore(&key8);
    set.add("side-monitor position restored as-is", !g8.pulled_back && g8.rect == side_r, "");

    // 11. 分辨率变更（显示器缩小）同通路拉回：副屏 1920 宽改 1000 宽，
    //     新键记忆位置 3000..3400 落在缩小后的屏外 → 拉回。
    //     [缺陷账本] 现象：本检查项红。根因：末位断言
    //     `intersect_area(&MAIN) == 0` 与主册判据原文矛盾——主册 F237
    //     「恢复位置若在已拔掉的显示器区域外，自动拉回主屏并提示一次」
    //     且设计要点明确「热拔/分辨率变更同一条判定通路」，拉回落点
    //     即主屏（与检查 2 的 within_main 既定语义一致）；拉回后窗口
    //     在主屏内，与 MAIN 相交面积必 > 0，属检查项断言写反。修法：
    //     改检查项——按主册「拉回主屏」断言几何在主屏内（贴边留 8px）。
    let key11 = MemKey { app: 6, doc: 0 };
    m8.remember(key11, Rect::new(3000, 200, 400, 300), false, 2);
    assert!(m8.monitor_on(1, Rect::new(1920, 0, 1000, 1080)));
    let g9 = m8.restore(&key11);
    set.add(
        "resolution change pulls back too",
        g9.pulled_back
            && g9.rect.x >= MAIN.x + PULLBACK_MARGIN_PX
            && g9.rect.right() <= MAIN.right()
            && g9.rect.bottom() <= MAIN.bottom()
            && g9.rect.intersect_area(&MAIN) > 0,
        "",
    );

    // 12. 账本：分钟聚合 + 区间求和 + 保留窗驱逐。
    let mut led = WinMemLedger::new();
    led.record_remember(5);
    led.record_remember(5);
    led.record_restore(false, 6);
    led.record_restore(true, 7);
    let s = led.range_sum(5, 7);
    set.add(
        "ledger aggregates remember/restore/pullback",
        s[0] == 2 && s[1] == 2 && s[2] == 1 && led.slot_count() == 3,
        "",
    );
    // [缺陷账本] 现象：ledger evicts by now 红。根因：检查项在分钟
    // 5..7 上调 evict_by_now(7)，而 MinuteBook 契约是「now 之前
    // cap_minutes 外的槽全丢」（保留窗 30 天）——7 分钟处无槽越出
    // 保留窗，属检查项误读驱逐语义。修法：改检查项，now 取保留窗
    // 界外（43200+7），断言 slot_count == 1 不变（恰留 minute 7）。
    set.add("ledger evicts by now", { led.evict_by_now(30 * 1440 + 7); led.slot_count() == 1 }, "");

    // 13. xors32 fuzz：随机记忆/显示器增删/恢复 1000 轮，不变量=
    //     有在位显示器时恢复结果必与其一相交、宽高 ≥ 最小尺寸、不 panic。
    let mut m9 = WinMemory::new();
    let mut x: u32 = 0x2545_F491;
    let mut ok = true;
    for i in 0..1000u32 {
        match xors32(&mut x) % 5 {
            0 => {
                let a = xors32(&mut x);
                let b = xors32(&mut x);
                let r = Rect::new(
                    (a % 3800) as i32 - 100,
                    (b % 1000) as i32 - 50,
                    200 + ((a >> 8) % 900) as i32,
                    150 + ((b >> 8) % 600) as i32,
                );
                m9.remember(MemKey { app: a % 8, doc: b % 3 }, r, false, i as u64);
            }
            1 => {
                let idx = (xors32(&mut x) as usize) % MONITOR_CAP;
                if xors32(&mut x) % 2 == 0 {
                    let r = if idx == 0 { MAIN } else { SIDE };
                    m9.monitor_on(idx, r);
                } else {
                    m9.monitor_off(idx);
                }
            }
            2 => {
                let a = xors32(&mut x);
                m9.forget(&MemKey { app: a % 8, doc: a % 3 });
            }
            _ => {
                let a = xors32(&mut x);
                let b = xors32(&mut x);
                let g = m9.restore(&MemKey { app: a % 8, doc: b % 3 });
                if !g.miss {
                    let any_mon = (0..MONITOR_CAP).any(|i| m9.monitor(i).is_some());
                    let touching =
                        (0..MONITOR_CAP).any(|i| m9.monitor(i).map_or(false, |mo| g.rect.intersect_area(&mo) > 0));
                    if (any_mon && !touching) || g.rect.w < MIN_WIN_W || g.rect.h < MIN_WIN_H {
                        ok = false;
                    }
                }
            }
        }
    }
    set.add("xors32 fuzz 1000 rounds invariants", ok, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_precision_ten_cases() {
        let mut m = WinMemory::new();
        for i in 0..10u32 {
            let key = MemKey { app: i, doc: i + 1 };
            let r =
                Rect::new(-50 + i as i32 * 7, 100 + i as i32 * 11, 320 + i as i32, 240 + i as i32 * 2);
            m.remember(key, r, false, i as u64);
            let g = m.restore(&key);
            assert!(!g.miss);
            assert_eq!(g.rect, r, "第 {i} 例恢复必须逐像素一致");
        }
    }

    #[test]
    fn miss_returns_minimal_safe_rect() {
        let mut m = WinMemory::new();
        let g = m.restore(&MemKey { app: 99, doc: 0 });
        assert!(g.miss);
        assert_eq!(g.rect.w, MIN_WIN_W);
        assert_eq!(g.rect.h, MIN_WIN_H);
        assert!(!g.pulled_back && !g.notify);
        assert_eq!(m.miss_count, 1);
    }

    #[test]
    fn hot_unplug_full_flow() {
        let mut m = WinMemory::new();
        assert!(m.monitor_on(0, MAIN));
        assert!(m.monitor_on(1, SIDE));
        let key = MemKey { app: 2, doc: 0 };
        m.remember(key, Rect::new(2400, 300, 700, 500), true, 1);
        assert!(m.monitor_off(1), "热拔副屏");
        let g = m.restore(&key);
        assert!(g.pulled_back && g.notify);
        assert!(g.maximized, "最大化态独立记忆");
        assert!(g.rect.right() <= MAIN.right(), "拉回后不越主屏右缘");
        assert_eq!(g.rect.w, 700, "宽度合法时不改变尺寸");
        // 屏回插后恢复：回用户原位置（记忆本体不动——拉回只作用当次
        // 恢复，与设计要点「显示器回插后仍回原位」一致）。
        // [缺陷账本] 旧断言要求回插后几何仍与主屏相交——与「记忆本体
        // 不动」自相矛盾（原位置在副屏上）。修法：断言回用户原位置。
        assert!(m.monitor_on(1, SIDE));
        let g2 = m.restore(&key);
        assert!(!g2.pulled_back);
        assert_eq!(g2.rect, Rect::new(2400, 300, 700, 500), "回插后回用户原位置");
        assert!(g2.rect.intersect_area(&SIDE) > 0);
    }

    #[test]
    fn resolution_shrink_then_grow() {
        let mut m = WinMemory::new();
        assert!(m.monitor_on(0, MAIN));
        assert!(m.monitor_on(1, SIDE));
        let key = MemKey { app: 4, doc: 0 };
        let far = Rect::new(3000, 200, 500, 400); // 副屏最右端
        m.remember(key, far, false, 1);
        // 副屏缩到 1000 宽（3000 > 1920+1000）→ 拉回。
        assert!(m.monitor_on(1, Rect::new(1920, 0, 1000, 1080)));
        let g = m.restore(&key);
        assert!(g.pulled_back);
        assert!(g.rect.intersect_area(&MAIN) > 0 || g.rect.intersect_area(&Rect::new(1920, 0, 1000, 1080)) > 0);
        // 副屏恢复原宽 → 回用户原位置。
        assert!(m.monitor_on(1, SIDE));
        let g2 = m.restore(&key);
        assert_eq!(g2.rect, far, "用户原位置在记忆中保持");
    }

    #[test]
    fn lru_order_is_exact() {
        let mut m = WinMemory::new();
        for i in 0..MEM_CAP as u32 {
            m.remember(MemKey { app: i, doc: 0 }, Rect::new(0, 0, 300, 200), false, i as u64);
        }
        assert_eq!(m.len(), MEM_CAP);
        // 摸活 0、3、62 → 队首变 [62,3,0,63,...]。
        for a in [0u32, 3, 62] {
            let _ = m.restore(&MemKey { app: a, doc: 0 });
        }
        m.remember(MemKey { app: MEM_CAP as u32, doc: 0 }, Rect::new(0, 0, 300, 200), false, 100);
        // 只淘汰队尾一个（最久未用 = app 1）。
        assert!(m.peek(&MemKey { app: 1, doc: 0 }).is_none(), "队尾 app1 被挤掉");
        assert!(m.peek(&MemKey { app: 2, doc: 0 }).is_some());
        assert!(m.peek(&MemKey { app: 0, doc: 0 }).is_some());
        assert!(m.peek(&MemKey { app: 62, doc: 0 }).is_some());
        assert!(m.peek(&MemKey { app: MEM_CAP as u32, doc: 0 }).is_some());
        assert_eq!(m.len(), MEM_CAP, "容量守恒");
    }

    #[test]
    fn forget_app_leaves_others() {
        let mut m = WinMemory::new();
        for app in 1..=5u32 {
            m.remember(MemKey { app, doc: 1 }, Rect::new(0, 0, 300, 200), false, app as u64);
            m.remember(MemKey { app, doc: 2 }, Rect::new(1, 1, 300, 200), false, app as u64);
        }
        assert_eq!(m.len(), 10);
        assert_eq!(m.forget_app(3), 2, "应用 3 的两条都被清除");
        assert_eq!(m.len(), 8);
        assert!(m.peek(&MemKey { app: 3, doc: 1 }).is_none());
        assert!(m.peek(&MemKey { app: 4, doc: 2 }).is_some());
        assert!(!m.forget(&MemKey { app: 3, doc: 1 }), "重复遗忘显性失败");
    }

    #[test]
    fn winmem_selfcheck_all_green() {
        let set = run_winmem_checks();
        assert!(set.all_passed(), "F237 自检存在红项");
        assert!(!set.truncated());
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
//
// 判据锚 F237。持久化面 = 记忆表 framed 记录（键=窗口身份哈希+显示器
// 指纹）；壳接线面 = 显示器变更拉回几何（钳入新屏纯函数）；判定面 =
// run_winmem_v2_checks（首条持久化 round-trip）。

// -- 持久化 I/O 面 ---------------------------------------------------------

/// v2 记录头 magic「VXH1」+ 版本（全域 v2 段统一）。
pub const V2_MAGIC: [u8; 4] = *b"VXH1";
pub const V2_VERSION: u8 = 1;

/// 四类损坏显性拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2SaveErr {
    BadMagic,
    BadVersion,
    BadLen,
    BadChecksum,
}

/// FNV-1a 64 位取低 32 位（常数与 vdesk 音频指纹同族）。
fn v2_fnv1a32(data: &[u8]) -> u32 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h as u32
}

/// 单记录条目上限（容量上限在册：每次落盘至多 16 键的点名快照，
/// 全表 64 键由调用方分两批落盘——不静默截断）。
pub const V2_SNAP_CAP: usize = 16;

/// 记录容量上限在册：count u8 + 16 条 × (hash u32 + app u32 + doc u32
/// + xywh i32 ×4 + max u8) = 465 字节 payload（每条 29 字节）。
pub const V2_PAYLOAD_MAX: usize = 1 + V2_SNAP_CAP * 29;
pub const V2_REC_MAX: usize = 5 + V2_PAYLOAD_MAX + 4;

/// 窗口身份哈希（主册 F237 v2 锚：键 = 窗口身份哈希——FNV-1a 逐字节
/// 覆盖 app/doc 两级键，跨重启稳定的身份指纹）。
pub fn v2_key_hash(key: &MemKey) -> u32 {
    let mut b = [0u8; 8];
    b[..4].copy_from_slice(&key.app.to_le_bytes());
    b[4..].copy_from_slice(&key.doc.to_le_bytes());
    v2_fnv1a32(&b)
}

/// 显示器指纹（主册 F237 v2 锚：显示器指纹——在位显示器几何逐槽
/// 灌入 FNV，布局一变指纹即变，落盘记录自带环境烙印）。
pub fn v2_monitor_fp(m: &WinMemory) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for i in 0..MONITOR_CAP {
        let v = m.monitor(i).unwrap_or(Rect::new(0, 0, 0, 0));
        for word in [v.x, v.y, v.w, v.h] {
            for b in word.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
    }
    h
}

/// 记忆表快照记录（主册 F237 v2：记忆表持久化——每条带窗口身份
/// 哈希，恢复面可按键哈希快速对账）。
#[derive(Clone, Copy, Debug)]
pub struct V2MemRec {
    pub hash: u32,
    pub key: MemKey,
    pub rect: Rect,
    pub maximized: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct V2MemSnap {
    pub entries: [Option<V2MemRec>; V2_SNAP_CAP],
    pub count: usize,
}

impl V2MemSnap {
    /// 从记忆表点名导出（热路径表不暴露迭代器——调用方持键集按名取）。
    pub fn capture(m: &WinMemory, keys: &[MemKey]) -> V2MemSnap {
        let mut snap = V2MemSnap { entries: [const { None }; V2_SNAP_CAP], count: 0 };
        for key in keys.iter().take(V2_SNAP_CAP) {
            if let Some((rect, maximized)) = m.peek(key) {
                snap.entries[snap.count] = Some(V2MemRec {
                    hash: v2_key_hash(key),
                    key: *key,
                    rect,
                    maximized,
                });
                snap.count += 1;
            }
        }
        snap
    }

    pub fn to_bytes(&self, out: &mut [u8]) -> Option<usize> {
        if out.len() < 6 {
            return None;
        }
        out[..4].copy_from_slice(&V2_MAGIC);
        out[4] = V2_VERSION;
        out[5] = self.count as u8;
        let mut n = 6usize;
        for e in self.entries[..self.count].iter().flatten() {
            if out.len() < n + 29 {
                return None;
            }
            out[n..n + 4].copy_from_slice(&e.hash.to_le_bytes());
            out[n + 4..n + 8].copy_from_slice(&e.key.app.to_le_bytes());
            out[n + 8..n + 12].copy_from_slice(&e.key.doc.to_le_bytes());
            out[n + 12..n + 16].copy_from_slice(&e.rect.x.to_le_bytes());
            out[n + 16..n + 20].copy_from_slice(&e.rect.y.to_le_bytes());
            out[n + 20..n + 24].copy_from_slice(&e.rect.w.to_le_bytes());
            out[n + 24..n + 28].copy_from_slice(&e.rect.h.to_le_bytes());
            out[n + 28] = e.maximized as u8;
            n += 29;
        }
        if out.len() < n + 4 {
            return None;
        }
        let sum = v2_fnv1a32(&out[..n]);
        out[n..n + 4].copy_from_slice(&sum.to_le_bytes());
        Some(n + 4)
    }

    /// 解码：四类损坏 + 身份哈希与键不符（记录被改键）一律拒绝。
    pub fn from_bytes(buf: &[u8]) -> Result<V2MemSnap, V2SaveErr> {
        if buf.len() < 10 {
            return Err(V2SaveErr::BadLen);
        }
        if buf[..4] != V2_MAGIC {
            return Err(V2SaveErr::BadMagic);
        }
        if buf[4] != V2_VERSION {
            return Err(V2SaveErr::BadVersion);
        }
        let body = buf.len() - 4;
        let sum = u32::from_le_bytes([buf[body], buf[body + 1], buf[body + 2], buf[body + 3]]);
        if v2_fnv1a32(&buf[..body]) != sum {
            return Err(V2SaveErr::BadChecksum);
        }
        let count = buf[5] as usize;
        if count > V2_SNAP_CAP || body != 6 + count * 29 {
            return Err(V2SaveErr::BadLen);
        }
        let mut snap = V2MemSnap { entries: [const { None }; V2_SNAP_CAP], count: 0 };
        for k in 0..count {
            let n = 6 + k * 29;
            if n + 29 > body {
                return Err(V2SaveErr::BadLen);
            }
            let key = MemKey {
                app: u32::from_le_bytes([buf[n + 4], buf[n + 5], buf[n + 6], buf[n + 7]]),
                doc: u32::from_le_bytes([buf[n + 8], buf[n + 9], buf[n + 10], buf[n + 11]]),
            };
            let hash = u32::from_le_bytes([buf[n], buf[n + 1], buf[n + 2], buf[n + 3]]);
            if hash != v2_key_hash(&key) {
                return Err(V2SaveErr::BadChecksum);
            }
            let w = i32::from_le_bytes([buf[n + 20], buf[n + 21], buf[n + 22], buf[n + 23]]);
            let h = i32::from_le_bytes([buf[n + 24], buf[n + 25], buf[n + 26], buf[n + 27]]);
            if w <= 0 || h <= 0 {
                return Err(V2SaveErr::BadLen);
            }
            snap.entries[snap.count] = Some(V2MemRec {
                hash,
                key,
                rect: Rect::new(
                    i32::from_le_bytes([buf[n + 12], buf[n + 13], buf[n + 14], buf[n + 15]]),
                    i32::from_le_bytes([buf[n + 16], buf[n + 17], buf[n + 18], buf[n + 19]]),
                    w,
                    h,
                ),
                maximized: buf[n + 28] != 0,
            });
            snap.count += 1;
        }
        Ok(snap)
    }

    /// 按窗口身份哈希查条目。
    pub fn find_by_hash(&self, hash: u32) -> Option<&V2MemRec> {
        self.entries[..self.count].iter().flatten().find(|e| e.hash == hash)
    }
}

// -- UI 壳接线面 -----------------------------------------------------------

/// 显示器变更拉回几何（纯函数）：钳入新屏工作区（贴边留
/// PULLBACK_MARGIN_PX——与 restore 管线同一margin常量与同一
/// clamped_into 落点，一处一事实）。
pub fn v2_pullback_geometry(rect: Rect, new_screen: &Rect) -> Rect {
    let work = Rect::new(
        new_screen.x + PULLBACK_MARGIN_PX,
        new_screen.y + PULLBACK_MARGIN_PX,
        (new_screen.w - 2 * PULLBACK_MARGIN_PX).max(1),
        (new_screen.h - 2 * PULLBACK_MARGIN_PX).max(1),
    );
    rect.clamped_into(&work)
}

// -- 判定面扩展 ------------------------------------------------------------

/// F237 v2 自检（首条必为持久化 round-trip）。
pub fn run_winmem_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F237-winmem-v2");

    // 1. 持久化 round-trip（验主册 v2 锚「记忆表持久化」：几何/最大化态
    //    /身份哈希逐项还原）。
    let mut m = WinMemory::new();
    let k1 = MemKey { app: 42, doc: 0 };
    let k2 = MemKey { app: 42, doc: 7 };
    let r1 = Rect::new(120, 80, 800, 600);
    m.remember(k1, r1, true, 1);
    m.remember(k2, Rect::new(0, 0, 300, 200), false, 2);
    let snap = V2MemSnap::capture(&m, &[k1, k2]);
    let mut buf = [0u8; V2_REC_MAX];
    let wrote = snap.to_bytes(&mut buf).unwrap_or(0);
    let back = V2MemSnap::from_bytes(&buf[..wrote]);
    set.add(
        "v2 persist round-trip: memory snapshot",
        wrote > 0
            && back.as_ref().map(|s| s.count).unwrap_or(0) == 2
            && back.as_ref().ok().and_then(|s| s.find_by_hash(v2_key_hash(&k1))).map(|e| e.rect) == Some(r1)
            && back.as_ref().ok().and_then(|s| s.find_by_hash(v2_key_hash(&k1))).map(|e| e.maximized) == Some(true),
        "",
    );

    // 2. 四类损坏全拒绝 + 改键篡改（哈希与键不符）拒绝。
    //    [缺陷账本] 现象：BadLen 分支红。根因：检查项用「截短 1 字节」
    //    构造长度损坏，但实现先验校验和后查 count 域，截短必先撞
    //    BadChecksum——BadLen 分支未被真正测到，属检查项构造缺陷。
    //    修法：改检查项——count 翻到容量外并重算校验和（真测 BadLen）；
    //    改键篡改同理重算校验和，落到「哈希与键不符」分支（否则只是
    //    撞校验和分支，key-tamper 名不副实）。
    let mut b1 = buf;
    b1[0] = b'X';
    let mut b2 = buf;
    b2[4] = 8;
    let mut b3 = [0u8; V2_REC_MAX];
    b3[..wrote].copy_from_slice(&buf[..wrote]);
    b3[5] = (V2_SNAP_CAP + 1) as u8; // count 越界（> V2_SNAP_CAP）
    let body3 = wrote - 4;
    let sum3 = v2_fnv1a32(&b3[..body3]);
    b3[body3..body3 + 4].copy_from_slice(&sum3.to_le_bytes());
    let mut b4 = buf;
    b4[8] ^= 0xFF; // 翻载荷字节（校验和覆盖域内）→ BadChecksum
    let mut bk = buf;
    bk[10] ^= 0x01; // 改 doc 字节 → 哈希对不上。
    let bodyk = wrote - 4;
    let sumk = v2_fnv1a32(&bk[..bodyk]);
    bk[bodyk..bodyk + 4].copy_from_slice(&sumk.to_le_bytes());
    set.add(
        "v2 persist rejects magic/version/len/checksum/key-tamper",
        wrote > 0
            && matches!(V2MemSnap::from_bytes(&b1), Err(V2SaveErr::BadMagic))
            && matches!(V2MemSnap::from_bytes(&b2), Err(V2SaveErr::BadVersion))
            && matches!(V2MemSnap::from_bytes(&b3[..wrote]), Err(V2SaveErr::BadLen))
            && matches!(V2MemSnap::from_bytes(&b4), Err(V2SaveErr::BadChecksum))
            && matches!(V2MemSnap::from_bytes(&bk), Err(V2SaveErr::BadChecksum)),
        "",
    );

    // 3. 拉回几何（验主册「显示器变更拉回」：副屏坐标钳入新屏、贴边
    //    留 8px；屏内几何原样不动）。
    let new_main = Rect::new(0, 0, 1366, 768);
    let far = Rect::new(3000, 500, 600, 400);
    let pulled = v2_pullback_geometry(far, &new_main);
    let keep = v2_pullback_geometry(Rect::new(100, 100, 400, 300), &new_main);
    set.add(
        "v2 pullback geometry clamps into new screen",
        pulled.right() <= new_main.right()
            && pulled.bottom() <= new_main.bottom()
            && pulled.x >= PULLBACK_MARGIN_PX
            && keep == Rect::new(100, 100, 400, 300),
        "",
    );

    // 4. 显示器指纹（验主册 v2 锚「显示器指纹」：热拔/换分辨率指纹必变，
    //    无显示器变化指纹稳定）。
    let mut m2 = WinMemory::new();
    let _ = m2.monitor_on(0, Rect::new(0, 0, 1920, 1080));
    let fp0 = v2_monitor_fp(&m2);
    let _ = m2.monitor_on(1, Rect::new(1920, 0, 1920, 1080));
    let fp1 = v2_monitor_fp(&m2);
    let _ = m2.monitor_on(1, Rect::new(1920, 0, 1000, 1080));
    let fp2 = v2_monitor_fp(&m2);
    let _ = m2.monitor_on(1, Rect::new(1920, 0, 1920, 1080));
    set.add(
        "v2 monitor fingerprint reacts to layout change",
        fp0 != fp1 && fp1 != fp2 && v2_monitor_fp(&m2) == fp1,
        "",
    );

    // 5. 身份哈希稳定性（同键恒同哈希、不同键不同哈希——样本抽验）。
    set.add(
        "v2 key hash stable & discriminating",
        v2_key_hash(&k1) == v2_key_hash(&MemKey { app: 42, doc: 0 })
            && v2_key_hash(&k1) != v2_key_hash(&k2)
            && v2_key_hash(&MemKey { app: 0, doc: 0 }) != v2_key_hash(&MemKey { app: 1, doc: 0 }),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_mem_snap_roundtrip_exact() {
        let mut m = WinMemory::new();
        let k = MemKey { app: 9, doc: 3 };
        m.remember(k, Rect::new(-10, -5, 640, 480), false, 5);
        let snap = V2MemSnap::capture(&m, &[k]);
        let mut buf = [0u8; V2_REC_MAX];
        let n = snap.to_bytes(&mut buf).unwrap();
        let back = V2MemSnap::from_bytes(&buf[..n]).unwrap();
        assert_eq!(back.count, 1);
        assert_eq!(back.entries[0].unwrap().rect, Rect::new(-10, -5, 640, 480));
    }

    #[test]
    fn v2_pullback_margin_visible() {
        // 恰好贴死右缘的窗拉回后右侧留出 8px（可再拖动）。
        let s = Rect::new(0, 0, 1000, 800);
        let p = v2_pullback_geometry(Rect::new(0, 0, 992, 300), &s);
        assert_eq!(p.right(), 1000 - PULLBACK_MARGIN_PX);
    }

    #[test]
    fn winmem_v2_selfcheck_all_green() {
        let s = run_winmem_v2_checks();
        assert!(s.all_passed(), "F237 v2 自检存在红项");
        assert!(!s.truncated());
    }
}
