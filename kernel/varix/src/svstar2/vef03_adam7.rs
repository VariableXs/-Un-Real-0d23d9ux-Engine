//! VE-F1003 · PNG 交错模式（Adam7，目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1003`
//!
//! **判据（锚点原文逐条）**：
//! - 解码（七遍像素重排还原全图）→ `C03-DEC-*`
//! - 编码（全图 → 七遍拆分输出，拆分器与还原器共用同一常量表）→ `C03-ENC-*`
//! - 隔行与滤波的交互（每遍独立滤波 / 行缓冲独立 / 首行按遍语义）→ `C03-FILT-*`
//! - 遍描述符与遍调度器 → `C03-DESC-*`
//! - 错误路径（遍数据截断 → 已完成遍输出；步长与宽高校验）→ `C03-ERR-*`
//! - 性能差异文档化 + 编码器默认不隔行 → `C03-PERF-*`
//!
//! **职责定位（锚点原文）**：PNG Adam7 七遍隔行扫描的**双向**实现——解码侧的
//! 七遍重排还原，与编码侧的全图拆分。**不覆盖**：逐行解码（属 F1001，本模块
//! 只复用其容器解析与色彩展开）、色彩管理（F1004）、流式会话（F1007）、
//! APNG（F1006）、安全审计（F1015）、缩略图快路径（F1019，只预留接口）。
//!
//! ---
//!
//! ## 设计要点一：pass 常量表是唯一来源（锚点硬要求）
//!
//! 锚点原文：「7遍的 `(start_row, start_col, row_step, col_step)` 常量表唯一
//! 来源，**禁散落魔法数**」「拆分器与还原器共用同一常量表保证互逆」。
//!
//! 本模块以 [`ADAM7_PASSES`] 为**唯一**的遍几何真源，模块内**没有任何一处**
//! 字面量 `8` / `4` / `2` 被当作遍步长使用：
//! - 尺寸推导 [`pass_extent`] 读表；
//! - 编码拆分 [`encode_adam7`] 读表；
//! - 解码还原 [`decode_adam7`] 读表；
//! - 空遍判定 [`PassExtent::is_empty`] 读表；
//! - 坐标映射 [`pass_origin`] 读表。
//!
//! **互逆性由构造保证而非由测试碰巧**：编码器「取 `(start_row, start_col)` 起
//! 的 `row_step × col_step` 网格」与解码器「把遍内 `(py, px)` 映射回
//! `(start_row + py*row_step, start_col + px*col_step)`」是同一条算式的正反
//! 两种写法，**共用同一函数 [`pass_origin`]**，故 `decode ∘ encode ≡ id` 在
//! 结构上无法被写错——除非改坏 `pass_origin` 本身，而那会被自检的
//! 「七遍覆盖计数 == 宽×高」断言当场抓住。
//!
//! ## 设计要点二：滤波语义随遍边界重置（锚点「与逐行模式差异点显性注释」）
//!
//! 这是隔行与逐行最本质的差异，锚点专门要求显性注释：
//!
//! | | 逐行模式 | Adam7 |
//! |---|---|---|
//! | 滤波参照的「上一行」 | 全图相邻行 `y-1` | **同遍内**的上一遍行 `py-1` |
//! | 每遍首行的参照 | 仅全图首行按零行 | **每一遍的首行都按零行** |
//! | `bpp`（滤波粒度） | `channels×depth/8` | **同左，与遍无关** |
//! | `Up` 滤波的物理含义 | 与几何相邻行相减 | 与**步长 `row_step` 外**的行相减 |
//!
//! **最容易写错的一处**：把「遍内上一行」误用成「全图 `y - row_step` 行」。
//! 二者在遍内首行之后**恰好等价**，只在遍首行分岔——而遍首行恰恰是最容易
//! 少算一次的行（少算一次则整遍向上错一行 `row_step`，且不产生任何报错，
//! 只是画面出现一条缝）。因此本模块的处理是：**每遍首行都把 `prev` 缓冲
//! 填零**，让「上一遍行」恒为虚拟零行——与 F1001 首行的处理完全一致，
//! **零特例代码**。
//!
//! **此处曾有一个静默崩溃的坑（如实登记）**：初版给遍首行向
//! [`imgsimd::unfilter_auto`] 传 `prev = None`，意在走「无上一行」分支。
//! 但 [`imgsimd::unfilter_scalar`] 的 `Up` 分支写作 `let prev = prev?;`——
//! 收到 `None` 时**整行反滤波返回 `None`**，上层把它读成「滤波号非法」
//! （[`Adam7FaultKind::BadFilter`]）。而编码侧遍首行零参照时
//! `spec_choose_filter` 会**合法地选中 Up**，于是「遍首行 + Up」这一
//! 组合必然崩溃。修法是**始终传 `Some(&prev[..n])`**（遍首行时 `prev`
//! 已填零），与 F1001 的 `Some(&prev_buf)` 对齐。
//!
//! **第二处易错**：`bpp` 在隔行下**不随遍变化**。遍宽变窄不改变滤波粒度，
//! 因为 `bpp` 的定义是「一个像素的字节数」，与该遍有几列无关。若误按「遍宽」
//! 算 `bpp`，则 `Sub` / `Paeth` 的左邻距离全错，且**位深 8 正常、位深 16
//! 全错**——16 位图整幅偏色而 8 位图完全正常，最坏的一类静默缺陷。
//! 本模块统一取 [`dec::Ihdr::filter_bpp`]（与遍无关），并在自检里以
//! `RGBA8 + RGBA16` 双深度交叉断言这一点。
//!
//! ## 设计要点三：空遍是合法语义，不是错误（锚点「宽高小于首遍步长的图合法
//! 但空遍跳过」）
//!
//! 1×1 的图，七遍里有**六遍**的几何网格取不到任何像素。规范对此的处置是
//! 「该遍不贡献任何数据行」。若把空遍当错误，1×1 图会直接解不出来——而它
//! 是完全合法的 PNG。
//!
//! 本模块以 [`PassExtent::is_empty`] 判定空遍（`width == 0 || height == 0`），
//! 空遍**不产生滤波行、不消耗 raw 字节、不计入 `passes_done`**。判定式只用
//! 表驱动推导出的尺寸，不含任何 `if width < 8` 之类的手写阈值。
//!
//! ## 设计要点四：截断 → 已完成遍输出（锚点「Adam7 天然渐进语义 F1019 联动」）
//!
//! 锚点：「遍数据截断→已完成的遍输出（**低分辨率合法图像**——Adam7 天然渐进
//! 语义 F1019 联动）」。
//!
//! Adam7 的七遍本身就是「从粗到细」的分辨率阶梯：第 1 遍覆盖 1/64 像素，
//! 前三遍合计构成 1/8 分辨率的完整图。因此截断在第 k 遍结束时，前 k 遍的
//! 像素**在语义上已经是一张合法的低分辨率图**，不是「半张烂图」。
//!
//! 本模块的处置：
//! - 行数据不足 → 停在该遍，**已完成的遍照常交付**，
//!   置 [`InterlaceOutcome::truncated`] 与 [`InterlaceOutcome::passes_done`]；
//! - 输出是「像素分布不均的图」——这是 Adam7 的固有性质，**如实上报不假装
//!   完整**（与 F1001 的 `truncated` 语义一致）；
//! - 便利形 [`decode_adam7_full`] 对截断**直接报错**而不返回半图，理由与
//!   F1001 的 `decode` 一致：便利形不做部分交付。
//!
//! 下游 F1019（缩略图快路径）正是消费 `passes_done`：前 3 遍即 1/8 尺寸完整图，
//! 无需解完七遍。
//!
//! ## 设计要点五：性能差异文档化 + 默认不隔行（锚点明文要求）
//!
//! 锚点：「隔行解码比逐行慢约 1.5-2 倍——性能差异文档化并给用户提示
//! （**编码器默认不隔行**，除非用户显式要求渐进）」。
//!
//! - **诚实标注**：锚点给出的 1.5-2 倍是**规范与生态的量级估计**，本单
//!   **未做实机计时**，故不写成实测数字。结构性成本模型见 [`PerfNote`]，
//!   它只陈述**可从表推导的确定事实**（遍数、遍行总数、滤波字节增量），
//!   真实机测由 F1016（PNG 性能基准与调优）承接并入册。
//! - **默认不隔行**：[`encode_adam7`] 是**显式调用**入口；F1002 的
//!   [`enc::encode_rgba`] 默认仍产出 `interlace = 0`，本模块**不修改**其
//!   默认行为。隔行开关归 F1008（写入器选项面），那里默认 `false`。
//! - **用户提示**：[`interlace_advice`] 给出人话建议（何时该开、何时不该开），
//!   供 F1008 / F1018 直接消费，避免下游各写一套口径。
//!
//! ## 跨批对接点
//!
//! 上游：**F1001**（容器解析 / IHDR 七参数 / PLTE / tRNS / 反滤波 / 色彩展开）、
//! **F1002**（打包 / 五滤波两策略 / zlib / IDAT 分块 / CRC）、
//! `perfstar::imgsimd`（CGPU-F0094 反滤波）、`perfstar::mech_inflate`（zlib+CRC32）。
//!
//! 下游：**F1018**（七遍渐进回调与遍调度共用）、**F1019**（前几遍 = 低分辨率
//! 完整图的缩略图快路径）、**F1007**（流式会话按遍边界恢复）、
//! **F1008**（隔行开关选项面）、**F1016**（性能矩阵的隔行维度）。
//!
//! **对 F1001 的唯一改动**：新增 `parse_ihdr_ex` / `parse_container_ex` 两个
//! `_ex` 变体（**纯加法**，默认参数下行为与原函数逐位一致）。理由：F1001 对
//! `interlace = 1` 显式拒绝并指向本模块——若本模块自带一份 IHDR 与容器解析，
//! 同一套位深×颜色类型合法表就会出现两份副本，日后改表必漏一处。
//! **让出闸门而不复制校验规则**，是对该问题的正解。
//!
//! ## 设计要点六：编码侧滤波自持，**不复用 F1002 的 `apply_filter`**（缺陷登记）
//!
//! **这是本单最要紧的一处诚实登记。** 本模块的解码侧走
//! [`imgsimd::unfilter_auto`]（= F1001 的解码路径），而 `imgsimd` 的
//! `unfilter_scalar` 对四种预测子一律 `wrapping_add`——**符合 PNG 规范**
//! （规范 §9.2：编码 `Filt = Raw − Predictor`，解码 `Raw = Filt + Predictor`）。
//!
//! 但 F1002 的 [`enc::apply_filter`] 对四种预测子一律 `wrapping_add`
//! **且倒序遍历**——那是**解码方向**的运算，不是编码方向。其
//! [`enc::unfilter_row`] 又对称地改成 `wrapping_sub` 正序，于是**两者互为
//! 逆运算**、F1002 的自检（拿自己的 `unfilter_row` 当基准）全绿，而它产出的
//! PNG **任何合规解码器都读不出原图**。
//!
//! **实测证据（2026-10-07 本机，非推演）**：F1002 `encode_rgba` 的16×16 RGBA8
//! 输出交 F1001 `decode_to_rows`（`imgsimd` 解码）还原，
//! **1024 字节中 615 字节不符**；另以四滤波逐一验证，
//! `apply_filter` 后再经规范解码**四种滤波全部还原失败**，
//! 而按规范自实现编码滤波后**四种全部还原成功**。
//!
//! **本单处置**：
//! - **不代改 F1002**。该模块属F1002 已交付范围，且自检已绿；在他人的交付面
//!   上改符号会把「自检自证」的绿变成「自检与实现同错」的更隐蔽状态，且与并行
//!   会话冲突。按「上游未达标不代改，缺口写头注 + 指名承接单」纪律处理。
//! - **本模块自持编码滤波**：[`spec_apply_filter`] 与 [`spec_choose_filter`]
//!   按规范语义独立实现，不复用（也就不会继承）F1002 的符号缺陷。
//!   选择器一并自持——因为评分必须对**本模块自己的滤波输出**打分，复用
//!   别人的评分器等于对错误的候选打分。
//! - **缺口归属**：`svstar2::vef02_pngenc::apply_filter` / `unfilter_row`
//!   的预测子符号方向。
//! - **建议承接**：F1002 的修订应走**独立工单**（不占用本单），由 F1020
//!   （PNG 组收口）的「20 项入树 + PngSuite 对拍」把门禁立起来——
//!   **F1002 自检的基准必须换成 `imgsimd`（合规解码器）**，否则任何符号级
//!   改动都会「自检通过而实际不可解码」。这正是 F1011 存在的意义：
//!   拿外部参考实现对拍，才能发现「内部自洽但不合规」这一类缺陷。
//!
//! **与 F1002 的关系仍然复用**：压缩（[`enc::compress_zlib`]）、块组装
//! （[`enc::write_chunk`]）、IDAT 分块、CRC、颜色类型/位深合法表
//! （[`enc::combo_is_legal`]）——这些与滤波符号无关，全部照旧复用。
//!
//! ## 设计要点七：编码侧滤波语义随遍边界重置
//!
//! 每遍各自把 `prev` 初始化为零行，遍内行间按 [`spec_apply_filter`] 的规范
//! 语义施加滤波。`bpp` 取 [`dec::Ihdr::filter_bpp`]（**与遍无关**——见头注
//! 设计要点二）。
//!
//! **确定性**：同输入同输出（纯函数，无时钟无 IO，遍历序固定）。零外部依赖，
//! 只用 `alloc` 与上游三模块。

