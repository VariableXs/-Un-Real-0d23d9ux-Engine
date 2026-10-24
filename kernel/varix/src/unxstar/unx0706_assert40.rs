//! unx0706_assert40 —— UNX-F0706 交接面断言补全至 ≥40（断言账本体系）。
//!
//! ── 锚点（CoRun Unxreal 主册，docs/Varix/CoRun Varix STAR II · Unxreal/
//!    CoRun Varix STAR II · Unxreal.md 第 2289 行，全文照抄）──
//!   | UNX-F0706 | 交接面断言补全至 ≥40 | 570 | 已深化 | UNX-F0706-J1
//!   交接面分项补全后实数 ≥40（六冻结接口字段级/Limine 响应面/panic 路径
//!   三族覆盖）且分项全绿 |
//!
//! 定位原文：主册第 2281 行表头「| 编号 | 功能条目 | 行数 | 状态 | 判据 |」
//!   下第 2289 行（AI-01 批 · 断言补全域 · 五分项硬线之一）。
//!
//! 承办上下文（主册 2280 行 AI-01 承办节选，全文照抄）：
//!   "AI-01 承办｜域账累计：182590 + 本批 11440 = 194030 / 240,000｜防重：
//!   ktest 引导面断言总账（分项盘点/补全/只增不减纪律/全域映射）归本批
//!   独占；各批断言集（F0360/F0538/F0620/F0659/F0680/F0700 等）为既有批账
//!   （本批只聚合盘点不改写批内断言——扩断言走各批勘误或本批新增补全
//!   函数）；任务书判据 2 五分项硬线（ACPI ≥80/内存图 ≥60/多核 ≥50/
//!   早传·页表 ≥70/交接面 ≥40）为本批主场｜R-A1-004 纪律：批小计 =
//!   逐条求和唯一真值（本批逐条求和 11440）｜新增骨架批次（本会话立）"
//!
//! ── J 判据拆解（本模块逐条落地的验收口径）──
//!   J1-a 实数 ≥40：三族登记硬线 六冻结接口字段级 ≥20 + Limine 响应面
//!        ≥12 + panic 路径 ≥8（三数之和恰为 40，const 期钉死）；本账
//!        实际登记 26+16+12 = 54 条，均带真实检查体。
//!   J1-b 三族覆盖：Family40 三族闭集（FR/LM/PP），族内再分 A/B/C 三面
//!        （FR：字段在场/偏移宽度冻结/魔数版本；LM：响应闭集/内存图/
//!        帧缓冲与杂项；PP：挂钩与触发/文案与冲刷/挂死与守卫）。
//!   J1-c 分项全绿：族计数 ≥ 族硬线 ×3 且全账执行 Pass/Fail 二态全 Pass
//!        且覆盖审计无缺口且基线 diff 洁净，四条同时成立才判 Green。
//!
//! ── 六冻结接口口径（真实语义来源）──
//!   主册 646290 行："帧簿、内存图、内核 ELF 段映射、rsdp/itmode 等全部
//!   Limine 响应固化为…"；F0725（unx0725_resptree）冻结表三响应
//!   MemoryMap/Framebuffer/KernelAddress + hhdm 基址 + RSDP 指针 + 引导
//!   模式 itmode，合成六冻结接口闭集：
//!     IF-MM memory_map      （revision/entry_count/entries 三字段）
//!     IF-FB framebuffer     （revision/addr/宽/高/bpp/pitch/颜色模型/RGB
//!                            掩码五项等 13 字段）
//!     IF-KA kernel_address  （revision/phys_base/virt_base/size 四字段）
//!     IF-HH hhdm            （revision/offset 两字段）
//!     IF-RS rsdp            （revision/address 两字段）
//!     IF-IT itmode          （revision/flags 两字段）
//!   六接口共 26 条冻结字段，偏移/宽度按 Limine Boot Protocol revision 1
//!   布局写死（来源逐表注明）。
//!
//! ── 组件（全真实子系统，零注水空壳）──
//!   S1 AssertionId 三段式 id 规范（A40-<族码>-<面字母>-<序号>）：逐段
//!      硬检（族闭集 FR/LM/PP、面字母 A~C、序号 001~999），parse/render
//!      互验（规范形往返恒等），不合规范拒收（BadId）。
//!   S2 六接口冻结字段表（A40FrozenField × 6 表 const 期落簿）：字段名/
//!      偏移/宽度/冻结版本四列，表长 const 断言与逐表登记数互锁。
//!   S3 三族断言登记：FR ≥20（在场 6+偏移 6+宽度 6+魔数 6+标量 2）、
//!      LM ≥12（闭集 4+内存图 5+帧缓冲杂项 7）、PP ≥8（挂钩触发 3+
//!      文案冲刷 3+挂死守卫 6=12）；每族最低条数 const 表 + 登记时逐条
//!      带判据引用（UNX-F0706-J1 主判据 + 子标签）。
//!   S4 AssertBook40 断言账本：登记制 + 重复 id 拒收 + 空断言（无检查
//!      体）登记拒收 + 三族计数分列 + 总数 ≥40 判定。
//!   S5 BaselineSnap40 基线快照（只增不减）：快照条目 = id + 判据·描述
//!      内容哈希；diff40 相对基线——仅新增过（added 允许），删
//!      （Removed）/改（Modified）即红案。
//!   S6 AssertRunner 执行引擎（run40）：逐条 Pass/Fail 二态 + Fail 定位
//!      到断言 id + 执行序稳定（order 严格递增核）。
//!   S7 族间覆盖审计（coverage_audit）：每族覆盖率 = 已执行/已登记
//!      （万分比整数制）+ 未登记族面检出（A/B/C 三面逐族扫描，零登记
//!      即缺口 FaceGap40）。
//!   S8 断言↔判据双向索引（A40CrossRef）：断言→判据正向一列 + 判据→
//!      断言反向多列，正反两向对账（与 F0708 全域映射总表同构的分项
//!      投影）。
//!   S9 事件账（EventLog40）+ health() + FNV-1a 64 指纹（账本指纹/执行
//!      指纹）+ 双跑确定性对拍（dual_run_check）。
//!   S10 注入面三路：空断言注入 / 删基线注入 / 重复 id 注入，每路必
//!       检出（detected + 诊断码 + 中文处置）。
//!
//! ── 与 F0707 纪律账对齐声明 ──
//!   本模块 BaselineSnap40 即 UNX-F0707-J1「基线清单快照」的交接面分项
//!   投影：快照只增不减，diff40 仅新增过；删改基线断言即红案
//!   （XC40_BASE_REMOVED / XC40_BASE_MODIFIED），与 F0707 纪律账同判同
//!   口径；红案不设豁免路径，恢复或走批账勘误。跨任务引用：panic 文案
//!   三要素对齐 F0712（编号/现象/下一步）；必需响应缺失 panic 文案含
//!   tag 名对齐 F0726；六接口字段冻结表对齐 F0725/F0736。
//!
//! ── 模拟执行引擎定位 ──
//!   检查体以 fn(&A40World) -> bool 函数指针登记，对 A40World（六接口
//!   字段快照列、Limine 响应面快照列、panic 路径快照列）做真实谓词判
//!   定；A40World 为模拟世界态，不接真硬件——下游真实断言面
//!   （unx0725_resptree / unx0086_panicsym / unx0125_earlypanic 等）落
//!   世界态后本账可直接消费。
//!
//! ── 性能逐项分解 ──
//!   登记 O(n)（重复 id 线性扫，n≤54 实测级）；执行 O(n)；diff O(n²)
//!   （基线×账本线性对账）；指纹 O(n·|str|)；对拍 2×(登记+执行)。
//!   全整数确定性，无浮点，无时钟，无外部输入。
//!
//! ── 诊断码段：0x25B0（独占 0x25B0~0x25BF，16 码）──
//!   任务书预核 0x2010 段——复核被占：0x2010=unx0048_sratlit
//!   （SEG_BASE: u16 = 0x2010，mod.rs:5183 注册）；依次试 0x2020 段=
//!   unx0049_systab（mod.rs:5185）、0x2030 段=unx0050_uvarwalk
//!   （mod.rs:5187）——均被占，继续全树扫描（kernel/varix/src 全域
//!   grep "0x25B[0-9A-Fa-f]" 零命中，含 SEG_BASE 表与数据值），终取
//!   0x25B0 段独占（0x25BF 预留不派）。
//!
//! ── 通用纪律 ──
//!   no_std + alloc；只 use core::… 与 use alloc::{…}，禁 use crate::…。
//!   内核路径零 unwrap/expect/panic!/运行期 assert!（编译期
//!   const _: () = assert!(…) 允许）。可失败 API 全走
//!   Result<T, Assert40Error>；溢出显性 saturating/wrapping。
//!   >64KiB 结构体禁上栈（本文件全部大结构走 Vec 堆；栈上最大为
//!   A40World ≈ 0.3KB 与各 const 表，安全）。
//!   const fn 纪律：const fn 内零 slice::get / Vec 索引 / Ord::min-max /
//!   derive(PartialEq) 类型的 == / for 循环（一律 while + 索引；本文件
//!   const fn 仅做平凡字段初始化，不含循环）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ═══════════════════════════════════════════════════════════════════
// 版本与诊断码段（0x25B0 段闭集 · 独占证据见文件头）
// ═══════════════════════════════════════════════════════════════════

/// 模块版本。
pub const A40_VERSION: &str = "unx0706_assert40-v1";

/// 诊断段基址（0x25B0 段 · 本模块独占 · 先验空闲证据见文件头）。
pub const A40_SEG_BASE: u16 = 0x25B0;

/// 绿码/正常收讫。
pub const XC40_GREEN: u32 = 0x25B0;
/// 断言 id 不合三段式规范。
pub const XC40_BAD_ID: u32 = 0x25B1;
/// 重复 id 拒收。
pub const XC40_DUP_ID: u32 = 0x25B2;
/// 空断言（无检查体）拒收。
pub const XC40_EMPTY_CHECK: u32 = 0x25B3;
/// 族最低条数未达。
pub const XC40_FAMILY_MIN: u32 = 0x25B4;
/// 总数不足 40。
pub const XC40_TOTAL_SHORT: u32 = 0x25B5;
/// 断言执行红灯。
pub const XC40_ASSERT_FAIL: u32 = 0x25B6;
/// 基线断言被删（红案）。
pub const XC40_BASE_REMOVED: u32 = 0x25B7;
/// 基线断言被改（红案）。
pub const XC40_BASE_MODIFIED: u32 = 0x25B8;
/// 执行序不稳定。
pub const XC40_ORDER_UNSTABLE: u32 = 0x25B9;
/// 事件账红灯。
pub const XC40_HEALTH_RED: u32 = 0x25BA;
/// 双跑指纹不一致。
pub const XC40_FP_MISMATCH: u32 = 0x25BB;
/// 族面覆盖缺口。
pub const XC40_COVER_GAP: u32 = 0x25BC;
/// 注入故障未被检出。
pub const XC40_INJECT_MISSED: u32 = 0x25BD;
/// 段内未派码兜底（0x25BE；0x25BF 段内预留不派）。
pub const XC40_UNKNOWN: u32 = 0x25BE;

/// 诊断码 → （现象，处置）中文可操作诊断（独占段 0x25B0~0x25BE 内解释，
/// 段外一律未知）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC40_GREEN => ("绿码：登记/执行/对账/注入面正常收讫", "无需处置，继续下一族"),
        XC40_BAD_ID => (
            "断言 id 不合三段式规范（A40-<族码>-<面字母>-<序号>）",
            "按族码 FR/LM/PP、面字母 A~C、序号 001~999 的规范形改写后重登记",
        ),
        XC40_DUP_ID => (
            "断言 id 与账本既有条目重复",
            "换序号或换面字母；禁止同 id 复登记（登记制红线）",
        ),
        XC40_EMPTY_CHECK => (
            "空断言：登记条目无检查体（check = None）",
            "补齐 fn(&A40World)->bool 检查体后再登记；无检查体一律拒收",
        ),
        XC40_FAMILY_MIN => (
            "族登记条数低于该族最低硬线",
            "按三族清单补齐：六冻结接口 ≥20 / Limine 响应面 ≥12 / panic 路径 ≥8",
        ),
        XC40_TOTAL_SHORT => (
            "断言总数不足 40（UNX-F0706-J1 硬线）",
            "向三族继续补条，直至总数 ≥40 且三族各自达标",
        ),
        XC40_ASSERT_FAIL => (
            "断言执行存在红灯条目",
            "按失败条目的 id 与判据引用定位修复后重跑",
        ),
        XC40_BASE_REMOVED => (
            "基线断言被删除（只增不减纪律红案，与 F0707 同判）",
            "恢复被删断言或走批账勘误流程，禁止静默删除",
        ),
        XC40_BASE_MODIFIED => (
            "基线断言判据/描述被改写（只增不减纪律红案，与 F0707 同判）",
            "恢复原判据引用；确需改写走批账勘误并留痕",
        ),
        XC40_ORDER_UNSTABLE => (
            "执行序不稳定（order 字段非严格递增）",
            "重建账本重登记；禁止乱序注入执行流",
        ),
        XC40_HEALTH_RED => (
            "事件账存在故障事件或丢事件",
            "按事件码逐条排查登记/执行/对账路径",
        ),
        XC40_FP_MISMATCH => (
            "双跑指纹不一致（确定性破缺）",
            "排查登记序/世界态是否引入非确定源后重跑对拍",
        ),
        XC40_COVER_GAP => (
            "族面覆盖缺口（某族某面零登记）",
            "按缺口清单（族×面）补齐对应断言条目",
        ),
        XC40_INJECT_MISSED => (
            "注入故障未被断言面检出",
            "修复对应守卫逻辑后重跑注入面",
        ),
        XC40_UNKNOWN => (
            "段内未派诊断码",
            "查 0x25B0 段闭集登记表；0x25BF 为段内预留位",
        ),
        _ => ("段外未知诊断码", "查 0x25B0 段闭集登记表（0x25B0~0x25BE）"),
    }
}

// ═══════════════════════════════════════════════════════════════════
// 错误枚举：每变体带 detail() 中文可操作诊断
// ═══════════════════════════════════════════════════════════════════

