//! ibase — Varix STAR I · I 通用域·四分队（AI-U4 · F551-F600）共享底盘。
//!
//! 判据唯一源：《Varix STAR I start.md》第 7 部分 I 通用域 F551-F600 各节。
//! 本文件只收「多项共用」的小件，一项一事实：
//!
//! - [`hhmm`]：HHMM 时刻解析与跨午夜时段判定（F554 定时勿扰 / F567 定时静音 /
//!   F565 登录问候共用的唯一时段语义源——三处不各写一套）；
//! - [`Tint12`]：12 色标签板（F566 文件夹颜色标记唯一色源，走主题令牌名）；
//! - [`dedupe_name`]：重名递增命名（F588 图片粘贴为文件「截图 2026-09-25_1430」
//!   F521 同规的唯一命名语义源）；
//! - [`Stamp`]：统一 ms 钟注入约定（全域模块一律收 ms 实参，不自取时钟）。
//!
//! 铁律对齐：零堆热路径约束按模块自持；本底盘提供的都是定长/纯函数件。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// HHMM 时刻与跨午夜时段（F554 / F567 / F565 唯一语义源）
// ---------------------------------------------------------------------------

/// 一天分钟数（24×60）。
pub const MINUTES_PER_DAY: u32 = 1_440;

/// HHMM 解析：`h*100+m`，h<24、m<60 才合法。
///
/// 返回当天第几分钟（0..1440）。
pub fn hhmm(h: u32, m: u32) -> Option<u32> {
    if h < 24 && m < 60 {
        Some(h * 60 + m)
    } else {
        None
    }
}

/// 判定 `now_min`（当天分钟）是否落在时段 `[start, end)` 内。
///
/// **跨午夜语义**：`start <= end` 为普通时段（含 start、不含 end）；
/// `start > end` 为跨午夜时段（如 22:00-8:00：now>=start 或 now<end）。
/// start == end 视为空时段（永不命中——诚实零覆盖，不做 24h 魔法）。
pub fn in_window(now_min: u32, start: u32, end: u32) -> bool {
    if start == end {
        return false;
    }
    if start < end {
        now_min >= start && now_min < end
    } else {
        now_min >= start || now_min < end
    }
}

// ---------------------------------------------------------------------------
// 12 色标签板（F566 唯一色源）
// ---------------------------------------------------------------------------

/// F566 十二色标签——索引即语义位，颜色一律走主题令牌名（F151），
/// 本枚举只持「令牌名 + 缺省语义文案」，不持 RGB（一处一事实：色值在令牌表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint12 {
    Red,
    Orange,
    Amber,
    Yellow,
    Lime,
    Green,
    Teal,
    Cyan,
    Blue,
    Violet,
    Magenta,
    Slate,
}

impl Tint12 {
    /// 全部十二枚（顺序稳定——持久化按序号存取，不许重排）。
    pub const ALL: [Tint12; 12] = [
        Tint12::Red,
        Tint12::Orange,
        Tint12::Amber,
        Tint12::Yellow,
        Tint12::Lime,
        Tint12::Green,
        Tint12::Teal,
        Tint12::Cyan,
        Tint12::Blue,
        Tint12::Violet,
        Tint12::Magenta,
        Tint12::Slate,
    ];

    /// 持久化序号（0-11）。
    pub fn index(self) -> u8 {
        self as u8
    }

    /// 自持久化序号还原（越界返回 None——不给默认值，防脏数据静默变脸）。
    pub fn from_index(i: u8) -> Option<Tint12> {
        Self::ALL.get(i as usize).copied()
    }

