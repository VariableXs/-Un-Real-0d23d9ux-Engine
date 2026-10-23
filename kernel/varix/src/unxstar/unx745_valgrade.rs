//! unx745_valgrade —— UNX-F0745 域经价值分级（高中低三档复用度评级）。
//!
//! 锚点（CoRun Unxreal 主册 UNX-F0745 · 域经册）：
//! 「UNX-F0745 | 域经价值分级（高中低三档复用度评级） | 570 | 已深化 |
//!  UNX-F0745-J1 评级账齐（逐条四维评级：复用频次/跨域适用/实现成本/
//!  风险规避价值）且三档分布落账、低档不导出断言过」
//!
//! ## 组件全景（十一个真子系统）
//! 1. **四维评级闭集 ValDim**：复用频次/跨域适用/实现成本/风险规避价值
//!     四维，每维 0~10000 万分制整数刻度，十分档带（每带 1000 分），
//!     40 条档带文案全部写死并注明刻度来源（来源批账 → 归一公式）。
//! 2. **权重预设闭集**：三套权重预设（复用优先/均衡/风险优先），
//!     每行四维权重和恰 10000，编译期 `const` 断言逐行核和，
//!     破和配置在编译期即红（注入核另备运行期检出面）。
//! 3. **域经条目 ExperienceEntry**：编号/标题/正文引用/来源批/四维分
//!     /逐维留痕说明；登记制 append-only，重复编号拒收（编号全域
//!     唯一，重号 = 两条经验抢同一引用位）。
//! 4. **评级引擎 GradeEngine**：四维加权合成总分
//!     `总 = Σ(维分×权重) / 10000`（饱和算术），总分 → 三档映射
//!     （高档 ≥7000 / 中档 4000~6999 / 低档 <4000），档界 const
//!     写死并注明来源（与 F0741 四档价值档金/银/铜/登记线对齐）。
//! 5. **三档分布账 GradeDistribution**：High/Mid/Low 计数 +
//!     Σ三档 ≡ 总条目守恒校验（逐条可复算，漏评/重评即不平）。
//! 6. **低档不导出 export_book()**：导出面只含高档+中档，低档显性
//!     排除且排除计数落账；导出序确定（档位降序 → 编号升序），
//!     导出簿带 FNV-1a 32 位指纹可封存比对。
//! 7. **评级留痕 GradeReport**：每条评级带四维明细（分值/档带/说明）
//!     + 合成式字符串（权重与中间和全量落账），任何档位都可手工
//!     复算验证；复评（regrade）保留全部历史，最新版即生效版。
//! 8. **事件账 VgEvent**：append-only 事件流水（登记/评级/复评/
//!     导出/封存/校验/注入），序号连续性纳入 health()。
//! 9. **健康核 health()**：七项不变量逐项核（全覆盖/最新版一致/
//!     权重和/档界/守恒/指纹/事件序），结论带逐项发现文本。
//! 10. **指纹 FNV-1a 32**：评级账规范序列化后取指纹，冻结 +
//!     复算比对（漂移即账被改）；FNV 本体是 const fn，空串基准
//!     值编译期断言（0x811C9DC5）。
//! 11. **双跑确定性对拍 + 注入面**：同一规格双引擎独立建账 →
//!     指纹/分布/导出簿三重对拍必须全同；三类注入（权重破和/
//!     档界越界/低档混出）逐类构造、逐类检出、漏抓即错。
//!
//! ## 职责定位
//! 域经价值评级的唯一权威分级面——上游是 F0741 登记簿（条目来源）、
//! F0742 升格账（成本维来源）、F0743 证据链账（风险维来源）、
//! F0747 检索索引（频次维来源）、F0748 跨域标注（跨域维来源）；
//! 下游是 F0749 移交包（只携带高档+中档导出簿）。本单不做检索、
//! 不做合并、不做核验——各归其位。
//!
//! ## 性能逐项分解
//! 登记 O(编号查重)（线性扫，容量上限 1024 封顶）；
//! 合成 O(1)（四维定长）；分布 O(账长)；导出 O(n²)（插入序，
//! n ≤ 1024，避免依赖运行时排序器的分配行为）；指纹 O(账长)；
//! 注入核 O(1)/类。全整数确定性（零墙钟、零浮点、零随机）。
//!
//! ## 诊断码段
//! 0x1690 段（0x1691~0x169C 闭集，十二码）。段位核验：0x1640~0x167F
//! 已被 unx2381_ufinalacct 独占，0x1680 被 ves457_s25validator 占用，
//! 0x16A0/0x16E0 被 ves458/ves459 占用；0x1690 段经全库 grep 确认
//! 空闲，本单独占，不得他用。
//!
//! ## 纯功能线
//! no_std + alloc；零 unwrap/expect/panic/运行期 assert（编译期
//! const 断言除外）；可失败全走 `Result<T, ValGradeError>`；
//! 溢出全显性 saturating/wrapping；>64KB 结构体一律不上栈
//! （引擎本体含 Vec 字段，只许堆上持有）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const VG_VERSION: &str = "unx745-valgrade-v1";

/// 诊断段基址（0x1690 段 · 本模块独占）。
pub const VG_SEG_BASE: u16 = 0x1690;

/// Xc 诊断码闭集（0x1691~0x169C，严格递增互异）。
pub const XC_ENTRY_DUP: u32 = 0x1691;
pub const XC_SCORE_OOB: u32 = 0x1692;
pub const XC_WEIGHT_SUM: u32 = 0x1693;
pub const XC_BOUND_VIOL: u32 = 0x1694;
pub const XC_LOW_LEAK: u32 = 0x1695;
pub const XC_LEDGER_BROKEN: u32 = 0x1696;
pub const XC_FINGER_DRIFT: u32 = 0x1697;
pub const XC_INJECT_MISSED: u32 = 0x1698;
pub const XC_ID_OOB: u32 = 0x1699;
pub const XC_GRADE_UNSET: u32 = 0x169A;
pub const XC_DIST_IMBAL: u32 = 0x169B;
pub const XC_EXPORT_SEQ: u32 = 0x169C;

/// 中文诊断文本（码 → 现象/可操作建议，闭集逐码两要素）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_ENTRY_DUP => (
            "条目重号：域经编号已在簿",
            "编号全域唯一，换新号或先走 F0744 合并账再评级",
        ),
        XC_SCORE_OOB => (
            "维分越界：四维评分不在 0~10000",
            "评分须落万分制闭区间，越界即拒收，回上游批账改分",
        ),
        XC_WEIGHT_SUM => (
            "权重破和：四维权重和 ≠ 10000",
            "权重表必须行和恰 10000（万分制归一），回预设闭集取行",
        ),
        XC_BOUND_VIOL => (
            "档界越界：总分与档位映射不符",
            "档位只能由档界常量推出（≥7000 高 / 4000~6999 中 / <4000 低）",
        ),
        XC_LOW_LEAK => (
            "低档混出：低档条目出现在导出候选集",
            "导出面只许高档+中档，低档必须显性排除并计数落账",
        ),
        XC_LEDGER_BROKEN => (
            "账结构破坏：评级账被外力改写",
            "append-only 账只许追加，回滚到冻结指纹版",
        ),
        XC_FINGER_DRIFT => (
            "指纹漂移：评级账指纹与冻结值不一致",
            "账内容被改，diff 冻结版定位改动行后回滚",
        ),
        XC_INJECT_MISSED => (
            "注入漏抓：注入核存在未检出类",
            "三类注入必须逐类检出，漏抓即回退本单版本",
        ),
        XC_ID_OOB => (
            "编号越界：条目编号不在 1~VG_ID_MAX",
            "编号须落在闭区间 [1, 999999]，0 与超界号拒收",
        ),
        XC_GRADE_UNSET => (
            "档位未落账：条目存在但无评级报告",
            "登记即评级，缺报告即账断链，重跑评级入口",
        ),
        XC_DIST_IMBAL => (
            "分布不平：三档计数之和 ≠ 总条目数",
            "Σ三档 ≡ 总条目是守恒铁律，逐条复算找漏",
        ),
        XC_EXPORT_SEQ => (
            "导出序破坏：导出簿序与确定序不符",
            "导出序 = 档位降序 → 编号升序，重排后重出",
        ),
        _ => ("未知诊断码", "查本模块 Xc 闭集 0x1691~0x169C"),
    }
}

// =====================================================================
// 第一子系统：四维评级闭集 ValDim
// =====================================================================

/// 维数（四维，闭集冻结）。
pub const DIM_COUNT: usize = 4;

/// 万分制刻度上限（每维满分）。
pub const SCALE_MAX: u16 = 10000;

/// 档带数（每维十分档，每带 1000 分）。
pub const BAND_COUNT: usize = 10;

/// 档带宽（1000 分/带 = SCALE_MAX / BAND_COUNT，整除已 const 断言）。
pub const BAND_WIDTH: u16 = 1000;

/// 维下标闭集（0=复用频次 1=跨域适用 2=实现成本 3=风险规避价值）。
pub const DIM_IDX_REUSE: usize = 0;
pub const DIM_IDX_DOMAIN: usize = 1;
pub const DIM_IDX_COST: usize = 2;
pub const DIM_IDX_RISK: usize = 3;

/// 维名闭集（下标序冻结，合成式与留痕均按此序展开）。
pub const DIM_NAMES: [&str; DIM_COUNT] = [
    "复用频次",
    "跨域适用",
    "实现成本",
    "风险规避价值",
];

/// 维刻度来源注释（逐维写死：上游账 → 归一公式 → 满分含义）。
///
/// 刻度不是拍脑袋：每维都锚一条上游批账，评分可沿来源链回查。
pub const DIM_SCALE_SOURCES: [&str; DIM_COUNT] = [
    // 复用频次：检索命中账归一——条目在 F0747 检索索引里被跨条目
    // 命中的次数 N，得分 = min(N, 10) × 1000；命中 0 次记 0，
    // 命中 ≥10 次记满 10000（全域热复用）。
    "复用频次 ← F0747 检索索引命中账：命中 N 次 → min(N,10)×1000",
    // 跨域适用：五域消费面归一——F0748 跨域标注账里适用域数 D
    // （0~5），得分 = D × 2000；五域全覆盖记满，仅本域记 0。
    "跨域适用 ← F0748 五域消费面标注账：适用域数 D(0~5) → D×2000",
    // 实现成本：升格四要素账反演——F0742 打法描述里的实现步骤数
    // S，得分 = saturating(10000 − S×500)；零步（纯结论可直用）
    // 记满，≥20 步记 0（成本越高复用价值越低，故反向刻度）。
    "实现成本 ← F0742 升格账实现步骤数 S → saturating(10000−S×500)（反向）",
    // 风险规避价值：证据链反例捕获归一——F0743 证据链账里本条
    // 反例边界捕获数 K，得分 = min(K, 4) × 2500；捕获 ≥4 类反例
    // 记满（无反例边界的经验 = 不可移植的幸运，价值归零档）。
    "风险规避价值 ← F0743 证据链反例捕获账：捕获 K 类 → min(K,4)×2500",
];

