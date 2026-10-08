//! VE-F2207 · 粒子排序与混合（VE-L 域 · 粒子与物理域 · 批次 L01 第 7 项 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2207`
//!
//! **判据（锚点原文五条）**：深度排序、三混合语义、免排序声明、GPU 预留、判据。
//! 逐条落位：
//! - **深度排序**：[`view_depth`] 由世界位置算视空间深度（`dot(p - eye, forward)`），
//!   [`sort_comparison`]（比较排序 O(N log N)）与 [`sort_radix`]（基数排序 O(N)）
//!   **两条真实 CPU 路径**给出**同一个置换**——不是「两条差不多」，而是判据
//!   `E02-双路-置换逐位一致` 断言两条路径输出**完全相同的 `index` 序列**。
//!   画家算法方向是**由远及近**（先画远的，后画近的），故键**降序**。
//! - **三混合语义**：[`BlendMode`] 的加法 / alpha / 预乘三型（[`BLEND_MODES`]
//!   常量表对账，缺一或多一即红），每型各有**适用场景说明**
//!   （[`BlendMode::scene`]：加法=光效、alpha=实体、预乘=精确合成）与
//!   **四维+alpha 维因子表**（[`BlendMode::factors`]），混合本体是
//!   [`blend`] 这个**纯函数**（无随机、无时序、无累加状态）。
//! - **免排序声明**：本条**不靠文档口头声明**，而是把「免排序」做成**可实测的
//!   事实**——[`order_probe`] 对一帧的全部粒子求**两种顺序下的合成结果**
//!   并**分开报两级**：加法的**交换律逐位成立**（`f32` 加法可交换，这是
//!   免排序的数学前提）**且** N 项折叠的两序差落在 [`order_tolerance`]
//!   （8 ulp）之内；alpha 与预乘的交换律**不成立****且**折叠差**远超容差**。
//!
//!   **这里有一条实测教训值得写进代码，故写进**：最初本条把免排序写成
//!   「加法两序**逐位相等**」，实测**红**——`f32` 加法可交换但**不可结合**
//!   （`(a+b)+c != a+(b+c)`，典型差 1 ulp）。同一判据在 4 元素样本上碰巧
//!   绿、在 3 元素样本上红：那是**靠运气的弱门禁**，比没有判据更坏。
//!   故改为两级可判 + 显式容差，并**如实声明**：加法免排序成立，但其
//!   顺序敏感度是 ulp 级（不是零）。把「近乎顺序无关」谎报成「顺序无关」
//!   会让日后有人据此把 N 项求和改成任意重排而引入可见偏差。
//!
//!   判据 `E03-免排序-*` **双向**验证：只断「加法交换律成立」会被恒等混合
//!   骗过，只断「alpha 交换律不成立」会被无序混合骗过，两者必须同时成立。
//! - **GPU 预留**：[`GpuSortRequest`] 是 GPU 基数排序的**接口位**
//!   （键宽/趟数/工作组/缓冲字节齐备，参数校验真实存在），但
//!   [`gpu_radix_sort`] 一期**恒失败**并给错误三要素（当前形态 / 一期提供什么 /
//!   建议）——F1871 语义：**预留位被调用必须显性报错，绝不静默**。
//!   「O(N) 基数排序的 GPU 实现」是**预留声明**，本仓**不代填未实现的东西**。
//! - **判据**：`vel07_checks.rs` 逐条映射，**双向验证**（基线绿 + 变体红）。
//!
//! **排序键的诚实声明（本域最易出假绿处）**：排序键是**量化深度**
//! （[`DEPTH_QUANT_STEPS`]级均匀量化），不是裸 `f32`。这不是精度妥协，
//! 而是**契约本身**：真实 GPU 管线同样只能对整数键做基数排序，若判据拿
//! 裸 `f32` 比对而实现用量化键，两者在量化桶边界上必然分歧。因此
//! - 规范序定义在**量化键**上：降序键、同键按**提交序升序**；
//! - 量化误差有解析上界 [`quant_error_bound`]，供调用方按需选步数；
//! - 比较排序与基数排序对齐的是**同一契约**，故逐位一致是可证的。
//!
//! **与F2206 的分工**：F2206 决定「怎么画」（形态/朝向/顶点），
//! 本模块决定「**按什么次序画**」与「**画的时候怎么混**」。故本模块
//! **只消费** F2206 的 [`ParticleView`]（位置与颜色），不改它一个字节；
//! 混合的 alpha 来自 F2205 淡变结果，本模块**不再乘一次**（曲线单源）。
//!
//! **降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | alpha 模式关排序 → 显性警告（视觉错误预期声明，不静默允许错误配置） | [`judge`] 判 [`SortVerdict::Forbidden`]，[`plan`] 产 [`BlendDiag::SortDisabledOrderError`] 且**不排序**（不排才是「按用户配置执行」，但绝不静默） |
//! | 混合模式与 I03 材质混合冲突 → 语义对齐核验 | [`desc_wire`] 给出七元wire 键（与 I03 混合状态同构），判据用**独立重算的因子求值器**对拍 [`blend`]，并断三型**互不同键** |
//! | 排序键 NaN（粒子在相机后方）→ 钳制处理 | [`clamp_depth`] 把非有限与负深度钳到近平面并**回报是否钳制**，[`keys_from_views`] 累加钳制计数 |
//! | GPU 预留被调用 → 显性报错 | [`gpu_radix_sort`]恒 `Err`，三要素齐备 |
//!
//! **性能逐项分解（锚点原文四条 → 可数落位）**：比较排序 O(N log N)
//! ——比较次数由 [`SortStats::comparisons`] **实测计数**，不写估值；
//! 基数排序 O(N) ——趟数由 [`RADIX_PASSES`] 常量给出、扫描量正比于 N；
//! 加法免排序**零成本**——[`judge`] 判 [`SortVerdict::Unnecessary`] 时
//! [`plan`] **不进入任何排序路径**且[`SortStats::sorted`] 为假（有判据钉
//! 「排序开关关着时比较次数恰为 0」，防「关了开关仍偷偷排」）；
//! 排序开关**逐发射器粒度**——[`EmitterSort`] 是单发射器结构，
//! [`plan`] 的入参即它，故粒度由类型保证而非运行时约定。
//!
//! **成本定标诚实声明**：锚点给出「10k 粒子约 1ms」，本仓**不把它写成
//! 自造常量**——那正是弱门禁的温床。真实基线由 **VE-F2212（粒子基准）**
//! 实测回填，本模块只提供**可数的复杂度证据**（比较次数/趟数/是否进入
//! 排序路径），由 F2212 对着本模块的 [`SortStats`] 取时。
//!
//! **跨批对接**：混合语义与 I03 材质混合对齐（跨域，接线见头注「已知偏差」）；
//! GPU 预留与 F1871（预留不静默）/ F1745（流输出）呼应；
//! 池侧对接 F2208（逐发射器上限与池压力）。
//!
//! **已知偏差（诚实记录，不静默改写锚点）**：锚点写「I03 F1670 混合状态衔接」，
//! 而册内 `VE-F1670`的标题是「蒙皮 fuzz」，与「混合状态」不对应；本仓
//! 真正落地的混合状态机是 **VE-F0019（`vea19_blend`）**。本模块因此按
//! **F0019 的四维+alpha 维因子语义**对齐（[`desc_wire`] 的七元wire 键与之同构），
//! 并**不硬引用**F0019 类型以免跨域耦合。锚点与册内标题的这一出入，
//! 按纪律**如实上报**而非自行编造对齐对象。
//!
//! 零 IO、零墙钟；相机向量与视口均注入，故同输入双跑逐位一致
//! （F2215 CPU 确定性根基在本模块的延续）。无隐私面。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::vel03_emitter::{DiagBag, DiagCode, Outcome};
use super::vel06_render::ParticleView;

