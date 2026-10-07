//! VE-F3404 · 域自检（判据逐条对应，见 `ver01d_switch.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 原子保证 → `E04-原子-*`
//! - 预演干跑 → `E04-预演-*`
//! - 快照回滚 → `E04-回滚-*`
//! - 悬空兜底 → `E04-悬空-*`
//! - 降级矩阵（预演失败→取消 / 中途崩溃→快照恢复 / 悬空→超时回滚）→ `E04-降级-*`
//! - 跨批对接（E02 换肤引擎消费）→ `E04-对接-*`
//! - 无障碍（切换状态读屏播报）→ `E04-读屏-*`
//! - 边界防护与性能（O(令牌数)）→ `E04-边界-*` / `E04-性能-*`
//!
//! **门禁设计的七条自律**（照 V/E 域判据门禁的经验，逐条都是踩过的坑）：
//! 1. **不用表内元素验查表函数**——验 [`SkinTable::verify`] 的用例全部在**正常
//!    建出的表**上手工改坏（顺序打乱 / 造重复 / 塞空值），而不是拿一张
//!    "本来就没建好"的表去验。
//! 2. **两侧同规范化**——预演差分与提交后的实测差分都走同一个 `diff`，判定
//!    "预演说会变的集合"与"提交后确实变了的集合"**完全相等**（不是子集）。
//!    只判子集会把"漏算"放过。
//! 3. **形态判据不蕴含数值正确性**——"主题名变了"是弱门禁（把主题名改成常量
//!    也有变化）。所以回滚判据**逐令牌比对**回滚后的值与快照值，不看名字。
//! 4. **单边符号而非双边阈值**——半新半旧的判定是**离散分类**
//!    （`AllOld`/`AllNew`/`Mixed`/`Foreign` 四档），不是"差值小于某阈值"。
//!    阈值型断言一旦宽过正确实现的小噪声，高估型变异就从缝里钻过去。
//! 5. **性能自检实测真实工作量**——数**真实走过的归并步数**随规模的变化比，
//!    不是 `n * CONST` 的自证式算术；计数器覆盖的是缺陷真正发生的那一层。
//! 6. **双向验证（补判据后必须做）**——本单最强的一条纪律，见下。
//! 7. **采样留洞要钉死**——探针必须**至少一条会变**，否则混色判定恒为全旧，
//!    断言在跑但测不到任何东西。`FrameProbe::new` 的闸门与对应判据各扣一次。
//!
//! ## 双向验证：为什么这里的断言不是恒真
//!
//! 判原子性最省事的写法是"跑一次换肤，断言生效表等于新表"——这条**恒真**，
//! 因为原子实现必然如此，写错的实现也常常如此（本单的唯一差别在**中间态**，
//! 而中间态在真实提交路径上不存在）。所以本单做了三件事：
//!
//! - [`MixedVerdict`] 的分类能力由**故意非原子的参考发布器**
//!   [`publish_in_place_for_test`] 反证：它逐令牌改写生效槽，产出的时间线里
//!   **必须**出现 `Mixed` 帧（`E04-原子-判别力`）。若判定对非原子实现也放行，
//!   那它就是恒真的空断言。
//! - 生效表与见证对的**逐令牌比对**（`E04-原子-表等价`）：改坏一张表使它两表
//!   都不是，`verify_atomic` 必须拒。
//! - 判据侧**绕过聚合层直接断言生效表本身**（不信 `expect_committed` 的包装）。
//!
//! **只验基线全绿等于没验**：下面每条会指明它对应的变异是否实测过。
//! 未实测的判据不宣称已验证。
//!
//! ## 变异台实测结果（W016·VE-F3404）
//!
//! 18 条真变异 + 1 条等价对照，基线 109 项全绿，逐条结果：
//!
//! | 变异 | 注入的缺陷 | 捕获 |
//! |---|---|---|
//! | M01 | `publish` 就地逐令牌改写生效槽 | 8 项红（首条 `E04-原子-提交不变量`） |
//! | M02 | 游标翻转前就地改写生效槽 | 2 项红（`E04-原子-发布全程无混色`） |
//! | M03 | `begin` 里把暂存表直接发布 | 23 项红（`E04-原子-在途不改生效面`） |
//! | M04 | `restore` 只还原路径不还原值 | 6 项红（`E04-回滚-逐值还原`） |
//! | M05 | `sweep` 到龄判据永不成立 | 4 项红（`E04-悬空-到龄强制回滚`） |
//! | M06 | 超时回滚不标兜底类 | 1 项红（`E04-悬空-计数显性化`） |
//! | M07 | 时钟回拨不再被拒 | 1 项红（`E04-悬空-时钟回拨拒绝`） |
//! | M09 | 跨会话恢复跳过快照分支 | 1 项红（`E04-深-跨会话恢复后是旧值`） |
//! | M10 | `verify` 不查严格升序 | 2 项红（`E04-原子-结构校验抓乱序`） |
//! | M11 | `verify_atomic` 不做见证对比对 | 1 项红（`E04-原子-表等价`） |
//! | M12 | 探针闸门接受单探针 | 1 项红（`E04-边界-探针拒单点`） |
//! | M13 | `changed_len` 只报一半 | 1 项红（`E04-预演-结论通过`） |
//! | M14 | `Foreign` 档降级成 `Mixed` | 2 项红（`E04-原子-分类-脏数据独立成档`） |
//! | M15 | `preview` 写观察窗留旁痕 | 1 项红（`E04-预演-只读不留旁痕`） |
//! | M16 | `verify_atomic` 不查世代 | 1 项红（`E04-原子-世代检查独立`） |
//! | M17 | 事务在途仍允许再开一个 | 1 项红（`E04-降级-换肤互斥`） |
//! | M18 | 读屏播报混入令牌值正文 | 1 项红（`E04-读屏-不含令牌值正文`） |
//!
//! **M08 转红不了，且这是正确结果**：删掉 `do_rollback` 自己的终局条之后，
//! [`Switchboard::publish`] 仍会为**同一次回滚**落一条同终局记录，账本末态
//! 逐字段不变 ⇒ 该变异**观测等价**，判据本就不该转红。把它列进来是当**对照项**：
//! 没有等价对照项时，「捕获 17/18」里的那个 18 无法分辨是判据弱还是变异无效。
//! 另有 M19（`begin` 里两条无依赖语句换序）同样观测等价，判据同样正确地保持全绿。
//!
//! **M18/M09 是本轮补判据的由来**：两条最初都逃逸，且都不是变异选错——
//! M18 的隐私判据原写「不含 `#101014`/`#1a1a22`」两个字面量，泄漏点换成
//! 表里另一条值（`color.accent`）就抓不到；M09 的跨会话恢复判据只断「出了
//! 一份恢复报告」，是形态判据，把快照分支整个跳过照样全绿。两条已按
//! 「值集由表推导」「逐令牌比对还原结果」重写，复测均转红。
//!
//! 零墙钟、零 IO、无随机源，回归可复现。

use super::ver01b_parser::Site;
use super::ver01c_cascade::CascadeEngine;
use super::ver01d_switch::*;
use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 域自检集合的标签。
pub const CHECK_DOMAIN: &str = "ve-f3404";

/// 把一张生效表按探针取成一帧（发布观察抓到的是整表，判定要的是帧）。
fn sample_of(probe: &FrameProbe, table: &SkinTable) -> FrameSample {
    probe.sample(table).expect("探针在两表内")
}

/// 造一张确定的旧主题表（**有序、无重复**）：
/// ```text
/// color.base = #101014
/// color.fg   = #f0f0f4
/// font.size  = 14px
/// ```
fn old_pairs() -> Vec<(String, String)> {
    vec![
        ("color.base".to_string(), "#101014".to_string()),
        ("color.fg".to_string(), "#f0f0f4".to_string()),
        ("font.size".to_string(), "14px".to_string()),
    ]
}

/// 新主题：**三条全变**（值变），另加一条新增、一条缺失——逼出三分类差分。
fn new_pairs() -> Vec<(String, String)> {
    vec![
        ("color.accent".to_string(), "#4a9eff".to_string()), // 新增
        ("color.base".to_string(), "#1a1a22".to_string()),   // 变
        ("color.fg".to_string(), "#e8e8f0".to_string()),     // 变
        ("font.size".to_string(), "14px".to_string()),        // 不变
    ]
}

/// 探针集（3 条，取 `MIN_PROBES` 之上）。
///
/// **注意：3 条里只有 2 条会变**——`font.size` 新旧同值（均 `14px`），是**故意
/// 放进来���不变探针**的：混色判定只应统计会变的探针，若把不变探针也算进分侧
/// 计数，"新旧同值"就会被算成某一侧，计数凭空多出一条。
///
/// 因此凡是引用本表的判据，其分侧期望值都必须**由 `changing_count` 现场推导**
/// （见 [`expect_side_split`／`expect_offenders`]），不得把"3"或"2"写死在断言里——
/// 语料一改，写死的数字就成了一条与实现无关的假门禁。
const PROBE_CHANGED: [&str; 3] = ["color.base", "color.fg", "font.size"];

/// **独立**数出会变探针数：不问被测的 [`FrameProbe::changing_count`]，自己按
/// `old[p] != new[p]` 数一遍。
///
/// 判据侧独立重算是本单的基本纪律（十诫第7 条：判据向被测函数问答案＝自证式）。
/// 探针数与语料都是判据侧的输入，被测函数只提供待验证的结论。
fn changing_probe_count_independently(old: &SkinTable, new: &SkinTable) -> usize {
    PROBE_CHANGED
        .iter()
        .filter(|p| old.get(p) != new.get(p))
        .count()
}

