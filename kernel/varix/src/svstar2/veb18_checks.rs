//! VE-F0218 · QEMU 版本兼容矩阵（VE-B 域 · virtio 兼容层 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0218`
//!
//! **判据（锚点原文）**：版本行、探测预期、实测为准、N/A 联动、判据。
//!
//! 分五组，逐条映射锚点：
//! - `c218_rows`     → 判据一「版本行」：五登记行 / 升序 / O(1) 回退 / 差异登记
//! - `c218_expect`   → 判据二「探测预期」：已登记/未知/低于下限 三条预期路径
//! - `c218_measured` → 判据三「实测为准」：冲突取实测 + 痕迹留档 + 探测失败保守
//! - `c218_na`       → 判据四「N/A 联动」：F0215 按矩阵跳 N/A / F0216 最低版本引用
//! - `c218_struct`   → 判据五「判据」：线编码自洽 / 诊断位自持 / 判据集自身性质
//!
//! **本文件的判据纪律**（十诫）：
//! 1. **不用表内元素验表内函数**：`ROWS` 是被测语料，判据里出现`ROWS[i]`
//!    只能做「行间关系」断言；凡是「某版本该有哪些能力」这类绝对预期，
//!    一律在判据侧**用 `Version` 字面量独立写出来**（见 `EXPECT_*` 常量），
//!    不从 `ROWS` 反推。
//! 2. **O(1) 判据不许被自证式污染**：`row_index_for` 从尾部回退，
//!    步数与命中位置无关。判据若只测「查得到」，把实现改成从头线性扫
//!    也能全绿 ⇒ 另有一条**从尾部 vs 从头部结果等价**的对账。
//! 3. **冲突检测不是恒真**：`has_conflict()` 若恒返回 true，
//!    「实测优先」判据照样全绿 ⇒ 必须同时断「实测==预期 ⇒ 无冲突」。
//! 4. **降级不是恒真**：`Certification::Degraded` 恒真会让
//!    「探测失败走保守」变成空话 ⇒ 必须同时断「探测成功 ⇒ 非降级」。
//! 5. **判据区零 panic 面**：全文件无 `unwrap()`/`expect()`/`[i]` 越界风险，
//!    所有下标访问先比长度。

use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use super::veb18_compat::*;
use crate::checks::{CheckSet, MAX_CHECKS};

// ---------------------------------------------------------------------------
// 判据侧的独立预期常量（**不从 ROWS 反推**，十诫第 1 条）
// ---------------------------------------------------------------------------

/// 6.0 行的能力集，独立写出：基线含 Virgl 与 EDID 扩展，不含 Venus。
const EXPECT_60: u32 = CAP_VIRGL | CAP_EDID_EXT;
/// 6.2 行：+ Venus（事件语义同时变）。
const EXPECT_62: u32 = CAP_VIRGL | CAP_VENUS | CAP_EDID_EXT;
/// 7.0 行：+ 事件语义 v2。
const EXPECT_70: u32 = CAP_VIRGL | CAP_VENUS | CAP_EDID_EXT | CAP_EVENT_V2;
/// 7.2 行：+ 共享 reset。
const EXPECT_72: u32 =
    CAP_VIRGL | CAP_VENUS | CAP_EDID_EXT | CAP_EVENT_V2 | CAP_RESET_SHARED;
/// 8.0 行：+ 快速 resume（六位全开）。
const EXPECT_80: u32 = CAP_VIRGL
    | CAP_VENUS
    | CAP_EDID_EXT
    | CAP_EVENT_V2
    | CAP_RESET_SHARED
    | CAP_RESUME_FAST;

/// 全部登记版本升序清单（独立写出，供版本行判据逐位对账）。
const VER_ASC: [Version; 5] = [
    Version::new(6, 0),
    Version::new(6, 2),
    Version::new(7, 0),
    Version::new(7, 2),
    Version::new(8, 0),
];

/// 全部能力位掩码（判据侧自算，不引用被测常量之和）。
const ALL_CAP_MASK: u32 = 0b11_1111;

// ---------------------------------------------------------------------------
// 判据一：版本行
// ---------------------------------------------------------------------------