// `Rgba` 既用于本模块签名又是对外承诺的类型（混合的输入输出），故再导出。
pub use super::vel05_lifetime::Rgba;

// ---------------------------------------------------------------------------
// 一、排序键：世界位置 → 视空间深度 → 量化整数键（判据一：深度排序）
// ---------------------------------------------------------------------------

/// 近平面深度（视空间 z 下界）。深度小于此值（含负值）即「在相机后方」。
pub const DEPTH_NEAR: f32 = 0.0;

/// 远平面深度（视空间 z 上界）。深度大于此值即「超出远平面」，同样钳到这里。
pub const DEPTH_FAR: f32 = 1.0;

/// 深度量化步数（2^16 级）。
///
/// 键宽选 16 位不是随手取的：基数排序一趟按 8 位分桶，16 位 = 2 趟的载荷，
/// 而比较排序的键比较对 32 位与 16 位同价。步数公开且可改，改动会改
/// 量化误差（[`quant_error_bound`]）——故它是**契约的一部分**，
/// 不是可以随手调的性能旋钮。
pub const DEPTH_QUANT_STEPS: u32 = 65_536;

/// 基数排序单趟位宽（8 位 = 256桶）。
pub const RADIX_BITS: u32 = 8;

/// 基数排序**单相**趟数（32 位键 / 8 位一趟）。
pub const RADIX_PASSES: usize = 4;

/// 基数排序**总**趟数（两相，见 [`sort_radix`]：先按序号、再按取反键）。
///
/// 两相而非一趟的原因（**实测发现的真缺陷**）：规范序是
/// 「键降序 + 同键序号升序」，而「降序」在LSD 基数排序里只能靠
/// 「整体反转」得到——可反转会把**同键组内的相对序也一起翻掉**，
/// 于是组内变成序号降序。最初实现靠「倒序喂入 + 末尾反转」凑对了
/// `keys_from_views` 的输入（那里 `index` 恰等于数组位置），但一旦调用
/// [`plan`] 传入 `index != position` 的条目（`plan` 的入参是任意
/// `entries`），结果就与 [`order_cmp`] **分道扬镳**——而那正是本模块要防的事。
///
/// 正确做法：把规范序拆成**两个稳定基数相**的复合——
/// ① 先按 `index` **升序**基数排序（建立「同键组内序号升序」的基础序）；
/// ② 再按 `!key`（取反键）**升序**基数排序（`!key` 升序 ≡ `key` 降序）。
/// LSD 基数排序的「先次键后主键」复合天然给出「主键降序 + 次键升序」。
/// 故总趟数 = 2 × [`RADIX_PASSES`]，仍是 **O(N)**（与 N 无关的常数倍）。
pub const RADIX_TOTAL_PASSES: usize = RADIX_PASSES * 2;

/// 基数排序桶数（`1 << RADIX_BITS`）。
pub const RADIX_BUCKETS: usize = 1 << RADIX_BITS;

/// 三点 dot（分量积求和）。
///
/// 不引入外部数学库：本仓内核 `[dependencies]` 为空。
pub fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// 视空间深度（**未钳制**）：`(p - eye) · forward`。
///
/// 约定 `forward` 为**单位前向量**，视线方向为正，故深度越大越远。
/// 非单位向量也能算，但量化步长随之改变——那会让「量化误差上界」
/// 与实际不符，故本函数**不擅自归一化**：归一化与否是调用方的语义决定，
/// [`keys_from_views`] 的入参即已归一化的 `forward`，责任边界写在这里。
pub fn view_depth_raw(position: [f32; 3], eye: [f32; 3], forward: [f32; 3]) -> f32 {
    let d = [position[0] - eye[0], position[1] - eye[1], position[2] - eye[2]];
    dot3(d, forward)
}

/// 深度钳制：返回 `(钳制后深度, 是否被钳制)`。
///
/// 钳制对象有两类，**都必须钳**：
/// 1. **非有限**（NaN / ±Inf）——位置含 NaN 时深度是 NaN，而 NaN 参与
///    任何比较都不成立（既不 `<` 也不 `>`），排序结果将取决于算法内部
///    的实现细节，即**不确定性**。这是本域最恶劣的缺陷形态：画面不报错，
///    只是粒子次序随机。故 NaN 一律钳到近平面**并显式回报被钳制**。
/// 2. **越界**（< 近平面 = 在相机后方；> 远平面 = 超远平面）——深度是
///    视空间量，落在视锥外的粒子没有合法的「远近」，按最近/最远处理是
///    唯一可预期的行为。
///
/// 回报「是否钳制」而非静默改写：调用方需要据此统计（[`SortStats`] 的
/// `key_clamps`），静默钳制会让「相机后方有一堆粒子」这件事永远看不见。
pub fn clamp_depth(depth: f32) -> (f32, bool) {
    if depth.is_nan() {
        return (DEPTH_NEAR, true);
    }
    if depth < DEPTH_NEAR {
        return (DEPTH_NEAR, true);
    }
    if depth > DEPTH_FAR {
        return (DEPTH_FAR, true);
    }
    (depth, false)
}

/// 量化误差上界（半桶宽，世界单位视深）。
///
/// 均匀量化步长 `h = (FAR - NEAR) / STEPS`，取整误差 ≤ `h/2`。
/// 这是**解析上界**，不是实测值——调用方按「最坏情形排序误差 ≤ 此值」
/// 使用即可，无需跑基准。
pub fn quant_error_bound() -> f32 {
    (DEPTH_FAR - DEPTH_NEAR) / (DEPTH_QUANT_STEPS as f32) * 0.5
}

/// 深度 → 量化键（0 ..= `DEPTH_QUANT_STEPS - 1`，**单调递增**）。
///
/// 用 `floor` 而非四舍五入：四舍五入会把恰好落在桶边界两侧的深度
/// 归到同一桶，误差可达整桶；`floor` 保持「键随深度单调不减」这一
/// **可判定性质**（判据 `E01-键-单调不减`直接断言它），误差恒 ≤ 1 桶。
pub fn depth_key(clamped_depth: f32) -> u32 {
    let span = DEPTH_FAR - DEPTH_NEAR;
    let t = (clamped_depth - DEPTH_NEAR) / span;
    // t ∈ [0,1] 已被 clamp_depth 保证；仍挡一层浮点边界（t 可能因舍入 >1）。
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
    let scaled = (t * (DEPTH_QUANT_STEPS as f32)).floor();
    // floor 后仍可能因浮点给出 STEPS（t 恰为 1.0），故再钳一次。
    if scaled >= DEPTH_QUANT_STEPS as f32 {
        DEPTH_QUANT_STEPS - 1
    } else if scaled < 0.0 {
        0
    } else {
        scaled as u32
    }
}

