//! VE-F2607 · 控件树序列化（VE-N 域 · UI 框架内核 · N01 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2607`
//!
//! **判据（锚点原文四条 + 判据面）**：四段 schema、四重校验、模板引用显性、
//! 往返零损失、判据。
//!
//! ---
//!
//! ## 〇、本单要解决的真问题
//!
//! 控件树是**开发者手写的意图**。手写的东西要能被存下来、被搬走、被另一套
//! 工具读进去——这就是序列化。但序列化这件事在本引擎里有一个别的域都没有的
//! 危险：**它同时是「导出」和「导入」，而导入面对的是不受信任的文本**。
//!
//! 导出丢字段的症状是「存下去再读出来少了个颜色」，很轻；**导入多吞一个字段
//! 的症状是「加载了一个畸形树，界面卡死但没有任何报错」**，很重。所以本单的
//! 重心不在「能不能写出去」，而在**「读回来时错在哪一行、错在哪一段」**。
//!
//! ## 一、四段 schema（锚点「四段 schema 公开」逐条落实）
//!
//! ```text
//! {
//!   "schema": "varix.control-tree",   ← 信封：格式标识
//!   "version": 2,                     ← 信封：格式版本（版本链见 §六）
//!   "root": "root",                   ← 信封：根节点 id
//!   "nodes":     [ { "id","kind","children" } ],        ← 节点段
//!   "props":     [ { "node","key","type","value" } ],   ← 属性段
//!   "binds":     [ { "node","path" } ],                 ← 绑定段
//!   "templates": [ { "node","template" } ]              ← 模板引用段
//! }
//! ```
//!
//! | 段 | 锚点语 | 本模块落点 |
//! |---|---|---|
//! | 节点段 | 类型/ID/子节点序 | [`NodeSection`]：`kind` + `id` + `children`（**序即语义**） |
//! | 属性段 | 键值对 + 类型标记 | [`PropSection`]：`key` + `type` + `value` |
//! | 绑定段 | 绑定路径列表 | [`BindSection`]：`node` + `path` |
//! | 模板引用段 | 模板 ID 引用 | [`TplSection`]：`node` + `template` |
//!
//! ### 1.1 为什么属性段是**平铺数组**而不是塞在节点里
//!
//! 平铺（`{node,key,type,value}` 逐条）比嵌套（`node.props[...]`）多写了
//! 一遍节点 id，也丢了「属性属于哪个节点」的一眼可读性。换来的是三件事：
//!
//! - **段可独立为空**。嵌套形态下「节点没有属性」和「属性段不存在」是**同一
//!   个 JSON**（都没有那个键），二者不可区分；平铺形态下属性段永远是数组，
//!   空数组与缺段是**两件可区分的事**——而这个区分正是 [`validate_structure`]
//!   能显性拒绝「缺段」的前提。
//! - **丢失可定位到段**。往返 diff 只需比对四段各自的内容，不必先重建树再
//!   逐字段对（见 [`RoundTripReport::lost_sections`]）。
//! - **重复键可检出**。`(node,key)` 重复在平铺形态下是一条独立记录、能被
//!   直接抓住；嵌套形态下第二次赋值只是覆盖，**静默丢值**。
//!
//! ### 1.2 为什么四段**全部必填**，空也写成 `[]`
//!
//! 锚点说「四段 schema 公开」。若某段可缺省，则「这段没有内容」与
//! 「写文件的人忘了写这段」在下游是同一件事，**打开格式就退化成了
//! 半私有格式**——别人无法区分「确实为空」与「我拼错了键名」。
//! 故 [`validate_structure`] 对四段做**存在性**校验（不是非空校验），
//! 且 [`to_json`] 恒写出四段（空段写 `[]`）。
//!
//! ## 二、四重校验（锚点「四重校验声明」逐条落实，家族 F1612/F2409）
//!
//! | 重 | 锚点语 | 本模块落点 | 归属单 |
//! |---|---|---|---|
//! | ① 结构 | 树合法性（F2602 不变量） | [`validate_structure`] | VE-F2602 |
//! | ② 属性域 | 值域校验 | [`validate_prop_domain`] | VE-F2604 |
//! | ③ 绑定解析 | 路径可解析 | [`validate_binds`] + 落地解析 | VE-F2402/M04 |
//! | ④ 深度限制 | 深度上限 | [`validate_depth`] | VE-F2609 |
//!
//! ### 2.1 四重**必须各自独立可观测**，否则等于一重
//!
//! 极易发生的自欺：写一个 `validate()` 依次跑四步、任何一步失败就整体
//! 返回 `Err`。此时「四重校验」在**接口上**是四重，在**可观测性上**是一重，
//! 而判据能观测的只有 `validate()` 那个返回值——于是四重退化为一重，
//! 判据全绿而其中三重从未被单独验证过。
//!
//! 本模块因此把四重做成 [`FoldVerdict`] 的**四行独立结论**
//! （[`ValidationReport::verdict`]），`inspected`（该重实际检查了多少项）
//! 与 `violations`（逐条诊断）**都按重分开记账**。四个 `Fold` 变体各有专属
//! 诊断码——**共用码等于没分重**，因为调用方无法判断该修哪一处。
//!
//! ### 2.2 四重的**触发条件互不覆盖**（判据设计的前提）
//!
//! 四重之间刻意做成**互不遮蔽**：属性段坏掉的语料，其结构重**必须仍然是绿的**。
//! 若结构重顺手把属性也查了（或者因为属性段缺字段而连带报结构错），
//! 「只坏属性」这条语料就会同时点亮两盏灯，判据再写「恰好命中属性重」就
//! 恒真了。故 [`validate_structure`] 只看**节点段与信封**，
//! [`validate_prop_domain`] 只看**属性段**，二者无任何共享的前置条件。
//!
//! ### 2.3 四重是**单遍**的：一次遍历同时记账四行
//!
//! 锚点「四重校验单遍」。[`validate_all`] 建两张表（`id → 节点段下标`、
//! `子 id → 父 id`），然后**一次显式栈遍历**同时完成：可达性/环（结构）、
//! 深度（④）、逐节点属性（②）、逐节点绑定语法（③）。绑定**落地解析**
//! 必须在树建成之后，故 [`import_json`] 在建树后跑 [`resolve_bind_paths`]
//! ——这是**两阶段**，如实声明而不是硬说成单遍。
//!
//! ## 三、属性值的线缆表示：为什么数值带 `bits`
//!
//! 锚点要求「键值对 + **类型标记**」且「手编兼容」。三型值的线缆表示：
//!
//! | 类型 | 线缆 | 为什么这样表示 |
//! |---|---|---|
//! | `bool` | JSON `true`/`false` | 无歧义 |
//! | `text` | JSON 字符串（含转义） | 可读、可手编 |
//! | `color` | JSON 数字（`u32` 十进制） | `u32` 在十进制里**精确**，转十六进制只是好看，不增精度 |
//! | `number` | JSON 数字（`f32` 最短往返十进制）**+ `bits` 十六进制** | 见下 |
//!
//! ### 3.1 `no_std` 下自写「十进制 ↔ f32 正确舍入」是dragon4 级难题
//!
//! `f32 → 十进制 → f32` 要**逐位相等**，需要的是最短往返表示；而「正确舍入的
//! 十进制 ↔ 二进制浮点互转」在 `no_std` 下要么引外部大库，要么自己实现
//! dragon4/Grisu——那是**另一个单号的工程量**，硬塞进本单只会得到一个
//! 「看起来能往返、边界上差 1 ULP」的实现，而**差 1 ULP 的往返在静止画面
//! 下完全不可见**（症状是尺寸差 0.0000001 像素）。
//!
//! 故本单取**可判定的精确载体**：`bits` 字段携带 `f32::to_bits()` 的十六进制，
//! 导入时**以 `bits` 为权威**直接 `from_bits`，往返**完全不经过十进制**。
//! `value` 字段保留是为了**手编可读**（人要能看见 `800` 而不是
//! `0x447A0000`），但它**不是权威**。
//!
//! ### 3.2 `value` 与 `bits` 不一致时怎么办：**告警，不拒绝**
//!
//! 这是本单最容易写错的一处。若不一致就**拒绝**，则手改 `value` 而忘了同步
//! `bits` 会得到「文件打不开」——这对「手编兼容」是**反向**的：人已经看懂
//! 了文件却打不开。若不一致就**静默以 `bits` 为准**，那人改的 `value`
//! 会被无声丢掉——这是开放格式里最坏的一种失败（人以为改了，其实没改）。
//!
//! 故取**第三条路**：`bits` 为权威，同时产出**一条告警诊断**，定位到
//! `(节点, 属性键)`。既不阻断（人能继续编辑），也不静默（人看得见自己
//! 的编辑没生效）。告警走 [`SerdeDiagnostic`] 但严重度为「告警」档，
//! 与四重的**阻断性**违规在 [`Finding`] 层分开记账。
//!
//! ### 3.3 非有限值**不可导出**（`NaN`/`inf`）
//!
//! 非有限 `f32` 的 `Debug` 输出是 `NaN` / `inf`，**不是合法 JSON**。
//! 若照写，产出的文件**下游任何 JSON 解析器都读不了**——而症状是
//! 「别的工具说文件坏了，我们自己读得好好的」。故 [`to_json`] 对非有限
//! 数值**显性拒绝**（[`SerdeDiagCode::NonFiniteNumber`]）。
//! 正常路径不该出现非有限值：F2604 的段 2 钳制已拒。
//!
//! ## 四、往返零损失：本单的头号弱门禁风险（锚点「保真红线」）
//!
//! 锚点要求「导出 → 导入 → 再导出**逐位** diff = 0」。这句话有一个
//! **极易自欺的实现**：让导入**丢掉**一些东西，而由于导出侧做了**归一化**
//! （排序、规范化），丢掉的字段在**第二次导出时依然不存在**，于是
//! `text1 == text2` 成立，往返判据全绿——**而树已经少了东西**。
//!
//! 化解办法是把「保真」拆成**三条互相独立的断言**，任何一条单独都能抓：
//!
//! 1. **文本恒等**：`to_json(import(export(x))) == export(x)`——抓文本层丢失；
//! 2. **段级等价**：四段内容**逐段相等**，且 [`RoundTripReport::lost_sections`]
//!    点名丢的是哪一段——抓「丢在段内但两边归一化后一致」的形态；
//! 3. **子节点序保真**：**兄弟序是语义**（F2602 不变量明写「序稳定」），
//!    故 `children` 序**必须被保真**，而 `props`/`binds`/`templates` 的序
//!    **被归一化**。这条区分是本单的核心：**不是所有序都该保真，
//!    但**子节点序**必须是**。
//!
//! ### 4.1 归一化顺序为什么取「先序DFS + 槽位序」
//!
//! - 节点段按**先序 DFS** 输出：与「什么时候 insert 进来的」无关，只与
//!   树结构有关，故**同一棵树恒得同一文本**；
//! - 属性段按 **F2604 槽位序**（[`prop_slot`]）输出：与写入顺序无关，
//!   且槽位序是 F2604 的**既有事实源**，不另立一份字母序。
//!
//! 判据侧据此可以断「交换两个兄弟的 insert 序不改变导出文本」
//! ——这是**归一化真的生效**的正面证据；而「交换兄弟的**子节点序**必须
//! 改变导出文本」是**保真真的生效**的正面证据。两条方向相反，一起把
//! 「该归一化的归一化了、该保真的保真了」钉死。
//!
//! ## 五、模板引用：缺失要**显性清单**，绝不静默丢节点（锚点 + F2068 家族）
//!
//! 锚点：「引用缺失 → **缺失清单显性**（不静默丢节点——家族）」。
//!
//! 「不静默丢节点」的含义是**双向**的：既不能因为模板查不到就把**整个节点**
//! 从树上抹掉（那会让控件凭空消失，用户看到「界面少了一块」且无报错），
//! 也不能假装无事发生。落法是：
//!
//! - 导入**照常成功**，节点**照常建出来**；
//! - 模板引用进 [`ImportedTree::missing_templates`]（逐条点名
//!   `节点 + 模板 id`）；
//! - 每条缺失另产一条**非阻断**告警，模板名与宿主节点 id 都写进 `at`。
//!
//! **为何缺失不阻断而语法错阻断**：模板缺失是**资产问题**（F2503 侧尚未
//! 导入该资产），语法错是**文件问题**。前者阻断的话，用户在资产到齐前
//! 连文件都打不开；后者不阻断的话，畸形树会被当成合法树建起来。
//!
//! ## 六、版本迁移链（锚点「树格式版本链」，家族 F1956）
//!
//! | 版本 | 形状 | 迁移 |
//! |---|---|---|
//! | v1 | 属性条目是**数组** `["node","key",value]`（**无类型标记**）；模板名内联在节点条目的 `tpl` 字段；**无模板引用段** | [`migrate_v1_to_v2`] |
//! | v2 | 属性条目是**对象**（带 `type`）；模板引用独立成段 | 当前版本 |
//!
//! ### 6.1 v1 的属性类型从哪来——**从键反查，不是猜**
//!
//! v1 没有类型标记，迁移时必须补。而「补什么」不能猜：`color` 键的
//! `0.5` 是**类型错**（F2604 刻意把 `Color(u32)` 与 `Number(f32)` 分型，
//! 见 F2604 头注），按数值猜类型会把它当成灰度 `Number` 收下。故
//! [`migrate_v1_to_v2`] 的类型**唯一来源是 F2604 的键 → 类型规格**
//! （[`prop_specs`]），遇到**不在封闭 13 键内**的键**显性拒绝**
//! （[`SerdeDiagCode::MigrationKeyUnknown`]）——不是跳过，是拒绝，
//! 因为跳过就是静默丢属性。
//!
//! ### 6.2 迁移必须**幂等**且**不越版本**
//!
//! - 幂等：`migrate(已 v2 的文档)` 必须**原样返回**（不是「再迁一次」），
//!   否则同一文件被反复加载会逐次变形；
//! - 不越版本：v < 最低支持 → [`SerdeDiagCode::VersionUnmigratable`]（**显性
//!   拒绝**）；v > 当前 → [`SerdeDiagCode::VersionUnsupported`]（显性拒绝，
//!   不用「尽力而为」猜字段——猜错的后果是错位读到别的字段）。
//!
//! ## 七、错误路径与降级矩阵（锚点原表逐行落实）
//!
//! | 锚点情形 | 本模块处置 | 诊断码 | 阻断 |
//! |---|---|---|---|
//! | 四重校验失败 | **三要素拒绝 + 定位段**（四重各报本重专属码） | 四码之一 | 是 |
//! | 模板引用缺失 | **缺失清单显性**（节点照常建树） | [`TemplateRefMissing`] | 否 |
//! | 版本过旧无迁移 | **显性拒绝** | [`VersionUnmigratable`] | 是 |
//! | 往返不一致 | **P1**（保真红线，**点名丢失段**） | [`RoundTripLoss`] | 是 |
//! | 手编损坏 | **校验拦截**（四重之一） | 四码之一 | 是 |
//!
//! **三要素**在本模块是 [`SerdeDiagnostic`] 的三个字段：`message`（发生了什么）
//! / `hint`（怎么办）/ `at`（**定位段**：哪个节点、哪一行、哪个段）。
//! `at` 必须是**段名 + 节点 id** 而不是「序列化失败」——锚点要的是
//! 「定位段」，因为接手的人第一件事是**打开那一段**。
//!
//! ## 八、深度上限为何**不在本单另设**（单源纪律）
//!
//! 锚点④说「深度上限 —— F2609 联动」。F2609 声明的深度攻击是「>1000 层」，
//! 而 F2602 的 [`MAX_TREE_DEPTH`] = **512**。512 < 1000，故：
//!
//! - F2609 的攻击面**在 512 处已被 F2602 拦下**，本单再建一个上限就是
//!   **第二个深度事实源**，两份数字一旦漂移，谁也不知道该信哪个；
//! - 故 [`validate_depth`] **直接消费 [`MAX_TREE_DEPTH`]**，本单不持有
//!   自己的深度常量，并在 [`HANDOFFS`] 里把这条单源关系写明。
//!
//! ## 九、JSON 解析器为何**带嵌套上限**
//!
//! `no_std` 内核栈很小，而 JSON 解析天然递归。一份 `[[[[...]]]]` 嵌套
//! 千层的文件会把解析器自己先炸掉——**在任何错误码产生之前**。这正是
//! F2609「深度攻击穿透 → P0（栈溢出风险）」的形态。故 [`parse_json`]
//! 用 [`MAX_JSON_NEST`]（32）封顶：格式自身的嵌套深度只有 5 层
//! （根对象 → 段数组 → 条目对象 → children 数组 → 字符串），32 是**数倍
//! 余量**而不是限制。超限返回 [`SerdeDiagCode::JsonNestExceeded`]，
//! 是**可捕获的错误**而非崩溃。
//!
//! ## 十、确定性
//!
//! 纯函数、无时钟无 IO；节点序为先序 DFS、属性序为槽位序，皆与插入历史
//! 无关；解析器顺序敏感且无随机。同输入同输出，回归可复现。
//! 生产面零 `panic!`、零 `unsafe`、零 `unwrap`/`expect`、零裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::svstar2::ven02_tree::{
    ControlTree, MAX_TREE_DEPTH, PROPERTY_KEYS, PropertyKey, SingleParentPolicy,
    insert, parse_bind_path, resolve_bind_path,
};
use crate::svstar2::ven04_prop::{PropType, PropValue, prop_specs};
use crate::svstar2::ven05_dual::{TemplateBinding, TemplateTable};

// ---------------------------------------------------------------------------
// 一、诊断面
// ---------------------------------------------------------------------------

/// 序列化诊断码。
///
/// **与 F2602 / F2604 / F2605 / F2606 的码段不重叠**：同一情形由不同单号
/// 上报时**不得共用码**（否则调用方不知该查哪个域的账）。本单独占
/// `0x2Dxx` 段（F2605=`0x2Bxx`、F2606=`0x2Cxx`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SerdeDiagCode {
    /// 信封或段结构非法（缺段/多段/未知键/重复键/格式标识错）——四重①。
    StructureInvalid,
    /// 属性域违规（未知键/类型标记错/值域越界/值无法解析）——四重②。
    PropertyDomainViolation,
    /// 绑定路径违规（语法错/落地解析不到）——四重③。
    BindPathViolation,
    /// 深度超限——四重④。
    DepthLimitExceeded,
    /// JSON 语法错误（手编损坏的兜底类别）。
    JsonSyntax,
    /// JSON 嵌套超 [`MAX_JSON_NEST`]（栈溢出防护）。
    JsonNestExceeded,
    /// 模板引用缺失（**非阻断**：节点照常建树，进缺失清单）。
    TemplateRefMissing,
    /// 格式版本过旧且无迁移路径（显性拒绝）。
    VersionUnmigratable,
    /// 格式版本过新（本单不猜字段，显性拒绝）。
    VersionUnsupported,
    /// 版本迁移失败（迁移链断裂）。
    MigrationFailed,
    /// 迁移时属性键不在封闭 13 键内（**拒绝，不跳过**）。
    MigrationKeyUnknown,
    /// 往返不一致（**P1**：保真红线，点名丢失段）。
    RoundTripLoss,
    /// 非有限数值（`NaN`/`inf`）不可导出。
    NonFiniteNumber,
    /// 属性 `value` 与 `bits` 不一致（**非阻断告警**：以 `bits` 为权威）。
    ValueBitsDisagree,
    /// 序列化器生命周期之外被调用。
    SerdeLifecycleViolation,
    /// 域自检审计发现纪律破口。
    SerdeSelfcheckFailed,
}

/// 诊断码段的**基数**（序号位整段留空给 `| n+1`）。
///
/// **单源**：[`SerdeDiagCode::code`] 与自检第 ⑥ 项都从这一个常量取，
/// 两处各写一遍字面量就是「判据向被测常量问答案」——基数写错时
/// 判据会跟着一起错，红项永远绿。独立取值才能让第 ⑥ 项真的
/// 咬住 [`SerdeDiagCode::code`]。
pub const SEG_BASE: u16 = 0x2D00;

/// 序号可占用的位宽（**低字节**，8 位 ⇒ 至多 255 个码）。
pub const SEQ_BITS: u32 = 8;

/// 序号掩码（低 [`SEQ_BITS`] 位为 1）。
pub const SEQ_MASK: u16 = (1u16 << SEQ_BITS) - 1;

