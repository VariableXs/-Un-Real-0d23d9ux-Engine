//! unx1902 ↔ UNX-F1902 惰性保存策略（TS 标志与首次使用陷阱）
//!
//! 单号：UNX-F1902
//! 标题：惰性保存策略（TS 标志与首次使用陷阱）
//! 域：CoRun · UNX-A3-B16 段「扩展状态」
//! 目标行数：2200 ~ 3000（硬指标）
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F1902-J1）：
//! `UNX-F1902-J1 惰性保存决策路径断言全过（TS 置位→首次使用 #NM 陷阱→
//!  恢复/清位闭环），保存时机三态（切出必存/惰性/永存）逐态正确 10/10`
//!
//! ## 组件清单（本单交付的纯功能件）
//! 1. `ExtState` / `SaveArea` / `SaveTicket`：x87、SSE、AVX(YMM)、AVX-512 四类
//!    扩展状态区的建模，各带 XCR0 使能位、脏标记、保存区偏移/大小/对齐。
//! 2. `Cleanliness` 闭集：`Clean / Dirty / Saved / LazyPending / Restored`。
//! 3. 保存原语 `save()` / `restore()`：模拟 fxsave64 / fxrstor64 语义，
//!    64 字节对齐断言 + 往返保真指纹断言（`ROUNDTRIP_SAMPLES` 组样本矩阵）。
//! 4. `SavePolicy` 三态 `AlwaysOnSwitchOut / Lazy / Never`，`decide()` 决策
//!    函数与 3×2×2 = 12 格全穷举判定表。
//! 5. `TsCell`：CR0.TS 延迟使能状态机 Clear→Set→Trap→Restore→Clear。
//! 6. `LazySaveEngine`：切出 / 切入 / 执行浮点 / #NM 陷入四段协同，
//!    扩展状态账守恒（F1908 消费口径）。
//! 7. 摊销窗接口 `AmortWindow`（F1903 预告）：窗内 LazyPending 不得误判丢失。
//! 8. 注入核 `inject_all()`：十形态注入 + 独立检出。
//! 9. 观测导出 `export_kv()` / `export_tlv()`（含长度自洽断言）。
//!
//! ## 职责定位
//! 本单**只做纯功能决策建模与断言**：不碰真实 CR0/XCR0、不做真实 fxsave、
//! 不注册真实 IDT 项、不产生真实 IO 与墙钟时间。寄存器内容是 `u64` 载荷
//! 数组，时间是逻辑 `tick: u64`。本单是 F1901（保存区裁断布局）的策略侧
//! 消费者，是 F1903（深度状态摊销保存）的上游接口提供者。
//!
//! ## 与册内其他单的显式对齐点
//! - **F1827 浮点使能位纪律（本单核心上游）**：F1827 定「延迟使能」——
//!   浮点单元不是切内即开，而是切出后置 CR0.TS，首次真正触碰时由 #NM
//!   代偿开路。本单把该纪律落到决策层：TS 何时置、#NM 由谁消费、消费后
//!   必须清 TS，三动作由 `TsCell` 状态机保证闭环。
//! - **F1901 XSAVE 区按 XCR0 位图逐位裁断布局**：`Region::xcr0_bit()` 与
//!   `Region::aligned_size()` 沿用 F1901 逐位裁断口径，XCR0 未置位的区域
//!   不占保存区，本单 `enabled` 即该裁断结果的镜像。
//! - **F1824 FPU 切换策略（默认 eager / lazy 预留）**：`Lazy` 与
//!   `AlwaysOnSwitchOut` 是 F1824 已启用开关的落地；`Never` 是 F1824 未
//!   启用时的诚实占位（承诺全程不触碰浮点）。
//! - **F1825 FPU 保存恢复原语**：`save()` / `restore()` 是 fxsave64 /
//!   fxrstor64 的语义替身，含对齐断言与往返保真断言。
//! - **F1826 SIMD 向量状态**：`Region::Sse` / `Region::Avx` 的载荷宽度与
//!   F1826 的 XMM / YMM 一一对应。
//! - **F1805 切出路径 / F1806 切入路径**：`switch_out()` / `switch_in()`
//!   是两条路径在本域内的收窄实现，只负责扩展状态侧。
//! - **F1903 AVX-512 深度状态摊销保存**：本单留 `AmortWindow` 接口与账，
//!   窗内延迟落盘与窗闭强制落盘由 F1903 接管。
//! - **F1908 扩展状态账**：本单产出
//!   `save_count == restore_count + lazy_pending_count（+ evicted_count）`
//!   的守恒式，由 F1908 汇总对账。
//!
//! ## 扩展状态区与描述符布局（文件头声明，本单契约）
//! ```text
//! 描述符（每线程一份，静态区，非保存区本体，共 EXTDESC_LEN = 0x28 字节）：
//!   +0x00  magic        u64  = EXTDESC_MAGIC
//!   +0x08  version      u64  = V1902_VERSION_ID
//!   +0x10  xcr0         u64  XCR0 位图模拟（x87/SSE/AVX/AVX-512 使能）
//!   +0x18  cr0          u64  CR0 快照（仅 TS 位有意义）
//!   +0x20  ts_state     u64  TsState 闭集序号
//!
//! RegionDesc（每区 REGION_DESC_LEN = 0x40 = 64 字节，区内亦 64 对齐）：
//!   +0x00  enabled      u64  来自 XCR0 位图（镜像 F1901 裁断结果）
//!   +0x08  dirty        u64  0 / 1 脏标记
//!   +0x10  cleanliness  u64  Cleanliness 闭集序号
//!   +0x18  save_off     u64  保存区偏移（相对保存区基址，必 64 对齐）
//!   +0x20  save_size    u64  保存区字节数（必 64 对齐）
//!   +0x28  align        u64  对齐要求，本单恒为 SAVE_ALIGN = 64
//!   +0x30  policy       u64  SavePolicy 闭集序号
//!   +0x38  payload_words u64  活动载荷字数（各区不同）
//!
//! 保存区本体（每线程一片，按 save_off 逐区排布，基址 64 对齐）：
//!   x87     区 [ 基础区  64B | 预留 0 ]
//!   SSE     区 [ XMM 组 128B | 预留 0 ]
//!   AVX     区 [ XMM 组 128B | YMM 组 128B ]
//!   AVX-512 区 [ XMM 组 128B | YMM 组 128B | ZMM 组 256B ]
//!   各区占位 = aligned_size(Region)，即 payload_words*8 向上取整到 64。
//!
//! 布局不变量（layout_check 逐条断言）：
//!   L1 save_off % 64 == 0；L2 save_size % 64 == 0；
//!   L3 save_size == aligned_size(region)；
//!   L4 同线程相邻使能区 save_off 无重叠且首区 off == 0；
//!   L5 未使能区（enabled = 0）不参与排布（size = 0、off = 0）。
//! ```
//!
//! ## 性能逐项分解（模型侧成本，非真实硬件时序）
//! - **切出一次** `switch_out`：`O(REGION_COUNT)` = 4 次决策表查询 +
//!   至多 4 次 `save()`（各 `O(payload_words)` 拷贝）+ 1 次 TS 迁移。
//! - **切入一次** `switch_in`：`O(REGION_COUNT)` 判定 + 至多 4 次
//!   `restore()`（`O(payload_words)` 指纹对拍）；缺区拒绝 `O(1)` 短路。
//! - **一次 #NM 陷入** `enter_nm`：`O(pending_records)` ≤ `O(REGION_COUNT)`，
//!   含 pending 扫描 + 保存 + 指纹对拍 + TS 双迁移 + 重启计数自增，
//!   恒定小常数，无循环上界风险。
//! - **注入核十形态**：造证 `O(1)` 定向，检出 `O(FORM_COUNT + threads×regions)`。
//! - **观测导出**：`export_kv` `O(metric_count)`；`export_tlv`
//!   `O(metric_count + FORM_COUNT)`，附一遍长度回读自洽校验 `tlv_len_ok`。
//! - **决策表全穷举**：`3×2×2 = 12` 格构造期一次性求值并冻结；运行期
//!   `decide()` 是纯 `match`，可落成跳转表，无查表开销。
//! - **往返保真样本矩阵**：`ROUNDTRIP_SAMPLES = 16` 组，构造期一次性核验。
//!
//! ## 诊断码段
//! 独占段 `0xB140 ~ 0xB14F`（16 码），主码 `0xB140`：
//! - `0xB140` 凭空 #NM（无待存惰性记录）
//! - `0xB141` #NM 重入
//! - `0xB142` 陷阱后未清 TS
//! - `0xB143` 惰性记录未被消费（闭环断裂）
//! - `0xB144` Never 策略却执行保存
//! - `0xB145` AlwaysOnSwitchOut 却跳过保存
//! - `0xB146` 切入缺有效保存区仍放行
//! - `0xB147` 保存区未 64 字节对齐
//! - `0xB148` 往返保真破坏
//! - `0xB149` 扩展状态账不守恒
//! - `0xB14A` 线程未登记 / 属主错配
//! - `0xB14B` 序号越界
//! - `0xB14C` 策略赋值非法
//! - `0xB14D` 摊销窗状态机非法迁移
//! - `0xB14E` TS 状态机非法迁移
//! - `0xB14F` 布局不变量破坏
//!
//! ## 纯功能线
//! `no_std + alloc`，零 `unwrap` / `expect` / `panic!`、零浮点、零真实 IO、
//! 零墙钟；确定性 LCG（`LCG_A = 6364136223846793005`、
//! `LCG_C = 1442695040888963407`、模 `2^64`）保证同种子同轨迹；自定义
//! `LzResult<T>`；无 `#[cfg(test)]`。
// 本单为「册内自足模块」：在探针 crate 中以私有 `mod m;` 引入时，全部
// `pub` 项都会被判为未可达；这是引入方式使然，不是本单死代码。
#![allow(dead_code)] extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
// ===========================================================================
// 0. 常量、标量与确定性 LCG
// ===========================================================================
/// 模块版本标识（写入描述符 +0x08）。
pub const V1902_VERSION_ID: u64 = 0x0001_9020_0000_0001;
/// 模块版本串。
pub const V1902_VERSION: &str = "v1902-lazysave-ts0";
/// 诊断码段基址。
pub const V1902_SEG_BASE: u16 = 0xB140;
/// 诊断码段上界（独占，含）。
pub const V1902_SEG_END: u16 = 0xB14F;
/// 诊断主码。
pub const V1902_MAIN_CODE: u16 = 0xB140;
/// 描述符魔数。
pub const EXTDESC_MAGIC: u64 = 0x5645_5853_3232_4445;
/// 描述符总长（字节）。
pub const EXTDESC_LEN: usize = 0x28;
/// 单区描述符长（字节），恒 64 对齐。
pub const REGION_DESC_LEN: usize = 0x40;
/// 保存区对齐要求（fxsave64 要求 64 字节对齐）。
pub const SAVE_ALIGN: usize = 64;
/// CR0 第 3 位：Task Switched（延迟使能 / 脏保存标志）。
pub const CR0_TS: u64 = 1 << 3;
/// #NM（Device Not Available）中断向量号。
pub const NM_VECTOR: u8 = 19;
/// 扩展状态区种类数。
pub const REGION_COUNT: usize = 4;
/// 注入形态数（判据十形态）。
pub const FORM_COUNT: usize = 10;
/// 策略 × 脏 × 使能 判定表格数（3×2×2）。
pub const MATRIX_CELLS: usize = 12;
/// 三态逐例判定用例数（判据 10/10）。
pub const POLICY_CASE_COUNT: usize = 10;
/// 往返保真样本矩阵规模。
pub const ROUNDTRIP_SAMPLES: usize = 16;
/// 惰性记录陈旧阈值（逻辑 tick），超过即闭环断裂候选。
pub const LAZY_STALE_TICKS: u64 = 64;
/// LCG 乘数（标准 64 位 LCG 常数）。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量。
pub const LCG_C: u64 = 1442695040888963407;
/// 确定性 LCG 发生器（模 2^64 溢出即回绕）。
#[derive(Clone, Copy, Debug)] pub struct Lcg {
    state: u64,
}
impl Lcg {
/// 以给定种子构造。
    pub fn new(seed: u64) -> Lcg { Lcg { state: seed } }
/// 当前内部状态（只读，便于轨迹断言）。
    pub fn raw(&self) -> u64 {
        self.state }
/// 取下一个 u64（标准 64 位 LCG 一步）。
    pub fn next_u64(&mut self) -> u64 {
        self.state = lcg_step(self.state);
        self.state }
/// 取 `[0, n)` 内值；n = 0 时返回 0。
    pub fn below(&mut self, n: usize) -> usize {
if n == 0 { return 0; }
        (self.next_u64() % (n as u64)) as usize }
}
/// 与 `Lcg` 等价的纯函数式步进（注入核可自由复现轨迹）。
pub fn lcg_step(state: u64) -> u64 {
    state.wrapping_mul(LCG_A).wrapping_add(LCG_C) }
/// 32 位 FNV 变体混合（把字节序列折叠成 u32）。
pub fn mix32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
for b in bytes { h ^= *b as u32; h = h.wrapping_mul(0x0100_0193); h ^= h >> 13; }
    h }
/// 64 位混合指纹（把 u64 载荷折叠成 u64）。
pub fn mix64(words: &[u64]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
for w in words { h ^= *w; h = h.wrapping_mul(0x0000_0100_0000_01b3); h ^= h >> 29; }
    h }
// ===========================================================================
// 1. 诊断码与错误类型
// ===========================================================================
/// 十形态注入 / 检出码（`0xB140 ~ 0xB149`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum Form {
/// 凭空 #NM：陷入时无任何待存惰性记录。
    SpuriousTrap,
/// #NM 重入：陷阱处理中又来 #NM。
    TrapReentry,
/// 陷阱返回后 TS 未清零。
    TsNotCleared,
/// 惰性记录未被消费，闭环断裂。
    LazyRecordUnconsumed,
/// Never 策略却执行了保存。
    SaveOnNever,
/// AlwaysOnSwitchOut 却跳过保存。
    SkipOnAlways,
/// 切入时缺有效保存区仍被放行。
    MissingAreaAccepted,
/// 保存区未 64 字节对齐。
    MisalignedSaveArea,
/// 往返保真破坏（restore 后字段不符）。
    RoundTripBroken,
/// 扩展状态账不守恒。
    LedgerUnbalanced,
}
impl Form {
/// 全部形态（判据顺序）。
    pub const ALL: [Form; FORM_COUNT] = [ Form::SpuriousTrap, Form::TrapReentry, Form::TsNotCleared, Form::LazyRecordUnconsumed, Form::SaveOnNever,
        Form::SkipOnAlways, Form::MissingAreaAccepted, Form::MisalignedSaveArea, Form::RoundTripBroken, Form::LedgerUnbalanced, ];
/// 序号（0..9）。
    pub fn index(self) -> usize {
        match self {
            Form::SpuriousTrap => 0, Form::TrapReentry => 1, Form::TsNotCleared => 2, Form::LazyRecordUnconsumed => 3, Form::SaveOnNever => 4,
            Form::SkipOnAlways => 5, Form::MissingAreaAccepted => 6, Form::MisalignedSaveArea => 7, Form::RoundTripBroken => 8,
            Form::LedgerUnbalanced => 9,
        }
    }
/// 诊断码（独占段内连续）。
    pub fn code(self) -> u16 {
        V1902_SEG_BASE + (self.index() as u16) }
/// 形态短名。
    pub fn name(self) -> &'static str {
        match self {
            Form::SpuriousTrap => "spurious_nm", Form::TrapReentry => "nm_reentry", Form::TsNotCleared => "ts_not_cleared",
            Form::LazyRecordUnconsumed => "lazy_unconsumed", Form::SaveOnNever => "save_on_never", Form::SkipOnAlways => "skip_on_always",
            Form::MissingAreaAccepted => "missing_area_accepted", Form::MisalignedSaveArea => "save_area_misaligned",
            Form::RoundTripBroken => "roundtrip_broken", Form::LedgerUnbalanced => "ledger_unbalanced",
        }
    }
/// 形态判读。
    pub fn describe(self) -> &'static str {
        match self {
            Form::SpuriousTrap => "凭空 #NM：陷入时无待存惰性记录", Form::TrapReentry => "#NM 重入：陷阱处理中又来 #NM", Form::TsNotCleared => "陷阱返回后 CR0.TS 未清零",
            Form::LazyRecordUnconsumed => "惰性记录未被消费，闭环断裂", Form::SaveOnNever => "Never 策略却执行了保存", Form::SkipOnAlways => "AlwaysOnSwitchOut 却跳过保存",
            Form::MissingAreaAccepted => "切入缺有效保存区仍放行", Form::MisalignedSaveArea => "保存区未 64 字节对齐", Form::RoundTripBroken => "往返保真破坏：restore 后字段不符",
            Form::LedgerUnbalanced => "扩展状态账不守恒",
        }
    }
}
/// 段内辅助诊断码（`0xB14A ~ 0xB14F`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum AuxCode {
/// `0xB14A` 线程未登记 / 属主错配。
    ThreadUnknown,
/// `0xB14B` 序号越界。
    IndexOutOfRange,
/// `0xB14C` 策略赋值非法。
    PolicyInvalid,
/// `0xB14D` 摊销窗非法迁移。
    WindowIllegal,
/// `0xB14E` TS 状态机非法迁移。
    TsIllegal,