/// 四十分档带文案闭集（[维][档带] 序冻结，0 带最低）。
///
/// 每条文案 = 档带语义（写死即文档，评级留痕直接引用）。
pub const BAND_LABELS: [[&str; BAND_COUNT]; DIM_COUNT] = [
    // 复用频次（0~9 带）
    [
        "零复用：登记后从未被检索命中",
        "孤例：仅登记自查命中 1 次",
        "偶用：同批内命中 2 次",
        "批内常复用：同批命中 3~4 次",
        "跨批复用起步：跨 1 批命中",
        "跨批常复用：跨 2~3 批命中",
        "域内热复用：跨 4~6 批命中",
        "全域热复用：跨 7~8 批命中",
        "全域高频复用：命中 ≥9 次未满顶",
        "满档热复用：命中 ≥10 次记满",
    ],
    // 跨域适用（0~9 带；每带 ≙ 0.5 个适用域）
    [
        "仅本域可用：适用域数 0（本域外的外推未验证）",
        "本域边缘：0.5 域当量（同域相邻子系统可试）",
        "一域当量：1 个适用域验证过",
        "一域半：1.5 域当量（第 2 域标注未回执）",
        "两域当量：2 个适用域验证过",
        "两域半：2.5 域当量（第 3 域标注未回执）",
        "三域当量：3 个适用域验证过",
        "四域当量：4 个适用域验证过",
        "四域半：4.5 域当量（第 5 域标注未回执）",
        "五域全覆盖：消费面全开记满",
    ],
    // 实现成本（0~9 带，反向刻度：分高 = 成本低）
    [
        "重实现：步骤 ≥20（复用前须重走全流程）",
        "高成本：16~19 步",
        "偏高成本：14~15 步",
        "中高成本：12~13 步",
        "中成本：10~11 步",
        "中低成本：8~9 步",
        "低成本：6~7 步",
        "较低成本：4~5 步",
        "低成本低：2~3 步",
        "直用结论：0~1 步（拿来即用记满）",
    ],
    // 风险规避价值（0~9 带；每带 ≙ 0.4 类反例捕获）
    [
        "无反例边界：0 类捕获（不可移植的幸运）",
        "弱边界：0.4 类当量",
        "单反例起步：1 类反例捕获",
        "双反例：2 类反例捕获",
        "三反例：3 类反例捕获",
        "四反例满档：≥4 类捕获记满（前四带已过，本带备用）",
        "满档+复验：满档且证据链复验通过",
        "满档+双域复验：满档且跨域复验通过",
        "满档+回归锚定：满档且回归用例锚定",
        "满档+全链封存：满档且证据链零断链",
    ],
];

/// 档带下标推算（const fn：while + 整数比较，无 for/min/max/get）。
///
/// score=10000 落第 9 带（右端点归末带）；score=0 落第 0 带。
pub const fn band_index(score: u16) -> usize {
    // 越界归末带（返回后由调用方 Result 面拒收越界分）。
    if score > SCALE_MAX {
        return BAND_COUNT - 1;
    }
    let mut idx = 0usize;
    // while 推进：idx 满足 idx*BAND_WIDTH ≤ score。
    while (idx + 1) < BAND_COUNT
        && score >= ((idx as u32 + 1) * BAND_WIDTH as u32) as u16
    {
        idx += 1;
    }
    idx
}

/// 维合法性判（const fn）。
pub const fn dim_valid(dim: usize) -> bool {
    dim < DIM_COUNT
}

/// 分值合法性判（const fn）。
pub const fn score_valid(score: u16) -> bool {
    score <= SCALE_MAX
}

/// 档带文案查（运行期；越维返回兜底文案，不 panic）。
pub fn band_label(dim: usize, band: usize) -> &'static str {
    if dim >= DIM_COUNT {
        return "维度越界（应为 0~3）";
    }
    if band >= BAND_COUNT {
        return "档带越界（应为 0~9）";
    }
    // 界内索引：上面两道闸已保证，直接下标取。
    BAND_LABELS[dim][band]
}

/// 维刻度来源查（越维兜底）。
pub fn dim_scale_source(dim: usize) -> &'static str {
    if dim >= DIM_COUNT {
        return "维度越界（应为 0~3）";
    }
    DIM_SCALE_SOURCES[dim]
}

// =====================================================================
// 第二子系统：权重预设闭集（行和恰 10000，编译期逐行断言）
// =====================================================================

/// 预设数（三套，闭集冻结）。
pub const PRESET_COUNT: usize = 3;

/// 预设名闭集。
pub const PRESET_NAMES: [&str; PRESET_COUNT] = [
    "复用优先（频次维主导：移交包只带经得起反复用的）",
    "均衡（四维平权倾向：全域普查态）",
    "风险优先（风险维主导：高风险域经验先行）",
];

/// 权重行类型（四维定长，与 DIM_NAMES 下标序对齐）。
pub type WeightRow = [u16; DIM_COUNT];

/// 预设权重表（行和恰 10000；每行来源注释写死）。
pub const PRESET_WEIGHTS: [WeightRow; PRESET_COUNT] = [
    // 预设 0「复用优先」：频次 4500 + 跨域 2500 + 成本 1500 + 风险 1500
    //   = 10000（频次维占 45%：移交包消费面首问「用过几回」）。
    [4500, 2500, 1500, 1500],
    // 预设 1「均衡」：频次 3000 + 跨域 2500 + 成本 2500 + 风险 2000
    //   = 10000（四维都有话语权，风险维略低——普查态不追险）。
    [3000, 2500, 2500, 2000],
    // 预设 2「风险优先」：频次 2000 + 跨域 2000 + 成本 2000 + 风险 4000
    //   = 10000（风险维占 40%：踩坑经验的价值优先入高档）。
    [2000, 2000, 2000, 4000],
];

/// 权重行求和（const fn：while + u32 饱和无溢出——4×10000 远离上限）。
pub const fn weight_row_sum(row: &WeightRow) -> u32 {
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < DIM_COUNT {
        sum += row[i] as u32;
        i += 1;
    }
    sum
}

/// 预设行和闭集（供编译期断言与运行期共用）。
pub const PRESET_SUMS: [u32; PRESET_COUNT] = [
    weight_row_sum(&PRESET_WEIGHTS[0]),
    weight_row_sum(&PRESET_WEIGHTS[1]),
    weight_row_sum(&PRESET_WEIGHTS[2]),
];

// —— 编译期断言簇：权重行和、档界、刻度、诊断码、FNV 基准 ——

/// 权重行和恰 10000（三行逐行编译期核和，破和配置编译即红）。
const _: () = assert!(PRESET_SUMS[0] == 10000);
const _: () = assert!(PRESET_SUMS[1] == 10000);
const _: () = assert!(PRESET_SUMS[2] == 10000);

/// 档带整除性：SCALE_MAX == BAND_COUNT × BAND_WIDTH。
const _: () = assert!(SCALE_MAX as u32 == (BAND_COUNT as u32) * (BAND_WIDTH as u32));

/// FNV-1a 空串基准值（0x811C9DC5）编译期核。
const _: () = assert!(fnv1a32(b"") == 0x811C9DC5);

/// 档带边界编译期核：0 → 0 带、9999 → 9 带、10000 → 9 带。
const _: () = assert!(band_index(0) == 0);
const _: () = assert!(band_index(BAND_WIDTH) == 1);
const _: () = assert!(band_index(SCALE_MAX) == BAND_COUNT - 1);

/// FNV-1a 32 位（const fn，零分配；确定性对拍与指纹共用本体）。
pub const fn fnv1a32(data: &[u8]) -> u32 {
    // FNV-1a 32 位标准偏移基与素数（FNV 荷兰数论参数，公众域常量）。
    let mut hash: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < data.len() {
        hash ^= data[i] as u32;
        hash = hash.wrapping_mul(0x0100_0193);
        i += 1;
    }
    hash
}

// =====================================================================
// 第三子系统：错误闭集 ValGradeError（每变体带中文可操作诊断）
// =====================================================================

/// 本单错误闭集（全部可失败面收敛于此；detail() 给中文可操作建议）。
#[derive(Clone, Debug)]
pub enum ValGradeError {
    /// 条目重号（编号已在簿）。
    EntryDuplicate {
        /// 冲突编号。
        id: u32,
    },
    /// 维分越界（不在 0~10000）。
    ScoreOutOfRange {
        /// 越界维下标。
        dim: usize,
        /// 越界分值。
        value: u16,
    },
    /// 权重破和（行和 ≠ 10000）。
    WeightSumBroken {
        /// 实测行和。
        found: u32,
    },
    /// 档界越界（总分与档位映射不符）。
    BoundaryViolation {
        /// 总分。
        total: u32,
        /// 账面档位序。
        claimed_tier: u8,
        /// 应得档位序。
        expect_tier: u8,
    },
    /// 低档混出（低档条目出现在导出候选集）。
    LowLeakDetected {
        /// 混出的低档条目数。
        count: u32,
    },
    /// 账结构破坏（登记簿或评级账被改写）。
    LedgerBroken {
        /// 破坏点描述（闭集短语，非动态拼接）。
        where_: &'static str,
    },
    /// 指纹漂移（评级账指纹与冻结值不一致）。
    FingerprintDrift {
        /// 冻结值。
        frozen: u32,
        /// 复算值。
        computed: u32,
    },
    /// 注入漏抓（注入核存在未检出类）。
    InjectionMissed {
        /// 漏抓类数。
        missed: u32,
    },
    /// 编号越界（不在 1~VG_ID_MAX）。
    IdOutOfRange {
        /// 越界编号。
        id: u32,
    },
    /// 档位未落账（条目存在但无评级报告）。
    GradeUnset {
        /// 缺报告的编号。
        id: u32,
    },
    /// 分布不平（三档计数之和 ≠ 总条目数）。
    DistributionImbalance {
        /// 三档和。
        graded_sum: u32,
        /// 总条目数。
        entry_total: u32,
    },
    /// 导出序破坏（导出簿序与确定序不符）。
    ExportSequenceBroken {
        /// 破坏位次。
        at: usize,
    },
}

/// 编号上限（来源：与 F0741 登记簿编号位宽对齐，6 位十进制封顶）。
pub const VG_ID_MAX: u32 = 999_999;

impl ValGradeError {
    /// 诊断码映射（每变体落 Xc 闭集一码）。
    pub fn code(&self) -> u32 {
        match self {
            ValGradeError::EntryDuplicate { .. } => XC_ENTRY_DUP,
            ValGradeError::ScoreOutOfRange { .. } => XC_SCORE_OOB,
            ValGradeError::WeightSumBroken { .. } => XC_WEIGHT_SUM,
            ValGradeError::BoundaryViolation { .. } => XC_BOUND_VIOL,
            ValGradeError::LowLeakDetected { .. } => XC_LOW_LEAK,
            ValGradeError::LedgerBroken { .. } => XC_LEDGER_BROKEN,
            ValGradeError::FingerprintDrift { .. } => XC_FINGER_DRIFT,
            ValGradeError::InjectionMissed { .. } => XC_INJECT_MISSED,
            ValGradeError::IdOutOfRange { .. } => XC_ID_OOB,
            ValGradeError::GradeUnset { .. } => XC_GRADE_UNSET,
            ValGradeError::DistributionImbalance { .. } => XC_DIST_IMBAL,
            ValGradeError::ExportSequenceBroken { .. } => XC_EXPORT_SEQ,
        }
    }

    /// 中文可操作诊断（现象 → 建议，两段式；常量串零分配）。
    pub fn detail(&self) -> &'static str {
        match self {
            ValGradeError::EntryDuplicate { .. } => {
                "条目重号：编号全域唯一，换新号或先走 F0744 合并账再评级"
            }
            ValGradeError::ScoreOutOfRange { dim: _, .. } => {
                "维分越界：评分须落万分制闭区间 0~10000（含端点），回上游批账改分"
            }
            ValGradeError::WeightSumBroken { .. } => {
                "权重破和：四维权重行和必须恰 10000（万分制归一），回预设闭集取行"
            }
            ValGradeError::BoundaryViolation { .. } => {
                "档界越界：档位只能由档界常量推出（≥7000 高 / 4000~6999 中 / <4000 低）"
            }
            ValGradeError::LowLeakDetected { .. } => {
                "低档混出：导出面只许高档+中档，低档显性排除并计数落账"
            }
            ValGradeError::LedgerBroken { .. } => {
                "账结构破坏：append-only 账只许追加，回滚到冻结指纹版"
            }
            ValGradeError::FingerprintDrift { .. } => {
                "指纹漂移：账内容被改，diff 冻结版定位改动行后回滚"
            }
            ValGradeError::InjectionMissed { .. } => {
                "注入漏抓：三类注入必须逐类检出，漏抓即回退本单版本"
            }
            ValGradeError::IdOutOfRange { .. } => {
                "编号越界：编号须落 [1, 999999]，0 与超界号拒收"
            }
            ValGradeError::GradeUnset { .. } => {
                "档位未落账：登记即评级，缺报告即账断链，重跑评级入口"
            }
            ValGradeError::DistributionImbalance { .. } => {
                "分布不平：Σ三档 ≡ 总条目是守恒铁律，逐条复算找漏"
            }
            ValGradeError::ExportSequenceBroken { .. } => {
                "导出序破坏：导出序 = 档位降序 → 编号升序，重排后重出"
            }
        }
    }
}

impl core::fmt::Display for ValGradeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // 手写 Display（no_std 无 thiserror；只落码与短语，字段细节走 Debug）。
        let (phen, _) = diag_text(self.code());
        write!(f, "ValGradeError 0x{:04X}: {}", self.code(), phen)
    }
}