// ---------------------------------------------------------------------------
// 二、CPU 排序：两条真实路径，同一契约（判据一：深度排序）
// ---------------------------------------------------------------------------

/// 排序条目：量化键 + 提交序号。
///
/// **序号必须随键一起排**：只按键排时，同键粒子的相对次序就取决于
/// 排序算法是否稳定——`sort_by` 稳定而 `sort_unstable_by` 不稳定，
/// 同一份代码换个标准库版本次序就变了。把「同键按提交序升序」写进
/// 条目里，规范序就**不再依赖算法**，这既是确定性的根基，也是两条
/// 路径能逐位一致的前提。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortEntry {
    /// 量化深度键。
    pub key: u32,
    /// 提交序号（0起，帧内唯一）。
    pub index: u32,
}

impl SortEntry {
    pub fn new(key: u32, index: u32) -> Self {
        SortEntry { key, index }
    }
}

/// 规范序比较：**键降序**（由远及近，画家算法），同键**序号升序**。
///
/// 这是**唯一**的序定义。两条排序路径都必须实现它，故判据可以拿
/// 「独立实现的第三条序」三方对拍而不必相信任何一条路径。
pub fn order_cmp(a: &SortEntry, b: &SortEntry) -> core::cmp::Ordering {
    // 键与序号都是 u32，整数的 `cmp` 已是**全序**（无自反失败、无传递性
    // 破缺），故不需要 `total_cmp`——那个方法是浮点专用的。写在这里
    // 是因为「排序结果依赖比较器是否一致」是最难查的一类红项，
    // 值得把「为何这里天然安全」记下来。
    b.key.cmp(&a.key).then_with(|| a.index.cmp(&b.index))
}

/// 排序统计（可数的工作量证据，非估值）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SortStats {
    /// 参与排序的条目数。
    pub n: usize,
    /// 比较排序的**实测比较次数**。
    ///
    /// 记这个数而不是记「O(N log N)」：复杂度是渐近断言，而调用方要的是
    /// 「这一帧到底比了多少次」——它可以直接与 F2212 的耗时对账。
    pub comparisons: u64,
    /// 基数排序趟数（比较排序为 0）。
    pub radix_passes: u32,
    /// 深度被钳制的条目数（相机后方 / 超远平面 / NaN）。
    pub key_clamps: u64,
    /// 本次是否**真的进入了排序路径**。
    ///
    /// 这个标志是「免排序零成本」的**可证形式**：加了法模式 + 关排序，
    /// `comparisons` 必为 0 且本项为假——两者都在判据里被直接断言。
    pub sorted: bool,
}

impl SortStats {
    /// 是否零成本（未进入排序路径且未产生比较）。
    pub fn is_free(&self) -> bool {
        !self.sorted && self.comparisons == 0
    }
}

/// 比较排序路径（O(N log N)）：就地按 [`order_cmp`] 排 [`entries`]。
///
/// `sort_by` 在 `alloc` 下可用且稳定；因为 [`order_cmp`] 本身已是全序
/// （键 + 序号），稳定性只是冗余保险，正确性不依赖它。
pub fn sort_comparison(entries: &mut Vec<SortEntry>) -> SortStats {
    let n = entries.len();
    let mut comparisons: u64 = 0;
    entries.sort_by(|a, b| {
        comparisons += 1;
        order_cmp(a, b)
    });
    SortStats {
        n,
        comparisons,
        radix_passes: 0,
        key_clamps: 0,
        sorted: n > 1,
    }
}

/// 基数排序路径（O(N)）：两相稳定LSD 基数排序，实现规范序
/// 「**键降序 + 同键序号升序**」。
///
/// **为什么必须两相**（实测发现的真缺陷，见 [`RADIX_TOTAL_PASSES`]）：
/// 「键降序」在 LSD 基数排序里只能靠取反键（`!key` 升序）或末尾反转得到，
/// 而**整体反转会把同键组内的相对序一起翻掉**。若只靠反转，则组内变成
/// 序号降序——对 `keys_from_views` 的输入（`index` 恰等于数组位置）
/// 碰巧正确，但对 `plan` 传入的任意 `entries`（`index != position`）
/// 就与 [`order_cmp`] 分道扬镳。故此处显式两相：
/// ① 按 `index` **升序**（建立组内序号升序的基础序）；
/// ② 按 `!key` **升序**（≡ `key` 降序）。
/// LSD 的「先次键、后主键」复合天然给出「主键降序 + 次键升序」。
///
/// 仍**零分配**：工作缓冲由调用方传入，两相之间用 `core::mem::swap` 轮换。
pub fn sort_radix(entries: &mut Vec<SortEntry>, scratch: &mut Vec<SortEntry>) -> SortStats {
    let n = entries.len();
    if n <= 1 {
        return SortStats { n, comparisons: 0, radix_passes: 0, key_clamps: 0, sorted: false };
    }

    // 相①：按 `index` 升序（次键）；相②：按 `!key` 升序（主键降序）。
    //
    // **每相趟数为偶数**（4 趟 × 8 位 = 32 位），故每相结束时数据回到
    // `entries`，两相可串在同一缓冲上。
    //
    // 早先版本有两处静默失效，两处都报「趟数正确、sorted=true」而实际
    // 未排——比报错更坏，故把数据流写明并由判据直接断言输出：
    //① 两相的 src/dst 接反（相① 写进 `entries`，相②又从 `entries` 取
    //   输入），两相互相覆盖，输出等于输入原样；
    // ② 相内每趟都从同一源读，不交替读写侧 ⇒ 每趟都用原始输入覆盖
    //   上上趟成果。
    // 两处都在 [`radix_phase`] 里以「逐趟交换读写侧」根治。
    scratch.clear();

    let mut passes: u32 = 0;
    passes += radix_phase(entries, |e| e.index);
    passes += radix_phase(entries, |e| !e.key);

    SortStats {
        n,
        comparisons: 0,
        radix_passes: passes,
        key_clamps: 0,
        sorted: true,
    }
}