/// `0xB14F` 布局不变量破坏。
    LayoutBroken,
}
impl AuxCode {
/// 诊断码。
    pub fn code(self) -> u16 {
        match self {
            AuxCode::ThreadUnknown => 0xB14A, AuxCode::IndexOutOfRange => 0xB14B, AuxCode::PolicyInvalid => 0xB14C, AuxCode::WindowIllegal => 0xB14D,
            AuxCode::TsIllegal => 0xB14E, AuxCode::LayoutBroken => 0xB14F,
        }
    }
/// 短名。
    pub fn name(self) -> &'static str {
        match self {
            AuxCode::ThreadUnknown => "thread_unknown", AuxCode::IndexOutOfRange => "index_out_of_range", AuxCode::PolicyInvalid => "policy_invalid",
            AuxCode::WindowIllegal => "window_illegal", AuxCode::TsIllegal => "ts_illegal", AuxCode::LayoutBroken => "layout_broken",
        }
    }
}
/// 错误类型：诊断码 + 上下文细节。
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub struct LzErr {
/// 诊断码（独占段内）。
    pub code: u16,
/// 上下文细节（线程号 / 序号 / tick 等）。
    pub detail: u64,
}
impl LzErr {
/// 构造错误。
    pub fn new(code: u16, detail: u64) -> LzErr { LzErr { code, detail } }
/// 由形态构造错误。
    pub fn of_form(f: Form, detail: u64) -> LzErr {
        LzErr::new(f.code(), detail) }
/// 由辅助码构造错误。
    pub fn of_aux(a: AuxCode, detail: u64) -> LzErr {
        LzErr::new(a.code(), detail) }
}
/// 本单 `Result`。
pub type LzResult<T> = Result<T, LzErr>;
/// 错误串（纯分配，无 IO）。
pub fn err_str(e: LzErr) -> String {
    format!("0x{:04X}/{}", e.code, e.detail) }
// ===========================================================================
// 2. 区域闭集 / 清洁度闭集 / 三态策略 / 决策函数
// ===========================================================================
/// 扩展状态区种类（承 F1901 逐位裁断布局）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum Region {
/// x87 基础区（F1825 原语主承载）。
    X87,
/// SSE / XMM 区（F1826）。
    Sse,
/// AVX / YMM 区（F1826 + F1824）。
    Avx,
/// AVX-512 / ZMM 区（F1903 摊销保存对象）。
    Avx512,
}
impl Region {
/// 全部区域。
    pub const ALL: [Region; REGION_COUNT] = [Region::X87, Region::Sse, Region::Avx, Region::Avx512];
/// 序号。
pub fn index(self) -> usize { match self { Region::X87 => 0, Region::Sse => 1, Region::Avx => 2, Region::Avx512 => 3, } }
/// 由序号还原；越界返回 `None`（不 panic）。
    pub fn from_index(i: usize) -> Option<Region> {
match i { 0 => Some(Region::X87), 1 => Some(Region::Sse), 2 => Some(Region::Avx), 3 => Some(Region::Avx512), _ => None, }
    }
/// XCR0 位图位号（承 F1901 逐位裁断）。
pub fn xcr0_bit(self) -> u64 { match self { Region::X87 => 1 << 0, Region::Sse => 1 << 1, Region::Avx => 1 << 2, Region::Avx512 => 1 << 5, } }
/// 区域短名。
pub fn name(self) -> &'static str { match self { Region::X87 => "x87", Region::Sse => "sse", Region::Avx => "avx", Region::Avx512 => "avx512", } }
/// 活动载荷字数（各区宽度不同，承 F1826 的 XMM/YMM 分层）。
pub fn payload_words(self) -> usize { match self { Region::X87 => 8, Region::Sse => 16, Region::Avx => 32, Region::Avx512 => 64, } }
/// 保存区占位字节数（向上取整到 64 对齐）。
    pub fn aligned_size(self) -> usize {
        let raw = self.payload_words() * 8;
        let rem = raw % SAVE_ALIGN;
        if rem == 0 {
            raw } else {
            raw + (SAVE_ALIGN - rem) }
    }
/// 是否为深度区（F1903 摊销对象）。
    pub fn is_deep(self) -> bool {
        matches!(self, Region::Avx512) }
}
/// 状态区清洁度闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum Cleanliness {
/// 未被写入的净状态。
    Clean,
/// 活动状态被写入，尚未落盘。
    Dirty,
/// 已落入保存区。
    Saved,
/// 已登记惰性挂起，尚未落盘（等待首次使用触发）。
    LazyPending,
/// 已从保存区恢复回来。
    Restored,
}
impl Cleanliness {
/// 全部状态。
    pub const ALL: [Cleanliness; 5] = [ Cleanliness::Clean, Cleanliness::Dirty, Cleanliness::Saved, Cleanliness::LazyPending, Cleanliness::Restored,
    ];
/// 闭集序号。
    pub fn index(self) -> usize {
match self { Cleanliness::Clean => 0, Cleanliness::Dirty => 1, Cleanliness::Saved => 2, Cleanliness::LazyPending => 3, Cleanliness::Restored => 4, }
    }
/// 短名。
    pub fn name(self) -> &'static str {
        match self {
            Cleanliness::Clean => "clean", Cleanliness::Dirty => "dirty", Cleanliness::Saved => "saved", Cleanliness::LazyPending => "lazy_pending",
            Cleanliness::Restored => "restored",
        }
    }
}
/// 保存时机三态（判据核心：切出必存 / 惰性 / 永存）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum SavePolicy {
/// 切出必存：出队即落保存区，脏则必存、不脏也存（策略从严）。
    AlwaysOnSwitchOut,
/// 惰性：切出时若脏则不存，只置 CR0.TS 并登记 LazyPending， 保存推迟到首次使用（#NM 陷阱代偿）。
    Lazy,
/// 永存：从不保存（进程承诺全程不触碰该区），切入时若 XCR0 使能位 要求该区状态则必须已有可用保存区，否则拒绝切入（不静默）。
    Never,
}
impl SavePolicy {
/// 全部策略（三态闭集，序号即判定表行序）。
    pub const ALL: [SavePolicy; 3] = [ SavePolicy::AlwaysOnSwitchOut, SavePolicy::Lazy, SavePolicy::Never, ];
/// 序号（0..2）。
pub fn index(self) -> usize { match self { SavePolicy::AlwaysOnSwitchOut => 0, SavePolicy::Lazy => 1, SavePolicy::Never => 2, } }
/// 短名。
    pub fn name(self) -> &'static str {
match self { SavePolicy::AlwaysOnSwitchOut => "always_on_switch_out", SavePolicy::Lazy => "lazy", SavePolicy::Never => "never", }
    }
/// 是否以 #NM 陷阱为保存触发路径。
    pub fn uses_trap(self) -> bool {
        matches!(self, SavePolicy::Lazy) }
/// 是否必须保证保存区有效（Never 不然）。
    pub fn requires_area(self) -> bool {
        !matches!(self, SavePolicy::Never) }
/// 由序号还原；越界返回 `None`。
    pub fn from_index(i: usize) -> Option<SavePolicy> {
match i { 0 => Some(SavePolicy::AlwaysOnSwitchOut), 1 => Some(SavePolicy::Lazy), 2 => Some(SavePolicy::Never), _ => None, }
    }
}
/// 切出时刻的动作闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum Action {
/// 立即落盘。
    SaveNow,
/// 推迟到首次使用（置 TS + 登记 LazyPending）。
    DeferToLazy,
/// 不保存（永存或净跳过）。
    Skip,
}
impl Action {
/// 全部动作。
    pub const ALL: [Action; 3] = [Action::SaveNow, Action::DeferToLazy, Action::Skip];
/// 序号。
pub fn index(self) -> usize { match self { Action::SaveNow => 0, Action::DeferToLazy => 1, Action::Skip => 2, } }
/// 短名。
pub fn name(self) -> &'static str { match self { Action::SaveNow => "save_now", Action::DeferToLazy => "defer_to_lazy", Action::Skip => "skip", } }
}
/// 决策函数：策略 × 脏标记 × XCR0 使能位 → 切出动作。 语义（判据 UNX-F1902-J1 子句「保存时机三态逐态正确」）： - `AlwaysOnSwitchOut`：恒 `SaveNow`（不脏也存，策略从严）。 - `Lazy`：脏 → `DeferToLazy`（切出不存，只置 TS + 登记）；净 → `Skip`。 - `Never`：恒 `Skip`（承诺全程不触碰，任何保存都是纯浪费）。 使能位不改变动作（`Never` 的使能位冲突由切入侧拒绝兜底，不在切出侧 偷偷升格为保存——那会让三态退化成四态）。 判据：UNX-F1902-J1 —— 决策函数是三态唯一的动作来源。
pub fn decide(policy: SavePolicy, dirty: bool, _enabled: bool) -> Action {
    match policy {
        SavePolicy::AlwaysOnSwitchOut => Action::SaveNow, SavePolicy::Lazy => {
            if dirty {
                Action::DeferToLazy } else {
                Action::Skip }
        }
        SavePolicy::Never => Action::Skip,
    }
}
// ===========================================================================
// 3. 判定表（3×2×2 全穷举）
// ===========================================================================
/// 判定表一格：策略 × 脏 × 使能 → 期望动作 + 实际动作 + 是否通过。
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub struct MatrixCell {
/// 策略。
    pub policy: SavePolicy,
/// 脏标记输入。
    pub dirty: bool,
/// XCR0 使能位输入。
    pub enabled: bool,
/// 期望动作（人工写死的判据期望）。
    pub expect: Action,
/// 实际动作（由 `decide()` 求值）。
    pub actual: Action,
/// 判定结果。
    pub pass: bool,
}
impl MatrixCell {
/// 表格行串（观测用）。
    pub fn row(&self) -> String {
        format!( "{}|dirty={}|en={}|expect={}|actual={}|{}", self.policy.name(), if self.dirty { 1 } else { 0 }, if self.enabled { 1 } else { 0 },
            self.expect.name(), self.actual.name(), if self.pass { "PASS" } else { "FAIL" }
        ) }
}
/// 构造判定表一格的期望值（**独立于 `decide()` 的第二实现**， 用于交叉校验：若二者同源则判定表永真，测试即失效）。
pub fn expect_action(policy: SavePolicy, dirty: bool, _enabled: bool) -> Action {
    match policy {
        // 从严：不脏也存。
        SavePolicy::AlwaysOnSwitchOut => Action::SaveNow,
        // 惰性：脏则推迟，净则跳过。
        SavePolicy::Lazy => {
            if dirty {
                Action::DeferToLazy } else {
                Action::Skip }
        }
        // 永存：从不保存。
        SavePolicy::Never => Action::Skip,
    }
}
/// 全穷举判定表（12 格）。 判据：UNX-F1902-J1 —— 「保存时机三态逐态正确」，本表是逐态正确性的 机械化底座，12/12 通过方可宣告三态判定闭合。
pub fn decision_matrix() -> Vec<MatrixCell> {
    let mut out: Vec<MatrixCell> = Vec::new();
    for policy in SavePolicy::ALL {
        for dirty in [false, true] {
            for enabled in [false, true] {
                let expect = expect_action(policy, dirty, enabled);
                let actual = decide(policy, dirty, enabled);
                out.push(MatrixCell {
                    policy, dirty, enabled, expect, actual, pass: expect == actual,
                });
            }
        }
    }
    out }
/// 判定表全过（12/12）。
pub fn decision_matrix_ok() -> bool {
    let m = decision_matrix();
if m.len() != MATRIX_CELLS { return false; }
    let mut n = 0usize;
for c in m.iter() { if c.pass { n = n.wrapping_add(1); } }
    n == MATRIX_CELLS }
/// 判定表按策略分组的通过计数（每态 4 格）。
pub fn matrix_by_policy() -> [usize; 3] {
    let mut out = [0usize; 3];
for c in decision_matrix().iter() { if c.pass { out[c.policy.index()] = out[c.policy.index()].wrapping_add(1); } }
    out }
// ===========================================================================
// 4. 扩展状态区建模（承 F1901）
// ===========================================================================
/// 扩展状态区（单区单线程）。 字段与文件头布局声明一一对应；`save_off` / `save_size` / `align` 三元组构成该区的保存区描述符投影。
#[derive(Clone, Debug)] pub struct ExtState {
/// 区域种类。
    pub region: Region,
/// XCR0 使能位镜像（F1901 裁断结果）。
    pub enabled: bool,
/// 脏标记。
    pub dirty: bool,
/// 清洁度闭集。
    pub cleanliness: Cleanliness,
/// 保存区偏移（相对保存区基址，必 64 对齐；未使能区为 0）。
    pub save_off: usize,
/// 保存区字节数（必 64 对齐；未使能区为 0）。
    pub save_size: usize,
/// 对齐要求（恒 `SAVE_ALIGN`）。
    pub align: usize,
/// 活动载荷（`payload_words` 个 u64）。
    pub payload: Vec<u64>,
/// 触碰计数（本单用作"脏的产生次数"证据）。
    pub touches: u64,
}
impl ExtState {
/// 构造一个未使能、净状态的空区。
    pub fn blank(region: Region, enabled: bool) -> ExtState {
        ExtState {
            region, enabled, dirty: false, cleanliness: Cleanliness::Clean, save_off: 0, save_size: 0, align: SAVE_ALIGN,
            payload: vec![0u64; region.payload_words()], touches: 0,
        }
    }
/// 按使能位排布保存区（承 F1901：仅使能区占位，逐区首尾相接）。
    pub fn layout(&mut self, off: usize) -> usize {
        if self.enabled {
            self.save_off = off;
            self.save_size = self.region.aligned_size();
            self.save_size } else {
            self.save_off = 0;
            self.save_size = 0;
            0 }
    }
/// 载荷指纹（往返保真比对键）。
    pub fn fingerprint(&self) -> u64 {
        mix64(&self.payload) }
/// 标脏并推进触碰计数。
    pub fn touch(&mut self, rng: &mut Lcg) {
        let idx = rng.below(self.payload.len());
if let Some(cell) = self.payload.get_mut(idx) { *cell = rng.next_u64(); }
        self.touches = self.touches.wrapping_add(1);
        self.dirty = true;
        self.cleanliness = Cleanliness::Dirty;
    }
/// 以种子填充确定性载荷（用于样本矩阵）。
    pub fn seed_payload(&mut self, seed: u64) {
        let mut r = Lcg::new(seed);
for i in 0..self.payload.len() { if let Some(cell) = self.payload.get_mut(i) { *cell = r.next_u64(); } }
    }
/// 布局不变量自检（L1/L2/L3/L5）。
    pub fn layout_ok(&self) -> bool {
if self.align != SAVE_ALIGN { return false; }
if !self.enabled { return self.save_off == 0 && self.save_size == 0; }
if self.save_off % SAVE_ALIGN != 0 { return false; }
if self.save_size % SAVE_ALIGN != 0 { return false; }
if self.save_size != self.region.aligned_size() { return false; }
        true }
/// 一行描述（观测用）。
    pub fn describe(&self) -> String {
        format!( "{}:en={}dirty={}cl={}off=0x{:X}sz={}al={}fp=0x{:016X}", self.region.name(), if self.enabled { 1 } else { 0 },
            if self.dirty { 1 } else { 0 }, self.cleanliness.name(), self.save_off, self.save_size, self.align, self.fingerprint() ) }
}
/// 描述符字节序编码（`EXTDESC_LEN` 字节，小端 u64 排列）。 布局见文件头；这是本单对外的**唯一**描述符真值来源，观测导出复用它。
pub fn encode_desc(xcr0: u64, cr0: u64, ts_state: u64) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::with_capacity(EXTDESC_LEN);
for w in [EXTDESC_MAGIC, V1902_VERSION_ID, xcr0, cr0, ts_state] { push_u64(&mut v, w); }
    v }
/// 区域描述符字节序编码（`REGION_DESC_LEN` 字节）。
pub fn encode_region_desc(st: &ExtState, policy: SavePolicy) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::with_capacity(REGION_DESC_LEN);
    let words = [ if st.enabled { 1u64 } else { 0u64 }, if st.dirty { 1u64 } else { 0u64 }, st.cleanliness.index() as u64, st.save_off as u64,
        st.save_size as u64, st.align as u64, policy.index() as u64, st.payload.len() as u64, ];
for w in words { push_u64(&mut v, w); }
    v }
/// 小端压入 u64。
pub fn push_u64(v: &mut Vec<u8>, w: u64) { for i in 0..8 { let byte = ((w >> (8 * i)) & 0xFF) as u8; v.push(byte); } }
/// 小端压入 u32。
pub fn push_u32(v: &mut Vec<u8>, w: u32) { for i in 0..4 { let byte = ((w >> (8 * i)) & 0xFF) as u8; v.push(byte); } }
/// 小端读回 u64。
pub fn read_u64(v: &[u8], off: usize) -> u64 {
    let mut w: u64 = 0;
    let mut i = 0usize;
while i < 8 { if let Some(b) = v.get(off + i) { w |= (*b as u64) << (8 * i); } i = i.wrapping_add(1); }
    w }
/// 小端读回 u32。
pub fn read_u32(v: &[u8], off: usize) -> u64 {
    let mut w: u64 = 0;
    let mut i = 0usize;
while i < 4 { if let Some(b) = v.get(off + i) { w |= (*b as u64) << (8 * i); } i = i.wrapping_add(1); }
    w }
/// 读回 TLV 长度字段（与 `push_tlv` 的「大端 2 字节」写法对称）。
pub fn read_u16(v: &[u8], off: usize) -> u64 {
    let mut w: u64 = 0;
    let mut i = 0usize;
while i < 2 { if let Some(b) = v.get(off + i) { w |= (*b as u64) << (8 * (1 - i)); } i = i.wrapping_add(1); }
    w }
// ===========================================================================
// 5. 保存原语（模拟 fxsave64 / fxrstor64）
// ===========================================================================
/// 保存区（单区单线程的落盘体）。
#[derive(Clone, Debug)] pub struct SaveArea {
/// 区偏移（对齐断言对象）。
    pub off: usize,
/// 区字节数。
    pub size: usize,
/// 对齐要求（构造时可能被故意破坏以注入形态 8）。
    pub align: usize,
/// 保存内容（`size/8` 个 u64）。
    pub words: Vec<u64>,
/// 是否持有有效内容。
    pub valid: bool,
/// 保存时的逻辑 tick。
    pub stamp: u64,
/// 保存代数（每次 save 递增，用于往返对拍）。
    pub generation: u64,
/// `save()` 落盘时写入的**期望指纹**（restore 时与之对拍）。
    pub expect_fingerprint: u64,
}
impl SaveArea {
/// 构造与 `ExtState` 匹配的保存区。
    pub fn for_state(st: &ExtState) -> SaveArea {
        let n = if st.save_size == 0 {
            0 } else {
            st.save_size / 8 };
        SaveArea {
            off: st.save_off, size: st.save_size, align: SAVE_ALIGN, words: vec![0u64; n], valid: false, stamp: 0, generation: 0,
            expect_fingerprint: 0,
        }
    }
/// 对齐是否合规（fxsave64 的 64 字节对齐硬要求）。
    pub fn aligned_ok(&self) -> bool {
        self.align == SAVE_ALIGN && self.off % SAVE_ALIGN == 0 && self.size % SAVE_ALIGN == 0 }
/// 保存内容指纹。
    pub fn fingerprint(&self) -> u64 {
        mix64(&self.words) }
}
/// 保存票据（一次 `save()` 的凭据）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub struct SaveTicket {
/// 区序号。
    pub region: usize,