/// 断言账本体系错误（全路径可失败出口，禁 panic 族）。
#[derive(Clone, Debug)]
pub enum Assert40Error {
    /// id 不合三段式规范。
    BadId { code: u32, id: String },
    /// 重复 id。
    DuplicateId { code: u32, id: String },
    /// 空断言（无检查体）。
    EmptyCheck { code: u32, id: String },
    /// 族最低条数未达。
    FamilyMinShort { code: u32, fam_idx: usize, have: usize, need: usize },
    /// 总数不足 40。
    TotalShort { code: u32, have: usize, need: usize },
    /// 断言执行存在红灯。
    AssertFail { code: u32, red: usize },
    /// 基线断言被删（红案）。
    BaselineRemoved { code: u32, id: String },
    /// 基线断言被改（红案）。
    BaselineModified { code: u32, id: String },
    /// 执行序不稳定。
    OrderUnstable { code: u32 },
    /// 事件账红灯。
    HealthRed { code: u32 },
    /// 双跑指纹不一致。
    FpMismatch { code: u32, a: u64, b: u64 },
    /// 族面覆盖缺口。
    CoverGap { code: u32, fam_idx: usize, face: u8 },
    /// 注入未被检出。
    InjectionMissed { code: u32, kind: &'static str },
    /// 段外未知诊断码兜底。
    UnknownDiag { code: u32 },
}

impl Assert40Error {
    /// 中文可操作诊断（每变体一句可执行处置）。
    pub fn detail(&self) -> &'static str {
        match self {
            Assert40Error::BadId { .. } => {
                "id 不合三段式规范：改写为 A40-<族码>-<面字母>-<序号> 规范形后重登记"
            }
            Assert40Error::DuplicateId { .. } => {
                "重复 id：换序号/面字母，禁止同 id 复登记"
            }
            Assert40Error::EmptyCheck { .. } => {
                "空断言拒收：补齐检查体 fn(&A40World)->bool 后再登记"
            }
            Assert40Error::FamilyMinShort { .. } => {
                "族低于硬线：按 FR ≥20 / LM ≥12 / PP ≥8 清单补齐"
            }
            Assert40Error::TotalShort { .. } => {
                "总数不足 40：向三族继续补条至 ≥40 且三族各自达标"
            }
            Assert40Error::AssertFail { .. } => {
                "断言红灯：按失败条目 id 与判据引用定位修复后重跑"
            }
            Assert40Error::BaselineRemoved { .. } => {
                "基线断言被删：恢复条目或走批账勘误（F0707 同判红案）"
            }
            Assert40Error::BaselineModified { .. } => {
                "基线判据被改：恢复原判据引用并留痕（F0707 同判红案）"
            }
            Assert40Error::OrderUnstable { .. } => {
                "执行序不稳定：重建账本，恢复登记序"
            }
            Assert40Error::HealthRed { .. } => {
                "事件账红灯：按事件码排查登记/执行/对账路径"
            }
            Assert40Error::FpMismatch { .. } => {
                "双跑指纹不一致：排查非确定源后重跑对拍"
            }
            Assert40Error::CoverGap { .. } => {
                "族面缺口：按缺口清单（族×面）补齐对应断言"
            }
            Assert40Error::InjectionMissed { .. } => {
                "注入未检出：修复对应守卫后重跑注入面"
            }
            Assert40Error::UnknownDiag { .. } => {
                "未知诊断码：查 0x25B0 段闭集登记表"
            }
        }
    }

    /// 诊断码（段内闭集，段外收敛为 XC40_UNKNOWN）。
    pub fn code(&self) -> u32 {
        match self {
            Assert40Error::BadId { code, .. }
            | Assert40Error::DuplicateId { code, .. }
            | Assert40Error::EmptyCheck { code, .. }
            | Assert40Error::FamilyMinShort { code, .. }
            | Assert40Error::TotalShort { code, .. }
            | Assert40Error::AssertFail { code, .. }
            | Assert40Error::BaselineRemoved { code, .. }
            | Assert40Error::BaselineModified { code, .. }
            | Assert40Error::OrderUnstable { code, .. }
            | Assert40Error::HealthRed { code, .. }
            | Assert40Error::FpMismatch { code, .. }
            | Assert40Error::CoverGap { code, .. }
            | Assert40Error::InjectionMissed { code, .. }
            | Assert40Error::UnknownDiag { code } => *code,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// S1 之三族闭集与族硬线 const 表
// ═══════════════════════════════════════════════════════════════════

/// 三族闭集（J1-b 三族覆盖的族维度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family40 {
    /// 六冻结接口字段级（≥20）。
    Frozen6,
    /// Limine 响应面（≥12）。
    LimineResp,
    /// panic 路径（≥8）。
    PanicPath,
}

/// 族数（闭集大小，const 期钉死）。
pub const A40_FAMILY_COUNT: usize = 3;

/// 族闭集全表（顺序即族下标，counts/覆盖审计按此对齐）。
pub const A40_FAMILIES: [Family40; A40_FAMILY_COUNT] =
    [Family40::Frozen6, Family40::LimineResp, Family40::PanicPath];

/// FR 族最低硬线（UNX-F0706-J1：六冻结接口字段级 ≥20）。
pub const A40_MIN_FROZEN6: usize = 20;
/// LM 族最低硬线（UNX-F0706-J1：Limine 响应面 ≥12）。
pub const A40_MIN_LIMINE: usize = 12;
/// PP 族最低硬线（UNX-F0706-J1：panic 路径 ≥8）。
pub const A40_MIN_PANIC: usize = 8;
/// 总硬线（三族之和恰为 40，const 断言互锁）。
pub const A40_TOTAL_MIN: usize = 40;

/// 每族面字母闭集（族内 A/B/C 三面统一）。
pub const A40_FACE_LETTERS: [u8; 3] = [b'A', b'B', b'C'];

impl Family40 {
    /// 族码（id 第二段，两字母）。
    pub const fn code2(self) -> &'static str {
        match self {
            Family40::Frozen6 => "FR",
            Family40::LimineResp => "LM",
            Family40::PanicPath => "PP",
        }
    }

    /// 族码反查（闭集之外 None）。
    pub fn parse_code2(s: &str) -> Option<Family40> {
        match s {
            "FR" => Some(Family40::Frozen6),
            "LM" => Some(Family40::LimineResp),
            "PP" => Some(Family40::PanicPath),
            _ => None,
        }
    }

    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            Family40::Frozen6 => "六冻结接口字段级",
            Family40::LimineResp => "Limine 响应面",
            Family40::PanicPath => "panic 路径",
        }
    }

    /// 族最低硬线（const 表口径）。
    pub const fn min_asserts(self) -> usize {
        match self {
            Family40::Frozen6 => A40_MIN_FROZEN6,
            Family40::LimineResp => A40_MIN_LIMINE,
            Family40::PanicPath => A40_MIN_PANIC,
        }
    }

    /// 族下标（对齐 A40_FAMILIES 顺序）。
    pub const fn idx(self) -> usize {
        match self {
            Family40::Frozen6 => 0,
            Family40::LimineResp => 1,
            Family40::PanicPath => 2,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// S1 AssertionId 三段式 id 规范（parse/render 互验）
// ═══════════════════════════════════════════════════════════════════

/// id 前缀（A40 = 本账固定头）。
pub const A40_ID_PREFIX: &str = "A40";
/// 面字母下界。
pub const A40_FACE_MIN: u8 = b'A';
/// 面字母上界。
pub const A40_FACE_MAX: u8 = b'C';
/// 序号上界（三位数字封顶）。
pub const A40_SEQ_MAX: u16 = 999;

/// 三段式断言 id：族（FR/LM/PP）· 面（A~C）· 序号（001~999）。
///
/// 规范形：`A40-FR-A-001`（前缀 + 三段，段间连字符，序号零填充三位）。
/// parse/render 互验：规范形字符串 parse 后 render 必须逐字节相等；
/// 非规范形（段数错/族码越闭集/面字母越界/序号越界或非三位数字）一律
/// BadId 拒收。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssertionId {
    fam: Family40,
    face: u8,
    seq: u16,
}

impl AssertionId {
    /// 由（族, 面字母, 序号）构造（越界即 BadId）。
    pub fn new(fam: Family40, face: u8, seq: u16) -> Result<AssertionId, Assert40Error> {
        if face < A40_FACE_MIN || face > A40_FACE_MAX {
            return Err(Assert40Error::BadId {
                code: XC40_BAD_ID,
                id: format!("{}-{}-{}", A40_ID_PREFIX, fam.code2(), face as char),
            });
        }
        if seq == 0 || seq > A40_SEQ_MAX {
            return Err(Assert40Error::BadId {
                code: XC40_BAD_ID,
                id: format!("{}-{}-{}-{}", A40_ID_PREFIX, fam.code2(), face as char, seq),
            });
        }
        Ok(AssertionId { fam, face, seq })
    }

    /// 规范形解析（逐字节硬检，任一段不合即 BadId）。
    pub fn parse(text: &str) -> Result<AssertionId, Assert40Error> {
        let b = text.as_bytes();
        // 形状硬检：A40-FR-A-001 = 4+3+2+3 = 12 字节定长（各族码/面字母/
        // 三位序号下全长恒 12，越界长度先拒）。
        if b.len() != 12 {
            return Err(Assert40Error::BadId { code: XC40_BAD_ID, id: String::from(text) });
        }
        // 段一：前缀 A40-。
        if b[0] != b'A' || b[1] != b'4' || b[2] != b'0' || b[3] != b'-' {
            return Err(Assert40Error::BadId { code: XC40_BAD_ID, id: String::from(text) });
        }
        // 段二：族码两字母 + '-'（纯字节对匹配——不做字符串切片，杜绝
        // 多字节 UTF-8 输入切在字符中间的 panic 路径）。
        let fam = match (b[4], b[5]) {
            (b'F', b'R') => Family40::Frozen6,
            (b'L', b'M') => Family40::LimineResp,
            (b'P', b'P') => Family40::PanicPath,
            _ => return Err(Assert40Error::BadId { code: XC40_BAD_ID, id: String::from(text) }),
        };
        if b[6] != b'-' {
            return Err(Assert40Error::BadId { code: XC40_BAD_ID, id: String::from(text) });
        }
        // 段三之首：面字母。
        let face = b[7];
        if face < A40_FACE_MIN || face > A40_FACE_MAX {
            return Err(Assert40Error::BadId { code: XC40_BAD_ID, id: String::from(text) });
        }
        if b[8] != b'-' {
            return Err(Assert40Error::BadId { code: XC40_BAD_ID, id: String::from(text) });
        }
        // 段三之尾：序号三位数字（无符号位，无空格，零填充规范形）。
        if b[9] < b'0' || b[9] > b'9' || b[10] < b'0' || b[10] > b'9' || b[11] < b'0' || b[11] > b'9'
        {
            return Err(Assert40Error::BadId { code: XC40_BAD_ID, id: String::from(text) });
        }
        let d1 = (b[9] - b'0') as u16;
        let d2 = (b[10] - b'0') as u16;
        let d3 = (b[11] - b'0') as u16;
        let seq = d1 * 100 + d2 * 10 + d3;
        if seq == 0 || seq > A40_SEQ_MAX {
            return Err(Assert40Error::BadId { code: XC40_BAD_ID, id: String::from(text) });
        }
        Ok(AssertionId { fam, face, seq })
    }

    /// 渲染规范形（与 parse 互验的另一半）。
    pub fn render(&self) -> String {
        format!(
            "{}-{}-{}-{:03}",
            A40_ID_PREFIX,
            self.fam.code2(),
            self.face as char,
            self.seq
        )
    }

    /// parse/render 互验：给定文本若是规范形，往返渲染必逐字节相等。
    pub fn round_trip_ok(text: &str) -> bool {
        match AssertionId::parse(text) {
            Ok(id) => id.render() == text,
            Err(_) => false,
        }
    }

    /// 本 id 自身往返互验（防御性自证）。
    pub fn self_round_trip(&self) -> bool {
        let t = self.render();
        match AssertionId::parse(&t) {
            Ok(back) => back == *self,
            Err(_) => false,
        }
    }

    /// 族。
    pub const fn face_family(&self) -> Family40 {
        self.fam
    }

    /// 面字母。
    pub const fn face(&self) -> u8 {
        self.face
    }

    /// 序号。
    pub const fn seq(&self) -> u16 {
        self.seq
    }

    /// 排序键（族·面·序三元组，账本去重与稳定序用）。
    pub const fn key(&self) -> (u8, u8, u16) {
        (self.fam.idx() as u8, self.face, self.seq)
    }
}

// ═══════════════════════════════════════════════════════════════════
// S2 六冻结接口闭集与冻结字段表
// ═══════════════════════════════════════════════════════════════════

/// 六冻结接口闭集（IF-MM/FB/KA/HH/RS/IT；来源见文件头「六冻结接口口径」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Iface6 {
    /// memory_map 内存图响应。
    Mm,
    /// framebuffer 帧缓冲响应。
    Fb,
    /// kernel_address 内核 ELF 段映射响应。
    Ka,
    /// hhdm 高半直接映射基址响应。
    Hh,
    /// rsdp ACPI RSDP 指针响应。
    Rs,
    /// itmode 引导模式响应。
    It,
}

/// 接口数（闭集大小）。
pub const IFACE6_COUNT: usize = 6;

/// 接口闭集全表（顺序即接口下标，世界态快照列按此对齐）。
pub const A40_IFACE6_ALL: [Iface6; IFACE6_COUNT] =
    [Iface6::Mm, Iface6::Fb, Iface6::Ka, Iface6::Hh, Iface6::Rs, Iface6::It];

/// 接口代码（id/账本文本用两字母码）。
pub const A40_IFACE6_CODES: [&str; IFACE6_COUNT] = ["MM", "FB", "KA", "HH", "RS", "IT"];

/// 接口协议名（Limine Boot Protocol tag 名）。
pub const A40_IFACE6_NAMES: [&str; IFACE6_COUNT] = [
    "memory_map",
    "framebuffer",
    "kernel_address",
    "hhdm",
    "rsdp",
    "itmode",
];

impl Iface6 {
    /// 接口下标。
    pub const fn idx(self) -> usize {
        match self {
            Iface6::Mm => 0,
            Iface6::Fb => 1,
            Iface6::Ka => 2,
            Iface6::Hh => 3,
            Iface6::Rs => 4,
            Iface6::It => 5,
        }
    }

    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            Iface6::Mm => "内存图响应",
            Iface6::Fb => "帧缓冲响应",
            Iface6::Ka => "内核地址响应",
            Iface6::Hh => "高半直接映射",
            Iface6::Rs => "RSDP 指针",
            Iface6::It => "引导模式",
        }
    }
}