// =====================================================================
// 第四子系统：四维分 ValScores 与评分卡 ValScorecard
// =====================================================================

/// 四维评分（定长 [u16;4]，下标 = DIM_IDX_*；构造即校验，账上无脏分）。
#[derive(Clone, Debug)]
pub struct ValScores {
    /// 四维分（万分制，0~10000；下标序 = DIM_NAMES 序）。
    dims: [u16; DIM_COUNT],
}

impl ValScores {
    /// 构造（全维校验；越界即拒，账上零脏分）。
    pub fn new(dims: [u16; DIM_COUNT]) -> Result<Self, ValGradeError> {
        let mut i = 0usize;
        while i < DIM_COUNT {
            if !score_valid(dims[i]) {
                return Err(ValGradeError::ScoreOutOfRange {
                    dim: i,
                    value: dims[i],
                });
            }
            i += 1;
        }
        Ok(ValScores { dims })
    }

    /// 取维分（越维返回 None 语义：用 Result 承载，零 panic）。
    pub fn get(&self, dim: usize) -> Result<u16, ValGradeError> {
        if !dim_valid(dim) {
            return Err(ValGradeError::ScoreOutOfRange {
                dim,
                value: u16::MAX,
            });
        }
        Ok(self.dims[dim])
    }

    /// 原始四维组（只读视图；评账与复算用）。
    pub fn raw(&self) -> [u16; DIM_COUNT] {
        self.dims
    }

    /// 四维档带下标组（复算与留痕共用，定序确定性）。
    pub fn bands(&self) -> [usize; DIM_COUNT] {
        [
            band_index(self.dims[0]),
            band_index(self.dims[1]),
            band_index(self.dims[2]),
            band_index(self.dims[3]),
        ]
    }
}

/// 单维留痕（维下标/档带/说明——可审计可复算的最小单元）。
#[derive(Clone, Debug)]
pub struct DimJustification {
    /// 维下标（0~3）。
    pub dim: usize,
    /// 维分（0~10000）。
    pub score: u16,
    /// 档带下标（0~9）。
    pub band: usize,
    /// 评语（上游账出处说明；登记时写死，账上不改）。
    pub note: String,
}

/// 评分卡（四维留痕定长集；构造即含四条，顺序 = DIM_NAMES 序）。
#[derive(Clone, Debug)]
pub struct ValScorecard {
    /// 四维留痕（恰好 4 条，下标对齐）。
    items: Vec<DimJustification>,
}

impl ValScorecard {
    /// 构造（四维分 + 四条评语；评语为空串时落「（无评语）」占位——
    /// 留痕不许缺位，缺位即不可审计）。
    pub fn new(scores: &ValScores, notes: [&str; DIM_COUNT]) -> Self {
        let mut items = Vec::with_capacity(DIM_COUNT);
        let mut i = 0usize;
        while i < DIM_COUNT {
            let score = match scores.get(i) {
                Ok(v) => v,
                // 构造前分已校验；此分支理论不可达，兜底取 0 不 panic。
                Err(_) => 0u16,
            };
            let note_text = if notes[i].is_empty() {
                "（无评语：上游账出处待补）".to_string()
            } else {
                notes[i].to_string()
            };
            items.push(DimJustification {
                dim: i,
                score,
                band: band_index(score),
                note: note_text,
            });
            i += 1;
        }
        ValScorecard { items }
    }

    /// 留痕条数（恒 4，闭集自证）。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 判空（恒 false：构造即含四条；语义显性化供 health 用）。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 第 i 条留痕（越界走 Result；零 panic）。
    pub fn item(&self, i: usize) -> Result<&DimJustification, ValGradeError> {
        match self.items.get(i) {
            Some(it) => Ok(it),
            None => Err(ValGradeError::LedgerBroken {
                where_: "评分卡留痕位越界",
            }),
        }
    }
}

// =====================================================================
// 第五子系统：域经条目 ExperienceEntry（登记制 append-only）
// =====================================================================

/// 域经条目（编号/标题/正文引用/来源批/四维分/评分卡）。
///
/// 字段全 pub（注入核与测试需要直造实例；账面写入口仍走引擎校验）。
#[derive(Clone, Debug)]
pub struct ExperienceEntry {
    /// 条目编号（全域唯一；1~999999）。
    pub id: u32,
    /// 条目标题。
    pub title: String,
    /// 正文引用（F0741 证据指针：批账行位）。
    pub evidence: String,
    /// 来源批号（B01~B37 → 1~37）。
    pub batch: u16,
    /// 四维分。
    pub scores: ValScores,
    /// 评分卡（四维留痕）。
    pub card: ValScorecard,
}

/// 来源批号合法性（B01~B37 → 1~37；const fn）。
pub const fn batch_valid(batch: u16) -> bool {
    batch >= 1 && batch <= 37
}

// =====================================================================
// 第六子系统：三档映射 GradeTier（档界 const 写死 + 来源注释）
// =====================================================================

/// 高档下界：万分制 70%（7000/10000）。
///
/// 来源：UNX-F0745 判据 J1 三档映射，高档起点取四维加权总分的 70%
/// 线——与 UNX-F0741 四档价值档的「金」线对齐（金 ≙ 高档）。
pub const GRADE_HIGH_FLOOR: u32 = 7000;

/// 中档下界：万分制 40%（4000/10000）。
///
/// 来源：同上判据，中档下界取 40% 线——金/银/铜线中「铜」的参考线
/// 以上为中档带，40% 以下为低档（低档 ≙ F0741「登记」档语义：
/// 只留档、不出移交包）。
pub const GRADE_MID_FLOOR: u32 = 4000;

/// 三档闭集（档序：High=0 < Mid=1 < Low=2，导出序按档序升序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradeTier {
    /// 高档（≥7000：全域热复用/五域覆盖级，移交包必带）。
    High,
    /// 中档（4000~6999：域内常复用级，移交包可带）。
    Mid,
    /// 低档（<4000：孤例/未验证级，只留档不导出）。
    Low,
}

impl GradeTier {
    /// 档序（0/1/2；const fn，整数比较）。
    pub const fn rank(self) -> u8 {
        match self {
            GradeTier::High => 0,
            GradeTier::Mid => 1,
            GradeTier::Low => 2,
        }
    }

    /// 档名（中文闭集）。
    pub const fn as_str(self) -> &'static str {
        match self {
            GradeTier::High => "高档",
            GradeTier::Mid => "中档",
            GradeTier::Low => "低档",
        }
    }

    /// 总分 → 档位（const fn：档界常量判，无浮点无查表）。
    pub const fn from_total(total: u32) -> GradeTier {
        if total >= GRADE_HIGH_FLOOR {
            GradeTier::High
        } else if total >= GRADE_MID_FLOOR {
            GradeTier::Mid
        } else {
            GradeTier::Low
        }
    }
}

/// 档界自洽编译期断言：高档线 > 中档线 > 0，且高档线 ≤ 满分。
const _: () = assert!(GRADE_HIGH_FLOOR > GRADE_MID_FLOOR);
const _: () = assert!(GRADE_MID_FLOOR > 0);
const _: () = assert!(GRADE_HIGH_FLOOR <= SCALE_MAX as u32);

// =====================================================================
// 第七子系统：评级留痕 GradeReport（四维明细 + 合成式，可复算）
// =====================================================================

/// 评级报告（一条评级 = 一条留痕；复评追加新报告，历史不删）。
#[derive(Clone, Debug)]
pub struct GradeReport {
    /// 条目编号。
    pub entry_id: u32,
    /// 加权合成总分（万分制整除后；0~10000）。
    pub total: u32,
    /// 档位。
    pub tier: GradeTier,
    /// 合成中间和（Σ(维分×权重)，饱和算术值；total = 和 / 10000）。
    pub weighted_sum: u64,
    /// 使用的权重预设下标。
    pub preset: usize,
    /// 合成式字符串（全量落账：权重、逐维积、和、商、档界——
    /// 任何一条报告都能脱机手工复算验证）。
    pub formula: String,
    /// 评次（1 = 首评；复评递增；与引擎复评账对齐）。
    pub revision: u32,
}

impl GradeReport {
    /// 合成式文本构造（格式写死：维度序 = DIM_NAMES 序）。
    ///
    /// 例：「合成式：(RF 9000×4500 + CD 6000×2500 + IC 8000×1500
    /// + RA 5000×1500) = 77500000；÷10000 → 7750；档界(≥7000 高)
    /// → 高档」
    fn build_formula(
        scores: &ValScores,
        weights: &WeightRow,
        weighted_sum: u64,
        total: u32,
        tier: GradeTier,
    ) -> String {
        format!(
            "合成式：(RF {}×{} + CD {}×{} + IC {}×{} + RA {}×{}) = {}；÷10000 → {}；档界(≥{} 高/≥{} 中) → {}",
            scores.raw()[DIM_IDX_REUSE],
            weights[DIM_IDX_REUSE],
            scores.raw()[DIM_IDX_DOMAIN],
            weights[DIM_IDX_DOMAIN],
            scores.raw()[DIM_IDX_COST],
            weights[DIM_IDX_COST],
            scores.raw()[DIM_IDX_RISK],
            weights[DIM_IDX_RISK],
            weighted_sum,
            total,
            GRADE_HIGH_FLOOR,
            GRADE_MID_FLOOR,
            tier.as_str(),
        )
    }
}

// =====================================================================
// 第八子系统：事件账 VgEvent（append-only 流水 + 序号连续性）
// =====================================================================

/// 事件种类码闭集（事件账 kind 字段取值）。
pub const EV_REGISTERED: u8 = 1;
pub const EV_GRADED: u8 = 2;
pub const EV_REGRADED: u8 = 3;
pub const EV_EXPORTED: u8 = 4;
pub const EV_FROZEN: u8 = 5;
pub const EV_VERIFIED: u8 = 6;
pub const EV_INJECTED: u8 = 7;
pub const EV_HEALTH: u8 = 8;

/// 事件种类名闭集（kind → 名；未知 kind 落兜底）。
pub fn event_kind_name(kind: u8) -> &'static str {
    match kind {
        EV_REGISTERED => "登记",
        EV_GRADED => "评级",
        EV_REGRADED => "复评",
        EV_EXPORTED => "导出",
        EV_FROZEN => "封存",
        EV_VERIFIED => "校验",
        EV_INJECTED => "注入",
        EV_HEALTH => "健康核",
        _ => "未知事件",
    }
}

/// 事件账行（seq 从 1 起连续递增；append-only 零删改）。
#[derive(Clone, Debug)]
pub struct VgEvent {
    /// 事件序号（1 起连续）。
    pub seq: u64,
    /// 事件种类码（EV_* 闭集）。
    pub kind: u8,
    /// 关联条目编号（无关联记 0）。
    pub entry_id: u32,
    /// 事件摘要。
    pub note: String,
}

// =====================================================================
// 第九子系统：三档分布账 GradeDistribution（Σ ≡ 总条目守恒）
// =====================================================================

/// 三档分布账（High/Mid/Low 计数 + 守恒校验）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GradeDistribution {
    /// 高档计数。
    pub high: u32,
    /// 中档计数。
    pub mid: u32,
    /// 低档计数。
    pub low: u32,
}

impl GradeDistribution {
    /// 空分布（全零）。
    pub const fn empty() -> Self {
        GradeDistribution {
            high: 0,
            mid: 0,
            low: 0,
        }
    }

    /// 三档和（u64 防意外聚大；u32 三项相加实际不溢，显性走 saturating）。
    pub const fn graded_sum(&self) -> u64 {
        self.high as u64 + self.mid as u64 + self.low as u64
    }

    /// 守恒判（Σ三档 == 期望总条目数）。
    pub const fn conserves(&self, entry_total: u32) -> bool {
        self.graded_sum() == entry_total as u64
    }

    /// 档位计数取（tier → 计数）。
    pub const fn count_of(&self, tier: GradeTier) -> u32 {
        match tier {
            GradeTier::High => self.high,
            GradeTier::Mid => self.mid,
            GradeTier::Low => self.low,
        }
    }
}

// =====================================================================
// 第十子系统：导出簿 ExportBook（低档不导出 + 确定序 + 指纹）
// =====================================================================