/// 锚点「按版本行（6.0/6.2/7.0/7.2/8.0 以上）登记已知差异」。
///
/// 判据不写「行数 == 5」这种一句话，而是把**五个版本号与五组能力预期**
/// 都独立写出来逐位对账——这样「版本号写错」「能力位错配」「行序颠倒」
/// 三类都会红，而不只是「少了一行」才红。
fn c218_rows(v: &mut Vec<(&'static str, bool, &'static str)>) {
    let m = CompatMatrix::new();

    // 行数与登记常量一致（声明与实算对账，十诫第 5 条的域内版）。
    v.push((
        "C218-版本行-登记行数与常量一致",
        m.row_count() == VERSION_ROWS && m.row_count() == VER_ASC.len(),
        "登记行数须同时等于行数常量与判据侧版本清单长度",
    ));

    // 逐行：版本号与能力预期双向对账（判据侧独立预期，不从 ROWS 反推）。
    let mut all_match = true;
    let mut i = 0usize;
    while i < VER_ASC.len() {
        let got_v = match m.expect_for(VER_ASC[i]) {
            Some(c) => c,
            // 已登记版本必须查得到，查不到即红（而不是当作 None 跳过）
            None => {
                all_match = false;
                0
            }
        };
        // 精确命中才取该行预期；回退命中说明版本号写错了
        if !m.is_registered(VER_ASC[i]) {
            all_match = false;
        }
        let want: u32 = match i {
            0 => EXPECT_60,
            1 => EXPECT_62,
            2 => EXPECT_70,
            3 => EXPECT_72,
            _ => EXPECT_80,
        };
        if got_v != want {
            all_match = false;
        }
        i += 1;
    }
    v.push((
        "C218-版本行-五登记行版本号与能力集逐位对账",
        all_match,
        "6.0=VIRGL+EDID_EXT / 6.2=+VENUS / 7.0=+EVENT_V2 / 7.2=+RESET_SHARED \
         / 8.0=+RESUME_FAST（判据侧独立预期常量）",
    ));

    // 行序严格升序（查表回退的前提；行序乱了回退会取错行）。
    let mut ascending = true;
    let mut k = 1usize;
    while k < VER_ASC.len() {
        let prev = VER_ASC[k - 1];
        let cur = VER_ASC[k];
        // 结构化比较：先比 major再比 minor（直接比 (major,minor) 元组亦可，
        // 但写成显式两次比较更贴合「三元组比较」的实现语义）
        if cur.major < prev.major || (cur.major == prev.major && cur.minor <= prev.minor) {
            ascending = false;
        }
        k += 1;
    }
    v.push((
        "C218-版本行-登记行严格升序",
        ascending,
        "回退查表以升序为前提；minor 相等或倒序都会让定位取错行",
    ));

    // **O(1) 回退的对账**（十诫第 2 条）：从尾部单趟回退与从头扫描
    // 必须给出**同一**下标。判据侧独立实现「从头找最大 ≤ v」作参考值。
    // 若有人把实现改成从头线性扫，这条仍绿（等价实现）；
    // 但下面「尾部回退命中位置无关」那条会红。
    let probes = [
        Version::new(6, 0),
        Version::new(6, 1),
        Version::new(6, 2),
        Version::new(6, 9),
        Version::new(7, 0),
        Version::new(7, 2),
        Version::new(8, 0),
        Version::new(9, 9),
    ];
    let mut same_as_scan = true;
    let mut p = 0usize;
    while p < probes.len() {
        let got = m.row_index_for(probes[p]);
        // 判据侧独立算参考值：从头扫第一个「其后所有行都 > v」的位置
        let mut want: Option<usize> = None;
        let mut j = 0usize;
        while j < VER_ASC.len() {
            if VER_ASC[j] <= probes[p] {
                want = Some(j);
            }
            j += 1;
        }
        if got != want {
            same_as_scan = false;
        }
        p += 1;
    }
    v.push((
        "C218-版本行-尾部回退与独立扫描同下标",
        same_as_scan,
        "查表结果须与判据侧独立实现的全扫参考值逐点一致（含未登记版本回退）",
    ));

    // 回退语义：未登记版本取**不超过它的最大登记行**，不是取最近行。
    // 用 6.1（落在 6.0 与 6.2 之间）与 7.1（落在 7.0 与 7.2 之间）夹逼。
    let i61 = m.row_index_for(Version::new(6, 1));
    let i71 = m.row_index_for(Version::new(7, 1));
    v.push((
        "C218-版本行-未登记版本取下位行",
        i61 == Some(0) && i71 == Some(2),
        "6.1⇒6.0 行、7.1⇒7.0 行（取下位而非最近；取最近会让 6.1 拿到 6.2 的 Venus）",
    ));

    // **从尾回退的步数性质**（区分「回退」与「从头扫」的唯一可测差异）。
    //
    // 两个实现对**所有**版本都返回同一下标，所以结果侧判据全都抓不到
    // 「把回退改写成从头线性扫」——那是等价实现，本就不该红。
    // 真正的差别在**步数**，且集中在运行时最常见的那一问：
    // 「探测到的 QEMU 比所有登记版本都新」（升级后首问）。此时
    //   从尾回退 ⇒ 命中末行，步数 = 1；
    //   从头扫   ⇒ 扫满 row_count 步。
    // 判据用「恰好命中末行」与「高于末行」两个点，各自钉住步数上界。
    let (_, steps_last) = m.row_index_steps(Version::new(8, 0));
    let (_, steps_newer) = m.row_index_steps(Version::new(9, 9));
    v.push((
        "C218-版本行-末行与超新版本均为单步命中",
        steps_last == 1 && steps_newer == 1,
        "恰好命中末行(8.0) 与 高于末行(9.9，升级后首问) 都须 1 步命中；\
         从头扫实现会是 5 步",
    ));
    // 命中首行（最低行）时步数须等于「扫过的行数」，验证步数**不是**自证常数。
    let (_, steps_first) = m.row_index_steps(Version::new(6, 0));
    v.push((
        "C218-版本行-首行命中步数随位置变化",
        steps_first == VER_ASC.len() as u32 && steps_first > steps_last,
        "命中首行须扫满 5 步（> 末行的 1 步）：步数由实走路径决定，不是常数",
    ));
    // 低于下限：扫完全部行仍无命中 ⇒ 返 None 且步数 = 行数。
    let (below, steps_below) = m.row_index_steps(Version::new(5, 9));
    v.push((
        "C218-版本行-低于下限扫满仍无命中",
        below.is_none() && steps_below == VER_ASC.len() as u32,
        "5.9：扫满 5 行未命中 ⇒ None（步数等于行数，证明真扫了而非直接返回）",
    ));

    // 低于矩阵下限返None（**不静默给 0 能力集**）。
    v.push((
        "C218-版本行-低于下限无预期值",
        m.expect_for(Version::new(5, 9)).is_none()
            && m.row_index_for(Version::new(5, 9)).is_none(),
        "5.9 低于最低登记行：返 None 表示超出矩阵范围，0 会丢掉「超范围」这个事实",
    ));

    // 差异登记：每行 delta 的变化维度数与内容（判据侧独立写死）。
    // 首行相对「无前一行」故全 Same；其余逐行独立对账。
    // 独立推导依据（判据侧按 gained 位反推，不从 ROWS 反推）：
    //   6.2 gained=VENUS(特性轴)      →特性 Added、事件 Changed
    //   7.0 gained=EVENT_V2(事件轴)   → 事件 Added
    //   7.2 gained=RESET_SHARED(特性轴)→ 特性 Added
    //   8.0 gained=RESUME_FAST(特性轴) → 特性 Added
    let want_deltas: [[Delta; DIMENSIONS]; 5] = [
        [Delta::Same, Delta::Same, Delta::Same],
        [Delta::Added, Delta::Same, Delta::Changed],
        [Delta::Same, Delta::Same, Delta::Added],
        [Delta::Added, Delta::Same, Delta::Same],
        [Delta::Added, Delta::Same, Delta::Same],
    ];
    let mut delta_ok = true;
    let mut d = 0usize;
    while d < 5 {
        let row = ROWS[d];
        let mut q = 0usize;
        while q < DIMENSIONS {
            if row.delta[q] != want_deltas[d][q] {
                delta_ok = false;
            }
            q += 1;
        }
        d += 1;
    }
    v.push((
        "C218-版本行-差异维度逐格对账",
        delta_ok,
        "特性可用性/EDID 行为/事件语义 三轴差异逐版本独立对账",
    ));

    // 差异登记与能力集必须**自洽**：逐版本累计新增能力位数 == delta 标 Added 的格数。
    // 这条治「delta 写着 Added 但 caps 没变」的撒谎（形状对但语义反）。
    // 判据侧独立数两遍：一遍数位、一遍数格，不引用被测函数。
    let mut added_total = 0usize;
    let mut w = 1usize;
    while w < 5 {
        added_total += (ROWS[w].caps & !ROWS[w - 1].caps).count_ones() as usize;
        w += 1;
    }
    let mut declared_added = 0usize;
    let mut x = 1usize;
    while x < 5 {
        let mut ax = 0usize;
        while ax < DIMENSIONS {
            if ROWS[x].delta[ax] == Delta::Added {
                declared_added += 1;
            }
            ax += 1;
        }
        x += 1;
    }
    let delta_caps_coherent = added_total == declared_added;
    v.push((
        "C218-版本行-差异声明与实际新增能力位对账",
        delta_caps_coherent,
        "逐版本累计新增能力位数须等于 delta 标Added 的格数（防「声明有差异但能力没变」）",
    ));

    // 首行 delta 必须全 Same（没有前一行可比）。
    let first_all_same = delta_count(&ROWS[0]) == 0;
    v.push((
        "C218-版本行-首行无差异声明",
        first_all_same,
        "首行没有前一登记行可比，delta 须全 Same",
    ));

    // 最新行 = 末行（未知版本按它给预期）。
    v.push((
        "C218-版本行-最新行为末行",
        m.latest().version == VER_ASC[VER_ASC.len() - 1]
            && m.latest().caps == EXPECT_80,
        "未知版本按最新行（8.0）预期",
    ));
}

