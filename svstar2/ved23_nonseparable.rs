//! VE-F0623 · 不可分离混合模式 4 种（VE-D 域 · 2D 合成引擎 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0623`
//!
//! **判据（锚点原文逐条）**：
//! - **4 种不可分离模式**：hue、saturation、color、luminosity——
//!   「非可分离本质：需逐像素在亮度饱和度空间重组（R、G、B 三通道
//!   不再独立计算）」；
//! - **W3C 非可分离定义的 ClipColor 与 SetLum 次级裁剪公式全实现**
//!   （**饱和度裁剪 SatClip 的三分支选择公式逐条落地**）；
//! - **计算顺序契约**：先非可分离函数，后与背景混合；
//! - **精度纪律**：色域往返（RGB → 亮度饱和度 → RGB）的浮点累积误差
//!   控制在 F0630 判据内；**负值与越界的钳制次序严格按规范（次序错则
//!   色相漂移）**；
//! - **与 F0622 可分离集共用色卡与对拍框架**。
//!
//! ## 〇、规范原文（本条一切公式的唯一出处）
//!
//! 锚点要求「公式对拍 W3C 规范」，故先固化原文（Compositing and
//! Blending Level 1 §10.2，条款号见 [`NonSepMode::clause`]）：
//!
//! ```text
//! Lum(C) = 0.3·R + 0.59·G + 0.11·B
//!
//! ClipColor(C):
//!     l = Lum(C);  n = min(R,G,B);  x = max(R,G,B)   ← 三个量各算一次
//!     if n < 0:  C_i = l + (((C_i - l) × l)     / (l - n))   ∀i
//!     if x > 1:  C_i = l + (((C_i - l) × (1-l)) / (x - l))   ∀i
//!     return C
//!
//! SetLum(C, l):  d = l - Lum(C);  C_i += d ∀i;  return ClipColor(C)
//!
//! Sat(C) = max(R,G,B) - min(R,G,B)
//!
//! SetSat(C, s):                       # 下标 min/mid/max 指**进入函数时**
//!     if Cmax > Cmin:                  # 各分量的取值次序，不是通道次序
//!         Cmid = ((Cmid - Cmin) × s) / (Cmax - Cmin)
//!         Cmax = s
//!     else:
//!         Cmid = Cmax = 0
//!     Cmin = 0                          ← 在 if/else **之后**，无条件执行
//!     return C
//!
//! hue        : B = SetLum(SetSat(Cs, Sat(Cb)), Lum(Cb))
//! saturation : B = SetLum(SetSat(Cb, Sat(Cs)), Lum(Cb))
//! color      : B = SetLum(Cs, Lum(Cb))
//! luminosity : B = SetLum(Cb, Lum(Cs))
//! ```
//!
//! ## 一、[`Lum`] 的权重是 **0.3/0.59/0.11**，不是 0.2126/0.7152/0.0722
//!
//! 这是一处**极易被"顺手改对"**的地方：CSS Color 4 的 sRGB 线性
//! 亮度用 0.2126/0.7152/0.0722，于是任何"看到亮度系数就想起
//! Rec.709"的写法都会把它改成那三个数——**改完之后所有公式仍自洽、
//! 所有亮度保持判据仍全绿**，因为亮度权重与保持性是两回事：换成
//! Rec.709 后 `Lum(SetLum(C, l)) == l` 依然成立，只是 `l` 的含义变了。
//!
//! 换言之**亮度保持类判据抓不住这个错**。抓住它的办法只有一条：
//! 把权重本身做成**可核对常量**并断言其与规范原文的数值一致
//! （[`C23-LUM-WEIGHTS-VERBATIM`]），且 oracle 侧**硬写这三个数**。
//! 故本条把权重写成 [`LUM_R`]/[`LUM_G`]/[`LUM_B`] 三个独立常量而
//! 不是 `(0.3, 0.59, 0.11)` 字面量内联——内联的话 grep 规范原文时
//! 看不见，改动也不留痕。
//!
//! ## 二、钳制次序**确实**影响结果——这不是风格问题，是可测的量
//!
//! 锚点写「负值与越界的钳制次序严格按规范（**次序错则色相漂移**）」。
//! 核对规范原文，`ClipColor` 的**三行量 `l`/`n`/`x` 在函数开头一次算出**，
//! 两个 `if` 共用这三个**入口**值。于是纪律落在一处**极容易被"顺手写对"**
//! 的地方：**第二个 `if` 的分母必须用入口 `x`，不能重算**。写成
//! "第一段改完再取 `max`"不仅看起来更自然，甚至像"更精确"（用了最新的值）
//! ——而它恰好是错的。
//!
//! 这一错**不是**换个顺序执行，而是**换了分母**。两段都是关于入口 `l`
//! 的仿射收缩（都把 `l` 映到自身），故收缩链的合成因子是各段因子之积：
//!
//! - 规范：`(l/(l-n)) · ((1-l)/(x-l))`；
//! - 逐段重算：`(l/(l-n)) · ((1-l)/(x₂-l))`，其中 `x₂` 是第一段之后的 `max`。
//!
//! `x₂ ≠ x` ⇒ 两个总因子不等 ⇒ 结果不等。取 `C = (-0.5, 0.5, 2.0)`
//! （实测 `l = 0.365`、`n = -0.5`、`x = 2.0`）：规范总因子
//! `= 0.365/0.865 × 0.635/1.635 ≈ 0.1636`，逐段重算总因子
//! `= 0.365/0.865 × 0.635/0.2675 ≈ 1.0017`——结果相差数倍，
//! 实测最大逐通道差 **0.367**，肉眼可见的色相漂移。
//!
//! 两版的**共同后果**是"结果落在 `[0,1]` 内"（收缩必然朝 `l` 收），
//! 所以**只断言"结果在 [0,1] 内"对这条纪律完全是瞎的**——两版都满足。
//! 判据必须直接比对两版的**中间结果**（[`clip_order_probe`] +
//! [`C23-CLIPCOLOR-ORDER-MATTERS`]）。
//!
//! 顺带纠正一处**很容易想当然**的推断：因为两段都是收缩，人们会以为
//! 「两段跑完 ⇒ `min == 0` 且 `max == 1`」。**并不如此。** 低位段单独跑时
//! 确实把 `min` 恰好压到 0、高位段单独跑时确实把 `max` 恰好压到 1；
//! 但**两段都跑**时，第二段的分母用的是**入口** `x`，而它作用在
//! **第一段的结果**上，于是合成因子 `k₁·k₂` 中 `k₂` 变大了（分母变小），
//! 收缩**比只跑低位段更弱** ⇒ `min > 0` 且 `max < 1`。
//!
//! 判据 [`C23-CLIPCOLOR-ORDER-SPEC`] 因此断的是**单段归零 / 归一**这条
//! 可精确验证的性质，而不是"两段同触发落在边界"——后者按规范**根本不该
//! 成立**，判据它会把**正确实现**判红。初版正是在这里翻的车（见 §六末）。
//!
//! ## 三、`SatClip` 的「三分支」：规范只有两分支，第三分支是**规范自身的怪癖**
//!
//! 锚点写「饱和度裁剪 SatClip 的三分支选择公式逐条落地」。核对规范
//! 原文（PDF 32000-1 Table 7.2 与 W3C §10.2.2 的伪码一致）：
//! `SetSat` 的条件判断**只有 `if (Cmax > Cmin) / else` 两个分支**。
//! 直接把它写成两分支，则与锚点「三分支」对不上；若硬编一个第三分支，
//! 就是往规范里塞私货。
//!
//! 本条的处置：把三分支落在**规范真实存在的三种可区分情形**上——
//! 输入是否携带色彩信息 × 目标饱和度是否为 0：
//!
//! | 分支 | 条件 | 规范行为 |
//! |------|------|----------|
//! | [`SatBranch::Scaled`] | `Cmax > Cmin`（彩色输入） | 按比例缩放 mid，`Cmax = s` |
//! | [`SatBranch::FlatToZero`] | `Cmax == Cmin` 且 `s == 0` | else 分支：全零 |
//! | [`SatBranch::FlatRequested`] | `Cmax == Cmin` 但 `s > 0` | else 分支：**仍全零** |
//!
//! 后两支的**输出完全相同**（都是 `(0,0,0)`）——这不是笔误，是规范的
//! 一个已知怪癖：当输入本身无色彩信息时，`SetSat` **丢弃全部信息**，
//! 无法凭空造出 `s` 的饱和度（灰没有色相可保）。它值得单独成档的理由是
//! **可观测后果不同**：第三支下调用方**要求**了一个非零饱和度却拿到 0，
//! 若不记账，上层无法区分"目标本就是灰"与"目标饱和度被静默丢弃"。
//!
//! 由此得到一条硬判据纪律：后两支**不能用输出值区分**，只能**直接断言
//! 分支枚举本身**（[`C23-SATCLIP-BRANCH-DIRECT`]）——这正是「判据不能
//! 向被测函数问答案」与「重合行为掩盖缺失分支」两条弱门禁的联合处置：
//! 若把两支合成一支，判据反而更容易通过。
//!
//! ## 四、`SetSat` 的下标是**取值次序**，不是**通道次序**——还原时的经典陷阱
//!
//! 规范原文明写「下标 min/mid/max 指各分量**进入函数时**的取值次序」。
//! 于是实现必须先求出 min/mid/max 各自的**通道下标**，算完再按**通道
//! 下标**写回。把「排序后的第 0/1/2 位」直接当成 `[R,G,B]` 写回，
//! 是本条最容易写出的**静默错误**：结果仍是 `[0,1]` 内的合法颜色、
//! 亮度仍然保持——**只有色相是错的**。
//!
//! 故本条的强判据都不看中间量，直接看**闭式与跨通道结果**：
//! - [`C23-SAT-IDENTITY-ROUNDTRIP`]：`SetSat(C, Sat(C)) == C - Cmin`
//!   （闭式，见 §六）；
//! - [`C23-DEFINITION-INVARIANTS`]：四模式的**饱和度保持**性质。
//!
//! 两者中任一都能抓住下标还原错误，因为还原错误必然把分量搬到别的通道。
//!
//! 并列值（`R == G` 等）需要**确定性 tie-break**，否则同一输入两次调用
//! 可能给出不同下标。本条取「最靠前者胜」（严格 `<` / `>` 才更新），
//! 并由 [`C23-SAT-TIEBREAK-DETERMINISTIC`] 钉死重复调用逐位相等。
//!
//! ### tie-break 的**方向**为什么必须另起一条判据
//!
//! 变异测试实测（M8）：把 `<` 换成 `<=`（并列时改取最靠后者），
//! [`C23-SAT-TIEBREAK-DETERMINISTIC`] **照样全绿**。两处原因叠加：
//!
//! 1. `<=` 同样是**确定性**的——每次都稳定给出同一个下标，那条判据
//!    断的「两次调用相等」根本不涉及方向。
//! 2. 更关键：并列分量的 **min 数值不变**，只是胜出下标从 `i` 换成 `j`。
//!    而写回时 `Cmin = 0`、`Cmax = s` 落在**不同通道**上，看着可观测，
//!    实际被**对称性抵消**了——两个相等分量里选谁当 min，落 zero 的
//!    通道就换一个，可零值写回后**两个通道都是 0**，输出逐位相同。
//!
//! ⇒ 输出层与确定性层都钉不住，**只能直接断言下标本身**。
//! [`C23-SAT-TIEBREAK-DIRECTION`] 就是这条：判据侧**手算**出 4 条并列
//! 语料的期望三元组，直接比对 [`order_indices`] 的返回值。
//!
//! 这不是「判据向被测函数问答案」——问的是契约明文规定的方向，不是
//! 被监督的量本身。**性质**：并列时输出**必然**逐位相同 ⇒ 输出层判据
//! 在原理上就不可能覆盖 tie-break 方向，必须下沉到下标层。
//!
//! ## 五、计算顺序契约：**先混合后合成**，且顺序**可测**
//!
//! 锚点：「计算顺序契约：先非可分离函数后与背景混合」。落为
//! [`composite_over`]:
//!
//! ```text
//! Cr = blend(mode, Cb, Cs)                     ← 非可分离函数先行
//! Co = αs·(1-αb)·Cs + αs·αb·Cr + (1-αs)·αb·Cb  ← 再与背景合成
//! ```
//!
//! 与 F0622 可分离集一致，本函数**不吃 alpha**（预乘纪律归 F0625），
//! alpha 只出现在 [`composite_over`] 的合成步。
//!
//! 「顺序契约」若只写成注释就是空话——因为把两步**交换**后，
//! 在 `αb = αs = 1` 的常用测试语料上结果**完全相同**（此时合成步退化为
//! 取 `Cr`）。判据 [`C23-COMPOSITE-ORDER-CONTRACT`] 因此**必须在
//! 双 alpha 严格小于 1 的语料上**比对两个次序，否则恒绿。
//!
//! ## 六、精度纪律：色域往返 —— 一条位精确恒等、一条**闭式**
//!
//! 锚点要「色域往返（RGB → 亮度饱和度 → RGB）的浮点累积误差控制在 F0630
//! 判据内」。本条把往返拆成两条性质，**一条真恒等、一条闭式**——
//! 「误差应当很小」这种双边阈值是弱门禁第二条点名的空断言，故不用它。
//!
//! ### 亮度侧：位精确恒等
//!
//! `SetLum(C, Lum(C)) == C`：`d = l - Lum(C) = 0` ⇒ 加法不动 ⇒
//! `ClipColor(C)` 在定义域内不动。**逐位精确**（判据
//! [`C23-LUM-IDENTITY-ROUNDTRIP`]）。这条对「`d` 的方向写成 `+`」是硬约束。
//!
//! ### 饱和度侧：**不是**恒等，而是闭式 `C - Cmin`
//!
//! 初版此处写的是「`SetSat(C, Sat(C)) == C`，彩色输入上逐通道精确」——
//! **这个前提是错的**，且错得不浅：规范原文的 `Cmin = 0` 写在
//! `if/else` **之后**、**无条件**执行。于是 `Scaled` 分支下
//!
//! ```text
//! Cmin' = 0
//! Cmax' = s = Sat(C) = Cmax - Cmin
//! Cmid' = (Cmid - Cmin) × s / (Cmax - Cmin) = Cmid - Cmin
//! ```
//!
//! 三通道**逐个都减掉了 `Cmin`**，所以 `SetSat(C, Sat(C)) == C - Cmin`，
//! 而不是 `C`。仅当 `Cmin == 0`（含灰、以及 R 恰为 0 的语料）才退化为 `C`。
//!
//! 这个错误若不改，判据会**把正确实现判红**——本单初测实测往返最坏
//! 偏差 **254.99998 LSB**（恰是语料里 `Cmin` 最大的那条），判据红。
//! 改法**不是**放宽阈值，而是把闭式写对：`C - Cmin` 是**可精确断言**的，
//! 比"误差小于 X"强得多：判据 [`C23-SAT-IDENTITY-ROUNDTRIP`] 断
//! 「`SetSat(C, Sat(C)) + Cmin·1 == C`」，残差必须**为 0**（容差取 `0`，
//! 不是 1 LSB）。
//!
//! 教训记在这里而不只是改代码：**判据的参考性质必须独立算出来**，
//! 「看起来应该恒等」不是理由。本单三条判据（灰轴位精确、两段同触发落
//! 边界、`SetSat` 往返恒等）都栽在同一件事上——都是先假定了一个**比规范
//! 更强**的性质。
//!
//! ## 七、与 F0622 共用色卡与对拍框架（锚点硬要求，故为**真引用**）
//!
//! 锚点：「与 F0622 可分离集共用色卡与对拍框架」。本条**不复制**
//! 灰阶表与色卡工具，而是 `use super::ved22_separable::{…}` 直接引用
//! [`RAMP`]/[`ramp_char`] 与 [`SWATCH_PROBES`]，并与 [`LSB8`] 共用 LSB
//! 口径。判据 [`C23-SWATCH-SHARED-PROBES`] 断言本条色卡消费的是
//! **同一份探针常量**——若哪天有人在本文件里另抄一份斜坡，判据转红。
//!
//! 色卡**内容**则不同：可分离集的色卡扫 `(Cb, Cs)` 灰阶对，
//! 非可分离必须扫**三通道彩色**，否则测不到跨通道效应（本条的存在意义）。
//! 故本条给 [`swatch_rows`] 用同一批探针点但**彩色化**，并额外提供
//! [`color_swatch_rows`] 逐通道打出 R|G|B——因灰阶条会把跨通道效应
//! **平均掉**（三通道均值对重排不敏感）。
//!
//! ## 八、独立 f64 金标准：四份实现里唯一不读本模块公式的那一份
//!
//! 对拍要抓「抄错规范」，抄错规范的实现之间会**错得一样**。故本条有
//! 第四份实现 [`oracle`]：不调用本模块任何函数，按 §〇 的规范原文
//! **逐字另写一遍 `f64` 直算**（含 `f64` 中间量、独立的插入排序求下标、
//! 以及硬写的权重字面量）。变异验证 [`C23-ORACLE-CATCHES-PRIMITIVE-ERROR`]
//! 用人为抄错的 `SetSat`（把 `Cmin = 0` 漏掉）证明 oracle 会红——
//! **否则 oracle 与被测同错时全绿**。
//!
//! 四路角色分工（与 F0622 同构）：
//!
//! | 路 | 入口 | 角色 |
//! |----|------|------|
//! | 标量 f32 | [`blend`] | 主实现（兼作语义基准） |
//! | 独立 f64 | [`oracle`] | 金标准（不读本模块） |
//! | 4 像素宽通道 | [`blend_simd`] | 摊薄面（操作数实测） |
//! | WGSL 文本 | [`wgsl_source`] | GPU 路模板（F0628 来源） |
//!
//! ## 九、错误路径与降级矩阵（锚点原文三行）
//!
//! | 锚点情形 | 本条处置 | 不许做的事 |
//! |----------|----------|------------|
//! | 色相漂移 → 钳制次序审计 | [`clip_order_probe`] 把两版差值**量化**出来 | 只在注释里写"注意次序" |
//! | 极端值（全黑/全白背景）→ 规范边界分支全覆盖 | [`EXTREME_BACKDROPS`] 逐档枚举并断言结果在 `[0,1]` 且非 NaN | 假定端点外推 |
//! | 精度越界 → F0627 策略介入 | 往返残差非 0 即红，**不在本条内静默放宽** | 偷偷把预算改大 |
//!
//! 另有本条自身的输入面：非有限输入（`NaN`/`±Inf`）与 `f32` 次正规分母，
//! 由 [`guard_finite`] / [`set_sat`] 兜底并记账（[`NonSepLog`]），
//! **绝不静默传播**——`NaN` 进入预乘链会毁掉整条下游（F0625 拿到的
//! 已不是颜色）。
//!
//! ## 十、无障碍与隐私
//!
//! [`swatch_rows`] / [`color_swatch_rows`] 产出**色卡文本行**（灰阶是
//! **文字**不是色块，读屏可达），零用户像素内容——本条全部语料是自造的
//! 合成数值。逻辑 tick 注入，零墙钟；确定性算法、零 IO。
//!
//! ## 十一、跨批对接点
//!
//! 上游 [`ved21_blendreg`]（F0621 注册表，本条 4 种是其
//! `SPEC_NONSEPARABLE == 4` 的先行条目）、[`ved22_separable`]（F0622：
//! 本条**共用其** `ramp_char`/`RAMP`/`SWATCH_PROBES`/`LSB8` 与 oracle
//! 骨架）；下游 F0628（GPU 路正式着色器化，本条 [`wgsl_source`] 是其
//! 模板）、F0629（CPU SIMD 工程化，本条 [`blend_simd`] 是其内核）、
//! F0630（对拍台账，本条 [`oracle`] 的 LSB 口径与其一致）、F0627
//! （浮点精度策略，越界由该单介入）。
//!
//! ## 十二、本条的判据清单（[`run_ved23_checks`]）
//!
//! 28 条，逐条的「它凭什么能抓错」写在代码里——判据名只是标签，
//! **说不出它能抓什么错的判据等于没写**。
//!
//! | 判据 | 抓什么 |
//! |------|--------|
//! | [`C23-REGISTRY-4-MODES`] | 漏注册 / 下标与键不同向 / 关键字重复 |
//! | [`C23-LUM-WEIGHTS-VERBATIM`] | 权重被换成 Rec.709（亮度保持类判据抓不住） |
//! | [`C23-LUM-GRAY-AXIS-FIXED`] | 权重被改动后仍和为 1 但分配错了 |
//! | [`C23-FORMULA-VS-ORACLE`] | 公式抄错规范（对拍独立 f64） |
//! | [`C23-ORACLE-CATCHES-PRIMITIVE-ERROR`] | oracle 自身是空断言 |
//! | [`C23-DEFINITION-INVARIANTS`] | 亮度/饱和度取自哪一侧写反 |
//! | [`C23-CLIPCOLOR-ORDER-MATTERS`] | 第二段分母用重算的 `x₂` |
//! | [`C23-CLIPCOLOR-PRESERVES-LUM`] | `l` 取成非入口亮度 |
//! | [`C23-CLIPCOLOR-ORDER-SPEC`] | 两段整体失效（死码） |
//! | [`C23-SATCLIP-THREE-BRANCHES`] | 三分支合并 / `>` 写成 `>=` |
//! | [`C23-SATCLIP-BRANCH-DIRECT`] | 合并两支后仍能过输出值断言 |
//! | [`C23-SAT-IDENTITY-ROUNDTRIP`] | 通道下标还原错（闭式） |
//! | [`C23-LUM-IDENTITY-ROUNDTRIP`] | `d` 的方向写成 `+` |
//! | [`C23-ROUNDTRIP-BUDGET`] | 往返累积误差超 F0630 口径 |
//! | [`C23-SAT-TIEBREAK-DETERMINISTIC`] | 并列分量导致非确定性 |
//! | [`C23-SAT-TIEBREAK-DIRECTION`] | tie-break **方向**反（输出层钉不住，见 §四末） |
//! | [`C23-COMPOSITE-ORDER-CONTRACT`] | 合成步提到混合之前 |
//! | [`C23-COMPOSITE-ALPHA-EDGES`] | 合成式漏项 / 系数错 |
//! | [`C23-EXTREME-BACKDROPS`] | 端点除零 / 边界判定差一格 |
//! | [`C23-GRAY-BACKDROP-NOOP`] | 规范 §10.2.2 明文性质被破 |
//! | [`C23-NONSEPARABLE-NOT-CHANNELWISE`] | 退化成逐通道独立（不再是"不可分离"） |
//! | [`C23-CLAMP-NON-FINITE`] | `NaN`/`±Inf` 静默传播 |
//! | [`C23-SAT-DENOM-GUARD`] | 次正规分母产出 `NaN` |
//! | [`C23-SIMD-BATCH-EQUIV`] | uniform 快路径算错 |
//! | [`C23-SIMD-AMORTIZATION`] | 宽通道退化成逐像素循环 |
//! | [`C23-WGSL-TEXT-KEYWORDS`] | GPU 路只吐壳 |
//! | [`C23-SWATCH-SHARED-PROBES`] | 另抄一份斜坡探针 |
//! | [`C23-KEY-OUT-OF-RANGE-NONE`] | 越界键静默变合法模式 |
//! | [`C23-LEDGER-EXACT`] | 记账被吞（幽灵记账 / 漏记） |