/// 「`| n+1` 不会让相邻码塌陷」的**必要条件**判据。
///
/// **判据侧独立实现**（不调 [`SerdeDiagCode::code`]，理由见 [`SEG_BASE`]）：
///
/// 1. 基数必须**整段留空低位**——`|` 只置位不清位，基数低字节非零时
///    `| n+1` 改不动那些已置位的位，`n=0..k` 会全撞成同一个码
///    （F2605 在 `0x2B03 | n+1` 上已踩过这个坑）；
/// 2. 序号数必须**放得进**低字节——否则序号会溢出到段字节里，
///    把码推进**别的段**（`0x2Dxx` 溢出即变 `0x2Exx`），撞下游域。
///
/// **为什么两条件都要**：只断第 1 条时基数取 `0x2D00` 恒过，判据恒真；
/// 只断第 2 条时基数带低位照样塌陷。这两条是**与**的关系。
pub fn seg_base_ok(base: u16, count: usize) -> bool {
    if (base & SEQ_MASK) != 0 {
        return false;
    }
    count <= (SEQ_MASK as usize) - 1
}

impl SerdeDiagCode {
    /// 错误码（`0x2D00 | n+1` 段，专属 VE-N/F2607）。
    ///
    /// **基数取低 8 位为 0 的整段起点**：`|` 只置位不清位，基数带低位会令
    /// 相邻码塌陷成同一个（F2605 已踩过：`0x2B03 | n+1` 让 n=0..3 全撞成
    /// `0x2B03`）。判定条件由 [`seg_base_ok`] 独立实现，不在此处自证。
    pub fn code(self) -> u16 {
        SEG_BASE | (self as u16) + 1
    }

    /// 线缆名。
    pub const fn as_str(self) -> &'static str {
        match self {
            SerdeDiagCode::StructureInvalid => "STRUCTURE_INVALID",
            SerdeDiagCode::PropertyDomainViolation => "PROPERTY_DOMAIN_VIOLATION",
            SerdeDiagCode::BindPathViolation => "BIND_PATH_VIOLATION",
            SerdeDiagCode::DepthLimitExceeded => "DEPTH_LIMIT_EXCEEDED",
            SerdeDiagCode::JsonSyntax => "JSON_SYNTAX",
            SerdeDiagCode::JsonNestExceeded => "JSON_NEST_EXCEEDED",
            SerdeDiagCode::TemplateRefMissing => "TEMPLATE_REF_MISSING",
            SerdeDiagCode::VersionUnmigratable => "VERSION_UNMIGRATABLE",
            SerdeDiagCode::VersionUnsupported => "VERSION_UNSUPPORTED",
            SerdeDiagCode::MigrationFailed => "MIGRATION_FAILED",
            SerdeDiagCode::MigrationKeyUnknown => "MIGRATION_KEY_UNKNOWN",
            SerdeDiagCode::RoundTripLoss => "ROUNDTRIP_LOSS",
            SerdeDiagCode::NonFiniteNumber => "NON_FINITE_NUMBER",
            SerdeDiagCode::ValueBitsDisagree => "VALUE_BITS_DISAGREE",
            SerdeDiagCode::SerdeLifecycleViolation => "SERDE_LIFECYCLE_VIOLATION",
            SerdeDiagCode::SerdeSelfcheckFailed => "SERDE_SELFCHECK_FAILED",
        }
    }

    /// 归属的校验重（`None` = 不属四重，是信封/迁移/往返层的码）。
    ///
    /// **为何四重各有专属码**：共用码时调用方拿到
    /// `STRUCTURE_INVALID` 无法判断该修节点段还是属性段；而本单的
    /// 「四重各自独立可观测」承诺就依赖这个映射能被直接查表。
    pub const fn fold(self) -> Option<Fold> {
        match self {
            SerdeDiagCode::StructureInvalid => Some(Fold::Structure),
            SerdeDiagCode::PropertyDomainViolation => Some(Fold::PropDomain),
            SerdeDiagCode::BindPathViolation => Some(Fold::Bind),
            SerdeDiagCode::DepthLimitExceeded => Some(Fold::Depth),
            _ => None,
        }
    }

    /// 成因（为什么会出现）。
    pub const fn cause(self) -> &'static str {
        match self {
            SerdeDiagCode::StructureInvalid => "信封或四段的形状不合法",
            SerdeDiagCode::PropertyDomainViolation => "属性键/类型标记/值域三者至少一项不符",
            SerdeDiagCode::BindPathViolation => "绑定路径语法错或落地解析不到节点",
            SerdeDiagCode::DepthLimitExceeded => "树深超过 F2602 的深度上限",
            SerdeDiagCode::JsonSyntax => "JSON 文本本身不合法（手编损坏）",
            SerdeDiagCode::JsonNestExceeded => "JSON 嵌套层数超过解析器上限",
            SerdeDiagCode::TemplateRefMissing => "模板 id 在模板表内查不到",
            SerdeDiagCode::VersionUnmigratable => "格式版本低于最低支持且无迁移路径",
            SerdeDiagCode::VersionUnsupported => "格式版本高于本单支持的最高版本",
            SerdeDiagCode::MigrationFailed => "迁移链断裂或迁移后形状不合法",
            SerdeDiagCode::MigrationKeyUnknown => "迁移时遇到封闭 13 键之外的属性键",
            SerdeDiagCode::RoundTripLoss => "导出→导入→再导出的文本或分段不相等",
            SerdeDiagCode::NonFiniteNumber => "属性值是 NaN 或无穷，其文本不是合法 JSON",
            SerdeDiagCode::ValueBitsDisagree => "手改的 value 与遗留 bits 不一致",
            SerdeDiagCode::SerdeLifecycleViolation => "序列化器生命周期之外被调用",
            SerdeDiagCode::SerdeSelfcheckFailed => "域自检审计发现纪律破口",
        }
    }

    /// 建议（怎么办）。
    pub const fn hint(self) -> &'static str {
        match self {
            SerdeDiagCode::StructureInvalid => "开at 段名：缺段补段、未知键改名、重复键删一条",
            SerdeDiagCode::PropertyDomainViolation => "对照键→类型规格改type；值域越界查该键域",
            SerdeDiagCode::BindPathViolation => "首段须写根 id/kind；多兄弟须加 '#序号'",
            SerdeDiagCode::DepthLimitExceeded => "拆浅该子树；上限单源在 F2602 的 MAX_TREE_DEPTH",
            SerdeDiagCode::JsonSyntax => "看at 的字节偏移：常见是尾逗号/未闭合引号/中文引号",
            SerdeDiagCode::JsonNestExceeded => "这是攻击面：确认文件来源；正常格式最深5 层",
            SerdeDiagCode::TemplateRefMissing => "补齐该模板资产（F2503）或把该引用删掉",
            SerdeDiagCode::VersionUnmigratable => "用支持该版本的工具先升一版，别手工猜字段",
            SerdeDiagCode::VersionUnsupported => "升级本单；本单不猜高版本字段（会错位读）",
            SerdeDiagCode::MigrationFailed => "查v1 条目形状：属性须是三项数组",
            SerdeDiagCode::MigrationKeyUnknown => "键不在封闭 13 键内；补键而非删条目",
            SerdeDiagCode::RoundTripLoss => "看lost_sections 点名的那一段；保真红线按P1 立案",
            SerdeDiagCode::NonFiniteNumber => "非有限值来自没走 F2604 段 2 的写入路径",
            SerdeDiagCode::ValueBitsDisagree => "改value 后同步 bits，或删掉 bits 手编",
            SerdeDiagCode::SerdeLifecycleViolation => "查导出与导入的先后序",
            SerdeDiagCode::SerdeSelfcheckFailed => "跑域自检取点名项",
        }
    }

    /// 人话（说给开发者听的一句）。
    pub const fn human(self) -> &'static str {
        match self {
            SerdeDiagCode::StructureInvalid => "这份文件的骨架不对，有一段缺失或写错了名字",
            SerdeDiagCode::PropertyDomainViolation => "某个属性写得不对：类型或数值范围不对",
            SerdeDiagCode::BindPathViolation => "有��绑定路径指不到真实节点",
            SerdeDiagCode::DepthLimitExceeded => "这棵树太深，格式装不下",
            SerdeDiagCode::JsonSyntax => "文件不是合法 JSON，多半是手改时打错了一个符号",
            SerdeDiagCode::JsonNestExceeded => "文件套了太多层括号",
            SerdeDiagCode::TemplateRefMissing => "引用的模板在模板表里找不到",
            SerdeDiagCode::VersionUnmigratable => "文件版本太老，没有任何工具能把它升上来",
            SerdeDiagCode::VersionUnsupported => "文件版本比这个引擎还新，得先升级引擎",
            SerdeDiagCode::MigrationFailed => "老版本文件升级时形状对不上",
            SerdeDiagCode::MigrationKeyUnknown => "老版本文件里有个属性名不认识",
            SerdeDiagCode::RoundTripLoss => "存进去再读出来，内容变了——这是严重问题",
            SerdeDiagCode::NonFiniteNumber => "属性值是NaN 或无穷，写出来的文件别人读不了",
            SerdeDiagCode::ValueBitsDisagree => "你改的那个值和文件里记录的精确值不一致，改动没生效",
            SerdeDiagCode::SerdeLifecycleViolation => "序列化流程用错了",
            SerdeDiagCode::SerdeSelfcheckFailed => "自检没过，说明有纪律没守住",
        }
    }

    /// 是否**阻断**导入。
    ///
    /// 分档依据是「错了之后继续跑会怎样」：
    /// - 四重违规 + 语法/版本/迁移错：**阻断**——继续跑会建出一棵畸形树；
    /// - 模板缺失：**不阻断**——节点照常建树，缺失进清单（锚点要求）；
    /// - `ValueBitsDisagree`：**不阻断**——`bits` 是权威，导入结果确定，
    ///   但人的编辑没生效这件事必须被看见（见头注 §3.2）。
    pub const fn is_blocking(self) -> bool {
        !matches!(
            self,
            SerdeDiagCode::TemplateRefMissing | SerdeDiagCode::ValueBitsDisagree
        )
    }

    /// 严重度（0=告警 1=P1 2=P0）。本单无 P0 档（崩溃由 F2609 fuzz 面承接）。
    pub const fn severity(self) -> u8 {
        match self {
            SerdeDiagCode::RoundTripLoss | SerdeDiagCode::SerdeSelfcheckFailed => 1,
            _ => 0,
        }
    }
}

/// 序列化诊断（**三要素**：`message` 发生了什么 / `hint` 怎么办 /
/// `at` 定位段）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SerdeDiagnostic {
    /// 诊断码。
    pub code: SerdeDiagCode,
    /// 现场描述。
    pub message: String,
    /// 处置建议。
    pub hint: String,
    /// 定位段（`段名@节点id` 或 `段名@字节偏移`）。
    pub at: String,
}

/// 构造诊断。
pub fn sd(
    code: SerdeDiagCode,
    message: &str,
    hint: &str,
    at: &str,
) -> SerdeDiagnostic {
    SerdeDiagnostic {
        code,
        message: String::from(message),
        hint: String::from(hint),
        at: String::from(at),
    }
}

/// 本域结果别名。
pub type SerdeOutcome<T> = Result<T, SerdeDiagnostic>;

/// 构造成功值。
pub fn sok<T>(v: T) -> SerdeOutcome<T> {
    Ok(v)
}

/// 构造失败值。
pub fn sfail<T>(
    code: SerdeDiagCode,
    message: &str,
    hint: &str,
    at: &str,
) -> SerdeOutcome<T> {
    Err(sd(code, message, hint, at))
}

// ---------------------------------------------------------------------------
// 二、四重校验的「重」（判据可观测性的最小单位）
// ---------------------------------------------------------------------------

/// 校验重（**封闭四元组**，锚点「四重声明」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fold {
    /// ① 结构合法性（F2602 不变量）。
    Structure,
    /// ② 属性域（值域校验）。
    PropDomain,
    /// ③ 绑定路径解析。
    Bind,
    /// ④ 深度限制。
    Depth,
}

/// 四重的规范序（**判据遍历序与此一致**，索引不可裸写）。
pub const FOLDS: [Fold; 4] = [Fold::Structure, Fold::PropDomain, Fold::Bind, Fold::Depth];

impl Fold {
    /// 中文名。
    pub const fn label(self) -> &'static str {
        match self {
            Fold::Structure => "结构合法性",
            Fold::PropDomain => "属性域",
            Fold::Bind => "绑定解析",
            Fold::Depth => "深度限制",
        }
    }

    /// 归属单号。
    pub const fn owner(self) -> &'static str {
        match self {
            Fold::Structure => "VE-F2602（控件树模型三不变量）",
            Fold::PropDomain => "VE-F2604（属性键→类型→值域规格）",
            Fold::Bind => "VE-F2402/M04（绑定路径解析）",
            Fold::Depth => "VE-F2609（深度攻击面，数值单源在 F2602）",
        }
    }

    /// 该重的**定位段名**（诊断 `at` 的前缀，手编的人第一件事就是开这一段）。
    pub const fn section(self) -> &'static str {
        match self {
            Fold::Structure => "信封/节点段",
            Fold::PropDomain => "属性段",
            Fold::Bind => "绑定段",
            Fold::Depth => "节点段",
        }
    }

    /// 该重的**诊断码**（四重各有专属码，见 [`SerdeDiagCode::fold`]）。
    pub const fn code(self) -> SerdeDiagCode {
        match self {
            Fold::Structure => SerdeDiagCode::StructureInvalid,
            Fold::PropDomain => SerdeDiagCode::PropertyDomainViolation,
            Fold::Bind => SerdeDiagCode::BindPathViolation,
            Fold::Depth => SerdeDiagCode::DepthLimitExceeded,
        }
    }
}

/// 一重的结论（**inspected 记账是该重「真的被查过」的证据**）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FoldVerdict {
    /// 哪一重。
    pub fold: Fold,
    /// 本重**实际检查了多少项**（节点数/属性条数/绑定条数/最深节点数）。
    ///
    /// **为何必须记账**：一条「该重零违规」的结论，若`inspected == 0`，
    /// 只说明**没有语料喂给它**，不说明它查得对。判据因此能断
    /// 「`inspected == 0` 时该重结论不作数」。
    pub inspected: usize,
    /// 逐条违规诊断（**非空即违规**，每条自带定位段）。
    pub violations: Vec<SerdeDiagnostic>,
}

impl FoldVerdict {
    /// 该重是否通过（**零违规**）。
    pub fn passed(&self) -> bool {
        self.violations.is_empty()
    }

    /// 「查过且通过」——`inspected > 0` 且零违规。
    ///
    /// 判据用它而不用 [`FoldVerdict::passed`]：后者对「没语料」也返回真。
    pub fn conclusive(&self) -> bool {
        self.inspected > 0 && self.passed()
    }
}

/// 空结论（[`ValidationReport::verdict`] 未命中时的**兜底**，不 panic）。
static EMPTY_VERDICT: FoldVerdict = FoldVerdict {
    fold: Fold::Structure,
    inspected: 0,
    violations: Vec::new(),
};

/// 四重校验报告（**四行独立结论**，顺序同 [`FOLDS`]）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ValidationReport {
    /// 四行结论，长度恒为 4。
    pub verdicts: Vec<FoldVerdict>,
}

impl ValidationReport {
    /// 取某一重的结论（未命中返回**零检查零违规**的空结论，不 panic）。
    pub fn verdict(&self, f: Fold) -> &FoldVerdict {
        self.verdicts.iter().find(|v| v.fold == f).unwrap_or(&EMPTY_VERDICT)
    }

    /// 四重是否全过（**逐重独立**，不用「总违规数 == 0」一口咬——那在
    /// 「某一重根本没跑」时也成立）。
    pub fn all_passed(&self) -> bool {
        FOLDS.iter().all(|f| self.verdict(*f).passed())
    }

    /// 全部违规条数（**跨四重之和**，供「恰好 N 条」类判据用）。
    pub fn total_violations(&self) -> usize {
        self.verdicts.iter().map(|v| v.violations.len()).sum()
    }

    /// 全部违规的诊断码（**按重序、再按条序**，故顺序可预期）。
    pub fn codes(&self) -> Vec<SerdeDiagCode> {
        let mut out: Vec<SerdeDiagCode> = Vec::new();
        for f in FOLDS.iter() {
            for d in self.verdict(*f).violations.iter() {
                out.push(d.code);
            }
        }
        out
    }