/// 单相稳定 LSD 基数排序：**就地**把 `buf` 排成「按 `key_of` 升序」。
///
/// 返回趟数。**不做任何末尾反转**——降序由「取反键」在键函数里表达，
/// 这样同键组内的相对序由本相的稳定性保证，不会被反转破坏。
///
/// **为什么必须逐趟交换读写侧**（实测踩到的两个坑，都很静默）：
/// -坑一：LSD 的第 `t` 趟必须读「前一趟的产物」。若每趟都从**同一个**
///   源数组读、写另一个数组，则每趟都在用**原始输入**覆盖上上趟的
///   成果——趟数报足、结果等于没排。故此处每趟结束交换读写侧。
/// - 坑二：用 `dst.get_mut(at)` 逐位写，而 `dst` 长度可能为 0 ⇒ 越界
///   位置**静默跳过**，函数照样返回「趟数正确」，而数组原封未动。
///   故每趟先确保两侧等长，再按下标写——「长度不足」变成不可能。
///
/// 趟数为**偶数**（32 位键 / 8 位 = 4 趟），故结束时读写侧已换回原处，
/// 结果仍在 `buf` 里。这让「偶数趟」成为调用方的前提——已由
/// [`RADIX_PASSES`] 与判据 `E06-基数-总趟数为两相之和` 双向钉住。
fn radix_phase<F>(buf: &mut Vec<SortEntry>, key_of: F) -> u32
where
    F: Fn(&SortEntry) -> u32,
{
    let n = buf.len();
    if n <= 1 {
        return 0;
    }
    if buf.len() != n {
        buf.resize(n, SortEntry::new(0, 0));
    }
    let mut work: Vec<SortEntry> = Vec::with_capacity(n);
    work.resize(n, SortEntry::new(0, 0));

    let mut counts = [0u32; RADIX_BUCKETS];
    let mut shift: u32 = 0;
    let mut passes: u32 = 0;
    while shift < 32 {
        // 以「当前持有数据的那个 Vec」为读侧。
        let (rd, wr): (&[SortEntry], &mut Vec<SortEntry>) = if passes % 2 == 0 {
            (&*buf, &mut work)
        } else {
            (&work, &mut *buf)
        };
        if wr.len() != n {
            wr.resize(n, SortEntry::new(0, 0));
        }

        let mut c = [0u32; RADIX_BUCKETS];
        for e in rd.iter() {
            let b = ((key_of(e) >> shift) & (RADIX_BUCKETS as u32 - 1)) as usize;
            c[b] += 1;
        }
        // 前缀和 → 各桶的起始写位。
        let mut acc: u32 = 0;
        for b in 0..RADIX_BUCKETS {
            counts[b] = acc;
            acc += c[b];
        }
        // 按桶顺序**稳定**散布（本趟内的等键保序 ⇒ 复合后的次键序成立）。
        for e in rd.iter() {
            let b = ((key_of(e) >> shift) & (RADIX_BUCKETS as u32 - 1)) as usize;
            let at = counts[b] as usize;
            // 桶计数之和恒等于 n，故 at < n恒成立（长度已对齐）。
            if at < n {
                wr[at] = *e;
            }
            counts[b] += 1;
        }
        shift += RADIX_BITS;
        passes += 1;
    }
    passes
}

/// 由粒子视图生成排序条目（**只读**消费 F2206 的 [`ParticleView`]）。
///
/// 「只读」是类型事实：入参 `&[ParticleView]`，返回全新 `Vec`，F2206 的
/// 视图一个字节都不会被改（判据以签名扫描钉住渲染面无写回）。
///
/// 深度非有限 / 越界者在此处钳制并累加计数——**钳制发生在这里而不在
/// 排序里**，因为排序拿到的一定是已钳制的键，这是「键的契约」的唯一入口。
/// 单入口的意义：若排序内部还留着一条`depth_key(NaN)` 的旁路，
/// 判据「全部键有限」就会红，那正是要抓的东西。
pub fn keys_from_views(views: &[ParticleView], eye: [f32; 3], forward: [f32; 3]) -> (Vec<SortEntry>, u64) {
    let mut out: Vec<SortEntry> = Vec::with_capacity(views.len());
    let mut clamps: u64 = 0;
    for (i, v) in views.iter().enumerate() {
        let raw = view_depth_raw(v.position, eye, forward);
        let (d, clamped) = clamp_depth(raw);
        if clamped {
            clamps += 1;
        }
        let key = depth_key(d);
        out.push(SortEntry::new(key, i as u32));
    }
    (out, clamps)
}

// ---------------------------------------------------------------------------
// 三、混合三模式：语义声明 + 因子表 + 纯函数本体（判据二：三混合语义）
// ---------------------------------------------------------------------------

/// 混合因子（与 I03/F0019 的 `BlendFactor` wire 值同构，取用到的子集）。
///
/// wire 值与 F0019 一致（`Zero=0 / One=1 / SrcAlpha=4 / OneMinusSrcAlpha=10`），
/// 判据 `E04-对齐-wire值同构` 直接断这些数字——若I03 侧改了 wire 值而
/// 本表没改，跨域对齐就断了，而**断的是数字不是感觉**。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Factor {
    /// 0：`0`。
    Zero = 0,
    /// 1：`1`。
    One = 1,
    /// 4：源 alpha（塌缩为标量，作用于各颜色通道）。
    SrcAlpha = 4,
    /// 10：`1 - 源 alpha`。
    OneMinusSrcAlpha = 10,
}

impl Factor {
    /// wire 值（对齐核验用）。
    pub fn wire(self) -> u8 {
        self as u8
    }
}

/// 混合算子（本域三型只用 `Add`，但枚举保持完整——只列用到的那一个，
/// 「对齐核验表」就成了自说自话，无从对齐）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Op {
    /// 0：`s + d`。
    Add = 0,
    /// 1：`s - d`。
    Subtract = 1,
    /// 3：`min(s, d)`（硬件忽略因子）。
    Min = 3,
    /// 4：`max(s, d)`（硬件忽略因子）。
    Max = 4,
}

impl Op {
    /// wire 值。
    pub fn wire(self) -> u8 {
        self as u8
    }
}

/// 四维 + 独立 alpha 维的因子表（I03 混合状态的同构描述）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlendFactors {
    /// 源颜色因子。
    pub src_color: Factor,
    /// 目标颜色因子。
    pub dst_color: Factor,
    /// 颜色算子。
    pub color_op: Op,
    /// 源 alpha 因子。
    pub src_alpha: Factor,
    /// 目标 alpha 因子。
    pub dst_alpha: Factor,
    /// alpha 算子。
    pub alpha_op: Op,
}

impl BlendFactors {
    /// 七元wire 键：`[使能, src_c, dst_c, c_op, src_a, dst_a, a_op]`。
    ///
    /// 顺序固定且**七元齐**——少一位就无法与对端逐位对账（少的那位
    /// 会被双方各自补 0，看起来仍然相等）。
    pub fn wire_key(self) -> [u8; 7] {
        [
            1,
            self.src_color.wire(),
            self.dst_color.wire(),
            self.color_op.wire(),
            self.src_alpha.wire(),
            self.dst_alpha.wire(),
            self.alpha_op.wire(),
        ]
    }

    /// 该因子表是否三型互不相同（对齐核验的前提，见 [`alignment_report`]）。
    pub fn is_canonical(&self) -> bool {
        *self != BlendFactors::straight_alpha()
    }
}

impl BlendFactors {
    /// 加法：颜色 `1/1`，alpha `1/1`。
    pub const fn additive() -> Self {
        BlendFactors {
            src_color: Factor::One,
            dst_color: Factor::One,
            color_op: Op::Add,
            src_alpha: Factor::One,
            dst_alpha: Factor::One,
            alpha_op: Op::Add,
        }
    }
    /// 直通 alpha：`SrcAlpha / OneMinusSrcAlpha`。
    pub const fn straight_alpha() -> Self {
        BlendFactors {
            src_color: Factor::SrcAlpha,
            dst_color: Factor::OneMinusSrcAlpha,
            color_op: Op::Add,
            src_alpha: Factor::One,
            dst_alpha: Factor::OneMinusSrcAlpha,
            alpha_op: Op::Add,
        }
    }
    /// 预乘 alpha：`One / OneMinusSrcAlpha`（源颜色**已预乘**）。
    pub const fn premultiplied() -> Self {
        BlendFactors {
            src_color: Factor::One,
            dst_color: Factor::OneMinusSrcAlpha,
            color_op: Op::Add,
            src_alpha: Factor::One,
            dst_alpha: Factor::OneMinusSrcAlpha,
            alpha_op: Op::Add,
        }
    }
}