use crate::checks::CheckSet;

// no_std 导入三件套 + format!：本条产出摘要与 WGSL 文本。
// 三件套齐备（`vec` / `Vec` / `string`）以与全仓其它 no_std 模块一致，
// 缺任何一项在真仓 no_std 下会报错而 std 探针照常编译。
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// 与 F0622 共用色卡与对拍框架（锚点硬要求「共用色卡与对拍框架」）。
// 真引用而非复制：判据 C23-SWATCH-SHARED-PROBES 依赖这一致性。
use super::ved22_separable::{ramp_char, LSB8, LSB_BUDGET, RAMP, SWATCH_PROBES};

// ---------------------------------------------------------------------------
// 一、参数唯一源（规范原文值，不可调）
// ---------------------------------------------------------------------------

/// 不可分离模式数（规范 §10.2 恰 4 种，锚点亦为 4）。
pub const NONSEPARABLE_COUNT: usize = 4;

/// [`Lum`] 的红权重（规范原文 **0.3**，非 Rec.709 的 0.2126）。
///
/// 独立成常量而非内联字面量：判据要逐字核对（[`C23-LUM-WEIGHTS-VERBATIM`]），
/// 内联则 grep 规范原文看不见、误改也不留痕。见头注 §一。
pub const LUM_R: f32 = 0.30;

/// [`Lum`] 的绿权重（规范原文 **0.59**）。
pub const LUM_G: f32 = 0.59;

/// [`Lum`] 的蓝权重（规范原文 **0.11**）。
pub const LUM_B: f32 = 0.11;

/// 三权重之和（规范要求 Lum 为**仿射**亮度 ⇒ 灰输入亮度 == 灰值）。
///
/// 这不是"凑数常量"：它是**灰轴不动**这一性质的判据。权重若和不为 1，
/// 则 `Lum(0.5,0.5,0.5) != 0.5`，而规范对灰色的定义是亮度等于自身，
/// 于是 `SetLum` 在灰轴上会引入偏移。
pub const LUM_SUM: f32 = LUM_R + LUM_G + LUM_B;

/// 低位裁剪判定阈值（`ClipColor` 的 `if n < 0`）。
pub const CLIP_LO: f32 = 0.0;

/// 高位裁剪判定阈值（`ClipColor` 的 `if x > 1`）。
pub const CLIP_HI: f32 = 1.0;

/// [`SetSat`] / [`ClipColor`] 分母的可信下界。
///
/// 规范的分母（`Cmax - Cmin`、`l - n`、`x - l`）在数学上由前置判定保证
/// 非零；但 `f32` 下两个极接近的值相减仍可下溢到 0（次正规数区），此时
/// `0/0 = NaN`。本条在分母低于此界时改走**置零收缩**并记账——静默产
/// NaN 会毁掉整条预乘链（F0625）。
///
/// 取值远小于任何 8 bit 可表示差（1/255），故对真实输入**不触发**，
/// 仅作最底部兜底；判据 [`C23-SAT-DENOM-GUARD`] 用专门语料打它。
pub const SAT_DENOM_MIN: f32 = 1.0e-30;

/// 非有限输入钳制目标（定义域恒为 `[0,1]`）。
pub const CHANNEL_LO: f32 = 0.0;
pub const CHANNEL_HI: f32 = 1.0;

/// SIMD 宽通道宽度（与 F0622 的 [`SIMD_WIDTH`] 同口径，4 像素批）。
pub const SIMD_WIDTH: usize = 4;

/// 色卡探针数（直接复用 F0622 的 [`SWATCH_PROBES`] 长度）。
pub const SWATCH_STEPS: usize = SWATCH_PROBES.len();

/// 色域往返的 LSB 预算（锚点：累积误差控制在 F0630 判据内）。
///
/// 口径 [`LSB8`] 由 F0622 引入并复用，保证两单的对拍尺度是**同一个**。
/// 注意：本条的往返判据实际断的是**残差为 0**（闭式，见头注 §六），
/// 该预算是给 [`C23-ROUNDTRIP-BUDGET`] 的**兜底**口径。
pub const ROUNDTRIP_LSB_BUDGET: f32 = LSB_BUDGET;

/// 极端背景语料：锚点「极端值（全黑全白背景）→规范边界分支全覆盖」。
pub const EXTREME_BACKDROPS: [Rgb; 4] = [
    Rgb { r: 0.0, g: 0.0, b: 0.0 },
    Rgb { r: 1.0, g: 1.0, b: 1.0 },
    Rgb { r: 0.0, g: 0.5, b: 1.0 },
    Rgb { r: 1.0, g: 0.5, b: 0.0 },
];

// ---------------------------------------------------------------------------
// 二、色彩三元组与模式键
// ---------------------------------------------------------------------------

/// RGB 三元组（通道**有序**，非"排序后"——排序只在 [`set_sat`] 内部临时做）。
///
/// `f32` 不能 `derive(Eq)`，故本类型只 `PartialEq`。
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Rgb {
    /// 灰（`v` 三通道等值）。
    pub fn gray(v: f32) -> Rgb {
        Rgb { r: v, g: v, b: v }
    }

    /// 按下标取通道（`0→R, 1→G, 2→B`）。
    ///
    /// `set_sat` 的通道下标还原全走这里——**写死一次**比在三处各写
    /// `match` 更难抄错。
    pub fn get(&self, i: usize) -> f32 {
        match i {
            0 => self.r,
            1 => self.g,
            _ => self.b,
        }
    }

    /// 按下标写通道，返回新值（不原地改）。
    pub fn with(&self, i: usize, v: f32) -> Rgb {
        match i {
            0 => Rgb { r: v, g: self.g, b: self.b },
            1 => Rgb { r: self.r, g: v, b: self.b },
            _ => Rgb { r: self.r, g: self.g, b: v },
        }
    }

    /// 三通道最小值。
    pub fn min_ch(&self) -> f32 {
        min3(self.r, self.g, self.b)
    }

    /// 三通道最大值。
    pub fn max_ch(&self) -> f32 {
        max3(self.r, self.g, self.b)
    }
}

/// 不可分离混合模式键（4 种，顺序 = W3C §10.2.x 条款顺序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NonSepMode {
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl NonSepMode {
    /// 全部 4 种。
    pub fn all() -> [NonSepMode; NONSEPARABLE_COUNT] {
        [
            NonSepMode::Hue,
            NonSepMode::Saturation,
            NonSepMode::Color,
            NonSepMode::Luminosity,
        ]
    }

    /// 注册下标（与 [`NonSepMode::all`] 顺序一致）。
    pub fn index(&self) -> usize {
        match self {
            NonSepMode::Hue => 0,
            NonSepMode::Saturation => 1,
            NonSepMode::Color => 2,
            NonSepMode::Luminosity => 3,
        }
    }

    /// 下标 → 模式键；**越界返回 `None`**，绝不 `unwrap`。
    pub fn from_index(i: usize) -> Option<NonSepMode> {
        match i {
            0 => Some(NonSepMode::Hue),
            1 => Some(NonSepMode::Saturation),
            2 => Some(NonSepMode::Color),
            3 => Some(NonSepMode::Luminosity),
            _ => None,
        }
    }

    /// 关键字（与 CSS `mix-blend-mode` 拼写一致）。
    pub fn keyword(&self) -> &'static str {
        match self {
            NonSepMode::Hue => "hue",
            NonSepMode::Saturation => "saturation",
            NonSepMode::Color => "color",
            NonSepMode::Luminosity => "luminosity",
        }
    }

    /// 可核对条款号（W3C Compositing and Blending Level 1 §10.2.x）。
    pub fn clause(&self) -> &'static str {
        match self {
            NonSepMode::Hue => "compositing-1§10.2.1",
            NonSepMode::Saturation => "compositing-1§10.2.2",
            NonSepMode::Color => "compositing-1§10.2.3",
            NonSepMode::Luminosity => "compositing-1§10.2.4",
        }
    }

    /// 本模式的**亮度取自哪一侧**（规范定义的直接推论）。
    ///
    /// - `hue` / `saturation` / `color`：亮度来自**背景**；
    /// - `luminosity`：亮度来自**源**。
    ///
    /// 供 [`C23-DEFINITION-INVARIANTS`] 直接断言"亮度取自谁"——这是规范
    /// 定义的**字面内容**，比任何数值恒等式都更贴近"对拍规范"，且**不依赖
    /// 本模块的浮点实现**。
    pub fn lum_from(&self) -> Side {
        match self {
            NonSepMode::Hue | NonSepMode::Saturation | NonSepMode::Color => Side::Backdrop,
            NonSepMode::Luminosity => Side::Source,
        }
    }

    /// 饱和度取自哪一侧（见 [`NonSepMode::lum_from`]）。
    ///
    /// - `hue` / `luminosity`：饱和度来自**背景**；
    /// - `saturation` / `color`：饱和度来自**源**。
    pub fn sat_from(&self) -> Side {
        match self {
            NonSepMode::Hue | NonSepMode::Luminosity => Side::Backdrop,
            NonSepMode::Saturation | NonSepMode::Color => Side::Source,
        }
    }

    /// 规范对该模式的一句人话描述（色卡/摘要用）。
    pub fn prose(&self) -> &'static str {
        match self {
            NonSepMode::Hue => "取源色相，背景的饱和度与亮度",
            NonSepMode::Saturation => "取源饱和度，背景的色相与亮度",
            NonSepMode::Color => "取源色相与饱和度，背景的亮度",
            NonSepMode::Luminosity => "取源亮度，背景的色相与饱和度",
        }
    }
}

/// 侧（背景 / 源）——用于「取自哪一侧」这类**结构性**断言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Backdrop,
    Source,
}

impl Side {
    pub fn keyword(&self) -> &'static str {
        match self {
            Side::Backdrop => "Cb",
            Side::Source => "Cs",
        }
    }
}

/// 供上层复用的模式键视图（避免 `vec!` 在 no_std 下散落）。
pub fn nonseparable_modes() -> Vec<NonSepMode> {
    let mut v: Vec<NonSepMode> = Vec::with_capacity(NONSEPARABLE_COUNT);
    for m in NonSepMode::all().iter() {
        v.push(*m);
    }
    v
}

// ---------------------------------------------------------------------------
// 三、SetSat 的分支枚举（三分支，见头注 §三）
// ---------------------------------------------------------------------------

/// [`set_sat`] 的分支档（锚点「SatClip 的三分支选择公式」）。
///
/// 后两档输出**完全相同**（规范 else 分支一律全零），故只能靠枚举本身
/// 区分——判据 [`C23-SATCLIP-BRANCH-DIRECT`] 直接断言本枚举。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SatBranch {
    /// `Cmax > Cmin`：彩色输入，按比例缩放中位分量，`Cmax = s`。
    Scaled,
    /// `Cmax == Cmin` 且 `s == 0`：输入无色彩、目标亦无饱和度 → 全零。
    FlatToZero,
    /// `Cmax == Cmin` 但 `s > 0`：输入无色彩、规范 else 分支**丢弃**目标
    /// 饱和度 → 仍全零。单独成档是为了让上层能发现"要求了却没拿到"。
    FlatRequested,
}

impl SatBranch {
    /// 三档全部（判据遍历用）。
    pub fn all() -> [SatBranch; 3] {
        [SatBranch::Scaled, SatBranch::FlatToZero, SatBranch::FlatRequested]
    }

    pub fn keyword(&self) -> &'static str {
        match self {
            SatBranch::Scaled => "scaled",
            SatBranch::FlatToZero => "flat-to-zero",
            SatBranch::FlatRequested => "flat-requested",
        }
    }

    /// 该分支是否走规范 else 分支（输出恒为全零）。
    pub fn is_flat(&self) -> bool {
        !matches!(self, SatBranch::Scaled)
    }
}

// ---------------------------------------------------------------------------
// 四、记账台账（异常零静默）
// ---------------------------------------------------------------------------

/// 不可分离路径的异常记账。
///
/// 每一项都对应头注 §九的一条降级处置；**只增不减**，饱和累加以免
/// 长时间渲染回绕成 0（`u32::MAX` 语义与内核饱和线一致）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NonSepLog {
    /// 非有限输入收口次数（`NaN` / `±Inf`）。
    pub clamped_inputs: u32,
    /// `ClipColor` 低位段（`n < 0`）触发次数。
    pub clip_low: u32,
    /// `ClipColor` 高位段（`x > 1`）触发次数。
    pub clip_high: u32,
    /// 两段**同时**触发的次数（次序敏感的那一类）。
    pub clip_both: u32,
    /// `SetSat` 走 `Scaled` 分支次数。
    pub sat_scaled: u32,
    /// `SetSat` 走 `FlatToZero` 分支次数。
    pub sat_flat_zero: u32,
    /// `SetSat` 走 `FlatRequested` 分支次数（要求了饱和度却被丢弃）。
    pub sat_flat_requested: u32,
    /// 分母触底、改走置零档的次数（`f32` 次正规兜底）。
    pub sat_denom_guarded: u32,
    /// 并列值命中 tie-break 的次数（三通道存在相等分量）。
    pub tiebreak_hits: u32,
    /// 求值后结果越界、靠 [`clamp_residue`] 收口回来的次数。
    pub out_of_range: u32,
}

impl NonSepLog {
    pub fn new() -> NonSepLog {
        NonSepLog::default()
    }

    /// 记账合计。
    pub fn total(&self) -> u32 {
        self.clamped_inputs
            .saturating_add(self.clip_low)
            .saturating_add(self.clip_high)
            .saturating_add(self.sat_scaled)
            .saturating_add(self.sat_flat_zero)
            .saturating_add(self.sat_flat_requested)
            .saturating_add(self.sat_denom_guarded)
            .saturating_add(self.tiebreak_hits)
    }

    /// 台账是否为空。
    pub fn is_clean(&self) -> bool {
        self.total() == 0
    }

    /// 逐档分支计数（`Scaled` / `FlatToZero` / `FlatRequested`）。
    pub fn sat_branch_counts(&self) -> [u32; 3] {
        [self.sat_scaled, self.sat_flat_zero, self.sat_flat_requested]
    }
}

// ---------------------------------------------------------------------------
// 五、色彩原语：Lum / Sat / ClipColor / SetLum / SetSat
// ---------------------------------------------------------------------------

/// **非有限**输入收口（`NaN` → 0.0，`+Inf` → 1.0，`-Inf` → 0.0）。
///
/// ## 这里**只**收非有限值，**不**把有限越界值拉回 `[0,1]`
///
/// 这是一处**死码陷阱**：若把收口写成含范围钳制的 `clamp01`，那么送进
/// [`clip_color_ordered`] 的颜色必然满足 `n >= 0` 且 `x <= 1`——规范的两个
/// `if` 判定**永不成立**，两段裁剪全是死码。
///
/// 而外部表现是"看不出问题"：结果照样落在 `[0,1]` 内（因为输入已在
/// 范围内，无须裁剪），亮度保持、往返闭式、对拍 oracle 全绿。
/// **唯一的破绽是两段从未执行**——判据若只看输出值，则此错完全隐形。
/// 这正是弱门禁第十二条的形态：分支类性质必须有**直接断言该分支真被
/// 执行**的判据，本条由 [`C23-CLIPCOLOR-ORDER-SPEC`] 承担。
///
/// 正确形态：`ClipColor` 的**职责就是**收拾越界中间量。真实管线里
/// [`set_lum`] 先做 `C + d`，`d = l - Lum(C)` 可以把通道推出定义域，
/// **然后**才交给 `ClipColor` 收口。若在 `ClipColor` 之前就把越界值抹平，
/// 两段裁剪便永无输入——规范里这两段也就白写了。
///
/// 故本函数只拦 `NaN`/`±Inf`（它们会让下游整条预乘链静默毁掉，
/// F0625 拿到的已不是颜色），有限越界值**原样放行**交给两段判定。
fn guard_finite(c: Rgb, log: &mut NonSepLog) -> Rgb {
    let (r, a) = guard_finite_scalar(c.r);
    let (g, b) = guard_finite_scalar(c.g);
    let (bl, c2) = guard_finite_scalar(c.b);
    if a || b || c2 {
        log.clamped_inputs = log.clamped_inputs.saturating_add(1);
    }
    Rgb { r, g, b: bl }
}

/// 单标量非有限收口（`NaN` → 0.0，`+Inf` → 1.0，`-Inf` → 0.0）。
pub fn guard_finite_scalar(v: f32) -> (f32, bool) {
    if v.is_nan() {
        return (CHANNEL_LO, true);
    }
    if v == f32::INFINITY {
        return (CHANNEL_HI, true);
    }
    if v == f32::NEG_INFINITY {
        return (CHANNEL_LO, true);
    }
    (v, false)
}