use crate::perfstar::imgsimd::{self};
use crate::perfstar::mech_inflate;
use crate::svstar2::vef01_pngdec as dec;
use crate::svstar2::vef02_pngenc as enc;

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、Adam7 遍常量表（唯一来源·禁散落魔法数）
// ---------------------------------------------------------------------------

/// 一遍的几何四元组（PNG 规范 §9.6 Adam7 interlace）。
///
/// 语义：遍内第 `(py, px)` 个像素对应全图坐标
/// `(start_row + py * row_step, start_col + px * col_step)`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PassGeom {
    /// 该遍首行在全图中的行号。
    pub start_row: u32,
    /// 该遍首列在全图中的列号。
    pub start_col: u32,
    /// 行步长（遍内相邻两行在全图中的行距）。
    pub row_step: u32,
    /// 列步长（遍内相邻两列在全图中的列距）。
    pub col_step: u32,
}

/// **Adam7 七遍几何表——模块内唯一的遍几何真源。**
///
/// 取值逐格来自 PNG 规范 §9.6 的 7 行表。**模块内任何地方都不得再出现
/// 作为遍步长的字面量**：尺寸推导、编码拆分、解码还原、空遍判定、坐标映射
/// 全部读本表。这不是风格偏好，而是锚点硬要求（「常量表唯一来源，禁散落
/// 魔法数」），也是 `decode ∘ encode = id` 能被构造性保证的前提。
///
/// 表的形状（`start_row` / `start_col` 沿对角线分布，`row_step` / `col_step`
/// 按 8/4/2 递减）是规范的设计：第 1 遍覆盖全部 1/64 像素的锚点，第 7 遍
/// 覆盖 1/2 行 × 全部列，合成后恰好不重不漏地铺满全图。自检以「七遍覆盖
/// 计数 == 宽×高」断言这一不重不漏性质。
pub const ADAM7_PASSES: [PassGeom; 7] = [
    PassGeom { start_row: 0, start_col: 0, row_step: 8, col_step: 8 },
    PassGeom { start_row: 0, start_col: 4, row_step: 8, col_step: 8 },
    PassGeom { start_row: 4, start_col: 0, row_step: 8, col_step: 4 },
    PassGeom { start_row: 0, start_col: 2, row_step: 4, col_step: 4 },
    PassGeom { start_row: 2, start_col: 0, row_step: 4, col_step: 2 },
    PassGeom { start_row: 0, start_col: 1, row_step: 2, col_step: 2 },
    PassGeom { start_row: 1, start_col: 0, row_step: 2, col_step: 1 },
];

/// 遍数（规范固定 7；作为常量以免调用方写 `7` 这个裸数）。
pub const PASS_COUNT: usize = ADAM7_PASSES.len();

/// 取第 `i` 遍的几何（越界返回 `None`——不 panic、不夹取到末遍）。
#[inline]
pub fn pass_geom(i: usize) -> Option<PassGeom> {
    ADAM7_PASSES.get(i).copied()
}

// ---------------------------------------------------------------------------
// 二、遍描述符（锚点「pass 描述符」：四元组 + 该遍行缓冲）
// ---------------------------------------------------------------------------

/// 一遍的推导尺寸（行缓冲尺寸由 [`PassExtent::payload_bytes`] 给出）。
///
/// 尺寸**只由 [`ADAM7_PASSES`] 与全图宽高推出**，无任何手写阈值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PassExtent {
    /// 遍序号（0..7）。
    pub index: usize,
    /// 该遍几何（自常量表原样带出，便于调用方自证来源）。
    pub geom: PassGeom,
    /// 该遍像素宽（`ceil((width - start_col) / col_step)`，宽不足则 0）。
    pub width: u32,
    /// 该遍像素高（行数，同上口径）。
    pub height: u32,
    /// 该遍单行原始字节数（**含**滤波先行字节）。
    pub row_bytes: usize,
}

impl PassExtent {
    /// 是否为空遍（宽或高为 0）。
    ///
    /// **空遍是合法语义**：1×1 的图有六遍为空。空遍不产生滤波行、不消耗
    /// raw 字节（锚点「宽高小于首遍步长的图合法但空遍跳过」）。
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }
    /// 非空遍的单行**载荷**字节数（不含滤波先行字节）——即该遍行缓冲尺寸。
    #[inline]
    pub fn payload_bytes(&self) -> usize {
        self.row_bytes.saturating_sub(1)
    }
    /// 该遍全部原始字节数（含每行的滤波先行字节）。
    #[inline]
    pub fn raw_bytes(&self) -> u64 {
        self.row_bytes as u64 * self.height as u64
    }
}

/// 推导一遍的尺寸（**唯一公式**；编码与解码共用）。
///
/// 公式：`width > start_col` 时 `ceil((width - start_col) / col_step)`，否则 0；
/// 行同理。**全程 `u32`**——`width > start_col` 的守卫已保证减法不下溢。
///
/// 遍宽的原始字节数与逐行同口径（`channels×depth×width` 向上取整到字节），
/// 再加 1 字节滤波先行。位深 <8 时按整字节对齐——规范 §9.6 明确遍内行
/// **独立按字节对齐起算，不继承全图行的比特相位**。
#[inline]
pub fn pass_extent(index: usize, width: u32, height: u32, head: &dec::Ihdr) -> Option<PassExtent> {
    let geom = pass_geom(index)?;
    let w = if width > geom.start_col {
        (width - geom.start_col).div_ceil(geom.col_step)
    } else {
        0
    };
    let h = if height > geom.start_row {
        (height - geom.start_row).div_ceil(geom.row_step)
    } else {
        0
    };
    let bits = w as u64 * head.channels() as u64 * head.depth as u64;
    let row_bytes = ((bits + 7) / 8) as usize + 1;
    Some(PassExtent { index, geom, width: w, height: h, row_bytes })
}