/// 保存区偏移。
    pub off: usize,
/// 保存区字节数。
    pub size: usize,
/// 保存代数。
    pub generation: u64,
/// 保存时逻辑 tick。
    pub tick: u64,
/// 保存内容指纹。
    pub fingerprint: u64,
}
impl SaveTicket {
/// 票据串（观测用）。
    pub fn describe(&self) -> String {
        format!( "ticket#{}off=0x{:X}sz={}gen={}tick={}fp=0x{:016X}",
            self.region, self.off, self.size, self.generation, self.tick, self.fingerprint ) }
}
/// 保存原语：`fxsave64` 语义替身。 断言链（按序，任一失败即返回 `Err`）： 1. 区必须使能（未使能区不占保存区，无从保存）；否则 `0xB14F`。 2. 保存区必须 64 字节对齐（`off` / `size` / `align`）；否则 `0xB147`。 3. 保存区容量必须容纳载荷；否则 `0xB14F`。 判据：UNX-F1902-J1 —— 保存是三态里唯一被允许落盘的出口。
pub fn save(st: &mut ExtState, area: &mut SaveArea, tick: u64) -> LzResult<SaveTicket> {
if !st.enabled { return Err(LzErr::of_aux(AuxCode::LayoutBroken, st.region.index() as u64)); }
if !area.aligned_ok() { return Err(LzErr::of_form(Form::MisalignedSaveArea, st.region.index() as u64)); }
if area.words.len() * 8 < st.payload.len() * 8 { return Err(LzErr::of_aux(AuxCode::LayoutBroken, st.region.index() as u64)); }
    let mut i = 0usize;
    while i < st.payload.len() {
        let w = st.payload.get(i).copied().unwrap_or(0);
if let Some(cell) = area.words.get_mut(i) { *cell = w; }
        i = i.wrapping_add(1);
    }
    area.valid = true;
    area.stamp = tick;
    area.generation = area.generation.wrapping_add(1);
    area.expect_fingerprint = area.fingerprint();
    st.dirty = false;
    st.cleanliness = Cleanliness::Saved;
    Ok(SaveTicket {
        region: st.region.index(), off: area.off, size: area.size, generation: area.generation, tick, fingerprint: area.fingerprint(),
    }) }
/// 恢复原语：`fxrstor64` 语义替身。 断言链： 1. 保存区必须有效；否则 `0xB146`（切入缺区）。 2. 保存区必须 64 字节对齐；否则 `0xB147`。 3. 载荷长度必须与活动区一致；否则 `0xB148`。 4. **落盘期望指纹必须与保存区当前指纹逐位相等**；否则 `0xB148` （保存区在中途被改写 = 往返保真破坏）。 判据：UNX-F1902-J1 —— 恢复后 `restored` 且脏位清零。
pub fn restore(area: &SaveArea, st: &mut ExtState, tick: u64) -> LzResult<u64> {
if !area.valid { return Err(LzErr::of_form(Form::MissingAreaAccepted, st.region.index() as u64)); }
if !area.aligned_ok() { return Err(LzErr::of_form(Form::MisalignedSaveArea, st.region.index() as u64)); }
if area.words.len() != st.payload.len() { return Err(LzErr::of_form( Form::RoundTripBroken, st.region.index() as u64, )); }
    // 反篡改前置校验：保存区内容必须仍是 save() 写入的那一份。
    let before = area.fingerprint();
if area.expect_fingerprint != before { return Err(LzErr::of_form( Form::RoundTripBroken, st.region.index() as u64, )); }
    let mut i = 0usize;
    while i < st.payload.len() {
        let w = area.words.get(i).copied().unwrap_or(0);
if let Some(cell) = st.payload.get_mut(i) { *cell = w; }
        i = i.wrapping_add(1);
    }
if st.fingerprint() != before { return Err(LzErr::of_form( Form::RoundTripBroken, st.region.index() as u64, )); }
    st.dirty = false;
    st.cleanliness = Cleanliness::Restored;
    // tick 仅参与票据对齐语义，避免未使用告警的同时保留可追溯性。
    let _ = tick;
    Ok(before) }
/// 就地保存（`save()` 的槽位版：借用已解约，可直接改 `s.state` / `s.area`）。
pub fn save_clone(st: &mut ExtState, area: &mut SaveArea, tick: u64) -> LzResult<SaveTicket> {
    save(st, area, tick) }
/// 就地恢复（`restore()` 的槽位版）。
pub fn restore_clone(area: &SaveArea, st: &mut ExtState, tick: u64) -> LzResult<u64> {
    restore(area, st, tick) }
/// 往返保真单样本结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub struct RoundTrip {
/// 样本序号。
    pub sample: usize,
/// 区域。
    pub region: Region,
/// save 是否成功。
    pub saved: bool,
/// restore 是否成功。
    pub restored: bool,
/// 逐字段比对零差（载荷 + 指纹 + 代数 + 清洁度）。
    pub zero_diff: bool,
}
/// 往返保真比对：save → 改脏 → restore → 逐字段比对。
pub fn roundtrip_once(sample: usize, seed: u64) -> RoundTrip {
    let region = match Region::from_index(sample % REGION_COUNT) {
        Some(r) => r, None => Region::X87,
    };
    let mut st = ExtState::blank(region, true);
    st.layout(0);
    st.seed_payload(seed);
    let mut rng = Lcg::new(seed ^ 0xA5A5_A5A5_A5A5_A5A5);
    st.touch(&mut rng);
    let mut area = SaveArea::for_state(&st);
    let saved = save(&mut st, &mut area, 1).is_ok();
    // 破坏现场：再改若干次脏，确保 restore 确实在纠正状态。
for _ in 0..3 { st.touch(&mut rng); }
    let restored = restore(&area, &mut st, 2).is_ok();
    let zero_diff = area.valid && st.fingerprint() == area.fingerprint() && !st.dirty
        && st.cleanliness == Cleanliness::Restored && st.payload.len() == region.payload_words();
    RoundTrip {
        sample, region, saved, restored, zero_diff,
    }
}
/// 往返保真样本矩阵（`ROUNDTRIP_SAMPLES` 组）。
pub fn roundtrip_matrix(seed: u64) -> Vec<RoundTrip> {
    let mut out: Vec<RoundTrip> = Vec::new();
for i in 0..ROUNDTRIP_SAMPLES { out.push(roundtrip_once(i, lcg_step(seed.wrapping_add(i as u64)))); }
    out }
/// 往返保真矩阵全过。
pub fn roundtrip_matrix_ok(seed: u64) -> bool {
    let m = roundtrip_matrix(seed);
if m.len() != ROUNDTRIP_SAMPLES { return false; }
for r in m.iter() { if !(r.saved && r.restored && r.zero_diff) { return false; } }
    true }
// ===========================================================================
// 6. CR0.TS 延迟使能状态机
// ===========================================================================
/// TS 状态闭集：`Clear → Set → Trap → Restore → Clear`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum TsState {
/// TS=0，FPU 可用（浮点单元对当前线程已开路）。
    Clear,
/// TS=1，惰性挂起（切出置位，等待首次使用）。
    Set,
/// #NM 陷入处理中（重入禁令生效区间）。
    Trap,
/// 已执行 fxrstor，准备清 TS。
    Restore,
}
impl TsState {
/// 全部状态。
    pub const ALL: [TsState; 4] = [TsState::Clear, TsState::Set, TsState::Trap, TsState::Restore];
/// 序号。
pub fn index(self) -> usize { match self { TsState::Clear => 0, TsState::Set => 1, TsState::Trap => 2, TsState::Restore => 3, } }
/// 短名。
    pub fn name(self) -> &'static str {
match self { TsState::Clear => "clear", TsState::Set => "set", TsState::Trap => "trap", TsState::Restore => "restore", }
    }
/// 合法后继（状态机邻接表；其余迁移判非法 `0xB14E`）。
    pub fn next_allowed(self) -> &'static [TsState] {
        match self {
            // Clear → Set（切出置位）；Clear → Clear（空转幂等）
            TsState::Clear => &[TsState::Clear, TsState::Set],
            // Set → Trap（首次使用触发 #NM）；Set → Set（幂等重入拒绝）
            TsState::Set => &[TsState::Set, TsState::Trap],
            // Trap → Restore（完成保存/恢复）；Trap → Trap（重入，禁止）
            TsState::Trap => &[TsState::Restore],
            // Restore → Clear（清 TS 闭环）
            TsState::Restore => &[TsState::Clear],
        }
    }
/// 迁移是否合法。
    pub fn can_go(self, next: TsState) -> bool {
for c in self.next_allowed() { if *c == next { return true; } }
        false }
}
/// TS 单元（每线程一份）。
#[derive(Clone, Debug)] pub struct TsCell {
/// 当前状态。
    pub state: TsState,
/// 置位时的 tick。
    pub set_tick: u64,
/// 陷入时的 tick。
    pub trap_tick: u64,
/// 清位时的 tick。
    pub clear_tick: u64,
/// 置位次数。
    pub set_count: u64,
/// 陷入次数。
    pub trap_count: u64,
/// 闭环完成次数（Restore→Clear）。
    pub close_count: u64,
}
impl TsCell {
/// 初态：Clear。
pub fn new() -> TsCell { TsCell { state: TsState::Clear, set_tick: 0, trap_tick: 0, clear_tick: 0, set_count: 0, trap_count: 0, close_count: 0, } }
/// TS 位投影（`CR0_TS` 掩码）。
pub fn bit(&self) -> u64 { if self.state == TsState::Clear { 0 } else { CR0_TS } }
/// 合法迁移。
    pub fn go(&mut self, next: TsState, tick: u64) -> LzResult<()> {
if !self.state.can_go(next) { return Err(LzErr::of_aux(AuxCode::TsIllegal, self.state.index() as u64)); }
        self.state = next;
        match next {
            TsState::Set => {
                self.set_tick = tick;
                self.set_count = self.set_count.wrapping_add(1);
            }
            TsState::Trap => {
                self.trap_tick = tick;
                self.trap_count = self.trap_count.wrapping_add(1);
            }
            TsState::Clear => {
                self.clear_tick = tick;
                self.close_count = self.close_count.wrapping_add(1);
            }
            TsState::Restore => {}
        }
        Ok(()) }
/// 一行描述。
    pub fn describe(&self) -> String {
        format!( "ts={}bit=0x{:X}set={}trap={}close={}", self.state.name(), self.bit(), self.set_count, self.trap_count, self.close_count ) }
}
/// 一次 #NM 陷入的逐段落账。
#[derive(Clone, Debug)] pub struct NmTrace {
/// 线程号。
    pub tid: u64,
/// 进入时的 TS 状态。
    pub enter_state: TsState,
/// 查到的待存区数（0 即凭空陷阱候选）。
    pub pending_found: usize,
/// 实际执行的保存次数。
    pub saves_done: usize,
/// 实际执行的恢复次数。
    pub restores_done: usize,
/// 出口 TS 状态。
    pub exit_state: TsState,
/// 被重执行的原指令次数（应为 1）。
    pub restart: u64,
/// 是否闭环通过。
    pub closed: bool,
}
impl NmTrace {
/// 一行描述。
    pub fn describe(&self) -> String {
        format!( "nm(tid=0x{:X}) enter={} found={} save={} restore={} exit={} restart={} {}", self.tid, self.enter_state.name(), self.pending_found,
            self.saves_done, self.restores_done, self.exit_state.name(), self.restart, if self.closed { "CLOSED" } else { "OPEN" }
        ) }
}
// ===========================================================================
// 7. 摊销窗接口（F1903 预告）
// ===========================================================================
/// 摊销窗状态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum WindowState {
/// 窗闭：任何保存都立即落盘。
    Closed,
/// 窗开：深度区允许延迟保存到窗闭。
    Open,
}
impl WindowState {
/// 全部状态。
    pub const ALL: [WindowState; 2] = [WindowState::Closed, WindowState::Open];
/// 序号。
pub fn index(self) -> usize { match self { WindowState::Closed => 0, WindowState::Open => 1, } }
/// 短名。
pub fn name(self) -> &'static str { match self { WindowState::Closed => "closed", WindowState::Open => "open", } }
}
/// 摊销窗（AVX-512 深度区专用；本单只留接口与账）。
#[derive(Clone, Debug)] pub struct AmortWindow {
/// 状态。
    pub state: WindowState,
/// 开窗 tick。
    pub opened_tick: u64,
/// 窗内累计延迟保存数（由 F1903 消费）。
    pub deferred_in_window: u64,
/// 窗闭强制落盘数（由 F1903 消费）。
    pub forced_flush: u64,
/// 开窗次数。
    pub open_count: u64,
}
impl AmortWindow {
/// 初态：窗闭。
pub fn new() -> AmortWindow { AmortWindow { state: WindowState::Closed, opened_tick: 0, deferred_in_window: 0, forced_flush: 0, open_count: 0, } }
/// 开窗（仅深度区可开；非深度区请求判非法 `0xB14D`）。
    pub fn open(&mut self, region: Region, tick: u64) -> LzResult<()> {
if !region.is_deep() { return Err(LzErr::of_aux(AuxCode::WindowIllegal, region.index() as u64)); }
if self.state == WindowState::Open { return Err(LzErr::of_aux(AuxCode::WindowIllegal, region.index() as u64)); }
        self.state = WindowState::Open;
        self.opened_tick = tick;
        self.open_count = self.open_count.wrapping_add(1);
        Ok(()) }
/// 关窗（窗闭时请求判非法 `0xB14D`）。
    pub fn close(&mut self) -> LzResult<()> {
if self.state == WindowState::Closed { return Err(LzErr::of_aux(AuxCode::WindowIllegal, 0)); }
        self.state = WindowState::Closed;
        Ok(()) }
/// 记录一次窗内延迟保存（F1903 接口位）。
pub fn note_deferred(&mut self) { if self.state == WindowState::Open { self.deferred_in_window = self.deferred_in_window.wrapping_add(1); } }
/// 记录一次窗闭强制落盘（F1903 接口位）。
    pub fn note_flush(&mut self) { self.forced_flush = self.forced_flush.wrapping_add(1); }
/// 窗内惰性挂起是否应被豁免"丢失"判定（E 项要求）。
    pub fn holds(&self, pending: bool) -> bool {
        pending && self.state == WindowState::Open }
/// 一行描述。
    pub fn describe(&self) -> String {
        format!( "window={}open@{}deferred={}flush={}opens={}", self.state.name(), self.opened_tick, self.deferred_in_window, self.forced_flush,
            self.open_count ) }
}
// ===========================================================================
// 8. 线程槽位与扩展状态账
// ===========================================================================
/// 单区账（每线程每区一份，F1908 汇总单元）。 守恒口径：`save_count == restore_count + lazy_pending`。 `lazy_pending` 的含义是**尚未被恢复消费掉的保存条目数**——它同时覆盖 「已落盘但尚未 fxrstor」与「已登记惰性挂起尚未落盘」两种在飞状态。 因此：save → 在飞 +1；defer（不 save）→ 在飞 +1；restore → 在飞 -1。
#[derive(Clone, Copy, Debug)] pub struct RegionLedger {
/// 已执行保存次数。
    pub save_count: u64,
/// 已执行恢复次数。
    pub restore_count: u64,
/// 当前在飞（既未恢复也未重新落盘）的条目数。
    pub lazy_pending: u64,
/// 被淘汰（丢弃）的保存区数（F1903 摊销窗消费的额度）。
    pub evicted: u64,
}
impl RegionLedger {
/// 零账。
pub fn zero() -> RegionLedger { RegionLedger { save_count: 0, restore_count: 0, lazy_pending: 0, evicted: 0, } }
/// 守恒式 D 项：`save == restore + lazy_pending`。
    pub fn balanced(&self) -> bool {
        self.save_count == self.restore_count + self.lazy_pending }
/// 守恒式 D 扩展（含淘汰）：`save == restore + lazy_pending + evicted`。
    pub fn balanced_with_evicted(&self) -> bool {
        self.save_count == self.restore_count + self.lazy_pending + self.evicted }
/// 偏离量（0 表示守恒；正负分别表示多存/少存）。
    pub fn deviation(&self) -> i64 {
        let lhs = self.save_count as i64;
        let rhs = (self.restore_count + self.lazy_pending) as i64;
        lhs - rhs }
/// 一行描述。
    pub fn describe(&self, region: Region) -> String {
        format!( "ledger({}):save={}restore={}lazy={}evict={}dev={}", region.name(), self.save_count, self.restore_count, self.lazy_pending,
            self.evicted, self.deviation() ) }
}
/// 惰性挂起记录。
#[derive(Clone, Copy, Debug)] pub struct LazyRecord {
/// 线程号。
    pub tid: u64,
/// 区序号。
    pub region: usize,
/// 登记 tick。
    pub since: u64,
/// 登记时的载荷指纹（消费时必须与之对拍）。
    pub fingerprint: u64,
/// 是否处于摊销窗内（窗内不得误判丢失）。
    pub in_window: bool,
}
impl LazyRecord {
/// 一行描述。
    pub fn describe(&self) -> String {
        format!( "lazy(tid=0x{:X},r={},since={},fp=0x{:016X},win={})", self.tid, self.region, self.since, self.fingerprint,
            if self.in_window { 1 } else { 0 }
        ) }
}
/// 单线程槽位（每区一条：状态 + 保存区 + 策略 + 账）。
#[derive(Clone, Debug)] pub struct RegionSlot {
/// 区域。
    pub region: Region,
/// 活动状态区。
    pub state: ExtState,
/// 保存区。
    pub area: SaveArea,
/// 保存策略（每线程每区一个）。
    pub policy: SavePolicy,
/// 惰性挂起记录（`None` 表示无挂起）。
    pub lazy: Option<LazyRecord>,
/// 账。
    pub ledger: RegionLedger,
/// 最近一次票据。
    pub last_ticket: Option<SaveTicket>,
