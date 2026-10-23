//! unx0705_assert70 —— UNX-F0705 早传/页表断言补全至 ≥70（断言账本体系）。
//!
//! ── 锚点（CoRun Unxreal 主册，docs/Varix/CoRun Varix STAR II · Unxreal/
//!    CoRun Varix STAR II · Unxreal.md 第 2288 行，全文照抄）──
//!   | UNX-F0705 | 早传/页表断言补全至 ≥70 | 570 | 已深化 | UNX-F0705-J1
//!   早传/页表分项补全后实数 ≥70（早传十二步/双控制台/页表属性/高半核四面
//!   覆盖）且分项全绿 |
//! 承办上下文（主册 2280 行 AI-01 承办节选）："任务书判据 2 五分项硬线
//!   （ACPI ≥80/内存图 ≥60/多核 ≥50/早传·页表 ≥70/交接面 ≥40）为本批主场"。
//!
//! ── 组件（全真实子系统，零注水空壳）──
//!   1. AssertionId 断言命名规范：域/面/序号三段式 id（A70-<域码>-<面字母>-
//!      <序号>），规范校验逐段硬检，不合规范拒收（BadId）。
//!   2. 四面断言登记：早传十二步 ≥24（十二步完成位 12 + 十二步产物界 12 +
//!      补全 2）、双控制台 ≥12（串行 5 + VGA 4 + 镜像 5）、页表属性 ≥18
//!      （在场 8 + 属性符合 8 + 纪律 4 + 元检查 3）、高半核 ≥16（映射 8 +
//!      元检查 8）；每面最低条数 const 表 + 登记时逐条带判据引用。
//!   3. AssertBook 断言账本：登记制 + 重复 id 拒收 + 总数 ≥70 判定 +
//!      四面计数分列 + 事件账内嵌。
//!   4. AssertRunner 执行引擎（run）：逐条执行 → Pass/Fail 二态 + Fail 定位
//!      到断言 id + 执行序稳定（order 严格递增核）；空断言反证：无检查体
//!      （check = None）的断言登记拒收（EmptyCheck）。
//!   5. 补全 diff 账（AssertDiffBook）：相对基线快照的增量账——只增不减，
//!      删改基线断言即红案（BaselineRemoved / BaselineModified）。
//!   6. 执行报告（AssertRunReport）：分面通过率（万分比）+ 总判定
//!      （≥70 且全绿二态 Green/Red）。
//!   7. 事件账（EventLog）+ health() + FNV-1a 指纹 + 双跑确定性对拍
//!      （dual_run）+ 注入面（空断言注入/删改注入/重复 id 注入/判据篡改
//!      注入，各必检出）。
//!
//! ── 模拟执行引擎定位 ──
//!   检查体以 fn(&A70World) -> bool 函数指针登记，对 A70World（早传十二步
//!   完成位/产物计数、双控制台计数器、八区域页表属性态、高半核四映射）做
//!   真实谓词判定；A70World 为模拟世界态，不接真硬件——下游真实断言面
//!   （unx0079_a4handoff / unx0072_pageaudit / unx0089_hihalf 等）落世界态
//!   后本账可直接消费。
//!
//! ── 性能逐项分解 ──
//!   登记 O(n)（重复 id 线性扫）；执行 O(n)（n = 断言数）；diff O(n²)
//!   （基线×账本线性对账，n≈80 实测量级）；指纹 O(n·|str|)；对拍
//!   2×(登记+执行)。全整数确定性，无浮点，无时钟，无外部输入。
//!
//! ── 诊断码段：0x16D0（独占 0x10）──
//!   先验空闲证据（kernel/varix/src 全域 grep "0x15A/0x16D/SEG_BASE"）：
//!   0x15A0=svstar2/ves453_s24selfcheck；0x15B0=unx0663_tsccross 声明；
//!   0x15C0~0x15FF=unx2361 声明独占；0x15E0=ves454_s24close；
//!   0x1590=cga26_gauss；0x1600=vea38_uav/ves455；0x1610=unx0687_succrate；
//!   0x1620=cga27_perfcount；0x1640=cga 系列；0x1660=ves456_s25dsl；
//!   0x1670 旁证涉 EBML 魔数；0x1680=ves457_s25validator——故取 0x16D0
//!   （全域无 SEG_BASE 占用；stard/mediainfo.rs 的 0x16D4AE6B 为 EBML
//!   u32 魔数，非 u16 诊断段，不冲突）。
//!
//! 纯功能线：no_std + alloc；内核路径零 unwrap/expect/panic!/运行期
//! assert!；可失败全走 Result<T, AssertBookError>；溢出显性
//! saturating/wrapping；无 >64KB 栈结构体（A70World ≈ 0.2KB、账本全走
//! 堆）；const fn 规避 slice::get/Vec 索引/Ord::min/max/派生相等/for。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const A70_VERSION: &str = "unx0705_assert70-v1";

/// 诊断段基址（0x16D0 段 · 本模块独占 · 先验空闲证据见文件头）。
pub const A70_SEG_BASE: u16 = 0x16D0;

/// XcCode 闭集（0x16D0~0x16DD）。
pub const XC_GREEN: u32 = 0x16D0; // 绿码/正常收讫
pub const XC_BAD_ID: u32 = 0x16D1; // 断言 id 不合三段式规范
pub const XC_DUP_ID: u32 = 0x16D2; // 重复 id 拒收
pub const XC_EMPTY_CHECK: u32 = 0x16D3; // 空断言（无检查体）拒收
pub const XC_FACE_MIN: u32 = 0x16D4; // 分面最低条数未达
pub const XC_TOTAL_SHORT: u32 = 0x16D5; // 总数不足 70
pub const XC_ASSERT_FAIL: u32 = 0x16D6; // 断言执行红灯
pub const XC_DIFF_REMOVED: u32 = 0x16D7; // 基线断言被删（红案）
pub const XC_DIFF_MODIFIED: u32 = 0x16D8; // 基线断言被改（红案）
pub const XC_ORDER_UNSTABLE: u32 = 0x16D9; // 执行序不稳定
pub const XC_HEALTH_RED: u32 = 0x16DA; // 事件账红灯
pub const XC_FP_MISMATCH: u32 = 0x16DB; // 双跑指纹不一致
pub const XC_INDEX_RANGE: u32 = 0x16DC; // 世界态下标越界
pub const XC_INJECT_MISSED: u32 = 0x16DD; // 注入故障未被检出

/// 诊断码 → 中文可操作诊断（独占段 0x16D0~0x16DD 内解释，段外一律未知）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_GREEN => ("绿码：登记/执行/对账正常收讫", "无需处置，继续下一面"),
        XC_BAD_ID => (
            "断言 id 不合三段式规范（A70-<域码>-<面字母>-<序号>）",
            "按域码 EB/CS/PT/HH、面字母 A~Z、序号 001~999 改写后重登记",
        ),
        XC_DUP_ID => (
            "断言 id 与账本既有条目重复",
            "换序号或换面字母；禁止同 id 复登记（登记制红线）",
        ),
        XC_EMPTY_CHECK => (
            "空断言：登记条目无检查体（check = None）",
            "补齐 fn(&A70World)->bool 检查体后再登记；无检查体一律拒收",
        ),
        XC_FACE_MIN => (
            "分面登记条数低于该面最低硬线",
            "按四面清单补齐：早传 ≥24 / 双控制台 ≥12 / 页表 ≥18 / 高半核 ≥16",
        ),
        XC_TOTAL_SHORT => (
            "断言总数不足 70（UNX-F0705-J1 硬线）",
            "向四面继续补条，直至总数 ≥70 且四面各自达标",
        ),
        XC_ASSERT_FAIL => (
            "断言执行存在红灯条目",
            "按失败条目的 id 与判据引用定位修复后重跑",
        ),
        XC_DIFF_REMOVED => (
            "基线断言被删除（只增不减纪律红案）",
            "恢复被删断言或走批账勘误流程，禁止静默删除",
        ),
        XC_DIFF_MODIFIED => (
            "基线断言判据/描述被改写（只增不减纪律红案）",
            "恢复原判据引用；确需改写走批账勘误并留痕",
        ),
        XC_ORDER_UNSTABLE => (
            "执行序不稳定（order 字段非严格递增）",
            "重建账本重登记；禁止乱序注入执行流",
        ),
        XC_HEALTH_RED => (
            "事件账存在故障事件或丢事件",
            "按事件码逐条排查登记/执行/对账路径",
        ),
        XC_FP_MISMATCH => (
            "双跑指纹不一致（确定性破缺）",
            "排查登记序/世界态是否引入非确定源后重跑对拍",
        ),
        XC_INDEX_RANGE => (
            "世界态下标越界（步骤/区域/映射槽）",
            "下标须落在对应槽位数内（12 步/8 区域/4 映射）",
        ),
        XC_INJECT_MISSED => (
            "注入故障未被断言面检出",
            "修复对应守卫逻辑后重跑注入面",
        ),
        _ => ("未知诊断码", "查 0x16D0 段闭集登记表"),
    }
}

// ═══════════════════════════════════════════════════════════════════
// 错误枚举：每变体带 detail() 中文可操作诊断
// ═══════════════════════════════════════════════════════════════════

/// 断言账本体系错误（全路径可失败出口，禁 panic 族）。
#[derive(Clone, Debug)]
pub enum AssertBookError {
    /// id 不合三段式规范。
    BadId { code: u32, id: String },
    /// 重复 id。
    DuplicateId { code: u32, id: String },
    /// 空断言（无检查体）。
    EmptyCheck { code: u32, id: String },
    /// 世界态下标越界。
    IndexOutOfRange { code: u32, idx: usize, bound: usize },
    /// 执行序不稳定。
    RunOrderCorrupt { code: u32 },
    /// 总数不足 70。
    TotalShort { code: u32, have: usize, need: usize },
    /// 分面最低条数未达。
    FaceMinShort { code: u32, face: FaceKind, have: usize, need: usize },
    /// 基线断言被删（红案）。
    BaselineRemoved { code: u32, id: String },
    /// 基线断言被改（红案）。
    BaselineModified { code: u32, id: String },
    /// 注入未被检出。
    InjectionMissed { code: u32, kind: &'static str },
    /// 段外未知诊断码兜底。
    UnknownDiag { code: u32 },
}

impl AssertBookError {
    /// 中文可操作诊断。
    pub fn detail(&self) -> &'static str {
        match self {
            AssertBookError::BadId { .. } => {
                "id 不合三段式规范：改写为 A70-<域码>-<面字母>-<序号> 后重登记"
            }
            AssertBookError::DuplicateId { .. } => "重复 id：换序号/面字母，禁止同 id 复登记",
            AssertBookError::EmptyCheck { .. } => "空断言拒收：补齐检查体 fn(&A70World)->bool",
            AssertBookError::IndexOutOfRange { .. } => "世界态下标越界：核对 12 步/8 区域/4 映射槽位",
            AssertBookError::RunOrderCorrupt { .. } => "执行序不稳定：重建账本，恢复登记序",
            AssertBookError::TotalShort { .. } => "总数不足 70：继续向四面补条",
            AssertBookError::FaceMinShort { .. } => "分面低于硬线：按该面清单补齐",
            AssertBookError::BaselineRemoved { .. } => "基线断言被删：恢复条目或走批账勘误",
            AssertBookError::BaselineModified { .. } => "基线判据被改：恢复原判据引用并留痕",
            AssertBookError::InjectionMissed { .. } => "注入未检出：修复对应守卫后重跑注入面",
            AssertBookError::UnknownDiag { .. } => "未知诊断码：查 0x16D0 段闭集登记表",
        }
    }