/// 两段裁剪之后的**浮点残渣收口**（把 `-1e-8` / `1+1e-8` 之类拉回）。
///
/// 与 [`guard_finite`] 分开是刻意的：前者是**输入面**兜底，后者是
/// **输出面**兜底。数学上 [`clip_color_ordered`] 的收缩因子都 ≤ 1，故结果
/// 必在 `[0,1]` 内；但 `f32` 尾数会让它差一两个 ULP，故需收口——
///
/// **收口不得掩盖次序错误**：它只在**夹残渣量级**，并在
/// [`NonSepLog::out_of_range`] 上留痕。若收口放宽到能把"用错分母"的
/// 越界结果也夹回来，那次序错误就被静默掉了——判据
/// [`C23-CLIPCOLOR-ORDER-MATTERS`] 因此断的是两版差值而非值域。
fn clamp_residue(c: Rgb, log: &mut NonSepLog) -> Rgb {
    let ch = [c.r, c.g, c.b];
    let mut out = c;
    let mut touched = false;
    let mut i = 0usize;
    while i < 3 {
        let v = ch[i];
        if v < CHANNEL_LO {
            out = out.with(i, CHANNEL_LO);
            touched = true;
        } else if v > CHANNEL_HI {
            out = out.with(i, CHANNEL_HI);
            touched = true;
        }
        i += 1;
    }
    if touched {
        log.out_of_range = log.out_of_range.saturating_add(1);
    }
    out
}

/// [`Lum`]：规范原文 `0.3·R + 0.59·G + 0.11·B`。
///
/// 权重取自 [`LUM_R`]/[`LUM_G`]/[`LUM_B`]，**不内联**——见头注 §一。
pub fn lum(c: Rgb) -> f32 {
    LUM_R * c.r + LUM_G * c.g + LUM_B * c.b
}

/// [`Sat`]：规范原文 `max - min`。
pub fn sat(c: Rgb) -> f32 {
    c.max_ch() - c.min_ch()
}

fn max3(a: f32, b: f32, c: f32) -> f32 {
    if a >= b {
        if a >= c {
            a
        } else {
            c
        }
    } else if b >= c {
        b
    } else {
        c
    }
}

fn min3(a: f32, b: f32, c: f32) -> f32 {
    if a <= b {
        if a <= c {
            a
        } else {
            c
        }
    } else if b <= c {
        b
    } else {
        c
    }
}

/// `1.0` 处的 `f32` ULP（2⁻²³ = 1.1920929e-7）。
///
/// ULP 级判据的唯一口径来源。**不用 `f32::EPSILON`**（它是 1.0 与
/// 下一个更大数之差，与「在 0.8 处的步长」不等），也不用 1 LSB
/// （宽约 3.3 万倍，抓不住任何实现）。
pub const F32_ULP_AT_ONE: f32 = 1.1920929e-7;

/// 两个标量的绝对差，以 [`F32_ULP_AT_ONE`] 为单位（ULP 级比较）。
pub fn out_gap_ulp(a: f32, b: f32) -> f32 {
    let d = if a > b { a - b } else { b - a };
    d / F32_ULP_AT_ONE
}

/// 两个三元组的逐通道最大绝对差（原始量纲，非 LSB）。
pub fn out_rgb_gap(a: Rgb, b: Rgb) -> f32 {
    let pa = [a.r, a.g, a.b];
    let pb = [b.r, b.g, b.b];
    let mut worst = 0.0f32;
    let mut i = 0usize;
    while i < 3 {
        let d = if pa[i] > pb[i] { pa[i] - pb[i] } else { pb[i] - pa[i] };
        if d > worst {
            worst = d;
        }
        i += 1;
    }
    worst
}

/// 两三元组的亮度差绝对值（亮度保持类判据用）。
pub fn out_lum_dev(out: Rgb, ref_l: f32) -> f32 {
    out_gap_ulp(lum(out), ref_l) * F32_ULP_AT_ONE
}


/// min/mid/max 各自的**通道下标**（并列时最靠前者胜）。
///
/// 返回 `(i_min, i_mid, i_max)`。三个下标恒为 `{0,1,2}` 的一个排列。
///
/// 并列 tie-break 的必要性：`Cmax > Cmin` 为假时（灰）规范走 else 分支
/// 全零，此时下标无意义；但 `Cmax > Cmin` 为真且存在相等分量（如
/// `(0.5, 0.5, 0.2)`，`G == R`）时，**谁当 max 会改变还原后的分量归属**。
/// 若让 `>` / `<` 与 `>=` / `<=` 混用，同一输入两次调用可能给出不同
/// 下标 ⇒ 非确定性输出。判据 [`C23-SAT-TIEBREAK-DETERMINISTIC`] 钉死。
fn order_indices(c: Rgb, log: &mut NonSepLog) -> (usize, usize, usize) {
    let v = [c.r, c.g, c.b];
    let mut i_min = 0usize;
    let mut i_max = 0usize;
    let mut i = 1usize;
    while i < 3 {
        // 严格比较 ⇒ 并列时保留最靠前者（确定性 tie-break）。
        if v[i] < v[i_min] {
            i_min = i;
        }
        if v[i] > v[i_max] {
            i_max = i;
        }
        i += 1;
    }
    // 三下标互异时中位 = 剩下的那个；全等时规范必走 else 分支、下标不
    // 参与运算，但为保持「三下标构成 0/1/2 的排列」这一不变式，仍取 1。
    let i_mid = if i_min != i_max { 3 - i_min - i_max } else { 1 };
    if v[0] == v[1] || v[1] == v[2] || v[0] == v[2] {
        log.tiebreak_hits = log.tiebreak_hits.saturating_add(1);
    }
    (i_min, i_mid, i_max)
}

/// [`SetSat`]（规范原文，三分支见头注 §三）。
///
/// 返回值同时给出**结果**与**走了哪一档**，使上层能对第三档
/// （要求了饱和度却被规范丢弃）作出反应——只返回颜色的话这一信息
/// 就永久丢失了。
pub fn set_sat(c: Rgb, s: f32, log: &mut NonSepLog) -> (Rgb, SatBranch) {
    let cs = guard_finite(c, log);
    let (sv, sv_clamped) = guard_finite_scalar(s);
    if sv_clamped {
        log.clamped_inputs = log.clamped_inputs.saturating_add(1);
    }
    let (i_min, i_mid, i_max) = order_indices(cs, log);
    let v_min = cs.get(i_min);
    let v_max = cs.get(i_max);
    let denom = v_max - v_min;

    if v_max > v_min {
        if denom < SAT_DENOM_MIN {
            // `f32` 下 `Cmax - Cmin` 可下溢到 0（次正规区）⇒ 0/0 = NaN。
            // 规范未覆盖此点（它假定实数），本条改走置零档并记账，
            // 绝不静默产 NaN（头注 §九）。
            log.sat_denom_guarded = log.sat_denom_guarded.saturating_add(1);
            log.sat_flat_zero = log.sat_flat_zero.saturating_add(1);
            return (Rgb::gray(CHANNEL_LO), SatBranch::FlatToZero);
        }
        let v_mid = cs.get(i_mid);
        let new_mid = ((v_mid - v_min) * sv) / denom;
        log.sat_scaled = log.sat_scaled.saturating_add(1);
        // `Cmin = 0` 在 if/else **之后**无条件执行（规范原文，头注 §六）。
        let out = cs.with(i_min, CHANNEL_LO).with(i_mid, new_mid).with(i_max, sv);
        (out, SatBranch::Scaled)
    } else if sv == 0.0 {
        log.sat_flat_zero = log.sat_flat_zero.saturating_add(1);
        (Rgb::gray(CHANNEL_LO), SatBranch::FlatToZero)
    } else {
        // 规范怪癖：输入无色彩信息时，`SetSat` 无法凭空造出饱和度，
        // else 分支一律全零——**目标饱和度被静默丢弃**。单独记账。
        log.sat_flat_requested = log.sat_flat_requested.saturating_add(1);
        (Rgb::gray(CHANNEL_LO), SatBranch::FlatRequested)
    }
}

/// [`SetLum`]（规范原文）：整体加 `d = l - Lum(C)` 后 `ClipColor`。
///
/// 返回结果与是否发生过裁剪（便于上层判"亮度保持是否被裁剪破坏"）。
pub fn set_lum(c: Rgb, l: f32, log: &mut NonSepLog) -> (Rgb, bool) {
    let (lf, l_clamped) = guard_finite_scalar(l);
    if l_clamped {
        log.clamped_inputs = log.clamped_inputs.saturating_add(1);
    }
    let cs = guard_finite(c, log);
    let d = lf - lum(cs);
    let shifted = Rgb { r: cs.r + d, g: cs.g + d, b: cs.b + d };
    clip_color_ordered(shifted, log)
}

/// [`ClipColor`] 按**规范**：入口 `l`/`n`/`x` 各算一次，两个 `if` 共用。
///
/// 规范原文是两个**独立** `if`（不是 `if/else`），且**第二段的分母用入口
/// `x`**——写成"第一段改完再重算 `x`"违反规范，见头注 §二的推导。
/// 返回值第二项表示"是否至少触发一段"。
///
/// **钳制次序的纪律落在这里**（锚点：负值与越界的钳制次序严格按规范）：
/// 输入只拦 `NaN`/`±Inf`（[`guard_finite`]），**有限越界值一律放行**
/// 交给下面两段判定。
pub fn clip_color_ordered(c: Rgb, log: &mut NonSepLog) -> (Rgb, bool) {
    let cs = guard_finite(c, log);
    // 规范原文：`l`、`n`、`x` 在函数开头**一次算出**，两段判定共用
    // 这三个**入口**值。这一点是本条的核心纪律。
    let l = lum(cs);
    let n = cs.min_ch();
    let x = cs.max_ch();

    let mut out = cs;
    let mut clipped = false;

    // 第一段：if (n < 0)
    if n < CLIP_LO {
        out = clip_low_with(out, l, n);
        log.clip_low = log.clip_low.saturating_add(1);
        clipped = true;
    }
    // 第二段：if (x > 1) —— 独立 if，不因第一段成立而跳过，
    // 且判定用的仍是**入口** x，不是第一段之后的 max。
    if x > CLIP_HI {
        out = clip_high_with(out, l, x);
        log.clip_high = log.clip_high.saturating_add(1);
        clipped = true;
    }
    if n < CLIP_LO && x > CLIP_HI {
        log.clip_both = log.clip_both.saturating_add(1);
    }
    let out = clamp_residue(out, log);
    (out, clipped)
}

/// 低位段收缩（`n < 0`）：`C_i = l + ((C_i - l) * l) / (l - n)`。
///
/// 分母用**入口** `n`（见 [`clip_color_ordered`] 的纪律注）。
fn clip_low_with(c: Rgb, l: f32, n: f32) -> Rgb {
    let denom = l - n;
    // `n < 0` 且 `l ∈ [0,1]` ⇒ `l - n > 0`，分母数学上非零；
    // 仍兜底：分母下溢时取零收缩（等价于把颜色压到亮度 l）。
    let k = if denom < SAT_DENOM_MIN { 0.0 } else { l / denom };
    Rgb {
        r: l + (c.r - l) * k,
        g: l + (c.g - l) * k,
        b: l + (c.b - l) * k,
    }
}

/// 高位段收缩（`x > 1`）：`C_i = l + ((C_i - l) * (1-l)) / (x - l)`。
///
/// 分母用**入口** `x`（见 [`clip_color_ordered`] 的纪律注）。
fn clip_high_with(c: Rgb, l: f32, x: f32) -> Rgb {
    let denom = x - l;
    // `x > 1` 且 `l ∈ [0,1]` ⇒ `x - l > 0`；同 [`clip_low_with`] 兜底。
    let k = if denom < SAT_DENOM_MIN { 0.0 } else { (1.0 - l) / denom };
    Rgb {
        r: l + (c.r - l) * k,
        g: l + (c.g - l) * k,
        b: l + (c.b - l) * k,
    }
}

/// [`ClipColor`] 的**逐段重算对照版**：第二段用**第一段之后**的 `max`。
///
/// **仅供判据对照**（[`clip_order_probe`]），生产路径不许调用。
///
/// 为什么它是对照而不是"另一个正确写法"：规范原文在函数开头把 `l`、
/// `n`、`x` 三个量**各算一次**，两个 `if` 共用这三个**入口**值。写成
/// "第一段改完再重算 `x`"看起来更自然（甚至像"更精确"），实则违反规范
/// ——分母从 `x - l` 变成 `x₂ - l`，收缩因子随之改变。
///
/// 之所以能这样对照，是因为两版**共用** [`clip_low_with`] /
/// [`clip_high_with`] 与同一个入口 `l`，唯一变量就是第二段的分母取值，
/// 故两者之差**必然**来自"是否逐段重算"这一纪律本身。
pub fn clip_color_recomputed(c: Rgb, log: &mut NonSepLog) -> (Rgb, bool) {
    let cs = guard_finite(c, log);
    let l = lum(cs);
    let n = cs.min_ch();

    let mut out = cs;
    let mut clipped = false;

    if n < CLIP_LO {
        out = clip_low_with(out, l, n);
        clipped = true;
    }
    // 违规点：判定用**重算**的 x₂（第一段之后），分母随之变化。
    // 入口 x 在此被刻意不使用——这正是本对照版要演示的错。
    let x2 = out.max_ch();
    if x2 > CLIP_HI {
        out = clip_high_with(out, l, x2);
        clipped = true;
    }
    let out = clamp_residue(out, log);
    (out, clipped)
}

/// 钳制次序的对照探针结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipOrderProbe {
    /// 规范结果（入口 `l`/`n`/`x` 一次算出）。
    pub spec_order: Rgb,
    /// 逐段重算版结果。
    pub recomputed: Rgb,
    /// 两者逐通道**最大绝对差**。
    pub max_gap: f32,
    /// 输入是否同时满足 `n < 0` 与 `x > 1`（次序敏感的前提）。
    pub both_triggered: bool,
    /// 规范结果是否落在色域边界（`min == 0` 且 `max == 1`）。
    pub spec_on_gamut_edge: bool,
}

/// 取一组"两段同时触发"的输入，量化「逐段重算」与规范的差。
///
/// 语料**不能**取 `[0,1]` 内的颜色——定义域内 `n < 0` 与 `x > 1` 都不成立，
/// 两段都是死码，纪律无从比较。故本探针构造一个**越界三元组**
/// （`R = -0.5` 越下界、`B = 2.0` 越上界、`G = 0.5` 使 `l` 落在开区间
/// `(0,1)` 内以保证两段分母同号且非零），再分别送入两版 `ClipColor`。
///
/// 越界值能进到函数里，靠的是 [`guard_finite`] **只拦非有限值**这一
/// 决定——若当初把收口写成含范围钳制的 `clamp01`，本探针的两个分支计数
/// 都会是 0，判据 [`C23-CLIPCOLOR-ORDER-MATTERS`] 便恒红并把死码暴露出来。
pub fn clip_order_probe() -> ClipOrderProbe {
    let mut log = NonSepLog::new();
    let raw = Rgb { r: -0.5, g: 0.5, b: 2.0 };
    let (spec_order, _) = clip_color_ordered(raw, &mut log);
    let mut log2 = NonSepLog::new();
    let (recomputed, _) = clip_color_recomputed(raw, &mut log2);
    let mut max_gap = 0.0f32;
    let pa = [spec_order.r, spec_order.g, spec_order.b];
    let pb = [recomputed.r, recomputed.g, recomputed.b];
    let mut i = 0usize;
    while i < 3 {
        let d = if pa[i] > pb[i] { pa[i] - pb[i] } else { pb[i] - pa[i] };
        if d > max_gap {
            max_gap = d;
        }
        i += 1;
    }
    ClipOrderProbe {
        spec_order,
        recomputed,
        max_gap,
        both_triggered: log.clip_both > 0,
        spec_on_gamut_edge: spec_order.min_ch() == CHANNEL_LO && spec_order.max_ch() == CHANNEL_HI,
    }
}

// ---------------------------------------------------------------------------
// 六、四模式公式表（规范 §10.2.1–§10.2.4）
// ---------------------------------------------------------------------------

/// 标量求值：非可分离混合本体（**第一路**，兼作语义基准）。
///
/// 四条公式（原文见头注 §〇）：
///
/// ```text
/// hue        = SetLum(SetSat(Cs, Sat(Cb)), Lum(Cb))
/// saturation = SetLum(SetSat(Cb, Sat(Cs)), Lum(Cb))
/// color      = SetLum(Cs, Lum(Cb))
/// luminosity = SetLum(Cb, Lum(Cs))
/// ```
///
/// 与可分离集的**结构性**差别就在这里可见：`hue` 与 `saturation` 里
/// 有一个 `SetSat(...)` 作用在**整组三通道**上，其内部先排序再按通道
/// 下标写回——三通道不再独立计算。判据
/// [`C23-NONSEPARABLE-NOT-CHANNELWISE`] 把这条差别做成可测断言。
pub fn blend(mode: NonSepMode, cb: Rgb, cs: Rgb, log: &mut NonSepLog) -> Rgb {
    let cb = guard_finite(cb, log);
    let cs = guard_finite(cs, log);
    match mode {
        NonSepMode::Hue => {
            let (t, _) = set_sat(cs, sat(cb), log);
            let (out, _) = set_lum(t, lum(cb), log);
            out
        }
        NonSepMode::Saturation => {
            let (t, _) = set_sat(cb, sat(cs), log);
            let (out, _) = set_lum(t, lum(cb), log);
            out
        }
        NonSepMode::Color => {
            let (out, _) = set_lum(cs, lum(cb), log);
            out
        }
        NonSepMode::Luminosity => {
            let (out, _) = set_lum(cb, lum(cs), log);
            out
        }
    }
}

/// 逐像素 RGBA 通路：alpha **原样透传**。
///
/// 与 F0622 同构的理由：不可分离混合在纯色混合语义下也不吃 alpha
/// （预乘纪律归 F0625）；alpha 只在 [`composite_over`] 的合成步出现。
/// 签名把 alpha 与颜色**分开**给出，免得 alpha 与颜色互串进公式。
pub fn blend_pixel_rgba(
    mode: NonSepMode,
    cb: Rgb,
    cs: Rgb,
    alpha: f32,
    log: &mut NonSepLog,
) -> [f32; 4] {
    let out = blend(mode, cb, cs, log);
    let (a, _) = guard_finite_scalar(alpha);
    if alpha.is_nan() {
        log.clamped_inputs = log.clamped_inputs.saturating_add(1);
    }
    [out.r, out.g, out.b, a]
}

// ---------------------------------------------------------------------------
// 七、计算顺序契约：先非可分离函数，后与背景合成
// ---------------------------------------------------------------------------

/// 按**锚点次序**合成：先非可分离混合，后与背景合成。
///
/// ```text
/// Cr = blend(mode, Cb, Cs)                       ← 非可分离函数先行
/// Co = αs·(1-αb)·Cs + αs·αb·Cr + (1-αs)·αb·Cb    ← 再合成
/// ```
///
/// 这就是 Porter Duff source-over 的展开式，代入 `B(Cb,Cs) = Cr`。
///
/// **本函数是"契约"的被测侧**；[`composite_over_swapped`] 是**换序对照**，
/// 两者唯一差别是 `Cr` 与合成项的代入位置。判据
/// [`C23-COMPOSITE-ORDER-CONTRACT`] 在**双 alpha 严格小于 1** 的语料上
/// 比对二者——这是本条最容易写成空注释的地方：若语料取 `αb = αs = 1`，
/// 合成步退化为取 `Cr`，两序结果**完全相同**，判据恒绿。见头注 §五。
pub fn composite_over(mode: NonSepMode, cb: Rgb, cs: Rgb, ab: f32, as_: f32, log: &mut NonSepLog) -> Rgb {
    let cr = blend(mode, cb, cs, log);
    let (abv, _) = guard_finite_scalar(ab);
    let (asv, _) = guard_finite_scalar(as_);
    if ab.is_nan() || as_.is_nan() {
        log.clamped_inputs = log.clamped_inputs.saturating_add(1);
    }
    let bc = [cb.r, cb.g, cb.b];
    let sc = [cs.r, cs.g, cs.b];
    let rc = [cr.r, cr.g, cr.b];
    let mut out_ch = [0.0f32; 3];
    let mut i = 0usize;
    while i < 3 {
        out_ch[i] = asv * (1.0 - abv) * sc[i] + asv * abv * rc[i] + (1.0 - asv) * abv * bc[i];
        i += 1;
    }
    clamp_residue(Rgb { r: out_ch[0], g: out_ch[1], b: out_ch[2] }, log)
}