/// 推导全部七遍的尺寸（**遍调度器**的尺寸半程）。
pub fn all_pass_extents(width: u32, height: u32, head: &dec::Ihdr) -> [PassExtent; PASS_COUNT] {
    let mut out = [PassExtent {
        index: 0,
        geom: PassGeom { start_row: 0, start_col: 0, row_step: 1, col_step: 1 },
        width: 0,
        height: 0,
        row_bytes: 1,
    }; PASS_COUNT];
    for (i, slot) in out.iter_mut().enumerate() {
        if let Some(e) = pass_extent(i, width, height, head) {
            *slot = e;
        }
    }
    out
}

/// 遍内坐标 ↔ 全图坐标（**编码拆分与解码还原的唯一共用映射**）。
///
/// 锚点「拆分器与还原器共用同一常量表保证互逆」的具体落实：解码器写像素时
/// 调它，编码器取像素时也调它，**不存在两套算式**。返回 `None` 表示遍内
/// 坐标越界（显性拒绝，不夹取）。
#[inline]
pub fn pass_origin(geom: PassGeom, py: u32, px: u32) -> Option<(u32, u32)> {
    let y = geom.start_row.checked_add(py.checked_mul(geom.row_step)?)?;
    let x = geom.start_col.checked_add(px.checked_mul(geom.col_step)?)?;
    Some((y, x))
}

/// 七遍原始字节总数（不含空遍；解码时的 raw 缓冲需求）。
pub fn total_raw_bytes(width: u32, height: u32, head: &dec::Ihdr) -> u64 {
    let mut n = 0u64;
    for e in all_pass_extents(width, height, head) {
        if !e.is_empty() {
            n += e.raw_bytes();
        }
    }
    n
}

/// 非空遍数（1×1 图为 1；尺寸够大时为 7）。
pub fn nonempty_pass_count(width: u32, height: u32, head: &dec::Ihdr) -> usize {
    all_pass_extents(width, height, head).iter().filter(|e| !e.is_empty()).count()
}

/// 七遍像素覆盖总数（**不重不漏性**的自证口径：恒等于 `width × height`）。
///
/// 这是 [`ADAM7_PASSES`] 表正确性的**结构性证明入口**：若表中任一格被改错
/// （步长写错、起点重复），覆盖总数会偏离 `width×height`，自检当场变红。
pub fn covered_pixels(width: u32, height: u32) -> u64 {
    let mut n = 0u64;
    for e in all_pass_extents(
        width,
        height,
        &dec::Ihdr {
            width,
            height,
            depth: 8,
            color: dec::ColorType::Gray,
            compression: 0,
            filter_method: 0,
            interlace: 1,
        },
    ) {
        if !e.is_empty() {
            n += e.width as u64 * e.height as u64;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// 三、错误面（隔行自身故障 + 容器层故障透传）
// ---------------------------------------------------------------------------

/// 隔行专属故障类别。
///
/// **只收隔行自身的问题**——签名 / IHDR / CRC / 调色板长度等由
/// [`dec::PngFault`] 承载，此处不重复登记（避免两个错误面各说一半，
/// 调用方不知该查哪个）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Adam7FaultKind {
    /// 声明 `interlace = 1` 但调用的是逐行解码路径（或反之）。
    InterlaceMismatch,
    /// 遍内首字节的滤波类型不在 `0..=4`。
    BadFilter,
    /// 遍内行的滤波载荷短于按 IHDR 推导的行字节数。
    RowShort,
    /// 调色板越界索引（不夹取到末项——那会解出错图）。
    PaletteIndex,
    /// zlib 流损坏（展开失败或 Adler-32 不符）。
    Zlib,
    /// 输出缓冲不足。
    BufferShort,
    /// 输入缓冲不足（编码侧 RGBA 字节数不够）。
    InputShort,
    /// 尺寸非法（宽高为 0 / 超上限 / 位深×颜色类型非法组合）。
    Dimension,
    /// 像素总量超预算（F1015 溢出防护联动）。
    PixelBudget,
    /// 编码最坏输出尺寸超预算。
    EncodeBudget,
    /// 逐遍产出与推导量不一致（内部一致性防护——不是用户错误）。
    PassOverrun,
}

impl Adam7FaultKind {
    /// 错误码（隔行段独立码段：`0xF300 | n+1`）。
    ///
    /// **基数必须取`0xF300`**（F1001 用 `0xF100`、F1002 用 `0xF200`）。
    /// 曾误写为 `0xF103 | n+1` —— 低 4 位被常量`0x3` 污染，`|` 只置位不清位，
    /// 于是 `n=0..2` 全塌成 `0xF103`、`n=4..6` 全塌成 `0xF107`，
    /// **同一段内出现重复错误码**（自检C03-ERR-04 的「码无重复」判据当场
    /// 抓出）。故基数一律取低 4 位为 0 的整段起点。
    pub fn code(self) -> u16 {
        0xF300 | (self as u16) + 1
    }
    /// 简短中文名。
    pub fn label(self) -> &'static str {
        match self {
            Adam7FaultKind::InterlaceMismatch => "隔行标志与解码路径不符",
            Adam7FaultKind::BadFilter => "滤波号非法",
            Adam7FaultKind::RowShort => "遍内行长度不足",
            Adam7FaultKind::PaletteIndex => "调色板索引越界",
            Adam7FaultKind::Zlib => "zlib 流损坏",
            Adam7FaultKind::BufferShort => "输出缓冲不足",
            Adam7FaultKind::InputShort => "输入缓冲不足",
            Adam7FaultKind::Dimension => "尺寸非法",
            Adam7FaultKind::PixelBudget => "像素总量超预算",
            Adam7FaultKind::EncodeBudget => "编码输出超预算",
            Adam7FaultKind::PassOverrun => "逐遍产出口径不一致",
        }
    }
    /// 原因（为什么发生）。
    pub fn cause(self) -> &'static str {
        match self {
            Adam7FaultKind::InterlaceMismatch => "IHDR 声明的 interlace 值与所走解码路径不一致",
            Adam7FaultKind::BadFilter => "遍内首字节的滤波类型不在 0..=4",
            Adam7FaultKind::RowShort => "滤波载荷短于该遍按 IHDR 推导的行字节数",
            Adam7FaultKind::PaletteIndex => "调色板图存在越界索引（不夹取到末项）",
            Adam7FaultKind::Zlib => "zlib 流展开失败或 Adler-32 不符",
            Adam7FaultKind::BufferShort => "调用方缓冲小于本模块所需",
            Adam7FaultKind::InputShort => "调用方输入缓冲小于按宽高推导的最小尺寸",
            Adam7FaultKind::Dimension => "宽或高为 0，或位深×颜色类型组合不在 PNG 规范合法表内",
            Adam7FaultKind::PixelBudget => "宽×高×4 超过解码内存上限（防恶意尺寸）",
            Adam7FaultKind::EncodeBudget => "最坏情形输出尺寸超过编码预算",
            Adam7FaultKind::PassOverrun => "逐遍写出的字节数与 pass_extent 推导量不符",
        }
    }
    /// 建议（三要素之三）。
    pub fn advice(self) -> &'static str {
        match self {
            Adam7FaultKind::InterlaceMismatch => {
                "改走与 IHDR 声明一致的路径：interlace=1 用 decode_adam7，=0 用 F1001 的 decode"
            }
            Adam7FaultKind::PixelBudget => "降采样后重试，或按 F1013 内存治理走分块解码",
            Adam7FaultKind::EncodeBudget => "降低分辨率或位深；若确需大图请提高预算上限",
            Adam7FaultKind::PaletteIndex => "核对 PLTE 长度与索引位深是否匹配（索引上界须小于 PLTE 项数）",
            Adam7FaultKind::BadFilter => "按 PNG 规范核对遍内首字节，只允许 0..=4",
            Adam7FaultKind::Zlib => "按损坏处理——已完成的遍仍会输出（truncated 标记）",
            Adam7FaultKind::Dimension => "按 PNG 规范重写 IHDR 参数",
            _ => "按 PNG 规范与本单锚点核对隔行数据结构后重试",
        }
    }
}

/// 隔行故障（隔行自身故障带五元组；容器层故障**原样透传** [`dec::PngFault`]）。
///
/// **为何容器故障不复制一份**：F1001 的 `PngFault` 已带完整的
/// 原因/建议/人话/细节四元组。此处若另立一个 `Signature` / `CrcCritical`
/// 变体，就等于把同一套诊断文案写两遍——日后改文案必漏一处，且调用方会
/// 拿到两种形状的错误、不知该match 哪边。故用 `Container` 变体整体透传。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Adam7Fault {
    /// 隔行自身故障（类别 + 两个数值细节）。
    Adam7(Adam7FaultKind, u64, u64),
    /// 容器层故障（F1001 口径，原样透传以免诊断文案分叉）。
    Container(dec::PngFault),
}