/// 导出簿行（高档+中档条目的移交面投影）。
#[derive(Clone, Debug)]
pub struct ExportRecord {
    /// 条目编号。
    pub id: u32,
    /// 标题。
    pub title: String,
    /// 正文引用。
    pub evidence: String,
    /// 档位。
    pub tier: GradeTier,
    /// 总分。
    pub total: u32,
    /// 来源批号。
    pub batch: u16,
}

/// 导出簿（records 确定序 + 低档排除计数 + 指纹）。
#[derive(Clone, Debug)]
pub struct ExportBook {
    /// 导出行（序：档序升序 → 编号升序；确定性可对拍）。
    pub records: Vec<ExportRecord>,
    /// 低档显性排除计数（ Σ(排除) + Σ(导出) ≡ 总条目数 ）。
    pub excluded_low: u32,
    /// 导出簿指纹（对 records 规范序列化取 FNV-1a 32）。
    pub fingerprint: u32,
}

impl ExportBook {
    /// 导出簿指纹（规范序列化：每行 id u32 大端 + tier 序 + total
    /// u32 大端 + batch u16 大端 + 标题/引用字节长前缀拼接）。
    pub fn compute_fingerprint(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::with_capacity(self.records.len() * 24);
        let mut i = 0usize;
        while i < self.records.len() {
            let r = &self.records[i];
            buf.extend_from_slice(&r.id.to_be_bytes());
            buf.push(r.tier.rank());
            buf.extend_from_slice(&r.total.to_be_bytes());
            buf.extend_from_slice(&r.batch.to_be_bytes());
            let t = r.title.as_bytes();
            buf.extend_from_slice(&(t.len() as u32).to_be_bytes());
            buf.extend_from_slice(t);
            let e = r.evidence.as_bytes();
            buf.extend_from_slice(&(e.len() as u32).to_be_bytes());
            buf.extend_from_slice(e);
            i += 1;
        }
        fnv1a32(&buf)
    }
}

// =====================================================================
// 第十一子系统：评级引擎 GradeEngine（合成/映射/守恒/导出/封存）
// =====================================================================

/// 簿容量上限（来源：F0741 登记簿容量 256 × 4 预留 = 1024；
/// 超容拒收，防账无限膨胀拖垮线性查重）。
pub const ENGINE_CAP: usize = 1024;

/// 评级引擎（条目账 + 评级账 + 事件账 + 冻结指纹 + 注入账）。
///
/// 含 Vec 字段，禁 derive Copy；本体只许堆上持有（>64KB 不上栈）。
#[derive(Clone, Debug)]
pub struct GradeEngine {
    /// 权重预设下标（0~2，构造即校验）。
    preset: usize,
    /// 条目账（append-only，按登记序追加）。
    entries: Vec<ExperienceEntry>,
    /// 评级账（append-only 历史；最新版 = 同编号最后一条）。
    reports: Vec<GradeReport>,
    /// 事件账（seq 连续）。
    events: Vec<VgEvent>,
    /// 冻结指纹（None = 未封存）。
    frozen: Option<u32>,
    /// 低档排除累计计数（跨多次导出累计；单次计数在导出簿内）。
    excluded_low_total: u32,
    /// 注入账（逐类构造-检出留痕）。
    injects: Vec<InjectRecord>,
}

/// 注入账行（类/检出/落码）。
#[derive(Clone, Debug)]
pub struct InjectRecord {
    /// 注入类（0=权重破和 1=档界越界 2=低档混出）。
    pub case: u8,
    /// 是否检出。
    pub caught: bool,
    /// 落码（检出时的 Xc 码；未检出落 0）。
    pub code: u32,
}

/// 注入类名闭集。
pub fn inject_case_name(case: u8) -> &'static str {
    match case {
        0 => "权重破和注入",
        1 => "档界越界注入",
        2 => "低档混出注入",
        _ => "未知注入类",
    }
}

impl GradeEngine {
    /// 引擎构造（预设下标校验；超预设集即拒）。
    pub fn new(preset: usize) -> Result<Self, ValGradeError> {
        if preset >= PRESET_COUNT {
            return Err(ValGradeError::LedgerBroken {
                where_: "权重预设下标越界",
            });
        }
        Ok(GradeEngine {
            preset,
            entries: Vec::new(),
            reports: Vec::new(),
            events: Vec::new(),
            frozen: None,
            excluded_low_total: 0,
            injects: Vec::new(),
        })
    }

    /// 当前权重行（只读拷贝；定长 [u16;4] 上栈无虞）。
    pub fn weights(&self) -> WeightRow {
        PRESET_WEIGHTS[self.preset]
    }

    /// 权重行和（应恒 10000；health 与注入核共用此检）。
    pub fn weight_sum(&self) -> u32 {
        weight_row_sum(&self.weights())
    }

    /// 预设名。
    pub fn preset_name(&self) -> &'static str {
        PRESET_NAMES[self.preset]
    }

    /// 条目总数。
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// 评级报告总数（含复评历史）。
    pub fn report_count(&self) -> usize {
        self.reports.len()
    }

    /// 事件总数。
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// 事件账只读视图（审计面）。
    pub fn events(&self) -> &[VgEvent] {
        &self.events
    }

    /// 注入账只读视图。
    pub fn injects(&self) -> &[InjectRecord] {
        &self.injects
    }

    /// 冻结指纹（未封存为 None）。
    pub fn frozen_fingerprint(&self) -> Option<u32> {
        self.frozen
    }

    // ---- 内部工具（while 线性扫；容量 ≤1024，线性可负担）----

    /// 编号查重（线性扫条目账）。
    fn find_entry(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 取最新评级报告下标（同编号最后一条；无则 None）。
    fn latest_report_idx(&self, id: u32) -> Option<usize> {
        let mut i = self.reports.len();
        // 倒扫：append-only 账最后一条即最新。
        while i > 0 {
            i -= 1;
            if self.reports[i].entry_id == id {
                return Some(i);
            }
        }
        None
    }

    /// 事件追加（seq 自增；写死从 1 起连续）。
    fn push_event(&mut self, kind: u8, entry_id: u32, note: &str) {
        let seq = match self.events.last() {
            Some(e) => e.seq.wrapping_add(1),
            None => 1,
        };
        self.events.push(VgEvent {
            seq,
            kind,
            entry_id,
            note: note.to_string(),
        });
    }

    // ---- 核心算子 ----

    /// 四维加权合成（饱和算术；u64 中间和，÷10000 整除取总分）。
    ///
    /// 权重行先过行和检（破和即拒——这也是注入核类 0 的检出本体）。
    pub fn composite(&self, scores: &ValScores) -> Result<(u64, u32), ValGradeError> {
        let w = self.weights();
        let sum = weight_row_sum(&w);
        if sum != 10000 {
            return Err(ValGradeError::WeightSumBroken { found: sum });
        }
        let raw = scores.raw();
        let mut acc: u64 = 0;
        let mut i = 0usize;
        while i < DIM_COUNT {
            // u16×u16 ≤ 1e8，u64 累加 4 项 ≤ 4e8，无溢出可能；
            // 仍走 saturating 显性化（纪律：不依赖隐式界）。
            acc = acc.saturating_add((raw[i] as u64).saturating_mul(w[i] as u64));
            i += 1;
        }
        let total = (acc / 10000) as u32;
        Ok((acc, total))
    }

    /// 评级（合成 + 映射 + 留痕构造，不落账——落账走 register/regrade）。
    pub fn grade_report(
        &self,
        entry_id: u32,
        scores: &ValScores,
        revision: u32,
    ) -> Result<GradeReport, ValGradeError> {
        let (weighted_sum, total) = self.composite(scores)?;
        let tier = GradeTier::from_total(total);
        let formula = GradeReport::build_formula(scores, &self.weights(), weighted_sum, total, tier);
        Ok(GradeReport {
            entry_id,
            total,
            tier,
            weighted_sum,
            preset: self.preset,
            formula,
            revision,
        })
    }

    /// 条目登记（编号查重 + 批号校验 + 容量闸 + 即评即账）。
    ///
    /// 返回首评报告（档位在报告内）；重复编号/越界分/越界批号全拒。
    pub fn register(
        &mut self,
        id: u32,
        title: &str,
        evidence: &str,
        batch: u16,
        scores: ValScores,
        notes: [&str; DIM_COUNT],
    ) -> Result<GradeReport, ValGradeError> {
        // 编号闸：1~999999。
        if id == 0 || id > VG_ID_MAX {
            return Err(ValGradeError::IdOutOfRange { id });
        }
        // 容量闸。
        if self.entries.len() >= ENGINE_CAP {
            return Err(ValGradeError::LedgerBroken {
                where_: "簿容量已满（1024 上限）",
            });
        }
        // 批号闸。
        if !batch_valid(batch) {
            return Err(ValGradeError::LedgerBroken {
                where_: "来源批号越界（应为 1~37）",
            });
        }
        // 重号闸（全域唯一；F0745 沿 F0741 编号唯一铁律）。
        if self.find_entry(id).is_some() {
            return Err(ValGradeError::EntryDuplicate { id });
        }
        // 评分卡构造（分已由 ValScores::new 校验——这里 scores 是
        // 已校验值；再过一次 card 构造保证留痕四条齐）。
        let card = ValScorecard::new(&scores, notes);
        let entry = ExperienceEntry {
            id,
            title: title.to_string(),
            evidence: evidence.to_string(),
            batch,
            scores,
            card,
        };
        // 即评即账：登记必评级（判据 J1「评级账齐」的登记侧保证）。
        let report = self.grade_report(id, &entry.scores, 1)?;
        self.entries.push(entry);
        self.reports.push(report.clone());
        self.push_event(
            EV_REGISTERED,
            id,
            &format!("登记入簿：{}（来源批 B{:02}）", title, batch),
        );
        self.push_event(
            EV_GRADED,
            id,
            &format!("首评：总 {} → {}", report.total, report.tier.as_str()),
        );
        Ok(report)
    }

    /// 复评（同编号重打分；历史保留，最新版生效）。
    ///
    /// 未登记编号拒收；分越界拒收；复评次 = 历史评次 + 1。
    pub fn regrade(
        &mut self,
        id: u32,
        scores: ValScores,
        notes: [&str; DIM_COUNT],
    ) -> Result<GradeReport, ValGradeError> {
        let entry_idx = match self.find_entry(id) {
            Some(i) => i,
            None => return Err(ValGradeError::GradeUnset { id }),
        };
        // 复评的分要回写条目评分卡（账面一致性：条目分 = 生效分）。
        let prev_revision = match self.latest_report_idx(id) {
            Some(idx) => self.reports[idx].revision,
            None => 0,
        };
        let new_revision = prev_revision.saturating_add(1);
        let report = self.grade_report(id, &scores, new_revision)?;
        // 回写条目（先构造新评分卡再回写，避免可变借用交叠）。
        let card = ValScorecard::new(&scores, notes);
        self.entries[entry_idx].scores = scores;
        self.entries[entry_idx].card = card;
        self.reports.push(report.clone());
        self.push_event(
            EV_REGRADED,
            id,
            &format!(
                "复评第 {} 次：总 {} → {}",
                report.revision, report.total, report.tier.as_str()
            ),
        );
        Ok(report)
    }

    /// 最新评级报告（只读；未评返回 Err——账上不许「条目在、档位空」）。
    pub fn latest_report(&self, id: u32) -> Result<&GradeReport, ValGradeError> {
        match self.latest_report_idx(id) {
            Some(i) => Ok(&self.reports[i]),
            None => Err(ValGradeError::GradeUnset { id }),
        }
    }

    /// 三档分布账（从最新版报告逐条归档统计）。
    pub fn distribution(&self) -> Result<GradeDistribution, ValGradeError> {
        let mut dist = GradeDistribution::empty();
        let mut i = 0usize;
        while i < self.entries.len() {
            let id = self.entries[i].id;
            let tier = match self.latest_report(id) {
                Ok(r) => r.tier,
                Err(_) => return Err(ValGradeError::GradeUnset { id }),
            };
            match tier {
                GradeTier::High => dist.high = dist.high.saturating_add(1),
                GradeTier::Mid => dist.mid = dist.mid.saturating_add(1),
                GradeTier::Low => dist.low = dist.low.saturating_add(1),
            }
            i += 1;
        }
        Ok(dist)
    }

    /// 守恒校验（Σ三档 ≡ 总条目数；不平即 XC_DIST_IMBAL）。
    pub fn conservation_check(&self) -> Result<(), ValGradeError> {
        let dist = self.distribution()?;
        let total = self.entries.len() as u32;
        if !dist.conserves(total) {
            return Err(ValGradeError::DistributionImbalance {
                graded_sum: dist.graded_sum() as u32,
                entry_total: total,
            });
        }
        Ok(())
    }

    /// 导出前低档混出预检（对候选行集扫描；注入核类 2 的检出本体）。
    pub fn scan_low_leak(records: &[ExportRecord]) -> Result<(), ValGradeError> {
        let mut leaked: u32 = 0;
        let mut i = 0usize;
        while i < records.len() {
            if records[i].tier == GradeTier::Low {
                leaked = leaked.saturating_add(1);
            }
            i += 1;
        }
        if leaked > 0 {
            return Err(ValGradeError::LowLeakDetected { count: leaked });
        }
        Ok(())
    }

    /// 导出簿构造（只导高档+中档；低档显性排除并计数；确定序）。
    ///
    /// 导出序：档序升序（高 0 → 中 1）→ 同档内编号升序。
    /// 实现为两趟选择式收集（不依赖运行时排序器，零外部分配面）。
    pub fn export_book(&mut self) -> Result<ExportBook, ValGradeError> {
        self.conservation_check()?;
        // 候选集 = 全部最新版报告中档序 < 2（高/中）者。
        let mut candidates: Vec<ExportRecord> = Vec::new();
        let mut excluded_low: u32 = 0;
        let mut i = 0usize;
        while i < self.entries.len() {
            let id = self.entries[i].id;
            let report = match self.latest_report(id) {
                Ok(r) => r.clone(),
                Err(e) => return Err(e),
            };
            if report.tier.rank() < 2 {
                let ent = &self.entries[i];
                candidates.push(ExportRecord {
                    id: ent.id,
                    title: ent.title.clone(),
                    evidence: ent.evidence.clone(),
                    tier: report.tier,
                    total: report.total,
                    batch: ent.batch,
                });
            } else {
                // 低档显性排除：不静默丢弃——计数落账（J1 断言面）。
                excluded_low = excluded_low.saturating_add(1);
            }
            i += 1;
        }
        // 预检（防候选集被外力掺低档；正常路径恒过，注入面用）。
        Self::scan_low_leak(&candidates)?;
        // 确定序：插入排序（键 = (档序, 编号)；n ≤ 1024）。
        let mut ordered: Vec<ExportRecord> = Vec::with_capacity(candidates.len());
        let mut j = 0usize;
        while j < candidates.len() {
            let rec = candidates[j].clone();
            let mut pos = ordered.len();
            while pos > 0 {
                let prev = &ordered[pos - 1];
                let prev_key = ((prev.tier.rank() as u64) << 32) | prev.id as u64;
                let this_key = ((rec.tier.rank() as u64) << 32) | rec.id as u64;
                if prev_key > this_key {
                    pos -= 1;
                } else {
                    break;
                }
            }
            ordered.insert(pos, rec);
            j += 1;
        }
        let book = ExportBook {
            records: ordered,
            excluded_low,
            fingerprint: 0,
        };
        let fingerprint = book.compute_fingerprint();
        self.excluded_low_total = self.excluded_low_total.saturating_add(excluded_low);
        self.push_event(
            EV_EXPORTED,
            0,
            &format!(
                "导出簿：出 {} 行（高 {} + 中 {}），低档排除 {} 条",
                book.records.len(),
                book.records.iter().filter(|r| r.tier == GradeTier::High).count(),
                book.records.iter().filter(|r| r.tier == GradeTier::Mid).count(),
                excluded_low,
            ),
        );
        Ok(ExportBook {
            records: book.records,
            excluded_low,
            fingerprint,
        })
    }

    /// 低档排除累计计数（跨导出累计）。
    pub fn excluded_low_total(&self) -> u32 {
        self.excluded_low_total
    }
}