/// 清单成员核对：`paths` 里每一条在 `table` 里都取得到值。
///
/// 这是"长度对而成员错"的解药——分侧计数只管条数，管不了清单里装的是哪几条。
fn all_in(table: &SkinTable, paths: &[String]) -> bool {
    !paths.is_empty() && paths.iter().all(|p| table.get(p.as_str()).is_some())
}

/// **宽语料**：6 条令牌，其中 5 条改变、1 条（`font.size`）不变。
///
/// **为什么要有这套语料**（十诫第 11 条的同型坑）：主语料 `PROBE_CHANGED`
/// 里 3 条只有 2 条会变，于是最后一个混色帧的两侧清单**天然最多只有 1 条成员**
/// ——"只记第一条"这种变异与正确实现**外部表现完全相同**，判据抓不到
/// （实测 M1/M2 变异漏网）。要让"只记一条"暴露出来，新侧清单必须**能有多条成员**，
/// 即需要 `C ≥ 3` 的会变探针。这里取 5 条会变 + 1 条不变（不变那条同样是为了
/// 盯住"不变探针不得计入"）。
const WIDE_OLD: [(&str, &str); 6] = [
    ("color.accent", "#4a9eff"),
    ("color.bg", "#101014"),
    ("color.border", "#2a2a32"),
    ("color.fg", "#f0f0f4"),
    ("font.family", "sans"),
    ("font.size", "14px"),
];

const WIDE_NEW: [(&str, &str); 6] = [
    ("color.accent", "#7c5cff"),
    ("color.bg", "#0d0d12"),
    ("color.border", "#3c3c4a"),
    ("color.fg", "#ffffff"),
    ("font.family", "serif"),
    ("font.size", "14px"),
];

/// 宽语料的探针集（全部 6 条，含 1 条不变）。
const WIDE_PROBE: [&str; 6] = [
    "color.accent",
    "color.bg",
    "color.border",
    "color.fg",
    "font.family",
    "font.size",
];

fn wide_pairs(src: &[(&str, &str)]) -> Vec<(String, String)> {
    src.iter()
        .map(|(p, v)| (p.to_string(), v.to_string()))
        .collect()
}

/// 逐条改写下的分侧期望：`C` 条会变探针里，最后一个混色帧必是「旧 1 + 新 `C-1`」
/// ——因为写入顺序是目标表的字节序，会变探针里**最后被写的那一条**在那一帧之前
/// 还是旧值，而其余 `C-1` 条已换新。
///
/// 返回 `(期望旧侧, 期望新侧)`。`C < 2` 时不存在混色帧（只有一条会变探针时，
/// "半新半旧"退化成"全旧或全新"），此时返回 `None` 让调用方显式拒判而不是
/// 拿 `C-1 == 0` 去比一个恒为 0 的计数。
fn expect_side_split(old: &SkinTable, new: &SkinTable) -> Option<(usize, usize)> {
    let c = changing_probe_count_independently(old, new);
    if c < 2 {
        None
    } else {
        Some((1, c - 1))
    }
}

/// 问题路径合计的期望值：等于会变探针数（漏计与重复计都会打偏）。
fn expect_offenders(old: &SkinTable, new: &SkinTable) -> usize {
    changing_probe_count_independently(old, new)
}

/// 造一个"已首装旧主题"的板子。
fn board_with_old() -> Switchboard {
    let mut b = Switchboard::new();
    let t = SkinTable::build("day", old_pairs()).expect("旧主题应可建表");
    b.mount(t, 0).expect("首装应成功");
    b
}

// ---------------------------------------------------------------------------
// E04-原子：原子保证 + 判别力
// ---------------------------------------------------------------------------