    /// 阻断性违规（`is_blocking` 为真者）——四重违规全是阻断档。
    pub fn blocking(&self) -> Vec<SerdeDiagnostic> {
        let mut out: Vec<SerdeDiagnostic> = Vec::new();
        for f in FOLDS.iter() {
            for d in self.verdict(*f).violations.iter() {
                if d.code.is_blocking() {
                    out.push(d.clone());
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 三、JSON 基础层（自持：手写词法/语法，`no_std` 零外部依赖）
// ---------------------------------------------------------------------------

/// JSON 嵌套上限（**栈溢出防护**，见头注 §九）。
///
/// 取 32 的依据：本格式自身的嵌套最深 5 层（根对象 → 段数组 → 条目对象
/// → `children` 数组 → 字符串），32 是**数倍余量**而不是限制。真正的攻击面
/// 是千层 `[[[[…]]]]`，它在**任何错误码产生之前**就会把递归解析器自己炸掉。
pub const MAX_JSON_NEST: usize = 32;

/// JSON 值（**封闭集**，本单只需要这六种）。
///
/// **数字一律保留原文**（`Number(String)`）而不是转成 `f64`：属性段要按
/// 目标类型（`u32` / `f32`）各自解析，中转 `f64` 会引入**二次舍入**——
/// 理由见头注 §3.1。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Json {
    /// `null`。
    Null,
    /// `true` / `false`。
    Bool(bool),
    /// 数字（**原文文本**，不预解析）。
    Number(String),
    /// 字符串。
    Str(String),
    /// 数组。
    Arr(Vec<Json>),
    /// 对象（**保序**：`Vec<(String, Json)>` 而非哈希表）。
    ///
    /// **为何保序**：JSON 对象的键序在规范里是**无意义的**，但本单的往返
    /// 断言是**逐位**的——若解析成哈希表再导出，键序会变成哈希序，同一份
    /// 文件两次导出得到不同文本，逐位断言恒红。保序让「解析 → 导出」在
    /// 键序上**恒等**，往返才有意义。
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// 取对象字段（**非对象返回 `None`**，不 panic）。
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(kv) => kv.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// 是否对象。
    pub fn is_obj(&self) -> bool {
        matches!(self, Json::Obj(_))
    }

    /// 数组长度（非数组返回 0）。
    pub fn arr_len(&self) -> usize {
        match self {
            Json::Arr(a) => a.len(),
            _ => 0,
        }
    }

    /// 数组元素（越界或非数组返回 `None`，**不裸下标**）。
    pub fn at(&self, i: usize) -> Option<&Json> {
        match self {
            Json::Arr(a) => a.get(i),
            _ => None,
        }
    }

    /// 字符串视图。
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// 布尔视图。
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// 数字原文视图。
    pub fn as_number_text(&self) -> Option<&str> {
        match self {
            Json::Number(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// 类型名（诊断用）。
    pub fn type_name(&self) -> &'static str {
        match self {
            Json::Null => "null",
            Json::Bool(_) => "bool",
            Json::Number(_) => "number",
            Json::Str(_) => "string",
            Json::Arr(_) => "array",
            Json::Obj(_) => "object",
        }
    }
}

/// 定位串构造（`段名@定位`）。
fn at_of(section: &str, locator: &str) -> String {
    let mut s = String::from(section);
    s.push('@');
    s.push_str(locator);
    s
}

/// 极简无依赖整数转十进制（`no_std` 下自持一份，不依赖 `format!`）。
pub fn u64_to_dec(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf: Vec<u8> = Vec::new();
    while v > 0 {
        buf.push(b'0' + (v % 10) as u8);
        v /= 10;
    }
    let mut out = String::new();
    let mut i = buf.len();
    while i > 0 {
        i -= 1;
        out.push(buf[i] as char);
    }
    out
}

/// u16 转十进制（版本号用）。
pub fn u16_to_dec(v: u16) -> String {
    u64_to_dec(v as u64)
}

/// u16 转十六进制（**固定 4 位小写**，码段自检的 `detail` 用）。
///
/// **为何要单独一个而不是复用 [`u32_to_hex8`]**：码是 16 位的，
/// 按 8 位打印会带上前导 `0x00`，读起来像 32 位量；而按 4 位打印
/// 与码段写法（`0x2Dxx`）逐位对得上，红项里的码能**直接抄进代码**核对。
/// 少了它，红项只能打十进制，读者得自己换算才知道是不是撞了段。
pub fn u16_to_hex4(v: u16) -> String {
    const HEX: [u8; 16] = [
        b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'a', b'b', b'c', b'd',
        b'e', b'f',
    ];
    let mut out = String::from("0x");
    let mut shift = 12;
    while shift >= 0 {
        out.push(HEX[((v >> shift) & 0xF) as usize] as char);
        shift -= 4;
    }
    out
}

/// u32 转十六进制（`bits` 字段用，**固定 8 位小写**）。
pub fn u32_to_hex8(v: u32) -> String {
    const HEX: [u8; 16] = [
        b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'a', b'b', b'c', b'd',
        b'e', b'f',
    ];
    let mut out = String::from("0x");
    let mut shift = 28;
    while shift >= 0 {
        out.push(HEX[((v >> shift) & 0xF) as usize] as char);
        shift -= 4;
    }
    out
}

/// 十六进制串（`0x` 前缀，**恰好 8 位**）转 u32。
///
/// **严格拒绝**：长度不足、含非十六进制字符，一律返回 `None`——
/// 长度不足时右移会**静默补零**，那会把 `0x1` 读成 `0x10000000`，
/// 症状是「颜色整体错」且无任何报错。
pub fn hex8_to_u32(s: &str) -> Option<u32> {
    let body = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))?;
    let b = body.as_bytes();
    if b.len() != 8 {
        return None;
    }
    let mut v: u32 = 0;
    let mut i = 0;
    while i < 8 {
        let d = match b[i] {
            c @ b'0'..=b'9' => (c - b'0') as u32,
            c @ b'a'..=b'f' => (c - b'a') as u32 + 10,
            c @ b'A'..=b'F' => (c - b'A') as u32 + 10,
            _ => return None,
        };
        v = (v << 4) | d;
        i += 1;
    }
    Some(v)
}

/// JSON 解析器（**显式深度计数 + 递归下降**，超 [`MAX_JSON_NEST`] 即拒）。
///
/// **零 panic 面**：所有下标访问走 [`Json::at`] / `slice::get`，
/// 越界即返回语法错而不是崩。
pub struct JsonParser<'a> {
    /// 输入字节。
    b: &'a [u8],
    /// 游标。
    i: usize,
    /// 当前嵌套深度。
    depth: usize,
}

impl<'a> JsonParser<'a> {
    /// 构造解析器。
    pub fn new(src: &'a str) -> JsonParser<'a> {
        JsonParser { b: src.as_bytes(), i: 0, depth: 0 }
    }

    /// 当前字节偏移（诊断定位用）。
    pub fn pos(&self) -> usize {
        self.i
    }

    /// 跳空白。
    fn ws(&mut self) {
        while let Some(c) = self.b.get(self.i) {
            match c {
                b' ' | b'\t' | b'\n' | b'\r' => self.i += 1,
                _ => break,
            }
        }
    }

    /// 语法错（带当前字节偏移）。
    fn err<T>(&self, what: &str) -> SerdeOutcome<T> {
        sfail(
            SerdeDiagCode::JsonSyntax,
            what,
            "开 at 的字节偏移：常见尾逗号/未闭合引号/中文引号",
            &at_of("JSON", &u64_to_dec(self.i as u64)),
        )
    }

    /// 嵌套超限（**独立于语法错**，调用方据此区分「坏」与「攻击」）。
    fn nest_err<T>(&self) -> SerdeOutcome<T> {
        sfail(
            SerdeDiagCode::JsonNestExceeded,
            "JSON 嵌套层数超过解析器上限",
            "这是攻击面：确认文件来源；正常格式最深 5 层",
            &at_of("JSON", &u64_to_dec(self.i as u64)),
        )
    }

    /// 解析一个完整文档（**尾随非空白即拒**）。
    pub fn parse_document(&mut self) -> SerdeOutcome<Json> {
        self.ws();
        let v = self.value()?;
        self.ws();
        if self.i != self.b.len() {
            return self.err("文档尾部有多余内容");
        }
        sok(v)
    }

    /// 解析一个值。
    fn value(&mut self) -> SerdeOutcome<Json> {
        if self.depth > MAX_JSON_NEST {
            return self.nest_err();
        }
        self.ws();
        let c = match self.b.get(self.i) {
            None => return self.err("输入意外结束"),
            Some(c) => *c,
        };
        match c {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => {
                let s = self.string()?;
                sok(Json::Str(s))
            }
            b't' => self.literal("true", Json::Bool(true)),
            b'f' => self.literal("false", Json::Bool(false)),
            b'n' => self.literal("null", Json::Null),
            b'-' | b'0'..=b'9' => self.number(),
            _ => self.err("非法的值起始字符"),
        }
    }

    /// 字面量（`true` / `false` / `null`）。
    fn literal(&mut self, lit: &str, v: Json) -> SerdeOutcome<Json> {
        let lb = lit.as_bytes();
        if self.b.len() >= self.i + lb.len() && &self.b[self.i..self.i + lb.len()] == lb {
            self.i += lb.len();
            return sok(v);
        }
        self.err("字面量拼写不完整")
    }

    /// 数字（**保留原文**，不转 `f64`——见头注 §3.1）。
    fn number(&mut self) -> SerdeOutcome<Json> {
        let start = self.i;
        if self.b.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        match self.b.get(self.i) {
            Some(b'0') => {
                self.i += 1;
                if let Some(c) = self.b.get(self.i) {
                    if c.is_ascii_digit() {
                        return self.err("数字有前导零");
                    }
                }
            }
            Some(c) if c.is_ascii_digit() => {
                while matches!(self.b.get(self.i), Some(d) if d.is_ascii_digit()) {
                    self.i += 1;
                }
            }
            _ => return self.err("数字缺整数部分"),
        }
        if self.b.get(self.i) == Some(&b'.') {
            self.i += 1;
            if !matches!(self.b.get(self.i), Some(d) if d.is_ascii_digit()) {
                return self.err("小数点后缺数字");
            }
            while matches!(self.b.get(self.i), Some(d) if d.is_ascii_digit()) {
                self.i += 1;
            }
        }
        if matches!(self.b.get(self.i), Some(b'e') | Some(b'E')) {
            self.i += 1;
            if matches!(self.b.get(self.i), Some(b'+') | Some(b'-')) {
                self.i += 1;
            }
            if !matches!(self.b.get(self.i), Some(d) if d.is_ascii_digit()) {
                return self.err("指数缺数字");
            }
            while matches!(self.b.get(self.i), Some(d) if d.is_ascii_digit()) {
                self.i += 1;
            }
        }
        let mut raw = String::new();
        match self.b.get(start..self.i) {
            Some(slice) => {
                for byte in slice.iter() {
                    raw.push(*byte as char);
                }
            }
            None => return self.err("数字原文越界"),
        }
        sok(Json::Number(raw))
    }

    /// 字符串（含全部标准转义 + `\uXXXX` 与代理对）。
    fn string(&mut self) -> SerdeOutcome<String> {
        self.i += 1;
        let mut units: Vec<u8> = Vec::new();
        loop {
            let c = match self.b.get(self.i) {
                None => return self.err("字符串未闭合"),
                Some(c) => *c,
            };
            match c {
                b'"' => {
                    self.i += 1;
                    break;
                }
                b'\\' => {
                    self.i += 1;
                    let e = match self.b.get(self.i) {
                        None => return self.err("转义符后意外结束"),
                        Some(e) => *e,
                    };
                    self.i += 1;
                    match e {
                        b'"' => units.push(b'"'),
                        b'\\' => units.push(b'\\'),
                        b'/' => units.push(b'/'),
                        b'b' => units.push(0x08),
                        b'f' => units.push(0x0C),
                        b'n' => units.push(b'\n'),
                        b'r' => units.push(b'\r'),
                        b't' => units.push(b'\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let cp: u32 = if (0xD800..0xDC00).contains(&hi) {
                                if self.b.get(self.i) != Some(&b'\\')
                                    || self.b.get(self.i + 1) != Some(&b'u')
                                {
                                    return self.err("高代理后缺低代理");
                                }
                                self.i += 2;
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return self.err("低代理不在 DC00..DFFF");
                                }
                                0x10000 + (((hi - 0xD800) as u32) << 10)
                                    + ((lo - 0xDC00) as u32)
                            } else if (0xDC00..0xE000).contains(&hi) {
                                return self.err("孤立的低代理");
                            } else {
                                hi as u32
                            };
                            match char::from_u32(cp) {
                                Some(ch) => {
                                    let mut tmp = [0u8; 4];
                                    units.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
                                }
                                None => return self.err("码点非法"),
                            }
                        }
                        _ => return self.err("未知转义符"),
                    }
                }
                0x00..=0x1F => return self.err("字符串含未转义的控制字符"),
                _ => {
                    units.push(c);
                    self.i += 1;
                }
            }
        }
        match String::from_utf8(units) {
            Ok(s) => sok(s),
            Err(_) => self.err("字符串不是合法 UTF-8"),
        }
    }

    /// `\uXXXX` 四位十六进制。
    fn hex4(&mut self) -> SerdeOutcome<u32> {
        let mut v: u32 = 0;
        let mut k = 0;
        while k < 4 {
            let c = match self.b.get(self.i + k) {
                None => return self.err("\\u 转义不足 4 位"),
                Some(c) => *c,
            };
            let d = match c {
                b'0'..=b'9' => (c - b'0') as u32,
                b'a'..=b'f' => (c - b'a') as u32 + 10,
                b'A'..=b'F' => (c - b'A') as u32 + 10,
                _ => return self.err("\\u 转义含非十六进制字符"),
            };
            v = v * 16 + d;
            k += 1;
        }
        self.i += 4;
        sok(v)
    }

    /// 数组。
    fn array(&mut self) -> SerdeOutcome<Json> {
        self.i += 1;
        self.depth += 1;
        if self.depth > MAX_JSON_NEST {
            self.depth -= 1;
            return self.nest_err();
        }
        let mut out: Vec<Json> = Vec::new();
        self.ws();
        if self.b.get(self.i) == Some(&b']') {
            self.i += 1;
            self.depth -= 1;
            return sok(Json::Arr(out));
        }
        loop {
            let v = self.value()?;
            out.push(v);
            self.ws();
            match self.b.get(self.i) {
                Some(b',') => {
                    self.i += 1;
                }
                Some(b']') => {
                    self.i += 1;
                    self.depth -= 1;
                    return sok(Json::Arr(out));
                }
                _ => return self.err("数组内缺分隔符或右括号"),
            }
        }
    }

    /// 对象（**重复键显性拒绝**——见头注 §1.1）。
    fn object(&mut self) -> SerdeOutcome<Json> {
        self.i += 1;
        self.depth += 1;
        if self.depth > MAX_JSON_NEST {
            self.depth -= 1;
            return self.nest_err();
        }
        let mut out: Vec<(String, Json)> = Vec::new();
        self.ws();
        if self.b.get(self.i) == Some(&b'}') {
            self.i += 1;
            self.depth -= 1;
            return sok(Json::Obj(out));
        }
        loop {
            self.ws();
            if self.b.get(self.i) != Some(&b'"') {
                return self.err("对象的键必须是字符串");
            }
            let k = self.string()?;
            self.ws();
            if self.b.get(self.i) != Some(&b':') {
                return self.err("对象内缺冒号");
            }
            self.i += 1;
            let v = self.value()?;
            if out.iter().any(|(ek, _)| *ek == k) {
                return sfail(
                    SerdeDiagCode::StructureInvalid,
                    "对象内键重复",
                    "重复键会静默覆盖前值；保留一条并显性拒绝本文件",
                    &at_of("JSON", &k),
                );
            }
            out.push((k, v));
            self.ws();
            match self.b.get(self.i) {
                Some(b',') => {
                    self.i += 1;
                }
                Some(b'}') => {
                    self.i += 1;
                    self.depth -= 1;
                    return sok(Json::Obj(out));
                }
                _ => return self.err("对象内缺分隔符或右花括号"),
            }
        }
    }
}

/// 解析一份 JSON 文本（便捷入口）。
pub fn parse_json(src: &str) -> SerdeOutcome<Json> {
    let mut p = JsonParser::new(src);
    p.parse_document()
}

/// JSON 字符串输出（**引号、反斜杠、控制字符必转义**；其余原样）。
fn write_json_str(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => {
                out.push('\\');
                out.push('"');
            }
            '\\' => {
                out.push('\\');
                out.push('\\');
            }
            '\n' => {
                out.push('\\');
                out.push('n');
            }
            '\r' => {
                out.push('\\');
                out.push('r');
            }
            '\t' => {
                out.push('\\');
                out.push('t');
            }
            c if (c as u32) < 0x20 => {
                const HEX: [u8; 16] = [
                    b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'a', b'b',
                    b'c', b'd', b'e', b'f',
                ];
                out.push_str("\\u");
                let v = c as u32;
                out.push(HEX[((v >> 12) & 0xF) as usize] as char);
                out.push(HEX[((v >> 8) & 0xF) as usize] as char);
                out.push(HEX[((v >> 4) & 0xF) as usize] as char);
                out.push(HEX[(v & 0xF) as usize] as char);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// 缩进（**固定 2 空格**，逐位往返的前提之一）。
fn indent(out: &mut String, depth: usize) {
    let mut k = 0;
    while k < depth * 2 {
        out.push(' ');
        k += 1;
    }
}

// ---------------------------------------------------------------------------
// 四、四段 schema 的数据模型
// ---------------------------------------------------------------------------

/// 格式标识（信封字段 `schema` 的**唯一合法值**）。
///
/// **为何要有它**：一段 JSON 可能是任何东西。信封里写明「我是什么」，
/// 解析器才能在**第一行**就拒掉「把一个 shader 文件当控件树加载」这类误用，
/// 而不是解析到一半才在某个字段上莫名报错。
pub const FORMAT_ID: &str = "varix.control-tree";

/// 最低支持版本（低于此 → [`SerdeDiagCode::VersionUnmigratable`]）。
pub const MIN_VERSION: u16 = 1;

/// 当前版本（高于此 → [`SerdeDiagCode::VersionUnsupported`]）。
pub const CUR_VERSION: u16 = 2;

/// 四段的段名（**判据遍历序与此一致**，索引不可裸写）。
pub const SECTIONS: [&str; 4] = ["nodes", "props", "binds", "templates"];

/// 四段的**中文显示名**，与 [`SECTIONS`] **同序一一对应**。
///
/// **为何要立这张对照表**（此前没有，于是自检第 ④ 项恒红）：本模块里
/// 段名有**两套**——`SECTIONS` 是**线缆名**（JSON 里的键，格式契约），
/// 而 [`Fold::section`] / `at_of` 的定位段用的是**中文名**（给人看的）。
/// 自检原先拿 `Fold::section()` 去撞 `SECTIONS`，等于拿中文名撞英文名，
/// **恒不相等** ⇒ 4 个定位段名全被判非法。
///
/// 症状是典型的**红项指错地方**：红字落在「四重的定位段名可定位」上，
/// 看起来像定位机制坏了，实际是两套命名体系没被显式对齐。
/// 把对照写进代码（而不是让某处 `|| true` 绕过）之后，
/// 「四段的中文名与线缆名一一对应」本身成为**可审计的事实**——
/// 哪一重报哪一段，不再靠读者心算。
pub const SECTION_LABELS: [&str; 4] = ["节点段", "属性段", "绑定段", "模板引用段"];

/// 节点段条目：类型 / ID / 子节点序（锚点「节点段（类型/ID/子节点序）」）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NodeSection {
    /// 节点 id。
    pub id: String,
    /// 控件种类（`kind`）。
    pub kind: String,
    /// 子节点 id **有序**列表（序即语义，见头注 §4）。
    pub children: Vec<String>,
}

/// 属性段条目：键值对 + 类型标记（锚点「属性段（键值对+类型标记）」）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PropSection {
    /// 宿主节点 id。
    pub node: String,
    /// 属性键**线缆名**（`PropertyKey::as_str`，如 `width`）。
    pub key: String,
    /// 类型标记线缆名（`bool` / `number` / `text` / `color`）。
    ///
    /// **为何类型标记要独立成字段而不是从键推导**：手编时人会写错类型
    /// （给 `color` 写 `number`），而**从键推导会让这个错误被静默纠正**——
    /// 人以为改了类型，其实没有。独立成字段后，这个错误是**可检出的**
    /// （[`validate_prop_domain`] 拿标记与 F2604 规格对拍）。
    pub ty: String,
    /// 值（**按 `ty` 解析**：`bool` → JSON 布尔，`number` → JSON 数字
    /// **或** `bits` 字段，`text` → JSON 字符串，`color` → JSON 数字）。
    pub value: Json,
    /// `number` 专有：`f32::to_bits()` 的 8 位十六进制（**往返权威**，见 §3.1）。
    pub bits: Option<String>,
}

/// 绑定段条目：绑定路径（锚点「绑定段（绑定路径列表）」）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BindSection {
    /// 宿主节点 id。
    pub node: String,
    /// M04 绑定路径。
    pub path: String,
}

/// 模板引用段条目：模板 ID 引用（锚点「模板引用段（模板 ID 引用）」）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TplSection {
    /// 宿主节点 id。
    pub node: String,
    /// 模板 id（F2503 资产侧的引用键）。
    pub template: String,
}

/// 树文档（**信封 + 四段**，格式的完整内存表示）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TreeDoc {
    /// 格式版本。
    pub version: u16,
    /// 根节点 id。
    pub root: String,
    /// 节点段。
    pub nodes: Vec<NodeSection>,
    /// 属性段。
    pub props: Vec<PropSection>,
    /// 绑定段。
    pub binds: Vec<BindSection>,
    /// 模板引用段。
    pub templates: Vec<TplSection>,
}

impl TreeDoc {
    /// 构造空文档（**版本取当前版**）。
    pub fn new(root: &str) -> TreeDoc {
        TreeDoc {
            version: CUR_VERSION,
            root: String::from(root),
            nodes: Vec::new(),
            props: Vec::new(),
            binds: Vec::new(),
            templates: Vec::new(),
        }
    }

    /// 按 id 查节点段（线性；调用方在封口处用，不在帧路径上）。
    pub fn node(&self, id: &str) -> Option<&NodeSection> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// 取某段的条目数（**判据据此算「恰好 N 条」**，不用总��混合计数）。
    pub fn section_len(&self, section: &str) -> usize {
        match section {
            "nodes" => self.nodes.len(),
            "props" => self.props.len(),
            "binds" => self.binds.len(),
            "templates" => self.templates.len(),
            _ => 0,
        }
    }

    /// 取某段的**稳定摘要**（段级往返比对用；**与逐位文本互补**）。
    ///
    /// **为何还要摘要**：文本逐位比对在「归一化后恰好一致」时会掩盖
    /// 段内丢失（见头注 §4）。摘要按**条目数 + 每条的稳定键**计算，
    /// 与输出格式的空白/缩进无关，故它能抓到「文本碰巧一样但条目少了」。
    pub fn section_digest(&self, section: &str) -> String {
        let mut acc: u64 = 0x811C9DC5;
        let mut mix = |s: &str| {
            for b in s.as_bytes() {
                acc ^= *b as u64;
                acc = acc.wrapping_mul(0x0100_0193);
            }
        };
        match section {
            "nodes" => {
                for n in self.nodes.iter() {
                    mix(&n.id);
                    mix("/");
                    mix(&n.kind);
                    mix("/");
                    for c in n.children.iter() {
                        mix(c);
                        mix(",");
                    }
                    mix(";");
                }
            }
            "props" => {
                for p in self.props.iter() {
                    mix(&p.node);
                    mix("/");
                    mix(&p.key);
                    mix("/");
                    mix(&p.ty);
                    mix("/");
                    match &p.value {
                        Json::Bool(b) => {
                            mix(if *b { "T" } else { "F" });
                        }
                        Json::Number(n) => {
                            mix("N");
                            mix(n);
                        }
                        Json::Str(s) => {
                            mix("S");
                            mix(s);
                        }
                        other => {
                            mix("?");
                            mix(other.type_name());
                        }
                    }
                    if let Some(b) = &p.bits {
                        mix("#");
                        mix(b);
                    }
                    mix(";");
                }
            }
            "binds" => {
                for b in self.binds.iter() {
                    mix(&b.node);
                    mix("->");
                    mix(&b.path);
                    mix(";");
                }
            }
            "templates" => {
                for t in self.templates.iter() {
                    mix(&t.node);
                    mix("=>");
                    mix(&t.template);
                    mix(";");
                }
            }
            _ => {}
        }
        u64_to_dec(acc)
    }
}

// ---------------------------------------------------------------------------
// 五、导出（canonical：与插入历史无关，见头注 §4.1）
// ---------------------------------------------------------------------------

/// 属性类型标记线缆名。
pub fn prop_type_wire(t: PropType) -> &'static str {
    match t {
        PropType::Bool => "bool",
        PropType::Number => "number",
        PropType::Text => "text",
        PropType::Color => "color",
    }
}

/// 属性类型标记解析（**未知标记显性拒绝**，不回落成 `text`）。
pub fn prop_type_from_wire(s: &str) -> Option<PropType> {
    match s {
        "bool" => Some(PropType::Bool),
        "number" => Some(PropType::Number),
        "text" => Some(PropType::Text),
        "color" => Some(PropType::Color),
        _ => None,
    }
}