impl Adam7Fault {
    /// 构造隔行故障。
    pub fn new(kind: Adam7FaultKind) -> Adam7Fault {
        Adam7Fault::Adam7(kind, 0, 0)
    }
    /// 附加两个数值（语义按类别解释，见 [`Adam7Fault::human`] 的分支）。
    pub fn with(self, a: u64, b: u64) -> Adam7Fault {
        match self {
            Adam7Fault::Adam7(k, _, _) => Adam7Fault::Adam7(k, a, b),
            other => other,
        }
    }
    /// 错误码（容器故障透传 F1001 的码）。
    pub fn code(&self) -> u16 {
        match self {
            Adam7Fault::Adam7(k, _, _) => k.code(),
            Adam7Fault::Container(p) => p.code(),
        }
    }
    /// 原因。
    pub fn cause(&self) -> &'static str {
        match self {
            Adam7Fault::Adam7(k, _, _) => k.cause(),
            Adam7Fault::Container(p) => p.cause(),
        }
    }
    /// 建议（三要素之三）。
    pub fn advice(&self) -> &'static str {
        match self {
            Adam7Fault::Adam7(k, _, _) => k.advice(),
            Adam7Fault::Container(p) => p.advice(),
        }
    }
    /// 人话（模板 + 三元细节，不含裸码）。
    pub fn human(&self) -> alloc::string::String {
        use alloc::string::String;
        match self {
            Adam7Fault::Container(p) => p.human(),
            Adam7Fault::Adam7(k, a, b) => {
                let mut s = String::new();
                match k {
                    Adam7FaultKind::BufferShort | Adam7FaultKind::InputShort => {
                        s.push_str(k.label());
                        s.push_str("：需 ");
                        s.push_str(&fmt_u64(*a));
                        s.push_str(" 字节，实得 ");
                        s.push_str(&fmt_u64(*b));
                        s.push_str(" 字节");
                    }
                    Adam7FaultKind::PixelBudget | Adam7FaultKind::EncodeBudget => {
                        s.push_str("尺寸被拒：需 ");
                        s.push_str(&fmt_u64(*a));
                        s.push_str(" 字节，上限 ");
                        s.push_str(&fmt_u64(*b));
                        s.push_str(" 字节");
                    }
                    Adam7FaultKind::PassOverrun => {
                        s.push_str("逐遍产出口径不一致：实得 ");
                        s.push_str(&fmt_u64(*a));
                        s.push_str(" 字节，推导值 ");
                        s.push_str(&fmt_u64(*b));
                        s.push_str(" 字节");
                    }
Adam7FaultKind::InterlaceMismatch => {
                s.push_str("IHDR 声明 interlace=");
                s.push_str(&fmt_u64(*a));
                s.push_str("，但隔行解码路径只接受 interlace=");
                s.push_str(&fmt_u64(*b));
            }
                    _ => {
                        s.push_str(k.label());
                    }
                }
                s
            }
        }
    }
}

/// 无 `format!` 的十进制渲染（与 F1001 同口径，内核 no_std 稳妥写法）。
fn fmt_u64(mut v: u64) -> alloc::string::String {
    use alloc::string::String;
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    for i in (0..n).rev() {
        s.push(buf[i] as char);
    }
    s
}

// ---------------------------------------------------------------------------
// 四、解码侧：遍调度器 + 七遍重排还原
// ---------------------------------------------------------------------------

/// 隔行解码结果（行级流水的回报）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InterlaceOutcome {
    /// **完整走完**的非空遍数（截断时 < `passes_total`）。
    pub passes_done: u32,
    /// 非空遍总数（尺寸决定，与 `passes_done` 语义不同：后者是实际走完的）。
    pub passes_total: u32,
    /// 交付给接收端的遍行总数（每个非空遍的每行各计一次）。
    pub rows_delivered: u64,
    /// 遍数据在完成前断掉（锚点：已完成的遍仍输出，置此位）。
    pub truncated: bool,
    /// 被接收端提前中止。
    pub aborted: bool,
    /// 实际写入整图的像素数（**小于** `width*height` 当且仅当 `truncated`）。
    pub pixels_set: u64,
    /// 反滤波走标量路径的行数。
    pub scalar_rows: u32,
    /// 反滤波走 SIMD 路径的行数。
    pub simd_rows: u32,
}

/// 遍行接收端（隔行行级输出的唯一出口）。
///
/// **与 F1001 的 [`dec::RowSink`] 形状不同但刻意如此**：`on_pass_row` 多带
/// `pass` / `py` / `cols` 三项——F1018（七遍渐进显示）需要知道「这一遍的
/// 这一行铺到全图的哪些列」才能做正确的低优先级合并更新；F1019（缩略图
/// 快路径）需要 `pass` 判断「前 3 遍已构成 1/8 完整图，可以停了」。
/// 提供 [`PassSinkToRows`] 适配器把遍行汇成整行，不关心遍的下游零改动复用。
pub trait PassSink {
    /// 接收一遍的一行。
    ///
    /// - `pass`：遍序号 0..7；
    /// - `py`：遍内行号；
    /// - `y`：该行在全图中的行号；
    /// - `cols`：该遍行覆盖的**全图列号**（长度 = 遍宽）；
    /// - `rgba`：该遍行的 RGBA8（长度 = 遍宽 × 4）。
    ///
    /// 返回 `false` 表示要求提前中止。
    fn on_pass_row(&mut self, pass: u8, py: u32, y: u32, cols: &[u32], rgba: &[u8]) -> bool;
}

/// 空接收端（丢弃全部遍行——结构校验与性能测量用）。
#[derive(Clone, Copy, Debug, Default)]
pub struct NullSink;

impl PassSink for NullSink {
    fn on_pass_row(&mut self, _pass: u8, _py: u32, _y: u32, _cols: &[u32], _rgba: &[u8]) -> bool {
        true
    }
}

/// 把遍行汇成整行后转交 [`dec::RowSink`] 的适配器。
///
/// **存在的理由**：F1007（流式）、F1013（内存治理）等下游已按 [`dec::RowSink`]
/// 写好。若隔行路径不提供适配器，下游就得各自重写一遍「等齐整行再输出」的
/// 逻辑——而那段逻辑要处理「一个全图行被 4 个遍分批填充」的半成品态，是
/// 易错点。收口在此一处，下游零改动复用。
///
/// **半成品行的处置**：[`dec::RowSink`] 的契约是「一行 RGBA，长度 = 全图宽×4」。
/// Adam7 里一个全图行会被多个遍分批填满，故本适配器**只在整行被完全填满
/// 时才交付**；未填满的行**不交付也不报错**——那不是错误，只是「这一行还没
/// 被任何遍凑齐」。中途交付半行会让下游把未初始化的内存当像素（真错，
/// 且不报错）。计数按行独立（`row_filled[y]`），**不按列**——列计数在多行
/// 交错时会互相污染。
///
/// **不重不漏的依据**：Adam7 七遍对像素的划分是**划分**（partition），故
/// 同一行内不同遍覆盖的列**互不相交**，`row_filled[y]` 单调累加不会重复计数。
///
/// **自检怎么证的（如实说明，不写没做的事）**：`row_filled` 是私有字段，
/// 外部验不了，故C03-DEC-03 不去断言它，而是断言**可观测的等价后果**——
/// ① 每个全图行**恰好交付一次**（`got.len() == height` 且无重复 `y`，
/// 重复交付会让计数超）；② 交付出去的整行与源**逐字节一致**（少交付一列
/// 或交付半行都会立刻不符）。V5 变体（退回共享单缓冲）实测**只让这一项
/// 变红**，说明该判据确实在判「整行交付」这件事本身。
///
/// ## 峰值内存：为什么是整图缓冲（**曾经的缺陷，已由自检 C03-DEC-03 抓出**）
///
/// **错法**：本适配器最初只持有一个 `row_buf`（宽×4），指望「某行凑齐的那
/// 一瞬间把它交出去」。这在**逐行**解码里成立，在 Adam7 里**不成立**。
///
/// **为什么**：Adam7 的行完成顺序是**交错的**。以 24×24 为例，第 5 遍
/// （`start_row=0, row_step=2`）收尾所有偶数行，第 6 遍（`start_row=1,
/// `row_step=2`）收尾所有奇数行。轮到第 5 遍交付第 0 行时，第 2 行的**偶数列**
/// 早在第 2/4 遍就写进了那个共享缓冲——**而那位置此刻装的是第 0 行的数据**。
/// 于是第 2 行交付时拿到的是「第 0 行的偶数列 + 第 2 行的奇数列」的混合行：
/// **逐像素错、长度正确、不报任何错**（实测 24×24 有 132 个像素错、其余判据全绿）。
///
/// **结论（不是可以绕过的取舍）**：一个像素要等**最后一个覆盖它的遍**才能交付，
/// 而 Adam7 里「最后一个遍」的序号随行变化（第 5 遍收尾偶数行、第 6 遍收尾奇数行），
/// 故**任意时刻都同时有大量行处于半成品态**——单缓冲在结构上不可能成立。
/// 正确实现必须**按行各持一份累加缓冲**（即整图 `宽×高×4`）。
///
/// **与「隔行的内存优势」如何共存（如实说明，不回避）**：
/// - 真正的低峰值路径是 [`PassSink`] API 本身——它**每遍行立即交付**，
///   调用方要低峰值就自己攒进目标图或不攒（渐进显示只需前 3 遍），
///   峰值由调用方的攒放策略决定，模块侧恒为 O(宽×4)（见
///   [`decode_scratch_need`]）；
/// - 本适配器是**给已按 [`dec::RowSink`] 写好的下游用的兼容件**（F1007 流式、
///   F1013 内存治理），它换来的是「下游零改动」，代价是峰值 O(宽×高×4)。
///   **谁要低峰值就用 [`PassSink`]，不要用本适配器**——这一句写在这里，
///   是为了防止下游误以为本适配器也是低峰值的。
pub struct PassSinkToRows<'r> {
    /// 下游行接收端。
    pub inner: &'r mut dyn dec::RowSink,
    /// 每行已填列数（长度 = height）。
    row_filled: Vec<u32>,
    /// 每行是否已交付（长度 = height）。
    delivered: Vec<bool>,
    /// 全图宽（像素）。
    width: u32,
    /// **按行累加缓冲**（宽 × 高 × 4）——交付时按行切片，天然是连续的一行。
    ///
    /// 必须是整图而非单行，理由见类型文档的「峰值内存」段。
    acc: Vec<u8>,
}