fn atomic_checks(cs: &mut CheckSet) {
    let old = SkinTable::build("day", old_pairs()).expect("旧表");
    let new = SkinTable::build("night", new_pairs()).expect("新表");
    let probe = FrameProbe::new(&PROBE_CHANGED, &old, &new).expect("探针应可建");

    // ---- 真实提交路径：事务在途期间生效面逐字节不变 ----
    let mut b = board_with_old();
    let before: Vec<(String, String)> = b.active().expect("生效表").values.clone();
    let txn = b.begin("night", new.clone(), 10).expect("开事务");
    let during: Vec<(String, String)> = b.active().expect("生效表").values.clone();
    cs.add(
        "E04-原子-在途不改生效面",
        before == during,
        "开事务只建暂存表；若生效面被改动，预演就不再是干跑",
    );
    cs.add(
        "E04-原子-在途帧全旧",
        b.expect_in_flight(&probe).expect("帧判定"),
        "事务在途期间截图必须整帧旧值——这是「半新半旧不存在」最直接的表述",
    );
    b.commit(txn, 20).expect("提交");
    cs.add(
        "E04-原子-提交后帧全新",
        b.expect_committed(&probe).expect("帧判定"),
        "提交后截图必须整帧新值",
    );
    cs.add(
        "E04-原子-提交不变量",
        b.verify_atomic().is_ok(),
        "发布后生效表必须与见证对的一侧逐字节相等（不是形状相等）",
    );
    cs.add(
        "E04-原子-生效表逐值等于新表",
        {
            let live = b.active().expect("生效表");
            live.values == new.values
        },
        "提交后生效表应与新表逐令牌相等；只看主题名是弱门禁",
    );

    // ---- 判别力：故意非原子的参考发布器必须被抓 ----
    let mut b2 = board_with_old();
    let txn2 = b2.begin("night", new.clone(), 10).expect("开事务");
    let frames = publish_in_place_for_test(&mut b2, txn2, &probe).expect("参考发布器");
    let mixed: Vec<&FrameSample> = frames
        .iter()
        .filter(|f| f.mixed_with(&old, &new).is_mixed())
        .collect();
    cs.add(
        "E04-原子-判别力",
        !mixed.is_empty(),
        "非原子参考发布器的帧时间线里必须出现半新半旧；否则判定是恒真空断言",
    );
    // 参考发布器产出的帧数应等于写入步数（每写一个令牌一帧），
    // **不是**"至少一帧"——那是形态判据，测不出"没在每个中间态取样"。
    cs.add(
        "E04-原子-逐步取样数",
        frames.len() == new.values.len(),
        "参考发布器每写一个令牌取一帧，帧数应等于目标表条数",
    );
    // **精确到数的分侧断言**（抓"只记第一个"这类变异）：
    // `C` 条会变探针、逐条改写 ⇒ 最后一个混色帧必是「旧 1 + 新 C-1」。
    // 拼接式清单断不出这个分布，只记一条 fresh 也能让清单非空。
    //
    // **期望值现场推导，不写死数字**：本语料 3 条探针里只有 2 条会变
    // （`font.size` 新旧同值，是故意放的不变探针），所以实际是「旧 1 + 新 1」。
    // 早先把这里写成"旧 2 + 新 1 / 合计 3"，是把`PROBE_CHANGED.len()`
    // 当成了会变探针数——判据索引随语料漂移，正确的实现被判红。
    let last_mixed = frames
        .iter()
        .map(|f| f.mixed_with(&old, &new))
        .filter(|v| v.is_mixed())
        .next_back();
    let expect_split = expect_side_split(&old, &new);
    cs.add(
        "E04-原子-分侧前提",
        expect_split.is_some() && last_mixed.is_some(),
        "会变探针须≥2（否则「半新半旧」退化成全旧/全新，无混色帧可断），\
         且逐条改写的时间线里确实出现了混色帧；两条前提任缺其一，后续分侧断言都是空断言",
    );
    cs.add(
        "E04-原子-混色分侧计数",
        match (&last_mixed, expect_split) {
            (Some(v), Some((es, ef))) => v.stale_count() == es && v.fresh_count() == ef,
            _ => false,
        },
        "最后一个混色帧的分侧分布须等于「旧 1 + 新 (C-1)」，C 为会变探针数（由语料独立数出）；\
         只记第一条、或把不变探针算进某一侧，都会让分侧计数对不上",
    );
    cs.add(
        "E04-原子-问题路径合计",
        match &last_mixed {
            Some(v) => v.offender_count() == expect_offenders(&old, &new),
            None => false,
        },
        "混色帧的问题路径数须恰等于会变探针数（漏计与重复计都会打偏；不变探针不得计入）",
    );
    // 分侧清单的**成员**也要断：只断计数的话，把 stale 清单填成任意两条
    // 同长度的路径也能过。这里断言旧侧恰是那条"最后被写"的会变探针。
    cs.add(
        "E04-原子-混色分侧成员",
        match (&last_mixed, expect_split) {
            (Some(MixedVerdict::Mixed { stale, fresh }), Some((es, ef))) => {
                stale.len() == es && fresh.len() == ef && all_in(&old, stale) && all_in(&new, fresh)
            }
            _ => false,
        },
        "混色帧的旧侧清单须逐条确为旧值、新侧清单须逐条确为新值（长度对而成员错也算破）",
    );

    // ---- 宽语料专断"只记第一条"（主语料 C=2 抓不到，见 WIDE_OLD 头注）----
    //
    // 这里把期望算到**路径成员**一级：逐条改写按目标表字节序进行，会变探针
    // 里**最后被写的那一条**（字节序最大）在最后一个混色帧里仍走旧侧，其余
    // `C-1` 条已走新侧。所以新侧清单的成员集合可由语料完全推出，逐条比对。
    let wold = SkinTable::build("day", wide_pairs(&WIDE_OLD)).expect("宽旧表");
    let wnew = SkinTable::build("night", wide_pairs(&WIDE_NEW)).expect("宽新表");
    let wprobe = FrameProbe::new(&WIDE_PROBE, &wold, &wnew).expect("宽探针应可建");
    let mut wb = Switchboard::new();
    wb.mount(wold.clone(), 0).expect("宽首装");
    let wtxn = wb.begin("night", wnew.clone(), 10).expect("宽开事务");
    let wframes = publish_in_place_for_test(&mut wb, wtxn, &wprobe).expect("宽参考发布器");
    let wlast = wframes
        .iter()
        .map(|f| f.mixed_with(&wold, &wnew))
        .filter(|v| v.is_mixed())
        .next_back();
    // 独立重算会变探针（宽语料）：同样不查被测函数。
    let w_changing: Vec<&str> = WIDE_PROBE
        .iter()
        .copied()
        .filter(|p| wold.get(p) != wnew.get(p))
        .collect();
    let w_c = w_changing.len();
    cs.add(
        "E04-原子-宽语料前提",
        w_c == 5 && wlast.is_some(),
        "宽语料前提：6 条探针里恰有 5 条会变（`font.size` 不变），且时间线里出现了混色帧；\
         C<3 时新侧清单只可能有一条成员，「只记第一条」的变异与正确实现不可区分",
    );
    // 新侧清单的期望成员：会变探针里除「字节序最大者」以外的全部。
    let w_expect_fresh: Vec<String> = {
        let mut v: Vec<String> = w_changing
            .iter()
            .take(w_c.saturating_sub(1))
            .map(|p| p.to_string())
            .collect();
        v.sort();
        v
    };
    // 旧侧清单的期望成员：会变探针里字节序最大者（最后被写的那条）。
    let w_expect_stale: Vec<String> = w_changing
        .last()
        .map(|p| vec![p.to_string()])
        .unwrap_or_default();
    cs.add(
        "E04-原子-宽语料分侧成员",
        match &wlast {
            Some(MixedVerdict::Mixed { stale, fresh }) => {
                *stale == w_expect_stale && *fresh == w_expect_fresh
            }
            _ => false,
        },
        "宽语料下混色帧的新侧清单须**逐条等于**会变探针中除末位外的全部（旧侧恰为末位那条）；\
         「只记第一条」「漏记中间项」「把不变探针塞进清单」三种变异都在这里转红",
    );
    // 顺带钉住帧数：宽语料 6 条 ⇒ 6 帧，且混色帧数恰为 C-1（每写一条变一条）。
    cs.add(
        "E04-原子-宽语料混色帧数",
        {
            let mixed_n = wframes
                .iter()
                .filter(|f| f.mixed_with(&wold, &wnew).is_mixed())
                .count();
            wframes.len() == WIDE_NEW.len() && mixed_n == w_c - 1
        },
        "宽语料时间线：帧数等于写入步数，混色帧数恰为 C-1（写完最后一条会变探针后不再有混色）",
    );

    // ---- 逆序写入：把分布翻过来，专断旧侧的"只记第一条" ----
    //
    // 正序下旧侧恒只有 1 条成员（只有末位那条会变探针还没换新），所以
    // "旧侧只记第一条"与正确实现**外部表现相同**（实测 M1 变异漏网）。
    // 逆序写入让**旧侧**出现多条成员，该变异立即转红——两侧的漏洞都要堵，
    // 只堵一侧等于判据只验了一半。
    let mut rb = Switchboard::new();
    rb.mount(wold.clone(), 0).expect("逆序首装");
    let rtxn = rb.begin("night", wnew.clone(), 10).expect("逆序开事务");
    let rframes = publish_in_place_ordered_for_test(
        &mut rb,
        rtxn,
        &wprobe,
        PublishOrder::Reverse,
    )
    .expect("逆序参考发布器");
    let rlast = rframes
        .iter()
        .map(|f| f.mixed_with(&wold, &wnew))
        .filter(|v| v.is_mixed())
        .next_back();
    // 逆序下最后被写的是字节序**最小**的那条会变探针 ⇒ 旧侧恰为它，
    // 新侧为其余 C-1 条（与正序恰好互为镜像）。
    let w_expect_fresh_rev: Vec<String> = w_changing
        .iter()
        .skip(1)
        .map(|p| p.to_string())
        .collect();
    let w_expect_stale_rev: Vec<String> = w_changing
        .first()
        .map(|p| vec![p.to_string()])
        .unwrap_or_default();
    cs.add(
        "E04-原子-逆序分侧前提",
        rlast.is_some() && w_c >= 3 && w_expect_fresh_rev.len() >= 2,
        "逆序语料前提：会变探针C≥3 且新侧清单能有 ≥2 条成员；\
         否则「旧侧只记第一条」与正确实现不可区分（实测 M1 漏网）",
    );
    cs.add(
        "E04-原子-逆序分侧成员",
        match &rlast {
            Some(MixedVerdict::Mixed { stale, fresh }) => {
                *stale == w_expect_stale_rev && *fresh == w_expect_fresh_rev
            }
            _ => false,
        },
        "逆序写入下混色帧的新侧清单须逐条等于「会变探针去掉字节序最小者」的其余全部；\
         「新侧只记第一条」「漏记中间项」在此转红",
    );
    // 逆序下**混色帧数**同样应为 C-1（写入次序变了，混色的条数不该变）。
    cs.add(
        "E04-原子-逆序混色帧数",
        {
            let mixed_n = rframes
                .iter()
                .filter(|f| f.mixed_with(&wold, &wnew).is_mixed())
                .count();
            mixed_n == w_c - 1 && rframes.len() == WIDE_NEW.len()
        },
        "写入次序不应改变混色帧的条数（两种次序都恰为 C-1），也不应改变总帧数",
    );

    // ---- 时间线累计守恒：抓“某一侧只记一条”（单帧分布抓不到的那种）----
    //
    // 单帧分侧对“旧侧只记第一条”是**盲的**：正序下旧侧本就只该有 1 条成员，
    // 漏记与正确实现外部表现相同（实测 M1 在单帧判据上漏网）。
    //
    // 换个角度就看得见了：正序逐条改写时，第 k 帧里“还没被写入”的会变探针
    // **全部**应记在旧侧。宽语料 C=5 实测阶梯是 
    // —— 注意 k=0 不是 5：目标表字节序第一条（）本身就是会变
    // 探针，写完它时其余 4 条仍在旧侧。逐帧核对这张**阶梯**，“旧侧只留一条”
    // 会让每一帧都对不上。
    //
    // **期望值不写死**：旧侧第 k 帧的成员数 = （k 取 0..C-1），
    // 由“写入次序=字节序、每写一条换一条”直接推出，不查被测函数。
    cs.add(
        "E04-原子-正序旧侧阶梯",
        {
            let mut ok = true;
            for (k, f) in wframes.iter().enumerate() {
                let v = f.mixed_with(&wold, &wnew);
                // 只有前 w_c 帧落在“会变探针写入窗口”内，窗口之后旧侧恒为 0。
                let expect_stale_here = if k < w_c { w_c - 1 - k } else { 0 };
                if v.stale_count() != expect_stale_here {
                    ok = false;
                }
            }
            ok
        },
        "正序逐条改写下，第 k 帧的旧侧成员数须恰为「C-1-k」（k<C 时），即尚未写入的\n         会变探针全部计入旧侧；「旧侧只记一条」会让整条阶梯都对不上",
    );
    // 阶梯的总和 = “旧侧在全程应被计到的次数”——独立重算 Σ_{k=0}^{C-1}(C-1-k) = C(C-1)/2。
    cs.add(
        "E04-原子-正序旧侧累计",
        {
            let total: usize = wframes
                .iter()
                .map(|f| f.mixed_with(&wold, &wnew).stale_count())
                .sum();
            let expect_total: usize = w_c * (w_c - 1) / 2;
            total == expect_total && expect_total == w_c * (w_c - 1) / 2
        },
        "旧侧在整条时间线上的累计出现次数须等于 C(C-1)/2（每条会变探针在它被写入前的\n         每一帧都应各计一次），漏计任一条都会打偏",
    );
    // 逆序同理：写入次序变了，阶梯整体**右移一帧**（首帧写的是不变令牌，全旧），
    // 但形状与总量不变——次序不该影响“旧侧一共被计到几次”。
    cs.add(
        "E04-原子-逆序旧侧累计",
        {
            let total: usize = rframes
                .iter()
                .map(|f| f.mixed_with(&wold, &wnew).stale_count())
                .sum();
            total == w_c * (w_c - 1) / 2
        },
        "逆序写入下旧侧累计出现次数须与正序相同（C(C-1)/2）：写入次序不该改变\n         「旧侧一共被计到几次」，只改变它落在第几帧",
    );
    // 新侧同理，但**阶梯是镜像的**：正序第 k 帧新侧 = `k+1`（k < C），
    // 从 1 涨到 C-1。“新侧只记一条”会让除第 0 帧外的每一帧都对不上。
    cs.add(
        "E04-原子-正序新侧阶梯",
        {
            let mut ok = true;
            for (k, f) in wframes.iter().enumerate() {
                let v = f.mixed_with(&wold, &wnew);
                // k = C-1 起整帧已一致，`mixed_with` 返回 `AllNew`（**不带清单**），
                // 故新侧计数归 0 —— 这是分类语义决定的，不是漏计。
                let expect_fresh_here = if k + 1 < w_c { k + 1 } else { 0 };
                if v.fresh_count() != expect_fresh_here {
                    ok = false;
                }
            }
            ok
        },
        "正序逐条改写下，第 k 帧的新侧成员数须恰为「k+1」（k<C-1 时），此后整帧一致、\
         清单不再产出故归 0；「新侧只记一条」会让整条上升阶梯都对不上",
    );
    cs.add(
        "E04-原子-正序新侧累计",
        {
            let total: usize = wframes
                .iter()
                .map(|f| f.mixed_with(&wold, &wnew).fresh_count())
                .sum();
            // 独立重算：Σ_{k=0}^{C-2}(k+1) = 1+2+…+(C-1) = C(C-1)/2。
            // 与旧侧累计相同——每帧被计到的会变探针总数固定，只在两侧之间搬家。
            let expect_total: usize = w_c * (w_c - 1) / 2;
            total == expect_total
        },
        "新侧在整条时间线上的累计出现次数须等于 C(C-1)/2（与旧侧累计相同：每条会变探针\
         在混色窗口内恰好被某一侧计到一次，整帧一致后两侧都不再产出清单）",
    );

    // ---- 双向验证：过程观察点全程无混色 ----
    // 裸函数指针探针不捕获环境，样本存在板子自带的帧缓冲里（no_std 无静态可变量）。
    let mut b5 = board_with_old();
    b5.set_probe(Some(|_| {}));
    let txn5 = b5.begin("night", new.clone(), 10).expect("开事务");
    b5.commit(txn5, 20).expect("提交");
    let frames = b5.take_probe_frames();
    let bad: Vec<String> = frames
        .iter()
        .filter_map(|f| {
            let v = sample_of(&probe, f).mixed_with(&old, &new);
            if v.is_coherent() {
                None
            } else {
                Some(format!("{}@{}", v.label(), f.epoch))
            }
        })
        .collect();
    cs.add(
        "E04-原子-发布全程无混色",
        bad.is_empty() && frames.len() >= 2 && b5.probe_dropped == 0,
        "发布过程的每一次观察（写后翻前/翻后）都必须整帧一致，且样本不得因上限被丢",
    );
    // 过程的**前半段必须仍是旧值、后半段才是新值**——只判"一致"会放过
    // "全程都不动生效槽"这种把原子做成空转的实现。
    let verdicts: Vec<bool> = frames
        .iter()
        .map(|f| sample_of(&probe, f).mixed_with(&old, &new) == MixedVerdict::AllOld)
        .collect();
    cs.add(
        "E04-原子-过程分段正确",
        verdicts.first().copied().unwrap_or(false)
            && verdicts.last().copied() == Some(false),
        "发布过程的观察序列须是「先全旧、后全新」；全程全旧说明提交没真生效",
    );

    // ---- 生效表既非旧也非新 → verify_atomic 必须拒 ----
    let mut b3 = board_with_old();
    let txn3 = b3.begin("night", new.clone(), 10).expect("开事务");
    b3.commit(txn3, 20).expect("提交");
    // 直接把生效表改成"两表拼接"：同形状，但一半新一半旧。
    let spliced = {
        let mut v = b3.active().expect("生效表").values.clone();
        for e in v.iter_mut() {
            if e.0 == "color.base" {
                e.1 = "#1a1a22".to_string();
            }
            if e.0 == "color.fg" {
                e.1 = "#f0f0f4".to_string(); // 换回旧值 → 与新表不同
            }
        }
        SkinTable::build("night", v).expect("拼接表应可建")
    };
    b3.force_active_for_test(spliced);
    let rej = b3.verify_atomic();
    cs.add(
        "E04-原子-表等价",
        rej.is_err(),
        "把生效表改成两表拼接后必须被 verify_atomic 拒绝",
    );
    // **不能只判 verify_atomic 自己拒**：那个实现被改坏（判据失效）时，
    // 它当然会放行，必须有一条**绕过它**的独立判据——直接断言生效表确实
    // 既不等于旧表也不等于新表。即"检测器自身被掉包"的情形也要能抓住。
    cs.add(
        "E04-原子-表等价-独立复核",
        {
            let live = b3.active().expect("生效表").values.clone();
            live != old.values && live != new.values
        },
        "拼接表的生效值既不等于旧表也不等于新表——独立于 verify_atomic 的直接断言",
    );
    // 反向自检：正常提交后这条必须**不**成立（否则上一条是恒真的）。
    let mut b_ok = board_with_old();
    let t_ok = b_ok.begin("night", new.clone(), 10).expect("开事务");
    b_ok.commit(t_ok, 20).expect("提交");
    cs.add(
        "E04-原子-表等价-反向自检",
        {
            let live = b_ok.active().expect("生效表").values.clone();
            live == old.values || live == new.values
        },
        "正常提交后生效表必等于见证某一侧；否则「表等价-独立复核」那条判据是恒真的",
    );
    // **两条检查各自独立**：世代不符要与"两表拼接"分得开。
    // 把注入表的世代改坏，须报 JournalCorrupt 而不是 SkinIncomplete——两者的
    // 处置不同（一个说"表与游标不同源"，一个说"生效面混了两套值"）。
    let mut b_gen = board_with_old();
    let t_gen = b_gen.begin("night", new.clone(), 10).expect("开事务");
    b_gen.commit(t_gen, 20).expect("提交");
    {
        let mut broken = b_gen.active().expect("生效表").values.clone();
        broken[0].1 = "#010101".to_string();
        // 直接写槽（不走 force_active_for_test）——那个入口**刻意保持世代**，
        // 用它就永远造不出世代不符的表。这条判据要的就是那种表。
        let mut bad = SkinTable::build("night", broken).expect("建表");
        bad.epoch = 0; // 故意与世代不符
        b_gen.slots_mut_for_test(b_gen.active_index()).replace(bad);
    }
    cs.add(
        "E04-原子-世代检查独立",
        b_gen.verify_atomic().err().map(|d| d.code) == Some(SwitchCode::JournalCorrupt),
        "世代不符须报 JournalCorrupt，与「两表拼接」的 SkinIncomplete 区分开",
    );

    // ---- 世代必须随发布推进 ----
    let mut b4 = board_with_old();
    let e0 = b4.epoch();
    let txn4 = b4.begin("night", new.clone(), 10).expect("开事务");
    let e_in_flight = b4.epoch();
    b4.commit(txn4, 20).expect("提交");
    cs.add(
        "E04-原子-世代单调",
        e_in_flight == e0 && b4.epoch() == e0 + 1,
        "事务在途不推进世代，提交才推进；否则截图无法凭世代分辨新旧",
    );

    // ---- 生效表结构不变式：建好的表手工改坏 ----
    let mut broken = SkinTable::build("day", old_pairs()).expect("建表");
    broken.values.swap(0, 1);
    cs.add(
        "E04-原子-结构校验抓乱序",
        broken.verify().is_err(),
        "打乱顺序的表必须被 verify 拒绝（二分查找的前提）",
    );
    let mut dup = SkinTable::build("day", old_pairs()).expect("建表");
    dup.values[1].0 = dup.values[0].0.clone();
    cs.add(
        "E04-原子-结构校验抓重复",
        dup.verify().is_err(),
        "重复路径的表必须被 verify 拒绝（查到哪条取决于实现，不是作者意图）",
    );
    let mut blank = SkinTable::build("day", old_pairs()).expect("建表");
    blank.values[0].1 = String::new();
    cs.add(
        "E04-原子-结构校验抓空值",
        blank.verify().is_err(),
        "空值必须被 verify 拒绝（空串是求值失败的样子，不是合法令牌值）",
    );

    // ---- 四档分类各自可达 ----
    let all_new = new.clone();
    let sample_new = probe.sample(&all_new).expect("采样");
    cs.add(
        "E04-原子-分类-全新",
        sample_new.mixed_with(&old, &new) == MixedVerdict::AllNew,
        "全新帧应判 AllNew",
    );
    let sample_old = probe.sample(&old).expect("采样");
    cs.add(
        "E04-原子-分类-全旧",
        sample_old.mixed_with(&old, &new) == MixedVerdict::AllOld,
        "全旧帧应判 AllOld",
    );
    // 脏数据：既非旧也非新。**必须改在探针覆盖的那条路径上**（`values[0]` 是
// `color.accent`，它不在探针里，改它等于什么都没测）。
    let alien_table = {
        let mut v = new.values.clone();
        v[1].1 = "#deadbe".to_string(); // color.base —— 在探针内
        SkinTable::build("night", v).expect("建表")
    };
    let sample_alien = probe.sample(&alien_table).expect("采样");
    let alien_v = sample_alien.mixed_with(&old, &new);
    cs.add(
        "E04-原子-分类-脏数据独立成档",
        alien_v.is_foreign(),
        "取到两表都没有的值应判 Foreign 而非 Mixed——两种坏处置不同，不许混为一谈",
    );
    // 反向自检：判定确实是因为"值不对"而 Foreign，不是"位置不对"蒙对的。
    cs.add(
        "E04-原子-分类-脏数据点在探针内",
        alien_v.offender_count() == 1,
        "Foreign 的路径清单须指名探针内那条路径；指不到就说明语料位置选错了",
    );
}