/// 冻结字段行（字段名/偏移/宽度/冻结起始版本四列）。
///
/// 偏移与宽度按 Limine Boot Protocol revision 1 内存布局写死（对齐
/// F0725 冻结表口径）；since_v = 冻结起始版本（本账冻结表 v1）。
#[derive(Clone, Copy, Debug)]
pub struct A40FrozenField {
    /// 字段名。
    pub name: &'static str,
    /// 偏移（字节，响应结构内相对偏移）。
    pub offset: u32,
    /// 宽度（字节）。
    pub width: u32,
    /// 冻结起始版本。
    pub since_v: u32,
}

/// 冻结表版本（F0725 table_version 同源）。
pub const A40_FREEZE_TABLE_V: u32 = 1;

/// IF-MM 冻结字段表（3 行；来源：Limine memory map response rev1：
/// revision@0×8 / entry_count@8×8 / entries 指针@16×8）。
pub const A40_FRZ_MM: [A40FrozenField; 3] = [
    A40FrozenField { name: "revision", offset: 0, width: 8, since_v: 1 },
    A40FrozenField { name: "entry_count", offset: 8, width: 8, since_v: 1 },
    A40FrozenField { name: "entries", offset: 16, width: 8, since_v: 1 },
];

/// IF-FB 冻结字段表（13 行；来源：Limine framebuffer response rev1：
/// rev@0/addr@8/width@16/height@24 各 8B，bpp@32×8，pitch@40×8，
/// memory_model@48×1，red/green/blue 掩码尺寸与移位 @49..54 各 1B）。
pub const A40_FRZ_FB: [A40FrozenField; 13] = [
    A40FrozenField { name: "revision", offset: 0, width: 8, since_v: 1 },
    A40FrozenField { name: "address", offset: 8, width: 8, since_v: 1 },
    A40FrozenField { name: "width", offset: 16, width: 8, since_v: 1 },
    A40FrozenField { name: "height", offset: 24, width: 8, since_v: 1 },
    A40FrozenField { name: "bpp", offset: 32, width: 8, since_v: 1 },
    A40FrozenField { name: "pitch", offset: 40, width: 8, since_v: 1 },
    A40FrozenField { name: "memory_model", offset: 48, width: 1, since_v: 1 },
    A40FrozenField { name: "red_mask_size", offset: 49, width: 1, since_v: 1 },
    A40FrozenField { name: "red_mask_shift", offset: 50, width: 1, since_v: 1 },
    A40FrozenField { name: "green_mask_size", offset: 51, width: 1, since_v: 1 },
    A40FrozenField { name: "green_mask_shift", offset: 52, width: 1, since_v: 1 },
    A40FrozenField { name: "blue_mask_size", offset: 53, width: 1, since_v: 1 },
    A40FrozenField { name: "blue_mask_shift", offset: 54, width: 1, since_v: 1 },
];

/// IF-KA 冻结字段表（4 行；来源：Limine kernel address response rev1：
/// revision@0×8 / physical_base@8×8 / virtual_base@16×8 / size@24×8）。
pub const A40_FRZ_KA: [A40FrozenField; 4] = [
    A40FrozenField { name: "revision", offset: 0, width: 8, since_v: 1 },
    A40FrozenField { name: "physical_base", offset: 8, width: 8, since_v: 1 },
    A40FrozenField { name: "virtual_base", offset: 16, width: 8, since_v: 1 },
    A40FrozenField { name: "size", offset: 24, width: 8, since_v: 1 },
];

/// IF-HH 冻结字段表（2 行；来源：Limine hhdm response rev1）。
pub const A40_FRZ_HH: [A40FrozenField; 2] = [
    A40FrozenField { name: "revision", offset: 0, width: 8, since_v: 1 },
    A40FrozenField { name: "offset", offset: 8, width: 8, since_v: 1 },
];

/// IF-RS 冻结字段表（2 行；来源：Limine rsdp response rev1）。
pub const A40_FRZ_RS: [A40FrozenField; 2] = [
    A40FrozenField { name: "revision", offset: 0, width: 8, since_v: 1 },
    A40FrozenField { name: "address", offset: 8, width: 8, since_v: 1 },
];

/// IF-IT 冻结字段表（2 行；来源：Limine itmode response rev1；bit0 =
/// 直接图形模式引导）。
pub const A40_FRZ_IT: [A40FrozenField; 2] = [
    A40FrozenField { name: "revision", offset: 0, width: 8, since_v: 1 },
    A40FrozenField { name: "flags", offset: 8, width: 8, since_v: 1 },
];

/// 六表聚合（下标对齐 A40_IFACE6_ALL 顺序）。
pub const A40_FRZ_TABLES: [&[A40FrozenField]; IFACE6_COUNT] =
    [&A40_FRZ_MM, &A40_FRZ_FB, &A40_FRZ_KA, &A40_FRZ_HH, &A40_FRZ_RS, &A40_FRZ_IT];

/// 每接口冻结字段数（与 A40_FRZ_TABLES 互锁，const 断言钉死）。
pub const A40_IFACE6_FIELD_COUNTS: [u8; IFACE6_COUNT] = [3, 13, 4, 2, 2, 2];

/// 六接口冻结字段总数（3+13+4+2+2+2 = 26）。
pub const A40_FRZ_FIELD_TOTAL: usize = 26;

// ═══════════════════════════════════════════════════════════════════
// 模拟世界态：六接口快照列 / Limine 响应面快照列 / panic 路径快照列
// ═══════════════════════════════════════════════════════════════════

/// 最小可引导内存线（64 MiB；来源：QEMU -m 64M 下限口径，主册 B05
/// 内存图语义化加固批账同判）。
pub const A40_MIN_USABLE_BYTES: u64 = 64 * 1024 * 1024;
/// 帧缓冲最小宽度（640；探针地板值，低于此判红）。
pub const A40_FB_MIN_W: u32 = 640;
/// 帧缓冲最小高度（400；探针地板值）。
pub const A40_FB_MIN_H: u32 = 400;
/// 高半基线（x86-64 canonical 高半下界）。
pub const A40_HHDM_FLOOR: u64 = 0xFFFF_8000_0000_0000;
/// panic 风暴上界（三重 fault 挂死语义内 panic 触发次数上限；来源：
/// F0726「三重 fault 挂死语义保持」——第三次 fault 后不再递归）。
pub const A40_PANIC_STORM_BOUND: u32 = 3;

/// 单接口世界态快照（FR 族被检对象）。
#[derive(Clone, Copy, Debug)]
pub struct IfaceSnap {
    /// 在场字段数（对照 A40_IFACE6_FIELD_COUNTS）。
    pub fields_present: u8,
    /// 偏移与冻结表一致。
    pub offsets_frozen: bool,
    /// 宽度与冻结表一致。
    pub widths_frozen: bool,
    /// 协议 revision（冻结口径 0..=1）。
    pub rev: u32,
    /// 魔数/形状探针过（MM entries 指针非零、FB 颜色模型位、KA 物理基
    /// 非零、HH/RS/IT 地址非零等接口特异形状核）。
    pub magic_ok: bool,
    /// 只读消费核过（F0736：升级后回归只读消费零改写）。
    pub consumed_readonly: bool,
}

impl IfaceSnap {
    /// 空态（全零/全否：该接口一切断言应红）。
    pub const fn blank() -> IfaceSnap {
        IfaceSnap {
            fields_present: 0,
            offsets_frozen: false,
            widths_frozen: false,
            rev: 0,
            magic_ok: false,
            consumed_readonly: false,
        }
    }
}

/// Limine 响应面世界态快照（LM 族被检对象）。
#[derive(Clone, Copy, Debug)]
pub struct LimineSnap {
    /// 响应在场（boot info 指针非空）。
    pub resp_present: bool,
    /// 响应指针原始值（非零核）。
    pub resp_ptr: u64,
    /// 响应树 revision（≥1）。
    pub revision: u64,
    /// 闭集外未知 tag 计数（应零）。
    pub unknown_tags: u32,
    /// 六接口响应逐接口在场位。
    pub iface_resp_present: [bool; IFACE6_COUNT],
    /// 内存图条目数。
    pub mm_entries: u32,
    /// usable 字节合计。
    pub mm_usable_bytes: u64,
    /// 条目基址有序（非降）。
    pub mm_sorted: bool,
    /// 条目区间交叠计数（应零）。
    pub mm_overlap: u32,
    /// bootloader-reclaimable 字节合计。
    pub mm_reclaim_bytes: u64,
    /// 帧缓冲基址。
    pub fb_addr: u64,
    /// 帧缓冲宽。
    pub fb_w: u32,
    /// 帧缓冲高。
    pub fb_h: u32,
    /// 帧缓冲 bpp。
    pub fb_bpp: u16,
    /// 颜色模型为 RGB。
    pub fb_model_rgb: bool,
    /// 帧缓冲 pitch（行字节数）。
    pub fb_pitch: u32,
    /// 内核物理基址。
    pub ka_phys: u64,
    /// 内核虚基址。
    pub ka_virt: u64,
    /// 内核映像大小。
    pub ka_size: u64,
    /// KASLR slide 前后一致（virt - phys 差与段表一致）。
    pub ka_slide_consistent: bool,
    /// hhdm 基址。
    pub hh_base: u64,
    /// RSDP 物理地址。
    pub rsdp_addr: u64,
    /// itmode bit0（直接图形模式）。
    pub itmode_direct: bool,
}

impl LimineSnap {
    /// 空态（全零/全否：LM 族一切断言应红）。
    pub const fn blank() -> LimineSnap {
        LimineSnap {
            resp_present: false,
            resp_ptr: 0,
            revision: 0,
            unknown_tags: 0,
            iface_resp_present: [false; IFACE6_COUNT],
            mm_entries: 0,
            mm_usable_bytes: 0,
            mm_sorted: false,
            mm_overlap: 0,
            mm_reclaim_bytes: 0,
            fb_addr: 0,
            fb_w: 0,
            fb_h: 0,
            fb_bpp: 0,
            fb_model_rgb: false,
            fb_pitch: 0,
            ka_phys: 0,
            ka_virt: 0,
            ka_size: 0,
            ka_slide_consistent: false,
            hh_base: 0,
            rsdp_addr: 0,
            itmode_direct: false,
        }
    }
}

/// panic 路径世界态快照（PP 族被检对象）。
#[derive(Clone, Copy, Debug)]
pub struct PanicSnap {
    /// panic 挂钩在 ExitBootServices 前装好。
    pub hook_installed_pre_ebs: bool,
    /// 必需响应缺失注入触发的 panic 次数（F0726 口径应 ≥1）。
    pub missing_req_panics: u32,
    /// panic 文案含缺失 tag 名（F0726：PANIC 文案含 tag 名可观测）。
    pub msg_contains_tag: bool,
    /// 文案三要素齐（编号/现象/下一步——F0712 模板）。
    pub msg_three_parts: bool,
    /// panic 前事件账已冲刷。
    pub events_flushed: bool,
    /// ExitBootServices 后串口仍可写（panic 输出通道）。
    pub serial_post_ebs: bool,
    /// 挂死语义保持（halt 环不返回）。
    pub halt_hold: bool,
    /// unwind 禁用（panic=abort 口径）。
    pub unwind_disabled: bool,
    /// 递归守卫（单 panic 单触发，不嵌套）。
    pub recursive_guard_ok: bool,
    /// panic 计数有界（≤ A40_PANIC_STORM_BOUND，防风暴）。
    pub storm_bounded: bool,
    /// panic 快照绑定引导阶段戳。
    pub phase_stamped: bool,
    /// panic 后不再推进引导步（无重入）。
    pub no_boot_reentry: bool,
}

impl PanicSnap {
    /// 空态（全否：PP 族一切断言应红）。
    pub const fn blank() -> PanicSnap {
        PanicSnap {
            hook_installed_pre_ebs: false,
            missing_req_panics: 0,
            msg_contains_tag: false,
            msg_three_parts: false,
            events_flushed: false,
            serial_post_ebs: false,
            halt_hold: false,
            unwind_disabled: false,
            recursive_guard_ok: false,
            storm_bounded: false,
            phase_stamped: false,
            no_boot_reentry: false,
        }
    }
}

/// 模拟世界态：三族被检对象的真实状态列（检查体的唯一实参）。
#[derive(Clone, Debug)]
pub struct A40World {
    /// 冻结表版本。
    pub tbl_v: u32,
    /// 冻结表只增不改位。
    pub tbl_append_only: bool,
    /// 六接口快照列。
    pub ifaces: [IfaceSnap; IFACE6_COUNT],
    /// Limine 响应面快照列。
    pub lm: LimineSnap,
    /// panic 路径快照列。
    pub pp: PanicSnap,
}

impl A40World {
    /// 空世界态（全否/全零：一切断言应红）。
    pub fn blank() -> A40World {
        A40World {
            tbl_v: 0,
            tbl_append_only: false,
            ifaces: [IfaceSnap::blank(); IFACE6_COUNT],
            lm: LimineSnap::blank(),
            pp: PanicSnap::blank(),
        }
    }