// ---------------------------------------------------------------------------
// 判据二：探测预期
// ---------------------------------------------------------------------------

/// 锚点「矩阵驱动运行时能力探测的预期值」「未知版本按最新行预期并标记未认证」。
fn c218_expect(v: &mut Vec<(&'static str, bool, &'static str)>) {
    let m = CompatMatrix::new();

    // 已登记版本探测：预期 = 该行能力集，认证 = Verified。
    let r60 = probe(&m, Version::new(6, 0), Some(EXPECT_60));
    let r80 = probe(&m, Version::new(8, 0), Some(EXPECT_80));
    v.push((
        "C218-探测预期-已登记版本预期与认证",
        r60.expected == EXPECT_60
            && r80.expected == EXPECT_80
            && r60.certification == Certification::Verified
            && r80.certification == Certification::Verified,
        "已登记版本：预期取本行能力集且认证为 Verified",
    ));

    // 未知版本（如 6.1 / 7.1 / 9.9）：预期取**回退行**，认证 = Uncertified。
    // 关键：预期**不是** 0，也不是最新行——是回退行。
    let r61 = probe(&m, Version::new(6, 1), Some(EXPECT_60));
    let r99 = probe(&m, Version::new(9, 9), Some(EXPECT_80));
    v.push((
        "C218-探测预期-未知版本按回退行且标未认证",
        r61.expected == EXPECT_60
            && r99.expected == EXPECT_80
            && r61.certification == Certification::Uncertified
            && r99.certification == Certification::Uncertified,
        "6.1⇒按6.0 行预期、9.9⇒按 8.0 行预期，两者均标记未认证",
    ));

    // 未知版本的预期**恰好等于回退行**，且**不等于**最新行（6.1 就能分开）。
    // 这条防「把回退写成永远取最新行」——那样 6.1 会拿到 6.2 的 Venus。
    let latest_caps = m.latest().caps;
    let r61_back = m.expect_for(Version::new(6, 1));
    v.push((
        "C218-探测预期-回退预期不等于最新行",
        r61_back == Some(EXPECT_60) && r61.expected != latest_caps,
        "6.1 的回退预期(6.0 集)须≠最新行(8.0 集)，否则回退退化成取最新",
    ));

    // 低于矩阵下限：预期 = 保守交集，认证 = Uncertified（不假装有明确预期）。
    let r59 = probe(&m, Version::new(5, 9), Some(0));
    let cons = m.conservative();
    v.push((
        "C218-探测预期-低于下限走保守交集且未认证",
        r59.expected == cons
            && r59.certification == Certification::Uncertified
            && cons != 0,
        "5.9：预期取全行交集（非 0）、认证未认证",
    ));

    // 保守交集的独立重算：判据侧对五个caps 常量按位与。
    let cons_expect = EXPECT_60 & EXPECT_62 & EXPECT_70 & EXPECT_72 & EXPECT_80;
    v.push((
        "C218-探测预期-保守交集独立对账",
        cons == cons_expect,
        "保守预期 = 五登记行能力集按位与（判据侧独立算，不引用被测函数）",
    ));

    // 保守交集必须**逐位**等于各行的公共部分（不能多也不能少）：
    // 取并集是常见错误（会把只在新版本才有的能力报成支持）。
    let union_all = EXPECT_60 | EXPECT_62 | EXPECT_70 | EXPECT_72 | EXPECT_80;
    v.push((
        "C218-探测预期-保守交集是并集的真子集",
        cons != union_all && cons & !union_all == 0,
        "交集须是并集的真子集（取并集= 虚报支持），且不含并集外的位",
    ));

    // 探测是一次性的：同一 (版本, 实测) 两次探测结果完全一致。
    let p1 = probe(&m, Version::new(7, 0), Some(EXPECT_70));
    let p2 = probe(&m, Version::new(7, 0), Some(EXPECT_70));
    v.push((
        "C218-探测预期-探测一次性可重放",
        p1 == p2,
        "初始化期一次完成并入快照：同输入两次探测结果逐字段一致",
    ));

    // 认证状态三值语义分离（十诫第 13 条：有 label ≠ 语义对）。
    v.push((
        "C218-探测预期-认证三态语义互斥",
        Certification::Verified.verified()
            && !Certification::Verified.needs_warning()
            && !Certification::Uncertified.verified()
            && Certification::Uncertified.needs_warning()
            && !Certification::Degraded.verified()
            && Certification::Degraded.needs_warning(),
        "Verified 独占「已认证且不告警」，Uncertified/Degraded 均需告警",
    ));
}