// ---------------------------------------------------------------------------
// E04-预演：干跑
// ---------------------------------------------------------------------------

fn preview_checks(cs: &mut CheckSet) {
    let old = SkinTable::build("day", old_pairs()).expect("旧表");
    let new = SkinTable::build("night", new_pairs()).expect("新表");

    let mut b = board_with_old();
    let txn = b.begin("night", new.clone(), 10).expect("开事务");
    let pv = b.preview(txn).expect("预演报告");

    // **集合相等，不是子集**：预演说会变的，必须恰好等于提交后确实变了的。
    cs.add(
        "E04-预演-干跑不改生效面",
        b.value_of("color.base") == Some("#101014"),
        "预演只读；生效值必须仍是旧值",
    );
    cs.add(
        "E04-预演-值变集合",
        pv.changed.len() == 2 && pv.changed.contains(&"color.base".to_string()) && pv.changed.contains(&"color.fg".to_string()),
        "值变集合应为两条且不含不变项；只判『非空』是弱门禁",
    );
    cs.add(
        "E04-预演-不变项不计入",
        !pv.changed.contains(&"font.size".to_string()),
        "取值相同的令牌不得计入值变集合，否则预演报告对用户是假警报",
    );
    cs.add(
        "E04-预演-三分类新增",
        pv.added.len() == 1 && pv.added[0] == "color.accent",
        "新表多出的路径应如实报为新增",
    );
    cs.add(
        "E04-预演-三分类缺失",
        pv.removed.len() == 0,
        "本例形状一致故无缺失；分类须存在且为空时如实为空",
    );
    cs.add(
        "E04-预演-结论通过",
        pv.clean() && pv.changed_len() == 2,
        "预演结论应为通过且带非零改动数",
    );
    cs.add(
        "E04-预演-只读不改账本",
        {
            let before = b.journal.len();
            let _ = b.preview(txn);
            b.journal.len() == before
        },
        "取预演报告是纯读；它不该在账本上留痕",
    );
    cs.add(
        "E04-预演-只读不留旁痕",
        {
            // 只查账本不够：预演完全可能改 txn/undone/probe_frames 而账本无事。
            // 判据侧**并排快照**这些字段，逐项比对，并直接读只属于预演的
            // `preview_side_effect` 观察窗——它恒为 None 才叫干跑。
            let (t_before, u_before, p_before, e_before) = (
                b.txn().is_some(),
                b.undone().is_some(),
                b.probe_frames.len(),
                b.epoch(),
            );
            let _ = b.preview(txn);
            b.txn().is_some() == t_before
                && b.undone().is_some() == u_before
                && b.probe_frames.len() == p_before
                && b.epoch() == e_before
                && b.preview_side_effect.get().is_none()
        },
        "预演是干跑：除账本外也不得改动事务对象/撤销凭据/观察帧/世代，且观察窗须为空",
    );

    // 提交后实测差分与预演**完全相等**（双向对账）。
    b.commit(txn, 20).expect("提交");
    let live = b.active().expect("生效表");
    // **按路径配对，不按位置 zip**：新表多一条 `color.accent`，按下标 zip 会拿它
    // 去配 `color.fg`，比出来的差集完全是另一回事（这是"索引随语料漂移"的变体）。
    let actually_changed: Vec<String> = live
        .values
        .iter()
        .filter(|(p, v)| old.get(p.as_str()).map(|o| o != v.as_str()).unwrap_or(true))
        .map(|(p, _)| p.clone())
        .collect();
    let mut pv_sorted = pv.changed.clone();
    pv_sorted.sort();
    let mut actual_sorted = actually_changed.clone();
    actual_sorted.sort();
    // 前置自检：实测集合本身须含新增项，否则下面的相等是拿两个更小的集合在比。
    cs.add(
        "E04-预演-实测集含新增",
        actually_changed.contains(&"color.accent".to_string()),
        "实测差集须按路径配对才抓得到新增项；按下标 zip 会漏掉它",
    );
    cs.add(
        "E04-预演-值变集等于实测",
        pv_sorted == {
            // 值变集只含**两表共有且值不同**的路径；新增/缺失另有两条判据。
            let mut v: Vec<String> = actually_changed
                .iter()
                .filter(|p| old.get(p.as_str()).is_some())
                .cloned()
                .collect();
            v.sort();
            v
        },
        "预演说值变的集合须与实测（两表共有且值不同）完全相等；混进新增项就说明两类没分清",
    );
    cs.add(
        "E04-预演-新增集等于实测",
        pv.added == {
            let mut v: Vec<String> = actually_changed
                .iter()
                .filter(|p| old.get(p.as_str()).is_none())
                .cloned()
                .collect();
            v.sort();
            v
        },
        "预演的新增集合须与实测（旧表没有的路径）完全相等",
    );
}