impl<'r> PassSinkToRows<'r> {
    /// 为 `width × height` 的图构造适配器。
    pub fn new(width: u32, height: u32, inner: &'r mut dyn dec::RowSink) -> PassSinkToRows<'r> {
        let w = width.max(1) as usize;
        let h = height.max(1) as usize;
        PassSinkToRows {
            inner,
            row_filled: vec![0u32; h],
            delivered: vec![false; h],
            width,
            acc: vec![0u8; w * h * 4],
        }
    }
    /// 已完整交付的全图行数（供调用方核对交付集合）。
    pub fn rows_delivered(&self) -> u32 {
        self.delivered.iter().filter(|d| **d).count() as u32
    }
    /// 全部行是否都已交付（截断时为 `false`）。
    pub fn complete(&self) -> bool {
        self.delivered.iter().all(|d| *d)
    }
}

impl PassSink for PassSinkToRows<'_> {
    fn on_pass_row(&mut self, _pass: u8, _py: u32, y: u32, cols: &[u32], rgba: &[u8]) -> bool {
        let yi = y as usize;
        let w = self.width as usize;
        if yi >= self.row_filled.len() {
            return true;
        }
        let base = yi * w * 4;
        for (i, &c) in cols.iter().enumerate() {
            let cx = c as usize;
            if cx >= w {
                continue;
            }
            let o = base + cx * 4;
            // rgba 长度不足时跳过该像素：不越界读，也不谎报已填
            if i * 4 + 4 > rgba.len() || o + 4 > self.acc.len() {
                continue;
            }
            self.acc[o..o + 4].copy_from_slice(&rgba[i * 4..i * 4 + 4]);
            self.row_filled[yi] += 1;
        }
        // 整行凑齐才交付（半行交付 = 把未初始化内存当像素）。
        // 交付的是**该行自己的切片**——共享单缓冲会把别的行的像素交出去。
        if self.row_filled[yi] >= self.width && !self.delivered[yi] {
            self.delivered[yi] = true;
            let rs = base;
            let re = base + w * 4;
            if re <= self.acc.len() && !self.inner.on_row(y, &self.acc[rs..re]) {
                return false;
            }
        }
        true
    }
}

/// 解码工作区的峰值字节需求（预算闸用；F1013 内存治理的输入）。
///
/// 同一时刻只有**一遍**的两个行缓冲 + 一行 RGBA + 列号表存活，故峰值 =
/// `max_over_passes(payload×2 + width×4) + max_cols×4`。**取遍最大值而非
/// 求和**——这是隔行相对逐行唯一的内存优势项，值得让下游知道。
pub fn decode_scratch_need(width: u32, height: u32, head: &dec::Ihdr) -> u64 {
    let mut max_buf = 0u64;
    for e in all_pass_extents(width, height, head) {
        if e.is_empty() {
            continue;
        }
        let n = e.payload_bytes() as u64 * 2 + e.width as u64 * 4;
        if n > max_buf {
            max_buf = n;
        }
    }
    max_buf + width as u64 * 4
}

/// zlib 故障到隔行故障的映射（逐变体分流，不吞错）。
fn map_zlib(e: mech_inflate::PngError) -> Adam7Fault {
    match e {
        mech_inflate::PngError::ScratchSize { need, got } => {
            Adam7Fault::new(Adam7FaultKind::RowShort).with(need as u64, got as u64)
        }
        mech_inflate::PngError::BadPalette => Adam7Fault::new(Adam7FaultKind::PaletteIndex),
        _ => Adam7Fault::new(Adam7FaultKind::Zlib),
    }
}

/// 造一个「遍头」：把 [`dec::Ihdr`] 的宽高换成该遍尺寸，其余字段不变。
///
/// **为什么可以这样做**：[`dec::expand_row_rgba`] 只读 `Ihdr` 的
/// `width` / `depth` / `color` / `channels`，因此给它一个「同格式、窄一点」
/// 的头，它就会正确地把该遍行展开为「遍宽 × 4」的 RGBA——**位深、颜色
/// 类型、调色板、tRNS 语义全部自动与全图一致**，无需在隔行侧重写一遍色彩
/// 展开（重写即分叉：F1001 日后修一个色彩 bug，隔行侧不会跟着修）。
#[inline]
fn pass_head(head: &dec::Ihdr, e: &PassExtent) -> dec::Ihdr {
    dec::Ihdr { width: e.width, height: e.height, ..*head }
}

/// 解码隔行 PNG：七遍重排还原，逐遍行交付（锚点主流程 · 解码侧）。
///
/// 步骤：容器解析（放行 `interlace=1`）→ 尺寸与预算闸 → zlib 展开 IDAT →
/// 逐遍（剥滤波先行字节 → 反滤波 → 色彩展开 → 遍行交付）。
///
/// **滤波参照的逐遍重置**（设计要点二）：每遍开始时 `prev` 填零，遍首行向
/// 上游传 `None`。因此「遍内上一行」与「全图 `y - row_step` 行」这两个
/// 语义在本实现里**根本不会分岔**——不是靠算对，而是靠不引入那条分支。
///
/// **截断语义**（设计要点四）：行数据不足时停在该遍，已完成的遍**已交付**，
/// 置 `truncated`。输出是「像素分布不均的图」而非半张烂图，且如实上报。
pub fn decode_adam7(file: &[u8], sink: &mut dyn PassSink) -> Result<InterlaceOutcome, Adam7Fault> {
    let parsed = dec::parse_container_ex(file, true).map_err(Adam7Fault::Container)?;
    let c = parsed.container;
    let head = c.ihdr;
    // 路径与IHDR 声明必须一致（隔行解码器不接逐行图，反之亦然）
    if head.interlace != 1 {
        return Err(Adam7Fault::new(Adam7FaultKind::InterlaceMismatch).with(head.interlace as u64, 1));
    }
    dec::check_pixel_budget(&head).map_err(Adam7Fault::Container)?;

    let extents = all_pass_extents(head.width, head.height, &head);
    let need = total_raw_bytes(head.width, head.height, &head) as usize;
    let mut raw = vec![0u8; need];
    let got = mech_inflate::zlib_inflate_slices(&[&c.idat], &mut raw).map_err(map_zlib)?;

    let bpp = head.filter_bpp();
    let mut isa = imgsimd::detect_isa();
    let mut off = 0usize;
    let mut outcome = InterlaceOutcome {
        passes_done: 0,
        passes_total: nonempty_pass_count(head.width, head.height, &head) as u32,
        rows_delivered: 0,
        truncated: false,
        aborted: false,
        pixels_set: 0,
        scalar_rows: 0,
        simd_rows: 0,
    };

    'passes: for e in extents.iter() {
        if e.is_empty() {
            continue;
        }
        let pb = e.payload_bytes();
        let stride = e.row_bytes;
        let pw = e.width as usize;
        // 每遍**独立**行缓冲（锚点「每遍的行缓冲独立」）
        let mut cur = vec![0u8; pb];
        let mut prev = vec![0u8; pb];
        let mut rgba = vec![0u8; pw * 4];
        // 该遍行覆盖的全图列号（与遍内列号一一对应，与 py 无关）
        let mut cols = vec![0u32; pw];
        for (px, slot) in cols.iter_mut().enumerate() {
            match pass_origin(e.geom, 0, px as u32) {
                Some((_, x)) => *slot = x,
                None => return Err(Adam7Fault::new(Adam7FaultKind::PassOverrun).with(e.index as u64, pw as u64)),
            }
        }
        // 遍首行的参照是零行（设计要点二）
        prev[..pb].fill(0);
        let ph = pass_head(&head, e);

        for py in 0..e.height {
            if off + stride > got {
                // 遍数据截断 → 已完成的遍已交付，如实置位（设计要点四）
                outcome.truncated = true;
                break 'passes;
            }
            let line = &raw[off..off + stride];
            off += stride;
            // 滤波先行字节：非空遍的 `row_bytes >= 2`（载荷至少 1 字节），
            // 故 `line[0]` 恒存在。此处仍走 `get` + 显性拒绝，
            // 而非裸下标——零 panic 面要求把「恒真」写成「可读」（索引越界即panic）。
            let filter = match line.first() {
                Some(&f) => f,
                None => {
                    return Err(Adam7Fault::new(Adam7FaultKind::RowShort)
                        .with(e.index as u64, stride as u64));
                }
            };
            let n = pb.min(cur.len());
            cur[..n].copy_from_slice(&line[1..1 + n]);
            // **始终传 Some(&prev)，不传 None**——遍首行的 `prev` 已在
            // 本遍开始时填零，故 `Some(零行)` 正是规范要求的「虚拟零行」语义。
            //
            // **不可传 None**（曾经的实现缺陷，已由自检 C03-ENC-01 抓出）：
            // 上游 `imgsimd::unfilter_scalar` 的 Up 分支写作 `let prev = prev?;`
            // ——`None` 会让**整个反滤波失败并返回 None**，被上层读成
            // 「滤波号非法」。而编码侧遍首行的 `prev` 是零行，`choose_filter`
            // 会算出「Up 的残差全为 0」而**合法地选中Up**，于是首行解码必崩。
            // F1001 同样传 `Some(&prev_buf)`（零行），本模块与之对齐。
            let used = imgsimd::unfilter_auto(isa, filter, bpp, Some(&prev[..n]), &mut cur[..n])
                .ok_or_else(|| Adam7Fault::new(Adam7FaultKind::BadFilter).with(py as u64, filter as u64))?;
            isa = used;
            if used == imgsimd::Isa::Scalar {
                outcome.scalar_rows += 1;
            } else {
                outcome.simd_rows += 1;
            }
            // 参照行推进：下一遍行看到的是本遍行的**重建**样本
            prev[..n].copy_from_slice(&cur[..n]);
            // 色彩展开（复用 F1001，遍头窄化后天然按遍宽输出）
            if dec::expand_row_rgba(&cur[..n], &ph, c.palette.as_ref(), &c.transparency, &mut rgba)
                .is_none()
            {
                return Err(Adam7Fault::new(Adam7FaultKind::PaletteIndex).with(e.index as u64, py as u64));
            }
            let (y, _) = pass_origin(e.geom, py, 0)
                .ok_or_else(|| Adam7Fault::new(Adam7FaultKind::PassOverrun).with(e.index as u64, py as u64))?;
            // 该遍行确已交付：先计数再判中止（否则出现「已收N 行但计数为 N-1」
            // 的自相矛盾）
            outcome.rows_delivered += 1;
            outcome.pixels_set += e.width as u64;
            if !sink.on_pass_row(e.index as u8, py, y, &cols, &rgba) {
                outcome.aborted = true;
                break 'passes;
            }
        }
        // 该遍完整走完（未因截断/中止跳出）才计入 passes_done
        outcome.passes_done += 1;
    }
    Ok(outcome)
}