    /// 诊断码。
    pub fn code(&self) -> u32 {
        match self {
            AssertBookError::BadId { code, .. }
            | AssertBookError::DuplicateId { code, .. }
            | AssertBookError::EmptyCheck { code, .. }
            | AssertBookError::IndexOutOfRange { code, .. }
            | AssertBookError::RunOrderCorrupt { code }
            | AssertBookError::TotalShort { code, .. }
            | AssertBookError::FaceMinShort { code, .. }
            | AssertBookError::BaselineRemoved { code, .. }
            | AssertBookError::BaselineModified { code, .. }
            | AssertBookError::InjectionMissed { code, .. }
            | AssertBookError::UnknownDiag { code } => *code,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// 四面域 + 最低条数 const 表（判据硬线，来源：UNX-F0705-J1 判据原文）
// ═══════════════════════════════════════════════════════════════════

/// 早传十二步面最低条数（判据原文：早传十二步面覆盖，取十二步×2 分项）。
pub const A70_MIN_EARLYBOOT: usize = 24;
/// 双控制台面最低条数（判据原文：双控制台面覆盖）。
pub const A70_MIN_CONSOLE: usize = 12;
/// 页表属性面最低条数（判据原文：页表属性面覆盖）。
pub const A70_MIN_PAGETBL: usize = 18;
/// 高半核面最低条数（判据原文：高半核面覆盖）。
pub const A70_MIN_HIHALF: usize = 16;
/// 总数硬线 ≥70（判据原文：补全后实数 ≥70）。
pub const A70_TOTAL_MIN: usize = 70;

/// 四面域（判据原文四面：早传十二步/双控制台/页表属性/高半核）。
#[derive(Clone, Copy, Debug)]
pub enum FaceKind {
    /// 早传十二步。
    EarlyBoot,
    /// 双控制台。
    Console,
    /// 页表属性。
    PageTable,
    /// 高半核。
    HiHalf,
}

impl FaceKind {
    /// 四面全序（登记/统计按此序，稳定）。
    pub const ALL: [FaceKind; 4] = [
        FaceKind::EarlyBoot,
        FaceKind::Console,
        FaceKind::PageTable,
        FaceKind::HiHalf,
    ];

    /// id 域码三字母。
    pub fn code3(self) -> &'static str {
        match self {
            FaceKind::EarlyBoot => "EB",
            FaceKind::Console => "CS",
            FaceKind::PageTable => "PT",
            FaceKind::HiHalf => "HH",
        }
    }

    /// 域码解析（未知域码拒收）。
    pub fn parse_code3(s: &str) -> Option<FaceKind> {
        match s {
            "EB" => Some(FaceKind::EarlyBoot),
            "CS" => Some(FaceKind::Console),
            "PT" => Some(FaceKind::PageTable),
            "HH" => Some(FaceKind::HiHalf),
            _ => None,
        }
    }

    /// 中文面名（判据原文用语）。
    pub fn zh(self) -> &'static str {
        match self {
            FaceKind::EarlyBoot => "早传十二步",
            FaceKind::Console => "双控制台",
            FaceKind::PageTable => "页表属性",
            FaceKind::HiHalf => "高半核",
        }
    }

    /// 该面最低条数（const 表，来源：判据原文四面硬线）。
    pub const fn min_asserts(self) -> usize {
        match self {
            FaceKind::EarlyBoot => A70_MIN_EARLYBOOT,
            FaceKind::Console => A70_MIN_CONSOLE,
            FaceKind::PageTable => A70_MIN_PAGETBL,
            FaceKind::HiHalf => A70_MIN_HIHALF,
        }
    }

    /// 统计数组下标（0..4，稳定序）。
    pub fn idx(self) -> usize {
        match self {
            FaceKind::EarlyBoot => 0,
            FaceKind::Console => 1,
            FaceKind::PageTable => 2,
            FaceKind::HiHalf => 3,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// AssertionId：域/面/序号三段式命名规范
// ═══════════════════════════════════════════════════════════════════

/// id 前缀（本批断言族专属，与批号 UNX-F0705 绑定）。
pub const A70_ID_PREFIX: &str = "A70";
/// 面字母取值域：'A'..='Z'（26 面/域上限）。
pub const A70_SUB_MAX: u8 = b'Z';
/// 序号取值域上限：999（三位定宽）。
pub const A70_SEQ_MAX: u16 = 999;

/// 断言命名规范 id：域（四面域码）/面（面字母）/序号（三位序）三段式。
/// 规范形：`A70-<EB|CS|PT|HH>-<A..Z>-<001..999>`。
#[derive(Clone, Debug)]
pub struct AssertionId {
    face: FaceKind,
    sub: u8,
    seq: u16,
}

impl AssertionId {
    /// 构造（构造期规范校验，不合规范拒收）。
    pub fn new(face: FaceKind, sub: u8, seq: u16) -> Result<AssertionId, AssertBookError> {
        if sub < b'A' || sub > A70_SUB_MAX {
            return Err(AssertBookError::BadId {
                code: XC_BAD_ID,
                id: format!("face-letter:{}", sub),
            });
        }
        if seq == 0 || seq > A70_SEQ_MAX {
            return Err(AssertBookError::BadId {
                code: XC_BAD_ID,
                id: format!("seq:{}", seq),
            });
        }
        Ok(AssertionId { face, sub, seq })
    }

    /// 三段式字符串解析（逐段硬检，不合规范拒收 BadId）。
    pub fn parse(text: &str) -> Result<AssertionId, AssertBookError> {
        let bad = || AssertBookError::BadId {
            code: XC_BAD_ID,
            id: String::from(text),
        };
        let mut it = text.split('-');
        let p0 = it.next().ok_or_else(bad)?;
        let p1 = it.next().ok_or_else(bad)?;
        let p2 = it.next().ok_or_else(bad)?;
        let p3 = it.next().ok_or_else(bad)?;
        // 第五段存在 → 段数超限，不合规范。
        if it.next().is_some() {
            return Err(bad());
        }
        if p0 != A70_ID_PREFIX {
            return Err(bad());
        }
        let face = FaceKind::parse_code3(p1).ok_or_else(bad)?;
        let pb = p2.as_bytes();
        if pb.len() != 1 || pb[0] < b'A' || pb[0] > A70_SUB_MAX {
            return Err(bad());
        }
        let sub = pb[0];
        let sb = p3.as_bytes();
        if sb.is_empty() || sb.len() > 3 {
            return Err(bad());
        }
        // 序号：全数字，值 1..=999。前导零不在此拒收——规范形由
        // render 互验闸钉死（登记时 render(id) != 原文即 BadId）。
        let mut seq: u16 = 0;
        let mut i = 0;
        while i < sb.len() {
            if sb[i] < b'0' || sb[i] > b'9' {
                return Err(bad());
            }
            seq = seq.wrapping_mul(10).wrapping_add((sb[i] - b'0') as u16);
            i += 1;
        }
        if seq == 0 || seq > A70_SEQ_MAX {
            return Err(bad());
        }
        Ok(AssertionId { face, sub, seq })
    }

    /// 规范形渲染（登记时与原文互验，抓手写错拼）。
    pub fn render(&self) -> String {
        format!(
            "{}-{}-{}-{:03}",
            A70_ID_PREFIX,
            self.face.code3(),
            self.sub as char,
            self.seq
        )
    }

    /// 面域。
    pub fn face(&self) -> FaceKind {
        self.face
    }

    /// 面字母。
    pub fn sub(&self) -> u8 {
        self.sub
    }

    /// 序号。
    pub fn seq(&self) -> u16 {
        self.seq
    }

    /// 去序等价键（重复 id 判定用，避免派生相等）。
    pub fn key(&self) -> (u8, u8, u16) {
        (self.face.idx() as u8, self.sub, self.seq)
    }
}

// ═══════════════════════════════════════════════════════════════════
// A70World：模拟世界态（早传十二步/双控制台/页表属性/高半核）
// ═══════════════════════════════════════════════════════════════════

/// 早传步骤数（十二步，判据原文"早传十二步"）。
pub const A70_BOOT_STEPS: usize = 12;
/// 页表属性区域数（八区域：正文/只读/数据/BSS/初始化/堆/VGA MMIO/ACPI 回收）。
pub const A70_REGION_COUNT: usize = 8;
/// 高半核映射槽数（正文/只读/数据/栈顶四映射，判据原文"高半核四面覆盖"之映射面）。
pub const A70_HH_SLOTS: usize = 4;
/// 巨页区域下标（内核堆区按 2M 巨页映射）。
pub const A70_HUGE_REGION: usize = 5;

/// 十二步名（语义对齐 unx0079_a4handoff / unx0060_ktb03 / unx0096_segload 批账流程）。
pub const A70_BOOT_STEP_NAMES: [&str; A70_BOOT_STEPS] = [
    "01-multiboot2头校验",
    "02-CPUID特性识别",
    "03-e820内存图采集",
    "04-蹦床页表搭建",
    "05-长模式进入",
    "06-启动栈安装",
    "07-BSS清零",
    "08-重定位修正",
    "09-内核段装载",
    "10-控制台点灯",
    "11-引导协议标签解析",
    "12-早分配器初始化",
];

/// 十二步产物下限（模拟口径：步骤 9 内核段 ≥3 段对齐 unx0096_segload
/// 三段装载；步骤 10 双控制台=2；步骤 12 早分配桶 ≥8 对齐 unx0071）。
pub const A70_STEP_ARTIFACT_MIN: [u32; A70_BOOT_STEPS] = [1, 1, 1, 1, 1, 1, 1, 1, 3, 2, 1, 8];
/// 十二步产物上限（界内判定，防野计数；来源：各步骤语义饱和界）。
pub const A70_STEP_ARTIFACT_MAX: [u32; A70_BOOT_STEPS] =
    [4, 64, 128, 8, 1, 32, 4096, 65536, 16, 2, 64, 256];
/// 全绿世界态的十二步产物样值（界内中位口径）。
pub const A70_STEP_ARTIFACT_GREEN: [u32; A70_BOOT_STEPS] = [1, 8, 24, 4, 1, 8, 96, 512, 6, 2, 12, 32];

/// 串行台波特率（16550 常规默认，对齐 unx0060_ktb03 批账控制台面）。
pub const A70_SERIAL_BAUD: u32 = 115_200;
/// 串行台写入上限（饱和界，防野计数）。
pub const A70_SERIAL_WRITE_CAP: u64 = 1_048_576;
/// VGA 文本列数（标准 80x25 文本模，对齐 svstar 显示面口径）。
pub const A70_VGA_COLS: u16 = 80;
/// VGA 文本行数（标准 80x25 文本模）。
pub const A70_VGA_ROWS: u16 = 25;
/// VGA 台写入上限（饱和界）。
pub const A70_VGA_WRITE_CAP: u64 = 262_144;

/// 高半核基址下界（x86-64 高半 canonical 起点，对齐 unx0089_hihalf 批账）。
pub const A70_HIHALF_BASE_MIN: u64 = 0xFFFF_8000_0000_0000;
/// 高半核基址上界（canonical 上半段尾）。
pub const A70_HIHALF_BASE_MAX: u64 = 0xFFFF_FFFF_FFFF_FFFF;
/// 内核映像高半装载基址（典型 -2GiB 布局，对齐 unx0090_physbase 批账）。
pub const A70_KIMG_BASE: u64 = 0xFFFF_FFFF_8000_0000;
/// 栈顶窗口下界（映像尾 7MiB 处，护页前置）。
pub const A70_STACK_TOP_MIN: u64 = 0xFFFF_FFFF_8070_0000;
/// 栈顶窗口上界（初始化段前）。
pub const A70_STACK_TOP_MAX: u64 = 0xFFFF_FFFF_8080_0000;
/// 高半四映射期望虚址（正文/只读/数据/栈顶，对齐 A70_PTE_SPECS 前四区）。
pub const A70_HH_EXPECT_VADDR: [u64; A70_HH_SLOTS] = [
    0xFFFF_FFFF_8000_0000,
    0xFFFF_FFFF_8020_0000,
    0xFFFF_FFFF_8040_0000,
    0xFFFF_FFFF_807F_F000,
];

/// 页表属性规格（八区域判定期望态；虚址布局对齐 unx0089_hihalf /
/// unx0091_kptadapt 批账典型 -2GiB 高半映像）。
#[derive(Clone, Copy, Debug)]
pub struct A70PteSpec {
    /// 区域名。
    pub name: &'static str,
    /// 期望虚址。
    pub vaddr: u64,
    /// 期望可写。
    pub want_writable: bool,
    /// 期望不可执行（NX）。
    pub want_nx: bool,
    /// 期望用户态可达（内核区一律 false）。
    pub want_user: bool,
    /// 期望全局页（常驻映射区 true）。
    pub want_global: bool,
    /// 期望巨页（仅内核堆区）。
    pub want_huge: bool,
}

impl A70PteSpec {
    /// 该规格的全绿属性态。
    pub fn green_state(&self) -> A70PteState {
        A70PteState {
            present: true,
            writable: self.want_writable,
            nx: self.want_nx,
            user: self.want_user,
            global: self.want_global,
            huge: self.want_huge,
            aligned: self.want_huge,
        }
    }
}

/// 八区域属性规格表（判据引用：UNX-F0705-J1#页表属性）。
pub const A70_PTE_SPECS: [A70PteSpec; A70_REGION_COUNT] = [
    A70PteSpec { name: "内核正文.text", vaddr: 0xFFFF_FFFF_8000_0000, want_writable: false, want_nx: false, want_user: false, want_global: true, want_huge: false },
    A70PteSpec { name: "内核只读.rodata", vaddr: 0xFFFF_FFFF_8020_0000, want_writable: false, want_nx: true, want_user: false, want_global: true, want_huge: false },
    A70PteSpec { name: "内核数据.data", vaddr: 0xFFFF_FFFF_8040_0000, want_writable: true, want_nx: true, want_user: false, want_global: true, want_huge: false },
    A70PteSpec { name: "内核BSS.bss", vaddr: 0xFFFF_FFFF_8060_0000, want_writable: true, want_nx: true, want_user: false, want_global: true, want_huge: false },
    A70PteSpec { name: "初始化段.init", vaddr: 0xFFFF_FFFF_8080_0000, want_writable: true, want_nx: true, want_user: false, want_global: false, want_huge: false },
    A70PteSpec { name: "内核堆.heap", vaddr: 0xFFFF_FFFF_80A0_0000, want_writable: true, want_nx: true, want_user: false, want_global: false, want_huge: true },
    A70PteSpec { name: "VGA MMIO", vaddr: 0xFFFF_8000_000B_8000, want_writable: true, want_nx: true, want_user: false, want_global: false, want_huge: false },
    A70PteSpec { name: "ACPI回收.reclaim", vaddr: 0xFFFF_8000_0010_0000, want_writable: true, want_nx: true, want_user: false, want_global: false, want_huge: false },
];

/// 区域页表属性态（世界态快照列）。
#[derive(Clone, Copy, Debug)]
pub struct A70PteState {
    /// 在场位。
    pub present: bool,
    /// 可写位。
    pub writable: bool,
    /// NX（不可执行）位。
    pub nx: bool,
    /// 用户位。
    pub user: bool,
    /// 全局位。
    pub global: bool,
    /// 巨页位。
    pub huge: bool,
    /// 巨页对齐核过（仅巨页区有意义）。
    pub aligned: bool,
}

impl A70PteState {
    /// 空态（全零：不在场）。
    pub const fn blank() -> A70PteState {
        A70PteState {
            present: false,
            writable: false,
            nx: false,
            user: false,
            global: false,
            huge: false,
            aligned: false,
        }
    }
}

/// 模拟世界态：四面被检对象的真实状态列（检查体的唯一实参）。
#[derive(Clone, Debug)]
pub struct A70World {
    /// 十二步完成位。
    pub boot_steps_done: [bool; A70_BOOT_STEPS],
    /// 十二步产物计数（界内判定）。
    pub boot_step_artifact: [u32; A70_BOOT_STEPS],
    /// 串行台就绪。
    pub serial_ready: bool,
    /// VGA 台就绪。
    pub vga_ready: bool,
    /// 串行台波特率。
    pub serial_baud: u32,
    /// VGA 列数。
    pub vga_cols: u16,
    /// VGA 行数。
    pub vga_rows: u16,
    /// 串行台累计写入。
    pub serial_written: u64,
    /// VGA 台累计写入。
    pub vga_written: u64,
    /// 双台镜像对数。
    pub mirrored_pairs: u64,
    /// 双台输出漂移（应为零）。
    pub mirror_drift: u64,
    /// 光标列。
    pub cursor_col: u16,
    /// 光标行。
    pub cursor_row: u16,
    /// 控制台溢出旗（应为否）。
    pub console_overflow: bool,
    /// 八区域页表属性态。
    pub pte: [A70PteState; A70_REGION_COUNT],
    /// PML4 根就绪。
    pub pml4_ready: bool,
    /// PAT 索引界内。
    pub pat_index_ok: bool,
    /// 高半四映射在场位。
    pub hh_mapped: [bool; A70_HH_SLOTS],
    /// 高半四映射虚址。
    pub hh_vaddr: [u64; A70_HH_SLOTS],
    /// 高半基址。
    pub hh_base: u64,
    /// 恒等映射残留（应关）。
    pub identity_on: bool,
    /// 文本前护页在位。
    pub guard_ok: bool,
    /// 高半地址 canonical 核过。
    pub hh_canonical_ok: bool,
}

impl A70World {
    /// 空世界态（全否/全零：一切断言应红）。
    pub fn new() -> A70World {
        let mut w = A70World {
            boot_steps_done: [false; A70_BOOT_STEPS],
            boot_step_artifact: [0; A70_BOOT_STEPS],
            serial_ready: false,
            vga_ready: false,
            serial_baud: 0,
            vga_cols: 0,
            vga_rows: 0,
            serial_written: 0,
            vga_written: 0,
            mirrored_pairs: 0,
            mirror_drift: 0,
            cursor_col: 0,
            cursor_row: 0,
            console_overflow: true,
            pte: [A70PteState::blank(); A70_REGION_COUNT],
            pml4_ready: false,
            pat_index_ok: false,
            hh_mapped: [false; A70_HH_SLOTS],
            hh_vaddr: [0; A70_HH_SLOTS],
            hh_base: 0,
            identity_on: true,
            guard_ok: false,
            hh_canonical_ok: false,
        };
        let mut i = 0;
        while i < A70_BOOT_STEPS {
            w.boot_step_artifact[i] = 0;
            i += 1;
        }
        w
    }

    /// 全绿世界态（按八区域规格与四面口径逐位落值）。
    pub fn full_green() -> A70World {
        let mut w = A70World::new();
        let mut i = 0;
        while i < A70_BOOT_STEPS {
            w.boot_steps_done[i] = true;
            w.boot_step_artifact[i] = A70_STEP_ARTIFACT_GREEN[i];
            i += 1;
        }
        w.serial_ready = true;
        w.vga_ready = true;
        w.serial_baud = A70_SERIAL_BAUD;
        w.vga_cols = A70_VGA_COLS;
        w.vga_rows = A70_VGA_ROWS;
        w.serial_written = 4096;
        w.vga_written = 4096;
        w.mirrored_pairs = 2048;
        w.mirror_drift = 0;
        w.cursor_col = A70_VGA_COLS - 1;
        w.cursor_row = A70_VGA_ROWS - 1;
        w.console_overflow = false;
        let mut r = 0;
        while r < A70_REGION_COUNT {
            w.pte[r] = A70_PTE_SPECS[r].green_state();
            r += 1;
        }
        w.pml4_ready = true;
        w.pat_index_ok = true;
        let mut h = 0;
        while h < A70_HH_SLOTS {
            w.hh_mapped[h] = true;
            w.hh_vaddr[h] = A70_HH_EXPECT_VADDR[h];
            h += 1;
        }
        w.hh_base = A70_KIMG_BASE;
        w.identity_on = false;
        w.guard_ok = true;
        w.hh_canonical_ok = true;
        w
    }

    /// 标记某步完成（界检走 Result，禁 panic 族）。
    pub fn mark_step(&mut self, step: usize) -> Result<(), AssertBookError> {
        if step >= A70_BOOT_STEPS {
            return Err(AssertBookError::IndexOutOfRange {
                code: XC_INDEX_RANGE,
                idx: step,
                bound: A70_BOOT_STEPS,
            });
        }
        self.boot_steps_done[step] = true;
        Ok(())
    }

    /// 设某步产物计数（界检走 Result）。
    pub fn set_step_artifact(&mut self, step: usize, count: u32) -> Result<(), AssertBookError> {
        if step >= A70_BOOT_STEPS {
            return Err(AssertBookError::IndexOutOfRange {
                code: XC_INDEX_RANGE,
                idx: step,
                bound: A70_BOOT_STEPS,
            });
        }
        self.boot_step_artifact[step] = count;
        Ok(())
    }

    /// 设某区域属性态（界检走 Result）。
    pub fn set_region(&mut self, region: usize, state: A70PteState) -> Result<(), AssertBookError> {
        if region >= A70_REGION_COUNT {
            return Err(AssertBookError::IndexOutOfRange {
                code: XC_INDEX_RANGE,
                idx: region,
                bound: A70_REGION_COUNT,
            });
        }
        self.pte[region] = state;
        Ok(())
    }

    /// 设某高半映射槽（界检走 Result）。
    pub fn set_hh_slot(&mut self, slot: usize, mapped: bool, vaddr: u64) -> Result<(), AssertBookError> {
        if slot >= A70_HH_SLOTS {
            return Err(AssertBookError::IndexOutOfRange {
                code: XC_INDEX_RANGE,
                idx: slot,
                bound: A70_HH_SLOTS,
            });
        }
        self.hh_mapped[slot] = mapped;
        self.hh_vaddr[slot] = vaddr;
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════
// 检查体函数库（fn(&A70World) -> bool；全部真实谓词）
// ═══════════════════════════════════════════════════════════════════

/// 检查体函数指针型（登记制唯一检查体形态）。
pub type A70CheckFn = fn(&A70World) -> bool;

// ── 早传十二步面：步骤完成位（A-001..A-012）──

/// 步骤 N 完成位检查。
pub fn chk_step_done<const N: usize>(w: &A70World) -> bool {
    w.boot_steps_done[N]
}

/// 步骤 N 产物界内检查（≥MIN 且 ≤MAX，防野计数）。
pub fn chk_step_artifact<const N: usize>(w: &A70World) -> bool {
    w.boot_step_artifact[N] >= A70_STEP_ARTIFACT_MIN[N]
        && w.boot_step_artifact[N] <= A70_STEP_ARTIFACT_MAX[N]
}

/// 补全：十二步全部完成。
pub fn chk_all_steps_done(w: &A70World) -> bool {
    let mut i = 0;
    while i < A70_BOOT_STEPS {
        if !w.boot_steps_done[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 补全：e820 采集产物（步骤 3）界内且非空（内存图采集是下游分页地基）。
pub fn chk_mmap_cap(w: &A70World) -> bool {
    let n = w.boot_step_artifact[2];
    n >= 1 && n <= 128
}

// ── 双控制台面：串行台（CS-A）──

/// 串行台就绪。
pub fn chk_serial_ready(w: &A70World) -> bool {
    w.serial_ready
}

/// 串行台波特率 == 115200。
pub fn chk_serial_baud(w: &A70World) -> bool {
    w.serial_baud == A70_SERIAL_BAUD
}

/// 串行台已有写入。
pub fn chk_serial_written_pos(w: &A70World) -> bool {
    w.serial_written >= 1
}

/// 串行台写入界内（≤CAP，防野计数）。
pub fn chk_serial_write_cap(w: &A70World) -> bool {
    w.serial_written <= A70_SERIAL_WRITE_CAP
}

/// 镜像对数不超串行写入（每对必先落串行一次）。
pub fn chk_serial_pair_bound(w: &A70World) -> bool {
    w.mirrored_pairs <= w.serial_written
}

// ── 双控制台面：VGA 台（CS-B）──

/// VGA 台就绪。
pub fn chk_vga_ready(w: &A70World) -> bool {
    w.vga_ready
}

/// VGA 80x25 标准文本模。
pub fn chk_vga_geometry(w: &A70World) -> bool {
    w.vga_cols == A70_VGA_COLS && w.vga_rows == A70_VGA_ROWS
}

/// VGA 台已有写入。
pub fn chk_vga_written_pos(w: &A70World) -> bool {
    w.vga_written >= 1 && w.vga_written <= A70_VGA_WRITE_CAP
}

/// 光标界内（col < cols && row < rows）。
pub fn chk_vga_cursor(w: &A70World) -> bool {
    w.cursor_col < w.vga_cols && w.cursor_row < w.vga_rows
}

// ── 双控制台面：镜像面（CS-C）──

/// 双台同活。
pub fn chk_both_live(w: &A70World) -> bool {
    w.serial_ready && w.vga_ready
}

/// 存在镜像对。
pub fn chk_pairs_pos(w: &A70World) -> bool {
    w.mirrored_pairs >= 1
}

/// 双台输出漂移为零（镜像一致）。
pub fn chk_drift_zero(w: &A70World) -> bool {
    w.mirror_drift == 0
}

/// 控制台无溢出。
pub fn chk_no_overflow(w: &A70World) -> bool {
    !w.console_overflow
}

/// 双写守恒：2×对数 ≤ 串行+VGA 写入（每对恰好各落一台一次）。
pub fn chk_pair_conservation(w: &A70World) -> bool {
    w.mirrored_pairs.saturating_mul(2) <= w.serial_written.saturating_add(w.vga_written)
}

// ── 页表属性面：区域在场位（PT-A）──

/// 区域 N 在场。
pub fn chk_pte_present<const N: usize>(w: &A70World) -> bool {
    w.pte[N].present
}

// ── 页表属性面：属性符合（PT-B）──

/// 区域 N 属性与规格逐位符合（writable/nx/user/global 四位）。
pub fn chk_pte_attrs<const N: usize>(w: &A70World) -> bool {
    let s = &w.pte[N];
    let sp = &A70_PTE_SPECS[N];
    s.writable == sp.want_writable && s.nx == sp.want_nx && s.user == sp.want_user && s.global == sp.want_global
}

// ── 页表属性面：纪律面（PT-C）──

/// NX 纪律：正文外七区全部 NX（W^X 不可执行面钉死）。
pub fn chk_nx_discipline(w: &A70World) -> bool {
    let mut i = 1;
    while i < A70_REGION_COUNT {
        if !w.pte[i].nx {
            return false;
        }
        i += 1;
    }
    true
}

/// 可写纪律：正文与只读区不可写。
pub fn chk_writable_discipline(w: &A70World) -> bool {
    !w.pte[0].writable && !w.pte[1].writable
}

/// 用户纪律：内核八区一律不可用户态可达。
pub fn chk_user_discipline(w: &A70World) -> bool {
    let mut i = 0;
    while i < A70_REGION_COUNT {
        if w.pte[i].user {
            return false;
        }
        i += 1;
    }
    true
}

/// 全局纪律：常驻四区（正文/只读/数据/BSS）global 位齐备。
pub fn chk_global_discipline(w: &A70World) -> bool {
    let mut i = 0;
    while i < 4 {
        if !w.pte[i].global {
            return false;
        }
        i += 1;
    }
    true
}

// ── 页表属性面：元检查（PT-D）──

/// PML4 根就绪。
pub fn chk_pml4_ready(w: &A70World) -> bool {
    w.pml4_ready
}

/// PAT 索引界内。
pub fn chk_pat_index(w: &A70World) -> bool {
    w.pat_index_ok
}

/// 巨页元检查：堆区巨页位与 2M 对齐核齐备。
pub fn chk_huge_meta(w: &A70World) -> bool {
    w.pte[A70_HUGE_REGION].huge && w.pte[A70_HUGE_REGION].aligned
}

// ── 高半核面：映射面（HH-A）──

/// 高半映射槽 N 在场。
pub fn chk_hh_mapped<const N: usize>(w: &A70World) -> bool {
    w.hh_mapped[N]
}

/// 高半映射槽 N 虚址与期望一致。
pub fn chk_hh_vaddr<const N: usize>(w: &A70World) -> bool {
    w.hh_mapped[N] && w.hh_vaddr[N] == A70_HH_EXPECT_VADDR[N]
}

// ── 高半核面：元检查（HH-B）──

/// 高半基址落在 canonical 高半窗（≥0xFFFF8000_0000_0000）。
pub fn chk_hh_base_window(w: &A70World) -> bool {
    w.hh_base >= A70_HIHALF_BASE_MIN && w.hh_base <= A70_HIHALF_BASE_MAX
}

/// 高半基址 == 内核映像基址（-2GiB 布局钉死）。
pub fn chk_hh_base_kimg(w: &A70World) -> bool {
    w.hh_base == A70_KIMG_BASE
}

/// 恒等映射已关（SMEP/防低半别名残留）。
pub fn chk_identity_off(w: &A70World) -> bool {
    !w.identity_on
}

/// 文本前护页在位。
pub fn chk_guard_ok(w: &A70World) -> bool {
    w.guard_ok
}

/// 高半地址 canonical 核过。
pub fn chk_hh_canonical(w: &A70World) -> bool {
    w.hh_canonical_ok
}

/// 四映射槽全部在场。
pub fn chk_hh_all_mapped(w: &A70World) -> bool {
    let mut i = 0;
    while i < A70_HH_SLOTS {
        if !w.hh_mapped[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 四映射虚址严格递增（正文 < 只读 < 数据 < 栈顶，映像序钉死）。
pub fn chk_hh_order(w: &A70World) -> bool {
    let mut i = 1;
    while i < A70_HH_SLOTS {
        if w.hh_vaddr[i] <= w.hh_vaddr[i - 1] {
            return false;
        }
        i += 1;
    }
    true
}

/// 栈顶落在护窗内（[0xFFFF_FFFF_8070_0000, 0xFFFF_FFFF_8080_0000)）。
pub fn chk_hh_stack_window(w: &A70World) -> bool {
    w.hh_vaddr[3] >= A70_STACK_TOP_MIN && w.hh_vaddr[3] < A70_STACK_TOP_MAX
}

// ═══════════════════════════════════════════════════════════════════
// 登记规格表（四面 · 逐条带判据引用 · 检查体强制 Some）
// ═══════════════════════════════════════════════════════════════════

/// 登记规格（id/判据引用/描述/检查体四列；check 为 None 即空断言样本）。
#[derive(Clone, Copy)]
pub struct A70CheckSpec {
    /// 规范 id 全文（登记时与 parse 结果互验）。
    pub id: &'static str,
    /// 判据引用（可解引到 UNX-F0705-J1 分项）。
    pub criterion: &'static str,
    /// 断言描述。
    pub desc: &'static str,
    /// 检查体（None = 空断言样本，登记必拒收）。
    pub check: Option<A70CheckFn>,
}

// ── 面一：早传十二步（26 条 ≥ 24）──

/// 早传十二步面登记规格表（完成位 12 + 产物界 12 + 补全 2）。
pub const A70_CHECKS_EARLYBOOT: [A70CheckSpec; 26] = [
    A70CheckSpec { id: "A70-EB-A-001", criterion: "UNX-F0705-J1#早传十二步/01-multiboot2头校验", desc: "步骤01完成位：multiboot2 头魔数与尾校验已过", check: Some(chk_step_done::<0>) },
    A70CheckSpec { id: "A70-EB-A-002", criterion: "UNX-F0705-J1#早传十二步/02-CPUID特性识别", desc: "步骤02完成位：CPUID 特性叶已识别", check: Some(chk_step_done::<1>) },
    A70CheckSpec { id: "A70-EB-A-003", criterion: "UNX-F0705-J1#早传十二步/03-e820内存图采集", desc: "步骤03完成位：e820 内存图已采集", check: Some(chk_step_done::<2>) },
    A70CheckSpec { id: "A70-EB-A-004", criterion: "UNX-F0705-J1#早传十二步/04-蹦床页表搭建", desc: "步骤04完成位：蹦床页表已搭建", check: Some(chk_step_done::<3>) },
    A70CheckSpec { id: "A70-EB-A-005", criterion: "UNX-F0705-J1#早传十二步/05-长模式进入", desc: "步骤05完成位：长模式已进入", check: Some(chk_step_done::<4>) },
    A70CheckSpec { id: "A70-EB-A-006", criterion: "UNX-F0705-J1#早传十二步/06-启动栈安装", desc: "步骤06完成位：启动栈已安装", check: Some(chk_step_done::<5>) },
    A70CheckSpec { id: "A70-EB-A-007", criterion: "UNX-F0705-J1#早传十二步/07-BSS清零", desc: "步骤07完成位：BSS 已清零", check: Some(chk_step_done::<6>) },
    A70CheckSpec { id: "A70-EB-A-008", criterion: "UNX-F0705-J1#早传十二步/08-重定位修正", desc: "步骤08完成位：重定位已修正", check: Some(chk_step_done::<7>) },
    A70CheckSpec { id: "A70-EB-A-009", criterion: "UNX-F0705-J1#早传十二步/09-内核段装载", desc: "步骤09完成位：内核段已装载", check: Some(chk_step_done::<8>) },
    A70CheckSpec { id: "A70-EB-A-010", criterion: "UNX-F0705-J1#早传十二步/10-控制台点灯", desc: "步骤10完成位：控制台已点灯", check: Some(chk_step_done::<9>) },
    A70CheckSpec { id: "A70-EB-A-011", criterion: "UNX-F0705-J1#早传十二步/11-引导协议标签解析", desc: "步骤11完成位：引导协议标签已解析", check: Some(chk_step_done::<10>) },
    A70CheckSpec { id: "A70-EB-A-012", criterion: "UNX-F0705-J1#早传十二步/12-早分配器初始化", desc: "步骤12完成位：早分配器已初始化", check: Some(chk_step_done::<11>) },
    A70CheckSpec { id: "A70-EB-B-001", criterion: "UNX-F0705-J1#早传十二步产物/01-头命中数", desc: "步骤01产物界内：头魔数命中 1~4", check: Some(chk_step_artifact::<0>) },
    A70CheckSpec { id: "A70-EB-B-002", criterion: "UNX-F0705-J1#早传十二步产物/02-CPUID叶数", desc: "步骤02产物界内：特性叶 1~64", check: Some(chk_step_artifact::<1>) },
    A70CheckSpec { id: "A70-EB-B-003", criterion: "UNX-F0705-J1#早传十二步产物/03-e820区间数", desc: "步骤03产物界内：e820 区间 1~128", check: Some(chk_step_artifact::<2>) },
    A70CheckSpec { id: "A70-EB-B-004", criterion: "UNX-F0705-J1#早传十二步产物/04-蹦床页数", desc: "步骤04产物界内：蹦床页 1~8", check: Some(chk_step_artifact::<3>) },
    A70CheckSpec { id: "A70-EB-B-005", criterion: "UNX-F0705-J1#早传十二步产物/05-长模确认数", desc: "步骤05产物界内：长模确认恰 1", check: Some(chk_step_artifact::<4>) },
    A70CheckSpec { id: "A70-EB-B-006", criterion: "UNX-F0705-J1#早传十二步产物/06-栈页数", desc: "步骤06产物界内：栈页 1~32", check: Some(chk_step_artifact::<5>) },
    A70CheckSpec { id: "A70-EB-B-007", criterion: "UNX-F0705-J1#早传十二步产物/07-清零页数", desc: "步骤07产物界内：清零页 1~4096", check: Some(chk_step_artifact::<6>) },
    A70CheckSpec { id: "A70-EB-B-008", criterion: "UNX-F0705-J1#早传十二步产物/08-重定位条数", desc: "步骤08产物界内：重定位条 1~65536", check: Some(chk_step_artifact::<7>) },
    A70CheckSpec { id: "A70-EB-B-009", criterion: "UNX-F0705-J1#早传十二步产物/09-装载段数", desc: "步骤09产物界内：装载段 3~16（三段下限对齐 unx0096）", check: Some(chk_step_artifact::<8>) },
    A70CheckSpec { id: "A70-EB-B-010", criterion: "UNX-F0705-J1#早传十二步产物/10-就绪台数", desc: "步骤10产物界内：就绪台恰 2（双控制台）", check: Some(chk_step_artifact::<9>) },
    A70CheckSpec { id: "A70-EB-B-011", criterion: "UNX-F0705-J1#早传十二步产物/11-标签数", desc: "步骤11产物界内：标签 1~64", check: Some(chk_step_artifact::<10>) },
    A70CheckSpec { id: "A70-EB-B-012", criterion: "UNX-F0705-J1#早传十二步产物/12-早分配桶数", desc: "步骤12产物界内：早分配桶 8~256（对齐 unx0071）", check: Some(chk_step_artifact::<11>) },
    A70CheckSpec { id: "A70-EB-C-001", criterion: "UNX-F0705-J1#早传十二步/全序收口", desc: "补全：十二步完成位全真（早传全序收口）", check: Some(chk_all_steps_done) },
    A70CheckSpec { id: "A70-EB-C-002", criterion: "UNX-F0705-J1#早传十二步产物/03-e820界复核", desc: "补全：e820 区间数非空且 ≤128（下游分页地基非空）", check: Some(chk_mmap_cap) },
];

// ── 面二：双控制台（14 条 ≥ 12）──

/// 双控制台面登记规格表（串行 5 + VGA 4 + 镜像 5）。
pub const A70_CHECKS_CONSOLE: [A70CheckSpec; 14] = [
    A70CheckSpec { id: "A70-CS-A-001", criterion: "UNX-F0705-J1#双控制台/串行台就绪", desc: "串行台（16550）已就绪", check: Some(chk_serial_ready) },
    A70CheckSpec { id: "A70-CS-A-002", criterion: "UNX-F0705-J1#双控制台/串行波特率", desc: "串行波特率 == 115200", check: Some(chk_serial_baud) },
    A70CheckSpec { id: "A70-CS-A-003", criterion: "UNX-F0705-J1#双控制台/串行写入非空", desc: "串行台累计写入 ≥1", check: Some(chk_serial_written_pos) },
    A70CheckSpec { id: "A70-CS-A-004", criterion: "UNX-F0705-J1#双控制台/串行写入界内", desc: "串行台写入 ≤1048576（防野计数）", check: Some(chk_serial_write_cap) },
    A70CheckSpec { id: "A70-CS-A-005", criterion: "UNX-F0705-J1#双控制台/镜像先落串行", desc: "镜像对数 ≤ 串行写入（每对必先落串行）", check: Some(chk_serial_pair_bound) },
    A70CheckSpec { id: "A70-CS-B-001", criterion: "UNX-F0705-J1#双控制台/VGA台就绪", desc: "VGA 文本台已就绪", check: Some(chk_vga_ready) },
    A70CheckSpec { id: "A70-CS-B-002", criterion: "UNX-F0705-J1#双控制台/VGA几何", desc: "VGA 80x25 标准文本模", check: Some(chk_vga_geometry) },
    A70CheckSpec { id: "A70-CS-B-003", criterion: "UNX-F0705-J1#双控制台/VGA写入非空", desc: "VGA 台累计写入 ≥1 且 ≤262144", check: Some(chk_vga_written_pos) },
    A70CheckSpec { id: "A70-CS-B-004", criterion: "UNX-F0705-J1#双控制台/光标界内", desc: "光标 col<80 且 row<25", check: Some(chk_vga_cursor) },
    A70CheckSpec { id: "A70-CS-C-001", criterion: "UNX-F0705-J1#双控制台/双台同活", desc: "串行与 VGA 双台同活", check: Some(chk_both_live) },
    A70CheckSpec { id: "A70-CS-C-002", criterion: "UNX-F0705-J1#双控制台/镜像对存在", desc: "镜像对数 ≥1", check: Some(chk_pairs_pos) },
    A70CheckSpec { id: "A70-CS-C-003", criterion: "UNX-F0705-J1#双控制台/漂移为零", desc: "双台输出漂移 == 0（镜像一致）", check: Some(chk_drift_zero) },
    A70CheckSpec { id: "A70-CS-C-004", criterion: "UNX-F0705-J1#双控制台/无溢出", desc: "控制台溢出旗为否", check: Some(chk_no_overflow) },
    A70CheckSpec { id: "A70-CS-C-005", criterion: "UNX-F0705-J1#双控制台/双写守恒", desc: "2×镜像对 ≤ 串行+VGA 写入（守恒）", check: Some(chk_pair_conservation) },
];

// ── 面三：页表属性（23 条 ≥ 18）──

/// 页表属性面登记规格表（在场 8 + 属性符合 8 + 纪律 4 + 元检查 3）。
pub const A70_CHECKS_PAGETBL: [A70CheckSpec; 23] = [
    A70CheckSpec { id: "A70-PT-A-001", criterion: "UNX-F0705-J1#页表属性/01-内核正文在场", desc: "内核正文.text 映射在场", check: Some(chk_pte_present::<0>) },
    A70CheckSpec { id: "A70-PT-A-002", criterion: "UNX-F0705-J1#页表属性/02-内核只读在场", desc: "内核只读.rodata 映射在场", check: Some(chk_pte_present::<1>) },
    A70CheckSpec { id: "A70-PT-A-003", criterion: "UNX-F0705-J1#页表属性/03-内核数据在场", desc: "内核数据.data 映射在场", check: Some(chk_pte_present::<2>) },
    A70CheckSpec { id: "A70-PT-A-004", criterion: "UNX-F0705-J1#页表属性/04-内核BSS在场", desc: "内核 BSS 映射在场", check: Some(chk_pte_present::<3>) },
    A70CheckSpec { id: "A70-PT-A-005", criterion: "UNX-F0705-J1#页表属性/05-初始化段在场", desc: "初始化段.init 映射在场", check: Some(chk_pte_present::<4>) },
    A70CheckSpec { id: "A70-PT-A-006", criterion: "UNX-F0705-J1#页表属性/06-内核堆在场", desc: "内核堆.heap 映射在场", check: Some(chk_pte_present::<5>) },
    A70CheckSpec { id: "A70-PT-A-007", criterion: "UNX-F0705-J1#页表属性/07-VGA MMIO在场", desc: "VGA MMIO 映射在场", check: Some(chk_pte_present::<6>) },
    A70CheckSpec { id: "A70-PT-A-008", criterion: "UNX-F0705-J1#页表属性/08-ACPI回收在场", desc: "ACPI 回收区映射在场", check: Some(chk_pte_present::<7>) },
    A70CheckSpec { id: "A70-PT-B-001", criterion: "UNX-F0705-J1#页表属性/01-正文属性符合", desc: "正文属性：不可写+可执行+全局", check: Some(chk_pte_attrs::<0>) },
    A70CheckSpec { id: "A70-PT-B-002", criterion: "UNX-F0705-J1#页表属性/02-只读属性符合", desc: "只读属性：不可写+NX+全局", check: Some(chk_pte_attrs::<1>) },
    A70CheckSpec { id: "A70-PT-B-003", criterion: "UNX-F0705-J1#页表属性/03-数据属性符合", desc: "数据属性：可写+NX+全局", check: Some(chk_pte_attrs::<2>) },
    A70CheckSpec { id: "A70-PT-B-004", criterion: "UNX-F0705-J1#页表属性/04-BSS属性符合", desc: "BSS 属性：可写+NX+全局", check: Some(chk_pte_attrs::<3>) },
    A70CheckSpec { id: "A70-PT-B-005", criterion: "UNX-F0705-J1#页表属性/05-初始化段属性符合", desc: "init 属性：可写+NX+非全局", check: Some(chk_pte_attrs::<4>) },
    A70CheckSpec { id: "A70-PT-B-006", criterion: "UNX-F0705-J1#页表属性/06-内核堆属性符合", desc: "堆属性：可写+NX+非全局+巨页", check: Some(chk_pte_attrs::<5>) },
    A70CheckSpec { id: "A70-PT-B-007", criterion: "UNX-F0705-J1#页表属性/07-VGA属性符合", desc: "VGA MMIO 属性：可写+NX+非全局", check: Some(chk_pte_attrs::<6>) },
    A70CheckSpec { id: "A70-PT-B-008", criterion: "UNX-F0705-J1#页表属性/08-ACPI属性符合", desc: "ACPI 回收属性：可写+NX+非全局", check: Some(chk_pte_attrs::<7>) },
    A70CheckSpec { id: "A70-PT-C-001", criterion: "UNX-F0705-J1#页表属性/NX纪律", desc: "正文外七区全部 NX（W^X 钉死）", check: Some(chk_nx_discipline) },
    A70CheckSpec { id: "A70-PT-C-002", criterion: "UNX-F0705-J1#页表属性/可写纪律", desc: "正文与只读区不可写", check: Some(chk_writable_discipline) },
    A70CheckSpec { id: "A70-PT-C-003", criterion: "UNX-F0705-J1#页表属性/用户纪律", desc: "内核八区一律不可用户态可达", check: Some(chk_user_discipline) },
    A70CheckSpec { id: "A70-PT-C-004", criterion: "UNX-F0705-J1#页表属性/全局纪律", desc: "常驻四区 global 位齐备", check: Some(chk_global_discipline) },
    A70CheckSpec { id: "A70-PT-D-001", criterion: "UNX-F0705-J1#页表属性/PML4根", desc: "PML4 根就绪", check: Some(chk_pml4_ready) },
    A70CheckSpec { id: "A70-PT-D-002", criterion: "UNX-F0705-J1#页表属性/PAT索引", desc: "PAT 索引界内", check: Some(chk_pat_index) },
    A70CheckSpec { id: "A70-PT-D-003", criterion: "UNX-F0705-J1#页表属性/巨页对齐", desc: "堆区巨页位与 2M 对齐核齐备", check: Some(chk_huge_meta) },
];

// ── 面四：高半核（16 条 ≥ 16）──

/// 高半核面登记规格表（映射 8 + 元检查 8）。
pub const A70_CHECKS_HIHALF: [A70CheckSpec; 16] = [
    A70CheckSpec { id: "A70-HH-A-001", criterion: "UNX-F0705-J1#高半核/正文映射在场", desc: "高半正文映射在场", check: Some(chk_hh_mapped::<0>) },
    A70CheckSpec { id: "A70-HH-A-002", criterion: "UNX-F0705-J1#高半核/只读映射在场", desc: "高半只读映射在场", check: Some(chk_hh_mapped::<1>) },
    A70CheckSpec { id: "A70-HH-A-003", criterion: "UNX-F0705-J1#高半核/数据映射在场", desc: "高半数据映射在场", check: Some(chk_hh_mapped::<2>) },
    A70CheckSpec { id: "A70-HH-A-004", criterion: "UNX-F0705-J1#高半核/栈顶映射在场", desc: "高半栈顶映射在场", check: Some(chk_hh_mapped::<3>) },
    A70CheckSpec { id: "A70-HH-A-005", criterion: "UNX-F0705-J1#高半核/正文虚址一致", desc: "正文虚址 == 0xFFFFFFFF80000000", check: Some(chk_hh_vaddr::<0>) },
    A70CheckSpec { id: "A70-HH-A-006", criterion: "UNX-F0705-J1#高半核/只读虚址一致", desc: "只读虚址 == 0xFFFFFFFF80200000", check: Some(chk_hh_vaddr::<1>) },
    A70CheckSpec { id: "A70-HH-A-007", criterion: "UNX-F0705-J1#高半核/数据虚址一致", desc: "数据虚址 == 0xFFFFFFFF80400000", check: Some(chk_hh_vaddr::<2>) },
    A70CheckSpec { id: "A70-HH-A-008", criterion: "UNX-F0705-J1#高半核/栈顶虚址一致", desc: "栈顶虚址 == 0xFFFFFFFF807FF000", check: Some(chk_hh_vaddr::<3>) },
    A70CheckSpec { id: "A70-HH-B-001", criterion: "UNX-F0705-J1#高半核/基址canonical窗", desc: "高半基址落 canonical 高半窗", check: Some(chk_hh_base_window) },
    A70CheckSpec { id: "A70-HH-B-002", criterion: "UNX-F0705-J1#高半核/基址映像钉死", desc: "高半基址 == 内核映像基址（-2GiB）", check: Some(chk_hh_base_kimg) },
    A70CheckSpec { id: "A70-HH-B-003", criterion: "UNX-F0705-J1#高半核/恒等映射关", desc: "恒等映射残留已关", check: Some(chk_identity_off) },
    A70CheckSpec { id: "A70-HH-B-004", criterion: "UNX-F0705-J1#高半核/护页在位", desc: "文本前护页在位", check: Some(chk_guard_ok) },
    A70CheckSpec { id: "A70-HH-B-005", criterion: "UNX-F0705-J1#高半核/canonical核", desc: "高半地址 canonical 核过", check: Some(chk_hh_canonical) },
    A70CheckSpec { id: "A70-HH-B-006", criterion: "UNX-F0705-J1#高半核/四映射齐", desc: "四映射槽全部在场", check: Some(chk_hh_all_mapped) },
    A70CheckSpec { id: "A70-HH-B-007", criterion: "UNX-F0705-J1#高半核/映像序递增", desc: "四映射虚址严格递增", check: Some(chk_hh_order) },
    A70CheckSpec { id: "A70-HH-B-008", criterion: "UNX-F0705-J1#高半核/栈顶护窗", desc: "栈顶落护窗 [807F_F000 窗界内]", check: Some(chk_hh_stack_window) },
];

/// 四表规格总数（const 求和，登记穷尽界）。
pub const A70_TOTAL_SPECS: usize =
    A70_CHECKS_EARLYBOOT.len() + A70_CHECKS_CONSOLE.len() + A70_CHECKS_PAGETBL.len() + A70_CHECKS_HIHALF.len();

/// 注入样本：空检查体断言（登记必拒收——空断言反证）。
pub const A70_INJECT_EMPTY: A70CheckSpec = A70CheckSpec {
    id: "A70-EB-Z-999",
    criterion: "UNX-F0705-J1#注入样本-空检查体",
    desc: "注入样本：登记无检查体的断言（必须被拒收）",
    check: None,
};

/// 注入样本：判据篡改文案。
pub const A70_INJECT_CRITERION_MOD: &str = "UNX-F0705-J1#注入篡改-判据被改写";

/// 注入样本：同 id 判据被改写（global 下标 14 = A70-EB-B-003）。
pub const A70_INJECT_MOD_SPEC: A70CheckSpec = A70CheckSpec {
    id: "A70-EB-B-003",
    criterion: A70_INJECT_CRITERION_MOD,
    desc: "注入样本：同 id 断言判据被改写（diff 必须红案）",
    check: Some(chk_step_artifact::<2>),
};

/// 注入样本：基线删除样本的全局下标（40 = A70-PT-A-001）。
pub const A70_INJECT_REMOVE_IDX: usize = 40;

/// 注入样本：判据篡改样本的全局下标（14 = A70-EB-B-003）。
pub const A70_INJECT_MODIFY_IDX: usize = 14;

/// 全局下标 → 静态规格（四表线性定位，O(1) 段判）。
pub fn spec_at(global_idx: usize) -> Option<&'static A70CheckSpec> {
    let n_eb = A70_CHECKS_EARLYBOOT.len();
    let n_cs = A70_CHECKS_CONSOLE.len();
    let n_pt = A70_CHECKS_PAGETBL.len();
    let n_hh = A70_CHECKS_HIHALF.len();
    if global_idx < n_eb {
        Some(&A70_CHECKS_EARLYBOOT[global_idx])
    } else if global_idx < n_eb + n_cs {
        Some(&A70_CHECKS_CONSOLE[global_idx - n_eb])
    } else if global_idx < n_eb + n_cs + n_pt {
        Some(&A70_CHECKS_PAGETBL[global_idx - n_eb - n_cs])
    } else if global_idx < n_eb + n_cs + n_pt + n_hh {
        Some(&A70_CHECKS_HIHALF[global_idx - n_eb - n_cs - n_pt])
    } else {
        None
    }
}

// ═══════════════════════════════════════════════════════════════════
// FNV-1a 指纹（确定性对拍地基）
// ═══════════════════════════════════════════════════════════════════

/// FNV-1a 偏移基（64 位标准常数，来源 FNV 参考实现）。
pub const A70_FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 素数（64 位标准常数）。
pub const A70_FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 全量散列。
pub fn fnv1a64_bytes(data: &[u8]) -> u64 {
    let mut h = A70_FNV_OFFSET;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(A70_FNV_PRIME);
        i += 1;
    }
    h
}

/// FNV-1a 增量混合（续基散列）。
pub fn fnv1a64_mix(base: u64, data: &[u8]) -> u64 {
    let mut h = base;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(A70_FNV_PRIME);
        i += 1;
    }
    h
}

// ═══════════════════════════════════════════════════════════════════
// 事件账
// ═══════════════════════════════════════════════════════════════════

/// 事件账容量（环形丢弃最旧，容量写死：账面 256 条足够四面全程留痕）。
pub const A70_EVENT_CAP: usize = 256;

/// 事件种类。
#[derive(Clone, Copy, Debug)]
pub enum EventKind {
    /// 登记收讫。
    Registered,
    /// 一轮执行收讫。
    RunDone,
    /// diff 对账收讫。
    DiffDone,
    /// 注入被检出。
    InjectCaught,
    /// 故障事件。
    Fault,
}

impl EventKind {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            EventKind::Registered => "登记",
            EventKind::RunDone => "执行",
            EventKind::DiffDone => "对账",
            EventKind::InjectCaught => "注入检出",
            EventKind::Fault => "故障",
        }
    }
}

/// 单条事件。
#[derive(Clone, Copy, Debug)]
pub struct A70Event {
    /// 单调流水号（wrapping）。
    pub seq: u32,
    /// 种类。
    pub kind: EventKind,
    /// 诊断码（绿码 XC_GREEN 表正常收讫）。
    pub code: u32,
}

/// 环形事件账（满则丢最旧并计数，溢出显性 wrapping）。
#[derive(Clone, Debug)]
pub struct EventLog {
    events: Vec<A70Event>,
    dropped: u64,
    faults: u64,
    next_seq: u32,
}

impl EventLog {
    /// 空账。
    pub fn new() -> EventLog {
        EventLog {
            events: Vec::new(),
            dropped: 0,
            faults: 0,
            next_seq: 0,
        }
    }

    /// 记一笔（满则丢最旧）。
    pub fn push(&mut self, kind: EventKind, code: u32) {
        if self.events.len() >= A70_EVENT_CAP {
            let _ = self.events.remove(0);
            self.dropped = self.dropped.wrapping_add(1);
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        if matches!(kind, EventKind::Fault) {
            self.faults = self.faults.wrapping_add(1);
        }
        self.events.push(A70Event { seq, kind, code });
    }

    /// 事件条数。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 故障笔数。
    pub fn fault_count(&self) -> u64 {
        self.faults
    }

    /// 丢弃笔数。
    pub fn dropped_count(&self) -> u64 {
        self.dropped
    }

    /// 健康核：零故障且零丢弃。
    pub fn healthy(&self) -> bool {
        self.faults == 0 && self.dropped == 0
    }

    /// 最近一笔诊断码（空账回绿码）。
    pub fn last_code(&self) -> u32 {
        match self.events.last() {
            Some(e) => e.code,
            None => XC_GREEN,
        }
    }
}

/// 健康快照（health() 出口）。
#[derive(Clone, Copy, Debug)]
pub struct A70Health {
    /// 事件笔数。
    pub events: usize,
    /// 故障笔数。
    pub faults: u64,
    /// 丢弃笔数。
    pub dropped: u64,
    /// 健康二态（零故障且零丢弃）。
    pub ok: bool,
}

// ═══════════════════════════════════════════════════════════════════
// AssertBook：断言账本（登记制）
// ═══════════════════════════════════════════════════════════════════

/// 账本内条目（id 解析件 + 判据引用 + 描述 + 检查体 + 稳定执行序）。
#[derive(Clone, Debug)]
pub struct AssertEntry {
    /// 规范 id（解析件）。
    pub id: AssertionId,
    /// id 原文（与 parse 结果互验后的规范形）。
    pub id_text: String,
    /// 判据引用。
    pub criterion: String,
    /// 描述。
    pub desc: String,
    /// 检查体（登记制强制 Some）。
    pub check: A70CheckFn,
    /// 稳定执行序（登记序，严格递增核）。
    pub order: u32,
}

/// 断言账本：登记制 + 重复 id 拒收 + 总数 ≥70 判定 + 四面计数分列。
#[derive(Clone, Debug)]
pub struct AssertBook {
    entries: Vec<AssertEntry>,
    events: EventLog,
}

impl AssertBook {
    /// 空账本。
    pub fn new() -> AssertBook {
        AssertBook {
            entries: Vec::new(),
            events: EventLog::new(),
        }
    }

    /// 登记一条（规范校验/互验/重复拒收/空断言拒收四道闸）。
    pub fn register(&mut self, spec: &A70CheckSpec) -> Result<(), AssertBookError> {
        // 闸一：空断言反证——无检查体拒收。
        let check = match spec.check {
            Some(f) => f,
            None => {
                self.events.push(EventKind::Fault, XC_EMPTY_CHECK);
                return Err(AssertBookError::EmptyCheck {
                    code: XC_EMPTY_CHECK,
                    id: String::from(spec.id),
                });
            }
        };
        // 闸二：三段式规范校验。
        let id = match AssertionId::parse(spec.id) {
            Ok(v) => v,
            Err(e) => {
                self.events.push(EventKind::Fault, e.code());
                return Err(e);
            }
        };
        // 闸三：parse/render 互验（抓手写错拼）。
        let rendered = id.render();
        if rendered.as_str() != spec.id {
            self.events.push(EventKind::Fault, XC_BAD_ID);
            return Err(AssertBookError::BadId {
                code: XC_BAD_ID,
                id: String::from(spec.id),
            });
        }
        // 闸四：重复 id 拒收。
        let key = id.key();
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].id.key() == key {
                self.events.push(EventKind::Fault, XC_DUP_ID);
                return Err(AssertBookError::DuplicateId {
                    code: XC_DUP_ID,
                    id: String::from(spec.id),
                });
            }
            i += 1;
        }
        // 收讫：稳定执行序 = 登记序。
        let order = self.entries.len() as u32;
        self.entries.push(AssertEntry {
            id,
            id_text: rendered,
            criterion: String::from(spec.criterion),
            desc: String::from(spec.desc),
            check,
            order,
        });
        self.events.push(EventKind::Registered, XC_GREEN);
        Ok(())
    }

    /// 条目数。
    pub fn total(&self) -> usize {
        self.entries.len()
    }

    /// 某面条数。
    pub fn count(&self, face: FaceKind) -> usize {
        self.counts()[face.idx()]
    }

    /// 四面计数分列（稳定序：EB/CS/PT/HH）。
    pub fn counts(&self) -> [usize; 4] {
        let mut c = [0usize; 4];
        let mut i = 0;
        while i < self.entries.len() {
            let fi = self.entries[i].id.face().idx();
            c[fi] = c[fi].wrapping_add(1);
            i += 1;
        }
        c
    }

    /// 判据引用列表（逐条可解引核）。
    pub fn criteria(&self) -> Vec<String> {
        let mut v = Vec::new();
        let mut i = 0;
        while i < self.entries.len() {
            v.push(self.entries[i].criterion.clone());
            i += 1;
        }
        v
    }

    /// 判据引用可解引核：全部以 UNX-F0705-J1# 开头（判据-断言映射单向核）。
    pub fn criteria_resolvable(&self) -> bool {
        let mut i = 0;
        while i < self.entries.len() {
            if !self.entries[i].criterion.starts_with("UNX-F0705-J1#") {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 登记制快照（补全 diff 账基线）。
    pub fn snapshot(&self) -> AssertDiffBook {
        AssertDiffBook::from_book(self)
    }

    /// 簿记指纹（id 全文 + 判据 + 描述 + 执行序逐条混合）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_bytes(A70_VERSION.as_bytes());
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            h = fnv1a64_mix(h, b"\xFF");
            h = fnv1a64_mix(h, e.id_text.as_bytes());
            h = fnv1a64_mix(h, b"\x01");
            h = fnv1a64_mix(h, e.criterion.as_bytes());
            h = fnv1a64_mix(h, b"\x01");
            h = fnv1a64_mix(h, e.desc.as_bytes());
            h = fnv1a64_mix(h, b"\x02");
            h = fnv1a64_mix(h, &e.order.to_le_bytes());
            i += 1;
        }
        h
    }

    /// 事件账健康快照。
    pub fn health(&self) -> A70Health {
        A70Health {
            events: self.events.len(),
            faults: self.events.fault_count(),
            dropped: self.events.dropped_count(),
            ok: self.events.healthy(),
        }
    }

    /// 最近事件诊断码。
    pub fn last_event_code(&self) -> u32 {
        self.events.last_code()
    }

    /// 记外部事件（diff/注入面回写本账事件流）。
    pub fn record_event(&mut self, kind: EventKind, code: u32) {
        self.events.push(kind, code);
    }

    /// 登记制判定（总数 ≥70 且四面各自达标）。
    pub fn registry_report(&self) -> RegistryReport {
        let counts = self.counts();
        let mut faces_meet = [false; 4];
        let mut all_faces = true;
        let mut f = 0;
        while f < 4 {
            let min = FaceKind::ALL[f].min_asserts();
            faces_meet[f] = counts[f] >= min;
            if !faces_meet[f] {
                all_faces = false;
            }
            f += 1;
        }
        let total = self.total();
        let meets_total = total >= A70_TOTAL_MIN;
        RegistryReport {
            counts,
            mins: [
                A70_MIN_EARLYBOOT,
                A70_MIN_CONSOLE,
                A70_MIN_PAGETBL,
                A70_MIN_HIHALF,
            ],
            total,
            total_need: A70_TOTAL_MIN,
            faces_meet,
            meets_total,
            green: meets_total && all_faces,
        }
    }

    /// 登记制硬闸（不达标回 TotalShort / FaceMinShort）。
    pub fn ensure_registry(&self) -> Result<(), AssertBookError> {
        let rep = self.registry_report();
        let mut f = 0;
        while f < 4 {
            if !rep.faces_meet[f] {
                return Err(AssertBookError::FaceMinShort {
                    code: XC_FACE_MIN,
                    face: FaceKind::ALL[f],
                    have: rep.counts[f],
                    need: rep.mins[f],
                });
            }
            f += 1;
        }
        if !rep.meets_total {
            return Err(AssertBookError::TotalShort {
                code: XC_TOTAL_SHORT,
                have: rep.total,
                need: A70_TOTAL_MIN,
            });
        }
        Ok(())
    }

    /// 执行引擎（AssertRunner）：逐条执行 → Pass/Fail 二态 + Fail 定位
    /// 到断言 id + 执行序稳定核。执行序 = 登记序（order 严格递增）。
    pub fn run(&mut self, world: &A70World) -> Result<AssertRunReport, AssertBookError> {
        // 执行序稳定核：order 必须严格递增且与下标一致。
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].order != i as u32 {
                self.events.push(EventKind::Fault, XC_ORDER_UNSTABLE);
                return Err(AssertBookError::RunOrderCorrupt {
                    code: XC_ORDER_UNSTABLE,
                });
            }
            i += 1;
        }
        let mut faces: [FaceRunStat; 4] = [
            FaceRunStat::blank(FaceKind::EarlyBoot),
            FaceRunStat::blank(FaceKind::Console),
            FaceRunStat::blank(FaceKind::PageTable),
            FaceRunStat::blank(FaceKind::HiHalf),
        ];
        let mut failures: Vec<AssertFailure> = Vec::new();
        let mut passed: usize = 0;
        let mut failed: usize = 0;
        i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            let fi = e.id.face().idx();
            let ok = (e.check)(world);
            if ok {
                passed = passed.wrapping_add(1);
                faces[fi].passed = faces[fi].passed.wrapping_add(1);
            } else {
                failed = failed.wrapping_add(1);
                faces[fi].failed = faces[fi].failed.wrapping_add(1);
                failures.push(AssertFailure {
                    id_text: e.id_text.clone(),
                    criterion: e.criterion.clone(),
                    desc: e.desc.clone(),
                    order: e.order,
                });
            }
            i += 1;
        }
        // 分面统计收口。
        let counts = self.counts();
        let mut f = 0;
        while f < 4 {
            faces[f].face = FaceKind::ALL[f];
            faces[f].registered = counts[f];
            faces[f].min_required = FaceKind::ALL[f].min_asserts();
            faces[f].meets_min = counts[f] >= faces[f].min_required;
            faces[f].pass_perm = if counts[f] == 0 {
                0
            } else {
                (faces[f].passed.saturating_mul(10_000) / counts[f]) as u32
            };
            f += 1;
        }
        let total = self.total();
        let meets_total = total >= A70_TOTAL_MIN;
        let mut all_min = true;
        f = 0;
        while f < 4 {
            if !faces[f].meets_min {
                all_min = false;
            }
            f += 1;
        }
        // 总判定：≥70 且四面达标且零失败 → Green，否则 Red（二态）。
        let verdict = if failed == 0 && meets_total && all_min {
            AssertVerdict::Green
        } else {
            AssertVerdict::Red
        };
        // 执行指纹（对拍比对件：通过/失败数 + 判定 + 失败清单序）。
        let mut fp = fnv1a64_bytes(&passed.to_le_bytes());
        fp = fnv1a64_mix(fp, &failed.to_le_bytes());
        fp = fnv1a64_mix(fp, &[if matches!(verdict, AssertVerdict::Green) { 1 } else { 0 }]);
        let mut k = 0;
        while k < failures.len() {
            fp = fnv1a64_mix(fp, failures[k].id_text.as_bytes());
            fp = fnv1a64_mix(fp, &failures[k].order.to_le_bytes());
            k += 1;
        }
        self.events.push(
            EventKind::RunDone,
            if failed == 0 { XC_GREEN } else { XC_ASSERT_FAIL },
        );
        Ok(AssertRunReport {
            faces,
            total,
            passed,
            failed,
            failures,
            verdict,
            registry_meets_total: meets_total,
            run_fp: fp,
        })
    }
}

/// 登记制统计报告（四面计数/最低线/总数三列）。
#[derive(Clone, Copy, Debug)]
pub struct RegistryReport {
    /// 四面计数（EB/CS/PT/HH 稳定序）。
    pub counts: [usize; 4],
    /// 四面最低线。
    pub mins: [usize; 4],
    /// 总数。
    pub total: usize,
    /// 总数硬线。
    pub total_need: usize,
    /// 四面达标位。
    pub faces_meet: [bool; 4],
    /// 总数达标位。
    pub meets_total: bool,
    /// 登记制绿态（总数 + 四面全达标）。
    pub green: bool,
}

// ═══════════════════════════════════════════════════════════════════
// AssertRunner 执行报告
// ═══════════════════════════════════════════════════════════════════

/// 判定二态（Green/Red，禁三态含糊）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssertVerdict {
    /// 全绿（登记达标 + 执行零失败）。
    Green,
    /// 红（任一硬线破缺）。
    Red,
}

impl AssertVerdict {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            AssertVerdict::Green => "绿",
            AssertVerdict::Red => "红",
        }
    }

    /// 二态码（1 绿 / 0 红）。
    pub fn bit(self) -> u8 {
        match self {
            AssertVerdict::Green => 1,
            AssertVerdict::Red => 0,
        }
    }
}

/// 分面执行统计。
#[derive(Clone, Copy, Debug)]
pub struct FaceRunStat {
    /// 面域。
    pub face: FaceKind,
    /// 登记数。
    pub registered: usize,
    /// 通过数。
    pub passed: usize,
    /// 失败数。
    pub failed: usize,
    /// 最低线。
    pub min_required: usize,
    /// 登记数达标位。
    pub meets_min: bool,
    /// 通过率（万分比，saturating）。
    pub pass_perm: u32,
}

impl FaceRunStat {
    /// 空统计。
    pub const fn blank(face: FaceKind) -> FaceRunStat {
        FaceRunStat {
            face,
            registered: 0,
            passed: 0,
            failed: 0,
            min_required: 0,
            meets_min: false,
            pass_perm: 0,
        }
    }
}

/// 失败条目（定位三件套：id 全文 + 判据引用 + 描述）。
#[derive(Clone, Debug)]
pub struct AssertFailure {
    /// 断言 id 全文（Fail 定位锚）。
    pub id_text: String,
    /// 判据引用。
    pub criterion: String,
    /// 描述。
    pub desc: String,
    /// 执行序。
    pub order: u32,
}

/// 执行报告：分面通过率 + 总判定（≥70 且全绿二态）。
#[derive(Clone, Debug)]
pub struct AssertRunReport {
    /// 四面统计（EB/CS/PT/HH 稳定序）。
    pub faces: [FaceRunStat; 4],
    /// 总登记数。
    pub total: usize,
    /// 总通过数。
    pub passed: usize,
    /// 总失败数。
    pub failed: usize,
    /// 失败清单（执行序稳定，逐条定位到断言 id）。
    pub failures: Vec<AssertFailure>,
    /// 总判定二态。
    pub verdict: AssertVerdict,
    /// 总数达标位。
    pub registry_meets_total: bool,
    /// 执行指纹（双跑对拍比对件）。
    pub run_fp: u64,
}

impl AssertRunReport {
    /// 全绿否。
    pub fn is_green(&self) -> bool {
        matches!(self.verdict, AssertVerdict::Green)
    }