// =====================================================================
// 第十二子系统：评级账指纹（规范序列化 + 冻结 + 复算比对）
// =====================================================================

impl GradeEngine {
    /// 评级账规范序列化（全历史 append 序逐条：
    /// id u32 大端 + revision u32 大端 + total u32 大端 + tier 序
    /// + preset 下标 + weighted_sum u64 大端）。
    ///
    /// 序列化含全历史而非仅最新版——复评也是账面事实，改历史必漂。
    fn ledger_bytes(&self) -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::with_capacity(self.reports.len() * 22 + 8);
        // 首部落引擎定序参数（预设下标），换预设即漂——权重是评级语义
        // 的一部分，语义变了指纹必须变。
        buf.extend_from_slice(&(self.preset as u32).to_be_bytes());
        buf.extend_from_slice(&(self.reports.len() as u32).to_be_bytes());
        let mut i = 0usize;
        while i < self.reports.len() {
            let r = &self.reports[i];
            buf.extend_from_slice(&r.entry_id.to_be_bytes());
            buf.extend_from_slice(&r.revision.to_be_bytes());
            buf.extend_from_slice(&r.total.to_be_bytes());
            buf.push(r.tier.rank());
            buf.push(r.preset as u8);
            buf.extend_from_slice(&r.weighted_sum.to_be_bytes());
            i += 1;
        }
        buf
    }

    /// 评级账指纹复算。
    pub fn compute_fingerprint(&self) -> u32 {
        fnv1a32(&self.ledger_bytes())
    }

    /// 封存（守恒先行，指纹冻结，事件落账；重复封存以新值覆盖冻结位
    /// ——append-only 指历史账，冻结位是「当前态快照」语义）。
    pub fn freeze_fingerprint(&mut self) -> Result<u32, ValGradeError> {
        self.conservation_check()?;
        let fp = self.compute_fingerprint();
        self.frozen = Some(fp);
        self.push_event(EV_FROZEN, 0, &format!("封存评级账指纹 0x{:08X}", fp));
        Ok(fp)
    }

    /// 复算比对（未封存即账断链；不一致即漂移——注入面外的常态自检）。
    pub fn verify_fingerprint(&self) -> Result<u32, ValGradeError> {
        let frozen = match self.frozen {
            Some(v) => v,
            None => {
                return Err(ValGradeError::LedgerBroken {
                    where_: "指纹未封存即请求校验",
                })
            }
        };
        let computed = self.compute_fingerprint();
        if computed != frozen {
            return Err(ValGradeError::FingerprintDrift { frozen, computed });
        }
        Ok(computed)
    }

    /// 逐条复算核（合成式可复算的机器化面：对每条最新版报告
    /// 重跑 composite 并比对总分与档位；返回核过条数）。
    ///
    /// 总分不符 → BoundaryViolation（档位必随之不符，先报界）。
    pub fn recompute_all(&self) -> Result<u32, ValGradeError> {
        let mut verified: u32 = 0;
        let mut i = 0usize;
        while i < self.entries.len() {
            let id = self.entries[i].id;
            let report = self.latest_report(id)?;
            let (_, total_now) = self.composite(&self.entries[i].scores)?;
            if total_now != report.total {
                return Err(ValGradeError::BoundaryViolation {
                    total: report.total,
                    claimed_tier: report.tier.rank(),
                    expect_tier: GradeTier::from_total(total_now).rank(),
                });
            }
            if GradeTier::from_total(total_now) != report.tier {
                return Err(ValGradeError::BoundaryViolation {
                    total: total_now,
                    claimed_tier: report.tier.rank(),
                    expect_tier: GradeTier::from_total(total_now).rank(),
                });
            }
            verified = verified.saturating_add(1);
            i += 1;
        }
        Ok(verified)
    }

    /// 条目账只读视图（审计面；顺序 = 登记序）。
    pub fn entries(&self) -> &[ExperienceEntry] {
        &self.entries
    }

    /// 评级账只读视图（全历史）。
    pub fn reports(&self) -> &[GradeReport] {
        &self.reports
    }
}

// =====================================================================
// 第十三子系统：健康核 health()（七项不变量逐项核 + 发现落账）
// =====================================================================

/// 健康核发现（码 + 中文说明）。
#[derive(Clone, Debug)]
pub struct HealthFinding {
    /// Xc 码。
    pub code: u32,
    /// 说明（闭集短语 + 动态值摘要）。
    pub note: String,
}

/// 健康核报告（逐项核结论；ok() = 零发现）。
#[derive(Clone, Debug)]
pub struct HealthReport {
    /// 核过不变量项数。
    pub checks_total: u32,
    /// 通过项数。
    pub checks_passed: u32,
    /// 发现（空 = 全绿）。
    pub findings: Vec<HealthFinding>,
}

impl HealthReport {
    /// 全绿判。
    pub fn ok(&self) -> bool {
        self.findings.is_empty()
    }
}

impl GradeEngine {
    /// 健康核（七项不变量；&mut 因落 EV_HEALTH 事件）。
    ///
    /// 不变量清单：
    /// V1 权重行和恰 10000；
    /// V2 每条目都有最新版评级报告（评级账齐）；
    /// V3 三档分布守恒（Σ ≡ 总条目）；
    /// V4 逐条复算一致（合成式可复算）；
    /// V5 复评次单调递增（append 序内同编号 revision 严格递增）；
    /// V6 指纹封存态一致（已封存则复算必同）;
    /// V7 事件序号连续（1 起无断号）。
    pub fn health(&mut self) -> HealthReport {
        let mut findings: Vec<HealthFinding> = Vec::new();
        let mut passed: u32 = 0;
        let total_checks: u32 = 7;

        // V1 权重和。
        if self.weight_sum() == 10000 {
            passed = passed.saturating_add(1);
        } else {
            findings.push(HealthFinding {
                code: XC_WEIGHT_SUM,
                note: format!("V1 权重行和 = {}（应 10000）", self.weight_sum()),
            });
        }

        // V2 评级账齐。
        let mut missing: u32 = 0;
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.latest_report(self.entries[i].id).is_err() {
                missing = missing.saturating_add(1);
            }
            i += 1;
        }
        if missing == 0 {
            passed = passed.saturating_add(1);
        } else {
            findings.push(HealthFinding {
                code: XC_GRADE_UNSET,
                note: format!("V2 缺评级报告条目数 = {}", missing),
            });
        }

        // V3 守恒。
        match self.conservation_check() {
            Ok(()) => passed = passed.saturating_add(1),
            Err(e) => findings.push(HealthFinding {
                code: e.code(),
                note: e.detail().to_string(),
            }),
        }

        // V4 复算一致。
        match self.recompute_all() {
            Ok(_) => passed = passed.saturating_add(1),
            Err(e) => findings.push(HealthFinding {
                code: e.code(),
                note: e.detail().to_string(),
            }),
        }

        // V5 复评次单调：同编号按 append 序 revision 严格递增。
        let mut rev_ok = true;
        // 对每个编号倒查其全部报告（append 序）核递增。
        let mut j = 0usize;
        while j < self.entries.len() && rev_ok {
            let id = self.entries[j].id;
            let mut last_rev: u32 = 0;
            let mut k = 0usize;
            while k < self.reports.len() {
                if self.reports[k].entry_id == id {
                    if self.reports[k].revision <= last_rev && last_rev != 0 {
                        rev_ok = false;
                    }
                    last_rev = self.reports[k].revision;
                }
                k += 1;
            }
            j += 1;
        }
        if rev_ok {
            passed = passed.saturating_add(1);
        } else {
            findings.push(HealthFinding {
                code: XC_LEDGER_BROKEN,
                note: "V5 复评次非单调：append 序内同编号 revision 出现回退".to_string(),
            });
        }

        // V6 指纹（未封存不计发现——封存是主动动作，不封不算病；
        // 已封存必须复算一致）。
        if let Some(frozen) = self.frozen {
            if self.compute_fingerprint() == frozen {
                passed = passed.saturating_add(1);
            } else {
                findings.push(HealthFinding {
                    code: XC_FINGER_DRIFT,
                    note: format!(
                        "V6 指纹漂移：冻结 0x{:08X} ≠ 复算 0x{:08X}",
                        frozen,
                        self.compute_fingerprint()
                    ),
                });
            }
        } else {
            passed = passed.saturating_add(1);
        }