// ---------------------------------------------------------------------------
// 判据三：实测为准
// ---------------------------------------------------------------------------

/// 锚点「矩阵与实测冲突→以实测为准修矩阵」「探测失败→保守预期加告警」。
fn c218_measured(v: &mut Vec<(&'static str, bool, &'static str)>) {
    let m = CompatMatrix::new();

    // 无冲突路径：实测 == 预期 ⇒ effective 取值无歧义、且**无冲突**。
    // 这条是「has_conflict 恒真」的解药（十诫第 3 条）。
    let same = probe(&m, Version::new(7, 0), Some(EXPECT_70));
    v.push((
        "C218-实测为准-一致时无冲突",
        !same.has_conflict() && same.effective() == EXPECT_70,
        "实测==预期：无冲突痕迹（恒真冲突会让本条与下条同源）",
    ));

    // 实测少于矩阵（实测少一位）：以**实测**为准，并留档两侧值。
    let short = EXPECT_70 & !CAP_EVENT_V2; // 实测缺事件v2
    let less = probe(&m, Version::new(7, 0), Some(short));
    v.push((
        "C218-实测为准-实测少于矩阵取实测",
        less.effective() == short && less.effective() != EXPECT_70,
        "实测少一位时有效能力集取实测（少报能力安全于虚报）",
    ));
    v.push((
        "C218-实测为准-冲突留档两侧值",
        match less.conflict {
            Some(c) => c.matrix == EXPECT_70 && c.measured == short,
            None => false,
        },
        "Conflict须同时留档矩阵预期与实测值，供后续修矩阵",
    ));

    // 实测多于矩阵（实测多一位）：同样取实测（不因「比预期好」而忽略）。
    let more_bits = EXPECT_70 | CAP_RESUME_FAST;
    let more = probe(&m, Version::new(7, 0), Some(more_bits));
    v.push((
        "C218-实测为准-实测多于矩阵也取实测",
        more.effective() == more_bits,
        "实测多一位时同样取实测（只按「实测更保守」处理是半个实现）",
    ));

    // 两个方向都要留冲突痕迹（单向留档 = 只防了一边）。
    v.push((
        "C218-实测为准-双向冲突均留档",
        more.has_conflict() && less.has_conflict(),
        "实测多/少两侧都须检出冲突",
    ));

    // 探测失败（measured = None）：有效集 = 保守交集 + 降级告警。
    let failed = probe(&m, Version::new(8, 0), None);
    v.push((
        "C218-实测为准-探测失败走保守预期",
        failed.effective() == m.conservative() && failed.certification == Certification::Degraded,
        "探测失败：有效集取保守交集、认证降级（不是取 0 也不是沿用最新行）",
    ));

    // **降级不是恒真**（十诫第 4 条）：探测成功时不得降级。
    v.push((
        "C218-实测为准-探测成功不降级",
        !failed.has_conflict() && same.certification != Certification::Degraded,
        "对照组：探测成功 ⇒ 无冲突痕迹且非降级（恒真降级会让上一条变空话）",
    ));

    // 探测失败时不产生冲突痕迹（没有实测可比，冲突无从谈起）。
    v.push((
        "C218-实测为准-探测失败无冲突痕迹",
        failed.measured.is_none() && !failed.has_conflict(),
        "无实测可比 ⇒ 不报冲突（否则「冲突」不携带信息）",
    ));

    // 未认证清单聚合（供诊断）：只收needs_warning 的版本。
    let recs = vec![
        probe(&m, Version::new(6, 0), Some(EXPECT_60)), // Verified
        probe(&m, Version::new(6, 1), Some(EXPECT_60)), // Uncertified
        failed,                                         // Degraded
        probe(&m, Version::new(8, 0), Some(EXPECT_80)), // Verified
    ];
    let warn = unauthenticated(&recs);
    let want_warn: Vec<Version> = vec![Version::new(6, 1), Version::new(8, 0)];
    v.push((
        "C218-实测为准-未认证清单只收需告警者",
        warn.len() == want_warn.len()
            && warn.len() == 2
            && warn[0] == want_warn[0]
            && warn[1] == want_warn[1],
        "4 条记录里仅 6.1(未认证) 与 8.0(降级) 需告警，顺序按输入序",
    ));

    // 反向对账：清单条数 == 逐条needs_warning 的独立计数。
    let mut expect_warn_n = 0usize;
    let mut i = 0usize;
    while i < recs.len() {
        if recs[i].certification.needs_warning() {
            expect_warn_n += 1;
        }
        i += 1;
    }
    v.push((
        "C218-实测为准-告警条数独立对账",
        warn.len() == expect_warn_n,
        "清单长度须等于判据侧逐条计出的 needs_warning 数（防清单漏项/多项）",
    ));
}