    /// 主题令牌名（F151 令牌表锚点）。
    pub fn token(self) -> &'static str {
        match self {
            Tint12::Red => "tint-red",
            Tint12::Orange => "tint-orange",
            Tint12::Amber => "tint-amber",
            Tint12::Yellow => "tint-yellow",
            Tint12::Lime => "tint-lime",
            Tint12::Green => "tint-green",
            Tint12::Teal => "tint-teal",
            Tint12::Cyan => "tint-cyan",
            Tint12::Blue => "tint-blue",
            Tint12::Violet => "tint-violet",
            Tint12::Magenta => "tint-magenta",
            Tint12::Slate => "tint-slate",
        }
    }

    /// 缺省语义文案（用户可改——改后语义存用户配置层，色令牌不变）。
    pub fn default_semantics(self) -> &'static str {
        match self {
            Tint12::Red => "项目红",
            Tint12::Orange => "待办橙",
            Tint12::Amber => "提醒琥珀",
            Tint12::Yellow => "财务黄",
            Tint12::Lime => "草稿青柠",
            Tint12::Green => "个人绿",
            Tint12::Teal => "收藏青",
            Tint12::Cyan => "资料青蓝",
            Tint12::Blue => "工作蓝",
            Tint12::Violet => "创意紫",
            Tint12::Magenta => "灵感品红",
            Tint12::Slate => "归档灰",
        }
    }
}

// ---------------------------------------------------------------------------
// 重名递增命名（F588 唯一命名语义源，F521 同规）
// ---------------------------------------------------------------------------