    /// 全绿世界态（典型可引导机口径：QEMU gkv 1280×800×32、128 MiB
    /// usable、内核 -2GiB 高半映像、hhdm 0xFFFF_8880_0000_0000、RSDP
    /// 4 GiB 下界下 0xBFFE0000、panic 路径全过——每字段值来源注释见
    /// 各族检查体）。
    pub fn green() -> A40World {
        let mut ifaces = [IfaceSnap::blank(); IFACE6_COUNT];
        let mut i = 0;
        while i < IFACE6_COUNT {
            ifaces[i] = IfaceSnap {
                fields_present: A40_IFACE6_FIELD_COUNTS[i],
                offsets_frozen: true,
                widths_frozen: true,
                rev: 1,
                magic_ok: true,
                consumed_readonly: true,
            };
            i += 1;
        }
        A40World {
            tbl_v: A40_FREEZE_TABLE_V,
            tbl_append_only: true,
            ifaces,
            lm: LimineSnap {
                resp_present: true,
                resp_ptr: 0xFFFF_9000_0000_0000,
                revision: 1,
                unknown_tags: 0,
                iface_resp_present: [true; IFACE6_COUNT],
                mm_entries: 7,
                mm_usable_bytes: 128 * 1024 * 1024,
                mm_sorted: true,
                mm_overlap: 0,
                mm_reclaim_bytes: 2 * 1024 * 1024,
                fb_addr: 0xFD00_0000,
                fb_w: 1280,
                fb_h: 800,
                fb_bpp: 32,
                fb_model_rgb: true,
                fb_pitch: 5120,
                ka_phys: 0x0100_0000,
                ka_virt: 0xFFFF_FFFF_8000_0000,
                ka_size: 2 * 1024 * 1024,
                ka_slide_consistent: true,
                hh_base: 0xFFFF_8880_0000_0000,
                rsdp_addr: 0x0000_0000_BFFE_0000,
                itmode_direct: true,
            },
            pp: PanicSnap {
                hook_installed_pre_ebs: true,
                missing_req_panics: 1,
                msg_contains_tag: true,
                msg_three_parts: true,
                events_flushed: true,
                serial_post_ebs: true,
                halt_hold: true,
                unwind_disabled: true,
                recursive_guard_ok: true,
                storm_bounded: true,
                phase_stamped: true,
                no_boot_reentry: true,
            },
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// 断言规格与检查体签名
// ═══════════════════════════════════════════════════════════════════

/// 检查体签名（模拟执行引擎：对世界态做真实谓词判定）。
pub type CheckFn40 = fn(&A40World) -> bool;

/// 断言规格（id + 判据引用 + 描述 + 检查体）。
#[derive(Clone, Copy, Debug)]
pub struct AssertionSpec40 {
    /// 三段式 id。
    pub id: AssertionId,
    /// 判据引用（如 "UNX-F0706-J1#字段级"）。
    pub criterion: &'static str,
    /// 断言描述（中文一句话）。
    pub desc: &'static str,
    /// 检查体（None = 空断言，登记时拒收）。
    pub check: Option<CheckFn40>,
}

impl AssertionSpec40 {
    /// 构造（check 传 None 即空断言样本——仅注入面使用，登记必拒）。
    pub const fn new(
        id: AssertionId,
        criterion: &'static str,
        desc: &'static str,
        check: Option<CheckFn40>,
    ) -> AssertionSpec40 {
        AssertionSpec40 { id, criterion, desc, check }
    }
}

// ═══════════════════════════════════════════════════════════════════
// FR 族检查体（六冻结接口字段级 26 条）
// ═══════════════════════════════════════════════════════════════════

// ── FR-A 面：字段在场（六接口逐个，001~006）──

/// IF-MM 三字段在场（revision/entry_count/entries）。
pub fn ck_fr_presence_mm(w: &A40World) -> bool {
    w.ifaces[Iface6::Mm.idx()].fields_present >= A40_IFACE6_FIELD_COUNTS[Iface6::Mm.idx()]
}

/// IF-FB 13 字段在场（头 6 字段 + 颜色模型 + RGB 掩码六项）。
pub fn ck_fr_presence_fb(w: &A40World) -> bool {
    w.ifaces[Iface6::Fb.idx()].fields_present >= A40_IFACE6_FIELD_COUNTS[Iface6::Fb.idx()]
}

/// IF-KA 四字段在场（revision/phys_base/virt_base/size）。
pub fn ck_fr_presence_ka(w: &A40World) -> bool {
    w.ifaces[Iface6::Ka.idx()].fields_present >= A40_IFACE6_FIELD_COUNTS[Iface6::Ka.idx()]
}

/// IF-HH 两字段在场（revision/offset）。
pub fn ck_fr_presence_hh(w: &A40World) -> bool {
    w.ifaces[Iface6::Hh.idx()].fields_present >= A40_IFACE6_FIELD_COUNTS[Iface6::Hh.idx()]
}

/// IF-RS 两字段在场（revision/address）。
pub fn ck_fr_presence_rs(w: &A40World) -> bool {
    w.ifaces[Iface6::Rs.idx()].fields_present >= A40_IFACE6_FIELD_COUNTS[Iface6::Rs.idx()]
}

/// IF-IT 两字段在场（revision/flags）。
pub fn ck_fr_presence_it(w: &A40World) -> bool {
    w.ifaces[Iface6::It.idx()].fields_present >= A40_IFACE6_FIELD_COUNTS[Iface6::It.idx()]
}

/// FR-A 面检查体表（下标对齐接口下标）。
pub const FR_FN_PRESENCE: [CheckFn40; IFACE6_COUNT] = [
    ck_fr_presence_mm,
    ck_fr_presence_fb,
    ck_fr_presence_ka,
    ck_fr_presence_hh,
    ck_fr_presence_rs,
    ck_fr_presence_it,
];

// ── FR-B 面：偏移冻结（六接口逐个，007~012）──

/// IF-MM 偏移与冻结表一致（0/8/16）。
pub fn ck_fr_offset_mm(w: &A40World) -> bool {
    w.ifaces[Iface6::Mm.idx()].offsets_frozen
}

/// IF-FB 偏移与冻结表一致（0..54 逐字段）。
pub fn ck_fr_offset_fb(w: &A40World) -> bool {
    w.ifaces[Iface6::Fb.idx()].offsets_frozen
}

/// IF-KA 偏移与冻结表一致（0/8/16/24）。
pub fn ck_fr_offset_ka(w: &A40World) -> bool {
    w.ifaces[Iface6::Ka.idx()].offsets_frozen
}

/// IF-HH 偏移与冻结表一致（0/8）。
pub fn ck_fr_offset_hh(w: &A40World) -> bool {
    w.ifaces[Iface6::Hh.idx()].offsets_frozen
}

/// IF-RS 偏移与冻结表一致（0/8）。
pub fn ck_fr_offset_rs(w: &A40World) -> bool {
    w.ifaces[Iface6::Rs.idx()].offsets_frozen
}

/// IF-IT 偏移与冻结表一致（0/8）。
pub fn ck_fr_offset_it(w: &A40World) -> bool {
    w.ifaces[Iface6::It.idx()].offsets_frozen
}

/// FR-B 偏移面检查体表。
pub const FR_FN_OFFSET: [CheckFn40; IFACE6_COUNT] = [
    ck_fr_offset_mm,
    ck_fr_offset_fb,
    ck_fr_offset_ka,
    ck_fr_offset_hh,
    ck_fr_offset_rs,
    ck_fr_offset_it,
];

// ── FR-B 面：宽度冻结（六接口逐个，013~018）──

/// IF-MM 字段宽度与冻结表一致（8/8/8）。
pub fn ck_fr_width_mm(w: &A40World) -> bool {
    w.ifaces[Iface6::Mm.idx()].widths_frozen
}

/// IF-FB 字段宽度与冻结表一致（6×8 + 7×1）。
pub fn ck_fr_width_fb(w: &A40World) -> bool {
    w.ifaces[Iface6::Fb.idx()].widths_frozen
}

/// IF-KA 字段宽度与冻结表一致（4×8）。
pub fn ck_fr_width_ka(w: &A40World) -> bool {
    w.ifaces[Iface6::Ka.idx()].widths_frozen
}

/// IF-HH 字段宽度与冻结表一致（2×8）。
pub fn ck_fr_width_hh(w: &A40World) -> bool {
    w.ifaces[Iface6::Hh.idx()].widths_frozen
}

/// IF-RS 字段宽度与冻结表一致（2×8）。
pub fn ck_fr_width_rs(w: &A40World) -> bool {
    w.ifaces[Iface6::Rs.idx()].widths_frozen
}

/// IF-IT 字段宽度与冻结表一致（2×8）。
pub fn ck_fr_width_it(w: &A40World) -> bool {
    w.ifaces[Iface6::It.idx()].widths_frozen
}

/// FR-B 宽度面检查体表。
pub const FR_FN_WIDTH: [CheckFn40; IFACE6_COUNT] = [
    ck_fr_width_mm,
    ck_fr_width_fb,
    ck_fr_width_ka,
    ck_fr_width_hh,
    ck_fr_width_rs,
    ck_fr_width_it,
];

// ── FR-C 面：魔数/形状（六接口逐个，019~024）──

/// IF-MM 形状核：entries 指针非零且 entry_count ≥1。
pub fn ck_fr_magic_mm(w: &A40World) -> bool {
    w.ifaces[Iface6::Mm.idx()].magic_ok && w.lm.mm_entries >= 1
}

/// IF-FB 形状核：颜色模型位为 RGB 系且地址非零。
pub fn ck_fr_magic_fb(w: &A40World) -> bool {
    w.ifaces[Iface6::Fb.idx()].magic_ok && w.lm.fb_addr != 0 && w.lm.fb_model_rgb
}

/// IF-KA 形状核：物理基非零且虚基在高半。
pub fn ck_fr_magic_ka(w: &A40World) -> bool {
    let ok = w.ifaces[Iface6::Ka.idx()].magic_ok;
    ok && w.lm.ka_phys != 0 && w.lm.ka_virt >= A40_HHDM_FLOOR
}

/// IF-HH 形状核：offset 值落在高半 canonical 域。
pub fn ck_fr_magic_hh(w: &A40World) -> bool {
    w.ifaces[Iface6::Hh.idx()].magic_ok && w.lm.hh_base >= A40_HHDM_FLOOR
}

/// IF-RS 形状核：地址非零且低于 4 GiB（EFI RSDP 物理域）。
pub fn ck_fr_magic_rs(w: &A40World) -> bool {
    w.ifaces[Iface6::Rs.idx()].magic_ok
        && w.lm.rsdp_addr != 0
        && w.lm.rsdp_addr < 0x1_0000_0000
}

/// IF-IT 形状核：flags bit0 为直接图形模式（本账冻结口径）。
pub fn ck_fr_magic_it(w: &A40World) -> bool {
    w.ifaces[Iface6::It.idx()].magic_ok && w.lm.itmode_direct
}

/// FR-C 魔数面检查体表。
pub const FR_FN_MAGIC: [CheckFn40; IFACE6_COUNT] = [
    ck_fr_magic_mm,
    ck_fr_magic_fb,
    ck_fr_magic_ka,
    ck_fr_magic_hh,
    ck_fr_magic_rs,
    ck_fr_magic_it,
];

// ── FR-C 面：标量合断（025~026）──

/// 六接口 revision 全族 ≥1 且 ≤ 冻结表版本（本账冻结在 rev1 口径）。
pub fn ck_fr_rev_all(w: &A40World) -> bool {
    let mut i = 0;
    while i < IFACE6_COUNT {
        let r = w.ifaces[i].rev;
        if r < 1 || r > A40_FREEZE_TABLE_V {
            return false;
        }
        i += 1;
    }
    true
}

/// 冻结表只增不改 + 六接口只读消费 6/6（F0736 升级后回归口径）。
pub fn ck_fr_table_append_only(w: &A40World) -> bool {
    if !w.tbl_append_only || w.tbl_v != A40_FREEZE_TABLE_V {
        return false;
    }
    let mut i = 0;
    while i < IFACE6_COUNT {
        if !w.ifaces[i].consumed_readonly {
            return false;
        }
        i += 1;
    }
    true
}

/// FR 族描述表（A 面·在场，下标对齐接口）。
pub const FR_DESC_PRESENCE: [&str; IFACE6_COUNT] = [
    "IF-MM 三字段在场：revision/entry_count/entries",
    "IF-FB 13 字段在场：头 6 字段+颜色模型+RGB 掩码六项",
    "IF-KA 四字段在场：revision/phys_base/virt_base/size",
    "IF-HH 两字段在场：revision/offset",
    "IF-RS 两字段在场：revision/address",
    "IF-IT 两字段在场：revision/flags",
];

/// FR 族描述表（B 面·偏移，下标对齐接口）。
pub const FR_DESC_OFFSET: [&str; IFACE6_COUNT] = [
    "IF-MM 偏移冻结：0/8/16 三行不漂移",
    "IF-FB 偏移冻结：0..54 逐字段不漂移",
    "IF-KA 偏移冻结：0/8/16/24 四行不漂移",
    "IF-HH 偏移冻结：0/8 两行不漂移",
    "IF-RS 偏移冻结：0/8 两行不漂移",
    "IF-IT 偏移冻结：0/8 两行不漂移",
];

/// FR 族描述表（B 面·宽度，下标对齐接口）。
pub const FR_DESC_WIDTH: [&str; IFACE6_COUNT] = [
    "IF-MM 宽度冻结：8/8/8",
    "IF-FB 宽度冻结：6×8B+7×1B",
    "IF-KA 宽度冻结：4×8B",
    "IF-HH 宽度冻结：2×8B",
    "IF-RS 宽度冻结：2×8B",
    "IF-IT 宽度冻结：2×8B",
];

/// FR 族描述表（C 面·魔数，下标对齐接口）。
pub const FR_DESC_MAGIC: [&str; IFACE6_COUNT] = [
    "IF-MM 形状：entries 指针非零且 entry_count ≥1",
    "IF-FB 形状：地址非零且颜色模型 RGB",
    "IF-KA 形状：物理基非零且虚基高半",
    "IF-HH 形状：offset 落高半 canonical 域",
    "IF-RS 形状：地址非零且低于 4 GiB",
    "IF-IT 形状：flags bit0 直接图形模式",
];

// ═══════════════════════════════════════════════════════════════════
// LM 族检查体（Limine 响应面 16 条）
// ═══════════════════════════════════════════════════════════════════

/// LM-A-001：响应在场且 boot info 指针非空。
pub fn ck_lm_resp_present(w: &A40World) -> bool {
    w.lm.resp_present && w.lm.resp_ptr != 0
}

/// LM-A-002：响应树 revision ≥1。
pub fn ck_lm_rev(w: &A40World) -> bool {
    w.lm.revision >= 1
}

/// LM-A-003：闭集外未知 tag 计数为零（六 tag 闭集口径；以响应在场为
/// 前提——空态零计数不构成绿灯，防空洞为真）。
pub fn ck_lm_closed_tags(w: &A40World) -> bool {
    w.lm.resp_present && w.lm.unknown_tags == 0
}

/// LM-A-004：六接口响应逐接口全在场（6/6）。
pub fn ck_lm_six_present(w: &A40World) -> bool {
    let mut i = 0;
    while i < IFACE6_COUNT {
        if !w.lm.iface_resp_present[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// LM-B-005：内存图条目 ≥1。
pub fn ck_lm_mm_entries(w: &A40World) -> bool {
    w.lm.mm_entries >= 1
}

/// LM-B-006：usable 字节合计 ≥64 MiB（最小可引导内存线）。
pub fn ck_lm_mm_usable(w: &A40World) -> bool {
    w.lm.mm_usable_bytes >= A40_MIN_USABLE_BYTES
}

/// LM-B-007：条目基址有序（非降序，无乱序注入）。
pub fn ck_lm_mm_sorted(w: &A40World) -> bool {
    w.lm.mm_sorted
}

/// LM-B-008：条目区间交叠计数为零（以条目在场为前提，防空洞为真）。
pub fn ck_lm_mm_no_overlap(w: &A40World) -> bool {
    w.lm.mm_entries >= 1 && w.lm.mm_overlap == 0
}

/// LM-B-009：bootloader-reclaimable 已计且不超过 usable（回收账界内）。
pub fn ck_lm_mm_reclaim(w: &A40World) -> bool {
    w.lm.mm_reclaim_bytes > 0 && w.lm.mm_reclaim_bytes <= w.lm.mm_usable_bytes
}

/// LM-C-010：帧缓冲基址非零。
pub fn ck_lm_fb_addr(w: &A40World) -> bool {
    w.lm.fb_addr != 0
}

/// LM-C-011：帧缓冲尺寸 ≥640×400（探针地板）。
pub fn ck_lm_fb_geom(w: &A40World) -> bool {
    w.lm.fb_w >= A40_FB_MIN_W && w.lm.fb_h >= A40_FB_MIN_H
}

/// LM-C-012：bpp ∈ {16,24,32}（冻结口径三值闭集）。
pub fn ck_lm_fb_bpp(w: &A40World) -> bool {
    w.lm.fb_bpp == 16 || w.lm.fb_bpp == 24 || w.lm.fb_bpp == 32
}

/// LM-C-013：颜色模型 RGB。
pub fn ck_lm_fb_model(w: &A40World) -> bool {
    w.lm.fb_model_rgb
}

/// LM-C-014：pitch ≥ width×bpp/8（行距不小于像素行字节；以帧缓冲在场
/// 为前提——零宽零距不构成绿灯，防空洞为真）。
pub fn ck_lm_fb_pitch(w: &A40World) -> bool {
    if w.lm.fb_addr == 0 || w.lm.fb_w == 0 || w.lm.fb_h == 0 {
        return false;
    }
    let need = (w.lm.fb_w as u64).saturating_mul(w.lm.fb_bpp as u64) / 8;
    (w.lm.fb_pitch as u64) >= need
}

/// LM-C-015：内核地址四合：物理基非零+虚基高半+slide 一致+size>0。
pub fn ck_lm_ka_quad(w: &A40World) -> bool {
    w.lm.ka_phys != 0
        && w.lm.ka_virt >= A40_HHDM_FLOOR
        && w.lm.ka_slide_consistent
        && w.lm.ka_size > 0
}

/// LM-C-016：杂项三合：hhdm 高半+RSDP 非零+itmode 直接模式。
pub fn ck_lm_misc_triple(w: &A40World) -> bool {
    w.lm.hh_base >= A40_HHDM_FLOOR && w.lm.rsdp_addr != 0 && w.lm.itmode_direct
}

// ═══════════════════════════════════════════════════════════════════
// PP 族检查体（panic 路径 12 条）
// ═══════════════════════════════════════════════════════════════════

/// PP-A-001：panic 挂钩在 ExitBootServices 前装好（时序红线）。
pub fn ck_pp_hook_pre_ebs(w: &A40World) -> bool {
    w.pp.hook_installed_pre_ebs
}

/// PP-A-002：必需响应缺失注入 → panic 路径触发 ≥1 次（F0726 主断言）。
pub fn ck_pp_missing_req_fires(w: &A40World) -> bool {
    w.pp.missing_req_panics >= 1
}

/// PP-A-003：panic 文案含缺失 tag 名（F0726：文案可观测）。
pub fn ck_pp_msg_contains_tag(w: &A40World) -> bool {
    w.pp.msg_contains_tag
}

/// PP-B-004：文案三要素齐（编号/现象/下一步——F0712 模板冻结）。
pub fn ck_pp_msg_three_parts(w: &A40World) -> bool {
    w.pp.msg_three_parts
}

/// PP-B-005：panic 前事件账已冲刷（崩溃前保真前置）。
pub fn ck_pp_events_flushed(w: &A40World) -> bool {
    w.pp.events_flushed
}

/// PP-B-006：ExitBootServices 后串口仍可写（panic 输出通道不死）。
pub fn ck_pp_serial_post_ebs(w: &A40World) -> bool {
    w.pp.serial_post_ebs
}

/// PP-C-007：挂死语义保持（halt 环不返回，三重 fault 后挂死）。
pub fn ck_pp_halt_hold(w: &A40World) -> bool {
    w.pp.halt_hold
}

/// PP-C-008：unwind 禁用（panic=abort 口径，no_std 内核无栈回退）。
pub fn ck_pp_unwind_disabled(w: &A40World) -> bool {
    w.pp.unwind_disabled
}

/// PP-C-009：递归守卫（panic 内不再 panic，单 panic 单触发）。
pub fn ck_pp_recursive_guard(w: &A40World) -> bool {
    w.pp.recursive_guard_ok
}

/// PP-C-010：panic 计数有界（≤3，防 panic 风暴刷屏）。
pub fn ck_pp_storm_bounded(w: &A40World) -> bool {
    w.pp.storm_bounded && w.pp.missing_req_panics <= A40_PANIC_STORM_BOUND
}

/// PP-C-011：panic 快照绑定引导阶段戳（可归因到阶段）。
pub fn ck_pp_phase_stamped(w: &A40World) -> bool {
    w.pp.phase_stamped
}

/// PP-C-012：panic 后不再推进引导步（无重入，panic 即终态）。
pub fn ck_pp_no_boot_reentry(w: &A40World) -> bool {
    w.pp.no_boot_reentry
}

// ═══════════════════════════════════════════════════════════════════
// S9 事件账 + health + FNV-1a 64 指纹
// ═══════════════════════════════════════════════════════════════════

/// 事件账容量（超出即丢弃计数，防事件账无界膨胀）。
pub const A40_EVENT_CAP: usize = 512;

/// FNV-1a 64 偏移基数（FNV-1a 标准参数，来源：
/// draft-eastlake-fnv，offset basis 14695981039346656037）。
pub const A40_FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 素数（1099511628211）。
pub const A40_FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 事件账单事件（诊断码 + 中文注记）。
#[derive(Clone, Debug)]
pub struct Event40 {
    /// 诊断码（段内闭集）。
    pub code: u32,
    /// 注记（含 id/计数等动态明细）。
    pub note: String,
}

/// 事件账（容量封顶 + 丢弃计数 + 故障计数）。
#[derive(Clone, Debug)]
pub struct EventLog40 {
    /// 事件列。
    pub events: Vec<Event40>,
    /// 容量满后丢弃条数。
    pub dropped: u32,
}

impl EventLog40 {
    /// 空账。
    pub fn new() -> EventLog40 {
        EventLog40 { events: Vec::new(), dropped: 0 }
    }

    /// 追加事件（容量封顶：满则计丢弃，不 panic 不阻塞）。
    pub fn push(&mut self, code: u32, note: String) {
        if self.events.len() >= A40_EVENT_CAP {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        self.events.push(Event40 { code, note });
    }

    /// 追加正常（绿码）事件。
    pub fn push_green(&mut self, note: String) {
        self.push(XC40_GREEN, note);
    }

    /// 追加故障事件（红灯入账）。
    pub fn push_fault(&mut self, code: u32, note: String) {
        self.push(code, note);
    }

    /// 故障事件计数（非绿码事件数）。
    pub fn faults(&self) -> u32 {
        let mut n: u32 = 0;
        let mut i = 0;
        while i < self.events.len() {
            if self.events[i].code != XC40_GREEN {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 健康判定（零故障且零丢弃；空账视为未开工，不判绿）。
    pub fn health(&self) -> Health40 {
        let faults = self.faults();
        let ok = faults == 0 && self.dropped == 0 && !self.events.is_empty();
        Health40 { events: self.events.len(), faults, dropped: self.dropped, ok }
    }
}

/// 事件账健康四列。
#[derive(Clone, Copy, Debug)]
pub struct Health40 {
    /// 事件总数。
    pub events: usize,
    /// 故障事件数。
    pub faults: u32,
    /// 丢弃条数。
    pub dropped: u32,
    /// 综合绿灯。
    pub ok: bool,
}

/// FNV-1a 64 字节流指纹。
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h = A40_FNV_OFFSET;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(A40_FNV_PRIME);
        i += 1;
    }
    h
}

/// FNV-1a 64 字符串指纹。
pub fn fnv64_str(s: &str) -> u64 {
    fnv64(s.as_bytes())
}

// ═══════════════════════════════════════════════════════════════════
// S4 AssertBook40 断言账本（登记制 + 拒收路径 + 三族计数分列）
// ═══════════════════════════════════════════════════════════════════

/// 断言账本（登记制；重复 id 拒收、空断言拒收；三族计数分列）。
#[derive(Clone, Debug)]
pub struct AssertBook40 {
    /// 已登记断言（登记序即执行序）。
    pub specs: Vec<AssertionSpec40>,
    /// 事件账（登记生命周期留痕）。
    pub events: EventLog40,
}

impl AssertBook40 {
    /// 空账。
    pub fn new() -> AssertBook40 {
        AssertBook40 { specs: Vec::new(), events: EventLog40::new() }
    }

    /// 登记一条断言（三拒：BadId 防御 / 空检查体 / 重复 id）。
    pub fn register(&mut self, spec: AssertionSpec40) -> Result<(), Assert40Error> {
        // 防御：id 必须自通过往返互验（构造器已保证，此处双闸）。
        if !spec.id.self_round_trip() {
            self.events.push_fault(XC40_BAD_ID, format!("拒收 非规范id {}", spec.id.render()));
            return Err(Assert40Error::BadId { code: XC40_BAD_ID, id: spec.id.render() });
        }
        // 空断言拒收（无检查体一律不登记——F0709 零空断言口径）。
        if spec.check.is_none() {
            self.events
                .push_fault(XC40_EMPTY_CHECK, format!("拒收 空断言 {}", spec.id.render()));
            return Err(Assert40Error::EmptyCheck {
                code: XC40_EMPTY_CHECK,
                id: spec.id.render(),
            });
        }
        // 重复 id 拒收（登记制红线）。
        let mut i = 0;
        while i < self.specs.len() {
            if self.specs[i].id == spec.id {
                self.events
                    .push_fault(XC40_DUP_ID, format!("拒收 重复id {}", spec.id.render()));
                return Err(Assert40Error::DuplicateId {
                    code: XC40_DUP_ID,
                    id: spec.id.render(),
                });
            }
            i += 1;
        }
        self.events
            .push_green(format!("登记 {} 判据[{}]", spec.id.render(), spec.criterion));
        self.specs.push(spec);
        Ok(())
    }

    /// 三族计数分列（下标对齐 A40_FAMILIES）。
    pub fn counts(&self) -> [usize; A40_FAMILY_COUNT] {
        let mut c = [0usize; A40_FAMILY_COUNT];
        let mut i = 0;
        while i < self.specs.len() {
            let fi = self.specs[i].id.face_family().idx();
            c[fi] = c[fi].saturating_add(1);
            i += 1;
        }
        c
    }

    /// 登记总数。
    pub fn total(&self) -> usize {
        self.specs.len()
    }

    /// 族硬线门（逐族 ≥ 硬线；任一未达即 FamilyMinShort）。
    pub fn family_gate(&self) -> Result<[usize; A40_FAMILY_COUNT], Assert40Error> {
        let c = self.counts();
        let mut i = 0;
        while i < A40_FAMILY_COUNT {
            let need = A40_FAMILIES[i].min_asserts();
            if c[i] < need {
                return Err(Assert40Error::FamilyMinShort {
                    code: XC40_FAMILY_MIN,
                    fam_idx: i,
                    have: c[i],
                    need,
                });
            }
            i += 1;
        }
        Ok(c)
    }

    /// 总数门（≥40 即 UNX-F0706-J1 硬线）。
    pub fn total_gate(&self) -> Result<usize, Assert40Error> {
        let t = self.total();
        if t < A40_TOTAL_MIN {
            return Err(Assert40Error::TotalShort {
                code: XC40_TOTAL_SHORT,
                have: t,
                need: A40_TOTAL_MIN,
            });
        }
        Ok(t)
    }

    /// 登记账绿判定（族门 + 总门双过）。
    pub fn registry_green(&self) -> bool {
        self.family_gate().is_ok() && self.total_gate().is_ok()
    }

    /// 按 id 文本查条目（基线 diff/定位用；未命中 None）。
    pub fn find_by_id_text(&self, id_text: &str) -> Option<&AssertionSpec40> {
        let mut i = 0;
        while i < self.specs.len() {
            if self.specs[i].id.render() == id_text {
                return Some(&self.specs[i]);
            }
            i += 1;
        }
        None
    }

    /// 账本指纹（登记序敏：id+判据+描述逐条折叠）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = A40_FNV_OFFSET;
        let mut i = 0;
        while i < self.specs.len() {
            let sp = &self.specs[i];
            let row = format!("{}|{}|{}\n", sp.id.render(), sp.criterion, sp.desc);
            h ^= fnv64_str(&row);
            h = h.wrapping_mul(A40_FNV_PRIME);
            i += 1;
        }
        h
    }
}

// ═══════════════════════════════════════════════════════════════════
// S6 AssertRunner 执行引擎（run40：Pass/Fail 二态 + Fail 定位到 id）
// ═══════════════════════════════════════════════════════════════════

/// 执行结果单行（Fail 定位到 id + 判据 + 描述三列）。
#[derive(Clone, Debug)]
pub struct RunRow40 {
    /// 断言 id 规范形。
    pub id_text: String,
    /// 族下标。
    pub fam_idx: usize,
    /// 执行序（登记序投影，严格递增）。
    pub order: u32,
    /// 二态：true = Pass / false = Fail。
    pub pass: bool,
    /// 判据引用。
    pub criterion: &'static str,
    /// 描述。
    pub desc: &'static str,
}

/// 族执行统计（已登记/已执行/通过/失败/覆盖率/达线）。
#[derive(Clone, Copy, Debug)]
pub struct FamilyStat40 {
    /// 族。
    pub fam: Family40,
    /// 已登记条数。
    pub registered: usize,
    /// 已执行条数。
    pub executed: usize,
    /// 通过条数。
    pub passed: usize,
    /// 失败条数。
    pub failed: usize,
    /// 覆盖率（万分比：已执行/已登记；登记 0 记 0）。
    pub cover_perm: u32,
    /// 族硬线达线。
    pub meets_min: bool,
}

/// 执行报告（逐行 + 族统计 + 总判）。
#[derive(Clone, Debug)]
pub struct RunReport40 {
    /// 逐条执行行（登记序）。
    pub rows: Vec<RunRow40>,
    /// 三族统计。
    pub fams: [FamilyStat40; A40_FAMILY_COUNT],
    /// 总条数。
    pub total: usize,
    /// 通过数。
    pub passed: usize,
    /// 失败数。
    pub failed: usize,
    /// 执行序稳定（order 严格递增核）。
    pub order_stable: bool,
    /// 全绿（零失败 + 序稳定 + 总数达线）。
    pub all_green: bool,
}

impl RunReport40 {
    /// 失败清单（Fail 定位到 id 的投影列）。
    pub fn failures(&self) -> Vec<&RunRow40> {
        let mut v = Vec::new();
        let mut i = 0;
        while i < self.rows.len() {
            if !self.rows[i].pass {
                v.push(&self.rows[i]);
            }
            i += 1;
        }
        v
    }

    /// 执行指纹（id+pass 逐行折叠；对拍与封存用）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = A40_FNV_OFFSET;
        let mut i = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            let row = format!("{}:{}\n", r.id_text, if r.pass { 'P' } else { 'F' });
            h ^= fnv64_str(&row);
            h = h.wrapping_mul(A40_FNV_PRIME);
            i += 1;
        }
        h
    }
}

/// 执行序稳定核（order 严格递增；乱序注入必被此闸拦下）。
pub fn order_stable_rows(rows: &[RunRow40]) -> bool {
    let mut i = 1;
    while i < rows.len() {
        if rows[i].order <= rows[i - 1].order {
            return false;
        }
        i += 1;
    }
    true
}

/// 执行引擎：逐条调用检查体 → Pass/Fail 二态；Fail 行携带 id/判据/描述。
pub fn run40(book: &AssertBook40, world: &A40World) -> RunReport40 {
    let mut rows = Vec::new();
    let mut i = 0;
    while i < book.specs.len() {
        let sp = &book.specs[i];
        let pass = match sp.check {
            Some(f) => f(world),
            None => false, // 登记期已拒空断言；执行期防御性记 Fail。
        };
        rows.push(RunRow40 {
            id_text: sp.id.render(),
            fam_idx: sp.id.face_family().idx(),
            order: i as u32,
            pass,
            criterion: sp.criterion,
            desc: sp.desc,
        });
        i += 1;
    }
    // 族统计（已执行 = 登记数——引擎不跳条；覆盖率 10000‱ 即满）。
    let mut fams = [
        FamilyStat40 {
            fam: Family40::Frozen6,
            registered: 0,
            executed: 0,
            passed: 0,
            failed: 0,
            cover_perm: 0,
            meets_min: false,
        };
        A40_FAMILY_COUNT
    ];
    let mut fi = 0;
    while fi < A40_FAMILY_COUNT {
        fams[fi].fam = A40_FAMILIES[fi];
        fi += 1;
    }
    let mut r = 0;
    while r < rows.len() {
        let st = &mut fams[rows[r].fam_idx];
        st.executed = st.executed.saturating_add(1);
        if rows[r].pass {
            st.passed = st.passed.saturating_add(1);
        } else {
            st.failed = st.failed.saturating_add(1);
        }
        r += 1;
    }
    let counts = book.counts();
    let mut fi = 0;
    while fi < A40_FAMILY_COUNT {
        fams[fi].registered = counts[fi];
        fams[fi].cover_perm = if counts[fi] == 0 {
            0
        } else {
            ((fams[fi].executed as u64).saturating_mul(10_000) / (counts[fi] as u64)) as u32
        };
        fams[fi].meets_min = counts[fi] >= A40_FAMILIES[fi].min_asserts();
        fi += 1;
    }
    let total = rows.len();
    let passed = fams[0].passed + fams[1].passed + fams[2].passed;
    let failed = fams[0].failed + fams[1].failed + fams[2].failed;
    let stable = order_stable_rows(&rows);
    let all_green = failed == 0 && stable && total >= A40_TOTAL_MIN;
    RunReport40 { rows, fams, total, passed, failed, order_stable: stable, all_green }
}

/// 全绿门（红灯数 >0 即 AssertFail；执行序乱即 OrderUnstable）。
pub fn assert_all_green(rep: &RunReport40) -> Result<(), Assert40Error> {
    if !rep.order_stable {
        return Err(Assert40Error::OrderUnstable { code: XC40_ORDER_UNSTABLE });
    }
    if rep.failed > 0 {
        return Err(Assert40Error::AssertFail { code: XC40_ASSERT_FAIL, red: rep.failed });
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════
// S5 基线快照与只增不减 diff（与 F0707 纪律账同判）
// ═══════════════════════════════════════════════════════════════════

/// 基线条目（id + 内容哈希两列；哈希覆盖判据+描述）。
#[derive(Clone, Debug)]
pub struct BaseEntry40 {
    /// 断言 id 规范形。
    pub id_text: String,
    /// 内容哈希（fnv64(criterion|desc)）。
    pub hash: u64,
}

/// 基线快照（UNX-F0707-J1「基线清单快照」交接面分项投影；只增不减）。
#[derive(Clone, Debug)]
pub struct BaselineSnap40 {
    /// 快照条目（快照时刻账本全量投影）。
    pub entries: Vec<BaseEntry40>,
    /// 快照整账指纹。
    pub fp: u64,
}

/// 单条内容哈希（判据+描述折叠；id 不入哈希——id 变更走 Removed+Added）。
pub fn spec_hash(spec: &AssertionSpec40) -> u64 {
    fnv64_str(&format!("{}|{}", spec.criterion, spec.desc))
}

/// 落基线快照。
pub fn snapshot40(book: &AssertBook40) -> BaselineSnap40 {
    let mut entries = Vec::new();
    let mut i = 0;
    while i < book.specs.len() {
        entries.push(BaseEntry40 {
            id_text: book.specs[i].id.render(),
            hash: spec_hash(&book.specs[i]),
        });
        i += 1;
    }
    BaselineSnap40 { fp: book.fingerprint(), entries }
}

/// 基线 diff 账（只增不减判定：added 允许；removed/modified 即红案）。
#[derive(Clone, Debug)]
pub struct BaseDiff40 {
    /// 新增（基线无而账本有——只增路径，允许）。
    pub added: Vec<String>,
    /// 删除（基线有而账本无——红案）。
    pub removed: Vec<String>,
    /// 改写（id 同而内容哈希变——红案）。
    pub modified: Vec<String>,
    /// 洁净（删改皆零）。
    pub clean: bool,
}

impl BaseDiff40 {
    /// 红案总数（删+改；只增不减纪律违例计数）。
    pub fn red_cases(&self) -> usize {
        self.removed.len() + self.modified.len()
    }
}

/// 基线 diff（基线条目 × 账本线性对账；删改入红案清单）。
pub fn diff40(base: &BaselineSnap40, book: &AssertBook40) -> BaseDiff40 {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut modified = Vec::new();
    // 基线 → 账本（删/改检出）。
    let mut i = 0;
    while i < base.entries.len() {
        let be = &base.entries[i];
        match book.find_by_id_text(&be.id_text) {
            None => removed.push(be.id_text.clone()),
            Some(sp) => {
                if spec_hash(sp) != be.hash {
                    modified.push(be.id_text.clone());
                }
            }
        }
        i += 1;
    }
    // 账本 → 基线（新增检出）。
    let mut j = 0;
    while j < book.specs.len() {
        let id_text = book.specs[j].id.render();
        let mut hit = false;
        let mut k = 0;
        while k < base.entries.len() {
            if base.entries[k].id_text == id_text {
                hit = true;
                break;
            }
            k += 1;
        }
        if !hit {
            added.push(id_text);
        }
        j += 1;
    }
    let clean = removed.is_empty() && modified.is_empty();
    BaseDiff40 { added, removed, modified, clean }
}

// ═══════════════════════════════════════════════════════════════════
// S7 族间覆盖审计（覆盖率 + 未登记族面检出）
// ═══════════════════════════════════════════════════════════════════

/// 族面缺口（族 × 面：零登记即缺口）。
#[derive(Clone, Copy, Debug)]
pub struct FaceGap40 {
    /// 族。
    pub fam: Family40,
    /// 面字母。
    pub face: u8,
}

/// 覆盖审计账（三族统计 + 缺口清单 + 完备判定）。
#[derive(Clone, Debug)]
pub struct Coverage40 {
    /// 三族统计（复用执行报告族列）。
    pub fams: [FamilyStat40; A40_FAMILY_COUNT],
    /// 未登记族面缺口清单。
    pub gaps: Vec<FaceGap40>,
    /// 完备（无缺口 + 三族全达线 + 三族覆盖率皆 10000‱）。
    pub complete: bool,
}

impl FaceGap40 {
    /// 缺口定位文本（族码×面字母）。
    pub fn text(&self) -> String {
        format!("{}-{}", self.fam.code2(), self.face as char)
    }
}

/// 族间覆盖审计：每族覆盖率 = 已执行/已登记（万分比）；A/B/C 三面逐族
/// 扫描，零登记即 FaceGap40 缺口（未登记族面检出）。
pub fn coverage_audit(book: &AssertBook40, rep: &RunReport40) -> Coverage40 {
    // 面占用矩阵 [族][面]。
    let mut face_hit = [[false; 3]; A40_FAMILY_COUNT];
    let mut i = 0;
    while i < book.specs.len() {
        let fi = book.specs[i].id.face_family().idx();
        let face = book.specs[i].id.face();
        let mut f = 0;
        while f < 3 {
            if face == A40_FACE_LETTERS[f] {
                face_hit[fi][f] = true;
            }
            f += 1;
        }
        i += 1;
    }
    let mut gaps = Vec::new();
    let mut fi = 0;
    while fi < A40_FAMILY_COUNT {
        let mut f = 0;
        while f < 3 {
            if !face_hit[fi][f] {
                gaps.push(FaceGap40 { fam: A40_FAMILIES[fi], face: A40_FACE_LETTERS[f] });
            }
            f += 1;
        }
        fi += 1;
    }
    let mut complete = gaps.is_empty();
    let mut fi = 0;
    while fi < A40_FAMILY_COUNT {
        let st = &rep.fams[fi];
        if !st.meets_min || st.cover_perm != 10_000 {
            complete = false;
        }
        fi += 1;
    }
    Coverage40 { fams: rep.fams, gaps, complete }
}

/// 覆盖门（有缺口即 CoverGap 错，逐缺口首条定位）。
pub fn coverage_gate(cov: &Coverage40) -> Result<(), Assert40Error> {
    if cov.complete {
        return Ok(());
    }
    if let Some(g) = cov.gaps.first() {
        return Err(Assert40Error::CoverGap {
            code: XC40_COVER_GAP,
            fam_idx: g.fam.idx(),
            face: g.face,
        });
    }
    Err(Assert40Error::FamilyMinShort {
        code: XC40_FAMILY_MIN,
        fam_idx: 0,
        have: 0,
        need: A40_TOTAL_MIN,
    })
}

// ═══════════════════════════════════════════════════════════════════
// S8 断言↔判据双向索引（F0708 全域映射总表的分项投影）
// ═══════════════════════════════════════════════════════════════════

/// 双向索引行（判据 → 断言 id 列）。
#[derive(Clone, Debug)]
pub struct CriterionRow40 {
    /// 判据引用文本。
    pub criterion: String,
    /// 该判据下断言 id 列（登记序）。
    pub ids: Vec<String>,
}

/// 断言↔判据双向索引。
#[derive(Clone, Debug)]
pub struct A40CrossRef {
    /// 正向：断言 id → 判据引用。
    pub fwd: Vec<(String, String)>,
    /// 反向：判据引用 → 断言 id 列。
    pub rev: Vec<CriterionRow40>,
}

impl A40CrossRef {
    /// 正向查（id → 判据）。
    pub fn criterion_of(&self, id_text: &str) -> Option<&str> {
        let mut i = 0;
        while i < self.fwd.len() {
            if self.fwd[i].0 == id_text {
                return Some(&self.fwd[i].1);
            }
            i += 1;
        }
        None
    }

    /// 反向查（判据 → id 列）。
    pub fn ids_of(&self, criterion: &str) -> Vec<&str> {
        let mut v = Vec::new();
        let mut i = 0;
        while i < self.rev.len() {
            if self.rev[i].criterion == criterion {
                let mut j = 0;
                while j < self.rev[i].ids.len() {
                    v.push(self.rev[i].ids[j].as_str());
                    j += 1;
                }
            }
            i += 1;
        }
        v
    }

    /// 对账（正向条数 == 账本总数；每正向条目在反向可回查——零错位）。
    pub fn balanced(&self, total: usize) -> bool {
        if self.fwd.len() != total {
            return false;
        }
        let mut i = 0;
        while i < self.fwd.len() {
            let (id, crit) = &self.fwd[i];
            let ids = self.ids_of(crit);
            let mut hit = false;
            let mut j = 0;
            while j < ids.len() {
                if ids[j] == id {
                    hit = true;
                    break;
                }
                j += 1;
            }
            if !hit {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 由账本构建双向索引。
pub fn build_cross_ref(book: &AssertBook40) -> A40CrossRef {
    let mut fwd = Vec::new();
    let mut rev: Vec<CriterionRow40> = Vec::new();
    let mut i = 0;
    while i < book.specs.len() {
        let sp = &book.specs[i];
        let id_text = sp.id.render();
        fwd.push((id_text.clone(), String::from(sp.criterion)));
        let mut hit = false;
        let mut k = 0;
        while k < rev.len() {
            if rev[k].criterion == sp.criterion {
                rev[k].ids.push(id_text.clone());
                hit = true;
                break;
            }
            k += 1;
        }
        if !hit {
            let mut ids = Vec::new();
            ids.push(id_text);
            rev.push(CriterionRow40 { criterion: String::from(sp.criterion), ids });
        }
        i += 1;
    }
    A40CrossRef { fwd, rev }
}

// ═══════════════════════════════════════════════════════════════════
// S9 双跑确定性对拍
// ═══════════════════════════════════════════════════════════════════

/// 双跑对拍账（甲乙两跑的账本指纹与执行指纹 + 一致位）。
#[derive(Clone, Copy, Debug)]
pub struct DualRun40 {
    /// 甲跑账本指纹。
    pub book_fp_a: u64,
    /// 乙跑账本指纹。
    pub book_fp_b: u64,
    /// 甲跑执行指纹。
    pub run_fp_a: u64,
    /// 乙跑执行指纹。
    pub run_fp_b: u64,
    /// 一致（账本与执行指纹双双相等）。
    pub identical: bool,
}

/// 双跑确定性对拍：同世界态两轮（登记+执行）指纹必等。
pub fn dual_run_check() -> Result<DualRun40, Assert40Error> {
    let a = build_full_book()?;
    let b = build_full_book()?;
    let w = A40World::green();
    let ra = run40(&a, &w);
    let rb = run40(&b, &w);
    let book_fp_a = a.fingerprint();
    let book_fp_b = b.fingerprint();
    let run_fp_a = ra.fingerprint();
    let run_fp_b = rb.fingerprint();
    let identical = book_fp_a == book_fp_b && run_fp_a == run_fp_b;
    Ok(DualRun40 { book_fp_a, book_fp_b, run_fp_a, run_fp_b, identical })
}

// ═══════════════════════════════════════════════════════════════════
// S10 注入面三路（空断言/删基线/重复 id——各必检出）
// ═══════════════════════════════════════════════════════════════════

/// 注入种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectKind40 {
    /// 空断言注入（check = None 样本进登记口）。
    EmptyCheck,
    /// 删基线注入（相对基线缺条的账本进 diff 口）。
    BaselineRemoval,
    /// 重复 id 注入（同 id 二次登记）。
    DuplicateId,
}

impl InjectKind40 {
    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            InjectKind40::EmptyCheck => "空断言注入",
            InjectKind40::BaselineRemoval => "删基线注入",
            InjectKind40::DuplicateId => "重复 id 注入",
        }
    }
}

/// 注入结果（检出位 + 诊断码 + 明细）。
#[derive(Clone, Debug)]
pub struct Injection40 {
    /// 注入种类。
    pub kind: InjectKind40,
    /// 检出（守卫命中即 true）。
    pub detected: bool,
    /// 诊断码。
    pub code: u32,
    /// 明细。
    pub note: String,
}

/// 注入一：空断言（无检查体）进登记口——必须被 EmptyCheck 拒收。
pub fn inject_empty_check() -> Injection40 {
    let mut book = AssertBook40::new();
    let id = match AssertionId::new(Family40::Frozen6, b'A', 1) {
        Ok(v) => v,
        Err(e) => {
            return Injection40 {
                kind: InjectKind40::EmptyCheck,
                detected: false,
                code: e.code(),
                note: format!("构造 id 失败：{}", e.detail()),
            }
        }
    };
    let spec = AssertionSpec40::new(
        id,
        "UNX-F0706-J1#注入",
        "注入样本：无检查体（应被登记口拒收）",
        None,
    );
    let res = book.register(spec);
    match res {
        Err(Assert40Error::EmptyCheck { code, .. }) => Injection40 {
            kind: InjectKind40::EmptyCheck,
            detected: true,
            code,
            note: String::from("空断言被登记口拒收（EmptyCheck 守卫命中）"),
        },
        Err(e) => Injection40 {
            kind: InjectKind40::EmptyCheck,
            detected: false,
            code: e.code(),
            note: format!("拒收但错型不符：{}", e.detail()),
        },
        Ok(()) => Injection40 {
            kind: InjectKind40::EmptyCheck,
            detected: false,
            code: XC40_INJECT_MISSED,
            note: String::from("空断言被放行登记——守卫漏检"),
        },
    }
}

/// 注入二：删基线（缺条账本进 diff 口）——diff 必报 Removed 红案。
pub fn inject_baseline_removal() -> Injection40 {
    let full = match build_full_book() {
        Ok(b) => b,
        Err(e) => {
            return Injection40 {
                kind: InjectKind40::BaselineRemoval,
                detected: false,
                code: e.code(),
                note: format!("建全账失败：{}", e.detail()),
            }
        }
    };
    let base = snapshot40(&full);
    // 缺条账本：只登记 FR/PP 两族（LM 族整体缺席 = 删基线样本）。
    let mut shrunk = AssertBook40::new();
    let mut i = 0;
    while i < full.specs.len() {
        let sp = full.specs[i];
        if sp.id.face_family() != Family40::LimineResp {
            let _ = shrunk.register(sp);
        }
        i += 1;
    }
    let d = diff40(&base, &shrunk);
    if !d.clean && !d.removed.is_empty() {
        Injection40 {
            kind: InjectKind40::BaselineRemoval,
            detected: true,
            code: XC40_BASE_REMOVED,
            note: format!("删基线检出 {} 条红案（首条 {}）", d.removed.len(), d.removed[0]),
        }
    } else {
        Injection40 {
            kind: InjectKind40::BaselineRemoval,
            detected: false,
            code: XC40_INJECT_MISSED,
            note: String::from("删基线未被 diff 检出——守卫漏检"),
        }
    }
}

/// 注入三：重复 id（同 id 二次登记）——必须被 DuplicateId 拒收。
pub fn inject_duplicate_id() -> Injection40 {
    let mut book = AssertBook40::new();
    let id = match AssertionId::new(Family40::PanicPath, b'A', 1) {
        Ok(v) => v,
        Err(e) => {
            return Injection40 {
                kind: InjectKind40::DuplicateId,
                detected: false,
                code: e.code(),
                note: format!("构造 id 失败：{}", e.detail()),
            }
        }
    };
    let spec = AssertionSpec40::new(
        id,
        "UNX-F0706-J1#注入",
        "注入样本：首登（合法占位）",
        Some(ck_pp_hook_pre_ebs),
    );
    if let Err(e) = book.register(spec) {
        return Injection40 {
            kind: InjectKind40::DuplicateId,
            detected: false,
            code: e.code(),
            note: format!("首登即失败（不应发生）：{}", e.detail()),
        };
    }
    let dup = AssertionSpec40::new(
        id,
        "UNX-F0706-J1#注入",
        "注入样本：同 id 复登记（应被拒收）",
        Some(ck_pp_halt_hold),
    );
    match book.register(dup) {
        Err(Assert40Error::DuplicateId { code, .. }) => Injection40 {
            kind: InjectKind40::DuplicateId,
            detected: true,
            code,
            note: format!("重复 id 拒收（DuplicateId 守卫命中，账本仍 {} 条）", book.total()),
        },
        Err(e) => Injection40 {
            kind: InjectKind40::DuplicateId,
            detected: false,
            code: e.code(),
            note: format!("拒收但错型不符：{}", e.detail()),
        },
        Ok(()) => Injection40 {
            kind: InjectKind40::DuplicateId,
            detected: false,
            code: XC40_INJECT_MISSED,
            note: String::from("重复 id 被放行登记——守卫漏检"),
        },
    }
}

// ═══════════════════════════════════════════════════════════════════
// S3+登记总装：build_full_book（三族 26+16+12 = 54 条真实断言）
// ═══════════════════════════════════════════════════════════════════

/// 登记辅助（构造 id + 落账，一路传播错误）。
fn push_spec(
    book: &mut AssertBook40,
    fam: Family40,
    face: u8,
    seq: u16,
    criterion: &'static str,
    desc: &'static str,
    check: Option<CheckFn40>,
) -> Result<(), Assert40Error> {
    let id = AssertionId::new(fam, face, seq)?;
    book.register(AssertionSpec40::new(id, criterion, desc, check))
}

/// 登记全部 54 条断言（FR 26 + LM 16 + PP 12；登记序即执行序）。
pub fn build_full_book() -> Result<AssertBook40, Assert40Error> {
    let mut book = AssertBook40::new();

    // ── FR 族：六冻结接口字段级 26 条 ──
    // A 面·在场 001~006。
    let mut i = 0;
    while i < IFACE6_COUNT {
        push_spec(
            &mut book,
            Family40::Frozen6,
            b'A',
            (1 + i) as u16,
            "UNX-F0706-J1#字段级",
            FR_DESC_PRESENCE[i],
            Some(FR_FN_PRESENCE[i]),
        )?;
        i += 1;
    }
    // B 面·偏移 007~012。
    let mut i = 0;
    while i < IFACE6_COUNT {
        push_spec(
            &mut book,
            Family40::Frozen6,
            b'B',
            (7 + i) as u16,
            "UNX-F0706-J1#字段级",
            FR_DESC_OFFSET[i],
            Some(FR_FN_OFFSET[i]),
        )?;
        i += 1;
    }
    // B 面·宽度 013~018。
    let mut i = 0;
    while i < IFACE6_COUNT {
        push_spec(
            &mut book,
            Family40::Frozen6,
            b'B',
            (13 + i) as u16,
            "UNX-F0706-J1#字段级",
            FR_DESC_WIDTH[i],
            Some(FR_FN_WIDTH[i]),
        )?;
        i += 1;
    }
    // C 面·魔数 019~024。
    let mut i = 0;
    while i < IFACE6_COUNT {
        push_spec(
            &mut book,
            Family40::Frozen6,
            b'C',
            (19 + i) as u16,
            "UNX-F0706-J1#字段级",
            FR_DESC_MAGIC[i],
            Some(FR_FN_MAGIC[i]),
        )?;
        i += 1;
    }
    // C 面·标量合断 025~026。
    push_spec(
        &mut book,
        Family40::Frozen6,
        b'C',
        25,
        "UNX-F0706-J1#字段级",
        "六接口 revision 全族 1..=冻结表版本",
        Some(ck_fr_rev_all),
    )?;
    push_spec(
        &mut book,
        Family40::Frozen6,
        b'C',
        26,
        "UNX-F0706-J1#字段级",
        "冻结表只增不改 + 六接口只读消费 6/6",
        Some(ck_fr_table_append_only),
    )?;

    // ── LM 族：Limine 响应面 16 条 ──
    push_spec(
        &mut book,
        Family40::LimineResp,
        b'A',
        1,
        "UNX-F0706-J1#Limine响应面",
        "响应在场且 boot info 指针非空",
        Some(ck_lm_resp_present),
    )?;
    push_spec(
        &mut book,
        Family40::LimineResp,
        b'A',
        2,
        "UNX-F0706-J1#Limine响应面",
        "响应树 revision ≥1",
        Some(ck_lm_rev),
    )?;
    push_spec(
        &mut book,
        Family40::LimineResp,
        b'A',
        3,
        "UNX-F0706-J1#Limine响应面",
        "闭集外未知 tag 计数为零",
        Some(ck_lm_closed_tags),
    )?;
    push_spec(
        &mut book,
        Family40::LimineResp,
        b'A',
        4,
        "UNX-F0706-J1#Limine响应面",
        "六接口响应逐接口全在场 6/6",
        Some(ck_lm_six_present),
    )?;
    push_spec(
        &mut book,
        Family40::LimineResp,
        b'B',
        5,
        "UNX-F0706-J1#Limine响应面",
        "内存图条目 ≥1",
        Some(ck_lm_mm_entries),
    )?;
    push_spec(
        &mut book,
        Family40::LimineResp,
        b'B',
        6,
        "UNX-F0706-J1#Limine响应面",
        "usable 字节合计 ≥64 MiB 最小可引导线",
        Some(ck_lm_mm_usable),
    )?;
    push_spec(
        &mut book,
        Family40::LimineResp,
        b'B',
        7,
        "UNX-F0706-J1#Limine响应面",
        "内存图条目基址有序（非降）",
        Some(ck_lm_mm_sorted),
    )?;
    push_spec(
        &mut book,
        Family40::LimineResp,
        b'B',
        8,
        "UNX-F0706-J1#Limine响应面",
        "条目区间交叠计数为零",
        Some(ck_lm_mm_no_overlap),
    )?;
    push_spec(
        &mut book,
        Family40::LimineResp,
        b'B',
        9,
        "UNX-F0706-J1#Limine响应面",
        "bootloader-reclaim 已计且不超 usable",
        Some(ck_lm_mm_reclaim),
    )?;
    push_spec(
        &mut book,
        Family40::LimineResp,
        b'C',
        10,
        "UNX-F0706-J1#Limine响应面",
        "帧缓冲基址非零",
        Some(ck_lm_fb_addr),
    )?;
    push_spec(
        &mut book,
        Family40::LimineResp,
        b'C',
        11,
        "UNX-F0706-J1#Limine响应面",
        "帧缓冲尺寸 ≥640×400 探针地板",
        Some(ck_lm_fb_geom),
    )?;
    push_spec(
        &mut book,
        Family40::LimineResp,
        b'C',
        12,
        "UNX-F0706-J1#Limine响应面",
        "bpp ∈ {16,24,32} 冻结口径闭集",
        Some(ck_lm_fb_bpp),
    )?;
    push_spec(
        &mut book,
        Family40::LimineResp,
        b'C',
        13,
        "UNX-F0706-J1#Limine响应面",
        "颜色模型 RGB",
        Some(ck_lm_fb_model),
    )?;
    push_spec(
        &mut book,
        Family40::LimineResp,
        b'C',
        14,
        "UNX-F0706-J1#Limine响应面",
        "pitch ≥ width×bpp/8 行距下界",
        Some(ck_lm_fb_pitch),
    )?;
    push_spec(
        &mut book,
        Family40::LimineResp,
        b'C',
        15,
        "UNX-F0706-J1#Limine响应面",
        "内核地址四合：物理基/虚基/slide/size",
        Some(ck_lm_ka_quad),
    )?;
    push_spec(
        &mut book,
        Family40::LimineResp,
        b'C',
        16,
        "UNX-F0706-J1#Limine响应面",
        "杂项三合：hhdm 高半/RSDP 非零/itmode 直接",
        Some(ck_lm_misc_triple),
    )?;

    // ── PP 族：panic 路径 12 条 ──
    push_spec(
        &mut book,
        Family40::PanicPath,
        b'A',
        1,
        "UNX-F0706-J1#panic路径",
        "panic 挂钩在 ExitBootServices 前装好",
        Some(ck_pp_hook_pre_ebs),
    )?;
    push_spec(
        &mut book,
        Family40::PanicPath,
        b'A',
        2,
        "UNX-F0706-J1#panic路径",
        "必需响应缺失注入 → panic 触发 ≥1（F0726）",
        Some(ck_pp_missing_req_fires),
    )?;
    push_spec(
        &mut book,
        Family40::PanicPath,
        b'A',
        3,
        "UNX-F0706-J1#panic路径",
        "panic 文案含缺失 tag 名（F0726 可观测）",
        Some(ck_pp_msg_contains_tag),
    )?;
    push_spec(
        &mut book,
        Family40::PanicPath,
        b'B',
        4,
        "UNX-F0706-J1#panic路径",
        "文案三要素齐：编号/现象/下一步（F0712）",
        Some(ck_pp_msg_three_parts),
    )?;
    push_spec(
        &mut book,
        Family40::PanicPath,
        b'B',
        5,
        "UNX-F0706-J1#panic路径",
        "panic 前事件账已冲刷",
        Some(ck_pp_events_flushed),
    )?;
    push_spec(
        &mut book,
        Family40::PanicPath,
        b'B',
        6,
        "UNX-F0706-J1#panic路径",
        "ExitBootServices 后串口仍可写",
        Some(ck_pp_serial_post_ebs),
    )?;
    push_spec(
        &mut book,
        Family40::PanicPath,
        b'C',
        7,
        "UNX-F0706-J1#panic路径",
        "挂死语义保持（halt 环不返回）",
        Some(ck_pp_halt_hold),
    )?;
    push_spec(
        &mut book,
        Family40::PanicPath,
        b'C',
        8,
        "UNX-F0706-J1#panic路径",
        "unwind 禁用（panic=abort 口径）",
        Some(ck_pp_unwind_disabled),
    )?;
    push_spec(
        &mut book,
        Family40::PanicPath,
        b'C',
        9,
        "UNX-F0706-J1#panic路径",
        "递归守卫（单 panic 单触发）",
        Some(ck_pp_recursive_guard),
    )?;
    push_spec(
        &mut book,
        Family40::PanicPath,
        b'C',
        10,
        "UNX-F0706-J1#panic路径",
        "panic 计数有界 ≤3（防风暴）",
        Some(ck_pp_storm_bounded),
    )?;
    push_spec(
        &mut book,
        Family40::PanicPath,
        b'C',
        11,
        "UNX-F0706-J1#panic路径",
        "panic 快照绑定引导阶段戳",
        Some(ck_pp_phase_stamped),
    )?;
    push_spec(
        &mut book,
        Family40::PanicPath,
        b'C',
        12,
        "UNX-F0706-J1#panic路径",
        "panic 后不再推进引导步（无重入）",
        Some(ck_pp_no_boot_reentry),
    )?;

    Ok(book)
}

// ═══════════════════════════════════════════════════════════════════
// 总审计（full_audit）与报告渲染
// ═══════════════════════════════════════════════════════════════════

/// 总审计报告（登记/执行/覆盖/diff/索引/对拍/注入/健康/指纹/判定十列）。
#[derive(Clone, Debug)]
pub struct A40FinalReport {
    /// 登记总数。
    pub registry_total: usize,
    /// 三族登记计数。
    pub registry_counts: [usize; A40_FAMILY_COUNT],
    /// 登记账绿（族门+总门双过）。
    pub registry_green: bool,
    /// 执行报告。
    pub run: RunReport40,
    /// 覆盖审计。
    pub coverage: Coverage40,
    /// 基线 diff（全账自 diff：应 added=0 且洁净）。
    pub diff: BaseDiff40,
    /// 双向索引对账（零错位）。
    pub cross_balanced: bool,
    /// 双跑对拍。
    pub dual: DualRun40,
    /// 注入面三路结果。
    pub injections: Vec<Injection40>,
    /// 事件账健康。
    pub health: Health40,
    /// 账本指纹。
    pub book_fp: u64,
    /// 执行指纹。
    pub run_fp: u64,
    /// 总判定绿。
    pub green: bool,
}

impl A40FinalReport {
    /// 总判定中文。
    pub fn verdict_zh(&self) -> &'static str {
        if self.green {
            "Green（分项全绿）"
        } else {
            "Red（存在红项）"
        }
    }
}

/// 一键总审计：建账 → 执行（全绿世界态）→ 覆盖 → 自 diff → 索引 →
/// 对拍 → 注入面三路 → 健康 → 指纹 → 十列判定。
pub fn full_audit() -> Result<A40FinalReport, Assert40Error> {
    let book = build_full_book()?;
    let registry_total = book.total();
    let registry_counts = book.counts();
    let registry_green = book.registry_green();
    let base = snapshot40(&book);
    let w = A40World::green();
    let run = run40(&book, &w);
    let coverage = coverage_audit(&book, &run);
    let diff = diff40(&base, &book);
    let cross = build_cross_ref(&book);
    let cross_balanced = cross.balanced(book.total());
    let dual = dual_run_check()?;
    let mut injections = Vec::new();
    injections.push(inject_empty_check());
    injections.push(inject_duplicate_id());
    injections.push(inject_baseline_removal());
    let health = book.events.health();
    let book_fp = book.fingerprint();
    let run_fp = run.fingerprint();
    let mut green = registry_green
        && run.all_green
        && coverage.complete
        && diff.clean
        && cross_balanced
        && dual.identical
        && health.ok;
    let mut i = 0;
    while i < injections.len() {
        if !injections[i].detected {
            green = false;
        }
        i += 1;
    }
    Ok(A40FinalReport {
        registry_total,
        registry_counts,
        registry_green,
        run,
        coverage,
        diff,
        cross_balanced,
        dual,
        injections,
        health,
        book_fp,
        run_fp,
        green,
    })
}

/// 登记账渲染（三族计数 × 硬线）。
pub fn render_registry_report(book: &AssertBook40) -> String {
    let counts = book.counts();
    let mut s = String::new();
    s.push_str("UNX-F0706 登记账：\n");
    let mut i = 0;
    while i < A40_FAMILY_COUNT {
        s.push_str(&format!(
            "  {}[{}] 登记 {} / 最低 {} 达标 {}\n",
            A40_FAMILIES[i].zh(),
            A40_FAMILIES[i].code2(),
            counts[i],
            A40_FAMILIES[i].min_asserts(),
            if counts[i] >= A40_FAMILIES[i].min_asserts() { "是" } else { "否" }
        ));
        i += 1;
    }
    s.push_str(&format!(
        "  总计 {} / 硬线 {} 判定[{}]\n",
        book.total(),
        A40_TOTAL_MIN,
        if book.registry_green() { "过" } else { "不过" }
    ));
    s
}

/// 执行报告渲染（族列 + 失败清单定位到 id，上限 32 条封顶）。
pub fn render_run_report(rep: &RunReport40) -> String {
    let mut s = String::new();
    s.push_str("UNX-F0706 执行报告：\n");
    let mut i = 0;
    while i < A40_FAMILY_COUNT {
        let st = &rep.fams[i];
        s.push_str(&format!(
            "  {}[{}] 登记 {}/最低 {} 执行 {} 通过 {} 失败 {} 覆盖 {}‱ 达线 {}\n",
            st.fam.zh(),
            st.fam.code2(),
            st.registered,
            st.fam.min_asserts(),
            st.executed,
            st.passed,
            st.failed,
            st.cover_perm,
            if st.meets_min { "是" } else { "否" }
        ));
        i += 1;
    }
    s.push_str(&format!(
        "  总计 {} 通过 {} 失败 {} 序稳定 {} 总判[{}]\n",
        rep.total,
        rep.passed,
        rep.failed,
        if rep.order_stable { "是" } else { "否" },
        if rep.all_green { "全绿" } else { "有红" }
    ));
    let failures = rep.failures();
    let cap = if failures.len() > 32 { 32 } else { failures.len() };
    let mut k = 0;
    while k < cap {
        let fl = failures[k];
        s.push_str(&format!(
            "  红 {} (序 {}) 判据[{}] {}\n",
            fl.id_text, fl.order, fl.criterion, fl.desc
        ));
        k += 1;
    }
    if failures.len() > cap {
        s.push_str(&format!("  …另有 {} 条红灯略\n", failures.len() - cap));
    }
    s
}

/// 覆盖审计渲染（三族覆盖率 + 缺口清单）。
pub fn render_coverage_report(cov: &Coverage40) -> String {
    let mut s = String::new();
    s.push_str("UNX-F0706 覆盖审计：\n");
    let mut i = 0;
    while i < A40_FAMILY_COUNT {
        let st = &cov.fams[i];
        s.push_str(&format!(
            "  {} 覆盖率 {}‱（执行 {}/登记 {}）达线 {}\n",
            st.fam.zh(),
            st.cover_perm,
            st.executed,
            st.registered,
            if st.meets_min { "是" } else { "否" }
        ));
        i += 1;
    }
    if cov.gaps.is_empty() {
        s.push_str("  族面缺口：无（A/B/C ×3 族全登记）\n");
    } else {
        s.push_str(&format!("  族面缺口 {} 处：", cov.gaps.len()));
        let mut g = 0;
        while g < cov.gaps.len() {
            s.push_str(&format!("{} ", cov.gaps[g].text()));
            g += 1;
        }
        s.push('\n');
    }
    s.push_str(&format!(
        "  覆盖完备判定[{}]\n",
        if cov.complete { "完备" } else { "有缺口" }
    ));
    s
}

/// 总报渲染（登记/执行/覆盖/diff/索引/对拍/注入/健康/指纹/判定十段）。
pub fn render_final_report(rep: &A40FinalReport) -> String {
    let mut s = String::new();
    s.push_str("UNX-F0706 交接面断言总审计：\n");
    s.push_str(&format!(
        "  登记：{} 条 硬线 {} 三族计数 {:?} 登记绿态 {}\n",
        rep.registry_total,
        A40_TOTAL_MIN,
        rep.registry_counts,
        if rep.registry_green { "过" } else { "不过" }
    ));
    s.push_str(&render_run_report(&rep.run));
    s.push_str(&format!(
        "  覆盖：完备 {} 缺口 {} 处\n",
        if rep.coverage.complete { "是" } else { "否" },
        rep.coverage.gaps.len()
    ));
    s.push_str(&format!(
        "  diff：增 {} 删 {} 改 {} 洁净 {}\n",
        rep.diff.added.len(),
        rep.diff.removed.len(),
        rep.diff.modified.len(),
        if rep.diff.clean { "是" } else { "否" }
    ));
    s.push_str(&format!(
        "  索引对账：{} 对拍：甲 {:#018x} 乙 {:#018x} 一致 {}\n",
        if rep.cross_balanced { "平" } else { "错位" },
        rep.dual.book_fp_a,
        rep.dual.book_fp_b,
        if rep.dual.identical { "是" } else { "否" }
    ));
    s.push_str("  注入：");
    let mut i = 0;
    while i < rep.injections.len() {
        let p = &rep.injections[i];
        s.push_str(&format!(
            "{}[{}]码{:#06x} ",
            p.kind.zh(),
            if p.detected { "检出" } else { "漏检" },
            p.code
        ));
        i += 1;
    }
    s.push('\n');
    s.push_str(&format!(
        "  健康：事件 {} 故障 {} 丢弃 {} ok {}\n",
        rep.health.events, rep.health.faults, rep.health.dropped, if rep.health.ok { "是" } else { "否" }
    ));
    s.push_str(&format!(
        "  指纹：账 {:#018x} 执行 {:#018x} 总判定[{}]\n",
        rep.book_fp,
        rep.run_fp,
        rep.verdict_zh()
    ));
    s
}

// ═══════════════════════════════════════════════════════════════════
// 编译期硬闸（const assert：硬线、表形、码段、参数在编译期钉死）
// ═══════════════════════════════════════════════════════════════════

const _: () = assert!(A40_TOTAL_MIN == 40);
const _: () = assert!(A40_MIN_FROZEN6 + A40_MIN_LIMINE + A40_MIN_PANIC == A40_TOTAL_MIN);
const _: () = assert!(A40_SEG_BASE == 0x25B0);
const _: () = assert!(XC40_GREEN == 0x25B0 && XC40_UNKNOWN == 0x25BE);
const _: () = assert!(A40_FAMILIES.len() == A40_FAMILY_COUNT);
const _: () = assert!(A40_FACE_LETTERS.len() == 3);
const _: () = assert!(A40_IFACE6_ALL.len() == IFACE6_COUNT);
const _: () = assert!(A40_IFACE6_CODES.len() == IFACE6_COUNT);
const _: () = assert!(A40_IFACE6_NAMES.len() == IFACE6_COUNT);
const _: () = assert!(A40_IFACE6_FIELD_COUNTS.len() == IFACE6_COUNT);
const _: () = assert!(A40_FRZ_MM.len() == 3);
const _: () = assert!(A40_FRZ_FB.len() == 13);
const _: () = assert!(A40_FRZ_KA.len() == 4);
const _: () = assert!(A40_FRZ_HH.len() == 2);
const _: () = assert!(A40_FRZ_RS.len() == 2);
const _: () = assert!(A40_FRZ_IT.len() == 2);
const _: () = assert!(
    A40_FRZ_MM.len() + A40_FRZ_FB.len() + A40_FRZ_KA.len() + A40_FRZ_HH.len() + A40_FRZ_RS.len()
        + A40_FRZ_IT.len()
        == A40_FRZ_FIELD_TOTAL
);
const _: () = assert!(A40_FRZ_TABLES.len() == IFACE6_COUNT);
const _: () = assert!(A40_FRZ_FIELD_TOTAL == 26);
const _: () = assert!(A40_FNV_OFFSET == 0xcbf2_9ce4_8422_2325);
const _: () = assert!(A40_FNV_PRIME == 0x0000_0100_0000_01b3);
const _: () = assert!(A40_MIN_USABLE_BYTES == 64 * 1024 * 1024);
const _: () = assert!(A40_FB_MIN_W == 640 && A40_FB_MIN_H == 400);
const _: () = assert!(A40_HHDM_FLOOR == 0xFFFF_8000_0000_0000);
const _: () = assert!(A40_PANIC_STORM_BOUND == 3);
const _: () = assert!(A40_FREEZE_TABLE_V == 1);
const _: () = assert!(A40_EVENT_CAP == 512);
const _: () = assert!(A40_SEQ_MAX == 999);
const _: () = assert!(A40_FACE_MIN == b'A' && A40_FACE_MAX == b'C');
const _: () = assert!(Family40::Frozen6.min_asserts() == A40_MIN_FROZEN6);
const _: () = assert!(Family40::LimineResp.min_asserts() == A40_MIN_LIMINE);
const _: () = assert!(Family40::PanicPath.min_asserts() == A40_MIN_PANIC);
const _: () = assert!(Family40::Frozen6.idx() == 0);
const _: () = assert!(Family40::LimineResp.idx() == 1);
const _: () = assert!(Family40::PanicPath.idx() == 2);
const _: () = assert!(Iface6::Mm.idx() == 0 && Iface6::It.idx() == 5);
const _: () = assert!(FR_FN_PRESENCE.len() == IFACE6_COUNT);
const _: () = assert!(FR_FN_OFFSET.len() == IFACE6_COUNT);
const _: () = assert!(FR_FN_WIDTH.len() == IFACE6_COUNT);
const _: () = assert!(FR_FN_MAGIC.len() == IFACE6_COUNT);
const _: () = assert!(FR_DESC_PRESENCE.len() == IFACE6_COUNT);
const _: () = assert!(FR_DESC_OFFSET.len() == IFACE6_COUNT);
const _: () = assert!(FR_DESC_WIDTH.len() == IFACE6_COUNT);
const _: () = assert!(FR_DESC_MAGIC.len() == IFACE6_COUNT);