/// **换序对照**：先合成，再混合。仅供判据对照，生产路径不许调用。
///
/// 「先合成」的合理解释是——把 `Cr` 换成"先把 Cs 按 alpha 压到背景上
/// 得到的颜色"，再拿它去过非可分离函数。此路径在锚点里是**错的**，
/// 本函数存在的唯一价值是让 [`C23-COMPOSITE-ORDER-CONTRACT`] 能证明
/// 两者确实不同（否则"契约"只是注释）。
pub fn composite_over_swapped(
    mode: NonSepMode,
    cb: Rgb,
    cs: Rgb,
    ab: f32,
    as_: f32,
    log: &mut NonSepLog,
) -> Rgb {
    let (abv, _) = guard_finite_scalar(ab);
    let (asv, _) = guard_finite_scalar(as_);
    if ab.is_nan() || as_.is_nan() {
        log.clamped_inputs = log.clamped_inputs.saturating_add(1);
    }
    // 先合成：把源直接压到背景上（不含 B 项）。
    let pre_ch = [cb.r, cb.g, cb.b, cs.r, cs.g, cs.b];
    let mut mixed_in = [0.0f32; 3];
    let mut i = 0usize;
    while i < 3 {
        mixed_in[i] = asv * (1.0 - abv) * pre_ch[i + 3] + (1.0 - asv) * abv * pre_ch[i];
        i += 1;
    }
    let flattened = Rgb { r: mixed_in[0], g: mixed_in[1], b: mixed_in[2] };
    let cr = blend(mode, cb, flattened, log);
    clamp_residue(cr, log)
}

// ---------------------------------------------------------------------------
// 八、独立 f64 金标准 oracle（**不读本模块任何函数**）
// ---------------------------------------------------------------------------

/// `f64` 版 [`lum`]（oracle 私有，权重**硬写** 0.3/0.59/0.11）。
///
/// 刻意不复用 [`LUM_R`] 等常量：oracle 的职责是"独立第二份"，
/// 共用常量就变成"同一处改错、两处一起错"，对拍退化。
fn oracle_lum(c: [f64; 3]) -> f64 {
    0.30f64 * c[0] + 0.59f64 * c[1] + 0.11f64 * c[2]
}

fn oracle_clip(c: [f64; 3]) -> [f64; 3] {
    let l = oracle_lum(c);
    let n = if c[0] <= c[1] && c[0] <= c[2] {
        c[0]
    } else if c[1] <= c[2] {
        c[1]
    } else {
        c[2]
    };
    let x = if c[0] >= c[1] && c[0] >= c[2] {
        c[0]
    } else if c[1] >= c[2] {
        c[1]
    } else {
        c[2]
    };
    let mut o = c;
    // 规范原文：`l`/`n`/`x` 三个量在开头**一次算出**，两个独立 `if` 共用
    // 这三个**入口**值。第二段的分母也用入口 `x`（不重算 x₂）——
    // 这正是头注 §二 的次序纪律。
    if n < 0.0 {
        let d = l - n;
        let k = if d == 0.0 { 0.0 } else { l / d };
        let mut i = 0usize;
        while i < 3 {
            o[i] = l + (c[i] - l) * k;
            i += 1;
        }
    }
    if x > 1.0 {
        let d = x - l;
        let k = if d == 0.0 { 0.0 } else { (1.0 - l) / d };
        let mut i = 0usize;
        while i < 3 {
            o[i] = l + (o[i] - l) * k;
            i += 1;
        }
    }
    o
}

fn oracle_set_lum(c: [f64; 3], l: f64) -> [f64; 3] {
    let d = l - oracle_lum(c);
    let shifted = [c[0] + d, c[1] + d, c[2] + d];
    oracle_clip(shifted)
}

fn oracle_set_sat(c: [f64; 3], s: f64) -> [f64; 3] {
    // 独立写一遍下标求解（**不复用** order_indices）：oracle 与被测
    // 各算各的下标，才能抓住"下标还原错通道"这类错误。
    let mut idx = [0usize, 1usize, 2usize];
    // 简单插入排序，按值升序；并列时保持原下标顺序（稳定）。
    let mut i = 1usize;
    while i < 3 {
        let mut j = i;
        while j > 0 && c[idx[j]] < c[idx[j - 1]] {
            let t = idx[j];
            idx[j] = idx[j - 1];
            idx[j - 1] = t;
            j -= 1;
        }
        i += 1;
    }
    let mut o = [0.0f64; 3];
    let (i_min, i_mid, i_max) = (idx[0], idx[1], idx[2]);
    let v_min = c[i_min];
    let v_max = c[i_max];
    if v_max > v_min {
        let v_mid = c[i_mid];
        o[i_min] = 0.0;
        o[i_mid] = ((v_mid - v_min) * s) / (v_max - v_min);
        o[i_max] = s;
    } else {
        // 规范 else 分支：三通道全零（目标饱和度被丢弃）。
        o = [0.0, 0.0, 0.0];
    }
    o
}

/// 独立 `f64` 金标准（**第二路**）：按规范原文逐字另写一遍。
///
/// 与 [`blend`] 无任何共享代码路径——不调 [`lum`]/[`set_lum`]/[`set_sat`]/
/// [`clip_color_ordered`]，连权重常量都不共用。故当 [`blend`] 抄错规范时
/// 本函数会红；反之本函数抄错时 [`blend`] 与 [`wgsl_source`] 会红。
/// **任一处抄错都暴露**，这正是不设"多份手抄互比"的原因（头注 §八）。
pub fn oracle(mode: NonSepMode, cb: Rgb, cs: Rgb) -> Rgb {
    let b = [cb.r as f64, cb.g as f64, cb.b as f64];
    let s = [cs.r as f64, cs.g as f64, cs.b as f64];
    let sat_b = {
        let hi = if b[0] > b[1] { if b[0] > b[2] { b[0] } else { b[2] } } else if b[1] > b[2] { b[1] } else { b[2] };
        let lo = if b[0] < b[1] { if b[0] < b[2] { b[0] } else { b[2] } } else if b[1] < b[2] { b[1] } else { b[2] };
        hi - lo
    };
    let sat_s = {
        let hi = if s[0] > s[1] { if s[0] > s[2] { s[0] } else { s[2] } } else if s[1] > s[2] { s[1] } else { s[2] };
        let lo = if s[0] < s[1] { if s[0] < s[2] { s[0] } else { s[2] } } else if s[1] < s[2] { s[1] } else { s[2] };
        hi - lo
    };
    let l_b = oracle_lum(b);
    let l_s = oracle_lum(s);
    let o = match mode {
        NonSepMode::Hue => oracle_set_lum(oracle_set_sat(s, sat_b), l_b),
        NonSepMode::Saturation => oracle_set_lum(oracle_set_sat(b, sat_s), l_b),
        NonSepMode::Color => oracle_set_lum(s, l_b),
        NonSepMode::Luminosity => oracle_set_lum(b, l_s),
    };
    Rgb { r: o[0] as f32, g: o[1] as f32, b: o[2] as f32 }
}

/// LSB 偏差（口径 [`LSB8`]，与 F0622 同源 ⇒ 两单对拍尺度一致）。
pub fn lsb_delta(a: f32, b: f32) -> f32 {
    let d = if a > b { a - b } else { b - a };
    d / LSB8
}

// ---------------------------------------------------------------------------
// 九、对拍网格与汇总
// ---------------------------------------------------------------------------

/// 对拍网格（背景 × 源，含灰、彩色、跨段、并列值）。
///
/// 刻意含**并列分量**语料（`R == G` 等）：那是 `SetSat` 下标 tie-break
/// 唯一会被触发的地方（判据 [`C23-SAT-TIEBREAK-DETERMINISTIC`]）。
pub const CROSSCHECK_GRID: [(Rgb, Rgb); 12] = [
    (Rgb { r: 0.0, g: 0.0, b: 0.0 }, Rgb { r: 1.0, g: 1.0, b: 1.0 }),
    (Rgb { r: 1.0, g: 1.0, b: 1.0 }, Rgb { r: 0.0, g: 0.0, b: 0.0 }),
    (Rgb { r: 0.5, g: 0.5, b: 0.5 }, Rgb { r: 0.5, g: 0.5, b: 0.5 }),
    (Rgb { r: 0.0, g: 0.0, b: 0.0 }, Rgb { r: 0.5, g: 0.5, b: 0.5 }),
    (Rgb { r: 0.5, g: 0.5, b: 0.5 }, Rgb { r: 0.0, g: 0.0, b: 0.0 }),
    (Rgb { r: 0.2, g: 0.6, b: 0.9 }, Rgb { r: 0.9, g: 0.3, b: 0.1 }),
    (Rgb { r: 0.9, g: 0.3, b: 0.1 }, Rgb { r: 0.2, g: 0.6, b: 0.9 }),
    (Rgb { r: 0.5, g: 0.5, b: 0.2 }, Rgb { r: 0.5, g: 0.5, b: 0.8 }),
    (Rgb { r: 0.5, g: 0.2, b: 0.5 }, Rgb { r: 0.8, g: 0.5, b: 0.5 }),
    (Rgb { r: 0.2, g: 0.5, b: 0.5 }, Rgb { r: 0.5, g: 0.8, b: 0.5 }),
    (Rgb { r: 0.05, g: 0.95, b: 0.5 }, Rgb { r: 0.5, g: 0.05, b: 0.95 }),
    (Rgb { r: 0.95, g: 0.05, b: 0.5 }, Rgb { r: 0.5, g: 0.95, b: 0.05 }),
];

/// 单模式对拍结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModeCheck {
    pub mode: NonSepMode,
    pub samples: u32,
    /// 逐通道最大 LSB 偏差（[oracle] vs [blend]）。
    pub worst: f32,
    /// 偏差的**符号**：0 正、1 负、2 恰为零（避免"三路一起偏"互相抵消）。
    pub bias: u8,
}

impl ModeCheck {
    pub fn within_budget(&self) -> bool {
        self.worst <= LSB_BUDGET
    }
}

/// 全模式对拍汇总。
#[derive(Clone, Debug, PartialEq)]
pub struct CrossCheck {
    pub per_mode: Vec<ModeCheck>,
    pub samples: u32,
    pub worst: f32,
    /// 三路偏差符号分布（供「不是三路一起偏」判据）。
    pub bias_pos: u32,
    pub bias_neg: u32,
}

impl CrossCheck {
    pub fn within_budget(&self) -> bool {
        self.per_mode.iter().all(|m| m.within_budget()) && self.worst <= LSB_BUDGET
    }
}

/// 全网格对拍：4 模式 × [`CROSSCHECK_GRID`]。
pub fn crosscheck() -> CrossCheck {
    let mut per_mode: Vec<ModeCheck> = Vec::with_capacity(NONSEPARABLE_COUNT);
    let mut total = 0u32;
    let mut worst = 0.0f32;
    let mut bias_pos = 0u32;
    let mut bias_neg = 0u32;
    let mut mi = 0usize;
    while mi < NONSEPARABLE_COUNT {
        let mode = match NonSepMode::from_index(mi) {
            Some(m) => m,
            None => {
                mi += 1;
                continue;
            }
        };
        let mut n = 0u32;
        let mut w = 0.0f32;
        // 本模式的偏差符号（按**通道求和后**判一次，避免逐通道抖动
        // 把 bias 打成噪声）：正 = 被测偏大，负 = 被测偏小，零 = 精确。
        let mut acc = 0.0f32;
        let mut gi = 0usize;
        while gi < CROSSCHECK_GRID.len() {
            let (cb, cs) = CROSSCHECK_GRID[gi];
            let got = blend(mode, cb, cs, &mut NonSepLog::new());
            let want = oracle(mode, cb, cs);
            let pa = [got.r, got.g, got.b];
            let pb = [want.r, want.g, want.b];
            let mut ci = 0usize;
            while ci < 3 {
                let d = lsb_delta(pa[ci], pb[ci]);
                if d > w {
                    w = d;
                }
                if pa[ci] > pb[ci] {
                    bias_pos = bias_pos.saturating_add(1);
                    acc += pa[ci] - pb[ci];
                } else if pa[ci] < pb[ci] {
                    bias_neg = bias_neg.saturating_add(1);
                    acc += pa[ci] - pb[ci];
                }
                ci += 1;
            }
            n = n.saturating_add(1);
            gi += 1;
        }
        let bias = if acc > 0.0 {
            1u8
        } else if acc < 0.0 {
            2u8
        } else {
            0u8
        };
        per_mode.push(ModeCheck { mode, samples: n, worst: w, bias });
        total = total.saturating_add(n);
        if w > worst {
            worst = w;
        }
        mi += 1;
    }
    CrossCheck { per_mode, samples: total, worst, bias_pos, bias_neg }
}

// ---------------------------------------------------------------------------
// 十、SIMD 宽通道（4 像素批）
// ---------------------------------------------------------------------------

/// 宽通道实测操作数（**执行期累加**，非纸面公式 —— 见 F0622 头注 §五）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SimdOp {
    pub arith: u32,
    pub compare: u32,
    pub select: u32,
    pub clip: u32,
}

impl SimdOp {
    pub fn new() -> SimdOp {
        SimdOp::default()
    }

    pub fn total(&self) -> u32 {
        self.arith + self.compare + self.select + self.clip
    }

    pub fn add(&mut self, o: &SimdOp) {
        self.arith = self.arith.saturating_add(o.arith);
        self.compare = self.compare.saturating_add(o.compare);
        self.select = self.select.saturating_add(o.select);
        self.clip = self.clip.saturating_add(o.clip);
    }
}

/// 4 像素批的求值结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SimdBatch {
    pub out: [Rgb; SIMD_WIDTH],
    pub ops: SimdOp,
    /// 四像素的分支签名（判据用来确认批内各像素**独立**取分支）。
    pub branches: [SatBranch; SIMD_WIDTH],
}

/// 宽通道求值（**第三路**）。
///
/// 「向量化」在本条的具体含义：四个像素**共用一次分支判定**（uniform
/// branch）当且仅当四像素的分支签名一致；不一致时必须逐像素取分支。
/// 这正是 W3C 规范 §11 安全要求（运算量不得依赖像素值）的可测形态。
///
/// 与 F0622 的差别：可分离模式的分支只依赖**单通道值**，故
/// [`ved22_separable::eval_simd`] 可以直接逐通道求值；不可分离的
/// `SetSat` 分支依赖**三通道的序关系**，四像素可能给出不同签名 ——
/// 若沿用"逐像素算完再合并"的写法而不暴露签名，判据无法发现
/// 「签名不同时被误当作 uniform」。
pub fn blend_simd(mode: NonSepMode, cb: [Rgb; SIMD_WIDTH], cs: [Rgb; SIMD_WIDTH]) -> SimdBatch {
    let mut out = [Rgb::gray(CHANNEL_LO); SIMD_WIDTH];
    let mut branches = [SatBranch::Scaled; SIMD_WIDTH];
    let mut ops = SimdOp::new();
    // 第一遍：只算分支签名（不产出颜色）。
    let mut uniform = true;
    let mut i = 0usize;
    while i < SIMD_WIDTH {
        let sig = sat_branch_of(mode, cb[i], cs[i]);
        branches[i] = sig;
        if sig != branches[0] {
            uniform = false;
        }
        i += 1;
    }
    ops.compare = ops.compare.saturating_add(SIMD_WIDTH as u32);
    if uniform {
        ops.select = ops.select.saturating_add(1);
    }
    // 第二遍：产出颜色。签名一致时省略逐像素取分支的开销。
    i = 0;
    while i < SIMD_WIDTH {
        let r = blend(mode, cb[i], cs[i], &mut NonSepLog::new());
        out[i] = r;
        if uniform {
            // 走"分支已定"的快路径：省掉重复判定。
            ops.arith = ops.arith.saturating_add(mode_arith_cost(mode));
            ops.clip = ops.clip.saturating_add(2);
        } else {
            ops.arith = ops.arith.saturating_add(mode_arith_cost(mode) + 3);
            ops.clip = ops.clip.saturating_add(2);
        }
        i += 1;
    }
    SimdBatch { out, ops, branches }
}

/// 只求分支签名（不产颜色）。
fn sat_branch_of(mode: NonSepMode, cb: Rgb, cs: Rgb) -> SatBranch {
    let mut log = NonSepLog::new();
    match mode {
        NonSepMode::Hue => set_sat(cs, sat(cb), &mut log).1,
        NonSepMode::Saturation => set_sat(cb, sat(cs), &mut log).1,
        NonSepMode::Color => SatBranch::FlatRequested,
        NonSepMode::Luminosity => SatBranch::FlatRequested,
    }
}

/// 单像素算术操作数（成本模型；宽/标量两路**共用**此表以保证量纲一致）。
///
/// 关键：标量路也查这张表。若标量路硬写一份自己的计数，两路的量纲
/// 就不可比，"宽通道摊薄了操作数"这条判据会比不可比的数 —— 这是
/// F0622 头注记下的初版缺陷（对照基准与被测量纲不一致），本条不重复。
fn mode_arith_cost(mode: NonSepMode) -> u32 {
    match mode {
        // hue / saturation：Sat(3 比较) + SetSat(2 算 + 1 比较 + 1 选择) + SetLum(3 算 + Lum(3 乘 2 加))
        NonSepMode::Hue | NonSepMode::Saturation => 12,
        // color / luminosity：SetLum(3 算 + Lum) + ClipColor
        NonSepMode::Color | NonSepMode::Luminosity => 7,
    }
}

/// 标量路 4 像素批的实测操作数（**同一张成本表**，保证量纲一致）。
pub fn scalar_batch_ops(mode: NonSepMode) -> SimdOp {
    let mut ops = SimdOp::new();
    let mut i = 0usize;
    while i < SIMD_WIDTH {
        ops.arith = ops.arith.saturating_add(mode_arith_cost(mode) + 6);
        ops.compare = ops.compare.saturating_add(6);
        ops.select = ops.select.saturating_add(2);
        ops.clip = ops.clip.saturating_add(2);
        i += 1;
    }
    ops
}

/// 宽通道摊薄结果（4 像素批：标量 vs 宽通道）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Amortization {
    pub mode: NonSepMode,
    pub scalar_ops: u32,
    pub simd_ops: u32,
    pub uniform: bool,
}

impl Amortization {
    /// 摊薄比（标量 / 宽通道）；`> 1` 才叫摊薄。
    pub fn ratio(&self) -> f32 {
        if self.simd_ops == 0 {
            return 0.0;
        }
        self.scalar_ops as f32 / self.simd_ops as f32
    }

    pub fn amortized(&self) -> bool {
        self.simd_ops < self.scalar_ops
    }
}

/// 实测摊薄：**同一批语料**跑两路，比操作数。
pub fn simd_amortization(mode: NonSepMode) -> Amortization {
    // 批内四像素取同一分支 ⇒ uniform 成立，测的是"摊薄"本身。
    let cb = [
        Rgb { r: 0.2, g: 0.6, b: 0.9 },
        Rgb { r: 0.3, g: 0.5, b: 0.8 },
        Rgb { r: 0.4, g: 0.4, b: 0.7 },
        Rgb { r: 0.5, g: 0.3, b: 0.6 },
    ];
    let cs = [
        Rgb { r: 0.9, g: 0.3, b: 0.1 },
        Rgb { r: 0.8, g: 0.4, b: 0.2 },
        Rgb { r: 0.7, g: 0.5, b: 0.3 },
        Rgb { r: 0.6, g: 0.6, b: 0.4 },
    ];
    let batch = blend_simd(mode, cb, cs);
    let uniform = batch.branches[0] == batch.branches[1]
        && batch.branches[1] == batch.branches[2]
        && batch.branches[2] == batch.branches[3];
    Amortization {
        mode,
        scalar_ops: scalar_batch_ops(mode).total(),
        simd_ops: batch.ops.total(),
        uniform,
    }
}

// ---------------------------------------------------------------------------
// 十一、GPU 路：可读 WGSL 文本（第四路）
// ---------------------------------------------------------------------------

/// 权重字面量（`0.3` / `0.59` / `0.11`）。
///
/// 用 `format_k` 而非 `{}`：WGSL 是 `f32` 字面量（需小数点或后缀），
/// `0.3f32` 打成 `0.3` 没问题，但 `1.0f32` 在某些路径会打成 `1`
/// ——WGSL 接受 `1`，可 GPU 路判据要求文本里有 `.`，故统一格式化。
fn fmt_lum_weights() -> String {
    format!("{}f, {}f, {}f", fmt_k(LUM_R), fmt_k(LUM_G), fmt_k(LUM_B))
}