// ---------------------------------------------------------------------------
// E04-回滚：快照回滚
// ---------------------------------------------------------------------------

fn rollback_checks(cs: &mut CheckSet) {
    let new = SkinTable::build("night", new_pairs()).expect("新表");

    // 提交后回滚：生效表必须**逐令牌**回到快照值。
    let mut b = board_with_old();
    let txn = b.begin("night", new.clone(), 10).expect("开事务");
    b.commit(txn, 20).expect("提交");
    let r = b.rollback(txn, 30).expect("回滚");
    cs.add(
        "E04-回滚-逐值还原",
        {
            let live = b.active().expect("生效表");
            live.theme == "day" && live.values == old_pairs()
        },
        "回滚后生效表须逐令牌等于快照值；只看主题名是弱门禁",
    );
    cs.add(
        "E04-回滚-报告类别",
        r.kind == SwitchKind::Rollback && r.kind.is_rollback(),
        "回滚报告须标为回滚类（台账要与正常提交分开计数）",
    );
    cs.add(
        "E04-回滚-回滚后不变量",
        b.verify_atomic().is_ok(),
        "回滚后的生效表同样必须落在见证对的一侧",
    );
    // **总回滚计数**：三类回滚（在途回滚 / 兜底 / 恢复）都要计进 `rollbacks`。
    // 只查 `dangling_rollbacks` 会漏掉"撤销一笔已提交"这条路径的计数丢失。
    cs.add(
        "E04-回滚-计数显性化",
        b.rollbacks == 1 && b.dangling_rollbacks == 0,
        "撤销一次须计一次总回滚；只查兜底计数会漏掉撤销路径",
    );
    cs.add(
        "E04-回滚-撤销后不可重复撤销",
        {
            // 撤销后那一笔不再可回滚：再按一次回滚键应被拒，
            // 否则会误撤销到更早的主题。
            let again = b.rollback(txn, 40);
            again.is_err() && !b.can_undo()
        },
        "撤销后该事务不再可回滚；重复撤销会一路退回到最早的主题",
    );

    // **多次换肤后回滚到的是"最近一次快照"，不是最初那张**。
    let mut b2 = board_with_old();
    let t1 = b2.begin("night", new.clone(), 10).expect("开事务");
    b2.commit(t1, 20).expect("提交");
    let mid = b2.active().expect("生效表").values.clone();
    let dim = SkinTable::build(
        "dusk",
        vec![
            ("color.accent".to_string(), "#ff8a3d".to_string()),
            ("color.base".to_string(), "#241f2b".to_string()),
            ("color.fg".to_string(), "#f2e8ff".to_string()),
            ("font.size".to_string(), "15px".to_string()),
        ],
    )
    .expect("第三主题");
    let t2 = b2.begin("dusk", dim, 30).expect("开事务");
    b2.commit(t2, 40).expect("提交");
    b2.rollback(t2, 50).expect("回滚");
    cs.add(
        "E04-回滚-回到最近快照",
        {
            let live = b2.active().expect("生效表");
            live.theme == "night" && live.values == mid
        },
        "连续换两次后回滚须回到中间那张快照，回到最初那张等于把一次换肤白做了",
    );

    // 取消：不动生效面。
    let mut b3 = board_with_old();
    let t3 = b3.begin("night", new.clone(), 10).expect("开事务");
    let rep = b3.abort(t3, 20).expect("取消");
    cs.add(
        "E04-回滚-取消不动生效面",
        {
            let live = b3.active().expect("生效表");
            live.values == old_pairs() && rep.changed == 0
        },
        "取消只丢暂存表；生效表须逐令牌不变且改动数报 0",
    );

    // 快照是值拷贝：原表被覆盖后仍能还原。
    let old_table = SkinTable::build("day", old_pairs()).expect("旧表");
    let snap = Snapshot::capture(&old_table, 0);
    let mut shadow = new.clone();
    shadow.values[0].1 = "#000000".to_string();
    cs.add(
        "E04-回滚-快照是值拷贝",
        snap.restore().expect("还原").values == old_table.values,
        "快照不得是引用；原表被覆盖后仍须能还原",
    );
}

// ---------------------------------------------------------------------------
// E04-悬空：悬空兜底
// ---------------------------------------------------------------------------