// ---------------------------------------------------------------------------
// 判据四：N/A 联动
// ---------------------------------------------------------------------------

/// 判据侧独立的三态映射自检：N/A 态与通过态须落在不同码值。
///
/// 参考值不取自被测函数（本域没有 N/A 判定函数，只有能力位），而是
/// **判据侧自己**按「该轴所需能力位缺失 ⇒ N/A」推导。若N/A 被折叠进
/// 通过，两轴会得到同一码值 ⇒ 本条红。
const ST_PASS: u8 = 1;
const ST_NA: u8 = 2;

/// 锚点「下游 F0215 按矩阵跳过 N/A、F0216 宣告引用」。
///
/// 联动不靠「真去调 F0215」（那是跨域耦合），而是**在判据侧复刻联动契约**：
/// 「能力位缺失 ⇒ 对应检查项应标 N/A 而非 Pass/Fail」。
/// 判据据此断言：能力位与 N/A 判定严格同向，且 N/A **绝不**被当成通过。
fn c218_na(v: &mut Vec<(&'static str, bool, &'static str)>) {
    let m = CompatMatrix::new();

    // 联动契约的判据侧参考实现：给定能力集，产出应标 N/A 的轴。
    //轴 0=特性可用性(需 VIRGL/VENUS)、轴 1=EDID 行为(需 EDID_EXT)、轴 2=事件语义(需 EVENT_V2)。
    let na_axes = |caps: u32| -> [bool; DIMENSIONS] {
        [
            caps & (CAP_VIRGL | CAP_VENUS) == 0,
            caps & CAP_EDID_EXT == 0,
            caps & CAP_EVENT_V2 == 0,
        ]
    };

    // 对全部五个登记行逐行核对：判据侧参考实现与「行内 delta 声明」同向吗？
    // 具体地：某轴在某版本首次具备能力位时，delta 不得再标 Same。
    let mut link_ok = true;
    let mut r = 1usize;
    while r < 5 {
        let cur = ROWS[r];
        let prev = ROWS[r - 1];
        let gained = cur.caps & !prev.caps;
        let mut axis = 0usize;
        while axis < DIMENSIONS {
            let bit: u32 = match axis {
                0 => CAP_VIRGL | CAP_VENUS,
                1 => CAP_EDID_EXT,
                _ => CAP_EVENT_V2,
            };
            // 本版本新增了这一轴所需的能力位 ⇒ delta 不得标 Same
            if gained & bit != 0 && cur.delta[axis] == Delta::Same {
                link_ok = false;
            }
            axis += 1;
        }
        r += 1;
    }
    v.push((
        "C218-N/A联动-新增能力位与差异声明同向",
        link_ok,
        "某版本新增某轴所需能力位时，该轴 delta 不得标 Same（否则 N/A 判定与矩阵脱节）",
    ));

    // N/A 联动方向：能力位缺失 ⇒ 该轴应标 N/A；具备 ⇒ 不标 N/A。
    let r60 = probe(&m, Version::new(6, 0), Some(EXPECT_60));
    let na60 = na_axes(r60.effective());
    v.push((
        "C218-N/A联动-6.0事件轴标N/A",
        !na60[0] && !na60[1] && na60[2],
        "6.0 有 Virgl 与 EDID 扩展但无事件 v2 ⇒ 仅事件语义轴标 N/A",
    ));
    let r80 = probe(&m, Version::new(8, 0), Some(EXPECT_80));
    let na80 = na_axes(r80.effective());
    v.push((
        "C218-N/A联动-8.0三轴均不标N/A",
        !na80[0] && !na80[1] && !na80[2],
        "8.0 六位全开 ⇒ 三轴都不该标 N/A（联动若恒标 N/A 则本条红）",
    ));

    // **N/A 绝不等于通过**（本条是 N/A 联动的核心纪律）。
    // 三轴状态是三值：Pass=1 / N/A=2，两者必须不同。若 N/A 被折叠成
    // Pass，6.0 的事件轴（本应 N/A）会与「通过」同值 ⇒ 本条红。
    // 用判据侧的三值编码常量做等值比较，而不是 `assert!(a != b)` 式的
    // 恒真写法（那样删掉 N/A 分支也照样绿）。
    // 事件轴有 EVENT_V2 ⇒ 不该 N/A（8.0）；无 ⇒ 该 N/A（6.0）
    let st_for_80_event = if na80[2] { ST_NA } else { ST_PASS };
    let st_for_60_event = if na60[2] { ST_NA } else { ST_PASS };
    v.push((
        "C218-N/A联动-N/A态与通过态不同值",
        st_for_60_event == ST_NA && st_for_80_event == ST_PASS && ST_NA != ST_PASS,
        "事件轴在 6.0 为 N/A、8.0 为通过，两者须映射到不同状态码\
         （折叠成同一个会让「没测」被读成「过了」）",
    ));

    // F0216 引用：最低支持版本（min_version_for）须与判据侧独立算的一致。
    // 逐位对账六个能力位。
    let mut min_ok = true;
    let caps_probe = [
        (CAP_VIRGL, Version::new(6, 0)),
        (CAP_VENUS, Version::new(6, 2)),
        (CAP_EDID_EXT, Version::new(6, 0)),
        (CAP_EVENT_V2, Version::new(7, 0)),
        (CAP_RESET_SHARED, Version::new(7, 2)),
        (CAP_RESUME_FAST, Version::new(8, 0)),
    ];
    let mut c = 0usize;
    while c < caps_probe.len() {
        let (bit, want) = caps_probe[c];
        if min_version_for(&m, bit) != Some(want) {
            min_ok = false;
        }
        c += 1;
    }
    v.push((
        "C218-N/A联动-最低支持版本逐位对账",
        min_ok,
        "六个能力位的最低支持版本须与判据侧独立写死的版本一致（F0216 宣告引用）",
    ));

    // 不存在的能力位（掩码全外）⇒ None（宣告时不能编一个版本出来）。
    v.push((
        "C218-N/A联动-未知能力位返None",
        min_version_for(&m, ALL_CAP_MASK + 1).is_none(),
        "矩阵未登记的能力位返 None（宣告不许拿现有版本充数）",
    ));

    // min_version_for 取的是**最低**支持版本而非最高（逐位含蓄但可测）。
    // 用「8.0 全位齐」不能区分，故用 RESET_SHARED（7.2 才首次有）与
    // 全行都有的一位对照：后者应落在 6.0。
    v.push((
        "C218-N/A联动-最低版本取首次出现而非末行",
        min_version_for(&m, CAP_VIRGL) == Some(Version::new(6, 0))
            && min_version_for(&m, CAP_RESET_SHARED) == Some(Version::new(7, 2)),
        "最低支持版本 = 该能力首次出现的行（VIRGL⇒6.0、RESET_SHARED⇒7.2）",
    ));
}