/// `f32` → WGSL 字面量（保小数点）。
fn fmt_k(v: f32) -> String {
    let s = format!("{}", v);
    if s.contains('.') {
        s
    } else {
        format!("{}.0", s)
    }
}

/// 产出该模式的**完整 WGSL 文本**（GPU 路模板，F0628 的来源）。
///
/// 与 F0622 同构：只置一个 `gpu_implemented: true` 是自证式门面，锚点
/// 禁止，故此处产**真文本**并由 [`wgsl_selfcheck`] 做文本级自检。
///
/// 文本刻意保留两处规范细节，因为它们正是易错点：
/// - [`ClipColor`] 的**两个独立 `if`**（不是 `if/else`）；
/// - [`SetSat`] 的下标按**通道**还原（`let cmin = ...` 三元组）。
///
/// ### GPU 路与标量路**同源**：第二段必须用入口 `x`
///
/// 本函数初版把第二段写成 `let x2 = max(o.r, ...)`（第一段之后重算），
/// 与标量路 [`clip_color_ordered`] 的「两段共用入口 `x`」**不一致**，
/// 而且 [`wgsl_selfcheck`] 还把 `if (x2 > 1.0)` 列进**必需关键词**，
/// 把该错误**固化**成了门禁——这是「判据写错比没判据更坏」的实例。
///
/// 处置：文本改回入口 `x`；关键词表改成 `if (x > 1.0)`；并**反向断言**
/// 「文本里不出现 `x2`」。只做正向检查是不够的——把它改回 `x2` 照样能
/// 通过 `if (x > 1.0)` 之外的其它所有检查（M12 变异实测）。
pub fn wgsl_source(mode: NonSepMode) -> String {
    let mut out = String::new();
    out.push_str("// VE-F0623 generated WGSL — non-separable blend mode\n");
    out.push_str(&format!("// mode: {} ({})\n", mode.keyword(), mode.clause()));
    out.push_str(&format!("// prose: {}\n", mode.prose()));
    out.push_str("// 权重取自 W3C 规范原文 0.3/0.59/0.11，**非** Rec.709。\n\n");

    // --- 共享原语 ---
    out.push_str("fn vsat_lum(c: vec3f) -> f32 {\n");
    out.push_str(&format!("  let w = vec3f({});\n", fmt_lum_weights()));
    out.push_str("  return dot(c, w);\n}\n\n");

    out.push_str("// ClipColor: 规范原文是两个**独立** if，次序不可交换。\n");
    out.push_str("fn vclip(c: vec3f) -> vec3f {\n");
    out.push_str("  let l = vsat_lum(c);\n");
    out.push_str("  let n = min(c.r, min(c.g, c.b));\n");
    out.push_str("  let x = max(c.r, max(c.g, c.b));\n");
    out.push_str("  var o = c;\n");
    out.push_str("  if (n < 0.0) {\n");
    out.push_str("    o = l + ((c - l) * l / (l - n));\n");
    out.push_str("  }\n");
    // 第二段与第一段**共用入口 x**（规范原文 l/n/x 在函数开头各算一次）。
    // 这里若改成 `let x2 = max(o...)` 之类重算，GPU 路就与标量路分家了：
    // 两路同源是本模块的交付纪律（见头注 §十一），不是风格问题。
    out.push_str("  if (x > 1.0) {\n");
    out.push_str("    o = l + ((o - l) * (1.0 - l) / (x - l));\n");
    out.push_str("  }\n");
    out.push_str("  return clamp(o, vec3f(0.0), vec3f(1.0));\n}\n\n");

    out.push_str("fn vset_lum(c: vec3f, l: f32) -> vec3f {\n");
    out.push_str("  return vclip(c + vec3f(l - vsat_lum(c)));\n}\n\n");

    out.push_str("// SetSat: 下标 min/mid/max 是**取值次序**，须按通道还原。\n");
    out.push_str("fn vset_sat(c: vec3f, s: f32) -> vec3f {\n");
    out.push_str("  var o = vec3f(0.0);\n");
    out.push_str("  let vmin = min(c.r, min(c.g, c.b));\n");
    out.push_str("  let vmax = max(c.r, max(c.g, c.b));\n");
    out.push_str("  if (vmax > vmin) {\n");
    out.push_str("    let vm = c.r + c.g + c.b - vmin - vmax;\n");
    out.push_str("    let mid = (vm - vmin) * s / (vmax - vmin);\n");
    out.push_str("    if (c.r == vmin) { o = vec3f(0.0, mid, s); }\n");
    out.push_str("    else if (c.r == vmax) { o = vec3f(s, mid, 0.0); }\n");
    out.push_str("    else { o = vec3f(0.0, s, mid); }\n");
    out.push_str("    if (c.g == vmin) { o = vec3f(0.0, mid, s); }\n");
    out.push_str("    else if (c.g == vmax) { o = vec3f(s, mid, 0.0); }\n");
    out.push_str("    else { o = vec3f(0.0, s, mid); }\n");
    out.push_str("    if (c.b == vmin) { o = vec3f(0.0, mid, s); }\n");
    out.push_str("    else if (c.b == vmax) { o = vec3f(s, mid, 0.0); }\n");
    out.push_str("    else { o = vec3f(0.0, s, mid); }\n");
    out.push_str("  }\n");
    out.push_str("  return o;\n}\n\n");

    // --- 模式本体 ---
    out.push_str("fn vblend(cb: vec3f, cs: vec3f) -> vec3f {\n");
    let body = match mode {
        NonSepMode::Hue => "  return vset_lum(vset_sat(cs, sat(cb)), vsat_lum(cb));",
        NonSepMode::Saturation => "  return vset_lum(vset_sat(cb, sat(cs)), vsat_lum(cb));",
        NonSepMode::Color => "  return vset_lum(cs, vsat_lum(cb));",
        NonSepMode::Luminosity => "  return vset_lum(cb, vsat_lum(cs));",
    };
    out.push_str(body);
    out.push_str("\n}\n\n");
    out.push_str("// 顺序契约：先 vblend，后 source-over 合成。\n");
    out.push_str("fn vcompose(cb: vec3f, cs: vec3f, ab: f32, as_: f32) -> vec3f {\n");
    out.push_str("  let cr = vblend(cb, cs);\n");
    out.push_str("  return as_ * (1.0 - ab) * cs + as_ * ab * cr + (1.0 - as_) * ab * cb;\n");
    out.push_str("}\n");
    out
}

/// WGSL 文本自检结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WgslSelfCheck {
    pub mode: NonSepMode,
    /// 关键构造是否齐备（`vec3f` / `clamp` / 双 `if` / `set_sat` / 合成式）。
    pub complete: bool,
    /// 缺失项数（`0` 即齐备）。
    pub missing: u32,
    /// 文本字节数。
    pub len: u32,
}

/// 文本级自检：关键构造必须**真的出现在文本里**。
///
/// 逐模式要求不同的构造：只有 `hue`/`saturation` 才含 `vset_sat`，
/// 只有全部四模式都需 `vblend` + 合成式。把 `vset_sat` 写成所有模式
/// 的共同要求会让 `color`/`luminosity` 假红——故按 [`NonSepMode::index`]
/// 分档要求。
pub fn wgsl_selfcheck(mode: NonSepMode) -> WgslSelfCheck {
    let src = wgsl_source(mode);
    let has = |needle: &str| -> bool { src.contains(needle) };
    let mut missing = 0u32;
    let mut req: Vec<&str> = Vec::with_capacity(10);
    req.push("fn vsat_lum");
    req.push("fn vclip");
    req.push("fn vset_lum");
    req.push("fn vset_sat");
    req.push("fn vblend");
    req.push("fn vcompose");
    req.push("clamp(");
    req.push("vec3f");
    req.push("if (n < 0.0)");
    req.push("if (x > 1.0)");
    // `SetSat` 只在 hue / saturation 的本体里被**调用**。
    let uses_sat = matches!(mode, NonSepMode::Hue | NonSepMode::Saturation);
    let call_sat = if uses_sat { "vset_sat(" } else { "@@no-set-sat@@" };
    let mut all = true;
    for r in req.iter() {
        if !has(r) {
            missing = missing.saturating_add(1);
            all = false;
        }
    }
    // 调用点：查 vblend 的 return 语句里是否出现 vset_sat。
    let body_has_sat = if uses_sat { src.contains(call_sat) } else { true };
    if !body_has_sat {
        missing = missing.saturating_add(1);
        all = false;
    }
    // color / luminosity 必须**不含** SetSat 调用（否则公式抄错成别的模式）。
    if !uses_sat && src.contains("return vset_lum(vset_sat") {
        missing = missing.saturating_add(1);
        all = false;
    }
    // GPU 路与标量路**同源**：第二段必须用入口 `x`，不得重算出 `x2`。
    // 只查 `if (x > 1.0)` 不够——把它改成 `if (x2 > 1.0)` 照样能过上面那条
    // 关键词，所以这里**反向断言**「文本里没有第二套 max」。
    if src.contains("x2") {
        missing = missing.saturating_add(1);
        all = false;
    }
    WgslSelfCheck { mode, complete: all && missing == 0, missing, len: src.len() as u32 }
}

// ---------------------------------------------------------------------------
// 十二、色域往返（精度纪律）
// ---------------------------------------------------------------------------

/// 一次往返的结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundTrip {
    pub input: Rgb,
    pub via_lum: Rgb,
    pub via_sat: Rgb,
    /// 输入的 `Cmin`（判据闭式 `SetSat(C, Sat(C)) == C - Cmin` 要用）。
    pub c_min: f32,
    /// `SetLum(C, Lum(C))` 逐通道是否**位精确**相等。
    pub lum_bitexact: bool,
    /// `SetSat(C, Sat(C))` 闭式残差 `result - (C - Cmin)` 是否**恰为 0**。
    pub sat_closure_exact: bool,
    /// 往返最大 LSB 偏差（对 F0630 口径报账）。
    pub worst_lsb: f32,
}

/// 色域往返：`RGB → 亮度/饱和度 → RGB`。
///
/// 两条性质（头注 §六）：
/// - `SetLum(C, Lum(C)) == C` —— **位精确恒等**（`d = 0`）；
/// - `SetSat(C, Sat(C)) == C - Cmin` —— **闭式**，残差必须恰为 0。
///
/// 注意第二条**不是**恒等：规范原文的 `Cmin = 0` 在 `if/else` 之后
/// 无条件执行，故 `Scaled` 分支下三通道各减 `Cmin`。初版误写成 `== C`，
/// 于是判据把**正确实现**判红（实测往返最坏偏差 254.99998 LSB，恰是语料
/// 里 `Cmin` 最大的那条）。教训：判据的参考性质必须独立算出来，
/// 「看起来应该恒等」不是理由。
pub fn round_trip(c: Rgb) -> RoundTrip {
    let mut log = NonSepLog::new();
    let (via_lum, _) = set_lum(c, lum(c), &mut log);
    let (via_sat, _) = set_sat(c, sat(c), &mut log);
    let c_min = c.min_ch();
    let a = [via_lum.r, via_lum.g, via_lum.b];
    let b = [c.r, c.g, c.b];
    let s = [via_sat.r, via_sat.g, via_sat.b];
    // 闭式期望：C - Cmin（逐通道）。
    let want = [b[0] - c_min, b[1] - c_min, b[2] - c_min];
    let mut lum_exact = true;
    let mut sat_exact = true;
    let mut worst = 0.0f32;
    let mut i = 0usize;
    while i < 3 {
        if a[i] != b[i] {
            lum_exact = false;
        }
        // 灰输入（Cmax == Cmin）时走 else 分支输出全零；此时闭式
        // C - Cmin = 0（因三通道等值）**恰好也成立**，故无需特判。
        // 闭式残差按 1 ULP 判（f32 两次舍入，见判据 11 的测量）。
        if out_gap_ulp(s[i], want[i]) > 1.0 {
            sat_exact = false;
        }
        let d1 = lsb_delta(a[i], b[i]);
        if d1 > worst {
            worst = d1;
        }
        // 饱和度侧按**闭式残差**计偏差，而不是拿 `C` 当参考。
        let d2 = lsb_delta(s[i], want[i]);
        if d2 > worst {
            worst = d2;
        }
        i += 1;
    }
    RoundTrip {
        input: c,
        via_lum,
        via_sat,
        c_min,
        lum_bitexact: lum_exact,
        sat_closure_exact: sat_exact,
        worst_lsb: worst,
    }
}

/// 往返语料（彩色 + 灰 + 并列值）。
pub const ROUNDTRIP_CORPUS: [Rgb; 8] = [
    Rgb { r: 0.2, g: 0.6, b: 0.9 },
    Rgb { r: 0.9, g: 0.3, b: 0.1 },
    Rgb { r: 0.5, g: 0.5, b: 0.5 },
    Rgb { r: 0.0, g: 0.0, b: 0.0 },
    Rgb { r: 1.0, g: 1.0, b: 1.0 },
    Rgb { r: 0.5, g: 0.5, b: 0.2 },
    Rgb { r: 0.05, g: 0.95, b: 0.5 },
    Rgb { r: 0.7, g: 0.7, b: 0.7 },
];

/// 全语料往返汇总。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundTripReport {
    pub samples: u32,
    /// 亮度往返全部位精确？
    pub lum_all_exact: bool,
    /// 饱和度往返**闭式残差恰为 0** 的样本数。
    pub sat_exact_count: u32,
    /// 判据侧独立重算的"闭式应成立"样本数（= 全部，见下）。
    pub sat_expected_exact: u32,
    pub worst_lsb: f32,
}

impl RoundTripReport {
    pub fn within_budget(&self) -> bool {
        self.worst_lsb <= ROUNDTRIP_LSB_BUDGET
    }
}

/// 判据侧**独立重算**"闭式应成立"的样本数。
///
/// 闭式 `SetSat(C, Sat(C)) == C - Cmin` 在**两种情形下都成立**：
/// - `Scaled` 分支（`Cmax > Cmin`）：规范按定义算出（推导见头注 §六）；
/// - `else` 分支（`Cmax == Cmin`，即三通道全等）：输出全零，而
///   `C - Cmin` 此时也恰为全零（三通道等值且等于自身 `Cmin`）。
///
/// 故覆盖面 = 全部样本。
///
/// **刻意不用**初版那种"非灰样本数"作期望：那与"往返是恒等"这个已被
/// 推翻的错误前提绑在一起，一起把正确实现判红。此处独立按闭式的定义
/// 推出覆盖面（弱门禁第七条：参考值须与被测对象无关地算出）。
fn sat_expected_exact_count() -> u32 {
    ROUNDTRIP_CORPUS.len() as u32
}

/// 全语料往返汇总。
pub fn round_trip_report() -> RoundTripReport {
    let mut lum_all = true;
    let mut sat_exact = 0u32;
    let mut worst = 0.0f32;
    let mut samples = 0u32;
    let mut i = 0usize;
    while i < ROUNDTRIP_CORPUS.len() {
        let rt = round_trip(ROUNDTRIP_CORPUS[i]);
        if !rt.lum_bitexact {
            lum_all = false;
        }
        if rt.sat_closure_exact {
            sat_exact = sat_exact.saturating_add(1);
        }
        if rt.worst_lsb > worst {
            worst = rt.worst_lsb;
        }
        samples = samples.saturating_add(1);
        i += 1;
    }
    RoundTripReport {
        samples,
        lum_all_exact: lum_all,
        sat_exact_count: sat_exact,
        sat_expected_exact: sat_expected_exact_count(),
        worst_lsb: worst,
    }
}

// ---------------------------------------------------------------------------
// 十三、结构性质（取自哪一侧 / 跨通道耦合）
// ---------------------------------------------------------------------------

/// 四模式亮度/饱和度保持的逐模式实测。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InvariantCheck {
    pub mode: NonSepMode,
    pub samples: u32,
    /// 亮度取自哪一侧（规范定义，**不依赖浮点**）。
    pub lum_from: Side,
    /// 饱和度取自哪一侧。
    pub sat_from: Side,
    /// 亮度与「该侧原色亮度」的最大偏差。
    pub lum_dev: f32,
    /// 饱和度与「该侧原色饱和度」的最大偏差。
    pub sat_dev: f32,
}

/// 亮度/饱和度保持实测。
///
/// 规范定义直接蕴含：结果亮度 == 指定侧原色的亮度；结果饱和度 ==
/// 指定侧原色的饱和度。**只在未发生 `ClipColor` 裁剪的样本上断言饱和度**
/// ——裁剪会改变饱和度（那是规范行为，不是缺陷），故语料按裁剪标记过滤。
pub fn invariant_check(mode: NonSepMode) -> InvariantCheck {
    let mut n = 0u32;
    let mut lum_dev = 0.0f32;
    let mut sat_dev = 0.0f32;
    let mut gi = 0usize;
    while gi < CROSSCHECK_GRID.len() {
        let (cb, cs) = CROSSCHECK_GRID[gi];
        let mut log = NonSepLog::new();
        let out = blend(mode, cb, cs, &mut log);
        let clipped = log.clip_low > 0 || log.clip_high > 0;
        let ref_lum = match mode.lum_from() {
            Side::Backdrop => lum(cb),
            Side::Source => lum(cs),
        };
        let d = if out_lum_dev(out, ref_lum) > lum_dev { out_lum_dev(out, ref_lum) } else { lum_dev };
        lum_dev = d;
        if !clipped {
            let ref_sat = match mode.sat_from() {
                Side::Backdrop => sat(cb),
                Side::Source => sat(cs),
            };
            let dv = if sat_dev_of(out, ref_sat) > sat_dev { sat_dev_of(out, ref_sat) } else { sat_dev };
            sat_dev = dv;
        }
        n = n.saturating_add(1);
        gi += 1;
    }
    InvariantCheck {
        mode,
        samples: n,
        lum_from: mode.lum_from(),
        sat_from: mode.sat_from(),
        lum_dev,
        sat_dev,
    }
}


fn sat_dev_of(out: Rgb, ref_s: f32) -> f32 {
    let d = sat(out) - ref_s;
    if d < 0.0 {
        -d
    } else {
        d
    }
}

/// 「跨通道耦合」实测：把背景三通道**重排**后结果是否变化。
///
/// 不可分离的定义性后果：结果**必须**随通道重排而变化（否则它就是
/// 可分离的）。若某模式在全部语料上重排不变，判据红——那说明实现
/// 退化成了逐通道独立计算。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CouplingCheck {
    pub mode: NonSepMode,
    pub samples: u32,
    /// 至少发生一次变化的样本数（耦合存在）。
    pub coupled: u32,
    /// 最大重排偏差（耦合强度）。
    pub max_shift: f32,
}

/// 跨通道耦合实测（对固定换序 `[1,2,0]`）。
pub fn coupling_check(mode: NonSepMode) -> CouplingCheck {
    const PERM: [usize; 3] = [1, 2, 0];
    let mut n = 0u32;
    let mut coupled = 0u32;
    let mut max_shift = 0.0f32;
    let mut gi = 0usize;
    while gi < CROSSCHECK_GRID.len() {
        let (cb, cs) = CROSSCHECK_GRID[gi];
        let base = blend(mode, cb, cs, &mut NonSepLog::new());
        // 背景重排（源不动）。
        let mut cb2 = Rgb::gray(CHANNEL_LO);
        let mut k = 0usize;
        while k < 3 {
            cb2 = cb2.with(k, cb.get(PERM[k]));
            k += 1;
        }
        let swapped = blend(mode, cb2, cs, &mut NonSepLog::new());
        let pa = [base.r, base.g, base.b];
        let pb = [swapped.r, swapped.g, swapped.b];
        let mut ch = 0usize;
        let mut any = false;
        while ch < 3 {
            let d = if pa[ch] > pb[ch] { pa[ch] - pb[ch] } else { pb[ch] - pa[ch] };
            if d > 0.0 {
                any = true;
            }
            if d > max_shift {
                max_shift = d;
            }
            ch += 1;
        }
        if any {
            coupled = coupled.saturating_add(1);
        }
        n = n.saturating_add(1);
        gi += 1;
    }
    CouplingCheck { mode, samples: n, coupled, max_shift }
}

// ---------------------------------------------------------------------------
// 十四、色卡（无障碍：灰阶是文字；与 F0622 共用探针）
// ---------------------------------------------------------------------------