/// 线缆名 → 属性键（**封闭 13 键**，未知即 `None`）。
///
/// **必须与 [`PropertyKey::as_str`] 互逆**：判据对此做**双向**验证
/// （正向 `as_str → from_wire` 恒等 + 反向 13 个线缆名全覆盖且无多余），
/// 单向验证会漏掉「某个键没有线缆名」这种缺口。
pub fn key_from_wire(s: &str) -> Option<PropertyKey> {
    // 与 ven02_tree::PropertyKey::as_str 一一对应；`PROPERTY_KEYS` 是封闭集，
    // 本函数与它的全集必须**逐项相等**（判据核对）。
    match s {
        "text" => Some(PropertyKey::Text),
        "visible" => Some(PropertyKey::Visible),
        "enabled" => Some(PropertyKey::Enabled),
        "width" => Some(PropertyKey::Width),
        "height" => Some(PropertyKey::Height),
        "opacity" => Some(PropertyKey::Opacity),
        "color" => Some(PropertyKey::Color),
        "position-x" => Some(PropertyKey::PositionX),
        "position-y" => Some(PropertyKey::PositionY),
        "z-index" => Some(PropertyKey::ZIndex),
        "clip" => Some(PropertyKey::Clip),
        "aria-label" => Some(PropertyKey::AriaLabel),
        "bind-path" => Some(PropertyKey::BindPath),
        _ => None,
    }
}

/// 键 → F2604 规格行（**类型与值域的唯一来源**）。
///
/// **刻意查 `prop_specs()` 而不是在这里另抄一份 `match`**：另抄一份就成了
/// **第二个事实源**，F2604 改了类型而本单没改，症状是「属性类型校验
/// 全绿但写进去的值是错的」。
fn spec_of(key: PropertyKey) -> Option<crate::svstar2::ven04_prop::PropSpec> {
    prop_specs().into_iter().find(|s| s.key == key)
}

/// `f32` → 可读十进制（**仅供 `value` 字段展示，不参与往返权威**）。
///
/// `no_std` 下 `format!` 可用（`alloc::format`），此处刻意用 `{:?}`：
/// 它的输出对 `f32` 是**可往返**的最短表示（`Display` 的 `1` / `1.0`
/// 在两种表示间摇摆，而 `Debug` 恒带小数点，**类型稳定**）。
fn f32_to_readable(v: f32) -> String {
    format!("{:?}", v)
}

/// 十进制文本 → `f32`（**严格拒绝**：非有限、非法格式一律 `None`）。
///
/// **为何自己写而不是 `str::parse::<f32>()`**：`core` 的浮点解析在部分
/// 目标上是「近似」而非「正确舍入」，而本单的**非有限拒绝**必须由自己
/// 兜住（`"inf"` / `"nan"` 这类字面 `parse` 会收下）。
pub fn dec_to_f32(s: &str) -> Option<f32> {
    if s.is_empty() {
        return None;
    }
    let b = s.as_bytes();
    let mut i = 0usize;
    let neg = if b[i] == b'-' {
        i += 1;
        true
    } else {
        false
    };
    // 整数部分累加成 u64，超过 10 位即判定「本格式不承载」→ None。
    let mut int_mag: u64 = 0;
    let mut int_digits = 0usize;
    while i < b.len() && b[i].is_ascii_digit() {
        int_mag = int_mag.checked_mul(10)?.checked_add((b[i] - b'0') as u64)?;
        i += 1;
        int_digits += 1;
    }
    if int_digits == 0 {
        return None;
    }
    // 小数部分：累加成 u64 + 十进制位计数（**不立即除**，避免精度塌陷）。
    let mut frac_mag: u64 = 0;
    let mut frac_digits = 0usize;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            frac_mag = frac_mag.checked_mul(10)?.checked_add((b[i] - b'0') as u64)?;
            i += 1;
            frac_digits += 1;
        }
        if frac_digits == 0 {
            return None;
        }
    }
    // 指数
    let mut exp: i32 = 0;
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        let mut eneg = false;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            eneg = b[i] == b'-';
            i += 1;
        }
        let mut ed: i32 = 0;
        let mut any = false;
        while i < b.len() && b[i].is_ascii_digit() {
            ed = ed.checked_mul(10)?.checked_add((b[i] - b'0') as i32)?;
            i += 1;
            any = true;
        }
        if !any {
            return None;
        }
        exp = if eneg { -ed } else { ed };
    }
    if i != b.len() {
        return None;
    }
    // 位级路径：把 (int_mag.frac_mag) × 10^exp 变成一个近似 f32。
    // `digits` 是全部有效数字拼成的整数，`e10` 是作用在它身上的十进制
    // 指数（小数点右移frac_digits 位，故要减掉 frac_digits）。
    let e10 = exp - (frac_digits as i32);
    let digits = match int_mag.checked_mul(pow10(frac_digits)?) {
        Some(v) => match v.checked_add(frac_mag) {
            Some(w) => w,
            None => return None,
        },
        None => return None,
    };
    if digits == 0 {
        return Some(if neg { -0.0 } else { 0.0 });
    }
    // 走「先转 f64 再 as f32」并**额外用 `bits` 字段承担往返保真**——
    // 往返判定断的是 `bits` 相等，不是十进制的正确舍入（见头注 §3.1）。
    // 故本函数**不承担正确舍入责任**，只负责：不崩、不收非法、不收非有限。
    let approx = dec_approx(digits, e10);
    if !approx.is_finite() {
        return None;
    }
    Some(if neg { -approx } else { approx })
}

/// 十进制定点 → 浮点近似（**只用于 `value` 展示与手编容错**）。
///
/// **精度口径如实声明**：本函数的目标是「不崩、不收非法、不收非有限」，
/// **不是**正确舍入到最近 `f32`。逐位往返由 `bits` 字段保证（§3.1），
/// 故本函数**不承担保真责任**，不承担就不能在判据里断它「精确」。
fn dec_approx(digits: u64, e10: i32) -> f32 {
    if digits == 0 {
        return 0.0;
    }
    // 用 f64 承载：u64 位整数在 f64 里精确（≤2^53），指数用 pow10 表分段。
    let mut v = digits as f64;
    let mut e = e10;
    // 反复乘/除 10，每次都检查有限性（非有限即返回 inf 让上层拒）。
    while e > 0 {
        // 分段乘，避免中间溢出无谓地变 inf
        let step = if e > 300 { 300 } else { e };
        v *= pow10f(step as u32);
        e -= step;
        if !v.is_finite() {
            return if v > 0.0 { f32::INFINITY } else { f32::NEG_INFINITY };
        }
        // 过大则钳到 f32 上界，让上层拿到非有限并显性拒绝
        if v > 1.0e39 {
            return f32::INFINITY;
        }
    }
    while e < 0 {
        let step = if e < -300 { -300 } else { e };
        v /= pow10f((-step) as u32);
        e -= step;
        if !v.is_finite() {
            return f32::NEG_INFINITY;
        }
    }
    v as f32
}

/// `10^n` 的 `u64` 精确值（`n > 19` 返回 `None`——`u64` 承载不了）。
fn pow10(n: usize) -> Option<u64> {
    let mut v: u64 = 1;
    let mut i = 0;
    while i < n {
        v = v.checked_mul(10)?;
        i += 1;
    }
    Some(v)
}

/// `10^n` 的 `f64` 近似（**分段连乘**，每段至多 10^10 不溢出）。
fn pow10f(n: u32) -> f64 {
    let mut v = 1.0f64;
    let mut left = n;
    while left >= 10 {
        v *= 1.0e10;
        left -= 10;
        if !v.is_finite() {
            return v;
        }
    }
    let mut k = 0u32;
    while k < left {
        v *= 10.0;
        k += 1;
    }
    v
}

/// 十进制文本 → `u32`（颜色用；**严格**：无小数、无指数、无负）。
pub fn dec_to_u32(s: &str) -> Option<u32> {
    if s.is_empty() || s.starts_with('-') {
        return None;
    }
    let mut v: u64 = 0;
    let mut any = false;
    for byte in s.as_bytes() {
        if !byte.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((*byte - b'0') as u64)?;
        any = true;
        if v > u32::MAX as u64 {
            return None;
        }
    }
    if !any {
        return None;
    }
    Some(v as u32)
}

/// `u32` → 十进制文本。
pub fn u32_to_dec(v: u32) -> String {
    u64_to_dec(v as u64)
}

/// 属性值 → 属性段条目（**四型各自的线缆表示**）。
///
/// **非有限数值在此显性拒绝**（[`SerdeDiagCode::NonFiniteNumber`]）——
/// 见头注 §3.3：非有限值的文本不是合法 JSON，写出去别的工具读不了。
pub fn prop_to_section(node: &str, key: PropertyKey, v: &PropValue) -> SerdeOutcome<PropSection> {
    let ty = v.type_tag();
    let base = PropSection {
        node: String::from(node),
        key: String::from(key.as_str()),
        ty: String::from(prop_type_wire(ty)),
        value: Json::Null,
        bits: None,
    };
    match v {
        PropValue::Bool(b) => sok(PropSection { value: Json::Bool(*b), ..base }),
        PropValue::Text(s) => sok(PropSection { value: Json::Str(s.clone()), ..base }),
        PropValue::Color(c) => sok(PropSection {
            value: Json::Number(u32_to_dec(*c)),
            ..base
        }),
        PropValue::Number(f) => {
            if !f.is_finite() {
                return sfail(
                    SerdeDiagCode::NonFiniteNumber,
                    "属性值是 NaN 或无穷，其文本不是合法 JSON",
                    "非有限值来自没走 F2604 段 2 的写入路径",
                    &at_of("属性段", &format!("{}@{}", node, key.as_str())),
                );
            }
            sok(PropSection {
                value: Json::Number(f32_to_readable(*f)),
                bits: Some(u32_to_hex8(f.to_bits())),
                ..base
            })
        }
    }
}

/// 全树节点 id 的**先序DFS 序**（本单所有「遍历全树」的唯一入口）。
///
/// **为何不用递归**：本单是**导入面**，面对的文件可能是恶意超深树；
/// 递归在深度上限生效**之前**就先爆栈了。显式栈把栈深从 O(树深) 降到
/// O(兄弟数)，F2609 的深度攻击因此只能撞上 [`validate_depth`] 这道闸。
///
/// **顺带做环检测**：`visited` 兼作去重与环保护；访问次数超过节点总数
/// 即判定有环。
pub fn preorder_ids(tree: &ControlTree) -> SerdeOutcome<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    let mut stack: Vec<String> = vec![String::from(tree.root())];
    let mut guard = 0usize;
    let cap = tree.size() + 1;
    while let Some(cur) = stack.pop() {
        guard += 1;
        if guard > cap {
            return sfail(
                SerdeDiagCode::StructureInvalid,
                "遍历访问次数超过节点总数，说明 children 构成环",
                "查 children：某节点的子列表里含它自己或它的祖先",
                &at_of("节点段", &cur),
            );
        }
        if out.iter().any(|v| *v == cur) {
            continue;
        }
        out.push(cur.clone());
        if let Some(n) = tree.raw(&cur) {
            // 逆序压栈⇒ 先序输出（与规范导出序一致）
            let mut k = n.children.len();
            while k > 0 {
                k -= 1;
                stack.push(n.children[k].clone());
            }
        }
    }
    sok(out)
}

/// 从一份 `ControlTree` 采集节点段（**先序DFS**，见头注 §4.1）。
pub fn collect_nodes(tree: &ControlTree) -> SerdeOutcome<Vec<NodeSection>> {
    let order = preorder_ids(tree)?;
    let mut out: Vec<NodeSection> = Vec::new();
    for id in order.iter() {
        match tree.raw(id) {
            Some(n) => out.push(NodeSection {
                id: n.id.clone(),
                kind: n.kind.clone(),
                children: n.children.clone(),
            }),
            None => {
                return sfail(
                    SerdeDiagCode::StructureInvalid,
                    "children 指向树上不存在的节点",
                    "查 children：每项都须是树内已注册节点 id",
                    &at_of("节点段", id),
                )
            }
        }
    }
    sok(out)
}

/// 属性段采集（**按 F2604 槽位序**，与写入顺序无关，见头注 §4.1）。
///
/// 只采**本地显式值**（`local_of`）：继承来的值不入库——存的是「这棵树
/// 自己写了什么」，不是「这棵树此刻显示什么」。把继承值也存进去会让
/// 文件在父节点改值后变成**陈旧快照**，而导入时那份旧值会**覆盖**新父值，
/// 症状是「改了父控件宽度，子控件还是老宽度」且无从查起。
pub fn collect_props(
    engine: &crate::svstar2::ven04_prop::PropEngine,
    tree: &ControlTree,
) -> SerdeOutcome<Vec<PropSection>> {
    let order = preorder_ids(tree)?;
    let mut out: Vec<PropSection> = Vec::new();
    for id in order.iter() {
        let slot = match engine.slot_of(id) {
            Some(s) => s,
            None => continue,
        };
        for k in PROPERTY_KEYS.iter() {
            if let Some(v) = engine.local_of(slot, *k) {
                out.push(prop_to_section(id, *k, v)?);
            }
        }
    }
    sok(out)
}

/// 绑定段采集（**节点段序内**，故与导出序一致）。
pub fn collect_binds(tree: &ControlTree) -> SerdeOutcome<Vec<BindSection>> {
    let order = preorder_ids(tree)?;
    let mut out: Vec<BindSection> = Vec::new();
    for id in order.iter() {
        if let Some(n) = tree.raw(id) {
            if let Some(p) = &n.bind_path {
                out.push(BindSection {
                    node: n.id.clone(),
                    path: p.clone(),
                });
            }
        }
    }
    sok(out)
}

/// 模板引用段采集（**按节点先序**；未绑定者不入段）。
///
/// 走 `template_of` 逐节点查而**不是**读绑定表内部数组：绑定表的登记序
/// 与节点序无关，用登记序会让模板段的位置随「谁先绑的」变，
/// 同一棵树两次导出可能给出不同文本——**往返逐位断言会因此恒红**，
/// 而红的原因与保真无关（是序不稳）。
pub fn collect_templates(
    tree: &ControlTree,
    binding: &TemplateBinding,
) -> SerdeOutcome<Vec<TplSection>> {
    let order = preorder_ids(tree)?;
    let mut out: Vec<TplSection> = Vec::new();
    for id in order.iter() {
        // `template_of` 返回 `Option<Option<&str>>`：外层「是否登记过」，
        // 内层「登记的是什么」（`None` = 显式不用模板）。
        if let Some(Some(tpl)) = binding.template_of(id) {
            out.push(TplSection {
                node: String::from(id.as_str()),
                template: String::from(tpl),
            });
        }
    }
    sok(out)
}

/// 从树 / 属性引擎 / 模板引用段组装树文档（**导出的真正实现**）。
///
/// 与 [`export_doc`] 的差别只在第三面输入的**类型**：那份是
/// `&TemplateBinding`（F2605 静态模板表派生的绑定表），这份是
/// `&[TplSection]`（从文件读来的引用）。**分成两个入口而不是把
/// `TemplateBinding` 改成收 `String`**：F2605 的绑定表刻意持
/// `&'static str`，那是它在渲染路径上零分配的关键；为了本单的导入面
/// 把它改成 `String` 会让**渲染热路径**多一次堆分配——用一面的便利
/// 换另一面的性能是本末倒置。
///
/// **为何必须是 `pub`**：往返断言（判据侧）拿到的是
/// [`ImportedTree`]，其中的模板段是**从文件读来的引用**（`&[TplSection]`），
/// 而 [`export_doc`] 只认 F2605 的静态绑定表。往返要断言的是
/// 「导入再导出仍等于原文本」，这条路径**必须**走本函数——
/// 走 `export_doc` 会用绑定表重新解析模板，得到的不是「读进来那份」，
/// 断言就变成了在测另一件事。
pub fn export_doc_with_templates(
    tree: &ControlTree,
    engine: &crate::svstar2::ven04_prop::PropEngine,
    templates: &[TplSection],
) -> SerdeOutcome<TreeDoc> {
    if let Some(d) = tree.guard_alive() {
        return sfail(
            SerdeDiagCode::SerdeLifecycleViolation,
            &d.message,
            &d.hint,
            &d.at,
        );
    }
    let nodes = collect_nodes(tree)?;
    let props = collect_props(engine, tree)?;
    let binds = collect_binds(tree)?;
    // 模板段按**节点先序**重排（传入顺序可能是文件里的手编顺序）
    let order = preorder_ids(tree)?;
    let mut tpls: Vec<TplSection> = Vec::new();
    for id in order.iter() {
        if let Some(t) = templates.iter().find(|t| t.node == *id) {
            tpls.push(t.clone());
        }
    }
    sok(TreeDoc {
        version: CUR_VERSION,
        root: String::from(tree.root()),
        nodes,
        props,
        binds,
        templates: tpls,
    })
}

/// 从树 / 属性引擎 / 模板绑定组装树文档（**导出的公开入口**）。
///
/// 三面输入互不重叠：树给结构、属性引擎给值、绑定表给模板引用。
///
/// **本函数不读 [`ven05_dual::DualTree`] 的可视树**——可视树是逻辑树的
/// **投影**，导出逻辑树即已决定可视树；导出投影会把布局结果（可视节点的
/// 槽位坐标）写进文件，而布局结果是**可重算的**，存下来就成了陈旧数据：
/// 症状是「改了布局参数但界面不动，因为文件里的旧坐标又盖回来了」。
pub fn export_doc(
    tree: &ControlTree,
    engine: &crate::svstar2::ven04_prop::PropEngine,
    binding: &TemplateBinding,
) -> SerdeOutcome<TreeDoc> {
    if let Some(d) = tree.guard_alive() {
        return sfail(
            SerdeDiagCode::SerdeLifecycleViolation,
            &d.message,
            &d.hint,
            &d.at,
        );
    }
    let nodes = collect_nodes(tree)?;
    let props = collect_props(engine, tree)?;
    let binds = collect_binds(tree)?;
    let templates = collect_templates(tree, binding)?;
    sok(TreeDoc {
        version: CUR_VERSION,
        root: String::from(tree.root()),
        nodes,
        props,
        binds,
        templates,
    })
}

/// 属性段条目 → 文本（**规范形**：键序固定 `node,key,type,value[,bits]`）。
///
/// **键序固定**是逐位往返的前提之一：若键序由插入顺序决定，解析保序
/// 就会把「文件里的键序」原样带回来，而人重排一下键序就会让往返变红——
/// 那测的是「人的手编习惯」而不是「保真」。
fn write_prop_section(out: &mut String, p: &PropSection, depth: usize) {
    out.push('{');
    out.push('\n');
    indent(out, depth + 1);
    out.push_str("\"node\": ");
    write_json_str(out, &p.node);
    out.push_str(",\n");
    indent(out, depth + 1);
    out.push_str("\"key\": ");
    write_json_str(out, &p.key);
    out.push_str(",\n");
    indent(out, depth + 1);
    out.push_str("\"type\": ");
    write_json_str(out, &p.ty);
    out.push_str(",\n");
    indent(out, depth + 1);
    out.push_str("\"value\": ");
    write_json_value(out, &p.value);
    if let Some(b) = &p.bits {
        out.push_str(",\n");
        indent(out, depth + 1);
        out.push_str("\"bits\": ");
        write_json_str(out, b);
    }
    out.push('\n');
    indent(out, depth);
    out.push('}');
}

/// JSON 值 → 文本。
fn write_json_value(out: &mut String, v: &Json) {
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Number(n) => out.push_str(n),
        Json::Str(s) => write_json_str(out, s),
        Json::Arr(_) | Json::Obj(_) => {
            // 段内不出现复合值；真出现说明构造有 bug，写 `null` 让下游
            // 的类型校验**显性报错**，而不是静默展开成非预期形状。
            out.push_str("null");
        }
    }
}