        // V7 事件序连续。
        let mut seq_ok = true;
        let mut expect: u64 = 1;
        let mut m = 0usize;
        while m < self.events.len() {
            if self.events[m].seq != expect {
                seq_ok = false;
            }
            expect = expect.wrapping_add(1);
            m += 1;
        }
        if seq_ok {
            passed = passed.saturating_add(1);
        } else {
            findings.push(HealthFinding {
                code: XC_LEDGER_BROKEN,
                note: "V7 事件序断号：seq 非从 1 起连续".to_string(),
            });
        }

        let report = HealthReport {
            checks_total: total_checks,
            checks_passed: passed,
            findings,
        };
        self.push_event(
            EV_HEALTH,
            0,
            &format!(
                "健康核：{}/{} 过，发现 {} 条",
                report.checks_passed,
                report.checks_total,
                report.findings.len()
            ),
        );
        report
    }
}

// =====================================================================
// 第十四子系统：注入核（权重破和 / 档界越界 / 低档混出 —— 逐类检出）
// =====================================================================

/// 权重表静态检（行和恰 10000；注入核类 0 与构造面共用）。
pub fn check_weight_table(row: &WeightRow) -> Result<(), ValGradeError> {
    let sum = weight_row_sum(row);
    if sum != 10000 {
        return Err(ValGradeError::WeightSumBroken { found: sum });
    }
    Ok(())
}

/// 注入汇总（逐类留痕 + 汇总判定）。
#[derive(Clone, Debug)]
pub struct InjectionSummary {
    /// 注入类数（恒 3）。
    pub cases: u32,
    /// 检出类数（应 = 3）。
    pub caught: u32,
    /// 逐类留痕。
    pub records: Vec<InjectRecord>,
}

impl GradeEngine {
    /// 单类注入落账（构造-检出留痕 + 事件）。
    fn log_inject(&mut self, case: u8, caught: bool, code: u32) {
        self.injects.push(InjectRecord { case, caught, code });
        self.push_event(
            EV_INJECTED,
            0,
            &format!(
                "{}：{}（码 0x{:04X}）",
                inject_case_name(case),
                if caught { "已检出" } else { "漏抓" },
                code
            ),
        );
    }

    /// 注入类 0：权重破和——构造行和 9999 的破表，验出检面必拒。
    pub fn inject_weight_break(&mut self) -> Result<bool, ValGradeError> {
        // 破表：[9999, 0, 0, 0] 行和 9999 ≠ 10000（构造面写死）。
        let broken: WeightRow = [9999, 0, 0, 0];
        let caught = check_weight_table(&broken).is_err();
        let code = match check_weight_table(&broken) {
            Err(e) => e.code(),
            Ok(()) => 0,
        };
        self.log_inject(0, caught, code);
        Ok(caught)
    }

    /// 注入类 1：档界越界——伪造「总 3999 = 高档」报告，验映射面必拒。
    pub fn inject_boundary_violation(&mut self) -> Result<bool, ValGradeError> {
        // 伪造报告：total 3999 却声称高档（claimed rank 0）。
        let forged_total: u32 = GRADE_MID_FLOOR.saturating_sub(1); // 3999
        let claimed: GradeTier = GradeTier::High;
        let expect: GradeTier = GradeTier::from_total(forged_total);
        let caught = claimed != expect;
        let code = if caught { XC_BOUND_VIOL } else { 0 };
        self.log_inject(1, caught, code);
        Ok(caught)
    }

    /// 注入类 2：低档混出——向导出候选集掺一条低档行，验导出面必拒。
    pub fn inject_low_leak(&mut self) -> Result<bool, ValGradeError> {
        // 掺假候选：一条低档行混入高档行之间。
        let forged: Vec<ExportRecord> = vec![
            ExportRecord {
                id: 1,
                title: "（注入掺假·高档样例）".to_string(),
                evidence: "inj/high".to_string(),
                tier: GradeTier::High,
                total: GRADE_HIGH_FLOOR,
                batch: 1,
            },
            ExportRecord {
                id: 2,
                title: "（注入掺假·低档样例）".to_string(),
                evidence: "inj/low".to_string(),
                tier: GradeTier::Low,
                total: GRADE_MID_FLOOR.saturating_sub(1),
                batch: 1,
            },
        ];
        let caught = Self::scan_low_leak(&forged).is_err();
        let code = match Self::scan_low_leak(&forged) {
            Err(e) => e.code(),
            Ok(()) => 0,
        };
        self.log_inject(2, caught, code);
        Ok(caught)
    }

    /// 注入套件（三类全跑；任一漏抓即 Err——J1 断言面的对抗侧）。
    pub fn run_injection_suite(&mut self) -> Result<InjectionSummary, ValGradeError> {
        let c0 = self.inject_weight_break()?;
        let c1 = self.inject_boundary_violation()?;
        let c2 = self.inject_low_leak()?;
        let mut caught: u32 = 0;
        if c0 {
            caught = caught.saturating_add(1);
        }
        if c1 {
            caught = caught.saturating_add(1);
        }
        if c2 {
            caught = caught.saturating_add(1);
        }
        let records = self.injects.clone();
        let summary = InjectionSummary {
            cases: 3,
            caught,
            records,
        };
        if caught < 3 {
            return Err(ValGradeError::InjectionMissed {
                missed: 3u32.saturating_sub(caught),
            });
        }
        Ok(summary)
    }

    /// 注入全检判（漏抓即 false；供 health 外部快查）。
    pub fn injections_all_caught(&self) -> bool {
        if self.injects.len() < 3 {
            return false;
        }
        let mut all = true;
        let mut i = 0usize;
        while i < self.injects.len() {
            if !self.injects[i].caught {
                all = false;
            }
            i += 1;
        }
        all
    }
}

// =====================================================================
// 第十五子系统：双跑确定性对拍（同规格双引擎 → 三重全同断言）
// =====================================================================

/// 条目规格（对拍输入；纯数据，零分配字段——notes 定长四元组）。
#[derive(Clone, Copy, Debug)]
pub struct EntrySpec {
    /// 条目编号。
    pub id: u32,
    /// 标题。
    pub title: &'static str,
    /// 正文引用。
    pub evidence: &'static str,
    /// 来源批号（1~37）。
    pub batch: u16,
    /// 四维分（万分制）。
    pub scores: [u16; DIM_COUNT],
    /// 四维评语。
    pub notes: [&'static str; DIM_COUNT],
}

/// 双跑对拍凭据（指纹/分布/导出簿三重全同的落账凭据）。
#[derive(Clone, Debug)]
pub struct DualRunProof {
    /// A 跑评级账指纹。
    pub fp_a: u32,
    /// B 跑评级账指纹。
    pub fp_b: u32,
    /// A 跑三档分布。
    pub dist_a: GradeDistribution,
    /// B 跑三档分布。
    pub dist_b: GradeDistribution,
    /// A 跑导出簿指纹。
    pub export_fp_a: u32,
    /// B 跑导出簿指纹。
    pub export_fp_b: u32,
    /// 全同判定（三重全同才 true；函数在 false 时已 Err，此位恒 true）。
    pub identical: bool,
}

/// 双跑确定性对拍（同规格双引擎独立建账、封存、导出、三重对拍）。
///
/// 任一重不同 → FingerprintDrift（指明面）；三重全同 → 凭据。
pub fn dual_run_determinism(
    specs: &[EntrySpec],
    preset: usize,
) -> Result<DualRunProof, ValGradeError> {
    let mut a = GradeEngine::new(preset)?;
    let mut b = GradeEngine::new(preset)?;
    let mut i = 0usize;
    while i < specs.len() {
        let s = &specs[i];
        let scores = ValScores::new(s.scores)?;
        // 双跑同输入同序注册（确定性铁律：序不同即输入不同）。
        a.register(s.id, s.title, s.evidence, s.batch, scores.clone(), s.notes)?;
        b.register(s.id, s.title, s.evidence, s.batch, scores, s.notes)?;
        i += 1;
    }
    let fp_a = a.freeze_fingerprint()?;
    let fp_b = b.freeze_fingerprint()?;
    let dist_a = a.distribution()?;
    let dist_b = b.distribution()?;
    let book_a = a.export_book()?;
    let book_b = b.export_book()?;
    if fp_a != fp_b {
        return Err(ValGradeError::FingerprintDrift {
            frozen: fp_a,
            computed: fp_b,
        });
    }
    if dist_a != dist_b {
        return Err(ValGradeError::DistributionImbalance {
            graded_sum: dist_a.graded_sum() as u32,
            entry_total: dist_b.graded_sum() as u32,
        });
    }
    if book_a.fingerprint != book_b.fingerprint {
        return Err(ValGradeError::ExportSequenceBroken { at: 0 });
    }
    Ok(DualRunProof {
        fp_a,
        fp_b,
        dist_a,
        dist_b,
        export_fp_a: book_a.fingerprint,
        export_fp_b: book_b.fingerprint,
        identical: true,
    })
}

// =====================================================================
// 编译期自检簇（本单全量 const 断言汇总；任何一条破即编译红）
// =====================================================================

/// 诊断码闭集自检：十二码严格递增且全落 0x1690 段。
const _: () = assert!(XC_ENTRY_DUP == VG_SEG_BASE as u32 + 1);
const _: () = assert!(XC_SCORE_OOB == XC_ENTRY_DUP + 1);
const _: () = assert!(XC_WEIGHT_SUM == XC_SCORE_OOB + 1);
const _: () = assert!(XC_BOUND_VIOL == XC_WEIGHT_SUM + 1);
const _: () = assert!(XC_LOW_LEAK == XC_BOUND_VIOL + 1);
const _: () = assert!(XC_LEDGER_BROKEN == XC_LOW_LEAK + 1);
const _: () = assert!(XC_FINGER_DRIFT == XC_LEDGER_BROKEN + 1);
const _: () = assert!(XC_INJECT_MISSED == XC_FINGER_DRIFT + 1);
const _: () = assert!(XC_ID_OOB == XC_INJECT_MISSED + 1);
const _: () = assert!(XC_GRADE_UNSET == XC_ID_OOB + 1);
const _: () = assert!(XC_DIST_IMBAL == XC_GRADE_UNSET + 1);
const _: () = assert!(XC_EXPORT_SEQ == XC_DIST_IMBAL + 1);

/// 档界边界语义自检：3999 低 / 4000 中 / 6999 中 / 7000 高。
const _: () = assert!(matches!(
    GradeTier::from_total(GRADE_MID_FLOOR - 1),
    GradeTier::Low
));
const _: () = assert!(matches!(GradeTier::from_total(GRADE_MID_FLOOR), GradeTier::Mid));
const _: () = assert!(matches!(
    GradeTier::from_total(GRADE_HIGH_FLOOR - 1),
    GradeTier::Mid
));
const _: () = assert!(matches!(GradeTier::from_total(GRADE_HIGH_FLOOR), GradeTier::High));

// =====================================================================
// 冒烟测试（隔离探针 cfg(test) 面；断言仅存在于测试路径）
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// 样本规格集：12 条域经条目，覆盖三档 + 三处档界边界
    /// （7000 恰高档 / 4000 恰中档 / 6999 恰中档），预设 0「复用优先」。
    ///
    /// 期望分布（预设 0 权重 [4500,2500,1500,1500]）：
    /// 高档 4（id 1/6/7/12）、中档 5（id 2/4/8/9/11）、低档 3（id 3/5/10）。
    fn sample_specs() -> Vec<EntrySpec> {
        vec![
            EntrySpec {
                id: 1,
                title: "页表遍历降级路径的逐级回退打法",
                evidence: "B07/L12",
                batch: 7,
                scores: [9000, 8000, 9000, 8000],
                notes: [
                    "检索命中 9 次跨 5 批",
                    "内存/调度两域验证",
                    "步骤 2 可直用",
                    "捕获 3 类反例",
                ],
            },
            EntrySpec {
                id: 2,
                title: "串口日志行缓冲的双阈值刷新",
                evidence: "B11/L03",
                batch: 11,
                scores: [5000, 5000, 5000, 5000],
                notes: ["同批命中 5 次", "单域", "步骤 10", "反例 2 类"],
            },
            EntrySpec {
                id: 3,
                title: "EDID 逐字节校验的偶发误判记录（孤例）",
                evidence: "B03/L21",
                batch: 3,
                scores: [1000, 1000, 1000, 1000],
                notes: ["零外部命中", "外推未验证", "步骤 18", "无反例边界"],
            },
            EntrySpec {
                id: 4,
                title: "ACPI 表头校验和的批量预扫",
                evidence: "B05/L08",
                batch: 5,
                scores: [6000, 4000, 6000, 4000],
                notes: ["跨 2 批命中 6 次", "1.5 域", "步骤 8", "反例 1 类"],
            },
            EntrySpec {
                id: 5,
                title: "启动横幅渲染的帧时序（未复验）",
                evidence: "B13/L30",
                batch: 13,
                scores: [2000, 3000, 1000, 500],
                notes: ["仅自查", "0.5 域", "步骤 18", "无边界"],
            },
            EntrySpec {
                id: 6,
                title: "五域消费面的批账行号对齐规约",
                evidence: "B20/L02",
                batch: 20,
                scores: [10000, 10000, 5000, 5000],
                notes: [
                    "命中 ≥10 次满档",
                    "五域全覆盖",
                    "步骤 10",
                    "反例 2 类",
                ],
            },
            EntrySpec {
                id: 7,
                title: "档界恰高档样例：四维齐 7000",
                evidence: "B22/L14",
                batch: 22,
                scores: [7000, 7000, 7000, 7000],
                notes: ["", "", "", ""],
            },
            EntrySpec {
                id: 8,
                title: "档界恰中档样例：四维齐 4000",
                evidence: "B23/L09",
                batch: 23,
                scores: [4000, 4000, 4000, 4000],
                notes: ["", "", "", ""],
            },
            EntrySpec {
                id: 9,
                title: "档界恰中档上沿样例：四维齐 6999",
                evidence: "B24/L17",
                batch: 24,
                scores: [6999, 6999, 6999, 6999],
                notes: ["", "", "", ""],
            },
            EntrySpec {
                id: 10,
                title: "早期分配器的低复用观察",
                evidence: "B02/L44",
                batch: 2,
                scores: [900, 800, 700, 600],
                notes: ["零命中", "仅本域", "重实现", "无反例"],
            },
            EntrySpec {
                id: 11,
                title: "NMI 窗口看门狗的重入抑制",
                evidence: "B15/L05",
                batch: 15,
                scores: [7000, 5000, 8000, 6000],
                notes: ["跨 4 批命中 7 次", "2 域", "步骤 4", "反例 2 类"],
            },
            EntrySpec {
                id: 12,
                title: "热复用结论：批账指纹守恒铁律",
                evidence: "B30/L01",
                batch: 30,
                scores: [8500, 7500, 6500, 5500],
                notes: ["命中 8 次跨 5 批", "3 域验证", "步骤 7", "反例 2 类"],
            },
        ]
    }