/// 为 `base`（不含扩展名）在 `taken`（已占用名集合，不含扩展名）里找落位名：
/// 首选 `base`；被占则 `base (2)`、`base (3)` … 递增。
///
/// `reserve` 上限防御：递增超过 4096 次返回 None（磁盘上同名 4096 份是异常态，
/// 诚实拒绝优于静默无限递增）。
pub fn dedupe_name(base: &str, taken: &[&str]) -> Option<alloc::string::String> {
    if !taken.contains(&base) {
        return Some(base.into());
    }
    for n in 2..=4_097u32 {
        let cand = alloc::format!("{} ({})", base, n);
        if !taken.iter().any(|t| *t == cand) {
            return Some(cand);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Clone 环形账（留痕类记录含 String，不能走 sbase 的 Copy 版 RingLog）
// ---------------------------------------------------------------------------

/// 定长环形账（Clone 版）——新入逐出最旧，`newest_first` 按新到旧出列。
///
/// 与 [`crate::star::sbase::RingLog`]（Copy 版）分工：本域留痕记录带
/// 调用方名称（String），走 Clone 语义。
pub struct CloneLog<T: Clone, const N: usize> {
    buf: [Option<T>; N],
    head: usize,
    len: usize,
}

impl<T: Clone, const N: usize> CloneLog<T, N> {
    pub fn push(&mut self, item: T) {
        self.buf[self.head] = Some(item);
        self.head = (self.head + 1) % N;
        if self.len < N {
            self.len += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 新到旧快照。
    pub fn newest_first(&self) -> alloc::vec::Vec<T> {
        let mut out = alloc::vec::Vec::with_capacity(self.len);
        let mut i = (self.head + N - 1) % N;
        for _ in 0..self.len {
            if let Some(v) = &self.buf[i] {
                out.push(v.clone());
            }
            i = (i + N - 1) % N;
        }
        out
    }
}

impl<T: Clone, const N: usize> Default for CloneLog<T, N> {
    fn default() -> Self {
        CloneLog {
            buf: [(); N].map(|_| None),
            head: 0,
            len: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// 域常量
// ---------------------------------------------------------------------------

/// 域标识（CheckSet 聚合用）。
pub const ISTAR_DOMAIN: &str = "istar-u4";

// ---------------------------------------------------------------------------
// ibase 自检
// ---------------------------------------------------------------------------

pub fn run_ibase_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. HHMM 解析：合法三例 + 越界拒绝（h=24 / m=60）。
    let ok1 = hhmm(0, 0) == Some(0);
    let ok2 = hhmm(22, 0) == Some(1_320);
    let ok3 = hhmm(23, 59) == Some(1_439);
    let bad1 = hhmm(24, 0).is_none();
    let bad2 = hhmm(12, 60).is_none();
    set.add("hhmm parse and reject", ok1 && ok2 && ok3 && bad1 && bad2, "");

    // 2. 普通时段：含头不含尾（9:00-12:00 → 540 命中、720 不命中、719 命中）。
    let a = in_window(540, 540, 720);
    let b = in_window(719, 540, 720);
    let c = !in_window(720, 540, 720);
    set.add("normal window half-open", a && b && c, "");

    // 3. 跨午夜时段（22:00-8:00）：23:00 命中、07:59 命中、08:00 不命中、21:59 不命中。
    let a = in_window(1_380, 1_320, 480);
    let b = in_window(479, 1_320, 480);
    let c = !in_window(480, 1_320, 480);
    let d = !in_window(1_319, 1_320, 480);
    set.add("cross-midnight window", a && b && c && d, "");

    // 4. 空时段（start==end）永不命中——诚实零覆盖。
    set.add("empty window never matches", !in_window(600, 600, 600), "");

    // 5. Tint12 十二枚序号 round-trip 全对；越界还原 None（脏数据不静默变脸）。
    let rt = Tint12::ALL
        .iter()
        .all(|t| Tint12::from_index(t.index()) == Some(*t));
    let oob = Tint12::from_index(12).is_none();
    set.add("tint12 roundtrip and oob", rt && oob, "");

    // 6. Tint12 令牌名唯一且 12 个（主题令牌锚点不重复）。
    let mut uniq = true;
    for i in 0..12u8 {
        for j in (i + 1)..12u8 {
            let a = Tint12::from_index(i).unwrap().token();
            let b = Tint12::from_index(j).unwrap().token();
            if a == b {
                uniq = false;
            }
        }
    }
    set.add("tint12 tokens unique", uniq, "");

    // 7. 重名递增：首用直落、重名 (2)、连占 (3)；空集合直落。
    let d1 = dedupe_name("截图 2026-09-25_1430", &[]).unwrap();
    let d2 = dedupe_name("name", &["name"]).unwrap();
    let d3 = dedupe_name("name", &["name", "name (2)"]).unwrap();
    set.add(
        "dedupe increments",
        d1 == "截图 2026-09-25_1430" && d2 == "name (2)" && d3 == "name (3)",
        "",
    );

    // 8. 重名递增防御：base+4096 个别名全占（"x (2)".."x (4097)"）→ 诚实 None。
    let mut flood: alloc::vec::Vec<alloc::string::String> = alloc::vec::Vec::new();
    flood.push("x".into());
    for n in 2..=4_097u32 {
        flood.push(alloc::format!("x ({})", n));
    }
    let refs: alloc::vec::Vec<&str> = flood.iter().map(|s| s.as_str()).collect();
    set.add("dedupe cap honest none", dedupe_name("x", &refs).is_none(), "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_midnight_exact_bounds() {
        // 22:00-8:00 四点边界钉死（F554 判据用例同参）。
        assert!(in_window(22 * 60, 22 * 60, 8 * 60));
        assert!(in_window(7 * 60 + 59, 22 * 60, 8 * 60));
        assert!(!in_window(8 * 60, 22 * 60, 8 * 60));
        assert!(!in_window(21 * 60 + 59, 22 * 60, 8 * 60));
    }

    #[test]
    fn tint12_order_stable() {
        assert_eq!(Tint12::Red.index(), 0);
        assert_eq!(Tint12::Slate.index(), 11);
        assert_eq!(Tint12::from_index(5), Some(Tint12::Green));
    }

    #[test]
    fn dedupe_exact_cap() {
        let mut taken: alloc::vec::Vec<alloc::string::String> = alloc::vec::Vec::new();
        taken.push("x".into());
        for n in 2..=4_097u32 {
            taken.push(alloc::format!("x ({})", n));
        }
        // base + 4096 个别名全被占 → 无落位 → None（诚实拒绝）。
        let refs: alloc::vec::Vec<&str> = taken.iter().map(|s| s.as_str()).collect();
        assert!(dedupe_name("x", &refs).is_none());
        // 差一仍可落位（4097 上限的最后一位没占满时给最后候选）。
        let mut almost: alloc::vec::Vec<alloc::string::String> = taken.clone();
        almost.pop(); // 腾出 "x (4097)"
        let refs2: alloc::vec::Vec<&str> = almost.iter().map(|s| s.as_str()).collect();
        assert_eq!(dedupe_name("x", &refs2).as_deref(), Some("x (4097)"));
    }

    #[test]
    fn dedupe_unicode_safe() {
        let got = dedupe_name("报告", &["报告"]).unwrap();
        assert_eq!(got, "报告 (2)");
    }
}