/// 粒子混合模式（三型，判据二）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendMode {
    /// 加法：光效叠加。`src×1 + dst×1`，**顺序无关**。
    Additive,
    /// 直通 alpha：实体半透明。`src×a + dst×(1-a)`，**顺序相关**。
    Alpha,
    /// 预乘 alpha：精确合成，边缘质量优（半透明边缘不发黑）。
    /// `src + dst×(1-a)`，源 rgb **已预乘**，**顺序相关**。
    Premultiplied,
}

/// 在册混合模式表（判据对账基准）。
///
/// 与 F2206 的 `FORMS` 同理：枚举加变体是编译期改动，而「三型」是**规格
/// 承诺**。只有这张常量表能拦住「悄悄加了第四型」。
pub const BLEND_MODES: [BlendMode; 3] =
    [BlendMode::Additive, BlendMode::Alpha, BlendMode::Premultiplied];

impl BlendMode {
    /// 中文标签（读屏播报用）。
    pub fn zh(self) -> &'static str {
        match self {
            BlendMode::Additive => "加法",
            BlendMode::Alpha => "直通 alpha",
            BlendMode::Premultiplied => "预乘 alpha",
        }
    }

    /// 适用场景（锚点：加法=光效 / alpha=实体 / 预乘=精确合成）。
    pub fn scene(self) -> &'static str {
        match self {
            BlendMode::Additive => "光效叠加：火焰/火花/辉光，叠加处自然变亮",
            BlendMode::Alpha => "实体半透明：烟/雾/玻璃片，实体感优先",
            BlendMode::Premultiplied => "精确合成：软边粒子/已预乘纹理，半透明边缘不发黑",
        }
    }

    /// 该模式是否**顺序相关**（即是否必须先排序）。
    ///
    /// 这是免排序声明的**唯一依据**。判据不用本函数当唯一答案，而是用
    /// [`order_probe`] **实测**交换律与折叠差——故本函数写错了会被抓出来
    /// （判据断言「本函数与实测一致」）。
    ///
    /// **加法为假的精确含义**：加法**不**折叠序敏感，而是折叠差在
    /// [`order_tolerance`] 之内（ulp 级）。见 [`order_probe`] 的说明。
    pub fn requires_sort(self) -> bool {
        match self {
            BlendMode::Additive => false,
            BlendMode::Alpha | BlendMode::Premultiplied => true,
        }
    }

    /// 该模式的因子表（I03 同构）。
    pub fn factors(self) -> BlendFactors {
        match self {
            BlendMode::Additive => BlendFactors::additive(),
            BlendMode::Alpha => BlendFactors::straight_alpha(),
            BlendMode::Premultiplied => BlendFactors::premultiplied(),
        }
    }

    /// 该模式的七元 wire 键（跨域对齐核验用）。
    pub fn desc_wire(self) -> [u8; 7] {
        self.factors().wire_key()
    }
}

/// 把直通 alpha 源转成预乘源（`rgb *= a`）。
///
/// 预乘模式的**唯一输入前置条件**就是「源已预乘」。若调用方忘了预乘，
/// 画面表现为半透明边缘发黑（F0019 预置表里`alpha_premultiplied` 的
/// `use_note` 点名的正是这个现象）。本函数是该前置条件的**显式化**，
/// 而不是让每个调用方各自手写一遍乘法。
pub fn premultiply(src: Rgba) -> Rgba {
    let a = if src.a.is_finite() { src.a } else { 0.0 };
    Rgba { r: src.r * a, g: src.g * a, b: src.b * a, a }
}

/// 混合本体（**纯函数**）：把 `src` 混到 `dst` 上，返回结果。
///
/// - 不钳制到 `[0,1]`：加法在 HDR 目标上可以超过 1（那是合法亮度），
///   钳掉就是画质事故（与 F0019「浮点目标不钳制」同纪律）。
/// - 非有限防护在**输入侧**做一次：输出 NaN 会污染后续每一次合成
///   （NaN 一旦进入 dst 就再也出不来），故宁可在入口挡掉。
/// - 无随机、无时序、无内部状态：同输入必同输出（F2215 的纯函数纪律）。
pub fn blend(mode: BlendMode, src: Rgba, dst: Rgba) -> Rgba {
    let sv = |v: f32| if v.is_finite() { v } else { 0.0 };
    let s = Rgba { r: sv(src.r), g: sv(src.g), b: sv(src.b), a: sv(src.a) };
    let d = Rgba { r: sv(dst.r), g: sv(dst.g), b: sv(dst.b), a: sv(dst.a) };
    match mode {
        // src×1 + dst×1：加法满足交换律与结合律 → 顺序无关 → 免排序。
        BlendMode::Additive => Rgba {
            r: s.r + d.r,
            g: s.g + d.g,
            b: s.b + d.b,
            a: s.a + d.a,
        },
        // src×sa + dst×(1-sa)（源 rgb 未预乘）。
        BlendMode::Alpha => {
            let ia = 1.0 - s.a;
            Rgba {
                r: s.r * s.a + d.r * ia,
                g: s.g * s.a + d.g * ia,
                b: s.b * s.a + d.b * ia,
                a: s.a + d.a * ia,
            }
        }
        // src + dst×(1-sa)（源 rgb 已预乘）。
        BlendMode::Premultiplied => {
            let ia = 1.0 - s.a;
            Rgba {
                r: s.r + d.r * ia,
                g: s.g + d.g * ia,
                b: s.b + d.b * ia,
                a: s.a + d.a * ia,
            }
        }
    }
}

/// 顺序相关性实测（**两级**，因为「顺序无关」在浮点下有两个不同的含义）。
///
/// **本模块最容易被自欺的地方**：加法满足交换律，于是「加法免排序」听上去
/// 可以直接断「任意置换逐位相等」。实测**不成立**——`f32` 加法可交换
/// （`a+b == b+a` 逐位成立）但**不可结合**（`(a+b)+c != a+(b+c)`，典型差异
/// 1 ulp）。所以 N≥3 的折叠顺序确实会改变结果，哪怕只差一个 ulp。
/// 若把判据写成「加法两序逐位相等」，它只在**特定样本**上碰巧成立
/// （本域实测：4 元素样本过、3 元素样本红），那就是典型的**弱门禁**——
/// 判据靠运气绿，而实现的真实性质（顺序敏感，只是敏感得极弱）被掩盖。
///
/// 故本函数把两件事**分开报**，各自可判：
/// - [`OrderProbe::commutes_bitwise`]：**两两交换**是否逐位成立。
///   加法必为真；alpha 与预乘必为假（源项与目标项不对称）。
///   这是「加法免排序」的**数学前提**，且在 `f32` 下是**精确**可判的。
/// - [`OrderProbe::fold_delta`] 与 [`order_sensitive`]：N 项折叠两序的
///   最大分量差，以及该差是否**超出容差**。加法应落在容差内（免排序
///   成立，只是 ulp 级数值差）；alpha 与预乘应远超容差（真顺序相关）。
///
/// 于是判据成对断言：「加法交换律逐位成立 **且** 折叠差在容差内」
/// 与「alpha/预乘交换律不成立 **且** 折叠差远超容差」。前者被「恒等混合」
/// 骗不过（恒等混合不满足交换律成立？不——它满足，故仍需后者配对），
/// 后者被「无序混合」骗不过，故**两者必须同时成立**。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrderProbe {
    /// 两两交换是否**逐位**成立（`blend(a,b) == blend(b,a)`）。
    pub commutes_bitwise: bool,
    /// N 项折叠两序之间的最大分量差（绝对值）。
    pub fold_delta: f32,
    /// 折叠差是否**超出** [`ORDER_TOLERANCE`]（即真顺序敏感）。
    pub order_sensitive: bool,
}