    /// 建样本引擎（登记全样本；测试断言用 unwrap 语义不存在——
    /// 失败即 panic 由测试框架承担，账面路径零 panic 不受影响）。
    fn build_engine(preset: usize) -> GradeEngine {
        let mut eng = GradeEngine::new(preset).ok().expect("引擎构造必成");
        let specs = sample_specs();
        let mut i = 0usize;
        while i < specs.len() {
            let s = &specs[i];
            let scores = ValScores::new(s.scores).ok().expect("样本分必合法");
            eng.register(s.id, s.title, s.evidence, s.batch, scores, s.notes)
                .ok()
                .expect("样本登记必成");
            i += 1;
        }
        eng
    }

    #[test]
    fn band_index_boundaries() {
        assert_eq!(band_index(0), 0);
        assert_eq!(band_index(999), 0);
        assert_eq!(band_index(1000), 1);
        assert_eq!(band_index(4999), 4);
        assert_eq!(band_index(5000), 5);
        assert_eq!(band_index(9999), 9);
        assert_eq!(band_index(10000), 9);
        // 越界归末带（拒收由 Result 面负责）。
        assert_eq!(band_index(10001), 9);
    }

    #[test]
    fn band_label_closed_set_complete() {
        // 4 维 × 10 带 = 40 条文案，全部非空且可查。
        let mut d = 0usize;
        while d < DIM_COUNT {
            let mut b = 0usize;
            while b < BAND_COUNT {
                let label = band_label(d, b);
                assert!(!label.is_empty());
                assert!(!band_label(d, b).contains("越界"));
                b += 1;
            }
            d += 1;
        }
        // 越界兜底不 panic。
        assert!(band_label(9, 0).contains("越界"));
        assert!(band_label(0, 99).contains("越界"));
        // 刻度来源四条齐。
        let mut s = 0usize;
        while s < DIM_COUNT {
            assert!(!dim_scale_source(s).is_empty());
            s += 1;
        }
        assert!(dim_scale_source(99).contains("越界"));
    }

    #[test]
    fn weight_presets_sum_and_names() {
        assert_eq!(PRESET_COUNT, 3);
        let mut p = 0usize;
        while p < PRESET_COUNT {
            assert_eq!(weight_row_sum(&PRESET_WEIGHTS[p]), 10000);
            assert_eq!(PRESET_SUMS[p], 10000);
            assert!(!PRESET_NAMES[p].is_empty());
            p += 1;
        }
        // 引擎侧：三预设权重和全 10000；越界预设拒收。
        let mut q = 0usize;
        while q < PRESET_COUNT {
            let eng = GradeEngine::new(q).ok().expect("预设必合法");
            assert_eq!(eng.weight_sum(), 10000);
            q += 1;
        }
        assert!(GradeEngine::new(3).is_err());
        assert!(GradeEngine::new(99).is_err());
    }

    #[test]
    fn tier_mapping_boundaries() {
        assert_eq!(GradeTier::from_total(0), GradeTier::Low);
        assert_eq!(GradeTier::from_total(3999), GradeTier::Low);
        assert_eq!(GradeTier::from_total(4000), GradeTier::Mid);
        assert_eq!(GradeTier::from_total(6999), GradeTier::Mid);
        assert_eq!(GradeTier::from_total(7000), GradeTier::High);
        assert_eq!(GradeTier::from_total(10000), GradeTier::High);
        // 档序与档名闭集。
        assert_eq!(GradeTier::High.rank(), 0);
        assert_eq!(GradeTier::Mid.rank(), 1);
        assert_eq!(GradeTier::Low.rank(), 2);
        assert_eq!(GradeTier::High.as_str(), "高档");
        assert_eq!(GradeTier::Mid.as_str(), "中档");
        assert_eq!(GradeTier::Low.as_str(), "低档");
    }

    #[test]
    fn val_scores_validation() {
        assert!(ValScores::new([0, 0, 0, 0]).is_ok());
        assert!(ValScores::new([10000, 10000, 10000, 10000]).is_ok());
        let bad = ValScores::new([5000, 10001, 0, 0]);
        match bad {
            Err(e) => {
                assert_eq!(e.code(), XC_SCORE_OOB);
                assert!(matches!(e, ValGradeError::ScoreOutOfRange { dim: 1, value: 10001 }));
            }
            Ok(_) => panic!("越界分必须拒收"),
        }
        let s = ValScores::new([9000, 8000, 7000, 6000]).ok().expect("合法分");
        assert_eq!(s.get(0).ok(), Some(9000));
        assert_eq!(s.get(3).ok(), Some(6000));
        assert!(s.get(4).is_err());
        assert_eq!(s.bands(), [9, 8, 7, 6]);
    }

    #[test]
    fn register_and_duplicate_rejected() {
        let mut eng = build_engine(0);
        assert_eq!(eng.entry_count(), 12);
        assert_eq!(eng.report_count(), 12); // 登记即评级：一登一评
        // 重号拒收。
        let dup = eng.register(
            1,
            "重号试探",
            "B01/L01",
            1,
            ValScores::new([1000, 1000, 1000, 1000]).ok().expect("合法"),
            ["", "", "", ""],
        );
        match dup {
            Err(ValGradeError::EntryDuplicate { id }) => assert_eq!(id, 1),
            _ => panic!("重号必须拒收"),
        }
        // 编号越界拒收（0 与超上限）。
        assert!(eng.register(0, "x", "x", 1,
            ValScores::new([100, 100, 100, 100]).ok().expect("合法"), ["", "", "", ""]).is_err());
        assert!(eng.register(1_000_000, "x", "x", 1,
            ValScores::new([100, 100, 100, 100]).ok().expect("合法"), ["", "", "", ""]).is_err());
        // 批号越界拒收。
        assert!(eng.register(99, "x", "x", 0,
            ValScores::new([100, 100, 100, 100]).ok().expect("合法"), ["", "", "", ""]).is_err());
        assert!(eng.register(99, "x", "x", 38,
            ValScores::new([100, 100, 100, 100]).ok().expect("合法"), ["", "", "", ""]).is_err());
        // 分越界拒收（登记侧：ValScores::new 构造面即红）。
        let bad_scores = ValScores::new([100, 20000, 100, 100]);
        assert!(matches!(
            bad_scores,
            Err(ValGradeError::ScoreOutOfRange { dim: 1, value: 20000 })
        ));
    }

    #[test]
    fn formula_recomputable() {
        let eng = build_engine(0);
        // 逐条复算：报告总分 = 手工四维加权（同一算式）。
        let specs = sample_specs();
        let mut i = 0usize;
        while i < specs.len() {
            let s = &specs[i];
            let report = eng.latest_report(s.id).ok().expect("样本必有报告");
            let manual: u64 = (s.scores[0] as u64) * 4500
                + (s.scores[1] as u64) * 2500
                + (s.scores[2] as u64) * 1500
                + (s.scores[3] as u64) * 1500;
            assert_eq!(report.weighted_sum, manual, "id {} 中间和不符", s.id);
            assert_eq!(report.total as u64, manual / 10000, "id {} 总分不符", s.id);
            assert_eq!(report.tier, GradeTier::from_total(report.total));
            // 合成式留痕非空且含关键量。
            assert!(report.formula.contains("合成式"));
            assert!(report.formula.contains(&format!("→ {}", report.total)));
            assert_eq!(report.revision, 1);
            assert_eq!(report.preset, 0);
            i += 1;
        }
        // 抽查边界条目总分。
        assert_eq!(eng.latest_report(7).ok().expect("在簿").total, 7000);
        assert_eq!(eng.latest_report(8).ok().expect("在簿").total, 4000);
        assert_eq!(eng.latest_report(9).ok().expect("在簿").total, 6999);
    }

    #[test]
    fn scorecard_justifications() {
        let eng = build_engine(0);
        let ent = &eng.entries()[0];
        assert_eq!(ent.card.len(), 4);
        assert!(!ent.card.is_empty());
        let mut d = 0usize;
        while d < 4 {
            let item = ent.card.item(d).ok().expect("四条齐");
            assert_eq!(item.dim, d);
            assert_eq!(item.score, ent.scores.raw()[d]);
            assert_eq!(item.band, band_index(item.score));
            assert!(!item.note.is_empty());
            d += 1;
        }
        assert!(ent.card.item(4).is_err());
        // 空评语落占位（留痕不许缺位）。
        let ent7 = &eng.entries()[6];
        let it0 = ent7.card.item(0).ok().expect("在");
        assert!(it0.note.contains("无评语"));
    }

    #[test]
    fn distribution_conservation() {
        let eng = build_engine(0);
        let dist = eng.distribution().ok().expect("全有报告");
        // 期望：高 4（1/6/7/12）中 5（2/4/8/9/11）低 3（3/5/10）。
        assert_eq!(dist.high, 4);
        assert_eq!(dist.mid, 5);
        assert_eq!(dist.low, 3);
        assert_eq!(dist.graded_sum(), 12);
        assert!(dist.conserves(12));
        assert!(eng.conservation_check().is_ok());
        // 计数取面。
        assert_eq!(dist.count_of(GradeTier::High), 4);
        assert_eq!(dist.count_of(GradeTier::Mid), 5);
        assert_eq!(dist.count_of(GradeTier::Low), 3);
    }