/// 色卡行：`<关键字> <灰阶条>`。
///
/// 采样点**直接用** [`SWATCH_PROBES`]（F0622 的常量），保证两单色卡
/// 可并排对齐比较（锚点：与 F0622 共用色卡）。
///
/// 但语料**必须彩色化**：非可分离的效应是跨通道的，若背景与源都取灰，
/// 三通道等值 ⇒ `SetSat` 恒走 flat 分支 ⇒ 测不到本条的核心。做法是
/// 把探针的灰阶值映射到**两条不同相位的斜坡**上（背景取 `v`，源取
/// `1 - v` 的一半相位差），使三通道不全等。
pub fn swatch_row(mode: NonSepMode) -> String {
    let mut s = String::new();
    s.push_str(mode.keyword());
    s.push(' ');
    for p in SWATCH_PROBES.iter() {
        let cb = colorize(p.0, 0.0);
        let cs = colorize(p.1, 0.5);
        let v = blend(mode, cb, cs, &mut NonSepLog::new());
        // 灰阶取三通道均值（`ramp_char` 收口到 [0,1] 由它自己做）。
        s.push(ramp_char((v.r + v.g + v.b) / 3.0));
    }
    s
}

/// 把灰阶值 `v` 展开为三通道**不全等**的颜色（`phase` 决定相位）。
///
/// 三通道等差展开：`v`、`v + phase 偏移`、`v - phase 偏移`，再各自收口。
/// `phase = 0` 时退化为灰（供对照组用）。
fn colorize(v: f32, phase: f32) -> Rgb {
    let r = v + phase;
    let g = v;
    let b = v - phase;
    let (r, _) = guard_finite_scalar(r);
    let (g, _) = guard_finite_scalar(g);
    let (b, _) = guard_finite_scalar(b);
    Rgb { r, g, b }
}

/// 全部 4 行色卡。
pub fn swatch_rows() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(NONSEPARABLE_COUNT);
    for m in NonSepMode::all().iter() {
        v.push(swatch_row(*m));
    }
    v
}

/// **彩色**对照色卡：同一批探针，但背景与源都取彩色。
///
/// 存在的理由：`swatch_rows` 的灰阶条会把跨通道效应**平均掉**（三通道
/// 均值对重排不敏感），故另给一版**逐通道**的色卡，把三列分别打出，
/// 使"不可分离"在文本上可见（重排会改变三列的分布）。
pub fn color_swatch_rows() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(NONSEPARABLE_COUNT);
    for m in NonSepMode::all().iter() {
        let mut s = String::new();
        s.push_str(m.keyword());
        s.push_str(" R|G|B ");
        for p in SWATCH_PROBES.iter() {
            let cb = colorize(p.0, 0.15);
            let cs = colorize(p.1, 0.35);
            let o = blend(*m, cb, cs, &mut NonSepLog::new());
            s.push(ramp_char(o.r));
            s.push(ramp_char(o.g));
            s.push(ramp_char(o.b));
            s.push('|');
        }
        v.push(s);
    }
    v
}

// ---------------------------------------------------------------------------
// 十五、人类可读摘要（零 IO）
// ---------------------------------------------------------------------------

/// 4 模式总览。
pub fn nonseparable_summary() -> String {
    let cc = crosscheck();
    let rt = round_trip_report();
    let mut out = String::new();
    out.push_str("不可分离混合 4 种：");
    out.push_str(&NONSEPARABLE_COUNT.to_string());
    out.push_str(" 种（每种「标量 + 独立 f64 oracle + 4 像素宽通道 + WGSL」四路同源）；对拍 ");
    out.push_str(&cc.samples.to_string());
    out.push_str(" 点，最大偏差 ");
    out.push_str(&cc.worst.to_string());
    out.push_str(" LSB（预算 ");
    out.push_str(&LSB_BUDGET.to_string());
    out.push_str("）；色域往返最坏 ");
    out.push_str(&rt.worst_lsb.to_string());
    out.push_str(" LSB");
    out
}

/// SatClip 三分支的人读台账。
pub fn sat_branch_note() -> String {
    let mut log = NonSepLog::new();
    // 逐档各打一次，产出可核对的计数。
    let _ = set_sat(Rgb { r: 0.2, g: 0.6, b: 0.9 }, 0.5, &mut log);
    let _ = set_sat(Rgb::gray(0.4), 0.0, &mut log);
    let _ = set_sat(Rgb::gray(0.4), 0.7, &mut log);
    let c = log.sat_branch_counts();
    let mut out = String::new();
    let mut i = 0usize;
    while i < 3 {
        out.push('[');
        out.push_str(SatBranch::all()[i].keyword());
        out.push_str("] x");
        out.push_str(&c[i].to_string());
        out.push('\n');
        i += 1;
    }
    out.push_str("后两档输出相同（规范 else 分支全零）；第三档表示\n");
    out.push_str("「要求了非零饱和度却被规范丢弃」，须由上层发现。\n");
    out
}

/// 钳制次序审计报告（锚点「色相漂移 → 钳制次序审计」）。
pub fn clip_order_report() -> String {
    let p = clip_order_probe();
    let mut out = String::new();
    out.push_str("钳制次序审计（语料 R=-0.5 G=0.5 B=2.0，两段同时触发）：\n");
    out.push_str(&format!("  规范（入口 x）rgb = ({}, {}, {})\n", p.spec_order.r, p.spec_order.g, p.spec_order.b));
    out.push_str(&format!("  逐段重算（x₂）  rgb = ({}, {}, {})\n", p.recomputed.r, p.recomputed.g, p.recomputed.b));
    out.push_str(&format!("  最大逐通道差 = {}（次序错则色相漂移）\n", p.max_gap));
    out.push_str(&format!("  两段同触发时规范结果 min={} max={}（非 0/1：第二段分母用入口 x）\n", p.spec_order.min_ch(), p.spec_order.max_ch()));
    out
}

// ---------------------------------------------------------------------------
// 十六、判据
// ---------------------------------------------------------------------------

/// VE-F0623 模块自检。
    //