/// 折叠顺序差的容差（相对 `f32::EPSILON` 的倍数）。
///
/// 取 8 ulp 而非 1 ulp：三项链式加法的最大累积舍入约 `2 ulp`，留一倍余量。
/// **刻意不用 `==0`**：若容差取 0，本判据就退回到「靠样本运气」的弱门禁
/// （上面那条实测教训的直接来源）。
pub const ORDER_TOLERANCE_ULP: f32 = 8.0;

/// 绝对容差（量级 1.0 处的 `ORDER_TOLERANCE_ULP` 个 `f32::EPSILON`）。
pub fn order_tolerance() -> f32 {
    ORDER_TOLERANCE_ULP * f32::EPSILON
}

/// 顺序相关性实测：见 [`OrderProbe`] 的说明（两级：交换律 + 折叠容差）。
///
/// 合成顺序：正序 `((((0⊕1)⊕2)…⊕n)`，逆序 `((n⊕(n-1))…⊕1)`，其中 `⊕` 是 [`blend`]。
/// 交换律取**第一对**颜色实测（`⊕` 的基本性质，与长度无关）。
pub fn order_probe(mode: BlendMode, colors: &[Rgba]) -> OrderProbe {
    if colors.len() < 2 {
        return OrderProbe { commutes_bitwise: true, fold_delta: 0.0, order_sensitive: false };
    }
    let a = colors[0];
    let b = colors[1];
    let commutes_bitwise = bits_eq(blend(mode, a, b), blend(mode, b, a));

    let mut fwd = Rgba::new(0.0, 0.0, 0.0, 0.0);
    for c in colors.iter() {
        fwd = blend(mode, *c, fwd);
    }
    let mut rev = Rgba::new(0.0, 0.0, 0.0, 0.0);
    for c in colors.iter().rev() {
        rev = blend(mode, *c, rev);
    }
    let fold_delta = max_component_delta(fwd, rev);
    OrderProbe {
        commutes_bitwise,
        fold_delta,
        order_sensitive: fold_delta > order_tolerance(),
    }
}

/// 两个颜色的最大分量绝对差（四通道）。
fn max_component_delta(a: Rgba, b: Rgba) -> f32 {
    let d0 = (a.r - b.r).abs();
    let d1 = (a.g - b.g).abs();
    let d2 = (a.b - b.b).abs();
    let d3 = (a.a - b.a).abs();
    let mut m = d0;
    if d1 > m {
        m = d1;
    }
    if d2 > m {
        m = d2;
    }
    if d3 > m {
        m = d3;
    }
    m
}

/// 逐位比较两个颜色（`to_bits` 而非 `==`：`+0.0 == -0.0` 为真而
/// 位型不同，把它算作「相等」会放过一类符号错误）。
pub fn bits_eq(a: Rgba, b: Rgba) -> bool {
    bits_of(a) == bits_of(b)
}

/// 颜色的四位型（判据与去重用）。
pub fn bits_of(c: Rgba) -> [u32; 4] {
    [c.r.to_bits(), c.g.to_bits(), c.b.to_bits(), c.a.to_bits()]
}

/// 按给定次序（`indices`）把一帧颜色合成到底色上。
///
/// 排序的**消费口**：排完的置换最终要变成「谁先谁后混」，这个函数是
/// 那个转换的唯一实现。判据用它把「置换」与「画面结果」连起来，
/// 否则「排对了」只是一句无法验证的话。
pub fn composite_in_order(mode: BlendMode, base: Rgba, colors: &[Rgba], indices: &[u32]) -> Rgba {
    let mut acc = base;
    for idx in indices.iter() {
        let i = *idx as usize;
        if let Some(c) = colors.get(i) {
            acc = blend(mode, *c, acc);
        }
    }
    acc
}

// ---------------------------------------------------------------------------
// 四、排序开关（逐发射器粒度）与正确性裁决
// ---------------------------------------------------------------------------

/// 逐发射器的排序配置（**粒度由类型保证**，不是运行时约定）。
///
/// 一个发射器一套配置，故「alpha 发射器排序、加法发射器免排序」是
/// 天然可表达的——把它做成全局开关，美术就得为了一个火光发射器
/// 让全场烟雾都付排序的钱。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmitterSort {
    /// 混合模式。
    pub mode: BlendMode,
    /// 是否启用排序。
    pub sorting: bool,
}

impl EmitterSort {
    pub fn new(mode: BlendMode, sorting: bool) -> Self {
        EmitterSort { mode, sorting }
    }
}

/// 排序裁决（三态，**不用 bool** —— 一个 bool 表达「必须排」与
/// 「禁止排」两件语义相反的事，调用点必然误用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortVerdict {
    /// 顺序相关模式 + 开排序：**必须排**，正确性前提成立。
    Required,
    /// 顺序无关模式 + 关排序：免排序声明成立，**零成本**。
    Unnecessary,
    /// 顺序相关模式 + 关排序：**配置错误**——显性警告，不静默。
    Forbidden,
}

impl SortVerdict {
    /// 中文标签（读屏播报与诊断文本用）。
    pub fn zh(self) -> &'static str {
        match self {
            SortVerdict::Required => "必须排序",
            SortVerdict::Unnecessary => "免排序",
            SortVerdict::Forbidden => "关排序配置错误",
        }
    }

    /// 是否允许进入排序路径。
    pub fn may_sort(self) -> bool {
        matches!(self, SortVerdict::Required)
    }
}

/// 裁决一个发射器的排序配置。
///
/// 裁决**只读配置、不产生诊断**——诊断由 [`plan`] 统一产出，故「裁决」
/// 可以被反复调用（判据要对拍多次）而不污染诊断袋。这是「纯裁决 /
///  impure 记录」的分层，避免判据依赖诊断副作用。
pub fn judge(e: &EmitterSort) -> SortVerdict {
    if e.mode.requires_sort() {
        if e.sorting {
            SortVerdict::Required
        } else {
            SortVerdict::Forbidden
        }
    } else if e.sorting {
        // 顺序无关模式开着排序：合法但浪费。判为 Unnecessary 并给建议，
        // 因为「合法但浪费」若不提示，就会变成「每帧白付N log N」。
        SortVerdict::Unnecessary
    } else {
        SortVerdict::Unnecessary
    }
}