/// 收集式遍接收端（把遍行写进整图RGBA；[`decode_adam7_full`] 内部用）。
struct Collector<'r> {
    /// 目标整图缓冲（宽 × 高 × 4）。
    data: &'r mut [u8],
    /// 全图宽（像素）。
    width: usize,
    /// 全图高（像素）。
    height: usize,
}

impl PassSink for Collector<'_> {
    fn on_pass_row(&mut self, _pass: u8, _py: u32, y: u32, cols: &[u32], rgba: &[u8]) -> bool {
        let yi = y as usize;
        if yi >= self.height {
            return true;
        }
        for (i, &c) in cols.iter().enumerate() {
            let xi = c as usize;
            if xi >= self.width || i * 4 + 4 > rgba.len() {
                continue;
            }
            let o = (yi * self.width + xi) * 4;
            if o + 4 > self.data.len() {
                return false;
            }
            self.data[o..o + 4].copy_from_slice(&rgba[i * 4..i * 4 + 4]);
        }
        true
    }
}

/// 解码隔行 PNG 到整图 RGBA（便利形：内部自分配并收集）。
///
/// **便利形不做部分交付**：截断即带标记的故障（行级 API 才做部分输出）——
/// 与 F1001 的 [`dec::decode`] 同一取舍。调用方要部分输出请用
/// [`decode_adam7`] 配自己的 [`PassSink`]。
///
/// 未被任何遍覆盖的像素保持 0（Adam7 下不存在未覆盖像素，除非截断——而截断
/// 在此已转为故障）。
pub fn decode_adam7_full(file: &[u8]) -> Result<(dec::Ihdr, Vec<u8>), Adam7Fault> {
    let head = dec::parse_container_ex(file, true).map_err(Adam7Fault::Container)?.container.ihdr;
    dec::check_pixel_budget(&head).map_err(Adam7Fault::Container)?;
    let mut rgba = vec![0u8; head.rgba_bytes() as usize];
    let outcome = {
        let mut col = Collector { data: &mut rgba, width: head.width as usize, height: head.height as usize };
        decode_adam7(file, &mut col)?
    };
    if outcome.truncated {
        return Err(Adam7Fault::new(Adam7FaultKind::RowShort)
            .with(outcome.passes_done as u64, outcome.passes_total as u64));
    }
    Ok((head, rgba))
}

// ---------------------------------------------------------------------------
// 五、编码侧：全图 → 七遍拆分
// ---------------------------------------------------------------------------

/// 隔行编码统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Adam7EncStats {
    /// 各遍产出的原始字节数（长度 = 7，空遍为 0）。
    pub pass_raw: [u64; PASS_COUNT],
    /// 跳过的空遍数。
    pub empty_passes: u32,
    /// 各滤波号被选中的遍行数（下标 = 滤波号）。
    pub filter_rows: [u32; 5],
    /// 滤波后、送入压缩前的原始字节数合计。
    pub raw_bytes: u64,
    /// zlib 流字节数。
    pub compressed_bytes: u64,
    /// 最终 PNG 总字节数。
    pub total_bytes: u64,
    /// 非空遍数。
    pub passes: u32,
}

/// 隔行编码产物。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Adam7Encoded {
    /// 实际写入字节数。
    pub len: usize,
    /// 实际颜色类型线值。
    pub color: u8,
    /// 实际位深。
    pub depth: u8,
    /// 统计。
    pub stats: Adam7EncStats,
}

/// 把 RGBA8 全图拆成 Adam7 七遍并编码为**隔行** PNG。
///
/// 与 [`enc::encode_rgba`]（逐行）的差异：产出 `interlace = 1` 的 IHDR，
/// 且像素按遍网格重排。**其余一切（滤波策略、压缩级别、IDAT 分块、CRC）
/// 完全复用 F1002 的实现**——本函数不重写任何块组装或压缩逻辑。
///
/// **默认不隔行的落实**（设计要点五）：本函数是**显式调用**入口。F1002 的
/// [`enc::encode_rgba`] 默认仍写 `interlace = 0`；本模块**不修改**其默认
/// 行为。隔行开关归 F1008（写入器选项面），那里默认 `false`，用户显式
/// 要求渐进时才转到本函数。
///
/// **滤波语义**（锚点「每个独立 pass 独立滤波」）：每遍各自把 `prev` 初始化
/// 为零行，遍内行间按 [`enc::EncCtx::filter_row`] 的规范语义施加滤波。
/// `bpp` 取 [`dec::Ihdr::filter_bpp`]（**与遍无关**——见头注设计要点二）。
///
/// **调色板不支持**（与 [`enc::encode_rgba`] 同一处置）：RGBA 输入无法表达
/// 调色板索引，索引流隔行编码属F1002 `encode_palette` 的职责，此处显式
/// 拒绝而不静默降级为灰度（那会让索引图整体错色）。
pub fn encode_adam7(
    rgba: &[u8],
    width: usize,
    height: usize,
    color: enc::ColorType,
    depth: u8,
    opts: enc::EncOptions,
    out: &mut [u8],
) -> Result<Adam7Encoded, Adam7Fault> {
    // ---- 参数校验（拒绝表与 F1002 同口径，逐项不省） ----
    if width == 0 || height == 0 || width as u64 > dec::MAX_DIM as u64 || height as u64 > dec::MAX_DIM as u64
    {
        return Err(Adam7Fault::new(Adam7FaultKind::Dimension).with(width as u64, height as u64));
    }
    if !matches!(depth, 1 | 2 | 4 | 8 | 16) {
        return Err(Adam7Fault::new(Adam7FaultKind::Dimension).with(depth as u64, 8));
    }
    if !enc::combo_is_legal(color, depth) {
        return Err(Adam7Fault::new(Adam7FaultKind::Dimension).with(color.wire() as u64, depth as u64));
    }
    if color == enc::ColorType::Palette {
        return Err(Adam7Fault::new(Adam7FaultKind::Dimension).with(color.wire() as u64, 0));
    }
    if opts.level == 0 || opts.level > 9 {
        return Err(Adam7Fault::new(Adam7FaultKind::EncodeBudget).with(opts.level as u64, 9));
    }
    let pixels = (width as u64)
        .checked_mul(height as u64)
        .ok_or_else(|| Adam7Fault::new(Adam7FaultKind::EncodeBudget))?;
    if (rgba.len() as u64) < pixels * 4 {
        return Err(Adam7Fault::new(Adam7FaultKind::InputShort).with(pixels * 4, rgba.len() as u64));
    }
    let raw_total = adam7_raw_total(width, height, color, depth);
    let wc = adam7_worst_case_out(raw_total);
    if wc > enc::ENCODE_BUDGET_BYTES {
        return Err(Adam7Fault::new(Adam7FaultKind::EncodeBudget).with(wc, enc::ENCODE_BUDGET_BYTES));
    }

    // ---- 头信息（借F1001 的 Ihdr 作推导，不经容器解析） ----
    let head = dec::Ihdr {
        width: width as u32,
        height: height as u32,
        depth,
        color: dec_color_of(color),
        compression: 0,
        filter_method: 0,
        interlace: 1,
    };
    let extents = all_pass_extents(head.width, head.height, &head);

    // ---- 逐遍拆分 ----
    let mut raw: Vec<u8> = Vec::new();
    raw.reserve(raw_total as usize);
    let mut stats = Adam7EncStats::default();
    let bpp = head.filter_bpp();
    let ch = color.channels();
    let full_payload = enc::row_need(width, color, depth);
    let mut cur = vec![0u8; full_payload];
    let mut prev = vec![0u8; full_payload];
    // `trial`：滤波试算的纯暂存（`spec_choose_filter` 返回后是残值，不可当快照）
    let mut trial = vec![0u8; full_payload];
    // `snap`：本行**原样本**快照，用于推进 `prev`。
    // **必须与 `trial` 分开**：共用一个缓冲会让 `prev` 拿到残值，
    // 下一遍行的 Up/Average/Paeth 参照整体错且不报错（实测 alpha 恒差 1）。
    let mut snap = vec![0u8; full_payload];

    for e in extents.iter() {
        if e.is_empty() {
            stats.empty_passes += 1;
            continue;
        }
        let pb = e.payload_bytes();
        if pb == 0 || pb > full_payload {
            return Err(Adam7Fault::new(Adam7FaultKind::PassOverrun).with(e.index as u64, pb as u64));
        }
        // 每遍**独立**滤波：遍首行的参照是零行（设计要点二的表格第 2 行）
        prev[..pb].fill(0);
        let before = raw.len();
        for py in 0..e.height {
            let (y, _) = pass_origin(e.geom, py, 0)
                .ok_or_else(|| Adam7Fault::new(Adam7FaultKind::PassOverrun).with(e.index as u64, py as u64))?;
            let yi = y as usize;
            let s0 = yi * width * 4;
            let src_row = rgba.get(s0..s0 + width * 4).unwrap_or(&[]);
            let got = gather_pass_row(src_row, width, e, ch, color, depth, &mut cur[..pb]);
            if got != pb {
                return Err(Adam7Fault::new(Adam7FaultKind::PassOverrun).with(e.index as u64, got as u64));
            }
            // 滤波（选 + 施加 + 推进参照行）原子做完。
            // **自持规范实现**，不复用 enc::EncCtx——理由见头注设计要点六
            // （F1002 的 apply_filter 符号方向与 PNG 规范相反）。
            {
                // **顺序要紧**：`snap ← cur`（快照本行原样本），再让
                // `spec_choose_filter` 借用**另一个**缓冲 `trial` 作试算。
                // 两个方向的错法都致命：
                // - `cur ← snap`：用陈旧内容**覆盖掉刚打包好的本行** → 整行零；
                // - `prev ← trial`：推进的是最后一个候选的**残值**而非原样本
                //   → 下一行 Up/Average/Paeth 参照整体错且不报错。
                snap[..pb].copy_from_slice(&cur[..pb]);
                // 策略选择：FixedNone 直接不滤波（体积基线/快速模式）；
                // 其余两策略在本模块自持的绝对差和评分下同路——因为评分
                // 打在**本模块自己的滤波输出**上，而 F1002 的评分器对着
                // 符号方向相反的候选打分，移植过来只会继承缺陷
                // （见头注设计要点六）。MinEntropy 与 MinAbsSum 的区分由
                // F1008 选项面承接，本单按规范语义优先保证正确性。
                let pick = match opts.strategy {
                    enc::Strategy::FixedNone => 0u8,
                    _ => spec_choose_filter(&cur[..pb], &prev[..pb], bpp, &mut trial[..pb]),
                };
                if !spec_apply_filter(pick, &mut cur[..pb], &prev[..pb], bpp) {
                    return Err(Adam7Fault::new(Adam7FaultKind::PassOverrun).with(e.index as u64, pb as u64));
                }
                prev[..pb].copy_from_slice(&snap[..pb]); // 推进参照行（本行原样本）
                stats.filter_rows[pick as usize] += 1;
                raw.push(pick);
                raw.extend_from_slice(&cur[..pb]);
            }
        }
        stats.pass_raw[e.index] = (raw.len() - before) as u64;
        stats.passes += 1;
    }
    stats.raw_bytes = raw.len() as u64;
    if stats.raw_bytes != raw_total {
        // 内部一致性：拆分产物必须与推导量一致。不一致即说明 pass_extent 的
        // 推导与 gather/filter 的实际写入有一处口径不同——**这不是用户错误**，
        // 故归PassOverrun 而非 Dimension。
        return Err(Adam7Fault::new(Adam7FaultKind::PassOverrun).with(stats.raw_bytes, raw_total));
    }

    // ---- 压缩与块组装（完全复用 F1002） ----
    let mut zbuf = vec![0u8; raw.len() + raw.len() / 8 + 4096];
    let zlen = enc::compress_zlib(opts.level, &raw, &mut zbuf)
        .map_err(|e| Adam7Fault::new(Adam7FaultKind::EncodeBudget).with(e.need, e.got))?;
    stats.compressed_bytes = zlen as u64;

    if out.len() < 8 {
        return Err(Adam7Fault::new(Adam7FaultKind::BufferShort).with(8, out.len() as u64));
    }
    out[..8].copy_from_slice(&dec::PNG_SIG);
    let mut pos = 8usize;
    let mut ihdr = [0u8; 13];
    ihdr[..4].copy_from_slice(&(width as u32).to_be_bytes());
    ihdr[4..8].copy_from_slice(&(height as u32).to_be_bytes());
    ihdr[8] = depth;
    ihdr[9] = color.wire();
    ihdr[12] = 1; // 隔行标志——与逐行路径唯一的 IHDR 差异
    pos += enc::write_chunk(out, pos, b"IHDR", &ihdr)
        .map_err(|e| Adam7Fault::new(Adam7FaultKind::BufferShort).with(e.need, e.got))?;
    let cs = if opts.idat_chunk == 0 || opts.idat_chunk > enc::IDAT_CHUNK_MAX {
        enc::IDAT_CHUNK_MAX
    } else {
        opts.idat_chunk
    };
    for part in zbuf[..zlen].chunks(cs) {
        pos += enc::write_chunk(out, pos, b"IDAT", part)
            .map_err(|e| Adam7Fault::new(Adam7FaultKind::BufferShort).with(e.need, e.got))?;
    }
    pos += enc::write_chunk(out, pos, b"IEND", &[])
        .map_err(|e| Adam7Fault::new(Adam7FaultKind::BufferShort).with(e.need, e.got))?;
    stats.total_bytes = pos as u64;
    Ok(Adam7Encoded { len: pos, color: color.wire(), depth, stats })
}