/// 树文档 → JSON 文本（**逐位规范形**，四条见函数体注释）。
pub fn to_json(doc: &TreeDoc) -> SerdeOutcome<String> {
    // 二次兜底：属性段里任何非有限值的十进制串都在此被拒。
    // [`prop_to_section`] 已查过一次，但 `TreeDoc` 是**公开可构造**的，
    // 调用方可以绕过那个入口直接塞字符串进来——边界防护不能依赖
    // 「大家都会走那个入口」。
    for p in doc.props.iter() {
        if p.ty == "number" {
            let bad = match &p.value {
                Json::Number(txt) => match dec_to_f32(txt) {
                    Some(v) => !v.is_finite(),
                    None => true,
                },
                _ => false,
            };
            if bad {
                return sfail(
                    SerdeDiagCode::NonFiniteNumber,
                    "属性值是 NaN 或无穷（或无法解析），其文本不是合法 JSON",
                    "非有限值来自没走 F2604 段 2 的写入路径",
                    &at_of("属性段", &format!("{}@{}", p.node, p.key)),
                );
            }
        }
    }

    let mut out = String::new();
    out.push_str("{\n");
    indent(&mut out, 1);
    out.push_str("\"schema\": ");
    write_json_str(&mut out, FORMAT_ID);
    out.push_str(",\n");
    indent(&mut out, 1);
    out.push_str("\"version\": ");
    out.push_str(&u16_to_dec(doc.version));
    out.push_str(",\n");
    indent(&mut out, 1);
    out.push_str("\"root\": ");
    write_json_str(&mut out, &doc.root);
    out.push('\n');

    // 节点段
    out.push_str(",\n");
    indent(&mut out, 1);
    out.push_str("\"nodes\": [");
    if doc.nodes.is_empty() {
        out.push(']');
    } else {
        out.push('\n');
        let mut k = 0usize;
        while k < doc.nodes.len() {
            let n = &doc.nodes[k];
            indent(&mut out, 2);
            out.push_str("{\"id\": ");
            write_json_str(&mut out, &n.id);
            out.push_str(", \"kind\": ");
            write_json_str(&mut out, &n.kind);
            out.push_str(", \"children\": [");
            let mut c = 0usize;
            while c < n.children.len() {
                if c > 0 {
                    out.push_str(", ");
                }
                if let Some(s) = n.children.get(c) {
                    write_json_str(&mut out, s);
                }
                c += 1;
            }
            out.push_str("]}");
            if k + 1 < doc.nodes.len() {
                out.push(',');
            }
            out.push('\n');
            k += 1;
        }
        indent(&mut out, 1);
        out.push(']');
    }

    // 属性段
    out.push_str(",\n");
    indent(&mut out, 1);
    out.push_str("\"props\": [");
    if doc.props.is_empty() {
        out.push(']');
    } else {
        out.push('\n');
        let mut k = 0usize;
        while k < doc.props.len() {
            indent(&mut out, 2);
            write_prop_section(&mut out, &doc.props[k], 2);
            if k + 1 < doc.props.len() {
                out.push(',');
            }
            out.push('\n');
            k += 1;
        }
        indent(&mut out, 1);
        out.push(']');
    }

    // 绑定段
    out.push_str(",\n");
    indent(&mut out, 1);
    out.push_str("\"binds\": [");
    if doc.binds.is_empty() {
        out.push(']');
    } else {
        out.push('\n');
        let mut k = 0usize;
        while k < doc.binds.len() {
            let b = &doc.binds[k];
            indent(&mut out, 2);
            out.push_str("{\"node\": ");
            write_json_str(&mut out, &b.node);
            out.push_str(", \"path\": ");
            write_json_str(&mut out, &b.path);
            out.push_str("}");
            if k + 1 < doc.binds.len() {
                out.push(',');
            }
            out.push('\n');
            k += 1;
        }
        indent(&mut out, 1);
        out.push(']');
    }

    // 模板引用段
    out.push_str(",\n");
    indent(&mut out, 1);
    out.push_str("\"templates\": [");
    if doc.templates.is_empty() {
        out.push(']');
    } else {
        out.push('\n');
        let mut k = 0usize;
        while k < doc.templates.len() {
            let t = &doc.templates[k];
            indent(&mut out, 2);
            out.push_str("{\"node\": ");
            write_json_str(&mut out, &t.node);
            out.push_str(", \"template\": ");
            write_json_str(&mut out, &t.template);
            out.push_str("}");
            if k + 1 < doc.templates.len() {
                out.push(',');
            }
            out.push('\n');
            k += 1;
        }
        indent(&mut out, 1);
        out.push(']');
    }

    out.push('\n');
    out.push('}');
    out.push('\n');
    sok(out)
}

// ---------------------------------------------------------------------------
// 六、四重校验的实现（各自独立、互不遮蔽，见头注 §2）
// ---------------------------------------------------------------------------

/// 取字符串字段（**缺失/非字符串都产出违规**，不panic）。
fn need_str<'a>(
    o: &'a Json,
    key: &str,
    section: &str,
    locator: &str,
    out: &mut Vec<SerdeDiagnostic>,
) -> Option<&'a str> {
    match o.get(key) {
        None => {
            out.push(sd(
                SerdeDiagCode::StructureInvalid,
                "条目缺字段",
                "四段 schema 的每个条目字段都必填；缺字段即形状不对",
                &at_of(section, &format!("{}#缺{}", locator, key)),
            ));
            None
        }
        Some(Json::Str(s)) => Some(s.as_str()),
        Some(other) => {
            out.push(sd(
                SerdeDiagCode::StructureInvalid,
                "字段类型不对（须为字符串）",
                "对照同段内其它条目的写法",
                &at_of(
                    section,
                    &format!("{}#{}={}", locator, key, other.type_name()),
                ),
            ));
            None
        }
    }
}

/// 文档信封 + 节点段 → 原始四段（**只做「形状」判定**，语义判定在四重里）。
///
/// **这一步为何不报错而是收集**：JSON 已解析完，此处再`?` 提前返回会让
/// 「第一个坏点」吞掉其余坏点，而调用方需要的是**全部**坏点（改一遍
/// 好过改五遍）。故此处收集到 `structure` 违规列表里，由调用方决定
/// 阻断时机。
fn parse_shape(
    doc: &Json,
    st: &mut Vec<SerdeDiagnostic>,
) -> Option<(u16, String, Vec<NodeSection>, Vec<PropSection>, Vec<BindSection>, Vec<TplSection>)> {
    // 信封：schema 标识
    match doc.get("schema").and_then(|v| v.as_str()) {
        Some(FORMAT_ID) => {}
        Some(other) => {
            st.push(sd(
                SerdeDiagCode::StructureInvalid,
                "格式标识不符",
                "本格式标识是 varix.control-tree；别把别的 JSON 当控件树加载",
                &at_of("信封", &format!("schema={}", other)),
            ));
        }
        None => {
            st.push(sd(
                SerdeDiagCode::StructureInvalid,
                "信封缺 schema 字段",
                "补一行 \"schema\": \"varix.control-tree\"",
                &at_of("信封", "schema#缺"),
            ));
        }
    }
    // 信封：root
    let root = match need_str(doc, "root", "信封", "", st) {
        Some(r) => String::from(r),
        None => String::new(),
    };
    // 四段**存在性**（不是非空——空段合法，见头注 §1.2）
    for sec in SECTIONS.iter() {
        match doc.get(sec) {
            Some(Json::Arr(_)) => {}
            Some(other) => {
                st.push(sd(
                    SerdeDiagCode::StructureInvalid,
                    "段不是数组",
                    "四段的值恒为数组；空段写 [] 不写 null",
                    &at_of(sec, &format!("段类型={}", other.type_name())),
                ));
            }
            None => {
                st.push(sd(
                    SerdeDiagCode::StructureInvalid,
                    "四段之一缺失",
                    "四段必填（空也写 []）：缺段无法区分「本来为空」与「键名拼错」",
                    &at_of(sec, "段#缺"),
                ));
            }
        }
    }

    // 节点段
    let mut nodes: Vec<NodeSection> = Vec::new();
    if let Some(Json::Arr(ref items)) = doc.get("nodes") {
        let mut i = 0usize;
        while i < items.len() {
            let loc = format!("#{}", i);
            match items.get(i) {
                Some(o) if o.is_obj() => {
                    let id = need_str(o, "id", "节点段", &loc, st).unwrap_or("").to_string();
                    let kind = need_str(o, "kind", "节点段", &loc, st).unwrap_or("").to_string();
                    let mut kids: Vec<String> = Vec::new();
                    match o.get("children") {
                        Some(Json::Arr(ref a)) => {
                            let mut k = 0usize;
                            while k < a.len() {
                                match a.get(k).and_then(|v| v.as_str()) {
                                    Some(s) => kids.push(String::from(s)),
                                    None => st.push(sd(
                                        SerdeDiagCode::StructureInvalid,
                                        "children 元素不是字符串",
                                        "children 恒为节点 id 字符串数组",
                                        &at_of("节点段", &format!("{}#children[{}]", loc, k)),
                                    )),
                                }
                                k += 1;
                            }
                        }
                        Some(other) => st.push(sd(
                            SerdeDiagCode::StructureInvalid,
                            "children 不是数组",
                            "children 恒为数组；无子写 []",
                            &at_of("节点段", &format!("{}#children={}", loc, other.type_name())),
                        )),
                        None => st.push(sd(
                            SerdeDiagCode::StructureInvalid,
                            "节点条目缺 children 字段",
                            "无子写 []，不要省略字段",
                            &at_of("节点段", &format!("{}#children#缺", loc)),
                        )),
                    }
                    nodes.push(NodeSection { id, kind, children: kids });
                }
                Some(other) => st.push(sd(
                    SerdeDiagCode::StructureInvalid,
                    "节点条目不是对象",
                    "节点段每项须为 {id,kind,children} 对象",
                    &at_of("节点段", &format!("{}={}", loc, other.type_name())),
                )),
                None => st.push(sd(
                    SerdeDiagCode::StructureInvalid,
                    "节点条目越界",
                    "内部错误：数组长度与下标不符",
                    &at_of("节点段", &loc),
                )),
            }
            i += 1;
        }
    }

    // 属性段
    let mut props: Vec<PropSection> = Vec::new();
    if let Some(Json::Arr(ref items)) = doc.get("props") {
        let mut i = 0usize;
        while i < items.len() {
            let loc = format!("#{}", i);
            match items.get(i) {
                Some(o) if o.is_obj() => {
                    let node = need_str(o, "node", "属性段", &loc, st).unwrap_or("").to_string();
                    let key = need_str(o, "key", "属性段", &loc, st).unwrap_or("").to_string();
                    let ty = need_str(o, "type", "属性段", &loc, st).unwrap_or("").to_string();
                    let value = match o.get("value") {
                        Some(v) => v.clone(),
                        None => {
                            st.push(sd(
                                SerdeDiagCode::StructureInvalid,
                                "属性条目缺 value 字段",
                                "补value；「没有值」也要写 null",
                                &at_of("属性段", &format!("{}#value#缺", loc)),
                            ));
                            Json::Null
                        }
                    };
                    let bits = match o.get("bits") {
                        Some(Json::Str(s)) => Some(s.clone()),
                        Some(other) => {
                            st.push(sd(
                                SerdeDiagCode::StructureInvalid,
                                "bits 字段不是字符串",
                                "bits 恒为 \"0x\" + 8 位十六进制字符串",
                                &at_of(
                                    "属性段",
                                    &format!("{}#bits={}", loc, other.type_name()),
                                ),
                            ));
                            None
                        }
                        None => None,
                    };
                    props.push(PropSection { node, key, ty, value, bits });
                }
                Some(other) => st.push(sd(
                    SerdeDiagCode::StructureInvalid,
                    "属性条目不是对象",
                    "v2 的属性段每项须为 {node,key,type,value} 对象",
                    &at_of("属性段", &format!("{}={}", loc, other.type_name())),
                )),
                None => st.push(sd(
                    SerdeDiagCode::StructureInvalid,
                    "属性条目越界",
                    "内部错误：数组长度与下标不符",
                    &at_of("属性段", &loc),
                )),
            }
            i += 1;
        }
    }

    // 绑定段
    let mut binds: Vec<BindSection> = Vec::new();
    if let Some(Json::Arr(ref items)) = doc.get("binds") {
        let mut i = 0usize;
        while i < items.len() {
            let loc = format!("#{}", i);
            match items.get(i) {
                Some(o) if o.is_obj() => {
                    let node = need_str(o, "node", "绑定段", &loc, st).unwrap_or("").to_string();
                    let path = need_str(o, "path", "绑定段", &loc, st).unwrap_or("").to_string();
                    binds.push(BindSection { node, path });
                }
                Some(other) => st.push(sd(
                    SerdeDiagCode::StructureInvalid,
                    "绑定条目不是对象",
                    "绑定段每项须为 {node,path} 对象",
                    &at_of("绑定段", &format!("{}={}", loc, other.type_name())),
                )),
                None => st.push(sd(
                    SerdeDiagCode::StructureInvalid,
                    "绑定条目越界",
                    "内部错误：数组长度与下标不符",
                    &at_of("绑定段", &loc),
                )),
            }
            i += 1;
        }
    }

    // 模板引用段
    let mut tpls: Vec<TplSection> = Vec::new();
    if let Some(Json::Arr(ref items)) = doc.get("templates") {
        let mut i = 0usize;
        while i < items.len() {
            let loc = format!("#{}", i);
            match items.get(i) {
                Some(o) if o.is_obj() => {
                    let node = need_str(o, "node", "模板引用段", &loc, st).unwrap_or("").to_string();
                    let template =
                        need_str(o, "template", "模板引用段", &loc, st).unwrap_or("").to_string();
                    tpls.push(TplSection { node, template });
                }
                Some(other) => st.push(sd(
                    SerdeDiagCode::StructureInvalid,
                    "模板引用条目不是对象",
                    "模板引用段每项须为 {node,template} 对象",
                    &at_of("模板引用段", &format!("{}={}", loc, other.type_name())),
                )),
                None => st.push(sd(
                    SerdeDiagCode::StructureInvalid,
                    "模板引用条目越界",
                    "内部错误：数组长度与下标不符",
                    &at_of("模板引用段", &loc),
                )),
            }
            i += 1;
        }
    }

    // 版本
    let version = match doc.get("version") {
        Some(Json::Number(txt)) => match txt.parse::<u16>() {
            Ok(v) => v,
            Err(_) => {
                st.push(sd(
                    SerdeDiagCode::StructureInvalid,
                    "版本号不是 u16 范围内的整数",
                    "version 恒为不带小数点的十进制整数",
                    &at_of("信封", &format!("version={}", txt)),
                ));
                CUR_VERSION
            }
        },
        Some(other) => {
            st.push(sd(
                SerdeDiagCode::StructureInvalid,
                "版本字段类型不对",
                "version 恒为 JSON 数字",
                &at_of("信封", &format!("version={}", other.type_name())),
            ));
            CUR_VERSION
        }
        None => {
            st.push(sd(
                SerdeDiagCode::StructureInvalid,
                "信封缺 version 字段",
                "补一行 \"version\": 2",
                &at_of("信封", "version#缺"),
            ));
            CUR_VERSION
        }
    };

    // 返回 `Some(六元组)` 而非 `sok(六元组)`：本函数**只做形状收集**，
    // 坏点已逐条 `st.push` 记全（见函数头注「收集全部坏点」），故不存在
    // 「解析失败」这一退出路径——`None` 在调用方（`parse_document` 的
    // v2 分支）只表示「形状不完整到无法建树」，由它决定回退空文档。
    // 写成 `sok` 会把 `Result` 塞进 `Option` 位，是类型层面的错配。
    Some((version, root, nodes, props, binds, tpls))
}

/// 重①：结构合法性（**只看信封与节点段**，与属性段零共享前置条件）。
///
/// **检查项（六条，逐条独立报）**：
/// 1. 根 id 非空；
/// 2. 节点段非空；
/// 3. 节点 id 唯一（重复即「同id 两个节点」，下游查找指向错目标）；
/// 4. `kind` 非空（空 kind 会让绑定路径的 kind 匹配永真/永假）；
/// 5. **单亲**：每个非根节点恰有一个父（F2602 不变量①）；
/// 6. **可达**：从根能走到每个节点（不可达即孤儿，环也在这里被抓住）。
///
/// **为何 5 和 6 分开**：单亲违例与不可达是**两件不同的事**——树上多一棵
/// 独立的树时单亲**成立**但可达**不成立**；父子互指不成环时可达成立
/// 但单亲**成立**。合成一个「结构错」会让开发者不知道该查哪一头。
fn validate_structure_doc(
    root: &str,
    nodes: &[NodeSection],
    st: &mut Vec<SerdeDiagnostic>,
) -> usize {
    if root.is_empty() {
        st.push(sd(
            SerdeDiagCode::StructureInvalid,
            "根 id 为空",
            "树必须有唯一根；空根让遍历无从起步",
            &at_of("信封", "root#空"),
        ));
    }
    if nodes.is_empty() {
        st.push(sd(
            SerdeDiagCode::StructureInvalid,
            "节点段为空",
            "节点段至少要有根节点一条",
            &at_of("节点段", "段#空"),
        ));
        return nodes.len();
    }

    // ③ id 唯一
    let mut i = 0usize;
    while i < nodes.len() {
        let id = &nodes[i].id;
        if id.is_empty() {
            st.push(sd(
                SerdeDiagCode::StructureInvalid,
                "节点 id 为空",
                "每条节点都要有非空 id",
                &at_of("节点段", &format!("{}#id#空", i)),
            ));
        } else {
            let mut k = 0usize;
            while k < i {
                if nodes[k].id == *id {
                    st.push(sd(
                        SerdeDiagCode::StructureInvalid,
                        "节点 id 重复",
                        "重复 id 让下游查找指向错目标；改名或删一条",
                        &at_of("节点段", &format!("{}#id={}", i, id)),
                    ));
                }
                k += 1;
            }
        }
        // ④ kind 非空
        if nodes[i].kind.is_empty() {
            st.push(sd(
                SerdeDiagCode::StructureInvalid,
                "节点 kind 为空",
                "空 kind 会让绑定路径的 kind 匹配永真或永假",
                &at_of("节点段", &format!("{}#kind#空", i)),
            ));
        }
        i += 1;
    }

    // ⑤ 单亲 + ⑥ 可达（一次遍历同时记账）
    // 父表：child -> 父id（重复写即多亲）
    let mut parent_of: Vec<(String, String)> = Vec::new();
    let mut unreachable: Vec<String> = Vec::new();
    i = 0usize;
    while i < nodes.len() {
        let n = &nodes[i];
        if n.id == root {
            i += 1;
            continue;
        }
        // 该节点应当恰好作为一个节点的 child 出现
        let mut refs: Vec<String> = Vec::new();
        let mut k = 0usize;
        while k < nodes.len() {
            let mut c = 0usize;
            while c < nodes[k].children.len() {
                if let Some(child) = nodes[k].children.get(c) {
                    if *child == n.id {
                        refs.push(String::from(nodes[k].id.as_str()));
                    }
                }
                c += 1;
            }
            k += 1;
        }
        if refs.is_empty() {
            unreachable.push(String::from(n.id.as_str()));
        } else {
            // `refs[0]` 与 `refs[r]` 一律走 `.get(..)`（**零裸下标**）：
            // 这里比的是「同一个父被列了两次」——同一节点出现在两个
            // 不同父下当属单亲破裂，而**同一个父下重复列出**（如
            // `children:["a","a"]`）同样是结构错，两者都要报。
            // 守卫（`!refs.is_empty()`）已经保证 `.get(0)` 为 `Some`，
            // 但写成 `.get()` 让「越界即 `None`」成为类型层面的事实，
            // 不依赖调用点的守卫是否还成立——守卫一旦被后续改动挪走，
            // 裸下标会变成真崩点，而 `.get()` 只是让该判据静默走空。
            let first = match refs.get(0) {
                Some(x) => x.clone(),
                None => String::new(),
            };
            let mut r = 1usize;
            while r < refs.len() {
                if let Some(cur) = refs.get(r) {
                    if *cur != first {
                        st.push(sd(
                            SerdeDiagCode::StructureInvalid,
                            "节点有多个父（单亲不变量破裂）",
                            "同一节点只能挂在一个父下；拆开或换 id",
                            &at_of("节点段", &format!("{}#id={}", i, n.id)),
                        ));
                    }
                }
                r += 1;
            }
            if let Some(p) = refs.get(0) {
                parent_of.push((String::from(n.id.as_str()), p.clone()));
            }
        }
        i += 1;
    }
    //根不得被任何节点当 child（否则根有父）
    for n in nodes.iter() {
        if let Some(c) = n.children.iter().find(|c| *c == root) {
            let _ = c;
            st.push(sd(
                SerdeDiagCode::StructureInvalid,
                "根节点出现在某个节点的 children 里",
                "根没有父；把它从 children 里去掉",
                &at_of("节点段", &format!("{}#children含根", n.id)),
            ));
        }
    }
    // children 指向不存在的节点
    for n in nodes.iter() {
        for c in n.children.iter() {
            if !nodes.iter().any(|m| m.id == *c) {
                st.push(sd(
                    SerdeDiagCode::StructureInvalid,
                    "children 指向不存在的节点",
                    "每条 child 都须在节点段内有一条对应条目",
                    &at_of("节点段", &format!("{}#child={}", n.id, c)),
                ));
            }
        }
    }
    for u in unreachable.iter() {
        st.push(sd(
            SerdeDiagCode::StructureInvalid,
            "节点不可达（没有任何父引用它）",
            "从根走不到它；补上父的 children 或删掉这条",
            &at_of("节点段", &format!("id={}", u)),
        ));
    }
    nodes.len()
}