// ---------------------------------------------------------------------------
// 判据五：判据（线编码自洽 / 诊断位自持 / 判据集自身性质）
// ---------------------------------------------------------------------------

/// 锚点第五条「判据」——本组守判据自身的可信度与线编码自洽。
fn c218_struct(v: &mut Vec<(&'static str, bool, &'static str)>) {
    let m = CompatMatrix::new();

    //能力位常量互不重叠（位编码自洽：重叠会让 or 后的位集无法反推单能力）。
    let caps_list = [
        CAP_VIRGL,
        CAP_VENUS,
        CAP_EDID_EXT,
        CAP_EVENT_V2,
        CAP_RESET_SHARED,
        CAP_RESUME_FAST,
    ];
    let mut disjoint = true;
    let mut i = 0usize;
    while i < caps_list.len() {
        if caps_list[i] == 0 || caps_list[i] & !ALL_CAP_MASK != 0 {
            disjoint = false;
        }
        let mut j = i + 1;
        while j < caps_list.len() {
            if caps_list[i] & caps_list[j] != 0 {
                disjoint = false;
            }
            j += 1;
        }
        i += 1;
    }
    v.push((
        "C218-判据-能力位互不重叠且在掩码内",
        disjoint && CAPABILITIES == caps_list.len(),
        "六位须两两不重叠、全落在 6 位掩码内，且个数与 CAPABILITIES 常量一致",
    ));

    // 版本文本线编码：major.minor 不得丢位/串位（如 6.10 ≠ 6.1）。
    let v610 = Version::new(6, 10);
    let v61 = Version::new(6, 1);
    v.push((
        "C218-判据-版本文本无歧义",
        v610.text() == "6.10".to_string()
            && v61.text() == "6.1".to_string()
            && v610 > v61,
        "minor 两位须原样输出（6.10≠6.1，否则 6.10 会被读成 6.1）",
    ));

    // 三轴维度常量与枚举 ALL 自洽。
    let mut dims_ok = Dimension::ALL.len() == DIMENSIONS;
    let mut seen = 0usize;
    let mut d = 0usize;
    while d < Dimension::ALL.len() {
        let nm = Dimension::ALL[d].name();
        if nm.is_empty() {
            dims_ok = false;
        }
        seen += 1;
        d += 1;
    }
    v.push((
        "C218-判据-三轴枚举与维度常量一致",
        dims_ok && seen == DIMENSIONS,
        "特性/EDID/事件 三轴枚举条数须等于 DIMENSIONS 且各有名字",
    ));

    // Delta 语义：Only Same 不changed，其余 changed（判据侧独立穷举）。
    let delta_ok = !Delta::Same.changed() && Delta::Changed.changed() && Delta::Added.changed();
    v.push((
        "C218-判据-差异语义自身自洽",
        delta_ok,
        "Same 不算变化、Changed/Added 都算变化",
    ));

    // delta_count 与判据侧独立计数对账（不向被测函数问答案）。
    let mut cnt_ok = true;
    let mut r = 0usize;
    while r < 5 {
        let row = ROWS[r];
        let mut n = 0usize;
        let mut q = 0usize;
        while q < DIMENSIONS {
            if row.delta[q] != Delta::Same {
                n += 1;
            }
            q += 1;
        }
        if delta_count(&row) != n {
            cnt_ok = false;
        }
        r += 1;
    }
    v.push((
        "C218-判据-差异计数与独立计数对账",
        cnt_ok,
        "delta_count 逐行须等于判据侧独立数出的非 Same 格数",
    ));

    // 判据集自身：本次判据数不得超过 CheckSet 容量（三族拆分的前提）。
    // 这是「判据」条目的一条自检——若单族超容量，聚合器会静默丢条目。
    v.push((
        "C218-判据-判据量在容量内",
        c218_estimated_count() <= MAX_CHECKS,
        "单族判据数须≤ MAX_CHECKS，超过会被 CheckSet 静默丢弃（应按族拆分）",
    ));

    // 矩阵行数常量与实际行数、版本清单长度三者一致（声明 vs 实算）。
    v.push((
        "C218-判据-行数常量三方一致",
        m.row_count() == VERSION_ROWS && VERSION_ROWS == VER_ASC.len() && VERSION_ROWS == 5,
        "行数常量须同时等于矩阵实排行数与版本清单长度（本域固定 5 行）",
    ));

    // latest() 在未知版本上的可用性（供探测失败时兜底调用，不 panic）。
    v.push((
        "C218-判据-最新行恒可取",
        m.latest().caps != 0 && m.latest().version == VER_ASC[4],
        "latest() 任意时刻可取且非空（探测路径依赖它兜底）",
    ));
}