    #[test]
    fn export_excludes_low_and_ordered() {
        let mut eng = build_engine(0);
        let book = eng.export_book().ok().expect("守恒必过");
        // 低档 3 条（id 3/5/10）全排除；导出 9 行。
        assert_eq!(book.excluded_low, 3);
        assert_eq!(book.records.len(), 9);
        assert_eq!(eng.excluded_low_total(), 3);
        // 序：档序升序 → 编号升序；低档 id 全不在导出面。
        let mut prev_key: u64 = 0;
        let mut i = 0usize;
        while i < book.records.len() {
            let r = &book.records[i];
            assert!(r.tier.rank() < 2, "低档混出 id={}", r.id);
            assert!(r.id != 3 && r.id != 5 && r.id != 10, "低档 id={} 混出", r.id);
            let key = ((r.tier.rank() as u64) << 32) | r.id as u64;
            assert!(key > prev_key, "导出序破坏 @{}", i);
            prev_key = key;
            i += 1;
        }
        // 首行 = 高档最小号（id 1），末行 = 中档最大号（id 11）。
        assert_eq!(book.records[0].id, 1);
        assert_eq!(book.records[0].tier, GradeTier::High);
        assert_eq!(book.records[8].id, 11);
        assert_eq!(book.records[8].tier, GradeTier::Mid);
        // 低档混出预检直测。
        assert!(GradeEngine::scan_low_leak(&book.records).is_ok());
    }

    #[test]
    fn export_deterministic_repeat() {
        let mut eng = build_engine(0);
        let b1 = eng.export_book().ok().expect("第一次导出");
        let b2 = eng.export_book().ok().expect("第二次导出");
        assert_eq!(b1.fingerprint, b2.fingerprint, "同态双导出指纹必须全同");
        assert_eq!(b1.records.len(), b2.records.len());
        let mut i = 0usize;
        while i < b1.records.len() {
            assert_eq!(b1.records[i].id, b2.records[i].id);
            assert_eq!(b1.records[i].total, b2.records[i].total);
            i += 1;
        }
    }

    #[test]
    fn regrade_history_and_latest_wins() {
        let mut eng = build_engine(0);
        // 低档 id=3 复评拉到高档：[8000,8000,8000,8000] → 8000。
        let r2 = eng
            .regrade(3, ValScores::new([8000, 8000, 8000, 8000]).ok().expect("合法"), [
                "复评：命中账翻新", "跨域回执到账", "步骤降到 4", "反例补 3 类",
            ])
            .ok()
            .expect("复评必成");
        assert_eq!(r2.revision, 2);
        assert_eq!(r2.total, 8000);
        assert_eq!(r2.tier, GradeTier::High);
        // 最新版生效。
        assert_eq!(eng.latest_report(3).ok().expect("在").tier, GradeTier::High);
        // 历史保留：报告总数 12 + 1 = 13。
        assert_eq!(eng.report_count(), 13);
        // 分布与守恒随最新版走：高 5 中 5 低 2。
        let dist = eng.distribution().ok().expect("全有报告");
        assert_eq!((dist.high, dist.mid, dist.low), (5, 5, 2));
        assert!(eng.conservation_check().is_ok());
        // 未登记编号复评拒收。
        assert!(eng.regrade(777, ValScores::new([1, 1, 1, 1]).ok().expect("合法"), ["", "", "", ""]).is_err());
        // 复评后低档排除计数变化：再导出应排 2 条（id 5/10）。
        let book = eng.export_book().ok().expect("导出");
        assert_eq!(book.excluded_low, 2);
        assert_eq!(book.records.len(), 10);
        // 本测试自建引擎未导出过，累计 = 本次 2（隔离计数，非跨用例）。
        assert_eq!(eng.excluded_low_total(), 2);
    }

    #[test]
    fn freeze_verify_drift_cycle() {
        let mut eng = build_engine(0);
        // 未封存即校验 → 账断链错。
        assert!(eng.verify_fingerprint().is_err());
        let fp1 = eng.freeze_fingerprint().ok().expect("封存必成");
        assert_eq!(eng.frozen_fingerprint(), Some(fp1));
        assert!(eng.verify_fingerprint().is_ok());
        assert_eq!(eng.verify_fingerprint().ok(), Some(fp1));
        // 封存后复评 → 指纹漂移检出。
        eng.regrade(5, ValScores::new([9000, 9000, 9000, 9000]).ok().expect("合法"), [
            "复评", "复评", "复评", "复评",
        ])
        .ok()
        .expect("复评必成");
        match eng.verify_fingerprint() {
            Err(ValGradeError::FingerprintDrift { frozen, computed }) => {
                assert_eq!(frozen, fp1);
                assert_ne!(computed, fp1, "漂移值必须不同于冻结值");
            }
            other => panic!("封存后改账必须漂移，实得 {:?}", other.map(|_| ())),
        }
        // 再封存 → 校验恢复全绿。
        let fp2 = eng.freeze_fingerprint().ok().expect("再封存");
        assert_ne!(fp2, fp1);
        assert!(eng.verify_fingerprint().is_ok());
    }

    #[test]
    fn recompute_all_consistent() {
        let mut eng = build_engine(0);
        let n = eng.recompute_all().ok().expect("复算必全过");
        assert_eq!(n, 12);
        // 复评后再复算仍全过（最新版报告与条目分一致）。
        eng.regrade(10, ValScores::new([3000, 3000, 3000, 3000]).ok().expect("合法"), [
            "a", "b", "c", "d",
        ])
        .ok()
        .expect("复评必成");
        assert_eq!(eng.recompute_all().ok(), Some(12));
    }

    #[test]
    fn health_all_green() {
        let mut eng = build_engine(0);
        eng.freeze_fingerprint().ok().expect("封存");
        eng.export_book().ok().expect("导出");
        eng.run_injection_suite().ok().expect("注入全检");
        let hp = eng.health();
        assert!(hp.ok(), "发现未清：{:?}", hp.findings);
        assert_eq!(hp.checks_total, 7);
        assert_eq!(hp.checks_passed, 7);
        // 健康核事件落账且序连续。
        assert_eq!(eng.events()[eng.event_count() - 1].kind, EV_HEALTH);
        let mut expect: u64 = 1;
        let mut i = 0usize;
        while i < eng.event_count() {
            assert_eq!(eng.events()[i].seq, expect);
            expect += 1;
            i += 1;
        }
    }

    #[test]
    fn injection_suite_all_caught() {
        let mut eng = build_engine(0);
        let summary = eng.run_injection_suite().ok().expect("三类必全检出");
        assert_eq!(summary.cases, 3);
        assert_eq!(summary.caught, 3);
        assert!(eng.injections_all_caught());
        // 三类留痕码位各落本段。
        assert_eq!(summary.records[0].case, 0);
        assert_eq!(summary.records[0].code, XC_WEIGHT_SUM);
        assert_eq!(summary.records[1].case, 1);
        assert_eq!(summary.records[1].code, XC_BOUND_VIOL);
        assert_eq!(summary.records[2].case, 2);
        assert_eq!(summary.records[2].code, XC_LOW_LEAK);
        // 注入类名闭集。
        assert_eq!(inject_case_name(0), "权重破和注入");
        assert_eq!(inject_case_name(1), "档界越界注入");
        assert_eq!(inject_case_name(2), "低档混出注入");
        // 单类重放仍检出。
        assert_eq!(eng.inject_weight_break().ok(), Some(true));
        assert_eq!(eng.inject_boundary_violation().ok(), Some(true));
        assert_eq!(eng.inject_low_leak().ok(), Some(true));
    }

    #[test]
    fn dual_run_determinism_full() {
        let specs = sample_specs();
        let proof = dual_run_determinism(&specs, 0).ok().expect("双跑必全同");
        assert!(proof.identical);
        assert_eq!(proof.fp_a, proof.fp_b);
        assert_eq!(proof.export_fp_a, proof.export_fp_b);
        assert_eq!(proof.dist_a, proof.dist_b);
        assert_eq!((proof.dist_a.high, proof.dist_a.mid, proof.dist_a.low), (4, 5, 3));
        // 换预设（风险优先）双跑仍全同，且分布不同（权重语义变了）。
        let proof2 = dual_run_determinism(&specs, 2).ok().expect("预设 2 双跑全同");
        assert_eq!(proof2.fp_a, proof2.fp_b);
        assert_ne!(proof.fp_a, proof2.fp_a, "换预设指纹必变（语义含权重）");
    }

    #[test]
    fn error_details_and_codes_closed() {
        // 12 变体逐一构造：码全落 0x1691~0x169C，detail 非空且含中文。
        let errs: [ValGradeError; 12] = [
            ValGradeError::EntryDuplicate { id: 1 },
            ValGradeError::ScoreOutOfRange { dim: 0, value: 1 },
            ValGradeError::WeightSumBroken { found: 9999 },
            ValGradeError::BoundaryViolation { total: 3999, claimed_tier: 0, expect_tier: 2 },
            ValGradeError::LowLeakDetected { count: 1 },
            ValGradeError::LedgerBroken { where_: "测试" },
            ValGradeError::FingerprintDrift { frozen: 1, computed: 2 },
            ValGradeError::InjectionMissed { missed: 1 },
            ValGradeError::IdOutOfRange { id: 0 },
            ValGradeError::GradeUnset { id: 9 },
            ValGradeError::DistributionImbalance { graded_sum: 11, entry_total: 12 },
            ValGradeError::ExportSequenceBroken { at: 3 },
        ];
        let mut i = 0usize;
        while i < errs.len() {
            let code = errs[i].code();
            assert!(code >= 0x1691 && code <= 0x169C, "码 0x{:04X} 出段", code);
            let detail = errs[i].detail();
            assert!(!detail.is_empty());
            assert!(detail.chars().count() > 8);
            let (phen, adv) = diag_text(code);
            assert!(!phen.is_empty() && !adv.is_empty());
            i += 1;
        }
        // 未知码兜底不 panic。
        let (phen, _) = diag_text(0xDEAD);
        assert!(phen.contains("未知"));
    }

    #[test]
    fn event_kinds_closed_and_seq_continuous() {
        let mut eng = build_engine(0);
        eng.export_book().ok().expect("导出");
        eng.freeze_fingerprint().ok().expect("封存");
        eng.run_injection_suite().ok().expect("注入");
        eng.health();
        // kind 全落闭集 1~8。
        let mut i = 0usize;
        while i < eng.event_count() {
            let ev = &eng.events()[i];
            assert!((1..=8).contains(&ev.kind), "kind {} 出闭集", ev.kind);
            assert!(!event_kind_name(ev.kind).is_empty());
            i += 1;
        }
        // 全事件种类在样本流程里至少出现一次（登记/评级/导出/封存/注入/健康）。
        let mut seen = [false; 9];
        let mut j = 0usize;
        while j < eng.event_count() {
            seen[eng.events()[j].kind as usize] = true;
            j += 1;
        }
        assert!(seen[EV_REGISTERED as usize]);
        assert!(seen[EV_GRADED as usize]);
        assert!(seen[EV_EXPORTED as usize]);
        assert!(seen[EV_FROZEN as usize]);
        assert!(seen[EV_INJECTED as usize]);
        assert!(seen[EV_HEALTH as usize]);
    }

    #[test]
    fn fingerprint_fnv_baseline() {
        // FNV 本体基准（编译期已断言，此处运行期双保险）。
        assert_eq!(fnv1a32(b""), 0x811C_9DC5);
        assert_eq!(fnv1a32(b"a"), 0xE40C_292C);
        // 同输入同指纹、异输入异指纹（抽样）。
        assert_eq!(fnv1a32(b"unx745"), fnv1a32(b"unx745"));
        assert_ne!(fnv1a32(b"unx745"), fnv1a32(b"unx746"));
    }

    #[test]
    fn display_and_version() {
        let e = ValGradeError::EntryDuplicate { id: 7 };
        let disp = alloc::format!("{}", e);
        assert!(disp.contains("0x1691"));
        assert!(disp.contains("条目重号"));
        assert_eq!(VG_VERSION, "unx745-valgrade-v1");
        assert_eq!(VG_SEG_BASE, 0x1690);
        assert_eq!(ENGINE_CAP, 1024);
        assert_eq!(VG_ID_MAX, 999_999);
    }

    #[test]
    fn preset2_risk_first_changes_grades() {
        // 同一样本在「风险优先」下分布不同——权重不是摆设。
        let eng = build_engine(2);
        let dist = eng.distribution().ok().expect("全有报告");
        // 预设 2 权重 [2000,2000,2000,4000]：四维和同则总分同维均值，
        // 手算 id=12 [8500,7500,6500,5500]：
        // 8500*2000+7500*2000+6500*2000+5500*4000 = 17M+15M+13M+22M = 67M → 6700 中档。
        assert_eq!(eng.latest_report(12).ok().expect("在").total, 6700);
        assert_eq!(eng.latest_report(12).ok().expect("在").tier, GradeTier::Mid);
        assert!(eng.conservation_check().is_ok());
        let _ = dist;
    }
}