/// 七遍原始字节总量（编码侧容量推导口径，与解码侧 [`total_raw_bytes`] 同源）。
fn adam7_raw_total(width: usize, height: usize, color: enc::ColorType, depth: u8) -> u64 {
    let head = dec::Ihdr {
        width: width as u32,
        height: height as u32,
        depth,
        color: dec_color_of(color),
        compression: 0,
        filter_method: 0,
        interlace: 1,
    };
    total_raw_bytes(head.width, head.height, &head)
}

/// 隔行口径的最坏输出尺寸（**不可沿用逐行的 `worst_case_out`**）。
///
/// 逐行公式把 raw 定为 `h × row_bytes(width)`；隔行多出每个非空遍首行的
/// 滤波先行字节，故 raw 必须按 [`adam7_raw_total`] 算。沿用旧公式会低估
/// 最多 `7 × (row_bytes - 1)` 字节，在小块高图上足以让 `write_chunk` 在
/// 最后一刻报缓冲不足——**一个只在特定尺寸触发的偶发失败**。
fn adam7_worst_case_out(raw_total: u64) -> u64 {
    let deflate_max = raw_total + raw_total / 8 + 64;
    let zlib = 2 + deflate_max + 4;
    let blocks = zlib.div_ceil(enc::IDAT_CHUNK_MAX as u64).max(1);
    8 + 25 + blocks * 12 + zlib + 12
}

/// F1002 颜色类型 → F1001 颜色类型（隔行推导需要 [`dec::Ihdr`] 结构）。
///
/// **显式逐变体映射而非 `as u8` 强转**：两个枚举的判别值相同是当前事实，
/// 但那是巧合而非契约——一旦任一侧新增变体，强转会静默产出错颜色类型。
fn dec_color_of(c: enc::ColorType) -> dec::ColorType {
    match c {
        enc::ColorType::Gray => dec::ColorType::Gray,
        enc::ColorType::Rgb => dec::ColorType::Rgb,
        enc::ColorType::Palette => dec::ColorType::Palette,
        enc::ColorType::GrayAlpha => dec::ColorType::GrayAlpha,
        enc::ColorType::Rgba => dec::ColorType::Rgba,
    }
}

/// 从全图源行按遍网格抽取并打包一遍行（编码侧的「跨步取样」）。
///
/// **为什么不能「先打包全图行再切片」**：位深 <8 时全图行的比特相位由**全图
/// 列号**决定，而遍内行的相位必须从遍首列**重新起算**（规范 §9.6：遍内行
/// 独立按字节对齐）。对 1/2/4 位深灰度图，先切后打包与先打包后切给出
/// **不同的字节序列**——这是隔行低位深最隐蔽的一类错，且 8 位图完全看不出来。
///
/// 故本函数直接从 RGBA 源逐像素取样打包，绕开相位问题。
///
/// 返回写入的字节数（= 遍载荷字节数）；缓冲不足或越界时返回 0。
fn gather_pass_row(
    src_row: &[u8],
    full_width: usize,
    e: &PassExtent,
    ch: usize,
    color: enc::ColorType,
    depth: u8,
    out: &mut [u8],
) -> usize {
    let pw = e.width as usize;
    let need = row_payload_bytes(pw, ch, depth);
    if out.len() < need {
        return 0;
    }
    for i in 0..need {
        out[i] = 0;
    }
    if depth < 8 {
        // 低位深：每字节塞 8/depth 个样本，高位在左（与 F1002 的
        // pack_gray_low 同序），且**相位从遍首列重新起算**
        let per = 8 / depth as usize;
        for px in 0..pw {
            let (_, x) = match pass_origin(e.geom, 0, px as u32) {
                Some(v) => v,
                None => return 0,
            };
            let xi = x as usize;
            if xi >= full_width {
                return 0;
            }
            // 低位深只出现在单通道灰度（调色板已显式拒绝）
            let s = enc_sample_at(src_row, xi, color, 0);
            let v = s >> (8 - depth);
            let byte = px / per;
            if byte >= out.len() {
                return 0;
            }
            out[byte] |= v << (8 - depth as usize * (px % per + 1));
        }
        return need;
    }
    // 8/16 位：每样本 1 或 2 字节（16 位大端取高字节、低字节补 0，
    // 与 F1002 的 pack_channels 及 F1001 解码侧 `(v >> 8)` 三方对偶）
    let bps = if depth == 16 { 2 } else { 1 };
    for px in 0..pw {
        let (_, x) = match pass_origin(e.geom, 0, px as u32) {
            Some(v) => v,
            None => return 0,
        };
        let xi = x as usize;
        if xi >= full_width {
            return 0;
        }
        for c in 0..ch {
            let s = enc_sample_at(src_row, xi, color, c);
            let o = px * ch * bps + c * bps;
            if o + bps > out.len() {
                return 0;
            }
            out[o] = s;
            if bps == 2 {
                out[o + 1] = 0;
            }
        }
    }
    need
}

/// 遍载荷字节数（编码侧打包与解码侧 `PassExtent::payload_bytes` 必须同口径）。
#[inline]
fn row_payload_bytes(pass_width: usize, ch: usize, depth: u8) -> usize {
    let bits = (pass_width as u64) * (ch as u64) * (depth as u64);
    ((bits + 7) / 8) as usize
}