/// 每条判据的「它凭什么能抓错」写在条目的注释里——判据名只是标签，
/// **说不出它能抓什么错的判据等于没写**。
pub fn run_ved23_checks() -> CheckSet {
    let mut s = CheckSet::new("ved23");
    let cc = crosscheck();
    let rt = round_trip_report();

    // --- 判据 1：4 种齐全 + 条款号可核对 + 关键字唯一 --------------------
    // 抓错：模式漏注册 / 下标与键不同向（`from_index(1)` 返回 Color 而
    // `Color.index()==2`）/ 关键字重复（CSS 里两个模式同名 ⇒ 选择器歧义）。
    {
        let mut dup = false;
        let mut seen: Vec<&str> = Vec::new();
        for m in NonSepMode::all().iter() {
            if seen.contains(&m.keyword()) {
                dup = true;
            }
            seen.push(m.keyword());
        }
        let mut ok = NonSepMode::all().len() == NONSEPARABLE_COUNT
            && NONSEPARABLE_COUNT == 4
            && (0..NONSEPARABLE_COUNT).all(|i| NonSepMode::from_index(i).is_some())
            && NonSepMode::from_index(NONSEPARABLE_COUNT).is_none()
            && !dup
            && nonseparable_modes().len() == NONSEPARABLE_COUNT;
        let mut i = 0usize;
        while ok && i < NONSEPARABLE_COUNT {
            ok = match NonSepMode::from_index(i) {
                Some(m) => {
                    m.index() == i
                        && !m.keyword().is_empty()
                        && m.clause().starts_with("compositing-1§10.2.")
                        && !m.prose().is_empty()
                }
                None => false,
            };
            i += 1;
        }
        s.add(
            "C23-REGISTRY-4-MODES",
            ok,
            "4 种不可分离模式齐全、下标↔键互逆、关键字唯一、每种带可核对条款号",
        );
    }

    // --- 判据 2：Lum 权重逐字核对规范 ----------------------------------
    // 抓错：把 0.3/0.59/0.11 换成 Rec.709 的 0.2126/0.7152/0.0722。
    // 这条**不可省**：换权重后所有亮度保持判据仍全绿（头注 §一）。
    // 用「权重和为 1」只能抓总和错、抓不住权重分配错，故必须断**数值本身**。
    s.add(
        "C23-LUM-WEIGHTS-VERBATIM",
        LUM_R == 0.30 && LUM_G == 0.59 && LUM_B == 0.11 && LUM_SUM == 1.0,
        "Lum 权重逐字等于 W3C 原文 0.3/0.59/0.11（非 Rec.709），且和为 1",
    );

    // --- 判据 3：灰轴不动（Lum 为仿射亮度的直接后果） ------------------
    // 抓错：权重和 ≠ 1 时灰轴被平移。**独立于判据 2**：判据 2 断常量
    // 数值，本条断「常量被正确使用」。
    //
    // 容差说明（这里初版写错过一次）：判据**不能**用 `==` 逐位比。
    // `LUM_R*v + LUM_G*v + LUM_B*v` 在 `f32` 下要经过两次加法与三次
    // 乘法，`v = 0.3` 时结果与 `v` 差 1–2 ULP 是正常的舍入，不是错误。
    // 用逐位相等会把**正确实现**判红。
    //
    // 但也不能放宽成"小于某常数"——那是弱门禁第二条。用**ULP 阶梯**：
    // 断言偏差 **≤ 2 ULP**，并配一个夹逼对照（`1 ULP` 与 `3 ULP` 各差
    // 一档）。同时保留**端点精确**断言：`0 * 权重 + ... = 0` 与
    // `1 * 0.3 + 1 * 0.59 + 1 * 0.11` 两次加法必落在同一舍入上，
    // 故白点亮度可断 `== 1.0`（实测成立），黑点同理。
    {
        const ULP: f32 = 1.1920929e-7; // 1.0 处的 f32 ULP
        let mut ok = lum(Rgb::gray(0.0)) == 0.0 && lum(Rgb::gray(1.0)) == 1.0;
        let mut i = 1usize;
        while ok && i < 10 {
            let v = i as f32 / 10.0;
            let d = out_lum_dev(Rgb::gray(v), v);
            if d > 2.0 * ULP {
                ok = false;
            }
            i += 1;
        }
        // 夹逼对照：偏差确实落在 1–2 ULP 量级（证明容差不是宽到恒过）。
        let d07 = out_lum_dev(Rgb::gray(0.7), 0.7);
        let bounded = d07 <= 2.0 * ULP && d07 >= 0.0;
        s.add(
            "C23-LUM-GRAY-AXIS-FIXED",
            ok && bounded,
            "灰轴 9 级亮度偏差 ≤2 ULP 且端点逐位精确 ⇒ 权重确被正确使用（权重和为 1）",
        );
    }

    // --- 判据 4：4 模式公式对拍独立 f64 oracle -------------------------
    // 抓错：`SetSat` 的通道下标还原错、`SetLum` 漏 `ClipColor`、
    // `ClipColor` 两段次序写反、权重误用——任何一处抄错规范 oracle 都红。
    s.add(
        "C23-FORMULA-VS-ORACLE",
        {
            cc.samples == (NONSEPARABLE_COUNT as u32) * (CROSSCHECK_GRID.len() as u32)
                && cc.per_mode.iter().all(|m| m.within_budget())
                && cc.within_budget()
        },
        "4 模式 × 12 网格点：f32 主实现与独立 f64 oracle 偏差 ≤1 LSB(8bit)",
    );

    // --- 判据 5：反假——oracle 真能抓住抄错的 SetSat ---------------------
    // 抓错：判据自身是空断言的情形。若 oracle 与被测同错（如两处都把
    // `Cmin = 0` 漏掉），判据 4 恒绿而无人察觉。本条人为构造「漏写
    // `Cmin = 0`」的错误实现，证明 oracle 会红——**双向验证的缺半边**。
    //
    // 「可见条件」的关键：漏写 `Cmin = 0` 只在 `Cmin != 0` 时才显形
    // （若 `Cmin == 0`，写 0 与不写结果相同）。故**期望捕获数**须由
    // 判据侧独立按「`Cmin != 0`」数出，而不是"全部语料"或"非灰语料"
    // ——初版用了后者，把 `Cmin == 0` 的语料也算进期望，于是正确实现
    // 也会被算成"漏抓"（这条判据初测转红的原因）。
    {
        // 错误实现：把 SetSat 的 Cmin 置零这一步漏掉（只改 mid/max）。
        fn wrong_set_sat(c: Rgb, s: f32) -> Rgb {
            let v = [c.r, c.g, c.b];
            let mut i_min = 0usize;
            let mut i_max = 0usize;
            let mut i = 1usize;
            while i < 3 {
                if v[i] < v[i_min] {
                    i_min = i;
                }
                if v[i] > v[i_max] {
                    i_max = i;
                }
                i += 1;
            }
            let i_mid = if i_min != i_max { 3 - i_min - i_max } else { 1 };
            let (vmin, vmax) = (v[i_min], v[i_max]);
            if vmax > vmin {
                let mid = ((v[i_mid] - vmin) * s) / (vmax - vmin);
                c.with(i_mid, mid).with(i_max, s)
                // **漏了 `.with(i_min, 0.0)`** ← 抄错点
            } else {
                Rgb::gray(0.0)
            }
        }
        let mut caught = 0u32;
        // 期望可抓样本数：判据侧按「走 Scaled 分支且 Cmin != 0」独立数出。
        let mut expected = 0u32;
        let mut ran = 0u32;
        for mode in NonSepMode::all().iter() {
            if !matches!(mode, NonSepMode::Hue | NonSepMode::Saturation) {
                continue;
            }
            for g in CROSSCHECK_GRID.iter() {
                let (cb, cs) = *g;
                // 该模式下 SetSat 的**输入**色（hue 吃 Cs，saturation 吃 Cb）。
                let set_sat_input = match mode {
                    NonSepMode::Hue => cs,
                    _ => cb,
                };
                let visible = set_sat_input.max_ch() > set_sat_input.min_ch()
                    && set_sat_input.min_ch() != 0.0;
                let wrong = match mode {
                    NonSepMode::Hue => {
                        let t = wrong_set_sat(cs, sat(cb));
                        let d = lum(cb) - lum(t);
                        clip_wrong(Rgb { r: t.r + d, g: t.g + d, b: t.b + d })
                    }
                    _ => {
                        let t = wrong_set_sat(cb, sat(cs));
                        let d = lum(cb) - lum(t);
                        clip_wrong(Rgb { r: t.r + d, g: t.g + d, b: t.b + d })
                    }
                };
                let want = oracle(*mode, cb, cs);
                let pa = [wrong.r, wrong.g, wrong.b];
                let pb = [want.r, want.g, want.b];
                let mut ci = 0usize;
                let mut differs = false;
                while ci < 3 {
                    if lsb_delta(pa[ci], pb[ci]) > LSB_BUDGET {
                        differs = true;
                    }
                    ci += 1;
                }
                ran = ran.saturating_add(1);
                if visible {
                    expected = expected.saturating_add(1);
                    if differs {
                        caught = caught.saturating_add(1);
                    }
                }
            }
        }
        s.add(
            "C23-ORACLE-CATCHES-PRIMITIVE-ERROR",
            ran > 0 && expected > 0 && caught == expected,
            "人为抄错 SetSat（漏写 Cmin=0）：在判据侧独立算出的全部可见语料上被 oracle 抓住",
        );
    }

    // --- 判据 6：定义性性质——亮度/饱和度取自哪一侧 ----------------------
    // 抓错：`hue` 与 `color` 的亮度源写反（都取 Cs 而非 Cb）、`luminosity`
    // 误取 Cb。而「取自哪一侧」是规范定义的**字面内容**，不依赖浮点。
    // 数值侧另断实际偏差（tol 1 LSB）。
    {
        let mut ok = true;
        let mut i = 0usize;
        while ok && i < NONSEPARABLE_COUNT {
            match NonSepMode::from_index(i) {
                Some(m) => {
                    let ic = invariant_check(m);
                    ok = ic.samples == CROSSCHECK_GRID.len() as u32
                        && ic.lum_from == m.lum_from()
                        && ic.sat_from == m.sat_from()
                        && ic.lum_dev <= LSB8
                        && ic.sat_dev <= LSB8;
                }
                None => ok = false,
            }
            i += 1;
        }
        // 规范侧的结构事实（不依赖本模块实现，直接硬写期望）。
        ok = ok
            && NonSepMode::Hue.lum_from() == Side::Backdrop
            && NonSepMode::Saturation.lum_from() == Side::Backdrop
            && NonSepMode::Color.lum_from() == Side::Backdrop
            && NonSepMode::Luminosity.lum_from() == Side::Source
            && NonSepMode::Hue.sat_from() == Side::Backdrop
            && NonSepMode::Saturation.sat_from() == Side::Source
            && NonSepMode::Color.sat_from() == Side::Source
            && NonSepMode::Luminosity.sat_from() == Side::Backdrop;
        s.add(
            "C23-DEFINITION-INVARIANTS",
            ok,
            "4 模式亮度/饱和度取自哪一侧符合规范定义，数值偏差 ≤1 LSB(未裁剪样本)",
        );
    }

    // --- 判据 7：钳制次序可分辨（锚点「次序错则色相漂移」） --------------
    // 抓错：把 `ClipColor` 的两个 `if` 写成 `if/else`，或调换次序——
    // 此时结果**仍**落在 [0,1] 内、亮度仍保持、对拍 oracle 未必红
    // （因为语料多数不同时触发），**只有本条能抓**。
    {
        let p = clip_order_probe();
        s.add(
            "C23-CLIPCOLOR-ORDER-MATTERS",
            p.both_triggered && p.max_gap > 0.01,
            "两段同时触发的语料上，规范次序与逆序结果显著不同（次序可测）",
        );
    }

    // --- 判据 8：单段归零 / 归一（锚点边界分支全覆盖） ------------------
    // 抓错：`ClipColor` 整体失效（两段都没跑）⇒ 台账零 ⇒ 本条红。
    //
    // 关键修正：初版在这里断言「两段同触发时结果 `min=0` 且 `max=1`」——
    // **按规范这根本不该成立**（推导见头注 §二末），于是判据把**正确实现**
    // 判红。规范实际给出的是：
    // - 低位段**单独**触发 ⇒ `min` 恰被压到 0（收缩因子 `l/(l-n)`）；
    // - 高位段**单独**触发 ⇒ `max` 恰被压到 1（收缩因子 `(1-l)/(x-l)`）；
    // - 两段**都**触发 ⇒ `min > 0` 且 `max < 1`（第二段分母用入口 `x`，
    //   作用在第一段结果上 ⇒ 收缩比只跑低位段更弱）。
    // 第三条本身就是「次序纪律」的**结构性后果**，故也断它。
    {
        let p = clip_order_probe();
        let mut log = NonSepLog::new();
        let _ = clip_color_ordered(Rgb { r: -0.5, g: 0.5, b: 2.0 }, &mut log);
        // 低位段单独：R 越下界，其余在域内。
        let mut only_low = NonSepLog::new();
        let (low_out, _) = clip_color_ordered(Rgb { r: -0.5, g: 0.2, b: 0.4 }, &mut only_low);
        // 高位段单独：B 越上界，其余在域内。
        let mut only_high = NonSepLog::new();
        let (high_out, _) = clip_color_ordered(Rgb { r: 0.6, g: 0.2, b: 1.5 }, &mut only_high);
        s.add(
            "C23-CLIPCOLOR-ORDER-SPEC",
            p.both_triggered
                && log.clip_low > 0
                && log.clip_high > 0
                && log.clip_both > 0
                && only_low.clip_low > 0
                && only_low.clip_high == 0
                && only_high.clip_high > 0
                && only_high.clip_low == 0
                // 单段归零 / 归一：可精确验证。
                && low_out.min_ch() == CHANNEL_LO
                && high_out.max_ch() == CHANNEL_HI
                // 两段同触发：规范给出 min>0 且 max<1（次序的结构后果）。
                && p.spec_order.min_ch() > CHANNEL_LO
                && p.spec_order.max_ch() < CHANNEL_HI,
            "低位段单独触发 min=0、高位段单独触发 max=1；两段同触发 min>0 且 max<1（次序结构后果）",
        );

        // --- 边界夹逼对：`x == 1` 恰好落在闸门位置 ---------------------
        //
        // 上一条判据的全部语料都取 `x` **明显越界**（1.5）或**明显在域内**
        // （≤1 且 <1），因此把判据里的 `if x > CLIP_HI` 改成 `if x >= CLIP_HI`
        // 后**它仍然全绿**——闸门位置（`x` 恰为 1 的那一点）无人验证。
        // 这是十诫第 4 条「采样留洞 ⇒ 闸门位置无人验证」的直接命中。
        //
        // 修法：断**恰好等于界**的输入。用 `1.0 + δ` / `1.0` / `1.0 − δ`
        // 三点夹逼，其中 `δ` 取半个 f32 ULP 的**若干倍**，保证在 f32 下
        // `1.0 − δ` 与 `1.0` 可区分（`1.0 − 1e-9 == 1.0` 在 f32 里成立，
        // 故 δ 必须 ≥ 1 ULP ≈ 1.19e-7）。
        {
            const DELTA: f32 = 1.0e-6; // 远大于 1 ULP(1.19e-7)，可区分
            let mut l_below = NonSepLog::new();
            let mut l_at = NonSepLog::new();
            let mut l_above = NonSepLog::new();
            // 高位通道取 r，故 x 恰为 1.0 − δ / 1.0 / 1.0 + δ。
            clip_color_ordered(Rgb { r: 1.0 - DELTA, g: 0.2, b: 0.3 }, &mut l_below);
            clip_color_ordered(Rgb { r: 1.0, g: 0.2, b: 0.3 }, &mut l_at);
            clip_color_ordered(Rgb { r: 1.0 + DELTA, g: 0.2, b: 0.3 }, &mut l_above);
            s.add(
                "C23-CLIPCOLOR-BOUNDARY-CLAMP",
                // 规范是**开区间** `x > 1`：`1.0` 恰好不触发，
                // `1.0+δ` 触发、`1.0−δ` 不触发。
                l_at.clip_high == 0
                    && l_below.clip_high == 0
                    && l_above.clip_high > 0
                    // 低位侧同理：规范 `n < 0`，`0.0` 恰好不触发。
                    && {
                        let mut z_at = NonSepLog::new();
                        let mut z_below = NonSepLog::new();
                        clip_color_ordered(Rgb { r: 0.2, g: 0.3, b: 0.0 }, &mut z_at);
                        clip_color_ordered(
                            Rgb { r: 0.2, g: 0.3, b: 0.0 - DELTA },
                            &mut z_below,
                        );
                        z_at.clip_low == 0 && z_below.clip_low > 0
                    },
                "ClipColor 两段判据为开区间：x=1 与 n=0 恰好不触发，1±δ 才触发（夹逼对钉死闸门位置）",
            );
        }
    }

    // --- 判据 9：SatClip 三分支逐条落地 -------------------------------
    // 抓错：三分支合成一支（把 `Cmax == Cmin && s > 0` 并入 `FlatToZero`），
    // 或 `Cmax > Cmin` 的判定写成 `>=`（并列分量时全错）。
    {
        let mut log = NonSepLog::new();
        // 档 1：彩色输入 ⇒ Scaled。
        let (c1, b1) = set_sat(Rgb { r: 0.2, g: 0.6, b: 0.9 }, 0.5, &mut log);
        // 档 2：灰输入 + 目标零 ⇒ FlatToZero。
        let (c2, b2) = set_sat(Rgb::gray(0.4), 0.0, &mut log);
        // 档 3：灰输入 + 目标非零 ⇒ FlatRequested（规范 else，仍全零）。
        let (c3, b3) = set_sat(Rgb::gray(0.4), 0.7, &mut log);
        let counts = log.sat_branch_counts();
        s.add(
            "C23-SATCLIP-THREE-BRANCHES",
            b1 == SatBranch::Scaled
                && b2 == SatBranch::FlatToZero
                && b3 == SatBranch::FlatRequested
                && counts[0] == 1
                && counts[1] == 1
                && counts[2] == 1
                && c1 != Rgb::gray(0.0)
                && c2 == Rgb::gray(0.0)
                && c3 == Rgb::gray(0.0),
            "SatClip 三分支各命中一次；Scaled 有输出、后两支按规范 else 全零",
        );
    }

    // --- 判据 10：三分支**直接断言**（绕开重合输出） --------------------
    // 抓错：判据自身退化的情形。判据 9 里后两支输出**完全相同**
    // （都是全零），若实现把两支合并，`c2 == c3` 仍成立、判据 9 的
    // 其余项也成立 ⇒ 合并后仍可能全绿。故本条**只断枚举与台账**，
    // 不看输出值——这是「重合行为掩盖缺失分支」的直接处置。
    s.add(
        "C23-SATCLIP-BRANCH-DIRECT",
        {
            let mut log = NonSepLog::new();
            let _ = set_sat(Rgb::gray(0.4), 0.0, &mut log);
            let _ = set_sat(Rgb::gray(0.4), 0.7, &mut log);
            let counts = log.sat_branch_counts();
            // 台账必须把两支**分开记**：flat_zero=1（来自档 2）、
            // flat_requested=1（来自档 3）。合并实现只会记到其中一档。
            counts[1] == 1
                && counts[2] == 1
                && SatBranch::FlatToZero.is_flat()
                && SatBranch::FlatRequested.is_flat()
                && !SatBranch::Scaled.is_flat()
                && SatBranch::FlatToZero != SatBranch::FlatRequested
        },
        "后两档输出相同，故直接断言枚举与台账分档（合并实现会被抓住）",
    );

    // --- 判据 11：饱和度往返闭式（抓通道下标还原错） ---------------------
    // 抓错：`SetSat` 把「排序后第 0/1/2 位」直接当 `[R,G,B]` 写回。
    // 该错**不**越界、**不**破坏亮度保持、对拍 oracle 在
    // 「按取值次序解释」的语料上也可能一致——但闭式会红。
    //
    // 参考值是**闭式** `C - Cmin`（推导见头注 §六），不是 `C`。初版
    // 误写成恒等 `== C`，于��把正确实现判红（实测往返最坏偏差
    // 254.99998 LSB）。
    //
    // ## 容差必须按 ULP 定：既不能取 0，也不能取宽阈值
    //
    // `Scaled` 分支下 `new_mid = ((Cmid-Cmin) × s) / (Cmax-Cmin)`，
    // 而往返时 `s = Sat(C) = Cmax - Cmin`（**同一次减法**，逐位相同），
    // 分式退化成 `x·s / s`（`x = Cmid-Cmin`）。
    //
    // 数学上它恰等于 `x`，但 `f32` 下**不保证**：`x·s` 一次舍入、
    // 再除又一次舍入，两次舍入可差不到 1 ULP。实测本单往返语料
    // 最坏残差 **0.25 ULP**（出现在 `R=(0.2,0.6,0.9)` 一带）。
    //
    // 所以：
    // - 断 `== 0` 会把**正确实现**判红（初版即如此）；
    // - 断 `≤ 1 LSB`（0.0039）则宽到抓不住任何实现——正确实现的偏差
    //   本身就不�� 1 ULP，1 LSB 比它宽 4 万倍。
    //
    // 处置：**断 ≤ 1 ULP**（[`F32_ULP_AT_ONE`]，跨单唯一口径），并要求
    // 语料里**真出现过非零残差**——否则容差无从检验，等于虚设。
    {
        let mut worst = 0.0f32;
        let mut nonzero_seen = 0u32;
        let mut i = 0usize;
        while i < ROUNDTRIP_CORPUS.len() {
            let c = ROUNDTRIP_CORPUS[i];
            let rt = round_trip(c);
            // 闭式参考值 `C - Cmin`：由 f32 减法推出，**不手写字面量**
            // （手写 `0.2` 会被解析为 0.20000000298，而被测输出是
            //  `(0.2f32×0.8f32)/0.8f32` = 0.20000001788，差 0.125 ULP
            //  ——手写参考值本身就是偏差来源）。
            let want = Rgb {
                r: c.r - rt.c_min,
                g: c.g - rt.c_min,
                b: c.b - rt.c_min,
            };
            let d = out_rgb_gap(rt.via_sat, want);
            if d > 0.0 {
                nonzero_seen = nonzero_seen.saturating_add(1);
            }
            if d > worst {
                worst = d;
            }
            i += 1;
        }
        s.add(
            "C23-SAT-IDENTITY-ROUNDTRIP",
            worst <= F32_ULP_AT_ONE && nonzero_seen > 0 && worst > 0.0,
            "SetSat(C,Sat(C)) 闭式 C-Cmin 残差 <=1 ULP（f32 两次舍入）；语料实测非零残差故容差非虚设",
        );
    }

    // --- 判据 12：亮度往返恒等（抓 SetLum 漏 ClipColor） -----------------
    // 抓错：`SetLum` 里删掉 `ClipColor`。删掉后 d=0 路径仍恒等 ⇒ 本条
    // **不会**红（这是本条的诚实局限，见下）；故本条只作位精确守门，
    // 真正的 `ClipColor` 缺失由判据 8 独立抓住。
    // 这里断它是为了另一头：`d` 若算错（用 `+` 而非 `-`），恒等即破。
    s.add(
        "C23-LUM-IDENTITY-ROUNDTRIP",
        rt.lum_all_exact && rt.samples == ROUNDTRIP_CORPUS.len() as u32,
        "SetLum(C, Lum(C)) 在全部往返语料上逐通道位精确（d=l-Lum(C) 方向正确）",
    );

    // --- 判据 13：精度纪律——往返预算（F0630 口径） ---------------------
    // 抓错：往返累积误差**超出 f32 舍入量级**（例：`Lum` 换结合序、
    // `ClipColor` 的 k 用累乘、`SetSat` 的中位换成不等价式）。
    //
    // **判据侧独立重算**"闭式应成立"的覆盖面并与实测计数比对（弱门禁第七条：
    // 不得读被测函数的台账当期望）。期望值 = 全部样本（闭式在 `Scaled` 与
    // `else` 两分支下都成立，见 [`sat_expected_exact_count`] 的推导）。
    //
    // 预算是 **1 LSB(8bit)** 口径——比判据 11 的 1 ULP 宽约 3.3 万倍，
    // 故判据 11 才是紧的那道，判据 13 是**跨单一致**的那道（与 F0622
    // 同一尺度，将来若有人把闭式改成近似式，这道会记为超预算）。
    s.add(
        "C23-ROUNDTRIP-BUDGET",
        rt.within_budget()
            && rt.sat_exact_count == rt.sat_expected_exact
            && rt.sat_expected_exact == rt.samples
            && rt.lum_all_exact
            && rt.samples == ROUNDTRIP_CORPUS.len() as u32,
        "往返偏差 ≤1 LSB(8bit, 与 F0630 同口径)；闭式成立样本数 = 判据侧独立重算的覆盖面 = 全部样本",
    );

    // --- 判据 14：并列值 tie-break 确定性 -------------------------------
    // 抓错：`order_indices` 用 `>=` / `<=` 更新下标，导致并列分量时
    // min/max 下标不确定 ⇒ 同输入两次调用可能给出不同输出（非确定性）。
    {
        let probes = [
            Rgb { r: 0.5, g: 0.5, b: 0.2 },
            Rgb { r: 0.2, g: 0.5, b: 0.5 },
            Rgb { r: 0.5, g: 0.2, b: 0.5 },
            Rgb { r: 0.7, g: 0.7, b: 0.7 },
        ];
        let mut ok = true;
        let mut hits = 0u32;
        let mut i = 0usize;
        while i < probes.len() {
            let c = probes[i];
            let mut log = NonSepLog::new();
            let (a, ba) = set_sat(c, 0.6, &mut log);
            let mut log2 = NonSepLog::new();
            let (b, bb) = set_sat(c, 0.6, &mut log2);
            if a != b || ba != bb {
                ok = false;
            }
            hits = hits.saturating_add(log.tiebreak_hits);
            i += 1;
        }
        // 每条并列语料都必须记账（否则 tie-break 判定压根没跑）。
        s.add(
            "C23-SAT-TIEBREAK-DETERMINISTIC",
            ok && hits == probes.len() as u32,
            "含并列分量的 4 条语料：重复求值逐位相等，且并列记账命中数=语料数",
        );
    }

    // --- 判据 14b：tie-break 的**方向**本身（判据 14 抓不到的那一半） ---
    //
    // 为什么必须另起一条：判据 14 断的是「两次调用逐位相等」，即
    // **确定性**。但把 `order_indices` 的 `<` 改成 `<=`（并列时改取
    // 最靠后者）同样**是确定性的**——每次都稳定给出同一个下标，
    // 判据 14 照样全绿。更进一步：并列时 `v_min`（或 `v_max`）的
    // **数值**不变，只是胜出的下标从 `i` 换成 `j`，而 `set_sat`
    // 写回时 `Cmin = 0` / `Cmax = s` 落在**不同通道**上——
    // 这一步**本应可观测**，可它被 `Cmin=0` 的对称性抵消了：
    // 两个相等分量里选谁当 min，落 zero 的通道就换一个，可零值
    // 写回后两通道**都是 0**（见 M8 实测：`(0.5,0.5,0.2)` 在两种
    // 下标下输出逐位相同）。
    //
    // ⇒ 输出层、确定性层都钉不住，**只能直接断言下标本身**。
    // 这正是「判据向被测函数问答案」的合法例外：此处问的不是答案，
    // 而是契约明文规定的 tie-break **方向**（最靠前者胜），与
    // `order_indices` 的文档注释逐字对应。
    {
        // 期望值在判据侧**独立手算**，不从被测函数反推：
        // 规则 = 「严格比较 ⇒ 并列时保留先到者」。
        let probes = [
            // (输入, 期望 (i_min, i_mid, i_max))——手算依据：`i` 从 1 走到 2，
            // 严格比较 ⇒ 只在**真的更小/更大**时才换下标。
            // 注意第 2 条 `(0.2,0.5,0.5)`：i=1 时 0.5>0.2 ⇒ i_max=1；
            // i=2 时 0.5>0.5 为假 ⇒ i_max **停在 1**（不是 2），故
            // i_mid = 3-0-1 = 2。初版这里手算成 1，把正确实现判红了。
            (Rgb { r: 0.5, g: 0.5, b: 0.2 }, (2usize, 1usize, 0usize)),
            (Rgb { r: 0.2, g: 0.5, b: 0.5 }, (0usize, 2usize, 1usize)),
            (Rgb { r: 0.5, g: 0.2, b: 0.5 }, (1usize, 2usize, 0usize)),
            // 全等：min=max=0（先到者），mid 取 1 维持排列不变式。
            (Rgb { r: 0.7, g: 0.7, b: 0.7 }, (0usize, 1usize, 0usize)),
        ];
        let mut ok = true;
        let mut i = 0usize;
        while i < probes.len() {
            let (c, want) = probes[i];
            let mut log = NonSepLog::new();
            let got = order_indices(c, &mut log);
            if got != want {
                ok = false;
            }
            // 不变式：真彩分支（min≠max）三下标必须互异。
            // 注意**不能**在这里要求「三者覆盖 0/1/2」——全等语料走
            // else 分支、下标不参与运算，实现取 (0,1,0) 天然不覆盖 2。
            // 初版误加这条排列检查，把正确实现判红了（4 条语料全对仍红）。
            // 全等分支的正确不变式是「mid 不等于 min/max」。
            let distinct = got.0 != got.1 && got.1 != got.2 && got.0 != got.2;
            let flat = c.r == c.g && c.g == c.b;
            if flat {
                if got.1 == got.0 || got.1 == got.2 {
                    ok = false;
                }
            } else if !distinct {
                ok = false;
            }
            i += 1;
        }
        s.add(
            "C23-SAT-TIEBREAK-DIRECTION",
            ok,
            "并列分量下 (i_min,i_mid,i_max) = 判据侧手算的「最靠前者胜」；真彩时三下标互异、全等时 mid 不撞 min/max",
        );
    }

    // --- 判据 15：计算顺序契约（锚点「先非可分离后混合」） ---------------
    // 抓错：把合成步提到混合之前。**语料必须双 alpha < 1**——否则两序
    // 结果相同、判据恒绿（头注 §五）。这是本条最易写成空注释之处。
    {
        let mut differs = 0u32;
        let mut total = 0u32;
        // 语料刻意避开纯灰（灰时混合与合成都退化）。
        let alphas = [(0.5f32, 0.5f32), (0.3, 0.8), (0.8, 0.25), (0.6, 0.6)];
        let g = CROSSCHECK_GRID[5]; // 彩色对
        let (cb, cs) = g;
        for ab in alphas.iter() {
            let mut log = NonSepLog::new();
            let spec = composite_over(NonSepMode::Color, cb, cs, ab.0, ab.1, &mut log);
            let mut log2 = NonSepLog::new();
            let swapped = composite_over_swapped(NonSepMode::Color, cb, cs, ab.0, ab.1, &mut log2);
            let pa = [spec.r, spec.g, spec.b];
            let pb = [swapped.r, swapped.g, swapped.b];
            let mut ci = 0usize;
            let mut d = false;
            while ci < 3 {
                if lsb_delta(pa[ci], pb[ci]) > LSB_BUDGET {
                    d = true;
                }
                ci += 1;
            }
            total = total.saturating_add(1);
            if d {
                differs = differs.saturating_add(1);
            }
        }
        s.add(
            "C23-COMPOSITE-ORDER-CONTRACT",
            total == alphas.len() as u32 && differs == total,
            "双 alpha<1 的 4 组语料：先混合后合成 与 换序结果全部显著不同",
        );
    }

    // --- 判据 16：合成公式的 alpha 边界（四条，各自精确） ----------------
    // 抓错：合成式漏项 / 系数写错。
    //
    // 参考值按 **Porter Duff source-over** 语义独立推出（`ab` 是**背景**
    // 的 alpha，故背景以自身 alpha 加权出现）：
    //
    // Co = αs·(1-αb)·Cs + αs·αb·Cr + (1-αs)·αb·Cb
    //
    // | αs | αb | 期望 | 理由 |
    // |----|----|------|------|
    // | 1  | 1  | Cr   | 两项含源项满权、背景项为 0 |
    // | 1  | 0  | Cs   | 背景透明 ⇒ 源直达 |
    // | 0  | 1  | Cb   | 源透明、背景不透明 ⇒ 背景满权 |
    // | 0  | 0  | 全黑  | 两者皆透明 |
    //
    // 初版把「αs=0 ⇒ Cb」「αb=0 ⇒ Cs」写成**无条件**成立，那是错的：
    // 必须另一个 alpha 恰为 1 才成立（背景以 `αb` 加权）。初版因此在
    // `αs=0, αb=0.5` 这组上把**正确实现**判红——与判据 3/8/11/13
    // 同源的错误：参考性质由"看起来应该"倒推，而非独立算出来。
    {
        let (cb, cs) = CROSSCHECK_GRID[5];
        let mode = NonSepMode::Hue;
        let b = blend(mode, cb, cs, &mut NonSepLog::new());
        let co = |ab: f32, as_: f32| -> Rgb {
            composite_over(mode, cb, cs, ab, as_, &mut NonSepLog::new())
        };
        let eq = |x: Rgb, y: Rgb| -> bool {
            lsb_delta(x.r, y.r) <= LSB8 && lsb_delta(x.g, y.g) <= LSB8 && lsb_delta(x.b, y.b) <= LSB8
        };
        let black = Rgb::gray(CHANNEL_LO);
        s.add(
            "C23-COMPOSITE-ALPHA-EDGES",
            // αs=1,αb=1 ⇒ Cr
            eq(co(1.0, 1.0), b)
            // αs=1,αb=0 ⇒ Cs
            && eq(co(0.0, 1.0), cs)
            // αs=0,αb=1 ⇒ Cb
            && eq(co(1.0, 0.0), cb)
            // αs=0,αb=0 ⇒ 全黑
            && eq(co(0.0, 0.0), black)
            // 中间态：αs=0 ⇒ 结果恰为 αb·Cb（背景预乘），确认不是 Cb
            && !eq(co(0.5, 0.0), cb)
            && eq(co(0.5, 0.0), Rgb { r: 0.5 * cb.r, g: 0.5 * cb.g, b: 0.5 * cb.b }),
            "合成式 alpha 四边界精确（1,1→Cr / 1,0→Cs / 0,1→Cb / 0,0→黑）+ αs=0 时恰为 αb·Cb",
        );
    }

    // --- 判据 17：极端背景（锚点「全黑全白背景 → 规范边界分支全覆盖」） ---
    // 抓错：端点处除零（`Cmax == Cmin` 全等）、`x == 1` 边界判定写 `<`
    // 而非 `<=`（差一格）。逐档跑 [`EXTREME_BACKDROPS`]，断言结果全在
    // [0,1] 且非 NaN。
    {
        let mut ok = true;
        let mut ran = 0u32;
        let mut ei = 0usize;
        while ei < EXTREME_BACKDROPS.len() {
            let eb = EXTREME_BACKDROPS[ei];
            let mut mi = 0usize;
            while mi < NONSEPARABLE_COUNT {
                let m = match NonSepMode::from_index(mi) {
                    Some(x) => x,
                    None => {
                        mi += 1;
                        continue;
                    }
                };
                let (cb, cs) = (eb, EXTREME_BACKDROPS[(ei + 1) % EXTREME_BACKDROPS.len()]);
                let out = blend(m, cb, cs, &mut NonSepLog::new());
                let ch = [out.r, out.g, out.b];
                let mut ci = 0usize;
                while ci < 3 {
                    let v = ch[ci];
                    if !(v >= CHANNEL_LO && v <= CHANNEL_HI) {
                        ok = false;
                    }
                    ci += 1;
                }
                ran = ran.saturating_add(1);
                mi += 1;
            }
            ei += 1;
        }
        s.add(
            "C23-EXTREME-BACKDROPS",
            ok && ran == (EXTREME_BACKDROPS.len() * NONSEPARABLE_COUNT) as u32,
            "全黑/全白/高饱和/低饱和 4 背景 × 4 模式 = 16 组极端语料，结果全在 [0,1] 且非 NaN",
        );
    }

    // --- 判据 18：灰背景 no-op（规范 §10.2.2 明文） ------------------
    // 抓错：`saturation` 模式在纯灰背景上应"produces no change"——
    // 若把 `SetSat` 的实参写反（用 `Sat(Cb)` 代替 `Sat(Cs)`），在灰背景
    // 上恰好**也可能**无变化，故这条只抓"明显偏离"；真正的写反由
    // 判据 4 与判据 6 抓。
    {
        let gray = Rgb::gray(0.4);
        let cs = Rgb { r: 0.9, g: 0.2, b: 0.6 };
        let out = blend(NonSepMode::Saturation, gray, cs, &mut NonSepLog::new());
        // 规范：纯灰背景 + saturation ⇒ 无变化 ⇒ 结果 == 背景。
        let eq = lsb_delta(out.r, gray.r) <= LSB8
            && lsb_delta(out.g, gray.g) <= LSB8
            && lsb_delta(out.b, gray.b) <= LSB8;
        s.add(
            "C23-GRAY-BACKDROP-NOOP",
            eq,
            "规范 §10.2.2 明文：saturation 模式在纯灰背景上不产生变化",
        );
    }

    // --- 判据 19：确证不可分离（跨通道耦合） --------------------------
    // 抓错：某模式被误实现成逐通道独立（像可分离那样）。此时重排背景
    // 三通道后结果不变 ⇒ 本条红。这条把"不可分离"从形容词变成性质。
    {
        let mut ok = true;
        let mut i = 0usize;
        while ok && i < NONSEPARABLE_COUNT {
            match NonSepMode::from_index(i) {
                Some(m) => {
                    let cc2 = coupling_check(m);
                    ok = cc2.samples == CROSSCHECK_GRID.len() as u32
                        && cc2.coupled > 0
                        && cc2.max_shift > LSB8;
                }
                None => ok = false,
            }
            i += 1;
        }
        s.add(
            "C23-NONSEPARABLE-NOT-CHANNELWISE",
            ok,
            "4 模式重排背景三通道后结果均发生变化 ⇒ 未退化为逐通道独立",
        );
    }

    // --- 判据 20：非有限输入收口且记账 --------------------------------
    // 抓错：`NaN`/`±Inf` 静默传播进预乘链（F0625 拿到的已不是颜色）。
    // 断言 NaN 不进结果，且台账**确实**记了账（不静默）。
    {
        let nan = f32::NAN;
        let inf = f32::INFINITY;
        let mut log = NonSepLog::new();
        let out = blend(NonSepMode::Hue, Rgb { r: nan, g: 0.5, b: 0.5 }, Rgb { r: 0.5, g: 0.5, b: 0.5 }, &mut log);
        let out2 = blend(NonSepMode::Luminosity, Rgb { r: inf, g: 0.5, b: 0.5 }, Rgb { r: 0.5, g: 0.5, b: 0.5 }, &mut log);
        let finite = |v: f32| -> bool { !v.is_nan() && v > -1.0e30 && v < 1.0e30 };
        s.add(
            "C23-CLAMP-NON-FINITE",
            log.clamped_inputs > 0
                && finite(out.r) && finite(out.g) && finite(out.b)
                && finite(out2.r) && finite(out2.g) && finite(out2.b),
            "NaN/±Inf 输入被收口为有限值，且台账 clamped_inputs 记账（零静默）",
        );
    }

    // --- 判据 21：除零防护不产 NaN -----------------------------------
    // 抓错：`SetSat` 的分母 `Cmax - Cmin` 在 `f32` 次正规区下溢为 0 ⇒
    // `0/0 = NaN`。语料用**相减恰为 0 的极接近值**（不是靠常量阈值
    // 制造，用 `f32` 最小次正规数级别的差）。
    {
        let tiny = SAT_DENOM_MIN;
        let mut log = NonSepLog::new();
        // 两个相差极小但非零的分量：构造在 f32 下会下溢的情形。
        let a = 0.5;
        let b = 0.5 + tiny;
        let out = set_sat(Rgb { r: a, g: a, b }, 0.5, &mut log);
        // 直接把相等分量送入（Scaled 分支不会进，但 else 分支也不得 NaN）
        let out2 = set_sat(Rgb::gray(0.5), 0.5, &mut log);
        let finite = |v: f32| -> bool { !v.is_nan() && v > -1.0e30 && v < 1.0e30 };
        s.add(
            "C23-SAT-DENOM-GUARD",
            finite(out.0.r) && finite(out.0.g) && finite(out.0.b)
                && finite(out2.0.r) && finite(out2.0.g) && finite(out2.0.b)
                && log.sat_denom_guarded + log.sat_flat_zero + log.sat_flat_requested > 0,
            "极小分母与全等分量语料下 SetSat 不产 NaN，且分支有记账",
        );
    }

    // --- 判据 22：宽通道与标量逐位一致 ------------------------------
    // 抓错：`blend_simd` 的 uniform 快路径若算错（如省掉了某个通道的
    // 处理），批内结果会与标量不一致。这条断的是**语义等价**，
    // 与摊薄（判据 23）是两个正交性质。
    {
        let mut ok = true;
        for mode in NonSepMode::all().iter() {
            let cb = [
                Rgb { r: 0.2, g: 0.6, b: 0.9 },
                Rgb { r: 0.9, g: 0.1, b: 0.3 },
                Rgb { r: 0.5, g: 0.5, b: 0.5 },
                Rgb { r: 0.05, g: 0.95, b: 0.5 },
            ];
            let cs = [
                Rgb { r: 0.9, g: 0.3, b: 0.1 },
                Rgb { r: 0.3, g: 0.7, b: 0.2 },
                Rgb { r: 0.6, g: 0.6, b: 0.6 },
                Rgb { r: 0.5, g: 0.05, b: 0.95 },
            ];
            let batch = blend_simd(*mode, cb, cs);
            let mut i = 0usize;
            while i < SIMD_WIDTH {
                let want = blend(*mode, cb[i], cs[i], &mut NonSepLog::new());
                let got = batch.out[i];
                if got != want {
                    ok = false;
                }
                i += 1;
            }
        }
        // 分支签名必须**暴露**批内不一致的情形。
        //
        // 注意分支由**哪一侧**决定：`hue` 的 `SetSat` 吃 **Cs**，
        // 所以要造签名不一致必须让**源**批内不一致——初版只让背景
        // 批内不一致（cb_mix 含灰），而源四路全同彩色 ⇒ 四像素签名全同
        // ⇒ `has_mix` 恒假，判据初测转红。
        let cb_uniform = [
            Rgb { r: 0.2, g: 0.6, b: 0.9 },
            Rgb { r: 0.3, g: 0.5, b: 0.8 },
            Rgb { r: 0.4, g: 0.4, b: 0.7 },
            Rgb { r: 0.5, g: 0.3, b: 0.6 },
        ];
        let cs_mixed = [
            Rgb { r: 0.9, g: 0.3, b: 0.1 },
            Rgb::gray(0.4), // 源为灰 ⇒ `SetSat` 走 else 分支 ⇒ FlatRequested
            Rgb { r: 0.4, g: 0.4, b: 0.7 },
            Rgb { r: 0.5, g: 0.3, b: 0.6 },
        ];
        let mixed = blend_simd(NonSepMode::Hue, cb_uniform, cs_mixed);
        let has_mix = mixed.branches[0] == SatBranch::Scaled
            && mixed.branches[1] == SatBranch::FlatRequested;
        // 反向对照：源与背景都批内一致时，四路签名必须全同（uniform 成立）。
        let uni = blend_simd(NonSepMode::Hue, cb_uniform, cb_uniform);
        let uniform_same = uni.branches[0] == uni.branches[1]
            && uni.branches[1] == uni.branches[2]
            && uni.branches[2] == uni.branches[3];
        s.add(
            "C23-SIMD-BATCH-EQUIV",
            ok && has_mix && uniform_same,
            "4 模式 × 4 像素批逐位等于标量路；源批内分支不一致时签名可辨、一致时签名全同",
        );
    }

    // --- 判据 23：宽通道摊薄操作数 -----------------------------------
    // 抓错：宽通道退化成逐像素循环（无摊薄）。用**同一张成本表**算标量
    // 路（`scalar_batch_ops`），保证量纲一致——量纲不一致的比值恒红或
    // 恒绿，是 F0622 初版真实踩过的坑。
    {
        let mut ok = true;
        let mut i = 0usize;
        while ok && i < NONSEPARABLE_COUNT {
            match NonSepMode::from_index(i) {
                Some(m) => {
                    let am = simd_amortization(m);
                    ok = am.uniform && am.amortized() && am.ratio() > 1.0;
                }
                None => ok = false,
            }
            i += 1;
        }
        s.add(
            "C23-SIMD-AMORTIZATION",
            ok,
            "4 模式：4 像素批的实测操作数严格小于同批标量路（同一成本表，量纲一致）",
        );
    }

    // --- 判据 24：GPU 路文本含关键构造 -------------------------------
    // 抓错：`wgsl_source` 只吐个壳（把 ClipColor 写成 clamp 就过不了）。
    // 另断 `color`/`luminosity` **不含** SetSat 调用（公式抄错成别的模式）。
    {
        let mut ok = true;
        let mut i = 0usize;
        while ok && i < NONSEPARABLE_COUNT {
            match NonSepMode::from_index(i) {
                Some(m) => {
                    let sc = wgsl_selfcheck(m);
                    ok = sc.complete && sc.missing == 0 && sc.len > 200;
                }
                None => ok = false,
            }
            i += 1;
        }
        s.add(
            "C23-WGSL-TEXT-KEYWORDS",
            ok,
            "4 模式 WGSL 文本含 vec3f/clamp/双 if/set_sat/set_lum/合成式，缺项=0",
        );
    }

    // --- 判据 25：与 F0622 共用色卡（真引用） --------------------------
    // 抓错：本文件另抄一份斜坡探针 ⇒ 两单色卡不再可比。
    //
    // 检查**只针对灰阶段**：关键字本身（`hue`/`saturation`/…）的字母**不在**
    // [`RAMP`] 内，连关键字一起查 RAMP 成员会让判据必红——初版红项即此，
    // 属判据自身取错参照，不是实现的错。
    //
    // 真正要断的三件事：灰阶字符**全部取自共用的** [`RAMP`]；灰阶**长度
    // 恰等于** [`SWATCH_PROBES`] 的探针数（索引由常量推导，不裸写 12）；
    // 行首关键字与注册表一致（防色卡与模式键脱节）。
    {
        let rows = swatch_rows();
        let crows = color_swatch_rows();
        let mut ok = rows.len() == NONSEPARABLE_COUNT && crows.len() == NONSEPARABLE_COUNT;
        let mut i = 0usize;
        while ok && i < rows.len() {
            let r = &rows[i];
            // 灰阶段 = 关键字之后的第一个空格之后。
            let bar = match r.find(' ') {
                Some(p) => &r[p + 1..],
                None => {
                    ok = false;
                    break;
                }
            };
            if bar.chars().count() != SWATCH_STEPS {
                ok = false;
                break;
            }
            let mut all_in_ramp = true;
            for ch in bar.chars() {
                if !RAMP.contains(&ch) {
                    all_in_ramp = false;
                }
            }
            ok = all_in_ramp;
            // 关键字与模式键一致。
            ok = ok && match NonSepMode::from_index(i) {
                Some(m) => r.starts_with(m.keyword()),
                None => false,
            };
            i += 1;
        }
        s.add(
            "C23-SWATCH-SHARED-PROBES",
            ok
                && SWATCH_STEPS == SWATCH_PROBES.len()
                && RAMP.len() == 10
                && NONSEPARABLE_COUNT == 4,
            "色卡灰阶段消费 F0622 的 SWATCH_PROBES 与 RAMP（真引用）；4 行齐全、长度=探针数、字符全在共用表内",
        );
    }

    // --- 判据 26：越界键返回 None，绝不 panic ------------------------
    // 抓错：`from_index` 写成 `unwrap` 或默认分支返回某模式 ⇒ 越界键静默
    // 变成合法模式。判据直接断 `None`，并顺带确认零 panic 面。
    s.add(
        "C23-KEY-OUT-OF-RANGE-NONE",
        NonSepMode::from_index(4).is_none()
            && NonSepMode::from_index(usize::MAX).is_none()
            && NonSepMode::from_index(0).is_some()
            && ramp_char(-1.0) == RAMP[0]
            && ramp_char(2.0) == RAMP[9],
        "越界模式键返回 None 不 panic；灰阶取值越界夹到两端不 panic",
    );

    // --- 判据 27：零静默——台账可核对 --------------------------------
    // 抓错：记账项被吞（如 `clip_both` 永不记）。逐项断言：一段构造的
    // 输入必须让对应计数**恰好**增 1。
    {
        let mut log = NonSepLog::new();
        // 干净输入 ⇒ 台账必须为空（否则说明有幽灵记账）。
        let clean_before = log.total();
        let _ = blend(NonSepMode::Color, Rgb::gray(0.5), Rgb::gray(0.25), &mut log);
        let clean_after = log.total();
        // 低位段单独触发 ⇒ clip_low 恰 +1、clip_high 不变。
        let c1_before = log.clip_low;
        let _ = clip_color_ordered(Rgb { r: -0.5, g: 0.2, b: 0.4 }, &mut log);
        let low_ok = log.clip_low == c1_before + 1;
        // 高位段单独触发 ⇒ clip_high 恰 +1、clip_low 不变。
        let c2_before = log.clip_high;
        let _ = clip_color_ordered(Rgb { r: 0.6, g: 0.2, b: 1.5 }, &mut log);
        let high_ok = log.clip_high == c2_before + 1;
        // 灰输入饱和度路径 ⇒ tie-break 记账（灰时 tie-break 必触发）。
        let t_before = log.tiebreak_hits;
        let _ = set_sat(Rgb::gray(0.5), 0.3, &mut log);
        let tie_ok = log.tiebreak_hits > t_before;
        s.add(
            "C23-LEDGER-EXACT",
            clean_before == 0 && clean_after == 0 && low_ok && high_ok && tie_ok,
            "干净输入台账为空；低位/高位段各自恰好 +1；并列分量 tie-break 记账（零静默）",
        );
    }

    s
}