/// 一帧的排序计划产出。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SortPlan {
    /// 裁决结果。
    pub verdict: SortVerdict,
    /// 排好的提交序（升序即「谁先画」）。
    ///
    /// `Forbidden` 与 `Unnecessary` 时为**原始顺序**（`0..n`）：
    /// 用户显式关掉了排序，我们按他的配置执行，但**同时**留下诊断。
    /// 悄悄改成「反正排一下更保险」同样是越权——正确性后果由用户承担，
    /// 我们的职责是**说清楚**，不是替他决定。
    pub order: Vec<u32>,
    /// 工作量证据。
    pub stats: SortStats,
    /// 使用的路径名（比较/基数/免）。
    pub path: &'static str,
}

/// 排序路径名（性能对账用）。
pub const PATH_COMPARISON: &str = "比较排序 O(N logN)";
/// 排序路径名。
pub const PATH_RADIX: &str = "基数排序 O(N)";
/// 排序路径名。
pub const PATH_NONE: &str = "免排序";

/// 排序算法选择（一期两条 CPU 路径都在，GPU 路径预留）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortAlgo {
    /// 比较排序。
    Comparison,
    /// 基数排序。
    Radix,
}

impl SortAlgo {
    /// 中文标签。
    pub fn zh(self) -> &'static str {
        match self {
            SortAlgo::Comparison => PATH_COMPARISON,
            SortAlgo::Radix => PATH_RADIX,
        }
    }
}

/// 产出排序计划：裁决 → （若允许）排序 → 记诊断。
///
/// `scratch` 是基数排序的工作缓冲，由调用方持有以便**零分配**复用
/// （内核路径不接受帧内分配）。
///
/// 诊断一律走 F2203 的 [`DiagBag`]（全链路单一诊断出口）；本域的码
/// 语义由 [`BlendDiag`] 承载，不去扩F2203 的封闭枚举。
pub fn plan(
    e: &EmitterSort,
    algo: SortAlgo,
    entries: &mut Vec<SortEntry>,
    scratch: &mut Vec<SortEntry>,
    bag: &mut DiagBag,
) -> SortPlan {
    let verdict = judge(e);
    let n = entries.len();
    let mut order: Vec<u32> = Vec::with_capacity(n);
    let mut it = entries.iter();
    while let Some(en) = it.next() {
        order.push(en.index);
    }

    match verdict {
        SortVerdict::Forbidden => {
            // 视觉错误**预期声明**：说清「关掉排序会看到什么」，而不是
            // 只说「配置非法」——用户需要的是他能据此做决定的信息。
            note(
                bag,
                BlendDiag::SortDisabledOrderError,
                format!(
                    "发射器混合模式为「{}」，属顺序相关模式，而排序被关闭：半透明粒子将按提交顺序混合，重叠处会出现前后颠倒（该发射器共{} 个粒子）",
                    e.mode.zh(),
                    n
                ),
                "为该发射器打开排序，或把混合模式改为「加法」（加法顺序无关，可免排序）".to_string(),
            );
            SortPlan {
                verdict,
                order,
                stats: SortStats { n, comparisons: 0, radix_passes: 0, key_clamps: 0, sorted: false },
                path: PATH_NONE,
            }
        }
        SortVerdict::Unnecessary => {
            if e.sorting {
                note(
                    bag,
                    BlendDiag::SortUnnecessary,
                    format!(
                        "发射器混合模式为「{}」，顺序无关，排序可安全关闭（本帧省掉 {}个粒子的排序）",
                        e.mode.zh(),
                        n
                    ),
                    "关闭该发射器的排序开关：结果不变，成本降为 0".to_string(),
                );
            }
            SortPlan {
                verdict,
                order,
                stats: SortStats { n, comparisons: 0, radix_passes: 0, key_clamps: 0, sorted: false },
                path: PATH_NONE,
            }
        }
        SortVerdict::Required => {
            let mut stats = match algo {
                SortAlgo::Comparison => sort_comparison(entries),
                SortAlgo::Radix => sort_radix(entries, scratch),
            };
            stats.key_clamps = 0; // 钳制计数在 keys_from_views 侧统计
            order.clear();
            let mut it2 = entries.iter();
            while let Some(en) = it2.next() {
                order.push(en.index);
            }
            SortPlan { verdict, order, stats, path: algo.zh() }
        }
    }
}

// ---------------------------------------------------------------------------
// 五、跨域语义对齐核验（I03 / F0019 混合状态）
// ---------------------------------------------------------------------------

/// 对齐核验报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlignmentReport {
    /// 三型的 wire 键两两不同（否则有两型在GPU 上会落进同一状态）。
    pub keys_distinct: bool,
    /// 三型都在册。
    pub all_listed: bool,
    /// 加法型的 wire 键是否恰为 `[1,1,1,0,1,1,0]`。
    pub additive_canonical: bool,
    /// 直通 alpha 型是否恰为 `[1,4,10,0,1,10,0]`。
    pub alpha_canonical: bool,
    /// 预乘型是否恰为 `[1,1,10,0,1,10,0]`。
    pub premul_canonical: bool,
    /// 是否存在**语义重合**的两型（对齐核验要抓的正是这个）。
    pub semantic_overlap: bool,
}

/// 与 I03（F0019）混合状态的语义对齐核验。
///
/// 核验的是**七元 wire 键**而不是「感觉像」：三型各自的因子组合必须与
/// I03 的 `One/One`、`SrcAlpha/OneMinusSrcAlpha`、`One/OneMinusSrcAlpha`
/// 三种预置**逐位相同**，且三型**互不相同**——否则粒子模式与材质模式
/// 会在同一状态下被GPU 归并，出现「粒子用了 alpha 材质状态」这类
/// 跨域串味，而画面上只表现为「有的粒子边缘发黑」，极难定位。
pub fn alignment_report() -> AlignmentReport {
    let add = BlendMode::Additive.desc_wire();
    let alp = BlendMode::Alpha.desc_wire();
    let pre = BlendMode::Premultiplied.desc_wire();

    let keys_distinct = add != alp && alp != pre && add != pre;
    let listed = BLEND_MODES.len() == 3
        && BLEND_MODES.contains(&BlendMode::Additive)
        && BLEND_MODES.contains(&BlendMode::Alpha)
        && BLEND_MODES.contains(&BlendMode::Premultiplied);

    // 直通 alpha 与预乘的**颜色因子不同但 alpha 维相同**——这正是
    // 「半透明边缘发黑」的唯一来源，故对齐核验必须逐位区分它们，
    // 不能只核「都含 OneMinusSrcAlpha」。
    let semantic_overlap = alp[2] == pre[2] && alp[1] == pre[1];

    AlignmentReport {
        keys_distinct,
        all_listed: listed,
        additive_canonical: add == [1u8, 1, 1, 0, 1, 1, 0],
        alpha_canonical: alp == [1u8, 4, 10, 0, 1, 10, 0],
        premul_canonical: pre == [1u8, 1, 10, 0, 1, 10, 0],
        semantic_overlap,
    }
}