/// 策略冲突标记（Never 却持有非零态 / Always 被跳过等）。
    pub conflicts: u64,
}
impl RegionSlot {
/// 构造槽位（按使能位排布保存区）。
    pub fn new(region: Region, enabled: bool, policy: SavePolicy, off: usize) -> RegionSlot {
        let mut state = ExtState::blank(region, enabled);
        let size = state.layout(off);
        RegionSlot {
            region, area: SaveArea {
                off: state.save_off, size, align: SAVE_ALIGN, words: vec![0u64; size / 8], valid: false, stamp: 0, generation: 0,
                expect_fingerprint: 0,
            }, state, policy, lazy: None, ledger: RegionLedger::zero(), last_ticket: None, conflicts: 0,
        }
    }
/// 当前动作（决策函数求值）。
    pub fn action(&self) -> Action {
        decide(self.policy, self.state.dirty, self.state.enabled) }
/// 是否需要有效保存区才能切入。 判据：XCR0 使能位要求该区状态、且该区仍持有未落盘的脏状态时， 必须已有可用保存区，否则拒绝切入（不静默）。`Never` 也不例外—— 承诺与事实冲突时以事实为准。
    pub fn needs_area(&self) -> bool {
        self.state.enabled && self.state.dirty }
/// 一行描述。
    pub fn describe(&self) -> String {
        format!( "[{} policy={} act={} lazy={} conflicts={}]", self.region.name(), self.policy.name(), self.action().name(),
            if self.lazy.is_some() { 1 } else { 0 }, self.conflicts ) }
}
/// 单线程完整上下文。
#[derive(Clone, Debug)] pub struct ThreadSlot {
/// 线程号。
    pub tid: u64,
/// XCR0 位图模拟。
    pub xcr0: u64,
/// TS 单元。
    pub ts: TsCell,
/// 摊销窗。
    pub window: AmortWindow,
/// 四区槽位。
    pub slots: Vec<RegionSlot>,
/// 切入次数。
    pub switch_in_count: u64,
/// 切出次数。
    pub switch_out_count: u64,
/// 执行浮点指令次数。
    pub fp_exec_count: u64,
/// #NM 陷入次数。
    pub nm_count: u64,
/// 摊销窗豁免次数（窗内挂起免判丢失）。
    pub window_exempt: u64,
/// 切入时保存区守卫被绕过（仅注入核使用；真实路径恒为 false）。
    pub area_guard_bypass: bool,
/// 在守卫被绕过时于缺区状态下放行的切入次数（形态 7 的引擎侧证据）。
    pub accepted_without_area: u64,
}
impl ThreadSlot {
/// 构造线程槽（`xcr0` 决定各区使能，布局按使能位逐区排布）。
    pub fn new(tid: u64, xcr0: u64) -> ThreadSlot {
        let mut slots: Vec<RegionSlot> = Vec::new();
        let mut off: usize = 0;
        for region in Region::ALL {
            let enabled = (xcr0 & region.xcr0_bit()) != 0;
            slots.push(RegionSlot::new( region, enabled, SavePolicy::Lazy, off, ));
            off = off.wrapping_add(region.aligned_size());
        }
        ThreadSlot {
            tid, xcr0, ts: TsCell::new(), window: AmortWindow::new(), slots, switch_in_count: 0, switch_out_count: 0, fp_exec_count: 0, nm_count: 0,
            window_exempt: 0, area_guard_bypass: false, accepted_without_area: 0,
        }
    }
/// 布局不变量检查（含 L4 无重叠）。
    pub fn layout_ok(&self) -> bool {
        let mut cursor: usize = 0;
        let mut any = false;
        for slot in self.slots.iter() {
if !slot.state.layout_ok() { return false; }
if slot.state.enabled { if slot.state.save_off != cursor { return false; } cursor = cursor.wrapping_add(slot.state.save_size); any = true; }
        }
        any }
/// 账整体守恒。
    pub fn ledger_ok(&self) -> bool {
for slot in self.slots.iter() { if !slot.ledger.balanced() && !slot.ledger.balanced_with_evicted() { return false; } }
        true }
/// 全账快照（观测用）。
    pub fn ledger_rows(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
for slot in self.slots.iter() { v.push(slot.ledger.describe(slot.region)); }
        v }
/// 一行描述。
    pub fn describe(&self) -> String {
        format!( "tid=0x{:X} xcr0=0x{:X} {} {} in={} out={} fp={} nm={} layout={} ledger={}", self.tid, self.xcr0, self.ts.describe(),
            self.window.describe(), self.switch_in_count, self.switch_out_count, self.fp_exec_count, self.nm_count,
            if self.layout_ok() { 1 } else { 0 }, if self.ledger_ok() { 1 } else { 0 }
        ) }
}
// ===========================================================================
// 9. 切出 / 切入 / 执行 / 陷入 四段协同引擎
// ===========================================================================
/// 切出报告。
#[derive(Clone, Debug)] pub struct SwitchOutReport {
/// 线程号。
    pub tid: u64,
/// 切出时 tick。
    pub tick: u64,
/// 立即保存的区序号。
    pub saved: Vec<usize>,
/// 推迟到惰性的区序号。
    pub deferred: Vec<usize>,
/// 跳过的区序号。
    pub skipped: Vec<usize>,
/// 本次是否置了 TS。
    pub ts_set: bool,
/// 策略冲突计数增量。
    pub conflicts: u64,
}
impl SwitchOutReport {
/// 一行描述。
    pub fn describe(&self) -> String {
        format!( "switchout(tid=0x{:X},tick={})saved={} deferred={} skipped={} ts_set={} conflicts={}", self.tid, self.tick, join_usize(&self.saved),
            join_usize(&self.deferred), join_usize(&self.skipped), if self.ts_set { 1 } else { 0 }, self.conflicts ) }
}
/// 切入报告。
#[derive(Clone, Debug)] pub struct SwitchInReport {
/// 线程号。
    pub tid: u64,
/// 切入时 tick。
    pub tick: u64,
/// 已恢复的区序号。
    pub restored: Vec<usize>,
/// 被拒绝的区序号（缺有效保存区）。
    pub rejected: Vec<usize>,
/// 入口时 TS 是否为置位。
    pub ts_was_set: bool,
/// 出口时 TS 是否清零。
    pub ts_cleared: bool,
/// 是否被整体拒绝。
    pub refused: bool,
}
impl SwitchInReport {
/// 一行描述。
    pub fn describe(&self) -> String {
        format!( "switchin(tid=0x{:X},tick={})restored={} rejected={} ts_in={} ts_out={} refused={}", self.tid, self.tick, join_usize(&self.restored),
            join_usize(&self.rejected), if self.ts_was_set { 1 } else { 0 }, if self.ts_cleared { 1 } else { 0 }, if self.refused { 1 } else { 0 }
        ) }
}
/// 整数列表串（观测用）。
pub fn join_usize(v: &[usize]) -> String {
if v.is_empty() { return "-".to_string(); }
    let mut s = String::new();
    let mut i = 0usize;
while i < v.len() { if i > 0 { s.push(','); } s.push_str(v.get(i).copied().unwrap_or(0).to_string().as_str()); i = i.wrapping_add(1); }
    s }
/// 执行结果闭集（执行一条浮点/SIMD 指令）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum ExecOutcome {
/// 直接执行（TS=0，单元可用）。
    Executed,
/// 触发 #NM（TS=1，进入陷入处理）。
    TrapNm,
/// 被拒（策略/使能不允许执行）。
    Rejected,
}
impl ExecOutcome {
/// 序号。
pub fn index(self) -> usize { match self { ExecOutcome::Executed => 0, ExecOutcome::TrapNm => 1, ExecOutcome::Rejected => 2, } }
/// 短名。
    pub fn name(self) -> &'static str {
match self { ExecOutcome::Executed => "executed", ExecOutcome::TrapNm => "trap_nm", ExecOutcome::Rejected => "rejected", }
    }
}
/// 惰性保存引擎（纯功能，无真实硬件访问）。
#[derive(Clone, Debug)] pub struct LazySaveEngine {
/// 逻辑时钟。
    tick: u64,
/// 确定性随机源。
    rng: Lcg,
/// 线程表（`BTreeMap` 保证遍历确定性）。
    threads: BTreeMap<u64, ThreadSlot>,
/// 注入的畸形（供检出扫描）。
    inject: Vec<Injected>,
/// 指标。
    metrics: Metrics,
/// #NM 轨迹台账。
    traces: Vec<NmTrace>,