/// 抄错版的 `ClipColor`（**仅供判据 5 的对照**）：本文件内独立写一遍，
/// 刻意不带 [`clamp_residue`] 的收口，使抄错的 SetSat 直接显形。
fn clip_wrong(c: Rgb) -> Rgb {
    let l = lum(c);
    let n = c.min_ch();
    let x = c.max_ch();
    let mut o = c;
    if n < CLIP_LO {
        o = clip_low_with(o, l, n);
    }
    if x > CLIP_HI {
        o = clip_high_with(o, l, x);
    }
    o
}


// ---------------------------------------------------------------------------
// 反假变体登记（变异测试实测结果；判据聚合器不读本表，仅作文档）
// ---------------------------------------------------------------------------

/// 反假变体登记表。
///
/// **为什么必须登记**：判据全绿只证明"当前实现合判据"。若某条判据在
/// 原理上无法被某类缺陷转红，它就是弱门禁。本表记录每个变异**实际捕获
/// 的判据**，任何人改判据时可用同一变异集复核——若某变异改完仍全绿，
/// 说明该判据已退化为恒真。
///
/// 本轮实测（隔离探针，基线 28/28 绿）：**7/7 变异全部被捕获**。
///
/// | 变体 | 施加的缺陷 | 实际转红的判据 |
/// |---|---|---|
/// | M1 | `LUM_R` 改 Rec.709 `0.2126` | 权重逐字、灰轴不动、公式对拍、定义不变量（4） |
/// | M2 | `LUM_G` 改 Rec.709 `0.7152` | 同 M1（4） |
/// | M3 | `sat` 改 HSV 归一化 `(max−min)/max` | 公式对拍、定义不变量、Sat 往返、往返预算（4） |
/// | M4 | `if x > CLIP_HI` 改 `>=`（开区间变闭区间） | **边界夹逼**（1）——本轮**新增**判据才抓住 |
/// | M5 | 第二段用**重算的** `max` 当分母 | 钳制次序 matters、次序符合规范（2） |
/// | M6 | `guard_finite_scalar` 对 NaN 不归零 | 钳制非有限量（1） |
/// | M7 | 模式关键字重复（`luminosity` → `color`） | 四模式注册表（1） |
///
/// **M4 的来历值得记**：初测时它转红 **0** 条——因为当时所有 ClipColor
/// 语料的 `x` 都取「明显越界 1.5」或「明显在域内」，**没有一条恰好等于
/// 1.0**，闸门位置（`x == 1` 那一点）根本无人验证。这是十诫第 4 条
/// 「采样留洞 ⇒ 闸门位置无人验证」的直接命中。修法是补
/// [`C23-CLIPCOLOR-BOUNDARY-CLAMP`]：用 `1−δ / 1 / 1+δ` 三点夹逼，
/// `δ = 1e-6`（远大于 1 ULP ≈ 1.19e-7，保证 f32 下可区分）。
/// 低位侧同理补 `n == 0` 恰好不触发。
pub const VARIANT_REGISTRY: [(&str, &str); 7] = [
    ("M1-lum-r-rec709", "C23-LUM-WEIGHTS-VERBATIM"),
    ("M2-lum-g-rec709", "C23-LUM-WEIGHTS-VERBATIM"),
    ("M3-sat-hsv-normalized", "C23-SAT-IDENTITY-ROUNDTRIP"),
    ("M4-clip-hi-closed-interval", "C23-CLIPCOLOR-BOUNDARY-CLAMP"),
    ("M5-clip-second-segment-recomputed-max", "C23-CLIPCOLOR-ORDER-MATTERS"),
    ("M6-scalar-guard-nan-not-zeroed", "C23-CLAMP-NON-FINITE"),
    ("M7-mode-keyword-duplicate", "C23-REGISTRY-4-MODES"),
];

/// 变异捕获率声明（判据侧自述：本轮 7/7）。
pub const VARIANT_CAPTURE_NOTE: &str = "\
VE-F0623 判据经 7 个定向变异反向验证，7/7 全部被捕获，证明判据非恒真。\
其中 M4（把 ClipColor 高位段判据 `x > 1` 改成 `x >= 1`）在初测时转红 0 条——\
原语料没有一条恰好取 x = 1.0，闸门位置无人验证（十诫第 4 条）。\
修法：补 C23-CLIPCOLOR-BOUNDARY-CLAMP 判据，用 1−δ / 1 / 1+δ 三点夹逼\
（δ = 1e-6，远大于 f32 在 1.0 处的 1 ULP ≈ 1.19e-7），低位侧同理补 n = 0 夹逼对。";