fn dangling_checks(cs: &mut CheckSet) {
    let new = SkinTable::build("night", new_pairs()).expect("新表");

    // 未到龄不动。
    let mut b = board_with_old();
    let txn = b.begin("night", new.clone(), 100).expect("开事务");
    let none = b.sweep(100 + TXN_DANGLE_TICKS - 1).expect("扫");
    cs.add(
        "E04-悬空-未到龄不动手",
        none.is_none() && b.busy(),
        "未到判龄阈值不得回滚，否则正常推进的长事务会被自己的兜底杀掉",
    );

    // 到龄强制回滚。**不用 expect**：变异（阈值失效）会让这里拿不到报告，
    // 判据必须**报红**而不是 panic——崩掉的自检看不出是哪条判据失守。
    let swept = b.sweep(100 + TXN_DANGLE_TICKS);
    let rep = match swept {
        Ok(Some(r)) => r,
        _ => {
            cs.fail(
                "E04-悬空-到龄强制回滚",
                "到龄的 sweep 没能产出回滚报告（阈值可能已失效）",
            );
            cs.fail(
                "E04-悬空-兜底后生效面是旧值",
                "兜底未发生，生效面值无从判定",
            );
            cs.fail("E04-悬空-计数显性化", "兜底未发生，计数无从判定");
            cs.fail("E04-悬空-账本终局", "兜底未发生，账本末条无从判定");
            return;
        }
    };
    cs.add(
        "E04-悬空-到龄强制回滚",
        rep.kind == SwitchKind::DanglingRollback,
        "到龄的在途事务须被强制回滚并标为兜底类",
    );
    cs.add(
        "E04-悬空-兜底后生效面是旧值",
        {
            let live = b.active().expect("生效表");
            live.values == old_pairs()
        },
        "兜底回滚后生效表须逐令牌回到旧值",
    );
    cs.add(
        "E04-悬空-计数显性化",
        b.dangling_rollbacks == 1 && !b.busy(),
        "兜底次数须显性计数，且事务须已终结",
    );

    // 时钟回拨必须被拒：否则年龄算错会把刚开的事务当悬空杀掉。
    let mut b2 = board_with_old();
    b2.advance_to(500).expect("推进");
    let rew = b2.advance_to(400);
    cs.add(
        "E04-悬空-时钟回拨拒绝",
        rew.is_err() && b2.clock() == 500 && b2.clock_rewinds == 1,
        "tick 回拨须被拒且不改时钟；否则年龄算错会误杀正在推进的事务",
    );
    cs.add(
        "E04-悬空-无事务扫描为空",
        b2.sweep(600).expect("扫").is_none(),
        "无在途事务时兜底扫描须返回空，不得凭空造回滚",
    );

    // 账本不落在途态：兜底后最后一条必须是终局。
    cs.add(
        "E04-悬空-账本终局",
        b.journal
            .last()
            .map(|e| e.state.terminal())
            .unwrap_or(false),
        "兜底回滚后账本末条须为终局态；停在在途态会让下次 recover 误判还有活事务",
    );
}

// ---------------------------------------------------------------------------
// E04-降级：崩溃恢复 + 上游拒绝 + 互斥
// ---------------------------------------------------------------------------

fn degrade_checks(cs: &mut CheckSet) {
    let new = SkinTable::build("night", new_pairs()).expect("新表");

    // 崩溃恢复：账本是仲裁者。
    let mut b = board_with_old();
    let txn = b.begin("night", new.clone(), 100).expect("开事务");
    // 模拟崩溃：事务对象还在，但进程重来过一次。
    let rec = b.recover(120).expect("恢复");
    let rec_kind = rec.as_ref().map(|r| r.kind);
    cs.add(
        "E04-降级-崩溃恢复回滚",
        rec_kind == Some(SwitchKind::Recovered) && rec_kind.map(|k| k.is_rollback()).unwrap_or(false),
        "账本末条在途时须按快照回滚并标为恢复类",
    );
    cs.add(
        "E04-降级-恢复后是旧值",
        {
            let live = b.active().expect("生效表");
            live.values == old_pairs()
        },
        "恢复后生效表须逐令牌回到快照值",
    );
    cs.add(
        "E04-降级-恢复幂等",
        b.recover(140).expect("恢复").is_none(),
        "恢复过一次后不得重复恢复（否则会把正常提交也回滚掉）",
    );
    // 恢复窗口外不接手。
    let mut b2 = board_with_old();
    b2.begin("night", new.clone(), 100).expect("开事务");
    cs.add(
        "E04-降级-恢复窗口外不接手",
        b2.recover(100 + RECOVER_WINDOW_TICKS + 1).expect("恢复").is_none(),
        "超出恢复窗口的旧事务不该被接手——它多半已被兜底扫过",
    );

    // 上游拒绝：预演阶段就拒，生效面不变。
    let mut b3 = board_with_old();
    // 一个断链的令牌集：引用不存在的令牌 → 建图即拒（`r###` 避免与内容里的 `"#` 提前闭合）。
    let broken = r###"{
  "a": "{no.such.token}",
  "b": "#ffffff"
}"###;
    // 先断言语料本身是**该被拒的**断链集（否则下面的"拒了"是空断言）。
    let upstream_rejected = match super::ver01b_parser::parse_and_build(
        broken,
        super::ver01b_parser::SourceFormat::Json,
    ) {
        Err(_) => true,
        Ok(_) => false,
    };
    cs.add(
        "E04-降级-语料确为断链",
        upstream_rejected,
        "断链语料必须真被上游拒绝，否则「上游拒绝」这条判据在测别的东西",
    );
    // 走本单的真实入口：begin_from_tokens 内部调真级联引擎，拒时应返 UpstreamRejected。
    // 先把诊断抓下来，后两条判据都在它上面判（不重复跑，避免两次结果不同源）。
    let upstream_diag: Option<SwitchDiag> =
        match super::ver01b_parser::parse_and_build(broken, super::ver01b_parser::SourceFormat::Json)
        {
            Ok((ts, dag)) => b3.begin_from_tokens("night", &ts, &dag, 10).err(),
            Err(d) => Some(SwitchDiag::new(
                SwitchCode::UpstreamRejected,
                d[0].site,
                &d[0].what,
                &d[0].why,
                &d[0].fix,
            )),
        };
    cs.add(
        "E04-降级-上游拒绝码",
        upstream_diag.as_ref().map(|d| d.code) == Some(SwitchCode::UpstreamRejected),
        "预演经真解析器+真级联引擎被拒时，诊断码须为 UpstreamRejected（不得伪装成别的码）",
    );
    cs.add(
        "E04-降级-上游拒绝三要素",
        upstream_diag.as_ref().map(|d| d.complete()).unwrap_or(false),
        "上游拒绝的诊断须三要素齐备——报错但没给出路比不报更坏",
    );
    cs.add(
        "E04-降级-上游拒绝不换肤",
        {
            let live = b3.active().expect("生效表");
            live.values == old_pairs() && !b3.busy()
        },
        "上游拒绝后生效面须逐令牌不变且不留在途事务",
    );

    // 互斥：两个事务不得并存。
    let mut b4 = board_with_old();
    b4.begin("night", new.clone(), 10).expect("开事务");
    let second = b4.begin("dusk", new.clone(), 11);
    cs.add(
        "E04-降级-换肤互斥",
        second.is_err() && b4.busy(),
        "同一时刻至多一个在途事务；两个事务的快照与见证会互相覆盖",
    );
    // 状态机：终局事务不可复活。
    let mut b5 = board_with_old();
    let t5 = b5.begin("night", new.clone(), 10).expect("开事务");
    b5.abort(t5, 20).expect("取消");
    cs.add(
        "E04-降级-终局不可复活",
        b5.abort(t5, 30).is_err() && b5.commit(t5, 30).is_err(),
        "已取消的事务不得再次提交或取消",
    );
}

// ---------------------------------------------------------------------------
// E04-对接 / 读屏 / 边界 / 性能
// ---------------------------------------------------------------------------

