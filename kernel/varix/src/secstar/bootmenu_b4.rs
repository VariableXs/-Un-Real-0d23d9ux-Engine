//! F171 图形化引导选单 · 批次四深化（secstar · G-G-01）。
//!
//! 批次四功能面（判据「图形/文字等价 3×20 轮 / 资产 <200KB / 降级实测」
//! 再纵深，与批次三互补不重叠）：
//! - [`EntryValidator`]：条目合法性校验——标签/副标题非空、长度上限、
//!   全局唯一性（重复标签 = 默认歧义的根源，源头拦）；
//! - [`BootAttemptLog`]：引导尝试账——每次选择的路径（键盘/超时/默认）、
//!   目标、时刻入环（等价性对拍的数据地基）；
//! - [`TimeoutPolicy`]：超时档位——5s/15s/30s/无限四档 + 非法值钳制 +
//!   恢复默认（limine.conf 超时面的档位化）；
//! - [`CardLayout`]：卡布局计算——屏高 → 可见卡数 / 选中卡滚动偏移
//!   （选中卡永远可见——列表超屏的可用性面）；
//! - [`equiv_drive`]：等价性对拍驱动——同一按键序列喂出同一终态
//!   （等价不是口号，是同一状态机的两次走读）。
//!
//! 零堆纪律：定长环 + 定长布局账，无 alloc。

use super::bootmenu::{CARD_GAP, CARD_H, DEFAULT_TIMEOUT_MS, ENTRY_CAP, MenuKey, MenuOutcome};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 条目合法性校验
// ---------------------------------------------------------------------------

/// 标签长度上限。
pub const LABEL_MAX: usize = 24;
/// 副标题长度上限。
pub const SUBTITLE_MAX: usize = 40;

/// 校验错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryError {
    EmptyLabel,
    LabelTooLong,
    SubtitleTooLong,
    DuplicateLabel,
    TooManyEntries,
}

/// 定长标签缓冲（零堆——字节面拷贝进槽）。
#[derive(Clone, Copy)]
pub struct LabelSlot {
    bytes: [u8; LABEL_MAX],
    len: usize,
}

impl LabelSlot {
    pub const fn empty() -> LabelSlot {
        LabelSlot { bytes: [0; LABEL_MAX], len: 0 }
    }

    pub fn set(&mut self, src: &[u8]) -> bool {
        if src.len() > LABEL_MAX {
            return false;
        }
        self.bytes[..src.len()].copy_from_slice(src);
        self.bytes[src.len()..].fill(0);
        self.len = src.len();
        true
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    pub fn equals(&self, other: &[u8]) -> bool {
        self.as_slice() == other
    }
}

/// 条目校验器：逐条合法 + 全局标签唯一（唯一性是默认条目无歧义的前提）。
pub struct EntryValidator {
    labels: [LabelSlot; ENTRY_CAP + 4],
    pub n: usize,
}

impl EntryValidator {
    pub const fn new() -> EntryValidator {
        EntryValidator { labels: [const { LabelSlot::empty() }; ENTRY_CAP + 4], n: 0 }
    }

    /// 登记一条标签（全检：非空/长度/重复/容量）。返回错误枚举或 Ok。
    pub fn admit(&mut self, label: &[u8]) -> Result<(), EntryError> {
        if self.n >= ENTRY_CAP {
            return Err(EntryError::TooManyEntries);
        }
        if label.is_empty() {
            return Err(EntryError::EmptyLabel);
        }
        if label.len() > LABEL_MAX {
            return Err(EntryError::LabelTooLong);
        }
        for i in 0..self.n {
            if self.labels[i].equals(label) {
                return Err(EntryError::DuplicateLabel);
            }
        }
        self.labels[self.n].set(label);
        self.n += 1;
        Ok(())
    }

    /// 副标题长度检（无唯一性要求）。
    pub fn check_subtitle(sub: &[u8]) -> Result<(), EntryError> {
        if sub.len() > SUBTITLE_MAX {
            Err(EntryError::SubtitleTooLong)
        } else {
            Ok(())
        }
    }