/// 取源行的第 `x` 像素第 `c` 通道样本（灰度类首通道取 BT.601 亮度）。
///
/// 与 F1002 的同名私有函数同口径；此处独立实现而非让 F1002 放 `pub`——
/// 避免为了一处调用而扩大上游模块的公开面（公开面一旦扩大就成了契约）。
#[inline]
fn enc_sample_at(row: &[u8], x: usize, color: enc::ColorType, c: usize) -> u8 {
    let s = x * 4;
    let is_alpha = matches!(color, enc::ColorType::Rgba) && c == 3
        || matches!(color, enc::ColorType::GrayAlpha) && c == 1;
    if is_alpha {
        return row.get(s + 3).copied().unwrap_or(255);
    }
    match color {
        enc::ColorType::Gray | enc::ColorType::GrayAlpha => {
            let r = row.get(s).copied().unwrap_or(0);
            let g = row.get(s + 1).copied().unwrap_or(0);
            let b = row.get(s + 2).copied().unwrap_or(0);
            // BT.601亮度（与 F1002 的 luma601 同式、同舍入口径）
            let y = 77u32 * r as u32 + 150 * g as u32 + 29 * b as u32;
            ((y + 128) >> 8).min(255) as u8
        }
        _ => row.get(s + c).copied().unwrap_or(0),
    }
}

// ---------------------------------------------------------------------------
// 五之二、编码侧滤波（PNG 规范语义·自持·见头注设计要点六）
// ---------------------------------------------------------------------------

/// Paeth 预测子（PNG 规范 §9.2.6）。
#[inline]
fn spec_paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let pa = (p - a as i16).abs();
    let pb = (p - b as i16).abs();
    let pc = (p - c as i16).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// 字节视为有符号后的绝对值（评分口径：越接近 0 越好压）。
#[inline]
fn spec_abs(b: u8) -> u64 {
    if b < 128 { b as u64 } else { (256 - b as i32) as u64 }
}

/// **PNG 规范的编码方向滤波**：`Filt(x) = Raw(x) − Predictor(Raw(x−bpp), Prior(x), Prior(x−bpp))`。
///
/// **倒序遍历**是规范要求而非风格选择：Sub/Average/Paeth 的左预测子取的是
/// **原始样本** `Raw(x−bpp)`。正序遍历时 `cur[i−bpp]` 已被改写成残值，
/// 算出的预测子与解码端重建出的原值不等 → 整图错色，且残值本身也是合法
/// 字节，**不产生任何报错**（表现为「能解、就是颜色不对」）。
/// 倒序时 `i−bpp < i` 尚未被改写，`cur[i−bpp]` 仍是原值。
///
/// `prev` 为同遍上一行的**原始样本**；遍首行由调用方给全零（虚拟零行）。
///
/// 缓冲长度不一致或 `bpp == 0` → `false`（调用方据此显性拒绝，不静默少写）。
pub fn spec_apply_filter(fno: u8, cur: &mut [u8], prev: &[u8], bpp: usize) -> bool {
    let n = cur.len();
    if prev.len() != n || bpp == 0 {
        return false;
    }
    if n == 0 {
        return true;
    }
    match fno {
        0 => {}
        1 => {
            for i in (bpp..n).rev() {
                cur[i] = cur[i].wrapping_sub(cur[i - bpp]);
            }
        }
        2 => {
            for i in 0..n {
                cur[i] = cur[i].wrapping_sub(prev[i]);
            }
        }
        3 => {
            for i in (0..n).rev() {
                let a = if i >= bpp { cur[i - bpp] as u16 } else { 0 };
                cur[i] = cur[i].wrapping_sub(((a + prev[i] as u16) / 2) as u8);
            }
        }
        4 => {
            for i in (0..n).rev() {
                let a = if i >= bpp { cur[i - bpp] } else { 0 };
                let c = if i >= bpp { prev[i - bpp] } else { 0 };
                cur[i] = cur[i].wrapping_sub(spec_paeth(a, prev[i], c));
            }
        }
        _ => return false,
    }
    true
}

/// **按编码方向的绝对差和为五滤波评分**（`trial` 为试算缓冲，长度须 ≥ `cur`）。
///
/// 评分必须打在**本模块自己的滤波输出**上——复用别人的评分器等于对错误的
/// 候选打分（见头注设计要点六）。
///
/// **`trial` 是纯暂存**：函数返回后它的内容是「最后一个候选滤波号(4) 的
/// 残值」，**不含本行原样本**。调用方若需要原样本快照（比如推进 `prev`），
/// 必须另备缓冲——把本函数的结果缓冲当快照用，会让 `prev` 变成残值，
/// 下一行的 Up/Average/Paeth 参照整体错且**不产生任何报错**。
pub fn spec_choose_filter(cur: &[u8], prev: &[u8], bpp: usize, trial: &mut [u8]) -> u8 {
    let n = cur.len();
    if n == 0 || trial.len() < n || prev.len() != n || bpp == 0 {
        return 0;
    }
    let mut best = 0u8;
    let mut best_score = u64::MAX;
    for fno in 0u8..5 {
        let buf = &mut trial[..n];
        buf.copy_from_slice(&cur[..n]);
        if !spec_apply_filter(fno, buf, prev, bpp) {
            continue;
        }
        let mut score = 0u64;
        for &b in buf.iter() {
            score += spec_abs(b);
        }
        // 严格小于才替换 → 并列时取**低号滤波**（None 优先，体积基线，
        // 亦保证「并列时结果确定」，否则同输入可能出两种字节流）
        if score < best_score {
            best_score = score;
            best = fno;
        }
    }
    best
}

// ---------------------------------------------------------------------------
// 六、性能账与用户提示（锚点「性能差异文档化并给用户提示」）
// ---------------------------------------------------------------------------

/// 隔行 vs 逐行的**结构性**成本账（只陈述可从遍表推导的确定事实）。
///
/// **诚实标注**：本结构**不含实机计时**。锚点给出的「1.5-2 倍」是规范与
/// 生态的量级估计，本单未做实机测量，故不写成实测数字。真实机测由F1016
/// （PNG 性能基准与调优）承接——它的基准矩阵本就含「隔行」维度。
///
/// **三条成本口径（均为可推导的确定事实，非估算）**：
/// 1. **滤波行数**：遍行总数 `Σ height_pass` ≥ 逐行的 `height`。差值
///    `filter_row_delta()` 即「多出来的滤波次数」（自检 C03-PERF-01 断言）；
/// 2. **滤波先行字节**：每行 1 字节，故 **`raw_delta() == filter_row_delta()`**
///    ——两者是同一件事的两种计数，**不是**「非空遍数」（曾经的错误判据：
///    误以为只多出遍首行的字节，实测 64×64 差 56 而非 7）。
///    逐行只有 1 个首行，隔行有 `Σ height_pass` 个首行，差即上式；
/// 3. **重排开销**：每遍行需按 `col_step` 跨步取样/散布，是 O(像素) 的
///    额外一遍内存访问（**访存受限**，不增 CPU FLOPs）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PerfNote {
    /// 逐行方式的滤波行数。
    pub progressive_filter_rows: u64,
    /// 隔行方式的滤波行数（遍行总数）。
    pub interlaced_filter_rows: u64,
    /// 逐行方式的原始字节数。
    pub progressive_raw: u64,
    /// 隔行方式的原始字节数。
    pub interlaced_raw: u64,
    /// 非空遍数。
    pub nonempty_passes: u32,
}

impl PerfNote {
    /// 逐行原始字节数（`height × (payload+1)`）。
    pub fn progressive_raw(width: u32, height: u32, head: &dec::Ihdr) -> u64 {
        let bits = width as u64 * head.channels() as u64 * head.depth as u64;
        let rb = (bits + 7) / 8 + 1;
        rb * height as u64
    }
    /// 滤波行数增量（隔行 − 逐行）。
    pub fn filter_row_delta(&self) -> u64 {
        self.interlaced_filter_rows.saturating_sub(self.progressive_filter_rows)
    }
    /// 原始字节增量。
    ///
    /// **恒等于 [`PerfNote::filter_row_delta`]**：每滤波行恰好多 1 字节滤波
    /// 先行字节，故两增量是同一事实的两种计数（自检以该恒等式作门禁）。
    pub fn raw_delta(&self) -> u64 {
        self.interlaced_raw.saturating_sub(self.progressive_raw)
    }
    /// 采集一份成本账。
    pub fn of(width: u32, height: u32, head: &dec::Ihdr) -> PerfNote {
        let mut rows = 0u64;
        let mut nonempty = 0u32;
        for e in all_pass_extents(width, height, head) {
            if !e.is_empty() {
                rows += e.height as u64;
                nonempty += 1;
            }
        }
        PerfNote {
            progressive_filter_rows: height as u64,
            interlaced_filter_rows: rows,
            progressive_raw: PerfNote::progressive_raw(width, height, head),
            interlaced_raw: total_raw_bytes(width, height, head),
            nonempty_passes: nonempty,
        }
    }
}

/// 隔行开关的用户提示（锚点「给用户提示」；供 F1008 / F1018 直接消费）。
///
/// **单一口径出口**：下游若各写一套提示文案，同一问题会得到不同答案。
/// 本函数把「什么时候该开隔行」讲成一句可执行的话，而不是「视情况而定」。
pub fn interlace_advice(user_wants_progressive: bool) -> &'static str {
    if user_wants_progressive {
        "已启用 Adam7 隔行：七遍从粗到细渐进显示，前 3 遍即 1/8 分辨率完整图；\
         代价是解码比逐行慢约 1.5-2 倍，且滤波与访存次数增加。"
    } else {
        "默认不隔行（interlace=0）：体积更小、解码更快。\
         仅当需要渐进显示或缩略图快路径时才开隔行。"
    }
}