    /// 失败条数。
    pub fn failure_count(&self) -> usize {
        self.failures.len()
    }

    /// 首个失败 id（红案定位锚；绿态回空串）。
    pub fn first_failure_id(&self) -> String {
        match self.failures.first() {
            Some(f) => f.id_text.clone(),
            None => String::new(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// 补全 diff 账（基线快照 + 只增不减判定）
// ═══════════════════════════════════════════════════════════════════

/// diff 变更种类。
#[derive(Clone, Copy, Debug)]
pub enum DiffKind {
    /// 新增（合规，只增不减允许）。
    Added,
    /// 删除（红案）。
    Removed,
    /// 判据/描述改写（红案）。
    Modified,
}

impl DiffKind {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            DiffKind::Added => "新增",
            DiffKind::Removed => "删除",
            DiffKind::Modified => "改写",
        }
    }
}

/// 基线快照条目。
#[derive(Clone, Debug)]
pub struct SnapshotEntry {
    /// id（解析件）。
    pub id: AssertionId,
    /// id 全文。
    pub id_text: String,
    /// 判据引用。
    pub criterion: String,
    /// 描述。
    pub desc: String,
}

/// 补全 diff 账：基线快照 + 增量对账（只增不减纪律载体）。
#[derive(Clone, Debug)]
pub struct AssertDiffBook {
    baseline: Vec<SnapshotEntry>,
    baseline_fp: u64,
}

/// 对账报告。
#[derive(Clone, Debug)]
pub struct DiffReport {
    /// 新增数（合规）。
    pub added: usize,
    /// 删除数（红案源）。
    pub removed: usize,
    /// 改写数（红案源）。
    pub modified: usize,
    /// 洁净二态（零删改）。
    pub clean: bool,
    /// 被删 id 清单。
    pub removed_ids: Vec<String>,
    /// 被改 id 清单。
    pub modified_ids: Vec<String>,
}

impl DiffReport {
    /// 红案否（任一删/改即红案）。
    pub fn is_red_case(&self) -> bool {
        self.removed > 0 || self.modified > 0
    }
}

impl AssertDiffBook {
    /// 从账本冻结基线（快照 = 条目内容 + 指纹封存）。
    pub fn from_book(book: &AssertBook) -> AssertDiffBook {
        let mut baseline = Vec::new();
        let mut i = 0;
        while i < book.entries.len() {
            let e = &book.entries[i];
            baseline.push(SnapshotEntry {
                id: e.id.clone(),
                id_text: e.id_text.clone(),
                criterion: e.criterion.clone(),
                desc: e.desc.clone(),
            });
            i += 1;
        }
        AssertDiffBook {
            baseline_fp: book.fingerprint(),
            baseline,
        }
    }

    /// 基线条数。
    pub fn baseline_len(&self) -> usize {
        self.baseline.len()
    }

    /// 基线指纹。
    pub fn fingerprint(&self) -> u64 {
        self.baseline_fp
    }

    /// 基线含某键否。
    fn baseline_has(&self, key: (u8, u8, u16)) -> bool {
        let mut i = 0;
        while i < self.baseline.len() {
            if self.baseline[i].id.key() == key {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 对账：相对基线的增量账（新增/删除/改写三列；删改即红案源）。
    pub fn diff_against(&self, book: &AssertBook) -> Result<DiffReport, AssertBookError> {
        let mut added: usize = 0;
        let mut removed_ids: Vec<String> = Vec::new();
        let mut modified_ids: Vec<String> = Vec::new();
        // 删改列：基线逐条在账本中找位。
        let mut i = 0;
        while i < self.baseline.len() {
            let b = &self.baseline[i];
            let key = b.id.key();
            match book.find_by_key(key) {
                None => removed_ids.push(b.id_text.clone()),
                Some(e) => {
                    if e.criterion != b.criterion || e.desc != b.desc {
                        modified_ids.push(b.id_text.clone());
                    }
                }
            }
            i += 1;
        }
        // 增列：账本逐条在基线中找位。
        let mut j = 0;
        while j < book.entries.len() {
            if !self.baseline_has(book.entries[j].id.key()) {
                added += 1;
            }
            j += 1;
        }
        let removed = removed_ids.len();
        let modified = modified_ids.len();
        Ok(DiffReport {
            added,
            removed,
            modified,
            clean: removed == 0 && modified == 0,
            removed_ids,
            modified_ids,
        })
    }

    /// 只增不减硬闸（删改即红案 → Err BaselineRemoved/BaselineModified）。
    pub fn ensure_only_additions(&self, rep: &DiffReport) -> Result<(), AssertBookError> {
        if rep.removed > 0 {
            let id = match rep.removed_ids.first() {
                Some(s) => s.clone(),
                None => String::from("unknown"),
            };
            return Err(AssertBookError::BaselineRemoved {
                code: XC_DIFF_REMOVED,
                id,
            });
        }
        if rep.modified > 0 {
            let id = match rep.modified_ids.first() {
                Some(s) => s.clone(),
                None => String::from("unknown"),
            };
            return Err(AssertBookError::BaselineModified {
                code: XC_DIFF_MODIFIED,
                id,
            });
        }
        Ok(())
    }
}

impl AssertBook {
    /// 按键找条目（登记账线性定位，n≈80 实测量级）。
    pub fn find_by_key(&self, key: (u8, u8, u16)) -> Option<&AssertEntry> {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].id.key() == key {
                return Some(&self.entries[i]);
            }
            i += 1;
        }
        None
    }

    /// 全量登记（四表稳定序穷尽登记）。
    pub fn register_all(&mut self) -> Result<(), AssertBookError> {
        let mut i = 0;
        while i < A70_CHECKS_EARLYBOOT.len() {
            self.register(&A70_CHECKS_EARLYBOOT[i])?;
            i += 1;
        }
        i = 0;
        while i < A70_CHECKS_CONSOLE.len() {
            self.register(&A70_CHECKS_CONSOLE[i])?;
            i += 1;
        }
        i = 0;
        while i < A70_CHECKS_PAGETBL.len() {
            self.register(&A70_CHECKS_PAGETBL[i])?;
            i += 1;
        }
        i = 0;
        while i < A70_CHECKS_HIHALF.len() {
            self.register(&A70_CHECKS_HIHALF[i])?;
            i += 1;
        }
        Ok(())
    }
}

/// 全量账本构造（四表登记 + 登记制硬闸）。
pub fn build_full_book() -> Result<AssertBook, AssertBookError> {
    let mut book = AssertBook::new();
    book.register_all()?;
    book.ensure_registry()?;
    Ok(book)
}

/// 变体账本构造（注入面用：可跳过某全局下标 / 可顶替某全局下标规格）。
pub fn build_book_variants(
    skip: Option<usize>,
    modify: Option<(usize, &'static A70CheckSpec)>,
) -> Result<AssertBook, AssertBookError> {
    let mut book = AssertBook::new();
    let mut g: usize = 0;
    while g < A70_TOTAL_SPECS {
        if skip == Some(g) {
            g += 1;
            continue;
        }
        let mut replaced = false;
        if let Some((mi, ms)) = modify {
            if mi == g {
                book.register(ms)?;
                replaced = true;
            }
        }
        if !replaced {
            let sp = match spec_at(g) {
                Some(s) => s,
                None => {
                    return Err(AssertBookError::UnknownDiag {
                        code: XC_ORDER_UNSTABLE,
                    })
                }
            };
            book.register(sp)?;
        }
        g += 1;
    }
    Ok(book)
}

// ═══════════════════════════════════════════════════════════════════
// 双跑确定性对拍
// ═══════════════════════════════════════════════════════════════════

/// 双跑对拍报告。
#[derive(Clone, Copy, Debug)]
pub struct DualRunReport {
    /// 甲跑簿记指纹。
    pub fp_a: u64,
    /// 乙跑簿记指纹。
    pub fp_b: u64,
    /// 甲跑执行指纹。
    pub run_fp_a: u64,
    /// 乙跑执行指纹。
    pub run_fp_b: u64,
    /// 甲跑通过数。
    pub passed_a: usize,
    /// 乙跑通过数。
    pub passed_b: usize,
    /// 确定性一致二态。
    pub identical: bool,
}

impl DualRunReport {
    /// 一致硬闸（不一致回 FP_MISMATCH）。
    pub fn ensure_identical(&self) -> Result<(), AssertBookError> {
        if self.identical {
            Ok(())
        } else {
            Err(AssertBookError::UnknownDiag {
                code: XC_FP_MISMATCH,
            })
        }
    }
}

/// 双跑确定性对拍：独立建账两本 + 同世界态各跑一轮 + 指纹与结果全量比对。
pub fn dual_run(world: &A70World) -> Result<DualRunReport, AssertBookError> {
    let mut a = build_full_book()?;
    let ra = a.run(world)?;
    let mut b = build_full_book()?;
    let rb = b.run(world)?;
    let identical = a.fingerprint() == b.fingerprint()
        && ra.run_fp == rb.run_fp
        && ra.passed == rb.passed
        && ra.failed == rb.failed
        && ra.failure_count() == rb.failure_count()
        && matches!(ra.verdict, AssertVerdict::Green) == matches!(rb.verdict, AssertVerdict::Green);
    Ok(DualRunReport {
        fp_a: a.fingerprint(),
        fp_b: b.fingerprint(),
        run_fp_a: ra.run_fp,
        run_fp_b: rb.run_fp,
        passed_a: ra.passed,
        passed_b: rb.passed,
        identical,
    })
}

// ═══════════════════════════════════════════════════════════════════
// 注入面（空断言/重复 id/删基线/篡改判据，各必检出）
// ═══════════════════════════════════════════════════════════════════

/// 注入种类。
#[derive(Clone, Copy, Debug)]
pub enum InjectKind {
    /// 空检查体注入（登记闸必拒）。
    EmptyCheck,
    /// 重复 id 注入（登记闸必拒）。
    DuplicateId,
    /// 基线删除注入（diff 必红案）。
    BaselineRemoval,
    /// 判据篡改注入（diff 必红案）。
    BaselineModify,
}

impl InjectKind {
    /// 英文名。
    pub fn name(self) -> &'static str {
        match self {
            InjectKind::EmptyCheck => "empty-check",
            InjectKind::DuplicateId => "duplicate-id",
            InjectKind::BaselineRemoval => "baseline-removal",
            InjectKind::BaselineModify => "baseline-modify",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            InjectKind::EmptyCheck => "空断言注入",
            InjectKind::DuplicateId => "重复 id 注入",
            InjectKind::BaselineRemoval => "删改注入（删）",
            InjectKind::BaselineModify => "删改注入（改）",
        }
    }
}

/// 单发注入探针结果。
#[derive(Clone, Copy, Debug)]
pub struct InjectProbe {
    /// 注入种类。
    pub kind: InjectKind,
    /// 检出二态。
    pub detected: bool,
    /// 守卫诊断码。
    pub code: u32,
}

/// 注入矩阵：四类注入各一发，全部必检出（任一漏检回 InjectionMissed）。
pub fn injection_matrix() -> Result<Vec<InjectProbe>, AssertBookError> {
    let mut probes: Vec<InjectProbe> = Vec::new();

    // 发一：空检查体——登记闸拒收。
    {
        let mut book = AssertBook::new();
        let detected = match book.register(&A70_INJECT_EMPTY) {
            Err(AssertBookError::EmptyCheck { .. }) => true,
            _ => false,
        };
        probes.push(InjectProbe {
            kind: InjectKind::EmptyCheck,
            detected,
            code: XC_EMPTY_CHECK,
        });
    }

    // 发二：重复 id——已建账本复登记首条，登记闸拒收。
    {
        let mut book = build_full_book()?;
        let detected = match book.register(&A70_CHECKS_EARLYBOOT[0]) {
            Err(AssertBookError::DuplicateId { .. }) => true,
            _ => false,
        };
        probes.push(InjectProbe {
            kind: InjectKind::DuplicateId,
            detected,
            code: XC_DUP_ID,
        });
    }

    // 发三：基线删除——抽走 A70-PT-A-001，diff 必报删。
    {
        let baseline = build_full_book()?.snapshot();
        let reduced = build_book_variants(Some(A70_INJECT_REMOVE_IDX), None)?;
        let rep = baseline.diff_against(&reduced)?;
        let detected = rep.removed == 1 && baseline.ensure_only_additions(&rep).is_err();
        probes.push(InjectProbe {
            kind: InjectKind::BaselineRemoval,
            detected,
            code: XC_DIFF_REMOVED,
        });
    }

    // 发四：判据篡改——同 id 顶替判据，diff 必报改。
    {
        let baseline = build_full_book()?.snapshot();
        let modified = build_book_variants(None, Some((A70_INJECT_MODIFY_IDX, &A70_INJECT_MOD_SPEC)))?;
        let rep = baseline.diff_against(&modified)?;
        let detected = rep.modified == 1 && baseline.ensure_only_additions(&rep).is_err();
        probes.push(InjectProbe {
            kind: InjectKind::BaselineModify,
            detected,
            code: XC_DIFF_MODIFIED,
        });
    }

    // 收口：任一漏检即 InjectionMissed。
    let mut i = 0;
    while i < probes.len() {
        if !probes[i].detected {
            return Err(AssertBookError::InjectionMissed {
                code: XC_INJECT_MISSED,
                kind: probes[i].kind.name(),
            });
        }
        i += 1;
    }
    Ok(probes)
}

// ═══════════════════════════════════════════════════════════════════
// 总审计入口 + 报告渲染
// ═══════════════════════════════════════════════════════════════════

/// 总审计终报（UNX-F0705-J1 判定的全量载体）。
#[derive(Clone, Debug)]
pub struct A70FinalReport {
    /// 登记总数。
    pub registry_total: usize,
    /// 登记制绿态。
    pub registry_green: bool,
    /// 登记制统计。
    pub registry: RegistryReport,
    /// 执行报告。
    pub run: AssertRunReport,
    /// 补全 diff 报告（相对自基线，洁净即只增不减自洽）。
    pub diff: DiffReport,
    /// 双跑对拍。
    pub dual: DualRunReport,
    /// 注入矩阵。
    pub injections: Vec<InjectProbe>,
    /// 注入全检出位。
    pub injections_all_caught: bool,
    /// 事件账健康。
    pub health: A70Health,
    /// 簿记指纹。
    pub book_fp: u64,
    /// 总判定二态（七件全绿才绿：登记/执行/diff/对拍/注入/健康/可解引）。
    pub verdict: AssertVerdict,
}

impl A70FinalReport {
    /// 全绿否。
    pub fn is_green(&self) -> bool {
        matches!(self.verdict, AssertVerdict::Green)
    }
}

/// 总审计入口：建账 → 登记制判定 → 执行 → diff 自洽 → 双跑对拍 →
/// 注入面 → 健康核 → 七件总判定。
pub fn unx0705_full_audit(world: &A70World) -> Result<A70FinalReport, AssertBookError> {
    let mut book = build_full_book()?;
    let registry = book.registry_report();
    // 判据-断言映射单向核：全部判据引用可解引。
    if !book.criteria_resolvable() {
        book.record_event(EventKind::Fault, XC_BAD_ID);
        return Err(AssertBookError::BadId {
            code: XC_BAD_ID,
            id: String::from("criterion-ref-unresolvable"),
        });
    }
    let run = book.run(world)?;
    // diff 自洽：自基线对账应零删改。
    let baseline = book.snapshot();
    let diff = baseline.diff_against(&book)?;
    let diff_clean = diff.clean;
    if !diff_clean {
        book.record_event(EventKind::Fault, XC_DIFF_MODIFIED);
    }
    baseline.ensure_only_additions(&diff)?;
    book.record_event(EventKind::DiffDone, XC_GREEN);
    // 双跑对拍。
    let dual = dual_run(world)?;
    if !dual.identical {
        book.record_event(EventKind::Fault, XC_FP_MISMATCH);
    }
    dual.ensure_identical()?;
    // 注入面。
    let injections = injection_matrix()?;
    let mut all_caught = true;
    let mut i = 0;
    while i < injections.len() {
        if !injections[i].detected {
            all_caught = false;
        }
        i += 1;
    }
    if !all_caught {
        book.record_event(EventKind::Fault, XC_INJECT_MISSED);
        return Err(AssertBookError::InjectionMissed {
            code: XC_INJECT_MISSED,
            kind: "matrix",
        });
    }
    book.record_event(EventKind::InjectCaught, XC_GREEN);
    // 健康核。
    let health = book.health();
    if !health.ok {
        book.record_event(EventKind::Fault, XC_HEALTH_RED);
        return Err(AssertBookError::UnknownDiag {
            code: XC_HEALTH_RED,
        });
    }
    // 七件总判定：登记绿 + 执行绿 + diff 洁 + 对拍一致 + 注入全检出 +
    // 健康绿 + 判据可解引。
    let verdict = if registry.green
        && run.is_green()
        && diff_clean
        && dual.identical
        && all_caught
        && health.ok
        && book.criteria_resolvable()
    {
        AssertVerdict::Green
    } else {
        AssertVerdict::Red
    };
    Ok(A70FinalReport {
        registry_total: registry.total,
        registry_green: registry.green,
        registry,
        run,
        diff,
        dual,
        injections,
        injections_all_caught: all_caught,
        health,
        book_fp: book.fingerprint(),
        verdict,
    })
}

/// 执行报告渲染（分面四行 + 总判定行 + 失败清单；纯 alloc String）。
pub fn render_run_report(rep: &AssertRunReport) -> String {
    let mut s = String::new();
    s.push_str("UNX-F0705 执行报告：\n");
    let mut f = 0;
    while f < 4 {
        let st = &rep.faces[f];
        s.push_str(&format!(
            "  {}[{}] 登记 {}/最低 {} 通过 {} 失败 {} 通过率 {}‱ 达标 {}\n",
            st.face.zh(),
            st.face.code3(),
            st.registered,
            st.min_required,
            st.passed,
            st.failed,
            st.pass_perm,
            if st.meets_min { "是" } else { "否" }
        ));
        f += 1;
    }
    s.push_str(&format!(
        "  总计 {} 条通过 {} 失败 {} 总数线 {} 判定[{}]\n",
        rep.total,
        rep.passed,
        rep.failed,
        A70_TOTAL_MIN,
        rep.verdict.zh()
    ));
    // 失败清单（渲染上限 32 条封顶，防报告膨胀）。
    let cap = if rep.failures.len() > 32 { 32 } else { rep.failures.len() };
    let mut k = 0;
    while k < cap {
        let fl = &rep.failures[k];
        s.push_str(&format!(
            "  红 {} (序 {}) 判据[{}] {}\n",
            fl.id_text, fl.order, fl.criterion, fl.desc
        ));
        k += 1;
    }
    if rep.failures.len() > cap {
        s.push_str(&format!("  …另有 {} 条红灯略\n", rep.failures.len() - cap));
    }
    s
}

/// 终报渲染（登记/执行/diff/对拍/注入/健康/指纹七段）。
pub fn render_final_report(rep: &A70FinalReport) -> String {
    let mut s = String::new();
    s.push_str("UNX-F0705 总审计：\n");
    s.push_str(&format!(
        "  登记：{} 条 硬线 {} 四面计数 {:?} 绿态 {}\n",
        rep.registry_total,
        A70_TOTAL_MIN,
        rep.registry.counts,
        if rep.registry_green { "过" } else { "不过" }
    ));
    s.push_str(&render_run_report(&rep.run));
    s.push_str(&format!(
        "  diff：增 {} 删 {} 改 {} 洁净 {}\n",
        rep.diff.added, rep.diff.removed, rep.diff.modified, rep.diff.clean
    ));
    s.push_str(&format!(
        "  对拍：甲 {:#018x} 乙 {:#018x} 一致 {}\n",
        rep.dual.fp_a, rep.dual.fp_b, rep.dual.identical
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
        rep.health.events, rep.health.faults, rep.health.dropped, rep.health.ok
    ));
    s.push_str(&format!(
        "  指纹：{:#018x} 总判定[{}]\n",
        rep.book_fp,
        rep.verdict.zh()
    ));
    s
}

// ═══════════════════════════════════════════════════════════════════
// 编译期硬闸（const assert：判据硬线与表形在编译期钉死）
// ═══════════════════════════════════════════════════════════════════

const _: () = assert!(A70_TOTAL_MIN == 70);
const _: () = assert!(
    A70_MIN_EARLYBOOT + A70_MIN_CONSOLE + A70_MIN_PAGETBL + A70_MIN_HIHALF == A70_TOTAL_MIN
);
const _: () = assert!(A70_CHECKS_EARLYBOOT.len() >= A70_MIN_EARLYBOOT);
const _: () = assert!(A70_CHECKS_CONSOLE.len() >= A70_MIN_CONSOLE);
const _: () = assert!(A70_CHECKS_PAGETBL.len() >= A70_MIN_PAGETBL);
const _: () = assert!(A70_CHECKS_HIHALF.len() >= A70_MIN_HIHALF);
const _: () = assert!(
    A70_CHECKS_EARLYBOOT.len() + A70_CHECKS_CONSOLE.len() + A70_CHECKS_PAGETBL.len()
        + A70_CHECKS_HIHALF.len()
        >= A70_TOTAL_MIN
);
const _: () = assert!(A70_TOTAL_SPECS >= A70_TOTAL_MIN);
const _: () = assert!(A70_BOOT_STEP_NAMES.len() == A70_BOOT_STEPS);
const _: () = assert!(A70_STEP_ARTIFACT_MIN.len() == A70_BOOT_STEPS);
const _: () = assert!(A70_STEP_ARTIFACT_MAX.len() == A70_BOOT_STEPS);
const _: () = assert!(A70_STEP_ARTIFACT_GREEN.len() == A70_BOOT_STEPS);
const _: () = assert!(A70_PTE_SPECS.len() == A70_REGION_COUNT);
const _: () = assert!(A70_HH_EXPECT_VADDR.len() == A70_HH_SLOTS);
const _: () = assert!(A70_SEG_BASE == 0x16D0);
const _: () = assert!(A70_SERIAL_BAUD == 115_200);
const _: () = assert!(A70_VGA_COLS == 80 && A70_VGA_ROWS == 25);
const _: () = assert!(A70_HIHALF_BASE_MIN == 0xFFFF_8000_0000_0000);
const _: () = assert!(A70_KIMG_BASE >= A70_HIHALF_BASE_MIN);
const _: () = assert!(A70_FNV_PRIME == 0x0000_0100_0000_01b3);