/// 摊销窗开关次数（F1903 消费位）。
    window_toggles: u64,
}
impl LazySaveEngine {
/// 构造空引擎。
    pub fn new(seed: u64) -> LazySaveEngine {
        LazySaveEngine {
            tick: 0, rng: Lcg::new(seed), threads: BTreeMap::new(), inject: Vec::new(), metrics: Metrics::zero(), traces: Vec::new(),
            window_toggles: 0,
        }
    }
/// 当前逻辑 tick。
    pub fn now(&self) -> u64 {
        self.tick }
/// 推进逻辑 tick 并返回新值。
    pub fn advance(&mut self) -> u64 {
        self.tick = self.tick.wrapping_add(1);
        self.tick }
/// 线程是否存在。
    pub fn has_thread(&self, tid: u64) -> bool {
        self.threads.contains_key(&tid) }
/// 线程只读访问。
    pub fn thread(&self, tid: u64) -> LzResult<&ThreadSlot> {
match self.threads.get(&tid) { Some(t) => Ok(t), None => Err(LzErr::of_aux(AuxCode::ThreadUnknown, tid)), }
    }
/// 线程可变访问。
    pub fn thread_mut(&mut self, tid: u64) -> LzResult<&mut ThreadSlot> {
match self.threads.get_mut(&tid) { Some(t) => Ok(t), None => Err(LzErr::of_aux(AuxCode::ThreadUnknown, tid)), }
    }
/// 线程号列表（确定性序）。
    pub fn tids(&self) -> Vec<u64> {
        let mut v: Vec<u64> = Vec::new();
for k in self.threads.keys() { v.push(*k); }
        v }
/// 登记线程。
    pub fn admit(&mut self, tid: u64, xcr0: u64) -> LzResult<()> {
if self.threads.contains_key(&tid) { return Err(LzErr::of_aux(AuxCode::ThreadUnknown, tid)); }
        self.threads.insert(tid, ThreadSlot::new(tid, xcr0));
        Ok(()) }
/// 撤销线程。
    pub fn retire(&mut self, tid: u64) -> LzResult<()> {
match self.threads.remove(&tid) { Some(_) => Ok(()), None => Err(LzErr::of_aux(AuxCode::ThreadUnknown, tid)), }
    }
/// 设置某区策略。
    pub fn set_policy( &mut self, tid: u64, region: Region, policy: SavePolicy, ) -> LzResult<()> {
        let t = self.thread_mut(tid)?;
        let idx = region.index();
        match t.slots.get_mut(idx) {
            Some(s) => {
                s.policy = policy;
                self.metrics.bump_policy(policy);
                Ok(()) }
            None => Err(LzErr::of_aux(AuxCode::IndexOutOfRange, idx as u64)),
        }
    }
/// 标脏（模拟线程写扩展寄存器）。
    pub fn touch(&mut self, tid: u64, region: Region) -> LzResult<()> {
        let mut rng = self.rng;
        let idx = region.index();
        let r = match self.thread_mut(tid) {
            Ok(t) => match t.slots.get_mut(idx) {
                Some(s) => {
                    s.state.touch(&mut rng);
                    Ok(()) }
                None => Err(LzErr::of_aux(AuxCode::IndexOutOfRange, idx as u64)),
            }, Err(e) => Err(e),
        };
        // rng 已按值复制推进，写回以维持确定性轨迹连续。
        self.rng = rng;
        r }
/// 批量标脏 `n` 次。
    pub fn touch_n(&mut self, tid: u64, region: Region, n: usize) -> LzResult<()> {
        let mut i = 0usize;
while i < n { self.touch(tid, region)?; i = i.wrapping_add(1); }
        Ok(()) }
/// 取区槽位只读。
    pub fn slot(&self, tid: u64, region: Region) -> LzResult<&RegionSlot> {
        let t = self.thread(tid)?;
match t.slots.get(region.index()) { Some(s) => Ok(s), None => Err(LzErr::of_aux(AuxCode::IndexOutOfRange, region.index() as u64)), }
    }
/// 取区槽位可变。
    pub fn slot_mut(&mut self, tid: u64, region: Region) -> LzResult<&mut RegionSlot> {
        let t = self.thread_mut(tid)?;
        let idx = region.index();
match t.slots.get_mut(idx) { Some(s) => Ok(s), None => Err(LzErr::of_aux(AuxCode::IndexOutOfRange, idx as u64)), }
    }
/// 以闭包方式访问区槽位（借用安全的统一入口）。 本单所有「读—改—写」路径都走这里：`BTreeMap` 的可变借用不允许在 持有 `&mut ThreadSlot` 的同时再借 `&self`，逐处手写 `match` 会把 每个动作膨胀到十几行；收拢到单点后动作本体保持可读。 闭包额外拿到摊销窗与其豁免计数（`DeferToLazy` 分支需要）—— 用字段解构做**不相交借用**，而非传 `&mut ThreadSlot`。
    pub fn with_slot<R>( &mut self, tid: u64, region: Region, f: impl FnOnce(&mut RegionSlot, &mut AmortWindow, &mut u64) -> LzResult<R>,
    ) -> LzResult<R> {
        let idx = region.index();
        let t = self.thread_mut(tid)?;
        let ThreadSlot {
            slots, window, window_exempt, .. } = t;
match slots.get_mut(idx) { Some(s) => f(s, window, window_exempt), None => Err(LzErr::of_aux(AuxCode::IndexOutOfRange, idx as u64)), }
    }
/// 以闭包方式访问线程（借用安全的统一入口）。
    pub fn with_thread<R>( &mut self, tid: u64, f: impl FnOnce(&mut ThreadSlot) -> LzResult<R>, ) -> LzResult<R> {
        f(self.thread_mut(tid)?) }
    // -----------------------------------------------------------------------
    // 切出侧（F1805 收窄）：置 TS / 登记 LazyPending / 决定是否立即保存
    // -----------------------------------------------------------------------
/// 切出路径（判据核心子句「切出必存 / 惰性 / 永存」的实现处）。 判据：UNX-F1902-J1 —— 决策表是唯一动作来源；本函数只做「动作 → 副作用」。
    pub fn switch_out(&mut self, tid: u64) -> LzResult<SwitchOutReport> {
        let tick = self.advance();
        let mut saved: Vec<usize> = Vec::new();
        let mut deferred: Vec<usize> = Vec::new();
        let mut skipped: Vec<usize> = Vec::new();
        let mut conflicts: u64 = 0;
        let mut need_ts = false;
        // 第一遍：求动作。未使能区按 F1901 裁断结果统一收敛为 Skip
        //（决策表仍按策略求值给出动作，但无保存区可落）。
        let mut plan: Vec<(Region, Action)> = Vec::new();
        self.with_thread(tid, |t| {
            t.switch_out_count = t.switch_out_count.wrapping_add(1);
            for slot in t.slots.iter() {
                let a = if slot.state.enabled {
                    slot.action() } else {
                    Action::Skip };
                plan.push((slot.region, a));
            }
            Ok(()) })?;
        // 第二遍：执行副作用。
        for (region, action) in plan {
            let idx = region.index();
            match action {
                Action::SaveNow => {
                    let (never_conflict, tk) = self.with_slot(tid, region, |s, _w, _x| {
                            let r = save_clone(&mut s.state, &mut s.area, tick);
                            Ok((s.policy == SavePolicy::Never, r)) })?;
                    if never_conflict {
                        self.with_slot(tid, region, |s, _w, _x| {
                            s.conflicts = s.conflicts.wrapping_add(1);
                            Ok(()) })?;
                        conflicts = conflicts.wrapping_add(1);
                    }
                    match tk {
                        Ok(ticket) => {
                            self.with_slot(tid, region, |s, _w, _x| {
                                s.last_ticket = Some(ticket);
                                s.ledger.save_count = s.ledger.save_count.wrapping_add(1);
                                // 已落盘即在飞条目 +1，等切入侧 fxrstor 消费。
                                s.ledger.lazy_pending = s.ledger.lazy_pending.wrapping_add(1);
                                s.lazy = None;
                                Ok(()) })?;
                            saved.push(idx);
                        }
                        Err(e) => {
                            self.with_slot(tid, region, |s, _w, _x| {
                                s.conflicts = s.conflicts.wrapping_add(1);
                                Ok(()) })?;
                            return Err(e);
                        }
                    }
                }
                Action::DeferToLazy => {
                    self.with_slot(tid, region, |s, w, x| {
                        let in_window = w.holds(true);
if in_window { *x = x.wrapping_add(1); }
                        s.lazy = Some(LazyRecord {
                            tid, region: idx, since: tick, fingerprint: s.state.fingerprint(), in_window,
                        });
                        // 未落盘的挂起条目同样计入在飞（不增加 save_count）。
                        s.ledger.lazy_pending = s.ledger.lazy_pending.wrapping_add(1);
                        s.state.cleanliness = Cleanliness::LazyPending;
                        Ok(()) })?;
                    need_ts = true;
                    deferred.push(idx);
                }
                Action::Skip => {
                    let c = self.with_slot(tid, region, |s, _w, _x| {
                        // 从严策略被跳过 / 永存承诺与事实冲突：以事实为准记账。
                        let mut n = 0u64;
if s.state.dirty { n = n.wrapping_add(1); if s.policy == SavePolicy::AlwaysOnSwitchOut { n = n.wrapping_add(1); } }
                        s.conflicts = s.conflicts.wrapping_add(n);
                        Ok(n) })?;
                    conflicts = conflicts.wrapping_add(c);
                    skipped.push(idx);
                }
            }
        }
        // 第三遍：置 TS（任一惰性挂起才置）。
        let ts_set = if need_ts {
            self.with_thread(tid, |t| Ok(t.ts.go(TsState::Set, tick).is_ok()))? } else {
            false };
        let m = &mut self.metrics;
        m.switch_outs = m.switch_outs.wrapping_add(1);
        m.immediate_saves = m.immediate_saves.wrapping_add(saved.len() as u64);
        m.deferrals = m.deferrals.wrapping_add(deferred.len() as u64);
        m.skips = m.skips.wrapping_add(skipped.len() as u64);
        m.conflicts = m.conflicts.wrapping_add(conflicts);
        Ok(SwitchOutReport {
            tid, tick, saved, deferred, skipped, ts_set, conflicts,
        }) }
    // -----------------------------------------------------------------------
    // 切入侧（F1806 收窄）：先验保存区，再 fxrstor，再清 TS
    // -----------------------------------------------------------------------
/// 切入路径（判据核心子句「恢复/清位闭环」的恢复侧）。 判据：UNX-F1902-J1 —— 缺有效保存区必须拒绝切入，不静默降级。
    pub fn switch_in(&mut self, tid: u64) -> LzResult<SwitchInReport> {
        let tick = self.advance();
        // 第一遍：准入校验（缺有效保存区则拒绝，不静默）。
        let (ts_was_set, bypass, mut rejected, plan) = self.with_thread(tid, |t| {
            t.switch_in_count = t.switch_in_count.wrapping_add(1);
            let mut rej: Vec<usize> = Vec::new();
            let mut ok: Vec<usize> = Vec::new();
            for slot in t.slots.iter() {
if slot.needs_area() && !slot.area.valid { rej.push(slot.region.index()); }
if slot.area.valid { ok.push(slot.region.index()); }
            }
            Ok((t.ts.state != TsState::Clear, t.area_guard_bypass, rej, ok)) })?;
        let refused = !rejected.is_empty();
        rejected = rejected.clone();
        if refused && bypass {
            // 守卫被绕过：缺区仍放行——形态 7 的引擎侧现场证据。
            self.with_thread(tid, |t| {
                t.accepted_without_area = 1;
                Ok(()) })?;
            self.metrics.missing_area_accepted = self.metrics.missing_area_accepted.wrapping_add(1);
            return Ok(SwitchInReport {
                tid, tick, restored: Vec::new(), rejected, ts_was_set, ts_cleared: false, refused: false,
            });
        }
        if refused {
            self.metrics.switch_in_refused = self.metrics.switch_in_refused.wrapping_add(1);
            return Ok(SwitchInReport {
                tid, tick, restored: Vec::new(), rejected, ts_was_set, ts_cleared: false, refused,
            });
        }
        // 第二遍：恢复（fxrstor），消费在飞条目。
        let mut restored: Vec<usize> = Vec::new();
        for idx in plan {
            let region = match Region::from_index(idx) {
                Some(r) => r, None => continue,
            };
            self.with_slot(tid, region, |s, _w, _x| {
                let r = restore_clone(&s.area, &mut s.state, tick);
                if r.is_ok() {
                    s.ledger.restore_count = s.ledger.restore_count.wrapping_add(1);
if s.ledger.lazy_pending > 0 { s.ledger.lazy_pending = s.ledger.lazy_pending.wrapping_sub(1); s.lazy = None; }
                }
                r.map(|_| ()) })?;
            restored.push(idx);
        }
        // 第三遍：清 TS（闭环的清位侧）。
        let ts_cleared = self.with_thread(tid, |t| {
            let v = if t.ts.state == TsState::Set || t.ts.state == TsState::Restore {
                t.ts.go(TsState::Clear, tick).is_ok() } else {
                t.ts.state == TsState::Clear };
            Ok(v) })?;
        self.metrics.switch_ins = self.metrics.switch_ins.wrapping_add(1);
        self.metrics.restores = self.metrics.restores.wrapping_add(restored.len() as u64);
        Ok(SwitchInReport {
            tid, tick, restored, rejected, ts_was_set, ts_cleared, refused,
        }) }
    // -----------------------------------------------------------------------
    // 执行侧：TS=1 时浮点指令触发 #NM
    // -----------------------------------------------------------------------
/// 执行一条浮点 / SIMD 指令。 判据：UNX-F1902-J1 —— 「TS 置位 → 首次使用 #NM 陷阱」的触发点。
    pub fn exec_fp(&mut self, tid: u64, region: Region) -> LzResult<ExecOutcome> {
        let tick = self.advance();
        let mut outcome = ExecOutcome::Rejected;
        {
            let t = self.thread_mut(tid)?;
            t.fp_exec_count = t.fp_exec_count.wrapping_add(1);
            let idx = region.index();
            let slot_ok = match t.slots.get(idx) {
                Some(s) => s.state.enabled && s.policy != SavePolicy::Never, None => false,
            };
if slot_ok { outcome = if t.ts.state == TsState::Set { ExecOutcome::TrapNm } else { ExecOutcome::Executed }; }
            let _ = tick;
        }
        self.metrics.fp_execs = self.metrics.fp_execs.wrapping_add(1);
        match outcome {
            ExecOutcome::Executed => {
                self.metrics.fp_direct = self.metrics.fp_direct.wrapping_add(1);
            }
            ExecOutcome::TrapNm => {
                self.metrics.fp_trapped = self.metrics.fp_trapped.wrapping_add(1);
            }
            ExecOutcome::Rejected => {
                self.metrics.fp_rejected = self.metrics.fp_rejected.wrapping_add(1);
            }
        }
        Ok(outcome) }
    // -----------------------------------------------------------------------
    // #NM 陷入处理：查账 → 保存 → 恢复 → 清 TS → 重执行
    // -----------------------------------------------------------------------
/// #NM 陷入处理（判据核心子句「#NM 陷阱 → 恢复/清位闭环」）。 完整路径：TS=1 时执行浮点指令 → #NM → 查 LazyPending 账 → 找到待存区 → 执行保存（指纹对拍）→ fxrstor → 清 TS → 重执行原指令恰好一次。每一步都落账，闭环以 `NmTrace` 返回。 判据：UNX-F1902-J1 —— 进入 #NM 时必有 LazyPending 记录， 出口时该记录被消费且 TS 清零。
    pub fn enter_nm(&mut self, tid: u64) -> LzResult<NmTrace> {
        let tick = self.advance();
        // 步骤 0：重入禁令（F1848 同法）——陷阱处理态不得再次陷入。
        let cur = self.thread(tid)?.ts.state;
        if cur == TsState::Trap || cur == TsState::Restore {
            self.metrics.reentries = self.metrics.reentries.wrapping_add(1);
            return Err(self.fail_nm(tid, cur, Form::TrapReentry));
        }
        let enter_state = cur;
        if enter_state != TsState::Set {
            // Clear 下收到 #NM：邻接表 Clear 不含 Trap，迁移非法，
            // 故在发起迁移**之前**判凭空陷阱，避免误记成状态机错误。
            self.metrics.spurious = self.metrics.spurious.wrapping_add(1);
            return Err(self.fail_nm(tid, enter_state, Form::SpuriousTrap));
        }
        // 步骤 1：合法入口一定是 Set，推入 Trap 并落 nm_count。
        self.with_thread(tid, |t| {
            t.nm_count = t.nm_count.wrapping_add(1);
            t.ts.go(TsState::Trap, tick) })?;
        // 步骤 2：查账，收集待存惰性记录。
        let pend = self.with_thread(tid, |t| {
            let pend = t.slots .iter() .filter(|s| s.lazy.is_some()) .map(|s| s.region.index()) .collect::<Vec<usize>>();
            Ok(pend) })?;
        let pending_found = pend.len();
        if pending_found == 0 {
            // TS 置位却无任何待存记录：闭环无处可闭，判凭空陷阱。
            self.metrics.spurious = self.metrics.spurious.wrapping_add(1);
            let st = self.thread(tid)?.ts.state;
            return Err(self.fail_nm(tid, st, Form::SpuriousTrap));
        }
        self.metrics.lazy_hits = self.metrics.lazy_hits.wrapping_add(1);
        // 步骤 3：逐条消费——保存 + 指纹对拍 + 作废挂起记录。
        let mut saves_done = 0usize;
        let mut mismatch = 0u64;
        for idx in pend {
            let region = match Region::from_index(idx) {
                Some(r) => r, None => continue,
            };
            mismatch = mismatch.wrapping_add(self.with_slot(tid, region, |s, _w, _x| {
                let expect_fp = s.lazy.map(|r| r.fingerprint).unwrap_or(0);
                let tk = save_clone(&mut s.state, &mut s.area, tick)?;
                let bad = if tk.fingerprint != expect_fp { 1 } else { 0 };
                s.last_ticket = Some(tk);
                s.ledger.save_count = s.ledger.save_count.wrapping_add(1);
                // 挂起条目被这次保存「就地消费」：save +1、在飞不变、记录作废。
                s.lazy = None;
                Ok(bad as u64) })?);
            saves_done = saves_done.wrapping_add(1);
        }
        self.metrics.fingerprint_mismatch = self.metrics.fingerprint_mismatch.wrapping_add(mismatch);
        // 步骤 4：fxrstor（切入/陷阱侧按需恢复已保存但未在活的区）。
        let restore_list = self.with_thread(tid, |t| {
            let list = t.slots .iter() .filter(|s| s.area.valid && s.state.cleanliness == Cleanliness::Restored)
                .map(|s| s.region.index()) .collect::<Vec<usize>>();
            Ok(list) })?;
        let mut restores_done = 0usize;
        for idx in restore_list {
            let region = match Region::from_index(idx) {
                Some(r) => r, None => continue,
            };
            self.with_slot(tid, region, |s, _w, _x| {
                restore_clone(&s.area, &mut s.state, tick)?;
                s.ledger.restore_count = s.ledger.restore_count.wrapping_add(1);
                Ok(()) })?;
            restores_done = restores_done.wrapping_add(1);
        }
        // 步骤 5：清 TS 闭环（Restore → Clear）。
        let exit_state = self.with_thread(tid, |t| {
            t.ts.go(TsState::Restore, tick)?;
            t.ts.go(TsState::Clear, tick)?;
            Ok(t.ts.state) })?;
        // 步骤 6：重执行原指令恰好一次。
        self.metrics.restarts = self.metrics.restarts.wrapping_add(1);
        self.metrics.nm_traps = self.metrics.nm_traps.wrapping_add(1);
        let tr = NmTrace {
            tid, enter_state, pending_found, saves_done, restores_done, exit_state, restart: 1,
            closed: exit_state == TsState::Clear && pending_found > 0,
        };
        self.traces.push(tr.clone());
        Ok(tr) }
/// 落一条**失败**的 #NM 轨迹并返回对应错误。 失败轨迹同样必须入账：凭空陷阱与重入都是判据要检出的形态， 「没发生过的失败」等于没检测。
    fn fail_nm(&mut self, tid: u64, state: TsState, form: Form) -> LzErr {
        let tr = NmTrace {
            tid, enter_state: state, pending_found: 0, saves_done: 0, restores_done: 0, exit_state: state, restart: 0, closed: false,
        };
        self.traces.push(tr);
        LzErr::of_form(form, tid) }
/// #NM 轨迹台账（只读）。
    pub fn traces(&self) -> &[NmTrace] {
        &self.traces }
/// 摊销窗开（仅 AVX-512）。
    pub fn open_window(&mut self, tid: u64, region: Region) -> LzResult<()> {
        let tick = self.advance();
        self.thread_mut(tid)?.window.open(region, tick)?;
        self.window_toggles = self.window_toggles.wrapping_add(1);
        self.metrics.window_toggles = self.metrics.window_toggles.wrapping_add(1);
        Ok(()) }
/// 摊销窗闭（窗闭强制落盘由 F1903 接管，本单只记账）。
    pub fn close_window(&mut self, tid: u64) -> LzResult<u64> {
        self.thread_mut(tid)?.window.close()?;
        self.window_toggles = self.window_toggles.wrapping_add(1);
        self.metrics.window_toggles = self.metrics.window_toggles.wrapping_add(1);
        Ok(self.thread(tid)?.window.forced_flush) }
/// 全引擎账守恒。
    pub fn ledger_ok(&self) -> bool {
for t in self.threads.values() { if !t.ledger_ok() { return false; } }
        true }
/// 全引擎布局不变量。
    pub fn layout_ok(&self) -> bool {
for t in self.threads.values() { if !t.layout_ok() { return false; } }
        true }
/// 注入台账。
    pub fn injected(&self) -> &[Injected] {
        &self.inject }
/// 记录一次注入（仅供注入核与检出扫描使用）。
    pub fn note_inject(&mut self, inj: Injected) { self.inject.push(inj); }
/// 记一次往返保真破坏（形态 9 的引擎侧证据）。
    pub fn note_roundtrip_break(&mut self) { self.metrics.roundtrip_breaks = self.metrics.roundtrip_breaks.wrapping_add(1); }
/// 指标快照。
    pub fn metrics(&self) -> Metrics {
        let mut m = self.metrics;
        m.window_toggles = self.window_toggles;
        m }
}
// ===========================================================================
// 10. 指标
// ===========================================================================
/// 观测指标（任务单 G 项）。
#[derive(Clone, Copy, Debug)] pub struct Metrics {
/// 三态分布：AlwaysOnSwitchOut 计数。
    pub policy_always: u64,
/// 三态分布：Lazy 计数。
    pub policy_lazy: u64,
/// 三态分布：Never 计数。
    pub policy_never: u64,
/// #NM 触发次数（闭环成功）。
    pub nm_traps: u64,
/// 凭空 #NM 次数。
    pub spurious: u64,
/// #NM 重入次数。
    pub reentries: u64,
/// 惰性命中率分子（真正因惰性挂起而触发的 #NM）。
    pub lazy_hits: u64,
/// 立即保存次数。
    pub immediate_saves: u64,
/// 切出次数。
    pub switch_outs: u64,
/// 切入次数。
    pub switch_ins: u64,
/// 切入被拒次数。
    pub switch_in_refused: u64,
/// 缺有效保存区仍被放行的切入次数（形态 7 的引擎侧证据）。
    pub missing_area_accepted: u64,
/// 恢复次数。
    pub restores: u64,
/// 推迟次数。
    pub deferrals: u64,
/// 跳过次数。
    pub skips: u64,
/// 浮点执行总次数。
    pub fp_execs: u64,
/// 浮点直接执行次数。
    pub fp_direct: u64,
/// 浮点触发陷阱次数。
    pub fp_trapped: u64,
/// 浮点被拒次数。
    pub fp_rejected: u64,
/// 原指令重执行次数。
    pub restarts: u64,
/// 往返保真校验次数。
    pub roundtrip_checks: u64,
/// 往返保真失配次数。
    pub roundtrip_breaks: u64,
/// 惰性指纹对拍失配次数。
    pub fingerprint_mismatch: u64,
/// 账守恒偏离数。
    pub ledger_deviations: u64,
/// 摊销窗开关次数。
    pub window_toggles: u64,
/// 窗内挂起豁免次数。
    pub window_exempt: u64,
/// 策略冲突计数。
    pub conflicts: u64,
/// 检出命中计数（按十形态累加）。
    pub detected: u64,
}
impl Metrics {
/// 全零指标。
    pub fn zero() -> Metrics {
        Metrics {
            policy_always: 0, policy_lazy: 0, policy_never: 0, nm_traps: 0, spurious: 0, reentries: 0, lazy_hits: 0, immediate_saves: 0,
            switch_outs: 0, switch_ins: 0, switch_in_refused: 0, missing_area_accepted: 0, restores: 0, deferrals: 0, skips: 0, fp_execs: 0,
            fp_direct: 0, fp_trapped: 0, fp_rejected: 0, restarts: 0, roundtrip_checks: 0, roundtrip_breaks: 0, fingerprint_mismatch: 0,
            ledger_deviations: 0, window_toggles: 0, window_exempt: 0, conflicts: 0, detected: 0,
        }
    }
/// 三态分布计一票。
    pub fn bump_policy(&mut self, p: SavePolicy) {
        match p {
            SavePolicy::AlwaysOnSwitchOut => {
                self.policy_always = self.policy_always.wrapping_add(1) }
            SavePolicy::Lazy => self.policy_lazy = self.policy_lazy.wrapping_add(1),
            SavePolicy::Never => self.policy_never = self.policy_never.wrapping_add(1),
        }
    }
/// 三态分布总和。
    pub fn policy_total(&self) -> u64 {
        self.policy_always .wrapping_add(self.policy_lazy) .wrapping_add(self.policy_never) }
/// 惰性命中率（万分比，避免浮点）。
    pub fn lazy_hit_permille(&self) -> u64 {
if self.nm_traps == 0 { return 0; }
        self.lazy_hits .wrapping_mul(1000) .checked_div(self.nm_traps) .unwrap_or(0) }
/// 立即保存率（万分比）。
    pub fn immediate_rate_permille(&self) -> u64 {
        let total = self.immediate_saves.wrapping_add(self.deferrals);
if total == 0 { return 0; }
        self.immediate_saves .wrapping_mul(1000) .checked_div(total) .unwrap_or(0) }
/// 指标键值对（观测导出共用）。
    pub fn kv(&self) -> Vec<(&'static str, u64)> {
        vec![ ("policy_always", self.policy_always), ("policy_lazy", self.policy_lazy), ("policy_never", self.policy_never),
            ("nm_traps", self.nm_traps), ("spurious_nm", self.spurious), ("nm_reentry", self.reentries), ("lazy_hits", self.lazy_hits),
            ("immediate_saves", self.immediate_saves), ("switch_outs", self.switch_outs), ("switch_ins", self.switch_ins),
            ("switch_in_refused", self.switch_in_refused), ("missing_area_accepted", self.missing_area_accepted), ("restores", self.restores),
            ("deferrals", self.deferrals), ("skips", self.skips), ("fp_execs", self.fp_execs), ("fp_direct", self.fp_direct),
            ("fp_trapped", self.fp_trapped), ("fp_rejected", self.fp_rejected), ("restarts", self.restarts),
            ("roundtrip_checks", self.roundtrip_checks), ("roundtrip_breaks", self.roundtrip_breaks), ("fp_mismatch", self.fingerprint_mismatch),
            ("ledger_deviations", self.ledger_deviations), ("window_toggles", self.window_toggles), ("window_exempt", self.window_exempt),
            ("conflicts", self.conflicts), ("detected", self.detected), ] }
/// 指标指纹（跨进程比对键）。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::new();
for (_k, v) in self.kv() { words.push(v); }
        mix64(&words) }
}
// ===========================================================================
// 11. 注入核（十形态）
// ===========================================================================
/// 一条注入记录。
#[derive(Clone, Copy, Debug)] pub struct Injected {
/// 形态。
    pub form: Form,