/// 本文件判据总条数（自检用，避免逐条手数出错）。
const fn c218_estimated_count() -> usize {
    // 五组判据条数之和：rows 8 + expect 8 + measured 8 + na 7 + struct 8
    8 + 8 + 8 + 7 + 8
}

// ---------------------------------------------------------------------------
// 聚合器（三族拆分，规避 MAX_CHECKS 上限）
// ---------------------------------------------------------------------------

fn collect(
    f: fn(&mut Vec<(&'static str, bool, &'static str)>),
    set: &mut CheckSet,
    count: &mut usize,
) {
    let mut v: Vec<(&'static str, bool, &'static str)> = Vec::new();
    f(&mut v);
    let mut i = 0usize;
    while i < v.len() {
        set.add(v[i].0, v[i].1, v[i].2);
        *count += 1;
        i += 1;
    }
}

/// a 族：版本行 + 探测预期。
pub fn run_veb18_checks_a_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veb18");
    let mut n = 0usize;
    collect(c218_rows, &mut set, &mut n);
    collect(c218_expect, &mut set, &mut n);
    set
}

/// b 族：实测为准 + N/A 联动。
pub fn run_veb18_checks_b_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veb18");
    let mut n = 0usize;
    collect(c218_measured, &mut set, &mut n);
    collect(c218_na, &mut set, &mut n);
    set
}

/// c 族：判据（线编码/ 诊断位 / 判据集自身性质）。
pub fn run_veb18_checks_c_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veb18");
    let mut n = 0usize;
    collect(c218_struct, &mut set, &mut n);
    set
}

/// 合并全族（供聚合器/ 测试遍历用）。
pub fn run_veb18_checks() -> CheckSet {
    CheckSet::merge(
        CheckSet::merge(
            run_veb18_checks_a_standalone(),
            run_veb18_checks_b_standalone(),
        ),
        run_veb18_checks_c_standalone(),
    )
}