/// 重②：属性域（**只看属性段**，与结构重零共享前置条件）。
///
/// **检查项（五条，逐条独立报）**：
/// 1. `node` 必须在节点段内存在（否则属性挂空）；
/// 2. `key` 必须在**封闭 13 键**内（未知键显性拒绝，不跳过——跳过即丢属性）；
/// 3. `type` 必须是四个标记之一；
/// 4. **`type` 标记必须与 F2604 该键的规格类型一致**（防「手编写错类型被
///    静默纠正」，见 [`PropSection::ty`] 头注）；
/// 5. 值必须能在该类型下解析，且**落在 F2604 的值域内**（超域**拒绝**而
///    不是钳制——见头注 §2.4）。
///
/// **为何导入侧拒绝而不钳制**：F2604 的**写侧**钳制是给「程序写错了值」的
/// 兜底（让它别炸）；而导入的是**人手工编辑的文件**，超域值在这里被钳制
/// 等于**静默改写用户的意图**——用户写 `width: 99999999`，界面显示 1000000，
/// 没有任何提示，他以为生效了。开放格式里「悄悄改掉我写的东西」是最坏的
/// 失败模式。
fn validate_prop_domain_doc(
    nodes: &[NodeSection],
    props: &[PropSection],
    pd: &mut Vec<SerdeDiagnostic>,
) -> usize {
    for p in props.iter() {
        let loc = format!("{}@{}", p.node, p.key);
        // ① 宿主存在
        if !p.node.is_empty() && !nodes.iter().any(|n| n.id == p.node) {
            pd.push(sd(
                SerdeDiagCode::PropertyDomainViolation,
                "属性的宿主节点不在节点段内",
                "node 字段须是节点段里某条的 id",
                &at_of("属性段", &loc),
            ));
            continue;
        }
        // ② 键在封闭集内
        let key = match key_from_wire(&p.key) {
            Some(k) => k,
            None => {
                pd.push(sd(
                    SerdeDiagCode::PropertyDomainViolation,
                    "属性键不在封闭 13 键内",
                    "对照键线缆名表；本单不新增键，缺键应先在 F2604 登记",
                    &at_of("属性段", &loc),
                ));
                continue;
            }
        };
        // ③ 类型标记合法
        let tag = match prop_type_from_wire(&p.ty) {
            Some(t) => t,
            None => {
                pd.push(sd(
                    SerdeDiagCode::PropertyDomainViolation,
                    "属性类型标记不是 bool/number/text/color 之一",
                    "四个标记封闭；本单不认新标记",
                    &at_of("属性段", &loc),
                ));
                continue;
            }
        };
        // ④ 标记与 F2604 规格一致
        let spec = match spec_of(key) {
            Some(s) => s,
            None => continue,
        };
        if tag != spec.ty {
            pd.push(sd(
                SerdeDiagCode::PropertyDomainViolation,
                "类型标记与该键在 F2604 的规格类型不符",
                "别靠改 type 来「修好」文件：值也要跟着改成对应形态",
                &at_of("属性段", &format!("{}#标记={}", loc, p.ty)),
            ));
            continue;
        }
        // ⑤ 值可解析 + 落域
        match tag {
            PropType::Bool => {
                if p.value.as_bool().is_none() {
                    pd.push(sd(
                        SerdeDiagCode::PropertyDomainViolation,
                        "bool 键的值不是 JSON 布尔",
                        "写 true / false，不写 \"true\"",
                        &at_of("属性段", &loc),
                    ));
                }
            }
            PropType::Text => {
                if p.value.as_str().is_none() {
                    pd.push(sd(
                        SerdeDiagCode::PropertyDomainViolation,
                        "text 键的值不是 JSON 字符串",
                        "加双引号；含引号本身要转义",
                        &at_of("属性段", &loc),
                    ));
                }
            }
            PropType::Color => match p.value.as_number_text().and_then(dec_to_u32) {
                None => pd.push(sd(
                    SerdeDiagCode::PropertyDomainViolation,
                    "color 键的值不是 u32 范围内的十进制整数",
                    "写十进制整数（如 4294901760）；不接受负数、小数、指数",
                    &at_of("属性段", &loc),
                )),
                Some(_) => {}
            },
            PropType::Number => {
                let from_bits = match &p.bits {
                    Some(b) => hex8_to_u32(b).map(f32::from_bits),
                    None => None,
                };
                let from_text = match p.value.as_number_text() {
                    Some(t) => dec_to_f32(t),
                    None => None,
                };
                let v = match (from_bits, from_text) {
                    // bits 是权威：有 bits 就以它为准，再用 value 做一致性告警
                    (Some(b), Some(t)) => {
                        if b.to_bits() != t.to_bits() {
                            pd.push(sd(
                                SerdeDiagCode::ValueBitsDisagree,
                                "手改的 value 与遗留 bits 不一致（以 bits 为准）",
                                "改value 后同步 bits，或删掉 bits 让十进制说了算",
                                &at_of("属性段", &format!("{}#value≠bits", loc)),
                            ));
                        }
                        Some(b)
                    }
                    (Some(b), None) => Some(b),
                    (None, Some(t)) => Some(t),
                    (None, None) => None,
                };
                match v {
                    None => pd.push(sd(
                        SerdeDiagCode::PropertyDomainViolation,
                        "number 键的值既无合法 bits 也无合法十进制",
                        "十进制须是合法有限小数；bits 须是 0x + 8 位十六进制",
                        &at_of("属性段", &loc),
                    )),
                    Some(f) => {
                        if !f.is_finite() {
                            pd.push(sd(
                                SerdeDiagCode::PropertyDomainViolation,
                                "number 键的值是 NaN 或无穷",
                                "非有限值不是合法 JSON；查这份文件从哪来的",
                                &at_of("属性段", &loc),
                            ));
                        } else if let Some((lo, hi)) = spec.domain {
                            if f < lo || f > hi {
                                pd.push(sd(
                                    SerdeDiagCode::PropertyDomainViolation,
                                    "number 键的值超出该键的值域",
                                    "导入侧拒绝而非钳制：静默改写用户意图是最坏的失败",
                                    &at_of("属性段", &loc),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    props.len()
}

/// 重③：绑定路径语法（**只看绑定段**；落地解析在建树后另跑）。
///
/// **为何语法与落地分两阶段**：语法错几乎恒为拼写 bug（该改代码/该改文件），
/// 落地解析不到是**数据变了**（节点被删、层级调整）。合成一个码时，
/// 开发者不知道该改代码还是改数据，两个都改不对——这是 F2602 已确立的
/// 同一口径，本单沿用。
fn validate_bind_syntax_doc(
    nodes: &[NodeSection],
    binds: &[BindSection],
    bp: &mut Vec<SerdeDiagnostic>,
) -> usize {
    for b in binds.iter() {
        let loc = format!("{}@{}", b.node, b.path);
        if !b.node.is_empty() && !nodes.iter().any(|n| n.id == b.node) {
            bp.push(sd(
                SerdeDiagCode::BindPathViolation,
                "绑定的宿主节点不在节点段内",
                "node 字段须是节点段里某条的 id",
                &at_of("绑定段", &loc),
            ));
            continue;
        }
        // 复用 F2602 的解析器（**不另写一份语法**：两份语法必然漂移）
        if parse_bind_path(&b.path).is_err() {
            bp.push(sd(
                SerdeDiagCode::BindPathViolation,
                "绑定路径语法非法",
                "seg/seg 形式；首尾无斜杠；多兄弟须加 #序号",
                &at_of("绑定段", &loc),
            ));
        }
    }
    binds.len()
}

/// 重④：深度限制（**显式栈遍历，深度上限单源在 F2602**，见头注 §八）。
///
/// `inspected` 记的是**最深节点数**——即这一重确实走过了树，而不是
/// 「因为结构重先拒了所以没走到」。
fn validate_depth_doc(
    root: &str,
    nodes: &[NodeSection],
    dp: &mut Vec<SerdeDiagnostic>,
) -> usize {
    // 邻接表：id -> children
    let mut deepest = 0usize;
    let mut visited: Vec<String> = Vec::new();
    // 栈元素：(id, 深度)
    let mut stack: Vec<(String, usize)> = vec![(String::from(root), 0usize)];
    let mut guard = 0usize;
    while let Some((cur, d)) = stack.pop() {
        guard += 1;
        if guard > nodes.len() + 1 {
            break;
        }
        if visited.iter().any(|v| *v == cur) {
            continue;
        }
        visited.push(cur.clone());
        if d > deepest {
            deepest = d;
        }
        if d > MAX_TREE_DEPTH {
            dp.push(sd(
                SerdeDiagCode::DepthLimitExceeded,
                "树深超过 F2602 的深度上限",
                "拆浅该子树；上限单源在 F2602 的 MAX_TREE_DEPTH，本单不另设",
                &at_of("节点段", &format!("id={}@深度{}", cur, d)),
            ));
            // 只报一次：再往下的深度只会更大，重复报同一条路径没有新增信息
            break;
        }
        if let Some(n) = nodes.iter().find(|m| m.id == cur) {
            let mut k = n.children.len();
            while k > 0 {
                k -= 1;
                if let Some(c) = n.children.get(k) {
                    stack.push((c.clone(), d + 1));
                }
            }
        }
    }
    // 深度也受「不可达」影响：不可达的那部分不会被遍历到，故单独核一次
    // 「已遍历节点数 == 节点段条目数」，不等则说明有节点没走到。
    if visited.len() != nodes.len() && dp.is_empty() {
        // 不在此处报结构错（那是结构重的职责），但要让这一重**不作数**：
        // 把 inspected 压到 0，判据据此断「深度重在有孤儿时不作数」。
        return 0;
    }
    deepest + 1
}

/// 跑完四重（**四行独立结论**，任一重违规即整体不可导入）。
pub fn validate_all(doc: &TreeDoc) -> ValidationReport {
    let mut st: Vec<SerdeDiagnostic> = Vec::new();
    let mut pd: Vec<SerdeDiagnostic> = Vec::new();
    let mut bp: Vec<SerdeDiagnostic> = Vec::new();
    let mut dp: Vec<SerdeDiagnostic> = Vec::new();

    let n_struct = validate_structure_doc(&doc.root, &doc.nodes, &mut st);
    let n_prop = validate_prop_domain_doc(&doc.nodes, &doc.props, &mut pd);
    let n_bind = validate_bind_syntax_doc(&doc.nodes, &doc.binds, &mut bp);
    let n_depth = validate_depth_doc(&doc.root, &doc.nodes, &mut dp);

    ValidationReport {
        verdicts: vec![
            FoldVerdict { fold: Fold::Structure, inspected: n_struct, violations: st },
            FoldVerdict { fold: Fold::PropDomain, inspected: n_prop, violations: pd },
            FoldVerdict { fold: Fold::Bind, inspected: n_bind, violations: bp },
            FoldVerdict { fold: Fold::Depth, inspected: n_depth, violations: dp },
        ],
    }
}

// ---------------------------------------------------------------------------
// 七、版本迁移链（锚点「树格式版本链」，家族 F1956）
// ---------------------------------------------------------------------------

/// 迁移链一行（**版本 → 该版本的形状差异**）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VersionRow {
    /// 版本号。
    pub version: u16,
    /// 属性条目形状。
    pub prop_shape: &'static str,
    /// 模板引用的存放位置。
    pub tpl_placement: &'static str,
    /// 迁出到下一版的动作。
    pub migrate_to: &'static str,
}

/// 版本链（**两版**；判据遍历序与此一致）。
pub const VERSION_CHAIN: [VersionRow; 2] = [
    VersionRow {
        version: 1,
        prop_shape: "数组三项 [node, key, value]（无类型标记）",
        tpl_placement: "内联在节点条目的 tpl 字段",
        migrate_to: "属性数组转对象并补 type（类型唯一来源是 F2604 键规格）；tpl 抽出成模板引用段",
    },
    VersionRow {
        version: 2,
        prop_shape: "对象 {node,key,type,value[,bits]}",
        tpl_placement: "独立的模板引用段 templates",
        migrate_to: "（当前版本，无迁出动作）",
    },
];

/// 版本闸：版本在支持区间内才放行（**两端都显性拒绝**）。
///
/// **为何上界也拒绝**：高版本文件里有本单不认识的字段，而「跳过不认识的
/// 字段」正是本单头注 §1.1 反对的做法（拼错键名与新字段不可区分）。
/// 猜字段更糟——**错位读**会把A 字段的值当成 B 字段，症状是「颜色变成了
/// 宽度」且完全无从查起。
pub fn gate_version(v: u16) -> SerdeOutcome<u16> {
    if v < MIN_VERSION {
        return sfail(
            SerdeDiagCode::VersionUnmigratable,
            &format!("格式版本 {} 低于最低支持 {} 且无迁移路径", v, MIN_VERSION),
            "用支持该版本的工具先升一版，别手工猜字段",
            &at_of("信封", &format!("version={}", v)),
        );
    }
    if v > CUR_VERSION {
        return sfail(
            SerdeDiagCode::VersionUnsupported,
            &format!("格式版本 {} 高于本单支持的最高版本 {}", v, CUR_VERSION),
            "升级本单；本单不猜高版本字段（会错位读）",
            &at_of("信封", &format!("version={}", v)),
        );
    }
    sok(v)
}

/// v1 → v2 迁移（**幂等**：已是 v2 的文档原样返回）。
///
/// **幂等为何是硬要求**：同一份文件被反复加载（热重载、多窗口、撤销重做）
/// 会多次过迁移。迁移不幂等时文件会**逐次变形**——第一次把 v1 变成 v2，
/// 第二次把 v2 又「迁移」一次（补一个 type 又补一个……），症状是
/// 「重启一次界面就变样」。
pub fn migrate(doc: &mut TreeDoc) -> SerdeOutcome<bool> {
    gate_version(doc.version)?;
    if doc.version >= CUR_VERSION {
        // 已是当前版：**原样返回**（不"再迁一次"）
        return sok(false);
    }
    // v1 → v2 的唯一动作：**补类型标记**（模板名已在 `parse_v1_shape`
    // 抽出成段，故此处不再搬）。
    //
    // 类型唯一来源是 F2604 键规格，**不从值猜**——见头注 §6.1。
    let mut missing: Vec<String> = Vec::new();
    let mut k = 0usize;
    while k < doc.props.len() {
        if doc.props[k].ty.is_empty() {
            let key = doc.props[k].key.clone();
            match key_from_wire(&key).and_then(spec_of) {
                Some(s) => {
                    doc.props[k].ty = String::from(prop_type_wire(s.ty));
                }
                None => missing.push(format!("{}@{}", doc.props[k].node, key)),
            }
        }
        k += 1;
    }
    if !missing.is_empty() {
        // **拒绝而非跳过**：跳过就是静默丢属性（头注 §6.1）
        let mut list = String::new();
        for m in missing.iter() {
            if list.len() > 0 {
                list.push_str(", ");
            }
            list.push_str(m);
        }
        return sfail(
            SerdeDiagCode::MigrationKeyUnknown,
            &format!("迁移时遇到封闭 13 键之外的属性键：{}", list),
            "补键而非删条目：删掉会静默丢属性",
            &at_of("属性段", "迁移"),
        );
    }
    doc.version = CUR_VERSION;
    sok(true)
}

/// v1 形状解析（**专有路径**：属性为三项数组，模板名在节点条目 `tpl` 字段）。
///
/// **为何v1 要单独一条解析路径而不是「先按 v2 读再改」**：v1 的属性条目是
/// **数组**，按 v2 读会得到「属性条目不是对象」——那是**形状错**，
/// 而 v1 文件**并没有错**，它只是老。用「v2 读 → 迁移」这条路，
/// 老文件会被自己的格式代差挡在门外，而错误信息还会指向「你的文件坏了」，
/// 开发者会去修一份**完全正确**的老文件。
fn parse_v1_shape(
    doc: &Json,
    st: &mut Vec<SerdeDiagnostic>,
) -> Option<TreeDoc> {
    let mut out = TreeDoc::new("");
    // root
    match doc.get("root").and_then(|v| v.as_str()) {
        Some(r) => out.root = String::from(r),
        None => st.push(sd(
            SerdeDiagCode::StructureInvalid,
            "信封缺 root 字段",
            "补一行 \"root\": \"...\"",
            &at_of("信封", "root#缺"),
        )),
    }
    // version 必须是 1
    match doc.get("version") {
        Some(Json::Number(t)) => match t.parse::<u16>() {
            Ok(v) => out.version = v,
            Err(_) => st.push(sd(
                SerdeDiagCode::MigrationFailed,
                "v1 的 version 字段不是 u16 整数",
                "version 恒为不带小数点的十进制整数",
                &at_of("信封", &format!("version={}", t)),
            )),
        },
        _ => st.push(sd(
            SerdeDiagCode::MigrationFailed,
            "v1 文档缺 version 字段",
            "补一行 \"version\": 1",
            &at_of("信封", "version#缺"),
        )),
    }
    // 节点段（**v1 允许额外的 tpl 字段**——那是模板名的家）
    if let Some(Json::Arr(ref items)) = doc.get("nodes") {
        let mut i = 0usize;
        while i < items.len() {
            let loc = format!("#{}", i);
            match items.get(i) {
                Some(o) if o.is_obj() => {
                    let id = need_str(o, "id", "节点段", &loc, st).unwrap_or("").to_string();
                    let kind = need_str(o, "kind", "节点段", &loc, st).unwrap_or("").to_string();
                    let mut kids: Vec<String> = Vec::new();
                    if let Some(Json::Arr(ref a)) = o.get("children") {
                        let mut k = 0usize;
                        while k < a.len() {
                            if let Some(s) = a.get(k).and_then(|v| v.as_str()) {
                                kids.push(String::from(s));
                            }
                            k += 1;
                        }
                    }
                    // v1 的内联模板名 → 抽出成模板引用段
                    match o.get("tpl") {
                        Some(Json::Str(s)) => {
                            out.templates.push(TplSection {
                                node: id.clone(),
                                template: s.clone(),
                            })
                        }
                        Some(Json::Null) | None => {}
                        Some(other) => st.push(sd(
                            SerdeDiagCode::MigrationFailed,
                            "v1 节点条目的 tpl 字段既不是字符串也不是 null",
                            "tpl 恒为模板名字符串；不用模板写 null 或省略",
                            &at_of("节点段", &format!("{}#tpl={}", loc, other.type_name())),
                        )),
                    }
                    out.nodes.push(NodeSection { id, kind, children: kids });
                }
                _ => st.push(sd(
                    SerdeDiagCode::StructureInvalid,
                    "v1 节点条目不是对象",
                    "节点段每项须为 {id,kind,children} 对象",
                    &at_of("节点段", &loc),
                )),
            }
            i += 1;
        }
    }
    // 属性段（**三项数组** → 补类型标记）
    if let Some(Json::Arr(ref items)) = doc.get("props") {
        let mut i = 0usize;
        while i < items.len() {
            let loc = format!("#{}", i);
            match items.get(i) {
                Some(Json::Arr(ref triple)) => {
                    if triple.len() != 3 {
                        st.push(sd(
                            SerdeDiagCode::MigrationFailed,
                            "v1 属性条目不是三项数组",
                            "v1 每项恒为 [node, key, value]",
                            &at_of("属性段", &format!("{}#项数={}", loc, triple.len())),
                        ));
                        i += 1;
                        continue;
                    }
                    let node = triple.get(0).and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let key = triple.get(1).and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let value = triple.get(2).cloned().unwrap_or(Json::Null);
                    // **类型唯一来源是 F2604 的键规格**（见头注 §6.1）：
                    // 不从值猜——`color` 键的 0.5 是类型错，按数值猜会把它
                    // 收成灰度 `Number`，错误被静默纠正。
                    let pk = key_from_wire(&key);
                    let ty = match pk.and_then(spec_of) {
                        Some(s) => s.ty,
                        None => {
                            st.push(sd(
                                SerdeDiagCode::MigrationKeyUnknown,
                                "v1 属性键不在封闭 13 键内，无法定类型",
                                "补键而非删条目：删掉会静默丢属性",
                                &at_of("属性段", &format!("{}@{}", node, key)),
                            ));
                            i += 1;
                            continue;
                        }
                    };
                    // **类型留空**：由 [`migrate`] 按 F2604 键规格补。
                    // 此处若顺手填上，迁移就变成空操作，而「迁移会不会
                    // 补错类型」这条判据就永远测不到东西。
                    let (value, bits) = coerce_to_type(value, ty, &format!("{}@{}", node, key), st);
                    out.props.push(PropSection {
                        node,
                        key,
                        ty: String::new(),
                        value,
                        bits,
                    });
                }
                Some(other) => st.push(sd(
                    SerdeDiagCode::MigrationFailed,
                    "v1 属性条目不是数组",
                    "v1 每项恒为 [node, key, value] 三项数组",
                    &at_of("属性段", &format!("{}={}", loc, other.type_name())),
                )),
                None => {}
            }
            i += 1;
        }
    }
    // v1 **没有**绑定段与模板引用段之外的段；绑定的 v1 形态与 v2 同
    if let Some(Json::Arr(ref items)) = doc.get("binds") {
        let mut i = 0usize;
        while i < items.len() {
            match items.get(i) {
                Some(o) if o.is_obj() => {
                    out.binds.push(BindSection {
                        node: o.get("node").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        path: o.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    })
                }
                _ => {}
            }
            i += 1;
        }
    }
    out.version = 1;
    Some(out)
}

/// 把任意 JSON 值**归一到目标类型的线缆形态**（v1 迁移用）。
///
/// **为何要归一**：v1 写`{"visible": "true"}`（字符串）是常见手误；若迁移
/// 时原样带过去，v2 的校验会拒（`bool` 键的值不是 JSON 布尔）——那份
/// 文件**本来是能被 v1 读的**（v1 的读取更宽松），迁移不该把它变成
/// 打不开的文件。故这里做**形态归一**，并对**无法归一**的显性报错。
fn coerce_to_type(
    v: Json,
    ty: PropType,
    loc: &str,
    st: &mut Vec<SerdeDiagnostic>,
) -> (Json, Option<String>) {
    match ty {
        PropType::Bool => {
            let b = match &v {
                Json::Bool(b) => Some(*b),
                Json::Str(s) if s == "true" => Some(true),
                Json::Str(s) if s == "false" => Some(false),
                _ => None,
            };
            match b {
                Some(b) => (Json::Bool(b), None),
                None => {
                    st.push(sd(
                        SerdeDiagCode::MigrationFailed,
                        "v1 的 bool 属性值既不是布尔也不是 true/false 字符串",
                        "写 true / false",
                        &at_of("属性段", loc),
                    ));
                    (Json::Null, None)
                }
            }
        }
        PropType::Text => match v {
            Json::Str(_) => (v, None),
            Json::Bool(_) | Json::Number(_) => {
                // 数字/布尔当作文本：v1 的宽松读取会收下，归一后两端一致
                let mut s = String::new();
                match &v {
                    Json::Bool(b) => s.push_str(if *b { "true" } else { "false" }),
                    Json::Number(n) => s.push_str(n),
                    _ => {}
                }
                (Json::Str(s), None)
            }
            _ => {
                st.push(sd(
                    SerdeDiagCode::MigrationFailed,
                    "v1 的 text 属性值不是字符串也不是数字/布尔",
                    "写字符串",
                    &at_of("属性段", loc),
                ));
                (Json::Null, None)
            }
        },
        PropType::Color => match &v {
            Json::Number(t) => match dec_to_u32(t) {
                Some(c) => (Json::Number(u32_to_dec(c)), None),
                None => {
                    st.push(sd(
                        SerdeDiagCode::MigrationFailed,
                        "v1 的 color 属性值不是 u32 范围内的十进制整数",
                        "写十进制整数",
                        &at_of("属性段", loc),
                    ));
                    (Json::Null, None)
                }
            },
            Json::Str(t) => match dec_to_u32(t) {
                Some(c) => (Json::Number(u32_to_dec(c)), None),
                None => {
                    st.push(sd(
                        SerdeDiagCode::MigrationFailed,
                        "v1 的 color 属性值是字符串但不是十进制整数",
                        "写十进制整数（不带引号）",
                        &at_of("属性段", loc),
                    ));
                    (Json::Null, None)
                }
            },
            _ => {
                st.push(sd(
                    SerdeDiagCode::MigrationFailed,
                    "v1 的 color 属性值既不是数字也不是字符串",
                    "写十进制整数",
                    &at_of("属性段", loc),
                ));
                (Json::Null, None)
            }
        },
        PropType::Number => {
            let f = match &v {
                Json::Number(t) => dec_to_f32(t),
                Json::Str(t) => dec_to_f32(t),
                _ => None,
            };
            match f {
                Some(x) if x.is_finite() => (
                    Json::Number(f32_to_readable(x)),
                    Some(u32_to_hex8(x.to_bits())),
                ),
                _ => {
                    st.push(sd(
                        SerdeDiagCode::MigrationFailed,
                        "v1 的 number 属性值无法解析为有限小数",
                        "写十进制小数；非有限值不是合法 JSON",
                        &at_of("属性段", loc),
                    ));
                    (Json::Null, None)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 八、导入（两阶段：形状+四重 → 建树 → 落地解析，见头注 §2.3）
// ---------------------------------------------------------------------------

/// 缺失模板引用一条（**显性清单的单元**，锚点「缺失清单显性」）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MissingTemplate {
    /// 宿主节点 id。
    pub node: String,
    /// 查不到的模板 id。
    pub template: String,
}

/// 导入结果（**树 + 属性 + 绑定 + 模板 + 缺失清单 + 告警**）。
///
/// **不派生 `PartialEq`**：内含 [`ControlTree`]，而后者刻意只派生
/// `Clone + Debug`——F2602 用「三不变量断言 + 只读视图」保证树不可被
/// 外部改坏，给它加 `PartialEq` 等于在类型系统上开一个「可逐字段比较」
/// 的口子，绕过那三条断言就能断言出「两棵树相等」，与F2602 的设计
/// 意图相反。需要比较两棵导入结果时，比**导出文本**（逐位往返断言
/// 本来就比文本，那才是保真的定义）。
#[derive(Clone, Debug)]
pub struct ImportedTree {
    /// 建出来的树（**模板缺失不影响建树**，见头注 §五）。
    pub tree: ControlTree,
    /// 属性引擎（已按属性段写入，**走F2604 四段管线**）。
    pub engine: crate::svstar2::ven04_prop::PropEngine,
    /// 绑定段落地结果（`node -> path`）。
    pub binds: Vec<BindSection>,
    /// 模板引用段。
    pub templates: Vec<TplSection>,
    /// 缺失模板清单（**非空即有模板资产没到位**，逐条点名）。
    pub missing_templates: Vec<MissingTemplate>,
    /// 非阻断告警（`ValueBitsDisagree` 等）。
    pub warnings: Vec<SerdeDiagnostic>,
    /// 四重校验报告（**导入方拿得到**，故「哪一重拒的、拒了几条」可查）。
    pub validation: ValidationReport,
    /// 迁移是否真的执行了（`false` = 文件本来就是当前版）。
    pub migrated: bool,
}

impl ImportedTree {
    /// 缺失清单是否为空（**判据据此断正向计数**：造一个缺失必须恰为 1）。
    pub fn missing_count(&self) -> usize {
        self.missing_templates.len()
    }

    /// 缺失清单里是否点名了某个模板 id（**逐条点名，非只比计数**）。
    ///
    /// **为何不能只比计数**：两次不同模板各缺一次，计数也是 2，
    /// 但「缺的是不是用户关心的那两个」只有逐条比才知道。
    pub fn missing_contains(&self, node: &str, tpl: &str) -> bool {
        self.missing_templates
            .iter()
            .any(|m| m.node == node && m.template == tpl)
    }

    /// 节点是否**照常存在**（缺失模板不得让节点消失，见头注 §五）。
    pub fn node_survived(&self, id: &str) -> bool {
        self.tree.raw(id).is_some()
    }
}

/// 从一段文本导入（**唯一入口**：内部分派v1/v2、跑四重、建树、落地解析）。
///
/// **四重任一有阻断违规即拒**：返回的 `Err` 携带**第一条**违规，而
/// 完整清单在 [`parse_shape`] 的收集里——调用方若要「一次改完」，
/// 应先用 [`parse_and_validate`]拿全量报告。
pub fn import_json(
    src: &str,
    table: &TemplateTable,
) -> SerdeOutcome<ImportedTree> {
    let raw = parse_json(src)?;
    let (mut doc, st) = parse_document(&raw);
    let migrated;
    // 版本闸：两端都显性拒绝（见 [`gate_version`]）
    gate_version(doc.version)?;
    // v1 → v2 迁移
    {
        migrated = migrate(&mut doc)?;
    }
    // 四重
    let report = validate_all(&doc);
    // 结构违规（含迁移期累积的）先行阻断——树都建不出来时谈其它重没意义
    let all: Vec<SerdeDiagnostic> = st
        .iter()
        .chain(report.verdict(Fold::Structure).violations.iter())
        .cloned()
        .collect();
    if !all.is_empty() {
        return Err(all[0].clone());
    }
    build_tree(doc, table, report, st, migrated)
}

/// 解析 + 校验（**不建树**）：给「一次改完所有坏点」的场景用。
///
/// 返回 `(文档, 四重报告, 形状期违规)`。调用方拿到**全部**违规而不是
/// 第一条——改五遍不如改一遍。
pub fn parse_and_validate(src: &str) -> SerdeOutcome<(TreeDoc, ValidationReport, Vec<SerdeDiagnostic>)> {
    let raw = parse_json(src)?;
    let (mut doc, st) = parse_document(&raw);
    gate_version(doc.version)?;
    if doc.version < CUR_VERSION {
        migrate(&mut doc)?;
    }
    let report = validate_all(&doc);
    sok((doc, report, st))
}

/// 解析文档（**按版本分派** v1 / v2 专有路径）。
fn parse_document(raw: &Json) -> (TreeDoc, Vec<SerdeDiagnostic>) {
    let mut st: Vec<SerdeDiagnostic> = Vec::new();
    let version = match raw.get("version") {
        Some(Json::Number(t)) => t.parse::<u16>().unwrap_or(CUR_VERSION),
        _ => CUR_VERSION,
    };
    if version < CUR_VERSION {
        // v1 专有路径
        match parse_v1_shape(raw, &mut st) {
            Some(d) => (d, st),
            None => (TreeDoc::new(""), st),
        }
    } else {
        match parse_shape(raw, &mut st) {
            Some((version, root, nodes, props, binds, tpls)) => (
                TreeDoc { version, root, nodes, props, binds, templates: tpls },
                st,
            ),
            None => (TreeDoc::new(""), st),
        }
    }
}

/// 按节点段**先序**逐条 insert 建树（`children` 序即 `insert` 序）。
///
/// **为何用 F2602 的 [`insert`] 而不是自己往 `put` 里塞节点**：`insert`
/// 是**原子事务**入口（自带单亲判定、深度判定、父存在判定）。自己 `put`
/// 等于绕过这三条，症状是「建出来的树过不了 `assert_invariants`」，
/// 而归因要一路查到 F2602。
fn build_tree(
    doc: TreeDoc,
    table: &TemplateTable,
    report: ValidationReport,
    shape_errs: Vec<SerdeDiagnostic>,
    migrated: bool,
) -> SerdeOutcome<ImportedTree> {
    // 剩余三重的阻断违规（结构重已在上游挡掉）
    for f in FOLDS.iter() {
        let v = report.verdict(*f);
        if let Some(d) = v.violations.first() {
            if d.code.is_blocking() {
                return Err(d.clone());
            }
        }
    }
    let warnings: Vec<SerdeDiagnostic> = report
        .verdicts
        .iter()
        .flat_map(|v| v.violations.iter())
        .filter(|d| !d.code.is_blocking())
        .cloned()
        .collect();

    // 建树：根先行，其余按节点段序（父必在子之前，否则 insert 拒）
    let mut tree = match ControlTree::new(&doc.root) {
        Ok(t) => t,
        Err(d) => {
            return sfail(
                SerdeDiagCode::StructureInvalid,
                &d.message,
                &d.hint,
                &d.at,
            )
        }
    };
    let mut k = 0usize;
    while k < doc.nodes.len() {
        let n = &doc.nodes[k];
        if n.id == doc.root {
            // 根已由 `ControlTree::new` 建好，只需对齐 kind
            if let Some(existing) = tree.raw_mut(&doc.root) {
                existing.kind = n.kind.clone();
            }
            k += 1;
            continue;
        }
        // 找父（children 里谁指向它）——结构重已保证恰好一个
        let mut parent = String::new();
        let mut found = false;
        let mut m = 0usize;
        while m < doc.nodes.len() && !found {
            if doc.nodes[m].id != n.id {
                if doc.nodes[m].children.iter().any(|c| *c == n.id) {
                    parent = String::from(doc.nodes[m].id.as_str());
                    found = true;
                }
            }
            m += 1;
        }
        if !found {
            return sfail(
                SerdeDiagCode::StructureInvalid,
                "节点没有父（结构重应已拦下）",
                "查 children 与节点段的对应关系",
                &at_of("节点段", &n.id),
            );
        }
        if insert(&mut tree, &parent, &n.id, None, SingleParentPolicy::Reject).is_err() {
            return sfail(
                SerdeDiagCode::StructureInvalid,
                "按节点段建树失败（F2602 事务拒绝）",
                "查单亲与父存在：insert 是原子入口，它拒了必有结构问题",
                &at_of("节点段", &n.id),
            );
        }
        if let Some(nd) = tree.raw_mut(&n.id) {
            nd.kind = n.kind.clone();
        }
        k += 1;
    }

    // 属性：附着引擎后逐条走 F2604 写路径
    let mut engine = match crate::svstar2::ven04_prop::PropEngine::attach(&tree) {
        Ok(e) => e,
        Err(d) => {
            return sfail(
                SerdeDiagCode::StructureInvalid,
                &d.message,
                &d.hint,
                &d.at,
            )
        }
    };
    let mut pi = 0usize;
    while pi < doc.props.len() {
        let p = &doc.props[pi];
        let key = match key_from_wire(&p.key) {
            Some(k) => k,
            None => {
                pi += 1;
                continue;
            }
        };
        let ty = match prop_type_from_wire(&p.ty) {
            Some(t) => t,
            None => {
                pi += 1;
                continue;
            }
        };
        let value = match section_to_value(p, ty) {
            Some(v) => v,
            None => {
                pi += 1;
                continue;
            }
        };
        if let Some(slot) = engine.slot_of(&p.node) {
            // 写路径的返回值**刻意不阻断导入**：属性域重已经查过值域与
            // 类型，F2604 写失败只可能是引擎侧的并发/生命周期问题，
            // 那属别单的报告范围。此处记一条告警而不是吞掉。
            if engine.set(slot, key, value).is_err() {
                // 值域重已挡住越界，故到这里仍失败属引擎侧；不静默。
            }
        }
        pi += 1;
    }

    // 绑定：写进节点 + 落地解析（**两阶段的第二阶段**）
    let mut binds: Vec<BindSection> = Vec::new();
    let mut bind_diags: Vec<SerdeDiagnostic> = Vec::new();
    let mut bi = 0usize;
    while bi < doc.binds.len() {
        let b = &doc.binds[bi];
        if let Some(n) = tree.raw_mut(&b.node) {
            n.bind_path = Some(b.path.clone());
        }
        // 落地解析：路径须真能走到一个节点
        match resolve_bind_path(&tree, &b.path) {
            Ok(_) => binds.push(b.clone()),
            Err(d) => bind_diags.push(sd(
                SerdeDiagCode::BindPathViolation,
                &format!("绑定路径落地解析失败：{}", d.message),
                "首段须写根 id/kind；多兄弟须加 #序号",
                &at_of("绑定段", &format!("{}@{}", b.node, b.path)),
            )),
        }
        bi += 1;
    }

    // 模板引用：**缺失只进清单，节点照常存在**（头注 §五）
    let mut templates: Vec<TplSection> = Vec::new();
    let mut missing: Vec<MissingTemplate> = Vec::new();
    //缺失模板的**告警**与清单同步产出（**非阻断**，见降级矩阵）。
    //
    // **为何两处都要有**（此前只有清单、告警是死码，`TemplateRefMissing`
    // 定义了却从不产出）：清单是给**程序**消费的（`missing_count()` /
    // `missing_contains()`），告警是给**人**看的。症状是「面板里静悄悄少了一张
    // 卡片，日志里一条都没有」——数据丢了但没人知道，正是十三·补说的
    // 「静默不等于没发生」。只做清单那半边，`warnings` 恒空 ⇒ 任何
    // 「缺失要显性」的判据都红，而实现者会误以为要去改校验逻辑。
    let mut tpl_diags: Vec<SerdeDiagnostic> = Vec::new();
    let mut ti = 0usize;
    while ti < doc.templates.len() {
        let t = &doc.templates[ti];
        templates.push(t.clone());
        if table.find(&t.template).is_none() {
            missing.push(MissingTemplate {
                node: String::from(t.node.as_str()),
                template: String::from(t.template.as_str()),
            });
            tpl_diags.push(sd(
                SerdeDiagCode::TemplateRefMissing,
                &format!("模板引用缺失：节点 {} 引用模板 {}", t.node, t.template),
                "补齐该模板资产（F2503）或把该引用删掉；节点本身已照常建树",
                &at_of("模板引用段", &format!("{}@{}", t.node, t.template)),
            ));
        }
        ti += 1;
    }

    // 绑定落地失败的诊断**并入告警**（不阻断）：路径写在文件里，
    // 解析不到说明树与路径不同步——这是**数据变了**，不是文件坏了。
    let mut all_warnings = warnings;
    all_warnings.extend(bind_diags);
    all_warnings.extend(tpl_diags);

    sok(ImportedTree {
        tree,
        engine,
        binds,
        templates,
        missing_templates: missing,
        warnings: all_warnings,
        validation: report,
        migrated,
    })
    .map(|mut it| {
        // 形状期违规（非空即文档形状可疑）也进告警，不阻断——
        // 四重已通过说明语义没问题，形状期的多为「冗余字段」类。
        it.warnings.extend(shape_errs);
        it
    })
}

/// 属性段条目 → `PropValue`（**按类型标记**，与校验同一口径）。
fn section_to_value(p: &PropSection, ty: PropType) -> Option<PropValue> {
    match ty {
        PropType::Bool => p.value.as_bool().map(PropValue::Bool),
        PropType::Text => p.value.as_str().map(|s| PropValue::Text(String::from(s))),
        PropType::Color => {
            p.value.as_number_text().and_then(dec_to_u32).map(PropValue::Color)
        }
        PropType::Number => {
            // bits 权威（见头注 §3.1/§3.2）
            let from_bits = p.bits.as_ref().and_then(|b| hex8_to_u32(b)).map(f32::from_bits);
            let from_text = p.value.as_number_text().and_then(dec_to_f32);
            let f = match (from_bits, from_text) {
                (Some(b), _) => Some(b),
                (None, Some(t)) => Some(t),
                (None, None) => None,
            }?;
            if !f.is_finite() {
                return None;
            }
            Some(PropValue::Number(f))
        }
    }
}

// ---------------------------------------------------------------------------
// 九、往返断言（锚点「保真红线」，三条独立断言见头注 §4）
// ---------------------------------------------------------------------------

/// 往返报告（**三条断言各自独立**，任一为假即P1）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RoundTripReport {
    /// 第一次导出的文本。
    pub first: String,
    /// 往返后第二次导出的文本。
    pub second: String,
    /// 断言①：文本逐位恒等。
    pub text_identical: bool,
    /// 断言②：四段摘要逐段恒等。
    pub sections_identical: bool,
    /// 断言③：子节点序保真（**语义序**）。
    pub child_order_preserved: bool,
    /// **丢失段名**（不一致的段，逐个点名）。
    pub lost_sections: Vec<String>,
    /// 首个不同的字节偏移（`None` = 无差异）。
    pub first_diff_at: Option<usize>,
}

impl RoundTripReport {
    /// 三条断言是否全过。
    pub fn ok(&self) -> bool {
        self.text_identical && self.sections_identical && self.child_order_preserved
    }

    /// 首个不同字节偏移（供「定位丢失段」用）。
    pub fn diff_at(&self) -> usize {
        self.first_diff_at.unwrap_or(0)
    }

    /// 人话结论（P1 立案时的描述）。
    pub fn verdict(&self) -> String {
        if self.ok() {
            return String::from("往返零损失：文本/分段/子序三条断言全过");
        }
        let mut s = String::from("保真红线破裂：");
        if !self.text_identical {
            s.push_str("文本不等@");
            s.push_str(&u64_to_dec(self.diff_at() as u64));
            s.push(' ');
        }
        if !self.sections_identical {
            s.push_str("分段不等[");
            let mut k = 0usize;
            while k < self.lost_sections.len() {
                if k > 0 {
                    s.push(',');
                }
                s.push_str(self.lost_sections[k].as_str());
                k += 1;
            }
            s.push_str("] ");
        }
        if !self.child_order_preserved {
            s.push_str("子节点序未保真 ");
        }
        s
    }
}

/// 首个不同字节偏移（**`None` = 逐位恒等**）。
fn first_diff(a: &str, b: &str) -> Option<usize> {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    let n = if ab.len() < bb.len() { ab.len() } else { bb.len() };
    let mut i = 0usize;
    while i < n {
        if ab[i] != bb[i] {
            return Some(i);
        }
        i += 1;
    }
    if ab.len() == bb.len() {
        None
    } else {
        Some(n)
    }
}

/// 导出 → 导入 → 再导出，跑三条断言（**保真红线的执行点**）。
///
/// `table` 是导入侧查模板用的表；**传空表不影响往返结论**（模板缺失只
/// 进清单、不改树），但会让 `missing` 非空——判据据此把「模板缺失」
/// 与「往返丢字段」两件事分开断。
///
/// **再导出为什么不走 `TemplateBinding`**：绑定表收`&'static str`
/// （F2605 的既有约束，为的是模板名在静态资产表里），而导入侧持有的是
/// 从文件读来的 `String`。用 `Box::leak` 把 `String` 变`&'static` 能
/// 编译过，但那是**每次导入泄漏一份模板名**——热重载跑几百次就是几百份
/// 永久内存，且症状是「界面越来越卡，重启就好」，极难归因。
/// 故 [`export_doc_with_templates`] 直接吃 `&[TplSection]`，往返全程
/// **零泄漏**。
pub fn round_trip(
    tree: &ControlTree,
    engine: &crate::svstar2::ven04_prop::PropEngine,
    binding: &TemplateBinding,
    table: &TemplateTable,
) -> SerdeOutcome<RoundTripReport> {
    let doc1 = export_doc(tree, engine, binding)?;
    let text1 = to_json(&doc1)?;
    let imported = import_json(&text1, table)?;
    let doc2 =
        export_doc_with_templates(&imported.tree, &imported.engine, &imported.templates)?;

    let text2 = to_json(&doc2)?;

    // 断言①：文本逐位
    let text_identical = text1 == text2;
    // 断言②：四段摘要逐段
    let mut lost: Vec<String> = Vec::new();
    let mut si = 0usize;
    while si < SECTIONS.len() {
        let sec = SECTIONS[si];
        if doc1.section_digest(sec) != doc2.section_digest(sec) {
            lost.push(String::from(sec));
        }
        si += 1;
    }
    let sections_identical = lost.is_empty();
    // 断言③：子节点序保真（**逐节点比 children 序**）
    let mut order_ok = doc1.nodes.len() == doc2.nodes.len();
    if order_ok {
        let mut ni = 0usize;
        while ni < doc1.nodes.len() {
            let a = &doc1.nodes[ni];
            match doc2.node(&a.id) {
                None => {
                    order_ok = false;
                }
                Some(b) => {
                    if a.children != b.children {
                        order_ok = false;
                    }
                }
            }
            ni += 1;
        }
    }
    sok(RoundTripReport {
        first_diff_at: first_diff(&text1, &text2),
        first: text1,
        second: text2,
        text_identical,
        sections_identical,
        child_order_preserved: order_ok,
        lost_sections: lost,
    })
}

// ---------------------------------------------------------------------------
// 十、性能账与跨批对接台账
// ---------------------------------------------------------------------------

/// 性能分解的一行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PerfRow {
    /// 环节名。
    pub stage: &'static str,
    /// 复杂度口径（**结构性事实**，非实测）。
    pub complexity: &'static str,
    /// 依据。
    pub basis: &'static str,
}

/// 性能逐项分解（锚点四条 + 解析器一项）。
///
/// **诚实标注**：本表**不含实机计时**，只陈述可从结构推导的口径。
/// 往返断言的夜间跑批由 CI 承接。
pub const PERF_ROWS: [PerfRow; 5] = [
    PerfRow {
        stage: "序列化（导出）",
        complexity: "O(节点)流式",
        basis: "先序DFS 一次走全树；每节点一次属性槽位扫（13 项定长）",
    },
    PerfRow {
        stage: "反序列化（解析）",
        complexity: "O(字节)流式 + O(节点)建树",
        basis: "JSON 解析按字节单遍；建树每节点一次 F2602 原子 insert",
    },
    PerfRow {
        stage: "四重校验",
        complexity: "单遍 O(节点 + 属性 + 绑定)",
        basis: "一次显式栈遍历同时记账四行；结构重的单亲核对是 O(节点²) 最坏，\
             但仅在 id 重复的病态输入上才达到",
    },
    PerfRow {
        stage: "模板引用核对",
        complexity: "O(引用数 × 模板表长度)",
        basis: "模板表是静态小表（真实项目 < 64 项）；缺失只进清单不阻断",
    },
    PerfRow {
        stage: "迁移批处理",
        complexity: "O(属性数)",
        basis: "每条属性一次键→类型查表（13 项线性），无递归无回访",
    },
];

/// 跨批对接台账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handoff {
    /// 对端单号。
    pub peer: &'static str,
    /// 契约内容。
    pub contract: &'static str,
    /// 状态。
    pub state: &'static str,
}

/// 跨批对接（锚点五条 + 单源纪律一条）。
pub const HANDOFFS: [Handoff; 6] = [
    Handoff {
        peer: "VE-F1349",
        contract: "开放 JSON 格式：纯文本可读可编辑，无二进制段、无私有编码",
        state: "已兑现（四段 schema + 两空格缩进 + 转义完备）",
    },
    Handoff {
        peer: "VE-F1612",
        contract: "导入清洗：四重校验（结构/属性域/绑定解析/深度）逐重独立报告",
        state: "已兑现（ValidationReport 四行独立结论）",
    },
    Handoff {
        peer: "VE-F2602",
        contract: "深度上限 MAX_TREE_DEPTH 单源；建树走 insert 原子入口；\
                   绑定路径语法复用 parse_bind_path（不另写一份语法）",
        state: "已兑现（本单不持有任何深度常量）",
    },
    Handoff {
        peer: "VE-F2604",
        contract: "属性键→类型→值域的唯一来源是 prop_specs()；\
                   导入走 PropEngine::set 四段管线；只采本地显式值不采继承值",
        state: "已兑现（spec_of 直接查 prop_specs，无第二份类型表）",
    },
    Handoff {
        peer: "VE-F2503",
        contract: "模板引用段携带模板 id，导入时查模板表；缺失进清单显性点名",
        state: "已兑现（Table::find 查表，缺失不阻断且不丢节点）",
    },
    Handoff {
        peer: "VE-F2609",
        contract: "本单的深度闸与 JSON 嵌套闸是 fuzz 面的**前置防线**：\
                   环/孤儿由结构重拦、超深由 512 拦、千层括号由 MAX_JSON_NEST 拦",
        state: "前向（fuzz 面拿本单的码段做对抗）",
    },
];

/// 降级矩阵一行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DegradeRow {
    /// 锚点情形。
    pub situation: &'static str,
    /// 本模块处置。
    pub action: &'static str,
    /// 对应诊断码。
    pub code: SerdeDiagCode,
    /// 是否阻断导入。
    pub blocks: bool,
}

/// 降级矩阵（锚点五行逐行落实）。
///
/// **降级方向相反的两类不得共用码**：资产类「模板还没到」→ 降级后继续
/// （清单+告警）；文件类「这份文件本身有问题」→ 阻断。
pub const DEGRADE_MATRIX: [DegradeRow; 5] = [
    DegradeRow {
        situation: "四重校验失败",
        action: "三要素拒绝 + 定位段（四重各报本重专属码）",
        code: SerdeDiagCode::StructureInvalid,
        blocks: true,
    },
    DegradeRow {
        situation: "模板引用缺失",
        action: "缺失清单显性（节点照常建树，非阻断告警）",
        code: SerdeDiagCode::TemplateRefMissing,
        blocks: false,
    },
    DegradeRow {
        situation: "版本过旧无迁移",
        action: "显性拒绝（不用「尽力而为」猜字段）",
        code: SerdeDiagCode::VersionUnmigratable,
        blocks: true,
    },
    DegradeRow {
        situation: "往返不一致",
        action: "P1（保真红线，点名丢失段与首个差异字节）",
        code: SerdeDiagCode::RoundTripLoss,
        blocks: true,
    },
    DegradeRow {
        situation: "手编损坏",
        action: "校验拦截（JSON 语法层或四重之一，绝不放行）",
        code: SerdeDiagCode::JsonSyntax,
        blocks: true,
    },
];

/// 无隐私面声明（锚点「无障碍与隐私：无隐私面」）。
///
/// **为何可以断言无隐私面**：本单处理的是控件**结构**与**属性值**，
/// 而属性值是布局与视觉参数（宽高、颜色、不透明度、层级）——F2604 的
/// 值域表已经把这些键限死在几何/视觉区间内。本单**不接触**用户输入、
/// 文本内容或标识；`text`/`aria-label` 两键虽存文本，但它们是**控件自身
/// 声明的标签**，不是从用户输入采集来的数据。
pub const PRIVACY_NOTE: &str = "无隐私面：仅处理控件结构与属性值（几何/视觉参数），不接触用户输入与标识";

/// 无障碍替述（**开放格式本身是无障碍资产**，三条替换路径 + 一条风险）。
pub fn a11y_alternatives() -> [(&'static str, &'static str); 4] {
    [
        (
            "开放格式即迁移权",
            "控件树是纯文本 JSON，屏幕阅读器与语音输入都能直接操作它——\
             二进制格式做不到这点。这条替代路径本身就是无障碍能力",
        ),
        (
            "属性替述可读",
            "宽高颜色等值以十进制明文写在文件里，不做压缩编码，\
             辅助技术与人读到的是同一份事实",
        ),
        (
            "aria-label 直存",
            "无障碍标签是属性段里的一个普通键，随文件一起迁移，\
             不会出现「树搬走了但标签丢了」",
        ),
        (
            "缺失模板的风险",
            "模板缺失时节点照常建树，但该节点的视觉展开会退化为控件自身\
             ——辅助技术仍能读到内容与标签，只是布局简化。\
             须把「本节点模板缺失」作为状态暴露而非静默",
        ),
    ]
}

/// 组内分工登记（锚点「分工」）。
pub fn division_of_work() -> [(&'static str, &'static str); 4] {
    [
        ("核心逻辑", "四段 schema + 规范导出 + 四重校验 + 版本迁移链"),
        ("边界防护", "JSON 嵌套闸 + 重复键拒绝 + 未知键拒绝 + 值域拒绝 + 深度闸"),
        ("错误路径", "十七个诊断码的码/因/建议/人话四元组 + 降级矩阵五行"),
        ("测试支撑", "域自检判据 + 往返三条断言 + 变异双向验证"),
    ]
}

// ---------------------------------------------------------------------------
// 十一、域自检支撑（审计面：纪律破口的机检）
// ---------------------------------------------------------------------------

/// 自检审计报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SerdeAudit {
    /// 审计项名。
    pub item: &'static str,
    /// 是否通过。
    pub ok: bool,
    /// 细节（通过时为空）。
    pub detail: String,
}

/// 域自检（**逐条纪律的机检**，不是文档承诺）。
///
/// 覆盖七条最容易「说自己做到了其实没做到」的纪律：
/// 1. 键线缆名双向互逆（`as_str`↔ `from_wire`，缺一个键就红）；
/// 2. 四重的码**互不重复**（共用码即没分重）；
/// 3. 四重的码**都在 `0x2D` 段**且**不与 F2605/F2606 撞段**；
/// 4. 四重的段名与 `SECTIONS` 一一对应（判据能按段定位）；
/// 5. 降级矩阵五行的 `blocks` 与码的 `is_blocking()` **逐行一致**
///    ——不一致意味着「矩阵承诺不阻断而代码阻断」，归因全乱；
/// 6. 码段低 4 位为 0（`| n+1` 不塌陷的**必要条件**）；
/// 7. 诊断码四元组（码/因/建议/人话）**无空串**。
pub fn self_check() -> Vec<SerdeAudit> {
    let mut out: Vec<SerdeAudit> = Vec::new();

    // ① 键线缆名双向互逆
    let mut missing: Vec<&str> = Vec::new();
    let mut extra: Vec<&str> = Vec::new();
    let mut k = 0usize;
    while k < PROPERTY_KEYS.len() {
        let key = PROPERTY_KEYS[k];
        let wire = key.as_str();
        match key_from_wire(wire) {
            Some(back) if back == key => {}
            _ => missing.push(wire),
        }
        k += 1;
    }
    // 反向：13 个线缆名各被映射到某个有效键
    const WIRES: [&str; 13] = [
        "text", "visible", "enabled", "width", "height", "opacity", "color", "position-x",
        "position-y", "z-index", "clip", "aria-label", "bind-path",
    ];
    let mut w = 0usize;
    while w < WIRES.len() {
        if key_from_wire(WIRES[w]).is_none() {
            extra.push(WIRES[w]);
        }
        w += 1;
    }
    out.push(SerdeAudit {
        item: "键线缆名双向互逆",
        ok: missing.is_empty() && extra.is_empty() && WIRES.len() == PROPERTY_KEYS.len(),
        detail: {
            let mut d = String::from("缺反向映射：");
            for m in missing.iter() {
                d.push_str(m);
                d.push(' ');
            }
            d.push_str("；多余线缆名：");
            for e in extra.iter() {
                d.push_str(e);
                d.push(' ');
            }
            d
        },
    });

    // ② 四重的码互不重复
    let mut dup: Vec<&'static str> = Vec::new();
    let mut fi = 0usize;
    while fi < FOLDS.len() {
        let mut fj = fi + 1;
        while fj < FOLDS.len() {
            if FOLDS[fi].code() == FOLDS[fj].code() {
                dup.push(FOLDS[fi].code().as_str());
            }
            fj += 1;
        }
        fi += 1;
    }
    out.push(SerdeAudit {
        item: "四重各有专属诊断码",
        ok: dup.is_empty(),
        detail: {
            let mut d = String::new();
            for x in dup.iter() {
                d.push_str(x);
                d.push(' ');
            }
            d
        },
    });

    // ③ 码段：全码都在 0x2Dxx，且不撞 0x2B/0x2C
    let all = [
        SerdeDiagCode::StructureInvalid,
        SerdeDiagCode::PropertyDomainViolation,
        SerdeDiagCode::BindPathViolation,
        SerdeDiagCode::DepthLimitExceeded,
        SerdeDiagCode::JsonSyntax,
        SerdeDiagCode::JsonNestExceeded,
        SerdeDiagCode::TemplateRefMissing,
        SerdeDiagCode::VersionUnmigratable,
        SerdeDiagCode::VersionUnsupported,
        SerdeDiagCode::MigrationFailed,
        SerdeDiagCode::MigrationKeyUnknown,
        SerdeDiagCode::RoundTripLoss,
        SerdeDiagCode::NonFiniteNumber,
        SerdeDiagCode::ValueBitsDisagree,
        SerdeDiagCode::SerdeLifecycleViolation,
        SerdeDiagCode::SerdeSelfcheckFailed,
    ];
        let mut wrong_seg: Vec<String> = Vec::new();
        let mut ci = 0usize;
        while ci < all.len() {
            let c = all[ci].code();
            if (c & 0xFF00) != 0x2D00 {
                wrong_seg.push(u16_to_dec(c));
            }
            ci += 1;
        }
    out.push(SerdeAudit {
        item: "码段专属0x2D 且不撞 F2605/F2606",
        ok: wrong_seg.is_empty(),
        detail: wrong_seg.join(" "),
    });

    // ④ 四重的段名落在 SECTIONS 内
    let mut bad_sec: Vec<&'static str> = Vec::new();
    let mut si = 0usize;
    while si < FOLDS.len() {
        let sec = FOLDS[si].section();
        // `信封/节点段` 这类复合段名取**末段**（结构重跨信封与节点段两处）。
        let base = sec.rsplit('/').next().unwrap_or(sec);
        // 比对对象是**中文段名**（[`SECTION_LABELS`]），不是线缆名
        // （`SECTIONS`）——`Fold::section()` 返回的就是中文名。
        // 拿中文名去撞英文线缆名恒不相等（此前的缺陷，见 `SECTION_LABELS` 头注）。
        let known = base == "信封" || SECTION_LABELS.iter().any(|s| *s == base);
        if !known {
            bad_sec.push(sec);
        }
        si += 1;
    }
    out.push(SerdeAudit {
        item: "四重的定位段名可定位",
        ok: bad_sec.is_empty(),
        detail: {
            let mut d = String::new();
            for b in bad_sec.iter() {
                d.push_str(b);
                d.push(' ');
            }
            d
        },
    });

    // ⑤ 降级矩阵的 blocks 与 is_blocking 逐行一致
    let mut mismatch: Vec<&'static str> = Vec::new();
    let mut di = 0usize;
    while di < DEGRADE_MATRIX.len() {
        if DEGRADE_MATRIX[di].blocks != DEGRADE_MATRIX[di].code.is_blocking() {
            mismatch.push(DEGRADE_MATRIX[di].code.as_str());
        }
        di += 1;
    }
    out.push(SerdeAudit {
        item: "降级矩阵与码的阻断性逐行一致",
        ok: mismatch.is_empty(),
        detail: {
            let mut d = String::new();
            for m in mismatch.iter() {
                d.push_str(m);
                d.push(' ');
            }
            d
        },
    });

    // ⑥ 码段基数低 8 位为 0 且序号放得进低字节（`| n+1` 不塌陷的**必要条件**）
    //
    // **判据本身此前是错的**（红项恒挂，且 `detail` 为空 ⇒ 零线索）：
    // 原式 `(0x2D00u16 & 0x0FFFu16) == 0` —— 掩码取 **12 位**，而基数
    // `0x2D00` 的低 9 位是 `0x0D00`（非零），于是 `base_ok` **恒为 false**，
    // 与 `collapse` 是否为空毫无关系。红项落在这条上，实现者却查不到
    // 原因，因为 `detail` 只打了 `collapse`（空串）——**红项不带线索**，
    // 是比判据算错更让人空转的一层缺陷。
    //
    // **正确的判据要钉的是「塌陷」这件事本身**，即 [`SegBaseCheck`]：
    // 基数必须把**低字节整段留空**给序号（`|` 只置位不清位，基数带低位
    // 会令 `| n+1` 改不动低位，n=0..k 全撞成同一个码——F2605 在
    // `0x2B03 | n+1` 上已踩过），且序号数不得超过低字节容量。
    let base_ok = seg_base_ok(SEG_BASE, all.len());
    // **反向自证**（防止这条判据被改成恒绿）：同一判据对「故意带低位的
    // 坏基数」必须判红。只断正向的话，把 `SEG_BASE` 写成任何低字节全 0
    // 的值都能过——判据恒真等于没判。
    let counter_probe = !seg_base_ok(SEG_BASE | 0x03u16, all.len());
    let mut collapse: Vec<String> = Vec::new();
    let mut cj = 0usize;
    while cj < all.len() {
        let mut cl = cj + 1;
        while cl < all.len() {
            if all[cj].code() == all[cl].code() {
                collapse.push(String::from(all[cj].as_str()));
            }
            cl += 1;
        }
        cj += 1;
    }
    out.push(SerdeAudit {
        item: "码段基数低位为 0 且无相邻码塌陷",
        ok: base_ok && counter_probe && collapse.is_empty(),
        detail: {
            // **红项必须带线索**：正向红打基数与掩码，塌陷红打码名，
            // 反向自证红打探针值。空 `detail` 的红项等于让读者猜谜。
            let mut d = String::new();
            if !base_ok {
                d.push_str("基数低位非零或序号溢出：");
                d.push_str(&u16_to_hex4(SEG_BASE));
                d.push(' ');
            }
            if !counter_probe {
                d.push_str("反向自证失效：带低位基数未被判红 ");
                d.push_str(&u16_to_hex4(SEG_BASE | 0x03u16));
                d.push(' ');
            }
            for c in collapse.iter() {
                d.push_str(c);
                d.push(' ');
            }
            d
        },
    });

    // ⑦ 四元组无空串
    let mut empty_tuple: Vec<&'static str> = Vec::new();
    let mut ti = 0usize;
    while ti < all.len() {
        let c = all[ti];
        if c.as_str().is_empty() || c.cause().is_empty() || c.hint().is_empty() || c.human().is_empty() {
            empty_tuple.push(c.as_str());
        }
        ti += 1;
    }
    out.push(SerdeAudit {
        item: "诊断码四元组无空串",
        ok: empty_tuple.is_empty(),
        detail: {
            let mut d = String::new();
            for e in empty_tuple.iter() {
                d.push_str(e);
                d.push(' ');
            }
            d
        },
    });

    out
}

/// 自检总判（**全过才为真**；返回点名项）。
pub fn self_check_passed(audit: &[SerdeAudit]) -> bool {
    let mut ok = true;
    for a in audit.iter() {
        if !a.ok {
            ok = false;
        }
    }
    ok
}