/// 注入时的逻辑 tick。
    pub tick: u64,
/// 关联线程。
    pub tid: u64,
/// 关联区序号。
    pub region: usize,
/// 检出时应命中的判据描述。
    pub expect: &'static str,
}
impl Injected {
/// 一行描述。
    pub fn describe(&self) -> String {
        format!( "inject #{} {} tick={} tid=0x{:X} r={} expect=[{}]", self.form.index(), self.form.name(), self.tick, self.tid, self.region,
            self.expect ) }
}
/// 检出结论。
#[derive(Clone, Copy, Debug)] pub struct Detection {
/// 形态。
    pub form: Form,
/// 是否检出。
    pub hit: bool,
/// 命中证据描述。
    pub evidence: &'static str,
/// 命中次数。
    pub hits: u64,
}
impl Detection {
/// 一行描述。
    pub fn describe(&self) -> String {
        format!( "detect #{} {} {} hits={} [{}]", self.form.index(), self.form.name(), if self.hit { "HIT" } else { "MISS" }, self.hits,
            self.evidence ) }
}
/// 检出扫描的判定输入（从引擎抽取的**证据**集合）。
#[derive(Clone, Debug)] pub struct Evidence {
/// 凭空 #NM 次数。
    pub spurious: u64,
/// #NM 重入次数。
    pub reentries: u64,
/// 陷阱后 TS 未清次数。
    pub ts_leak: u64,
/// 惰性记录未消费条数。
    pub lazy_unconsumed: u64,
/// Never 却保存次数。
    pub save_on_never: u64,
/// Always 却跳过次数。
    pub skip_on_always: u64,
/// 切入缺区放行次数。
    pub missing_area_accepted: u64,
/// 未对齐保存区次数。
    pub misaligned: u64,
/// 往返保真破坏次数。
    pub roundtrip_breaks: u64,
/// 账偏离条数。
    pub ledger_deviations: u64,
}
impl Evidence {
/// 全零证据。
    pub fn zero() -> Evidence {
        Evidence {
            spurious: 0, reentries: 0, ts_leak: 0, lazy_unconsumed: 0, save_on_never: 0, skip_on_always: 0, missing_area_accepted: 0, misaligned: 0,
            roundtrip_breaks: 0, ledger_deviations: 0,
        }
    }
/// 取某形态的命中次数。
    pub fn of(&self, f: Form) -> u64 {
        match f {
            Form::SpuriousTrap => self.spurious, Form::TrapReentry => self.reentries, Form::TsNotCleared => self.ts_leak,
            Form::LazyRecordUnconsumed => self.lazy_unconsumed, Form::SaveOnNever => self.save_on_never, Form::SkipOnAlways => self.skip_on_always,
            Form::MissingAreaAccepted => self.missing_area_accepted, Form::MisalignedSaveArea => self.misaligned,
            Form::RoundTripBroken => self.roundtrip_breaks, Form::LedgerUnbalanced => self.ledger_deviations,
        }
    }
/// 证据指纹。
    pub fn fingerprint(&self) -> u64 {
        let words = [ self.spurious, self.reentries, self.ts_leak, self.lazy_unconsumed, self.save_on_never, self.skip_on_always,
            self.missing_area_accepted, self.misaligned, self.roundtrip_breaks, self.ledger_deviations, ];
        mix64(&words) }
}
/// 十形态检出扫描（每形态一个判定）。 判据：UNX-F1902-J1 —— 十形态逐一检出，10/10。
pub fn detect_all(ev: &Evidence) -> Vec<Detection> {
    let mut out: Vec<Detection> = Vec::new();
    for f in Form::ALL {
        let hits = ev.of(f);
        let (hit, why) = match f {
            Form::SpuriousTrap => (hits > 0, "无待存记录却收到 #NM"), Form::TrapReentry => (hits > 0, "陷阱处理态再次陷入 #NM"),
            Form::TsNotCleared => (hits > 0, "闭环出口 TS 仍为 1"), Form::LazyRecordUnconsumed => (hits > 0, "LazyPending 超龄未消费"),
            Form::SaveOnNever => (hits > 0, "Never 区出现 save_count 增长"), Form::SkipOnAlways => (hits > 0, "Always 区切出未落盘"),
            Form::MissingAreaAccepted => (hits > 0, "切入在缺区下仍放行"), Form::MisalignedSaveArea => (hits > 0, "保存区未 64 字节对齐"),
            Form::RoundTripBroken => (hits > 0, "restore 后指纹不符"), Form::LedgerUnbalanced => (hits > 0, "save != restore + lazy"),
        };
        out.push(Detection {
            form: f, hit, evidence: why, hits,
        });
    }
    out }
/// 检出全过（10/10）。
pub fn detect_all_ok(ev: &Evidence) -> bool {
    let d = detect_all(ev);
if d.len() != FORM_COUNT { return false; }
for x in d.iter() { if !x.hit { return false; } }
    true }
/// 检出结论串（观测导出用）。
pub fn detection_rows(d: &[Detection]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
for x in d.iter() { v.push(x.describe()); }
    v }
// ===========================================================================
// 12. 注入核实现：每形态一函数 + inject_all()
// ===========================================================================
/// 形态 1：凭空 #NM——TS 未置位直接发 #NM。
pub fn inject_spurious_trap(e: &mut LazySaveEngine, tid: u64) -> LzResult<u64> {
    let now = e.advance();
    // 先保证 TS 为 Clear（全新线程即为此态）。
    e.thread_mut(tid)?.ts.go(TsState::Clear, now)?;
    let _ = e.enter_nm(tid);
    e.note_inject(Injected {
        form: Form::SpuriousTrap, tick: e.now(), tid, region: 0, expect: "spurious=1",
    });
    Ok(1) }
/// 形态 2：#NM 重入——陷阱处理中再次陷入。
pub fn inject_reentry(e: &mut LazySaveEngine, tid: u64) -> LzResult<u64> {
    // 构造一个真实的挂起记录，使第一次陷入可进入 Trap 态。
    let now = e.now();
    let idx = 0usize;
    {
        let t = e.thread_mut(tid)?;
        t.ts.go(TsState::Set, now)?;
        if let Some(s) = t.slots.get_mut(idx) {
            s.state.dirty = true;
            s.lazy = Some(LazyRecord {
                tid, region: idx, since: now, fingerprint: s.state.fingerprint(), in_window: false,
            });
            s.ledger.lazy_pending = s.ledger.lazy_pending.wrapping_add(1);
        }
    }
    // 第一次陷入把 TS 推到 Trap/Restore；第二次即重入。
    let _ = e.enter_nm(tid);
    // 强制回到 Trap 态以模拟「处理中又来一次」。
    e.thread_mut(tid)?.ts.state = TsState::Trap;
    let r = e.enter_nm(tid);
    let code_ok = matches!(r, Err(x) if x.code == Form::TrapReentry.code());
    e.note_inject(Injected {
        form: Form::TrapReentry, tick: e.now(), tid, region: idx, expect: "reentry=1",
    });
    Ok(if code_ok { 1 } else { 0 }) }
/// 形态 3：陷阱后未清 TS——闭环出口 TS 仍为 1。
pub fn inject_ts_not_cleared(e: &mut LazySaveEngine, tid: u64) -> LzResult<u64> {
    let now = e.now();
    // 强行把 TS 拉回 Set 并记账，模拟「保存做了但 TS 没清」。
    let leak = {
        let t = e.thread_mut(tid)?;
        t.ts.state = TsState::Set;
        if t.ts.bit() == CR0_TS {
            1 } else {
            0 }
    };
    e.note_inject(Injected {
        form: Form::TsNotCleared, tick: now, tid, region: 0, expect: "ts_leak=1",
    });
    Ok(leak) }
/// 形态 4：惰性记录未被消费——挂起后超龄不消费。
pub fn inject_lazy_unconsumed(e: &mut LazySaveEngine, tid: u64) -> LzResult<u64> {
    let idx = 1usize;
    let now = e.now();
    {
        let t = e.thread_mut(tid)?;
        if let Some(s) = t.slots.get_mut(idx) {
            s.state.dirty = true;
            s.lazy = Some(LazyRecord {
                tid, region: idx, since: now.wrapping_sub(LAZY_STALE_TICKS).wrapping_sub(1), fingerprint: s.state.fingerprint(), in_window: false,
            });
            s.ledger.lazy_pending = s.ledger.lazy_pending.wrapping_add(1);
        }
    }
    let cur = e.now();
    let aged = {
        let t = e.thread(tid)?;
        let mut n = 0u64;
        for slot in t.slots.iter() {
if let Some(l) = slot.lazy { if cur.wrapping_sub(l.since) > LAZY_STALE_TICKS && !l.in_window { n = n.wrapping_add(1); } }
        }
        n };
    e.note_inject(Injected {
        form: Form::LazyRecordUnconsumed, tick: cur, tid, region: idx, expect: "lazy_unconsumed=1",
    });
    Ok(aged) }
/// 形态 5：Never 策略却执行了保存。
pub fn inject_save_on_never(e: &mut LazySaveEngine, tid: u64) -> LzResult<u64> {
    let region = Region::Sse;
    e.set_policy(tid, region, SavePolicy::Never)?;
    e.touch(tid, region)?;
    let now = e.now();
    // 绕过策略直接落盘，模拟"承诺永存却存了"。
    let core = {
        let s = e.slot(tid, region)?;
        (s.state.clone(), s.area.clone()) };
    let (mut st, mut ar) = core;
    let ok = save(&mut st, &mut ar, now).is_ok();
if ok { let s = e.slot_mut(tid, region)?; s.ledger.save_count = s.ledger.save_count.wrapping_add(1); }
    e.note_inject(Injected {
        form: Form::SaveOnNever, tick: e.now(), tid, region: region.index(), expect: "save_on_never=1",
    });
    Ok(if ok { 1 } else { 0 }) }
/// 形态 6：Always 策略却跳过保存。
pub fn inject_skip_on_always(e: &mut LazySaveEngine, tid: u64) -> LzResult<u64> {
    let region = Region::Avx;
    e.set_policy(tid, region, SavePolicy::AlwaysOnSwitchOut)?;
    e.touch(tid, region)?;
    let skipped = {
        // 直接标冲突并保持脏，等价于 Always 被跳过。
        let s = e.slot_mut(tid, region)?;
        s.area.valid = false;
        s.state.dirty = true;
        s.conflicts = s.conflicts.wrapping_add(1);
        s.area.valid == false && s.policy == SavePolicy::AlwaysOnSwitchOut };
    e.note_inject(Injected {
        form: Form::SkipOnAlways, tick: e.now(), tid, region: region.index(), expect: "skip_on_always=1",
    });
    Ok(if skipped { 1 } else { 0 }) }
/// 形态 7：切入时缺有效保存区仍放行。
pub fn inject_missing_area_accepted(e: &mut LazySaveEngine, tid: u64) -> LzResult<u64> {
    let region = Region::X87;
    e.set_policy(tid, region, SavePolicy::Lazy)?;
    e.touch(tid, region)?;
    {
        let s = e.slot_mut(tid, region)?;
        s.area.valid = false;
for w in s.area.words.iter_mut() { *w = 0; }
    }
    // 绕过守卫：模拟「实现漏了准入校验」这一形态本身。
    e.thread_mut(tid)?.area_guard_bypass = true;
    let r = e.switch_in(tid)?;
    let leaked = !r.refused && r.rejected.is_empty();
    e.thread_mut(tid)?.area_guard_bypass = false;
    e.note_inject(Injected {
        form: Form::MissingAreaAccepted, tick: e.now(), tid, region: region.index(), expect: "missing_area_accepted=1",
    });
    Ok(if leaked { 1 } else { 0 }) }
/// 形态 8：保存区未 64 字节对齐。
pub fn inject_misaligned(e: &mut LazySaveEngine, tid: u64) -> LzResult<u64> {
    let region = Region::Sse;
    e.touch(tid, region)?;
    e.slot_mut(tid, region)?.area.align = 32; // 故意破坏对齐
    let now = e.now();
    let core = {
        let s = e.slot(tid, region)?;
        (s.state.clone(), s.area.clone()) };
    let (mut st, mut ar) = core;
    let r = save(&mut st, &mut ar, now);
    let bad = matches!(r, Err(x) if x.code == Form::MisalignedSaveArea.code());
    e.note_inject(Injected {
        form: Form::MisalignedSaveArea, tick: e.now(), tid, region: region.index(), expect: "misaligned=1",
    });
    Ok(if bad { 1 } else { 0 }) }
/// 形态 9：往返保真破坏——保存区内容被改后 restore。
pub fn inject_roundtrip_broken(e: &mut LazySaveEngine, tid: u64) -> LzResult<u64> {
    let region = Region::Avx;
    e.touch_n(tid, region, 3)?;
    let now = e.now();
    {
        let core = {
            let s = e.slot(tid, region)?;
            (s.state.clone(), s.area.clone()) };
        let (mut st, mut ar) = core;
if save(&mut st, &mut ar, now).is_err() { return Ok(0); }
        let s = e.slot_mut(tid, region)?;
        s.state = st;
        s.area = ar;
    }
    // 篡改保存区一个字节（模拟中途被改写）。
    {
        let s = e.slot_mut(tid, region)?;
if let Some(w) = s.area.words.get_mut(0) { *w ^= 0xFFFF_FFFF_FFFF_FFFF; }
    }
    let cur = e.now();
    let broken = {
        let core = {
            let s = e.slot(tid, region)?;
            (s.area.clone(), s.state.clone()) };
        let (ar, mut st) = core;
        restore(&ar, &mut st, cur).is_err() };
if broken { e.note_roundtrip_break(); }
    e.note_inject(Injected {
        form: Form::RoundTripBroken, tick: e.now(), tid, region: region.index(), expect: "roundtrip_breaks=1",
    });
    Ok(if broken { 1 } else { 0 }) }
/// 形态 10：扩展状态账不守恒。
pub fn inject_ledger_unbalanced(e: &mut LazySaveEngine, tid: u64) -> LzResult<u64> {
    let region = Region::Sse;
    let s = e.slot_mut(tid, region)?;
    // 凭空多记一次保存，使 save != restore + lazy。
    s.ledger.save_count = s.ledger.save_count.wrapping_add(3);
    let dev = {
        let s = e.slot(tid, region)?;
        if s.ledger.balanced() || s.ledger.balanced_with_evicted() {
            0 } else {
            1 }
    };
    e.note_inject(Injected {
        form: Form::LedgerUnbalanced, tick: e.now(), tid, region: region.index(), expect: "ledger_deviations=1",
    });
    Ok(dev) }
/// 十形态全注入（任务单 F 项）。 返回 10 条注入记录；同时把检出证据填入传入的 `Evidence`。
pub fn inject_all(e: &mut LazySaveEngine) -> Vec<Injected> {
    // 十形态各用独立线程，避免相互污染证据。
    let mut tids: [u64; FORM_COUNT] = [0u64; FORM_COUNT];
    let mut i = 0usize;
    while i < FORM_COUNT {
        let tid = 0xF1902_0000_0000 + (i as u64);
        tids[i] = tid;
        // 全区使能的 XCR0：x87|SSE|AVX|AVX512
        let xcr0 = Region::X87.xcr0_bit() | Region::Sse.xcr0_bit() | Region::Avx.xcr0_bit() | Region::Avx512.xcr0_bit();
        let _ = e.admit(tid, xcr0);
        i = i.wrapping_add(1);
    }
    // 1
    let _ = inject_spurious_trap(e, tids[0]);
    // 2
    let _ = inject_reentry(e, tids[1]);
    // 3
    let _ = inject_ts_not_cleared(e, tids[2]);
    // 4
    let _ = inject_lazy_unconsumed(e, tids[3]);
    // 5
    let _ = inject_save_on_never(e, tids[4]);
    // 6
    let _ = inject_skip_on_always(e, tids[5]);
    // 7
    let _ = inject_missing_area_accepted(e, tids[6]);
    // 8
    let _ = inject_misaligned(e, tids[7]);
    // 9
    let _ = inject_roundtrip_broken(e, tids[8]);
    // 10
    let _ = inject_ledger_unbalanced(e, tids[9]);
    e.injected().to_vec() }
/// 从引擎抽取检出证据（独立扫描路径，不依赖注入台账）。
pub fn scan_evidence(e: &LazySaveEngine) -> Evidence {
    let mut ev = Evidence::zero();
    ev.spurious = e.metrics().spurious;
    ev.reentries = e.metrics().reentries;
    // TS 泄漏：任何线程在"非 Trap 非 Restore"状态下 bit 仍为 1 且无挂起。
    for t in e.threads.values() {
if t.ts.bit() == CR0_TS && t.ts.state != TsState::Trap && t.ts.state != TsState::Restore { ev.ts_leak = ev.ts_leak.wrapping_add(1); }
        for slot in t.slots.iter() {
            if let Some(l) = slot.lazy {
if e.now().wrapping_sub(l.since) > LAZY_STALE_TICKS && !l.in_window { ev.lazy_unconsumed = ev.lazy_unconsumed.wrapping_add(1); }
            }
if slot.policy == SavePolicy::Never && slot.ledger.save_count > 0 { ev.save_on_never = ev.save_on_never.wrapping_add(slot.ledger.save_count); }
            if slot.policy == SavePolicy::AlwaysOnSwitchOut && slot.state.dirty && !slot.area.valid && slot.conflicts > 0 {
                ev.skip_on_always = ev.skip_on_always.wrapping_add(1);
            }
if !slot.area.aligned_ok() { ev.misaligned = ev.misaligned.wrapping_add(1); }
if !slot.ledger.balanced() && !slot.ledger.balanced_with_evicted() { ev.ledger_deviations = ev.ledger_deviations.wrapping_add(1); }
        }
    }
    ev.roundtrip_breaks = e.metrics().roundtrip_breaks;
    // 形态 7 的证据来自引擎侧的放行计数（缺区仍放行即为漏洞）。
    ev.missing_area_accepted = e.metrics().missing_area_accepted;
    ev }
// ===========================================================================
// 13. 三态逐例判定（判据 10/10）
// ===========================================================================
/// 一条三态用例。
#[derive(Clone, Copy, Debug)] pub struct PolicyCase {
/// 用例序号（0..9）。
    pub index: usize,