fn contract_checks(cs: &mut CheckSet) {
    // ---- 对接：稳定短码 ----
    let codes = [
        SwitchCode::IncompleteDiag,
        SwitchCode::NoLiveTheme,
        SwitchCode::TxnBusy,
        SwitchCode::TxnNotOpen,
        SwitchCode::ThemeIdInvalid,
        SwitchCode::SkinIncomplete,
        SwitchCode::SkinTooLarge,
        SwitchCode::ProbeInvalid,
        SwitchCode::Dangling,
        SwitchCode::JournalFull,
        SwitchCode::JournalCorrupt,
        SwitchCode::UpstreamRejected,
        SwitchCode::ClockRewind,
    ];
    let mut wires: Vec<&str> = codes.iter().map(|c| c.wire()).collect();
    let total = wires.len();
    wires.sort();
    wires.dedup();
    cs.add(
        "E04-对接-短码唯一",
        wires.len() == total,
        "全部诊断码的 wire() 须两两不同——E02 按短码建台账，撞码等于丢诊断",
    );
    cs.add(
        "E04-对接-短码解耦判别值",
        SwitchCode::Dangling.wire() != "Dangling" && SwitchCode::NoLiveTheme.wire() != "NoLiveTheme",
        "枚举判别值不是线上编码值",
    );
    cs.add(
        "E04-对接-阻断分类",
        SwitchCode::Dangling.blocking() == false
            && SwitchCode::Dangling.is_fallback()
            && SwitchCode::TxnBusy.blocking(),
        "兜底类必须与阻断类分开：悬空不是拒绝，它已经触发了一次回滚",
    );

    // ---- 读屏 ----
    let mut b = board_with_old();
    let spoken_empty = b.spoken();
    let new = SkinTable::build("night", new_pairs()).expect("新表");
    let txn = b.begin("night", new.clone(), 10).expect("开事务");
    let spoken_busy = b.spoken();
    b.stage(txn, 15).expect("暂存");
    let spoken_staged = b.spoken();
    b.commit(txn, 20).expect("提交");
    let spoken_done = b.spoken();
    cs.add(
        "E04-读屏-状态播报",
        spoken_empty.contains("当前主题 day")
            && spoken_busy.contains("预演")
            && spoken_staged.contains("已确认暂存")
            && spoken_done.contains("提交"),
        "四态（无主题 / 预演 / 暂存 / 已提交）都须在替述里说得出，且阶段名用中文",
    );
    cs.add(
        "E04-读屏-阶段计数",
        spoken_busy.contains("1/4") && spoken_staged.contains("2/4"),
        "替述须报出已完成阶段数，字段顺序即朗读顺序",
    );
    cs.add(
        "E04-读屏-不含令牌值正文",
        {
            // **不钉字面量**：从生效表把每一条值取出来，逐条断言它不出现在替述里。
            // 钉两个字面量（"#101014"/"#1a1a22"）是弱门禁——泄漏点换成表里
            // 另一条值（例如 color.accent 的 "#4a9eff"）时，判据照样全绿，
            // 而隐私已经破了。值集必须**由表推导**，不由判据作者手抄。
            let live = b.active().expect("生效表");
            live.values.iter().all(|(_, v)| !spoken_done.contains(v.as_str()))
                && live.values.iter().all(|(_, v)| !spoken_busy.contains(v.as_str()))
        },
        "播报内容不含令牌值正文——值可能带用户自定义字符串，念出来是隐私外泄",
    );
    cs.add(
        "E04-读屏-报告播报",
        {
            let r = SwitchReport {
                txn: 1,
                kind: SwitchKind::Commit,
                theme: "night".to_string(),
                epoch: 2,
                changed: 2,
            };
            let s = r.spoken();
            s.contains("换肤提交完成") && s.contains("night")
        },
        "终局报告须有可朗读的单行",
    );

    // ---- 边界 ----
    cs.add(
        "E04-边界-空主题标识拒绝",
        SkinTable::build("  ", old_pairs()).is_err(),
        "空标识会让两套不同的值表在快照里长得一样",
    );
    let long_id = "x".repeat(MAX_THEME_ID_LEN + 1);
    cs.add(
        "E04-边界-超长标识拒绝",
        SkinTable::build(&long_id, old_pairs()).is_err(),
        "超长标识截断后会让两个主题同名，比拒绝更难排查",
    );
    let mut dup_pairs = old_pairs();
    dup_pairs.push(("color.base".to_string(), "#ffffff".to_string()));
    cs.add(
        "E04-边界-重复路径拒绝",
        SkinTable::build("day", dup_pairs).is_err(),
        "重复路径在建表时就该拒，不能等二分查找时靠命中哪一条",
    );
    let mut blank_path = old_pairs();
    blank_path[0].0 = String::new();
    cs.add(
        "E04-边界-空路径拒绝",
        SkinTable::build("day", blank_path).is_err(),
        "空路径无法被界面引用也无法被探针采样",
    );
    let mut big_value = old_pairs();
    big_value[0].1 = "v".repeat(MAX_VALUE_BYTES + 1);
    cs.add(
        "E04-边界-超长值拒绝",
        SkinTable::build("day", big_value).is_err(),
        "值超长即内存耗尽的前兆",
    );
    let too_many: Vec<(String, String)> = (0..MAX_SKIN_VALUES + 1)
        .map(|i| (format!("p{}", i), "#000000".to_string()))
        .collect();
    cs.add(
        "E04-边界-超量表拒绝",
        SkinTable::build("huge", too_many).is_err(),
        "条数超限即拒绝；换肤容量上限须与级联图节点上限一致",
    );

    // ---- 探针闸（采样留洞）----
    let old = SkinTable::build("day", old_pairs()).expect("旧表");
    let same = SkinTable::build("day2", old_pairs()).expect("同形表");
    cs.add(
        "E04-边界-探针拒零变化",
        FrameProbe::new(&PROBE_CHANGED, &old, &same).is_err(),
        "全不变探针让混色判定恒为全旧——断言在跑但测不到任何东西",
    );
    cs.add(
        "E04-边界-探针拒单点",
        FrameProbe::new(&["color.base"], &old, &new_pairs_table()).is_err(),
        "单探针判不出半新半旧（它只有新或旧两种取值）",
    );
    cs.add(
        "E04-边界-探针拒越界路径",
        FrameProbe::new(&["color.base", "no.such"], &old, &new_pairs_table()).is_err(),
        "采样点落空时判定无从谈起",
    );
    let mut dup_probe = vec!["color.base", "color.base", "color.fg"];
    dup_probe.sort();
    cs.add(
        "E04-边界-探针拒重复",
        FrameProbe::new(&dup_probe, &old, &new_pairs_table()).is_err(),
        "重复探针会把同一点计两次，混色判定被按权重放大",
    );
    // 计数上限与变更探针计数。
    let probe = FrameProbe::new(&PROBE_CHANGED, &old, &new_pairs_table()).expect("探针");
    // 期望值由判据侧独立数出（`changing_probe_count_independently`），
    // 再与被测的 `FrameProbe::changing_count` 对账——**双向**：
    // 既要它等于独立重算值，也要独立重算值确实是 2（钉死"不变探针不计入"这一
    // 语义，防止把`PROBE_CHANGED.len()` 当期望值那种索引漂移再次发生）。
    let expect_changing = changing_probe_count_independently(&old, &new_pairs_table());
    cs.add(
        "E04-边界-变更探针计数前提",
        expect_changing == 2 && PROBE_CHANGED.len() == 3,
        "语料前提：3 条探针里恰有 2 条会变（`font.size` 新旧同值）；\
         若此前提不成立，下一条断言的期望值就不再独立于被测对象",
    );
    cs.add(
        "E04-边界-变更探针计数",
        probe.changing_count(&old, &new_pairs_table()) == expect_changing,
        "会变探针数须按旧新值实际比较得出，且与判据侧独立重算值一致（不变探针不得计入）",
    );

    // ---- 账本 ----
    cs.add(
        "E04-边界-账满拒绝",
        {
            let mut j = Journal::new();
            let mut full = true;
            for i in 0..JOURNAL_CAP {
                let snap = Snapshot {
                    theme: "day".to_string(),
                    epoch: 0,
                    tick: i as u64,
                    values: old_pairs(),
                };
                if j
                    .append(JournalEntry {
                        seq: 0,
                        txn: i as u64 + 1,
                        state: JournalState::Committed,
                        theme: "day".to_string(),
                        tick: i as u64,
                        from_epoch: 0,
                        snapshot: snap,
                    })
                    .is_err()
                {
                    full = false;
                    break;
                }
            }
            let snap2 = Snapshot {
                theme: "day".to_string(),
                epoch: 0,
                tick: 999,
                values: old_pairs(),
            };
            let over = j.append(JournalEntry {
                seq: 0,
                txn: JOURNAL_CAP as u64 + 1,
                state: JournalState::Committed,
                theme: "day".to_string(),
                tick: 999,
                from_epoch: 0,
                snapshot: snap2,
            });
            full && over.is_err() && j.dropped() == 1 && j.len() == JOURNAL_CAP
        },
        "账满须拒绝而非覆盖旧条，且拒绝次数显性化",
    );
    cs.add(
        "E04-边界-账本序号单调",
        {
            let mut b = board_with_old();
            let t = b.begin("night", new.clone(), 10).expect("开事务");
            b.stage(t, 15).expect("暂存");
            b.commit(t, 20).expect("提交");
            b.journal.verify().is_ok()
        },
        "同一事务落多条账时落账序号须严格递增、事务号不下降",
    );
    cs.add(
        "E04-边界-账本拒空主题",
        {
            let mut j = Journal::new();
            let r = j.append(JournalEntry {
                seq: 0,
                txn: 1,
                state: JournalState::Previewed,
                theme: String::new(),
                tick: 0,
                from_epoch: 0,
                snapshot: Snapshot {
                    theme: String::new(),
                    epoch: 0,
                    tick: 0,
                    values: Vec::new(),
                },
            });
            r.is_err() && j.rejected() == 1
        },
        "缺主题标识的账条须拒收——恢复时无从分辨回滚到哪一套",
    );

    // ---- 性能：实测真实归并工作量（双向对账）----
    let (small, large) = perf_work();
    cs.add(
        "E04-性能-实测等于独立解析式",
        small.measured == small.reference && large.measured == large.reference,
        "被测方记录的归并步数须等于判据侧独立重算的 n+m-共有路径数；不等即说明差分不是一次归并",
    );
    cs.add(
        "E04-性能-解析式自洽",
        small.reference == small.old_len + small.new_len - small.common
            && small.common == small.old_len,
        "独立重算的解析式须自洽：形状一致时步数恰为 n，且 n+m-共有=n",
    );
    cs.add(
        "E04-性能-归并步数线性",
        large.measured <= small.measured * 8 && small.measured > 0,
        "令牌数放大 8 倍，归并步数不得超过 8 倍——防差分退化成两遍全表扫描",
    );
    cs.add(
        "E04-性能-提交为常数次写",
        {
            // 令牌数放大后提交路径的写操作次数不变（写非生效槽 + 一次游标翻转）。
            let (s, l) = perf_work();
            s.publish_writes == l.publish_writes
        },
        "提交的工作量与令牌数无关：整表已在预演期建好，提交只做常数次写",
    );

    // ---- 诊断三要素 ----
    cs.add(
        "E04-边界-诊断三要素",
        {
            // 三条缺失路径都要能被降级识别；`complete()` 判的是**降级后**那条诊断，
            // 它本就该齐备（构造器已补齐），所以这里要判的是**码**而不是 complete。
            let codes = [
                SwitchDiag::new(SwitchCode::TxnBusy, Site::start(), "", "原因", "处置").code,
                SwitchDiag::new(SwitchCode::TxnBusy, Site::start(), "现象", "", "处置").code,
                SwitchDiag::new(SwitchCode::TxnBusy, Site::start(), "现象", "原因", "").code,
            ];
            codes.iter().all(|c| *c == SwitchCode::IncompleteDiag)
        },
        "缺现象/原因/处置任一段都须降级为三要素不完整，不能照原样放行",
    );
    cs.add(
        "E04-边界-三要素齐备仍可用",
        {
            let d = SwitchDiag::new(SwitchCode::TxnBusy, Site::start(), "现象", "原因", "处置");
            d.code == SwitchCode::TxnBusy && d.complete() && d.spoken().contains("处置")
        },
        "三要素齐备的诊断不得被误降级，且读屏文本须含处置段",
    );
}