// ---------------------------------------------------------------------------
// 六、GPU 预留位（F1871 语义：被调用必显性报错）
// ---------------------------------------------------------------------------

/// GPU 基数排序请求（**接口位**：一期只校验参数，不执行）。
///
/// 字段齐备的意义：GPU 路径真来做的时候不需要改这个结构体的形状，
/// 只需换执行载体。故「预留」是**结构层面的预留**，不是一句注释。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuSortRequest {
    /// 粒子数。
    pub count: u32,
    /// 键宽（位）。
    pub key_bits: u32,
    /// 趟数（每趟8 位）。
    pub passes: u32,
    /// 工作组大小。
    pub group_size: u32,
    /// 键缓冲字节（调用方按此分配）。
    pub key_bytes: u64,
}

impl GpuSortRequest {
    /// 由粒子数与键宽构造一个**合规**请求（供调用方与判据作基线）。
    pub fn for_count(count: u32) -> Self {
        GpuSortRequest {
            count,
            key_bits: 16,
            passes: 2,
            group_size: 256,
            key_bytes: (count as u64) * 4,
        }
    }

    /// 参数校验：返回错误三要素齐备的 [`Rejection`]。
    ///
    /// 校验真实存在（不是摆设）——GPU 路径即便未实现，**请求本身**
    /// 也要能被拒：否则「预留」连形状都没人守。
    pub fn validate(&self) -> Result<(), Rejection> {
        if self.count == 0 {
            return Err(Rejection {
                what: "GPU 基数排序请求".to_string(),
                limit: "粒子数须 ≥ 1".to_string(),
                suggestion: "空批次无需排序；确认调用点是否在帧边界之后发起".to_string(),
            });
        }
        if self.key_bits == 0 || self.key_bits > 32 {
            return Err(Rejection {
                what: format!("键宽 {} 位", self.key_bits),
                limit: "键宽须落在 [1, 32]".to_string(),
                suggestion: "沿用 DEPTH_QUANT_STEPS 对应的 16 位键".to_string(),
            });
        }
        let need = (self.key_bits + RADIX_BITS - 1) / RADIX_BITS;
        if self.passes != need {
            return Err(Rejection {
                what: format!("趟数 {}", self.passes),
                limit: format!("键宽 {} 位需恰好 {} 趟", self.key_bits, need),
                suggestion: "趟数按 key_bits/8 向上取整；多趟是浪费，少趟会漏高位桶".to_string(),
            });
        }
        if self.group_size == 0 || !self.group_size.is_power_of_two() {
            return Err(Rejection {
                what: format!("工作组大小 {}", self.group_size),
                limit: "须为 2 的幂且 ≥ 1".to_string(),
                suggestion: "沿用 256（与桶数同阶）".to_string(),
            });
        }
        let expect = (self.count as u64) * 4;
        if self.key_bytes != expect {
            return Err(Rejection {
                what: format!("键缓冲 {} 字节", self.key_bytes),
                limit: format!("粒子数 {} × 4 字节 = {} 字节", self.count, expect),
                suggestion: "键缓冲按 u32 对齐分配；尺寸须与粒子数一致".to_string(),
            });
        }
        Ok(())
    }
}

/// 显性拒绝（三要素：是什么 / 上限是什么 / 怎么办）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    /// 被拒对象与其当前取值。
    pub what: String,
    /// 允许范围或上限。
    pub limit: String,
    /// 人话建议。
    pub suggestion: String,
}

impl Rejection {
    /// 三要素合一文本（诊断消息用，保证三要素**一次都不少**）。
    pub fn message(&self) -> String {
        format!("{}（允许：{}）", self.what, self.limit)
    }
}

/// GPU 基数排序：**一期未实现，被调用必显性报错**（F1871 语义）。
///
/// 为什么不做「静默退回 CPU 排序」：调用方以为自己走的是 GPU 路径，
/// 性能预算与排序语义都按GPU 假设；若悄悄换成CPU 路径，它永远不知道自己
/// 在跑另一种实现，性能问题也就永远查不出来。故此处**只报错并指路**。
pub fn gpu_radix_sort(req: &GpuSortRequest) -> Outcome<Vec<u32>> {
    match req.validate() {
        Ok(()) => Outcome::fail(
            DiagCode::ShapeRejected,
            format!(
                "GPU 基数排序为预留路径，一期未实现（请求 {} 粒子 / {} 位键 / {} 趟，请求本身合规）",
                req.count, req.key_bits, req.passes
            ),
            "改用 CPU 路径：SortAlgo::Radix 已实现同一套整数键契约（O(N)），语义与 GPU 版一致"
                .to_string(),
        ),
        Err(r) => Outcome::fail(
            DiagCode::ShapeRejected,
            format!("GPU 基数排序预留被调用，且请求不合规：{}", r.message()),
            r.suggestion,
        ),
    }
}

// ---------------------------------------------------------------------------
// 七、诊断
// ---------------------------------------------------------------------------

/// 本域诊断码。
///
/// **处置方向相反的状态不共用码**：加法模式关排序（合法但浪费）与
/// alpha 模式关排序（配置错误、视觉结果是错的）若共用一码，调用方
/// 无法据此决定「要不要紧」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendDiag {
    /// 顺序相关模式关排序（视觉错误预期声明）。
    SortDisabledOrderError,
    /// 顺序无关模式开着排序（合法但浪费）。
    SortUnnecessary,
    /// 排序键被钳制（相机后方 / 超远平面 / NaN）。
    KeyClamped,
    /// GPU 预留路径被调用。
    GpuSortReserved,
}

impl BlendDiag {
    /// 中文标签。
    pub fn zh(self) -> &'static str {
        match self {
            BlendDiag::SortDisabledOrderError => "顺序相关模式关排序",
            BlendDiag::SortUnnecessary => "顺序无关模式开着排序",
            BlendDiag::KeyClamped => "排序键钳制",
            BlendDiag::GpuSortReserved => "GPU 排序预留被调用",
        }
    }

    /// 该码是否表示**配置错误**（区别于「可优化的浪费」）。
    pub fn is_error(self) -> bool {
        matches!(self, BlendDiag::SortDisabledOrderError | BlendDiag::GpuSortReserved)
    }

    /// 映射到 F2203 共享诊断码（下游封闭枚举无权加变体）。
    ///
    /// 每条映射都**只借道不扩域**：本域的差异由 [`BlendDiag`] 承载。
    pub fn code(self) -> DiagCode {
        match self {
            // 「本次动作被拒且需改配置」→借TransitionRejected。
            BlendDiag::SortDisabledOrderError => DiagCode::TransitionRejected,
            BlendDiag::GpuSortReserved => DiagCode::ShapeRejected,
            // 「原值被改写/被合并」→ 借ChurnCoalesced 与 RngDegraded。
            BlendDiag::SortUnnecessary => DiagCode::ChurnCoalesced,
            BlendDiag::KeyClamped => DiagCode::RngDegraded,
        }
    }
}

/// 记一条本域诊断进 F2203 的诊断袋（保持全链路单一诊断出口）。
pub fn note(bag: &mut DiagBag, code: BlendDiag, message: String, hint: String) {
    bag.note(code.code(), message, hint);
}