/// 用例名。
    pub name: &'static str,
/// 策略态。
    pub policy: SavePolicy,
/// 极性：正例 / 反例。
    pub positive: bool,
/// 脏标记。
    pub dirty: bool,
/// XCR0 使能位。
    pub enabled: bool,
/// 期望切出动作。
    pub expect: Action,
/// 实际切出动作（引擎跑出来的）。
    pub actual: Action,
/// 期望的 TS 置位。
    pub expect_ts: bool,
/// 实际 TS 置位。
    pub actual_ts: bool,
/// 期望的保存区状态：`Some` 要求保存区有效。
    pub expect_area: bool,
/// 实际保存区是否有效。
    pub actual_area: bool,
/// 判定结果。
    pub pass: bool,
/// 切入侧附加断言名（`""` 表示本例不含切入断言）。
    pub probe: &'static str,
}
impl PolicyCase {
/// 一行描述。
    pub fn describe(&self) -> String {
        format!( "case#{}({}{}) {} dirty={} en={} act {}/{} ts {}/{} area {}/{} probe={}", self.index, if self.positive { "+" } else { "-" },
            self.name, if self.dirty { 1 } else { 0 }, if self.enabled { 1 } else { 0 }, self.expect.name(), self.actual.name(),
            if self.expect_ts { 1 } else { 0 }, if self.actual_ts { 1 } else { 0 }, if self.expect_area { 1 } else { 0 },
            if self.actual_area { 1 } else { 0 }, self.probe, if self.pass { "PASS" } else { "FAIL" }
        ) }
}
/// 用例规格（不含实际值；规格与执行严格分离）。
struct CaseSpec {
    name: &'static str, policy: SavePolicy, positive: bool, dirty: bool, enabled: bool, expect: Action, expect_ts: bool, expect_area: bool,
    probe: &'static str,
}
/// 十例清单的**规格**。 判据：UNX-F1902-J1 —— 三态各有专属正反例，共 10 例，逐例判定。 分布：`AlwaysOnSwitchOut` 4 例（含「不脏也存」的从严正例）、 `Lazy` 4 例（脏则推迟、净则跳过，各含正反例）、 `Never` 2 例（正：从不保存；反：切入缺区被拒）。 期望的推导口径与 `decide()` 同源但走另一条书写路径： - `SaveNow` ⇒ 切出后该区 `area.valid == true`； - `DeferToLazy` ⇒ 切出后该区 `area.valid == false` 且 `ts == Set`； - `Skip` ⇒ `area.valid == false` 且 `ts` 不因本区而置。
fn policy_case_specs() -> Vec<CaseSpec> {
    vec![ CaseSpec {
            name: "always_dirty_saves", policy: SavePolicy::AlwaysOnSwitchOut, positive: true, dirty: true, enabled: true, expect: Action::SaveNow,
            expect_ts: false, expect_area: true, probe: "",
        }, CaseSpec {
            name: "always_clean_also_saves", policy: SavePolicy::AlwaysOnSwitchOut, positive: true, dirty: false, enabled: true,
            expect: Action::SaveNow, expect_ts: false, expect_area: true, probe: "",
        }, CaseSpec {
            name: "always_ledger_grows", policy: SavePolicy::AlwaysOnSwitchOut, positive: true, dirty: true, enabled: true, expect: Action::SaveNow,
            expect_ts: false, expect_area: true, probe: "ledger_save+1",
        }, CaseSpec {
            name: "always_offscreen_strict", policy: SavePolicy::AlwaysOnSwitchOut, positive: false, dirty: true, enabled: false,
            // 未使能区按 F1901 裁断：无保存区可落，动作收敛为 Skip，
            // 但「承诺必存」与「无区可存」的冲突必须被记账。
            expect: Action::Skip, expect_ts: false, expect_area: false, probe: "conflict_recorded",
        }, CaseSpec {
            name: "lazy_dirty_defers", policy: SavePolicy::Lazy, positive: true, dirty: true, enabled: true, expect: Action::DeferToLazy,
            expect_ts: true, expect_area: false, probe: "",
        }, CaseSpec {
            name: "lazy_dirty_closed_loop", policy: SavePolicy::Lazy, positive: true, dirty: true, enabled: true, expect: Action::DeferToLazy,
            expect_ts: true, expect_area: false, probe: "nm_closure",
        }, CaseSpec {
            name: "lazy_clean_skips", policy: SavePolicy::Lazy, positive: true, dirty: false, enabled: true, expect: Action::Skip, expect_ts: false,
            expect_area: false, probe: "",
        }, CaseSpec {
            name: "lazy_disabled_skips", policy: SavePolicy::Lazy, positive: false, dirty: true, enabled: false, expect: Action::Skip,
            expect_ts: false, expect_area: false, probe: "",
        }, CaseSpec {
            name: "never_never_saves", policy: SavePolicy::Never, positive: true, dirty: true, enabled: true, expect: Action::Skip, expect_ts: false,
            expect_area: false, probe: "",
        }, CaseSpec {
            name: "never_switchin_refused", policy: SavePolicy::Never, positive: false, dirty: true, enabled: true, expect: Action::Skip,
            expect_ts: false, expect_area: false, probe: "switchin_refuse",
        }, ] }
/// 单区动作（从切出报告里取该区的实际动作）。
fn action_of(report: &SwitchOutReport, idx: usize) -> Action {
if report.saved.contains(&idx) { return Action::SaveNow; }
if report.deferred.contains(&idx) { return Action::DeferToLazy; }
    Action::Skip }
/// 跑三态十例（每例一个独立线程，跑真实切出路径取实际值）。 判据：UNX-F1902-J1 —— 逐态正确 10/10。
pub fn run_policy_cases(seed: u64) -> Vec<PolicyCase> {
    let mut out: Vec<PolicyCase> = Vec::new();
    for (i, spec) in policy_case_specs().into_iter().enumerate() {
        let tid = 0xC0DE_0000_0000_0001 + (i as u64);
        // XCR0 位图：使能位为真时全开，为假时全关（验证裁断路径）。
        let xcr0 = if spec.enabled {
            Region::X87.xcr0_bit() | Region::Sse.xcr0_bit() | Region::Avx.xcr0_bit() | Region::Avx512.xcr0_bit() } else {
            0 };
        let mut e = LazySaveEngine::new(seed.wrapping_add(i as u64));
        let _ = e.admit(tid, xcr0);
        // 全区统一设定策略：本例只观测 x87 区（idx 0）。
for r in Region::ALL { let _ = e.set_policy(tid, r, spec.policy); }
if spec.dirty { let _ = e.touch_n(tid, Region::X87, 2); }
        let mut extra_ok = true;
        let report = e.switch_out(tid);
        let (actual, actual_ts, actual_area) = match report {
            Ok(r) => {
                let a = action_of(&r, 0);
                let area = e .slot(tid, Region::X87) .map(|s| s.area.valid) .unwrap_or(false);
                (a, r.ts_set, area) }
            Err(_) => (Action::Skip, false, false),
        };
        // 附加探针。
        match spec.probe {
            "ledger_save+1" => {
                extra_ok = e .slot(tid, Region::X87) .map(|s| s.ledger.save_count == 1) .unwrap_or(false);
            }
            "conflict_recorded" => {
                extra_ok = e .slot(tid, Region::X87) .map(|s| s.conflicts > 0) .unwrap_or(false);
            }
            "nm_closure" => {
                // 切出后首次执行 → #NM → 保存 → 清 TS。
                let fired = e.exec_fp(tid, Region::X87).unwrap_or(ExecOutcome::Rejected);
                let tr = e.enter_nm(tid);
                extra_ok = fired == ExecOutcome::TrapNm && matches!(&tr, Ok(x) if x.closed && x.saves_done == 1)
                    && e .thread(tid) .map(|t| t.ts.state == TsState::Clear && t.ts.bit() == 0) .unwrap_or(false) && e .slot(tid, Region::X87)
                        .map(|s| s.lazy.is_none() && s.area.valid) .unwrap_or(false);
            }
            "switchin_refuse" => {
                // Never + 脏 + 无有效保存区 → 切入必须被拒（不静默）。
                extra_ok = matches!(e.switch_in(tid), Ok(x) if x.refused);
            }
            _ => {}
        }
        let pass = actual == spec.expect && actual_ts == spec.expect_ts && actual_area == spec.expect_area && extra_ok;
        out.push(PolicyCase {
            index: i, name: spec.name, policy: spec.policy, positive: spec.positive, dirty: spec.dirty, enabled: spec.enabled, expect: spec.expect,
            actual, expect_ts: spec.expect_ts, actual_ts, expect_area: spec.expect_area, actual_area, pass, probe: spec.probe,
        });
    }
    out }
/// 三态十例全过（10/10）。
pub fn policy_cases_ok(seed: u64) -> bool {
    let c = run_policy_cases(seed);
if c.len() != POLICY_CASE_COUNT { return false; }
    let mut n = 0usize;
for x in c.iter() { if x.pass { n = n.wrapping_add(1); } }
    n == POLICY_CASE_COUNT }
/// 三态分布计数（按十例规格统计）。
pub fn policy_case_distribution() -> [usize; 3] {
    let mut out = [0usize; 3];
for s in policy_case_specs() { out[s.policy.index()] = out[s.policy.index()].wrapping_add(1); }
    out }
/// 三态十例描述行。
pub fn policy_case_rows(c: &[PolicyCase]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
for x in c { v.push(x.describe()); }
    v }
// ===========================================================================
// 14. 闭环断言（TS 置位 → #NM → 恢复/清位）
// ===========================================================================
/// 闭环断言结果。
#[derive(Clone, Debug)] pub struct ClosureReport {
/// 全程 TS 状态轨迹（描述）。
    pub ts_trace: Vec<String>,
/// 是否进入过 Trap 态。
    pub entered_trap: bool,
/// 出口 TS 是否为 Clear。
    pub exit_cleared: bool,
/// 惰性记录是否被消费完。
    pub lazy_consumed: bool,
/// 原指令是否恰好重执行一次。
    pub restart_once: bool,
/// 账是否守恒。
    pub ledger_ok: bool,
/// 综合闭环结论。
    pub closed: bool,
}
impl ClosureReport {
/// 一行描述。
    pub fn describe(&self) -> String {
        format!( "closure(trap={} cleared={} lazy={} restart={} ledger={} => {})", if self.entered_trap { 1 } else { 0 },
            if self.exit_cleared { 1 } else { 0 }, if self.lazy_consumed { 1 } else { 0 }, if self.restart_once { 1 } else { 0 },
            if self.ledger_ok { 1 } else { 0 }, if self.closed { "CLOSED" } else { "OPEN" }
        ) }
}
/// 跑一次完整闭环：切出（脏+惰性）→ 首次执行浮点 → #NM → 保存 → 清 TS → 重执行。 判据：UNX-F1902-J1 —— 主路径断言。
pub fn run_closure(seed: u64) -> ClosureReport {
    let tid = 0xC105_0000_0000_0001;
    let xcr0 = Region::X87.xcr0_bit() | Region::Sse.xcr0_bit();
    let mut e = LazySaveEngine::new(seed);
    let mut ts_trace: Vec<String> = Vec::new();
    let _ = e.admit(tid, xcr0);
    let _ = e.set_policy(tid, Region::Sse, SavePolicy::Lazy);
    let _ = e.touch_n(tid, Region::Sse, 3);
if let Ok(t) = e.thread(tid) { ts_trace.push(t.ts.describe()); }
    let _ = e.switch_out(tid);
if let Ok(t) = e.thread(tid) { ts_trace.push(t.ts.describe()); }
    let outcome = e.exec_fp(tid, Region::Sse).unwrap_or(ExecOutcome::Rejected);
    let entered_trap = outcome == ExecOutcome::TrapNm;
    let tr = e.enter_nm(tid);
    let mut restart_once = false;
if let Ok(t) = tr { restart_once = t.restart == 1 && t.closed; }
    let t = match e.thread(tid) {
        Ok(t) => t, Err(_) => {
            return ClosureReport {
                ts_trace, entered_trap, exit_cleared: false, lazy_consumed: false, restart_once: false, ledger_ok: false, closed: false,
            }
        }
    };
    ts_trace.push(t.ts.describe());
    let exit_cleared = t.ts.state == TsState::Clear && t.ts.bit() == 0;
    let mut lazy_consumed = true;
for slot in t.slots.iter() { if slot.lazy.is_some() { lazy_consumed = false; } }
    let ledger_ok = t.ledger_ok();
    ClosureReport {
        ts_trace, entered_trap, exit_cleared, lazy_consumed, restart_once, ledger_ok,
        closed: entered_trap && exit_cleared && lazy_consumed && restart_once && ledger_ok,
    }
}
/// 闭环全过。
pub fn closure_ok(seed: u64) -> bool {
    run_closure(seed).closed }
/// 跑一段**干净生命周期**（三态各一线程，全部走完切出→闭环→切入）， 返回观测行与引擎引用。用于在不注入畸形的前提下判定布局 / 账不变量。
pub fn run_lifecycle(seed: u64) -> (Vec<String>, LazySaveEngine) {
    let mut e = LazySaveEngine::new(seed);
    let mut log: Vec<String> = Vec::new();
    let xcr0 = Region::X87.xcr0_bit() | Region::Sse.xcr0_bit() | Region::Avx.xcr0_bit();
    // A：Always（脏 → 切出即存 → 切入恢复）
    let a = 0x11A0_0000_0000_0001u64;
    let _ = e.admit(a, xcr0);
for r in Region::ALL { let _ = e.set_policy(a, r, SavePolicy::AlwaysOnSwitchOut); }
    let _ = e.touch_n(a, Region::Sse, 3);
if let Ok(r) = e.switch_out(a) { log.push(r.describe()); }
    // B：Lazy（脏 → 切出不存 → 首次使用 #NM → 保存 → 清 TS → 切入恢复）
    let b = 0x11B0_0000_0000_0002u64;
    let _ = e.admit(b, xcr0);
for r in Region::ALL { let _ = e.set_policy(b, r, SavePolicy::Lazy); }
    let _ = e.touch_n(b, Region::Sse, 3);
if let Ok(r) = e.switch_out(b) { log.push(r.describe()); }
    log.push(format!("exec_fp(B)={}", e.exec_fp(b, Region::Sse).unwrap_or(ExecOutcome::Rejected).name()));
if let Ok(t) = e.enter_nm(b) { log.push(t.describe()); }
if let Ok(r) = e.switch_in(b) { log.push(r.describe()); }
    // C：Never（承诺不触碰浮点 → 从不保存，切入放行）
    let c = 0x11C0_0000_0000_0003u64;
    let _ = e.admit(c, xcr0);
for r in Region::ALL { let _ = e.set_policy(c, r, SavePolicy::Never); }
if let Ok(r) = e.switch_out(c) { log.push(r.describe()); }
if let Ok(r) = e.switch_in(c) { log.push(r.describe()); }
if let Ok(r) = e.switch_in(a) { log.push(r.describe()); }
    (log, e) }
/// 多线程闭环巡检（`n` 个线程各跑一次闭环）。
pub fn closure_sweep(seed: u64, n: usize) -> Vec<ClosureReport> {
    let mut out: Vec<ClosureReport> = Vec::new();
    let mut i = 0usize;
while i < n { out.push(run_closure(seed.wrapping_add(lcg_step(i as u64)))); i = i.wrapping_add(1); }
    out }
// ===========================================================================
// 15. 摊销窗协同（E 项）
// ===========================================================================
/// 窗内惰性挂起测试结果。
#[derive(Clone, Debug)] pub struct WindowProbe {
/// 开窗是否成功。
    pub opened: bool,
/// 非深度区开窗是否被拒。
    pub shallow_refused: bool,
/// 窗内挂起是否被豁免。
    pub exempt: bool,
/// 窗闭后挂起是否仍被豁免（应为否）。
    pub exempt_after_close: bool,
/// 窗闭强制落盘计数。
    pub forced_flush: u64,
}
impl WindowProbe {
/// 一行描述。
    pub fn describe(&self) -> String {
        format!( "window(open={} shallow_refused={} exempt={} after_close={} flush={})", if self.opened { 1 } else { 0 },
            if self.shallow_refused { 1 } else { 0 }, if self.exempt { 1 } else { 0 }, if self.exempt_after_close { 1 } else { 0 },
            self.forced_flush ) }
}
/// 跑窗协同探针。
pub fn run_window_probe(seed: u64) -> WindowProbe {
    let tid = 0xA0C0_0000_0000_0001;
    let xcr0 = Region::Avx512.xcr0_bit();
    let mut e = LazySaveEngine::new(seed);
    let _ = e.admit(tid, xcr0);
    let _ = e.set_policy(tid, Region::Avx512, SavePolicy::Lazy);
    let shallow_refused = e.open_window(tid, Region::Sse).is_err();
    let opened = e.open_window(tid, Region::Avx512).is_ok();
    // 窗内挂起一次。
    let _ = e.touch_n(tid, Region::Avx512, 2);
    let _ = e.switch_out(tid);
    let exempt = {
        let t = match e.thread(tid) {
            Ok(t) => t, Err(_) => {
                return WindowProbe {
                    opened, shallow_refused, exempt: false, exempt_after_close: false, forced_flush: 0,
                }
            }
        };
        let mut n = false;
for slot in t.slots.iter() { if let Some(l) = slot.lazy { if l.in_window { n = true; } } }
        n };
    let forced_flush = e.close_window(tid).unwrap_or(0);
    let exempt_after_close = {
        let t = match e.thread(tid) {
            Ok(t) => t, Err(_) => {
                return WindowProbe {
                    opened, shallow_refused, exempt, exempt_after_close: false, forced_flush,
                }
            }
        };
        let mut n = false;
for slot in t.slots.iter() { if t.window.holds(slot.lazy.is_some()) { n = true; } }
        n };
    WindowProbe {
        opened, shallow_refused, exempt, exempt_after_close, forced_flush,
    }
}
// ===========================================================================
// 16. 观测导出（G 项）
// ===========================================================================
/// 观测包（导出聚合体）。
#[derive(Clone, Debug)] pub struct Observation {
/// 指标。
    pub metrics: Metrics,