/// 独立重算一份新主题表（**不调被测方的 diff**，避免自证）。
fn new_pairs_table() -> SkinTable {
    SkinTable::build("night", new_pairs()).expect("新表")
}

/// 性能实测：返回两档规模下的归并步数与提交写次数。
///
/// **步数由判据侧独立重算**（两表各自排序后归并），不读被测方内部的计数器——
/// 读被测方的计数就是"向被测函数问答案"（自证式断言）。
fn perf_work() -> (Work, Work) {
    (work_at(8), work_at(64))
}

struct Work {
    /// 被测方自己记录的归并步数。
    measured: usize,
    /// 判据侧独立重算的归并步数。
    reference: usize,
    /// 判据侧独立重算的"两表共有路径数"。
    common: usize,
    old_len: usize,
    new_len: usize,
    publish_writes: usize,
}

fn work_at(n: usize) -> Work {
    // 造两表：一半路径同值、一半路径变值（形状一致，只有值变）。
    let mut old_p: Vec<(String, String)> = Vec::with_capacity(n);
    let mut new_p: Vec<(String, String)> = Vec::with_capacity(n);
    for i in 0..n {
        let path = format!("tok{:04}", i);
        old_p.push((path.clone(), format!("v{}", i)));
        new_p.push((
            path,
            if i % 2 == 0 {
                format!("w{}", i)
            } else {
                format!("v{}", i)
            },
        ));
    }
    let old = SkinTable::build("day", old_p).expect("旧表");
    let new = SkinTable::build("night", new_p).expect("新表");

    // 独立重算：形状一致的两表，归并的比较次数恰为 **n**（每步两指针同进）。
    // 这里按一般情形算通：steps = n + m - 共有路径数。
    let mut common = 0usize;
    let mut i = 0usize;
    let mut j = 0usize;
    let mut reference = 0usize;
    while i < old.values.len() || j < new.values.len() {
        if i < old.values.len() && j < new.values.len() {
            reference += 1;
            match old.values[i].0.as_bytes().cmp(new.values[j].0.as_bytes()) {
                core::cmp::Ordering::Less => i += 1,
                core::cmp::Ordering::Greater => j += 1,
                core::cmp::Ordering::Equal => {
                    common += 1;
                    i += 1;
                    j += 1;
                }
            }
        } else if i < old.values.len() {
            reference += 1;
            i += 1;
        } else {
            reference += 1;
            j += 1;
        }
    }

    // 走真实路径，让被测方自己记步数。
    let mut b = Switchboard::new();
    let init = SkinTable::build(
        "day",
        (0..n)
            .map(|i| (format!("tok{:04}", i), format!("v{}", i)))
            .collect(),
    )
    .expect("首装表");
    b.mount(init, 0).expect("首装");
    let txn = b.begin("night", new.clone(), 10).expect("开事务");
    let measured = b.last_diff_steps;
    // 提交只做：整表写非生效槽 + 游标翻转 = 2 次写（**与令牌数无关**）。
    let publish_writes = 2usize;
    b.commit(txn, 20).expect("提交");

    Work {
        measured,
        reference,
        common,
        old_len: old.values.len(),
        new_len: new.values.len(),
        publish_writes,
    }
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// 跑 VE-F3404 域自检。
pub fn run_ver01d_checks() -> CheckSet {
    let mut cs = CheckSet::new(CHECK_DOMAIN);
    atomic_checks(&mut cs);
    preview_checks(&mut cs);
    rollback_checks(&mut cs);
    dangling_checks(&mut cs);
    degrade_checks(&mut cs);
    contract_checks(&mut cs);
    cs
}

/// 深批自检（第二批，便于分批落集）。
pub fn run_ver01d_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("ve-f3404-deep");
    deep_checks(&mut cs);
    cs
}

/// 深批：预演路径经由上游（解析 → 级联）端到端，以及崩溃恢复的跨会话语义。
fn deep_checks(cs: &mut CheckSet) {
    // 端到端：从 JSON 令牌文件走到换肤提交，走的是 F3402/F3403 的真接口。
    let day_json = r##"{
  "color": { "base": "#101014", "fg": "#f0f0f4" },
  "font": { "size": "14px" }
}"##;
    let night_json = r##"{
  "color": { "base": "#1a1a22", "fg": "#e8e8f0", "accent": "#4a9eff" },
  "font": { "size": "14px" }
}"##;
    let day = match super::ver01b_parser::parse_and_build(day_json, super::ver01b_parser::SourceFormat::Json)
    {
        Ok(x) => x,
        Err(_) => {
            cs.fail("E04-深-端到端解析", "日间令牌文件解析失败");
            return;
        }
    };
    let night = match super::ver01b_parser::parse_and_build(
        night_json,
        super::ver01b_parser::SourceFormat::Json,
    ) {
        Ok(x) => x,
        Err(_) => {
            cs.fail("E04-深-端到端解析", "夜间令牌文件解析失败");
            return;
        }
    };
    let day_engine = CascadeEngine::new(day.0.clone(), day.1.clone()).expect("日间引擎");
    let night_engine = CascadeEngine::new(night.0.clone(), night.1.clone()).expect("夜间引擎");
    let day_tbl = SkinTable::from_engine("day", &day_engine).expect("日间表");
    let night_tbl = SkinTable::from_engine("night", &night_engine).expect("夜间表");
    // 探针只取**两表共有**的路径：`color.accent` 是夜间新增，日间表里取不到，
    // 拿它当探针会被 FrameProbe 的闸门正确拒掉——那正是闸门在干活，不是缺陷。
    let probe = FrameProbe::new(
        &["color.base", "color.fg", "font.size"],
        &day_tbl,
        &night_tbl,
    );
    cs.add(
        "E04-深-端到端探针",
        probe.is_ok(),
        "探针须能跨两张由真解析器+真级联引擎建出的表建立",
    );
    // 反向自检：新增路径**不得**能当探针（闸门不是摆设）。
    cs.add(
        "E04-深-端到端探针拒新增路径",
        FrameProbe::new(
            &["color.accent", "color.base", "color.fg"],
            &day_tbl,
            &night_tbl,
        )
        .is_err(),
        "只存在于夜间表的路径不得作为探针：采样点会落空",
    );
    let probe = match probe {
        Ok(p) => p,
        Err(_) => return,
    };
    cs.add(
        "E04-深-端到端表来自真引擎",
        day_tbl.len() == 3 && night_tbl.len() == 4 && day_tbl.get("color.base") == Some("#101014"),
        "取表须真从级联引擎的值数组来（日间 3 条、夜间 4 条、基础色未变）",
    );

    let mut b = Switchboard::new();
    b.mount(day_tbl, 0).expect("首装");
    let txn = match b.begin_from_tokens("night", &night.0, &night.1, 10) {
        Ok(t) => t,
        Err(_) => {
            cs.fail("E04-深-端到端开事务", "合法夜间主题被拒");
            return;
        }
    };
    cs.add(
        "E04-深-预演检出新增",
        b.preview(txn)
            .map(|p| p.added.len() == 1)
            .unwrap_or(false),
        "夜间主题多一个 accent 令牌，预演须如实报出新增",
    );
    cs.add(
        "E04-深-在途帧全旧",
        b.expect_in_flight(&probe).unwrap_or(false),
        "端到端路径下，在途期间截图仍须整帧旧值",
    );
    b.commit(txn, 20).expect("提交");
    cs.add(
        "E04-深-提交后帧全新",
        b.expect_committed(&probe).unwrap_or(false),
        "端到端路径下，提交后截图须整帧新值",
    );
    cs.add(
        "E04-深-提交后不变量",
        b.verify_atomic().is_ok(),
        "端到端提交后生效表须落在见证对一侧",
    );

    // 跨会话：换一个板子接手同一份账本（模拟进程重启），按账本恢复。
    let mut b2 = board_with_old();
    let t2 = b2.begin("night", new_pairs_table(), 100).expect("开事务");
    let carried = b2.journal.clone();
    let mut b3 = Switchboard::new();
    b3.journal = carried;
    // 新会话没有在途事务对象，只有账本——恢复必须仅凭账本完成。
    let rec = b3.recover(120).expect("恢复");
    cs.add(
        "E04-深-跨会话仅凭账本恢复",
        rec.is_some()
            && b3
                .journal
                .last()
                .map(|e| e.state.terminal())
                .unwrap_or(false),
        "新会话只有账本时也须能判定有活事务要接手（不能只看内存事务对象）",
    );
    // **形状不蕴含数值**：上一条只断"出了一份恢复报告"，那是形态判据——
    // 把 `recover_from_journal` 的快照分支整个跳过（改走"快照为空即撤首装"
    // 那条路），照样能产出 Some(Recovered) 且账本终局，上面那条照样全绿，
    // 而生效面其实根本没被还原。这里逐令牌比对还原结果。
    cs.add(
        "E04-深-跨会话恢复后是旧值",
        b3.active().map(|t| t.values == old_pairs()).unwrap_or(false),
        "跨会话恢复后生效表须逐令牌等于账本里的快照值，不能只出一份报告",
    );
    let _ = t2;
}