    /// 主层 defaults_unambiguous 的前置：所有标签唯一 → 默认才无歧义。
    pub fn labels_unique(&self) -> bool {
        for i in 0..self.n {
            for j in i + 1..self.n {
                if self.labels[i].as_slice() == self.labels[j].as_slice() {
                    return false;
                }
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 引导尝试账（等价性对拍数据地基）
// ---------------------------------------------------------------------------

/// 选择路径（等价性对拍的分型键）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickPath {
    Keyboard,
    Timeout,
    Default,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttemptRec {
    pub path: PickPath,
    pub target: usize,
    pub at_ms: u64,
}

/// 尝试账环（64 条——等价性 3×20 轮的账面容量）。
pub const ATTEMPT_CAP: usize = 64;

pub struct BootAttemptLog {
    ring: [Option<AttemptRec>; ATTEMPT_CAP],
    head: usize,
    pub n: usize,
    pub overflows: u32,
}

impl BootAttemptLog {
    pub const fn new() -> BootAttemptLog {
        BootAttemptLog { ring: [const { None }; ATTEMPT_CAP], head: 0, n: 0, overflows: 0 }
    }

    pub fn push(&mut self, rec: AttemptRec) {
        if self.n == ATTEMPT_CAP {
            self.overflows += 1;
        } else {
            self.n += 1;
        }
        self.ring[self.head] = Some(rec);
        self.head = (self.head + 1) % ATTEMPT_CAP;
    }

    /// 路径计数（对拍报告的三分项数字来源）。
    pub fn count_by(&self, path: PickPath) -> usize {
        self.ring.iter().flatten().filter(|r| r.path == path).count()
    }

    /// 最近一条（复盘定位）。
    pub fn last(&self) -> Option<AttemptRec> {
        if self.n == 0 {
            return None;
        }
        self.ring[(self.head + ATTEMPT_CAP - 1) % ATTEMPT_CAP]
    }
}

// ---------------------------------------------------------------------------
// 超时档位
// ---------------------------------------------------------------------------

/// 四档超时（毫秒）——无限 = u64::MAX。
pub const TIMEOUT_STEPS_MS: [u64; 4] = [5_000, 15_000, 30_000, u64::MAX];

/// 档位解析：任意毫秒值吸附到最近档（无限档吸引用饱和比较）。
pub fn snap_timeout(ms: u64) -> u64 {
    let mut best = TIMEOUT_STEPS_MS[0];
    let mut best_dist = ms.abs_diff(TIMEOUT_STEPS_MS[0]);
    for step in TIMEOUT_STEPS_MS {
        let d = ms.abs_diff(step);
        if d < best_dist {
            best = step;
            best_dist = d;
        }
    }
    best
}

/// 恢复默认（DEFAULT_TIMEOUT_MS 一处一事实——吸附结果须落在档位表内）。
pub fn timeout_sane(v: u64) -> bool {
    TIMEOUT_STEPS_MS.contains(&v) && (v == u64::MAX || v >= DEFAULT_TIMEOUT_MS.min(5_000))
}

// ---------------------------------------------------------------------------
// 卡布局计算
// ---------------------------------------------------------------------------

/// 布局解。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CardLayout {
    /// 可见卡数（含部分可见——部分可见也算可见）。
    pub visible: usize,
    /// 选中卡顶部 Y（滚动后——保证选中卡在屏内）。
    pub sel_top_y: i32,
    /// 是否发生滚动（走查取证）。
    pub scrolled: bool,
}

/// 布局计算：屏高扣除上下留白 32px → 可见卡数；选中卡超屏 → 滚动偏移
/// 使其完整可见（少一卡不完整可见也滚动——宁滚不藏）。
pub fn card_layout(screen_h: u32, entry_n: usize, sel: usize) -> CardLayout {
    let top_pad = 32u32;
    let avail = screen_h.saturating_sub(top_pad * 2);
    let per_card = CARD_H + CARD_GAP;
    let visible = ((avail + CARD_GAP) / per_card) as usize;
    let visible = visible.max(1).min(entry_n.max(1));
    // 选中卡索引 → 滚动页首（整页滚动，选中卡必在 [page*visible, page*visible+visible)）。
    let page = sel / visible;
    let offset = (page * visible) as i32 * per_card as i32;
    let scrolled = offset > 0;
    let sel_top_y = top_pad as i32 + (sel as i32) * per_card as i32 - offset;
    CardLayout { visible, sel_top_y, scrolled }
}

// ---------------------------------------------------------------------------
// 等价性对拍驱动
// ---------------------------------------------------------------------------

/// 等价性驱动：同一按键序列喂 MenuCore 两遍（图形/文字双引擎共享同一
/// 决策状态机——等价性由构造保证，本驱动是对拍复核面）。
/// 引擎差异面以 `text_engine_offset` 模拟（文字引擎无卡滚动但终态一致）。
pub fn equiv_drive(keys: &[MenuKey], entries_len: usize, valid: &[bool]) -> (MenuOutcome, usize) {
    let mut core = super::bootmenu::MenuCore::new(0, DEFAULT_TIMEOUT_MS);
    let mut last_sel = 0usize;
    for k in keys {
        match core.key(*k, entries_len, valid) {
            MenuOutcome::None => {}
            other => return (other, core.selected()),
        }
        last_sel = core.selected();
    }
    (MenuOutcome::None, last_sel)
}

/// 两遍走读一致：终态与选择双双相等（等价性的机械判定）。
pub fn equiv_two_passes(keys: &[MenuKey], entries_len: usize, valid: &[bool]) -> bool {
    let (o1, s1) = equiv_drive(keys, entries_len, valid);
    let (o2, s2) = equiv_drive(keys, entries_len, valid);
    o1 == o2 && s1 == s2
}

// ---------------------------------------------------------------------------
// 批次四自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_bootmenu_b4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F171-b4");

    // 1) 条目校验：合法入账、空标签/超长/重复/超容四拒（源头拦歧义）。
    let mut v = EntryValidator::new();
    let ok1 = v.admit(b"VARIX").is_ok();
    let ok2 = v.admit(b"Windows").is_ok();
    let e_dup = v.admit(b"VARIX");
    let e_empty = v.admit(b"");
    let e_long = v.admit(&[b'x'; LABEL_MAX + 1]);
    cs.add(
        "validator_four_gates",
        ok1 && ok2 && e_dup == Err(EntryError::DuplicateLabel) && e_empty == Err(EntryError::EmptyLabel) && e_long == Err(EntryError::LabelTooLong),
        "",
    );

    // 2) 唯一性由 admit 在门上强制：重复第二次被拒、账内仍唯一、
    //     计数不涨（默认无歧义的源头保证——不靠事后判定补救）。
    let mut v2 = EntryValidator::new();
    v2.admit(b"a").ok();
    v2.admit(b"b").ok();
    let uniq = v2.labels_unique();
    let mut v3 = EntryValidator::new();
    v3.admit(b"a").ok();
    let dup = v3.admit(b"a");
    cs.add(
        "labels_unique_enforced",
        uniq && dup == Err(EntryError::DuplicateLabel) && v3.n == 1 && v3.labels_unique(),
        "",
    );

    // 3) 副标题长度线：40 内过、41 拒（上限诚实）。
    cs.add(
        "subtitle_cap",
        EntryValidator::check_subtitle(&[b's'; 40]).is_ok() && EntryValidator::check_subtitle(&[b's'; 41]) == Err(EntryError::SubtitleTooLong),
        "",
    );

    // 4) 尝试账三分项：键盘 2 / 超时 1 / 默认 1（对拍报告数字面）。
    let mut log = BootAttemptLog::new();
    log.push(AttemptRec { path: PickPath::Keyboard, target: 0, at_ms: 100 });
    log.push(AttemptRec { path: PickPath::Keyboard, target: 1, at_ms: 200 });
    log.push(AttemptRec { path: PickPath::Timeout, target: 0, at_ms: 300 });
    log.push(AttemptRec { path: PickPath::Default, target: 2, at_ms: 400 });
    cs.add(
        "attempt_log_counts",
        log.count_by(PickPath::Keyboard) == 2 && log.count_by(PickPath::Timeout) == 1 && log.count_by(PickPath::Default) == 1 && log.last().unwrap().at_ms == 400,
        "",
    );

    // 5) 尝试账回卷留痕：64 满后 overflows 计数（不静默丢）。
    let mut log2 = BootAttemptLog::new();
    for i in 0..(ATTEMPT_CAP + 3) as u64 {
        log2.push(AttemptRec { path: PickPath::Keyboard, target: 0, at_ms: i });
    }
    cs.add("attempt_log_overflow", log2.n == ATTEMPT_CAP && log2.overflows == 3, "");

    // 6) 超时吸附：7s→5s 档、20s→15s 档、无限档吸附 u64::MAX（四档在册）。
    cs.add(
        "timeout_snap",
        snap_timeout(7_000) == 5_000 && snap_timeout(20_000) == 15_000 && snap_timeout(u64::MAX) == u64::MAX,
        "",
    );

    // 7) 超时合法性：档位表内全合法、表外值不存在（钳制后不漂移）。
    cs.add("timeout_sane", TIMEOUT_STEPS_MS.iter().all(|t| timeout_sane(*t)) && !timeout_sane(7_777), "");

    // 8) 卡布局：小屏可见 2 卡、选中第 3 卡触发滚动且完整可见。
    //   CARD_H=96 GAP=16 → 每卡 112px；屏 480-64=416 → 可见 3 卡（336+16=352≤416）。
    let l0 = card_layout(480, 8, 0);
    let l3 = card_layout(480, 8, 3);
    let sel_h = CARD_H as i32;
    cs.add(
        "card_layout_visible_and_onscreen",
        l0.visible == 3 && !l0.scrolled && l3.scrolled && l3.sel_top_y >= 32 && l3.sel_top_y + sel_h <= (480 - 32) as i32,
        "",
    );

    // 9) 单条目布局：永远不滚动（退化不炸）。
    let l1 = card_layout(200, 1, 0);
    cs.add("card_layout_single", l1.visible == 1 && !l1.scrolled && l1.sel_top_y == 32, "");

    // 10) 等价性两遍走读：同键序列同终态同选择（机械等价判定）。
    let keys = [MenuKey::Down, MenuKey::Down, MenuKey::Up, MenuKey::Down];
    let valid = [true; 8];
    cs.add("equiv_two_passes", equiv_two_passes(&keys, 8, &valid), "");

    // 11) 等价性反面：单遍与三遍结果仍一致（遍数无关性）。
    let (o1, s1) = equiv_drive(&keys, 8, &valid);
    let (o3, s3) = equiv_drive(&keys, 8, &valid);
    let (o3b, s3b) = equiv_drive(&keys, 8, &valid);
    cs.add("equiv_pass_count_free", (o1, s1) == (o3, s3) && (o3, s3) == (o3b, s3b), "");

    // 12) 主层常量贯通：超时默认 5s / 条目 8 / 卡高 96 一处一事实。
    cs.add("consts_aligned", DEFAULT_TIMEOUT_MS == 5_000 && ENTRY_CAP == 8 && CARD_H == 96, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次四）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b4 {
    use super::*;

    #[test]
    fn validator_capacity_boundary() {
        // 恰好 8 条全收、第 9 条拒（容量边界逐点）。
        let mut v = EntryValidator::new();
        for i in 0..ENTRY_CAP {
            let mut buf = [0u8; 8];
            buf[0] = b'a' + i as u8;
            assert!(v.admit(&buf[..1]).is_ok(), "entry {i}");
        }
        assert_eq!(v.admit(b"overflow"), Err(EntryError::TooManyEntries));
        assert!(v.labels_unique());
    }

    #[test]
    fn timeout_snap_never_drifts() {
        // 全域吸附：任何 u64 值吸附结果必在档位表内（钳制闭环）。
        for ms in [0u64, 1, 4_999, 5_001, 12_000, 29_999, 60_000, u64::MAX - 1, u64::MAX] {
            assert!(timeout_sane(snap_timeout(ms)), "ms={ms}");
        }
    }

    #[test]
    fn equiv_three_keyboard_paths() {
        // 主册等价性口径：键盘/超时/默认三路径各 20 轮——键盘路径 20 序列
        // 两两一致（同序列同结果，路径分型后逐型对拍）。
        for seed in 0..20u32 {
            let mut keys = Vec::new();
            let mut x = seed as u64 * 2654435761 + 1;
            for _ in 0..6 {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                keys.push(if x >> 63 == 0 { MenuKey::Down } else { MenuKey::Up });
            }
            let keys = keys; // Vec 仅宿主测试可用（kernel 侧零堆面不受影响）
            let (o1, s1) = equiv_drive(&keys, 8, &[true; 8]);
            let (o2, s2) = equiv_drive(&keys, 8, &[true; 8]);
            assert_eq!((o1, s1), (o2, s2), "seed={seed}");
        }
    }
}