/// 三态分布。
    pub policy_dist: [usize; 3],
/// 判定表通过数（0..12）。
    pub matrix_pass: usize,
/// 三态用例通过数（0..10）。
    pub cases_pass: usize,
/// 往返保真通过数（0..ROUNDTRIP_SAMPLES）。
    pub roundtrip_pass: usize,
/// 十形态检出结论。
    pub detections: Vec<Detection>,
/// 检出通过数（0..10）。
    pub detect_pass: usize,
/// 闭环结论。
    pub closure: ClosureReport,
/// 窗探针结论。
    pub window: WindowProbe,
/// 布局不变量。
    pub layout_ok: bool,
/// 账守恒。
    pub ledger_ok: bool,
}
impl Observation {
/// 全过（判据总闸）。
    pub fn all_ok(&self) -> bool {
        self.matrix_pass == MATRIX_CELLS && self.cases_pass == POLICY_CASE_COUNT
            && self.roundtrip_pass == ROUNDTRIP_SAMPLES && self.detect_pass == FORM_COUNT && self.closure.closed && self.layout_ok && self.ledger_ok }
/// 一行摘要。
    pub fn summary(&self) -> String {
        format!( "matrix={}/{} cases={}/{} roundtrip={}/{} detect={}/{} layout={} ledger={} closure={}", self.matrix_pass, MATRIX_CELLS,
            self.cases_pass, POLICY_CASE_COUNT, self.roundtrip_pass, ROUNDTRIP_SAMPLES, self.detect_pass, FORM_COUNT,
            if self.layout_ok { 1 } else { 0 }, if self.ledger_ok { 1 } else { 0 }, if self.closure.closed { 1 } else { 0 }
        ) }
}
/// 采一份观测（走完整流程）。
pub fn observe(seed: u64) -> Observation {
    let matrix = decision_matrix();
    let mut matrix_pass = 0usize;
for c in matrix.iter() { if c.pass { matrix_pass = matrix_pass.wrapping_add(1); } }
    let cases = run_policy_cases(seed);
    let mut cases_pass = 0usize;
for c in cases.iter() { if c.pass { cases_pass = cases_pass.wrapping_add(1); } }
    let rt = roundtrip_matrix(seed);
    let mut roundtrip_pass = 0usize;
for r in rt.iter() { if r.saved && r.restored && r.zero_diff { roundtrip_pass = roundtrip_pass.wrapping_add(1); } }
    // 注入 + 检出。
    let mut e = LazySaveEngine::new(seed);
    let _ = inject_all(&mut e);
    let ev = scan_evidence(&mut e);
    let detections = detect_all(&ev);
    let mut detect_pass = 0usize;
for d in detections.iter() { if d.hit { detect_pass = detect_pass.wrapping_add(1); } }
    let closure = run_closure(seed);
    let window = run_window_probe(seed);
    // 布局 / 账不变量必须在**干净生命周期**引擎上判定：注入引擎按设计
    // 就是畸形现场，其账必然不守恒（那正是形态 10 的证据）。
    let clean = run_lifecycle(seed);
    let layout_ok = clean.1.layout_ok();
    let ledger_ok = clean.1.ledger_ok();
    let metrics = e.metrics();
    Observation {
        metrics, policy_dist: policy_case_distribution(), matrix_pass, cases_pass, roundtrip_pass, detections, detect_pass, closure, window,
        layout_ok, ledger_ok,
    }
}
/// 导出键值文本（纯字符串，无 IO）。
pub fn export_kv(ob: &Observation) -> String {
    let mut s = String::new();
    s.push_str("UNX-F1902.lazysave.v1\n");
    s.push_str(&format!("version={}\n", V1902_VERSION));
    s.push_str(&format!("seg=0x{:04X}..0x{:04X} main=0x{:04X}\n", V1902_SEG_BASE, V1902_SEG_END, V1902_MAIN_CODE));
    s.push_str(&format!("nm_vector={}\n", NM_VECTOR));
    s.push_str(&format!("save_align={}\n", SAVE_ALIGN));
    s.push_str(&format!("region_count={}\n", REGION_COUNT));
    // 三态分布
    s.push_str(&format!( "policy.always={} policy.lazy={} policy.never={} policy.total={}\n", ob.policy_dist[0], ob.policy_dist[1], ob.policy_dist[2],
        ob.policy_dist[0] + ob.policy_dist[1] + ob.policy_dist[2] ));
for (k, v) in ob.metrics.kv() { s.push_str(&format!("m.{k}={v}\n")); }
    s.push_str(&format!("rate.lazy_hit_permille={}\n", ob.metrics.lazy_hit_permille()));
    s.push_str(&format!( "rate.immediate_permille={}\n", ob.metrics.immediate_rate_permille() ));
    s.push_str(&format!("assert.matrix={}/{}\n", ob.matrix_pass, MATRIX_CELLS));
    s.push_str(&format!("assert.cases={}/{}\n", ob.cases_pass, POLICY_CASE_COUNT));
    s.push_str(&format!( "assert.roundtrip={}/{}\n", ob.roundtrip_pass, ROUNDTRIP_SAMPLES ));
    s.push_str(&format!("assert.detect={}/{}\n", ob.detect_pass, FORM_COUNT));
    s.push_str(&format!("assert.layout={}\n", if ob.layout_ok { 1 } else { 0 }));
    s.push_str(&format!("assert.ledger={}\n", if ob.ledger_ok { 1 } else { 0 }));
    s.push_str(&format!("closure={}\n", ob.closure.describe()));
    s.push_str(&format!("window={}\n", ob.window.describe()));
for d in ob.detections.iter() { s.push_str(&format!("detect.{}.{}\n", d.form.name(), d.form.code())); }
    s.push_str(&format!("fp={}\n", ob.metrics.fingerprint()));
    s }
/// TLV 标签。
pub mod tlv_tag {
/// 头（魔数 + 版本 + 长度）。
    pub const HEAD: u8 = 0x01;
/// 状态区描述符。
    pub const DESC: u8 = 0x10;
/// 区域描述符。
    pub const REGION: u8 = 0x11;
/// 扩展状态账。
    pub const LEDGER: u8 = 0x20;
/// 指标。
    pub const METRIC: u8 = 0x21;
/// 十形态检出。
    pub const DETECT: u8 = 0x30;
/// 指纹。
    pub const FINGERPRINT: u8 = 0x7F;
}
/// TLV 头长（tag 1 + len 2 + payload 24 = magic 8 + version 8 + len 8）。
pub const TLV_HEAD_LEN: usize = 1 + 2 + 24;
/// 导出 TLV 字节流。 结构：`HEAD(0x01) + [len] + payload16` 后接若干 `(tag:u8, len:u16, payload)`。 长度自洽由 `tlv_len_ok()` 回读校验。
pub fn export_tlv(ob: &Observation, sample: &ThreadSlot) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    // 状态区描述符
    push_tlv(&mut body, tlv_tag::DESC, &encode_desc(sample.xcr0, sample.ts.bit(), sample.ts.state.index() as u64));
    // 各区描述符
for slot in sample.slots.iter() { push_tlv( &mut body, tlv_tag::REGION, &encode_region_desc(&slot.state, slot.policy), ); }
    // 扩展状态账
for row in sample.ledger_rows() { let bytes: Vec<u8> = row.into_bytes(); push_tlv(&mut body, tlv_tag::LEDGER, &bytes); }
    // 指标
    for (k, v) in ob.metrics.kv() {
        let mut p: Vec<u8> = Vec::new();
        p.push((k.len() & 0xFF) as u8);
for b in k.as_bytes() { p.push(*b); }
        push_u64(&mut p, v);
        push_tlv(&mut body, tlv_tag::METRIC, &p);
    }
    // 十形态检出
    for d in ob.detections.iter() {
        let mut p: Vec<u8> = Vec::new();
        push_u32(&mut p, d.form.code() as u32);
        push_u32(&mut p, d.hits as u32);
        p.push(if d.hit { 1u8 } else { 0u8 });
for b in d.form.name().as_bytes() { p.push(*b); }
        push_tlv(&mut body, tlv_tag::DETECT, &p);
    }
    // 指纹
    let fp_words: [u64; 3] = [ob.metrics.fingerprint(), V1902_VERSION_ID, ob.metrics.fingerprint() ^ V1902_SEG_BASE as u64];
    let mut fp_bytes: Vec<u8> = Vec::new();
for w in fp_words { push_u64(&mut fp_bytes, w); }
    push_tlv(&mut body, tlv_tag::FINGERPRINT, &fp_bytes);
    // 组头
    let mut out: Vec<u8> = Vec::with_capacity(body.len() + TLV_HEAD_LEN);
    let head_payload_len: u64 = body.len() as u64;
    let mut head_payload: Vec<u8> = Vec::new();
    push_u64(&mut head_payload, 0x5645_5853_3232_4445); // magic
    push_u64(&mut head_payload, V1902_VERSION_ID);
    push_u64(&mut head_payload, head_payload_len);
    push_tlv(&mut out, tlv_tag::HEAD, &head_payload);
    out.extend_from_slice(&body);
    out }
/// 追加一条 TLV。
pub fn push_tlv(out: &mut Vec<u8>, tag: u8, payload: &[u8]) {
    out.push(tag);
    let n = payload.len() as u64;
    out.push(((n >> 8) & 0xFF) as u8);
    out.push((n & 0xFF) as u8);
    let mut i = 0usize;
while i < payload.len() { if let Some(b) = payload.get(i) { out.push(*b); } i = i.wrapping_add(1); }
}
/// TLV 长度自洽断言。
pub fn tlv_len_ok(buf: &[u8]) -> bool {
if buf.len() < TLV_HEAD_LEN { return false; }
if buf.first().copied() != Some(tlv_tag::HEAD) { return false; }
    // HEAD 载荷第三字（偏移 3+16）声明的后续字节数。
    let declared = read_u64(buf, 3 + 16) as usize;
    let mut off = TLV_HEAD_LEN;
    let mut walked = 0usize;
    while off < buf.len() {
        let tag = match buf.get(off) {
            Some(t) => *t, None => return false,
        };
if tag == 0 { return false; }
        let len = read_u16(buf, off + 1) as usize;
        off = off.wrapping_add(3);
if off.wrapping_add(len) > buf.len() { return false; }
        off = off.wrapping_add(len);
        walked = walked.wrapping_add(1);
    }
if off != buf.len() { return false; }
    // 首条 HEAD 载荷长度必须等于后续全部字节数。
if declared.wrapping_add(TLV_HEAD_LEN) != buf.len() { return false; }
    walked >= 1 }
/// 采样线程（供 TLV 导出演示用）。
pub fn sample_slot(xcr0: u64) -> ThreadSlot {
    ThreadSlot::new(0xF190_2000_0000_FFFF, xcr0) }
// ===========================================================================
// 17. 自检套件
// ===========================================================================
/// 一条自检项。
#[derive(Clone, Copy, Debug)] pub struct Check {
/// 项号。
    pub id: &'static str,
/// 通过与否。
    pub pass: bool,
/// 说明。
    pub note: &'static str,
}
/// 自检套件（判据总闸）。 判据：UNX-F1902-J1 —— 决策路径断言全过 + 三态逐态 10/10 + 十形态 10/10。
pub fn self_check(seed: u64) -> Vec<Check> {
    let mut out: Vec<Check> = Vec::new();
    // C1 判定表 12 格
    out.push(Check {
        id: "C1902-01 matrix12", pass: decision_matrix_ok(), note: "3x2x2=12 格全穷举判定",
    });
    // C2 三态逐态 10 例
    let cases = run_policy_cases(seed);
    let mut n = 0usize;
for c in cases.iter() { if c.pass { n = n.wrapping_add(1); } }
    out.push(Check {
        id: "C1902-02 cases10", pass: n == POLICY_CASE_COUNT, note: "三态各有正反例共 10 例",
    });
    // C3 闭环
    let cl = run_closure(seed);
    out.push(Check {
        id: "C1902-03 closure", pass: cl.closed, note: "TS置位→#NM→保存→清TS→重执行",
    });
    // C4 往返保真
    out.push(Check {
        id: "C1902-04 roundtrip", pass: roundtrip_matrix_ok(seed), note: "样本矩阵逐字段零差",
    });
    // C5 十形态检出
    let mut e = LazySaveEngine::new(seed);
    let _ = inject_all(&mut e);
    let ev = scan_evidence(&mut e);
    out.push(Check {
        id: "C1902-05 detect10", pass: detect_all_ok(&ev), note: "十形态逐一检出",
    });
    // C6 布局不变量（干净生命周期引擎，注入引擎按设计即畸形现场）
    let clean = run_lifecycle(seed);
    out.push(Check {
        id: "C1902-06 layout", pass: clean.1.layout_ok() && clean.1.ledger_ok(), note: "L1..L5 布局不变量 + 干净账守恒",
    });
    // C7 窗协同
    let wp = run_window_probe(seed);
    out.push(Check {
        id: "C1902-07 window", pass: wp.opened && wp.shallow_refused && wp.exempt, note: "窗内挂起豁免、非深度区拒开",
    });
    // C8 TLV 长度自洽
    let ob = observe(seed);
    let buf = export_tlv(&ob, &sample_slot(0b111_1011));
    out.push(Check {
        id: "C1902-08 tlv", pass: tlv_len_ok(&buf), note: "TLV 长度回读自洽",
    });
    // C9 KV 非空
    let kv = export_kv(&ob);
    out.push(Check {
        id: "C1902-09 kv", pass: kv.contains("UNX-F1902") && kv.contains("assert.detect"), note: "KV 导出含键",
    });
    // C10 总闸
    out.push(Check {
        id: "C1902-10 gate", pass: ob.all_ok(), note: "综合总闸",
    });
    out }
/// 自检全过。
pub fn self_check_ok(seed: u64) -> bool {
    let c = self_check(seed);
for x in c.iter() { if !x.pass { return false; } }
    true }
/// 自检报告串。
pub fn self_check_report(seed: u64) -> String {
    let c = self_check(seed);
    let mut s = String::new();
for x in c.iter() { s.push_str(&format!( "{} [{}] {}\n", x.id, if x.pass { "PASS" } else { "FAIL" }, x.note )); }
    s }
/// 十形态清单串（汇报用）。
pub fn form_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
for f in Form::ALL { v.push(format!( "#{} 0x{:04X} {} — {}", f.index(), f.code(), f.name(), f.describe() )); }
    v }
/// 核心不变量清单串（汇报用）。
pub fn invariant_manifest() -> Vec<String> {
    vec![ "INV-1 决策表 12 格：expect_action 与 decide 双实现必须一致".to_string(), "INV-2 AlwaysOnSwitchOut 恒 SaveNow（不脏也存，策略从严）".to_string(),
        "INV-3 Lazy 脏→DeferToLazy 且必置 TS，净→Skip 且不置 TS".to_string(), "INV-4 Never 恒 Skip；切入缺有效保存区必须 refused（不静默）".to_string(),
        "INV-5 #NM 进入必有 LazyPending 记录，出口该记录被消费且 TS=0".to_string(), "INV-6 陷阱处理态不得再陷入 #NM（重入禁令，F1848 同法）".to_string(),
        "INV-7 惰性保存时保存区指纹必须等于登记时指纹".to_string(), "INV-8 save 必 64 字节对齐，否则 0xB147".to_string(), "INV-9 restore 后载荷指纹与保存区逐位相等，且脏位清零".to_string(),
        "INV-10 账守恒 save == restore + lazy_pending（+ evicted）".to_string(), "INV-11 摊销窗内 LazyPending 豁免「丢失」判定，窗闭后不再豁免".to_string(),
        "INV-12 非深度区请求开摊销窗必须 0xB14D".to_string(), "INV-13 布局：使能区 off 连续、size==aligned_size、未使能区零占位".to_string(),
        "INV-14 TLV 导出长度回读必须自洽（HEAD 载荷长 == 后续字节数）".to_string(), ] }
/// 三态 10 例判定结果串（汇报用）。
pub fn policy_case_report(seed: u64) -> String {
    let cases = run_policy_cases(seed);
    let mut s = String::new();
for c in cases.iter() { s.push_str(&c.describe()); s.push('\n'); }
    let n = cases.iter().filter(|c| c.pass).count();
    s.push_str(&format!("policy_cases={}/10\n", n));
    s }
/// 十形态判定结果串（汇报用）。
pub fn detect_report(seed: u64) -> String {
    let mut e = LazySaveEngine::new(seed);
    let _ = inject_all(&mut e);
    let ev = scan_evidence(&e);
    let d = detect_all(&ev);
    let mut s = String::new();
for x in d.iter() { s.push_str(&x.describe()); s.push('\n'); }
    let n = d.iter().filter(|x| x.hit).count();
    s.push_str(&format!("detect={}/10\n", n));
    s }
// ===========================================================================
// 18. 全流程演示场景（可观测轨迹）
// ===========================================================================
/// 跑一个多线程混合场景：惰性 / 必存 / 永存三态各自走一遍完整生命周期。
pub fn demo_scene(seed: u64) -> Vec<String> {
    run_lifecycle(seed).0 }
/// 场景账快照串。
pub fn demo_ledger(seed: u64) -> String {
    let (_, e) = run_lifecycle(seed);
    let mut out = String::new();
    for tid in e.tids() {
        if let Ok(t) = e.thread(tid) {
            out.push_str(&t.describe());
            out.push('\n');
for row in t.ledger_rows() { out.push_str("  "); out.push_str(&row); out.push('\n'); }
        }
    }
    out }
// ===========================================================================
// 19. 版本锚
// ===========================================================================
/// 本单判据编号。
pub const CRITERION_ID: &str = "UNX-F1902-J1";
/// 本单判据原文。
pub const CRITERION_TEXT: &str = "UNX-F1902-J1 惰性保存决策路径断言全过（TS 置位→首次使用 #NM 陷阱→恢复/清位闭环），保存时机三态（切出必存/惰性/永存）逐态正确 10/10";
/// 本单册内关联单号。
pub const RELATED_TICKETS: [&str; 8] = [ "F1827", "F1824", "F1825", "F1826", "F1805", "F1806", "F1903", "F1908", ];
/// 关联单串。
pub fn related_str() -> String {
    let mut s = String::new();
for t in RELATED_TICKETS.iter() { s.push_str(t); s.push(' '); }
    s }
